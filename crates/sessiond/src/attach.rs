//! Multi-subscriber attach registry + reattach replay orchestration (spec §7 attach model, §6.2
//! reattach flow, §13 backpressure/honest-degradation, §5.2-5.4 multi-subscriber).
//!
//! N independent [`PushSink`]s may co-attach the SAME session (Pv2 §5.2: a session has 0..N
//! attachments, one per attached connection — GUI + future agents watching the same session at
//! once). Entries are keyed by `(SessionId, conn_id)`, so a fresh `attach()` from a NEW connection
//! never supersedes/stops another connection's existing attachment to the same session; each
//! attachment gets its own unique `sub_id` (allocated from a per-registry monotonic counter) and
//! its own independent forwarder. Each `attach` emits a fresh sanitized `Push::Replay` (built from
//! [`crate::pty_supervisor::Supervisor::snapshot_scrollback`]) to ONLY that attachment's sink, then
//! forwards live PTY bytes as `Push::Output` until that attachment's own detach or the sink
//! closes. `detach` stops `Output` only for that one `(session, conn)` pair — the PTY keeps
//! running and its scrollback ring keeps filling (spec §7 keep-alive), and every OTHER attachment
//! (this connection's or another's) for the same session is unaffected.
//!
//! ## The std-mpsc → async bridge
//!
//! [`crate::pty_supervisor::Supervisor::subscribe_output`] feeds bytes into a
//! `std::sync::mpsc::Sender<Vec<u8>>` from the supervisor's own blocking OS reader thread — it is
//! not async. Each `attach()` therefore:
//! 1. Creates a fresh `std::sync::mpsc::channel()` and registers its sender with the supervisor
//!    (`subscribe_output`) *before* snapshotting the scrollback, so no live byte produced between
//!    snapshot and subscribe is ever lost (the ring is authoritative for anything before the
//!    snapshot; the channel is authoritative for anything after).
//! 2. Sends `Push::Replay` first.
//! 3. Spawns a `tokio::task::spawn_blocking` forwarder that loops on the std receiver's
//!    `recv_timeout` (short, bounded poll — `JoinHandle::abort()` alone cannot preempt a thread
//!    parked in a blocking `recv`), strips injected OSC-133/OSC-7 marks from each chunk (see
//!    [`LiveOscStripper`]), wraps the result as `Push::Output`, and forwards it into the async
//!    `client_tx` via `blocking_send` (never `.await` inside `spawn_blocking`).
//! 4. The forwarder stops when: a shared `cancel` flag is observed (checked immediately after
//!    every `recv_timeout` wakeup, including one that already returned bytes — so `detach`/
//!    supersede take effect immediately rather than at the next idle poll), the std sender is
//!    dropped (supervisor's single sink slot was replaced by a fresh `subscribe_output` call, so
//!    `recv_timeout` returns `Disconnected`), or `client_tx` is closed (`blocking_send` fails).
//!
//! A `JoinHandle` per attachment lets `detach`/supersede abort the forwarder task once it observes
//! `cancel`, so a superseded/detached attachment never leaks a blocked OS thread; `cancel` is what
//! makes that teardown prompt rather than dependent on `abort()`'s cooperative-yield semantics.

use std::sync::Arc;
use std::sync::Mutex as StdMutex;

use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::protocol::{Push, SessionId};
use crate::pty_supervisor::Supervisor;

/// Sink for one attached client: bounded outbound channel of protocol `Push` frames. Owned by the
/// caller (Task 12's per-connection outbound queue); `AttachRegistry` only ever `.send()`s into
/// it and treats a closed/full-forever sink as "client gone" (spec §13: one slow/dead client must
/// never stall the daemon or an unrelated session).
pub type PushSink = mpsc::Sender<Push>;

/// Errors surfaced by [`AttachRegistry::attach`].
#[derive(Debug, PartialEq, Eq)]
pub enum AttachError {
    /// No live session with this id (spec §7: unknown/exited/killed).
    NoSuchSession,
    /// The client's sink was already closed before the first `Push::Replay` could be sent.
    SinkClosed,
}

impl std::fmt::Display for AttachError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AttachError::NoSuchSession => write!(f, "no such session"),
            AttachError::SinkClosed => write!(f, "client sink closed"),
        }
    }
}

impl std::error::Error for AttachError {}

/// Internal outcome of [`AttachRegistry::attach_live`] (D5): distinguishes the race-lost case
/// (`subscribe_output` refused because `is_active` flipped after `attach()`'s own read — the
/// session is still tracked, just no longer live) from a genuinely closed client sink, so
/// [`AttachRegistry::attach`] can fall back to replay-only for the former instead of surfacing a
/// spurious `NoSuchSession`. Not part of the public API — `attach()` maps this to [`AttachError`]
/// (or consumes it to retry) before returning.
enum LiveAttachError {
    /// `subscribe_output` refused because the session raced to inactive; caller should retry via
    /// `attach_replay_only`.
    LostRace,
    /// The client's sink was already closed before the first `Push::Replay` could be sent.
    SinkClosed,
}

/// One attachment's teardown state. An attachment is either [`AttachEntry::Live`] (subscribed to
/// the supervisor's live output fan-out, with its own forwarder task) or
/// [`AttachEntry::ReplayOnly`] (attached to an INACTIVE session — exited-unreaped or
/// boot-rehydrated, Pv2 §7/BL-7 — which got a `Push::Replay` but has no live reader to subscribe
/// to). Both variants are tracked in the same `entries` map so `attachment_count()`/`detach`/
/// `detach_all_for_conn` behave uniformly regardless of which kind an attachment is; only `Live`
/// holds a `sub_id`/supervisor sink or a forwarder to cancel — `ReplayOnly` holds nothing to leak
/// and detaching it is just a map removal.
enum AttachEntry {
    Live {
        /// This attachment's unique subscriber id in the supervisor's sink list (Pv2 §5.1/§5.2).
        /// Passed to `unsubscribe_output` on detach so this attachment's sink is pruned from the
        /// supervisor without disturbing any other subscriber's sink for the same session.
        sub_id: u64,
        /// Aborts the `spawn_blocking` forwarder task. Aborting a `spawn_blocking` task does not
        /// preempt it mid-`recv()`, so we also flip `cancel` — the forwarder polls it after every
        /// `recv()` wakeup, including the wakeup caused by the std sender being dropped when
        /// `unsubscribe_output` removes this attachment's sink in the supervisor.
        handle: JoinHandle<()>,
        cancel: Arc<std::sync::atomic::AtomicBool>,
    },
    /// Attached to an inactive session for scrollback replay only: the `Push::Replay` was already
    /// sent by `attach()`; nothing further ever arrives (no live reader, so no live `Output` can
    /// ever follow) and there is nothing to unsubscribe/cancel/abort on detach.
    ReplayOnly,
}

/// Multi-subscriber attach registry (Pv2 §5.2-5.4). Holds 0..N live [`AttachEntry`] values per
/// `SessionId` — one per `(SessionId, conn_id)` pair — so N different connections may co-attach
/// the same session simultaneously; none supersedes another.
pub struct AttachRegistry {
    supervisor: Arc<Supervisor>,
    entries: StdMutex<std::collections::HashMap<(SessionId, u64), AttachEntry>>,
    /// Monotonic counter allocating a fresh, registry-wide-unique `sub_id` for every attachment
    /// (Pv2 §5.1: the supervisor's sink list is keyed by caller-assigned `sub_id`; this registry
    /// is the one caller responsible for assigning them, so it owns uniqueness).
    next_sub_id: std::sync::atomic::AtomicU64,
    /// D5 test-only race hook: invoked in [`attach_live`](Self::attach_live) at the EXACT point
    /// between `attach()`'s `meta.is_active` read (still observed `true`) and the
    /// `subscribe_output` call — the same window `is_active` can flip false in production (the
    /// wait thread races ahead of `subscribe_output`'s own under-lock re-check,
    /// `pty_supervisor.rs`'s `subscribe_output`). Production always leaves this `None`; a test can
    /// install a closure here (e.g. one that kills the session out from under the in-flight
    /// `attach()` call) to deterministically reproduce the race instead of hoping wall-clock
    /// timing happens to hit a window a few CPU instructions wide.
    #[cfg(test)]
    before_subscribe_hook: StdMutex<Option<Box<dyn Fn() + Send + Sync>>>,
}

