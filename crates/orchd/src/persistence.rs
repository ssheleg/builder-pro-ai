//! Durable SQLite persistence for `bpa-orchd` (spec §5, §5.1). Mirrors
//! `bpa_sessiond::persistence`'s open/degrade/quarantine shape, re-seated onto
//! `bpa_daemon_core::migrate::run_migrations` (S3 phase 1 extraction) from day one — there is no
//! pre-extraction inline `migrate` to preserve here, unlike sessiond's history.
//!
//! Schema v1 (spec §5.1, LOCKED DDL) is applied as a single `Migration { upto: 1 }` step; every
//! later domain migration (T10+) appends further steps to the same table, never mutates this one.
//! Schema v2 (S4 spec §4, LOCKED DDL) appends the knowledge-graph `graph_node`/`graph_edge`
//! tables as `Migration { upto: 2 }`, additive-forward-only per D1 — the graph persistence itself
//! (invariants, enum⇄TEXT mapping) lives in the sibling `crate::graph` module.
//! Schema v3 (S-EXT spec §4, LOCKED DDL) appends ALL nine MCP/connectors/skills/trust tables
//! (`mcp_server`, `mcp_tool`, `account`, `mcp_invocation`, `mcp_artifact`, `skill`,
//! `consent_grant`, `policy`, `audit_log`) as ONE `Migration { upto: 3 }` step, purely additive
//! (no backfill — new subsystem) — the `mcp_server`/`mcp_tool` persistence this task (S-EXT T2)
//! actually implements lives in the sibling `crate::mcp::registry` module; the other seven
//! tables' CRUD lands in later S-EXT tasks.
//! Schema v4 (S-IDEA spec §4, LOCKED DDL, task T2) appends ONE additive table, `research_run` —
//! a thin idea↔invocation↔artifact provenance link (D2: the actual ResearchArtifact IS the
//! pre-existing `mcp_artifact` row a run's tool call produces, no blob duplication) as ONE
//! `Migration { upto: 4 }` step. Its CRUD + the D11 boot-reconcile query live in the sibling
//! `crate::research` module, mirroring how `crate::mcp::registry`/`crate::graph` build their own
//! `impl Db` blocks on top of this file's `conn()`/`now_ms()`/`OrchdPersistError` seam. Task T4
//! additionally folds graph-ingest-on-accept (D9) into this file's own `set_insight_status` — see
//! that method's doc comment.
//! Schema v5 (SCN-051/ST-037, task priority) appends ONE additive column,
//! `task.priority TEXT NOT NULL DEFAULT 'normal'` (urgent|normal), as ONE
//! `Migration { upto: 5 }` step — existing tasks backfill to `'normal'` via the column DEFAULT.
//! Its CRUD lives in this file's own task block (`create_task` gains the priority arg,
//! `set_task_priority` is the new focused mutator mirroring `set_task_status`).

use std::fmt;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use bpa_orchd_proto::{
    AuditRow, DomainTask, FitVerdict, Goal, GoalKind, GoalStatus, GraphEntityType, Idea,
    IdeaLifecycle, Insight, InsightStatus, McpArtifact, McpInvocation, Policy, PolicyRules,
    PolicyScope, Project, ProjectStatus, RuleScope, RuleSet, TaskPriority, TaskSource, TaskStatus,
};
use rusqlite::{Connection, OptionalExtension};
use tracing::{info, warn};
use uuid::Uuid;

/// Current schema/migration version stored in `PRAGMA user_version` (spec §5.1; S4 spec §4 D1
/// bumps this 1→2 for the additive knowledge-graph tables; S-EXT spec §4 bumps this 2→3 for the
/// additive MCP/connectors/skills/trust tables; S-IDEA spec §4 D7 bumps this 3→4 for the
/// additive `research_run` table; SCN-051/ST-037 bumps this 4→5 for the additive
/// `task.priority` column).
pub const SCHEMA_VERSION: i64 = 5;

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

/// The outcome of opening the on-disk database (spec D3, BL-94): a clean open vs. a recovery after
/// a corrupt image was quarantined aside and a fresh database recreated. `boot::open_db_degrading`
/// maps this (plus the in-memory-fallback case it handles itself) onto the storage-degradation
/// mode the frontend surfaces as an honest banner.
#[derive(Debug, Clone, PartialEq)]
pub enum DbOpenOutcome {
    Clean,
    RecoveredFromCorruption { quarantined_to: PathBuf },
}

impl Db {
    /// Open (or create) the database at `path`. Sets WAL + busy_timeout + foreign_keys, runs
    /// migrations in a transaction. On a corrupt image, quarantines the file
    /// (`orchd.db.corrupt-<ts>`) and recreates a fresh database rather than crashing (mirrors
    /// `bpa_sessiond::persistence::Db::open`). Discards the recovery outcome — use
    /// [`Db::open_with_outcome`] when the caller needs to know whether a recovery happened.
    pub fn open(path: &Path) -> Result<Db, PersistError> {
        Self::open_with_outcome(path).map(|(db, _outcome)| db)
    }

    /// Like [`Db::open`] but reports whether a corrupt on-disk image had to be quarantined and a
    /// fresh database recreated (spec D3, BL-94). `boot::open_db_degrading` uses the outcome to set
    /// the storage-degradation mode the frontend surfaces.
    pub fn open_with_outcome(path: &Path) -> Result<(Db, DbOpenOutcome), PersistError> {
        match Self::open_inner(path) {
            Ok(db) => Ok((db, DbOpenOutcome::Clean)),
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
                let db = Self::open_inner(path)?;
                Ok((
                    db,
                    DbOpenOutcome::RecoveredFromCorruption {
                        quarantined_to: dst,
                    },
                ))
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

    /// Best-effort WAL checkpoint on graceful shutdown: fold as much of the write-ahead log into
    /// the main database file as possible so the next boot's WAL replay stays small (spec §5).
    ///
    /// Uses **PASSIVE** mode, NOT `TRUNCATE`/`RESTART`. PASSIVE checkpoints every frame it can
    /// without ever waiting on a reader/writer lock and returns immediately; TRUNCATE additionally
    /// waits for all readers to finish so it can zero the WAL file, and that wait can BLOCK — under
    /// a slow CI runner (a lingering WAL holder / filesystem timing) the drain's `TRUNCATE` hung
    /// past the e2e shutdown timeout, deterministically reddening `phase2`. A checkpoint documented
    /// as "best-effort" must never block the graceful-shutdown ack, so PASSIVE is the correct mode:
    /// correctness is unaffected — SQLite replays any un-checkpointed WAL tail on the next open
    /// (which the relaunch does anyway). Any failure is a typed error, never a panic.
    pub fn checkpoint(&self) -> Result<(), PersistError> {
        self.conn
            .pragma_update(None, "wal_checkpoint", "PASSIVE")
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
            bpa_daemon_core::migrate::Migration {
                upto: 4,
                apply: migrate_v4,
            },
            bpa_daemon_core::migrate::Migration {
                upto: 5,
                apply: migrate_v5,
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

/// v0 -> v1: `orchd.db` schema v1 (spec §5.1, LOCKED DDL — transcribed verbatim, including the
/// spec's own inline comments, so this body and the spec text can be diffed directly).
/// `pub(crate)`: `crate::graph`'s test module builds a REAL v1 fixture directly on top of this
/// step (apply v1 alone, insert legacy rows, THEN apply [`migrate_v2`]) to prove the backfill.
pub(crate) fn migrate_v1(tx: &rusqlite::Transaction) -> rusqlite::Result<()> {
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

/// v1 -> v2: knowledge-graph tables (S4 spec §4, LOCKED DDL — transcribed verbatim, including the
/// spec's own inline comments) PLUS an idempotent backfill: for every existing project's
/// strategic goal that has no `entity_ref` node yet, seed one via [`crate::graph`]'s
/// `seed_strategic_entity_ref` (the exact same insert `create_project` uses for NEW projects,
/// D6) — so a pre-S4 `orchd.db` gets a non-empty graph on upgrade too. `pub(crate)`: reused
/// directly (not just via `run_migrations`) by `crate::graph`'s own migration-backfill tests.
pub(crate) fn migrate_v2(tx: &rusqlite::Transaction) -> rusqlite::Result<()> {
    tx.execute_batch(
        "CREATE TABLE graph_node (
           id TEXT PRIMARY KEY,
           project_id TEXT NOT NULL REFERENCES project(id) ON DELETE CASCADE,
           kind TEXT NOT NULL CHECK (kind IN ('concept','fact','artifact','decision','note','entity_ref')),
           entity_type TEXT CHECK (entity_type IN ('goal','idea','insight','task')),
           entity_id TEXT,
           label TEXT NOT NULL, body TEXT NOT NULL DEFAULT '',
           pos_x REAL NOT NULL DEFAULT 0, pos_y REAL NOT NULL DEFAULT 0,
           created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL,
           CHECK ((kind = 'entity_ref') = (entity_type IS NOT NULL AND entity_id IS NOT NULL))
         );
         CREATE INDEX graph_node_by_project ON graph_node(project_id);
         CREATE UNIQUE INDEX graph_node_one_per_entity
           ON graph_node(entity_type, entity_id) WHERE kind = 'entity_ref';
         CREATE TABLE graph_edge (
           id TEXT PRIMARY KEY,
           source_node_id TEXT NOT NULL REFERENCES graph_node(id) ON DELETE CASCADE,
           target_node_id TEXT NOT NULL REFERENCES graph_node(id) ON DELETE CASCADE,
           kind TEXT NOT NULL CHECK (kind IN ('relates','depends','derives','supports','contradicts','parent')),
           label TEXT NOT NULL DEFAULT '', created_at INTEGER NOT NULL,
           CHECK (source_node_id <> target_node_id)
         );
         CREATE INDEX graph_edge_by_source ON graph_edge(source_node_id);
         CREATE INDEX graph_edge_by_target ON graph_edge(target_node_id);
         CREATE UNIQUE INDEX graph_edge_uniq ON graph_edge(source_node_id, target_node_id, kind);
         -- migration also runs, ONCE, an idempotent backfill: for every existing project's strategic goal
         -- that has no entityRef node, INSERT one (so pre-S4 projects get a seeded graph on upgrade).
         -- user_version → 2",
    )?;

    let mut stmt = tx.prepare(
        "SELECT g.id, g.project_id, g.title
         FROM goal g
         WHERE g.kind = 'strategic'
           AND NOT EXISTS (
             SELECT 1 FROM graph_node n
             WHERE n.kind = 'entity_ref' AND n.entity_type = 'goal' AND n.entity_id = g.id
           )",
    )?;
    let pending: Vec<(String, String, String)> = stmt
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)))?
        .collect::<rusqlite::Result<_>>()?;
    drop(stmt);
    for (goal_id, project_id, title) in pending {
        crate::graph::seed_strategic_entity_ref(tx, &project_id, &goal_id, &title)?;
    }
    Ok(())
}

/// v2 -> v3: MCP server/tool registry + connectors/skills/trust tables (S-EXT spec §4, LOCKED
/// DDL — transcribed verbatim, including the spec's own inline comments, so this body and the
/// spec text can be diffed directly). Purely additive, no backfill (spec §4 "Idempotent-migration
/// note": "a v2→v3 upgrade of an existing `orchd.db` with live projects creates the tables and
/// seeds nothing — new subsystem"). This task (S-EXT T2) implements CRUD for `mcp_server`/
/// `mcp_tool` only (`crate::mcp::registry`); the other seven tables (`account`, `mcp_invocation`,
/// `mcp_artifact`, `skill`, `consent_grant`, `policy`, `audit_log`) land in the schema HERE, in
/// this same additive step, so every later S-EXT task builds on it — their CRUD lands in later
/// tasks.
pub(crate) fn migrate_v3(tx: &rusqlite::Transaction) -> rusqlite::Result<()> {
    tx.execute_batch(
        r#"-- MCP servers registry
         CREATE TABLE mcp_server (
           id             TEXT PRIMARY KEY,             -- uuid v4
           name           TEXT NOT NULL,
           transport      TEXT NOT NULL,                -- 'http' | 'stdio'
           url            TEXT,                          -- http: endpoint (…/mcp); null for stdio
           command        TEXT,                          -- stdio: executable; null for http
           args_json      TEXT NOT NULL DEFAULT '[]',    -- stdio: JSON array of args
           env_json       TEXT NOT NULL DEFAULT '{}',    -- stdio: JSON object (allowlisted at spawn)
           scope          TEXT NOT NULL,                 -- 'global' | 'project'
           project_id     TEXT,                          -- non-null iff scope='project'; FK -> project(id) ON DELETE CASCADE
           auth_kind      TEXT NOT NULL DEFAULT 'none',  -- 'none' | 'bearer' | 'oauth'
           secret_ref     TEXT,                          -- Keychain account key for bearer; null otherwise
           account_id     TEXT,                          -- FK -> account(id) for oauth; null otherwise
           enabled        INTEGER NOT NULL DEFAULT 1,
           timeout_ms     INTEGER NOT NULL DEFAULT 30000,
           max_retries    INTEGER NOT NULL DEFAULT 2,
           protocol_version TEXT,                         -- last negotiated; null until first connect
           created_at     INTEGER NOT NULL,
           updated_at     INTEGER NOT NULL,
           FOREIGN KEY(project_id) REFERENCES project(id) ON DELETE CASCADE,
           CHECK ( (scope='project') = (project_id IS NOT NULL) ),
           CHECK ( transport IN ('http','stdio') ),
           CHECK ( (transport='http') = (url IS NOT NULL) )
         );
         CREATE INDEX mcp_server_by_project ON mcp_server(project_id);

         -- Cached tool descriptors (refreshed on connect + tools/list_changed)
         CREATE TABLE mcp_tool (
           id             TEXT PRIMARY KEY,             -- uuid v4
           server_id      TEXT NOT NULL,
           name           TEXT NOT NULL,
           title          TEXT,
           description    TEXT,
           input_schema_json TEXT NOT NULL DEFAULT '{}',
           enabled        INTEGER NOT NULL DEFAULT 1,   -- per-tool allowlist (S0/S1 §16: "enabled tools are an explicit per-server allowlist"); default on-fetch
           fetched_at     INTEGER NOT NULL,
           FOREIGN KEY(server_id) REFERENCES mcp_server(id) ON DELETE CASCADE,
           UNIQUE(server_id, name)
         );
         CREATE INDEX mcp_tool_by_server ON mcp_tool(server_id);

         -- External OAuth accounts (connectors); token bytes in Keychain, only refs here
         CREATE TABLE account (
           id             TEXT PRIMARY KEY,             -- uuid v4
           provider       TEXT NOT NULL,                -- e.g. 'prowl','x','linkedin','generic-oauth'
           label          TEXT NOT NULL,                -- owner-facing name
           auth_kind      TEXT NOT NULL,                -- 'oauth' | 'apikey'
           secret_ref     TEXT NOT NULL,                -- Keychain account key (token/apikey lives there)
           scopes_json    TEXT NOT NULL DEFAULT '[]',
           expires_at     INTEGER,                       -- access-token expiry epoch ms; null if none
           refresh_ref    TEXT,                          -- Keychain key for refresh token; null if none
           created_at     INTEGER NOT NULL,
           updated_at     INTEGER NOT NULL
         );

         -- Per-call invocation records (cost/latency from call #1). Exactly ONE source is set:
         -- server_id (an MCP tools/call) XOR account_id (a direct-API connector_invoke) — the
         -- CHECK below enforces it. ConnectorInvoke reuses this invocation/artifact path
         -- identically to McpCallTool (S-EXT §6/§7, D9; T12 review). server_id is nullable (was
         -- NOT NULL in the unreleased v3 first cut) so a connector row — which has no mcp_server
         -- to reference — can live here without a synthetic server row.
         CREATE TABLE mcp_invocation (
           id             TEXT PRIMARY KEY,             -- uuid v4
           server_id      TEXT,                          -- MCP tools/call source; null for a connector_invoke
           account_id     TEXT,                          -- connector_invoke source; null for an MCP tools/call
           tool_name      TEXT NOT NULL,                 -- MCP tool name OR connector op name
           project_id     TEXT,                          -- context if called within a project
           request_hash   TEXT NOT NULL,                 -- sha256 of args (NOT the args themselves)
           ok             INTEGER NOT NULL,
           error_kind     TEXT,                          -- null on ok
           latency_ms     INTEGER NOT NULL,
           cost_usd       REAL,                          -- null unless server reports usage
           input_tokens   INTEGER,
           output_tokens  INTEGER,
           started_at     INTEGER NOT NULL,
           FOREIGN KEY(server_id) REFERENCES mcp_server(id) ON DELETE CASCADE,
           FOREIGN KEY(account_id) REFERENCES account(id) ON DELETE CASCADE,
           CHECK ( (server_id IS NOT NULL) <> (account_id IS NOT NULL) )
         );
         CREATE INDEX mcp_invocation_by_server ON mcp_invocation(server_id, started_at);

         -- Durable artifacts (tool results); untrusted by construction. server_id/account_id XOR
         -- mirrors mcp_invocation (an MCP tools/call vs a connector_invoke); is_untrusted=1 for
         -- BOTH sources (S-EXT D9/§6: every artifact from McpCallTool AND ConnectorInvoke is
         -- untrusted). Survives orchd restart for the connector path too (T12 review DoD).
         CREATE TABLE mcp_artifact (
           id             TEXT PRIMARY KEY,             -- uuid v4
           invocation_id  TEXT NOT NULL,
           server_id      TEXT,                          -- MCP source; null for a connector_invoke
           account_id     TEXT,                          -- connector source; null for an MCP tools/call
           tool_name      TEXT NOT NULL,
           project_id     TEXT,
           content_json   TEXT NOT NULL,                 -- full structured result
           content_text   TEXT,                          -- flattened text for preview/search
           is_untrusted   INTEGER NOT NULL DEFAULT 1,    -- always 1 for external output (S6b mediation flag)
           created_at     INTEGER NOT NULL,
           FOREIGN KEY(invocation_id) REFERENCES mcp_invocation(id) ON DELETE CASCADE,
           FOREIGN KEY(server_id) REFERENCES mcp_server(id) ON DELETE CASCADE,
           FOREIGN KEY(account_id) REFERENCES account(id) ON DELETE CASCADE,
           CHECK ( (server_id IS NOT NULL) <> (account_id IS NOT NULL) )
         );
         CREATE INDEX mcp_artifact_by_project ON mcp_artifact(project_id, created_at);

         -- Skills registry (SKILL.md format; files-as-truth)
         CREATE TABLE skill (
           id             TEXT PRIMARY KEY,             -- uuid v4
           name           TEXT NOT NULL,
           description    TEXT NOT NULL,
           md_path        TEXT NOT NULL,                 -- absolute path to SKILL.md (validated within an allowed root)
           md_hash        TEXT NOT NULL,                 -- sha256 of file at register time
           scope          TEXT NOT NULL,                 -- 'global' | 'project'
           project_id     TEXT,
           created_at     INTEGER NOT NULL,
           updated_at     INTEGER NOT NULL,
           FOREIGN KEY(project_id) REFERENCES project(id) ON DELETE CASCADE,
           CHECK ( (scope='project') = (project_id IS NOT NULL) )
         );

         -- Trust: persisted consent grants + policy caps + append-only audit
         CREATE TABLE consent_grant (
           id             TEXT PRIMARY KEY,
           kind           TEXT NOT NULL,                 -- 'connect' | 'stdio_exec'
           server_id      TEXT NOT NULL,
           fingerprint    TEXT NOT NULL,                 -- url (http) or command+hash (stdio) at grant time
           granted_at     INTEGER NOT NULL,
           FOREIGN KEY(server_id) REFERENCES mcp_server(id) ON DELETE CASCADE,
           UNIQUE(server_id, kind)
         );
         -- (task T18, S-EXT §6/BL-22 — this table's CRUD lands in T18; the two CHECKs below
         -- correct-in-place the unreleased v3 DDL the same way T12 review corrected
         -- mcp_invocation/mcp_artifact's server_id/account_id nullability, rather than a new
         -- schema-version step: no real orchd.db has ever populated this still-CRUD-less table)
         CREATE TABLE policy (
           id             TEXT PRIMARY KEY,
           scope          TEXT NOT NULL,                 -- 'global' | 'project' | 'server'
           ref_id         TEXT,                          -- project_id or server_id per scope; null for global
           spend_cap_usd  REAL,                          -- null = unlimited
           rate_per_min   INTEGER,                       -- null = unlimited
           created_at     INTEGER NOT NULL,
           updated_at     INTEGER NOT NULL,
           CHECK ( scope IN ('global','project','server') ),
           CHECK ( (scope='global') = (ref_id IS NULL) )
         );
         CREATE TABLE audit_log (
           id             TEXT PRIMARY KEY,
           at             INTEGER NOT NULL,
           action         TEXT NOT NULL,                 -- 'connect'|'disconnect'|'stdio_spawn'|'tool_call'|'connector_invoke'|'consent_grant'|'policy_deny'
           server_id      TEXT,
           tool_name      TEXT,
           project_id     TEXT,
           decision       TEXT NOT NULL,                 -- 'allow'|'deny'
           reason         TEXT,                          -- e.g. 'spend_cap_exceeded'; NEVER secret/arg content
           invocation_id  TEXT
         );
         CREATE INDEX audit_log_by_at ON audit_log(at);
         -- additive only; a v2->v3 upgrade of an existing orchd.db with live projects creates
         -- these tables and seeds nothing (no backfill needed — new subsystem).
         -- user_version → 3"#,
    )
}

/// v3 -> v4: `orchd.db` schema v4 (S-IDEA spec §4, LOCKED DDL — transcribed verbatim, including
/// the spec's own inline comments, so this body and the spec text can be diffed directly). Adds
/// ONE additive table, `research_run` — a thin idea↔invocation↔artifact provenance link a
/// research run leaves behind (D2: the actual ResearchArtifact IS the pre-existing `mcp_artifact`
/// row the run's `tools/call` produces, S-EXT schema v3 — no blob duplication, one source of
/// truth). Its CRUD + the D11 boot-reconcile query live in the sibling `crate::research` module.
pub(crate) fn migrate_v4(tx: &rusqlite::Transaction) -> rusqlite::Result<()> {
    tx.execute_batch(
        r#"-- Research runs: idea + MCP server/tool + args -> pending|running|done|failed
         CREATE TABLE research_run (
           id             TEXT PRIMARY KEY,             -- uuid v4
           idea_id        TEXT NOT NULL,                -- FK -> idea(id) ON DELETE CASCADE
           server_id      TEXT NOT NULL,                -- FK -> mcp_server(id) ON DELETE CASCADE (the research MCP server)
           tool_name      TEXT NOT NULL,               -- the owner-chosen tool on that server
           args_json      TEXT NOT NULL DEFAULT '{}',   -- the invocation args (owner-supplied; NOT a secret)
           status         TEXT NOT NULL DEFAULT 'pending', -- 'pending'|'running'|'done'|'failed'
           invocation_id  TEXT,                          -- FK -> mcp_invocation(id); set on 'done'. Best-effort on 'failed':
                                                         --   NULL for policy_cap_exceeded/consent (no invocation row written),
                                                         --   and NULL for the Mcp(_) failure family too — call_tool records an
                                                         --   mcp_invocation for those but does NOT return its id (shipped error
                                                         --   type carries no id; see §6). Accepted partial-provenance: a failed
                                                         --   run's record is its error_kind, not an invocation link.
           artifact_id    TEXT,                          -- FK -> mcp_artifact(id); set on 'done' ONLY
           error_kind     TEXT,                          -- set on 'failed' (e.g. 'policy_cap_exceeded','tool_error','transport','timeout','interrupted'); NEVER a secret/arg/tool-output
           created_at     INTEGER NOT NULL,
           updated_at     INTEGER NOT NULL,
           FOREIGN KEY(idea_id)   REFERENCES idea(id)       ON DELETE CASCADE,
           FOREIGN KEY(server_id) REFERENCES mcp_server(id) ON DELETE CASCADE,
           CHECK ( status IN ('pending','running','done','failed') ),
           CHECK ( (status='done') = (artifact_id IS NOT NULL) )   -- a done run has an artifact; others don't
         );
         CREATE INDEX research_run_by_idea ON research_run(idea_id, created_at);
         -- Idempotent-migration note: additive table only; a v3->v4 upgrade of a live orchd.db
         -- creates this table and seeds nothing.
         -- user_version → 4"#,
    )
}

/// v4 -> v5: SCN-051 task priority (ST-037). ONE additive column on `task`:
/// `priority TEXT NOT NULL DEFAULT 'normal'` with the urgent/normal CHECK, matching the style of
/// v1's other `task` TEXT-enum columns (`status`/`source`). Purely additive, forward-only per D1:
/// every pre-v5 task backfills to `'normal'` via the column DEFAULT itself (SQLite materializes
/// the default for existing rows on `ADD COLUMN`), so no explicit backfill `UPDATE` is needed —
/// see `v4_fixture_migrates_to_v5_and_backfills_existing_tasks_to_normal` below for the proof
/// against a REAL v4 fixture.
pub(crate) fn migrate_v5(tx: &rusqlite::Transaction) -> rusqlite::Result<()> {
    tx.execute_batch(
        "ALTER TABLE task ADD COLUMN priority TEXT NOT NULL DEFAULT 'normal'
           CHECK (priority IN ('urgent','normal'));
         -- user_version → 5",
    )
}

// ================================================================================
// ---- domain persistence (spec §5.2): project + project_workspace + goal CRUD ----
// ================================================================================

/// Title of the strategic goal auto-created with every project (spec §5.2; the owner edits it
/// afterwards, it is never auto-changed again and never deletable). `pub(crate)`: `crate::graph`'s
/// tests reuse this literal rather than duplicating it (drift-proof).
pub(crate) const STRATEGIC_GOAL_TITLE: &str = "Strategic goal";

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
/// against. `pub(crate)`: `crate::graph` reuses this for its own dup-`(source,target,kind)` edge
/// and dup-`(entity_type,entity_id)` entityRef conflict mapping.
pub(crate) fn is_constraint_violation(e: &rusqlite::Error) -> bool {
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
/// `pub(crate)`: `crate::graph` shares this clock for `graph_node`/`graph_edge` timestamps.
pub(crate) fn now_ms() -> i64 {
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

/// Inverse of [`decode_project_status`] — needed by T9's `insert_project_raw` (spec §8), which
/// writes an ALREADY-TYPED `Project.status` (parsed from an import bundle) back to its `TEXT`
/// column; every other project-status write in this file goes through the `Create*`/mutating
/// verbs above, which hard-code the literal `'active'`/`'archived'` SQL themselves instead of
/// needing a reusable encoder.
fn encode_project_status(s: &ProjectStatus) -> &'static str {
    match s {
        ProjectStatus::Active => "active",
        ProjectStatus::Archived => "archived",
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

// ---- idea / insight / task enum <-> TEXT helpers (spec §5.1 CHECK literals, snake_case — the
// persistence layer OWNS this mapping; it is deliberately distinct from `bpa_orchd_proto`'s wire
// serde reprs, which are camelCase, e.g. `IdeaLifecycle::InDev` is `"inDev"` on the wire but
// `"in_dev"` in the DB CHECK constraint). ----

fn encode_idea_lifecycle(l: &IdeaLifecycle) -> &'static str {
    match l {
        IdeaLifecycle::Captured => "captured",
        IdeaLifecycle::Researching => "researching",
        IdeaLifecycle::Specced => "specced",
        IdeaLifecycle::InDev => "in_dev",
        IdeaLifecycle::Shipped => "shipped",
        IdeaLifecycle::Archived => "archived",
    }
}

fn decode_idea_lifecycle(s: &str) -> Result<IdeaLifecycle, OrchdPersistError> {
    match s {
        "captured" => Ok(IdeaLifecycle::Captured),
        "researching" => Ok(IdeaLifecycle::Researching),
        "specced" => Ok(IdeaLifecycle::Specced),
        "in_dev" => Ok(IdeaLifecycle::InDev),
        "shipped" => Ok(IdeaLifecycle::Shipped),
        "archived" => Ok(IdeaLifecycle::Archived),
        other => Err(OrchdPersistError::Io(format!(
            "corrupt idea.lifecycle value: {other}"
        ))),
    }
}

fn encode_fit_verdict(v: &FitVerdict) -> &'static str {
    match v {
        FitVerdict::Fit => "fit",
        FitVerdict::NoFit => "no_fit",
        FitVerdict::Unknown => "unknown",
    }
}

fn decode_fit_verdict(s: &str) -> Result<FitVerdict, OrchdPersistError> {
    match s {
        "fit" => Ok(FitVerdict::Fit),
        "no_fit" => Ok(FitVerdict::NoFit),
        "unknown" => Ok(FitVerdict::Unknown),
        other => Err(OrchdPersistError::Io(format!(
            "corrupt insight.fit_verdict value: {other}"
        ))),
    }
}

fn encode_insight_status(s: &InsightStatus) -> &'static str {
    match s {
        InsightStatus::New => "new",
        InsightStatus::Accepted => "accepted",
        InsightStatus::Archived => "archived",
    }
}

fn decode_insight_status(s: &str) -> Result<InsightStatus, OrchdPersistError> {
    match s {
        "new" => Ok(InsightStatus::New),
        "accepted" => Ok(InsightStatus::Accepted),
        "archived" => Ok(InsightStatus::Archived),
        other => Err(OrchdPersistError::Io(format!(
            "corrupt insight.status value: {other}"
        ))),
    }
}

fn encode_task_status(s: &TaskStatus) -> &'static str {
    match s {
        TaskStatus::Backlog => "backlog",
        TaskStatus::Todo => "todo",
        TaskStatus::Waiting => "waiting",
        TaskStatus::Progress => "progress",
        TaskStatus::Testing => "testing",
        TaskStatus::Done => "done",
    }
}

fn decode_task_status(s: &str) -> Result<TaskStatus, OrchdPersistError> {
    match s {
        "backlog" => Ok(TaskStatus::Backlog),
        "todo" => Ok(TaskStatus::Todo),
        "waiting" => Ok(TaskStatus::Waiting),
        "progress" => Ok(TaskStatus::Progress),
        "testing" => Ok(TaskStatus::Testing),
        "done" => Ok(TaskStatus::Done),
        other => Err(OrchdPersistError::Io(format!(
            "corrupt task.status value: {other}"
        ))),
    }
}

/// `task.priority` TEXT literals (SCN-051, schema v5) — the exact strings the v5 CHECK
/// constraint locks, mirroring [`encode_task_status`]'s enum⇄TEXT shape.
fn encode_task_priority(p: &TaskPriority) -> &'static str {
    match p {
        TaskPriority::Urgent => "urgent",
        TaskPriority::Normal => "normal",
    }
}

fn decode_task_priority(s: &str) -> Result<TaskPriority, OrchdPersistError> {
    match s {
        "urgent" => Ok(TaskPriority::Urgent),
        "normal" => Ok(TaskPriority::Normal),
        other => Err(OrchdPersistError::Io(format!(
            "corrupt task.priority value: {other}"
        ))),
    }
}

fn encode_task_source(s: &TaskSource) -> &'static str {
    match s {
        TaskSource::Idea => "idea",
        TaskSource::Insight => "insight",
        TaskSource::Bug => "bug",
        TaskSource::Plan => "plan",
    }
}

