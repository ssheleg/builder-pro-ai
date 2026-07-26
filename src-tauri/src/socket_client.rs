//! Hop-B client: the Tauri core's Unix-domain-socket connection to `bpa-sessiond` (spec §7, §8.1,
//! §13). Owns socket-path resolution, handshake, monotonic request/response correlation, push
//! fan-out, and a bounded-backoff reconnect loop that surfaces connect/disconnect transitions so
//! the broker (Task 17) can raise `daemon://disconnected` / `daemon://reconnected`.
//!
//! ## Framing (spec §7)
//!
//! Every wire message is a `u32`-LE length prefix + `CBOR(Frame)`. We reuse the protocol
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
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

#[cfg(test)]
use bpa_protocol::preamble::encode_daemon_reply;
use bpa_protocol::preamble::{
    decode_daemon_reply, encode_client_preamble, ClientPreamble, DaemonReply, PREAMBLE_TIMEOUT,
};
use bpa_protocol::sync::lock;
use bpa_protocol::{
    encode_frame, Frame, FrameDecoder, Push, Request, Response, CLIENT_MAX_VERSION,
    CLIENT_MIN_VERSION,
};
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
/// Number of consecutive *transient* handshake failures (EOF / timeout / garbage / bad magic — the
/// connection reached the daemon at the socket level, but the handshake itself did not complete
/// honestly) `connect_with_backoff` tolerates, within one reconnect cycle, before escalating to the
/// fatal unknown-range `IncompatibleDaemon{0,0}` classification. Does NOT count plain
/// connect-refused (`HandshakeError::Io` — nothing listening yet), which keeps its existing
/// unlimited-until-bound retry: a booting daemon that hasn't bound its socket yet is a completely
/// different, unambiguous case from one that accepted the connection and then failed to speak.
///
/// Rationale for the cap value: EOF mid-handshake is inherently ambiguous — it is exactly what a
/// v2 daemon looks like while it is still inside `cold_rehydrate_sessions` (bound but not yet
/// serving, spec's ~3-5s window) OR mid-crash-loop, and it is also exactly what a genuine v1
/// daemon looks like (v1 has no codec-agnostic preamble reader; it reads the connection as a
/// bincode `Request::Hello` frame it cannot decode and closes). A booting-but-compatible v2 daemon
/// recovers well within `HANDSHAKE_SUSPECT_CAP` retries (each one backs off up to `BACKOFF_CAP`);
/// a real v1 daemon closes on every single attempt and so reliably exhausts the cap, correctly
/// reaching the upgrade dialog instead of retrying forever.
const HANDSHAKE_SUSPECT_CAP: u32 = 8;

/// Production initial-connect retry budget (round-3 hardening H3): the `attempts` value `lib.rs`'s
/// `bring_up_daemon` passes to [`DaemonClient::connect_with_retry`] at app boot (paired with its
/// 500 ms delay: up to ~4 s of bounded retry, inside the spec's "~3-5 s" window). Named — rather
/// than a literal `8` at the call site — because the round-2 R1 fix's initial-connect Incompatible
/// escalation only fires if the retry loop actually reaches `HANDSHAKE_SUSPECT_CAP` consecutive
/// transient failures: an `attempts` quietly edited below the cap would exhaust the loop as plain
/// `Disconnected` and silently reintroduce the Critical (upgrade dialog unreachable). The
/// relationship is enforced twice: the compile-time assertion right below, and the runtime clamp in
/// [`DaemonClient::connect_with_retry`] itself.
pub(crate) const BOOT_CONNECT_ATTEMPTS: u32 = 8;

// H3 compile-time guard: the boot path's attempt budget must never be set below
// HANDSHAKE_SUSPECT_CAP, or the initial-connect Incompatible escalation (round-2 R1, CRITICAL)
// becomes unreachable — see BOOT_CONNECT_ATTEMPTS's doc. Fails the build, not a test run.
const _: () = assert!(
    BOOT_CONNECT_ATTEMPTS >= HANDSHAKE_SUSPECT_CAP,
    "BOOT_CONNECT_ATTEMPTS must be >= HANDSHAKE_SUSPECT_CAP, or the initial-connect \
     IncompatibleDaemon escalation (upgrade dialog) becomes unreachable"
);

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
    /// The handshake preamble (spec §4.5) found no overlap between this client's `[min, max]` and
    /// the daemon's (a genuine, well-formed `DaemonReply::Incompatible` was decoded), OR a
    /// transient handshake failure (EOF / timeout / garbage / bad magic — see
    /// `HandshakeError::TransientHandshake`) repeated `HANDSHAKE_SUSPECT_CAP` times in a row on the
    /// same connect cycle, at which point it is no longer treated as "daemon merely slow to boot"
    /// and is escalated to this same fatal shape. In the unknown-range case `daemon_min`/
    /// `daemon_max` are the `0, 0` sentinel — there was no reply to read a real range out of.
    /// Fatal: unlike `Disconnected`, this is never retried by the reconnect loop (a stale client
    /// build will never become compatible by waiting) — it is the signal that drives the upgrade
    /// flow instead.
    #[error("incompatible daemon (daemon supports [{daemon_min}, {daemon_max}])")]
    IncompatibleDaemon { daemon_min: u16, daemon_max: u16 },
    /// This single request, once CBOR-encoded, exceeds `bpa_protocol::MAX_FRAME_LEN` (finding [2]:
    /// e.g. an ~8.4 MiB `WriteStdin` paste — CBOR encodes `Vec<u8>` as an array of unsigned
    /// integers, so every byte >= 0x18 costs 2 wire bytes, meaning even moderate text pastes can
    /// exceed the 16 MiB frame cap). Detected by encoding the frame BEFORE it ever reaches the
    /// socket-write path, so a single oversized request fails ONLY itself — the connection, and
    /// every other in-flight/future request on it, stays completely unaffected. `size` is the
    /// encoded CBOR body length in bytes (matches `bpa_protocol::FrameError::Oversized`'s payload).
    #[error("request too large once encoded ({size} bytes exceeds the frame cap)")]
    RequestTooLarge { size: usize },
}

/// Emitted by the reconnect loop so the broker can raise `daemon://disconnected` /
/// `daemon://reconnected` / `daemon://incompatible` (spec §6.3, §13, §6.2). `Connected` is fired
/// both for the initial connect and for every successful reconnect; callers that need to
/// distinguish "first connect" from "reconnected after a drop" can track whether they have already
/// observed a `Disconnected`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnState {
    Connected,
    Disconnected,
    /// The reconnect loop hit a fatal handshake classification — either a genuine decoded
    /// `DaemonReply::Incompatible{min,max}`, or a run of `HANDSHAKE_SUSPECT_CAP` consecutive
    /// transient handshake failures that got escalated to the same unknown-range (`0, 0`) shape.
    /// Fired from inside `connection_task` BEFORE it returns (finding [11]: previously this case
    /// only logged a `tracing::error!` and silently left the client's slot holding a dead
    /// connection forever, with no event and no honest UI signal).
    Incompatible {
        daemon_min: u16,
        daemon_max: u16,
    },
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

