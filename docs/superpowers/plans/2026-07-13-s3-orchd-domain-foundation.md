# S3 — `bpa-orchd` + App-Domain Foundation: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship the second launchd daemon `bpa-orchd` (final ADR-HOST topology) with domain schema v1 + full CRUD for Projects/Goals/Ideas/Insights/Tasks/RuleSet, and the owner-facing UI (project rail groups, goal tree, ideas inbox + ⌘K capture, task list, ruleset editor, export/import, Home goals panel).

**Architecture:** Phase 1 extracts shared `bpa-daemon-core` and re-seats `bpa-sessiond` on it (byte-identical behavior, full gate = phase boundary). Phase 2 boots the orchd skeleton (own socket/DB/launchd label, preamble `[1,1]`). Phase 3 lands domain persistence + handlers + ruleset files + export/import. Phases 4–5 wire core client/commands and the frontend. Spec is authoritative: `docs/superpowers/specs/2026-07-13-s3-orchd-domain-foundation-design.md` (D1–D9, wire enums §4.2, DDL §5.1, invariants §5.2).

**Tech Stack:** existing workspace only — `rusqlite 0.32`, `ciborium 0.2`, `ts-rs 10.1`, `uuid 1.23`, `sha2 0.10`, `tokio`. **D9: zero new external dependencies** (`sha2` is already in the graph; add it to the relevant `Cargo.toml` members only).

## Global Constraints

- Spec §4.2 wire enums are copied VERBATIM (order frozen append-only from day one); orchd version space `[1,1]` via `ORCHD_CLIENT_MIN/MAX_VERSION` / `ORCHD_DAEMON_MIN/MAX_VERSION` consts — never literals at call sites (the S2 wave-E lesson, commit 2f42e95).
- Phase 1 changes NO sessiond behavior: every extraction keeps sessiond public API via thin wrappers; on-disk paths (socket, lockfile, bpa.db, logs) byte-identical. Full existing suite green after each Phase-1 task.
- orchd DB: spec §5.1 DDL verbatim; `PRAGMA foreign_keys=ON` per connection; WAL; unix-ms timestamps; `uuid::Uuid::new_v4().to_string()` ids; migrations via `daemon_core::migrate::run_migrations` (fail-closed, forward-only, one tx per step).
- Every §5.2 invariant is a typed `OrchdErrorCode`; no silent failure anywhere (spec §11 matrix).
- RuleSet md files: files are truth; atomic write (tmp+rename); sha256 hex hashes; content NEVER logged.
- Every `#[tauri::command]` = thin wrapper over a unit-testable inner fn; serde tag="kind" camelCase per-variant (container `rename_all` does NOT cascade into struct-variant fields — S2 Task-8 lesson).
- TS mirrors: `src/ipc/orchd-types.ts` is ts-rs GENERATED (parity-gated); hand-written TS types only for core-local shapes.
- Design-system: new atom ⇒ new row in `docs/design-system.md` in the SAME task. Amber reserved for «нужен ты».
- Gate: `bash scripts/final-suite.sh` → `ALL GATES PASSED` (extended in T20 to 9 stages incl. `bpa-orchd` coverage ≥80% + `e2e:orchd`).
- Commits: conventional, trailer `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`.
- Work in a worktree branch `worktree-s3` (superpowers:using-git-worktrees), never on main.

## Task graph

Phase 1 (sequential): T1 → T2 → T3. Phase 2: T4 → T5. Phase 3 (sequential, shared `crates/orchd/src/persistence.rs` and `socket_server.rs`): T6 → T7 → T8 → T9 → T10. Phase 4: T11 → T12. Phase 5: T13, then parallel group {T14, T15, T16, T17} (non-overlapping files), then T18 → T19. Close: T20 → T21 → T22.

---

## Phase 1 — `bpa-daemon-core` extraction (sessiond re-seat)

### Task 1: daemon-core crate — `dirs` + `singleton` + `logging`

**Files:** Create `crates/daemon-core/Cargo.toml` (name `bpa-daemon-core`; deps: `tokio` (net,io-util,time), `tracing`, `tracing-subscriber`, `tracing-appender`, `libc` — copy exact versions/features from `crates/sessiond/Cargo.toml`), `crates/daemon-core/src/lib.rs` (`pub mod dirs; pub mod singleton; pub mod logging;`), `crates/daemon-core/src/{dirs.rs,singleton.rs,logging.rs}`. Modify root `Cargo.toml` (add member), `crates/sessiond/Cargo.toml` (dep on daemon-core), `crates/sessiond/src/{boot.rs,singleton.rs,logging.rs}`.
**Interfaces — Produces (spec §3, locked):** `dirs::app_support_dir() -> PathBuf` (moved from `sessiond/boot.rs`, `$HOME`-resolved, same `ai.builderpro.desktop` leaf); `singleton::{LockGuard, acquire_lock_at(&Path) -> io::Result<LockGuard>, resolve_socket_path(file_name: &str) -> PathBuf, resolve_lockfile(file_name: &str) -> PathBuf, ensure_socket_dir() -> io::Result<()>, assert_socket_path_len(&Path) -> io::Result<()>, set_socket_mode(&Path) -> io::Result<()>, check_peer_cred(BorrowedFd) -> io::Result<()>}` (name-parameterized versions of `sessiond/singleton.rs`); `logging::{init_to_file(&Path) -> io::Result<()>, flush()}`. Sessiond keeps its exact current publics as one-line wrappers passing its current file names (socket/lock names read from the current `resolve_*` impls before deleting them — copy the literals).

- [ ] **Step 1: RED.** Move each module's existing tests into daemon-core alongside the code; add: `resolve_socket_path("a.sock")` / `resolve_lockfile("a.lock")` end with the given names in the same runtime dir sessiond used; sessiond wrapper test asserting `bpa_sessiond`'s resolved socket/lock paths are BYTE-IDENTICAL to their pre-move literals (hardcode expected leaf names in the test). `cargo test -p bpa-daemon-core -p bpa-sessiond` → FAIL (crate empty).
- [ ] **Step 2: GREEN.** Move code, parameterize names, wire wrappers. No logic edits.
- [ ] **Step 3:** `cargo test --workspace` → PASS (sessiond 168 + integration binaries untouched). `cargo clippy --workspace -- -D warnings`, `cargo fmt --all -- --check`. Commit `refactor(daemon-core): extract dirs/singleton/logging; sessiond re-seated, paths byte-identical (S3 §3, phase 1)`.

