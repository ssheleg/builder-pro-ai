//! Durable SQLite persistence for the session daemon (spec §11).
//! Best-effort: the in-memory ring is the Layer-1 source of truth; this layer
//! degrades honestly (logs, never panics) on lock/disk/corruption failures.

use std::fmt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use bpa_protocol::{SessionId, SessionLifecycle, SessionMeta, Workspace};
use rusqlite::Connection;
use tracing::{info, warn};

/// Current schema/migration version stored in `PRAGMA user_version`.
pub const SCHEMA_VERSION: i64 = 2;

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

fn quarantine(path: &Path) -> PathBuf {
    let ts = now_secs();
    let mut q = path.as_os_str().to_os_string();
    q.push(format!(".corrupt-{ts}"));
    PathBuf::from(q)
}

impl Db {
    /// Open (or create) the database at `path`. Sets WAL + busy_timeout, runs
    /// migrations in a transaction. On a corrupt image, quarantines the file
    /// (`bpa.db.corrupt-<ts>`) and recreates a fresh database rather than crashing.
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

    /// Open a private, in-memory database (test-support). Not durable across
    /// process restarts — intended for unit/integration tests in downstream
    /// tasks that need a `Db` without touching the filesystem.
    pub fn open_in_memory() -> Result<Db, PersistError> {
        let conn = Connection::open_in_memory().map_err(|e| PersistError::Open(e.to_string()))?;
        // In-memory databases can't use WAL (no shared-memory file backing), but we
        // still apply busy_timeout + foreign_keys for parity with the on-disk path.
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

    /// Force a WAL checkpoint, flushing the write-ahead log into the main database
    /// file (test-support + graceful-shutdown helper; spec §11 "flush + WAL
    /// checkpoint on graceful shutdown"). Best-effort: any failure is a typed error,
    /// never a panic.
    pub fn checkpoint(&self) -> Result<(), PersistError> {
        self.conn
            .pragma_update(None, "wal_checkpoint", "TRUNCATE")
            .map_err(classify)?;
        Ok(())
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

    /// Run migrations from `from_version` to `SCHEMA_VERSION` in one transaction.
    /// Fails closed (typed error) on any error — never panics (spec §11).
    fn migrate(&self, from_version: i64) -> Result<(), PersistError> {
        if from_version == SCHEMA_VERSION {
            return Ok(());
        }
        if from_version > SCHEMA_VERSION {
            return Err(PersistError::Migration(format!(
                "db user_version {from_version} newer than supported {SCHEMA_VERSION}"
            )));
        }
        let tx = self
            .conn
            .unchecked_transaction()
            .map_err(|e| PersistError::Migration(e.to_string()))?;
        if from_version < 1 {
            tx.execute_batch(
                "CREATE TABLE IF NOT EXISTS workspace (
                   id TEXT PRIMARY KEY, name TEXT NOT NULL, root_path TEXT NOT NULL);
                 CREATE TABLE IF NOT EXISTS session (
                   id TEXT PRIMARY KEY,
                   workspace_id TEXT NOT NULL REFERENCES workspace(id),
                   title TEXT NOT NULL, shell TEXT NOT NULL, cwd TEXT NOT NULL,
                   cols INTEGER NOT NULL, rows INTEGER NOT NULL,
                   lifecycle TEXT NOT NULL,
                   exit_code INTEGER, exit_signal TEXT,
                   created_at INTEGER NOT NULL);
                 CREATE TABLE IF NOT EXISTS scrollback (
                   session_id TEXT NOT NULL REFERENCES session(id),
                   seq INTEGER NOT NULL, bytes BLOB NOT NULL, ts INTEGER NOT NULL,
                   PRIMARY KEY (session_id, seq));",
            )
            .map_err(|e| PersistError::Migration(e.to_string()))?;
        }
        if from_version < 2 {
            tx.execute_batch(
                "CREATE TABLE IF NOT EXISTS command_events (
                   session_id TEXT NOT NULL REFERENCES session(id),
                   seq        INTEGER NOT NULL,
                   ts         INTEGER NOT NULL,
                   kind       TEXT NOT NULL,
                   exit_code  INTEGER,
                   origin     TEXT NOT NULL DEFAULT 'gui',
                   PRIMARY KEY (session_id, seq));",
            )
            .map_err(|e| PersistError::Migration(e.to_string()))?;
        }
        tx.pragma_update(None, "user_version", SCHEMA_VERSION)
            .map_err(|e| PersistError::Migration(e.to_string()))?;
        tx.commit()
            .map_err(|e| PersistError::Migration(e.to_string()))?;
        Ok(())
    }
}

