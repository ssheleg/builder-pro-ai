//! The `#[tauri::command]` surface (spec §6.1): thin webview-facing wrappers over the daemon
//! request/response round-trip, plus the one CORE-ONLY command (`pick_folder`) that never reaches
//! the daemon.
//!
//! Every brokered command follows the same shape: build a `bpa_protocol::Request` from the
//! command's args, `state.client()?.request(req).await` (the `?` surfaces `CommandError::
//! Disconnected` when the slot is empty — daemon not up, or a fatal `IncompatibleDaemon`), unwrap
//! the expected `Response` variant (or turn a mismatched/`Error` variant into a typed
//! `CommandError`). The request-building and response-unwrapping halves are pulled out as plain
//! functions (`build_*` / `expect_*`) so they are unit-testable without a Tauri runtime or a live
//! socket; the `#[tauri::command]` fns themselves are exercised against a real `DaemonClient`
//! talking to a stub daemon (see the `commands_over_stub_daemon` test module), reusing the T14
//! stub-daemon pattern.
//!
//! ## `orchd_*` surface (spec §9, S3 T12)
//!
//! Mirrors the shape above exactly, over the second daemon: `state.orchd()?.request(req).await`
//! (`state.orchd()` mirrors `state.client()`), then the matching `expect_*` unwrapper. `bpa-orchd`
//! already converts its own wire `OrchdResponse::Error` into `Err(OrchdClientError::Daemon)`
//! inside `OrchdClient::request` (see `orchd_client.rs`'s `run_connection`), so by the time an
//! `orchd_*` command sees `Ok(response)` it can never be `OrchdResponse::Error` — `?` on
//! `.request(..).await` alone surfaces that case as `CommandError::Daemon` via the `From<
//! OrchdClientError>` impl below. `err_from_orchd_response` still matches `OrchdResponse::Error`
//! defensively (belt-and-suspenders, same as `err_from_response` above), never as the only path.

use std::collections::BTreeMap;
use std::sync::Arc;

use bpa_orchd_proto::{
    Account, AuditRow, ConnectorOp, DomainTask, FitVerdict, Goal, GoalKind, GoalStatus, GraphEdge,
    GraphEdgeKind, GraphNeighborhood, GraphNode, GraphNodeKind, GraphView, Idea, IdeaLifecycle,
    Insight, InsightStatus, McpArtifact, McpAuthKind, McpCallResult, McpConnectReport,
    McpInvocation, McpScope, McpServer, McpTool, McpTransport, OAuthChallenge, OrchdRequest,
    OrchdResponse, Policy, PolicyRules, PolicyScope, Project, ResearchRun, RuleScope, RuleSetView,
    Skill, SkillScope, StorageStatus, TaskSource, TaskStatus,
};
use bpa_protocol::{
    CommandEvent, Request, Response, SessionId, SessionMeta, TerminalEvent, Workspace, WorkspaceId,
};
use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::AppHandle;
use tauri::State;
use tauri_plugin_dialog::DialogExt;

use crate::broker::Broker;
use crate::orchd_client::{OrchdClient, OrchdClientError, OrchdClientSlot};
use crate::socket_client::{ClientError, DaemonClient};

/// The daemon client slot: `None` while disconnected/incompatible, `Some` once a connection is
/// live. Swappable (rather than a fixed `Arc<DaemonClient>`) because a fatal `IncompatibleDaemon`
/// leaves the old client's `connection_task` dead (its `cmd_tx` is closed) — there is no way to
/// "reconnect" the same `DaemonClient` in place. The upgrade flow (spec §6.2) instead restarts the
/// whole process (`upgrade_daemon` -> `app.restart()`); the slot exists so `AppState` can always be
/// `manage`d (even before any connection succeeds) and commands can gate on `client()` returning
/// `Disconnected` instead of a `State<AppState>` extraction panicking.
pub type ClientSlot = std::sync::Arc<std::sync::RwLock<Option<std::sync::Arc<DaemonClient>>>>;

/// Queryable daemon connection status (finding [12], spec §6.2): a pull-based fallback for the
/// single-shot `daemon://incompatible`/`daemon://disconnected`/`daemon://reconnected` events. The
/// boot-time `daemon://incompatible` emit races webview `listen()` registration — if it fires
/// before React mounts, it is lost forever (Tauri events are not replayed to late subscribers) and
/// the upgrade flow becomes unreachable. `daemon_status` lets the frontend poll the current truth
/// instead of depending solely on catching that one emit.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum DaemonStatus {
    Connected,
    Disconnected,
    /// Per-variant `rename_all` kept even though only `daemon_min`/`daemon_max` exist here (Task-8
    /// lesson, same as `CommandError::IncompatibleDaemon`): the container's `rename_all` does NOT
    /// cascade into struct-variant fields.
    #[serde(rename_all = "camelCase")]
    Incompatible {
        daemon_min: u16,
        daemon_max: u16,
    },
}

/// Shared, mutex-guarded slot for the current [`DaemonStatus`], written at every place the truth
/// changes (both of `bring_up_daemon`'s outcomes in `lib.rs`, and the second `on_conn` callback
/// registered alongside the broker's) and read by the `daemon_status` command.
pub type StatusSlot = Arc<std::sync::Mutex<DaemonStatus>>;

/// Queryable orchd connection status (S3 T11, spec §9): the same pull-based-fallback shape as
/// [`DaemonStatus`], for the second daemon (`bpa-orchd`). Written by `lib.rs`'s `bring_up_orchd`
/// at every place the truth changes, mirroring `bring_up_daemon`'s `DaemonStatus` wiring exactly.
/// A dedicated `orchd_status` command (and the matching frontend poll) is out of this task's
/// scope — this type exists now so `AppState`'s shape is locked once, not re-shaped by a later
/// task — but the slot is already kept live and correct from the very first connect attempt.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum OrchdStatus {
    Connected,
    Disconnected,
    /// Field names match `OrchdClientError::IncompatibleOrchd`/`orchd_client::ConnState::
    /// Incompatible` exactly (`daemon_min`/`daemon_max`, spec §9's locked shape), so the mapping
    /// in `lib.rs` copies them straight across with no renaming. Per-variant `rename_all` kept
    /// even so (same Task-8 lesson as `DaemonStatus::Incompatible`): the container's
    /// `rename_all` does NOT cascade into struct-variant fields.
    #[serde(rename_all = "camelCase")]
    Incompatible {
        daemon_min: u16,
        daemon_max: u16,
    },
}

/// Shared, mutex-guarded slot for the current [`OrchdStatus`] — same shape/rationale as
/// [`StatusSlot`].
pub type OrchdStatusSlot = Arc<std::sync::Mutex<OrchdStatus>>;

/// Per-session write-serialization locks (round-2 regression R3): a chunked `write_stdin` (finding
/// [2]/C4) sends several sequential `Request::WriteStdin` frames on the shared connection, awaiting
/// each `Ack` before sending the next. The frontend fires `writeStdin` fire-and-forget per xterm
/// `onData` with no client-side serialization, and Tauri runs overlapping `#[tauri::command]`
/// invocations concurrently — so a second `write_stdin` call for the SAME session (a fast keystroke,
/// or a second paste) invoked while a multi-chunk paste is still in flight can enqueue its own
/// `WriteStdin` request on the client's FIFO command channel BETWEEN two chunks of the first call,
/// corrupting byte order at the PTY. Holding this session's lock across the whole chunk loop makes a
/// chunked write atomic with respect to every other write to the SAME session, while writes to
/// DIFFERENT sessions stay fully concurrent (each session gets its own `tokio::sync::Mutex`).
///
/// Keyed by `SessionId` behind a `std::sync::Mutex` (not `tokio::sync::Mutex`): the outer map is
/// only ever touched for the instant it takes to look up/insert an `Arc<tokio::sync::Mutex<()>>`,
/// never held across an `.await` — the actual serialization happens on the per-session
/// `tokio::sync::Mutex` returned from `lock_for`.
#[derive(Default)]
pub struct WriteStdinLocks {
    inner: std::sync::Mutex<std::collections::HashMap<SessionId, Arc<tokio::sync::Mutex<()>>>>,
}

impl WriteStdinLocks {
    pub fn new() -> Self {
        Self::default()
    }

    /// Return the per-session lock for `session_id`, creating it on first use. Entries live until
    /// the session ends: the broker evicts them on `Push::ChildExited` (round-3 hardening H2, see
    /// [`crate::broker::register`]) so a days-long process with create/kill churn cannot
    /// accumulate one map entry per session-id-ever-written forever. `pub(crate)` (not private)
    /// so the broker's eviction tests can create entries the same way `write_stdin_locked` does.
    pub(crate) fn lock_for(&self, session_id: &SessionId) -> Arc<tokio::sync::Mutex<()>> {
        let mut map = self.inner.lock().unwrap();
        map.entry(session_id.clone())
            .or_insert_with(|| Arc::new(tokio::sync::Mutex::new(())))
            .clone()
    }

    /// Drop `session_id`'s lock entry (round-3 hardening H2): called from the broker's `on_push`
    /// wiring when the daemon reports `Push::ChildExited` for the session, so the map shrinks in
    /// step with session churn instead of growing unboundedly. Removing while a concurrent
    /// `write_stdin_locked` still holds the `Arc` is safe — that guard keeps its own clone of the
    /// `Arc<tokio::sync::Mutex<()>>` alive until it drops, and a late write after eviction simply
    /// re-creates a fresh entry via `lock_for` (harmless: the session is gone, so that write fails
    /// `Disconnected`/`NoSuchSession` at the daemon anyway). Runs inline on the `DaemonClient`
    /// connection task (locked contract, see `broker.rs` module docs) — a short,
    /// never-held-across-`.await` `std::sync::Mutex` critical section, same as `lock_for`.
    pub(crate) fn evict(&self, session_id: &SessionId) {
        self.inner.lock().unwrap().remove(session_id);
    }

    /// Whether an entry currently exists for `session_id` — test observability for the H2
    /// eviction contract (`broker.rs`'s eviction tests).
    #[cfg(test)]
    pub(crate) fn contains(&self, session_id: &SessionId) -> bool {
        self.inner.lock().unwrap().contains_key(session_id)
    }
}

/// Shared, Tauri-managed application state: the swappable daemon-client slot, the push broker, the
/// launchd agent (used by `upgrade_daemon`), the pull-queryable daemon status (finding [12]), and
/// the per-session `write_stdin` serialization locks (round-2 regression R3). Constructed once in
/// `lib.rs`'s `setup()` and **always** registered via `app.manage(...)` — unlike the old design,
/// `AppState` is never left unmanaged, even when the daemon is down or speaks an incompatible
/// protocol version (spec §6.2).
///
/// `orchd`/`orchd_launchd`/`orchd_status` (S3 T11, spec §9) mirror `client`/`launchd`/`status`
/// exactly, for the second daemon (`bpa-orchd`) `lib.rs`'s `bring_up_orchd` brings up alongside
/// `bring_up_daemon` — added now (additively) so the orchd command surface + `orchd_upgrade`
/// (later tasks) land on an already-locked `AppState` shape rather than reshaping it again.
pub struct AppState {
    pub client: ClientSlot,
    pub broker: Arc<Broker>,
    pub launchd: Arc<crate::launchd::LaunchdAgent<'static>>,
    pub status: StatusSlot,
    pub write_stdin_locks: Arc<WriteStdinLocks>,
    pub orchd: OrchdClientSlot,
    pub orchd_launchd: Arc<crate::launchd::LaunchdAgent<'static>>,
    pub orchd_status: OrchdStatusSlot,
}

/// The exact logic behind `AppState::client()`, pulled out as a free function over a bare
/// `ClientSlot` so it is unit-testable without needing a full `AppState` (which requires an
/// `Arc<Broker>` — buildable only from a real Tauri `AppHandle`, unavailable in a `cargo test`
/// process without a genuine OS event loop on the main thread). `AppState::client()` below is a
/// one-line delegate to this, so the two share the identical, real code path.
pub(crate) fn slot_client(slot: &ClientSlot) -> Result<Arc<DaemonClient>, CommandError> {
    slot.read()
        .unwrap()
        .clone()
        .ok_or(CommandError::Disconnected)
}

/// The exact logic behind `AppState::orchd()` — mirrors [`slot_client`] exactly, for the second
/// daemon (S3 T12, spec §9).
pub(crate) fn slot_orchd(slot: &OrchdClientSlot) -> Result<Arc<OrchdClient>, CommandError> {
    slot.read()
        .unwrap()
        .clone()
        .ok_or(CommandError::Disconnected)
}

/// Read the current [`DaemonStatus`] out of a bare `StatusSlot` — pulled out as a free function for
/// the same unit-testability reason as `slot_client` above.
pub(crate) fn read_status(slot: &StatusSlot) -> DaemonStatus {
    slot.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Write a new [`DaemonStatus`] into a bare `StatusSlot`.
pub(crate) fn write_status(slot: &StatusSlot, status: DaemonStatus) {
    *slot.lock().unwrap_or_else(|e| e.into_inner()) = status;
}

/// Write a new [`OrchdStatus`] into a bare `OrchdStatusSlot` — mirrors `write_status` exactly, for
/// the second daemon (S3 T11, spec §9). No `read_orchd_status` counterpart yet: a dedicated
/// `orchd_status` command (mirroring `daemon_status`) is out of this task's scope — this function
/// exists because `bring_up_orchd` already needs to WRITE the truth at every connect/reconnect
/// transition, exactly like `write_status` does for sessiond from day one.
pub(crate) fn write_orchd_status(slot: &OrchdStatusSlot, status: OrchdStatus) {
    *slot.lock().unwrap_or_else(|e| e.into_inner()) = status;
}

impl AppState {
    /// Current live client, or `Disconnected` if the slot is empty (daemon not up / incompatible).
    /// Clones the `Arc` and drops the read guard BEFORE returning, so callers never hold the lock
    /// across an `.await`.
    pub fn client(&self) -> Result<Arc<DaemonClient>, CommandError> {
        slot_client(&self.client)
    }
    pub fn set_client(&self, c: Option<Arc<DaemonClient>>) {
        *self.client.write().unwrap() = c;
    }

    /// Current live orchd client, or `Disconnected` if the slot is empty — mirrors
    /// [`AppState::client`] exactly, for the second daemon (S3 T12, spec §9).
    pub fn orchd(&self) -> Result<Arc<OrchdClient>, CommandError> {
        slot_orchd(&self.orchd)
    }
}

/// Options for `create_session`. `env_overrides` defaults to `[]`; the frontend normally omits it
/// entirely (it exists because S6 agents drive this surface too — spec §6.1).
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CreateOpts {
    #[serde(default)]
    pub shell: Option<String>,
    #[serde(default)]
    pub cwd: Option<String>,
    #[serde(default)]
    pub env_overrides: Vec<(String, String)>,
    #[serde(default)]
    pub cols: Option<u16>,
    #[serde(default)]
    pub rows: Option<u16>,
}

/// If `cols`/`rows` are omitted, the core sends the default **80x24** (spec §6.1); the frontend
/// passes a real size after the first `fitAddon.fit()`, then calls `resize()`.
pub fn resolve_size(opts: &CreateOpts) -> (u16, u16) {
    (opts.cols.unwrap_or(80), opts.rows.unwrap_or(24))
}

/// Error surfaced to the webview. Serializes so `invoke()` rejects the JS Promise with a typed
/// shape the frontend can match on (rather than an opaque string).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum CommandError {
    /// A typed daemon-side `Response::Error { code, message }`, or a local pre-flight validation
    /// failure reshaped into the same wire shape (e.g. `create_workspace`'s path check).
    Daemon { code: String, message: String },
    /// The daemon socket is not currently connected (spec §13: honest degradation — never a fake
    /// success).
    Disconnected,
    /// An unexpected local failure: a daemon reply of the wrong `Response` variant, a closed
    /// dialog channel, etc. Never used to paper over a real daemon error.
    Internal(String),
    /// The handshake preamble (spec §4.5) found the daemon's protocol range incompatible with this
    /// client build (or the handshake reply couldn't be trusted at all). Distinct from
    /// `Disconnected`: this is the signal that drives the upgrade flow (spec §6.2), not something a
    /// bounded reconnect can ever resolve on its own.
    #[serde(rename_all = "camelCase")]
    IncompatibleDaemon { daemon_min: u16, daemon_max: u16 },
    /// The daemon upgrade flow's `launchctl kickstart -k` failed (spec §6.2.4: an honest failure
    /// banner, never a fake success). `reason` is one word so `rename_all` is a no-op here, but the
    /// per-variant attr is kept anyway per the Task-8 lesson: the container's `rename_all` does NOT
    /// cascade into struct-variant fields.
    #[serde(rename_all = "camelCase")]
    UpgradeFailed { reason: String },
    /// A single request, once CBOR-encoded, exceeded `bpa_protocol::MAX_FRAME_LEN` (finding [2]:
    /// e.g. an ~8.4 MiB `write_stdin` paste). Distinct from `Internal`/`Daemon`: this is a LOCAL,
    /// per-request encode failure caught before the socket write, never a daemon-side rejection or
    /// a dead connection — the connection stays alive and every other request keeps working.
    /// `size` is the encoded CBOR body length in bytes.
    #[serde(rename_all = "camelCase")]
    TooLarge { size: usize },
    /// ADDITIVE (S3 T12, spec §9): the second daemon's (`bpa-orchd`) handshake preamble found its
    /// protocol range incompatible with this client build. Mirrors `IncompatibleDaemon` exactly,
    /// distinct field names (`orchd_min`/`orchd_max`) so the frontend can tell the two daemons'
    /// upgrade flows apart when both fire (spec §10/§11: sessiond's dialog takes precedence, but
    /// both flags must be independently readable).
    #[serde(rename_all = "camelCase")]
    IncompatibleOrchd { orchd_min: u16, orchd_max: u16 },
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CommandError::Daemon { code, message } => write!(f, "daemon error [{code}]: {message}"),
            CommandError::Disconnected => write!(f, "daemon disconnected"),
            CommandError::Internal(m) => write!(f, "internal error: {m}"),
            CommandError::IncompatibleDaemon {
                daemon_min,
                daemon_max,
            } => write!(
                f,
                "incompatible daemon (daemon supports [{daemon_min}, {daemon_max}])"
            ),
            CommandError::UpgradeFailed { reason } => write!(f, "daemon upgrade failed: {reason}"),
            CommandError::TooLarge { size } => {
                write!(f, "request too large once encoded ({size} bytes)")
            }
            CommandError::IncompatibleOrchd {
                orchd_min,
                orchd_max,
            } => write!(
                f,
                "incompatible orchd (orchd supports [{orchd_min}, {orchd_max}])"
            ),
        }
    }
}

impl std::error::Error for CommandError {}

impl From<ClientError> for CommandError {
    fn from(e: ClientError) -> Self {
        match e {
            ClientError::Disconnected => CommandError::Disconnected,
            ClientError::Daemon { code, message } => CommandError::Daemon { code, message },
            ClientError::IncompatibleDaemon {
                daemon_min,
                daemon_max,
            } => CommandError::IncompatibleDaemon {
                daemon_min,
                daemon_max,
            },
            ClientError::RequestTooLarge { size } => CommandError::TooLarge { size },
        }
    }
}

/// Mirrors `From<ClientError> for CommandError` exactly, for the second daemon (S3 T12, spec §9).
/// `OrchdClientError::Daemon { code, message }` already carries `code` as `OrchdErrorCode`'s
/// `Debug` name (e.g. `"Invariant"`, set by `orchd_client.rs`'s own `run_connection`) — spec §9's
/// locked code strings — so it copies straight across, same as `ClientError::Daemon` does today.
impl From<OrchdClientError> for CommandError {
    fn from(e: OrchdClientError) -> Self {
        match e {
            OrchdClientError::Disconnected => CommandError::Disconnected,
            OrchdClientError::Daemon { code, message } => CommandError::Daemon { code, message },
            OrchdClientError::IncompatibleOrchd {
                daemon_min,
                daemon_max,
            } => CommandError::IncompatibleOrchd {
                orchd_min: daemon_min,
                orchd_max: daemon_max,
            },
            OrchdClientError::RequestTooLarge { size } => CommandError::TooLarge { size },
        }
    }
}

// ── pure request builders (unit-tested without a socket) ───────────────────────────────────

pub(crate) fn build_create_session(workspace_id: WorkspaceId, opts: Option<CreateOpts>) -> Request {
    let opts = opts.unwrap_or_default();
    let (cols, rows) = resolve_size(&opts);
    Request::CreateSession {
        workspace_id,
        shell: opts.shell,
        cwd: opts.cwd,
        env_overrides: opts.env_overrides,
        cols,
        rows,
    }
}

/// Chunk size for `write_stdin` (finding [2]): 1 MiB of raw bytes CBOR-encodes to at most ~2 MiB
/// (CBOR encodes `Vec<u8>` as an array of unsigned integers — worst case every byte costs 2 wire
/// bytes), far under `bpa_protocol::MAX_FRAME_LEN` (16 MiB), leaving generous headroom for the rest
/// of the `Request::WriteStdin` frame envelope.
pub(crate) const WRITE_STDIN_CHUNK: usize = 1024 * 1024;

/// Split `data`'s raw UTF-8 bytes into `Request::WriteStdin` requests of at most
/// `WRITE_STDIN_CHUNK` bytes each (finding [2]: a single large paste, once CBOR-encoded, can exceed
/// the 16 MiB frame cap and tear down the whole connection). Splits on raw byte boundaries (not
/// UTF-8 char boundaries) — the daemon/PTY treat `WriteStdin`'s `bytes` as an opaque byte stream, so
/// a chunk boundary landing mid-multi-byte-codepoint is harmless: the two chunks are written to the
/// same PTY fd back-to-back, in order (`write_stdin`'s caller awaits each chunk sequentially on the
/// same connection before sending the next, so FIFO delivery order is preserved), and the shell/PTY
/// reassembles the byte stream exactly as if it had arrived in one write. Returns a single
/// (possibly empty-bytes) request for input at or under the chunk size, so the common case (a
/// normal keystroke or small paste) is still exactly one `Request::WriteStdin`, same as before
/// chunking existed.
pub(crate) fn build_write_stdin_chunks(session_id: SessionId, data: String) -> Vec<Request> {
    let bytes = data.into_bytes();
    if bytes.len() <= WRITE_STDIN_CHUNK {
        return vec![Request::WriteStdin { session_id, bytes }];
    }
    bytes
        .chunks(WRITE_STDIN_CHUNK)
        .map(|chunk| Request::WriteStdin {
            session_id: session_id.clone(),
            bytes: chunk.to_vec(),
        })
        .collect()
}

