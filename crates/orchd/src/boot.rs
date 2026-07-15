//! Testable daemon boot core (spec §5, mirrors `bpa_sessiond::boot` minus PTY concerns).
//! `main.rs` is a thin wrapper over [`run`] that adds process concerns (tracing init, the
//! single-instance flock, SIGTERM/SIGINT wiring); `run` itself only binds the socket, opens the
//! DB (degrading honestly on failure), ensures the global ruleset, drives
//! [`socket_server::serve`] until told to stop, and drains.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{watch, Mutex};

use bpa_daemon_core::singleton::{assert_socket_path_len, set_socket_mode};

use crate::connectors::accounts::ConnectorsState;
use crate::persistence::Db;
use crate::socket_server::{serve, ServerDeps};

/// On-disk file name of the daemon's SQLite database, under `{app-support}/`.
const DB_FILE_NAME: &str = "orchd.db";

/// Template content written to a fresh `rules/global.md` (spec §5.2: `# Глобальные правила\n`).
const GLOBAL_RULESET_TEMPLATE: &str = "# Глобальные правила\n";

/// Resolve `~/Library/Application Support/ai.builderpro.desktop` (spec §5: durable state — DB,
/// settings, logs — lives here, never next to the short socket path). Thin wrapper over
/// `bpa_daemon_core::dirs::app_support_dir` — body unchanged.
pub(crate) fn app_support_dir() -> PathBuf {
    bpa_daemon_core::dirs::app_support_dir()
}

/// Test-support wrapper exposing [`app_support_dir`] to integration tests (mirrors
/// `bpa_sessiond::app_support_dir_for_test`, D6: proves a test's `$HOME` isolation actually
/// redirects the daemon's on-disk DB/rules path under its own tempdir) without widening the
/// crate's real boot entry point beyond [`run`]. Not part of the daemon boot contract.
#[doc(hidden)]
pub fn app_support_dir_for_test() -> PathBuf {
    app_support_dir()
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Bind a fresh [`UnixListener`] at `socket`, cleaning up a stale socket file left behind by a
/// crashed daemon (spec §5, mirrors `bpa_sessiond::boot::bind_fresh` byte-for-byte). The caller
/// is expected to already hold the single-instance flock, so any pre-existing file at `socket`
/// is necessarily a stale artifact rather than a live peer.
async fn bind_fresh(socket: &Path) -> std::io::Result<UnixListener> {
    assert_socket_path_len(socket)?;

    if socket.exists() {
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
            let _ = std::fs::remove_file(socket);
            UnixListener::bind(socket)?
        }
        Err(e) => return Err(e),
    };
    set_socket_mode(socket)?;
    Ok(listener)
}

/// Open the daemon's durable SQLite DB, degrading honestly to an in-memory DB on failure (spec
/// §5: mirrors `bpa_sessiond::boot::open_db_degrading` byte-for-byte, just a different on-disk
/// file name). Only a failure of the in-memory fallback itself is unrecoverable.
fn open_db_degrading(app_support: &Path) -> Db {
    let _ = std::fs::create_dir_all(app_support);
    let db_path = app_support.join(DB_FILE_NAME);
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
                    tracing::error!(error = %e2, "in-memory DB fallback also failed");
                    panic!("no usable database backend: {e2}");
                }
            }
        }
    }
}

/// Ensure the GLOBAL ruleset row + `{app-support}/rules/global.md` exist (spec §5.2: "ensured at
/// every orchd boot (idempotent)"). Writes the template file only if missing (an operator's
/// hand-edits to an existing file are never clobbered by a later boot), then `INSERT OR IGNORE`s
/// the DB row — the partial unique index `ruleset_single_global` makes a second/Nth boot's insert
/// a silent no-op rather than a duplicate row or an error. Best-effort: a failure here is logged
/// and must never fail the whole boot (mirrors the DB's own honest-degradation stance) — rule
/// content is supplementary state, not a boot-blocking dependency. Re-seated (T8) onto
/// `ruleset_files::write_atomic`/`sha256_hex` so the global.md write path and hashing are the SAME
/// code every other ruleset file write/hash in this crate uses — no duplicate hashing impl here.
fn ensure_global_ruleset(db: &Db, app_support: &Path) {
    if let Err(e) = ensure_global_ruleset_inner(db, app_support) {
        tracing::error!(error = %e, "failed to ensure global ruleset");
    }
}

