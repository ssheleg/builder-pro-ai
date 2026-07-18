# Changelog

All notable changes to Builder Pro AI. Format: keepachangelog.com; versioning: semver.

## [0.9.1] — 2026-07-19

**S-DIAG + S-DESIGN — reconstructable error logs and a WCAG-AA contrast pass.** A follow-up to the
0.9.0 redesign: a final design review (measured contrast) plus a diagnostics layer so failure causes
survive the toast. Frontend-only, no wire/schema change; 925-test suite green.

### Added
- **Structured, reconstructable error log (S-DIAG).** Errors were surfaced only as a 4 s toast and
  then lost; a React render crash was an unrecoverable white screen. Now:
  - `src/ipc/diag.ts` — a secret-scrubbed (`Bearer`/token/key/app-password/home-dir redaction),
    bounded (200) diagnostics ring with machine error classification.
  - `store.reportError(op, e)` records a structured event (op, kind, message, scrubbed detail) +
    a `console.error` breadcrumb + the toast; every `refresh*` failure path routes through it.
  - An **`ErrorBoundary`** around the app catches render crashes, records them, and shows a recovery
    card instead of a white screen.
  - A **Diagnostics panel** (sidebar footer, with an error-count badge) lists the log newest-first
    with "Copy support bundle" (scrubbed JSON) and "Clear".

### Fixed
- **WCAG AA contrast across the palette (S-DESIGN).** The design review measured that in the light
  theme every colored Badge failed AA (each semantic tone 3.09–4.00 as text on its `-weak` bg), and
  in dark the white primary-button label was 3.26 on `--accent`. Darkened the five light foreground
  tones to clear 4.5:1 on their tightest pairing, and added a theme-aware `--on-accent` token for
  button labels. `src/ui/contrast.ts` + a `tokens.css`-parsing test now assert AA for every text
  pair in both themes as a permanent regression guard.

### Changed
- The primary-button label color is the `--on-accent` token (was a literal `#fff`) — the last
  hardcoded color in the component tree is gone.

## [0.9.0] — 2026-07-18

**S-UXR — UX-scenario base + testing loop + "Calm Control Room" UI redesign.** A frontend + QA
slice: a maintained catalog of every first-session UX scenario, a code-traced audit of all of
them, and a full restyle of the webview onto a design-token system so the app is metrics-forward
and works in light **and** dark. **No wire version bump** — `bpa-orchd` stays `[1,1]`; **no schema
migration** — this slice adds no verb and no persisted-shape change. The 886-test suite is the
behavior-preservation guarantee. See `docs/superpowers/plans/2026-07-18-s-uxr.md`.

### Added
- **Design-token system (`src/ui/tokens.css`):** a full light/dark palette (neutral slate + one
  calm-blue accent + `ok`/`warn`/`danger`/`info` semantics), plus space/radius/type/shadow scales,
  driven by `data-theme` on the root. Every colour/space/type value in the UI is now a `var(--…)`.
- **Theme system + toggle (`src/ui/theme.ts`, `ThemeToggle`):** `light`/`dark`/`system` with
  OS-preference follow, `localStorage` persistence, and FOUC-free boot apply; `statusTone()` maps
  entity status → tone. A toggle in the sidebar footer cycles the three modes.
- **Primitives kit (`src/ui/primitives.tsx`):** token-only building blocks — `Panel`, `Stat`
  (mono tabular-nums metric tile), `Sparkline` (inline SVG), `Badge`, `Button`, `Field`/`Input`/
  `TextArea`/`Select`, `EmptyState`, `Dialog`. No external UI/icon/chart dependency.
- **UX-scenario base (`docs/qa/ux-scenarios.md`):** 181 English scenarios across 15 epics
  (onboarding → every feature/button/state/error/result), a 7-column format, plus a maintenance
  **rule** (CONTRIBUTING) and an **advisory CI gate** (`scripts/check-ux-scenarios.sh`) that warns
  when UI changes land without a catalog update.
- **UX audit results (`docs/qa/ux-test-results.md`):** every scenario traced against code
  (handler → IPC verb → store slice → UI). Pass-1 verdict: **169 OK / 10 UX-gaps / 0 bugs / 2
  stale-doc** over 181 scenarios.

### Changed
- **Every view restyled onto tokens + primitives**, behavior-preserving: app shell + sidebar +
  banners, Home (whole-store `Stat` tiles), workspace/terminal/files, project panel (goals/tasks/
  rules), ideas/research/insights, the knowledge graph, and the extensions surface. The result is
  calmer, denser, metrics-forward, and correctly themed in both light and dark.
- **Focus ring is now theme-aware and single-source** in `tokens.css` (`var(--accent)`, offset 2px
  per design-system §6.8) with its `:focus:not(:focus-visible)` companion.

### Fixed
- **FI-02 (Important):** a failed directory load in the file tree now renders a distinct
  `file-row-failed` row with a working inline **Retry**, instead of a permanently stuck "Loading…"
  row.
- **TE-04 (a11y):** terminal tabs activate from the keyboard (Enter/Space), matching click.
- **TA-06:** a task's provenance (`source`) now renders as a Badge on the row.
- **ID-05:** the "+ idea" and orphan link-to-project buttons now *look* disabled (not just behave
  disabled) while the orchestrator is down.
- **GR-02 / GR-12:** a rejected optimistic graph edge (self-loop / duplicate / failure) is rolled
  back instead of lingering until the next reconcile push.
- **ON-07:** the Home "goals loading" line clears after a *failed* fetch instead of persisting.

### Removed
- **Legacy static dark-only palette (`src/theme.ts`) and `src/index.css`** — fully superseded by
  the token system; no remaining consumer.

### Docs
- `ux-scenarios.md` re-synced to code; the stale **EX-04 / RE-12** rows (which still branded the
  already-fixed MCP-connect timeout, BL-89, a Tier-0 Critical) corrected to "timeout-bounded".

## [0.8.0] — 2026-07-17

**S-POLISH — reliability, honesty, English-only, and Tier-2 feature completeness (P1–P4).** A
consolidation slice: backend reliability + observability (P1), a full English-only sweep with a
CI-enforced no-Cyrillic gate (P2), frontend reliability across every mutating/read surface (P3),
and four Tier-2 feature gaps closed (P4). **No wire version bump** — `bpa-orchd` stays `[1,1]`; the
four net-new verbs (`GetStorageStatus`, `UnarchiveProject`, `GraphUpdateEdge`,
`ConnectorListProviders`) are all appended at the enum TAIL (append-only), and no schema migration
lands. Closes BL-89..97 and objectives O-2..O-7 (plus BL-53, the un-archive verb). See
`docs/superpowers/plans/2026-07-16-s-polish.md`.

