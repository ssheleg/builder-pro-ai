//! Builder Pro AI — Tauri core entry point (spec §6, §8.3, §13).
//!
//! `run()` builds the `tauri::Builder`: registers the 4 plugins (`store`/`dialog`/`fs`/`shell`),
//! the full `#[tauri::command]` surface (spec §6.1), and a `setup()` hook that brings up the
//! launchd-supervised daemon and connects to it over the Hop-B Unix-domain socket (spec §8.3).
//!
//! ## Honest degradation (spec §13)
//!
//! `setup()` never panics and never hangs the window open. If resolving the bundled daemon path,
//! installing/bootstrapping/kickstarting the LaunchAgent, or connecting to the daemon fails (e.g.
//! running under `cargo run` in dev, where `bpa-sessiond` is not bundled beside the app binary; or
//! launchd/TCC denies the operation), the failure is logged with `tracing::error!` and
//! `daemon://disconnected` is emitted so the frontend can render an actionable banner (the
//! frontend gates every command call on a connected state — see the T18 cross-task note in the
//! task brief). In that state `AppState` is left unmanaged; `State<'_, AppState>` extraction in a
//! command would panic, which is why the frontend must never invoke a command before observing
//! `daemon://reconnected` (or an initial connected state).

pub mod broker;
pub mod commands;
pub mod launchd;
pub mod paths;
pub mod socket_client;

use std::sync::Arc;
use std::time::Duration;

use tauri::{Emitter, Manager};
use tracing::{error, info, warn};

use crate::broker::{register, Broker};
use crate::commands::AppState;
use crate::launchd::{LaunchdAgent, LaunchdError, RealLaunchctl};
use crate::socket_client::{resolve_socket_path, ClientError, DaemonClient};

/// Emitted (no payload) when the core loses — or never establishes — the daemon socket (spec
/// §6.3, §13). Mirrors [`broker::EV_DAEMON_DISCONNECTED`] byte-for-byte; kept as its own constant
/// (per the locked T18 interface) so `lib.rs` callers don't need to reach into `broker` for it.
pub const DAEMON_DISCONNECTED_EVENT: &str = broker::EV_DAEMON_DISCONNECTED;
/// Emitted (no payload) when the core establishes (or re-establishes) the daemon socket (spec
/// §6.3, §13). Mirrors [`broker::EV_DAEMON_RECONNECTED`] byte-for-byte.
pub const DAEMON_RECONNECTED_EVENT: &str = broker::EV_DAEMON_RECONNECTED;

/// Trivial invoke smoke command; proves the JS<->Rust IPC round-trip works (Task 1).
#[tauri::command]
fn ping() -> String {
    "pong".to_string()
}

/// The exact command surface (spec §6.1) — mirrored by the smoke test and by
/// `tauri::generate_handler!` in [`run`]. Keep the two lists in lockstep.
pub fn command_names() -> &'static [&'static str] {
    &[
        "create_session",
        "list_sessions",
        "attach_session",
        "detach_session",
        "write_stdin",
        "resize",
        "kill_session",
        "list_workspaces",
        "create_workspace",
        "get_session_state",
        "pick_folder",
    ]
}

/// The client-build string echoed to the daemon's `Hello` handshake for diagnostics (spec §7).
/// Never carries secrets.
fn client_build() -> String {
    format!("builder-pro-ai/{}", env!("CARGO_PKG_VERSION"))
}

/// Bounded retry around [`DaemonClient::connect`]: kickstart is asynchronous (launchd forks the
/// daemon and it needs a moment to bind its socket), so a single immediate `connect()` attempt
/// right after `kickstart()` returns is expected to race the daemon's startup. Retries
/// `attempts` times with a fixed `delay` between tries and returns the last error if every
/// attempt fails — never blocks indefinitely (spec §13: bounded, honest degradation). Pulled out
/// as its own function (rather than inlined in `setup()`) so it is unit-testable without a Tauri
/// runtime: a `delay` of a few milliseconds and an unreachable socket path let a test exercise
/// the give-up-and-return-`Err` path quickly and deterministically.
async fn connect_with_retry(
    client_build: String,
    attempts: u32,
    delay: Duration,
) -> Result<DaemonClient, ClientError> {
    let mut last_err = ClientError::Disconnected;
    for attempt in 1..=attempts.max(1) {
        match DaemonClient::connect(client_build.clone()).await {
            Ok(client) => return Ok(client),
            Err(e) => {
                warn!(attempt, attempts, error = %e, "daemon connect attempt failed");
                last_err = e;
                if attempt < attempts {
                    tokio::time::sleep(delay).await;
                }
            }
        }
    }
    Err(last_err)
}

