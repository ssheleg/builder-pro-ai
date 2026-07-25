//! Socket-level tests for the T10 domain dispatch table (spec §4.2, §6, §7): boot the real daemon
//! `run()` on a temp socket under an isolated `$HOME` (mirrors `boot_integration.rs`'s pattern —
//! `app_support_dir()` reads `$HOME` at every request, not just at boot, since `ImportBundle`/
//! `CreateProject`'s post-commit ruleset write both resolve it fresh per-request), drive real
//! `OrchdRequest`s over the wire via two connections, and assert both halves of spec §6's
//! contract: a successful mutating verb replies the updated entity AND every OTHER connected
//! client observes the matching coarse push; a FAILED verb broadcasts nothing.

use std::path::Path;
use std::time::{Duration, Instant};

use bpa_orchd::protocol::{
    encode_orchd_frame, Account, AccountAuthKind, AuditRow, ConnectorOp, ContextScope, DocMeta,
    DocView, DomainTask, Gate, Goal, GoalKind, GraphEdge, GraphEdgeKind, GraphNeighborhood,
    GraphNode, GraphNodeKind, GraphView, Idea, McpArtifact, McpAuthKind, McpCallResult,
    McpConnectReport, McpScope, McpServer, McpTool, McpTransport, OAuthChallenge, OrchdErrorCode,
    OrchdFrame, OrchdFrameDecoder, OrchdPush, OrchdRequest, OrchdResponse, Policy, PolicyScope,
    Project, ProjectStatus, ResearchRun, ResearchStatus, RuleFileState, RuleScope, RuleSetView,
    Skill, SkillFileState, SkillScope, Stage, SupervisorConfig, TaskPriority, TaskSource, Workflow,
    WorkflowScope, ORCHD_CLIENT_MAX_VERSION, ORCHD_CLIENT_MIN_VERSION, ORCHD_DAEMON_MAX_VERSION,
};
use bpa_protocol::{decode_daemon_reply, encode_client_preamble, ClientPreamble, DaemonReply};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::sync::watch;

// ---- HOME isolation (mirrors `boot_integration.rs`'s `HOME_LOCK`/`HomeGuard` byte-for-byte —
// `app_support_dir()` reads process-global `$HOME`, so every test in this file that boots the
// real daemon core must isolate it under its own tempdir and serialize with every other test
// doing the same). ----
static HOME_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct HomeGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    prior: Option<std::ffi::OsString>,
}

impl HomeGuard {
    fn set(dir: &Path) -> Self {
        let lock = HOME_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let prior = std::env::var_os("HOME");
        // Symlink `Library/Keychains` from the REAL `$HOME` into this test's isolated `dir`
        // (S-EXT task T13a discovery, connector dispatch tests): `bpa_secrets`
        // (`security_framework::passwords::*`) resolves the default macOS Keychain via a
        // `$HOME`-derived path. Under a bare synthetic `$HOME` pointing at a fresh, otherwise-
        // empty tempdir, Security.framework finds no keychain at `<dir>/Library/Keychains` and
        // attempts to provision a brand-new one — which requires an interactive "choose a
        // password" prompt that blocks forever in this harness's non-interactive test run
        // (confirmed empirically: a bare fake `$HOME` hangs `bpa_secrets::set` indefinitely; this
        // symlink alone makes the identical call return in well under a second). Harmless for
        // every OTHER test in this file that never touches Keychain — `app_support_dir()`'s own
        // `Library/Application Support` subtree stays a REAL, freshly isolated directory under
        // `dir`; only `Library/Keychains` is shared with the real environment, and only as a
        // symlink to the SAME keychain this test process could already read/write directly (this
        // grants no new access — it only lets the OS's own default-keychain path resolution find
        // the keychain that's already there).
        if let Some(real_home) = &prior {
            let keychains_link = dir.join("Library/Keychains");
            if let Some(parent) = keychains_link.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::os::unix::fs::symlink(
                Path::new(real_home).join("Library/Keychains"),
                &keychains_link,
            );
        }
        std::env::set_var("HOME", dir);
        HomeGuard { _lock: lock, prior }
    }
}