/// Send `data` to `session_id`'s stdin, chunked (`build_write_stdin_chunks`), while holding that
/// session's write-serialization lock (round-2 regression R3) across the whole chunk loop — so a
/// concurrent call for the SAME session (a keystroke or a second paste racing a multi-chunk paste,
/// both possible since Tauri runs overlapping command invocations and the frontend does not
/// serialize `writeStdin` calls itself) cannot interleave its own `WriteStdin` request between two
/// chunks of this one. A different session's lock is untouched, so writes to different sessions
/// never block each other. Pulled out as a plain function (rather than inlined in the
/// `#[tauri::command]`) so it is unit-testable against a real `DaemonClient` + stub daemon without a
/// Tauri runtime, mirroring `build_write_stdin_chunks`'s own testability rationale.
pub(crate) async fn write_stdin_locked(
    locks: &WriteStdinLocks,
    client: &DaemonClient,
    session_id: SessionId,
    data: String,
) -> Result<(), CommandError> {
    let lock = locks.lock_for(&session_id);
    let _guard = lock.lock().await;
    for req in build_write_stdin_chunks(session_id.clone(), data) {
        expect_ack(client.request(req).await?)?;
    }
    Ok(())
}

// ── core-side path pre-flights (spec §13/§16 defense in depth) ─────────────────────────────
//
// The daemon is the security-authoritative validator (S6 agents drive the same surface), but the
// core validates too so a bad path fails fast BEFORE a socket round-trip. These are pulled out as
// pure functions so they can be unit-tested directly — the guard logic itself, not a reconstruction
// of its output — and reused byte-identically by the `#[tauri::command]` wrappers.

/// Pre-flight for `create_session`: validate an explicitly-provided cwd BEFORE brokering. `None` or
/// an empty-string cwd ⇒ `Ok(())` (the daemon defaults an omitted cwd to `$HOME`, so the core must
/// not validate-and-reject that). A present, non-empty cwd is run through the shared
/// [`crate::paths::validate_dir`]; any failure is reshaped into a `CommandError::Daemon` carrying
/// the path error's stable wire code. The daemon re-validates and canonicalizes independently.
pub(crate) fn preflight_cwd(opts: &Option<CreateOpts>) -> Result<(), CommandError> {
    if let Some(cwd) = opts
        .as_ref()
        .and_then(|o| o.cwd.as_deref())
        .filter(|c| !c.is_empty())
    {
        crate::paths::validate_dir(std::path::Path::new(cwd)).map_err(|e| {
            CommandError::Daemon {
                code: e.code().to_string(),
                message: e.to_string(),
            }
        })?;
    }
    Ok(())
}

/// Pre-flight for `create_workspace`: validate AND canonicalize `root_path` BEFORE brokering.
/// Returns the canonicalized path as a `String` (exactly what the wrapper forwards to the daemon),
/// or a `CommandError::Daemon` carrying the path error's stable wire code. The daemon re-validates
/// independently (defense in depth for S6 agents driving the same surface).
pub(crate) fn preflight_workspace_root(root_path: &str) -> Result<String, CommandError> {
    let validated = crate::paths::validate_dir(std::path::Path::new(root_path)).map_err(|e| {
        CommandError::Daemon {
            code: e.code().to_string(),
            message: e.to_string(),
        }
    })?;
    Ok(validated.to_string_lossy().into_owned())
}

// ── response unwrappers: map the expected variant, or a typed error ────────────────────────

fn err_from_response(res: Response) -> CommandError {
    match res {
        Response::Error { code, message } => CommandError::Daemon { code, message },
        other => CommandError::Internal(format!("unexpected daemon response: {other:?}")),
    }
}

pub(crate) fn expect_session(res: Response) -> Result<SessionMeta, CommandError> {
    match res {
        Response::Session(m) => Ok(m),
        other => Err(err_from_response(other)),
    }
}

fn expect_sessions(res: Response) -> Result<Vec<SessionMeta>, CommandError> {
    match res {
        Response::Sessions(v) => Ok(v),
        other => Err(err_from_response(other)),
    }
}

fn expect_workspace(res: Response) -> Result<Workspace, CommandError> {
    match res {
        Response::Workspace(w) => Ok(w),
        other => Err(err_from_response(other)),
    }
}

fn expect_workspaces(res: Response) -> Result<Vec<Workspace>, CommandError> {
    match res {
        Response::Workspaces(v) => Ok(v),
        other => Err(err_from_response(other)),
    }
}

fn expect_command_events(res: Response) -> Result<Vec<CommandEvent>, CommandError> {
    match res {
        Response::CommandEvents(v) => Ok(v),
        other => Err(err_from_response(other)),
    }
}

fn expect_ack(res: Response) -> Result<(), CommandError> {
    match res {
        Response::Ack => Ok(()),
        other => Err(err_from_response(other)),
    }
}

// ── orchd response unwrappers (spec §9, S3 T12) — mirrors the block above exactly ──────────

/// Aggregate per-family row counts from a successful `orchd_import_bundle` (spec §4.2's
/// `OrchdResponse::ImportReport { projects, goals, ideas, insights, tasks, rulesets }`), re-shaped
/// into its own camelCase-serialized struct: `OrchdResponse`'s frame variants are Hop-B wire-only
/// (plain Rust snake_case, never TS-exported — see `bpa_orchd_proto`'s module docs), not something
/// a `#[tauri::command]` should hand the webview directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub projects: u32,
    pub goals: u32,
    pub ideas: u32,
    pub insights: u32,
    pub tasks: u32,
    pub rulesets: u32,
}

/// Mirrors `err_from_response` exactly, for `OrchdResponse` (spec §9). `OrchdResponse::Error` is
/// handled here defensively — see this module's top-level doc for why it is never the ONLY path
/// an orchd daemon error reaches `CommandError::Daemon` through.
fn err_from_orchd_response(res: OrchdResponse) -> CommandError {
    match res {
        OrchdResponse::Error { code, message } => CommandError::Daemon {
            code: format!("{code:?}"),
            message,
        },
        other => CommandError::Internal(format!("unexpected orchd response: {other:?}")),
    }
}

fn expect_pong(res: OrchdResponse) -> Result<(), CommandError> {
    match res {
        OrchdResponse::Pong => Ok(()),
        other => Err(err_from_orchd_response(other)),
    }
}

fn expect_project(res: OrchdResponse) -> Result<Project, CommandError> {
    match res {
        OrchdResponse::Project(p) => Ok(p),
        other => Err(err_from_orchd_response(other)),
    }
}

fn expect_projects(res: OrchdResponse) -> Result<Vec<Project>, CommandError> {
    match res {
        OrchdResponse::Projects(v) => Ok(v),
        other => Err(err_from_orchd_response(other)),
    }
}

fn expect_goal(res: OrchdResponse) -> Result<Goal, CommandError> {
    match res {
        OrchdResponse::Goal(g) => Ok(g),
        other => Err(err_from_orchd_response(other)),
    }
}

fn expect_goals(res: OrchdResponse) -> Result<Vec<Goal>, CommandError> {
    match res {
        OrchdResponse::Goals(v) => Ok(v),
        other => Err(err_from_orchd_response(other)),
    }
}

fn expect_idea(res: OrchdResponse) -> Result<Idea, CommandError> {
    match res {
        OrchdResponse::Idea(i) => Ok(i),
        other => Err(err_from_orchd_response(other)),
    }
}

fn expect_ideas(res: OrchdResponse) -> Result<Vec<Idea>, CommandError> {
    match res {
        OrchdResponse::Ideas(v) => Ok(v),
        other => Err(err_from_orchd_response(other)),
    }
}

fn expect_insight(res: OrchdResponse) -> Result<Insight, CommandError> {
    match res {
        OrchdResponse::Insight(i) => Ok(i),
        other => Err(err_from_orchd_response(other)),
    }
}

fn expect_insights(res: OrchdResponse) -> Result<Vec<Insight>, CommandError> {
    match res {
        OrchdResponse::Insights(v) => Ok(v),
        other => Err(err_from_orchd_response(other)),
    }
}

/// Named to avoid clashing with `expect_ack`-adjacent sessiond naming, mirroring
/// `bpa_orchd_proto::DomainTask`'s own "avoid the `tokio::task` clash" rationale.
fn expect_domain_task(res: OrchdResponse) -> Result<DomainTask, CommandError> {
    match res {
        OrchdResponse::Task(t) => Ok(t),
        other => Err(err_from_orchd_response(other)),
    }
}

fn expect_domain_tasks(res: OrchdResponse) -> Result<Vec<DomainTask>, CommandError> {
    match res {
        OrchdResponse::Tasks(v) => Ok(v),
        other => Err(err_from_orchd_response(other)),
    }
}

fn expect_rule_set_view(res: OrchdResponse) -> Result<RuleSetView, CommandError> {
    match res {
        OrchdResponse::RuleSetView(v) => Ok(v),
        other => Err(err_from_orchd_response(other)),
    }
}

fn expect_export_json(res: OrchdResponse) -> Result<String, CommandError> {
    match res {
        OrchdResponse::ExportJson(s) => Ok(s),
        other => Err(err_from_orchd_response(other)),
    }
}

fn expect_import_report(res: OrchdResponse) -> Result<ImportReport, CommandError> {
    match res {
        OrchdResponse::ImportReport {
            projects,
            goals,
            ideas,
            insights,
            tasks,
            rulesets,
        } => Ok(ImportReport {
            projects,
            goals,
            ideas,
            insights,
            tasks,
            rulesets,
        }),
        other => Err(err_from_orchd_response(other)),
    }
}

fn expect_orchd_ack(res: OrchdResponse) -> Result<(), CommandError> {
    match res {
        OrchdResponse::Ack => Ok(()),
        other => Err(err_from_orchd_response(other)),
    }
}

// ── S4 knowledge-graph orchd response unwrappers (spec §3, appended) — mirrors the block above
// exactly, one unwrapper per `OrchdResponse::Graph*`/`Neighborhood`/`GraphNodes` variant ─────────

fn expect_graph_node(res: OrchdResponse) -> Result<GraphNode, CommandError> {
    match res {
        OrchdResponse::GraphNode(n) => Ok(n),
        other => Err(err_from_orchd_response(other)),
    }
}

fn expect_graph_edge(res: OrchdResponse) -> Result<GraphEdge, CommandError> {
    match res {
        OrchdResponse::GraphEdge(e) => Ok(e),
        other => Err(err_from_orchd_response(other)),
    }
}

fn expect_graph_view(res: OrchdResponse) -> Result<GraphView, CommandError> {
    match res {
        OrchdResponse::GraphView(v) => Ok(v),
        other => Err(err_from_orchd_response(other)),
    }
}

fn expect_neighborhood(res: OrchdResponse) -> Result<GraphNeighborhood, CommandError> {
    match res {
        OrchdResponse::Neighborhood(n) => Ok(n),
        other => Err(err_from_orchd_response(other)),
    }
}

fn expect_graph_nodes(res: OrchdResponse) -> Result<Vec<GraphNode>, CommandError> {
    match res {
        OrchdResponse::GraphNodes(v) => Ok(v),
        other => Err(err_from_orchd_response(other)),
    }
}

// ── S-EXT MCP orchd response unwrappers (spec §5, appended) — mirrors the S4 graph block
// exactly, one unwrapper per `OrchdResponse::Mcp*` variant ────────────────────────────────────

fn expect_mcp_server(res: OrchdResponse) -> Result<McpServer, CommandError> {
    match res {
        OrchdResponse::McpServer(s) => Ok(s),
        other => Err(err_from_orchd_response(other)),
    }
}

fn expect_mcp_servers(res: OrchdResponse) -> Result<Vec<McpServer>, CommandError> {
    match res {
        OrchdResponse::McpServers(v) => Ok(v),
        other => Err(err_from_orchd_response(other)),
    }
}

fn expect_mcp_tool(res: OrchdResponse) -> Result<McpTool, CommandError> {
    match res {
        OrchdResponse::McpTool(t) => Ok(t),
        other => Err(err_from_orchd_response(other)),
    }
}

fn expect_mcp_tools(res: OrchdResponse) -> Result<Vec<McpTool>, CommandError> {
    match res {
        OrchdResponse::McpTools(v) => Ok(v),
        other => Err(err_from_orchd_response(other)),
    }
}

fn expect_mcp_connect_report(res: OrchdResponse) -> Result<McpConnectReport, CommandError> {
    match res {
        OrchdResponse::McpConnectReport(r) => Ok(r),
        other => Err(err_from_orchd_response(other)),
    }
}

fn expect_mcp_call_result(res: OrchdResponse) -> Result<McpCallResult, CommandError> {
    match res {
        OrchdResponse::McpCallResult(r) => Ok(r),
        other => Err(err_from_orchd_response(other)),
    }
}

fn expect_mcp_invocations(res: OrchdResponse) -> Result<Vec<McpInvocation>, CommandError> {
    match res {
        OrchdResponse::McpInvocations(v) => Ok(v),
        other => Err(err_from_orchd_response(other)),
    }
}

fn expect_mcp_artifacts(res: OrchdResponse) -> Result<Vec<McpArtifact>, CommandError> {
    match res {
        OrchdResponse::McpArtifacts(v) => Ok(v),
        other => Err(err_from_orchd_response(other)),
    }
}

fn expect_mcp_artifact(res: OrchdResponse) -> Result<McpArtifact, CommandError> {
    match res {
        OrchdResponse::McpArtifact(a) => Ok(a),
        other => Err(err_from_orchd_response(other)),
    }
}

// ── S-EXT Connector orchd response unwrappers (spec §5, appended, task T13a) — mirrors the MCP
// block above exactly, one unwrapper per `OrchdResponse::{Account,Accounts,OAuthChallenge,
// ConnectorOps}` variant ─────────────────────────────────────────────────────────────────────

fn expect_account(res: OrchdResponse) -> Result<Account, CommandError> {
    match res {
        OrchdResponse::Account(a) => Ok(a),
        other => Err(err_from_orchd_response(other)),
    }
}

fn expect_accounts(res: OrchdResponse) -> Result<Vec<Account>, CommandError> {
    match res {
        OrchdResponse::Accounts(v) => Ok(v),
        other => Err(err_from_orchd_response(other)),
    }
}

fn expect_oauth_challenge(res: OrchdResponse) -> Result<OAuthChallenge, CommandError> {
    match res {
        OrchdResponse::OAuthChallenge(c) => Ok(c),
        other => Err(err_from_orchd_response(other)),
    }
}

fn expect_connector_ops(res: OrchdResponse) -> Result<Vec<ConnectorOp>, CommandError> {
    match res {
        OrchdResponse::ConnectorOps(v) => Ok(v),
        other => Err(err_from_orchd_response(other)),
    }
}

// ── S-EXT Skills orchd response unwrappers (spec §5, D11, Q14, appended, task T17) — mirrors the
// MCP/Connector blocks above exactly, one unwrapper per `OrchdResponse::Skill*` variant ─────────

fn expect_skill(res: OrchdResponse) -> Result<Skill, CommandError> {
    match res {
        OrchdResponse::Skill(s) => Ok(s),
        other => Err(err_from_orchd_response(other)),
    }
}

fn expect_skills(res: OrchdResponse) -> Result<Vec<Skill>, CommandError> {
    match res {
        OrchdResponse::Skills(v) => Ok(v),
        other => Err(err_from_orchd_response(other)),
    }
}

// ── S-EXT Trust orchd response unwrappers (spec §4/§5/§6, BL-22, appended, task T18) — mirrors
// the Skills block above exactly, one unwrapper per `OrchdResponse::{Policy,Policies,AuditRows}`
// variant ─────────────────────────────────────────────────────────────────────────────────────

fn expect_policy(res: OrchdResponse) -> Result<Policy, CommandError> {
    match res {
        OrchdResponse::Policy(p) => Ok(p),
        other => Err(err_from_orchd_response(other)),
    }
}

fn expect_policies(res: OrchdResponse) -> Result<Vec<Policy>, CommandError> {
    match res {
        OrchdResponse::Policies(v) => Ok(v),
        other => Err(err_from_orchd_response(other)),
    }
}

fn expect_audit_rows(res: OrchdResponse) -> Result<Vec<AuditRow>, CommandError> {
    match res {
        OrchdResponse::AuditRows(v) => Ok(v),
        other => Err(err_from_orchd_response(other)),
    }
}

// ── S-IDEA research orchd response unwrappers (spec §5/§6, task T5) — mirrors the Trust block
// above exactly, one unwrapper per `OrchdResponse::{ResearchRun,ResearchRuns}` variant ─────────

fn expect_research_run(res: OrchdResponse) -> Result<ResearchRun, CommandError> {
    match res {
        OrchdResponse::ResearchRun(r) => Ok(r),
        other => Err(err_from_orchd_response(other)),
    }
}

fn expect_research_runs(res: OrchdResponse) -> Result<Vec<ResearchRun>, CommandError> {
    match res {
        OrchdResponse::ResearchRuns(v) => Ok(v),
        other => Err(err_from_orchd_response(other)),
    }
}

fn expect_storage_status(res: OrchdResponse) -> Result<StorageStatus, CommandError> {
    match res {
        OrchdResponse::StorageStatus(s) => Ok(s),
        other => Err(err_from_orchd_response(other)),
    }
}

// ── #[tauri::command] surface (spec §6.1) ───────────────────────────────────────────────────

#[tauri::command]
pub async fn create_session(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    opts: Option<CreateOpts>,
) -> Result<SessionMeta, CommandError> {
    // Defense in depth (spec §13/§16): reject an invalid cwd BEFORE brokering, mirroring
    // create_workspace's root_path pre-flight. `preflight_cwd` skips None/empty (the daemon defaults
    // those to $HOME); the daemon re-validates (and canonicalizes) independently.
    preflight_cwd(&opts)?;
    let req = build_create_session(workspace_id, opts);
    expect_session(state.client()?.request(req).await?)
}

#[tauri::command]
pub async fn list_sessions(state: State<'_, AppState>) -> Result<Vec<SessionMeta>, CommandError> {
    expect_sessions(state.client()?.request(Request::ListSessions).await?)
}

#[tauri::command]
pub async fn attach_session(
    state: State<'_, AppState>,
    session_id: SessionId,
    on_event: Channel<TerminalEvent>,
) -> Result<(), CommandError> {
    // Register the channel BEFORE asking the daemon to attach, so the first Push::Replay it sends
    // is delivered rather than raced (spec §7 reattach flow).
    state
        .broker
        .register_attachment(session_id.clone(), on_event);
    match state
        .client()?
        .request(Request::AttachSession {
            session_id: session_id.clone(),
        })
        .await
    {
        Ok(res) => expect_ack(res).inspect_err(|_e| {
            // Attach rejected by the daemon: drop the just-registered channel so it doesn't leak.
            state.broker.remove_attachment(&session_id);
        }),
        Err(e) => {
            state.broker.remove_attachment(&session_id);
            Err(e.into())
        }
    }
}

#[tauri::command]
pub async fn detach_session(
    state: State<'_, AppState>,
    session_id: SessionId,
) -> Result<(), CommandError> {
    let out = expect_ack(
        state
            .client()?
            .request(Request::DetachSession {
                session_id: session_id.clone(),
            })
            .await?,
    );
    state.broker.remove_attachment(&session_id);
    out
}

#[tauri::command]
pub async fn write_stdin(
    state: State<'_, AppState>,
    session_id: SessionId,
    data: String,
) -> Result<(), CommandError> {
    // Chunked (finding [2]): a single oversized paste, once CBOR-encoded, could exceed the 16 MiB
    // frame cap and tear down the whole daemon connection. Send each ≤1 MiB chunk as a sequential
    // `Request::WriteStdin` on the SAME connection — awaiting each one before sending the next
    // preserves FIFO order at the PTY. On any chunk failure, stop immediately and surface that
    // error honestly rather than silently dropping the remaining chunks or retrying.
    //
    // Serialized per session (round-2 regression R3): `write_stdin_locked` holds this session's
    // lock across the whole chunk loop so a concurrent call for the SAME session cannot interleave
    // a request between two chunks of this one; a different session's write proceeds unblocked.
    let client = state.client()?;
    write_stdin_locked(&state.write_stdin_locks, &client, session_id, data).await
}

#[tauri::command]
pub async fn resize(
    state: State<'_, AppState>,
    session_id: SessionId,
    cols: u16,
    rows: u16,
) -> Result<(), CommandError> {
    expect_ack(
        state
            .client()?
            .request(Request::Resize {
                session_id,
                cols,
                rows,
            })
            .await?,
    )
}

#[tauri::command]
pub async fn kill_session(
    state: State<'_, AppState>,
    session_id: SessionId,
) -> Result<(), CommandError> {
    let out = expect_ack(
        state
            .client()?
            .request(Request::KillSession {
                session_id: session_id.clone(),
            })
            .await?,
    );
    state.broker.remove_attachment(&session_id);
    out
}

#[tauri::command]
pub async fn list_workspaces(state: State<'_, AppState>) -> Result<Vec<Workspace>, CommandError> {
    expect_workspaces(state.client()?.request(Request::ListWorkspaces).await?)
}

#[tauri::command]
pub async fn create_workspace(
    state: State<'_, AppState>,
    name: String,
    root_path: String,
) -> Result<Workspace, CommandError> {
    // Fail fast on an invalid root BEFORE touching the daemon (spec §13/§16); the daemon
    // re-validates independently (defense in depth for S6 agents driving the same surface).
    let root_path = preflight_workspace_root(&root_path)?;
    expect_workspace(
        state
            .client()?
            .request(Request::CreateWorkspace { name, root_path })
            .await?,
    )
}

/// Add an additional root directory to a workspace (spec §3.3/§6.6, multi-root). The daemon
/// re-validates `path` (canonicalizes, rejects paths that aren't existing directories)
/// independently, and DEDUPS IDEMPOTENTLY: adding a path that's already one of the workspace's
/// current roots is a no-op success (returns the unchanged `Workspace`, not an error, and never
/// persists a second identical root — see `bpa_sessiond::persistence::Db::add_workspace_root`) —
/// this wrapper itself is deliberately thin, same shape as `create_workspace`/`list_workspaces`.
/// On success the daemon also broadcasts `Push::WorkspaceUpdated` (see
/// `broker::EV_WORKSPACE_UPDATED`) to every connected client, including this one — even on the
/// idempotent no-op path, since it's a harmless resync rather than a spurious change notification;
/// the returned `Workspace` here is this caller's direct reply, not sourced from that broadcast.
#[tauri::command]
pub async fn add_workspace_root(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    path: String,
) -> Result<Workspace, CommandError> {
    expect_workspace(
        state
            .client()?
            .request(Request::AddWorkspaceRoot { workspace_id, path })
            .await?,
    )
}

/// Remove a root directory from a workspace (spec §3.3/§6.6). The daemon rejects removing a
/// workspace's last remaining root (`CommandError::Daemon { code: "LastRoot", .. }` — a workspace
/// always has at least one root) rather than silently emptying it.
#[tauri::command]
pub async fn remove_workspace_root(
    state: State<'_, AppState>,
    workspace_id: WorkspaceId,
    path: String,
) -> Result<Workspace, CommandError> {
    expect_workspace(
        state
            .client()?
            .request(Request::RemoveWorkspaceRoot { workspace_id, path })
            .await?,
    )
}

/// Fetch a session's recent command lifecycle events (spec §3.3, Pv2 §7 `command_events` table),
/// newest-first, capped at `limit`.
#[tauri::command]
pub async fn get_command_events(
    state: State<'_, AppState>,
    session_id: SessionId,
    limit: u32,
) -> Result<Vec<CommandEvent>, CommandError> {
    expect_command_events(
        state
            .client()?
            .request(Request::GetCommandEvents { session_id, limit })
            .await?,
    )
}

