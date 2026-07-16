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
    /// «source deleted». Always `false` for non-`entityRef` nodes and for a still-live
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
    /// The MCP server this call targeted (`Some` for an `McpCallTool` invocation); `null` for a
    /// `ConnectorInvoke`, which carries the account instead (spec §4 XOR; T12 review:
    /// ConnectorInvoke reuses this invocation record path). Exactly one of the two is set.
    pub server_id: Option<String>,
    /// The connector account this call targeted (`Some` for a `ConnectorInvoke`); `null` for an
    /// `McpCallTool`.
    pub account_id: Option<String>,
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
    /// The MCP server that produced this artifact (`Some` for an `McpCallTool` result); `null`
    /// for a `ConnectorInvoke` artifact, which carries the account instead (spec §4 XOR, D9; T12
    /// review: ConnectorInvoke persists a durable untrusted artifact too). Exactly one is set.
    pub server_id: Option<String>,
    /// The connector account that produced this artifact (`Some` for a `ConnectorInvoke` result);
    /// `null` for an `McpCallTool`.
    pub account_id: Option<String>,
    pub tool_name: String,
    pub project_id: Option<String>,
    pub content_json: String,
    pub content_text: Option<String>,
    pub is_untrusted: bool,
    #[ts(type = "number")]
    pub created_at: i64,
}

// ---- S-EXT Connector/OAuth entities (spec §5/§7, appended — order FROZEN append-only) ----
// Phase-2 subset: external OAuth/apikey accounts (the spec §4 `account` table) + the direct-API
// adapter's op list + the OAuth authorize challenge, with the usual camelCase + ts-rs wire
// treatment layered on top (mirrors the `McpServer` entity block byte-for-byte). Deliberately
// deviates from the "mirror the DB row 1:1" precedent T3 set for `McpServer`: `secret_ref`/
// `refresh_ref` are Keychain REFERENCE strings, same non-secret shape as `McpServer.secret_ref`,
// but the frontend never needs them (no UI surface reads a Keychain key name), so they are
// omitted from this wire entity entirely rather than round-tripped for no consumer — narrower
// surface, nothing to leak, one field less to keep in sync.

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "orchd-types.ts")]
pub struct Account {
    pub id: String,
    pub provider: String,
    pub label: String,
    pub auth_kind: AccountAuthKind,
    pub scopes: Vec<String>,
    #[ts(type = "number | null")]
    pub expires_at: Option<i64>,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number")]
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "orchd-types.ts")]
pub enum AccountAuthKind {
    Oauth,
    Apikey,
}

/// One operation a `ConnectorAdapter` (spec §7) exposes for a given account's provider.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "orchd-types.ts")]
pub struct ConnectorOp {
    pub name: String,
    pub description: Option<String>,
}

/// `OrchdRequest::ConnectorBeginOAuth`'s success payload (spec §5): the PKCE authorize URL to
/// open in the browser, plus the CSRF `state` the subsequent `ConnectorCompleteOAuth` must echo.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "orchd-types.ts")]
pub struct OAuthChallenge {
    pub authorize_url: String,
    pub state: String,
}

// ---- S-EXT Skills entities (spec §4/§5, D11, Q14, task T17, appended — order FROZEN
// append-only). Plumbing-only registry: SKILL.md-format files are the source of truth, the DB
// only stores `md_path`/`md_hash` (files-as-truth, mirrors `RuleSet`/`RuleSetView` — D4 of S3).
// There is NO runtime consumer of this registry yet (that's S6b's agent org) — see D11: "UI
// lists/adds/removes; a banner states skills run once the agent org ships". Field set mirrors the
// spec §4 `skill` DDL columns 1:1 (same field names as `bpa_orchd::skills::SkillRow` from this
// task), with the usual camelCase + ts-rs wire treatment layered on top (mirrors the
// `McpServer`/`Account` entity blocks byte-for-byte). ----