### Added
- **Storage-degradation mode on the wire + honest banner (BL-94, spec D3):** `bpa-orchd` already
  degraded honestly to an in-memory DB on an unusable disk and quarantined a corrupt on-disk image
  aside, but the resulting mode was invisible to the GUI (which kept implying data was durable). P1
  plumbs the boot fact to the frontend: `Db::open_with_outcome(path) -> Result<(Db, DbOpenOutcome)>`
  (`Clean` / `RecoveredFromCorruption{quarantined_to}`; the existing `Db::open` delegates +
  discards); `boot::open_db_degrading` maps the outcome to a `StorageStatus { storage_mode,
  quarantined_path }` (`StorageMode` = `persistent` / `recovered_from_corruption` /
  `in_memory_fallback`) stored in `ServerDeps` at boot; an append-only `GetStorageStatus ->
  StorageStatus` wire verb returns it verbatim (a pure read, no push — the mode is a boot fact only
  a restart changes); the `orchd_storage_status` Tauri command exposes it. P3 consumes it in a
  `StorageBanner` (`src/components/StorageBanner.tsx`) — a persistent red-accent `role="alert"` for
  `in_memory_fallback` ("changes will NOT survive a restart") and `recovered_from_corruption` (names
  the quarantined path), pulled on connect + every reconnect; `persistent` shows nothing. Copy in
  `strings.storage.*`; ops detail in `docs/runbook-orchd.md` ("Storage-degradation modes").
- **Structured per-request completion tracing (O-6, spec D4):** each dispatch layer wraps its
  dispatch call ONCE and emits a single completion line carrying only a low-cardinality quartet —
  `verb` / `outcome` (`ok`/`err`) / `error_code` (error lines only) / `elapsed_ms` — with no
  per-verb handler edits. Added in `bpa-orchd`'s `socket_server.rs` dispatch wrapper (over an
  exhaustive, wildcard-free `OrchdRequest::verb_name()` — a new verb fails to compile until named),
  the Tauri core's `orchd_client::request`, and `bpa-sessiond`'s dispatch wrapper, so a request is
  followed end-to-end (core → daemon) by the same field names. NO args, bodies, tokens, tool
  output, ids, or PII are ever logged — enforced by the extended `no_secrets_in_logs*` tests.
  Documented in `docs/runbook-orchd.md` ("Per-request tracing fields").
- **English-only UI + docs + a CI-enforced no-Cyrillic gate (O-2, spec D1/D2):** every webview
  string routed through `src/strings.ts` (English), Rust-side user-facing strings/templates
  translated in place, and the living docs translated. A new gate `scripts/check-english.sh` fails
  the build on any Cyrillic (`U+0400..U+04FF`) outside a CLOSED `scripts/english-allowlist.txt` of
  frozen historical records (superseded specs/plans/qa/research), wired as stage 1 of
  `scripts/final-suite.sh` and into CI. `CONTRIBUTING.md` gains the standing English-only rule; a
  reset-DB runbook step is documented. The app is now English.