fn decode_task_source(s: &str) -> Result<TaskSource, OrchdPersistError> {
    match s {
        "idea" => Ok(TaskSource::Idea),
        "insight" => Ok(TaskSource::Insight),
        "bug" => Ok(TaskSource::Bug),
        "plan" => Ok(TaskSource::Plan),
        other => Err(OrchdPersistError::Io(format!(
            "corrupt task.source value: {other}"
        ))),
    }
}

/// `task.tags` JSON array of strings round-trip (metric-refs-style JSON, mirrors
/// [`decode_metric_refs`]).
fn decode_tags(s: &str) -> Result<Vec<String>, OrchdPersistError> {
    serde_json::from_str(s)
        .map_err(|e| OrchdPersistError::Io(format!("corrupt task.tags json: {e}")))
}

/// `project.status` guard shared by every mutator (spec §5.2: "EVERY mutating verb touching
/// [an archived project] or its children ⇒ `Invariant`"). Takes `&Connection` so it works both
/// directly against `&self.conn` and — via `rusqlite::Transaction`'s
/// `Deref<Target = Connection>` — against an in-flight `&Transaction`. `pub(crate)`:
/// `crate::graph` reuses this exact guard (S4 spec §5 D11: every graph mutator honors it too).
pub(crate) fn ensure_project_active(
    conn: &Connection,
    project_id: &str,
) -> Result<(), OrchdPersistError> {
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

/// Archived-project guard for idea/insight rows, whose `project_id` column is NULLABLE
/// (orphaning keeps the row — spec §5.1 `idea`/`insight` DDL comments). A `None` (orphan) is
/// ALWAYS mutable — there is no project to be archived; a `Some(pid)` defers to
/// [`ensure_project_active`] as-is (so an unknown `pid` still surfaces `NotFound`, and an
/// archived `pid` still surfaces `Invariant`).
fn ensure_optional_project_active(
    conn: &Connection,
    project_id: Option<&str>,
) -> Result<(), OrchdPersistError> {
    match project_id {
        Some(pid) => ensure_project_active(conn, pid),
        None => Ok(()),
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

/// Raw `idea` row (text-encoded `lifecycle`) before decoding into the wire [`Idea`] type —
/// mirrors [`GoalRow`]'s shape.
struct IdeaRow {
    id: String,
    project_id: Option<String>,
    title: String,
    body: String,
    lifecycle: String,
    created_at: i64,
    updated_at: i64,
}

impl IdeaRow {
    fn from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<IdeaRow> {
        Ok(IdeaRow {
            id: r.get(0)?,
            project_id: r.get(1)?,
            title: r.get(2)?,
            body: r.get(3)?,
            lifecycle: r.get(4)?,
            created_at: r.get(5)?,
            updated_at: r.get(6)?,
        })
    }

    fn into_idea(self) -> Result<Idea, OrchdPersistError> {
        Ok(Idea {
            id: self.id,
            project_id: self.project_id,
            title: self.title,
            body: self.body,
            lifecycle: decode_idea_lifecycle(&self.lifecycle)?,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

fn load_idea(conn: &Connection, id: &str) -> Result<Idea, OrchdPersistError> {
    conn.query_row(
        "SELECT id, project_id, title, body, lifecycle, created_at, updated_at
         FROM idea WHERE id = ?1",
        rusqlite::params![id],
        IdeaRow::from_row,
    )
    .optional()?
    .ok_or(OrchdPersistError::NotFound)?
    .into_idea()
}

/// Raw `insight` row (text-encoded `fit_verdict`/`status`) before decoding into the wire
/// [`Insight`] type — mirrors [`GoalRow`]'s shape.
struct InsightRow {
    id: String,
    project_id: Option<String>,
    source: String,
    title: String,
    body: String,
    fit_verdict: Option<String>,
    fit_reasoning: String,
    status: String,
    resolution_reasoning: String,
    created_at: i64,
    updated_at: i64,
}

impl InsightRow {
    fn from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<InsightRow> {
        Ok(InsightRow {
            id: r.get(0)?,
            project_id: r.get(1)?,
            source: r.get(2)?,
            title: r.get(3)?,
            body: r.get(4)?,
            fit_verdict: r.get(5)?,
            fit_reasoning: r.get(6)?,
            status: r.get(7)?,
            resolution_reasoning: r.get(8)?,
            created_at: r.get(9)?,
            updated_at: r.get(10)?,
        })
    }

    fn into_insight(self) -> Result<Insight, OrchdPersistError> {
        Ok(Insight {
            id: self.id,
            project_id: self.project_id,
            source: self.source,
            title: self.title,
            body: self.body,
            fit_verdict: self
                .fit_verdict
                .as_deref()
                .map(decode_fit_verdict)
                .transpose()?,
            fit_reasoning: self.fit_reasoning,
            status: decode_insight_status(&self.status)?,
            resolution_reasoning: self.resolution_reasoning,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

fn load_insight(conn: &Connection, id: &str) -> Result<Insight, OrchdPersistError> {
    conn.query_row(
        "SELECT id, project_id, source, title, body, fit_verdict, fit_reasoning, status,
                resolution_reasoning, created_at, updated_at
         FROM insight WHERE id = ?1",
        rusqlite::params![id],
        InsightRow::from_row,
    )
    .optional()?
    .ok_or(OrchdPersistError::NotFound)?
    .into_insight()
}

/// Raw `task` row (text-encoded `status`/`source`, JSON-encoded `tags`) before decoding into the
/// wire [`DomainTask`] type — mirrors [`GoalRow`]'s shape.
struct TaskRow {
    id: String,
    project_id: String,
    parent_id: Option<String>,
    title: String,
    body: String,
    status: String,
    priority: String,
    source: String,
    source_id: Option<String>,
    tags: String,
    rank: f64,
    rank_agent: Option<f64>,
    rank_agent_reasoning: String,
    created_at: i64,
    updated_at: i64,
}

impl TaskRow {
    fn from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<TaskRow> {
        Ok(TaskRow {
            id: r.get(0)?,
            project_id: r.get(1)?,
            parent_id: r.get(2)?,
            title: r.get(3)?,
            body: r.get(4)?,
            status: r.get(5)?,
            priority: r.get(6)?,
            source: r.get(7)?,
            source_id: r.get(8)?,
            tags: r.get(9)?,
            rank: r.get(10)?,
            rank_agent: r.get(11)?,
            rank_agent_reasoning: r.get(12)?,
            created_at: r.get(13)?,
            updated_at: r.get(14)?,
        })
    }

    fn into_task(self) -> Result<DomainTask, OrchdPersistError> {
        Ok(DomainTask {
            id: self.id,
            project_id: self.project_id,
            parent_id: self.parent_id,
            title: self.title,
            body: self.body,
            status: decode_task_status(&self.status)?,
            priority: decode_task_priority(&self.priority)?,
            source: decode_task_source(&self.source)?,
            source_id: self.source_id,
            tags: decode_tags(&self.tags)?,
            rank: self.rank,
            rank_agent: self.rank_agent,
            rank_agent_reasoning: self.rank_agent_reasoning,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

fn load_task(conn: &Connection, id: &str) -> Result<DomainTask, OrchdPersistError> {
    conn.query_row(
        "SELECT id, project_id, parent_id, title, body, status, priority, source, source_id,
                tags, rank, rank_agent, rank_agent_reasoning, created_at, updated_at
         FROM task WHERE id = ?1",
        rusqlite::params![id],
        TaskRow::from_row,
    )
    .optional()?
    .ok_or(OrchdPersistError::NotFound)?
    .into_task()
}

/// Task analogue of [`ancestor_chain_contains`] (identical walk-up shape, `task` table instead
/// of `goal`; spec §5.2 "Task `parent_id` same-project + cycle-rejected (same walk-up)").
/// [`Db::create_task`] calls this defensively when validating `parent_id` against the
/// about-to-be-inserted task's own (pre-generated) id — in v1 there is no task reparent verb, so
/// a NEWLY created task can never actually be its own ancestor and this branch cannot trigger
/// through the public API today, but the walk-up logic itself is real and independently exercised
/// (see the `task_ancestor_chain_contains_*` tests below) for a future reparent verb (backlog,
/// spec §13) to reuse as-is.
fn task_ancestor_chain_contains(
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
                "SELECT parent_id FROM task WHERE id = ?1",
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
    /// (`title: "Strategic goal"`, empty body — owner edits it, never deletable), its
    /// `entity_ref` graph node (S4 spec §5 D6: `crate::graph::seed_strategic_entity_ref`, same
    /// tx — a project's graph is never empty) AND the project's `ruleset` DB row
    /// (`scope='project'`, default `md_path`, `md_hash=''`, `policy='{}'`; the FILE itself is
    /// written later by the T10 dispatch handler, not here). `workspace_ids` empty ⇒
    /// `Invariant`; a `workspace_id` already linked to ANY project (the
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

        let strategic_goal_id = Uuid::new_v4().to_string();
        tx.execute(
            "INSERT INTO goal
               (id, project_id, parent_id, kind, title, body, ord, status, metric_refs,
                created_at, updated_at)
             VALUES (?1, ?2, NULL, 'strategic', ?3, '', 0, 'active', '[]', ?4, ?4)",
            rusqlite::params![strategic_goal_id, id, STRATEGIC_GOAL_TITLE, now],
        )?;
        // D6: seed the strategic-goal entityRef node in the SAME tx (S4 spec §5) — a project's
        // graph is never empty.
        crate::graph::seed_strategic_entity_ref(
            &tx,
            &id,
            &strategic_goal_id,
            STRATEGIC_GOAL_TITLE,
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

    /// `UnarchiveProject` (spec D7, O-3): the exact reverse of [`Db::archive_project`], flipping
    /// `status='archived'` back to `'active'`. Unknown `id` ⇒ `NotFound`; an already-`active`
    /// project ⇒ `Invariant("project is not archived")` — the mirror of `archive_project`'s
    /// already-archived `Invariant`, so neither verb ever silently no-ops. Cannot reuse
    /// [`ensure_project_active`] (whose semantics are the opposite: it PASSES on `active` and
    /// FAILS on `archived`), so the status is read and matched inline here.
    pub fn unarchive_project(&self, id: &str) -> Result<Project, OrchdPersistError> {
        let tx = self.conn.unchecked_transaction()?;
        let status: Option<String> = tx
            .query_row(
                "SELECT status FROM project WHERE id = ?1",
                rusqlite::params![id],
                |r| r.get(0),
            )
            .optional()?;
        match status.as_deref() {
            None => return Err(OrchdPersistError::NotFound),
            Some("archived") => {}
            Some(_) => {
                return Err(OrchdPersistError::Invariant(
                    "project is not archived".to_string(),
                ));
            }
        }
        tx.execute(
            "UPDATE project SET status = 'active', updated_at = ?2 WHERE id = ?1",
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

    /// Single-project fetch by id (spec §8's `export::export_project` needs a by-id lookup that
    /// no other project verb provides — every other project method either creates/mutates one
    /// project or lists ALL of them). Reads work unconditionally (no archived-project guard),
    /// mirrors [`Db::list_projects`]/[`Db::list_goals`]. Unknown `id` ⇒ `NotFound`.
    pub fn get_project(&self, id: &str) -> Result<Project, OrchdPersistError> {
        load_project(&self.conn, id)
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

// ================================================================================
// ---- domain persistence (spec §5.2): idea + insight + task CRUD (T7) ----
// ================================================================================

impl Db {
    /// `CreateIdea` (spec §4.2/§5.2). `project_id: None` ⇒ orphan idea (always mutable, per the
    /// `idea.project_id` `ON DELETE SET NULL` column comment "orphaning keeps the idea");
    /// `Some(pid)` ⇒ `pid` must resolve to an existing, active project
    /// ([`ensure_optional_project_active`] gives `NotFound`/`Invariant` as appropriate).
    /// `lifecycle` always starts `Captured` (DB column default `'captured'`).
    pub fn create_idea(
        &self,
        project_id: Option<&str>,
        title: &str,
        body: &str,
    ) -> Result<Idea, OrchdPersistError> {
        let tx = self.conn.unchecked_transaction()?;
        ensure_optional_project_active(&tx, project_id)?;

        let id = Uuid::new_v4().to_string();
        let now = now_ms();
        tx.execute(
            "INSERT INTO idea (id, project_id, title, body, lifecycle, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, 'captured', ?5, ?5)",
            rusqlite::params![id, project_id, title, body, now],
        )?;

        let idea = load_idea(&tx, &id)?;
        tx.commit()?;
        Ok(idea)
    }

    /// `UpdateIdea` (spec §4.2). Only provided fields change, `updated_at` bumps only if at
    /// least one did. Guard uses the idea's OWN current `project_id`
    /// ([`ensure_optional_project_active`] — orphan ideas are always mutable). Unknown `id` ⇒
    /// `NotFound`.
    pub fn update_idea(
        &self,
        id: &str,
        title: Option<&str>,
        body: Option<&str>,
    ) -> Result<Idea, OrchdPersistError> {
        let tx = self.conn.unchecked_transaction()?;
        let project_id: Option<String> = tx
            .query_row(
                "SELECT project_id FROM idea WHERE id = ?1",
                rusqlite::params![id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(OrchdPersistError::NotFound)?;
        ensure_optional_project_active(&tx, project_id.as_deref())?;

        if title.is_some() || body.is_some() {
            tx.execute(
                "UPDATE idea SET
                   title = COALESCE(?2, title),
                   body = COALESCE(?3, body),
                   updated_at = ?4
                 WHERE id = ?1",
                rusqlite::params![id, title, body, now_ms()],
            )?;
        }

        let idea = load_idea(&tx, id)?;
        tx.commit()?;
        Ok(idea)
    }

    /// `SetIdeaProject` (D11 dedicated verb, spec §4.2: no `Option<Option<T>>`): `project_id:
    /// None` detaches (column ⇒ `NULL`). Guards BOTH the idea's CURRENT project (if any —
    /// moving/detaching a child of an archived project is itself a mutating verb touching that
    /// project) and the NEW target project (if any — attaching to an archived project is
    /// disallowed; an unknown target `pid` ⇒ `NotFound`, via [`ensure_project_active`] inside
    /// [`ensure_optional_project_active`]). Unknown `id` ⇒ `NotFound`.
    pub fn set_idea_project(
        &self,
        id: &str,
        project_id: Option<&str>,
    ) -> Result<Idea, OrchdPersistError> {
        let tx = self.conn.unchecked_transaction()?;
        let current_project_id: Option<String> = tx
            .query_row(
                "SELECT project_id FROM idea WHERE id = ?1",
                rusqlite::params![id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(OrchdPersistError::NotFound)?;
        ensure_optional_project_active(&tx, current_project_id.as_deref())?;
        ensure_optional_project_active(&tx, project_id)?;

        tx.execute(
            "UPDATE idea SET project_id = ?2, updated_at = ?3 WHERE id = ?1",
            rusqlite::params![id, project_id, now_ms()],
        )?;

        let idea = load_idea(&tx, id)?;
        tx.commit()?;
        Ok(idea)
    }

    /// `SetIdeaLifecycle` (spec §4.2). Guard uses the idea's OWN current `project_id`. Unknown
    /// `id` ⇒ `NotFound`.
    pub fn set_idea_lifecycle(
        &self,
        id: &str,
        lifecycle: IdeaLifecycle,
    ) -> Result<Idea, OrchdPersistError> {
        let tx = self.conn.unchecked_transaction()?;
        let project_id: Option<String> = tx
            .query_row(
                "SELECT project_id FROM idea WHERE id = ?1",
                rusqlite::params![id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(OrchdPersistError::NotFound)?;
        ensure_optional_project_active(&tx, project_id.as_deref())?;

        tx.execute(
            "UPDATE idea SET lifecycle = ?2, updated_at = ?3 WHERE id = ?1",
            rusqlite::params![id, encode_idea_lifecycle(&lifecycle), now_ms()],
        )?;

        let idea = load_idea(&tx, id)?;
        tx.commit()?;
        Ok(idea)
    }

    /// `DeleteIdea` (spec §4.2). Guard uses the idea's OWN current `project_id`. Unknown `id` ⇒
    /// `NotFound`.
    pub fn delete_idea(&self, id: &str) -> Result<(), OrchdPersistError> {
        let tx = self.conn.unchecked_transaction()?;
        let project_id: Option<String> = tx
            .query_row(
                "SELECT project_id FROM idea WHERE id = ?1",
                rusqlite::params![id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(OrchdPersistError::NotFound)?;
        ensure_optional_project_active(&tx, project_id.as_deref())?;

        tx.execute("DELETE FROM idea WHERE id = ?1", rusqlite::params![id])?;
        tx.commit()?;
        Ok(())
    }

    /// `ListIdeas` (spec §4.2): `project_id: None` ⇒ every idea including orphans; `Some(pid)`
    /// ⇒ only that project's ideas (orphans excluded). `created_at DESC`. Reads work
    /// unconditionally (no archived-project guard on reads, mirrors [`Db::list_goals`]).
    pub fn list_ideas(&self, project_id: Option<&str>) -> Result<Vec<Idea>, OrchdPersistError> {
        let mut stmt = self.conn.prepare(
            "SELECT id FROM idea WHERE ?1 IS NULL OR project_id = ?1 ORDER BY created_at DESC, id",
        )?;
        let ids: Vec<String> = stmt
            .query_map(rusqlite::params![project_id], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        drop(stmt);
        ids.iter().map(|id| load_idea(&self.conn, id)).collect()
    }

    /// `CreateInsight` (spec §4.2/§5.2). `project_id: None` ⇒ orphan insight (always mutable);
    /// `Some(pid)` ⇒ `pid` must resolve to an existing, active project. `status` always starts
    /// `New`, `fit_verdict` starts unset (`NULL`), `fit_reasoning`/`resolution_reasoning` start
    /// `""` (DB column defaults).
    pub fn create_insight(
        &self,
        project_id: Option<&str>,
        source: &str,
        title: &str,
        body: &str,
    ) -> Result<Insight, OrchdPersistError> {
        let tx = self.conn.unchecked_transaction()?;
        ensure_optional_project_active(&tx, project_id)?;

        let id = Uuid::new_v4().to_string();
        let now = now_ms();
        tx.execute(
            "INSERT INTO insight
               (id, project_id, source, title, body, status, fit_reasoning,
                resolution_reasoning, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'new', '', '', ?6, ?6)",
            rusqlite::params![id, project_id, source, title, body, now],
        )?;

        let insight = load_insight(&tx, &id)?;
        tx.commit()?;
        Ok(insight)
    }

    /// `UpdateInsight` (spec §4.2). Only provided fields change. Guard uses the insight's OWN
    /// current `project_id`. Unknown `id` ⇒ `NotFound`.
    pub fn update_insight(
        &self,
        id: &str,
        title: Option<&str>,
        body: Option<&str>,
    ) -> Result<Insight, OrchdPersistError> {
        let tx = self.conn.unchecked_transaction()?;
        let project_id: Option<String> = tx
            .query_row(
                "SELECT project_id FROM insight WHERE id = ?1",
                rusqlite::params![id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(OrchdPersistError::NotFound)?;
        ensure_optional_project_active(&tx, project_id.as_deref())?;

        if title.is_some() || body.is_some() {
            tx.execute(
                "UPDATE insight SET
                   title = COALESCE(?2, title),
                   body = COALESCE(?3, body),
                   updated_at = ?4
                 WHERE id = ?1",
                rusqlite::params![id, title, body, now_ms()],
            )?;
        }

        let insight = load_insight(&tx, id)?;
        tx.commit()?;
        Ok(insight)
    }

    /// `SetInsightFitVerdict` (D11 dedicated verb, spec §4.2: no `Option<Option<T>>`) — sets
    /// BOTH `fit_verdict` (nullable, `None` ⇒ `NULL`) and `fit_reasoning` (non-null, always
    /// overwritten) together. Guard uses the insight's OWN current `project_id`. Unknown `id` ⇒
    /// `NotFound`.
    pub fn set_insight_fit_verdict(
        &self,
        id: &str,
        fit_verdict: Option<FitVerdict>,
        fit_reasoning: &str,
    ) -> Result<Insight, OrchdPersistError> {
        let tx = self.conn.unchecked_transaction()?;
        let project_id: Option<String> = tx
            .query_row(
                "SELECT project_id FROM insight WHERE id = ?1",
                rusqlite::params![id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(OrchdPersistError::NotFound)?;
        ensure_optional_project_active(&tx, project_id.as_deref())?;

        let fit_verdict_text = fit_verdict.as_ref().map(encode_fit_verdict);
        tx.execute(
            "UPDATE insight SET fit_verdict = ?2, fit_reasoning = ?3, updated_at = ?4
             WHERE id = ?1",
            rusqlite::params![id, fit_verdict_text, fit_reasoning, now_ms()],
        )?;

        let insight = load_insight(&tx, id)?;
        tx.commit()?;
        Ok(insight)
    }

    /// `SetInsightStatus` (spec §4.2). `resolution_reasoning: None` leaves the column unchanged
    /// (D11: plain `Option<T>` on a NON-nullable column means "absent/null = unchanged"; `Some`
    /// overwrites, including with `""`). Guard uses the insight's OWN current `project_id`.
    /// Unknown `id` ⇒ `NotFound`.
    ///
    /// **Graph-ingest on accept (S-IDEA spec §6 D9, task T4).** When `status` transitions to
    /// `Accepted`, this ALSO seeds the insight as an `entity_ref` graph node
    /// (`crate::graph::Db::add_entity_ref_node`) — the owner-curated insight is graph-ingested,
    /// never the raw untrusted `mcp_artifact` a research run produced (D9: "graph-ingest the
    /// insight, not the raw artifact"). `add_entity_ref_node` opens and commits its OWN
    /// transaction (see its doc comment: SQLite has no nested `BEGIN` on one connection, so it
    /// can't be folded into the status-update `tx` above) — this call happens SEQUENTIALLY, after
    /// that transaction has already committed. The ingest is BEST-EFFORT (the status is already
    /// durably committed, so a failed ingest must not fail the whole call — see the inline
    /// comment): a `Conflict` (the node already exists — a re-accept after an intervening archive;
    /// archiving never removes the node, S4's orphan-on-delete model) is a benign no-op, and any
    /// OTHER `add_entity_ref_node` error is logged-and-swallowed (mirroring
    /// `socket_server::write_initial_ruleset_file`'s post-commit precedent), so this method still
    /// returns `Ok` once the status is committed. A project-less (orphan) insight has no
    /// `Some(project_id)` to ingest against — the graph is project-scoped — so it is silently
    /// skipped, same honest-degradation shape as everywhere else an orphan insight is handled.
    /// `Archived`/`New` transitions never seed a node.
    pub fn set_insight_status(
        &self,
        id: &str,
        status: InsightStatus,
        resolution_reasoning: Option<&str>,
    ) -> Result<Insight, OrchdPersistError> {
        let tx = self.conn.unchecked_transaction()?;
        let project_id: Option<String> = tx
            .query_row(
                "SELECT project_id FROM insight WHERE id = ?1",
                rusqlite::params![id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(OrchdPersistError::NotFound)?;
        ensure_optional_project_active(&tx, project_id.as_deref())?;

        tx.execute(
            "UPDATE insight SET
               status = ?2,
               resolution_reasoning = COALESCE(?3, resolution_reasoning),
               updated_at = ?4
             WHERE id = ?1",
            rusqlite::params![
                id,
                encode_insight_status(&status),
                resolution_reasoning,
                now_ms()
            ],
        )?;

        let insight = load_insight(&tx, id)?;
        tx.commit()?;

        // Graph-ingest is a BEST-EFFORT post-commit side effect (mirrors
        // `socket_server::write_initial_ruleset_file`'s log-and-swallow precedent): the accept
        // is ALREADY durably committed above, so a failure here must NOT turn the whole call into
        // an `Err` — that would hand the client an error reply (and skip the `InsightsChanged`
        // push) while the DB has genuinely flipped the status. A `Conflict` (the node already
        // exists — a re-accept after archive; archiving never removes the node, S4's
        // orphan-on-delete model) is the expected benign case and is silently ignored; any OTHER
        // error is logged (`insight_id`/`entity_type`/error only — the insight `title` is NEVER
        // logged, it can carry owner PII) and swallowed, so the accept still returns `Ok`.
        if matches!(status, InsightStatus::Accepted) {
            if let Some(pid) = insight.project_id.as_deref() {
                match self.add_entity_ref_node(
                    pid,
                    GraphEntityType::Insight,
                    id,
                    &insight.title,
                    0.0,
                    0.0,
                ) {
                    Ok(_) | Err(OrchdPersistError::Conflict(_)) => {}
                    Err(e) => warn!(
                        insight_id = %id,
                        entity_type = "insight",
                        error = %e,
                        "graph-ingest of an accepted insight failed; status is committed, the \
                         entityRef node is missing until a re-accept retries it"
                    ),
                }
            }
        }

        Ok(insight)
    }

    /// `DeleteInsight` (spec §4.2). Guard uses the insight's OWN current `project_id`. Unknown
    /// `id` ⇒ `NotFound`.
    pub fn delete_insight(&self, id: &str) -> Result<(), OrchdPersistError> {
        let tx = self.conn.unchecked_transaction()?;
        let project_id: Option<String> = tx
            .query_row(
                "SELECT project_id FROM insight WHERE id = ?1",
                rusqlite::params![id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(OrchdPersistError::NotFound)?;
        ensure_optional_project_active(&tx, project_id.as_deref())?;

        tx.execute("DELETE FROM insight WHERE id = ?1", rusqlite::params![id])?;
        tx.commit()?;
        Ok(())
    }

    /// `ListInsights` (spec §4.2): `project_id: None` ⇒ every insight including orphans;
    /// `Some(pid)` ⇒ only that project's insights. `created_at DESC`. Reads work
    /// unconditionally, mirrors [`Db::list_ideas`].
    pub fn list_insights(
        &self,
        project_id: Option<&str>,
    ) -> Result<Vec<Insight>, OrchdPersistError> {
        let mut stmt = self.conn.prepare(
            "SELECT id FROM insight WHERE ?1 IS NULL OR project_id = ?1
             ORDER BY created_at DESC, id",
        )?;
        let ids: Vec<String> = stmt
            .query_map(rusqlite::params![project_id], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        drop(stmt);
        ids.iter().map(|id| load_insight(&self.conn, id)).collect()
    }

    /// `CreateTask` (spec §4.2/§5.2). `project_id` is REQUIRED (unlike idea/insight — the `task`
    /// table has `project_id TEXT NOT NULL`) and must resolve to an existing, active project
    /// (unknown ⇒ `NotFound`, archived ⇒ `Invariant`, via [`ensure_project_active`]). `parent_id`
    /// (if `Some`) must reference an EXISTING task (else `NotFound`) in the SAME `project_id`
    /// (else `Invariant`); the walk-up cycle guard ([`task_ancestor_chain_contains`]) is checked
    /// defensively too (see that function's doc for why it can't trigger through this verb in
    /// v1). `rank = COALESCE(MAX(rank), 0) + 1024` scoped to `project_id` (first task in a
    /// project ⇒ exactly `1024`). `status` defaults `Backlog` when `None`; `priority` defaults
    /// `Normal` when `None` (SCN-051 — set at create time OR later via [`Db::set_task_priority`]).
    /// `rank_agent` starts unset, `rank_agent_reasoning` starts `""` (DB column defaults) — those
    /// are agent-set fields with no owning verb in T7.
    #[allow(clippy::too_many_arguments)]
    pub fn create_task(
        &self,
        project_id: &str,
        parent_id: Option<&str>,
        title: &str,
        body: &str,
        status: Option<TaskStatus>,
        source: TaskSource,
        source_id: Option<&str>,
        tags: &[String],
        priority: Option<TaskPriority>,
    ) -> Result<DomainTask, OrchdPersistError> {
        let tx = self.conn.unchecked_transaction()?;
        ensure_project_active(&tx, project_id)?;

        let id = Uuid::new_v4().to_string();

        if let Some(parent) = parent_id {
            let parent_project: Option<String> = tx
                .query_row(
                    "SELECT project_id FROM task WHERE id = ?1",
                    rusqlite::params![parent],
                    |r| r.get(0),
                )
                .optional()?;
            let parent_project = parent_project.ok_or(OrchdPersistError::NotFound)?;
            if parent_project != project_id {
                return Err(OrchdPersistError::Invariant(
                    "task parent_id must belong to the same project".to_string(),
                ));
            }
            if task_ancestor_chain_contains(&tx, parent, &id)? {
                return Err(OrchdPersistError::Invariant(
                    "cannot create a task under itself or one of its own descendants".to_string(),
                ));
            }
        }

        let max_rank: Option<f64> = tx.query_row(
            "SELECT MAX(rank) FROM task WHERE project_id = ?1",
            rusqlite::params![project_id],
            |r| r.get(0),
        )?;
        let rank = max_rank.unwrap_or(0.0) + 1024.0;

        let tags_json = serde_json::to_string(tags)
            .map_err(|e| OrchdPersistError::Io(format!("failed to serialize tags: {e}")))?;
        let status = status.unwrap_or(TaskStatus::Backlog);
        let priority = priority.unwrap_or_default();
        let now = now_ms();
        tx.execute(
            "INSERT INTO task
               (id, project_id, parent_id, title, body, status, priority, source, source_id,
                tags, rank, rank_agent, rank_agent_reasoning, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, NULL, '', ?12, ?12)",
            rusqlite::params![
                id,
                project_id,
                parent_id,
                title,
                body,
                encode_task_status(&status),
                encode_task_priority(&priority),
                encode_task_source(&source),
                source_id,
                tags_json,
                rank,
                now
            ],
        )?;

        let task = load_task(&tx, &id)?;
        tx.commit()?;
        Ok(task)
    }

    /// `UpdateTask` (spec §4.2). Only provided fields change; `tags` round-trips as a JSON
    /// array of strings. Guard uses the task's OWN `project_id` (never null for a task). Unknown
    /// `id` ⇒ `NotFound`.
    pub fn update_task(
        &self,
        id: &str,
        title: Option<&str>,
        body: Option<&str>,
        tags: Option<&[String]>,
    ) -> Result<DomainTask, OrchdPersistError> {
        let tx = self.conn.unchecked_transaction()?;
        let project_id: String = tx
            .query_row(
                "SELECT project_id FROM task WHERE id = ?1",
                rusqlite::params![id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(OrchdPersistError::NotFound)?;
        ensure_project_active(&tx, &project_id)?;

        let tags_json = tags
            .map(serde_json::to_string)
            .transpose()
            .map_err(|e| OrchdPersistError::Io(format!("failed to serialize tags: {e}")))?;

        if title.is_some() || body.is_some() || tags_json.is_some() {
            tx.execute(
                "UPDATE task SET
                   title = COALESCE(?2, title),
                   body = COALESCE(?3, body),
                   tags = COALESCE(?4, tags),
                   updated_at = ?5
                 WHERE id = ?1",
                rusqlite::params![id, title, body, tags_json, now_ms()],
            )?;
        }

        let task = load_task(&tx, id)?;
        tx.commit()?;
        Ok(task)
    }

    /// `SetTaskStatus` (spec §4.2). Guard uses the task's OWN `project_id`. Unknown `id` ⇒
    /// `NotFound`.
    pub fn set_task_status(
        &self,
        id: &str,
        status: TaskStatus,
    ) -> Result<DomainTask, OrchdPersistError> {
        let tx = self.conn.unchecked_transaction()?;
        let project_id: String = tx
            .query_row(
                "SELECT project_id FROM task WHERE id = ?1",
                rusqlite::params![id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(OrchdPersistError::NotFound)?;
        ensure_project_active(&tx, &project_id)?;

        tx.execute(
            "UPDATE task SET status = ?2, updated_at = ?3 WHERE id = ?1",
            rusqlite::params![id, encode_task_status(&status), now_ms()],
        )?;

        let task = load_task(&tx, id)?;
        tx.commit()?;
        Ok(task)
    }

    /// `SetTaskPriority` (SCN-051, ST-037): the focused per-mutation verb for the urgent/normal
    /// flip — the exact [`Db::set_task_status`] shape (guard uses the task's OWN `project_id`;
    /// unknown `id` ⇒ `NotFound`; archived project ⇒ `Invariant`). Sorting urgent-ahead within a
    /// status group is a CLIENT concern (`TasksList.tsx`) — this verb only persists the field;
    /// workflow continuation (SCN-049, future) reads it back through `list_tasks`.
    pub fn set_task_priority(
        &self,
        id: &str,
        priority: TaskPriority,
    ) -> Result<DomainTask, OrchdPersistError> {
        let tx = self.conn.unchecked_transaction()?;
        let project_id: String = tx
            .query_row(
                "SELECT project_id FROM task WHERE id = ?1",
                rusqlite::params![id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(OrchdPersistError::NotFound)?;
        ensure_project_active(&tx, &project_id)?;

        tx.execute(
            "UPDATE task SET priority = ?2, updated_at = ?3 WHERE id = ?1",
            rusqlite::params![id, encode_task_priority(&priority), now_ms()],
        )?;

        let task = load_task(&tx, id)?;
        tx.commit()?;
        Ok(task)
    }

    /// `SetTaskRank` (spec §4.2/§5.2): takes an explicit `f64` verbatim — fractional
    /// insert-between midpoint math is the CLIENT's move, this verb just persists whatever it is
    /// given. Guard uses the task's OWN `project_id`. Unknown `id` ⇒ `NotFound`.
    pub fn set_task_rank(&self, id: &str, rank: f64) -> Result<DomainTask, OrchdPersistError> {
        let tx = self.conn.unchecked_transaction()?;
        let project_id: String = tx
            .query_row(
                "SELECT project_id FROM task WHERE id = ?1",
                rusqlite::params![id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(OrchdPersistError::NotFound)?;
        ensure_project_active(&tx, &project_id)?;

        tx.execute(
            "UPDATE task SET rank = ?2, updated_at = ?3 WHERE id = ?1",
            rusqlite::params![id, rank, now_ms()],
        )?;

        let task = load_task(&tx, id)?;
        tx.commit()?;
        Ok(task)
    }

    /// `DeleteTask` (spec §4.2). Guard uses the task's OWN `project_id`. The FK
    /// `task.parent_id REFERENCES task(id) ON DELETE CASCADE` removes the whole subtask
    /// subtree. Unknown `id` ⇒ `NotFound`.
    pub fn delete_task(&self, id: &str) -> Result<(), OrchdPersistError> {
        let tx = self.conn.unchecked_transaction()?;
        let project_id: String = tx
            .query_row(
                "SELECT project_id FROM task WHERE id = ?1",
                rusqlite::params![id],
                |r| r.get(0),
            )
            .optional()?
            .ok_or(OrchdPersistError::NotFound)?;
        ensure_project_active(&tx, &project_id)?;

        tx.execute("DELETE FROM task WHERE id = ?1", rusqlite::params![id])?;
        tx.commit()?;
        Ok(())
    }

    /// `ListTasks` (spec §4.2): `project_id: None` ⇒ every task across every project;
    /// `Some(pid)` ⇒ only that project's tasks. `ORDER BY rank`. Reads work unconditionally,
    /// mirrors [`Db::list_ideas`].
    pub fn list_tasks(
        &self,
        project_id: Option<&str>,
    ) -> Result<Vec<DomainTask>, OrchdPersistError> {
        let mut stmt = self
            .conn
            .prepare("SELECT id FROM task WHERE ?1 IS NULL OR project_id = ?1 ORDER BY rank, id")?;
        let ids: Vec<String> = stmt
            .query_map(rusqlite::params![project_id], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        drop(stmt);
        ids.iter().map(|id| load_task(&self.conn, id)).collect()
    }
}

// ================================================================================
// ---- domain persistence (spec §5.2, §7): ruleset get/upsert/acknowledge (T8) ----
// ================================================================================

fn encode_rule_scope(s: &RuleScope) -> &'static str {
    match s {
        RuleScope::Global => "global",
        RuleScope::Project => "project",
    }
}

fn decode_rule_scope(s: &str) -> Result<RuleScope, OrchdPersistError> {
    match s {
        "global" => Ok(RuleScope::Global),
        "project" => Ok(RuleScope::Project),
        other => Err(OrchdPersistError::Io(format!(
            "corrupt ruleset.scope value: {other}"
        ))),
    }
}

/// Local "shape contract" mirror of `bpa_orchd_proto::PolicyRules`. Two jobs:
/// 1. STRICT-VALIDATE a policy before it is stored (spec §5.2: "`PolicyRules` strict-validated
///    (`deny_unknown_fields`, `spend_cap_usd >= 0`, non-empty allowlist entries)").
///    [`validate_policy`] round-trips the caller's already-typed `&PolicyRules` through JSON into
///    THIS struct: because it derives `deny_unknown_fields`, that round trip fails loudly the
///    moment `PolicyRules` (the wire type in `bpa-orchd-proto`) grows a field this mirror doesn't
///    know about yet, instead of silently dropping the unrecognized field into the DB's JSON blob
///    — the persistence layer's understanding of "what a policy is" is forced to stay in lockstep
///    with the wire type. See the `validate_policy_rejects_an_unknown_json_key` test below for a
///    direct proof of the mechanism.
/// 2. DECODE a stored `ruleset.policy` value back into [`RuleSet`] ([`RuleSetRow::into_ruleset`]):
///    `#[serde(default)]` lets the DB's own `'{}'` default (every ruleset row starts this way —
///    `Db::create_project`'s INSERT, the migration's `DEFAULT '{}'`) decode into "no fields set"
///    instead of a spurious "missing field" error — `PolicyRules` itself has no such defaults
///    (`approval_classes`/`path_allowlist` are non-`Option` on the wire, by design: an update that
///    actually SETS them must send them explicitly, spec D11).
#[derive(Debug, Default, serde::Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
struct PolicyRulesStrict {
    spend_cap_usd: Option<f64>,
    approval_classes: Vec<String>,
    path_allowlist: Vec<String>,
}

/// Validates `policy` (spec §5.2) and returns its canonical JSON encoding (camelCase, matching
/// the wire shape — the DB's `ruleset.policy` column stores exactly this) ready to store.
/// `spend_cap_usd`, if present, must be non-negative; every `approval_classes`/`path_allowlist`
/// entry must be non-empty; any key [`PolicyRulesStrict`]'s mirror doesn't recognize ⇒
/// `Validation` (see its doc for why that check exists at all despite the input already being a
/// typed `&PolicyRules`).
fn validate_policy(policy: &PolicyRules) -> Result<String, OrchdPersistError> {
    let json = serde_json::to_string(policy)
        .map_err(|e| OrchdPersistError::Io(format!("failed to serialize policy: {e}")))?;
    let strict: PolicyRulesStrict = serde_json::from_str(&json)
        .map_err(|e| OrchdPersistError::Validation(format!("invalid policy: {e}")))?;
    if let Some(cap) = strict.spend_cap_usd {
        if cap < 0.0 {
            return Err(OrchdPersistError::Validation(
                "policy.spend_cap_usd must be >= 0".to_string(),
            ));
        }
    }
    if strict.approval_classes.iter().any(|s| s.is_empty()) {
        return Err(OrchdPersistError::Validation(
            "policy.approval_classes entries must be non-empty".to_string(),
        ));
    }
    if strict.path_allowlist.iter().any(|s| s.is_empty()) {
        return Err(OrchdPersistError::Validation(
            "policy.path_allowlist entries must be non-empty".to_string(),
        ));
    }
    Ok(json)
}

/// Raw `ruleset` row (text-encoded `scope`, JSON-encoded `policy`) before decoding into the wire
/// [`RuleSet`] type — mirrors [`GoalRow`]'s shape.
struct RuleSetRow {
    id: String,
    scope: String,
    project_id: Option<String>,
    md_path: String,
    md_hash: String,
    policy: String,
    created_at: i64,
    updated_at: i64,
}

impl RuleSetRow {
    fn from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<RuleSetRow> {
        Ok(RuleSetRow {
            id: r.get(0)?,
            scope: r.get(1)?,
            project_id: r.get(2)?,
            md_path: r.get(3)?,
            md_hash: r.get(4)?,
            policy: r.get(5)?,
            created_at: r.get(6)?,
            updated_at: r.get(7)?,
        })
    }

    fn into_ruleset(self) -> Result<RuleSet, OrchdPersistError> {
        // See `PolicyRulesStrict`'s doc (job 2): decoding through it (not directly into
        // `PolicyRules`) lets the DB's `'{}'` default decode cleanly.
        let decoded: PolicyRulesStrict = serde_json::from_str(&self.policy)
            .map_err(|e| OrchdPersistError::Io(format!("corrupt ruleset.policy json: {e}")))?;
        let policy = PolicyRules {
            spend_cap_usd: decoded.spend_cap_usd,
            approval_classes: decoded.approval_classes,
            path_allowlist: decoded.path_allowlist,
        };
        Ok(RuleSet {
            id: self.id,
            scope: decode_rule_scope(&self.scope)?,
            project_id: self.project_id,
            md_path: self.md_path,
            md_hash: self.md_hash,
            policy,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

/// `scope`/`project_id` lookup (spec §5.1 CHECK: exactly one `global` row with `project_id IS
/// NULL`, at most one `project` row per `project_id`). `IS` (not `=`) so `project_id: None`
/// correctly matches `project_id IS NULL` rather than never matching (SQL `NULL = NULL` is
/// `NULL`, not true) — same pattern as [`Db::create_goal`]'s `parent_id IS ?2` lookup.
fn load_ruleset_row_by_scope(
    conn: &Connection,
    scope: &RuleScope,
    project_id: Option<&str>,
) -> Result<RuleSetRow, OrchdPersistError> {
    conn.query_row(
        "SELECT id, scope, project_id, md_path, md_hash, policy, created_at, updated_at
         FROM ruleset WHERE scope = ?1 AND project_id IS ?2",
        rusqlite::params![encode_rule_scope(scope), project_id],
        RuleSetRow::from_row,
    )
    .optional()?
    .ok_or(OrchdPersistError::NotFound)
}

fn load_ruleset_row_by_id(conn: &Connection, id: &str) -> Result<RuleSetRow, OrchdPersistError> {
    conn.query_row(
        "SELECT id, scope, project_id, md_path, md_hash, policy, created_at, updated_at
         FROM ruleset WHERE id = ?1",
        rusqlite::params![id],
        RuleSetRow::from_row,
    )
    .optional()?
    .ok_or(OrchdPersistError::NotFound)
}

fn load_ruleset_by_id(conn: &Connection, id: &str) -> Result<RuleSet, OrchdPersistError> {
    load_ruleset_row_by_id(conn, id)?.into_ruleset()
}

impl Db {
    /// `GetRuleSet`'s DB-row half (spec §4.2/§7 — the FILE half is read separately, fresh, by
    /// `ruleset_files::read_state`; the socket dispatch layer (a later task) assembles both into
    /// the wire `RuleSetView`). Unknown `(scope, project_id)` ⇒ `NotFound`.
    pub fn get_ruleset(
        &self,
        scope: RuleScope,
        project_id: Option<&str>,
    ) -> Result<RuleSet, OrchdPersistError> {
        load_ruleset_row_by_scope(&self.conn, &scope, project_id)?.into_ruleset()
    }

    /// `UpsertRuleSet` (spec §7, D4). Every ruleset ROW already exists by the time this is
    /// called — the global row is ensured at every boot (`boot::ensure_global_ruleset`), a
    /// project's row is auto-created WITH the project (`Db::create_project`, spec §5.2) — so this
    /// only ever UPDATES that existing row's `md_path`/`md_hash`/`policy`; unknown
    /// `(scope, project_id)` ⇒ `NotFound`. Order (spec §7), all inside one transaction:
    /// 1. `md_path: Some` is validated (absolute + parent dir exists, else `Validation`) and
    ///    repoints the row — checked BEFORE any side effect, so an invalid path never leaves a
    ///    half-applied write.
    /// 2. `md_content: Some` is written atomically ([`crate::ruleset_files::write_atomic`]) to the
    ///    (possibly just-repointed) path and its sha256 replaces `md_hash`.
    /// 3. `policy: Some` is strict-validated ([`validate_policy`]) and replaces `policy`.
    ///
    /// `updated_at` always bumps — unlike the other `update_*` verbs (which only bump when a
    /// field actually changed), every `UpsertRuleSet` call is by definition a real touch; there is
    /// no "nothing provided" no-op form of an upsert.
    pub fn upsert_ruleset(
        &self,
        scope: RuleScope,
        project_id: Option<&str>,
        md_content: Option<&str>,
        md_path: Option<&str>,
        policy: Option<&PolicyRules>,
    ) -> Result<RuleSet, OrchdPersistError> {
        if let Some(p) = md_path {
            let path = Path::new(p);
            if !path.is_absolute() {
                return Err(OrchdPersistError::Validation(
                    "ruleset md_path must be absolute".to_string(),
                ));
            }
            let parent_exists = path.parent().is_some_and(|d| d.is_dir());
            if !parent_exists {
                return Err(OrchdPersistError::Validation(
                    "ruleset md_path's parent directory does not exist".to_string(),
                ));
            }
        }
        // Validate policy up front too (before touching the DB/filesystem) — a Validation error
        // must never leave a half-applied upsert behind.
        let policy_json = policy.map(validate_policy).transpose()?;

        let tx = self.conn.unchecked_transaction()?;
        // spec §5.2: a project-scoped ruleset row is a child of `project` — the archived-project
        // guard must fire before the row is loaded/written, same as every other project-scoped
        // mutator. A global-scoped ruleset (`project_id: None`) has no project, so no check.
        ensure_optional_project_active(&tx, project_id)?;
        let row = load_ruleset_row_by_scope(&tx, &scope, project_id)?;
        let effective_path = md_path.unwrap_or(&row.md_path).to_string();

        let new_hash = match md_content {
            Some(content) => Some(
                crate::ruleset_files::write_atomic(Path::new(&effective_path), content)
                    .map_err(|e| OrchdPersistError::Io(e.to_string()))?,
            ),
            None => None,
        };

        tx.execute(
            "UPDATE ruleset SET
               md_path = ?2,
               md_hash = COALESCE(?3, md_hash),
               policy = COALESCE(?4, policy),
               updated_at = ?5
             WHERE id = ?1",
            rusqlite::params![row.id, effective_path, new_hash, policy_json, now_ms()],
        )?;

        let ruleset = load_ruleset_by_id(&tx, &row.id)?;
        tx.commit()?;
        Ok(ruleset)
    }

    /// `AcknowledgeRuleFile` (spec §7): re-reads the file at the row's CURRENT `md_path` and
    /// stores its fresh sha256 as `md_hash` — the owner's "yes, I've seen the external edit, this
    /// is now the accepted content" action (spec §11: "`ExternallyModified` → banner + [Accept]
    /// (rehash)"). Unknown `id` ⇒ `NotFound`. The file being missing is `Invariant("file
    /// missing")` (spec, task-8 brief) — the ROW is found, it's the FILE that's gone, and
    /// "acknowledge" a file that isn't there is a contradiction in terms, not silently ignored.
    /// Any OTHER read failure (permission denied, …) is `Io`, distinct from the missing-file case.
    /// A project-scoped ruleset row is a child of `project` (spec §5.2) — the archived-project
    /// guard fires right after the row is looked up (needed to know its `project_id`) but before
    /// the file is read or the row is written; a global-scoped ruleset (`project_id: None`) has
    /// no project, so no check.
    pub fn acknowledge_rule_file(&self, id: &str) -> Result<RuleSet, OrchdPersistError> {
        let tx = self.conn.unchecked_transaction()?;
        let row = load_ruleset_row_by_id(&tx, id)?;
        ensure_optional_project_active(&tx, row.project_id.as_deref())?;

        let content = match std::fs::read_to_string(&row.md_path) {
            Ok(c) => c,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                return Err(OrchdPersistError::Invariant("file missing".to_string()));
            }
            Err(e) => return Err(OrchdPersistError::Io(e.to_string())),
        };
        let hash = crate::ruleset_files::sha256_hex(&content);

        tx.execute(
            "UPDATE ruleset SET md_hash = ?2, updated_at = ?3 WHERE id = ?1",
            rusqlite::params![id, hash, now_ms()],
        )?;

        let ruleset = load_ruleset_by_id(&tx, id)?;
        tx.commit()?;
        Ok(ruleset)
    }
}

// ================================================================================
// ---- raw-insert helpers for import (T9, spec §8, D7): field-verbatim inserts used ONLY by
// `export::import_bundle`'s single transaction. Every value here comes from an already-parsed,
// already-typed bundle row and is written to the DB EXACTLY as given — ids, `created_at`,
// `updated_at`, `rank`, `ord`, `md_hash` included; nothing here generates an id, stamps
// `now_ms()`, or auto-creates a strategic goal/ruleset row the way the `Create*` verb methods
// above do (spec §8: "Import inserts raw rows in the tx (NOT the `CreateProject` verb) —
// §5.2 auto-creates never double-fire"). Each helper maps a PK/UNIQUE collision to
// `Conflict("<entity> <id> already exists")`; any other rusqlite error passes through as `Sql`
// unchanged. Callers run every insert inside ONE transaction with `defer_foreign_keys` on (see
// `export::import_project_bundles`), so these can run in whatever order the bundle's arrays are
// in — a bundle's `tasks[]`/`goals[]` are NOT guaranteed parent-before-child (e.g. `tasks[]` is
// sorted by `rank`, which can put a reparented/reranked subtask ahead of its parent).
// ================================================================================

/// Maps a raw-insert failure to `Conflict("<entity> <id> already exists")` (spec §8: "any id
/// already present in the store ⇒ `Conflict`") when it's a PK/UNIQUE collision, otherwise passes
/// the raw SQL error through unchanged. Shared by every `insert_*_raw` helper below.
fn conflict_or_sql(e: rusqlite::Error, entity: &str, id: &str) -> OrchdPersistError {
    if is_constraint_violation(&e) {
        OrchdPersistError::Conflict(format!("{entity} {id} already exists"))
    } else {
        OrchdPersistError::Sql(e)
    }
}

/// Raw-inserts a `project` row plus its `project_workspace` links, in the bundle's own
/// `workspace_ids` order (`ord` = array index — mirrors [`Db::create_project`]'s own `ord`
/// assignment, but here the ids/order come straight from the bundle, not freshly assigned). A
/// `workspace_id` already linked to ANY project ⇒ `Conflict` (reuses [`map_workspace_conflict`]'s
/// existing wording), same as the live verb.
pub(crate) fn insert_project_raw(
    tx: &rusqlite::Transaction,
    p: &Project,
) -> Result<(), OrchdPersistError> {
    tx.execute(
        "INSERT INTO project (id, name, description, status, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            p.id,
            p.name,
            p.description,
            encode_project_status(&p.status),
            p.created_at,
            p.updated_at
        ],
    )
    .map_err(|e| conflict_or_sql(e, "project", &p.id))?;
    for (ord, workspace_id) in p.workspace_ids.iter().enumerate() {
        tx.execute(
            "INSERT INTO project_workspace (project_id, workspace_id, ord) VALUES (?1, ?2, ?3)",
            rusqlite::params![p.id, workspace_id, ord as i64],
        )
        .map_err(|e| map_workspace_conflict(e, workspace_id))?;
    }
    Ok(())
}

/// Raw-inserts one `goal` row.
pub(crate) fn insert_goal_raw(
    tx: &rusqlite::Transaction,
    g: &Goal,
) -> Result<(), OrchdPersistError> {
    let metric_refs = serde_json::to_string(&g.metric_refs)
        .map_err(|e| OrchdPersistError::Io(format!("failed to serialize goal.metric_refs: {e}")))?;
    tx.execute(
        "INSERT INTO goal
           (id, project_id, parent_id, kind, title, body, ord, status, metric_refs,
            created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        rusqlite::params![
            g.id,
            g.project_id,
            g.parent_id,
            encode_goal_kind(&g.kind),
            g.title,
            g.body,
            g.ord,
            encode_goal_status(&g.status),
            metric_refs,
            g.created_at,
            g.updated_at
        ],
    )
    .map_err(|e| conflict_or_sql(e, "goal", &g.id))?;
    Ok(())
}

/// Raw-inserts one `idea` row.
pub(crate) fn insert_idea_raw(
    tx: &rusqlite::Transaction,
    i: &Idea,
) -> Result<(), OrchdPersistError> {
    tx.execute(
        "INSERT INTO idea (id, project_id, title, body, lifecycle, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![
            i.id,
            i.project_id,
            i.title,
            i.body,
            encode_idea_lifecycle(&i.lifecycle),
            i.created_at,
            i.updated_at
        ],
    )
    .map_err(|e| conflict_or_sql(e, "idea", &i.id))?;
    Ok(())
}

/// Raw-inserts one `insight` row.
pub(crate) fn insert_insight_raw(
    tx: &rusqlite::Transaction,
    ins: &Insight,
) -> Result<(), OrchdPersistError> {
    let fit_verdict = ins.fit_verdict.as_ref().map(encode_fit_verdict);
    tx.execute(
        "INSERT INTO insight
           (id, project_id, source, title, body, fit_verdict, fit_reasoning, status,
            resolution_reasoning, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
        rusqlite::params![
            ins.id,
            ins.project_id,
            ins.source,
            ins.title,
            ins.body,
            fit_verdict,
            ins.fit_reasoning,
            encode_insight_status(&ins.status),
            ins.resolution_reasoning,
            ins.created_at,
            ins.updated_at
        ],
    )
    .map_err(|e| conflict_or_sql(e, "insight", &ins.id))?;
    Ok(())
}

/// Raw-inserts one `task` row.
pub(crate) fn insert_task_raw(
    tx: &rusqlite::Transaction,
    t: &DomainTask,
) -> Result<(), OrchdPersistError> {
    let tags = serde_json::to_string(&t.tags)
        .map_err(|e| OrchdPersistError::Io(format!("failed to serialize task.tags: {e}")))?;
    tx.execute(
        "INSERT INTO task
           (id, project_id, parent_id, title, body, status, priority, source, source_id, tags,
            rank, rank_agent, rank_agent_reasoning, created_at, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        rusqlite::params![
            t.id,
            t.project_id,
            t.parent_id,
            t.title,
            t.body,
            encode_task_status(&t.status),
            // A pre-priority bundle's task decoded `priority` as `Normal` via the wire type's
            // `#[serde(default)]` (SCN-051 back-compat), so this write is always well-formed.
            encode_task_priority(&t.priority),
            encode_task_source(&t.source),
            t.source_id,
            tags,
            t.rank,
            t.rank_agent,
            t.rank_agent_reasoning,
            t.created_at,
            t.updated_at
        ],
    )
    .map_err(|e| conflict_or_sql(e, "task", &t.id))?;
    Ok(())
}

/// Inserts (or, for the GLOBAL scope, RECONCILES) one `ruleset` row. The caller
/// (`export::import_ruleset`) has already resolved `r.md_path` to its FINAL on-disk location
/// (verbatim if under `app_support`, repointed to the scope's default app-support path otherwise)
/// and written any `mdContent` there BEFORE calling this — this function only ever writes the row,
/// never touches the filesystem itself. `md_hash` is written verbatim from `r.md_hash` (never
/// recomputed here), per D7 field-verbatim preservation.
///
/// **Scope split (spec §8 whole-store restore):**
/// - `scope='global'` is a boot-seeded SINGLETON — `boot::ensure_global_ruleset` pre-creates
///   exactly one such row at EVERY daemon boot, guarded by the `ruleset_single_global` partial
///   unique index. A blind `INSERT` of a bundle's own global row would therefore ALWAYS collide
///   on a real (booted) daemon, rolling back the entire one-tx import and losing every project.
///   So global is RECONCILED: `ON CONFLICT(scope) WHERE scope='global' DO UPDATE` overwrites the
///   existing seeded row's `md_path`/`md_hash`/`policy`/`updated_at` with the bundle's, keeping
///   the seeded row's `id`/`created_at` (both boot impl details, not meaningful data). Into a
///   TRULY empty store (no seeded row — e.g. the spec §8 "import into an empty store" DoD) the
///   same statement just INSERTs the bundle's row verbatim, so the round-trip guarantee holds.
/// - `scope='project'` keeps the strict field-verbatim `INSERT`: a project's ruleset row is
///   1:1 with its (globally-unique) project, so a real collision there means the project already
///   exists ⇒ `Conflict` + full rollback, which is correct.
pub(crate) fn insert_ruleset_raw(
    tx: &rusqlite::Transaction,
    r: &RuleSet,
) -> Result<(), OrchdPersistError> {
    let policy = serde_json::to_string(&r.policy)
        .map_err(|e| OrchdPersistError::Io(format!("failed to serialize ruleset.policy: {e}")))?;
    match r.scope {
        RuleScope::Global => {
            // Reconcile the boot-seeded singleton (or insert verbatim if the store is empty).
            tx.execute(
                "INSERT INTO ruleset
                   (id, scope, project_id, md_path, md_hash, policy, created_at, updated_at)
                 VALUES (?1, 'global', NULL, ?2, ?3, ?4, ?5, ?6)
                 ON CONFLICT(scope) WHERE scope = 'global' DO UPDATE SET
                   md_path = excluded.md_path,
                   md_hash = excluded.md_hash,
                   policy = excluded.policy,
                   updated_at = excluded.updated_at",
                rusqlite::params![
                    r.id,
                    r.md_path,
                    r.md_hash,
                    policy,
                    r.created_at,
                    r.updated_at
                ],
            )
            .map_err(|e| conflict_or_sql(e, "ruleset", &r.id))?;
        }
        RuleScope::Project => {
            tx.execute(
                "INSERT INTO ruleset
                   (id, scope, project_id, md_path, md_hash, policy, created_at, updated_at)
                 VALUES (?1, 'project', ?2, ?3, ?4, ?5, ?6, ?7)",
                rusqlite::params![
                    r.id,
                    r.project_id,
                    r.md_path,
                    r.md_hash,
                    policy,
                    r.created_at,
                    r.updated_at
                ],
            )
            .map_err(|e| conflict_or_sql(e, "ruleset", &r.id))?;
        }
    }
    Ok(())
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
        // S4 spec §4 (schema v2, additive): knowledge-graph tables.
        "graph_node",
        "graph_edge",
        // S-EXT spec §4 (schema v3, additive): MCP/connectors/skills/trust tables.
        "mcp_server",
        "mcp_tool",
        "account",
        "mcp_invocation",
        "mcp_artifact",
        "skill",
        "consent_grant",
        "policy",
        "audit_log",
        // S-IDEA spec §4 (schema v4, additive): research-run provenance link.
        "research_run",
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
    fn open_in_memory_creates_schema_v5_with_every_table() {
        let db = Db::open_in_memory().unwrap();
        // SCN-051 bumped SCHEMA_VERSION 4->5 (additive, the `task.priority` column only).
        assert_eq!(user_version(db.conn()), 5);
        for table in TABLES {
            assert!(table_exists(db.conn(), table), "missing table {table}");
        }
    }

    #[test]
    fn open_on_disk_creates_schema_v5_with_every_table() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("orchd.db");
        let db = Db::open(&path).unwrap();
        assert_eq!(user_version(db.conn()), 5);
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
        assert_eq!(user_version(db.conn()), 5);

        let found = std::fs::read_dir(dir.path()).unwrap().any(|e| {
            e.unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with("orchd.db.corrupt-")
        });
        assert!(found, "expected an orchd.db.corrupt-<ts> quarantine file");
    }

    #[test]
    fn open_with_outcome_reports_recovery_on_a_corrupt_db() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("orchd.db");
        std::fs::write(&path, b"not a sqlite database").unwrap();

        let (db, outcome) =
            Db::open_with_outcome(&path).expect("open must quarantine and recreate, not error");
        assert_eq!(user_version(db.conn()), 5);
        match outcome {
            DbOpenOutcome::RecoveredFromCorruption { quarantined_to } => {
                assert!(
                    quarantined_to.exists(),
                    "the quarantined corrupt image must remain on disk at {}",
                    quarantined_to.display()
                );
                assert!(quarantined_to
                    .file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with("orchd.db.corrupt-"));
            }
            other => panic!("expected RecoveredFromCorruption, got {other:?}"),
        }
    }

    #[test]
    fn open_with_outcome_reports_clean_on_a_fresh_db() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("orchd.db");
        let (_db, outcome) = Db::open_with_outcome(&path).unwrap();
        assert_eq!(outcome, DbOpenOutcome::Clean);
    }

    // ---- v4 -> v5 migration (SCN-051 task priority, REAL v4 fixture) ----

    /// Builds a REAL schema-v4 database (apply `migrate_v1`..`migrate_v4` alone — the exact
    /// pre-priority on-disk shape), inserts a pre-priority task row, THEN applies [`migrate_v5`]
    /// — proving both the new `task.priority` column's shape and that every EXISTING task
    /// backfills to `'normal'` via the column DEFAULT (mirrors `crate::graph`'s v1→v2
    /// backfill-test approach).
    #[test]
    fn v4_fixture_migrates_to_v5_and_backfills_existing_tasks_to_normal() {
        let conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", "ON").unwrap();
        let v4_steps: &[bpa_daemon_core::migrate::Migration] = &[
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
            bpa_daemon_core::migrate::Migration {
                upto: 4,
                apply: migrate_v4,
            },
        ];
        bpa_daemon_core::migrate::run_migrations(&conn, 0, 4, v4_steps).unwrap();
        let has_priority: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM pragma_table_info('task') WHERE name = 'priority'",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(
            has_priority, 0,
            "the v4 fixture must NOT have task.priority yet"
        );

        let now = 1_700_000_000_000i64;
        conn.execute(
            "INSERT INTO project (id, name, description, status, created_at, updated_at)
             VALUES ('p1', 'Acme', '', 'active', ?1, ?1)",
            rusqlite::params![now],
        )
        .unwrap();
        conn.execute(
            "INSERT INTO task (id, project_id, parent_id, title, body, status, source,
                               source_id, tags, rank, rank_agent, rank_agent_reasoning,
                               created_at, updated_at)
             VALUES ('t1', 'p1', NULL, 'legacy', '', 'backlog', 'plan',
                     NULL, '[]', 1024.0, NULL, '', ?1, ?1)",
            rusqlite::params![now],
        )
        .unwrap();

        let v5_steps: &[bpa_daemon_core::migrate::Migration] =
            &[bpa_daemon_core::migrate::Migration {
                upto: 5,
                apply: migrate_v5,
            }];
        bpa_daemon_core::migrate::run_migrations(&conn, 4, 5, v5_steps).unwrap();

        assert_eq!(user_version(&conn), 5);
        let priority: String = conn
            .query_row("SELECT priority FROM task WHERE id = 't1'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(
            priority, "normal",
            "a pre-v5 task must backfill to 'normal' via the column DEFAULT"
        );
        // The CHECK constraint must reject anything outside the locked urgent/normal pair.
        let err = conn
            .execute("UPDATE task SET priority = 'blocker' WHERE id = 't1'", [])
            .unwrap_err();
        assert!(matches!(err, rusqlite::Error::SqliteFailure(_, _)));
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
        assert_eq!(goals[0].title, "Strategic goal");
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
    fn unarchive_project_sets_status_active() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        db.archive_project(&project.id).unwrap();
        let restored = db.unarchive_project(&project.id).unwrap();
        assert_eq!(restored.status, ProjectStatus::Active);
        // Mutations work again once un-archived (proves the guard actually cleared).
        db.update_project(&project.id, Some("A2"), None).unwrap();
    }

    #[test]
    fn unarchive_project_unknown_id_is_not_found() {
        let db = Db::open_in_memory().unwrap();
        let err = db.unarchive_project("nope").unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound));
    }

    #[test]
    fn unarchive_project_already_active_is_invariant() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let err = db.unarchive_project(&project.id).unwrap_err();
        match err {
            OrchdPersistError::Invariant(m) => assert_eq!(m, "project is not archived"),
            other => panic!("expected Invariant(\"project is not archived\"), got {other:?}"),
        }
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

    // ================================================================================
    // ---- idea / insight / task CRUD (T7), every invariant from spec §5.2's table, TDD
    // (written RED before `impl Db { create_idea, ... }` above went GREEN) ----
    // ================================================================================

    fn idea_lifecycle_raw(db: &Db, id: &str) -> String {
        db.conn()
            .query_row(
                "SELECT lifecycle FROM idea WHERE id = ?1",
                rusqlite::params![id],
                |r| r.get(0),
            )
            .unwrap()
    }

    fn insight_fit_verdict_raw(db: &Db, id: &str) -> Option<String> {
        db.conn()
            .query_row(
                "SELECT fit_verdict FROM insight WHERE id = ?1",
                rusqlite::params![id],
                |r| r.get(0),
            )
            .unwrap()
    }

    fn insight_fit_reasoning_raw(db: &Db, id: &str) -> String {
        db.conn()
            .query_row(
                "SELECT fit_reasoning FROM insight WHERE id = ?1",
                rusqlite::params![id],
                |r| r.get(0),
            )
            .unwrap()
    }

    fn task_rank_raw(db: &Db, id: &str) -> f64 {
        db.conn()
            .query_row(
                "SELECT rank FROM task WHERE id = ?1",
                rusqlite::params![id],
                |r| r.get(0),
            )
            .unwrap()
    }

    // ---- create_idea / list_ideas ----

    #[test]
    fn create_idea_defaults_lifecycle_captured_orphan_by_default() {
        let db = Db::open_in_memory().unwrap();
        let idea = db.create_idea(None, "t", "b").unwrap();
        assert!(uuid::Uuid::parse_str(&idea.id).is_ok(), "id must be a uuid");
        assert!(idea.project_id.is_none());
        assert_eq!(idea.title, "t");
        assert_eq!(idea.body, "b");
        assert_eq!(idea.lifecycle, IdeaLifecycle::Captured);
        assert_eq!(idea.created_at, idea.updated_at);
        assert_eq!(idea_lifecycle_raw(&db, &idea.id), "captured");
    }

    #[test]
    fn create_idea_attached_to_project() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let idea = db.create_idea(Some(&project.id), "t", "").unwrap();
        assert_eq!(idea.project_id.as_deref(), Some(project.id.as_str()));
    }

    #[test]
    fn create_idea_unknown_project_is_not_found() {
        let db = Db::open_in_memory().unwrap();
        let err = db.create_idea(Some("nope"), "t", "").unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound));
    }

    #[test]
    fn archived_project_blocks_create_idea() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        db.archive_project(&project.id).unwrap();
        let err = db.create_idea(Some(&project.id), "t", "").unwrap_err();
        assert!(
            matches!(&err, OrchdPersistError::Invariant(m) if m == "project archived"),
            "got {err:?}"
        );
    }

    #[test]
    fn list_ideas_none_includes_orphans_but_project_filter_excludes_them() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let orphan = db.create_idea(None, "orphan", "").unwrap();
        let attached = db.create_idea(Some(&project.id), "attached", "").unwrap();

        let all = db.list_ideas(None).unwrap();
        let all_ids: Vec<&str> = all.iter().map(|i| i.id.as_str()).collect();
        assert!(all_ids.contains(&orphan.id.as_str()));
        assert!(all_ids.contains(&attached.id.as_str()));

        let scoped = db.list_ideas(Some(&project.id)).unwrap();
        let scoped_ids: Vec<&str> = scoped.iter().map(|i| i.id.as_str()).collect();
        assert!(
            !scoped_ids.contains(&orphan.id.as_str()),
            "orphan idea must not appear in a project-scoped list"
        );
        assert!(scoped_ids.contains(&attached.id.as_str()));
    }

    #[test]
    fn list_ideas_orders_created_at_desc() {
        let db = Db::open_in_memory().unwrap();
        let older = db.create_idea(None, "older", "").unwrap();
        let newer = db.create_idea(None, "newer", "").unwrap();
        // force distinct timestamps regardless of clock resolution, so DESC order is
        // unambiguous.
        db.conn()
            .execute(
                "UPDATE idea SET created_at = 1000 WHERE id = ?1",
                rusqlite::params![older.id],
            )
            .unwrap();
        db.conn()
            .execute(
                "UPDATE idea SET created_at = 2000 WHERE id = ?1",
                rusqlite::params![newer.id],
            )
            .unwrap();

        let all = db.list_ideas(None).unwrap();
        assert_eq!(all[0].id, newer.id, "newest idea must sort first");
        assert_eq!(all[1].id, older.id);
    }

    // ---- update_idea ----

    #[test]
    fn update_idea_changes_only_provided_fields() {
        let db = Db::open_in_memory().unwrap();
        let idea = db.create_idea(None, "t0", "b0").unwrap();
        let updated = db.update_idea(&idea.id, Some("t1"), None).unwrap();
        assert_eq!(updated.title, "t1");
        assert_eq!(updated.body, "b0", "body left untouched when None");
    }

    #[test]
    fn update_idea_unknown_id_is_not_found() {
        let db = Db::open_in_memory().unwrap();
        let err = db.update_idea("nope", Some("x"), None).unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound));
    }

    #[test]
    fn archived_project_blocks_update_idea() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let idea = db.create_idea(Some(&project.id), "t", "").unwrap();
        db.archive_project(&project.id).unwrap();
        let err = db.update_idea(&idea.id, Some("x"), None).unwrap_err();
        assert!(
            matches!(&err, OrchdPersistError::Invariant(m) if m == "project archived"),
            "got {err:?}"
        );
    }

    #[test]
    fn orphan_idea_remains_mutable_with_no_project() {
        // An orphan idea has no project to be archived, so every mutator on it must succeed
        // unconditionally — proves ensure_optional_project_active's None branch is a true no-op.
        let db = Db::open_in_memory().unwrap();
        let idea = db.create_idea(None, "t", "b").unwrap();

        let updated = db.update_idea(&idea.id, Some("t2"), None).unwrap();
        assert_eq!(updated.title, "t2");
        let relifecycled = db
            .set_idea_lifecycle(&idea.id, IdeaLifecycle::Researching)
            .unwrap();
        assert_eq!(relifecycled.lifecycle, IdeaLifecycle::Researching);
        db.delete_idea(&idea.id).unwrap();
        assert!(db.list_ideas(None).unwrap().is_empty());
    }

    // ---- set_idea_project (D11) ----

    #[test]
    fn set_idea_project_none_detaches() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let idea = db.create_idea(Some(&project.id), "t", "").unwrap();

        let detached = db.set_idea_project(&idea.id, None).unwrap();
        assert!(detached.project_id.is_none());

        // orphan now: appears in the None-scoped list but not the project-scoped one.
        assert!(db
            .list_ideas(Some(&project.id))
            .unwrap()
            .into_iter()
            .all(|i| i.id != idea.id));
    }

    #[test]
    fn set_idea_project_attaches_orphan_to_a_project() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let idea = db.create_idea(None, "t", "").unwrap();

        let attached = db.set_idea_project(&idea.id, Some(&project.id)).unwrap();
        assert_eq!(attached.project_id.as_deref(), Some(project.id.as_str()));
    }

    #[test]
    fn set_idea_project_unknown_target_is_not_found() {
        let db = Db::open_in_memory().unwrap();
        let idea = db.create_idea(None, "t", "").unwrap();
        let err = db.set_idea_project(&idea.id, Some("nope")).unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound));
    }

    #[test]
    fn set_idea_project_unknown_id_is_not_found() {
        let db = Db::open_in_memory().unwrap();
        let err = db.set_idea_project("nope", None).unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound));
    }

    #[test]
    fn set_idea_project_blocked_when_target_project_archived() {
        let db = Db::open_in_memory().unwrap();
        let idea = db.create_idea(None, "t", "").unwrap();
        let target = db.create_project("A", "", &ids(&["w1"])).unwrap();
        db.archive_project(&target.id).unwrap();

        let err = db.set_idea_project(&idea.id, Some(&target.id)).unwrap_err();
        assert!(
            matches!(&err, OrchdPersistError::Invariant(m) if m == "project archived"),
            "got {err:?}"
        );
    }

    #[test]
    fn set_idea_project_blocked_when_current_project_archived() {
        let db = Db::open_in_memory().unwrap();
        let source = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let idea = db.create_idea(Some(&source.id), "t", "").unwrap();
        db.archive_project(&source.id).unwrap();

        let err = db.set_idea_project(&idea.id, None).unwrap_err();
        assert!(
            matches!(&err, OrchdPersistError::Invariant(m) if m == "project archived"),
            "got {err:?}"
        );
    }

    // ---- set_idea_lifecycle ----

    #[test]
    fn set_idea_lifecycle_persists_snake_case_db_literal() {
        // The wire enum tag is camelCase ("inDev"); the DB CHECK-constraint literal (spec §5.1)
        // is snake_case ("in_dev") — this pins the persistence layer's OWN mapping, independent
        // of serde.
        let db = Db::open_in_memory().unwrap();
        let idea = db.create_idea(None, "t", "").unwrap();

        let updated = db
            .set_idea_lifecycle(&idea.id, IdeaLifecycle::InDev)
            .unwrap();
        assert_eq!(updated.lifecycle, IdeaLifecycle::InDev);
        assert_eq!(idea_lifecycle_raw(&db, &idea.id), "in_dev");
    }

    #[test]
    fn set_idea_lifecycle_unknown_id_is_not_found() {
        let db = Db::open_in_memory().unwrap();
        let err = db
            .set_idea_lifecycle("nope", IdeaLifecycle::Shipped)
            .unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound));
    }

    #[test]
    fn archived_project_blocks_set_idea_lifecycle() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let idea = db.create_idea(Some(&project.id), "t", "").unwrap();
        db.archive_project(&project.id).unwrap();
        let err = db
            .set_idea_lifecycle(&idea.id, IdeaLifecycle::Archived)
            .unwrap_err();
        assert!(matches!(err, OrchdPersistError::Invariant(_)));
    }

    // ---- delete_idea ----

    #[test]
    fn delete_idea_removes_row() {
        let db = Db::open_in_memory().unwrap();
        let idea = db.create_idea(None, "t", "").unwrap();
        db.delete_idea(&idea.id).unwrap();
        assert!(db.list_ideas(None).unwrap().is_empty());
    }

    #[test]
    fn delete_idea_unknown_id_is_not_found() {
        let db = Db::open_in_memory().unwrap();
        let err = db.delete_idea("nope").unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound));
    }

    #[test]
    fn archived_project_blocks_delete_idea() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let idea = db.create_idea(Some(&project.id), "t", "").unwrap();
        db.archive_project(&project.id).unwrap();
        let err = db.delete_idea(&idea.id).unwrap_err();
        assert!(matches!(err, OrchdPersistError::Invariant(_)));
        assert_eq!(db.list_ideas(None).unwrap().len(), 1, "delete must not run");
    }

    // ---- create_insight / list_insights ----

    #[test]
    fn create_insight_defaults_status_new_orphan_by_default() {
        let db = Db::open_in_memory().unwrap();
        let insight = db.create_insight(None, "src", "t", "b").unwrap();
        assert!(insight.project_id.is_none());
        assert_eq!(insight.source, "src");
        assert_eq!(insight.title, "t");
        assert_eq!(insight.body, "b");
        assert_eq!(insight.status, InsightStatus::New);
        assert!(insight.fit_verdict.is_none());
        assert_eq!(insight.fit_reasoning, "");
        assert_eq!(insight.resolution_reasoning, "");
    }

    #[test]
    fn create_insight_unknown_project_is_not_found() {
        let db = Db::open_in_memory().unwrap();
        let err = db.create_insight(Some("nope"), "s", "t", "").unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound));
    }

    #[test]
    fn archived_project_blocks_create_insight() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        db.archive_project(&project.id).unwrap();
        let err = db
            .create_insight(Some(&project.id), "s", "t", "")
            .unwrap_err();
        assert!(matches!(err, OrchdPersistError::Invariant(_)));
    }

    #[test]
    fn list_insights_none_includes_orphans_but_project_filter_excludes_them() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let orphan = db.create_insight(None, "s", "orphan", "").unwrap();
        let attached = db
            .create_insight(Some(&project.id), "s", "attached", "")
            .unwrap();

        let all_ids: Vec<String> = db
            .list_insights(None)
            .unwrap()
            .into_iter()
            .map(|i| i.id)
            .collect();
        assert!(all_ids.contains(&orphan.id));
        assert!(all_ids.contains(&attached.id));

        let scoped_ids: Vec<String> = db
            .list_insights(Some(&project.id))
            .unwrap()
            .into_iter()
            .map(|i| i.id)
            .collect();
        assert!(!scoped_ids.contains(&orphan.id));
        assert!(scoped_ids.contains(&attached.id));
    }

    // ---- update_insight ----

    #[test]
    fn update_insight_changes_only_provided_fields() {
        let db = Db::open_in_memory().unwrap();
        let insight = db.create_insight(None, "s", "t0", "b0").unwrap();
        let updated = db.update_insight(&insight.id, None, Some("b1")).unwrap();
        assert_eq!(updated.title, "t0", "title left untouched when None");
        assert_eq!(updated.body, "b1");
    }

    #[test]
    fn update_insight_unknown_id_is_not_found() {
        let db = Db::open_in_memory().unwrap();
        let err = db.update_insight("nope", Some("x"), None).unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound));
    }

    #[test]
    fn archived_project_blocks_update_insight() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let insight = db.create_insight(Some(&project.id), "s", "t", "").unwrap();
        db.archive_project(&project.id).unwrap();
        let err = db.update_insight(&insight.id, Some("x"), None).unwrap_err();
        assert!(matches!(err, OrchdPersistError::Invariant(_)));
    }

    #[test]
    fn orphan_insight_remains_mutable_with_no_project() {
        let db = Db::open_in_memory().unwrap();
        let insight = db.create_insight(None, "s", "t", "").unwrap();
        let updated = db.update_insight(&insight.id, Some("t2"), None).unwrap();
        assert_eq!(updated.title, "t2");
        db.delete_insight(&insight.id).unwrap();
        assert!(db.list_insights(None).unwrap().is_empty());
    }

    // ---- set_insight_fit_verdict (D11) ----

    #[test]
    fn set_insight_fit_verdict_stores_verdict_and_reasoning() {
        let db = Db::open_in_memory().unwrap();
        let insight = db.create_insight(None, "s", "t", "").unwrap();

        let updated = db
            .set_insight_fit_verdict(
                &insight.id,
                Some(FitVerdict::NoFit),
                "doesn't fit because X",
            )
            .unwrap();
        assert_eq!(updated.fit_verdict, Some(FitVerdict::NoFit));
        assert_eq!(updated.fit_reasoning, "doesn't fit because X");
        // DB CHECK-constraint literal is snake_case ("no_fit"), distinct from the wire tag
        // ("noFit").
        assert_eq!(
            insight_fit_verdict_raw(&db, &insight.id),
            Some("no_fit".to_string())
        );
        assert_eq!(
            insight_fit_reasoning_raw(&db, &insight.id),
            "doesn't fit because X"
        );
    }

    #[test]
    fn set_insight_fit_verdict_none_clears_verdict_but_keeps_reasoning() {
        let db = Db::open_in_memory().unwrap();
        let insight = db.create_insight(None, "s", "t", "").unwrap();
        db.set_insight_fit_verdict(&insight.id, Some(FitVerdict::Fit), "fits")
            .unwrap();

        let cleared = db
            .set_insight_fit_verdict(&insight.id, None, "undecided again")
            .unwrap();
        assert!(cleared.fit_verdict.is_none());
        assert_eq!(cleared.fit_reasoning, "undecided again");
        assert_eq!(insight_fit_verdict_raw(&db, &insight.id), None);
    }

    #[test]
    fn set_insight_fit_verdict_unknown_id_is_not_found() {
        let db = Db::open_in_memory().unwrap();
        let err = db
            .set_insight_fit_verdict("nope", Some(FitVerdict::Fit), "")
            .unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound));
    }

    #[test]
    fn archived_project_blocks_set_insight_fit_verdict() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let insight = db.create_insight(Some(&project.id), "s", "t", "").unwrap();
        db.archive_project(&project.id).unwrap();
        let err = db
            .set_insight_fit_verdict(&insight.id, Some(FitVerdict::Fit), "x")
            .unwrap_err();
        assert!(matches!(err, OrchdPersistError::Invariant(_)));
    }

    // ---- set_insight_status ----

    #[test]
    fn set_insight_status_updates_status_and_resolution_reasoning() {
        let db = Db::open_in_memory().unwrap();
        let insight = db.create_insight(None, "s", "t", "").unwrap();

        let updated = db
            .set_insight_status(&insight.id, InsightStatus::Accepted, Some("looks good"))
            .unwrap();
        assert_eq!(updated.status, InsightStatus::Accepted);
        assert_eq!(updated.resolution_reasoning, "looks good");
    }

    #[test]
    fn set_insight_status_none_reasoning_leaves_it_unchanged() {
        let db = Db::open_in_memory().unwrap();
        let insight = db.create_insight(None, "s", "t", "").unwrap();
        db.set_insight_status(&insight.id, InsightStatus::Accepted, Some("kept"))
            .unwrap();

        let updated = db
            .set_insight_status(&insight.id, InsightStatus::Archived, None)
            .unwrap();
        assert_eq!(updated.status, InsightStatus::Archived);
        assert_eq!(
            updated.resolution_reasoning, "kept",
            "None must leave resolution_reasoning unchanged (D11: non-nullable column)"
        );
    }

    #[test]
    fn set_insight_status_unknown_id_is_not_found() {
        let db = Db::open_in_memory().unwrap();
        let err = db
            .set_insight_status("nope", InsightStatus::Accepted, None)
            .unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound));
    }

    #[test]
    fn archived_project_blocks_set_insight_status() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let insight = db.create_insight(Some(&project.id), "s", "t", "").unwrap();
        db.archive_project(&project.id).unwrap();
        let err = db
            .set_insight_status(&insight.id, InsightStatus::Accepted, None)
            .unwrap_err();
        assert!(matches!(err, OrchdPersistError::Invariant(_)));
    }

    // ---- delete_insight ----

    #[test]
    fn delete_insight_removes_row() {
        let db = Db::open_in_memory().unwrap();
        let insight = db.create_insight(None, "s", "t", "").unwrap();
        db.delete_insight(&insight.id).unwrap();
        assert!(db.list_insights(None).unwrap().is_empty());
    }

    #[test]
    fn delete_insight_unknown_id_is_not_found() {
        let db = Db::open_in_memory().unwrap();
        let err = db.delete_insight("nope").unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound));
    }

    #[test]
    fn archived_project_blocks_delete_insight() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let insight = db.create_insight(Some(&project.id), "s", "t", "").unwrap();
        db.archive_project(&project.id).unwrap();
        let err = db.delete_insight(&insight.id).unwrap_err();
        assert!(matches!(err, OrchdPersistError::Invariant(_)));
    }

    // ---- create_task / rank math ----

    #[test]
    fn create_task_rank_sequence_1024_2048_3072() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();

        let t1 = db
            .create_task(
                &project.id,
                None,
                "t1",
                "",
                None,
                TaskSource::Plan,
                None,
                &[],
                None,
            )
            .unwrap();
        let t2 = db
            .create_task(
                &project.id,
                None,
                "t2",
                "",
                None,
                TaskSource::Plan,
                None,
                &[],
                None,
            )
            .unwrap();
        let t3 = db
            .create_task(
                &project.id,
                None,
                "t3",
                "",
                None,
                TaskSource::Plan,
                None,
                &[],
                None,
            )
            .unwrap();

        assert_eq!(t1.rank, 1024.0, "first task in a project must rank 1024");
        assert_eq!(t2.rank, 2048.0);
        assert_eq!(t3.rank, 3072.0);
    }

    #[test]
    fn create_task_rank_is_scoped_per_project() {
        let db = Db::open_in_memory().unwrap();
        let a = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let b = db.create_project("B", "", &ids(&["w2"])).unwrap();
        db.create_task(
            &a.id,
            None,
            "a1",
            "",
            None,
            TaskSource::Plan,
            None,
            &[],
            None,
        )
        .unwrap();

        // b's first task must still rank 1024, unaffected by a's existing task.
        let b1 = db
            .create_task(
                &b.id,
                None,
                "b1",
                "",
                None,
                TaskSource::Plan,
                None,
                &[],
                None,
            )
            .unwrap();
        assert_eq!(b1.rank, 1024.0);
    }

    #[test]
    fn create_task_defaults_status_backlog_and_round_trips_source_and_tags() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let task = db
            .create_task(
                &project.id,
                None,
                "t",
                "b",
                None,
                TaskSource::Idea,
                Some("idea-1"),
                &ids(&["urgent", "backend"]),
                None,
            )
            .unwrap();

        assert_eq!(task.status, TaskStatus::Backlog);
        assert_eq!(task.source, TaskSource::Idea);
        assert_eq!(task.source_id.as_deref(), Some("idea-1"));
        assert_eq!(task.tags, ids(&["urgent", "backend"]));
        assert!(task.rank_agent.is_none());
        assert_eq!(task.rank_agent_reasoning, "");

        // re-fetch independently to prove the tags JSON round-tripped through SQLite.
        let refetched = db
            .list_tasks(Some(&project.id))
            .unwrap()
            .into_iter()
            .find(|t| t.id == task.id)
            .unwrap();
        assert_eq!(refetched.tags, ids(&["urgent", "backend"]));
    }

    #[test]
    fn create_task_explicit_status_is_honored() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let task = db
            .create_task(
                &project.id,
                None,
                "t",
                "",
                Some(TaskStatus::Waiting),
                TaskSource::Bug,
                None,
                &[],
                None,
            )
            .unwrap();
        assert_eq!(task.status, TaskStatus::Waiting);
    }

    #[test]
    fn create_task_unknown_project_is_not_found() {
        let db = Db::open_in_memory().unwrap();
        let err = db
            .create_task(
                "nope",
                None,
                "t",
                "",
                None,
                TaskSource::Plan,
                None,
                &[],
                None,
            )
            .unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound));
    }

    #[test]
    fn create_task_unknown_parent_is_not_found() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let err = db
            .create_task(
                &project.id,
                Some("nope"),
                "t",
                "",
                None,
                TaskSource::Plan,
                None,
                &[],
                None,
            )
            .unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound));
    }

    #[test]
    fn create_task_cross_project_parent_is_invariant() {
        let db = Db::open_in_memory().unwrap();
        let a = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let b = db.create_project("B", "", &ids(&["w2"])).unwrap();
        let a_task = db
            .create_task(
                &a.id,
                None,
                "a1",
                "",
                None,
                TaskSource::Plan,
                None,
                &[],
                None,
            )
            .unwrap();

        let err = db
            .create_task(
                &b.id,
                Some(&a_task.id),
                "b1",
                "",
                None,
                TaskSource::Plan,
                None,
                &[],
                None,
            )
            .unwrap_err();
        assert!(
            matches!(&err, OrchdPersistError::Invariant(m) if m == "task parent_id must belong to the same project"),
            "got {err:?}"
        );
    }

    #[test]
    fn create_task_parent_makes_a_valid_subtask() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let parent = db
            .create_task(
                &project.id,
                None,
                "parent",
                "",
                None,
                TaskSource::Plan,
                None,
                &[],
                None,
            )
            .unwrap();
        let child = db
            .create_task(
                &project.id,
                Some(&parent.id),
                "child",
                "",
                None,
                TaskSource::Plan,
                None,
                &[],
                None,
            )
            .unwrap();
        assert_eq!(child.parent_id.as_deref(), Some(parent.id.as_str()));
    }

    #[test]
    fn archived_project_blocks_create_task() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        db.archive_project(&project.id).unwrap();
        let err = db
            .create_task(
                &project.id,
                None,
                "t",
                "",
                None,
                TaskSource::Plan,
                None,
                &[],
                None,
            )
            .unwrap_err();
        assert!(
            matches!(&err, OrchdPersistError::Invariant(m) if m == "project archived"),
            "got {err:?}"
        );
    }

    #[test]
    fn list_tasks_still_works_on_archived_project() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        db.create_task(
            &project.id,
            None,
            "t",
            "",
            None,
            TaskSource::Plan,
            None,
            &[],
            None,
        )
        .unwrap();
        db.archive_project(&project.id).unwrap();
        let tasks = db.list_tasks(Some(&project.id)).unwrap();
        assert_eq!(tasks.len(), 1, "reads still work on an archived project");
    }

    #[test]
    fn task_ancestor_chain_contains_detects_direct_and_transitive_cycle() {
        // Tasks have no reparent verb in T7 (only create_task ever sets parent_id), so the
        // walk-up cycle-guard branch inside create_task can never actually trigger through the
        // public API — a brand-new task cannot be its own ancestor before it exists. This test
        // exercises the reusable walk-up helper directly instead (same shape as goals'
        // `ancestor_chain_contains`), which is what create_task's defensive check — and any
        // future reparent verb — relies on.
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let a = db
            .create_task(
                &project.id,
                None,
                "a",
                "",
                None,
                TaskSource::Plan,
                None,
                &[],
                None,
            )
            .unwrap();
        let b = db
            .create_task(
                &project.id,
                Some(&a.id),
                "b",
                "",
                None,
                TaskSource::Plan,
                None,
                &[],
                None,
            )
            .unwrap();
        let c = db
            .create_task(
                &project.id,
                Some(&b.id),
                "c",
                "",
                None,
                TaskSource::Plan,
                None,
                &[],
                None,
            )
            .unwrap();

        assert!(task_ancestor_chain_contains(db.conn(), &b.id, &a.id).unwrap());
        assert!(
            task_ancestor_chain_contains(db.conn(), &c.id, &a.id).unwrap(),
            "transitive ancestor (grandparent) must be detected"
        );
        assert!(
            task_ancestor_chain_contains(db.conn(), &b.id, &b.id).unwrap(),
            "a node is its own trivial ancestor (self-cycle case)"
        );
        assert!(
            !task_ancestor_chain_contains(db.conn(), &a.id, &c.id).unwrap(),
            "wrong direction: c is a's descendant, not its ancestor"
        );
    }

    // ---- update_task ----

    #[test]
    fn update_task_changes_only_provided_fields_and_tags_round_trip() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let task = db
            .create_task(
                &project.id,
                None,
                "t0",
                "b0",
                None,
                TaskSource::Plan,
                None,
                &[],
                None,
            )
            .unwrap();

        let updated = db
            .update_task(&task.id, Some("t1"), None, Some(&ids(&["x"])))
            .unwrap();
        assert_eq!(updated.title, "t1");
        assert_eq!(updated.body, "b0", "body left untouched when None");
        assert_eq!(updated.tags, ids(&["x"]));
    }

    #[test]
    fn update_task_unknown_id_is_not_found() {
        let db = Db::open_in_memory().unwrap();
        let err = db.update_task("nope", Some("x"), None, None).unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound));
    }

    #[test]
    fn archived_project_blocks_update_task() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let task = db
            .create_task(
                &project.id,
                None,
                "t",
                "",
                None,
                TaskSource::Plan,
                None,
                &[],
                None,
            )
            .unwrap();
        db.archive_project(&project.id).unwrap();
        let err = db.update_task(&task.id, Some("x"), None, None).unwrap_err();
        assert!(matches!(err, OrchdPersistError::Invariant(_)));
    }

    // ---- set_task_status ----

    #[test]
    fn set_task_status_updates_status_and_db_literal() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let task = db
            .create_task(
                &project.id,
                None,
                "t",
                "",
                None,
                TaskSource::Plan,
                None,
                &[],
                None,
            )
            .unwrap();

        let updated = db.set_task_status(&task.id, TaskStatus::Progress).unwrap();
        assert_eq!(updated.status, TaskStatus::Progress);
        let raw: String = db
            .conn()
            .query_row(
                "SELECT status FROM task WHERE id = ?1",
                rusqlite::params![task.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(raw, "progress");
    }

    #[test]
    fn set_task_status_unknown_id_is_not_found() {
        let db = Db::open_in_memory().unwrap();
        let err = db.set_task_status("nope", TaskStatus::Done).unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound));
    }

    #[test]
    fn archived_project_blocks_set_task_status() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let task = db
            .create_task(
                &project.id,
                None,
                "t",
                "",
                None,
                TaskSource::Plan,
                None,
                &[],
                None,
            )
            .unwrap();
        db.archive_project(&project.id).unwrap();
        let err = db.set_task_status(&task.id, TaskStatus::Done).unwrap_err();
        assert!(matches!(err, OrchdPersistError::Invariant(_)));
    }

    // ---- task priority (SCN-051, ST-037) ----

    #[test]
    fn create_task_defaults_priority_normal_and_honors_explicit_urgent() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();

        // `None` ⇒ `Normal` (SCN-051 create-form default; mirrors `status`'s `None` ⇒ Backlog).
        let normal = db
            .create_task(
                &project.id,
                None,
                "n",
                "",
                None,
                TaskSource::Plan,
                None,
                &[],
                None,
            )
            .unwrap();
        assert_eq!(normal.priority, TaskPriority::Normal);

        // Explicit `Urgent` persists and round-trips through an independent re-fetch.
        let urgent = db
            .create_task(
                &project.id,
                None,
                "u",
                "",
                None,
                TaskSource::Plan,
                None,
                &[],
                Some(TaskPriority::Urgent),
            )
            .unwrap();
        assert_eq!(urgent.priority, TaskPriority::Urgent);
        let refetched = db
            .list_tasks(Some(&project.id))
            .unwrap()
            .into_iter()
            .find(|t| t.id == urgent.id)
            .unwrap();
        assert_eq!(refetched.priority, TaskPriority::Urgent);
    }

    #[test]
    fn set_task_priority_updates_priority_and_db_literal() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let task = db
            .create_task(
                &project.id,
                None,
                "t",
                "",
                None,
                TaskSource::Plan,
                None,
                &[],
                None,
            )
            .unwrap();

        let updated = db
            .set_task_priority(&task.id, TaskPriority::Urgent)
            .unwrap();
        assert_eq!(updated.priority, TaskPriority::Urgent);
        assert!(
            updated.updated_at >= task.updated_at,
            "priority change must touch updated_at"
        );
        let raw: String = db
            .conn()
            .query_row(
                "SELECT priority FROM task WHERE id = ?1",
                rusqlite::params![task.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(raw, "urgent");

        // …and back to normal (SCN-051: the row control offers both directions).
        let reverted = db
            .set_task_priority(&task.id, TaskPriority::Normal)
            .unwrap();
        assert_eq!(reverted.priority, TaskPriority::Normal);
    }

    #[test]
    fn set_task_priority_unknown_id_is_not_found() {
        let db = Db::open_in_memory().unwrap();
        let err = db
            .set_task_priority("nope", TaskPriority::Urgent)
            .unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound));
    }

    #[test]
    fn archived_project_blocks_set_task_priority() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let task = db
            .create_task(
                &project.id,
                None,
                "t",
                "",
                None,
                TaskSource::Plan,
                None,
                &[],
                None,
            )
            .unwrap();
        db.archive_project(&project.id).unwrap();
        let err = db
            .set_task_priority(&task.id, TaskPriority::Urgent)
            .unwrap_err();
        assert!(matches!(err, OrchdPersistError::Invariant(_)));
    }

    // ---- set_task_rank ----

    #[test]
    fn set_task_rank_persists_f64_midpoint() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let t1 = db
            .create_task(
                &project.id,
                None,
                "t1",
                "",
                None,
                TaskSource::Plan,
                None,
                &[],
                None,
            )
            .unwrap();
        let t2 = db
            .create_task(
                &project.id,
                None,
                "t2",
                "",
                None,
                TaskSource::Plan,
                None,
                &[],
                None,
            )
            .unwrap();
        assert_eq!(t1.rank, 1024.0);
        assert_eq!(t2.rank, 2048.0);

        let midpoint = (t1.rank + t2.rank) / 2.0; // 1536.0 — client-side insert-between math
        let moved = db.set_task_rank(&t2.id, midpoint).unwrap();
        assert_eq!(moved.rank, 1536.0);
        assert_eq!(task_rank_raw(&db, &t2.id), 1536.0);
    }

    #[test]
    fn set_task_rank_unknown_id_is_not_found() {
        let db = Db::open_in_memory().unwrap();
        let err = db.set_task_rank("nope", 1.0).unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound));
    }

    #[test]
    fn archived_project_blocks_set_task_rank() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let task = db
            .create_task(
                &project.id,
                None,
                "t",
                "",
                None,
                TaskSource::Plan,
                None,
                &[],
                None,
            )
            .unwrap();
        db.archive_project(&project.id).unwrap();
        let err = db.set_task_rank(&task.id, 1.0).unwrap_err();
        assert!(matches!(err, OrchdPersistError::Invariant(_)));
    }

    // ---- delete_task ----

    #[test]
    fn delete_task_cascades_subtasks() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let parent = db
            .create_task(
                &project.id,
                None,
                "parent",
                "",
                None,
                TaskSource::Plan,
                None,
                &[],
                None,
            )
            .unwrap();
        let child = db
            .create_task(
                &project.id,
                Some(&parent.id),
                "child",
                "",
                None,
                TaskSource::Plan,
                None,
                &[],
                None,
            )
            .unwrap();
        let grandchild = db
            .create_task(
                &project.id,
                Some(&child.id),
                "gc",
                "",
                None,
                TaskSource::Plan,
                None,
                &[],
                None,
            )
            .unwrap();

        db.delete_task(&parent.id).unwrap();

        let remaining = db.list_tasks(Some(&project.id)).unwrap();
        assert!(
            remaining.is_empty(),
            "parent, child AND grandchild must all be gone"
        );

        let raw_count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM task WHERE id IN (?1, ?2, ?3)",
                rusqlite::params![parent.id, child.id, grandchild.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(raw_count, 0);
    }

    #[test]
    fn delete_task_unknown_id_is_not_found() {
        let db = Db::open_in_memory().unwrap();
        let err = db.delete_task("nope").unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound));
    }

    #[test]
    fn archived_project_blocks_delete_task() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let task = db
            .create_task(
                &project.id,
                None,
                "t",
                "",
                None,
                TaskSource::Plan,
                None,
                &[],
                None,
            )
            .unwrap();
        db.archive_project(&project.id).unwrap();
        let err = db.delete_task(&task.id).unwrap_err();
        assert!(matches!(err, OrchdPersistError::Invariant(_)));
    }

    // ---- list_tasks ----

    #[test]
    fn list_tasks_none_includes_every_project_ordered_by_rank() {
        let db = Db::open_in_memory().unwrap();
        let a = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let b = db.create_project("B", "", &ids(&["w2"])).unwrap();
        let a1 = db
            .create_task(
                &a.id,
                None,
                "a1",
                "",
                None,
                TaskSource::Plan,
                None,
                &[],
                None,
            )
            .unwrap();
        let b1 = db
            .create_task(
                &b.id,
                None,
                "b1",
                "",
                None,
                TaskSource::Plan,
                None,
                &[],
                None,
            )
            .unwrap();

        let all = db.list_tasks(None).unwrap();
        let all_ids: Vec<&str> = all.iter().map(|t| t.id.as_str()).collect();
        assert!(all_ids.contains(&a1.id.as_str()));
        assert!(all_ids.contains(&b1.id.as_str()));

        let scoped = db.list_tasks(Some(&a.id)).unwrap();
        assert_eq!(scoped.len(), 1);
        assert_eq!(scoped[0].id, a1.id);
    }

    #[test]
    fn list_tasks_orders_by_rank() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let t1 = db
            .create_task(
                &project.id,
                None,
                "t1",
                "",
                None,
                TaskSource::Plan,
                None,
                &[],
                None,
            )
            .unwrap();
        let t2 = db
            .create_task(
                &project.id,
                None,
                "t2",
                "",
                None,
                TaskSource::Plan,
                None,
                &[],
                None,
            )
            .unwrap();
        // move t2 to rank BEFORE t1.
        db.set_task_rank(&t2.id, 1.0).unwrap();

        let tasks = db.list_tasks(Some(&project.id)).unwrap();
        assert_eq!(tasks[0].id, t2.id, "lower rank must sort first");
        assert_eq!(tasks[1].id, t1.id);
    }
}

