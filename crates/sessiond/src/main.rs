//! bpa-sessiond entrypoint (spec §8.3). launchd invokes: `bpa-sessiond --socket <path>`.
//!
//! This binary is a thin process-concerns wrapper over [`bpa_sessiond::run`]: it parses args,
//! initializes tracing, acquires the single-instance flock, resolves the socket dir/path, wires
//! SIGTERM/SIGINT into the shutdown watch, and hands off to the testable boot core. The daemon
//! runs in the **foreground** the whole time — launchd supervises it directly (spec §8.3); this
//! process must never double-fork or `setsid`.

use std::path::PathBuf;
use std::process::ExitCode;

use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::watch;

use bpa_sessiond::singleton::{acquire_single_instance_lock, ensure_socket_dir, resolve_socket_path};

/// CLI args (spec §8.3 launchd ProgramArguments: `--socket <path>`).
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
                println!("bpa-sessiond {}", env!("CARGO_PKG_VERSION"));
                std::process::exit(0);
            }
            other => {
                eprintln!("bpa-sessiond: ignoring unknown argument: {other}");
            }
        }
    }
    Args { socket }
}

/// Initialize structured logging under `{APP_SUPPORT}/logs/sessiond.tracing.log` (spec §13,
/// §16: no secret values are logged — only paths, session ids, and lifecycle events). Falls
/// back to `EnvFilter`'s default (`info`) when `RUST_LOG` is unset.
fn init_tracing() {
    use tracing_subscriber::{fmt, prelude::*, EnvFilter};

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    let log_dir = home.join("Library/Application Support/ai.builderpro.desktop/logs");
    if let Err(e) = std::fs::create_dir_all(&log_dir) {
        eprintln!("bpa-sessiond: failed to create log dir {}: {e}", log_dir.display());
    } else {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&log_dir, std::fs::Permissions::from_mode(0o700));
        }
    }

    let file_appender = tracing_appender::rolling::never(&log_dir, "sessiond.tracing.log");
    // `serve`/`run` also emit to stderr indirectly via launchd's StandardOutPath/StandardErrorPath
    // capture (spec §8.3 plist); the file layer is the daemon's own structured log.
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_writer(file_appender).with_ansi(false))
        .init();
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
        proto = bpa_sessiond::protocol::PROTO_VERSION,
        "bpa-sessiond starting"
    );

    // The socket dir (and therefore the lockfile's parent) must exist before we can even open
    // the lockfile (spec §8.1-8.2): ensure it first, then take the flock inside it.
    if let Err(e) = ensure_socket_dir() {
        tracing::error!(error = %e, "failed to ensure socket dir");
        return ExitCode::FAILURE;
    }

    // Single-instance flock (spec §8.2): held for the whole process lifetime via this binding.
    // A second daemon that cannot take the lock exits cleanly — this is an idempotent
    // `launchctl kickstart`, not an error (spec §8.3).
    let _lock = match acquire_single_instance_lock() {
        Ok(guard) => guard,
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
            tracing::info!("another sessiond already holds the single-instance lock; exiting");
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            tracing::error!(error = %e, "failed to acquire single-instance lock");
            return ExitCode::FAILURE;
        }
    };

    let socket = args.socket.unwrap_or_else(resolve_socket_path);

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    if let Err(e) = spawn_signal_watcher(shutdown_tx) {
        tracing::error!(error = %e, "failed to install signal handlers");
        return ExitCode::FAILURE;
    }

    match bpa_sessiond::run(socket, shutdown_rx).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            tracing::error!(error = %e, "sessiond run failed");
            ExitCode::FAILURE
        }
    }
}