impl AttachRegistry {
    pub fn new(supervisor: Arc<Supervisor>) -> Self {
        AttachRegistry {
            supervisor,
            entries: StdMutex::new(std::collections::HashMap::new()),
            next_sub_id: std::sync::atomic::AtomicU64::new(1),
            #[cfg(test)]
            before_subscribe_hook: StdMutex::new(None),
        }
    }

    /// D5 test-only: install a closure run once, synchronously, at the exact race window
    /// described on [`before_subscribe_hook`](Self::before_subscribe_hook). Not part of the
    /// production API.
    #[cfg(test)]
    #[doc(hidden)]
    pub fn set_before_subscribe_hook_for_test(&self, hook: impl Fn() + Send + Sync + 'static) {
        *self.before_subscribe_hook.lock().unwrap() = Some(Box::new(hook));
    }

    /// Register `sink` as an ADDITIONAL independent consumer for `session_id` — Pv2 §5.2: N
    /// connections may co-attach the same session; this NEVER supersedes/stops another
    /// connection's (or this connection's own prior) attachment for the same session. `conn_id`
    /// records which connection owns this attachment so teardown can be scoped per-connection.
    ///
    /// Branches on liveness (Pv2 §7, BL-7 cold-rehydrate): "subscribe to live output" and "replay
    /// scrollback" are separable — only the replay actually needs the ring `snapshot_scrollback`
    /// reads, not a live reader.
    /// - **LIVE** (`meta.is_active`): unchanged path — allocate a `sub_id`, `subscribe_output` so
    ///   no live byte produced between subscribe and snapshot is ever lost, snapshot + send
    ///   `Push::Replay`, then spawn the forwarder streaming live `Push::Output` (OSC-133/OSC-7
    ///   stripped) until this attachment's own detach or `sink` closes.
    /// - **INACTIVE** (exited-but-unreaped, OR boot-rehydrated — `!meta.is_active` but still
    ///   tracked in the supervisor map): NO `subscribe_output` call at all — there is no live
    ///   reader thread to feed a fresh std channel, so subscribing would wire a forwarder that
    ///   polls forever for a byte that will never arrive (a leak). Just `snapshot_scrollback` +
    ///   send `Push::Replay`, then register a [`AttachEntry::ReplayOnly`] entry — no sink, no
    ///   forwarder, nothing to unsubscribe/cancel/abort on detach, but still counted so
    ///   `attachment_count()`/`detach`/`detach_all_for_conn` stay correct.
    /// - **UNKNOWN** (`supervisor.meta` errors — never created, or already fully reaped by
    ///   `kill`): `AttachError::NoSuchSession`, the genuine not-found path.
    ///
    /// **D5 race**: between this `meta.is_active` read (observed `true`) and `attach_live`'s
    /// `subscribe_output` call, the wait thread can flip `is_active` to `false` and clear the
    /// sinks list — `subscribe_output`'s own under-lock re-check (`pty_supervisor.rs`) then
    /// refuses. Pre-fix this surfaced as a spurious `NoSuchSession` for a session that IS still
    /// attachable, just replay-only now (the session is genuinely still in the supervisor map —
    /// only its liveness flipped mid-call). `attach_live` reports this distinctly (as opposed to
    /// the genuine "session was never tracked at all" case) so `attach()` can fall back to
    /// [`attach_replay_only`](Self::attach_replay_only) instead of erroring — nothing has been
    /// sent to `sink` yet at the point `subscribe_output` can fail (it is the very first
    /// operation in `attach_live`), so the fallback is a clean do-over, not a partial/corrupt
    /// attach.
    pub async fn attach(
        &self,
        conn_id: u64,
        session_id: &SessionId,
        sink: PushSink,
    ) -> Result<(), AttachError> {
        let meta = self
            .supervisor
            .meta(session_id)
            .map_err(|_| AttachError::NoSuchSession)?;

        let entry = if meta.is_active {
            match self.attach_live(session_id, sink.clone()).await {
                Ok(entry) => entry,
                Err(LiveAttachError::LostRace) => {
                    // The is_active flip raced ahead of subscribe_output; fall back to
                    // replay-only rather than surfacing a spurious NoSuchSession. If the session
                    // has ALSO vanished from the map entirely by now (e.g. immediately killed
                    // right after exiting), attach_replay_only's own `meta`-backed calls will
                    // correctly fail NoSuchSession — the genuine not-found path is preserved.
                    self.attach_replay_only(session_id, sink).await?
                }
                Err(LiveAttachError::SinkClosed) => return Err(AttachError::SinkClosed),
            }
        } else {
            self.attach_replay_only(session_id, sink).await?
        };

        // Insert at this connection's own key. If THIS SAME connection already held an
        // attachment for this session (a re-attach without an intervening detach — e.g. a client
        // that sends AttachSession twice), retire that stale entry exactly like `detach` would.
        // This is NOT a cross-connection supersede (another connection's attachment for the same
        // session is never touched, and is not reachable through this key) — it only prevents a
        // same-connection re-attach from silently leaking the previous forwarder's OS thread and
        // stale supervisor sink (a no-op for a stale `ReplayOnly` entry — nothing to release).
        let stale = self
            .entries
            .lock()
            .unwrap()
            .insert((session_id.clone(), conn_id), entry);
        if let Some(stale) = stale {
            self.retire(session_id, stale);
        }
        Ok(())
    }

    /// LIVE branch of [`attach`](Self::attach): subscribe-then-snapshot-then-replay-then-forward.
    /// Returns [`LiveAttachError::LostRace`] (D5) specifically when `subscribe_output` refuses
    /// because `is_active` flipped to `false` between `attach()`'s read and this call — the
    /// caller falls back to `attach_replay_only` for that case rather than erroring; every other
    /// failure here still means "no such session" (surfaced by the caller as `AttachError`).
    async fn attach_live(
        &self,
        session_id: &SessionId,
        sink: PushSink,
    ) -> Result<AttachEntry, LiveAttachError> {
        // Allocate this attachment's own unique sub_id (Pv2 §5.1/§5.2). No supersede: multiple
        // connections — and multiple sub_ids — may be live for the same session_id at once.
        let sub_id = self
            .next_sub_id
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);

        // D5 test-only race hook: fires here, right before `subscribe_output`, at the exact
        // window `is_active` can flip in production between `attach()`'s read and this call. A
        // no-op in production (the hook is always `None` there).
        #[cfg(test)]
        if let Some(hook) = self.before_subscribe_hook.lock().unwrap().as_ref() {
            hook();
        }

        // Bridge: std channel fed by the supervisor's blocking reader thread. `subscribe_output`
        // PUSHES this sink onto the session's sink list alongside any other live subscriber.
        let (std_tx, std_rx) = std::sync::mpsc::channel::<Vec<u8>>();
        self.supervisor
            .subscribe_output(session_id, sub_id, std_tx)
            .map_err(|_| LiveAttachError::LostRace)?;

        // Snapshot AFTER subscribing: any byte the reader thread produces from this point on is
        // captured by `std_rx`; anything before is covered by the snapshot. No gap, no double
        // delivery beyond what the ring itself already coalesces. A failure here is a narrower,
        // later race (the session vanished entirely AFTER a successful subscribe) — genuinely
        // "gone", not the is_active-flip race `LostRace` exists for, so it still surfaces via the
        // catch-all mapping below.
        let (cols, rows, content) = self
            .supervisor
            .snapshot_scrollback(session_id)
            .map_err(|_| LiveAttachError::LostRace)?;

        // Replay MUST be the first frame the client observes.
        let replay = Push::Replay {
            session_id: session_id.clone(),
            cols,
            rows,
            content,
        };
        sink.send(replay)
            .await
            .map_err(|_| LiveAttachError::SinkClosed)?;

