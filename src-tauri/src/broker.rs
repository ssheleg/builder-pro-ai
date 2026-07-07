//! Hop-B `Push` frame broker (spec §6.3, §7): fans daemon-pushed frames out to either an
//! attached session's `Channel<TerminalEvent>` (high-frequency PTY firehose) or the Tauri global
//! event bus (low-frequency lifecycle/workspace notifications).
//!
//! ## Design
//!
//! [`map_push`] is a **pure** function — `Push -> BrokerAction` — with no Tauri runtime
//! dependency, so the mapping table (spec §7 "Broker mapping (core)") is exhaustively unit-tested
//! without spinning up a webview. [`Broker::dispatch_push`] is the thin, side-effecting shell
//! around it: look up the attach-map entry for `SendChannel`, `app.emit` for `Emit`.
//!
//! [`register`] wires [`Broker::dispatch_push`]/[`Broker::dispatch_conn`] into `DaemonClient::
//! on_push`/`on_conn` (called from T18's `setup()`). Those `DaemonClient` callbacks run **inline
//! on the connection task** (locked contract, see `socket_client` docs) — every branch here is
//! therefore non-blocking: no `.await` on a lock across the callback, no blocking I/O. The attach
//! map uses `std::sync::Mutex` (not `tokio::sync::Mutex`) precisely so `register_attachment`/
//! `remove_attachment`/`dispatch_push` never need to be `async fn` and can be called synchronously
//! from inside the non-async `on_push`/`on_conn` closures.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use bpa_protocol::{Push, SessionId, TerminalEvent};
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter};
use tracing::{debug, warn};

use crate::socket_client::{ConnState, DaemonClient};

/// Global event names (spec §6.3). Kept as constants so this module and T18's frontend-facing
/// wiring (and tests) can never drift on the string literal.
pub const EV_SESSION_CREATED: &str = "session://created";
pub const EV_SESSION_STATE_CHANGED: &str = "session://state-changed";
pub const EV_SESSION_EXITED: &str = "session://exited";
pub const EV_WORKSPACE_CREATED: &str = "workspace://created";
pub const EV_DAEMON_DISCONNECTED: &str = "daemon://disconnected";
pub const EV_DAEMON_RECONNECTED: &str = "daemon://reconnected";
/// Emitted (no payload) when the handshake preamble (spec §4.5) finds the daemon's protocol range
/// incompatible with this client build — the signal that drives the upgrade flow (spec §6.2).
/// Distinct from `EV_DAEMON_DISCONNECTED`: a bounded reconnect can never resolve this on its own.
pub const EV_DAEMON_INCOMPATIBLE: &str = "daemon://incompatible";

