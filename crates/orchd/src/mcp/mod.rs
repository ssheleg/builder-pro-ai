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
//!
//! S-EXT T5 adds [`lifecycle`] (connect) and [`invoke`] (`tools/call`) on top of this registry,
//! gated by the sibling `crate::trust` choke-point, plus [`cache`] (tool-list → `mcp_tool`
//! cache-write translation) and the [`ToolCaller`] test/production session seam below.

use std::collections::BTreeMap;

pub mod cache;
pub mod invoke;
pub mod lifecycle;
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

// ================================================================================
// ---- wire conversions (S-EXT spec §5, task T6): crate-local persistence rows/enums (this
// module, T2) <-> the T3 wire entities in `bpa_orchd_proto`. `socket_server::dispatch` is the
// only caller — every row this crate reads from `mcp_server`/`mcp_tool` is converted here on its
// way out to a client; every wire enum a client sends in (`McpAddServer`/`McpUpdateServer`) is
// converted back on its way into a `NewMcpServer`/`McpServerPatch`. Field sets are IDENTICAL 1:1
// (both mirror the same spec §4 DDL columns — T3's entity doc comment says so explicitly), so
// every impl below is a plain field-for-field move, no logic. Referenced fully-qualified
// (`bpa_orchd_proto::McpTransport` etc.) throughout rather than imported bare, since this
// module's OWN `McpTransport`/`McpScope`/`McpAuthKind` share the exact same short names — bare
// imports of both would collide.
// ================================================================================

impl From<bpa_orchd_proto::McpTransport> for McpTransport {
    fn from(t: bpa_orchd_proto::McpTransport) -> Self {
        match t {
            bpa_orchd_proto::McpTransport::Http => McpTransport::Http,
            bpa_orchd_proto::McpTransport::Stdio => McpTransport::Stdio,
        }
    }
}

impl From<McpTransport> for bpa_orchd_proto::McpTransport {
    fn from(t: McpTransport) -> Self {
        match t {
            McpTransport::Http => bpa_orchd_proto::McpTransport::Http,
            McpTransport::Stdio => bpa_orchd_proto::McpTransport::Stdio,
        }
    }
}

impl From<bpa_orchd_proto::McpScope> for McpScope {
    fn from(s: bpa_orchd_proto::McpScope) -> Self {
        match s {
            bpa_orchd_proto::McpScope::Global => McpScope::Global,
            bpa_orchd_proto::McpScope::Project => McpScope::Project,
        }
    }
}

impl From<McpScope> for bpa_orchd_proto::McpScope {
    fn from(s: McpScope) -> Self {
        match s {
            McpScope::Global => bpa_orchd_proto::McpScope::Global,
            McpScope::Project => bpa_orchd_proto::McpScope::Project,
        }
    }
}

impl From<bpa_orchd_proto::McpAuthKind> for McpAuthKind {
    fn from(k: bpa_orchd_proto::McpAuthKind) -> Self {
        match k {
            bpa_orchd_proto::McpAuthKind::None => McpAuthKind::None,
            bpa_orchd_proto::McpAuthKind::Bearer => McpAuthKind::Bearer,
            bpa_orchd_proto::McpAuthKind::Oauth => McpAuthKind::Oauth,
        }
    }
}

impl From<McpAuthKind> for bpa_orchd_proto::McpAuthKind {
    fn from(k: McpAuthKind) -> Self {
        match k {
            McpAuthKind::None => bpa_orchd_proto::McpAuthKind::None,
            McpAuthKind::Bearer => bpa_orchd_proto::McpAuthKind::Bearer,
            McpAuthKind::Oauth => bpa_orchd_proto::McpAuthKind::Oauth,
        }
    }
}

