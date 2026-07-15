//! Wire contract for `bpa-orchd` — the S3 domain daemon (spec §4.2, entities: projects,
//! goals, ideas, insights, tasks, rulesets).
//!
//! Enum variant order is FROZEN append-only from day one (spec §4.2 heading). Codec is CBOR
//! (RFC 8949, via `bpa_protocol`'s generic `encode_cbor_frame`/`CborFrameDecoder<T>`, spec
//! §4.1); framing lives entirely in `bpa-protocol`, this crate only supplies `T = OrchdFrame`
//! instantiations (mirrors `crates/protocol/src/lib.rs`'s `Frame`/`encode_frame`/
//! `FrameDecoder` thin-wrapper pattern).
//!
//! Every type derives `Serialize`/`Deserialize` (CBOR-compatible). The **entities** (`Project`
//! … `RuleSetView`) and their enums, plus `OrchdErrorCode`, additionally derive `ts_rs::TS` and
//! `#[serde(rename_all = "camelCase")]` — they are the source of truth for the generated
//! `src/ipc/orchd-types.ts` (never hand-edited). The **frame** types (`OrchdRequest`,
//! `OrchdResponse`, `OrchdPush`, `OrchdFrame`) are Hop-B wire-only (core ⇄ `bpa-orchd`) and are
//! NOT exported to TS — same as `bpa-protocol`'s `Request`/`Response`/`Push`/`Frame` — so their
//! field names stay plain Rust snake_case on the wire.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// `pub const ORCHD_CLIENT_MIN_VERSION: u16 = 1;` etc. (spec §4.2). All four pinned to `1` —
/// this is the first wire version of `bpa-orchd`; `negotiate()` (from `bpa_protocol::preamble`)
/// is reused as-is, orchd just passes `(1, 1)` on both sides (spec §4.1).
pub const ORCHD_CLIENT_MIN_VERSION: u16 = 1;
pub const ORCHD_CLIENT_MAX_VERSION: u16 = 1;
pub const ORCHD_DAEMON_MIN_VERSION: u16 = 1;
pub const ORCHD_DAEMON_MAX_VERSION: u16 = 1;

// ================================================================================
// ---- entities (spec §4.2) ----
// ================================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "orchd-types.ts")]
pub struct Project {
    pub id: String,
    pub name: String,
    pub description: String,
    pub status: ProjectStatus,
    /// ordered, soft refs to sessiond
    pub workspace_ids: Vec<String>,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number")]
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "orchd-types.ts")]
pub enum ProjectStatus {
    Active,
    Archived,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "orchd-types.ts")]
pub struct Goal {
    pub id: String,
    pub project_id: String,
    pub parent_id: Option<String>,
    pub kind: GoalKind,
    pub title: String,
    pub body: String,
    #[ts(type = "number")]
    pub ord: i64,
    pub status: GoalStatus,
    pub metric_refs: Vec<String>,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number")]
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "orchd-types.ts")]
pub enum GoalKind {
    Strategic,
    Additional,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "orchd-types.ts")]
pub enum GoalStatus {
    Active,
    Achieved,
    Dropped,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "orchd-types.ts")]
pub struct Idea {
    pub id: String,
    pub project_id: Option<String>,
    pub title: String,
    pub body: String,
    pub lifecycle: IdeaLifecycle,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number")]
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "orchd-types.ts")]
pub enum IdeaLifecycle {
    Captured,
    Researching,
    Specced,
    InDev,
    Shipped,
    Archived,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "orchd-types.ts")]
pub struct Insight {
    pub id: String,
    pub project_id: Option<String>,
    pub source: String,
    pub title: String,
    pub body: String,
    pub fit_verdict: Option<FitVerdict>,
    pub fit_reasoning: String,
    pub status: InsightStatus,
    pub resolution_reasoning: String,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number")]
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "orchd-types.ts")]
pub enum FitVerdict {
    Fit,
    NoFit,
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "orchd-types.ts")]
pub enum InsightStatus {
    New,
    Accepted,
    Archived,
}

