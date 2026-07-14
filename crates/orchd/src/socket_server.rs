//! `bpa-orchd` socket server (spec §5, mirrors `bpa_sessiond::socket_server` minus PTY/attach/
//! scrollback concerns): a tokio `UnixListener` accept loop with one task per connected client,
//! the codec-agnostic preamble handshake + version negotiation gate (shared with sessiond via
//! `bpa_daemon_core::handshake`), request/response correlation, a bounded per-client outbound
//! queue (overflow ⇒ drop+disconnect), peer-cred refusal of foreign euids, and a
//! `Broadcaster<OrchdFrame>` client registry fanning out every domain-change push (spec §6).
//!
//! ## Dispatch (spec §4.2, §5, §6, §7)
//!
//! Every `OrchdRequest` verb is dispatched to its `persistence::Db` (T6-T8) or `export` (T9)
//! counterpart. `OrchdRequest::Ping` → `Pong`; `OrchdRequest::OrchdShutdown { drain }` → (if
//! `drain`: a best-effort WAL checkpoint) reply `Ack` and flip the shared shutdown watch — the
//! SAME trigger `main.rs`'s SIGTERM handler flips, so a GUI-initiated shutdown and an operator
//! signal converge on one graceful-exit path (mirrors sessiond's `Request::DaemonShutdown`
//! dispatch arm). Every domain verb replies the updated entity (`Ack` for deletes,
//! `ImportReport` for `ImportBundle`) and — ONLY on success — broadcasts the matching coarse
//! `OrchdPush` (spec §6: "Failed requests broadcast NOTHING"). `OrchdPersistError` maps to the
//! wire `OrchdResponse::Error{code, message}` per spec §6 (`Sql→Io`, `Io(String)→Io`, the rest
//! 1:1). `GetRuleSet`/`UpsertRuleSet`/`AcknowledgeRuleFile` all reply `RuleSetView` — assembled by
//! pairing the DB row with a FRESH `ruleset_files::read_state` read (spec §7: never cached).
//! `CreateProject`'s auto-created ruleset DB row (written inside `Db::create_project`'s own
//! transaction) gets its FILE written here, post-commit, by delegating to `Db::upsert_ruleset`
//! (which already does "write_atomic + rehash" — see [`write_initial_ruleset_file`]'s doc for why
//! a write failure there is logged and swallowed rather than rolling back the committed project).
//! `ExportProject`/`ExportAll`/`ImportBundle` read `bpa_daemon_core::dirs::app_support_dir()` and
//! (export only) the wall clock — this module is the ONE place in the crate allowed to call
//! `SystemTime::now()` for the `exported_at` stamp (`export.rs` itself never does — it takes
//! `exported_at` as a parameter; see [`now_ms`]). `persistence.rs` has its own, separate
//! `now_ms`/`now_secs` for row `created_at`/`updated_at` timestamps and DB-quarantine filenames —
//! this module is not the only `SystemTime::now()` caller in the crate, only the only one for
//! the export stamp.

use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use rusqlite::OptionalExtension;
use tokio::io::AsyncWriteExt;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{mpsc, watch, Mutex};

use bpa_daemon_core::singleton::check_peer_cred;
use bpa_orchd_proto::{
    encode_orchd_frame, DomainTask, Goal, Idea, Insight, OrchdErrorCode, OrchdFrame,
    OrchdFrameDecoder, OrchdPush, OrchdRequest, OrchdResponse, Project, RuleScope, RuleSet,
    RuleSetView, ORCHD_DAEMON_MAX_VERSION, ORCHD_DAEMON_MIN_VERSION,
};

use crate::export;
use crate::persistence::{Db, OrchdPersistError};
use crate::ruleset_files;

/// Per-client bounded outbound queue depth (frames). Overflow (a client that stopped reading) ⇒
/// drop + disconnect that client rather than buffer unboundedly (mirrors sessiond's
/// `CLIENT_OUTQ_CAP`).
pub const CLIENT_OUTQ_CAP: usize = 1024;

/// Bound on how long connection cleanup waits for the writer task to notice its queue is closed
/// and exit on its own, before forcibly aborting it (mirrors sessiond's `WRITER_JOIN_TIMEOUT`).
const WRITER_JOIN_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(200);

/// Registry of every connected client's outbound queue (spec §5, §6): every successful mutating
/// verb's dispatch arm fans a domain-change push out through this.
type Broadcaster = bpa_daemon_core::broadcast::Broadcaster<OrchdFrame>;

