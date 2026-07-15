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

use bpa_orchd_proto::OrchdPush;
use bpa_protocol::{Push, SessionId, TerminalEvent};
use serde::Serialize;
use tauri::ipc::Channel;
use tauri::{AppHandle, Emitter};
use tracing::{debug, warn};

use crate::commands::WriteStdinLocks;
use crate::orchd_client::{ConnState as OrchdConnState, OrchdClient};
use crate::socket_client::{ConnState, DaemonClient};

/// Global event names (spec §6.3). Kept as constants so this module and T18's frontend-facing
/// wiring (and tests) can never drift on the string literal.
pub const EV_SESSION_CREATED: &str = "session://created";
pub const EV_SESSION_STATE_CHANGED: &str = "session://state-changed";
pub const EV_SESSION_EXITED: &str = "session://exited";
pub const EV_WORKSPACE_CREATED: &str = "workspace://created";
/// Emitted after `Request::AddWorkspaceRoot`/`RemoveWorkspaceRoot` succeeds (spec §3.3/§6.6): the
/// daemon broadcasts `Push::WorkspaceUpdated` to every connected client (same
/// broadcast-to-all rationale as `EV_WORKSPACE_CREATED`) with the workspace's new `roots`.
pub const EV_WORKSPACE_UPDATED: &str = "workspace://updated";
pub const EV_DAEMON_DISCONNECTED: &str = "daemon://disconnected";
pub const EV_DAEMON_RECONNECTED: &str = "daemon://reconnected";
/// Emitted (no payload) when the handshake preamble (spec §4.5) finds the daemon's protocol range
/// incompatible with this client build — the signal that drives the upgrade flow (spec §6.2).
/// Distinct from `EV_DAEMON_DISCONNECTED`: a bounded reconnect can never resolve this on its own.
pub const EV_DAEMON_INCOMPATIBLE: &str = "daemon://incompatible";

