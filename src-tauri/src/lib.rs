//! Builder Pro AI — Tauri core entry point (spec §6, §8.3, §13, §6.2).
//!
//! `run()` builds the `tauri::Builder`: registers the 4 plugins (`store`/`dialog`/`fs`/`shell`),
//! the full `#[tauri::command]` surface (spec §6.1), and a `setup()` hook that brings up the
//! launchd-supervised daemon and connects to it over the Hop-B Unix-domain socket (spec §8.3).
//!
//! ## Always-managed `AppState`, swappable client slot (spec §6.2)
//!
//! `AppState` is **always** `manage`d — unconditionally, before the daemon connect attempt even
//! resolves — so `State<'_, AppState>` extraction in a `#[tauri::command]` (including
//! `commands::upgrade_daemon`) never panics, regardless of whether the daemon is up. What varies is
//! `AppState.client` (a `commands::ClientSlot`, i.e. `Arc<RwLock<Option<Arc<DaemonClient>>>>`):
//! it is `None` while disconnected or incompatible, `Some` once a connection is live. Commands gate
//! on this via `state.client()?`, which surfaces `CommandError::Disconnected` for an empty slot —
//! never a panic, never a silent hang.
//!
//! This design exists because a fatal `ClientError::IncompatibleDaemon` (see below) leaves the
//! `DaemonClient`'s `connection_task` dead (its `cmd_tx` is closed) — there is no way to
//! "reconnect" that same client in place, and on the *initial*-connect-incompatible path (a new
//! client build meeting an old, not-yet-upgraded daemon at launch — the dominant trigger) there was
//! never a working client to begin with. The upgrade flow instead force-kickstarts the daemon and
//! restarts the whole app (`commands::upgrade_daemon`), which re-runs `setup()` from scratch.
//!
//! ## Three connect outcomes (spec §13, §6.2)
//!
//! `bring_up_daemon` resolves `connect_with_retry` to exactly one of three outcomes, each wiring
//! `AppState.client` and emitting a distinct signal:
//! - **Connected**: the slot is populated (`Some`), the [`Broker`] is registered against the live
//!   client, and (on a *re*connect after a prior disconnect) `daemon://reconnected` fires from
//!   inside `register`'s `on_conn` wiring.
//! - **`IncompatibleDaemon`**: the slot stays `None`; `daemon://incompatible` fires so the frontend
//!   can offer the upgrade flow. This is fatal and is never retried (see `connect_with_retry`
//!   below) — a stale daemon build will never become compatible by waiting.
//! - **Any other error** (daemon not up yet, launchd failure, etc.): the slot stays `None`;
//!   `daemon://disconnected` fires (spec §13: actionable degradation, never a silent hang). The
//!   frontend gates every command call on observing a connected state before invoking.
//!
//! `setup()` itself never panics and never hangs the window open — all of the above happens inside
//! `bring_up_daemon`, spawned on `tauri::async_runtime` so the window opens immediately regardless
//! of how long the daemon bring-up takes.

pub mod broker;
pub mod commands;
pub mod fs_explorer;
pub mod launchd;
pub mod paths;
pub mod socket_client;

use std::sync::Arc;
use std::time::Duration;

use tauri::{Emitter, Manager};
use tracing::{error, info, warn};

use crate::broker::{register, Broker};
use crate::commands::{AppState, DaemonStatus, StatusSlot};
use crate::launchd::{LaunchdAgent, LaunchdError, RealLaunchctl};
use crate::socket_client::{resolve_socket_path, ClientError, ConnState, DaemonClient};

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