fn ensure_global_ruleset_inner(db: &Db, app_support: &Path) -> std::io::Result<()> {
    let rules_dir = app_support.join("rules");
    std::fs::create_dir_all(&rules_dir)?;
    let md_path = rules_dir.join("global.md");
    if !md_path.exists() {
        // write_atomic also creates parent dirs itself, but rules_dir must exist unconditionally
        // above so the `md_path.exists()` check just performed is meaningful even on a fresh
        // app-support dir.
        crate::ruleset_files::write_atomic(&md_path, GLOBAL_RULESET_TEMPLATE)?;
    }
    let content = std::fs::read_to_string(&md_path)?;
    let md_hash = crate::ruleset_files::sha256_hex(&content);
    let md_path_str = md_path.to_string_lossy().into_owned();
    let now = now_ms();
    let id = uuid::Uuid::new_v4().to_string();

    db.conn()
        .execute(
            "INSERT OR IGNORE INTO ruleset
                (id, scope, project_id, md_path, md_hash, policy, created_at, updated_at)
             VALUES (?1, 'global', NULL, ?2, ?3, '{}', ?4, ?4)",
            rusqlite::params![id, md_path_str, md_hash, now],
        )
        .map_err(|e| std::io::Error::other(e.to_string()))?;
    Ok(())
}

/// Boot-reconcile of interrupted research runs (S-IDEA spec D11): flips every non-terminal
/// (`pending`/`running`) `research_run` row to `failed{interrupted}` — the AUTHORITATIVE
/// backstop for the async run driver's detached `tokio::spawn` task (a later task, T4), which is
/// NOT tracked by the shutdown drain's `JoinSet` and so can be lost outright on a crash/restart/
/// drain mid-run. Best-effort: logs the affected count on success, logs (never panics) on
/// failure — mirrors [`ensure_global_ruleset`]'s honest-degradation stance (state ensured at
/// every boot, but never boot-blocking).
fn reconcile_interrupted(db: &Db) {
    match db.reconcile_interrupted_research_runs() {
        Ok(0) => tracing::info!("boot-reconcile: no interrupted research runs found"),
        Ok(count) => tracing::warn!(
            count,
            "boot-reconcile: flipped interrupted research runs to failed"
        ),
        Err(e) => tracing::error!(error = %e, "failed to reconcile interrupted research runs"),
    }
}

