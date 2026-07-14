use std::collections::BTreeMap;

use bpa_orchd_proto::*;

/// Hop-B framing (`u32`-LE length prefix + CBOR body) must round-trip every `OrchdFrame`
/// byte-identically, mirroring `crates/protocol/tests/roundtrip.rs::assert_frame_roundtrip`.
fn assert_frame_roundtrip(frame: OrchdFrame) {
    let bytes = encode_orchd_frame(&frame).expect("encode");
    let mut decoder = OrchdFrameDecoder::new();
    decoder.push(&bytes);
    let mut decoded = decoder.decode().expect("decode");
    assert_eq!(decoded.len(), 1, "expected exactly one decoded frame");
    let back = decoded.remove(0);
    assert_eq!(
        encode_orchd_frame(&back).expect("re-encode"),
        bytes,
        "frame did not round-trip byte-identically"
    );
}

/// Encode `frame`, assert its raw CBOR wire bytes contain `needle` as a literal UTF-8
/// substring (CBOR text-string values embed their UTF-8 payload verbatim, so this proves
/// the *wire* representation — not just Rust-level equality after a round-trip, which
/// would still pass even if `rename_all = "camelCase"` were silently dropped).
fn assert_wire_contains(frame: &OrchdFrame, needle: &str) {
    let bytes = encode_orchd_frame(frame).expect("encode");
    let text = String::from_utf8_lossy(&bytes);
    assert!(
        text.contains(needle),
        "expected wire bytes to contain {needle:?}; got (lossy utf8):\n{text}"
    );
}

fn sample_project() -> Project {
    Project {
        id: "proj-1".into(),
        name: "Demo Project".into(),
        description: "A demo project".into(),
        status: ProjectStatus::Active,
        workspace_ids: vec!["ws-1".into(), "ws-2".into()],
        created_at: 1_720_000_000,
        updated_at: 1_720_000_100,
    }
}

fn sample_goal() -> Goal {
    Goal {
        id: "goal-1".into(),
        project_id: "proj-1".into(),
        parent_id: Some("goal-0".into()),
        kind: GoalKind::Additional,
        title: "Ship it".into(),
        body: "Ship the thing".into(),
        ord: 3,
        status: GoalStatus::Active,
        metric_refs: vec!["metric-1".into()],
        created_at: 1_720_000_000,
        updated_at: 1_720_000_100,
    }
}

fn sample_idea() -> Idea {
    Idea {
        id: "idea-1".into(),
        project_id: Some("proj-1".into()),
        title: "New idea".into(),
        body: "Idea body".into(),
        lifecycle: IdeaLifecycle::InDev,
        created_at: 1_720_000_000,
        updated_at: 1_720_000_100,
    }
}

fn sample_insight() -> Insight {
    Insight {
        id: "insight-1".into(),
        project_id: Some("proj-1".into()),
        source: "support-ticket".into(),
        title: "Users confused".into(),
        body: "Insight body".into(),
        fit_verdict: Some(FitVerdict::NoFit),
        fit_reasoning: "doesn't match roadmap".into(),
        status: InsightStatus::Accepted,
        resolution_reasoning: "resolved via docs".into(),
        created_at: 1_720_000_000,
        updated_at: 1_720_000_100,
    }
}

fn sample_task() -> DomainTask {
    DomainTask {
        id: "task-1".into(),
        project_id: "proj-1".into(),
        parent_id: None,
        title: "Do the thing".into(),
        body: "Task body".into(),
        status: TaskStatus::Backlog,
        source: TaskSource::Idea,
        source_id: Some("idea-1".into()),
        tags: vec!["urgent".into()],
        rank: 1.5,
        rank_agent: Some(2.5),
        rank_agent_reasoning: "agent reasoning".into(),
        created_at: 1_720_000_000,
        updated_at: 1_720_000_100,
    }
}

fn sample_policy() -> PolicyRules {
    PolicyRules {
        spend_cap_usd: Some(100.0),
        approval_classes: vec!["deploy".into()],
        path_allowlist: vec!["/tmp".into()],
    }
}

fn sample_ruleset() -> RuleSet {
    RuleSet {
        id: "rules-1".into(),
        scope: RuleScope::Project,
        project_id: Some("proj-1".into()),
        md_path: "/tmp/rules.md".into(),
        md_hash: "abc123".into(),
        policy: sample_policy(),
        created_at: 1_720_000_000,
        updated_at: 1_720_000_100,
    }
}