/// Shared dependency bundle handed to the server and every per-client task.
///
/// `db` is `Arc<Mutex<Db>>` (not `Arc<Db>`) because [`Db`] holds a `rusqlite::Connection` and is
/// `Send + !Sync`: the async Mutex both makes it shareable across the per-client tasks and
/// serializes access to the single connection (mirrors `bpa_sessiond::socket_server::ServerDeps`).
pub struct ServerDeps {
    pub db: Arc<Mutex<Db>>,
    /// Human-readable daemon build string echoed in the accepted preamble reply.
    pub daemon_build: String,
    /// The SAME `watch::Sender` whose receiver drives [`serve`]'s accept loop (and every
    /// connected client's dispatch loop). `OrchdRequest::OrchdShutdown` is the only dispatch arm
    /// that fires this: flipping it to `true` is exactly the SIGTERM path (`main.rs`'s signal
    /// watcher flips the same channel).
    pub shutdown_tx: watch::Sender<bool>,
}

impl ServerDeps {
    pub fn new(db: Arc<Mutex<Db>>, daemon_build: String, shutdown_tx: watch::Sender<bool>) -> Self {
        ServerDeps {
            db,
            daemon_build,
            shutdown_tx,
        }
    }
}

/// Accept loop (spec §5): peer-cred gate on accept, one task per client, handshake-gated
/// dispatch. Runs until `shutdown` flips to `true` or the `listener` errors, then returns.
pub async fn serve(
    listener: UnixListener,
    deps: Arc<ServerDeps>,
    mut shutdown: watch::Receiver<bool>,
) -> std::io::Result<()> {
    let broadcaster = Broadcaster::default();
    // Monotonic per-connection id (used only to key the broadcaster registry).
    let mut next_conn_id: u64 = 1;

    let result = loop {
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break Ok(());
                }
            }
            accepted = listener.accept() => {
                let stream = match accepted {
                    Ok((stream, _addr)) => stream,
                    Err(e) => {
                        tracing::warn!(error = %e, "accept failed");
                        continue;
                    }
                };
                {
                    use std::os::fd::AsFd;
                    if let Err(e) = check_peer_cred(stream.as_fd()) {
                        tracing::warn!(error = %e, "peer-cred rejected connection");
                        drop(stream);
                        continue;
                    }
                }
                let conn_id = next_conn_id;
                next_conn_id += 1;
                let deps = deps.clone();
                let broadcaster = broadcaster.clone();
                let client_shutdown = shutdown.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_client(conn_id, stream, deps, broadcaster, client_shutdown).await {
                        tracing::debug!(conn = conn_id, error = %e, "client task ended");
                    }
                });
            }
        }
    };

    result
}

/// Drive one connected client end to end: handshake gate → split reader/writer with a bounded
/// outbound queue → dispatch loop. Returns `Ok(())` on a clean disconnect and `Err` on a
/// framing/protocol error or outbound overflow (the caller only logs it).
async fn handle_client(
    conn_id: u64,
    mut stream: UnixStream,
    deps: Arc<ServerDeps>,
    broadcaster: Broadcaster,
    mut shutdown: watch::Receiver<bool>,
) -> std::io::Result<()> {
    // ---- Preamble handshake: a fixed, codec-independent header precedes the CBOR frame stream
    // so a version-incompatible peer can always be told so, even if it can't decode CBOR.
    match bpa_daemon_core::handshake::server_handshake(
        &mut stream,
        ORCHD_DAEMON_MIN_VERSION,
        ORCHD_DAEMON_MAX_VERSION,
        &deps.daemon_build,
    )
    .await
    {
        Ok(Some(_chosen)) => {} // Accepted; fall through into the CBOR dispatch loop below
        Ok(None) => return Ok(()), // Incompatible: reply already written, just close
        Err(_) => return Ok(()), // malformed/garbage preamble, or the read/write timed out
    }

    // ---- Split into an independent reader + writer, joined by a bounded outbound queue. ----
    let (mut rd, mut wr) = stream.into_split();
    let (out_tx, mut out_rx) = mpsc::channel::<OrchdFrame>(CLIENT_OUTQ_CAP);

    // Register this client for domain-change push fan-out (spec §6) — every successful mutating
    // verb's dispatch arm broadcasts through it.
    broadcaster.register(conn_id, out_tx.clone());

    // Writer task: drains the bounded queue and writes to the socket. Exits on EPIPE/write error
    // (⇒ the client is gone) or when the queue is closed (all senders dropped).
    let mut writer = tokio::spawn(async move {
        while let Some(frame) = out_rx.recv().await {
            let bytes = match encode_orchd_frame(&frame) {
                Ok(b) => b,
                Err(e) => {
                    tracing::error!(error = %e, "frame encode failed; dropping client");
                    break;
                }
            };
            if wr.write_all(&bytes).await.is_err() || wr.flush().await.is_err() {
                break; // EPIPE / dead client
            }
        }
    });

    // ---- Dispatch loop: correlate every Request{id} with exactly one Response{id}. ----
    let mut reader = OrchdFrameReader::new();
    let outcome: std::io::Result<()> = loop {
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break Ok(());
                }
            }
            frame = reader.next(&mut rd) => {
                match frame {
                    Ok(Some(OrchdFrame::Request { id, req })) => {
                        let res = dispatch(&deps, &broadcaster, req).await;
                        if out_tx.try_send(OrchdFrame::Response { id, res }).is_err() {
                            break Err(std::io::Error::new(
                                std::io::ErrorKind::WouldBlock,
                                "client outbound queue overflow",
                            ));
                        }
                    }
                    Ok(Some(OrchdFrame::Response { .. } | OrchdFrame::Push(_))) => {
                        tracing::warn!(conn = conn_id, "ignoring unexpected inbound Response/Push");
                    }
                    Ok(None) => break Ok(()),  // client closed cleanly
                    Err(e) => break Err(e),    // framing/protocol error ⇒ disconnect
                }
            }
        }
    };

    // ---- Cleanup: deregister from fan-out, then let the writer drain/exit (bounded, not
    // unconditional — see sessiond's identical rationale: the writer may be parked inside a
    // stalled write to a client that stopped reading). ----
    broadcaster.deregister(conn_id);
    drop(out_tx);
    if tokio::time::timeout(WRITER_JOIN_TIMEOUT, &mut writer)
        .await
        .is_err()
    {
        writer.abort();
    }
    outcome
}

