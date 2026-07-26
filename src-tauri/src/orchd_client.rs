//! Hop-B client: the Tauri core's Unix-domain-socket connection to `bpa-orchd` (spec §9, S3 T11).
//! MIRRORS `socket_client.rs`'s `DaemonClient` structure exactly, instantiated over
//! `bpa_orchd_proto`'s wire types instead of `bpa_protocol`'s — same socket-path-resolution
//! pattern (a LOCAL `socket_dir()` copy, not a shared dependency: src-tauri gains NO
//! `bpa-daemon-core` dependency), same handshake/correlation/push-fan-out/reconnect-loop design,
//! same `HANDSHAKE_SUSPECT_CAP` transient-handshake-failure escalation. Only the wire types differ:
//! `OrchdRequest`/`OrchdResponse`/`OrchdPush`/`OrchdFrame` (via `bpa_orchd_proto::{
//! encode_orchd_frame, OrchdFrameDecoder}`) instead of `Request`/`Response`/`Push`/`Frame`, and the
//! preamble negotiates `[ORCHD_CLIENT_MIN_VERSION, ORCHD_CLIENT_MAX_VERSION]` instead of
//! `[CLIENT_MIN_VERSION, CLIENT_MAX_VERSION]`. The codec-agnostic preamble handshake itself
//! (`bpa_protocol::preamble::{ClientPreamble, DaemonReply, encode_client_preamble,
//! decode_daemon_reply, PREAMBLE_TIMEOUT}`) is generic and reused as-is — `bpa-orchd-proto`'s own
//! docs note `negotiate()` is shared verbatim across daemons (spec §4.1/§4.2).
//!
//! ## Framing (spec §4.1/§4.2)
//!
//! Every wire message is a `u32`-LE length prefix + `CBOR(OrchdFrame)`, via
//! `bpa_orchd_proto::{encode_orchd_frame, OrchdFrameDecoder}` — a thin instantiation of
//! `bpa_protocol`'s generic CBOR-framing core over `OrchdFrame` instead of `Frame`, so the core and
//! `bpa-orchd` can never drift on the oversized-length / partial-frame rules.
//!
//! ## Design
//!
//! A single owning **reader/writer task** (`tokio::spawn`) drives one live connection at a time:
//! it owns the socket, the `FrameReader`, and the request-correlation map, so there is exactly one
//! writer and no shared-writer lock across an `.await`. `OrchdClient` itself is just a cheap handle
//! holding an `mpsc::Sender<OrchdClientCmd>` to that task plus the push/conn callback registries (so
//! `on_push`/`on_conn` can be registered any time, including after `connect()` returns).
//!
//! On disconnect (read EOF / IO error / write failure) the task drains every in-flight request with
//! `Err(OrchdClientError::Disconnected)` (never a fake success), fires `on_conn(Disconnected)`, then
//! reconnects with bounded exponential backoff (`BACKOFF_START` doubling up to `BACKOFF_CAP`).
//! A version-`Incompatible` reply is fatal and is not retried — the task exits with a
//! `tracing::error!` rather than looping forever against an unrecoverable version skew.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bpa_orchd_proto::{
    encode_orchd_frame, OrchdFrame, OrchdFrameDecoder, OrchdPush, OrchdRequest, OrchdResponse,
    ORCHD_CLIENT_MAX_VERSION, ORCHD_CLIENT_MIN_VERSION,
};
#[cfg(test)]
use bpa_protocol::preamble::encode_daemon_reply;
use bpa_protocol::preamble::{
    decode_daemon_reply, encode_client_preamble, ClientPreamble, DaemonReply, PREAMBLE_TIMEOUT,
};
use bpa_protocol::sync::lock;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::sync::{mpsc, oneshot};

/// Initial reconnect backoff delay (mirrors `socket_client::BACKOFF_START`, spec §13).
const BACKOFF_START: Duration = Duration::from_millis(100);
/// Reconnect backoff cap (mirrors `socket_client::BACKOFF_CAP`).
const BACKOFF_CAP: Duration = Duration::from_secs(5);
/// Per-request timeout: a request that never gets a correlated reply (daemon hung, or the
/// connection drops mid-request) fails honestly instead of hanging the caller forever.
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
/// Depth of the command channel from `OrchdClient::request` callers to the owning task.
const CMD_CHANNEL_CAP: usize = 256;
/// Number of consecutive *transient* handshake failures (EOF / timeout / garbage / bad magic)
/// `connect_with_backoff` tolerates, within one reconnect cycle, before escalating to the fatal
/// unknown-range `IncompatibleOrchd{0,0}` classification. Mirrors
/// `socket_client::HANDSHAKE_SUSPECT_CAP`'s rationale exactly (see that constant's docs) — orchd
/// has no history of a v1-era pre-preamble daemon, but the classification itself must stay
/// identical: a booting-but-compatible daemon still inside its own cold-start window looks
/// byte-for-byte the same on the wire as a permanently unhandshakeable one until the cap is
/// exhausted.
const HANDSHAKE_SUSPECT_CAP: u32 = 8;

/// Production initial-connect retry budget (mirrors `socket_client::BOOT_CONNECT_ATTEMPTS`):
/// the `attempts` value `lib.rs`'s `bring_up_orchd` passes to
/// [`OrchdClient::connect_with_retry`] at app boot (paired with its 500 ms delay: up to ~4 s of
/// bounded retry). Named — rather than a literal `8` at the call site — for the identical reason
/// `socket_client::BOOT_CONNECT_ATTEMPTS` is named: the initial-connect Incompatible escalation
/// only fires if the retry loop actually reaches `HANDSHAKE_SUSPECT_CAP` consecutive transient
/// failures, enforced both by the compile-time assertion below and the runtime clamp in
/// [`OrchdClient::connect_with_retry`] itself.
pub(crate) const BOOT_CONNECT_ATTEMPTS: u32 = 8;

const _: () = assert!(
    BOOT_CONNECT_ATTEMPTS >= HANDSHAKE_SUSPECT_CAP,
    "BOOT_CONNECT_ATTEMPTS must be >= HANDSHAKE_SUSPECT_CAP, or the initial-connect \
     IncompatibleOrchd escalation (upgrade dialog) becomes unreachable"
);

// ---------------------------------------------------------------------------------------------
// Socket path resolution — a LOCAL copy of `socket_client.rs`'s `socket_dir()`/
// `resolve_socket_path()` pattern (spec §8.1/§9), joining `orchd.sock` instead of `d.sock`. Kept
// as an independent copy rather than a shared helper (src-tauri gains NO `bpa-daemon-core`
// dependency — locked contract) so the core's own resolution logic never depends on the daemon
// crates' internal `bpa-daemon-core::singleton` module; `bpa-orchd`'s own `main.rs`/`boot.rs`
// resolve the identical path independently via `bpa-daemon-core`, so any drift here would mean
// the core connects to a socket `bpa-orchd` never binds.
// ---------------------------------------------------------------------------------------------

/// macOS `sun_path` is 104 bytes including the NUL terminator; usable length is strictly < 104
/// (spec §8.1). Mirrors `socket_client::SUN_PATH_MAX` / `bpa_daemon_core::singleton::SUN_PATH_MAX`
/// exactly.
const SUN_PATH_MAX: usize = 104;

/// Resolve the daemon's runtime directory: `$XDG_RUNTIME_DIR/bpa` if `XDG_RUNTIME_DIR` is set and
/// non-empty, else `/tmp/bpa-<uid>`. Byte-for-byte identical to `socket_client::socket_dir()` and
/// `bpa_daemon_core::singleton::socket_dir()` — both daemons share the same runtime directory,
/// distinguished only by their socket file's leaf name (`d.sock` vs `orchd.sock`).
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

/// Resolve `bpa-orchd`'s Unix-domain-socket path (`<dir>/orchd.sock`). Panics if the resolved
/// path would overflow `sun_path` (`>= 104` bytes) — mirrors `resolve_socket_path`'s fail-fast
/// rationale exactly: this is a hard boot-time misconfiguration, not a runtime condition worth
/// papering over.
pub fn resolve_orchd_socket_path() -> PathBuf {
    let path = socket_dir().join("orchd.sock");
    let len = path.as_os_str().len();
    assert!(
        len < SUN_PATH_MAX,
        "orchd socket path length {len} >= sun_path max {SUN_PATH_MAX}: {}",
        path.display()
    );
    path
}

// ---------------------------------------------------------------------------------------------
// Errors + connection-state
// ---------------------------------------------------------------------------------------------

