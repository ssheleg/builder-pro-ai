# S-EXT — MCP client + Connectors + Skills — Design Spec

> Slice: **S-EXT** (roadmap §3). Host: **`bpa-orchd`**. First **application-driven egress + Keychain**
> surface in the product (`reqwest` is already latent in the build graph via `tauri`, but no
> BuilderProAI code performs outbound network I/O today; `security-framework`/Keychain is genuinely
> first-time). Terminal artifact of the brainstorming cycle; locks the contracts a zero-context
> implementer (and parallel subagents) build against. Date: 2026-07-15.
>
> **Pinned dependency contracts** (verified via Context7 against current crates.io/docs.rs):
> ```toml
> rmcp = { version = "2.2", features = ["client", "transport-child-process", "transport-streamable-http-client-reqwest", "auth"] }
> security-framework = "3.7"
> oauth2 = "5.0"                      # default (async reqwest) features only — do NOT add reqwest-blocking
> reqwest = { version = "0.13", default-features = false, features = ["rustls", "json", "stream"] }
> ```
> rmcp ships `reqwest` with `default-features=false` and NO TLS backend → the workspace MUST add
> `reqwest` with `rustls` explicitly (else no TLS / an accidental native-tls/OpenSSL pull → breaks the
> notarized build). `tokio` currently lacks the `"process"` feature — add it (Phase 3 stdio spawn).

## 1. Goal & scope

Give the app an **outbound extension layer**: connect to external MCP servers, discover and invoke
their tools with typed retries / timeouts / honest degradation, persist results as durable
artifacts, hold external OAuth accounts ("connectors") securely, and register portable skills —
all managed from a UI, all gated by a trust layer. This is the tool-provider substrate the future
agent org (S6b) and research pipeline (S-IDEA) consume.

**In scope (full S-EXT, user-chosen):**
- MCP server **registry** (add / enable / disable, global + per-project scope), persisted in `orchd.db`.
- Both transports: **Streamable HTTP** (remote servers, e.g. prowl.chat) and **stdio** (local process servers).
- **Auth**: static bearer / API-key AND OAuth 2.1 (discovery via Protected Resource Metadata, PKCE, refresh) for MCP servers; secrets in **macOS Keychain**, never SQLite/logs.
- **Connectors** = an **OAuth-account layer**: external accounts (claude.ai-style) whose tokens live in Keychain and are exposed to consumers. Two consumer shapes: (a) OAuth-authenticated MCP servers; (b) direct-API "social" connectors (a typed adapter trait; one reference adapter ships).
- **Tool discovery** (`tools/list`, paginated) + `notifications/tools/list_changed` handling → cache refresh + push.
- **Typed invoke** (`tools/call`) with per-server timeout, bounded retries (idempotency-aware), honest degradation.
- **Per-call cost / token / latency capture** from the first call (invocation records).
- **Durable artifacts**: a tool result persists as a `mcp_artifact` row surviving orchd restart (the DoD).
- **Skills**: SKILL.md-format registry + storage + management UI. ⚠️ **No runtime consumer yet** (the agent org that loads skills is S6b) — this slice ships the registry/storage/UI plumbing only; documented honestly, not presented as executable.
- **Trust layer (BL-22)**: per-server connect consent, stdio-exec consent, spend/rate caps, tool-result **untrusted-data tagging**, append-only audit log.
- **Management UI**: MCP servers, connectors/accounts, tools browser, invocation log, artifacts, skills.

**Non-goals / deferred (explicit):**
- MCP **sampling** (server→client LLM calls): DISABLED, capability not advertised (Q6). Backlog.
- MCP **resources** / **prompts**: not surfaced in v1 (Q6). Backlog (BL-new).
- **Active** prompt-injection *mediation* of tool results (rewriting/quarantine enforcement at an agent boundary): results are TAGGED untrusted + stored quarantined now; the mediation point is where an agent consumes them (S6b). This slice builds the tag + the boundary, not an agent.
- Agent runtime / workflow steps that *invoke* tools (S6b / SW1) — this slice exposes the invoke API; nothing here auto-invokes.
- Windows/Linux Keychain — macOS only (project is macOS-only).
- BL-27 (Keychain access while screen **locked** for unattended orchd runs): interactive v1 runs screen-unlocked; documented residual, not resolved here.

## 2. Locked decisions