/// named `DomainTask` to avoid the `tokio::task` clash (spec §4.2).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "orchd-types.ts")]
pub struct DomainTask {
    pub id: String,
    pub project_id: String,
    pub parent_id: Option<String>,
    pub title: String,
    pub body: String,
    pub status: TaskStatus,
    pub source: TaskSource,
    pub source_id: Option<String>,
    pub tags: Vec<String>,
    pub rank: f64,
    pub rank_agent: Option<f64>,
    pub rank_agent_reasoning: String,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number")]
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "orchd-types.ts")]
pub enum TaskStatus {
    Backlog,
    Todo,
    Waiting,
    Progress,
    Testing,
    Done,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "orchd-types.ts")]
pub enum TaskSource {
    Idea,
    Insight,
    Bug,
    Plan,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "orchd-types.ts")]
pub struct PolicyRules {
    pub spend_cap_usd: Option<f64>,
    pub approval_classes: Vec<String>,
    pub path_allowlist: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "orchd-types.ts")]
pub struct RuleSet {
    pub id: String,
    pub scope: RuleScope,
    pub project_id: Option<String>,
    pub md_path: String,
    pub md_hash: String,
    pub policy: PolicyRules,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number")]
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "orchd-types.ts")]
pub enum RuleScope {
    Global,
    Project,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "orchd-types.ts")]
pub enum RuleFileState {
    Ok,
    Missing,
    ExternallyModified,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "orchd-types.ts")]
pub struct RuleSetView {
    pub rule: RuleSet,
    pub md_content: Option<String>,
    pub file_state: RuleFileState,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "orchd-types.ts")]
pub enum OrchdErrorCode {
    NotFound,
    Invariant,
    Validation,
    Conflict,
    Io,
    // S-EXT MCP trust choke-point (spec §6, D10, task T6, appended — order FROZEN append-only
    // like every other block in this file). `Consent`: an `McpConnect` denied because no valid
    // `consent_grant` exists for the server's CURRENT url (first connect, or a fingerprint
    // mismatch after the url changed, spec D10) — lets a client show the consent dialog
    // specifically rather than a generic failure. `Policy`: an `McpCallTool` denied because the
    // tool is disabled/unrecognized (the per-tool allowlist, spec §6) — surfaced BEFORE any
    // network/persistence access ever happens (T5's own choke-point guarantee). Neither existed
    // before this task: `mcp::OrchdMcpError::{ConsentRequired,ToolDisabled}` (T5) had no wire
    // code to map onto — T5's own doc comment flagged this gap explicitly.
    Consent,
    Policy,
}

// ---- S4 knowledge graph entities (spec §3, appended — order FROZEN append-only) ----

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "orchd-types.ts")]
pub struct GraphNode {
    pub id: String,
    pub project_id: String,
    pub kind: GraphNodeKind,
    pub entity_type: Option<GraphEntityType>,
    pub entity_id: Option<String>,
    pub label: String,
    pub body: String,
    pub pos_x: f64,
    pub pos_y: f64,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number")]
    pub updated_at: i64,
    /// `true` when this is an `entityRef` node whose referenced domain entity has been deleted
    /// (D3 soft-ref orphan). Set by the read-time label resolver (`resolve_node_label` in
    /// `crates/orchd/src/graph.rs`), never by the client — the UI renders an orphaned node with
    /// «источник удалён». Always `false` for non-`entityRef` nodes and for a still-live
    /// `entityRef` node.
    pub is_orphan: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "orchd-types.ts")]
pub enum GraphNodeKind {
    Concept,
    Fact,
    Artifact,
    Decision,
    Note,
    EntityRef,
}

// NO ruleset variant here: RuleSet has no title/label field, so an entityRef to it would be
// unresolvable; nothing in S4 creates one (only the D6 strategic-goal seed).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "orchd-types.ts")]
pub enum GraphEntityType {
    Goal,
    Idea,
    Insight,
    Task,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "orchd-types.ts")]
pub struct GraphEdge {
    pub id: String,
    pub source_node_id: String,
    pub target_node_id: String,
    pub kind: GraphEdgeKind,
    pub label: String,
    #[ts(type = "number")]
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "orchd-types.ts")]
pub enum GraphEdgeKind {
    Relates,
    Depends,
    Derives,
    Supports,
    Contradicts,
    Parent,
}