/// Encode a lifecycle into (tag, exit_code, exit_signal) columns (spec §11).
fn encode_lifecycle(lc: &SessionLifecycle) -> (&'static str, Option<i64>, Option<String>) {
    match lc {
        SessionLifecycle::AtPrompt => ("atPrompt", None, None),
        SessionLifecycle::Typing => ("typing", None, None),
        SessionLifecycle::Running => ("running", None, None),
        SessionLifecycle::Exited { code, signal } => {
            ("exited", code.map(|c| c as i64), signal.clone())
        }
    }
}

/// Decode (tag, exit_code, exit_signal) back into a lifecycle (spec §11).
fn decode_lifecycle(
    tag: &str,
    exit_code: Option<i64>,
    exit_signal: Option<String>,
) -> Result<SessionLifecycle, PersistError> {
    match tag {
        "atPrompt" => Ok(SessionLifecycle::AtPrompt),
        "typing" => Ok(SessionLifecycle::Typing),
        "running" => Ok(SessionLifecycle::Running),
        "exited" => Ok(SessionLifecycle::Exited {
            code: exit_code.map(|c| (c & 0xff) as u8),
            signal: exit_signal,
        }),
        other => Err(PersistError::Sql(format!(
            "unknown lifecycle tag {other:?}"
        ))),
    }
}

impl Db {
    pub fn upsert_workspace(&self, ws: &Workspace) -> Result<(), PersistError> {
        self.conn.execute(
            "INSERT INTO workspace (id, name, root_path) VALUES (?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, root_path = excluded.root_path",
            rusqlite::params![ws.id, ws.name, ws.root_path],
        )?;
        Ok(())
    }