/// RuleSet persistence tests (spec §5.2, §7, task-8 brief): `get_ruleset`/`upsert_ruleset`/
/// `acknowledge_rule_file`, every validation branch, TDD (written RED before the `impl Db` block
/// above went GREEN).
#[cfg(test)]
mod ruleset_tests {
    use super::*;

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    /// Fetch a freshly-created project's own (auto-created) ruleset row.
    fn project_ruleset(db: &Db, project_id: &str) -> RuleSet {
        db.get_ruleset(RuleScope::Project, Some(project_id))
            .expect("project ruleset row must exist")
    }

    // ---- get_ruleset ----

    #[test]
    fn get_ruleset_unknown_scope_project_id_is_not_found() {
        let db = Db::open_in_memory().unwrap();
        // A bare in-memory DB has no ruleset rows at all yet — `Db::open*` only applies the
        // schema; the global row is inserted separately by `boot::ensure_global_ruleset`.
        let err = db.get_ruleset(RuleScope::Global, None).unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound));
    }

    #[test]
    fn get_ruleset_returns_the_row_auto_created_with_the_project() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();

        let rs = project_ruleset(&db, &project.id);

        assert_eq!(rs.scope, RuleScope::Project);
        assert_eq!(rs.project_id.as_deref(), Some(project.id.as_str()));
        assert!(rs.md_path.contains(&format!("project-{}.md", project.id)));
        assert_eq!(rs.md_hash, "");
        assert_eq!(rs.policy.spend_cap_usd, None);
        assert!(rs.policy.approval_classes.is_empty());
        assert!(rs.policy.path_allowlist.is_empty());
    }

    // ---- upsert_ruleset: md_content rehash ----

    #[test]
    fn upsert_ruleset_with_content_writes_the_file_and_rehashes() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let before = project_ruleset(&db, &project.id);

        // HERMETICITY: a project row's default md_path resolves under the REAL
        // `app_support_dir()` ($HOME/Library/Application Support/...) — a test that writes content
        // with `md_path: None` would leave junk in the production app-support tree. Repoint to an
        // explicit tempdir path so the file write lands there and is cleaned up on drop (same
        // pattern as `upsert_ruleset_md_path_repoints_and_writes_content_there`).
        let dir = tempfile::tempdir().unwrap();
        let md_path = dir.path().join("rules.md");

        let updated = db
            .upsert_ruleset(
                RuleScope::Project,
                Some(&project.id),
                Some("# rules\n"),
                Some(md_path.to_str().unwrap()),
                None,
            )
            .unwrap();

        assert_ne!(updated.md_hash, before.md_hash);
        assert_eq!(
            updated.md_hash,
            crate::ruleset_files::sha256_hex("# rules\n")
        );
        assert_eq!(updated.md_path, md_path.to_string_lossy());
        assert_eq!(std::fs::read_to_string(&md_path).unwrap(), "# rules\n");
        assert!(updated.updated_at >= before.updated_at);

        // Persisted, not just returned — re-fetch confirms the row itself changed.
        let refetched = project_ruleset(&db, &project.id);
        assert_eq!(refetched.md_hash, updated.md_hash);
    }

    #[test]
    fn upsert_ruleset_unknown_scope_project_id_is_not_found() {
        let db = Db::open_in_memory().unwrap();
        let err = db
            .upsert_ruleset(RuleScope::Project, Some("nope"), Some("x"), None, None)
            .unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound));
    }

    #[test]
    fn archived_project_blocks_upsert_ruleset() {
        // spec §5.2: EVERY mutating verb touching an archived project or its children ⇒
        // `Invariant`. A project-scoped ruleset row is a child of `project` — upsert-ing it must
        // be blocked the same as every other project-scoped mutator.
        //
        // HERMETICITY: a project row's default md_path resolves under the REAL
        // `app_support_dir()` ($HOME/Library/Application Support/...) — pass an explicit tempdir
        // `md_path` so IF the guard regresses and the write actually runs, it lands in the
        // tempdir (cleaned up on drop) rather than the real app-support tree (same pattern as
        // `upsert_ruleset_with_content_writes_the_file_and_rehashes`).
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let before = project_ruleset(&db, &project.id);
        let dir = tempfile::tempdir().unwrap();
        let md_path = dir.path().join("rules.md");
        db.archive_project(&project.id).unwrap();

        let err = db
            .upsert_ruleset(
                RuleScope::Project,
                Some(&project.id),
                Some("# rules\n"),
                Some(md_path.to_str().unwrap()),
                None,
            )
            .unwrap_err();

        assert!(
            matches!(&err, OrchdPersistError::Invariant(m) if m == "project archived"),
            "got {err:?}"
        );
        // No partial effect: the row's hash/path are unchanged, and no file was written.
        let after = project_ruleset(&db, &project.id);
        assert_eq!(after.md_hash, before.md_hash);
        assert_eq!(after.md_path, before.md_path);
        assert!(!md_path.exists(), "guard must fire before any file write");
    }

    // ---- upsert_ruleset: md_path validation ----

    #[test]
    fn upsert_ruleset_relative_md_path_is_validation() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();

        let err = db
            .upsert_ruleset(
                RuleScope::Project,
                Some(&project.id),
                None,
                Some("relative/rules.md"),
                None,
            )
            .unwrap_err();

        assert!(matches!(err, OrchdPersistError::Validation(_)));
        // No partial effect: the row's path is unchanged.
        assert_ne!(
            project_ruleset(&db, &project.id).md_path,
            "relative/rules.md"
        );
    }

    #[test]
    fn upsert_ruleset_md_path_with_missing_parent_dir_is_validation() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let missing_parent = dir.path().join("does-not-exist").join("rules.md");

        let err = db
            .upsert_ruleset(
                RuleScope::Project,
                Some(&project.id),
                None,
                Some(missing_parent.to_str().unwrap()),
                None,
            )
            .unwrap_err();

        assert!(matches!(err, OrchdPersistError::Validation(_)));
    }

    #[test]
    fn upsert_ruleset_md_path_repoints_and_writes_content_there() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let new_path = dir.path().join("custom-rules.md");

        let updated = db
            .upsert_ruleset(
                RuleScope::Project,
                Some(&project.id),
                Some("custom content"),
                Some(new_path.to_str().unwrap()),
                None,
            )
            .unwrap();

        assert_eq!(updated.md_path, new_path.to_string_lossy());
        assert_eq!(
            std::fs::read_to_string(&new_path).unwrap(),
            "custom content"
        );
    }

    #[test]
    fn upsert_ruleset_md_path_alone_repoints_without_writing_content() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let new_path = dir.path().join("repointed.md");

        let updated = db
            .upsert_ruleset(
                RuleScope::Project,
                Some(&project.id),
                None,
                Some(new_path.to_str().unwrap()),
                None,
            )
            .unwrap();

        assert_eq!(updated.md_path, new_path.to_string_lossy());
        assert!(
            !new_path.exists(),
            "repointing alone (no md_content) must not write a file"
        );
    }

    // ---- upsert_ruleset: policy validation ----

    #[test]
    fn upsert_ruleset_policy_negative_spend_cap_is_validation() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let policy = PolicyRules {
            spend_cap_usd: Some(-1.0),
            approval_classes: vec![],
            path_allowlist: vec![],
        };

        let err = db
            .upsert_ruleset(
                RuleScope::Project,
                Some(&project.id),
                None,
                None,
                Some(&policy),
            )
            .unwrap_err();

        assert!(matches!(err, OrchdPersistError::Validation(_)));
    }

    #[test]
    fn upsert_ruleset_policy_empty_approval_class_entry_is_validation() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let policy = PolicyRules {
            spend_cap_usd: None,
            approval_classes: vec!["".to_string()],
            path_allowlist: vec![],
        };

        let err = db
            .upsert_ruleset(
                RuleScope::Project,
                Some(&project.id),
                None,
                None,
                Some(&policy),
            )
            .unwrap_err();

        assert!(matches!(err, OrchdPersistError::Validation(_)));
    }

    #[test]
    fn upsert_ruleset_policy_empty_path_allowlist_entry_is_validation() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let policy = PolicyRules {
            spend_cap_usd: None,
            approval_classes: vec![],
            path_allowlist: vec!["".to_string()],
        };

        let err = db
            .upsert_ruleset(
                RuleScope::Project,
                Some(&project.id),
                None,
                None,
                Some(&policy),
            )
            .unwrap_err();

        assert!(matches!(err, OrchdPersistError::Validation(_)));
    }

    #[test]
    fn upsert_ruleset_policy_valid_is_stored_and_round_trips() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let policy = PolicyRules {
            spend_cap_usd: Some(12.5),
            approval_classes: vec!["deploy".to_string()],
            path_allowlist: vec!["/tmp".to_string()],
        };

        let updated = db
            .upsert_ruleset(
                RuleScope::Project,
                Some(&project.id),
                None,
                None,
                Some(&policy),
            )
            .unwrap();
        assert_eq!(updated.policy, policy);

        // Persisted, not just returned.
        assert_eq!(project_ruleset(&db, &project.id).policy, policy);
    }

    #[test]
    fn validate_policy_rejects_an_unknown_json_key() {
        // Direct unit-level proof of the deny_unknown_fields mirror (spec §5.2, PolicyRulesStrict
        // doc above): a hand-crafted JSON payload with an extra key must be rejected. This is the
        // exact mechanism `validate_policy` leans on to catch drift between `PolicyRules` (the
        // wire type) and `PolicyRulesStrict` (this module's storage-validation mirror) if the
        // wire type ever grows a field this mirror hasn't been updated to know about — see
        // `PolicyRulesStrict`'s doc comment.
        let err = serde_json::from_str::<PolicyRulesStrict>(
            r#"{"spendCapUsd":1.0,"approvalClasses":[],"pathAllowlist":[],"extra":"nope"}"#,
        )
        .unwrap_err();
        assert!(err.to_string().contains("unknown field"), "got: {err}");
    }

    // ---- acknowledge_rule_file ----

    #[test]
    fn acknowledge_rule_file_unknown_id_is_not_found() {
        let db = Db::open_in_memory().unwrap();
        let err = db.acknowledge_rule_file("nope").unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound));
    }

    #[test]
    fn acknowledge_rule_file_missing_file_is_invariant() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let rs = project_ruleset(&db, &project.id);
        // create_project persists the row's md_path but never writes the file itself (see
        // `Db::create_project`'s own doc comment: "the FILE itself is written later by the T10
        // dispatch handler") — so acknowledging fresh off project creation must see a missing
        // file, with no extra setup needed to prove this branch.

        let err = db.acknowledge_rule_file(&rs.id).unwrap_err();

        assert!(
            matches!(&err, OrchdPersistError::Invariant(m) if m == "file missing"),
            "got {err:?}"
        );
    }

    #[test]
    fn acknowledge_rule_file_after_external_edit_stores_the_new_hash() {
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();

        // HERMETICITY: repoint md_path to a tempdir so BOTH the upsert's file write AND the
        // simulated external hand-edit below land under the tempdir, not the real
        // `app_support_dir()` tree (see `upsert_ruleset_with_content_writes_the_file_and_rehashes`).
        let dir = tempfile::tempdir().unwrap();
        let md_path = dir.path().join("rules.md");
        let rs = db
            .upsert_ruleset(
                RuleScope::Project,
                Some(&project.id),
                Some("v1"),
                Some(md_path.to_str().unwrap()),
                None,
            )
            .unwrap();

        // Simulate an external hand-edit: write different content directly, bypassing orchd.
        std::fs::write(&rs.md_path, "v2 - hand edited").unwrap();

        let acknowledged = db.acknowledge_rule_file(&rs.id).unwrap();

        assert_eq!(
            acknowledged.md_hash,
            crate::ruleset_files::sha256_hex("v2 - hand edited")
        );
        assert_ne!(acknowledged.md_hash, rs.md_hash);
        assert!(acknowledged.updated_at >= rs.updated_at);

        // The whole point of Acknowledge (spec §7/§11: "[Accept] → AcknowledgeRuleFile"):
        // read_state against the acknowledged hash must now report Ok, not ExternallyModified.
        let (content, state) = crate::ruleset_files::read_state(
            Path::new(&acknowledged.md_path),
            &acknowledged.md_hash,
        );
        assert_eq!(content, Some("v2 - hand edited".to_string()));
        assert_eq!(state, bpa_orchd_proto::RuleFileState::Ok);
    }

    #[test]
    fn archived_project_blocks_acknowledge_rule_file() {
        // spec §5.2: EVERY mutating verb touching an archived project or its children ⇒
        // `Invariant`. A project-scoped ruleset row is a child of `project` — acknowledging its
        // file must be blocked the same as every other project-scoped mutator.
        let db = Db::open_in_memory().unwrap();
        let project = db.create_project("A", "", &ids(&["w1"])).unwrap();
        let dir = tempfile::tempdir().unwrap();
        let md_path = dir.path().join("rules.md");
        let rs = db
            .upsert_ruleset(
                RuleScope::Project,
                Some(&project.id),
                Some("v1"),
                Some(md_path.to_str().unwrap()),
                None,
            )
            .unwrap();
        // Simulate an external hand-edit, same as the happy-path test above.
        std::fs::write(&rs.md_path, "v2 - hand edited").unwrap();
        db.archive_project(&project.id).unwrap();

        let err = db.acknowledge_rule_file(&rs.id).unwrap_err();

        assert!(
            matches!(&err, OrchdPersistError::Invariant(m) if m == "project archived"),
            "got {err:?}"
        );
        // No partial effect: the row's hash is still the pre-archive value, not the hand-edited
        // content's hash.
        assert_eq!(project_ruleset(&db, &project.id).md_hash, rs.md_hash);
    }
}