/// `fileState` is NOT itself a DB column — it is computed FRESH at read time by re-hashing the
/// SKILL.md file against its stored hash (files-as-truth, mirrors how `RuleSetView` covers the
/// `ruleset` table the same way). `Skill` intentionally has no `AcknowledgeRuleFile`-style verb to
/// clear a `Modified` state — this registry has no equivalent "I've seen the external edit"
/// affordance (out of scope for a plumbing-only slice), so re-adding (or, once a consumer exists,
/// re-registering) the skill is the only way to refresh the stored hash.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "orchd-types.ts")]
pub struct Skill {
    pub id: String,
    pub name: String,
    pub description: String,
    pub md_path: String,
    pub md_hash: String,
    pub scope: SkillScope,
    pub project_id: Option<String>,
    pub file_state: SkillFileState,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number")]
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "orchd-types.ts")]
pub enum SkillScope {
    Global,
    Project,
}

/// Files-as-truth read-time classification (mirrors `RuleFileState`'s role for `ruleset`, but a
/// distinct wire enum with its own — task-17-brief-specified — variant names): `Present` (the
/// file's current sha256 matches the stored hash), `Modified` (it exists but the hash no longer
/// matches — hand-edited or replaced outside orchd since it was registered), `Missing` (the file
/// is gone).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "orchd-types.ts")]
pub enum SkillFileState {
    Present,
    Modified,
    Missing,
}

// ---- S-EXT Trust entities (spec §4/§5/§6, BL-22, task T18, appended — order FROZEN
// append-only): the `policy` spend/rate-cap table + the `audit_log` rows surfaced to the
// Log/audit UI. Distinct from the pre-existing `PolicyRules` above (a per-ruleset
// owner-consent policy, S1, with its OWN unrelated spend-cap field): `Policy` here is the trust
// CHOKE-POINT's spend/rate cap row (`crate::trust::authorize`, spec §6), keyed by scope + a
// reference id, not a ruleset. ----

/// `policy` row (spec §4): a spend/rate cap at one of three scopes. `None` cap fields mean
/// "unlimited" for that dimension — a `Policy` with BOTH fields `None` is a legal (if pointless)
/// row. See [`PolicyScope`] for the scope/reference-id pairing rule.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "orchd-types.ts")]
pub struct Policy {
    pub id: String,
    pub scope: PolicyScope,
    /// The project or server this policy applies to — `Some` for a project- or server-scoped
    /// policy, `None` for the single global-scope policy (spec §4: null for global).
    pub ref_id: Option<String>,
    pub spend_cap_usd: Option<f64>,
    #[ts(type = "number | null")]
    pub rate_per_min: Option<i64>,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number")]
    pub updated_at: i64,
}

/// `policy.scope` (spec §4/§6, BL-22): which axis a cap applies to. `crate::trust`'s
/// effective-policy resolution (task T18) is MOST-SPECIFIC-wins — `Server` overrides `Project`
/// overrides `Global` — the whole matching row wins outright, not a per-field merge (see
/// `trust::resolve_policy`'s own doc comment for the full rationale).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "orchd-types.ts")]
pub enum PolicyScope {
    Global,
    Project,
    Server,
}

/// `audit_log` row (spec §4/§6, BL-22, task T18): every trust-choke-point decision, allow or
/// deny, surfaced to the Log/audit UI. `reason`/every other field NEVER carries secrets or
/// tool-call arguments (spec §6) — only the fixed action/decision/reason vocabulary
/// `crate::trust::authorize` writes (request content lives in the matching invocation's own
/// request-hash field, a sha256, never here).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "orchd-types.ts")]
pub struct AuditRow {
    pub id: String,
    #[ts(type = "number")]
    pub at: i64,
    /// One of a fixed action-literal vocabulary (spec §4): a connect/disconnect, a stdio
    /// process spawn, an MCP tool call, a direct-API connector invoke, a consent grant, or a
    /// policy-cap denial.
    pub action: String,
    pub server_id: Option<String>,
    pub tool_name: Option<String>,
    pub project_id: Option<String>,
    /// `'allow'|'deny'`.
    pub decision: String,
    /// e.g. consent required, tool disabled, rate limit exceeded, spend cap exceeded; `None` on
    /// an `'allow'` row.
    pub reason: Option<String>,
    pub invocation_id: Option<String>,
}