#[tauri::command]
pub async fn get_session_state(
    state: State<'_, AppState>,
    session_id: SessionId,
) -> Result<SessionMeta, CommandError> {
    expect_session(
        state
            .client()?
            .request(Request::GetSessionState { session_id })
            .await?,
    )
}

/// CORE-ONLY (finding [12], spec §6.2): pull-based fallback for the single-shot
/// `daemon://incompatible`/`daemon://disconnected`/`daemon://reconnected` events, which can be
/// lost if they fire before the webview's `listen()` registrations complete (Tauri events are not
/// replayed to late subscribers). Never errors in practice — always returns `Ok` with whatever the
/// status slot currently holds, so the frontend can poll it on mount/reconnect-retry without
/// needing its own error-recovery path for this specific call.
#[tauri::command]
pub async fn daemon_status(state: State<'_, AppState>) -> Result<DaemonStatus, CommandError> {
    Ok(read_status(&state.status))
}

/// CORE-ONLY (spec §6.1): the native folder picker must run in the GUI process, never brokered to
/// the daemon — there is deliberately no `Request` variant for it (see
/// `pick_folder_is_core_only_no_daemon_request` below). Returns the chosen absolute path, or
/// `None` if the user canceled the dialog.
#[tauri::command]
pub async fn pick_folder(app: AppHandle) -> Result<Option<String>, CommandError> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog().file().pick_folder(move |maybe_path| {
        let _ = tx.send(maybe_path);
    });
    let chosen = rx.await.map_err(|e| {
        CommandError::Internal(format!(
            "dialog channel closed before a result arrived: {e}"
        ))
    })?;
    Ok(chosen.map(|p| p.to_string()))
}

/// CORE-ONLY (spec §6.1, mirrors `pick_folder` above verbatim — same "the native file dialog must
/// run in the GUI process, never brokered to the daemon" rationale): the file picker `SkillsTab`
/// (S-EXT §8, D11, task T17) uses to let the owner choose an existing SKILL.md. Filtered to `.md`
/// files (`add_filter`) — the owner still picks the exact file, this just narrows the dialog's
/// default view; `skill_add`'s own `md_path` validation (`bpa_orchd::skills::registry`) is the
/// actual security boundary, not this filter. Returns the chosen absolute path, or `None` if the
/// user canceled the dialog.
#[tauri::command]
pub async fn pick_skill_file(app: AppHandle) -> Result<Option<String>, CommandError> {
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .add_filter("SKILL.md", &["md"])
        .set_title("Select SKILL.md")
        .pick_file(move |maybe_path| {
            let _ = tx.send(maybe_path);
        });
    let chosen = rx.await.map_err(|e| {
        CommandError::Internal(format!(
            "dialog channel closed before a result arrived: {e}"
        ))
    })?;
    Ok(chosen.map(|p| p.to_string()))
}

/// Core of the daemon-upgrade flow (spec §6.2): best-effort drain the current client (if any),
/// then force the launchd-managed daemon to relaunch with the new bundled binary. Pulled out as a
/// plain async fn (not a `#[tauri::command]`) so it is unit-testable with a mock-runner
/// `LaunchdAgent` and no Tauri runtime — mirrors the rest of this module's "pure helpers are
/// unit-testable" pattern.
///
/// (a) The drain is best-effort and its error is swallowed: an OLD daemon (the dominant trigger
/// for this flow) cannot parse the v2 `DaemonShutdown` frame at all, so any failure here is
/// expected and must never block the upgrade. (b) The kickstart is the opposite — its failure is
/// the one honest, surfaced error (spec §6.2.4): never claim success when `launchctl kickstart -k`
/// actually failed.
///
/// Uses [`crate::launchd::LaunchdAgent::kickstart_force`] (`-k`) deliberately, NOT the plain
/// boot-path `kickstart()`: this function only ever runs from the `upgrade_daemon` command, which
/// is gated behind the T10b consent dialog — the user has already agreed that the running
/// daemon's live sessions will end. The boot path must never reach this function.
pub async fn upgrade_daemon_core(
    client: Option<Arc<DaemonClient>>,
    agent: &crate::launchd::LaunchdAgent<'_>,
) -> Result<(), CommandError> {
    if let Some(c) = client {
        let _ = c.request(Request::DaemonShutdown { drain: true }).await;
    }
    agent
        .kickstart_force()
        .map_err(|e| CommandError::UpgradeFailed {
            reason: e.to_string(),
        })?;
    Ok(())
}

/// `#[tauri::command]` entry point for the upgrade flow (spec §6.2): drains the current session
/// (best-effort), force-kickstarts the daemon, then relaunches the whole app via
/// `AppHandle::restart()` — re-running `setup()` from scratch so the fresh process negotiates the
/// (now-matching) protocol version with the newly-kickstarted daemon and rehydrates its inactive
/// sessions through the normal startup path. `restart()` never returns (`-> !`, macOS-bundle-aware:
/// it reads `Info.plist` to find the updated binary rather than assuming the on-disk path is
/// unchanged) — the frontend (Task 10b) must not await a resolved result from this command's
/// `invoke()` call.
#[tauri::command]
pub async fn upgrade_daemon(
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), CommandError> {
    let client = state.client.read().unwrap().clone();
    upgrade_daemon_core(client, &state.launchd).await?;
    app.restart();
}

// ── orchd #[tauri::command] surface (spec §9, §4.2 — S3 T12) ───────────────────────────────
//
// One thin command per `OrchdRequest` verb, named `orchd_` + snake_case verb (locked, spec §9).
// `OrchdRequest::OrchdShutdown` has NO command here (it's internal-only, used by
// `orchd_upgrade_core` below) — every other verb gets exactly one.

#[tauri::command]
pub async fn orchd_ping(state: State<'_, AppState>) -> Result<(), CommandError> {
    expect_pong(state.orchd()?.request(OrchdRequest::Ping).await?)
}

#[tauri::command]
pub async fn orchd_create_project(
    state: State<'_, AppState>,
    name: String,
    description: String,
    workspace_ids: Vec<String>,
) -> Result<Project, CommandError> {
    expect_project(
        state
            .orchd()?
            .request(OrchdRequest::CreateProject {
                name,
                description,
                workspace_ids,
            })
            .await?,
    )
}

#[tauri::command]
pub async fn orchd_update_project(
    state: State<'_, AppState>,
    id: String,
    name: Option<String>,
    description: Option<String>,
) -> Result<Project, CommandError> {
    expect_project(
        state
            .orchd()?
            .request(OrchdRequest::UpdateProject {
                id,
                name,
                description,
            })
            .await?,
    )
}

#[tauri::command]
pub async fn orchd_archive_project(
    state: State<'_, AppState>,
    id: String,
) -> Result<Project, CommandError> {
    expect_project(
        state
            .orchd()?
            .request(OrchdRequest::ArchiveProject { id })
            .await?,
    )
}

#[tauri::command]
pub async fn orchd_list_projects(state: State<'_, AppState>) -> Result<Vec<Project>, CommandError> {
    expect_projects(state.orchd()?.request(OrchdRequest::ListProjects).await?)
}

#[tauri::command]
pub async fn orchd_add_project_workspace(
    state: State<'_, AppState>,
    project_id: String,
    workspace_id: String,
) -> Result<Project, CommandError> {
    expect_project(
        state
            .orchd()?
            .request(OrchdRequest::AddProjectWorkspace {
                project_id,
                workspace_id,
            })
            .await?,
    )
}

#[tauri::command]
pub async fn orchd_remove_project_workspace(
    state: State<'_, AppState>,
    project_id: String,
    workspace_id: String,
) -> Result<Project, CommandError> {
    expect_project(
        state
            .orchd()?
            .request(OrchdRequest::RemoveProjectWorkspace {
                project_id,
                workspace_id,
            })
            .await?,
    )
}

#[tauri::command]
pub async fn orchd_create_goal(
    state: State<'_, AppState>,
    project_id: String,
    parent_id: Option<String>,
    kind: GoalKind,
    title: String,
    body: String,
) -> Result<Goal, CommandError> {
    expect_goal(
        state
            .orchd()?
            .request(OrchdRequest::CreateGoal {
                project_id,
                parent_id,
                kind,
                title,
                body,
            })
            .await?,
    )
}

#[tauri::command]
pub async fn orchd_update_goal(
    state: State<'_, AppState>,
    id: String,
    title: Option<String>,
    body: Option<String>,
    status: Option<GoalStatus>,
    metric_refs: Option<Vec<String>>,
) -> Result<Goal, CommandError> {
    expect_goal(
        state
            .orchd()?
            .request(OrchdRequest::UpdateGoal {
                id,
                title,
                body,
                status,
                metric_refs,
            })
            .await?,
    )
}

#[tauri::command]
pub async fn orchd_move_goal(
    state: State<'_, AppState>,
    id: String,
    new_parent_id: Option<String>,
    new_ord: i64,
) -> Result<Goal, CommandError> {
    expect_goal(
        state
            .orchd()?
            .request(OrchdRequest::MoveGoal {
                id,
                new_parent_id,
                new_ord,
            })
            .await?,
    )
}

#[tauri::command]
pub async fn orchd_delete_goal(state: State<'_, AppState>, id: String) -> Result<(), CommandError> {
    expect_orchd_ack(
        state
            .orchd()?
            .request(OrchdRequest::DeleteGoal { id })
            .await?,
    )
}