impl Drop for HomeGuard {
    fn drop(&mut self) {
        match self.prior.take() {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
    }
}

async fn send_frame(s: &mut UnixStream, f: &OrchdFrame) {
    let bytes = encode_orchd_frame(f).unwrap();
    s.write_all(&bytes).await.unwrap();
    s.flush().await.unwrap();
}

async fn recv_frame(s: &mut UnixStream) -> OrchdFrame {
    let mut lenb = [0u8; 4];
    s.read_exact(&mut lenb).await.unwrap();
    let len = u32::from_le_bytes(lenb) as usize;
    let mut body = vec![0u8; len];
    s.read_exact(&mut body).await.unwrap();
    let mut decoder = OrchdFrameDecoder::new();
    decoder.push(&lenb);
    decoder.push(&body);
    let mut frames = decoder.decode().unwrap();
    frames.remove(0)
}

async fn preamble_handshake(s: &mut UnixStream) -> u16 {
    let bytes = encode_client_preamble(&ClientPreamble {
        min: ORCHD_CLIENT_MIN_VERSION,
        max: ORCHD_CLIENT_MAX_VERSION,
        build: "test".into(),
    });
    s.write_all(&bytes).await.unwrap();
    s.flush().await.unwrap();

    let mut header = [0u8; 9];
    s.read_exact(&mut header).await.unwrap();
    let mut buf = header.to_vec();
    if header[4] == 1 {
        let build_len = u16::from_le_bytes(header[7..9].try_into().unwrap()) as usize;
        let mut build = vec![0u8; build_len];
        s.read_exact(&mut build).await.unwrap();
        buf.extend_from_slice(&build);
    }
    match decode_daemon_reply(&buf).expect("valid daemon reply") {
        DaemonReply::Accepted { chosen, .. } => chosen,
        other => panic!("expected Accepted, got {other:?}"),
    }
}

async fn connect_when_ready(socket: &Path) -> UnixStream {
    for _ in 0..100 {
        if socket.exists() {
            if let Ok(c) = UnixStream::connect(socket).await {
                return c;
            }
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("daemon did not bind socket in time");
}

/// One handshaked client connection with request/id correlation + push draining helpers.
struct Client {
    stream: UnixStream,
    next_id: u64,
}

impl Client {
    async fn connect(socket: &Path) -> Self {
        let mut stream = connect_when_ready(socket).await;
        let chosen = preamble_handshake(&mut stream).await;
        assert_eq!(
            chosen, ORCHD_DAEMON_MAX_VERSION,
            "a client speaking exactly the daemon's own range must be offered the daemon's max"
        );
        Client { stream, next_id: 1 }
    }

    /// The requester's own connection is ALSO a registered broadcast target, and the dispatch
    /// loop enqueues the push (if any) into every registered client's queue — including this
    /// one's — strictly BEFORE it enqueues the correlated `Response` (mirrors sessiond's own
    /// `Frame::Push(_) => continue` convention in `boot_integration.rs`, e.g. its
    /// `rehydrate_attach`-style loops): so a mutating request's OWN connection sees its own push
    /// first, then the response. Skip any interleaved `Push` while waiting for the matching id.
    async fn request(&mut self, req: OrchdRequest) -> OrchdResponse {
        let id = self.next_id;
        self.next_id += 1;
        send_frame(&mut self.stream, &OrchdFrame::Request { id, req }).await;
        loop {
            match recv_frame(&mut self.stream).await {
                OrchdFrame::Response { id: rid, res } => {
                    assert_eq!(rid, id, "response id must correlate with the request id");
                    return res;
                }
                OrchdFrame::Push(_) => continue,
                other => panic!("expected a Response or Push frame, got {other:?}"),
            }
        }
    }

    /// Sends a request WITHOUT awaiting its response, returning the correlation id — lets a test
    /// keep a slow request in-flight on this connection while doing other work on ANOTHER
    /// connection, then collect the response later via [`Client::recv_response`]. Used by the
    /// concurrency regression test (T6 review fix) to prove a slow `McpCallTool` doesn't block
    /// other connections' DB ops.
    async fn send_request(&mut self, req: OrchdRequest) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        send_frame(&mut self.stream, &OrchdFrame::Request { id, req }).await;
        id
    }

    /// Awaits the correlated `Response` for a previously [`Client::send_request`]-ed `id`,
    /// skipping any interleaved `Push` (same correlation discipline as [`Client::request`]).
    async fn recv_response(&mut self, id: u64) -> OrchdResponse {
        loop {
            match recv_frame(&mut self.stream).await {
                OrchdFrame::Response { id: rid, res } => {
                    assert_eq!(rid, id, "response id must correlate with the request id");
                    return res;
                }
                OrchdFrame::Push(_) => continue,
                other => panic!("expected a Response or Push frame, got {other:?}"),
            }
        }
    }

    /// Blocks until a push arrives (panics on anything else, including a timeout — callers that
    /// expect NO push use [`Client::recv_push_timeout`] instead).
    async fn recv_push(&mut self) -> OrchdPush {
        match recv_frame(&mut self.stream).await {
            OrchdFrame::Push(p) => p,
            other => panic!("expected a Push frame, got {other:?}"),
        }
    }

    /// Waits up to `dur` for a push; `None` means the bounded wait elapsed with nothing received
    /// (the "no push was broadcast" assertion a failed mutating verb needs — spec §6: "Failed
    /// requests broadcast NOTHING").
    async fn recv_push_timeout(&mut self, dur: Duration) -> Option<OrchdPush> {
        match tokio::time::timeout(dur, recv_frame(&mut self.stream)).await {
            Ok(OrchdFrame::Push(p)) => Some(p),
            Ok(other) => panic!("expected a Push frame or a timeout, got {other:?}"),
            Err(_) => None,
        }
    }

    /// Sends a real `OrchdShutdown{drain:false}` and waits for both the `Ack` and `run()`'s
    /// return, so every test leaves its daemon task cleanly stopped rather than relying on the
    /// test's tokio runtime to abort it on drop.
    async fn shutdown(&mut self, boot: tokio::task::JoinHandle<std::io::Result<()>>) {
        let res = self
            .request(OrchdRequest::OrchdShutdown { drain: false })
            .await;
        assert_eq!(res, OrchdResponse::Ack);
        let result = tokio::time::timeout(Duration::from_secs(5), boot)
            .await
            .expect("daemon did not shut down in time")
            .expect("join");
        assert!(result.is_ok(), "run() returned an error: {result:?}");
    }
}

fn expect_project(res: OrchdResponse) -> Project {
    match res {
        OrchdResponse::Project(p) => p,
        other => panic!("expected Project, got {other:?}"),
    }
}

fn expect_goal(res: OrchdResponse) -> Goal {
    match res {
        OrchdResponse::Goal(g) => g,
        other => panic!("expected Goal, got {other:?}"),
    }
}

fn expect_goals(res: OrchdResponse) -> Vec<Goal> {
    match res {
        OrchdResponse::Goals(v) => v,
        other => panic!("expected Goals, got {other:?}"),
    }
}

fn expect_idea(res: OrchdResponse) -> Idea {
    match res {
        OrchdResponse::Idea(i) => i,
        other => panic!("expected Idea, got {other:?}"),
    }
}

fn expect_task(res: OrchdResponse) -> DomainTask {
    match res {
        OrchdResponse::Task(t) => t,
        other => panic!("expected Task, got {other:?}"),
    }
}

fn expect_ruleset_view(res: OrchdResponse) -> RuleSetView {
    match res {
        OrchdResponse::RuleSetView(v) => v,
        other => panic!("expected RuleSetView, got {other:?}"),
    }
}

fn expect_doc_view(res: OrchdResponse) -> DocView {
    match res {
        OrchdResponse::DocView(v) => v,
        other => panic!("expected DocView, got {other:?}"),
    }
}

fn expect_docs(res: OrchdResponse) -> Vec<DocMeta> {
    match res {
        OrchdResponse::Docs(v) => v,
        other => panic!("expected Docs, got {other:?}"),
    }
}

fn expect_ack(res: OrchdResponse) {
    match res {
        OrchdResponse::Ack => {}
        other => panic!("expected Ack, got {other:?}"),
    }
}

fn expect_workflow(res: OrchdResponse) -> Workflow {
    match res {
        OrchdResponse::Workflow(w) => w,
        other => panic!("expected Workflow, got {other:?}"),
    }
}

fn expect_workflows(res: OrchdResponse) -> Vec<Workflow> {
    match res {
        OrchdResponse::Workflows(v) => v,
        other => panic!("expected Workflows, got {other:?}"),
    }
}

fn expect_graph_node(res: OrchdResponse) -> GraphNode {
    match res {
        OrchdResponse::GraphNode(n) => n,
        other => panic!("expected GraphNode, got {other:?}"),
    }
}

fn expect_graph_edge(res: OrchdResponse) -> GraphEdge {
    match res {
        OrchdResponse::GraphEdge(e) => e,
        other => panic!("expected GraphEdge, got {other:?}"),
    }
}

fn expect_graph_view(res: OrchdResponse) -> GraphView {
    match res {
        OrchdResponse::GraphView(v) => v,
        other => panic!("expected GraphView, got {other:?}"),
    }
}

fn expect_neighborhood(res: OrchdResponse) -> GraphNeighborhood {
    match res {
        OrchdResponse::Neighborhood(n) => n,
        other => panic!("expected Neighborhood, got {other:?}"),
    }
}

fn expect_graph_nodes(res: OrchdResponse) -> Vec<GraphNode> {
    match res {
        OrchdResponse::GraphNodes(v) => v,
        other => panic!("expected GraphNodes, got {other:?}"),
    }
}

// ---- S-EXT MCP dispatch response helpers (task T6) ----

fn expect_mcp_server(res: OrchdResponse) -> McpServer {
    match res {
        OrchdResponse::McpServer(s) => s,
        other => panic!("expected McpServer, got {other:?}"),
    }
}

fn expect_mcp_tools(res: OrchdResponse) -> Vec<McpTool> {
    match res {
        OrchdResponse::McpTools(v) => v,
        other => panic!("expected McpTools, got {other:?}"),
    }
}

fn expect_mcp_tool(res: OrchdResponse) -> McpTool {
    match res {
        OrchdResponse::McpTool(t) => t,
        other => panic!("expected McpTool, got {other:?}"),
    }
}

fn expect_mcp_connect_report(res: OrchdResponse) -> McpConnectReport {
    match res {
        OrchdResponse::McpConnectReport(r) => r,
        other => panic!("expected McpConnectReport, got {other:?}"),
    }
}

fn expect_mcp_call_result(res: OrchdResponse) -> McpCallResult {
    match res {
        OrchdResponse::McpCallResult(r) => r,
        other => panic!("expected McpCallResult, got {other:?}"),
    }
}

fn expect_mcp_artifacts(res: OrchdResponse) -> Vec<McpArtifact> {
    match res {
        OrchdResponse::McpArtifacts(v) => v,
        other => panic!("expected McpArtifacts, got {other:?}"),
    }
}

fn expect_error_code(res: OrchdResponse) -> OrchdErrorCode {
    match res {
        OrchdResponse::Error { code, .. } => code,
        other => panic!("expected Error, got {other:?}"),
    }
}

// ---- S-EXT Connector dispatch response helpers (task T13a) ----

fn expect_account(res: OrchdResponse) -> Account {
    match res {
        OrchdResponse::Account(a) => a,
        other => panic!("expected Account, got {other:?}"),
    }
}

fn expect_accounts(res: OrchdResponse) -> Vec<Account> {
    match res {
        OrchdResponse::Accounts(v) => v,
        other => panic!("expected Accounts, got {other:?}"),
    }
}

fn expect_connector_ops(res: OrchdResponse) -> Vec<ConnectorOp> {
    match res {
        OrchdResponse::ConnectorOps(v) => v,
        other => panic!("expected ConnectorOps, got {other:?}"),
    }
}

fn expect_connector_providers(res: OrchdResponse) -> Vec<String> {
    match res {
        OrchdResponse::ConnectorProviders(v) => v,
        other => panic!("expected ConnectorProviders, got {other:?}"),
    }
}

fn expect_oauth_challenge(res: OrchdResponse) -> OAuthChallenge {
    match res {
        OrchdResponse::OAuthChallenge(c) => c,
        other => panic!("expected OAuthChallenge, got {other:?}"),
    }
}

// ---- S-EXT Skills dispatch response helpers (task T17) ----

fn expect_skill(res: OrchdResponse) -> Skill {
    match res {
        OrchdResponse::Skill(s) => s,
        other => panic!("expected Skill, got {other:?}"),
    }
}

fn expect_skills(res: OrchdResponse) -> Vec<Skill> {
    match res {
        OrchdResponse::Skills(v) => v,
        other => panic!("expected Skills, got {other:?}"),
    }
}

// ---- S-EXT Trust dispatch response helpers (task T18, BL-22) ----

fn expect_policy(res: OrchdResponse) -> Policy {
    match res {
        OrchdResponse::Policy(p) => p,
        other => panic!("expected Policy, got {other:?}"),
    }
}

fn expect_policies(res: OrchdResponse) -> Vec<Policy> {
    match res {
        OrchdResponse::Policies(v) => v,
        other => panic!("expected Policies, got {other:?}"),
    }
}

fn expect_audit_rows(res: OrchdResponse) -> Vec<AuditRow> {
    match res {
        OrchdResponse::AuditRows(v) => v,
        other => panic!("expected AuditRows, got {other:?}"),
    }
}

fn expect_research_run(res: OrchdResponse) -> ResearchRun {
    match res {
        OrchdResponse::ResearchRun(r) => r,
        other => panic!("expected ResearchRun, got {other:?}"),
    }
}

fn expect_research_runs(res: OrchdResponse) -> Vec<ResearchRun> {
    match res {
        OrchdResponse::ResearchRuns(v) => v,
        other => panic!("expected ResearchRuns, got {other:?}"),
    }
}

/// `McpAddServer` convenience: an `http`, globally-scoped, unauthenticated server pointed at
/// `url` (the loopback stub's base url from [`spawn_stub_mcp_server`]) — every MCP dispatch test
/// below that needs "a registered server" uses this.
async fn add_mcp_server(c: &mut Client, url: &str) -> McpServer {
    expect_mcp_server(
        c.request(OrchdRequest::McpAddServer {
            name: "Stub".to_string(),
            transport: McpTransport::Http,
            url: Some(url.to_string()),
            command: None,
            args: None,
            env: None,
            scope: McpScope::Global,
            project_id: None,
            auth_kind: McpAuthKind::None,
            timeout_ms: None,
            max_retries: None,
        })
        .await,
    )
}

/// Test-only convenience: `CreateProject` with a freshly generated (guaranteed-unique)
/// `workspace_id` — `project_workspace.workspace_id` is UNIQUE table-wide (S3 spec §5.2), so
/// every project a graph test creates needs its own.
async fn create_project(c: &mut Client, name: &str) -> Project {
    expect_project(
        c.request(OrchdRequest::CreateProject {
            name: name.to_string(),
            description: String::new(),
            workspace_ids: vec![uuid::Uuid::new_v4().to_string()],
        })
        .await,
    )
}

/// Test-only convenience: `CreateIdea`, project-less (`project_id: None`) — every research
/// dispatch test below that needs "an idea to run research against" uses this.
async fn create_idea(c: &mut Client, title: &str) -> Idea {
    expect_idea(
        c.request(OrchdRequest::CreateIdea {
            project_id: None,
            title: title.to_string(),
            body: String::new(),
        })
        .await,
    )
}

/// Test-only convenience: `GraphAddNode` with a fixed `Concept` kind / origin position — every
/// graph dispatch test that needs "a node to exist" and doesn't care about its kind/position uses
/// this.
async fn add_node(c: &mut Client, project_id: &str, label: &str) -> GraphNode {
    expect_graph_node(
        c.request(OrchdRequest::GraphAddNode {
            project_id: project_id.to_string(),
            kind: GraphNodeKind::Concept,
            label: label.to_string(),
            body: String::new(),
            pos_x: 0.0,
            pos_y: 0.0,
        })
        .await,
    )
}

/// Test-only convenience: `GraphAddEdge` with a fixed `Relates` kind / empty label.
async fn add_edge(c: &mut Client, source_node_id: &str, target_node_id: &str) -> GraphEdge {
    expect_graph_edge(
        c.request(OrchdRequest::GraphAddEdge {
            source_node_id: source_node_id.to_string(),
            target_node_id: target_node_id.to_string(),
            kind: GraphEdgeKind::Relates,
            label: String::new(),
        })
        .await,
    )
}

/// Collects exactly `n` pushes (each bounded by a generous per-push timeout, mirroring
/// `import_bundle_happy_path_returns_report_and_broadcasts_family_pushes`'s multi-push collection
/// pattern) — used by the cross-project graph tests, which expect more than one `GraphChanged`
/// push per mutation.
async fn collect_pushes(c: &mut Client, n: usize) -> Vec<OrchdPush> {
    let mut seen = Vec::with_capacity(n);
    for _ in 0..n {
        seen.push(
            c.recv_push_timeout(Duration::from_secs(2))
                .await
                .unwrap_or_else(|| panic!("expected {n} pushes, got only {}", seen.len())),
        );
    }
    seen
}

/// Boots `bpa_orchd::run()` on a fresh temp socket. The caller MUST already hold a `HomeGuard`
/// (its lifetime must outlive every request this test sends — `app_support_dir()` is re-read on
/// every `CreateProject`/`ImportBundle`, not cached at boot).
async fn boot_daemon(socket: &Path) -> tokio::task::JoinHandle<std::io::Result<()>> {
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let socket_for_task = socket.to_path_buf();
    tokio::spawn(async move { bpa_orchd::run(socket_for_task, shutdown_tx, shutdown_rx).await })
}

// ================================================================================
// ---- S-EXT MCP dispatch (task T6, spec §5/§6): real per-verb socket dispatch + coarse-
// invalidation pushes. The connect/call tests below drive a REAL loopback rmcp Streamable-HTTP
// stub MCP server (bound to `127.0.0.1:0`, spec §9's "e2e ... against a local stub MCP server")
// rather than an in-memory/duplex fake — this exercises the exact production path
// `socket_server::dispatch` uses: `mcp::connect_session` (`crates/orchd/src/mcp/mod.rs`) ->
// `bpa_mcp::connect(TransportConfig::Http{url}, bearer)` -> a real rmcp `StreamableHttpClient`
// transport talking HTTP to this process's own loopback server. Mirrors rmcp 2.2.0's OWN
// `tests/test_streamable_http_protocol_version.rs` axum-wiring pattern (verified against the
// vendored crate source) and `bpa-mcp`'s `tests/stub.rs` `#[tool_router(server_handler)]`
// unit-struct shape (verified working in task T4) — combining the two: T4's proven macro
// pattern, served over T's proven axum/TcpListener wiring instead of a duplex pair.
// ================================================================================

#[derive(Debug, serde::Deserialize, rmcp::schemars::JsonSchema)]
struct EchoRequest {
    /// The message to echo back.
    msg: String,
}

/// The loopback stub MCP server: an `echo` tool plus a deliberately-`slow_echo` tool (sleeps
/// [`SLOW_ECHO_DELAY`] server-side before replying), mirroring `bpa-mcp`'s own
/// `tests/stub.rs::StubServer` shape (unit struct + `#[tool_router(server_handler)]`, which
/// auto-generates the whole `ServerHandler` impl — no manual `call_tool`/`list_tools` wiring
/// needed). `slow_echo` exists only for `mcp_call_tool_does_not_block_other_db_ops` (T6 review
/// fix): it holds the tool-call's NETWORK phase open long enough to prove orchd is NOT holding
/// the DB lock across it.
#[derive(Debug, Clone, Copy, Default)]
struct EchoServer;

/// How long `slow_echo` sleeps server-side before echoing. Chosen well above the few-ms a
/// concurrent `ListProjects` needs, so the concurrency assertion has a wide, non-flaky margin.
const SLOW_ECHO_DELAY: Duration = Duration::from_millis(1500);

#[rmcp::tool_router(server_handler)]
impl EchoServer {
    #[rmcp::tool(description = "Echo the given message back")]
    fn echo(
        &self,
        rmcp::handler::server::wrapper::Parameters(EchoRequest { msg }): rmcp::handler::server::wrapper::Parameters<
            EchoRequest,
        >,
    ) -> String {
        msg
    }

    #[rmcp::tool(description = "Echo the given message back after a deliberate server-side delay")]
    async fn slow_echo(
        &self,
        rmcp::handler::server::wrapper::Parameters(EchoRequest { msg }): rmcp::handler::server::wrapper::Parameters<
            EchoRequest,
        >,
    ) -> String {
        tokio::time::sleep(SLOW_ECHO_DELAY).await;
        msg
    }
}

/// Spawns [`EchoServer`] on an OS-assigned loopback TCP port via rmcp's `StreamableHttpService`
/// mounted into a minimal `axum::Router` (exactly rmcp 2.2.0's own
/// `test_streamable_http_protocol_version.rs::spawn_server_with_manager` shape) and returns its
/// base url (`http://127.0.0.1:<port>/mcp`) — the value every test below passes as
/// `McpAddServer.url`. The server task runs for the remainder of the test process; no explicit
/// shutdown is needed (dropped along with the tokio runtime when the test function returns,
/// mirroring how every OTHER spawned task in this file that isn't explicitly `.shutdown()`ed —
/// e.g. `boot_daemon`'s own task after a test panics — is cleaned up).
async fn spawn_stub_mcp_server() -> String {
    let session_manager = std::sync::Arc::new(
        rmcp::transport::streamable_http_server::session::local::LocalSessionManager::default(),
    );
    let service = rmcp::transport::streamable_http_server::StreamableHttpService::new(
        || Ok(EchoServer),
        session_manager,
        rmcp::transport::streamable_http_server::StreamableHttpServerConfig::default(),
    );
    let router = axum::Router::new().nest_service("/mcp", service);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind loopback stub mcp server");
    let addr = listener.local_addr().expect("stub mcp server local_addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    format!("http://{addr}/mcp")
}

#[tokio::test]
async fn create_project_returns_project_broadcasts_projects_changed_and_writes_ruleset_file() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;

    let mut c1 = Client::connect(&socket).await;
    let mut c2 = Client::connect(&socket).await;
    // Synchronize: a Ping/Pong round trip on c2 proves the server task has finished registering
    // c2 with the broadcaster (registration happens synchronously, in the same per-connection
    // task, strictly BEFORE the dispatch loop that would answer this Ping) — otherwise c1's
    // CreateProject below could race c2's registration and the push would be silently missed (a
    // not-yet-registered client looks identical to a dead one to `Broadcaster::broadcast`).
    assert_eq!(c2.request(OrchdRequest::Ping).await, OrchdResponse::Pong);

    let project = expect_project(
        c1.request(OrchdRequest::CreateProject {
            name: "Acme".to_string(),
            description: "desc".to_string(),
            workspace_ids: vec!["w1".to_string()],
        })
        .await,
    );
    assert_eq!(project.workspace_ids, vec!["w1".to_string()]);
    assert!(!project.id.is_empty());

    match c2.recv_push().await {
        OrchdPush::ProjectsChanged => {}
        other => panic!("expected ProjectsChanged, got {other:?}"),
    }

    let md_path = home_dir
        .path()
        .join("Library/Application Support/ai.builderpro.desktop/rules")
        .join(format!("project-{}.md", project.id));
    let content = std::fs::read_to_string(&md_path)
        .unwrap_or_else(|e| panic!("ruleset file must exist at {}: {e}", md_path.display()));
    assert_eq!(content, "# Project rules Acme\n");

    c1.shutdown(boot).await;
}

#[tokio::test]
async fn create_goal_broadcasts_goals_changed_with_correct_project_id() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;

    let mut c1 = Client::connect(&socket).await;
    let project = expect_project(
        c1.request(OrchdRequest::CreateProject {
            name: "Proj".to_string(),
            description: String::new(),
            workspace_ids: vec!["w1".to_string()],
        })
        .await,
    );

    let goals = expect_goals(
        c1.request(OrchdRequest::ListGoals {
            project_id: project.id.clone(),
        })
        .await,
    );
    let strategic = goals
        .into_iter()
        .find(|g| g.kind == GoalKind::Strategic)
        .expect("CreateProject auto-creates a strategic goal");

    // c2 connects only AFTER the project (and its own ProjectsChanged push) already landed, so
    // the only push it can possibly observe below is the one this test is actually proving.
    let mut c2 = Client::connect(&socket).await;
    assert_eq!(c2.request(OrchdRequest::Ping).await, OrchdResponse::Pong);

    let goal = expect_goal(
        c1.request(OrchdRequest::CreateGoal {
            project_id: project.id.clone(),
            parent_id: Some(strategic.id.clone()),
            kind: GoalKind::Additional,
            title: "Child goal".to_string(),
            body: String::new(),
        })
        .await,
    );
    assert_eq!(goal.project_id, project.id);

    match c2.recv_push().await {
        OrchdPush::GoalsChanged { project_id } => assert_eq!(project_id, project.id),
        other => panic!("expected GoalsChanged, got {other:?}"),
    }

    c1.shutdown(boot).await;
}

#[tokio::test]
async fn remove_last_project_workspace_is_invariant_and_broadcasts_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;

    let mut c1 = Client::connect(&socket).await;
    let project = expect_project(
        c1.request(OrchdRequest::CreateProject {
            name: "Proj".to_string(),
            description: String::new(),
            workspace_ids: vec!["only-ws".to_string()],
        })
        .await,
    );

    let mut c2 = Client::connect(&socket).await;
    assert_eq!(c2.request(OrchdRequest::Ping).await, OrchdResponse::Pong);

    let res = c1
        .request(OrchdRequest::RemoveProjectWorkspace {
            project_id: project.id.clone(),
            workspace_id: "only-ws".to_string(),
        })
        .await;
    match res {
        OrchdResponse::Error { code, .. } => assert_eq!(code, OrchdErrorCode::Invariant),
        other => panic!("expected Error{{Invariant}}, got {other:?}"),
    }

    assert!(
        c2.recv_push_timeout(Duration::from_millis(300))
            .await
            .is_none(),
        "a failed RemoveProjectWorkspace must broadcast nothing"
    );

    c1.shutdown(boot).await;
}

#[tokio::test]
async fn unarchive_project_returns_active_project_and_broadcasts_projects_changed() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;

    let mut c1 = Client::connect(&socket).await;
    let project = create_project(&mut c1, "Acme").await;
    let archived = expect_project(
        c1.request(OrchdRequest::ArchiveProject {
            id: project.id.clone(),
        })
        .await,
    );
    assert_eq!(archived.status, ProjectStatus::Archived);

    // c2 connects only AFTER the archive (and its ProjectsChanged push) already landed, so the
    // only push it can see below is the un-archive's own.
    let mut c2 = Client::connect(&socket).await;
    assert_eq!(c2.request(OrchdRequest::Ping).await, OrchdResponse::Pong);

    let restored = expect_project(
        c1.request(OrchdRequest::UnarchiveProject {
            id: project.id.clone(),
        })
        .await,
    );
    assert_eq!(restored.id, project.id);
    assert_eq!(restored.status, ProjectStatus::Active);

    match c2.recv_push().await {
        OrchdPush::ProjectsChanged => {}
        other => panic!("expected ProjectsChanged, got {other:?}"),
    }

    c1.shutdown(boot).await;
}

#[tokio::test]
async fn unarchive_active_project_is_invariant_and_broadcasts_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;

    let mut c1 = Client::connect(&socket).await;
    let project = create_project(&mut c1, "Acme").await;

    let mut c2 = Client::connect(&socket).await;
    assert_eq!(c2.request(OrchdRequest::Ping).await, OrchdResponse::Pong);

    // The project is still `active` — un-archiving it is the mirror `Invariant` of archiving an
    // already-archived project (spec D7): nothing to reverse.
    let res = c1
        .request(OrchdRequest::UnarchiveProject {
            id: project.id.clone(),
        })
        .await;
    match res {
        OrchdResponse::Error { code, message } => {
            assert_eq!(code, OrchdErrorCode::Invariant);
            assert_eq!(message, "invariant violated: project is not archived");
        }
        other => panic!("expected Error{{Invariant}}, got {other:?}"),
    }

    assert!(
        c2.recv_push_timeout(Duration::from_millis(300))
            .await
            .is_none(),
        "a failed UnarchiveProject must broadcast nothing"
    );

    c1.shutdown(boot).await;
}

#[tokio::test]
async fn get_ruleset_ok_then_externally_modified_after_on_disk_edit() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;

    let mut c1 = Client::connect(&socket).await;
    let project = expect_project(
        c1.request(OrchdRequest::CreateProject {
            name: "Acme".to_string(),
            description: String::new(),
            workspace_ids: vec!["w1".to_string()],
        })
        .await,
    );

    let view = expect_ruleset_view(
        c1.request(OrchdRequest::GetRuleSet {
            scope: RuleScope::Project,
            project_id: Some(project.id.clone()),
        })
        .await,
    );
    assert_eq!(view.file_state, RuleFileState::Ok);
    assert_eq!(view.md_content.as_deref(), Some("# Project rules Acme\n"));

    std::fs::write(&view.rule.md_path, "someone edited this by hand\n").unwrap();

    let view2 = expect_ruleset_view(
        c1.request(OrchdRequest::GetRuleSet {
            scope: RuleScope::Project,
            project_id: Some(project.id.clone()),
        })
        .await,
    );
    assert_eq!(view2.file_state, RuleFileState::ExternallyModified);
    assert_eq!(
        view2.md_content.as_deref(),
        Some("someone edited this by hand\n")
    );

    c1.shutdown(boot).await;
}

