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
//! T2 implements the CRUD + the D11 boot-reconcile query (the `impl Db` block below). Task T4
//! adds the async run driver ([`run_research`]) that actually calls `mcp::invoke::call_tool` and
//! drives `pending`→`running`→`done`/`failed` — [`Db::set_research_run_running`]/
//! [`Db::set_research_run_done`]/[`Db::set_research_run_failed`] are its building blocks, each a
//! single atomicity-preserving `UPDATE` (spec §4 "Transition atomicity" note) — plus [`start_run`]
//! (spec §6 steps 1-3, D3), the entry point `socket_server`'s `ResearchStartRun` dispatch arm
//! calls (a later task, T5).

use std::future::Future;
use std::sync::Arc;

use bpa_daemon_core::broadcast::Broadcaster;
use bpa_orchd_proto::{OrchdFrame, OrchdPush};
use rusqlite::{Connection, OptionalExtension};
use tokio::sync::Mutex;
use uuid::Uuid;

use crate::mcp::{McpServerRow, OrchdMcpError, ToolCaller};
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

// ================================================================================
// ---- wire conversions (S-IDEA spec §5, task T5): this module's persistence-layer
// `ResearchRunRow`/`ResearchStatus` -> `bpa_orchd_proto`'s wire `ResearchRun`/`ResearchStatus`
// (the entity task T3 already defined) — mirrors `mcp::mod`'s own
// `McpServerRow -> bpa_orchd_proto::McpServer` conversion block byte-for-byte (S-EXT task T6):
// every field is a plain field-for-field move (both sides mirror the same spec §4 DDL columns —
// the wire `ResearchRun`'s own doc comment says so explicitly), `i64` timestamps pass straight
// through (`#[ts(type = "number")]` on the wire side already handles the TS-side cast, nothing to
// do here). Referenced fully-qualified (`bpa_orchd_proto::ResearchStatus`/`bpa_orchd_proto::
// ResearchRun`) rather than imported bare, since this module's OWN `ResearchStatus` shares the
// exact same short name — a bare import of both would collide (same rationale `mcp::mod`'s own
// comment gives for `McpTransport`/`McpScope`/`McpAuthKind`). `socket_server`'s dispatch arms
// (task T5) call `.into()` on a loaded [`ResearchRunRow`] (or `Vec<ResearchRunRow>` via
// `.map(Into::into)`), exactly like every other read/mutate dispatch arm in this crate.
// ================================================================================

impl From<ResearchStatus> for bpa_orchd_proto::ResearchStatus {
    fn from(s: ResearchStatus) -> Self {
        match s {
            ResearchStatus::Pending => bpa_orchd_proto::ResearchStatus::Pending,
            ResearchStatus::Running => bpa_orchd_proto::ResearchStatus::Running,
            ResearchStatus::Done => bpa_orchd_proto::ResearchStatus::Done,
            ResearchStatus::Failed => bpa_orchd_proto::ResearchStatus::Failed,
        }
    }
}

