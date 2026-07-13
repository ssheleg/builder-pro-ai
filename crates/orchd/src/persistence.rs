//! Durable SQLite persistence for `bpa-orchd` (spec §5, §5.1). Mirrors
//! `bpa_sessiond::persistence`'s open/degrade/quarantine shape, re-seated onto
//! `bpa_daemon_core::migrate::run_migrations` (S3 phase 1 extraction) from day one — there is no
//! pre-extraction inline `migrate` to preserve here, unlike sessiond's history.
//!
//! Schema v1 (spec §5.1, LOCKED DDL) is applied as a single `Migration { upto: 1 }` step; every
//! later domain migration (T10+) appends further steps to the same table, never mutates this one.

use std::fmt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use bpa_orchd_proto::{Goal, GoalKind, GoalStatus, Project, ProjectStatus};
use rusqlite::{Connection, OptionalExtension};
use tracing::{info, warn};
use uuid::Uuid;

/// Current schema/migration version stored in `PRAGMA user_version` (spec §5.1).
pub const SCHEMA_VERSION: i64 = 1;

#[derive(Debug)]
pub enum PersistError {
    Open(String),
    Sql(String),
    Migration(String),
    Corrupt(String),
}

impl fmt::Display for PersistError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PersistError::Open(m) => write!(f, "db open failed: {m}"),
            PersistError::Sql(m) => write!(f, "db sql error: {m}"),
            PersistError::Migration(m) => write!(f, "db migration failed: {m}"),
            PersistError::Corrupt(m) => write!(f, "db corrupt: {m}"),
        }
    }
}

impl std::error::Error for PersistError {}

impl From<rusqlite::Error> for PersistError {
    fn from(e: rusqlite::Error) -> Self {
        classify(e)
    }
}

/// SQLite-backed persistence handle for `orchd.db` (spec §5.1). Full domain CRUD (project /
/// goal / idea / insight / task / ruleset) lands in T10+; this skeleton only owns open/migrate/
/// checkpoint plus the raw [`Db::conn`] seam future CRUD methods (and this crate's own
/// integration tests, which have no other way to assert on-disk schema state) build on.
#[derive(Debug)]
pub struct Db {
    conn: Connection,
}

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// True if the rusqlite error is a corruption / not-a-database error.
fn is_corruption(e: &rusqlite::Error) -> bool {
    use rusqlite::ffi::ErrorCode;
    if let rusqlite::Error::SqliteFailure(err, _) = e {
        matches!(
            err.code,
            ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase
        )
    } else {
        false
    }
}

fn classify(e: rusqlite::Error) -> PersistError {
    if is_corruption(&e) {
        PersistError::Corrupt(e.to_string())
    } else {
        PersistError::Sql(e.to_string())
    }
}

/// Quarantine path for a corrupt on-disk database: `<path>.corrupt-<unix-ts>` (mirrors
/// `bpa_sessiond::persistence::quarantine` byte-for-byte).
fn quarantine(path: &Path) -> PathBuf {
    let ts = now_secs();
    let mut q = path.as_os_str().to_os_string();
    q.push(format!(".corrupt-{ts}"));
    PathBuf::from(q)
}

impl Db {
    /// Open (or create) the database at `path`. Sets WAL + busy_timeout + foreign_keys, runs
    /// migrations in a transaction. On a corrupt image, quarantines the file
    /// (`orchd.db.corrupt-<ts>`) and recreates a fresh database rather than crashing (mirrors
    /// `bpa_sessiond::persistence::Db::open`).
    pub fn open(path: &Path) -> Result<Db, PersistError> {
        match Self::open_inner(path) {
            Ok(db) => Ok(db),
            Err(PersistError::Corrupt(msg)) => {
                let dst = quarantine(path);
                warn!(
                    ?path,
                    ?dst,
                    "database corrupt, quarantining and recreating: {msg}"
                );
                std::fs::rename(path, &dst)
                    .map_err(|e| PersistError::Open(format!("quarantine rename failed: {e}")))?;
                // Sidecar WAL/SHM files from the corrupt db would confuse the fresh one.
                for suffix in ["-wal", "-shm"] {
                    let mut side = path.as_os_str().to_os_string();
                    side.push(suffix);
                    let _ = std::fs::remove_file(PathBuf::from(side));
                }
                Self::open_inner(path)
            }
            Err(other) => Err(other),
        }
    }

    /// Open a private, in-memory database (test-support). Not durable across process restarts.
    pub fn open_in_memory() -> Result<Db, PersistError> {
        let conn = Connection::open_in_memory().map_err(|e| PersistError::Open(e.to_string()))?;
        // In-memory databases can't use WAL (no shared-memory file backing), but we still apply
        // busy_timeout + foreign_keys for parity with the on-disk path (spec §5.1).
        conn.pragma_update(None, "busy_timeout", 5000_i64)
            .map_err(classify)?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(classify)?;
        let user_version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(classify)?;
        let db = Db { conn };
        db.migrate(user_version)?;
        info!("in-memory database opened (schema v{SCHEMA_VERSION})");
        Ok(db)
    }

    /// Force a WAL checkpoint, flushing the write-ahead log into the main database file
    /// (graceful-shutdown helper; spec §5 "flush + WAL checkpoint on graceful shutdown").
    /// Best-effort: any failure is a typed error, never a panic.
    pub fn checkpoint(&self) -> Result<(), PersistError> {
        self.conn
            .pragma_update(None, "wal_checkpoint", "TRUNCATE")
            .map_err(classify)?;
        Ok(())
    }

    /// Raw connection accessor. Test-support seam (this crate's integration tests have no other
    /// way to assert on-disk schema/FK/row state) that also doubles as the seam T10's domain CRUD
    /// methods will be built directly on top of — not part of a stable public query API.
    #[doc(hidden)]
    pub fn conn(&self) -> &Connection {
        &self.conn
    }

    fn open_inner(path: &Path) -> Result<Db, PersistError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)
                .map_err(|e| PersistError::Open(format!("create dir failed: {e}")))?;
        }
        let conn = Connection::open(path).map_err(|e| PersistError::Open(e.to_string()))?;

        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(classify)?;
        conn.pragma_update(None, "busy_timeout", 5000_i64)
            .map_err(classify)?;
        conn.pragma_update(None, "foreign_keys", "ON")
            .map_err(classify)?;

        // Force a read to surface "not a database" / corruption at open time.
        let user_version: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(classify)?;

        let db = Db { conn };
        db.migrate(user_version)?;
        info!(?path, "database opened (WAL, schema v{SCHEMA_VERSION})");
        Ok(db)
    }

    /// Run migrations from `from_version` to `SCHEMA_VERSION` in one transaction. Fails closed
    /// (typed error) on any error — never panics. Thin wrapper over
    /// `bpa_daemon_core::migrate::run_migrations` (S3 phase 1 extraction, spec §3).
    fn migrate(&self, from_version: i64) -> Result<(), PersistError> {
        const STEPS: &[bpa_daemon_core::migrate::Migration] =
            &[bpa_daemon_core::migrate::Migration {
                upto: 1,
                apply: migrate_v1,
            }];
        bpa_daemon_core::migrate::run_migrations(&self.conn, from_version, SCHEMA_VERSION, STEPS)
            .map_err(|e| match e {
                bpa_daemon_core::migrate::MigrateError::VersionTooNew { found, supported } => {
                    PersistError::Migration(format!(
                        "db user_version {found} newer than supported {supported}"
                    ))
                }
                bpa_daemon_core::migrate::MigrateError::Sql(e) => {
                    PersistError::Migration(e.to_string())
                }
            })
    }
}

/// v0 -> v1: `orchd.db` schema v1 (spec §5.1, LOCKED DDL — transcribed verbatim, including the
/// spec's own inline comments, so this body and the spec text can be diffed directly).
fn migrate_v1(tx: &rusqlite::Transaction) -> rusqlite::Result<()> {
    tx.execute_batch(
        "CREATE TABLE project (
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
         -- user_version set to 1 by the migration runner",
    )
}