// ---- S-IDEA research entities (spec §5, task T3, appended — order FROZEN append-only): the
// `research_run` table (schema v4, task T2) is a THIN provenance link (idea↔invocation↔artifact,
// D2) — this entity mirrors `bpa_orchd::research::ResearchRunRow`'s field set 1:1, with the usual
// camelCase + ts-rs wire treatment layered on top (mirrors the `McpArtifact` entity block
// byte-for-byte). `ResearchStatus`'s wire tags (`pending`/`running`/`done`/`failed`) match T2's
// `research_run.status` TEXT literals exactly (`decode_research_status` in
// `crates/orchd/src/research/mod.rs`).

/// `research_run` row (spec §4/§5, schema v4). The actual research artifact is the pre-existing
/// `McpArtifact` row a run's `tools/call` produces (D2) — this row is only the provenance link
/// plus status; `invocationId`/`artifactId` are `Some` only once the run reaches `done` (spec §4
/// CHECK linking status and the artifact reference).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "orchd-types.ts")]
pub struct ResearchRun {
    pub id: String,
    pub idea_id: String,
    pub server_id: String,
    pub tool_name: String,
    pub args_json: String,
    pub status: ResearchStatus,
    pub invocation_id: Option<String>,
    pub artifact_id: Option<String>,
    pub error_kind: Option<String>,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number")]
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "orchd-types.ts")]
pub enum ResearchStatus {
    Pending,
    Running,
    Done,
    Failed,
}

/// Storage-degradation mode surfaced to the frontend (spec D3, BL-94). Fixed at boot: the daemon
/// either opened its on-disk DB normally (`Persistent`), fell back to a non-persistent in-memory
/// DB because the disk was unavailable (`InMemoryFallback`), or recovered from a corrupt on-disk
/// image that was quarantined aside (`RecoveredFromCorruption`, with `quarantinedPath` naming the
/// saved copy). Pulled once on connect and on every reconnect — there is no push, since the mode
/// never changes without a daemon restart.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "orchd-types.ts")]
pub struct StorageStatus {
    pub storage_mode: StorageMode,
    pub quarantined_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export_to = "orchd-types.ts")]
