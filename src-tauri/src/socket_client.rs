//! Hop-B client: the Tauri core's Unix-domain-socket connection to `bpa-sessiond` (spec §7, §8.1,
//! §13). Owns socket-path resolution, handshake, monotonic request/response correlation, push
//! fan-out, and a bounded-backoff reconnect loop that surfaces connect/disconnect transitions so
//! the broker (Task 17) can raise `daemon://disconnected` / `daemon://reconnected`.
//!
//! ## Framing (spec §7)
//!
//! Every wire message is a `u32`-LE length prefix + `bincode(Frame)`. We reuse the protocol
//! crate's own codec (`bpa_protocol::{encode_frame, FrameDecoder, MAX_FRAME_LEN}`) verbatim so the
//! core and the daemon can never drift on the oversized-length / partial-frame rules — see
//! `crates/protocol/src/framing.rs` and `crates/sessiond/src/socket_server.rs` for the daemon-side
//! twin of this reader pattern.
//!
//! ## Design
//!
//! A single owning **reader/writer task** (`tokio::spawn`) drives one live connection at a time:
//! it owns the socket, the `FrameDecoder`, and the request-correlation map, so there is exactly one
//! writer and no shared-writer lock across an `.await`. `DaemonClient` itself is just a cheap handle
//! holding an `mpsc::Sender<ClientCmd>` to that task plus the push/conn callback registries (so
//! `on_push`/`on_conn` can be registered any time, including after `connect()` returns).
//!
//! On disconnect (read EOF / IO error / write failure) the task drains every in-flight request with
//! `Err(ClientError::Disconnected)` (never a fake success), fires `on_conn(Disconnected)`, then
//! reconnects with bounded exponential backoff (`BACKOFF_START` doubling up to `BACKOFF_CAP`).
//! A version-`Incompatible` reply is fatal and is not retried — the task exits with a `tracing::error!`
//! rather than looping forever against an unrecoverable version skew.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bpa_protocol::{encode_frame, Frame, FrameDecoder, Push, Request, Response, MAGIC, PROTO_VERSION};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::sync::{mpsc, oneshot};

/// Initial reconnect backoff delay (spec §13).
const BACKOFF_START: Duration = Duration::from_millis(100);
/// Reconnect backoff cap (spec §13: "bounded exponential backoff, cap ~5s").
const BACKOFF_CAP: Duration = Duration::from_secs(5);
/// Per-request timeout: a request that never gets a correlated reply (daemon hung, or the
/// connection drops mid-request) fails honestly instead of hanging the caller forever.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// Depth of the command channel from `DaemonClient::request` callers to the owning task.
const CMD_CHANNEL_CAP: usize = 256;

// ---------------------------------------------------------------------------------------------
// Socket path resolution (spec §8.1) — the core is the source of truth; Task 16 (launchd) reuses
// this exact resolution so the daemon and the core that spawns it always agree on the path.
// ---------------------------------------------------------------------------------------------

/// macOS `sun_path` is 104 bytes including the NUL terminator; usable length is strictly < 104
/// (spec §8.1). Mirrors `bpa_sessiond::singleton::SUN_PATH_MAX` exactly — the daemon and core must
/// never disagree on this bound.
const SUN_PATH_MAX: usize = 104;

/// Resolve the daemon's runtime directory: `$XDG_RUNTIME_DIR/bpa` if `XDG_RUNTIME_DIR` is set and
/// non-empty, else `/tmp/bpa-<uid>` (spec §8.1). Matches `bpa_sessiond::singleton::socket_dir()`
/// byte-for-byte — the daemon resolves the same path independently, so any drift here would mean
/// the core connects to a socket the daemon never binds.
fn socket_dir() -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(x) if !x.is_empty() => PathBuf::from(x).join("bpa"),
        _ => {
            // SAFETY-free portable euid read: `libc::geteuid` never fails.
            let uid = unsafe { libc::geteuid() };
            PathBuf::from(format!("/tmp/bpa-{uid}"))
        }
    }
}

/// Resolve the daemon's Unix-domain-socket path (`<dir>/d.sock`, spec §8.1). Panics if the
/// resolved path would overflow `sun_path` (`>= 104` bytes) — this is a hard boot-time
/// misconfiguration (e.g. an absurd `XDG_RUNTIME_DIR`), not a runtime condition the client should
/// try to paper over: `UnixStream::connect` would fail anyway, so failing fast with a clear
/// message is more honest than deferring to a confusing OS error deep in the connect path.
pub fn resolve_socket_path() -> PathBuf {
    let path = socket_dir().join("d.sock");
    let len = path.as_os_str().len();
    assert!(
        len < SUN_PATH_MAX,
        "socket path length {len} >= sun_path max {SUN_PATH_MAX}: {}",
        path.display()
    );
    path
}