| # | Decision |
|---|---|
| **D1** | **Host = `bpa-orchd`.** Egress lives in orchd (survives GUI close → S6b/SW2 unattended tool calls need no re-plumb; owns `orchd.db` for registry+artifacts). Never Hop-B (sessiond domain untouched). |
| **D2** | **Buy, don't build the protocol.** Use the official **`rmcp` = "2.2"** crate (client) for JSON-RPC, transports, `initialize` version negotiation, SSE, session-id, `tools/*`. Client API: `service.serve(transport) -> RunningService<RoleClient>`, `peer().list_tools()/list_all_tools()`, `peer().call_tool(CallToolRequestParams)`. `StreamableHttpClientTransport::from_uri(url)`; `StreamableHttpClientTransportConfig::auth_header(token)` — token is the **bare bearer WITHOUT the `Bearer ` prefix** (rmcp prepends). A thin `bpa-mcp` crate wraps rmcp behind a project-shaped, stub-testable interface; orchd domain code never imports rmcp types directly. |
| **D3** | **Protocol version**: advertise `protocolVersion` `"2025-11-25"` in `initialize` — matches rmcp `ProtocolVersion::LATEST` (`V_2025_11_25`); accept the server's negotiated version (rmcp handles). HTTP requests carry `MCP-Protocol-Version` (rmcp handles). Client capabilities: `{}` (no sampling, no roots-listChanged) in v1. `list_changed` handled via rmcp's `ClientHandler::on_tool_list_changed` hook. |
| **D4** | **Secrets in Keychain only** (`bpa-secrets`, `security-framework::passwords`). `orchd.db` stores a **secret ref** (service+account key, token kind, expiry, scopes) — never the token bytes. No secret in any log (extends the no-secrets-in-logs test discipline to the MCP/connector surface). |
| **D5** | **Connector = OAuth-account layer.** One `account` registry + one OAuth 2.1 authorization-code+PKCE flow driver (`oauth2 = "5.0"`, async reqwest client only). An MCP server may *reference* an account for its bearer; a direct-API social connector references an account and implements a typed `ConnectorAdapter`. Accounts are first-class, decoupled from both consumers. **SSRF guard**: the token-exchange HTTP client MUST set `.redirect(reqwest::redirect::Policy::none())` (oauth2-rs guidance) — DoD in `accounts.rs`. **For MCP-server OAuth specifically**, prefer reusing rmcp's own `transport::auth::{AuthorizationManager, AuthClient}` (feature `auth`) — it already implements SEP-985 Protected-Resource-Metadata discovery + WWW-Authenticate parsing + rmcp-2.0's resource-indicator (RFC 8707) anti-spoofing + metadata-SSRF blocking, so the MCP-server-OAuth consumer inherits protocol-correct security instead of re-deriving it; the hand-rolled `oauth2` driver backs the direct-API (non-MCP) connector accounts. If `accounts.rs` does drive MCP-server OAuth itself, RFC 8707 resource-indicator binding is a locked DoD item there. |
| **D6** | **Transports both**, but **Streamable HTTP is the DoD path** (prowl.chat is remote). stdio ships in Phase 3 behind an **execution-consent gate** (spawning a local process from a registry entry is code-exec; BL-22). |
| **D7** | **Retries/timeouts/degradation**: per-server config (`timeout_ms`, `max_retries`). `initialize`/`tools/list` are retriable (idempotent). `tools/call` retries ONLY when the server declares the tool idempotent-safe or the failure is a transport-level pre-dispatch error (never blind re-invoke of a possibly-side-effecting tool). Every terminal failure → typed error → honest UI state + audit row, never a silent swallow. |
| **D8** | **Cost/latency from call #1.** Every `tools/call` writes an `mcp_invocation` row: `{server_id, tool_name, request_hash, latency_ms, ok, error_kind, cost_usd?, input_tokens?, output_tokens?, started_at}`. Cost/token fields are `Option` (MCP tool results rarely carry token accounting; populated when the server reports usage, else null — honestly). |
| **D9** | **Artifacts durable + untrusted.** A successful `tools/call` result persists as `mcp_artifact` `{id, invocation_id, project_id?, server_id, tool_name, content_json, content_text?, is_untrusted:true, created_at}`. `is_untrusted` is always true for external tool output (the S6b mediation flag). Survives orchd restart (DoD e2e). |
| **D10** | **Trust layer is a single choke point** in orchd: every connect / stdio-spawn / tool-call passes `trust::authorize(action, policy) -> Decision` before dispatch; every decision + outcome writes an append-only `audit_log` row. Consent for first connect / first stdio-spawn is an owner gate (persisted grant; re-prompt on binary/URL change — mirrors the daemon build-change consent pattern). Spend/rate caps are policy rows enforced pre-dispatch. |
| **D11** | **Skills = plumbing only.** `skill` registry stores SKILL.md files (name, description, path/hash, scope) in the SKILL.md format for portability (Q14). No executor. UI lists/adds/removes; a banner states "skills run once the agent org ships (S6b)". Files-as-truth like RuleSet (D4 of S3): DB stores `md_path`+`md_hash`, external edits surface honestly. |
| **D12** | **Additive schema v3.** `orchd.db` `SCHEMA_VERSION` 2→3 via one additive `Migration{upto:3}` (whole-chain single-tx, forward-only, fail-closed — the established `bpa_daemon_core::migrate` contract). New tables only; no existing-table change. |
| **D13** | **orchd-proto append-only.** New `Mcp*` / `Connector* `/ `Skill*` request/response variants + `Mcp*Changed` pushes appended at the END of the frozen enums (same discipline as S4 graph verbs). orchd version space stays `[1,1]` (additive; no wire-breaking change). |
| **D14** | **Phasing (execution order).** Phase 1 = DoD-critical (registry + HTTP transport + bearer/API-key auth + connect + tools/list + per-tool allowlist + tools/call + artifact + invocation + trust choke-point + minimal UI + **stub-MCP e2e, prowl-shaped**). Phase 2 = OAuth 2.1 (rmcp `auth` for MCP-server OAuth; `oauth2` for connector accounts) + connector account layer + one reference direct-API adapter + accounts UI. Phase 3 = stdio transport + exec-consent + **fresh DYLD_*/LD_* env denylist (+ close BL-1 in sessiond same pass)** + skills registry+UI + spend/rate caps + list_changed + management-UI polish + full hardening. **Phase 1 completion = the first testable version.** The roadmap DoD line "prowl.chat connected" is fully closed only once the owner runs the §10 Human step (real creds) — CI proves the identical mechanism against a local stub. |

