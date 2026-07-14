//! MCP server/tool registry persistence (S-EXT spec §4, schema v3). Sibling module to
//! `persistence`/`graph` (`pub mod mcp;` in `lib.rs`): unlike `graph` — which decodes its rows
//! into wire types from the already-public `bpa_orchd_proto` crate — this task predates the
//! S-EXT wire protocol (T3), so [`registry`] introduces its own row/enum types here rather than
//! borrowing from a proto crate that doesn't have them yet. `pub` (not crate-private, unlike
//! `mod graph;`) so those types stay nameable as `bpa_orchd::mcp::McpServerRow` etc. — both for
//! this crate's OWN later tasks (T3's wire-protocol conversions, `socket_server` dispatch) and to
//! avoid ever tripping the private-type-in-public-interface lint on `persistence::Db`'s methods.
//!
//! This task (S-EXT T2) implements CRUD for `mcp_server`/`mcp_tool` only — see
//! [`registry`]. The other seven S-EXT tables (`account`, `mcp_invocation`, `mcp_artifact`,
//! `skill`, `consent_grant`, `policy`, `audit_log`) are created by the SAME `Migration{upto:3}`
//! step (spec §4: "one migration, additive-only, all tables verbatim") but their persistence
//! CRUD lands in later S-EXT tasks — no row/enum types for them exist in this module yet.

use std::collections::BTreeMap;

pub mod registry;

/// `mcp_server.transport` (spec §4 CHECK: `transport IN ('http','stdio')`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpTransport {
    Http,
    Stdio,
}

/// `mcp_server.scope` / `skill.scope` (spec §4; this task only persists `mcp_server`, but the
/// enum name is scoped to `Mcp*` since `skill`'s CRUD — landing in a later task — may end up
/// wanting its own copy rather than reusing this one, given `skill` has no `transport` sibling).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpScope {
    Global,
    Project,
}

/// `mcp_server.auth_kind` (spec §4: `'none' | 'bearer' | 'oauth'`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum McpAuthKind {
    None,
    Bearer,
    Oauth,
}

/// Full `mcp_server` row, decoded (spec §4 columns; `args_json`/`env_json` TEXT columns decoded
/// into `args`/`env`, `transport`/`scope`/`auth_kind` TEXT columns decoded into their enums,
/// `enabled` INTEGER decoded into `bool` — mirrors `graph::GraphNodeRow::into_node`'s decode
/// shape). Doubles as both the raw-row AND the public read type (no separate wire DTO exists yet
/// — that's T3's job to build on top of this).
#[derive(Debug, Clone, PartialEq)]
pub struct McpServerRow {
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
    pub timeout_ms: i64,
    pub max_retries: i64,
    pub protocol_version: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Input to [`registry`]'s `Db::add_mcp_server` (spec §4: `id`/`created_at`/`updated_at` are
/// assigned by the insert itself — `uuid v4` / `now_ms()` — never supplied by the caller;
/// `protocol_version` starts `NULL` ("last negotiated; null until first connect", spec §4) and is
/// set later by a connect-flow verb this task doesn't implement).
#[derive(Debug, Clone)]
pub struct NewMcpServer {
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
    pub timeout_ms: i64,
    pub max_retries: i64,
}

/// Partial update for `Db::update_mcp_server` (COALESCE idiom — mirrors
/// `persistence::Db::update_project`/`update_goal`: a field left `None` is left untouched, never
/// cleared). Deliberately excludes `transport`, `scope`, and `project_id` — those three are fixed
/// at `add_mcp_server` time because they're load-bearing for the spec §4 CHECK invariants
/// (`scope='project' ⟺ project_id`, `transport='http' ⟺ url`), and a patch that could silently
/// flip `transport` without also being able to null out `url` (COALESCE never clears a column)
/// would risk violating that CHECK; `url` itself CAN be patched (to a new non-empty value, never
/// cleared), but `Db::update_mcp_server` rejects patching it on a `stdio`-transport server so the
/// CHECK can never be violated through this path either.
#[derive(Debug, Clone, Default)]
pub struct McpServerPatch {
    pub name: Option<String>,
    pub url: Option<String>,
    pub command: Option<String>,
    pub args: Option<Vec<String>>,
    pub env: Option<BTreeMap<String, String>>,
    pub auth_kind: Option<McpAuthKind>,
    pub secret_ref: Option<String>,
    pub account_id: Option<String>,
    pub timeout_ms: Option<i64>,
    pub max_retries: Option<i64>,
}

/// Full `mcp_tool` row, decoded (spec §4 columns; `enabled` INTEGER decoded into `bool` — mirrors
/// [`McpServerRow`]'s shape). `input_schema_json` is kept as the raw TEXT column rather than
/// decoded into a structured JSON-Schema type: no wire type for "a tool's input schema" exists
/// yet (T3), so there is nothing meaningful to decode it INTO — every other JSON TEXT column in
/// this crate (`goal.metric_refs`, `task.tags`) decodes into a concrete Rust type because the
/// wire type it feeds already exists; this one doesn't, so it stays a passthrough string.
#[derive(Debug, Clone, PartialEq)]
pub struct McpToolRow {
    pub id: String,
    pub server_id: String,
    pub name: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub input_schema_json: String,
    pub enabled: bool,
    pub fetched_at: i64,
}

/// Input to `Db::upsert_mcp_tools` (spec §4: "Cached tool descriptors (refreshed on connect +
/// tools/list_changed)"). No `enabled` field: every upserted tool is inserted `enabled=1` (spec
/// §4 `mcp_tool.enabled` comment: "default on-fetch") — a fresh `tools/list` fetch always resets
/// the per-tool allowlist to fully-on, and `Db::set_mcp_tool_enabled` is the ONLY way to turn one
/// off again afterwards (until the next upsert resets it).
#[derive(Debug, Clone)]
pub struct NewMcpTool {
    pub name: String,
    pub title: Option<String>,
    pub description: Option<String>,
    pub input_schema_json: String,
}