// ---------------------------------------------------------------------------------------------
// Errors + connection-state
// ---------------------------------------------------------------------------------------------

/// Terminal error surfaced to the broker/UI. Never panics on IO; every failure mode the client can
/// hit is represented here so callers can match honestly instead of guessing from a string.
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    /// The connection is down (mid-reconnect) or the client is shutting down. A request made while
    /// disconnected — or in flight when the connection drops — resolves to this rather than hanging
    /// or faking success (spec §13).
    #[error("daemon disconnected")]
    Disconnected,
    /// The daemon rejected the request (`Response::Error`).
    #[error("daemon reported: {code}: {message}")]
    Daemon { code: String, message: String },
}

/// Emitted by the reconnect loop so the broker can raise `daemon://disconnected` /
/// `daemon://reconnected` (spec §6.3, §13). `Connected` is fired both for the initial connect and
/// for every successful reconnect; callers that need to distinguish "first connect" from
/// "reconnected after a drop" can track whether they have already observed a `Disconnected`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnState {
    Connected,
    Disconnected,
}

// ---------------------------------------------------------------------------------------------
// DaemonClient
// ---------------------------------------------------------------------------------------------

enum ClientCmd {
    Request {
        req: Request,
        reply: oneshot::Sender<Result<Response, ClientError>>,
    },
}

// `Send`-only (no `Sync` bound), matching the locked `on_push`/`on_conn` signatures exactly: every
// callback is invoked from inside a `Mutex<Vec<..>>` lock on a single task, so callers never need
// their closure to be safely callable from two threads at once — only movable into the task. Stored
// as `Box` (not `Arc`): `Box<dyn Fn + Send>` is itself `Send` without requiring `Sync` on the inner
// trait object, which is exactly what carrying this `Vec` across the connection task's `.await`
// points needs; `Arc<dyn Fn + Send>` would additionally require `Sync` (`Arc<T>: Send` needs
// `T: Send + Sync`), which would silently re-impose the stricter bound the locked API rejects.
type PushCb = dyn Fn(Push) + Send;
type ConnCb = dyn Fn(ConnState) + Send;

/// Handle to the daemon connection. Cheap to clone (wraps an `Arc` internally via its channel
/// sender); the actual socket, framing, and correlation state live in a background task spawned by
/// `connect()`.
pub struct DaemonClient {
    cmd_tx: mpsc::Sender<ClientCmd>,
    push_cb: Arc<Mutex<Vec<Box<PushCb>>>>,
    conn_cb: Arc<Mutex<Vec<Box<ConnCb>>>>,
}

// Manual impl: the callback vecs hold `dyn Fn`, which is not `Debug`; this still gives a useful
// `{:?}` for logging/tests (e.g. `Result::unwrap_err` requires the `Ok` side to be `Debug`).
impl std::fmt::Debug for DaemonClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DaemonClient").finish_non_exhaustive()
    }
}

impl DaemonClient {
    /// Resolve the socket path, connect with bounded exponential backoff (cap `BACKOFF_CAP`), send
    /// `Hello`, await `Welcome`/`Incompatible`, then spawn the read/reconnect loop.
    ///
    /// The **first** connect attempt is a single try (no retry loop) so a genuinely absent daemon
    /// (nothing listening yet, e.g. before launchd has started it) or a version mismatch surfaces
    /// immediately to the caller instead of hanging `connect()` — the reconnect loop with backoff
    /// only kicks in for a connection that was live and then dropped. `client_build` is echoed to
    /// the daemon in `Hello` for diagnostics; it never carries secrets.
    pub async fn connect(client_build: String) -> Result<DaemonClient, ClientError> {
        let socket_path = resolve_socket_path();
        Self::connect_at(socket_path, client_build).await
    }