// Retrieval result: the project's own nodes + all incident edges + the foreign endpoints
// (cross-project "ghosts") so the UI can render boundary edges.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "orchd-types.ts")]
pub struct GraphView {
    // nodes belonging to the queried project.
    pub nodes: Vec<GraphNode>,
    // every edge incident to any of `nodes`.
    pub edges: Vec<GraphEdge>,
    // foreign endpoints of cross-project edges (ghosts).
    pub external_nodes: Vec<GraphNode>,
}

/// Subgraph within N hops of a start node, cross-project (the agent retrieval query).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "orchd-types.ts")]
pub struct GraphNeighborhood {
    pub root_id: String,
    pub nodes: Vec<GraphNode>,
    pub edges: Vec<GraphEdge>,
}

// ---- S-EXT MCP entities (spec §5, appended — order FROZEN append-only) ----
// Phase-1 subset: server/tool registry + call results + invocation/artifact history. Field
// sets mirror the spec §4 `mcp_server`/`mcp_tool`/`mcp_invocation`/`mcp_artifact` DDL columns
// 1:1 (same field names as `bpa_orchd::mcp::{McpServerRow, McpToolRow}` from T2), with the
// usual camelCase + ts-rs wire treatment layered on top (mirrors the `GraphNode` entity block
// byte-for-byte). `secret_ref`/`account_id` are Keychain/account REFERENCES, never the secret
// value itself (spec §5: "token -> Keychain, ref -> DB; token NEVER logged/echoed").

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "orchd-types.ts")]
pub struct McpServer {
    pub id: String,
    pub name: String,
    pub transport: McpTransport,
    pub url: Option<String>,
    pub command: Option<String>,
    pub args: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub scope: McpScope,
    pub project_id: Option<String>,
    pub auth_kind: McpAuthKind,
    pub secret_ref: Option<String>,
    pub account_id: Option<String>,
    pub enabled: bool,
    #[ts(type = "number")]
    pub timeout_ms: i64,
    #[ts(type = "number")]
    pub max_retries: i64,
    /// last negotiated MCP protocol version; `null` until first successful `McpConnect`.
    pub protocol_version: Option<String>,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number")]
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "orchd-types.ts")]
pub enum McpTransport {
    Http,
    Stdio,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "orchd-types.ts")]
pub enum McpScope {
    Global,
    Project,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "orchd-types.ts")]
pub enum McpAuthKind {
    None,
    Bearer,
    Oauth,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "orchd-types.ts")]
pub struct McpTool {
    pub id: String,
    pub server_id: String,
    pub name: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub input_schema_json: String,
    pub enabled: bool,
    #[ts(type = "number")]
    pub fetched_at: i64,
}

/// `OrchdRequest::McpConnect`'s success payload (spec §5).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "orchd-types.ts")]
pub struct McpConnectReport {
    pub protocol_version: String,
    #[ts(type = "number")]
    pub tool_count: i64,
}

/// `OrchdRequest::McpCallTool` / `ConnectorInvoke`'s success payload (spec §5); the JSON result
/// is the tool's full structured output, already persisted as a durable artifact row.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "orchd-types.ts")]
pub struct McpCallResult {
    pub artifact_id: String,
    pub invocation_id: String,
    pub content_json: String,
    pub is_error: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "orchd-types.ts")]
pub struct McpInvocation {
    pub id: String,
    pub server_id: String,
    pub tool_name: String,
    pub project_id: Option<String>,
    /// sha256 of args, NEVER the args themselves (spec §4: no arg content logged).
    pub request_hash: String,
    pub ok: bool,
    pub error_kind: Option<String>,
    #[ts(type = "number")]
    pub latency_ms: i64,
    pub cost_usd: Option<f64>,
    #[ts(type = "number | null")]
    pub input_tokens: Option<i64>,
    #[ts(type = "number | null")]
    pub output_tokens: Option<i64>,
    #[ts(type = "number")]
    pub started_at: i64,
}

/// Durable tool-call result (spec §4: "untrusted by construction"). The untrusted flag is
/// always `true` for every artifact this Phase-1 slice creates — the S6b agent-boundary flag
/// this quarantines against, not something a client can ever clear via the wire.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "orchd-types.ts")]
pub struct McpArtifact {
    pub id: String,
    pub invocation_id: String,
    pub server_id: String,
    pub tool_name: String,
    pub project_id: Option<String>,
    pub content_json: String,
    pub content_text: Option<String>,
    pub is_untrusted: bool,
    #[ts(type = "number")]
    pub created_at: i64,
}

