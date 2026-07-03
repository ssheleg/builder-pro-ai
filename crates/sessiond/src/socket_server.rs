//! Hop-B socket server (spec §7, §8.2, §11, §13, §16): a tokio `UnixListener` accept loop with
//! one task per connected client, the `Hello` handshake gate, request/response correlation, a
//! bounded per-client outbound queue (overflow ⇒ drop+disconnect), peer-cred refusal of foreign
//! euids, supervisor→`Push` fan-out to every connected client, DB persistence, and daemon-side
//! path validation.
//!
//! ## What this module owns vs. what Task 13 owns
//!
//! [`serve`] is the pure server entry point: given an already-bound [`UnixListener`], a
//! [`ServerDeps`] bundle (supervisor / db / attach / build string), and a `watch` shutdown
//! receiver, it runs the accept loop until `shutdown` flips to `true` or the listener errors, then
//! returns. Task 13 (daemon boot) owns process concerns around it: the `flock` single-instance
//! lock, socket-dir permissions, `bind` + stale-socket unlink, SIGTERM handling, and flipping the
//! shutdown `watch`. `serve` never touches those — it is drivable in isolation by tests.
//!
//! ## Framing (spec §7)
//!
//! Every wire message is `u32`-LE length + `bincode(Frame)`. We reuse the protocol crate's own
//! codec ([`crate::protocol::encode_frame`] / [`crate::protocol::FrameDecoder`] /
//! [`crate::protocol::MAX_FRAME_LEN`]) verbatim so both sides share one implementation and the
//! oversized-length / partial-frame rules can never drift. `FrameDecoder` buffers partial frames
//! across socket reads; an oversized declared length is a hard [`FrameError::Oversized`] that
//! disconnects the client without allocating the bogus body.
//!
//! ## Bounded outbound queue + non-stalling writer (spec §13)
//!
//! Each client owns exactly one writer task draining a bounded `mpsc::Sender<Frame>` of depth
//! [`CLIENT_OUTQ_CAP`]. Every producer — the per-request dispatch reply, the broadcast fan-out of
//! supervisor callbacks, and the `AttachRegistry` push pump — enqueues through that one queue with
//! `try_send`. On `TrySendError::Full` (a client that stopped reading) or an `EPIPE`/write error in
//! the writer, that client is dropped and disconnected and its attach entries are torn down —
//! without ever blocking another client or pausing an unrelated session's PTY. No producer ever
//! `.await`s on a full queue, so one dead client cannot exert backpressure on the supervisor.

use std::sync::Arc;

use tokio::io::AsyncWriteExt;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{mpsc, watch, Mutex};

use crate::attach::{AttachError, AttachRegistry, PushSink};
use crate::persistence::Db;
use crate::pty_supervisor::{SessionSpec, StatusUpdate, Supervisor, SupervisorError};
use crate::shell_integration::{classify_shell, write_session_assets};
use crate::singleton::check_peer_cred;

use bpa_protocol::{
    encode_frame, Frame, FrameDecoder, Push, Request, Response, SessionMeta, Workspace, MAGIC,
    PROTO_VERSION,
};

/// Per-client bounded outbound queue depth (frames). Overflow (a client that stopped reading) ⇒
/// drop + disconnect that client rather than buffer unboundedly (spec §13, no memory-DoS).
pub const CLIENT_OUTQ_CAP: usize = 1024;

/// Cadence of the best-effort scrollback persistence sweep (spec §11: batched, ~500 ms).
const SCROLLBACK_FLUSH_INTERVAL: std::time::Duration = std::time::Duration::from_millis(1000);

/// Shared dependency bundle handed to the server and every per-client task.
///
/// `db` is `Arc<Mutex<Db>>` (not `Arc<Db>`) because [`Db`] holds a `rusqlite::Connection` and is
/// `Send + !Sync`: the async Mutex both makes it shareable across the per-client tasks and
/// serializes access to the single connection. Task 13 constructs this once and passes it to
/// [`serve`].
pub struct ServerDeps {
    pub supervisor: Arc<Supervisor>,
    pub db: Arc<Mutex<Db>>,
    pub attach: Arc<AttachRegistry>,
    /// Human-readable daemon build string echoed in `Response::Welcome` (spec §7).
    pub daemon_build: String,
    /// Per-session runtime dir root for shell-integration assets (ZDOTDIR / bpa-bash.sh). Task 13
    /// passes the resolved socket dir; tests pass a tempdir.
    pub runtime_root: std::path::PathBuf,
    /// Ids of sessions this daemon created and believes are live. The in-memory supervisor state is
    /// the Layer-1 source of truth (spec §11): persistence is best-effort, so `ListSessions` /
    /// `GetSessionState` must surface a running session even if its DB row failed to write (e.g. a
    /// transient DB error). Entries are added on `CreateSession`; a dead entry is simply skipped
    /// because `supervisor.meta(id)` returns `NoSuchSession` once the session is reaped.
    live_sessions: std::sync::Mutex<std::collections::HashSet<bpa_protocol::SessionId>>,
}

impl ServerDeps {
    /// Construct a [`ServerDeps`] with an empty live-session tracker.
    pub fn new(
        supervisor: Arc<Supervisor>,
        db: Arc<Mutex<Db>>,
        attach: Arc<AttachRegistry>,
        daemon_build: String,
        runtime_root: std::path::PathBuf,
    ) -> Self {
        ServerDeps {
            supervisor,
            db,
            attach,
            daemon_build,
            runtime_root,
            live_sessions: std::sync::Mutex::new(std::collections::HashSet::new()),
        }
    }
}

/// Registry of every connected client's outbound queue, so supervisor callbacks can fan a single
/// `Push` out to all of them (spec §7: `SessionCreated` / `StateChanged` / `ChildExited` reach
/// every client). Each client registers on connect and deregisters on disconnect. Sends use
/// `try_send`; a full/closed queue is silently skipped here (the owning client task independently
/// detects overflow on its own reply path and tears itself down) so one dead client never blocks
/// the fan-out to the others.
#[derive(Clone, Default)]
struct Broadcaster {
    inner: Arc<std::sync::Mutex<std::collections::HashMap<u64, mpsc::Sender<Frame>>>>,
}

impl Broadcaster {
    fn register(&self, id: u64, tx: mpsc::Sender<Frame>) {
        self.inner.lock().unwrap().insert(id, tx);
    }

    fn deregister(&self, id: u64) {
        self.inner.lock().unwrap().remove(&id);
    }

    /// Enqueue `push` into every registered client's outbound queue (best-effort, non-blocking).
    fn broadcast(&self, push: Push) {
        let map = self.inner.lock().unwrap();
        for tx in map.values() {
            let _ = tx.try_send(Frame::Push(push.clone()));
        }
    }
}

