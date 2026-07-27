//! Hop-B socket server (spec §7, §8.2, §11, §13, §16): a tokio `UnixListener` accept loop with
//! one task per connected client, the codec-agnostic preamble handshake + version negotiation gate
//! (Pv2 §4.2/§4.4), request/response correlation, a bounded per-client outbound queue (overflow ⇒
//! drop+disconnect), peer-cred refusal of foreign euids, supervisor→`Push` fan-out to every
//! connected client, DB persistence, and daemon-side path validation.
//!
//! ## Handshake (Pv2 §4.2/§4.4)
//!
//! The very first bytes on every connection are a fixed, codec-independent preamble — not a CBOR
//! frame — so a version-incompatible peer can always be told so, even one that cannot decode this
//! daemon's CBOR at all (the failure mode `Request::Hello`-as-a-CBOR-frame had in v1). `handle_client`
//! delegates the whole handshake to `bpa_daemon_core::handshake::server_handshake` (S3 phase 1
//! extraction, spec §3 — moved out of this module verbatim): it reads the client's `[min, max]` +
//! build string within `bpa_protocol::PREAMBLE_TIMEOUT`, negotiates a version via
//! `bpa_protocol::negotiate`, and writes `Accepted`/`Incompatible` before this module ever touches
//! the CBOR frame dispatch loop below. A stuck, silent, or garbage-writing peer is closed once the
//! timeout elapses rather than left to hang the connection task indefinitely.
//!
//! ## What this module owns vs. what Task 13 owns
//!
//! [`serve`] is the pure server entry point: given an already-bound [`UnixListener`], a
//! [`ServerDeps`] bundle (supervisor / db / attach / build string), and a `watch` shutdown
//! receiver, it runs the accept loop until `shutdown` flips to `true` or the listener errors, then
//! returns. Task 13 (daemon boot) owns process concerns around it: the `flock` single-instance
//! lock, socket-dir permissions, `bind` + stale-socket unlink, SIGTERM handling, and the post-`serve`
//! drain (supervisor killpg, DB checkpoint, socket unlink). `serve` never touches those — it is
//! drivable in isolation by tests. There are now TWO triggers for the one shutdown `watch`: `main.rs`'s
//! SIGTERM/SIGINT handler, and `Request::DaemonShutdown`'s dispatch arm below (Pv2 §6.1) — both hold
//! a clone of the same `watch::Sender` (`ServerDeps::shutdown_tx`), so a GUI-initiated shutdown and
//! an operator signal converge on the identical graceful-exit path.
//!
//! ## Framing (spec §7)
//!
//! Every wire message is `u32`-LE length + `CBOR(Frame)`. We reuse the protocol crate's own
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
use tokio::sync::{mpsc, watch, Mutex, Notify};

use crate::attach::{AttachError, AttachRegistry, PushSink};
use crate::persistence::Db;
use crate::pty_supervisor::{SessionSpec, StatusUpdate, Supervisor, SupervisorError};
use crate::shell_integration::{classify_shell, write_session_assets};
use crate::singleton::check_peer_cred;

use bpa_protocol::sync::lock;
use bpa_protocol::{
    encode_frame, Frame, FrameDecoder, Push, Request, Response, SessionMeta, Workspace,
    DAEMON_MAX_VERSION, DAEMON_MIN_VERSION,
};

/// Per-client bounded outbound queue depth (frames). Overflow (a client that stopped reading) ⇒
/// drop + disconnect that client rather than buffer unboundedly (spec §13, no memory-DoS).
pub const CLIENT_OUTQ_CAP: usize = 1024;

/// Bound on how long connection cleanup waits for the writer task to notice its queue is closed
/// and exit on its own, before forcibly aborting it (spec §13). A client that stopped reading (the
/// overflow case this queue exists to handle) can leave the writer's own `write_all` blocked
/// indefinitely on a full kernel socket send buffer that will never drain — waiting on that task
/// unboundedly here would re-introduce the exact hang spec §13's bounded queue was meant to avoid,
/// just one step later in the teardown path. 200 ms is generous for the ordinary case (queue
/// closed, in-flight write already completed) while keeping a stuck client's forced disconnect fast.
const WRITER_JOIN_TIMEOUT: std::time::Duration = std::time::Duration::from_millis(200);

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
    /// Human-readable daemon build string echoed in the accepted preamble reply (Pv2 §4.2/§4.4).
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
    /// Ids of workspaces with an in-flight `RemoveWorkspace` (SES-1, audit 2026-07-24, probe
    /// p5). Set right after the removal's existence check and held (via an RAII guard) until
    /// the delete transaction AND the post-delete stray sweep complete. `CreateSession` rejects
    /// any id in this set with the same typed `NoSuchWorkspace` error it returns for a
    /// never-existent id (SES-4), so a create can no longer slip between the removal's victim
    /// sweep and its delete tx and survive as an orphaned live shell inside a deleted
    /// workspace. `std::sync::Mutex` like `live_sessions`: an instant insert/contains/remove,
    /// never held across an `.await`.
    closing_workspaces: std::sync::Mutex<std::collections::HashSet<bpa_protocol::WorkspaceId>>,
    /// The SAME `watch::Sender` whose receiver drives [`serve`]'s accept loop (and every connected
    /// client's dispatch loop, and the scrollback flusher). `Request::DaemonShutdown` is the only
    /// dispatch arm that fires this (spec §6.1): flipping it to `true` is exactly the SIGTERM path
    /// (`main.rs`'s signal watcher flips the same channel), so a GUI-initiated shutdown and an
    /// operator SIGTERM converge on one graceful-exit mechanism rather than two. Cloned into
    /// `ServerDeps` (not moved) because `main.rs`/`boot::run` also needs the receiver half.
    pub shutdown_tx: watch::Sender<bool>,
    /// Round-3 hardening (H1): `JoinHandle`s for every in-flight `on_exited` final-flush task
    /// (`flush_session_final`, spawned by `install_push_callbacks`'s `on_exited` closure via
    /// `rt_handle.spawn`). A session killed mid-session (natural exit, or an explicit
    /// `KillSession`) has its final flush scheduled as a DETACHED task — fine while the tokio
    /// runtime keeps running, because nothing needs to wait for it. But at CLEAN shutdown,
    /// `boot::run` calls `supervisor.shutdown_all()` (which synchronously kills every still-live
    /// session, and each kill's `on_exited` callback schedules exactly this kind of task) and then
    /// immediately returns and lets `#[tokio::main]` drop the runtime — which does NOT await
    /// detached tasks. Without tracking, that final scrollback tail and the terminal `Exited`
    /// lifecycle for every session killed by shutdown can be silently discarded. `boot::run` drains
    /// this vec (via [`ServerDeps::await_pending_final_flushes`]) AFTER `shutdown_all()` schedules
    /// them and BEFORE `db.checkpoint()`, awaiting each one so the write is guaranteed to have
    /// landed before the process exits. `std::sync::Mutex` (not `tokio::sync::Mutex`): only ever
    /// touched for the instant it takes to push/drain a `Vec`, never held across an `.await`.
    pending_final_flushes: std::sync::Mutex<Vec<tokio::task::JoinHandle<()>>>,
}

impl ServerDeps {
    /// Construct a [`ServerDeps`] with an empty live-session tracker.
    pub fn new(
        supervisor: Arc<Supervisor>,
        db: Arc<Mutex<Db>>,
        attach: Arc<AttachRegistry>,
        daemon_build: String,
        runtime_root: std::path::PathBuf,
        shutdown_tx: watch::Sender<bool>,
    ) -> Self {
        ServerDeps {
            supervisor,
            db,
            attach,
            daemon_build,
            runtime_root,
            live_sessions: std::sync::Mutex::new(std::collections::HashSet::new()),
            closing_workspaces: std::sync::Mutex::new(std::collections::HashSet::new()),
            shutdown_tx,
            pending_final_flushes: std::sync::Mutex::new(Vec::new()),
        }
    }

    /// Track a spawned final-flush task so a later clean shutdown can await it (H1). Called only
    /// from the `on_exited` closure in `install_push_callbacks`, right after `rt_handle.spawn`.
    ///
    /// Sweeps already-finished handles first (same no-unbounded-growth bar as the rest of this
    /// round): a mid-life exit's flush completes within milliseconds and nothing else ever drains
    /// the vec until shutdown, so without this sweep a days-long daemon with create/kill churn
    /// would accumulate one dead `JoinHandle` per session-ever-exited. `JoinHandle::is_finished`
    /// is a cheap atomic read; retaining an in-flight handle (the only kind shutdown actually
    /// needs) is exactly the awaited-drain contract.
    fn track_final_flush(&self, handle: tokio::task::JoinHandle<()>) {
        let mut pending = lock(&self.pending_final_flushes);
        pending.retain(|h| !h.is_finished());
        pending.push(handle);
    }

    /// Current number of tracked, not-yet-collected final-flush handles (after the same
    /// finished-handle sweep `track_final_flush` performs) — test observability for the
    /// no-unbounded-growth contract.
    #[cfg(test)]
    pub(crate) fn pending_final_flush_count(&self) -> usize {
        let mut pending = lock(&self.pending_final_flushes);
        pending.retain(|h| !h.is_finished());
        pending.len()
    }

    /// Drain and await every final-flush task scheduled so far (H1): called by `boot::run` after
    /// `supervisor.shutdown_all()` (which synchronously kills every still-live session and, via
    /// each kill's `on_exited` callback, schedules that session's final flush onto this vec) and
    /// BEFORE `db.checkpoint()` — guaranteeing every session's terminal scrollback + `Exited`
    /// lifecycle is durably written before the process exits, without relying on any detached task
    /// surviving the `#[tokio::main]` runtime drop. A task that panicked is logged and otherwise
    /// ignored (`flush_session_final` itself never panics on a DB error — it logs and swallows —
    /// so a `JoinError` here would only ever come from a genuine bug, not an expected failure mode);
    /// this must never propagate a panic into the shutdown path itself.
    pub async fn await_pending_final_flushes(&self) {
        let handles: Vec<_> = lock(&self.pending_final_flushes).drain(..).collect();
        for handle in handles {
            if let Err(e) = handle.await {
                tracing::warn!(error = %e, "final flush task panicked during shutdown drain");
            }
        }
    }

    /// `true` while `workspace_id` has an in-flight `RemoveWorkspace` (SES-1) — the gate
    /// `CreateSession` consults before spawning (see the field's doc).
    fn is_workspace_closing(&self, workspace_id: &bpa_protocol::WorkspaceId) -> bool {
        lock(&self.closing_workspaces).contains(workspace_id)
    }

    /// Mark `workspace_id` as closing (SES-1) and return the RAII guard that unmarks it on
    /// drop — so EVERY exit path of the `RemoveWorkspace` dispatch arm (success, a teardown
    /// error, a delete-tx error, an early not-found return is impossible here because the
    /// existence check runs before this is called) releases the gate.
    fn begin_closing_workspace(
        &self,
        workspace_id: &bpa_protocol::WorkspaceId,
    ) -> ClosingWorkspaceGuard<'_> {
        lock(&self.closing_workspaces).insert(workspace_id.clone());
        ClosingWorkspaceGuard {
            deps: self,
            workspace_id: workspace_id.clone(),
        }
    }
}

/// RAII guard for [`ServerDeps::closing_workspaces`] (SES-1): removes the id from the closing
/// set on drop. Never panics on a poisoned mutex (poison-safe cleanup, same pattern as
/// `pending_final_flushes`).
struct ClosingWorkspaceGuard<'a> {
    deps: &'a ServerDeps,
    workspace_id: bpa_protocol::WorkspaceId,
}

impl Drop for ClosingWorkspaceGuard<'_> {
    fn drop(&mut self) {
        lock(&self.deps.closing_workspaces).remove(&self.workspace_id);
    }
}

/// Registry of every connected client's outbound queue, so supervisor callbacks can fan a single
/// `Push` out to all of them (spec §7: `SessionCreated` / `StateChanged` / `ChildExited` reach
/// every client). Each client registers on connect and deregisters on disconnect. Sends use
/// `try_send`; a full/closed queue is silently skipped here (the owning client task independently
/// detects overflow on its own reply path and tears itself down) so one dead client never blocks
/// the fan-out to the others.
///
/// Re-seated (S3 phase 1, spec §3) onto `bpa_daemon_core::broadcast::Broadcaster<F>` — the
/// generic extraction of this exact type (was `socket_server.rs:211-231`). `Frame` is the fanned
/// -out value here, so every call site now wraps its `Push` in `Frame::Push(..)` before calling
/// `.broadcast(..)` — the bytes ultimately enqueued on each client's outbound queue are
/// byte-identical to before the re-seat.
type Broadcaster = bpa_daemon_core::broadcast::Broadcaster<Frame>;

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
    install_push_callbacks(&deps, broadcaster.clone());

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
/// Takes the whole `deps` bundle (rather than separate `supervisor`/`attach` params) because the
/// `on_exited` closure below also needs `deps.db` for the round-2-regression (R2) final flush.
///
/// The `attach` registry is used so a session that ends drops its attach entry (orphan cleanup)
/// before the `ChildExited` push is built — keeping the registry from growing unbounded across
/// create/kill churn. The drop is GRACEFUL (`remove_session` removes the map entry without
/// cancelling the forwarder): the reader thread closes the sink on its own exit, so the forwarder
/// delivers the session's trailing output and then self-terminates rather than being truncated.
///
/// The `on_exited` closure holds a `Weak<AttachRegistry>` and a `Weak<ServerDeps>`, NOT strong
/// `Arc`s: `deps.supervisor` owns this callback (directly, and transitively via `deps.attach`), so a
/// strong reference back to either from here would form a reference cycle that leaks the supervisor
/// (and its live PTY sessions) forever — the same rationale that already applied to `attach` before
/// this fix, now extended to `deps`. Upgrading each `Weak` per call is cheap and yields `None` only
/// once the daemon is already tearing down (nothing left to clean up/flush).
fn install_push_callbacks(deps: &Arc<ServerDeps>, broadcaster: Broadcaster) {
    let supervisor = &deps.supervisor;
    let attach = &deps.attach;

    let b_created = broadcaster.clone();
    supervisor.on_created(move |meta: SessionMeta| {
        b_created.broadcast(Frame::Push(Push::SessionCreated { meta }));
    });

    let b_status = broadcaster.clone();
    supervisor.on_status(move |u: StatusUpdate| {
        b_status.broadcast(Frame::Push(Push::StateChanged {
            session_id: u.session_id,
            lifecycle: u.lifecycle,
            waiting_for_input: u.waiting_for_input,
            cwd: u.cwd,
        }));
    });

    let b_exited = broadcaster;
    let attach_exited = Arc::downgrade(attach);
    let deps_exited = Arc::downgrade(deps);
    // `on_exited` fires on the supervisor's WAIT THREAD (a plain `std::thread`, see
    // `pty_supervisor.rs`'s module-level threading contract) — not a tokio task — so the final flush
    // (an async DB write) cannot be `.await`ed inline here. Capture the current runtime `Handle` once,
    // at registration time (this function itself always runs from inside a tokio context: `serve`'s
    // async body, or a test's `#[tokio::test]`), and use it to spawn the flush as its own task from
    // inside the sync callback.
    let rt_handle = tokio::runtime::Handle::current();
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
        //
        // H1 (round-3 hardening): the final-flush task spawned below is still detached in the
        // ordinary mid-session case (nothing needs to wait for it while the runtime keeps running),
        // but its `JoinHandle` is now ALSO pushed onto `deps.pending_final_flushes` so a clean
        // shutdown (`boot::run`, after `supervisor.shutdown_all()`) can await every flush scheduled
        // by the kills that shutdown just triggered, before checkpointing and exiting — closing the
        // window where `#[tokio::main]` dropping the runtime could silently discard a still-pending
        // detached task. See `ServerDeps::await_pending_final_flushes`.
        if let Some(attach) = attach_exited.upgrade() {
            let _ = attach.remove_session(&session_id);
        }

        // Round-2 regression R2: a session's FINAL scrollback tail + terminal `Exited` lifecycle
        // must be persisted exactly once, right here, before it stops being covered by the
        // live-only periodic flush sweep (`flush_scrollback_once` skips any `!is_active` session,
        // and by the time this callback runs the wait thread has already flipped `is_active` to
        // `false` — see that field's write, above this callback in `pty_supervisor.rs`). Without
        // this, a session that exits between two flush ticks (or lives under one whole
        // `SCROLLBACK_FLUSH_INTERVAL`) loses its trailing output and is rehydrated on the next boot
        // still reporting a live lifecycle. `on_exited` firing at all is itself the "exited THIS
        // process lifetime" signal (a cold-rehydrated session from `boot::cold_rehydrate_sessions`/
        // `rehydrate_inactive` has no wait thread and so can never reach this callback) — no
        // additional liveness check is needed to distinguish it from the periodic sweep's
        // rehydrated-inactive skip, which stays untouched.
        //
        // CAPTURE SYNCHRONOUSLY, HERE, before spawning: `KillSession`'s dispatch arm calls
        // `Supervisor::kill`, which joins this exact wait thread (blocking on it) and then, the
        // MOMENT it unblocks, removes the session from the supervisor's live map — a race between
        // that removal and an async task merely SCHEDULED (not yet run) by `rt_handle.spawn` below
        // would make `supervisor.meta`/`snapshot_scrollback` from inside the spawned task
        // nondeterministically fail with `NoSuchSession`, silently dropping exactly the data this
        // fix exists to save. `meta()`/`snapshot_scrollback()`/`drain_command_events()` are all
        // cheap, synchronous, mutex-guarded reads — safe to call directly on this thread while the
        // session is still guaranteed present in the map (removal happens strictly after this
        // callback returns, both for `kill()`'s join and for a natural exit, which never removes
        // the map entry at all until a later reap).
        let snapshot = deps_exited.upgrade().and_then(|deps| {
            let meta = deps.supervisor.meta(&session_id).ok()?;
            let scrollback = deps
                .supervisor
                .snapshot_scrollback(&session_id)
                .ok()
                .map(|(_c, _r, bytes)| bytes);
            let events = deps.supervisor.drain_command_events(&session_id);
            Some((deps, meta, scrollback, events))
        });

        if let Some((deps, meta, scrollback, events)) = snapshot {
            // `on_exited` is an `Fn` (may be called again for a future session), so each invocation
            // clones its own handle to the broadcaster rather than moving the shared one out of the
            // closure's environment.
            let b_exited = b_exited.clone();
            let deps_for_track = deps.clone();
            let handle = rt_handle.spawn(async move {
                flush_session_final(&deps, &session_id, meta, scrollback, events).await;
                b_exited.broadcast(Frame::Push(Push::ChildExited {
                    session_id,
                    code,
                    signal,
                }));
            });
            // H1: track this handle so a clean shutdown can await it (see the doc comment above and
            // `ServerDeps::await_pending_final_flushes`). Registering AFTER `spawn()` returns is
            // safe — `spawn()` itself is synchronous (it only schedules the task onto the runtime;
            // the task body has not necessarily run yet), so there is no window where a concurrent
            // `await_pending_final_flushes` drain could run between the spawn and this push and miss
            // the handle: both this callback and any drain caller execute on the SAME tokio runtime,
            // and `boot::run` only calls the drain strictly after `shutdown_all()` (which triggers
            // this very callback synchronously, via `kill()`'s wait-thread join) has returned.
            deps_for_track.track_final_flush(handle);
        }
    });
}