fn sample_ruleset_view() -> RuleSetView {
    RuleSetView {
        rule: sample_ruleset(),
        md_content: Some("# Rules".into()),
        file_state: RuleFileState::Ok,
    }
}

fn sample_graph_node() -> GraphNode {
    GraphNode {
        id: "node-1".into(),
        project_id: "proj-1".into(),
        kind: GraphNodeKind::EntityRef,
        entity_type: Some(GraphEntityType::Task),
        entity_id: Some("task-1".into()),
        label: "Node label".into(),
        body: "Node body".into(),
        pos_x: 12.5,
        pos_y: -3.25,
        created_at: 1_720_000_000,
        updated_at: 1_720_000_100,
        is_orphan: false,
    }
}

fn sample_graph_edge() -> GraphEdge {
    GraphEdge {
        id: "edge-1".into(),
        source_node_id: "node-1".into(),
        target_node_id: "node-2".into(),
        kind: GraphEdgeKind::Contradicts,
        label: "Edge label".into(),
        created_at: 1_720_000_000,
    }
}

fn sample_graph_view() -> GraphView {
    GraphView {
        nodes: vec![sample_graph_node()],
        edges: vec![sample_graph_edge()],
        external_nodes: vec![sample_graph_node()],
    }
}

fn sample_graph_neighborhood() -> GraphNeighborhood {
    GraphNeighborhood {
        root_id: "node-1".into(),
        nodes: vec![sample_graph_node()],
        edges: vec![sample_graph_edge()],
    }
}

fn sample_mcp_server() -> McpServer {
    McpServer {
        id: "mcp-1".into(),
        name: "Demo MCP".into(),
        transport: McpTransport::Http,
        url: Some("https://example.com/mcp".into()),
        command: None,
        args: vec![],
        env: BTreeMap::new(),
        scope: McpScope::Global,
        project_id: None,
        auth_kind: McpAuthKind::Bearer,
        secret_ref: Some("mcp-1-bearer".into()),
        account_id: None,
        enabled: true,
        timeout_ms: 30_000,
        max_retries: 2,
        protocol_version: Some("2024-11-05".into()),
        created_at: 1_720_000_000,
        updated_at: 1_720_000_100,
    }
}

fn sample_mcp_tool() -> McpTool {
    McpTool {
        id: "tool-1".into(),
        server_id: "mcp-1".into(),
        name: "search".into(),
        title: Some("Search".into()),
        description: Some("Searches things".into()),
        input_schema_json: "{}".into(),
        enabled: true,
        fetched_at: 1_720_000_000,
    }
}

fn sample_mcp_connect_report() -> McpConnectReport {
    McpConnectReport {
        protocol_version: "2024-11-05".into(),
        tool_count: 3,
    }
}

fn sample_mcp_call_result() -> McpCallResult {
    McpCallResult {
        artifact_id: "artifact-1".into(),
        invocation_id: "invocation-1".into(),
        content_json: "{\"ok\":true}".into(),
        is_error: false,
    }
}

fn sample_mcp_invocation() -> McpInvocation {
    McpInvocation {
        id: "invocation-1".into(),
        server_id: "mcp-1".into(),
        tool_name: "search".into(),
        project_id: Some("proj-1".into()),
        request_hash: "deadbeef".into(),
        ok: true,
        error_kind: None,
        latency_ms: 120,
        cost_usd: Some(0.002),
        input_tokens: Some(50),
        output_tokens: Some(20),
        started_at: 1_720_000_000,
    }
}

fn sample_mcp_artifact() -> McpArtifact {
    McpArtifact {
        id: "artifact-1".into(),
        invocation_id: "invocation-1".into(),
        server_id: "mcp-1".into(),
        tool_name: "search".into(),
        project_id: Some("proj-1".into()),
        content_json: "{\"ok\":true}".into(),
        content_text: Some("ok".into()),
        is_untrusted: true,
        created_at: 1_720_000_000,
    }
}