/// Terminal error surfaced to the broker/UI. Mirrors `socket_client::ClientError` exactly (spec
/// §9's locked shape) — never panics on IO; every failure mode the client can hit is represented
/// here so callers can match honestly instead of guessing from a string.
#[derive(Debug, thiserror::Error)]
pub enum OrchdClientError {
    /// The connection is down (mid-reconnect) or the client is shutting down. A request made
    /// while disconnected — or in flight when the connection drops — resolves to this rather than
    /// hanging or faking success (spec §13).
    #[error("orchd disconnected")]
    Disconnected,
    /// The daemon rejected the request (`OrchdResponse::Error`). `code` is the `OrchdErrorCode`
    /// variant's `Debug` name (`"NotFound"` | `"Invariant"` | `"Validation"` | `"Conflict"` |
    /// `"Io"`, spec §9) rather than the enum itself, matching `ClientError::Daemon`'s
    /// `code: String` shape and `commands.rs`'s planned `CommandError::Daemon` mapping.
    #[error("orchd reported: {code}: {message}")]
    Daemon { code: String, message: String },
    /// The handshake preamble found no overlap between this client's `[ORCHD_CLIENT_MIN_VERSION,
    /// ORCHD_CLIENT_MAX_VERSION]` and the daemon's, OR a transient handshake failure repeated
    /// `HANDSHAKE_SUSPECT_CAP` times in a row on the same connect cycle. Mirrors
    /// `ClientError::IncompatibleDaemon` exactly — see its docs for the full transient-vs-fatal
    /// rationale. In the unknown-range case `daemon_min`/`daemon_max` are the `0, 0` sentinel.
    #[error("incompatible orchd (orchd supports [{daemon_min}, {daemon_max}])")]
    IncompatibleOrchd { daemon_min: u16, daemon_max: u16 },
    /// This single request, once CBOR-encoded, exceeds `bpa_protocol::MAX_FRAME_LEN`. Mirrors
    /// `ClientError::RequestTooLarge` exactly: detected by encoding the frame BEFORE it ever
    /// reaches the socket-write path, so a single oversized request fails ONLY itself.
    #[error("request too large once encoded ({size} bytes exceeds the frame cap)")]
    RequestTooLarge { size: usize },
}

/// Emitted by the reconnect loop so the broker can raise `orchd://down` / `orchd://up` /
/// `orchd://incompatible` (spec §9). Mirrors `socket_client::ConnState` exactly. `Connected` is
/// fired both for the initial connect and for every successful reconnect.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnState {
    Connected,
    Disconnected,
    /// The reconnect loop hit a fatal handshake classification — either a genuine decoded
    /// `DaemonReply::Incompatible{min,max}`, or a run of `HANDSHAKE_SUSPECT_CAP` consecutive
    /// transient handshake failures escalated to the same unknown-range (`0, 0`) shape.
    Incompatible {
        daemon_min: u16,
        daemon_max: u16,
    },
}

// ---------------------------------------------------------------------------------------------
// OrchdClient
// ---------------------------------------------------------------------------------------------

enum OrchdClientCmd {
    Request {
        req: OrchdRequest,
        reply: oneshot::Sender<Result<OrchdResponse, OrchdClientError>>,
    },
}

// `Send`-only (no `Sync` bound) — mirrors `socket_client`'s `PushCb`/`ConnCb` rationale exactly:
// every callback is invoked from inside a `Mutex<Vec<..>>` lock on a single task.
type PushCb = dyn Fn(OrchdPush) + Send;
type ConnCb = dyn Fn(ConnState) + Send;

/// Current `ConnState` plus every registered `on_conn` callback, behind a single `Mutex` — mirrors
/// `socket_client::ConnCbState` exactly (see its docs for the atomicity rationale).
struct ConnCbState {
    current: ConnState,
    cbs: Vec<Box<ConnCb>>,
}

/// State shared between `OrchdClient` (the cheap public handle) and `connection_task` — mirrors
/// `socket_client::SharedState` exactly.
struct SharedState {
    push_cb: Mutex<Vec<Box<PushCb>>>,
    conn_cb: Mutex<ConnCbState>,
    /// `true` only while `run_connection` is actively serving a live connection; see
    /// `socket_client::SharedState::live`'s docs for the full request()-during-reconnect-gap
    /// rationale this mirrors exactly.
    live: AtomicBool,
}

/// Handle to the `bpa-orchd` connection. Cheap to clone (wraps an `Arc` internally via its channel
/// sender); the actual socket, framing, and correlation state live in a background task spawned by
/// `connect()`. Mirrors `socket_client::DaemonClient` exactly.
pub struct OrchdClient {
    cmd_tx: mpsc::Sender<OrchdClientCmd>,
    shared: Arc<SharedState>,
}

/// Swappable slot: `None` while disconnected/incompatible, `Some` once a connection is live.
/// Mirrors `commands::ClientSlot` exactly, for the second daemon (spec §9's locked shape).
pub type OrchdClientSlot = Arc<std::sync::RwLock<Option<Arc<OrchdClient>>>>;

impl std::fmt::Debug for OrchdClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OrchdClient").finish_non_exhaustive()
    }
}

impl OrchdClient {
    /// Resolve the socket path, connect with bounded exponential backoff (cap `BACKOFF_CAP`), write
    /// the codec-agnostic client preamble, await the daemon's `Accepted`/`Incompatible` reply, then
    /// spawn the read/reconnect loop. Mirrors `DaemonClient::connect` exactly: the **first** connect
    /// attempt is a single try (no retry loop) so a genuinely absent daemon or a version mismatch
    /// surfaces immediately instead of hanging `connect()`. `client_build` is echoed to the daemon
    /// in the preamble for diagnostics; it never carries secrets.
    pub async fn connect(client_build: String) -> Result<OrchdClient, OrchdClientError> {
        let socket_path = resolve_orchd_socket_path();
        Self::connect_at(socket_path, client_build).await
    }

    /// Bounded-retry initial connect — mirrors `DaemonClient::connect_with_retry` exactly: resolves
    /// the socket path, then attempts [`connect_and_handshake`] up to `attempts` times (fixed
    /// `delay` between tries), applying the SAME [`HandshakeSuspectCounter`] classification
    /// `connect_with_backoff` (the reconnect loop) uses. `attempts` is clamped up to
    /// [`HANDSHAKE_SUSPECT_CAP`] for the identical reason `DaemonClient::connect_with_retry`
    /// clamps it — see that method's docs.
    pub async fn connect_with_retry(
        client_build: String,
        attempts: u32,
        delay: Duration,
    ) -> Result<OrchdClient, OrchdClientError> {
        let socket_path = resolve_orchd_socket_path();
        let attempts = attempts.max(HANDSHAKE_SUSPECT_CAP);
        Self::connect_at_with_retry(socket_path, client_build, attempts, delay).await
    }

    /// Shared implementation behind [`connect_with_retry`](Self::connect_with_retry): connects at an
    /// explicit socket path (production always resolves via `resolve_orchd_socket_path()`; tests
    /// point this at a stub daemon's tempdir socket instead).
    async fn connect_at_with_retry(
        socket_path: PathBuf,
        client_build: String,
        attempts: u32,
        delay: Duration,
    ) -> Result<OrchdClient, OrchdClientError> {
        let mut counter = HandshakeSuspectCounter::new();
        let mut last_err = OrchdClientError::Disconnected;
        for attempt in 1..=attempts.max(1) {
            let result = connect_and_handshake(&socket_path, &client_build).await;
            match counter.classify(result) {
                RetryDecision::Connected(stream, reader) => {
                    return Ok(Self::finish_connect(socket_path, client_build, stream, reader));
                }
                RetryDecision::Fatal(HandshakeError::Incompatible { min, max }) => {
                    tracing::warn!(attempt, orchd_min = min, orchd_max = max, "orchd incompatible; not retrying");
                    return Err(OrchdClientError::IncompatibleOrchd {
                        daemon_min: min,
                        daemon_max: max,
                    });
                }
                RetryDecision::Fatal(_) => unreachable!(
                    "HandshakeSuspectCounter::classify only returns RetryDecision::Fatal for HandshakeError::Incompatible"
                ),
                RetryDecision::Retry => {
                    tracing::warn!(attempt, attempts, "orchd connect attempt failed");
                    last_err = OrchdClientError::Disconnected;
                    if attempt < attempts.max(1) {
                        tokio::time::sleep(delay).await;
                    }
                }
            }
        }
        Err(last_err)
    }