        let cancel = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let cancel_bg = cancel.clone();
        let sid = session_id.clone();
        let sink_bg = sink;
        let handle = tokio::task::spawn_blocking(move || {
            let mut stripper = LiveOscStripper::new();
            loop {
                if cancel_bg.load(std::sync::atomic::Ordering::Acquire) {
                    return;
                }
                match std_rx.recv_timeout(std::time::Duration::from_millis(20)) {
                    Ok(bytes) => {
                        // Re-check cancellation with the bytes already in hand: `detach`/
                        // supersede must be effective immediately (no further Output reaches
                        // the client), not merely at the next poll wakeup. `JoinHandle::abort`
                        // alone cannot preempt a blocking `recv_timeout`, so `cancel` is the
                        // authoritative signal.
                        if cancel_bg.load(std::sync::atomic::Ordering::Acquire) {
                            return;
                        }
                        let cleaned = stripper.strip(&bytes);
                        if cleaned.is_empty() {
                            continue;
                        }
                        let frame = Push::Output {
                            session_id: sid.clone(),
                            bytes: cleaned,
                        };
                        if sink_bg.blocking_send(frame).is_err() {
                            return; // client gone
                        }
                    }
                    Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return, // detached/session gone
                }
            }
        });

        Ok(AttachEntry::Live {
            sub_id,
            handle,
            cancel,
        })
    }

    /// INACTIVE branch of [`attach`](Self::attach) (Pv2 §7, BL-7 cold-rehydrate): replay-only, no
    /// live subscription. `snapshot_scrollback` alone carries the session's final/persisted
    /// scrollback — it only reads the in-memory ring, which a rehydrated entry has pre-filled
    /// (`pty_supervisor::Supervisor::rehydrate_inactive`) even though it has no reader thread.
    async fn attach_replay_only(
        &self,
        session_id: &SessionId,
        sink: PushSink,
    ) -> Result<AttachEntry, AttachError> {
        let (cols, rows, content) = self
            .supervisor
            .snapshot_scrollback(session_id)
            .map_err(|_| AttachError::NoSuchSession)?;

        let replay = Push::Replay {
            session_id: session_id.clone(),
            cols,
            rows,
            content,
        };
        sink.send(replay)
            .await
            .map_err(|_| AttachError::SinkClosed)?;

        Ok(AttachEntry::ReplayOnly)
    }

    /// Release whatever `entry` holds: unsubscribe + cancel + abort for `Live`, nothing for
    /// `ReplayOnly` (it never held a supervisor sink or a forwarder to begin with).
    fn retire(&self, session_id: &SessionId, entry: AttachEntry) {
        if let AttachEntry::Live {
            sub_id,
            handle,
            cancel,
        } = entry
        {
            self.supervisor.unsubscribe_output(session_id, sub_id);
            cancel.store(true, std::sync::atomic::Ordering::Release);
            handle.abort();
        }
    }

    /// Stop `Output` forwarding for THIS connection's attachment to `session_id` only. Other
    /// connections' (or this connection's own, if it had more than one — it never does since a
    /// connection attaches a session at most once) attachments to the same session are completely
    /// unaffected — a detach from a connection that never attached this session is a no-op, so
    /// one client's `DetachSession` can never tear down another client's live stream. For a `Live`
    /// entry, unsubscribes its `sub_id` from the supervisor's sink list (Pv2 §5.2) so live bytes
    /// stop being pushed to it; the PTY keeps running and its ring keeps filling (spec §7
    /// keep-alive). For a `ReplayOnly` entry (Pv2 §7 inactive attach) this is just a map removal —
    /// there was never a sink/forwarder to release.
    pub fn detach(&self, conn_id: u64, session_id: &SessionId) {
        let entry = self
            .entries
            .lock()
            .unwrap()
            .remove(&(session_id.clone(), conn_id));
        if let Some(entry) = entry {
            self.retire(session_id, entry);
        }
    }

    /// Drop every attach entry (daemon shutdown drain). Used by `serve()` shutdown and boot's
    /// belt-and-braces drain — a whole-daemon teardown, not a per-client one.
    pub fn detach_all(&self) {
        let mut map = self.entries.lock().unwrap();
        for ((session_id, _conn_id), entry) in map.drain() {
            self.retire(&session_id, entry);
        }
    }

    /// Drop every attach entry OWNED BY `conn_id` — called when that client disconnects. Entries
    /// owned by other live connections (including other attachments to the SAME session) keep
    /// streaming (teardown is per-connection; a session may have many independent subscribers,
    /// Pv2 §5.2).
    pub fn detach_all_for_conn(&self, conn_id: u64) {
        let mut map = self.entries.lock().unwrap();
        let owned: Vec<(SessionId, u64)> =
            map.keys().filter(|(_, c)| *c == conn_id).cloned().collect();
        for key in owned {
            if let Some(entry) = map.remove(&key) {
                self.retire(&key.0, entry);
            }
        }
    }

    /// Remove EVERY attach entry for `session_id` when the session itself has ENDED (KillSession or
    /// natural child exit) — every connection currently attached to it, not just one. Graceful:
    /// does NOT cancel/abort any `Live` forwarder — the reader thread drops the session's sink(s) on
    /// its own exit, so each forwarder drains every remaining byte and terminates on `Disconnected`.
    /// Cancelling here instead would race the reader thread and truncate the session's final
    /// output to whichever attached clients are still watching (a real, user-visible loss).
    /// `unsubscribe_output` is called per removed `Live` entry for symmetry/defense-in-depth even
    /// though it is moot here — the supervisor's reader-exit path has already cleared its own sinks
    /// list by the time a session ends, so this is a harmless no-op there, not a required step. A
    /// `ReplayOnly` entry has nothing to unsubscribe and contributes no handle.
    /// Returns the `Live` forwarders' `JoinHandle`s (production callers drop them — each task is
    /// detached and self-terminating; tests await them to prove termination). Empty if no entry
    /// existed for this session, or if every entry for it was `ReplayOnly`.
    pub fn remove_session(&self, session_id: &SessionId) -> Vec<JoinHandle<()>> {
        let mut map = self.entries.lock().unwrap();
        let owned: Vec<(SessionId, u64)> = map
            .keys()
            .filter(|(sid, _)| sid == session_id)
            .cloned()
            .collect();
        let mut handles = Vec::with_capacity(owned.len());
        for key in owned {
            if let Some(AttachEntry::Live { sub_id, handle, .. }) = map.remove(&key) {
                self.supervisor.unsubscribe_output(session_id, sub_id);
                handles.push(handle);
            }
        }
        handles
    }

    /// Number of live attachments (observability + test hook). May exceed the number of live
    /// sessions once multiple connections co-attach the same session (Pv2 §5.2).
    pub fn attachment_count(&self) -> usize {
        self.entries.lock().unwrap().len()
    }
}

// ---- Live-output OSC-133/OSC-7 stripper (spec §10.3 + amendment §C) ----
//
// The raw bytes from `subscribe_output` are the supervisor's verbatim live stream — it still
// contains our injected OSC-133 (`ESC ] 133 ; ...`) and OSC-7 (`ESC ] 7 ; ...`) marks (the
// supervisor's live sink intentionally does NOT filter; only the scrollback ring sanitizes on
// its own copy). This is a narrower filter than `scrollback::Sanitizer`: it strips ONLY OSC-133
// and OSC-7 (not title OSCs, not alt-screen/bracketed-paste CSI toggles), because live output
// must let vim/less/alt-screen apps and window-title changes work exactly as the child emits
// them — only our own injected marks are internal plumbing that must never reach the client.

const ESC: u8 = 0x1B;
const BEL: u8 = 0x07;

/// Streaming, stateful per-attachment filter that drops OSC-133 and OSC-7 sequences from a live
/// PTY byte stream while passing every other byte through verbatim (SGR, cursor ops, alt-screen
/// toggles, title OSCs, plain text). Buffers a partial escape sequence across `strip()` calls.
struct LiveOscStripper {
    /// Bytes of an in-progress `ESC ]` sequence not yet classified as strip-or-keep.
    carry: Vec<u8>,
    /// Once a partial sequence is confirmed to be OSC-133/OSC-7 (i.e. classification is settled
    /// even though the terminator hasn't arrived yet), every subsequent byte — including the
    /// eventual terminator — must be dropped rather than re-entering ground state or failing
    /// open; mirrors `scrollback::Sanitizer`'s `discarding_until_terminator`.
    discarding_until_terminator: bool,
}

/// Bound the in-progress, not-yet-classified carry so a malformed/adversarial stream can't grow
/// it unboundedly; unrecognized sequences fail open (flushed verbatim) past this cap so genuine
/// long output is never silently lost.
const CARRY_CAP: usize = 256;
/// Once a carry is confirmed OSC-133/OSC-7, bound memory while still discarding-until-terminator
/// (never fail open — leaking the tail would forward a fragment of our own protocol marker).
const RECOGNIZED_CAP: usize = 8192;