/// SCN-054 (FLW-21, ST-041) full docs lifecycle over the wire: `UpsertDoc` creates the row AND
/// the file under the project's rules dir (`rules/docs/<project-id>/<name>.md`) and broadcasts
/// `DocsChanged{project_id}` to the other connected client; `ListDocs`/`GetDoc` read it back;
/// an on-disk hand-edit flips `GetDoc` to `ExternallyModified` (the "file changed externally" +
/// Accept banner) and `AcknowledgeDocFile` accepts it back to `Ok`; deleting the file on disk
/// flips `GetDoc` to `Missing` (the "file lost" + Recreate banner) and `UpsertDoc{md_content:
/// ""}` recreates it in place; `DeleteDoc` removes row + file and answers `Ack`. Every mutation
/// pushes, every read doesn't (spec §6's mutating-request scoping) — the exact ruleset contract,
/// times N named files.
#[tokio::test]
async fn docs_lifecycle_round_trip_with_external_change_lost_file_and_pushes() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;

    let mut c1 = Client::connect(&socket).await;
    let mut c2 = Client::connect(&socket).await;
    // Synchronize: c2's Pong proves its push registration happened-before c1's mutations below
    // (mirrors `create_with_priority_and_set_task_priority_round_trip_and_push`).
    assert_eq!(c2.request(OrchdRequest::Ping).await, OrchdResponse::Pong);

    let project = create_project(&mut c1, "Docs").await;
    assert_eq!(
        c2.recv_push_timeout(Duration::from_secs(2)).await,
        Some(OrchdPush::ProjectsChanged)
    );

    // Empty state first (SCN-054: "No documents in this project yet.").
    let empty = expect_docs(
        c1.request(OrchdRequest::ListDocs {
            project_id: project.id.clone(),
        })
        .await,
    );
    assert!(empty.is_empty());

    // "+ doc" — create via the one upsert verb.
    let created = expect_doc_view(
        c1.request(OrchdRequest::UpsertDoc {
            project_id: project.id.clone(),
            name: "notes".to_string(),
            md_content: "# notes\n".to_string(),
        })
        .await,
    );
    assert_eq!(created.doc.name, "notes");
    assert_eq!(created.file_state, RuleFileState::Ok);
    assert_eq!(created.md_content.as_deref(), Some("# notes\n"));
    let expected_path = home_dir
        .path()
        .join("Library/Application Support/ai.builderpro.desktop/rules/docs")
        .join(&project.id)
        .join("notes.md");
    assert_eq!(created.doc.md_path, expected_path.to_string_lossy());
    assert_eq!(
        std::fs::read_to_string(&expected_path).unwrap(),
        "# notes\n"
    );
    assert_eq!(
        c2.recv_push_timeout(Duration::from_secs(2)).await,
        Some(OrchdPush::DocsChanged {
            project_id: project.id.clone()
        })
    );

    // The list shows name + last-modified (mtime-scale sanity only — exact mtime is the FS's).
    let listed = expect_docs(
        c1.request(OrchdRequest::ListDocs {
            project_id: project.id.clone(),
        })
        .await,
    );
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name, "notes");
    assert!(listed[0].modified_at > 0);

    // External hand-edit (an agent writing the same file) → "file changed externally".
    std::fs::write(&expected_path, "agent-edited\n").unwrap();
    let modified = expect_doc_view(
        c1.request(OrchdRequest::GetDoc {
            project_id: project.id.clone(),
            name: "notes".to_string(),
        })
        .await,
    );
    assert_eq!(modified.file_state, RuleFileState::ExternallyModified);
    assert_eq!(modified.md_content.as_deref(), Some("agent-edited\n"));

    // [Accept] → the new content is the accepted truth, state back to Ok; pushes.
    let acknowledged = expect_doc_view(
        c1.request(OrchdRequest::AcknowledgeDocFile {
            id: created.doc.id.clone(),
        })
        .await,
    );
    assert_eq!(acknowledged.file_state, RuleFileState::Ok);
    assert_eq!(acknowledged.md_content.as_deref(), Some("agent-edited\n"));
    assert_eq!(
        c2.recv_push_timeout(Duration::from_secs(2)).await,
        Some(OrchdPush::DocsChanged {
            project_id: project.id.clone()
        })
    );

    // File lost on disk → Missing, no content.
    std::fs::remove_file(&expected_path).unwrap();
    let lost = expect_doc_view(
        c1.request(OrchdRequest::GetDoc {
            project_id: project.id.clone(),
            name: "notes".to_string(),
        })
        .await,
    );
    assert_eq!(lost.file_state, RuleFileState::Missing);
    assert_eq!(lost.md_content, None);

    // [Recreate] — the same upsert verb with empty content; pushes.
    let recreated = expect_doc_view(
        c1.request(OrchdRequest::UpsertDoc {
            project_id: project.id.clone(),
            name: "notes".to_string(),
            md_content: String::new(),
        })
        .await,
    );
    assert_eq!(recreated.doc.id, created.doc.id);
    assert_eq!(recreated.file_state, RuleFileState::Ok);
    assert_eq!(std::fs::read_to_string(&expected_path).unwrap(), "");
    assert_eq!(
        c2.recv_push_timeout(Duration::from_secs(2)).await,
        Some(OrchdPush::DocsChanged {
            project_id: project.id.clone()
        })
    );

    // "delete document?" confirmed → Ack, row + file gone, pushes.
    expect_ack(
        c1.request(OrchdRequest::DeleteDoc {
            id: created.doc.id.clone(),
        })
        .await,
    );
    assert!(!expected_path.exists());
    assert_eq!(
        c2.recv_push_timeout(Duration::from_secs(2)).await,
        Some(OrchdPush::DocsChanged {
            project_id: project.id.clone()
        })
    );
    let after_delete = expect_docs(
        c1.request(OrchdRequest::ListDocs {
            project_id: project.id.clone(),
        })
        .await,
    );
    assert!(after_delete.is_empty());

    c1.shutdown(boot).await;
}

/// SCN-054 error paths over the wire: a traversal/invalid name answers `Error{Validation}` and
/// broadcasts nothing (a FAILED verb never pushes, spec §6), and `GetDoc` on an unknown name
/// answers `Error{NotFound}` (the toast path in `DocsPanel.tsx`).
#[tokio::test]
async fn invalid_doc_name_is_validation_and_unknown_doc_is_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;

    let mut c1 = Client::connect(&socket).await;
    let mut c2 = Client::connect(&socket).await;
    assert_eq!(c2.request(OrchdRequest::Ping).await, OrchdResponse::Pong);

    let project = create_project(&mut c1, "Docs").await;
    assert_eq!(
        c2.recv_push_timeout(Duration::from_secs(2)).await,
        Some(OrchdPush::ProjectsChanged)
    );

    match c1
        .request(OrchdRequest::UpsertDoc {
            project_id: project.id.clone(),
            name: "../escape".to_string(),
            md_content: "evil".to_string(),
        })
        .await
    {
        OrchdResponse::Error { code, .. } => assert_eq!(code, OrchdErrorCode::Validation),
        other => panic!("expected Error{{Validation}}, got {other:?}"),
    }
    // A failed verb broadcasts nothing.
    assert_eq!(c2.recv_push_timeout(Duration::from_millis(300)).await, None);

    match c1
        .request(OrchdRequest::GetDoc {
            project_id: project.id.clone(),
            name: "never-created".to_string(),
        })
        .await
    {
        OrchdResponse::Error { code, .. } => assert_eq!(code, OrchdErrorCode::NotFound),
        other => panic!("expected Error{{NotFound}}, got {other:?}"),
    }

    c1.shutdown(boot).await;
}

/// SW1 (docs/ux/plans/2026-07-24-workflow-authoring.md) full workflow-authoring lifecycle over the
/// wire: `WorkflowUpsert` (empty id) creates the row AND the JSON file under the app-support rules
/// tree and broadcasts the bare `WorkflowsChanged` to the other client; `WorkflowList`/
/// `WorkflowGet` read the full definition back; an update mutates it and pushes again; an invalid
/// save (`WorkflowUpsert` with an unknown `defaultAgent`) answers `Error{Validation}` and
/// broadcasts NOTHING (a failed verb never pushes, spec §6); `WorkflowDelete` removes row + file,
/// answers `Ack` and pushes. Authoring only — nothing here runs a workflow (S6b).
#[tokio::test]
async fn workflow_authoring_lifecycle_round_trips_with_validation_and_pushes() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;

    let mut c1 = Client::connect(&socket).await;
    let mut c2 = Client::connect(&socket).await;
    // c2's Pong proves its push registration happened-before c1's mutations (mirrors the docs test).
    assert_eq!(c2.request(OrchdRequest::Ping).await, OrchdResponse::Pong);

    let project = create_project(&mut c1, "WF").await;
    assert_eq!(
        c2.recv_push_timeout(Duration::from_secs(2)).await,
        Some(OrchdPush::ProjectsChanged)
    );

    // Empty library first.
    assert!(expect_workflows(
        c1.request(OrchdRequest::WorkflowList {
            scope: None,
            project_id: None,
        })
        .await
    )
    .is_empty());

    // "+ New workflow" — create via the one upsert verb (empty id ⇒ create). Two stages: one
    // pinning a known agent, one inheriting the workflow default (agent: None).
    let stages = vec![
        Stage {
            id: "s0".to_string(),
            name: "plan".to_string(),
            prompt: "Draft the plan".to_string(),
            skill_ids: vec![],
            agent: Some("hermes".to_string()),
            context_scope: ContextScope::Handoff,
            outputs: vec!["plan.md".to_string()],
            gate: Gate::Manual,
        },
        Stage {
            id: "s1".to_string(),
            name: "build".to_string(),
            prompt: "Build it".to_string(),
            skill_ids: vec![],
            agent: None,
            context_scope: ContextScope::Inherit,
            outputs: vec![],
            gate: Gate::Auto,
        },
    ];
    let created = expect_workflow(
        c1.request(OrchdRequest::WorkflowUpsert {
            id: String::new(),
            name: "ship-feature".to_string(),
            description: "Author, review, ship".to_string(),
            scope: WorkflowScope::Project,
            project_id: Some(project.id.clone()),
            default_agent: "claude-code".to_string(),
            stages: stages.clone(),
            global_skill_ids: vec!["gs-1".to_string()],
            supervisor: SupervisorConfig::default(),
        })
        .await,
    );
    assert!(!created.id.is_empty());
    assert_eq!(created.stages, stages);
    assert_eq!(created.file_state, SkillFileState::Present);
    assert!(!created.hash.is_empty());
    let expected_path = home_dir
        .path()
        .join("Library/Application Support/ai.builderpro.desktop/rules/workflows")
        .join(&project.id)
        .join(format!("{}.json", created.id));
    assert_eq!(created.json_path, expected_path.to_string_lossy());
    assert!(expected_path.exists(), "the JSON file must be written");
    assert_eq!(
        c2.recv_push_timeout(Duration::from_secs(2)).await,
        Some(OrchdPush::WorkflowsChanged)
    );

    // List + get read the full definition back.
    let listed = expect_workflows(
        c1.request(OrchdRequest::WorkflowList {
            scope: Some(WorkflowScope::Project),
            project_id: Some(project.id.clone()),
        })
        .await,
    );
    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].id, created.id);
    let got = expect_workflow(
        c1.request(OrchdRequest::WorkflowGet {
            id: created.id.clone(),
        })
        .await,
    );
    assert_eq!(got, created);

    // Update mutates + pushes.
    let updated = expect_workflow(
        c1.request(OrchdRequest::WorkflowUpsert {
            id: created.id.clone(),
            name: "ship-feature".to_string(),
            description: "now with review".to_string(),
            scope: WorkflowScope::Project,
            project_id: Some(project.id.clone()),
            default_agent: "opencode".to_string(),
            stages: stages.clone(),
            global_skill_ids: vec![],
            supervisor: SupervisorConfig::default(),
        })
        .await,
    );
    assert_eq!(updated.id, created.id);
    assert_eq!(updated.default_agent, "opencode");
    assert_eq!(updated.description, "now with review");
    assert_eq!(
        c2.recv_push_timeout(Duration::from_secs(2)).await,
        Some(OrchdPush::WorkflowsChanged)
    );

    // An invalid save (unknown defaultAgent) answers Error{Validation} and pushes NOTHING.
    match c1
        .request(OrchdRequest::WorkflowUpsert {
            id: String::new(),
            name: "bad".to_string(),
            description: String::new(),
            scope: WorkflowScope::Global,
            project_id: None,
            default_agent: "gpt-5".to_string(),
            stages: vec![],
            global_skill_ids: vec![],
            supervisor: SupervisorConfig::default(),
        })
        .await
    {
        OrchdResponse::Error { code, .. } => assert_eq!(code, OrchdErrorCode::Validation),
        other => panic!("expected Error{{Validation}}, got {other:?}"),
    }
    assert_eq!(c2.recv_push_timeout(Duration::from_millis(300)).await, None);

    // Delete removes row + file, answers Ack and pushes.
    expect_ack(
        c1.request(OrchdRequest::WorkflowDelete {
            id: created.id.clone(),
        })
        .await,
    );
    assert!(!expected_path.exists(), "the JSON file must be removed");
    assert_eq!(
        c2.recv_push_timeout(Duration::from_secs(2)).await,
        Some(OrchdPush::WorkflowsChanged)
    );
    assert!(expect_workflows(
        c1.request(OrchdRequest::WorkflowList {
            scope: None,
            project_id: None,
        })
        .await
    )
    .is_empty());

    c1.shutdown(boot).await;
}

#[tokio::test]
async fn set_idea_project_none_detaches() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;

    let mut c1 = Client::connect(&socket).await;
    let project = expect_project(
        c1.request(OrchdRequest::CreateProject {
            name: "Proj".to_string(),
            description: String::new(),
            workspace_ids: vec!["w1".to_string()],
        })
        .await,
    );

    let idea = expect_idea(
        c1.request(OrchdRequest::CreateIdea {
            project_id: Some(project.id.clone()),
            title: "Idea".to_string(),
            body: String::new(),
        })
        .await,
    );
    assert_eq!(idea.project_id.as_deref(), Some(project.id.as_str()));

    let detached = expect_idea(
        c1.request(OrchdRequest::SetIdeaProject {
            id: idea.id.clone(),
            project_id: None,
        })
        .await,
    );
    assert_eq!(detached.project_id, None);

    c1.shutdown(boot).await;
}

#[tokio::test]
async fn import_bundle_happy_path_returns_report_and_broadcasts_family_pushes() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;

    let mut c1 = Client::connect(&socket).await;
    let mut c2 = Client::connect(&socket).await;
    assert_eq!(c2.request(OrchdRequest::Ping).await, OrchdResponse::Pong);

    let project_id = uuid::Uuid::new_v4().to_string();
    let goal_id = uuid::Uuid::new_v4().to_string();
    let task_id = uuid::Uuid::new_v4().to_string();
    let json = serde_json::json!({
        "bundleFormat": 1,
        "exportedAt": 0,
        "project": {
            "id": project_id, "name": "Imported", "description": "",
            "status": "active", "workspaceIds": ["import-ws"],
            "createdAt": 0, "updatedAt": 0
        },
        "goals": [{
            "id": goal_id, "projectId": project_id, "parentId": null,
            "kind": "strategic", "title": "Strategic", "body": "", "ord": 0,
            "status": "active", "metricRefs": [], "createdAt": 0, "updatedAt": 0
        }],
        "ideas": [],
        "insights": [],
        "tasks": [{
            "id": task_id, "projectId": project_id, "parentId": null,
            "title": "T", "body": "", "status": "backlog", "source": "plan",
            "sourceId": null, "tags": [], "rank": 1024.0, "rankAgent": null,
            "rankAgentReasoning": "", "createdAt": 0, "updatedAt": 0
        }],
        "ruleset": null
    })
    .to_string();

    let res = c1.request(OrchdRequest::ImportBundle { json }).await;
    match res {
        OrchdResponse::ImportReport {
            projects,
            goals,
            ideas,
            insights,
            tasks,
            rulesets,
        } => {
            assert_eq!(projects, 1);
            assert_eq!(goals, 1);
            assert_eq!(ideas, 0);
            assert_eq!(insights, 0);
            assert_eq!(tasks, 1);
            assert_eq!(rulesets, 0);
        }
        other => panic!("expected ImportReport, got {other:?}"),
    }

    // This bundle touches exactly 3 families (project, goals, tasks) — no ideas/insights/ruleset.
    let mut seen = Vec::new();
    for _ in 0..3 {
        seen.push(c2.recv_push_timeout(Duration::from_secs(2)).await.expect(
            "expected a push within the timeout — ImportBundle must broadcast every touched family",
        ));
    }
    assert!(
        seen.contains(&OrchdPush::ProjectsChanged),
        "missing ProjectsChanged in {seen:?}"
    );
    assert!(
        seen.contains(&OrchdPush::GoalsChanged {
            project_id: project_id.clone()
        }),
        "missing GoalsChanged in {seen:?}"
    );
    assert!(
        seen.contains(&OrchdPush::TasksChanged {
            project_id: project_id.clone()
        }),
        "missing TasksChanged in {seen:?}"
    );
    assert!(
        c2.recv_push_timeout(Duration::from_millis(200))
            .await
            .is_none(),
        "no extra pushes beyond the 3 touched families"
    );

    c1.shutdown(boot).await;
}