#[tauri::command]
pub async fn orchd_list_goals(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<Vec<Goal>, CommandError> {
    expect_goals(
        state
            .orchd()?
            .request(OrchdRequest::ListGoals { project_id })
            .await?,
    )
}

#[tauri::command]
pub async fn orchd_create_idea(
    state: State<'_, AppState>,
    project_id: Option<String>,
    title: String,
    body: String,
) -> Result<Idea, CommandError> {
    expect_idea(
        state
            .orchd()?
            .request(OrchdRequest::CreateIdea {
                project_id,
                title,
                body,
            })
            .await?,
    )
}

#[tauri::command]
pub async fn orchd_update_idea(
    state: State<'_, AppState>,
    id: String,
    title: Option<String>,
    body: Option<String>,
) -> Result<Idea, CommandError> {
    expect_idea(
        state
            .orchd()?
            .request(OrchdRequest::UpdateIdea { id, title, body })
            .await?,
    )
}

#[tauri::command]
pub async fn orchd_set_idea_project(
    state: State<'_, AppState>,
    id: String,
    project_id: Option<String>,
) -> Result<Idea, CommandError> {
    expect_idea(
        state
            .orchd()?
            .request(OrchdRequest::SetIdeaProject { id, project_id })
            .await?,
    )
}

#[tauri::command]
pub async fn orchd_set_idea_lifecycle(
    state: State<'_, AppState>,
    id: String,
    lifecycle: IdeaLifecycle,
) -> Result<Idea, CommandError> {
    expect_idea(
        state
            .orchd()?
            .request(OrchdRequest::SetIdeaLifecycle { id, lifecycle })
            .await?,
    )
}

#[tauri::command]
pub async fn orchd_delete_idea(state: State<'_, AppState>, id: String) -> Result<(), CommandError> {
    expect_orchd_ack(
        state
            .orchd()?
            .request(OrchdRequest::DeleteIdea { id })
            .await?,
    )
}

#[tauri::command]
pub async fn orchd_list_ideas(
    state: State<'_, AppState>,
    project_id: Option<String>,
) -> Result<Vec<Idea>, CommandError> {
    expect_ideas(
        state
            .orchd()?
            .request(OrchdRequest::ListIdeas { project_id })
            .await?,
    )
}

#[tauri::command]
pub async fn orchd_create_insight(
    state: State<'_, AppState>,
    project_id: Option<String>,
    source: String,
    title: String,
    body: String,
) -> Result<Insight, CommandError> {
    expect_insight(
        state
            .orchd()?
            .request(OrchdRequest::CreateInsight {
                project_id,
                source,
                title,
                body,
            })
            .await?,
    )
}

#[tauri::command]
pub async fn orchd_update_insight(
    state: State<'_, AppState>,
    id: String,
    title: Option<String>,
    body: Option<String>,
) -> Result<Insight, CommandError> {
    expect_insight(
        state
            .orchd()?
            .request(OrchdRequest::UpdateInsight { id, title, body })
            .await?,
    )
}

#[tauri::command]
pub async fn orchd_set_insight_fit_verdict(
    state: State<'_, AppState>,
    id: String,
    fit_verdict: Option<FitVerdict>,
    fit_reasoning: String,
) -> Result<Insight, CommandError> {
    expect_insight(
        state
            .orchd()?
            .request(OrchdRequest::SetInsightFitVerdict {
                id,
                fit_verdict,
                fit_reasoning,
            })
            .await?,
    )
}

#[tauri::command]
pub async fn orchd_set_insight_status(
    state: State<'_, AppState>,
    id: String,
    status: InsightStatus,
    resolution_reasoning: Option<String>,
) -> Result<Insight, CommandError> {
    expect_insight(
        state
            .orchd()?
            .request(OrchdRequest::SetInsightStatus {
                id,
                status,
                resolution_reasoning,
            })
            .await?,
    )
}

#[tauri::command]
pub async fn orchd_delete_insight(
    state: State<'_, AppState>,
    id: String,
) -> Result<(), CommandError> {
    expect_orchd_ack(
        state
            .orchd()?
            .request(OrchdRequest::DeleteInsight { id })
            .await?,
    )
}

#[tauri::command]
pub async fn orchd_list_insights(
    state: State<'_, AppState>,
    project_id: Option<String>,
) -> Result<Vec<Insight>, CommandError> {
    expect_insights(
        state
            .orchd()?
            .request(OrchdRequest::ListInsights { project_id })
            .await?,
    )
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn orchd_create_task(
    state: State<'_, AppState>,
    project_id: String,
    parent_id: Option<String>,
    title: String,
    body: String,
    status: Option<TaskStatus>,
    source: TaskSource,
    source_id: Option<String>,
    tags: Vec<String>,
) -> Result<DomainTask, CommandError> {
    expect_domain_task(
        state
            .orchd()?
            .request(OrchdRequest::CreateTask {
                project_id,
                parent_id,
                title,
                body,
                status,
                source,
                source_id,
                tags,
            })
            .await?,
    )
}

#[tauri::command]
pub async fn orchd_update_task(
    state: State<'_, AppState>,
    id: String,
    title: Option<String>,
    body: Option<String>,
    tags: Option<Vec<String>>,
) -> Result<DomainTask, CommandError> {
    expect_domain_task(
        state
            .orchd()?
            .request(OrchdRequest::UpdateTask {
                id,
                title,
                body,
                tags,
            })
            .await?,
    )
}

#[tauri::command]
pub async fn orchd_set_task_status(
    state: State<'_, AppState>,
    id: String,
    status: TaskStatus,
) -> Result<DomainTask, CommandError> {
    expect_domain_task(
        state
            .orchd()?
            .request(OrchdRequest::SetTaskStatus { id, status })
            .await?,
    )
}

#[tauri::command]
pub async fn orchd_set_task_rank(
    state: State<'_, AppState>,
    id: String,
    rank: f64,
) -> Result<DomainTask, CommandError> {
    expect_domain_task(
        state
            .orchd()?
            .request(OrchdRequest::SetTaskRank { id, rank })
            .await?,
    )
}

#[tauri::command]
pub async fn orchd_delete_task(state: State<'_, AppState>, id: String) -> Result<(), CommandError> {
    expect_orchd_ack(
        state
            .orchd()?
            .request(OrchdRequest::DeleteTask { id })
            .await?,
    )
}

#[tauri::command]
pub async fn orchd_list_tasks(
    state: State<'_, AppState>,
    project_id: Option<String>,
) -> Result<Vec<DomainTask>, CommandError> {
    expect_domain_tasks(
        state
            .orchd()?
            .request(OrchdRequest::ListTasks { project_id })
            .await?,
    )
}

#[tauri::command]
pub async fn orchd_get_ruleset(
    state: State<'_, AppState>,
    scope: RuleScope,
    project_id: Option<String>,
) -> Result<RuleSetView, CommandError> {
    expect_rule_set_view(
        state
            .orchd()?
            .request(OrchdRequest::GetRuleSet { scope, project_id })
            .await?,
    )
}

#[tauri::command]
pub async fn orchd_upsert_ruleset(
    state: State<'_, AppState>,
    scope: RuleScope,
    project_id: Option<String>,
    md_content: Option<String>,
    md_path: Option<String>,
    policy: Option<PolicyRules>,
) -> Result<RuleSetView, CommandError> {
    expect_rule_set_view(
        state
            .orchd()?
            .request(OrchdRequest::UpsertRuleSet {
                scope,
                project_id,
                md_content,
                md_path,
                policy,
            })
            .await?,
    )
}

#[tauri::command]
pub async fn orchd_acknowledge_rule_file(
    state: State<'_, AppState>,
    id: String,
) -> Result<RuleSetView, CommandError> {
    expect_rule_set_view(
        state
            .orchd()?
            .request(OrchdRequest::AcknowledgeRuleFile { id })
            .await?,
    )
}

#[tauri::command]
pub async fn orchd_export_project(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<String, CommandError> {
    expect_export_json(
        state
            .orchd()?
            .request(OrchdRequest::ExportProject { project_id })
            .await?,
    )
}

#[tauri::command]
pub async fn orchd_export_all(state: State<'_, AppState>) -> Result<String, CommandError> {
    expect_export_json(state.orchd()?.request(OrchdRequest::ExportAll).await?)
}

#[tauri::command]
pub async fn orchd_import_bundle(
    state: State<'_, AppState>,
    json: String,
) -> Result<ImportReport, CommandError> {
    expect_import_report(
        state
            .orchd()?
            .request(OrchdRequest::ImportBundle { json })
            .await?,
    )
}

// ── orchd special flows (spec §9: reveal / export-to-file / import-from-file / lifecycle) ──

/// Pure core of `orchd_reveal_rules_file` (spec §9: "JS never passes a path"): fetches the
/// `RuleSetView` for `(scope, project_id)` and returns the path `opener::reveal` should be called
/// with — always `view.rule.md_path`, i.e. the path `GetRuleSet` itself just returned, never
/// anything a caller could substitute. Pulled out from the `#[tauri::command]` wrapper so the
/// SECURITY-RELEVANT part (the path always comes from the daemon's own reply) is unit-testable
/// against a stub daemon without ever invoking the real `opener::reveal` (which would open a
/// Finder window) inside a test process.
pub(crate) async fn reveal_rules_file_core(
    client: &OrchdClient,
    scope: RuleScope,
    project_id: Option<String>,
) -> Result<String, CommandError> {
    let view = expect_rule_set_view(
        client
            .request(OrchdRequest::GetRuleSet { scope, project_id })
            .await?,
    )?;
    Ok(view.rule.md_path)
}

#[tauri::command]
pub async fn orchd_reveal_rules_file(
    state: State<'_, AppState>,
    scope: RuleScope,
    project_id: Option<String>,
) -> Result<(), CommandError> {
    let client = state.orchd()?;
    let path = reveal_rules_file_core(&client, scope, project_id).await?;
    opener::reveal(&path)
        .map_err(|e| CommandError::Internal(format!("failed to reveal rules file: {e}")))
}

/// Sanitize a string for safe use as a filename component: keeps alphanumerics/`-`/`_`, collapses
/// whitespace to `-`, drops everything else (defense in depth — the exported name is user data,
/// e.g. a project's `name` field, and must never inject a path separator/traversal into the
/// written filename even though `dest_dir` itself is a trusted `pick_folder` result). Falls back
/// to `"project"` if sanitizing empties the string.
fn sanitize_filename_component(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else if c.is_whitespace() {
                '-'
            } else {
                '\0'
            }
        })
        .filter(|c| *c != '\0')
        .collect();
    let trimmed = cleaned.trim_matches('-');
    if trimmed.is_empty() {
        "project".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Derive the base filename (before `-export.json`) for `orchd_export_to_file` (spec §9: "write
/// `<name-or-'store'>-export.json`"). A whole-store export always uses the literal `"store"`; a
/// per-project export reads the name straight out of the already-serialized export JSON
/// (`export::export_project`'s own top-level `project.name` field, spec §8) rather than requiring
/// a second value threaded through the call site — keeping this a pure function of the one string
/// `orchd_export_to_file` already has in hand. An unparseable/missing name (should not happen
/// against a real daemon reply) falls back to `"project"`.
pub(crate) fn export_base_name(json: &str, is_project_export: bool) -> String {
    if !is_project_export {
        return "store".to_string();
    }
    let name = serde_json::from_str::<serde_json::Value>(json)
        .ok()
        .and_then(|v| v.get("project")?.get("name")?.as_str().map(str::to_string))
        .unwrap_or_else(|| "project".to_string());
    sanitize_filename_component(&name)
}

/// Write `json` to `<dest_dir>/<base_name>-export.json` (spec §9: "write via `std::fs`, error →
/// `CommandError::Internal`"). Returns the written path. Pulled out from the `#[tauri::command]`
/// wrapper so the filesystem write is unit-testable directly against a tempdir.
pub(crate) fn write_export_file(
    dest_dir: &str,
    base_name: &str,
    json: &str,
) -> Result<std::path::PathBuf, CommandError> {
    let path = std::path::Path::new(dest_dir).join(format!("{base_name}-export.json"));
    std::fs::write(&path, json)
        .map_err(|e| CommandError::Internal(format!("failed to write export file: {e}")))?;
    Ok(path)
}

/// `dest_dir` is documented (spec §9) to be the exact `pick_folder` result passed straight
/// through — JS supplies it, but only ever as the return value of the `pick_folder` command (a
/// native folder picker), never a freehand path. Returns the written file's absolute path so the
/// frontend can toast/display it.
#[tauri::command]
pub async fn orchd_export_to_file(
    state: State<'_, AppState>,
    project_id: Option<String>,
    dest_dir: String,
) -> Result<String, CommandError> {
    let client = state.orchd()?;
    let is_project_export = project_id.is_some();
    let json = match project_id {
        Some(id) => expect_export_json(
            client
                .request(OrchdRequest::ExportProject { project_id: id })
                .await?,
        )?,
        None => expect_export_json(client.request(OrchdRequest::ExportAll).await?)?,
    };
    let base_name = export_base_name(&json, is_project_export);
    let path = write_export_file(&dest_dir, &base_name, &json)?;
    Ok(path.to_string_lossy().into_owned())
}

/// Local, pre-brokering read cap (spec §9: "10 MiB read cap guard") — independent of
/// `bpa_protocol::MAX_FRAME_LEN` (16 MiB), which the CLIENT enforces once the request is actually
/// CBOR-encoded (`OrchdClientError::RequestTooLarge`). Checked against the file's METADATA before
/// any read happens, so an absurdly large file is rejected without ever being loaded into memory.
pub(crate) const IMPORT_FILE_READ_CAP: u64 = 10 * 1024 * 1024;

/// Read `path` for `orchd_import_from_file`, enforcing [`IMPORT_FILE_READ_CAP`]. Pulled out from
/// the `#[tauri::command]` wrapper so the cap is unit-testable without a stub daemon.
pub(crate) fn read_import_file(path: &str) -> Result<String, CommandError> {
    let meta = std::fs::metadata(path)
        .map_err(|e| CommandError::Internal(format!("cannot read import file: {e}")))?;
    if meta.len() > IMPORT_FILE_READ_CAP {
        return Err(CommandError::Internal(format!(
            "import file ({} bytes) exceeds the {IMPORT_FILE_READ_CAP}-byte read cap",
            meta.len()
        )));
    }
    std::fs::read_to_string(path)
        .map_err(|e| CommandError::Internal(format!("cannot read import file: {e}")))
}

#[tauri::command]
pub async fn orchd_import_from_file(
    state: State<'_, AppState>,
    path: String,
) -> Result<ImportReport, CommandError> {
    let json = read_import_file(&path)?;
    expect_import_report(
        state
            .orchd()?
            .request(OrchdRequest::ImportBundle { json })
            .await?,
    )
}

/// Drops the current orchd client slot, then re-runs `lib.rs`'s `bring_up_orchd` connect sequence
/// from scratch (spec §9's locked flow — the [Retry] button's target, T19). Spawned via
/// `tauri::async_runtime::spawn` rather than awaited inline: `bring_up_orchd`'s bounded retry can
/// take up to ~4s (`BOOT_CONNECT_ATTEMPTS` x 500ms), and the frontend observes the outcome via the
/// `orchd://down|up|incompatible` events / `AppState.orchd_status` rather than this command's
/// resolved `Promise` — mirrors the fire-and-forget shape `setup()` itself uses for the initial
/// bring-up.
#[tauri::command]
pub async fn orchd_reconnect(
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<(), CommandError> {
    state.orchd.write().unwrap().take();
    let agent = state.orchd_launchd.clone();
    let broker = state.broker.clone();
    let slot = state.orchd.clone();
    let status = state.orchd_status.clone();
    tauri::async_runtime::spawn(crate::bring_up_orchd(app, agent, broker, slot, status));
    Ok(())
}

/// Core of the orchd-upgrade flow (spec §9): mirrors [`upgrade_daemon_core`] EXACTLY for the
/// second daemon — best-effort `OrchdShutdown{drain:true}` (an old, not-yet-upgraded `bpa-orchd`
/// binary may not even parse this frame; the failure is expected and swallowed, same rationale as
/// `upgrade_daemon_core`'s sessiond drain), then the one honest, surfaced failure:
/// `kickstart_force()` (spec §6.2.4/§9: never claim success when `launchctl kickstart -k` failed).
pub async fn orchd_upgrade_core(
    client: Option<Arc<OrchdClient>>,
    agent: &crate::launchd::LaunchdAgent<'_>,
) -> Result<(), CommandError> {
    if let Some(c) = client {
        let _ = c.request(OrchdRequest::OrchdShutdown { drain: true }).await;
    }
    agent
        .kickstart_force()
        .map_err(|e| CommandError::UpgradeFailed {
            reason: e.to_string(),
        })?;
    Ok(())
}

/// `#[tauri::command]` entry point for the orchd-upgrade flow — mirrors [`upgrade_daemon`]
/// verbatim, over the orchd slot/launchd agent instead of the sessiond ones (spec §9).
#[tauri::command]
pub async fn orchd_upgrade(state: State<'_, AppState>, app: AppHandle) -> Result<(), CommandError> {
    let client = state.orchd.read().unwrap().clone();
    orchd_upgrade_core(client, &state.orchd_launchd).await?;
    app.restart();
}

// ── S4 knowledge-graph orchd #[tauri::command] surface (spec §3, appended — S4 T5) ────────────
//
// Same one-thin-command-per-verb shape as the block above, over the S4 `OrchdRequest::Graph*`
// verbs (spec §3's frozen-append-only wire additions). `OrchdRequest::GraphDeleteNode`/
// `GraphDeleteEdge` -> `OrchdResponse::Ack` mirror `orchd_delete_goal`'s `expect_orchd_ack` shape.

#[tauri::command]
pub async fn orchd_graph_add_node(
    state: State<'_, AppState>,
    project_id: String,
    kind: GraphNodeKind,
    label: String,
    body: String,
    pos_x: f64,
    pos_y: f64,
) -> Result<GraphNode, CommandError> {
    expect_graph_node(
        state
            .orchd()?
            .request(OrchdRequest::GraphAddNode {
                project_id,
                kind,
                label,
                body,
                pos_x,
                pos_y,
            })
            .await?,
    )
}

#[tauri::command]
pub async fn orchd_graph_update_node(
    state: State<'_, AppState>,
    id: String,
    label: Option<String>,
    body: Option<String>,
) -> Result<GraphNode, CommandError> {
    expect_graph_node(
        state
            .orchd()?
            .request(OrchdRequest::GraphUpdateNode { id, label, body })
            .await?,
    )
}

#[tauri::command]
pub async fn orchd_graph_move_node(
    state: State<'_, AppState>,
    id: String,
    pos_x: f64,
    pos_y: f64,
) -> Result<GraphNode, CommandError> {
    expect_graph_node(
        state
            .orchd()?
            .request(OrchdRequest::GraphMoveNode { id, pos_x, pos_y })
            .await?,
    )
}

#[tauri::command]
pub async fn orchd_graph_delete_node(
    state: State<'_, AppState>,
    id: String,
) -> Result<(), CommandError> {
    expect_orchd_ack(
        state
            .orchd()?
            .request(OrchdRequest::GraphDeleteNode { id })
            .await?,
    )
}

#[tauri::command]
pub async fn orchd_graph_add_edge(
    state: State<'_, AppState>,
    source_node_id: String,
    target_node_id: String,
    kind: GraphEdgeKind,
    label: String,
) -> Result<GraphEdge, CommandError> {
    expect_graph_edge(
        state
            .orchd()?
            .request(OrchdRequest::GraphAddEdge {
                source_node_id,
                target_node_id,
                kind,
                label,
            })
            .await?,
    )
}

#[tauri::command]
pub async fn orchd_graph_delete_edge(
    state: State<'_, AppState>,
    id: String,
) -> Result<(), CommandError> {
    expect_orchd_ack(
        state
            .orchd()?
            .request(OrchdRequest::GraphDeleteEdge { id })
            .await?,
    )
}

#[tauri::command]
pub async fn orchd_graph_list_project(
    state: State<'_, AppState>,
    project_id: String,
) -> Result<GraphView, CommandError> {
    expect_graph_view(
        state
            .orchd()?
            .request(OrchdRequest::GraphListProject { project_id })
            .await?,
    )
}

#[tauri::command]
pub async fn orchd_graph_neighborhood(
    state: State<'_, AppState>,
    node_id: String,
    depth: u32,
) -> Result<GraphNeighborhood, CommandError> {
    expect_neighborhood(
        state
            .orchd()?
            .request(OrchdRequest::GraphNeighborhood { node_id, depth })
            .await?,
    )
}

#[tauri::command]
pub async fn orchd_graph_search(
    state: State<'_, AppState>,
    query: String,
    project_id: Option<String>,
) -> Result<Vec<GraphNode>, CommandError> {
    expect_graph_nodes(
        state
            .orchd()?
            .request(OrchdRequest::GraphSearch { query, project_id })
            .await?,
    )
}

// ── S-EXT MCP orchd #[tauri::command] surface (spec §5, appended — S-EXT T7) ───────────────────
//
// Same one-thin-command-per-verb shape as the S4 graph block above, over the `OrchdRequest::Mcp*`
// / `TrustGrantConsent` verbs (spec §5's frozen-append-only wire additions). `McpConnect` denied
// for missing/stale consent surfaces as `OrchdResponse::Error{code: Consent, ..}`, an `McpCallTool`
// against a disabled tool surfaces as `Error{code: Policy, ..}` — both already flow through
// `err_from_orchd_response`/`From<OrchdClientError>` into `CommandError::Daemon{code, message}`
// exactly like every other daemon error, no special-casing needed here (spec §9 doc block above).
// `mcp_set_server_bearer`'s `token` is passed straight through to the request struct and never
// touched otherwise — it is not logged, echoed, or included in any `Debug`/tracing output in this
// module (spec §5: "token NEVER logged/echoed").

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn mcp_add_server(
    state: State<'_, AppState>,
    name: String,
    transport: McpTransport,
    url: Option<String>,
    command: Option<String>,
    args: Option<Vec<String>>,
    env: Option<BTreeMap<String, String>>,
    scope: McpScope,
    project_id: Option<String>,
    auth_kind: McpAuthKind,
    timeout_ms: Option<i64>,
    max_retries: Option<i64>,
) -> Result<McpServer, CommandError> {
    expect_mcp_server(
        state
            .orchd()?
            .request(OrchdRequest::McpAddServer {
                name,
                transport,
                url,
                command,
                args,
                env,
                scope,
                project_id,
                auth_kind,
                timeout_ms,
                max_retries,
            })
            .await?,
    )
}

#[tauri::command]
pub async fn mcp_list_servers(
    state: State<'_, AppState>,
    project_id: Option<String>,
) -> Result<Vec<McpServer>, CommandError> {
    expect_mcp_servers(
        state
            .orchd()?
            .request(OrchdRequest::McpListServers { project_id })
            .await?,
    )
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub async fn mcp_update_server(
    state: State<'_, AppState>,
    id: String,
    name: Option<String>,
    url: Option<String>,
    command: Option<String>,
    args: Option<Vec<String>>,
    env: Option<BTreeMap<String, String>>,
    auth_kind: Option<McpAuthKind>,
    timeout_ms: Option<i64>,
    max_retries: Option<i64>,
) -> Result<McpServer, CommandError> {
    expect_mcp_server(
        state
            .orchd()?
            .request(OrchdRequest::McpUpdateServer {
                id,
                name,
                url,
                command,
                args,
                env,
                auth_kind,
                timeout_ms,
                max_retries,
            })
            .await?,
    )
}

#[tauri::command]
pub async fn mcp_set_server_enabled(
    state: State<'_, AppState>,
    id: String,
    enabled: bool,
) -> Result<McpServer, CommandError> {
    expect_mcp_server(
        state
            .orchd()?
            .request(OrchdRequest::McpSetServerEnabled { id, enabled })
            .await?,
    )
}

#[tauri::command]
pub async fn mcp_delete_server(state: State<'_, AppState>, id: String) -> Result<(), CommandError> {
    expect_orchd_ack(
        state
            .orchd()?
            .request(OrchdRequest::McpDeleteServer { id })
            .await?,
    )
}

/// `token` -> Keychain, ref -> DB, on the orchd side; this wrapper never logs or echoes it (spec
/// §5).
#[tauri::command]
pub async fn mcp_set_server_bearer(
    state: State<'_, AppState>,
    id: String,
    token: String,
) -> Result<(), CommandError> {
    expect_orchd_ack(
        state
            .orchd()?
            .request(OrchdRequest::McpSetServerBearer { id, token })
            .await?,
    )
}

#[tauri::command]
pub async fn mcp_connect(
    state: State<'_, AppState>,
    id: String,
) -> Result<McpConnectReport, CommandError> {
    expect_mcp_connect_report(
        state
            .orchd()?
            .request(OrchdRequest::McpConnect { id })
            .await?,
    )
}

#[tauri::command]
pub async fn mcp_disconnect(state: State<'_, AppState>, id: String) -> Result<(), CommandError> {
    expect_orchd_ack(
        state
            .orchd()?
            .request(OrchdRequest::McpDisconnect { id })
            .await?,
    )
}

#[tauri::command]
pub async fn mcp_list_tools(
    state: State<'_, AppState>,
    server_id: String,
) -> Result<Vec<McpTool>, CommandError> {
    expect_mcp_tools(
        state
            .orchd()?
            .request(OrchdRequest::McpListTools { server_id })
            .await?,
    )
}

#[tauri::command]
pub async fn mcp_set_tool_enabled(
    state: State<'_, AppState>,
    tool_id: String,
    enabled: bool,
) -> Result<McpTool, CommandError> {
    expect_mcp_tool(
        state
            .orchd()?
            .request(OrchdRequest::McpSetToolEnabled { tool_id, enabled })
            .await?,
    )
}

#[tauri::command]
pub async fn mcp_call_tool(
    state: State<'_, AppState>,
    server_id: String,
    tool_name: String,
    args_json: String,
    project_id: Option<String>,
) -> Result<McpCallResult, CommandError> {
    expect_mcp_call_result(
        state
            .orchd()?
            .request(OrchdRequest::McpCallTool {
                server_id,
                tool_name,
                args_json,
                project_id,
            })
            .await?,
    )
}

#[tauri::command]
pub async fn mcp_list_invocations(
    state: State<'_, AppState>,
    server_id: Option<String>,
    project_id: Option<String>,
    limit: Option<i64>,
) -> Result<Vec<McpInvocation>, CommandError> {
    expect_mcp_invocations(
        state
            .orchd()?
            .request(OrchdRequest::McpListInvocations {
                server_id,
                project_id,
                limit,
            })
            .await?,
    )
}

#[tauri::command]
pub async fn mcp_list_artifacts(
    state: State<'_, AppState>,
    project_id: Option<String>,
    server_id: Option<String>,
    limit: Option<i64>,
) -> Result<Vec<McpArtifact>, CommandError> {
    expect_mcp_artifacts(
        state
            .orchd()?
            .request(OrchdRequest::McpListArtifacts {
                project_id,
                server_id,
                limit,
            })
            .await?,
    )
}

#[tauri::command]
pub async fn mcp_get_artifact(
    state: State<'_, AppState>,
    id: String,
) -> Result<McpArtifact, CommandError> {
    expect_mcp_artifact(
        state
            .orchd()?
            .request(OrchdRequest::McpGetArtifact { id })
            .await?,
    )
}

#[tauri::command]
pub async fn trust_grant_consent(
    state: State<'_, AppState>,
    server_id: String,
    kind: String,
) -> Result<(), CommandError> {
    expect_orchd_ack(
        state
            .orchd()?
            .request(OrchdRequest::TrustGrantConsent { server_id, kind })
            .await?,
    )
}

// ── S-EXT Connectors orchd #[tauri::command] surface (spec §5/§7, appended — task T13a) ────────
//
// Same one-thin-command-per-verb shape as the MCP block above, over the `OrchdRequest::Connector*`
// verbs (spec §5's frozen-append-only wire additions). `connector_add_api_key`'s `api_key` and
// `connector_complete_oauth`'s `code` are passed straight through to the request struct and never
// touched otherwise — not logged, echoed, or included in any `Debug`/tracing output in this module
// (spec §5/§6: never logged/echoed). An unregistered-provider `ConnectorBeginOAuth`, or a
// spend/rate-cap `ConnectorInvoke` denial (`Error{code:Policy}`), both already flow through
// `err_from_orchd_response`/`From<OrchdClientError>` into `CommandError::Daemon{code, message}`
// exactly like every other daemon error — no special-casing needed here (spec §9 doc block above).

#[tauri::command]
pub async fn connector_begin_oauth(
    state: State<'_, AppState>,
    provider: String,
    label: String,
    scopes: Option<Vec<String>>,
    server_id: Option<String>,
) -> Result<OAuthChallenge, CommandError> {
    expect_oauth_challenge(
        state
            .orchd()?
            .request(OrchdRequest::ConnectorBeginOAuth {
                provider,
                label,
                scopes,
                server_id,
            })
            .await?,
    )
}

/// `code` -> exchanged for tokens on the orchd side (Keychain); this wrapper never logs or echoes
/// it (spec §5/§6). The webview-facing parameter is named `oauth_state` (not `state`, which is
/// reserved here for Tauri's own injected `State<'_, AppState>`) — it is `OAuthChallenge.state`,
/// the CSRF token `ConnectorBeginOAuth` returned, echoed back to complete the PKCE round-trip.
#[tauri::command]
pub async fn connector_complete_oauth(
    state: State<'_, AppState>,
    oauth_state: String,
    code: String,
) -> Result<Account, CommandError> {
    expect_account(
        state
            .orchd()?
            .request(OrchdRequest::ConnectorCompleteOAuth {
                state: oauth_state,
                code,
            })
            .await?,
    )
}

/// `api_key` -> Keychain, ref -> DB, on the orchd side; this wrapper never logs or echoes it (spec
/// §5/§6).
#[tauri::command]
pub async fn connector_add_api_key(
    state: State<'_, AppState>,
    provider: String,
    label: String,
    api_key: String,
) -> Result<Account, CommandError> {
    expect_account(
        state
            .orchd()?
            .request(OrchdRequest::ConnectorAddApiKey {
                provider,
                label,
                api_key,
            })
            .await?,
    )
}

#[tauri::command]
pub async fn connector_list_accounts(
    state: State<'_, AppState>,
) -> Result<Vec<Account>, CommandError> {
    expect_accounts(
        state
            .orchd()?
            .request(OrchdRequest::ConnectorListAccounts)
            .await?,
    )
}

#[tauri::command]
pub async fn connector_delete_account(
    state: State<'_, AppState>,
    id: String,
) -> Result<(), CommandError> {
    expect_orchd_ack(
        state
            .orchd()?
            .request(OrchdRequest::ConnectorDeleteAccount { id })
            .await?,
    )
}

#[tauri::command]
pub async fn connector_list_ops(
    state: State<'_, AppState>,
    account_id: String,
) -> Result<Vec<ConnectorOp>, CommandError> {
    expect_connector_ops(
        state
            .orchd()?
            .request(OrchdRequest::ConnectorListOps { account_id })
            .await?,
    )
}

#[tauri::command]
pub async fn connector_invoke(
    state: State<'_, AppState>,
    account_id: String,
    op: String,
    args_json: String,
    project_id: Option<String>,
) -> Result<McpCallResult, CommandError> {
    expect_mcp_call_result(
        state
            .orchd()?
            .request(OrchdRequest::ConnectorInvoke {
                account_id,
                op,
                args_json,
                project_id,
            })
            .await?,
    )
}

// ── S-EXT Skills orchd #[tauri::command] surface (spec §5/§8, D11, Q14, appended — task T17)
// ───────────────────────────────────────────────────────────────────────────────────────────
//
// Same one-thin-command-per-verb shape as the MCP/Connector blocks above, over the
// `OrchdRequest::Skill*` verbs (spec §5's frozen-append-only wire additions). PLUMBING ONLY
// (D11): this registry has no runtime consumer until the S6b agent org — these three commands
// (add/list/delete) are the entire surface, mirroring `bpa_orchd_proto::Skill`'s own doc comment.

#[tauri::command]
pub async fn skill_add(
    state: State<'_, AppState>,
    name: Option<String>,
    description: Option<String>,
    md_path: String,
    scope: SkillScope,
    project_id: Option<String>,
) -> Result<Skill, CommandError> {
    expect_skill(
        state
            .orchd()?
            .request(OrchdRequest::SkillAdd {
                name,
                description,
                md_path,
                scope,
                project_id,
            })
            .await?,
    )
}

#[tauri::command]
pub async fn skill_list(
    state: State<'_, AppState>,
    project_id: Option<String>,
) -> Result<Vec<Skill>, CommandError> {
    expect_skills(
        state
            .orchd()?
            .request(OrchdRequest::SkillList { project_id })
            .await?,
    )
}

#[tauri::command]
pub async fn skill_delete(state: State<'_, AppState>, id: String) -> Result<(), CommandError> {
    expect_orchd_ack(
        state
            .orchd()?
            .request(OrchdRequest::SkillDelete { id })
            .await?,
    )
}

// ── S-EXT Trust orchd #[tauri::command] surface (spec §4/§5/§6, BL-22, appended — task T18)
// ───────────────────────────────────────────────────────────────────────────────────────────
//
// Same one-thin-command-per-verb shape as the MCP/Connector/Skills blocks above, over the
// `OrchdRequest::Trust*` verbs (spec §5's frozen-append-only wire additions). A spend/rate-cap
// denial on `mcp_call_tool`/`connector_invoke` (above) already surfaces as
// `CommandError{kind:"daemon",code:"Policy"}` through the existing `err_from_orchd_response`
// path — no special-casing needed here; these three commands are the caps CONFIGURATION +
// audit-log READ surface only.

/// UPSERT keyed by `(scope, refId)` (spec §4) — `scope:"global"` requires `refId: null`,
/// `scope:"project"|"server"` requires `refId: Some(<id>)`; a mismatch rejects with
/// `CommandError{kind:"daemon",code:"Validation"}` BEFORE any row is written. `None` cap fields
/// mean "unlimited" for that dimension.
#[tauri::command]
pub async fn trust_set_policy(
    state: State<'_, AppState>,
    scope: PolicyScope,
    ref_id: Option<String>,
    spend_cap_usd: Option<f64>,
    rate_per_min: Option<i64>,
) -> Result<Policy, CommandError> {
    expect_policy(
        state
            .orchd()?
            .request(OrchdRequest::TrustSetPolicy {
                scope,
                ref_id,
                spend_cap_usd,
                rate_per_min,
            })
            .await?,
    )
}

#[tauri::command]
pub async fn trust_list_policies(state: State<'_, AppState>) -> Result<Vec<Policy>, CommandError> {
    expect_policies(
        state
            .orchd()?
            .request(OrchdRequest::TrustListPolicies)
            .await?,
    )
}

/// Newest-first, optionally capped at `limit` (spec §4 `audit_log`) — every trust-choke-point
/// decision, allow or deny, for the Log/audit UI.
#[tauri::command]
pub async fn trust_list_audit(
    state: State<'_, AppState>,
    limit: Option<i64>,
) -> Result<Vec<AuditRow>, CommandError> {
    expect_audit_rows(
        state
            .orchd()?
            .request(OrchdRequest::TrustListAudit { limit })
            .await?,
    )
}

// ── S-IDEA research (spec §5/§6, task T5) — thin proxies over the 3 net-new research verbs,
// mirroring every `orchd_*` command above exactly (build the request, `.request(..).await?`,
// unwrap the expected `OrchdResponse` variant). Deliberately named WITHOUT the `orchd_` prefix
// (spec §3 module-layout table: "commands.rs: research_start_run / research_list_runs /
// research_get_run (proxy)") — the frontend ipc wrapper names these bind to are
// `researchStartRun`/`researchListRuns`/`researchGetRun` (Tauri's camelCase default), not
// `orchdResearchStartRun` etc. ────────────────────────────────────────────────────────────────

/// Starts a research run (spec §6 step 1): inserts `research_run{pending}` and flips the idea
/// `captured`->`researching` (only if currently `captured`), THEN spawns the background driver
/// that actually calls `tool_name` on `server_id` via the SHIPPED `mcp::invoke::call_tool` path.
/// The reply is that freshly-inserted `pending` row — the run's terminal state (`done`/`failed`)
/// arrives later via `orchd://research-runs-changed` (`ResearchRunsChanged`), NOT this reply.
#[tauri::command]
pub async fn research_start_run(
    state: State<'_, AppState>,
    idea_id: String,
    server_id: String,
    tool_name: String,
    args_json: String,
) -> Result<ResearchRun, CommandError> {
    expect_research_run(
        state
            .orchd()?
            .request(OrchdRequest::ResearchStartRun {
                idea_id,
                server_id,
                tool_name,
                args_json,
            })
            .await?,
    )
}

/// Runs for one idea, newest first (spec §5). Plain read — broadcasts nothing.
#[tauri::command]
pub async fn research_list_runs(
    state: State<'_, AppState>,
    idea_id: String,
) -> Result<Vec<ResearchRun>, CommandError> {
    expect_research_runs(
        state
            .orchd()?
            .request(OrchdRequest::ResearchListRuns { idea_id })
            .await?,
    )
}

/// One run by id (spec §5). Plain read — broadcasts nothing. An unknown `id` surfaces as
/// `CommandError::Daemon{code:"NotFound"}` (the daemon's `ResearchGetRun` dispatch arm maps its
/// own `Db::get_research_run`'s honest `Ok(None)` to `Error{NotFound}` — see `socket_server.rs`).
#[tauri::command]
pub async fn research_get_run(
    state: State<'_, AppState>,
    id: String,
) -> Result<ResearchRun, CommandError> {
    expect_research_run(
        state
            .orchd()?
            .request(OrchdRequest::ResearchGetRun { id })
            .await?,
    )
}

/// The daemon's storage-degradation mode (spec D3, BL-94). Plain read — broadcasts nothing; the
/// mode is fixed at boot, so the frontend pulls it once on connect and on every reconnect to drive
/// the honest "running in memory / recovered from a corrupt database" banner.
#[tauri::command]
pub async fn orchd_storage_status(
    state: State<'_, AppState>,
) -> Result<StorageStatus, CommandError> {
    expect_storage_status(
        state
            .orchd()?
            .request(OrchdRequest::GetStorageStatus)
            .await?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_opts_defaults_env_overrides_and_reads_camel_case() {
        // envOverrides omitted -> defaults to [].
        let json = r#"{ "shell": "/bin/zsh", "cols": 120, "rows": 40 }"#;
        let opts: CreateOpts = serde_json::from_str(json).unwrap();
        assert_eq!(opts.shell.as_deref(), Some("/bin/zsh"));
        assert_eq!(opts.cwd, None);
        assert_eq!(opts.env_overrides, Vec::<(String, String)>::new());
        assert_eq!(opts.cols, Some(120));
        assert_eq!(opts.rows, Some(40));

        // envOverrides present as camelCase, array of [k,v] pairs.
        let json2 = r#"{ "envOverrides": [["FOO", "bar"], ["BAZ", "qux"]] }"#;
        let opts2: CreateOpts = serde_json::from_str(json2).unwrap();
        assert_eq!(
            opts2.env_overrides,
            vec![
                ("FOO".to_string(), "bar".to_string()),
                ("BAZ".to_string(), "qux".to_string())
            ]
        );
        assert_eq!(opts2.shell, None);
    }

    #[test]
    fn create_session_uses_80x24_when_size_omitted() {
        let opts = CreateOpts {
            shell: None,
            cwd: None,
            env_overrides: vec![],
            cols: None,
            rows: None,
        };
        assert_eq!(resolve_size(&opts), (80, 24));

        let opts2 = CreateOpts {
            shell: None,
            cwd: None,
            env_overrides: vec![],
            cols: Some(100),
            rows: Some(30),
        };
        assert_eq!(resolve_size(&opts2), (100, 30));
    }

    #[test]
    fn create_session_builds_request_with_defaults() {
        let req = build_create_session("ws-1".to_string(), None);
        match req {
            Request::CreateSession {
                workspace_id,
                shell,
                cwd,
                env_overrides,
                cols,
                rows,
            } => {
                assert_eq!(workspace_id, "ws-1");
                assert_eq!(shell, None);
                assert_eq!(cwd, None);
                assert_eq!(env_overrides, Vec::<(String, String)>::new());
                assert_eq!((cols, rows), (80, 24));
            }
            other => panic!("expected CreateSession, got {other:?}"),
        }
    }

    #[test]
    fn create_session_builds_request_with_opts() {
        let opts = CreateOpts {
            shell: Some("/bin/bash".into()),
            cwd: Some("/tmp/x".into()),
            env_overrides: vec![("K".into(), "V".into())],
            cols: Some(90),
            rows: Some(25),
        };
        let req = build_create_session("ws-2".to_string(), Some(opts));
        match req {
            Request::CreateSession {
                workspace_id,
                shell,
                cwd,
                env_overrides,
                cols,
                rows,
            } => {
                assert_eq!(workspace_id, "ws-2");
                assert_eq!(shell.as_deref(), Some("/bin/bash"));
                assert_eq!(cwd.as_deref(), Some("/tmp/x"));
                assert_eq!(env_overrides, vec![("K".to_string(), "V".to_string())]);
                assert_eq!((cols, rows), (90, 25));
            }
            other => panic!("expected CreateSession, got {other:?}"),
        }
    }

    #[test]
    fn write_stdin_chunks_builds_single_request_utf8_bytes_under_chunk_size() {
        let reqs = build_write_stdin_chunks("s".to_string(), "héllo".to_string());
        assert_eq!(
            reqs.len(),
            1,
            "input under WRITE_STDIN_CHUNK must be one request"
        );
        match &reqs[0] {
            Request::WriteStdin { session_id, bytes } => {
                assert_eq!(session_id, "s");
                assert_eq!(bytes, &"héllo".as_bytes().to_vec());
            }
            other => panic!("expected WriteStdin, got {other:?}"),
        }
    }

    #[test]
    fn write_stdin_chunks_splits_oversized_input_into_ordered_1mib_chunks() {
        // 3.5 MiB of input -> 4 sequential chunks (3 full 1 MiB + one 0.5 MiB remainder), in order,
        // reassembling byte-identically to the original input (finding [2]).
        let total = WRITE_STDIN_CHUNK * 3 + WRITE_STDIN_CHUNK / 2;
        let data: String = (0..total)
            .map(|i| (b'a' + (i % 26) as u8) as char)
            .collect();
        let expected_bytes = data.clone().into_bytes();

        let reqs = build_write_stdin_chunks("sess-1".to_string(), data);
        assert_eq!(reqs.len(), 4, "expected 4 chunks for 3.5 MiB input");

        let mut reassembled = Vec::new();
        for (i, req) in reqs.iter().enumerate() {
            match req {
                Request::WriteStdin { session_id, bytes } => {
                    assert_eq!(
                        session_id, "sess-1",
                        "chunk {i} must target the same session"
                    );
                    assert!(
                        bytes.len() <= WRITE_STDIN_CHUNK,
                        "chunk {i} exceeds WRITE_STDIN_CHUNK: {}",
                        bytes.len()
                    );
                    reassembled.extend_from_slice(bytes);
                }
                other => panic!("expected WriteStdin, got {other:?}"),
            }
        }
        assert_eq!(
            reqs[0].clone(),
            Request::WriteStdin {
                session_id: "sess-1".to_string(),
                bytes: expected_bytes[0..WRITE_STDIN_CHUNK].to_vec(),
            }
        );
        assert_eq!(
            reassembled, expected_bytes,
            "chunks must reassemble byte-identically, in order"
        );
    }

    #[test]
    fn write_stdin_chunks_exact_multiple_of_chunk_size() {
        // Exactly on a chunk boundary: must not produce a trailing empty chunk.
        let data: String = "x".repeat(WRITE_STDIN_CHUNK * 2);
        let reqs = build_write_stdin_chunks("s".to_string(), data);
        assert_eq!(
            reqs.len(),
            2,
            "exact multiple must not leave a trailing empty chunk"
        );
        for req in &reqs {
            match req {
                Request::WriteStdin { bytes, .. } => assert_eq!(bytes.len(), WRITE_STDIN_CHUNK),
                other => panic!("expected WriteStdin, got {other:?}"),
            }
        }
    }

    #[test]
    fn write_stdin_chunks_empty_input_yields_single_empty_request() {
        let reqs = build_write_stdin_chunks("s".to_string(), String::new());
        assert_eq!(reqs.len(), 1);
        match &reqs[0] {
            Request::WriteStdin { bytes, .. } => assert!(bytes.is_empty()),
            other => panic!("expected WriteStdin, got {other:?}"),
        }
    }

    #[test]
    fn response_error_becomes_command_error_daemon() {
        let res = Response::Error {
            code: "InvalidWorkspaceRoot".into(),
            message: "gone".into(),
        };
        let err = expect_session(res).unwrap_err();
        match err {
            CommandError::Daemon { code, message } => {
                assert_eq!(code, "InvalidWorkspaceRoot");
                assert_eq!(message, "gone");
            }
            other => panic!("expected Daemon error, got {other:?}"),
        }
    }

    #[test]
    fn response_session_unwraps_to_meta() {
        let meta = SessionMeta {
            id: "s".into(),
            workspace_id: "w".into(),
            title: "t".into(),
            shell: "/bin/zsh".into(),
            cwd: "/".into(),
            cols: 80,
            rows: 24,
            lifecycle: bpa_protocol::SessionLifecycle::AtPrompt,
            waiting_for_input: false,
            is_active: true,
            created_at: 0,
        };
        let got = expect_session(Response::Session(meta.clone())).unwrap();
        assert_eq!(got.id, "s");
        // A wrong variant is an Internal protocol error, not a silent default.
        assert!(matches!(
            expect_session(Response::Ack),
            Err(CommandError::Internal(_))
        ));
    }

    #[test]
    fn client_error_disconnected_maps_to_command_error_disconnected() {
        let err: CommandError = ClientError::Disconnected.into();
        assert_eq!(err, CommandError::Disconnected);
    }

    #[test]
    fn client_error_daemon_maps_to_command_error_daemon() {
        let err: CommandError = ClientError::Daemon {
            code: "X".into(),
            message: "Y".into(),
        }
        .into();
        assert_eq!(
            err,
            CommandError::Daemon {
                code: "X".into(),
                message: "Y".into()
            }
        );
    }

    #[test]
    fn client_error_incompatible_daemon_maps_to_command_error_incompatible_daemon() {
        let err: CommandError = ClientError::IncompatibleDaemon {
            daemon_min: 2,
            daemon_max: 3,
        }
        .into();
        assert_eq!(
            err,
            CommandError::IncompatibleDaemon {
                daemon_min: 2,
                daemon_max: 3
            }
        );
    }

    #[test]
    fn command_error_serializes_with_camel_case_tag() {
        let v = serde_json::to_value(CommandError::Daemon {
            code: "C".into(),
            message: "M".into(),
        })
        .unwrap();
        assert_eq!(v["kind"], "daemon");
        assert_eq!(v["code"], "C");
        assert_eq!(v["message"], "M");

        let v2 = serde_json::to_value(CommandError::Disconnected).unwrap();
        assert_eq!(v2["kind"], "disconnected");

        let v3 = serde_json::to_value(CommandError::IncompatibleDaemon {
            daemon_min: 2,
            daemon_max: 2,
        })
        .unwrap();
        assert_eq!(v3["kind"], "incompatibleDaemon");
        assert_eq!(v3["daemonMin"], 2);
        assert_eq!(v3["daemonMax"], 2);
    }

    // ── From<OrchdClientError> for CommandError (S3 T12, spec §9) ──────────────────────────

    #[test]
    fn orchd_client_error_disconnected_maps_to_command_error_disconnected() {
        let err: CommandError = OrchdClientError::Disconnected.into();
        assert_eq!(err, CommandError::Disconnected);
    }

    #[test]
    fn orchd_client_error_daemon_maps_to_command_error_daemon() {
        let err: CommandError = OrchdClientError::Daemon {
            code: "Invariant".into(),
            message: "cannot remove the last project workspace".into(),
        }
        .into();
        assert_eq!(
            err,
            CommandError::Daemon {
                code: "Invariant".into(),
                message: "cannot remove the last project workspace".into(),
            }
        );
    }

    #[test]
    fn orchd_client_error_incompatible_orchd_maps_to_command_error_incompatible_orchd() {
        let err: CommandError = OrchdClientError::IncompatibleOrchd {
            daemon_min: 2,
            daemon_max: 3,
        }
        .into();
        assert_eq!(
            err,
            CommandError::IncompatibleOrchd {
                orchd_min: 2,
                orchd_max: 3
            }
        );
    }

    #[test]
    fn orchd_client_error_request_too_large_maps_to_command_error_too_large() {
        let err: CommandError = OrchdClientError::RequestTooLarge { size: 999 }.into();
        assert_eq!(err, CommandError::TooLarge { size: 999 });
    }

    #[test]
    fn command_error_serializes_incompatible_orchd_with_camel_case() {
        let v = serde_json::to_value(CommandError::IncompatibleOrchd {
            orchd_min: 4,
            orchd_max: 5,
        })
        .unwrap();
        assert_eq!(v["kind"], "incompatibleOrchd");
        assert_eq!(v["orchdMin"], 4);
        assert_eq!(v["orchdMax"], 5);
    }

    #[test]
    fn pick_folder_is_core_only_no_daemon_request() {
        // Every brokered command has a Request variant it forwards. pick_folder must NOT — there
        // is deliberately no Request::PickFolder. This test documents and locks that: if someone
        // adds a daemon round-trip for folder picking, this exhaustive match breaks to compile,
        // forcing a conscious decision rather than a silent regression.
        fn is_folder_picking_request(r: &Request) -> bool {
            match r {
                Request::ListWorkspaces
                | Request::CreateWorkspace { .. }
                | Request::ListSessions
                | Request::CreateSession { .. }
                | Request::AttachSession { .. }
                | Request::DetachSession { .. }
                | Request::WriteStdin { .. }
                | Request::Resize { .. }
                | Request::KillSession { .. }
                | Request::GetSessionState { .. }
                | Request::DaemonShutdown { .. }
                | Request::AddWorkspaceRoot { .. }
                | Request::RemoveWorkspaceRoot { .. }
                | Request::GetCommandEvents { .. } => false,
            }
        }
        assert!(!is_folder_picking_request(&Request::ListWorkspaces));
    }

    // ── upgrade_daemon_core / AppState client-slot tests (spec §6.2) ───────────────────────────
    //
    // Mock-runner pattern mirrors `launchd::tests::MockLaunchctl`/`agent()` exactly (that mock is
    // private to `launchd.rs`'s own test module, so it's replicated here rather than imported) —
    // records every `launchctl <args...>` invocation and returns scripted `LaunchctlOutput`s in
    // order, so these tests never touch the real service database.

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

    /// Build a test `LaunchdAgent` on a temp dir with the given mock runner (same shape as
    /// `launchd::tests::agent()`).
    fn test_agent<'a>(
        runner: &'a dyn crate::launchd::LaunchctlRunner,
        root: &std::path::Path,
    ) -> crate::launchd::LaunchdAgent<'a> {
        crate::launchd::LaunchdAgent {
            runner,
            uid: 501,
            launch_agents_dir: root.join("LaunchAgents"),
            app_support_dir: root.join("AppSupport"),
            daemon_path: std::path::PathBuf::from(
                "/Applications/Builder Pro AI.app/Contents/MacOS/bpa-sessiond",
            ),
            socket_path: std::path::PathBuf::from("/tmp/bpa-501/d.sock"),
            label: crate::launchd::LABEL,
            stdout_log_name: "sessiond.out.log",
            stderr_log_name: "sessiond.err.log",
        }
    }

    #[tokio::test]
    async fn upgrade_daemon_core_drains_then_kickstarts() {
        // The stub daemon replies Ack to whatever it receives; the contract is best-effort drain
        // (an OLD daemon can't parse the v2 DaemonShutdown frame at all), so this test's primary
        // assertion is the kickstart shape/success — not that the stub specifically observed
        // DaemonShutdown{drain:true} (asserting that too would require a stub that inspects the
        // request, which the shared `connect_to_stub` helper doesn't expose here).
        let (client, _sock) = super::commands_over_stub_daemon::connect_to_stub(|req| match req {
            Request::DaemonShutdown { drain } => {
                assert!(
                    drain,
                    "upgrade_daemon_core must request a drain, not a hard kill"
                );
                Response::Ack
            }
            other => panic!("expected DaemonShutdown, got {other:?}"),
        })
        .await;

        let mock = MockLaunchctl::new(vec![ok_output()]);
        let tmp = tempfile::tempdir().unwrap();
        let agent = test_agent(&mock, tmp.path());

        let result = upgrade_daemon_core(Some(Arc::new(client)), &agent).await;

        assert!(result.is_ok(), "expected Ok, got {result:?}");
        assert_eq!(
            mock.calls(),
            vec![vec![
                "kickstart",
                "-k",
                "gui/501/ai.builderpro.desktop.sessiond"
            ]]
        );
    }

    #[tokio::test]
    async fn upgrade_daemon_core_without_client_still_kickstarts() {
        let mock = MockLaunchctl::new(vec![ok_output()]);
        let tmp = tempfile::tempdir().unwrap();
        let agent = test_agent(&mock, tmp.path());

        let result = upgrade_daemon_core(None, &agent).await;

        assert!(result.is_ok(), "expected Ok, got {result:?}");
        assert_eq!(
            mock.calls(),
            vec![vec![
                "kickstart",
                "-k",
                "gui/501/ai.builderpro.desktop.sessiond"
            ]],
            "kickstart must still run even with no client to drain"
        );
    }

    #[tokio::test]
    async fn upgrade_daemon_core_surfaces_kickstart_failure() {
        let boom = crate::launchd::LaunchctlOutput {
            code: 78,
            stdout: String::new(),
            stderr: "Operation not permitted (TCC)".into(),
        };
        let mock = MockLaunchctl::new(vec![boom]);
        let tmp = tempfile::tempdir().unwrap();
        let agent = test_agent(&mock, tmp.path());

        let err = upgrade_daemon_core(None, &agent).await.unwrap_err();

        match err {
            CommandError::UpgradeFailed { reason } => {
                assert!(
                    reason.contains("Operation not permitted"),
                    "reason must carry the honest launchctl failure, got: {reason}"
                );
            }
            other => panic!("expected UpgradeFailed, got {other:?}"),
        }
    }

    // ── orchd_upgrade_core tests (S3 T12, spec §9) — mirrors the upgrade_daemon_core block
    // ── above exactly, over the orchd stub/agent instead of the sessiond ones. ──────────────

    /// Mirrors `test_agent` exactly, with the orchd identity (`ORCHD_LABEL`/`bpa-orchd`/
    /// `orchd.sock`) — so `mock.calls()` below asserts the orchd `launchctl` invocation string,
    /// not the sessiond one.
    fn test_orchd_agent<'a>(
        runner: &'a dyn crate::launchd::LaunchctlRunner,
        root: &std::path::Path,
    ) -> crate::launchd::LaunchdAgent<'a> {
        crate::launchd::LaunchdAgent {
            runner,
            uid: 501,
            launch_agents_dir: root.join("LaunchAgents"),
            app_support_dir: root.join("AppSupport"),
            daemon_path: std::path::PathBuf::from(
                "/Applications/Builder Pro AI.app/Contents/MacOS/bpa-orchd",
            ),
            socket_path: std::path::PathBuf::from("/tmp/bpa-501/orchd.sock"),
            label: crate::launchd::ORCHD_LABEL,
            stdout_log_name: "orchd.out.log",
            stderr_log_name: "orchd.err.log",
        }
    }

    #[tokio::test]
    async fn orchd_upgrade_core_drains_then_kickstarts() {
        let (client, _sock) =
            super::orchd_commands_over_stub_daemon::connect_orchd_to_stub(|req| match req {
                OrchdRequest::OrchdShutdown { drain } => {
                    assert!(
                        drain,
                        "orchd_upgrade_core must request a drain, not a hard kill"
                    );
                    OrchdResponse::Ack
                }
                other => panic!("expected OrchdShutdown, got {other:?}"),
            })
            .await;

        let mock = MockLaunchctl::new(vec![ok_output()]);
        let tmp = tempfile::tempdir().unwrap();
        let agent = test_orchd_agent(&mock, tmp.path());

        let result = orchd_upgrade_core(Some(Arc::new(client)), &agent).await;

        assert!(result.is_ok(), "expected Ok, got {result:?}");
        assert_eq!(
            mock.calls(),
            vec![vec![
                "kickstart",
                "-k",
                "gui/501/ai.builderpro.desktop.orchd"
            ]]
        );
    }

    #[tokio::test]
    async fn orchd_upgrade_core_without_client_still_kickstarts() {
        let mock = MockLaunchctl::new(vec![ok_output()]);
        let tmp = tempfile::tempdir().unwrap();
        let agent = test_orchd_agent(&mock, tmp.path());

        let result = orchd_upgrade_core(None, &agent).await;

        assert!(result.is_ok(), "expected Ok, got {result:?}");
        assert_eq!(
            mock.calls(),
            vec![vec![
                "kickstart",
                "-k",
                "gui/501/ai.builderpro.desktop.orchd"
            ]],
            "kickstart must still run even with no client to drain"
        );
    }

    #[tokio::test]
    async fn orchd_upgrade_core_surfaces_kickstart_failure() {
        let boom = crate::launchd::LaunchctlOutput {
            code: 78,
            stdout: String::new(),
            stderr: "Operation not permitted (TCC)".into(),
        };
        let mock = MockLaunchctl::new(vec![boom]);
        let tmp = tempfile::tempdir().unwrap();
        let agent = test_orchd_agent(&mock, tmp.path());

        let err = orchd_upgrade_core(None, &agent).await.unwrap_err();

        match err {
            CommandError::UpgradeFailed { reason } => {
                assert!(
                    reason.contains("Operation not permitted"),
                    "reason must carry the honest launchctl failure, got: {reason}"
                );
            }
            other => panic!("expected UpgradeFailed, got {other:?}"),
        }
    }

    // `AppState::client()`/`set_client()` are one-line delegates to `slot_client`/a direct
    // `RwLock` write over the bare `ClientSlot` (see their definitions above) — tested here
    // directly against a `ClientSlot`, rather than through a fully-constructed `AppState`, because
    // `AppState.broker: Arc<Broker>` can only be built from a real Tauri `AppHandle`, which
    // requires a genuine OS event loop on the main thread (confirmed empirically: `tauri::
    // Builder::default().build(...)` panics with "EventLoop must be created on the main thread"
    // when run from a `cargo test` worker thread, and `tauri::test`'s mock builder only ever
    // produces a `MockRuntime`-typed handle, which does not satisfy `AppState`'s `Wry`-defaulted
    // `Broker` field). These tests exercise the exact same slot logic `AppState::client()` calls.

    #[test]
    fn app_state_client_returns_disconnected_when_slot_empty() {
        let slot: ClientSlot = Arc::new(std::sync::RwLock::new(None));
        assert_eq!(slot_client(&slot).unwrap_err(), CommandError::Disconnected);
    }

    #[tokio::test]
    async fn app_state_client_returns_client_when_present() {
        let slot: ClientSlot = Arc::new(std::sync::RwLock::new(None));
        assert_eq!(slot_client(&slot).unwrap_err(), CommandError::Disconnected);

        let (client, _sock) =
            super::commands_over_stub_daemon::connect_to_stub(|_req| Response::Ack).await;
        *slot.write().unwrap() = Some(Arc::new(client));

        let got = slot_client(&slot).expect("slot must now yield the client");
        assert!(
            got.request(Request::ListWorkspaces).await.is_ok(),
            "the returned client must be the real, live one"
        );
    }

    // ── AppState::orchd()/slot_orchd (S3 T12, spec §9) — mirrors the ClientSlot block above ──

    #[test]
    fn app_state_orchd_returns_disconnected_when_slot_empty() {
        let slot: OrchdClientSlot = Arc::new(std::sync::RwLock::new(None));
        assert_eq!(slot_orchd(&slot).unwrap_err(), CommandError::Disconnected);
    }

    #[tokio::test]
    async fn app_state_orchd_returns_client_when_present() {
        let slot: OrchdClientSlot = Arc::new(std::sync::RwLock::new(None));
        assert_eq!(slot_orchd(&slot).unwrap_err(), CommandError::Disconnected);

        let (client, _sock) =
            super::orchd_commands_over_stub_daemon::connect_orchd_to_stub(|_req| {
                OrchdResponse::Pong
            })
            .await;
        *slot.write().unwrap() = Some(Arc::new(client));

        let got = slot_orchd(&slot).expect("slot must now yield the client");
        assert!(
            got.request(OrchdRequest::Ping).await.is_ok(),
            "the returned client must be the real, live one"
        );
    }

    // ── DaemonStatus / StatusSlot (finding [12], spec §6.2) ─────────────────────────────────
    //
    // Tested here directly against a bare `StatusSlot`, same rationale as `slot_client` above
    // (`AppState` itself needs a real Tauri `AppHandle` to construct) — `read_status`/
    // `write_status` are exactly what `AppState.status`/the `daemon_status` command delegate to.

    #[test]
    fn daemon_status_serializes_with_camel_case_tag() {
        let v = serde_json::to_value(DaemonStatus::Connected).unwrap();
        assert_eq!(v["kind"], "connected");

        let v2 = serde_json::to_value(DaemonStatus::Disconnected).unwrap();
        assert_eq!(v2["kind"], "disconnected");

        let v3 = serde_json::to_value(DaemonStatus::Incompatible {
            daemon_min: 2,
            daemon_max: 2,
        })
        .unwrap();
        assert_eq!(v3["kind"], "incompatible");
        assert_eq!(v3["daemonMin"], 2);
        assert_eq!(v3["daemonMax"], 2);
    }

    #[test]
    fn status_slot_starts_disconnected_and_reflects_writes() {
        let slot: StatusSlot = Arc::new(std::sync::Mutex::new(DaemonStatus::Disconnected));
        assert_eq!(read_status(&slot), DaemonStatus::Disconnected);

        write_status(&slot, DaemonStatus::Connected);
        assert_eq!(read_status(&slot), DaemonStatus::Connected);

        write_status(
            &slot,
            DaemonStatus::Incompatible {
                daemon_min: 3,
                daemon_max: 3,
            },
        );
        assert_eq!(
            read_status(&slot),
            DaemonStatus::Incompatible {
                daemon_min: 3,
                daemon_max: 3
            }
        );

        // Reflects a later recovery too (mid-session reconnect after a manual fix).
        write_status(&slot, DaemonStatus::Connected);
        assert_eq!(read_status(&slot), DaemonStatus::Connected);
    }

    #[tokio::test]
    async fn daemon_status_command_returns_whatever_the_slot_holds() {
        // `daemon_status` itself needs a live `State<AppState>` (requires a real AppHandle), so
        // this exercises the identical underlying logic (`read_status` over a `StatusSlot`) the
        // command is a one-line delegate to — the command body is `Ok(read_status(&state.status))`,
        // nothing else to diverge.
        let slot: StatusSlot = Arc::new(std::sync::Mutex::new(DaemonStatus::Disconnected));
        assert_eq!(
            Ok::<DaemonStatus, CommandError>(read_status(&slot)),
            Ok(DaemonStatus::Disconnected)
        );

        write_status(
            &slot,
            DaemonStatus::Incompatible {
                daemon_min: 5,
                daemon_max: 6,
            },
        );
        assert_eq!(
            Ok::<DaemonStatus, CommandError>(read_status(&slot)),
            Ok(DaemonStatus::Incompatible {
                daemon_min: 5,
                daemon_max: 6
            })
        );
    }
}

/// Command-level integration tests: exercise the real `#[tauri::command]` request-building logic
/// (`build_create_session`, `create_workspace`'s pre-flight `validate_dir`) against a real
/// `DaemonClient` talking to a stub Unix-domain-socket daemon, reusing the T14 stub-daemon
/// pattern (handshake, then reply to each `Request` with a distinguishable `Response`).
///
/// These do NOT exercise the `#[tauri::command]` fns themselves (that needs a live `AppHandle`/
/// `State`, which needs a running Tauri app — out of scope for a unit test), only the
/// `DaemonClient::request` round-trip and the `validate_dir`-before-request-ing behavior that the
/// commands are thin wrappers over.
#[cfg(test)]
pub(crate) mod commands_over_stub_daemon {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use bpa_protocol::preamble::{decode_client_preamble, encode_daemon_reply, DaemonReply};
    use bpa_protocol::{encode_frame, Frame, FrameDecoder, Request, Response};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{UnixListener, UnixStream};

    use crate::socket_client::DaemonClient;

    use super::*;

    // Serializes tests that mutate XDG_RUNTIME_DIR (DaemonClient::connect() resolves the socket
    // path from it) across the whole set/connect/remove window, matching the discipline
    // socket_client's own tests use for the same shared-process-state reason. `tokio::sync::Mutex`
    // (not `std::sync::Mutex`) because the guard must stay held across the `connect().await` below
    // — that `.await` is exactly the section that needs serializing, since a second test's
    // `set_var` racing in in the middle of it would otherwise redirect this test's `connect()` to
    // the wrong socket.
    //
    // `pub(super)` (not private): `orchd_commands_over_stub_daemon`'s `OrchdClient::connect` reads
    // the SAME process-wide `XDG_RUNTIME_DIR` — a second, independent lock there would not
    // serialize against this one and reintroduces exactly the race this lock exists to prevent
    // (both `commands.rs` test modules manipulating the same env var must share ONE lock).
    pub(super) static ENV_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    async fn read_frame(stream: &mut UnixStream) -> Option<Frame> {
        // Read exactly one length-prefixed CBOR frame via the shared FrameDecoder: read the
        // 4-byte LE length, then that many body bytes, feed both into the decoder, and take the
        // single frame it yields (mirrors the length-prefix framing `encode_frame` produces).
        let mut len_buf = [0u8; 4];
        stream.read_exact(&mut len_buf).await.ok()?;
        let len = u32::from_le_bytes(len_buf) as usize;
        let mut body = vec![0u8; len];
        stream.read_exact(&mut body).await.ok()?;
        let mut decoder = FrameDecoder::new();
        decoder.push(&len_buf);
        decoder.push(&body);
        decoder.decode().ok()?.into_iter().next()
    }

    async fn write_stub_frame(stream: &mut UnixStream, frame: &Frame) {
        let bytes = encode_frame(frame).unwrap();
        stream.write_all(&bytes).await.unwrap();
        stream.flush().await.unwrap();
    }

    /// Read and decode one client preamble off the wire (mirrors
    /// `bpa_sessiond::socket_server::read_client_preamble`'s test-stub shape): the fixed 10-byte
    /// header (`magic | min | max | build_len`), then exactly `build_len` more bytes.
    async fn read_client_preamble_stub(stream: &mut UnixStream) -> bpa_protocol::ClientPreamble {
        let mut header = [0u8; 10];
        stream.read_exact(&mut header).await.unwrap();
        let build_len = u16::from_le_bytes(header[8..10].try_into().unwrap()) as usize;
        let mut buf = header.to_vec();
        if build_len > 0 {
            let mut build = vec![0u8; build_len];
            stream.read_exact(&mut build).await.unwrap();
            buf.extend_from_slice(&build);
        }
        decode_client_preamble(&buf).expect("valid client preamble")
    }

    /// Bind a stub daemon under a fresh tempdir, handshake, then reply to exactly one Request with
    /// `respond`. Returns the connected, handshaken `DaemonClient` (via `XDG_RUNTIME_DIR`, so
    /// `DaemonClient::connect` resolves to this stub's socket).
    pub(crate) async fn connect_to_stub<F>(respond: F) -> (DaemonClient, PathBuf)
    where
        F: FnOnce(Request) -> Response + Send + 'static,
    {
        let dir = tempfile::tempdir().unwrap();
        let bpa_dir = dir.path().join("bpa");
        std::fs::create_dir_all(&bpa_dir).unwrap();
        let sock_path = bpa_dir.join("d.sock");

        let listener = UnixListener::bind(&sock_path).unwrap();
        let ready = Arc::new(AtomicBool::new(false));
        let ready2 = ready.clone();
        let sock_path2 = sock_path.clone();
        tokio::spawn(async move {
            ready2.store(true, Ordering::SeqCst);
            let (mut stream, _) = listener.accept().await.unwrap();
            let _client_preamble = read_client_preamble_stub(&mut stream).await;
            let reply = encode_daemon_reply(&DaemonReply::Accepted {
                chosen: 2,
                build: "stub".into(),
            });
            stream.write_all(&reply).await.unwrap();
            stream.flush().await.unwrap();

            if let Some(Frame::Request { id, req }) = read_frame(&mut stream).await {
                let res = respond(req);
                write_stub_frame(&mut stream, &Frame::Response { id, res }).await;
            }
            // Keep the stream (and tempdir) alive for the rest of the test.
            let _ = sock_path2;
            std::future::pending::<()>().await;
        });
        while !ready.load(Ordering::SeqCst) {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }

        // set_var/remove_var are `unsafe` under edition 2024's stricter env API; sound here only
        // because `ENV_TEST_LOCK` (an async-aware `tokio::sync::Mutex`) is held across the whole
        // set/connect/remove window, serializing every test in this module against this exact race.
        let client = {
            let _guard = ENV_TEST_LOCK.lock().await;
            unsafe {
                std::env::set_var("XDG_RUNTIME_DIR", dir.path());
            }
            let client = DaemonClient::connect("test-build".to_string())
                .await
                .unwrap();
            unsafe {
                std::env::remove_var("XDG_RUNTIME_DIR");
            }
            client
        };
        std::mem::forget(dir); // keep the tempdir (and its socket) alive for the test's duration

        (client, sock_path)
    }

    #[tokio::test]
    async fn create_session_round_trips_through_real_daemon_client() {
        let (client, _sock) = connect_to_stub(|req| match req {
            Request::CreateSession {
                workspace_id,
                cols,
                rows,
                ..
            } => {
                assert_eq!(workspace_id, "ws-1");
                assert_eq!((cols, rows), (80, 24));
                Response::Session(SessionMeta {
                    id: "sess-new".into(),
                    workspace_id: "ws-1".into(),
                    title: "shell".into(),
                    shell: "/bin/zsh".into(),
                    cwd: "/work".into(),
                    cols: 80,
                    rows: 24,
                    lifecycle: bpa_protocol::SessionLifecycle::AtPrompt,
                    waiting_for_input: false,
                    is_active: true,
                    created_at: 0,
                })
            }
            other => panic!("expected CreateSession, got {other:?}"),
        })
        .await;

        let req = build_create_session("ws-1".to_string(), None);
        let res = client.request(req).await.unwrap();
        let meta = expect_session(res).unwrap();
        assert_eq!(meta.id, "sess-new");
        assert_eq!(meta.workspace_id, "ws-1");
    }

    #[tokio::test]
    async fn daemon_error_response_becomes_command_error_daemon_end_to_end() {
        let (client, _sock) = connect_to_stub(|_req| Response::Error {
            code: "InvalidWorkspaceRoot".into(),
            message: "no such dir".into(),
        })
        .await;

        let res = client.request(Request::ListWorkspaces).await;
        // ClientError::Daemon is raised directly by DaemonClient::request (spec §7 correlation:
        // Response::Error rejects the awaiting Promise) — confirm the CommandError From impl
        // reshapes it identically to the pure err_from_response path tested above.
        let err: CommandError = res.unwrap_err().into();
        assert_eq!(
            err,
            CommandError::Daemon {
                code: "InvalidWorkspaceRoot".into(),
                message: "no such dir".into()
            }
        );
    }

    // Escaping-symlink layout (mirrors bpa-paths' `symlink_escaping_parent_is_rejected`):
    //   base/outside/          (real dir, OUTSIDE `named`)
    //   base/named/link -> ../outside
    // validate_dir(base/named/link) canonicalizes to base/outside, whose parent (base) != the
    // canonical parent of the input (base/named) ⇒ SymlinkEscape. Returns (tempdir, link path).
    fn escaping_symlink_layout() -> (tempfile::TempDir, String) {
        let base = tempfile::tempdir().unwrap();
        let outside = base.path().join("outside");
        let named = base.path().join("named");
        std::fs::create_dir(&outside).unwrap();
        std::fs::create_dir(&named).unwrap();
        let link = named.join("link");
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        let link_str = link.to_string_lossy().into_owned();
        (base, link_str)
    }

    fn opts_with_cwd(cwd: Option<&str>) -> Option<CreateOpts> {
        Some(CreateOpts {
            shell: None,
            cwd: cwd.map(|s| s.to_string()),
            env_overrides: vec![],
            cols: None,
            rows: None,
        })
    }

    // ---- REAL coverage of `create_session`'s core-side cwd pre-flight (spec §13/§16). Calls the
    // actual `preflight_cwd` guard (which `create_session` invokes before any socket round-trip) and
    // asserts on its real return — not a reconstruction of a constructed error. ----
    #[test]
    fn preflight_cwd_accepts_none_empty_and_valid_dir() {
        // None ⇒ Ok (daemon defaults an omitted cwd to $HOME).
        assert!(
            preflight_cwd(&None).is_ok(),
            "omitted cwd must pass (defaults to $HOME)"
        );
        assert!(
            preflight_cwd(&Some(CreateOpts::default())).is_ok(),
            "opts with cwd=None must pass"
        );
        // Empty-string cwd ⇒ Ok (also "daemon defaults to $HOME").
        assert!(
            preflight_cwd(&opts_with_cwd(Some(""))).is_ok(),
            "empty cwd must pass"
        );
        // A real, existing directory ⇒ Ok.
        let dir = tempfile::tempdir().unwrap();
        let cwd = dir.path().to_string_lossy().into_owned();
        assert!(
            preflight_cwd(&opts_with_cwd(Some(&cwd))).is_ok(),
            "valid dir must pass"
        );
    }

    #[test]
    fn preflight_cwd_rejects_missing_relative_and_symlink_escape() {
        // Nonexistent absolute path ⇒ CwdMissing.
        let dir = tempfile::tempdir().unwrap();
        let gone = dir
            .path()
            .join("does-not-exist")
            .to_string_lossy()
            .into_owned();
        match preflight_cwd(&opts_with_cwd(Some(&gone))).unwrap_err() {
            CommandError::Daemon { code, .. } => assert_eq!(code, "CwdMissing"),
            other => panic!("expected Daemon/CwdMissing, got {other:?}"),
        }
        // Relative path ⇒ RelativePath (rejected before any fs access).
        match preflight_cwd(&opts_with_cwd(Some("relative/not/absolute"))).unwrap_err() {
            CommandError::Daemon { code, .. } => assert_eq!(code, "RelativePath"),
            other => panic!("expected Daemon/RelativePath, got {other:?}"),
        }
        // Symlink escaping its lexical parent ⇒ SymlinkEscape.
        let (_layout, link) = escaping_symlink_layout();
        match preflight_cwd(&opts_with_cwd(Some(&link))).unwrap_err() {
            CommandError::Daemon { code, .. } => assert_eq!(code, "SymlinkEscape"),
            other => panic!("expected Daemon/SymlinkEscape, got {other:?}"),
        }
    }

    // ---- REAL coverage of `create_workspace`'s root_path pre-flight (spec §13/§16): calls the
    // actual `preflight_workspace_root` and asserts on its real return (canonicalized path on
    // success, real wire codes on failure). ----
    #[test]
    fn preflight_workspace_root_canonicalizes_valid_and_rejects_bad() {
        // Valid dir ⇒ Ok(canonicalized string) — this is exactly what the wrapper forwards.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_string_lossy().into_owned();
        let expected = std::fs::canonicalize(&root)
            .unwrap()
            .to_string_lossy()
            .into_owned();
        assert_eq!(
            preflight_workspace_root(&root).expect("valid root"),
            expected,
            "valid root must be forwarded canonicalized"
        );
        // Relative ⇒ RelativePath.
        match preflight_workspace_root("relative/not/absolute").unwrap_err() {
            CommandError::Daemon { code, .. } => assert_eq!(code, "RelativePath"),
            other => panic!("expected Daemon/RelativePath, got {other:?}"),
        }
        // Nonexistent absolute ⇒ CwdMissing (the code `PathError::Missing` yields).
        let gone = dir.path().join("nope").to_string_lossy().into_owned();
        match preflight_workspace_root(&gone).unwrap_err() {
            CommandError::Daemon { code, .. } => assert_eq!(code, "CwdMissing"),
            other => panic!("expected Daemon/CwdMissing, got {other:?}"),
        }
        // Symlink escape ⇒ SymlinkEscape.
        let (_layout, link) = escaping_symlink_layout();
        match preflight_workspace_root(&link).unwrap_err() {
            CommandError::Daemon { code, .. } => assert_eq!(code, "SymlinkEscape"),
            other => panic!("expected Daemon/SymlinkEscape, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn create_workspace_accepts_valid_dir_and_forwards_canonicalized_root() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().to_path_buf();
        let expected_canonical = std::fs::canonicalize(&root).unwrap();
        let expected_str = expected_canonical.to_string_lossy().into_owned();

        let (client, _sock) = {
            let expected_str = expected_str.clone();
            connect_to_stub(move |req| match req {
                Request::CreateWorkspace { name, root_path } => {
                    assert_eq!(name, "My Workspace");
                    assert_eq!(root_path, expected_str);
                    Response::Workspace(Workspace {
                        id: "w-1".into(),
                        name,
                        roots: vec![root_path.clone()],
                        root_path,
                    })
                }
                other => panic!("expected CreateWorkspace, got {other:?}"),
            })
            .await
        };

        let validated = crate::paths::validate_dir(&root).unwrap();
        let root_path = validated.to_string_lossy().into_owned();
        let res = client
            .request(Request::CreateWorkspace {
                name: "My Workspace".to_string(),
                root_path,
            })
            .await
            .unwrap();
        let ws = expect_workspace(res).unwrap();
        assert_eq!(ws.id, "w-1");
        assert_eq!(ws.root_path, expected_str);
    }

    // ── workspace-root + command-events wrappers (spec §3.3/§6.6, S2 Task 7) ───────────────────
    //
    // Same rationale as `create_workspace_accepts_valid_dir_...` above: the `#[tauri::command]`
    // fns need a live `State<AppState>` (out of scope for a unit test), but each one's entire body
    // is `expect_*(client.request(req).await?)` — these drive that exact shape directly against a
    // real `DaemonClient` + stub daemon.

    #[tokio::test]
    async fn add_workspace_root_round_trips_through_real_daemon_client() {
        let (client, _sock) = connect_to_stub(|req| match req {
            Request::AddWorkspaceRoot { workspace_id, path } => {
                assert_eq!(workspace_id, "w-1");
                assert_eq!(path, "/second/root");
                Response::Workspace(Workspace {
                    id: "w-1".into(),
                    name: "N".into(),
                    root_path: "/first/root".into(),
                    roots: vec!["/first/root".into(), "/second/root".into()],
                })
            }
            other => panic!("expected AddWorkspaceRoot, got {other:?}"),
        })
        .await;

        let res = client
            .request(Request::AddWorkspaceRoot {
                workspace_id: "w-1".to_string(),
                path: "/second/root".to_string(),
            })
            .await
            .unwrap();
        let ws = expect_workspace(res).unwrap();
        assert_eq!(ws.id, "w-1");
        assert_eq!(
            ws.roots,
            vec!["/first/root".to_string(), "/second/root".to_string()]
        );
    }

    #[tokio::test]
    async fn remove_workspace_root_last_root_error_maps_to_command_error_daemon() {
        let (client, _sock) = connect_to_stub(|req| match req {
            Request::RemoveWorkspaceRoot { workspace_id, path } => {
                assert_eq!(workspace_id, "w-1");
                assert_eq!(path, "/only/root");
                Response::Error {
                    code: "LastRoot".into(),
                    message: "a workspace must keep at least one root".into(),
                }
            }
            other => panic!("expected RemoveWorkspaceRoot, got {other:?}"),
        })
        .await;

        // `Response::Error` is raised directly as `Err(ClientError::Daemon)` by
        // `DaemonClient::request` (spec §7 correlation), same as
        // `daemon_error_response_becomes_command_error_daemon_end_to_end` above — it never reaches
        // `expect_workspace` as an `Ok(Response::Error)`.
        let res = client
            .request(Request::RemoveWorkspaceRoot {
                workspace_id: "w-1".to_string(),
                path: "/only/root".to_string(),
            })
            .await;
        let err: CommandError = res.unwrap_err().into();
        match err {
            CommandError::Daemon { code, .. } => assert_eq!(code, "LastRoot"),
            other => panic!("expected CommandError::Daemon, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn get_command_events_round_trips_through_real_daemon_client() {
        let (client, _sock) = connect_to_stub(|req| match req {
            Request::GetCommandEvents { session_id, limit } => {
                assert_eq!(session_id, "sess-1");
                assert_eq!(limit, 50);
                Response::CommandEvents(vec![
                    CommandEvent {
                        session_id: "sess-1".into(),
                        seq: 2,
                        ts: 200,
                        kind: "finished".into(),
                        exit_code: Some(0),
                        origin: "osc133".into(),
                    },
                    CommandEvent {
                        session_id: "sess-1".into(),
                        seq: 1,
                        ts: 100,
                        kind: "started".into(),
                        exit_code: None,
                        origin: "osc133".into(),
                    },
                ])
            }
            other => panic!("expected GetCommandEvents, got {other:?}"),
        })
        .await;

        let res = client
            .request(Request::GetCommandEvents {
                session_id: "sess-1".to_string(),
                limit: 50,
            })
            .await
            .unwrap();
        let events = expect_command_events(res).unwrap();
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].seq, 2);
        assert_eq!(events[0].kind, "finished");
        assert_eq!(events[1].seq, 1);
        assert_eq!(events[1].kind, "started");
    }

    // ── write_stdin chunking end-to-end (finding [2], spec's item C4) ──────────────────────────
    //
    // `write_stdin` (the `#[tauri::command]`) needs a live `State<AppState>`/`AppHandle` (out of
    // scope for a unit test, same limitation documented at this module's top) — but its entire body
    // is `for req in build_write_stdin_chunks(...) { expect_ack(client.request(req).await?)?; }`.
    // These tests drive that exact loop directly against a real `DaemonClient` + stub daemon, which
    // is real coverage of the whole chunking behavior, not a reconstruction.

    /// Bind a stub daemon that replies to an unbounded SEQUENCE of requests (unlike `connect_to_stub`,
    /// which only ever answers one) — each incoming `Frame::Request` is handed to `respond`, in
    /// order, and the reply is written back before the next read. Needed for the chunking tests,
    /// which send several sequential `WriteStdin` requests on the same connection.
    async fn connect_to_stub_sequence<F>(mut respond: F) -> (DaemonClient, PathBuf)
    where
        F: FnMut(Request) -> Response + Send + 'static,
    {
        let dir = tempfile::tempdir().unwrap();
        let bpa_dir = dir.path().join("bpa");
        std::fs::create_dir_all(&bpa_dir).unwrap();
        let sock_path = bpa_dir.join("d.sock");

        let listener = UnixListener::bind(&sock_path).unwrap();
        let ready = Arc::new(AtomicBool::new(false));
        let ready2 = ready.clone();
        tokio::spawn(async move {
            ready2.store(true, Ordering::SeqCst);
            let (mut stream, _) = listener.accept().await.unwrap();
            let _client_preamble = read_client_preamble_stub(&mut stream).await;
            let reply = encode_daemon_reply(&DaemonReply::Accepted {
                chosen: 2,
                build: "stub".into(),
            });
            stream.write_all(&reply).await.unwrap();
            stream.flush().await.unwrap();

            loop {
                let Some(Frame::Request { id, req }) = read_frame(&mut stream).await else {
                    break;
                };
                let res = respond(req);
                write_stub_frame(&mut stream, &Frame::Response { id, res }).await;
            }
        });
        while !ready.load(Ordering::SeqCst) {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }

        let client = {
            let _guard = ENV_TEST_LOCK.lock().await;
            unsafe {
                std::env::set_var("XDG_RUNTIME_DIR", dir.path());
            }
            let client = DaemonClient::connect("test-build".to_string())
                .await
                .unwrap();
            unsafe {
                std::env::remove_var("XDG_RUNTIME_DIR");
            }
            client
        };
        std::mem::forget(dir);

        (client, sock_path)
    }

    /// Drive `build_write_stdin_chunks`' output through `client.request` sequentially, exactly like
    /// `write_stdin`'s command body — stops and returns the first error, same as the real command.
    async fn drive_write_stdin(
        client: &DaemonClient,
        session_id: SessionId,
        data: String,
    ) -> Result<(), CommandError> {
        for req in build_write_stdin_chunks(session_id, data) {
            expect_ack(client.request(req).await?)?;
        }
        Ok(())
    }

    #[tokio::test]
    async fn write_stdin_chunking_sends_ordered_chunks_stub_observes_and_reassembles() {
        // 3.5 MiB input -> 4 sequential WriteStdin frames, observed by the stub in order; the
        // reassembled bytes must exactly match the original input.
        let total = WRITE_STDIN_CHUNK * 3 + WRITE_STDIN_CHUNK / 2;
        let data: String = (0..total)
            .map(|i| (b'a' + (i % 26) as u8) as char)
            .collect();
        let expected_bytes = data.clone().into_bytes();

        let observed: Arc<std::sync::Mutex<Vec<Vec<u8>>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        let observed2 = observed.clone();
        let (client, _sock) = connect_to_stub_sequence(move |req| match req {
            Request::WriteStdin { session_id, bytes } => {
                assert_eq!(
                    session_id, "sess-1",
                    "every chunk must target the same session"
                );
                observed2.lock().unwrap().push(bytes);
                Response::Ack
            }
            other => panic!("expected WriteStdin, got {other:?}"),
        })
        .await;

        drive_write_stdin(&client, "sess-1".to_string(), data)
            .await
            .expect("chunked write_stdin must succeed end to end");

        let chunks = observed.lock().unwrap().clone();
        assert_eq!(chunks.len(), 4, "expected 4 sequential WriteStdin frames");
        for (i, c) in chunks.iter().enumerate() {
            assert!(
                c.len() <= WRITE_STDIN_CHUNK,
                "chunk {i} exceeds WRITE_STDIN_CHUNK: {}",
                c.len()
            );
        }
        let reassembled: Vec<u8> = chunks.into_iter().flatten().collect();
        assert_eq!(
            reassembled, expected_bytes,
            "chunks observed by the stub, in order, must reassemble byte-identically"
        );
    }

    #[tokio::test]
    async fn write_stdin_chunking_stops_and_surfaces_error_on_chunk_failure() {
        // The stub Acks the first chunk, then rejects the second with a daemon error. The chunk
        // loop must stop immediately (never send the 3rd/4th chunks) and surface that error
        // honestly, rather than silently swallowing it or continuing.
        let total = WRITE_STDIN_CHUNK * 3 + WRITE_STDIN_CHUNK / 2; // 4 chunks worth
        let data: String = "z".repeat(total);

        let seen: Arc<std::sync::atomic::AtomicUsize> =
            Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let seen2 = seen.clone();
        let (client, _sock) = connect_to_stub_sequence(move |req| match req {
            Request::WriteStdin { .. } => {
                let n = seen2.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if n == 1 {
                    Response::Error {
                        code: "NoSuchSession".into(),
                        message: "session gone mid-paste".into(),
                    }
                } else {
                    Response::Ack
                }
            }
            other => panic!("expected WriteStdin, got {other:?}"),
        })
        .await;

        let err = drive_write_stdin(&client, "sess-1".to_string(), data)
            .await
            .unwrap_err();
        match err {
            CommandError::Daemon { code, .. } => assert_eq!(code, "NoSuchSession"),
            other => panic!("expected Daemon error surfaced honestly, got {other:?}"),
        }
        assert_eq!(
            seen.load(std::sync::atomic::Ordering::SeqCst),
            2,
            "must stop after the failing chunk, never sending the remaining ones"
        );
    }

    // ── write_stdin per-session serialization (round-2 regression R3) ──────────────────────────
    //
    // `write_stdin_locked` must hold session A's lock across A's whole chunk loop so a concurrent
    // call for A (issued mid-paste, exactly like a fast keystroke or a second paste racing the
    // frontend's fire-and-forget `writeStdin` invocations) cannot interleave a request between two
    // of A's chunks; a concurrent call for a DIFFERENT session B must NOT be blocked by A's lock.

    /// A stub daemon that Acks every request but stalls the FIRST `WriteStdin` chunk for session
    /// `stall_session` until `release` is notified — so a concurrent second call for the same
    /// session has a wide window to (if unserialized) enqueue its own request in between this
    /// call's chunks. Requests are read and responded to sequentially off the one connection
    /// (mirroring the real daemon's per-connection dispatch loop), recording each observed
    /// `(session_id, bytes)` pair in arrival order.
    type ObservedWrites = Arc<std::sync::Mutex<Vec<(SessionId, Vec<u8>)>>>;

    async fn connect_to_stub_recording(
        stall_session: SessionId,
        release: Arc<tokio::sync::Notify>,
    ) -> (DaemonClient, PathBuf, ObservedWrites) {
        let observed: ObservedWrites = Arc::new(std::sync::Mutex::new(Vec::new()));
        let observed2 = observed.clone();

        let dir = tempfile::tempdir().unwrap();
        let bpa_dir = dir.path().join("bpa");
        std::fs::create_dir_all(&bpa_dir).unwrap();
        let sock_path = bpa_dir.join("d.sock");

        let listener = UnixListener::bind(&sock_path).unwrap();
        let ready = Arc::new(AtomicBool::new(false));
        let ready2 = ready.clone();
        tokio::spawn(async move {
            ready2.store(true, Ordering::SeqCst);
            let (mut stream, _) = listener.accept().await.unwrap();
            let _client_preamble = read_client_preamble_stub(&mut stream).await;
            let reply = encode_daemon_reply(&DaemonReply::Accepted {
                chosen: 2,
                build: "stub".into(),
            });
            stream.write_all(&reply).await.unwrap();
            stream.flush().await.unwrap();

            let mut stalled_once = false;
            loop {
                let Some(Frame::Request { id, req }) = read_frame(&mut stream).await else {
                    break;
                };
                if let Request::WriteStdin { session_id, bytes } = &req {
                    observed2
                        .lock()
                        .unwrap()
                        .push((session_id.clone(), bytes.clone()));
                    if !stalled_once && *session_id == stall_session {
                        stalled_once = true;
                        // Hold this reply back so the caller's chunk loop is mid-flight, widening
                        // the window for a concurrent second call (if unserialized) to enqueue its
                        // own request before this one's next chunk goes out.
                        release.notified().await;
                    }
                }
                let res = Response::Ack;
                write_stub_frame(&mut stream, &Frame::Response { id, res }).await;
            }
        });
        while !ready.load(Ordering::SeqCst) {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }

        let client = {
            let _guard = ENV_TEST_LOCK.lock().await;
            unsafe {
                std::env::set_var("XDG_RUNTIME_DIR", dir.path());
            }
            let client = DaemonClient::connect("test-build".to_string())
                .await
                .unwrap();
            unsafe {
                std::env::remove_var("XDG_RUNTIME_DIR");
            }
            client
        };
        std::mem::forget(dir);

        (client, sock_path, observed)
    }

    #[tokio::test]
    async fn write_stdin_locked_keeps_same_session_chunks_contiguous_under_concurrency() {
        // Session "A" gets a 3-chunk paste whose first chunk is held by the stub; while it's held,
        // a second, single-chunk write_stdin call for the SAME session A is fired concurrently.
        // Without per-session serialization, A's second call could enqueue its request between A's
        // first and second chunk. With `write_stdin_locked`, A's second call must wait for A's
        // whole first call to finish, so every chunk observed for A stays contiguous, in order.
        let session_a: SessionId = "sess-A".to_string();
        let release = Arc::new(tokio::sync::Notify::new());
        let (client, _sock, observed) =
            connect_to_stub_recording(session_a.clone(), release.clone()).await;
        let client = Arc::new(client);
        let locks = Arc::new(WriteStdinLocks::new());

        let total = WRITE_STDIN_CHUNK * 2 + WRITE_STDIN_CHUNK / 2; // 3 chunks
        let data_a1: String = "a".repeat(total);
        let expected_a1_bytes = data_a1.clone().into_bytes();
        let data_a2 = "SECOND".to_string();

        let client_c1 = client.clone();
        let locks_c1 = locks.clone();
        let session_a_c1 = session_a.clone();
        let call1 = tokio::spawn(async move {
            write_stdin_locked(&locks_c1, &client_c1, session_a_c1, data_a1).await
        });

        // Give call1 time to enqueue its first chunk and have the stub stall on it.
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;

        let client_c2 = client.clone();
        let locks_c2 = locks.clone();
        let session_a_c2 = session_a.clone();
        let data_a2_clone = data_a2.clone();
        let call2 = tokio::spawn(async move {
            write_stdin_locked(&locks_c2, &client_c2, session_a_c2, data_a2_clone).await
        });

        // Give call2 a chance to race in (it must block on the lock, not send anything yet).
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
        release.notify_one();

        call1.await.unwrap().expect("call1 must succeed");
        call2.await.unwrap().expect("call2 must succeed");

        let rows = observed.lock().unwrap().clone();
        let a_rows: Vec<Vec<u8>> = rows
            .iter()
            .filter(|(sid, _)| *sid == session_a)
            .map(|(_, b)| b.clone())
            .collect();
        assert_eq!(
            a_rows.len(),
            4,
            "expected 3 chunks from call1 + 1 chunk from call2, got {}",
            a_rows.len()
        );
        // call1's 3 chunks must be contiguous and in order — call2's single chunk must not have
        // been interleaved between them.
        let reassembled_first_three: Vec<u8> = a_rows[0..3].iter().flatten().copied().collect();
        assert_eq!(
            reassembled_first_three, expected_a1_bytes,
            "call1's chunks must arrive contiguously and reassemble byte-identically — a \
             concurrent call2 for the SAME session must not interleave into the middle of call1's \
             multi-chunk paste"
        );
        assert_eq!(
            a_rows[3],
            data_a2.into_bytes(),
            "call2's single chunk must arrive only after call1's chunks are fully sent"
        );
    }

    #[tokio::test]
    async fn write_stdin_locked_different_sessions_are_not_blocked_by_each_other() {
        // `WriteStdinLocks` hands out one `tokio::sync::Mutex` PER session (`lock_for`) — holding
        // session A's lock across a slow operation must never contend with session B's lock. This
        // exercises the lock map directly (independent of any daemon/stub round-trip, which is
        // strictly serialized per-connection regardless of session — a real property of this
        // single-connection design, not something a per-session lock could or should change): if
        // `lock_for` returned the SAME mutex for every session (the bug this map exists to avoid),
        // session B's `try_lock` below would fail while A's guard is held.
        let locks = WriteStdinLocks::new();
        let session_a: SessionId = "sess-A".to_string();
        let session_b: SessionId = "sess-B".to_string();

        let lock_a = locks.lock_for(&session_a);
        let _guard_a = lock_a.lock().await;

        let lock_b = locks.lock_for(&session_b);
        let started = std::time::Instant::now();
        let guard_b = tokio::time::timeout(std::time::Duration::from_millis(500), lock_b.lock())
            .await
            .expect("session B's lock must be acquirable promptly while A's is held");
        let elapsed = started.elapsed();
        drop(guard_b);

        assert!(
            elapsed < std::time::Duration::from_millis(100),
            "acquiring session B's lock took {elapsed:?} while session A's lock was held; \
             different sessions must use independent locks and never block each other"
        );
    }

    // End-to-end companion: with two real, concurrent `write_stdin_locked` calls against two
    // different sessions on a stub that never stalls, both must complete and each session's bytes
    // must be observed exactly once, proving the lock map doesn't accidentally serialize the whole
    // client (e.g. via a single global mutex) even under real request/response round-trips.
    #[tokio::test]
    async fn write_stdin_locked_different_sessions_both_complete_concurrently_over_stub() {
        let session_a: SessionId = "sess-A".to_string();
        let session_b: SessionId = "sess-B".to_string();
        let never_stall: SessionId = "unused".to_string();
        let release = Arc::new(tokio::sync::Notify::new());
        // never_stall never matches session_a/session_b, so the stub never stalls either call.
        let (client, _sock, observed) =
            connect_to_stub_recording(never_stall, release.clone()).await;
        let client = Arc::new(client);
        let locks = Arc::new(WriteStdinLocks::new());

        let client_a = client.clone();
        let locks_a = locks.clone();
        let call_a = tokio::spawn(async move {
            write_stdin_locked(&locks_a, &client_a, session_a.clone(), "from-a".to_string()).await
        });
        let client_b = client.clone();
        let locks_b = locks.clone();
        let call_b = tokio::spawn(async move {
            write_stdin_locked(&locks_b, &client_b, session_b.clone(), "from-b".to_string()).await
        });

        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            call_a.await.unwrap().expect("call_a must succeed");
            call_b.await.unwrap().expect("call_b must succeed");
        })
        .await
        .expect("both concurrent per-session writes must complete promptly");

        let rows = observed.lock().unwrap().clone();
        assert!(
            rows.iter()
                .any(|(sid, b)| sid == "sess-A" && b == b"from-a"),
            "session A's chunk must have been observed"
        );
        assert!(
            rows.iter()
                .any(|(sid, b)| sid == "sess-B" && b == b"from-b"),
            "session B's chunk must have been observed"
        );
    }
}

/// orchd-flavored counterpart of [`commands_over_stub_daemon`] (S3 T12, spec §9): mirrors its
/// stub-daemon pattern exactly (handshake, then reply to each `OrchdRequest` with a
/// distinguishable `OrchdResponse`), instantiated over `bpa_orchd_proto`'s wire types and
/// `orchd.sock` instead of `bpa_protocol`'s and `d.sock`. The stub's `Accepted` reply uses
/// `ORCHD_DAEMON_MAX_VERSION` (the version CONST, never a hardcoded literal — spec §9's locked
/// test discipline).
#[cfg(test)]
pub(crate) mod orchd_commands_over_stub_daemon {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use bpa_orchd_proto::{
        encode_orchd_frame, OrchdFrame, OrchdFrameDecoder, OrchdRequest, OrchdResponse,
        ORCHD_DAEMON_MAX_VERSION,
    };
    use bpa_protocol::preamble::{decode_client_preamble, encode_daemon_reply, DaemonReply};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{UnixListener, UnixStream};

    use crate::orchd_client::OrchdClient;

    use super::*;

    // REUSES `commands_over_stub_daemon::ENV_TEST_LOCK` rather than declaring a second, independent
    // lock: `OrchdClient::connect` and `DaemonClient::connect` both resolve their socket path from
    // the SAME process-wide `XDG_RUNTIME_DIR` env var, so both test modules must serialize against
    // ONE shared lock — two independent locks over the same mutable global would not serialize
    // against each other and would silently reintroduce the exact race this discipline exists to
    // prevent (confirmed empirically: this module originally declared its own lock, and
    // `commands::tests::app_state_client_returns_client_when_present` — a `commands_over_stub_daemon`
    // consumer — flaked with a spurious `Disconnected` under `cargo test`'s default parallelism).
    use super::commands_over_stub_daemon::ENV_TEST_LOCK;

    async fn read_frame(stream: &mut UnixStream) -> Option<OrchdFrame> {
        let mut len_buf = [0u8; 4];
        stream.read_exact(&mut len_buf).await.ok()?;
        let len = u32::from_le_bytes(len_buf) as usize;
        let mut body = vec![0u8; len];
        stream.read_exact(&mut body).await.ok()?;
        let mut decoder = OrchdFrameDecoder::new();
        decoder.push(&len_buf);
        decoder.push(&body);
        decoder.decode().ok()?.into_iter().next()
    }

    async fn write_stub_frame(stream: &mut UnixStream, frame: &OrchdFrame) {
        let bytes = encode_orchd_frame(frame).unwrap();
        stream.write_all(&bytes).await.unwrap();
        stream.flush().await.unwrap();
    }

    async fn read_client_preamble_stub(stream: &mut UnixStream) -> bpa_protocol::ClientPreamble {
        let mut header = [0u8; 10];
        stream.read_exact(&mut header).await.unwrap();
        let build_len = u16::from_le_bytes(header[8..10].try_into().unwrap()) as usize;
        let mut buf = header.to_vec();
        if build_len > 0 {
            let mut build = vec![0u8; build_len];
            stream.read_exact(&mut build).await.unwrap();
            buf.extend_from_slice(&build);
        }
        decode_client_preamble(&buf).expect("valid client preamble")
    }

    /// Bind a stub `bpa-orchd` under a fresh tempdir, handshake, then reply to exactly one
    /// `OrchdRequest` with `respond`. Returns the connected, handshaken `OrchdClient` (via
    /// `XDG_RUNTIME_DIR`, so `OrchdClient::connect` resolves to this stub's socket).
    pub(crate) async fn connect_orchd_to_stub<F>(respond: F) -> (OrchdClient, PathBuf)
    where
        F: FnOnce(OrchdRequest) -> OrchdResponse + Send + 'static,
    {
        let dir = tempfile::tempdir().unwrap();
        let bpa_dir = dir.path().join("bpa");
        std::fs::create_dir_all(&bpa_dir).unwrap();
        let sock_path = bpa_dir.join("orchd.sock");

        let listener = UnixListener::bind(&sock_path).unwrap();
        let ready = Arc::new(AtomicBool::new(false));
        let ready2 = ready.clone();
        let sock_path2 = sock_path.clone();
        tokio::spawn(async move {
            ready2.store(true, Ordering::SeqCst);
            let (mut stream, _) = listener.accept().await.unwrap();
            let _client_preamble = read_client_preamble_stub(&mut stream).await;
            let reply = encode_daemon_reply(&DaemonReply::Accepted {
                chosen: ORCHD_DAEMON_MAX_VERSION,
                build: "stub".into(),
            });
            stream.write_all(&reply).await.unwrap();
            stream.flush().await.unwrap();

            if let Some(OrchdFrame::Request { id, req }) = read_frame(&mut stream).await {
                let res = respond(req);
                write_stub_frame(&mut stream, &OrchdFrame::Response { id, res }).await;
            }
            // Keep the stream (and tempdir) alive for the rest of the test.
            let _ = sock_path2;
            std::future::pending::<()>().await;
        });
        while !ready.load(Ordering::SeqCst) {
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }

        let client = {
            let _guard = ENV_TEST_LOCK.lock().await;
            unsafe {
                std::env::set_var("XDG_RUNTIME_DIR", dir.path());
            }
            let client = OrchdClient::connect("test-build".to_string())
                .await
                .unwrap();
            unsafe {
                std::env::remove_var("XDG_RUNTIME_DIR");
            }
            client
        };
        std::mem::forget(dir); // keep the tempdir (and its socket) alive for the test's duration

        (client, sock_path)
    }

    #[tokio::test]
    async fn orchd_create_project_round_trips_through_real_orchd_client() {
        let (client, _sock) = connect_orchd_to_stub(|req| match req {
            OrchdRequest::CreateProject {
                name,
                description,
                workspace_ids,
            } => OrchdResponse::Project(bpa_orchd_proto::Project {
                id: "proj-1".into(),
                name,
                description,
                status: bpa_orchd_proto::ProjectStatus::Active,
                workspace_ids,
                created_at: 0,
                updated_at: 0,
            }),
            other => panic!("expected CreateProject, got {other:?}"),
        })
        .await;

        let req = OrchdRequest::CreateProject {
            name: "Proj".into(),
            description: "Desc".into(),
            workspace_ids: vec!["ws-1".into()],
        };
        let res = client.request(req).await.unwrap();
        let project = expect_project(res).unwrap();
        assert_eq!(project.id, "proj-1");
        assert_eq!(project.name, "Proj");
        assert_eq!(project.workspace_ids, vec!["ws-1".to_string()]);
    }

    #[tokio::test]
    async fn orchd_invariant_error_response_becomes_command_error_daemon_invariant_end_to_end() {
        let (client, _sock) = connect_orchd_to_stub(|_req| OrchdResponse::Error {
            code: bpa_orchd_proto::OrchdErrorCode::Invariant,
            message: "a project must keep at least one workspace".into(),
        })
        .await;

        let res = client.request(OrchdRequest::ListProjects).await;
        // OrchdClientError::Daemon is raised directly by OrchdClient::request (mirrors
        // socket_client's own Response::Error handling) — confirm the CommandError `From` impl
        // reshapes it to `Daemon { code: "Invariant", .. }`, the spec §9-locked shape.
        let err: CommandError = res.unwrap_err().into();
        match err {
            CommandError::Daemon { code, message } => {
                assert_eq!(code, "Invariant");
                assert_eq!(message, "a project must keep at least one workspace");
            }
            other => panic!("expected CommandError::Daemon, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn orchd_reveal_rules_file_core_uses_the_get_rule_set_returned_path() {
        // Locks the security property spec §9 requires: the path handed to `opener::reveal` is
        // ALWAYS whatever GetRuleSet's own reply carried, never anything JS could substitute.
        // Asserted at this inner-fn boundary (never calling the real `opener::reveal`, which would
        // open a Finder window inside the test process).
        let (client, _sock) = connect_orchd_to_stub(|req| match req {
            OrchdRequest::GetRuleSet { scope, project_id } => {
                assert_eq!(scope, RuleScope::Global);
                assert_eq!(project_id, None);
                OrchdResponse::RuleSetView(bpa_orchd_proto::RuleSetView {
                    rule: bpa_orchd_proto::RuleSet {
                        id: "rs-1".into(),
                        scope: RuleScope::Global,
                        project_id: None,
                        md_path: "/app-support/global-rules.md".into(),
                        md_hash: "abc123".into(),
                        policy: bpa_orchd_proto::PolicyRules {
                            spend_cap_usd: None,
                            approval_classes: vec![],
                            path_allowlist: vec![],
                        },
                        created_at: 0,
                        updated_at: 0,
                    },
                    md_content: Some("# rules".into()),
                    file_state: bpa_orchd_proto::RuleFileState::Ok,
                })
            }
            other => panic!("expected GetRuleSet, got {other:?}"),
        })
        .await;

        let path = super::reveal_rules_file_core(&client, RuleScope::Global, None)
            .await
            .unwrap();
        assert_eq!(path, "/app-support/global-rules.md");
    }

    #[tokio::test]
    async fn orchd_export_to_file_all_export_writes_json_named_store() {
        let (client, _sock) = connect_orchd_to_stub(|req| match req {
            OrchdRequest::ExportAll => {
                OrchdResponse::ExportJson(r#"{"bundleFormat":1,"projects":[]}"#.into())
            }
            other => panic!("expected ExportAll, got {other:?}"),
        })
        .await;

        let json =
            expect_export_json(client.request(OrchdRequest::ExportAll).await.unwrap()).unwrap();
        let base = super::export_base_name(&json, false);
        assert_eq!(base, "store");

        let dest = tempfile::tempdir().unwrap();
        let path = super::write_export_file(dest.path().to_str().unwrap(), &base, &json).unwrap();
        assert_eq!(
            path.file_name().unwrap().to_str().unwrap(),
            "store-export.json"
        );
        let written = std::fs::read_to_string(&path).unwrap();
        assert_eq!(written, json);
    }

    #[tokio::test]
    async fn orchd_export_to_file_project_export_uses_sanitized_project_name() {
        let (client, _sock) = connect_orchd_to_stub(|req| match req {
            OrchdRequest::ExportProject { project_id } => {
                assert_eq!(project_id, "proj-1");
                OrchdResponse::ExportJson(
                    r#"{"bundleFormat":1,"project":{"id":"proj-1","name":"My Cool Project!"}}"#
                        .into(),
                )
            }
            other => panic!("expected ExportProject, got {other:?}"),
        })
        .await;

        let json = expect_export_json(
            client
                .request(OrchdRequest::ExportProject {
                    project_id: "proj-1".into(),
                })
                .await
                .unwrap(),
        )
        .unwrap();
        let base = super::export_base_name(&json, true);
        assert_eq!(base, "My-Cool-Project");

        let dest = tempfile::tempdir().unwrap();
        let path = super::write_export_file(dest.path().to_str().unwrap(), &base, &json).unwrap();
        assert_eq!(
            path.file_name().unwrap().to_str().unwrap(),
            "My-Cool-Project-export.json"
        );
    }

    #[tokio::test]
    async fn orchd_import_from_file_reads_file_and_round_trips_through_stub() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("bundle.json");
        let json_content = r#"{"bundleFormat":1,"projects":[]}"#;
        std::fs::write(&file_path, json_content).unwrap();

        let (client, _sock) = connect_orchd_to_stub(move |req| match req {
            OrchdRequest::ImportBundle { json } => {
                assert_eq!(json, json_content);
                OrchdResponse::ImportReport {
                    projects: 1,
                    goals: 2,
                    ideas: 3,
                    insights: 4,
                    tasks: 5,
                    rulesets: 6,
                }
            }
            other => panic!("expected ImportBundle, got {other:?}"),
        })
        .await;

        let read_json = super::read_import_file(file_path.to_str().unwrap()).unwrap();
        assert_eq!(read_json, json_content);
        let report = expect_import_report(
            client
                .request(OrchdRequest::ImportBundle { json: read_json })
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(
            report,
            ImportReport {
                projects: 1,
                goals: 2,
                ideas: 3,
                insights: 4,
                tasks: 5,
                rulesets: 6,
            }
        );
    }

    #[test]
    fn orchd_import_from_file_refuses_a_file_over_the_10_mib_cap() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("huge.json");
        let huge = vec![b'a'; (super::IMPORT_FILE_READ_CAP + 1) as usize];
        std::fs::write(&file_path, &huge).unwrap();

        let err = super::read_import_file(file_path.to_str().unwrap()).unwrap_err();
        match err {
            CommandError::Internal(msg) => {
                assert!(
                    msg.contains("exceeds"),
                    "expected an honest 'exceeds the cap' message, got: {msg}"
                );
            }
            other => panic!("expected CommandError::Internal, got {other:?}"),
        }
    }

    #[test]
    fn orchd_import_from_file_accepts_a_file_exactly_at_the_10_mib_cap() {
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("exact.json");
        let exact = vec![b'a'; super::IMPORT_FILE_READ_CAP as usize];
        std::fs::write(&file_path, &exact).unwrap();

        let read = super::read_import_file(file_path.to_str().unwrap()).unwrap();
        assert_eq!(read.len(), super::IMPORT_FILE_READ_CAP as usize);
    }

    // ── S4 knowledge-graph orchd_graph_* commands (spec §3, S4 T5) ─────────────────────────────
    //
    // Same rationale as `orchd_create_project_round_trips_through_real_orchd_client` /
    // `orchd_invariant_error_response_becomes_command_error_daemon_invariant_end_to_end` above: the
    // `#[tauri::command]` fns need a live `State<AppState>` (out of scope for a unit test), but each
    // one's entire body is `expect_*(client.request(req).await?)` — these drive that exact shape
    // directly against a real `OrchdClient` + stub daemon. `orchd_graph_add_node` stands in for all
    // nine (identical shape); the `Invariant` error mapping is proven once, generically, exactly as
    // the sessiond-domain test above proves it once via `ListProjects`.

    #[tokio::test]
    async fn orchd_graph_add_node_round_trips_through_real_orchd_client() {
        let (client, _sock) = connect_orchd_to_stub(|req| match req {
            OrchdRequest::GraphAddNode {
                project_id,
                kind,
                label,
                body,
                pos_x,
                pos_y,
            } => OrchdResponse::GraphNode(bpa_orchd_proto::GraphNode {
                id: "node-1".into(),
                project_id,
                kind,
                entity_type: None,
                entity_id: None,
                label,
                body,
                pos_x,
                pos_y,
                created_at: 0,
                updated_at: 0,
                is_orphan: false,
            }),
            other => panic!("expected GraphAddNode, got {other:?}"),
        })
        .await;

        let req = OrchdRequest::GraphAddNode {
            project_id: "proj-1".into(),
            kind: bpa_orchd_proto::GraphNodeKind::Concept,
            label: "Label".into(),
            body: "Body".into(),
            pos_x: 10.0,
            pos_y: 20.0,
        };
        let res = client.request(req).await.unwrap();
        let node = expect_graph_node(res).unwrap();
        assert_eq!(node.id, "node-1");
        assert_eq!(node.project_id, "proj-1");
        assert_eq!(node.label, "Label");
        assert_eq!(node.body, "Body");
        assert_eq!(node.pos_x, 10.0);
        assert_eq!(node.pos_y, 20.0);
        assert_eq!(node.kind, bpa_orchd_proto::GraphNodeKind::Concept);
    }

    #[tokio::test]
    async fn orchd_graph_add_node_invariant_error_response_becomes_command_error_daemon_invariant()
    {
        let (client, _sock) = connect_orchd_to_stub(|_req| OrchdResponse::Error {
            code: bpa_orchd_proto::OrchdErrorCode::Invariant,
            message: "node label must not be empty".into(),
        })
        .await;

        let res = client
            .request(OrchdRequest::GraphAddNode {
                project_id: "proj-1".into(),
                kind: bpa_orchd_proto::GraphNodeKind::Concept,
                label: String::new(),
                body: String::new(),
                pos_x: 0.0,
                pos_y: 0.0,
            })
            .await;
        // OrchdClientError::Daemon is raised directly by OrchdClient::request (mirrors the
        // sessiond-domain Invariant test above) — confirm the `From` impl reshapes it to
        // `Daemon { code: "Invariant", .. }`, the spec §9-locked shape, for the S4 graph verbs too.
        let err: CommandError = res.unwrap_err().into();
        match err {
            CommandError::Daemon { code, message } => {
                assert_eq!(code, "Invariant");
                assert_eq!(message, "node label must not be empty");
            }
            other => panic!("expected CommandError::Daemon, got {other:?}"),
        }
    }

    // ── S-EXT MCP orchd_mcp_*/trust_* commands (spec §5, S-EXT T7) ─────────────────────────────
    //
    // Same rationale as the S4 graph block above: `mcp_add_server` stands in for the happy-path
    // shape shared by all 15 (`expect_*(client.request(req).await?)`); the `Consent`/`Policy`
    // error-code mappings are proven once each, directly, since they are the two NEW
    // `OrchdErrorCode` variants this slice's daemon side (T5/T6) introduced specifically for the
    // trust choke-point (spec §6) and are not exercised by any existing sessiond/graph test.

    #[tokio::test]
    async fn mcp_add_server_round_trips_through_real_orchd_client() {
        let (client, _sock) = connect_orchd_to_stub(|req| match req {
            OrchdRequest::McpAddServer {
                name,
                transport,
                url,
                command,
                args,
                env,
                scope,
                project_id,
                auth_kind,
                timeout_ms,
                max_retries,
            } => OrchdResponse::McpServer(McpServer {
                id: "srv-1".into(),
                name,
                transport,
                url,
                command,
                args: args.unwrap_or_default(),
                env: env.unwrap_or_default(),
                scope,
                project_id,
                auth_kind,
                secret_ref: None,
                account_id: None,
                enabled: true,
                timeout_ms: timeout_ms.unwrap_or(30_000),
                max_retries: max_retries.unwrap_or(3),
                protocol_version: None,
                created_at: 0,
                updated_at: 0,
            }),
            other => panic!("expected McpAddServer, got {other:?}"),
        })
        .await;

        let req = OrchdRequest::McpAddServer {
            name: "prowl".into(),
            transport: McpTransport::Http,
            url: Some("https://prowl.chat/mcp".into()),
            command: None,
            args: None,
            env: None,
            scope: McpScope::Global,
            project_id: None,
            auth_kind: McpAuthKind::Bearer,
            timeout_ms: None,
            max_retries: None,
        };
        let res = client.request(req).await.unwrap();
        let server = expect_mcp_server(res).unwrap();
        assert_eq!(server.id, "srv-1");
        assert_eq!(server.name, "prowl");
        assert_eq!(server.url.as_deref(), Some("https://prowl.chat/mcp"));
        assert_eq!(server.transport, McpTransport::Http);
        assert_eq!(server.auth_kind, McpAuthKind::Bearer);
        assert!(server.enabled);
    }

    #[tokio::test]
    async fn mcp_connect_consent_error_response_becomes_command_error_daemon_consent() {
        let (client, _sock) = connect_orchd_to_stub(|_req| OrchdResponse::Error {
            code: bpa_orchd_proto::OrchdErrorCode::Consent,
            message: "no consent grant for this server's current url".into(),
        })
        .await;

        let res = client
            .request(OrchdRequest::McpConnect { id: "srv-1".into() })
            .await;
        // Same `From<OrchdClientError> for CommandError` path as the Invariant test above —
        // confirm the two S-EXT-only `OrchdErrorCode` variants (spec §5/§6 trust choke-point)
        // reshape into `CommandError::Daemon` identically to every pre-existing error code.
        let err: CommandError = res.unwrap_err().into();
        match err {
            CommandError::Daemon { code, message } => {
                assert_eq!(code, "Consent");
                assert_eq!(message, "no consent grant for this server's current url");
            }
            other => panic!("expected CommandError::Daemon, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn mcp_call_tool_policy_error_response_becomes_command_error_daemon_policy() {
        let (client, _sock) = connect_orchd_to_stub(|_req| OrchdResponse::Error {
            code: bpa_orchd_proto::OrchdErrorCode::Policy,
            message: "tool is disabled".into(),
        })
        .await;

        let res = client
            .request(OrchdRequest::McpCallTool {
                server_id: "srv-1".into(),
                tool_name: "echo".into(),
                args_json: "{}".into(),
                project_id: None,
            })
            .await;
        let err: CommandError = res.unwrap_err().into();
        match err {
            CommandError::Daemon { code, message } => {
                assert_eq!(code, "Policy");
                assert_eq!(message, "tool is disabled");
            }
            other => panic!("expected CommandError::Daemon, got {other:?}"),
        }
    }

    // ── S-EXT Connector orchd `connector_*` commands (spec §5/§7, S-EXT T13a) ──────────────────
    //
    // Same rationale as the MCP block above: `connector_add_api_key` stands in for the happy-path
    // shape shared by every `connector_*` verb (`expect_*(client.request(req).await?)`); the
    // `Policy` error-code mapping is proven once, directly, since `ConnectorInvoke`'s spend/rate-
    // cap denial (spec §6: "connector_invoke passes through trust::authorize IDENTICALLY to
    // McpCallTool") is the connector-side analogue of `mcp_call_tool_policy_error_...` above and
    // is not exercised by any existing MCP test.

    #[tokio::test]
    async fn connector_add_api_key_round_trips_through_real_orchd_client() {
        let (client, _sock) = connect_orchd_to_stub(|req| match req {
            OrchdRequest::ConnectorAddApiKey {
                provider,
                label,
                api_key,
            } => {
                assert_eq!(
                    api_key, "sk-live-test-do-not-leak-42",
                    "api_key must round-trip to the wire request unchanged"
                );
                OrchdResponse::Account(Account {
                    id: "acct-1".into(),
                    provider,
                    label,
                    auth_kind: bpa_orchd_proto::AccountAuthKind::Apikey,
                    scopes: vec![],
                    expires_at: None,
                    created_at: 0,
                    updated_at: 0,
                })
            }
            other => panic!("expected ConnectorAddApiKey, got {other:?}"),
        })
        .await;

        let req = OrchdRequest::ConnectorAddApiKey {
            provider: "generic-rest".into(),
            label: "My REST".into(),
            api_key: "sk-live-test-do-not-leak-42".into(),
        };
        let res = client.request(req).await.unwrap();
        let account = expect_account(res).unwrap();
        assert_eq!(account.id, "acct-1");
        assert_eq!(account.provider, "generic-rest");
        assert_eq!(account.label, "My REST");
        assert_eq!(account.auth_kind, bpa_orchd_proto::AccountAuthKind::Apikey);
    }

    #[tokio::test]
    async fn connector_invoke_policy_error_response_becomes_command_error_daemon_policy() {
        let (client, _sock) = connect_orchd_to_stub(|_req| OrchdResponse::Error {
            code: bpa_orchd_proto::OrchdErrorCode::Policy,
            message: "connector invoke denied: spend cap exceeded".into(),
        })
        .await;

        let res = client
            .request(OrchdRequest::ConnectorInvoke {
                account_id: "acct-1".into(),
                op: "get".into(),
                args_json: "{}".into(),
                project_id: None,
            })
            .await;
        let err: CommandError = res.unwrap_err().into();
        match err {
            CommandError::Daemon { code, message } => {
                assert_eq!(code, "Policy");
                assert_eq!(message, "connector invoke denied: spend cap exceeded");
            }
            other => panic!("expected CommandError::Daemon, got {other:?}"),
        }
    }

    // ── S-EXT Skills orchd `skill_*` commands (spec §5/§8, D11, Q14, task T17) ─────────────────
    //
    // Same rationale as the MCP/Connector blocks above: `skill_add` stands in for the happy-path
    // shape shared by all three verbs (`expect_*(client.request(req).await?)`); a `Validation`
    // error response (the one `add_skill` actually produces — e.g. no name available from either
    // an explicit override or the SKILL.md frontmatter) proves the generic `Error->Daemon`
    // mapping once, directly, mirroring `mcp_connect_consent_error_...`'s style above.

    #[tokio::test]
    async fn skill_add_round_trips_through_real_orchd_client() {
        let (client, _sock) = connect_orchd_to_stub(|req| match req {
            OrchdRequest::SkillAdd {
                name,
                description,
                md_path,
                scope,
                project_id,
            } => OrchdResponse::Skill(Skill {
                id: "skill-1".into(),
                name: name.unwrap_or_else(|| "Parsed From Frontmatter".into()),
                description: description.unwrap_or_default(),
                md_path,
                md_hash: "deadbeef".into(),
                scope,
                project_id,
                file_state: bpa_orchd_proto::SkillFileState::Present,
                created_at: 0,
                updated_at: 0,
            }),
            other => panic!("expected SkillAdd, got {other:?}"),
        })
        .await;

        let req = OrchdRequest::SkillAdd {
            name: Some("My Skill".into()),
            description: Some("does things".into()),
            md_path: "/tmp/skills/demo/SKILL.md".into(),
            scope: SkillScope::Global,
            project_id: None,
        };
        let res = client.request(req).await.unwrap();
        let skill = expect_skill(res).unwrap();
        assert_eq!(skill.id, "skill-1");
        assert_eq!(skill.name, "My Skill");
        assert_eq!(skill.scope, SkillScope::Global);
        assert_eq!(skill.file_state, bpa_orchd_proto::SkillFileState::Present);
    }

    #[tokio::test]
    async fn skill_add_validation_error_response_becomes_command_error_daemon_validation() {
        let (client, _sock) = connect_orchd_to_stub(|_req| OrchdResponse::Error {
            code: bpa_orchd_proto::OrchdErrorCode::Validation,
            message: "skill: name required (pass it explicitly or via the SKILL.md frontmatter)"
                .into(),
        })
        .await;

        let res = client
            .request(OrchdRequest::SkillAdd {
                name: None,
                description: None,
                md_path: "/tmp/skills/demo/SKILL.md".into(),
                scope: SkillScope::Global,
                project_id: None,
            })
            .await;
        let err: CommandError = res.unwrap_err().into();
        match err {
            CommandError::Daemon { code, message } => {
                assert_eq!(code, "Validation");
                assert_eq!(
                    message,
                    "skill: name required (pass it explicitly or via the SKILL.md frontmatter)"
                );
            }
            other => panic!("expected CommandError::Daemon, got {other:?}"),
        }
    }

    // ── S-IDEA research (spec §5/§6, task T5) — mirrors the `CreateProject`/`SkillAdd` round-trip
    // + error-mapping pair above exactly, over `ResearchStartRun`. ─────────────────────────────

    #[tokio::test]
    async fn research_start_run_round_trips_through_real_orchd_client() {
        let (client, _sock) = connect_orchd_to_stub(|req| match req {
            OrchdRequest::ResearchStartRun {
                idea_id,
                server_id,
                tool_name,
                args_json,
            } => OrchdResponse::ResearchRun(bpa_orchd_proto::ResearchRun {
                id: "run-1".into(),
                idea_id,
                server_id,
                tool_name,
                args_json,
                status: bpa_orchd_proto::ResearchStatus::Pending,
                invocation_id: None,
                artifact_id: None,
                error_kind: None,
                created_at: 0,
                updated_at: 0,
            }),
            other => panic!("expected ResearchStartRun, got {other:?}"),
        })
        .await;

        let req = OrchdRequest::ResearchStartRun {
            idea_id: "idea-1".into(),
            server_id: "server-1".into(),
            tool_name: "echo".into(),
            args_json: "{}".into(),
        };
        let res = client.request(req).await.unwrap();
        let run = expect_research_run(res).unwrap();
        assert_eq!(run.id, "run-1");
        assert_eq!(run.idea_id, "idea-1");
        assert_eq!(run.server_id, "server-1");
        assert_eq!(run.tool_name, "echo");
        assert_eq!(run.status, bpa_orchd_proto::ResearchStatus::Pending);
        assert!(run.invocation_id.is_none());
        assert!(run.artifact_id.is_none());
    }

    #[tokio::test]
    async fn research_start_run_error_response_becomes_command_error_daemon_end_to_end() {
        let (client, _sock) = connect_orchd_to_stub(|_req| OrchdResponse::Error {
            code: bpa_orchd_proto::OrchdErrorCode::NotFound,
            message: "not found".into(),
        })
        .await;

        let res = client
            .request(OrchdRequest::ResearchStartRun {
                idea_id: "no-such-idea".into(),
                server_id: "server-1".into(),
                tool_name: "echo".into(),
                args_json: "{}".into(),
            })
            .await;
        // Mirrors `orchd_invariant_error_response_becomes_command_error_daemon_invariant_end_to_end`
        // above: `OrchdClientError::Daemon` is raised directly by `OrchdClient::request`, so `?` in
        // `research_start_run`'s own body would surface it via the `From<OrchdClientError>` impl —
        // confirm that reshape lands on the spec §9-locked `Daemon{code, message}` shape.
        let err: CommandError = res.unwrap_err().into();
        match err {
            CommandError::Daemon { code, message } => {
                assert_eq!(code, "NotFound");
                assert_eq!(message, "not found");
            }
            other => panic!("expected CommandError::Daemon, got {other:?}"),
        }
    }

    // ── Storage-degradation mode (spec D3, BL-94) — same round-trip + error-mapping pair as the
    // research commands above, over `GetStorageStatus`. ────────────────────────────────────────

    #[tokio::test]
    async fn orchd_storage_status_round_trips_through_real_orchd_client() {
        let (client, _sock) = connect_orchd_to_stub(|req| match req {
            OrchdRequest::GetStorageStatus => {
                OrchdResponse::StorageStatus(bpa_orchd_proto::StorageStatus {
                    storage_mode: bpa_orchd_proto::StorageMode::RecoveredFromCorruption,
                    quarantined_path: Some("/x/orchd.db.corrupt-1".into()),
                })
            }
            other => panic!("expected GetStorageStatus, got {other:?}"),
        })
        .await;

        let res = client
            .request(OrchdRequest::GetStorageStatus)
            .await
            .unwrap();
        let status = expect_storage_status(res).unwrap();
        assert_eq!(
            status.storage_mode,
            bpa_orchd_proto::StorageMode::RecoveredFromCorruption
        );
        assert_eq!(
            status.quarantined_path.as_deref(),
            Some("/x/orchd.db.corrupt-1")
        );
    }

    #[tokio::test]
    async fn orchd_storage_status_error_response_becomes_command_error_daemon_end_to_end() {
        let (client, _sock) = connect_orchd_to_stub(|_req| OrchdResponse::Error {
            code: bpa_orchd_proto::OrchdErrorCode::Io,
            message: "io".into(),
        })
        .await;

        let res = client.request(OrchdRequest::GetStorageStatus).await;
        let err: CommandError = res.unwrap_err().into();
        match err {
            CommandError::Daemon { code, message } => {
                assert_eq!(code, "Io");
                assert_eq!(message, "io");
            }
            other => panic!("expected CommandError::Daemon, got {other:?}"),
        }
    }
}