/// One-shot final persist for a session that just exited THIS process lifetime (round-2 regression
/// R2): persists the given, already-captured meta snapshot (terminal `Exited{code,signal}`
/// lifecycle + final cwd/cols/rows — by the time `on_exited` fires the wait thread has already
/// written all of these into the supervisor's in-memory state) and scrollback ring snapshot, then
/// the given drained `command_events`. `meta`/`scrollback`/`events` are captured SYNCHRONOUSLY by
/// the caller (`install_push_callbacks`'s `on_exited` closure) before this async fn is even spawned
/// — see that call site's doc comment for why: reading them here, after an `.await` hop onto the
/// tokio scheduler, would race a concurrent `Supervisor::kill`'s post-join map removal and could
/// silently lose the very data this fix exists to save. Reuses the exact same
/// `upsert_session`/`append_scrollback`/`append_command_event` calls the periodic sweep
/// (`flush_scrollback_once`) uses — no new DB-writer pattern — but, unlike that sweep, does NOT gate
/// on `is_active` (the caller already captured the final in-memory state regardless of it).
/// Idempotent with any prior periodic tick: `upsert_session` is a plain upsert and
/// `append_scrollback` always replaces the row at `seq = 0` (never appends a second row), so calling
/// this after (or racing) a flush tick for the same session converges on the same final state rather
/// than duplicating anything. A DB failure here is logged and swallowed, exactly like the periodic
/// sweep — persistence is best-effort and must never propagate into (or block) the exit-notification
/// path.
async fn flush_session_final(
    deps: &Arc<ServerDeps>,
    session_id: &bpa_protocol::SessionId,
    meta: SessionMeta,
    scrollback: Option<Vec<u8>>,
    events: Vec<crate::pty_supervisor::CommandEvent>,
) {
    {
        let db = deps.db.lock().await;
        if let Err(e) = db.upsert_session(&meta) {
            tracing::debug!(session = %session_id, error = %e, "final flush: meta persist failed");
        }
    }

    if let Some(bytes) = scrollback {
        let ts = now_secs();
        let db = deps.db.lock().await;
        if let Err(e) = db.append_scrollback(session_id, 0, &bytes, ts) {
            tracing::debug!(session = %session_id, error = %e, "final flush: scrollback append failed");
        }
    }

    if !events.is_empty() {
        let db = deps.db.lock().await;
        for ev in events {
            if let Err(e) = db.append_command_event(
                session_id,
                ev.seq as i64,
                ev.ts,
                ev.kind,
                ev.exit_code,
                "gui",
            ) {
                tracing::debug!(
                    session = %session_id, error = %e, "final flush: command_events append failed"
                );
            }
        }
    }
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

/// One persistence sweep: for every session known to the DB that is STILL LIVE in the supervisor
/// (`supervisor.meta(id).is_active`), persist its current meta (cols/rows/cwd/lifecycle — spec
/// §11: resize/cwd/lifecycle changes are otherwise only ever mutated in-memory, so without this a
/// restart would rehydrate stale create-time columns), snapshot its scrollback and replace the
/// stored blob, and drain + persist any accumulated best-effort `command_events` (schema v2, spec
/// §7, Pv2 `origin` amendment). A cold-rehydrated or exited-but-unreaped (inactive) session is
/// SKIPPED entirely: its persisted rows are already the final/immutable copy (no live reader ever
/// changes them again), so re-upserting them every tick would be pure write amplification with no
/// data ever changing — across daemon restarts, N accumulated dead rehydrated sessions would
/// otherwise mean N needless SQLite writes every second, forever (D1). Best-effort; logs and
/// continues on error — a DB failure here must never stall a live PTY. This is the ONLY place
/// `command_events` reach the DB: the reader thread that accumulates them has no DB handle (see the
/// `pty_supervisor` module-level threading contract), so draining happens here, outside that thread.
async fn flush_scrollback_once(deps: &Arc<ServerDeps>) {
    // Snapshot every persisted session id OUTSIDE the DB lock (supervisor calls are sync).
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
        // Live-only gate (D1/D2): `meta()` returns `Err(NoSuchSession)` for an id the supervisor
        // doesn't track at all, and `Ok(meta)` with `is_active == false` for a cold-rehydrated or
        // exited-but-unreaped entry — either way there is no live reader that could have changed
        // this session's rows since it was persisted, so skip the sweep for it entirely.
        let meta = match deps.supervisor.meta(&id) {
            Ok(meta) if meta.is_active => meta,
            _ => continue,
        };

        // Persist the current meta (D2): resize/cwd/lifecycle are otherwise only mutated
        // in-memory (the only production `upsert_session` call is at CreateSession), so without
        // this a restart would rehydrate stale create-time cols/rows/cwd/lifecycle.
        {
            let db = deps.db.lock().await;
            if let Err(e) = db.upsert_session(&meta) {
                tracing::debug!(session = %id, error = %e, "scrollback flush: meta persist failed");
            }
        }

        if let Ok((_c, _r, bytes)) = deps.supervisor.snapshot_scrollback(&id) {
            let db = deps.db.lock().await;
            if let Err(e) = db.append_scrollback(&id, 0, &bytes, ts) {
                tracing::debug!(session = %id, error = %e, "scrollback flush: append failed");
            }
        }

        // Drain + persist any command_events accumulated since the last sweep (best-effort,
        // origin="gui" — the only actor this daemon currently drives commands as).
        let events = deps.supervisor.drain_command_events(&id);
        if !events.is_empty() {
            let db = deps.db.lock().await;
            for ev in events {
                if let Err(e) =
                    db.append_command_event(&id, ev.seq as i64, ev.ts, ev.kind, ev.exit_code, "gui")
                {
                    tracing::debug!(
                        session = %id, error = %e, "command_events flush: append failed"
                    );
                }
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
    stream: UnixStream,
    deps: Arc<ServerDeps>,
    broadcaster: Broadcaster,
    mut shutdown: watch::Receiver<bool>,
) -> std::io::Result<()> {
    // One decoder for the whole connection lifetime — carries any bytes batched after the
    // handshake preamble across the reader/writer split so no pipelined request is ever lost.
    let mut reader = FrameReader::new();

    // ---- Preamble handshake (Pv2 §4.2/§4.4): a fixed, codec-independent header precedes the CBOR
    // frame stream so a version-incompatible peer can always be told so, even if it can't decode
    // CBOR. Re-seated (S3 phase 1, spec §3) onto `bpa_daemon_core::handshake::server_handshake`
    // (moved out of this module verbatim, was `socket_server.rs:606-643`): every read/write there
    // is still `PREAMBLE_TIMEOUT`-bounded, so a stuck or garbage-writing peer still cannot hang
    // this connection task or hold the socket open indefinitely (fail closed). This call site
    // keeps its OWN quiet-`Ok(())` handling of both the `Ok(None)` (Incompatible already written,
    // just close) and `Err` (malformed/timeout, close without a reply) cases — sessiond's
    // externally observable behavior is unchanged.
    let mut stream = stream;
    match bpa_daemon_core::handshake::server_handshake(
        &mut stream,
        DAEMON_MIN_VERSION,
        DAEMON_MAX_VERSION,
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
    let (out_tx, mut out_rx) = mpsc::channel::<Frame>(CLIENT_OUTQ_CAP);

    // Register this client for supervisor push fan-out.
    broadcaster.register(conn_id, out_tx.clone());

    // Writer task: drains the bounded queue and writes to the socket. Exits on EPIPE/write error
    // (⇒ the client is gone) or when the queue is closed (all senders dropped).
    let mut writer = tokio::spawn(async move {
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
    // is uniform with everything else. `overflow_notify` is D4's honest-degradation signal: the
    // adapter's OWN forwarding task runs independently of the dispatch loop below (it is fed by
    // `AttachRegistry`'s per-attachment forwarders, not by inbound requests), so a `try_send`
    // overflow there — flood output + a slow/stalled GUI — must be able to tear down this whole
    // connection exactly like a request/response overflow does, rather than silently stopping
    // only the push path while the connection stays "up" and every future push is dropped.
    let overflow_notify = Arc::new(Notify::new());
    let push_sink = make_push_sink(out_tx.clone(), conn_id, overflow_notify.clone());

    // ---- Dispatch loop: correlate every Request{id} with exactly one Response{id}. ----
    let outcome: std::io::Result<()> = loop {
        tokio::select! {
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break Ok(());
                }
            }
            // D4: the push-forwarding task hit an outbound-queue overflow (a slow/stalled client
            // during a flood) and could not enqueue further pushes. Tear down THIS connection the
            // same way a request/response overflow does, so the client's reconnect+replay path
            // restores a consistent state instead of silently losing every future push forever.
            _ = overflow_notify.notified() => {
                break Err(std::io::Error::new(
                    std::io::ErrorKind::WouldBlock,
                    "client outbound queue overflow (push forwarder)",
                ));
            }
            frame = reader.next(&mut rd) => {
                match frame {
                    Ok(Some(Frame::Request { id, req })) => {
                        let res = dispatch(&deps, conn_id, &push_sink, &broadcaster, req).await;
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
    // keep-alive). Then let the writer drain/exit — bounded, not unconditional: dropping `out_tx`
    // only makes `out_rx.recv()` return `None` on its NEXT poll, but the writer may already be
    // parked inside `wr.write_all(...).await` on a client that stopped reading (the very overflow
    // this queue exists to catch, spec §13) — that write cannot complete until the client reads or
    // the kernel gives up, neither of which is guaranteed to happen promptly. Waiting on it
    // unconditionally here would hang THIS cleanup path forever for exactly the client this branch
    // exists to disconnect. `WRITER_JOIN_TIMEOUT` caps the wait; a still-running writer past that is
    // aborted outright (its socket half is dropped either way on return, closing the connection). ----
    broadcaster.deregister(conn_id);
    deps.attach.detach_all_for_conn(conn_id);
    drop(push_sink);
    drop(out_tx);
    if tokio::time::timeout(WRITER_JOIN_TIMEOUT, &mut writer)
        .await
        .is_err()
    {
        writer.abort();
    }
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

/// Wrap a `Push` sink over the client's bounded `Frame` queue. A gone writer (queue closed — the
/// connection is already being torn down for some other reason) just stops the forwarder, same as
/// before. An OUTBOUND-QUEUE OVERFLOW (`try_send` returns `Full`) is different (D4): silently
/// stopping only this forwarder would leave the connection's dispatch loop running normally while
/// every future push for it is dropped forever — "up" but not actually delivering anything, so a
/// re-attach on the same connection later fails `SinkClosed` instead of recovering. Honest
/// degradation instead: log a structured warning and fire `overflow_notify` so `handle_client`'s
/// dispatch loop tears down THIS WHOLE connection, driving the client through its normal
/// reconnect+replay path to a consistent state — no unbounded buffering, no half-alive connection.
fn make_push_sink(
    out_tx: mpsc::Sender<Frame>,
    conn_id: u64,
    overflow_notify: Arc<Notify>,
) -> PushSink {
    let (tx, mut rx) = mpsc::channel::<Push>(CLIENT_OUTQ_CAP);
    tokio::spawn(async move {
        while let Some(push) = rx.recv().await {
            match out_tx.try_send(Frame::Push(push)) {
                Ok(()) => {}
                Err(mpsc::error::TrySendError::Full(_)) => {
                    tracing::warn!(
                        conn = conn_id,
                        reason = "outbound overflow — disconnecting slow client",
                        "push forwarder: outbound queue full; disconnecting connection"
                    );
                    overflow_notify.notify_one();
                    break;
                }
                Err(mpsc::error::TrySendError::Closed(_)) => break, // writer already gone
            }
        }
    });
    tx
}

/// Convert any `Display` error into an `InvalidData` `io::Error`.
fn to_io<E: std::fmt::Display>(e: E) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
}

/// The single per-request completion-tracing choke-point (spec D4, O-6), mirroring orchd's
/// `socket_server::dispatch` wrapper. Wraps [`dispatch_inner`] so every verb emits exactly one
/// structured `info!` line when its response is ready — captured on every return path (including
/// `dispatch_inner`'s early `return`s) with no per-arm edit. The line carries `verb` (from the
/// exhaustive [`Request::verb_name`]), `outcome` (`"ok"`/`"err"`), `error_code` (the wire code
/// string, present only on an error), and `elapsed_ms` — never args, bodies, or the raw
/// `WriteStdin` terminal `bytes`.
async fn dispatch(
    deps: &Arc<ServerDeps>,
    conn_id: u64,
    push_sink: &PushSink,
    broadcaster: &Broadcaster,
    req: Request,
) -> Response {
    let verb = req.verb_name();
    let started = std::time::Instant::now();
    let res = dispatch_inner(deps, conn_id, push_sink, broadcaster, req).await;
    let elapsed_ms = started.elapsed().as_millis() as u64;
    match &res {
        Response::Error { code, .. } => {
            tracing::info!(verb, outcome = "err", error_code = %code, elapsed_ms, "request completed");
        }
        _ => {
            tracing::info!(verb, outcome = "ok", elapsed_ms, "request completed");
        }
    }
    res
}

/// Dispatch one `Request` to the right subsystem and produce the correlated `Response` (spec §7).
/// `conn_id` identifies the calling connection so attach/detach ownership is connection-scoped
/// (spec §7: single-attach per session, but teardown per connection). `broadcaster` is the SAME
/// registry `serve`'s accept loop uses to fan supervisor callbacks out to every connected client
/// (spec §7) — the `AddWorkspaceRoot`/`RemoveWorkspaceRoot` arms (spec §3.3) reuse it for
/// `Push::WorkspaceUpdated` so a workspace-root change is visible to every window/tab, not just the
/// one that issued the request. The per-request completion trace is added ONCE by the [`dispatch`]
/// wrapper above, so no arm here logs its own outcome.
async fn dispatch_inner(
    deps: &Arc<ServerDeps>,
    conn_id: u64,
    push_sink: &PushSink,
    broadcaster: &Broadcaster,
    req: Request,
) -> Response {
    match req {
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
                        message: format!(
                            "workspace root is not an existing directory: {root_path}"
                        ),
                    }
                }
            };
            let w = Workspace {
                id: uuid::Uuid::new_v4().to_string(),
                name,
                root_path: canonical.clone(),
                roots: vec![canonical],
            };
            let db = deps.db.lock().await;
            match db.upsert_workspace(&w) {
                Ok(()) => {
                    // BL-178: broadcast to EVERY client (consistent with AddWorkspaceRoot/
                    // RemoveWorkspaceRoot/RemoveWorkspace). Pre-BL-178 this only `try_send`'d on the
                    // requester's own `push_sink`, so a second concurrently-connected window never
                    // learned of a new workspace until it re-listed. The requester still gets the
                    // `Response::Workspace` below; everyone (incl. the requester) gets the Push.
                    broadcaster.broadcast(Frame::Push(Push::WorkspaceCreated {
                        workspace: w.clone(),
                    }));
                    Response::Workspace(w)
                }
                Err(e) => err("DbError", e),
            }
        }

        // spec §3.3: append a new root to an existing workspace. `path` goes through the SAME
        // §16 `validate_dir` gate as `CreateWorkspace.root_path` (absolute, existing, canonical —
        // no relaxed rule for a second root) before it ever reaches the DB. Unlike
        // `Push::WorkspaceCreated` above (emitted on the requester's own `push_sink`, spec §7's
        // comment there notwithstanding — that sink only reaches THIS connection), `WorkspaceUpdated`
        // is emitted via `broadcaster.broadcast` so every OTHER connected client (not just the one
        // that issued the request) learns a workspace's roots changed — the whole point of a
        // multi-window/multi-tab GUI staying in sync (spec §3.3, §7).
        Request::AddWorkspaceRoot { workspace_id, path } => {
            let canonical = match validate_dir(&path) {
                Ok(p) => p,
                Err(_) => {
                    return Response::Error {
                        code: "InvalidWorkspaceRoot".into(),
                        message: format!("workspace root is not an existing directory: {path}"),
                    }
                }
            };
            let db = deps.db.lock().await;
            match db.add_workspace_root(&workspace_id, &canonical) {
                Ok(updated) => {
                    broadcaster.broadcast(Frame::Push(Push::WorkspaceUpdated(updated.clone())));
                    Response::Workspace(updated)
                }
                Err(e) => err(e.code(), e),
            }
        }

        // spec §3.3: remove a root. No `validate_dir` here — removal doesn't require the path to
        // still exist on disk (the whole point is often to drop a root that moved/was deleted), and
        // `Db::remove_workspace_root` matches against whatever string is already stored. Rejects
        // removing the LAST remaining root (`PersistError::LastRoot` → wire code `"LastRoot"`, spec
        // §3.3) so a workspace can never end up with zero roots. Same `WorkspaceUpdated` broadcast
        // as `AddWorkspaceRoot` above on success.
        Request::RemoveWorkspaceRoot { workspace_id, path } => {
            // Canonicalize for matching (SES-6, ported from the concurrent main-tree fix): stored
            // roots are canonical (added via validate_dir), so a non-canonical incoming path would
            // otherwise never match → silent no-op that hid the last-root `LastRoot` guard.
            // Tolerates a root that no longer exists on disk.
            let canonical = canonicalize_root_for_match(&path);
            let db = deps.db.lock().await;
            match db.remove_workspace_root(&workspace_id, &canonical) {
                Ok(updated) => {
                    broadcaster.broadcast(Frame::Push(Push::WorkspaceUpdated(updated.clone())));
                    Response::Workspace(updated)
                }
                Err(e) => err(e.code(), e),
            }
        }

        // spec §3.3: remove a whole workspace — destructive and TOTAL. Ordering is the whole design:
        //
        //   1. existence gate (an unknown id must not kill anything),
        //   2. SES-1 (audit 2026-07-24, probe p5): mark the workspace CLOSING — from here on,
        //      `CreateSession` rejects it with the same `NoSuchWorkspace` a never-existent id
        //      gets, so no new session can be spawned into a workspace that is being torn down
        //      (previously a create racing this arm could slip in after the victim sweep and
        //      survive as an orphaned live shell inside a deleted workspace),
        //   3. collect every session that belongs to the workspace — the persisted rows UNION the
        //      live ones the supervisor tracks, because persistence is best-effort (spec §11) and a
        //      running PTY whose row failed to write is exactly the one we must not miss,
        //   4. kill/close each one through [`close_session`] — the SAME machinery `KillSession`
        //      uses, never a second teardown — so a removal can never leave an orphaned child
        //      process behind (the "zombie" failure D3 already guards against elsewhere),
        //   5. await the final-flush tasks those kills scheduled, so no detached best-effort write
        //      can land AFTER the delete and leave a resurrected row,
        //   6. delete the workspace + its roots + its sessions + those sessions' dependent rows in
        //      ONE transaction (`Db::delete_workspace`), and
        //   7. SES-1 post-delete stray sweep: a create that had passed BOTH of `CreateSession`'s
        //      gates before the closing mark was set can still land a live session during steps
        //      3-6 — kill any such survivor now, through the same `close_session` machinery.
        //
        // The DB lock is taken and released around each step — never held across a kill or across
        // the flush drain, both of which take that same lock themselves. The closing mark is held
        // by an RAII guard from step 2 until this arm returns, so every exit path releases it.
        //
        // The push is `Push::WorkspaceRemoved`, NOT `WorkspaceUpdated`: consumers upsert an
        // `Updated` payload into their store, so reusing it here would re-insert the workspace the
        // user just deleted. Broadcast (not `push_sink`) for the same multi-window reason as
        // `Add`/`RemoveWorkspaceRoot`. An unknown `workspace_id` is `Db::workspace_session_ids`'s
        // not-found error, byte-identical in code+message to what `RemoveWorkspaceRoot` already
        // returns for an unknown id.
        Request::RemoveWorkspace { workspace_id } => {
            let mut victims: std::collections::BTreeSet<bpa_protocol::SessionId> = {
                let db = deps.db.lock().await;
                match db.workspace_session_ids(&workspace_id) {
                    Ok(ids) => ids.into_iter().collect(),
                    Err(e) => return err(e.code(), e),
                }
            };
            // Step 2 (SES-1): the workspace exists — gate out any new `CreateSession` for it
            // until this arm has fully finished (guard clears the mark on drop, any path).
            let _closing_guard = deps.begin_closing_workspace(&workspace_id);

            let tracked: Vec<bpa_protocol::SessionId> =
                lock(&deps.live_sessions).iter().cloned().collect();
            for id in tracked {
                if let Ok(meta) = deps.supervisor.meta(&id) {
                    if meta.workspace_id == workspace_id {
                        victims.insert(id);
                    }
                }
            }

            for id in &victims {
                // A per-session failure is logged and the sweep continues: leaving the remaining
                // sessions alive AND the workspace half-removed would be strictly worse than
                // finishing. `close_session` already Acks the honest-close (inactive) case, so the
                // only errors reachable here are genuine PTY/IO failures.
                if let Response::Error { code, message } = close_session(deps, id).await {
                    tracing::warn!(
                        session = %id, workspace = %workspace_id, code = %code, message = %message,
                        "RemoveWorkspace: session teardown failed (continuing with the removal)"
                    );
                }
                lock(&deps.live_sessions).remove(id);
            }
            deps.await_pending_final_flushes().await;

            let deleted = {
                let db = deps.db.lock().await;
                db.delete_workspace(&workspace_id)
            };
            match deleted {
                Ok(_session_ids) => {
                    // Step 7 (SES-1): post-delete stray sweep. A `CreateSession` that passed its
                    // gates BEFORE the closing mark was set could have spawned a live session
                    // during the steps above (its best-effort persist then failed the FK —
                    // workspace row gone — but the live PTY would survive as an orphan). Kill
                    // any session of this workspace that is still live, through the same
                    // `close_session` machinery, and drain the flushes those kills schedule.
                    let strays: Vec<bpa_protocol::SessionId> =
                        lock(&deps.live_sessions).iter().cloned().collect();
                    for id in strays {
                        let belongs = matches!(
                            deps.supervisor.meta(&id),
                            Ok(meta) if meta.workspace_id == workspace_id
                        );
                        if !belongs {
                            continue;
                        }
                        if let Response::Error { code, message } = close_session(deps, &id).await {
                            tracing::warn!(
                                session = %id, workspace = %workspace_id, code = %code,
                                message = %message,
                                "RemoveWorkspace: stray-session teardown failed (continuing)"
                            );
                        }
                        lock(&deps.live_sessions).remove(&id);
                    }
                    deps.await_pending_final_flushes().await;

                    broadcaster.broadcast(Frame::Push(Push::WorkspaceRemoved {
                        workspace_id: workspace_id.clone(),
                    }));
                    Response::Ack
                }
                Err(e) => err(e.code(), e),
            }
        }

        // spec §3.3: read back a session's command history, newest-first, capped at `limit` — pure
        // read, no push (nothing else changed). An unknown `session_id` is `Db::list_command_events`'s
        // honest empty `Vec`, not an error (matches `ListSessions`/`ListWorkspaces`'s style: only a
        // genuine DB failure is a `Response::Error`).
        Request::GetCommandEvents { session_id, limit } => {
            let db = deps.db.lock().await;
            match db.list_command_events(&session_id, limit) {
                Ok(events) => Response::CommandEvents(events),
                Err(e) => err(e.code(), e),
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
                lock(&deps.live_sessions).iter().cloned().collect();
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

        Request::CreateSession {
            workspace_id,
            shell,
            cwd,
            env_overrides,
            cols,
            rows,
        } => {
            // SES-4 (audit 2026-07-24, probe p4): a session for a workspace that does not exist
            // must be REJECTED up front — previously the create succeeded and only the persist
            // failed (silently, FK, log-only), so the session worked until the next restart and
            // then vanished, with no client-visible error anywhere on the path.
            // SES-1 (probe p5): the same typed error gates a workspace that is mid-
            // `RemoveWorkspace`, closing the create/remove race at the entry point (the
            // removal's post-delete stray sweep covers the residual window — see that arm).
            if deps.is_workspace_closing(&workspace_id) {
                return Response::Error {
                    code: "NoSuchWorkspace".into(),
                    message: format!("no workspace {workspace_id}"),
                };
            }
            {
                let db = deps.db.lock().await;
                match db.workspace_exists(&workspace_id) {
                    Ok(true) => {}
                    Ok(false) => {
                        return Response::Error {
                            code: "NoSuchWorkspace".into(),
                            message: format!("no workspace {workspace_id}"),
                        };
                    }
                    Err(e) => return err("DbError", e),
                }
            }
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
                Err(resp) => return *resp,
            };
            match deps.supervisor.create(spec) {
                Ok(id) => match deps.supervisor.meta(&id) {
                    Ok(meta) => {
                        // Track the live session so ListSessions/GetSessionState surface it even if
                        // the (best-effort) persist below fails (spec §11).
                        lock(&deps.live_sessions).insert(id.clone());
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
            match deps
                .attach
                .attach(conn_id, &session_id, push_sink.clone())
                .await
            {
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

        Request::Resize {
            session_id,
            cols,
            rows,
        } => match deps.supervisor.resize(&session_id, cols, rows) {
            Ok(()) => Response::Ack,
            Err(e) => err(code_for(&e), e),
        },

        Request::KillSession { session_id } => close_session(deps, &session_id).await,

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

        // Real DaemonShutdown semantics (Pv2 §6.1): `drain:true` flushes scrollback + command_events
        // for every live session (best-effort, same routine as the periodic sweep) BEFORE Acking;
        // `drain:false` skips the flush. Either way we then flip the shared shutdown watch — the
        // SAME trigger `main.rs`'s SIGTERM handler flips — so a GUI-initiated shutdown converges on
        // the identical graceful-exit path: `serve()`'s accept loop stops, this connection's dispatch
        // loop breaks on the next `shutdown.changed()`, and `boot::run` runs its post-`serve` drain
        // (supervisor killpg, DB checkpoint, socket unlink) and the process exits cleanly. Because
        // launchd's `KeepAlive{Crashed}` only restarts on a crash, a clean exit here is NOT
        // auto-restarted (the upgrade flow, Task 10, `kickstart`s the replacement explicitly).
        //
        // Ordering is deliberate: the flush and the `send` both happen here, BEFORE this function
        // returns `Response::Ack`. The caller (`dispatch_loop` in `handle_client`) only enqueues the
        // reply into this connection's bounded outbound queue AFTER `dispatch` returns, so flipping
        // the watch first cannot race the client out of receiving its Ack — the accept loop and other
        // connections' dispatch loops only observe the flip on their NEXT `tokio::select!` poll, and
        // this connection's own dispatch loop only observes it after this in-flight request/response
        // round-trip already enqueued the reply. The writer task drains that queue independently of
        // the shutdown signal, and the existing `WRITER_JOIN_TIMEOUT` cap on connection cleanup gives
        // it time to flush the already-enqueued Ack before this connection is torn down.
        Request::DaemonShutdown { drain } => {
            if drain {
                flush_scrollback_once(deps).await;
            }
            let _ = deps.shutdown_tx.send(true);
            Response::Ack
        }
    }
}

/// The ONE session-teardown path (spec §9.8 + D3). Extracted verbatim from what was
/// `Request::KillSession`'s dispatch arm, and now shared by BOTH that arm and
/// `Request::RemoveWorkspace`'s per-session sweep — deliberately not duplicated, so a workspace
/// removal terminates a PTY through exactly the same `killpg → grace → SIGKILL → reap` machinery a
/// direct kill does, and inherits the same honest-close behaviour for PTY-less entries.
///
/// Live session ⇒ [`Supervisor::kill`] (`Ack`). PTY-less/inactive entry ⇒ `kill()` fails
/// `NoSuchSession`, which pre-D3 left it an unkillable zombie: still in the supervisor map (so
/// `ListSessions` kept surfacing it) and its rows still in the DB (so every future restart kept
/// resurrecting it). Distinguish "genuinely unknown" from "known but inactive" via `meta()`, and
/// for the latter perform an honest close: drop any replay-only attach entries, remove the
/// supervisor map entry, and delete the persisted rows — so `ListSessions` stops showing it and it
/// never comes back. A genuinely unknown id is the honest `NoSuchSession` error.
async fn close_session(deps: &Arc<ServerDeps>, session_id: &bpa_protocol::SessionId) -> Response {
    match deps.supervisor.kill(session_id) {
        Ok(()) => Response::Ack,
        Err(SupervisorError::NoSuchSession(_)) => match deps.supervisor.meta(session_id) {
            Ok(meta) if !meta.is_active => {
                let _ = deps.attach.remove_session(session_id);
                let _ = deps.supervisor.remove_inactive(session_id);
                lock(&deps.live_sessions).remove(session_id);
                let db = deps.db.lock().await;
                if let Err(e) = db.delete_session(session_id) {
                    tracing::warn!(
                        session = %session_id, error = %e,
                        "KillSession: honest-close delete_session failed (best-effort)"
                    );
                }
                Response::Ack
            }
            _ => Response::Error {
                code: "NoSuchSession".into(),
                message: format!("no session {session_id}"),
            },
        },
        Err(e) => err(code_for(&e), e),
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
    Response::Error {
        code: code.into(),
        message: e.to_string(),
    }
}

/// The user's INTERACTIVE PATH — what Terminal.app/iTerm would set — resolved ONCE per daemon
/// process (cached in `OnceLock`). The launchd-managed daemon inherits only the minimal system
/// PATH, so without this a spawned shell misses Homebrew (`/opt/homebrew/bin`), npm/pnpm globals
/// (`claude`, `codex`, `tsx`…), nvm/volta, `~/.cargo/bin`, asdf, etc. — `claude: command not found`
/// even though it works in the user's own Terminal. Resolved by running the user's default login
/// shell (`$SHELL -l -c 'printf %s "$PATH"'`), which sources `/etc/paths` (via `path_helper`) +
/// `~/.zprofile`/`~/.zshrc` exactly like a login Terminal. Falls back to the process PATH / the
/// minimal system PATH if the resolution fails (never blocks session creation).
fn user_login_path() -> &'static str {
    static CACHE: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    CACHE.get_or_init(|| {
        let shell = std::env::var("SHELL").unwrap_or_else(|_| "/bin/zsh".to_string());
        let resolved = std::process::Command::new(&shell)
            .args(["-l", "-c", "printf %s \"$PATH\""])
            .output()
            .ok()
            .filter(|o| o.status.success())
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .filter(|s| !s.is_empty() && s.contains('/'));
        match resolved {
            Some(p) => p,
            None => {
                std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin:/usr/sbin:/sbin".into())
            }
        }
    })
}

/// Validate a workspace root / session cwd via the shared `bpa-paths` validator (spec §16):
/// absolute + exists + is-a-directory + no symlink-escape of the lexical parent — byte-for-byte
/// the same rule the core enforces. Returns the canonical path string on success.
fn validate_dir(path: &str) -> Result<String, bpa_paths::PathError> {
    bpa_paths::validate_dir(std::path::Path::new(path)).map(|p| p.to_string_lossy().into_owned())
}

/// Canonicalize a workspace-root path for MATCHING against the roots stored at `AddWorkspaceRoot`
/// time (which `validate_dir` canonicalized). `RemoveWorkspaceRoot` must also work for a root that
/// no longer exists on disk (the common case — the folder moved/was deleted), so unlike
/// `validate_dir` this does NOT fail closed: a `canonicalize` failure falls back to canonicalizing
/// the PARENT (which usually still exists) and re-joining the basename, matching the canonical form
/// the root was stored under. SES-6 (ported from the concurrent main-tree fix): without this the
/// incoming (non-canonical) path never matched the stored canonical path, so removal was a silent
/// no-op and the last-root `LastRoot` guard never fired — the daemon ACKed without changing
/// anything.
fn canonicalize_root_for_match(path: &str) -> String {
    let p = std::path::Path::new(path);
    if let Ok(c) = std::fs::canonicalize(p) {
        return c.to_string_lossy().into_owned();
    }
    match (p.parent(), p.file_name()) {
        (Some(parent), Some(name)) if !parent.as_os_str().is_empty() => {
            match std::fs::canonicalize(parent) {
                Ok(c) => c.join(name).to_string_lossy().into_owned(),
                Err(_) => path.to_string(),
            }
        }
        _ => path.to_string(),
    }
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
) -> Result<SessionSpec, Box<Response>> {
    // ---- Shell selection: explicit → $SHELL → /bin/zsh (all must be absolute). ----
    let shell_path = shell
        .filter(|s| !s.is_empty())
        .or_else(|| std::env::var("SHELL").ok())
        .unwrap_or_else(|| "/bin/zsh".to_string());
    if !std::path::Path::new(&shell_path).is_absolute() {
        return Err(Box::new(Response::Error {
            code: "InvalidShell".into(),
            message: format!("shell path must be absolute: {shell_path}"),
        }));
    }

    // ---- cwd validation (§16): canonical, absolute, existing directory. Default to $HOME. ----
    let cwd_input = cwd
        .filter(|c| !c.is_empty())
        .or_else(|| std::env::var("HOME").ok())
        .unwrap_or_else(|| "/".to_string());
    let cwd_canonical = match validate_dir(&cwd_input) {
        Ok(p) => std::path::PathBuf::from(p),
        Err(_) => {
            return Err(Box::new(Response::Error {
                code: "CwdMissing".into(),
                message: format!("cwd is not an existing directory: {cwd_input}"),
            }))
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
    push_env("HOME", None);
    push_env("USER", None);
    push_env("LOGNAME", None);
    push_env("LANG", None);
    push_env("SHELL", Some(&shell_path));
    push_env("SSH_AUTH_SOCK", None);
    // PATH: the user's REAL interactive PATH (Homebrew, npm/pnpm globals → `claude`/`codex`, nvm,
    // cargo…), resolved from their login shell so spawned shells match Terminal.app — NOT the
    // daemon's minimal launchd PATH (which made `claude`/`brew` "command not found"). See
    // `user_login_path`.
    env.push(("PATH".to_string(), user_login_path().to_string()));

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

    // ---- Caller overrides win last (spec §7 env_overrides), but NEVER a dynamic-linker
    // injection var (`DYLD_*`/`LD_*` — closes BL-1, S-EXT §6 task T16): stripped from the
    // caller-supplied overrides BEFORE they're merged into the child env, via the SAME shared
    // denylist orchd's stdio spawn uses (`bpa_daemon_core::env_filter`) — one implementation, no
    // second unfiltered path.
    let mut env_overrides = env_overrides;
    bpa_daemon_core::env_filter::strip_dangerous_env(&mut env_overrides);
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
    use bpa_protocol::{
        decode_daemon_reply, encode_client_preamble, ClientPreamble, DaemonReply, Frame, Push,
        Request, Response, CLIENT_MAX_VERSION, CLIENT_MIN_VERSION,
    };
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{UnixListener, UnixStream};

    #[test]
    fn canonicalize_root_for_match_resolves_an_existing_dir_to_its_canonical_form() {
        // SES-6 (ported from the concurrent main-tree fix): a workspace root stored via
        // validate_dir is canonical; a remove that sends a non-canonical spelling (here: a path
        // with a `.` segment, which canonicalize collapses) must resolve to the same canonical
        // form so it MATCHES and the LastRoot guard can fire.
        let dir = tempfile::tempdir().unwrap();
        let real = std::fs::canonicalize(dir.path()).unwrap();
        // A non-canonical spelling: parent + "." + basename.
        let dot = real
            .parent()
            .unwrap()
            .join(".")
            .join(real.file_name().unwrap());
        assert_eq!(
            canonicalize_root_for_match(&dot.to_string_lossy()),
            real.to_string_lossy().into_owned(),
            "a non-canonical existing path must canonicalize to match the stored root"
        );
    }

    #[test]
    fn canonicalize_root_for_match_tolerates_a_gone_root_via_its_parent() {
        // SES-6: removal must work for a root that no longer exists on disk (folder deleted/moved)
        // — canonicalize fails, so we canonicalize the PARENT (which exists) and re-join the name.
        let parent = tempfile::tempdir().unwrap();
        let parent_canon = std::fs::canonicalize(parent.path()).unwrap();
        let gone = parent_canon.join("deleted-root");
        let got = canonicalize_root_for_match(&gone.to_string_lossy());
        assert_eq!(
            got,
            gone.to_string_lossy().into_owned(),
            "a deleted root must normalize via the canonical parent + basename, got {got}"
        );
    }

    // ---- framing helpers (mirror the server codec on the client side) ----
    async fn send_frame(s: &mut UnixStream, f: &Frame) {
        // `encode_frame` already prepends the u32-LE length prefix — write its output verbatim,
        // do NOT add a second prefix on top.
        let bytes = encode_frame(f).unwrap();
        s.write_all(&bytes).await.unwrap();
        s.flush().await.unwrap();
    }

    async fn recv_frame(s: &mut UnixStream) -> Frame {
        // Read exactly one length-prefixed CBOR frame via the shared FrameDecoder: read the 4-byte
        // LE length, then that many body bytes, feed both into the decoder, and take the single
        // frame it yields (mirrors the length-prefix framing `encode_frame` produces).
        let mut lenb = [0u8; 4];
        s.read_exact(&mut lenb).await.unwrap();
        let len = u32::from_le_bytes(lenb) as usize;
        let mut body = vec![0u8; len];
        s.read_exact(&mut body).await.unwrap();
        let mut decoder = FrameDecoder::new();
        decoder.push(&lenb);
        decoder.push(&body);
        let mut frames = decoder.decode().unwrap();
        frames.remove(0)
    }

    /// Like [`recv_frame`] but bounded: panics on a 3 s timeout so a regressed handshake gate (that
    /// wrongly keeps the connection open instead of closing) fails fast instead of hanging the suite.
    async fn recv_frame_t(s: &mut UnixStream) -> Frame {
        match tokio::time::timeout(std::time::Duration::from_secs(3), recv_frame(s)).await {
            Ok(f) => f,
            Err(_) => panic!("timed out waiting for a frame (handshake/close regression?)"),
        }
    }

    /// Send a `CreateWorkspace` request over `c` with request id `id` and drain BOTH resulting
    /// frames — the `Response::Workspace` and its accompanying `Push::WorkspaceCreated` — before
    /// returning the created `Workspace`. Order between the two is NOT guaranteed: the push travels
    /// through `push_sink`'s own async forwarder task (`make_push_sink`, scheduled independently by
    /// the runtime), while the response is enqueued directly by `handle_client`'s dispatch loop
    /// right after `dispatch` returns — so the response can (and, in practice, usually does) win the
    /// race. Draining exactly 2 frames unconditionally (rather than breaking as soon as the target
    /// `Response` is seen) is required so this helper never leaves an undrained push frame sitting
    /// in the socket buffer to desync a later, strictly `id`-keyed read on the same connection.
    async fn create_workspace(
        c: &mut UnixStream,
        id: u64,
        name: &str,
        root_path: &str,
    ) -> Workspace {
        send_frame(
            c,
            &Frame::Request {
                id,
                req: Request::CreateWorkspace {
                    name: name.into(),
                    root_path: root_path.into(),
                },
            },
        )
        .await;
        let mut got: Option<Workspace> = None;
        for _ in 0..2 {
            match recv_frame_t(c).await {
                Frame::Response {
                    id: rid,
                    res: Response::Workspace(w),
                } if rid == id => got = Some(w),
                Frame::Push(Push::WorkspaceCreated { .. }) => {}
                other => panic!("unexpected frame while creating workspace: {other:?}"),
            }
        }
        got.expect("expected a Workspace response from create_workspace")
    }

    /// Build [`ServerDeps`] wired to `shutdown_tx` — the SAME sender whose receiver half the caller
    /// drives [`serve`] with — so `Request::DaemonShutdown`'s dispatch arm (which calls
    /// `deps.shutdown_tx.send(true)`) flips the identical watch that stops the server under test.
    fn test_deps_with_shutdown(
        shutdown_tx: watch::Sender<bool>,
    ) -> (Arc<ServerDeps>, tempfile::TempDir) {
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
            shutdown_tx,
        ));
        (deps, runtime)
    }

    fn test_deps() -> (Arc<ServerDeps>, tempfile::TempDir) {
        let (tx, _rx) = tokio::sync::watch::channel(false);
        test_deps_with_shutdown(tx)
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
        let (tx, rx) = tokio::sync::watch::channel(false);
        let (deps, runtime) = test_deps_with_shutdown(tx.clone());
        let jh = tokio::spawn(async move {
            let _ = serve(listener, deps, rx).await;
        });
        (path, tx, jh, dir, runtime)
    }

    /// Send a [`ClientPreamble`] (min/max = 3, the current client range) and read back the daemon's
    /// decoded [`DaemonReply`]. Distinct from [`send_frame`]/[`recv_frame`]: the preamble is a raw
    /// codec-agnostic byte layout (Pv2 §4.2), not a CBOR `Frame`, so it needs its own wire helper.
    async fn preamble(s: &mut UnixStream) -> DaemonReply {
        send_preamble(s, CLIENT_MIN_VERSION, CLIENT_MAX_VERSION, "test").await;
        recv_daemon_reply(s).await
    }

    /// Write a client preamble advertising the given `[min, max]` + build string.
    async fn send_preamble(s: &mut UnixStream, min: u16, max: u16, build: &str) {
        let bytes = encode_client_preamble(&ClientPreamble {
            min,
            max,
            build: build.into(),
        });
        s.write_all(&bytes).await.unwrap();
        s.flush().await.unwrap();
    }

    /// Read and decode one [`DaemonReply`] off the wire. Bounded to 3 s so a regressed handshake
    /// gate (that wrongly never replies) fails the test fast instead of hanging the suite.
    async fn recv_daemon_reply(s: &mut UnixStream) -> DaemonReply {
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            // Accepted: magic(4)+result(1)+chosen(2)+build_len(2) = 9 bytes, then build_len more.
            // Incompatible: magic(4)+result(1)+min(2)+max(2) = 9 bytes, no trailing body. Both
            // fixed headers are 9 bytes, so read that first and branch on the trailing build only
            // for Accepted.
            let mut header = [0u8; 9];
            s.read_exact(&mut header).await.unwrap();
            let result = header[4];
            let mut buf = header.to_vec();
            if result == 1 {
                let build_len = u16::from_le_bytes(header[7..9].try_into().unwrap()) as usize;
                let mut build = vec![0u8; build_len];
                s.read_exact(&mut build).await.unwrap();
                buf.extend_from_slice(&build);
            }
            decode_daemon_reply(&buf).expect("valid daemon reply")
        })
        .await
        .expect("timed out waiting for daemon reply (handshake regression?)")
    }

    // ---- BL-1 (S-EXT §6, task T16): `env_overrides` must never be able to inject a
    // dynamic-linker var into the resolved child env. `resolve_session_spec` is pure/sync, so a
    // plain `#[test]` exercises it directly — no server/PTY needed. ----
    #[test]
    fn env_overrides_strips_dyld_and_ld_vars_but_keeps_benign_overrides() {
        let runtime_root = std::path::Path::new("/tmp");
        let spec = resolve_session_spec(
            runtime_root,
            "ws-test".into(),
            Some("/bin/sh".into()), // basename "sh" — classify_shell returns None, no runtime_root I/O
            Some("/tmp".into()),
            vec![
                ("DYLD_INSERT_LIBRARIES".into(), "/evil.dylib".into()),
                ("LD_PRELOAD".into(), "/evil.so".into()),
                ("FOO".into(), "bar".into()),
            ],
            80,
            24,
        )
        .expect("resolve_session_spec should succeed for an existing cwd");

        assert!(
            !spec.env.iter().any(|(k, _)| k == "DYLD_INSERT_LIBRARIES"),
            "DYLD_INSERT_LIBRARIES must never reach the resolved child env: {:?}",
            spec.env
        );
        assert!(
            !spec.env.iter().any(|(k, _)| k == "LD_PRELOAD"),
            "LD_PRELOAD must never reach the resolved child env: {:?}",
            spec.env
        );
        assert!(
            spec.env.iter().any(|(k, v)| k == "FOO" && v == "bar"),
            "a benign override must still apply: {:?}",
            spec.env
        );
    }

    #[tokio::test]
    async fn handshake_happy_path_returns_accepted() {
        let (path, _tx, _jh, _d, _r) = spawn_server().await;
        let mut c = UnixStream::connect(&path).await.unwrap();
        match preamble(&mut c).await {
            DaemonReply::Accepted { chosen, build } => {
                assert_eq!(chosen, CLIENT_MAX_VERSION.min(DAEMON_MAX_VERSION));
                assert_eq!(build, "test");
            }
            other => panic!("expected Accepted, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn incompatible_client_range_is_rejected_and_closed() {
        let (path, _tx, _jh, _d, _r) = spawn_server().await;
        let mut c = UnixStream::connect(&path).await.unwrap();
        // Client advertises [2, 2] (a stale pre-S2 build); daemon now only speaks [3, 3]
        // (CLIENT/DAEMON_*_VERSION bumped v2 -> v3 in S2, `[0.3.0]`) ⇒ no overlap. This is the
        // mirror of the critical wire-compat scenario the bump fixes: an old GUI against a new
        // daemon must get Incompatible, never a silent misdecode.
        send_preamble(&mut c, 2, 2, "test").await;
        match recv_daemon_reply(&mut c).await {
            DaemonReply::Incompatible { min, max } => {
                assert_eq!((min, max), (DAEMON_MIN_VERSION, DAEMON_MAX_VERSION));
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
    async fn garbage_preamble_closes_without_hang() {
        let (path, _tx, _jh, _d, _r) = spawn_server().await;
        let mut c = UnixStream::connect(&path).await.unwrap();
        // Write 4 random/garbage bytes (not a valid magic, not even a full header) then nothing
        // more: the daemon must give up and close within PREAMBLE_TIMEOUT rather than hang forever
        // waiting for the rest of a header that will never arrive.
        c.write_all(&[0xDE, 0xAD, 0xBE, 0xEF]).await.unwrap();
        c.flush().await.unwrap();
        let mut buf = [0u8; 1];
        let n = tokio::time::timeout(
            bpa_protocol::PREAMBLE_TIMEOUT + std::time::Duration::from_secs(2),
            c.read(&mut buf),
        )
        .await
        .expect("server must close a garbage preamble within PREAMBLE_TIMEOUT (timed out waiting)")
        .unwrap();
        assert_eq!(
            n, 0,
            "server must close the connection on a garbage/short preamble"
        );
    }

    #[tokio::test]
    async fn requests_are_answered_with_matching_ids_concurrently() {
        let (path, _tx, _jh, _d, _r) = spawn_server().await;
        let mut c = UnixStream::connect(&path).await.unwrap();
        assert!(matches!(
            preamble(&mut c).await,
            DaemonReply::Accepted { .. }
        ));

        // Fire three ListWorkspaces requests with distinct ids back-to-back.
        for id in [11u64, 22, 33] {
            send_frame(
                &mut c,
                &Frame::Request {
                    id,
                    req: Request::ListWorkspaces,
                },
            )
            .await;
        }
        let mut seen = std::collections::HashSet::new();
        for _ in 0..3 {
            match recv_frame(&mut c).await {
                Frame::Response {
                    id,
                    res: Response::Workspaces(_),
                } => {
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
        assert!(matches!(
            preamble(&mut c).await,
            DaemonReply::Accepted { .. }
        ));

        // /tmp is a real, existing directory — passes §16 validation.
        send_frame(
            &mut c,
            &Frame::Request {
                id: 5,
                req: Request::CreateWorkspace {
                    name: "w".into(),
                    root_path: "/tmp".into(),
                },
            },
        )
        .await;

        let mut got_resp: Option<Workspace> = None;
        let mut got_push = false;
        for _ in 0..2 {
            match recv_frame(&mut c).await {
                Frame::Response {
                    id: 5,
                    res: Response::Workspace(w),
                } => got_resp = Some(w),
                Frame::Push(Push::WorkspaceCreated { .. }) => got_push = true,
                other => panic!("unexpected frame {other:?}"),
            }
        }
        assert!(got_resp.is_some() && got_push);

        // The workspace is persisted: a subsequent ListWorkspaces reflects it.
        send_frame(
            &mut c,
            &Frame::Request {
                id: 6,
                req: Request::ListWorkspaces,
            },
        )
        .await;
        loop {
            match recv_frame(&mut c).await {
                Frame::Response {
                    id: 6,
                    res: Response::Workspaces(v),
                } => {
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
        assert!(matches!(
            preamble(&mut c).await,
            DaemonReply::Accepted { .. }
        ));
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
            Frame::Response {
                id: 9,
                res: Response::Error { code, .. },
            } => {
                assert_eq!(code, "InvalidWorkspaceRoot");
            }
            other => panic!("expected InvalidWorkspaceRoot error, got {other:?}"),
        }
    }

    // ---- S2 §3.3: AddWorkspaceRoot appends a root, persists it, replies with the updated
    // Workspace, and broadcasts Push::WorkspaceUpdated to EVERY connected client — a second,
    // otherwise-idle connection must observe it too (spec §7: multi-window/multi-tab GUI stays in
    // sync). This is why the new arms broadcast via `Broadcaster` rather than reusing the
    // `push_sink` `Push::WorkspaceCreated` uses above: `push_sink` is this connection's OWN
    // outbound queue (see `make_push_sink`'s doc comment), so it can never reach a second
    // connection no matter which push variant is sent through it. ----
    #[tokio::test]
    async fn add_workspace_root_persists_and_broadcasts_to_other_clients() {
        let (path, _tx, _jh, _d, _r) = spawn_server().await;

        let mut c1 = UnixStream::connect(&path).await.unwrap();
        assert!(matches!(
            preamble(&mut c1).await,
            DaemonReply::Accepted { .. }
        ));
        let workspace_id = create_workspace(&mut c1, 1, "w", "/tmp").await.id;

        // A second, unrelated connection — registered for broadcast BEFORE the request below is
        // issued, so it can observe the push.
        let mut c2 = UnixStream::connect(&path).await.unwrap();
        assert!(matches!(
            preamble(&mut c2).await,
            DaemonReply::Accepted { .. }
        ));

        let second_root = tempfile::tempdir().unwrap();
        let second_root_path = second_root.path().to_str().unwrap().to_string();

        send_frame(
            &mut c1,
            &Frame::Request {
                id: 2,
                req: Request::AddWorkspaceRoot {
                    workspace_id: workspace_id.clone(),
                    path: second_root_path,
                },
            },
        )
        .await;

        let mut got_resp: Option<Workspace> = None;
        let mut got_push_on_c1 = false;
        for _ in 0..2 {
            match recv_frame_t(&mut c1).await {
                Frame::Response {
                    id: 2,
                    res: Response::Workspace(w),
                } => got_resp = Some(w),
                Frame::Push(Push::WorkspaceUpdated(_)) => got_push_on_c1 = true,
                other => panic!("unexpected frame on c1: {other:?}"),
            }
        }
        let updated = got_resp.expect("expected a Workspace response on c1");
        assert_eq!(updated.id, workspace_id);
        assert_eq!(
            updated.roots.len(),
            2,
            "expected 2 roots after AddWorkspaceRoot"
        );
        assert!(
            got_push_on_c1,
            "the requester must also observe its own WorkspaceUpdated push (it is registered \
             with the broadcaster like every other client)"
        );

        match recv_frame_t(&mut c2).await {
            Frame::Push(Push::WorkspaceUpdated(w)) => {
                assert_eq!(w.id, workspace_id);
                assert_eq!(w.roots.len(), 2);
            }
            other => panic!(
                "expected WorkspaceUpdated push on the second, unrelated connection, got {other:?}"
            ),
        }
    }

    #[tokio::test]
    async fn add_workspace_root_rejects_missing_dir_and_persists_nothing() {
        let (path, _tx, _jh, _d, _r) = spawn_server().await;
        let mut c = UnixStream::connect(&path).await.unwrap();
        assert!(matches!(
            preamble(&mut c).await,
            DaemonReply::Accepted { .. }
        ));
        let workspace_id = create_workspace(&mut c, 1, "w", "/tmp").await.id;

        send_frame(
            &mut c,
            &Frame::Request {
                id: 2,
                req: Request::AddWorkspaceRoot {
                    workspace_id: workspace_id.clone(),
                    path: "/nonexistent/path/xyzzy".into(),
                },
            },
        )
        .await;
        match recv_frame_t(&mut c).await {
            Frame::Response {
                id: 2,
                res: Response::Error { code, .. },
            } => {
                assert_eq!(code, "InvalidWorkspaceRoot");
            }
            other => panic!("expected InvalidWorkspaceRoot error, got {other:?}"),
        }

        // Nothing persisted: the workspace still has exactly its original root.
        send_frame(
            &mut c,
            &Frame::Request {
                id: 3,
                req: Request::ListWorkspaces,
            },
        )
        .await;
        loop {
            match recv_frame_t(&mut c).await {
                Frame::Response {
                    id: 3,
                    res: Response::Workspaces(v),
                } => {
                    let w = v
                        .iter()
                        .find(|w| w.id == workspace_id)
                        .expect("workspace must still be present");
                    assert_eq!(
                        w.roots.len(),
                        1,
                        "a rejected AddWorkspaceRoot must persist nothing"
                    );
                    break;
                }
                Frame::Push(_) => continue,
                other => panic!("unexpected frame {other:?}"),
            }
        }
    }

    #[tokio::test]
    async fn remove_workspace_root_last_one_is_rejected_with_last_root_code() {
        let (path, _tx, _jh, _d, _r) = spawn_server().await;
        let mut c = UnixStream::connect(&path).await.unwrap();
        assert!(matches!(
            preamble(&mut c).await,
            DaemonReply::Accepted { .. }
        ));
        let created = create_workspace(&mut c, 1, "w", "/tmp").await;
        let (workspace_id, root_path) = (created.id, created.root_path);

        send_frame(
            &mut c,
            &Frame::Request {
                id: 2,
                req: Request::RemoveWorkspaceRoot {
                    workspace_id,
                    path: root_path,
                },
            },
        )
        .await;
        match recv_frame_t(&mut c).await {
            Frame::Response {
                id: 2,
                res: Response::Error { code, .. },
            } => {
                assert_eq!(code, "LastRoot");
            }
            other => panic!("expected LastRoot error, got {other:?}"),
        }
    }

    // ---- Success path for RemoveWorkspaceRoot (the test above only covers the LastRoot
    // rejection): removing a NON-last root persists, replies with the shrunk Workspace, and
    // broadcasts WorkspaceUpdated to a second connection exactly like AddWorkspaceRoot. ----
    #[tokio::test]
    async fn remove_workspace_root_non_last_persists_and_broadcasts() {
        let (path, _tx, _jh, _d, _r) = spawn_server().await;
        let mut c1 = UnixStream::connect(&path).await.unwrap();
        assert!(matches!(
            preamble(&mut c1).await,
            DaemonReply::Accepted { .. }
        ));
        let workspace_id = create_workspace(&mut c1, 1, "w", "/tmp").await.id;

        let second_root = tempfile::tempdir().unwrap();
        let second_root_path = second_root.path().to_str().unwrap().to_string();
        send_frame(
            &mut c1,
            &Frame::Request {
                id: 2,
                req: Request::AddWorkspaceRoot {
                    workspace_id: workspace_id.clone(),
                    path: second_root_path,
                },
            },
        )
        .await;
        // Drain the response + this connection's own broadcast push (order is not guaranteed),
        // capturing the response so the removal below targets the STORED canonical string —
        // `remove_workspace_root` does an exact string match with no re-canonicalization (see its
        // doc comment), and `tempfile::tempdir()`'s own path is not necessarily already canonical
        // (e.g. macOS's `/var` → `/private/var` symlink), so reusing our own pre-`validate_dir`
        // string here would silently no-op instead of removing anything.
        let mut added: Option<Workspace> = None;
        for _ in 0..2 {
            match recv_frame_t(&mut c1).await {
                Frame::Response {
                    id: 2,
                    res: Response::Workspace(w),
                } => added = Some(w),
                Frame::Push(Push::WorkspaceUpdated(_)) => {}
                other => panic!("unexpected frame on c1: {other:?}"),
            }
        }
        let added = added.expect("expected a Workspace response for AddWorkspaceRoot");
        assert_eq!(added.roots.len(), 2);
        let original_root_path = added.root_path.clone();
        let canonical_second_root = added
            .roots
            .into_iter()
            .find(|r| *r != original_root_path)
            .expect("expected a second root distinct from root_path");

        // A second, unrelated connection observes the REMOVE's broadcast.
        let mut c2 = UnixStream::connect(&path).await.unwrap();
        assert!(matches!(
            preamble(&mut c2).await,
            DaemonReply::Accepted { .. }
        ));

        send_frame(
            &mut c1,
            &Frame::Request {
                id: 3,
                req: Request::RemoveWorkspaceRoot {
                    workspace_id: workspace_id.clone(),
                    path: canonical_second_root,
                },
            },
        )
        .await;

        let mut got_resp: Option<Workspace> = None;
        for _ in 0..2 {
            match recv_frame_t(&mut c1).await {
                Frame::Response {
                    id: 3,
                    res: Response::Workspace(w),
                } => got_resp = Some(w),
                Frame::Push(Push::WorkspaceUpdated(_)) => {}
                other => panic!("unexpected frame on c1: {other:?}"),
            }
        }
        let updated = got_resp.expect("expected a Workspace response");
        assert_eq!(updated.roots.len(), 1, "back down to 1 root after remove");

        match recv_frame_t(&mut c2).await {
            Frame::Push(Push::WorkspaceUpdated(w)) => {
                assert_eq!(w.id, workspace_id);
                assert_eq!(w.roots.len(), 1);
            }
            other => panic!("expected WorkspaceUpdated push on c2, got {other:?}"),
        }
    }

    // ---- S2 §3.3: GetCommandEvents reads a session's command_events rows back via the real
    // dispatch arm over the wire, newest-first, capped at `limit`. Seeded directly via `Db` (same
    // `deps` handle the test keeps, mirroring
    // `kill_session_on_rehydrated_inactive_session_is_an_honest_close`'s inline
    // listener+deps+serve pattern) rather than driving a real PTY through OSC-133 end to end —
    // deterministic and immune to shell/timing flakiness while still exercising the exact
    // `list_command_events` call the production arm makes. ----
    #[tokio::test]
    async fn get_command_events_returns_newest_first_and_respects_limit() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("d.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let (tx, rx) = tokio::sync::watch::channel(false);
        let (deps, _runtime) = test_deps_with_shutdown(tx.clone());
        let _jh = tokio::spawn({
            let deps = deps.clone();
            async move {
                let _ = serve(listener, deps, rx).await;
            }
        });

        let session_id = "s-command-events".to_string();
        {
            let db = deps.db.lock().await;
            db.upsert_workspace(&Workspace {
                id: "ws".into(),
                name: "ws".into(),
                root_path: "/tmp".into(),
                roots: vec!["/tmp".into()],
            })
            .unwrap();
            db.upsert_session(&SessionMeta {
                id: session_id.clone(),
                workspace_id: "ws".into(),
                title: "t".into(),
                shell: "/bin/sh".into(),
                cwd: "/tmp".into(),
                cols: 80,
                rows: 24,
                lifecycle: bpa_protocol::SessionLifecycle::Exited {
                    code: Some(0),
                    signal: None,
                },
                waiting_for_input: false,
                is_active: false,
                created_at: 1_700_000_000,
            })
            .unwrap();
            // 3 started/finished pairs, ascending seq/ts — seq 0..=5.
            for i in 0..3i64 {
                db.append_command_event(
                    &session_id,
                    i * 2,
                    1_700_000_000 + i,
                    "started",
                    None,
                    "gui",
                )
                .unwrap();
                db.append_command_event(
                    &session_id,
                    i * 2 + 1,
                    1_700_000_000 + i,
                    "finished",
                    Some(i as u8),
                    "gui",
                )
                .unwrap();
            }
        }

        let mut c = UnixStream::connect(&path).await.unwrap();
        assert!(matches!(
            preamble(&mut c).await,
            DaemonReply::Accepted { .. }
        ));
        send_frame(
            &mut c,
            &Frame::Request {
                id: 1,
                req: Request::GetCommandEvents {
                    session_id: session_id.clone(),
                    limit: 4,
                },
            },
        )
        .await;
        match recv_frame_t(&mut c).await {
            Frame::Response {
                id: 1,
                res: Response::CommandEvents(events),
            } => {
                assert_eq!(events.len(), 4, "limit must be respected");
                let seqs: Vec<i64> = events.iter().map(|e| e.seq).collect();
                assert_eq!(seqs, vec![5, 4, 3, 2], "must be newest-first (seq DESC)");
                assert_eq!(events[0].kind, "finished");
                assert_eq!(events[0].exit_code, Some(2));
                assert!(events.iter().all(|e| e.session_id == session_id));
            }
            other => panic!("expected CommandEvents, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn create_session_persists_and_get_reflects_it() {
        let (path, _tx, _jh, _d, _r) = spawn_server().await;
        let mut c = UnixStream::connect(&path).await.unwrap();
        assert!(matches!(
            preamble(&mut c).await,
            DaemonReply::Accepted { .. }
        ));

        // SES-1/SES-4: CreateSession now requires a real, existing workspace — create one first.
        let ws = create_workspace(&mut c, 100, "ws", "/tmp").await;
        // Use /bin/sh (unrecognized by classify_shell) so no integration assets are needed and the
        // resolution is deterministic; cwd=/tmp exists.
        send_frame(
            &mut c,
            &Frame::Request {
                id: 1,
                req: Request::CreateSession {
                    workspace_id: ws.id.clone(),
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
                Frame::Response {
                    id: 1,
                    res: Response::Session(meta),
                } => {
                    assert_eq!(meta.cols, 80);
                    assert_eq!(meta.rows, 24);
                    break meta.id;
                }
                Frame::Response {
                    id: 1,
                    res: Response::Error { code, message },
                } => {
                    panic!("create failed: {code}: {message}");
                }
                Frame::Push(_) => continue,
                other => panic!("unexpected frame {other:?}"),
            }
        };

        // GetSessionState returns the live session.
        send_frame(
            &mut c,
            &Frame::Request {
                id: 2,
                req: Request::GetSessionState {
                    session_id: session_id.clone(),
                },
            },
        )
        .await;
        loop {
            match recv_frame(&mut c).await {
                Frame::Response {
                    id: 2,
                    res: Response::Session(meta),
                } => {
                    assert_eq!(meta.id, session_id);
                    break;
                }
                Frame::Push(_) => continue,
                other => panic!("expected Session, got {other:?}"),
            }
        }

        // ListSessions reflects the persisted session too.
        send_frame(
            &mut c,
            &Frame::Request {
                id: 3,
                req: Request::ListSessions,
            },
        )
        .await;
        loop {
            match recv_frame(&mut c).await {
                Frame::Response {
                    id: 3,
                    res: Response::Sessions(v),
                } => {
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
            &Frame::Request {
                id: 4,
                req: Request::KillSession { session_id },
            },
        )
        .await;
    }

    #[tokio::test]
    async fn create_session_rejects_missing_cwd() {
        let (path, _tx, _jh, _d, _r) = spawn_server().await;
        let mut c = UnixStream::connect(&path).await.unwrap();
        assert!(matches!(
            preamble(&mut c).await,
            DaemonReply::Accepted { .. }
        ));
        // SES-1/SES-4: the workspace must exist before cwd validation is even reached.
        let ws = create_workspace(&mut c, 100, "ws", "/tmp").await;
        send_frame(
            &mut c,
            &Frame::Request {
                id: 1,
                req: Request::CreateSession {
                    workspace_id: ws.id.clone(),
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
            Frame::Response {
                id: 1,
                res: Response::Error { code, .. },
            } => {
                assert_eq!(code, "CwdMissing");
            }
            other => panic!("expected CwdMissing error, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn create_session_rejects_relative_cwd() {
        let (path, _tx, _jh, _d, _r) = spawn_server().await;
        let mut c = UnixStream::connect(&path).await.unwrap();
        assert!(matches!(
            preamble(&mut c).await,
            DaemonReply::Accepted { .. }
        ));
        // SES-1/SES-4: the workspace must exist before cwd validation is even reached.
        let ws = create_workspace(&mut c, 100, "ws", "/tmp").await;
        // A relative path that does not exist relative to the daemon's cwd ⇒ rejected.
        send_frame(
            &mut c,
            &Frame::Request {
                id: 1,
                req: Request::CreateSession {
                    workspace_id: ws.id.clone(),
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
            Frame::Response {
                id: 1,
                res: Response::Error { code, .. },
            } => {
                assert_eq!(code, "CwdMissing");
            }
            other => panic!("expected CwdMissing error for relative cwd, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn attach_first_push_is_replay_then_output() {
        let (path, _tx, _jh, _d, _r) = spawn_server().await;
        let mut c = UnixStream::connect(&path).await.unwrap();
        assert!(matches!(
            preamble(&mut c).await,
            DaemonReply::Accepted { .. }
        ));

        // SES-1/SES-4: CreateSession now requires a real, existing workspace — create one first.
        let ws = create_workspace(&mut c, 100, "ws", "/tmp").await;
        // Create a shell that waits for a go-signal, so we can attach before any output.
        // We drive the child via WriteStdin over the same connection.
        send_frame(
            &mut c,
            &Frame::Request {
                id: 1,
                req: Request::CreateSession {
                    workspace_id: ws.id.clone(),
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
                Frame::Response {
                    id: 1,
                    res: Response::Session(meta),
                } => break meta.id,
                Frame::Response {
                    id: 1,
                    res: Response::Error { code, message },
                } => {
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
                Frame::Response {
                    id: 2,
                    res: Response::Ack,
                } => break,
                Frame::Push(_) => continue,
                other => panic!("expected Ack for write, got {other:?}"),
            }
        }

        send_frame(
            &mut c,
            &Frame::Request {
                id: 3,
                req: Request::AttachSession {
                    session_id: session_id.clone(),
                },
            },
        )
        .await;

        // The FIRST push for this session must be a Replay; then Output flows after we release it.
        // Collect frames: expect Ack(id 3) + Replay push (order may interleave), then release + Output.
        let mut got_ack = false;
        let mut got_replay = false;
        for _ in 0..4 {
            match recv_frame(&mut c).await {
                Frame::Response {
                    id: 3,
                    res: Response::Ack,
                } => got_ack = true,
                Frame::Push(Push::Replay {
                    session_id: sid, ..
                }) => {
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
        assert!(
            got_ack && got_replay,
            "attach must Ack and deliver Replay first"
        );

        // Release the child; Output with the printed text must follow.
        send_frame(
            &mut c,
            &Frame::Request {
                id: 4,
                req: Request::WriteStdin {
                    session_id: session_id.clone(),
                    bytes: b"go\n".to_vec(),
                },
            },
        )
        .await;

        let mut collected = Vec::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            match tokio::time::timeout(std::time::Duration::from_millis(500), recv_frame(&mut c))
                .await
            {
                Ok(Frame::Push(Push::Output {
                    session_id: sid,
                    bytes,
                })) if sid == session_id => {
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

        send_frame(
            &mut c,
            &Frame::Request {
                id: 5,
                req: Request::KillSession { session_id },
            },
        )
        .await;
    }

    #[tokio::test]
    async fn slow_client_is_disconnected_without_stalling_a_second_client() {
        let (path, _tx, _jh, _d, _r) = spawn_server().await;

        // Client A connects, handshakes, then STOPS reading — we flood its outq with replies.
        let mut a = UnixStream::connect(&path).await.unwrap();
        assert!(matches!(
            preamble(&mut a).await,
            DaemonReply::Accepted { .. }
        ));

        for id in 0..(CLIENT_OUTQ_CAP as u64 + 512) {
            let f = Frame::Request {
                id,
                req: Request::ListWorkspaces,
            };
            // `encode_frame` already prepends the u32-LE length prefix — write its output
            // verbatim, do NOT add a second prefix on top (the previous raw-codec serializer had
            // no framing of its own, hence the hand-rolled prefix this call site used to need).
            let bytes = encode_frame(&f).unwrap();
            if a.write_all(&bytes).await.is_err() {
                break;
            }
            let _ = a.flush().await;
        }

        // Client B connects fresh and MUST be served normally.
        let mut b = UnixStream::connect(&path).await.unwrap();
        assert!(matches!(
            preamble(&mut b).await,
            DaemonReply::Accepted { .. }
        ));
        send_frame(
            &mut b,
            &Frame::Request {
                id: 1,
                req: Request::ListWorkspaces,
            },
        )
        .await;
        match tokio::time::timeout(std::time::Duration::from_secs(2), recv_frame(&mut b)).await {
            Ok(Frame::Response {
                id: 1,
                res: Response::Workspaces(_),
            }) => {}
            Ok(other) => panic!("B expected Workspaces, got {other:?}"),
            Err(_) => panic!("B was stalled by A's backpressure — bounded-outq isolation broken"),
        }
    }

    // ---- D4 (Important, pre-existing but amplified by multi-attach): a transient overflow of THE
    // PUSH-FORWARDING PATH specifically (not the request/response path the test above already
    // covers) must disconnect the slow client honestly rather than silently killing only the push
    // pipe while the connection stays "up". Attach a session that floods live `Push::Output`, never
    // read from the socket, and drive the forwarder's `try_send` into `CLIENT_OUTQ_CAP` overflow —
    // then assert the socket observes EOF (the connection was actually torn down), and that a FRESH
    // connection can attach the SAME session and receive pushes normally (no lingering
    // daemon-side damage from the disconnected client). ----
    #[tokio::test]
    async fn push_forwarder_overflow_disconnects_the_slow_client_and_a_fresh_reattach_recovers() {
        let (path, _tx, _jh, _d, _r) = spawn_server().await;

        // ---- Client A: create + attach a session that floods output continuously, then STOP
        // reading entirely so `AttachRegistry`'s forwarder backs up `push_sink` -> `out_tx` until
        // `make_push_sink`'s `try_send` overflows `CLIENT_OUTQ_CAP`. ----
        let mut a = UnixStream::connect(&path).await.unwrap();
        assert!(matches!(
            preamble(&mut a).await,
            DaemonReply::Accepted { .. }
        ));
        // SES-1/SES-4: CreateSession now requires a real, existing workspace — create one first.
        let ws = create_workspace(&mut a, 100, "ws", "/tmp").await;
        send_frame(
            &mut a,
            &Frame::Request {
                id: 1,
                req: Request::CreateSession {
                    workspace_id: ws.id.clone(),
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
            match recv_frame_t(&mut a).await {
                Frame::Response {
                    id: 1,
                    res: Response::Session(m),
                } => break m.id,
                Frame::Response {
                    id: 1,
                    res: Response::Error { code, message },
                } => panic!("create failed: {code}: {message}"),
                Frame::Push(_) => continue,
                other => panic!("unexpected {other:?}"),
            }
        };
        send_frame(
            &mut a,
            &Frame::Request {
                id: 2,
                req: Request::AttachSession {
                    session_id: session_id.clone(),
                },
            },
        )
        .await;
        // Drain the Ack + initial Replay so both are off the wire before we stop reading.
        let (mut ack, mut replay) = (false, false);
        for _ in 0..4 {
            match recv_frame_t(&mut a).await {
                Frame::Response {
                    id: 2,
                    res: Response::Ack,
                } => ack = true,
                Frame::Push(Push::Replay { .. }) => replay = true,
                Frame::Push(_) => continue,
                other => panic!("unexpected before attach settle {other:?}"),
            }
            if ack && replay {
                break;
            }
        }
        assert!(ack && replay, "attach must Ack and deliver Replay first");

        // Flood a large volume of output so the reader thread keeps feeding `Push::Output` frames
        // into A's forwarder far faster than a non-reading client can ever drain — this is what
        // eventually overflows `CLIENT_OUTQ_CAP` on the push path specifically.
        send_frame(
            &mut a,
            &Frame::Request {
                id: 3,
                req: Request::WriteStdin {
                    session_id: session_id.clone(),
                    bytes: b"yes | head -c 20000000\n".to_vec(),
                },
            },
        )
        .await;

        // STOP reading entirely for a while: the socket is left open but nothing drains it, so
        // the kernel send buffer fills, `wr.write_all` in the writer task blocks, `out_tx` backs
        // up to its `CLIENT_OUTQ_CAP` bound, and `make_push_sink`'s forwarder's `try_send` starts
        // returning `Full`. Genuinely NOT reading (not even to discard) is what creates the
        // backpressure — a loop that keeps calling `read()` (even discarding the bytes) would
        // keep draining the kernel buffer and the queue would never actually fill.
        tokio::time::sleep(std::time::Duration::from_secs(3)).await;

        // NOW drain whatever backlog is sitting in the kernel buffer; once that backlog is
        // exhausted the daemon must have already torn the connection down (D4), so the read loop
        // terminates in EOF (or an error) rather than blocking forever waiting for more.
        let eof_seen = tokio::time::timeout(std::time::Duration::from_secs(20), async {
            let mut buf = [0u8; 65536];
            loop {
                match a.read(&mut buf).await {
                    Ok(0) => return true,  // EOF: the daemon closed the connection
                    Ok(_) => continue,     // still draining backlog; keep going until EOF or error
                    Err(_) => return true, // reset/closed also counts as "connection gone"
                }
            }
        })
        .await;
        assert_eq!(
            eof_seen,
            Ok(true),
            "a push-forwarder outbound-queue overflow must disconnect the slow client (EOF), \
             not leave the connection silently half-alive"
        );

        // ---- A fresh connection must be able to attach and receive pushes normally — proving the
        // daemon-side connection-registry/writer-task machinery is NOT corrupted by A's forced
        // disconnect (no lingering half-torn-down state blocking a fresh connection). The FLOODING
        // session's child (`yes | head -c ...`) is still running at this point (session keep-alive,
        // spec §7) and would immediately re-trigger the SAME overflow against any new slow reader —
        // that repeat trip is D4 working as intended, not a regression, but it would make THIS
        // specific assertion (does a fresh connection recover) indistinguishable from a second
        // overflow. Kill the flooding session first, then prove recovery on a brand-new QUIET
        // session — a clean, deterministic proof that the daemon itself is healthy post-disconnect. ----
        let mut b = UnixStream::connect(&path).await.unwrap();
        assert!(matches!(
            preamble(&mut b).await,
            DaemonReply::Accepted { .. }
        ));
        send_frame(
            &mut b,
            &Frame::Request {
                id: 1,
                req: Request::KillSession {
                    session_id: session_id.clone(),
                },
            },
        )
        .await;
        loop {
            match recv_frame_t(&mut b).await {
                Frame::Response {
                    id: 1,
                    res: Response::Ack,
                } => break,
                Frame::Push(_) => continue,
                other => panic!("expected Ack killing the flooding session, got {other:?}"),
            }
        }

        send_frame(
            &mut b,
            &Frame::Request {
                id: 2,
                req: Request::CreateSession {
                    workspace_id: ws.id.clone(),
                    shell: Some("/bin/sh".into()),
                    cwd: Some("/tmp".into()),
                    env_overrides: vec![],
                    cols: 80,
                    rows: 24,
                },
            },
        )
        .await;
        let quiet_id = loop {
            match recv_frame_t(&mut b).await {
                Frame::Response {
                    id: 2,
                    res: Response::Session(m),
                } => break m.id,
                Frame::Response {
                    id: 2,
                    res: Response::Error { code, message },
                } => panic!("create failed: {code}: {message}"),
                Frame::Push(_) => continue,
                other => panic!("unexpected {other:?}"),
            }
        };
        send_frame(
            &mut b,
            &Frame::Request {
                id: 3,
                req: Request::AttachSession {
                    session_id: quiet_id.clone(),
                },
            },
        )
        .await;
        let (mut b_ack, mut b_replay) = (false, false);
        for _ in 0..4 {
            match recv_frame_t(&mut b).await {
                Frame::Response {
                    id: 3,
                    res: Response::Ack,
                } => b_ack = true,
                Frame::Push(Push::Replay {
                    session_id: sid, ..
                }) if sid == quiet_id => b_replay = true,
                Frame::Push(_) => continue,
                other => panic!("unexpected before attach settle {other:?}"),
            }
            if b_ack && b_replay {
                break;
            }
        }
        assert!(
            b_ack && b_replay,
            "a fresh connection must be able to attach a session after A's forced disconnect \
             (b_ack={b_ack}, b_replay={b_replay})"
        );

        // And live Output actually reaches B (the daemon's push fan-out is fully healthy
        // post-disconnect, not just its request/response path).
        send_frame(
            &mut b,
            &Frame::Request {
                id: 4,
                req: Request::WriteStdin {
                    session_id: quiet_id.clone(),
                    bytes: b"printf 'RECOVERY_MARKER\\n'\n".to_vec(),
                },
            },
        )
        .await;
        let mut collected = Vec::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            match tokio::time::timeout(std::time::Duration::from_millis(500), recv_frame(&mut b))
                .await
            {
                Ok(Frame::Push(Push::Output { bytes, .. })) => {
                    collected.extend_from_slice(&bytes);
                    if collected.windows(15).any(|w| w == b"RECOVERY_MARKER") {
                        break;
                    }
                }
                Ok(_) => continue,
                Err(_) => continue,
            }
        }
        assert!(
            collected.windows(15).any(|w| w == b"RECOVERY_MARKER"),
            "fresh connection must receive live pushes after recovering from A's overflow \
             disconnect, got: {collected:?}"
        );

        send_frame(
            &mut b,
            &Frame::Request {
                id: 5,
                req: Request::KillSession {
                    session_id: quiet_id,
                },
            },
        )
        .await;
    }

    #[tokio::test]
    async fn oversized_frame_is_rejected() {
        let (path, _tx, _jh, _d, _r) = spawn_server().await;
        let mut c = UnixStream::connect(&path).await.unwrap();
        assert!(matches!(
            preamble(&mut c).await,
            DaemonReply::Accepted { .. }
        ));
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
        assert!(matches!(
            preamble(&mut c).await,
            DaemonReply::Accepted { .. }
        ));

        for (id, req) in [
            (
                1u64,
                Request::WriteStdin {
                    session_id: "ghost".into(),
                    bytes: vec![1],
                },
            ),
            (
                2,
                Request::Resize {
                    session_id: "ghost".into(),
                    cols: 10,
                    rows: 10,
                },
            ),
            (
                3,
                Request::KillSession {
                    session_id: "ghost".into(),
                },
            ),
        ] {
            send_frame(&mut c, &Frame::Request { id, req }).await;
            match recv_frame(&mut c).await {
                Frame::Response {
                    id: rid,
                    res: Response::Error { code, .. },
                } => {
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
        assert!(matches!(
            preamble(&mut c).await,
            DaemonReply::Accepted { .. }
        ));

        send_frame(
            &mut c,
            &Frame::Request {
                id: 1,
                req: Request::DetachSession {
                    session_id: "ghost".into(),
                },
            },
        )
        .await;
        assert!(matches!(
            recv_frame(&mut c).await,
            Frame::Response {
                id: 1,
                res: Response::Ack
            }
        ));

        send_frame(
            &mut c,
            &Frame::Request {
                id: 2,
                req: Request::DaemonShutdown { drain: false },
            },
        )
        .await;
        assert!(matches!(
            recv_frame(&mut c).await,
            Frame::Response {
                id: 2,
                res: Response::Ack
            }
        ));
    }

    // ---- Task 9 (Pv2 §6.1): `DaemonShutdown{drain}` real semantics — flush + graceful exit,
    // replacing the old no-op Ack. `drain:true` must (a) persist scrollback (+ command_events) for
    // every live session, (b) still Ack the requesting client, and (c) stop `serve()` (the SAME
    // shutdown watch a SIGTERM flips), all in that order — the flush happens BEFORE the watch flips,
    // and the watch flip happens BEFORE `dispatch` returns the `Ack` a caller-side test can observe. ----
    #[tokio::test]
    async fn daemon_shutdown_drain_flushes_then_exits() {
        // Spawn the server manually (rather than via `spawn_server()`) so the test keeps its own
        // `Arc<ServerDeps>` handle — needed below to assert directly against `deps.db` after
        // `serve()` has returned and this connection is gone.
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("d.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let (tx, rx) = tokio::sync::watch::channel(false);
        let (deps, _runtime) = test_deps_with_shutdown(tx.clone());
        let jh = tokio::spawn({
            let deps = deps.clone();
            async move {
                let _ = serve(listener, deps, rx).await;
            }
        });

        let mut c = UnixStream::connect(&path).await.unwrap();
        assert!(matches!(
            preamble(&mut c).await,
            DaemonReply::Accepted { .. }
        ));

        // A real workspace row first: `session.workspace_id` is `NOT NULL REFERENCES
        // workspace(id)` with `foreign_keys = ON` (persistence.rs), so `upsert_session` below would
        // otherwise fail its (best-effort, silently-swallowed) FK check and never persist — which
        // would make this test pass for the wrong reason (an always-empty scrollback looking like
        // "the arm didn't flush" instead of "the row was never insertable").
        send_frame(
            &mut c,
            &Frame::Request {
                id: 1,
                req: Request::CreateWorkspace {
                    name: "w".into(),
                    root_path: "/tmp".into(),
                },
            },
        )
        .await;
        let workspace_id = loop {
            match recv_frame(&mut c).await {
                Frame::Response {
                    id: 1,
                    res: Response::Workspace(w),
                } => break w.id,
                Frame::Push(_) => continue,
                other => panic!("unexpected frame {other:?}"),
            }
        };

        // Create a session and make it emit output (a scrollback sweep persists whatever's in the
        // ring at flush time — `printf` is instant, so by the time we send DaemonShutdown the
        // supervisor's in-memory ring already has the bytes).
        send_frame(
            &mut c,
            &Frame::Request {
                id: 2,
                req: Request::CreateSession {
                    workspace_id,
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
                Frame::Response {
                    id: 2,
                    res: Response::Session(meta),
                } => break meta.id,
                Frame::Response {
                    id: 2,
                    res: Response::Error { code, message },
                } => panic!("create failed: {code}: {message}"),
                Frame::Push(_) => continue,
                other => panic!("unexpected frame {other:?}"),
            }
        };

        // Attach so we can observe live `Output` pushes: the PTY reader thread appends to the same
        // scrollback ring it pushes from, so seeing the marker in an `Output` push is proof the ring
        // already holds those bytes — no arbitrary sleep-and-hope needed before triggering drain.
        send_frame(
            &mut c,
            &Frame::Request {
                id: 3,
                req: Request::AttachSession {
                    session_id: session_id.clone(),
                },
            },
        )
        .await;
        loop {
            match recv_frame(&mut c).await {
                Frame::Response {
                    id: 3,
                    res: Response::Ack,
                } => break,
                Frame::Push(_) => continue,
                other => panic!("expected Ack for attach, got {other:?}"),
            }
        }

        send_frame(
            &mut c,
            &Frame::Request {
                id: 4,
                req: Request::WriteStdin {
                    session_id: session_id.clone(),
                    bytes: b"printf 'DRAIN_MARKER\\n'\n".to_vec(),
                },
            },
        )
        .await;
        let mut collected = Vec::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            match tokio::time::timeout(std::time::Duration::from_millis(500), recv_frame(&mut c))
                .await
            {
                Ok(Frame::Push(Push::Output {
                    session_id: sid,
                    bytes,
                })) if sid == session_id => {
                    collected.extend_from_slice(&bytes);
                    if collected.windows(12).any(|w| w == b"DRAIN_MARKER") {
                        break;
                    }
                }
                Ok(Frame::Response {
                    id: 4,
                    res: Response::Ack,
                }) => continue,
                Ok(_) => continue,
                Err(_) => continue,
            }
        }
        assert!(
            collected.windows(12).any(|w| w == b"DRAIN_MARKER"),
            "expected live Output containing DRAIN_MARKER before triggering drain, got: {collected:?}"
        );

        send_frame(
            &mut c,
            &Frame::Request {
                id: 5,
                req: Request::DaemonShutdown { drain: true },
            },
        )
        .await;

        // (1) The client must actually receive the Ack — proving flush-then-Ack-then-exit ordering
        // doesn't drop or race the reply out from under the connection.
        assert!(
            matches!(
                recv_frame_t(&mut c).await,
                Frame::Response {
                    id: 5,
                    res: Response::Ack
                }
            ),
            "DaemonShutdown{{drain:true}} must Ack before the connection is torn down"
        );

        // (3) serve()'s accept loop must stop (graceful exit) — bounded so a regression (the arm
        // never flipping the shared watch) fails fast instead of hanging the suite.
        tokio::time::timeout(std::time::Duration::from_secs(3), jh)
            .await
            .expect("serve() must return after DaemonShutdown{drain:true} (graceful exit)")
            .expect("serve() task must not panic");

        // (2) The session's scrollback must be persisted in the DB — proving the arm actually ran
        // the flush routine rather than just Acking (the old no-op behavior).
        let db = deps.db.lock().await;
        let scrollback = db.load_scrollback(&session_id).expect("load_scrollback");
        assert!(
            scrollback
                .windows(b"DRAIN_MARKER".len())
                .any(|w| w == b"DRAIN_MARKER"),
            "expected DaemonShutdown{{drain:true}} to have persisted scrollback containing \
             DRAIN_MARKER, got {} bytes: {:?}",
            scrollback.len(),
            String::from_utf8_lossy(&scrollback)
        );
    }

    #[tokio::test]
    async fn daemon_shutdown_no_drain_exits_without_flush() {
        let (path, _tx, jh, _d, _r) = spawn_server().await;
        let mut c = UnixStream::connect(&path).await.unwrap();
        assert!(matches!(
            preamble(&mut c).await,
            DaemonReply::Accepted { .. }
        ));

        send_frame(
            &mut c,
            &Frame::Request {
                id: 1,
                req: Request::DaemonShutdown { drain: false },
            },
        )
        .await;
        assert!(matches!(
            recv_frame_t(&mut c).await,
            Frame::Response {
                id: 1,
                res: Response::Ack
            }
        ));

        tokio::time::timeout(std::time::Duration::from_secs(3), jh)
            .await
            .expect("serve() must return after DaemonShutdown{drain:false} (graceful exit)")
            .expect("serve() task must not panic");
    }

    // ---- D1 (Important): the periodic flush sweep must NOT re-write a cold-rehydrated INACTIVE
    // session's scrollback blob — its persisted bytes are already the final/immutable copy (no
    // live reader will ever change them again). Pre-fix, `flush_scrollback_once` iterated every
    // row `db.list_sessions()` returned regardless of liveness, so N accumulated dead rehydrated
    // sessions meant N needless SQLite writes every tick, forever. This proves the live/inactive
    // split via the `ts` column `append_scrollback` upserts on every write: an unchanged `ts`
    // across two sweeps means the inactive session's row was genuinely skipped. ----
    #[tokio::test]
    async fn flush_skips_inactive_rehydrated_session_but_still_flushes_a_live_one() {
        let (deps, _rt) = test_deps();

        // ---- Inactive (cold-rehydrated) session: persisted directly, then rehydrated into the
        // supervisor as a PTY-less, replay-only entry — exactly the boot::cold_rehydrate_sessions
        // path, without needing a real process restart. ----
        let dead_id = "dead-rehydrated".to_string();
        let dead_meta = SessionMeta {
            id: dead_id.clone(),
            workspace_id: "ws".into(),
            title: "t".into(),
            shell: "/bin/sh".into(),
            cwd: "/tmp".into(),
            cols: 80,
            rows: 24,
            lifecycle: bpa_protocol::SessionLifecycle::Exited {
                code: Some(0),
                signal: None,
            },
            waiting_for_input: false,
            is_active: false,
            created_at: 1_700_000_000,
        };
        {
            let db = deps.db.lock().await;
            db.upsert_workspace(&bpa_protocol::Workspace {
                id: "ws".into(),
                name: "ws".into(),
                root_path: "/tmp".into(),
                roots: vec!["/tmp".into()],
            })
            .unwrap();
            db.upsert_session(&dead_meta).unwrap();
            db.append_scrollback(&dead_id, 0, b"OLD_PERSISTED_BYTES", 111)
                .unwrap();
        }
        deps.supervisor
            .rehydrate_inactive(dead_meta, b"OLD_PERSISTED_BYTES".to_vec())
            .expect("rehydrate_inactive");

        // ---- Live session: created for real, with an attach so its ring definitely has bytes.
        // `workspace_id` must match the "ws" row seeded above — `session.workspace_id` is `NOT
        // NULL REFERENCES workspace(id)` with `foreign_keys = ON`, so an unmatched id would make
        // `upsert_session` fail its (best-effort, silently-swallowed) FK check and never persist,
        // which would make this test pass for the wrong reason. ----
        let mut live_spec = sh_spec("printf LIVE_BYTES; read _hold");
        live_spec.workspace_id = "ws".into();
        let live_id = deps
            .supervisor
            .create(live_spec)
            .expect("create live session");
        // Persist the create-time row, mirroring the production `CreateSession` dispatch arm
        // (`socket_server.rs`'s `Request::CreateSession` handler) — the flush sweep keeps an
        // already-persisted live session's row FRESH; it is not the only place a session's row
        // is ever written.
        {
            let db = deps.db.lock().await;
            let meta = deps.supervisor.meta(&live_id).unwrap();
            db.upsert_session(&meta).unwrap();
        }
        // Give the reader thread a moment to feed the ring so the flush snapshot is non-empty.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            let (_c, _r, bytes) = deps.supervisor.snapshot_scrollback(&live_id).unwrap();
            if bytes.windows(10).any(|w| w == b"LIVE_BYTES") {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "live session's scrollback never filled"
            );
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }

        // ---- First sweep: seeds the live row's scrollback and leaves the dead row exactly as
        // pre-seeded. ----
        flush_scrollback_once(&deps).await;

        let dead_ts_after_first = {
            let db = deps.db.lock().await;
            db.scrollback_row_ts_for_test(&dead_id)
                .unwrap()
                .expect("dead row must still exist")
        };
        assert_eq!(
            dead_ts_after_first, 111,
            "an inactive rehydrated session's scrollback row must NOT be re-written by a flush \
             tick — ts must stay at its pre-seeded value"
        );
        let live_ts_after_first = {
            let db = deps.db.lock().await;
            db.scrollback_row_ts_for_test(&live_id)
                .unwrap()
                .expect("live row must have been written by the first sweep")
        };

        // ---- Second sweep, forced to a distinguishable later timestamp boundary: the live
        // session's row must be written AGAIN (ts advances or content is confirmed live), while
        // the dead row must remain untouched. ----
        tokio::time::sleep(std::time::Duration::from_millis(1100)).await;
        flush_scrollback_once(&deps).await;

        let dead_ts_after_second = {
            let db = deps.db.lock().await;
            db.scrollback_row_ts_for_test(&dead_id)
                .unwrap()
                .expect("dead row must still exist")
        };
        assert_eq!(
            dead_ts_after_second, 111,
            "a SECOND flush tick must still skip the inactive rehydrated session entirely"
        );

        let live_ts_after_second = {
            let db = deps.db.lock().await;
            db.scrollback_row_ts_for_test(&live_id)
                .unwrap()
                .expect("live row must still exist")
        };
        assert!(
            live_ts_after_second > live_ts_after_first,
            "a live session's scrollback row MUST still be re-written on every flush tick \
             (live_ts_after_first={live_ts_after_first}, live_ts_after_second={live_ts_after_second})"
        );

        let _ = deps.supervisor.kill(&live_id);
    }

    // ---- D2 (Important): the SAME live-only flush sweep must also persist the session's CURRENT
    // meta (cols/rows/cwd/lifecycle), not just its scrollback — otherwise a resize is only ever
    // reflected in-memory (the sole production `upsert_session` call is at CreateSession) and a
    // restart rehydrates stale create-time columns. Create -> resize -> flush -> read the DB row
    // directly and assert cols/rows match the resize. ----
    #[tokio::test]
    async fn flush_persists_resized_cols_rows_for_a_live_session() {
        let (deps, _rt) = test_deps();
        {
            let db = deps.db.lock().await;
            db.upsert_workspace(&bpa_protocol::Workspace {
                id: "ws".into(),
                name: "ws".into(),
                root_path: "/tmp".into(),
                roots: vec!["/tmp".into()],
            })
            .unwrap();
        }

        let mut spec = sh_spec("read _hold");
        spec.workspace_id = "ws".into();
        let id = deps.supervisor.create(spec).expect("create");

        // Persist the create-time row (mirrors the production CreateSession dispatch arm) so the
        // row exists at 80x24 before the resize.
        {
            let db = deps.db.lock().await;
            let meta = deps.supervisor.meta(&id).unwrap();
            assert_eq!((meta.cols, meta.rows), (80, 24));
            db.upsert_session(&meta).unwrap();
        }

        deps.supervisor.resize(&id, 220, 55).expect("resize");

        // The DB row must still say 80x24 until a flush sweep runs (proves the fix isn't a no-op
        // that happened to already be correct via some other path).
        {
            let db = deps.db.lock().await;
            let rows = db.list_sessions().unwrap();
            let row = rows.iter().find(|m| m.id == id).expect("row present");
            assert_eq!(
                (row.cols, row.rows),
                (80, 24),
                "precondition: DB row must still be stale pre-flush"
            );
        }

        flush_scrollback_once(&deps).await;

        let db = deps.db.lock().await;
        let rows = db.list_sessions().unwrap();
        let row = rows.iter().find(|m| m.id == id).expect("row present");
        assert_eq!(
            (row.cols, row.rows),
            (220, 55),
            "a flush sweep must persist the resized cols/rows for a live session"
        );

        let _ = deps.supervisor.kill(&id);
    }

    // ---- BL-124 (lock-poisoning hardening): the periodic flusher must SURVIVE a poisoned
    // supervisor `sessions` map. Pre-hardening, one panic under that guard (anywhere in the
    // process) made every later `.lock().unwrap()` inside `meta()`/`snapshot_scrollback()` panic
    // too — the first `flush_scrollback_once` tick after the panic would itself panic, the
    // flusher task would die, and persistence was silently off for the rest of the daemon's life
    // (the audit's mortal scenario). With poison-tolerant acquisition the sweep must complete and
    // actually persist. ----
    #[tokio::test]
    async fn flush_survives_a_poisoned_supervisor_sessions_map() {
        let (deps, _rt) = test_deps();
        {
            let db = deps.db.lock().await;
            db.upsert_workspace(&bpa_protocol::Workspace {
                id: "ws".into(),
                name: "ws".into(),
                root_path: "/tmp".into(),
                roots: vec!["/tmp".into()],
            })
            .unwrap();
        }

        let mut spec = sh_spec("printf LIVE_BYTES; read _hold");
        spec.workspace_id = "ws".into();
        let id = deps.supervisor.create(spec).expect("create");
        {
            let db = deps.db.lock().await;
            let meta = deps.supervisor.meta(&id).unwrap();
            db.upsert_session(&meta).unwrap();
        }

        // Poison the supervisor's sessions map: a thread panics while holding its guard. Every
        // supervisor method below (`meta`, `snapshot_scrollback`, `drain_command_events`, `kill`)
        // acquires that same mutex on its path.
        deps.supervisor.poison_sessions_map_for_test();

        // The sweep must NOT panic (pre-hardening it would, killing the flusher task) and must
        // genuinely persist: the live session's scrollback row lands in the DB.
        flush_scrollback_once(&deps).await;

        {
            let db = deps.db.lock().await;
            assert!(
                db.scrollback_row_ts_for_test(&id).unwrap().is_some(),
                "the flush sweep must still persist after the sessions map was poisoned"
            );
        }

        // The supervisor itself must also still serve after the poison (not just the flusher).
        assert!(deps.supervisor.meta(&id).unwrap().is_active);
        let _ = deps.supervisor.kill(&id);
    }

    // ---- BL-124 (lock-poisoning hardening): a poisoned `live_sessions` set must not take the
    // dispatch layer down with it. Pre-hardening, `Request::ListSessions`'s
    // `live_sessions.lock().unwrap()` would panic on every call after one panic under that guard
    // — a permanent ListSessions outage (frozen session list in the UI). ----
    #[tokio::test]
    async fn list_sessions_survives_a_poisoned_live_sessions_set() {
        let (deps, _rt) = test_deps();

        // Poison `live_sessions` the same way `Supervisor::poison_sessions_map_for_test` does —
        // a thread panics while holding the guard (raw std API, not the tolerant helper).
        {
            let deps_ref = deps.clone();
            std::thread::scope(|s| {
                let panicked = s
                    .spawn(move || {
                        let _guard = deps_ref.live_sessions.lock().unwrap();
                        panic!("deliberate test panic under the live_sessions guard");
                    })
                    .join();
                assert!(panicked.is_err(), "the poisoning thread must panic");
            });
            assert!(deps.live_sessions.is_poisoned());
        }

        let (sink_tx, _sink_rx) = tokio::sync::mpsc::channel(8);
        let broadcaster = Broadcaster::new();
        let res = dispatch(&deps, 1, &sink_tx, &broadcaster, Request::ListSessions).await;
        assert!(
            matches!(res, Response::Sessions(_)),
            "ListSessions must keep serving after live_sessions was poisoned, got {res:?}"
        );
    }

    // ---- D2: lifecycle/cwd freshness through the same flush-persists-meta path. A session that
    // moves past AtPrompt into Running (via a real OSC-133 C mark) must have that lifecycle
    // reflected in the DB row after a flush sweep, not just in-memory. ----
    #[tokio::test]
    async fn flush_persists_lifecycle_freshness_for_a_live_session() {
        let (deps, _rt) = test_deps();
        {
            let db = deps.db.lock().await;
            db.upsert_workspace(&bpa_protocol::Workspace {
                id: "ws".into(),
                name: "ws".into(),
                root_path: "/tmp".into(),
                roots: vec!["/tmp".into()],
            })
            .unwrap();
        }

        let mut spec = sh_spec("printf '\\033]133;C\\007'; read _hold");
        spec.workspace_id = "ws".into();
        let id = deps.supervisor.create(spec).expect("create");
        {
            let db = deps.db.lock().await;
            let meta = deps.supervisor.meta(&id).unwrap();
            db.upsert_session(&meta).unwrap();
        }

        // Wait for the OSC-133 C mark to drive the in-memory lifecycle to Running.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if deps.supervisor.meta(&id).unwrap().lifecycle
                == bpa_protocol::SessionLifecycle::Running
            {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "lifecycle never advanced to Running"
            );
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }

        flush_scrollback_once(&deps).await;

        let db = deps.db.lock().await;
        // Read the RAW lifecycle tag, not `list_sessions`: the read path deliberately maps a
        // persisted `running` to `Exited { code: None }` (SES-3), which would mask exactly the
        // write-side freshness this test proves — the flush must have written the CURRENT
        // (`running`) tag over the create-time `atPrompt`.
        let tag = db.raw_lifecycle_tag(&id);
        assert_eq!(
            tag, "running",
            "a flush sweep must persist the live lifecycle, not the create-time AtPrompt"
        );

        let _ = deps.supervisor.kill(&id);
    }

    // ---- Round-2 regression R2: a session's FINAL scrollback tail + terminal lifecycle must be
    // persisted on natural exit, even though `flush_scrollback_once`'s periodic sweep now correctly
    // (D1) skips it the instant `is_active` flips to `false`. `install_push_callbacks` must be
    // wired (mirrors `killed_session_attach_entry_is_reaped`) so `on_exited`'s final-flush spawn
    // actually runs — `test_deps()` alone does not register it. ----

    /// Poll until `deps.supervisor.meta(id)` reports `is_active == false` (the wait thread has run)
    /// — bounded so a regression that never reaps hangs the test fast instead of forever.
    async fn wait_until_inactive(deps: &Arc<ServerDeps>, id: &str) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if let Ok(meta) = deps.supervisor.meta(id) {
                if !meta.is_active {
                    return;
                }
            }
            assert!(
                std::time::Instant::now() < deadline,
                "session {id} never went inactive"
            );
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }

    /// Poll the DB directly until `f` returns `Some`, or panic at the deadline. The final flush
    /// (round-2 R2) runs on a spawned task off the wait thread's sync callback, so it is not
    /// synchronous with `wait_until_inactive` above — tests must poll rather than assume the write
    /// has landed the instant `is_active` flips.
    async fn wait_for_db<T, F>(deps: &Arc<ServerDeps>, mut f: F) -> T
    where
        F: FnMut(&Db) -> Option<T>,
    {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            {
                let db = deps.db.lock().await;
                if let Some(v) = f(&db) {
                    return v;
                }
            }
            assert!(
                std::time::Instant::now() < deadline,
                "expected DB state never arrived within the deadline"
            );
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }

    // ---- (a) create -> print marker -> exit naturally (exited-unreaped, no daemon restart) ->
    // the DB scrollback row must contain the marker AND the session row's lifecycle must be
    // Exited. Pre-fix: `flush_scrollback_once`'s `is_active` gate flips false the instant the wait
    // thread reaps the child, racing (and normally beating) the very next periodic tick, so this
    // session's trailing output/terminal lifecycle was NEVER persisted — the row stayed absent (if
    // the create-time flush never ran) or stuck reporting the create-time lifecycle. ----
    #[tokio::test]
    async fn on_exit_flush_persists_final_scrollback_and_exited_lifecycle() {
        let (deps, _rt) = test_deps();
        install_push_callbacks(&deps, Broadcaster::default());
        {
            let db = deps.db.lock().await;
            db.upsert_workspace(&bpa_protocol::Workspace {
                id: "ws".into(),
                name: "ws".into(),
                root_path: "/tmp".into(),
                roots: vec!["/tmp".into()],
            })
            .unwrap();
        }

        let mut spec = sh_spec("printf 'EXIT_MARKER\\n'; exit 7");
        spec.workspace_id = "ws".into();
        let id = deps.supervisor.create(spec).expect("create");
        {
            // Mirror the production CreateSession dispatch arm's immediate persist.
            let db = deps.db.lock().await;
            let meta = deps.supervisor.meta(&id).unwrap();
            db.upsert_session(&meta).unwrap();
        }

        wait_until_inactive(&deps, &id).await;

        let scrollback = wait_for_db(&deps, |db| {
            let bytes = db.load_scrollback(&id).ok()?;
            bytes
                .windows(b"EXIT_MARKER".len())
                .any(|w| w == b"EXIT_MARKER")
                .then_some(bytes)
        })
        .await;
        assert!(
            scrollback
                .windows(b"EXIT_MARKER".len())
                .any(|w| w == b"EXIT_MARKER"),
            "final flush must persist the trailing output emitted right before exit"
        );

        let row = wait_for_db(&deps, |db| {
            db.list_sessions().ok()?.into_iter().find(|m| {
                m.id == id && matches!(m.lifecycle, bpa_protocol::SessionLifecycle::Exited { .. })
            })
        })
        .await;
        match row.lifecycle {
            bpa_protocol::SessionLifecycle::Exited { code, .. } => {
                assert_eq!(code, Some(7), "exit code must be persisted");
            }
            other => panic!("expected Exited lifecycle in the persisted row, got {other:?}"),
        }
    }

    // ---- (b) a session that lives under ONE whole SCROLLBACK_FLUSH_INTERVAL (create, print, exit
    // fast — the periodic sweep never gets a tick against it while it's still live) must still have
    // its scrollback persisted via the on-exit final flush, not just the periodic sweep. ----
    #[tokio::test]
    async fn on_exit_flush_persists_scrollback_for_a_session_shorter_than_one_flush_interval() {
        let (deps, _rt) = test_deps();
        install_push_callbacks(&deps, Broadcaster::default());
        {
            let db = deps.db.lock().await;
            db.upsert_workspace(&bpa_protocol::Workspace {
                id: "ws".into(),
                name: "ws".into(),
                root_path: "/tmp".into(),
                roots: vec!["/tmp".into()],
            })
            .unwrap();
        }

        // No `read _hold`: exits as fast as the shell can print and return — well under
        // SCROLLBACK_FLUSH_INTERVAL (1s), so no periodic tick ever observes this session live.
        let mut spec = sh_spec("printf 'FAST_MARKER\\n'");
        spec.workspace_id = "ws".into();
        let id = deps.supervisor.create(spec).expect("create");
        {
            let db = deps.db.lock().await;
            let meta = deps.supervisor.meta(&id).unwrap();
            db.upsert_session(&meta).unwrap();
        }

        wait_until_inactive(&deps, &id).await;

        let scrollback = wait_for_db(&deps, |db| {
            let bytes = db.load_scrollback(&id).ok()?;
            bytes
                .windows(b"FAST_MARKER".len())
                .any(|w| w == b"FAST_MARKER")
                .then_some(bytes)
        })
        .await;
        assert!(
            scrollback
                .windows(b"FAST_MARKER".len())
                .any(|w| w == b"FAST_MARKER"),
            "a session shorter-lived than one flush interval must still have its scrollback \
             persisted by the on-exit final flush"
        );
    }

    // ---- (c) D1 regression guard: a rehydrated (loaded-from-disk, never-live-this-process)
    // inactive session must still NOT be re-flushed by a periodic tick — the final-flush addition
    // must not reopen the write-amplification bug D1 fixed. `rehydrate_inactive` never spawns a
    // wait thread, so `on_exited` (and thus the new final flush) can never fire for it either;
    // this test proves BOTH mechanisms leave it alone. ----
    #[tokio::test]
    async fn rehydrated_inactive_session_is_not_reflushed_by_periodic_tick_or_final_flush() {
        let (deps, _rt) = test_deps();
        install_push_callbacks(&deps, Broadcaster::default());
        {
            let db = deps.db.lock().await;
            db.upsert_workspace(&bpa_protocol::Workspace {
                id: "ws".into(),
                name: "ws".into(),
                root_path: "/tmp".into(),
                roots: vec!["/tmp".into()],
            })
            .unwrap();
        }

        let dead_id = "dead-r2".to_string();
        let dead_meta = SessionMeta {
            id: dead_id.clone(),
            workspace_id: "ws".into(),
            title: "t".into(),
            shell: "/bin/sh".into(),
            cwd: "/tmp".into(),
            cols: 80,
            rows: 24,
            lifecycle: bpa_protocol::SessionLifecycle::Exited {
                code: Some(0),
                signal: None,
            },
            waiting_for_input: false,
            is_active: false,
            created_at: 1_700_000_000,
        };
        {
            let db = deps.db.lock().await;
            db.upsert_session(&dead_meta).unwrap();
            db.append_scrollback(&dead_id, 0, b"OLD_PERSISTED_BYTES", 222)
                .unwrap();
        }
        deps.supervisor
            .rehydrate_inactive(dead_meta, b"OLD_PERSISTED_BYTES".to_vec())
            .expect("rehydrate_inactive");

        // Give any (incorrect) async final-flush spawn a generous window to have run if it were
        // ever going to — rehydrate_inactive never spawns a wait thread, so on_exited cannot fire,
        // but this also guards against a future regression that called the flush unconditionally
        // from somewhere else.
        tokio::time::sleep(std::time::Duration::from_millis(200)).await;
        flush_scrollback_once(&deps).await;
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;

        let db = deps.db.lock().await;
        let ts = db
            .scrollback_row_ts_for_test(&dead_id)
            .unwrap()
            .expect("dead row must still exist");
        assert_eq!(
            ts, 222,
            "a rehydrated (never-live-this-process) inactive session must not be re-flushed by \
             either the periodic sweep or the on-exit final flush"
        );
    }

    // ---- (d) resize -> exit -> read row directly: persisted cols/rows must reflect the resize
    // AND lifecycle must be Exited — the final flush must persist the CURRENT meta (like the
    // periodic sweep's D2 fix), not a stale create-time snapshot. ----
    #[tokio::test]
    async fn on_exit_flush_persists_resized_cols_rows_and_exited_lifecycle() {
        let (deps, _rt) = test_deps();
        install_push_callbacks(&deps, Broadcaster::default());
        {
            let db = deps.db.lock().await;
            db.upsert_workspace(&bpa_protocol::Workspace {
                id: "ws".into(),
                name: "ws".into(),
                root_path: "/tmp".into(),
                roots: vec!["/tmp".into()],
            })
            .unwrap();
        }

        let mut spec = sh_spec("read _hold");
        spec.workspace_id = "ws".into();
        let id = deps.supervisor.create(spec).expect("create");
        {
            let db = deps.db.lock().await;
            let meta = deps.supervisor.meta(&id).unwrap();
            assert_eq!((meta.cols, meta.rows), (80, 24));
            db.upsert_session(&meta).unwrap();
        }

        deps.supervisor.resize(&id, 132, 43).expect("resize");
        // `kill()` synchronously joins the wait thread (which runs `on_exited` up through spawning
        // the final flush) AND removes the session from the supervisor's live map before returning
        // — unlike a natural exit, there is no `Ok(meta.is_active == false)` state to poll for here,
        // `meta(&id)` goes straight to `Err(NoSuchSession)`. Go straight to polling the DB below.
        deps.supervisor.kill(&id).expect("kill");

        let row = wait_for_db(&deps, |db| {
            db.list_sessions().ok()?.into_iter().find(|m| {
                m.id == id
                    && (m.cols, m.rows) == (132, 43)
                    && matches!(m.lifecycle, bpa_protocol::SessionLifecycle::Exited { .. })
            })
        })
        .await;
        assert_eq!((row.cols, row.rows), (132, 43));
        assert!(
            matches!(row.lifecycle, bpa_protocol::SessionLifecycle::Exited { .. }),
            "expected Exited lifecycle in the persisted row, got {:?}",
            row.lifecycle
        );
    }

    // ---- Round-3 hardening H1: `await_pending_final_flushes` must be a genuine barrier — after
    // it returns, every final flush scheduled by an earlier kill has ALREADY landed in the DB. No
    // `wait_for_db` polling here, deliberately: the whole point of the awaited drain is that
    // `boot::run` can checkpoint and exit immediately after it, so this test reads the DB
    // synchronously right after the await and requires the terminal state to be present. Pre-fix
    // (fire-and-forget spawn, nothing to await), the write raced this read nondeterministically —
    // and raced the runtime drop in production. ----
    #[tokio::test]
    async fn await_pending_final_flushes_is_a_barrier_for_killed_sessions_writes() {
        let (deps, _rt) = test_deps();
        install_push_callbacks(&deps, Broadcaster::default());
        {
            let db = deps.db.lock().await;
            db.upsert_workspace(&bpa_protocol::Workspace {
                id: "ws".into(),
                name: "ws".into(),
                root_path: "/tmp".into(),
                roots: vec!["/tmp".into()],
            })
            .unwrap();
        }

        let mut spec = sh_spec("printf 'BARRIER_MARKER\\n'; read _hold");
        spec.workspace_id = "ws".into();
        let id = deps.supervisor.create(spec).expect("create");
        {
            // Mirror the production CreateSession dispatch arm's immediate persist.
            let db = deps.db.lock().await;
            let meta = deps.supervisor.meta(&id).unwrap();
            db.upsert_session(&meta).unwrap();
        }

        // Let the marker land in the in-memory ring before killing (the printf runs immediately;
        // poll the supervisor's own snapshot rather than sleeping blind).
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if let Ok((_c, _r, bytes)) = deps.supervisor.snapshot_scrollback(&id) {
                if bytes
                    .windows(b"BARRIER_MARKER".len())
                    .any(|w| w == b"BARRIER_MARKER")
                {
                    break;
                }
            }
            assert!(
                std::time::Instant::now() < deadline,
                "marker never reached the in-memory ring"
            );
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }

        // Exactly what `boot::run`'s shutdown path does: shutdown_all (kill → on_exited schedules
        // the flush and tracks its handle) then the awaited drain.
        deps.supervisor.shutdown_all();
        deps.await_pending_final_flushes().await;

        // Synchronous read, no polling: the awaited drain must have flushed everything already.
        let db = deps.db.lock().await;
        let row = db
            .list_sessions()
            .unwrap()
            .into_iter()
            .find(|m| m.id == id)
            .expect("row must be present after the awaited drain");
        assert!(
            matches!(row.lifecycle, bpa_protocol::SessionLifecycle::Exited { .. }),
            "awaited drain must have persisted the terminal Exited lifecycle, got {:?}",
            row.lifecycle
        );
        let bytes = db.load_scrollback(&id).unwrap();
        assert!(
            bytes
                .windows(b"BARRIER_MARKER".len())
                .any(|w| w == b"BARRIER_MARKER"),
            "awaited drain must have persisted the final scrollback tail"
        );
    }

    // ---- H1's own no-unbounded-growth guard: the pending-final-flush tracker must not
    // accumulate one dead JoinHandle per session-ever-exited for the daemon's lifetime — finished
    // handles are swept (at track time in production; `pending_final_flush_count` applies the same
    // sweep) so only in-flight flushes are ever retained. ----
    #[tokio::test]
    async fn completed_final_flush_handles_are_swept_not_accumulated() {
        let (deps, _rt) = test_deps();
        install_push_callbacks(&deps, Broadcaster::default());
        {
            let db = deps.db.lock().await;
            db.upsert_workspace(&bpa_protocol::Workspace {
                id: "ws".into(),
                name: "ws".into(),
                root_path: "/tmp".into(),
                roots: vec!["/tmp".into()],
            })
            .unwrap();
        }

        let mut spec = sh_spec("read _hold");
        spec.workspace_id = "ws".into();
        let id = deps.supervisor.create(spec).expect("create");
        {
            let db = deps.db.lock().await;
            let meta = deps.supervisor.meta(&id).unwrap();
            db.upsert_session(&meta).unwrap();
        }

        // `kill()` joins the wait thread, whose `on_exited` schedules + tracks the flush task
        // synchronously — on this (current-thread) test runtime the task cannot have run yet, so
        // exactly one in-flight handle must be tracked here.
        deps.supervisor.kill(&id).expect("kill");
        assert_eq!(
            deps.pending_final_flush_count(),
            1,
            "the just-scheduled final flush must be tracked as in-flight"
        );

        // Once the flush task completes, its handle must be collectable — the count (which sweeps
        // exactly like `track_final_flush` does) must drop back to zero, never grow forever.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if deps.pending_final_flush_count() == 0 {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "completed final-flush handle was never swept"
            );
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
    }

    // ---- Peer-cred: honest same-process test. A cross-uid peer cannot be forged in the sandbox,
    // so we assert the accepted (same-euid) path works end to end; the rejection logic itself is
    // unit-tested in `singleton.rs` (peer_cred_rejects_foreign_uid_simulated). ----
    #[tokio::test]
    async fn peer_cred_same_uid_is_accepted() {
        let (path, _tx, _jh, _d, _r) = spawn_server().await;
        let mut c = UnixStream::connect(&path).await.unwrap();
        // If peer-cred wrongly rejected our own euid, the handshake would never complete.
        assert!(matches!(
            preamble(&mut c).await,
            DaemonReply::Accepted { .. }
        ));
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
        assert!(matches!(
            preamble(&mut c).await,
            DaemonReply::Accepted { .. }
        ));
        send_frame(
            &mut c,
            &Frame::Request {
                id: 9,
                req: Request::CreateWorkspace {
                    name: "esc".into(),
                    root_path: link,
                },
            },
        )
        .await;
        match recv_frame_t(&mut c).await {
            Frame::Response {
                id: 9,
                res: Response::Error { code, .. },
            } => {
                assert_eq!(code, "InvalidWorkspaceRoot");
            }
            other => {
                panic!("expected InvalidWorkspaceRoot for symlink-escaping root, got {other:?}")
            }
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
        assert!(matches!(
            preamble(&mut c).await,
            DaemonReply::Accepted { .. }
        ));
        // SES-1/SES-4: the workspace must exist before cwd validation is even reached.
        let ws = create_workspace(&mut c, 100, "ws", "/tmp").await;
        send_frame(
            &mut c,
            &Frame::Request {
                id: 1,
                req: Request::CreateSession {
                    workspace_id: ws.id.clone(),
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
            Frame::Response {
                id: 1,
                res: Response::Error { code, .. },
            } => {
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
                (
                    "HOME".into(),
                    std::env::var("HOME").unwrap_or_else(|_| "/tmp".into()),
                ),
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
        install_push_callbacks(&deps, Broadcaster::default());

        // Create a live session directly via the supervisor (a long sleep so it stays alive).
        let id = deps.supervisor.create(sh_spec("sleep 5")).expect("create");

        // Attach it (a bounded mpsc sink stands in for a client's push queue).
        let (sink, _client) = mpsc::channel::<Push>(16);
        deps.attach.attach(1, &id, sink).await.expect("attach");
        assert_eq!(
            deps.attach.attachment_count(),
            1,
            "attach registered one entry"
        );

        // Kill joins the wait thread, so on_exited → remove_session has run by the time kill returns.
        deps.supervisor.kill(&id).expect("kill");
        assert_eq!(
            deps.attach.attachment_count(),
            0,
            "killed session's attach entry must be reaped (no orphan)"
        );
    }

    // ---- D3 (Important): KillSession on a rehydrated (PTY-less, inactive) session must be an
    // HONEST CLOSE, not the pre-fix unkillable-zombie behavior — Ack (not NoSuchSession),
    // ListSessions no longer contains it, and its DB rows (session + scrollback) are gone so a
    // second cold-rehydrate never resurrects it. Drives the real dispatch arm over the wire, then
    // asserts directly against the DB (same `deps` the test keeps its own handle to, mirroring
    // `daemon_shutdown_drain_flushes_then_exits`'s pattern for post-`serve()` DB assertions). ----
    #[tokio::test]
    async fn kill_session_on_rehydrated_inactive_session_is_an_honest_close() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("d.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let (tx, rx) = tokio::sync::watch::channel(false);
        let (deps, _runtime) = test_deps_with_shutdown(tx.clone());
        let jh = tokio::spawn({
            let deps = deps.clone();
            async move {
                let _ = serve(listener, deps.clone(), rx).await;
            }
        });

        // Seed a persisted, INACTIVE session directly (mirrors what `boot::cold_rehydrate_sessions`
        // does on a real restart) — a workspace row + session row + scrollback row, then rehydrate
        // it into the running supervisor as a PTY-less replay-only entry.
        let dead_id = "dead-rehydrated-honest-close".to_string();
        let dead_meta = SessionMeta {
            id: dead_id.clone(),
            workspace_id: "ws".into(),
            title: "t".into(),
            shell: "/bin/sh".into(),
            cwd: "/tmp".into(),
            cols: 80,
            rows: 24,
            lifecycle: bpa_protocol::SessionLifecycle::Exited {
                code: Some(0),
                signal: None,
            },
            waiting_for_input: false,
            is_active: false,
            created_at: 1_700_000_000,
        };
        {
            let db = deps.db.lock().await;
            db.upsert_workspace(&Workspace {
                id: "ws".into(),
                name: "ws".into(),
                root_path: "/tmp".into(),
                roots: vec!["/tmp".into()],
            })
            .unwrap();
            db.upsert_session(&dead_meta).unwrap();
            db.append_scrollback(&dead_id, 0, b"OLD_MARKER", 1).unwrap();
        }
        deps.supervisor
            .rehydrate_inactive(dead_meta, b"OLD_MARKER".to_vec())
            .expect("rehydrate_inactive");

        let mut c = UnixStream::connect(&path).await.unwrap();
        assert!(matches!(
            preamble(&mut c).await,
            DaemonReply::Accepted { .. }
        ));

        // KillSession must Ack — NOT NoSuchSession (the pre-fix zombie behavior).
        send_frame(
            &mut c,
            &Frame::Request {
                id: 1,
                req: Request::KillSession {
                    session_id: dead_id.clone(),
                },
            },
        )
        .await;
        match recv_frame_t(&mut c).await {
            Frame::Response {
                id: 1,
                res: Response::Ack,
            } => {}
            other => panic!(
                "KillSession on a rehydrated inactive session must Ack (honest close), got {other:?}"
            ),
        }

        // ListSessions no longer contains it.
        send_frame(
            &mut c,
            &Frame::Request {
                id: 2,
                req: Request::ListSessions,
            },
        )
        .await;
        loop {
            match recv_frame_t(&mut c).await {
                Frame::Response {
                    id: 2,
                    res: Response::Sessions(v),
                } => {
                    assert!(
                        v.iter().all(|m| m.id != dead_id),
                        "killed rehydrated session must no longer appear in ListSessions, got: {v:?}"
                    );
                    break;
                }
                Frame::Push(_) => continue,
                other => panic!("expected Sessions, got {other:?}"),
            }
        }

        // DB rows are gone.
        {
            let db = deps.db.lock().await;
            assert!(
                db.list_sessions().unwrap().iter().all(|m| m.id != dead_id),
                "DB session row must be deleted"
            );
            assert_eq!(
                db.load_scrollback(&dead_id).unwrap(),
                Vec::<u8>::new(),
                "DB scrollback rows must be deleted"
            );
        }

        // A second "daemon restart" (cold-rehydrate against the same DB) must NOT resurrect it.
        let fresh_supervisor = Arc::new(Supervisor::new());
        {
            let db = deps.db.lock().await;
            for meta in db.list_sessions().unwrap() {
                let sb = db.load_scrollback(&meta.id).unwrap_or_default();
                let _ = fresh_supervisor.rehydrate_inactive(meta, sb);
            }
        }
        assert!(
            matches!(
                fresh_supervisor.meta(&dead_id),
                Err(SupervisorError::NoSuchSession(_))
            ),
            "a killed rehydrated session must never be resurrected by a subsequent cold-rehydrate"
        );

        // ---- Also: kill on a LIVE session still works (regression guard on the unchanged path). ----
        send_frame(
            &mut c,
            &Frame::Request {
                id: 3,
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
        let live_id = loop {
            match recv_frame_t(&mut c).await {
                Frame::Response {
                    id: 3,
                    res: Response::Session(m),
                } => break m.id,
                Frame::Response {
                    id: 3,
                    res: Response::Error { code, message },
                } => panic!("create failed: {code}: {message}"),
                Frame::Push(_) => continue,
                other => panic!("unexpected {other:?}"),
            }
        };
        send_frame(
            &mut c,
            &Frame::Request {
                id: 4,
                req: Request::KillSession {
                    session_id: live_id.clone(),
                },
            },
        )
        .await;
        loop {
            match recv_frame_t(&mut c).await {
                Frame::Response {
                    id: 4,
                    res: Response::Ack,
                } => break,
                Frame::Push(_) => continue,
                other => panic!("KillSession on a live session must still Ack, got {other:?}"),
            }
        }
        assert!(
            matches!(
                deps.supervisor.meta(&live_id),
                Err(SupervisorError::NoSuchSession(_))
            ),
            "a killed live session must be fully reaped/removed exactly as before"
        );

        tx.send(true).unwrap();
        let _ = tokio::time::timeout(std::time::Duration::from_secs(3), jh).await;
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
        assert!(matches!(
            preamble(&mut a).await,
            DaemonReply::Accepted { .. }
        ));
        // SES-1/SES-4: CreateSession now requires a real, existing workspace — create one first.
        let ws = create_workspace(&mut a, 100, "ws", "/tmp").await;
        send_frame(
            &mut a,
            &Frame::Request {
                id: 1,
                req: Request::CreateSession {
                    workspace_id: ws.id.clone(),
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
                Frame::Response {
                    id: 1,
                    res: Response::Session(m),
                } => break m.id,
                Frame::Response {
                    id: 1,
                    res: Response::Error { code, message },
                } => {
                    panic!("A create failed: {code}: {message}")
                }
                Frame::Push(_) => continue,
                other => panic!("A unexpected {other:?}"),
            }
        };
        send_frame(
            &mut a,
            &Frame::Request {
                id: 2,
                req: Request::AttachSession {
                    session_id: sa_id.clone(),
                },
            },
        )
        .await;
        // Drain A's Ack + first Replay.
        let (mut a_ack, mut a_replay) = (false, false);
        for _ in 0..4 {
            match recv_frame_t(&mut a).await {
                Frame::Response {
                    id: 2,
                    res: Response::Ack,
                } => a_ack = true,
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
        assert!(matches!(
            preamble(&mut b).await,
            DaemonReply::Accepted { .. }
        ));
        send_frame(
            &mut b,
            &Frame::Request {
                id: 1,
                req: Request::CreateSession {
                    workspace_id: ws.id.clone(),
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
                Frame::Response {
                    id: 1,
                    res: Response::Session(m),
                } => break m.id,
                Frame::Response {
                    id: 1,
                    res: Response::Error { code, message },
                } => {
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
                Frame::Response {
                    id: 2,
                    res: Response::Ack,
                } => break,
                Frame::Push(_) => continue,
                other => panic!("B expected Ack for prime write, got {other:?}"),
            }
        }
        // Attach SB (drain Ack + Replay).
        send_frame(
            &mut b,
            &Frame::Request {
                id: 3,
                req: Request::AttachSession {
                    session_id: sb_id.clone(),
                },
            },
        )
        .await;
        let (mut b_ack, mut b_replay) = (false, false);
        for _ in 0..4 {
            match recv_frame_t(&mut b).await {
                Frame::Response {
                    id: 3,
                    res: Response::Ack,
                } => b_ack = true,
                Frame::Push(Push::Replay { session_id, .. }) if session_id == sb_id => {
                    b_replay = true
                }
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
                req: Request::WriteStdin {
                    session_id: sb_id.clone(),
                    bytes: b"go\n".to_vec(),
                },
            },
        )
        .await;
        let mut b_out = Vec::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            match tokio::time::timeout(std::time::Duration::from_millis(500), recv_frame(&mut b))
                .await
            {
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
                Frame::Response {
                    id: 5,
                    res: Response::Ack,
                } => break,
                Frame::Push(_) => continue,
                other => panic!("B expected Ack for post-disconnect write, got {other:?}"),
            }
        }
        send_frame(
            &mut b,
            &Frame::Request {
                id: 6,
                req: Request::WriteStdin {
                    session_id: sb_id.clone(),
                    bytes: b"go\n".to_vec(),
                },
            },
        )
        .await;
        let mut b_out2 = Vec::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            match tokio::time::timeout(std::time::Duration::from_millis(500), recv_frame(&mut b))
                .await
            {
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
        send_frame(
            &mut b,
            &Frame::Request {
                id: 7,
                req: Request::KillSession { session_id: sb_id },
            },
        )
        .await;
    }

    // ---- Task 7 D5 (spec §5.2-5.4): TWO connections attach the SAME session and BOTH stream
    // independently — the multi-subscriber model, no supersede. A: connect+preamble+attach; B:
    // connect+preamble+attach the SAME session. Both must get their own Replay, then both must
    // see live Output. A detaches → B keeps streaming. A disconnects → B keeps streaming.
    // KillSession → both get ChildExited and both forwarders terminate (no leak: attachment_count
    // returns to 0). ----
    #[tokio::test]
    async fn two_connections_attach_same_session_both_stream_independently() {
        let (path, _tx, _jh, _d, _r) = spawn_server().await;

        // ---- Client A: connect, create+prime the shared session, attach it. ----
        let mut a = UnixStream::connect(&path).await.unwrap();
        assert!(matches!(
            preamble(&mut a).await,
            DaemonReply::Accepted { .. }
        ));
        // SES-1/SES-4: CreateSession now requires a real, existing workspace — create one first.
        let ws = create_workspace(&mut a, 100, "ws", "/tmp").await;
        send_frame(
            &mut a,
            &Frame::Request {
                id: 1,
                req: Request::CreateSession {
                    workspace_id: ws.id.clone(),
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
            match recv_frame_t(&mut a).await {
                Frame::Response {
                    id: 1,
                    res: Response::Session(m),
                } => break m.id,
                Frame::Response {
                    id: 1,
                    res: Response::Error { code, message },
                } => panic!("A create failed: {code}: {message}"),
                Frame::Push(_) => continue,
                other => panic!("A unexpected {other:?}"),
            }
        };
        // Prime the child to block on a go-signal, then print a marker on each release, then
        // block again — repeatable rounds of live output on demand.
        send_frame(
            &mut a,
            &Frame::Request {
                id: 2,
                req: Request::WriteStdin {
                    session_id: session_id.clone(),
                    bytes: b"read _g1; printf 'ROUND1\\n'; read _g2; printf 'ROUND2\\n'\n".to_vec(),
                },
            },
        )
        .await;
        loop {
            match recv_frame_t(&mut a).await {
                Frame::Response {
                    id: 2,
                    res: Response::Ack,
                } => break,
                Frame::Push(_) => continue,
                other => panic!("A expected Ack for prime write, got {other:?}"),
            }
        }

        send_frame(
            &mut a,
            &Frame::Request {
                id: 3,
                req: Request::AttachSession {
                    session_id: session_id.clone(),
                },
            },
        )
        .await;
        let (mut a_ack, mut a_replay) = (false, false);
        for _ in 0..4 {
            match recv_frame_t(&mut a).await {
                Frame::Response {
                    id: 3,
                    res: Response::Ack,
                } => a_ack = true,
                Frame::Push(Push::Replay {
                    session_id: sid, ..
                }) if sid == session_id => a_replay = true,
                Frame::Push(_) => continue,
                other => panic!("A unexpected before attach settle {other:?}"),
            }
            if a_ack && a_replay {
                break;
            }
        }
        assert!(a_ack && a_replay, "A must Ack + Replay its attach");

        // ---- Client B: connect, attach the SAME session. Must ALSO get its own Replay — proving
        // no supersede of A's attachment. ----
        let mut b = UnixStream::connect(&path).await.unwrap();
        assert!(matches!(
            preamble(&mut b).await,
            DaemonReply::Accepted { .. }
        ));
        send_frame(
            &mut b,
            &Frame::Request {
                id: 1,
                req: Request::AttachSession {
                    session_id: session_id.clone(),
                },
            },
        )
        .await;
        let (mut b_ack, mut b_replay) = (false, false);
        for _ in 0..4 {
            match recv_frame_t(&mut b).await {
                Frame::Response {
                    id: 1,
                    res: Response::Ack,
                } => b_ack = true,
                Frame::Push(Push::Replay {
                    session_id: sid, ..
                }) if sid == session_id => b_replay = true,
                Frame::Push(_) => continue,
                other => panic!("B unexpected before attach settle {other:?}"),
            }
            if b_ack && b_replay {
                break;
            }
        }
        assert!(
            b_ack && b_replay,
            "B must Ack + get its OWN Replay attaching the same session A already holds \
             (no supersede)"
        );

        // ---- Release round 1: BOTH A and B must see live Output (independent forwarders). ----
        send_frame(
            &mut a,
            &Frame::Request {
                id: 4,
                req: Request::WriteStdin {
                    session_id: session_id.clone(),
                    bytes: b"g1\n".to_vec(),
                },
            },
        )
        .await;
        loop {
            match recv_frame_t(&mut a).await {
                Frame::Response {
                    id: 4,
                    res: Response::Ack,
                } => break,
                Frame::Push(_) => continue,
                other => panic!("A expected Ack for round-1 release, got {other:?}"),
            }
        }

        let mut a_out = Vec::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < deadline && !a_out.windows(6).any(|w| w == b"ROUND1") {
            match tokio::time::timeout(std::time::Duration::from_millis(500), recv_frame(&mut a))
                .await
            {
                Ok(Frame::Push(Push::Output {
                    session_id: sid,
                    bytes,
                })) if sid == session_id => {
                    a_out.extend_from_slice(&bytes);
                }
                Ok(_) => continue,
                Err(_) => continue,
            }
        }
        assert!(
            a_out.windows(6).any(|w| w == b"ROUND1"),
            "A must receive live Output (round 1), got: {a_out:?}"
        );

        let mut b_out = Vec::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < deadline && !b_out.windows(6).any(|w| w == b"ROUND1") {
            match tokio::time::timeout(std::time::Duration::from_millis(500), recv_frame(&mut b))
                .await
            {
                Ok(Frame::Push(Push::Output {
                    session_id: sid,
                    bytes,
                })) if sid == session_id => {
                    b_out.extend_from_slice(&bytes);
                }
                Ok(_) => continue,
                Err(_) => continue,
            }
        }
        assert!(
            b_out.windows(6).any(|w| w == b"ROUND1"),
            "B must ALSO receive its own live Output (round 1), got: {b_out:?}"
        );

        // ---- A detaches: B must keep streaming. ----
        send_frame(
            &mut a,
            &Frame::Request {
                id: 5,
                req: Request::DetachSession {
                    session_id: session_id.clone(),
                },
            },
        )
        .await;
        loop {
            match recv_frame_t(&mut a).await {
                Frame::Response {
                    id: 5,
                    res: Response::Ack,
                } => break,
                Frame::Push(_) => continue,
                other => panic!("A expected Ack for DetachSession, got {other:?}"),
            }
        }

        send_frame(
            &mut a,
            &Frame::Request {
                id: 6,
                req: Request::WriteStdin {
                    session_id: session_id.clone(),
                    bytes: b"g2\n".to_vec(),
                },
            },
        )
        .await;
        loop {
            match recv_frame_t(&mut a).await {
                Frame::Response {
                    id: 6,
                    res: Response::Ack,
                } => break,
                Frame::Push(_) => continue,
                other => panic!("A expected Ack for round-2 release, got {other:?}"),
            }
        }

        // A must NOT see any more Output for this session after detaching.
        let a_after_detach = tokio::time::timeout(std::time::Duration::from_millis(400), async {
            loop {
                match recv_frame(&mut a).await {
                    Frame::Push(Push::Output {
                        session_id: sid, ..
                    }) if sid == session_id => return true,
                    _ => continue,
                }
            }
        })
        .await;
        assert!(
            a_after_detach.is_err(),
            "A must not receive further Output after DetachSession"
        );

        // B must still see ROUND2 — proving A's detach did not disturb B's independent forwarder.
        let mut b_out2 = Vec::new();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < deadline && !b_out2.windows(6).any(|w| w == b"ROUND2") {
            match tokio::time::timeout(std::time::Duration::from_millis(500), recv_frame(&mut b))
                .await
            {
                Ok(Frame::Push(Push::Output {
                    session_id: sid,
                    bytes,
                })) if sid == session_id => {
                    b_out2.extend_from_slice(&bytes);
                }
                Ok(_) => continue,
                Err(_) => continue,
            }
        }
        assert!(
            b_out2.windows(6).any(|w| w == b"ROUND2"),
            "B must still stream after A detached, got: {b_out2:?}"
        );

        // ---- A disconnects entirely: B must be unaffected. ----
        drop(a);

        // ---- KillSession over B: both A (already gone) and B get ChildExited; B's forwarder
        // must terminate cleanly (no leaked entries — verified via a fresh attach cycle below is
        // unnecessary here since the daemon-internal registry isn't reachable from this
        // socket-level test; the ChildExited delivery to B is the observable proof of teardown). ----
        send_frame(
            &mut b,
            &Frame::Request {
                id: 2,
                req: Request::KillSession {
                    session_id: session_id.clone(),
                },
            },
        )
        .await;

        let mut b_saw_exit = false;
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            match tokio::time::timeout(std::time::Duration::from_millis(500), recv_frame(&mut b))
                .await
            {
                Ok(Frame::Push(Push::ChildExited {
                    session_id: sid, ..
                })) if sid == session_id => {
                    b_saw_exit = true;
                    break;
                }
                Ok(Frame::Response { id: 2, .. }) => continue,
                Ok(_) => continue,
                Err(_) => continue,
            }
        }
        assert!(
            b_saw_exit,
            "B must receive ChildExited after KillSession (both forwarders must terminate)"
        );
    }

    // ---- `Request::RemoveWorkspace` (spec §3.3): the missing capability. A workspace whose roots
    // were deleted off disk used to be undeletable, so real DBs accumulated hundreds of dead
    // workspaces the sidebar still rendered.
    //
    // These two cases are the CHEAP ones (no PTY): the not-found contract and the failed-removal
    // atomicity contract, both driven through the real dispatch arm over the wire. The full
    // destructive path — a real live `/bin/sh` killed rather than orphaned, every dependent row
    // gone, `Push::WorkspaceRemoved` broadcast, an unrelated workspace untouched — lives in
    // `crates/sessiond/tests/remove_workspace.rs`, which compiles into its own test binary so its
    // PTY burst does not run concurrently with this binary's load-sensitive attach timing tests
    // (see that file's module doc).
    //
    // `spawn_server`/`test_deps_with_shutdown` use an in-memory `Db` and a tempdir runtime root, so
    // nothing here can touch the developer's real `~/Library/Application Support/…/bpa.db` — the
    // very database this feature exists to let a user clean up. ----

    /// Drain frames on `c` until the response correlated to `id` arrives, skipping any pushes that
    /// happen to interleave (a `RemoveWorkspace` that kills sessions also broadcasts `ChildExited`
    /// / `StateChanged` on the requester's own connection).
    async fn recv_response_for(c: &mut UnixStream, id: u64) -> Response {
        for _ in 0..64 {
            match recv_frame_t(c).await {
                Frame::Response { id: rid, res } if rid == id => return res,
                other => assert!(
                    !matches!(other, Frame::Request { .. }),
                    "the daemon must never send a Request frame, got {other:?}"
                ),
            }
        }
        panic!("no response for request id {id} after 64 frames");
    }

    /// Unknown workspace id ⇒ the SAME `Response::Error` a client already gets from
    /// `RemoveWorkspaceRoot` for an unknown id (code AND message), asserted by direct comparison —
    /// no new error code for a client to learn.
    #[tokio::test]
    async fn remove_workspace_unknown_id_is_the_same_not_found_error_as_remove_workspace_root() {
        let (path, _tx, _jh, _d, _r) = spawn_server().await;
        let mut c = UnixStream::connect(&path).await.unwrap();
        assert!(matches!(
            preamble(&mut c).await,
            DaemonReply::Accepted { .. }
        ));

        let unknown = "no-such-workspace".to_string();
        send_frame(
            &mut c,
            &Frame::Request {
                id: 1,
                req: Request::RemoveWorkspace {
                    workspace_id: unknown.clone(),
                },
            },
        )
        .await;
        let removed = recv_response_for(&mut c, 1).await;

        send_frame(
            &mut c,
            &Frame::Request {
                id: 2,
                req: Request::RemoveWorkspaceRoot {
                    workspace_id: unknown.clone(),
                    path: "/tmp".into(),
                },
            },
        )
        .await;
        let root = recv_response_for(&mut c, 2).await;

        match (&removed, &root) {
            (
                Response::Error { code, message },
                Response::Error {
                    code: rcode,
                    message: rmessage,
                },
            ) => {
                assert_eq!(
                    code, rcode,
                    "not-found code must mirror RemoveWorkspaceRoot"
                );
                assert_eq!(
                    message, rmessage,
                    "not-found message must mirror RemoveWorkspaceRoot"
                );
                assert!(
                    message.contains("not found") && message.contains(&unknown),
                    "the error must say which workspace was missing, got {message}"
                );
            }
            other => panic!("both verbs must report an error for an unknown id, got {other:?}"),
        }
    }

    /// A failed removal must not half-apply: the workspace and its sessions are still there and
    /// still usable. Driven through the real dispatch arm with the DB-level failure injected the
    /// same deterministic way `delete_workspace_is_atomic_…` does (a `BEFORE DELETE` trigger that
    /// `RAISE(ABORT)`s), so the arm's error path is exercised end to end over the wire.
    #[tokio::test]
    async fn remove_workspace_failure_leaves_no_partially_removed_state() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("d.sock");
        let listener = UnixListener::bind(&path).unwrap();
        let (tx, rx) = tokio::sync::watch::channel(false);
        let (deps, _runtime) = test_deps_with_shutdown(tx.clone());
        let jh = tokio::spawn({
            let deps = deps.clone();
            async move {
                let _ = serve(listener, deps, rx).await;
            }
        });

        let mut c = UnixStream::connect(&path).await.unwrap();
        assert!(matches!(
            preamble(&mut c).await,
            DaemonReply::Accepted { .. }
        ));
        let workspace_id = create_workspace(&mut c, 1, "w", "/tmp").await.id;

        let sid = "s-atomic".to_string();
        {
            let db = deps.db.lock().await;
            db.upsert_session(&SessionMeta {
                id: sid.clone(),
                workspace_id: workspace_id.clone(),
                title: "t".into(),
                shell: "/bin/sh".into(),
                cwd: "/tmp".into(),
                cols: 80,
                rows: 24,
                lifecycle: bpa_protocol::SessionLifecycle::Exited {
                    code: Some(0),
                    signal: None,
                },
                waiting_for_input: false,
                is_active: false,
                created_at: 1_700_000_000,
            })
            .unwrap();
            db.append_scrollback(&sid, 0, b"STILL HERE", 1).unwrap();
            db.append_command_event(&sid, 0, 1_700_000_000, "started", None, "gui")
                .unwrap();
            db.inject_delete_failure_for_test().unwrap();
        }

        send_frame(
            &mut c,
            &Frame::Request {
                id: 2,
                req: Request::RemoveWorkspace {
                    workspace_id: workspace_id.clone(),
                },
            },
        )
        .await;
        match recv_response_for(&mut c, 2).await {
            Response::Error { code, .. } => assert_eq!(code, "DbSql"),
            other => panic!("a failed removal must report an honest error, got {other:?}"),
        }

        {
            let db = deps.db.lock().await;
            assert!(
                db.list_workspaces()
                    .unwrap()
                    .iter()
                    .any(|w| w.id == workspace_id),
                "a failed removal must leave the workspace in place, not half-removed"
            );
            assert_eq!(
                db.workspace_session_ids(&workspace_id).unwrap(),
                vec![sid.clone()],
                "the session row must survive a failed removal"
            );
            assert_eq!(db.load_scrollback(&sid).unwrap(), b"STILL HERE");
            assert_eq!(db.list_command_events(&sid, 10).unwrap().len(), 1);
            db.clear_delete_failure_for_test().unwrap();
        }

        // With the injected failure gone, a retry removes everything — the failure left the DB
        // fully workable, not wedged.
        send_frame(
            &mut c,
            &Frame::Request {
                id: 3,
                req: Request::RemoveWorkspace {
                    workspace_id: workspace_id.clone(),
                },
            },
        )
        .await;
        match recv_response_for(&mut c, 3).await {
            Response::Ack => {}
            other => panic!("the retry must succeed, got {other:?}"),
        }
        {
            let db = deps.db.lock().await;
            assert!(db.list_workspaces().unwrap().is_empty());
            assert!(db.list_sessions().unwrap().is_empty());
            assert_eq!(db.load_scrollback(&sid).unwrap(), Vec::<u8>::new());
        }

        tx.send(true).unwrap();
        let _ = tokio::time::timeout(std::time::Duration::from_secs(3), jh).await;
    }
}