enum Verdict {
    Drop,
    Keep,
    Incomplete,
}

impl LiveOscStripper {
    fn new() -> Self {
        LiveOscStripper {
            carry: Vec::new(),
            discarding_until_terminator: false,
        }
    }

    fn strip(&mut self, chunk: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(chunk.len());
        for &b in chunk {
            if self.discarding_until_terminator {
                if b == BEL {
                    self.discarding_until_terminator = false;
                } else if b == b'\\' && self.carry.last() == Some(&ESC) {
                    self.discarding_until_terminator = false;
                    self.carry.clear();
                } else if b == ESC {
                    self.carry.clear();
                    self.carry.push(ESC);
                } else {
                    self.carry.clear();
                }
                continue;
            }
            if self.carry.is_empty() {
                if b == ESC {
                    self.carry.push(b);
                } else {
                    out.push(b);
                }
                continue;
            }
            self.carry.push(b);
            match classify(&self.carry) {
                Verdict::Incomplete => {
                    if is_osc133_or_osc7_prefix(&self.carry) {
                        if self.carry.len() > RECOGNIZED_CAP {
                            self.carry.clear();
                            self.discarding_until_terminator = true;
                        }
                    } else if self.carry.len() > CARRY_CAP {
                        out.append(&mut self.carry);
                    }
                }
                Verdict::Drop => {
                    self.carry.clear();
                }
                Verdict::Keep => {
                    out.append(&mut self.carry);
                }
            }
        }
        out
    }
}

/// True once `seq` (`ESC ] ...`) is unambiguously the start of OSC-133 or OSC-7 — i.e. it has a
/// complete leading identifier followed by `;` equal to `"133"`/`"7"`, or is still a valid prefix
/// of one of those idents while accumulating.
fn is_osc133_or_osc7_prefix(seq: &[u8]) -> bool {
    if seq.len() < 2 || seq[0] != ESC || seq[1] != b']' {
        return false;
    }
    let body = &seq[2..];
    let ident_end = body.iter().position(|&c| c == b';');
    let ident = match ident_end {
        Some(end) => &body[..end],
        None => body,
    };
    match ident_end {
        Some(_) => matches!(ident, b"133" | b"7"),
        None => {
            !ident.is_empty()
                && [b"133".as_slice(), b"7"]
                    .iter()
                    .any(|full| full.starts_with(ident))
        }
    }
}

/// Classify a candidate escape sequence (always starts with ESC). Only `ESC ]` (OSC) sequences
/// are ever inspected for stripping; every other escape family is kept as soon as it can be
/// unambiguously recognized as "not an OSC" (byte 2 present and != `]`) — this lets non-OSC CSI
/// sequences pass through with minimal buffering.
fn classify(seq: &[u8]) -> Verdict {
    if seq.len() < 2 {
        return Verdict::Incomplete;
    }
    if seq[1] != b']' {
        // Not an OSC at all (e.g. CSI `[`, or a 2-byte escape) — never something we strip here.
        return Verdict::Keep;
    }
    classify_osc(seq)
}

