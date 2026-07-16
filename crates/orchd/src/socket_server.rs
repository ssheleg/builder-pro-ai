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
    encode_orchd_frame, DomainTask, Goal, GraphNode, Idea, Insight, OrchdErrorCode, OrchdFrame,
    OrchdFrameDecoder, OrchdPush, OrchdRequest, OrchdResponse, Project, RuleScope, RuleSet,
    RuleSetView, ORCHD_DAEMON_MAX_VERSION, ORCHD_DAEMON_MIN_VERSION,
};

use crate::connectors::{self, accounts::ConnectorsState};
use crate::export;
use crate::mcp::{self, OrchdMcpError};
use crate::persistence::{Db, NewPolicy, OrchdPersistError};
use crate::research;
use crate::ruleset_files;
use crate::skills::{self, NewSkill};

/// Per-client bounded outbound queue depth (frames). Overflow (a client that stopped reading) ⇒
/// drop + disconnect that client rather than buffer unboundedly (mirrors sessiond's
/// `CLIENT_OUTQ_CAP`).
pub const CLIENT_OUTQ_CAP: usize = 1024;

/// Bound on how long connection cleanup waits for the writer task to notice its queue is closed
/// and exit on its own, before forcibly aborting it (mirrors sessiond's `WRITER_JOIN_TIMEOUT`).
const WRITER_JOIN_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(200);

/// After the shutdown watch flips, [`serve`] waits up to this long for the in-flight per-connection
/// tasks to finish flushing their queued frames (chiefly the `OrchdShutdown` → `Ack` that the
/// requesting client is blocked on) before returning — after which `run()` unlinks the socket and
/// the PROCESS exits, which would otherwise kill those detached writers mid-flush. Without this
/// drain the shutdown ack races process teardown and is lost on a slow/loaded runner (the CI
/// `phase2 OrchdShutdown timed out` hang). Generously larger than `WRITER_JOIN_TIMEOUT` so every
/// connection's own bounded writer-join can complete first.
const SHUTDOWN_DRAIN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(3);

/// `ConnectorBeginOAuth`'s redirect URI (spec §5/§10, task T13a). PKCE requires a fixed
/// `redirect_uri` to be echoed identically on both the `/authorize` request and the token
/// exchange (RFC 6749 §4.1.3) — `accounts::ConnectorsState::begin_oauth`/`complete_oauth` need
/// SOME value here to construct a well-formed `authorize_url` and round-trip the exchange, but
/// actually CAPTURING a browser redirect at this address (a loopback HTTP listener bound at boot,
/// or a registered `ai.builderpro.desktop://` custom URL scheme) is a frontend/owner concern this
/// slice does not build — nothing in this codebase listens on this address today. A fixed
/// loopback placeholder is the honest v1 shape (spec: "v1 can use a localhost loopback
/// placeholder since the actual browser-redirect capture is a frontend/owner concern"): it makes
/// the PKCE flow itself correct and testable (state/verifier round-trip against a real IdP-shaped
/// token endpoint, see `connectors::accounts`'s own tests) without pretending the redirect capture
/// exists yet. Wiring a real capture mechanism is tracked as a residual Human/owner step (spec
/// §10) — whoever builds it must update this constant to match.
const CONNECTOR_OAUTH_REDIRECT: &str = "http://127.0.0.1:0/callback";

/// Registry of every connected client's outbound queue (spec §5, §6): every successful mutating
/// verb's dispatch arm fans a domain-change push out through this.
type Broadcaster = bpa_daemon_core::broadcast::Broadcaster<OrchdFrame>;

/// Shared dependency bundle handed to the server and every per-client task.
///
/// `db` is `Arc<Mutex<Db>>` (not `Arc<Db>`) because [`Db`] holds a `rusqlite::Connection` and is
/// `Send + !Sync`: the async Mutex both makes it shareable across the per-client tasks and
/// serializes access to the single connection (mirrors `bpa_sessiond::socket_server::ServerDeps`).
/// Every dispatch arm below locks it for the duration of its DB work and drops the guard before
/// returning; the MCP arms (`McpConnect`/`McpCallTool`) additionally pass this SHARED handle down
/// to `mcp::lifecycle::connect`/`mcp::invoke::call_tool`, which lock it themselves in short
/// phases AROUND their network round-trip so no `Db` guard is ever held across an `.await` (T6
/// review fix — a held guard there would stall every other orchd connection for the whole MCP
/// round-trip, and would force a captured `&Db` across a suspension point, making the spawned
/// per-connection task `!Send`).
pub struct ServerDeps {
    pub db: Arc<Mutex<Db>>,
    /// Connector OAuth-account layer (S-EXT spec §5/§7, task T13a): the OAuth provider registry
    /// PLUS the in-flight `begin_oauth` pending-PKCE map (`connectors::accounts` module doc
    /// comment). Both maps are process-local, `std::sync::Mutex`-guarded, and already safely
    /// shared internally (see [`ConnectorsState`]'s own doc comment) — a plain `Arc` (no extra
    /// `tokio::sync::Mutex` wrapper) is enough here, mirroring how `db` needs its OWN async
    /// `Mutex` (a `rusqlite::Connection` is `!Sync`) while `connectors` does not. Lives for the
    /// daemon's whole lifetime, constructed once at boot alongside `db` (`boot::run`) — v1 boots
    /// with an EMPTY provider registry (no real IdP credentials ship with the app); an
    /// unregistered provider's `ConnectorBeginOAuth` fails with a typed error (honest v1
    /// behavior, spec §10: wiring a real provider is an owner/config step, never fabricated here).
    pub connectors: Arc<ConnectorsState>,
    /// Human-readable daemon build string echoed in the accepted preamble reply.
    pub daemon_build: String,
    /// The SAME `watch::Sender` whose receiver drives [`serve`]'s accept loop (and every
    /// connected client's dispatch loop). `OrchdRequest::OrchdShutdown` is the only dispatch arm
    /// that fires this: flipping it to `true` is exactly the SIGTERM path (`main.rs`'s signal
    /// watcher flips the same channel).
    pub shutdown_tx: watch::Sender<bool>,
    /// The storage-degradation mode this daemon booted into (spec D3, BL-94), fixed at boot by
    /// `boot::open_db_degrading` and returned verbatim by the `GetStorageStatus` dispatch arm so
    /// the frontend can surface an honest "running in memory / recovered from corruption" banner.
    pub storage_status: bpa_orchd_proto::StorageStatus,
}