fn all_requests() -> Vec<OrchdRequest> {
    vec![
        OrchdRequest::Ping,
        OrchdRequest::CreateProject {
            name: "Demo".into(),
            description: "desc".into(),
            workspace_ids: vec!["ws-1".into()],
        },
        OrchdRequest::UpdateProject {
            id: "proj-1".into(),
            name: Some("Renamed".into()),
            description: None,
        },
        OrchdRequest::ArchiveProject {
            id: "proj-1".into(),
        },
        OrchdRequest::ListProjects,
        OrchdRequest::AddProjectWorkspace {
            project_id: "proj-1".into(),
            workspace_id: "ws-2".into(),
        },
        OrchdRequest::RemoveProjectWorkspace {
            project_id: "proj-1".into(),
            workspace_id: "ws-2".into(),
        },
        OrchdRequest::CreateGoal {
            project_id: "proj-1".into(),
            parent_id: Some("goal-0".into()),
            kind: GoalKind::Strategic,
            title: "Goal title".into(),
            body: "Goal body".into(),
        },
        OrchdRequest::UpdateGoal {
            id: "goal-1".into(),
            title: Some("New title".into()),
            body: None,
            status: Some(GoalStatus::Achieved),
            metric_refs: Some(vec!["metric-2".into()]),
        },
        OrchdRequest::MoveGoal {
            id: "goal-1".into(),
            new_parent_id: Some("goal-2".into()),
            new_ord: 5,
        },
        OrchdRequest::DeleteGoal {
            id: "goal-1".into(),
        },
        OrchdRequest::ListGoals {
            project_id: "proj-1".into(),
        },
        OrchdRequest::CreateIdea {
            project_id: Some("proj-1".into()),
            title: "Idea title".into(),
            body: "Idea body".into(),
        },
        OrchdRequest::UpdateIdea {
            id: "idea-1".into(),
            title: Some("New idea title".into()),
            body: None,
        },
        OrchdRequest::SetIdeaProject {
            id: "idea-1".into(),
            project_id: None,
        },
        OrchdRequest::SetIdeaLifecycle {
            id: "idea-1".into(),
            lifecycle: IdeaLifecycle::InDev,
        },
        OrchdRequest::DeleteIdea {
            id: "idea-1".into(),
        },
        OrchdRequest::ListIdeas {
            project_id: Some("proj-1".into()),
        },
        OrchdRequest::CreateInsight {
            project_id: Some("proj-1".into()),
            source: "support".into(),
            title: "Insight title".into(),
            body: "Insight body".into(),
        },
        OrchdRequest::UpdateInsight {
            id: "insight-1".into(),
            title: Some("New title".into()),
            body: None,
        },
        OrchdRequest::SetInsightFitVerdict {
            id: "insight-1".into(),
            fit_verdict: Some(FitVerdict::NoFit),
            fit_reasoning: "no fit reasoning".into(),
        },
        OrchdRequest::SetInsightStatus {
            id: "insight-1".into(),
            status: InsightStatus::Archived,
            resolution_reasoning: Some("archived reasoning".into()),
        },
        OrchdRequest::DeleteInsight {
            id: "insight-1".into(),
        },
        OrchdRequest::ListInsights { project_id: None },
        OrchdRequest::CreateTask {
            project_id: "proj-1".into(),
            parent_id: None,
            title: "Task title".into(),
            body: "Task body".into(),
            status: Some(TaskStatus::Backlog),
            source: TaskSource::Bug,
            source_id: None,
            tags: vec!["bug".into()],
        },
        OrchdRequest::UpdateTask {
            id: "task-1".into(),
            title: Some("New task title".into()),
            body: None,
            tags: Some(vec!["urgent".into()]),
        },
        OrchdRequest::SetTaskStatus {
            id: "task-1".into(),
            status: TaskStatus::Progress,
        },
        OrchdRequest::SetTaskRank {
            id: "task-1".into(),
            rank: 4.2,
        },
        OrchdRequest::DeleteTask {
            id: "task-1".into(),
        },
        OrchdRequest::ListTasks {
            project_id: Some("proj-1".into()),
        },
        OrchdRequest::GetRuleSet {
            scope: RuleScope::Global,
            project_id: None,
        },
        OrchdRequest::UpsertRuleSet {
            scope: RuleScope::Project,
            project_id: Some("proj-1".into()),
            md_content: Some("# Rules".into()),
            md_path: Some("/tmp/rules.md".into()),
            policy: Some(sample_policy()),
        },
        OrchdRequest::AcknowledgeRuleFile {
            id: "rules-1".into(),
        },
        OrchdRequest::ExportProject {
            project_id: "proj-1".into(),
        },
        OrchdRequest::ExportAll,
        OrchdRequest::ImportBundle { json: "{}".into() },
        OrchdRequest::OrchdShutdown { drain: true },
        OrchdRequest::OrchdShutdown { drain: false },
        OrchdRequest::GraphAddNode {
            project_id: "proj-1".into(),
            kind: GraphNodeKind::Concept,
            label: "New concept".into(),
            body: "Concept body".into(),
            pos_x: 1.0,
            pos_y: 2.0,
        },
        OrchdRequest::GraphUpdateNode {
            id: "node-1".into(),
            label: Some("Updated label".into()),
            body: Some("Updated body".into()),
        },
        OrchdRequest::GraphMoveNode {
            id: "node-1".into(),
            pos_x: 5.5,
            pos_y: 6.5,
        },
        OrchdRequest::GraphDeleteNode {
            id: "node-1".into(),
        },
        OrchdRequest::GraphAddEdge {
            source_node_id: "node-1".into(),
            target_node_id: "node-2".into(),
            kind: GraphEdgeKind::Depends,
            label: "depends on".into(),
        },
        OrchdRequest::GraphDeleteEdge {
            id: "edge-1".into(),
        },
        OrchdRequest::GraphListProject {
            project_id: "proj-1".into(),
        },
        OrchdRequest::GraphNeighborhood {
            node_id: "node-1".into(),
            depth: 3,
        },
        OrchdRequest::GraphSearch {
            query: "concept".into(),
            project_id: Some("proj-1".into()),
        },
        OrchdRequest::McpAddServer {
            name: "Demo MCP".into(),
            transport: McpTransport::Http,
            url: Some("https://example.com/mcp".into()),
            command: None,
            args: None,
            env: None,
            scope: McpScope::Global,
            project_id: None,
            auth_kind: McpAuthKind::None,
            timeout_ms: None,
            max_retries: None,
        },
        OrchdRequest::McpListServers {
            project_id: Some("proj-1".into()),
        },
        OrchdRequest::McpUpdateServer {
            id: "mcp-1".into(),
            name: Some("Renamed MCP".into()),
            url: None,
            command: None,
            args: Some(vec!["--flag".into()]),
            env: Some(BTreeMap::from([("KEY".to_string(), "value".to_string())])),
            auth_kind: Some(McpAuthKind::Bearer),
            timeout_ms: Some(5_000),
            max_retries: Some(3),
        },
        OrchdRequest::McpSetServerEnabled {
            id: "mcp-1".into(),
            enabled: false,
        },
        OrchdRequest::McpDeleteServer { id: "mcp-1".into() },
        OrchdRequest::McpSetServerBearer {
            id: "mcp-1".into(),
            token: "secret-token".into(),
        },
        OrchdRequest::McpConnect { id: "mcp-1".into() },
        OrchdRequest::McpDisconnect { id: "mcp-1".into() },
        OrchdRequest::McpListTools {
            server_id: "mcp-1".into(),
        },
        OrchdRequest::McpSetToolEnabled {
            tool_id: "tool-1".into(),
            enabled: false,
        },
        OrchdRequest::McpCallTool {
            server_id: "mcp-1".into(),
            tool_name: "search".into(),
            args_json: "{\"q\":\"rust\"}".into(),
            project_id: Some("proj-1".into()),
        },
        OrchdRequest::McpListInvocations {
            server_id: Some("mcp-1".into()),
            project_id: None,
            limit: Some(50),
        },
        OrchdRequest::McpListArtifacts {
            project_id: Some("proj-1".into()),
            server_id: None,
            limit: None,
        },
        OrchdRequest::McpGetArtifact {
            id: "artifact-1".into(),
        },
        OrchdRequest::TrustGrantConsent {
            server_id: "mcp-1".into(),
            kind: "connect".into(),
        },
    ]
}