/// The effect a single `Push` frame should have on Hop-A, decided without touching the Tauri
/// runtime. Produced by [`map_push`]; consumed by [`Broker::dispatch_push`].
#[derive(Debug, Clone, PartialEq)]
pub enum BrokerAction {
    /// Forward `event` to the `Channel<TerminalEvent>` attached to `session_id`, if any is
    /// currently registered (a push for an unattached/detached session is dropped, not queued).
    SendChannel(SessionId, TerminalEvent),
    /// `app.emit(event, payload)` on the Tauri global event bus. `payload` is pre-serialized to
    /// `serde_json::Value` here (rather than carried as a generic) so `BrokerAction` itself stays
    /// a plain, comparable, non-generic enum — convenient for exhaustive unit tests.
    Emit(&'static str, serde_json::Value),
    /// No Hop-A effect (e.g. `Push::Error`, which is logged, not surfaced as an event).
    Ignore,
}

/// Pure mapping table: `Push` variant -> `BrokerAction` (spec §7 "Broker mapping (core)", §6.3
/// "Global events"). No I/O, no Tauri runtime — fully unit-testable.
///
/// - `Push::Replay` / `Push::Output` -> `SendChannel` (the PTY firehose; never touches the global
///   event bus, spec §6.2 "Bytes never enter React/Zustand state").
/// - `Push::StateChanged` -> `session://state-changed`, snake_case daemon fields renamed to
///   camelCase (spec §6.3 table: `{ sessionId, lifecycle, waitingForInput, cwd }`).
/// - `Push::ChildExited` -> `session://exited`, reshaped to `{ sessionId, code, signal }`; `code`/
///   `signal` `None` serialize as JSON `null` (`serde_json::to_value` on `Option<T>` never
///   coerces `None` to a default — spec §5 exit-code note).
/// - `Push::SessionCreated` -> `session://created` with the raw `SessionMeta` payload. The daemon
///   broadcasts this to every connected client, **including the one that requested the create**
///   (the `create_session` command's own `Response::Session` already resolved that caller's
///   Promise) — de-duplicating/upserting by id is the frontend store's job, not the broker's.
/// - `Push::WorkspaceCreated` -> `workspace://created` with the raw `Workspace` payload, same
///   broadcast-to-all rationale.
/// - `Push::Error` -> `Ignore` (logged by the caller; async/un-correlated daemon errors are not
///   surfaced as a Hop-A event in S1 — spec §7 broker-mapping table: "log + mark session errored").
pub fn map_push(push: Push) -> BrokerAction {
    match push {
        Push::Replay {
            session_id,
            cols,
            rows,
            content,
        } => BrokerAction::SendChannel(
            session_id,
            TerminalEvent::Replay {
                cols,
                rows,
                content,
            },
        ),
        Push::Output { session_id, bytes } => {
            BrokerAction::SendChannel(session_id, TerminalEvent::Output { bytes })
        }
        Push::StateChanged {
            session_id,
            lifecycle,
            waiting_for_input,
            cwd,
        } => {
            let payload = serde_json::json!({
                "sessionId": session_id,
                "lifecycle": lifecycle,
                "waitingForInput": waiting_for_input,
                "cwd": cwd,
            });
            BrokerAction::Emit(EV_SESSION_STATE_CHANGED, payload)
        }
        Push::ChildExited {
            session_id,
            code,
            signal,
        } => {
            let payload = serde_json::json!({
                "sessionId": session_id,
                "code": code,
                "signal": signal,
            });
            BrokerAction::Emit(EV_SESSION_EXITED, payload)
        }
        Push::SessionCreated { meta } => {
            let payload = serde_json::to_value(meta)
                .expect("SessionMeta is a plain-data struct; serialization cannot fail");
            BrokerAction::Emit(EV_SESSION_CREATED, payload)
        }
        Push::WorkspaceCreated { workspace } => {
            let payload = serde_json::to_value(workspace)
                .expect("Workspace is a plain-data struct; serialization cannot fail");
            BrokerAction::Emit(EV_WORKSPACE_CREATED, payload)
        }
        Push::Error {
            session_id,
            code,
            message,
        } => {
            warn!(target: "broker", ?session_id, code = %code, message = %message, "daemon async error push");
            BrokerAction::Ignore
        }
    }
}

/// Conn-state tracking (spec §13: "On reconnect -> `daemon://reconnected`"; spec §6.2: "On fatal
/// handshake mismatch -> `daemon://incompatible`"). `DaemonClient::on_conn` fires `Connected` both
/// for the **initial** connect and for every successful reconnect (locked contract, T14 docs) —
/// this function is the pure decision of which of the three global events (if any) that transition
/// should raise, given whether a `Disconnected` has already been observed. `seen_disconnected` is
/// read-then-written by the caller (see [`Broker::on_conn`]) so this stays a plain, easily-tested
/// function rather than needing to own the flag itself.
///
/// Returns the event name to emit, or `None` (the very first `Connected`, before any
/// `Disconnected` was ever seen, is not a "reconnect" and gets no event per spec §6.3's table,
/// which lists only `daemon://disconnected` / `daemon://reconnected` — never a "connected" event).
pub fn map_conn_state(state: ConnState, seen_disconnected: &mut bool) -> Option<&'static str> {
    match state {
        ConnState::Disconnected => {
            *seen_disconnected = true;
            Some(EV_DAEMON_DISCONNECTED)
        }
        ConnState::Connected => {
            if std::mem::replace(seen_disconnected, false) {
                Some(EV_DAEMON_RECONNECTED)
            } else {
                None
            }
        }
        // Finding [11]: a mid-session fatal handshake classification (genuine Incompatible reply,
        // or HANDSHAKE_SUSPECT_CAP consecutive transient failures escalated to the same shape) must
        // reach the frontend exactly like the initial-connect path does, so the upgrade dialog is
        // reachable instead of the connection task silently dying with no event at all. Set
        // `seen_disconnected = true` (this connection is now permanently dead — the connection task
        // has already returned) so that IF the user manually recovers later (quit+relaunch,
        // upgrade+restart) and a fresh client eventually connects, the resulting `Connected`
        // correctly fires `daemon://reconnected` rather than being silently swallowed as "not a
        // reconnect" by the `seen_disconnected == false` branch above.
        ConnState::Incompatible { .. } => {
            *seen_disconnected = true;
            Some(EV_DAEMON_INCOMPATIBLE)
        }
    }
}

