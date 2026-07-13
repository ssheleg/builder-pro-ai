# S3 — `bpa-orchd` + App-Domain Foundation (Projects · Goals · Ideas · Insights · Tasks · RuleSet)

**Date:** 2026-07-13
**Status:** Approved (brainstorm 2026-07-13; owner decisions D1–D9 below)
**Depends on:** S0, S2 (`[0.3.0]` shipped), ADR-HOST (platform overview §2), Data-layer charter
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
| **D2** | **Final architecture immediately**: extract shared **`bpa-daemon-core`** crate FIRST, re-seat `bpa-sessiond` on it (full gate green = phase gate), THEN build orchd on top. No "conscious duplication now, refactor later". |
| **D3** | **Extended UI**: CRUD UI + pulled-forward ⌘K idea quick-capture (from S-IDEA) + goals panel on Home (from SH). NOT pulled: research pipeline (needs S-EXT), kanban board (S5), fit-test agent (S-IDEA). |
| **D4** | **RuleSet markdown layer: files are the source of truth**; DB stores `md_path` + `md_hash` (sha256). External edits/deletions surfaced honestly (§7). |
| **D5** | **Goal hierarchy = full tree from day one**: exactly one `strategic` root per project, `additional` subgoals at arbitrary depth via `parent_id`. |
| **D6** | **Coarse-grained invalidation pushes** (`ProjectsChanged`, `GoalsChanged{project_id}`, …) — GUI re-fetches lists; no full-entity push mirroring (hundreds of rows, re-fetch is cheap; zero snapshot-drift risk). |
| **D7** | **Export/import**: per-project JSON bundle + whole-store variant, `bundleFormat: 1`, ids preserved, import-into-empty → re-export is semantically identical (§8). Id collision on import ⇒ typed `Conflict`, no silent merge. |
| **D8** | **Protocol**: orchd reuses the Pv2 preamble verbatim (same `BPAA` magic — daemons are distinguished by socket path) with its **own independent version space `[1,1]`**; own frame enums in a new `bpa-orchd-proto` crate; `bpa-protocol` framing generalized additively (§4.1). |
| **D9** | **No new external dependencies.** Every crate S3 needs is already in the resolved graph and exercised in-repo: `rusqlite 0.32`, `ciborium 0.2`, `ts-rs 10.1`, `uuid 1.23`, `sha2 0.10`, `tokio`, `libc`. Context7 check: not applicable — no new/changed external API is being locked from memory; all patterns copied from working in-repo code (sessiond persistence/socket/preamble). |

## 2. Architecture

### 2.1 Crate topology (final)

```
crates/
  paths          (unchanged)
  protocol       (bpa-protocol) — terminal Hop-B wire; §4.1 additive framing generalization
  daemon-core    (NEW, bpa-daemon-core) — shared daemon infrastructure (§3)
  sessiond       (bpa-sessiond) — re-seated on daemon-core; behavior byte-identical
  orchd-proto    (NEW, bpa-orchd-proto) — orchd wire enums + version consts + ts-rs (§4.2)
  orchd          (NEW, bpa-orchd) — the second daemon (§5, §6)
src-tauri        — second client `orchd_client.rs`, broker events, `orchd_*` commands (§9)
src/             — domain store slice + UI components (§10)
```

Two daemons are fully independent: own sockets (`bpa-sessiond.sock` / `orchd.sock` in the same
runtime dir), own DBs (`bpa.db` / `orchd.db` in the same app-support dir
`~/Library/Application Support/ai.builderpro.desktop/`), own launchd labels
(`ai.builderpro.sessiond` / **`ai.builderpro.orchd`**), own lock files, own wire version spaces.
GUI holds two connections; failure of either degrades only its own panels (§11).

### 2.2 Implementation phases (risk order, plan-binding)

1. **Phase 1 — extraction:** create `bpa-daemon-core`; move/parameterize the five shared modules
   (§3); re-seat sessiond via thin wrappers so its call sites do not change. Phase gate: full
   8-stage `final-suite.sh` green (168 sessiond unit + boot integration + e2e are the regression
   net).
2. **Phase 2 — orchd skeleton:** `bpa-orchd-proto` + `bpa-orchd` boot/singleton/socket/DB-v1 +
   launchd plist + Ping/Shutdown. Phase gate: orchd boots, handshakes `[1,1]`, survives restart.
3. **Phase 3 — domain:** persistence CRUD + invariants → handlers + pushes → RuleSet files →
   export/import → core client/commands → frontend slice → UI → docs+gate → final review+merge.