/// Boot core: bind the listener, open the DB, ensure the global ruleset, run [`serve`] until
/// `shutdown` flips to `true` (or the listener errors), then drain. Returns once fully drained.
///
/// `socket` is bound as-is (no path resolution / dir creation here — the caller, `main.rs` in
/// production or a test harness here, owns `ensure_socket_dir` + resolving the path so this
/// function stays pure and drivable against a bare temp-dir socket in tests).
///
/// `shutdown_tx`/`shutdown_rx` are the two halves of ONE `watch::channel` (mirrors
/// `bpa_sessiond::boot::run`): the caller owns construction so it can also wire its own triggers
/// (e.g. `main.rs`'s SIGTERM handler) onto the sender; `shutdown_tx` is cloned into
/// [`ServerDeps`] so the `OrchdRequest::OrchdShutdown` dispatch arm can flip the SAME watch a
/// GUI-initiated shutdown and an operator SIGTERM converge on one graceful-exit path.
pub async fn run(
    socket: PathBuf,
    shutdown_tx: watch::Sender<bool>,
    shutdown_rx: watch::Receiver<bool>,
) -> std::io::Result<()> {
    let listener = bind_fresh(&socket).await?;

    let app_support = app_support_dir();
    let db = open_db_degrading(&app_support);
    ensure_global_ruleset(&db, &app_support);
    reconcile_interrupted(&db);
    let db = Arc::new(Mutex::new(db));

    // S-EXT connector OAuth-account layer (spec §5/§7, task T13a): lives for the daemon's whole
    // lifetime alongside `db` (its in-flight `begin_oauth` pending-PKCE map must survive across
    // requests). v1 boots with an EMPTY OAuth provider registry — no real IdP credentials ship
    // with this app (spec §10: wiring prowl.chat/X/etc is an owner step in the UI, never
    // fabricated here); `ConnectorBeginOAuth` for an unregistered `provider` honestly fails with a
    // typed error until an owner (or a later config-file-backed registry, spec D14 Phase 3) calls
    // `ConnectorsState::register_oauth_provider`. The api-key and generic-rest connector paths
    // need no provider registry at all and work from a fresh boot.
    let connectors = Arc::new(ConnectorsState::new());

    let deps = Arc::new(ServerDeps::new(
        db.clone(),
        connectors,
        env!("CARGO_PKG_VERSION").to_string(),
        shutdown_tx,
    ));

    tracing::info!(socket = %socket.display(), "orchd serving");
    let serve_res = serve(listener, deps, shutdown_rx).await;

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
    tracing::info!("orchd drained; exiting");
    serve_res
}

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn ensure_global_ruleset_writes_file_and_row_idempotently() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_in_memory().unwrap();

        ensure_global_ruleset(&db, dir.path());
        ensure_global_ruleset(&db, dir.path());

        let md_path = dir.path().join("rules/global.md");
        assert_eq!(
            std::fs::read_to_string(&md_path).unwrap(),
            GLOBAL_RULESET_TEMPLATE
        );

        let count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM ruleset WHERE scope = 'global'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            count, 1,
            "second ensure call must not duplicate the global row"
        );
    }

    /// S-IDEA spec D11: boot-reconcile must run cleanly (no panic, zero affected rows) on a
    /// fresh DB that has never had a research run — mirrors
    /// `ensure_global_ruleset_writes_file_and_row_idempotently`'s "call the boot wrapper
    /// directly, assert the DB state it leaves behind" shape.
    #[test]
    fn reconcile_interrupted_research_runs_on_fresh_db_is_a_noop() {
        let db = Db::open_in_memory().unwrap();

        reconcile_interrupted(&db); // must not panic

        let count = db.reconcile_interrupted_research_runs().unwrap();
        assert_eq!(
            count, 0,
            "a fresh db has no interrupted research runs to reconcile"
        );
    }

    /// Deferred from T5 (task-8 brief): a later boot must never clobber an operator's hand-edit
    /// of `global.md`. The FIRST `ensure_global_ruleset` call in
    /// `ensure_global_ruleset_writes_file_and_row_idempotently` above only proves the TEMPLATE
    /// content survives repeated calls — this proves ARBITRARY hand-edited content survives too,
    /// which is the actual owner-facing guarantee (spec §5.2 "Writes the template file only if
    /// missing").
    #[test]
    fn ensure_global_ruleset_does_not_clobber_a_hand_edited_file() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open_in_memory().unwrap();

        // First boot creates the template file + DB row.
        ensure_global_ruleset(&db, dir.path());

        // Operator hand-edits the file (e.g. directly in an editor, or via a repo-tracked copy).
        let md_path = dir.path().join("rules/global.md");
        let hand_edited = "# Мои собственные правила\n\nникогда не перезаписывай меня\n";
        std::fs::write(&md_path, hand_edited).unwrap();

        // A later boot (app relaunch, daemon restart) must NOT overwrite the hand-edit.
        ensure_global_ruleset(&db, dir.path());

        assert_eq!(
            std::fs::read_to_string(&md_path).unwrap(),
            hand_edited,
            "ensure_global_ruleset must never overwrite an existing file, hand-edited or not"
        );
    }
}