// ================================================================================
// ---- domain persistence (spec §5.2): project + project_workspace + goal CRUD ----
// ================================================================================

/// Title of the strategic goal auto-created with every project (spec §5.2; the owner edits it
/// afterwards, it is never auto-changed again and never deletable).
const STRATEGIC_GOAL_TITLE: &str = "Стратегическая цель";

/// Domain persistence error (spec §5.2, §6 wire mapping: `NotFound→NotFound`,
/// `Invariant→Invariant`, `Conflict→Conflict`, `Validation→Validation`, `Sql→Io`,
/// `Io(String)→Io`). `Io(String)` is kept distinct from `Sql(rusqlite::Error)` as the non-SQL
/// I/O producer other tasks build on (e.g. T9's export frame-cap guard); this task also uses it
/// defensively for "the DB returned a value that violates its own CHECK constraint", which can
/// only happen if the on-disk file was hand-edited outside this crate.
#[derive(Debug)]
pub enum OrchdPersistError {
    NotFound,
    Invariant(String),
    Conflict(String),
    Validation(String),
    Io(String),
    Sql(rusqlite::Error),
}

impl fmt::Display for OrchdPersistError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OrchdPersistError::NotFound => write!(f, "not found"),
            OrchdPersistError::Invariant(m) => write!(f, "invariant violated: {m}"),
            OrchdPersistError::Conflict(m) => write!(f, "conflict: {m}"),
            OrchdPersistError::Validation(m) => write!(f, "validation failed: {m}"),
            OrchdPersistError::Io(m) => write!(f, "io error: {m}"),
            OrchdPersistError::Sql(e) => write!(f, "db sql error: {e}"),
        }
    }
}

impl std::error::Error for OrchdPersistError {}

impl From<rusqlite::Error> for OrchdPersistError {
    fn from(e: rusqlite::Error) -> Self {
        OrchdPersistError::Sql(e)
    }
}

/// True if `e` is a SQLite UNIQUE/PRIMARY KEY (or any other) constraint violation. Coarse on
/// purpose, mirroring [`is_corruption`]'s shape above: every call site that uses this has
/// already validated every OTHER constraint on the row it's inserting (FK target exists via an
/// explicit prior lookup, NOT NULL columns populated, CHECK-legal literals), so a
/// `ConstraintViolation` at that point can only be the UNIQUE/PK collision it's guarding
/// against.
fn is_constraint_violation(e: &rusqlite::Error) -> bool {
    use rusqlite::ffi::ErrorCode;
    if let rusqlite::Error::SqliteFailure(err, _) = e {
        matches!(err.code, ErrorCode::ConstraintViolation)
    } else {
        false
    }
}

/// Maps a `project_workspace` insert failure to `Conflict` (spec §5.2: "a workspace can be
/// linked to at most one project") when it's a constraint violation, otherwise passes the raw
/// SQL error through unchanged.
fn map_workspace_conflict(e: rusqlite::Error, workspace_id: &str) -> OrchdPersistError {
    if is_constraint_violation(&e) {
        OrchdPersistError::Conflict(format!(
            "workspace {workspace_id} is already linked to a project"
        ))
    } else {
        OrchdPersistError::Sql(e)
    }
}

/// Domain-row timestamp clock (spec §5.1: "Timestamps unix-ms"). Distinct from [`now_secs`]
/// above, which is seconds-resolution and only used for the corrupt-DB quarantine suffix.
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Default `ruleset.md_path` for a project's own rules file (spec §5.2:
/// `{app-support}/rules/project-<uuid>.md`). The FILE itself is written later by the T10
/// dispatch handler via `ruleset_files.rs` — this task persists only the DB row's path string.
fn project_ruleset_md_path(project_id: &str) -> String {
    bpa_daemon_core::dirs::app_support_dir()
        .join("rules")
        .join(format!("project-{project_id}.md"))
        .to_string_lossy()
        .into_owned()
}

fn decode_project_status(s: &str) -> Result<ProjectStatus, OrchdPersistError> {
    match s {
        "active" => Ok(ProjectStatus::Active),
        "archived" => Ok(ProjectStatus::Archived),
        other => Err(OrchdPersistError::Io(format!(
            "corrupt project.status value: {other}"
        ))),
    }
}

fn encode_goal_kind(k: &GoalKind) -> &'static str {
    match k {
        GoalKind::Strategic => "strategic",
        GoalKind::Additional => "additional",
    }
}

fn decode_goal_kind(s: &str) -> Result<GoalKind, OrchdPersistError> {
    match s {
        "strategic" => Ok(GoalKind::Strategic),
        "additional" => Ok(GoalKind::Additional),
        other => Err(OrchdPersistError::Io(format!(
            "corrupt goal.kind value: {other}"
        ))),
    }
}

fn encode_goal_status(s: &GoalStatus) -> &'static str {
    match s {
        GoalStatus::Active => "active",
        GoalStatus::Achieved => "achieved",
        GoalStatus::Dropped => "dropped",
    }
}

fn decode_goal_status(s: &str) -> Result<GoalStatus, OrchdPersistError> {
    match s {
        "active" => Ok(GoalStatus::Active),
        "achieved" => Ok(GoalStatus::Achieved),
        "dropped" => Ok(GoalStatus::Dropped),
        other => Err(OrchdPersistError::Io(format!(
            "corrupt goal.status value: {other}"
        ))),
    }
}

fn decode_metric_refs(s: &str) -> Result<Vec<String>, OrchdPersistError> {
    serde_json::from_str(s)
        .map_err(|e| OrchdPersistError::Io(format!("corrupt goal.metric_refs json: {e}")))
}

/// `project.status` guard shared by every mutator (spec §5.2: "EVERY mutating verb touching
/// [an archived project] or its children ⇒ `Invariant`"). Takes `&Connection` so it works both
/// directly against `&self.conn` and — via `rusqlite::Transaction`'s
/// `Deref<Target = Connection>` — against an in-flight `&Transaction`.
fn ensure_project_active(conn: &Connection, project_id: &str) -> Result<(), OrchdPersistError> {
    let status: Option<String> = conn
        .query_row(
            "SELECT status FROM project WHERE id = ?1",
            rusqlite::params![project_id],
            |r| r.get(0),
        )
        .optional()?;
    match status.as_deref() {
        None => Err(OrchdPersistError::NotFound),
        Some("archived") => Err(OrchdPersistError::Invariant("project archived".to_string())),
        Some(_) => Ok(()),
    }
}

/// Assembles a full [`Project`] (base row + `workspace_ids` joined from `project_workspace`,
/// ordered by `ord` — spec §5.2 `list_projects`).
fn load_project(conn: &Connection, id: &str) -> Result<Project, OrchdPersistError> {
    let base: Option<(String, String, String, String, i64, i64)> = conn
        .query_row(
            "SELECT id, name, description, status, created_at, updated_at
             FROM project WHERE id = ?1",
            rusqlite::params![id],
            |r| {
                Ok((
                    r.get(0)?,
                    r.get(1)?,
                    r.get(2)?,
                    r.get(3)?,
                    r.get(4)?,
                    r.get(5)?,
                ))
            },
        )
        .optional()?;
    let (id, name, description, status, created_at, updated_at) =
        base.ok_or(OrchdPersistError::NotFound)?;
    let status = decode_project_status(&status)?;

    let mut stmt = conn
        .prepare("SELECT workspace_id FROM project_workspace WHERE project_id = ?1 ORDER BY ord")?;
    let workspace_ids: Vec<String> = stmt
        .query_map(rusqlite::params![id], |r| r.get(0))?
        .collect::<Result<_, _>>()?;

    Ok(Project {
        id,
        name,
        description,
        status,
        workspace_ids,
        created_at,
        updated_at,
    })
}

