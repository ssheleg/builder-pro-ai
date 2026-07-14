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