    /// Shared implementation behind `connect()`: connects at an explicit socket path. A single try
    /// (see `connect()`'s own docs) — `connect_with_retry` above is the bounded-retry-with-
    /// escalation entry point production boot uses instead.
    async fn connect_at(
        socket_path: PathBuf,
        client_build: String,
    ) -> Result<OrchdClient, OrchdClientError> {
        let (stream, reader) = connect_and_handshake(&socket_path, &client_build)
            .await
            .map_err(|e| match e {
                // A single transient handshake failure on the *initial* connect is ambiguous in
                // exactly the same way it is on reconnect — map it to `Disconnected` rather than
                // instantly surfacing a false `IncompatibleOrchd`/upgrade dialog for a daemon that is
                // merely slow to finish booting. A CALLER retrying this single try repeatedly must
                // use `connect_with_retry` (above) instead of calling this directly in a loop.
                HandshakeError::Io(_) | HandshakeError::TransientHandshake => {
                    OrchdClientError::Disconnected
                }
                HandshakeError::Incompatible { min, max } => OrchdClientError::IncompatibleOrchd {
                    daemon_min: min,
                    daemon_max: max,
                },
            })?;

        Ok(Self::finish_connect(
            socket_path,
            client_build,
            stream,
            reader,
        ))
    }

    /// Shared tail of both `connect_at` (single try) and `connect_at_with_retry` (bounded retry
    /// with escalation): the handshake has already succeeded — build the shared state, seed it
    /// `Connected`/live, and spawn the owning connection task. Mirrors
    /// `DaemonClient::finish_connect`'s synchronous-seed rationale exactly.
    fn finish_connect(
        socket_path: PathBuf,
        client_build: String,
        stream: UnixStream,
        reader: FrameReader,
    ) -> OrchdClient {
        let (cmd_tx, cmd_rx) = mpsc::channel::<OrchdClientCmd>(CMD_CHANNEL_CAP);
        let shared = Arc::new(SharedState {
            push_cb: Mutex::new(Vec::new()),
            conn_cb: Mutex::new(ConnCbState {
                current: ConnState::Connected,
                cbs: Vec::new(),
            }),
            live: AtomicBool::new(true),
        });

        tokio::spawn(connection_task(
            socket_path,
            client_build,
            stream,
            reader,
            cmd_rx,
            shared.clone(),
        ));

        OrchdClient { cmd_tx, shared }
    }

    /// Allocate a monotonic id, send `OrchdFrame::Request { id, .. }`, and await the correlated
    /// `OrchdResponse`. `OrchdResponse::Error` maps to `Err(OrchdClientError::Daemon)`. If the
    /// connection is down, returns `Err(OrchdClientError::Disconnected)` rather than hanging. Times
    /// out after `REQUEST_TIMEOUT` as a last-resort safety net. Mirrors `DaemonClient::request`
    /// exactly, including the pre-enqueue `live` liveness check.
    pub async fn request(&self, req: OrchdRequest) -> Result<OrchdResponse, OrchdClientError> {
        // The single per-request completion-tracing choke-point on the CORE side (spec D4, O-6):
        // one structured `info!` line per request, covering ALL 133 Tauri command handlers at the
        // one layer they share (they all funnel through here) instead of a per-handler edit.
        // `verb` is the exhaustive, low-cardinality `OrchdRequest::verb_name` (reused verbatim from
        // the daemon's own dispatch trace); the line carries verb + outcome + error_code + elapsed
        // only — never the request args/body/tokens or the error `message` (which can hold paths).
        let verb = req.verb_name();
        let started = std::time::Instant::now();
        let result = self.request_inner(req).await;
        let elapsed_ms = started.elapsed().as_millis() as u64;
        match &result {
            Ok(_) => {
                tracing::info!(verb, outcome = "ok", elapsed_ms, "orchd request completed");
            }
            Err(e) => {
                // A low-cardinality code name only — the daemon-side `OrchdErrorCode` debug name
                // for a rejected request, or the client-transport failure variant otherwise. Never
                // the accompanying `message`.
                let error_code: &str = match e {
                    OrchdClientError::Daemon { code, .. } => code,
                    OrchdClientError::Disconnected => "Disconnected",
                    OrchdClientError::IncompatibleOrchd { .. } => "IncompatibleOrchd",
                    OrchdClientError::RequestTooLarge { .. } => "RequestTooLarge",
                };
                tracing::info!(
                    verb,
                    outcome = "err",
                    error_code,
                    elapsed_ms,
                    "orchd request completed"
                );
            }
        }
        result
    }

    /// The transport half of [`request`](Self::request): allocate a monotonic id, send
    /// `OrchdFrame::Request { id, .. }`, and await the correlated `OrchdResponse` (an
    /// `OrchdResponse::Error` already arrives here as `Err(OrchdClientError::Daemon)` — mapped by
    /// the connection task). Split out so the completion trace above wraps a single call and sees
    /// every outcome, including the pre-enqueue disconnected short-circuit.
    async fn request_inner(&self, req: OrchdRequest) -> Result<OrchdResponse, OrchdClientError> {
        if !self.shared.live.load(Ordering::Acquire) {
            return Err(OrchdClientError::Disconnected);
        }

        let (reply_tx, reply_rx) = oneshot::channel();
        self.cmd_tx
            .send(OrchdClientCmd::Request {
                req,
                reply: reply_tx,
            })
            .await
            .map_err(|_| OrchdClientError::Disconnected)?;

        match tokio::time::timeout(REQUEST_TIMEOUT, reply_rx).await {
            Ok(Ok(res)) => res,
            Ok(Err(_)) => Err(OrchdClientError::Disconnected),
            Err(_) => Err(OrchdClientError::Disconnected),
        }
    }

    /// Register a callback invoked for every `OrchdPush` frame the daemon sends. Multiple callbacks
    /// may be registered; each receives every push. Callbacks must not block (they run inline on
    /// the connection task) — hand off to a channel/spawn for anything slow.
    pub fn on_push(&self, cb: impl Fn(OrchdPush) + Send + 'static) {
        lock(&self.shared.push_cb).push(Box::new(cb));
    }

    /// Register a callback invoked on every connect/disconnect transition. Mirrors
    /// `DaemonClient::on_conn` exactly, including the immediate-replay-of-current-state guarantee
    /// (see its docs for the race it closes).
    pub fn on_conn(&self, cb: impl Fn(ConnState) + Send + 'static) {
        let mut guard = lock(&self.shared.conn_cb);
        cb(guard.current);
        guard.cbs.push(Box::new(cb));
    }
}

// ---------------------------------------------------------------------------------------------
// Framing + handshake helpers
// ---------------------------------------------------------------------------------------------

/// A stateful frame reader for one connection: owns `bpa_orchd_proto::OrchdFrameDecoder` plus a
/// queue of already-decoded-but-not-yet-returned frames. Mirrors `socket_client::FrameReader`
/// exactly.
struct FrameReader {
    decoder: OrchdFrameDecoder,
    pending: std::collections::VecDeque<OrchdFrame>,
    buf: Box<[u8; 16 * 1024]>,
}

impl FrameReader {
    fn new() -> Self {
        FrameReader {
            decoder: OrchdFrameDecoder::new(),
            pending: std::collections::VecDeque::new(),
            buf: Box::new([0u8; 16 * 1024]),
        }
    }