impl From<McpServerRow> for bpa_orchd_proto::McpServer {
    fn from(r: McpServerRow) -> Self {
        bpa_orchd_proto::McpServer {
            id: r.id,
            name: r.name,
            transport: r.transport.into(),
            url: r.url,
            command: r.command,
            args: r.args,
            env: r.env,
            scope: r.scope.into(),
            project_id: r.project_id,
            auth_kind: r.auth_kind.into(),
            secret_ref: r.secret_ref,
            account_id: r.account_id,
            enabled: r.enabled,
            timeout_ms: r.timeout_ms,
            max_retries: r.max_retries,
            protocol_version: r.protocol_version,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

impl From<McpToolRow> for bpa_orchd_proto::McpTool {
    fn from(r: McpToolRow) -> Self {
        bpa_orchd_proto::McpTool {
            id: r.id,
            server_id: r.server_id,
            name: r.name,
            title: r.title,
            description: r.description,
            input_schema_json: r.input_schema_json,
            enabled: r.enabled,
            fetched_at: r.fetched_at,
        }
    }
}

// ================================================================================
// ---- test/production session seam (S-EXT §3, D2, task T5) ----
// ================================================================================

/// Test/production seam for a live MCP session: [`lifecycle::connect`]/[`invoke::call_tool`] are
/// generic over this trait rather than depending on [`bpa_mcp::McpSession`] directly, so their
/// own tests can inject an in-process fake (no network/rmcp needed — see
/// [`test_support::FakeSession`], `#[cfg(test)]`-only). [`bpa_mcp::McpSession`] implements this
/// for production (below).
///
/// Native `async fn` in trait (stable since Rust 1.75; this workspace pins 1.92) rather than the
/// `async-trait` crate, per the task-5 brief's preference. This deliberately makes `ToolCaller`
/// NOT `dyn`-compatible (async-fn traits aren't `dyn`-safe on stable Rust) — every caller is
/// generic (`S: ToolCaller`) instead of taking `&dyn ToolCaller`, so that cost never applies.
///
/// `#[allow(async_fn_in_trait)]`: the lint warns that implementors can't add auto-trait bounds
/// (e.g. `Send`) to the returned future — irrelevant here, `ToolCaller` is an intra-crate seam
/// (never re-exported to another crate), so there is no external consumer that would need one.
#[allow(async_fn_in_trait)]
pub trait ToolCaller {
    async fn list_tools(&self) -> Result<Vec<bpa_mcp::McpTool>, bpa_mcp::McpError>;
    async fn call_tool(
        &self,
        name: &str,
        args: serde_json::Value,
    ) -> Result<bpa_mcp::McpToolResult, bpa_mcp::McpError>;
    fn protocol_version(&self) -> String;
}

impl ToolCaller for bpa_mcp::McpSession {
    async fn list_tools(&self) -> Result<Vec<bpa_mcp::McpTool>, bpa_mcp::McpError> {
        bpa_mcp::McpSession::list_tools(self).await
    }

    async fn call_tool(
        &self,
        name: &str,
        args: serde_json::Value,
    ) -> Result<bpa_mcp::McpToolResult, bpa_mcp::McpError> {
        bpa_mcp::McpSession::call_tool(self, name, args).await
    }

    fn protocol_version(&self) -> String {
        bpa_mcp::McpSession::protocol_version(self)
    }
}

/// Production [`ToolCaller`] factory (S-EXT §3, D2, D6, task T16): builds a live session over
/// either transport a [`McpServerRow`] can name — Streamable HTTP (Phase 1, spec D6) or a stdio
/// child process (Phase 3, spec D6, gated by [`connect_action`]'s `StdioSpawn` consent BEFORE
/// this function is ever called — see `lifecycle::connect`/`invoke::call_tool`). Passed as the
/// `connect_fn` argument to [`lifecycle::connect`]/[`invoke::call_tool`] in production; tests
/// inject a fake factory instead so neither module needs network/rmcp to test.
///
/// The stdio arm strips `DYLD_*`/`LD_*` from `server.env` via the SAME shared denylist
/// `bpa-sessiond`'s `env_overrides` path uses (`bpa_daemon_core::env_filter`, task T16, closes
/// BL-1) BEFORE it ever reaches [`bpa_mcp::TransportConfig::Stdio`] — `bpa-mcp` itself does no
/// filtering by design (see [`bpa_mcp::TransportConfig::Stdio`]'s own doc comment), so this call
/// site is the ONLY place a stdio server's env gets sanitized; skipping this step would open a
/// second, unfiltered dynamic-linker-injection spawn path alongside sessiond's.
///
/// Takes `server` BY VALUE (a clone of the already-loaded row), not by reference:
/// `lifecycle::connect`/`invoke::call_tool`'s own `connect_fn` generic bound is
/// `FnOnce(McpServerRow, Option<String>) -> Fut`. An `&McpServerRow` parameter would force the
/// bound into an implicit `for<'a> FnOnce(&'a McpServerRow, ...) -> Fut` (Rust's `Fn(&T)` sugar),
/// which requires ONE `Fut` type valid for every `'a` — but an `async fn` taking a reference
/// parameter produces a future whose opaque type captures that specific `'a`, so no single `Fut`
/// can satisfy the `for<'a>` bound. Taking `server` owned sidesteps this entirely.
pub async fn connect_session(
    server: McpServerRow,
    bearer: Option<String>,
) -> Result<bpa_mcp::McpSession, bpa_mcp::McpError> {
    let cfg = build_transport_config(server)?;
    bpa_mcp::connect(cfg, bearer).await
}

/// Pure half of [`connect_session`]: pick + build the [`bpa_mcp::TransportConfig`] for `server`,
/// WITHOUT ever touching the network or spawning a process. Split out from `connect_session`
/// specifically so the env-filtering step (task T16, the whole point of this function existing)
/// is unit-testable on its own — asserting what env a stdio spawn WOULD use — without needing a
/// live Tokio child process or rmcp handshake in the test.
fn build_transport_config(
    server: McpServerRow,
) -> Result<bpa_mcp::TransportConfig, bpa_mcp::McpError> {
    match server.transport {
        McpTransport::Http => {
            let url = server.url.ok_or_else(|| {
                bpa_mcp::McpError::Protocol(
                    "mcp_server has no url (transport='http' requires one)".to_string(),
                )
            })?;
            Ok(bpa_mcp::TransportConfig::Http { url })
        }
        McpTransport::Stdio => {
            let command = server.command.ok_or_else(|| {
                bpa_mcp::McpError::Protocol(
                    "mcp_server has no command (transport='stdio' requires one)".to_string(),
                )
            })?;
            // The child inherits NOTHING on its own (bpa-mcp `env_clear()`s before applying this
            // map), so orchd must supply the COMPLETE env here: its own ambient env as the base
            // (for PATH/HOME/… the child needs to run) MERGED with the DB-configured `server.env`,
            // with the WHOLE result `DYLD_*`/`LD_*`-stripped.
            let env = build_stdio_env(std::env::vars(), server.env);
            Ok(bpa_mcp::TransportConfig::Stdio {
                command,
                args: server.args,
                env,
            })
        }
    }
}

/// Build the COMPLETE environment a stdio MCP child receives (task T16 review, Critical fix):
/// orchd's own `ambient` env as the base, the DB-configured `server_env` merged on top
/// (`server_env` wins on a key collision — "caller overrides" semantics), then the shared
/// `DYLD_*`/`LD_*` denylist applied to the ENTIRE result. This guarantees no dynamic-linker
/// injection var from EITHER source (orchd's ambient env OR the DB-configured server env) can
/// reach the child, while still handing the child a functional base env (the filtered ambient
/// PATH/HOME/… survives). Because `bpa_mcp::build_stdio_transport` `env_clear()`s before applying
/// this map, whatever this function returns IS the child's entire environment — nothing is
/// inherited implicitly.
///
/// `ambient` is a parameter (rather than reading `std::env::vars()` inside) so the merge/filter
/// logic is unit-testable with a synthetic ambient env; production passes `std::env::vars()`.
pub(crate) fn build_stdio_env(
    ambient: impl IntoIterator<Item = (String, String)>,
    server_env: BTreeMap<String, String>,
) -> BTreeMap<String, String> {
    let mut env: BTreeMap<String, String> = ambient.into_iter().collect();
    // `server_env` overrides the ambient base on any shared key (BTreeMap::extend = insert-or-
    // replace), matching sessiond's "caller overrides win last" env_overrides semantics.
    env.extend(server_env);
    bpa_daemon_core::env_filter::strip_dangerous_env_map(&mut env);
    env
}

/// Compute the [`crate::trust::Action`] required to (re)connect `server`'s live session (spec
/// §6/D10, task T16). Both `lifecycle::connect` (the explicit `McpConnect` verb) and
/// `invoke::call_tool` (Phase-1's per-call reconnect — there is no persisted session to check
/// once, spec: "connect-per-call is fine") call this SAME function immediately before ever
/// invoking [`connect_session`], so a stdio spawn can never reach [`bpa_mcp::connect`] via either
/// path without first passing through the SAME consent gate.
///
/// `Http` reuses the pre-existing `connect`-kind gate (URL fingerprint, unchanged — task T16
/// changes nothing about the http path, spec: "http server's 'connect' consent path stays
/// unchanged"). `Stdio` uses the distinct `stdio_exec` consent kind (spec §6/D6/D10) with
/// [`crate::trust::stdio_exec_fingerprint`]'s fingerprint over the CURRENT `command`/`args` —
/// see [`fingerprint_for`], which both this function and `TrustGrantConsent`'s dispatch handler
/// (grant time) call, so a grant and a later authorize check are always computed identically.
/// Default per-call MCP timeout used when a server's `timeout_ms` is non-positive (B2).
pub(crate) const DEFAULT_MCP_TIMEOUT_MS: u64 = 30_000;

/// The effective per-call timeout for an MCP server (B2). A non-positive `timeout_ms` — 0, or a
/// negative value that reached the row (the wire type is a signed i64 and `validate_new_server`
/// never floored it) — would make `tokio::time::timeout(ZERO, …)` elapse immediately, bricking
/// EVERY connect and tool-call on that server with a misleading `Timeout`. Treat non-positive as
/// the default; a positive value (however small — the tests deliberately use 50 ms) passes through.
pub(crate) fn effective_timeout(timeout_ms: i64) -> std::time::Duration {
    if timeout_ms <= 0 {
        std::time::Duration::from_millis(DEFAULT_MCP_TIMEOUT_MS)
    } else {
        std::time::Duration::from_millis(timeout_ms as u64)
    }
}

pub(crate) fn connect_action(server: &McpServerRow) -> crate::trust::Action {
    match server.transport {
        McpTransport::Http => crate::trust::Action::Connect {
            server_id: server.id.clone(),
            fingerprint: fingerprint_for(server, crate::trust::CONSENT_KIND_CONNECT),
        },
        McpTransport::Stdio => crate::trust::Action::StdioSpawn {
            server_id: server.id.clone(),
            fingerprint: fingerprint_for(server, crate::trust::CONSENT_KIND_STDIO_EXEC),
        },
    }
}

/// Compute the connect/spawn-consent fingerprint for `server` under consent `kind` (`'connect'`
/// | `'stdio_exec'`, spec §4 `consent_grant.kind`) — shared by [`connect_action`] (authorize
/// time) and `socket_server`'s `TrustGrantConsent` dispatch handler (grant time), so both always
/// derive the SAME value from the SAME server row and can never silently diverge (task T16).
pub(crate) fn fingerprint_for(server: &McpServerRow, kind: &str) -> String {
    if kind == crate::trust::CONSENT_KIND_STDIO_EXEC {
        crate::trust::stdio_exec_fingerprint(
            server.command.as_deref().unwrap_or(""),
            &server.args,
            &server.env,
        )
    } else {
        server.url.clone().unwrap_or_default()
    }
}

/// Resolve `server`'s bearer token from Keychain when `auth_kind == Bearer` (spec D4: secrets
/// live only in Keychain, `orchd.db` stores a ref, never the bytes). `None` for every other
/// `auth_kind` — Phase 1 doesn't drive MCP-server OAuth yet (spec D5/D14: Phase 2).
pub(crate) fn resolve_bearer(
    server: &McpServerRow,
) -> Result<Option<String>, bpa_secrets::SecretError> {
    if server.auth_kind != McpAuthKind::Bearer {
        return Ok(None);
    }
    let secret_ref = bpa_secrets::mcp_bearer_ref(&server.id);
    let bytes = bpa_secrets::get(&secret_ref)?;
    let token = String::from_utf8(bytes).map_err(|_| {
        bpa_secrets::SecretError::Keychain("stored bearer is not valid utf-8".to_string())
    })?;
    Ok(Some(token))
}

/// Domain error from the MCP connect/call flows (S-EXT §6, D10, task T5). NOT a wire type —
/// mapping this onto `bpa_orchd_proto::OrchdErrorCode`/`OrchdResponse::Error` at the verb
/// boundary is dispatch's job (a later task); `OrchdRequest::McpCallTool`'s own doc comment
/// already establishes today's precedent (`Error{Io}` for a disabled-tool denial, since no
/// dedicated `Consent`/`Policy` wire code exists yet) — this type exists so the underlying
/// reason stays distinguishable in Rust even before the wire mapping gets richer.
#[derive(Debug)]
pub enum OrchdMcpError {
    /// `trust::authorize` denied a `Connect` action (no matching `consent_grant`).
    ConsentRequired,
    /// `trust::authorize` denied a `ToolCall` action (`mcp_tool.enabled=0`, or unrecognized).
    ToolDisabled,
    /// `trust::authorize` denied a `ToolCall` action for a spend/rate POLICY-CAP breach (task
    /// T18, spec §6/BL-22) — distinct from [`OrchdMcpError::ToolDisabled`] (the per-tool
    /// allowlist denial) so the wire error message can honestly name WHICH cap tripped
    /// (`"rate_limit_exceeded"`/`"spend_cap_exceeded"`, the carried `String`) rather than reusing
    /// `ToolDisabled`'s fixed "tool disabled" text for an unrelated reason. Still maps to the
    /// SAME `Error{Policy}` wire code (spec §6) as `ToolDisabled` — a client already handling one
    /// denial kind handles the other; only the message differs.
    PolicyCapExceeded(String),
    /// The MCP session/transport itself failed — terminal.
    Mcp(bpa_mcp::McpError),
    /// A Keychain lookup failed while resolving a bearer token. Carries only a description
    /// (`bpa_secrets::SecretError`'s own `Display`, which never renders secret bytes), never the
    /// `SecretError` value itself, to keep this type simple.
    Secret(String),
    /// Underlying persistence failure.
    Persist(crate::persistence::OrchdPersistError),
}

impl std::fmt::Display for OrchdMcpError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OrchdMcpError::ConsentRequired => write!(f, "consent required"),
            OrchdMcpError::ToolDisabled => write!(f, "tool disabled"),
            OrchdMcpError::PolicyCapExceeded(reason) => write!(f, "policy cap exceeded: {reason}"),
            OrchdMcpError::Mcp(e) => write!(f, "mcp error: {e}"),
            OrchdMcpError::Secret(m) => write!(f, "secret error: {m}"),
            OrchdMcpError::Persist(e) => write!(f, "persistence error: {e}"),
        }
    }
}

