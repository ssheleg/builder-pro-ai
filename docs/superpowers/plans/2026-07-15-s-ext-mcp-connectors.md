# S-EXT — MCP client + Connectors + Skills — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give the app an outbound extension layer hosted in `bpa-orchd` — connect to MCP servers, discover/invoke tools (typed, retried, gated, audited), persist results as durable artifacts, hold external OAuth accounts in Keychain, register SKILL.md skills — all managed from a «Расширения» UI.

**Architecture:** First application-driven egress + Keychain surface, all in `bpa-orchd` (never Hop-B). Buy the MCP protocol (`rmcp` official SDK) behind a thin `bpa-mcp` wrapper; `bpa-secrets` wraps macOS Keychain. `orchd.db` schema v3 (additive) holds the registry/cache/invocations/artifacts/accounts/skills/trust rows; `orchd-proto` gains append-only `Mcp*/Connector*/Skill*` verbs; core proxies to a new `ExtPanel` UI. A single `trust.rs` choke-point gates every connect/spawn/call with consent + policy + audit.

**Tech Stack:** Rust (tokio, rusqlite, `bpa-daemon-core::migrate`), `rmcp = "2.2"`, `security-framework = "3.7"`, `oauth2 = "5.0"`, `reqwest = "0.13"` (rustls), React 19 / Zustand / Vite, Node e2e harness.

**Spec:** `docs/superpowers/specs/2026-07-15-s-ext-mcp-connectors-design.md` — all §§ referenced below are that spec. The spec's §4 (DDL) and §5 (wire verbs) are the **verbatim contracts**; tasks reference them rather than re-pasting (DRY).

## Global Constraints (every task implicitly includes these)

- **Deps pinned** (spec §1): `rmcp = { version = "2.2", features = ["client","transport-child-process","transport-streamable-http-client-reqwest","auth"] }`; `security-framework = "3.7"`; `oauth2 = "5.0"` (async reqwest only, NO `reqwest-blocking`); `reqwest = { version = "0.13", default-features = false, features = ["rustls","json","stream"] }`. Add `"process"` to `tokio` features (Phase 3). rmcp `auth_header` takes the bare bearer WITHOUT `"Bearer "`.
- **Host = `bpa-orchd`**; egress ONLY in `bpa-mcp`/`connectors`; never Hop-B (sessiond untouched).
- **Wire layering** (spec §5): frame enums `OrchdRequest/Response/Push` stay plain snake_case Hop-B-only (derive `Debug,Clone,Serialize,Deserialize,PartialEq` — NO ts-rs, NO camelCase), variants appended at END; new ENTITY structs get `#[serde(rename_all="camelCase")]` + `#[derive(...,ts_rs::TS)]` + `#[ts(export_to="orchd-types.ts")]`, i64 timestamps `#[ts(type="number")]` (mirror `GraphNode`).
- **Secrets in Keychain only** (`bpa-secrets`); `orchd.db` stores refs, never token bytes; no secret in any log.
- **Trust choke-point**: every connect / stdio-spawn / tool-call / connector-invoke passes `trust::authorize` before dispatch; each writes an `audit_log` row; results are `is_untrusted=1`.
- **Migration**: additive `Migration{upto:3}` mirroring `migrate_v2` (`crates/orchd/src/persistence.rs`), whole-chain single-tx, forward-only.
- **Production-grade**: TDD (failing test → confirm fail → minimal impl → pass → commit); honest error degradation on every external call; structured logs w/o secrets; docs in the same slice.
- **Gate**: `bash scripts/final-suite.sh` → `ALL GATES PASSED` (9 stages); orchd coverage ≥80%; ts-rs parity; no env-fragile wall-clock asserts (S4 lesson: strict timing only in release). Known BL-40 attach PTY flake → retry-once. Commit trailer `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`.
- **PATH for Rust**: `export PATH="$HOME/.cargo/bin:$HOME/.rustup/toolchains/1.92-aarch64-apple-darwin/bin:/opt/homebrew/bin:$PATH"`. Frontend: `npx vitest run`, `npx tsc --noEmit`. Gate needs staged sidecars (`cargo build -p bpa-sessiond -p bpa-orchd` → copy to `src-tauri/binaries/bpa-{sessiond,orchd}-aarch64-apple-darwin`, gitignored).

## Dependency graph & parallel groups

```
P1 (testable milestone):
  T1 (deps + bpa-secrets)  ─┐
  T2 (schema v3 + registry persistence) depends T1(workspace)   ─┐
  T3 (orchd-proto P1 entities+verbs)                            ─┤ contracts-first, SEQUENTIAL
  T4 (bpa-mcp wrap rmcp)   depends T1                            ─┘
  T5 (orchd mcp module + trust) depends T2,T3,T4
  T6 (socket dispatch P1)  depends T3,T5
  T7 (core commands+broker P1) depends T3(ts),T6
  T8 (frontend ipc+store+ExtPanel P1) depends T7
  T9 (e2e stub-MCP + gate) depends T6  [T8 parallel-safe w/ T9: disjoint files]
P2 (OAuth + connectors):
  T10 (proto P2 entities+verbs) SEQUENTIAL
  T11 (bpa-secrets refresh helpers + accounts.rs OAuth/oauth2) depends T10
  T12 (connectors adapter + generic-rest + ConnectorInvoke) depends T11
  T13 (dispatch P2 + core + accounts UI) depends T11,T12  [split: 13a orchd/core, 13b frontend — parallel]
  T14 (e2e connector phase + gate)
P3 (stdio + skills + caps + hardening + docs):
  T15 (tokio process + stdio transport in bpa-mcp) 
  T16 (DYLD/LD denylist shared helper + orchd stdio-spawn + close BL-1 in sessiond)  [depends T15]
  T17 (skills registry + proto + dispatch + SkillsTab)  [parallel w/ T15/16 — disjoint]
  T18 (spend/rate caps policy + list_changed refresh + InvocationLog/Artifacts/Journal tabs)
  T19 (docs truth + CHANGELOG [0.6.0] + backlog deltas + gate)
  T20 (whole-branch review + merge + CI green)
```

