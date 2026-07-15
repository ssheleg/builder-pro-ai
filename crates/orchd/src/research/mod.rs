//! Research-run persistence (S-IDEA spec §4/§6, schema v4, task T2). Sibling module to
//! `persistence`/`graph`/`mcp::registry` — builds its own `impl Db` block directly on
//! `persistence::Db`'s `conn()`/`now_ms()`/`OrchdPersistError` seam, exactly like those modules
//! do (see `persistence`'s own module doc comment for the pattern every domain module added
//! after the T2/T10 skeleton follows).
//!
//! `research_run` is a THIN table (D2): the actual ResearchArtifact IS the pre-existing
//! `mcp_artifact` row a run's `tools/call` produces (S-EXT schema v3) — this table is only the
//! provenance link (idea↔invocation↔artifact) plus a `pending`/`running`/`done`/`failed` status.
//! No blob duplication, one source of truth.
//!
//! This task (T2) implements the CRUD + the D11 boot-reconcile query only. The async run driver
//! that actually calls `mcp::invoke::call_tool` and drives `pending`→`running`→`done`/`failed`
//! lands in a later task (T4); this module's `set_research_run_*` methods are its building
//! blocks, each a single atomicity-preserving `UPDATE` (spec §4 "Transition atomicity" note).

use rusqlite::{Connection, OptionalExtension};
use uuid::Uuid;

use crate::persistence::{now_ms, Db, OrchdPersistError};

// ---- research_run.status <-> TEXT (spec §4 CHECK: `status IN
// ('pending','running','done','failed')`) — snake_case DB literal mapping, mirrors
// `mcp::registry`'s enum<->TEXT helpers' shape. ----

/// `research_run.status` (spec §5: the wire enum's camelCase repr — `pending`/`running`/`done`/
/// `failed` — is a later task's job, T3; this is the persistence-layer enum only).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ResearchStatus {
    Pending,
    Running,
    Done,
    Failed,
}

fn decode_research_status(s: &str) -> Result<ResearchStatus, OrchdPersistError> {
    match s {
        "pending" => Ok(ResearchStatus::Pending),
        "running" => Ok(ResearchStatus::Running),
        "done" => Ok(ResearchStatus::Done),
        "failed" => Ok(ResearchStatus::Failed),
        other => Err(OrchdPersistError::Io(format!(
            "corrupt research_run.status value: {other}"
        ))),
    }
}

/// Full `research_run` row, decoded (`status` TEXT decoded into [`ResearchStatus`] — mirrors
/// `mcp::McpServerRow`'s raw-row/decode shape).
#[derive(Debug, Clone, PartialEq)]
pub struct ResearchRunRow {
    pub id: String,
    pub idea_id: String,
    pub server_id: String,
    pub tool_name: String,
    pub args_json: String,
    pub status: ResearchStatus,
    pub invocation_id: Option<String>,
    pub artifact_id: Option<String>,
    pub error_kind: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

/// Input to [`Db::start_research_run`] (spec §5: mirrors `orchd-proto`'s future
/// `ResearchStartRun{idea_id, server_id, tool_name, args_json}` request fields 1:1 — T3's wire
/// conversion, when it lands, will be a plain field-for-field move, like `mcp`'s `NewMcpServer`
/// conversions).
#[derive(Debug, Clone)]
pub struct NewResearchRun {
    pub idea_id: String,
    pub server_id: String,
    pub tool_name: String,
    pub args_json: String,
}

/// Raw `research_run` row (text-encoded `status`) before decoding into [`ResearchRunRow`] —
/// mirrors `mcp::registry::McpServerRawRow`'s shape.
struct ResearchRunRawRow {
    id: String,
    idea_id: String,
    server_id: String,
    tool_name: String,
    args_json: String,
    status: String,
    invocation_id: Option<String>,
    artifact_id: Option<String>,
    error_kind: Option<String>,
    created_at: i64,
    updated_at: i64,
}

impl ResearchRunRawRow {
    fn from_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<ResearchRunRawRow> {
        Ok(ResearchRunRawRow {
            id: r.get(0)?,
            idea_id: r.get(1)?,
            server_id: r.get(2)?,
            tool_name: r.get(3)?,
            args_json: r.get(4)?,
            status: r.get(5)?,
            invocation_id: r.get(6)?,
            artifact_id: r.get(7)?,
            error_kind: r.get(8)?,
            created_at: r.get(9)?,
            updated_at: r.get(10)?,
        })
    }

