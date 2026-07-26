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
        priority: TaskPriority::Urgent,
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
        // SCN-046 / A-7 CEO supervisor config rides inside PolicyRules.
        supervisor: SupervisorConfig {
            enabled: true,
            delegated_classes: vec!["safe-shell".into(), "file-write".into()],
            instruction: "Keep changes small.".into(),
            custom_rules: vec!["never push to main".into()],
        },
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
        server_id: Some("mcp-1".into()),
        account_id: None,
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
        server_id: Some("mcp-1".into()),
        account_id: None,
        tool_name: "search".into(),
        project_id: Some("proj-1".into()),
        content_json: "{\"ok\":true}".into(),
        content_text: Some("ok".into()),
        is_untrusted: true,
        created_at: 1_720_000_000,
    }
}

fn sample_account() -> Account {
    Account {
        id: "account-1".into(),
        provider: "generic-rest".into(),
        label: "Demo account".into(),
        auth_kind: AccountAuthKind::Oauth,
        scopes: vec!["read".into(), "write".into()],
        expires_at: Some(1_720_000_500),
        created_at: 1_720_000_000,
        updated_at: 1_720_000_100,
    }
}

fn sample_connector_op() -> ConnectorOp {
    ConnectorOp {
        name: "get".into(),
        description: Some("HTTP GET against the account's base URL".into()),
    }
}

fn sample_oauth_challenge() -> OAuthChallenge {
    OAuthChallenge {
        authorize_url: "https://example.com/oauth/authorize?state=abc".into(),
        state: "abc".into(),
    }
}

fn sample_skill() -> Skill {
    Skill {
        id: "skill-1".into(),
        name: "Demo Skill".into(),
        description: "A demo skill".into(),
        md_path: "/Users/demo/skills/demo/SKILL.md".into(),
        md_hash: "deadbeef".into(),
        scope: SkillScope::Global,
        project_id: None,
        file_state: SkillFileState::Present,
        created_at: 1_720_000_000,
        updated_at: 1_720_000_100,
    }
}

/// Named to avoid colliding with [`sample_policy`] above, which builds `PolicyRules` (the
/// pre-existing, unrelated per-ruleset owner-consent policy — see `Policy`'s own doc comment for
/// why the two types coexist).
fn sample_trust_policy() -> Policy {
    Policy {
        id: "policy-1".into(),
        scope: PolicyScope::Server,
        ref_id: Some("mcp-1".into()),
        spend_cap_usd: Some(5.0),
        rate_per_min: Some(30),
        created_at: 1_720_000_000,
        updated_at: 1_720_000_100,
    }
}

fn sample_audit_row() -> AuditRow {
    AuditRow {
        id: "audit-1".into(),
        at: 1_720_000_200,
        action: "policy_deny".into(),
        server_id: Some("mcp-1".into()),
        tool_name: Some("search".into()),
        project_id: Some("proj-1".into()),
        decision: "deny".into(),
        reason: Some("rate_limit_exceeded".into()),
        invocation_id: None,
    }
}

/// Named to avoid colliding with `sample_mcp_server`'s bare `server_id` sample values above.
fn sample_research_run() -> ResearchRun {
    ResearchRun {
        id: "run-1".into(),
        idea_id: "idea-1".into(),
        server_id: "mcp-1".into(),
        tool_name: "search".into(),
        args_json: "{\"q\":\"rust\"}".into(),
        status: ResearchStatus::Pending,
        invocation_id: None,
        artifact_id: None,
        error_kind: None,
        created_at: 1_720_000_000,
        updated_at: 1_720_000_100,
    }
}

fn sample_stage() -> Stage {
    Stage {
        id: "stage-1".into(),
        name: "Plan".into(),
        prompt: "Draft the plan".into(),
        skill_ids: vec!["skill-1".into()],
        // `Some` so the round-trip proves the Option<String> agent binding, distinct from the
        // workflow-level default.
        agent: Some("hermes".into()),
        context_scope: ContextScope::Handoff,
        outputs: vec!["plan.md".into()],
        gate: Gate::Manual,
    }
}