    /// Return the next complete `OrchdFrame`, reading from `stream` only when nothing is buffered.
    /// Mirrors `socket_client::FrameReader::next` exactly.
    async fn next(
        &mut self,
        stream: &mut (impl AsyncReadExt + Unpin),
    ) -> std::io::Result<Option<OrchdFrame>> {
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

/// Write already-encoded frame bytes to the socket. Mirrors `socket_client::write_encoded_frame`
/// exactly (protocol-agnostic — operates on raw bytes only).
async fn write_encoded_frame(
    stream: &mut (impl AsyncWriteExt + Unpin),
    bytes: &[u8],
) -> std::io::Result<()> {
    stream.write_all(bytes).await?;
    stream.flush().await
}

/// Encode an `OrchdFrame::Request` for the wire, distinguishing an oversized-once-encoded body
/// (`OrchdClientError::RequestTooLarge` — resolved directly against that request's own oneshot,
/// connection untouched) from every other encode failure. Mirrors
/// `socket_client::encode_request_frame` exactly.
fn encode_request_frame(frame: &OrchdFrame) -> Result<Vec<u8>, OrchdClientError> {
    encode_orchd_frame(frame).map_err(|e| match e {
        bpa_protocol::FrameError::Oversized(size) => OrchdClientError::RequestTooLarge {
            size: size as usize,
        },
        other => {
            tracing::error!(error = %other, "unexpected orchd frame encode failure");
            OrchdClientError::Disconnected
        }
    })
}

/// Errors from a single connect+handshake attempt. Mirrors `socket_client::HandshakeError`
/// exactly.
enum HandshakeError {
    /// `UnixStream::connect` itself failed (daemon not up yet / ENOENT / connection refused).
    Io(std::io::Error),
    /// The connection reached the daemon at the socket level, but the handshake itself did not
    /// complete honestly. See `socket_client::HandshakeError::TransientHandshake`'s docs for the
    /// full ambiguous-vs-fatal rationale this mirrors exactly.
    TransientHandshake,
    /// The daemon explicitly, decodably replied `DaemonReply::Incompatible{min,max}`.
    Incompatible { min: u16, max: u16 },
}

impl From<std::io::Error> for HandshakeError {
    fn from(e: std::io::Error) -> Self {
        HandshakeError::Io(e)
    }
}

/// Sentinel for "the daemon's real supported range is unknown". Mirrors
/// `socket_client::UNKNOWN_RANGE` exactly.
const UNKNOWN_RANGE: HandshakeError = HandshakeError::Incompatible { min: 0, max: 0 };

/// What a caller's retry loop should do after one `connect_and_handshake` attempt. Mirrors
/// `socket_client::RetryDecision` exactly.
enum RetryDecision {
    Connected(UnixStream, FrameReader),
    Fatal(HandshakeError),
    Retry,
}

/// Shared "classify + count consecutive transient handshake failures + escalate" state. Mirrors
/// `socket_client::HandshakeSuspectCounter` exactly.
#[derive(Default)]
struct HandshakeSuspectCounter {
    consecutive_transient: u32,
}

impl HandshakeSuspectCounter {
    fn new() -> Self {
        Self::default()
    }

    fn classify(
        &mut self,
        result: Result<(UnixStream, FrameReader), HandshakeError>,
    ) -> RetryDecision {
        match result {
            Ok((stream, reader)) => {
                self.consecutive_transient = 0;
                RetryDecision::Connected(stream, reader)
            }
            Err(e @ HandshakeError::Incompatible { .. }) => RetryDecision::Fatal(e),
            Err(HandshakeError::TransientHandshake) => {
                self.consecutive_transient += 1;
                if self.consecutive_transient >= HANDSHAKE_SUSPECT_CAP {
                    tracing::error!(
                        cap = HANDSHAKE_SUSPECT_CAP,
                        "transient orchd handshake failure repeated past HANDSHAKE_SUSPECT_CAP; \
                         escalating to incompatible"
                    );
                    RetryDecision::Fatal(UNKNOWN_RANGE)
                } else {
                    tracing::warn!(
                        consecutive_transient = self.consecutive_transient,
                        cap = HANDSHAKE_SUSPECT_CAP,
                        "transient orchd handshake failure; will retry"
                    );
                    RetryDecision::Retry
                }
            }
            Err(HandshakeError::Io(err)) => {
                self.consecutive_transient = 0;
                tracing::warn!(error = %err, "orchd connect failed; will retry");
                RetryDecision::Retry
            }
        }
    }
}

/// Read the daemon's preamble reply off `stream`. Mirrors `socket_client::read_daemon_reply`
/// exactly — the wire format is generic (`bpa_protocol::preamble`), not sessiond-specific.
async fn read_daemon_reply(stream: &mut UnixStream) -> std::io::Result<DaemonReply> {
    const HEADER_LEN: usize = 4 + 1 + 2 + 2;
    let mut header = [0u8; HEADER_LEN];
    stream.read_exact(&mut header).await?;
    let result = header[4];
    let mut buf = header.to_vec();
    if result == 1 {
        let build_len = u16::from_le_bytes(header[7..9].try_into().unwrap()) as usize;
        if build_len > bpa_protocol::preamble::MAX_PREAMBLE_BUILD_LEN {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "daemon reply build string exceeds MAX_PREAMBLE_BUILD_LEN",
            ));
        }
        if build_len > 0 {
            let mut build = vec![0u8; build_len];
            stream.read_exact(&mut build).await?;
            buf.extend_from_slice(&build);
        }
    }
    decode_daemon_reply(&buf)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string()))
}

/// Connect once and run the preamble handshake: write the client's `[ORCHD_CLIENT_MIN_VERSION,
/// ORCHD_CLIENT_MAX_VERSION]` + `client_build`, then read the daemon's reply under
/// `PREAMBLE_TIMEOUT`. Mirrors `socket_client::connect_and_handshake` exactly, substituting the
/// orchd version consts.
async fn connect_and_handshake(
    socket_path: &Path,
    client_build: &str,
) -> Result<(UnixStream, FrameReader), HandshakeError> {
    let mut stream = UnixStream::connect(socket_path).await?;

    let preamble_bytes = encode_client_preamble(&ClientPreamble {
        min: ORCHD_CLIENT_MIN_VERSION,
        max: ORCHD_CLIENT_MAX_VERSION,
        build: client_build.to_string(),
    });

    let handshake = async {
        stream.write_all(&preamble_bytes).await?;
        stream.flush().await?;
        read_daemon_reply(&mut stream).await
    };

    let reply = match tokio::time::timeout(PREAMBLE_TIMEOUT, handshake).await {
        Ok(Ok(reply)) => reply,
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "orchd handshake failed reading daemon reply; treating as transient");
            return Err(HandshakeError::TransientHandshake);
        }
        Err(_) => {
            tracing::warn!(
                "orchd handshake timed out waiting for daemon reply; treating as transient"
            );
            return Err(HandshakeError::TransientHandshake);
        }
    };

    match reply {
        DaemonReply::Accepted { chosen, build } => {
            let reader = FrameReader::new();
            tracing::info!(chosen, daemon_build = %build, "orchd handshake accepted");
            Ok((stream, reader))
        }
        DaemonReply::Incompatible { min, max } => Err(HandshakeError::Incompatible { min, max }),
    }
}

/// Connect with bounded exponential backoff. Mirrors `socket_client::connect_with_backoff`
/// exactly.
async fn connect_with_backoff(
    socket_path: &Path,
    client_build: &str,
) -> Result<(UnixStream, FrameReader), HandshakeError> {
    let mut delay = BACKOFF_START;
    let mut counter = HandshakeSuspectCounter::new();
    loop {
        let result = connect_and_handshake(socket_path, client_build).await;
        match counter.classify(result) {
            RetryDecision::Connected(stream, reader) => return Ok((stream, reader)),
            RetryDecision::Fatal(e) => return Err(e),
            RetryDecision::Retry => {
                tracing::warn!(
                    delay_ms = delay.as_millis(),
                    "backing off before next orchd connect attempt"
                );
                tokio::time::sleep(delay).await;
                delay = std::cmp::min(delay * 2, BACKOFF_CAP);
            }
        }
    }
}

// ---------------------------------------------------------------------------------------------
// Connection-owning task: handshake already done for the first connection; drives request writes,
// frame reads, correlation, push fan-out, and reconnect-on-drop. Mirrors
// `socket_client::{fire_conn, fire_push, LoopEnd, run_connection, connection_task}` exactly.
// ---------------------------------------------------------------------------------------------

fn fire_conn(cbs: &Mutex<ConnCbState>, state: ConnState) {
    let mut guard = lock(cbs);
    guard.current = state;
    for cb in guard.cbs.iter() {
        cb(state);
    }
}

fn fire_push(cbs: &Mutex<Vec<Box<PushCb>>>, push: OrchdPush) {
    for cb in lock(cbs).iter() {
        cb(push.clone());
    }
}

#[derive(PartialEq, Eq)]
enum LoopEnd {
    ConnectionLost,
    ClientDropped,
}

