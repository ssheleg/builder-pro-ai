use std::fs;
use std::path::PathBuf;

use bpa_orchd_proto::*;
use ts_rs::TS;

/// Absolute path to the generated shared TS file (repo root `src/ipc/orchd-types.ts`).
fn types_ts_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../src/ipc")
}

fn types_ts_path() -> PathBuf {
    types_ts_dir().join("orchd-types.ts")
}

/// Force every exported entity/enum type to (re)write its TS binding, then read the file
/// back. `export_all_to` disregards `TS_RS_EXPORT_DIR`, so the output directory is
/// deterministic regardless of environment or working directory. Each type's
/// `#[ts(export_to = "orchd-types.ts")]` attribute is just the filename, so every type gets
/// merged into a single `orchd-types.ts` under `out_dir` (mirrors
/// `crates/protocol/tests/ts_export.rs`).
fn export_and_read() -> String {
    Project::export_all_to(types_ts_dir()).expect("export Project");
    ProjectStatus::export_all_to(types_ts_dir()).expect("export ProjectStatus");
    Goal::export_all_to(types_ts_dir()).expect("export Goal");
    GoalKind::export_all_to(types_ts_dir()).expect("export GoalKind");
    GoalStatus::export_all_to(types_ts_dir()).expect("export GoalStatus");
    Idea::export_all_to(types_ts_dir()).expect("export Idea");
    IdeaLifecycle::export_all_to(types_ts_dir()).expect("export IdeaLifecycle");
    Insight::export_all_to(types_ts_dir()).expect("export Insight");
    FitVerdict::export_all_to(types_ts_dir()).expect("export FitVerdict");
    InsightStatus::export_all_to(types_ts_dir()).expect("export InsightStatus");
    DomainTask::export_all_to(types_ts_dir()).expect("export DomainTask");
    TaskStatus::export_all_to(types_ts_dir()).expect("export TaskStatus");
    TaskSource::export_all_to(types_ts_dir()).expect("export TaskSource");
    PolicyRules::export_all_to(types_ts_dir()).expect("export PolicyRules");
    RuleSet::export_all_to(types_ts_dir()).expect("export RuleSet");
    RuleScope::export_all_to(types_ts_dir()).expect("export RuleScope");
    RuleFileState::export_all_to(types_ts_dir()).expect("export RuleFileState");
    RuleSetView::export_all_to(types_ts_dir()).expect("export RuleSetView");
    OrchdErrorCode::export_all_to(types_ts_dir()).expect("export OrchdErrorCode");
    GraphNode::export_all_to(types_ts_dir()).expect("export GraphNode");
    GraphNodeKind::export_all_to(types_ts_dir()).expect("export GraphNodeKind");
    GraphEntityType::export_all_to(types_ts_dir()).expect("export GraphEntityType");
    GraphEdge::export_all_to(types_ts_dir()).expect("export GraphEdge");
    GraphEdgeKind::export_all_to(types_ts_dir()).expect("export GraphEdgeKind");
    GraphView::export_all_to(types_ts_dir()).expect("export GraphView");
    GraphNeighborhood::export_all_to(types_ts_dir()).expect("export GraphNeighborhood");
    McpServer::export_all_to(types_ts_dir()).expect("export McpServer");
    McpTransport::export_all_to(types_ts_dir()).expect("export McpTransport");
    McpScope::export_all_to(types_ts_dir()).expect("export McpScope");
    McpAuthKind::export_all_to(types_ts_dir()).expect("export McpAuthKind");
    McpTool::export_all_to(types_ts_dir()).expect("export McpTool");
    McpConnectReport::export_all_to(types_ts_dir()).expect("export McpConnectReport");
    McpCallResult::export_all_to(types_ts_dir()).expect("export McpCallResult");
    McpInvocation::export_all_to(types_ts_dir()).expect("export McpInvocation");
    McpArtifact::export_all_to(types_ts_dir()).expect("export McpArtifact");
    Account::export_all_to(types_ts_dir()).expect("export Account");
    AccountAuthKind::export_all_to(types_ts_dir()).expect("export AccountAuthKind");
    ConnectorOp::export_all_to(types_ts_dir()).expect("export ConnectorOp");
    OAuthChallenge::export_all_to(types_ts_dir()).expect("export OAuthChallenge");
    Skill::export_all_to(types_ts_dir()).expect("export Skill");
    SkillScope::export_all_to(types_ts_dir()).expect("export SkillScope");
    SkillFileState::export_all_to(types_ts_dir()).expect("export SkillFileState");
    Policy::export_all_to(types_ts_dir()).expect("export Policy");
    PolicyScope::export_all_to(types_ts_dir()).expect("export PolicyScope");
    AuditRow::export_all_to(types_ts_dir()).expect("export AuditRow");
    fs::read_to_string(types_ts_path()).expect("read generated orchd-types.ts")
}