impl ServerDeps {
    pub fn new(
        db: Arc<Mutex<Db>>,
        connectors: Arc<ConnectorsState>,
        daemon_build: String,
        shutdown_tx: watch::Sender<bool>,
        storage_status: bpa_orchd_proto::StorageStatus,
    ) -> Self {
        ServerDeps {
            db,
            connectors,
            daemon_build,
            shutdown_tx,
            storage_status,
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
    // Track the spawned per-connection tasks so shutdown can wait for their outbound queues to
    // flush before the process exits (see `SHUTDOWN_DRAIN_TIMEOUT`). The `join_next` arm below
    // reaps finished connections during normal operation so this stays bounded to live clients.
    let mut conns: tokio::task::JoinSet<()> = tokio::task::JoinSet::new();

    let result = loop {
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break Ok(());
                }
            }
            Some(_joined) = conns.join_next(), if !conns.is_empty() => {
                // A connection task finished; reaped so `conns` doesn't accumulate handles.
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
                conns.spawn(async move {
                    if let Err(e) = handle_client(conn_id, stream, deps, broadcaster, client_shutdown).await {
                        tracing::debug!(conn = conn_id, error = %e, "client task ended");
                    }
                });
            }
        }
    };

    // Shutdown drain: every live connection task shares the same `shutdown` watch that just
    // flipped, so each is winding down (its dispatch loop breaks, then it bounded-joins its own
    // writer). Wait for them here — bounded — so the `OrchdShutdown` ack (and any other queued
    // frame) is flushed to the socket BEFORE `run()` returns and the process exits. Without this,
    // process teardown races the detached writers and the ack is lost on a slow runner.
    let _ = tokio::time::timeout(SHUTDOWN_DRAIN_TIMEOUT, async {
        while conns.join_next().await.is_some() {}
    })
    .await;

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

/// Maps an `OrchdMcpError` (S-EXT §6, T5's `mcp::lifecycle::connect`/`mcp::invoke::call_tool`
/// error type — NOT a wire type itself, see that type's own doc comment) onto the wire
/// `OrchdResponse::Error` shape (spec §5/§6, task T6). `ConsentRequired` is the trust
/// choke-point's Connect-side denial (spec D10: no valid `consent_grant` for the server's CURRENT
/// url) — surfaces as `Error{Consent}` (a dedicated `OrchdErrorCode` this task adds) so a client
/// can show the consent dialog specifically, rather than a generic failure. `ToolDisabled` is the
/// ToolCall-side allowlist denial (a disabled/unrecognized tool, spec §6) and
/// `PolicyCapExceeded` (task T18) is a spend/rate cap breach — BOTH surface as `Error{Policy}`
/// (also new this task); only the message text tells them apart. Every other MCP-level failure
/// (`Mcp`/`Secret`) has no richer wire code yet — mirrors [`map_err`]'s own `Sql -> Io` precedent.
/// `Persist` delegates straight to [`map_err`] since it already wraps an `OrchdPersistError` —
/// reusing its NotFound/Invariant/Validation/Conflict mapping rather than flattening every
/// persistence failure down to `Io`.
fn map_mcp_err(e: OrchdMcpError) -> OrchdResponse {
    let message = e.to_string();
    match e {
        OrchdMcpError::ConsentRequired => OrchdResponse::Error {
            code: OrchdErrorCode::Consent,
            message,
        },
        // `PolicyCapExceeded` (task T18, spec §6/BL-22 — a spend/rate cap breach) maps to the
        // SAME `Error{Policy}` wire code as `ToolDisabled` (the per-tool allowlist denial); only
        // the message differs (see `OrchdMcpError::PolicyCapExceeded`'s own doc comment).
        OrchdMcpError::ToolDisabled | OrchdMcpError::PolicyCapExceeded(_) => OrchdResponse::Error {
            code: OrchdErrorCode::Policy,
            message,
        },
        OrchdMcpError::Persist(inner) => map_err(inner),
        OrchdMcpError::Mcp(_) | OrchdMcpError::Secret(_) => OrchdResponse::Error {
            code: OrchdErrorCode::Io,
            message,
        },
    }
}

/// Maps a `bpa_secrets::SecretError` (Keychain failure, spec D4) onto the wire
/// `OrchdResponse::Error` shape (task T6, `McpSetServerBearer`'s Keychain write). No dedicated
/// wire code exists for a Keychain failure — mirrors [`map_err`]'s own `Sql -> Io` precedent. The
/// message is `SecretError`'s own `Display` text, which structurally never carries the secret
/// bytes (see `bpa_secrets::SecretError`'s own doc comment: neither variant holds a `Vec<u8>`).
fn map_secret_err(e: bpa_secrets::SecretError) -> OrchdResponse {
    OrchdResponse::Error {
        code: OrchdErrorCode::Io,
        message: e.to_string(),
    }
}

/// Maps a `connectors::accounts::ConnectorError` (S-EXT spec §5/§7, task T13a — `ConnectorBeginOAuth`/
/// `ConnectorCompleteOAuth`/`ConnectorAddApiKey`/`ConnectorListOps`'s failure modes: unknown
/// provider/state, invalid provider config, token exchange, Keychain) onto the wire
/// `OrchdResponse::Error` shape. Mirrors [`map_secret_err`]'s own "no dedicated wire code yet,
/// `Sql -> Io` precedent" — none of these failure modes has a richer code today. `Persist`
/// delegates to [`map_err`] (reuses its NotFound/Invariant/Validation/Conflict mapping rather than
/// flattening every persistence failure down to `Io`, same rationale as [`map_mcp_err`]'s own
/// `Persist` arm).
fn map_connector_err(e: connectors::accounts::ConnectorError) -> OrchdResponse {
    use connectors::accounts::ConnectorError;
    match e {
        ConnectorError::Persist(inner) => map_err(inner),
        other => OrchdResponse::Error {
            code: OrchdErrorCode::Io,
            message: other.to_string(),
        },
    }
}