pub enum StorageMode {
    Persistent,
    InMemoryFallback,
    RecoveredFromCorruption,
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
    // S-EXT Connectors / accounts (spec §5/§7, appended — order FROZEN append-only). Phase-2
    // subset: OAuth/apikey account lifecycle + the generic direct-API adapter invoke path.
    /// → `OrchdResponse::OAuthChallenge`; opens the browser to `authorize_url` (PKCE); pending
    /// state keyed by `state` until the matching `ConnectorCompleteOAuth`.
    ConnectorBeginOAuth {
        provider: String,
        label: String,
        scopes: Option<Vec<String>>,
        server_id: Option<String>,
    },
    /// → `OrchdResponse::Account`; exchanges `code` for tokens (Keychain), pushes
    /// `ConnectorsChanged`.
    ConnectorCompleteOAuth {
        state: String,
        code: String,
    },
    /// → `OrchdResponse::Account`; `api_key` -> Keychain, ref -> DB. `api_key` NEVER
    /// logged/echoed. Pushes `ConnectorsChanged`.
    ConnectorAddApiKey {
        provider: String,
        label: String,
        api_key: String,
    },
    /// → `OrchdResponse::Accounts`.
    ConnectorListAccounts,
    /// → `OrchdResponse::Ack`; pushes `ConnectorsChanged`.
    ConnectorDeleteAccount {
        id: String,
    },
    /// → `OrchdResponse::ConnectorOps`; the account's provider adapter's op list (spec §7).
    ConnectorListOps {
        account_id: String,
    },
    /// → `OrchdResponse::McpCallResult`; reuses the MCP call/artifact/invocation path (spec §6
    /// "connector_invoke passes through trust::authorize identically to McpCallTool").
    ConnectorInvoke {
        account_id: String,
        op: String,
        args_json: String,
        project_id: Option<String>,
    },
    // S-EXT Skills (spec §4/§5, D11, Q14, task T17, appended — order FROZEN append-only):
    // plumbing-only registry — see `Skill`'s own doc comment above ("no runtime consumer until
    // S6b agent org").
    /// → `OrchdResponse::Skill`; pushes `SkillsChanged`. `name`/`description: None` ⇒ parsed from
    /// the SKILL.md frontmatter at `md_path` (spec §4 comment: "parses SKILL.md frontmatter if
    /// name/desc omitted"); neither an explicit `name` NOR a parseable frontmatter `name` ⇒
    /// `Error{Validation}`.
    SkillAdd {
        name: Option<String>,
        description: Option<String>,
        md_path: String,
        scope: SkillScope,
        project_id: Option<String>,
    },
    /// → `OrchdResponse::Skills` (global + the given project's, when `project_id: Some` — mirrors
    /// `McpListServers`'s scoping exactly).
    SkillList {
        project_id: Option<String>,
    },
    /// → `OrchdResponse::Ack`; pushes `SkillsChanged`.
    SkillDelete {
        id: String,
    },
    // S-EXT Trust: policy caps + audit log (spec §4/§5/§6, BL-22, task T18, appended — order
    // FROZEN append-only).
    /// → `OrchdResponse::Policy`; UPSERT keyed by `(scope, ref_id)` (spec §4) — `scope:"global"`
    /// requires `ref_id: None`, `scope:"project"|"server"` requires `ref_id: Some(<id>)`; a
    /// mismatch is `Error{Validation}`. `None` cap fields mean "unlimited" for that dimension.
    /// Pushes `PoliciesChanged`.
    TrustSetPolicy {
        scope: PolicyScope,
        ref_id: Option<String>,
        spend_cap_usd: Option<f64>,
        rate_per_min: Option<i64>,
    },
    /// → `OrchdResponse::Policies`. Read-only, broadcasts nothing.
    TrustListPolicies,
    /// → `OrchdResponse::AuditRows`. Newest-first, optionally capped at `limit`. Read-only,
    /// broadcasts nothing.
    TrustListAudit {
        limit: Option<i64>,
    },
    // S-IDEA research (spec §5, task T3, appended — order FROZEN append-only): starts a research
    // run against an MCP tool for an idea (spec §6 step 1). Persistence + the pending→researching
    // idea-lifecycle flip already land in T2 (`bpa_orchd::research::Db::start_research_run`); the
    // async run driver that actually calls the tool and drives pending→running→done/failed is a
    // later task (T4/T5) — this task only wires the wire shape + a temporary dispatch stub.
    /// → `OrchdResponse::ResearchRun`.
    ResearchStartRun {
        idea_id: String,
        server_id: String,
        tool_name: String,
        args_json: String,
    },
    /// → `OrchdResponse::ResearchRuns` (newest first, mirrors `Db::list_research_runs`'s order).
    ResearchListRuns {
        idea_id: String,
    },
    /// → `OrchdResponse::ResearchRun`.
    ResearchGetRun {
        id: String,
    },
    /// → `OrchdResponse::StorageStatus`. Reports the daemon's storage-degradation mode (spec D3,
    /// BL-94); fixed at boot, pulled on connect and reconnect.
    GetStorageStatus,
}

