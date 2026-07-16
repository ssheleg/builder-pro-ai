# S-POLISH — post-0.7.0 UX-remediation program (design)

**Status:** approved decomposition, per-slice specs/plans follow.
**Target release:** [0.8.0] (cut at P5).
**Source of findings:** `docs/qa/ux-investigation-report.md` (101 first-session scenarios, 4 waves) +
`docs/qa/ux-first-session-scenarios.md` (catalog). Backlog rows BL-89…BL-97.

## 1. Goal

Bring the shipped v0.7.0 first-session experience to production-grade: close the one Critical
reliability bug and all Important reliability gaps, expose the capabilities that are wired but
UI-unreachable, add per-verb observability, and convert the entire product to English with a
standing enforcement rule. Delivered as **five sequenced slices**, each independently shippable,
each its own spec → plan → subagent-driven execution → two-stage review → merge → CI green.

## 2. Owner decisions (locked via review, 2026-07-16)

| # | Decision |
|---|---|
| O-1 | Term mapping confirmed: **hypothesis = Idea (+ its verified form Insight)**, **feature = DomainTask**. Catalog stands; no re-cut. |
| O-2 | **English everywhere.** New standing rule: all UI strings, docs, and locked-copy are English; anything non-English is translated. Enforced by a CI no-cyrillic gate. |
| O-3 | Project **archive + a NEW un-archive verb** + UI controls (no one-way trap). |
| O-4 | `metric_refs` **owner editor on goals**, shipped in v1. |
| O-5 | Build the **config-backed OAuth provider registry** (D14 phase 3); this activates the BL-91 timeout fix on a now-reachable path. |
| O-6 | **Add per-verb structured tracing** (fix, not merely document the gap). |
| O-7 | Graph editor is a **v1 defect** — add node title/body form + inline-rename + edge-label editing (backend `update_node` already wired). |
| Scope | **100% — all tiers, multi-slice.** Autonomous to a complete result. |
| EN-how | **Central `src/strings.ts`** module (not inline). |
| Migration | **Reset the local DB** after P2 (no schema migration for the cosmetic EN rename). |

## 3. Decomposition (Approach A — order by coupling)

Frontend coupling is total (the EN sweep touches all 36 components; reliability + feature fixes
touch the same components). Backend timeouts/tracing are Rust-only (near-zero overlap). Ordering
therefore: backend-first (ships the Critical without waiting on EN), then the EN baseline, then
frontend logic on that baseline, then features, then release.

| Slice | Title | Contents | Layer |
|---|---|---|---|
| **P1** | Backend reliability + observability | BL-89 (McpConnect timeout), BL-91 (OAuth-exchange timeout), BL-90 (import file-before-commit), BL-94-back (surface DB-degraded mode on the status wire), O-6 (per-verb tracing) | Rust only |
| **P2** | English localization + enforcement | `src/strings.ts` + all 36 components + 11 Rust strings + 28 docs → English; CI no-cyrillic gate; reset-DB human step | Frontend + Rust strings + docs + CI |
| **P3** | Frontend reliability + UX polish | BL-92, BL-93, BL-94-front, BL-95, BL-96, BL-97 + Tier-3 frontend minors | Frontend |
| **P4** | Tier-2 features | O-3 archive/un-archive, O-4 metric_refs editor, O-5 OAuth registry, O-7 graph editor | Rust + frontend |
| **P5** | Docs truth + CHANGELOG [0.8.0] + final gate | version bump, traceability, close BL rows (docs already EN from P2) | Docs |

Each slice's own spec locks its detailed contracts before its plan. This program spec locks only
the **cross-cutting** contracts below (§4), which multiple slices depend on.

## 4. Cross-cutting contracts (locked here — every slice honors these)

### D1 — `src/strings.ts` central copy module (P2 introduces; P3/P4 extend)
- One module `src/strings.ts`, a nested `const strings = { … } as const` grouped by surface area
  (`common`, `home`, `project`, `goals`, `ideas`, `research`, `insights`, `tasks`, `graph`,
  `ext`, `errors`, `daemon`, …). Leaf values are English strings; parameterized copy is a function
  `(x) => \`…${x}…\``.
- Every UI call-site references a key (`strings.research.runButton`), never an inline literal.
  Component tests assert against `strings.*` keys (import the same module), not hard-coded literals,
  so copy changes never silently break tests.
- **English-only. No runtime i18n framework, no locale switch, no multi-language** (YAGNI — the rule
  is "always English", not "translatable"). `strings.ts` is a single-locale copy registry.