/// SCN-051 (ST-037) wire round-trip: `CreateTask{priority: Some(Urgent)}` persists the priority,
/// omitting it (`None`) defaults to `Normal`, and the appended `SetTaskPriority` verb flips it —
/// each mutation answering `OrchdResponse::Task` AND broadcasting `TasksChanged{project_id}` to
/// the other connected client (the same push contract as every other task verb).
#[tokio::test]
async fn create_with_priority_and_set_task_priority_round_trip_and_push() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;

    let mut c1 = Client::connect(&socket).await;
    let mut c2 = Client::connect(&socket).await;
    // Synchronize: c2's Pong proves its push registration happened-before c1's mutations below
    // (mirrors `import_bundle_happy_path_returns_report_and_broadcasts_family_pushes`).
    assert_eq!(c2.request(OrchdRequest::Ping).await, OrchdResponse::Pong);

    let project = create_project(&mut c1, "Priorities").await;
    // create_project broadcasts ProjectsChanged — drain it so the task-push asserts stay exact.
    assert_eq!(
        c2.recv_push_timeout(Duration::from_secs(2)).await,
        Some(OrchdPush::ProjectsChanged)
    );

    // Create with an explicit urgent priority.
    let urgent = expect_task(
        c1.request(OrchdRequest::CreateTask {
            project_id: project.id.clone(),
            parent_id: None,
            title: "urgent one".into(),
            body: String::new(),
            status: None,
            source: TaskSource::Plan,
            source_id: None,
            tags: vec![],
            priority: Some(TaskPriority::Urgent),
        })
        .await,
    );
    assert_eq!(urgent.priority, TaskPriority::Urgent);
    assert_eq!(
        c2.recv_push_timeout(Duration::from_secs(2)).await,
        Some(OrchdPush::TasksChanged {
            project_id: project.id.clone()
        })
    );

    // Create WITHOUT a priority — defaults to Normal (SCN-051 create-form default).
    let normal = expect_task(
        c1.request(OrchdRequest::CreateTask {
            project_id: project.id.clone(),
            parent_id: None,
            title: "normal one".into(),
            body: String::new(),
            status: None,
            source: TaskSource::Plan,
            source_id: None,
            tags: vec![],
            priority: None,
        })
        .await,
    );
    assert_eq!(normal.priority, TaskPriority::Normal);
    assert_eq!(
        c2.recv_push_timeout(Duration::from_secs(2)).await,
        Some(OrchdPush::TasksChanged {
            project_id: project.id.clone()
        })
    );

    // Flip it urgent via the appended SetTaskPriority verb.
    let flipped = expect_task(
        c1.request(OrchdRequest::SetTaskPriority {
            id: normal.id.clone(),
            priority: TaskPriority::Urgent,
        })
        .await,
    );
    assert_eq!(flipped.id, normal.id);
    assert_eq!(flipped.priority, TaskPriority::Urgent);
    assert_eq!(
        c2.recv_push_timeout(Duration::from_secs(2)).await,
        Some(OrchdPush::TasksChanged {
            project_id: project.id.clone()
        })
    );

    // And the flip is durable: an independent ListTasks read sees it.
    match c1
        .request(OrchdRequest::ListTasks {
            project_id: Some(project.id.clone()),
        })
        .await
    {
        OrchdResponse::Tasks(tasks) => {
            let refetched = tasks.iter().find(|t| t.id == normal.id).unwrap();
            assert_eq!(refetched.priority, TaskPriority::Urgent);
        }
        other => panic!("expected Tasks, got {other:?}"),
    }

    c1.shutdown(boot).await;
}

/// SCN-051 error path: `SetTaskPriority` on an unknown id answers `Error{NotFound}` (the toast
/// + revert path in `TasksList.tsx`), mirroring `unknown_id_delete_task_is_not_found` below.
#[tokio::test]
async fn unknown_id_set_task_priority_is_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;

    let mut c1 = Client::connect(&socket).await;
    let res = c1
        .request(OrchdRequest::SetTaskPriority {
            id: "nonexistent-task-id".to_string(),
            priority: TaskPriority::Urgent,
        })
        .await;
    match res {
        OrchdResponse::Error { code, .. } => assert_eq!(code, OrchdErrorCode::NotFound),
        other => panic!("expected Error{{NotFound}}, got {other:?}"),
    }

    c1.shutdown(boot).await;
}

#[tokio::test]
async fn unknown_id_delete_task_is_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;

    let mut c1 = Client::connect(&socket).await;
    let res = c1
        .request(OrchdRequest::DeleteTask {
            id: "nonexistent-task-id".to_string(),
        })
        .await;
    match res {
        OrchdResponse::Error { code, .. } => assert_eq!(code, OrchdErrorCode::NotFound),
        other => panic!("expected Error{{NotFound}}, got {other:?}"),
    }

    c1.shutdown(boot).await;
}

// ---- S4 knowledge graph dispatch + `GraphChanged` push fan-out (spec §6, S4 task-4 brief) ----

#[tokio::test]
async fn graph_add_node_returns_node_and_broadcasts_graph_changed_to_its_project() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;

    let mut c1 = Client::connect(&socket).await;
    let project = create_project(&mut c1, "Proj").await;

    // c2 connects only AFTER the project (and its ProjectsChanged push) already landed, so the
    // only push it can observe below is the one this test is actually proving.
    let mut c2 = Client::connect(&socket).await;
    assert_eq!(c2.request(OrchdRequest::Ping).await, OrchdResponse::Pong);

    let node = expect_graph_node(
        c1.request(OrchdRequest::GraphAddNode {
            project_id: project.id.clone(),
            kind: GraphNodeKind::Concept,
            label: "Idea seed".to_string(),
            body: "body".to_string(),
            pos_x: 1.0,
            pos_y: 2.0,
        })
        .await,
    );
    assert_eq!(node.project_id, project.id);
    assert_eq!(node.label, "Idea seed");

    match c2.recv_push().await {
        OrchdPush::GraphChanged { project_id } => assert_eq!(project_id, project.id),
        other => panic!("expected GraphChanged, got {other:?}"),
    }
    assert!(
        c2.recv_push_timeout(Duration::from_millis(200))
            .await
            .is_none(),
        "a same-project GraphAddNode must broadcast exactly one GraphChanged"
    );

    c1.shutdown(boot).await;
}

#[tokio::test]
async fn graph_add_edge_cross_project_broadcasts_graph_changed_for_both_endpoint_projects() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;

    let mut c1 = Client::connect(&socket).await;
    let project_a = create_project(&mut c1, "A").await;
    let project_b = create_project(&mut c1, "B").await;
    let node_a = add_node(&mut c1, &project_a.id, "Node A").await;
    let node_b = add_node(&mut c1, &project_b.id, "Node B").await;

    let mut c2 = Client::connect(&socket).await;
    assert_eq!(c2.request(OrchdRequest::Ping).await, OrchdResponse::Pong);

    let edge = expect_graph_edge(
        c1.request(OrchdRequest::GraphAddEdge {
            source_node_id: node_a.id.clone(),
            target_node_id: node_b.id.clone(),
            kind: GraphEdgeKind::Relates,
            label: "cross".to_string(),
        })
        .await,
    );
    assert_eq!(edge.source_node_id, node_a.id);
    assert_eq!(edge.target_node_id, node_b.id);

    let seen = collect_pushes(&mut c2, 2).await;
    assert!(
        seen.contains(&OrchdPush::GraphChanged {
            project_id: project_a.id.clone()
        }),
        "missing GraphChanged for the source's project in {seen:?}"
    );
    assert!(
        seen.contains(&OrchdPush::GraphChanged {
            project_id: project_b.id.clone()
        }),
        "missing GraphChanged for the target's project in {seen:?}"
    );
    assert!(
        c2.recv_push_timeout(Duration::from_millis(200))
            .await
            .is_none(),
        "a cross-project GraphAddEdge must broadcast exactly two GraphChanged pushes"
    );

    c1.shutdown(boot).await;
}

#[tokio::test]
async fn graph_add_edge_same_project_broadcasts_exactly_one_graph_changed() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;

    let mut c1 = Client::connect(&socket).await;
    let project = create_project(&mut c1, "Proj").await;
    // Both endpoints in ONE project: `edge_endpoint_projects` returns (P, P), so the two ids are
    // identical and `broadcast_graph_changed`'s HashSet dedup must collapse them to a SINGLE push.
    let node_a = add_node(&mut c1, &project.id, "Node A").await;
    let node_b = add_node(&mut c1, &project.id, "Node B").await;

    let mut c2 = Client::connect(&socket).await;
    assert_eq!(c2.request(OrchdRequest::Ping).await, OrchdResponse::Pong);

    let edge = expect_graph_edge(
        c1.request(OrchdRequest::GraphAddEdge {
            source_node_id: node_a.id.clone(),
            target_node_id: node_b.id.clone(),
            kind: GraphEdgeKind::Relates,
            label: String::new(),
        })
        .await,
    );
    assert_eq!(edge.source_node_id, node_a.id);
    assert_eq!(edge.target_node_id, node_b.id);

    match c2.recv_push().await {
        OrchdPush::GraphChanged { project_id } => assert_eq!(project_id, project.id),
        other => panic!("expected GraphChanged, got {other:?}"),
    }
    // The dedup guard: a same-project edge must NOT emit a second (duplicate) GraphChanged.
    assert!(
        c2.recv_push_timeout(Duration::from_millis(300))
            .await
            .is_none(),
        "a same-project GraphAddEdge must broadcast exactly ONE GraphChanged, not two"
    );

    c1.shutdown(boot).await;
}

#[tokio::test]
async fn graph_update_edge_returns_edge_and_broadcasts_graph_changed_for_both_endpoint_projects() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;

    let mut c1 = Client::connect(&socket).await;
    let project_a = create_project(&mut c1, "A").await;
    let project_b = create_project(&mut c1, "B").await;
    let node_a = add_node(&mut c1, &project_a.id, "Node A").await;
    let node_b = add_node(&mut c1, &project_b.id, "Node B").await;
    // `add_edge` (test helper) creates a cross-project `Relates` edge.
    let edge = add_edge(&mut c1, &node_a.id, &node_b.id).await;

    let mut c2 = Client::connect(&socket).await;
    assert_eq!(c2.request(OrchdRequest::Ping).await, OrchdResponse::Pong);

    let updated = expect_graph_edge(
        c1.request(OrchdRequest::GraphUpdateEdge {
            id: edge.id.clone(),
            kind: GraphEdgeKind::Depends,
        })
        .await,
    );
    assert_eq!(updated.id, edge.id);
    assert_eq!(updated.kind, GraphEdgeKind::Depends);

    let seen = collect_pushes(&mut c2, 2).await;
    assert!(
        seen.contains(&OrchdPush::GraphChanged {
            project_id: project_a.id.clone()
        }),
        "missing GraphChanged for the source endpoint's project in {seen:?}"
    );
    assert!(
        seen.contains(&OrchdPush::GraphChanged {
            project_id: project_b.id.clone()
        }),
        "missing GraphChanged for the target endpoint's project in {seen:?}"
    );
    assert!(
        c2.recv_push_timeout(Duration::from_millis(200))
            .await
            .is_none(),
        "a cross-project GraphUpdateEdge must broadcast exactly two GraphChanged pushes"
    );

    c1.shutdown(boot).await;
}

#[tokio::test]
async fn graph_update_edge_unknown_id_is_not_found_and_broadcasts_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;

    let mut c1 = Client::connect(&socket).await;
    // A second observer proves a FAILED update broadcasts nothing (spec §6).
    let mut c2 = Client::connect(&socket).await;
    assert_eq!(c2.request(OrchdRequest::Ping).await, OrchdResponse::Pong);

    let res = c1
        .request(OrchdRequest::GraphUpdateEdge {
            id: "no-such-edge".to_string(),
            kind: GraphEdgeKind::Depends,
        })
        .await;
    match res {
        OrchdResponse::Error { code, .. } => assert_eq!(code, OrchdErrorCode::NotFound),
        other => panic!("expected NotFound Error, got {other:?}"),
    }
    assert!(
        c2.recv_push_timeout(Duration::from_millis(200))
            .await
            .is_none(),
        "a failed (NotFound) GraphUpdateEdge must broadcast nothing"
    );

    c1.shutdown(boot).await;
}

#[tokio::test]
async fn graph_delete_node_cross_project_broadcasts_graph_changed_for_foreign_project_too() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;

    let mut c1 = Client::connect(&socket).await;
    let project_a = create_project(&mut c1, "A").await;
    let project_b = create_project(&mut c1, "B").await;
    let node_a = add_node(&mut c1, &project_a.id, "Node A").await;
    let node_b = add_node(&mut c1, &project_b.id, "Node B").await;
    add_edge(&mut c1, &node_a.id, &node_b.id).await;

    let mut c2 = Client::connect(&socket).await;
    assert_eq!(c2.request(OrchdRequest::Ping).await, OrchdResponse::Pong);

    // Deleting node A (in project A) must invalidate project B too — B's `GraphListProject` view
    // shows A as an `external_nodes` ghost via the incident edge, and that ghost is about to
    // disappear along with the cascaded edge.
    expect_ack(
        c1.request(OrchdRequest::GraphDeleteNode {
            id: node_a.id.clone(),
        })
        .await,
    );

    let seen = collect_pushes(&mut c2, 2).await;
    assert!(
        seen.contains(&OrchdPush::GraphChanged {
            project_id: project_a.id.clone()
        }),
        "missing GraphChanged for the node's own project in {seen:?}"
    );
    assert!(
        seen.contains(&OrchdPush::GraphChanged {
            project_id: project_b.id.clone()
        }),
        "missing GraphChanged for the FOREIGN project (reachable via the cascaded edge) in {seen:?}"
    );
    assert!(
        c2.recv_push_timeout(Duration::from_millis(200))
            .await
            .is_none(),
        "expected exactly two GraphChanged pushes, got extra"
    );

    c1.shutdown(boot).await;
}

#[tokio::test]
async fn graph_update_node_and_move_node_cross_project_broadcast_foreign_project_too() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;

    let mut c1 = Client::connect(&socket).await;
    let project_a = create_project(&mut c1, "A").await;
    let project_b = create_project(&mut c1, "B").await;
    let node_a = add_node(&mut c1, &project_a.id, "Node A").await;
    let node_b = add_node(&mut c1, &project_b.id, "Node B").await;
    add_edge(&mut c1, &node_a.id, &node_b.id).await;

    let mut c2 = Client::connect(&socket).await;
    assert_eq!(c2.request(OrchdRequest::Ping).await, OrchdResponse::Pong);

    // GraphUpdateNode on node A (own project A) must also invalidate project B.
    let updated = expect_graph_node(
        c1.request(OrchdRequest::GraphUpdateNode {
            id: node_a.id.clone(),
            label: Some("Renamed A".to_string()),
            body: None,
        })
        .await,
    );
    assert_eq!(updated.label, "Renamed A");

    let seen_update = collect_pushes(&mut c2, 2).await;
    assert!(
        seen_update.contains(&OrchdPush::GraphChanged {
            project_id: project_a.id.clone()
        }),
        "missing GraphChanged for the node's own project after update in {seen_update:?}"
    );
    assert!(
        seen_update.contains(&OrchdPush::GraphChanged {
            project_id: project_b.id.clone()
        }),
        "missing GraphChanged for the FOREIGN project after update in {seen_update:?}"
    );
    // GraphUpdateNode must broadcast EXACTLY those two — a stray third push here would otherwise
    // leak forward into the MoveNode phase's `collect_pushes(&mut c2, 2)` below and let that
    // phase pass spuriously.
    assert!(
        c2.recv_push_timeout(Duration::from_millis(200))
            .await
            .is_none(),
        "a cross-project GraphUpdateNode must broadcast exactly two GraphChanged pushes"
    );

    // GraphMoveNode on the same cross-project node must ALSO invalidate project B.
    let moved = expect_graph_node(
        c1.request(OrchdRequest::GraphMoveNode {
            id: node_a.id.clone(),
            pos_x: 10.0,
            pos_y: 20.0,
        })
        .await,
    );
    assert_eq!(moved.pos_x, 10.0);
    assert_eq!(moved.pos_y, 20.0);

    let seen_move = collect_pushes(&mut c2, 2).await;
    assert!(
        seen_move.contains(&OrchdPush::GraphChanged {
            project_id: project_a.id.clone()
        }),
        "missing GraphChanged for the node's own project after move in {seen_move:?}"
    );
    assert!(
        seen_move.contains(&OrchdPush::GraphChanged {
            project_id: project_b.id.clone()
        }),
        "missing GraphChanged for the FOREIGN project after move in {seen_move:?}"
    );
    assert!(
        c2.recv_push_timeout(Duration::from_millis(200))
            .await
            .is_none(),
        "a cross-project GraphMoveNode must broadcast exactly two GraphChanged pushes"
    );

    c1.shutdown(boot).await;
}