/// Raw `goal` row (text-encoded `kind`/`status`/`metric_refs`) before decoding into the wire
/// [`Goal`] type.
struct GoalRow {
    id: String,
    project_id: String,
    parent_id: Option<String>,
    kind: String,
    title: String,
    body: String,
    ord: i64,
    status: String,
    metric_refs: String,
    created_at: i64,
    updated_at: i64,
}

impl GoalRow {
    fn from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<GoalRow> {
        Ok(GoalRow {
            id: r.get(0)?,
            project_id: r.get(1)?,
            parent_id: r.get(2)?,
            kind: r.get(3)?,
            title: r.get(4)?,
            body: r.get(5)?,
            ord: r.get(6)?,
            status: r.get(7)?,
            metric_refs: r.get(8)?,
            created_at: r.get(9)?,
            updated_at: r.get(10)?,
        })
    }

    fn into_goal(self) -> Result<Goal, OrchdPersistError> {
        Ok(Goal {
            id: self.id,
            project_id: self.project_id,
            parent_id: self.parent_id,
            kind: decode_goal_kind(&self.kind)?,
            title: self.title,
            body: self.body,
            ord: self.ord,
            status: decode_goal_status(&self.status)?,
            metric_refs: decode_metric_refs(&self.metric_refs)?,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

fn load_goal(conn: &Connection, id: &str) -> Result<Goal, OrchdPersistError> {
    conn.query_row(
        "SELECT id, project_id, parent_id, kind, title, body, ord, status, metric_refs,
                created_at, updated_at
         FROM goal WHERE id = ?1",
        rusqlite::params![id],
        GoalRow::from_row,
    )
    .optional()?
    .ok_or(OrchdPersistError::NotFound)?
    .into_goal()
}

/// Walks the parent chain starting at `start_id`, returning `true` if `target_id` is `start_id`
/// itself or one of its ancestors. Used by [`Db::move_goal`]'s cycle guard: reparenting `id`
/// under `new_parent_id` is only safe if `new_parent_id` is NOT `id` itself and NOT already a
/// descendant of `id` — which is exactly "walking up from `new_parent_id` eventually reaches
/// `id`" (spec §5.2 "cycles rejected (walk-up check)").
fn ancestor_chain_contains(
    conn: &Connection,
    start_id: &str,
    target_id: &str,
) -> Result<bool, OrchdPersistError> {
    let mut current = Some(start_id.to_string());
    while let Some(cur) = current {
        if cur == target_id {
            return Ok(true);
        }
        current = conn
            .query_row(
                "SELECT parent_id FROM goal WHERE id = ?1",
                rusqlite::params![cur],
                |r| r.get::<_, Option<String>>(0),
            )
            .optional()?
            .flatten();
    }
    Ok(false)
}

impl Db {
    /// `CreateProject` (spec §5.2): ONE transaction inserts the `project` row, its
    /// `project_workspace` links (`ord` 0..), the auto-created strategic `goal`
    /// (`title: "Стратегическая цель"`, empty body — owner edits it, never deletable) AND the
    /// project's `ruleset` DB row (`scope='project'`, default `md_path`, `md_hash=''`,
    /// `policy='{}'`; the FILE itself is written later by the T10 dispatch handler, not here).
    /// `workspace_ids` empty ⇒ `Invariant`; a `workspace_id` already linked to ANY project (the
    /// `project_workspace.workspace_id` UNIQUE index — including a duplicate within
    /// `workspace_ids` itself) ⇒ `Conflict`, rolling back the whole transaction.
    pub fn create_project(
        &self,
        name: &str,
        description: &str,
        workspace_ids: &[String],
    ) -> Result<Project, OrchdPersistError> {
        if workspace_ids.is_empty() {
            return Err(OrchdPersistError::Invariant(
                "project requires at least one workspace_id".to_string(),
            ));
        }

        let tx = self.conn.unchecked_transaction()?;
        let now = now_ms();
        let id = Uuid::new_v4().to_string();

        tx.execute(
            "INSERT INTO project (id, name, description, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, 'active', ?4, ?4)",
            rusqlite::params![id, name, description, now],
        )?;

        for (ord, workspace_id) in workspace_ids.iter().enumerate() {
            tx.execute(
                "INSERT INTO project_workspace (project_id, workspace_id, ord) VALUES (?1, ?2, ?3)",
                rusqlite::params![id, workspace_id, ord as i64],
            )
            .map_err(|e| map_workspace_conflict(e, workspace_id))?;
        }

        tx.execute(
            "INSERT INTO goal
               (id, project_id, parent_id, kind, title, body, ord, status, metric_refs,
                created_at, updated_at)
             VALUES (?1, ?2, NULL, 'strategic', ?3, '', 0, 'active', '[]', ?4, ?4)",
            rusqlite::params![Uuid::new_v4().to_string(), id, STRATEGIC_GOAL_TITLE, now],
        )?;

        tx.execute(
            "INSERT INTO ruleset (id, scope, project_id, md_path, md_hash, policy, created_at, updated_at)
             VALUES (?1, 'project', ?2, ?3, '', '{}', ?4, ?4)",
            rusqlite::params![
                Uuid::new_v4().to_string(),
                id,
                project_ruleset_md_path(&id),
                now
            ],
        )?;

        let project = load_project(&tx, &id)?;
        tx.commit()?;
        Ok(project)
    }

    /// `UpdateProject` (spec §5.2). `name`/`description` left untouched when `None`;
    /// `updated_at` only bumps when at least one field is actually provided. Unknown `id` ⇒
    /// `NotFound`; archived project ⇒ `Invariant`.
    pub fn update_project(
        &self,
        id: &str,
        name: Option<&str>,
        description: Option<&str>,
    ) -> Result<Project, OrchdPersistError> {
        let tx = self.conn.unchecked_transaction()?;
        ensure_project_active(&tx, id)?;
        if name.is_some() || description.is_some() {
            tx.execute(
                "UPDATE project SET
                   name = COALESCE(?2, name),
                   description = COALESCE(?3, description),
                   updated_at = ?4
                 WHERE id = ?1",
                rusqlite::params![id, name, description, now_ms()],
            )?;
        }
        let project = load_project(&tx, id)?;
        tx.commit()?;
        Ok(project)
    }

    /// `ArchiveProject` (spec §5.2): sets `status='archived'`. One-way in v1 — archiving an
    /// already-archived project is itself a mutating verb touching an archived project, so it
    /// re-uses [`ensure_project_active`] and fails `Invariant` rather than silently no-op'ing.
    pub fn archive_project(&self, id: &str) -> Result<Project, OrchdPersistError> {
        let tx = self.conn.unchecked_transaction()?;
        ensure_project_active(&tx, id)?;
        tx.execute(
            "UPDATE project SET status = 'archived', updated_at = ?2 WHERE id = ?1",
            rusqlite::params![id, now_ms()],
        )?;
        let project = load_project(&tx, id)?;
        tx.commit()?;
        Ok(project)
    }

    /// `ListProjects` (spec §5.2): every project, each with `workspace_ids` joined by `ord`.
    pub fn list_projects(&self) -> Result<Vec<Project>, OrchdPersistError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM project ORDER BY created_at, id")?;
        let ids: Vec<String> = stmt
            .query_map([], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        drop(stmt);
        ids.iter().map(|id| load_project(&self.conn, id)).collect()
    }

    /// `AddProjectWorkspace` (spec §5.2): appends at `ord = max(ord) + 1`. A `workspace_id`
    /// already linked to ANY project ⇒ `Conflict`; unknown/archived `project_id` ⇒
    /// `NotFound`/`Invariant`.
    pub fn add_project_workspace(
        &self,
        project_id: &str,
        workspace_id: &str,
    ) -> Result<Project, OrchdPersistError> {
        let tx = self.conn.unchecked_transaction()?;
        ensure_project_active(&tx, project_id)?;
        let max_ord: Option<i64> = tx.query_row(
            "SELECT MAX(ord) FROM project_workspace WHERE project_id = ?1",
            rusqlite::params![project_id],
            |r| r.get(0),
        )?;
        tx.execute(
            "INSERT INTO project_workspace (project_id, workspace_id, ord) VALUES (?1, ?2, ?3)",
            rusqlite::params![
                project_id,
                workspace_id,
                max_ord.map(|o| o + 1).unwrap_or(0)
            ],
        )
        .map_err(|e| map_workspace_conflict(e, workspace_id))?;
        let project = load_project(&tx, project_id)?;
        tx.commit()?;
        Ok(project)
    }

    /// `RemoveProjectWorkspace` (spec §5.2): refuses to remove the project's LAST remaining
    /// workspace link (⇒ `Invariant`). Removing a `workspace_id` that isn't actually linked to
    /// `project_id` is an idempotent no-op (mirrors `bpa_sessiond::remove_workspace_root`'s
    /// honest-degradation policy for "nothing to do" — never an error, never a silent
    /// side-effect-that-looks-like-success).
    pub fn remove_project_workspace(
        &self,
        project_id: &str,
        workspace_id: &str,
    ) -> Result<Project, OrchdPersistError> {
        let tx = self.conn.unchecked_transaction()?;
        ensure_project_active(&tx, project_id)?;

        let total: i64 = tx.query_row(
            "SELECT COUNT(*) FROM project_workspace WHERE project_id = ?1",
            rusqlite::params![project_id],
            |r| r.get(0),
        )?;
        let is_linked: i64 = tx.query_row(
            "SELECT COUNT(*) FROM project_workspace WHERE project_id = ?1 AND workspace_id = ?2",
            rusqlite::params![project_id, workspace_id],
            |r| r.get(0),
        )?;
        if is_linked > 0 && total <= 1 {
            return Err(OrchdPersistError::Invariant(
                "cannot remove the project's last workspace link".to_string(),
            ));
        }

        tx.execute(
            "DELETE FROM project_workspace WHERE project_id = ?1 AND workspace_id = ?2",
            rusqlite::params![project_id, workspace_id],
        )?;
        let project = load_project(&tx, project_id)?;
        tx.commit()?;
        Ok(project)
    }

    /// `CreateGoal` (spec §5.2, D5 one-root-tree). The strategic goal is the ONLY root: it is
    /// always parent-less and auto-created with the project, and every other goal is an
    /// `Additional` subgoal that MUST have a non-null parent. So a parent-less `Additional`
    /// (would be a second top-level root) ⇒ `Invariant`, and a `Strategic` with a non-null
    /// `parent_id` ⇒ `Invariant`. A second `kind: Strategic` on a project that already has one ⇒
    /// `Invariant` (checked up front rather than relying on the
    /// `goal_one_strategic_per_project` partial unique index, which exists purely as the DB's
    /// own belt-and-braces backstop). `parent_id` unknown ⇒ `NotFound`; belonging to a
    /// different project ⇒ `Invariant`. `ord = max(sibling ord) + 1`, where siblings share
    /// `(project_id, parent_id)`.
    pub fn create_goal(
        &self,
        project_id: &str,
        parent_id: Option<&str>,
        kind: GoalKind,
        title: &str,
        body: &str,
    ) -> Result<Goal, OrchdPersistError> {
        let tx = self.conn.unchecked_transaction()?;
        ensure_project_active(&tx, project_id)?;

        // D5 one-root-tree: kind ⇄ parent_id must be consistent.
        match (&kind, parent_id) {
            (GoalKind::Additional, None) => {
                return Err(OrchdPersistError::Invariant(
                    "additional goal requires a parent".to_string(),
                ));
            }
            (GoalKind::Strategic, Some(_)) => {
                return Err(OrchdPersistError::Invariant(
                    "strategic goal must be a root (parent_id must be null)".to_string(),
                ));
            }
            _ => {}
        }

        if let Some(parent) = parent_id {
            let parent_project: Option<String> = tx
                .query_row(
                    "SELECT project_id FROM goal WHERE id = ?1",
                    rusqlite::params![parent],
                    |r| r.get(0),
                )
                .optional()?;
            let parent_project = parent_project.ok_or(OrchdPersistError::NotFound)?;
            if parent_project != project_id {
                return Err(OrchdPersistError::Invariant(
                    "goal parent_id must belong to the same project".to_string(),
                ));
            }
        }

        if matches!(kind, GoalKind::Strategic) {
            let existing: i64 = tx.query_row(
                "SELECT COUNT(*) FROM goal WHERE project_id = ?1 AND kind = 'strategic'",
                rusqlite::params![project_id],
                |r| r.get(0),
            )?;
            if existing > 0 {
                return Err(OrchdPersistError::Invariant(
                    "project already has a strategic goal".to_string(),
                ));
            }
        }

        let max_ord: Option<i64> = tx.query_row(
            "SELECT MAX(ord) FROM goal WHERE project_id = ?1 AND parent_id IS ?2",
            rusqlite::params![project_id, parent_id],
            |r| r.get(0),
        )?;

        let id = Uuid::new_v4().to_string();
        let now = now_ms();
        tx.execute(
            "INSERT INTO goal
               (id, project_id, parent_id, kind, title, body, ord, status, metric_refs,
                created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'active', '[]', ?8, ?8)",
            rusqlite::params![
                id,
                project_id,
                parent_id,
                encode_goal_kind(&kind),
                title,
                body,
                max_ord.map(|o| o + 1).unwrap_or(0),
                now
            ],
        )?;

        let goal = load_goal(&tx, &id)?;
        tx.commit()?;
        Ok(goal)
    }

    /// `UpdateGoal` (spec §5.2). Every field independently optional; only provided fields
    /// change, `updated_at` bumps only if at least one did. `metric_refs` round-trips as a JSON
    /// array of strings (Q12 forward). Unknown `id` ⇒ `NotFound`; goal's project archived ⇒
    /// `Invariant`.
    pub fn update_goal(
        &self,
        id: &str,
        title: Option<&str>,
        body: Option<&str>,
        status: Option<GoalStatus>,
        metric_refs: Option<&[String]>,
    ) -> Result<Goal, OrchdPersistError> {
        let tx = self.conn.unchecked_transaction()?;
        let project_id: String = tx
            .query_row(
                "SELECT project_id FROM goal WHERE id = ?1",
                rusqlite::params![id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(OrchdPersistError::NotFound)?;
        ensure_project_active(&tx, &project_id)?;

        let status_text = status.as_ref().map(encode_goal_status);
        let metric_refs_json = metric_refs
            .map(serde_json::to_string)
            .transpose()
            .map_err(|e| OrchdPersistError::Io(format!("failed to serialize metric_refs: {e}")))?;

        if title.is_some() || body.is_some() || status_text.is_some() || metric_refs_json.is_some()
        {
            tx.execute(
                "UPDATE goal SET
                   title = COALESCE(?2, title),
                   body = COALESCE(?3, body),
                   status = COALESCE(?4, status),
                   metric_refs = COALESCE(?5, metric_refs),
                   updated_at = ?6
                 WHERE id = ?1",
                rusqlite::params![id, title, body, status_text, metric_refs_json, now_ms()],
            )?;
        }

        let goal = load_goal(&tx, id)?;
        tx.commit()?;
        Ok(goal)
    }

    /// `MoveGoal` (spec §5.2, D5 one-root-tree). A negative `new_ord` (would corrupt
    /// `list_goals`' zero-padded lexical sort key) ⇒ `Invariant`. Moving the strategic root ⇒
    /// `Invariant`. Moving an `Additional` goal to a null parent (would make it a second
    /// top-level root) ⇒ `Invariant` — the strategic root is the only legitimate null-parent
    /// goal, and it can't be moved. A cross-project `new_parent_id` ⇒ `Invariant`; a cycle
    /// (`new_parent_id` is `id` itself or a descendant of `id`, found by walking up
    /// `new_parent_id`'s ancestor chain) ⇒ `Invariant`.
    pub fn move_goal(
        &self,
        id: &str,
        new_parent_id: Option<&str>,
        new_ord: i64,
    ) -> Result<Goal, OrchdPersistError> {
        if new_ord < 0 {
            return Err(OrchdPersistError::Invariant(
                "new_ord must be non-negative".to_string(),
            ));
        }

        let tx = self.conn.unchecked_transaction()?;
        let (project_id, kind): (String, String) = tx
            .query_row(
                "SELECT project_id, kind FROM goal WHERE id = ?1",
                rusqlite::params![id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .ok_or(OrchdPersistError::NotFound)?;
        ensure_project_active(&tx, &project_id)?;

        if kind == "strategic" {
            return Err(OrchdPersistError::Invariant(
                "the strategic root goal cannot be moved".to_string(),
            ));
        }

        // D5 one-root-tree: a non-strategic goal can never become a root.
        let new_parent = new_parent_id.ok_or_else(|| {
            OrchdPersistError::Invariant("additional goal requires a parent".to_string())
        })?;

        let parent_project: Option<String> = tx
            .query_row(
                "SELECT project_id FROM goal WHERE id = ?1",
                rusqlite::params![new_parent],
                |r| r.get(0),
            )
            .optional()?;
        let parent_project = parent_project.ok_or(OrchdPersistError::NotFound)?;
        if parent_project != project_id {
            return Err(OrchdPersistError::Invariant(
                "new_parent_id must belong to the same project".to_string(),
            ));
        }
        if ancestor_chain_contains(&tx, new_parent, id)? {
            return Err(OrchdPersistError::Invariant(
                "cannot move a goal under itself or one of its own descendants".to_string(),
            ));
        }

        tx.execute(
            "UPDATE goal SET parent_id = ?2, ord = ?3, updated_at = ?4 WHERE id = ?1",
            rusqlite::params![id, new_parent_id, new_ord, now_ms()],
        )?;
        let goal = load_goal(&tx, id)?;
        tx.commit()?;
        Ok(goal)
    }

    /// `DeleteGoal` (spec §5.2). Deleting the strategic root ⇒ `Invariant`; otherwise the FK
    /// `goal.parent_id REFERENCES goal(id) ON DELETE CASCADE` removes the whole subtree.
    pub fn delete_goal(&self, id: &str) -> Result<(), OrchdPersistError> {
        let tx = self.conn.unchecked_transaction()?;
        let (project_id, kind): (String, String) = tx
            .query_row(
                "SELECT project_id, kind FROM goal WHERE id = ?1",
                rusqlite::params![id],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?
            .ok_or(OrchdPersistError::NotFound)?;
        ensure_project_active(&tx, &project_id)?;

        if kind == "strategic" {
            return Err(OrchdPersistError::Invariant(
                "the strategic root goal cannot be deleted".to_string(),
            ));
        }

        tx.execute("DELETE FROM goal WHERE id = ?1", rusqlite::params![id])?;
        tx.commit()?;
        Ok(())
    }

    /// `ListGoals` (spec §5.2): every goal for `project_id`, parents before children then `ord`
    /// at EVERY depth. A `WITH RECURSIVE` walk builds a dot-joined, zero-padded-`ord` sort key
    /// per node (`"000000000000"`, `"000000000000.000000000001"`, ...) so a parent's key is
    /// always a strict string-prefix of its descendants' keys, which lexical `ORDER BY` turns
    /// into a full pre-order depth-first listing — the strategic root (the only
    /// `parent_id IS NULL` row) always sorts first, and this holds at any depth, unlike sorting
    /// on `(parent_id IS NOT NULL), parent_id, ord` alone (which only orders correctly for a
    /// 2-level tree — see the `list_goals_parents_before_children_then_ord` test below for a
    /// concrete case that heuristic gets wrong and this one gets right). Reads work on an
    /// archived project (spec §5.2 "reads still work"); unknown `project_id` ⇒ `NotFound`.
    pub fn list_goals(&self, project_id: &str) -> Result<Vec<Goal>, OrchdPersistError> {
        let exists: bool = self
            .conn
            .query_row(
                "SELECT 1 FROM project WHERE id = ?1",
                rusqlite::params![project_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !exists {
            return Err(OrchdPersistError::NotFound);
        }

        let mut stmt = self.conn.prepare(
            "WITH RECURSIVE tree(id, sort_key) AS (
               SELECT id, printf('%012d', ord)
                 FROM goal WHERE project_id = ?1 AND parent_id IS NULL
               UNION ALL
               SELECT g.id, tree.sort_key || '.' || printf('%012d', g.ord)
                 FROM goal g JOIN tree ON g.parent_id = tree.id
                WHERE g.project_id = ?1
             )
             SELECT goal.id, goal.project_id, goal.parent_id, goal.kind, goal.title, goal.body,
                    goal.ord, goal.status, goal.metric_refs, goal.created_at, goal.updated_at
             FROM goal
             JOIN tree ON goal.id = tree.id
             WHERE goal.project_id = ?1
             ORDER BY tree.sort_key",
        )?;
        let rows: Vec<GoalRow> = stmt
            .query_map(rusqlite::params![project_id], GoalRow::from_row)?
            .collect::<Result<_, _>>()?;
        rows.into_iter().map(GoalRow::into_goal).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TABLES: &[&str] = &[
        "project",
        "project_workspace",
        "goal",
        "idea",
        "insight",
        "task",
        "ruleset",
    ];

    fn table_exists(conn: &Connection, name: &str) -> bool {
        conn.query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
            [name],
            |_| Ok(()),
        )
        .is_ok()
    }

    fn user_version(conn: &Connection) -> i64 {
        conn.query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap()
    }

    #[test]
    fn open_in_memory_creates_schema_v1_with_every_table() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(user_version(db.conn()), 1);
        for table in TABLES {
            assert!(table_exists(db.conn(), table), "missing table {table}");
        }
    }

    #[test]
    fn open_on_disk_creates_schema_v1_with_every_table() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("orchd.db");
        let db = Db::open(&path).unwrap();
        assert_eq!(user_version(db.conn()), 1);
        for table in TABLES {
            assert!(table_exists(db.conn(), table), "missing table {table}");
        }
    }

    #[test]
    fn foreign_keys_are_enforced() {
        let db = Db::open_in_memory().unwrap();
        let err = db
            .conn()
            .execute(
                "INSERT INTO goal (id, project_id, kind, title, created_at, updated_at)
                 VALUES ('g1', 'no-such-project', 'strategic', 't', 0, 0)",
                [],
            )
            .unwrap_err();
        assert!(matches!(err, rusqlite::Error::SqliteFailure(_, _)));
    }

    #[test]
    fn corrupt_db_is_quarantined_and_recreated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("orchd.db");
        std::fs::write(&path, b"not a sqlite database").unwrap();

        let db = Db::open(&path).expect("open must quarantine and recreate, not error");
        assert_eq!(user_version(db.conn()), 1);

        let found = std::fs::read_dir(dir.path()).unwrap().any(|e| {
            e.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("orchd.db.corrupt-")
        });
        assert!(found, "expected an orchd.db.corrupt-<ts> quarantine file");
    }

    #[test]
    fn checkpoint_on_disk_db_does_not_error() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("orchd.db");
        let db = Db::open(&path).unwrap();
        db.checkpoint().unwrap();
    }

    #[test]
    fn checkpoint_on_in_memory_db_does_not_error() {
        let db = Db::open_in_memory().unwrap();
        db.checkpoint().unwrap();
    }
}

/// Domain CRUD tests (spec §5.2): project + project_workspace + goal, every invariant from the
/// table in §5.2, TDD (written RED before `impl Db { create_project, ... }` above went GREEN).
#[cfg(test)]
mod domain_tests {
    use super::*;

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn ruleset_row(db: &Db, project_id: &str) -> (String, String, String, String, String) {
        db.conn()
            .query_row(
                "SELECT scope, project_id, md_path, md_hash, policy FROM ruleset WHERE project_id = ?1",
                rusqlite::params![project_id],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                        r.get::<_, String>(3)?,
                        r.get::<_, String>(4)?,
                    ))
                },
            )
            .expect("project ruleset row must exist")
    }

    // ---- create_project ----

    #[test]
    fn create_project_creates_strategic_goal_and_ruleset_row() {
        let db = Db::open_in_memory().unwrap();
        let project = db
            .create_project("Acme", "desc", &ids(&["w1", "w2"]))
            .unwrap();

        assert!(
            uuid::Uuid::parse_str(&project.id).is_ok(),
            "id must be a uuid"
        );
        assert_eq!(project.name, "Acme");
        assert_eq!(project.description, "desc");
        assert_eq!(project.status, ProjectStatus::Active);
        assert_eq!(project.workspace_ids, ids(&["w1", "w2"]));
        // spec §5.1: "Timestamps unix-ms" — a unix-seconds value here would be ~10 digits
        // (< 3_000_000_000); a unix-ms "today" value is ~13 digits.
        assert!(
            project.created_at > 1_700_000_000_000,
            "created_at must be unix-ms, got {}",
            project.created_at
        );
        assert_eq!(project.created_at, project.updated_at);

        let goals = db.list_goals(&project.id).unwrap();
        assert_eq!(goals.len(), 1);
        assert_eq!(goals[0].kind, GoalKind::Strategic);
        assert_eq!(goals[0].title, "Стратегическая цель");
        assert_eq!(goals[0].body, "");
        assert!(goals[0].parent_id.is_none());
        assert!(
            uuid::Uuid::parse_str(&goals[0].id).is_ok(),
            "goal id must be a uuid"
        );

        let (scope, ruleset_project_id, md_path, md_hash, policy) = ruleset_row(&db, &project.id);
        assert_eq!(scope, "project");
        assert_eq!(ruleset_project_id, project.id);
        assert!(md_path.contains(&format!("project-{}.md", project.id)));
        assert_eq!(md_hash, "");
        assert_eq!(policy, "{}");
    }

    #[test]
    fn create_project_empty_workspace_ids_is_invariant() {
        let db = Db::open_in_memory().unwrap();
        let err = db.create_project("Acme", "", &[]).unwrap_err();
        assert!(matches!(err, OrchdPersistError::Invariant(_)));
        assert_eq!(db.list_projects().unwrap().len(), 0);
    }

    #[test]
    fn create_project_workspace_linked_to_another_project_is_conflict() {
        let db = Db::open_in_memory().unwrap();
        db.create_project("A", "", &ids(&["w1"])).unwrap();
        let err = db.create_project("B", "", &ids(&["w1"])).unwrap_err();
        assert!(matches!(err, OrchdPersistError::Conflict(_)));
    }

    #[test]
    fn create_project_duplicate_workspace_id_in_same_call_is_conflict() {
        let db = Db::open_in_memory().unwrap();
        let err = db.create_project("A", "", &ids(&["w1", "w1"])).unwrap_err();
        assert!(matches!(err, OrchdPersistError::Conflict(_)));
        // ONE transaction: the failed link insert rolls back the whole create, no project or
        // goal or ruleset row survives.
        assert_eq!(db.list_projects().unwrap().len(), 0);
    }

    // ---- update_project / archive_project / list_projects ----

    #[test]
    fn update_project_changes_only_provided_fields() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "d0", &ids(&["w1"])).unwrap();
        let updated = db.update_project(&project.id, Some("A2"), None).unwrap();
        assert_eq!(updated.name, "A2");
        assert_eq!(updated.description, "d0");
        assert!(updated.updated_at >= project.updated_at);
    }

    #[test]
    fn update_project_unknown_id_is_not_found() {
        let db = Db::open_in_memory().unwrap();
        let err = db.update_project("nope", Some("x"), None).unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound));
    }

    #[test]
    fn archive_project_sets_status_archived() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let archived = db.archive_project(&project.id).unwrap();
        assert_eq!(archived.status, ProjectStatus::Archived);
    }

    #[test]
    fn archived_project_blocks_update_project() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        db.archive_project(&project.id).unwrap();
        let err = db.update_project(&project.id, Some("x"), None).unwrap_err();
        assert!(matches!(err, OrchdPersistError::Invariant(_)));
    }

    #[test]
    fn archived_project_blocks_archive_project_again() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        db.archive_project(&project.id).unwrap();
        let err = db.archive_project(&project.id).unwrap_err();
        assert!(matches!(err, OrchdPersistError::Invariant(_)));
    }

    #[test]
    fn archived_project_blocks_create_goal() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        db.archive_project(&project.id).unwrap();
        let err = db
            .create_goal(&project.id, None, GoalKind::Additional, "g", "")
            .unwrap_err();
        assert!(matches!(err, OrchdPersistError::Invariant(_)));
    }

    #[test]
    fn archived_project_list_goals_still_works() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        db.archive_project(&project.id).unwrap();
        let goals = db.list_goals(&project.id).unwrap();
        assert_eq!(goals.len(), 1, "reads still work on an archived project");
    }

    #[test]
    fn list_projects_returns_every_project() {
        let db = Db::open_in_memory().unwrap();
        db.create_project("A", "", &ids(&["w1"])).unwrap();
        db.create_project("B", "", &ids(&["w2"])).unwrap();
        let all = db.list_projects().unwrap();
        assert_eq!(all.len(), 2);
    }

    // ---- add_project_workspace / remove_project_workspace ----

    #[test]
    fn add_project_workspace_appends_and_conflicts_when_linked_elsewhere() {
        let db = Db::open_in_memory().unwrap();
        let a = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let b = db.create_project("B", "", &ids(&["w2"])).unwrap();

        let a2 = db.add_project_workspace(&a.id, "w3").unwrap();
        assert_eq!(a2.workspace_ids, ids(&["w1", "w3"]));

        let err = db.add_project_workspace(&b.id, "w1").unwrap_err();
        assert!(matches!(err, OrchdPersistError::Conflict(_)));
    }

    #[test]
    fn add_project_workspace_unknown_project_is_not_found() {
        let db = Db::open_in_memory().unwrap();
        let err = db.add_project_workspace("nope", "w1").unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound));
    }

    #[test]
    fn remove_project_workspace_removes_link() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1", "w2"])).unwrap();
        let updated = db.remove_project_workspace(&project.id, "w1").unwrap();
        assert_eq!(updated.workspace_ids, ids(&["w2"]));
    }

    #[test]
    fn remove_project_workspace_last_link_is_invariant() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let err = db.remove_project_workspace(&project.id, "w1").unwrap_err();
        assert!(matches!(err, OrchdPersistError::Invariant(_)));
    }

    #[test]
    fn remove_project_workspace_unlinked_id_is_a_no_op_even_at_one_remaining() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        // "w-other" was never linked, so removing it must not trip the last-link guard even
        // though the project currently has exactly one (unrelated) link.
        let unchanged = db.remove_project_workspace(&project.id, "w-other").unwrap();
        assert_eq!(unchanged.workspace_ids, ids(&["w1"]));
    }

    // ---- archived-project guard coverage (T6 review): every mutator touching an archived
    // project (or a goal within one) ⇒ Invariant("project archived"), and the underlying row is
    // left unchanged. update_project / archive_project / create_goal / update_goal are covered
    // above; these four close the gap for the remaining mutators. ----

    #[test]
    fn archived_project_blocks_add_project_workspace() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        db.archive_project(&project.id).unwrap();
        let err = db.add_project_workspace(&project.id, "w2").unwrap_err();
        assert!(
            matches!(&err, OrchdPersistError::Invariant(m) if m == "project archived"),
            "got {err:?}"
        );
        // row unchanged: no w2 link was added.
        assert_eq!(
            db.list_projects().unwrap()[0].workspace_ids,
            ids(&["w1"]),
            "archived add must not mutate the link set"
        );
    }

    #[test]
    fn archived_project_blocks_remove_project_workspace() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1", "w2"])).unwrap();
        db.archive_project(&project.id).unwrap();
        let err = db.remove_project_workspace(&project.id, "w1").unwrap_err();
        assert!(
            matches!(&err, OrchdPersistError::Invariant(m) if m == "project archived"),
            "got {err:?}"
        );
        // row unchanged: both links survive.
        assert_eq!(
            db.list_projects().unwrap()[0].workspace_ids,
            ids(&["w1", "w2"]),
            "archived remove must not mutate the link set"
        );
    }

    #[test]
    fn archived_project_blocks_move_goal() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let root_id = db.list_goals(&project.id).unwrap()[0].id.clone();
        let c1 = db
            .create_goal(&project.id, Some(&root_id), GoalKind::Additional, "c1", "")
            .unwrap();
        let c2 = db
            .create_goal(&project.id, Some(&root_id), GoalKind::Additional, "c2", "")
            .unwrap();
        db.archive_project(&project.id).unwrap();

        let err = db.move_goal(&c2.id, Some(&c1.id), 0).unwrap_err();
        assert!(
            matches!(&err, OrchdPersistError::Invariant(m) if m == "project archived"),
            "got {err:?}"
        );
        // row unchanged: c2 still parented to the root at its original ord, not moved under c1.
        let c2_after = db
            .list_goals(&project.id)
            .unwrap()
            .into_iter()
            .find(|g| g.id == c2.id)
            .unwrap();
        assert_eq!(c2_after.parent_id.as_deref(), Some(root_id.as_str()));
        assert_eq!(c2_after.ord, c2.ord);
    }

    #[test]
    fn archived_project_blocks_delete_goal() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let root_id = db.list_goals(&project.id).unwrap()[0].id.clone();
        let child = db
            .create_goal(&project.id, Some(&root_id), GoalKind::Additional, "c", "")
            .unwrap();
        db.archive_project(&project.id).unwrap();

        let err = db.delete_goal(&child.id).unwrap_err();
        assert!(
            matches!(&err, OrchdPersistError::Invariant(m) if m == "project archived"),
            "got {err:?}"
        );
        // row unchanged: the child goal still exists.
        assert_eq!(
            db.list_goals(&project.id).unwrap().len(),
            2,
            "archived delete must not remove the goal"
        );
    }

    // ---- create_goal ----

    #[test]
    fn create_goal_second_strategic_is_invariant() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let err = db
            .create_goal(&project.id, None, GoalKind::Strategic, "x", "")
            .unwrap_err();
        assert!(matches!(err, OrchdPersistError::Invariant(_)));
    }

    #[test]
    fn create_goal_additional_without_parent_is_invariant() {
        // D5 / §5.2 one-root-tree: strategic is the ONLY root; every Additional goal is a
        // subgoal and MUST have a non-null parent. A parent-less Additional would be a second
        // top-level root beside the strategic goal.
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let err = db
            .create_goal(&project.id, None, GoalKind::Additional, "orphan", "")
            .unwrap_err();
        assert!(
            matches!(&err, OrchdPersistError::Invariant(m) if m == "additional goal requires a parent"),
            "got {err:?}"
        );
        // no second root was created: only the auto strategic goal remains.
        assert_eq!(db.list_goals(&project.id).unwrap().len(), 1);
    }

    #[test]
    fn create_goal_strategic_with_parent_is_invariant() {
        // Symmetric sanity: the strategic root is always parent-less; a caller must never create
        // a strategic goal with a non-null parent.
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let root_id = db.list_goals(&project.id).unwrap()[0].id.clone();
        let err = db
            .create_goal(&project.id, Some(&root_id), GoalKind::Strategic, "x", "")
            .unwrap_err();
        assert!(
            matches!(&err, OrchdPersistError::Invariant(m) if m == "strategic goal must be a root (parent_id must be null)"),
            "got {err:?}"
        );
    }

    #[test]
    fn create_goal_cross_project_parent_is_invariant() {
        let db = Db::open_in_memory().unwrap();
        let a = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let b = db.create_project("B", "", &ids(&["w2"])).unwrap();
        let b_root_id = db.list_goals(&b.id).unwrap()[0].id.clone();
        let err = db
            .create_goal(&a.id, Some(&b_root_id), GoalKind::Additional, "x", "")
            .unwrap_err();
        assert!(matches!(err, OrchdPersistError::Invariant(_)));
    }

    #[test]
    fn create_goal_unknown_project_is_not_found() {
        let db = Db::open_in_memory().unwrap();
        let err = db
            .create_goal("nope", None, GoalKind::Additional, "x", "")
            .unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound));
    }

    #[test]
    fn create_goal_unknown_parent_is_not_found() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let err = db
            .create_goal(&project.id, Some("nope"), GoalKind::Additional, "x", "")
            .unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound));
    }

    #[test]
    fn create_goal_ord_increments_per_sibling_group() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let root_id = db.list_goals(&project.id).unwrap()[0].id.clone();
        let c1 = db
            .create_goal(&project.id, Some(&root_id), GoalKind::Additional, "c1", "")
            .unwrap();
        let c2 = db
            .create_goal(&project.id, Some(&root_id), GoalKind::Additional, "c2", "")
            .unwrap();
        assert_eq!(c1.ord, 0);
        assert_eq!(c2.ord, 1);
    }

    // ---- update_goal ----

    #[test]
    fn update_goal_updates_fields_and_metric_refs_round_trip() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let root_id = db.list_goals(&project.id).unwrap()[0].id.clone();
        let goal = db
            .create_goal(&project.id, Some(&root_id), GoalKind::Additional, "t", "b")
            .unwrap();

        let updated = db
            .update_goal(
                &goal.id,
                Some("t2"),
                None,
                Some(GoalStatus::Achieved),
                Some(&ids(&["m1", "m2"])),
            )
            .unwrap();
        assert_eq!(updated.title, "t2");
        assert_eq!(updated.body, "b", "body left untouched when None");
        assert_eq!(updated.status, GoalStatus::Achieved);
        assert_eq!(updated.metric_refs, ids(&["m1", "m2"]));

        // re-fetch independently (via list_goals) to prove the JSON round-tripped through SQLite,
        // not just through the in-memory `Goal` returned by `update_goal`.
        let refetched = db
            .list_goals(&project.id)
            .unwrap()
            .into_iter()
            .find(|g| g.id == goal.id)
            .unwrap();
        assert_eq!(refetched.metric_refs, ids(&["m1", "m2"]));
    }

    #[test]
    fn update_goal_unknown_id_is_not_found() {
        let db = Db::open_in_memory().unwrap();
        let err = db
            .update_goal("nope", Some("x"), None, None, None)
            .unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound));
    }

    #[test]
    fn archived_project_blocks_update_goal() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let root_id = db.list_goals(&project.id).unwrap()[0].id.clone();
        db.archive_project(&project.id).unwrap();
        let err = db
            .update_goal(&root_id, Some("x"), None, None, None)
            .unwrap_err();
        assert!(matches!(err, OrchdPersistError::Invariant(_)));
    }

    // ---- move_goal ----

    #[test]
    fn move_goal_strategic_root_is_invariant() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let root_id = db.list_goals(&project.id).unwrap()[0].id.clone();
        let err = db.move_goal(&root_id, None, 0).unwrap_err();
        assert!(matches!(err, OrchdPersistError::Invariant(_)));
    }

    #[test]
    fn move_goal_cross_project_parent_is_invariant() {
        let db = Db::open_in_memory().unwrap();
        let a = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let b = db.create_project("B", "", &ids(&["w2"])).unwrap();
        let a_root_id = db.list_goals(&a.id).unwrap()[0].id.clone();
        let b_root_id = db.list_goals(&b.id).unwrap()[0].id.clone();
        let child = db
            .create_goal(&a.id, Some(&a_root_id), GoalKind::Additional, "c", "")
            .unwrap();
        let err = db.move_goal(&child.id, Some(&b_root_id), 0).unwrap_err();
        assert!(matches!(err, OrchdPersistError::Invariant(_)));
    }

    #[test]
    fn move_goal_under_own_descendant_or_self_is_cycle_invariant() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let root_id = db.list_goals(&project.id).unwrap()[0].id.clone();
        let child = db
            .create_goal(
                &project.id,
                Some(&root_id),
                GoalKind::Additional,
                "child",
                "",
            )
            .unwrap();
        let grandchild = db
            .create_goal(&project.id, Some(&child.id), GoalKind::Additional, "gc", "")
            .unwrap();

        // reparent `child` under its own descendant `grandchild` -> cycle.
        let err = db
            .move_goal(&child.id, Some(&grandchild.id), 0)
            .unwrap_err();
        assert!(matches!(err, OrchdPersistError::Invariant(_)));

        // reparent a goal under itself -> cycle.
        let err = db.move_goal(&child.id, Some(&child.id), 0).unwrap_err();
        assert!(matches!(err, OrchdPersistError::Invariant(_)));
    }

    #[test]
    fn move_goal_updates_parent_and_ord() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let root_id = db.list_goals(&project.id).unwrap()[0].id.clone();
        let c1 = db
            .create_goal(&project.id, Some(&root_id), GoalKind::Additional, "c1", "")
            .unwrap();
        let c2 = db
            .create_goal(&project.id, Some(&root_id), GoalKind::Additional, "c2", "")
            .unwrap();

        let moved = db.move_goal(&c2.id, Some(&c1.id), 7).unwrap();
        assert_eq!(moved.parent_id.as_deref(), Some(c1.id.as_str()));
        assert_eq!(moved.ord, 7);
    }

    #[test]
    fn move_goal_unknown_id_is_not_found() {
        let db = Db::open_in_memory().unwrap();
        let err = db.move_goal("nope", None, 0).unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound));
    }

    #[test]
    fn move_goal_unknown_new_parent_is_not_found() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let root_id = db.list_goals(&project.id).unwrap()[0].id.clone();
        let child = db
            .create_goal(
                &project.id,
                Some(&root_id),
                GoalKind::Additional,
                "child",
                "",
            )
            .unwrap();
        let err = db.move_goal(&child.id, Some("nope"), 0).unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound));
    }

    #[test]
    fn move_goal_negative_ord_is_invariant() {
        // A negative ord would corrupt list_goals' zero-padded (`printf('%012d', ord)`) lexical
        // sort key, so it's rejected up front and the goal is left where it was.
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let root_id = db.list_goals(&project.id).unwrap()[0].id.clone();
        let child = db
            .create_goal(&project.id, Some(&root_id), GoalKind::Additional, "c", "")
            .unwrap();
        let err = db.move_goal(&child.id, Some(&root_id), -1).unwrap_err();
        assert!(
            matches!(&err, OrchdPersistError::Invariant(m) if m == "new_ord must be non-negative"),
            "got {err:?}"
        );
        let after = db
            .list_goals(&project.id)
            .unwrap()
            .into_iter()
            .find(|g| g.id == child.id)
            .unwrap();
        assert_eq!(after.ord, child.ord, "rejected move must not change ord");
    }

    #[test]
    fn move_goal_additional_to_root_is_invariant() {
        // D5 one-root-tree consistency with create_goal: an Additional goal can never become a
        // second top-level root, so moving it to a null parent is rejected. (The strategic root
        // is the only legitimate null-parent goal, and it can't be moved at all.)
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let root_id = db.list_goals(&project.id).unwrap()[0].id.clone();
        let child = db
            .create_goal(&project.id, Some(&root_id), GoalKind::Additional, "c", "")
            .unwrap();
        let err = db.move_goal(&child.id, None, 0).unwrap_err();
        assert!(
            matches!(&err, OrchdPersistError::Invariant(m) if m == "additional goal requires a parent"),
            "got {err:?}"
        );
        let after = db
            .list_goals(&project.id)
            .unwrap()
            .into_iter()
            .find(|g| g.id == child.id)
            .unwrap();
        assert_eq!(
            after.parent_id.as_deref(),
            Some(root_id.as_str()),
            "rejected move must leave the goal parented"
        );
    }

    // ---- delete_goal ----

    #[test]
    fn delete_goal_strategic_is_invariant() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let root_id = db.list_goals(&project.id).unwrap()[0].id.clone();
        let err = db.delete_goal(&root_id).unwrap_err();
        assert!(matches!(err, OrchdPersistError::Invariant(_)));
    }

    #[test]
    fn delete_goal_unknown_id_is_not_found() {
        let db = Db::open_in_memory().unwrap();
        let err = db.delete_goal("nope").unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound));
    }

    #[test]
    fn delete_goal_cascades_subtree() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let root_id = db.list_goals(&project.id).unwrap()[0].id.clone();
        let child = db
            .create_goal(
                &project.id,
                Some(&root_id),
                GoalKind::Additional,
                "child",
                "",
            )
            .unwrap();
        let grandchild = db
            .create_goal(&project.id, Some(&child.id), GoalKind::Additional, "gc", "")
            .unwrap();

        db.delete_goal(&child.id).unwrap();

        let remaining = db.list_goals(&project.id).unwrap();
        assert_eq!(remaining.len(), 1, "child AND grandchild must both be gone");
        assert_eq!(remaining[0].id, root_id);

        // belt-and-braces: the grandchild row is gone at the SQL level too, not just filtered
        // out of list_goals.
        let raw_count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM goal WHERE id = ?1",
                rusqlite::params![grandchild.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(raw_count, 0);
    }

    // ---- list_goals ----

    #[test]
    fn list_goals_parents_before_children_then_ord() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let root_id = db.list_goals(&project.id).unwrap()[0].id.clone();

        // c2 is created FIRST (ord=0 under root), c1 SECOND (ord=1 under root); c2 gets a
        // grandchild. A naive `ORDER BY (parent_id IS NOT NULL), parent_id, ord` would sort
        // c2's grandchild against c1 by their raw `parent_id` text, not by tree depth — this
        // test pins the actually-correct DFS order instead.
        let c2 = db
            .create_goal(&project.id, Some(&root_id), GoalKind::Additional, "c2", "")
            .unwrap();
        let c2_grandchild = db
            .create_goal(&project.id, Some(&c2.id), GoalKind::Additional, "c2-gc", "")
            .unwrap();
        let c1 = db
            .create_goal(&project.id, Some(&root_id), GoalKind::Additional, "c1", "")
            .unwrap();

        let goals = db.list_goals(&project.id).unwrap();
        assert_eq!(goals.len(), 4);
        let order: Vec<&str> = goals.iter().map(|g| g.id.as_str()).collect();
        let pos = |id: &str| order.iter().position(|x| *x == id).unwrap();

        assert_eq!(order[0], root_id, "strategic root must sort first");
        assert!(
            pos(&c2.id) < pos(&c2_grandchild.id),
            "a parent must sort before its own child"
        );
        assert!(
            pos(&c2_grandchild.id) < pos(&c1.id),
            "c2's whole subtree (incl. its grandchild) must sort before c2's later sibling c1"
        );
    }

    #[test]
    fn list_goals_unknown_project_is_not_found() {
        let db = Db::open_in_memory().unwrap();
        let err = db.list_goals("nope").unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound));
    }
}
