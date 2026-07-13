# S3 — `bpa-orchd` + App-Domain Foundation: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship the second launchd daemon `bpa-orchd` (final ADR-HOST topology) with domain schema v1 + full CRUD for Projects/Goals/Ideas/Insights/Tasks/RuleSet, and the owner-facing UI (project rail groups, goal tree, ideas inbox + ⌘K capture, task list, ruleset editor, export/import, Home goals panel).

**Architecture:** Phase 1 extracts shared `bpa-daemon-core` (six modules) and re-seats `bpa-sessiond` on it (byte-identical behavior + on-disk names, full gate = phase boundary). Phase 2 boots the orchd skeleton (own socket/DB/launchd label, preamble `[1,1]`). Phase 3 lands domain persistence + dispatch + ruleset files + export/import. Phases 4–5 wire core client/commands and the frontend. Spec is authoritative: `docs/superpowers/specs/2026-07-13-s3-orchd-domain-foundation-design.md` (D1–D12, wire enums §4.2, DDL §5.1, invariants §5.2, verified extraction table §3).

**Tech Stack:** existing workspace only — `rusqlite`, `ciborium`, `ts-rs`, `uuid`, `sha2`, `tokio`, `thiserror`. **D9: zero new external dependencies**; `sha2` gets a `[workspace.dependencies]` entry (it is transitive-only today; `uuid` is already a workspace dep used by sessiond).

## Global Constraints

- Spec §4.2 wire enums copied VERBATIM (frozen append-only); orchd version space `[1,1]` via `ORCHD_CLIENT_MIN/MAX_VERSION` / `ORCHD_DAEMON_MIN/MAX_VERSION` — **consts at every call site incl. tests and stubs, never literals** (the S2 wave-E lesson, commit 2f42e95; note: the existing sessiond stub replies a literal `chosen: 2` — do NOT copy that bug into orchd stubs).
- Phase 1 changes NO sessiond behavior: every extraction keeps sessiond's public API via thin wrappers; on-disk names byte-identical — socket `d.sock`, lock `d.lock` (under `$XDG_RUNTIME_DIR/bpa` else `/tmp/bpa-{uid}`), label `ai.builderpro.desktop.sessiond`, logs `sessiond.tracing.log`/`sessiond.out.log`/`sessiond.err.log`, rendered plist byte-identical (asserted by test). Full existing suite green after each Phase-1 task.
- orchd names (spec §2.1): socket `orchd.sock`, lock `orchd.lock`, DB `orchd.db`, label `ai.builderpro.desktop.orchd`, logs `orchd.tracing.log`/`orchd.out.log`/`orchd.err.log`. Plist is RENDERED AT RUNTIME by `src-tauri/src/launchd.rs` — no `.plist` file in the repo.
- orchd DB: spec §5.1 DDL verbatim; `PRAGMA foreign_keys=ON` per connection; WAL + `busy_timeout=5000` on disk (in-memory: no WAL); unix-ms timestamps; `uuid::Uuid::new_v4().to_string()` ids; migration via `daemon_core::migrate::run_migrations` (whole-chain single tx, fail-closed — spec §3 semantics, NOT per-step).
- D11: no `Option<Option<T>>` anywhere on the wire — nullable-field updates via `SetIdeaProject`/`SetInsightFitVerdict`.
- Every §5.2 invariant = typed error; failed mutations broadcast NO push; no silent failure (spec §11 matrix).
- RuleSet md: files are truth; atomic write (tmp+rename); sha256 hex; content NEVER logged; the ONLY file I/O in orchd (D4 narrow exception).
- Import preserves EVERY field verbatim (`created_at`/`updated_at`/`rank`/`ord`/`md_hash` — never re-stamped).
- Every `#[tauri::command]` = thin wrapper over a unit-testable inner fn; core-local error enums serde `tag="kind"` camelCase per-variant (container `rename_all` does NOT cascade into struct-variant fields).
- `src/ipc/orchd-types.ts` is ts-rs GENERATED (never hand-edited; gate-diffed).
- Design-system: new atom ⇒ new `| Atom | Contract |` row in `docs/design-system.md` in the SAME task. Amber reserved for «нужен ты»; HomeGoals mounts BELOW the attention sections.
- Gate: `bash scripts/final-suite.sh` → `ALL GATES PASSED` (T20 extends to 9 stages; `.github/workflows/ci.yml` updated in lockstep).
- Commits: conventional, trailer `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`.
- Work in a worktree branch `worktree-s3` (superpowers:using-git-worktrees), never on main.

## Task graph

Phase 1 (sequential): T1 → T2 → T3. Phase 2: T4 → T5. Phase 3 (sequential, shared orchd files): T6 → T7 → T8 → T9 → T10. Phase 4: T11 → T12. Phase 5: T13, then parallel group {T14, T15, T16, T17} (non-overlapping source files; design-system.md rows are distinct-line additions — controller serializes those doc edits if a conflict bites), then T18 → T19. Close: T20 → T21 → T22.

---

## Phase 1 — `bpa-daemon-core` extraction (sessiond re-seat)

### Task 1: daemon-core crate — `dirs` + `singleton` + `logging`