#[tokio::test]
async fn graph_delete_edge_cross_project_broadcasts_graph_changed_for_both_endpoint_projects() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;

    let mut c1 = Client::connect(&socket).await;
    let project_a = create_project(&mut c1, "A").await;
    let project_b = create_project(&mut c1, "B").await;
    let node_a = add_node(&mut c1, &project_a.id, "Node A").await;
    let node_b = add_node(&mut c1, &project_b.id, "Node B").await;
    let edge = add_edge(&mut c1, &node_a.id, &node_b.id).await;

    let mut c2 = Client::connect(&socket).await;
    assert_eq!(c2.request(OrchdRequest::Ping).await, OrchdResponse::Pong);

    expect_ack(
        c1.request(OrchdRequest::GraphDeleteEdge {
            id: edge.id.clone(),
        })
        .await,
    );

    let seen = collect_pushes(&mut c2, 2).await;
    assert!(
        seen.contains(&OrchdPush::GraphChanged {
            project_id: project_a.id.clone()
        }),
        "missing GraphChanged for the source endpoint's project in {seen:?}"
    );
    assert!(
        seen.contains(&OrchdPush::GraphChanged {
            project_id: project_b.id.clone()
        }),
        "missing GraphChanged for the target endpoint's project in {seen:?}"
    );
    assert!(
        c2.recv_push_timeout(Duration::from_millis(200))
            .await
            .is_none(),
        "expected exactly two GraphChanged pushes, got extra"
    );

    c1.shutdown(boot).await;
}

#[tokio::test]
async fn graph_list_project_returns_view_and_broadcasts_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;

    let mut c1 = Client::connect(&socket).await;
    let project = create_project(&mut c1, "Proj").await;
    let node = add_node(&mut c1, &project.id, "Solo node").await;

    let mut c2 = Client::connect(&socket).await;
    assert_eq!(c2.request(OrchdRequest::Ping).await, OrchdResponse::Pong);

    let view = expect_graph_view(
        c1.request(OrchdRequest::GraphListProject {
            project_id: project.id.clone(),
        })
        .await,
    );
    assert!(view.nodes.iter().any(|n| n.id == node.id));
    assert!(view.external_nodes.is_empty());

    assert!(
        c2.recv_push_timeout(Duration::from_millis(300))
            .await
            .is_none(),
        "GraphListProject is a read verb and must broadcast nothing"
    );

    c1.shutdown(boot).await;
}

#[tokio::test]
async fn graph_add_edge_self_loop_is_invariant_and_broadcasts_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;

    let mut c1 = Client::connect(&socket).await;
    let project = create_project(&mut c1, "Proj").await;
    let node = add_node(&mut c1, &project.id, "Solo node").await;

    let mut c2 = Client::connect(&socket).await;
    assert_eq!(c2.request(OrchdRequest::Ping).await, OrchdResponse::Pong);

    let res = c1
        .request(OrchdRequest::GraphAddEdge {
            source_node_id: node.id.clone(),
            target_node_id: node.id.clone(),
            kind: GraphEdgeKind::Relates,
            label: String::new(),
        })
        .await;
    match res {
        OrchdResponse::Error { code, .. } => assert_eq!(code, OrchdErrorCode::Invariant),
        other => panic!("expected Error{{Invariant}}, got {other:?}"),
    }

    assert!(
        c2.recv_push_timeout(Duration::from_millis(300))
            .await
            .is_none(),
        "a failed (self-loop) GraphAddEdge must broadcast nothing"
    );

    c1.shutdown(boot).await;
}

#[tokio::test]
async fn graph_neighborhood_returns_correct_subgraph() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;

    let mut c1 = Client::connect(&socket).await;
    let project = create_project(&mut c1, "Proj").await;
    let node_a = add_node(&mut c1, &project.id, "A").await;
    let node_b = add_node(&mut c1, &project.id, "B").await;
    let node_c = add_node(&mut c1, &project.id, "C").await;
    let edge_ab = add_edge(&mut c1, &node_a.id, &node_b.id).await;
    add_edge(&mut c1, &node_b.id, &node_c.id).await;

    // A second listener, synchronized via Ping/Pong, so the "read verb broadcasts nothing"
    // assertion below has a registered client that WOULD observe any stray push.
    let mut c2 = Client::connect(&socket).await;
    assert_eq!(c2.request(OrchdRequest::Ping).await, OrchdResponse::Pong);

    // Depth 1 rooted at A: A is directly connected to B only (the A-B edge), NOT to C (that's
    // two hops away via B) — a genuine, non-trivial subgraph check.
    let neighborhood = expect_neighborhood(
        c1.request(OrchdRequest::GraphNeighborhood {
            node_id: node_a.id.clone(),
            depth: 1,
        })
        .await,
    );
    assert_eq!(neighborhood.root_id, node_a.id);
    let node_ids: std::collections::HashSet<String> =
        neighborhood.nodes.iter().map(|n| n.id.clone()).collect();
    assert_eq!(
        node_ids,
        std::collections::HashSet::from([node_a.id.clone(), node_b.id.clone()]),
        "depth-1 neighborhood of A must contain exactly {{A, B}}, not C"
    );
    assert_eq!(neighborhood.edges.len(), 1);
    assert_eq!(neighborhood.edges[0].id, edge_ab.id);

    assert!(
        c2.recv_push_timeout(Duration::from_millis(300))
            .await
            .is_none(),
        "GraphNeighborhood is a read verb and must broadcast nothing"
    );

    c1.shutdown(boot).await;
}

#[tokio::test]
async fn graph_search_returns_matching_nodes_workspace_wide_and_broadcasts_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;

    let mut c1 = Client::connect(&socket).await;
    let project_a = create_project(&mut c1, "A").await;
    let project_b = create_project(&mut c1, "B").await;
    let node_a = add_node(&mut c1, &project_a.id, "Widget Factory").await;
    let node_b = add_node(&mut c1, &project_b.id, "Gadget Factory").await;
    add_node(&mut c1, &project_a.id, "Unrelated").await;

    let mut c2 = Client::connect(&socket).await;
    assert_eq!(c2.request(OrchdRequest::Ping).await, OrchdResponse::Pong);

    let nodes = expect_graph_nodes(
        c1.request(OrchdRequest::GraphSearch {
            query: "Factory".to_string(),
            project_id: None,
        })
        .await,
    );
    let node_ids: std::collections::HashSet<String> = nodes.iter().map(|n| n.id.clone()).collect();
    assert!(node_ids.contains(&node_a.id));
    assert!(node_ids.contains(&node_b.id));

    assert!(
        c2.recv_push_timeout(Duration::from_millis(300))
            .await
            .is_none(),
        "GraphSearch is a read verb and must broadcast nothing"
    );

    c1.shutdown(boot).await;
}

// ---- S-EXT MCP dispatch (task T6) ----

#[tokio::test]
async fn mcp_add_server_returns_server_and_broadcasts_mcp_servers_changed() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;

    let mut c1 = Client::connect(&socket).await;
    let mut c2 = Client::connect(&socket).await;
    assert_eq!(c2.request(OrchdRequest::Ping).await, OrchdResponse::Pong);

    let stub_url = spawn_stub_mcp_server().await;
    let server = add_mcp_server(&mut c1, &stub_url).await;
    assert_eq!(server.url.as_deref(), Some(stub_url.as_str()));
    assert_eq!(server.transport, McpTransport::Http);
    assert_eq!(server.scope, McpScope::Global);
    assert!(server.enabled, "a freshly added server defaults to enabled");
    assert!(!server.id.is_empty());

    match c2.recv_push().await {
        OrchdPush::McpServersChanged { project_id } => {
            assert_eq!(
                project_id, None,
                "a global-scope server pushes project_id: None"
            )
        }
        other => panic!("expected McpServersChanged, got {other:?}"),
    }

    c1.shutdown(boot).await;
}

#[tokio::test]
async fn mcp_connect_without_consent_is_error_consent() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;

    let mut c1 = Client::connect(&socket).await;
    let stub_url = spawn_stub_mcp_server().await;
    let server = add_mcp_server(&mut c1, &stub_url).await;

    let res = c1
        .request(OrchdRequest::McpConnect {
            id: server.id.clone(),
        })
        .await;
    assert_eq!(
        expect_error_code(res),
        OrchdErrorCode::Consent,
        "a connect attempt with no consent_grant at all must deny with Error{{Consent}}"
    );

    // The trust choke-point denies BEFORE any network call — no tools were ever cached.
    let tools = expect_mcp_tools(
        c1.request(OrchdRequest::McpListTools {
            server_id: server.id.clone(),
        })
        .await,
    );
    assert!(
        tools.is_empty(),
        "a denied connect must never reach the stub server or cache tools"
    );

    c1.shutdown(boot).await;
}

#[tokio::test]
async fn mcp_connect_after_consent_returns_report_and_broadcasts_tools_changed_and_lists_echo() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;

    let mut c1 = Client::connect(&socket).await;
    let stub_url = spawn_stub_mcp_server().await;
    let server = add_mcp_server(&mut c1, &stub_url).await;

    expect_ack(
        c1.request(OrchdRequest::TrustGrantConsent {
            server_id: server.id.clone(),
            kind: "connect".to_string(),
        })
        .await,
    );

    // c2 connects only AFTER the server-add + consent-grant pushes already landed, so the only
    // push it can observe below is the McpToolsChanged this test is actually proving.
    let mut c2 = Client::connect(&socket).await;
    assert_eq!(c2.request(OrchdRequest::Ping).await, OrchdResponse::Pong);

    let report = expect_mcp_connect_report(
        c1.request(OrchdRequest::McpConnect {
            id: server.id.clone(),
        })
        .await,
    );
    assert!(
        report.tool_count >= 1,
        "the stub server advertises at least the echo tool"
    );
    assert!(!report.protocol_version.is_empty());

    match c2.recv_push().await {
        OrchdPush::McpToolsChanged { server_id } => assert_eq!(server_id, server.id),
        other => panic!("expected McpToolsChanged, got {other:?}"),
    }

    let tools = expect_mcp_tools(
        c1.request(OrchdRequest::McpListTools {
            server_id: server.id.clone(),
        })
        .await,
    );
    assert!(
        tools.iter().any(|t| t.name == "echo"),
        "expected the stub's echo tool in {tools:?}"
    );
    assert!(
        tools.iter().find(|t| t.name == "echo").unwrap().enabled,
        "a freshly cached tool defaults to enabled (the per-tool allowlist, spec §4)"
    );

    c1.shutdown(boot).await;
}

#[tokio::test]
async fn mcp_call_tool_echo_returns_result_and_broadcasts_artifact_and_invocation_pushes() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;

    let mut c1 = Client::connect(&socket).await;
    let stub_url = spawn_stub_mcp_server().await;
    let server = add_mcp_server(&mut c1, &stub_url).await;
    expect_ack(
        c1.request(OrchdRequest::TrustGrantConsent {
            server_id: server.id.clone(),
            kind: "connect".to_string(),
        })
        .await,
    );
    expect_mcp_connect_report(
        c1.request(OrchdRequest::McpConnect {
            id: server.id.clone(),
        })
        .await,
    );

    // c2 connects only after every setup push already landed, so it observes exactly the two
    // pushes this test is proving.
    let mut c2 = Client::connect(&socket).await;
    assert_eq!(c2.request(OrchdRequest::Ping).await, OrchdResponse::Pong);

    let args_json = serde_json::json!({"msg": "hello from T6"}).to_string();
    let result = expect_mcp_call_result(
        c1.request(OrchdRequest::McpCallTool {
            server_id: server.id.clone(),
            tool_name: "echo".to_string(),
            args_json,
            project_id: None,
        })
        .await,
    );
    assert!(!result.is_error, "echo is not a tool-level error");
    assert!(!result.artifact_id.is_empty());
    assert!(!result.invocation_id.is_empty());
    assert!(
        result.content_json.contains("hello from T6"),
        "expected the echoed message in {}",
        result.content_json
    );

    let seen = collect_pushes(&mut c2, 2).await;
    assert!(
        seen.contains(&OrchdPush::McpArtifactsChanged { project_id: None }),
        "missing McpArtifactsChanged in {seen:?}"
    );
    assert!(
        seen.contains(&OrchdPush::McpInvocationLogged {
            server_id: server.id.clone()
        }),
        "missing McpInvocationLogged in {seen:?}"
    );

    let artifacts = expect_mcp_artifacts(
        c1.request(OrchdRequest::McpListArtifacts {
            project_id: None,
            server_id: Some(server.id.clone()),
            limit: None,
        })
        .await,
    );
    assert_eq!(artifacts.len(), 1);
    assert_eq!(artifacts[0].id, result.artifact_id);
    assert!(artifacts[0].is_untrusted, "spec D9: always untrusted");

    c1.shutdown(boot).await;
}

#[tokio::test]
async fn mcp_call_tool_on_disabled_tool_is_error_policy_and_artifact_count_unchanged() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;

    let mut c1 = Client::connect(&socket).await;
    let stub_url = spawn_stub_mcp_server().await;
    let server = add_mcp_server(&mut c1, &stub_url).await;
    expect_ack(
        c1.request(OrchdRequest::TrustGrantConsent {
            server_id: server.id.clone(),
            kind: "connect".to_string(),
        })
        .await,
    );
    expect_mcp_connect_report(
        c1.request(OrchdRequest::McpConnect {
            id: server.id.clone(),
        })
        .await,
    );

    let tools = expect_mcp_tools(
        c1.request(OrchdRequest::McpListTools {
            server_id: server.id.clone(),
        })
        .await,
    );
    let echo_tool = tools
        .iter()
        .find(|t| t.name == "echo")
        .expect("echo tool must be cached after connect");

    let disabled = expect_mcp_tool(
        c1.request(OrchdRequest::McpSetToolEnabled {
            tool_id: echo_tool.id.clone(),
            enabled: false,
        })
        .await,
    );
    assert!(!disabled.enabled);

    let before = expect_mcp_artifacts(
        c1.request(OrchdRequest::McpListArtifacts {
            project_id: None,
            server_id: Some(server.id.clone()),
            limit: None,
        })
        .await,
    )
    .len();

    let res = c1
        .request(OrchdRequest::McpCallTool {
            server_id: server.id.clone(),
            tool_name: "echo".to_string(),
            args_json: "{}".to_string(),
            project_id: None,
        })
        .await;
    assert_eq!(
        expect_error_code(res),
        OrchdErrorCode::Policy,
        "a disabled tool must be denied with Error{{Policy}} before any dispatch"
    );

    let after = expect_mcp_artifacts(
        c1.request(OrchdRequest::McpListArtifacts {
            project_id: None,
            server_id: Some(server.id.clone()),
            limit: None,
        })
        .await,
    )
    .len();
    assert_eq!(before, after, "a denied call must not write a new artifact");

    c1.shutdown(boot).await;
}