/// `bpa-orchd` coarse-invalidation event names (spec §9, D10): deliberately kept under the
/// `orchd://` prefix rather than per-resource names (`project://changed`) — these are
/// daemon-scoped invalidation signals, not resource lifecycle events (D10's rationale), mirroring
/// the `daemon://…` trio's naming convention. Produced by [`map_orchd_push`]; consumed by
/// `lib.rs`'s `bring_up_orchd` wiring (via [`register_orchd`]).
pub const EV_ORCHD_PROJECTS_CHANGED: &str = "orchd://projects-changed";
/// Payload `{ projectId }`.
pub const EV_ORCHD_GOALS_CHANGED: &str = "orchd://goals-changed";
pub const EV_ORCHD_IDEAS_CHANGED: &str = "orchd://ideas-changed";
pub const EV_ORCHD_INSIGHTS_CHANGED: &str = "orchd://insights-changed";
/// Payload `{ projectId }`.
pub const EV_ORCHD_TASKS_CHANGED: &str = "orchd://tasks-changed";
/// Payload `{ scope, projectId? }`.
pub const EV_ORCHD_RULESET_CHANGED: &str = "orchd://ruleset-changed";
/// S4 knowledge graph (spec §3, appended — order FROZEN append-only). Payload `{ projectId }`.
pub const EV_ORCHD_GRAPH_CHANGED: &str = "orchd://graph-changed";
/// S-EXT MCP coarse-invalidation quartet (spec §5/§8, appended — order FROZEN append-only,
/// mirrors the `EV_ORCHD_GRAPH_CHANGED` precedent). Payload `{ projectId }` (`projectId` may be
/// `null` — a global-scope server/artifact change).
pub const EV_ORCHD_MCP_SERVERS_CHANGED: &str = "orchd://mcp-servers-changed";
/// Payload `{ serverId }`.
pub const EV_ORCHD_MCP_TOOLS_CHANGED: &str = "orchd://mcp-tools-changed";
/// Payload `{ projectId }` (may be `null`, see [`EV_ORCHD_MCP_SERVERS_CHANGED`]).
pub const EV_ORCHD_MCP_ARTIFACTS_CHANGED: &str = "orchd://mcp-artifacts-changed";
/// Payload `{ serverId }`.
pub const EV_ORCHD_MCP_INVOCATION_LOGGED: &str = "orchd://mcp-invocation-logged";
/// S-EXT connectors/accounts coarse-invalidation (spec §5/§7/§8, task T10, appended — order
/// FROZEN append-only). No payload (`null`) — the spec §4 `account` table has no `project_id`
/// column to scope by, same "nothing to name" precedent as `EV_ORCHD_PROJECTS_CHANGED` above.
pub const EV_ORCHD_CONNECTORS_CHANGED: &str = "orchd://connectors-changed";
/// orchd connection-state trio (spec §9): unlike [`EV_DAEMON_DISCONNECTED`]/
/// [`EV_DAEMON_RECONNECTED`] (which track "is this a reconnect after a disconnect" via
/// [`map_conn_state`]'s `seen_disconnected` flag), orchd's mapping ([`map_orchd_conn_state`]) is
/// a DIRECT 1:1 from `orchd_client::ConnState` — every `Connected` fires `orchd://up`, every
/// `Disconnected` fires `orchd://down`, a fatal `Incompatible` fires `orchd://incompatible`. The
/// frontend (S3 T13) tracks a plain `orchdDown` boolean rather than sessiond's richer
/// disconnected/reconnected distinction, so no reconnect-tracking state is needed here.
pub const EV_ORCHD_DOWN: &str = "orchd://down";
pub const EV_ORCHD_UP: &str = "orchd://up";
pub const EV_ORCHD_INCOMPATIBLE: &str = "orchd://incompatible";

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
/// - `Push::WorkspaceUpdated` -> `workspace://updated` with the raw `Workspace` payload (spec
///   §3.3/§6.6: fired after `Add`/`RemoveWorkspaceRoot`), same broadcast-to-all rationale.
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
        Push::WorkspaceUpdated(workspace) => {
            let payload = serde_json::to_value(workspace)
                .expect("Workspace is a plain-data struct; serialization cannot fail");
            BrokerAction::Emit(EV_WORKSPACE_UPDATED, payload)
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

/// Pure mapping table: `OrchdPush` variant -> `BrokerAction` (spec §9). Mirrors [`map_push`]'s
/// design exactly — no I/O, no Tauri runtime, fully unit-testable — but simpler: an `OrchdPush`
/// never maps to `SendChannel` (orchd has no PTY firehose / attach channels), so every arm is
/// `BrokerAction::Emit`. Fields already carry the wire's `project_id`/`scope` in Rust snake_case
/// (`OrchdPush` is Hop-B wire-only, not TS-exported — see `bpa_orchd_proto`'s module docs); the
/// JSON payloads built here are the camelCase reshaping the frontend actually consumes, same as
/// `map_push`'s `StateChanged`/`ChildExited` arms. Variants with no fields (`ProjectsChanged`/
/// `IdeasChanged`/`InsightsChanged`) emit a `null` payload rather than an empty object — there is
/// nothing to name.
pub fn map_orchd_push(push: OrchdPush) -> BrokerAction {
    match push {
        OrchdPush::ProjectsChanged => {
            BrokerAction::Emit(EV_ORCHD_PROJECTS_CHANGED, serde_json::Value::Null)
        }
        OrchdPush::GoalsChanged { project_id } => BrokerAction::Emit(
            EV_ORCHD_GOALS_CHANGED,
            serde_json::json!({ "projectId": project_id }),
        ),
        OrchdPush::IdeasChanged => {
            BrokerAction::Emit(EV_ORCHD_IDEAS_CHANGED, serde_json::Value::Null)
        }
        OrchdPush::InsightsChanged => {
            BrokerAction::Emit(EV_ORCHD_INSIGHTS_CHANGED, serde_json::Value::Null)
        }
        OrchdPush::TasksChanged { project_id } => BrokerAction::Emit(
            EV_ORCHD_TASKS_CHANGED,
            serde_json::json!({ "projectId": project_id }),
        ),
        OrchdPush::RuleSetChanged { scope, project_id } => BrokerAction::Emit(
            EV_ORCHD_RULESET_CHANGED,
            serde_json::json!({ "scope": scope, "projectId": project_id }),
        ),
        OrchdPush::GraphChanged { project_id } => BrokerAction::Emit(
            EV_ORCHD_GRAPH_CHANGED,
            serde_json::json!({ "projectId": project_id }),
        ),
        // S-EXT MCP (spec §5, appended — order FROZEN append-only), mirrors GraphChanged's
        // camelCase-reshape precedent above. `project_id` on `McpServersChanged`/
        // `McpArtifactsChanged` is `Option<String>` (global-scope changes carry `None`); `json!`
        // serializes that to JSON `null`, which the frontend's coarse-invalidation refetch (T8)
        // treats as "refresh everything", same as `RuleSetChanged`'s optional `project_id` above.
        OrchdPush::McpServersChanged { project_id } => BrokerAction::Emit(
            EV_ORCHD_MCP_SERVERS_CHANGED,
            serde_json::json!({ "projectId": project_id }),
        ),
        OrchdPush::McpToolsChanged { server_id } => BrokerAction::Emit(
            EV_ORCHD_MCP_TOOLS_CHANGED,
            serde_json::json!({ "serverId": server_id }),
        ),
        OrchdPush::McpArtifactsChanged { project_id } => BrokerAction::Emit(
            EV_ORCHD_MCP_ARTIFACTS_CHANGED,
            serde_json::json!({ "projectId": project_id }),
        ),
        OrchdPush::McpInvocationLogged { server_id } => BrokerAction::Emit(
            EV_ORCHD_MCP_INVOCATION_LOGGED,
            serde_json::json!({ "serverId": server_id }),
        ),
        // Finalized (task T10 wired the mapping shape; task T13a landed the `connector_*`
        // dispatch/commands that actually trigger this push — `ConnectorCompleteOAuth`/
        // `ConnectorAddApiKey`/`ConnectorDeleteAccount`, spec §5). No payload: the spec §4
        // `account` table has no `project_id` column to scope by.
        OrchdPush::ConnectorsChanged => {
            BrokerAction::Emit(EV_ORCHD_CONNECTORS_CHANGED, serde_json::Value::Null)
        }
    }
}

/// Pure mapping from an orchd `ConnState` transition to the event name [`Broker::
/// dispatch_orchd_conn`] should emit (spec §9). See [`EV_ORCHD_DOWN`]'s docs for why this is a
/// direct 1:1 mapping rather than [`map_conn_state`]'s reconnect-tracking scheme.
pub fn map_orchd_conn_state(state: OrchdConnState) -> &'static str {
    match state {
        OrchdConnState::Connected => EV_ORCHD_UP,
        OrchdConnState::Disconnected => EV_ORCHD_DOWN,
        OrchdConnState::Incompatible { .. } => EV_ORCHD_INCOMPATIBLE,
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

    /// Apply [`map_orchd_push`] and carry out its `BrokerAction` (spec §9). Mirrors
    /// [`dispatch_push`](Self::dispatch_push)'s non-blocking contract exactly — this is the body
    /// invoked (via [`register_orchd`]) from `OrchdClient::on_push`. An `OrchdPush` never produces
    /// `SendChannel`/`Ignore` (see [`map_orchd_push`]'s docs), but both are handled defensively
    /// (logged, not panicked) rather than assumed unreachable.
    pub fn dispatch_orchd_push(&self, push: OrchdPush) {
        match map_orchd_push(push) {
            BrokerAction::Emit(event, payload) => self.emit(event, payload),
            other => {
                warn!(target: "broker", ?other, "unexpected BrokerAction for an OrchdPush");
            }
        }
    }

    /// Apply [`map_orchd_conn_state`] and emit the resulting `orchd://down|up|incompatible` event
    /// (spec §9). This is the body invoked (via [`register_orchd`]) from `OrchdClient::on_conn`.
    pub fn dispatch_orchd_conn(&self, state: OrchdConnState) {
        self.emit_no_payload(map_orchd_conn_state(state));
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

/// Round-3 hardening (H2): evict the per-session `write_stdin` serialization lock when the daemon
/// reports the session's child exited — `Push::ChildExited` is the one signal the core observes
/// for EVERY way a session ends (natural exit, `KillSession` from this or any other client), so
/// hooking eviction here keeps `AppState.write_stdin_locks` shrinking in step with create/kill
/// churn instead of accumulating one entry per session-id-ever-written for the life of the
/// process. Pulled out as a plain function (rather than inlined in [`register`]'s closure) so it
/// is unit-testable without a Tauri `AppHandle` — the same testability seam rationale as
/// [`map_push`]/[`map_conn_state`]. Takes `&Push` (not `Push`) so [`register`]'s `on_push` closure
/// can run it BEFORE handing the same `Push` to `Broker::dispatch_push`, without a clone. Every
/// non-`ChildExited` push is a no-op. Non-blocking (a short `std::sync::Mutex` critical section in
/// `WriteStdinLocks::evict`), per the module-level on-connection-task contract.
pub(crate) fn evict_write_lock_on_child_exited(locks: &WriteStdinLocks, push: &Push) {
    if let Push::ChildExited { session_id, .. } = push {
        locks.evict(session_id);
    }
}

/// Wire a [`Broker`] into a `DaemonClient`'s push/conn callbacks (spec §7 correlation: `Push`
/// frames are fanned out by the broker; §13: conn transitions raise `daemon://disconnected`/
/// `daemon://reconnected`). Called once from T18's `setup()`, after both the `DaemonClient` and
/// the `Broker` (inside `AppState`) exist. `write_stdin_locks` is the SAME map `AppState` holds
/// (H2): the `on_push` closure evicts a session's write-serialization lock on its `ChildExited`
/// before dispatching, so the map cannot grow unboundedly across session churn.
///
/// `client.on_push`/`client.on_conn` callbacks run inline on `DaemonClient`'s connection task
/// (locked contract) — both closures below only take a short `std::sync::Mutex` lock and call
/// non-blocking `Channel::send`/`AppHandle::emit`, so neither one blocks that task.
pub fn register(
    broker: Arc<Broker>,
    client: &DaemonClient,
    write_stdin_locks: Arc<WriteStdinLocks>,
) {
    let push_broker = broker.clone();
    client.on_push(move |push| {
        evict_write_lock_on_child_exited(&write_stdin_locks, &push);
        push_broker.dispatch_push(push)
    });

    // One `seen_disconnected` flag per registration, captured by the `on_conn` closure. `on_conn`
    // replays the *current* `ConnState` synchronously before returning (T14 locked contract), so
    // this flag starts correctly: if the daemon is already down when `register` runs, the replayed
    // `Disconnected` sets it immediately; if already up (the normal initial-connect case), the
    // replayed `Connected` leaves it `false` and emits nothing (see `map_conn_state` docs).
    let seen_disconnected = std::sync::Mutex::new(false);
    client.on_conn(move |state| broker.dispatch_conn(state, &seen_disconnected));
}

/// Wire a [`Broker`] into an `OrchdClient`'s push/conn callbacks (spec §9) — mirrors [`register`]
/// exactly, for the second daemon: `on_push` -> [`Broker::dispatch_orchd_push`] (`map_orchd_push`
/// -> emit); `on_conn` -> [`Broker::dispatch_orchd_conn`] (`orchd://down|up|incompatible`).
/// Called once from `lib.rs`'s `bring_up_orchd`, after the `OrchdClient` connects — replaces the
/// T11 placeholder that only logged pushes via `tracing::debug!`. The SAME `Broker` instance
/// `register` wires sessiond into may be reused here: `Broker`'s orchd-facing methods only touch
/// `self.app` (never the sessiond-only `self.attachments` map), so there is no cross-daemon state
/// to keep separate.
///
/// `client.on_push`/`client.on_conn` callbacks run inline on `OrchdClient`'s connection task
/// (locked contract, see `orchd_client` module docs) — both closures below only call non-blocking
/// `AppHandle::emit`, so neither one blocks that task.
pub fn register_orchd(broker: Arc<Broker>, client: &OrchdClient) {
    let push_broker = broker.clone();
    client.on_push(move |push| push_broker.dispatch_orchd_push(push));
    client.on_conn(move |state| broker.dispatch_orchd_conn(state));
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
            roots: vec!["/root".into()],
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
    fn workspace_updated_maps_to_emit() {
        let ws = Workspace {
            id: "w1".into(),
            name: "N".into(),
            root_path: "/root".into(),
            roots: vec!["/root".into(), "/root2".into()],
        };
        let action = map_push(Push::WorkspaceUpdated(ws));
        match action {
            BrokerAction::Emit(event, payload) => {
                assert_eq!(event, EV_WORKSPACE_UPDATED);
                assert_eq!(payload["id"], "w1");
                assert_eq!(payload["rootPath"], "/root");
                assert_eq!(payload["roots"], serde_json::json!(["/root", "/root2"]));
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

    // ── map_orchd_push: exhaustive, one test per OrchdPush variant (spec §9, S3 T12) ──────

    #[test]
    fn orchd_projects_changed_maps_to_emit_with_null_payload() {
        let action = map_orchd_push(OrchdPush::ProjectsChanged);
        match action {
            BrokerAction::Emit(event, payload) => {
                assert_eq!(event, EV_ORCHD_PROJECTS_CHANGED);
                assert!(payload.is_null());
            }
            other => panic!("expected Emit, got {other:?}"),
        }
    }

    #[test]
    fn orchd_goals_changed_maps_to_emit_with_camel_case_project_id_payload() {
        let action = map_orchd_push(OrchdPush::GoalsChanged {
            project_id: "proj-1".to_string(),
        });
        match action {
            BrokerAction::Emit(event, payload) => {
                assert_eq!(event, EV_ORCHD_GOALS_CHANGED);
                assert_eq!(payload["projectId"], "proj-1");
                // snake_case key must NOT leak through.
                assert!(payload.get("project_id").is_none());
            }
            other => panic!("expected Emit, got {other:?}"),
        }
    }

    #[test]
    fn orchd_ideas_changed_maps_to_emit_with_null_payload() {
        let action = map_orchd_push(OrchdPush::IdeasChanged);
        match action {
            BrokerAction::Emit(event, payload) => {
                assert_eq!(event, EV_ORCHD_IDEAS_CHANGED);
                assert!(payload.is_null());
            }
            other => panic!("expected Emit, got {other:?}"),
        }
    }

    #[test]
    fn orchd_insights_changed_maps_to_emit_with_null_payload() {
        let action = map_orchd_push(OrchdPush::InsightsChanged);
        match action {
            BrokerAction::Emit(event, payload) => {
                assert_eq!(event, EV_ORCHD_INSIGHTS_CHANGED);
                assert!(payload.is_null());
            }
            other => panic!("expected Emit, got {other:?}"),
        }
    }

    #[test]
    fn orchd_tasks_changed_maps_to_emit_with_camel_case_project_id_payload() {
        let action = map_orchd_push(OrchdPush::TasksChanged {
            project_id: "proj-2".to_string(),
        });
        match action {
            BrokerAction::Emit(event, payload) => {
                assert_eq!(event, EV_ORCHD_TASKS_CHANGED);
                assert_eq!(payload["projectId"], "proj-2");
                assert!(payload.get("project_id").is_none());
            }
            other => panic!("expected Emit, got {other:?}"),
        }
    }

    #[test]
    fn orchd_ruleset_changed_maps_to_emit_with_scope_and_project_id_payload() {
        let action = map_orchd_push(OrchdPush::RuleSetChanged {
            scope: bpa_orchd_proto::RuleScope::Project,
            project_id: Some("proj-3".to_string()),
        });
        match action {
            BrokerAction::Emit(event, payload) => {
                assert_eq!(event, EV_ORCHD_RULESET_CHANGED);
                assert_eq!(payload["scope"], "project");
                assert_eq!(payload["projectId"], "proj-3");
            }
            other => panic!("expected Emit, got {other:?}"),
        }
    }

    #[test]
    fn orchd_ruleset_changed_global_scope_has_null_project_id() {
        let action = map_orchd_push(OrchdPush::RuleSetChanged {
            scope: bpa_orchd_proto::RuleScope::Global,
            project_id: None,
        });
        match action {
            BrokerAction::Emit(event, payload) => {
                assert_eq!(event, EV_ORCHD_RULESET_CHANGED);
                assert_eq!(payload["scope"], "global");
                assert!(payload["projectId"].is_null());
            }
            other => panic!("expected Emit, got {other:?}"),
        }
    }

    #[test]
    fn orchd_graph_changed_maps_to_emit_with_camel_case_project_id_payload() {
        let action = map_orchd_push(OrchdPush::GraphChanged {
            project_id: "proj-1".to_string(),
        });
        match action {
            BrokerAction::Emit(event, payload) => {
                assert_eq!(event, EV_ORCHD_GRAPH_CHANGED);
                assert_eq!(payload["projectId"], "proj-1");
                // snake_case key must NOT leak through.
                assert!(payload.get("project_id").is_none());
            }
            other => panic!("expected Emit, got {other:?}"),
        }
    }

    // ── map_orchd_push: S-EXT MCP quartet, one test per OrchdPush::Mcp* variant (spec §5,
    // S-EXT T7) — mirrors the GoalsChanged/GraphChanged camelCase-payload tests above exactly ──

    #[test]
    fn orchd_mcp_servers_changed_maps_to_emit_with_camel_case_project_id_payload() {
        let action = map_orchd_push(OrchdPush::McpServersChanged {
            project_id: Some("proj-1".to_string()),
        });
        match action {
            BrokerAction::Emit(event, payload) => {
                assert_eq!(event, EV_ORCHD_MCP_SERVERS_CHANGED);
                assert_eq!(payload["projectId"], "proj-1");
                assert!(payload.get("project_id").is_none());
            }
            other => panic!("expected Emit, got {other:?}"),
        }
    }

    #[test]
    fn orchd_mcp_servers_changed_global_scope_has_null_project_id() {
        let action = map_orchd_push(OrchdPush::McpServersChanged { project_id: None });
        match action {
            BrokerAction::Emit(event, payload) => {
                assert_eq!(event, EV_ORCHD_MCP_SERVERS_CHANGED);
                assert!(payload["projectId"].is_null());
            }
            other => panic!("expected Emit, got {other:?}"),
        }
    }

    #[test]
    fn orchd_mcp_tools_changed_maps_to_emit_with_camel_case_server_id_payload() {
        let action = map_orchd_push(OrchdPush::McpToolsChanged {
            server_id: "srv-1".to_string(),
        });
        match action {
            BrokerAction::Emit(event, payload) => {
                assert_eq!(event, EV_ORCHD_MCP_TOOLS_CHANGED);
                assert_eq!(payload["serverId"], "srv-1");
                assert!(payload.get("server_id").is_none());
            }
            other => panic!("expected Emit, got {other:?}"),
        }
    }

    #[test]
    fn orchd_mcp_artifacts_changed_maps_to_emit_with_camel_case_project_id_payload() {
        let action = map_orchd_push(OrchdPush::McpArtifactsChanged {
            project_id: Some("proj-2".to_string()),
        });
        match action {
            BrokerAction::Emit(event, payload) => {
                assert_eq!(event, EV_ORCHD_MCP_ARTIFACTS_CHANGED);
                assert_eq!(payload["projectId"], "proj-2");
                assert!(payload.get("project_id").is_none());
            }
            other => panic!("expected Emit, got {other:?}"),
        }
    }

    #[test]
    fn orchd_mcp_invocation_logged_maps_to_emit_with_camel_case_server_id_payload() {
        let action = map_orchd_push(OrchdPush::McpInvocationLogged {
            server_id: "srv-2".to_string(),
        });
        match action {
            BrokerAction::Emit(event, payload) => {
                assert_eq!(event, EV_ORCHD_MCP_INVOCATION_LOGGED);
                assert_eq!(payload["serverId"], "srv-2");
                assert!(payload.get("server_id").is_none());
            }
            other => panic!("expected Emit, got {other:?}"),
        }
    }

    // ── map_orchd_push: S-EXT connectors/accounts (spec §5/§7/§8, task T10) — mirrors the
    // ProjectsChanged/IdeasChanged/InsightsChanged null-payload precedent above exactly ────────

    #[test]
    fn orchd_connectors_changed_maps_to_emit_with_null_payload() {
        let action = map_orchd_push(OrchdPush::ConnectorsChanged);
        match action {
            BrokerAction::Emit(event, payload) => {
                assert_eq!(event, EV_ORCHD_CONNECTORS_CHANGED);
                assert!(payload.is_null());
            }
            other => panic!("expected Emit, got {other:?}"),
        }
    }

    // ── map_orchd_conn_state: direct 1:1 mapping (spec §9, S3 T12) ─────────────────────────

    #[test]
    fn map_orchd_conn_state_maps_every_variant() {
        assert_eq!(map_orchd_conn_state(OrchdConnState::Connected), EV_ORCHD_UP);
        assert_eq!(
            map_orchd_conn_state(OrchdConnState::Disconnected),
            EV_ORCHD_DOWN
        );
        assert_eq!(
            map_orchd_conn_state(OrchdConnState::Incompatible {
                daemon_min: 2,
                daemon_max: 2,
            }),
            EV_ORCHD_INCOMPATIBLE
        );
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

    // ── H2 (round-3 hardening): write_stdin lock eviction on ChildExited ───────────────────
    //
    // `evict_write_lock_on_child_exited` is exactly what `register`'s `on_push` closure runs on
    // every delivered push, before `dispatch_push` — the broker path, minus the AppHandle-bound
    // dispatch (a `Broker` needs a real Tauri `AppHandle`, unconstructable in `cargo test`; see
    // `commands.rs`'s AppState-test rationale). Entries are created here through `lock_for`, the
    // same call `write_stdin_locked` makes on a write.

    #[test]
    fn child_exited_evicts_only_that_sessions_write_lock() {
        let locks = WriteStdinLocks::new();
        let _dead = locks.lock_for(&"dead".to_string()); // a write created this entry
        let _live = locks.lock_for(&"live".to_string()); // another session was written too
        assert!(locks.contains(&"dead".to_string()));
        assert!(locks.contains(&"live".to_string()));

        evict_write_lock_on_child_exited(
            &locks,
            &Push::ChildExited {
                session_id: "dead".to_string(),
                code: Some(0),
                signal: None,
            },
        );

        assert!(
            !locks.contains(&"dead".to_string()),
            "ChildExited must evict the exited session's write lock entry"
        );
        assert!(
            locks.contains(&"live".to_string()),
            "a still-live session's entry must be untouched"
        );
    }

    #[test]
    fn non_child_exited_pushes_never_evict() {
        let locks = WriteStdinLocks::new();
        let _entry = locks.lock_for(&"s1".to_string());

        evict_write_lock_on_child_exited(
            &locks,
            &Push::Output {
                session_id: "s1".to_string(),
                bytes: vec![1, 2, 3],
            },
        );
        evict_write_lock_on_child_exited(
            &locks,
            &Push::StateChanged {
                session_id: "s1".to_string(),
                lifecycle: SessionLifecycle::Running,
                waiting_for_input: false,
                cwd: "/".to_string(),
            },
        );
        evict_write_lock_on_child_exited(
            &locks,
            &Push::Error {
                session_id: Some("s1".to_string()),
                code: "boom".into(),
                message: "bad".into(),
            },
        );

        assert!(
            locks.contains(&"s1".to_string()),
            "only ChildExited may evict a session's write lock entry"
        );
    }

    #[test]
    fn child_exited_for_an_unknown_session_is_a_harmless_noop() {
        let locks = WriteStdinLocks::new();
        let _entry = locks.lock_for(&"s1".to_string());

        // A ChildExited for a session never written to (no entry) must not panic or disturb
        // other entries — e.g. another client's session this GUI never typed into.
        evict_write_lock_on_child_exited(
            &locks,
            &Push::ChildExited {
                session_id: "never-written".to_string(),
                code: None,
                signal: Some("SIGKILL".to_string()),
            },
        );
        assert!(locks.contains(&"s1".to_string()));
    }

    #[test]
    fn late_write_after_eviction_recreates_a_fresh_entry() {
        // The documented (harmless) re-creation path: a write racing the eviction simply puts a
        // fresh entry back — the daemon rejects the write anyway (session gone), and different-
        // session concurrency is unaffected. This locks the "no panic / no stale Arc reuse"
        // shape of that race.
        let locks = WriteStdinLocks::new();
        let first = locks.lock_for(&"s".to_string());
        evict_write_lock_on_child_exited(
            &locks,
            &Push::ChildExited {
                session_id: "s".to_string(),
                code: Some(1),
                signal: None,
            },
        );
        assert!(!locks.contains(&"s".to_string()));

        let second = locks.lock_for(&"s".to_string());
        assert!(locks.contains(&"s".to_string()));
        assert!(
            !Arc::ptr_eq(&first, &second),
            "a post-eviction write must get a FRESH lock, not the evicted one"
        );
    }
}