## 3. `bpa-daemon-core` — extraction contract

Extract from sessiond, parameterizing hardcoded names. Sessiond keeps its public API via thin
wrappers (integration tests and call sites unchanged). Modules and locked APIs:

| Module | Locked public API (signatures) | Source |
|---|---|---|
| `dirs` | `pub fn app_support_dir() -> PathBuf` (both daemons share `ai.builderpro.desktop`; `$HOME`-resolved, test-isolatable exactly as today) | `sessiond/boot.rs::app_support_dir` |
| `singleton` | `pub struct LockGuard; pub fn acquire_lock_at(path: &Path) -> io::Result<LockGuard>; pub fn resolve_socket_path(file_name: &str) -> PathBuf; pub fn resolve_lockfile(file_name: &str) -> PathBuf; pub fn ensure_socket_dir() -> io::Result<()>; pub fn assert_socket_path_len(&Path) -> io::Result<()>; pub fn set_socket_mode(&Path) -> io::Result<()>; pub fn check_peer_cred(BorrowedFd) -> io::Result<()>` | `sessiond/singleton.rs` (name params added) |
| `logging` | `pub fn init_to_file(path: &Path) -> io::Result<()>; pub fn flush()` — no-secrets discipline preserved verbatim | `sessiond/logging.rs` |
| `migrate` | `pub struct Migration { pub from: i64, pub apply: fn(&rusqlite::Transaction) -> rusqlite::Result<()> }`; `pub fn run_migrations(conn: &mut Connection, target: i64, steps: &[Migration]) -> Result<(), MigrateError>` — fail-closed, forward-only, each step one transaction, `PRAGMA user_version` bumped inside the step's tx (extracted semantics of `persistence.rs::migrate`, byte-compatible with existing sessiond DBs) | `sessiond/persistence.rs::migrate` |
| `handshake` | `pub async fn server_handshake(stream: &mut UnixStream, min: u16, max: u16, build: &str) -> Result<Option<u16>, io::Error>` — `PREAMBLE_TIMEOUT`-bounded read, `negotiate`, reply Accepted/Incompatible, `Ok(None)` = incompatible-and-replied (caller closes). Reuses `bpa_protocol::preamble` types | `sessiond/socket_server.rs` accept-path |

Rule: extraction moves code with its tests; NO behavior change; sessiond's `resolve_socket_path()`
wrapper passes its current file name so on-disk paths stay identical (zero migration for existing
installs).

## 4. Wire protocol

### 4.1 `bpa-protocol` additive framing generalization

`framing.rs` gains a generic core; existing API unchanged:

```rust
pub fn encode_cbor_frame<T: serde::Serialize>(v: &T) -> Result<Vec<u8>, FrameError>;
pub struct CborFrameDecoder<T> { /* buffer + PhantomData<T> */ }
impl<T: serde::de::DeserializeOwned> CborFrameDecoder<T> {
    pub fn new() -> Self;
    pub fn push(&mut self, chunk: &[u8]);
    pub fn decode(&mut self) -> Result<Vec<T>, FrameError>;
}
```

`encode_frame(&Frame)` / `FrameDecoder` become thin instantiations (`FrameDecoder` =
`CborFrameDecoder<Frame>` behind the existing type name and methods). `MAX_FRAME_LEN` shared.
Preamble module: unchanged (D8 — same magic, version consts live per-protocol-crate).

### 4.2 `bpa-orchd-proto` — wire contract (LOCKED, enum order FROZEN append-only from day one)

Version consts: `pub const ORCHD_CLIENT_MIN_VERSION: u16 = 1;` (same for MAX, DAEMON_MIN/MAX — all
`1`). All types `#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]`, entity/enum types
additionally `#[derive(TS)]` with `#[serde(rename_all = "camelCase")]` on structs and
`#[serde(rename_all = "camelCase")]` tag values on plain enums (mirror `bpa-protocol` style).
ts-rs export target: `src/ipc/orchd-types.ts` (parity-gated like `types.ts`).

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
    UpdateIdea { id: String, title: Option<String>, body: Option<String>,
                 project_id: Option<Option<String>> },
    SetIdeaLifecycle { id: String, lifecycle: IdeaLifecycle },
    DeleteIdea { id: String },
    ListIdeas { project_id: Option<String> }, // None ⇒ ALL ideas incl. orphans
    // Insight
    CreateInsight { project_id: Option<String>, source: String, title: String, body: String },
    UpdateInsight { id: String, title: Option<String>, body: Option<String>,
                    fit_verdict: Option<Option<FitVerdict>>, fit_reasoning: Option<String> },
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

