//! Socket-level tests for the T10 domain dispatch table (spec §4.2, §6, §7): boot the real daemon
//! `run()` on a temp socket under an isolated `$HOME` (mirrors `boot_integration.rs`'s pattern —
//! `app_support_dir()` reads `$HOME` at every request, not just at boot, since `ImportBundle`/
//! `CreateProject`'s post-commit ruleset write both resolve it fresh per-request), drive real
//! `OrchdRequest`s over the wire via two connections, and assert both halves of spec §6's
//! contract: a successful mutating verb replies the updated entity AND every OTHER connected
//! client observes the matching coarse push; a FAILED verb broadcasts nothing.

use std::path::Path;
use std::time::Duration;

use bpa_orchd::protocol::{
    encode_orchd_frame, Goal, GoalKind, GraphEdge, GraphEdgeKind, GraphNeighborhood, GraphNode,
    GraphNodeKind, GraphView, Idea, OrchdErrorCode, OrchdFrame, OrchdFrameDecoder, OrchdPush,
    OrchdRequest, OrchdResponse, Project, RuleFileState, RuleScope, RuleSetView,
    ORCHD_CLIENT_MAX_VERSION, ORCHD_CLIENT_MIN_VERSION, ORCHD_DAEMON_MAX_VERSION,
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

fn expect_ruleset_view(res: OrchdResponse) -> RuleSetView {
    match res {
        OrchdResponse::RuleSetView(v) => v,
        other => panic!("expected RuleSetView, got {other:?}"),
    }
}

fn expect_ack(res: OrchdResponse) {
    match res {
        OrchdResponse::Ack => {}
        other => panic!("expected Ack, got {other:?}"),
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
    assert_eq!(content, "# Правила проекта Acme\n");

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
    assert_eq!(view.md_content.as_deref(), Some("# Правила проекта Acme\n"));

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
