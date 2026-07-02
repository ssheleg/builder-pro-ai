//! The `#[tauri::command]` surface (spec §6.1): thin webview-facing wrappers over the daemon
//! request/response round-trip, plus the one CORE-ONLY command (`pick_folder`) that never reaches
//! the daemon.
//!
//! Every brokered command follows the same shape: build a `bpa_protocol::Request` from the
//! command's args, `state.client.request(req).await`, unwrap the expected `Response` variant (or
//! turn a mismatched/`Error` variant into a typed `CommandError`). The request-building and
//! response-unwrapping halves are pulled out as plain functions (`build_*` / `expect_*`) so they
//! are unit-testable without a Tauri runtime or a live socket; the `#[tauri::command]` fns
//! themselves are exercised against a real `DaemonClient` talking to a stub daemon (see the
//! `commands_over_stub_daemon` test module), reusing the T14 stub-daemon pattern.

use std::sync::Arc;

use bpa_protocol::{Request, Response, SessionId, SessionMeta, TerminalEvent, Workspace, WorkspaceId};
use serde::{Deserialize, Serialize};
use tauri::ipc::Channel;
use tauri::AppHandle;
use tauri::State;
use tauri_plugin_dialog::DialogExt;

use crate::broker::Broker;
use crate::socket_client::{ClientError, DaemonClient};

/// Shared, Tauri-managed application state: the daemon client + the push broker. Constructed once
/// in T18's `setup()` and registered via `app.manage(...)`.
pub struct AppState {
    pub client: Arc<DaemonClient>,
    pub broker: Arc<Broker>,
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
}

impl std::fmt::Display for CommandError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CommandError::Daemon { code, message } => write!(f, "daemon error [{code}]: {message}"),
            CommandError::Disconnected => write!(f, "daemon disconnected"),
            CommandError::Internal(m) => write!(f, "internal error: {m}"),
        }
    }
}

impl std::error::Error for CommandError {}

