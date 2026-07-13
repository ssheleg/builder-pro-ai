# S3 — `bpa-orchd` + App-Domain Foundation (Projects · Goals · Ideas · Insights · Tasks · RuleSet)

**Date:** 2026-07-13 (amended same day: 4-agent audit vs vision/prior-specs/code — see §15)
**Status:** Approved (brainstorm 2026-07-13; owner decisions D1–D12 below)
**Depends on:** S0, S2 (`[0.3.0]` shipped — left-rail/Home/HomeView build on S2 UI; recorded as a
delta vs the roadmap row's "S0, ADR-HOST"), ADR-HOST (platform overview §2), Data-layer charter
**Roadmap row:** overview §3 S3 — "Projects + Goal hierarchy + Ideas + Tasks/Subtasks + RuleSet
data model". DoD: goals+ideas+tasks CRUD survive restart; Project⇄Workspace enforced;
export/import round-trips.

---

## 0. Goal

Ship the second launchd daemon **`bpa-orchd`** (ADR-HOST's app-domain host) with domain schema v1
and full CRUD for the six S3 entity families, plus a working owner-facing UI: project management,
goal-tree editor, ideas inbox with ⌘K quick-capture, flat task list, insights list, RuleSet
editor, per-project export/import. After S3 the platform has its final two-daemon topology; S4/S5/
S-EXT/SW1 build on this store without re-architecting.

## 1. Owner decisions (locked this cycle)

| # | Decision |
|---|---|
| **D1** | **Bootstrap `bpa-orchd` in S3** (not an interim core-hosted store). Charter-true: one host forever. |
| **D2** | **Final architecture immediately**: extract shared **`bpa-daemon-core`** crate FIRST (six modules: dirs, singleton, logging, migrate, handshake, broadcast), re-seat `bpa-sessiond` on it (full gate green = phase gate), THEN build orchd on top. No "conscious duplication now, refactor later". |
| **D3** | **Extended UI**: CRUD UI + pulled-forward ⌘K idea quick-capture (from S-IDEA) + goals panel on Home (from SH). NOT pulled: research pipeline (needs S-EXT), kanban board (S5), fit-test agent (S-IDEA), **spawn-project-from-idea flow (S-IDEA — the data enabler `Idea.project_id: Option` ships now, the UI flow does not; §14)**. |
| **D4** | **RuleSet markdown layer: files are the source of truth**; DB stores `md_path` + `md_hash` (sha256). External edits/deletions surfaced honestly (§7). This is a deliberate NARROW exception to the "orchd gets its own file API in S9" line (S2 spec §9 / architecture.md): orchd touches exactly ONE file family (rules md), app-support-defaulted, atomic-write, never a general file API. T21 amends the architecture.md wording. |
| **D5** | **Goal hierarchy = full tree from day one**: exactly one `strategic` root per project, `additional` subgoals at arbitrary depth via `parent_id`. (Deliberate superset of the charter's "1 strategic + N additional" two-level wording — recorded as an S3 delta in the overview row, T21.) |
| **D6** | **Coarse-grained invalidation pushes** (`ProjectsChanged`, `GoalsChanged{project_id}`, …) — GUI re-fetches lists; no full-entity push mirroring (hundreds of rows, re-fetch is cheap; zero snapshot-drift risk). |
| **D7** | **Export/import**: per-project JSON bundle + whole-store variant, `bundleFormat: 1`, ids AND all row fields preserved verbatim, import-into-empty → re-export is semantically identical (§8). Id collision on import ⇒ typed `Conflict`, no silent merge. |
| **D8** | **Protocol**: orchd reuses the Pv2 preamble verbatim (same `BPAA` magic — daemons are distinguished by socket path; the preamble carries no single-daemon assumption, Pv2 amendment §3) with its **own independent version space `[1,1]`**; own frame enums in a new `bpa-orchd-proto` crate; `bpa-protocol` framing generalized additively (§4.1). |
| **D9** | **No new external dependencies.** Every crate S3 needs is already in the resolved graph and exercised in-repo: `rusqlite`, `ciborium`, `ts-rs`, `uuid`, `sha2` (transitive today — gets a `[workspace.dependencies]` entry), `tokio`, `thiserror`, `libc`. Context7 check: not applicable — no new/changed external API is being locked from memory; all patterns copied from working in-repo code (sessiond persistence/socket/preamble). |
| **D10** | **Event naming keeps the `orchd://` prefix** (`orchd://projects-changed`, not `project://changed`): these are coarse daemon-scoped invalidation signals, not resource lifecycle events; mirrors the existing `daemon://…` trio and leaves the resource namespaces free for future fine-grained events. Recorded as a conscious deviation from S2's resource-scoped style. |
| **D11** | **No `Option<Option<T>>` on the wire.** ts-rs collapses double-options and JSON drops absent keys — nullable-field updates get DEDICATED verbs instead (`SetIdeaProject`, `SetInsightFitVerdict`, §4.2). Plain `Option<T>` update fields exist only for NON-nullable columns, semantics "absent/null = unchanged". |
| **D12** | **Hop-B wire errors stay `Error { code, message }`** (with `code` upgraded to a typed enum) — this matches the sessiond Hop-B precedent (`Response::Error{code: String, message}`); S2's tagged-union error convention (`FsError`, `CommandError`) is the CORE-LOCAL (Hop-A) convention and is honored there (§9). |

## 2. Architecture

### 2.1 Crate topology (final)

```
crates/
  paths          (unchanged)
  protocol       (bpa-protocol) — terminal Hop-B wire; §4.1 additive framing generalization.
                  Stays the home of the SHARED preamble+framing primitives (conscious choice:
                  no neutral "wire-core" crate churn; daemon-core + orchd-proto depend on it)
  daemon-core    (NEW, bpa-daemon-core) — shared daemon infrastructure (§3)
  sessiond       (bpa-sessiond) — re-seated on daemon-core; behavior byte-identical
  orchd-proto    (NEW, bpa-orchd-proto) — orchd wire enums + version consts + ts-rs (§4.2)
  orchd          (NEW, bpa-orchd) — the second daemon (§5, §6)
src-tauri        — second client `orchd_client.rs`, broker events, `orchd_*` commands (§9)
src/             — domain store slice + UI components (§10)
```

Two daemons are fully independent, side by side (real current names on the sessiond side —
verified against code):

| | `bpa-sessiond` (existing) | `bpa-orchd` (new) |
|---|---|---|
| Socket | `{runtime_dir}/d.sock` | `{runtime_dir}/orchd.sock` |
| Lockfile | `{runtime_dir}/d.lock` | `{runtime_dir}/orchd.lock` |
| runtime_dir | `$XDG_RUNTIME_DIR/bpa` else `/tmp/bpa-{uid}` (shared resolution, daemon-core) | same |
| DB | `{app-support}/bpa.db` | `{app-support}/orchd.db` |
| launchd label | `ai.builderpro.desktop.sessiond` | **`ai.builderpro.desktop.orchd`** |
| plist | RENDERED AT RUNTIME by `src-tauri/src/launchd.rs` into `~/Library/LaunchAgents` (no repo file) | same mechanism, parameterized (§9) |
| tracing log | `{app-support}/logs/sessiond.tracing.log` | `{app-support}/logs/orchd.tracing.log` |
| launchd out/err | `logs/sessiond.out.log` / `.err.log` | `logs/orchd.out.log` / `.err.log` |
| Wire version | `[3,3]` | `[1,1]` (independent space) |

`{app-support}` = `~/Library/Application Support/ai.builderpro.desktop` (shared dir, both DBs).
GUI holds two connections; failure of either degrades only its own panels (§11).

### 2.2 Implementation phases (risk order, plan-binding)

1. **Phase 1 — extraction:** create `bpa-daemon-core`; move/parameterize the six shared modules
   (§3); re-seat sessiond via thin wrappers so its call sites and ON-DISK NAMES do not change.
   Phase gate: full `final-suite.sh` green (sessiond 168 unit + boot integration + no_secrets +
   rehydrate + e2e are the regression net).
2. **Phase 2 — orchd skeleton:** `bpa-orchd-proto` + `bpa-orchd` boot/singleton/socket/DB-v1 +
   launchd wiring + Ping/Shutdown. Phase gate: orchd boots, handshakes `[1,1]`, survives restart.
3. **Phase 3 — domain:** persistence CRUD + invariants → dispatch + pushes → RuleSet files →
   export/import → core client/commands → frontend slice → UI → e2e/gate → docs → review+merge.

## 3. `bpa-daemon-core` — extraction contract

Extract from sessiond, parameterizing hardcoded names. Sessiond keeps its public API via thin
wrappers (integration tests and call sites unchanged; on-disk paths byte-identical). Verified
source facts are noted per row. Modules and locked APIs:

| Module | Locked public API | Source (verified) |
|---|---|---|
| `dirs` | `pub fn app_support_dir() -> PathBuf` | `sessiond/boot.rs:19` — currently `pub(crate)`, body: `$HOME` (fallback `/tmp`) + `Library/Application Support/ai.builderpro.desktop`. Becomes `pub` here; sessiond re-exports `app_support_dir_for_test` unchanged |
| `singleton` | `pub struct LockGuard; pub fn acquire_lock_at(path: &Path) -> io::Result<LockGuard>` (today PRIVATE in sessiond — promoted); `pub fn acquire_single_instance_lock(lock_file_name: &str) -> io::Result<LockGuard>`; `pub fn resolve_socket_path(file_name: &str) -> PathBuf`; `pub fn resolve_lockfile(file_name: &str) -> PathBuf`; `pub fn ensure_socket_dir() -> io::Result<()>`; `pub fn assert_socket_path_len(&Path) -> io::Result<()>`; `pub fn set_socket_mode(&Path) -> io::Result<()>`; `pub fn check_peer_cred(BorrowedFd<'_>) -> io::Result<()>` | `sessiond/singleton.rs` — literals today: `"d.sock"`/`"d.lock"` under `$XDG_RUNTIME_DIR/bpa` else `/tmp/bpa-{uid}`; sessiond wrappers pass exactly `"d.sock"`/`"d.lock"`; `acquire_lock_at_for_test` stays `#[doc(hidden)]` in the sessiond wrapper |
| `logging` | (a) PRODUCTION: `pub fn init_tracing(log_file_name: &str) -> …` — extracted from `sessiond/main.rs::init_tracing` (lines 48-76): `{app-support}/logs` dir, chmod `0o700`, `tracing_appender::rolling::never(dir, log_file_name)`, non-ANSI; sessiond `main.rs` re-seats passing `"sessiond.tracing.log"`. (b) TEST SEAM: `pub fn init_to_file(path: &Path) -> io::Result<()>; pub fn flush()` moved as-is from `sessiond/logging.rs` (it is test-only today — module doc says so) | `sessiond/main.rs:48-76` + `sessiond/logging.rs` |
| `migrate` | `pub struct Migration { pub upto: i64, pub apply: fn(&rusqlite::Transaction) -> rusqlite::Result<()> }`; `pub fn run_migrations(conn: &Connection, from_version: i64, target: i64, steps: &[Migration]) -> Result<(), MigrateError>` — EXACT sessiond semantics: early-return when `from_version == target`; `from_version > target` ⇒ `MigrateError::VersionTooNew { found, supported }`; otherwise ONE `unchecked_transaction()` for the WHOLE chain, apply every step where `from_version < step.upto` in order, single `pragma_update(user_version, target)` INSIDE the tx, commit; any error ⇒ whole-chain rollback, version untouched (fail-closed). `pub enum MigrateError { VersionTooNew { found: i64, supported: i64 }, Sql(rusqlite::Error) }` | `sessiond/persistence.rs:191-253` — NOT one-tx-per-step; sessiond keeps `PersistError::Migration` + `.code() == "DbMigration"` by wrapping `MigrateError` |
| `handshake` | `pub async fn server_handshake(stream: &mut UnixStream, min: u16, max: u16, build: &str) -> io::Result<Option<u16>>` — moves `read_client_preamble` (+ `CLIENT_PREAMBLE_HEADER_LEN = 10`) out of `sessiond/socket_server.rs:748-776`; `PREAMBLE_TIMEOUT`-bounded read AND reply-write; `negotiate(client.min, client.max, min, max)`; `Ok(Some(chosen))` accepted (build filled from the `build` param), `Ok(None)` = Incompatible written, caller closes; malformed/timeout ⇒ `Err` (sessiond's current "quiet `Ok(())`" wrapper behavior is preserved at ITS call site) | `sessiond/socket_server.rs:606-643` |
| `broadcast` | `pub struct Broadcaster<F: Clone + Send + 'static> { … }` — generic extraction of sessiond's fan-out: `HashMap<u64, tokio::sync::mpsc::Sender<F>>` behind `Arc<Mutex<…>>`; `pub fn register(&self, id: u64, tx: mpsc::Sender<F>)`, `deregister(&self, id: u64)`, `broadcast(&self, f: F)` using non-blocking `try_send`, full/closed silently skipped (one dead client never blocks fan-out) | `sessiond/socket_server.rs:211-231` — sessiond re-seats as `Broadcaster<Frame>` |

Rule: extraction moves code WITH its tests; NO behavior change; a dedicated sessiond test asserts
resolved socket/lock paths and the rendered plist are byte-identical pre/post re-seat.

## 4. Wire protocol

### 4.1 `bpa-protocol` additive framing generalization

`framing.rs` today is fully concrete (`encode_frame(&Frame)`, `FrameDecoder` with hardcoded
`ciborium::from_reader::<Frame>`; `FrameError{Oversized, Decode, Encode}`; `MAX_FRAME_LEN` =
16 MiB). It gains a generic core; the existing API stays as thin instantiations:

```rust
pub fn encode_cbor_frame<T: serde::Serialize>(v: &T) -> Result<Vec<u8>, FrameError>;
pub struct CborFrameDecoder<T> { /* buf: Vec<u8> + PhantomData<T> */ }
impl<T: serde::de::DeserializeOwned> CborFrameDecoder<T> {
    pub fn new() -> Self;
    pub fn push(&mut self, chunk: &[u8]);
    pub fn decode(&mut self) -> Result<Vec<T>, FrameError>;
}
```

`encode_frame`/`FrameDecoder` keep their exact signatures (implemented over the generics).
`MAX_FRAME_LEN` shared. Preamble module unchanged (D8): `negotiate(client_min, client_max,
daemon_min, daemon_max)` is ALREADY a pure function of both ranges — orchd passes `(1,1)`.

### 4.2 `bpa-orchd-proto` — wire contract (LOCKED, enum order FROZEN append-only from day one)

Version consts: `pub const ORCHD_CLIENT_MIN_VERSION: u16 = 1;` (same for MAX, DAEMON_MIN/MAX — all
`1`). All types `#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]`, entity/enum types
additionally `#[derive(TS)]` with `#[serde(rename_all = "camelCase")]` on structs and enum
variants (mirror `bpa-protocol` style; wire/TS strings camelCase — `inDev`, `noFit`; the SQL
TEXT literals in §5.1 are snake_case — the persistence layer owns that mapping).
ts-rs export target: `src/ipc/orchd-types.ts` via `#[ts(export_to = "orchd-types.ts")]` +
`export_all_to("../../src/ipc")` (mirror `crates/protocol/tests/ts_export.rs`).

```rust
// ---- entities ----
pub struct Project { pub id: String, pub name: String, pub description: String,
    pub status: ProjectStatus, pub workspace_ids: Vec<String>, // ordered, soft refs to sessiond
    pub created_at: i64, pub updated_at: i64 }
pub enum ProjectStatus { Active, Archived }

pub struct Goal { pub id: String, pub project_id: String, pub parent_id: Option<String>,
    pub kind: GoalKind, pub title: String, pub body: String, pub ord: i64,
    pub status: GoalStatus, pub metric_refs: Vec<String>, pub created_at: i64, pub updated_at: i64 }
pub enum GoalKind { Strategic, Additional }
pub enum GoalStatus { Active, Achieved, Dropped }

pub struct Idea { pub id: String, pub project_id: Option<String>, pub title: String,
    pub body: String, pub lifecycle: IdeaLifecycle, pub created_at: i64, pub updated_at: i64 }
pub enum IdeaLifecycle { Captured, Researching, Specced, InDev, Shipped, Archived }

pub struct Insight { pub id: String, pub project_id: Option<String>, pub source: String,
    pub title: String, pub body: String, pub fit_verdict: Option<FitVerdict>,
    pub fit_reasoning: String, pub status: InsightStatus, pub resolution_reasoning: String,
    pub created_at: i64, pub updated_at: i64 }
pub enum FitVerdict { Fit, NoFit, Unknown }
pub enum InsightStatus { New, Accepted, Archived }

pub struct DomainTask { pub id: String, pub project_id: String, pub parent_id: Option<String>,
    pub title: String, pub body: String, pub status: TaskStatus, pub source: TaskSource,
    pub source_id: Option<String>, pub tags: Vec<String>, pub rank: f64,
    pub rank_agent: Option<f64>, pub rank_agent_reasoning: String,
    pub created_at: i64, pub updated_at: i64 }   // named DomainTask to avoid tokio::task clash
pub enum TaskStatus { Backlog, Todo, Waiting, Progress, Testing, Done }
pub enum TaskSource { Idea, Insight, Bug, Plan }

pub struct PolicyRules { pub spend_cap_usd: Option<f64>, pub approval_classes: Vec<String>,
    pub path_allowlist: Vec<String> }
pub struct RuleSet { pub id: String, pub scope: RuleScope, pub project_id: Option<String>,
    pub md_path: String, pub md_hash: String, pub policy: PolicyRules,
    pub created_at: i64, pub updated_at: i64 }
pub enum RuleScope { Global, Project }
pub enum RuleFileState { Ok, Missing, ExternallyModified }
pub struct RuleSetView { pub rule: RuleSet, pub md_content: Option<String>,
    pub file_state: RuleFileState }

pub enum OrchdErrorCode { NotFound, Invariant, Validation, Conflict, Io }

// ---- frames ----
pub enum OrchdRequest {
    Ping,
    // Project
    CreateProject { name: String, description: String, workspace_ids: Vec<String> }, // ≥1 enforced
    UpdateProject { id: String, name: Option<String>, description: Option<String> },
    ArchiveProject { id: String },
    ListProjects,
    AddProjectWorkspace { project_id: String, workspace_id: String },
    RemoveProjectWorkspace { project_id: String, workspace_id: String }, // last one ⇒ Invariant
    // Goal
    CreateGoal { project_id: String, parent_id: Option<String>, kind: GoalKind,
                 title: String, body: String },
    UpdateGoal { id: String, title: Option<String>, body: Option<String>,
                 status: Option<GoalStatus>, metric_refs: Option<Vec<String>> },
    MoveGoal { id: String, new_parent_id: Option<String>, new_ord: i64 }, // same project only
    DeleteGoal { id: String },            // cascades subtree; strategic root ⇒ Invariant
    ListGoals { project_id: String },
    // Idea
    CreateIdea { project_id: Option<String>, title: String, body: String },
    UpdateIdea { id: String, title: Option<String>, body: Option<String> },
    SetIdeaProject { id: String, project_id: Option<String> },   // D11: dedicated verb, None=detach
    SetIdeaLifecycle { id: String, lifecycle: IdeaLifecycle },
    DeleteIdea { id: String },
    ListIdeas { project_id: Option<String> }, // None ⇒ ALL ideas incl. orphans
    // Insight
    CreateInsight { project_id: Option<String>, source: String, title: String, body: String },
    UpdateInsight { id: String, title: Option<String>, body: Option<String> },
    SetInsightFitVerdict { id: String, fit_verdict: Option<FitVerdict>,
                           fit_reasoning: String },              // D11: dedicated verb
    SetInsightStatus { id: String, status: InsightStatus, resolution_reasoning: Option<String> },
    DeleteInsight { id: String },
    ListInsights { project_id: Option<String> }, // None ⇒ ALL
    // Task
    CreateTask { project_id: String, parent_id: Option<String>, title: String, body: String,
                 status: Option<TaskStatus>, source: TaskSource, source_id: Option<String>,
                 tags: Vec<String> },
    UpdateTask { id: String, title: Option<String>, body: Option<String>,
                 tags: Option<Vec<String>> },
    SetTaskStatus { id: String, status: TaskStatus },
    SetTaskRank { id: String, rank: f64 },
    DeleteTask { id: String },            // cascades subtasks
    ListTasks { project_id: Option<String> },
    // RuleSet
    GetRuleSet { scope: RuleScope, project_id: Option<String> },   // → RuleSetView
    UpsertRuleSet { scope: RuleScope, project_id: Option<String>,
                    md_content: Option<String>,  // Some ⇒ write file + rehash
                    md_path: Option<String>,     // Some ⇒ repoint (validated absolute)
                    policy: Option<PolicyRules> },
    AcknowledgeRuleFile { id: String },   // re-read file → store new hash (or report Missing)
    // Export / import
    ExportProject { project_id: String }, // → Response::ExportJson
    ExportAll,                            // → Response::ExportJson
    ImportBundle { json: String },        // → Response::ImportReport
    // Daemon
    OrchdShutdown { drain: bool },
}

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
    ImportReport { projects: u32, goals: u32, ideas: u32, insights: u32, tasks: u32,
                   rulesets: u32 },
    Error { code: OrchdErrorCode, message: String },
}

pub enum OrchdPush {
    ProjectsChanged,
    GoalsChanged { project_id: String },
    IdeasChanged,
    InsightsChanged,
    TasksChanged { project_id: String },
    RuleSetChanged { scope: RuleScope, project_id: Option<String> },
}

pub enum OrchdFrame {
    Request { id: u64, req: OrchdRequest },
    Response { id: u64, res: OrchdResponse },
    Push(OrchdPush),
}
```

`Update*` `Option<T>` fields exist only on NON-nullable columns; absent OR `null` both mean
"unchanged" (D11 — no double-options anywhere). Framing: `pub fn encode_orchd_frame(&OrchdFrame)`
+ `pub type OrchdFrameDecoder = CborFrameDecoder<OrchdFrame>` over the §4.1 generics.

## 5. `bpa-orchd` daemon

Modules mirror sessiond minus PTY: `main.rs` (tokio main, `daemon_core::logging::init_tracing
("orchd.tracing.log")`, watch shutdown, SIGTERM), `boot.rs` (singleton via
`resolve_lockfile("orchd.lock")` → DB open-degrading → global-ruleset ensure → socket serve),
`socket_server.rs` (accept → `daemon_core::handshake::server_handshake(1, 1, build)` with
`build = env!("CARGO_PKG_VERSION")` → dispatch → `Broadcaster<OrchdFrame>`), `persistence.rs`
(schema v1 + CRUD), `ruleset_files.rs` (§7), `export.rs` (§8).
`pub async fn run(socket: PathBuf, shutdown_tx: watch::Sender<bool>, shutdown_rx:
watch::Receiver<bool>) -> std::io::Result<()>` — mirror of `bpa_sessiond::run`'s exact shape.
DB open mirrors sessiond `open_db_degrading`: disk failure ⇒ `tracing::error!` + in-memory
fallback (honest degradation); panic only if SQLite itself is unusable. Rules md content is
never logged (no-secrets discipline; enforced by an orchd `no_secrets_in_logs`-style test).

### 5.1 `orchd.db` schema v1 (LOCKED DDL)

Applied as `Migration { upto: 1 }` via `daemon_core::migrate::run_migrations`. In `open_inner`:
`journal_mode=WAL`, `busy_timeout=5000`, **`foreign_keys=ON`** (sessiond parity + orchd actually
uses FK cascades); in-memory variant: no WAL, same busy_timeout + foreign_keys.

```sql
CREATE TABLE project (
  id TEXT PRIMARY KEY, name TEXT NOT NULL, description TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active','archived')),
  created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL
);
CREATE TABLE project_workspace (
  project_id TEXT NOT NULL REFERENCES project(id) ON DELETE CASCADE,
  workspace_id TEXT NOT NULL UNIQUE,           -- one project per workspace (soft sessiond ref)
  ord INTEGER NOT NULL,
  PRIMARY KEY (project_id, workspace_id)
);
CREATE TABLE goal (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES project(id) ON DELETE CASCADE,
  parent_id TEXT REFERENCES goal(id) ON DELETE CASCADE,
  kind TEXT NOT NULL CHECK (kind IN ('strategic','additional')),
  title TEXT NOT NULL, body TEXT NOT NULL DEFAULT '', ord INTEGER NOT NULL DEFAULT 0,
  status TEXT NOT NULL DEFAULT 'active' CHECK (status IN ('active','achieved','dropped')),
  metric_refs TEXT NOT NULL DEFAULT '[]',      -- JSON array of strings (Q12 forward)
  created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL
);
CREATE UNIQUE INDEX goal_one_strategic_per_project ON goal(project_id) WHERE kind='strategic';
CREATE INDEX goal_by_project ON goal(project_id);
CREATE TABLE idea (
  id TEXT PRIMARY KEY,
  project_id TEXT REFERENCES project(id) ON DELETE SET NULL,   -- orphaning keeps the idea
  title TEXT NOT NULL, body TEXT NOT NULL DEFAULT '',
  lifecycle TEXT NOT NULL DEFAULT 'captured'
    CHECK (lifecycle IN ('captured','researching','specced','in_dev','shipped','archived')),
  created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL
);
CREATE INDEX idea_by_project ON idea(project_id);
CREATE TABLE insight (
  id TEXT PRIMARY KEY,
  project_id TEXT REFERENCES project(id) ON DELETE SET NULL,
  source TEXT NOT NULL DEFAULT '', title TEXT NOT NULL, body TEXT NOT NULL DEFAULT '',
  fit_verdict TEXT CHECK (fit_verdict IN ('fit','no_fit','unknown')),
  fit_reasoning TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL DEFAULT 'new' CHECK (status IN ('new','accepted','archived')),
  resolution_reasoning TEXT NOT NULL DEFAULT '',
  created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL
);
CREATE INDEX insight_by_project ON insight(project_id);
CREATE TABLE task (
  id TEXT PRIMARY KEY,
  project_id TEXT NOT NULL REFERENCES project(id) ON DELETE CASCADE,
  parent_id TEXT REFERENCES task(id) ON DELETE CASCADE,
  title TEXT NOT NULL, body TEXT NOT NULL DEFAULT '',
  status TEXT NOT NULL DEFAULT 'backlog'
    CHECK (status IN ('backlog','todo','waiting','progress','testing','done')),
  source TEXT NOT NULL CHECK (source IN ('idea','insight','bug','plan')),
  source_id TEXT, tags TEXT NOT NULL DEFAULT '[]',             -- JSON array of strings
  rank REAL NOT NULL, rank_agent REAL, rank_agent_reasoning TEXT NOT NULL DEFAULT '',
  created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL
);
CREATE INDEX task_by_project_status ON task(project_id, status);
CREATE INDEX task_by_parent ON task(parent_id);
CREATE TABLE ruleset (
  id TEXT PRIMARY KEY,
  scope TEXT NOT NULL CHECK (scope IN ('global','project')),
  project_id TEXT UNIQUE REFERENCES project(id) ON DELETE CASCADE,
  md_path TEXT NOT NULL, md_hash TEXT NOT NULL DEFAULT '',
  policy TEXT NOT NULL DEFAULT '{}',                            -- JSON PolicyRules
  created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL,
  CHECK ((scope='global' AND project_id IS NULL) OR (scope='project' AND project_id IS NOT NULL))
);
CREATE UNIQUE INDEX ruleset_single_global ON ruleset(scope) WHERE scope='global';
-- user_version set to 1 by the migration runner
```

Timestamps unix-ms. Ids `uuid::Uuid::new_v4().to_string()`.

### 5.2 Invariants (enforced in persistence layer, each a typed error)

| Invariant | Error |
|---|---|
| `CreateProject` requires ≥1 `workspace_ids`; `RemoveProjectWorkspace` refuses the last link | `Invariant` |
| A workspace can be linked to at most one project (UNIQUE) | `Conflict` |
| Exactly one strategic goal per project, auto-created WITH the project (`title: "Стратегическая цель"`, empty body, owner edits it) — never deletable; `CreateGoal{kind: Strategic}` on a project that has one | `Invariant` |
| `MoveGoal`/`CreateGoal parent_id` must stay within one project; cycles rejected (walk-up check) | `Invariant` |
| Strategic root: `parent_id` always NULL; `MoveGoal` on it | `Invariant` |
| Task `parent_id` same-project + cycle-rejected (same walk-up) | `Invariant` |
| `rank` on create = `COALESCE(MAX(rank) FILTER project, 0) + 1024` (first task ⇒ `1024`); `SetTaskRank` takes an explicit f64 (fractional insert-between is the client's move) | — |
| Unknown ids anywhere | `NotFound` |
| `PolicyRules` strict-validated (`deny_unknown_fields`, `spend_cap_usd >= 0`, non-empty allowlist entries) | `Validation` |

Archived project: rows stay, reads work (honest history); EVERY mutating verb touching it or its
children ⇒ `Invariant`. Archive is one-way in v1; un-archive is a future additive verb (backlog
row, §13).

Auto-created rows (single tx with `CreateProject`): the strategic goal (above) AND the project's
ruleset row (`scope='project'`, default `md_path = {app-support}/rules/project-<uuid>.md`, file
written with a `# Правила проекта <name>` template header, `policy = '{}'`). The GLOBAL ruleset
row + `rules/global.md` are ensured at every orchd boot (idempotent).

## 6. Dispatch & error mapping

Every mutating request: reply to the requester (`OrchdResponse::<Entity>(updated)`; `Ack` for
deletes and `ImportReport` for import) AND broadcast the matching coarse push via
`Broadcaster<OrchdFrame>`: project verbs ⇒ `ProjectsChanged`; goal ⇒ `GoalsChanged{project_id}`;
idea ⇒ `IdeasChanged`; insight ⇒ `InsightsChanged`; task ⇒ `TasksChanged{project_id}`; ruleset ⇒
`RuleSetChanged{scope, project_id}`; `ImportBundle` ⇒ every push whose family the bundle touched.
Failed requests broadcast NOTHING.

`OrchdPersistError → OrchdErrorCode` mapping: `NotFound→NotFound`, `Invariant→Invariant`,
`Conflict→Conflict`, `Validation→Validation`, `Sql→Io`, `Io(String)→Io` (the non-SQL I/O
producer — e.g. the §8 export frame-cap guard constructs `OrchdPersistError::Io("export exceeds
the 16 MiB frame cap")`). Wire error shape `Error{code, message}` per D12.

## 7. RuleSet file layer (`ruleset_files.rs`)

- Default paths: global ⇒ `{app-support}/rules/global.md`; project ⇒
  `{app-support}/rules/project-<uuid>.md`. Owner may repoint `md_path` to any absolute path
  (e.g. a repo file) via `UpsertRuleSet{md_path}` — validated absolute + parent exists.
- `UpsertRuleSet{md_content: Some}`: create parent dirs, write file atomically (tmp+rename),
  `md_hash = sha256(content)` hex.
- `GetRuleSet`: read file fresh each time → `RuleSetView{ md_content, file_state }`:
  `Ok` (hash matches), `ExternallyModified` (exists, hash differs — content returned, info banner
  in UI with [Принять] → `AcknowledgeRuleFile`), `Missing` (`md_content: None`, UI offers
  «создать заново»).
- Deleting a ruleset row (only via project cascade) never deletes the file — honest note in UI.
- File content is never logged.
- No fs-watch on rules in v1 — freshness is read-on-Get + explicit Acknowledge (YAGNI; the GUI
  re-Gets on panel open and on `RuleSetChanged`).
- **Scope guard (D4):** this module is the ONLY file I/O in orchd; no general file API (that
  remains S9).

## 8. Export / import (`export.rs`)

Serde-serialized with the same camelCase entity structs (§4.2) — bundle keys are camelCase
(`workspaceIds`, `createdAt`, `metricRefs`, `fitVerdict`, …). Locked shapes:

```jsonc
// ExportProject
{ "bundleFormat": 1, "exportedAt": 1760000000000,
  "project": { /* Project */ },
  "goals": [ /* Goal */ ], "ideas": [ /* Idea */ ], "insights": [ /* Insight */ ],
  "tasks": [ /* DomainTask */ ],
  "ruleset": { "rule": { /* RuleSet */ }, "mdContent": "…" } | null
  // mdContent read live at export; missing file ⇒ "mdContent": null (null ≡ file missing;
  // an EMPTY file exports as ""). No separate mdMissing flag.
}
// ExportAll
{ "bundleFormat": 1, "exportedAt": …,
  "projects": [ /* per-project bundle objects, without bundleFormat/exportedAt */ ],
  "globalRuleset": { "rule": …, "mdContent": … } | null,
  "orphanIdeas": [ /* Idea with projectId null */ ],
  "orphanInsights": [ /* Insight with projectId null */ ] }
```

Import: single transaction; validates `bundleFormat == 1`; any id already present in the store ⇒
`Conflict{message: "<entity> <id> already exists"}`, transaction rolled back, nothing imported.
**Field-verbatim preservation (D7):** import writes every row field EXACTLY as exported —
`created_at`, `updated_at`, `rank`, `ord`, `md_hash` included; never re-stamped. Import inserts
raw rows in the tx (NOT the `CreateProject` verb) — §5.2 auto-creates never double-fire; the
bundle already carries the strategic goal + ruleset row, and the unique indexes back-stop any
malformed bundle. Ruleset md files are written ONLY when `md_path` is under app-support;
otherwise the import writes to the default app-support path and repoints — never touches files
outside app-support. Workspace soft-refs imported as-is; UI shows «workspace недоступен» for
unresolvable ids (§10). Accepts both bundle shapes (discriminate on `project` vs `projects` key).
**Size cap (honest):** an encoded `ExportJson` reply must fit `MAX_FRAME_LEN` (16 MiB shared cap);
export computes the serialized size first and answers `Error{Io, "export exceeds the 16 MiB frame
cap"}` instead of a doomed send. Chunked export = backlog row (§13); irrelevant at v0.x scale.
Round-trip guarantee (DoD): import into an empty store → `ExportAll` equals the original modulo
`exportedAt`.

## 9. Core integration (src-tauri)

- `orchd_client.rs` — MIRROR of `socket_client.rs` (verified structure): `OrchdClient::connect
  (client_build: String)` resolving `orchd.sock` via a LOCAL `socket_dir()` copy exactly as
  `socket_client.rs:108/:124` does today (src-tauri does NOT gain a `bpa-daemon-core` dep) +
  `connect_with_retry(build, attempts, delay)`; client preamble
  `[ORCHD_CLIENT_MIN_VERSION, ORCHD_CLIENT_MAX_VERSION]`; correlation = `AtomicU64` id +
  `HashMap<u64, oneshot::Sender<…>>` in a single connection task; pushes via callback registry
  `on_push(impl Fn(OrchdPush))` + `on_conn(impl Fn(ConnState))`; error enum
  `OrchdClientError { Disconnected, Daemon{code: String, message: String},
  IncompatibleOrchd{daemon_min: u16, daemon_max: u16}, RequestTooLarge{size} }`; slot type
  `OrchdClientSlot = Arc<RwLock<Option<Arc<OrchdClient>>>>`.
- `launchd.rs` — parameterized ADDITIVELY: `LaunchdAgent` gains `label: &'static str` +
  `stdout_log_name`/`stderr_log_name` fields (sessiond call sites pass the CURRENT values —
  `"ai.builderpro.desktop.sessiond"`, `"sessiond.out.log"`, `"sessiond.err.log"`; a test asserts
  the rendered sessiond plist is byte-identical pre/post). `resolve_daemon_path()` gains a
  binary-name param (`"bpa-sessiond"` / `"bpa-orchd"`, current_exe-sibling resolution unchanged).
  orchd label: **`ai.builderpro.desktop.orchd`**.
- Boot: `bring_up_orchd` mirrors `bring_up_daemon` EXACTLY (verified flow): build agent →
  `install_agent() + bootstrap() + kickstart()` UNCONDITIONALLY at every app boot → `connect_with_
  retry(client_build(), 8, 500ms)` → on Ok register broker + fill slot; on `IncompatibleOrchd`
  emit incompatible; else emit down. `AppState` gains `orchd: OrchdClientSlot` (+ orchd launchd
  agent + status slot, mirroring the sessiond fields).
- `broker.rs` — push→event consts: `EV_ORCHD_PROJECTS_CHANGED = "orchd://projects-changed"`,
  `EV_ORCHD_GOALS_CHANGED = "orchd://goals-changed"` (payload `{ projectId }`),
  `EV_ORCHD_IDEAS_CHANGED`, `EV_ORCHD_INSIGHTS_CHANGED`, `EV_ORCHD_TASKS_CHANGED` (payload
  `{ projectId }`), `EV_ORCHD_RULESET_CHANGED` (payload `{ scope, projectId? }`),
  `EV_ORCHD_DOWN = "orchd://down"`, `EV_ORCHD_UP = "orchd://up"`,
  `EV_ORCHD_INCOMPATIBLE = "orchd://incompatible"` (D10). Mapping = pure `map_orchd_push` free fn
  (mirror `map_push`'s `BrokerAction::Emit` style).
- `commands.rs` — thin `#[tauri::command]`s, one per §4.2 verb, names `orchd_` + snake_case verb
  (`orchd_create_project`, `orchd_set_idea_project`, `orchd_export_all`, …), inner-fn testable,
  stub tests mirror `connect_to_stub`/`connect_to_stub_sequence` (with `orchd.sock` + the same
  `ENV_TEST_LOCK` discipline; stub replies use the VERSION CONSTS, never literals).
  `CommandError` extended ADDITIVELY with `IncompatibleOrchd { orchd_min: u16, orchd_max: u16 }`;
  orchd daemon errors map to the existing `CommandError::Daemon { code, message }` with code
  strings `"NotFound" | "Invariant" | "Validation" | "Conflict" | "Io"`.
- **Rules-file reveal (no arbitrary paths from JS):** `orchd_reveal_rules_file(scope, project_id)`
  asks orchd `GetRuleSet`, then `opener::reveal(md_path)` on the RETURNED path. JS never passes a
  path.
- **Export/import file flows (locked):** `orchd_export_to_file(project_id | all, dest_dir)`
  writes `<name>-export.json` into a `pick_folder`-chosen dir; import = `pick_folder` + a small
  file-list dialog (existing `list_dir` on the chosen dir, filtered `*.json`) →
  `orchd_import_from_file(path)` (reads the file in core, calls `ImportBundle`).
- **Lifecycle commands (locked):** `orchd_reconnect()` — drops the slot and re-runs the
  `bring_up_orchd` connect sequence (the [Повторить] button's target); `orchd_upgrade()` —
  mirror of `upgrade_daemon` verbatim: best-effort `OrchdShutdown{drain:true}` →
  `kickstart_force()` on the orchd agent → `app.restart()`.

## 10. Frontend

- **Store `domainSlice`:** widen `view: "home" | "workspace" | "project"` (today the union AND
  `setView`'s param are both literal-typed at `store.ts:57`/`:125` — widen BOTH);
  `activeProjectId: string | null`, `projects: Project[]`, `goalsByProject: Record<string,
  Goal[]>`, `ideas: Idea[]`, `insights: Insight[]`, `tasksByProject: Record<string,
  DomainTask[]>`, `rulesets: Record<string, RuleSetView>` (key `` `global` `` /
  `` `project:${id}` ``), `orchdDown: boolean`, `orchdIncompatible: boolean`; actions
  `refresh*`, `openProject(id)`. Event bindings live in App.tsx's existing mount effect
  (the `track(onX(...))` pattern) — each `orchd://*-changed` re-fetches only the affected list.
  Workspace NAMES for project rows resolve from the existing `workspaces: Record<WorkspaceId,
  Workspace>` sessiond slice (client-side soft-ref join; unresolvable ⇒ chip).
- **Left rail restructure** (replaces the flat `list.map` in `WorkspaceSidebar.tsx`): ⌂ Home →
  project groups (project header row + its workspaces nested; workspace click = terminal view
  unchanged; project click = `openProject`) → «Без проекта» group for unlinked workspaces
  (attach → `AddProjectWorkspace`; or create-project dialog). «+ проект» → dialog: name,
  description, pick ≥1 unlinked workspace and/or create one inline (existing `pickFolder` +
  `createWorkspace`), then `orchd_create_project`.
- **Project panel** (`ProjectPanel.tsx`, tabs): Обзор (counters, workspace manage, export/import
  per §9 flows) · Цели (`GoalTree.tsx` — indent tree, strategic pinned root,
  add/edit/status/reparent/reorder, delete-subtree confirm) · Идеи (`IdeasList.tsx` — lifecycle
  chips, create/edit/delete, attach-to-project via `SetIdeaProject`) · Задачи (`TasksList.tsx` —
  status groups, subtask indent, rank ▲/▼ midpoint math via `SetTaskRank`, create dialog with
  source select) · Инсайты (`InsightsList.tsx` — fit badge, `SetInsightFitVerdict` owner
  override, archive requires non-empty reasoning) · Правила (`RulesetPanel.tsx` — md editor +
  reveal + policy form + file-state banners).
- **⌘K quick capture** (`QuickCapture.tsx`): global overlay, title/body/project select («без
  проекта»), Enter → `CreateIdea` → toast «идея сохранена»; disabled with honest inline note
  while `orchdDown`. (Spawn-project-from-idea = S-IDEA, D3.)
- **Home goals panel** (`HomeGoals.tsx`): mounts BELOW the S2 attention sections — the amber
  «Нужен ты» block KEEPS its pinned-top position (S2 §6.2 rule wins over goals prominence).
  Per active project: strategic goal title + direct additional children with status chips;
  click → `openProject` (Цели tab).
- **Honesty:** `orchdDown` ⇒ one shared banner «Оркестратор недоступен» + [Повторить] on every
  domain surface, mutating controls disabled. `orchdIncompatible` ⇒ upgrade dialog: the existing
  self-gated `UpgradeDialog` is GENERALIZED internally (reads both daemons' flag pairs; renders
  one dialog at a time, **sessiond first** if both are incompatible — after its
  `kickstart -k` + `app.restart()` the orchd incompatibility re-detects on relaunch and shows its
  own dialog; no combined choreography). orchd dialog copy: «Обновить фоновый сервис
  оркестратора — записи (проекты, цели, задачи) сохранены» (no live-session warning — orchd has
  no PTYs).
- Design-system: every new atom (project group row, lifecycle chip, tree row, policy form,
  quick-capture overlay, file-state banner) gets a `docs/design-system.md` row (2-column
  `| Atom | Contract |` format) in the same task. Amber stays reserved for «нужен ты».

## 11. Honest-degradation matrix

| Failure | Surface |
|---|---|
| orchd socket down / connect refused | `orchd://down` → banner + retry; terminals unaffected |
| orchd incompatible preamble | typed fatal → upgrade dialog (D4 flow) |
| BOTH daemons incompatible after an app update | sequential dialogs, sessiond first (§10) — no combined flow |
| Rules md file missing | `RuleFileState::Missing` → banner «файл утерян» + [Создать заново] |
| Rules md changed externally | `ExternallyModified` → banner + [Принять] (rehash) |
| Import id collision | `Conflict` toast with entity+id, nothing imported (tx rollback) |
| Export exceeds 16 MiB frame cap | typed `Io` error with an honest message, no doomed send |
| Workspace soft-ref unresolvable | chip «workspace недоступен» + [Отвязать] |
| Remove last project workspace | `Invariant` toast «у проекта должен остаться workspace» |
| orchd DB open failure at boot | sessiond-style degrade: log + in-memory fallback; panic only if SQLite unusable |

## 12. Testing & DoD

- TDD throughout. Unit: daemon-core (per-module moved tests + param-name tests + migration
  runner whole-chain/fail-closed/VersionTooNew + `server_handshake`
  accept/incompatible/garbage/timeout + generic `Broadcaster<F>`), generic framing round-trip,
  orchd-proto CBOR round-trip per variant + ts structural assertions, orchd persistence (CRUD ×
  6 families, every §5.2 invariant, cascades, FK-on proof, export/import round-trip incl.
  field-verbatim timestamps + collision), ruleset_files, socket dispatch (stub client: mutate →
  response + second connection receives push; failed mutate → NO push), core commands over an
  orchd stub, frontend slice + every §10 component (vitest; match the existing component-test
  environment setup exactly — the suite runs `environment: "node"` in `vite.config.ts` with
  library patterns already in use).
- **Phase-1 regression net:** the ENTIRE existing suite stays green after extraction (sessiond
  168 unit + boot_integration + no_secrets_in_logs + rehydrate_attach + e2e); byte-identical
  socket/lock paths + plist asserted by test.
- **e2e (`npm run e2e:orchd`, new script `tests/e2e/orchd-survive.mjs`):** reuses
  `tests/e2e/lib/daemon-harness.mjs` (its CBOR codec + framing are protocol-agnostic; `connect`
  gains an optional `{clientMin, clientMax}` param defaulting to the current `[3,3]` so the
  sessiond script is untouched; `spawnDaemon` already takes a binary path). Phases (log format
  `[e2e-orchd] phaseN OK: …`, final `[e2e-orchd] ALL PHASES PASSED`): boot on temp HOME →
  handshake `[1,1]` → create project (+2 goals, idea, task) → `OrchdShutdown{drain:true}` →
  relaunch → data intact → `ExportAll` → shutdown → delete `orchd.db*` → relaunch (fresh v1) →
  `ImportBundle` → re-export equals modulo `exportedAt`. This IS the roadmap DoD proof.
- **Gate:** `scripts/final-suite.sh` headers renumbered to `N/9`; stage 7 delegate
  `scripts/coverage-gate.sh` adds `cargo llvm-cov --package bpa-orchd --fail-under-lines 80`;
  stage 6 adds `cargo test -p bpa-orchd-proto --test ts_export` + `git diff --exit-code --
  src/ipc/orchd-types.ts`; new stage 9 `npm run e2e:orchd`. `.github/workflows/ci.yml` updated in
  lockstep (final-suite header comment demands it). `ALL GATES PASSED` required.
- Migration UX: manual check — existing 0.3.0 install boots, workspaces appear under «Без
  проекта», attach flow works, sessiond DB untouched.

## 13. Docs & release (T21 — enumerated, not generic)

- `docs/runbook-orchd.md` — mirror runbook-daemon.md with orchd's REAL names (§2.1 table).
- `docs/architecture.md`: "two OS processes" → three (core + 2 daemons); single-daemon diagram →
  two-daemon; "Hop B — core ⇄ daemon" singular → both connections; note orchd `chosen == 1`;
  module map += orchd/orchd-proto/daemon-core/orchd_client; "Two-daemon topology (… not built
  yet)" → shipped; **"orchd gets its own file API in S9" line reconciled with D4's narrow
  rules-md exception**.
- Survival truth table (overview §2 + wherever mirrored): ADD row "orchd restart / upgrade —
  domain data (projects/goals/ideas/insights/tasks/rules) fully survives (SQLite); no live
  runtime state exists to lose in S3".
- Overview §3 S3 row → SHIPPED + deltas (D2 extraction, D3 pulled-forward UI, D4 files-as-truth
  + narrow file exception, D5 full-tree superset, S2 dependency); «Current slice» → next.
- `README.md`: status line += S3; features += orchd + six families; test counts re-measured
  (was 384 Rust / 297 TS / 8 stages → new real numbers / 9 stages); coverage line += bpa-orchd.
- `CHANGELOG.md` `[0.4.0]`.
- `docs/traceability.md` S3 rows; `docs/design-system.md` sweep.
- `docs/backlog.md`: re-target stale S3 rows with reasoning — BL-4/8/9/30 (sessiond-domain;
  S3 deliberately does NOT touch sessiond behavior — move to the next sessiond cycle), BL-50/51/52
  (fs_explorer/fs_watcher; S3 doesn't touch fs — move to S4/S5 window); annotate BL-34 (stale-
  but-compatible binary never restarted) as now applying to orchd too; ADD rows: un-archive
  project verb (additive); chunked export (16 MiB cap); panel-level cross-project task rank
  (Q9 second half — additive column, S5); spawn-project-from-idea UI (S-IDEA pointer).

## 14. Out of scope (explicitly)

Research pipeline & prowl (S-IDEA), fit-test agent + auto-archive logic (S-IDEA),
**spawn-project-from-idea UI flow** (S-IDEA; data enabler ships), kanban board + **panel-level
cross-project rank** (S5; additive column later), policy ENFORCEMENT (S6c — S3 only
stores/validates), agents reading RuleSet md automatically (S6), metrics (S8), knowledge graph
(S4), orchd scheduler/workflow runtime (SW1/SW2), un-archive verb, chunked export, multi-device
sync, BL-4/8/9/30/50/51/52 (re-targeted with reasoning, §13).

## 15. Audit trail (2026-07-13)

Four parallel audits (sessiond internals, src-tauri, frontend+gate, docs/vision) cross-checked
this spec against the platform overview, vision v2, S2/Pv2 specs, backlog, and the REAL code on
main. All contradictions fixed in place (naming §2.1, §6 added, D4 file-API reconciliation),
missing items resolved (D11 no-double-options, field-verbatim import, rank base, frame cap,
double-incompatibility sequencing, Q9/spawn-idea deferrals made explicit), and every extraction
row in §3 now cites verified line-level source facts.