/// SEC-3 (2026-07-24 audit remediation): the trust rate cap counted only COMPLETED invocations,
/// so a parallel burst all read "under the cap" and EVERY call dispatched (check-then-act — the
/// probe showed 5/5 succeeding at `ratePerMin=1`). The dispatch layer now holds a per-effective-
/// policy async mutex across authorize+dispatch, so the burst serializes: exactly ONE call wins
/// (its invocation row is written before the next attempt's check runs) and the other four are
/// denied `rate_limit_exceeded`. The stub's `slow_echo` keeps the winner's network phase open
/// long enough for all five to be genuinely in-flight at once.
#[tokio::test]
async fn mcp_call_tool_rate_cap_burst_of_five_at_cap_one_allows_exactly_one() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;

    let mut c1 = Client::connect(&socket).await;
    let stub_url = spawn_stub_mcp_server().await;
    let server = add_mcp_server(&mut c1, &stub_url).await;
    expect_ack(
        c1.request(OrchdRequest::TrustGrantConsent {
            server_id: server.id.clone(),
            kind: "connect".to_string(),
        })
        .await,
    );
    expect_mcp_connect_report(
        c1.request(OrchdRequest::McpConnect {
            id: server.id.clone(),
        })
        .await,
    );

    // Server-scope policy: at most ONE call per trailing minute on this server.
    match c1
        .request(OrchdRequest::TrustSetPolicy {
            scope: PolicyScope::Server,
            ref_id: Some(server.id.clone()),
            spend_cap_usd: None,
            rate_per_min: Some(1),
        })
        .await
    {
        OrchdResponse::Policy(_) => {}
        other => panic!("expected Policy, got {other:?}"),
    }

    // Five SEPARATE connections (one connection serializes its own dispatch loop) fire the slow
    // tool call simultaneously — all five are in-flight before the stub's first reply lands.
    let mut callers = Vec::new();
    let mut ids = Vec::new();
    for _ in 0..5 {
        let mut c = Client::connect(&socket).await;
        let id = c
            .send_request(OrchdRequest::McpCallTool {
                server_id: server.id.clone(),
                tool_name: "slow_echo".to_string(),
                args_json: serde_json::json!({"msg": "burst"}).to_string(),
                project_id: None,
            })
            .await;
        ids.push(id);
        callers.push(c);
    }

    let mut allowed = 0;
    let mut denied_rate = 0;
    let mut other = Vec::new();
    for (i, mut c) in callers.into_iter().enumerate() {
        match c.recv_response(ids[i]).await {
            OrchdResponse::McpCallResult(_) => allowed += 1,
            OrchdResponse::Error { code, message } => {
                assert_eq!(
                    code,
                    OrchdErrorCode::Policy,
                    "denials must be Error{{Policy}}"
                );
                assert!(
                    message.contains("rate_limit_exceeded"),
                    "denial must name the rate cap: {message}"
                );
                denied_rate += 1;
            }
            res => other.push(res),
        }
    }

    assert!(other.is_empty(), "unexpected responses: {other:?}");
    assert_eq!(allowed, 1, "exactly ONE burst call may pass the cap");
    assert_eq!(
        denied_rate, 4,
        "the other four must deny rate_limit_exceeded"
    );

    c1.shutdown(boot).await;
}
/// fire a `slow_echo` call (the stub sleeps [`SLOW_ECHO_DELAY`] server-side) on `c1` WITHOUT
/// awaiting it, then time a plain `ListProjects` DB read on `c2` — it must return in a small
/// fraction of `SLOW_ECHO_DELAY`, whereas it would block for the full delay if `McpCallTool` held
/// the lock across its network await. This is the concurrency regression guard for the removed
/// `unsafe` `Sync` bound on `Db`: the three-phase lock/read → network → lock/write restructure is
/// what makes both the DoS fix AND `Send`-without-the-unsafe hold.
#[tokio::test]
async fn mcp_call_tool_does_not_block_other_db_ops() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;

    let mut c1 = Client::connect(&socket).await;
    let stub_url = spawn_stub_mcp_server().await;
    let server = add_mcp_server(&mut c1, &stub_url).await;
    expect_ack(
        c1.request(OrchdRequest::TrustGrantConsent {
            server_id: server.id.clone(),
            kind: "connect".to_string(),
        })
        .await,
    );
    expect_mcp_connect_report(
        c1.request(OrchdRequest::McpConnect {
            id: server.id.clone(),
        })
        .await,
    );

    // c2 connects only after every setup push already landed, so its ListProjects below observes
    // no interleaved push and returns the response directly.
    let mut c2 = Client::connect(&socket).await;
    assert_eq!(c2.request(OrchdRequest::Ping).await, OrchdResponse::Pong);

    // Fire the slow tool call on c1 WITHOUT awaiting it: the stub sleeps SLOW_ECHO_DELAY before
    // replying, so c1's McpCallTool is now parked in its NETWORK phase — which must hold no DB lock.
    let slow_id = c1
        .send_request(OrchdRequest::McpCallTool {
            server_id: server.id.clone(),
            tool_name: "slow_echo".to_string(),
            args_json: serde_json::json!({"msg": "slow"}).to_string(),
            project_id: None,
        })
        .await;

    // While that call is mid-flight, a plain DB read on c2 must complete promptly (it acquires the
    // daemon DB mutex the slow call is NOT holding during its network sleep).
    let start = Instant::now();
    let projects = c2.request(OrchdRequest::ListProjects).await;
    let elapsed = start.elapsed();
    match projects {
        OrchdResponse::Projects(_) => {}
        other => panic!("expected Projects, got {other:?}"),
    }
    assert!(
        elapsed < SLOW_ECHO_DELAY / 2,
        "c2's ListProjects took {elapsed:?} — it must not block behind the in-flight slow \
         McpCallTool (stub sleeps {SLOW_ECHO_DELAY:?}); the DB lock must never be held across the \
         network await"
    );

    // Drain the slow call's eventual result so the connection is left clean before shutdown.
    let slow = expect_mcp_call_result(c1.recv_response(slow_id).await);
    assert!(!slow.is_error);
    assert!(
        slow.content_json.contains("slow"),
        "expected the echoed message in {}",
        slow.content_json
    );

    c1.shutdown(boot).await;
}

// ================================================================================
// ---- S-EXT whole-branch final-review fix: `McpDeleteServer` must delete the server's bearer
// Keychain entry (`bpa_secrets::mcp_bearer_ref(id)`, written by `McpSetServerBearer`) alongside the
// DB row — a credential-material leak otherwise, and asymmetric with `ConnectorDeleteAccount`
// (`connectors::accounts::Db::delete_account`, which already cleans up its own Keychain entries).
// Mirrors `connector_keychain_available`'s round-trip skip-guard rationale below (a precise 4-
// OSStatus-code probe is `#[cfg(test)]`-private to `bpa_secrets`), scoped to the MCP bearer
// service/ref shape specifically — the exact Keychain entry this test proves gets cleaned up.
// ================================================================================

fn mcp_keychain_available() -> bool {
    // FULL `set → get (assert bytes) → delete` round-trip, NOT set-only — see
    // `connector_keychain_available`'s own doc comment for why a set-only probe is insufficient.
    let probe = bpa_secrets::mcp_bearer_ref("dispatch-integration-mcp-probe");
    let _ = bpa_secrets::delete(&probe); // clear any stray entry from a crashed prior run
    const PROBE_BYTES: &[u8] = b"probe-roundtrip-marker";
    let skip = |reason: String| {
        eprintln!(
            "SKIP mcp dispatch keychain-backed test: {reason} — graceful skip, not a pass. Run \
             locally with an unlocked login keychain (or a CI keychain on the search list) to \
             exercise the full assertion."
        );
        let _ = bpa_secrets::delete(&probe);
        false
    };
    if let Err(e) = bpa_secrets::set(&probe, PROBE_BYTES) {
        return skip(format!("login keychain unavailable ({e})"));
    }
    match bpa_secrets::get(&probe) {
        Ok(bytes) if bytes == PROBE_BYTES => {}
        Ok(_) => return skip("probe get returned the wrong bytes (keychain misconfigured)".into()),
        Err(e) => {
            return skip(format!(
                "probe get failed after a successful set ({e} — keychain likely not on the search \
                 list)"
            ));
        }
    }
    if let Err(e) = bpa_secrets::delete(&probe) {
        return skip(format!(
            "probe delete failed after a successful set+get ({e})"
        ));
    }
    true
}

/// RAII Keychain cleanup for the bearer entry `McpSetServerBearer` writes mid-test, mirroring
/// `DeleteAccountSecretsOnDrop`'s rationale one section down: a panic between that write and the
/// explicit `McpDeleteServer` below must never leave a permanent orphan entry in the developer's/
/// CI runner's real login keychain (service `ai.builderpro.desktop.mcp`, account = server id).
/// Deleting an already-deleted ref is a harmless no-op — never conflicts with the explicit delete
/// this test itself exercises.
struct DeleteMcpBearerOnDrop {
    server_id: String,
}

impl Drop for DeleteMcpBearerOnDrop {
    fn drop(&mut self) {
        let _ = bpa_secrets::delete(&bpa_secrets::mcp_bearer_ref(&self.server_id));
    }
}

#[tokio::test]
async fn mcp_delete_server_deletes_the_bearer_keychain_entry_no_orphan() {
    if !mcp_keychain_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;

    let mut c1 = Client::connect(&socket).await;

    // A bearer-authed server: `McpAddServer` (http, `auth_kind` starts `None` — matches the wire's
    // real create flow) then `McpSetServerBearer` writes the token to Keychain AND flips
    // `auth_kind` to `Bearer` in the DB (see that dispatch arm's own doc comment). The url is never
    // dialed (no `McpConnect` in this test), so it doesn't need a live stub behind it.
    let server = expect_mcp_server(
        c1.request(OrchdRequest::McpAddServer {
            name: "Bearer Stub".to_string(),
            transport: McpTransport::Http,
            url: Some("http://127.0.0.1:9/mcp".to_string()),
            command: None,
            args: None,
            env: None,
            scope: McpScope::Global,
            project_id: None,
            auth_kind: McpAuthKind::None,
            timeout_ms: None,
            max_retries: None,
        })
        .await,
    );
    // RAII cleanup guard installed BEFORE the Keychain write below (mirrors
    // `DeleteAccountSecretsOnDrop`'s own placement rationale): a panic between the write and the
    // explicit `McpDeleteServer` at the end still must never leave a permanent orphan entry.
    let _bearer_cleanup = DeleteMcpBearerOnDrop {
        server_id: server.id.clone(),
    };

    expect_ack(
        c1.request(OrchdRequest::McpSetServerBearer {
            id: server.id.clone(),
            token: "bearer-token-must-not-orphan-7f2a".to_string(),
        })
        .await,
    );

    // Keychain genuinely holds the bearer now.
    let stored = bpa_secrets::get(&bpa_secrets::mcp_bearer_ref(&server.id)).unwrap();
    assert_eq!(stored, b"bearer-token-must-not-orphan-7f2a");

    expect_ack(
        c1.request(OrchdRequest::McpDeleteServer {
            id: server.id.clone(),
        })
        .await,
    );

    // The DB row is gone (existing coverage elsewhere) — what THIS test proves is that the
    // Keychain entry is gone too, closing the S-EXT final-review credential-leak finding:
    // `McpDeleteServer` deleted the DB row but left this entry orphaned in the real Keychain.
    let after_delete = bpa_secrets::get(&bpa_secrets::mcp_bearer_ref(&server.id));
    assert!(
        matches!(after_delete, Err(bpa_secrets::SecretError::NotFound)),
        "McpDeleteServer must remove the bearer Keychain entry too (asymmetric with \
         ConnectorDeleteAccount/delete_account, which already cleans up its own) — got \
         {after_delete:?}"
    );

    c1.shutdown(boot).await;
}

// ================================================================================
// ---- S-EXT Connector dispatch (task T13a, spec §5/§6/§7): real per-verb socket dispatch,
// replacing T10's temporary stub arm (`OrchdResponse::Error{code:Io, message:"connector dispatch
// not yet implemented"}` for all 7 `Connector*` verbs). Keychain-backed verbs
// (`ConnectorAddApiKey`/`ConnectorDeleteAccount`/`ConnectorInvoke`) use the same loose-but-honest
// skip-guard `connectors::accounts`'s own crate-internal test module documents (a precise 4-
// OSStatus-code probe is `#[cfg(test)]`-private to `bpa_secrets`) — SKIP, not silently pass, when
// the login keychain is unavailable in this environment.
// ================================================================================

fn connector_keychain_available() -> bool {
    // FULL `set → get (assert bytes) → delete` round-trip, NOT set-only: Keychain Services'
    // "default keychain" and "search list" are independent, so a keychain a `set` writes to is not
    // necessarily the one a `get`/`delete` resolves. A CI keychain created + set-default + unlocked
    // but NOT added to the search list makes `set` succeed while `get`/`delete` fail "not found" —
    // a set-only probe would report "available" and the real dispatch test's read-back would then
    // panic. The round-trip catches that and SKIPs loudly instead.
    let probe = bpa_secrets::account_ref("dispatch-integration-connector-probe", "test");
    let _ = bpa_secrets::delete(&probe); // clear any stray entry from a crashed prior run
    const PROBE_BYTES: &[u8] = b"probe-roundtrip-marker";
    let skip = |reason: String| {
        eprintln!(
            "SKIP connector dispatch test: {reason} — graceful skip, not a pass. Run locally with \
             an unlocked login keychain (or a CI keychain on the search list) to exercise the \
             full assertion."
        );
        let _ = bpa_secrets::delete(&probe);
        false
    };
    if let Err(e) = bpa_secrets::set(&probe, PROBE_BYTES) {
        return skip(format!("login keychain unavailable ({e})"));
    }
    match bpa_secrets::get(&probe) {
        Ok(bytes) if bytes == PROBE_BYTES => {}
        Ok(_) => return skip("probe get returned the wrong bytes (keychain misconfigured)".into()),
        Err(e) => {
            return skip(format!(
                "probe get failed after a successful set ({e} — keychain likely not on the search \
                 list)"
            ));
        }
    }
    if let Err(e) = bpa_secrets::delete(&probe) {
        return skip(format!(
            "probe delete failed after a successful set+get ({e})"
        ));
    }
    true
}

/// RAII Keychain cleanup for a connector account created mid-test (task T13a review). Fires
/// `bpa_secrets::delete` for the account's `apikey`/`token`/`refresh` refs on drop, so an
/// assertion panic BETWEEN the `ConnectorAddApiKey` that creates the real Keychain entry and the
/// explicit `ConnectorDeleteAccount` at the end never leaves a permanent orphan entry in the
/// developer's/CI runner's real login keychain (service `ai.builderpro.desktop.account`, account
/// `{uuid}:apikey`). Mirrors `connectors::accounts`'s own `DeleteAccountSecretsOnDrop` — which is
/// `#[cfg(test)]`-private to that crate, so unreachable from this separate integration-test crate;
/// a thin owned-id equivalent is duplicated here (owned `String`, since the guard must outlive the
/// `.await` points and live to the end of the test). Belt-and-suspenders: the tests still exercise
/// the real `ConnectorDeleteAccount` delete path explicitly; this guard only covers the panic
/// case. Deleting an already-deleted ref is a harmless no-op (`bpa_secrets::delete` returns
/// `NotFound`, ignored) — so this never conflicts with the explicit delete.
struct DeleteAccountSecretsOnDrop {
    account_id: String,
}

impl Drop for DeleteAccountSecretsOnDrop {
    fn drop(&mut self) {
        for kind in ["apikey", "token", "refresh"] {
            let _ = bpa_secrets::delete(&bpa_secrets::account_ref(&self.account_id, kind));
        }
    }
}

/// Spawns a minimal loopback REST stub for `ConnectorInvoke` dispatch tests: `GET /ok` replies
/// `200 {"ok": true}` unconditionally. This only needs to prove the WIRE verb round-trips end to
/// end (dispatch -> `connectors::adapter::invoke` -> a real HTTP call -> persisted artifact) —
/// `GenericRestAdapter`'s own request-shaping (bearer header, JSON body, error status mapping) is
/// already covered by `adapter.rs`'s own unit tests (task T12).
async fn spawn_connector_rest_stub() -> String {
    let router = axum::Router::new().route(
        "/ok",
        axum::routing::get(|| async { axum::Json(serde_json::json!({"ok": true})) }),
    );
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind loopback connector rest stub");
    let addr = listener.local_addr().expect("stub local_addr");
    tokio::spawn(async move {
        let _ = axum::serve(listener, router).await;
    });
    format!("http://{addr}")
}