- Rust-side user-facing strings (locked templates, `STRATEGIC_GOAL_TITLE`, picker titles, error
  message text) are translated in place — Rust has no `strings.ts` equivalent; keep them as the
  existing `const`/inline literals, just English.

### D2 — CI no-cyrillic gate (P2 adds; every later slice must keep it green)
- New `final-suite.sh` stage (and mirrored `ci.yml` step): `scripts/check-english.sh` greps for
  Cyrillic (`[А-Яа-яЁё]`) across `src/`, `crates/`, `src-tauri/src/`, `tests/`, `scripts/`,
  `docs/`, `README.md`, `CHANGELOG.md` and must return **zero matches** outside the allowlist;
  any other match fails the gate. This is the machine-enforcement of O-2.
- **Allowlist (explicit file list, committed as `scripts/english-allowlist.txt`, one path per
  line, commented):** the pre-existing files under `docs/superpowers/specs/`,
  `docs/superpowers/plans/`, and `docs/qa/` written before this program — they are frozen
  historical decision/investigation records; retroactively rewriting them would falsify history
  and buys no product value. EXCEPTION to the exception: the living platform overview
  (`docs/superpowers/specs/2026-07-01-builderpro-platform-overview.md`) is NOT allowlisted — it is
  the active roadmap and IS translated. All NEW files anywhere (including new specs/plans) must be
  English — the allowlist is a closed list of exact pre-existing paths, so any new file is
  enforced automatically. A deliberate non-ASCII test payload may be added to the allowlist with a
  one-line reason; default is no exceptions.
- Standing rule recorded in `CONTRIBUTING.md`: all code, comments, UI copy, commits, and docs are
  English.

### D3 — DB-degraded mode on the wire (P1 backend, P3 frontend banner) — LOCKED
- Wire (append-only at every enum tail): request `GetStorageStatus` (unit variant) → response
  `StorageStatus(StorageStatus)`. Entity (camelCase + ts-rs, mirrors `McpArtifact` derives):
  `StorageStatus { storage_mode: StorageMode, quarantined_path: Option<String> }`. Enum
  `StorageMode { Persistent, InMemoryFallback, RecoveredFromCorruption }` (plain snake_case wire
  tags via `#[serde(rename_all = "snake_case")]`, exported to ts). No preamble change (the
  handshake stays frozen); the mode is fixed at boot, so pull-on-connect suffices — no push.
- Producer: `persistence::Db::open_with_outcome(path) -> Result<(Db, DbOpenOutcome)>` where
  `DbOpenOutcome { Clean, RecoveredFromCorruption { quarantined_to: PathBuf } }`; the existing
  `Db::open` delegates and discards the outcome (call-site compat). `boot::open_db_degrading`
  maps: fallback-to-memory → `InMemoryFallback`, quarantine outcome → `RecoveredFromCorruption`,
  else `Persistent`, and stores the resolved `StorageMode` (+ optional path) in `ServerDeps`.
- Consumer: a `research_get_run`-style Tauri command `orchd_storage_status`; the frontend fetches
  it on initial connect and on every `orchd://up`, stores it in a `storageStatus` slice.
- P3 renders a persistent honest banner for the two non-`Persistent` modes:
  `RecoveredFromCorruption` → "Database was corrupted and has been reset. The damaged copy was
  saved to <path>."; `InMemoryFallback` → "Storage unavailable — running in memory. Changes will
  NOT survive a restart."

### D4 — per-verb tracing convention (P1) — LOCKED (single choke-point, not 77 arm edits)
- orchd: ONE completion trace in `socket_server::dispatch` — an exhaustive
  `fn verb_name(&OrchdRequest) -> &'static str` match (compile-time exhaustive, so a new verb
  cannot ship untraced) + after dispatch:
  `info!(verb, outcome = "ok"|"err", error_code = ?, elapsed_ms)` where `error_code` is the wire
  `OrchdErrorCode` debug name when the response is `Error{..}`. Fields NEVER include args, bodies,
  tokens, tool output, or PII (extends the existing `no_secrets_in_logs` tests).
- core: ONE trace each in `orchd_client::request` and `socket_client`'s request path
  (request variant name + ok/err + error code + elapsed) — covers all 117 command handlers at the
  layer they share instead of editing each.
- sessiond dispatch gets the same single choke-point trace.

