//! Durable SQLite persistence for the session daemon (spec §11).
//! Best-effort: the in-memory ring is the Layer-1 source of truth; this layer
//! degrades honestly (logs, never panics) on lock/disk/corruption failures.

use std::fmt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use bpa_protocol::{
    CommandEvent, SessionId, SessionLifecycle, SessionMeta, Workspace, WorkspaceId,
};
use rusqlite::Connection;
use tracing::{info, warn};

/// Current schema/migration version stored in `PRAGMA user_version`.
/// v3 (S2 §3.2) adds `workspace_root` — equal ordered multi-root workspaces.
pub const SCHEMA_VERSION: i64 = 3;

#[derive(Debug)]
pub enum PersistError {
    Open(String),
    Sql(String),
    Migration(String),
    Corrupt(String),
    /// A `remove_workspace_root` call would leave the workspace with zero roots
    /// (spec §3.3: `RemoveWorkspaceRoot` rejects removing the LAST root).
    LastRoot,
}

impl fmt::Display for PersistError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PersistError::Open(m) => write!(f, "db open failed: {m}"),
            PersistError::Sql(m) => write!(f, "db sql error: {m}"),
            PersistError::Migration(m) => write!(f, "db migration failed: {m}"),
            PersistError::Corrupt(m) => write!(f, "db corrupt: {m}"),
            PersistError::LastRoot => write!(f, "cannot remove the last workspace root"),
        }
    }
}

impl std::error::Error for PersistError {}

