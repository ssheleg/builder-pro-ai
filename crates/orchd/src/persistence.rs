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

use rusqlite::Connection;
use tracing::{info, warn};

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
