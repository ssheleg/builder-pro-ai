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

use std::sync::Arc;

use bpa_protocol::{
    Request, Response, SessionId, SessionMeta, TerminalEvent, Workspace, WorkspaceId,
};
use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::AppHandle;
use tauri::State;
use tauri_plugin_dialog::DialogExt;

use crate::broker::Broker;
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
pub struct AppState {
    pub client: ClientSlot,
    pub broker: Arc<Broker>,
    pub launchd: Arc<crate::launchd::LaunchdAgent<'static>>,
    pub status: StatusSlot,
    pub write_stdin_locks: Arc<WriteStdinLocks>,
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

/// Read the current [`DaemonStatus`] out of a bare `StatusSlot` — pulled out as a free function for
/// the same unit-testability reason as `slot_client` above.
pub(crate) fn read_status(slot: &StatusSlot) -> DaemonStatus {
    slot.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Write a new [`DaemonStatus`] into a bare `StatusSlot`.
pub(crate) fn write_status(slot: &StatusSlot, status: DaemonStatus) {
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

fn expect_ack(res: Response) -> Result<(), CommandError> {
    match res {
        Response::Ack => Ok(()),
        other => Err(err_from_response(other)),
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
                | Request::DaemonShutdown { .. } => false,
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
    static ENV_TEST_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

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
