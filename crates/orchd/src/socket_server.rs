//! `bpa-orchd` socket server (spec §5, mirrors `bpa_sessiond::socket_server` minus PTY/attach/
//! scrollback concerns): a tokio `UnixListener` accept loop with one task per connected client,
//! the codec-agnostic preamble handshake + version negotiation gate (shared with sessiond via
//! `bpa_daemon_core::handshake`), request/response correlation, a bounded per-client outbound
//! queue (overflow ⇒ drop+disconnect), peer-cred refusal of foreign euids, and a
//! `Broadcaster<OrchdFrame>` client registry (T10+ wires domain-change pushes through it; this
//! skeleton only registers/deregisters — nothing broadcasts yet).
//!
//! ## Dispatch (spec §5, §6)
//!
//! `OrchdRequest::Ping` → `Pong`. `OrchdRequest::OrchdShutdown { drain }` → (if `drain`: a
//! best-effort WAL checkpoint) reply `Ack` and flip the shared shutdown watch — the SAME trigger
//! `main.rs`'s SIGTERM handler flips, so a GUI-initiated shutdown and an operator signal converge
//! on one graceful-exit path (mirrors sessiond's `Request::DaemonShutdown` dispatch arm). EVERY
//! other request is a stub `Error{Validation, "not implemented"}` — T10 replaces this one arm
//! with the real domain dispatch table; the boot/socket/handshake/shutdown plumbing around it
//! does not change.

use std::sync::Arc;

use tokio::io::AsyncWriteExt;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{mpsc, watch, Mutex};

use bpa_daemon_core::singleton::check_peer_cred;
use bpa_orchd_proto::{
    encode_orchd_frame, OrchdErrorCode, OrchdFrame, OrchdFrameDecoder, OrchdRequest, OrchdResponse,
    ORCHD_DAEMON_MAX_VERSION, ORCHD_DAEMON_MIN_VERSION,
};

use crate::persistence::Db;

/// Per-client bounded outbound queue depth (frames). Overflow (a client that stopped reading) ⇒
/// drop + disconnect that client rather than buffer unboundedly (mirrors sessiond's
/// `CLIENT_OUTQ_CAP`).
pub const CLIENT_OUTQ_CAP: usize = 1024;

/// Bound on how long connection cleanup waits for the writer task to notice its queue is closed
/// and exit on its own, before forcibly aborting it (mirrors sessiond's `WRITER_JOIN_TIMEOUT`).
const WRITER_JOIN_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(200);

/// Registry of every connected client's outbound queue (spec §5: T10+ fans domain-change pushes
/// out through this; this skeleton only registers/deregisters on connect/disconnect).
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

    // Register this client for the (currently unused) push fan-out — T10+ broadcasts through it.
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
                        let res = dispatch(&deps, req).await;
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

/// Dispatch one `OrchdRequest` to the right subsystem and produce the correlated
/// `OrchdResponse` (spec §5, §6). `Ping`/`OrchdShutdown` are real; every other verb is a stub —
/// T10 replaces this ONE match arm with the real domain dispatch table.
async fn dispatch(deps: &Arc<ServerDeps>, req: OrchdRequest) -> OrchdResponse {
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

        _ => OrchdResponse::Error {
            code: OrchdErrorCode::Validation,
            message: "not implemented".into(),
        },
    }
}