fn all_responses() -> Vec<OrchdResponse> {
    vec![
        OrchdResponse::Ack,
        OrchdResponse::Pong,
        OrchdResponse::Project(sample_project()),
        OrchdResponse::Projects(vec![sample_project()]),
        OrchdResponse::Goal(sample_goal()),
        OrchdResponse::Goals(vec![sample_goal()]),
        OrchdResponse::Idea(sample_idea()),
        OrchdResponse::Ideas(vec![sample_idea()]),
        OrchdResponse::Insight(sample_insight()),
        OrchdResponse::Insights(vec![sample_insight()]),
        OrchdResponse::Task(sample_task()),
        OrchdResponse::Tasks(vec![sample_task()]),
        OrchdResponse::RuleSetView(sample_ruleset_view()),
        OrchdResponse::ExportJson("{\"projects\":[]}".into()),
        OrchdResponse::ImportReport {
            projects: 1,
            goals: 2,
            ideas: 3,
            insights: 4,
            tasks: 5,
            rulesets: 6,
        },
        OrchdResponse::Error {
            code: OrchdErrorCode::NotFound,
            message: "not found".into(),
        },
        OrchdResponse::Error {
            code: OrchdErrorCode::Invariant,
            message: "invariant".into(),
        },
        OrchdResponse::Error {
            code: OrchdErrorCode::Validation,
            message: "validation".into(),
        },
        OrchdResponse::Error {
            code: OrchdErrorCode::Conflict,
            message: "conflict".into(),
        },
        OrchdResponse::Error {
            code: OrchdErrorCode::Io,
            message: "io".into(),
        },
        OrchdResponse::GraphNode(sample_graph_node()),
        OrchdResponse::GraphEdge(sample_graph_edge()),
        OrchdResponse::GraphView(sample_graph_view()),
        OrchdResponse::Neighborhood(sample_graph_neighborhood()),
        OrchdResponse::GraphNodes(vec![sample_graph_node()]),
        OrchdResponse::McpServer(sample_mcp_server()),
        OrchdResponse::McpServers(vec![sample_mcp_server()]),
        OrchdResponse::McpTool(sample_mcp_tool()),
        OrchdResponse::McpTools(vec![sample_mcp_tool()]),
        OrchdResponse::McpConnectReport(sample_mcp_connect_report()),
        OrchdResponse::McpCallResult(sample_mcp_call_result()),
        OrchdResponse::McpInvocations(vec![sample_mcp_invocation()]),
        OrchdResponse::McpArtifacts(vec![sample_mcp_artifact()]),
        OrchdResponse::McpArtifact(sample_mcp_artifact()),
    ]
}