/// OSC: `ESC ] <n> ; <text> <BEL | ESC \>`. Drop only idents `133` and `7`; keep everything else
/// (including title OSCs 0/1/2 — those are a live UX signal, not our internal plumbing) once the
/// sequence is fully terminated.
fn classify_osc(seq: &[u8]) -> Verdict {
    let body = &seq[2..];
    let terminated_bel = body.last() == Some(&BEL);
    let terminated_st =
        body.len() >= 2 && body[body.len() - 2] == ESC && body[body.len() - 1] == b'\\';
    if !terminated_bel && !terminated_st {
        return Verdict::Incomplete;
    }
    let ident_end = body.iter().position(|&c| c == b';').unwrap_or(body.len());
    let ident = &body[..ident_end];
    match ident {
        b"133" | b"7" => Verdict::Drop,
        _ => Verdict::Keep,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::pty_supervisor::SessionSpec;
    use std::time::Duration;

    fn base_env() -> Vec<(String, String)> {
        let path = std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into());
        vec![
            ("TERM".into(), "xterm-256color".into()),
            ("PATH".into(), path),
            (
                "HOME".into(),
                std::env::var("HOME").unwrap_or_else(|_| "/tmp".into()),
            ),
        ]
    }

    fn spec(args: Vec<String>) -> SessionSpec {
        SessionSpec {
            workspace_id: "ws-test".into(),
            shell: "/bin/sh".into(),
            args,
            cwd: std::path::PathBuf::from("/tmp"),
            env: base_env(),
            cols: 80,
            rows: 24,
            title: "t".into(),
        }
    }

    async fn recv_timeout(rx: &mut mpsc::Receiver<Push>, ms: u64) -> Option<Push> {
        tokio::time::timeout(Duration::from_millis(ms), rx.recv())
            .await
            .unwrap_or(None)
    }

    // ---- attach: Replay first (correct dims/content), then live Output frames follow. ----
    #[tokio::test]
    async fn attach_sends_replay_first_then_live_output() {
        let sup = Arc::new(Supervisor::new());
        // Block on a go-signal so nothing is emitted before we can attach — avoids any
        // subscribe/attach race with the reader thread.
        let id = sup
            .create(spec(vec![
                "-c".into(),
                "read _go; printf 'HELLO\\n'".into(),
            ]))
            .expect("create");

        let reg = AttachRegistry::new(sup.clone());
        let (sink, mut client) = mpsc::channel::<Push>(64);
        reg.attach(1, &id, sink).await.expect("attach");

        // First frame: Replay with the session's current dims and (empty, nothing written yet)
        // sanitized scrollback content.
        match recv_timeout(&mut client, 2000).await.expect("replay frame") {
            Push::Replay {
                session_id,
                cols,
                rows,
                content,
            } => {
                assert_eq!(session_id, id);
                assert_eq!((cols, rows), (80, 24));
                assert_eq!(content, Vec::<u8>::new());
            }
            other => panic!("expected Replay first, got {other:?}"),
        }

        // Release the child; live Output must follow with the printed text.
        sup.write_stdin(&id, b"go\n").expect("write go");
        let mut collected = Vec::new();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            match recv_timeout(&mut client, 500).await {
                Some(Push::Output { session_id, bytes }) => {
                    assert_eq!(session_id, id);
                    collected.extend_from_slice(&bytes);
                    if collected.windows(5).any(|w| w == b"HELLO") {
                        break;
                    }
                }
                Some(other) => panic!("expected Output, got {other:?}"),
                None => continue,
            }
        }
        assert!(
            collected.windows(5).any(|w| w == b"HELLO"),
            "expected live Output containing HELLO, got: {collected:?}"
        );

        let _ = sup.kill(&id);
    }

    // ---- second attach from a DIFFERENT connection does NOT supersede: both A and B keep
    // streaming their own independent live Output (Pv2 §5.2 multi-subscriber, no supersede). ----
    #[tokio::test]
    async fn second_attach_from_different_conn_does_not_supersede() {
        let sup = Arc::new(Supervisor::new());
        let id = sup
            .create(spec(vec![
                "-c".into(),
                "read _go; printf 'A\\n'; read _go2; printf 'B\\n'".into(),
            ]))
            .expect("create");

        let reg = AttachRegistry::new(sup.clone());

        // A on conn 1, B on conn 2: both remain attached — no cross-connection supersede.
        let (sink_a, mut client_a) = mpsc::channel::<Push>(64);
        reg.attach(1, &id, sink_a).await.expect("attach a");
        assert!(matches!(
            recv_timeout(&mut client_a, 2000).await.expect("replay a"),
            Push::Replay { .. }
        ));

        let (sink_b, mut client_b) = mpsc::channel::<Push>(64);
        reg.attach(2, &id, sink_b).await.expect("attach b");
        assert!(matches!(
            recv_timeout(&mut client_b, 2000).await.expect("replay b"),
            Push::Replay { .. }
        ));

        assert_eq!(
            reg.attachment_count(),
            2,
            "both A and B must remain attached — no supersede"
        );

        // Release the child; live output must reach BOTH A and B independently.
        sup.write_stdin(&id, b"go\n").expect("write go");

        let mut collected_a = Vec::new();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            match recv_timeout(&mut client_a, 500).await {
                Some(Push::Output { bytes, .. }) => {
                    collected_a.extend_from_slice(&bytes);
                    if collected_a.windows(1).any(|w| w == b"A") {
                        break;
                    }
                }
                Some(other) => panic!("expected Output on A, got {other:?}"),
                None => continue,
            }
        }
        assert!(
            collected_a.windows(1).any(|w| w == b"A"),
            "A must receive its own live Output, got: {collected_a:?}"
        );

        let mut collected_b = Vec::new();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            match recv_timeout(&mut client_b, 500).await {
                Some(Push::Output { bytes, .. }) => {
                    collected_b.extend_from_slice(&bytes);
                    if collected_b.windows(1).any(|w| w == b"A") {
                        break;
                    }
                }
                Some(other) => panic!("expected Output on B, got {other:?}"),
                None => continue,
            }
        }
        assert!(
            collected_b.windows(1).any(|w| w == b"A"),
            "B must ALSO receive its own independent live Output, got: {collected_b:?}"
        );

        let _ = sup.kill(&id);
    }

    // ---- detach stops Output while the session (PTY) stays alive. ----
    #[tokio::test]
    async fn detach_stops_output_session_stays_alive() {
        let sup = Arc::new(Supervisor::new());
        // The script blocks on a third `read` after printing AFTER so the child process is still
        // running (and `is_active` still true) for as long as this test needs it — without this,
        // the shell exits and gets reaped immediately after `printf 'AFTER'`, racing the
        // `sup.meta(&id).is_active` assertion below under CPU load/scheduling pressure (this raced
        // and failed intermittently: confirmed via repeated runs under synthetic CPU stress, see
        // Task 25 report). Keeping the child parked on stdin makes "the PTY is still alive when we
        // check" a fact of the test setup, not a race against the child's own exit timing.
        let id = sup
            .create(spec(vec![
                "-c".into(),
                "read _go; printf 'BEFORE\\n'; read _go2; printf 'AFTER\\n'; read _go3".into(),
            ]))
            .expect("create");

        let reg = AttachRegistry::new(sup.clone());
        let (sink, mut client) = mpsc::channel::<Push>(64);
        reg.attach(1, &id, sink).await.expect("attach");
        assert!(matches!(
            recv_timeout(&mut client, 2000).await.expect("replay"),
            Push::Replay { .. }
        ));

        sup.write_stdin(&id, b"go\n").expect("write go1");
        // Drain BEFORE so we know the pump was live and forwarding.
        let mut collected = Vec::new();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            match recv_timeout(&mut client, 500).await {
                Some(Push::Output { bytes, .. }) => {
                    collected.extend_from_slice(&bytes);
                    if collected.windows(6).any(|w| w == b"BEFORE") {
                        break;
                    }
                }
                Some(other) => panic!("expected Output, got {other:?}"),
                None => continue,
            }
        }
        assert!(collected.windows(6).any(|w| w == b"BEFORE"));

        reg.detach(1, &id);

        sup.write_stdin(&id, b"go2\n").expect("write go2");
        // No further Output should reach the detached client.
        let next = recv_timeout(&mut client, 500).await;
        assert!(
            next.is_none(),
            "detached sink must not receive further Output, got {next:?}"
        );

        // The session (PTY) itself is still alive/subscribable — keep-alive (spec §7).
        assert!(
            sup.meta(&id).map(|m| m.is_active).unwrap_or(false),
            "session must remain active after detach (PTY keeps running)"
        );

        let _ = sup.kill(&id);
    }

    // ---- unknown session -> AttachError::NoSuchSession. ----
    #[tokio::test]
    async fn attach_unknown_session_errors() {
        let sup = Arc::new(Supervisor::new());
        let reg = AttachRegistry::new(sup);
        let (sink, _client) = mpsc::channel::<Push>(4);
        let err = reg
            .attach(1, &"ghost-session".to_string(), sink)
            .await
            .unwrap_err();
        assert_eq!(err, AttachError::NoSuchSession);
    }

    // ---- live strip: OSC-133/OSC-7 removed from Output; SGR/alt-screen/title/text kept. ----
    #[tokio::test]
    async fn live_output_strips_osc133_osc7_but_keeps_everything_else() {
        let sup = Arc::new(Supervisor::new());
        // Emit (after a go-signal, so we can attach first): OSC-133 C mark, OSC-7 cwd mark,
        // alt-screen enter, SGR red text, title OSC, alt-screen leave, OSC-133 D end mark.
        let script = concat!(
            "read _go; ",
            "printf '\\033]133;C\\007'; ",
            "printf '\\033]7;file://h/tmp\\007'; ",
            "printf '\\033[?1049h'; ",
            "printf '\\033[31mRED\\033[0m'; ",
            "printf '\\033]0;My Title\\007'; ",
            "printf '\\033[?1049l'; ",
            "printf '\\033]133;D;0\\007'; ",
            "printf 'TAIL\\n'"
        );
        let id = sup
            .create(spec(vec!["-c".into(), script.into()]))
            .expect("create");

        let reg = AttachRegistry::new(sup.clone());
        let (sink, mut client) = mpsc::channel::<Push>(256);
        reg.attach(1, &id, sink).await.expect("attach");
        assert!(matches!(
            recv_timeout(&mut client, 2000).await.expect("replay"),
            Push::Replay { .. }
        ));

        sup.write_stdin(&id, b"go\n").expect("write go");

        let mut collected = Vec::new();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            match recv_timeout(&mut client, 500).await {
                Some(Push::Output { bytes, .. }) => {
                    collected.extend_from_slice(&bytes);
                    if collected.windows(4).any(|w| w == b"TAIL") {
                        break;
                    }
                }
                Some(other) => panic!("expected Output, got {other:?}"),
                None => continue,
            }
        }

        assert!(
            !contains(&collected, b"\x1b]133;"),
            "OSC-133 marks must be stripped from live Output, got: {collected:?}"
        );
        assert!(
            !contains(&collected, b"\x1b]7;"),
            "OSC-7 marks must be stripped from live Output, got: {collected:?}"
        );
        // Everything else must be kept verbatim for a live-attached terminal.
        assert!(
            contains(&collected, b"\x1b[?1049h"),
            "alt-screen enter must be kept"
        );
        assert!(
            contains(&collected, b"\x1b[?1049l"),
            "alt-screen leave must be kept"
        );
        assert!(
            contains(&collected, b"\x1b[31mRED\x1b[0m"),
            "SGR + text must be kept"
        );
        assert!(
            contains(&collected, b"\x1b]0;My Title\x07"),
            "title OSC must be kept live"
        );
        assert!(contains(&collected, b"TAIL"), "trailing text must be kept");

        let _ = sup.kill(&id);
    }

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|w| w == needle)
    }

    // ---- LiveOscStripper: an OSC-133 marker split across two strip() calls must be fully
    // dropped, including the terminator delivered in the second call — nothing leaks. ----
    //
    // This is a direct unit test of the struct (no PTY/tokio needed), exercising the same
    // split-sequence path `read()`-chunked live PTY output takes in production: a real terminal
    // emulator can hand `strip()` an OSC-133 sequence broken at an arbitrary byte boundary between
    // two separate reads. `carry` must accumulate across calls and `discarding_until_terminator`
    // (once the prefix is unambiguously recognized as OSC-133/OSC-7) must persist across the call
    // boundary so the terminator arriving in the NEXT call is still swallowed, not re-entered as
    // ground state and echoed to the client.
    #[test]
    fn live_osc_stripper_drops_osc133_marker_split_across_two_strip_calls() {
        let mut s = LiveOscStripper::new();

        // First call: text, then an OSC-133 "C" mark broken mid-sequence (no terminator yet).
        let out1 = s.strip(b"prompt$ \x1b]133;C");
        assert_eq!(
            out1, b"prompt$ ",
            "plain text before the split marker must pass through immediately"
        );

        // Second call: the rest of the identifier plus the BEL terminator, followed by more text.
        let out2 = s.strip(b"\x07cmd output\n");
        assert_eq!(
            out2, b"cmd output\n",
            "the OSC-133 marker's tail + terminator (delivered in the second strip() call) must \
             be fully dropped — nothing of \\x1b]133;C\\x07 may leak into the second call's output"
        );

        let mut combined = out1.clone();
        combined.extend_from_slice(&out2);
        assert!(
            !contains(&combined, b"\x1b]133;"),
            "OSC-133 prefix leaked across the strip() call boundary: {combined:?}"
        );
        assert!(
            !combined.contains(&BEL),
            "OSC-133 terminator (BEL) leaked across the strip() call boundary: {combined:?}"
        );
        assert_eq!(combined, b"prompt$ cmd output\n".to_vec());
    }

    // ---- Same split-across-calls scenario for OSC-7 (cwd mark), ST-terminated this time. ----
    #[test]
    fn live_osc_stripper_drops_osc7_marker_split_across_two_strip_calls_st_terminated() {
        let mut s = LiveOscStripper::new();

        // First call ends mid-payload, no terminator yet.
        let out1 = s.strip(b"a\x1b]7;file://host/some/very/long/pa");
        assert_eq!(out1, b"a");

        // Second call: rest of the payload + the two-byte ST terminator (ESC \\), split so the
        // ESC and '\\' themselves arrive together — then trailing text.
        let out2 = s.strip(b"th\x1b\\b");
        assert_eq!(
            out2, b"b",
            "OSC-7 payload tail + ST terminator (split across calls) must be fully dropped, only \
             the trailing 'b' survives"
        );

        let mut combined = out1.clone();
        combined.extend_from_slice(&out2);
        assert!(
            !contains(&combined, b"\x1b]7;"),
            "OSC-7 prefix leaked: {combined:?}"
        );
        assert!(
            !contains(&combined, b"file://host"),
            "OSC-7 payload leaked: {combined:?}"
        );
        assert_eq!(combined, b"ab".to_vec());
    }

    // ---- a same-connection re-attach (no cross-connection supersede anymore) does not leak a
    // spawn_blocking OS thread. ----
    //
    // Directly proves thread exit (not just registry bookkeeping): each retired entry's
    // `JoinHandle` is `.await`ed to completion, which only resolves once the underlying
    // `spawn_blocking` OS thread has actually returned from its closure.
    #[tokio::test]
    async fn same_conn_reattach_does_not_leak_thread() {
        let sup = Arc::new(Supervisor::new());
        let id = sup
            .create(spec(vec!["-c".into(), "sleep 5".into()]))
            .expect("create");

        let reg = AttachRegistry::new(sup.clone());

        // Re-attach the SAME connection to the SAME session several times; the registry must hold
        // exactly one entry at this (session, conn) key throughout (each prior forwarder for THIS
        // connection retired, not accumulated) — this is a same-connection re-attach, not a
        // cross-connection supersede.
        for _ in 0..5 {
            let (sink, _client) = mpsc::channel::<Push>(16);
            reg.attach(1, &id, sink).await.expect("attach");
            assert_eq!(
                reg.entries.lock().unwrap().len(),
                1,
                "registry must hold exactly one entry for this (session, conn) key at all times"
            );
        }

        // Take the still-live entry out ourselves and prove its JoinHandle completes promptly
        // once cancelled — i.e. the OS thread actually exits.
        let entry = reg
            .entries
            .lock()
            .unwrap()
            .remove(&(id.clone(), 1))
            .expect("entry present");
        let AttachEntry::Live { cancel, handle, .. } = entry else {
            panic!("expected a Live entry for a still-running session");
        };
        cancel.store(true, std::sync::atomic::Ordering::Release);
        let joined = tokio::time::timeout(Duration::from_secs(2), handle).await;
        assert!(
            joined.is_ok(),
            "forwarder task must exit promptly once cancelled — a hang here means a leaked thread"
        );

        // Exercise the real public `detach()` path on a fresh attach and prove it also removes
        // the entry (no dangling handle left in the registry to ever leak).
        let (sink, _client) = mpsc::channel::<Push>(16);
        reg.attach(1, &id, sink).await.expect("attach again");
        reg.detach(1, &id);
        assert!(
            reg.entries.lock().unwrap().is_empty(),
            "detach must remove the entry so no forwarder handle lingers in the registry"
        );

        let _ = sup.kill(&id);
    }

    // ---- Blocker A (attach.rs unit proof): detach_all_for_conn tears down ONLY the given
    // connection's entries; a second connection's attachment for a DIFFERENT session keeps
    // streaming live Output. This is the per-connection teardown that stops one client's disconnect
    // from corrupting another client's live stream (spec §7 single-attach is per-session, teardown
    // is per-connection). ----
    #[tokio::test]
    async fn detach_all_for_conn_only_removes_that_conns_entries() {
        let sup = Arc::new(Supervisor::new());
        // sa: conn 1 owns it, a plain long-lived sleep — its forwarder is what conn-1 teardown drops.
        let sa = sup
            .create(spec(vec!["-c".into(), "sleep 5".into()]))
            .expect("create sa");
        // sb: conn 2 owns it, blocked on a go-signal so we can prove it still streams AFTER conn-1
        // teardown by releasing it and observing SB_LIVE.
        let sb = sup
            .create(spec(vec![
                "-c".into(),
                "read _go; printf 'SB_LIVE\\n'; read _hold".into(),
            ]))
            .expect("create sb");

        let reg = AttachRegistry::new(sup.clone());

        let (sink_a, _client_a) = mpsc::channel::<Push>(64);
        reg.attach(1, &sa, sink_a)
            .await
            .expect("attach sa on conn 1");
        let (sink_b, mut client_b) = mpsc::channel::<Push>(64);
        reg.attach(2, &sb, sink_b)
            .await
            .expect("attach sb on conn 2");
        // Drain sb's Replay first.
        assert!(matches!(
            recv_timeout(&mut client_b, 2000).await.expect("replay sb"),
            Push::Replay { .. }
        ));
        assert_eq!(
            reg.attachment_count(),
            2,
            "both attachments live before teardown"
        );

        // Tear down ONLY conn 1: sa's entry is dropped, sb's remains.
        reg.detach_all_for_conn(1);
        assert_eq!(
            reg.attachment_count(),
            1,
            "only conn 1's entry removed; conn 2's must remain"
        );
        assert!(
            reg.entries.lock().unwrap().contains_key(&(sb.clone(), 2)),
            "sb (conn 2) must still be attached"
        );
        assert!(
            !reg.entries.lock().unwrap().contains_key(&(sa.clone(), 1)),
            "sa (conn 1) must be gone"
        );

        // sb's forwarder is STILL LIVE: release its go-signal and observe live Output reach conn 2.
        sup.write_stdin(&sb, b"go\n").expect("release sb");
        let mut collected = Vec::new();
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while std::time::Instant::now() < deadline {
            match recv_timeout(&mut client_b, 500).await {
                Some(Push::Output { bytes, .. }) => {
                    collected.extend_from_slice(&bytes);
                    if collected.windows(7).any(|w| w == b"SB_LIVE") {
                        break;
                    }
                }
                Some(_) => continue,
                None => continue,
            }
        }
        assert!(
            collected.windows(7).any(|w| w == b"SB_LIVE"),
            "sb's forwarder must still deliver live Output after conn 1's teardown, got: {collected:?}"
        );

        // A fresh attach for sa on conn 1 re-inserts (proves the slot was genuinely freed).
        let (sink_a2, _client_a2) = mpsc::channel::<Push>(64);
        reg.attach(1, &sa, sink_a2).await.expect("re-attach sa");
        assert_eq!(
            reg.attachment_count(),
            2,
            "re-attach of sa restores two entries"
        );

        let _ = sup.kill(&sa);
        let _ = sup.kill(&sb);
    }

    // ---- Blocker A (attach.rs unit proof): detach from a NON-owner connection is a no-op; only the
    // owning connection can detach. Prevents one client's DetachSession from tearing down another
    // client's attachment for the same session after a supersede. ----
    #[tokio::test]
    async fn detach_from_non_owner_is_a_noop() {
        let sup = Arc::new(Supervisor::new());
        let s = sup
            .create(spec(vec!["-c".into(), "sleep 5".into()]))
            .expect("create s");

        let reg = AttachRegistry::new(sup.clone());
        let (sink, _client) = mpsc::channel::<Push>(16);
        reg.attach(1, &s, sink).await.expect("attach on conn 1");
        assert_eq!(reg.attachment_count(), 1);

        // conn 2 does not own the entry: detach is a no-op.
        reg.detach(2, &s);
        assert_eq!(
            reg.attachment_count(),
            1,
            "detach from a non-owner connection must NOT remove the entry"
        );

        // The owner (conn 1) can detach it.
        reg.detach(1, &s);
        assert_eq!(
            reg.attachment_count(),
            0,
            "detach from the owning connection removes the entry"
        );

        let _ = sup.kill(&s);
    }

    // ---- Item 1 (BLOCKER regression): a session whose child exits NATURALLY must deliver its
    // trailing output to the attached client before the entry is reaped. Pre-fix, the wait thread
    // fired `on_exited` → `remove_session` → `abort_existing` (cancel + abort) the instant
    // `child.wait()` returned, racing the reader thread that was still draining the kernel PTY
    // buffer — so the final line (`FINAL_MARKER`) was sporadically dropped and never re-delivered.
    //
    // This is a RACE, so we loop several iterations: a single pass can pass by luck even against the
    // buggy code. The fix makes the reader thread drop the sink on its own exit, so the forwarder
    // drains every queued byte and terminates on `Disconnected` (graceful end-of-stream), while
    // `remove_session` only removes the map entry without cancelling. ----
    #[tokio::test]
    async fn natural_exit_final_output_reaches_attached_client_and_entry_is_reaped() {
        // 8 reps: this is a race — a single pass can succeed by luck even pre-fix. Pre-fix flake
        // rate measured at ~42% per 20-rep run (5/12 runs failed); see the fix report's RED section.
        for iter in 0..8 {
            let sup = Arc::new(Supervisor::new());
            let reg = Arc::new(AttachRegistry::new(sup.clone()));

            // Wire the production teardown by hand (mirrors socket_server::install_push_callbacks):
            // when the child exits, the wait thread calls `remove_session` — the exact path that
            // regressed. A `Weak` avoids a Supervisor⇄AttachRegistry cycle (same reasoning as prod).
            let reg_weak = Arc::downgrade(&reg);
            sup.on_exited(move |session_id, _code, _signal| {
                if let Some(reg) = reg_weak.upgrade() {
                    let _ = reg.remove_session(&session_id);
                }
            });

            // Child prints FINAL_MARKER and exits immediately (no trailing `read`): the reader
            // thread is still draining when `child.wait()` returns in the wait thread.
            let id = sup
                .create(spec(vec![
                    "-c".into(),
                    "read _go; printf 'FINAL_MARKER\\n'".into(),
                ]))
                .expect("create");

            let (sink, mut client) = mpsc::channel::<Push>(256);
            reg.attach(1, &id, sink).await.expect("attach");
            assert!(
                matches!(
                    recv_timeout(&mut client, 2000).await.expect("replay"),
                    Push::Replay { .. }
                ),
                "iter {iter}: expected Replay first"
            );

            // Release the child; it prints then exits. The trailing output MUST reach the client.
            sup.write_stdin(&id, b"go\n").expect("write go");

            // Drain until the marker arrives OR the channel genuinely CLOSES (the forwarder dropped
            // its sender ⇒ it forwarded every byte then terminated). Do NOT break on
            // `attachment_count() == 0`: under parallel-test load the wait thread's `remove_session`
            // reaps the map entry BEFORE the `spawn_blocking` forwarder finishes draining the final
            // `FINAL_MARKER` chunk, so an early break there races the in-flight tail and reports a
            // spurious loss. `recv()` returning `None` means the sender was dropped (graceful
            // end-of-stream after a full drain) — the only correct signal that "everything the
            // forwarder will ever send has been seen". A generous overall cap guards against a true
            // hang without being timing-sensitive to load.
            let mut collected = Vec::new();
            // Generous wall-clock cap: this is a CORRECTNESS assertion (the marker must arrive
            // before the stream closes), not a latency budget. A tight deadline turns a momentarily
            // oversubscribed host (full-workspace parallelism, a stalled scheduler) into a spurious
            // failure; 30s tolerates that without ever weakening the guarantee — the loop still
            // exits early the instant the marker lands or the channel closes.
            let deadline = std::time::Instant::now() + Duration::from_secs(30);
            loop {
                let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                if remaining.is_zero() {
                    break;
                }
                match tokio::time::timeout(remaining, client.recv()).await {
                    Ok(Some(Push::Output { bytes, .. })) => {
                        collected.extend_from_slice(&bytes);
                        if contains(&collected, b"FINAL_MARKER") {
                            break;
                        }
                    }
                    Ok(Some(_)) => continue,
                    // Channel closed: the forwarder drained fully and dropped its sender.
                    Ok(None) => break,
                    // Overall deadline hit (a genuine hang, not load jitter).
                    Err(_) => break,
                }
            }

            // (a) The trailing output reached the client (this is the assertion that failed
            // sporadically pre-fix).
            assert!(
                contains(&collected, b"FINAL_MARKER"),
                "iter {iter}: trailing output lost on natural exit, got: {collected:?}"
            );

            // (b) The attach entry is reaped within the deadline (forwarder self-terminated on the
            // reader's sink drop, then remove_session removed the map entry).
            // Generous cap for the same reason as (a): the reap is GUARANTEED (the wait thread's
            // on_exited → remove_session always runs), so this only tolerates a starved scheduler
            // under load — it never masks a missing reap (that would still fail after 30s).
            let reap_deadline = std::time::Instant::now() + Duration::from_secs(30);
            while reg.attachment_count() != 0 && std::time::Instant::now() < reap_deadline {
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            assert_eq!(
                reg.attachment_count(),
                0,
                "iter {iter}: attach entry must be reaped after natural exit"
            );

            // (c) A fresh remove_session returns empty — the entry is genuinely gone.
            assert!(
                reg.remove_session(&id).is_empty(),
                "iter {iter}: entry must already be gone (remove_session returns empty)"
            );

            let _ = sup.kill(&id);
        }
    }

    // ---- Item 1 (deterministic half): `remove_session` is GRACEFUL — it must NOT cancel/abort the
    // forwarder. After the session ends (here via `kill`, which joins the reader thread ⇒ the reader
    // drops the sink ⇒ the std channel closes), the forwarder drains and terminates ON ITS OWN.
    // `remove_session` returns the forwarder's JoinHandle so we can prove it completes — the no-leak
    // half of the guarantee (a graceful teardown that never hangs). ----
    #[tokio::test]
    async fn remove_session_lets_forwarder_drain_then_terminate() {
        let sup = Arc::new(Supervisor::new());
        let reg = AttachRegistry::new(sup.clone());

        let id = sup
            .create(spec(vec!["-c".into(), "read _hold".into()]))
            .expect("create");

        let (sink, _client) = mpsc::channel::<Push>(64);
        reg.attach(1, &id, sink).await.expect("attach");
        assert_eq!(reg.attachment_count(), 1);

        // `kill` joins the reader thread; the reader drops the sink on exit, closing the std channel
        // so the forwarder observes `Disconnected` and returns of its own accord.
        sup.kill(&id).expect("kill");

        let mut handles = reg.remove_session(&id);
        assert_eq!(
            handles.len(),
            1,
            "remove_session must return exactly one forwarder handle for a single attachment"
        );
        let handle = handles.remove(0);
        let joined = tokio::time::timeout(Duration::from_secs(2), handle).await;
        assert!(
            joined.is_ok(),
            "graceful remove_session must let the forwarder self-terminate — a hang means the \
             reader never dropped the sink (leak)"
        );

        // Entry is gone; a second remove is a no-op returning empty.
        assert!(
            reg.remove_session(&id).is_empty(),
            "entry must be removed exactly once"
        );
    }

    // ---- Task 7 D5 unit proof (Pv2 §5.2-5.4): two DIFFERENT connections attaching the SAME
    // session must BOTH remain attached — no supersede. `attachment_count()` must read 2, and a
    // second attach must not have torn down the first (this is the multi-subscriber model
    // replacing the old single-attach-per-session registry). ----
    #[tokio::test]
    async fn attach_two_conns_same_session_no_supersede() {
        let sup = Arc::new(Supervisor::new());
        let id = sup
            .create(spec(vec!["-c".into(), "sleep 5".into()]))
            .expect("create");

        let reg = AttachRegistry::new(sup.clone());

        let (sink_a, mut client_a) = mpsc::channel::<Push>(64);
        reg.attach(1, &id, sink_a).await.expect("attach a");
        assert!(matches!(
            recv_timeout(&mut client_a, 2000).await.expect("replay a"),
            Push::Replay { .. }
        ));
        assert_eq!(
            reg.attachment_count(),
            1,
            "one attachment after the first conn attaches"
        );

        let (sink_b, mut client_b) = mpsc::channel::<Push>(64);
        reg.attach(2, &id, sink_b).await.expect("attach b");
        assert!(matches!(
            recv_timeout(&mut client_b, 2000).await.expect("replay b"),
            Push::Replay { .. }
        ));

        // NO SUPERSEDE: both entries must be live — count is 2, not 1.
        assert_eq!(
            reg.attachment_count(),
            2,
            "two connections attaching the same session must BOTH remain attached (no supersede)"
        );

        let _ = sup.kill(&id);
    }

    // ---- Task 12r (Pv2 §7, BL-7 cold-rehydrate): after a natural exit the session lingers in the
    // supervisor map (only `kill`/rehydrate prunes it) with its reader thread already exited and
    // its sinks cleared. Attach must now SUCCEED via the replay-only branch: no `subscribe_output`
    // call is made at all (there's no live reader to feed a forwarder), `snapshot_scrollback` alone
    // supplies the session's final scrollback, the sink receives exactly one `Push::Replay`
    // carrying it, an entry IS registered (`attachment_count()==1`), and — because there is no live
    // reader — no `Push::Output` ever follows. Detach still removes the entry cleanly. ----
    #[tokio::test]
    async fn attach_on_inactive_session_replays_scrollback_without_live_subscription() {
        let sup = Arc::new(Supervisor::new());
        // Exits immediately on release; NOT killed, so it stays in the map with is_active=false.
        let id = sup
            .create(spec(vec!["-c".into(), "read _go; printf 'BYE\\n'".into()]))
            .expect("create");
        sup.write_stdin(&id, b"go\n").expect("write go");

        // Wait until the wait thread has recorded the exit (is_active=false) but do NOT kill (so the
        // session is still present in the supervisor map — the exited-but-unreaped window).
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            match sup.meta(&id) {
                Ok(m) if !m.is_active => break,
                Ok(_) => tokio::time::sleep(Duration::from_millis(20)).await,
                Err(e) => panic!("session vanished before we could observe the exited window: {e}"),
            }
            assert!(
                std::time::Instant::now() < deadline,
                "child never recorded its exit within the deadline"
            );
        }
        // Let the ring settle so the BYE marker is definitely folded in before we snapshot via attach.
        let settle_deadline = std::time::Instant::now() + Duration::from_secs(5);
        loop {
            let (_c, _r, bytes) = sup.snapshot_scrollback(&id).expect("snapshot pre-attach");
            if contains(&bytes, b"BYE") {
                break;
            }
            assert!(
                std::time::Instant::now() < settle_deadline,
                "BYE marker never reached the scrollback ring before the deadline"
            );
            tokio::time::sleep(Duration::from_millis(20)).await;
        }

        let reg = AttachRegistry::new(sup.clone());
        let (sink, mut client) = mpsc::channel::<Push>(16);
        reg.attach(1, &id, sink)
            .await
            .expect("attach on an inactive-but-tracked session must succeed (replay-only)");

        // Exactly one entry registered for detach/attachment_count bookkeeping.
        assert_eq!(
            reg.attachment_count(),
            1,
            "a replay-only attach must still register an entry"
        );

        // The sink must have received a Replay carrying the session's final scrollback.
        match recv_timeout(&mut client, 2000).await.expect("replay frame") {
            Push::Replay {
                session_id,
                content,
                ..
            } => {
                assert_eq!(session_id, id);
                assert!(
                    contains(&content, b"BYE"),
                    "Replay content must carry the session's final scrollback, got: {content:?}"
                );
            }
            other => panic!("expected Replay, got {other:?}"),
        }

        // No live Output can ever follow — there is no reader thread left to produce one.
        let next = recv_timeout(&mut client, 300).await;
        assert!(
            next.is_none(),
            "a replay-only attach must never receive a live Output push, got {next:?}"
        );

        // Detach removes the entry cleanly even though it never held a sub_id/forwarder.
        reg.detach(1, &id);
        assert_eq!(
            reg.attachment_count(),
            0,
            "detach must remove a replay-only entry just like a live one"
        );

        let _ = sup.kill(&id);
    }

    // ---- Task 12r: a `session_id` that was never created/persisted/rehydrated must still be
    // refused — the genuine not-found path is preserved even though inactive-but-tracked sessions
    // now succeed. ----
    #[tokio::test]
    async fn attach_on_unknown_session_errors_no_such_session() {
        let sup = Arc::new(Supervisor::new());
        let reg = AttachRegistry::new(sup);
        let (sink, _client) = mpsc::channel::<Push>(16);
        let err = reg
            .attach(1, &"never-existed-session".to_string(), sink)
            .await
            .unwrap_err();
        assert_eq!(
            err,
            AttachError::NoSuchSession,
            "attach on a truly unknown session id must still error"
        );
        assert_eq!(
            reg.attachment_count(),
            0,
            "no entry may be inserted for a refused attach"
        );
    }

    // ---- D5: attach races the child's own exit — `attach()` reads `meta.is_active == true`,
    // but by the time `attach_live` calls `subscribe_output`, the wait thread has already flipped
    // `is_active` to `false` and the reader thread has cleared the sinks list (the exact TOCTOU
    // window `pty_supervisor.rs`'s `subscribe_output` documents). Pre-fix this surfaced as a
    // spurious `AttachError::NoSuchSession` for a session that is genuinely still attachable —
    // just replay-only now. The `before_subscribe_hook_for_test` deterministically reproduces the
    // race by blocking `attach_live` right before `subscribe_output` until the child has
    // genuinely exited (the natural exited-but-unreaped pattern other tests in this file already
    // use, e.g. `attach_on_inactive_session_replays_scrollback_without_live_subscription`), so
    // this test proves the FIX, not a timing coincidence. ----
    #[tokio::test]
    async fn attach_falls_back_to_replay_only_when_is_active_flips_during_subscribe() {
        let sup = Arc::new(Supervisor::new());
        // Exits immediately once released, printing a marker first — NOT killed, so it lingers in
        // the exited-but-unreaped window `subscribe_output` refuses to subscribe into.
        let id = sup
            .create(spec(vec![
                "-c".into(),
                "read _go; printf 'RACE_MARKER\\n'".into(),
            ]))
            .expect("create");
        sup.write_stdin(&id, b"go\n").expect("write go");

        let reg = AttachRegistry::new(sup.clone());

        // Install the race hook BEFORE calling attach(): it blocks attach_live right before
        // subscribe_output until the child has genuinely exited (is_active observed false) —
        // this is what makes attach()'s EARLIER meta.is_active read (still true) stale by the
        // time subscribe_output actually runs, deterministically reproducing the race window.
        let sup_for_hook = sup.clone();
        let id_for_hook = id.clone();
        reg.set_before_subscribe_hook_for_test(move || {
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            loop {
                match sup_for_hook.meta(&id_for_hook) {
                    Ok(m) if !m.is_active => break,
                    Ok(_) => std::thread::sleep(Duration::from_millis(10)),
                    Err(e) => {
                        panic!("session vanished before the hook could observe its exit: {e}")
                    }
                }
                assert!(
                    std::time::Instant::now() < deadline,
                    "child never recorded its exit while the hook waited"
                );
            }
        });

        let (sink, mut client) = mpsc::channel::<Push>(64);
        let result = reg.attach(1, &id, sink).await;

        assert!(
            result.is_ok(),
            "attach must fall back to replay-only (not error) when is_active flips during \
             subscribe_output, got: {result:?}"
        );
        assert_eq!(
            reg.attachment_count(),
            1,
            "the fallback replay-only attach must still register an entry"
        );

        // The sink must have received a Replay (the replay-only path), carrying the marker the
        // child printed before it exited.
        match recv_timeout(&mut client, 2000).await.expect("replay frame") {
            Push::Replay {
                session_id,
                content,
                ..
            } => {
                assert_eq!(session_id, id);
                assert!(
                    contains(&content, b"RACE_MARKER"),
                    "Replay content must carry the session's scrollback, got: {content:?}"
                );
            }
            other => panic!("expected Replay, got {other:?}"),
        }

        // No live Output can ever follow — the fallback took the replay-only path, which never
        // subscribes a live forwarder.
        let next = recv_timeout(&mut client, 300).await;
        assert!(
            next.is_none(),
            "a race-fallback attach must never receive a live Output push, got {next:?}"
        );

        let _ = sup.kill(&id);
    }
}