---

## PHASE 1 — testable milestone

### Task 1: workspace deps + `bpa-secrets` crate (Keychain)

**Files:**
- Modify: root `Cargo.toml` (`[workspace] members`, `[workspace.dependencies]`)
- Create: `crates/secrets/Cargo.toml`, `crates/secrets/src/lib.rs`

**Interfaces — Produces:**
- `bpa_secrets::SecretRef { service: String, account: String }`
- `bpa_secrets::set(r: &SecretRef, secret: &[u8]) -> Result<(), SecretError>` (upsert)
- `bpa_secrets::get(r: &SecretRef) -> Result<Vec<u8>, SecretError>`
- `bpa_secrets::delete(r: &SecretRef) -> Result<(), SecretError>`
- `bpa_secrets::mcp_bearer_ref(server_id: &str) -> SecretRef` (service `"ai.builderpro.desktop.mcp"`, account = server_id)
- `bpa_secrets::account_ref(account_id: &str, kind: &str) -> SecretRef` (service `"ai.builderpro.desktop.account"`, account = `"{account_id}:{kind}"`, kind ∈ {`token`,`refresh`,`apikey`})
- `enum SecretError { NotFound, Keychain(String) }` (Display never prints the secret bytes)

- [ ] **Step 1: RED.** `crates/secrets/src/lib.rs` `#[cfg(test)]`: `set` a unique `SecretRef` (service `"ai.builderpro.desktop.test"`, account = a random-ish per-test string derived from the test name — NO `Math.random`/`Date`, use a fixed unique literal per test), `get` returns the bytes, `set` again updates, `delete` then `get` → `Err(NotFound)`. Teardown deletes. A second test: `SecretError` Display for a planted secret value does NOT contain the secret bytes. Run `cargo test -p bpa-secrets` → FAIL (crate empty).
- [ ] **Step 2: GREEN.** `Cargo.toml`: `[package] name="bpa-secrets"`, dep `security-framework = "3.7"`, `thiserror = { workspace = true }`. `lib.rs`: wrap `security_framework::passwords::{set_generic_password, get_generic_password, delete_generic_password}` (signature `(service:&str, account:&str, secret:&[u8])`; `get`→`Vec<u8>`; map the `NotFound` error variant to `SecretError::NotFound`, others to `Keychain(msg)` — msg must NOT include the secret). Add `bpa-secrets` to root `[workspace] members` and `[workspace.dependencies] bpa-secrets = { path = "crates/secrets" }`.
- [ ] **Step 3:** `cargo test -p bpa-secrets`, `cargo clippy -p bpa-secrets --all-targets -- -D warnings`, `cargo fmt --all -- --check`. Confirm hermetic (delete-in-teardown; real login keychain not polluted — use the `.test` service prefix + delete). Commit `feat(secrets): bpa-secrets — macOS Keychain generic-password wrapper (S-EXT §3, D4, BL-20)`.

### Task 2: `orchd.db` schema v3 + registry persistence

**Files:**
- Modify: `crates/orchd/src/persistence.rs` (SCHEMA_VERSION 2→3; add `Migration{upto:3}`)
- Create: `crates/orchd/src/mcp/mod.rs`, `crates/orchd/src/mcp/registry.rs`
- Modify: `crates/orchd/src/lib.rs` (`pub mod mcp;` if not via persistence)