Every mutating request: reply to requester (`Response::<entity>` with the updated row, or `Ack`
for deletes) **and** broadcast the matching coarse push to all connections (sessiond broadcaster
pattern). `Update*` `Option<Option<T>>` fields follow serde double-option semantics (absent =
unchanged, `null` = clear).

## 5. `bpa-orchd` daemon

Modules mirror sessiond minus PTY: `main.rs` (tokio main, watch shutdown, SIGTERM),
`boot.rs` (singleton → logging → DB open-degrading → socket serve), `socket_server.rs`
(accept loop → `daemon_core::handshake::server_handshake(1,1,build)` → frame dispatch →
broadcaster), `persistence.rs` (schema v1 + CRUD), `ruleset_files.rs` (§7), `export.rs` (§8).
Launchd: `ai.builderpro.orchd.plist`, KeepAlive, same install/bootstrap flow the core uses for
sessiond (`src-tauri/src/launchd.rs` parameterized for a second label). Log file:
`{app-support}/logs/orchd.log` (no-secrets discipline; rules md content is never logged).

### 5.1 `orchd.db` schema v1 (LOCKED DDL)

```sql
PRAGMA journal_mode=WAL;  -- same as bpa.db
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
-- PRAGMA user_version = 1  (set by the daemon-core migration runner)
```

`PRAGMA foreign_keys=ON` per connection (sessiond does not use FK cascades; orchd DOES — set it
in `open_inner`). Timestamps unix-ms. Ids `uuid::Uuid::new_v4().to_string()`.

### 5.2 Invariants (enforced in persistence layer, each a typed error)

| Invariant | Error |
|---|---|
| `CreateProject` requires ≥1 `workspace_ids`; `RemoveProjectWorkspace` refuses the last link | `Invariant` |
| A workspace can be linked to at most one project (UNIQUE) | `Conflict` |
| Exactly one strategic goal per project, auto-created WITH the project (`title: "Стратегическая цель"`, empty body, owner edits it) — never deletable; `CreateGoal{kind: Strategic}` on a project that has one | `Invariant` |
| `MoveGoal`/`CreateGoal parent_id` must stay within one project; cycles rejected (walk-up check) | `Invariant` |
| Strategic root: `parent_id` always NULL; `MoveGoal` on it | `Invariant` |
| Task `parent_id` same-project + cycle-rejected (same walk-up) | `Invariant` |
| `rank` assigned on create = max(rank in project)+1024; `SetTaskRank` takes an explicit f64 (fractional insert-between is the client's move) | — |
| Unknown ids anywhere | `NotFound` |
| `PolicyRules` strict-validated (no unknown JSON keys, non-negative cap, non-empty allowlist entries) | `Validation` |

Archived project: rows stay, reads work (honest history); EVERY mutating verb touching it or its
children ⇒ `Invariant`. Archive is one-way in v1; un-archive is a future additive verb (backlog
row, §13).

Auto-created rows (single tx with `CreateProject`): the strategic goal (§5.2 above) AND the
project's ruleset row (`scope='project'`, default `md_path = {app-support}/rules/project-<uuid>.md`,
file written with a `# Правила проекта <name>` template header, `policy = {}`). The GLOBAL
ruleset row + `rules/global.md` are ensured at every orchd boot (idempotent).

## 7. RuleSet file layer (`ruleset_files.rs`)

- Default paths: global ⇒ `{app-support}/rules/global.md`; project ⇒
  `{app-support}/rules/project-<uuid>.md`. Owner may repoint `md_path` to any absolute path
  (e.g. a repo file) via `UpsertRuleSet{md_path}` — validated absolute + parent exists.
- `UpsertRuleSet{md_content: Some}`: create parent dirs, write file atomically (tmp+rename),
  `md_hash = sha256(content)` hex.
- `GetRuleSet`: read file fresh each time → `RuleSetView{ md_content, file_state }`:
  `Ok` (hash matches), `ExternallyModified` (exists, hash differs — content returned, banner in
  UI with [Принять] → `AcknowledgeRuleFile`), `Missing` (`md_content: None`, UI offers
  «создать заново»).
- Deleting a ruleset row (only via project cascade) never deletes the file — honest note in UI.
- File content is never logged (secrets discipline).
- No fs-watch on rules in v1 — freshness is read-on-Get + explicit Acknowledge (YAGNI; the GUI
  re-Gets on panel open and on `RuleSetChanged`).

## 8. Export / import (`export.rs`)