async fn run_connection(
    stream: &mut UnixStream,
    reader: &mut FrameReader,
    cmd_rx: &mut mpsc::Receiver<OrchdClientCmd>,
    next_id: &AtomicU64,
    pending: &Mutex<HashMap<u64, oneshot::Sender<Result<OrchdResponse, OrchdClientError>>>>,
    push_cb: &Mutex<Vec<Box<PushCb>>>,
) -> LoopEnd {
    loop {
        tokio::select! {
            biased;
            cmd = cmd_rx.recv() => {
                match cmd {
                    None => return LoopEnd::ClientDropped,
                    Some(OrchdClientCmd::Request { req, reply }) => {
                        let id = next_id.fetch_add(1, Ordering::Relaxed);
                        let bytes = match encode_request_frame(&OrchdFrame::Request { id, req }) {
                            Ok(bytes) => bytes,
                            Err(e) => {
                                tracing::warn!(error = %e, "orchd request encode failed; failing only this request");
                                let _ = reply.send(Err(e));
                                continue;
                            }
                        };
                        lock(pending).insert(id, reply);
                        if let Err(e) = write_encoded_frame(stream, &bytes).await {
                            tracing::warn!(error = %e, "orchd write failed; dropping connection");
                            return LoopEnd::ConnectionLost;
                        }
                    }
                }
            }
            frame = reader.next(stream) => {
                match frame {
                    Err(e) => {
                        tracing::warn!(error = %e, "orchd read failed; dropping connection");
                        return LoopEnd::ConnectionLost;
                    }
                    Ok(None) => {
                        tracing::warn!("orchd closed the connection; dropping");
                        return LoopEnd::ConnectionLost;
                    }
                    Ok(Some(OrchdFrame::Response { id, res })) => {
                        if let Some(tx) = lock(pending).remove(&id) {
                            let mapped = match res {
                                OrchdResponse::Error { code, message } => {
                                    Err(OrchdClientError::Daemon { code: format!("{code:?}"), message })
                                }
                                other => Ok(other),
                            };
                            let _ = tx.send(mapped);
                        } else {
                            tracing::warn!(id, "orchd response for unknown request id; dropping");
                        }
                    }
                    Ok(Some(OrchdFrame::Push(p))) => fire_push(push_cb, p),
                    Ok(Some(OrchdFrame::Request { .. })) => {
                        tracing::warn!("unexpected Request frame from orchd; ignoring");
                    }
                }
            }
        }
    }
}