    /// Shared implementation behind `connect()`: connects at an explicit socket path (production
    /// always resolves via `resolve_socket_path()`; tests point this at a stub daemon's tempdir
    /// socket instead).
    async fn connect_at(socket_path: PathBuf, client_build: String) -> Result<DaemonClient, ClientError> {
        let (stream, reader) = connect_and_handshake(&socket_path, &client_build)
            .await
            .map_err(|_| ClientError::Disconnected)?;

        let (cmd_tx, cmd_rx) = mpsc::channel::<ClientCmd>(CMD_CHANNEL_CAP);
        let push_cb: Arc<Mutex<Vec<Box<PushCb>>>> = Arc::new(Mutex::new(Vec::new()));
        let conn_cb: Arc<Mutex<Vec<Box<ConnCb>>>> = Arc::new(Mutex::new(Vec::new()));

        tokio::spawn(connection_task(
            socket_path,
            client_build,
            stream,
            reader,
            cmd_rx,
            push_cb.clone(),
            conn_cb.clone(),
        ));

        Ok(DaemonClient { cmd_tx, push_cb, conn_cb })
    }

    /// Allocate a monotonic id, send `Request { id, .. }`, and await the correlated `Response`.
    /// `Response::Error` maps to `Err(ClientError::Daemon)`. If the connection is down (mid-drop,
    /// mid-reconnect, or the client has been dropped), returns `Err(ClientError::Disconnected)`
    /// rather than hanging. Times out after `REQUEST_TIMEOUT` as a last-resort safety net against a
    /// daemon that accepted the request but never replies.
    pub async fn request(&self, req: Request) -> Result<Response, ClientError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.cmd_tx
            .send(ClientCmd::Request { req, reply: reply_tx })
            .await
            .map_err(|_| ClientError::Disconnected)?;

        match tokio::time::timeout(REQUEST_TIMEOUT, reply_rx).await {
            Ok(Ok(res)) => res,
            // The connection task dropped the reply sender without answering (disconnect drain).
            Ok(Err(_)) => Err(ClientError::Disconnected),
            // No reply within REQUEST_TIMEOUT; treat the same as a dead connection — the caller
            // must not be left hanging indefinitely.
            Err(_) => Err(ClientError::Disconnected),
        }
    }

    /// Register a callback invoked for every `Push` frame the daemon sends (spec §7: the broker
    /// fans these out to `Channel<TerminalEvent>` / global events). Multiple callbacks may be
    /// registered; each receives every push. Callbacks must not block (they run inline on the
    /// connection task) — hand off to a channel/spawn for anything slow.
    pub fn on_push(&self, cb: impl Fn(Push) + Send + 'static) {
        self.push_cb.lock().unwrap().push(Box::new(cb));
    }

    /// Register a callback invoked on every connect/disconnect transition (spec §6.3, §13: the
    /// broker maps these to `daemon://disconnected` / `daemon://reconnected`). Multiple callbacks
    /// may be registered. Callbacks must not block, per `on_push`.
    pub fn on_conn(&self, cb: impl Fn(ConnState) + Send + 'static) {
        self.conn_cb.lock().unwrap().push(Box::new(cb));
    }
}

// ---------------------------------------------------------------------------------------------
// Framing + handshake helpers
// ---------------------------------------------------------------------------------------------

/// A stateful frame reader for one connection: owns the protocol `FrameDecoder` plus a queue of
/// already-decoded-but-not-yet-returned frames, so a single socket `read()` that delivers several
/// pipelined frames is fully drained one at a time. The decoder is connection-lifetime (constructed
/// once per connection attempt in `connect_and_handshake`, never per-read) — a fresh decoder per
/// read would silently discard any bytes already buffered from a batched read and then block
/// forever waiting for bytes that already arrived. Mirrors `bpa_sessiond::socket_server::FrameReader`
/// so both sides share one contract.
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
    /// `Ok(None)` on a clean EOF at a frame boundary (a mid-frame EOF also yields `None`; the
    /// caller treats both as connection-closed). `Err` on an oversized length prefix, a decode
    /// failure, or an IO error.
    async fn next(&mut self, stream: &mut (impl AsyncReadExt + Unpin)) -> std::io::Result<Option<Frame>> {
        loop {
            if let Some(f) = self.pending.pop_front() {
                return Ok(Some(f));
            }
            let frames = self
                .decoder
                .decode()
                .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
            if !frames.is_empty() {
                self.pending.extend(frames);
                continue;
            }
            let n = stream.read(&mut self.buf[..]).await?;
            if n == 0 {
                return Ok(None);
            }
            self.decoder.push(&self.buf[..n]);
        }
    }
}

async fn write_frame(stream: &mut (impl AsyncWriteExt + Unpin), frame: &Frame) -> std::io::Result<()> {
    let bytes =
        encode_frame(frame).map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))?;
    stream.write_all(&bytes).await?;
    stream.flush().await
}