fn sample_workflow() -> Workflow {
    Workflow {
        id: "wf-1".into(),
        name: "ship-feature".into(),
        description: "Author, review, ship".into(),
        scope: WorkflowScope::Project,
        project_id: Some("proj-1".into()),
        default_agent: "claude-code".into(),
        stages: vec![sample_stage()],
        global_skill_ids: vec!["skill-2".into()],
        // Reuses the wire SupervisorConfig verbatim (SW1 contract).
        supervisor: SupervisorConfig {
            enabled: true,
            delegated_classes: vec!["safe-shell".into()],
            instruction: "Keep diffs small.".into(),
            custom_rules: vec!["never push to main".into()],
        },
        file_state: SkillFileState::Present,
        json_path: "/Users/demo/rules/workflows/proj-1/wf-1.json".into(),
        hash: "deadbeef".into(),
        created_at: 1_720_000_000,
        updated_at: 1_720_000_100,
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
            priority: Some(TaskPriority::Urgent),
        },
        OrchdRequest::CreateTask {
            project_id: "proj-1".into(),
            parent_id: None,
            title: "Task title".into(),
            body: "Task body".into(),
            status: None,
            source: TaskSource::Plan,
            source_id: None,
            tags: vec![],
            // `None` ⇒ the daemon defaults to `Normal` (SCN-051 create-form default).
            priority: None,
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
            project_id: Some("proj-1".into()),
        },
        OrchdRequest::GraphMoveNode {
            id: "node-1".into(),
            pos_x: 5.5,
            pos_y: 6.5,
            project_id: None,
        },
        OrchdRequest::GraphDeleteNode {
            id: "node-1".into(),
            project_id: Some("proj-1".into()),
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
        OrchdRequest::ConnectorBeginOAuth {
            provider: "generic-rest".into(),
            label: "Demo account".into(),
            scopes: Some(vec!["read".into()]),
            server_id: None,
        },
        OrchdRequest::ConnectorCompleteOAuth {
            state: "abc".into(),
            code: "auth-code".into(),
        },
        OrchdRequest::ConnectorAddApiKey {
            provider: "generic-rest".into(),
            label: "Demo API key".into(),
            api_key: "sk-demo".into(),
        },
        OrchdRequest::ConnectorListAccounts,
        OrchdRequest::ConnectorDeleteAccount {
            id: "account-1".into(),
        },
        OrchdRequest::ConnectorListOps {
            account_id: "account-1".into(),
        },
        OrchdRequest::ConnectorInvoke {
            account_id: "account-1".into(),
            op: "get".into(),
            args_json: "{\"path\":\"/ping\"}".into(),
            project_id: Some("proj-1".into()),
        },
        OrchdRequest::SkillAdd {
            name: Some("Demo Skill".into()),
            description: Some("A demo skill".into()),
            md_path: "/Users/demo/skills/demo/SKILL.md".into(),
            scope: SkillScope::Global,
            project_id: None,
        },
        OrchdRequest::SkillAdd {
            name: None,
            description: None,
            md_path: "/Users/demo/skills/demo/SKILL.md".into(),
            scope: SkillScope::Project,
            project_id: Some("proj-1".into()),
        },
        OrchdRequest::SkillList {
            project_id: Some("proj-1".into()),
        },
        OrchdRequest::SkillDelete {
            id: "skill-1".into(),
        },
        OrchdRequest::TrustSetPolicy {
            scope: PolicyScope::Global,
            ref_id: None,
            spend_cap_usd: Some(10.0),
            rate_per_min: None,
        },
        OrchdRequest::TrustSetPolicy {
            scope: PolicyScope::Server,
            ref_id: Some("mcp-1".into()),
            spend_cap_usd: None,
            rate_per_min: Some(5),
        },
        OrchdRequest::TrustListPolicies,
        OrchdRequest::TrustListAudit { limit: Some(50) },
        OrchdRequest::TrustListAudit { limit: None },
        OrchdRequest::ResearchStartRun {
            idea_id: "idea-1".into(),
            server_id: "mcp-1".into(),
            tool_name: "search".into(),
            args_json: "{\"q\":\"rust\"}".into(),
        },
        OrchdRequest::ResearchListRuns {
            idea_id: "idea-1".into(),
        },
        OrchdRequest::ResearchGetRun { id: "run-1".into() },
        OrchdRequest::GetStorageStatus,
        OrchdRequest::UnarchiveProject {
            id: "proj-1".into(),
        },
        OrchdRequest::GraphUpdateEdge {
            id: "edge-1".into(),
            kind: GraphEdgeKind::Supports,
        },
        OrchdRequest::ConnectorListProviders,
        OrchdRequest::SetTaskPriority {
            id: "task-1".into(),
            priority: TaskPriority::Urgent,
        },
        OrchdRequest::SetTaskPriority {
            id: "task-1".into(),
            priority: TaskPriority::Normal,
        },
        // SW1 Workflow authoring (tail-appended).
        OrchdRequest::WorkflowList {
            scope: Some(WorkflowScope::Project),
            project_id: Some("proj-1".into()),
        },
        OrchdRequest::WorkflowList {
            scope: None,
            project_id: None,
        },
        OrchdRequest::WorkflowGet { id: "wf-1".into() },
        OrchdRequest::WorkflowUpsert {
            id: String::new(),
            name: "ship-feature".into(),
            description: "Author, review, ship".into(),
            scope: WorkflowScope::Project,
            project_id: Some("proj-1".into()),
            default_agent: "claude-code".into(),
            stages: vec![sample_stage()],
            global_skill_ids: vec!["skill-2".into()],
            supervisor: SupervisorConfig {
                enabled: true,
                delegated_classes: vec!["safe-shell".into()],
                instruction: "Keep diffs small.".into(),
                custom_rules: vec!["never push to main".into()],
            },
        },
        OrchdRequest::WorkflowDelete { id: "wf-1".into() },
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
        OrchdResponse::Account(sample_account()),
        OrchdResponse::Accounts(vec![sample_account()]),
        OrchdResponse::OAuthChallenge(sample_oauth_challenge()),
        OrchdResponse::ConnectorOps(vec![sample_connector_op()]),
        OrchdResponse::Skill(sample_skill()),
        OrchdResponse::Skills(vec![sample_skill()]),
        OrchdResponse::Policy(sample_trust_policy()),
        OrchdResponse::Policies(vec![sample_trust_policy()]),
        OrchdResponse::AuditRows(vec![sample_audit_row()]),
        OrchdResponse::Error {
            code: OrchdErrorCode::Policy,
            message: "rate_limit_exceeded".into(),
        },
        OrchdResponse::ResearchRun(sample_research_run()),
        OrchdResponse::ResearchRuns(vec![sample_research_run()]),
        OrchdResponse::StorageStatus(StorageStatus {
            storage_mode: StorageMode::RecoveredFromCorruption,
            quarantined_path: Some("/x/orchd.db.corrupt-1".into()),
        }),
        OrchdResponse::StorageStatus(StorageStatus {
            storage_mode: StorageMode::Persistent,
            quarantined_path: None,
        }),
        OrchdResponse::ConnectorProviders(vec!["prowl".into(), "github".into()]),
        OrchdResponse::ConnectorProviders(vec![]),
        // SW1 Workflow authoring (tail-appended).
        OrchdResponse::Workflow(sample_workflow()),
        OrchdResponse::Workflows(vec![sample_workflow()]),
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
        OrchdPush::ConnectorsChanged,
        OrchdPush::SkillsChanged {
            project_id: Some("proj-1".into()),
        },
        OrchdPush::SkillsChanged { project_id: None },
        OrchdPush::PoliciesChanged,
        OrchdPush::ResearchRunsChanged {
            idea_id: Some("idea-1".into()),
        },
        OrchdPush::ResearchRunsChanged { idea_id: None },
        // SW1 Workflow authoring (tail-appended): bare invalidation, no payload.
        OrchdPush::WorkflowsChanged,
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
fn task_priority_urgent_serializes_lowercase_on_the_wire() {
    // SCN-051 (ST-037): exact tag equality (a broken `rename_all` producing "Urgent" fails)…
    assert_serde_tag(&TaskPriority::Urgent, "urgent");
    assert_serde_tag(&TaskPriority::Normal, "normal");
    // …and prove the tag literally reaches the CBOR wire from a frame whose OTHER fields carry
    // no "urgent" substring, so the only source of "urgent" in the bytes is the serialized tag.
    let frame = OrchdFrame::Request {
        id: 1,
        req: OrchdRequest::SetTaskPriority {
            id: "task-1".into(),
            priority: TaskPriority::Urgent,
        },
    };
    assert_wire_contains(&frame, "urgent");
}

#[test]
fn task_priority_default_is_normal() {
    // SCN-051: `Normal` is the contract-wide default — the DB column default, the create-form
    // default, and the `#[serde(default)]` backfill below all hang off this one impl.
    assert_eq!(TaskPriority::default(), TaskPriority::Normal);
}

#[test]
fn domain_task_without_priority_key_deserializes_as_normal() {
    // SCN-051 back-compat: a pre-priority export bundle (schema ≤ v4) serialized its tasks
    // WITHOUT a `priority` key. `#[serde(default)]` on `DomainTask.priority` must decode such a
    // payload as `Normal` instead of rejecting the whole bundle. (Codec-independent: the same
    // derived `Deserialize` impl backs both this JSON input and the CBOR wire.)
    let json = r#"{
        "id": "task-1", "projectId": "proj-1", "parentId": null,
        "title": "T", "body": "", "status": "backlog", "source": "plan",
        "sourceId": null, "tags": [], "rank": 1024.0, "rankAgent": null,
        "rankAgentReasoning": "", "createdAt": 0, "updatedAt": 0
    }"#;
    let task: DomainTask = serde_json::from_str(json).expect("pre-priority task must deserialize");
    assert_eq!(task.priority, TaskPriority::Normal);
}

#[test]
fn create_task_without_priority_key_deserializes_as_none() {
    // SCN-051 back-compat mirror of the entity test above, for the verb: a `CreateTask` frame
    // from a pre-priority peer omits the `priority` key entirely — `#[serde(default)]` decodes
    // it as `None` (⇒ daemon defaults to `Normal`) instead of failing the frame.
    let json = r#"{"CreateTask": {
        "project_id": "proj-1", "parent_id": null, "title": "T", "body": "",
        "status": null, "source": "plan", "source_id": null, "tags": []
    }}"#;
    // NOTE: frame types are NOT camelCased (Hop-B wire-only) — field names stay snake_case;
    // `TaskSource`'s tag itself IS camelCased ("plan") because the entity enum is TS-exported.
    let req: OrchdRequest = serde_json::from_str(json).expect("pre-priority CreateTask decodes");
    match req {
        OrchdRequest::CreateTask { priority, .. } => assert_eq!(priority, None),
        other => panic!("expected CreateTask, got {other:?}"),
    }
}

