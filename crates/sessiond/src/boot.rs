//! Testable daemon boot core (spec §8.1-8.3, §13). `main.rs` is a thin wrapper over [`run`]
//! that adds process concerns (tracing init, the single-instance flock, SIGTERM/SIGINT wiring);
//! `run` itself only binds the socket, wires the in-process dependency graph, drives
//! [`socket_server::serve`] until told to stop, and drains.
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{watch, Mutex};

use crate::attach::AttachRegistry;
use crate::persistence::Db;
use crate::pty_supervisor::Supervisor;
use crate::singleton::{assert_socket_path_len, set_socket_mode};
use crate::socket_server::{serve, ServerDeps};

/// Resolve `~/Library/Application Support/ai.builderpro.desktop` (spec §8.1: durable state —
/// DB, settings, logs — lives here, never next to the short socket path).
pub(crate) fn app_support_dir() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    home.join("Library/Application Support/ai.builderpro.desktop")
}

/// Bind a fresh [`UnixListener`] at `socket`, cleaning up a stale socket file left behind by a
/// crashed daemon (spec §8.2). The caller is expected to already hold the single-instance flock,
/// so any pre-existing file at `socket` is necessarily a stale artifact rather than a live peer:
/// we still connect-probe it defensively (a `WouldBlock`-refused second daemon never reaches this
/// code path, but a foreign/unexpected process holding the path is not impossible) before
/// unlinking, and unlink unconditionally on any bind failure that looks like "address in use".
async fn bind_fresh(socket: &Path) -> std::io::Result<UnixListener> {
    assert_socket_path_len(socket)?;

    if socket.exists() {
        // Best-effort liveness probe; the result only informs the log message, not the
        // decision to unlink — we hold the flock, so we own cleanup of this path either way.
        match UnixStream::connect(socket).await {
            Ok(_live) => {
                tracing::warn!(
                    socket = %socket.display(),
                    "socket path answered a connect while we held the single-instance lock; unlinking anyway"
                );
            }
            Err(e) => {
                tracing::info!(socket = %socket.display(), error = %e, "removing stale socket file");
            }
        }
        if let Err(e) = std::fs::remove_file(socket) {
            if e.kind() != std::io::ErrorKind::NotFound {
                return Err(e);
            }
        }
    }

    let listener = match UnixListener::bind(socket) {
        Ok(l) => l,
        Err(e) if e.kind() == std::io::ErrorKind::AddrInUse => {
            // Race: something recreated the path between our unlink and bind. One more
            // unlink+bind attempt; if it still fails, surface the error rather than loop.
            let _ = std::fs::remove_file(socket);
            UnixListener::bind(socket)?
        }
        Err(e) => return Err(e),
    };
    set_socket_mode(socket)?;
    Ok(listener)
}

/// Open the daemon's durable SQLite DB, degrading honestly to an in-memory DB on failure
/// (spec §11: the in-memory session state is the Layer-1 source of truth; persistence is
/// best-effort). Only a failure of the in-memory fallback itself is unrecoverable.
fn open_db_degrading(app_support: &Path) -> Db {
    let _ = std::fs::create_dir_all(app_support);
    let db_path = app_support.join("bpa.db");
    match Db::open(&db_path) {
        Ok(db) => db,
        Err(e) => {
            tracing::error!(
                error = %e,
                path = %db_path.display(),
                "DB open failed; continuing in degraded (in-memory) mode"
            );
            match Db::open_in_memory() {
                Ok(db) => db,
                Err(e2) => {
                    // Even the in-memory fallback failing means SQLite itself is unusable in
                    // this process; there is no honest degraded path left to fall back to.
                    tracing::error!(error = %e2, "in-memory DB fallback also failed");
                    panic!("no usable database backend: {e2}");
                }
            }
        }
    }
}

/// Cold-rehydrate every persisted session into `supervisor` as an INACTIVE, PTY-less, replay-only
/// entry (Pv2 §7 / BL-7: "your records and scrollback reappear" after a daemon restart) — BEFORE
/// `serve()` starts accepting connections, so the very first `AttachSession` a reconnecting client
/// sends can already succeed via [`crate::pty_supervisor::Supervisor::rehydrate_inactive`] and the
/// existing `AttachSession → Push::Replay` path (attach.rs), no new wire request needed.
///
/// Best-effort at both levels: a failure to list persisted sessions at all is logged and this
/// function simply rehydrates nothing (the daemon still boots — spec §11, persistence is
/// best-effort, never a boot-blocking dependency); a failure to rehydrate ONE session (or to load
/// its scrollback, which best-effort-defaults to empty) is logged and skipped — it must never
/// abort the loop or the boot.
async fn cold_rehydrate_sessions(db: &Arc<Mutex<Db>>, supervisor: &Arc<Supervisor>) {
    let db = db.lock().await;
    match db.list_sessions() {
        Ok(sessions) => {
            for meta in sessions {
                let sb = db.load_scrollback(&meta.id).unwrap_or_default();
                let session_id = meta.id.clone();
                if let Err(e) = supervisor.rehydrate_inactive(meta, sb) {
                    tracing::warn!(session_id = %session_id, error = %e, "cold-rehydrate skipped");
                }
            }
        }
        Err(e) => {
            tracing::warn!(error = %e, "could not list persisted sessions for cold-rehydrate");
        }
    }
}