// ================================================================================
// ---- S-EXT trust layer: invocation / artifact / consent / audit CRUD (spec §4/§6, task T5) ----
// ================================================================================
//
// `mcp_server`/`mcp_tool` CRUD lives in `crate::mcp::registry` (T2). These remaining four
// schema-v3 tables land here instead, directly alongside the rest of this file's domain CRUD:
// `consent_grant`/`audit_log` back `crate::trust` (a crate-root module, not nested under `mcp`),
// so their persistence fits better here than in a new `mcp::` submodule; `mcp_invocation`/
// `mcp_artifact` sit alongside them for the same "not really mcp_server/mcp_tool registry
// concerns" reason (task-5 brief: "Modify: ... persistence.rs (invocation/artifact/consent/audit
// CRUD)").
//
// `insert_invocation`/`insert_artifact`/`list_invocations`/`list_artifacts`/`get_artifact`/
// `insert_audit`/`list_audit`/`upsert_policy`/`list_policies`/`get_policy` all return
// `bpa_orchd_proto::{McpInvocation, McpArtifact, AuditRow, Policy}` directly (the wire entities)
// rather than inventing parallel `*Row` structs — mirrors this file's OWN established convention
// for `Project`/`Goal`/`Idea`/`Insight`/`DomainTask`/`RuleSet` (all wire types, returned directly
// by this file's CRUD), unlike `crate::mcp::{McpServerRow, McpToolRow}` (T2), which predates
// T3's wire types and had nothing to reuse yet. `audit_log`'s `AuditRow` WAS crate-local (no
// `TrustListAudit` verb existed yet) until task T18 added the wire entity and this section
// switched `insert_audit` over to it — the same migration `McpServerRow` itself never needed
// since T3 landed before this section did.