#[tokio::test]
async fn connector_add_api_key_list_accounts_delete_account_dispatch() {
    if !connector_keychain_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;

    let mut c1 = Client::connect(&socket).await;
    let mut c2 = Client::connect(&socket).await;
    assert_eq!(c2.request(OrchdRequest::Ping).await, OrchdResponse::Pong);

    // ---- ConnectorAddApiKey -> Account, c2 observes ConnectorsChanged ----
    let account = expect_account(
        c1.request(OrchdRequest::ConnectorAddApiKey {
            provider: "generic-rest".to_string(),
            label: "My REST".to_string(),
            api_key: "test-api-key-dispatch-42".to_string(),
        })
        .await,
    );
    // RAII cleanup guard installed BEFORE any subsequent assertion (task T13a review): a panic
    // below must never leave the real Keychain entry orphaned. The explicit ConnectorDeleteAccount
    // at the end still tests the delete path; this is the panic-case backstop.
    let _account_cleanup = DeleteAccountSecretsOnDrop {
        account_id: account.id.clone(),
    };
    assert_eq!(account.provider, "generic-rest");
    assert_eq!(account.label, "My REST");
    assert_eq!(account.auth_kind, AccountAuthKind::Apikey);
    assert!(!account.id.is_empty());

    assert_eq!(
        c2.recv_push().await,
        OrchdPush::ConnectorsChanged,
        "ConnectorAddApiKey must broadcast ConnectorsChanged on success"
    );

    // ---- ConnectorListAccounts -> Accounts (contains the one just added), NO push ----
    let accounts = expect_accounts(c1.request(OrchdRequest::ConnectorListAccounts).await);
    assert!(
        accounts.iter().any(|a| a.id == account.id),
        "expected the just-added account in {accounts:?}"
    );
    assert_eq!(
        c2.recv_push_timeout(Duration::from_millis(200)).await,
        None,
        "a read verb (ConnectorListAccounts) must broadcast nothing"
    );

    // ---- ConnectorDeleteAccount -> Ack, c2 observes ConnectorsChanged ----
    expect_ack(
        c1.request(OrchdRequest::ConnectorDeleteAccount {
            id: account.id.clone(),
        })
        .await,
    );
    assert_eq!(
        c2.recv_push().await,
        OrchdPush::ConnectorsChanged,
        "ConnectorDeleteAccount must broadcast ConnectorsChanged on success"
    );

    let accounts_after = expect_accounts(c1.request(OrchdRequest::ConnectorListAccounts).await);
    assert!(
        !accounts_after.iter().any(|a| a.id == account.id),
        "deleted account must no longer be listed"
    );

    c1.shutdown(boot).await;
}

#[tokio::test]
async fn connector_invoke_against_rest_stub_returns_result_and_broadcasts_artifacts_changed() {
    if !connector_keychain_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;

    let mut c1 = Client::connect(&socket).await;
    let account = expect_account(
        c1.request(OrchdRequest::ConnectorAddApiKey {
            provider: "generic-rest".to_string(),
            label: "My REST".to_string(),
            api_key: "test-api-key-invoke-77".to_string(),
        })
        .await,
    );
    // RAII cleanup guard installed BEFORE any subsequent assertion (task T13a review) — see the
    // sibling test's comment; the explicit ConnectorDeleteAccount at the end still tests the
    // delete path, this backstops the panic case.
    let _account_cleanup = DeleteAccountSecretsOnDrop {
        account_id: account.id.clone(),
    };

    // c2 connects only after the account-add setup push already landed, so it observes exactly
    // the McpArtifactsChanged push this test is proving.
    let mut c2 = Client::connect(&socket).await;
    assert_eq!(c2.request(OrchdRequest::Ping).await, OrchdResponse::Pong);

    let stub_base = spawn_connector_rest_stub().await;
    let args_json = serde_json::json!({"url": format!("{stub_base}/ok")}).to_string();
    let result = expect_mcp_call_result(
        c1.request(OrchdRequest::ConnectorInvoke {
            account_id: account.id.clone(),
            op: "get".to_string(),
            args_json,
            project_id: None,
        })
        .await,
    );
    assert!(!result.is_error, "the stub answers 200, not a tool error");
    assert!(!result.artifact_id.is_empty());
    assert!(!result.invocation_id.is_empty());
    assert_eq!(
        result.content_json,
        serde_json::to_string(&serde_json::json!({"ok": true})).unwrap()
    );

    assert_eq!(
        c2.recv_push().await,
        OrchdPush::McpArtifactsChanged { project_id: None },
        "ConnectorInvoke must broadcast McpArtifactsChanged on success (reuses the MCP artifact \
         persistence path, spec §6/D9)"
    );

    let artifacts = expect_mcp_artifacts(
        c1.request(OrchdRequest::McpListArtifacts {
            project_id: None,
            server_id: None,
            limit: None,
        })
        .await,
    );
    let artifact = artifacts
        .iter()
        .find(|a| a.id == result.artifact_id)
        .unwrap_or_else(|| panic!("expected artifact {} in {artifacts:?}", result.artifact_id));
    assert!(artifact.is_untrusted, "spec D9: always untrusted");
    assert_eq!(artifact.account_id.as_deref(), Some(account.id.as_str()));
    assert_eq!(artifact.server_id, None);

    expect_ack(
        c1.request(OrchdRequest::ConnectorDeleteAccount {
            id: account.id.clone(),
        })
        .await,
    );

    c1.shutdown(boot).await;
}

#[tokio::test]
async fn connector_list_ops_returns_the_generic_rest_adapter_ops() {
    if !connector_keychain_available() {
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;
    let mut c1 = Client::connect(&socket).await;

    // ConnectorListOps resolves the account's provider adapter, so it needs a real account row —
    // an api-key `generic-rest` account is the cheapest (no OAuth flow, no provider registry).
    let account = expect_account(
        c1.request(OrchdRequest::ConnectorAddApiKey {
            provider: "generic-rest".to_string(),
            label: "My REST".to_string(),
            api_key: "test-api-key-listops-13".to_string(),
        })
        .await,
    );
    let _account_cleanup = DeleteAccountSecretsOnDrop {
        account_id: account.id.clone(),
    };

    // ---- ConnectorListOps -> ConnectorOps (the generic-rest adapter's get/post ops), NO push ----
    let ops = expect_connector_ops(
        c1.request(OrchdRequest::ConnectorListOps {
            account_id: account.id.clone(),
        })
        .await,
    );
    let op_names: Vec<&str> = ops.iter().map(|o| o.name.as_str()).collect();
    assert!(
        op_names.contains(&"get") && op_names.contains(&"post"),
        "generic-rest adapter must expose get/post ops, got {op_names:?}"
    );

    expect_ack(
        c1.request(OrchdRequest::ConnectorDeleteAccount {
            id: account.id.clone(),
        })
        .await,
    );

    c1.shutdown(boot).await;
}

#[tokio::test]
async fn connector_begin_oauth_unregistered_provider_is_error_no_pending_challenge() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;
    let mut c1 = Client::connect(&socket).await;

    // v1 boots with an EMPTY OAuth provider registry (spec §10: no real IdP creds ship with the
    // app) — `ConnectorBeginOAuth` for a provider nobody registered must fail honestly rather than
    // fabricate a challenge.
    let res = c1
        .request(OrchdRequest::ConnectorBeginOAuth {
            provider: "prowl".to_string(),
            label: "My Prowl".to_string(),
            scopes: None,
            server_id: None,
        })
        .await;
    assert_eq!(
        expect_error_code(res),
        OrchdErrorCode::Io,
        "ConnectorError has no dedicated wire code yet (mirrors map_secret_err's Sql->Io \
         precedent) — still a genuine Error response, never a fabricated OAuthChallenge"
    );

    c1.shutdown(boot).await;
}

#[tokio::test]
async fn connector_list_providers_returns_empty_when_no_config_file() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    // No oauth_providers.json written — the honest v1 default (spec D7): an empty registry, and a
    // read that returns an empty list rather than an error.
    let boot = boot_daemon(&socket).await;
    let mut c1 = Client::connect(&socket).await;

    let providers =
        expect_connector_providers(c1.request(OrchdRequest::ConnectorListProviders).await);
    assert!(
        providers.is_empty(),
        "with no oauth_providers.json the registry must be empty, got {providers:?}"
    );

    c1.shutdown(boot).await;
}

#[tokio::test]
async fn connector_list_providers_returns_names_from_config_file_no_secrets() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    // Plant an oauth_providers.json (spec D7 shape) in the app-support dir BEFORE boot — the loader
    // runs in `boot::run`. One provider carries a confidential `client_secret` to prove it never
    // rides back on the names-only `ConnectorListProviders` response.
    let app_support = home_dir
        .path()
        .join("Library/Application Support/ai.builderpro.desktop");
    std::fs::create_dir_all(&app_support).unwrap();
    std::fs::write(
        app_support.join("oauth_providers.json"),
        r#"{
          "prowl": {
            "client_id": "prowl-client-id",
            "auth_url": "https://prowl.chat/oauth/authorize",
            "token_url": "https://prowl.chat/oauth/token",
            "default_scopes": ["read"],
            "client_secret": "prowl-CLIENT-SECRET-must-not-ride-the-wire"
          },
          "github": {
            "client_id": "gh-client-id",
            "auth_url": "https://github.com/login/oauth/authorize",
            "token_url": "https://github.com/login/oauth/access_token"
          }
        }"#,
    )
    .unwrap();

    let boot = boot_daemon(&socket).await;
    let mut c1 = Client::connect(&socket).await;

    let providers =
        expect_connector_providers(c1.request(OrchdRequest::ConnectorListProviders).await);
    assert_eq!(
        providers,
        vec!["github".to_string(), "prowl".to_string()],
        "ConnectorListProviders must return the configured provider names, sorted"
    );

    // Names ONLY on the wire (spec D7): no client_id, no client_secret, no endpoint URLs.
    let joined = providers.join(",");
    assert!(
        !joined.contains("SECRET")
            && !joined.contains("client_id")
            && !joined.contains("prowl.chat"),
        "no client id/secret/URL may cross the wire, got {providers:?}"
    );

    // A provider from the config is now genuinely reachable: begin_oauth against it succeeds
    // (proving the full config — not just the name — was registered).
    let challenge = expect_oauth_challenge(
        c1.request(OrchdRequest::ConnectorBeginOAuth {
            provider: "github".to_string(),
            label: "My GitHub".to_string(),
            scopes: None,
            server_id: None,
        })
        .await,
    );
    assert!(challenge
        .authorize_url
        .contains("github.com/login/oauth/authorize"));

    c1.shutdown(boot).await;
}

// ---- S-EXT Skills dispatch (task T17) ----

/// Writes a minimal SKILL.md (full frontmatter: `name`+`description`) under a fresh tempdir and
/// returns `(TempDir, absolute path)` — the `TempDir` guard must outlive the path's use (mirrors
/// `skills::registry::tests::write_skill_md`, one layer up at the socket/dispatch level).
fn write_skill_md() -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("SKILL.md");
    std::fs::write(
        &path,
        "---\nname: Dispatch Skill\ndescription: registered over the wire\n---\n\nBody.\n",
    )
    .unwrap();
    (dir, path.to_string_lossy().to_string())
}

#[tokio::test]
async fn skill_add_returns_skill_and_broadcasts_skills_changed() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;

    let mut c1 = Client::connect(&socket).await;
    let mut c2 = Client::connect(&socket).await;
    assert_eq!(c2.request(OrchdRequest::Ping).await, OrchdResponse::Pong);

    let (_skill_guard, md_path) = write_skill_md();

    let skill = expect_skill(
        c1.request(OrchdRequest::SkillAdd {
            name: None,
            description: None,
            md_path: md_path.clone(),
            scope: SkillScope::Global,
            project_id: None,
        })
        .await,
    );
    // name/description omitted on the wire -> parsed from the SKILL.md frontmatter (Q14).
    assert_eq!(skill.name, "Dispatch Skill");
    assert_eq!(skill.description, "registered over the wire");
    assert_eq!(skill.scope, SkillScope::Global);
    assert_eq!(skill.project_id, None);
    assert_eq!(
        skill.file_state,
        SkillFileState::Present,
        "freshly added -> the stored hash matches the file that was just read"
    );
    assert!(!skill.id.is_empty());
    assert!(!skill.md_hash.is_empty());

    match c2.recv_push().await {
        OrchdPush::SkillsChanged { project_id } => {
            assert_eq!(
                project_id, None,
                "a global-scope skill pushes project_id: None"
            )
        }
        other => panic!("expected SkillsChanged, got {other:?}"),
    }

    c1.shutdown(boot).await;
}

#[tokio::test]
async fn skill_add_no_name_and_no_frontmatter_is_error_validation_and_broadcasts_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;

    let mut c1 = Client::connect(&socket).await;
    let mut c2 = Client::connect(&socket).await;
    assert_eq!(c2.request(OrchdRequest::Ping).await, OrchdResponse::Pong);

    let skill_dir = tempfile::tempdir().unwrap();
    let md_path = skill_dir.path().join("SKILL.md");
    std::fs::write(&md_path, "# No frontmatter here\n").unwrap();

    let res = c1
        .request(OrchdRequest::SkillAdd {
            name: None,
            description: None,
            md_path: md_path.to_string_lossy().to_string(),
            scope: SkillScope::Global,
            project_id: None,
        })
        .await;
    assert_eq!(expect_error_code(res), OrchdErrorCode::Validation);

    assert_eq!(
        c2.recv_push_timeout(Duration::from_millis(200)).await,
        None,
        "a failed SkillAdd must broadcast nothing (spec §6)"
    );

    c1.shutdown(boot).await;
}

#[tokio::test]
async fn skill_list_returns_skills_and_broadcasts_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;

    let mut c1 = Client::connect(&socket).await;
    let mut c2 = Client::connect(&socket).await;
    assert_eq!(c2.request(OrchdRequest::Ping).await, OrchdResponse::Pong);

    let (_skill_guard, md_path) = write_skill_md();
    let added = expect_skill(
        c1.request(OrchdRequest::SkillAdd {
            name: None,
            description: None,
            md_path,
            scope: SkillScope::Global,
            project_id: None,
        })
        .await,
    );
    // Drain the SkillAdd's own SkillsChanged push before the read-verb assertion below.
    c2.recv_push().await;

    let skills = expect_skills(
        c1.request(OrchdRequest::SkillList { project_id: None })
            .await,
    );
    assert!(
        skills.iter().any(|s| s.id == added.id),
        "expected the just-added skill in {skills:?}"
    );

    assert_eq!(
        c2.recv_push_timeout(Duration::from_millis(200)).await,
        None,
        "a read verb (SkillList) must broadcast nothing"
    );

    c1.shutdown(boot).await;
}

#[tokio::test]
async fn skill_delete_returns_ack_and_broadcasts_skills_changed() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;

    let mut c1 = Client::connect(&socket).await;
    let mut c2 = Client::connect(&socket).await;
    assert_eq!(c2.request(OrchdRequest::Ping).await, OrchdResponse::Pong);

    let (_skill_guard, md_path) = write_skill_md();
    let added = expect_skill(
        c1.request(OrchdRequest::SkillAdd {
            name: None,
            description: None,
            md_path,
            scope: SkillScope::Global,
            project_id: None,
        })
        .await,
    );
    c2.recv_push().await; // drain the SkillAdd push

    expect_ack(
        c1.request(OrchdRequest::SkillDelete {
            id: added.id.clone(),
        })
        .await,
    );
    match c2.recv_push().await {
        OrchdPush::SkillsChanged { project_id } => assert_eq!(project_id, None),
        other => panic!("expected SkillsChanged, got {other:?}"),
    }

    let skills = expect_skills(
        c1.request(OrchdRequest::SkillList { project_id: None })
            .await,
    );
    assert!(
        !skills.iter().any(|s| s.id == added.id),
        "deleted skill must no longer be listed"
    );

    c1.shutdown(boot).await;
}

#[tokio::test]
async fn skill_delete_unknown_id_is_error_not_found_and_broadcasts_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;

    let mut c1 = Client::connect(&socket).await;
    let mut c2 = Client::connect(&socket).await;
    assert_eq!(c2.request(OrchdRequest::Ping).await, OrchdResponse::Pong);

    let res = c1
        .request(OrchdRequest::SkillDelete {
            id: "missing".to_string(),
        })
        .await;
    assert_eq!(expect_error_code(res), OrchdErrorCode::NotFound);

    assert_eq!(
        c2.recv_push_timeout(Duration::from_millis(200)).await,
        None,
        "a failed SkillDelete must broadcast nothing (spec §6)"
    );

    c1.shutdown(boot).await;
}

// ---- S-EXT Trust: policy caps + audit log dispatch (task T18, spec §4/§5/§6, BL-22) ----