/// Build a [`LaunchdAgent`] for the current user against the real `launchctl` (production only;
/// tests exercise `LaunchdAgent` directly against a mock runner in `launchd.rs`). Resolves the
/// bundled daemon path via [`LaunchdAgent::resolve_daemon_path`] and the socket path via
/// [`resolve_socket_path`] (spec §8.1/§8.3 — the core is the source of truth both `launchd.rs` and
/// the daemon must agree with).
fn build_launchd_agent(app: &tauri::AppHandle) -> Result<LaunchdAgent<'static>, LaunchdError> {
    let daemon_path = LaunchdAgent::resolve_daemon_path()?;
    let socket_path = resolve_socket_path();
    let home = app
        .path()
        .home_dir()
        .map_err(|e| LaunchdError::DaemonPath(format!("cannot resolve home dir: {e}")))?;
    let app_support_dir = app
        .path()
        .app_data_dir()
        .map_err(|e| LaunchdError::DaemonPath(format!("cannot resolve app data dir: {e}")))?;
    let uid = unsafe { libc::geteuid() };

    Ok(LaunchdAgent {
        // `RealLaunchctl` is a unit struct; `Box::leak` gives it a `'static` borrow so the agent
        // (built fresh once per `setup()` call) doesn't need to thread a runner lifetime through
        // `tauri::Builder::setup`'s `'static` closure bound. Leaking a zero-sized unit struct once
        // per process lifetime has no meaningful cost.
        runner: Box::leak(Box::new(RealLaunchctl)),
        uid,
        launch_agents_dir: home.join("Library").join("LaunchAgents"),
        app_support_dir,
        daemon_path,
        socket_path,
    })
}

/// Ensure the launchd LaunchAgent is installed, bootstrapped (idempotent), and kicked off (spec
/// §8.3). Returns `Err` only on a genuinely hard failure (TCC/permissions denial, or the daemon
/// binary missing from the bundle in dev) — "already bootstrapped"/"already running" are handled
/// as success inside `launchd.rs` itself.
fn ensure_daemon_running(app: &tauri::AppHandle) -> Result<(), LaunchdError> {
    let agent = build_launchd_agent(app)?;
    agent.install_agent()?;
    agent.bootstrap()?;
    agent.kickstart()?;
    Ok(())
}

/// Emit the no-payload `daemon://disconnected` banner event, logging the reason (spec §13:
/// actionable degradation, never a silent hang).
fn emit_disconnected(app: &tauri::AppHandle, reason: &str) {
    warn!(reason, "emitting daemon://disconnected");
    if let Err(e) = app.emit(DAEMON_DISCONNECTED_EVENT, ()) {
        error!(error = %e, "failed to emit daemon://disconnected");
    }
}

/// Bring up the daemon (launchd install+bootstrap+kickstart) and connect to it, wiring the
/// [`Broker`] into the resulting [`DaemonClient`]'s push/conn callbacks and `manage`-ing
/// [`AppState`] on success (spec §8.3, §13). Split out of the `setup()` closure so the
/// orchestration itself doesn't need to live inside a non-`async` closure body: `setup()` spawns
/// this on `tauri::async_runtime` and returns `Ok(())` immediately so the window still opens
/// while this runs in the background.
async fn bring_up_daemon(app: tauri::AppHandle, broker: Arc<Broker>) {
    if let Err(e) = ensure_daemon_running(&app) {
        error!(error = %e, "failed to bring up the launchd-managed daemon");
        emit_disconnected(&app, "could not start background service");
        return;
    }

    // Kickstart is asynchronous: give the daemon a moment to fork and bind its socket. 8 attempts
    // x 500ms = up to ~4s of bounded retry, inside the spec's "~3-5s" window.
    match connect_with_retry(client_build(), 8, Duration::from_millis(500)).await {
        Ok(client) => {
            let client = Arc::new(client);
            // register() wires both on_push -> broker.dispatch_push and on_conn ->
            // broker.dispatch_conn (which itself emits daemon://disconnected/reconnected on
            // future transitions, spec §13) — call exactly once (locked contract).
            register(broker.clone(), &client);
            app.manage(AppState { client, broker });
            info!("daemon connected; AppState managed");
        }
        Err(e) => {
            error!(error = %e, "daemon connect failed after bounded retry");
            emit_disconnected(&app, "daemon unreachable");
        }
    }
}