/// Input to [`Db::insert_invocation`] (spec §4 `mcp_invocation`; `id` assigned here via uuid
/// v4). `started_at` is caller-supplied — unlike every OTHER table's `created_at` in this file
/// (always stamped by `now_ms()` at insert time), this row is only written AFTER the call
/// completes (once `latency_ms`/`ok`/`error_kind` are known), but `started_at` must still
/// reflect when the call actually began; `mcp::invoke::call_tool` captures it before dispatching.
#[derive(Debug, Clone)]
pub struct NewInvocation {
    /// The MCP server this call targeted — `Some` for an MCP `tools/call` (`mcp::invoke::
    /// call_tool`), `None` for a connector_invoke. Exactly one of `server_id`/`account_id` is
    /// `Some` (the DB CHECK enforces the XOR — spec §4, T12 review).
    pub server_id: Option<String>,
    /// The connector account this call targeted — `Some` for a `ConnectorInvoke`
    /// (`connectors::adapter::invoke`), `None` for an MCP `tools/call`.
    pub account_id: Option<String>,
    /// MCP tool name OR connector op name (spec §4 `tool_name` column doubles for both).
    pub tool_name: String,
    pub project_id: Option<String>,
    pub request_hash: String,
    pub ok: bool,
    pub error_kind: Option<String>,
    pub latency_ms: i64,
    pub cost_usd: Option<f64>,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub started_at: i64,
}