### Task 2: daemon-core `migrate` runner

**Files:** Create `crates/daemon-core/src/migrate.rs` (+ `pub mod migrate;` in lib.rs; add `rusqlite` dep to daemon-core matching sessiond's version/features). Modify `crates/sessiond/src/persistence.rs` (re-seat `migrate()` on the runner).
**Interfaces — Produces (spec §3, locked):** `pub struct Migration { pub from: i64, pub apply: fn(&rusqlite::Transaction) -> rusqlite::Result<()> }`; `pub fn run_migrations(conn: &mut Connection, target: i64, steps: &[Migration]) -> Result<(), MigrateError>` — semantics extracted from `sessiond/persistence.rs::migrate` (read `PRAGMA user_version`; apply each step whose `from` ≥ current in order; each step: ONE transaction that runs `apply` then bumps `user_version` to `from+1`; any error ⇒ rollback, version unchanged, `MigrateError` (fail-closed); version > target ⇒ `MigrateError::FutureSchema`). `pub enum MigrateError { FutureSchema { found: i64, supported: i64 }, Sql(rusqlite::Error) }`.

- [ ] **Step 1: RED.** daemon-core tests: fresh in-memory DB reaches target with all steps applied; mid-chain failure (step whose `apply` returns Err) leaves version at pre-step value and prior steps committed; future-version DB ⇒ `FutureSchema`; empty steps + target 0 ⇒ Ok. Sessiond: existing migration tests (v2 fixture → v3, fail-closed fixture) must pass UNCHANGED after re-seat. FAIL first.
- [ ] **Step 2: GREEN.** Implement runner; rewrite `Db::migrate` as a `&[Migration]` table (v1→v2, v2→v3 bodies moved verbatim into step fns).
- [ ] **Step 3:** `cargo test --workspace` → PASS. Commit `refactor(daemon-core): fail-closed migration runner; sessiond schema steps re-seated verbatim (S3 §3)`.

### Task 3: generic framing + `server_handshake` extraction

**Files:** Modify `crates/protocol/src/framing.rs` (generic core), create `crates/daemon-core/src/handshake.rs` (+ dep `bpa-protocol`), modify `crates/sessiond/src/socket_server.rs` (accept path re-seat).
**Interfaces — Produces (spec §4.1/§3, locked):** in `bpa-protocol`: `pub fn encode_cbor_frame<T: Serialize>(v: &T) -> Result<Vec<u8>, FrameError>`; `pub struct CborFrameDecoder<T> { … }` with `new()/push(&[u8])/decode() -> Result<Vec<T>, FrameError>`; existing `encode_frame(&Frame)` + `FrameDecoder` become thin instantiations (public API + `MAX_FRAME_LEN` unchanged). In daemon-core: `pub async fn server_handshake(stream: &mut tokio::net::UnixStream, min: u16, max: u16, build: &str) -> io::Result<Option<u16>>` — `PREAMBLE_TIMEOUT`-bounded read of the client preamble, `negotiate`, write Accepted/Incompatible reply; `Ok(Some(chosen))` accepted, `Ok(None)` incompatible-replied (caller closes), `Err` on timeout/garbage (extracted from sessiond's accept path, semantics identical incl. the bounded reply write).

- [ ] **Step 1: RED.** protocol: round-trip a toy `#[derive(Serialize,Deserialize)]` enum through `encode_cbor_frame`/`CborFrameDecoder` incl. split-buffer push and oversize reject; existing framing tests unchanged. daemon-core: `server_handshake` over a socketpair — compatible range → `Some(chosen)` + client reads Accepted; disjoint → `None` + client reads Incompatible + subsequent read = EOF after caller closes; garbage magic → `Err`; stalled client → `Err` within timeout. sessiond: full crate tests must stay green after re-seat. FAIL first.
- [ ] **Step 2: GREEN.** Implement; re-seat sessiond accept path on `server_handshake(DAEMON_MIN_VERSION, DAEMON_MAX_VERSION, build)`.
- [ ] **Step 3: Phase-1 gate.** `bash scripts/final-suite.sh` → `ALL GATES PASSED`. Commit `refactor(protocol,daemon-core): generic CBOR framing + extracted server_handshake; sessiond re-seated (S3 §4.1) — phase-1 gate green`.

## Phase 2 — orchd skeleton

### Task 4: `bpa-orchd-proto` crate

**Files:** Create `crates/orchd-proto/Cargo.toml` (name `bpa-orchd-proto`; deps `serde`, `ts-rs` matching `bpa-protocol`), `crates/orchd-proto/src/lib.rs`, `crates/orchd-proto/tests/ts_export.rs` (mirror `bpa-protocol`'s ts_export test targeting `src/ipc/orchd-types.ts`), `crates/orchd-proto/tests/roundtrip.rs`. Modify root `Cargo.toml` (member), `scripts/final-suite.sh` ts-parity stage (regen must include orchd-types.ts diff check).
**Interfaces — Produces:** ALL spec §4.2 types VERBATIM (entities, `OrchdErrorCode`, `OrchdRequest`, `OrchdResponse`, `OrchdPush`, `OrchdFrame`, `PolicyRules`, `RuleSetView`, `RuleFileState`) + `pub const ORCHD_CLIENT_MIN_VERSION: u16 = 1;` `ORCHD_CLIENT_MAX_VERSION = 1`, `ORCHD_DAEMON_MIN_VERSION = 1`, `ORCHD_DAEMON_MAX_VERSION = 1` + `pub fn encode_orchd_frame(&OrchdFrame)` / `pub type OrchdFrameDecoder = CborFrameDecoder<OrchdFrame>` re-exports over the T3 generics. Struct serde: `rename_all = "camelCase"`; enums: `rename_all = "camelCase"` tags (`in_dev` etc. per §5.1 CHECK literals — assert exact strings in tests); `Update*` double-Option fields get `#[serde(default, skip_serializing_if = "Option::is_none")]`.

- [ ] **Step 1: RED.** roundtrip.rs: CBOR encode/decode one instance of EVERY Request/Response/Push variant (construct with non-default field values); serde-string assertions: `IdeaLifecycle::InDev` ⇒ `"inDev"` on the wire but DB literal mapping is persistence's job (test only wire); `TaskStatus::Backlog` ⇒ `"backlog"`; double-option absent vs null distinguishable on `UpdateIdea.project_id`. ts_export: generated file contains `DomainTask`, `OrchdPush`, `bundleFormat`-relevant entity fields camelCased. FAIL (crate empty).
- [ ] **Step 2: GREEN.** Write the crate; run ts export; commit the generated `src/ipc/orchd-types.ts`.
- [ ] **Step 3:** `cargo test -p bpa-orchd-proto` + second regen `git diff --exit-code src/ipc/orchd-types.ts` → PASS. Commit `feat(orchd-proto): S3 wire contract — entities + frames [1,1] + ts-rs export (spec §4.2, frozen append-only)`.

### Task 5: `bpa-orchd` daemon skeleton

**Files:** Create `crates/orchd/Cargo.toml` (name `bpa-orchd`, bin `bpa-orchd`; deps: `bpa-daemon-core`, `bpa-orchd-proto`, `bpa-protocol`, `tokio`, `rusqlite`, `serde`, `serde_json`, `uuid`, `sha2`, `tracing`, `libc` — versions matching sessiond), `crates/orchd/src/{main.rs,lib.rs,boot.rs,socket_server.rs,persistence.rs}`, `crates/orchd/tests/boot_integration.rs`. Modify root `Cargo.toml` (member).
**Interfaces — Produces:** `bpa_orchd::run(socket: PathBuf, shutdown_tx: watch::Sender<bool>, shutdown_rx: watch::Receiver<bool>) -> anyhow-style Result` (mirror sessiond `run` shape exactly); socket file `orchd.sock`, lockfile `orchd.lock` (via daemon-core `resolve_*("orchd.sock")`), DB `{app_support}/orchd.db`, log `{app_support}/logs/orchd.log`; `persistence::Db::{open(&Path), open_in_memory()}` applying spec §5.1 DDL VERBATIM as migration step 0→1 via `daemon_core::migrate` + `PRAGMA foreign_keys=ON` + WAL in `open_inner`; boot ensures the GLOBAL ruleset row + `{app_support}/rules/global.md` (template `# Глобальные правила\n`, `policy='{}'`) idempotently; socket_server: accept → `server_handshake(ORCHD_DAEMON_MIN_VERSION, ORCHD_DAEMON_MAX_VERSION, build)` → dispatch loop handling `Ping` → `Pong` and `OrchdShutdown{drain}` → Ack + shutdown watch flip (drain: flush WAL checkpoint) — all other requests answer `Error{code: Validation, message: "not implemented"}` until T10 replaces the stub arm. Broadcaster: copy sessiond's push-forwarder pattern (per-connection mpsc + overflow disconnect).
**Launchd:** create `crates/orchd/launchd/ai.builderpro.orchd.plist` mirroring the sessiond plist (label `ai.builderpro.orchd`, program args → installed binary path, KeepAlive) — installation is wired in T11.

- [ ] **Step 1: RED.** boot_integration.rs (copy sessiond's HOME-isolation HomeGuard pattern + use `ORCHD_CLIENT_MIN/MAX_VERSION` consts in the test preamble — NEVER literals): boot on temp socket → handshake accepted `chosen==1` → Ping→Pong → clean shutdown unlinks socket; second-instance flock refusal; fresh DB has `user_version=1` + all §5.1 tables + `foreign_keys` effective (insert violating row → constraint error) + global ruleset row exists + `rules/global.md` on disk; double boot doesn't duplicate the global row. FAIL.
- [ ] **Step 2: GREEN.** Implement skeleton.
- [ ] **Step 3:** `cargo test -p bpa-orchd` → PASS; clippy/fmt clean. Commit `feat(orchd): daemon skeleton — boot/singleton/socket [1,1] + orchd.db schema v1 + global ruleset ensure (S3 §5)`.

## Phase 3 — domain

### Task 6: persistence — project + workspace links + goals

**Files:** Modify `crates/orchd/src/persistence.rs`.
**Interfaces — Produces (all `Result<_, OrchdPersistError>`; `pub enum OrchdPersistError { NotFound, Invariant(String), Conflict(String), Validation(String), Sql(rusqlite::Error) }` mapping 1:1 to `OrchdErrorCode` later):** `create_project(name: &str, description: &str, workspace_ids: &[String]) -> Result<Project>` (ONE tx: project row + links + strategic goal `title="Стратегическая цель"` + ruleset row `md_path={app_support}/rules/project-<id>.md` — §5.2 auto-create; empty `workspace_ids` ⇒ `Invariant`; duplicate ws ⇒ `Conflict`); `update_project(id, name: Option<&str>, description: Option<&str>) -> Result<Project>`; `archive_project(id) -> Result<Project>`; `list_projects() -> Result<Vec<Project>>` (workspace_ids joined ordered by `ord`); `add_project_workspace(project_id, workspace_id) -> Result<Project>`; `remove_project_workspace(project_id, workspace_id) -> Result<Project>` (last link ⇒ `Invariant`); `create_goal(project_id, parent_id: Option<&str>, kind: GoalKind, title, body) -> Result<Goal>` (second strategic ⇒ `Invariant`; cross-project parent ⇒ `Invariant`); `update_goal(id, title/body/status/metric_refs Options) -> Result<Goal>`; `move_goal(id, new_parent_id: Option<&str>, new_ord: i64) -> Result<Goal>` (strategic ⇒ `Invariant`; cycle walk-up ⇒ `Invariant`); `delete_goal(id) -> Result<()>` (strategic ⇒ `Invariant`; cascade via FK); `list_goals(project_id) -> Result<Vec<Goal>>` (ORDER BY parent_id NULLS FIRST, ord). Archived-project guard: every mutating fn checks project status first ⇒ `Invariant("project archived")`.

- [ ] **Step 1: RED.** In-memory-DB tests for every behavior above, including: create_project returns strategic goal present via list_goals; workspace UNIQUE across projects ⇒ `Conflict`; remove-last-workspace ⇒ `Invariant`; goal cycle (reparent a goal under its own descendant) ⇒ `Invariant`; goal subtree delete cascades grandchildren; metric_refs round-trips JSON; archived project blocks create_goal. FAIL.
- [ ] **Step 2: GREEN.** Implement.
- [ ] **Step 3:** `cargo test -p bpa-orchd persistence` → PASS. Commit `feat(orchd): project/workspace-link/goal persistence + §5.2 invariants (S3 §5.2, TDD)`.

### Task 7: persistence — ideas + insights + tasks

**Files:** Modify `crates/orchd/src/persistence.rs`.
**Interfaces — Produces:** `create_idea(project_id: Option<&str>, title, body) -> Result<Idea>`; `update_idea(id, title/body Options, project_id: Option<Option<&str>>) -> Result<Idea>`; `set_idea_lifecycle(id, IdeaLifecycle) -> Result<Idea>`; `delete_idea(id)`; `list_ideas(project_id: Option<&str>) -> Result<Vec<Idea>>` (None ⇒ all, newest-first `created_at DESC`); insight analogues (`create_insight(project_id, source, title, body)`, `update_insight(id, title/body/fit_verdict(double-opt)/fit_reasoning)`, `set_insight_status(id, status, resolution_reasoning: Option<&str>)`, `delete_insight`, `list_insights`); `create_task(project_id, parent_id: Option<&str>, title, body, status: Option<TaskStatus>, source: TaskSource, source_id: Option<&str>, tags: &[String]) -> Result<DomainTask>` (rank = `SELECT COALESCE(MAX(rank),0)+1024`; parent cross-project/cycle ⇒ `Invariant`); `update_task(id, title/body/tags Options)`; `set_task_status(id, TaskStatus)`; `set_task_rank(id, f64)`; `delete_task(id)` (subtask cascade); `list_tasks(project_id: Option<&str>) -> Result<Vec<DomainTask>>` (ORDER BY rank). Enum⇄TEXT mapping helpers assert §5.1 CHECK literals exactly (`in_dev`, `no_fit`).

- [ ] **Step 1: RED.** Tests: lifecycle/status literals hit the DB as §5.1 strings (query raw TEXT); orphan idea (project_id None) listed under None but not under a project; rank sequence 1024/2048/3072; set_task_rank midpoint persists f64; task cycle + cross-project parent ⇒ `Invariant`; delete_task cascades subtasks; double-option update clears vs keeps project_id. FAIL.
- [ ] **Step 2: GREEN + Step 3:** `cargo test -p bpa-orchd persistence` → PASS. Commit `feat(orchd): idea/insight/task persistence — rank math, cycles, cascade (S3 §5.2)`.

### Task 8: ruleset files + ruleset persistence

**Files:** Create `crates/orchd/src/ruleset_files.rs`; modify `crates/orchd/src/persistence.rs` (ruleset queries), `crates/orchd/src/boot.rs` (global-ensure re-seated on these fns if not already).
**Interfaces — Produces (spec §7):** `ruleset_files::write_atomic(path: &Path, content: &str) -> io::Result<String>` (create parent dirs; write `path.with_extension("tmp")` then rename; returns sha256 hex); `ruleset_files::read_state(path: &Path, stored_hash: &str) -> (Option<String>, RuleFileState)` (missing ⇒ `(None, Missing)`; hash equal ⇒ `Ok`; differs ⇒ `ExternallyModified` with content); persistence: `get_ruleset(scope, project_id: Option<&str>) -> Result<RuleSet>`; `upsert_ruleset(scope, project_id, md_content: Option<&str>, md_path: Option<&str>, policy: Option<&PolicyRules>) -> Result<RuleSet>` (md_path: absolute + parent exists else `Validation`; md_content: write_atomic + store new hash; policy: strict-validate — `serde_json` deny_unknown_fields struct, `spend_cap_usd >= 0`, allowlist entries non-empty else `Validation`); `acknowledge_rule_file(id) -> Result<RuleSet>` (re-read file, store hash; missing ⇒ `Invariant("file missing")`).

- [ ] **Step 1: RED.** Tests (tempdir): write_atomic creates dirs + no `.tmp`残 left + returns hash of exact bytes; read_state three states; upsert with content updates hash; repoint to relative path ⇒ `Validation`; policy with unknown key ⇒ `Validation`; negative cap ⇒ `Validation`; acknowledge after external edit returns rule with new hash; content string never appears in the log file (reuse sessiond's no-secrets log-capture test pattern with a planted marker). FAIL.
- [ ] **Step 2: GREEN + Step 3:** `cargo test -p bpa-orchd` → PASS. Commit `feat(orchd): ruleset file layer — files-as-truth, atomic writes, sha256 states (S3 §7, D4)`.

### Task 9: export / import

**Files:** Create `crates/orchd/src/export.rs`; modify `crates/orchd/src/persistence.rs` (raw-insert helpers used inside the import tx).
**Interfaces — Produces (spec §8):** `export_project(db, ruleset_dir_guard: &Path, project_id) -> Result<String>` (JSON per §8 shape; ruleset `mdContent` read live, missing file ⇒ `"mdMissing": true` + `mdContent: null`); `export_all(db, …) -> Result<String>` (projects array + `globalRuleset` + `orphanIdeas` + `orphanInsights`); `import_bundle(db, app_support: &Path, json: &str) -> Result<ImportCounts>` — parse (unknown `bundleFormat` ⇒ `Validation`), ONE tx raw-inserting all rows with original ids (`Conflict("<entity> <id> already exists")` on any PK/UNIQUE hit, full rollback); ruleset md files written ONLY when `md_path` starts with `app_support` — otherwise write to the default app-support path and repoint (spec §8 containment rule); accepts both the per-project bundle and the whole-store shape (discriminate on `project` vs `projects` key).

- [ ] **Step 1: RED.** Tests: build a project with 2-level goals, orphan idea, subtask, ruleset with content → export_all → wipe (fresh in-memory DB) → import → export_all again → `serde_json::Value` equality after deleting `exportedAt` keys; import into non-empty store with one colliding task id ⇒ `Conflict` AND zero new rows (counts unchanged); foreign md_path bundle lands under app-support; `bundleFormat: 2` ⇒ `Validation`; per-project bundle imports standalone. FAIL.
- [ ] **Step 2: GREEN + Step 3:** `cargo test -p bpa-orchd export` → PASS. Commit `feat(orchd): export/import — bundleFormat 1, tx-atomic, Conflict on collision, path containment (S3 §8, D7)`.

### Task 10: socket dispatch + pushes

**Files:** Modify `crates/orchd/src/socket_server.rs` (replace the T5 stub arm with full dispatch).
**Interfaces — Consumes:** T6–T9 persistence/export fns. **Produces:** every `OrchdRequest` variant dispatched per spec §4.2: mutating verbs reply the updated entity (`Ack` for deletes/imports' shutdown) AND broadcast the matching `OrchdPush` (spec §4.2 table: project verbs ⇒ `ProjectsChanged`; goal ⇒ `GoalsChanged{project_id}`; idea ⇒ `IdeasChanged`; insight ⇒ `InsightsChanged`; task ⇒ `TasksChanged{project_id}`; ruleset ⇒ `RuleSetChanged{scope, project_id}`; `ImportBundle` ⇒ ALL of the above pushes for touched families); `GetRuleSet` assembles `RuleSetView` via `ruleset_files::read_state`; `OrchdPersistError` → `Response::Error{code}` mapping (`NotFound→NotFound`, `Invariant→Invariant`, `Conflict→Conflict`, `Validation→Validation`, `Sql→Io`).

- [ ] **Step 1: RED.** Socket-level tests (stub client over `run()`, orchd version consts): CreateProject → `Response::Project` with workspace_ids + second connection receives `ProjectsChanged`; CreateGoal → goal + `GoalsChanged` carrying the right project_id; remove-last-workspace → `Error{Invariant}` and NO push; GetRuleSet returns `file_state: Ok` then, after an on-disk edit, `ExternallyModified`; ImportBundle happy path → `ImportReport` counts + pushes observed; unknown-id delete → `Error{NotFound}`. FAIL.
- [ ] **Step 2: GREEN + Step 3:** `cargo test -p bpa-orchd` → PASS (full crate). Commit `feat(orchd): full domain dispatch + coarse invalidation pushes (S3 §4.2/§6, D6)`.

## Phase 4 — core integration

### Task 11: orchd client + launchd parameterization

**Files:** Create `src-tauri/src/orchd_client.rs` (mirror `src-tauri/src/socket_client.rs` structure 1:1 — read it first); modify `src-tauri/src/launchd.rs` (parameterize label/plist/binary so both `ai.builderpro.sessiond` and `ai.builderpro.orchd` install/kickstart через one code path; sessiond call sites keep current behavior), `src-tauri/src/lib.rs` (managed state slot for the orchd client, mirror the sessiond slot).
**Interfaces — Produces:** `OrchdClient::connect(socket: &Path) -> Result<OrchdClient, OrchdConnectError>` (client preamble `[ORCHD_CLIENT_MIN_VERSION, ORCHD_CLIENT_MAX_VERSION]`; `OrchdConnectError::Incompatible{min,max}` typed fatal — never auto-retried); `request(req: OrchdRequest) -> Result<OrchdResponse, …>` (id-correlated); push subscription channel surfaced to the broker; launchd bootstrap-on-first-connect for the orchd plist (same flow sessiond uses today); connection-state notifications for down/up.

- [ ] **Step 1: RED.** commands_over_stub_daemon-style tests with a stub orchd (tokio task speaking the wire): connect+handshake ok; request/response correlation under two in-flight requests; incompatible stub ⇒ `Incompatible` typed; push received on the subscription channel; connect refusal ⇒ typed retryable error distinct from `Incompatible`. FAIL.
- [ ] **Step 2: GREEN + Step 3:** `cargo test -p builder-pro-ai` → PASS. Commit `feat(core): orchd client — preamble [1,1], correlation, typed Incompatible; launchd dual-label (S3 §9)`.

### Task 12: orchd commands + broker events

**Files:** Modify `src-tauri/src/commands.rs` (orchd_* wrappers), `src-tauri/src/broker.rs` (push→event mapping + consts), `src-tauri/src/lib.rs` (register commands).
**Interfaces — Produces (spec §9, locked):** event consts `EV_ORCHD_PROJECTS_CHANGED = "orchd://projects-changed"`, `EV_ORCHD_GOALS_CHANGED = "orchd://goals-changed"` (payload `{ projectId }`), `EV_ORCHD_IDEAS_CHANGED`, `EV_ORCHD_INSIGHTS_CHANGED`, `EV_ORCHD_TASKS_CHANGED` (payload `{ projectId }`), `EV_ORCHD_RULESET_CHANGED` (payload `{ scope, projectId? }`), `EV_ORCHD_DOWN = "orchd://down"`, `EV_ORCHD_UP = "orchd://up"`, `EV_ORCHD_INCOMPATIBLE = "orchd://incompatible"`; `#[tauri::command]`s one per §4.2 verb, names = snake_case of the verb prefixed `orchd_` (`orchd_create_project`, `orchd_list_goals`, `orchd_upsert_ruleset`, `orchd_export_all`, `orchd_import_bundle`, …), each returning the ts-rs entity type or `CommandError` (map `OrchdErrorCode` into the existing `CommandError::Daemon{code, message}` shape with code strings `"NotFound" | "Invariant" | "Validation" | "Conflict" | "Io"`).

- [ ] **Step 1: RED.** Stub-orchd tests: `orchd_create_project` happy + `Invariant` error mapping surfaces code string; broker unit test: each `OrchdPush` variant maps to its const + payload camelCase. FAIL.
- [ ] **Step 2: GREEN + Step 3:** `cargo test -p builder-pro-ai` → PASS. Commit `feat(core): orchd_* commands + orchd:// broker events (S3 §9)`.

## Phase 5 — frontend

### Task 13: frontend IPC + domain store slice

**Files:** Create `src/ipc/orchd.ts` (+ `src/ipc/orchd.test.ts`); modify `src/ipc/events.ts` (on-handlers for all §9 events), `src/store/store.ts` + `src/store/store.test.ts` (domain slice).
**Interfaces — Consumes:** `src/ipc/orchd-types.ts` (T4 generated). **Produces (locked):** `orchd.ts` typed `invoke` wrappers matching T12 command names/args verbatim; store: `view: "home" | "workspace" | "project"` (extend S2 union), `activeProjectId: string | null`, `projects: Project[]`, `goalsByProject: Record<string, Goal[]>`, `ideas: Idea[]`, `insights: Insight[]`, `tasksByProject: Record<string, DomainTask[]>`, `rulesets: Record<string, RuleSetView>` (key `` `global` `` / `` `project:${id}` ``), `orchdDown: boolean`, `orchdIncompatible: boolean`; actions: `refreshProjects/refreshGoals(projectId)/refreshIdeas/refreshInsights/refreshTasks(projectId)/refreshRuleset(key)` (fetch+replace), `openProject(id)` (sets view+activeProjectId), event bindings: each `orchd://*-changed` calls the matching refresh (goals/tasks only for the payload's projectId), `orchd://down|up` flips `orchdDown`, `orchd://incompatible` sets flag.

- [ ] **Step 1: RED.** store tests: openProject flips view; goals-changed event refreshes only the named project's goals (mock ipc); orchdDown true blocks nothing in state but is readable; rulesets keying. ipc test: wrapper passes exact command name + camelCase args (mock `invoke`). FAIL.
- [ ] **Step 2: GREEN + Step 3:** `npx vitest run store orchd` + `npx tsc --noEmit` → PASS. Commit `feat(ui): orchd ipc + domain store slice + project view state (S3 §10)`.

### Task 14: GoalTree component

**Files:** Create `src/components/GoalTree.tsx` + `src/components/GoalTree.test.tsx`; modify `docs/design-system.md` (tree-row atom).
**Interfaces — Consumes:** T13 slice (`goalsByProject`, refreshGoals) + `orchd.ts` (`orchdCreateGoal`, `orchdUpdateGoal`, `orchdMoveGoal`, `orchdDeleteGoal`). **Produces:** `<GoalTree projectId={string} />` — indent tree built from parent_id (strategic root pinned first, non-deletable, non-movable); per-row: title inline-edit, status select (active/achieved/dropped), «+ подцель», delete-with-confirm («удалить ветку целиком?»), ord up/down within siblings (`orchdMoveGoal` same parent, swapped ord); errors → toast via `describeOrchdError` helper (`Invariant`⇒«недопустимая операция: …» etc. — create the helper here in `src/ipc/orchd.ts`).

- [ ] **Step 1: RED.** Tests: renders 3-level tree with correct indent order; strategic row lacks delete/move controls; add-subgoal calls create with parent_id; delete confirm gates the call; move up swaps ord args; Invariant error shows toast. FAIL.
- [ ] **Step 2: GREEN + Step 3:** vitest + tsc PASS. Commit `feat(ui): goal tree editor — pinned strategic root, subtree ops (S3 §10, D5)`.

### Task 15: IdeasList + InsightsList

**Files:** Create `src/components/IdeasList.tsx`, `src/components/IdeasList.test.tsx`, `src/components/InsightsList.tsx`, `src/components/InsightsList.test.tsx`; modify `docs/design-system.md` (lifecycle-chip atom row).
**Interfaces — Consumes:** T13 slice + wrappers. **Produces:** `<IdeasList projectId={string | null} />` (null ⇒ orphan view for Home use later; list newest-first, lifecycle chip per row cycling through the §4.2 enum via a select, create form title+body, edit inline, delete confirm, «привязать к проекту» select on orphan rows → `orchdUpdateIdea` project_id); `<InsightsList projectId={string | null} />` (fit_verdict badge fit/no_fit/unknown/—, status select new/accepted/archived — archiving requires non-empty `resolution_reasoning` prompt (spec: archived-with-reasoning), source shown as caption).

- [ ] **Step 1: RED.** Tests: lifecycle chip renders current value + select fires SetIdeaLifecycle; archive without reasoning blocked with inline message; orphan idea attach fires double-option correctly (`{ projectId: id }` present). FAIL.
- [ ] **Step 2: GREEN + Step 3:** vitest + tsc PASS. Commit `feat(ui): ideas + insights lists — lifecycle chips, archive-with-reasoning (S3 §10)`.

### Task 16: TasksList

**Files:** Create `src/components/TasksList.tsx` + `src/components/TasksList.test.tsx`.
**Interfaces — Consumes:** T13 slice + wrappers. **Produces:** `<TasksList projectId={string} />` — rows grouped by the six statuses in §4.2 order, ordered by rank inside a group; subtask rows indented under parent; status select per row; rank ▲/▼ = `orchdSetTaskRank(id, midpoint)` where midpoint = `(prevRank + prevPrevRank)/2` style fractional math against the neighbor list (top ⇒ `firstRank - 1024`, bottom ⇒ `lastRank + 1024`); create dialog: title, body, source select (idea/insight/bug/plan), optional parent select, tags input (comma-split); delete cascades warning when the task has subtasks («удалит N подзадач»).

- [ ] **Step 1: RED.** Tests: grouping+order by rank; ▲ on second row computes midpoint between neighbors and calls SetTaskRank; top-move uses firstRank-1024; subtask indent; cascade warning counts children; create passes source correctly. FAIL.
- [ ] **Step 2: GREEN + Step 3:** vitest + tsc PASS. Commit `feat(ui): task list — status groups, fractional rank moves, subtasks (S3 §10, Q9/Q11 defaults)`.

### Task 17: RulesetPanel

**Files:** Create `src/components/RulesetPanel.tsx` + `src/components/RulesetPanel.test.tsx`; modify `docs/design-system.md` (policy-form + file-state-banner atom rows).
**Interfaces — Consumes:** T13 slice (`rulesets`) + wrappers (`orchdGetRuleset`, `orchdUpsertRuleset`, `orchdAcknowledgeRuleFile`) + existing `reveal_in_finder` (S2 fs command) for «открыть файл». **Produces:** `<RulesetPanel scope={"global" | "project"} projectId={string | null} />` — md content textarea (save ⇒ `orchdUpsertRuleset{ mdContent }`), path row + «открыть файл» (reveal via absolute path — call reveal with the md_path's parent as root and file name as rel? NO: md_path is app-support-absolute, outside workspace roots — add a dedicated thin `orchd_reveal_rules_file` command? — no new daemon verb: use Tauri `opener` already in src-tauri (S2) via a small `reveal_absolute(path)` command added HERE in `src-tauri/src/commands.rs` guarded to `{app_support}/rules/` or the stored md_path value, unit-tested); `file_state` banners: `ExternallyModified` ⇒ amber-free info banner + [Принять] (`orchdAcknowledgeRuleFile`), `Missing` ⇒ banner + [Создать заново] (`orchdUpsertRuleset{ mdContent: "" }`); policy form: spend cap number input (empty=null), approval classes chips input, path allowlist rows — client-side mirrors server validation, server `Validation` errors surface verbatim in the toast.

- [ ] **Step 1: RED.** Tests: three file_state renderings; Принять calls acknowledge; save passes mdContent; policy negative cap blocked client-side; reveal command called with stored path; `reveal_absolute` rust unit test rejects a path outside app-support AND not equal to the row's md_path. FAIL.
- [ ] **Step 2: GREEN + Step 3:** vitest + tsc + `cargo test -p builder-pro-ai` PASS. Commit `feat(ui): ruleset panel — files-as-truth states, policy form (S3 §7/§10)`.

### Task 18: ProjectPanel + left-rail restructure + App wiring

**Files:** Create `src/components/ProjectPanel.tsx` + test, `src/components/CreateProjectDialog.tsx` + test; modify `src/components/WorkspaceSidebar.tsx` (project groups), `src/App.tsx` (view === "project" branch + orchd event subscriptions + initial refreshProjects), `docs/design-system.md` (project-group-row atom).
**Interfaces — Consumes:** T13–T17 components/slice. **Produces:** `ProjectPanel` tabs Обзор/Цели/Идеи/Задачи/Инсайты/Правила (Цели ⇒ `<GoalTree/>`, Идеи ⇒ `<IdeasList projectId/>`, Задачи ⇒ `<TasksList/>`, Инсайты ⇒ `<InsightsList/>`, Правила ⇒ `<RulesetPanel scope="project"/>`); Обзор tab: entity counters, linked workspaces list with unresolvable-id chip «workspace недоступен» + [Отвязать] (resolve against the S2 store's workspaces), add-workspace select of unlinked workspaces, export button (`orchdExportProject` → save dialog via existing Tauri fs plugin? — NO new plugin: emit the JSON to a `showSaveDialog`-less flow: write via existing `create_file`/download? Simplest honest v1: copy-to-clipboard button + toast «JSON скопирован» AND a `orchd_export_to_file(project_id, dest_dir)` Tauri command writing `<name>-export.json` into a user-picked dir via the EXISTING `pickFolder` (S2) — lock this flow), import button (pickFolder→ file select via `list_dir`? — lock: `orchd_import_from_file(path)` command + `pickFolder`+filename input is clunky; instead reuse existing `pickFolder` for dir + `list_dir` file picker UI inside a small dialog — implementer follows this exact flow); sidebar: project groups with nested workspaces (S2 rows unchanged inside), «Без проекта» group + [привязать] per row (select existing project → `orchdAddProjectWorkspace`), «+ проект» → `CreateProjectDialog` (name, description, multi-select of unlinked workspaces + «создать workspace» inline via existing S2 `createWorkspace` flow, then `orchdCreateProject`); App: view branch renders `<ProjectPanel/>`, subscribes all `orchd://*` events once (mirror S2 subscription effect), initial `refreshProjects()` after connect.

- [ ] **Step 1: RED.** Tests: sidebar groups workspaces by project + Без проекта remainder; create dialog blocks submit with zero workspaces (Invariant pre-empted client-side with «нужен хотя бы один workspace»); ProjectPanel tab switching renders each child (smoke with mocked children); unresolvable workspace chip + Отвязать calls remove; export copies JSON (clipboard mock) and export-to-file command invoked with picked dir; App renders ProjectPanel on view=project. FAIL.
- [ ] **Step 2: GREEN + Step 3:** vitest + tsc + cargo (for the two new file commands) PASS. Commit `feat(ui): project panel + rail project groups + create/link flows + export-import UI (S3 §10)`.

### Task 19: QuickCapture (⌘K) + HomeGoals + orchd honesty surfaces

**Files:** Create `src/components/QuickCapture.tsx` + test, `src/components/HomeGoals.tsx` + test; modify `src/App.tsx` (global keydown, banners, upgrade dialog wiring), `src/components/HomeView.tsx` (mount HomeGoals above the attention queue), `docs/design-system.md` (quick-capture overlay atom).
**Interfaces — Consumes:** T13 slice, T12 events. **Produces:** ⌘K (metaKey+K, ignored while an input/textarea/xterm is focused) opens overlay: title (required), body, project select from `projects` + «без проекта» ⇒ `orchdCreateIdea` → success toast «идея сохранена», orchdDown ⇒ submit disabled + inline «оркестратор недоступен»; `HomeGoals`: one block per active project — strategic goal title + its direct `additional` children with status chips, click navigates `openProject(id)` (Цели tab default); orchd-down banner: single shared `<OrchdDownBanner/>` inline in ProjectPanel/HomeGoals/QuickCapture areas with [Повторить] → a `orchd_reconnect` command (added in T11's client as a thin retry entry — if absent, add here); `orchdIncompatible` ⇒ reuse S2 `UpgradeDialog` pattern with orchd copy («Обновить фоновый сервис оркестратора — записи сохранены») → kickstart via the T11 dual-label launchd + `app.restart()`.

- [ ] **Step 1: RED.** Tests: ⌘K toggles overlay, ignored when input focused; submit fires CreateIdea with null project; HomeGoals renders strategic+children per project and навигирует по клику; down-banner Повторить calls reconnect; incompatible flag renders dialog. FAIL.
- [ ] **Step 2: GREEN + Step 3:** vitest + tsc PASS. Commit `feat(ui): ⌘K idea capture + Home goals panel + orchd honesty surfaces (S3 §10/§11, D3)`.

## Phase 6 — close

### Task 20: e2e + gate extension

**Files:** Create `e2e/orchd-survive.mjs` (mirror the existing e2e script's structure — read `e2e/` current script first); modify `package.json` (`"e2e:orchd": "node e2e/orchd-survive.mjs"`), `scripts/final-suite.sh` (coverage stage adds `bpa-orchd ≥ 80%` mirroring the sessiond block; new stage `== 9/9 e2e orchd survive+roundtrip ==` running `npm run e2e:orchd`; renumber stage headers).
**Interfaces — Consumes:** built `bpa-orchd` binary, orchd-proto wire. **Produces (spec §12 script, exact phases):** phase0 boot on temp HOME → handshake `[1,1]`; phase1 CreateProject(+2 goals via CreateGoal, 1 idea, 1 task) asserts entities; phase2 `OrchdShutdown{drain:true}` → relaunch → ListProjects/ListGoals intact; phase3 `ExportAll` captured; phase4 shutdown → delete `orchd.db*` → relaunch (fresh v1) → `ImportBundle` → `ExportAll` deep-equal phase3 modulo `exportedAt`; each phase logs `[e2e-orchd] phaseN OK`.

- [ ] **Step 1:** Write script; run `npm run e2e:orchd` → `ALL PHASES PASSED`.
- [ ] **Step 2:** Extend final-suite; run `bash scripts/final-suite.sh` → `ALL GATES PASSED` (9 stages; fix coverage shortfalls in-scope by adding missing orchd unit tests, not by lowering the bar).
- [ ] **Step 3:** Commit `test(e2e): orchd survive-restart + export/import round-trip; gate → 9 stages incl. orchd coverage (S3 §12)`.

### Task 21: docs truth + CHANGELOG

**Files:** Create `docs/runbook-orchd.md` (mirror `docs/runbook-daemon.md` structure: label `ai.builderpro.orchd`, socket/lock/db/log paths, kickstart/bootstrap commands, upgrade choreography note). Modify `docs/architecture.md` (two-daemon topology + domain store section), `docs/superpowers/specs/2026-07-01-builderpro-platform-overview.md` (§3 S3 row → **SHIPPED/DONE** + deltas: daemon-core extraction (D2), extended UI pulled ⌘K/HomeGoals forward (D3), files-as-truth ruleset (D4); «Current slice» line → next S4 ∥ S-EXT ∥ S5), `README.md` (features + test counts + two-daemon mention), `docs/traceability.md` (S3 rows), `docs/backlog.md` (add: «un-archive project verb (additive)», «orchd Keychain unattended access — routed to S-EXT», close any BL rows S3 resolved), `CHANGELOG.md` (`[0.4.0]`: bpa-orchd daemon, domain schema v1, six entity families CRUD, project rail/panel UI, ⌘K capture, Home goals, export/import, daemon-core extraction), `docs/design-system.md` sweep (all new atom rows present — verify against T14–T19).

- [ ] **Step 1:** All edits; sanity `git grep -n 'orchd' docs/ | grep -i 'todo\|tbd'` → empty.
- [ ] **Step 2:** `bash scripts/final-suite.sh` → `ALL GATES PASSED`.
- [ ] **Step 3:** Commit `docs: S3 shipped — two-daemon topology, runbook-orchd, CHANGELOG [0.4.0]`.

### Task 22: whole-branch adversarial review + merge

- [ ] **Step 1:** `scripts/review-package $(git merge-base main HEAD) HEAD`; multi-lens adversarial review (lenses: sessiond-regression-from-extraction (diff every re-seated path), wire-contract drift (proto enums vs spec §4.2 verbatim + version consts at call sites), SQL/invariant honesty (§5.2 table vs tests), ruleset file safety (atomic write, containment, no-content-in-logs), import tx atomicity + collision, frontend honesty (down-banner coverage, double-option updates, rank math), cross-daemon isolation (orchd failure never touches terminal flows)). Verify → fix waves → re-gate (Pv2/S2 T15 pattern).
- [ ] **Step 2:** `superpowers:finishing-a-development-branch` — verify gate, present options; on merge: ff-merge to main, re-run gate on main, push origin.

---

## Self-review (done at write time)

Spec coverage: §2.1 crates→T1–T5; §2.2 phases→graph; §3 extraction table→T1(dirs/singleton/logging) T2(migrate) T3(handshake); §4.1→T3; §4.2→T4 (verbatim) + T10 dispatch; §5/§5.1 DDL→T5; §5.2 invariants→T6/T7 tests + §13 un-archive backlog→T21; §7→T8+T17; §8→T9+T18 UI; §9→T11/T12; §10→T13–T19 (rail T18, ⌘K+HomeGoals T19, view union T13); §11 matrix→T10 errors, T17 banners, T18 chip, T19 down/incompatible; §12→T20 + per-task TDD; §13→T21; §14 honored (no scheduler/enforcement/kanban tasks). Type consistency: `DomainTask`/`OrchdErrorCode`/`ORCHD_*_VERSION`/event consts/命名 `orchd_*` commands used consistently across T4/T10/T12/T13; `RuleSetView.file_state` naming matches §4.2. No placeholders; two flows deliberately locked in-plan where the spec was UI-silent (export-to-file via pickFolder; import file picker via list_dir dialog). Parallel group {T14,T15,T16,T17} shares no files (design-system.md edits are per-task distinct rows — merge-trivial, acceptable; if conflicts bite, controller serializes doc edits).