- **Double-submit guard on every mutating submit (`useSubmitGuard`, BL-95/spec D6):** a shared hook
  (`src/hooks/useSubmitGuard.ts`) whose `guard(handler)` runs at most one invocation at a time (a
  synchronous `useRef` lock, not the batched `submitting` state) and whose `submitting` flag drives
  `disabled={… || submitting}`. Wrapped around every create/connect/run/set control (QuickCapture,
  CreateProjectDialog, ResearchRunDialog, FormInsightDialog, SpawnProjectFromIdea, the
  Ideas/Tasks/Goals create-forms, the Servers/Connectors/Skills add-forms, the log's set-policy),
  killing the duplicate-row / duplicate-external-call / duplicate-spend double-fire (P-19,
  E-08/F-08/G-08/H-01/J-03..05).
- **Toast FIFO queue with manual dismiss (BL-97/spec D8):** `showToast` now APPENDS to a capped-at-5
  queue instead of clobbering a single slot, so a burst of failures is shown in turn; the visible
  toast auto-advances after 4s and can be closed early via a new close button (`dismissToast`), with
  the auto-advance timer re-armed per head so a stale timer never clears a later toast.
- **Tier-3 UX polish (empty-states, edit-revert, error signals, consent-recovery):** empty ≠
  loading ≠ failed across read surfaces — `WorkspaceSidebar` gains a dim zero-projects/zero-
  workspaces empty-state (P-11), `FileTree` an "empty folder" marker for a loaded-but-empty expanded
  directory (P-12), `CommandStrip` a distinct loading placeholder plus a retry-on-failure instead of
  rendering `null` forever (P-13), and `ConnectorsTab`'s ops list a "load failed" state with retry,
  distinct from a genuinely empty op catalog (P-15). `IdeasList` row title/body edits now REVERT to
  the server value on a rejected save (mirrors `GoalTree`, P-27); `ToolsBrowser`'s enable toggle
  shows an on-row error on failure (not just a toast, J-01); and a `Consent`-kind denial in
  `ToolsBrowser`/`ConnectorsTab` appends a recovery hint pointing to Extensions → Servers → Connect
  (P-20).
- **Project un-archive (O-3, closes BL-53):** S3 shipped `ArchiveProject` with no inverse — an
  archived project was a one-way trap. New append-only verb `UnarchiveProject { id } -> Project`
  (`Db::unarchive_project`) flips `archived → active` and pushes `ProjectsChanged`; guards mirror
  `ArchiveProject` (unknown `id` ⇒ `NotFound`; already-`active` ⇒ `Invariant`). Exposed as the
  `orchd_unarchive_project` command. Frontend: a collapsed dimmed «Archived (N)» sidebar group + a
  read-only archived-project banner with an «Un-archive» button.
- **Graph editor — edge-kind editing + node form + inline rename (O-7):** new append-only verb
  `GraphUpdateEdge { id, kind } -> GraphEdge` (`crates/orchd/src/graph.rs`) changes an existing
  edge's `kind` (its rendered label IS its kind — no separate label column, **no v5 migration**),
  pushing `GraphChanged` for BOTH endpoint projects; guards mirror `GraphAddEdge` (unknown ⇒
  `NotFound`, archived endpoint ⇒ `Invariant`). Exposed as `orchd_graph_update_edge`. The
  `GraphCanvas` toolbar gains an add-node title/body form and a double-click inline rename (both
  reusing the shipped `GraphAddNode`/`GraphUpdateNode` verbs) plus an edge-kind dropdown; only LOCAL
  nodes/edges are editable, ghosts stay read-only.
- **Config-backed OAuth provider registry + provider dropdown (O-5):** before P4, `bpa-orchd` booted
  with an empty OAuth provider registry, so every `ConnectorBeginOAuth` failed with
  `UnknownProvider` and the BL-91 timeout sat on an unreachable path. At boot,
  `connectors/registry_config.rs::load_oauth_providers` reads an OPTIONAL
  `<app-support>/oauth_providers.json` and registers each provider into `ConnectorsState`'s
  in-memory registry (activating the P1 timeout on a now-reachable path). Honest degradation: a
  MISSING file is the normal default (empty registry, info log), a MALFORMED file (bad JSON, missing
  required field, or a typo'd key — `deny_unknown_fields`) is error-logged and leaves the registry
  empty — NEITHER blocks boot. New append-only read verb `ConnectorListProviders ->
  ConnectorProviders` returns provider NAMES ONLY (a provider's `client_id`/`client_secret`/URLs
  never cross the wire; `client_secret` lives only in memory, never in `orchd.db` or a log). Exposed
  as `orchd_connector_list_providers`; `ConnectorsTab`'s OAuth flow gains a provider `<select>` + an
  honest "No OAuth providers configured" empty-state (begin button disabled). Format + degradation
  table in `docs/runbook-orchd.md` ("OAuth provider registry — `oauth_providers.json`").
- **`metric_refs` owner chip editor on goals (O-4) — pure frontend, no new wire:** the
  `Goal.metric_refs` field and the `UpdateGoal` verb that carries it both already shipped in S3; P4
  adds the missing owner-facing editor — a chip list + add input on each `GoalTree` row, persisting
  the row's full next array through the existing `orchd_update_goal` command (no schema change, no
  new verb, no backend change).

### Changed
- **Gate: 9 stages → 10.** `scripts/final-suite.sh` adds the English-only gate
  (`scripts/check-english.sh`) as stage 1, ahead of the Rust suite; `.github/workflows/ci.yml` runs
  the same set. All other stages (clippy, fmt, TS, tsc, ts-rs parity, both-daemon coverage ≥80%,
  e2e:survive, e2e:orchd) are unchanged.
- **The app is English-only, as a standing rule (O-2).** Non-English copy in code/docs/UI is a build
  failure outside the frozen-record allowlist; `CONTRIBUTING.md` records the rule.
- Test totals re-measured this pass: Rust workspace 1023 → **1062 tests** (0 failed,
  `RUST_TEST_THREADS=4`; the new un-archive / `GraphUpdateEdge` / OAuth-registry unit + dispatch
  tests, the extended `no_secrets_in_logs_connectors`, and the P1 storage/tracing tests);
  TypeScript 772 → **870 tests**, 47 → **51 files** (the `useSubmitGuard` hook + double-fire tests,
  `StorageBanner`, the archive/un-archive + metric-chip + graph-editor + provider-dropdown component
  suites, and the Tier-3 empty/loading/failed coverage) — see `README.md`/`docs/traceability.md`.

### Fixed
- **McpConnect handshake timeout (BL-89, spec D5):** `mcp/lifecycle.rs::connect` previously awaited
  `connect_fn(...)` and `session.list_tools()` unbounded — a peer that accepts the connection but
  never completes the handshake hung the calling task forever, and because orchd's dispatch is
  sequential over one shared connection it wedged the ENTIRE orchd pipeline. Both awaits are now
  bounded by the per-server `timeout_ms` (`OrchdMcpError::Mcp(McpError::Timeout)` on elapse), keeping
  the trust-gate-before-network ordering and the no-DB-lock-across-await property intact.
- **OAuth token-exchange timeout (BL-91, spec D5):** the SSRF-guarded token-exchange HTTP client in
  `connectors/accounts.rs` had no request timeout, so a never-answering token endpoint could hang
  an OAuth exchange indefinitely (unreachable in v1 until O-5's registry ships a provider, then
  live). Added `.timeout(Duration::from_secs(30))` (matching `GenericRestAdapter`), keeping
  `redirect::Policy::none()` + the SSRF guard.
- **Import writes ruleset files only after commit (BL-90):** a whole-store `ImportBundle` that
  failed partway (e.g. a later project colliding on id, rolling the DB transaction back) previously
  left an orphan ruleset `.md` file on disk from an earlier project. The file writes are now
  performed only after `tx.commit()` succeeds, so a rolled-back import leaves NO orphan file; the
  `app_support`/path-traversal validation is unchanged.
- **No silent no-op / no zombie terminal tab on a mutation failure (BL-93):** a rejected
  new-terminal / close-terminal / add-workspace round-trip now surfaces an honest toast
  (`describeCommandError`) instead of failing silently; a close disposes the xterm instance AND
  removes the tab in a `finally` (fixing the dead `removeSession` wiring), so a failed close never
  strands a zombie tab.
- **Reconnect rehydrates all live slices + research runs self-poll (BL-92/spec D8):** `onOrchdUp`
  now refetches the entire live surface (projects; the open project's goals/tasks/ideas/insights/
  ruleset/graph; the Extensions slices; research runs for every loaded idea; the global ruleset when
  opened; the storage status) rather than only projects, so nothing reconnects stale after an
  outage swallowed its `orchd://*-changed` push. `ResearchPane` additionally self-polls
  `researchListRuns` every 2s while a mounted run is non-terminal (and stops on terminal), so a
  missed terminal push never strands a run's badge at `pending`/`running`.
- **Partial-failure resume for spawn-project + insight-backlog (BL-95/spec D6):** when
  `SpawnProjectFromIdea`'s `setIdeaProject` fails after the project/workspace were created (or
  `FormInsightDialog`'s backlog `setIdeaLifecycle` fails after the task was created), the component
  now holds the created ids, names exactly what was created, and retry RESUMES from the failed step
  — never creating a second project/task.
- **Reopenable banner for a cancelled orchd upgrade (BL-96/spec D8):** cancelling the orchd upgrade
  dialog while the daemon is incompatible no longer dead-ends — a persistent banner
  (`OrchdUpgradeBanner`) with an "Update" button reopens the dialog.
- **`describeError` matches the error union in two more surfaces (P-16/P-17):** `SkillsTab`'s native
  file-picker failure and `CreateProjectDialog`'s nested "+ create workspace" failure now map their
  sessiond `CommandError` through `describeCommandError` (preserving the real message) instead of the
  orchd-specific `describeOrchdError` / a raw `Error.message`.

## [0.7.0] — 2026-07-16

### Added
- **Research pipeline — the idea→research→insight→task loop, entirely inside `bpa-orchd`,
  shipped WITHOUT the S6 agent org:** `orchd.db` schema v4 (additive, `SCHEMA_VERSION` 3→4) adds
  exactly ONE net-new table, `research_run` (idea↔MCP-invocation↔artifact provenance +
  `pending|running|done|failed` status). The "ResearchArtifact" the roadmap named is NOT a
  separate entity or blob store — it's the REUSED S-EXT `mcp_artifact` a run's tool call produces
  (no blob duplication, one source of truth, spec D2).
- **The async run driver — orchd's FIRST long-lived `tokio::spawn`:** `ResearchStartRun` inserts
  the run `pending` (in the same transaction as an idea's `captured→researching` lifecycle flip),
  spawns a detached background task, and returns immediately; the task calls the shipped
  `mcp::invoke::call_tool` (never holds the DB lock across the network await), then a SINGLE
  `UPDATE` moves the row to `done{artifact_id,invocation_id}` or `failed{error_kind}`. Every
  transition pushes `ResearchRunsChanged` — the frontend's "research pane" is the push, not a
  poll. `error_kind` is a typed classification only (`policy_cap_exceeded`/`timeout`/`tool_error`/
  `transport`/`interrupted`) — never args, a secret, or tool output.
- **Boot-reconcile of interrupted runs (D11) — the crash/restart safety net a detached background
  task otherwise lacks:** the spawned run task is NOT tracked by `OrchdShutdown{drain}`'s
  connection-task JoinSet, so a restart/crash mid-run would leave a row stuck `pending`/`running`
  forever without this fix. A new boot step (same "ensured at every boot" shape as
  `boot::ensure_global_ruleset`, right after `open_db`) flips every non-terminal run to
  `failed{interrupted}` on daemon start — the owner re-runs. Proven by a new e2e phase (below) that
  deliberately shuts the daemon down mid-run and asserts the reconcile fires on restart.
- **Connect-handshake timeout (D12) — a hang-forever fix in the shipped S-EXT MCP path:**
  `mcp::invoke::call_tool` previously wrapped only the `tools/call` RPC in
  `timeout(server.timeout_ms)`, not the preceding `connect_fn`/`initialize` handshake — a research
  (or any MCP) peer that accepts the connection but never completes `initialize` hung the caller
  forever. Now the connect handshake is bounded by the same per-server timeout too
  (`McpError::Timeout` on elapse) — benefits every MCP call, not just research.
- **Three wire verbs, append-only:** `ResearchStartRun`/`ResearchListRuns`/`ResearchGetRun` +
  entity `ResearchRun`/enum `ResearchStatus` + push `ResearchRunsChanged` — appended at the end of
  `bpa-orchd-proto`'s frozen enums; the orchd wire version space stays `[1,1]` (additive). Every
  other verb the flow needs (spawn-project, insight formation/fit-verdict, task formation,
  spend-preflight) reuses SHIPPED S3/S4/S-EXT verbs — `ResearchStartRun`/`ResearchListRuns`/
  `ResearchGetRun` are the ONLY net-new wire this slice adds. Tauri core gains matching
  `research_start_run`/`research_list_runs`/`research_get_run` commands and a
  `orchd://research-runs-changed` broker event.
- **Idea research flow (frontend):** per idea, «Research» opens `ResearchRunDialog` (pick a
  connected MCP server → `McpListTools` → pick a tool → owner-supplied args JSON → a
  spend-approval preflight reusing `TrustListPolicies`, with an honest "cost usually unknown until
  after the call" note — the trust layer's existing hard caps are unchanged, a breach surfaces as
  `failed{policy_cap_exceeded}`); `ResearchPane` lists runs by status and, on `done`, reuses the
  S-EXT artifact viewer + «unverified data» untrusted banner (NOT token-streaming — MCP
  `tools/call` is request/response in the connect-per-call model, an honest scope line, not a
  partial build); a failed run offers «form insight without research» (Q8 honest degradation,
  the owner path never dead-ends). `FormInsightDialog` prefills title/body from the artifact,
  shows a fit-context panel (the project's goals+`metric_refs` + a `GraphNeighborhood` read)
  beside owner-set `fit_verdict`/`fit_reasoning` (reusing `CreateInsight`/`SetInsightFitVerdict`);
  accepting graph-ingests the insight as an `entity_ref` node (D9, reusing `add_entity_ref_node`)
  and forming a task flips the idea `researching→specced`. `SpawnProjectFromIdea` closes BL-56 (the
  spawn-project-from-idea UI flow S3 deferred) — pure frontend orchestration over the existing
  `CreateWorkspace`/`CreateProject`/`SetIdeaProject` verbs, no new orchd verb. Every mutating
  control is `disabled` while `orchdDown`.
- **E2E (`npm run e2e:orchd`, extended with two phases):** phase 8 registers a local stub MCP
  research server, runs the whole idea→research→insight→task loop, restarts the daemon, and
  asserts the idea/run/insight/task all survive (the roadmap DoD proof). Phase 9 registers a
  BLOCKING stub, starts a run, shuts the daemon down while the run is still `running` (not
  `done`), relaunches, and asserts boot-reconcile flipped it to `failed{interrupted}` (D11 proof —
  the in-flight-at-restart race phase 8 deliberately avoids).
- **Rust:** unit coverage for the run state machine (`start_run`'s one-transaction insert+lifecycle
  flip; each transition a single `UPDATE`, never a two-step write that could violate the
  `(status='done')=(artifact_id IS NOT NULL)` CHECK; every typed failure family mapped to its
  `error_kind`; graph-ingest-on-accept is idempotent, a re-accept-after-archive `Conflict` is a
  benign no-op); `reconcile_interrupted_research_runs` (D11, empty-DB no-op + flips exactly
  `pending`/`running`, leaves `done`/`failed` untouched); the D12 connect-timeout regression
  (`call_tool_connect_that_never_resolves_times_out_not_hangs`); socket-dispatch tests for all
  three verbs + the `ResearchRunsChanged` push.

### Changed
- Gate: still 9 stages (`scripts/final-suite.sh`) — no new stage; `bpa-orchd`'s coverage gate now
  also exercises `research/mod.rs` and `boot.rs`'s reconcile step; `npm run e2e:orchd` grew phases
  8/9 above.
- Test totals grew with the new `research` module: Rust workspace 975 → **1023 tests** (0 failed,
  `RUST_TEST_THREADS=4`) — mostly inside `bpa-orchd` (`research/mod.rs`'s unit tests +
  `boot.rs`'s reconcile tests + the widened `dispatch_integration`) and `bpa-orchd-proto` (the
  three new verbs' CBOR round-trip + ts-rs parity); TypeScript 717 → **772 tests**, 43 → **47
  files** (the `components/idea/` flow: `ResearchRunDialog`/`ResearchPane`/`FormInsightDialog`/
  `SpawnProjectFromIdea`, plus `IdeasList`/store/ipc growth) — re-measured this pass, see
  `README.md`/`docs/traceability.md`.

### Fixed
- (carried into this slice from the post-S-EXT hardening pass, not previously changelogged)
  `McpDeleteServer` now deletes the server's bearer Keychain entry too (no orphaned credential on
  server delete); the project-cascade residual (deleting a project whose MCP servers still hold
  bearer entries) is filed as BL-81, not silently left undocumented.

## [0.6.0] — 2026-07-15

### Added
- **MCP client — the app's first outbound network egress + macOS Keychain surface, entirely
  inside `bpa-orchd`:** two new crates, `bpa-secrets` (the ONLY Keychain caller —
  `security-framework::passwords` set/get/delete, fixed service prefix
  `ai.builderpro.desktop`, never logs the secret bytes — BL-20) and `bpa-mcp` (a thin wrapper
  over the official `rmcp = "2.2"` SDK; orchd domain code never imports rmcp types directly).
  Both transports ship: **Streamable HTTP** (remote servers, e.g. prowl.chat) and **stdio**
  (local child processes, spawned only behind a dedicated `stdio_exec` consent gate). Egress
  never touches `bpa-sessiond` or the GUI core — `bpa-orchd` is the sole host.
- **`orchd.db` schema v3 (additive, `SCHEMA_VERSION` 2→3):** `mcp_server`/`mcp_tool` (the
  registry + cached tool descriptors, global or per-project scope), `account` (OAuth/api-key
  connector accounts — a Keychain ref only, never the token bytes), `mcp_invocation`/
  `mcp_artifact` (per-call records + durable results, `server_id` XOR `account_id` so an
  `McpCallTool` and a `ConnectorInvoke` share one persistence path), `skill` (SKILL.md
  registry), `consent_grant`/`policy`/`audit_log` (the trust layer).
- **Tool discovery + per-tool allowlist + typed invoke:** a server registry (add/enable/disable);
  cached `tools/list` per server; a per-tool `enabled` toggle enforced pre-dispatch (a disabled
  tool is rejected before any network call, `Error{Policy}`); typed `tools/call` with a
  per-server timeout and bounded retry — retried ONLY on a transport-level pre-dispatch failure,
  never a blind re-invoke of a possibly side-effecting tool — and honest degradation on every
  terminal failure (a typed error + an audit row, never a silent swallow).
- **Durable, untrusted artifacts:** every successful `tools/call` (and every successful
  `ConnectorInvoke`) persists an `mcp_artifact` row (`is_untrusted=1` — the flag a future S6b
  agent-boundary mediation step will read) that survives an `bpa-orchd` restart. Cost/token
  fields on `mcp_invocation` are `Option`, populated only when the server itself reports usage
  — honestly `null` otherwise, never a fabricated estimate.
- **Connectors — an OAuth-account layer, decoupled from MCP:** OAuth 2.1 (authorization-code +
  PKCE via the `oauth2` crate, an SSRF-guarded token-exchange client —
  `redirect::Policy::none()` — refresh-on-expiry) or a static api-key account; tokens/keys
  always in Keychain via `bpa-secrets`, `orchd.db` holding only the ref. One reference direct-API
  adapter — `GenericRestAdapter` (`provider="generic-rest"`, `get`/`post` against an
  account-scoped URL with the account's bearer) — proves the `ConnectorAdapter` trait seam.
  `ConnectorInvoke` routes through the identical trust-choke-point and invocation/artifact
  persistence path as `McpCallTool`.
- **Trust layer (BL-22), a single pre-dispatch choke-point (`trust::authorize`) in `bpa-orchd`:**
  connect consent (owner-granted, fingerprint = URL, re-prompted on change); a DISTINCT
  `stdio_exec` consent for spawning a local process (fingerprint = the resolved binary's
  sha256, falling back to the command string when the binary can't be resolved — re-prompted on
  a binary/command change); the per-tool allowlist; spend/rate policy caps (a rolling 60 s
  window, most-specific configured scope wins outright — server > project > global, never a
  per-field merge — a spend cap binds only when a call's cost is actually known); untrusted-
  result tagging on every artifact from both `McpCallTool` and `ConnectorInvoke`; an
  append-only `audit_log` row on every connect / stdio-spawn / tool-call / connector-invoke /
  consent / policy-deny (`reason` is NEVER a secret or tool argument).
- **Shared `DYLD_*`/`LD_*` env denylist (closes BL-1):** a new `bpa_daemon_core::env_filter`
  helper strips any `DYLD_*`/`LD_*`-prefixed key (case-sensitive) from a stdio MCP child's env
  AND from `bpa-sessiond`'s `env_overrides` (previously applied unfiltered — the original BL-1
  gap). A stdio child's env is `env_clear()`'d and built entirely by the caller (orchd's own
  filtered ambient env merged with the DB's `server.env`, server wins on collision) — no ambient
  inheritance leak from either source.
- **Skills — a SKILL.md-format registry (plumbing only):** CRUD + files-as-truth
  (`Present`/`Modified`/`Missing`, mirrors `ruleset_files.rs`'s pattern). There is no runtime
  consumer yet (that's the S6b agent org); the «Skills» tab states this honestly rather than
  presenting the registry as executable.
- **Frontend — «Extensions», a new top-level view** (alongside Home/Workspace/Project): Servers
  (MCP server registry + connect/consent), Tools (tool browser + per-tool allowlist +
  invoke, an untrusted-result banner on every response), Connectors (OAuth/api-key accounts +
  the generic-rest ops runner), Log (invocation log + audit log + a spend/rate policy
  editor), Artifacts (durable results + an untrusted banner per item), Skills (the skills
  registry). Every mutating control is `disabled` while `orchd://down`.
- **E2E (`npm run e2e:orchd`, extended with two phases):** phase 6 registers a local stub HTTP
  MCP server → grants connect consent → connects (tools cached) → lists tools → calls the
  `echo` tool → asserts a durable artifact → restarts orchd → asserts the artifact survived
  (S-EXT spec §9 Phase-1 DoD). Phase 7 does the connector-shaped analogue — an api-key
  `generic-rest` account against a local stub REST target → `ConnectorInvoke` → artifact
  survives an orchd restart — beginning with a Keychain-availability probe that gracefully,
  loudly SKIPs the phase (never a silent pass) on a runner whose login keychain is
  locked/unavailable, so the gate stays honest on headless CI without masking a real failure
  when the keychain IS available (S-EXT spec §9 Phase-2 DoD).
- **Rust:** unit + integration coverage for every layer above (registry CRUD + invariants;
  the trust choke-point's consent/allowlist/spend/rate-cap deny paths, each with an audit-row
  assertion; the DYLD/LD env-filter mutation-tested against a real spawned process; connector
  OAuth PKCE + SSRF-guard + Keychain-roundtrip; the generic-rest adapter against a loopback
  stub; SKILL.md frontmatter parse + symlink-escape rejection + file-state classification;
  socket-dispatch tests for every new verb, each asserting the correct coarse-invalidation
  push); orchd-proto CBOR round-trip + ts-rs parity for every new entity/verb/push (all
  append-only, orchd's wire version space stays `[1,1]`).

### Changed
- Gate: still 9 stages (`scripts/final-suite.sh`) — no new stage; every existing stage now also
  exercises the S-EXT surface (orchd coverage, ts-rs parity for the new proto types,
  `npm run e2e:orchd` phases 6/7).
- Test totals grew with the new crates/modules: Rust workspace 727 → **975 tests** (two new
  crates — `bpa-secrets`, `bpa-mcp` — plus the `bpa-orchd`/`bpa-orchd-proto` MCP/connector/
  skill/trust surface); TypeScript 559 → **717 tests**, 35 → **43 files** (re-measured this
  pass — see `README.md`/`docs/traceability.md`).

### Fixed
- `crates/orchd/src/connectors/adapter.rs`'s doc comment ("`connector_invoke` audit_log row
  (allow or, once T18 lands, deny)") was stale — the spend/rate policy caps T18 refers to
  shipped in this same slice. Reworded to "allow or deny".

## [0.5.0] — 2026-07-14

### Added
- **Knowledge graph — `orchd.db` schema v2:** two new tables, `graph_node` (typed `kind`:
  concept/fact/artifact/decision/note/`entityRef`) and `graph_edge` (typed `kind`:
  relates/depends/derives/supports/contradicts/parent), added by an additive forward-only
  migration (`SCHEMA_VERSION` 1→2) — a pre-S4 `orchd.db` upgrades on first boot with no data loss;
  sessiond's `bpa.db` is untouched. All persistence + retrieval logic lives in one new module,
  `crates/orchd/src/graph.rs` — no new crate.
- **`entityRef` nodes are soft-refs, not foreign keys (D3):** an `entityRef` node stores
  `entity_type` + `entity_id` (goal/idea/insight/task) with NO DB-enforced link to the domain
  row it names. Deleting the referenced goal/idea/insight/task never deletes or corrupts the
  graph node — the node persists, and a read-time resolver looks up the live domain row's title
  on every read; when the row is gone the node keeps its last-known stored label and the UI
  renders `isOrphan: true` («source deleted»). Exactly one `entityRef` node exists per
  `(entity_type, entity_id)` (partial unique index; a second attempt is a typed `Conflict`). A
  strategic-goal `entityRef` node is auto-seeded inside `CreateProject`'s own transaction (D6) —
  a project's graph is never empty — and the schema-v2 migration backfills one for every project
  that predates S4.
- **Cross-project edges:** a `graph_edge` may connect nodes belonging to two DIFFERENT projects —
  legal because both live in the one `orchd.db` store (`ON DELETE CASCADE` removes a node's
  incident edges automatically on delete). A cross-project edge survives BOTH projects' daemon
  restarts (S4 spec §8 DoD; proven by `tests/e2e/orchd-survive.mjs` phase 5: create two projects,
  add a node to each, link them, restart the daemon, assert the edge and the foreign node both
  reappear).
- **Workspace-wide graph retrieval API — the S6-agent contract, read AND write, NOT
  project-scoped (D5):**
  - `GraphListProject { project_id }` → the project's own nodes + every edge incident to them +
    the foreign endpoint nodes of any cross-project edge, returned as read-only `external_nodes`
    ghosts.
  - `GraphNeighborhood { node_id, depth }` → a bidirectional recursive-CTE traversal up to `depth`
    hops (clamped to 6), crossing project boundaries freely — the `<100 ms` DoD query: a depth-3
    neighborhood rooted at a project's strategic-goal node, on a synthetic 500-node/1000-edge
    graph, measures ~51 ms.
  - `GraphSearch { query, project_id: Option<..> }` → case-insensitive `label`/`body` substring
    search, workspace-wide when `project_id` is `None`, capped at 200 rows, newest-updated first.
  Plus the mutating verbs: `GraphAddNode`/`GraphUpdateNode`/`GraphMoveNode`/`GraphDeleteNode`/
  `GraphAddEdge`/`GraphDeleteEdge` — 9 graph verbs total, appended to the END of `OrchdRequest`/
  `OrchdResponse` (orchd-proto's frozen append-only wire discipline, unchanged version `[1,1]`).
  Every mutating verb honors the S3 archived-project guard (either endpoint's project archived ⇒
  `Invariant`); a self-loop edge is `Invariant`; a duplicate `(source, target, kind)` edge is
  `Conflict`.
- **`orchd://graph-changed` push, fanned out to every affected project (deduped):** a coarse
  `GraphChanged { projectId }` push (mirrors S3's other `orchd://*-changed` pushes) broadcasts on
  every successful mutation — not just to the mutated row's own project. A cross-project edge
  mutation pushes to BOTH endpoint projects; a node update/move/delete pushes to its own project
  PLUS every foreign project that has it as an `external_nodes` ghost — so a stale cross-project
  ghost is never left un-invalidated. Read verbs and failed mutations broadcast nothing (S3 §6
  discipline, unchanged).
- **Core:** 9 `orchd_graph_*` Tauri commands (thin wrappers over the new `OrchdClient` verbs, one
  per wire verb) and the `orchd://graph-changed` event, wired through `broker.rs`'s
  `map_orchd_push` exactly like every other `orchd://*-changed` push.
- **Frontend — graph canvas, a 7th `ProjectPanel` tab «Graph»:** an editable `@xyflow/react` (v12)
  canvas (`src/components/graph/GraphCanvas.tsx`), controlled via two pure, fully-unit-tested
  mapping helpers (`src/components/graph/graphMapping.ts`: `toFlowNodes`/`toFlowEdges`/
  `flowPositionChangeToMove`/`dedupeMovesById`, zero `@xyflow/react`/React imports — trivially
  testable under plain `node`). Dragging a node debounces (400 ms) into `GraphMoveNode`;
  connecting two nodes calls `GraphAddEdge`; a toolbar adds a node of a chosen kind, deletes the
  canvas's own multi-selection, and searches (a match gets a 2px accent outer ring — never a fill
  change). Every mutating control is `disabled` while `orchd://down` (mirrors `RulesetPanel`'s
  degradation contract); the search input stays live (it's a read). Clicking a cross-project
  ghost node navigates to its own project (`openProject`); clicking a LOCAL `entityRef` node is
  currently an honest no-op — the panel has no deep-link seam yet from the graph tab into a
  specific goal/idea/insight/task row on another tab, so this stays a no-op rather than faking a
  navigation that wouldn't actually land on the referenced entity (tracked as follow-up work, not
  silently dropped).
- **`@xyflow/react`** — the one new frontend dependency this cycle (Context7-verified v12
  controlled-component API: `nodes`/`edges` props, `onNodesChange`/`onConnect`,
  `ReactFlowProvider`).
- New design-system atoms: Graph node card (external ghosts dimmed/dashed, orphaned nodes get a
  `statusExited` border, a search match gets the accent ring), Graph toolbar (kind select + add +
  delete-selected + debounced search, mutating controls disabled while `orchdDown`)
  (`docs/design-system.md` §5).
- **E2E (`npm run e2e:orchd`, extended):** a new phase 5 — create two projects, add one node to
  each, add a CROSS-PROJECT edge, `OrchdShutdown{drain:true}` → relaunch → `GraphListProject`
  still shows the edge with the foreign node as an `external_nodes` ghost. This is the S4 spec §8
  DoD proof ("a cross-project link survives BOTH projects' restarts"). Existing phases 0-4
  (project/goal/idea/task CRUD survival + export/import round-trip) stay green, unchanged.
- **Rust:** `graph.rs` unit tests for every persistence/retrieval method and invariant (incl.
  `add_node{kind:EntityRef}` rejected as `Validation` — `entityRef` nodes are created only via the
  internal `add_entity_ref_node`, never the generic wire verb; entityRef soft-ref survival across
  a non-strategic domain-entity delete; the v1→v2 migration backfill from a real v1 fixture; the
  `<100 ms` perf assertion above); orchd-proto CBOR round-trip + ts-rs parity for every new
  variant; socket-dispatch tests over a real Unix socket (mutate → response + the correct
  `GraphChanged` push(es); cross-project edge/node mutations → push for the foreign project too;
  read verbs → no push; archived-project guard).

### Changed
- **`orchd.db` `SCHEMA_VERSION` 1 → 2** (additive, forward-only — see "Added" above).
- `crates/orchd/src/socket_server.rs` dispatch grows the 9 graph verb arms; `bpa-orchd`'s
  `Broadcaster<OrchdFrame>` fan-out gains the "broadcast once per distinct affected project"
  helper the graph pushes share with future multi-project push needs.
- Gate: still 9 stages (`scripts/final-suite.sh`) — no new stage. Stage 6 (ts-rs type-parity diff)
  now also covers the graph entities/verbs in `src/ipc/orchd-types.ts`; the orchd coverage gate
  (stage 7) and `npm run e2e:orchd` (stage 9) both now exercise the graph module.
- Test totals grew with the new module: Rust workspace 655 → **726 tests**; TypeScript
  502 → **559 tests**, 33 → **35 files** (re-measured this pass — see `README.md`/
  `docs/traceability.md`).

## [0.4.0] — 2026-07-14

### Added
- **`bpa-orchd`, the second launchd daemon:** a per-user LaunchAgent
  (`ai.builderpro.desktop.orchd`) hosting the app-domain store — projects, goals, ideas, insights,
  tasks, rulesets. Reuses `bpa-sessiond`'s patterns verbatim: fail-closed forward-only migrations,
  flock single-instance, peer-cred (`getpeereid`) refusal, drain/consent upgrade choreography. Its
  own Hop-B socket (`orchd.sock`/`orchd.lock`), own SQLite DB (`orchd.db`), own logs
  (`orchd.tracing.log`/`orchd.out.log`/`orchd.err.log`), own independent wire version space
  `[1,1]` (same `BPAA` preamble magic as sessiond — daemons distinguished by socket path, not by
  preamble content). Ops runbook: `docs/runbook-orchd.md`.
- **`bpa-daemon-core` extraction:** six shared modules (`dirs`, `singleton`, `logging`, `migrate`,
  `handshake`, `broadcast`) factored out of `bpa-sessiond` FIRST, then `bpa-sessiond` re-seated on
  them with behavior byte-identical (on-disk socket/lock/plist paths asserted unchanged by test)
  before `bpa-orchd` was built on the same foundation — final architecture immediately, no
  "duplicate now, refactor later".
- **Domain schema v1 + full CRUD for six entity families:** Project (workspace links, archive),
  Goal (full tree — exactly one `strategic` root per project, `additional` subgoals at arbitrary
  depth via `parent_id`, move/reorder, delete-subtree cascade), Idea (lifecycle
  captured→researching→specced→in-dev→shipped→archived, nullable `project_id` for orphan/inbox
  ideas, `SetIdeaProject` to attach/detach), Insight (fit-verdict fit/no-fit/unknown vs
  goals/metrics, owner override via `SetInsightFitVerdict`, archive requires non-empty
  `resolutionReasoning`), Task/Subtask (unified model — kanban is a future VIEW over it — status
  groups backlog/todo/waiting/progress/testing/done, `rank` reordering via midpoint math), RuleSet
  (global + per-project). Every create/update/delete replies the updated entity (or `Ack`) AND
  broadcasts a coarse `orchd://*-changed` push ONLY on success — failed requests broadcast
  nothing.
- **RuleSet markdown files — the source of truth (D4):** DB stores `md_path` + `md_hash`
  (sha256); files are atomic-written (tmp+rename); external edits/deletions surface honestly
  (`Ok` / `ExternallyModified` / `Missing`) instead of silently overwriting or hiding drift. A
  deliberate NARROW exception to "orchd gets its own file API in S9" (architecture.md amended) —
  this is the ONLY file I/O anywhere in the `bpa-orchd` crate, not a general file API.
- **Export / import:** per-project and whole-store JSON bundles (`bundleFormat: 1`), every row
  field preserved verbatim on import (ids, `created_at`/`updated_at`, `rank`, `md_hash` — never
  re-stamped), id collisions rejected as a typed `Conflict` with the whole transaction rolled
  back, round-trip proven (import into an empty store → re-export equals the original modulo
  `exportedAt`). A 16 MiB frame-cap guard answers a typed `Io` error instead of attempting a
  doomed oversized send (chunked export tracked as a backlog row).
- **Frontend — project management UI:** left-rail project groups (project header + nested
  workspace rows, «No project» group, create-project dialog); a tabbed `ProjectPanel` (Overview ·
  Goals `GoalTree` · Ideas `IdeasList` · Tasks `TasksList` · Insights `InsightsList` · Rules
  `RulesetPanel`); ⌘K quick-capture (`QuickCapture`) — global overlay, title/body/project select,
  `CreateIdea` on Enter, disabled with an honest inline note while orchd is down; `HomeGoals`
  mounted below the S2 attention sections (the amber «Needs you» block keeps its pinned-top spot)
  showing each active project's strategic goal + direct children with status chips.
- **Honest degradation for the second daemon:** `orchd://down` → shared banner + [Retry]
  (`orchd_reconnect`) on every domain surface, mutating controls disabled; `orchd://incompatible`
  → the existing `UpgradeDialog` generalized to read both daemons' flag pairs, rendering one
  dialog at a time (sessiond first if both are incompatible — no combined choreography); orchd's
  own upgrade copy is honest that no live session is at risk (no PTYs to lose).
- New design-system atoms: Tree row, Lifecycle chip, Policy form, File-state banner, Project group
  row, Quick-capture overlay (`docs/design-system.md` §5).
- **E2E (`npm run e2e:orchd`, `tests/e2e/orchd-survive.mjs`):** boot on a temp HOME → handshake
  `[1,1]` → create a project (+2 goals, an idea, a task) → `OrchdShutdown{drain:true}` → relaunch
  → data intact → `ExportAll` → shutdown → delete `orchd.db*` → relaunch (fresh v1) →
  `ImportBundle` → re-export equals the original modulo `exportedAt` — the roadmap DoD proof
  (goals+ideas+tasks CRUD survive restart; export/import round-trips).

### Changed
- **Gate: 8 stages → 9.** `scripts/final-suite.sh` adds `bpa-orchd` to the ts-rs type-parity diff
  (`src/ipc/orchd-types.ts`) and the coverage gate (`cargo llvm-cov --package bpa-orchd
  --fail-under-lines 80`, alongside `bpa-sessiond`'s existing gate), and a new stage 9
  `npm run e2e:orchd`. `.github/workflows/ci.yml` updated in lockstep.
- `src-tauri/src/launchd.rs`'s `LaunchdAgent` parameterized ADDITIVELY (`label`,
  `stdout_log_name`, `stderr_log_name` fields) so the same install/bootstrap/kickstart machinery
  renders either daemon's plist; sessiond call sites pass the pre-existing values byte-identically
  (asserted by test), orchd call sites pass its own identity.

### Fixed
- `crates/orchd/src/socket_server.rs`'s module doc overclaimed it was the only place in the crate
  calling `SystemTime::now()` — `persistence.rs` also does, for row `created_at`/`updated_at`.
  Reworded to scope the claim to the `exported_at` stamp specifically (T10 Minor).

## [0.3.0] — 2026-07-09

### Added
- **Multi-root workspaces:** a workspace is now an ordered list of equal repo roots
  (`Workspace.roots: Vec<String>`; `root_path` stays a compat mirror, always `roots[0]`). Daemon
  schema v3 adds `workspace_root(workspace_id, ord, path)` behind a fail-closed forward-only
  migration; new wire requests `AddWorkspaceRoot`/`RemoveWorkspaceRoot` (validated, last-root
  removal rejected) broadcast `Push::WorkspaceUpdated` → `workspace://updated` to every attached
  client (Pv2 multi-subscriber).
- **File explorer + read-only preview:** `listDir`/`readFilePreview`/`createFile`/`createDir`/
  `renameEntry`/`moveEntry`/`deleteEntry`(→Trash)/`revealInFinder`/`openExternal`, all core-local
  (`src-tauri/src/fs_explorer.rs`), gitignore-aware (`ignore` crate, `.git` always hidden),
  1 MiB-capped preview with honest binary/too-large/truncated placeholders — never a silent
  truncated-as-whole read. Every op validated against the active workspace's roots first
  (`bpa_paths::validate_path_within`/`validate_parent_within`, new shared-crate functions).
- **Live file watch:** debounced FSEvents watch (`notify` + `notify-debouncer-full`, 250 ms) per
  active workspace root, gitignore-filtered, capped/deduped `fs://changed{root,changedRelPaths}`
  (`["*"]` sentinel on overflow) or honest `fs://watch-error{root,reason}` — GUI-lifetime only
  (starts on activation, stops on switch/unmount).
- **Attention-first Home:** on open, sessions waiting for input are pinned first (amber) with a
  one-click «Go →» that navigates, activates, and focuses that terminal; then running; then
  recently exited (✓/✗ by exit code) — across every workspace, computed from the existing store,
  never polled.
- **OSC-133 command strip:** per-session recent-command chips (✓/✗ by exit code, running-dot for
  an in-flight command) sourced from `GetCommandEvents` (newest-first) — the first real UI
  consumer of the `command_events` table persisted since Pv2.
- **Terminal file links:** a pure, store-free regex resolver (`src/terminal/link-provider.ts`)
  lexically detects path-like tokens in terminal output (absolute/dot-relative/extensioned-relative,
  optional `:line[:col]` suffix) and an xterm `ILinkProvider` + OSC-8 `linkHandler` open a match in
  the right-rail preview on click, authoritatively re-validated against the workspace's roots at
  click time — a miss is a quiet toast, never a silent no-op.
- Three-rail UI: `⌂ Home` navigation rail, center Home/Workspace view, collapsible right FILES
  rail (`FileTree` + `FilePreview`).
- New design-system atoms: `Toast` (queue-of-one, `role="alert"`), `File tree`, `Preview pane`,
  `Command strip` (`docs/design-system.md` §5).

### Changed
- **Three-rail layout** replaces the two-pane (sidebar + terminal) shell; left rail is pure
  navigation, file explorer lives in a new collapsible right rail, hidden on Home.
- **MSRV: `rust-version` 1.77.2 → 1.88.0.** The declared floor was already false before this
  cycle: it never matched the resolved `Cargo.lock` graph (`plist`/`time`/`darling`/`serde_with`,
  pulled in transitively via `tauri`, declare 1.88.0 — verified against every locked crate's own
  `rust-version` field on both macOS targets). This cycle's own `trash` 5.2.6 addition (file
  delete → Trash) declares a lower 1.85.0, so it wasn't the binding constraint; 1.88.0 is. The
  pinned toolchain (`rust-toolchain.toml`) is 1.92, so this was never a build-breaking gap in
  practice on this repo's own CI/dev machines — only a false floor claim for anyone building on an
  older, "supported" Rust. Fixed in `Cargo.toml` and the S0+S1 spec's locked-versions table.
- **Protocol v2 → v3** (one planned wire break: S2's multi-root `Workspace` + new verbs are not
  v2-decodable). An old v2 daemon negotiates `Incompatible` → the upgrade-consent dialog +
  `kickstart -k` restart the bundled v3 daemon; existing 0.2.0 installs upgrade through the dialog
  and live sessions rehydrate inactive (D4).

### Fixed
- **BL-14:** `applyReplay` now calls `term.reset()` before every Replay (including re-attach) —
  a re-attach no longer duplicates scrollback into the xterm buffer.
- **BL-29:** app-wide explicit `:focus-visible` (2px accent ring, `src/index.css`) — every
  interactive element now shows a visible focus ring on keyboard navigation, matching what
  `docs/design-system.md` already promised.

## [0.2.0] — 2026-07-07

### Changed
- **Hop-B wire codec: bincode → CBOR (ciborium).** Tagged enums are plain serde derives; the v1
  dual-codec bridge (`*Shape` mirrors, `is_human_readable` split) is retired. One planned,
  non-silent wire break (see the upgrade flow below).
- Version negotiation: codec-agnostic preamble (`BPAA`, client `[min,max]` → daemon
  `Accepted{chosen}`/`Incompatible{range}`, 5s bound, 256B build-string cap) replaces the
  in-band `Hello`/`Welcome` frames.

### Added
- Multi-subscriber attach: N independent subscribers per session at the wire/daemon level
  (per-subscriber replay + backpressure; GUI stays a single subscriber for now).
- Real `DaemonShutdown{drain}`: flush scrollback + command events, ack, graceful exit —
  same path as SIGTERM; launchd does not auto-restart a clean exit.
- Upgrade consent flow: incompatible-daemon detection (typed, fatal, never auto-retried) →
  honest banner + consent dialog (N live sessions counted) → best-effort drain →
  `launchctl kickstart -k` → app relaunch; kickstart failure surfaces honestly.
- Schema v2: `command_events` table (best-effort from OSC-133 C/D marks, `origin` column),
  fail-closed forward-only migration from v1.
- Cold-rehydrate: at boot the daemon loads every persisted session as an inactive replay-only
  entry; attaching an inactive session replays its scrollback (no new wire request needed).
- E2E: harness speaks the v2 wire (preamble + hand-rolled standard CBOR); new phase 5 —
  drain → daemon exit → relaunch same state dir → rehydrated `isActive:false` + scrollback
  marker intact (closes BL-7).

### Fixed
- Daemon per-connection writer-task hang on a client peer that stops reading (bounded 200ms
  join + abort).
- E2E preamble reader `sock.unshift()` race (phase-0 hang).
- E2E harness wrote the real user DB (`HOME` now isolated per run).
- `CommandError` struct-variant fields now serialize camelCase to the webview
  (container-level `rename_all` does not cascade).

## [0.1.0] — 2026-07-04

### Added
- S0+S1 foundation + terminal core: launchd-managed `bpa-sessiond` daemon owning PTYs
  (survive-GUI-restart), OSC-133/7 shell integration, sanitized scrollback replay,
  SQLite persistence, React/xterm.js frontend with per-session attach state machine.
- Shared `bpa-protocol` + `bpa-paths` crates; ts-rs generated TS types (diff-gated).
- Gates: workspace tests, clippy -D warnings, rustfmt, vitest, tsc, ts-rs parity,
  daemon coverage ≥80 %, e2e survive-restart.