/// Maps a `connectors::adapter::ConnectorInvokeError` (S-EXT spec §5/§6, task T13a) onto the wire
/// `OrchdResponse::Error` shape — the `ConnectorInvoke` analogue of [`map_mcp_err`].
/// `Denied` is the trust choke-point's policy-side denial (spec §6: "connector_invoke passes
/// through `trust::authorize` IDENTICALLY to `McpCallTool`") — surfaces as `Error{Policy}`, same
/// wire code `McpCallTool`'s own `ToolDisabled` denial uses, so a client already handling one
/// handles the other. `Persist` delegates to [`map_err`]. `Adapter` (the adapter itself failed —
/// unknown provider/op, bad args, transport/timeout/HTTP-status) has no richer code yet, same
/// `Sql -> Io` precedent as every other unmapped failure family in this file.
fn map_connector_invoke_err(e: connectors::adapter::ConnectorInvokeError) -> OrchdResponse {
    use connectors::adapter::ConnectorInvokeError;
    match e {
        ConnectorInvokeError::Denied(reason) => OrchdResponse::Error {
            code: OrchdErrorCode::Policy,
            message: format!("connector invoke denied: {reason}"),
        },
        ConnectorInvokeError::Persist(inner) => map_err(inner),
        ConnectorInvokeError::Adapter(err) => OrchdResponse::Error {
            code: OrchdErrorCode::Io,
            message: err.to_string(),
        },
    }
}