#[test]
fn graph_node_mutation_verbs_without_project_id_key_deserialize_as_none() {
    // GRAPH-1 (BL-143) back-compat mirror of the CreateTask test above: a pre-GRAPH-1 peer
    // omits the `project_id` key on the three node-mutation verbs — `#[serde(default)]` must
    // decode it as `None` (⇒ legacy unchecked behavior) instead of failing the frame. Field
    // names stay snake_case (Hop-B frames are NOT camelCased).
    let update: OrchdRequest = serde_json::from_str(
        r#"{"GraphUpdateNode": {"id": "node-1", "label": "L", "body": null}}"#,
    )
    .expect("pre-GRAPH-1 GraphUpdateNode decodes");
    match update {
        OrchdRequest::GraphUpdateNode { project_id, .. } => assert_eq!(project_id, None),
        other => panic!("expected GraphUpdateNode, got {other:?}"),
    }
    let mv: OrchdRequest =
        serde_json::from_str(r#"{"GraphMoveNode": {"id": "node-1", "pos_x": 1.0, "pos_y": 2.0}}"#)
            .expect("pre-GRAPH-1 GraphMoveNode decodes");
    match mv {
        OrchdRequest::GraphMoveNode { project_id, .. } => assert_eq!(project_id, None),
        other => panic!("expected GraphMoveNode, got {other:?}"),
    }
    let delete: OrchdRequest = serde_json::from_str(r#"{"GraphDeleteNode": {"id": "node-1"}}"#)
        .expect("pre-GRAPH-1 GraphDeleteNode decodes");
    match delete {
        OrchdRequest::GraphDeleteNode { project_id, .. } => assert_eq!(project_id, None),
        other => panic!("expected GraphDeleteNode, got {other:?}"),
    }
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

// ---- S-EXT connector/OAuth wire-tag / entity-camelCase / frame-JSON-roundtrip tests (task
// T10, spec §5/§7 Phase-2 subset) ----

#[test]
fn account_auth_kind_wire_tags_are_lowercase() {
    // Discriminating: exact tag equality (a broken `rename_all` producing "Oauth" fails).
    assert_serde_tag(&AccountAuthKind::Oauth, "oauth");
    assert_serde_tag(&AccountAuthKind::Apikey, "apikey");
}

#[test]
fn account_entity_serializes_with_camelcase_keys() {
    let json = serde_json::to_string(&sample_account()).expect("serialize Account");
    assert!(
        json.contains("\"authKind\""),
        "Account.auth_kind must serialize as camelCase `authKind`; got:\n{json}"
    );
    assert!(
        json.contains("\"expiresAt\""),
        "Account.expires_at must serialize as camelCase `expiresAt`; got:\n{json}"
    );
    assert!(
        json.contains("\"createdAt\""),
        "Account.created_at must serialize as camelCase `createdAt`; got:\n{json}"
    );
    assert!(
        !json.contains("auth_kind") && !json.contains("expires_at") && !json.contains("created_at"),
        "generated JSON must not contain snake_case field names; got:\n{json}"
    );
    assert!(
        !json.contains("secretRef") && !json.contains("secret_ref"),
        "Account must NOT expose secret_ref on the wire (Keychain key structure); got:\n{json}"
    );
    assert!(
        !json.contains("refreshRef") && !json.contains("refresh_ref"),
        "Account must NOT expose refresh_ref on the wire (Keychain key structure); got:\n{json}"
    );
}

#[test]
fn oauth_challenge_entity_serializes_with_camelcase_authorize_url() {
    let json = serde_json::to_string(&sample_oauth_challenge()).expect("serialize OAuthChallenge");
    assert!(
        json.contains("\"authorizeUrl\""),
        "OAuthChallenge.authorize_url must serialize as camelCase `authorizeUrl`; got:\n{json}"
    );
    assert!(
        !json.contains("authorize_url"),
        "generated JSON must not contain snake_case `authorize_url`; got:\n{json}"
    );
}

#[test]
fn connector_accounts_response_json_roundtrips() {
    // Frame round-trip via `serde_json` directly (in addition to the CBOR-wire round-trip
    // exercised by `every_response_variant_roundtrips`): the frame stays plain snake_case even
    // though it wraps a camelCase entity.
    let original = OrchdResponse::Accounts(vec![sample_account()]);
    let json = serde_json::to_string(&original).expect("serialize OrchdResponse::Accounts");
    let decoded: OrchdResponse =
        serde_json::from_str(&json).expect("deserialize OrchdResponse::Accounts");
    assert_eq!(
        decoded, original,
        "OrchdResponse::Accounts must JSON round-trip byte-for-byte equal"
    );
}

// ---- S-EXT Skills (spec §4/§5, D11, Q14, task T17) ----

#[test]
fn skill_scope_wire_tags_are_lowercase() {
    assert_serde_tag(&SkillScope::Global, "global");
    assert_serde_tag(&SkillScope::Project, "project");
}

#[test]
fn skill_file_state_wire_tags_are_lowercase() {
    assert_serde_tag(&SkillFileState::Present, "present");
    assert_serde_tag(&SkillFileState::Modified, "modified");
    assert_serde_tag(&SkillFileState::Missing, "missing");
}

#[test]
fn skill_entity_serializes_with_camelcase_keys() {
    let json = serde_json::to_string(&sample_skill()).expect("serialize Skill");
    assert!(
        json.contains("\"mdPath\""),
        "Skill.md_path must serialize as camelCase `mdPath`; got:\n{json}"
    );
    assert!(
        json.contains("\"mdHash\""),
        "Skill.md_hash must serialize as camelCase `mdHash`; got:\n{json}"
    );
    assert!(
        json.contains("\"fileState\""),
        "Skill.file_state must serialize as camelCase `fileState`; got:\n{json}"
    );
    assert!(
        json.contains("\"projectId\""),
        "Skill.project_id must serialize as camelCase `projectId`; got:\n{json}"
    );
    assert!(
        !json.contains("md_path")
            && !json.contains("md_hash")
            && !json.contains("file_state")
            && !json.contains("project_id"),
        "generated JSON must not contain snake_case field names; got:\n{json}"
    );
}

#[test]
fn skill_list_response_json_roundtrips() {
    // Mirrors `connector_accounts_response_json_roundtrips` above: a plain-`serde_json`
    // round-trip in addition to the CBOR-wire round-trip `every_response_variant_roundtrips`
    // already exercises.
    let original = OrchdResponse::Skills(vec![sample_skill()]);
    let json = serde_json::to_string(&original).expect("serialize OrchdResponse::Skills");
    let decoded: OrchdResponse =
        serde_json::from_str(&json).expect("deserialize OrchdResponse::Skills");
    assert_eq!(
        decoded, original,
        "OrchdResponse::Skills must JSON round-trip byte-for-byte equal"
    );
}

// ---- S-EXT Trust entity/verb tests (spec §4/§5/§6, BL-22, task T18) ----

#[test]
fn policy_scope_server_serializes_lowercase_on_the_wire() {
    // Discriminating: exact tag equality (a broken `rename_all` producing "Server" fails).
    assert_serde_tag(&PolicyScope::Server, "server");
    let frame = OrchdFrame::Request {
        id: 1,
        req: OrchdRequest::TrustSetPolicy {
            scope: PolicyScope::Server,
            ref_id: Some("mcp-1".into()),
            spend_cap_usd: None,
            rate_per_min: Some(5),
        },
    };
    assert_wire_contains(&frame, "server");
}

#[test]
fn policy_response_json_uses_camelcase_ref_id_spend_cap_rate_per_min() {
    let json = serde_json::to_string(&sample_trust_policy()).expect("serialize Policy");
    assert!(
        json.contains("\"refId\""),
        "Policy.ref_id must serialize as camelCase `refId`; got:\n{json}"
    );
    assert!(
        json.contains("\"spendCapUsd\""),
        "Policy.spend_cap_usd must serialize as camelCase `spendCapUsd`; got:\n{json}"
    );
    assert!(
        json.contains("\"ratePerMin\""),
        "Policy.rate_per_min must serialize as camelCase `ratePerMin`; got:\n{json}"
    );
    assert!(
        !json.contains("ref_id")
            && !json.contains("spend_cap_usd")
            && !json.contains("rate_per_min"),
        "generated JSON must not contain snake_case field names; got:\n{json}"
    );
}

#[test]
fn audit_row_response_json_uses_camelcase_invocation_id_and_carries_the_policy_deny_action() {
    let json = serde_json::to_string(&sample_audit_row()).expect("serialize AuditRow");
    assert!(
        json.contains("\"invocationId\""),
        "AuditRow.invocation_id must serialize as camelCase `invocationId`; got:\n{json}"
    );
    assert!(
        json.contains("\"policy_deny\""),
        "AuditRow.action must carry the spec §4 'policy_deny' literal verbatim; got:\n{json}"
    );
    assert!(
        !json.contains("invocation_id"),
        "generated JSON must not contain snake_case `invocation_id`; got:\n{json}"
    );
}

#[test]
fn trust_set_policy_null_cap_fields_roundtrip_as_unlimited() {
    // D11-style "absent/null = unlimited" — a policy with both caps `None` is legal (spec §4:
    // "null = unlimited").
    let req = OrchdRequest::TrustSetPolicy {
        scope: PolicyScope::Global,
        ref_id: None,
        spend_cap_usd: None,
        rate_per_min: None,
    };
    assert_frame_roundtrip(OrchdFrame::Request { id: 1, req });
}

// ---- S-IDEA research wire tests (spec §5, task T3) ----

#[test]
fn research_status_wire_tags_are_lowercase() {
    // Discriminating: exact tag equality (a broken `rename_all` producing "Pending" fails).
    assert_serde_tag(&ResearchStatus::Pending, "pending");
    assert_serde_tag(&ResearchStatus::Running, "running");
    assert_serde_tag(&ResearchStatus::Done, "done");
    assert_serde_tag(&ResearchStatus::Failed, "failed");
}

#[test]
fn research_run_entity_serializes_with_camelcase_keys() {
    let json = serde_json::to_string(&sample_research_run()).expect("serialize ResearchRun");
    assert!(
        json.contains("\"ideaId\""),
        "ResearchRun.idea_id must serialize as camelCase `ideaId`; got:\n{json}"
    );
    assert!(
        json.contains("\"serverId\""),
        "ResearchRun.server_id must serialize as camelCase `serverId`; got:\n{json}"
    );
    assert!(
        json.contains("\"toolName\""),
        "ResearchRun.tool_name must serialize as camelCase `toolName`; got:\n{json}"
    );
    assert!(
        json.contains("\"argsJson\""),
        "ResearchRun.args_json must serialize as camelCase `argsJson`; got:\n{json}"
    );
    assert!(
        json.contains("\"invocationId\""),
        "ResearchRun.invocation_id must serialize as camelCase `invocationId`; got:\n{json}"
    );
    assert!(
        json.contains("\"artifactId\""),
        "ResearchRun.artifact_id must serialize as camelCase `artifactId`; got:\n{json}"
    );
    assert!(
        json.contains("\"errorKind\""),
        "ResearchRun.error_kind must serialize as camelCase `errorKind`; got:\n{json}"
    );
    assert!(
        json.contains("\"createdAt\""),
        "ResearchRun.created_at must serialize as camelCase `createdAt`; got:\n{json}"
    );
    assert!(
        json.contains("\"updatedAt\""),
        "ResearchRun.updated_at must serialize as camelCase `updatedAt`; got:\n{json}"
    );
    assert!(
        !json.contains("idea_id")
            && !json.contains("server_id")
            && !json.contains("tool_name")
            && !json.contains("args_json")
            && !json.contains("invocation_id")
            && !json.contains("artifact_id")
            && !json.contains("error_kind")
            && !json.contains("created_at")
            && !json.contains("updated_at"),
        "generated JSON must not contain snake_case field names; got:\n{json}"
    );
}

#[test]
fn research_runs_response_json_roundtrips() {
    // Frame round-trip via `serde_json` directly (in addition to the CBOR-wire round-trip
    // exercised by `every_response_variant_roundtrips`): the frame stays plain snake_case even
    // though it wraps a camelCase entity.
    let original = OrchdResponse::ResearchRuns(vec![sample_research_run()]);
    let json = serde_json::to_string(&original).expect("serialize OrchdResponse::ResearchRuns");
    let decoded: OrchdResponse =
        serde_json::from_str(&json).expect("deserialize OrchdResponse::ResearchRuns");
    assert_eq!(
        decoded, original,
        "OrchdResponse::ResearchRuns must JSON round-trip byte-for-byte equal"
    );
}

#[test]
fn research_runs_response_cbor_frame_roundtrips_losslessly() {
    // Discriminating CBOR-wire check for the exact frame the brief calls out
    // (`OrchdResponse::ResearchRuns(vec![sample])`), snake_case frame carrying the camelCase
    // entity — mirrors `mcp_servers_response_json_roundtrips`'s companion CBOR coverage via
    // `every_response_variant_roundtrips`, made explicit here for this one variant.
    let frame = OrchdFrame::Response {
        id: 1,
        res: OrchdResponse::ResearchRuns(vec![sample_research_run()]),
    };
    assert_frame_roundtrip(frame);
}

#[test]
fn research_start_run_request_stays_snake_case_on_the_wire() {
    // The frame itself is Hop-B wire-only plain Rust snake_case (NOT ts-rs, NOT camelCase) — the
    // request's own field names must reach the CBOR bytes verbatim, unlike the camelCase entity
    // it responds with.
    let frame = OrchdFrame::Request {
        id: 1,
        req: OrchdRequest::ResearchStartRun {
            idea_id: "idea-1".into(),
            server_id: "mcp-1".into(),
            tool_name: "search".into(),
            args_json: "{}".into(),
        },
    };
    assert_wire_contains(&frame, "idea_id");
    assert_wire_contains(&frame, "server_id");
    assert_wire_contains(&frame, "tool_name");
    assert_wire_contains(&frame, "args_json");
}

// ---- SCN-046 / A-7 CEO supervisor: PolicyRules additive field ----

#[test]
fn policy_rules_round_trips_supervisor_config_over_cbor() {
    // The whole point of A-7: the CEO config rides inside PolicyRules, so it must survive the same
    // Hop-B CBOR round-trip every other entity does. `sample_policy` now carries a fully-populated
    // supervisor; the ruleset-view response variant already threads it through the wire.
    let frame = OrchdFrame::Response {
        id: 7,
        res: OrchdResponse::RuleSetView(sample_ruleset_view()),
    };
    // The delegated-class tags reach the raw wire bytes verbatim (checked before the round-trip
    // consumes the frame by value).
    assert_wire_contains(&frame, "safe-shell");
    assert_wire_contains(&frame, "delegatedClasses");
    assert_frame_roundtrip(frame);
}

#[test]
fn supervisor_config_json_round_trips_via_policy_rules() {
    let policy = sample_policy();
    let json = serde_json::to_string(&policy).expect("serialize");
    let back: PolicyRules = serde_json::from_str(&json).expect("deserialize");
    assert_eq!(policy, back);
    // camelCase field names on the JSON wire (ts-rs parity with the generated TS type).
    assert!(json.contains("delegatedClasses"), "got: {json}");
    assert!(json.contains("customRules"), "got: {json}");
}

#[test]
fn policy_rules_without_supervisor_key_decodes_to_default_disabled() {
    // A-7 "old rows/bundles decode": a PolicyRules JSON blob predating SCN-046 has no `supervisor`
    // key; `#[serde(default)]` on the field must backfill it to a disabled/empty config instead of
    // erroring on a missing field.
    let json = r#"{"spendCapUsd":null,"approvalClasses":[],"pathAllowlist":[]}"#;
    let policy: PolicyRules = serde_json::from_str(json).expect("legacy policy JSON must decode");
    assert_eq!(policy.supervisor, SupervisorConfig::default());
    assert!(!policy.supervisor.enabled);
    assert!(policy.supervisor.delegated_classes.is_empty());
    assert!(policy.supervisor.custom_rules.is_empty());
    assert_eq!(policy.supervisor.instruction, "");
}

#[test]
fn supervisor_config_default_is_disabled_empty() {
    let d = SupervisorConfig::default();
    assert!(!d.enabled);
    assert!(d.delegated_classes.is_empty());
    assert!(d.custom_rules.is_empty());
    assert_eq!(d.instruction, "");
}

// ---- SW1 Workflow authoring entity/verb tests ----

#[test]
fn workflow_scope_wire_tags_are_lowercase() {
    assert_serde_tag(&WorkflowScope::Global, "global");
    assert_serde_tag(&WorkflowScope::Project, "project");
}

#[test]
fn gate_wire_tags_are_lowercase() {
    assert_serde_tag(&Gate::Auto, "auto");
    assert_serde_tag(&Gate::Manual, "manual");
}

#[test]
fn context_scope_wire_tags_are_camelcase() {
    for (value, tag) in [
        (ContextScope::Inherit, "inherit"),
        (ContextScope::Handoff, "handoff"),
        (ContextScope::Project, "project"),
        (ContextScope::Selected, "selected"),
    ] {
        assert_serde_tag(&value, tag);
    }
}

#[test]
fn workflow_entity_serializes_with_camelcase_keys() {
    let json = serde_json::to_string(&sample_workflow()).expect("serialize Workflow");
    for key in [
        "\"projectId\"",
        "\"defaultAgent\"",
        "\"globalSkillIds\"",
        "\"fileState\"",
        "\"jsonPath\"",
        "\"createdAt\"",
        "\"updatedAt\"",
    ] {
        assert!(
            json.contains(key),
            "Workflow must serialize {key} as camelCase; got:\n{json}"
        );
    }
    // Stage's own camelCase keys (nested in `stages`).
    assert!(
        json.contains("\"skillIds\"") && json.contains("\"contextScope\""),
        "Stage.skill_ids/context_scope must serialize as camelCase; got:\n{json}"
    );
    assert!(
        !json.contains("project_id")
            && !json.contains("default_agent")
            && !json.contains("global_skill_ids")
            && !json.contains("file_state")
            && !json.contains("json_path")
            && !json.contains("skill_ids")
            && !json.contains("context_scope"),
        "generated JSON must not contain snake_case field names; got:\n{json}"
    );
}

#[test]
fn workflow_response_json_roundtrips_the_full_definition() {
    // Mirrors `skill_list_response_json_roundtrips`: a plain-`serde_json` round-trip in addition to
    // the CBOR-wire round-trip `every_response_variant_roundtrips` already exercises. Proves the
    // FULL definition (stages incl. agent/contextScope/outputs, globals, supervisor) survives.
    let original = OrchdResponse::Workflow(sample_workflow());
    let json = serde_json::to_string(&original).expect("serialize OrchdResponse::Workflow");
    let decoded: OrchdResponse =
        serde_json::from_str(&json).expect("deserialize OrchdResponse::Workflow");
    assert_eq!(
        decoded, original,
        "OrchdResponse::Workflow must JSON round-trip byte-for-byte equal"
    );
}