/// The exact command surface (spec §6.1/§6.2) — mirrored by the smoke test and by
/// `tauri::generate_handler!` in [`run`]. Keep the two lists in lockstep.
///
/// `daemon_status` (finding [12] pull-fallback, added in the final-review fix wave) is
/// deliberately NOT in this list: this function documents/locks the original 12-command spec
/// surface exactly (see `command_names_are_the_twelve_spec_6_1_and_6_2_commands`), while
/// `daemon_status` is still registered directly in `run()`'s `invoke_handler!` below so it is
/// actually callable from the webview.
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
        "upgrade_daemon",
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
///
/// A thin delegate to [`DaemonClient::connect_with_retry`] (round-2 regression R1): the actual
/// bounded-retry-with-escalation loop now lives in `socket_client.rs` so it shares its
/// `HandshakeSuspectCounter` classification with the reconnect loop's `connect_with_backoff`
/// (see that type's docs) — this wrapper is kept, with its exact pre-fix name/signature, purely so
/// existing call sites and this module's own unit tests below don't need a Tauri runtime to exercise
/// it either.
///
/// `ClientError::IncompatibleDaemon` is fatal and returned immediately, with **no retry and no
/// sleep** (spec §6.2) — a version mismatch will never resolve itself by waiting. This now covers
/// TWO ways to reach it: a genuine, well-formed `DaemonReply::Incompatible` (unchanged), and
/// (round-2 regression R1) `HANDSHAKE_SUSPECT_CAP` CONSECUTIVE transient handshake failures (EOF /
/// timeout / garbage / bad magic — a present-but-unhandshakeable daemon, the dominant upgrade
/// scenario: an old v1 daemon still running under launchd that EOFs on every v2 preamble). Every
/// other error (plain connect-refused, most commonly `Disconnected` — nothing listening yet, normal
/// at boot) keeps the bounded backoff, never escalating.
async fn connect_with_retry(
    client_build: String,
    attempts: u32,
    delay: Duration,
) -> Result<DaemonClient, ClientError> {
    DaemonClient::connect_with_retry(client_build, attempts, delay).await
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
/// §8.3), using the given, already-built `agent` (the same one that ends up in `AppState.launchd`,
/// so a later `upgrade_daemon` kickstarts with identical config). Returns `Err` only on a
/// genuinely hard failure (TCC/permissions denial, or the daemon binary missing from the bundle
/// in dev) — "already bootstrapped"/"already running" are handled as success inside `launchd.rs`
/// itself.
fn ensure_daemon_running(agent: &LaunchdAgent<'_>) -> Result<(), LaunchdError> {
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

/// Emit the no-payload `daemon://incompatible` banner event (spec §6.2): the handshake found no
/// overlap between this client build's `[min, max]` and the daemon's, so the frontend should offer
/// the upgrade flow rather than wait for a reconnect that will never succeed on its own.
fn emit_incompatible(app: &tauri::AppHandle) {
    warn!("emitting daemon://incompatible");
    if let Err(e) = app.emit(broker::EV_DAEMON_INCOMPATIBLE, ()) {
        error!(error = %e, "failed to emit daemon://incompatible");
    }
}

/// Pure mapping from a `DaemonClient::connect`/`connect_with_retry` outcome to the [`DaemonStatus`]
/// the pull-based `daemon_status` command should report (finding [12]). Pulled out as a plain,
/// non-async function (rather than inlined at each of `bring_up_daemon`'s three call sites) so it
/// is unit-testable without a Tauri runtime, and so the status written into `AppState.status`
/// always agrees byte-for-byte with the event `bring_up_daemon` emits for the same outcome.
fn status_for_connect_result(result: &Result<DaemonClient, ClientError>) -> DaemonStatus {
    match result {
        Ok(_) => DaemonStatus::Connected,
        Err(ClientError::IncompatibleDaemon {
            daemon_min,
            daemon_max,
        }) => DaemonStatus::Incompatible {
            daemon_min: *daemon_min,
            daemon_max: *daemon_max,
        },
        Err(_) => DaemonStatus::Disconnected,
    }
}

/// Pure mapping from a mid-session [`ConnState`] transition (fired by `DaemonClient::on_conn`,
/// spec §13/§6.2) to the [`DaemonStatus`] the pull-based `daemon_status` command should report
/// (finding [12]). Mirrors [`broker::map_conn_state`]'s classification exactly — `Connected`/
/// `Disconnected` map 1:1, and a fatal `ConnState::Incompatible` (genuine reply, or
/// `HANDSHAKE_SUSPECT_CAP` transient failures escalated — finding [11]) maps to
/// `DaemonStatus::Incompatible` with the same range.
fn status_for_conn_state(state: ConnState) -> DaemonStatus {
    match state {
        ConnState::Connected => DaemonStatus::Connected,
        ConnState::Disconnected => DaemonStatus::Disconnected,
        ConnState::Incompatible {
            daemon_min,
            daemon_max,
        } => DaemonStatus::Incompatible {
            daemon_min,
            daemon_max,
        },
    }
}

/// Bring up the daemon (launchd install+bootstrap+kickstart) and connect to it, wiring the
/// [`Broker`] into the resulting [`DaemonClient`]'s push/conn callbacks (spec §8.3, §13, §6.2).
/// `AppState` is `manage`d **unconditionally** before the connect result is known, with an empty
/// client slot — see the module doc's "Always-managed `AppState`" section — so `upgrade_daemon`
/// can always extract state, even if the daemon has never come up or the connect fails as
/// incompatible. Split out of the `setup()` closure so the orchestration itself doesn't need to
/// live inside a non-`async` closure body: `setup()` spawns this on `tauri::async_runtime` and
/// returns `Ok(())` immediately so the window still opens while this runs in the background.
async fn bring_up_daemon(app: tauri::AppHandle, broker: Arc<Broker>) {
    let slot: commands::ClientSlot = Arc::new(std::sync::RwLock::new(None));
    // Created BEFORE any `app.manage(...)` call (finding [12]): every branch below needs to write
    // into this same slot, and the mid-session `on_conn` callback registered after a successful
    // connect needs to close over a clone of it — cloning an `Arc` created up front is simpler and
    // less error-prone than trying to pull it back out of `AppState` after `manage()`.
    let status: StatusSlot = Arc::new(std::sync::Mutex::new(DaemonStatus::Disconnected));
    // Round-2 regression R3: one lock map per app process, shared across every `AppState` branch
    // below exactly like `status` — `write_stdin`'s per-session serialization must hold regardless
    // of which of `bring_up_daemon`'s three outcomes this connect attempt lands on.
    let write_stdin_locks = Arc::new(commands::WriteStdinLocks::new());

    let agent = match build_launchd_agent(&app) {
        Ok(agent) => Arc::new(agent),
        Err(e) => {
            error!(error = %e, "failed to resolve the launchd agent (bundled daemon path/dirs)");
            app.manage(AppState {
                client: slot,
                broker,
                // A zero-arg, always-`Err` agent so `AppState.launchd` can still be `manage`d:
                // there is no daemon path to kickstart if `build_launchd_agent` itself failed
                // (e.g. `bpa-sessiond` missing from the bundle in dev), so `upgrade_daemon` would
                // surface an honest `UpgradeFailed` if invoked in this state, rather than the app
                // never managing `AppState` at all.
                launchd: Arc::new(unreachable_launchd_agent()),
                status,
                write_stdin_locks,
            });
            emit_disconnected(&app, "could not resolve the background service binary");
            return;
        }
    };

    if let Err(e) = ensure_daemon_running(&agent) {
        error!(error = %e, "failed to bring up the launchd-managed daemon");
        app.manage(AppState {
            client: slot,
            broker,
            launchd: agent,
            status,
            write_stdin_locks,
        });
        emit_disconnected(&app, "could not start background service");
        return;
    }

    // Kickstart is asynchronous: give the daemon a moment to fork and bind its socket.
    // BOOT_CONNECT_ATTEMPTS (8) x 500ms = up to ~4s of bounded retry, inside the spec's "~3-5s"
    // window. IncompatibleDaemon short-circuits this (no retry, see `connect_with_retry`'s docs).
    // The budget is the NAMED const (round-3 hardening H3), not a literal: it must stay >=
    // HANDSHAKE_SUSPECT_CAP or the initial-connect Incompatible escalation (round-2 R1) silently
    // becomes unreachable — enforced by a compile-time assert next to the const and a runtime
    // clamp inside `DaemonClient::connect_with_retry`.
    let connect_result = connect_with_retry(
        client_build(),
        crate::socket_client::BOOT_CONNECT_ATTEMPTS,
        Duration::from_millis(500),
    )
    .await;
    commands::write_status(&status, status_for_connect_result(&connect_result));

    // manage() UNCONDITIONALLY, before inspecting the outcome (locked contract): every branch
    // below needs `AppState` to already be registered so a later `upgrade_daemon` invocation can
    // extract it regardless of which of the three outcomes happened here.
    app.manage(AppState {
        client: slot.clone(),
        broker: broker.clone(),
        launchd: agent,
        status: status.clone(),
        write_stdin_locks: write_stdin_locks.clone(),
    });

    match connect_result {
        Ok(client) => {
            let client = Arc::new(client);
            // register() wires both on_push -> broker.dispatch_push (plus the H2 write-lock
            // eviction on ChildExited, against the SAME lock map AppState holds) and on_conn ->
            // broker.dispatch_conn (which itself emits daemon://disconnected/reconnected/
            // incompatible on future transitions, spec §13/§6.2) — call exactly once (locked
            // contract), BEFORE moving the client into the slot.
            register(broker, &client, write_stdin_locks);
            // Second `on_conn` registration (finding [12]): keeps `AppState.status` in sync with
            // every subsequent mid-session transition (disconnect / reconnect / fatal
            // incompatible), so `daemon_status` always reflects the current truth even for
            // transitions that happen long after this initial connect. Independent of the broker's
            // own `on_conn` registration above — `DaemonClient::on_conn` supports multiple
            // callbacks (locked contract) and each one is invoked with the *current* state
            // immediately upon registration, so this also seeds `status` correctly here even
            // though `write_status(Connected)` already happened two lines above.
            let status_for_conn = status.clone();
            client.on_conn(move |state| {
                commands::write_status(&status_for_conn, status_for_conn_state(state));
            });
            slot.write().unwrap().replace(client);
            info!("daemon connected; AppState managed");
        }
        Err(ClientError::IncompatibleDaemon {
            daemon_min,
            daemon_max,
        }) => {
            error!(
                daemon_min,
                daemon_max, "daemon speaks an incompatible protocol version"
            );
            emit_incompatible(&app);
        }
        Err(e) => {
            error!(error = %e, "daemon connect failed after bounded retry");
            emit_disconnected(&app, "daemon unreachable");
        }
    }
}

/// A `LaunchdAgent` whose every `launchctl` call fails (never touches the real service DB): used
/// only as `AppState.launchd`'s value when `build_launchd_agent` itself already failed (no
/// resolvable daemon path/dirs), so `AppState` can still be `manage`d unconditionally. Any
/// subsequent `upgrade_daemon` call in this state surfaces an honest `CommandError::UpgradeFailed`
/// rather than panicking on a missing field.
fn unreachable_launchd_agent() -> LaunchdAgent<'static> {
    struct AlwaysFail;
    impl crate::launchd::LaunchctlRunner for AlwaysFail {
        fn run(&self, _args: &[&str]) -> std::io::Result<crate::launchd::LaunchctlOutput> {
            Err(std::io::Error::other(
                "launchd agent unavailable: daemon path could not be resolved at startup",
            ))
        }
    }
    LaunchdAgent {
        runner: Box::leak(Box::new(AlwaysFail)),
        uid: unsafe { libc::geteuid() },
        launch_agents_dir: std::env::temp_dir(),
        app_support_dir: std::env::temp_dir(),
        daemon_path: std::env::temp_dir(),
        socket_path: std::env::temp_dir(),
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
            commands::add_workspace_root,
            commands::remove_workspace_root,
            commands::get_command_events,
            commands::get_session_state,
            commands::pick_folder,
            commands::upgrade_daemon,
            commands::daemon_status,
            fs_explorer::list_dir,
            fs_explorer::read_file_preview,
            fs_explorer::create_file,
            fs_explorer::create_dir,
            fs_explorer::rename_entry,
            fs_explorer::move_entry,
            fs_explorer::delete_entry,
            fs_explorer::reveal_in_finder,
            fs_explorer::open_external,
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

    // ── DaemonStatus pull-fallback mapping (finding [12], spec §6.2) ───────────────────────

    #[tokio::test]
    async fn status_for_connect_result_maps_ok_to_connected() {
        // Real, connected DaemonClient against a stub daemon — exercises the actual `Ok` arm, not
        // a reconstruction (mirrors the stub-daemon pattern used across this crate).
        let (client, _sock) = commands::commands_over_stub_daemon::connect_to_stub(|_req| {
            bpa_protocol::Response::Ack
        })
        .await;
        let result: Result<DaemonClient, ClientError> = Ok(client);
        assert_eq!(status_for_connect_result(&result), DaemonStatus::Connected);
    }

    #[test]
    fn status_for_connect_result_maps_incompatible_daemon() {
        let result: Result<DaemonClient, ClientError> = Err(ClientError::IncompatibleDaemon {
            daemon_min: 5,
            daemon_max: 6,
        });
        assert_eq!(
            status_for_connect_result(&result),
            DaemonStatus::Incompatible {
                daemon_min: 5,
                daemon_max: 6
            }
        );
    }

    #[test]
    fn status_for_connect_result_maps_other_errors_to_disconnected() {
        let result: Result<DaemonClient, ClientError> = Err(ClientError::Disconnected);
        assert_eq!(
            status_for_connect_result(&result),
            DaemonStatus::Disconnected
        );

        let result2: Result<DaemonClient, ClientError> = Err(ClientError::Daemon {
            code: "X".into(),
            message: "Y".into(),
        });
        assert_eq!(
            status_for_connect_result(&result2),
            DaemonStatus::Disconnected
        );
    }

    #[test]
    fn status_for_conn_state_maps_every_variant() {
        assert_eq!(
            status_for_conn_state(ConnState::Connected),
            DaemonStatus::Connected
        );
        assert_eq!(
            status_for_conn_state(ConnState::Disconnected),
            DaemonStatus::Disconnected
        );
        assert_eq!(
            status_for_conn_state(ConnState::Incompatible {
                daemon_min: 2,
                daemon_max: 3
            }),
            DaemonStatus::Incompatible {
                daemon_min: 2,
                daemon_max: 3
            }
        );
    }

    #[test]
    fn command_names_are_the_twelve_spec_6_1_and_6_2_commands() {
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
            "upgrade_daemon",
        ];
        assert_eq!(names.len(), expected.len(), "exactly 12 commands");
        for e in expected {
            assert!(names.contains(&e), "command surface must include {e}");
        }
    }

    // Mock-runner pattern mirrors `launchd::tests::MockLaunchctl`/`agent()` exactly (private to
    // that module's own test suite, so replicated here) — used to prove `ensure_daemon_running`
    // (the boot path, called on EVERY app launch) invokes the NON-force kickstart shape, never
    // `-k` (findings [10]/[16]: `-k` on the boot path force-kills a running daemon and destroys
    // every live session with zero consent, on every single relaunch).
    struct MockLaunchctl {
        calls: std::sync::Mutex<std::cell::RefCell<Vec<Vec<String>>>>,
        scripted: std::sync::Mutex<
            std::cell::RefCell<std::collections::VecDeque<crate::launchd::LaunchctlOutput>>,
        >,
    }
    impl MockLaunchctl {
        fn new(outputs: Vec<crate::launchd::LaunchctlOutput>) -> Self {
            MockLaunchctl {
                calls: std::sync::Mutex::new(std::cell::RefCell::new(Vec::new())),
                scripted: std::sync::Mutex::new(std::cell::RefCell::new(
                    outputs.into_iter().collect(),
                )),
            }
        }
        fn calls(&self) -> Vec<Vec<String>> {
            self.calls.lock().unwrap().borrow().clone()
        }
    }
    impl crate::launchd::LaunchctlRunner for MockLaunchctl {
        fn run(&self, args: &[&str]) -> std::io::Result<crate::launchd::LaunchctlOutput> {
            self.calls
                .lock()
                .unwrap()
                .borrow_mut()
                .push(args.iter().map(|s| s.to_string()).collect());
            let out = self
                .scripted
                .lock()
                .unwrap()
                .borrow_mut()
                .pop_front()
                .unwrap_or(crate::launchd::LaunchctlOutput {
                    code: 0,
                    stdout: String::new(),
                    stderr: String::new(),
                });
            Ok(out)
        }
    }

    fn ok_output() -> crate::launchd::LaunchctlOutput {
        crate::launchd::LaunchctlOutput {
            code: 0,
            stdout: String::new(),
            stderr: String::new(),
        }
    }

    #[test]
    fn ensure_daemon_running_uses_non_force_kickstart_on_boot() {
        // install_agent() does real fs writes but touches no launchctl; bootstrap() and
        // kickstart() are the two runner calls to script.
        let mock = MockLaunchctl::new(vec![ok_output(), ok_output()]);
        let tmp = tempfile::tempdir().unwrap();
        let agent = crate::launchd::LaunchdAgent {
            runner: &mock,
            uid: 501,
            launch_agents_dir: tmp.path().join("LaunchAgents"),
            app_support_dir: tmp.path().join("AppSupport"),
            daemon_path: std::path::PathBuf::from(
                "/Applications/Builder Pro AI.app/Contents/MacOS/bpa-sessiond",
            ),
            socket_path: std::path::PathBuf::from("/tmp/bpa-501/d.sock"),
        };

        ensure_daemon_running(&agent).expect("boot path must succeed against a scripted-ok mock");

        let calls = mock.calls();
        assert_eq!(
            calls.len(),
            2,
            "expected bootstrap + kickstart, got {calls:?}"
        );
        assert_eq!(calls[0][0], "bootstrap");
        assert_eq!(
            calls[1],
            vec!["kickstart", "gui/501/ai.builderpro.desktop.sessiond"],
            "boot-path kickstart must NEVER carry -k: force-killing a running daemon on every \
             app launch destroys live sessions with zero consent (findings [10]/[16])"
        );
    }

    #[test]
    fn daemon_event_names_are_locked() {
        assert_eq!(DAEMON_DISCONNECTED_EVENT, "daemon://disconnected");
        assert_eq!(DAEMON_RECONNECTED_EVENT, "daemon://reconnected");
        assert_eq!(broker::EV_DAEMON_INCOMPATIBLE, "daemon://incompatible");
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
        // exercises the give-up branch of connect_with_retry deterministically and quickly (the
        // requested 3 attempts are clamped up to HANDSHAKE_SUSPECT_CAP = 8 by the H3 guard, so
        // 8 attempts x 5ms).
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
    async fn connect_with_retry_clamps_zero_attempts_to_a_real_bounded_budget() {
        // A misconfigured 0-attempt call must not silently return without ever trying: the H3
        // clamp raises it to HANDSHAKE_SUSPECT_CAP (8) real attempts (pre-H3, `attempts.max(1)`
        // guaranteed exactly one) — still bounded, still an honest Err when nothing is listening.
        let started = std::time::Instant::now();
        let result =
            connect_with_retry("test-build".to_string(), 0, Duration::from_millis(5)).await;
        assert!(result.is_err());
        assert!(
            started.elapsed() < Duration::from_secs(5),
            "the clamped budget must stay bounded"
        );
    }

    // Serializes tests that mutate XDG_RUNTIME_DIR (DaemonClient::connect() resolves the socket
    // path from it), same discipline as `commands::commands_over_stub_daemon::ENV_TEST_LOCK` and
    // for the identical shared-process-state reason.
    static ENV_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    #[tokio::test]
    async fn connect_with_retry_does_not_retry_incompatible() {
        use bpa_protocol::preamble::{decode_client_preamble, encode_daemon_reply, DaemonReply};
        use std::sync::atomic::{AtomicBool, Ordering};
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        use tokio::net::UnixListener;

        let dir = tempfile::tempdir().unwrap();
        let bpa_dir = dir.path().join("bpa");
        std::fs::create_dir_all(&bpa_dir).unwrap();
        let sock_path = bpa_dir.join("d.sock");

        let listener = UnixListener::bind(&sock_path).unwrap();
        let ready = Arc::new(AtomicBool::new(false));
        let ready2 = ready.clone();
        // A stub daemon that always replies Incompatible to EVERY connection attempt it accepts —
        // if connect_with_retry incorrectly retried, this stub would serve every one of those
        // retries the same fatal reply, and the wall-clock assertion below would catch it.
        tokio::spawn(async move {
            ready2.store(true, Ordering::SeqCst);
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let mut header = [0u8; 10];
                if stream.read_exact(&mut header).await.is_err() {
                    continue;
                }
                let build_len = u16::from_le_bytes(header[8..10].try_into().unwrap()) as usize;
                let mut buf = header.to_vec();
                if build_len > 0 {
                    let mut build = vec![0u8; build_len];
                    if stream.read_exact(&mut build).await.is_err() {
                        continue;
                    }
                    buf.extend_from_slice(&build);
                }
                let _ = decode_client_preamble(&buf);
                let reply = encode_daemon_reply(&DaemonReply::Incompatible { min: 5, max: 6 });
                let _ = stream.write_all(&reply).await;
                let _ = stream.flush().await;
            }
        });
        while !ready.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }

        // 8 attempts x 500ms (bring_up_daemon's real config) would take ~3.5s if this incorrectly
        // retried; a prompt return well under that bound proves no retry happened.
        let started = std::time::Instant::now();
        let result = {
            let _guard = ENV_TEST_LOCK.lock().await;
            unsafe {
                std::env::set_var("XDG_RUNTIME_DIR", dir.path());
            }
            let result =
                connect_with_retry("test-build".to_string(), 8, Duration::from_millis(500)).await;
            unsafe {
                std::env::remove_var("XDG_RUNTIME_DIR");
            }
            result
        };
        let elapsed = started.elapsed();
        std::mem::forget(dir);

        match result {
            Err(ClientError::IncompatibleDaemon {
                daemon_min,
                daemon_max,
            }) => {
                assert_eq!((daemon_min, daemon_max), (5, 6));
            }
            other => panic!("expected IncompatibleDaemon, got {other:?}"),
        }
        assert!(
            elapsed < Duration::from_secs(1),
            "connect_with_retry took {elapsed:?}; IncompatibleDaemon must return promptly with no retry, not run the full ~3.5s of bounded backoff"
        );
    }

    // ---- Round-2 regression R1 (CRITICAL): `connect_with_retry` — the EXACT function
    // ---- `bring_up_daemon` calls for the INITIAL connect at app boot — must escalate to
    // ---- `IncompatibleDaemon{0,0}` once a present-but-unhandshakeable daemon (a stub that accepts
    // ---- every TCP connection but never completes the handshake, e.g. a genuine v1 daemon that
    // ---- EOFs reading a v2 preamble it cannot decode) has failed HANDSHAKE_SUSPECT_CAP consecutive
    // ---- times — never an infinite `Disconnected` retry that leaves the upgrade dialog
    // ---- unreachable. Combined with `status_for_connect_result_maps_incompatible_daemon` (this
    // ---- file) and `connect_with_retry_does_not_retry_incompatible` (above), this proves
    // ---- `bring_up_daemon`'s full match arm chain: connect_result -> write_status ->
    // ---- Err(IncompatibleDaemon) -> emit_incompatible + DaemonStatus::Incompatible, all reachable
    // ---- from this scenario now that connect_with_retry itself yields that variant. ----
    #[tokio::test]
    async fn connect_with_retry_escalates_to_incompatible_when_daemon_accepts_but_never_handshakes()
    {
        use std::sync::atomic::{AtomicBool, Ordering};
        use tokio::io::AsyncReadExt;
        use tokio::net::UnixListener;

        let dir = tempfile::tempdir().unwrap();
        let bpa_dir = dir.path().join("bpa");
        std::fs::create_dir_all(&bpa_dir).unwrap();
        let sock_path = bpa_dir.join("d.sock");

        let listener = UnixListener::bind(&sock_path).unwrap();
        let ready = Arc::new(AtomicBool::new(false));
        let ready2 = ready.clone();
        // A stub daemon that accepts every connection, reads the client's preamble, then closes
        // WITHOUT replying at all — exactly what a genuine v1/bincode daemon looks like from the
        // outside when handed a v2 CBOR preamble it cannot decode (it reads the connection as a
        // `Request::Hello` frame, fails, and closes). Never sends a real Accepted/Incompatible reply
        // on any attempt, so a correctly-fixed `connect_with_retry` must escalate once the cap of
        // HANDSHAKE_SUSPECT_CAP (8) consecutive such failures is exhausted, not retry forever.
        tokio::spawn(async move {
            ready2.store(true, Ordering::SeqCst);
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let mut header = [0u8; 10];
                let _ = stream.read_exact(&mut header).await;
                drop(stream);
            }
        });
        while !ready.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }

        let result = {
            let _guard = ENV_TEST_LOCK.lock().await;
            unsafe {
                std::env::set_var("XDG_RUNTIME_DIR", dir.path());
            }
            // The ACTUAL production budget const (round-3 hardening H3) — not a hand-picked `8` —
            // so a future edit lowering `BOOT_CONNECT_ATTEMPTS` below `HANDSHAKE_SUSPECT_CAP`
            // fails THIS test (the loop would exhaust as Disconnected before the escalation ever
            // fired), on top of failing the compile-time assert next to the const. Only the delay
            // differs from production (5ms instead of 500ms) so the test stays fast.
            let result = connect_with_retry(
                "test-build".to_string(),
                crate::socket_client::BOOT_CONNECT_ATTEMPTS,
                Duration::from_millis(5),
            )
            .await;
            unsafe {
                std::env::remove_var("XDG_RUNTIME_DIR");
            }
            result
        };
        std::mem::forget(dir);

        match &result {
            Err(ClientError::IncompatibleDaemon {
                daemon_min,
                daemon_max,
            }) => {
                assert_eq!(
                    (*daemon_min, *daemon_max),
                    (0, 0),
                    "expected the unknown-range sentinel after cap-exhausted transient failures"
                );
            }
            other => panic!(
                "expected IncompatibleDaemon{{0,0}} after the initial connect exhausted \
                 HANDSHAKE_SUSPECT_CAP consecutive transient failures, got {other:?} — a present-\
                 but-unhandshakeable daemon must reach the upgrade dialog, not retry \"daemon \
                 unreachable\" forever"
            ),
        }

        // The SAME outcome must also map to the pull-fallback DaemonStatus the frontend can poll
        // (finding [12]) — proving bring_up_daemon's `write_status(status_for_connect_result(..))`
        // call, immediately following connect_with_retry in production, would surface this
        // correctly too.
        assert_eq!(
            status_for_connect_result(&result),
            DaemonStatus::Incompatible {
                daemon_min: 0,
                daemon_max: 0
            }
        );
    }

    // ---- Round-3 hardening H3: the `attempts >= HANDSHAKE_SUSPECT_CAP` coupling must be
    // ---- structurally unbreakable at runtime too, not just by the compile-time assert on
    // ---- `BOOT_CONNECT_ATTEMPTS`: even a caller that UNDER-sets `attempts` on the boot-path
    // ---- entry (`connect_with_retry`) must still give the suspect counter its full
    // ---- HANDSHAKE_SUSPECT_CAP consecutive transient failures — so a present-but-
    // ---- unhandshakeable daemon still escalates to IncompatibleDaemon (upgrade dialog reachable)
    // ---- instead of exhausting a too-small loop as a silent `Disconnected` re-break of R1. ----
    #[tokio::test]
    async fn connect_with_retry_clamps_underset_attempts_so_escalation_still_fires() {
        use std::sync::atomic::{AtomicBool, Ordering};
        use tokio::io::AsyncReadExt;
        use tokio::net::UnixListener;

        let dir = tempfile::tempdir().unwrap();
        let bpa_dir = dir.path().join("bpa");
        std::fs::create_dir_all(&bpa_dir).unwrap();
        let sock_path = bpa_dir.join("d.sock");

        let listener = UnixListener::bind(&sock_path).unwrap();
        let ready = Arc::new(AtomicBool::new(false));
        let ready2 = ready.clone();
        // Same always-transient-fail stub as the R1 escalation test above: accepts every
        // connection, reads the preamble header, closes without ever replying.
        tokio::spawn(async move {
            ready2.store(true, Ordering::SeqCst);
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let mut header = [0u8; 10];
                let _ = stream.read_exact(&mut header).await;
                drop(stream);
            }
        });
        while !ready.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }

        let result = {
            let _guard = ENV_TEST_LOCK.lock().await;
            unsafe {
                std::env::set_var("XDG_RUNTIME_DIR", dir.path());
            }
            // attempts = 1, deliberately UNDER the cap: without the clamp this exhausts the loop
            // after a single transient failure and returns Disconnected — the exact silent
            // re-break of the round-2 R1 Critical this test exists to prevent.
            let result =
                connect_with_retry("test-build".to_string(), 1, Duration::from_millis(5)).await;
            unsafe {
                std::env::remove_var("XDG_RUNTIME_DIR");
            }
            result
        };
        std::mem::forget(dir);

        match &result {
            Err(ClientError::IncompatibleDaemon {
                daemon_min,
                daemon_max,
            }) => {
                assert_eq!(
                    (*daemon_min, *daemon_max),
                    (0, 0),
                    "expected the unknown-range sentinel after cap-exhausted transient failures"
                );
            }
            other => panic!(
                "expected IncompatibleDaemon{{0,0}} even with an under-set attempts budget — \
                 the boot path must clamp to HANDSHAKE_SUSPECT_CAP so the escalation stays \
                 reachable, got {other:?}"
            ),
        }
    }
}