async fn connection_task(
    socket_path: PathBuf,
    client_build: String,
    mut stream: UnixStream,
    mut reader: FrameReader,
    mut cmd_rx: mpsc::Receiver<OrchdClientCmd>,
    shared: Arc<SharedState>,
) {
    let next_id = AtomicU64::new(1);
    let pending: Mutex<HashMap<u64, oneshot::Sender<Result<OrchdResponse, OrchdClientError>>>> =
        Mutex::new(HashMap::new());

    loop {
        let end = run_connection(
            &mut stream,
            &mut reader,
            &mut cmd_rx,
            &next_id,
            &pending,
            &shared.push_cb,
        )
        .await;
        shared.live.store(false, Ordering::Release);

        for (_id, tx) in lock(&pending).drain() {
            let _ = tx.send(Err(OrchdClientError::Disconnected));
        }

        if end == LoopEnd::ClientDropped {
            return;
        }

        fire_conn(&shared.conn_cb, ConnState::Disconnected);
        tracing::warn!("orchd connection lost; reconnecting");

        match connect_with_backoff(&socket_path, &client_build).await {
            Ok((s, r)) => {
                stream = s;
                reader = r;
                shared.live.store(true, Ordering::Release);
                fire_conn(&shared.conn_cb, ConnState::Connected);
            }
            Err(HandshakeError::Incompatible { min, max }) => {
                tracing::error!(
                    min,
                    max,
                    client_max = ORCHD_CLIENT_MAX_VERSION,
                    "orchd became incompatible; giving up reconnect"
                );
                fire_conn(
                    &shared.conn_cb,
                    ConnState::Incompatible {
                        daemon_min: min,
                        daemon_max: max,
                    },
                );
                return;
            }
            Err(_) => unreachable!(
                "connect_with_backoff only returns Err for HandshakeError::Incompatible"
            ),
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

    use bpa_orchd_proto::{Project, ProjectStatus};

    // --- framing helpers reused by the stub daemon ---
    async fn read_frame(stream: &mut UnixStream) -> Option<OrchdFrame> {
        let mut len_buf = [0u8; 4];
        stream.read_exact(&mut len_buf).await.ok()?;
        let len = u32::from_le_bytes(len_buf) as usize;
        let mut body = vec![0u8; len];
        stream.read_exact(&mut body).await.ok()?;
        let mut decoder = OrchdFrameDecoder::new();
        decoder.push(&len_buf);
        decoder.push(&body);
        decoder.decode().ok()?.into_iter().next()
    }

    async fn write_stub_frame(stream: &mut UnixStream, frame: &OrchdFrame) {
        let bytes = encode_orchd_frame(frame).unwrap();
        stream.write_all(&bytes).await.unwrap();
        stream.flush().await.unwrap();
    }

    /// Read and decode one client preamble off the wire.
    async fn read_client_preamble_stub(stream: &mut UnixStream) -> ClientPreamble {
        let mut header = [0u8; 10];
        stream.read_exact(&mut header).await.unwrap();
        let build_len = u16::from_le_bytes(header[8..10].try_into().unwrap()) as usize;
        let mut buf = header.to_vec();
        if build_len > 0 {
            let mut build = vec![0u8; build_len];
            stream.read_exact(&mut build).await.unwrap();
            buf.extend_from_slice(&build);
        }
        bpa_protocol::preamble::decode_client_preamble(&buf).expect("valid client preamble")
    }

    /// Read the client preamble then reply `Accepted{chosen: ORCHD_DAEMON_MAX_VERSION, ..}` — the
    /// version const, never a hardcoded literal (spec §9's locked test discipline).
    async fn accept_handshake(stream: &mut UnixStream) {
        let _client_preamble = read_client_preamble_stub(stream).await;
        let reply = encode_daemon_reply(&DaemonReply::Accepted {
            chosen: bpa_orchd_proto::ORCHD_DAEMON_MAX_VERSION,
            build: "stub".into(),
        });
        stream.write_all(&reply).await.unwrap();
        stream.flush().await.unwrap();
    }

    fn tmp_sock() -> PathBuf {
        let dir = tempfile::tempdir().unwrap();
        // leak the tempdir so the socket path stays valid for the test's lifetime
        let p = dir.path().join("orchd.sock");
        std::mem::forget(dir);
        p
    }

    async fn wait_ready(ready: &AtomicBool) {
        while !ready.load(Ordering::SeqCst) {
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    }

    // ---- resolve_orchd_socket_path -------------------------------------------------------
    //
    // Same `ENV_TEST_LOCK` discipline as `socket_client`'s own env-dependent tests — a single
    // process-wide mutex, held across each mutate-read-restore window, serializes just these
    // tests against each other without slowing down any other test in this module.
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
    fn orchd_socket_path_uses_xdg_runtime_dir_when_set() {
        with_env("XDG_RUNTIME_DIR", Some("/run/user/501"), || {
            let sock = resolve_orchd_socket_path();
            assert_eq!(sock, PathBuf::from("/run/user/501/bpa/orchd.sock"));
        });
    }

    #[test]
    fn orchd_socket_path_falls_back_to_tmp_with_uid_when_xdg_unset() {
        with_env("XDG_RUNTIME_DIR", None, || {
            let sock = resolve_orchd_socket_path();
            let uid = unsafe { libc::geteuid() };
            assert_eq!(sock, PathBuf::from(format!("/tmp/bpa-{uid}/orchd.sock")));
        });
    }

    #[test]
    fn orchd_socket_path_is_under_104_bytes() {
        with_env("XDG_RUNTIME_DIR", Some("/run/user/501"), || {
            let sock = resolve_orchd_socket_path();
            assert!(sock.as_os_str().len() < SUN_PATH_MAX);
        });
    }

    // ---- handshake + correlation ----------------------------------------------------------

    /// Stub daemon: handshakes, then for each Request replies with a distinguishable Response.
    /// `CreateProject{name,..}` -> `Project{ id=name, name, .. }` so the client can assert the
    /// reply matches the request it sent (correlation proof). Delays the FIRST reply so a later
    /// request can overtake it, proving correlation is by id, not FIFO reply order.
    fn spawn_stub(path: PathBuf, ready: Arc<AtomicBool>) {
        tokio::spawn(async move {
            let listener = UnixListener::bind(&path).unwrap();
            ready.store(true, Ordering::SeqCst);
            let (mut stream, _) = listener.accept().await.unwrap();
            accept_handshake(&mut stream).await;

            let mut first_seen = false;
            loop {
                let Some(frame) = read_frame(&mut stream).await else {
                    break;
                };
                if let OrchdFrame::Request { id, req } = frame {
                    let res = match req {
                        OrchdRequest::CreateProject {
                            name,
                            description,
                            workspace_ids,
                        } => OrchdResponse::Project(Project {
                            id: name.clone(),
                            name,
                            description,
                            status: ProjectStatus::Active,
                            workspace_ids,
                            created_at: 0,
                            updated_at: 0,
                        }),
                        _ => OrchdResponse::Ack,
                    };
                    if !first_seen {
                        first_seen = true;
                        tokio::time::sleep(Duration::from_millis(120)).await;
                    }
                    write_stub_frame(&mut stream, &OrchdFrame::Response { id, res }).await;
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
            c1.request(OrchdRequest::CreateProject {
                name: "one".into(),
                description: "d1".into(),
                workspace_ids: vec!["w1".into()],
            })
            .await
        });
        tokio::time::sleep(Duration::from_millis(20)).await;
        let c2 = client.clone();
        let h2 = tokio::spawn(async move {
            c2.request(OrchdRequest::CreateProject {
                name: "two".into(),
                description: "d2".into(),
                workspace_ids: vec!["w2".into()],
            })
            .await
        });

        let r1 = h1.await.unwrap().unwrap();
        let r2 = h2.await.unwrap().unwrap();
        match r1 {
            OrchdResponse::Project(p) => assert_eq!(p.name, "one"),
            o => panic!("{o:?}"),
        }
        match r2 {
            OrchdResponse::Project(p) => assert_eq!(p.name, "two"),
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
            accept_handshake(&mut stream).await;
            if let Some(OrchdFrame::Request { id, .. }) = read_frame(&mut stream).await {
                write_stub_frame(
                    &mut stream,
                    &OrchdFrame::Response {
                        id,
                        res: OrchdResponse::Error {
                            code: bpa_orchd_proto::OrchdErrorCode::NotFound,
                            message: "gone".into(),
                        },
                    },
                )
                .await;
            }
        });
        wait_ready(&ready).await;

        let client = connect_at(path, "test".into()).await.unwrap();
        let err = client
            .request(OrchdRequest::DeleteIdea {
                id: "missing".into(),
            })
            .await
            .unwrap_err();
        match err {
            OrchdClientError::Daemon { code, message } => {
                assert_eq!(code, "NotFound");
                assert_eq!(message, "gone");
            }
            o => panic!("expected OrchdClientError::Daemon, got {o:?}"),
        }
    }

    #[tokio::test]
    async fn incompatible_orchd_reply_surfaces_typed_error() {
        let path = tmp_sock();
        let ready = Arc::new(AtomicBool::new(false));
        let p = path.clone();
        let r = ready.clone();
        tokio::spawn(async move {
            let listener = UnixListener::bind(&p).unwrap();
            r.store(true, Ordering::SeqCst);
            let (mut stream, _) = listener.accept().await.unwrap();
            let _client_preamble = read_client_preamble_stub(&mut stream).await;
            let reply = encode_daemon_reply(&DaemonReply::Incompatible { min: 2, max: 2 });
            stream.write_all(&reply).await.unwrap();
            stream.flush().await.unwrap();
        });
        wait_ready(&ready).await;

        let started = std::time::Instant::now();
        let err = connect_at(path, "test".into()).await.unwrap_err();
        let elapsed = started.elapsed();

        match err {
            OrchdClientError::IncompatibleOrchd {
                daemon_min,
                daemon_max,
            } => {
                assert_eq!((daemon_min, daemon_max), (2, 2));
            }
            o => panic!("expected OrchdClientError::IncompatibleOrchd, got {o:?}"),
        }
        assert!(
            elapsed < Duration::from_secs(1),
            "expected a fast, non-retried failure, took {elapsed:?}"
        );
    }

    #[tokio::test]
    async fn orchd_closing_during_handshake_is_transient_disconnected_not_hang() {
        let path = tmp_sock();
        let ready = Arc::new(AtomicBool::new(false));
        let p = path.clone();
        let r = ready.clone();
        tokio::spawn(async move {
            let listener = UnixListener::bind(&p).unwrap();
            r.store(true, Ordering::SeqCst);
            let (mut stream, _) = listener.accept().await.unwrap();
            let _client_preamble = read_client_preamble_stub(&mut stream).await;
            drop(stream);
        });
        wait_ready(&ready).await;

        let started = std::time::Instant::now();
        let result = tokio::time::timeout(
            PREAMBLE_TIMEOUT + Duration::from_secs(2),
            connect_at(path, "test".into()),
        )
        .await
        .expect("connect_at must not hang past PREAMBLE_TIMEOUT on a closed-mid-handshake peer");
        let elapsed = started.elapsed();

        let err = result.unwrap_err();
        assert!(
            matches!(err, OrchdClientError::Disconnected),
            "expected the retryable Disconnected (transient handshake failure), got {err:?}"
        );
        assert!(
            elapsed < PREAMBLE_TIMEOUT + Duration::from_secs(1),
            "connect_at took {elapsed:?}, expected to give up at or before PREAMBLE_TIMEOUT"
        );
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
                accept_handshake(&mut s).await;
                write_stub_frame(&mut s, &OrchdFrame::Push(OrchdPush::ProjectsChanged)).await;
                // drop `s` -> connection closes
            }
            // connection #2
            {
                let (mut s, _) = listener.accept().await.unwrap();
                accept_handshake(&mut s).await; // handshake again
                if let Some(OrchdFrame::Request { id, .. }) = read_frame(&mut s).await {
                    write_stub_frame(
                        &mut s,
                        &OrchdFrame::Response {
                            id,
                            res: OrchdResponse::Ack,
                        },
                    )
                    .await;
                }
            }
        });
        wait_ready(&ready).await;

        let pushes: Arc<Mutex<Vec<OrchdPush>>> = Arc::new(Mutex::new(Vec::new()));
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
            match client
                .request(OrchdRequest::ArchiveProject { id: "x".into() })
                .await
            {
                Ok(OrchdResponse::Ack) => {
                    got_ack = true;
                    break;
                }
                _ => tokio::time::sleep(Duration::from_millis(50)).await,
            }
        }
        assert!(got_ack, "expected an Ack after reconnect");

        let got_push = pushes
            .lock()
            .unwrap()
            .iter()
            .any(|p| matches!(p, OrchdPush::ProjectsChanged));
        assert!(got_push, "expected the ProjectsChanged push");

        let s = states.lock().unwrap().clone();
        assert_eq!(s.first(), Some(&ConnState::Connected));
        assert!(
            s.contains(&ConnState::Disconnected),
            "expected a Disconnected transition: {s:?}"
        );
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
            accept_handshake(&mut s).await;
            let _ = read_frame(&mut s).await;
            drop(s);
            std::future::pending::<()>().await;
        });
        wait_ready(&ready).await;

        let client = connect_at(path, "test".into()).await.unwrap();
        let err = client
            .request(OrchdRequest::ArchiveProject { id: "x".into() })
            .await
            .unwrap_err();
        assert!(
            matches!(err, OrchdClientError::Disconnected),
            "expected Disconnected, got {err:?}"
        );
    }

    #[tokio::test]
    async fn oversized_request_fails_itself_only_connection_stays_alive() {
        let path = tmp_sock();
        let ready = Arc::new(AtomicBool::new(false));
        spawn_stub(path.clone(), ready.clone());
        wait_ready(&ready).await;

        let client = connect_at(path, "test".into()).await.unwrap();

        // ~17 MB text string: CBOR encodes a text string ~1:1, so this alone (unlike an integer
        // array) comfortably exceeds bpa_protocol::MAX_FRAME_LEN (16 MiB) once encoded.
        let oversized = "A".repeat(17_000_000);
        let err = client
            .request(OrchdRequest::ImportBundle { json: oversized })
            .await
            .unwrap_err();
        assert!(
            matches!(err, OrchdClientError::RequestTooLarge { .. }),
            "expected RequestTooLarge, got {err:?}"
        );

        // The connection must still be alive: a normal request right after must succeed, with NO
        // reconnect/disconnect cycle in between.
        let ok = client
            .request(OrchdRequest::ArchiveProject { id: "x".into() })
            .await
            .expect("a normal request on the same connection must still succeed");
        assert!(matches!(ok, OrchdResponse::Ack));
    }

    /// Test-only alias: exercises `OrchdClient`'s private `connect_at` directly (bypassing
    /// `resolve_orchd_socket_path()`) so the stub daemon can bind an arbitrary tempdir path.
    async fn connect_at(
        socket_path: PathBuf,
        client_build: String,
    ) -> Result<OrchdClient, OrchdClientError> {
        OrchdClient::connect_at(socket_path, client_build).await
    }

    /// Test-only alias: exercises `OrchdClient`'s private `connect_at_with_retry` directly.
    async fn connect_at_with_retry(
        socket_path: PathBuf,
        client_build: String,
        attempts: u32,
        delay: Duration,
    ) -> Result<OrchdClient, OrchdClientError> {
        OrchdClient::connect_at_with_retry(socket_path, client_build, attempts, delay).await
    }

    // ---- connect-refused: must stay retryable Disconnected, never escalate to Incompatible ----

    #[tokio::test]
    async fn initial_connect_refused_never_escalates_within_the_cap() {
        // Plain connect-refused (nothing listening at all) is a completely different failure mode
        // from a transient handshake failure: it must keep retrying as plain "orchd not up yet"
        // and NEVER escalate to IncompatibleOrchd, even across a full HANDSHAKE_SUSPECT_CAP-sized
        // attempt budget.
        let dir = tempfile::tempdir().unwrap();
        let unreachable = dir.path().join("nothing-listens-here.sock");

        let started = std::time::Instant::now();
        let result = connect_at_with_retry(
            unreachable,
            "test".into(),
            HANDSHAKE_SUSPECT_CAP,
            Duration::from_millis(5),
        )
        .await;
        let elapsed = started.elapsed();

        assert!(
            matches!(result, Err(OrchdClientError::Disconnected)),
            "connect-refused must stay Disconnected, never escalate to IncompatibleOrchd: {result:?}"
        );
        assert!(
            elapsed < Duration::from_secs(5),
            "connect_at_with_retry took {elapsed:?}; expected a prompt bounded give-up"
        );
    }

    #[tokio::test]
    async fn genuine_incompatible_reply_on_initial_connect_is_immediately_fatal_no_retry() {
        let path = tmp_sock();
        let ready = Arc::new(AtomicBool::new(false));
        let p = path.clone();
        let r = ready.clone();
        tokio::spawn(async move {
            let listener = UnixListener::bind(&p).unwrap();
            r.store(true, Ordering::SeqCst);
            let (mut s, _) = listener.accept().await.unwrap();
            let _client_preamble = read_client_preamble_stub(&mut s).await;
            let reply = encode_daemon_reply(&DaemonReply::Incompatible { min: 5, max: 6 });
            s.write_all(&reply).await.unwrap();
            s.flush().await.unwrap();
            std::future::pending::<()>().await;
        });
        wait_ready(&ready).await;

        let started = std::time::Instant::now();
        let result =
            connect_at_with_retry(path, "test".into(), 8, Duration::from_millis(500)).await;
        let elapsed = started.elapsed();

        match result {
            Err(OrchdClientError::IncompatibleOrchd {
                daemon_min,
                daemon_max,
            }) => assert_eq!((daemon_min, daemon_max), (5, 6)),
            other => panic!("expected IncompatibleOrchd{{5,6}}, got {other:?}"),
        }
        assert!(
            elapsed < Duration::from_secs(1),
            "connect_at_with_retry took {elapsed:?}; a genuine Incompatible reply must return \
             promptly with no retry, not run the full ~3.5s of bounded backoff"
        );
    }

    #[tokio::test]
    async fn initial_connect_escalates_to_incompatible_after_cap_exhausted_transient_failures() {
        // The daemon ALWAYS EOFs mid-handshake, forever, from the very FIRST connection. After
        // HANDSHAKE_SUSPECT_CAP consecutive transient failures, `connect_at_with_retry` must give
        // up and return `IncompatibleOrchd{0,0}` (the unknown-range sentinel), never an infinite
        // `Disconnected` retry.
        let path = tmp_sock();
        let ready = Arc::new(AtomicBool::new(false));
        let p = path.clone();
        let r = ready.clone();
        tokio::spawn(async move {
            let listener = UnixListener::bind(&p).unwrap();
            r.store(true, Ordering::SeqCst);
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let mut header = [0u8; 10];
                let _ = stream.read_exact(&mut header).await;
                drop(stream);
            }
        });
        wait_ready(&ready).await;

        let started = std::time::Instant::now();
        let result = connect_at_with_retry(
            path,
            "test".into(),
            HANDSHAKE_SUSPECT_CAP,
            Duration::from_millis(5),
        )
        .await;
        let elapsed = started.elapsed();

        match result {
            Err(OrchdClientError::IncompatibleOrchd {
                daemon_min,
                daemon_max,
            }) => {
                assert_eq!(
                    (daemon_min, daemon_max),
                    (0, 0),
                    "expected the unknown-range sentinel, not a genuine daemon-advertised range"
                );
            }
            other => panic!(
                "expected IncompatibleOrchd{{0,0}} after {HANDSHAKE_SUSPECT_CAP} consecutive \
                 transient failures on the INITIAL connect, got {other:?}"
            ),
        }
        assert!(
            elapsed < Duration::from_secs(5),
            "connect_at_with_retry took {elapsed:?}; expected a prompt escalation once the cap \
             is exhausted, not a hang"
        );
    }

    // ---- BL-125: reconnect/handshake coverage ported from `socket_client::tests` (the two
    // ---- clients mirror each other; these scenarios existed only on the sessiond side). -------

    #[test]
    fn orchd_socket_path_falls_back_to_tmp_when_xdg_empty() {
        with_env("XDG_RUNTIME_DIR", Some(""), || {
            let sock = resolve_orchd_socket_path();
            let uid = unsafe { libc::geteuid() };
            assert_eq!(sock, PathBuf::from(format!("/tmp/bpa-{uid}/orchd.sock")));
        });
    }

    #[tokio::test]
    async fn request_during_reconnect_gap_fails_promptly() {
        let path = tmp_sock();
        let ready = Arc::new(AtomicBool::new(false));
        let p = path.clone();
        let r = ready.clone();
        tokio::spawn(async move {
            let listener = UnixListener::bind(&p).unwrap();
            r.store(true, Ordering::SeqCst);
            // connection #1: handshake, then immediately drop — never accept again, so the
            // client sits in the reconnect backoff loop for the rest of the test.
            let (mut s, _) = listener.accept().await.unwrap();
            accept_handshake(&mut s).await;
            drop(s);
            // Never accept connection #2: the client stays stuck in connect_with_backoff.
            std::future::pending::<()>().await;
        });
        wait_ready(&ready).await;

        let client = connect_at(path, "test".into()).await.unwrap();

        // Wait for the client to observe the drop and enter the reconnect gap.
        let states: Arc<Mutex<Vec<ConnState>>> = Arc::new(Mutex::new(Vec::new()));
        let states_cb = states.clone();
        client.on_conn(move |s| states_cb.lock().unwrap().push(s));
        for _ in 0..100 {
            if states.lock().unwrap().contains(&ConnState::Disconnected) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(
            states.lock().unwrap().contains(&ConnState::Disconnected),
            "test setup: client never observed the disconnect"
        );

        // Now request() while the client is stuck in connect_with_backoff (reconnect gap, no live
        // connection). This must fail promptly with Disconnected, not silently enqueue and hang
        // for up to REQUEST_TIMEOUT (30s).
        let started = std::time::Instant::now();
        let result = tokio::time::timeout(
            Duration::from_secs(1),
            client.request(OrchdRequest::ArchiveProject { id: "x".into() }),
        )
        .await;
        let elapsed = started.elapsed();

        let err = result
            .unwrap_or_else(|_| panic!("request() did not return within 1s (took > {elapsed:?}); it silently queued during the reconnect gap"))
            .unwrap_err();
        assert!(
            matches!(err, OrchdClientError::Disconnected),
            "expected Disconnected, got {err:?}"
        );
        assert!(
            elapsed < Duration::from_secs(1),
            "request() took {elapsed:?} to fail; expected a prompt failure, not a queued/timed-out one"
        );
    }

    #[tokio::test]
    async fn late_registered_on_conn_observes_current_connected_state() {
        let path = tmp_sock();
        let ready = Arc::new(AtomicBool::new(false));
        spawn_stub(path.clone(), ready.clone());
        wait_ready(&ready).await;

        let client = connect_at(path, "test".into()).await.unwrap();

        // Force the race: yield back to the scheduler (and give the connection task a moment to
        // run) *before* registering on_conn, so the initial `fire_conn(Connected)` has every
        // opportunity to have already fired and been missed by a callback that isn't registered
        // yet.
        tokio::task::yield_now().await;
        tokio::time::sleep(Duration::from_millis(50)).await;
        tokio::task::yield_now().await;

        let states: Arc<Mutex<Vec<ConnState>>> = Arc::new(Mutex::new(Vec::new()));
        let states_cb = states.clone();
        client.on_conn(move |s| states_cb.lock().unwrap().push(s));

        // The callback must be replayed with the CURRENT state immediately upon registration —
        // no further `.await` should be required to observe it.
        let s = states.lock().unwrap().clone();
        assert_eq!(
            s.first(),
            Some(&ConnState::Connected),
            "a callback registered after connect() (with an intervening await) must observe \
             ConnState::Connected immediately, got {s:?}"
        );
    }

    /// Accept a connection, read the client's preamble, then close WITHOUT replying — the
    /// "transient handshake failure" shape (EOF mid-handshake). Mirrors `socket_client`'s
    /// `accept_and_eof` exactly.
    async fn accept_and_eof(listener: &UnixListener) {
        let (mut s, _) = listener.accept().await.unwrap();
        let _client_preamble = read_client_preamble_stub(&mut s).await;
        drop(s);
    }

    #[tokio::test]
    async fn transient_handshake_failures_below_cap_eventually_connect_with_no_incompatible_event()
    {
        // N < HANDSHAKE_SUSPECT_CAP EOFs on RECONNECT attempts, then a real handshake succeeds.
        // The client must reconnect cleanly with no IncompatibleOrchd anywhere in the process and
        // no ConnState::Incompatible ever fired.
        let path = tmp_sock();
        let ready = Arc::new(AtomicBool::new(false));
        let p = path.clone();
        let r = ready.clone();
        tokio::spawn(async move {
            let listener = UnixListener::bind(&p).unwrap();
            r.store(true, Ordering::SeqCst);

            // connection #1: real handshake, then drop (simulates the very first disconnect).
            {
                let (mut s, _) = listener.accept().await.unwrap();
                accept_handshake(&mut s).await;
                drop(s);
            }
            // reconnect attempts #2..#4: EOF mid-handshake (transient), well under the cap of 8.
            for _ in 0..3 {
                accept_and_eof(&listener).await;
            }
            // reconnect attempt #5: real handshake succeeds.
            {
                let (mut s, _) = listener.accept().await.unwrap();
                accept_handshake(&mut s).await;
                if let Some(OrchdFrame::Request { id, .. }) = read_frame(&mut s).await {
                    write_stub_frame(
                        &mut s,
                        &OrchdFrame::Response {
                            id,
                            res: OrchdResponse::Ack,
                        },
                    )
                    .await;
                }
                std::future::pending::<()>().await;
            }
        });
        wait_ready(&ready).await;

        let client = connect_at(path, "test".into()).await.unwrap();
        let states: Arc<Mutex<Vec<ConnState>>> = Arc::new(Mutex::new(Vec::new()));
        let states_cb = states.clone();
        client.on_conn(move |s| states_cb.lock().unwrap().push(s));

        // Retry the request until it succeeds against the eventually-recovered connection —
        // proves the client reconnected fine despite the 3 transient EOFs in between.
        let mut got_ack = false;
        for _ in 0..200 {
            match client
                .request(OrchdRequest::ArchiveProject { id: "x".into() })
                .await
            {
                Ok(OrchdResponse::Ack) => {
                    got_ack = true;
                    break;
                }
                _ => tokio::time::sleep(Duration::from_millis(50)).await,
            }
        }
        assert!(
            got_ack,
            "client must recover and connect after transient handshake failures below the cap"
        );

        let s = states.lock().unwrap().clone();
        assert!(
            !s.iter()
                .any(|x| matches!(x, ConnState::Incompatible { .. })),
            "no ConnState::Incompatible must fire while under HANDSHAKE_SUSPECT_CAP: {s:?}"
        );
    }

    #[tokio::test]
    async fn transient_handshake_failures_exceeding_cap_surface_incompatible_orchd() {
        // The daemon ALWAYS EOFs mid-handshake, forever — a stand-in for a permanently
        // unhandshakeable orchd. After HANDSHAKE_SUSPECT_CAP consecutive transient failures, the
        // reconnect loop must give up and fire ConnState::Incompatible{0,0} (unknown range).
        let path = tmp_sock();
        let ready = Arc::new(AtomicBool::new(false));
        let p = path.clone();
        let r = ready.clone();
        tokio::spawn(async move {
            let listener = UnixListener::bind(&p).unwrap();
            r.store(true, Ordering::SeqCst);
            // connection #1: real handshake, then drop, to get the client into the reconnect loop.
            {
                let (mut s, _) = listener.accept().await.unwrap();
                accept_handshake(&mut s).await;
                drop(s);
            }
            // Every subsequent attempt EOFs mid-handshake, forever.
            loop {
                accept_and_eof(&listener).await;
            }
        });
        wait_ready(&ready).await;

        let client = connect_at(path, "test".into()).await.unwrap();
        let states: Arc<Mutex<Vec<ConnState>>> = Arc::new(Mutex::new(Vec::new()));
        let states_cb = states.clone();
        client.on_conn(move |s| states_cb.lock().unwrap().push(s));

        // Backoff doubles from 100ms up to 5s over 8 attempts — bound the wait generously.
        let deadline = std::time::Instant::now() + Duration::from_secs(30);
        let mut found = false;
        while std::time::Instant::now() < deadline {
            if states
                .lock()
                .unwrap()
                .iter()
                .any(|x| matches!(x, ConnState::Incompatible { .. }))
            {
                found = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let s = states.lock().unwrap().clone();
        assert!(
            found,
            "expected ConnState::Incompatible after HANDSHAKE_SUSPECT_CAP transient failures: {s:?}"
        );
        match s
            .iter()
            .find(|x| matches!(x, ConnState::Incompatible { .. }))
            .unwrap()
        {
            ConnState::Incompatible {
                daemon_min,
                daemon_max,
            } => {
                assert_eq!((*daemon_min, *daemon_max), (0, 0), "unknown-range sentinel");
            }
            _ => unreachable!(),
        }

        // The client must now be permanently dead: further requests fail Disconnected forever
        // rather than hanging (request() checks `live` before enqueuing).
        let err = client
            .request(OrchdRequest::ArchiveProject { id: "x".into() })
            .await
            .unwrap_err();
        assert!(matches!(err, OrchdClientError::Disconnected));
    }

    #[tokio::test]
    async fn genuine_incompatible_reply_on_reconnect_is_immediately_fatal_and_fires_conn_state() {
        // A GENUINE, well-formed Incompatible{2,2} reply (not an ambiguous EOF) must remain
        // immediately fatal with no retry at all — and must fire ConnState::Incompatible with the
        // real daemon-advertised range, not the 0,0 unknown-range sentinel.
        let path = tmp_sock();
        let ready = Arc::new(AtomicBool::new(false));
        let p = path.clone();
        let r = ready.clone();
        tokio::spawn(async move {
            let listener = UnixListener::bind(&p).unwrap();
            r.store(true, Ordering::SeqCst);
            // connection #1: real handshake, then drop.
            {
                let (mut s, _) = listener.accept().await.unwrap();
                accept_handshake(&mut s).await;
                drop(s);
            }
            // reconnect attempt: a genuine Incompatible{2,2} reply.
            let (mut s, _) = listener.accept().await.unwrap();
            let _client_preamble = read_client_preamble_stub(&mut s).await;
            let reply = encode_daemon_reply(&DaemonReply::Incompatible { min: 2, max: 2 });
            s.write_all(&reply).await.unwrap();
            s.flush().await.unwrap();
            std::future::pending::<()>().await;
        });
        wait_ready(&ready).await;

        let client = connect_at(path, "test".into()).await.unwrap();
        let states: Arc<Mutex<Vec<ConnState>>> = Arc::new(Mutex::new(Vec::new()));
        let states_cb = states.clone();
        client.on_conn(move |s| states_cb.lock().unwrap().push(s));

        let started = std::time::Instant::now();
        let mut found = false;
        for _ in 0..100 {
            if states
                .lock()
                .unwrap()
                .iter()
                .any(|x| matches!(x, ConnState::Incompatible { .. }))
            {
                found = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let elapsed = started.elapsed();
        let s = states.lock().unwrap().clone();
        assert!(found, "expected ConnState::Incompatible: {s:?}");
        assert!(
            elapsed < Duration::from_secs(2),
            "a genuine Incompatible reply must be immediately fatal, not retried: took {elapsed:?}"
        );
        match s
            .iter()
            .find(|x| matches!(x, ConnState::Incompatible { .. }))
            .unwrap()
        {
            ConnState::Incompatible {
                daemon_min,
                daemon_max,
            } => assert_eq!((*daemon_min, *daemon_max), (2, 2)),
            _ => unreachable!(),
        }
    }
}