// ================================================================================
// ---- frames (spec §4.2). Hop-B wire only (core ⇄ bpa-orchd). NOT exported to TS. ----
// ================================================================================

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum OrchdRequest {
    Ping,
    // Project
    /// `workspace_ids` ≥1 enforced by the persistence layer.
    CreateProject {
        name: String,
        description: String,
        workspace_ids: Vec<String>,
    },
    UpdateProject {
        id: String,
        name: Option<String>,
        description: Option<String>,
    },
    ArchiveProject {
        id: String,
    },
    ListProjects,
    AddProjectWorkspace {
        project_id: String,
        workspace_id: String,
    },
    /// removing the last workspace ⇒ `OrchdErrorCode::Invariant`.
    RemoveProjectWorkspace {
        project_id: String,
        workspace_id: String,
    },
    // Goal
    CreateGoal {
        project_id: String,
        parent_id: Option<String>,
        kind: GoalKind,
        title: String,
        body: String,
    },
    UpdateGoal {
        id: String,
        title: Option<String>,
        body: Option<String>,
        status: Option<GoalStatus>,
        metric_refs: Option<Vec<String>>,
    },
    /// same project only.
    MoveGoal {
        id: String,
        new_parent_id: Option<String>,
        new_ord: i64,
    },
    /// cascades subtree; deleting the strategic root ⇒ `OrchdErrorCode::Invariant`.
    DeleteGoal {
        id: String,
    },
    ListGoals {
        project_id: String,
    },
    // Idea
    CreateIdea {
        project_id: Option<String>,
        title: String,
        body: String,
    },
    UpdateIdea {
        id: String,
        title: Option<String>,
        body: Option<String>,
    },
    /// D11: dedicated verb, `project_id: None` detaches (no `Option<Option<T>>`).
    SetIdeaProject {
        id: String,
        project_id: Option<String>,
    },
    SetIdeaLifecycle {
        id: String,
        lifecycle: IdeaLifecycle,
    },
    DeleteIdea {
        id: String,
    },
    /// `project_id: None` ⇒ ALL ideas incl. orphans.
    ListIdeas {
        project_id: Option<String>,
    },
    // Insight
    CreateInsight {
        project_id: Option<String>,
        source: String,
        title: String,
        body: String,
    },
    UpdateInsight {
        id: String,
        title: Option<String>,
        body: Option<String>,
    },
    /// D11: dedicated verb (no `Option<Option<T>>`).
    SetInsightFitVerdict {
        id: String,
        fit_verdict: Option<FitVerdict>,
        fit_reasoning: String,
    },
    SetInsightStatus {
        id: String,
        status: InsightStatus,
        resolution_reasoning: Option<String>,
    },
    DeleteInsight {
        id: String,
    },
    /// `project_id: None` ⇒ ALL.
    ListInsights {
        project_id: Option<String>,
    },
    // Task
    CreateTask {
        project_id: String,
        parent_id: Option<String>,
        title: String,
        body: String,
        status: Option<TaskStatus>,
        source: TaskSource,
        source_id: Option<String>,
        tags: Vec<String>,
    },
    UpdateTask {
        id: String,
        title: Option<String>,
        body: Option<String>,
        tags: Option<Vec<String>>,
    },
    SetTaskStatus {
        id: String,
        status: TaskStatus,
    },
    SetTaskRank {
        id: String,
        rank: f64,
    },
    /// cascades subtasks.
    DeleteTask {
        id: String,
    },
    ListTasks {
        project_id: Option<String>,
    },
    // RuleSet
    /// → `OrchdResponse::RuleSetView`.
    GetRuleSet {
        scope: RuleScope,
        project_id: Option<String>,
    },
    UpsertRuleSet {
        scope: RuleScope,
        project_id: Option<String>,
        /// `Some` ⇒ write file + rehash.
        md_content: Option<String>,
        /// `Some` ⇒ repoint (validated absolute).
        md_path: Option<String>,
        policy: Option<PolicyRules>,
    },
    /// re-read file → store new hash (or report `Missing`).
    AcknowledgeRuleFile {
        id: String,
    },
    // Export / import
    /// → `OrchdResponse::ExportJson`.
    ExportProject {
        project_id: String,
    },
    /// → `OrchdResponse::ExportJson`.
    ExportAll,
    /// → `OrchdResponse::ImportReport`.
    ImportBundle {
        json: String,
    },
    // Daemon
    OrchdShutdown {
        drain: bool,
    },
    // S4 knowledge graph (spec §3, appended — order FROZEN append-only)
    /// → `OrchdResponse::GraphNode`. NO `entity_type`/`entity_id`: entityRef nodes are
    /// internal-only (created via `add_entity_ref_node`, not this wire verb).
    GraphAddNode {
        project_id: String,
        kind: GraphNodeKind,
        label: String,
        body: String,
        pos_x: f64,
        pos_y: f64,
    },
    /// → `OrchdResponse::GraphNode`.
    GraphUpdateNode {
        id: String,
        label: Option<String>,
        body: Option<String>,
    },
    /// → `OrchdResponse::GraphNode` (frequent).
    GraphMoveNode {
        id: String,
        pos_x: f64,
        pos_y: f64,
    },
    /// → `OrchdResponse::Ack` (cascades edges).
    GraphDeleteNode {
        id: String,
    },
    /// → `OrchdResponse::GraphEdge`.
    GraphAddEdge {
        source_node_id: String,
        target_node_id: String,
        kind: GraphEdgeKind,
        label: String,
    },
    /// → `OrchdResponse::Ack`.
    GraphDeleteEdge {
        id: String,
    },
    /// → `OrchdResponse::GraphView`.
    GraphListProject {
        project_id: String,
    },
    /// → `OrchdResponse::Neighborhood` (retrieval).
    GraphNeighborhood {
        node_id: String,
        depth: u32,
    },
    /// → `OrchdResponse::GraphNodes` (workspace-wide when `project_id: None`).
    GraphSearch {
        query: String,
        project_id: Option<String>,
    },
    // S-EXT MCP (spec §5, appended — order FROZEN append-only). Phase-1 subset: server/tool
    // registry, connect/call, invocation/artifact history, trust consent. `McpUpdateServer`
    // deliberately excludes `transport`/`scope`/`project_id` (fixed at `McpAddServer` time,
    // load-bearing for the spec §4 CHECK invariants — mirrors `bpa_orchd::mcp::McpServerPatch`).
    /// → `OrchdResponse::McpServer`.
    McpAddServer {
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
    },
    /// → `OrchdResponse::McpServers` (global + the given project's, when `project_id: Some`).
    McpListServers {
        project_id: Option<String>,
    },
    /// → `OrchdResponse::McpServer`.
    McpUpdateServer {
        id: String,
        name: Option<String>,
        url: Option<String>,
        command: Option<String>,
        args: Option<Vec<String>>,
        env: Option<BTreeMap<String, String>>,
        auth_kind: Option<McpAuthKind>,
        timeout_ms: Option<i64>,
        max_retries: Option<i64>,
    },
    /// → `OrchdResponse::McpServer`.
    McpSetServerEnabled {
        id: String,
        enabled: bool,
    },
    /// → `OrchdResponse::Ack`.
    McpDeleteServer {
        id: String,
    },
    /// → `OrchdResponse::Ack`; `token` -> Keychain, ref -> DB. `token` NEVER logged/echoed.
    McpSetServerBearer {
        id: String,
        token: String,
    },
    /// → `OrchdResponse::McpConnectReport`; trust-gated, caches tools, pushes `McpToolsChanged`.
    McpConnect {
        id: String,
    },
    /// → `OrchdResponse::Ack`.
    McpDisconnect {
        id: String,
    },
    /// → `OrchdResponse::McpTools` (from cache).
    McpListTools {
        server_id: String,
    },
    /// → `OrchdResponse::McpTool`; per-tool allowlist toggle.
    McpSetToolEnabled {
        tool_id: String,
        enabled: bool,
    },
    /// → `OrchdResponse::McpCallResult`; a disabled tool is rejected with `Error{Io}` before
    /// dispatch (allowlist enforced in `invoke.rs`, a later task).
    McpCallTool {
        server_id: String,
        tool_name: String,
        args_json: String,
        project_id: Option<String>,
    },
    /// → `OrchdResponse::McpInvocations`.
    McpListInvocations {
        server_id: Option<String>,
        project_id: Option<String>,
        limit: Option<i64>,
    },
    /// → `OrchdResponse::McpArtifacts`.
    McpListArtifacts {
        project_id: Option<String>,
        server_id: Option<String>,
        limit: Option<i64>,
    },
    /// → `OrchdResponse::McpArtifact`.
    McpGetArtifact {
        id: String,
    },
    /// → `OrchdResponse::Ack`; `kind` is `'connect'` | `'stdio_exec'` (spec §4 `consent_grant`).
    TrustGrantConsent {
        server_id: String,
        kind: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum OrchdResponse {
    Ack,
    Pong,
    Project(Project),
    Projects(Vec<Project>),
    Goal(Goal),
    Goals(Vec<Goal>),
    Idea(Idea),
    Ideas(Vec<Idea>),
    Insight(Insight),
    Insights(Vec<Insight>),
    Task(DomainTask),
    Tasks(Vec<DomainTask>),
    RuleSetView(RuleSetView),
    ExportJson(String),
    ImportReport {
        projects: u32,
        goals: u32,
        ideas: u32,
        insights: u32,
        tasks: u32,
        rulesets: u32,
    },
    Error {
        code: OrchdErrorCode,
        message: String,
    },
    // S4 knowledge graph (spec §3, appended — order FROZEN append-only)
    GraphNode(GraphNode),
    GraphEdge(GraphEdge),
    GraphView(GraphView),
    Neighborhood(GraphNeighborhood),
    GraphNodes(Vec<GraphNode>),
    // S-EXT MCP (spec §5, appended — order FROZEN append-only)
    McpServer(McpServer),
    McpServers(Vec<McpServer>),
    McpTool(McpTool),
    McpTools(Vec<McpTool>),
    McpConnectReport(McpConnectReport),
    McpCallResult(McpCallResult),
    McpInvocations(Vec<McpInvocation>),
    McpArtifacts(Vec<McpArtifact>),
    McpArtifact(McpArtifact),
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum OrchdPush {
    ProjectsChanged,
    GoalsChanged {
        project_id: String,
    },
    IdeasChanged,
    InsightsChanged,
    TasksChanged {
        project_id: String,
    },
    RuleSetChanged {
        scope: RuleScope,
        project_id: Option<String>,
    },
    // S4 knowledge graph (spec §3, appended — order FROZEN append-only)
    GraphChanged {
        project_id: String,
    },
    // S-EXT MCP (spec §5, appended — order FROZEN append-only)
    McpServersChanged {
        project_id: Option<String>,
    },
    McpToolsChanged {
        server_id: String,
    },
    McpArtifactsChanged {
        project_id: Option<String>,
    },
    McpInvocationLogged {
        server_id: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum OrchdFrame {
    /// core → daemon; id correlates the reply.
    Request { id: u64, req: OrchdRequest },
    /// daemon → core; echoes the request id.
    Response { id: u64, res: OrchdResponse },
    /// daemon → core; unsolicited (id-less).
    Push(OrchdPush),
}

// ================================================================================
// ---- framing (spec §4.2, over the §4.1 generics) ----
// ================================================================================

/// Serialize `frame` as CBOR and prepend a `u32`-LE length prefix. Thin instantiation of
/// `bpa_protocol::encode_cbor_frame` over `OrchdFrame` (mirrors `bpa_protocol::encode_frame`
/// over `Frame`).
pub fn encode_orchd_frame(frame: &OrchdFrame) -> Result<Vec<u8>, bpa_protocol::FrameError> {
    bpa_protocol::encode_cbor_frame(frame)
}

/// Thin instantiation of `bpa_protocol::CborFrameDecoder<T>` over `OrchdFrame` (mirrors
/// `bpa_protocol::FrameDecoder` over `Frame`).
pub type OrchdFrameDecoder = bpa_protocol::CborFrameDecoder<OrchdFrame>;