/// Input to [`Db::insert_artifact`] (spec §4 `mcp_artifact`; `id`/`created_at` assigned here).
/// No `is_untrusted` field: `insert_artifact` ALWAYS writes `is_untrusted=1` (spec D9: "always
/// true for external tool output") — not a caller-settable choice, see that method's doc.
#[derive(Debug, Clone)]
pub struct NewArtifact {
    pub invocation_id: String,
    /// `Some` for an MCP `tools/call` artifact, `None` for a connector_invoke artifact. Exactly
    /// one of `server_id`/`account_id` is `Some` (DB CHECK enforces the XOR — spec §4/D9, T12
    /// review: ConnectorInvoke persists a durable untrusted artifact too).
    pub server_id: Option<String>,
    /// `Some` for a connector_invoke artifact, `None` for an MCP `tools/call` artifact.
    pub account_id: Option<String>,
    pub tool_name: String,
    pub project_id: Option<String>,
    pub content_json: String,
    pub content_text: Option<String>,
}

/// Input to [`Db::insert_audit`] (spec §4 `audit_log`; `id`/`at` assigned here). `action` /
/// `decision` are the spec's own literal TEXT values (`'connect'|'disconnect'|'stdio_spawn'|
/// 'tool_call'|'connector_invoke'|'consent_grant'|'policy_deny'` / `'allow'|'deny'`) — kept as
/// plain `String` (not a Rust enum): `crate::trust` is this type's only caller today and already
/// works with those literals as `&'static str`.
#[derive(Debug, Clone)]
pub struct NewAudit {
    pub action: String,
    pub server_id: Option<String>,
    pub tool_name: Option<String>,
    pub project_id: Option<String>,
    pub decision: String,
    pub reason: Option<String>,
    pub invocation_id: Option<String>,
}