Serde-serialized with the same camelCase entity structs (§4.2). Locked shapes:

```jsonc
// ExportProject
{ "bundleFormat": 1, "exportedAt": 1760000000000,
  "project": { …Project… },                    // workspace_ids included (soft refs)
  "goals": [ …Goal… ], "ideas": [ …Idea… ], "insights": [ …Insight… ],
  "tasks": [ …DomainTask… ],
  "ruleset": { "rule": { …RuleSet… }, "mdContent": "…" } | null,  // mdContent read at export;
                                                                   // file missing ⇒ "mdMissing": true added
}
// ExportAll
{ "bundleFormat": 1, "exportedAt": …,
  "projects": [ …per-project bundle objects (без bundleFormat/exportedAt)… ],
  "globalRuleset": { "rule": …, "mdContent": … } | null,
  "orphanIdeas": [ …Idea with project_id null… ],
  "orphanInsights": [ …Insight with project_id null… ] }
```

Import: single transaction; validates `bundleFormat == 1`; any id already present in the store ⇒
`Conflict{message: "<entity> <id> already exists"}`, transaction rolled back, nothing imported.
Ruleset import writes the md file to the bundle's `md_path` **only if the path is under
app-support**; otherwise (foreign absolute path) writes to the default app-support path and
repoints — never touches files outside app-support on import. Workspace soft-refs are imported
as-is; UI shows «workspace недоступен» chip for unresolvable ids (§10). Import inserts rows
directly (raw persistence writes in the tx, NOT the `CreateProject` verb) — the §5.2 auto-create
never double-fires; the bundle already carries the strategic goal and ruleset row, and the unique
indexes back-stop any malformed bundle. Round-trip guarantee (DoD): import into an empty store →
`ExportAll` equals the original modulo `exportedAt`.

## 9. Core integration (src-tauri)

- `orchd_client.rs` — mirror of the sessiond socket client (`socket_client.rs` pattern): connect
  to `orchd.sock`, client preamble `[1,1]`, request-id correlation, push forwarding, swappable
  client slot, typed `IncompatibleOrchd` (fatal, no auto-retry), launchd bootstrap-on-first-connect
  (parameterized `launchd.rs`).
- `broker.rs` — map pushes → Tauri events (consts): `orchd://projects-changed`,
  `orchd://goals-changed` `{projectId}`, `orchd://ideas-changed`, `orchd://insights-changed`,
  `orchd://tasks-changed` `{projectId}`, `orchd://ruleset-changed` `{scope, projectId?}`,
  plus `orchd://down` / `orchd://up` connection-state events and `orchd://incompatible`.
- `commands.rs` — thin `#[tauri::command]` wrappers, one per §4.2 verb, `CommandError` mapping
  (`OrchdErrorCode` → typed TS error). Testable inner fns, stub-daemon tests (existing pattern).

## 10. Frontend