/// Whitespace- and quote-insensitive substring check so we assert structure, not
/// formatting (mirrors `crates/protocol/tests/ts_export.rs::contains_normalized`).
fn contains_normalized(haystack: &str, needle: &str) -> bool {
    let strip = |s: &str| s.split_whitespace().collect::<String>().replace('"', "");
    strip(haystack).contains(&strip(needle))
}

#[test]
fn generates_orchd_types_ts_at_shared_path() {
    let ts = export_and_read();
    assert!(!ts.is_empty(), "orchd-types.ts must not be empty");
    assert!(
        types_ts_path().exists(),
        "orchd-types.ts must exist at src/ipc/orchd-types.ts"
    );
}

#[test]
fn project_uses_camelcase_workspace_ids() {
    let ts = export_and_read();
    assert!(
        contains_normalized(&ts, "projectId") || ts.contains("export type Project"),
        "sanity: Project type present; got:\n{ts}"
    );
    assert!(
        contains_normalized(&ts, "workspaceIds: Array<string>")
            || contains_normalized(&ts, "workspaceIds: string[]"),
        "Project.workspace_ids must serialize as camelCase `workspaceIds`; got:\n{ts}"
    );
    assert!(
        !ts.contains("workspace_ids"),
        "generated TS must not contain snake_case `workspace_ids`"
    );
}

#[test]
fn goal_uses_camelcase_project_id_and_metric_refs() {
    let ts = export_and_read();
    assert!(
        contains_normalized(&ts, "projectId: string"),
        "Goal.project_id must serialize as camelCase `projectId`; got:\n{ts}"
    );
    assert!(
        contains_normalized(&ts, "metricRefs: Array<string>")
            || contains_normalized(&ts, "metricRefs: string[]"),
        "Goal.metric_refs must serialize as camelCase `metricRefs`; got:\n{ts}"
    );
    assert!(
        !ts.contains("metric_refs"),
        "generated TS must not contain snake_case `metric_refs`"
    );
}

#[test]
fn insight_uses_camelcase_fit_verdict() {
    let ts = export_and_read();
    assert!(
        contains_normalized(&ts, "fitVerdict"),
        "Insight.fit_verdict must serialize as camelCase `fitVerdict`; got:\n{ts}"
    );
    assert!(
        !ts.contains("fit_verdict"),
        "generated TS must not contain snake_case `fit_verdict`"
    );
}

#[test]
fn ruleset_uses_camelcase_md_path() {
    let ts = export_and_read();
    assert!(
        contains_normalized(&ts, "mdPath: string"),
        "RuleSet.md_path must serialize as camelCase `mdPath`; got:\n{ts}"
    );
    assert!(
        !ts.contains("md_path"),
        "generated TS must not contain snake_case `md_path`"
    );
}

#[test]
fn idea_lifecycle_wire_tags_are_camelcase() {
    let ts = export_and_read();
    for tag in [
        "captured",
        "researching",
        "specced",
        "inDev",
        "shipped",
        "archived",
    ] {
        assert!(
            contains_normalized(&ts, &format!("\"{tag}\"")),
            "IdeaLifecycle must include wire tag {tag:?}; got:\n{ts}"
        );
    }
    assert!(
        !ts.contains("in_dev"),
        "generated TS must not contain snake_case `in_dev`"
    );
}

#[test]
fn fit_verdict_wire_tags_are_camelcase() {
    let ts = export_and_read();
    for tag in ["fit", "noFit", "unknown"] {
        assert!(
            contains_normalized(&ts, &format!("\"{tag}\"")),
            "FitVerdict must include wire tag {tag:?}; got:\n{ts}"
        );
    }
    assert!(
        !ts.contains("no_fit"),
        "generated TS must not contain snake_case `no_fit`"
    );
}

#[test]
fn task_status_backlog_tag_present() {
    let ts = export_and_read();
    assert!(
        contains_normalized(&ts, "\"backlog\""),
        "TaskStatus must include wire tag \"backlog\"; got:\n{ts}"
    );
}

#[test]
fn domain_task_rank_is_ts_number() {
    let ts = export_and_read();
    assert!(
        contains_normalized(&ts, "rank: number"),
        "DomainTask.rank (f64) must be TS `number`; got:\n{ts}"
    );
}