### D5 — timeout wraps (P1)
- `mcp/lifecycle.rs` `McpConnect`: wrap `connect_fn(…)` and `list_tools()` in
  `tokio::time::timeout(Duration::from_millis(server.timeout_ms), …)`, `Err(_elapsed)` →
  `McpError::Timeout` — byte-for-byte mirroring the S-IDEA D12 fix in `mcp/invoke.rs`; regression
  test mirroring `call_tool_connect_that_never_resolves_times_out_not_hangs`.
- OAuth token-exchange/refresh client (`connectors/accounts.rs` `ssrf_guarded_http_client`): add
  `.timeout(Duration::from_secs(30))` (mirror `GenericRestAdapter`) + a test.

### D6 — double-submit + partial-failure discipline (P3, referenced by P4's new dialogs) — LOCKED
- Hook `useSubmitGuard()` (new `src/hooks/useSubmitGuard.ts`): returns
  `{ submitting, guard }` where `guard(fn)` returns a wrapped handler that no-ops while an
  invocation is in flight and flips `submitting` around the `await`. EVERY mutating submit
  (existing dialogs + list create-forms + any new P4 dialog) uses it and adds
  `disabled={… || submitting}`; each gets a double-fire test (two rapid clicks → wrapper called
  once).
- Multi-step chains resume-from-failed-step: the component keeps the ids of already-completed
  steps in state; on failure the error text names exactly what WAS created, and the retry button
  resumes from the failed step (never re-runs completed steps). Applies to SpawnProjectFromIdea
  (workspace/project ids) and FormInsightDialog's backlog step (task id → retry only the
  lifecycle flip).

### D7 — P4 feature contracts — LOCKED
- **Un-archive:** new wire verb `UnarchiveProject { id }` → `OrchdResponse::Project` + push
  `ProjectsChanged` (appended at enum tails). `persistence::unarchive_project`: `archived` →
  `active`; unknown id → `NotFound`; already-active → `Invariant("project is not archived")`.
  UI: ProjectPanel Overview gets an "Archive project" button (confirm dialog); the sidebar gains a
  collapsed dimmed "Archived" group listing archived projects; opening one shows a read-only
  banner with an "Un-archive" button. Archived projects stay read-only (existing guards).
- **metric_refs editor:** a chip editor on each goal row in `GoalTree` (add via text input +
  Enter, remove via chip ×) calling the existing `orchdUpdateGoal(…, metricRefs)`;
  `orchdCreateGoal` stays without metric_refs (goals are born empty; edit after). Fit-context
  consumes them unchanged.
- **Graph editor:** (a) add-node form (title input required, body textarea optional, kind select)
  replacing the hardcoded placeholder title; (b) inline rename — double-click a LOCAL
  (non-entityRef) node → input → `orchdGraphUpdateNode`; (c) edge editing — selecting an edge
  shows a kind select firing a NEW verb `GraphUpdateEdge { id, kind }` →
  `OrchdResponse::GraphEdge` + `GraphChanged` push (appended at tails; guards mirror
  `GraphAddEdge`: NotFound, archived-endpoint `Invariant`). The edge "label" IS its rendered
  `kind` — no new schema column (no v5 migration needed for this).
- **OAuth provider registry:** config file `<app-support>/oauth_providers.json`
  (`{ "<provider>": { "client_id", "auth_url", "token_url", "default_scopes"?: [..],
  "client_secret"? } }`), loaded at boot into `ConnectorsState` (missing file → empty registry,
  info-logged, NOT an error; malformed file → error-logged + empty registry, daemon still boots).
  New verb `ConnectorListProviders` → `OrchdResponse::ConnectorProviders(Vec<String>)` (names
  only — no secrets on the wire). UI: the free-text provider input becomes a dropdown fed by it;
  empty registry → honest empty-state "No OAuth providers configured — add one in
  oauth_providers.json (see runbook)" with the OAuth begin button disabled. `client_secret` is
  covered by the existing redacting-Debug pattern and never logged/returned.

### D8 — P3 reconnect/self-refresh contracts — LOCKED
- `onOrchdUp` refetches EVERY live slice: projects; the open project's goals/ideas/insights/
  tasks/ruleset/graph; mcp servers + artifacts + accounts + skills + policies + invocations;
  research runs for every idea currently holding runs in the store; the global ruleset when the
  rules surface is open; and `orchd_storage_status` (D3).
- Research runs additionally self-heal without pushes: while a `ResearchPane` is mounted and shows
  a non-terminal run (`pending`/`running`), it polls `researchListRuns(ideaId)` every 2 s and
  stops on terminal state — covers the lost-push and boot-reconcile cases with no new wire.