/// Per-session attach-channel map, shared between the `#[tauri::command]` handlers (which
/// register/remove entries) and the `on_push` callback (which reads them). Plain
/// `std::sync::Mutex` — never held across an `.await`, and required to be sync-lockable from the
/// non-async `on_push`/`on_conn` callbacks (see module docs).
pub type AttachMap = Arc<Mutex<HashMap<SessionId, Channel<TerminalEvent>>>>;

/// Owns the attach map and the `AppHandle` used to reach both the `Channel` firehose and the
/// global event bus. Constructed once in T18's `setup()` and stored in `AppState`; commands reach
/// it via `State<'_, AppState>`, and [`register`] wires it into the `DaemonClient`'s callbacks.
#[derive(Clone)]
pub struct Broker {
    app: AppHandle,
    attachments: AttachMap,
}

impl Broker {
    pub fn new(app: AppHandle) -> Self {
        Broker {
            app,
            attachments: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Register (or supersede) the attach channel for a session (spec §7: a second attach
    /// supersedes the prior registration — single-attach, GUI is one client).
    pub fn register_attachment(&self, session_id: SessionId, ch: Channel<TerminalEvent>) {
        self.attachments
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(session_id, ch);
    }

    /// Remove a session's attach channel (on detach / kill / exit / attach failure).
    pub fn remove_attachment(&self, session_id: &SessionId) {
        self.attachments
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .remove(session_id);
    }

    /// Apply [`map_push`] and carry out its `BrokerAction`. This is the body invoked (indirectly,
    /// via [`register`]) from `DaemonClient::on_push` — it MUST NOT block: the channel lookup is a
    /// short `std::sync::Mutex` critical section (never held across an `.await`, and there is none
    /// here), `Channel::send` is a non-blocking enqueue, and `AppHandle::emit` is non-blocking.
    pub fn dispatch_push(&self, push: Push) {
        match map_push(push) {
            BrokerAction::SendChannel(session_id, event) => {
                let guard = self.attachments.lock().unwrap_or_else(|e| e.into_inner());
                match guard.get(&session_id) {
                    Some(ch) => {
                        if let Err(e) = ch.send(event) {
                            warn!(target: "broker", session_id = %session_id, error = %e, "channel send failed");
                        }
                    }
                    None => {
                        debug!(target: "broker", session_id = %session_id, "push for unattached session dropped");
                    }
                }
            }
            BrokerAction::Emit(event, payload) => self.emit(event, payload),
            BrokerAction::Ignore => {}
        }
    }

    /// Apply [`map_conn_state`] and emit the resulting event, if any. `seen_disconnected` is a
    /// `Mutex<bool>` (rather than a plain `&mut bool`) because `DaemonClient::on_conn` requires an
    /// `Fn` callback (it may be invoked repeatedly, and once synchronously inside `on_conn` itself
    /// to replay the current state) — interior mutability is the only way for the closure in
    /// [`register`] to update the flag across calls while staying `Fn`.
    pub fn dispatch_conn(&self, state: ConnState, seen_disconnected: &std::sync::Mutex<bool>) {
        let mut guard = seen_disconnected.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(event) = map_conn_state(state, &mut guard) {
            drop(guard);
            self.emit_no_payload(event);
        }
    }

    fn emit<T: Serialize + Clone>(&self, event: &str, payload: T) {
        if let Err(e) = self.app.emit(event, payload) {
            warn!(target: "broker", event, error = %e, "emit failed");
        }
    }

    fn emit_no_payload(&self, event: &str) {
        if let Err(e) = self.app.emit(event, ()) {
            warn!(target: "broker", event, error = %e, "emit failed");
        }
    }
}

/// Wire a [`Broker`] into a `DaemonClient`'s push/conn callbacks (spec §7 correlation: `Push`
/// frames are fanned out by the broker; §13: conn transitions raise `daemon://disconnected`/
/// `daemon://reconnected`). Called once from T18's `setup()`, after both the `DaemonClient` and
/// the `Broker` (inside `AppState`) exist.
///
/// `client.on_push`/`client.on_conn` callbacks run inline on `DaemonClient`'s connection task
/// (locked contract) — both closures below only take a short `std::sync::Mutex` lock and call
/// non-blocking `Channel::send`/`AppHandle::emit`, so neither one blocks that task.
pub fn register(broker: Arc<Broker>, client: &DaemonClient) {
    let push_broker = broker.clone();
    client.on_push(move |push| push_broker.dispatch_push(push));

    // One `seen_disconnected` flag per registration, captured by the `on_conn` closure. `on_conn`
    // replays the *current* `ConnState` synchronously before returning (T14 locked contract), so
    // this flag starts correctly: if the daemon is already down when `register` runs, the replayed
    // `Disconnected` sets it immediately; if already up (the normal initial-connect case), the
    // replayed `Connected` leaves it `false` and emits nothing (see `map_conn_state` docs).
    let seen_disconnected = std::sync::Mutex::new(false);
    client.on_conn(move |state| broker.dispatch_conn(state, &seen_disconnected));
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpa_protocol::{SessionLifecycle, SessionMeta, Workspace};

    // ── map_push: exhaustive, one test per Push variant ────────────────────────────────────

    #[test]
    fn replay_maps_to_send_channel_with_terminal_event_replay() {
        let push = Push::Replay {
            session_id: "sess-1".to_string(),
            cols: 120,
            rows: 40,
            content: vec![1, 2, 3],
        };
        let action = map_push(push);
        assert_eq!(
            action,
            BrokerAction::SendChannel(
                "sess-1".to_string(),
                TerminalEvent::Replay {
                    cols: 120,
                    rows: 40,
                    content: vec![1, 2, 3]
                }
            )
        );
    }

    #[test]
    fn output_maps_to_send_channel_with_terminal_event_output() {
        let push = Push::Output {
            session_id: "sess-1".to_string(),
            bytes: vec![7, 8, 9],
        };
        let action = map_push(push);
        assert_eq!(
            action,
            BrokerAction::SendChannel(
                "sess-1".to_string(),
                TerminalEvent::Output {
                    bytes: vec![7, 8, 9]
                }
            )
        );
    }

    #[test]
    fn state_changed_maps_to_camel_case_emit_payload() {
        let push = Push::StateChanged {
            session_id: "sess-1".to_string(),
            lifecycle: SessionLifecycle::Running,
            waiting_for_input: true,
            cwd: "/work/proj".to_string(),
        };
        let action = map_push(push);
        match action {
            BrokerAction::Emit(event, payload) => {
                assert_eq!(event, EV_SESSION_STATE_CHANGED);
                assert_eq!(payload["sessionId"], "sess-1");
                assert_eq!(payload["lifecycle"]["kind"], "running");
                assert_eq!(payload["waitingForInput"], true);
                assert_eq!(payload["cwd"], "/work/proj");
                // snake_case keys must NOT leak through.
                assert!(payload.get("session_id").is_none());
                assert!(payload.get("waiting_for_input").is_none());
            }
            other => panic!("expected Emit, got {other:?}"),
        }
    }

    #[test]
    fn child_exited_maps_to_reshaped_emit_payload() {
        let push = Push::ChildExited {
            session_id: "sess-2".to_string(),
            code: Some(137),
            signal: Some("SIGKILL".to_string()),
        };
        let action = map_push(push);
        match action {
            BrokerAction::Emit(event, payload) => {
                assert_eq!(event, EV_SESSION_EXITED);
                assert_eq!(payload["sessionId"], "sess-2");
                assert_eq!(payload["code"], 137);
                assert_eq!(payload["signal"], "SIGKILL");
            }
            other => panic!("expected Emit, got {other:?}"),
        }
    }

    #[test]
    fn child_exited_none_fields_serialize_as_null_never_coerced_to_zero() {
        let push = Push::ChildExited {
            session_id: "sess-3".to_string(),
            code: None,
            signal: None,
        };
        match map_push(push) {
            BrokerAction::Emit(event, payload) => {
                assert_eq!(event, EV_SESSION_EXITED);
                assert!(payload["code"].is_null());
                assert!(payload["signal"].is_null());
            }
            other => panic!("expected Emit, got {other:?}"),
        }
    }

    #[test]
    fn session_created_maps_to_session_created_event_with_raw_meta() {
        let meta = SessionMeta {
            id: "s".into(),
            workspace_id: "w".into(),
            title: "t".into(),
            shell: "/bin/zsh".into(),
            cwd: "/".into(),
            cols: 80,
            rows: 24,
            lifecycle: SessionLifecycle::AtPrompt,
            waiting_for_input: false,
            is_active: true,
            created_at: 0,
        };
        let action = map_push(Push::SessionCreated { meta: meta.clone() });
        match action {
            BrokerAction::Emit(event, payload) => {
                assert_eq!(event, EV_SESSION_CREATED);
                assert_eq!(payload["id"], "s");
                assert_eq!(payload["workspaceId"], "w");
            }
            other => panic!("expected Emit, got {other:?}"),
        }
    }

    #[test]
    fn workspace_created_maps_to_workspace_created_event_with_raw_workspace() {
        let ws = Workspace {
            id: "w1".into(),
            name: "N".into(),
            root_path: "/root".into(),
        };
        let action = map_push(Push::WorkspaceCreated { workspace: ws });
        match action {
            BrokerAction::Emit(event, payload) => {
                assert_eq!(event, EV_WORKSPACE_CREATED);
                assert_eq!(payload["id"], "w1");
                assert_eq!(payload["rootPath"], "/root");
            }
            other => panic!("expected Emit, got {other:?}"),
        }
    }

    #[test]
    fn error_push_maps_to_ignore() {
        let push = Push::Error {
            session_id: Some("s".into()),
            code: "boom".into(),
            message: "bad".into(),
        };
        assert_eq!(map_push(push), BrokerAction::Ignore);

        let push_no_session = Push::Error {
            session_id: None,
            code: "boom".into(),
            message: "bad".into(),
        };
        assert_eq!(map_push(push_no_session), BrokerAction::Ignore);
    }

    // ── map_conn_state: initial-connect vs reconnect tracking ──────────────────────────────

    #[test]
    fn initial_connected_does_not_fire_reconnected() {
        let mut seen = false;
        let ev = map_conn_state(ConnState::Connected, &mut seen);
        assert_eq!(ev, None);
        assert!(!seen);
    }

    #[test]
    fn disconnected_then_connected_fires_reconnected() {
        let mut seen = false;
        let ev1 = map_conn_state(ConnState::Disconnected, &mut seen);
        assert_eq!(ev1, Some(EV_DAEMON_DISCONNECTED));
        assert!(seen);

        let ev2 = map_conn_state(ConnState::Connected, &mut seen);
        assert_eq!(ev2, Some(EV_DAEMON_RECONNECTED));
        assert!(!seen, "flag must clear after firing reconnected");
    }

    #[test]
    fn repeated_disconnected_stays_disconnected_and_keeps_seen_true() {
        let mut seen = false;
        map_conn_state(ConnState::Disconnected, &mut seen);
        let ev = map_conn_state(ConnState::Disconnected, &mut seen);
        assert_eq!(ev, Some(EV_DAEMON_DISCONNECTED));
        assert!(seen);
    }

    #[test]
    fn connected_after_reconnected_without_new_disconnect_fires_nothing() {
        let mut seen = false;
        map_conn_state(ConnState::Disconnected, &mut seen);
        map_conn_state(ConnState::Connected, &mut seen); // -> reconnected, clears flag
        let ev = map_conn_state(ConnState::Connected, &mut seen);
        assert_eq!(ev, None);
    }

    // ── map_conn_state: mid-session Incompatible (finding [11], spec §6.2) ─────────────────

    #[test]
    fn incompatible_maps_to_daemon_incompatible_event() {
        let mut seen = false;
        let ev = map_conn_state(
            ConnState::Incompatible {
                daemon_min: 3,
                daemon_max: 3,
            },
            &mut seen,
        );
        assert_eq!(ev, Some(EV_DAEMON_INCOMPATIBLE));
    }

    #[test]
    fn incompatible_sets_seen_disconnected_so_a_later_recovery_fires_reconnected() {
        // The connection task has permanently died at this point (no more reconnect attempts will
        // happen on THIS client) — but if the whole app later recovers (manual restart, or the
        // upgrade flow's app.restart()) and a fresh client connects, that Connected transition
        // must still be recognized as "recovering from a bad state", i.e. fire reconnected, not
        // silently be treated as a fresh first-ever connect.
        let mut seen = false;
        map_conn_state(
            ConnState::Incompatible {
                daemon_min: 0,
                daemon_max: 0,
            },
            &mut seen,
        );
        assert!(seen, "Incompatible must set seen_disconnected");
    }
}