/// Input to [`Db::upsert_policy`] (spec §4 `policy`, task T18, BL-22): `id`/`created_at`/
/// `updated_at` are assigned/stamped by the upsert itself. `scope`/`ref_id` together identify
/// WHICH row this call targets (the upsert key) — see [`PolicyScope`]'s own doc for the pairing
/// rule `upsert_policy` validates before writing.
#[derive(Debug, Clone)]
pub struct NewPolicy {
    pub scope: PolicyScope,
    pub ref_id: Option<String>,
    pub spend_cap_usd: Option<f64>,
    pub rate_per_min: Option<i64>,
}

/// `consent_grant` row, decoded (spec §4). Crate-local (no wire entity for it yet — `orchd-proto`
/// has no consent-listing verb). Carries the stored `fingerprint` so `crate::trust` can compare
/// it against the CURRENT server URL and re-prompt on mismatch (spec D10).
#[derive(Debug, Clone, PartialEq)]
pub struct ConsentRow {
    pub id: String,
    pub kind: String,
    pub server_id: String,
    pub fingerprint: String,
    pub granted_at: i64,
}

struct McpInvocationRawRow {
    id: String,
    server_id: Option<String>,
    account_id: Option<String>,
    tool_name: String,
    project_id: Option<String>,
    request_hash: String,
    ok: i64,
    error_kind: Option<String>,
    latency_ms: i64,
    cost_usd: Option<f64>,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    started_at: i64,
}

impl McpInvocationRawRow {
    fn from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: r.get(0)?,
            server_id: r.get(1)?,
            account_id: r.get(2)?,
            tool_name: r.get(3)?,
            project_id: r.get(4)?,
            request_hash: r.get(5)?,
            ok: r.get(6)?,
            error_kind: r.get(7)?,
            latency_ms: r.get(8)?,
            cost_usd: r.get(9)?,
            input_tokens: r.get(10)?,
            output_tokens: r.get(11)?,
            started_at: r.get(12)?,
        })
    }

    fn into_entity(self) -> McpInvocation {
        McpInvocation {
            id: self.id,
            server_id: self.server_id,
            account_id: self.account_id,
            tool_name: self.tool_name,
            project_id: self.project_id,
            request_hash: self.request_hash,
            ok: self.ok != 0,
            error_kind: self.error_kind,
            latency_ms: self.latency_ms,
            cost_usd: self.cost_usd,
            input_tokens: self.input_tokens,
            output_tokens: self.output_tokens,
            started_at: self.started_at,
        }
    }
}

const MCP_INVOCATION_COLUMNS: &str = "id, server_id, account_id, tool_name, project_id, \
     request_hash, ok, error_kind, latency_ms, cost_usd, input_tokens, output_tokens, started_at";

fn load_invocation(conn: &Connection, id: &str) -> Result<McpInvocation, OrchdPersistError> {
    let sql = format!("SELECT {MCP_INVOCATION_COLUMNS} FROM mcp_invocation WHERE id = ?1");
    let raw = conn
        .query_row(&sql, rusqlite::params![id], McpInvocationRawRow::from_row)
        .optional()?
        .ok_or(OrchdPersistError::NotFound)?;
    Ok(raw.into_entity())
}

struct McpArtifactRawRow {
    id: String,
    invocation_id: String,
    server_id: Option<String>,
    account_id: Option<String>,
    tool_name: String,
    project_id: Option<String>,
    content_json: String,
    content_text: Option<String>,
    is_untrusted: i64,
    created_at: i64,
}

impl McpArtifactRawRow {
    fn from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: r.get(0)?,
            invocation_id: r.get(1)?,
            server_id: r.get(2)?,
            account_id: r.get(3)?,
            tool_name: r.get(4)?,
            project_id: r.get(5)?,
            content_json: r.get(6)?,
            content_text: r.get(7)?,
            is_untrusted: r.get(8)?,
            created_at: r.get(9)?,
        })
    }

    fn into_entity(self) -> McpArtifact {
        McpArtifact {
            id: self.id,
            invocation_id: self.invocation_id,
            server_id: self.server_id,
            account_id: self.account_id,
            tool_name: self.tool_name,
            project_id: self.project_id,
            content_json: self.content_json,
            content_text: self.content_text,
            is_untrusted: self.is_untrusted != 0,
            created_at: self.created_at,
        }
    }
}

const MCP_ARTIFACT_COLUMNS: &str = "id, invocation_id, server_id, account_id, tool_name, \
     project_id, content_json, content_text, is_untrusted, created_at";

fn load_artifact(conn: &Connection, id: &str) -> Result<McpArtifact, OrchdPersistError> {
    let sql = format!("SELECT {MCP_ARTIFACT_COLUMNS} FROM mcp_artifact WHERE id = ?1");
    let raw = conn
        .query_row(&sql, rusqlite::params![id], McpArtifactRawRow::from_row)
        .optional()?
        .ok_or(OrchdPersistError::NotFound)?;
    Ok(raw.into_entity())
}

const AUDIT_COLUMNS: &str =
    "id, at, action, server_id, tool_name, project_id, decision, reason, invocation_id";

/// Decodes one `audit_log` row directly into the wire [`AuditRow`] (task T18 — see this
/// section's header comment for why `insert_audit`/`list_audit` return the wire type directly
/// rather than a crate-local `*Row`).
fn decode_audit_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<AuditRow> {
    Ok(AuditRow {
        id: r.get(0)?,
        at: r.get(1)?,
        action: r.get(2)?,
        server_id: r.get(3)?,
        tool_name: r.get(4)?,
        project_id: r.get(5)?,
        decision: r.get(6)?,
        reason: r.get(7)?,
        invocation_id: r.get(8)?,
    })
}

fn load_audit(conn: &Connection, id: &str) -> Result<AuditRow, OrchdPersistError> {
    let sql = format!("SELECT {AUDIT_COLUMNS} FROM audit_log WHERE id = ?1");
    conn.query_row(&sql, rusqlite::params![id], decode_audit_row)
        .optional()?
        .ok_or(OrchdPersistError::NotFound)
}

// ---- policy enum <-> TEXT helpers (spec §4 CHECK literal, task T18) — mirrors
// `mcp::registry::encode_scope`/`decode_scope`'s exact shape. ----

fn encode_policy_scope(s: &PolicyScope) -> &'static str {
    match s {
        PolicyScope::Global => "global",
        PolicyScope::Project => "project",
        PolicyScope::Server => "server",
    }
}

fn decode_policy_scope(s: &str) -> Result<PolicyScope, OrchdPersistError> {
    match s {
        "global" => Ok(PolicyScope::Global),
        "project" => Ok(PolicyScope::Project),
        "server" => Ok(PolicyScope::Server),
        other => Err(OrchdPersistError::Io(format!(
            "corrupt policy.scope value: {other}"
        ))),
    }
}

const POLICY_COLUMNS: &str =
    "id, scope, ref_id, spend_cap_usd, rate_per_min, created_at, updated_at";

struct PolicyRawRow {
    id: String,
    scope: String,
    ref_id: Option<String>,
    spend_cap_usd: Option<f64>,
    rate_per_min: Option<i64>,
    created_at: i64,
    updated_at: i64,
}

impl PolicyRawRow {
    fn from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Self> {
        Ok(Self {
            id: r.get(0)?,
            scope: r.get(1)?,
            ref_id: r.get(2)?,
            spend_cap_usd: r.get(3)?,
            rate_per_min: r.get(4)?,
            created_at: r.get(5)?,
            updated_at: r.get(6)?,
        })
    }