impl From<ResearchRunRow> for bpa_orchd_proto::ResearchRun {
    fn from(r: ResearchRunRow) -> Self {
        bpa_orchd_proto::ResearchRun {
            id: r.id,
            idea_id: r.idea_id,
            server_id: r.server_id,
            tool_name: r.tool_name,
            args_json: r.args_json,
            status: r.status.into(),
            invocation_id: r.invocation_id,
            artifact_id: r.artifact_id,
            error_kind: r.error_kind,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
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

// ================================================================================
// ---- the async run driver + its entry point (S-IDEA spec §6, D3/D9/D10/D11, task T4) ----
// ================================================================================

/// The async research-run driver (spec §6 steps 3a-3d, D3/D10/D11): a FREE async fn — NOT a
/// method on [`Db`] — so `#[cfg(test)]` can `.await` it directly with a fake `connect_fn` and
/// assert its effects with no `tokio::spawn` involved at all; [`start_run`] is the only caller
/// that actually spawns it, fire-and-forget, in production.
///
/// Takes an OWNED `Arc<Mutex<Db>>` + a cloned [`Broadcaster<OrchdFrame>`] — never a bare `&Db` or
/// `&Broadcaster` — because a detached `tokio::spawn`ed task cannot borrow anything from its
/// caller's stack frame; it must own (or `Arc`-share) everything it touches for its own
/// `'static` lifetime.
///
/// 3-phase, mirroring `mcp::invoke::call_tool` itself (spec D3: "already lock-safe — never holds
/// the DB lock across the network await", S-EXT T6 lesson): (1) lock → [`Db::set_research_run_running`]
/// → broadcast `ResearchRunsChanged` → UNLOCK, all before phase 2 ever starts; (2)
/// `mcp::invoke::call_tool` — NO `Db` guard is held anywhere across this `.await` (`call_tool`
/// re-locks internally, in its own short phases, exactly as its own doc comment describes); (3)
/// lock → [`Db::set_research_run_done`] (success) or [`Db::set_research_run_failed`] (failure,
/// `error_kind` from [`classify_run_error`]) → broadcast → unlock. Every transition this function
/// drives broadcasts `ResearchRunsChanged{idea_id: Some(idea_id)}` — a client watching one idea's
/// research pane sees `pending`→`running`→`done`/`failed` live (D3's "honest research pane": no
/// token streaming, just accurate status pushes).
///
/// If phase 1's `set_research_run_running` itself fails (e.g. the row was cascade-deleted out
/// from under this task — a concurrent `DeleteIdea`/`McpDeleteServer` racing the moment
/// [`start_run`] spawned this task, FK `ON DELETE CASCADE` on both `idea_id` and `server_id`),
/// the driver returns immediately WITHOUT ever touching the network: there is no row left to
/// carry a `done`/`failed` verdict, and dispatching the call anyway could needlessly consume a
/// spend/rate policy cap for a result nobody can ever read.
///
/// Structured `tracing` only — `run_id`/`idea_id`/`status`/`error_kind`, NEVER `args_json`, a
/// secret, or tool output (mirrors `mcp::invoke`'s own `record_failed_invocation` discipline).
///
/// `#[allow(clippy::too_many_arguments)]`: every parameter is a distinct, load-bearing piece of
/// the run's own identity/scope (`run_id`/`idea_id`/`server_id`/`tool_name`/`args_json`/
/// `project_id`) plus the two shared handles (`db`/`broadcaster`) plus the test/production
/// session seam (`connect_fn`) — mirrors `persistence::Db::create_task`'s own scoped allow
/// (same crate, same rationale: a flat field list, not a bundleable struct, since [`start_run`]
/// (the only production caller) is itself unpacking a freshly-loaded [`ResearchRunRow`] field by
/// field into this call).
#[allow(clippy::too_many_arguments)]
pub async fn run_research<F, Fut, S>(
    db: Arc<Mutex<Db>>,
    broadcaster: Broadcaster<OrchdFrame>,
    run_id: String,
    idea_id: String,
    server_id: String,
    tool_name: String,
    args_json: String,
    project_id: Option<String>,
    connect_fn: F,
) where
    F: FnOnce(McpServerRow, Option<String>) -> Fut,
    Fut: Future<Output = Result<S, bpa_mcp::McpError>>,
    S: ToolCaller,
{
    {
        let guard = db.lock().await;
        if let Err(e) = guard.set_research_run_running(&run_id) {
            tracing::warn!(
                run_id = %run_id,
                idea_id = %idea_id,
                error = %e,
                "research: run row gone before dispatch, aborting without calling the tool"
            );
            return;
        }
    }
    broadcaster.broadcast(OrchdFrame::Push(OrchdPush::ResearchRunsChanged {
        idea_id: Some(idea_id.clone()),
    }));

    // ---- Phase 2: network. NO `Db`/`MutexGuard` reference is alive here — `call_tool` locks
    // internally, in its own short phases, around its own network round-trip. ----
    let outcome = crate::mcp::invoke::call_tool(
        &db, &server_id, &tool_name, &args_json, project_id, connect_fn,
    )
    .await;

    match outcome {
        Ok(result) => {
            let guard = db.lock().await;
            match guard.set_research_run_done(&run_id, &result.invocation_id, &result.artifact_id) {
                Ok(_) => tracing::info!(
                    run_id = %run_id,
                    idea_id = %idea_id,
                    status = "done",
                    "research: run completed"
                ),
                Err(e) => tracing::warn!(
                    run_id = %run_id,
                    idea_id = %idea_id,
                    error = %e,
                    "research: failed to persist the done transition"
                ),
            }
        }
        Err(e) => {
            let error_kind = classify_run_error(&e);
            let guard = db.lock().await;
            match guard.set_research_run_failed(&run_id, error_kind) {
                Ok(_) => tracing::warn!(
                    run_id = %run_id,
                    idea_id = %idea_id,
                    status = "failed",
                    error_kind,
                    "research: run failed"
                ),
                Err(persist_err) => tracing::warn!(
                    run_id = %run_id,
                    idea_id = %idea_id,
                    error = %persist_err,
                    "research: failed to persist the failed transition"
                ),
            }
        }
    }

    broadcaster.broadcast(OrchdFrame::Push(OrchdPush::ResearchRunsChanged {
        idea_id: Some(idea_id),
    }));
}

/// Maps a terminal [`mcp::invoke::call_tool`](crate::mcp::invoke::call_tool) failure to the
/// `research_run.error_kind` TEXT stored on the `failed` transition (spec §6 step 3d). Every arm
/// returns a small FIXED label — the underlying error's own message text (which, for `Secret`/
/// `ToolError`/`Auth`, could echo server- or Keychain-derived content) is always discarded, never
/// stored: `error_kind` must NEVER carry args, a secret, or tool output.
fn classify_run_error(err: &OrchdMcpError) -> &'static str {
    match err {
        OrchdMcpError::PolicyCapExceeded(_) => "policy_cap_exceeded",
        OrchdMcpError::ToolDisabled => "tool_disabled",
        OrchdMcpError::ConsentRequired => "consent_required",
        // Covers `Transport`/`Protocol`/`Timeout`/`ToolError`/`Auth` uniformly — the SAME mapping
        // `mcp::invoke`'s own failed-`mcp_invocation` bookkeeping uses, so a connect-timeout (D12)
        // or a `tools/call` timeout both classify as `"timeout"` here too.
        OrchdMcpError::Mcp(e) => crate::mcp::invoke::classify_error_kind(e),
        OrchdMcpError::Secret(_) => "secret_error",
        OrchdMcpError::Persist(_) => "internal_error",
    }
}

/// `ResearchStartRun`'s entry point (spec §6 steps 1-3, D3): an `async fn` that acquires the
/// shared `Db` lock exactly like every other verb in `socket_server::dispatch` (`.lock().await`),
/// so `socket_server`'s dispatch arm (a later task, T5) calls it with `.await` before immediately
/// replying with the `pending` row it returns.
///
/// Sequence: (1) `db.lock().await` — the SAME acquisition every other dispatch-reachable op in
/// this crate uses: it never spuriously rejects a valid request under contention (a T4-review fix
/// replaced an earlier non-`async`, `try_lock`-on-contention→`Io` shape — under the multi-threaded
/// runtime, one task per connection plus concurrent run drivers all touching this same
/// `Arc<Mutex<Db>>` at phases 1/3 makes contention genuinely reachable, so a `try_lock` here would
/// have been the only dispatch-reachable op that could reject a valid request; a short `.await`
/// wait is correct). (2) [`Db::start_research_run`] (T2's one-tx insert+idea-flip). (3) resolve
/// the idea's OWN `project_id` (spec §6 step b: "`project_id = idea.project_id`" — the scope
/// `mcp::invoke::call_tool` authorizes/spends against) under the SAME held guard. (4) drop the
/// guard via block-scope, THEN `tokio::spawn` [`run_research`] against the PRODUCTION connect_fn
/// ([`crate::mcp::connect_session`]) — the driver reacquires the lock cleanly in its own phase 1.
/// Fire-and-forget: this function returns the `pending` row immediately; the run's terminal state
/// arrives later via the `ResearchRunsChanged` push `run_research` fires on every transition.
/// [`Db::reconcile_interrupted_research_runs`] (T2, D11) is the crash/restart backstop for this
/// detached, drain-untracked task.
pub async fn start_run(
    db: &Arc<Mutex<Db>>,
    broadcaster: &Broadcaster<OrchdFrame>,
    new: NewResearchRun,
) -> Result<ResearchRunRow, OrchdPersistError> {
    let (row, project_id) = {
        let guard = db.lock().await;
        let row = guard.start_research_run(new)?;
        let project_id = resolve_idea_project_id(guard.conn(), &row.idea_id)?;
        (row, project_id)
    };

    tokio::spawn(run_research(
        db.clone(),
        broadcaster.clone(),
        row.id.clone(),
        row.idea_id.clone(),
        row.server_id.clone(),
        row.tool_name.clone(),
        row.args_json.clone(),
        project_id,
        crate::mcp::connect_session,
    ));

    Ok(row)
}

/// Resolve `idea_id`'s OWN `project_id` (spec §6 step b). [`Db::start_research_run`]'s own
/// transaction (just committed, above) already proved the idea exists at insert time, so this is
/// expected to find a row; if it was deleted in the split-second window since (a concurrent
/// `DeleteIdea`, which `ON DELETE CASCADE`s the just-inserted `research_run` row too), degrading
/// to `None` (no project scope) rather than failing the whole [`start_run`] call is the honest
/// choice — the `pending` row already exists and is about to be returned to the caller;
/// [`run_research`]'s own phase-1 `set_research_run_running` independently re-checks the row
/// still exists and aborts cleanly if it doesn't.
fn resolve_idea_project_id(
    conn: &Connection,
    idea_id: &str,
) -> Result<Option<String>, OrchdPersistError> {
    let project_id: Option<Option<String>> = conn
        .query_row(
            "SELECT project_id FROM idea WHERE id = ?1",
            rusqlite::params![idea_id],
            |r| r.get(0),
        )
        .optional()?;
    Ok(project_id.flatten())
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

    // ---- schema v4 table, still present in the now-current schema v7 (fresh DB) ----

    #[test]
    fn fresh_db_has_research_run_table_at_schema_v7() {
        let db = new_db();
        let version: i64 = db
            .conn()
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .unwrap();
        // SCN-051 bumped SCHEMA_VERSION 4->5 (additive, `task.priority` only), SCN-054 bumped it
        // 5->6 (additive, the `doc` table only), and SW1 bumped it 6->7 (additive, the `workflow`
        // table only); the v4 `research_run` table this test checks for is unaffected.
        assert_eq!(version, 7);
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

    // ---- wire conversions (task T5) ----

    #[test]
    fn research_run_row_into_proto_research_run_maps_every_field_1to1() {
        let db = new_db();
        let idea_id = new_idea(&db, IdeaLifecycle::Captured);
        let server_id = new_server(&db);
        let row = db
            .start_research_run(new_run(&idea_id, &server_id))
            .unwrap();
        let row = db.set_research_run_done(&row.id, "inv-1", "art-1").unwrap();

        let wire: bpa_orchd_proto::ResearchRun = row.clone().into();

        assert_eq!(wire.id, row.id);
        assert_eq!(wire.idea_id, row.idea_id);
        assert_eq!(wire.server_id, row.server_id);
        assert_eq!(wire.tool_name, row.tool_name);
        assert_eq!(wire.args_json, row.args_json);
        assert_eq!(wire.status, bpa_orchd_proto::ResearchStatus::Done);
        assert_eq!(wire.invocation_id, row.invocation_id);
        assert_eq!(wire.artifact_id, row.artifact_id);
        assert_eq!(wire.error_kind, row.error_kind);
        assert_eq!(wire.created_at, row.created_at);
        assert_eq!(wire.updated_at, row.updated_at);
    }

    #[test]
    fn research_status_into_proto_maps_every_variant() {
        assert_eq!(
            bpa_orchd_proto::ResearchStatus::from(ResearchStatus::Pending),
            bpa_orchd_proto::ResearchStatus::Pending
        );
        assert_eq!(
            bpa_orchd_proto::ResearchStatus::from(ResearchStatus::Running),
            bpa_orchd_proto::ResearchStatus::Running
        );
        assert_eq!(
            bpa_orchd_proto::ResearchStatus::from(ResearchStatus::Done),
            bpa_orchd_proto::ResearchStatus::Done
        );
        assert_eq!(
            bpa_orchd_proto::ResearchStatus::from(ResearchStatus::Failed),
            bpa_orchd_proto::ResearchStatus::Failed
        );
    }
}

// ================================================================================
// ---- the async run driver + start_run (S-IDEA spec §6/§8, D3/D10/D11, task T4) ----
// ================================================================================

/// Hermetic (S-IDEA spec §8: fake session, NO network/spawn observed in assertions) — reuses the
/// SAME `crate::mcp::test_support::FakeSession` seam `mcp::invoke`'s own tests use, an
/// `Arc<Mutex<Db>>` + a real `Broadcaster` (never a fake broadcaster — the whole point is proving
/// the real fan-out registry receives the push), and calls [`run_research`] directly with
/// `.await` (no `tokio::spawn` needed to exercise it — that's the point of it being a free fn).
#[cfg(test)]
mod driver_tests {
    use std::collections::BTreeMap;
    use std::sync::atomic::{AtomicUsize, Ordering};

    use bpa_mcp::{McpError, McpToolResult};
    use serde_json::json;
    use tokio::sync::mpsc;

    use super::*;
    use crate::mcp::test_support::{FakeCallOutcome, FakeSession};
    use crate::mcp::{McpAuthKind, McpScope, McpTransport, NewMcpServer, NewMcpTool};
    use crate::persistence::NewPolicy;

    fn new_shared_db() -> Arc<Mutex<Db>> {
        Arc::new(Mutex::new(Db::open_in_memory().unwrap()))
    }

    /// Orphan idea (no project — `project_id: None` on the returned run driver call is the
    /// simplest honest case; `start_run`'s own project-resolution path is exercised separately by
    /// `start_run_returns_pending_row_and_flips_idea_to_researching`) + an enabled `search` tool
    /// on an HTTP server, mirroring `mcp::invoke`'s own `add_server`/`add_tool` test helpers.
    async fn seed_idea_and_tool(db: &Arc<Mutex<Db>>) -> (String, String) {
        let guard = db.lock().await;
        let idea = guard.create_idea(None, "An idea", "").unwrap();
        let server = guard
            .add_mcp_server(NewMcpServer {
                name: "prowl".to_string(),
                transport: McpTransport::Http,
                url: Some("https://example.com/mcp".to_string()),
                command: None,
                args: vec![],
                env: BTreeMap::new(),
                scope: McpScope::Global,
                project_id: None,
                auth_kind: McpAuthKind::None,
                secret_ref: None,
                account_id: None,
                enabled: true,
                timeout_ms: 5_000,
                max_retries: 0,
            })
            .unwrap();
        guard
            .upsert_mcp_tools(
                &server.id,
                vec![NewMcpTool {
                    name: "search".to_string(),
                    title: None,
                    description: None,
                    input_schema_json: "{}".to_string(),
                }],
            )
            .unwrap();
        (idea.id, server.id)
    }

    fn sample_result() -> McpToolResult {
        McpToolResult {
            content: json!([{"type": "text", "text": "findings"}]),
            structured: None,
            is_error: false,
            usage: None,
        }
    }

    /// A `captured` idea, no project — local to this module rather than reaching into the
    /// sibling `mod tests`' own `new_idea` (private there, not worth widening its visibility just
    /// for this one cross-module call).
    fn new_captured_idea(db: &Db) -> String {
        db.create_idea(None, "An idea", "").unwrap().id
    }

    /// A stdio-transport server with NO `mcp_tool` row registered at all — used only by
    /// `start_run`'s own entry-point test, where the spawned `run_research` task (which the test
    /// deliberately does not observe) must be guaranteed to deny at the per-tool allowlist gate
    /// (`ToolDisabled`) BEFORE `connect_fn`/the network is ever touched, no matter how the runtime
    /// schedules it — hermetic regardless of scheduling.
    fn new_stdio_server_without_tool(db: &Db) -> String {
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

    /// A push-fan-out subscriber a test can drain — mirrors `bpa_daemon_core::broadcast`'s own
    /// test shape (register a channel, assert what arrives).
    fn subscribe(broadcaster: &Broadcaster<OrchdFrame>) -> mpsc::Receiver<OrchdFrame> {
        let (tx, rx) = mpsc::channel(16);
        broadcaster.register(1, tx);
        rx
    }

    fn drain_research_runs_changed_for(
        rx: &mut mpsc::Receiver<OrchdFrame>,
        idea_id: &str,
    ) -> usize {
        let mut count = 0;
        while let Ok(frame) = rx.try_recv() {
            if let OrchdFrame::Push(OrchdPush::ResearchRunsChanged {
                idea_id: Some(pushed_idea_id),
            }) = frame
            {
                if pushed_idea_id == idea_id {
                    count += 1;
                }
            }
        }
        count
    }

    // ---- start_run entry point (spec §8: "a spawned run isn't needed for THIS assertion") — the
    // stdio server has NO `mcp_tool` row at all, so even if the spawned `run_research` task runs to
    // completion in the background, it denies at the per-tool allowlist gate (`ToolDisabled`)
    // BEFORE ever touching `connect_fn`/the network — hermetic regardless of scheduling. Now
    // `.await`s `start_run` (T4 review: it is an `async fn` acquiring the DB lock with
    // `.lock().await`, no more `try_lock`/busy-reject). ----

    #[tokio::test]
    async fn start_run_returns_pending_row_and_flips_idea_to_researching() {
        let db = new_shared_db();
        let (idea_id, server_id) = {
            let guard = db.lock().await;
            (
                new_captured_idea(&guard),
                new_stdio_server_without_tool(&guard),
            )
        };
        let broadcaster: Broadcaster<OrchdFrame> = Broadcaster::new();

        let row = start_run(
            &db,
            &broadcaster,
            NewResearchRun {
                idea_id: idea_id.clone(),
                server_id: server_id.clone(),
                tool_name: "search".to_string(),
                args_json: "{}".to_string(),
            },
        )
        .await
        .unwrap();

        assert_eq!(row.status, ResearchStatus::Pending);
        assert_eq!(row.idea_id, idea_id);
        assert_eq!(row.server_id, server_id);
        assert!(row.invocation_id.is_none());
        assert!(row.artifact_id.is_none());

        let lifecycle: String = db
            .lock()
            .await
            .conn()
            .query_row(
                "SELECT lifecycle FROM idea WHERE id = ?1",
                rusqlite::params![idea_id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(lifecycle, "researching");
    }

    // ---- run_research: fake success ----

    #[tokio::test]
    async fn run_research_success_marks_done_with_ids_writes_untrusted_artifact_and_broadcasts() {
        let db = new_shared_db();
        let (idea_id, server_id) = seed_idea_and_tool(&db).await;
        let run = {
            let guard = db.lock().await;
            guard
                .start_research_run(NewResearchRun {
                    idea_id: idea_id.clone(),
                    server_id: server_id.clone(),
                    tool_name: "search".to_string(),
                    args_json: "{}".to_string(),
                })
                .unwrap()
        };

        let broadcaster: Broadcaster<OrchdFrame> = Broadcaster::new();
        let mut rx = subscribe(&broadcaster);

        let call_count = Arc::new(AtomicUsize::new(0));
        let call_count_for_closure = call_count.clone();
        let connect_fn = move |_server: McpServerRow, _bearer: Option<String>| {
            let call_count = call_count_for_closure.clone();
            async move {
                Ok::<FakeSession, McpError>(
                    FakeSession::new(vec![], call_count)
                        .with_outcomes(vec![FakeCallOutcome::Ok(sample_result())]),
                )
            }
        };

        run_research(
            db.clone(),
            broadcaster.clone(),
            run.id.clone(),
            idea_id.clone(),
            server_id.clone(),
            "search".to_string(),
            "{}".to_string(),
            None,
            connect_fn,
        )
        .await;

        let updated = db.lock().await.get_research_run(&run.id).unwrap().unwrap();
        assert_eq!(updated.status, ResearchStatus::Done);
        assert!(updated.invocation_id.is_some());
        assert!(updated.artifact_id.is_some());
        assert!(updated.error_kind.is_none());
        assert_eq!(call_count.load(Ordering::SeqCst), 1);

        let artifacts = db
            .lock()
            .await
            .list_artifacts(None, Some(&server_id), None)
            .unwrap();
        assert_eq!(artifacts.len(), 1, "a durable mcp_artifact must exist");
        assert!(
            artifacts[0].is_untrusted,
            "spec D9: research artifacts are untrusted"
        );
        assert_eq!(artifacts[0].id, updated.artifact_id.clone().unwrap());

        assert!(
            drain_research_runs_changed_for(&mut rx, &idea_id) >= 1,
            "expected at least one ResearchRunsChanged{{idea_id}} push"
        );
    }

    // ---- run_research: policy-cap denial (task T18 seam) — failed before ever dispatching ----

    #[tokio::test]
    async fn run_research_policy_cap_exceeded_marks_failed_with_no_artifact() {
        let db = new_shared_db();
        let (idea_id, server_id) = seed_idea_and_tool(&db).await;
        let run = {
            let guard = db.lock().await;
            guard
                .upsert_policy(NewPolicy {
                    scope: bpa_orchd_proto::PolicyScope::Server,
                    ref_id: Some(server_id.clone()),
                    spend_cap_usd: None,
                    rate_per_min: Some(0),
                })
                .unwrap();
            guard
                .start_research_run(NewResearchRun {
                    idea_id: idea_id.clone(),
                    server_id: server_id.clone(),
                    tool_name: "search".to_string(),
                    args_json: "{}".to_string(),
                })
                .unwrap()
        };

        let broadcaster: Broadcaster<OrchdFrame> = Broadcaster::new();
        let mut rx = subscribe(&broadcaster);

        let call_count = Arc::new(AtomicUsize::new(0));
        let call_count_for_closure = call_count.clone();
        let connect_fn = move |_server: McpServerRow, _bearer: Option<String>| {
            let call_count = call_count_for_closure.clone();
            async move { Ok::<FakeSession, McpError>(FakeSession::new(vec![], call_count)) }
        };

        run_research(
            db.clone(),
            broadcaster.clone(),
            run.id.clone(),
            idea_id.clone(),
            server_id.clone(),
            "search".to_string(),
            "{}".to_string(),
            None,
            connect_fn,
        )
        .await;

        let updated = db.lock().await.get_research_run(&run.id).unwrap().unwrap();
        assert_eq!(updated.status, ResearchStatus::Failed);
        assert_eq!(updated.error_kind.as_deref(), Some("policy_cap_exceeded"));
        assert!(updated.artifact_id.is_none());
        assert!(updated.invocation_id.is_none());
        assert_eq!(
            call_count.load(Ordering::SeqCst),
            0,
            "a policy-cap-denied call must never dispatch to the session"
        );

        assert!(db
            .lock()
            .await
            .list_artifacts(None, Some(&server_id), None)
            .unwrap()
            .is_empty());
        assert!(drain_research_runs_changed_for(&mut rx, &idea_id) >= 1);
    }

    // ---- run_research: transport error (Mcp(_) family) — invocation_id stays NULL (§4 accepted
    // partial-provenance: the shipped call_tool error type carries no invocation id) ----

    #[tokio::test]
    async fn run_research_transport_error_marks_failed_invocation_id_null_no_artifact() {
        let db = new_shared_db();
        let (idea_id, server_id) = seed_idea_and_tool(&db).await;
        let run = {
            let guard = db.lock().await;
            guard
                .start_research_run(NewResearchRun {
                    idea_id: idea_id.clone(),
                    server_id: server_id.clone(),
                    tool_name: "search".to_string(),
                    args_json: "{}".to_string(),
                })
                .unwrap()
        };

        let broadcaster: Broadcaster<OrchdFrame> = Broadcaster::new();
        let mut rx = subscribe(&broadcaster);

        let call_count = Arc::new(AtomicUsize::new(0));
        let call_count_for_closure = call_count.clone();
        let connect_fn = move |_server: McpServerRow, _bearer: Option<String>| {
            let call_count = call_count_for_closure.clone();
            async move {
                Ok::<FakeSession, McpError>(FakeSession::new(vec![], call_count).with_outcomes(
                    vec![FakeCallOutcome::Err(McpError::Transport("boom".into()))],
                ))
            }
        };

        run_research(
            db.clone(),
            broadcaster.clone(),
            run.id.clone(),
            idea_id.clone(),
            server_id.clone(),
            "search".to_string(),
            "{}".to_string(),
            None,
            connect_fn,
        )
        .await;

        let updated = db.lock().await.get_research_run(&run.id).unwrap().unwrap();
        assert_eq!(updated.status, ResearchStatus::Failed);
        assert_eq!(updated.error_kind.as_deref(), Some("transport"));
        assert!(updated.artifact_id.is_none());
        assert!(
            updated.invocation_id.is_none(),
            "the Mcp(_) failure family carries no invocation id (spec §4 accepted partial-provenance)"
        );

        assert!(db
            .lock()
            .await
            .list_artifacts(None, Some(&server_id), None)
            .unwrap()
            .is_empty());
        // A failed `mcp_invocation` row IS still written by `call_tool` itself (spec D8) — just
        // not linked from `research_run.invocation_id` (see assertion above).
        let invocations = db
            .lock()
            .await
            .list_invocations(Some(&server_id), None, None)
            .unwrap();
        assert_eq!(invocations.len(), 1);
        assert!(!invocations[0].ok);

        assert!(drain_research_runs_changed_for(&mut rx, &idea_id) >= 1);
    }
}

/// Graph-ingest-on-accept (S-IDEA spec §6 D9, task T4): `Db::set_insight_status`'s accept path
/// seeds an `entity_ref` graph node. Lives here (not `persistence`'s own `#[cfg(test)] mod
/// tests`, nor `graph`'s) per the task-4 brief's TDD placement, even though the code under test
/// is `persistence::Db::set_insight_status` — this module is S-IDEA's home for the whole
/// research-run feature slice, and this behavior is D9's other half (D2/D9: "graph-ingest the
/// insight, not the raw artifact").
#[cfg(test)]
mod graph_ingest_tests {
    use bpa_orchd_proto::{GraphEntityType, GraphNodeKind, InsightStatus};
    use uuid::Uuid;

    use super::*;

    fn new_db() -> Db {
        Db::open_in_memory().unwrap()
    }

    /// `project_workspace.workspace_id` is UNIQUE across the whole table (S3 spec §5.2) — a fresh
    /// uuid per call so multiple tests never collide (mirrors `graph.rs`'s own `new_project` test
    /// helper).
    fn new_project(db: &Db) -> String {
        let workspace_id = Uuid::new_v4().to_string();
        db.create_project("P", "", &[workspace_id]).unwrap().id
    }

    fn entity_ref_node_count(db: &Db, insight_id: &str) -> i64 {
        db.conn()
            .query_row(
                "SELECT COUNT(*) FROM graph_node WHERE entity_type = 'insight' AND entity_id = ?1",
                rusqlite::params![insight_id],
                |r| r.get(0),
            )
            .unwrap()
    }

    #[test]
    fn accepting_a_research_insight_seeds_exactly_one_entity_ref_node() {
        let db = new_db();
        let project_id = new_project(&db);
        let insight = db
            .create_insight(Some(&project_id), "research-run:r1", "A finding", "body")
            .unwrap();
        assert_eq!(entity_ref_node_count(&db, &insight.id), 0);

        let updated = db
            .set_insight_status(&insight.id, InsightStatus::Accepted, Some("looks good"))
            .unwrap();
        assert_eq!(updated.status, InsightStatus::Accepted);
        assert_eq!(entity_ref_node_count(&db, &insight.id), 1);

        let view = db.list_project_graph(&project_id).unwrap();
        let node = view
            .nodes
            .iter()
            .find(|n| n.entity_id.as_deref() == Some(insight.id.as_str()))
            .expect("the seeded entityRef node must belong to the insight's own project");
        assert_eq!(node.kind, GraphNodeKind::EntityRef);
        assert_eq!(node.entity_type, Some(GraphEntityType::Insight));
        assert_eq!(node.label, "A finding");
    }

    #[test]
    fn re_accepting_after_archive_keeps_exactly_one_node_conflict_is_benign() {
        let db = new_db();
        let project_id = new_project(&db);
        let insight = db
            .create_insight(Some(&project_id), "research-run:r1", "A finding", "body")
            .unwrap();

        db.set_insight_status(&insight.id, InsightStatus::Accepted, None)
            .unwrap();
        assert_eq!(entity_ref_node_count(&db, &insight.id), 1);

        db.set_insight_status(&insight.id, InsightStatus::Archived, None)
            .unwrap();
        assert_eq!(
            entity_ref_node_count(&db, &insight.id),
            1,
            "archiving must not remove the node (S4 orphan-on-delete model)"
        );

        let reaccepted = db
            .set_insight_status(&insight.id, InsightStatus::Accepted, None)
            .unwrap();
        assert_eq!(reaccepted.status, InsightStatus::Accepted);
        assert_eq!(
            entity_ref_node_count(&db, &insight.id),
            1,
            "a re-accept's Conflict must be swallowed as a benign no-op, not surfaced as an error"
        );
    }

    #[test]
    fn archiving_a_new_insight_does_not_seed_a_graph_node() {
        let db = new_db();
        let project_id = new_project(&db);
        let insight = db
            .create_insight(Some(&project_id), "research-run:r1", "A finding", "body")
            .unwrap();

        db.set_insight_status(&insight.id, InsightStatus::Archived, Some("not relevant"))
            .unwrap();

        assert_eq!(entity_ref_node_count(&db, &insight.id), 0);
    }

    #[test]
    fn accepting_a_project_less_insight_is_a_no_op_ingest_not_an_error() {
        let db = new_db();
        let insight = db
            .create_insight(None, "research-run:r1", "Orphan finding", "body")
            .unwrap();

        let updated = db
            .set_insight_status(&insight.id, InsightStatus::Accepted, None)
            .unwrap();
        assert_eq!(updated.status, InsightStatus::Accepted);
        assert_eq!(entity_ref_node_count(&db, &insight.id), 0);
    }

    /// Best-effort post-commit ingest (T4 review Finding 2): the status flip is committed in its
    /// OWN transaction FIRST; graph-ingest runs afterwards in a separate transaction, best-effort.
    /// Even when that ingest does NO fresh insert — here the entityRef node was seeded out-of-band
    /// so `add_entity_ref_node` returns `Conflict`, structurally the SAME swallow branch a
    /// post-commit non-`Conflict` error takes (log-and-swallow) — the accept still returns `Ok`
    /// and the status is DURABLY committed (asserted by a fresh reload, independent of the returned
    /// value). This is the guarantee the fix adds: a failed post-commit ingest must never turn a
    /// committed accept into an `Err` reply. (A genuine non-`Conflict` ingest error is only
    /// reachable via a real archive-between-commit-and-ingest race, not deterministically
    /// injectable through the public API — every constraint violation maps to `Conflict`, and the
    /// only other errors, `NotFound`/`Invariant` from the project-active guard, can't occur when
    /// the status update on the SAME active project just committed — so this Conflict path is the
    /// deterministic proxy for the swallow branch.)
    #[test]
    fn accept_returns_ok_and_commits_status_even_when_ingest_is_a_noop() {
        let db = new_db();
        let project_id = new_project(&db);
        let insight = db
            .create_insight(Some(&project_id), "research-run:r1", "A finding", "body")
            .unwrap();
        // Seed the entityRef node out-of-band, so the accept's OWN ingest is a Conflict no-op from
        // the very first accept (not a re-accept) — isolating the "ingest did nothing, accept must
        // still succeed" contract.
        db.add_entity_ref_node(
            &project_id,
            GraphEntityType::Insight,
            &insight.id,
            "A finding",
            0.0,
            0.0,
        )
        .unwrap();
        assert_eq!(entity_ref_node_count(&db, &insight.id), 1);

        let updated = db
            .set_insight_status(&insight.id, InsightStatus::Accepted, Some("looks good"))
            .unwrap();
        assert_eq!(updated.status, InsightStatus::Accepted);

        // The status is durably committed regardless of the ingest outcome — prove it with a fresh
        // read straight from the row, not the returned value.
        let committed_status: String = db
            .conn()
            .query_row(
                "SELECT status FROM insight WHERE id = ?1",
                rusqlite::params![insight.id],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(committed_status, "accepted");
        assert_eq!(
            entity_ref_node_count(&db, &insight.id),
            1,
            "the no-op ingest must not have created a second node"
        );
    }
}