#[test]
fn no_snake_case_leakage_anywhere_in_generated_file() {
    let ts = export_and_read();
    for snake in [
        "project_id",
        "workspace_ids",
        "parent_id",
        "metric_refs",
        "fit_verdict",
        "fit_reasoning",
        "resolution_reasoning",
        "source_id",
        "rank_agent",
        "rank_agent_reasoning",
        "created_at",
        "updated_at",
        "md_path",
        "md_hash",
        "md_content",
        "file_state",
        "spend_cap_usd",
        "approval_classes",
        "path_allowlist",
        "entity_type",
        "entity_id",
        "pos_x",
        "pos_y",
        "source_node_id",
        "target_node_id",
        "external_nodes",
        "root_id",
        "server_id",
        "tool_name",
        "input_schema_json",
        "fetched_at",
        "timeout_ms",
        "max_retries",
        "auth_kind",
        "secret_ref",
        "account_id",
        "protocol_version",
        "tool_count",
        "artifact_id",
        "invocation_id",
        "content_json",
        "content_text",
        "is_untrusted",
        "is_error",
        "request_hash",
        "error_kind",
        "latency_ms",
        "cost_usd",
        "input_tokens",
        "output_tokens",
        "started_at",
        "expires_at",
        "authorize_url",
        "ref_id",
        "rate_per_min",
    ] {
        assert!(
            !ts.contains(snake),
            "generated orchd-types.ts must not contain snake_case `{snake}`; got:\n{ts}"
        );
    }
}

#[test]
fn graph_node_and_edge_use_camelcase_fields_and_ts_number_timestamps() {
    let ts = export_and_read();
    assert!(
        contains_normalized(&ts, "export type GraphNode"),
        "GraphNode type must be present; got:\n{ts}"
    );
    assert!(
        contains_normalized(&ts, "export type GraphEdge"),
        "GraphEdge type must be present; got:\n{ts}"
    );
    assert!(
        contains_normalized(&ts, "export type GraphView"),
        "GraphView type must be present; got:\n{ts}"
    );
    assert!(
        contains_normalized(&ts, "export type GraphNeighborhood"),
        "GraphNeighborhood type must be present; got:\n{ts}"
    );
    assert!(
        contains_normalized(&ts, "projectId: string"),
        "GraphNode.project_id must serialize as camelCase `projectId`; got:\n{ts}"
    );
    assert!(
        contains_normalized(&ts, "posX: number") && contains_normalized(&ts, "posY: number"),
        "GraphNode.pos_x/pos_y must serialize as camelCase `posX`/`posY`; got:\n{ts}"
    );
    assert!(
        contains_normalized(&ts, "entityType: GraphEntityType | null"),
        "GraphNode.entity_type must serialize as camelCase `entityType`; got:\n{ts}"
    );
    assert!(
        contains_normalized(&ts, "sourceNodeId: string"),
        "GraphEdge.source_node_id must serialize as camelCase `sourceNodeId`; got:\n{ts}"
    );
    assert!(
        contains_normalized(&ts, "externalNodes: Array<GraphNode>"),
        "GraphView.external_nodes must serialize as camelCase `externalNodes`; got:\n{ts}"
    );
    assert!(
        contains_normalized(&ts, "rootId: string"),
        "GraphNeighborhood.root_id must serialize as camelCase `rootId`; got:\n{ts}"
    );
    assert!(
        contains_normalized(&ts, "createdAt: number"),
        "GraphNode/GraphEdge.created_at (i64) must be TS `number`, not bigint; got:\n{ts}"
    );
    assert!(
        contains_normalized(&ts, "updatedAt: number"),
        "GraphNode.updated_at (i64) must be TS `number`, not bigint; got:\n{ts}"
    );
    assert!(
        !ts.contains("bigint"),
        "generated orchd-types.ts must never contain `bigint`; got:\n{ts}"
    );
}

#[test]
fn graph_node_kind_and_edge_kind_and_entity_type_wire_tags_are_camelcase() {
    let ts = export_and_read();
    for tag in [
        "concept",
        "fact",
        "artifact",
        "decision",
        "note",
        "entityRef",
    ] {
        assert!(
            contains_normalized(&ts, &format!("\"{tag}\"")),
            "GraphNodeKind must include wire tag {tag:?}; got:\n{ts}"
        );
    }
    for tag in [
        "relates",
        "depends",
        "derives",
        "supports",
        "contradicts",
        "parent",
    ] {
        assert!(
            contains_normalized(&ts, &format!("\"{tag}\"")),
            "GraphEdgeKind must include wire tag {tag:?}; got:\n{ts}"
        );
    }
    for tag in ["goal", "idea", "insight", "task"] {
        assert!(
            contains_normalized(&ts, &format!("\"{tag}\"")),
            "GraphEntityType must include wire tag {tag:?}; got:\n{ts}"
        );
    }
    assert!(
        !ts.contains("entity_ref") && !ts.contains("Ruleset") && !ts.contains("\"ruleset\""),
        "generated orchd-types.ts must not contain snake_case entity_ref or a Ruleset entity type; got:\n{ts}"
    );
}