/// Accept loop (spec §7/§8.2/§13): peer-cred gate on accept, one task per client, handshake-gated
/// dispatch, supervisor→push fan-out, best-effort scrollback persistence. Runs until `shutdown`
/// flips to `true` or the `listener` errors, then tears down attach state and returns.
pub async fn serve(
    listener: UnixListener,
    deps: Arc<ServerDeps>,
    mut shutdown: watch::Receiver<bool>,
) -> std::io::Result<()> {
    let broadcaster = Broadcaster::default();
    // Monotonic per-connection id (used only to key the broadcaster registry).
    let mut next_conn_id: u64 = 1;

    // ---- Wire supervisor callbacks → Push fan-out (spec §7). Registered ONCE. ----
    install_push_callbacks(&deps.supervisor, &deps.attach, broadcaster.clone());

    // ---- Best-effort periodic scrollback persistence (spec §11). ----
    let flush_task = spawn_scrollback_flusher(deps.clone(), shutdown.clone());

    let result = loop {
        tokio::select! {
            changed = shutdown.changed() => {
                // Sender dropped or value flipped; either way, stop if we're told to shut down.
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
                // Peer-cred gate (spec §8.2): refuse a peer whose euid != the daemon's.
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

    // Shutdown: stop the flusher and drop all attach forwarders (sessions keep running — spec §7).
    flush_task.abort();
    deps.attach.detach_all();
    result
}

/// Register the supervisor's status/created/exited callbacks so each becomes a broadcast `Push`.
/// The `attach` registry is passed so a session that ends drops its attach entry (orphan cleanup)
/// before the `ChildExited` push is built — keeping the registry from growing unbounded across
/// create/kill churn. The drop is GRACEFUL (`remove_session` removes the map entry without
/// cancelling the forwarder): the reader thread closes the sink on its own exit, so the forwarder
/// delivers the session's trailing output and then self-terminates rather than being truncated.
///
/// The `on_exited` closure holds a `Weak<AttachRegistry>`, NOT a strong `Arc`: the registry itself
/// owns an `Arc<Supervisor>`, and the supervisor owns this callback, so a strong reference here
/// would form a `Supervisor ⇄ AttachRegistry` cycle that leaks the supervisor (and its live PTY
/// sessions) forever. That leak would keep an attach forwarder's std channel alive across a test's
/// runtime drop, hanging `Runtime` teardown. Upgrading the `Weak` per call is cheap and yields
/// `None` only once the daemon is already tearing down (nothing left to clean up).
fn install_push_callbacks(
    supervisor: &Arc<Supervisor>,
    attach: &Arc<AttachRegistry>,
    broadcaster: Broadcaster,
) {
    let b_created = broadcaster.clone();
    supervisor.on_created(move |meta: SessionMeta| {
        b_created.broadcast(Push::SessionCreated { meta });
    });

    let b_status = broadcaster.clone();
    supervisor.on_status(move |u: StatusUpdate| {
        b_status.broadcast(Push::StateChanged {
            session_id: u.session_id,
            lifecycle: u.lifecycle,
            waiting_for_input: u.waiting_for_input,
            cwd: u.cwd,
        });
    });

    let b_exited = broadcaster;
    let attach_exited = Arc::downgrade(attach);
    supervisor.on_exited(move |session_id, code, signal| {
        // A session that ends (kill or natural exit, both reaped by the wait thread) drops its
        // attach entry so the registry can't grow unbounded across create/kill churn. This is a
        // GRACEFUL detach: `remove_session` only removes the map entry — it does NOT cancel/abort
        // the forwarder. The reader thread drops the session's sink on its own exit, so the
        // forwarder drains every remaining byte and self-terminates on `Disconnected`, delivering
        // the session's trailing output to the attached client before the stream ends. The returned
        // `JoinHandle` is intentionally dropped (the task is detached and self-terminating). Do this
        // BEFORE building the `Push::ChildExited` below, which moves `session_id`. `Weak::upgrade`
        // is `None` only if the whole daemon is already gone, in which case there is nothing to
        // reap.
        if let Some(attach) = attach_exited.upgrade() {
            let _ = attach.remove_session(&session_id);
        }
        b_exited.broadcast(Push::ChildExited { session_id, code, signal });
    });
}

/// Spawn the best-effort scrollback persistence sweep (spec §11). Every
/// [`SCROLLBACK_FLUSH_INTERVAL`] it snapshots each live session's sanitized ring and replaces its
/// stored blob (seq 0). All failures are logged and swallowed — persistence is best-effort and must
/// never crash the server or stall a live session.
fn spawn_scrollback_flusher(
    deps: Arc<ServerDeps>,
    mut shutdown: watch::Receiver<bool>,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(SCROLLBACK_FLUSH_INTERVAL);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                changed = shutdown.changed() => {
                    if changed.is_err() || *shutdown.borrow() {
                        return;
                    }
                }
                _ = ticker.tick() => {
                    flush_scrollback_once(&deps).await;
                }
            }
        }
    })
}