    fn into_entity(self) -> Result<Policy, OrchdPersistError> {
        Ok(Policy {
            id: self.id,
            scope: decode_policy_scope(&self.scope)?,
            ref_id: self.ref_id,
            spend_cap_usd: self.spend_cap_usd,
            rate_per_min: self.rate_per_min,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

fn load_policy(conn: &Connection, id: &str) -> Result<Policy, OrchdPersistError> {
    let sql = format!("SELECT {POLICY_COLUMNS} FROM policy WHERE id = ?1");
    let raw = conn
        .query_row(&sql, rusqlite::params![id], PolicyRawRow::from_row)
        .optional()?
        .ok_or(OrchdPersistError::NotFound)?;
    raw.into_entity()
}

impl Db {
    /// `insert_invocation` (spec §4 `mcp_invocation`, task-5 brief). Every `tools/call` attempt
    /// writes exactly one row — success or terminal failure (spec D8: "every tools/call writes
    /// an mcp_invocation row"). `id` assigned here (uuid v4); `started_at` is caller-supplied
    /// (see [`NewInvocation`]'s doc comment).
    pub fn insert_invocation(
        &self,
        new: NewInvocation,
    ) -> Result<McpInvocation, OrchdPersistError> {
        let tx = self.conn().unchecked_transaction()?;
        let id = Uuid::new_v4().to_string();
        tx.execute(
            "INSERT INTO mcp_invocation
               (id, server_id, account_id, tool_name, project_id, request_hash, ok, error_kind,
                latency_ms, cost_usd, input_tokens, output_tokens, started_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
            rusqlite::params![
                id,
                new.server_id,
                new.account_id,
                new.tool_name,
                new.project_id,
                new.request_hash,
                new.ok as i64,
                new.error_kind,
                new.latency_ms,
                new.cost_usd,
                new.input_tokens,
                new.output_tokens,
                new.started_at,
            ],
        )?;
        let row = load_invocation(&tx, &id)?;
        tx.commit()?;
        Ok(row)
    }

    /// `list_invocations` (task-5 brief): newest-first, optionally filtered by `server_id`
    /// and/or `project_id`, optionally capped at `limit` rows. Mirrors `list_ideas`/
    /// `list_insights`/`list_tasks`'s `?1 IS NULL OR col = ?1` optional-filter idiom.
    pub fn list_invocations(
        &self,
        server_id: Option<&str>,
        project_id: Option<&str>,
        limit: Option<i64>,
    ) -> Result<Vec<McpInvocation>, OrchdPersistError> {
        let mut stmt = self.conn().prepare(
            "SELECT id FROM mcp_invocation
             WHERE (?1 IS NULL OR server_id = ?1) AND (?2 IS NULL OR project_id = ?2)
             ORDER BY started_at DESC, id
             LIMIT ?3",
        )?;
        let ids: Vec<String> = stmt
            .query_map(
                rusqlite::params![server_id, project_id, limit.unwrap_or(i64::MAX)],
                |r| r.get(0),
            )?
            .collect::<Result<_, _>>()?;
        drop(stmt);
        ids.iter()
            .map(|id| load_invocation(self.conn(), id))
            .collect()
    }

    /// `insert_artifact` (spec §4 `mcp_artifact`, D9): `is_untrusted` is ALWAYS written as `1`
    /// (spec D9: "always true for external tool output") — not a field on [`NewArtifact`], so no
    /// code path can persist an artifact any other way.
    pub fn insert_artifact(&self, new: NewArtifact) -> Result<McpArtifact, OrchdPersistError> {
        let tx = self.conn().unchecked_transaction()?;
        let id = Uuid::new_v4().to_string();
        let now = now_ms();
        tx.execute(
            "INSERT INTO mcp_artifact
               (id, invocation_id, server_id, account_id, tool_name, project_id, content_json,
                content_text, is_untrusted, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 1, ?9)",
            rusqlite::params![
                id,
                new.invocation_id,
                new.server_id,
                new.account_id,
                new.tool_name,
                new.project_id,
                new.content_json,
                new.content_text,
                now,
            ],
        )?;
        let row = load_artifact(&tx, &id)?;
        tx.commit()?;
        Ok(row)
    }

    /// `list_artifacts` (task-5 brief): newest-first, optionally filtered by `project_id`
    /// and/or `server_id`, optionally capped at `limit`.
    pub fn list_artifacts(
        &self,
        project_id: Option<&str>,
        server_id: Option<&str>,
        limit: Option<i64>,
    ) -> Result<Vec<McpArtifact>, OrchdPersistError> {
        let mut stmt = self.conn().prepare(
            "SELECT id FROM mcp_artifact
             WHERE (?1 IS NULL OR project_id = ?1) AND (?2 IS NULL OR server_id = ?2)
             ORDER BY created_at DESC, id
             LIMIT ?3",
        )?;
        let ids: Vec<String> = stmt
            .query_map(
                rusqlite::params![project_id, server_id, limit.unwrap_or(i64::MAX)],
                |r| r.get(0),
            )?
            .collect::<Result<_, _>>()?;
        drop(stmt);
        ids.iter()
            .map(|id| load_artifact(self.conn(), id))
            .collect()
    }

    /// `get_artifact` (task-5 brief). Unknown `id` ⇒ `NotFound`.
    pub fn get_artifact(&self, id: &str) -> Result<McpArtifact, OrchdPersistError> {
        load_artifact(self.conn(), id)
    }

    /// `has_consent` (spec §6/D10): existence check only — `(server_id, kind)` is UNIQUE in
    /// `consent_grant`, so this is `true` iff a grant row exists. Does NOT compare `fingerprint`;
    /// the trust gate (`crate::trust::authorize`) uses [`Db::get_consent`] instead so it can
    /// re-prompt on a URL change (spec D10). This existence-only variant is retained for callers
    /// that only need to know whether *any* grant exists (e.g. a UI "consent granted" badge).
    pub fn has_consent(&self, server_id: &str, kind: &str) -> Result<bool, OrchdPersistError> {
        Ok(self.get_consent(server_id, kind)?.is_some())
    }

    /// `get_consent` (spec §6/D10, task-5 review fix): the stored grant row for `(server_id,
    /// kind)`, or `None` if never granted. `crate::trust::authorize` compares the returned
    /// `fingerprint` against the CURRENT server URL and re-prompts (denies with
    /// `consent_required`) on mismatch — closing the credential-exfil path where a server row's
    /// `url` is repointed after consent was granted for a different URL.
    pub fn get_consent(
        &self,
        server_id: &str,
        kind: &str,
    ) -> Result<Option<ConsentRow>, OrchdPersistError> {
        let row = self
            .conn()
            .query_row(
                "SELECT id, kind, server_id, fingerprint, granted_at
                 FROM consent_grant WHERE server_id = ?1 AND kind = ?2",
                rusqlite::params![server_id, kind],
                |r| {
                    Ok(ConsentRow {
                        id: r.get(0)?,
                        kind: r.get(1)?,
                        server_id: r.get(2)?,
                        fingerprint: r.get(3)?,
                        granted_at: r.get(4)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    }

    /// `grant_consent` (spec §6/D10, task-5 brief): upserts — `UNIQUE(server_id, kind)` means a
    /// re-grant (e.g. after a fingerprint change) replaces the prior grant's `fingerprint`/
    /// `granted_at` rather than erroring. `server_id` must reference an existing `mcp_server` row
    /// (`FOREIGN KEY ... `, `foreign_keys=ON` on every `Db` connection) — an unknown `server_id`
    /// surfaces as the FK's own `OrchdPersistError::Sql`, matching every other FK'd insert in
    /// this file (no bespoke existence pre-check here either).
    pub fn grant_consent(
        &self,
        server_id: &str,
        kind: &str,
        fingerprint: &str,
    ) -> Result<(), OrchdPersistError> {
        self.conn().execute(
            "INSERT INTO consent_grant (id, kind, server_id, fingerprint, granted_at)
             VALUES (?1, ?2, ?3, ?4, ?5)
             ON CONFLICT(server_id, kind) DO UPDATE SET
               fingerprint = excluded.fingerprint,
               granted_at = excluded.granted_at",
            rusqlite::params![
                Uuid::new_v4().to_string(),
                kind,
                server_id,
                fingerprint,
                now_ms(),
            ],
        )?;
        Ok(())
    }

    /// `insert_audit` (spec §6/D10, task-5 brief): the trust choke-point's append-only sink —
    /// `crate::trust::authorize` calls this on EVERY decision, allow or deny. `id`/`at` assigned
    /// here (uuid v4 / `now_ms()`).
    pub fn insert_audit(&self, new: NewAudit) -> Result<AuditRow, OrchdPersistError> {
        let tx = self.conn().unchecked_transaction()?;
        let id = Uuid::new_v4().to_string();
        let at = now_ms();
        tx.execute(
            "INSERT INTO audit_log
               (id, at, action, server_id, tool_name, project_id, decision, reason, invocation_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
            rusqlite::params![
                id,
                at,
                new.action,
                new.server_id,
                new.tool_name,
                new.project_id,
                new.decision,
                new.reason,
                new.invocation_id,
            ],
        )?;
        let row = load_audit(&tx, &id)?;
        tx.commit()?;
        Ok(row)
    }

    /// `list_audit` (spec §5 `TrustListAudit`, task T18): newest-first, optionally capped at
    /// `limit` — mirrors `list_invocations`'s own `?1 IS NULL OR ...`-optional-filter /
    /// `ORDER BY ... DESC, id` / `LIMIT ?N` idiom (here there is no filter column to make
    /// optional, just the cap).
    pub fn list_audit(&self, limit: Option<i64>) -> Result<Vec<AuditRow>, OrchdPersistError> {
        let mut stmt = self
            .conn()
            .prepare("SELECT id FROM audit_log ORDER BY at DESC, id LIMIT ?1")?;
        let ids: Vec<String> = stmt
            .query_map(rusqlite::params![limit.unwrap_or(i64::MAX)], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        drop(stmt);
        ids.iter().map(|id| load_audit(self.conn(), id)).collect()
    }

    /// `upsert_policy` (spec §4 `policy`, task T18, BL-22): UPSERT keyed by `(scope, ref_id)` —
    /// re-setting a policy for the same scope/reference replaces its caps in place rather than
    /// creating a second row. Validates the scope/`ref_id` pairing (`Global` ⇒ `ref_id: None`;
    /// `Project`/`Server` ⇒ `ref_id: Some(_)`) BEFORE writing — the DB's own two `CHECK`s (spec
    /// §4) are the same invariant as defense-in-depth, but a clean `Validation` error here (like
    /// every other hand-validated verb in this file) beats surfacing a raw `Sql` constraint
    /// failure to the caller.
    ///
    /// Implemented as an explicit read-then-write (a `SELECT ... WHERE scope = ?1 AND ref_id IS
    /// ?2` existence check, then `UPDATE` or `INSERT`) inside one transaction, NOT a SQL `ON
    /// CONFLICT` upsert: `ref_id` is `NULL` for the single global-scope row, and SQL's own NULL
    /// semantics make `NULL <> NULL` in a UNIQUE constraint (so two `INSERT`s with `ref_id: NULL`
    /// would never collide, defeating a plain `ON CONFLICT(scope, ref_id)`), while `... IS ?2`
    /// here correctly treats `NULL` as equal to `NULL`. Race-safe because it is: every caller
    /// reaches this through the SAME single `Arc<Mutex<Db>>` guard every other mutating verb in
    /// this daemon holds around its own read-then-write (see `mcp::invoke::call_tool`'s own doc
    /// comment on that invariant) — there is no concurrent SQL access to this connection to race
    /// against.
    pub fn upsert_policy(&self, new: NewPolicy) -> Result<Policy, OrchdPersistError> {
        match new.scope {
            PolicyScope::Global if new.ref_id.is_some() => {
                return Err(OrchdPersistError::Validation(
                    "policy.ref_id must be omitted for scope 'global'".to_string(),
                ));
            }
            PolicyScope::Project | PolicyScope::Server if new.ref_id.is_none() => {
                return Err(OrchdPersistError::Validation(
                    "policy.ref_id is required for scope 'project'/'server'".to_string(),
                ));
            }
            _ => {}
        }

        let tx = self.conn().unchecked_transaction()?;
        let now = now_ms();
        let scope_text = encode_policy_scope(&new.scope);
        let existing_id: Option<String> = tx
            .query_row(
                "SELECT id FROM policy WHERE scope = ?1 AND ref_id IS ?2",
                rusqlite::params![scope_text, new.ref_id],
                |r| r.get(0),
            )
            .optional()?;

        let id = match existing_id {
            Some(id) => {
                tx.execute(
                    "UPDATE policy SET spend_cap_usd = ?1, rate_per_min = ?2, updated_at = ?3 \
                     WHERE id = ?4",
                    rusqlite::params![new.spend_cap_usd, new.rate_per_min, now, id],
                )?;
                id
            }
            None => {
                let id = Uuid::new_v4().to_string();
                tx.execute(
                    "INSERT INTO policy
                       (id, scope, ref_id, spend_cap_usd, rate_per_min, created_at, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6)",
                    rusqlite::params![
                        id,
                        scope_text,
                        new.ref_id,
                        new.spend_cap_usd,
                        new.rate_per_min,
                        now,
                    ],
                )?;
                id
            }
        };
        let row = load_policy(&tx, &id)?;
        tx.commit()?;
        Ok(row)
    }

    /// `list_policies` (task T18): every configured policy, deterministically ordered (`scope`
    /// then `ref_id` then `id` — `NULL` `ref_id` — the single global row — sorts first).
    pub fn list_policies(&self) -> Result<Vec<Policy>, OrchdPersistError> {
        let mut stmt = self
            .conn()
            .prepare("SELECT id FROM policy ORDER BY scope, ref_id, id")?;
        let ids: Vec<String> = stmt
            .query_map([], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        drop(stmt);
        ids.iter().map(|id| load_policy(self.conn(), id)).collect()
    }

    /// `get_policy` (task T18, `crate::trust::resolve_policy`'s building block): the row at
    /// EXACTLY `(scope, ref_id)`, or `None` if no policy has been configured at that scope.
    pub fn get_policy(
        &self,
        scope: PolicyScope,
        ref_id: Option<&str>,
    ) -> Result<Option<Policy>, OrchdPersistError> {
        let id: Option<String> = self
            .conn()
            .query_row(
                "SELECT id FROM policy WHERE scope = ?1 AND ref_id IS ?2",
                rusqlite::params![encode_policy_scope(&scope), ref_id],
                |r| r.get(0),
            )
            .optional()?;
        id.map(|id| load_policy(self.conn(), &id)).transpose()
    }

    /// Count of `mcp_invocation` rows started at or after `since_ms`, scoped by `scope`/`ref_id`
    /// (task T18, spec §6 rate-limit check): `Global` counts every invocation (MCP tool calls
    /// AND connector invokes share this one table); `Project`/`Server` filter to the matching
    /// `project_id`/`server_id` column. The CURRENT (not-yet-dispatched) attempt this check is
    /// gating is never included — `crate::trust::authorize` runs BEFORE the row for this
    /// attempt would be written (spec §6: pre-dispatch).
    pub fn count_invocations_since(
        &self,
        scope: PolicyScope,
        ref_id: Option<&str>,
        since_ms: i64,
    ) -> Result<i64, OrchdPersistError> {
        let sql = match scope {
            PolicyScope::Global => "SELECT COUNT(*) FROM mcp_invocation WHERE started_at >= ?1",
            PolicyScope::Project => {
                "SELECT COUNT(*) FROM mcp_invocation WHERE started_at >= ?1 AND project_id = ?2"
            }
            PolicyScope::Server => {
                "SELECT COUNT(*) FROM mcp_invocation WHERE started_at >= ?1 AND server_id = ?2"
            }
        };
        let count = match scope {
            PolicyScope::Global => {
                self.conn()
                    .query_row(sql, rusqlite::params![since_ms], |r| r.get(0))?
            }
            PolicyScope::Project | PolicyScope::Server => {
                self.conn()
                    .query_row(sql, rusqlite::params![since_ms, ref_id], |r| r.get(0))?
            }
        };
        Ok(count)
    }

    /// Sum of `mcp_invocation.cost_usd` since `since_ms`, scoped by `scope`/`ref_id` (task T18,
    /// spec §6 spend-cap check): `COALESCE(SUM(...), 0)` so an empty window (or a window whose
    /// invocations never reported a cost) sums to `0.0`, never SQL `NULL` — a NULL `cost_usd` is
    /// the honest default (spec §4: "null unless server reports usage"), so a scope where NO
    /// invocation has ever reported a cost sums to `0.0` and can never trip a spend cap on its
    /// own (task T18 brief: "the spend cap binds ONLY when servers report cost"). Mirrors
    /// [`Db::count_invocations_since`]'s scope-filter shape exactly.
    pub fn sum_cost_since(
        &self,
        scope: PolicyScope,
        ref_id: Option<&str>,
        since_ms: i64,
    ) -> Result<f64, OrchdPersistError> {
        let sql = match scope {
            PolicyScope::Global => {
                "SELECT COALESCE(SUM(cost_usd), 0) FROM mcp_invocation WHERE started_at >= ?1"
            }
            PolicyScope::Project => {
                "SELECT COALESCE(SUM(cost_usd), 0) FROM mcp_invocation \
                 WHERE started_at >= ?1 AND project_id = ?2"
            }
            PolicyScope::Server => {
                "SELECT COALESCE(SUM(cost_usd), 0) FROM mcp_invocation \
                 WHERE started_at >= ?1 AND server_id = ?2"
            }
        };
        let sum = match scope {
            PolicyScope::Global => {
                self.conn()
                    .query_row(sql, rusqlite::params![since_ms], |r| r.get(0))?
            }
            PolicyScope::Project | PolicyScope::Server => {
                self.conn()
                    .query_row(sql, rusqlite::params![since_ms, ref_id], |r| r.get(0))?
            }
        };
        Ok(sum)
    }
}

#[cfg(test)]
mod trust_persistence_tests {
    use super::*;
    use crate::connectors::{AccountAuthKind, NewAccount};
    use crate::mcp::{McpAuthKind, McpScope, McpTransport, NewMcpServer};

    fn new_db() -> Db {
        Db::open_in_memory().unwrap()
    }

    fn add_server(db: &Db) -> String {
        db.add_mcp_server(NewMcpServer {
            name: "Prowl".to_string(),
            transport: McpTransport::Http,
            url: Some("https://example.com/mcp".to_string()),
            command: None,
            args: vec![],
            env: Default::default(),
            scope: McpScope::Global,
            project_id: None,
            auth_kind: McpAuthKind::None,
            secret_ref: None,
            account_id: None,
            enabled: true,
            timeout_ms: 30_000,
            max_retries: 2,
        })
        .unwrap()
        .id
    }

    /// Inserts an `account` row directly (no Keychain — pure DB, `secret_ref` is a plain fake
    /// ref string), so the connector invocation/artifact XOR path (T12 review) can be exercised
    /// at the persistence layer without a real credential.
    fn add_account(db: &Db) -> String {
        let id = uuid::Uuid::new_v4().to_string();
        db.insert_account(NewAccount {
            id: id.clone(),
            provider: "generic-rest".to_string(),
            label: "Test REST".to_string(),
            auth_kind: AccountAuthKind::Apikey,
            secret_ref: format!("{id}:apikey"),
            scopes: vec![],
            expires_at: None,
            refresh_ref: None,
        })
        .unwrap()
        .id
    }

    fn new_invocation(server_id: &str) -> NewInvocation {
        NewInvocation {
            server_id: Some(server_id.to_string()),
            account_id: None,
            tool_name: "search".to_string(),
            project_id: None,
            request_hash: "deadbeef".to_string(),
            ok: true,
            error_kind: None,
            latency_ms: 42,
            cost_usd: Some(0.01),
            input_tokens: Some(10),
            output_tokens: Some(20),
            started_at: now_ms(),
        }
    }

    /// A connector_invoke invocation — `account_id` set, `server_id` null (T12 review).
    fn new_connector_invocation(account_id: &str) -> NewInvocation {
        NewInvocation {
            server_id: None,
            account_id: Some(account_id.to_string()),
            tool_name: "get".to_string(),
            project_id: None,
            request_hash: "deadbeef".to_string(),
            ok: true,
            error_kind: None,
            latency_ms: 42,
            cost_usd: None,
            input_tokens: None,
            output_tokens: None,
            started_at: now_ms(),
        }
    }

    // ---- insert_invocation / list_invocations ----

    #[test]
    fn insert_invocation_round_trips() {
        let db = new_db();
        let server_id = add_server(&db);
        let row = db.insert_invocation(new_invocation(&server_id)).unwrap();
        assert!(!row.id.is_empty());
        assert_eq!(row.server_id.as_deref(), Some(server_id.as_str()));
        assert_eq!(row.account_id, None);
        assert_eq!(row.tool_name, "search");
        assert!(row.ok);
        assert_eq!(row.error_kind, None);
        assert_eq!(row.cost_usd, Some(0.01));
        assert_eq!(row.input_tokens, Some(10));
        assert_eq!(row.output_tokens, Some(20));
    }

    #[test]
    fn insert_connector_invocation_and_artifact_round_trip_with_account_id() {
        // T12 review: a connector_invoke persists a durable invocation + untrusted artifact via
        // the SAME insert path as an MCP tools/call, keyed by account_id (server_id null).
        let db = new_db();
        let account_id = add_account(&db);

        let invocation = db
            .insert_invocation(new_connector_invocation(&account_id))
            .unwrap();
        assert_eq!(invocation.server_id, None);
        assert_eq!(invocation.account_id.as_deref(), Some(account_id.as_str()));
        assert_eq!(invocation.tool_name, "get");

        let artifact = db
            .insert_artifact(NewArtifact {
                invocation_id: invocation.id.clone(),
                server_id: None,
                account_id: Some(account_id.clone()),
                tool_name: "get".to_string(),
                project_id: None,
                content_json: "{\"ok\":true}".to_string(),
                content_text: Some("ok".to_string()),
            })
            .unwrap();
        assert!(
            artifact.is_untrusted,
            "D9: connector artifact is untrusted too"
        );
        assert_eq!(artifact.server_id, None);
        assert_eq!(artifact.account_id.as_deref(), Some(account_id.as_str()));

        // it shows up in an unfiltered list (server-filtered lists correctly exclude it).
        let all = db.list_artifacts(None, None, None).unwrap();
        assert!(all.iter().any(|a| a.id == artifact.id));
    }

    #[test]
    fn insert_invocation_rejects_both_server_and_account_id_set() {
        // The XOR CHECK forbids a row with BOTH sources (or neither) set.
        let db = new_db();
        let server_id = add_server(&db);
        let account_id = add_account(&db);
        let mut both = new_invocation(&server_id);
        both.account_id = Some(account_id);
        let err = db.insert_invocation(both).unwrap_err();
        assert!(
            matches!(err, OrchdPersistError::Sql(_)),
            "XOR CHECK violation must surface as a Sql error; got {err:?}"
        );
    }

    #[test]
    fn insert_invocation_rejects_neither_server_nor_account_id_set() {
        let db = new_db();
        let server_id = add_server(&db);
        let mut neither = new_invocation(&server_id);
        neither.server_id = None;
        let err = db.insert_invocation(neither).unwrap_err();
        assert!(
            matches!(err, OrchdPersistError::Sql(_)),
            "XOR CHECK violation must surface as a Sql error; got {err:?}"
        );
    }

    #[test]
    fn list_invocations_filters_by_server_and_project_newest_first() {
        let db = new_db();
        let server_a = add_server(&db);
        let server_b = add_server(&db);

        let mut first = new_invocation(&server_a);
        first.started_at = 1_000;
        db.insert_invocation(first).unwrap();
        let mut second = new_invocation(&server_a);
        second.started_at = 2_000;
        let second_row = db.insert_invocation(second).unwrap();
        db.insert_invocation(new_invocation(&server_b)).unwrap();

        let for_a = db.list_invocations(Some(&server_a), None, None).unwrap();
        assert_eq!(for_a.len(), 2);
        assert_eq!(for_a[0].id, second_row.id, "newest first");

        let all = db.list_invocations(None, None, None).unwrap();
        assert_eq!(all.len(), 3);

        let capped = db.list_invocations(None, None, Some(1)).unwrap();
        assert_eq!(capped.len(), 1);
    }

    // ---- insert_artifact / list_artifacts / get_artifact ----

    #[test]
    fn insert_artifact_always_sets_is_untrusted() {
        let db = new_db();
        let server_id = add_server(&db);
        let invocation = db.insert_invocation(new_invocation(&server_id)).unwrap();

        let artifact = db
            .insert_artifact(NewArtifact {
                invocation_id: invocation.id.clone(),
                server_id: Some(server_id.clone()),
                account_id: None,
                tool_name: "search".to_string(),
                project_id: None,
                content_json: "{\"ok\":true}".to_string(),
                content_text: Some("ok".to_string()),
            })
            .unwrap();

        assert!(artifact.is_untrusted, "D9: every artifact is untrusted");
        assert_eq!(artifact.invocation_id, invocation.id);
        assert_eq!(artifact.content_json, "{\"ok\":true}");

        let fetched = db.get_artifact(&artifact.id).unwrap();
        assert_eq!(fetched, artifact);
    }

    #[test]
    fn get_artifact_unknown_id_is_not_found() {
        let db = new_db();
        let err = db.get_artifact("missing").unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound), "{err:?}");
    }

    #[test]
    fn list_artifacts_filters_by_project_and_server() {
        let db = new_db();
        let server_id = add_server(&db);
        let invocation = db.insert_invocation(new_invocation(&server_id)).unwrap();
        db.insert_artifact(NewArtifact {
            invocation_id: invocation.id.clone(),
            server_id: Some(server_id.clone()),
            account_id: None,
            tool_name: "search".to_string(),
            project_id: Some("proj-1".to_string()),
            content_json: "{}".to_string(),
            content_text: None,
        })
        .unwrap();
        db.insert_artifact(NewArtifact {
            invocation_id: invocation.id.clone(),
            server_id: Some(server_id.clone()),
            account_id: None,
            tool_name: "search".to_string(),
            project_id: Some("proj-2".to_string()),
            content_json: "{}".to_string(),
            content_text: None,
        })
        .unwrap();

        let for_proj_1 = db
            .list_artifacts(Some("proj-1"), Some(&server_id), None)
            .unwrap();
        assert_eq!(for_proj_1.len(), 1);
        assert_eq!(for_proj_1[0].project_id.as_deref(), Some("proj-1"));

        let for_server = db.list_artifacts(None, Some(&server_id), None).unwrap();
        assert_eq!(for_server.len(), 2);
    }

    // ---- has_consent / grant_consent ----

    #[test]
    fn has_consent_false_until_granted_then_true() {
        let db = new_db();
        let server_id = add_server(&db);
        assert!(!db.has_consent(&server_id, "connect").unwrap());

        db.grant_consent(&server_id, "connect", "https://example.com/mcp")
            .unwrap();
        assert!(db.has_consent(&server_id, "connect").unwrap());
        // a different kind is unaffected
        assert!(!db.has_consent(&server_id, "stdio_exec").unwrap());
    }

    #[test]
    fn grant_consent_upserts_on_re_grant() {
        let db = new_db();
        let server_id = add_server(&db);
        db.grant_consent(&server_id, "connect", "https://example.com/mcp")
            .unwrap();
        db.grant_consent(&server_id, "connect", "https://example.com/mcp-v2")
            .unwrap();

        let fingerprint: String = db
            .conn()
            .query_row(
                "SELECT fingerprint FROM consent_grant WHERE server_id = ?1 AND kind = 'connect'",
                rusqlite::params![server_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(fingerprint, "https://example.com/mcp-v2");

        let count: i64 = db
            .conn()
            .query_row(
                "SELECT COUNT(*) FROM consent_grant WHERE server_id = ?1",
                rusqlite::params![server_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(count, 1, "upsert must not create a second row");
    }

    #[test]
    fn get_consent_returns_stored_fingerprint_or_none() {
        let db = new_db();
        let server_id = add_server(&db);
        assert!(db.get_consent(&server_id, "connect").unwrap().is_none());

        db.grant_consent(&server_id, "connect", "https://example.com/mcp")
            .unwrap();
        let row = db.get_consent(&server_id, "connect").unwrap().unwrap();
        assert_eq!(row.kind, "connect");
        assert_eq!(row.server_id, server_id);
        assert_eq!(row.fingerprint, "https://example.com/mcp");

        // a re-grant at a new url updates the fingerprint get_consent returns
        db.grant_consent(&server_id, "connect", "https://example.com/mcp-v2")
            .unwrap();
        assert_eq!(
            db.get_consent(&server_id, "connect")
                .unwrap()
                .unwrap()
                .fingerprint,
            "https://example.com/mcp-v2"
        );
    }

    // ---- insert_audit ----

    #[test]
    fn insert_audit_round_trips_and_never_requires_secrets() {
        let db = new_db();
        let server_id = add_server(&db);
        let row = db
            .insert_audit(NewAudit {
                action: "connect".to_string(),
                server_id: Some(server_id.clone()),
                tool_name: None,
                project_id: None,
                decision: "deny".to_string(),
                reason: Some("consent_required".to_string()),
                invocation_id: None,
            })
            .unwrap();

        assert!(!row.id.is_empty());
        assert_eq!(row.action, "connect");
        assert_eq!(row.server_id.as_deref(), Some(server_id.as_str()));
        assert_eq!(row.decision, "deny");
        assert_eq!(row.reason.as_deref(), Some("consent_required"));
    }

    #[test]
    fn list_audit_is_newest_first_and_respects_limit() {
        let db = new_db();
        let server_id = add_server(&db);
        for i in 0..3 {
            db.insert_audit(NewAudit {
                action: "tool_call".to_string(),
                server_id: Some(server_id.clone()),
                tool_name: Some(format!("tool-{i}")),
                project_id: None,
                decision: "allow".to_string(),
                reason: None,
                invocation_id: None,
            })
            .unwrap();
            // `insert_audit` stamps `at` via `now_ms()` internally (not caller-supplied, unlike
            // `NewInvocation.started_at`) — a tiny real sleep guarantees three DISTINCT
            // millisecond timestamps so "newest first" has a deterministic order to assert
            // against, rather than racing millisecond-resolution clock granularity.
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        let all = db.list_audit(None).unwrap();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].tool_name.as_deref(), Some("tool-2"), "newest first");

        let capped = db.list_audit(Some(2)).unwrap();
        assert_eq!(capped.len(), 2);
    }

    // ---- upsert_policy / list_policies / get_policy (task T18, BL-22) ----

    #[test]
    fn upsert_policy_inserts_then_updates_in_place_on_re_set() {
        let db = new_db();
        let created = db
            .upsert_policy(NewPolicy {
                scope: PolicyScope::Global,
                ref_id: None,
                spend_cap_usd: Some(10.0),
                rate_per_min: Some(60),
            })
            .unwrap();
        assert_eq!(created.scope, PolicyScope::Global);
        assert_eq!(created.ref_id, None);
        assert_eq!(created.spend_cap_usd, Some(10.0));
        assert_eq!(created.rate_per_min, Some(60));

        let updated = db
            .upsert_policy(NewPolicy {
                scope: PolicyScope::Global,
                ref_id: None,
                spend_cap_usd: Some(20.0),
                rate_per_min: None,
            })
            .unwrap();
        assert_eq!(
            updated.id, created.id,
            "re-setting the same scope must UPDATE the existing row, not insert a second one"
        );
        assert_eq!(updated.spend_cap_usd, Some(20.0));
        assert_eq!(
            updated.rate_per_min, None,
            "null clears a previously-set cap"
        );

        let all = db.list_policies().unwrap();
        assert_eq!(all.len(), 1, "upsert must never create a duplicate row");
    }

    #[test]
    fn upsert_policy_project_and_server_scopes_are_independent_rows() {
        let db = new_db();
        let server_id = add_server(&db);
        let project = db
            .upsert_policy(NewPolicy {
                scope: PolicyScope::Project,
                ref_id: Some("proj-1".to_string()),
                spend_cap_usd: Some(5.0),
                rate_per_min: None,
            })
            .unwrap();
        let server = db
            .upsert_policy(NewPolicy {
                scope: PolicyScope::Server,
                ref_id: Some(server_id.clone()),
                spend_cap_usd: None,
                rate_per_min: Some(10),
            })
            .unwrap();
        assert_ne!(project.id, server.id);

        let all = db.list_policies().unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn upsert_policy_rejects_a_global_scope_with_a_ref_id() {
        let db = new_db();
        let err = db
            .upsert_policy(NewPolicy {
                scope: PolicyScope::Global,
                ref_id: Some("proj-1".to_string()),
                spend_cap_usd: None,
                rate_per_min: None,
            })
            .unwrap_err();
        assert!(matches!(err, OrchdPersistError::Validation(_)), "{err:?}");
    }

    #[test]
    fn upsert_policy_rejects_a_project_scope_without_a_ref_id() {
        let db = new_db();
        let err = db
            .upsert_policy(NewPolicy {
                scope: PolicyScope::Project,
                ref_id: None,
                spend_cap_usd: None,
                rate_per_min: None,
            })
            .unwrap_err();
        assert!(matches!(err, OrchdPersistError::Validation(_)), "{err:?}");
    }

    #[test]
    fn get_policy_returns_none_for_an_unconfigured_scope() {
        let db = new_db();
        assert_eq!(db.get_policy(PolicyScope::Global, None).unwrap(), None);
        assert_eq!(
            db.get_policy(PolicyScope::Project, Some("proj-1")).unwrap(),
            None
        );
    }

    // ---- count_invocations_since / sum_cost_since (task T18, BL-22) ----

    #[test]
    fn count_invocations_since_scopes_by_server_project_and_global() {
        let db = new_db();
        let server_a = add_server(&db);
        let server_b = add_server(&db);

        let mut in_proj = new_invocation(&server_a);
        in_proj.project_id = Some("proj-1".to_string());
        in_proj.started_at = 10_000;
        db.insert_invocation(in_proj).unwrap();

        let mut on_b = new_invocation(&server_b);
        on_b.started_at = 10_000;
        db.insert_invocation(on_b).unwrap();

        // Server-scoped: only server_a's row.
        assert_eq!(
            db.count_invocations_since(PolicyScope::Server, Some(&server_a), 0)
                .unwrap(),
            1
        );
        // Project-scoped: only the proj-1 row (server_b's call has no project_id).
        assert_eq!(
            db.count_invocations_since(PolicyScope::Project, Some("proj-1"), 0)
                .unwrap(),
            1
        );
        // Global: both rows.
        assert_eq!(
            db.count_invocations_since(PolicyScope::Global, None, 0)
                .unwrap(),
            2
        );
        // A `since_ms` after both rows excludes everything.
        assert_eq!(
            db.count_invocations_since(PolicyScope::Global, None, 20_000)
                .unwrap(),
            0
        );
    }

    #[test]
    fn sum_cost_since_coalesces_null_cost_to_zero() {
        let db = new_db();
        let server_id = add_server(&db);
        // new_invocation() sets cost_usd: Some(0.01) — override to NULL to prove the honest
        // "server never reported usage" default sums to 0.0, never SQL NULL/an error.
        let mut null_cost = new_invocation(&server_id);
        null_cost.cost_usd = None;
        null_cost.started_at = 1_000;
        db.insert_invocation(null_cost).unwrap();

        assert_eq!(
            db.sum_cost_since(PolicyScope::Server, Some(&server_id), 0)
                .unwrap(),
            0.0,
            "a NULL cost_usd must never make the sum NULL/error — it contributes 0.0"
        );

        let mut priced = new_invocation(&server_id);
        priced.cost_usd = Some(2.5);
        priced.started_at = 2_000;
        db.insert_invocation(priced).unwrap();

        assert_eq!(
            db.sum_cost_since(PolicyScope::Server, Some(&server_id), 0)
                .unwrap(),
            2.5,
            "only the reported cost counts; the NULL-cost row still contributes 0.0"
        );
    }
}