## 3. Architecture & module layout

```
crates/
  bpa-secrets/                 NEW — Keychain wrapper (security-framework::passwords)
    lib.rs                     set/get/delete generic password; SecretRef {service, account};
                               fixed service prefix "ai.builderpro.desktop"; NEVER logs the bytes
  bpa-mcp/                     NEW — thin wrapper over rmcp (protocol isolation, stub-testable)
    lib.rs                     re-exports the project-shaped API
    client.rs                  connect(TransportConfig, Option<Bearer>) -> McpSession
                               McpSession::{list_tools() -> Vec<McpTool>, call_tool(name, args)
                               -> McpToolResult, protocol_version(), close()}
    transport.rs               TransportConfig { Http{url}, Stdio{command, args, env} };
                               builds rmcp StreamableHttpClientTransport / child-process transport
    types.rs                   McpTool {name, title?, description?, input_schema:Json},
                               McpToolResult {content:Json, structured?:Json, is_error, usage?},
                               McpError {Transport, Protocol, Timeout, ToolError, Auth}
                               (maps rmcp types → project types; orchd never sees rmcp directly)
  bpa-orchd/
    src/mcp/
      registry.rs              mcp_server CRUD (add/list/enable/disable, global+project scope)
      lifecycle.rs             connect/disconnect; caches tools; handles list_changed → refresh+push
      invoke.rs                call_tool: trust-authorize → bpa_mcp::call_tool → invocation row
                               → artifact row; retry/timeout per D7; typed degradation
      cache.rs                 tool_cache read/write; staleness; list_changed invalidation
    src/connectors/
      accounts.rs              account registry CRUD; OAuth 2.1 auth-code+PKCE driver (oauth2 crate);
                               token refresh; token bytes → bpa-secrets, ref → DB
      adapter.rs               trait ConnectorAdapter { id, list_ops, invoke(op, args) };
                               registry of adapters; one reference adapter (see §7)
    src/skills/
      registry.rs              skill CRUD; SKILL.md parse (frontmatter name/description); md_hash;
                               files-as-truth (external edit/delete surfaced)
    src/trust.rs               authorize(Action, Policy)->Decision; ConsentStore (persisted grants);
                               PolicyStore (spend/rate caps); audit_log append; untrusted-tagging
    src/persistence.rs         SCHEMA_VERSION 2->3 + Migration{upto:3} (§4 DDL verbatim)
    src/socket_server.rs       dispatch arms for all new verbs (+ push fan-out)
  orchd-proto/src/lib.rs       append-only Mcp*/Connector*/Skill* verbs + responses + pushes; ts-rs
src-tauri/src/
  commands.rs                  mcp_*/connector_*/skill_* commands (proxy to orchd)
  broker.rs                    map new pushes → orchd://mcp-*, orchd://connector-*, orchd://skill-*
  lib.rs                       register commands
src/
  ipc/orchd.ts + events.ts     typed wrappers + listeners
  store/store.ts               mcp/connectors/skills slices + refresh + coarse-invalidation binds
                               + EXTEND the top-level `view` union (currently "home"|"workspace"|"project")
                               with "ext" (NB: view is a string-union + if/else, NOT a registry)
  components/WorkspaceSidebar.tsx  ADD «Расширения» nav button (nav buttons live inline here, no LeftRail comp)
  App.tsx                      ADD `view === "ext"` render branch (if/else chain, ~L413-422)
  components/ext/              ExtPanel (Расширения view): ServersTab, ToolsBrowser, ConnectorsTab,
                               InvocationLog, ArtifactsTab, SkillsTab; consent dialogs
```