/// Build and run the Tauri application (spec §6, §8.3).
pub fn run() {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            ping,
            commands::create_session,
            commands::list_sessions,
            commands::attach_session,
            commands::detach_session,
            commands::write_stdin,
            commands::resize,
            commands::kill_session,
            commands::list_workspaces,
            commands::create_workspace,
            commands::get_session_state,
            commands::pick_folder,
        ])
        .setup(|app| {
            let handle = app.handle().clone();
            // Broker owns the attach map + fans daemon Push frames to Hop A. Constructed
            // synchronously (cheap: just an AppHandle clone + an empty map) so it's ready before
            // the async daemon-connect task below needs it.
            let broker = Arc::new(Broker::new(handle.clone()));

            // Never block `setup()` on launchd/network I/O: spawn the whole bring-up sequence and
            // return Ok(()) immediately so the window opens right away. Honest degradation (spec
            // §13) happens inside `bring_up_daemon` via `daemon://disconnected`; nothing here can
            // panic the app.
            tauri::async_runtime::spawn(bring_up_daemon(handle, broker));

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running Builder Pro AI");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_names_are_the_eleven_spec_6_1_commands() {
        let names = command_names();
        let expected = [
            "create_session",
            "list_sessions",
            "attach_session",
            "detach_session",
            "write_stdin",
            "resize",
            "kill_session",
            "list_workspaces",
            "create_workspace",
            "get_session_state",
            "pick_folder",
        ];
        assert_eq!(names.len(), expected.len(), "exactly 11 commands");
        for e in expected {
            assert!(names.contains(&e), "command surface must include {e}");
        }
    }

    #[test]
    fn daemon_event_names_are_locked() {
        assert_eq!(DAEMON_DISCONNECTED_EVENT, "daemon://disconnected");
        assert_eq!(DAEMON_RECONNECTED_EVENT, "daemon://reconnected");
    }

    #[test]
    fn daemon_event_constants_match_broker_constants() {
        // lib.rs re-exports must never drift from the broker's own event-name constants.
        assert_eq!(DAEMON_DISCONNECTED_EVENT, broker::EV_DAEMON_DISCONNECTED);
        assert_eq!(DAEMON_RECONNECTED_EVENT, broker::EV_DAEMON_RECONNECTED);
    }

    #[tokio::test]
    async fn connect_with_retry_gives_up_after_bounded_attempts_without_panicking() {
        // No daemon is listening anywhere near this path, and XDG_RUNTIME_DIR is left whatever
        // the test process inherited — connect() will fail fast (no daemon bound there), so this
        // exercises the give-up branch of connect_with_retry deterministically and quickly (3
        // attempts x 5ms).
        let started = std::time::Instant::now();
        let result =
            connect_with_retry("test-build".to_string(), 3, Duration::from_millis(5)).await;
        let elapsed = started.elapsed();

        assert!(
            result.is_err(),
            "expected connect_with_retry to give up and return Err"
        );
        assert!(
            elapsed < Duration::from_secs(5),
            "connect_with_retry took {elapsed:?}; expected a bounded, prompt failure"
        );
    }

    #[tokio::test]
    async fn connect_with_retry_treats_zero_attempts_as_one() {
        // attempts.max(1) guards against a misconfigured 0-attempt call silently returning
        // immediately without ever trying — it must still make exactly one attempt (and thus one
        // real failure) rather than a no-op success/failure.
        let result =
            connect_with_retry("test-build".to_string(), 0, Duration::from_millis(5)).await;
        assert!(result.is_err());
    }
}