#[tokio::test]
async fn trust_set_policy_returns_policy_and_broadcasts_policies_changed() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;

    let mut c1 = Client::connect(&socket).await;
    let mut c2 = Client::connect(&socket).await;
    assert_eq!(c2.request(OrchdRequest::Ping).await, OrchdResponse::Pong);

    let policy = expect_policy(
        c1.request(OrchdRequest::TrustSetPolicy {
            scope: PolicyScope::Global,
            ref_id: None,
            spend_cap_usd: Some(10.0),
            rate_per_min: Some(30),
        })
        .await,
    );
    assert_eq!(policy.scope, PolicyScope::Global);
    assert_eq!(policy.ref_id, None);
    assert_eq!(policy.spend_cap_usd, Some(10.0));
    assert_eq!(policy.rate_per_min, Some(30));
    assert!(!policy.id.is_empty());

    match c2.recv_push().await {
        OrchdPush::PoliciesChanged => {}
        other => panic!("expected PoliciesChanged, got {other:?}"),
    }

    c1.shutdown(boot).await;
}

#[tokio::test]
async fn trust_set_policy_upserts_in_place_on_re_set_for_the_same_scope() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;

    let mut c1 = Client::connect(&socket).await;
    let mut c2 = Client::connect(&socket).await;
    assert_eq!(c2.request(OrchdRequest::Ping).await, OrchdResponse::Pong);

    let server = add_mcp_server(&mut c1, "http://127.0.0.1:0/mcp").await;
    c2.recv_push().await; // drain McpAddServer's own McpServersChanged

    let first = expect_policy(
        c1.request(OrchdRequest::TrustSetPolicy {
            scope: PolicyScope::Server,
            ref_id: Some(server.id.clone()),
            spend_cap_usd: Some(5.0),
            rate_per_min: None,
        })
        .await,
    );
    c2.recv_push().await; // drain the first PoliciesChanged

    let second = expect_policy(
        c1.request(OrchdRequest::TrustSetPolicy {
            scope: PolicyScope::Server,
            ref_id: Some(server.id.clone()),
            spend_cap_usd: Some(50.0),
            rate_per_min: Some(10),
        })
        .await,
    );
    match c2.recv_push().await {
        OrchdPush::PoliciesChanged => {}
        other => panic!("expected PoliciesChanged, got {other:?}"),
    }

    assert_eq!(
        second.id, first.id,
        "re-setting the same (scope, refId) must UPDATE the existing row, not insert a second"
    );
    assert_eq!(second.spend_cap_usd, Some(50.0));
    assert_eq!(second.rate_per_min, Some(10));

    let all = expect_policies(c1.request(OrchdRequest::TrustListPolicies).await);
    assert_eq!(
        all.iter().filter(|p| p.id == first.id).count(),
        1,
        "must never end up as two rows"
    );

    c1.shutdown(boot).await;
}

#[tokio::test]
async fn trust_set_policy_invalid_scope_ref_id_pairing_is_error_validation_and_broadcasts_nothing()
{
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;

    let mut c1 = Client::connect(&socket).await;
    let mut c2 = Client::connect(&socket).await;
    assert_eq!(c2.request(OrchdRequest::Ping).await, OrchdResponse::Pong);

    // Global scope with a ref_id set is an invalid pairing (spec §4: null for global).
    let res = c1
        .request(OrchdRequest::TrustSetPolicy {
            scope: PolicyScope::Global,
            ref_id: Some("proj-1".to_string()),
            spend_cap_usd: None,
            rate_per_min: None,
        })
        .await;
    assert_eq!(expect_error_code(res), OrchdErrorCode::Validation);

    assert_eq!(
        c2.recv_push_timeout(Duration::from_millis(200)).await,
        None,
        "a failed TrustSetPolicy must broadcast nothing (spec §6)"
    );

    c1.shutdown(boot).await;
}

#[tokio::test]
async fn trust_list_policies_returns_configured_policies_and_broadcasts_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;

    let mut c1 = Client::connect(&socket).await;
    let mut c2 = Client::connect(&socket).await;
    assert_eq!(c2.request(OrchdRequest::Ping).await, OrchdResponse::Pong);

    // A REAL project is required for a project-scope policy (DOM-6: `policy.ref_id` is now
    // existence-checked — a dangling project ref is Error{NotFound}).
    let project = expect_project(
        c1.request(OrchdRequest::CreateProject {
            name: "PolicyProj".to_string(),
            description: String::new(),
            workspace_ids: vec!["w1".to_string()],
        })
        .await,
    );

    let created = expect_policy(
        c1.request(OrchdRequest::TrustSetPolicy {
            scope: PolicyScope::Project,
            ref_id: Some(project.id.clone()),
            spend_cap_usd: Some(2.5),
            rate_per_min: Some(4),
        })
        .await,
    );
    // Drain the setup pushes: CreateProject's ProjectsChanged AND TrustSetPolicy's own
    // PoliciesChanged, so the read verb below is the only thing that could push next.
    c2.recv_push().await;
    c2.recv_push().await;

    let policies = expect_policies(c1.request(OrchdRequest::TrustListPolicies).await);
    assert!(
        policies.iter().any(|p| p.id == created.id),
        "expected the just-set policy in {policies:?}"
    );

    assert_eq!(
        c2.recv_push_timeout(Duration::from_millis(200)).await,
        None,
        "a read verb (TrustListPolicies) must broadcast nothing"
    );

    c1.shutdown(boot).await;
}

#[tokio::test]
async fn trust_list_audit_returns_rows_newest_first_and_broadcasts_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;

    let mut c1 = Client::connect(&socket).await;
    let mut c2 = Client::connect(&socket).await;
    assert_eq!(c2.request(OrchdRequest::Ping).await, OrchdResponse::Pong);

    // An UN-consented McpConnect denies at the trust gate BEFORE any network I/O (spec D10) —
    // the cheapest real way to produce a genuine `audit_log` row without a live stub MCP server.
    let server = add_mcp_server(&mut c1, "http://127.0.0.1:0/mcp").await;
    c2.recv_push().await; // drain McpAddServer's own McpServersChanged

    let res = c1
        .request(OrchdRequest::McpConnect {
            id: server.id.clone(),
        })
        .await;
    assert_eq!(expect_error_code(res), OrchdErrorCode::Consent);

    let rows = expect_audit_rows(
        c1.request(OrchdRequest::TrustListAudit { limit: None })
            .await,
    );
    let row = rows
        .iter()
        .find(|r| r.server_id.as_deref() == Some(server.id.as_str()))
        .expect("expected an audit row for the denied McpConnect");
    assert_eq!(row.action, "connect");
    assert_eq!(row.decision, "deny");
    assert_eq!(row.reason.as_deref(), Some("consent_required"));

    // `limit` caps the result.
    let capped = expect_audit_rows(
        c1.request(OrchdRequest::TrustListAudit { limit: Some(1) })
            .await,
    );
    assert_eq!(capped.len(), 1);

    assert_eq!(
        c2.recv_push_timeout(Duration::from_millis(200)).await,
        None,
        "a read verb (TrustListAudit) must broadcast nothing"
    );

    c1.shutdown(boot).await;
}

// ================================================================================
// ---- S-IDEA research dispatch (spec §5/§6, task T5): real per-verb socket dispatch against the
// SAME loopback stub MCP server the S-EXT MCP dispatch tests above use (`spawn_stub_mcp_server`,
// `EchoServer`'s `echo`/`slow_echo` tools) — the research verbs are, under the hood, a thin
// provenance wrapper around the exact `mcp::invoke::call_tool` path those tests already exercise,
// so reusing the same stub proves the real production wiring end to end (a research run against a
// REAL (loopback) MCP round-trip, not a fake `connect_fn` — the driver-level fakes already live in
// `research::mod::driver_tests`, task T4).
// ================================================================================

/// Bounded poll of `ResearchGetRun{id}` until its status leaves `pending`/`running` (i.e. reaches
/// a terminal state) or the deadline elapses. The stub's `echo` tool answers essentially
/// instantly (an in-process loopback HTTP round-trip), so a short bound with small sleeps is
/// hermetic — no environment-fragile wall-clock assertion (mirrors the S4 lesson the design spec's
/// own §8 testing-strategy section cites).
async fn poll_research_run_terminal(c: &mut Client, id: &str) -> ResearchRun {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let run = expect_research_run(
            c.request(OrchdRequest::ResearchGetRun { id: id.to_string() })
                .await,
        );
        if !matches!(
            run.status,
            ResearchStatus::Pending | ResearchStatus::Running
        ) {
            return run;
        }
        assert!(
            Instant::now() < deadline,
            "research run {id} did not reach a terminal state within the bound"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
}

#[tokio::test]
async fn research_start_run_returns_pending_row_then_reaches_done_via_pushes_and_get_run_poll() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;

    let mut c1 = Client::connect(&socket).await;
    let stub_url = spawn_stub_mcp_server().await;
    let server = add_mcp_server(&mut c1, &stub_url).await;
    expect_ack(
        c1.request(OrchdRequest::TrustGrantConsent {
            server_id: server.id.clone(),
            kind: "connect".to_string(),
        })
        .await,
    );
    expect_mcp_connect_report(
        c1.request(OrchdRequest::McpConnect {
            id: server.id.clone(),
        })
        .await,
    );
    let idea = create_idea(&mut c1, "Research this").await;

    // c2 connects only after every setup push already landed, so the pushes it observes below
    // are exactly the ones ResearchStartRun + its driver produce (not `McpServersChanged`/
    // `McpToolsChanged`/`IdeasChanged` from the setup steps above): the lifecycle flip's
    // `IdeasChanged` (DOM-9b), then the driver's `ResearchRunsChanged` transitions.
    let mut c2 = Client::connect(&socket).await;
    assert_eq!(c2.request(OrchdRequest::Ping).await, OrchdResponse::Pong);

    let args_json = serde_json::json!({"msg": "hello research"}).to_string();
    let run = expect_research_run(
        c1.request(OrchdRequest::ResearchStartRun {
            idea_id: idea.id.clone(),
            server_id: server.id.clone(),
            tool_name: "echo".to_string(),
            args_json: args_json.clone(),
        })
        .await,
    );
    assert_eq!(
        run.status,
        ResearchStatus::Pending,
        "ResearchStartRun's own reply is the freshly-inserted pending row, never the terminal one"
    );
    assert_eq!(run.idea_id, idea.id);
    assert_eq!(run.server_id, server.id);
    assert_eq!(run.tool_name, "echo");
    assert_eq!(run.args_json, args_json);
    assert!(run.invocation_id.is_none());
    assert!(run.artifact_id.is_none());
    assert!(run.error_kind.is_none());

    // DOM-9b (2026-07-24 audit remediation): the lifecycle flip (`captured`→`researching`) now
    // broadcasts `IdeasChanged` FIRST (synchronously, inside `start_run`, before the reply
    // above) — then the spawned driver pushes `ResearchRunsChanged{idea_id}` on EVERY transition
    // it drives (pending->running, then running->done).
    match c2
        .recv_push_timeout(Duration::from_secs(2))
        .await
        .expect("expected the IdeasChanged push from the lifecycle flip")
    {
        OrchdPush::IdeasChanged => {}
        other => panic!("expected IdeasChanged, got {other:?}"),
    }
    match c2
        .recv_push_timeout(Duration::from_secs(2))
        .await
        .expect("expected at least one ResearchRunsChanged push")
    {
        OrchdPush::ResearchRunsChanged { idea_id } => {
            assert_eq!(idea_id.as_deref(), Some(idea.id.as_str()))
        }
        other => panic!("expected ResearchRunsChanged, got {other:?}"),
    }

    let done = poll_research_run_terminal(&mut c1, &run.id).await;
    assert_eq!(done.status, ResearchStatus::Done);
    assert!(
        done.artifact_id.is_some(),
        "a done run must carry an artifactId"
    );
    assert!(
        done.invocation_id.is_some(),
        "a done run must carry an invocationId"
    );
    assert!(done.error_kind.is_none());

    c1.shutdown(boot).await;
}

#[tokio::test]
async fn research_list_runs_returns_the_ideas_runs_and_a_plain_read_broadcasts_nothing() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;

    let mut c1 = Client::connect(&socket).await;
    let stub_url = spawn_stub_mcp_server().await;
    let server = add_mcp_server(&mut c1, &stub_url).await;
    expect_ack(
        c1.request(OrchdRequest::TrustGrantConsent {
            server_id: server.id.clone(),
            kind: "connect".to_string(),
        })
        .await,
    );
    expect_mcp_connect_report(
        c1.request(OrchdRequest::McpConnect {
            id: server.id.clone(),
        })
        .await,
    );
    let idea = create_idea(&mut c1, "List this").await;

    let args_json = serde_json::json!({"msg": "list me"}).to_string();
    let run = expect_research_run(
        c1.request(OrchdRequest::ResearchStartRun {
            idea_id: idea.id.clone(),
            server_id: server.id.clone(),
            tool_name: "echo".to_string(),
            args_json,
        })
        .await,
    );
    // Let the run reach its terminal state (on c1) BEFORE c2 connects, so the read-broadcasts-
    // nothing assertion below isn't racing the driver's own `done` push.
    let done = poll_research_run_terminal(&mut c1, &run.id).await;
    assert_eq!(done.status, ResearchStatus::Done);

    let mut c2 = Client::connect(&socket).await;
    assert_eq!(c2.request(OrchdRequest::Ping).await, OrchdResponse::Pong);

    let runs = expect_research_runs(
        c1.request(OrchdRequest::ResearchListRuns {
            idea_id: idea.id.clone(),
        })
        .await,
    );
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].id, run.id);
    assert_eq!(runs[0].status, ResearchStatus::Done);

    let refetched = expect_research_run(
        c1.request(OrchdRequest::ResearchGetRun { id: run.id.clone() })
            .await,
    );
    assert_eq!(refetched.status, ResearchStatus::Done);

    assert_eq!(
        c2.recv_push_timeout(Duration::from_millis(200)).await,
        None,
        "ResearchListRuns/ResearchGetRun are plain reads — they must broadcast nothing"
    );

    c1.shutdown(boot).await;
}

#[tokio::test]
async fn research_get_run_unknown_id_is_error_not_found() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;

    let mut c1 = Client::connect(&socket).await;
    let res = c1
        .request(OrchdRequest::ResearchGetRun {
            id: "no-such-run".to_string(),
        })
        .await;
    assert_eq!(expect_error_code(res), OrchdErrorCode::NotFound);

    c1.shutdown(boot).await;
}

#[tokio::test]
async fn research_start_run_against_a_disabled_tool_reaches_failed_with_error_kind() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;

    let mut c1 = Client::connect(&socket).await;
    let stub_url = spawn_stub_mcp_server().await;
    let server = add_mcp_server(&mut c1, &stub_url).await;
    expect_ack(
        c1.request(OrchdRequest::TrustGrantConsent {
            server_id: server.id.clone(),
            kind: "connect".to_string(),
        })
        .await,
    );
    expect_mcp_connect_report(
        c1.request(OrchdRequest::McpConnect {
            id: server.id.clone(),
        })
        .await,
    );
    let idea = create_idea(&mut c1, "Fail this").await;

    let tools = expect_mcp_tools(
        c1.request(OrchdRequest::McpListTools {
            server_id: server.id.clone(),
        })
        .await,
    );
    let echo_tool = tools
        .iter()
        .find(|t| t.name == "echo")
        .expect("echo tool must be cached after connect");
    let disabled = expect_mcp_tool(
        c1.request(OrchdRequest::McpSetToolEnabled {
            tool_id: echo_tool.id.clone(),
            enabled: false,
        })
        .await,
    );
    assert!(!disabled.enabled);

    let run = expect_research_run(
        c1.request(OrchdRequest::ResearchStartRun {
            idea_id: idea.id.clone(),
            server_id: server.id.clone(),
            tool_name: "echo".to_string(),
            args_json: "{}".to_string(),
        })
        .await,
    );
    assert_eq!(run.status, ResearchStatus::Pending);

    let failed = poll_research_run_terminal(&mut c1, &run.id).await;
    assert_eq!(failed.status, ResearchStatus::Failed);
    assert_eq!(
        failed.error_kind.as_deref(),
        Some("tool_disabled"),
        "a call against a disabled tool must classify as tool_disabled (mirrors \
         classify_run_error's OrchdMcpError::ToolDisabled arm)"
    );
    assert!(failed.artifact_id.is_none());
    assert!(failed.invocation_id.is_none());

    c1.shutdown(boot).await;
}

#[tokio::test]
async fn get_storage_status_reports_persistent_for_a_fresh_daemon() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let boot = boot_daemon(&socket).await;
    let mut c1 = Client::connect(&socket).await;

    match c1.request(OrchdRequest::GetStorageStatus).await {
        OrchdResponse::StorageStatus(status) => {
            assert_eq!(
                status.storage_mode,
                bpa_orchd_proto::StorageMode::Persistent
            );
            assert!(
                status.quarantined_path.is_none(),
                "a fresh daemon has no quarantined path"
            );
        }
        other => panic!("expected StorageStatus, got {other:?}"),
    }

    c1.shutdown(boot).await;
}