/// A stateful frame reader for one connection. Owns the [`OrchdFrameDecoder`] plus a queue of
/// already-decoded-but-not-yet-returned frames, so a single socket `read()` that delivers several
/// pipelined frames is fully consumed and drained one at a time (mirrors sessiond's
/// `FrameReader`).
struct OrchdFrameReader {
    decoder: OrchdFrameDecoder,
    pending: std::collections::VecDeque<OrchdFrame>,
    buf: Box<[u8; 16 * 1024]>,
}

impl OrchdFrameReader {
    fn new() -> Self {
        OrchdFrameReader {
            decoder: OrchdFrameDecoder::new(),
            pending: std::collections::VecDeque::new(),
            buf: Box::new([0u8; 16 * 1024]),
        }
    }

    /// Return the next complete `OrchdFrame`, reading from `stream` only when nothing is
    /// buffered. `Ok(None)` on a clean EOF at a frame boundary; `InvalidData` on an oversized
    /// length prefix or a decode failure.
    async fn next<S>(&mut self, stream: &mut S) -> std::io::Result<Option<OrchdFrame>>
    where
        S: tokio::io::AsyncRead + Unpin,
    {
        use tokio::io::AsyncReadExt;
        loop {
            if let Some(f) = self.pending.pop_front() {
                return Ok(Some(f));
            }
            let frames = self.decoder.decode().map_err(to_io)?;
            if !frames.is_empty() {
                self.pending.extend(frames);
                continue;
            }
            let n = stream.read(&mut self.buf[..]).await?;
            if n == 0 {
                return Ok(None); // clean EOF; a mid-frame EOF yields None too (caller treats as close)
            }
            self.decoder.push(&self.buf[..n]);
        }
    }
}

/// Convert any `Display` error into an `InvalidData` `io::Error`.
fn to_io<E: std::fmt::Display>(e: E) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
}

/// Unix-ms wall-clock read for `ExportProject`/`ExportAll`'s caller-supplied `exported_at` stamp
/// (spec §8, task-10 brief: "the daemon must NOT call a wall clock in library code, but
/// socket_server is the daemon binary edge — you MAY read the clock here"). This is the ONE place
/// in the crate that calls `SystemTime::now()` outside a test — `export.rs` takes `exported_at`
/// as a parameter precisely so it never has to.
fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Maps a domain persistence failure to the wire `OrchdResponse::Error` shape (spec §6):
/// `NotFound→NotFound`, `Invariant→Invariant`, `Conflict→Conflict`, `Validation→Validation`,
/// `Io→Io`, and `Sql→Io` (a raw SQL error is still an I/O-class failure from the wire's point of
/// view — SQLite itself is a file on disk). The message is `OrchdPersistError`'s own `Display`
/// text in every case; no dispatch arm below constructs an error message by hand.
fn map_err(e: OrchdPersistError) -> OrchdResponse {
    let code = match &e {
        OrchdPersistError::NotFound => OrchdErrorCode::NotFound,
        OrchdPersistError::Invariant(_) => OrchdErrorCode::Invariant,
        OrchdPersistError::Conflict(_) => OrchdErrorCode::Conflict,
        OrchdPersistError::Validation(_) => OrchdErrorCode::Validation,
        OrchdPersistError::Io(_) => OrchdErrorCode::Io,
        OrchdPersistError::Sql(_) => OrchdErrorCode::Io,
    };
    OrchdResponse::Error {
        code,
        message: e.to_string(),
    }
}

/// A mutating project verb's shared reply/push shape (spec §6: "project verbs ... ⇒
/// `ProjectsChanged`"): on success, broadcast `ProjectsChanged` and reply the updated `Project`;
/// on failure, map the error and broadcast NOTHING.
fn respond_project(
    result: Result<Project, OrchdPersistError>,
    broadcaster: &Broadcaster,
) -> OrchdResponse {
    match result {
        Ok(project) => {
            broadcaster.broadcast(OrchdFrame::Push(OrchdPush::ProjectsChanged));
            OrchdResponse::Project(project)
        }
        Err(e) => map_err(e),
    }
}