**Interfaces — Consumes:** T1 (workspace). **Produces:** on `persistence::Db`:
- `add_mcp_server(NewMcpServer) -> Result<McpServerRow, OrchdPersistError>` (validates scope↔project_id invariant, transport↔url invariant per §4 CHECKs)
- `list_mcp_servers(project_id: Option<&str>) -> Result<Vec<McpServerRow>, _>` (global + the project's)
- `update_mcp_server(id, McpServerPatch) -> Result<McpServerRow, _>`, `set_mcp_server_enabled(id, bool)`, `delete_mcp_server(id) -> Result<(), _>`
- `set_mcp_server_secret_ref(id, secret_ref: &str)`
- `upsert_mcp_tools(server_id, Vec<NewMcpTool>) -> Result<(), _>` (replace cache for the server), `list_mcp_tools(server_id) -> Vec<McpToolRow>`, `set_mcp_tool_enabled(tool_id, bool) -> McpToolRow`, `get_mcp_tool(tool_id) -> Option<McpToolRow>`
- Row structs mirror §4 columns (snake_case Rust; `id: String` uuid; timestamps `i64` via `now_ms()`).

- [ ] **Step 1: RED.** `crates/orchd/src/mcp/registry.rs` `#[cfg(test)]` (in-memory Db per the existing graph.rs test pattern — open Db, run migrations): `add_mcp_server` (http, global) round-trips; a `scope='project'` with `project_id=None` → `Validation`; a `transport='http'` with `url=None` → `Validation`; `list_mcp_servers(Some(p))` returns global + p's, not another project's; `set_mcp_server_enabled` flips; `delete_mcp_server` cascades tools; `upsert_mcp_tools` replaces; `set_mcp_tool_enabled` flips `enabled`. Assert schema is v3 after migration. Run `cargo test -p bpa-orchd mcp::registry` → FAIL.
- [ ] **Step 2: GREEN.** In `persistence.rs`: bump `SCHEMA_VERSION` to 3; append `Migration { upto: 3, sql: <the §4 DDL, ALL tables verbatim> }` to the const array (mirror how `migrate_v2` added graph tables — `execute_batch` of the CREATE TABLE/INDEX statements; no backfill). Implement the registry methods in `mcp/registry.rs` (parameterized rusqlite; enum⇄TEXT for transport/scope/auth_kind; propagate errors via `?`→`OrchdPersistError`). `mcp/mod.rs`: `pub mod registry;` + shared row structs. Wire `pub mod mcp;` into the orchd lib.
- [ ] **Step 3:** `cargo test -p bpa-orchd mcp`, clippy, fmt. Commit `feat(orchd): schema v3 + MCP server/tool registry persistence (S-EXT §4)`.

### Task 3: `orchd-proto` — Phase-1 entities + verbs

**Files:**
- Modify: `crates/orchd-proto/src/lib.rs` (append entity structs + `OrchdRequest`/`OrchdResponse`/`OrchdPush` variants at END)
- Modify: `src/ipc/orchd-types.ts` (regenerated by the ts-rs export test)

**Interfaces — Produces (Phase-1 subset of §5; entity structs are camelCase+ts-rs, frame variants snake_case):**
- Entities: `McpServer`, `McpTool`, `McpConnectReport { protocol_version:String, tool_count:i64 }`, `McpCallResult { artifact_id:String, invocation_id:String, content_json:String, is_error:bool }`, `McpInvocation`, `McpArtifact`.
- Requests: `McpAddServer{...}`, `McpListServers{project_id:Option<String>}`, `McpUpdateServer{...}`, `McpSetServerEnabled{id,enabled}`, `McpDeleteServer{id}`, `McpSetServerBearer{id,token}`, `McpConnect{id}`, `McpDisconnect{id}`, `McpListTools{server_id}`, `McpSetToolEnabled{tool_id,enabled}`, `McpCallTool{server_id,tool_name,args_json,project_id:Option<String>}`, `McpListInvocations{server_id:Option<String>,project_id:Option<String>,limit:Option<i64>}`, `McpListArtifacts{project_id:Option<String>,server_id:Option<String>,limit:Option<i64>}`, `McpGetArtifact{id}`, `TrustGrantConsent{server_id,kind:String}`.
- Responses: `McpServer(McpServer)`, `McpServers(Vec<McpServer>)`, `McpTool(McpTool)`, `McpTools(Vec<McpTool>)`, `McpConnectReport(McpConnectReport)`, `McpCallResult(McpCallResult)`, `McpInvocations(Vec<McpInvocation>)`, `McpArtifacts(Vec<McpArtifact>)`, `McpArtifact(McpArtifact)` (reuse existing `Ack`, `Error{kind,message}`).
- Pushes: `McpServersChanged{project_id:Option<String>}`, `McpToolsChanged{server_id:String}`, `McpArtifactsChanged{project_id:Option<String>}`, `McpInvocationLogged{server_id:String}`.

- [ ] **Step 1: RED.** In `crates/orchd-proto/src/lib.rs` `#[cfg(test)]`: wire-tag tests — `serde_json::to_string(&OrchdResponse::McpServers(vec![]))` deserializes back equal (frame round-trip, snake_case); an `McpServer` entity serializes with camelCase keys (`createdAt`, `projectId`) — assert the exact serialized key is present. A `ts_export` test regenerates `orchd-types.ts` and it contains `McpServer`/`McpTool`/`McpArtifact`/`McpCallResult` with `isOrphan`-style camelCase + `createdAt: number`. Run `cargo test -p bpa-orchd-proto` → FAIL.
- [ ] **Step 2: GREEN.** Append the entity structs (mirror the `GraphNode` derive block byte-for-byte incl. `#[ts(export_to="orchd-types.ts")]` and `#[ts(type="number")]` on i64 fields) and the frame variants (plain snake_case, appended at END of each enum) per §5. Run the ts-export test to regenerate `orchd-types.ts`; commit the regenerated `.ts`.
- [ ] **Step 3:** `cargo test -p bpa-orchd-proto`, clippy, fmt; confirm `orchd-types.ts` gained the entity types. NB: `bpa-orchd` will NOT yet compile (socket_server has no dispatch arm for the new verbs) — add a temporary wildcard arm `OrchdRequest::Mcp*{..} | OrchdRequest::TrustGrantConsent{..} => OrchdResponse::Error{kind:Io,message:"not yet implemented"}` in `socket_server.rs` dispatch so the crate builds; T6 replaces it. Commit `feat(orchd-proto): S-EXT phase-1 MCP wire — entities + verbs + pushes (append-only) (S-EXT §5)`.

### Task 4: `bpa-mcp` crate — wrap rmcp (HTTP client)

**Files:**
- Modify: root `Cargo.toml` (member + workspace dep; add `reqwest` rustls to `[workspace.dependencies]`)
- Create: `crates/mcp/Cargo.toml`, `crates/mcp/src/{lib.rs,client.rs,transport.rs,types.rs,error.rs}`

**Interfaces — Consumes:** T1. **Produces:**
- `bpa_mcp::TransportConfig` — Phase 1: `Http { url: String }` (stdio variant added T15)
- `async bpa_mcp::connect(cfg: TransportConfig, bearer: Option<String>) -> Result<McpSession, McpError>`
- `McpSession::{ async list_tools() -> Result<Vec<McpTool>, McpError>, async call_tool(name:&str, args: serde_json::Value) -> Result<McpToolResult, McpError>, protocol_version() -> String, async close() }`
- `McpTool { name:String, title:Option<String>, description:Option<String>, input_schema: serde_json::Value }`
- `McpToolResult { content: serde_json::Value, structured: Option<serde_json::Value>, is_error: bool, usage: Option<Usage> }`, `Usage { input_tokens:Option<i64>, output_tokens:Option<i64>, cost_usd:Option<f64> }`
- `enum McpError { Transport(String), Protocol(String), Timeout, ToolError(String), Auth(String) }`

- [ ] **Step 1: RED.** `crates/mcp/tests/stub.rs`: build an in-process rmcp **stub server** (rmcp `server` feature, dev-dep) exposing 2 tools (`echo`, `add`), connect the client over a `tokio::io::duplex()` transport pair, assert `list_tools()` returns `echo`+`add` with their schemas; `call_tool("echo", {"msg":"hi"})` echoes; a tool the server marks error → `McpToolResult.is_error=true`; `call_tool` on an unknown tool → `McpError`. Run `cargo test -p bpa-mcp` → FAIL.
- [ ] **Step 2: GREEN.** `Cargo.toml`: `rmcp` (the pinned features), `reqwest` (rustls), `tokio`, `serde_json`, `thiserror`; dev-dep `rmcp` with `server`. `transport.rs`: `Http{url}` → `StreamableHttpClientTransport::from_uri(url)`, apply `StreamableHttpClientTransportConfig::auth_header(bearer)` when `bearer` present (bare token, no `"Bearer "`). `client.rs`: `().serve(transport)` → `RunningService<RoleClient>`; `list_tools` → `peer().list_all_tools()` mapped to `McpTool`; `call_tool` → `peer().call_tool(CallToolRequestParams{name, arguments})` mapped to `McpToolResult` (map rmcp `CallToolResult.is_error`, content, and any usage). `types.rs`/`error.rs`: the mapping + typed errors (rmcp `ServiceError`/transport errors → `McpError`; a wrapping `tokio::time::timeout(cfg-derived)` → `McpError::Timeout`). NB the duplex stub proves the client path without HTTP; the same `McpSession` API serves real HTTP in T5. Add `bpa-mcp` to workspace members/deps.
- [ ] **Step 3:** `cargo test -p bpa-mcp`, clippy, fmt. Commit `feat(mcp): bpa-mcp — rmcp client wrapper (HTTP transport, list/call tools, typed errors) (S-EXT §3, D2)`.

### Task 5: orchd MCP module — lifecycle + invoke + trust choke-point

**Files:**
- Create: `crates/orchd/src/mcp/lifecycle.rs`, `crates/orchd/src/mcp/invoke.rs`, `crates/orchd/src/mcp/cache.rs`, `crates/orchd/src/trust.rs`
- Modify: `crates/orchd/src/mcp/mod.rs`, `crates/orchd/src/persistence.rs` (invocation/artifact/consent/audit CRUD), `crates/orchd/src/lib.rs`

**Interfaces — Consumes:** T2 (registry), T3 (types), T4 (bpa-mcp). **Produces:**
- `persistence::Db` gains: `insert_invocation(NewInvocation)->InvocationRow`, `insert_artifact(NewArtifact)->ArtifactRow`, `list_invocations(...)`, `list_artifacts(...)`, `get_artifact(id)`, `has_consent(server_id,kind)->bool`, `grant_consent(server_id,kind,fingerprint)`, `insert_audit(AuditRow)`.
- `trust::authorize(db, Action, ctx) -> Decision` where `Action ∈ {Connect{server_id,fingerprint}, ToolCall{server_id,tool_name,project_id}}`; returns `Decision::Allow` or `Decision::Deny{reason}`; ALWAYS writes an `audit_log` row; Connect without a `consent_grant` → `Deny{reason:"consent_required"}`; a disabled tool → `Deny{reason:"tool_disabled"}` (Phase-1 policy scope; spend/rate caps are T18).
- `mcp::lifecycle::connect(db, server_id) -> Result<McpConnectReport>`: trust-authorize(Connect) → resolve bearer via `bpa_secrets::get(mcp_bearer_ref)` if `auth_kind='bearer'` → `bpa_mcp::connect(Http{url}, bearer)` → `list_tools` → `upsert_mcp_tools` → return report (protocol_version, tool_count). Push `McpToolsChanged` is emitted by the dispatch layer (T6), not here.
- `mcp::invoke::call_tool(db, server_id, tool_name, args_json, project_id) -> Result<McpCallResult>`: trust-authorize(ToolCall) (rejects disabled tool) → connect a session (or reuse; Phase-1 may connect-per-call) → `call_tool` with timeout+retry per `mcp_server.{timeout_ms,max_retries}` (retry only transport/pre-dispatch errors, NEVER blind re-invoke) → write `mcp_invocation` (latency, ok, error_kind, cost/tokens from `usage`) → on success write `mcp_artifact` (is_untrusted=1) → return `McpCallResult`.

- [ ] **Step 1: RED.** `crates/orchd/src/mcp/invoke.rs` + `trust.rs` `#[cfg(test)]` (in-memory Db + a fake session seam — inject a `TestSession` trait impl so no network/rmcp needed; define `trait ToolCaller { async fn list_tools(); async fn call_tool(); }` that `bpa_mcp::McpSession` implements and tests fake): connect without consent → `Deny{consent_required}` + an audit row written; after `grant_consent`, connect caches tools + returns report; `call_tool` on an enabled tool → writes invocation + artifact (is_untrusted=1), returns `McpCallResult`; `call_tool` on a disabled tool → `Deny{tool_disabled}`, NO invocation/artifact, audit row written; a transport error → invocation `ok=0, error_kind` set, NO artifact; retry retries a transport error but a `ToolError` is not retried. Run `cargo test -p bpa-orchd mcp::invoke trust` → FAIL.
- [ ] **Step 2: GREEN.** Add the persistence CRUD (invocation/artifact/consent/audit tables per §4). Implement `trust.rs` (the choke-point; audit-always; consent + per-tool-allowlist checks). Implement `lifecycle.rs`/`invoke.rs` against a `ToolCaller` seam (prod impl = `bpa_mcp::McpSession`; test impl = fake). `request_hash` = sha256(args_json) via `sha2` (already a workspace dep — confirm; else add). Structured `tracing` on connect/call/deny WITHOUT secrets/args.
- [ ] **Step 3:** `cargo test -p bpa-orchd mcp trust`, clippy, fmt. Commit `feat(orchd): MCP lifecycle + invoke + trust choke-point (consent, per-tool allowlist, audit, artifacts) (S-EXT §6, D9/D10)`.

### Task 6: orchd socket dispatch — Phase-1 verbs + pushes

**Files:** Modify `crates/orchd/src/socket_server.rs` (replace the T3 wildcard arm with real per-verb arms + push fan-out).

**Interfaces — Consumes:** T3, T5. **Produces:** dispatch for all Phase-1 verbs (§5); mutating verbs push on success only; `McpConnect`→`McpToolsChanged{server_id}`; `McpCallTool`→`McpArtifactsChanged{project_id}`+`McpInvocationLogged{server_id}`; `McpAdd/Update/SetEnabled/Delete/SetBearer`→`McpServersChanged{project_id}`; `McpSetToolEnabled`→`McpToolsChanged{server_id}`; read verbs push nothing; `Err`→`map_err`, nothing broadcast. `McpConnect` without consent → the `trust` Deny surfaces as `OrchdResponse::Error{kind:"Consent", message}`.

- [ ] **Step 1: RED.** `crates/orchd/tests/dispatch_integration.rs` (stub client over `run()`, orchd version consts, temp HOME): `McpAddServer`→`McpServer` + a listener gets `McpServersChanged`; `McpConnect` on a server, using a **stub MCP server the test spawns** (spawn a tiny in-process rmcp HTTP server bound to a loopback port, or reuse the bpa-mcp duplex seam via a test-only orchd hook) without consent → `Error{Consent}`; after `TrustGrantConsent` → `McpConnectReport` + `McpToolsChanged`; `McpCallTool`→`McpCallResult` + `McpArtifactsChanged` + `McpInvocationLogged`; `McpSetToolEnabled(disabled)` then `McpCallTool` → `Error{Policy}` no artifact. Run `cargo test -p bpa-orchd --test dispatch_integration` → FAIL.
- [ ] **Step 2: GREEN.** Replace the wildcard arm; wire each verb to its `mcp`/`trust`/`registry` call; broadcast per the rules (mirror the S4 `broadcast_graph_changed` helper style for any fan-out). For the connect/call tests, provide a test seam so orchd can be pointed at a loopback stub MCP server (a `#[cfg(test)]` server spawned in the test).
- [ ] **Step 3:** `cargo test -p bpa-orchd` (full crate), clippy, fmt. Commit `feat(orchd): MCP socket dispatch + coarse-invalidation pushes (S-EXT §5/§6)`.

### Task 7: core commands + broker events — Phase 1

**Files:** Modify `src-tauri/src/commands.rs`, `src-tauri/src/broker.rs`, `src-tauri/src/lib.rs`.

**Interfaces — Consumes:** T3 (ts types), T6. **Produces:** `mcp_add_server`, `mcp_list_servers`, `mcp_update_server`, `mcp_set_server_enabled`, `mcp_delete_server`, `mcp_set_server_bearer`, `mcp_connect`, `mcp_disconnect`, `mcp_list_tools`, `mcp_set_tool_enabled`, `mcp_call_tool`, `mcp_list_invocations`, `mcp_list_artifacts`, `mcp_get_artifact`, `trust_grant_consent` — each proxies `state.orchd()?.request(OrchdRequest::…)`, matches the response variant, maps `Error{kind,message}`→`CommandError::Daemon{code,message}`. Broker `EV_ORCHD_MCP_SERVERS_CHANGED="orchd://mcp-servers-changed"` (+ tools/artifacts/invocation-logged), `map_orchd_push` arms → camelCase `{serverId}`/`{projectId}` payloads (mirror `GoalsChanged`). Register all commands in `generate_handler!`.

- [ ] **Step 1: RED.** commands stub-orchd tests (mirror existing `orchd_*` command tests): `mcp_add_server` happy; an `Error{kind:"Consent"}` from `mcp_connect` → `CommandError::Daemon{code:"Consent"}`. broker unit: each new `OrchdPush::Mcp*` → the right `EV_ORCHD_MCP_*` + camelCase payload key present, snake_case absent. Run `cargo test -p builder-pro-ai` → FAIL.
- [ ] **Step 2: GREEN.** Add the commands (match the S3/S4 `orchd_*` shape), the broker consts+arms (exhaustive match — no wildcard), register in lib.rs. **Deps:** add `rmcp`/`reqwest`/`bpa-mcp`/`bpa-secrets` to nothing in src-tauri (egress is orchd-only) — core only speaks the wire. 
- [ ] **Step 3:** `cargo test -p builder-pro-ai` (stage sidecars first), clippy, fmt. Commit `feat(core): mcp_* commands + orchd://mcp-* events (S-EXT §5/§8)`.

### Task 8: frontend — ipc + store + «Расширения» view + Servers/Tools tabs

**Files:**
- Modify: `src/ipc/orchd.ts`, `src/ipc/events.ts`, `src/store/store.ts` (add `"ext"` to the `view` union + mcp slice), `src/components/WorkspaceSidebar.tsx` (nav button), `src/App.tsx` (`view==="ext"` branch)
- Create: `src/components/ext/ExtPanel.tsx`, `ServersTab.tsx`, `ToolsBrowser.tsx`, `ConnectDialog.tsx` + tests

**Interfaces — Consumes:** T7. **Produces:** typed wrappers `mcpAddServer/…/mcpCallTool/trustGrantConsent` (names/args match T7); `onOrchdMcpServersChanged`/`…ToolsChanged`/`…ArtifactsChanged`/`…InvocationLogged`; store `mcpServers`, `mcpToolsByServer`, `mcpArtifacts`, `refreshMcpServers()`, `refreshMcpTools(serverId)`, `refreshMcpArtifacts()`; App binds the change events (unconditional refresh, S3/S4 precedent); ExtPanel with tabs «Серверы»/«Инструменты» (+ placeholders for later tabs); ConnectDialog for first-connect consent; all mutating controls `disabled` while `orchdDown`; a tool result renders with an «непроверенные данные» untrusted banner.

- [ ] **Step 1: RED.** `ServersTab.test.tsx` (jsdom): renders server list from a stub store; add-server form → `mcpAddServer` called with camelCase args; connect on a consent-required server → ConnectDialog shown → grant → `trustGrantConsent`+`mcpConnect`; `orchdDown:true` disables add/connect (asserted not-called). `ToolsBrowser.test.tsx`: tool enable toggle → `mcpSetToolEnabled`; invoke form → `mcpCallTool`; result shows untrusted banner. store.test: `refreshMcpServers` replaces slice; `orchd://mcp-servers-changed` re-fetches. Run `npx vitest run` → FAIL.
- [ ] **Step 2: GREEN.** Implement wrappers/listeners/store slice/view wiring/components mirroring ProjectPanel tab conventions + honest-degradation. Extend `view` union in store.ts, add the sidebar button + App branch (the 3 named touch points).
- [ ] **Step 3:** `npx vitest run`, `npx tsc --noEmit`. Commit `feat(ui): «Расширения» view — MCP servers + tools browser + connect consent (S-EXT §8)`.

### Task 9: e2e stub-MCP phase + gate (Phase-1 DoD)

**Files:** Modify `tests/e2e/orchd-survive.mjs` (new phase + codec for the MCP verbs used); create `tests/e2e/lib/stub-mcp-server.mjs` (a minimal Streamable-HTTP MCP server the harness spawns).

**Interfaces — Consumes:** T6. **Produces:** a phase proving connect→list→call→durable-artifact-across-restart against the local stub.

- [ ] **Step 1.** `stub-mcp-server.mjs`: a tiny Node HTTP server implementing the MCP Streamable-HTTP subset the client uses (`initialize`→protocolVersion `2025-11-25`; `tools/list`→one `echo` tool; `tools/call echo`→echoes). Extend the harness codec (`encodeOrchdRequest`/`decodeOrchdResponse`/`decodeOrchdPush`) for `McpAddServer`/`TrustGrantConsent`/`McpConnect`/`McpListTools`/`McpCallTool`/`McpListArtifacts` + their responses/pushes (snake_case frame; float fields via the `cborFloat()` sentinel if any). New phase: spawn stub → `McpAddServer(http, url=stub)` → `TrustGrantConsent` → `McpConnect` (tools cached) → `McpListTools` (echo) → `McpCallTool echo` → assert `McpCallResult` + a persisted artifact → `OrchdShutdown{drain}` → relaunch → `McpListArtifacts` returns the artifact. Log `phaseN OK: mcp tool artifact survived restart`. `npm run e2e:orchd` → `ALL PHASES PASSED`.
- [ ] **Step 2.** `bash scripts/final-suite.sh` → `ALL GATES PASSED` (stage sidecars; orchd coverage ≥80% — add unit tests if short; retry-once on the BL-40 attach flake). Commit `test(e2e): mcp tool artifact survives restart (S-EXT phase-1 DoD) + gate green`.

**→ Phase 1 complete = first testable version** (the owner can add an MCP server + connect + invoke a tool + see a durable artifact, in-app).

---

## PHASE 2 — OAuth + connectors

### Task 10: `orchd-proto` — Phase-2 entities + verbs (append-only)
**Files:** `crates/orchd-proto/src/lib.rs`, regenerate `orchd-types.ts`.
**Produces:** entities `Account`, `ConnectorOp`, `OAuthChallenge{authorize_url,state}`; requests `ConnectorBeginOAuth`, `ConnectorCompleteOAuth`, `ConnectorAddApiKey`, `ConnectorListAccounts`, `ConnectorDeleteAccount`, `ConnectorListOps`, `ConnectorInvoke`; responses `Account`, `Accounts`, `OAuthChallenge`, `ConnectorOps`, (reuse `McpCallResult` for `ConnectorInvoke`); push `ConnectorsChanged`. Same append-only + entity-vs-frame layering.
- [ ] Steps: RED wire-tag + ts-export test → GREEN append + regen → temp wildcard dispatch arm for the P2 verbs → commit `feat(orchd-proto): S-EXT phase-2 connector wire (append-only)`.

### Task 11: `bpa-secrets` refresh helpers + `connectors/accounts.rs` OAuth
**Files:** `crates/orchd/src/connectors/mod.rs`, `accounts.rs`; `crates/orchd/src/persistence.rs` (account CRUD). Add `oauth2 = "5.0"` to orchd Cargo.toml.
**Produces:** `account` CRUD (§4); `accounts::begin_oauth(provider,label,scopes,redirect) -> OAuthChallenge` (PKCE via `PkceCodeChallenge::new_random_sha256`, `authorize_url` with `CsrfToken`); `accounts::complete_oauth(state, code) -> Account` (`exchange_code(code).set_pkce_verifier(v).request_async(&http)`, `http` built with `.redirect(reqwest::redirect::Policy::none())` — SSRF guard; tokens→Keychain via `bpa_secrets::account_ref`, ref→DB, expiry stored); `accounts::add_apikey(provider,label,api_key)->Account`; `accounts::token_for(account_id) -> AccountToken` (get from Keychain, refresh via `exchange_refresh_token` if expired).
- [ ] Steps: RED (in-memory Db + a fake token endpoint or a unit test of the PKCE URL build + the DB/keychain ref roundtrip; do NOT hit a real IdP) → GREEN → commit `feat(orchd): connector OAuth account layer (oauth2 PKCE, Keychain, refresh) (S-EXT §5, D5)`.

### Task 12: connector adapter trait + generic-rest + `ConnectorInvoke`
**Files:** `crates/orchd/src/connectors/adapter.rs`; `crates/orchd/src/mcp/invoke.rs` (extend the invoke/artifact path to be adapter-agnostic, or a shared `record_invocation` helper).
**Produces:** `trait ConnectorAdapter { fn provider()->&str; fn list_ops()->Vec<ConnectorOp>; async fn invoke(&self, tok:&AccountToken, op:&str, args:Value)->Result<Value,ConnectorError>; }`; a `GenericRestAdapter` (`provider="generic-rest"`, ops `get`/`post` against an account base_url with bearer, reqwest rustls); `connectors::invoke(db, account_id, op, args, project_id)` → `trust::authorize(ConnectorInvoke)` (same policy scope, `connector_invoke` audit, is_untrusted=1) → adapter.invoke → invocation + artifact (reuse the T5 record path).
- [ ] Steps: RED (fake ConnectorAdapter + in-memory Db: invoke writes invocation+artifact untrusted, trust-denied path audits, no artifact) → GREEN → commit `feat(orchd): connector adapter + generic-rest + ConnectorInvoke via trust (S-EXT §7, D5/D10)`.

### Task 13: dispatch P2 + core + accounts UI  [13a orchd/core, 13b frontend — parallel, disjoint files]
- **13a:** `socket_server.rs` P2 dispatch arms (+ `ConnectorsChanged` push); `src-tauri` `connector_*` commands + `orchd://connectors-changed` event. RED dispatch/broker tests → GREEN → commit `feat(orchd+core): connector dispatch + commands + event (S-EXT §5/§8)`.
- **13b:** `src/components/ext/ConnectorsTab.tsx` + tests (accounts list; "подключить OAuth" opens `authorize_url` in the browser via the existing open-url path, completes on redirect; "добавить API-ключ" masked; generic-rest ops runner); store connectors slice + `onOrchdConnectorsChanged`; orchdDown-disable. RED → GREEN → commit `feat(ui): Коннекторы tab — OAuth accounts + api-key + generic-rest runner (S-EXT §8)`.

### Task 14: e2e connector phase + gate
- [ ] Extend the harness: an api-key account (no real IdP) + a stub generic-rest endpoint the harness spawns → `ConnectorInvoke` → artifact persists → survives restart. `npm run e2e:orchd` green; `final-suite.sh` green. Commit `test(e2e): connector invoke artifact survives restart + gate`.

---

## PHASE 3 — stdio + skills + caps + hardening + docs

### Task 15: tokio `process` + stdio transport in `bpa-mcp`
**Files:** root `Cargo.toml` (`tokio` += `"process"`), `crates/mcp/src/transport.rs` (add `Stdio{command,args,env}`), tests.
**Produces:** `TransportConfig::Stdio{command,args,env}` → rmcp `transport-child-process` (`TokioChildProcess`); `bpa_mcp::connect` handles both variants.
- [ ] RED (stub stdio server binary in the test, or an rmcp child-process test) → GREEN → commit `feat(mcp): stdio (child-process) transport (S-EXT §3, D6)`.

### Task 16: shared DYLD/LD env denylist + orchd stdio-spawn + close BL-1 in sessiond
**Files:** create `crates/daemon-core/src/env_filter.rs` (or `bpa-paths`); modify `crates/sessiond/src/socket_server.rs:1219-1223` (apply the denylist to `env_overrides`); orchd stdio-spawn env path.
**Produces:** `pub fn strip_dangerous_env(pairs: &mut Vec<(String,String)>)` removing any key matching `DYLD_*`/`LD_*` (case-sensitive prefix); used by BOTH sessiond's `env_overrides` application and orchd's stdio spawn. Closes BL-1.
- [ ] RED: a sessiond test that `env_overrides` with `DYLD_INSERT_LIBRARIES` does NOT reach the child env (extend the existing env test); an orchd stdio-spawn test same. → GREEN → commit `fix(daemon-core): shared DYLD_*/LD_* env denylist for stdio spawn + sessiond env_overrides (closes BL-1) (S-EXT §6)`.

### Task 17: skills registry + proto + dispatch + SkillsTab  [parallel w/ T15/16 — disjoint]
**Files:** `crates/orchd/src/skills/registry.rs`, persistence skill CRUD, `orchd-proto` `Skill*` verbs (append-only) + entity, socket dispatch, `src-tauri` `skill_*` commands + `orchd://skills-changed`, `src/components/ext/SkillsTab.tsx`.
**Produces:** `skill` CRUD (§4; SKILL.md frontmatter parse for name/description; `md_hash` sha256; `bpa_paths::validate_path_within` on `md_path`; files-as-truth — external edit/delete surfaced like RuleSet); `SkillAdd/List/Delete` verbs; SkillsTab (list/add-pick-SKILL.md/remove + banner «Навыки исполняются, когда появится агент-оркестр (S6b)»).
- [ ] RED (registry: add parses frontmatter, hash stored, path-escape rejected; deleted-file surfaced) → GREEN → dispatch+core+UI (RED→GREEN) → commit `feat(orchd+ui): skills registry (SKILL.md, files-as-truth) + Навыки tab — plumbing (S-EXT §8, D11, Q14)`.

### Task 18: spend/rate caps + list_changed refresh + remaining UI tabs
**Files:** `crates/orchd/src/trust.rs` (policy enforcement), persistence `policy` CRUD, `orchd-proto` `TrustSetPolicy`/`TrustListAudit` + `Policy`/`AuditRow` entities, dispatch, core, `src/components/ext/{InvocationLog,ArtifactsTab}.tsx` + a policy editor + audit view; `crates/orchd/src/mcp/lifecycle.rs` (rmcp `on_tool_list_changed` → refresh cache + push `McpToolsChanged`).
**Produces:** spend-cap (sum `cost_usd` over window per scope) + rate-limit (N/min) enforced in `trust::authorize(ToolCall/ConnectorInvoke)`; `list_changed` handling; InvocationLog + Artifacts + audit UI tabs.
- [ ] RED (spend-cap breach → deny+audit; rate-limit → deny; list_changed → cache refreshed+push) → GREEN → UI (RED→GREEN) → commit `feat(orchd+ui): spend/rate policy caps + list_changed refresh + invocation/artifact/audit tabs (S-EXT §6/§8, BL-22)`.

### Task 19: docs truth + CHANGELOG [0.6.0] + backlog deltas + gate
**Files:** `docs/architecture.md` (egress + MCP + trust layer + new crates), overview roadmap §3 (S-EXT row → SHIPPED `[0.6.0]`, current-slice pointer moves, S-EXT unblocks S-IDEA/S6a-tools), `README.md` (features + measured test counts + MCP mention), `CHANGELOG.md` (`[0.6.0]`), `docs/traceability.md` (S-EXT rows), `docs/backlog.md` (per spec §11: close BL-1/20/22; re-target BL-27→S6b/SW2, BL-34→next daemon-upgrade; add sampling/resources-prompts/named-social-adapters/bulk-import rows), `docs/frontend-conventions.md` (ExtPanel + untrusted banner), `docs/runbook-daemon.md` (orchd egress/keychain notes).
- [ ] `git grep -niE 'todo|tbd' docs/architecture.md` empty; README counts re-measured; `bash scripts/final-suite.sh` green. Commit `docs: S-EXT shipped — MCP client + connectors + skills, CHANGELOG [0.6.0]`.

### Task 20: whole-branch review + merge + CI green
- [ ] `scripts/review-package MERGE_BASE HEAD` → final whole-branch review (most-capable model): contract consistency end-to-end (Keychain↔DB-ref↔wire↔UI), trust choke-point covers EVERY egress path (no un-gated connect/spawn/call/connector-invoke), no secret in logs (grep + the no-secrets tests), egress-only-in-bpa-mcp/connectors (sessiond untouched), migration additive+idempotent, rmcp/reqwest TLS pinned rustls, spec §§1–11 completeness table. Fix Critical/Important (one fix subagent, full list). → finishing-a-development-branch: ff-merge → main, push, **watch CI green** (stage sidecars; retry-once BL-40; no env-fragile timing asserts).

---

## Self-review notes (author)

- **Spec coverage:** §1 scope → P1-P3 tasks; §2 D1–D14 → T1(D4),T2(D12),T3(D13),T4(D2/D3),T5(D7/D9/D10),T6,T8, D5→T11-12, D6→T15, D11→T17, D14 phasing = the P1/P2/P3 split. §3 layout → T1/T4 crates, T2/T5 modules, T8 UI touch points. §4 DDL → T2 (all tables in one migration; invocation/artifact/consent/audit/policy CRUD land as their consumers arrive T5/T12/T18 but the TABLES all ship in T2's `Migration{upto:3}`). §5 verbs → T3/T10/T17/T18 (append-only) + T6/T13a dispatch. §6 trust → T5 (choke-point) + T16 (env denylist) + T18 (caps). §7 connectors → T12. §8 UI → T8/T13b/T17/T18. §9 tests → per-task TDD + T9/T14 e2e. §10 human steps → surfaced in T19 docs, not on the autonomous path. §11 backlog → T19.
- **No placeholders:** each task names exact files, produces/consumes signatures, and a concrete first failing test. The verbatim DDL/wire live in the spec (§4/§5) and are referenced (DRY) — implementers read the spec section named in the task.
- **Type consistency:** `McpCallResult` shared by `McpCallTool` (T3/T6) and `ConnectorInvoke` (T10/T12/T13a). `trust::authorize` `Action` enum extended additively (Connect/ToolCall in T5 → ConnectorInvoke in T12 → policy in T18). `is_untrusted=1` set in both invoke paths. Entity structs (camelCase+ts-rs) vs frame variants (snake_case) held consistently per the Global Constraints.
- **Parallel safety:** contracts-first tasks (T1–T5, T10–T12) are sequential; T8‖T9 (frontend vs e2e — disjoint), T13a‖T13b (orchd/core vs frontend — disjoint), T15/16‖T17 (mcp/daemon-core/sessiond vs skills/proto/ui — disjoint) are the only parallel groups; no two parallel tasks write the same file.