fn all_pushes() -> Vec<OrchdPush> {
    vec![
        OrchdPush::ProjectsChanged,
        OrchdPush::GoalsChanged {
            project_id: "proj-1".into(),
        },
        OrchdPush::IdeasChanged,
        OrchdPush::InsightsChanged,
        OrchdPush::TasksChanged {
            project_id: "proj-1".into(),
        },
        OrchdPush::RuleSetChanged {
            scope: RuleScope::Global,
            project_id: None,
        },
        OrchdPush::RuleSetChanged {
            scope: RuleScope::Project,
            project_id: Some("proj-1".into()),
        },
        OrchdPush::GraphChanged {
            project_id: "proj-1".into(),
        },
        OrchdPush::McpServersChanged {
            project_id: Some("proj-1".into()),
        },
        OrchdPush::McpToolsChanged {
            server_id: "mcp-1".into(),
        },
        OrchdPush::McpArtifactsChanged { project_id: None },
        OrchdPush::McpInvocationLogged {
            server_id: "mcp-1".into(),
        },
    ]
}

#[test]
fn every_request_variant_roundtrips() {
    for (i, req) in all_requests().into_iter().enumerate() {
        assert_frame_roundtrip(OrchdFrame::Request { id: i as u64, req });
    }
}

#[test]
fn every_response_variant_roundtrips() {
    for (i, res) in all_responses().into_iter().enumerate() {
        assert_frame_roundtrip(OrchdFrame::Response { id: i as u64, res });
    }
}

#[test]
fn every_push_variant_roundtrips() {
    for push in all_pushes() {
        assert_frame_roundtrip(OrchdFrame::Push(push));
    }
}