/// A mutating MCP-server verb's shared reply/push shape (spec §5/§6, task T6):
/// `McpServersChanged{project_id}` on success — the server's OWN `project_id` (`None` for a
/// global-scope server). Shared by `McpAddServer`/`McpUpdateServer`/`McpSetServerEnabled` — every
/// OTHER mcp-server verb below has a reply shape this helper doesn't fit (`McpDeleteServer`
/// replies a bare `Ack`, mirroring [`goal_project_id`]'s pre-delete-lookup shape;
/// `McpSetServerBearer` is a two-step Keychain-then-DB write that also replies `Ack`, not the
/// entity) and is handled inline in its own arm instead.
fn respond_mcp_server(
    result: Result<mcp::McpServerRow, OrchdPersistError>,
    broadcaster: &Broadcaster,
) -> OrchdResponse {
    match result {
        Ok(row) => {
            broadcaster.broadcast(OrchdFrame::Push(OrchdPush::McpServersChanged {
                project_id: row.project_id.clone(),
            }));
            OrchdResponse::McpServer(row.into())
        }
        Err(e) => map_err(e),
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

/// Broadcasts `GraphChanged` once per DISTINCT project id in `project_ids` (S4 spec §6: "every
/// affected project, deduped") — the single fan-out point every mutating graph verb's dispatch
/// arm below funnels through, so the dedup rule lives in exactly one place rather than being
/// re-implemented per verb.
fn broadcast_graph_changed(
    broadcaster: &Broadcaster,
    project_ids: impl IntoIterator<Item = String>,
) {
    let mut seen = std::collections::HashSet::new();
    for project_id in project_ids {
        if seen.insert(project_id.clone()) {
            broadcaster.broadcast(OrchdFrame::Push(OrchdPush::GraphChanged { project_id }));
        }
    }
}

/// Shared reply/push shape for `GraphUpdateNode`/`GraphMoveNode` (S4 spec §6): neither verb
/// touches the node's incident edges, so the affected-project set is
/// `node_project_ids_reachable(id)` — the node's own project PLUS every foreign project it's
/// reachable into via an incident edge (the node shows up there as an `external_nodes` ghost, so
/// that project's `GraphListProject` view must be invalidated too). Read AFTER the mutation
/// (unlike the delete verbs, which must read BEFORE) — update/move never changes the node's own
/// project or its incident edges, so the reachable set is identical either way, and reading here
/// lets this reuse the SAME `db` guard the mutation itself already holds.
fn respond_graph_node_reachable(
    db: &Db,
    result: Result<GraphNode, OrchdPersistError>,
    broadcaster: &Broadcaster,
) -> OrchdResponse {
    match result {
        Ok(node) => {
            let project_ids = db.node_project_ids_reachable(&node.id).unwrap_or_else(|e| {
                // Unreachable in practice: `node` was JUST successfully mutated under this same
                // `db` guard, and the single `Arc<Mutex<Db>>` serializes every request, so nothing
                // could have deleted it in between. Degrade honestly rather than dropping the
                // whole reply: log and fall back to the node's own project only.
                tracing::error!(
                    node_id = %node.id,
                    error = %e,
                    "node_project_ids_reachable failed immediately after a successful node \
                     mutation; falling back to the node's own project only"
                );
                vec![node.project_id.clone()]
            });
            broadcast_graph_changed(broadcaster, project_ids);
            OrchdResponse::GraphNode(node)
        }
        Err(e) => map_err(e),
    }
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

/// The single per-request completion-tracing choke-point (spec D4, O-6). Wraps [`dispatch_inner`]
/// so EVERY verb — the whole `OrchdRequest` enum, plus any future one — emits exactly one
/// structured `info!` line when its response is ready, without a per-arm edit and without any arm
/// being able to opt out (the wrap captures every return path, including `dispatch_inner`'s early
/// `return`s).
///
/// The line carries `verb` (from the exhaustive [`OrchdRequest::verb_name`] — a compile-time guard
/// that a new verb is named), `outcome` (`"ok"`/`"err"` derived from whether the response is
/// [`OrchdResponse::Error`]), `error_code` (the `OrchdErrorCode` debug name, present only on an
/// error), and `elapsed_ms`. It NEVER carries args, bodies, tokens, tool output, ids, or any other
/// payload value — only that low-cardinality quartet (enforced by
/// `tests/no_secrets_in_logs_tracing.rs`).
async fn dispatch(
    deps: &Arc<ServerDeps>,
    broadcaster: &Broadcaster,
    req: OrchdRequest,
) -> OrchdResponse {
    let verb = req.verb_name();
    let started = std::time::Instant::now();
    let res = dispatch_inner(deps, broadcaster, req).await;
    let elapsed_ms = started.elapsed().as_millis() as u64;
    match &res {
        OrchdResponse::Error { code, .. } => {
            tracing::info!(verb, outcome = "err", error_code = ?code, elapsed_ms, "request completed");
        }
        _ => {
            tracing::info!(verb, outcome = "ok", elapsed_ms, "request completed");
        }
    }
    res
}

/// Dispatch one `OrchdRequest` to the right subsystem and produce the correlated `OrchdResponse`
/// (spec §4.2, §5, §6, §7): every domain verb below is a thin translation between the wire
/// request and a `persistence::Db` (T6-T8) / `export` (T9) call, plus — on success only — the
/// matching coarse push via `broadcaster` (spec §6: "Failed requests broadcast NOTHING"). The
/// per-request completion trace is added ONCE by the [`dispatch`] wrapper above, so no arm here
/// logs its own outcome.
async fn dispatch_inner(
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

        // ---- Knowledge graph (S4 spec §6): every mutating verb below broadcasts `GraphChanged`
        // to every AFFECTED project (not just the mutated row's own), deduped, on success only —
        // via `broadcast_graph_changed`/[`respond_graph_node_reachable`] — because a cross-project
        // edge's foreign endpoint appears as an `external_nodes` ghost in that project's
        // `GraphListProject` view too (D7 "coarse invalidation, zero drift"). Read verbs
        // (`GraphListProject`/`GraphNeighborhood`/`GraphSearch`) broadcast nothing. ----
        OrchdRequest::GraphAddNode {
            project_id,
            kind,
            label,
            body,
            pos_x,
            pos_y,
        } => {
            let result = {
                let db = deps.db.lock().await;
                db.add_node(&project_id, kind, &label, &body, pos_x, pos_y)
            };
            match result {
                Ok(node) => {
                    broadcast_graph_changed(broadcaster, [node.project_id.clone()]);
                    OrchdResponse::GraphNode(node)
                }
                Err(e) => map_err(e),
            }
        }
        OrchdRequest::GraphUpdateNode { id, label, body } => {
            let db = deps.db.lock().await;
            let result = db.update_node(&id, label.as_deref(), body.as_deref());
            respond_graph_node_reachable(&db, result, broadcaster)
        }
        OrchdRequest::GraphMoveNode { id, pos_x, pos_y } => {
            let db = deps.db.lock().await;
            let result = db.move_node(&id, pos_x, pos_y);
            respond_graph_node_reachable(&db, result, broadcaster)
        }
        // `node_project_ids_reachable` is resolved BEFORE `delete_node`: the cascade removes the
        // node's incident cross-project edges, so the foreign endpoints would be unreachable from
        // it afterward (mirrors `goal_project_id`/`task_project_id`'s pre-delete lookup pattern).
        OrchdRequest::GraphDeleteNode { id } => {
            let db = deps.db.lock().await;
            match db.node_project_ids_reachable(&id) {
                Ok(project_ids) => match db.delete_node(&id) {
                    Ok(()) => {
                        broadcast_graph_changed(broadcaster, project_ids);
                        OrchdResponse::Ack
                    }
                    Err(e) => map_err(e),
                },
                Err(e) => map_err(e),
            }
        }
        OrchdRequest::GraphAddEdge {
            source_node_id,
            target_node_id,
            kind,
            label,
        } => {
            let db = deps.db.lock().await;
            match db.add_edge(&source_node_id, &target_node_id, kind, &label) {
                Ok(edge) => {
                    match db.edge_endpoint_projects(&edge.id) {
                        Ok((source_project, target_project)) => {
                            // Same-project edge ⇒ the two ids are equal ⇒ ONE push
                            // (`broadcast_graph_changed` dedups).
                            broadcast_graph_changed(broadcaster, [source_project, target_project]);
                        }
                        Err(e) => {
                            // Unreachable in practice: the edge was JUST inserted referencing
                            // these exact node ids, under the same serializing `db` guard.
                            tracing::error!(
                                edge_id = %edge.id,
                                error = %e,
                                "edge_endpoint_projects failed immediately after a successful \
                                 add_edge; GraphChanged push skipped"
                            );
                        }
                    }
                    OrchdResponse::GraphEdge(edge)
                }
                Err(e) => map_err(e),
            }
        }
        // `edge_endpoint_projects` is resolved BEFORE `delete_edge`: the row (and its endpoint
        // join) is gone afterward.
        OrchdRequest::GraphDeleteEdge { id } => {
            let db = deps.db.lock().await;
            match db.edge_endpoint_projects(&id) {
                Ok((source_project, target_project)) => match db.delete_edge(&id) {
                    Ok(()) => {
                        broadcast_graph_changed(broadcaster, [source_project, target_project]);
                        OrchdResponse::Ack
                    }
                    Err(e) => map_err(e),
                },
                Err(e) => map_err(e),
            }
        }
        OrchdRequest::GraphListProject { project_id } => {
            let db = deps.db.lock().await;
            match db.list_project_graph(&project_id) {
                Ok(view) => OrchdResponse::GraphView(view),
                Err(e) => map_err(e),
            }
        }
        OrchdRequest::GraphNeighborhood { node_id, depth } => {
            let db = deps.db.lock().await;
            match db.neighborhood(&node_id, depth) {
                Ok(n) => OrchdResponse::Neighborhood(n),
                Err(e) => map_err(e),
            }
        }
        OrchdRequest::GraphSearch { query, project_id } => {
            let db = deps.db.lock().await;
            match db.search_nodes(&query, project_id.as_deref()) {
                Ok(nodes) => OrchdResponse::GraphNodes(nodes),
                Err(e) => map_err(e),
            }
        }
        // ---- S-EXT MCP (spec §5/§6, task T6): every mutating server verb below ⇒
        // `McpServersChanged{project_id}` on success (via `respond_mcp_server` or inline);
        // `McpConnect` ⇒ `McpToolsChanged{server_id}`; `McpCallTool` ⇒
        // `McpArtifactsChanged{project_id}` AND `McpInvocationLogged{server_id}`;
        // `McpSetToolEnabled` ⇒ `McpToolsChanged{server_id}`; `TrustGrantConsent` ⇒
        // `McpServersChanged{project_id}` (so the UI reflects the new consent state). Every read
        // verb (`McpListServers`/`McpListTools`/`McpListInvocations`/`McpListArtifacts`/
        // `McpGetArtifact`) broadcasts nothing. `McpDisconnect` is a Phase-1 no-op (spec: connect-
        // per-call, no live session to tear down) — `Ack`, no DB access, no push. `Err` ⇒
        // `map_err`/`map_mcp_err`/`map_secret_err`, nothing broadcast (spec §6: "Failed requests
        // broadcast NOTHING"). ----
        OrchdRequest::McpAddServer {
            name,
            transport,
            url,
            command,
            args,
            env,
            scope,
            project_id,
            auth_kind,
            timeout_ms,
            max_retries,
        } => {
            let result = {
                let db = deps.db.lock().await;
                db.add_mcp_server(mcp::NewMcpServer {
                    name,
                    transport: transport.into(),
                    url,
                    command,
                    args: args.unwrap_or_default(),
                    env: env.unwrap_or_default(),
                    scope: scope.into(),
                    project_id,
                    auth_kind: auth_kind.into(),
                    secret_ref: None,
                    account_id: None,
                    enabled: true,
                    timeout_ms: timeout_ms.unwrap_or(30_000),
                    max_retries: max_retries.unwrap_or(2),
                })
            };
            respond_mcp_server(result, broadcaster)
        }
        OrchdRequest::McpListServers { project_id } => {
            let db = deps.db.lock().await;
            match db.list_mcp_servers(project_id.as_deref()) {
                Ok(v) => OrchdResponse::McpServers(v.into_iter().map(Into::into).collect()),
                Err(e) => map_err(e),
            }
        }
        OrchdRequest::McpUpdateServer {
            id,
            name,
            url,
            command,
            args,
            env,
            auth_kind,
            timeout_ms,
            max_retries,
        } => {
            let result = {
                let db = deps.db.lock().await;
                db.update_mcp_server(
                    &id,
                    mcp::McpServerPatch {
                        name,
                        url,
                        command,
                        args,
                        env,
                        auth_kind: auth_kind.map(Into::into),
                        secret_ref: None,
                        account_id: None,
                        timeout_ms,
                        max_retries,
                    },
                )
            };
            respond_mcp_server(result, broadcaster)
        }
        OrchdRequest::McpSetServerEnabled { id, enabled } => {
            let result = {
                let db = deps.db.lock().await;
                db.set_mcp_server_enabled(&id, enabled)
            };
            respond_mcp_server(result, broadcaster)
        }
        // `get_mcp_server` resolves the row (and its `project_id`) BEFORE `delete_mcp_server`:
        // the row is gone afterward (mirrors `goal_project_id`/`task_project_id`'s pre-delete
        // lookup pattern — here the lookup returns the whole row since the push needs
        // `project_id`, not just an id captured off a raw query).
        //
        // Keychain-then-DB ordering (S-EXT whole-branch final-review fix: mirrors
        // `connectors::accounts::Db::delete_account`'s own "Keychain first, fail-closed"
        // precedent — see that method's doc comment). The bearer Keychain entry at
        // `bpa_secrets::mcp_bearer_ref(&id)` is deleted BEFORE the SQL row disappears;
        // `SecretError::NotFound` is treated as success (idempotent, and expected for the common
        // case — a server with `auth_kind='none'`/`'oauth'` never had a bearer entry to begin
        // with, only `McpSetServerBearer` ever wrote one). Any OTHER Keychain failure aborts
        // before the row is removed, so a live credential never ends up with no DB reference
        // pointing at it — without this, the DELETE was asymmetric with `ConnectorDeleteAccount`
        // (which already cleans up its own Keychain entries) and orphaned bearer tokens in the
        // real Keychain on every server delete. `delete_secret_ignoring_not_found`
        // (`connectors::accounts`) is private to that module and not reachable here, so the same
        // match is inlined via `map_secret_err` (already used by `McpSetServerBearer` below for
        // the mirror-image Keychain write failure).
        OrchdRequest::McpDeleteServer { id } => {
            let db = deps.db.lock().await;
            match db.get_mcp_server(&id) {
                Ok(server) => {
                    match bpa_secrets::delete(&bpa_secrets::mcp_bearer_ref(&id)) {
                        Ok(()) | Err(bpa_secrets::SecretError::NotFound) => {}
                        Err(e) => return map_secret_err(e),
                    }
                    match db.delete_mcp_server(&id) {
                        Ok(()) => {
                            broadcaster.broadcast(OrchdFrame::Push(OrchdPush::McpServersChanged {
                                project_id: server.project_id,
                            }));
                            OrchdResponse::Ack
                        }
                        Err(e) => map_err(e),
                    }
                }
                Err(e) => map_err(e),
            }
        }
        // Existence-checked BEFORE the Keychain write (no stray Keychain entry left behind for
        // an unknown `id`); the token itself is never bound to a tracing field or an error
        // message anywhere on this path (spec D4/§5: "token NEVER logged/echoed" —
        // `bpa_secrets::SecretError`'s own `Display` structurally cannot carry it either, see
        // [`map_secret_err`]). `secret_ref` is stored as `id` itself — `bpa_secrets::
        // mcp_bearer_ref(id).account == id` (`crate::mcp::resolve_bearer` derives the Keychain
        // ref straight from the server id, never from this stored column — this column is the
        // human/UI-facing "a secret IS stored" marker, spec §4: "Keychain account key for
        // bearer").
        OrchdRequest::McpSetServerBearer { id, token } => {
            let exists = {
                let db = deps.db.lock().await;
                db.get_mcp_server(&id)
            };
            if let Err(e) = exists {
                return map_err(e);
            }
            if let Err(e) = bpa_secrets::set(&bpa_secrets::mcp_bearer_ref(&id), token.as_bytes()) {
                return map_secret_err(e);
            }
            let db = deps.db.lock().await;
            let result = db.set_mcp_server_secret_ref(&id, &id).and_then(|_| {
                db.update_mcp_server(
                    &id,
                    mcp::McpServerPatch {
                        auth_kind: Some(mcp::McpAuthKind::Bearer),
                        ..Default::default()
                    },
                )
            });
            match result {
                Ok(row) => {
                    broadcaster.broadcast(OrchdFrame::Push(OrchdPush::McpServersChanged {
                        project_id: row.project_id,
                    }));
                    OrchdResponse::Ack
                }
                Err(e) => map_err(e),
            }
        }
        // Trust-gated (spec D10): a denied connect (no valid `consent_grant` for the server's
        // CURRENT url) never reaches the network — `mcp::lifecycle::connect` returns
        // `OrchdMcpError::ConsentRequired` BEFORE calling `connect_fn`, mapped to `Error{Consent}`
        // by `map_mcp_err`, no push. `mcp::connect_session` is the SAME production factory
        // reported in the task-5 handoff — the only transport Phase 1 ships is HTTP (spec D6).
        OrchdRequest::McpConnect { id } => {
            // Pass the shared `Arc<Mutex<Db>>` directly (NOT a locked guard):
            // `mcp::lifecycle::connect` locks it itself in two short phases around the network
            // round-trip, holding NO guard across the MCP awaits (T6 review fix — a held guard
            // here would stall every other orchd connection for the whole round-trip).
            match mcp::lifecycle::connect(&deps.db, &id, mcp::connect_session).await {
                Ok(report) => {
                    broadcaster.broadcast(OrchdFrame::Push(OrchdPush::McpToolsChanged {
                        server_id: id,
                    }));
                    OrchdResponse::McpConnectReport(report)
                }
                Err(e) => map_mcp_err(e),
            }
        }
        // Phase-1 connect-per-call (spec: no live session to tear down between calls) — a no-op/
        // best-effort acknowledgement, no DB access, no push.
        OrchdRequest::McpDisconnect { .. } => OrchdResponse::Ack,
        OrchdRequest::McpListTools { server_id } => {
            let db = deps.db.lock().await;
            match db.list_mcp_tools(&server_id) {
                Ok(v) => OrchdResponse::McpTools(v.into_iter().map(Into::into).collect()),
                Err(e) => map_err(e),
            }
        }
        OrchdRequest::McpSetToolEnabled { tool_id, enabled } => {
            let result = {
                let db = deps.db.lock().await;
                db.set_mcp_tool_enabled(&tool_id, enabled)
            };
            match result {
                Ok(row) => {
                    broadcaster.broadcast(OrchdFrame::Push(OrchdPush::McpToolsChanged {
                        server_id: row.server_id.clone(),
                    }));
                    OrchdResponse::McpTool(row.into())
                }
                Err(e) => map_err(e),
            }
        }
        // Trust-gated (spec §6): a disabled/unrecognized tool is denied BEFORE any network call,
        // args parsing, or bearer resolution (`mcp::invoke::call_tool`'s own guarantee) —
        // `OrchdMcpError::ToolDisabled`, mapped to `Error{Policy}` by `map_mcp_err`, no push, no
        // invocation/artifact row written. A successful call ALSO writes `mcp_invocation`, even
        // when the tool's own result is `is_error:true` (a tool-level failure inside an otherwise-
        // successful RPC) — both pushes still fire, mirroring `mcp::invoke::call_tool`'s own
        // "the RPC itself completed" distinction.
        OrchdRequest::McpCallTool {
            server_id,
            tool_name,
            args_json,
            project_id,
        } => {
            // Pass the shared `Arc<Mutex<Db>>` directly (NOT a locked guard): `mcp::invoke::
            // call_tool` locks it itself in short phases around the network round-trip + retry
            // loop, holding NO guard across the MCP awaits (T6 review fix).
            match mcp::invoke::call_tool(
                &deps.db,
                &server_id,
                &tool_name,
                &args_json,
                project_id.clone(),
                mcp::connect_session,
            )
            .await
            {
                Ok(result) => {
                    broadcaster.broadcast(OrchdFrame::Push(OrchdPush::McpArtifactsChanged {
                        project_id,
                    }));
                    broadcaster.broadcast(OrchdFrame::Push(OrchdPush::McpInvocationLogged {
                        server_id,
                    }));
                    OrchdResponse::McpCallResult(result)
                }
                Err(e) => map_mcp_err(e),
            }
        }
        OrchdRequest::McpListInvocations {
            server_id,
            project_id,
            limit,
        } => {
            let db = deps.db.lock().await;
            match db.list_invocations(server_id.as_deref(), project_id.as_deref(), limit) {
                Ok(v) => OrchdResponse::McpInvocations(v),
                Err(e) => map_err(e),
            }
        }
        OrchdRequest::McpListArtifacts {
            project_id,
            server_id,
            limit,
        } => {
            let db = deps.db.lock().await;
            match db.list_artifacts(project_id.as_deref(), server_id.as_deref(), limit) {
                Ok(v) => OrchdResponse::McpArtifacts(v),
                Err(e) => map_err(e),
            }
        }
        OrchdRequest::McpGetArtifact { id } => {
            let db = deps.db.lock().await;
            match db.get_artifact(&id) {
                Ok(a) => OrchdResponse::McpArtifact(a),
                Err(e) => map_err(e),
            }
        }
        // Direct grant — NOT itself gated by `trust::authorize` (granting consent IS the
        // gate-setting action, not something that needs to pass through the choke-point it
        // configures). `kind` is `'connect'` (http, fingerprint = URL) or `'stdio_exec'` (stdio,
        // task T16, fingerprint = the resolved-binary/command-args hash) — `mcp::fingerprint_for`
        // is the SAME function `mcp::connect_action` calls at authorize time, so a grant made
        // here and a later `McpConnect`/`McpCallTool` authorize check always agree on what "the
        // current fingerprint" is.
        OrchdRequest::TrustGrantConsent { server_id, kind } => {
            let db = deps.db.lock().await;
            let server = match db.get_mcp_server(&server_id) {
                Ok(s) => s,
                Err(e) => return map_err(e),
            };
            let fingerprint = mcp::fingerprint_for(&server, &kind);
            match db.grant_consent(&server_id, &kind, &fingerprint) {
                Ok(()) => {
                    broadcaster.broadcast(OrchdFrame::Push(OrchdPush::McpServersChanged {
                        project_id: server.project_id,
                    }));
                    OrchdResponse::Ack
                }
                Err(e) => map_err(e),
            }
        }

        // ---- S-EXT Connectors / accounts (spec §5/§6/§7, task T13a): every mutating verb below
        // ⇒ `ConnectorsChanged` on success (spec §5's pushes list — no payload, the `account`
        // table has no `project_id` to scope by); `ConnectorInvoke` additionally ⇒
        // `McpArtifactsChanged{project_id}` (reuses the SAME artifact/invocation persistence path
        // as `McpCallTool`, spec §6/D9 — see `connectors::adapter::invoke`'s own doc comment). No
        // `McpInvocationLogged` push here: that push's wire shape is `{server_id: String}`
        // (non-optional), and a `ConnectorInvoke` has no `server_id` at all (`account_id` only) —
        // rather than force a fake server id through it, this dispatch arm simply doesn't send
        // that push for a connector call; `McpArtifactsChanged` alone is what the artifacts UI
        // actually refetches on. Every read verb (`ConnectorListAccounts`/`ConnectorListOps`)
        // broadcasts nothing. `Err` ⇒ `map_connector_err`/`map_connector_invoke_err`, nothing
        // broadcast (spec §6: "Failed requests broadcast NOTHING"). ----
        OrchdRequest::ConnectorBeginOAuth {
            provider,
            label,
            scopes,
            server_id: _,
        } => {
            // `server_id` (an optional MCP-server-OAuth link) is unused in v1: `begin_oauth`'s
            // Phase-2 scope is the standalone connector-account flow only (spec §5/§7) — an
            // MCP-server-OAuth consumer is a documented future extension, not built here.
            match deps.connectors.begin_oauth(
                &provider,
                &label,
                &scopes.unwrap_or_default(),
                CONNECTOR_OAUTH_REDIRECT,
            ) {
                Ok(challenge) => OrchdResponse::OAuthChallenge(challenge),
                Err(e) => map_connector_err(e),
            }
        }
        OrchdRequest::ConnectorCompleteOAuth { state, code } => {
            // `complete_oauth` locks `deps.db` itself in a short phase AFTER its own network
            // round-trip (the token exchange) — pass the SHARED handle, not a locked guard, same
            // T6-review discipline as `McpConnect`/`McpCallTool` above.
            match deps
                .connectors
                .complete_oauth(&deps.db, &state, &code)
                .await
            {
                Ok(row) => {
                    broadcaster.broadcast(OrchdFrame::Push(OrchdPush::ConnectorsChanged));
                    OrchdResponse::Account(row.into())
                }
                Err(e) => map_connector_err(e),
            }
        }
        OrchdRequest::ConnectorAddApiKey {
            provider,
            label,
            api_key,
        } => {
            // No network I/O (`add_apikey` only writes Keychain + the DB row) — lock `db` for the
            // duration, mirrors every other plain-CRUD dispatch arm above.
            let result = {
                let db = deps.db.lock().await;
                deps.connectors.add_apikey(&db, &provider, &label, &api_key)
            };
            match result {
                Ok(row) => {
                    broadcaster.broadcast(OrchdFrame::Push(OrchdPush::ConnectorsChanged));
                    OrchdResponse::Account(row.into())
                }
                Err(e) => map_connector_err(e),
            }
        }
        OrchdRequest::ConnectorListAccounts => {
            let db = deps.db.lock().await;
            match db.list_accounts() {
                Ok(v) => OrchdResponse::Accounts(v.into_iter().map(Into::into).collect()),
                Err(e) => map_err(e),
            }
        }
        OrchdRequest::ConnectorDeleteAccount { id } => {
            let result = {
                let db = deps.db.lock().await;
                db.delete_account(&id)
            };
            match result {
                Ok(()) => {
                    broadcaster.broadcast(OrchdFrame::Push(OrchdPush::ConnectorsChanged));
                    OrchdResponse::Ack
                }
                Err(e) => map_err(e),
            }
        }
        OrchdRequest::ConnectorListOps { account_id } => {
            let account = {
                let db = deps.db.lock().await;
                db.get_account(&account_id)
            };
            match account {
                Ok(account) => match connectors::adapter::list_ops(&account.provider) {
                    Ok(ops) => OrchdResponse::ConnectorOps(ops),
                    Err(e) => map_connector_err(e),
                },
                Err(e) => map_err(e),
            }
        }
        // Reuses the MCP call/artifact/invocation path (spec §6: "connector_invoke passes through
        // trust::authorize IDENTICALLY to McpCallTool"). `connectors::adapter::invoke` locks
        // `deps.db` itself in short phases around its own network round-trips (bearer resolution
        // — which may hit an OAuth refresh endpoint — plus the adapter's own HTTP call), holding
        // no guard across either await — same T6-review discipline as `McpCallTool` above.
        OrchdRequest::ConnectorInvoke {
            account_id,
            op,
            args_json,
            project_id,
        } => {
            match connectors::adapter::invoke(
                &deps.connectors,
                &deps.db,
                &account_id,
                &op,
                &args_json,
                project_id.clone(),
            )
            .await
            {
                Ok(result) => {
                    broadcaster.broadcast(OrchdFrame::Push(OrchdPush::McpArtifactsChanged {
                        project_id,
                    }));
                    OrchdResponse::McpCallResult(result)
                }
                Err(e) => map_connector_invoke_err(e),
            }
        }

        // ---- S-EXT Skills (spec §4/§5/§8, D11, Q14, task T17): a plumbing-only registry — see
        // `bpa_orchd_proto::Skill`'s own doc comment ("no runtime consumer until S6b agent org").
        // `SkillAdd`/`SkillDelete` ⇒ `SkillsChanged{project_id}` on success (mirrors
        // `McpServersChanged`'s scoping exactly — `project_id: None` for a global-scope skill);
        // `SkillList` is a READ and broadcasts nothing. `Err` ⇒ `map_err` (every failure mode here
        // is `OrchdPersistError`, no MCP/connector-specific error family involved), nothing
        // broadcast (spec §6: "Failed requests broadcast NOTHING"). ----
        OrchdRequest::SkillAdd {
            name,
            description,
            md_path,
            scope,
            project_id,
        } => {
            let result = {
                let db = deps.db.lock().await;
                db.add_skill(NewSkill {
                    name,
                    description,
                    md_path,
                    scope: match scope {
                        bpa_orchd_proto::SkillScope::Global => skills::SkillScope::Global,
                        bpa_orchd_proto::SkillScope::Project => skills::SkillScope::Project,
                    },
                    project_id,
                })
            };
            match result {
                Ok(row) => {
                    let view = row.into_view();
                    let project_id = view.skill.project_id.clone();
                    broadcaster
                        .broadcast(OrchdFrame::Push(OrchdPush::SkillsChanged { project_id }));
                    OrchdResponse::Skill(view.into())
                }
                Err(e) => map_err(e),
            }
        }
        OrchdRequest::SkillList { project_id } => {
            let db = deps.db.lock().await;
            match db.list_skills(project_id.as_deref()) {
                Ok(views) => OrchdResponse::Skills(views.into_iter().map(Into::into).collect()),
                Err(e) => map_err(e),
            }
        }
        OrchdRequest::SkillDelete { id } => {
            // `delete_skill` doesn't return the deleted row's `project_id`, and the push payload
            // needs it (spec §5's `SkillsChanged{project_id}` mirrors `McpServersChanged`'s
            // shape) — look the row up first, THEN delete, THEN broadcast the scope it actually
            // affected. Mirrors `McpDeleteServer`'s exact "get_mcp_server, then
            // delete_mcp_server" shape above, one entity later.
            let db = deps.db.lock().await;
            match db.get_skill(&id) {
                Ok(skill) => match db.delete_skill(&id) {
                    Ok(()) => {
                        broadcaster.broadcast(OrchdFrame::Push(OrchdPush::SkillsChanged {
                            project_id: skill.project_id,
                        }));
                        OrchdResponse::Ack
                    }
                    Err(e) => map_err(e),
                },
                Err(e) => map_err(e),
            }
        }

        // ---- S-EXT Trust: policy caps + audit log (spec §4/§5/§6, BL-22, task T18) ----
        // `TrustSetPolicy` ⇒ `PoliciesChanged` on success (no payload — see `OrchdPush::
        // PoliciesChanged`'s own doc comment: a policy change can be global/project/server
        // scoped, so there's no single natural id to name coarsely). `TrustListPolicies`/
        // `TrustListAudit` are READS and broadcast nothing. `Err` ⇒ `map_err` (every failure mode
        // here is `OrchdPersistError` — `upsert_policy`'s own scope/ref_id validation surfaces as
        // `Validation`), nothing broadcast (spec §6: "Failed requests broadcast NOTHING"). ----
        OrchdRequest::TrustSetPolicy {
            scope,
            ref_id,
            spend_cap_usd,
            rate_per_min,
        } => {
            let result = {
                let db = deps.db.lock().await;
                db.upsert_policy(NewPolicy {
                    scope,
                    ref_id,
                    spend_cap_usd,
                    rate_per_min,
                })
            };
            match result {
                Ok(policy) => {
                    broadcaster.broadcast(OrchdFrame::Push(OrchdPush::PoliciesChanged));
                    OrchdResponse::Policy(policy)
                }
                Err(e) => map_err(e),
            }
        }
        OrchdRequest::TrustListPolicies => {
            let db = deps.db.lock().await;
            match db.list_policies() {
                Ok(v) => OrchdResponse::Policies(v),
                Err(e) => map_err(e),
            }
        }
        OrchdRequest::TrustListAudit { limit } => {
            let db = deps.db.lock().await;
            match db.list_audit(limit) {
                Ok(v) => OrchdResponse::AuditRows(v),
                Err(e) => map_err(e),
            }
        }

        // ---- S-IDEA research (spec §5/§6, task T5) ----
        // `ResearchStartRun` -> [`research::start_run`] (T4): ONE call does the whole spec §6
        // steps 1-3 — insert `research_run{pending}` + the only-if-captured idea lifecycle flip
        // (its own `unchecked_transaction()`), THEN `tokio::spawn`s the background run driver
        // (`research::run_research`) against the SHARED `Arc<Mutex<Db>>`/`Broadcaster` and
        // returns the freshly-inserted `pending` row immediately. The reply below is exactly that
        // `pending` row — this arm must NOT broadcast `ResearchRunsChanged` itself: the spawned
        // driver ALREADY fires that push on every transition it drives (running/done/failed,
        // `research::run_research`'s own doc comment), so pushing here too would double-fire for
        // the SAME `pending`->`running` transition the driver's own phase 1 push already covers.
        // Pass the shared `Arc<Mutex<Db>>` + `broadcaster` directly (not a locked guard) — mirrors
        // `McpConnect`/`McpCallTool` above: `start_run` only holds the guard for its own short
        // insert+resolve phase, then drops it before spawning, so no guard is ever held across the
        // driver's later network `.await`.
        OrchdRequest::ResearchStartRun {
            idea_id,
            server_id,
            tool_name,
            args_json,
        } => {
            match research::start_run(
                &deps.db,
                broadcaster,
                research::NewResearchRun {
                    idea_id,
                    server_id,
                    tool_name,
                    args_json,
                },
            )
            .await
            {
                Ok(row) => OrchdResponse::ResearchRun(row.into()),
                Err(e) => map_err(e),
            }
        }
        // Plain read (spec §5: "runs for an idea, newest first") — no push, mirrors every other
        // `List*` verb in this file.
        OrchdRequest::ResearchListRuns { idea_id } => {
            let db = deps.db.lock().await;
            match db.list_research_runs(&idea_id) {
                Ok(v) => OrchdResponse::ResearchRuns(v.into_iter().map(Into::into).collect()),
                Err(e) => map_err(e),
            }
        }
        // Plain read — no push. `Db::get_research_run` is `Ok(None)` for an unknown id (its own
        // doc comment: a `ResearchGetRun` client could race a delete) — that honest `Option`
        // degradation is remapped to the wire `NotFound` error here, matching every other
        // single-row getter's "unknown id -> Error{NotFound}" contract (e.g. `McpGetArtifact`
        // above, whose `Db::get_artifact` already returns `Err(NotFound)` directly).
        OrchdRequest::ResearchGetRun { id } => {
            let db = deps.db.lock().await;
            match db.get_research_run(&id) {
                Ok(Some(row)) => OrchdResponse::ResearchRun(row.into()),
                Ok(None) => map_err(OrchdPersistError::NotFound),
                Err(e) => map_err(e),
            }
        }
        // Storage-degradation mode (spec D3, BL-94): fixed at boot, returned verbatim — no DB
        // access, so the frontend can read it even in the in-memory-fallback / recovered modes.
        OrchdRequest::GetStorageStatus => OrchdResponse::StorageStatus(deps.storage_status.clone()),
    }
}