impl PersistError {
    /// Stable wire code for `Response::Error { code, .. }` (spec §3.3), PascalCase
    /// to match the existing convention (`bpa_paths::PathError::code()`, the ad hoc
    /// codes in `socket_server.rs` — e.g. `"InvalidWorkspaceRoot"`, `"NoSuchSession"`).
    pub fn code(&self) -> &'static str {
        match self {
            PersistError::Open(_) => "DbOpen",
            PersistError::Sql(_) => "DbSql",
            PersistError::Migration(_) => "DbMigration",
            PersistError::Corrupt(_) => "DbCorrupt",
            PersistError::LastRoot => "LastRoot",
        }
    }
}

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
    ///
    /// Re-seated (S3 phase 1, spec §3) onto `bpa_daemon_core::migrate::run_migrations`: the
    /// whole-chain-transaction, fail-closed semantics now live there, byte-for-byte unchanged.
    /// This wrapper only supplies the 3-entry step table (each step's `execute_batch` body moved
    /// verbatim from the pre-extraction inline `migrate`) and re-wraps `MigrateError` back into
    /// `PersistError::Migration` so `.code() == "DbMigration"` and every message string existing
    /// consumers see stay identical.
    fn migrate(&self, from_version: i64) -> Result<(), PersistError> {
        const STEPS: &[bpa_daemon_core::migrate::Migration] = &[
            bpa_daemon_core::migrate::Migration {
                upto: 1,
                apply: migrate_v1,
            },
            bpa_daemon_core::migrate::Migration {
                upto: 2,
                apply: migrate_v2,
            },
            bpa_daemon_core::migrate::Migration {
                upto: 3,
                apply: migrate_v3,
            },
        ];
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

/// v0 -> v1: base schema (workspace/session/scrollback). Body moved verbatim from the
/// pre-extraction inline `migrate` (S3 phase 1, spec §3).
fn migrate_v1(tx: &rusqlite::Transaction) -> rusqlite::Result<()> {
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
}

/// v1 -> v2: `command_events`. Body moved verbatim from the pre-extraction inline `migrate`
/// (S3 phase 1, spec §3).
fn migrate_v2(tx: &rusqlite::Transaction) -> rusqlite::Result<()> {
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
}

/// v2 -> v3 (S2 §3.2): equal ordered multi-root workspaces. Every pre-existing workspace's
/// single `root_path` becomes its ord=0 root; `workspace.root_path` stays as a compat mirror
/// (kept in sync by `upsert_workspace`/`remove_workspace_root`). Body moved verbatim from the
/// pre-extraction inline `migrate` (S3 phase 1, spec §3).
fn migrate_v3(tx: &rusqlite::Transaction) -> rusqlite::Result<()> {
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS workspace_root (
           workspace_id TEXT NOT NULL REFERENCES workspace(id),
           ord          INTEGER NOT NULL,
           path         TEXT NOT NULL,
           PRIMARY KEY (workspace_id, ord));
         INSERT INTO workspace_root (workspace_id, ord, path)
           SELECT id, 0, root_path FROM workspace;",
    )
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
    /// Write the `workspace` row (`root_path` mirror := `ws.roots[0]`, per D2/spec §3.1) AND
    /// replace its `workspace_root` rows (delete-then-insert every `(id, ord=i, path=roots[i])`),
    /// all within one transaction — an update never leaves a torn mix of old and new roots
    /// (spec §3.2, §11 fail-closed policy). Rejects a `Workspace` with an empty `roots` (every
    /// workspace must have at least one root — a caller bug, not a transient failure).
    pub fn upsert_workspace(&self, ws: &Workspace) -> Result<(), PersistError> {
        if ws.roots.is_empty() {
            return Err(PersistError::Sql(format!(
                "workspace {} has no roots (roots must be non-empty)",
                ws.id
            )));
        }
        let tx = self.conn.unchecked_transaction()?;
        let root_path = &ws.roots[0];
        tx.execute(
            "INSERT INTO workspace (id, name, root_path) VALUES (?1, ?2, ?3)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, root_path = excluded.root_path",
            rusqlite::params![ws.id, ws.name, root_path],
        )?;
        tx.execute(
            "DELETE FROM workspace_root WHERE workspace_id = ?1",
            rusqlite::params![ws.id],
        )?;
        for (i, root) in ws.roots.iter().enumerate() {
            tx.execute(
                "INSERT INTO workspace_root (workspace_id, ord, path) VALUES (?1, ?2, ?3)",
                rusqlite::params![ws.id, i as i64, root],
            )?;
        }
        tx.commit()?;
        Ok(())
    }

    /// List every workspace with its ordered `roots` assembled from `workspace_root`
    /// (spec §3.2: joins + orders by `ord`). Defensive fallback: a workspace row with no
    /// `workspace_root` rows (shouldn't happen post-migration/`upsert_workspace`) still
    /// yields `roots = [root_path]` rather than an empty vec.
    pub fn list_workspaces(&self) -> Result<Vec<Workspace>, PersistError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, name, root_path FROM workspace ORDER BY id")?;
        let base: Vec<(String, String, String)> = stmt
            .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
            .collect::<Result<_, _>>()?;
        drop(stmt);

        let mut roots_stmt = self
            .conn
            .prepare("SELECT path FROM workspace_root WHERE workspace_id = ?1 ORDER BY ord")?;
        let mut out = Vec::with_capacity(base.len());
        for (id, name, root_path) in base {
            let roots: Vec<String> = roots_stmt
                .query_map(rusqlite::params![id], |r| r.get::<_, String>(0))?
                .collect::<Result<_, _>>()?;
            let roots = if roots.is_empty() {
                vec![root_path.clone()]
            } else {
                roots
            };
            out.push(Workspace {
                id,
                name,
                root_path,
                roots,
            });
        }
        Ok(out)
    }

    /// Append a new root at `ord = max(ord) + 1` (spec §3.3 `AddWorkspaceRoot`; the caller —
    /// `socket_server.rs` — validates the path with `bpa_paths::validate_dir` first). IDEMPOTENT
    /// on a duplicate: if `path` already names one of the workspace's current roots, this is a
    /// no-op — no second row is inserted, `ord` is left untouched, and the unchanged `Workspace`
    /// is returned as `Ok`, never an error. This rules out `roots = ["/a", "/a"]` entirely, which
    /// (pre-fix) was both a duplicate React key for `FileTree` (two rows silently sharing
    /// expanded/cache/selection state) AND an un-removable trap: `remove_workspace_root("/a")`
    /// filters out BOTH copies in one pass, so `remaining` jumps straight to empty and hits
    /// `PersistError::LastRoot` — the duplicate could never be individually removed. Returns the
    /// updated `Workspace` (assembled via the same path as `list_workspaces`). Adding a root
    /// never touches `root_path` (it always mirrors `roots[0]`, unaffected by an append or a
    /// no-op).
    pub fn add_workspace_root(
        &self,
        workspace_id: &WorkspaceId,
        path: &str,
    ) -> Result<Workspace, PersistError> {
        let tx = self.conn.unchecked_transaction()?;
        let already_present: i64 = tx.query_row(
            "SELECT COUNT(*) FROM workspace_root WHERE workspace_id = ?1 AND path = ?2",
            rusqlite::params![workspace_id, path],
            |r| r.get(0),
        )?;
        if already_present == 0 {
            let max_ord: Option<i64> = tx.query_row(
                "SELECT MAX(ord) FROM workspace_root WHERE workspace_id = ?1",
                rusqlite::params![workspace_id],
                |r| r.get(0),
            )?;
            let next_ord = max_ord.map(|o| o + 1).unwrap_or(0);
            tx.execute(
                "INSERT INTO workspace_root (workspace_id, ord, path) VALUES (?1, ?2, ?3)",
                rusqlite::params![workspace_id, next_ord, path],
            )?;
        }
        tx.commit()?;
        self.list_workspaces()?
            .into_iter()
            .find(|w| &w.id == workspace_id)
            .ok_or_else(|| {
                PersistError::Sql(format!(
                    "workspace {workspace_id} not found after add_workspace_root"
                ))
            })
    }

    /// Remove a root (spec §3.3 `RemoveWorkspaceRoot`), rejecting removal of the LAST
    /// remaining root (`PersistError::LastRoot`) so a workspace can never end up with zero
    /// roots. After removal, `ord` is re-normalized to be contiguous from 0 (so a later
    /// `AddWorkspaceRoot` keeps appending at a sane `ord`), and `workspace.root_path` is
    /// re-pointed at the new `roots[0]` if the removed root WAS the old `roots[0]`. If `path`
    /// isn't one of the workspace's current roots, this is an idempotent no-op (returns the
    /// unchanged `Workspace`) rather than an error — matches the honest-degradation policy
    /// (spec §11: never a silent no-op that looks like success-with-a-side-effect, but also
    /// never an error for "there was nothing to do").
    pub fn remove_workspace_root(
        &self,
        workspace_id: &WorkspaceId,
        path: &str,
    ) -> Result<Workspace, PersistError> {
        let tx = self.conn.unchecked_transaction()?;
        let mut stmt =
            tx.prepare("SELECT path FROM workspace_root WHERE workspace_id = ?1 ORDER BY ord")?;
        let existing: Vec<String> = stmt
            .query_map(rusqlite::params![workspace_id], |r| r.get::<_, String>(0))?
            .collect::<Result<_, _>>()?;
        drop(stmt);

        let remaining: Vec<String> = existing
            .iter()
            .filter(|p| p.as_str() != path)
            .cloned()
            .collect();

        if remaining.len() == existing.len() {
            // `path` isn't a current root of this workspace: nothing to remove.
            drop(tx);
            return self
                .list_workspaces()?
                .into_iter()
                .find(|w| &w.id == workspace_id)
                .ok_or_else(|| PersistError::Sql(format!("workspace {workspace_id} not found")));
        }
        if remaining.is_empty() {
            return Err(PersistError::LastRoot);
        }

        tx.execute(
            "DELETE FROM workspace_root WHERE workspace_id = ?1",
            rusqlite::params![workspace_id],
        )?;
        for (i, p) in remaining.iter().enumerate() {
            tx.execute(
                "INSERT INTO workspace_root (workspace_id, ord, path) VALUES (?1, ?2, ?3)",
                rusqlite::params![workspace_id, i as i64, p],
            )?;
        }
        tx.execute(
            "UPDATE workspace SET root_path = ?2 WHERE id = ?1",
            rusqlite::params![workspace_id, remaining[0]],
        )?;
        tx.commit()?;

        self.list_workspaces()?
            .into_iter()
            .find(|w| &w.id == workspace_id)
            .ok_or_else(|| {
                PersistError::Sql(format!(
                    "workspace {workspace_id} not found after remove_workspace_root"
                ))
            })
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

    /// Test-only RAW read of a session's persisted lifecycle tag, bypassing `query_sessions`'s
    /// SES-3 `running -> Exited{None}` read-path reconciliation — `socket_server.rs`'s
    /// flush-freshness test asserts on the WRITE side (the flush must persist the current
    /// `running` tag), which the reconciling read path would otherwise mask.
    #[cfg(test)]
    pub(crate) fn raw_lifecycle_tag(&self, session_id: &str) -> String {
        self.conn
            .query_row(
                "SELECT lifecycle FROM session WHERE id = ?1",
                rusqlite::params![session_id],
                |r| r.get(0),
            )
            .unwrap()
    }

    /// Rehydrate on restart (spec §11): every session is is_active=false,
    /// waiting_for_input=false because its PTY is gone — and a persisted `running`
    /// lifecycle comes back as `Exited { code: None }` for the same reason (SES-3,
    /// see [`Self::query_sessions`]).
    pub fn rehydrate(&self) -> Result<Vec<SessionMeta>, PersistError> {
        self.query_sessions()
    }

    /// Shared query path for `list_sessions`/`rehydrate`. Persisted rows never carry
    /// `is_active`/`waiting_for_input` state (those are runtime-only, in-memory
    /// concepts — S1 never persists `true` for either), so both accessors always
    /// return `false` for them; the two names exist for call-site clarity.
    ///
    /// Lifecycle reconciliation (SES-3, audit 2026-07-24, probe p3): a persisted `running`
    /// row only means "a flush tick fired while a command ran" — the PTY it described is
    /// necessarily gone by the time this row is read back (a daemon restart, or a reaped
    /// session), so handing it out as `Running` is a lie: a dead session claiming to run
    /// (the UI would show a spinner forever). It is mapped to `Exited { code: None }`
    /// ("unknown/aborted", the protocol's own semantics for `code: None`). `AtPrompt` is
    /// NOT remapped: an idle shell restored PTY-less is honestly "at prompt" — that is the
    /// 0.10.0 "restored" semantics. Genuinely LIVE sessions are unaffected: `ListSessions`
    /// overlays the supervisor's in-memory meta over these rows, so this mapping only ever
    /// surfaces for sessions no living PTY backs.
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
            // SES-3 (see the fn doc): a persisted `running` is a dead session's lie once read
            // back — reconcile to `Exited { code: None }` (unknown/aborted).
            if matches!(meta.lifecycle, SessionLifecycle::Running) {
                meta.lifecycle = SessionLifecycle::Exited {
                    code: None,
                    signal: None,
                };
            }
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

    /// Permanently remove a session and everything persisted under it — the `session` row, every
    /// `scrollback` row, and every `command_events` row — in ONE transaction (D3: an honest close
    /// for `KillSession` on an inactive/PTY-less rehydrated session, which has no live process to
    /// kill but must still stop being resurrected on every future restart). All three deletes
    /// commit atomically: a partial delete (e.g. scrollback gone but the session row surviving)
    /// would either resurrect an empty session on the next cold-rehydrate or leave orphaned rows
    /// with no owning session — this makes both impossible. Idempotent: deleting an id that was
    /// never persisted (or already deleted) is a no-op success, not an error.
    pub fn delete_session(&self, session_id: &SessionId) -> Result<(), PersistError> {
        let tx = self.conn.unchecked_transaction()?;
        tx.execute(
            "DELETE FROM command_events WHERE session_id = ?1",
            rusqlite::params![session_id],
        )?;
        tx.execute(
            "DELETE FROM scrollback WHERE session_id = ?1",
            rusqlite::params![session_id],
        )?;
        tx.execute(
            "DELETE FROM session WHERE id = ?1",
            rusqlite::params![session_id],
        )?;
        tx.commit()?;
        Ok(())
    }

    /// The ids of every session persisted under `workspace_id`, ordered `created_at, id` (the same
    /// order [`query_sessions`](Self::query_sessions) uses). Read-only companion to
    /// [`delete_workspace`](Self::delete_workspace): `Request::RemoveWorkspace`'s dispatch arm needs
    /// this list BEFORE it deletes anything, so it can kill each session's live PTY through the
    /// existing `KillSession` machinery first (a removal that deleted the rows but left the child
    /// process running would be exactly the orphan/zombie failure D3 exists to prevent).
    ///
    /// An unknown `workspace_id` is an ERROR, not an empty `Vec` — this is the existence check the
    /// removal path gates on, and it returns the SAME not-found shape
    /// [`remove_workspace_root`](Self::remove_workspace_root) already returns for an unknown id
    /// (`PersistError::Sql("workspace {id} not found")`, wire code `"DbSql"`), deliberately mirrored
    /// rather than given a new variant so clients need no new error handling for the new verb.
    /// Whether a `workspace` row with this id exists — plain boolean, no not-found error
    /// shaping (SES-1/SES-4, audit 2026-07-24: `CreateSession`'s up-front gate owns the typed
    /// `NoSuchWorkspace` error; this is just the lookup).
    pub fn workspace_exists(&self, workspace_id: &WorkspaceId) -> Result<bool, PersistError> {
        let exists: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM workspace WHERE id = ?1",
            rusqlite::params![workspace_id],
            |r| r.get(0),
        )?;
        Ok(exists > 0)
    }

    pub fn workspace_session_ids(
        &self,
        workspace_id: &WorkspaceId,
    ) -> Result<Vec<SessionId>, PersistError> {
        let exists: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM workspace WHERE id = ?1",
            rusqlite::params![workspace_id],
            |r| r.get(0),
        )?;
        if exists == 0 {
            return Err(PersistError::Sql(format!(
                "workspace {workspace_id} not found"
            )));
        }
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM session WHERE workspace_id = ?1 ORDER BY created_at, id")?;
        let ids: Vec<SessionId> = stmt
            .query_map(rusqlite::params![workspace_id], |r| r.get::<_, String>(0))?
            .collect::<Result<_, _>>()?;
        Ok(ids)
    }

    /// Does a workspace row exist for `id`? (SES-4: `CreateSession` must reject a bogus
    /// `workspace_id` UP FRONT instead of creating a live PTY whose every persist fails on the
    /// `session.workspace_id` FK and which then silently vanishes on the next restart with no
    /// client-visible error.) Cheap COUNT; never an error for a missing row (returns `Ok(false)`).
    pub fn workspace_exists(&self, workspace_id: &WorkspaceId) -> Result<bool, PersistError> {
        let exists: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM workspace WHERE id = ?1",
            rusqlite::params![workspace_id],
            |r| r.get(0),
        )?;
        Ok(exists > 0)
    }

    ///
    /// **Explicit ordered deletes, NOT `ON DELETE CASCADE`.** The v1/v3 schema declares plain
    /// `REFERENCES workspace(id)` / `REFERENCES session(id)` with no `ON DELETE` action (see
    /// [`migrate_v1`]/[`migrate_v2`]/[`migrate_v3`]), and both open paths set
    /// `PRAGMA foreign_keys = ON` — so those constraints are ENFORCED and deleting the parent first
    /// would simply fail. Retro-fitting cascades would mean a v4 migration that rebuilds four
    /// tables (SQLite cannot `ALTER` a foreign key), which is a far larger, riskier change than the
    /// five ordered statements below; children first, parents last:
    /// `command_events` → `scrollback` → `session` → `workspace_root` → `workspace`.
    ///
    /// **Transaction boundary:** the existence check, the id capture and all five deletes run
    /// inside the SAME `unchecked_transaction`, committed once at the end. Any failure returns via
    /// `?`, which drops the `Transaction` un-committed and therefore rolls the whole thing back —
    /// a partial removal (e.g. sessions deleted but the workspace row surviving, or a workspace
    /// gone while orphaned `scrollback` rows remain) is impossible. The dependent-row deletes match
    /// on `session_id IN (SELECT id FROM session WHERE workspace_id = ?)` rather than on a
    /// pre-captured id list, so a row written by a concurrent best-effort flush between the capture
    /// and the delete is still caught.
    ///
    /// An unknown `workspace_id` yields the same not-found error as
    /// [`workspace_session_ids`](Self::workspace_session_ids) — never a silent "success" for a
    /// workspace that was never there.
    pub fn delete_workspace(
        &self,
        workspace_id: &WorkspaceId,
    ) -> Result<Vec<SessionId>, PersistError> {
        let tx = self.conn.unchecked_transaction()?;

        let exists: i64 = tx.query_row(
            "SELECT COUNT(*) FROM workspace WHERE id = ?1",
            rusqlite::params![workspace_id],
            |r| r.get(0),
        )?;
        if exists == 0 {
            return Err(PersistError::Sql(format!(
                "workspace {workspace_id} not found"
            )));
        }

        let mut stmt =
            tx.prepare("SELECT id FROM session WHERE workspace_id = ?1 ORDER BY created_at, id")?;
        let deleted: Vec<SessionId> = stmt
            .query_map(rusqlite::params![workspace_id], |r| r.get::<_, String>(0))?
            .collect::<Result<_, _>>()?;
        drop(stmt);

        tx.execute(
            "DELETE FROM command_events
               WHERE session_id IN (SELECT id FROM session WHERE workspace_id = ?1)",
            rusqlite::params![workspace_id],
        )?;
        tx.execute(
            "DELETE FROM scrollback
               WHERE session_id IN (SELECT id FROM session WHERE workspace_id = ?1)",
            rusqlite::params![workspace_id],
        )?;
        tx.execute(
            "DELETE FROM session WHERE workspace_id = ?1",
            rusqlite::params![workspace_id],
        )?;
        tx.execute(
            "DELETE FROM workspace_root WHERE workspace_id = ?1",
            rusqlite::params![workspace_id],
        )?;
        tx.execute(
            "DELETE FROM workspace WHERE id = ?1",
            rusqlite::params![workspace_id],
        )?;
        tx.commit()?;

        info!(
            workspace = %workspace_id,
            sessions = deleted.len(),
            "workspace removed (roots, sessions, scrollback, command_events)"
        );
        Ok(deleted)
    }

    /// Read back the `ts` column of the `scrollback` row at `(session_id, seq=0)` (test-support,
    /// D1: proves whether a flush sweep re-wrote a session's scrollback blob — `append_scrollback`
    /// upserts `ts` on every write, so an unchanged `ts` across two sweeps means the row was
    /// genuinely skipped, not just written with byte-identical content). `None` if no such row
    /// exists yet.
    #[doc(hidden)]
    pub fn scrollback_row_ts_for_test(
        &self,
        session_id: &SessionId,
    ) -> Result<Option<i64>, PersistError> {
        let mut stmt = self
            .conn
            .prepare("SELECT ts FROM scrollback WHERE session_id = ?1 AND seq = 0")?;
        let mut rows = stmt.query_map([session_id], |r| r.get::<_, i64>(0))?;
        match rows.next() {
            Some(row) => Ok(Some(row?)),
            None => Ok(None),
        }
    }

    /// Test-support (crate-internal, compiled out of every real build): install a `BEFORE DELETE`
    /// trigger on `workspace` that `RAISE(ABORT)`s, so [`delete_workspace`](Self::delete_workspace)
    /// fails on its LAST statement — after the four child deletes have already run. That is the
    /// only deterministic way to prove the whole removal is one transaction: the abort makes
    /// `delete_workspace` return via `?`, dropping the un-committed `Transaction`, which must roll
    /// the child deletes back. Paired with [`clear_delete_failure_for_test`](Self::clear_delete_failure_for_test).
    #[cfg(test)]
    pub(crate) fn inject_delete_failure_for_test(&self) -> Result<(), PersistError> {
        self.conn.execute_batch(
            "CREATE TRIGGER bpa_test_block_workspace_delete BEFORE DELETE ON workspace
             BEGIN SELECT RAISE(ABORT, 'test-injected failure'); END;",
        )?;
        Ok(())
    }

    /// Remove the trigger installed by
    /// [`inject_delete_failure_for_test`](Self::inject_delete_failure_for_test), so a retry can
    /// prove the rolled-back database is still fully workable rather than wedged.
    #[cfg(test)]
    pub(crate) fn clear_delete_failure_for_test(&self) -> Result<(), PersistError> {
        self.conn
            .execute_batch("DROP TRIGGER bpa_test_block_workspace_delete;")?;
        Ok(())
    }

    /// Read back the most recent command-history rows for a session, newest-first
    /// (`ORDER BY seq DESC LIMIT ?`), capped at `limit` — the first consumer of
    /// `command_events` (spec §3.3 `Request::GetCommandEvents`, closing the "no UI" note
    /// from Pv2 §7). Returns the wire `bpa_protocol::CommandEvent` (the internal
    /// `session_id` is injected into every row since the table's own `session_id` column
    /// isn't otherwise carried by `CommandEventRow`'s narrower predecessor). An unknown
    /// `session_id` yields an empty `Vec`, never an error — spec §7 "honest, not an error"
    /// (rehydrated sessions may predate v2 rows).
    ///
    /// Note: prior to S2 this method returned ALL rows oldest-first as `Vec<CommandEventRow>`
    /// (test-support only, no production caller). Reconciled here to the spec §3.2/§3.3
    /// signature directly (newest-first, `limit`, `CommandEvent`) rather than adding a
    /// parallel method, since nothing outside this module's own tests ever called the old one.
    pub fn list_command_events(
        &self,
        session_id: &SessionId,
        limit: u32,
    ) -> Result<Vec<CommandEvent>, PersistError> {
        let mut stmt = self.conn.prepare(
            "SELECT seq, ts, kind, exit_code, origin FROM command_events
             WHERE session_id = ?1 ORDER BY seq DESC LIMIT ?2",
        )?;
        let rows = stmt.query_map(rusqlite::params![session_id, limit as i64], |r| {
            let exit_code: Option<i64> = r.get(3)?;
            Ok(CommandEvent {
                session_id: session_id.clone(),
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

#[cfg(test)]
mod tests {
    use super::*;
    use bpa_protocol::{SessionLifecycle, SessionMeta, Workspace};

    fn ws(id: &str) -> Workspace {
        Workspace {
            id: id.into(),
            name: format!("ws-{id}"),
            root_path: "/tmp".into(),
            roots: vec!["/tmp".into()],
        }
    }

    /// Build a `Workspace` with an explicit ordered multi-root list (`root_path`
    /// mirrors `roots[0]`, matching what `upsert_workspace` itself would compute).
    fn ws_multi(id: &str, roots: &[&str]) -> Workspace {
        Workspace {
            id: id.into(),
            name: format!("ws-{id}"),
            root_path: roots[0].into(),
            roots: roots.iter().map(|s| s.to_string()).collect(),
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
        // SES-3: a persisted `running` is a dead PTY's lie once read back — it rehydrates as
        // `Exited { code: None }` (unknown/aborted), never as `Running`.
        assert_eq!(
            s.lifecycle,
            SessionLifecycle::Exited {
                code: None,
                signal: None
            }
        );

        let sb = db.load_scrollback(&"s1".to_string()).unwrap();
        assert_eq!(sb, b"hello world");
    }

    #[test]
    fn every_lifecycle_variant_round_trips_except_running_maps_to_exited() {
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
            // SES-3: `Running` is the ONE variant that does NOT round-trip verbatim — a
            // persisted `running` describes a PTY that is gone by read time, so it comes back
            // as `Exited { code: None }` (see `query_sessions`). Everything else round-trips.
            let expected = if matches!(lc, SessionLifecycle::Running) {
                SessionLifecycle::Exited {
                    code: None,
                    signal: None,
                }
            } else {
                lc.clone()
            };
            assert_eq!(m.lifecycle, expected, "lifecycle mismatch for {id}");
        }
    }

    /// Focused SES-3 regression (audit 2026-07-24, probe p3): a daemon killed -9 mid-command
    /// leaves a `running` lifecycle row; after restart the restored session must report
    /// `Exited { code: None }`, not a spinner-forever `Running` — while a live `atPrompt`
    /// keeps the honest "restored idle shell" semantics (NOT remapped).
    #[test]
    fn persisted_running_rehydrates_as_exited_unknown_but_at_prompt_is_kept() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bpa.db");
        {
            let db = Db::open(&path).unwrap();
            db.upsert_workspace(&ws("w1")).unwrap();
            db.upsert_session(&meta("mid-cmd", "w1", SessionLifecycle::Running))
                .unwrap();
            db.upsert_session(&meta("idle", "w1", SessionLifecycle::AtPrompt))
                .unwrap();
        }
        // Reopen = the restart boundary the probe exercises.
        let db = Db::open(&path).unwrap();
        for accessor in [Db::list_sessions, Db::rehydrate] {
            let sessions = accessor(&db).unwrap();
            let mid_cmd = sessions.iter().find(|m| m.id == "mid-cmd").unwrap();
            assert_eq!(
                mid_cmd.lifecycle,
                SessionLifecycle::Exited {
                    code: None,
                    signal: None
                },
                "a persisted running must never come back claiming to run"
            );
            let idle = sessions.iter().find(|m| m.id == "idle").unwrap();
            assert_eq!(
                idle.lifecycle,
                SessionLifecycle::AtPrompt,
                "atPrompt keeps the 0.10.0 restored semantics"
            );
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

        // Open with the current daemon: migration must run all the way 1 -> SCHEMA_VERSION
        // (currently 3) in place, in one shot.
        let db = Db::open(&path).unwrap();
        let uv: i64 = db
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(uv, SCHEMA_VERSION);

        // command_events exists and is usable.
        db.append_command_event("s1", 0, 1_700_000_001, "started", None, "gui")
            .unwrap();
        let events = db.list_command_events(&"s1".to_string(), 10).unwrap();
        assert_eq!(events.len(), 1);

        // The pre-existing v1 session row survived the migration — its `running` lifecycle
        // reads back as `Exited { code: None }` (SES-3: a persisted `running` is a dead PTY's
        // lie once read back; the migration must not disturb the ROW, the read-path
        // reconciliation is what maps it).
        let sessions = db.rehydrate().unwrap();
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0].id, "s1");
        assert_eq!(sessions[0].workspace_id, "w1");
        assert_eq!(
            sessions[0].lifecycle,
            SessionLifecycle::Exited {
                code: None,
                signal: None
            }
        );

        // S2 §3.2: the v1 workspace also picked up an ord=0 workspace_root backfill row on
        // its way through v2 -> v3.
        let workspaces = db.list_workspaces().unwrap();
        assert_eq!(workspaces.len(), 1);
        assert_eq!(workspaces[0].id, "w1");
        assert_eq!(workspaces[0].root_path, "/tmp");
        assert_eq!(workspaces[0].roots, vec!["/tmp".to_string()]);
    }

    #[test]
    fn v3_reopen_is_noop() {
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
        // Reopening an already-current-version db must be a no-op migration
        // (from_version == SCHEMA_VERSION) and must not disturb existing rows.
        let db2 = Db::open(&path).unwrap();
        let uv: i64 = db2
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(uv, SCHEMA_VERSION);
        assert_eq!(db2.list_workspaces().unwrap().len(), 1);
        assert_eq!(db2.rehydrate().unwrap().len(), 1);
        assert_eq!(
            db2.list_command_events(&"s1".to_string(), 10)
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn user_version_ahead_of_schema_fails_closed() {
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
        // S2 §3.3: list_command_events is newest-first (ORDER BY seq DESC), unlike the old
        // (test-only) oldest-first CommandEventRow reader it replaced.
        let db = Db::open_in_memory().unwrap();
        db.upsert_workspace(&ws("w1")).unwrap();
        db.upsert_session(&meta("s1", "w1", SessionLifecycle::Running))
            .unwrap();

        db.append_command_event("s1", 0, 1_700_000_000, "started", None, "gui")
            .unwrap();
        db.append_command_event("s1", 1, 1_700_000_005, "finished", Some(7), "gui")
            .unwrap();

        let events = db.list_command_events(&"s1".to_string(), 10).unwrap();
        assert_eq!(events.len(), 2);

        // newest (seq=1) first.
        assert_eq!(events[0].session_id, "s1");
        assert_eq!(events[0].seq, 1);
        assert_eq!(events[0].ts, 1_700_000_005);
        assert_eq!(events[0].kind, "finished");
        assert_eq!(events[0].exit_code, Some(7));
        assert_eq!(events[0].origin, "gui");

        assert_eq!(events[1].session_id, "s1");
        assert_eq!(events[1].seq, 0);
        assert_eq!(events[1].ts, 1_700_000_000);
        assert_eq!(events[1].kind, "started");
        assert_eq!(events[1].exit_code, None);
        assert_eq!(events[1].origin, "gui");
    }

    #[test]
    fn list_command_events_respects_limit_and_unknown_session_is_empty() {
        let db = Db::open_in_memory().unwrap();
        db.upsert_workspace(&ws("w1")).unwrap();
        db.upsert_session(&meta("s1", "w1", SessionLifecycle::Running))
            .unwrap();
        for i in 0..5i64 {
            db.append_command_event("s1", i, 1_700_000_000 + i, "started", None, "gui")
                .unwrap();
        }

        let capped = db.list_command_events(&"s1".to_string(), 3).unwrap();
        assert_eq!(
            capped.iter().map(|e| e.seq).collect::<Vec<_>>(),
            vec![4, 3, 2],
            "limit=3 keeps only the 3 newest, newest-first"
        );

        let all = db.list_command_events(&"s1".to_string(), 100).unwrap();
        assert_eq!(
            all.len(),
            5,
            "a limit above the row count returns every row"
        );

        let empty = db
            .list_command_events(&"no-such-session".to_string(), 10)
            .unwrap();
        assert!(
            empty.is_empty(),
            "an unknown session_id is an empty list, not an error (spec §7)"
        );
    }

    // ---- D3: delete_session removes the session row + scrollback + command_events rows in one
    // transaction, leaves an unrelated session untouched, and is idempotent on a second call. ----
    #[test]
    fn delete_session_removes_session_scrollback_and_command_events_atomically() {
        let db = Db::open_in_memory().unwrap();
        db.upsert_workspace(&ws("w1")).unwrap();
        db.upsert_session(&meta("s1", "w1", SessionLifecycle::Running))
            .unwrap();
        db.upsert_session(&meta("s2", "w1", SessionLifecycle::Running))
            .unwrap();
        db.append_scrollback(&"s1".to_string(), 0, b"gone soon", 1)
            .unwrap();
        db.append_scrollback(&"s2".to_string(), 0, b"kept", 1)
            .unwrap();
        db.append_command_event("s1", 0, 1_700_000_000, "started", None, "gui")
            .unwrap();
        db.append_command_event("s2", 0, 1_700_000_000, "started", None, "gui")
            .unwrap();

        db.delete_session(&"s1".to_string()).unwrap();

        // s1 is fully gone: no session row, no scrollback, no command_events.
        assert!(
            db.list_sessions().unwrap().iter().all(|m| m.id != "s1"),
            "s1's session row must be gone"
        );
        assert_eq!(
            db.load_scrollback(&"s1".to_string()).unwrap(),
            Vec::<u8>::new(),
            "s1's scrollback rows must be gone"
        );
        assert!(
            db.list_command_events(&"s1".to_string(), 10)
                .unwrap()
                .is_empty(),
            "s1's command_events rows must be gone"
        );

        // s2 (a different session) is completely untouched.
        let s2 = db
            .list_sessions()
            .unwrap()
            .into_iter()
            .find(|m| m.id == "s2")
            .expect("s2 must survive s1's deletion");
        assert_eq!(s2.id, "s2");
        assert_eq!(db.load_scrollback(&"s2".to_string()).unwrap(), b"kept");
        assert_eq!(
            db.list_command_events(&"s2".to_string(), 10).unwrap().len(),
            1
        );

        // A restart-equivalent (rehydrate) must not resurrect s1.
        let rehydrated = db.rehydrate().unwrap();
        assert!(
            rehydrated.iter().all(|m| m.id != "s1"),
            "a deleted session must never be resurrected by rehydrate"
        );

        // Idempotent: deleting an already-deleted (or never-existing) id is a no-op success.
        db.delete_session(&"s1".to_string())
            .expect("deleting an already-gone session must be a no-op success, not an error");
        db.delete_session(&"never-existed".to_string())
            .expect("deleting a never-persisted session must be a no-op success");
    }

    // ---- Task 3 / spec §3.2: schema v3 workspace_root — multi-root persistence (RED first). ----

    #[test]
    fn fresh_db_is_v3_with_workspace_root_table() {
        let db = Db::open_in_memory().unwrap();
        assert_eq!(SCHEMA_VERSION, 3);
        let uv: i64 = db
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(uv, 3);

        let exists: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'table' AND name = 'workspace_root'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(exists, 1, "fresh db must have a workspace_root table");
    }

    #[test]
    fn v2_db_migrates_to_v3_backfills_ord0_for_every_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bpa.db");
        // Build a v2 db by hand: the exact v2 table set (workspace/session/scrollback/
        // command_events, NO workspace_root yet) + user_version=2, with TWO pre-existing
        // workspaces — mirrors `v1_db_migrates_to_v2_gains_command_events_keeps_rows` above,
        // one schema version further along.
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
                 CREATE TABLE command_events (
                   session_id TEXT NOT NULL REFERENCES session(id),
                   seq        INTEGER NOT NULL,
                   ts         INTEGER NOT NULL,
                   kind       TEXT NOT NULL,
                   exit_code  INTEGER,
                   origin     TEXT NOT NULL DEFAULT 'gui',
                   PRIMARY KEY (session_id, seq));
                 INSERT INTO workspace (id, name, root_path) VALUES ('w1', 'ws-w1', '/tmp/one');
                 INSERT INTO workspace (id, name, root_path) VALUES ('w2', 'ws-w2', '/tmp/two');",
            )
            .unwrap();
            conn.pragma_update(None, "user_version", 2_i64).unwrap();
        }

        // Open with the v3 daemon: migration must run 2 -> 3 in place.
        let db = Db::open(&path).unwrap();
        let uv: i64 = db
            .conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(uv, SCHEMA_VERSION);

        let mut list = db.list_workspaces().unwrap();
        list.sort_by(|a, b| a.id.cmp(&b.id));
        assert_eq!(
            list.len(),
            2,
            "both pre-existing workspaces survived the migration"
        );

        assert_eq!(list[0].id, "w1");
        assert_eq!(list[0].root_path, "/tmp/one");
        assert_eq!(
            list[0].roots,
            vec!["/tmp/one".to_string()],
            "w1 got its ord=0 backfill row"
        );

        assert_eq!(list[1].id, "w2");
        assert_eq!(list[1].root_path, "/tmp/two");
        assert_eq!(
            list[1].roots,
            vec!["/tmp/two".to_string()],
            "w2 got its ord=0 backfill row"
        );
    }

    #[test]
    fn migration_v2_to_v3_fails_closed_on_error_and_leaves_v2_intact() {
        // Fail-closed / rollback proof (spec §11): pre-create an INCOMPATIBLE workspace_root
        // table (missing the `path` column the migration's INSERT ... SELECT targets), so the
        // migration's own INSERT fails mid-transaction. Since the whole migration runs inside
        // one `unchecked_transaction`, that failure must roll back EVERYTHING — including the
        // (no-op, since the table already existed) `CREATE TABLE IF NOT EXISTS` — and must
        // leave `user_version` unchanged at 2, not partially bumped.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bpa.db");
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
                 CREATE TABLE command_events (
                   session_id TEXT NOT NULL REFERENCES session(id),
                   seq        INTEGER NOT NULL,
                   ts         INTEGER NOT NULL,
                   kind       TEXT NOT NULL,
                   exit_code  INTEGER,
                   origin     TEXT NOT NULL DEFAULT 'gui',
                   PRIMARY KEY (session_id, seq));
                 CREATE TABLE workspace_root (
                   workspace_id TEXT NOT NULL, ord INTEGER NOT NULL, bogus_column TEXT);
                 INSERT INTO workspace (id, name, root_path) VALUES ('w1', 'ws-w1', '/tmp');",
            )
            .unwrap();
            conn.pragma_update(None, "user_version", 2_i64).unwrap();
        }

        let err = Db::open(&path).unwrap_err();
        assert!(
            matches!(err, PersistError::Migration(_)),
            "expected a Migration error from the incompatible workspace_root table, got {err:?}"
        );

        // Re-open the raw file directly: the failed migration's transaction must have rolled
        // back cleanly, so user_version is still 2 (never partially bumped to 3).
        let conn = rusqlite::Connection::open(&path).unwrap();
        let uv: i64 = conn
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(
            uv, 2,
            "a failed migration must roll back and leave user_version untouched (fail-closed)"
        );
        // And the pre-existing workspace row is untouched too (not partially migrated).
        let root_path: String = conn
            .query_row("SELECT root_path FROM workspace WHERE id = 'w1'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(root_path, "/tmp");
    }

    #[test]
    fn upsert_workspace_multi_root_then_list_preserves_order_and_root_path_mirror() {
        let db = Db::open_in_memory().unwrap();
        db.upsert_workspace(&ws_multi("w1", &["/tmp/a", "/tmp/b"]))
            .unwrap();

        let list = db.list_workspaces().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(
            list[0].roots,
            vec!["/tmp/a".to_string(), "/tmp/b".to_string()]
        );
        assert_eq!(list[0].root_path, "/tmp/a", "root_path mirrors roots[0]");
    }

    #[test]
    fn upsert_workspace_replaces_roots_on_update_rather_than_accumulating() {
        let db = Db::open_in_memory().unwrap();
        db.upsert_workspace(&ws_multi("w1", &["/tmp/a", "/tmp/b"]))
            .unwrap();
        db.upsert_workspace(&ws_multi("w1", &["/tmp/x"])).unwrap();

        let list = db.list_workspaces().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(
            list[0].roots,
            vec!["/tmp/x".to_string()],
            "the old roots (a, b) must be gone, not accumulated alongside the new one"
        );
        assert_eq!(list[0].root_path, "/tmp/x");
    }

    #[test]
    fn upsert_workspace_rejects_empty_roots() {
        let db = Db::open_in_memory().unwrap();
        let w = Workspace {
            id: "w1".into(),
            name: "ws-w1".into(),
            root_path: "/tmp".into(),
            roots: vec![],
        };
        let err = db.upsert_workspace(&w).unwrap_err();
        assert!(
            matches!(err, PersistError::Sql(_)),
            "empty roots must be rejected, got {err:?}"
        );
        assert!(
            db.list_workspaces().unwrap().is_empty(),
            "a rejected upsert must not leave a partial workspace row"
        );
    }

    #[test]
    fn add_and_remove_workspace_root_ordering_and_renormalization() {
        let db = Db::open_in_memory().unwrap();
        db.upsert_workspace(&ws_multi("w1", &["/tmp/a"])).unwrap();

        let w2 = db.add_workspace_root(&"w1".to_string(), "/tmp/b").unwrap();
        assert_eq!(
            w2.roots,
            vec!["/tmp/a".to_string(), "/tmp/b".to_string()],
            "appended at ord=1"
        );
        assert_eq!(w2.root_path, "/tmp/a", "root_path unaffected by an append");

        let w3 = db.add_workspace_root(&"w1".to_string(), "/tmp/c").unwrap();
        assert_eq!(
            w3.roots,
            vec![
                "/tmp/a".to_string(),
                "/tmp/b".to_string(),
                "/tmp/c".to_string()
            ],
            "appended at ord=2"
        );

        // Remove the MIDDLE root: ord must re-normalize to be contiguous from 0.
        let w4 = db
            .remove_workspace_root(&"w1".to_string(), "/tmp/b")
            .unwrap();
        assert_eq!(
            w4.roots,
            vec!["/tmp/a".to_string(), "/tmp/c".to_string()],
            "b removed, a/c re-normalized to ord 0/1"
        );
        assert_eq!(w4.root_path, "/tmp/a");

        // A subsequent add appends after the re-normalized ord (not colliding with the old ord=2).
        let w5 = db.add_workspace_root(&"w1".to_string(), "/tmp/d").unwrap();
        assert_eq!(
            w5.roots,
            vec![
                "/tmp/a".to_string(),
                "/tmp/c".to_string(),
                "/tmp/d".to_string()
            ]
        );

        // Remove roots[0] itself: root_path mirror must re-point at the new roots[0].
        let w6 = db
            .remove_workspace_root(&"w1".to_string(), "/tmp/a")
            .unwrap();
        assert_eq!(w6.roots, vec!["/tmp/c".to_string(), "/tmp/d".to_string()]);
        assert_eq!(
            w6.root_path, "/tmp/c",
            "root_path re-pointed at the new roots[0]"
        );
    }

    #[test]
    fn remove_last_workspace_root_is_rejected() {
        let db = Db::open_in_memory().unwrap();
        db.upsert_workspace(&ws_multi("w1", &["/tmp/only"]))
            .unwrap();

        let err = db
            .remove_workspace_root(&"w1".to_string(), "/tmp/only")
            .unwrap_err();
        assert!(matches!(err, PersistError::LastRoot));
        assert_eq!(err.code(), "LastRoot");

        // The rejected removal must not have mutated anything.
        let list = db.list_workspaces().unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].roots, vec!["/tmp/only".to_string()]);
        assert_eq!(list[0].root_path, "/tmp/only");
    }

    #[test]
    fn remove_workspace_root_nonexistent_path_is_an_idempotent_noop() {
        let db = Db::open_in_memory().unwrap();
        db.upsert_workspace(&ws_multi("w1", &["/tmp/a", "/tmp/b"]))
            .unwrap();

        let unchanged = db
            .remove_workspace_root(&"w1".to_string(), "/tmp/does-not-exist")
            .unwrap();
        assert_eq!(
            unchanged.roots,
            vec!["/tmp/a".to_string(), "/tmp/b".to_string()],
            "a path that isn't a current root is a no-op, not an error"
        );
    }

    #[test]
    fn add_workspace_root_on_unknown_workspace_errors() {
        let db = Db::open_in_memory().unwrap();
        let err = db
            .add_workspace_root(&"no-such-workspace".to_string(), "/tmp/x")
            .unwrap_err();
        assert!(
            matches!(err, PersistError::Sql(_)),
            "adding a root to a nonexistent workspace must fail (FK constraint), got {err:?}"
        );
    }

    // ---- S2 final review, fix wave B / B1: AddWorkspaceRoot must be idempotent on a
    // duplicate path — the pre-fix behavior appended it UNCONDITIONALLY, producing
    // roots = ["/a", "/a"]: a duplicate FileTree React key AND an un-removable trap
    // (remove_workspace_root filters out BOTH copies at once, so `remaining` goes
    // straight to empty and hits `PersistError::LastRoot` — the duplicate can never be
    // individually removed). ----

    #[test]
    fn add_workspace_root_duplicate_path_is_idempotent_no_duplicate_row() {
        let db = Db::open_in_memory().unwrap();
        db.upsert_workspace(&ws_multi("w1", &["/tmp/a"])).unwrap();

        // Re-adding the SAME path must be a no-op: no duplicate row, roots unchanged.
        let again = db.add_workspace_root(&"w1".to_string(), "/tmp/a").unwrap();
        assert_eq!(
            again.roots,
            vec!["/tmp/a".to_string()],
            "adding an already-present root must not append a duplicate"
        );

        let count: i64 = db
            .conn
            .query_row(
                "SELECT COUNT(*) FROM workspace_root WHERE workspace_id = 'w1'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            count, 1,
            "exactly one workspace_root row must exist for /tmp/a, never two"
        );

        // A genuinely new root still appends normally (idempotency doesn't break the
        // happy path).
        let with_b = db.add_workspace_root(&"w1".to_string(), "/tmp/b").unwrap();
        assert_eq!(
            with_b.roots,
            vec!["/tmp/a".to_string(), "/tmp/b".to_string()],
            "a new path still appends"
        );

        // Re-adding /tmp/a again (now that there are two roots) is still a no-op: it
        // must not touch /tmp/b's ord, and must not reach roots = ["/tmp/a","/tmp/a","/tmp/b"]
        // (the un-removable-trap state this fix rules out entirely).
        let again2 = db.add_workspace_root(&"w1".to_string(), "/tmp/a").unwrap();
        assert_eq!(
            again2.roots,
            vec!["/tmp/a".to_string(), "/tmp/b".to_string()],
            "duplicate add against a multi-root workspace stays idempotent"
        );

        // Regression: the resulting workspace must remain fully removable — a state
        // reachable only via this idempotent add can never become an un-removable trap.
        let after_remove = db
            .remove_workspace_root(&"w1".to_string(), "/tmp/a")
            .unwrap();
        assert_eq!(
            after_remove.roots,
            vec!["/tmp/b".to_string()],
            "removing /tmp/a leaves exactly /tmp/b, proving no phantom duplicate survived"
        );
    }

    // ---- `Request::RemoveWorkspace` persistence half (`delete_workspace`): a workspace whose
    // roots have been deleted off disk was previously UNDELETABLE, so this must actually delete —
    // totally, and without leaving orphans behind. ----

    /// Raw row count for one table filtered by one string column — the tests below assert against
    /// the physical tables (not just the public accessors) so an orphaned `workspace_root` /
    /// `scrollback` / `command_events` row can't hide behind a JOIN that filters it out.
    fn count_where(db: &Db, table: &str, column: &str, value: &str) -> i64 {
        db.conn
            .query_row(
                &format!("SELECT COUNT(*) FROM {table} WHERE {column} = ?1"),
                rusqlite::params![value],
                |r| r.get(0),
            )
            .unwrap()
    }

    #[test]
    fn delete_workspace_removes_roots_sessions_and_every_dependent_row() {
        let db = Db::open_in_memory().unwrap();
        db.upsert_workspace(&ws_multi("doomed", &["/tmp/a", "/tmp/b", "/tmp/c"]))
            .unwrap();
        db.upsert_workspace(&ws_multi("keeper", &["/tmp/k"]))
            .unwrap();

        for sid in ["d1", "d2"] {
            db.upsert_session(&meta(sid, "doomed", SessionLifecycle::Running))
                .unwrap();
            db.append_scrollback(&sid.to_string(), 0, b"doomed bytes", 1)
                .unwrap();
            db.append_command_event(sid, 0, 1_700_000_000, "started", None, "gui")
                .unwrap();
            db.append_command_event(sid, 1, 1_700_000_001, "finished", Some(0), "gui")
                .unwrap();
        }
        db.upsert_session(&meta("k1", "keeper", SessionLifecycle::Running))
            .unwrap();
        db.append_scrollback(&"k1".to_string(), 0, b"kept bytes", 1)
            .unwrap();
        db.append_command_event("k1", 0, 1_700_000_000, "started", None, "gui")
            .unwrap();

        let deleted = db.delete_workspace(&"doomed".to_string()).unwrap();
        assert_eq!(
            deleted,
            vec!["d1".to_string(), "d2".to_string()],
            "delete_workspace must report exactly the sessions it removed"
        );

        // The workspace itself and its roots are gone.
        assert!(
            db.list_workspaces()
                .unwrap()
                .iter()
                .all(|w| w.id != "doomed"),
            "the workspace row must be gone"
        );
        assert_eq!(
            count_where(&db, "workspace", "id", "doomed"),
            0,
            "no workspace row may survive"
        );
        assert_eq!(
            count_where(&db, "workspace_root", "workspace_id", "doomed"),
            0,
            "all three workspace_root rows must be gone — no orphaned roots"
        );

        // Its sessions and every dependent row are gone.
        for sid in ["d1", "d2"] {
            assert!(
                db.list_sessions().unwrap().iter().all(|m| m.id != sid),
                "{sid}'s session row must be gone"
            );
            assert_eq!(
                db.load_scrollback(&sid.to_string()).unwrap(),
                Vec::<u8>::new(),
                "{sid}'s scrollback rows must be gone — no orphans"
            );
            assert!(
                db.list_command_events(&sid.to_string(), 10)
                    .unwrap()
                    .is_empty(),
                "{sid}'s command_events rows must be gone — no orphans"
            );
            assert_eq!(count_where(&db, "scrollback", "session_id", sid), 0);
            assert_eq!(count_where(&db, "command_events", "session_id", sid), 0);
        }

        // A restart-equivalent (rehydrate) must not resurrect any of it.
        assert!(
            db.rehydrate()
                .unwrap()
                .iter()
                .all(|m| m.workspace_id != "doomed"),
            "a removed workspace's sessions must never be resurrected by rehydrate"
        );

        // The sibling workspace is completely untouched.
        let keeper = db
            .list_workspaces()
            .unwrap()
            .into_iter()
            .find(|w| w.id == "keeper")
            .expect("the other workspace must survive");
        assert_eq!(keeper.roots, vec!["/tmp/k".to_string()]);
        assert_eq!(
            db.load_scrollback(&"k1".to_string()).unwrap(),
            b"kept bytes"
        );
        assert_eq!(
            db.list_command_events(&"k1".to_string(), 10).unwrap().len(),
            1
        );
    }

    #[test]
    fn delete_workspace_with_no_sessions_still_removes_the_workspace_and_its_roots() {
        let db = Db::open_in_memory().unwrap();
        db.upsert_workspace(&ws_multi("empty", &["/tmp/a", "/tmp/b"]))
            .unwrap();

        let deleted = db.delete_workspace(&"empty".to_string()).unwrap();
        assert!(
            deleted.is_empty(),
            "a session-less workspace reports no deleted sessions"
        );
        assert!(db.list_workspaces().unwrap().is_empty());
        assert_eq!(
            count_where(&db, "workspace_root", "workspace_id", "empty"),
            0
        );
    }

    /// Unknown id ⇒ the SAME not-found shape `remove_workspace_root` already returns, asserted by
    /// direct comparison of both the wire `code()` and the rendered message. This is the contract
    /// `Request::RemoveWorkspace` inherits, so a client needs no new error handling for the verb.
    #[test]
    fn delete_workspace_unknown_id_is_the_same_not_found_error_as_remove_workspace_root() {
        let db = Db::open_in_memory().unwrap();
        let unknown = "no-such-workspace".to_string();

        let del_err = db.delete_workspace(&unknown).unwrap_err();
        let ids_err = db.workspace_session_ids(&unknown).unwrap_err();
        let root_err = db.remove_workspace_root(&unknown, "/tmp/a").unwrap_err();

        assert!(
            matches!(del_err, PersistError::Sql(_)),
            "unknown workspace must be an honest error, not a silent success: {del_err:?}"
        );
        assert_eq!(del_err.code(), "DbSql");
        assert_eq!(del_err.code(), root_err.code());
        assert_eq!(del_err.to_string(), root_err.to_string());
        assert_eq!(ids_err.code(), root_err.code());
        assert_eq!(ids_err.to_string(), root_err.to_string());
        assert!(
            del_err.to_string().contains("not found"),
            "message must say what happened, got {del_err}"
        );
    }

    /// The whole removal is ONE transaction: a failure on the LAST statement
    /// (`DELETE FROM workspace`) must roll back the four deletes that already ran, leaving the
    /// workspace exactly as it was — never half-removed. The failure is injected deterministically
    /// with a `BEFORE DELETE` trigger that `RAISE(ABORT)`s, which aborts that statement and makes
    /// `delete_workspace` return via `?`, dropping the un-committed `Transaction` (rusqlite's
    /// default drop behaviour is rollback).
    #[test]
    fn delete_workspace_is_atomic_a_failure_leaves_no_partially_removed_state() {
        let db = Db::open_in_memory().unwrap();
        db.upsert_workspace(&ws_multi("doomed", &["/tmp/a", "/tmp/b"]))
            .unwrap();
        db.upsert_session(&meta("d1", "doomed", SessionLifecycle::Running))
            .unwrap();
        db.append_scrollback(&"d1".to_string(), 0, b"still here", 1)
            .unwrap();
        db.append_command_event("d1", 0, 1_700_000_000, "started", None, "gui")
            .unwrap();

        db.inject_delete_failure_for_test().unwrap();

        let e = db
            .delete_workspace(&"doomed".to_string())
            .expect_err("the injected failure must surface as an error, never a silent success");
        assert!(matches!(e, PersistError::Sql(_)), "got {e:?}");

        // NOTHING was removed: every row the earlier statements had already deleted is back.
        assert_eq!(count_where(&db, "workspace", "id", "doomed"), 1);
        assert_eq!(
            count_where(&db, "workspace_root", "workspace_id", "doomed"),
            2,
            "workspace_root deletes must have rolled back"
        );
        assert_eq!(
            count_where(&db, "session", "workspace_id", "doomed"),
            1,
            "session deletes must have rolled back"
        );
        assert_eq!(
            count_where(&db, "scrollback", "session_id", "d1"),
            1,
            "scrollback deletes must have rolled back"
        );
        assert_eq!(
            count_where(&db, "command_events", "session_id", "d1"),
            1,
            "command_events deletes must have rolled back"
        );
        assert_eq!(
            db.load_scrollback(&"d1".to_string()).unwrap(),
            b"still here"
        );

        // With the injected failure removed, the same call now removes everything — proving the
        // rollback left the DB in a fully workable (not wedged) state.
        db.clear_delete_failure_for_test().unwrap();
        assert_eq!(
            db.delete_workspace(&"doomed".to_string()).unwrap(),
            vec!["d1".to_string()]
        );
        assert_eq!(count_where(&db, "workspace", "id", "doomed"), 0);
        assert_eq!(
            count_where(&db, "workspace_root", "workspace_id", "doomed"),
            0
        );
        assert_eq!(count_where(&db, "session", "workspace_id", "doomed"), 0);
        assert_eq!(count_where(&db, "scrollback", "session_id", "d1"), 0);
        assert_eq!(count_where(&db, "command_events", "session_id", "d1"), 0);
    }

    #[test]
    fn workspace_session_ids_lists_only_that_workspaces_sessions() {
        let db = Db::open_in_memory().unwrap();
        db.upsert_workspace(&ws("w1")).unwrap();
        db.upsert_workspace(&ws("w2")).unwrap();
        db.upsert_session(&meta("a", "w1", SessionLifecycle::Running))
            .unwrap();
        db.upsert_session(&meta("b", "w1", SessionLifecycle::AtPrompt))
            .unwrap();
        db.upsert_session(&meta("c", "w2", SessionLifecycle::Running))
            .unwrap();

        assert_eq!(
            db.workspace_session_ids(&"w1".to_string()).unwrap(),
            vec!["a".to_string(), "b".to_string()]
        );
        assert_eq!(
            db.workspace_session_ids(&"w2".to_string()).unwrap(),
            vec!["c".to_string()]
        );

        // A workspace that exists but has no sessions is an empty Vec, NOT an error — only an
        // unknown workspace id is an error.
        db.upsert_workspace(&ws("w3")).unwrap();
        assert!(db
            .workspace_session_ids(&"w3".to_string())
            .unwrap()
            .is_empty());
    }

    #[test]
    fn workspace_exists_distinguishes_known_from_unknown() {
        // SES-4: CreateSession gates on this — a bogus workspace_id must read false (so the dispatch
        // returns NoSuchWorkspace up front) instead of letting a PTY spawn that can never persist.
        let db = Db::open_in_memory().unwrap();
        db.upsert_workspace(&ws("w1")).unwrap();
        assert!(db.workspace_exists(&"w1".to_string()).unwrap());
        assert!(!db.workspace_exists(&"nope".to_string()).unwrap());
    }

    /// Guard on the "explicit ordered deletes, not ON DELETE CASCADE" decision: if a future
    /// migration ever adds real cascades, this documents that today's schema has none AND that
    /// foreign keys are enforced (which is exactly why the delete order in `delete_workspace`
    /// matters). Deleting the parent first must fail.
    #[test]
    fn schema_has_no_cascade_and_enforces_foreign_keys() {
        let db = Db::open_in_memory().unwrap();
        let fk_on: i64 = db
            .conn
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .unwrap();
        assert_eq!(fk_on, 1, "foreign keys must be ENFORCED");

        let ddl: String = db
            .conn
            .query_row(
                "SELECT group_concat(sql, ';') FROM sqlite_master
                 WHERE type = 'table' AND name IN ('session','scrollback','command_events','workspace_root')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(
            !ddl.to_uppercase().contains("ON DELETE"),
            "no ON DELETE action is declared today, so delete_workspace must delete children \
             explicitly, in order; schema was: {ddl}"
        );

        db.upsert_workspace(&ws("w1")).unwrap();
        db.upsert_session(&meta("s1", "w1", SessionLifecycle::Running))
            .unwrap();
        let e = db
            .conn
            .execute("DELETE FROM workspace WHERE id = 'w1'", [])
            .expect_err("without a cascade, deleting a referenced workspace must violate the FK");
        assert!(
            e.to_string().to_uppercase().contains("FOREIGN KEY"),
            "expected a foreign-key violation, got {e}"
        );
    }
}