#[test]
fn regenerating_is_byte_identical() {
    let first = export_and_read();
    let second = export_and_read();
    assert_eq!(
        first, second,
        "regenerating orchd-types.ts must be byte-identical (no nondeterminism)"
    );
}

// ---- S-EXT MCP entity export tests (task T3) ----

#[test]
fn mcp_entities_are_present_with_camelcase_fields_and_ts_number_timestamps() {
    let ts = export_and_read();
    for expected in [
        "export type McpServer",
        "export type McpTool",
        "export type McpConnectReport",
        "export type McpCallResult",
        "export type McpInvocation",
        "export type McpArtifact",
    ] {
        assert!(
            contains_normalized(&ts, expected),
            "expected {expected:?} in generated orchd-types.ts; got:\n{ts}"
        );
    }
    assert!(
        contains_normalized(&ts, "createdAt: number"),
        "McpServer/McpArtifact.created_at (i64) must be TS `number`, not bigint; got:\n{ts}"
    );
    assert!(
        contains_normalized(&ts, "projectId: string | null"),
        "McpServer.project_id must serialize as camelCase `projectId`; got:\n{ts}"
    );
    assert!(
        contains_normalized(&ts, "authKind: McpAuthKind"),
        "McpServer.auth_kind must serialize as camelCase `authKind`; got:\n{ts}"
    );
    assert!(
        contains_normalized(&ts, "toolCount: number"),
        "McpConnectReport.tool_count (i64) must be TS `number`, not bigint; got:\n{ts}"
    );
    assert!(
        contains_normalized(&ts, "artifactId: string"),
        "McpCallResult.artifact_id must serialize as camelCase `artifactId`; got:\n{ts}"
    );
    assert!(
        contains_normalized(&ts, "inputTokens: number | null")
            && contains_normalized(&ts, "outputTokens: number | null"),
        "McpInvocation.input_tokens/output_tokens (Option<i64>) must be TS `number | null`, \
         not `bigint | null`; got:\n{ts}"
    );
    assert!(
        !ts.contains("bigint"),
        "generated orchd-types.ts must never contain `bigint`; got:\n{ts}"
    );
}

#[test]
fn mcp_transport_scope_auth_kind_wire_tags_are_camelcase() {
    let ts = export_and_read();
    for tag in ["http", "stdio"] {
        assert!(
            contains_normalized(&ts, &format!("\"{tag}\"")),
            "McpTransport must include wire tag {tag:?}; got:\n{ts}"
        );
    }
    for tag in ["global", "project"] {
        assert!(
            contains_normalized(&ts, &format!("\"{tag}\"")),
            "McpScope must include wire tag {tag:?}; got:\n{ts}"
        );
    }
    for tag in ["none", "bearer", "oauth"] {
        assert!(
            contains_normalized(&ts, &format!("\"{tag}\"")),
            "McpAuthKind must include wire tag {tag:?}; got:\n{ts}"
        );
    }
}

// ---- S-EXT connector/OAuth entity export tests (task T10, spec §5/§7 Phase-2 subset) ----