impl std::error::Error for OrchdMcpError {}

impl From<crate::persistence::OrchdPersistError> for OrchdMcpError {
    fn from(e: crate::persistence::OrchdPersistError) -> Self {
        OrchdMcpError::Persist(e)
    }
}

/// [`ToolCaller`] test double (S-EXT task-5 brief: "in-memory Db + a FakeSession impl of
/// ToolCaller returning canned tools / a canned result / an injectable error"). `pub(crate)` so
/// `mcp::lifecycle`'s and `mcp::invoke`'s own `#[cfg(test)]` modules can share ONE fake rather
/// than each defining their own.
#[cfg(test)]
pub(crate) mod test_support {
    use std::collections::VecDeque;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::{Arc, Mutex};

    use bpa_mcp::{McpError, McpTool, McpToolResult};
    use serde_json::Value;

    use super::ToolCaller;

    /// Canned response for one `FakeSession::call_tool` attempt.
    pub(crate) enum FakeCallOutcome {
        Ok(McpToolResult),
        Err(McpError),
    }

    /// A [`ToolCaller`] test double: canned `list_tools`, a FIFO queue of canned `call_tool`
    /// outcomes (lets a test script "fail N times then succeed"), and a shared call counter
    /// (`Arc<AtomicUsize>`, supplied by the test so it can still read it after the session is
    /// dropped) tests assert on to prove retry behavior.
    pub(crate) struct FakeSession {
        tools: Vec<McpTool>,
        protocol_version: String,
        outcomes: Mutex<VecDeque<FakeCallOutcome>>,
        call_count: Arc<AtomicUsize>,
    }