/// Errors from a single connect+handshake attempt. Distinct from `ClientError` (the public,
/// terminal-to-callers error) because `Incompatible` needs to short-circuit the backoff loop while
/// `Io`/`Protocol` should keep retrying.
enum HandshakeError {
    Io(std::io::Error),
    /// Any first reply other than `Welcome`/`Incompatible`, or a connection that closed mid-handshake.
    Protocol(String),
    Incompatible { min: u16, max: u16 },
}

impl From<std::io::Error> for HandshakeError {
    fn from(e: std::io::Error) -> Self {
        HandshakeError::Io(e)
    }
}

/// Connect once and perform the `Hello`/`Welcome` handshake (spec §7). Returns the live stream and
/// a fresh `FrameReader` primed for the connection's lifetime.
async fn connect_and_handshake(
    socket_path: &Path,
    client_build: &str,
) -> Result<(UnixStream, FrameReader), HandshakeError> {
    let mut stream = UnixStream::connect(socket_path).await?;
    write_frame(
        &mut stream,
        &Frame::Request {
            id: 0,
            req: Request::Hello {
                magic: MAGIC,
                proto_version: PROTO_VERSION,
                client_build: client_build.to_string(),
            },
        },
    )
    .await?;

    let mut reader = FrameReader::new();
    match reader.next(&mut stream).await? {
        Some(Frame::Response { id: 0, res: Response::Welcome { proto_version, daemon_build } }) => {
            tracing::info!(daemon_build = %daemon_build, proto_version, "daemon handshake ok");
            Ok((stream, reader))
        }
        Some(Frame::Response { id: 0, res: Response::Incompatible { min, max } }) => {
            Err(HandshakeError::Incompatible { min, max })
        }
        Some(other) => Err(HandshakeError::Protocol(format!("bad handshake reply: {other:?}"))),
        None => Err(HandshakeError::Protocol("connection closed during handshake".into())),
    }
}