    pub fn list_workspaces(&self) -> Result<Vec<Workspace>, PersistError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name, root_path FROM workspace ORDER BY id")?;
        let rows = stmt.query_map([], |r| {
            Ok(Workspace {
                id: r.get(0)?,
                name: r.get(1)?,
                root_path: r.get(2)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }

    pub fn upsert_session(&self, meta: &SessionMeta) -> Result<(), PersistError> {
        let (tag, exit_code, exit_signal) = encode_lifecycle(&meta.lifecycle);
        self.conn.execute(
            "INSERT INTO session
               (id, workspace_id, title, shell, cwd, cols, rows, lifecycle,
                exit_code, exit_signal, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
             ON CONFLICT(id) DO UPDATE SET
               workspace_id = excluded.workspace_id, title = excluded.title,
               shell = excluded.shell, cwd = excluded.cwd, cols = excluded.cols,
               rows = excluded.rows, lifecycle = excluded.lifecycle,
               exit_code = excluded.exit_code, exit_signal = excluded.exit_signal,
               created_at = excluded.created_at",
            rusqlite::params![
                meta.id,
                meta.workspace_id,
                meta.title,
                meta.shell,
                meta.cwd,
                meta.cols as i64,
                meta.rows as i64,
                tag,
                exit_code,
                exit_signal,
                meta.created_at,
            ],
        )?;
        Ok(())
    }

    pub fn list_sessions(&self) -> Result<Vec<SessionMeta>, PersistError> {
        self.query_sessions()
    }

    /// Rehydrate on restart (spec §11): every session is is_active=false,
    /// waiting_for_input=false because its PTY is gone.
    pub fn rehydrate(&self) -> Result<Vec<SessionMeta>, PersistError> {
        self.query_sessions()
    }

    /// Shared query path for `list_sessions`/`rehydrate`. Persisted rows never carry
    /// `is_active`/`waiting_for_input` state (those are runtime-only, in-memory
    /// concepts — S1 never persists `true` for either), so both accessors always
    /// return `false` for them; the two names exist for call-site clarity.
    fn query_sessions(&self) -> Result<Vec<SessionMeta>, PersistError> {
        let mut stmt = self.conn.prepare(
            "SELECT id, workspace_id, title, shell, cwd, cols, rows,
                    lifecycle, exit_code, exit_signal, created_at
             FROM session ORDER BY created_at, id",
        )?;
        let rows = stmt.query_map([], |r| {
            let cols: i64 = r.get(5)?;
            let rows_: i64 = r.get(6)?;
            let tag: String = r.get(7)?;
            let exit_code: Option<i64> = r.get(8)?;
            let exit_signal: Option<String> = r.get(9)?;
            Ok((
                SessionMeta {
                    id: r.get(0)?,
                    workspace_id: r.get(1)?,
                    title: r.get(2)?,
                    shell: r.get(3)?,
                    cwd: r.get(4)?,
                    cols: cols as u16,
                    rows: rows_ as u16,
                    lifecycle: SessionLifecycle::AtPrompt, // placeholder, set below
                    waiting_for_input: false,
                    is_active: false,
                    created_at: r.get(10)?,
                },
                tag,
                exit_code,
                exit_signal,
            ))
        })?;
        let mut out = Vec::new();
        for row in rows {
            let (mut meta, tag, exit_code, exit_signal) = row?;
            meta.lifecycle = decode_lifecycle(&tag, exit_code, exit_signal)?;
            out.push(meta);
        }
        Ok(out)
    }

    pub fn append_scrollback(
        &self,
        session_id: &SessionId,
        seq: i64,
        bytes: &[u8],
        ts: i64,
    ) -> Result<(), PersistError> {
        self.conn.execute(
            "INSERT INTO scrollback (session_id, seq, bytes, ts) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(session_id, seq) DO UPDATE SET bytes = excluded.bytes, ts = excluded.ts",
            rusqlite::params![session_id, seq, bytes, ts],
        )?;
        Ok(())
    }

    pub fn load_scrollback(&self, session_id: &SessionId) -> Result<Vec<u8>, PersistError> {
        let mut stmt = self
            .conn
            .prepare("SELECT bytes FROM scrollback WHERE session_id = ?1 ORDER BY seq")?;
        let rows = stmt.query_map([session_id], |r| r.get::<_, Vec<u8>>(0))?;
        let mut out = Vec::new();
        for row in rows {
            out.extend_from_slice(&row?);
        }
        Ok(out)
    }

    /// Append one command-history row (schema v2, spec §7 + Pv2 `origin` amendment). Best-effort
    /// from the caller's perspective (persistence-layer errors never panic — spec §11) — the
    /// caller (the periodic flush sweep in `socket_server.rs`) logs and swallows any `Err` rather
    /// than stalling the PTY.
    pub fn append_command_event(
        &self,
        session_id: &str,
        seq: i64,
        ts: i64,
        kind: &str,
        exit_code: Option<u8>,
        origin: &str,
    ) -> Result<(), PersistError> {
        self.conn.execute(
            "INSERT INTO command_events (session_id, seq, ts, kind, exit_code, origin)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)
             ON CONFLICT(session_id, seq) DO UPDATE SET
               ts = excluded.ts, kind = excluded.kind, exit_code = excluded.exit_code,
               origin = excluded.origin",
            rusqlite::params![
                session_id,
                seq,
                ts,
                kind,
                exit_code.map(|c| c as i64),
                origin,
            ],
        )?;
        Ok(())
    }

    /// Read back every command-history row for a session, ordered by `seq` (test-support / future
    /// history queries).
    pub fn list_command_events(
        &self,
        session_id: &str,
    ) -> Result<Vec<CommandEventRow>, PersistError> {
        let mut stmt = self.conn.prepare(
            "SELECT seq, ts, kind, exit_code, origin FROM command_events
             WHERE session_id = ?1 ORDER BY seq",
        )?;
        let rows = stmt.query_map([session_id], |r| {
            let exit_code: Option<i64> = r.get(3)?;
            Ok(CommandEventRow {
                seq: r.get(0)?,
                ts: r.get(1)?,
                kind: r.get(2)?,
                exit_code: exit_code.map(|c| (c & 0xff) as u8),
                origin: r.get(4)?,
            })
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    }
}

/// One row of the `command_events` table (schema v2, spec §7 + Pv2 `origin` amendment), as read
/// back by [`Db::list_command_events`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandEventRow {
    pub seq: i64,
    pub ts: i64,
    pub kind: String,
    pub exit_code: Option<u8>,
    pub origin: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpa_protocol::{SessionLifecycle, SessionMeta, Workspace};

    fn ws(id: &str) -> Workspace {
        Workspace {
            id: id.into(),
            name: format!("ws-{id}"),
            root_path: "/tmp".into(),
        }
    }

    fn meta(id: &str, ws_id: &str, lc: SessionLifecycle) -> SessionMeta {
        SessionMeta {
            id: id.into(),
            workspace_id: ws_id.into(),
            title: format!("t-{id}"),
            shell: "/bin/zsh".into(),
            cwd: "/tmp".into(),
            cols: 80,
            rows: 24,
            lifecycle: lc,
            waiting_for_input: true,
            is_active: true,
            created_at: 1_700_000_000,
        }
    }

    #[test]
    fn persist_and_rehydrate_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bpa.db");
        {
            let db = Db::open(&path).unwrap();
            db.upsert_workspace(&ws("w1")).unwrap();
            db.upsert_session(&meta("s1", "w1", SessionLifecycle::Running))
                .unwrap();
            db.append_scrollback(&"s1".to_string(), 0, b"hello ", 1)
                .unwrap();
            db.append_scrollback(&"s1".to_string(), 1, b"world", 2)
                .unwrap();
        }
        let db = Db::open(&path).unwrap();
        let wss = db.list_workspaces().unwrap();
        assert_eq!(wss.len(), 1);
        assert_eq!(wss[0].id, "w1");

        let sessions = db.rehydrate().unwrap();
        assert_eq!(sessions.len(), 1);
        let s = &sessions[0];
        assert_eq!(s.id, "s1");
        assert_eq!(s.workspace_id, "w1");
        // rehydrated sessions are never active and never waiting
        assert!(!s.is_active);
        assert!(!s.waiting_for_input);
        assert_eq!(s.lifecycle, SessionLifecycle::Running);

        let sb = db.load_scrollback(&"s1".to_string()).unwrap();
        assert_eq!(sb, b"hello world");
    }

    #[test]
    fn every_lifecycle_variant_round_trips() {
        let dir = tempfile::tempdir().unwrap();
        let db = Db::open(&dir.path().join("bpa.db")).unwrap();
        db.upsert_workspace(&ws("w1")).unwrap();

        let variants = vec![
            ("a", SessionLifecycle::AtPrompt),
            ("t", SessionLifecycle::Typing),
            ("r", SessionLifecycle::Running),
            (
                "e0",
                SessionLifecycle::Exited {
                    code: Some(0),
                    signal: None,
                },
            ),
            (
                "e255",
                SessionLifecycle::Exited {
                    code: Some(255),
                    signal: None,
                },
            ),
            (
                "enone",
                SessionLifecycle::Exited {
                    code: None,
                    signal: None,
                },
            ),
            (
                "esig",
                SessionLifecycle::Exited {
                    code: None,
                    signal: Some("SIGKILL".into()),
                },
            ),
        ];
        for (id, lc) in &variants {
            db.upsert_session(&meta(id, "w1", lc.clone())).unwrap();
        }
        let got = db.rehydrate().unwrap();
        for (id, lc) in &variants {
            let m = got.iter().find(|m| &m.id == id).expect("session present");
            assert_eq!(&m.lifecycle, lc, "lifecycle mismatch for {id}");
        }
    }

    #[test]
    fn corrupt_db_is_quarantined_and_recreated() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bpa.db");
        // Write garbage that is NOT a valid SQLite header.
        std::fs::write(&path, b"this is definitely not a sqlite database file").unwrap();

        // open() must NOT error — it quarantines and recreates.
        let db = Db::open(&path).unwrap();
        // Fresh db is usable.
        db.upsert_workspace(&ws("w1")).unwrap();
        assert_eq!(db.list_workspaces().unwrap().len(), 1);

        // A quarantine file exists next to it.
        let found = std::fs::read_dir(dir.path())
            .unwrap()
            .filter_map(|e| e.ok())
            .any(|e| {
                e.file_name()
                    .to_string_lossy()
                    .starts_with("bpa.db.corrupt-")
            });
        assert!(found, "expected a bpa.db.corrupt-<ts> quarantine file");
    }

    #[test]
    fn busy_timeout_allows_concurrent_writers() {
        use std::sync::Arc;
        use std::thread;

        let dir = tempfile::tempdir().unwrap();
        let path = Arc::new(dir.path().join("bpa.db"));
        {
            let db = Db::open(&path).unwrap();
            db.upsert_workspace(&ws("w1")).unwrap();
        }

        // Two threads each open their own connection (WAL + busy_timeout=5000) and
        // hammer inserts. Without busy_timeout these would race to SQLITE_BUSY.
        let mut handles = Vec::new();
        for t in 0..2u8 {
            let p = Arc::clone(&path);
            handles.push(thread::spawn(move || {
                let db = Db::open(&p).unwrap();
                for i in 0..50i64 {
                    let sid = format!("s-{t}-{i}");
                    db.upsert_session(&meta(&sid, "w1", SessionLifecycle::Running))
                        .unwrap();
                }
            }));
        }
        for h in handles {
            h.join().unwrap();
        }
        let db = Db::open(&path).unwrap();
        assert_eq!(db.list_sessions().unwrap().len(), 100);
    }

    #[test]
    fn migration_runs_on_old_user_version() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bpa.db");
        // Simulate a pre-schema database: a valid SQLite file with user_version 0
        // and none of our tables.
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.pragma_update(None, "user_version", 0_i64).unwrap();
        }
        let db = Db::open(&path).unwrap();
        // Migration created our tables and bumped user_version to SCHEMA_VERSION.
        let uv: i64 = db
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(uv, SCHEMA_VERSION);
        db.upsert_workspace(&ws("w1")).unwrap();
        assert_eq!(db.list_workspaces().unwrap().len(), 1);
    }

    #[test]
    fn newer_user_version_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bpa.db");
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.pragma_update(None, "user_version", SCHEMA_VERSION + 1)
                .unwrap();
        }
        let err = Db::open(&path).unwrap_err();
        assert!(
            matches!(err, PersistError::Migration(_)),
            "expected Migration error, got {err:?}"
        );
    }

    #[test]
    fn committed_rows_survive_reopen() {
        // Simulate a hard crash: a connection is dropped WITHOUT a clean shutdown
        // checkpoint after committing rows. WAL guarantees committed rows are durable
        // and re-readable on the next open (spec §11 durability bound).
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bpa.db");
        {
            let db = Db::open(&path).unwrap();
            db.upsert_workspace(&ws("w1")).unwrap();
            db.upsert_session(&meta("s1", "w1", SessionLifecycle::Running))
                .unwrap();
            db.append_scrollback(&"s1".to_string(), 0, b"committed", 1)
                .unwrap();
            // No checkpoint / no clean close: `db` is dropped here abruptly.
            std::mem::drop(db);
        }
        // Reopen (fresh process would do the same): committed rows must be present.
        let db2 = Db::open(&path).unwrap();
        let sessions = db2.rehydrate().unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(
            db2.load_scrollback(&"s1".to_string()).unwrap(),
            b"committed"
        );
    }

    #[test]
    fn open_in_memory_supports_full_crud() {
        let db = Db::open_in_memory().unwrap();
        db.upsert_workspace(&ws("w1")).unwrap();
        db.upsert_session(&meta("s1", "w1", SessionLifecycle::AtPrompt))
            .unwrap();
        db.append_scrollback(&"s1".to_string(), 0, b"abc", 1)
            .unwrap();

        assert_eq!(db.list_workspaces().unwrap().len(), 1);
        assert_eq!(db.list_sessions().unwrap().len(), 1);
        assert_eq!(db.load_scrollback(&"s1".to_string()).unwrap(), b"abc");

        let rehydrated = db.rehydrate().unwrap();
        assert_eq!(rehydrated[0].lifecycle, SessionLifecycle::AtPrompt);
        assert!(!rehydrated[0].is_active);
        assert!(!rehydrated[0].waiting_for_input);
    }

    #[test]
    fn checkpoint_is_a_noop_success_and_data_still_readable() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bpa.db");
        let db = Db::open(&path).unwrap();
        db.upsert_workspace(&ws("w1")).unwrap();
        db.upsert_session(&meta("s1", "w1", SessionLifecycle::Running))
            .unwrap();
        db.append_scrollback(&"s1".to_string(), 0, b"data", 1)
            .unwrap();

        db.checkpoint().unwrap();

        // Checkpointing must not lose or corrupt anything.
        assert_eq!(db.list_workspaces().unwrap().len(), 1);
        assert_eq!(db.load_scrollback(&"s1".to_string()).unwrap(), b"data");

        // And a fresh reopen after checkpoint still rehydrates correctly.
        drop(db);
        let db2 = Db::open(&path).unwrap();
        assert_eq!(db2.rehydrate().unwrap().len(), 1);
    }

    #[test]
    fn checkpoint_on_in_memory_db_does_not_error() {
        // wal_checkpoint on a non-WAL (in-memory) connection must degrade gracefully:
        // it is a no-op success, never a panic.
        let db = Db::open_in_memory().unwrap();
        db.checkpoint().unwrap();
    }

    // ---- Task 11 / spec §7: schema v2 command_events (RED first). ----

    #[test]
    fn v1_db_migrates_to_v2_gains_command_events_keeps_rows() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bpa.db");
        // Build a v1 db by hand: the exact v1 table set + user_version=1, with a session row.
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE workspace (
                   id TEXT PRIMARY KEY, name TEXT NOT NULL, root_path TEXT NOT NULL);
                 CREATE TABLE session (
                   id TEXT PRIMARY KEY,
                   workspace_id TEXT NOT NULL REFERENCES workspace(id),
                   title TEXT NOT NULL, shell TEXT NOT NULL, cwd TEXT NOT NULL,
                   cols INTEGER NOT NULL, rows INTEGER NOT NULL,
                   lifecycle TEXT NOT NULL,
                   exit_code INTEGER, exit_signal TEXT,
                   created_at INTEGER NOT NULL);
                 CREATE TABLE scrollback (
                   session_id TEXT NOT NULL REFERENCES session(id),
                   seq INTEGER NOT NULL, bytes BLOB NOT NULL, ts INTEGER NOT NULL,
                   PRIMARY KEY (session_id, seq));
                 INSERT INTO workspace (id, name, root_path) VALUES ('w1', 'ws-w1', '/tmp');
                 INSERT INTO session
                   (id, workspace_id, title, shell, cwd, cols, rows, lifecycle,
                    exit_code, exit_signal, created_at)
                 VALUES ('s1', 'w1', 't-s1', '/bin/zsh', '/tmp', 80, 24, 'running',
                         NULL, NULL, 1700000000);",
            )
            .unwrap();
            conn.pragma_update(None, "user_version", 1_i64).unwrap();
        }

        // Open with the v2 daemon: migration must run 1 -> 2 in place.
        let db = Db::open(&path).unwrap();
        let uv: i64 = db
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(uv, SCHEMA_VERSION);

        // command_events exists and is usable.
        db.append_command_event("s1", 0, 1_700_000_001, "started", None, "gui")
            .unwrap();
        let events = db.list_command_events("s1").unwrap();
        assert_eq!(events.len(), 1);

        // The pre-existing v1 session row survived the migration untouched.
        let sessions = db.rehydrate().unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "s1");
        assert_eq!(sessions[0].workspace_id, "w1");
        assert_eq!(sessions[0].lifecycle, SessionLifecycle::Running);
    }

    #[test]
    fn v2_reopen_is_noop() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bpa.db");
        {
            let db = Db::open(&path).unwrap();
            db.upsert_workspace(&ws("w1")).unwrap();
            db.upsert_session(&meta("s1", "w1", SessionLifecycle::Running))
                .unwrap();
            db.append_command_event("s1", 0, 1, "started", None, "gui")
                .unwrap();
        }
        // Reopening an already-v2 db must be a no-op migration (from_version == SCHEMA_VERSION)
        // and must not disturb existing rows.
        let db2 = Db::open(&path).unwrap();
        let uv: i64 = db2
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(uv, SCHEMA_VERSION);
        assert_eq!(db2.list_workspaces().unwrap().len(), 1);
        assert_eq!(db2.rehydrate().unwrap().len(), 1);
        assert_eq!(db2.list_command_events("s1").unwrap().len(), 1);
    }

    #[test]
    fn user_version_3_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bpa.db");
        {
            let conn = rusqlite::Connection::open(&path).unwrap();
            conn.pragma_update(None, "user_version", SCHEMA_VERSION + 1)
                .unwrap();
        }
        let err = Db::open(&path).unwrap_err();
        assert!(
            matches!(err, PersistError::Migration(_)),
            "expected Migration error for a future user_version, got {err:?}"
        );
    }

    #[test]
    fn append_and_read_command_event_round_trips() {
        let db = Db::open_in_memory().unwrap();
        db.upsert_workspace(&ws("w1")).unwrap();
        db.upsert_session(&meta("s1", "w1", SessionLifecycle::Running))
            .unwrap();

        db.append_command_event("s1", 0, 1_700_000_000, "started", None, "gui")
            .unwrap();
        db.append_command_event("s1", 1, 1_700_000_005, "finished", Some(7), "gui")
            .unwrap();

        let events = db.list_command_events("s1").unwrap();
        assert_eq!(events.len(), 2);

        assert_eq!(events[0].seq, 0);
        assert_eq!(events[0].ts, 1_700_000_000);
        assert_eq!(events[0].kind, "started");
        assert_eq!(events[0].exit_code, None);
        assert_eq!(events[0].origin, "gui");

        assert_eq!(events[1].seq, 1);
        assert_eq!(events[1].ts, 1_700_000_005);
        assert_eq!(events[1].kind, "finished");
        assert_eq!(events[1].exit_code, Some(7));
        assert_eq!(events[1].origin, "gui");
    }
}