/// Boot core: bind the listener, wire the dependency graph, run [`serve`] until `shutdown`
/// flips to `true` (or the listener errors), then drain. Returns once fully drained.
///
/// `socket` is bound as-is (no path resolution / dir creation here — the caller, `main.rs` in
/// production or a test harness here, owns `ensure_socket_dir` + resolving the path so this
/// function stays pure and drivable against a bare temp-dir socket in tests).
///
/// `shutdown_tx` and `shutdown_rx` are the two halves of ONE `watch::channel` (the caller owns
/// construction so it can also wire its own triggers, e.g. `main.rs`'s SIGTERM handler, onto the
/// sender). `shutdown_rx` drives [`serve`]'s accept loop exactly as before; `shutdown_tx` is cloned
/// into [`ServerDeps`] so the `Request::DaemonShutdown` dispatch arm can flip the SAME watch a
/// GUI-initiated shutdown and an operator SIGTERM converge on one graceful-exit path (Pv2 §6.1).
pub async fn run(
    socket: PathBuf,
    shutdown_tx: watch::Sender<bool>,
    shutdown_rx: watch::Receiver<bool>,
) -> std::io::Result<()> {
    let listener = bind_fresh(&socket).await?;

    let app_support = app_support_dir();
    let db = Arc::new(Mutex::new(open_db_degrading(&app_support)));

    let supervisor = Arc::new(Supervisor::new());
    cold_rehydrate_sessions(&db, &supervisor).await;
    let attach = Arc::new(AttachRegistry::new(supervisor.clone()));
    // Per-session shell-integration assets (ZDOTDIR / bpa-bash.sh) live under a runtime-root
    // subdirectory next to the socket, keeping them on the same short-path filesystem as the
    // socket itself rather than under APP_SUPPORT.
    let runtime_root = socket
        .parent()
        .map(|p| p.join("runtime"))
        .unwrap_or_else(|| app_support.join("runtime"));
    let _ = std::fs::create_dir_all(&runtime_root);

    let deps = Arc::new(ServerDeps::new(
        supervisor.clone(),
        db.clone(),
        attach.clone(),
        env!("CARGO_PKG_VERSION").to_string(),
        runtime_root,
        shutdown_tx,
    ));

    tracing::info!(socket = %socket.display(), "sessiond serving");
    let serve_res = serve(listener, deps, shutdown_rx).await;

    // Drain (spec §8.3 / §13 DaemonShutdown semantics). `serve` already tears down attach
    // forwarders on its own exit path; calling `detach_all` again here is a no-op-safe
    // belt-and-braces in case `run` is ever driven with a `serve` that changes that contract.
    attach.detach_all();
    supervisor.shutdown_all(); // killpg each session: SIGTERM -> grace -> SIGKILL
    {
        let db = db.lock().await;
        if let Err(e) = db.checkpoint() {
            tracing::warn!(error = %e, "best-effort DB checkpoint on shutdown failed");
        }
    }
    if let Err(e) = std::fs::remove_file(&socket) {
        if e.kind() != std::io::ErrorKind::NotFound {
            tracing::warn!(error = %e, socket = %socket.display(), "failed to unlink socket on shutdown");
        }
    }
    tracing::info!("sessiond drained; exiting");
    serve_res
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_support_dir_is_under_home() {
        let dir = app_support_dir();
        assert!(dir.ends_with("Library/Application Support/ai.builderpro.desktop"));
    }

    #[tokio::test]
    async fn bind_fresh_rejects_overlong_path() {
        let long = PathBuf::from(format!("/tmp/{}/d.sock", "x".repeat(120)));
        let err = bind_fresh(&long).await.unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }

    #[tokio::test]
    async fn bind_fresh_removes_stale_regular_file_and_binds() {
        let dir = tempfile::tempdir().unwrap();
        let sock = dir.path().join("d.sock");
        std::fs::write(&sock, b"stale").unwrap();
        let listener = bind_fresh(&sock).await.expect("rebind over stale file");
        drop(listener);
        let md = std::fs::metadata(&sock).unwrap();
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(md.permissions().mode() & 0o777, 0o600);
    }
}
