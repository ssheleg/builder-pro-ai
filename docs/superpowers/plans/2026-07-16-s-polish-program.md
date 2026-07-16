# S-POLISH Program Implementation Plan (P1–P5, [0.8.0])

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development to
> implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Close every confirmed finding of the first-session UX investigation and ship the four
owner-approved features, ending at a fully English, CI-gated, production-grade [0.8.0].

**Architecture:** Five sequenced phases in ONE plan: P1 backend reliability/observability (Rust
only) → P2 English localization + enforcement → P3 frontend reliability/polish → P4 features →
P5 release docs. Cross-cutting contracts are LOCKED in the program spec
`docs/superpowers/specs/2026-07-16-s-polish-program-design.md` (D1–D8) — every task below cites
the contract it implements; on any perceived conflict the spec governs.

**Tech stack:** existing only — Rust 1.92 (tokio, rusqlite, tracing, reqwest via rmcp 2.2 /
connectors), Tauri 2, React 19 + TS + zustand + vitest. **No new dependencies.** No Context7 pass
needed: every API used below (tokio::time::timeout, reqwest builder timeout, tracing macros,
xyflow interactions) is already exercised by green tests in this repo; tasks mirror those proven
in-repo patterns byte-for-byte where possible.

## Global Constraints

- **English only** (spec D2): all new/changed strings, comments, tests, docs. From T6 onward the
  no-cyrillic gate enforces this; earlier tasks must simply not add Cyrillic.
- **Wire is append-only:** new enum variants at the very END of `OrchdRequest`/`OrchdResponse`/
  `OrchdPush`; frame enums plain snake_case, NO ts-rs; entity structs camelCase + ts-rs +
  `#[ts(export_to = "orchd-types.ts")]`, i64 → `#[ts(type = "number")]` (mirror `McpArtifact`).
- **Never hold the DB `Mutex` across a network await** (3-phase locking, as in
  `mcp/invoke.rs::call_tool`).
- **No secrets in logs** — extend, never weaken, the `no_secrets_in_logs*` tests.
- TDD per task: failing test → confirm fail → minimal impl → confirm pass → commit
  (conventional message + `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`).
- Full gate: `bash scripts/final-suite.sh` (RUST_TEST_THREADS=4, staged sidecars); BL-40 attach
  PTY flake = retry-once; `connectors::accounts` keychain hang = pre-existing env issue, CI is
  authoritative.
- PATH for all shell steps:
  `export PATH="$HOME/.cargo/bin:$HOME/.rustup/toolchains/1.92-aarch64-apple-darwin/bin:/opt/homebrew/bin:$PATH"`.
- Worktree: `.claude/worktrees/s-polish`, branch `worktree-s-polish`, base = current `main`.

---

# Phase P1 — backend reliability + observability (Rust only)

### Task T1: wire — StorageStatus entity + GetStorageStatus verb (spec D3)

**Files:** Modify `crates/orchd-proto/src/lib.rs`; regenerate `src/ipc/orchd-types.ts`
(ts_export test); temp dispatch arm in `crates/orchd/src/socket_server.rs`.

**Interfaces — Produces:** `StorageMode`, `StorageStatus`, `OrchdRequest::GetStorageStatus`,
`OrchdResponse::StorageStatus(StorageStatus)` (T2, T20 consume).

- [ ] **Step 1 (RED):** roundtrip + ts_export tests: `GetStorageStatus` encodes as bare string
  (unit variant, mirror `ConnectorListAccounts`); `StorageStatus` entity camelCase
  (`storageMode`, `quarantinedPath`); `StorageMode` wire tags `persistent` /
  `in_memory_fallback` / `recovered_from_corruption`. Run
  `cargo test -p bpa-orchd-proto` → FAIL.
- [ ] **Step 2 (GREEN):** append at tails:

```rust
/// How orchd's persistence layer actually opened at boot (spec S-POLISH D3). Fixed at boot —
/// clients pull it on connect; there is no push because it can never change mid-run.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "snake_case")]
#[ts(export_to = "orchd-types.ts")]
pub enum StorageMode {
    Persistent,
    InMemoryFallback,
    RecoveredFromCorruption,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize, ts_rs::TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "orchd-types.ts")]
pub struct StorageStatus {
    pub storage_mode: StorageMode,
    pub quarantined_path: Option<String>,
}
```

  plus `GetStorageStatus,` at the END of `OrchdRequest` and
  `StorageStatus(StorageStatus),` at the END of `OrchdResponse`. Temp dispatch arm returns
  `Error{ code: Io, message: "storage status not wired yet".into() }` (mirrors the T3-era
  research stub precedent). Regenerate ts.