/// One persistence sweep: for every session known to the DB, if it is still live in the supervisor,
/// snapshot its scrollback and replace the stored blob. Best-effort; logs and continues on error.
async fn flush_scrollback_once(deps: &Arc<ServerDeps>) {
    // Snapshot live session ids + their scrollback OUTSIDE the DB lock (supervisor calls are sync).
    let ids: Vec<String> = {
        let db = deps.db.lock().await;
        match db.list_sessions() {
            Ok(rows) => rows.into_iter().map(|m| m.id).collect(),
            Err(e) => {
                tracing::debug!(error = %e, "scrollback flush: list_sessions failed");
                return;
            }
        }
    };
    let ts = now_secs();
    for id in ids {
        if let Ok((_c, _r, bytes)) = deps.supervisor.snapshot_scrollback(&id) {
            let db = deps.db.lock().await;
            if let Err(e) = db.append_scrollback(&id, 0, &bytes, ts) {
                tracing::debug!(session = %id, error = %e, "scrollback flush: append failed");
            }
        }
    }
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
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
    // One decoder for the whole connection lifetime — carries any bytes batched after the Hello
    // frame across the reader/writer split so no pipelined request is ever lost.
    let mut reader = FrameReader::new();

    // ---- Handshake gate (spec §7): the FIRST frame MUST be Hello with matching magic+version. ----
    let first = match reader.next(&mut stream).await? {
        Some(f) => f,
        None => return Ok(()), // client closed before saying anything
    };
    let handshake_ok = matches!(
        &first,
        Frame::Request { id: 0, req: Request::Hello { magic, proto_version, .. } }
            if *magic == MAGIC && *proto_version == PROTO_VERSION
    );
    if !handshake_ok {
        // Wrong magic, wrong/out-of-range version, or a non-Hello first frame ⇒ refuse + close.
        let bytes = encode_frame(&Frame::Response {
            id: 0,
            res: Response::Incompatible { min: PROTO_VERSION, max: PROTO_VERSION },
        })
        .map_err(to_io)?;
        stream.write_all(&bytes).await?;
        stream.flush().await?;
        return Ok(()); // close the connection
    }
    let welcome = encode_frame(&Frame::Response {
        id: 0,
        res: Response::Welcome {
            proto_version: PROTO_VERSION,
            daemon_build: deps.daemon_build.clone(),
        },
    })
    .map_err(to_io)?;
    stream.write_all(&welcome).await?;
    stream.flush().await?;

    // ---- Split into an independent reader + writer, joined by a bounded outbound queue. ----
    let (mut rd, mut wr) = stream.into_split();
    let (out_tx, mut out_rx) = mpsc::channel::<Frame>(CLIENT_OUTQ_CAP);

    // Register this client for supervisor push fan-out.
    broadcaster.register(conn_id, out_tx.clone());

    // Writer task: drains the bounded queue and writes to the socket. Exits on EPIPE/write error
    // (⇒ the client is gone) or when the queue is closed (all senders dropped).
    let writer = tokio::spawn(async move {
        while let Some(frame) = out_rx.recv().await {
            let bytes = match encode_frame(&frame) {
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

    // The `PushSink` (`mpsc::Sender<Push>`) handed to `AttachRegistry`: a thin adapter that maps
    // each `Push` into `Frame::Push` on THIS client's bounded queue, so replay/output backpressure
    // is uniform with everything else.
    let push_sink = make_push_sink(out_tx.clone());

    // ---- Dispatch loop: correlate every Request{id} with exactly one Response{id}. ----
    let outcome: std::io::Result<()> = loop {
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break Ok(());
                }
            }
            frame = reader.next(&mut rd) => {
                match frame {
                    Ok(Some(Frame::Request { id, req })) => {
                        let res = dispatch(&deps, conn_id, &push_sink, req).await;
                        // Enqueue the reply without blocking; a full queue means the client stopped
                        // reading ⇒ drop + disconnect it (spec §13).
                        if out_tx.try_send(Frame::Response { id, res }).is_err() {
                            break Err(std::io::Error::new(
                                std::io::ErrorKind::WouldBlock,
                                "client outbound queue overflow",
                            ));
                        }
                    }
                    Ok(Some(Frame::Response { .. } | Frame::Push(_))) => {
                        // The core never sends these to the daemon; ignore defensively.
                        tracing::warn!(conn = conn_id, "ignoring unexpected inbound Response/Push");
                    }
                    Ok(None) => break Ok(()),  // client closed cleanly
                    Err(e) => break Err(e),    // framing/protocol error ⇒ disconnect
                }
            }
        }
    };

    // ---- Cleanup: deregister from fan-out, tear down ONLY THIS connection's attach forwarders
    // (per-connection teardown — spec §7: another client's attachment for an unrelated session must
    // keep streaming; a global detach_all here would corrupt it). Sessions keep running (§7
    // keep-alive). Then let the writer drain/exit. ----
    broadcaster.deregister(conn_id);
    deps.attach.detach_all_for_conn(conn_id);
    drop(push_sink);
    drop(out_tx);
    let _ = writer.await;
    outcome
}

/// A stateful frame reader for one connection. Owns the protocol [`FrameDecoder`] plus a queue of
/// already-decoded-but-not-yet-returned frames, so a single socket `read()` that delivers several
/// pipelined frames is fully consumed and drained one at a time. Persisting this across every read
/// (including the handshake read before the reader/writer split) is essential: a fresh decoder per
/// call would silently drop the extra frames buffered from a batched read and then block forever on
/// the next `read()` waiting for bytes that already arrived.
struct FrameReader {
    decoder: FrameDecoder,
    pending: std::collections::VecDeque<Frame>,
    buf: Box<[u8; 16 * 1024]>,
}

impl FrameReader {
    fn new() -> Self {
        FrameReader {
            decoder: FrameDecoder::new(),
            pending: std::collections::VecDeque::new(),
            buf: Box::new([0u8; 16 * 1024]),
        }
    }

    /// Return the next complete `Frame`, reading from `stream` only when nothing is buffered.
    /// `Ok(None)` on a clean EOF at a frame boundary; `InvalidData` on an oversized length prefix or
    /// a decode failure (spec §7/§13).
    async fn next<S>(&mut self, stream: &mut S) -> std::io::Result<Option<Frame>>
    where
        S: tokio::io::AsyncRead + Unpin,
    {
        use tokio::io::AsyncReadExt;
        loop {
            if let Some(f) = self.pending.pop_front() {
                return Ok(Some(f));
            }
            // Drain anything already buffered in the decoder before touching the socket.
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

/// Wrap a `Push` sink over the client's bounded `Frame` queue. Overflow / a gone writer stops the
/// forwarder (the client is being torn down anyway).
fn make_push_sink(out_tx: mpsc::Sender<Frame>) -> PushSink {
    let (tx, mut rx) = mpsc::channel::<Push>(CLIENT_OUTQ_CAP);
    tokio::spawn(async move {
        while let Some(push) = rx.recv().await {
            if out_tx.try_send(Frame::Push(push)).is_err() {
                break; // overflow or writer gone ⇒ stop forwarding
            }
        }
    });
    tx
}

/// Convert any `Display` error into an `InvalidData` `io::Error`.
fn to_io<E: std::fmt::Display>(e: E) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
}

/// Dispatch one `Request` to the right subsystem and produce the correlated `Response` (spec §7).
/// `conn_id` identifies the calling connection so attach/detach ownership is connection-scoped
/// (spec §7: single-attach per session, but teardown per connection).
async fn dispatch(
    deps: &Arc<ServerDeps>,
    conn_id: u64,
    push_sink: &PushSink,
    req: Request,
) -> Response {
    match req {
        Request::Hello { .. } => Response::Error {
            code: "UnexpectedHello".into(),
            message: "handshake already completed".into(),
        },

        Request::ListWorkspaces => {
            let db = deps.db.lock().await;
            match db.list_workspaces() {
                Ok(v) => Response::Workspaces(v),
                Err(e) => err("DbError", e),
            }
        }

        Request::CreateWorkspace { name, root_path } => {
            // §16: root_path must canonicalize to an absolute, existing directory.
            let canonical = match validate_dir(&root_path) {
                Ok(p) => p,
                Err(_) => {
                    return Response::Error {
                        code: "InvalidWorkspaceRoot".into(),
                        message: format!("workspace root is not an existing directory: {root_path}"),
                    }
                }
            };
            let w = Workspace {
                id: uuid::Uuid::new_v4().to_string(),
                name,
                root_path: canonical,
            };
            let db = deps.db.lock().await;
            match db.upsert_workspace(&w) {
                Ok(()) => {
                    // Broadcast to every client (spec §7). The awaiting caller still gets the
                    // Response below; other clients learn via the Push.
                    let _ = push_sink.try_send(Push::WorkspaceCreated { workspace: w.clone() });
                    Response::Workspace(w)
                }
                Err(e) => err("DbError", e),
            }
        }

        Request::ListSessions => {
            let persisted = {
                let db = deps.db.lock().await;
                match db.list_sessions() {
                    Ok(v) => v,
                    Err(e) => return err("DbError", e),
                }
            };
            // Overlay live supervisor state onto each persisted row, and union any live session the
            // daemon tracks that has no persisted row yet (best-effort persistence — spec §11: the
            // in-memory session is authoritative, so a running session is never hidden by a failed
            // DB write). De-dup by id.
            let mut by_id: std::collections::BTreeMap<bpa_protocol::SessionId, SessionMeta> =
                std::collections::BTreeMap::new();
            for mut m in persisted {
                if let Ok(live) = deps.supervisor.meta(&m.id) {
                    m = live;
                }
                by_id.insert(m.id.clone(), m);
            }
            let tracked: Vec<bpa_protocol::SessionId> =
                deps.live_sessions.lock().unwrap().iter().cloned().collect();
            for id in tracked {
                if by_id.contains_key(&id) {
                    continue;
                }
                if let Ok(live) = deps.supervisor.meta(&id) {
                    by_id.insert(id, live);
                }
            }
            Response::Sessions(by_id.into_values().collect())
        }

        Request::CreateSession { workspace_id, shell, cwd, env_overrides, cols, rows } => {
            let spec = match resolve_session_spec(
                &deps.runtime_root,
                workspace_id,
                shell,
                cwd,
                env_overrides,
                cols,
                rows,
            ) {
                Ok(spec) => spec,
                Err(resp) => return resp,
            };
            match deps.supervisor.create(spec) {
                Ok(id) => match deps.supervisor.meta(&id) {
                    Ok(meta) => {
                        // Track the live session so ListSessions/GetSessionState surface it even if
                        // the (best-effort) persist below fails (spec §11).
                        deps.live_sessions.lock().unwrap().insert(id.clone());
                        // Persist immediately (spec §11); best-effort — a DB failure does not fail
                        // the create (the live session is the source of truth).
                        {
                            let db = deps.db.lock().await;
                            if let Err(e) = db.upsert_session(&meta) {
                                tracing::warn!(session = %id, error = %e, "persist session failed");
                            }
                        }
                        // Broadcast to every client (via callbacks the supervisor also fires
                        // on_created; that path handles other clients). The reply is the Session.
                        Response::Session(meta)
                    }
                    Err(e) => err("CreateSessionFailed", e),
                },
                Err(e) => err("CreateSessionFailed", e),
            }
        }

        Request::AttachSession { session_id } => {
            match deps.attach.attach(conn_id, &session_id, push_sink.clone()).await {
                Ok(()) => Response::Ack,
                Err(AttachError::NoSuchSession) => Response::Error {
                    code: "NoSuchSession".into(),
                    message: format!("no session {session_id}"),
                },
                Err(AttachError::SinkClosed) => Response::Error {
                    code: "SinkClosed".into(),
                    message: "client sink closed".into(),
                },
            }
        }

        Request::DetachSession { session_id } => {
            deps.attach.detach(conn_id, &session_id);
            Response::Ack
        }

        Request::WriteStdin { session_id, bytes } => {
            match deps.supervisor.write_stdin(&session_id, &bytes) {
                Ok(()) => Response::Ack,
                Err(e) => err(code_for(&e), e),
            }
        }

        Request::Resize { session_id, cols, rows } => {
            match deps.supervisor.resize(&session_id, cols, rows) {
                Ok(()) => Response::Ack,
                Err(e) => err(code_for(&e), e),
            }
        }

        Request::KillSession { session_id } => match deps.supervisor.kill(&session_id) {
            Ok(()) => Response::Ack,
            Err(e) => err(code_for(&e), e),
        },

        Request::GetSessionState { session_id } => match deps.supervisor.meta(&session_id) {
            Ok(meta) => Response::Session(meta),
            Err(_) => {
                // Fall back to the persisted row (an exited-and-reaped session).
                let db = deps.db.lock().await;
                match db.list_sessions() {
                    Ok(v) => match v.into_iter().find(|m| m.id == session_id) {
                        Some(meta) => Response::Session(meta),
                        None => Response::Error {
                            code: "NoSuchSession".into(),
                            message: format!("no session {session_id}"),
                        },
                    },
                    Err(e) => err("DbError", e),
                }
            }
        },

        // S1: a GUI-initiated shutdown just Acks; the real drain is SIGTERM in Task 13.
        Request::DaemonShutdown { .. } => Response::Ack,
    }
}

/// Map a [`SupervisorError`] to a stable `Response::Error` code (spec §13).
fn code_for(e: &SupervisorError) -> &'static str {
    match e {
        SupervisorError::NoSuchSession(_) => "NoSuchSession",
        SupervisorError::Pty(_) => "PtyError",
        SupervisorError::Io(_) => "IoError",
        SupervisorError::Spawn(_) => "SpawnError",
    }
}

fn err(code: &str, e: impl std::fmt::Display) -> Response {
    Response::Error { code: code.into(), message: e.to_string() }
}

/// Validate a workspace root / session cwd via the shared `bpa-paths` validator (spec §16):
/// absolute + exists + is-a-directory + no symlink-escape of the lexical parent — byte-for-byte
/// the same rule the core enforces. Returns the canonical path string on success.
fn validate_dir(path: &str) -> Result<String, bpa_paths::PathError> {
    bpa_paths::validate_dir(std::path::Path::new(path)).map(|p| p.to_string_lossy().into_owned())
}

/// Resolve a protocol `CreateSession` into a fully-specified [`SessionSpec`] (spec §9.3/§16):
/// pick the shell, validate the cwd, assemble the §9.3 env allowlist + shell-integration env/args,
/// and derive a title. On a cwd violation returns `Err(Response::Error{code:"CwdMissing"})`.
fn resolve_session_spec(
    runtime_root: &std::path::Path,
    workspace_id: String,
    shell: Option<String>,
    cwd: Option<String>,
    env_overrides: Vec<(String, String)>,
    cols: u16,
    rows: u16,
) -> Result<SessionSpec, Response> {
    // ---- Shell selection: explicit → $SHELL → /bin/zsh (all must be absolute). ----
    let shell_path = shell
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("SHELL").ok())
        .unwrap_or_else(|| "/bin/zsh".to_string());
    if !std::path::Path::new(&shell_path).is_absolute() {
        return Err(Response::Error {
            code: "InvalidShell".into(),
            message: format!("shell path must be absolute: {shell_path}"),
        });
    }

    // ---- cwd validation (§16): canonical, absolute, existing directory. Default to $HOME. ----
    let cwd_input = cwd
        .filter(|c| !c.is_empty())
        .or_else(|| std::env::var("HOME").ok())
        .unwrap_or_else(|| "/".to_string());
    let cwd_canonical = match validate_dir(&cwd_input) {
        Ok(p) => std::path::PathBuf::from(p),
        Err(_) => {
            return Err(Response::Error {
                code: "CwdMissing".into(),
                message: format!("cwd is not an existing directory: {cwd_input}"),
            })
        }
    };

    // ---- Env allowlist (§9.3): a minimal safe base, then shell-integration env, then overrides. ----
    let mut env: Vec<(String, String)> = Vec::new();
    let mut push_env = |k: &str, default: Option<&str>| {
        if let Ok(v) = std::env::var(k) {
            env.push((k.to_string(), v));
        } else if let Some(d) = default {
            env.push((k.to_string(), d.to_string()));
        }
    };
    push_env("TERM", Some("xterm-256color"));
    push_env("PATH", Some("/usr/bin:/bin:/usr/sbin:/sbin"));
    push_env("HOME", None);
    push_env("USER", None);
    push_env("LOGNAME", None);
    push_env("LANG", None);
    push_env("SHELL", Some(&shell_path));
    push_env("SSH_AUTH_SOCK", None);

    // ---- Shell integration: OSC-133/OSC-7 injection via per-session assets, when recognized. ----
    let mut program = shell_path.clone();
    let mut args: Vec<String> = Vec::new();
    if let Some(kind) = classify_shell(&shell_path) {
        let session_runtime = runtime_root.join(format!("session-{}", uuid::Uuid::new_v4()));
        match write_session_assets(&session_runtime, kind) {
            Ok(spawn) => {
                // `write_session_assets` returns the canonical program path for the family
                // (/bin/zsh or /bin/bash). Keep the caller's shell path if they gave one, but use
                // the integration args + env.
                program = spawn.program;
                args = spawn.args;
                for (k, v) in spawn.env {
                    // Integration env overrides the base (e.g. ZDOTDIR, BPA_INJECTION).
                    env.retain(|(ek, _)| ek != &k);
                    env.push((k, v));
                }
            }
            Err(e) => {
                // Non-fatal: fall back to a bare shell with no integration (status won't advance,
                // but the session still works). Log and continue.
                tracing::warn!(error = %e, "shell integration asset write failed; bare shell");
            }
        }
    }

    // ---- Caller overrides win last (spec §7 env_overrides). ----
    for (k, v) in env_overrides {
        env.retain(|(ek, _)| ek != &k);
        env.push((k, v));
    }

    let title = std::path::Path::new(&program)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("shell")
        .to_string();

    Ok(SessionSpec {
        workspace_id,
        shell: program,
        args,
        cwd: cwd_canonical,
        env,
        cols,
        rows,
        title,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpa_protocol::{Frame, Push, Request, Response, MAGIC, PROTO_VERSION};
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{UnixListener, UnixStream};

    // ---- framing helpers (mirror the server codec on the client side) ----
    async fn send_frame(s: &mut UnixStream, f: &Frame) {
        let body = bincode::serialize(f).unwrap();
        s.write_all(&(body.len() as u32).to_le_bytes()).await.unwrap();
        s.write_all(&body).await.unwrap();
        s.flush().await.unwrap();
    }

    async fn recv_frame(s: &mut UnixStream) -> Frame {
        let mut lenb = [0u8; 4];
        s.read_exact(&mut lenb).await.unwrap();
        let len = u32::from_le_bytes(lenb) as usize;
        let mut body = vec![0u8; len];
        s.read_exact(&mut body).await.unwrap();
        bincode::deserialize(&body).unwrap()
    }

    /// Like [`recv_frame`] but bounded: panics on a 3 s timeout so a regressed handshake gate (that
    /// wrongly keeps the connection open instead of closing) fails fast instead of hanging the suite.
    async fn recv_frame_t(s: &mut UnixStream) -> Frame {
        match tokio::time::timeout(std::time::Duration::from_secs(3), recv_frame(s)).await {
            Ok(f) => f,
            Err(_) => panic!("timed out waiting for a frame (handshake/close regression?)"),
        }
    }

    fn test_deps() -> (Arc<ServerDeps>, tempfile::TempDir) {
        let supervisor = Arc::new(Supervisor::new());
        let db = Arc::new(Mutex::new(Db::open_in_memory().unwrap()));
        let attach = Arc::new(AttachRegistry::new(supervisor.clone()));
        let runtime = tempfile::tempdir().unwrap();
        let deps = Arc::new(ServerDeps::new(
            supervisor,
            db,
            attach,
            "test".into(),
            runtime.path().to_path_buf(),
        ));
        (deps, runtime)
    }

    async fn spawn_server() -> (
        std::path::PathBuf,
        tokio::sync::watch::Sender<bool>,
        tokio::task::JoinHandle<()>,
        tempfile::TempDir,
        tempfile::TempDir,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("d.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let (deps, runtime) = test_deps();
        let (tx, rx) = tokio::sync::watch::channel(false);
        let jh = tokio::spawn(async move {
            let _ = serve(listener, deps, rx).await;
        });
        (path, tx, jh, dir, runtime)
    }

    async fn hello(s: &mut UnixStream) -> Response {
        send_frame(
            s,
            &Frame::Request {
                id: 0,
                req: Request::Hello {
                    magic: MAGIC,
                    proto_version: PROTO_VERSION,
                    client_build: "t".into(),
                },
            },
        )
        .await;
        match recv_frame(s).await {
            Frame::Response { id, res } => {
                assert_eq!(id, 0);
                res
            }
            other => panic!("expected handshake Response, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn handshake_happy_path_returns_welcome() {
        let (path, _tx, _jh, _d, _r) = spawn_server().await;
        let mut c = UnixStream::connect(&path).await.unwrap();
        match hello(&mut c).await {
            Response::Welcome { proto_version, daemon_build } => {
                assert_eq!(proto_version, PROTO_VERSION);
                assert_eq!(daemon_build, "test");
            }
            other => panic!("expected Welcome, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn handshake_bad_magic_is_rejected_and_closes() {
        let (path, _tx, _jh, _d, _r) = spawn_server().await;
        let mut c = UnixStream::connect(&path).await.unwrap();
        send_frame(
            &mut c,
            &Frame::Request {
                id: 0,
                req: Request::Hello {
                    magic: 0xDEAD_BEEF,
                    proto_version: PROTO_VERSION,
                    client_build: "t".into(),
                },
            },
        )
        .await;
        match recv_frame_t(&mut c).await {
            Frame::Response { id: 0, res: Response::Incompatible { min, max } } => {
                assert_eq!((min, max), (PROTO_VERSION, PROTO_VERSION));
            }
            other => panic!("expected Incompatible, got {other:?}"),
        }
        // Connection must close: a follow-up read hits EOF (bounded so a regression fails fast).
        let mut buf = [0u8; 1];
        let n = tokio::time::timeout(std::time::Duration::from_secs(3), c.read(&mut buf))
            .await
            .expect("server must close after Incompatible (timed out)")
            .unwrap();
        assert_eq!(n, 0, "server must close after Incompatible");
    }

    #[tokio::test]
    async fn handshake_bad_version_is_rejected() {
        let (path, _tx, _jh, _d, _r) = spawn_server().await;
        let mut c = UnixStream::connect(&path).await.unwrap();
        send_frame(
            &mut c,
            &Frame::Request {
                id: 0,
                req: Request::Hello {
                    magic: MAGIC,
                    proto_version: PROTO_VERSION + 1,
                    client_build: "t".into(),
                },
            },
        )
        .await;
        match recv_frame_t(&mut c).await {
            Frame::Response { id: 0, res: Response::Incompatible { .. } } => {}
            other => panic!("expected Incompatible, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn non_hello_first_frame_is_rejected() {
        let (path, _tx, _jh, _d, _r) = spawn_server().await;
        let mut c = UnixStream::connect(&path).await.unwrap();
        send_frame(&mut c, &Frame::Request { id: 7, req: Request::ListWorkspaces }).await;
        match recv_frame_t(&mut c).await {
            Frame::Response { res: Response::Incompatible { .. }, .. } => {}
            other => panic!("first frame must be Hello; expected Incompatible, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn requests_are_answered_with_matching_ids_concurrently() {
        let (path, _tx, _jh, _d, _r) = spawn_server().await;
        let mut c = UnixStream::connect(&path).await.unwrap();
        assert!(matches!(hello(&mut c).await, Response::Welcome { .. }));

        // Fire three ListWorkspaces requests with distinct ids back-to-back.
        for id in [11u64, 22, 33] {
            send_frame(&mut c, &Frame::Request { id, req: Request::ListWorkspaces }).await;
        }
        let mut seen = std::collections::HashSet::new();
        for _ in 0..3 {
            match recv_frame(&mut c).await {
                Frame::Response { id, res: Response::Workspaces(_) } => {
                    seen.insert(id);
                }
                other => panic!("expected Workspaces response, got {other:?}"),
            }
        }
        assert_eq!(seen, [11, 22, 33].into_iter().collect());
    }

    #[tokio::test]
    async fn create_workspace_persists_and_pushes() {
        let (path, _tx, _jh, _d, _r) = spawn_server().await;
        let mut c = UnixStream::connect(&path).await.unwrap();
        assert!(matches!(hello(&mut c).await, Response::Welcome { .. }));

        // /tmp is a real, existing directory — passes §16 validation.
        send_frame(
            &mut c,
            &Frame::Request {
                id: 5,
                req: Request::CreateWorkspace { name: "w".into(), root_path: "/tmp".into() },
            },
        )
        .await;

        let mut got_resp: Option<Workspace> = None;
        let mut got_push = false;
        for _ in 0..2 {
            match recv_frame(&mut c).await {
                Frame::Response { id: 5, res: Response::Workspace(w) } => got_resp = Some(w),
                Frame::Push(Push::WorkspaceCreated { .. }) => got_push = true,
                other => panic!("unexpected frame {other:?}"),
            }
        }
        assert!(got_resp.is_some() && got_push);

        // The workspace is persisted: a subsequent ListWorkspaces reflects it.
        send_frame(&mut c, &Frame::Request { id: 6, req: Request::ListWorkspaces }).await;
        loop {
            match recv_frame(&mut c).await {
                Frame::Response { id: 6, res: Response::Workspaces(v) } => {
                    assert_eq!(v.len(), 1);
                    assert_eq!(v[0].name, "w");
                    break;
                }
                Frame::Push(_) => continue,
                other => panic!("unexpected frame {other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn create_workspace_rejects_missing_dir() {
        let (path, _tx, _jh, _d, _r) = spawn_server().await;
        let mut c = UnixStream::connect(&path).await.unwrap();
        assert!(matches!(hello(&mut c).await, Response::Welcome { .. }));
        send_frame(
            &mut c,
            &Frame::Request {
                id: 9,
                req: Request::CreateWorkspace {
                    name: "bad".into(),
                    root_path: "/nonexistent/path/xyzzy".into(),
                },
            },
        )
        .await;
        match recv_frame(&mut c).await {
            Frame::Response { id: 9, res: Response::Error { code, .. } } => {
                assert_eq!(code, "InvalidWorkspaceRoot");
            }
            other => panic!("expected InvalidWorkspaceRoot error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn create_session_persists_and_get_reflects_it() {
        let (path, _tx, _jh, _d, _r) = spawn_server().await;
        let mut c = UnixStream::connect(&path).await.unwrap();
        assert!(matches!(hello(&mut c).await, Response::Welcome { .. }));

        // Use /bin/sh (unrecognized by classify_shell) so no integration assets are needed and the
        // resolution is deterministic; cwd=/tmp exists.
        send_frame(
            &mut c,
            &Frame::Request {
                id: 1,
                req: Request::CreateSession {
                    workspace_id: "ws".into(),
                    shell: Some("/bin/sh".into()),
                    cwd: Some("/tmp".into()),
                    env_overrides: vec![],
                    cols: 80,
                    rows: 24,
                },
            },
        )
        .await;

        // Drain until the correlated Session response (Pushes may interleave).
        let session_id = loop {
            match recv_frame(&mut c).await {
                Frame::Response { id: 1, res: Response::Session(meta) } => {
                    assert_eq!(meta.cols, 80);
                    assert_eq!(meta.rows, 24);
                    break meta.id;
                }
                Frame::Response { id: 1, res: Response::Error { code, message } } => {
                    panic!("create failed: {code}: {message}");
                }
                Frame::Push(_) => continue,
                other => panic!("unexpected frame {other:?}"),
            }
        };

        // GetSessionState returns the live session.
        send_frame(
            &mut c,
            &Frame::Request { id: 2, req: Request::GetSessionState { session_id: session_id.clone() } },
        )
        .await;
        loop {
            match recv_frame(&mut c).await {
                Frame::Response { id: 2, res: Response::Session(meta) } => {
                    assert_eq!(meta.id, session_id);
                    break;
                }
                Frame::Push(_) => continue,
                other => panic!("expected Session, got {other:?}"),
            }
        }

        // ListSessions reflects the persisted session too.
        send_frame(&mut c, &Frame::Request { id: 3, req: Request::ListSessions }).await;
        loop {
            match recv_frame(&mut c).await {
                Frame::Response { id: 3, res: Response::Sessions(v) } => {
                    assert!(v.iter().any(|m| m.id == session_id));
                    break;
                }
                Frame::Push(_) => continue,
                other => panic!("expected Sessions, got {other:?}"),
            }
        }

        // Clean up the live child.
        send_frame(
            &mut c,
            &Frame::Request { id: 4, req: Request::KillSession { session_id } },
        )
        .await;
    }

    #[tokio::test]
    async fn create_session_rejects_missing_cwd() {
        let (path, _tx, _jh, _d, _r) = spawn_server().await;
        let mut c = UnixStream::connect(&path).await.unwrap();
        assert!(matches!(hello(&mut c).await, Response::Welcome { .. }));
        send_frame(
            &mut c,
            &Frame::Request {
                id: 1,
                req: Request::CreateSession {
                    workspace_id: "ws".into(),
                    shell: Some("/bin/sh".into()),
                    cwd: Some("/nonexistent/xyzzy".into()),
                    env_overrides: vec![],
                    cols: 80,
                    rows: 24,
                },
            },
        )
        .await;
        match recv_frame(&mut c).await {
            Frame::Response { id: 1, res: Response::Error { code, .. } } => {
                assert_eq!(code, "CwdMissing");
            }
            other => panic!("expected CwdMissing error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn create_session_rejects_relative_cwd() {
        let (path, _tx, _jh, _d, _r) = spawn_server().await;
        let mut c = UnixStream::connect(&path).await.unwrap();
        assert!(matches!(hello(&mut c).await, Response::Welcome { .. }));
        // A relative path that does not exist relative to the daemon's cwd ⇒ rejected.
        send_frame(
            &mut c,
            &Frame::Request {
                id: 1,
                req: Request::CreateSession {
                    workspace_id: "ws".into(),
                    shell: Some("/bin/sh".into()),
                    cwd: Some("relative/does/not/exist".into()),
                    env_overrides: vec![],
                    cols: 80,
                    rows: 24,
                },
            },
        )
        .await;
        match recv_frame(&mut c).await {
            Frame::Response { id: 1, res: Response::Error { code, .. } } => {
                assert_eq!(code, "CwdMissing");
            }
            other => panic!("expected CwdMissing error for relative cwd, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn attach_first_push_is_replay_then_output() {
        let (path, _tx, _jh, _d, _r) = spawn_server().await;
        let mut c = UnixStream::connect(&path).await.unwrap();
        assert!(matches!(hello(&mut c).await, Response::Welcome { .. }));

        // Create a shell that waits for a go-signal, so we can attach before any output.
        // We drive the child via WriteStdin over the same connection.
        send_frame(
            &mut c,
            &Frame::Request {
                id: 1,
                req: Request::CreateSession {
                    workspace_id: "ws".into(),
                    shell: Some("/bin/sh".into()),
                    cwd: Some("/tmp".into()),
                    env_overrides: vec![],
                    cols: 80,
                    rows: 24,
                },
            },
        )
        .await;
        let session_id = loop {
            match recv_frame(&mut c).await {
                Frame::Response { id: 1, res: Response::Session(meta) } => break meta.id,
                Frame::Response { id: 1, res: Response::Error { code, message } } => {
                    panic!("create failed: {code}: {message}")
                }
                Frame::Push(_) => continue,
                other => panic!("unexpected {other:?}"),
            }
        };

        // Prime the child to block on stdin, then attach.
        send_frame(
            &mut c,
            &Frame::Request {
                id: 2,
                req: Request::WriteStdin {
                    session_id: session_id.clone(),
                    bytes: b"read _go; printf 'HELLO_ATTACH\\n'\n".to_vec(),
                },
            },
        )
        .await;
        // Ack for WriteStdin (drain until it).
        loop {
            match recv_frame(&mut c).await {
                Frame::Response { id: 2, res: Response::Ack } => break,
                Frame::Push(_) => continue,
                other => panic!("expected Ack for write, got {other:?}"),
            }
        }

        send_frame(
            &mut c,
            &Frame::Request { id: 3, req: Request::AttachSession { session_id: session_id.clone() } },
        )
        .await;

        // The FIRST push for this session must be a Replay; then Output flows after we release it.
        // Collect frames: expect Ack(id 3) + Replay push (order may interleave), then release + Output.
        let mut got_ack = false;
        let mut got_replay = false;
        for _ in 0..4 {
            match recv_frame(&mut c).await {
                Frame::Response { id: 3, res: Response::Ack } => got_ack = true,
                Frame::Push(Push::Replay { session_id: sid, .. }) => {
                    assert_eq!(sid, session_id);
                    got_replay = true;
                }
                Frame::Push(_) => continue,
                other => panic!("unexpected before release {other:?}"),
            }
            if got_ack && got_replay {
                break;
            }
        }
        assert!(got_ack && got_replay, "attach must Ack and deliver Replay first");

        // Release the child; Output with the printed text must follow.
        send_frame(
            &mut c,
            &Frame::Request {
                id: 4,
                req: Request::WriteStdin { session_id: session_id.clone(), bytes: b"go\n".to_vec() },
            },
        )
        .await;

        let mut collected = Vec::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            match tokio::time::timeout(std::time::Duration::from_millis(500), recv_frame(&mut c)).await {
                Ok(Frame::Push(Push::Output { session_id: sid, bytes })) if sid == session_id => {
                    collected.extend_from_slice(&bytes);
                    if collected.windows(12).any(|w| w == b"HELLO_ATTACH") {
                        break;
                    }
                }
                Ok(_) => continue,
                Err(_) => continue,
            }
        }
        assert!(
            collected.windows(12).any(|w| w == b"HELLO_ATTACH"),
            "expected live Output containing HELLO_ATTACH, got: {collected:?}"
        );

        send_frame(&mut c, &Frame::Request { id: 5, req: Request::KillSession { session_id } }).await;
    }

    #[tokio::test]
    async fn slow_client_is_disconnected_without_stalling_a_second_client() {
        let (path, _tx, _jh, _d, _r) = spawn_server().await;

        // Client A connects, handshakes, then STOPS reading — we flood its outq with replies.
        let mut a = UnixStream::connect(&path).await.unwrap();
        assert!(matches!(hello(&mut a).await, Response::Welcome { .. }));

        for id in 0..(CLIENT_OUTQ_CAP as u64 + 512) {
            let f = Frame::Request { id, req: Request::ListWorkspaces };
            let body = bincode::serialize(&f).unwrap();
            if a.write_all(&(body.len() as u32).to_le_bytes()).await.is_err() {
                break;
            }
            if a.write_all(&body).await.is_err() {
                break;
            }
            let _ = a.flush().await;
        }

        // Client B connects fresh and MUST be served normally.
        let mut b = UnixStream::connect(&path).await.unwrap();
        assert!(matches!(hello(&mut b).await, Response::Welcome { .. }));
        send_frame(&mut b, &Frame::Request { id: 1, req: Request::ListWorkspaces }).await;
        match tokio::time::timeout(std::time::Duration::from_secs(2), recv_frame(&mut b)).await {
            Ok(Frame::Response { id: 1, res: Response::Workspaces(_) }) => {}
            Ok(other) => panic!("B expected Workspaces, got {other:?}"),
            Err(_) => panic!("B was stalled by A's backpressure — bounded-outq isolation broken"),
        }
    }

    #[tokio::test]
    async fn oversized_frame_is_rejected() {
        let (path, _tx, _jh, _d, _r) = spawn_server().await;
        let mut c = UnixStream::connect(&path).await.unwrap();
        assert!(matches!(hello(&mut c).await, Response::Welcome { .. }));
        // Announce a length beyond MAX_FRAME_LEN; the server must disconnect without allocating.
        let bogus_len = bpa_protocol::MAX_FRAME_LEN + 1;
        c.write_all(&bogus_len.to_le_bytes()).await.unwrap();
        c.flush().await.unwrap();
        let mut buf = [0u8; 1];
        let n = tokio::time::timeout(std::time::Duration::from_secs(3), c.read(&mut buf))
            .await
            .expect("server must close on oversized frame length (timed out)")
            .unwrap();
        assert_eq!(n, 0, "server must close on oversized frame length");
    }

    #[tokio::test]
    async fn write_resize_kill_unknown_session_errors() {
        let (path, _tx, _jh, _d, _r) = spawn_server().await;
        let mut c = UnixStream::connect(&path).await.unwrap();
        assert!(matches!(hello(&mut c).await, Response::Welcome { .. }));

        for (id, req) in [
            (1u64, Request::WriteStdin { session_id: "ghost".into(), bytes: vec![1] }),
            (2, Request::Resize { session_id: "ghost".into(), cols: 10, rows: 10 }),
            (3, Request::KillSession { session_id: "ghost".into() }),
        ] {
            send_frame(&mut c, &Frame::Request { id, req }).await;
            match recv_frame(&mut c).await {
                Frame::Response { id: rid, res: Response::Error { code, .. } } => {
                    assert_eq!(rid, id);
                    assert_eq!(code, "NoSuchSession");
                }
                other => panic!("expected NoSuchSession error, got {other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn detach_and_daemon_shutdown_ack() {
        let (path, _tx, _jh, _d, _r) = spawn_server().await;
        let mut c = UnixStream::connect(&path).await.unwrap();
        assert!(matches!(hello(&mut c).await, Response::Welcome { .. }));

        send_frame(
            &mut c,
            &Frame::Request { id: 1, req: Request::DetachSession { session_id: "ghost".into() } },
        )
        .await;
        assert!(matches!(
            recv_frame(&mut c).await,
            Frame::Response { id: 1, res: Response::Ack }
        ));

        send_frame(&mut c, &Frame::Request { id: 2, req: Request::DaemonShutdown { drain: false } }).await;
        assert!(matches!(
            recv_frame(&mut c).await,
            Frame::Response { id: 2, res: Response::Ack }
        ));
    }

    // ---- Peer-cred: honest same-process test. A cross-uid peer cannot be forged in the sandbox,
    // so we assert the accepted (same-euid) path works end to end; the rejection logic itself is
    // unit-tested in `singleton.rs` (peer_cred_rejects_foreign_uid_simulated). ----
    #[tokio::test]
    async fn peer_cred_same_uid_is_accepted() {
        let (path, _tx, _jh, _d, _r) = spawn_server().await;
        let mut c = UnixStream::connect(&path).await.unwrap();
        // If peer-cred wrongly rejected our own euid, the handshake would never complete.
        assert!(matches!(hello(&mut c).await, Response::Welcome { .. }));
    }

    /// Build a real escaping-symlink layout under a fresh tempdir and return `(tempdir, link_path)`.
    /// Mirrors `bpa_paths::validate_dir`'s `symlink_escaping_parent_is_rejected`:
    ///   base/outside/         (real dir, OUTSIDE `named`)
    ///   base/named/link -> ../outside
    /// The `link` canonicalizes to `base/outside`, whose parent (`base`) != the canonical parent of
    /// the input (`base/named`) → symlink-escape. The tempdir MUST be kept alive by the caller for
    /// the duration of the assertion (dropping it deletes the layout out from under the daemon).
    fn escaping_symlink_layout() -> (tempfile::TempDir, String) {
        let base = tempfile::tempdir().unwrap();
        let outside = base.path().join("outside");
        let named = base.path().join("named");
        std::fs::create_dir(&outside).unwrap();
        std::fs::create_dir(&named).unwrap();
        let link = named.join("link");
        std::os::unix::fs::symlink(&outside, &link).unwrap();
        let link_str = link.to_string_lossy().into_owned();
        (base, link_str)
    }

    // ---- §16 daemon path validation: a symlink that escapes its lexical parent must be rejected
    // as a workspace root, exactly as the core (and `bpa-paths`) reject it. The naive
    // `canonicalize()`-only validator ACCEPTS this (it silently resolves the escaping link), so on
    // the pre-fix daemon this returns `Response::Workspace` and the test FAILS. ----
    #[tokio::test]
    async fn create_workspace_rejects_symlink_escaping_root() {
        let (path, _tx, _jh, _d, _r) = spawn_server().await;
        let (_layout, link) = escaping_symlink_layout();
        let mut c = UnixStream::connect(&path).await.unwrap();
        assert!(matches!(hello(&mut c).await, Response::Welcome { .. }));
        send_frame(
            &mut c,
            &Frame::Request {
                id: 9,
                req: Request::CreateWorkspace { name: "esc".into(), root_path: link },
            },
        )
        .await;
        match recv_frame_t(&mut c).await {
            Frame::Response { id: 9, res: Response::Error { code, .. } } => {
                assert_eq!(code, "InvalidWorkspaceRoot");
            }
            other => panic!("expected InvalidWorkspaceRoot for symlink-escaping root, got {other:?}"),
        }
    }

    // ---- Same escaping-symlink layout as a session cwd: must be rejected with CwdMissing (the
    // wire code the cwd call site maps every validation failure to). Pre-fix: the session is
    // created and this FAILS. ----
    #[tokio::test]
    async fn create_session_rejects_symlink_escaping_cwd() {
        let (path, _tx, _jh, _d, _r) = spawn_server().await;
        let (_layout, link) = escaping_symlink_layout();
        let mut c = UnixStream::connect(&path).await.unwrap();
        assert!(matches!(hello(&mut c).await, Response::Welcome { .. }));
        send_frame(
            &mut c,
            &Frame::Request {
                id: 1,
                req: Request::CreateSession {
                    workspace_id: "ws".into(),
                    shell: Some("/bin/sh".into()),
                    cwd: Some(link),
                    env_overrides: vec![],
                    cols: 80,
                    rows: 24,
                },
            },
        )
        .await;
        match recv_frame_t(&mut c).await {
            Frame::Response { id: 1, res: Response::Error { code, .. } } => {
                assert_eq!(code, "CwdMissing");
            }
            other => panic!("expected CwdMissing for symlink-escaping cwd, got {other:?}"),
        }
    }

    /// Minimal deterministic `SessionSpec` for a `/bin/sh -c <script>` shell (mirrors the `spec()`
    /// helper in attach.rs tests): a real cwd, a minimal TERM/PATH/HOME env, 80x24.
    fn sh_spec(script: &str) -> SessionSpec {
        let path = std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into());
        SessionSpec {
            workspace_id: "ws-test".into(),
            shell: "/bin/sh".into(),
            args: vec!["-c".into(), script.into()],
            cwd: std::path::PathBuf::from("/tmp"),
            env: vec![
                ("TERM".into(), "xterm-256color".into()),
                ("PATH".into(), path),
                ("HOME".into(), std::env::var("HOME").unwrap_or_else(|_| "/tmp".into())),
            ],
            cols: 80,
            rows: 24,
            title: "sh".into(),
        }
    }

    // ---- Orphan-cleanup (folded-in deferred item): a session that is killed drops its attach
    // registry entry, so the registry cannot grow unbounded across create/kill churn. Deterministic:
    // `kill()` joins the wait thread, which runs `on_exited` → `remove_session` before it returns. ----
    #[tokio::test]
    async fn killed_session_attach_entry_is_reaped() {
        let (deps, _rt) = test_deps();
        install_push_callbacks(&deps.supervisor, &deps.attach, Broadcaster::default());

        // Create a live session directly via the supervisor (a long sleep so it stays alive).
        let id = deps.supervisor.create(sh_spec("sleep 5")).expect("create");

        // Attach it (a bounded mpsc sink stands in for a client's push queue).
        let (sink, _client) = mpsc::channel::<Push>(16);
        deps.attach.attach(1, &id, sink).await.expect("attach");
        assert_eq!(deps.attach.attachment_count(), 1, "attach registered one entry");

        // Kill joins the wait thread, so on_exited → remove_session has run by the time kill returns.
        deps.supervisor.kill(&id).expect("kill");
        assert_eq!(
            deps.attach.attachment_count(),
            0,
            "killed session's attach entry must be reaped (no orphan)"
        );
    }

    // ---- Blocker A end-to-end (the verdict's required two-client regression): client A's
    // disconnect must NOT tear down client B's live stream for an UNRELATED session. Under the
    // unfixed code, A's disconnect called `detach_all()` and killed B's forwarder too, so B never
    // receives B_AFTER. ----
    #[tokio::test]
    async fn client_disconnect_does_not_teardown_a_second_clients_attached_session() {
        let (path, _tx, _jh, _d, _r) = spawn_server().await;

        // ---- Client A: connect, create SA, attach (drain its Ack + Replay). ----
        let mut a = UnixStream::connect(&path).await.unwrap();
        assert!(matches!(hello(&mut a).await, Response::Welcome { .. }));
        send_frame(
            &mut a,
            &Frame::Request {
                id: 1,
                req: Request::CreateSession {
                    workspace_id: "ws".into(),
                    shell: Some("/bin/sh".into()),
                    cwd: Some("/tmp".into()),
                    env_overrides: vec![],
                    cols: 80,
                    rows: 24,
                },
            },
        )
        .await;
        let sa_id = loop {
            match recv_frame_t(&mut a).await {
                Frame::Response { id: 1, res: Response::Session(m) } => break m.id,
                Frame::Response { id: 1, res: Response::Error { code, message } } => {
                    panic!("A create failed: {code}: {message}")
                }
                Frame::Push(_) => continue,
                other => panic!("A unexpected {other:?}"),
            }
        };
        send_frame(
            &mut a,
            &Frame::Request { id: 2, req: Request::AttachSession { session_id: sa_id.clone() } },
        )
        .await;
        // Drain A's Ack + first Replay.
        let (mut a_ack, mut a_replay) = (false, false);
        for _ in 0..4 {
            match recv_frame_t(&mut a).await {
                Frame::Response { id: 2, res: Response::Ack } => a_ack = true,
                Frame::Push(Push::Replay { .. }) => a_replay = true,
                Frame::Push(_) => continue,
                other => panic!("A unexpected before attach settle {other:?}"),
            }
            if a_ack && a_replay {
                break;
            }
        }
        assert!(a_ack && a_replay, "A must Ack + Replay its attach");

        // ---- Client B: connect, create SB, prime it, attach, prove it is streaming (B_BEFORE). ----
        let mut b = UnixStream::connect(&path).await.unwrap();
        assert!(matches!(hello(&mut b).await, Response::Welcome { .. }));
        send_frame(
            &mut b,
            &Frame::Request {
                id: 1,
                req: Request::CreateSession {
                    workspace_id: "ws".into(),
                    shell: Some("/bin/sh".into()),
                    cwd: Some("/tmp".into()),
                    env_overrides: vec![],
                    cols: 80,
                    rows: 24,
                },
            },
        )
        .await;
        let sb_id = loop {
            match recv_frame_t(&mut b).await {
                Frame::Response { id: 1, res: Response::Session(m) } => break m.id,
                Frame::Response { id: 1, res: Response::Error { code, message } } => {
                    panic!("B create failed: {code}: {message}")
                }
                Frame::Push(_) => continue,
                other => panic!("B unexpected {other:?}"),
            }
        };
        // Prime SB to block on a go-signal, then print B_BEFORE (drain its Ack).
        send_frame(
            &mut b,
            &Frame::Request {
                id: 2,
                req: Request::WriteStdin {
                    session_id: sb_id.clone(),
                    bytes: b"read _go; printf 'B_BEFORE\\n'\n".to_vec(),
                },
            },
        )
        .await;
        loop {
            match recv_frame_t(&mut b).await {
                Frame::Response { id: 2, res: Response::Ack } => break,
                Frame::Push(_) => continue,
                other => panic!("B expected Ack for prime write, got {other:?}"),
            }
        }
        // Attach SB (drain Ack + Replay).
        send_frame(
            &mut b,
            &Frame::Request { id: 3, req: Request::AttachSession { session_id: sb_id.clone() } },
        )
        .await;
        let (mut b_ack, mut b_replay) = (false, false);
        for _ in 0..4 {
            match recv_frame_t(&mut b).await {
                Frame::Response { id: 3, res: Response::Ack } => b_ack = true,
                Frame::Push(Push::Replay { session_id, .. }) if session_id == sb_id => b_replay = true,
                Frame::Push(_) => continue,
                other => panic!("B unexpected before attach settle {other:?}"),
            }
            if b_ack && b_replay {
                break;
            }
        }
        assert!(b_ack && b_replay, "B must Ack + Replay its attach");
        // Release SB; collect B's Output until it contains B_BEFORE (proves B is streaming).
        send_frame(
            &mut b,
            &Frame::Request {
                id: 4,
                req: Request::WriteStdin { session_id: sb_id.clone(), bytes: b"go\n".to_vec() },
            },
        )
        .await;
        let mut b_out = Vec::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            match tokio::time::timeout(std::time::Duration::from_millis(500), recv_frame(&mut b)).await {
                Ok(Frame::Push(Push::Output { session_id, bytes })) if session_id == sb_id => {
                    b_out.extend_from_slice(&bytes);
                    if b_out.windows(8).any(|w| w == b"B_BEFORE") {
                        break;
                    }
                }
                Ok(_) => continue,
                Err(_) => continue,
            }
        }
        assert!(
            b_out.windows(8).any(|w| w == b"B_BEFORE"),
            "B must be streaming Output before A disconnects, got: {b_out:?}"
        );

        // ---- A disconnects: closes its socket → handle_client cleanup → detach_all_for_conn(A).
        // Under the unfixed code this called detach_all() and tore down SB's forwarder too. ----
        drop(a);

        // ---- Prove B STILL receives live Output for SB after A's disconnect (B_AFTER). ----
        send_frame(
            &mut b,
            &Frame::Request {
                id: 5,
                req: Request::WriteStdin {
                    session_id: sb_id.clone(),
                    bytes: b"read _go2; printf 'B_AFTER\\n'\n".to_vec(),
                },
            },
        )
        .await;
        // Drain the Ack for id 5 (Output may interleave).
        loop {
            match recv_frame_t(&mut b).await {
                Frame::Response { id: 5, res: Response::Ack } => break,
                Frame::Push(_) => continue,
                other => panic!("B expected Ack for post-disconnect write, got {other:?}"),
            }
        }
        send_frame(
            &mut b,
            &Frame::Request {
                id: 6,
                req: Request::WriteStdin { session_id: sb_id.clone(), bytes: b"go\n".to_vec() },
            },
        )
        .await;
        let mut b_out2 = Vec::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            match tokio::time::timeout(std::time::Duration::from_millis(500), recv_frame(&mut b)).await {
                Ok(Frame::Push(Push::Output { session_id, bytes })) if session_id == sb_id => {
                    b_out2.extend_from_slice(&bytes);
                    if b_out2.windows(7).any(|w| w == b"B_AFTER") {
                        break;
                    }
                }
                Ok(_) => continue,
                Err(_) => continue,
            }
        }
        assert!(
            b_out2.windows(7).any(|w| w == b"B_AFTER"),
            "B's stream must survive A's disconnect (blocker A): expected B_AFTER, got: {b_out2:?}"
        );

        // Clean up: kill SB over B (SA went away with A).
        send_frame(&mut b, &Frame::Request { id: 7, req: Request::KillSession { session_id: sb_id } }).await;
    }
}