/// Current `ConnState` plus every registered `on_conn` callback, behind a single `Mutex` so
/// "read the current state" and "subscribe to future changes" happen atomically (fixes the
/// initial-`Connected`-can-be-lost gap: a callback registered any time after `connect()` returns —
/// even racing the connection task's first `fire_conn(Connected)` — is guaranteed to either see
/// that transition go through `fire_conn` normally, or be invoked immediately with the state
/// `fire_conn` already recorded here before it got the lock).
struct ConnCbState {
    current: ConnState,
    cbs: Vec<Box<ConnCb>>,
}

/// State shared between `DaemonClient` (the cheap public handle) and `connection_task` (the
/// background task that owns the actual socket). Grouped into one struct — rather than three
/// separate `Arc` fields threaded individually through `connection_task`'s parameter list — purely
/// to keep that function's arity manageable; each field's own docs (below) still describe its
/// individual contract.
struct SharedState {
    push_cb: Mutex<Vec<Box<PushCb>>>,
    conn_cb: Mutex<ConnCbState>,
    /// `true` only while `run_connection` is actively serving a live connection; `false` while the
    /// client is between a lost connection and a successful reconnect (inside
    /// `connect_with_backoff`), and on shutdown. `request()` checks this *before* enqueuing so a
    /// call made during the reconnect gap fails immediately with `Disconnected` instead of
    /// silently queuing on the command channel and blocking on its reply for up to
    /// `REQUEST_TIMEOUT` — and possibly resolving on a *different*, newly-reconnected connection
    /// than the one the caller thought they were talking to.
    live: AtomicBool,
}

/// Handle to the daemon connection. Cheap to clone (wraps an `Arc` internally via its channel
/// sender); the actual socket, framing, and correlation state live in a background task spawned by
/// `connect()`.
pub struct DaemonClient {
    cmd_tx: mpsc::Sender<ClientCmd>,
    shared: Arc<SharedState>,
}

// Manual impl: the callback vecs hold `dyn Fn`, which is not `Debug`; this still gives a useful
// `{:?}` for logging/tests (e.g. `Result::unwrap_err` requires the `Ok` side to be `Debug`).
impl std::fmt::Debug for DaemonClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DaemonClient").finish_non_exhaustive()
    }
}

impl DaemonClient {
    /// Resolve the socket path, connect with bounded exponential backoff (cap `BACKOFF_CAP`), write
    /// the codec-agnostic client preamble, await the daemon's `Accepted`/`Incompatible` reply, then
    /// spawn the read/reconnect loop.
    ///
    /// The **first** connect attempt is a single try (no retry loop) so a genuinely absent daemon
    /// (nothing listening yet, e.g. before launchd has started it) or a version mismatch surfaces
    /// immediately to the caller instead of hanging `connect()` — the reconnect loop with backoff
    /// only kicks in for a connection that was live and then dropped. `client_build` is echoed to
    /// the daemon in the preamble for diagnostics; it never carries secrets.
    pub async fn connect(client_build: String) -> Result<DaemonClient, ClientError> {
        let socket_path = resolve_socket_path();
        Self::connect_at(socket_path, client_build).await
    }

    /// Bounded-retry initial connect (round-2 regression R1): resolves the socket path, then
    /// attempts [`connect_and_handshake`] up to `attempts` times (fixed `delay` between tries),
    /// applying the SAME [`HandshakeSuspectCounter`] classification `connect_with_backoff` (the
    /// reconnect loop) uses — this is the fix for R1: pre-fix, the initial-connect path had no
    /// counter at all and mapped every transient handshake failure straight to
    /// `ClientError::Disconnected` forever, so a present-but-unhandshakeable daemon (the dominant
    /// upgrade scenario: an old v1 daemon still running under launchd, which EOFs on every v2
    /// preamble it's handed) could never reach `IncompatibleDaemon` and the upgrade dialog was
    /// unreachable. Three outcomes, matching `connect_with_backoff`'s contract exactly:
    /// - connect-refused (`HandshakeError::Io`, nothing listening yet) never escalates — normal at
    ///   boot, keeps retrying for the full `attempts` budget.
    /// - `HANDSHAKE_SUSPECT_CAP` CONSECUTIVE transient handshake failures (EOF / timeout / garbage /
    ///   bad magic — the socket accepted but the handshake itself never completed honestly) escalate
    ///   to `ClientError::IncompatibleDaemon{daemon_min: 0, daemon_max: 0}` (the same unknown-range
    ///   sentinel the reconnect loop uses) rather than being retried past the cap.
    /// - a genuine, well-formed `DaemonReply::Incompatible{min,max}` is immediately fatal, exactly as
    ///   before (never retried, real range surfaced).
    ///
    /// Pulled out as its own function (rather than inlined in `lib.rs`'s `connect_with_retry`, which
    /// now just delegates here) so the handshake classification stays private to this module — the
    /// caller only ever sees the already-mapped `ClientError`.
    ///
    /// `attempts` is clamped up to [`HANDSHAKE_SUSPECT_CAP`] (round-3 hardening H3): the transient-
    /// failure escalation above only ever fires if the loop actually reaches the cap, so an
    /// `attempts` under it would exhaust the loop as plain `Disconnected` and silently reintroduce
    /// the round-2 R1 Critical (a present-but-unhandshakeable daemon never surfacing
    /// `IncompatibleDaemon`, upgrade dialog unreachable). Production passes
    /// [`BOOT_CONNECT_ATTEMPTS`] (compile-time-asserted `>= HANDSHAKE_SUSPECT_CAP`); this runtime
    /// clamp is the belt to that suspender for any other/future caller. Clamped HERE, on the boot-
    /// path entry, and deliberately NOT inside [`connect_at_with_retry`](Self::connect_at_with_retry)
    /// — tests drive that primitive directly with precise budgets.
    pub async fn connect_with_retry(
        client_build: String,
        attempts: u32,
        delay: Duration,
    ) -> Result<DaemonClient, ClientError> {
        let socket_path = resolve_socket_path();
        let attempts = attempts.max(HANDSHAKE_SUSPECT_CAP);
        Self::connect_at_with_retry(socket_path, client_build, attempts, delay).await
    }

    /// Shared implementation behind [`connect_with_retry`](Self::connect_with_retry): connects at an
    /// explicit socket path (production always resolves via `resolve_socket_path()`; tests point
    /// this at a stub daemon's tempdir socket instead).
    async fn connect_at_with_retry(
        socket_path: PathBuf,
        client_build: String,
        attempts: u32,
        delay: Duration,
    ) -> Result<DaemonClient, ClientError> {
        let mut counter = HandshakeSuspectCounter::new();
        let mut last_err = ClientError::Disconnected;
        for attempt in 1..=attempts.max(1) {
            let result = connect_and_handshake(&socket_path, &client_build).await;
            match counter.classify(result) {
                RetryDecision::Connected(stream, reader) => {
                    return Ok(Self::finish_connect(socket_path, client_build, stream, reader));
                }
                RetryDecision::Fatal(HandshakeError::Incompatible { min, max }) => {
                    tracing::warn!(attempt, daemon_min = min, daemon_max = max, "daemon incompatible; not retrying");
                    return Err(ClientError::IncompatibleDaemon {
                        daemon_min: min,
                        daemon_max: max,
                    });
                }
                RetryDecision::Fatal(_) => unreachable!(
                    "HandshakeSuspectCounter::classify only returns RetryDecision::Fatal for HandshakeError::Incompatible"
                ),
                RetryDecision::Retry => {
                    tracing::warn!(attempt, attempts, "daemon connect attempt failed");
                    last_err = ClientError::Disconnected;
                    if attempt < attempts.max(1) {
                        tokio::time::sleep(delay).await;
                    }
                }
            }
        }
        Err(last_err)
    }