    fn into_row(self) -> Result<ResearchRunRow, OrchdPersistError> {
        Ok(ResearchRunRow {
            id: self.id,
            idea_id: self.idea_id,
            server_id: self.server_id,
            tool_name: self.tool_name,
            args_json: self.args_json,
            status: decode_research_status(&self.status)?,
            invocation_id: self.invocation_id,
            artifact_id: self.artifact_id,
            error_kind: self.error_kind,
            created_at: self.created_at,
            updated_at: self.updated_at,
        })
    }
}

const RESEARCH_RUN_COLUMNS: &str = "id, idea_id, server_id, tool_name, args_json, status, \
     invocation_id, artifact_id, error_kind, created_at, updated_at";

fn load_research_run(conn: &Connection, id: &str) -> Result<ResearchRunRow, OrchdPersistError> {
    let sql = format!("SELECT {RESEARCH_RUN_COLUMNS} FROM research_run WHERE id = ?1");
    conn.query_row(&sql, rusqlite::params![id], ResearchRunRawRow::from_row)
        .optional()?
        .ok_or(OrchdPersistError::NotFound)?
        .into_row()
}

impl Db {
    /// `start_research_run` (spec §4 "Transition atomicity" note, §6 steps 1-2): ONE
    /// `unchecked_transaction()` — verifies BOTH the idea and the server exist, inserts
    /// `research_run{status:'pending'}`, and flips `idea.lifecycle` `captured`→`researching`
    /// ONLY if the idea is currently `captured` (spec §6 step 2: "leave later states" — an idea
    /// already `specced`/`in_dev`/`shipped`/`archived`, or already `researching` from an earlier
    /// concurrent run, keeps its own lifecycle). One transaction so a concurrent `DeleteIdea`
    /// (FK `idea_id` `ON DELETE CASCADE`) can't interleave a half-completed insert+flip. Unknown
    /// `idea_id`/`server_id` ⇒ `NotFound`.
    pub fn start_research_run(
        &self,
        new: NewResearchRun,
    ) -> Result<ResearchRunRow, OrchdPersistError> {
        let tx = self.conn().unchecked_transaction()?;

        let idea_exists = tx
            .query_row(
                "SELECT 1 FROM idea WHERE id = ?1",
                rusqlite::params![new.idea_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !idea_exists {
            return Err(OrchdPersistError::NotFound);
        }
        let server_exists = tx
            .query_row(
                "SELECT 1 FROM mcp_server WHERE id = ?1",
                rusqlite::params![new.server_id],
                |_| Ok(()),
            )
            .optional()?
            .is_some();
        if !server_exists {
            return Err(OrchdPersistError::NotFound);
        }

        let id = Uuid::new_v4().to_string();
        let now = now_ms();
        tx.execute(
            "INSERT INTO research_run
               (id, idea_id, server_id, tool_name, args_json, status, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'pending', ?6, ?6)",
            rusqlite::params![
                id,
                new.idea_id,
                new.server_id,
                new.tool_name,
                new.args_json,
                now
            ],
        )?;

        // Only-if-captured idea flip (spec §6 step 2) — same tx as the insert above, so the
        // "Transition atomicity" note's concurrent-DeleteIdea guard covers this write too.
        tx.execute(
            "UPDATE idea SET lifecycle = 'researching', updated_at = ?2
             WHERE id = ?1 AND lifecycle = 'captured'",
            rusqlite::params![new.idea_id, now],
        )?;

        let row = load_research_run(&tx, &id)?;
        tx.commit()?;
        Ok(row)
    }

    /// `set_research_run_running` (spec §6 step 3a). Unknown `id` ⇒ `NotFound`.
    pub fn set_research_run_running(&self, id: &str) -> Result<ResearchRunRow, OrchdPersistError> {
        let tx = self.conn().unchecked_transaction()?;
        let changed = tx.execute(
            "UPDATE research_run SET status = 'running', updated_at = ?2 WHERE id = ?1",
            rusqlite::params![id, now_ms()],
        )?;
        if changed == 0 {
            return Err(OrchdPersistError::NotFound);
        }
        let row = load_research_run(&tx, id)?;
        tx.commit()?;
        Ok(row)
    }

    /// `set_research_run_done` (spec §4 "Transition atomicity" note): status + `invocation_id` +
    /// `artifact_id` set together in ONE `UPDATE` — never two statements (would momentarily
    /// violate the CHECK `(status='done') = (artifact_id IS NOT NULL)`). Also clears any stale
    /// `error_kind`, keeping the row internally consistent. Unknown `id` ⇒ `NotFound`.
    pub fn set_research_run_done(
        &self,
        id: &str,
        invocation_id: &str,
        artifact_id: &str,
    ) -> Result<ResearchRunRow, OrchdPersistError> {
        let tx = self.conn().unchecked_transaction()?;
        let changed = tx.execute(
            "UPDATE research_run
               SET status = 'done', invocation_id = ?2, artifact_id = ?3, error_kind = NULL,
                   updated_at = ?4
             WHERE id = ?1",
            rusqlite::params![id, invocation_id, artifact_id, now_ms()],
        )?;
        if changed == 0 {
            return Err(OrchdPersistError::NotFound);
        }
        let row = load_research_run(&tx, id)?;
        tx.commit()?;
        Ok(row)
    }

    /// `set_research_run_failed` (spec §4 "Transition atomicity" note): status + `error_kind` set
    /// together in ONE `UPDATE`; `artifact_id` is force-`NULL`ed in the SAME statement so the
    /// CHECK `(status='done') = (artifact_id IS NOT NULL)` holds unconditionally. `invocation_id`
    /// is left untouched — spec §6 step 3d: best-effort partial provenance, some failure kinds
    /// carry one, others don't; this method doesn't decide that (its caller, a later task's run
    /// driver, does). Unknown `id` ⇒ `NotFound`.
    pub fn set_research_run_failed(
        &self,
        id: &str,
        error_kind: &str,
    ) -> Result<ResearchRunRow, OrchdPersistError> {
        let tx = self.conn().unchecked_transaction()?;
        let changed = tx.execute(
            "UPDATE research_run
               SET status = 'failed', error_kind = ?2, artifact_id = NULL, updated_at = ?3
             WHERE id = ?1",
            rusqlite::params![id, error_kind, now_ms()],
        )?;
        if changed == 0 {
            return Err(OrchdPersistError::NotFound);
        }
        let row = load_research_run(&tx, id)?;
        tx.commit()?;
        Ok(row)
    }

    /// `list_research_runs` (spec §5: "runs for an idea, newest first"). Tie-break on `id` DESC
    /// keeps the order fully deterministic when two runs share the same millisecond
    /// `created_at` (fast test/CI clocks) — `created_at` alone doesn't guarantee that.
    pub fn list_research_runs(
        &self,
        idea_id: &str,
    ) -> Result<Vec<ResearchRunRow>, OrchdPersistError> {
        let sql = format!(
            "SELECT {RESEARCH_RUN_COLUMNS} FROM research_run
             WHERE idea_id = ?1 ORDER BY created_at DESC, id DESC"
        );
        let mut stmt = self.conn().prepare(&sql)?;
        let raw_rows: Vec<ResearchRunRawRow> = stmt
            .query_map(rusqlite::params![idea_id], ResearchRunRawRow::from_row)?
            .collect::<Result<_, _>>()?;
        raw_rows.into_iter().map(|r| r.into_row()).collect()
    }

    /// `get_research_run` (spec §5): `Ok(None)` for an unknown `id` — unlike most single-row
    /// getters in this crate, this one has no "the caller already knows this id exists"
    /// precondition (a `ResearchGetRun` client could race a delete), so `Option` is the honest
    /// degradation rather than `NotFound`.
    pub fn get_research_run(&self, id: &str) -> Result<Option<ResearchRunRow>, OrchdPersistError> {
        let sql = format!("SELECT {RESEARCH_RUN_COLUMNS} FROM research_run WHERE id = ?1");
        self.conn()
            .query_row(&sql, rusqlite::params![id], ResearchRunRawRow::from_row)
            .optional()?
            .map(ResearchRunRawRow::into_row)
            .transpose()
    }

    /// `reconcile_interrupted_research_runs` (S-IDEA spec D11, boot-reconcile): flips every
    /// non-terminal (`pending`/`running`) run to `failed{interrupted}` in ONE `UPDATE`. The §4
    /// CHECK's `artifact_id` side stays satisfied without an explicit NULL-out here — a row in
    /// `pending`/`running` state already has `artifact_id IS NULL` per that same CHECK, since
    /// only `status='done'` may have one. This is the AUTHORITATIVE backstop for the async run
    /// driver's detached `tokio::spawn` task (a later task, T4), which is NOT tracked by the
    /// shutdown drain's `JoinSet` and so can be lost outright on crash/restart/drain mid-run —
    /// called from `boot::run` right after the DB opens (mirrors `ensure_global_ruleset`'s
    /// "ensured at every boot" placement). Idempotent: a second call on an already-reconciled DB
    /// touches zero rows. Returns the affected row count so the caller can log it.
    pub fn reconcile_interrupted_research_runs(&self) -> Result<usize, OrchdPersistError> {
        let changed = self.conn().execute(
            "UPDATE research_run SET status = 'failed', error_kind = 'interrupted', updated_at = ?1
             WHERE status IN ('pending', 'running')",
            rusqlite::params![now_ms()],
        )?;
        Ok(changed)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use bpa_orchd_proto::IdeaLifecycle;

    use super::*;
    use crate::mcp::{McpAuthKind, McpScope, McpTransport, NewMcpServer};

    fn new_db() -> Db {
        Db::open_in_memory().unwrap()
    }

    /// Orphan idea (no project) at the given lifecycle — `Captured` is `create_idea`'s own DB
    /// default; any other lifecycle is set via a follow-up `set_idea_lifecycle` call.
    fn new_idea(db: &Db, lifecycle: IdeaLifecycle) -> String {
        let idea = db.create_idea(None, "An idea", "").unwrap();
        if lifecycle == IdeaLifecycle::Captured {
            return idea.id;
        }
        db.set_idea_lifecycle(&idea.id, lifecycle).unwrap().id
    }

    /// Global stdio MCP server (no project scoping needed for these tests) — mirrors
    /// `mcp::registry`'s own `stdio_server` test helper.
    fn new_server(db: &Db) -> String {
        db.add_mcp_server(NewMcpServer {
            name: "prowl".to_string(),
            transport: McpTransport::Stdio,
            url: None,
            command: Some("/usr/local/bin/prowl-mcp".to_string()),
            args: vec![],
            env: BTreeMap::new(),
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

    fn new_run(idea_id: &str, server_id: &str) -> NewResearchRun {
        NewResearchRun {
            idea_id: idea_id.to_string(),
            server_id: server_id.to_string(),
            tool_name: "search".to_string(),
            args_json: "{}".to_string(),
        }
    }

    fn idea_lifecycle(db: &Db, id: &str) -> String {
        db.conn()
            .query_row("SELECT lifecycle FROM idea WHERE id = ?1", [id], |r| {
                r.get(0)
            })
            .unwrap()
    }

    // ---- schema v4 (fresh DB) ----

    #[test]
    fn fresh_db_has_research_run_table_at_schema_v4() {
        let db = new_db();
        let version: i64 = db
            .conn()
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(version, 4);
        let exists: bool = db
            .conn()
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'research_run'",
                [],
                |_| Ok(true),
            )
            .unwrap_or(false);
        assert!(exists, "missing table research_run");
    }

    // ---- start_research_run ----

    #[test]
    fn start_research_run_inserts_pending_and_flips_captured_idea_to_researching() {
        let db = new_db();
        let idea_id = new_idea(&db, IdeaLifecycle::Captured);
        let server_id = new_server(&db);

        let run = db
            .start_research_run(new_run(&idea_id, &server_id))
            .unwrap();

        assert_eq!(run.status, ResearchStatus::Pending);
        assert_eq!(run.idea_id, idea_id);
        assert_eq!(run.server_id, server_id);
        assert_eq!(run.tool_name, "search");
        assert_eq!(run.args_json, "{}");
        assert!(run.invocation_id.is_none());
        assert!(run.artifact_id.is_none());
        assert!(run.error_kind.is_none());

        assert_eq!(idea_lifecycle(&db, &idea_id), "researching");
    }

    #[test]
    fn start_research_run_does_not_change_a_specced_ideas_lifecycle() {
        let db = new_db();
        let idea_id = new_idea(&db, IdeaLifecycle::Specced);
        let server_id = new_server(&db);

        db.start_research_run(new_run(&idea_id, &server_id))
            .unwrap();

        assert_eq!(idea_lifecycle(&db, &idea_id), "specced");
    }

    #[test]
    fn start_research_run_does_not_change_an_archived_ideas_lifecycle() {
        let db = new_db();
        let idea_id = new_idea(&db, IdeaLifecycle::Archived);
        let server_id = new_server(&db);

        db.start_research_run(new_run(&idea_id, &server_id))
            .unwrap();

        assert_eq!(idea_lifecycle(&db, &idea_id), "archived");
    }

    #[test]
    fn start_research_run_unknown_idea_is_not_found() {
        let db = new_db();
        let server_id = new_server(&db);
        let err = db
            .start_research_run(new_run("no-such-idea", &server_id))
            .unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound));
    }

    #[test]
    fn start_research_run_unknown_server_is_not_found() {
        let db = new_db();
        let idea_id = new_idea(&db, IdeaLifecycle::Captured);
        let err = db
            .start_research_run(new_run(&idea_id, "no-such-server"))
            .unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound));
    }

    // ---- set_research_run_running / _done / _failed ----

    #[test]
    fn set_research_run_running_transitions_status() {
        let db = new_db();
        let idea_id = new_idea(&db, IdeaLifecycle::Captured);
        let server_id = new_server(&db);
        let run = db
            .start_research_run(new_run(&idea_id, &server_id))
            .unwrap();

        let updated = db.set_research_run_running(&run.id).unwrap();
        assert_eq!(updated.status, ResearchStatus::Running);
    }

    #[test]
    fn set_research_run_done_sets_status_and_artifact_and_invocation() {
        let db = new_db();
        let idea_id = new_idea(&db, IdeaLifecycle::Captured);
        let server_id = new_server(&db);
        let run = db
            .start_research_run(new_run(&idea_id, &server_id))
            .unwrap();

        let updated = db.set_research_run_done(&run.id, "inv-1", "art-1").unwrap();

        assert_eq!(updated.status, ResearchStatus::Done);
        assert_eq!(updated.invocation_id.as_deref(), Some("inv-1"));
        assert_eq!(updated.artifact_id.as_deref(), Some("art-1"));
        assert!(updated.error_kind.is_none());

        let reloaded = db.get_research_run(&run.id).unwrap().unwrap();
        assert_eq!(reloaded, updated);
    }

    #[test]
    fn set_research_run_failed_sets_status_and_error_kind_artifact_stays_null() {
        let db = new_db();
        let idea_id = new_idea(&db, IdeaLifecycle::Captured);
        let server_id = new_server(&db);
        let run = db
            .start_research_run(new_run(&idea_id, &server_id))
            .unwrap();

        let updated = db.set_research_run_failed(&run.id, "timeout").unwrap();

        assert_eq!(updated.status, ResearchStatus::Failed);
        assert_eq!(updated.error_kind.as_deref(), Some("timeout"));
        assert!(updated.artifact_id.is_none());
    }

    #[test]
    fn set_research_run_running_unknown_id_is_not_found() {
        let db = new_db();
        let err = db.set_research_run_running("no-such-run").unwrap_err();
        assert!(matches!(err, OrchdPersistError::NotFound));
    }

    // ---- CHECK constraint (persistence-level, direct SQL) ----

    #[test]
    fn check_constraint_rejects_a_done_row_with_null_artifact_id() {
        let db = new_db();
        let idea_id = new_idea(&db, IdeaLifecycle::Captured);
        let server_id = new_server(&db);
        let now = now_ms();

        let err = db
            .conn()
            .execute(
                "INSERT INTO research_run
                   (id, idea_id, server_id, tool_name, args_json, status, artifact_id,
                    created_at, updated_at)
                 VALUES ('r1', ?1, ?2, 'search', '{}', 'done', NULL, ?3, ?3)",
                rusqlite::params![idea_id, server_id, now],
            )
            .unwrap_err();
        assert!(matches!(err, rusqlite::Error::SqliteFailure(_, _)));
    }

    // ---- list_research_runs / get_research_run ----

    #[test]
    fn list_research_runs_orders_newest_first() {
        let db = new_db();
        let idea_id = new_idea(&db, IdeaLifecycle::Captured);
        let server_id = new_server(&db);

        let run1 = db
            .start_research_run(new_run(&idea_id, &server_id))
            .unwrap();
        // Backdate explicitly so ordering doesn't depend on two `now_ms()` calls landing in
        // different milliseconds (fast CI clocks can tie).
        db.conn()
            .execute(
                "UPDATE research_run SET created_at = 1000 WHERE id = ?1",
                rusqlite::params![run1.id],
            )
            .unwrap();
        let run2 = db
            .start_research_run(new_run(&idea_id, &server_id))
            .unwrap();
        db.conn()
            .execute(
                "UPDATE research_run SET created_at = 2000 WHERE id = ?1",
                rusqlite::params![run2.id],
            )
            .unwrap();

        let runs = db.list_research_runs(&idea_id).unwrap();
        assert_eq!(runs.len(), 2);
        assert_eq!(
            runs[0].id, run2.id,
            "newest (created_at=2000) must come first"
        );
        assert_eq!(runs[1].id, run1.id);
    }

    #[test]
    fn list_research_runs_only_returns_the_given_ideas_runs() {
        let db = new_db();
        let idea1 = new_idea(&db, IdeaLifecycle::Captured);
        let idea2 = new_idea(&db, IdeaLifecycle::Captured);
        let server_id = new_server(&db);

        db.start_research_run(new_run(&idea1, &server_id)).unwrap();
        db.start_research_run(new_run(&idea2, &server_id)).unwrap();

        let runs = db.list_research_runs(&idea1).unwrap();
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].idea_id, idea1);
    }