**Files:** Create `crates/daemon-core/Cargo.toml` (name `bpa-daemon-core`; deps `{ workspace = true }`: `tokio`, `rusqlite` (T2 will need it — add now), `tracing`, `tracing-subscriber`, `tracing-appender`, `rustix`, `libc`, `thiserror`; dev-dep `tempfile`), `crates/daemon-core/src/lib.rs` (`pub mod dirs; pub mod singleton; pub mod logging;`), `crates/daemon-core/src/{dirs.rs,singleton.rs,logging.rs}`. Modify root `Cargo.toml` (workspace member), `crates/sessiond/Cargo.toml` (dep `bpa-daemon-core = { workspace = true }` — also add it to `[workspace.dependencies]`), `crates/sessiond/src/{boot.rs,singleton.rs,logging.rs,main.rs}`.
**Interfaces — Produces (spec §3 table, verified against sessiond source):**
- `dirs::app_support_dir() -> PathBuf` — MOVED from `sessiond/boot.rs:19` (`$HOME` fallback `/tmp`, + `Library/Application Support/ai.builderpro.desktop`); `pub` here. Sessiond `boot.rs` calls it; `app_support_dir_for_test` re-export in `sessiond/lib.rs` unchanged.
- `singleton::` — parameterized moves from `sessiond/singleton.rs`: `resolve_socket_path(file_name: &str) -> PathBuf` / `resolve_lockfile(file_name: &str) -> PathBuf` (runtime dir resolution `$XDG_RUNTIME_DIR/bpa` else `/tmp/bpa-{uid}` moved verbatim as private `socket_dir()`); `acquire_single_instance_lock(lock_file_name: &str) -> io::Result<LockGuard>`; `pub fn acquire_lock_at(path: &Path) -> io::Result<LockGuard>` (promoted from private — daemon tests need it); `ensure_socket_dir()`, `assert_socket_path_len(&Path)`, `set_socket_mode(&Path)`, `check_peer_cred(BorrowedFd<'_>)` moved as-is. Sessiond keeps ITS exact current publics as one-line wrappers passing `"d.sock"`/`"d.lock"` (incl. the zero-arg `resolve_socket_path()`, `acquire_single_instance_lock()`, and `#[doc(hidden)] acquire_lock_at_for_test`).
- `logging::` — (a) `pub fn init_tracing(log_file_name: &str) -> std::io::Result<()>` EXTRACTED from `sessiond/main.rs::init_tracing` (lines 48-76): `{app_support}/logs` + chmod `0o700` + `tracing_appender::rolling::never(dir, log_file_name)` non-ANSI; `sessiond/main.rs` re-seats passing `"sessiond.tracing.log"` (keep the appender guard alive exactly as main.rs does today). (b) test seam `init_to_file(&Path)`/`flush()` moved as-is from `sessiond/logging.rs` (sessiond re-exports wrappers so its integration tests don't change imports).

- [ ] **Step 1: RED.** daemon-core tests (move each module's existing unit tests alongside): `resolve_socket_path("a.sock")`/`resolve_lockfile("a.lock")` end with those names under the same runtime dir rules (assert both XDG and /tmp branches via env manipulation under a test lock); double-`acquire_lock_at` on one path fails; `init_to_file` twice errors. Sessiond wrapper test: resolved socket == `…/d.sock`, lock == `…/d.lock` (hardcoded leaf assertions). Run `cargo test -p bpa-daemon-core -p bpa-sessiond` → FAIL (crate empty).
- [ ] **Step 2: GREEN.** Move code, parameterize names, wire wrappers. No logic edits.
- [ ] **Step 3:** `cargo test --workspace` → PASS (sessiond 168 + all integration binaries untouched); `cargo clippy --workspace --all-targets -- -D warnings`; `cargo fmt --all -- --check`. Commit `refactor(daemon-core): extract dirs/singleton/logging; sessiond re-seated, on-disk names byte-identical (S3 §3, phase 1)`.

### Task 2: daemon-core `migrate` runner

**Files:** Create `crates/daemon-core/src/migrate.rs` (+ `pub mod migrate;`). Modify `crates/sessiond/src/persistence.rs` (re-seat `migrate()`).
**Interfaces — Produces (spec §3, EXACT sessiond semantics from `persistence.rs:191-253`):**
```rust
pub struct Migration { pub upto: i64, pub apply: fn(&rusqlite::Transaction) -> rusqlite::Result<()> }
pub enum MigrateError { VersionTooNew { found: i64, supported: i64 }, Sql(rusqlite::Error) }
pub fn run_migrations(conn: &rusqlite::Connection, from_version: i64, target: i64,
                      steps: &[Migration]) -> Result<(), MigrateError>
```
Semantics: `from_version == target` ⇒ Ok early-return; `from_version > target` ⇒ `VersionTooNew`; else ONE `conn.unchecked_transaction()` for the WHOLE chain — apply every step where `from_version < step.upto` in order — single `tx.pragma_update(None, "user_version", target)` INSIDE the tx — `tx.commit()`. Any error ⇒ rollback, `user_version` untouched (fail-closed). Sessiond `Db::migrate` re-seats as a 3-entry `&[Migration]` table (v<1, v<2, v<3 `execute_batch` bodies moved VERBATIM into step fns) and wraps `MigrateError` into the existing `PersistError::Migration` so `.code() == "DbMigration"` and every message string consumers see stays identical (map `VersionTooNew{found,supported}` to the current text `"db user_version {found} newer than supported {supported}"`).

- [ ] **Step 1: RED.** daemon-core tests: fresh in-memory DB reaches target with all steps applied + `user_version == target`; a mid-chain failing step rolls back EVERYTHING (earlier steps' tables absent, version untouched — whole-chain semantics, NOT per-step); `VersionTooNew` on a future version; empty steps + target 0 ⇒ Ok. Sessiond: existing migration tests (v2 fixture → v3, fail-closed fixture at persistence.rs tests ~1343-1396) must pass UNCHANGED. FAIL first.
- [ ] **Step 2: GREEN.** Implement; re-seat.
- [ ] **Step 3:** `cargo test --workspace` → PASS. Commit `refactor(daemon-core): whole-chain fail-closed migration runner; sessiond steps re-seated verbatim (S3 §3)`.

### Task 3: generic framing + `server_handshake` + generic `Broadcaster`

**Files:** Modify `crates/protocol/src/framing.rs`; create `crates/daemon-core/src/handshake.rs` + `crates/daemon-core/src/broadcast.rs` (+ lib.rs mods; daemon-core gains dep `bpa-protocol = { workspace = true }`); modify `crates/sessiond/src/socket_server.rs` (re-seat accept path + Broadcaster).
**Interfaces — Produces:**
- `bpa-protocol` (spec §4.1): `pub fn encode_cbor_frame<T: Serialize>(v: &T) -> Result<Vec<u8>, FrameError>`; `pub struct CborFrameDecoder<T>` with `new()/push(&[u8])/decode() -> Result<Vec<T>, FrameError>` (length-prefix + `MAX_FRAME_LEN` oversize reject preserved). Existing `encode_frame(&Frame)` + `FrameDecoder` (struct name, `Default` derive, method signatures) re-implemented as thin instantiations — their tests unchanged.
- `daemon_core::handshake` (spec §3): `pub async fn server_handshake(stream: &mut tokio::net::UnixStream, min: u16, max: u16, build: &str) -> std::io::Result<Option<u16>>` — moves `read_client_preamble` + `CLIENT_PREAMBLE_HEADER_LEN` (=10) from `sessiond/socket_server.rs:748-776`; behavior extracted from lines 606-643: `PREAMBLE_TIMEOUT`-bounded preamble read, `negotiate(client.min, client.max, min, max)`, `PREAMBLE_TIMEOUT`-bounded reply write with `build` filled on Accepted; `Ok(Some(chosen))` / `Ok(None)` (Incompatible written, caller closes) / `Err` (garbage/timeout — sessiond's call site keeps ITS current quiet-`Ok(())` handling).
- `daemon_core::broadcast` (spec §3): `pub struct Broadcaster<F: Clone + Send + 'static>` — generic extraction of `sessiond/socket_server.rs:211-231` (`Arc<Mutex<HashMap<u64, mpsc::Sender<F>>>>`; `register(id, tx)`, `deregister(id)`, `broadcast(f)` via non-blocking `try_send`, full/closed silently skipped). Sessiond re-seats with `type Broadcaster = daemon_core::broadcast::Broadcaster<Frame>`-style alias (call sites unchanged).

- [ ] **Step 1: RED.** protocol: generic round-trip with a toy enum incl. split-buffer push + oversize reject; existing framing tests untouched. daemon-core handshake over a `UnixStream::pair`-style socketpair (or tmp socket): compatible → `Some(chosen)` + client decodes Accepted with the passed build; disjoint → `None` + client decodes Incompatible; garbage magic → `Err`; stalled client → `Err` within `PREAMBLE_TIMEOUT`. broadcast: two receivers get the frame; a full receiver queue is skipped without blocking; deregistered receiver gets nothing. sessiond: full crate green post re-seat. FAIL first.
- [ ] **Step 2: GREEN.** Implement + re-seat sessiond (`server_handshake(DAEMON_MIN_VERSION, DAEMON_MAX_VERSION, &deps.daemon_build)`).
- [ ] **Step 3: Phase-1 gate.** `bash scripts/final-suite.sh` → `ALL GATES PASSED`. Commit `refactor(protocol,daemon-core): generic framing + extracted server_handshake + generic Broadcaster; sessiond re-seated (S3 §3/§4.1) — phase-1 gate green`.

## Phase 2 — orchd skeleton

### Task 4: `bpa-orchd-proto` crate

**Files:** Create `crates/orchd-proto/Cargo.toml` (name `bpa-orchd-proto`; deps `serde`, `bpa-protocol` (framing generics), dev-dep `ts-rs` — mirror `crates/protocol/Cargo.toml`'s arrangement exactly), `crates/orchd-proto/src/lib.rs`, `crates/orchd-proto/tests/ts_export.rs`, `crates/orchd-proto/tests/roundtrip.rs`. Modify root `Cargo.toml` (member + `[workspace.dependencies]` entry for `bpa-orchd-proto`).
**Interfaces — Produces:** ALL spec §4.2 types VERBATIM (entities, `OrchdErrorCode`, `OrchdRequest` incl. `SetIdeaProject`/`SetInsightFitVerdict` — D11, `OrchdResponse`, `OrchdPush`, `OrchdFrame`, `PolicyRules`, `RuleSetView`, `RuleFileState`) + `ORCHD_CLIENT_MIN/MAX_VERSION = 1`, `ORCHD_DAEMON_MIN/MAX_VERSION = 1` + `pub fn encode_orchd_frame(&OrchdFrame) -> Result<Vec<u8>, FrameError>` + `pub type OrchdFrameDecoder = CborFrameDecoder<OrchdFrame>`. ts-rs: every entity/enum `#[derive(TS)] #[ts(export_to = "orchd-types.ts")]`; `tests/ts_export.rs` mirrors `crates/protocol/tests/ts_export.rs` — `export_all_to(CARGO_MANIFEST_DIR/../../src/ipc)` then STRUCTURAL assertions (contains-normalized) — camelCase (`projectId`, `workspaceIds`, `metricRefs`, `fitVerdict`, `mdPath`), wire tags (`inDev`, `noFit`), `rank: number`, absence of snake_case.
**NOTE:** the gate's diff check (`git diff --exit-code -- src/ipc/orchd-types.ts`) is wired in T20; here commit the generated file.

- [ ] **Step 1: RED.** roundtrip.rs: CBOR encode/decode one instance of EVERY Request/Response/Push variant (non-default field values) through `encode_orchd_frame`/`OrchdFrameDecoder`; serde-string assertions (`IdeaLifecycle::InDev` ⇒ wire `"inDev"`; `TaskStatus::Backlog` ⇒ `"backlog"`; `FitVerdict::NoFit` ⇒ `"noFit"`). ts_export structural assertions per above. FAIL (crate empty).
- [ ] **Step 2: GREEN.** Write the crate; run the export; commit generated `src/ipc/orchd-types.ts`.
- [ ] **Step 3:** `cargo test -p bpa-orchd-proto` → PASS. Commit `feat(orchd-proto): S3 wire contract — entities + frames [1,1] + ts-rs orchd-types.ts (spec §4.2, frozen append-only)`.

### Task 5: `bpa-orchd` daemon skeleton

**Files:** Create `crates/orchd/Cargo.toml` (name `bpa-orchd`, `[[bin]] name = "bpa-orchd"`; deps `{ workspace = true }`: `bpa-daemon-core`, `bpa-orchd-proto`, `bpa-protocol`, `tokio`, `rusqlite`, `serde`, `serde_json`, `uuid`, `sha2` (ADD `sha2 = "0.10"` to `[workspace.dependencies]` in root Cargo.toml), `tracing`, `thiserror`, `libc`; dev-dep `tempfile`), `crates/orchd/src/{main.rs,lib.rs,boot.rs,socket_server.rs,persistence.rs}`, `crates/orchd/tests/boot_integration.rs`. Modify root `Cargo.toml`.
**Interfaces — Produces:**
- `bpa_orchd::run(socket: PathBuf, shutdown_tx: watch::Sender<bool>, shutdown_rx: watch::Receiver<bool>) -> std::io::Result<()>` (mirror `bpa_sessiond::run` shape); `lib.rs` re-exports `run` + `app_support_dir_for_test`-style hook + `pub use bpa_orchd_proto as protocol;`.
- `main.rs`: `daemon_core::logging::init_tracing("orchd.tracing.log")`, singleton `acquire_single_instance_lock("orchd.lock")`, socket `resolve_socket_path("orchd.sock")`, SIGTERM → watch flip (mirror sessiond main.rs).
- `persistence::Db::{open(&Path), open_in_memory()}` — spec §5.1 DDL verbatim as `&[Migration{upto:1}]` via `run_migrations`; pragmas: WAL+busy_timeout 5000+foreign_keys ON on disk; busy_timeout+foreign_keys in-memory; `open` wraps with the sessiond corrupt-quarantine pattern (rename `orchd.db.corrupt-<ts>` + drop `-wal`/`-shm`, retry once); disk-open failure ⇒ log + in-memory fallback (mirror `open_db_degrading`).
- boot ensures GLOBAL ruleset row + `{app_support}/rules/global.md` (template `# Глобальные правила\n`, `policy='{}'`) idempotently.
- `socket_server.rs`: accept → `check_peer_cred` → `server_handshake(ORCHD_DAEMON_MIN_VERSION, ORCHD_DAEMON_MAX_VERSION, env!("CARGO_PKG_VERSION"))` → per-connection mpsc writer (cap 1024, mirror sessiond) + `Broadcaster<OrchdFrame>` register/deregister → dispatch loop: `Ping`→`Pong`, `OrchdShutdown{drain}`→(drain: WAL checkpoint) Ack + watch flip; ALL other verbs → `Error{Validation, "not implemented"}` stub arm (T10 replaces).

- [ ] **Step 1: RED.** `boot_integration.rs` (copy the HomeGuard/HOME_LOCK pattern from `sessiond/tests/boot_integration.rs:48-74`; preamble via `ORCHD_CLIENT_MIN/MAX_VERSION` consts): boot on temp socket → Accepted `chosen == ORCHD_DAEMON_MAX_VERSION` → Ping→Pong → clean shutdown unlinks socket; second-instance flock refusal; fresh DB `user_version=1` + all §5.1 tables + FK effective (violating insert → constraint error) + global ruleset row + `rules/global.md` on disk; double boot doesn't duplicate the global row. FAIL.
- [ ] **Step 2: GREEN.** Implement skeleton.
- [ ] **Step 3:** `cargo test -p bpa-orchd` → PASS; clippy/fmt clean. Commit `feat(orchd): daemon skeleton — boot/singleton/socket [1,1] + orchd.db schema v1 + global ruleset ensure (S3 §5)`.

## Phase 3 — domain

### Task 6: persistence — project + workspace links + goals

**Files:** Modify `crates/orchd/src/persistence.rs`.
**Interfaces — Produces (all `Result<_, OrchdPersistError>`; `pub enum OrchdPersistError { NotFound, Invariant(String), Conflict(String), Validation(String), Sql(rusqlite::Error) }`):** `create_project(name: &str, description: &str, workspace_ids: &[String]) -> Result<Project>` (ONE tx: project + links + strategic goal `"Стратегическая цель"` + ruleset row with default `md_path`; empty ids ⇒ `Invariant`; duplicate ws ⇒ `Conflict`; the ruleset FILE is written by the T10 handler via ruleset_files — DB row only here); `update_project(id, name: Option<&str>, description: Option<&str>)`; `archive_project(id)`; `list_projects() -> Vec<Project>` (workspace_ids joined by `ord`); `add_project_workspace(project_id, workspace_id)`; `remove_project_workspace(project_id, workspace_id)` (last ⇒ `Invariant`); `create_goal(project_id, parent_id: Option<&str>, kind, title, body)` (second strategic ⇒ `Invariant` via the partial unique index mapped to `Invariant`; cross-project parent ⇒ `Invariant`); `update_goal(id, title/body/status/metric_refs Options)`; `move_goal(id, new_parent_id: Option<&str>, new_ord: i64)` (strategic ⇒ `Invariant`; cycle walk-up ⇒ `Invariant`); `delete_goal(id)` (strategic ⇒ `Invariant`; FK cascade); `list_goals(project_id) -> Vec<Goal>` (parents-first, then `ord`). Archived-project guard on every mutator ⇒ `Invariant("project archived")`.

- [ ] **Step 1: RED.** In-memory tests: every behavior + create_project's strategic goal visible via list_goals + ruleset row present; workspace UNIQUE cross-project ⇒ `Conflict`; remove-last ⇒ `Invariant`; goal reparent-under-own-descendant ⇒ `Invariant`; subtree delete cascades grandchildren; metric_refs JSON round-trip; archived blocks create_goal. FAIL.
- [ ] **Step 2: GREEN + Step 3:** `cargo test -p bpa-orchd persistence` → PASS. Commit `feat(orchd): project/workspace-link/goal persistence + §5.2 invariants (S3, TDD)`.

### Task 7: persistence — ideas + insights + tasks

**Files:** Modify `crates/orchd/src/persistence.rs`.
**Interfaces — Produces:** `create_idea(project_id: Option<&str>, title, body)`; `update_idea(id, title: Option<&str>, body: Option<&str>)`; `set_idea_project(id, project_id: Option<&str>)` (D11 — `None` detaches); `set_idea_lifecycle(id, IdeaLifecycle)`; `delete_idea(id)`; `list_ideas(project_id: Option<&str>) -> Vec<Idea>` (None ⇒ ALL, `created_at DESC`); `create_insight(project_id: Option<&str>, source, title, body)`; `update_insight(id, title/body Options)`; `set_insight_fit_verdict(id, fit_verdict: Option<FitVerdict>, fit_reasoning: &str)` (D11); `set_insight_status(id, status, resolution_reasoning: Option<&str>)`; `delete_insight`; `list_insights(Option<&str>)`; `create_task(project_id, parent_id: Option<&str>, title, body, status: Option<TaskStatus>, source: TaskSource, source_id: Option<&str>, tags: &[String])` (rank = `SELECT COALESCE(MAX(rank), 0) + 1024 … WHERE project_id = ?` ⇒ first task 1024; parent cross-project/cycle ⇒ `Invariant`); `update_task(id, title/body/tags Options)`; `set_task_status(id, TaskStatus)`; `set_task_rank(id, f64)`; `delete_task(id)` (cascade); `list_tasks(Option<&str>) -> Vec<DomainTask>` (`ORDER BY rank`). Enum⇄TEXT helpers assert §5.1 CHECK literals (`in_dev`, `no_fit`) — distinct from wire camelCase (`inDev`, `noFit`).

- [ ] **Step 1: RED.** Tests: DB TEXT literals verified raw (`in_dev`); orphan idea under None-list only; rank 1024/2048/3072 + first-task base 1024; set_task_rank persists f64 midpoints; task cycle + cross-project parent ⇒ `Invariant`; delete cascades subtasks; `set_idea_project(None)` detaches; `set_insight_fit_verdict` stores verdict+reasoning. FAIL.
- [ ] **Step 2: GREEN + Step 3:** PASS. Commit `feat(orchd): idea/insight/task persistence — D11 set-verbs, rank math, cycles, cascade (S3 §5.2)`.

### Task 8: ruleset files + ruleset persistence

**Files:** Create `crates/orchd/src/ruleset_files.rs`; modify `crates/orchd/src/persistence.rs` (ruleset queries), `crates/orchd/src/boot.rs` (global-ensure re-seated onto these fns).
**Interfaces — Produces (spec §7):** `ruleset_files::write_atomic(path: &Path, content: &str) -> io::Result<String>` (create parent dirs; write `<path>.tmp` then rename; returns sha256 hex); `ruleset_files::read_state(path: &Path, stored_hash: &str) -> (Option<String>, RuleFileState)`; persistence: `get_ruleset(scope, project_id: Option<&str>) -> Result<RuleSet>`; `upsert_ruleset(scope, project_id, md_content: Option<&str>, md_path: Option<&str>, policy: Option<&PolicyRules>) -> Result<RuleSet>` (md_path absolute + parent exists else `Validation`; policy strict via `#[serde(deny_unknown_fields)]` mirror struct + `spend_cap_usd >= 0` + non-empty allowlist entries else `Validation`); `acknowledge_rule_file(id) -> Result<RuleSet>` (re-read → new hash; missing ⇒ `Invariant("file missing")`).

- [ ] **Step 1: RED.** Tempdir tests: write_atomic creates dirs, leaves no `.tmp`, returns the exact-bytes hash; read_state three states; upsert-with-content rehashes; relative md_path ⇒ `Validation`; unknown policy key / negative cap ⇒ `Validation`; acknowledge after external edit; content marker never in the log file (orchd variant of the no-secrets test pattern). FAIL.
- [ ] **Step 2: GREEN + Step 3:** PASS. Commit `feat(orchd): ruleset file layer — files-as-truth, atomic writes, sha256 states (S3 §7, D4)`.

### Task 9: export / import

**Files:** Create `crates/orchd/src/export.rs`; modify `crates/orchd/src/persistence.rs` (raw-insert helpers for the import tx).
**Interfaces — Produces (spec §8):** `export_project(db, project_id) -> Result<String>` (camelCase JSON; `mdContent` read live, missing ⇒ `null` — NO mdMissing flag); `export_all(db) -> Result<String>` (`projects` + `globalRuleset` + `orphanIdeas` + `orphanInsights`); size guard: serialized len must fit `MAX_FRAME_LEN` minus framing overhead, else `OrchdPersistError::Validation`-mapped honest error (`Io` on the wire per §6? — LOCK: return a dedicated error mapped to wire `Io` with message "export exceeds the 16 MiB frame cap"); `import_bundle(db, app_support: &Path, json: &str) -> Result<ImportCounts>` — `bundleFormat != 1` ⇒ `Validation`; ONE tx of raw inserts preserving EVERY field verbatim (`created_at`, `updated_at`, `rank`, `ord`, `md_hash`); any PK/UNIQUE hit ⇒ `Conflict("<entity> <id> already exists")` + full rollback; ruleset md written only under app-support (foreign path ⇒ write default path + repoint); accepts both shapes (`project` vs `projects` key).

- [ ] **Step 1: RED.** Tests: project with 2-level goals + orphan idea + subtask + ruleset content → export_all → fresh DB → import → export_all → `serde_json::Value` equality after removing `exportedAt`; timestamps/rank verbatim (explicitly assert an imported row's `updated_at` equals the exported value, NOT now); collision ⇒ `Conflict` + zero rows changed; foreign md_path lands under app-support; `bundleFormat: 2` ⇒ `Validation`; per-project bundle standalone; oversize guard fires on a synthetic >16 MiB bundle (construct via a huge body string). FAIL.
- [ ] **Step 2: GREEN + Step 3:** PASS. Commit `feat(orchd): export/import — bundleFormat 1, field-verbatim, tx-atomic, frame-cap guard (S3 §8, D7)`.

### Task 10: socket dispatch + pushes

**Files:** Modify `crates/orchd/src/socket_server.rs` (replace the T5 stub arm).
**Interfaces — Consumes:** T6–T9. **Produces:** every `OrchdRequest` variant dispatched per spec §6: mutating verbs reply the updated entity (`Ack` for deletes; `ImportReport` for import) AND broadcast the matching push (`ImportBundle` ⇒ pushes for every family the bundle touched); FAILED requests broadcast NOTHING; `CreateProject` handler also writes the project ruleset FILE via `ruleset_files::write_atomic` (template `# Правила проекта <name>\n`) after the tx commits — a file-write failure is logged + surfaced as `file_state: Missing` on next Get, never rolls back the committed project (honest, documented); `GetRuleSet` assembles `RuleSetView` via `read_state`; error mapping per spec §6 (`Sql→Io`).

- [ ] **Step 1: RED.** Socket-level tests (stub client over `run()`, version consts): CreateProject → `Response::Project` + second connection gets `ProjectsChanged` + rules file exists on disk; CreateGoal → `GoalsChanged{project_id}` correct; remove-last-workspace → `Error{Invariant}` + NO push observed (assert via a bounded-time push-drain); GetRuleSet Ok→ExternallyModified after on-disk edit; SetIdeaProject(None) detaches; ImportBundle → `ImportReport` + family pushes; unknown-id delete → `Error{NotFound}`. FAIL.
- [ ] **Step 2: GREEN + Step 3:** `cargo test -p bpa-orchd` (full crate) → PASS. Commit `feat(orchd): full domain dispatch + coarse invalidation pushes (S3 §4.2/§6, D6)`.

## Phase 4 — core integration

### Task 11: orchd client + launchd parameterization

**Files:** Create `src-tauri/src/orchd_client.rs` (READ `src-tauri/src/socket_client.rs` FIRST and mirror its structure 1:1); modify `src-tauri/src/launchd.rs`, `src-tauri/src/lib.rs`.
**Interfaces — Produces:**
- `OrchdClient` mirror of `DaemonClient` (verified shape): `connect(client_build: String)` (resolves `orchd.sock` via the daemon-core-parameterized resolution), `connect_with_retry(build, attempts, delay)`, `request(OrchdRequest) -> Result<OrchdResponse, OrchdClientError>` (AtomicU64 + `HashMap<u64, oneshot>` in one connection task), `on_push(impl Fn(OrchdPush) + Send + 'static)`, `on_conn(impl Fn(ConnState))`; `pub enum OrchdClientError { Disconnected, Daemon { code: String, message: String }, IncompatibleOrchd { daemon_min: u16, daemon_max: u16 }, RequestTooLarge { size: usize } }`; `pub type OrchdClientSlot = Arc<RwLock<Option<Arc<OrchdClient>>>>`.
- `launchd.rs` parameterized ADDITIVELY: `LaunchdAgent` gains fields `label: &'static str`, `stdout_log_name: &'static str`, `stderr_log_name: &'static str`; all `LABEL`-const uses switch to `self.label`; `resolve_daemon_path()` gains a `bin_name: &str` param. Sessiond call sites pass `"ai.builderpro.desktop.sessiond"` / `"sessiond.out.log"` / `"sessiond.err.log"` / `"bpa-sessiond"`. orchd agent: label **`ai.builderpro.desktop.orchd`**, logs `orchd.out.log`/`orchd.err.log`, bin `"bpa-orchd"`.
- `lib.rs`: `bring_up_orchd(handle, …)` mirroring `bring_up_daemon` (verified flow): build agent → `install_agent()+bootstrap()+kickstart()` unconditionally → `connect_with_retry(client_build(), BOOT_CONNECT_ATTEMPTS, 500ms)` → Ok: wire pushes + fill slot; `IncompatibleOrchd`: emit `orchd://incompatible`; else emit `orchd://down`. `AppState` gains `orchd: OrchdClientSlot` + orchd agent/status fields (mirror sessiond's).

- [ ] **Step 1: RED.** Byte-identical plist test: render the sessiond agent pre/post parameterization → identical string (golden assertion). orchd-stub tests (mirror `connect_to_stub` with `orchd.sock` + `ENV_TEST_LOCK`; stub replies `Accepted{chosen: ORCHD_DAEMON_MAX_VERSION}` — CONSTS): connect ok; two in-flight requests correlate; incompatible stub ⇒ typed `IncompatibleOrchd`; push reaches the `on_push` callback; connect-refused ⇒ retryable error ≠ Incompatible. FAIL.
- [ ] **Step 2: GREEN + Step 3:** `cargo test -p builder-pro-ai` → PASS (existing launchd/socket tests untouched). Commit `feat(core): orchd client [1,1] + launchd label/log/bin parameterization (S3 §9)`.

### Task 12: orchd commands + broker events

**Files:** Modify `src-tauri/src/commands.rs`, `src-tauri/src/broker.rs`, `src-tauri/src/lib.rs` (register).
**Interfaces — Produces (spec §9, locked):**
- broker: `pub fn map_orchd_push(push: OrchdPush) -> BrokerAction` (pure, mirror `map_push`) + consts `EV_ORCHD_PROJECTS_CHANGED = "orchd://projects-changed"`, `EV_ORCHD_GOALS_CHANGED = "orchd://goals-changed"` `{projectId}`, `EV_ORCHD_IDEAS_CHANGED = "orchd://ideas-changed"`, `EV_ORCHD_INSIGHTS_CHANGED = "orchd://insights-changed"`, `EV_ORCHD_TASKS_CHANGED = "orchd://tasks-changed"` `{projectId}`, `EV_ORCHD_RULESET_CHANGED = "orchd://ruleset-changed"` `{scope, projectId?}`, `EV_ORCHD_DOWN/UP/INCOMPATIBLE = "orchd://down|up|incompatible"`.
- commands: one `orchd_*` `#[tauri::command]` per §4.2 verb (`orchd_create_project`, `orchd_set_idea_project`, `orchd_set_insight_fit_verdict`, `orchd_get_ruleset`, `orchd_export_all`, `orchd_import_bundle`, …) via `state.orchd()?` accessor (mirror `state.client()`); `CommandError` gains ADDITIVE variant `IncompatibleOrchd { orchd_min: u16, orchd_max: u16 }` (serde camelCase per-variant); orchd `Error{code, message}` → `CommandError::Daemon { code: <enum-as-string>, message }`.
- special flows: `orchd_reveal_rules_file(scope, project_id)` — internally `GetRuleSet` then `opener::reveal(md_path)` (JS never passes a path); `orchd_export_to_file(project_id: Option<String>, dest_dir: String)` — export (project or all) + write `<name>-export.json` under the picked dir (validated: dest_dir must be the exact `pick_folder` result passed through — document that JS supplies it; write via std::fs, error → `CommandError::Internal`); `orchd_import_from_file(path: String)` — read file (10 MiB read cap guard) → `ImportBundle`.
- lifecycle commands (spec §9): `orchd_reconnect()` — drops the slot, re-runs the `bring_up_orchd` connect sequence (T19's [Повторить] target); `orchd_upgrade(app: AppHandle)` — mirror `upgrade_daemon` (commands.rs:713-721) verbatim: best-effort `request(OrchdShutdown{drain:true})` → `state.orchd_launchd.kickstart_force()?` → `app.restart()`.

- [ ] **Step 1: RED.** Stub tests: `orchd_create_project` happy + `Invariant` maps to `Daemon{code:"Invariant"}`; broker unit: every `OrchdPush` variant → correct const + camelCase payload; reveal flow invokes opener with the GetRuleSet-returned path (seam-mock opener call via cfg(test) indirection or assert at the inner-fn boundary); export_to_file writes the JSON; import_from_file round-trips through a stub; `orchd_upgrade` inner-fn sends `OrchdShutdown{drain:true}` then kickstarts (stub-sequence assertion, mirror the existing `upgrade_daemon_core` test). FAIL.
- [ ] **Step 2: GREEN + Step 3:** `cargo test -p builder-pro-ai` → PASS. Commit `feat(core): orchd_* commands + orchd:// broker events + IncompatibleOrchd (S3 §9, D10)`.

## Phase 5 — frontend

### Task 13: frontend IPC + domain store slice

**Files:** Create `src/ipc/orchd.ts` (+ `src/ipc/orchd.test.ts`); modify `src/ipc/events.ts`, `src/store/store.ts` + `src/store/store.test.ts`, `src/App.tsx` (subscriptions only).
**Interfaces — Consumes:** `src/ipc/orchd-types.ts` (T4). **Produces (locked):**
- `orchd.ts`: typed `invoke` wrappers matching T12 names/args verbatim (style: `invoke<Project>("orchd_create_project", { name, description, workspaceIds })`); `describeOrchdError(e: unknown): string` helper (`Invariant`⇒«недопустимая операция: …», `Conflict`⇒«конфликт: …», `NotFound`⇒«не найдено», `Validation`⇒«неверные данные: …», `Io`⇒«ошибка сервиса: …», Disconnected/down ⇒ «оркестратор недоступен»).
- store: widen `view: "home" | "workspace" | "project"` AND `setView(v)`'s param (both literal-typed today at `store.ts:57`/`:125`); add `activeProjectId`, `projects: Project[]`, `goalsByProject: Record<string, Goal[]>`, `ideas: Idea[]`, `insights: Insight[]`, `tasksByProject: Record<string, DomainTask[]>`, `rulesets: Record<string, RuleSetView>` (keys `` `global` ``/`` `project:${id}` ``), `orchdDown: boolean`, `orchdIncompatible: boolean`, `orchdUpgradeDialogOpen: boolean`; actions `refreshProjects/refreshGoals(projectId)/refreshIdeas/refreshInsights/refreshTasks(projectId)/refreshRuleset(key)`, `openProject(id)` (view+activeProjectId), `setOrchdDown/setOrchdIncompatible`.
- `events.ts`: `onOrchdProjectsChanged`, `onOrchdGoalsChanged({projectId})`, `onOrchdIdeasChanged`, `onOrchdInsightsChanged`, `onOrchdTasksChanged({projectId})`, `onOrchdRulesetChanged({scope, projectId?})`, `onOrchdDown`, `onOrchdUp`, `onOrchdIncompatible` (exact strings per T12 consts).
- `App.tsx`: add `track(onOrchd…(…))` lines to the EXISTING mount effect (the `track(p)` pattern at App.tsx:94-247) + initial `refreshProjects()` once connected.

- [ ] **Step 1: RED.** store tests: openProject flips view+id; goals-changed refreshes ONLY the named project (mock ipc); rulesets keying; orchdDown flag flow. ipc test: wrapper passes exact command name + camelCase args (mock `invoke`). FAIL.
- [ ] **Step 2: GREEN + Step 3:** `npx vitest run` + `npx tsc --noEmit` → PASS. Commit `feat(ui): orchd ipc + domain store slice + project view state (S3 §10)`.

### Task 14: GoalTree component

**Files:** Create `src/components/GoalTree.tsx` + `src/components/GoalTree.test.tsx`; modify `docs/design-system.md` (+1 row `| **Tree row** | indent level × 16px, inline title edit, status select; strategic root pinned, no delete/move |`).
**Interfaces — Consumes:** T13 slice + wrappers. **Produces:** `<GoalTree projectId={string} />` — indent tree from `parent_id` (strategic first, pinned, non-deletable/non-movable); per-row inline title edit (`orchd_update_goal`), status select (active/achieved/dropped), «+ подцель» (`orchd_create_goal{parent_id}`), delete-with-confirm «удалить ветку целиком?» (`orchd_delete_goal`), sibling ▲/▼ via `orchd_move_goal` (same parent, swapped ord); errors → `showToast(describeOrchdError(e))`. Match the existing component-test setup exactly (copy the environment/pragma arrangement from `src/components/HomeView.test.tsx`).

- [ ] **Step 1: RED.** Tests: 3-level tree indent order; strategic row lacks delete/move; add-subgoal passes parent_id; delete confirm gates; ▲ swaps ord args; Invariant error → toast. FAIL.
- [ ] **Step 2: GREEN + Step 3:** vitest + tsc PASS. Commit `feat(ui): goal tree editor — pinned strategic root, subtree ops (S3 §10, D5)`.

### Task 15: IdeasList + InsightsList

**Files:** Create `src/components/IdeasList.tsx` + test, `src/components/InsightsList.tsx` + test; modify `docs/design-system.md` (+1 row `| **Lifecycle chip** | small select-chip cycling a locked enum; one accent, no amber |`).
**Interfaces — Consumes:** T13. **Produces:** `<IdeasList projectId={string | null} />` — newest-first, lifecycle chip select (§4.2 enum), create form (title+body), inline edit, delete confirm, orphan rows get «привязать к проекту» select → `orchd_set_idea_project`; `<InsightsList projectId={string | null} />` — fit badge (fit/noFit/unknown/—), owner verdict override via `orchd_set_insight_fit_verdict` (verdict select + reasoning input), status select where `archived` REQUIRES non-empty `resolution_reasoning` (inline block message otherwise), source caption.

- [ ] **Step 1: RED.** Tests: chip select fires SetIdeaLifecycle; archive blocked without reasoning; orphan attach calls `orchd_set_idea_project` with the chosen id; verdict override passes reasoning. FAIL.
- [ ] **Step 2: GREEN + Step 3:** vitest + tsc PASS. Commit `feat(ui): ideas + insights lists — lifecycle chips, archive-with-reasoning, D11 set-verbs (S3 §10)`.

### Task 16: TasksList

**Files:** Create `src/components/TasksList.tsx` + test.
**Interfaces — Consumes:** T13. **Produces:** `<TasksList projectId={string} />` — six status groups in §4.2 order, rank-ordered inside; subtask indent under parent; status select; ▲/▼ = `orchd_set_task_rank(id, midpoint)` (midpoint between the two neighbors' ranks; top ⇒ `firstRank - 1024`; bottom ⇒ `lastRank + 1024`); create dialog (title, body, source select idea/insight/bug/plan, optional parent select, comma-split tags); delete warning «удалит N подзадач» when children exist.

- [ ] **Step 1: RED.** Tests: grouping+rank order; ▲ computes neighbor midpoint; top-move uses firstRank-1024; subtask indent; cascade warning count; create passes source. FAIL.
- [ ] **Step 2: GREEN + Step 3:** vitest + tsc PASS. Commit `feat(ui): task list — status groups, fractional rank moves, subtasks (S3 §10, Q9/Q11)`.

### Task 17: RulesetPanel

**Files:** Create `src/components/RulesetPanel.tsx` + test; modify `docs/design-system.md` (+2 rows: `| **Policy form** | numeric cap (empty=∞), chip inputs for classes/allowlist; client mirrors server validation |`, `| **File-state banner** | info banner (не amber): ExternallyModified → [Принять]; Missing → [Создать заново] |`).
**Interfaces — Consumes:** T13 (`rulesets`), wrappers `orchd_get_ruleset`/`orchd_upsert_ruleset`/`orchd_acknowledge_rule_file`/`orchd_reveal_rules_file`. **Produces:** `<RulesetPanel scope={"global" | "project"} projectId={string | null} />` — md textarea (save ⇒ `orchd_upsert_ruleset{mdContent}`), path row + «показать файл» (`orchd_reveal_rules_file(scope, projectId)` — no path from JS), file-state banners (`ExternallyModified` → [Принять] ⇒ acknowledge; `Missing` → [Создать заново] ⇒ upsert `mdContent: ""`), policy form (spend cap number, approval-classes chips, allowlist rows; client-side mirrors server rules; server `Validation` verbatim in toast). Refreshes on `orchd://ruleset-changed` + on mount.

- [ ] **Step 1: RED.** Tests: three file_state renders; Принять → acknowledge; save passes mdContent; negative cap blocked client-side; reveal invoked with scope/projectId only. FAIL.
- [ ] **Step 2: GREEN + Step 3:** vitest + tsc PASS. Commit `feat(ui): ruleset panel — files-as-truth states, policy form, path-free reveal (S3 §7/§10)`.

### Task 18: ProjectPanel + left-rail restructure + App wiring

**Files:** Create `src/components/ProjectPanel.tsx` + test, `src/components/CreateProjectDialog.tsx` + test; modify `src/components/WorkspaceSidebar.tsx` (replace the flat `list.map` at :92-121 with project groups), `src/App.tsx` (view === "project" branch), `docs/design-system.md` (+1 row `| **Project group row** | bold project header + nested workspace rows; «Без проекта» group last |`).
**Interfaces — Consumes:** T13–T17. **Produces:**
- `ProjectPanel` tabs: Обзор · Цели(`<GoalTree/>`) · Идеи(`<IdeasList projectId/>`) · Задачи(`<TasksList/>`) · Инсайты(`<InsightsList/>`) · Правила(`<RulesetPanel scope="project"/>`). Обзор: entity counters; linked workspaces (names resolved from the sessiond `workspaces` Record — unresolvable id ⇒ chip «workspace недоступен» + [Отвязать] ⇒ `orchd_remove_project_workspace`); add-workspace select (unlinked only); export: [Скопировать JSON] (clipboard + toast) AND [Сохранить в файл…] (`pickFolder` → `orchd_export_to_file`); import: [Импорт из файла…] (`pickFolder` → file list via existing `listDir` filtered `.json` → `orchd_import_from_file`).
- Sidebar: ⌂ Home (unchanged) → project groups (header row click ⇒ `openProject(id)`; nested workspace rows keep the EXACT current row rendering/click behavior) → «Без проекта» group (unlinked workspaces + [привязать] per row: project select → `orchd_add_project_workspace`) → «+ проект» → `CreateProjectDialog` (name, description, multi-select unlinked workspaces + inline «создать workspace» via existing `pickFolder`+`createWorkspace`; submit blocked with «нужен хотя бы один workspace» until ≥1 selected; then `orchd_create_project`).
- App: `view === "project"` renders `<ProjectPanel/>` (the existing home/workspace branches untouched).

- [ ] **Step 1: RED.** Tests: grouping (linked under headers, remainder in «Без проекта»); dialog submit-block at 0 workspaces; tab switching renders children (mocked); unresolvable chip + Отвязать; export copies JSON + export-to-file passes picked dir; App renders ProjectPanel on view=project. FAIL.
- [ ] **Step 2: GREEN + Step 3:** vitest + tsc PASS. Commit `feat(ui): project panel + rail project groups + create/link + export-import UI (S3 §10)`.

### Task 19: QuickCapture (⌘K) + HomeGoals + orchd honesty surfaces

**Files:** Create `src/components/QuickCapture.tsx` + test, `src/components/HomeGoals.tsx` + test, `src/components/OrchdDownBanner.tsx` (+ covered via consumers' tests); modify `src/App.tsx` (global keydown + `<QuickCapture/>` mount), `src/components/HomeView.tsx` (mount `<HomeGoals/>` AFTER the three attention sections — amber stays pinned-top per S2 §6.2), `src/components/UpgradeDialog.tsx` (generalize), `docs/design-system.md` (+1 row `| **Quick-capture overlay** | ⌘K portal; title+body+project select; Enter submits; Esc closes |`).
**Interfaces — Consumes:** T13. **Produces:**
- ⌘K (metaKey+K; ignored when focus is in an input/textarea/xterm): overlay with title (required), body, project select from `projects` + «без проекта» → `orchd_create_idea` → toast «идея сохранена»; while `orchdDown`: submit disabled + inline «оркестратор недоступен».
- `HomeGoals`: per active project — strategic goal title + direct additional children with status chips; click → `openProject(id)`; mounts BELOW «Завершились недавно».
- `OrchdDownBanner`: «Оркестратор недоступен» + [Повторить] → `orchd_reconnect` command (thin retry wrapper added in commands.rs if not present from T11/T12 — LOCK: add `orchd_reconnect` in T12); rendered by ProjectPanel + HomeGoals + QuickCapture when `orchdDown`.
- `UpgradeDialog` GENERALIZED (stays prop-less, store-gated): reads both flag pairs (`daemonIncompatible`/`upgradeDialogOpen` AND `orchdIncompatible`/`orchdUpgradeDialogOpen`); renders ONE dialog — sessiond takes precedence if both; orchd copy «Обновить фоновый сервис оркестратора — записи (проекты, цели, задачи) сохранены»; confirm → `orchd_upgrade` command (T12 provides: drain `OrchdShutdown{drain:true}` best-effort → `kickstart_force()` on the orchd agent → `app.restart()` — mirror `upgrade_daemon` verbatim; LOCK: add `orchd_upgrade` + its stub test in T12).

- [ ] **Step 1: RED.** Tests: ⌘K toggles, ignored in inputs; submit fires CreateIdea with null project; HomeGoals renders + navigates + sits after the attention sections (DOM order assertion); banner Повторить invokes reconnect; orchd-incompatible alone renders orchd copy; both-incompatible renders sessiond copy. FAIL.
- [ ] **Step 2: GREEN + Step 3:** vitest + tsc PASS. Commit `feat(ui): ⌘K idea capture + Home goals (below attention) + orchd honesty + dual-daemon upgrade dialog (S3 §10/§11, D3)`.

## Phase 6 — close

### Task 20: e2e + gate extension

**Files:** Create `tests/e2e/orchd-survive.mjs`; modify `tests/e2e/lib/daemon-harness.mjs` (ADDITIVE: `connect(sockPath, opts?)` gains optional `{clientMin, clientMax}` defaulting to the current `[3,3]` — the sessiond script needs NO change; the CBOR codec/framing are already protocol-agnostic), `package.json` (`"e2e:orchd": "node tests/e2e/orchd-survive.mjs"`), `scripts/final-suite.sh` (renumber headers `N/8`→`N/9` incl. the top comment block; stage 6 adds `cargo test -p bpa-orchd-proto --test ts_export` + `git diff --exit-code -- src/ipc/orchd-types.ts`; stage 8 also `cargo build -p bpa-orchd`; new `== 9/9 e2e orchd survive+roundtrip ==` → `npm run e2e:orchd`), `scripts/coverage-gate.sh` (add `cargo llvm-cov --package bpa-orchd --fail-under-lines 80`), `.github/workflows/ci.yml` (lockstep — mirror whatever stages it enumerates).
**Interfaces — Produces (spec §12 exact phases, log format `[e2e-orchd] phaseN OK: …`, final `[e2e-orchd] ALL PHASES PASSED`):** phase0 boot `target/debug/bpa-orchd` on temp HOME (`spawnDaemon(ORCHD_BIN, SOCK, env)`) → handshake `[1,1]`; phase1 CreateProject + 2×CreateGoal + CreateIdea + CreateTask, assert entities; phase2 `OrchdShutdown{drain:true}` → relaunch → ListProjects/ListGoals intact; phase3 `ExportAll` captured; phase4 shutdown → delete `orchd.db*` → relaunch (fresh v1) → `ImportBundle` → re-`ExportAll` deep-equal phase3 modulo `exportedAt`.

- [ ] **Step 1:** Write script; `cargo build -p bpa-orchd && npm run e2e:orchd` → `ALL PHASES PASSED`.
- [ ] **Step 2:** Extend gate files; `bash scripts/final-suite.sh` → `ALL GATES PASSED` (9 stages; coverage shortfalls fixed by ADDING orchd tests, never by lowering the bar).
- [ ] **Step 3:** Commit `test(e2e): orchd survive-restart + export/import round-trip; gate → 9 stages + orchd coverage (S3 §12)`.

### Task 21: docs truth + CHANGELOG

**Files:** Create `docs/runbook-orchd.md` (mirror runbook-daemon.md with orchd's REAL names — spec §2.1 table: label `ai.builderpro.desktop.orchd`, `orchd.sock`/`orchd.lock`, `orchd.db`, `orchd.tracing.log`/`orchd.out.log`/`orchd.err.log`). Modify per spec §13's ENUMERATED list: `docs/architecture.md` ("two OS processes"→three; diagram; "Hop B" singular→both; orchd `chosen == 1`; module map += daemon-core/orchd-proto/orchd/orchd_client; "not built yet"→shipped; **"orchd gets its own file API in S9" reconciled with the D4 narrow rules-md exception**), overview (§3 S3 row → SHIPPED + deltas D2/D3/D4/D5 + S2-dependency note; «Current slice» → next; §2 survival table += orchd-data row), `README.md` (status += S3; features += orchd + 6 families; RE-MEASURED test counts — run `cargo test --workspace` + `npx vitest run` and write the REAL numbers, was 384/297; "8 stages"→9; coverage line += bpa-orchd), `CHANGELOG.md` (`[0.4.0]`), `docs/traceability.md` (S3 rows), `docs/design-system.md` (sweep: T14-T19 rows all present), `docs/backlog.md` (re-target BL-4/8/9/30 → next sessiond cycle + BL-50/51/52 → S4/S5 window, each with one-line reasoning; annotate BL-34 += orchd; ADD: un-archive verb, chunked export (16 MiB cap), panel-level cross-project rank (Q9, S5 additive), spawn-project-from-idea UI (S-IDEA)).

- [ ] **Step 1:** All edits; sanity `git grep -niE 'todo|tbd' docs/runbook-orchd.md docs/architecture.md` → empty; `git grep -n 'ai.builderpro.orchd[^.]' docs/` → empty (only `.desktop.orchd`).
- [ ] **Step 2:** `bash scripts/final-suite.sh` → `ALL GATES PASSED`.
- [ ] **Step 3:** Commit `docs: S3 shipped — two-daemon topology, runbook-orchd, survival row, CHANGELOG [0.4.0]`.

### Task 22: whole-branch adversarial review + merge

- [ ] **Step 1:** `scripts/review-package $(git merge-base main HEAD) HEAD`; multi-lens adversarial review (lenses: sessiond-regression-from-extraction (diff every re-seated path; on-disk names; plist bytes), wire-contract drift (enums vs spec §4.2 verbatim; version consts at every call site incl. stubs/e2e), SQL/invariant honesty (§5.2 vs tests; FK cascades; archived guards), ruleset file safety (atomic, containment, no-content-in-logs, post-commit file-write honesty), import atomicity + field-verbatim + collision + frame-cap, frontend honesty (down-banner coverage, disabled mutations, HomeGoals below amber, dual-dialog precedence), cross-daemon isolation (orchd failure never touches terminal flows)). Verify → fix waves → re-gate (Pv2/S2 T15 pattern).
- [ ] **Step 2:** `superpowers:finishing-a-development-branch` — verify gate, present options; on merge: ff-merge to main, re-run gate on main, push origin.

---

## Self-review (done at write time, post-audit)

Spec coverage: §2.1 names→Global Constraints+T5/T11/T21; §2.2 phases→graph; §3 six modules→T1(dirs/singleton/logging) T2(migrate, whole-chain semantics) T3(handshake+broadcast); §4.1→T3; §4.2 incl. D11 set-verbs→T4, dispatch→T10, commands→T12, ipc→T13; §5/§5.1→T5; §5.2 (incl. rank base 1024, archived guard, auto-creates)→T6/T7/T10; §6→T10/T12; §7→T8/T17 + post-commit file-write honesty→T10; §8 (field-verbatim, frame-cap, mdContent-null)→T9/T18; §9 (client mirror, launchd params, bring_up, reveal/export/import flows, IncompatibleOrchd, orchd_upgrade/orchd_reconnect)→T11/T12/T19; §10 (view widening ×2, rail, panel, ⌘K, HomeGoals BELOW, dual dialog)→T13-T19; §11 matrix→T10/T17/T18/T19 tests; §12 (harness param, tests/e2e path, 9 stages, coverage-gate.sh, ci.yml)→T20; §13 enumerated→T21; §14/§15 honored (no spawn-flow/panel-rank/kanban tasks). Code-truth: all file paths, line anchors, API names, literals (`d.sock`, LABEL, `[e2e]` format, `connect_to_stub`, `store.ts:57`) verified by the 2026-07-13 4-agent audit. Type consistency: `DomainTask`/`OrchdErrorCode`/`ORCHD_*_VERSION`/`OrchdClientError::IncompatibleOrchd`/`CommandError::IncompatibleOrchd`/event consts/`orchd_*` command names consistent across T4/T10/T11/T12/T13. No placeholders. Parallel group {T14-T17} shares only design-system.md distinct rows (controller serializes on conflict).