- [ ] **Step 3:** `cargo test -p bpa-orchd-proto && cargo test -p bpa-orchd` → PASS. Commit
  `feat(orchd-proto): StorageMode/StorageStatus + GetStorageStatus verb (S-POLISH D3, append-only)`.

### Task T2: Db open outcome + boot storage mode + real dispatch (spec D3)

**Files:** Modify `crates/orchd/src/persistence.rs`, `crates/orchd/src/boot.rs`,
`crates/orchd/src/socket_server.rs` (ServerDeps + replace T1 stub),
`src-tauri/src/commands.rs` + `src-tauri/src/lib.rs` (command `orchd_storage_status`),
`src/ipc/orchd.ts` (wrapper `orchdStorageStatus`).

**Interfaces — Consumes:** T1. **Produces:** `Db::open_with_outcome(path) -> Result<(Db,
DbOpenOutcome)>`; `DbOpenOutcome { Clean, RecoveredFromCorruption { quarantined_to: PathBuf } }`;
`ServerDeps.storage_status: StorageStatus`; command
`orchd_storage_status() -> Result<StorageStatus, CommandError>`.

- [ ] **Step 1 (RED):** persistence tests: (a) fresh path → `(db, Clean)`; (b) corrupt file
  (write garbage bytes) → `(db, RecoveredFromCorruption{quarantined_to})` AND the quarantine file
  exists; (c) existing `Db::open` still compiles/passes (delegates, discards outcome). boot test:
  `open_db_degrading` on an unwritable dir → in-memory Db + `StorageMode::InMemoryFallback`.
  dispatch test: `GetStorageStatus` → `OrchdResponse::StorageStatus` echoing deps. Run → FAIL.
- [ ] **Step 2 (GREEN):** implement `open_with_outcome` by threading the existing quarantine
  branch's path outward (the quarantine logic itself is UNCHANGED); `boot::run` computes
  `StorageStatus { storage_mode, quarantined_path }` and stores it in `ServerDeps`; dispatch arm
  returns it (no lock needed — it is immutable). Command + wrapper mirror
  `research_get_run`/`researchGetRun` exactly (Error→`CommandError::Daemon`).
- [ ] **Step 3:** `cargo test -p bpa-orchd && cargo test -p builder-pro-ai --lib && cargo clippy
  --workspace --all-targets -- -D warnings` → PASS. Commit
  `feat(orchd): storage-mode surface — Db open outcome, boot resolution, GetStorageStatus dispatch + command (S-POLISH D3)`.

### Task T3: timeouts — McpConnect lifecycle + OAuth exchange (spec D5; BL-89, BL-91)

**Files:** Modify `crates/orchd/src/mcp/lifecycle.rs`, `crates/orchd/src/connectors/accounts.rs`.

- [ ] **Step 1 (RED):** (a) lifecycle test
  `connect_that_never_resolves_times_out_not_hangs`: connect_fn = `std::future::pending()`,
  server `timeout_ms: 50` → `McpError::Timeout` within bounded time — byte-for-byte mirror of
  `mcp/invoke.rs`'s `call_tool_connect_that_never_resolves_times_out_not_hangs`; (b) same for a
  session whose `list_tools` never resolves; (c) accounts test: make the HTTP client timeout a
  parameter — `fn ssrf_guarded_http_client(timeout: Duration)` (default call sites pass
  `Duration::from_secs(30)`) — test binds a `TcpListener` that accepts and never responds, calls
  `complete_oauth` with a 100 ms client → error (not a hang), asserted via
  `tokio::time::timeout(2s, …)` around the call. Run → FAIL ((a)/(b) hang-guarded by an outer
  test timeout).
- [ ] **Step 2 (GREEN):** wrap BOTH awaits in `lifecycle.rs`:

```rust
let timeout = std::time::Duration::from_millis(server.timeout_ms.max(1) as u64);
let session = match tokio::time::timeout(timeout, connect_fn(server.clone(), bearer)).await {
    Ok(res) => res?,
    Err(_elapsed) => return Err(OrchdMcpError::Mcp(bpa_mcp::McpError::Timeout)),
};
let tools = match tokio::time::timeout(timeout, session.list_tools()).await {
    Ok(res) => res.map_err(OrchdMcpError::Mcp)?,
    Err(_elapsed) => return Err(OrchdMcpError::Mcp(bpa_mcp::McpError::Timeout)),
};
```

  (adapt to the file's exact fn shape; the invoke.rs D12 block is the reference). Add
  `.timeout(timeout)` to the reqwest builder in `ssrf_guarded_http_client`.
- [ ] **Step 3:** `cargo test -p bpa-orchd mcp && cargo test -p bpa-orchd connectors -- --skip
  keychain` (skip only if the env hangs; name what was skipped) → PASS. Commit
  `fix(orchd): bound McpConnect connect/list_tools and OAuth exchange by timeouts (S-POLISH D5, BL-89 BL-91)`.

### Task T4: import — stage ruleset file writes until after commit (BL-90)

**Files:** Modify `crates/orchd/src/export.rs`.

- [ ] **Step 1 (RED):** test `import_collision_leaves_no_orphan_ruleset_file`: two-bundle import
  where bundle 1 carries a ruleset md and bundle 2 collides on a PK → import fails `Conflict`,
  DB untouched (existing assertion), AND bundle 1's md file does NOT exist on disk. Run → FAIL.
- [ ] **Step 2 (GREEN):** during the import tx, collect `(PathBuf, String)` pending writes
  instead of calling `write_atomic`; after `tx.commit()` succeeds, write them all (write failure
  after commit → `warn!` per file, import still Ok — post-commit best-effort, mirroring the
  insight graph-ingest precedent). Fix the now-true doc comment.
- [ ] **Step 3:** `cargo test -p bpa-orchd export` → PASS. Commit
  `fix(orchd): stage import ruleset file writes until after commit — no orphan on rollback (BL-90)`.

### Task T5: single choke-point tracing (spec D4; O-6)

**Files:** Modify `crates/orchd/src/socket_server.rs` (verb_name + dispatch trace),
`src-tauri/src/orchd_client.rs`, `src-tauri/src/socket_client.rs`,
`crates/sessiond/src/socket_server.rs`; extend `crates/orchd/tests/no_secrets_in_logs.rs`.

- [ ] **Step 1 (RED):** (a) unit test `verb_name_matches_wire_name` — for a sample of ≥10
  requests, `verb_name(&req)` equals the single-key CBOR map key (via
  `serde_json::to_value`; unit variants compare to the bare string); exhaustive `match` makes
  full coverage a compile-time property; (b) a tracing-subscriber capture test: dispatching one
  ok verb and one failing verb yields exactly one `dispatch` info line each with fields
  `verb`, `outcome`, `error_code` (err only), `elapsed_ms`, and no request payload text;
  (c) extend the no-secrets tests: a `McpSetServerBearer` dispatch traced line must not contain
  the token. Run → FAIL.
- [ ] **Step 2 (GREEN):** implement `fn verb_name(req: &OrchdRequest) -> &'static str`
  (exhaustive match, one line per verb) and wrap the dispatch call site:

```rust
let started = std::time::Instant::now();
let verb = verb_name(&req);
let response = dispatch_inner(req, deps, broadcaster).await;
let elapsed_ms = started.elapsed().as_millis() as u64;
match &response {
    OrchdResponse::Error { code, .. } => {
        tracing::info!(verb, outcome = "err", error_code = ?code, elapsed_ms, "dispatch")
    }
    _ => tracing::info!(verb, outcome = "ok", elapsed_ms, "dispatch"),
}
```

  Same single-point pattern in `orchd_client::request` / `socket_client` request path /
  sessiond dispatch (request variant name + ok/err + elapsed).
- [ ] **Step 3:** full `cargo test --workspace` (or per-crate if keychain env hangs) + clippy →
  PASS. Commit `feat(observability): single choke-point per-verb tracing across orchd, sessiond, and core clients (S-POLISH D4, O-6)`.

---

# Phase P2 — English localization + enforcement (spec D1, D2)

### Task T6: `src/strings.ts` + locked copy + conventions

**Files:** Create `src/strings.ts` + `src/strings.test.ts`.

**Produces (contract for T7–T11):** `export const strings = { … } as const` with top-level areas
`common`, `daemon`, `home`, `sidebar`, `workspace`, `files`, `terminal`, `project`, `goals`,
`ideas`, `research`, `insights`, `tasks`, `graph`, `rules`, `ext`, `errors`, `quickCapture`.
Parameterized copy = arrow functions. LOCKED strings (verbatim):

```ts
// errors (describeOrchdError mapping — same code → text shape as today, English)
invalidOperation: (msg: string) => `invalid operation: ${msg}`,
conflict: (msg: string) => `conflict: ${msg}`,
notFound: "not found",
invalidData: (msg: string) => `invalid data: ${msg}`,
serviceError: (msg: string) => `service error: ${msg}`,
consentRequired: (msg: string) => `connection consent required: ${msg}`,
policyDenied: (msg: string) => `denied by policy: ${msg}`,
orchdUnavailable: "orchestrator unavailable",
unknownOrchdError: "unknown orchestrator error",
// daemon
orchdDownBanner: "Orchestrator unavailable", retry: "Retry",
sessiondDisconnected: "Daemon disconnected — reconnecting…",
daemonOutdated: "Background service is outdated — update required", update: "Update",
storageRecovered: (p: string) =>
  `Database was corrupted and has been reset. The damaged copy was saved to ${p}.`,
storageInMemory: "Storage unavailable — running in memory. Changes will NOT survive a restart.",
// ext
untrustedData: "unverified data",
skillsRegistryNote: "Skills are a registry; they execute once the agent orchestra (S6b) arrives.",
comingSoon: "coming soon",
noOauthProviders: "No OAuth providers configured — add one in oauth_providers.json (see runbook)",
```

- [ ] **Step 1 (RED):** `strings.test.ts`: module exports every area; a recursive walk asserts
  every leaf is a non-empty string or function AND contains no Cyrillic. Run
  `npx vitest run src/strings.test.ts` → FAIL.
- [ ] **Step 2 (GREEN):** write the full module — every area populated with the ENGLISH copy for
  every string currently rendered by its components (source inventory: `grep -rnP '[А-Яа-яЁё]'
  src/ --include='*.tsx' --include='*.ts'`). Translation rule: meaning-preserving, terse UI
  English, sentence case, no exclamation marks. Commit
  `feat(ui): central English copy module src/strings.ts (S-POLISH D1)`.

### Tasks T7–T11: component sweep to `strings.*` (five batches)

**Rule for every batch:** each component's Cyrillic literals move to `strings.<area>.<key>`
(reusing T6 keys; add missing keys in the same commit); its co-located test updates to assert
via imported `strings.*` (never hard-coded literals); behavior is UNCHANGED — this is a
copy-source refactor, zero logic edits. Verify per batch:
`npx vitest run <batch files>` + `npx tsc --noEmit` + `grep -P '[А-Яа-яЁё]' <batch files>` → empty.
One commit per batch: `refactor(ui): batch N components to strings.ts (English) (S-POLISH P2)`.

- [ ] **T7 (core chrome):** App.tsx, WorkspaceSidebar, HomeView, HomeGoals, DaemonBanner,
  OrchdDownBanner, UpgradeDialog, Toast, QuickCapture, store.ts, terminal-manager.ts.
- [ ] **T8 (workspace/files):** TerminalTabs, CommandStrip, FilesRail, FileTree, FilePreview,
  CreateProjectDialog.
- [ ] **T9 (project domain):** ProjectPanel, GoalTree, IdeasList, TasksList, InsightsList,
  RulesetPanel.
- [ ] **T10 (idea/research + graph):** ResearchRunDialog, ResearchPane, FormInsightDialog,
  SpawnProjectFromIdea, GraphCanvas, ipc/orchd.ts (`describeOrchdError` → strings.errors),
  ipc/orchd-types.ts comments.
- [ ] **T11 (ext):** ExtPanel, ServersTab, ToolsBrowser, ConnectDialog, ConnectorsTab,
  ArtifactsTab, InvocationLog, SkillsTab.

### Task T12: Rust-side English strings

**Files:** Modify `crates/orchd/src/boot.rs`, `crates/orchd/src/socket_server.rs`,
`crates/orchd/src/persistence.rs`, `crates/orchd/src/graph.rs`, `src-tauri/src/commands.rs`
(+ the remaining files from `grep -rlP '[А-Яа-яЁё]' crates/ src-tauri/src/`).

- [ ] **Step 1 (RED):** update the tests that assert these literals (strategic-goal title,
  ruleset templates, picker title) to the English values below → FAIL.
- [ ] **Step 2 (GREEN):** `STRATEGIC_GOAL_TITLE = "Strategic goal"`;
  `GLOBAL_RULESET_TEMPLATE = "# Global rules\n"`; project template
  `format!("# Project rules: {}\n", project.name)`; `.set_title("Choose SKILL.md")`; translate
  every remaining Cyrillic literal/comment/doc-comment in Rust sources. NO schema migration —
  spec §2 locks the reset-local-DB decision; old rows keep old titles (owner resets).
- [ ] **Step 3:** `cargo test --workspace` (keychain caveat) + clippy + fmt → PASS. Commit
  `refactor: English Rust-side copy — templates, defaults, comments (S-POLISH P2)`.

### Task T13: docs sweep to English

**Files:** README.md, CHANGELOG.md, docs/architecture.md, docs/frontend-conventions.md,
docs/runbook-daemon.md, docs/runbook-orchd.md, docs/traceability.md, docs/backlog.md,
CONTRIBUTING.md, docs/build-macos.md, docs/superpowers/specs/2026-07-01-builderpro-platform-overview.md
(+ anything else `grep -rlP '[А-Яа-яЁё]'` finds outside the D2 allowlist).

- [ ] Translate all Cyrillic content in the enforced doc set (UI-copy citations now quote the
  English strings from T6–T12); fix the F-1 stale banner quote in architecture.md to the LOCKED
  `skillsRegistryNote`; add the standing English-only rule to CONTRIBUTING.md. Verify:
  `grep -rlP '[А-Яа-яЁё]' README.md CHANGELOG.md CONTRIBUTING.md docs/ | grep -v -f
  scripts/english-allowlist.txt` → empty. Commit
  `docs: English sweep + standing English-only rule (S-POLISH P2, O-2)`.

### Task T14: no-cyrillic CI gate + runbook DB-reset step

**Files:** Create `scripts/check-english.sh`, `scripts/english-allowlist.txt`; modify
`scripts/final-suite.sh` (stage 10), `.github/workflows/ci.yml`, `docs/runbook-orchd.md`.

- [ ] **Step 1:** `check-english.sh`:

```bash
#!/usr/bin/env bash
# S-POLISH D2: the product is English-only. Fails on any Cyrillic outside the allowlist
# (a closed list of pre-existing archival specs/plans/QA records).
set -euo pipefail
REPO="$(cd "$(dirname "$0")/.." && pwd)"
cd "$REPO"
matches=$(grep -rlP '[А-Яа-яЁё]' src crates src-tauri/src tests scripts docs README.md CHANGELOG.md CONTRIBUTING.md 2>/dev/null \
  | grep -vxF -f scripts/english-allowlist.txt || true)
if [ -n "$matches" ]; then
  echo "Cyrillic found outside the allowlist:" >&2
  echo "$matches" >&2
  exit 1
fi
echo "OK: English-only gate clean"
```

  `english-allowlist.txt` = the exact pre-existing files under docs/superpowers/specs|plans and
  docs/qa (generated once via the same grep at this task's start, MINUS the platform overview),
  each with a `#` reason header at the top of the file.
- [ ] **Step 2:** run the gate → must PASS (T7–T13 done); wire it as final-suite stage 10 + a
  ci.yml step in both jobs. Add the one-time human step to runbook-orchd.md: delete
  `~/Library/Application Support/ai.builderpro.desktop/orchd.db{,-wal,-shm}` once after this
  release so English defaults apply. Full `bash scripts/final-suite.sh` → `ALL GATES PASSED`.
  Commit `ci: English-only gate (stage 10) + allowlist + DB-reset runbook step (S-POLISH D2)`.

---

# Phase P3 — frontend reliability + polish

### Task T15: toast queue + manual dismiss (BL-97, spec D8)

**Files:** Modify `src/store/store.ts` (queue, cap 5 drop-oldest, auto-advance),
`src/components/Toast.tsx` (close button wiring `dismissToast`); tests.
- [ ] RED: two rapid `showToast` → both messages render in order; close button advances
  immediately; queue caps at 5. GREEN. `npx vitest run` batch → PASS. Commit
  `fix(ui): toast FIFO queue + manual dismiss (BL-97)`.

### Task T16: `useSubmitGuard` + apply everywhere (BL-95a, spec D6)

**Files:** Create `src/hooks/useSubmitGuard.ts` (+test); modify QuickCapture,
CreateProjectDialog, ResearchRunDialog, FormInsightDialog (all 3 stages), SpawnProjectFromIdea,
IdeasList/TasksList/GoalTree create-forms, ServersTab, 