impl OrchdRequest {
    /// A stable, low-cardinality `&'static str` name for this request's variant — the ONLY
    /// request-derived value allowed into a structured completion-trace field (spec D4, O-6).
    ///
    /// This is the single per-verb tracing choke-point's name source, reused by BOTH layers that
    /// see an `OrchdRequest`: the daemon's `socket_server::dispatch` completion trace and the
    /// core's `orchd_client::request` trace (which together cover all 77 daemon verbs and all 117
    /// command handlers without a per-arm edit). Living next to the enum keeps them in lockstep.
    ///
    /// The match is deliberately **exhaustive with no `_` wildcard**: adding a future variant to
    /// `OrchdRequest` fails to compile until it is named here — that is the point, so a new verb
    /// can never ship silently untraced. Fields are matched with `{ .. }` and never bound, so no
    /// argument, body, token, id, or other payload value can ever be captured into the name (a
    /// completion trace carries verb + outcome + error_code + elapsed only — never args/PII).
    pub fn verb_name(&self) -> &'static str {
        match self {
            Self::Ping => "Ping",
            Self::CreateProject { .. } => "CreateProject",
            Self::UpdateProject { .. } => "UpdateProject",
            Self::ArchiveProject { .. } => "ArchiveProject",
            Self::ListProjects => "ListProjects",
            Self::AddProjectWorkspace { .. } => "AddProjectWorkspace",
            Self::RemoveProjectWorkspace { .. } => "RemoveProjectWorkspace",
            Self::CreateGoal { .. } => "CreateGoal",
            Self::UpdateGoal { .. } => "UpdateGoal",
            Self::MoveGoal { .. } => "MoveGoal",
            Self::DeleteGoal { .. } => "DeleteGoal",
            Self::ListGoals { .. } => "ListGoals",
            Self::CreateIdea { .. } => "CreateIdea",
            Self::UpdateIdea { .. } => "UpdateIdea",
            Self::SetIdeaProject { .. } => "SetIdeaProject",
            Self::SetIdeaLifecycle { .. } => "SetIdeaLifecycle",
            Self::DeleteIdea { .. } => "DeleteIdea",
            Self::ListIdeas { .. } => "ListIdeas",
            Self::CreateInsight { .. } => "CreateInsight",
            Self::UpdateInsight { .. } => "UpdateInsight",
            Self::SetInsightFitVerdict { .. } => "SetInsightFitVerdict",
            Self::SetInsightStatus { .. } => "SetInsightStatus",
            Self::DeleteInsight { .. } => "DeleteInsight",
            Self::ListInsights { .. } => "ListInsights",
            Self::CreateTask { .. } => "CreateTask",
            Self::UpdateTask { .. } => "UpdateTask",
            Self::SetTaskStatus { .. } => "SetTaskStatus",
            Self::SetTaskRank { .. } => "SetTaskRank",
            Self::DeleteTask { .. } => "DeleteTask",
            Self::ListTasks { .. } => "ListTasks",
            Self::GetRuleSet { .. } => "GetRuleSet",
            Self::UpsertRuleSet { .. } => "UpsertRuleSet",
            Self::AcknowledgeRuleFile { .. } => "AcknowledgeRuleFile",
            Self::ExportProject { .. } => "ExportProject",
            Self::ExportAll => "ExportAll",
            Self::ImportBundle { .. } => "ImportBundle",
            Self::OrchdShutdown { .. } => "OrchdShutdown",
            Self::GraphAddNode { .. } => "GraphAddNode",
            Self::GraphUpdateNode { .. } => "GraphUpdateNode",
            Self::GraphMoveNode { .. } => "GraphMoveNode",
            Self::GraphDeleteNode { .. } => "GraphDeleteNode",
            Self::GraphAddEdge { .. } => "GraphAddEdge",
            Self::GraphDeleteEdge { .. } => "GraphDeleteEdge",
            Self::GraphListProject { .. } => "GraphListProject",
            Self::GraphNeighborhood { .. } => "GraphNeighborhood",
            Self::GraphSearch { .. } => "GraphSearch",
            Self::McpAddServer { .. } => "McpAddServer",
            Self::McpListServers { .. } => "McpListServers",
            Self::McpUpdateServer { .. } => "McpUpdateServer",
            Self::McpSetServerEnabled { .. } => "McpSetServerEnabled",
            Self::McpDeleteServer { .. } => "McpDeleteServer",
            Self::McpSetServerBearer { .. } => "McpSetServerBearer",
            Self::McpConnect { .. } => "McpConnect",
            Self::McpDisconnect { .. } => "McpDisconnect",
            Self::McpListTools { .. } => "McpListTools",
            Self::McpSetToolEnabled { .. } => "McpSetToolEnabled",
            Self::McpCallTool { .. } => "McpCallTool",
            Self::McpListInvocations { .. } => "McpListInvocations",
            Self::McpListArtifacts { .. } => "McpListArtifacts",
            Self::McpGetArtifact { .. } => "McpGetArtifact",
            Self::TrustGrantConsent { .. } => "TrustGrantConsent",
            Self::ConnectorBeginOAuth { .. } => "ConnectorBeginOAuth",
            Self::ConnectorCompleteOAuth { .. } => "ConnectorCompleteOAuth",
            Self::ConnectorAddApiKey { .. } => "ConnectorAddApiKey",
            Self::ConnectorListAccounts => "ConnectorListAccounts",
            Self::ConnectorDeleteAccount { .. } => "ConnectorDeleteAccount",
            Self::ConnectorListOps { .. } => "ConnectorListOps",
            Self::ConnectorInvoke { .. } => "ConnectorInvoke",
            Self::SkillAdd { .. } => "SkillAdd",
            Self::SkillList { .. } => "SkillList",
            Self::SkillDelete { .. } => "SkillDelete",
            Self::TrustSetPolicy { .. } => "TrustSetPolicy",
            Self::TrustListPolicies => "TrustListPolicies",
            Self::TrustListAudit { .. } => "TrustListAudit",
            Self::ResearchStartRun { .. } => "ResearchStartRun",
            Self::ResearchListRuns { .. } => "ResearchListRuns",
            Self::ResearchGetRun { .. } => "ResearchGetRun",
            Self::GetStorageStatus => "GetStorageStatus",
        }
    }
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
    // S-EXT Connectors / accounts (spec §5/§7, appended — order FROZEN append-only)
    Account(Account),
    Accounts(Vec<Account>),
    OAuthChallenge(OAuthChallenge),
    ConnectorOps(Vec<ConnectorOp>),
    // S-EXT Skills (spec §4/§5, D11, Q14, task T17, appended — order FROZEN append-only)
    Skill(Skill),
    Skills(Vec<Skill>),
    // S-EXT Trust: policy caps + audit log (spec §4/§5/§6, BL-22, task T18, appended — order
    // FROZEN append-only)
    Policy(Policy),
    Policies(Vec<Policy>),
    AuditRows(Vec<AuditRow>),
    // S-IDEA research (spec §5, task T3, appended — order FROZEN append-only)
    ResearchRun(ResearchRun),
    ResearchRuns(Vec<ResearchRun>),
    // Storage-degradation mode (spec D3, BL-94, appended — order FROZEN append-only)
    StorageStatus(StorageStatus),
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
    // S-EXT Connectors / accounts (spec §5/§7, appended — order FROZEN append-only). No
    // payload: the `account` table (spec §4) has no `project_id` column to scope by.
    ConnectorsChanged,
    // S-EXT Skills (spec §4/§5, D11, Q14, task T17, appended — order FROZEN append-only).
    SkillsChanged {
        project_id: Option<String>,
    },
    // S-EXT Trust: policy caps (spec §4/§5/§6, BL-22, task T18, appended — order FROZEN
    // append-only). No payload: a `policy` change can be global/project/server-scoped, so
    // there's no single natural `project_id`/`server_id` to name coarsely — mirrors
    // `ConnectorsChanged`'s "nothing to name" precedent.
    PoliciesChanged,
    // S-IDEA research (spec §5, task T3, appended — order FROZEN append-only): fired after a
    // research run's status changes (start/running/done/failed, T4/T5). `idea_id: Some` for a
    // single idea's runs changing (the common case); `None` reserved for a future workspace-wide
    // invalidation, mirroring `McpServersChanged`/`SkillsChanged`'s optional-scope shape.
    ResearchRunsChanged {
        idea_id: Option<String>,
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