**View-switch is not a registry** (code-truth): adding the «Расширения» top-level view touches exactly three existing files — `store.ts` (extend the `view` string-union + default), `WorkspaceSidebar.tsx` (the inline nav button), `App.tsx` (the `if/else` render branch) — named above so a subagent doesn't rediscover them by grep.

**Boundaries.** `bpa-mcp` depends on `rmcp` only; knows nothing of orchd/SQLite/Keychain — receives a `TransportConfig` + optional bearer, returns typed tools/results; tested against an in-process rmcp stub server. `bpa-secrets` is the ONLY Keychain caller. Egress exists ONLY in `bpa-mcp`/`connectors` (never Hop-B). The trust choke-point (`trust.rs`) is the single pre-dispatch gate for connect/spawn/call.

## 4. Data model — `orchd.db` schema v3 (additive)

`SCHEMA_VERSION` 2→3; one `Migration{upto:3}`, additive-only. DDL (verbatim contract):

```sql
-- MCP servers registry
CREATE TABLE mcp_server (
  id             TEXT PRIMARY KEY,             -- uuid v4
  name           TEXT NOT NULL,
  transport      TEXT NOT NULL,                -- 'http' | 'stdio'
  url            TEXT,                          -- http: endpoint (…/mcp); null for stdio
  command        TEXT,                          -- stdio: executable; null for http
  args_json      TEXT NOT NULL DEFAULT '[]',    -- stdio: JSON array of args
  env_json       TEXT NOT NULL DEFAULT '{}',    -- stdio: JSON object (allowlisted at spawn)
  scope          TEXT NOT NULL,                 -- 'global' | 'project'
  project_id     TEXT,                          -- non-null iff scope='project'; FK -> project(id) ON DELETE CASCADE
  auth_kind      TEXT NOT NULL DEFAULT 'none',  -- 'none' | 'bearer' | 'oauth'
  secret_ref     TEXT,                          -- Keychain account key for bearer; null otherwise
  account_id     TEXT,                          -- FK -> account(id) for oauth; null otherwise
  enabled        INTEGER NOT NULL DEFAULT 1,
  timeout_ms     INTEGER NOT NULL DEFAULT 30000,
  max_retries    INTEGER NOT NULL DEFAULT 2,
  protocol_version TEXT,                         -- last negotiated; null until first connect
  created_at     INTEGER NOT NULL,
  updated_at     INTEGER NOT NULL,
  FOREIGN KEY(project_id) REFERENCES project(id) ON DELETE CASCADE,
  CHECK ( (scope='project') = (project_id IS NOT NULL) ),
  CHECK ( transport IN ('http','stdio') ),
  CHECK ( (transport='http') = (url IS NOT NULL) )
);
CREATE INDEX mcp_server_by_project ON mcp_server(project_id);

-- Cached tool descriptors (refreshed on connect + tools/list_changed)
CREATE TABLE mcp_tool (
  id             TEXT PRIMARY KEY,             -- uuid v4
  server_id      TEXT NOT NULL,
  name           TEXT NOT NULL,
  title          TEXT,
  description    TEXT,
  input_schema_json TEXT NOT NULL DEFAULT '{}',
  enabled        INTEGER NOT NULL DEFAULT 1,   -- per-tool allowlist (S0/S1 §16: "enabled tools are an explicit per-server allowlist"); default on-fetch
  fetched_at     INTEGER NOT NULL,
  FOREIGN KEY(server_id) REFERENCES mcp_server(id) ON DELETE CASCADE,
  UNIQUE(server_id, name)
);
CREATE INDEX mcp_tool_by_server ON mcp_tool(server_id);

-- External OAuth accounts (connectors); token bytes in Keychain, only refs here
CREATE TABLE account (
  id             TEXT PRIMARY KEY,             -- uuid v4
  provider       TEXT NOT NULL,                -- e.g. 'prowl','x','linkedin','generic-oauth'
  label          TEXT NOT NULL,                -- owner-facing name
  auth_kind      TEXT NOT NULL,                -- 'oauth' | 'apikey'
  secret_ref     TEXT NOT NULL,                -- Keychain account key (token/apikey lives there)
  scopes_json    TEXT NOT NULL DEFAULT '[]',
  expires_at     INTEGER,                       -- access-token expiry epoch ms; null if none
  refresh_ref    TEXT,                          -- Keychain key for refresh token; null if none
  created_at     INTEGER NOT NULL,
  updated_at     INTEGER NOT NULL
);

-- Per-call invocation records (cost/latency from call #1)
CREATE TABLE mcp_invocation (
  id             TEXT PRIMARY KEY,             -- uuid v4
  server_id      TEXT NOT NULL,
  tool_name      TEXT NOT NULL,
  project_id     TEXT,                          -- context if called within a project
  request_hash   TEXT NOT NULL,                 -- sha256 of args (NOT the args themselves)
  ok             INTEGER NOT NULL,
  error_kind     TEXT,                          -- null on ok
  latency_ms     INTEGER NOT NULL,
  cost_usd       REAL,                          -- null unless server reports usage
  input_tokens   INTEGER,
  output_tokens  INTEGER,
  started_at     INTEGER NOT NULL,
  FOREIGN KEY(server_id) REFERENCES mcp_server(id) ON DELETE CASCADE
);
CREATE INDEX mcp_invocation_by_server ON mcp_invocation(server_id, started_at);

-- Durable artifacts (tool results); untrusted by construction
CREATE TABLE mcp_artifact (
  id             TEXT PRIMARY KEY,             -- uuid v4
  invocation_id  TEXT NOT NULL,
  server_id      TEXT NOT NULL,
  tool_name      TEXT NOT NULL,
  project_id     TEXT,
  content_json   TEXT NOT NULL,                 -- full structured result
  content_text   TEXT,                          -- flattened text for preview/search
  is_untrusted   INTEGER NOT NULL DEFAULT 1,    -- always 1 for external output (S6b mediation flag)
  created_at     INTEGER NOT NULL,
  FOREIGN KEY(invocation_id) REFERENCES mcp_invocation(id) ON DELETE CASCADE
);
CREATE INDEX mcp_artifact_by_project ON mcp_artifact(project_id, created_at);

-- Skills registry (SKILL.md format; files-as-truth)
CREATE TABLE skill (
  id             TEXT PRIMARY KEY,             -- uuid v4
  name           TEXT NOT NULL,
  description    TEXT NOT NULL,
  md_path        TEXT NOT NULL,                 -- absolute path to SKILL.md (validated within an allowed root)
  md_hash        TEXT NOT NULL,                 -- sha256 of file at register time
  scope          TEXT NOT NULL,                 -- 'global' | 'project'
  project_id     TEXT,
  created_at     INTEGER NOT NULL,
  updated_at     INTEGER NOT NULL,
  FOREIGN KEY(project_id) REFERENCES project(id) ON DELETE CASCADE,
  CHECK ( (scope='project') = (project_id IS NOT NULL) )
);

-- Trust: persisted consent grants + policy caps + append-only audit
CREATE TABLE consent_grant (
  id             TEXT PRIMARY KEY,
  kind           TEXT NOT NULL,                 -- 'connect' | 'stdio_exec'
  server_id      TEXT NOT NULL,
  fingerprint    TEXT NOT NULL,                 -- url (http) or command+hash (stdio) at grant time
  granted_at     INTEGER NOT NULL,
  FOREIGN KEY(server_id) REFERENCES mcp_server(id) ON DELETE CASCADE,
  UNIQUE(server_id, kind)
);
CREATE TABLE policy (
  id             TEXT PRIMARY KEY,
  scope          TEXT NOT NULL,                 -- 'global' | 'project' | 'server'
  ref_id         TEXT,                          -- project_id or server_id per scope; null for global
  spend_cap_usd  REAL,                          -- null = unlimited
  rate_per_min   INTEGER,                       -- null = unlimited
  created_at     INTEGER NOT NULL,
  updated_at     INTEGER NOT NULL
);
CREATE TABLE audit_log (
  id             TEXT PRIMARY KEY,
  at             INTEGER NOT NULL,
  action         TEXT NOT NULL,                 -- 'connect'|'disconnect'|'stdio_spawn'|'tool_call'|'connector_invoke'|'consent_grant'|'policy_deny'
  server_id      TEXT,
  tool_name      TEXT,
  project_id     TEXT,
  decision       TEXT NOT NULL,                 -- 'allow'|'deny'
  reason         TEXT,                          -- e.g. 'spend_cap_exceeded'; NEVER secret/arg content
  invocation_id  TEXT
);
CREATE INDEX audit_log_by_at ON audit_log(at);
```