#[test]
fn idea_lifecycle_in_dev_serializes_as_camelcase_on_the_wire() {
    let frame = OrchdFrame::Request {
        id: 1,
        req: OrchdRequest::SetIdeaLifecycle {
            id: "idea-1".into(),
            lifecycle: IdeaLifecycle::InDev,
        },
    };
    assert_wire_contains(&frame, "inDev");
}

#[test]
fn task_status_backlog_serializes_lowercase_on_the_wire() {
    let frame = OrchdFrame::Request {
        id: 1,
        req: OrchdRequest::SetTaskStatus {
            id: "task-1".into(),
            status: TaskStatus::Backlog,
        },
    };
    assert_wire_contains(&frame, "backlog");
}

#[test]
fn fit_verdict_no_fit_serializes_as_camelcase_on_the_wire() {
    let frame = OrchdFrame::Request {
        id: 1,
        req: OrchdRequest::SetInsightFitVerdict {
            id: "insight-1".into(),
            fit_verdict: Some(FitVerdict::NoFit),
            fit_reasoning: "reasoning".into(),
        },
    };
    assert_wire_contains(&frame, "noFit");
}

/// Assert a bare enum value serializes to *exactly* the given camelCase tag (a JSON string
/// literal). Serde's tag string is codec-independent — the same derived `Serialize` impl
/// backs both this `serde_json` output and the CBOR wire — so exact equality here proves the
/// `rename_all = "camelCase"` tag that also goes over the wire. Unlike a substring check
/// against a whole frame, this can never be satisfied tautologically by an unrelated field
/// value (e.g. an `entity_id` of `"task-1"` that happens to contain the substring `"task"`).
fn assert_serde_tag<T: serde::Serialize>(value: &T, expected_tag: &str) {
    let json = serde_json::to_string(value).expect("serialize enum value to JSON");
    assert_eq!(
        json,
        format!("\"{expected_tag}\""),
        "enum value must serialize to exactly the tag {expected_tag:?}; got {json}"
    );
}

#[test]
fn graph_node_kind_entity_ref_serializes_as_camelcase_on_the_wire() {
    // Discriminating: exact tag equality (a broken `rename_all` producing "EntityRef" fails).
    assert_serde_tag(&GraphNodeKind::EntityRef, "entityRef");
    // And prove the tag literally reaches the CBOR wire (no field here contains "entityRef").
    let frame = OrchdFrame::Request {
        id: 1,
        req: OrchdRequest::GraphAddNode {
            project_id: "proj-1".into(),
            kind: GraphNodeKind::EntityRef,
            label: "label".into(),
            body: "body".into(),
            pos_x: 0.0,
            pos_y: 0.0,
        },
    };
    assert_wire_contains(&frame, "entityRef");
}

#[test]
fn graph_edge_kind_contradicts_serializes_lowercase_on_the_wire() {
    // Discriminating: exact tag equality (a broken `rename_all` producing "Contradicts" fails).
    assert_serde_tag(&GraphEdgeKind::Contradicts, "contradicts");
    // And prove the tag literally reaches the CBOR wire (no field here contains "contradicts").
    let frame = OrchdFrame::Request {
        id: 1,
        req: OrchdRequest::GraphAddEdge {
            source_node_id: "node-1".into(),
            target_node_id: "node-2".into(),
            kind: GraphEdgeKind::Contradicts,
            label: "label".into(),
        },
    };
    assert_wire_contains(&frame, "contradicts");
}

#[test]
fn graph_entity_type_task_serializes_lowercase_on_the_wire() {
    // Discriminating: exact tag equality (a broken `rename_all` producing "Task" fails). A
    // substring check against a frame is NOT enough here — `sample_graph_node()` carries
    // `entity_id: Some("task-1")`, which trivially contains the substring "task".
    assert_serde_tag(&GraphEntityType::Task, "task");
    // And prove the tag literally reaches the CBOR wire from a node whose OTHER fields carry
    // no "task" substring, so the only source of "task" in the bytes is the serialized tag.
    let frame = OrchdFrame::Response {
        id: 1,
        res: OrchdResponse::GraphNode(GraphNode {
            id: "n-1".into(),
            project_id: "p-1".into(),
            kind: GraphNodeKind::EntityRef,
            entity_type: Some(GraphEntityType::Task),
            entity_id: Some("e-1".into()),
            label: "label".into(),
            body: "body".into(),
            pos_x: 0.0,
            pos_y: 0.0,
            created_at: 1,
            updated_at: 2,
            is_orphan: false,
        }),
    };
    assert_wire_contains(&frame, "task");
}