- BL-96: a persistent banner keyed on `orchdIncompatible && !orchdUpgradeDialogOpen` ("Orchestrator
  service is outdated — update required" + an "Update" button reopening the dialog), mirroring the
  sessiond `DaemonBanner` pattern.
- BL-97 toast: `showToast` appends to a FIFO queue (cap 5, drop-oldest); the visible toast renders
  a manual close (wires the existing `dismissToast`) and auto-advances every 4 s.

## 5. Per-slice Definition of Done

Common to every slice (from global rules): TDD (failing → minimal → green), honest error handling +
degradation on every external call, structured logs without secret leakage, module docs/runbook
updated in the same change, two-stage review (spec + code) per task, whole-branch review before
merge, full `scripts/final-suite.sh` green (now including the D2 no-cyrillic gate) + CI green on
macos-15. No placeholders, no deferred TODOs.

- **P1 DoD:** a never-answering MCP connect and a never-answering OAuth exchange both fail by
  timeout in bounded time (tests); import leaves no orphan file on rollback (test); `storage_mode`
  is readable by a client on connect (test); every mutating verb + command handler emits one
  structured trace line on ok and err (tests), zero secrets. Gate + CI green.
- **P2 DoD:** `grep -rP '[А-Яа-яЁё]'` over the enforced paths is empty; the app renders entirely in
  English; `strings.ts` is the single copy source referenced by all components; component tests
  assert via `strings.*`; the CI no-cyrillic stage is live and green. Runbook documents the one-time
  local-DB reset.
- **P3 DoD:** the reconnect handler rehydrates every live-visible slice + research runs self-refresh
  (no permanently-stuck run); the no-op triad surfaces a toast + disposes the terminal + drops the
  tab; degraded-mode banner shows for both non-persistent modes; every mutating submit is
  double-submit-guarded; the two multi-step chains are compensating; toast queues + has a dismiss.
  Each with a test. Gate + CI green.
- **P4 DoD:** archive AND un-archive reachable in UI and round-trip (tests); metric_refs editable on
  a goal and surfaced in fit-context (test); an OAuth provider registered via config completes an
  end-to-end (stubbed) begin→complete with the D5 timeout in force (test); a graph node can be
  created with a title, renamed inline, and an edge labeled (tests). Gate + CI green.
- **P5 DoD:** CHANGELOG `[0.8.0]`, roadmap/traceability/README truthful and re-measured, all closed
  BL rows marked done, remaining Tier-3 minors either fixed or explicitly backlog-tracked. Gate + CI.

## 6. Traceability — every finding maps to a slice

| Finding | Slice |
|---|---|
| BL-89 McpConnect timeout (Critical) | P1 (D5) |
| BL-91 OAuth-exchange timeout | P1 (D5) |
| BL-90 import file survives rollback | P1 |
| BL-94 DB-degradation no UI indication | P1 (wire, D3) + P3 (banner) |
| O-6 per-verb tracing | P1 (D4) |
| O-2 English everywhere + rule | P2 (D1, D2) |
| BL-92 reconnect rehydration + research stuck | P3 |
| BL-93 silent no-op triad + zombie tab | P3 |
| BL-95 partial-failure + double-submit guard | P3 (D6) |
| BL-96 upgrade-cancel dead-end | P3 |
| BL-97 toast clobber | P3 |
| Tier-3 frontend minors (empty-states, edit-revert, per-row error, consent-recovery link, loading≠empty) | P3 |
| O-3 project archive + un-archive | P4 |
| O-4 metric_refs editor | P4 |
| O-5 OAuth provider registry | P4 |
| O-7 graph editor (title/rename/edge-label) | P4 |
| CHANGELOG [0.8.0] + docs truth + close BLs | P5 |

## 7. Open items folded into slices (not blockers)

- Tier-3 minors not individually BL'd (raw-message vs describeError inconsistency, provenance
  invisibility H-02, N+1 refreshResearchRuns K-06, input-length limits K-04, XOR-CHECK→Io B-08,
  audit no-live-push P-23) are enumerated in the P3/P5 specs and either fixed there or explicitly
  backlog-tracked with a one-line reason (no silent drop).
- List virtualization (K-06) beyond FileTree is a P3 judgment call (measure first); if deferred,
  BL-tracked.

## 8. Human steps (isolated, one only)

After **P2** merges: delete the local `orchd.db` (and its `-wal`/`-shm`) once so the English
`STRATEGIC_GOAL_TITLE` and English ruleset templates apply to a fresh database. The runbook step
(P2) gives the exact path and command. No other manual step in the program.