/// A mutating goal verb's shared reply/push shape (spec §6: `GoalsChanged{project_id}`), routed
/// by the RETURNED goal's own `project_id` — every goal verb that reaches this helper already has
/// the updated row in hand, so no extra lookup is needed (contrast [`goal_project_id`], used only
/// by `DeleteGoal`, whose reply carries no entity to read a `project_id` off of).
fn respond_goal(
    result: Result<Goal, OrchdPersistError>,
    broadcaster: &Broadcaster,
) -> OrchdResponse {
    match result {
        Ok(goal) => {
            broadcaster.broadcast(OrchdFrame::Push(OrchdPush::GoalsChanged {
                project_id: goal.project_id.clone(),
            }));
            OrchdResponse::Goal(goal)
        }
        Err(e) => map_err(e),
    }
}

/// A mutating idea verb's shared reply/push shape (spec §6: coarse `IdeasChanged`, no id).
fn respond_idea(
    result: Result<Idea, OrchdPersistError>,
    broadcaster: &Broadcaster,
) -> OrchdResponse {
    match result {
        Ok(idea) => {
            broadcaster.broadcast(OrchdFrame::Push(OrchdPush::IdeasChanged));
            OrchdResponse::Idea(idea)
        }
        Err(e) => map_err(e),
    }
}

/// A mutating insight verb's shared reply/push shape (spec §6: coarse `InsightsChanged`, no id).
fn respond_insight(
    result: Result<Insight, OrchdPersistError>,
    broadcaster: &Broadcaster,
) -> OrchdResponse {
    match result {
        Ok(insight) => {
            broadcaster.broadcast(OrchdFrame::Push(OrchdPush::InsightsChanged));
            OrchdResponse::Insight(insight)
        }
        Err(e) => map_err(e),
    }
}

/// A mutating task verb's shared reply/push shape (spec §6: `TasksChanged{project_id}`), routed
/// by the RETURNED task's own `project_id` (mirrors [`respond_goal`]; contrast
/// [`task_project_id`], used only by `DeleteTask`).
fn respond_task(
    result: Result<DomainTask, OrchdPersistError>,
    broadcaster: &Broadcaster,
) -> OrchdResponse {
    match result {
        Ok(task) => {
            broadcaster.broadcast(OrchdFrame::Push(OrchdPush::TasksChanged {
                project_id: task.project_id.clone(),
            }));
            OrchdResponse::Task(task)
        }
        Err(e) => map_err(e),
    }
}

/// Assembles the wire `RuleSetView` (spec §4.2/§7) by pairing a DB-row `RuleSet` (from
/// `Db::get_ruleset`/`upsert_ruleset`/`acknowledge_rule_file` — every ruleset verb's persistence
/// call returns this same row shape) with a FRESH `ruleset_files::read_state` read of the file at
/// `rule.md_path` against `rule.md_hash` — spec §7: "`GetRuleSet`: read file fresh each time",
/// applied uniformly here to every ruleset-returning response, not just `GetRuleSet` itself (a
/// just-written file could in principle be edited externally in the instant between the DB write
/// and this read; reading fresh costs one `read_to_string` and is never wrong).
fn build_ruleset_view(rule: RuleSet) -> RuleSetView {
    let (md_content, file_state) =
        ruleset_files::read_state(Path::new(&rule.md_path), &rule.md_hash);
    RuleSetView {
        rule,
        md_content,
        file_state,
    }
}

/// A mutating ruleset verb's shared reply/push shape (spec §6: `RuleSetChanged{scope,
/// project_id}`) — shared by `UpsertRuleSet` and `AcknowledgeRuleFile` (both return a bare
/// `RuleSet` row from `persistence.rs`). `GetRuleSet` is a READ and does NOT use this helper — no
/// push on a read, per spec §6's "mutating request" scoping.
fn respond_ruleset(
    result: Result<RuleSet, OrchdPersistError>,
    broadcaster: &Broadcaster,
) -> OrchdResponse {
    match result {
        Ok(rule) => {
            broadcaster.broadcast(OrchdFrame::Push(OrchdPush::RuleSetChanged {
                scope: rule.scope.clone(),
                project_id: rule.project_id.clone(),
            }));
            OrchdResponse::RuleSetView(build_ruleset_view(rule))
        }
        Err(e) => map_err(e),
    }
}