- **Store `domainSlice`:** `projects: Project[]`, `goalsByProject: Record<string, Goal[]>`,
  `ideas: Idea[]`, `insights: Insight[]`, `tasksByProject: Record<string, DomainTask[]>`,
  `ruleset: Record<string, RuleSetView>` (key `global` | `project:<id>`), `orchdDown: boolean`,
  `orchdIncompatible: boolean`, `activeProjectId: string | null`,
  `view: "home" | "workspace" | "project"` (extends S2's view union). Coarse events re-fetch the
  affected list only.
- **Left rail restructure:** ⌂ Home → project groups (project header row + its workspaces
  nested; workspace click = terminal view unchanged; project click = project panel) → «Без
  проекта» group for unlinked workspaces (attach action → `AddProjectWorkspace` or
  create-project dialog). «+ проект» button → dialog: name, description, pick ≥1 existing
  unlinked workspace and/or create a new workspace inline (sessiond `CreateWorkspace` first, then
  `CreateProject` with the ids).
- **Project panel** (`ProjectPanel.tsx`, tabs): Обзор (counters, workspace manage, export/import
  buttons — export saves via dialog, import picks file → `ImportBundle`), Цели (`GoalTree.tsx` —
  indent tree, strategic pinned root, add/edit/status/reparent/reorder, delete-subtree with
  confirm), Идеи (`IdeasList.tsx` — lifecycle chips, create/edit/delete), Задачи
  (`TasksList.tsx` — flat list grouped by status, subtask indent, status select, rank up/down =
  `SetTaskRank` midpoint math, create dialog with source select), Инсайты (`InsightsList.tsx`),
  Правила (`RulesetPanel.tsx` — md preview + «открыть файл» (reveal), policy form,
  `ExternallyModified` banner + [Принять], `Missing` banner + [Создать заново]).
- **⌘K quick capture** (`QuickCapture.tsx`): global overlay (portal), fields title/body/project
  select («без проекта»), Enter → `CreateIdea` → toast «идея сохранена». Registered app-wide
  keydown, honest failure toast when orchd down.
- **Home goals panel** (`HomeGoals.tsx`): per-project block — strategic goal title + top-level
  additional goals with status chips, click → project panel Цели tab. Sits above the S2
  attention queue; SH inherits.
- **Honesty:** `orchdDown` ⇒ every domain surface renders one shared banner «Оркестратор
  недоступен» + [Повторить] (retry connect); no cached-data mutation allowed while down
  (buttons disabled). `orchdIncompatible` ⇒ D4-style upgrade dialog (no live-session warning
  needed — orchd has no PTYs).
- Design-system: every new atom (project group row, lifecycle chip, tree row, policy form,
  quick-capture overlay) gets a `docs/design-system.md` row in the same task. Amber stays
  reserved for «нужен ты»; one accent.

## 11. Honest-degradation matrix

| Failure | Surface |
|---|---|
| orchd socket down / connect refused | `orchd://down` → banner + retry; terminals unaffected |
| orchd incompatible preamble | typed fatal → upgrade dialog (D4 flow, kickstart -k + app restart) |
| Rules md file missing | `RuleFileState::Missing` → banner «файл утерян» + [Создать заново] |
| Rules md changed externally | `ExternallyModified` → banner + [Принять] (rehash) |
| Import id collision | `Conflict` toast with entity+id, nothing imported (tx rollback) |
| Workspace soft-ref unresolvable (sessiond lost it / imported bundle) | chip «workspace недоступен» + [Отвязать] |
| Remove last project workspace | `Invariant` toast «у проекта должен остаться workspace» |
| DB open failure at boot | mirror sessiond open-degrading pattern + log; daemon serves `Error{Io}` honestly |

## 12. Testing & DoD

- TDD throughout. Unit: daemon-core (singleton param paths, migration runner incl. fail-closed
  rollback fixture, server_handshake accept/incompatible/garbage/timeout), generic framing
  round-trip, orchd-proto CBOR round-trip per variant + ts parity test, orchd persistence (CRUD ×
  6 families, every §5.2 invariant, cascade behaviors, FK-on, export/import round-trip +
  collision), ruleset_files (atomic write, hash lifecycle, missing/modified states, import path
  containment), socket dispatch (stub client: mutate → response + second connection receives
  push), core commands over stub orchd, frontend slice + every §10 component (vitest).
- **Regression net for Phase 1:** entire existing suite must stay green after extraction
  (sessiond 168 unit + boot integration + no_secrets + rehydrate + e2e). No sessiond behavior
  change is acceptable.
- **e2e (`npm run e2e:orchd`, new script `e2e/orchd-survive.mjs`):** boot orchd → handshake
  `[1,1]` → create project (+2 goals, idea, task) → `OrchdShutdown{drain:true}` → relaunch →
  data intact → `ExportAll` → shutdown → delete `orchd.db` → relaunch (fresh v1 schema) →
  `ImportBundle` → re-export equals the first export modulo `exportedAt`. This IS the roadmap
  DoD proof (survive-restart + round-trip in one honest script).
- **Gate:** `scripts/final-suite.sh` extended: coverage stage additionally enforces
  `bpa-orchd ≥ 80%`; new stage 9 `e2e:orchd`. `ALL GATES PASSED` required.
- Migration UX: manual check — existing 0.3.0 install boots, workspaces appear under «Без
  проекта», attach flow works, sessiond DB untouched.

## 13. Docs & release

Same-change docs: `docs/runbook-orchd.md` (mirror runbook-daemon.md), `docs/architecture.md`
(two-daemon topology diagram + domain store), overview §3 S3 row → shipped + deltas, README
(features/test counts), `docs/traceability.md`, `CHANGELOG.md` `[0.4.0]`, design-system rows,
backlog: close/annotate BL rows this ships against; add row «un-archive project verb (additive)».

## 14. Out of scope (explicitly)

Research pipeline & prowl (S-IDEA), fit-test agent (S-IDEA), kanban board (S5), policy
ENFORCEMENT (S6c — S3 only stores/validates), agents reading RuleSet md automatically (S6),
metrics (S8), knowledge graph (S4), orchd scheduler/workflow runtime (SW1/SW2 — the daemon ships
with the store only), un-archive verb, multi-device sync.