Idempotent-migration note: additive tables only; a v2→v3 upgrade of an existing `orchd.db` with live projects creates the tables and seeds nothing (no backfill needed — new subsystem).

## 5. Wire protocol — `orchd-proto` (append-only)

**Two layers, matching the existing code (code-truth):**
- The **frame enums** `OrchdRequest` / `OrchdResponse` / `OrchdPush` are Hop-B wire-only (core ⇄ orchd), derive only `Debug, Clone, Serialize, Deserialize, PartialEq`, use **plain snake_case Rust field names**, and are **NOT** ts-rs-exported (exactly like today's `GraphAddNode`/`GraphMoveNode`). New variants are appended at END.
- The new **entity structs** returned inside responses — `McpServer`, `McpTool`, `Account`, `McpInvocation`, `McpArtifact`, `Skill`, `ConnectorOp`, `AuditRow`, `Policy`, `OAuthChallenge`, `McpConnectReport`, `McpCallResult` — DO carry `#[serde(rename_all="camelCase")]` + `#[derive(... ts_rs::TS)]` + `#[ts(export_to="orchd-types.ts")]`, with i64 timestamps as `#[ts(type="number")]` — mirroring the `GraphNode`/`GraphEdge` entity derive block byte-for-byte.

New `OrchdRequest` variants (appended at END; snake_case Rust fields on the wire):

```
// MCP servers
McpAddServer { name, transport, url?, command?, args?, env?, scope, project_id?, auth_kind, timeout_ms?, max_retries? } -> McpServer
McpListServers { project_id? } -> McpServers(Vec<McpServer>)         // global + (project's if given)
McpUpdateServer { id, <editable fields> } -> McpServer
McpSetServerEnabled { id, enabled } -> McpServer
McpDeleteServer { id } -> Ack
McpSetServerBearer { id, token }  -> Ack                              // token -> Keychain, ref -> DB; token NEVER logged/echoed
McpConnect { id } -> McpConnectReport { protocol_version, tool_count }   // trust-gated; caches tools; push McpToolsChanged
McpDisconnect { id } -> Ack
McpListTools { server_id } -> McpTools(Vec<McpTool>)                  // from cache
McpSetToolEnabled { tool_id, enabled } -> McpTool                     // per-tool allowlist toggle (S0/S1 §16)
McpCallTool { server_id, tool_name, args_json, project_id? } -> McpCallResult { artifact_id, invocation_id, content_json, is_error }
       // rejects a disabled tool with Error{Policy} BEFORE dispatch (allowlist enforced in invoke.rs)
McpListInvocations { server_id?, project_id?, limit? } -> McpInvocations(Vec<McpInvocation>)
McpListArtifacts { project_id?, server_id?, limit? } -> McpArtifacts(Vec<McpArtifact>)
McpGetArtifact { id } -> McpArtifact
// Connectors / accounts
ConnectorBeginOAuth { provider, label, scopes?, server_id? } -> OAuthChallenge { authorize_url, state }  // opens browser; PKCE
ConnectorCompleteOAuth { state, code } -> Account                     // exchanges code; tokens -> Keychain
ConnectorAddApiKey { provider, label, api_key } -> Account            // apikey -> Keychain
ConnectorListAccounts {} -> Accounts(Vec<Account>)
ConnectorDeleteAccount { id } -> Ack
ConnectorListOps { account_id } -> ConnectorOps(Vec<ConnectorOp>)     // direct-API adapter ops
ConnectorInvoke { account_id, op, args_json, project_id? } -> McpCallResult   // reuses artifact/invocation path
// Skills
SkillAdd { name?, description?, md_path, scope, project_id? } -> Skill   // parses SKILL.md frontmatter if name/desc omitted
SkillList { project_id? } -> Skills(Vec<Skill>)
SkillDelete { id } -> Ack
// Trust / policy
TrustGrantConsent { server_id, kind } -> Ack
TrustSetPolicy { scope, ref_id?, spend_cap_usd?, rate_per_min? } -> Policy
TrustListAudit { limit? } -> AuditRows(Vec<AuditRow>)
```

Pushes (coarse invalidation; camelCase): `McpServersChanged { project_id? }`, `McpToolsChanged { server_id }`, `McpArtifactsChanged { project_id? }`, `ConnectorsChanged`, `SkillsChanged { project_id? }`, `McpInvocationLogged { server_id }`. Frontend events: `orchd://mcp-servers-changed`, `orchd://mcp-tools-changed`, `orchd://mcp-artifacts-changed`, `orchd://connectors-changed`, `orchd://skills-changed`, `orchd://mcp-invocation-logged`.

## 6. Trust layer (BL-22)

Single choke-point `trust::authorize(action, ctx) -> Decision`:
- **connect**: first connect to a server requires an owner **consent grant** (persisted in `consent_grant`, keyed by `(server_id, kind='connect')`, fingerprint = URL). Re-prompt if the URL changes (fingerprint mismatch) — mirrors the daemon build-change consent pattern.
- **stdio_spawn**: spawning a stdio server's process requires a distinct `stdio_exec` consent (fingerprint = command + sha256 of the resolved binary). Re-prompt on binary change. stdio env is filtered at spawn by a **fresh `DYLD_*`/`LD_*` denylist implemented here** — code-truth: no such denylist exists yet anywhere (sessiond applies `env_overrides` UNFILTERED at `socket_server.rs:1219-1223`; the denylist is still-open **BL-1**). Because a second unfiltered spawn path must not exist, **close BL-1 in sessiond in the same pass** (one shared denylist helper, ideally in `bpa-daemon-core` or `bpa-paths`, used by both sessiond's `env_overrides` and orchd's stdio spawn). Base env stays the minimal-safe allowlist (sessiond §9.3 pattern).
- **tool_call**: (1) reject a **disabled tool** (per-tool allowlist, `mcp_tool.enabled=0`) with `Error{Policy}` before dispatch; (2) enforce `policy` spend/rate caps pre-dispatch — spend cap: sum `cost_usd` over the window for the scope, deny with `policy_deny` audit row + typed error if the next call would breach (cost estimate = 0 when unknown, so caps bind only when the server reports cost — honest); rate: N calls/min per scope.
- **connector_invoke**: `ConnectorInvoke` (direct-API adapter) passes through `trust::authorize` **identically to `McpCallTool`** — same policy scope (spend/rate caps), a `connector_invoke` audit action, and `is_untrusted=1` on its artifact. Direct-API adapters are third-party egress and must not bypass the choke-point.
- **untrusted tagging**: every `mcp_artifact` (from `McpCallTool` AND `ConnectorInvoke`) is `is_untrusted=1`. This is the flag an S6b agent boundary will read; this slice only sets it + stores results quarantined (not auto-fed anywhere).
- **audit**: every connect / spawn / call / connector_invoke / consent / deny appends an `audit_log` row `{action, decision, reason}`. `reason`/rows NEVER contain secrets or tool args (only `request_hash`).

## 7. Connectors (direct-API adapter)

`trait ConnectorAdapter { fn provider(&self) -> &str; fn list_ops(&self) -> Vec<ConnectorOp>; async fn invoke(&self, account: &AccountToken, op: &str, args: Json) -> Result<Json, ConnectorError>; }`. One **reference adapter** ships to prove the seam end-to-end without over-committing to a specific network's churn: a **generic REST connector** (`provider="generic-rest"`) — ops `get`/`post` against an account-scoped base URL with the account's bearer, JSON in/out, same retry/timeout/artifact/audit path as MCP calls. (A named social adapter — X/LinkedIn — is a follow-up backlog item; the generic-rest adapter + the account layer prove the contract and let the owner wire a real API immediately.)

## 8. Frontend — management UI

New left-rail entry «Расширения» → `ExtPanel` with tabs (mirrors ProjectPanel's tab pattern + honest-degradation discipline: all mutating controls disabled while `orchdDown`):
- **Серверы**: list (name, transport, scope, enabled, status dot); add-server form (transport picker → http url / stdio command+args); enable/disable; connect (→ consent dialog on first) / disconnect; set-bearer (masked input, never echoed back). Per-server: negotiated protocol version, tool count, last error.
- **Инструменты**: tools browser across enabled+connected servers; per tool: name/desc/input schema + an **enable/disable toggle** (`McpSetToolEnabled` — the per-tool allowlist; a disabled tool cannot be invoked); a "вызвать" form (JSON args, disabled for a disabled tool) → invoke → result panel (marked «непроверенные данные» / untrusted). Every mutating call disabled while `orchdDown`.
- **Коннекторы**: accounts list; "подключить OAuth" (opens browser to `authorize_url`, completes on redirect) / "добавить API-ключ" (masked); delete; per generic-rest account, an ops runner.
- **Журнал**: invocation log (server, tool, ok/err, latency, cost) + audit log (action, decision, reason).
- **Артефакты**: durable artifacts list + viewer (content, untrusted banner, source server/tool, project).
- **Навыки**: skills list (name, desc, path, scope) + add (pick SKILL.md) + remove; banner: «Навыки исполняются, когда появится агент-оркестр (S6b) — сейчас это реестр».

## 9. Testing strategy & DoD

- **`bpa-secrets`**: Keychain roundtrip (set/get/update/delete) against a test service prefix; delete-after; assert value never appears in a captured log (no-secrets discipline). Hermetic (unique per-test service+account keys; cleaned in teardown).
- **`bpa-mcp`**: connect to an **in-process rmcp stub server** (rmcp `server` feature in dev-deps) over a `tokio::io::duplex()` in-memory transport pair (rmcp transport is generic over any `AsyncRead+AsyncWrite` — the supported in-memory analogue of rmcp's TCP-stream example); `list_tools` returns the stub's tools; `call_tool` echoes; timeout + transport-error → typed `McpError`. No network.
- **orchd unit**: registry CRUD (scope invariants, FK cascade); tool-cache write/invalidate on list_changed; per-tool allowlist — `McpSetToolEnabled` toggles `mcp_tool.enabled`, and `McpCallTool` on a disabled tool → `Error{Policy}` + no dispatch + audit; invoke path writes invocation + artifact (is_untrusted=1); `ConnectorInvoke` routes through `trust::authorize` (spend/rate cap + `connector_invoke` audit + is_untrusted=1) identically to `McpCallTool`; trust `authorize` — connect without grant → denied+audit; spend-cap breach → deny+audit; policy rate-limit; retry policy (idempotent retried, side-effecting not); DYLD/LD env denylist strips `DYLD_*`/`LD_*` at stdio spawn (and the shared helper strips them in sessiond's `env_overrides` — BL-1 regression test); no-secrets-in-logs for MCP/connector surface (planted bearer/token never in logs).
- **orchd socket**: dispatch each verb; `McpCallTool` → `McpCallResult` + `McpArtifactsChanged` push; `McpConnect` first-time without consent → `Error{Consent}` (or the consent-required contract) + audit.
- **core**: each `mcp_*/connector_*/skill_*` command proxies + error-maps; broker maps each push → the right `orchd://…` event (camelCase payload).
- **frontend**: ipc wrapper name/arg parity; store slices refresh + coarse-invalidation binds; ExtPanel tabs render (jsdom); invoke form fires the wrapper; **all mutating controls disabled while `orchdDown`** (asserted, not-called); untrusted banner renders on a tool result / artifact.
- **e2e (the DoD)** — a new phase in the orchd e2e harness against a **local stub MCP server** the harness spawns (deterministic, no external prowl dependency in CI): register an HTTP MCP server → `McpConnect` (tools cached) → `McpListTools` (stub's tools) → `McpCallTool` → assert `McpCallResult` + a persisted `mcp_artifact` → `OrchdShutdown{drain}` → relaunch → `McpListArtifacts` returns the artifact (**durable across restart**). Log `phaseN OK: mcp tool artifact survived restart`.
- **gate**: the 9-stage `final-suite.sh` green; orchd coverage ≥80% (add tests to hold it); ts-rs parity covers the new proto types. Known BL-40 attach flake → retry-once. CI green (mind the release-vs-debug wall-clock lesson from S4 — no env-fragile timing asserts).

**DoD (spec §8 roadmap row):** an MCP server connects; its tools are listed; one tool is invoked; the result persists as a durable artifact surviving orchd restart (proven by the e2e phase against the stub; the same path connects to real prowl.chat interactively — a Human step for real creds). Phase-1 completion = the first version the owner can test in-app.

## 10. Human steps (residual — end, non-blocking to the autonomous path)

- **Real prowl.chat account + token**: connecting to the *real* prowl server needs the owner's account/API-key (a credential the agent must not create/enter). The autonomous path proves the whole mechanism against a local stub; wiring real prowl is: owner adds the server + pastes their key in the «Коннекторы»/«Серверы» UI. One block, at the very end.
- **Notarized signed build** (unchanged from prior slices): the reqwest/rustls egress + Keychain entitlement may require a `keychain-access-groups` entitlement and a hardened-runtime review; the notarized release is credential-gated (Apple ID). Documented; not on the test path (dev build tests fine).
- **BL-27** (Keychain while screen locked, unattended orchd): resolve before the first *unattended* MCP call (S6b). Interactive v1 unaffected.

## 11. Backlog deltas (filed by the docs task)

- **New backlog rows**: MCP sampling (deferred, Q6); MCP resources/prompts surface (deferred, Q6); named social direct-API adapters (X/LinkedIn) beyond the generic-rest reference; active tool-result prompt-injection mediation at the agent boundary (owner S6b); bulk MCP-server import (config file).
- **BL-1 (env `DYLD_*`/`LD_*` denylist)** — **CLOSED in this slice** (§6): a shared denylist helper lands for both sessiond's `env_overrides` and orchd's stdio spawn. Mark BL-1 done in the docs task.
- **BL-27 (Keychain while screen locked, unattended orchd)** — re-target owner slice **S-EXT → S6b/SW2** (first *unattended* MCP call); interactive v1 is unaffected (§1, §10). Update the backlog row's owner slice.
- **BL-34 (daemon build-string comparison / restart-to-update)** — NOT an MCP feature; re-target owner slice **S-EXT → next daemon-upgrade cycle** (applies to both `bring_up_orchd` and `bring_up_daemon`; unrelated to this slice's egress work). Update the backlog row's owner slice with that rationale rather than silently carrying it here.
- **BL-20 (Keychain for keys)** — satisfied by `bpa-secrets` + D4; mark done/covered.
- **BL-22 (MCP hardening)** — satisfied by §6 trust layer (connect/exec consent, spend caps, untrusted tagging, audit); mark done/covered.