/// Post-commit ruleset FILE write for a freshly created project (spec §7/§10, task-10 brief):
/// `Db::create_project` already committed the `ruleset` DB ROW (default `md_path`, `md_hash: ""`)
/// inside its OWN transaction before this is ever called. This writes the FILE at that path with
/// the locked template (`"# Правила проекта <name>\n"`) and stores its hash by delegating to
/// `Db::upsert_ruleset` — which, given `md_content: Some(_)`, already does exactly "atomic write +
/// rehash the row" (T8); calling it here is not a second hashing/writing implementation, just
/// this task's post-commit trigger for the one T8 already built.
///
/// A write failure (permission denied, disk full, …) is LOGGED and otherwise swallowed — it must
/// never roll back the already-committed project (spec: never rolls back the committed project;
/// honest, documented). The row's `md_hash` simply stays `""` in that case, so the very next
/// `GetRuleSet` (via [`build_ruleset_view`]) honestly reports `RuleFileState::Missing` until the
/// owner retries through `UpsertRuleSet`/`AcknowledgeRuleFile`.
async fn write_initial_ruleset_file(deps: &Arc<ServerDeps>, project: &Project) {
    let template = format!("# Правила проекта {}\n", project.name);
    let db = deps.db.lock().await;
    if let Err(e) = db.upsert_ruleset(
        RuleScope::Project,
        Some(&project.id),
        Some(&template),
        None,
        None,
    ) {
        tracing::error!(
            project_id = %project.id,
            error = %e,
            "failed to write the new project's initial ruleset file; GetRuleSet will report \
             RuleFileState::Missing until this is retried"
        );
    }
}

/// Looks up a goal's `project_id` directly via the doc-hidden `Db::conn()` raw-query seam (its own
/// doc: "the seam T10's domain CRUD methods will be built directly on top of"). `DeleteGoal`
/// replies a bare `Ack` (no entity to read a `project_id` off of, unlike every other goal verb —
/// see [`respond_goal`]), so the id its `GoalsChanged{project_id}` push needs to carry must be
/// captured BEFORE the row is gone. Unknown `id` ⇒ `NotFound` (mirrors `Db::delete_goal`'s own
/// unknown-id handling — this just surfaces it one step earlier so the caller never attempts the
/// delete at all).
fn goal_project_id(db: &Db, id: &str) -> Result<String, OrchdPersistError> {
    db.conn()
        .query_row(
            "SELECT project_id FROM goal WHERE id = ?1",
            rusqlite::params![id],
            |r| r.get(0),
        )
        .optional()
        .map_err(OrchdPersistError::from)?
        .ok_or(OrchdPersistError::NotFound)
}

/// Task analogue of [`goal_project_id`] — `DeleteTask` has the identical "`Ack`-only reply, need
/// the id captured before the delete" shape.
fn task_project_id(db: &Db, id: &str) -> Result<String, OrchdPersistError> {
    db.conn()
        .query_row(
            "SELECT project_id FROM task WHERE id = ?1",
            rusqlite::params![id],
            |r| r.get(0),
        )
        .optional()
        .map_err(OrchdPersistError::from)?
        .ok_or(OrchdPersistError::NotFound)
}

/// True if `v` is present and is a non-empty JSON array — the shared "did this bundle field
/// actually carry any rows" test [`import_touched_pushes`] uses for every family it inspects.
fn non_empty_json_array(v: Option<&serde_json::Value>) -> bool {
    v.and_then(serde_json::Value::as_array)
        .is_some_and(|a| !a.is_empty())
}

/// Extracts the exact `OrchdPush` set an `ImportBundle`'s (already validated, already committed)
/// `json` touched (spec §6: "`ImportBundle` ⇒ every push whose family the bundle touched"). Walks
/// the SAME two locked bundle shapes `export::import_bundle` discriminates on (a top-level
/// `project` vs `projects` key) as a raw `serde_json::Value`, rather than through `export.rs`'s
/// typed structs: this only needs to know WHICH project ids/scopes were present, not which
/// specific rows landed, and `export.rs` exposes no public API returning that (its `ImportCounts`
/// reply is aggregate-only) — re-parsing the same string the dispatch arm already has in hand is
/// simpler than widening that contract. A parse failure here is unreachable in practice
/// (`import_bundle` already parsed this exact `json` successfully before this is ever called) and
/// degrades to "no pushes" rather than panicking.
fn import_touched_pushes(json: &str) -> Vec<OrchdPush> {
    let Ok(value) = serde_json::from_str::<serde_json::Value>(json) else {
        return Vec::new();
    };

    let mut pushes = Vec::new();
    let mut any_project = false;
    let mut any_idea = false;
    let mut any_insight = false;

    {
        let mut visit_project_bundle = |bundle: &serde_json::Value| {
            let Some(project_id) = bundle
                .get("project")
                .and_then(|p| p.get("id"))
                .and_then(serde_json::Value::as_str)
            else {
                return;
            };
            any_project = true;
            if non_empty_json_array(bundle.get("goals")) {
                pushes.push(OrchdPush::GoalsChanged {
                    project_id: project_id.to_string(),
                });
            }
            if non_empty_json_array(bundle.get("tasks")) {
                pushes.push(OrchdPush::TasksChanged {
                    project_id: project_id.to_string(),
                });
            }
            if bundle.get("ruleset").is_some_and(|r| !r.is_null()) {
                pushes.push(OrchdPush::RuleSetChanged {
                    scope: RuleScope::Project,
                    project_id: Some(project_id.to_string()),
                });
            }
            if non_empty_json_array(bundle.get("ideas")) {
                any_idea = true;
            }
            if non_empty_json_array(bundle.get("insights")) {
                any_insight = true;
            }
        };

        if value.get("project").is_some() {
            visit_project_bundle(&value);
        } else if let Some(projects) = value.get("projects").and_then(|v| v.as_array()) {
            for bundle in projects {
                visit_project_bundle(bundle);
            }
        }
    }

    if value.get("globalRuleset").is_some_and(|r| !r.is_null()) {
        pushes.push(OrchdPush::RuleSetChanged {
            scope: RuleScope::Global,
            project_id: None,
        });
    }
    if non_empty_json_array(value.get("orphanIdeas")) {
        any_idea = true;
    }
    if non_empty_json_array(value.get("orphanInsights")) {
        any_insight = true;
    }

    if any_project {
        pushes.push(OrchdPush::ProjectsChanged);
    }
    if any_idea {
        pushes.push(OrchdPush::IdeasChanged);
    }
    if any_insight {
        pushes.push(OrchdPush::InsightsChanged);
    }
    pushes
}