    #[test]
    fn get_research_run_returns_none_for_unknown_id() {
        let db = new_db();
        assert!(db.get_research_run("no-such-run").unwrap().is_none());
    }

    #[test]
    fn get_research_run_returns_the_row() {
        let db = new_db();
        let idea_id = new_idea(&db, IdeaLifecycle::Captured);
        let server_id = new_server(&db);
        let run = db
            .start_research_run(new_run(&idea_id, &server_id))
            .unwrap();

        let fetched = db.get_research_run(&run.id).unwrap().unwrap();
        assert_eq!(fetched, run);
    }

    // ---- reconcile_interrupted_research_runs (D11 boot-reconcile) ----

    #[test]
    fn reconcile_interrupted_research_runs_flips_pending_and_running_leaves_done_and_failed() {
        let db = new_db();
        let idea_id = new_idea(&db, IdeaLifecycle::Captured);
        let server_id = new_server(&db);

        let pending = db
            .start_research_run(new_run(&idea_id, &server_id))
            .unwrap();
        let running = db
            .start_research_run(new_run(&idea_id, &server_id))
            .unwrap();
        db.set_research_run_running(&running.id).unwrap();
        let done = db
            .start_research_run(new_run(&idea_id, &server_id))
            .unwrap();
        db.set_research_run_done(&done.id, "inv", "art").unwrap();
        let failed = db
            .start_research_run(new_run(&idea_id, &server_id))
            .unwrap();
        db.set_research_run_failed(&failed.id, "tool_error")
            .unwrap();

        let count = db.reconcile_interrupted_research_runs().unwrap();
        assert_eq!(count, 2);

        let pending_after = db.get_research_run(&pending.id).unwrap().unwrap();
        assert_eq!(pending_after.status, ResearchStatus::Failed);
        assert_eq!(pending_after.error_kind.as_deref(), Some("interrupted"));

        let running_after = db.get_research_run(&running.id).unwrap().unwrap();
        assert_eq!(running_after.status, ResearchStatus::Failed);
        assert_eq!(running_after.error_kind.as_deref(), Some("interrupted"));

        let done_after = db.get_research_run(&done.id).unwrap().unwrap();
        assert_eq!(done_after.status, ResearchStatus::Done);
        assert_eq!(done_after.error_kind, None);
        assert_eq!(done_after.artifact_id.as_deref(), Some("art"));

        let failed_after = db.get_research_run(&failed.id).unwrap().unwrap();
        assert_eq!(failed_after.status, ResearchStatus::Failed);
        assert_eq!(
            failed_after.error_kind.as_deref(),
            Some("tool_error"),
            "an already-failed row's error_kind must NOT be clobbered by reconcile"
        );
    }

    #[test]
    fn reconcile_interrupted_research_runs_on_empty_db_returns_zero() {
        let db = new_db();
        assert_eq!(db.reconcile_interrupted_research_runs().unwrap(), 0);
    }

    // ---- CASCADE deletes ----

    #[test]
    fn deleting_the_idea_cascade_deletes_its_research_runs() {
        let db = new_db();
        let idea_id = new_idea(&db, IdeaLifecycle::Captured);
        let server_id = new_server(&db);
        let run = db
            .start_research_run(new_run(&idea_id, &server_id))
            .unwrap();

        db.delete_idea(&idea_id).unwrap();

        assert!(db.get_research_run(&run.id).unwrap().is_none());
    }

    #[test]
    fn deleting_the_server_cascade_deletes_its_research_runs() {
        let db = new_db();
        let idea_id = new_idea(&db, IdeaLifecycle::Captured);
        let server_id = new_server(&db);
        let run = db
            .start_research_run(new_run(&idea_id, &server_id))
            .unwrap();

        db.delete_mcp_server(&server_id).unwrap();

        assert!(db.get_research_run(&run.id).unwrap().is_none());
    }
}