#[test]
fn version_consts_are_locked_to_one() {
    assert_eq!(ORCHD_CLIENT_MIN_VERSION, 1);
    assert_eq!(ORCHD_CLIENT_MAX_VERSION, 1);
    assert_eq!(ORCHD_DAEMON_MIN_VERSION, 1);
    assert_eq!(ORCHD_DAEMON_MAX_VERSION, 1);
}

#[test]
fn no_double_option_on_update_verbs() {
    // D11: `Update*` verbs use single `Option<T>` (absent OR null both mean "unchanged");
    // there is no `Option<Option<T>>` anywhere in the wire contract. This is a compile-time
    // property (the fields below are plain `Option<T>`), asserted here by simply
    // constructing every `Update*`/`Set*` variant with `None` and `Some` payloads — if any
    // field were `Option<Option<T>>` these literals would not type-check.
    let _ = OrchdRequest::UpdateProject {
        id: "p".into(),
        name: None,
        description: None,
    };
    let _ = OrchdRequest::SetIdeaProject {
        id: "i".into(),
        project_id: None,
    };
    let _ = OrchdRequest::SetInsightFitVerdict {
        id: "i".into(),
        fit_verdict: None,
        fit_reasoning: String::new(),
    };
    // S-EXT: OrchdRequest::McpUpdateServer follows the same D11 shape.
    let _ = OrchdRequest::McpUpdateServer {
        id: "mcp-1".into(),
        name: None,
        url: None,
        command: None,
        args: None,
        env: None,
        auth_kind: None,
        timeout_ms: None,
        max_retries: None,
    };
}

// ---- S-EXT MCP wire-tag / entity-camelCase / frame-JSON-roundtrip tests (task T3) ----

#[test]
fn mcp_transport_wire_tags_are_lowercase() {
    // Discriminating: exact tag equality (a broken `rename_all` producing "Http" fails).
    assert_serde_tag(&McpTransport::Http, "http");
    assert_serde_tag(&McpTransport::Stdio, "stdio");
}

#[test]
fn mcp_scope_wire_tags_are_lowercase() {
    assert_serde_tag(&McpScope::Global, "global");
    assert_serde_tag(&McpScope::Project, "project");
}

#[test]
fn mcp_auth_kind_wire_tags_are_lowercase() {
    assert_serde_tag(&McpAuthKind::None, "none");
    assert_serde_tag(&McpAuthKind::Bearer, "bearer");
    assert_serde_tag(&McpAuthKind::Oauth, "oauth");
}

#[test]
fn mcp_server_entity_serializes_with_camelcase_keys() {
    let json = serde_json::to_string(&sample_mcp_server()).expect("serialize McpServer");
    assert!(
        json.contains("\"createdAt\""),
        "McpServer.created_at must serialize as camelCase `createdAt`; got:\n{json}"
    );
    assert!(
        json.contains("\"projectId\""),
        "McpServer.project_id must serialize as camelCase `projectId`; got:\n{json}"
    );
    assert!(
        json.contains("\"authKind\""),
        "McpServer.auth_kind must serialize as camelCase `authKind`; got:\n{json}"
    );
    assert!(
        !json.contains("created_at"),
        "generated JSON must not contain snake_case `created_at`; got:\n{json}"
    );
    assert!(
        !json.contains("project_id"),
        "generated JSON must not contain snake_case `project_id`; got:\n{json}"
    );
}

#[test]
fn mcp_servers_response_json_roundtrips() {
    // Frame round-trip via `serde_json` directly (in addition to the CBOR-wire round-trip
    // exercised by `every_response_variant_roundtrips`): the frame stays plain snake_case even
    // though it wraps a camelCase entity.
    let original = OrchdResponse::McpServers(vec![sample_mcp_server()]);
    let json = serde_json::to_string(&original).expect("serialize OrchdResponse::McpServers");
    let decoded: OrchdResponse =
        serde_json::from_str(&json).expect("deserialize OrchdResponse::McpServers");
    assert_eq!(
        decoded, original,
        "OrchdResponse::McpServers must JSON round-trip byte-for-byte equal"
    );
}