    /// Shared implementation behind `connect()`: connects at an explicit socket path (production
    /// always resolves via `resolve_socket_path()`; tests point this at a stub daemon's tempdir
    /// socket instead). A single try (see `connect()`'s own docs) — `connect_with_retry` above is
    /// the bounded-retry-with-escalation entry point production boot uses instead.
    async fn connect_at(
        socket_path: PathBuf,
        client_build: String,
    ) -> Result<DaemonClient, ClientError> {
        let (stream, reader) = connect_and_handshake(&socket_path, &client_build)
            .await
            .map_err(|e| match e {
                // A single transient handshake failure on the *initial* connect is ambiguous in
                // exactly the same way it is on reconnect (finding [1]) — map it to `Disconnected`
                // rather than instantly surfacing a false `IncompatibleDaemon`/upgrade dialog for a
                // daemon that is merely slow to finish cold-rehydrate. A CALLER retrying this single
                // try repeatedly must use `connect_with_retry` (above) instead of calling this
                // directly in a loop, so consecutive transient failures actually get counted and
                // escalated (round-2 regression R1) rather than retried forever.
                HandshakeError::Io(_) | HandshakeError::TransientHandshake => {
                    ClientError::Disconnected
                }
                HandshakeError::Incompatible { min, max } => ClientError::IncompatibleDaemon {
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

    /// Shared tail of both `connect_at` (single try) and `connect_at_with_retry` (bounded retry with
    /// escalation, round-2 regression R1): the handshake has already succeeded — build the shared
    /// state, seed it `Connected`/live (see the inline rationale below), and spawn the owning
    /// connection task.
    fn finish_connect(
        socket_path: PathBuf,
        client_build: String,
        stream: UnixStream,
        reader: FrameReader,
    ) -> DaemonClient {
        let (cmd_tx, cmd_rx) = mpsc::channel::<ClientCmd>(CMD_CHANNEL_CAP);
        // Seeded at `Connected`/`live=true` here — synchronously, before `tokio::spawn` even
        // schedules `connection_task` to run — because the handshake above already completed: the
        // connection genuinely *is* live at this point. Deferring this seed to the first statement
        // inside `connection_task` (as an alternative design) would reopen exactly the race this
        // fix closes: the caller could get back a `DaemonClient` and immediately call `request()`
        // before the freshly-spawned task ever gets scheduled, observing a stale `live == false`
        // and failing with a spurious `Disconnected` on a connection that is, in fact, up.
        // `connection_task` itself must NOT re-fire `Connected`/re-set `live` as its first act —
        // that would double-fire the initial transition to any callback registered in the (tiny)
        // window between `DaemonClient` being constructed and `on_conn` being called.
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

        DaemonClient { cmd_tx, shared }
    }

    /// Allocate a monotonic id, send `Request { id, .. }`, and await the correlated `Response`.
    /// `Response::Error` maps to `Err(ClientError::Daemon)`. If the connection is down (mid-drop,
    /// mid-reconnect, or the client has been dropped), returns `Err(ClientError::Disconnected)`
    /// rather than hanging. Times out after `REQUEST_TIMEOUT` as a last-resort safety net against a
    /// daemon that accepted the request but never replies.
    ///
    /// Checks the `live` liveness flag *before* enqueuing: while the client is between a lost
    /// connection and a successful reconnect, `cmd_tx.send` would otherwise still succeed (the
    /// channel stays open and buffered) and the caller would silently block on the reply oneshot
    /// for up to `REQUEST_TIMEOUT` — worse, if reconnect succeeds inside that window the request
    /// would be sent on the *new* connection with no indication to the caller that a disconnect
    /// ever happened. Honest degradation means failing immediately here instead (spec §13). A
    /// request that *is* in flight when the connection drops still resolves to `Disconnected` via
    /// the existing `pending` drain in `connection_task`, unaffected by this check.
    pub async fn request(&self, req: Request) -> Result<Response, ClientError> {
        // The single per-request completion-tracing choke-point on the CORE side (spec D4):
        // one structured `info!` line per request, covering ALL Tauri command handlers at the
        // one layer they share (they all funnel through here) instead of a per-handler edit.
        // Mirrors `OrchdClient::request`'s trace exactly (BL-125: the two clients had drifted —
        // this line existed only on the orchd side). `verb` is the exhaustive, low-cardinality
        // `Request::verb_name` (reused verbatim from the daemon's own dispatch trace); the line
        // carries verb + outcome + error_code + elapsed only — never the request args/body or the
        // error `message` (which can hold paths).
        let verb = req.verb_name();
        let started = std::time::Instant::now();
        let result = self.request_inner(req).await;
        let elapsed_ms = started.elapsed().as_millis() as u64;
        match &result {
            Ok(_) => {
                tracing::info!(verb, outcome = "ok", elapsed_ms, "daemon request completed");
            }
            Err(e) => {
                // A low-cardinality code name only — the daemon-side `Response::Error` code for
                // a rejected request, or the client-transport failure variant otherwise. Never
                // the accompanying `message`.
                let error_code: &str = match e {
                    ClientError::Daemon { code, .. } => code,
                    ClientError::Disconnected => "Disconnected",
                    ClientError::IncompatibleDaemon { .. } => "IncompatibleDaemon",
                    ClientError::RequestTooLarge { .. } => "RequestTooLarge",
                };
                tracing::info!(
                    verb,
                    outcome = "err",
                    error_code,
                    elapsed_ms,
                    "daemon request completed"
                );
            }
        }
        result
    }

    /// The transport half of [`request`](Self::request): allocate a monotonic id, send
    /// `Frame::Request { id, .. }`, and await the correlated `Response` (a `Response::Error`
    /// already arrives here as `Err(ClientError::Daemon)` — mapped by the connection task).
    /// Split out so the completion trace above wraps a single call and sees every outcome,
    /// including the pre-enqueue disconnected short-circuit. Mirrors
    /// `OrchdClient::request_inner` exactly.
    async fn request_inner(&self, req: Request) -> Result<Response, ClientError> {
        if !self.shared.live.load(Ordering::Acquire) {
            return Err(ClientError::Disconnected);
        }

        let (reply_tx, reply_rx) = oneshot::channel();
        self.cmd_tx
            .send(ClientCmd::Request {
                req,
                reply: reply_tx,
            })
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
        lock(&self.shared.push_cb).push(Box::new(cb));
    }

    /// Register a callback invoked on every connect/disconnect transition (spec §6.3, §13: the
    /// broker maps these to `daemon://disconnected` / `daemon://reconnected`). Multiple callbacks
    /// may be registered. Callbacks must not block, per `on_push`.
    ///
    /// Invokes `cb` immediately with the *current* `ConnState` before subscribing it to future
    /// transitions — both under the same lock, so there is no window in which a transition could
    /// fire between "read current state" and "start listening for changes" and be missed. This
    /// closes the gap where `connect()` spawns `connection_task`, which fires the initial
    /// `Connected` as its first act, but a caller registering `on_conn` after `connect()` returns
    /// (with no synchronization) could previously race that first firing and never observe it. A
    /// callback registered any time after `connect()` returns is now guaranteed to observe
    /// `Connected` — either via this immediate replay, or via the normal `fire_conn` call if it
    /// registers before the connection task gets there first.
    pub fn on_conn(&self, cb: impl Fn(ConnState) + Send + 'static) {
        let mut guard = lock(&self.shared.conn_cb);
        cb(guard.current);
        guard.cbs.push(Box::new(cb));
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
    async fn next(
        &mut self,
        stream: &mut (impl AsyncReadExt + Unpin),
    ) -> std::io::Result<Option<Frame>> {
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

/// Write already-encoded frame bytes to the socket. Encoding is deliberately NOT done in here
/// (finding [2]): a `FrameError::Oversized` is a purely LOCAL failure of this one frame — encoding
/// must happen before a connection is judged dead by it, so the caller encodes first (see
/// `encode_request_frame`) and only reaches this function with bytes that are already known-good.
/// Any error from here on really is a dead socket.
async fn write_encoded_frame(
    stream: &mut (impl AsyncWriteExt + Unpin),
    bytes: &[u8],
) -> std::io::Result<()> {
    stream.write_all(bytes).await?;
    stream.flush().await
}

/// Encode a `Frame::Request` for the wire, distinguishing an oversized-once-encoded body
/// (`ClientError::RequestTooLarge`, finding [2] — resolved directly against that request's own
/// oneshot, connection untouched) from every other encode failure (mapped to `ClientError::
/// Disconnected` defensively; in practice `Frame`/`Request` are plain-data types that cannot fail
/// CBOR encoding any other way, but this keeps the mapping total rather than panicking).
fn encode_request_frame(frame: &Frame) -> Result<Vec<u8>, ClientError> {
    encode_frame(frame).map_err(|e| match e {
        bpa_protocol::FrameError::Oversized(size) => ClientError::RequestTooLarge {
            size: size as usize,
        },
        other => {
            tracing::error!(error = %other, "unexpected frame encode failure");
            ClientError::Disconnected
        }
    })
}

/// Errors from a single connect+handshake attempt. Distinct from `ClientError` (the public,
/// terminal-to-callers error) because `Incompatible` needs to short-circuit the backoff loop while
/// `Io` (the `UnixStream::connect` itself failing — e.g. the daemon isn't listening yet) and
/// `TransientHandshake` should keep retrying (the latter only up to `HANDSHAKE_SUSPECT_CAP` times
/// in a row — see `connect_with_backoff`).
enum HandshakeError {
    /// `UnixStream::connect` itself failed (daemon not up yet / ENOENT / connection refused). Not a
    /// version problem — the bounded-backoff reconnect loop should keep trying, unbounded (this is
    /// unambiguous: nothing is listening yet, which is expected during a cold boot race).
    Io(std::io::Error),
    /// The connection reached the daemon at the socket level, but the handshake itself did not
    /// complete honestly: a read error, EOF/closed-mid-handshake, a bad magic, a malformed reply, or
    /// the read exceeding `PREAMBLE_TIMEOUT`. Deliberately distinct from a genuine `Incompatible`
    /// reply (finding [1]): this is what a v2 daemon looks like while still inside
    /// `cold_rehydrate_sessions` (bound but not yet serving) or mid-crash-loop — retryable, NOT
    /// immediately fatal. Counted toward `HANDSHAKE_SUSPECT_CAP` by `connect_with_backoff`; a run of
    /// `HANDSHAKE_SUSPECT_CAP` consecutive occurrences escalates to `Incompatible` (unknown-range
    /// sentinel `0, 0`) since a genuinely stale v1 daemon closes the connection this way on every
    /// single attempt and will never recover.
    TransientHandshake,
    /// The daemon explicitly, decodably replied `DaemonReply::Incompatible{min,max}`: a genuine,
    /// well-formed version-range mismatch. `min`/`max` carry the daemon's real advertised range.
    /// Fatal and never retried — a stale client build will never become compatible by waiting.
    Incompatible { min: u16, max: u16 },
}

impl From<std::io::Error> for HandshakeError {
    fn from(e: std::io::Error) -> Self {
        HandshakeError::Io(e)
    }
}

/// Sentinel for "the daemon's real supported range is unknown" — used only once a run of
/// `HANDSHAKE_SUSPECT_CAP` consecutive `TransientHandshake` failures has been escalated to fatal
/// (spec §4.5 unknown-range shape), never for a single ambiguous failure in isolation.
const UNKNOWN_RANGE: HandshakeError = HandshakeError::Incompatible { min: 0, max: 0 };

/// What a caller's retry loop should do after one `connect_and_handshake` attempt, decided by
/// [`HandshakeSuspectCounter::classify`]. Both `connect_with_backoff` (the reconnect loop) and
/// `DaemonClient::connect_at`'s initial-connect retry loop (round-2 regression R1) drive their own
/// sleep/backoff shape around this SAME classification, so the two paths can never again drift on
/// what counts as "give up and escalate" vs. "keep trying" — the exact bug R1 fixes (the initial
/// path had its own, simpler mapping that never escalated at all).
enum RetryDecision {
    /// The connection is live; stop retrying.
    Connected(UnixStream, FrameReader),
    /// Fatal: a genuine, well-formed `Incompatible{min,max}` reply, or `HANDSHAKE_SUSPECT_CAP`
    /// consecutive transient failures escalated to the same shape. Never retried further.
    Fatal(HandshakeError),
    /// Keep retrying (either a plain connect-refused, which never counts toward the cap, or a
    /// transient handshake failure that has not yet hit the cap).
    Retry,
}

/// Shared "classify + count consecutive transient handshake failures + escalate" state (round-2
/// regression R1): pulled out of `connect_with_backoff` so `DaemonClient::connect_at`'s
/// initial-connect retry loop can apply the IDENTICAL escalation rule instead of duplicating (or,
/// pre-fix, omitting) the counter. Only `TransientHandshake` (EOF / timeout / garbage / bad magic —
/// the daemon accepted the connection but the handshake itself did not complete honestly) increments
/// the counter; a plain `Io` (connect-refused — nothing listening yet, unambiguous and expected
/// during a cold boot race) resets it to zero and is never itself escalated, regardless of how many
/// times it happens. See `HANDSHAKE_SUSPECT_CAP`'s doc for the full EOF-is-ambiguous rationale.
#[derive(Default)]
struct HandshakeSuspectCounter {
    consecutive_transient: u32,
}

impl HandshakeSuspectCounter {
    fn new() -> Self {
        Self::default()
    }

    /// Classify the outcome of one `connect_and_handshake` attempt, updating the internal
    /// consecutive-transient-failure count as a side effect.
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
                        "transient handshake failure repeated past HANDSHAKE_SUSPECT_CAP; \
                         escalating to incompatible"
                    );
                    RetryDecision::Fatal(UNKNOWN_RANGE)
                } else {
                    tracing::warn!(
                        consecutive_transient = self.consecutive_transient,
                        cap = HANDSHAKE_SUSPECT_CAP,
                        "transient handshake failure; will retry"
                    );
                    RetryDecision::Retry
                }
            }
            Err(HandshakeError::Io(err)) => {
                self.consecutive_transient = 0;
                tracing::warn!(error = %err, "daemon connect failed; will retry");
                RetryDecision::Retry
            }
        }
    }
}

/// Read the daemon's preamble reply off `stream` (mirrors
/// `bpa_sessiond::socket_server::read_client_preamble`'s shape on the other side of the handshake):
/// read the fixed 9-byte header (`magic:u32 | result:u8 | a:u16 | b:u16`) first, then branch — for
/// `Accepted` (`result == 1`) `a` is `chosen` and `b` is `build_len`, followed by exactly `build_len`
/// more bytes (rejecting an oversized declared length before reading/allocating that many bytes);
/// for `Incompatible` (`result == 0`) `a`/`b` are `daemon_min`/`daemon_max` and there is no trailing
/// body. Returns `Ok(DaemonReply)` only for a well-formed reply; any read error, short read, bad
/// magic, or malformed body is the caller's problem to map to `HandshakeError::Incompatible`
/// (unknown range) — this function itself carries no opinion on retryability.
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

/// Connect once and run the preamble handshake (spec §4.2/§4.5): write the client's
/// `[CLIENT_MIN_VERSION, CLIENT_MAX_VERSION]` + `client_build`, then read the daemon's reply under
/// `PREAMBLE_TIMEOUT`. Returns the live stream and a fresh `FrameReader` primed for the connection's
/// lifetime on `Accepted`. On a genuine, well-formed `Incompatible{min,max}` reply, returns
/// `HandshakeError::Incompatible` with the real range (fatal, not retryable — spec §4.5). On ANY
/// other failure reading/decoding the reply (IO error, EOF/closed-mid-handshake, bad magic,
/// malformed body, or the read exceeding `PREAMBLE_TIMEOUT`), returns
/// `HandshakeError::TransientHandshake` (finding [1]: ambiguous — could be a booting-but-compatible
/// daemon — so retryable, up to `HANDSHAKE_SUSPECT_CAP` consecutive occurrences, rather than
/// instantly fatal).
async fn connect_and_handshake(
    socket_path: &Path,
    client_build: &str,
) -> Result<(UnixStream, FrameReader), HandshakeError> {
    let mut stream = UnixStream::connect(socket_path).await?;

    let preamble_bytes = encode_client_preamble(&ClientPreamble {
        min: CLIENT_MIN_VERSION,
        max: CLIENT_MAX_VERSION,
        build: client_build.to_string(),
    });

    let handshake = async {
        stream.write_all(&preamble_bytes).await?;
        stream.flush().await?;
        read_daemon_reply(&mut stream).await
    };

    let reply = match tokio::time::timeout(PREAMBLE_TIMEOUT, handshake).await {
        Ok(Ok(reply)) => reply,
        // Write failed, or the read hit an IO error / EOF / bad magic / malformed body: the
        // handshake did not complete honestly, but we cannot yet tell a booting-but-compatible
        // daemon apart from a genuinely stale one — treat it as transient (retryable up to the cap).
        Ok(Err(e)) => {
            tracing::warn!(error = %e, "handshake failed reading daemon reply; treating as transient");
            return Err(HandshakeError::TransientHandshake);
        }
        // Silent/stuck peer within PREAMBLE_TIMEOUT: also ambiguous (could be a slow cold-rehydrate)
        // — transient, not an instant fatal give-up.
        Err(_) => {
            tracing::warn!("handshake timed out waiting for daemon reply; treating as transient");
            return Err(HandshakeError::TransientHandshake);
        }
    };

    match reply {
        DaemonReply::Accepted { chosen, build } => {
            let reader = FrameReader::new();
            tracing::info!(chosen, daemon_build = %build, "daemon handshake accepted");
            Ok((stream, reader))
        }
        DaemonReply::Incompatible { min, max } => Err(HandshakeError::Incompatible { min, max }),
    }
}

/// Connect with bounded exponential backoff (`BACKOFF_START` doubling up to `BACKOFF_CAP`, spec
/// §13). A genuine, well-formed `Incompatible{min,max}` reply is fatal and returned immediately
/// without retrying — a stale client build will never become compatible by waiting.
///
/// `HandshakeError::TransientHandshake` (finding [1]: EOF / timeout / garbage / bad magic — the
/// daemon accepted the TCP-level connection but the handshake itself didn't complete honestly)
/// keeps retrying with the same backoff as a plain connect-refused, but only up to
/// `HANDSHAKE_SUSPECT_CAP` **consecutive** occurrences: once that many transient failures happen in
/// a row without an intervening plain connect-refused, this is escalated to the same fatal
/// unknown-range shape as a genuine `Incompatible` (`UNKNOWN_RANGE`) — see that constant's docs for
/// the EOF-is-ambiguous rationale. The counter resets to zero on a plain `Io` (connect-refused)
/// failure: that is an unrelated failure mode (nothing listening yet) and must not itself count
/// toward — or be capped by — the handshake-specific budget.
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
                    "backing off before next attempt"
                );
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

/// Record the new state and fan it out to every registered callback, all under one lock — so a
/// concurrent `on_conn` registration (which also takes this lock to read-then-subscribe) can never
/// interleave with a transition and observe a stale `current` after subscribing, or miss a
/// callback that was mid-registration.
fn fire_conn(cbs: &Mutex<ConnCbState>, state: ConnState) {
    let mut guard = lock(cbs);
    guard.current = state;
    for cb in guard.cbs.iter() {
        cb(state);
    }
}

fn fire_push(cbs: &Mutex<Vec<Box<PushCb>>>, push: Push) {
    for cb in lock(cbs).iter() {
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
    push_cb: &Mutex<Vec<Box<PushCb>>>,
) -> LoopEnd {
    loop {
        tokio::select! {
            biased;
            cmd = cmd_rx.recv() => {
                match cmd {
                    None => return LoopEnd::ClientDropped,
                    Some(ClientCmd::Request { req, reply }) => {
                        let id = next_id.fetch_add(1, Ordering::Relaxed);
                        // Encode BEFORE touching the socket (finding [2]): an oversized-once-encoded
                        // request (e.g. an ~8.4 MiB write_stdin paste) is a purely LOCAL failure of
                        // THIS request — it must resolve only this request's oneshot and leave the
                        // connection (and every other in-flight/future request on it) completely
                        // untouched, never reach `pending`/be treated as a dead socket.
                        let bytes = match encode_request_frame(&Frame::Request { id, req }) {
                            Ok(bytes) => bytes,
                            Err(e) => {
                                tracing::warn!(error = %e, "request encode failed; failing only this request");
                                let _ = reply.send(Err(e));
                                continue;
                            }
                        };
                        lock(pending).insert(id, reply);
                        if let Err(e) = write_encoded_frame(stream, &bytes).await {
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
                        if let Some(tx) = lock(pending).remove(&id) {
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
/// reconnects with backoff (re-running the preamble handshake each time) until the client is
/// dropped or the daemon becomes permanently incompatible.
async fn connection_task(
    socket_path: PathBuf,
    client_build: String,
    mut stream: UnixStream,
    mut reader: FrameReader,
    mut cmd_rx: mpsc::Receiver<ClientCmd>,
    shared: Arc<SharedState>,
) {
    // NOTE: the *initial* `Connected`/`live=true` transition is seeded synchronously by
    // `DaemonClient::connect_at` before this task is even spawned (not fired here) — the
    // handshake is already done by the time `connect_at` constructs `shared`, so seeding there
    // closes the race where a caller's `request()` or `on_conn()` could otherwise run before this
    // task gets scheduled for the first time. This loop only fires transitions for *subsequent*
    // disconnect/reconnect events.
    // Starts at 1, not 0: the preamble handshake is no longer a `Frame::Request` at all (it's a
    // raw, codec-agnostic byte exchange completed before this task ever runs), so no id is truly
    // "reserved" anymore — this just keeps id 0 permanently unused as a harmless, easy-to-recognize
    // sentinel in logs/traces rather than repurposing it for real request correlation.
    let next_id = AtomicU64::new(1);
    let pending: Mutex<HashMap<u64, oneshot::Sender<Result<Response, ClientError>>>> =
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
        // `run_connection` returned: the connection is no longer serving requests, whether because
        // it dropped (`ConnectionLost`, about to reconnect) or the client is shutting down
        // (`ClientDropped`). Clear liveness *before* draining `pending` / firing `Disconnected` so
        // any `request()` racing this teardown never observes a stale `live == true`.
        shared.live.store(false, Ordering::Release);

        // Honest degradation (spec §13): every in-flight request fails now rather than hanging or
        // faking success, regardless of why the connection ended.
        for (_id, tx) in lock(&pending).drain() {
            let _ = tx.send(Err(ClientError::Disconnected));
        }

        if end == LoopEnd::ClientDropped {
            return;
        }

        fire_conn(&shared.conn_cb, ConnState::Disconnected);
        tracing::warn!("daemon connection lost; reconnecting");

        match connect_with_backoff(&socket_path, &client_build).await {
            Ok((s, r)) => {
                stream = s;
                reader = r;
                // Mark live again only once we're about to loop back into `run_connection` (i.e.
                // actually ready to serve `cmd_rx`), same rationale as the initial seed.
                shared.live.store(true, Ordering::Release);
                fire_conn(&shared.conn_cb, ConnState::Connected);
            }
            Err(HandshakeError::Incompatible { min, max }) => {
                tracing::error!(
                    min,
                    max,
                    client_max = bpa_protocol::preamble::CLIENT_MAX_VERSION,
                    "daemon became incompatible; giving up reconnect"
                );
                // Finding [11]: previously this branch only logged and returned, leaving the
                // client's slot holding a dead connection forever with no signal at all — the UI
                // was stuck showing "reconnecting..." with no way to reach the upgrade flow. Fire
                // the fatal transition BEFORE returning so the broker can map it to
                // `daemon://incompatible` (spec §6.2) exactly as it does for the initial-connect
                // path.
                fire_conn(
                    &shared.conn_cb,
                    ConnState::Incompatible {
                        daemon_min: min,
                        daemon_max: max,
                    },
                );
                return;
            }
            // connect_with_backoff only returns Err for a fatal Incompatible classification (a
            // genuine reply, or HANDSHAKE_SUSPECT_CAP consecutive transient failures escalated to
            // the same shape); Io and TransientHandshake both loop internally.
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

    // --- framing helpers reused by the stub daemon ---
    async fn read_frame(stream: &mut UnixStream) -> Option<Frame> {
        // Read exactly one length-prefixed CBOR frame via the shared FrameDecoder: read the
        // 4-byte LE length, then that many body bytes, feed both into the decoder, and take the
        // single frame it yields (mirrors the length-prefix framing `encode_frame` produces).
        let mut len_buf = [0u8; 4];
        stream.read_exact(&mut len_buf).await.ok()?;
        let len = u32::from_le_bytes(len_buf) as usize;
        let mut body = vec![0u8; len];
        stream.read_exact(&mut body).await.ok()?;
        let mut decoder = FrameDecoder::new();
        decoder.push(&len_buf);
        decoder.push(&body);
        decoder.decode().ok()?.into_iter().next()
    }

    async fn write_stub_frame(stream: &mut UnixStream, frame: &Frame) {
        let bytes = encode_frame(frame).unwrap();
        stream.write_all(&bytes).await.unwrap();
        stream.flush().await.unwrap();
    }

    /// Read and decode one client preamble off the wire (mirrors
    /// `bpa_sessiond::socket_server::read_client_preamble`'s test-stub shape): the fixed 10-byte
    /// header (`magic | min | max | build_len`), then exactly `build_len` more bytes.
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

    /// Read the client preamble then reply `Accepted{chosen:3, build:"stub"}` — the standard
    /// stub-daemon handshake every test in this module drives before exercising request/response
    /// behavior.
    async fn accept_handshake(stream: &mut UnixStream) {
        let _client_preamble = read_client_preamble_stub(stream).await;
        let reply = encode_daemon_reply(&DaemonReply::Accepted {
            chosen: 3,
            build: "stub".into(),
        });
        stream.write_all(&reply).await.unwrap();
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
            accept_handshake(&mut stream).await;

            let mut first_seen = false;
            loop {
                let Some(frame) = read_frame(&mut stream).await else {
                    break;
                };
                if let Frame::Request { id, req } = frame {
                    let res = match req {
                        Request::CreateWorkspace { name, root_path } => {
                            Response::Workspace(bpa_protocol::Workspace {
                                id: name.clone(),
                                name,
                                roots: vec![root_path.clone()],
                                root_path,
                            })
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
            c1.request(Request::CreateWorkspace {
                name: "one".into(),
                root_path: "/".into(),
            })
            .await
        });
        tokio::time::sleep(Duration::from_millis(20)).await;
        let c2 = client.clone();
        let h2 = tokio::spawn(async move {
            c2.request(Request::CreateWorkspace {
                name: "two".into(),
                root_path: "/".into(),
            })
            .await
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
            accept_handshake(&mut stream).await;
            if let Some(Frame::Request { id, .. }) = read_frame(&mut stream).await {
                write_stub_frame(
                    &mut stream,
                    &Frame::Response {
                        id,
                        res: Response::Error {
                            code: "NoSuchSession".into(),
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
            .request(Request::KillSession {
                session_id: "missing".into(),
            })
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
    async fn incompatible_daemon_reply_surfaces_typed_error() {
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
            ClientError::IncompatibleDaemon {
                daemon_min,
                daemon_max,
            } => {
                assert_eq!((daemon_min, daemon_max), (2, 2));
            }
            o => panic!("expected ClientError::IncompatibleDaemon, got {o:?}"),
        }
        // A version-incompatible daemon must be reported immediately, not after backoff retries.
        assert!(
            elapsed < Duration::from_secs(1),
            "expected a fast, non-retried failure, took {elapsed:?}"
        );
    }

    #[tokio::test]
    async fn daemon_closing_during_handshake_is_transient_disconnected_not_hang() {
        // Finding [1]: an EOF mid-handshake on the INITIAL connect is ambiguous (it's exactly what
        // a booting-but-compatible v2 daemon looks like mid cold-rehydrate) — it must map to the
        // retryable `Disconnected`, not an instant fatal `IncompatibleDaemon`/false upgrade dialog.
        // It must still not hang past PREAMBLE_TIMEOUT.
        let path = tmp_sock();
        let ready = Arc::new(AtomicBool::new(false));
        let p = path.clone();
        let r = ready.clone();
        tokio::spawn(async move {
            let listener = UnixListener::bind(&p).unwrap();
            r.store(true, Ordering::SeqCst);
            // Accept the connection, read the client's preamble, then close without replying —
            // the client must not hang waiting for a reply that will never come; it must give up
            // by PREAMBLE_TIMEOUT (or sooner, on the EOF) and surface a retryable Disconnected.
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
            matches!(err, ClientError::Disconnected),
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
                write_stub_frame(
                    &mut s,
                    &Frame::Push(Push::WorkspaceCreated {
                        workspace: bpa_protocol::Workspace {
                            id: "w1".into(),
                            name: "w1".into(),
                            root_path: "/".into(),
                            roots: vec!["/".into()],
                        },
                    }),
                )
                .await;
                // drop `s` -> connection closes
            }
            // connection #2
            {
                let (mut s, _) = listener.accept().await.unwrap();
                accept_handshake(&mut s).await; // handshake again
                if let Some(Frame::Request { id, .. }) = read_frame(&mut s).await {
                    write_stub_frame(
                        &mut s,
                        &Frame::Response {
                            id,
                            res: Response::Ack,
                        },
                    )
                    .await;
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
            match client
                .request(Request::KillSession {
                    session_id: "x".into(),
                })
                .await
            {
                Ok(Response::Ack) => {
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
            .any(|p| matches!(p, Push::WorkspaceCreated { .. }));
        assert!(got_push, "expected the WorkspaceCreated push");

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
            .request(Request::KillSession {
                session_id: "x".into(),
            })
            .await
            .unwrap_err();
        assert!(
            matches!(err, ClientError::Disconnected),
            "expected Disconnected, got {err:?}"
        );
    }

    // ---- Finding [2]: an oversized-once-encoded request must fail ONLY itself, never the
    // ---- connection — a subsequent normal request on the same connection must still succeed. ----

    #[tokio::test]
    async fn oversized_request_fails_itself_only_connection_stays_alive() {
        let path = tmp_sock();
        let ready = Arc::new(AtomicBool::new(false));
        spawn_stub(path.clone(), ready.clone());
        wait_ready(&ready).await;

        let client = connect_at(path, "test".into()).await.unwrap();

        // ~8.5 MB of bytes all >= 0x18 (ASCII 'A' = 65): CBOR encodes each as a 2-byte item, so
        // this comfortably exceeds bpa_protocol::MAX_FRAME_LEN (16 MiB) once encoded — mirrors the
        // real-world "~8.4 MB paste" scenario from finding [2].
        let oversized = vec![b'A'; 8_500_000];
        let err = client
            .request(Request::WriteStdin {
                session_id: "s".into(),
                bytes: oversized,
            })
            .await
            .unwrap_err();
        assert!(
            matches!(err, ClientError::RequestTooLarge { .. }),
            "expected RequestTooLarge, got {err:?}"
        );

        // The connection must still be alive: a normal request right after must succeed, with NO
        // reconnect/disconnect cycle in between (this stub only ever accepts ONE connection —
        // spawn_stub's `listener.accept().await` is called exactly once — so success here already
        // proves the same connection served both requests).
        let ok = client
            .request(Request::KillSession {
                session_id: "x".into(),
            })
            .await
            .expect("a normal request on the same connection must still succeed");
        assert!(matches!(ok, Response::Ack));
    }

    /// Test-only alias: exercises `DaemonClient`'s private `connect_at` directly (bypassing
    /// `resolve_socket_path()`) so the stub daemon can bind an arbitrary tempdir path. Same
    /// codepath `connect()` uses in production.
    async fn connect_at(
        socket_path: PathBuf,
        client_build: String,
    ) -> Result<DaemonClient, ClientError> {
        DaemonClient::connect_at(socket_path, client_build).await
    }

    /// Test-only alias: exercises `DaemonClient`'s private `connect_at_with_retry` directly
    /// (bypassing `resolve_socket_path()`), mirroring `connect_at` above. Same codepath
    /// `lib.rs`'s `connect_with_retry`/`bring_up_daemon` uses in production for the INITIAL connect
    /// (round-2 regression R1).
    async fn connect_at_with_retry(
        socket_path: PathBuf,
        client_build: String,
        attempts: u32,
        delay: Duration,
    ) -> Result<DaemonClient, ClientError> {
        DaemonClient::connect_at_with_retry(socket_path, client_build, attempts, delay).await
    }

    // ---- Defect 1: request() must fail promptly (not silently queue) during the reconnect gap ---

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
            client.request(Request::KillSession {
                session_id: "x".into(),
            }),
        )
        .await;
        let elapsed = started.elapsed();

        let err = result
            .unwrap_or_else(|_| panic!("request() did not return within 1s (took > {elapsed:?}); it silently queued during the reconnect gap"))
            .unwrap_err();
        assert!(
            matches!(err, ClientError::Disconnected),
            "expected Disconnected, got {err:?}"
        );
        assert!(
            elapsed < Duration::from_secs(1),
            "request() took {elapsed:?} to fail; expected a prompt failure, not a queued/timed-out one"
        );
    }

    // ---- Defect 2: a late-registered on_conn callback must observe the current ConnState -------

    #[tokio::test]
    async fn late_registered_on_conn_observes_current_connected_state() {
        let path = tmp_sock();
        let ready = Arc::new(AtomicBool::new(false));
        spawn_stub(path.clone(), ready.clone());
        wait_ready(&ready).await;

        let client = connect_at(path, "test".into()).await.unwrap();

        // Force the race the review flagged: yield back to the scheduler (and give the connection
        // task a moment to run) *before* registering on_conn, so the initial `fire_conn(Connected)`
        // in `connection_task` has every opportunity to have already fired and been missed by a
        // callback that isn't registered yet.
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

    // ---- Finding [1]/[11]: transient handshake failures (EOF/timeout/garbage/bad-magic) must be
    // ---- retried up to HANDSHAKE_SUSPECT_CAP, and only escalate to a fatal, mid-session
    // ---- ConnState::Incompatible once the cap is actually exhausted. -----------------------------

    /// Accept a connection, read the client's preamble, then close WITHOUT replying — this is the
    /// "transient handshake failure" shape (EOF mid-handshake): exactly what a v2 daemon still
    /// inside `cold_rehydrate_sessions` looks like from the outside, and also exactly what a
    /// genuine v1 daemon (no codec-agnostic preamble reader) looks like.
    async fn accept_and_eof(listener: &UnixListener) {
        let (mut s, _) = listener.accept().await.unwrap();
        let _client_preamble = read_client_preamble_stub(&mut s).await;
        drop(s);
    }

    #[tokio::test]
    async fn transient_handshake_failures_below_cap_eventually_connect_with_no_incompatible_event()
    {
        // N < HANDSHAKE_SUSPECT_CAP EOFs on RECONNECT attempts, then a real handshake succeeds.
        // The client must reconnect cleanly with no IncompatibleDaemon anywhere in the process and
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
                if let Some(Frame::Request { id, .. }) = read_frame(&mut s).await {
                    write_stub_frame(
                        &mut s,
                        &Frame::Response {
                            id,
                            res: Response::Ack,
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

        // Retry KillSession until it succeeds against the eventually-recovered connection —
        // proves the client reconnected fine despite the 3 transient EOFs in between.
        let mut got_ack = false;
        for _ in 0..200 {
            match client
                .request(Request::KillSession {
                    session_id: "x".into(),
                })
                .await
            {
                Ok(Response::Ack) => {
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
    async fn transient_handshake_failures_exceeding_cap_surface_incompatible_daemon() {
        // The daemon ALWAYS EOFs mid-handshake, forever — a stand-in for a genuine, permanently
        // incompatible v1 daemon that can never complete the v2 preamble. After
        // HANDSHAKE_SUSPECT_CAP consecutive transient failures, the reconnect loop must give up
        // and fire ConnState::Incompatible{0,0} (unknown range).
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
            .request(Request::KillSession {
                session_id: "x".into(),
            })
            .await
            .unwrap_err();
        assert!(matches!(err, ClientError::Disconnected));
    }

    // ---- Round-2 regression R1 (CRITICAL): the INITIAL connect path (`connect_at_with_retry`,
    // ---- what `lib.rs`'s `connect_with_retry`/`bring_up_daemon` calls at app boot) must apply the
    // ---- IDENTICAL HANDSHAKE_SUSPECT_CAP escalation the reconnect loop already had — this is the
    // ---- dominant upgrade scenario: a present-but-unhandshakeable OLD daemon (e.g. a v1 daemon
    // ---- still running under launchd after installing a v2 app build) that accepts every TCP
    // ---- connection but EOFs on every v2 preamble, forever. Pre-fix, `connect_with_retry` mapped
    // ---- every such failure to `ClientError::Disconnected` and retried "daemon unreachable"
    // ---- forever — `daemon://incompatible` never fired and the upgrade dialog was unreachable. ----

    #[tokio::test]
    async fn initial_connect_escalates_to_incompatible_after_cap_exhausted_transient_failures() {
        // The daemon ALWAYS EOFs mid-handshake, forever, from the very FIRST connection — no live
        // connection ever existed to "reconnect" from; this is the initial-connect path, not the
        // reconnect loop. After HANDSHAKE_SUSPECT_CAP consecutive transient failures,
        // `connect_at_with_retry` must give up and return `IncompatibleDaemon{0,0}` (the same
        // unknown-range sentinel the reconnect loop uses), never an infinite `Disconnected` retry.
        let path = tmp_sock();
        let ready = Arc::new(AtomicBool::new(false));
        let p = path.clone();
        let r = ready.clone();
        tokio::spawn(async move {
            let listener = UnixListener::bind(&p).unwrap();
            r.store(true, Ordering::SeqCst);
            loop {
                accept_and_eof(&listener).await;
            }
        });
        wait_ready(&ready).await;

        // HANDSHAKE_SUSPECT_CAP is 8; give it exactly 8 attempts with a tiny delay so the test is
        // fast and deterministic (mirrors bring_up_daemon's real 8-attempts config, just with a
        // millisecond delay instead of 500ms).
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
            Err(ClientError::IncompatibleDaemon {
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
                "expected IncompatibleDaemon{{0,0}} after {HANDSHAKE_SUSPECT_CAP} consecutive \
                 transient failures on the INITIAL connect, got {other:?}"
            ),
        }
        assert!(
            elapsed < Duration::from_secs(5),
            "connect_at_with_retry took {elapsed:?}; expected a prompt escalation once the cap \
             is exhausted, not a hang"
        );
    }

    #[tokio::test]
    async fn initial_connect_refused_never_escalates_within_the_cap() {
        // Plain connect-refused (nothing listening at all — no UnixListener bound anywhere near
        // this path) is a completely different failure mode from a transient handshake failure: it
        // must keep retrying as plain "daemon not up yet" and NEVER escalate to Incompatible, even
        // across a full HANDSHAKE_SUSPECT_CAP-sized attempt budget (mirrors
        // `connect_with_retry_gives_up_after_bounded_attempts_without_panicking`'s existing
        // give-up-cleanly coverage, but proves the OUTCOME is specifically Disconnected, not
        // IncompatibleDaemon).
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
            matches!(result, Err(ClientError::Disconnected)),
            "connect-refused must stay Disconnected, never escalate to Incompatible: {result:?}"
        );
        assert!(
            elapsed < Duration::from_secs(5),
            "connect_at_with_retry took {elapsed:?}; expected a prompt bounded give-up"
        );
    }

    #[tokio::test]
    async fn initial_connect_transient_failures_below_cap_still_eventually_connect() {
        // N < HANDSHAKE_SUSPECT_CAP EOFs, then a real handshake succeeds — mirrors
        // `transient_handshake_failures_below_cap_eventually_connect_with_no_incompatible_event`
        // but for the INITIAL connect path: proves the escalation counter doesn't fire a false
        // positive before the cap is actually exhausted.
        let path = tmp_sock();
        let ready = Arc::new(AtomicBool::new(false));
        let p = path.clone();
        let r = ready.clone();
        tokio::spawn(async move {
            let listener = UnixListener::bind(&p).unwrap();
            r.store(true, Ordering::SeqCst);
            for _ in 0..3 {
                accept_and_eof(&listener).await;
            }
            let (mut s, _) = listener.accept().await.unwrap();
            accept_handshake(&mut s).await;
            if let Some(Frame::Request { id, .. }) = read_frame(&mut s).await {
                write_stub_frame(
                    &mut s,
                    &Frame::Response {
                        id,
                        res: Response::Ack,
                    },
                )
                .await;
            }
            std::future::pending::<()>().await;
        });
        wait_ready(&ready).await;

        let client = connect_at_with_retry(
            path,
            "test".into(),
            HANDSHAKE_SUSPECT_CAP,
            Duration::from_millis(5),
        )
        .await
        .expect("must connect once the real handshake attempt is reached, well under the cap");

        let ok = client
            .request(Request::KillSession {
                session_id: "x".into(),
            })
            .await
            .expect("a normal request on the connected client must succeed");
        assert!(matches!(ok, Response::Ack));
    }

    #[tokio::test]
    async fn genuine_incompatible_reply_on_initial_connect_is_immediately_fatal_no_retry() {
        // A GENUINE, well-formed Incompatible{5,6} reply on the very FIRST connect attempt must
        // remain immediately fatal with the real advertised range — no retry at all, exactly like
        // `connect_with_retry_does_not_retry_incompatible` in lib.rs already covers end-to-end via
        // `bring_up_daemon`'s real config; this proves the same at the `connect_at_with_retry`
        // level directly, with a bound tight enough to prove NO retry happened (not just "returned
        // within a generous ceiling").
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
            // A second accept here would prove a retry happened; the test's wall-clock bound below
            // catches that without needing to assert on it directly.
            std::future::pending::<()>().await;
        });
        wait_ready(&ready).await;

        let started = std::time::Instant::now();
        // 8 attempts x 500ms (bring_up_daemon's real config) would take ~3.5s if this incorrectly
        // retried; a prompt return well under that bound proves no retry happened.
        let result =
            connect_at_with_retry(path, "test".into(), 8, Duration::from_millis(500)).await;
        let elapsed = started.elapsed();

        match result {
            Err(ClientError::IncompatibleDaemon {
                daemon_min,
                daemon_max,
            }) => assert_eq!((daemon_min, daemon_max), (5, 6)),
            other => panic!("expected IncompatibleDaemon{{5,6}}, got {other:?}"),
        }
        assert!(
            elapsed < Duration::from_secs(1),
            "connect_at_with_retry took {elapsed:?}; a genuine Incompatible reply must return \
             promptly with no retry, not run the full ~3.5s of bounded backoff"
        );
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