#[test]
fn connector_entities_are_present_with_camelcase_fields_and_ts_number_timestamps() {
    let ts = export_and_read();
    for expected in [
        "export type Account",
        "export type ConnectorOp",
        "export type OAuthChallenge",
    ] {
        assert!(
            contains_normalized(&ts, expected),
            "expected {expected:?} in generated orchd-types.ts; got:\n{ts}"
        );
    }
    assert!(
        contains_normalized(&ts, "authKind: AccountAuthKind"),
        "Account.auth_kind must serialize as camelCase `authKind`; got:\n{ts}"
    );
    assert!(
        contains_normalized(&ts, "expiresAt: number | null"),
        "Account.expires_at (Option<i64>) must be TS `number | null`, not `bigint | null`; got:\n{ts}"
    );
    assert!(
        contains_normalized(&ts, "createdAt: number"),
        "Account.created_at (i64) must be TS `number`, not bigint; got:\n{ts}"
    );
    assert!(
        contains_normalized(&ts, "authorizeUrl: string"),
        "OAuthChallenge.authorize_url must serialize as camelCase `authorizeUrl`; got:\n{ts}"
    );
    // Scoped to the `Account` type's own definition line — `secretRef`/`refreshRef` legitimately
    // appear elsewhere in the file (`McpServer.secretRef`), so a whole-file substring check would
    // false-positive on that unrelated type.
    let account_line = ts
        .lines()
        .find(|l| l.contains("export type Account ="))
        .expect("Account type definition line present");
    assert!(
        !account_line.contains("secretRef") && !account_line.contains("secret_ref"),
        "Account must NOT expose secret_ref on the generated TS (Keychain key structure); got:\n{account_line}"
    );
    assert!(
        !account_line.contains("refreshRef") && !account_line.contains("refresh_ref"),
        "Account must NOT expose refresh_ref on the generated TS (Keychain key structure); got:\n{account_line}"
    );
    assert!(
        !ts.contains("bigint"),
        "generated orchd-types.ts must never contain `bigint`; got:\n{ts}"
    );
}

#[test]
fn account_auth_kind_wire_tags_are_camelcase() {
    let ts = export_and_read();
    for tag in ["oauth", "apikey"] {
        assert!(
            contains_normalized(&ts, &format!("\"{tag}\"")),
            "AccountAuthKind must include wire tag {tag:?}; got:\n{ts}"
        );
    }
}

#[test]
fn skill_uses_camelcase_md_path_md_hash_and_file_state() {
    let ts = export_and_read();
    assert!(
        contains_normalized(&ts, "mdPath: string"),
        "Skill.md_path must serialize as camelCase `mdPath`; got:\n{ts}"
    );
    assert!(
        contains_normalized(&ts, "mdHash: string"),
        "Skill.md_hash must serialize as camelCase `mdHash`; got:\n{ts}"
    );
    assert!(
        contains_normalized(&ts, "fileState: SkillFileState"),
        "Skill.file_state must serialize as camelCase `fileState`; got:\n{ts}"
    );
    assert!(
        contains_normalized(&ts, "createdAt: number"),
        "Skill.created_at (i64) must be TS `number`, not bigint; got:\n{ts}"
    );
    assert!(
        !ts.contains("md_path") && !ts.contains("md_hash") && !ts.contains("file_state"),
        "generated TS must not contain snake_case `md_path`/`md_hash`/`file_state`"
    );
}

#[test]
fn skill_file_state_wire_tags_are_camelcase() {
    let ts = export_and_read();
    for tag in ["present", "modified", "missing"] {
        assert!(
            contains_normalized(&ts, &format!("\"{tag}\"")),
            "SkillFileState must include wire tag {tag:?}; got:\n{ts}"
        );
    }
}

// ---- S-EXT Trust entity export tests (spec §4/§5/§6, BL-22, task T18) ----

#[test]
fn policy_and_audit_row_are_present_with_camelcase_fields_and_ts_number_timestamps() {
    let ts = export_and_read();
    for expected in ["export type Policy", "export type AuditRow"] {
        assert!(
            contains_normalized(&ts, expected),
            "expected {expected:?} in generated orchd-types.ts; got:\n{ts}"
        );
    }
    assert!(
        contains_normalized(&ts, "refId: string | null"),
        "Policy.ref_id must serialize as camelCase `refId`; got:\n{ts}"
    );
    assert!(
        contains_normalized(&ts, "spendCapUsd: number | null"),
        "Policy.spend_cap_usd must serialize as camelCase `spendCapUsd`; got:\n{ts}"
    );
    assert!(
        contains_normalized(&ts, "ratePerMin: number | null"),
        "Policy.rate_per_min (Option<i64>) must serialize as camelCase `ratePerMin`, TS \
         `number | null`, not `bigint | null`; got:\n{ts}"
    );
    assert!(
        contains_normalized(&ts, "invocationId: string | null"),
        "AuditRow.invocation_id must serialize as camelCase `invocationId`; got:\n{ts}"
    );
    assert!(
        !ts.contains("bigint"),
        "generated orchd-types.ts must never contain `bigint`; got:\n{ts}"
    );
}

#[test]
fn policy_scope_wire_tags_are_camelcase() {
    let ts = export_and_read();
    for tag in ["global", "project", "server"] {
        assert!(
            contains_normalized(&ts, &format!("\"{tag}\"")),
            "PolicyScope must include wire tag {tag:?}; got:\n{ts}"
        );
    }
}