/// Dispatch one `OrchdRequest` to the right subsystem and produce the correlated `OrchdResponse`
/// (spec §4.2, §5, §6, §7): every domain verb below is a thin translation between the wire
/// request and a `persistence::Db` (T6-T8) / `export` (T9) call, plus — on success only — the
/// matching coarse push via `broadcaster` (spec §6: "Failed requests broadcast NOTHING").
async fn dispatch(
    deps: &Arc<ServerDeps>,
    broadcaster: &Broadcaster,
    req: OrchdRequest,
) -> OrchdResponse {
    match req {
        OrchdRequest::Ping => OrchdResponse::Pong,

        // Real OrchdShutdown semantics (spec §5, §6, mirrors sessiond's `DaemonShutdown`
        // dispatch arm): `drain:true` flushes (WAL checkpoint) BEFORE Acking; either way we then
        // flip the shared shutdown watch. Ordering is deliberate — both happen here, before this
        // function returns `Ack`, so flipping the watch cannot race the client out of receiving
        // its own reply (the caller only enqueues the reply into this connection's bounded
        // outbound queue AFTER `dispatch` returns).
        OrchdRequest::OrchdShutdown { drain } => {
            if drain {
                let db = deps.db.lock().await;
                if let Err(e) = db.checkpoint() {
                    tracing::warn!(error = %e, "drain checkpoint failed");
                }
            }
            let _ = deps.shutdown_tx.send(true);
            OrchdResponse::Ack
        }

        // ---- Project (spec §4.2/§5.2/§6: every verb here ⇒ `ProjectsChanged` on success) ----
        OrchdRequest::CreateProject {
            name,
            description,
            workspace_ids,
        } => {
            let created = {
                let db = deps.db.lock().await;
                db.create_project(&name, &description, &workspace_ids)
            };
            match created {
                Ok(project) => {
                    write_initial_ruleset_file(deps, &project).await;
                    broadcaster.broadcast(OrchdFrame::Push(OrchdPush::ProjectsChanged));
                    OrchdResponse::Project(project)
                }
                Err(e) => map_err(e),
            }
        }
        OrchdRequest::UpdateProject {
            id,
            name,
            description,
        } => {
            let result = {
                let db = deps.db.lock().await;
                db.update_project(&id, name.as_deref(), description.as_deref())
            };
            respond_project(result, broadcaster)
        }
        OrchdRequest::ArchiveProject { id } => {
            let result = {
                let db = deps.db.lock().await;
                db.archive_project(&id)
            };
            respond_project(result, broadcaster)
        }
        OrchdRequest::ListProjects => {
            let db = deps.db.lock().await;
            match db.list_projects() {
                Ok(v) => OrchdResponse::Projects(v),
                Err(e) => map_err(e),
            }
        }
        OrchdRequest::AddProjectWorkspace {
            project_id,
            workspace_id,
        } => {
            let result = {
                let db = deps.db.lock().await;
                db.add_project_workspace(&project_id, &workspace_id)
            };
            respond_project(result, broadcaster)
        }
        OrchdRequest::RemoveProjectWorkspace {
            project_id,
            workspace_id,
        } => {
            let result = {
                let db = deps.db.lock().await;
                db.remove_project_workspace(&project_id, &workspace_id)
            };
            respond_project(result, broadcaster)
        }

        // ---- Goal (spec §4.2/§5.2/§6: every verb here ⇒ `GoalsChanged{project_id}`) ----
        OrchdRequest::CreateGoal {
            project_id,
            parent_id,
            kind,
            title,
            body,
        } => {
            let result = {
                let db = deps.db.lock().await;
                db.create_goal(&project_id, parent_id.as_deref(), kind, &title, &body)
            };
            respond_goal(result, broadcaster)
        }
        OrchdRequest::UpdateGoal {
            id,
            title,
            body,
            status,
            metric_refs,
        } => {
            let result = {
                let db = deps.db.lock().await;
                db.update_goal(
                    &id,
                    title.as_deref(),
                    body.as_deref(),
                    status,
                    metric_refs.as_deref(),
                )
            };
            respond_goal(result, broadcaster)
        }
        OrchdRequest::MoveGoal {
            id,
            new_parent_id,
            new_ord,
        } => {
            let result = {
                let db = deps.db.lock().await;
                db.move_goal(&id, new_parent_id.as_deref(), new_ord)
            };
            respond_goal(result, broadcaster)
        }
        OrchdRequest::DeleteGoal { id } => {
            let db = deps.db.lock().await;
            match goal_project_id(&db, &id) {
                Ok(project_id) => match db.delete_goal(&id) {
                    Ok(()) => {
                        broadcaster
                            .broadcast(OrchdFrame::Push(OrchdPush::GoalsChanged { project_id }));
                        OrchdResponse::Ack
                    }
                    Err(e) => map_err(e),
                },
                Err(e) => map_err(e),
            }
        }
        OrchdRequest::ListGoals { project_id } => {
            let db = deps.db.lock().await;
            match db.list_goals(&project_id) {
                Ok(v) => OrchdResponse::Goals(v),
                Err(e) => map_err(e),
            }
        }

        // ---- Idea (spec §4.2/§5.2/§6: every verb here ⇒ coarse `IdeasChanged`) ----
        OrchdRequest::CreateIdea {
            project_id,
            title,
            body,
        } => {
            let result = {
                let db = deps.db.lock().await;
                db.create_idea(project_id.as_deref(), &title, &body)
            };
            respond_idea(result, broadcaster)
        }
        OrchdRequest::UpdateIdea { id, title, body } => {
            let result = {
                let db = deps.db.lock().await;
                db.update_idea(&id, title.as_deref(), body.as_deref())
            };
            respond_idea(result, broadcaster)
        }
        OrchdRequest::SetIdeaProject { id, project_id } => {
            let result = {
                let db = deps.db.lock().await;
                db.set_idea_project(&id, project_id.as_deref())
            };
            respond_idea(result, broadcaster)
        }
        OrchdRequest::SetIdeaLifecycle { id, lifecycle } => {
            let result = {
                let db = deps.db.lock().await;
                db.set_idea_lifecycle(&id, lifecycle)
            };
            respond_idea(result, broadcaster)
        }
        OrchdRequest::DeleteIdea { id } => {
            let result = {
                let db = deps.db.lock().await;
                db.delete_idea(&id)
            };
            match result {
                Ok(()) => {
                    broadcaster.broadcast(OrchdFrame::Push(OrchdPush::IdeasChanged));
                    OrchdResponse::Ack
                }
                Err(e) => map_err(e),
            }
        }
        OrchdRequest::ListIdeas { project_id } => {
            let db = deps.db.lock().await;
            match db.list_ideas(project_id.as_deref()) {
                Ok(v) => OrchdResponse::Ideas(v),
                Err(e) => map_err(e),
            }
        }

        // ---- Insight (spec §4.2/§5.2/§6: every verb here ⇒ coarse `InsightsChanged`) ----
        OrchdRequest::CreateInsight {
            project_id,
            source,
            title,
            body,
        } => {
            let result = {
                let db = deps.db.lock().await;
                db.create_insight(project_id.as_deref(), &source, &title, &body)
            };
            respond_insight(result, broadcaster)
        }
        OrchdRequest::UpdateInsight { id, title, body } => {
            let result = {
                let db = deps.db.lock().await;
                db.update_insight(&id, title.as_deref(), body.as_deref())
            };
            respond_insight(result, broadcaster)
        }
        OrchdRequest::SetInsightFitVerdict {
            id,
            fit_verdict,
            fit_reasoning,
        } => {
            let result = {
                let db = deps.db.lock().await;
                db.set_insight_fit_verdict(&id, fit_verdict, &fit_reasoning)
            };
            respond_insight(result, broadcaster)
        }
        OrchdRequest::SetInsightStatus {
            id,
            status,
            resolution_reasoning,
        } => {
            let result = {
                let db = deps.db.lock().await;
                db.set_insight_status(&id, status, resolution_reasoning.as_deref())
            };
            respond_insight(result, broadcaster)
        }
        OrchdRequest::DeleteInsight { id } => {
            let result = {
                let db = deps.db.lock().await;
                db.delete_insight(&id)
            };
            match result {
                Ok(()) => {
                    broadcaster.broadcast(OrchdFrame::Push(OrchdPush::InsightsChanged));
                    OrchdResponse::Ack
                }
                Err(e) => map_err(e),
            }
        }
        OrchdRequest::ListInsights { project_id } => {
            let db = deps.db.lock().await;
            match db.list_insights(project_id.as_deref()) {
                Ok(v) => OrchdResponse::Insights(v),
                Err(e) => map_err(e),
            }
        }

        // ---- Task (spec §4.2/§5.2/§6: every verb here ⇒ `TasksChanged{project_id}`) ----
        OrchdRequest::CreateTask {
            project_id,
            parent_id,
            title,
            body,
            status,
            source,
            source_id,
            tags,
        } => {
            let result = {
                let db = deps.db.lock().await;
                db.create_task(
                    &project_id,
                    parent_id.as_deref(),
                    &title,
                    &body,
                    status,
                    source,
                    source_id.as_deref(),
                    &tags,
                )
            };
            respond_task(result, broadcaster)
        }
        OrchdRequest::UpdateTask {
            id,
            title,
            body,
            tags,
        } => {
            let result = {
                let db = deps.db.lock().await;
                db.update_task(&id, title.as_deref(), body.as_deref(), tags.as_deref())
            };
            respond_task(result, broadcaster)
        }
        OrchdRequest::SetTaskStatus { id, status } => {
            let result = {
                let db = deps.db.lock().await;
                db.set_task_status(&id, status)
            };
            respond_task(result, broadcaster)
        }
        OrchdRequest::SetTaskRank { id, rank } => {
            let result = {
                let db = deps.db.lock().await;
                db.set_task_rank(&id, rank)
            };
            respond_task(result, broadcaster)
        }
        OrchdRequest::DeleteTask { id } => {
            let db = deps.db.lock().await;
            match task_project_id(&db, &id) {
                Ok(project_id) => match db.delete_task(&id) {
                    Ok(()) => {
                        broadcaster
                            .broadcast(OrchdFrame::Push(OrchdPush::TasksChanged { project_id }));
                        OrchdResponse::Ack
                    }
                    Err(e) => map_err(e),
                },
                Err(e) => map_err(e),
            }
        }
        OrchdRequest::ListTasks { project_id } => {
            let db = deps.db.lock().await;
            match db.list_tasks(project_id.as_deref()) {
                Ok(v) => OrchdResponse::Tasks(v),
                Err(e) => map_err(e),
            }
        }

        // ---- RuleSet (spec §4.2/§7/§6) ----
        OrchdRequest::GetRuleSet { scope, project_id } => {
            let db = deps.db.lock().await;
            match db.get_ruleset(scope, project_id.as_deref()) {
                Ok(rule) => OrchdResponse::RuleSetView(build_ruleset_view(rule)),
                Err(e) => map_err(e),
            }
        }
        OrchdRequest::UpsertRuleSet {
            scope,
            project_id,
            md_content,
            md_path,
            policy,
        } => {
            let result = {
                let db = deps.db.lock().await;
                db.upsert_ruleset(
                    scope,
                    project_id.as_deref(),
                    md_content.as_deref(),
                    md_path.as_deref(),
                    policy.as_ref(),
                )
            };
            respond_ruleset(result, broadcaster)
        }
        OrchdRequest::AcknowledgeRuleFile { id } => {
            let result = {
                let db = deps.db.lock().await;
                db.acknowledge_rule_file(&id)
            };
            respond_ruleset(result, broadcaster)
        }

        // ---- Export / import (spec §8; `now_ms` is the ONE handler-level clock read) ----
        OrchdRequest::ExportProject { project_id } => {
            let exported_at = now_ms();
            let db = deps.db.lock().await;
            match export::export_project(&db, &project_id, exported_at) {
                Ok(json) => OrchdResponse::ExportJson(json),
                Err(e) => map_err(e),
            }
        }
        OrchdRequest::ExportAll => {
            let exported_at = now_ms();
            let db = deps.db.lock().await;
            match export::export_all(&db, exported_at) {
                Ok(json) => OrchdResponse::ExportJson(json),
                Err(e) => map_err(e),
            }
        }
        OrchdRequest::ImportBundle { json } => {
            let app_support = bpa_daemon_core::dirs::app_support_dir();
            let result = {
                let db = deps.db.lock().await;
                export::import_bundle(&db, &app_support, &json)
            };
            match result {
                Ok(counts) => {
                    for push in import_touched_pushes(&json) {
                        broadcaster.broadcast(OrchdFrame::Push(push));
                    }
                    OrchdResponse::ImportReport {
                        projects: counts.projects,
                        goals: counts.goals,
                        ideas: counts.ideas,
                        insights: counts.insights,
                        tasks: counts.tasks,
                        rulesets: counts.rulesets,
                    }
                }
                Err(e) => map_err(e),
            }
        }
    }
}