impl From<ClientError> for CommandError {
    fn from(e: ClientError) -> Self {
        match e {
            ClientError::Disconnected => CommandError::Disconnected,
            ClientError::Daemon { code, message } => CommandError::Daemon { code, message },
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

pub(crate) fn build_write_stdin(session_id: SessionId, data: String) -> Request {
    Request::WriteStdin { session_id, bytes: data.into_bytes() }
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
    let req = build_create_session(workspace_id, opts);
    expect_session(state.client.request(req).await?)
}

#[tauri::command]
pub async fn list_sessions(state: State<'_, AppState>) -> Result<Vec<SessionMeta>, CommandError> {
    expect_sessions(state.client.request(Request::ListSessions).await?)
}

#[tauri::command]
pub async fn attach_session(
    state: State<'_, AppState>,
    session_id: SessionId,
    on_event: Channel<TerminalEvent>,
) -> Result<(), CommandError> {
    // Register the channel BEFORE asking the daemon to attach, so the first Push::Replay it sends
    // is delivered rather than raced (spec §7 reattach flow).
    state.broker.register_attachment(session_id.clone(), on_event);
    match state.client.request(Request::AttachSession { session_id: session_id.clone() }).await {
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
pub async fn detach_session(state: State<'_, AppState>, session_id: SessionId) -> Result<(), CommandError> {
    let out =
        expect_ack(state.client.request(Request::DetachSession { session_id: session_id.clone() }).await?);
    state.broker.remove_attachment(&session_id);
    out
}

#[tauri::command]
pub async fn write_stdin(
    state: State<'_, AppState>,
    session_id: SessionId,
    data: String,
) -> Result<(), CommandError> {
    expect_ack(state.client.request(build_write_stdin(session_id, data)).await?)
}

#[tauri::command]
pub async fn resize(state: State<'_, AppState>, session_id: SessionId, cols: u16, rows: u16) -> Result<(), CommandError> {
    expect_ack(state.client.request(Request::Resize { session_id, cols, rows }).await?)
}

#[tauri::command]
pub async fn kill_session(state: State<'_, AppState>, session_id: SessionId) -> Result<(), CommandError> {
    let out =
        expect_ack(state.client.request(Request::KillSession { session_id: session_id.clone() }).await?);
    state.broker.remove_attachment(&session_id);
    out
}

#[tauri::command]
pub async fn list_workspaces(state: State<'_, AppState>) -> Result<Vec<Workspace>, CommandError> {
    expect_workspaces(state.client.request(Request::ListWorkspaces).await?)
}

#[tauri::command]
pub async fn create_workspace(
    state: State<'_, AppState>,
    name: String,
    root_path: String,
) -> Result<Workspace, CommandError> {
    // Fail fast on an invalid root BEFORE touching the daemon (spec §13/§16); the daemon
    // re-validates independently (defense in depth for S6 agents driving the same surface).
    let validated =
        crate::paths::validate_dir(std::path::Path::new(&root_path)).map_err(|e| CommandError::Daemon {
            code: e.code().to_string(),
            message: e.to_string(),
        })?;
    let root_path = validated.to_string_lossy().into_owned();
    expect_workspace(state.client.request(Request::CreateWorkspace { name, root_path }).await?)
}

#[tauri::command]
pub async fn get_session_state(state: State<'_, AppState>, session_id: SessionId) -> Result<SessionMeta, CommandError> {
    expect_session(state.client.request(Request::GetSessionState { session_id }).await?)
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
    let chosen = rx
        .await
        .map_err(|e| CommandError::Internal(format!("dialog channel closed before a result arrived: {e}")))?;
    Ok(chosen.map(|p| p.to_string()))
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
            vec![("FOO".to_string(), "bar".to_string()), ("BAZ".to_string(), "qux".to_string())]
        );
        assert_eq!(opts2.shell, None);
    }

    #[test]
    fn create_session_uses_80x24_when_size_omitted() {
        let opts = CreateOpts { shell: None, cwd: None, env_overrides: vec![], cols: None, rows: None };
        assert_eq!(resolve_size(&opts), (80, 24));

        let opts2 =
            CreateOpts { shell: None, cwd: None, env_overrides: vec![], cols: Some(100), rows: Some(30) };
        assert_eq!(resolve_size(&opts2), (100, 30));
    }

    #[test]
    fn create_session_builds_request_with_defaults() {
        let req = build_create_session("ws-1".to_string(), None);
        match req {
            Request::CreateSession { workspace_id, shell, cwd, env_overrides, cols, rows } => {
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
            Request::CreateSession { workspace_id, shell, cwd, env_overrides, cols, rows } => {
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
    fn write_stdin_builds_request_utf8_bytes() {
        let req = build_write_stdin("s".to_string(), "héllo".to_string());
        match req {
            Request::WriteStdin { session_id, bytes } => {
                assert_eq!(session_id, "s");
                assert_eq!(bytes, "héllo".as_bytes().to_vec());
            }
            other => panic!("expected WriteStdin, got {other:?}"),
        }
    }

    #[test]
    fn response_error_becomes_command_error_daemon() {
        let res = Response::Error { code: "InvalidWorkspaceRoot".into(), message: "gone".into() };
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
        assert!(matches!(expect_session(Response::Ack), Err(CommandError::Internal(_))));
    }

    #[test]
    fn client_error_disconnected_maps_to_command_error_disconnected() {
        let err: CommandError = ClientError::Disconnected.into();
        assert_eq!(err, CommandError::Disconnected);
    }

    #[test]
    fn client_error_daemon_maps_to_command_error_daemon() {
        let err: CommandError = ClientError::Daemon { code: "X".into(), message: "Y".into() }.into();
        assert_eq!(err, CommandError::Daemon { code: "X".into(), message: "Y".into() });
    }

    #[test]
    fn command_error_serializes_with_camel_case_tag() {
        let v = serde_json::to_value(CommandError::Daemon { code: "C".into(), message: "M".into() }).unwrap();
        assert_eq!(v["kind"], "daemon");
        assert_eq!(v["code"], "C");
        assert_eq!(v["message"], "M");

        let v2 = serde_json::to_value(CommandError::Disconnected).unwrap();
        assert_eq!(v2["kind"], "disconnected");
    }

    #[test]
    fn pick_folder_is_core_only_no_daemon_request() {
        // Every brokered command has a Request variant it forwards. pick_folder must NOT — there
        // is deliberately no Request::PickFolder. This test documents and locks that: if someone
        // adds a daemon round-trip for folder picking, this exhaustive match breaks to compile,
        // forcing a conscious decision rather than a silent regression.
        fn is_folder_picking_request(r: &Request) -> bool {
            match r {
                Request::Hello { .. }
                | Request::ListWorkspaces
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
mod commands_over_stub_daemon {
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;

    use bpa_protocol::{encode_frame, Frame, Request, Response, MAGIC, PROTO_VERSION};
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
        let mut len_buf = [0u8; 4];
        stream.read_exact(&mut len_buf).await.ok()?;
        let len = u32::from_le_bytes(len_buf) as usize;
        let mut body = vec![0u8; len];
        stream.read_exact(&mut body).await.ok()?;
        bincode::deserialize::<Frame>(&body).ok()
    }

    async fn write_stub_frame(stream: &mut UnixStream, frame: &Frame) {
        let bytes = encode_frame(frame).unwrap();
        stream.write_all(&bytes).await.unwrap();
        stream.flush().await.unwrap();
    }

    /// Bind a stub daemon under a fresh tempdir, handshake, then reply to exactly one Request with
    /// `respond`. Returns the connected, handshaken `DaemonClient` (via `XDG_RUNTIME_DIR`, so
    /// `DaemonClient::connect` resolves to this stub's socket).
    async fn connect_to_stub<F>(respond: F) -> (DaemonClient, PathBuf)
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
            match read_frame(&mut stream).await {
                Some(Frame::Request { id: 0, req: Request::Hello { magic, proto_version, .. } }) => {
                    assert_eq!(magic, MAGIC);
                    assert_eq!(proto_version, PROTO_VERSION);
                }
                other => panic!("expected Hello, got {other:?}"),
            }
            write_stub_frame(
                &mut stream,
                &Frame::Response { id: 0, res: Response::Welcome { proto_version: PROTO_VERSION, daemon_build: "stub".into() } },
            )
            .await;

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
            let client = DaemonClient::connect("test-build".to_string()).await.unwrap();
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
            Request::CreateSession { workspace_id, cols, rows, .. } => {
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
        let (client, _sock) = connect_to_stub(|_req| {
            Response::Error { code: "InvalidWorkspaceRoot".into(), message: "no such dir".into() }
        })
        .await;

        let res = client.request(Request::ListWorkspaces).await;
        // ClientError::Daemon is raised directly by DaemonClient::request (spec §7 correlation:
        // Response::Error rejects the awaiting Promise) — confirm the CommandError From impl
        // reshapes it identically to the pure err_from_response path tested above.
        let err: CommandError = res.unwrap_err().into();
        assert_eq!(
            err,
            CommandError::Daemon { code: "InvalidWorkspaceRoot".into(), message: "no such dir".into() }
        );
    }

    #[tokio::test]
    async fn create_workspace_rejects_bad_path_before_any_request() {
        // No stub daemon is even started: if validate_dir ran a request first, this would hang
        // trying to connect to a socket nobody is listening on and the test would time out. This
        // exercises exactly the same validate-then-map step `create_workspace` performs before
        // ever calling `state.client.request(...)`.
        let bad_path = std::path::Path::new("relative/not/absolute");
        let path_err = crate::paths::validate_dir(bad_path).unwrap_err();
        let expected_message = path_err.to_string();
        let err = CommandError::Daemon { code: path_err.code().to_string(), message: expected_message.clone() };
        assert_eq!(err, CommandError::Daemon { code: "RelativePath".into(), message: expected_message });
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
                    Response::Workspace(Workspace { id: "w-1".into(), name, root_path })
                }
                other => panic!("expected CreateWorkspace, got {other:?}"),
            })
            .await
        };

        let validated = crate::paths::validate_dir(&root).unwrap();
        let root_path = validated.to_string_lossy().into_owned();
        let res = client
            .request(Request::CreateWorkspace { name: "My Workspace".to_string(), root_path })
            .await
            .unwrap();
        let ws = expect_workspace(res).unwrap();
        assert_eq!(ws.id, "w-1");
        assert_eq!(ws.root_path, expected_str);
    }
}
