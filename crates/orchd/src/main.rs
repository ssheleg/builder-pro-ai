//! bpa-orchd entrypoint (spec §5). launchd invokes: `bpa-orchd --socket <path>`.
//!
//! This binary is a thin process-concerns wrapper over [`bpa_orchd::run`]: it parses args,
//! initializes tracing, acquires the single-instance flock, resolves the socket dir/path, wires
//! SIGTERM/SIGINT into the shutdown watch, and hands off to the testable boot core. The daemon
//! runs in the **foreground** the whole time — launchd supervises it directly (mirrors
//! `bpa-sessiond`'s exact process-concerns shape); this process must never double-fork or
//! `setsid`.

use std::path::PathBuf;
use std::process::ExitCode;

use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::watch;

use bpa_daemon_core::singleton::{
    acquire_single_instance_lock, ensure_socket_dir, resolve_socket_path,
};

/// CLI args (`--socket <path>`, mirrors `bpa-sessiond`'s launchd `ProgramArguments`).
struct Args {
    socket: Option<PathBuf>,
}

/// Parse `--socket <path>`; unknown flags are logged and ignored (launchd passes a fixed,
/// trusted argument set, so being permissive here is safe and avoids a boot-time hard-fail on
/// an unexpected extra flag).
fn parse_args() -> Args {
    let mut socket = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--socket" => socket = args.next().map(PathBuf::from),
            "--version" => {
                println!("bpa-orchd {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            other => {
                eprintln!("bpa-orchd: ignoring unknown argument: {other}");
            }
        }
    }
    Args { socket }
}

/// Initialize structured logging under `{APP_SUPPORT}/logs/orchd.tracing.log` (no secret values
/// are logged — only paths, ids, and lifecycle events). Falls back to `EnvFilter`'s default
/// (`info`) when `RUST_LOG` is unset.
fn init_tracing() {
    if let Err(e) = bpa_daemon_core::logging::init_tracing("orchd.tracing.log") {
        panic!("bpa-orchd: failed to init tracing: {e}");
    }
}

/// Install SIGTERM (and SIGINT, for dev `Ctrl-C`) handling: on first signal, flip the shutdown
/// watch so `serve` returns and `run` drains. A second signal is ignored (drain is already in
/// flight) rather than forcing an abrupt exit.
fn spawn_signal_watcher(shutdown_tx: watch::Sender<bool>) -> std::io::Result<()> {
    let mut sigterm = signal(SignalKind::terminate())?;
    let mut sigint = signal(SignalKind::interrupt())?;
    tokio::spawn(async move {
        tokio::select! {
            _ = sigterm.recv() => tracing::info!("SIGTERM received; draining"),
            _ = sigint.recv() => tracing::info!("SIGINT received; draining"),
        }
        let _ = shutdown_tx.send(true);
    });
    Ok(())
}

#[tokio::main]
async fn main() -> ExitCode {
    let args = parse_args();
    init_tracing();

    tracing::info!(
        version = env!("CARGO_PKG_VERSION"),
        proto_min = bpa_orchd::protocol::ORCHD_DAEMON_MIN_VERSION,
        proto_max = bpa_orchd::protocol::ORCHD_DAEMON_MAX_VERSION,
        "bpa-orchd starting"
    );

    // The socket dir (and therefore the lockfile's parent) must exist before we can even open
    // the lockfile: ensure it first, then take the flock inside it.
    if let Err(e) = ensure_socket_dir() {
        tracing::error!(error = %e, "failed to ensure socket dir");
        return ExitCode::FAILURE;
    }

    // Single-instance flock: held for the whole process lifetime via this binding. A second
    // daemon that cannot take the lock exits cleanly — this is an idempotent `launchctl
    // kickstart`, not an error.
    let _lock = match acquire_single_instance_lock("orchd.lock") {
        Ok(guard) => guard,
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
            tracing::info!("another orchd already holds the single-instance lock; exiting");
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            tracing::error!(error = %e, "failed to acquire single-instance lock");
            return ExitCode::FAILURE;
        }
    };

    let socket = args
        .socket
        .unwrap_or_else(|| resolve_socket_path("orchd.sock"));

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    // `run` also needs a sender clone for `ServerDeps` (so `OrchdRequest::OrchdShutdown` can flip
    // the same watch this SIGTERM/SIGINT handler flips); the signal watcher takes the original.
    if let Err(e) = spawn_signal_watcher(shutdown_tx.clone()) {
        tracing::error!(error = %e, "failed to install signal handlers");
        return ExitCode::FAILURE;
    }

    match bpa_orchd::run(socket, shutdown_tx, shutdown_rx).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!(error = %e, "orchd run failed");
            ExitCode::FAILURE
        }
    }
}