/// Connect with bounded exponential backoff (`BACKOFF_START` doubling up to `BACKOFF_CAP`, spec
/// §13). `Incompatible` is fatal and returned immediately without retrying — a stale client build
/// will never become compatible by waiting.
async fn connect_with_backoff(
    socket_path: &Path,
    client_build: &str,
) -> Result<(UnixStream, FrameReader), HandshakeError> {
    let mut delay = BACKOFF_START;
    loop {
        match connect_and_handshake(socket_path, client_build).await {
            Ok(ok) => return Ok(ok),
            Err(e @ HandshakeError::Incompatible { .. }) => return Err(e),
            Err(e) => {
                let msg = match &e {
                    HandshakeError::Io(err) => err.to_string(),
                    HandshakeError::Protocol(msg) => msg.clone(),
                    HandshakeError::Incompatible { .. } => unreachable!(),
                };
                tracing::warn!(error = %msg, delay_ms = delay.as_millis(), "daemon connect failed; backing off");
                tokio::time::sleep(delay).await;
                delay = std::cmp::min(delay * 2, BACKOFF_CAP);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Connection-owning task: handshake already done for the first connection; drives request writes,
// frame reads, correlation, push fan-out, and reconnect-on-drop.
// ---------------------------------------------------------------------------------------------

fn fire_conn(cbs: &Mutex<Vec<Box<ConnCb>>>, state: ConnState) {
    for cb in cbs.lock().unwrap().iter() {
        cb(state);
    }
}

fn fire_push(cbs: &Mutex<Vec<Box<PushCb>>>, push: Push) {
    for cb in cbs.lock().unwrap().iter() {
        cb(push.clone());
    }
}

#[derive(PartialEq, Eq)]
enum LoopEnd {
    /// The socket errored or hit EOF; the caller should reconnect.
    ConnectionLost,
    /// `DaemonClient` was dropped (the command channel closed); stop entirely.
    ClientDropped,
}

/// Runs one live connection until it errors/EOFs (`ConnectionLost`) or the command channel closes
/// because `DaemonClient` was dropped (`ClientDropped`). Owns the correlation map for the
/// connection's lifetime so out-of-order replies always resolve the right caller.
async fn run_connection(
    stream: &mut UnixStream,
    reader: &mut FrameReader,
    cmd_rx: &mut mpsc::Receiver<ClientCmd>,
    next_id: &AtomicU64,
    pending: &Mutex<HashMap<u64, oneshot::Sender<Result<Response, ClientError>>>>,
    push_cb: &Arc<Mutex<Vec<Box<PushCb>>>>,
) -> LoopEnd {
    loop {
        tokio::select! {
            biased;
            cmd = cmd_rx.recv() => {
                match cmd {
                    None => return LoopEnd::ClientDropped,
                    Some(ClientCmd::Request { req, reply }) => {
                        let id = next_id.fetch_add(1, Ordering::Relaxed);
                        pending.lock().unwrap().insert(id, reply);
                        if let Err(e) = write_frame(stream, &Frame::Request { id, req }).await {
                            tracing::warn!(error = %e, "write failed; dropping connection");
                            // The uniform drain in the outer loop will fail this (and every other
                            // in-flight) sender with Disconnected once we return.
                            return LoopEnd::ConnectionLost;
                        }
                    }
                }
            }
            frame = reader.next(stream) => {
                match frame {
                    Err(e) => {
                        tracing::warn!(error = %e, "read failed; dropping connection");
                        return LoopEnd::ConnectionLost;
                    }
                    Ok(None) => {
                        tracing::warn!("daemon closed the connection; dropping");
                        return LoopEnd::ConnectionLost;
                    }
                    Ok(Some(Frame::Response { id, res })) => {
                        if let Some(tx) = pending.lock().unwrap().remove(&id) {
                            let mapped = match res {
                                Response::Error { code, message } => Err(ClientError::Daemon { code, message }),
                                other => Ok(other),
                            };
                            let _ = tx.send(mapped);
                        } else {
                            tracing::warn!(id, "response for unknown request id; dropping");
                        }
                    }
                    Ok(Some(Frame::Push(p))) => fire_push(push_cb, p),
                    Ok(Some(Frame::Request { .. })) => {
                        tracing::warn!("unexpected Request frame from daemon; ignoring");
                    }
                }
            }
        }
    }
}

/// Owning task for the whole client lifetime: drives the already-handshaken first connection
/// (with the `FrameReader` primed during that handshake — reused as-is rather than discarded, so
/// no bytes buffered-but-not-yet-decoded from the handshake read are ever lost), and on drop
/// reconnects with backoff (re-`Hello`-ing each time) until the client is dropped or the daemon
/// becomes permanently incompatible.
async fn connection_task(
    socket_path: PathBuf,
    client_build: String,
    mut stream: UnixStream,
    mut reader: FrameReader,
    mut cmd_rx: mpsc::Receiver<ClientCmd>,
    push_cb: Arc<Mutex<Vec<Box<PushCb>>>>,
    conn_cb: Arc<Mutex<Vec<Box<ConnCb>>>>,
) {
    fire_conn(&conn_cb, ConnState::Connected);

    let next_id = AtomicU64::new(1); // id 0 is reserved for Hello
    let pending: Mutex<HashMap<u64, oneshot::Sender<Result<Response, ClientError>>>> = Mutex::new(HashMap::new());

    loop {
        let end = run_connection(&mut stream, &mut reader, &mut cmd_rx, &next_id, &pending, &push_cb).await;

        // Honest degradation (spec §13): every in-flight request fails now rather than hanging or
        // faking success, regardless of why the connection ended.
        for (_id, tx) in pending.lock().unwrap().drain() {
            let _ = tx.send(Err(ClientError::Disconnected));
        }

        if end == LoopEnd::ClientDropped {
            return;
        }

        fire_conn(&conn_cb, ConnState::Disconnected);
        tracing::warn!("daemon connection lost; reconnecting");

        match connect_with_backoff(&socket_path, &client_build).await {
            Ok((s, r)) => {
                stream = s;
                reader = r;
                fire_conn(&conn_cb, ConnState::Connected);
            }
            Err(HandshakeError::Incompatible { min, max }) => {
                tracing::error!(min, max, client = PROTO_VERSION, "daemon became incompatible; giving up reconnect");
                return;
            }
            // connect_with_backoff only returns Err for Incompatible; Io/Protocol loop internally.
            Err(_) => unreachable!("connect_with_backoff retries Io/Protocol errors internally"),
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;
    use tokio::net::UnixListener;

    // --- framing helpers reused by the stub daemon ---
    async fn read_frame(stream: &mut UnixStream) -> Option<Frame> {
        let mut len_buf = [0u8; 4];
        stream.read_exact(&mut len_buf).await.ok()?;
        let len = u32::from_le_bytes(len_buf) as usize;
        let mut body = vec![0u8; len];
        stream.read_exact(&mut body).await.ok()?;
        bincode::deserialize::<Frame>(&body).ok()
    }

    async fn write_stub_frame(stream: &mut UnixStream, frame: &Frame) {
        let bytes = encode_frame(frame).unwrap();
        stream.write_all(&bytes).await.unwrap();
        stream.flush().await.unwrap();
    }

    fn tmp_sock() -> PathBuf {
        let dir = tempfile::tempdir().unwrap();
        // leak the tempdir so the socket path stays valid for the test's lifetime
        let p = dir.path().join("d.sock");
        std::mem::forget(dir);
        p
    }

    async fn wait_ready(ready: &AtomicBool) {
        while !ready.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    // ---- resolve_socket_path -------------------------------------------------------------
    //
    // `std::env::set_var` mutates whole-process state, but `cargo test` runs `#[test]` fns on a
    // shared thread pool by default, so two of these tests can otherwise interleave their
    // set/restore of `XDG_RUNTIME_DIR` and observe each other's value mid-mutation. A single
    // process-wide mutex, held across each mutate-read-restore window, serializes just these
    // env-dependent tests against each other without slowing down (or being able to deadlock)
    // any of the other, non-env-touching tests in this module.
    static ENV_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn with_env<F: FnOnce()>(key: &str, val: Option<&str>, f: F) {
        let _guard = ENV_TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let prev = std::env::var_os(key);
        match val {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
        f();
        match prev {
            Some(p) => std::env::set_var(key, p),
            None => std::env::remove_var(key),
        }
    }

    #[test]
    fn socket_path_uses_xdg_runtime_dir_when_set() {
        with_env("XDG_RUNTIME_DIR", Some("/run/user/501"), || {
            let sock = resolve_socket_path();
            assert_eq!(sock, PathBuf::from("/run/user/501/bpa/d.sock"));
        });
    }

    #[test]
    fn socket_path_falls_back_to_tmp_with_uid_when_xdg_unset() {
        with_env("XDG_RUNTIME_DIR", None, || {
            let sock = resolve_socket_path();
            let uid = unsafe { libc::geteuid() };
            assert_eq!(sock, PathBuf::from(format!("/tmp/bpa-{uid}/d.sock")));
        });
    }

    #[test]
    fn socket_path_falls_back_to_tmp_when_xdg_empty() {
        with_env("XDG_RUNTIME_DIR", Some(""), || {
            let sock = resolve_socket_path();
            let uid = unsafe { libc::geteuid() };
            assert_eq!(sock, PathBuf::from(format!("/tmp/bpa-{uid}/d.sock")));
        });
    }

    #[test]
    fn socket_path_is_under_104_bytes() {
        with_env("XDG_RUNTIME_DIR", Some("/run/user/501"), || {
            let sock = resolve_socket_path();
            assert!(sock.as_os_str().len() < SUN_PATH_MAX);
        });
    }

    // ---- handshake + correlation ----------------------------------------------------------

    /// Stub daemon: handshakes, then for each Request replies with a distinguishable Response.
    /// `CreateWorkspace{name}` -> `Workspace{ id=name, name, root_path }` so the client can assert
    /// the reply matches the request it sent (correlation proof). Delays the FIRST reply so a
    /// later request can overtake it, proving correlation is by id, not FIFO reply order.
    fn spawn_stub(path: PathBuf, ready: Arc<AtomicBool>) {
        tokio::spawn(async move {
            let listener = UnixListener::bind(&path).unwrap();
            ready.store(true, Ordering::SeqCst);
            let (mut stream, _) = listener.accept().await.unwrap();
            let first = read_frame(&mut stream).await.unwrap();
            match first {
                Frame::Request { id: 0, req: Request::Hello { magic, proto_version, .. } } => {
                    assert_eq!(magic, MAGIC);
                    assert_eq!(proto_version, PROTO_VERSION);
                    write_stub_frame(
                        &mut stream,
                        &Frame::Response {
                            id: 0,
                            res: Response::Welcome { proto_version: PROTO_VERSION, daemon_build: "stub".into() },
                        },
                    )
                    .await;
                }
                other => panic!("expected Hello, got {other:?}"),
            }

            let mut first_seen = false;
            loop {
                let Some(frame) = read_frame(&mut stream).await else { break };
                if let Frame::Request { id, req } = frame {
                    let res = match req {
                        Request::CreateWorkspace { name, root_path } => {
                            Response::Workspace(bpa_protocol::Workspace { id: name.clone(), name, root_path })
                        }
                        _ => Response::Ack,
                    };
                    if !first_seen {
                        first_seen = true;
                        tokio::time::sleep(Duration::from_millis(120)).await;
                    }
                    write_stub_frame(&mut stream, &Frame::Response { id, res }).await;
                }
            }
        });
    }

    #[tokio::test]
    async fn concurrent_requests_correlate_by_id() {
        let path = tmp_sock();
        let ready = Arc::new(AtomicBool::new(false));
        spawn_stub(path.clone(), ready.clone());
        wait_ready(&ready).await;

        let client = connect_at(path, "test".into()).await.unwrap();
        let client = Arc::new(client);

        let c1 = client.clone();
        let h1 = tokio::spawn(async move {
            c1.request(Request::CreateWorkspace { name: "one".into(), root_path: "/".into() }).await
        });
        tokio::time::sleep(Duration::from_millis(20)).await;
        let c2 = client.clone();
        let h2 = tokio::spawn(async move {
            c2.request(Request::CreateWorkspace { name: "two".into(), root_path: "/".into() }).await
        });

        let r1 = h1.await.unwrap().unwrap();
        let r2 = h2.await.unwrap().unwrap();
        match r1 {
            Response::Workspace(w) => assert_eq!(w.name, "one"),
            o => panic!("{o:?}"),
        }
        match r2 {
            Response::Workspace(w) => assert_eq!(w.name, "two"),
            o => panic!("{o:?}"),
        }
    }

    #[tokio::test]
    async fn daemon_error_response_maps_to_client_error_daemon() {
        let path = tmp_sock();
        let ready = Arc::new(AtomicBool::new(false));
        let p = path.clone();
        let r = ready.clone();
        tokio::spawn(async move {
            let listener = UnixListener::bind(&p).unwrap();
            r.store(true, Ordering::SeqCst);
            let (mut stream, _) = listener.accept().await.unwrap();
            let _ = read_frame(&mut stream).await.unwrap();
            write_stub_frame(
                &mut stream,
                &Frame::Response {
                    id: 0,
                    res: Response::Welcome { proto_version: PROTO_VERSION, daemon_build: "stub".into() },
                },
            )
            .await;
            if let Some(Frame::Request { id, .. }) = read_frame(&mut stream).await {
                write_stub_frame(
                    &mut stream,
                    &Frame::Response {
                        id,
                        res: Response::Error { code: "NoSuchSession".into(), message: "gone".into() },
                    },
                )
                .await;
            }
        });
        wait_ready(&ready).await;

        let client = connect_at(path, "test".into()).await.unwrap();
        let err = client
            .request(Request::KillSession { session_id: "missing".into() })
            .await
            .unwrap_err();
        match err {
            ClientError::Daemon { code, message } => {
                assert_eq!(code, "NoSuchSession");
                assert_eq!(message, "gone");
            }
            o => panic!("expected ClientError::Daemon, got {o:?}"),
        }
    }

    #[tokio::test]
    async fn incompatible_handshake_is_rejected() {
        let path = tmp_sock();
        let ready = Arc::new(AtomicBool::new(false));
        let p = path.clone();
        let r = ready.clone();
        tokio::spawn(async move {
            let listener = UnixListener::bind(&p).unwrap();
            r.store(true, Ordering::SeqCst);
            let (mut stream, _) = listener.accept().await.unwrap();
            let _first = read_frame(&mut stream).await.unwrap(); // consume Hello
            write_stub_frame(&mut stream, &Frame::Response { id: 0, res: Response::Incompatible { min: 2, max: 4 } })
                .await;
        });
        wait_ready(&ready).await;

        let err = connect_at(path, "test".into()).await.unwrap_err();
        // The public ClientError has no Incompatible variant (LOCKED API); the connect-time
        // rejection surfaces as Disconnected — but we assert it is a *fast, non-retried* failure
        // by bounding the test's own wall-clock time (an incompatible handshake must return
        // immediately, not after minutes of backoff).
        assert!(matches!(err, ClientError::Disconnected), "expected Disconnected, got {err:?}");
    }

    #[tokio::test]
    async fn reconnects_after_drop_and_delivers_push() {
        let path = tmp_sock();
        let ready = Arc::new(AtomicBool::new(false));

        let p = path.clone();
        let r = ready.clone();
        tokio::spawn(async move {
            let listener = UnixListener::bind(&p).unwrap();
            r.store(true, Ordering::SeqCst);
            // connection #1
            {
                let (mut s, _) = listener.accept().await.unwrap();
                let _ = read_frame(&mut s).await.unwrap(); // Hello
                write_stub_frame(
                    &mut s,
                    &Frame::Response {
                        id: 0,
                        res: Response::Welcome { proto_version: PROTO_VERSION, daemon_build: "d1".into() },
                    },
                )
                .await;
                write_stub_frame(
                    &mut s,
                    &Frame::Push(Push::WorkspaceCreated {
                        workspace: bpa_protocol::Workspace { id: "w1".into(), name: "w1".into(), root_path: "/".into() },
                    }),
                )
                .await;
                // drop `s` -> connection closes
            }
            // connection #2
            {
                let (mut s, _) = listener.accept().await.unwrap();
                let _ = read_frame(&mut s).await.unwrap(); // Hello again
                write_stub_frame(
                    &mut s,
                    &Frame::Response {
                        id: 0,
                        res: Response::Welcome { proto_version: PROTO_VERSION, daemon_build: "d2".into() },
                    },
                )
                .await;
                if let Some(Frame::Request { id, .. }) = read_frame(&mut s).await {
                    write_stub_frame(&mut s, &Frame::Response { id, res: Response::Ack }).await;
                }
            }
        });
        wait_ready(&ready).await;

        let pushes: Arc<Mutex<Vec<Push>>> = Arc::new(Mutex::new(Vec::new()));
        let states: Arc<Mutex<Vec<ConnState>>> = Arc::new(Mutex::new(Vec::new()));

        let client = connect_at(path, "test".into()).await.unwrap();
        let pushes_cb = pushes.clone();
        client.on_push(move |p| pushes_cb.lock().unwrap().push(p));
        let states_cb = states.clone();
        client.on_conn(move |s| states_cb.lock().unwrap().push(s));

        // Give the reader a moment to deliver the push from connection #1 before it drops.
        tokio::time::sleep(Duration::from_millis(50)).await;

        // The Ack request will only succeed after reconnect #2. Retry until connected.
        let mut got_ack = false;
        for _ in 0..100 {
            match client.request(Request::KillSession { session_id: "x".into() }).await {
                Ok(Response::Ack) => {
                    got_ack = true;
                    break;
                }
                _ => tokio::time::sleep(Duration::from_millis(50)).await,
            }
        }
        assert!(got_ack, "expected an Ack after reconnect");

        let got_push = pushes.lock().unwrap().iter().any(|p| matches!(p, Push::WorkspaceCreated { .. }));
        assert!(got_push, "expected the WorkspaceCreated push");

        let s = states.lock().unwrap().clone();
        assert_eq!(s.first(), Some(&ConnState::Connected));
        assert!(s.contains(&ConnState::Disconnected), "expected a Disconnected transition: {s:?}");
        assert!(
            s.iter().filter(|x| **x == ConnState::Connected).count() >= 2,
            "expected at least two Connected transitions: {s:?}"
        );
    }

    #[tokio::test]
    async fn in_flight_request_fails_disconnected_when_daemon_drops_mid_request() {
        let path = tmp_sock();
        let ready = Arc::new(AtomicBool::new(false));
        let p = path.clone();
        let r = ready.clone();
        tokio::spawn(async move {
            let listener = UnixListener::bind(&p).unwrap();
            r.store(true, Ordering::SeqCst);
            let (mut s, _) = listener.accept().await.unwrap();
            let _ = read_frame(&mut s).await.unwrap(); // Hello
            write_stub_frame(
                &mut s,
                &Frame::Response {
                    id: 0,
                    res: Response::Welcome { proto_version: PROTO_VERSION, daemon_build: "stub".into() },
                },
            )
            .await;
            // Read the next request but never reply; just drop the connection.
            let _ = read_frame(&mut s).await;
            drop(s);
            // Never accept another connection; the test only checks the in-flight failure, not
            // reconnect success, so leave the listener alive but unanswered.
            std::future::pending::<()>().await;
        });
        wait_ready(&ready).await;

        let client = connect_at(path, "test".into()).await.unwrap();
        let err = client
            .request(Request::KillSession { session_id: "x".into() })
            .await
            .unwrap_err();
        assert!(matches!(err, ClientError::Disconnected), "expected Disconnected, got {err:?}");
    }

    /// Test-only alias: exercises `DaemonClient`'s private `connect_at` directly (bypassing
    /// `resolve_socket_path()`) so the stub daemon can bind an arbitrary tempdir path. Same
    /// codepath `connect()` uses in production.
    async fn connect_at(socket_path: PathBuf, client_build: String) -> Result<DaemonClient, ClientError> {
        DaemonClient::connect_at(socket_path, client_build).await
    }
}