    impl FakeSession {
        pub(crate) fn new(tools: Vec<McpTool>, call_count: Arc<AtomicUsize>) -> Self {
            Self {
                tools,
                protocol_version: "2025-11-25".to_string(),
                outcomes: Mutex::new(VecDeque::new()),
                call_count,
            }
        }

        pub(crate) fn with_outcomes(mut self, outcomes: Vec<FakeCallOutcome>) -> Self {
            self.outcomes = Mutex::new(outcomes.into_iter().collect());
            self
        }
    }

    impl ToolCaller for FakeSession {
        async fn list_tools(&self) -> Result<Vec<McpTool>, McpError> {
            Ok(self.tools.clone())
        }

        async fn call_tool(&self, _name: &str, _args: Value) -> Result<McpToolResult, McpError> {
            self.call_count.fetch_add(1, Ordering::SeqCst);
            let mut outcomes = self.outcomes.lock().unwrap();
            match outcomes.pop_front() {
                Some(FakeCallOutcome::Ok(result)) => Ok(result),
                Some(FakeCallOutcome::Err(err)) => Err(err),
                None => Ok(McpToolResult {
                    content: Value::Array(vec![]),
                    structured: None,
                    is_error: false,
                    usage: None,
                }),
            }
        }

        fn protocol_version(&self) -> String {
            self.protocol_version.clone()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trust::Action;

    #[test]
    fn effective_timeout_defaults_nonpositive_but_passes_small_positive() {
        use std::time::Duration;
        // A 0 (or clamped-negative) timeout_ms would make timeout(ZERO) fire instantly and brick
        // every call on the server — it must become the default, not zero (B2).
        assert_eq!(
            effective_timeout(0),
            Duration::from_millis(DEFAULT_MCP_TIMEOUT_MS)
        );
        assert_eq!(
            effective_timeout(-5),
            Duration::from_millis(DEFAULT_MCP_TIMEOUT_MS)
        );
        // A legitimate small-but-positive value passes through unchanged (tests use 50 ms).
        assert_eq!(effective_timeout(50), Duration::from_millis(50));
        assert_eq!(effective_timeout(5_000), Duration::from_millis(5_000));
    }

    fn stdio_server(command: &str, env: BTreeMap<String, String>) -> McpServerRow {
        McpServerRow {
            id: "srv-1".to_string(),
            name: "local".to_string(),
            transport: McpTransport::Stdio,
            url: None,
            command: Some(command.to_string()),
            args: vec!["--flag".to_string()],
            env,
            scope: McpScope::Global,
            project_id: None,
            auth_kind: McpAuthKind::None,
            secret_ref: None,
            account_id: None,
            enabled: true,
            timeout_ms: 30_000,
            max_retries: 2,
            protocol_version: None,
            created_at: 0,
            updated_at: 0,
        }
    }

    fn http_server(url: &str) -> McpServerRow {
        McpServerRow {
            id: "srv-2".to_string(),
            name: "remote".to_string(),
            transport: McpTransport::Http,
            url: Some(url.to_string()),
            command: None,
            args: vec![],
            env: BTreeMap::new(),
            scope: McpScope::Global,
            project_id: None,
            auth_kind: McpAuthKind::None,
            secret_ref: None,
            account_id: None,
            enabled: true,
            timeout_ms: 30_000,
            max_retries: 2,
            protocol_version: None,
            created_at: 0,
            updated_at: 0,
        }
    }

    // ---- build_transport_config: DYLD/LD denylist at the stdio spawn call site (task T16,
    // closes the "second unfiltered spawn path" gap alongside sessiond's BL-1 fix) ----

    #[test]
    fn build_transport_config_stdio_strips_dyld_and_ld_but_keeps_benign_env() {
        let mut env = BTreeMap::new();
        env.insert(
            "DYLD_INSERT_LIBRARIES".to_string(),
            "/evil.dylib".to_string(),
        );
        env.insert("LD_PRELOAD".to_string(), "/evil.so".to_string());
        env.insert("FOO".to_string(), "bar".to_string());
        let server = stdio_server("/usr/bin/true", env);

        let cfg = build_transport_config(server).expect("stdio config must build");
        match cfg {
            bpa_mcp::TransportConfig::Stdio { command, args, env } => {
                assert_eq!(command, "/usr/bin/true");
                assert_eq!(args, vec!["--flag".to_string()]);
                assert!(
                    !env.contains_key("DYLD_INSERT_LIBRARIES"),
                    "DYLD_INSERT_LIBRARIES must never reach a stdio spawn's env: {env:?}"
                );
                assert!(
                    !env.contains_key("LD_PRELOAD"),
                    "LD_PRELOAD must never reach a stdio spawn's env: {env:?}"
                );
                assert_eq!(env.get("FOO").map(String::as_str), Some("bar"));
            }
            other => panic!("expected Stdio, got {other:?}"),
        }
    }

    #[test]
    fn build_transport_config_http_is_unaffected_by_env_filtering() {
        let server = http_server("https://example.com/mcp");
        let cfg = build_transport_config(server).expect("http config must build");
        match cfg {
            bpa_mcp::TransportConfig::Http { url } => {
                assert_eq!(url, "https://example.com/mcp")
            }
            other => panic!("expected Http, got {other:?}"),
        }
    }

    // ---- build_stdio_env: the COMPLETE child env = filtered(ambient ∪ server.env), server wins
    // (task T16 review Critical — the child inherits nothing on its own, so orchd must supply the
    // whole env, and NO DYLD_*/LD_* from EITHER source may survive). Pure/synthetic ambient →
    // deterministic + parallel-safe (no process-global env mutation). ----

    #[test]
    fn build_stdio_env_merges_filtered_ambient_with_server_env_server_wins() {
        let ambient = vec![
            // benign ambient base the child needs — must survive
            ("PATH".to_string(), "/usr/bin:/bin".to_string()),
            ("HOME".to_string(), "/home/x".to_string()),
            // dangerous ambient (orchd's OWN env being poisoned) — must be stripped
            (
                "DYLD_INSERT_LIBRARIES".to_string(),
                "/ambient-evil.dylib".to_string(),
            ),
            ("LD_PRELOAD".to_string(), "/ambient-evil.so".to_string()),
            // a key the server also sets — server must win
            ("SHARED".to_string(), "ambient-value".to_string()),
        ];
        let mut server_env = BTreeMap::new();
        server_env.insert("FOO".to_string(), "bar".to_string());
        // dangerous DB-configured server env — must ALSO be stripped
        server_env.insert("LD_LIBRARY_PATH".to_string(), "/server-evil".to_string());
        server_env.insert("SHARED".to_string(), "server-value".to_string());

        let env = build_stdio_env(ambient, server_env);

        // benign ambient survives (child stays functional):
        assert_eq!(env.get("PATH").map(String::as_str), Some("/usr/bin:/bin"));
        assert_eq!(env.get("HOME").map(String::as_str), Some("/home/x"));
        // server.env merged in:
        assert_eq!(env.get("FOO").map(String::as_str), Some("bar"));
        // server wins on a shared key ("caller overrides" semantics):
        assert_eq!(env.get("SHARED").map(String::as_str), Some("server-value"));
        // NO dynamic-linker vars from EITHER source:
        assert!(
            !env.contains_key("DYLD_INSERT_LIBRARIES"),
            "ambient DYLD must be stripped: {env:?}"
        );
        assert!(
            !env.contains_key("LD_PRELOAD"),
            "ambient LD_PRELOAD must be stripped: {env:?}"
        );
        assert!(
            !env.contains_key("LD_LIBRARY_PATH"),
            "server-configured LD_LIBRARY_PATH must be stripped: {env:?}"
        );
    }

    // ---- REAL spawned-process proof of the COMPLETE orchd path (task T16 review Critical): plant
    // DYLD/LD in THIS (orchd-standin) process's ambient env, build the stdio config through
    // build_transport_config (which reads std::env::vars()), then spawn /usr/bin/env with that env
    // under env_clear (bpa-mcp's contract, independently proven in crates/mcp/src/transport.rs) and
    // read the child's ACTUAL environment. Proves ambient DYLD/LD never reach the child while the
    // filtered ambient base (PATH) does. ----
    #[tokio::test]
    async fn stdio_child_gets_filtered_ambient_no_dyld_via_orchd_path() {
        std::env::set_var("DYLD_INSERT_LIBRARIES", "/ambient-evil.dylib");
        std::env::set_var("LD_PRELOAD", "/ambient-evil.so");
        std::env::set_var("BPA_ORCHD_STDIO_ENV_TEST_MARKER", "benign-ambient");

        let mut server_env = BTreeMap::new();
        server_env.insert("FOO".to_string(), "bar".to_string());
        let server = stdio_server("/usr/bin/env", server_env);

        let cfg = build_transport_config(server).expect("stdio config must build");
        let bpa_mcp::TransportConfig::Stdio { env, .. } = cfg else {
            panic!("expected Stdio config");
        };

        // Spawn /usr/bin/env with the built env under env_clear — mirrors bpa-mcp's
        // build_stdio_transport contract (env_clear then envs), which the crate proves itself.
        let output = tokio::process::Command::new("/usr/bin/env")
            .env_clear()
            .envs(&env)
            .output()
            .await
            .expect("/usr/bin/env should spawn and exit");
        let text = String::from_utf8_lossy(&output.stdout);

        assert!(
            !text.contains("DYLD_INSERT_LIBRARIES"),
            "orchd's ambient DYLD must NOT reach the stdio child: {text:?}"
        );
        assert!(
            !text.contains("LD_PRELOAD"),
            "orchd's ambient LD_PRELOAD must NOT reach the stdio child: {text:?}"
        );
        assert!(
            text.contains("BPA_ORCHD_STDIO_ENV_TEST_MARKER=benign-ambient"),
            "a benign ambient var MUST reach the child (ambient base is included): {text:?}"
        );
        assert!(
            text.contains("FOO=bar"),
            "the DB-configured server.env must reach the child: {text:?}"
        );

        std::env::remove_var("DYLD_INSERT_LIBRARIES");
        std::env::remove_var("LD_PRELOAD");
        std::env::remove_var("BPA_ORCHD_STDIO_ENV_TEST_MARKER");
    }

    // ---- connect_action / fingerprint_for (task T16) ----

    #[test]
    fn connect_action_for_http_is_connect_kind_with_url_fingerprint() {
        let server = http_server("https://example.com/mcp");
        match connect_action(&server) {
            Action::Connect {
                server_id,
                fingerprint,
            } => {
                assert_eq!(server_id, "srv-2");
                assert_eq!(fingerprint, "https://example.com/mcp");
            }
            other => panic!("expected Action::Connect, got {other:?}"),
        }
    }

    #[test]
    fn connect_action_for_stdio_is_stdio_spawn_kind_with_command_fingerprint() {
        let server = stdio_server("/usr/bin/true", BTreeMap::new());
        match connect_action(&server) {
            Action::StdioSpawn {
                server_id,
                fingerprint,
            } => {
                assert_eq!(server_id, "srv-1");
                assert_eq!(
                    fingerprint,
                    crate::trust::stdio_exec_fingerprint(
                        "/usr/bin/true",
                        &["--flag".to_string()],
                        &Default::default()
                    )
                );
            }
            other => panic!("expected Action::StdioSpawn, got {other:?}"),
        }
    }

    #[test]
    fn fingerprint_for_grant_time_matches_connect_action_at_authorize_time() {
        // The load-bearing property: `TrustGrantConsent`'s dispatch handler (grant time) and
        // `connect_action` (authorize time) must derive the IDENTICAL fingerprint from the same
        // server row, or a freshly-granted consent would immediately fail its own re-check.
        let server = stdio_server("/usr/bin/true", BTreeMap::new());
        let granted = fingerprint_for(&server, crate::trust::CONSENT_KIND_STDIO_EXEC);
        let Action::StdioSpawn { fingerprint, .. } = connect_action(&server) else {
            panic!("expected Action::StdioSpawn");
        };
        assert_eq!(granted, fingerprint);
    }
}
