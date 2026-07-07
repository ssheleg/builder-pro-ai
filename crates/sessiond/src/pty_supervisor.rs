//! PTY supervisor (spec §9, §10.4). Owns every PTY; one blocking reader thread and one wait
//! thread (plus a lightweight status ticker) per session; process-group kill; env-hygiene;
//! the waiting-for-input heuristic.
//!
//! ## Public API (consumed verbatim by the broker, Tasks 11–13)
//!
//! - [`Supervisor::new`] — constructs the supervisor; calls `native_pty_system()` **once**
//!   and stores it. `Supervisor` is `Send + Sync`, so the broker holds it behind an
//!   `Arc<Supervisor>` and shares it across async tasks.
//! - [`Supervisor::create`] — opens one PTY from a fully-resolved [`SessionSpec`] (absolute
//!   shell path, shell-integration `args`, the §9.3 env **allowlist** already assembled by
//!   the caller, validated `cwd`). Returns the new [`SessionId`].
//! - [`Supervisor::write_stdin`] / [`Supervisor::resize`] / [`Supervisor::kill`] — drive one
//!   session. `kill` is a **process-group** kill (SIGTERM → 2 s grace → SIGKILL) then reap.
//! - [`Supervisor::subscribe_output`] / [`Supervisor::unsubscribe_output`] — register/remove one
//!   of N independent [`OutputSink`]s (each an `mpsc::Sender<Vec<u8>>`) per session, keyed by a
//!   caller-assigned `sub_id` (Pv2 §5.1: multi-subscriber fan-out for GUI + future co-viewing
//!   agents); each receives live PTY bytes verbatim and the attach layer forwards them as
//!   `Push::Output`.
//! - [`Supervisor::snapshot_scrollback`] — `(cols, rows, sanitized_bytes)` for `Push::Replay`.
//! - [`Supervisor::meta`] — build a [`bpa_protocol::SessionMeta`] from tracked state.
//! - Callbacks the broker registers so it can translate into protocol Pushes (spec §7):
//!   [`Supervisor::on_status`] → `Push::StateChanged`, [`Supervisor::on_created`] →
//!   `Push::SessionCreated`, [`Supervisor::on_exited`] → `Push::ChildExited`.
//!
//! ## Threading contract (spec §9 — every rule is load-bearing)
//!
//! 1. `native_pty_system()` is called once in [`Supervisor::new`] and stored.
//! 2. Per session: `openpty` → `spawn_command` → capture `pgid` → **`drop(slave)` immediately**
//!    (else the master `read()` never sees EOF when the child exits).
//! 3. `CommandBuilder::env_clear()` then set strictly the caller-provided allowlist — no
//!    daemon-internal / secret env reaches the child (§9.3 / §16).
//! 4. One blocking OS reader thread per PTY; `read() == Ok(0)` ⇒ EOF ⇒ teardown. Each chunk is
//!    fed to the OSC parser (advance lifecycle + cwd), the live grid, the sanitized scrollback
//!    ring, and every registered live [`OutputSink`] (fan-out to N subscribers, Pv2 §5.1).
//! 5. `writer = master.take_writer()` (take-once) owned behind a `Mutex`; `flush()` after writes.
//! 6. `killer = child.clone_killer()` captured **before** the wait thread; `wait()` runs only on
//!    the single owning wait thread (`Child` is `Send` not `Sync` — never shared).
//! 7. `resize` → `master.resize` (delivers SIGWINCH) + tracked cols/rows + `LiveGrid::resize`.
//! 8. `kill`/teardown signals the whole **process group** via `libc::killpg` (long-lived agent
//!    CLIs / dev servers would otherwise orphan), then always `killer.kill()` + reap.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::fd::RawFd;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize, PtySystem};
use tracing::{debug, warn};

use crate::live_grid::LiveGrid;
use crate::osc_parser::{advance_lifecycle, OscEvent, OscParser};
use crate::scrollback::ScrollbackRing;

use bpa_protocol::{SessionId, SessionLifecycle, SessionMeta, WorkspaceId};

/// Per-session sanitized scrollback ring capacity (spec §11: 256 KiB).
const SCROLLBACK_CAP: usize = 256 * 1024;
/// Reader thread read-buffer size (spec §9.4: 4–64 KiB; we use 32 KiB).
const READ_BUF: usize = 32 * 1024;
/// Output-quiescence window for the waiting-for-input heuristic (spec §10.4).
const QUIESCENT: Duration = Duration::from_millis(150);
/// Grace between SIGTERM and SIGKILL on a process-group kill (spec §9.8).
const KILL_GRACE: Duration = Duration::from_secs(2);
/// Status-ticker cadence: surfaces waiting-for-input flips without new bytes (the quiet-`cat`
/// case, spec §10.4).
const TICK: Duration = Duration::from_millis(200);

/// Errors surfaced by the supervisor. Typed (no `anyhow`) so the broker can map them to
/// `Response::Error { code, message }` (spec §13). Never panics on external failure.
#[derive(Debug, thiserror::Error)]
pub enum SupervisorError {
    /// No live session with this id (already killed, exited-and-reaped, or never created).
    #[error("no such session: {0}")]
    NoSuchSession(SessionId),
    /// A `portable-pty` / OS call failed while opening or driving a PTY.
    #[error("pty error: {0}")]
    Pty(String),
    /// An I/O error writing to the PTY master.
    #[error("io error: {0}")]
    Io(String),
    /// A worker thread could not be spawned.
    #[error("thread spawn failed: {0}")]
    Spawn(String),
}

/// Params to open one session. The caller (broker, Task 11/12 + `paths.rs`/`shell_integration`)
/// resolves everything to a zero-ambiguity spec before calling [`Supervisor::create`]:
/// - `shell` is an **absolute** program path (e.g. `/bin/zsh`).
/// - `args` are the shell-integration args (e.g. `["--init-file", "<path>"]`) or `[]`.
/// - `cwd` is already canonicalized/validated (absolute + existing dir, §16).
/// - `env` is the FULL §9.3 allowlist to set after `env_clear()` — it MUST include the
///   shell-integration var (`ZDOTDIR` or `BPA_INJECTION`) and `SSH_AUTH_SOCK` when present.
pub struct SessionSpec {
    pub workspace_id: WorkspaceId,
    pub shell: String,
    pub args: Vec<String>,
    pub cwd: std::path::PathBuf,
    pub env: Vec<(String, String)>,
    pub cols: u16,
    pub rows: u16,
    pub title: String,
}

/// A byte sink the supervisor feeds live PTY output to (the attach layer subscribes here and
/// forwards each chunk as `Push::Output`). Bytes are passed through verbatim.
pub type OutputSink = mpsc::Sender<Vec<u8>>;

/// Emitted to the broker when a session's status changes (→ `Push::StateChanged`, spec §7/§10.4).
#[derive(Debug, Clone, PartialEq)]
pub struct StatusUpdate {
    pub session_id: SessionId,
    pub lifecycle: SessionLifecycle,
    pub waiting_for_input: bool,
    pub cwd: String,
}

/// One best-effort command-history record accumulated in memory from an OSC-133 C/D mark (spec
/// §7, Pv2 `origin` amendment). The reader thread pushes these onto `Shared::pending_command_events`
/// as it parses; it never touches the DB directly (the reader thread has no DB handle — see the
/// module-level threading contract). `origin` is added by the caller at persist time (currently
/// always `"gui"`; the periodic flush sweep in `socket_server.rs` is the one place that writes
/// these through `Db::append_command_event`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandEvent {
    pub seq: u64,
    pub ts: i64,
    pub kind: &'static str,
    pub exit_code: Option<u8>,
}

/// Mutable per-session state shared between the reader / wait / ticker threads and the
/// supervisor's own methods. Every field is individually locked so no single lock is held
/// across a blocking read/write.
struct Shared {
    id: SessionId,
    workspace_id: WorkspaceId,
    title: String,
    shell: String,
    cwd: Mutex<String>,
    cols: Mutex<u16>,
    rows: Mutex<u16>,
    lifecycle: Mutex<SessionLifecycle>,
    is_active: Mutex<bool>,
    exit_code: Mutex<Option<u8>>,
    exit_signal: Mutex<Option<String>>,
    grid: Mutex<LiveGrid>,
    scrollback: Mutex<ScrollbackRing>,
    /// Live subscribers for this session's PTY output (Pv2 §5.1: N independent co-viewers, GUI
    /// and future agents). Keyed by caller-assigned `sub_id` so a specific subscriber can be
    /// removed without disturbing the others. The reader thread fans every chunk out to all
    /// entries and prunes any whose receiver has been dropped (`send` returns `Err`).
    sinks: Mutex<Vec<(u64, OutputSink)>>,
    last_output: Mutex<Instant>,
    master_fd: Option<RawFd>,
    waiting: Mutex<bool>,
    created_at: i64,
    /// Best-effort command-history events accumulated from OSC-133 C/D marks (spec §7), drained
    /// by the periodic flush sweep in `socket_server.rs` and written through
    /// `Db::append_command_event`. In-memory only — the reader thread that pushes onto this never
    /// opens a DB handle (see the module-level threading contract).
    pending_command_events: Mutex<Vec<CommandEvent>>,
    /// Monotonic per-session sequence number for `command_events` rows (starts at 0).
    command_seq: AtomicU64,
}

/// Owned per-session runtime state (the PTY master, writer, killer, and worker threads).
///
/// `pty` is `None` for a session built by [`Supervisor::rehydrate_inactive`] — a cold-restored,
/// PTY-less, replay-only entry (Pv2 §7 / BL-7): no live process, no OS threads, no master to
/// write/resize/kill. Every method that needs the PTY (`write_stdin`/`resize`/`kill`) fails
/// cleanly with a typed `SupervisorError` on such an entry rather than panicking or silently
/// no-op'ing — the session really is dead; pretending otherwise would be a lie to the caller.
struct Session {
    shared: Arc<Shared>,
    pty: Option<PtyRuntime>,
}

/// The PTY-backed half of a live [`Session`]: master/writer/killer + worker threads. Absent
/// entirely on a rehydrated (replay-only) entry — see [`Session::pty`].
struct PtyRuntime {
    // The master is retained so `resize` can deliver SIGWINCH for the session's lifetime.
    // `Box<dyn MasterPty + Send>` is `Send` but not `Sync`; wrapping it in a `Mutex` makes
    // `Session` (hence `Supervisor`) `Sync` so the broker can share `Arc<Supervisor>`. `resize`
    // is the only caller and it is infrequent, so serializing it costs nothing.
    master: Mutex<Box<dyn MasterPty + Send>>,
    writer: Mutex<Box<dyn Write + Send>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    pgid: Option<i32>,
    reader_thread: Mutex<Option<JoinHandle<()>>>,
    wait_thread: Mutex<Option<JoinHandle<()>>>,
    ticker_stop: Arc<Mutex<bool>>,
    ticker_thread: Mutex<Option<JoinHandle<()>>>,
}

type StatusCb = Arc<dyn Fn(StatusUpdate) + Send + Sync>;
type CreatedCb = Arc<dyn Fn(SessionMeta) + Send + Sync>;
type ExitedCb = Arc<dyn Fn(SessionId, Option<u8>, Option<String>) + Send + Sync>;

/// The PTY supervisor. Owns the shared `PtySystem` and the live-session map. `Send + Sync` so
/// the broker can hold it behind an `Arc` and drive it from many async tasks.
pub struct Supervisor {
    // `native_pty_system()` returns `Box<dyn PtySystem + Send>` (not `Sync`); wrapping it in a
    // Mutex both makes `Supervisor: Sync` and serializes the one call site (`openpty`), which is
    // cheap (session creation is rare).
    pty_system: Mutex<Box<dyn PtySystem + Send>>,
    sessions: Mutex<HashMap<SessionId, Arc<Session>>>,
    on_status: Mutex<Option<StatusCb>>,
    on_created: Mutex<Option<CreatedCb>>,
    on_exited: Mutex<Option<ExitedCb>>,
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl Supervisor {
    /// Construct the supervisor. Calls `native_pty_system()` exactly once (spec §9.1).
    pub fn new() -> Self {
        Supervisor {
            pty_system: Mutex::new(native_pty_system()),
            sessions: Mutex::new(HashMap::new()),
            on_status: Mutex::new(None),
            on_created: Mutex::new(None),
            on_exited: Mutex::new(None),
        }
    }

    /// Register the status callback (fired on lifecycle/cwd/waiting-for-input changes while a
    /// session is live). The broker translates each [`StatusUpdate`] into `Push::StateChanged`.
    pub fn on_status<F: Fn(StatusUpdate) + Send + Sync + 'static>(&self, cb: F) {
        *self.on_status.lock().unwrap() = Some(Arc::new(cb));
    }

    /// Register the created callback (fired once per successful [`create`](Self::create)).
    /// → `Push::SessionCreated`.
    pub fn on_created<F: Fn(SessionMeta) + Send + Sync + 'static>(&self, cb: F) {
        *self.on_created.lock().unwrap() = Some(Arc::new(cb));
    }

    /// Register the exited callback (fired once, from the wait thread, when the child is reaped).
    /// → `Push::ChildExited`. Carries `(session_id, code, signal)`.
    pub fn on_exited<F: Fn(SessionId, Option<u8>, Option<String>) + Send + Sync + 'static>(
        &self,
        cb: F,
    ) {
        *self.on_exited.lock().unwrap() = Some(Arc::new(cb));
    }

    /// Open one session: `openpty` → `spawn_command` → capture `pgid` → **drop slave** → wire the
    /// reader/wait/ticker threads (spec §9.2–§9.6). Returns the new [`SessionId`].
    pub fn create(&self, spec: SessionSpec) -> Result<SessionId, SupervisorError> {
        let pair = self
            .pty_system
            .lock()
            .unwrap()
            .openpty(PtySize {
                rows: spec.rows,
                cols: spec.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| SupervisorError::Pty(format!("openpty: {e}")))?;

        // §9.3 env hygiene: clear everything, then set ONLY the caller's allowlist.
        let mut cmd = CommandBuilder::new(&spec.shell);
        cmd.env_clear();
        for (k, v) in &spec.env {
            cmd.env(k, v);
        }
        for a in &spec.args {
            cmd.arg(a);
        }
        cmd.cwd(&spec.cwd);

        let mut child = pair
            .slave
            .spawn_command(cmd)
            .map_err(|e| SupervisorError::Pty(format!("spawn_command: {e}")))?;
        // Capture the pgid + master fd BEFORE dropping the slave.
        let pgid = pair.master.process_group_leader();
        let master_fd = pair.master.as_raw_fd();
        // §9.2: MUST drop the slave immediately or the master read() never sees EOF.
        drop(pair.slave);

        let reader = pair
            .master
            .try_clone_reader()
            .map_err(|e| SupervisorError::Pty(format!("try_clone_reader: {e}")))?;
        // §9.5: take the writer exactly once.
        let writer = pair
            .master
            .take_writer()
            .map_err(|e| SupervisorError::Pty(format!("take_writer: {e}")))?;
        // §9.6: capture the killer BEFORE starting the wait thread; the Child moves into it.
        let killer = child.clone_killer();

        let id: SessionId = uuid::Uuid::new_v4().to_string();
        let shared = Arc::new(Shared {
            id: id.clone(),
            workspace_id: spec.workspace_id.clone(),
            title: spec.title.clone(),
            shell: spec.shell.clone(),
            cwd: Mutex::new(spec.cwd.to_string_lossy().into_owned()),
            cols: Mutex::new(spec.cols),
            rows: Mutex::new(spec.rows),
            lifecycle: Mutex::new(SessionLifecycle::AtPrompt),
            is_active: Mutex::new(true),
            exit_code: Mutex::new(None),
            exit_signal: Mutex::new(None),
            grid: Mutex::new(LiveGrid::new(spec.cols, spec.rows)),
            scrollback: Mutex::new(ScrollbackRing::new(SCROLLBACK_CAP)),
            sinks: Mutex::new(Vec::new()),
            last_output: Mutex::new(Instant::now()),
            master_fd,
            waiting: Mutex::new(false),
            created_at: now_secs(),
            pending_command_events: Mutex::new(Vec::new()),
            command_seq: AtomicU64::new(0),
        });

        // ---- Reader thread (§9.4): the ONLY writer of parser/grid/scrollback from bytes. ----
        let reader_shared = shared.clone();
        let status_cb_reader = self.on_status.lock().unwrap().clone();
        let reader_thread = std::thread::Builder::new()
            .name(format!("bpa-reader-{id}"))
            .spawn(move || {
                let mut reader = reader;
                let mut parser = OscParser::new();
                let mut buf = vec![0u8; READ_BUF];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) => break, // EOF: child closed the slave side → teardown.
                        Err(e) => {
                            // A read error after the child exits is expected (EIO on some
                            // platforms); log at debug and tear down like EOF.
                            debug!(session = %reader_shared.id, "pty reader ended: {e}");
                            break;
                        }
                        Ok(n) => {
                            let chunk = &buf[..n];
                            *reader_shared.last_output.lock().unwrap() = Instant::now();

                            // (a) OSC parser: advance lifecycle + track cwd.
                            let events = parser.feed(chunk);
                            let mut status_dirty = false;
                            for ev in &events {
                                {
                                    let mut lc = reader_shared.lifecycle.lock().unwrap();
                                    advance_lifecycle(&mut lc, ev);
                                }
                                match ev {
                                    OscEvent::Cwd(path) => {
                                        *reader_shared.cwd.lock().unwrap() = path.clone();
                                    }
                                    OscEvent::CommandStart => {
                                        push_command_event(
                                            &reader_shared,
                                            "started",
                                            None,
                                        );
                                    }
                                    OscEvent::CommandEnd(code) => {
                                        push_command_event(
                                            &reader_shared,
                                            "finished",
                                            *code,
                                        );
                                    }
                                    _ => {}
                                }
                                status_dirty = true;
                            }

                            // (b) live grid, (c) sanitized scrollback ring.
                            reader_shared.grid.lock().unwrap().feed(chunk);
                            reader_shared.scrollback.lock().unwrap().push(chunk);

                            // (d) live broadcast — verbatim bytes to every attached subscriber.
                            // The parser is a side-channel extractor and does NOT filter the
                            // stream, so each client gets everything (alt-screen, SGR, title,
                            // text) exactly as the child emitted it. Send-and-prune: a `send`
                            // failure means that subscriber's receiver (and its forwarder) is
                            // gone, so we drop its entry here rather than waiting for an explicit
                            // `unsubscribe_output` — no leaked dead senders accumulate.
                            reader_shared
                                .sinks
                                .lock()
                                .unwrap()
                                .retain(|(_, tx)| tx.send(chunk.to_vec()).is_ok());

                            if status_dirty {
                                recompute_waiting(&reader_shared);
                                if let Some(cb) = &status_cb_reader {
                                    emit_status(cb, &reader_shared);
                                }
                            }
                        }
                    }
                }
                *reader_shared.is_active.lock().unwrap() = false;
                // Reader is the only producer into every subscribed sink. Clearing them all here
                // drops every std-channel Sender, so each attached forwarder drains its queued
                // bytes and then observes `Disconnected` — graceful end-of-stream instead of
                // truncation for every subscriber (once EOF is read, all output already read from
                // the kernel buffer ⇒ everything is in a channel or already forwarded). PTY master
                // EOF arrives once NO process holds the slave open: the daemon drops its slave copy
                // at spawn (spec §9), but that does NOT close slave fds inherited by the child's
                // descendants — a backgrounded descendant that keeps the slave open can extend EOF
                // past child exit, delaying this reader-exit tail until that descendant closes it
                // too (escaped-descendant handling is a known deferred item; behavior here is
                // unchanged). Whenever EOF (or `Err`/EIO) does arrive, both loop-break arms fall
                // through to this single reader-exit tail (Pv2 §5.1: extends the single-sink
                // truncation-fix design to N subscribers — every one gets the same graceful close).
                reader_shared.sinks.lock().unwrap().clear();
            })
            .map_err(|e| SupervisorError::Spawn(format!("reader thread: {e}")))?;

        // ---- Wait thread (§9.6): owns the Child; reaps and records exit status. ----
        let wait_shared = shared.clone();
        let exited_cb = self.on_exited.lock().unwrap().clone();
        let wait_thread = std::thread::Builder::new()
            .name(format!("bpa-wait-{id}"))
            .spawn(move || {
                let status = child.wait();
                let (code, signal) = match status {
                    Ok(s) => {
                        if let Some(sig) = s.signal() {
                            // Signal-terminated: code = None, carry the signal name (spec §5).
                            (None, Some(sig.to_string()))
                        } else {
                            (Some((s.exit_code() & 0xff) as u8), None)
                        }
                    }
                    // wait() failing is unexpected; degrade to "exited, code unknown".
                    Err(e) => {
                        warn!(session = %wait_shared.id, "child wait() failed: {e}");
                        (None, None)
                    }
                };
                *wait_shared.is_active.lock().unwrap() = false;
                *wait_shared.exit_code.lock().unwrap() = code;
                *wait_shared.exit_signal.lock().unwrap() = signal.clone();
                *wait_shared.lifecycle.lock().unwrap() = SessionLifecycle::Exited {
                    code,
                    signal: signal.clone(),
                };
                *wait_shared.waiting.lock().unwrap() = false;
                if let Some(cb) = &exited_cb {
                    cb(wait_shared.id.clone(), code, signal);
                }
            })
            .map_err(|e| SupervisorError::Spawn(format!("wait thread: {e}")))?;

        // ---- Ticker thread (§10.4): re-emit waiting-for-input when it flips with no new bytes. ----
        let ticker_shared = shared.clone();
        let ticker_stop = Arc::new(Mutex::new(false));
        let ticker_stop_thread = ticker_stop.clone();
        let status_cb_ticker = self.on_status.lock().unwrap().clone();
        let ticker_thread = std::thread::Builder::new()
            .name(format!("bpa-tick-{id}"))
            .spawn(move || loop {
                std::thread::sleep(TICK);
                if *ticker_stop_thread.lock().unwrap() {
                    break;
                }
                if !*ticker_shared.is_active.lock().unwrap() {
                    break;
                }
                let before = *ticker_shared.waiting.lock().unwrap();
                recompute_waiting(&ticker_shared);
                let after = *ticker_shared.waiting.lock().unwrap();
                if before != after {
                    if let Some(cb) = &status_cb_ticker {
                        emit_status(cb, &ticker_shared);
                    }
                }
            })
            .map_err(|e| SupervisorError::Spawn(format!("ticker thread: {e}")))?;

        let session = Arc::new(Session {
            shared: shared.clone(),
            pty: Some(PtyRuntime {
                master: Mutex::new(pair.master),
                writer: Mutex::new(writer),
                killer: Mutex::new(killer),
                pgid,
                reader_thread: Mutex::new(Some(reader_thread)),
                wait_thread: Mutex::new(Some(wait_thread)),
                ticker_stop,
                ticker_thread: Mutex::new(Some(ticker_thread)),
            }),
        });

        self.sessions.lock().unwrap().insert(id.clone(), session);

        if let Some(cb) = self.on_created.lock().unwrap().clone() {
            // meta() cannot fail here (we just inserted the session), but degrade gracefully.
            if let Ok(meta) = self.meta(&id) {
                cb(meta);
            }
        }
        Ok(id)
    }

    fn get(&self, id: &str) -> Result<Arc<Session>, SupervisorError> {
        self.sessions
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(|| SupervisorError::NoSuchSession(id.to_string()))
    }

    /// Write raw bytes to the session's PTY master (keystrokes / paste). `flush`es after
    /// (spec §9.5). A rehydrated (replay-only) entry has no PTY: fails honestly with
    /// `SupervisorError::NoSuchSession` rather than panicking or silently discarding the write —
    /// the session really is dead (Pv2 §7 cold-rehydrate; no reader/writer/threads survive a
    /// restart, only its persisted scrollback does).
    pub fn write_stdin(&self, id: &str, bytes: &[u8]) -> Result<(), SupervisorError> {
        let s = self.get(id)?;
        let pty = self.require_pty(&s, id)?;
        let mut w = pty.writer.lock().unwrap();
        w.write_all(bytes)
            .map_err(|e| SupervisorError::Io(format!("write_all: {e}")))?;
        w.flush()
            .map_err(|e| SupervisorError::Io(format!("flush: {e}")))?;
        Ok(())
    }

    /// Resize the PTY (delivers SIGWINCH to the child) and update tracked cols/rows + the live
    /// grid (spec §9.7). A rehydrated (replay-only) entry has no PTY to resize: fails honestly
    /// with `SupervisorError::NoSuchSession` (see [`write_stdin`](Self::write_stdin)).
    pub fn resize(&self, id: &str, cols: u16, rows: u16) -> Result<(), SupervisorError> {
        let s = self.get(id)?;
        let pty = self.require_pty(&s, id)?;
        pty.master
            .lock()
            .unwrap()
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| SupervisorError::Pty(format!("resize: {e}")))?;
        *s.shared.cols.lock().unwrap() = cols;
        *s.shared.rows.lock().unwrap() = rows;
        s.shared.grid.lock().unwrap().resize(cols, rows);
        Ok(())
    }

    /// Return `s`'s `PtyRuntime`, or a honest `NoSuchSession` error if `s` is a rehydrated
    /// (replay-only, PTY-less) entry — the session is tracked (so `get()` succeeded) but there is
    /// nothing live to drive. Shared by every PTY-driving method so each fails the same way.
    fn require_pty<'a>(&self, s: &'a Session, id: &str) -> Result<&'a PtyRuntime, SupervisorError> {
        s.pty
            .as_ref()
            .ok_or_else(|| SupervisorError::NoSuchSession(id.to_string()))
    }

    /// Register an additional live output subscriber for this session, keyed by caller-assigned
    /// `sub_id` (Pv2 §5.1: N independent co-viewers — GUI + future agents attached to the same
    /// session simultaneously). PUSHES onto the subscriber list; does NOT replace any existing
    /// entry. Callers own `sub_id` uniqueness (e.g. a connection/attach id) — subscribing twice
    /// with the same id simply pushes a second entry for that id (harmless: `unsubscribe_output`
    /// removes every entry matching that id, and a stale duplicate is pruned on its own first
    /// failed `send` like any other dead sink).
    ///
    /// Refuses an exited-but-unreaped session (`!is_active`): after natural child exit the session
    /// lingers in the map until a `kill`/rehydrate prunes it, but its reader thread has already
    /// exited and cleared the sinks list (dropping every producer). Subscribing a fresh sink here
    /// would wire a std channel that never sees a producer drop, so the attach forwarder would poll
    /// forever (leak). Returning `NoSuchSession` maps to `AttachError::NoSuchSession` at the attach
    /// layer — the client sees the same "session is gone" it would for a killed one.
    pub fn subscribe_output(
        &self,
        id: &str,
        sub_id: u64,
        sink: OutputSink,
    ) -> Result<(), SupervisorError> {
        let s = self.get(id)?;
        // Close the subscribe/reader-exit TOCTOU by checking `is_active` WHILE HOLDING the sinks
        // lock, then pushing into the already-held Vec. The reader exit tail runs in program order
        // `is_active = false` (§ reader tail, store) *happens-before* `sinks.clear()` (clear), each
        // in its own critical section. Serializing against `sinks.clear()` here therefore gives one
        // of two correct outcomes:
        //   - the reader's `sinks.clear()` has NOT run yet: it is now blocked on the sinks lock we
        //     hold. If our `is_active` read observes `false` we refuse; otherwise we push our
        //     `(sub_id, sink)`, release, and the reader's subsequent `clear()` removes and DROPS
        //     our Sender along with every other — the forwarder then sees `Disconnected` (no leak).
        //   - the reader's `sinks.clear()` has already run: because the store happens-before the
        //     clear, `is_active` is already `false`, so our read observes it and we refuse.
        // Either way a fresh sink is never left installed with no producer to drop it. Nothing
        // anywhere holds `is_active` while acquiring `sinks`, so nesting this short `is_active`
        // acquire under the sinks lock cannot deadlock.
        let mut sinks_guard = s.shared.sinks.lock().unwrap();
        if !*s.shared.is_active.lock().unwrap() {
            return Err(SupervisorError::NoSuchSession(id.to_string()));
        }
        sinks_guard.push((sub_id, sink));
        Ok(())
    }

    /// Remove the subscriber(s) registered under `sub_id` for this session (Pv2 §5.1), without
    /// disturbing any other subscriber. A no-op (not an error) if `id` is unknown or `sub_id` was
    /// never subscribed — unsubscribing is idempotent housekeeping, not a state assertion, so
    /// callers (e.g. a detaching connection) never need to special-case "already gone".
    pub fn unsubscribe_output(&self, id: &str, sub_id: u64) {
        if let Ok(s) = self.get(id) {
            s.shared.sinks.lock().unwrap().retain(|(sid, _)| *sid != sub_id);
        }
    }

    /// `(cols, rows, sanitized_scrollback_bytes)` — the payload for `Push::Replay` (spec §6.2/§11).
    pub fn snapshot_scrollback(&self, id: &str) -> Result<(u16, u16, Vec<u8>), SupervisorError> {
        let s = self.get(id)?;
        let cols = *s.shared.cols.lock().unwrap();
        let rows = *s.shared.rows.lock().unwrap();
        let bytes = s.shared.scrollback.lock().unwrap().snapshot();
        Ok((cols, rows, bytes))
    }

    /// Take (drain) every pending best-effort [`CommandEvent`] accumulated for `id` since the last
    /// drain (spec §7). Returns an empty `Vec` for an unknown/gone session — best-effort, never an
    /// error — mirroring [`snapshot_scrollback`](Self::snapshot_scrollback)'s "session may already
    /// be gone" tolerance. The periodic flush sweep in `socket_server.rs` is the sole caller and is
    /// the only place these events reach the DB.
    pub fn drain_command_events(&self, id: &str) -> Vec<CommandEvent> {
        match self.get(id) {
            Ok(s) => std::mem::take(&mut *s.shared.pending_command_events.lock().unwrap()),
            Err(_) => Vec::new(),
        }
    }

    /// Build a [`SessionMeta`] snapshot from tracked state (spec §5). The internal lifecycle is
    /// already a `protocol::SessionLifecycle` (the OSC parser reuses the wire type); `Typing` is
    /// never produced.
    pub fn meta(&self, id: &str) -> Result<SessionMeta, SupervisorError> {
        let s = self.get(id)?;
        let sh = s.shared.clone();
        // Collect each field into a local so every MutexGuard is dropped before `s`/`sh`.
        let cwd = sh.cwd.lock().unwrap().clone();
        let cols = *sh.cols.lock().unwrap();
        let rows = *sh.rows.lock().unwrap();
        let lifecycle = sh.lifecycle.lock().unwrap().clone();
        let waiting_for_input = *sh.waiting.lock().unwrap();
        let is_active = *sh.is_active.lock().unwrap();
        Ok(SessionMeta {
            id: sh.id.clone(),
            workspace_id: sh.workspace_id.clone(),
            title: sh.title.clone(),
            shell: sh.shell.clone(),
            cwd,
            cols,
            rows,
            lifecycle,
            waiting_for_input,
            is_active,
            created_at: sh.created_at,
        })
    }

    /// Process-group kill (spec §9.8): `killpg(SIGTERM)` → ≤2 s grace → `killpg(SIGKILL)`, then
    /// always `killer.kill()` + join the worker threads to reap the child (no zombie / no
    /// orphaned grandchildren). Falls back to `killer.kill()` when there is no process-group
    /// leader (non-POSIX / ConPTY).
    ///
    /// A rehydrated (replay-only) entry has no PTY/process/threads to kill: fails honestly with
    /// `SupervisorError::NoSuchSession` (see [`write_stdin`](Self::write_stdin)) and leaves the
    /// entry in the map untouched — there is nothing to reap, and removing the entry here would
    /// destroy the only surviving copy of its scrollback before a future `attach` can replay it.
    pub fn kill(&self, id: &str) -> Result<(), SupervisorError> {
        let s = self.get(id)?;
        let pty = self.require_pty(&s, id)?;
        if let Some(pgid) = pty.pgid {
            // SIGTERM the whole group (the shell + any un-`setsid`'d descendants).
            unsafe {
                libc::killpg(pgid, libc::SIGTERM);
            }
            let start = Instant::now();
            while start.elapsed() < KILL_GRACE {
                if !*s.shared.is_active.lock().unwrap() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            if *s.shared.is_active.lock().unwrap() {
                // Grace elapsed and the child is still alive: escalate to SIGKILL on the group.
                unsafe {
                    libc::killpg(pgid, libc::SIGKILL);
                }
            }
        }
        // Always kill the immediate child + reap it (join the wait thread) to avoid a zombie.
        let _ = pty.killer.lock().unwrap().kill();
        *pty.ticker_stop.lock().unwrap() = true;
        if let Some(h) = pty.wait_thread.lock().unwrap().take() {
            let _ = h.join();
        }
        if let Some(h) = pty.reader_thread.lock().unwrap().take() {
            let _ = h.join();
        }
        if let Some(h) = pty.ticker_thread.lock().unwrap().take() {
            let _ = h.join();
        }
        // Drop the session from the live map: its PTY is gone and it must not be re-driven.
        self.sessions.lock().unwrap().remove(id);
        Ok(())
    }

    /// Kill every live session (SIGTERM → grace → SIGKILL per group, then reap). Used on daemon
    /// shutdown (spec §9.8 / §13). Idempotent; safe to call with no live sessions. A rehydrated
    /// (replay-only) entry has nothing to kill — `kill()` returns `NoSuchSession` for it, which
    /// this loop swallows like any other per-id failure, leaving the entry in the map (harmless:
    /// the whole process is exiting either way).
    pub fn shutdown_all(&self) {
        let ids: Vec<SessionId> = self.sessions.lock().unwrap().keys().cloned().collect();
        for id in ids {
            let _ = self.kill(&id);
        }
    }

    /// Cold-rehydrate a session loaded purely from persistence (Pv2 §7 / BL-7: "your records and
    /// scrollback reappear" after a daemon restart) as an INACTIVE, PTY-less, replay-only entry —
    /// `is_active=false`, `cols`/`rows` from `meta`, a [`ScrollbackRing`] pre-filled with
    /// `scrollback` (already-sanitized persisted bytes), and **no** reader/wait/ticker threads and
    /// **no** PTY master (`pty: None`). This is what lets `AttachSession` on it later succeed via
    /// `snapshot_scrollback` alone (attach.rs), without needing a live reader.
    ///
    /// `meta()` on the rehydrated id then returns `meta` (with `is_active` forced false regardless
    /// of what the persisted row said — a cold-rehydrated session is never "active", even if it
    /// was killed mid-write and its last persisted row happened to say otherwise);
    /// `snapshot_scrollback()` returns the (possibly cap-truncated) pre-filled bytes; `write_stdin`/
    /// `resize`/`kill` all return `SupervisorError::NoSuchSession` — an honest failure (the session
    /// really is dead), never a panic or a silent no-op lie.
    ///
    /// Idempotent-ish: if a LIVE entry with the same id already exists, this is a no-op — a live
    /// session always wins over a stale persisted copy (boot only calls this before any session can
    /// have been created in the fresh process, but the guard also protects any future caller that
    /// might rehydrate against an already-populated supervisor). Overwrites an existing INACTIVE
    /// entry for the same id (e.g. a second rehydrate attempt) with the fresh persisted state.
    pub fn rehydrate_inactive(
        &self,
        meta: SessionMeta,
        scrollback: Vec<u8>,
    ) -> Result<(), SupervisorError> {
        {
            let sessions = self.sessions.lock().unwrap();
            if let Some(existing) = sessions.get(&meta.id) {
                if *existing.shared.is_active.lock().unwrap() {
                    // A live session always wins over a stale persisted copy.
                    return Ok(());
                }
            }
        }

        let mut ring = ScrollbackRing::new(SCROLLBACK_CAP);
        // `push` itself enforces the cap (keeping the tail), matching the ring's own semantics —
        // exceeding-cap persisted blobs are truncated exactly like a live ring would truncate.
        ring.push(&scrollback);

        let shared = Arc::new(Shared {
            id: meta.id.clone(),
            workspace_id: meta.workspace_id.clone(),
            title: meta.title.clone(),
            shell: meta.shell.clone(),
            cwd: Mutex::new(meta.cwd.clone()),
            cols: Mutex::new(meta.cols),
            rows: Mutex::new(meta.rows),
            lifecycle: Mutex::new(meta.lifecycle.clone()),
            is_active: Mutex::new(false),
            exit_code: Mutex::new(None),
            exit_signal: Mutex::new(None),
            grid: Mutex::new(LiveGrid::new(meta.cols, meta.rows)),
            scrollback: Mutex::new(ring),
            sinks: Mutex::new(Vec::new()),
            last_output: Mutex::new(Instant::now()),
            master_fd: None,
            waiting: Mutex::new(false),
            created_at: meta.created_at,
            pending_command_events: Mutex::new(Vec::new()),
            command_seq: AtomicU64::new(0),
        });

        let session = Arc::new(Session { shared, pty: None });
        self.sessions.lock().unwrap().insert(meta.id.clone(), session);
        Ok(())
    }
}

impl Default for Supervisor {
    fn default() -> Self {
        Self::new()
    }
}

/// Push one best-effort [`CommandEvent`] onto `shared`'s pending queue, assigning the next
/// monotonic per-session `seq` (spec §7). Called only from the reader thread as it parses OSC-133
/// C/D marks; never touches the DB (see the module-level threading contract) — the periodic flush
/// sweep in `socket_server.rs` drains and persists these separately.
fn push_command_event(shared: &Arc<Shared>, kind: &'static str, exit_code: Option<u8>) {
    let seq = shared.command_seq.fetch_add(1, Ordering::SeqCst);
    shared.pending_command_events.lock().unwrap().push(CommandEvent {
        seq,
        ts: now_secs(),
        kind,
        exit_code,
    });
}

/// Build + fire a [`StatusUpdate`] from the current shared state.
fn emit_status(cb: &StatusCb, shared: &Arc<Shared>) {
    cb(StatusUpdate {
        session_id: shared.id.clone(),
        lifecycle: shared.lifecycle.lock().unwrap().clone(),
        waiting_for_input: *shared.waiting.lock().unwrap(),
        cwd: shared.cwd.lock().unwrap().clone(),
    });
}

/// Recompute the §10.4 waiting-for-input heuristic and store it on `shared`.
///
/// `waiting_for_input == Running ∧ (ICANON&ECHO on the master) ∧ ¬alt-screen ∧
/// output-quiescent≥150 ms ∧ cursor-col≠0`. Documented **best-effort**: OSC-driven lifecycle is
/// child-forgeable (§10.3) and the tty/grid signals are inherently racy, so the broker/UI must
/// never present this as certain.
fn recompute_waiting(shared: &Arc<Shared>) {
    let is_running = matches!(*shared.lifecycle.lock().unwrap(), SessionLifecycle::Running);
    let quiescent = shared.last_output.lock().unwrap().elapsed() >= QUIESCENT;
    let (not_alt, not_col0) = {
        let grid = shared.grid.lock().unwrap();
        (!grid.is_alt_screen(), grid.cursor_col() != 0)
    };
    let line_mode = match shared.master_fd {
        Some(fd) => termios_icanon_echo(fd),
        None => false, // fail-safe: no fd ⇒ cannot confirm canonical line mode.
    };
    let waiting = is_running && line_mode && not_alt && quiescent && not_col0;
    *shared.waiting.lock().unwrap() = waiting;
}

/// True iff the PTY line discipline currently has both `ICANON` and `ECHO` set (canonical line
/// input, i.e. a program reading a whole line with echo — the `cat`/prompt case). Any
/// `tcgetattr` failure fails safe to `false`.
fn termios_icanon_echo(fd: RawFd) -> bool {
    // SAFETY: `fd` is the PTY master fd owned by the live `Session` (kept alive for the session's
    // lifetime); `tcgetattr` only reads into the zeroed `termios` and never retains the pointer.
    unsafe {
        let mut t: libc::termios = std::mem::zeroed();
        if libc::tcgetattr(fd, &mut t) != 0 {
            return false;
        }
        (t.c_lflag & libc::ICANON) != 0 && (t.c_lflag & libc::ECHO) != 0
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

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

    fn spec_for(shell: &str, args: Vec<String>) -> SessionSpec {
        SessionSpec {
            workspace_id: "ws-test".into(),
            shell: shell.into(),
            args,
            cwd: std::path::PathBuf::from("/tmp"),
            env: base_env(),
            cols: 80,
            rows: 24,
            title: "t".into(),
        }
    }

    fn drain_until(rx: &mpsc::Receiver<Vec<u8>>, needle: &[u8], timeout: Duration) -> Vec<u8> {
        let start = Instant::now();
        let mut acc = Vec::new();
        while start.elapsed() < timeout {
            if let Ok(chunk) = rx.recv_timeout(Duration::from_millis(100)) {
                acc.extend_from_slice(&chunk);
                if acc.windows(needle.len()).any(|w| w == needle) {
                    return acc;
                }
            }
        }
        acc
    }

    fn wait_for<F: Fn() -> bool>(f: F, timeout: Duration) -> bool {
        let start = Instant::now();
        while start.elapsed() < timeout {
            if f() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        f()
    }

    /// Returns true while a pid exists (`kill(pid, 0)` succeeds), false once it is gone.
    fn pid_alive(pid: i32) -> bool {
        unsafe { libc::kill(pid, 0) == 0 }
    }

    const ESC: u8 = 0x1b;

    // ---- §14.1: echo roundtrip (write to stdin, read it back through the master). ----
    #[test]
    fn echo_roundtrip_via_sh() {
        let sup = Supervisor::new();
        let id = sup.create(spec_for("/bin/sh", vec![])).expect("create");
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        sup.subscribe_output(&id, 1, tx).expect("subscribe");
        sup.write_stdin(&id, b"printf BPA_MARKER_OK\n")
            .expect("write");
        let out = drain_until(&rx, b"BPA_MARKER_OK", Duration::from_secs(5));
        assert!(
            out.windows(b"BPA_MARKER_OK".len())
                .any(|w| w == b"BPA_MARKER_OK"),
            "expected echoed marker in output, got: {}",
            String::from_utf8_lossy(&out)
        );
        sup.kill(&id).expect("kill");
    }

    // ---- §14.1: drop(slave) → EOF; child exit tears down + records the exit code. ----
    #[test]
    fn child_exit_marks_inactive_via_eof() {
        let sup = Supervisor::new();
        let mut spec = spec_for("/bin/sh", vec!["-c".into(), "exit 7".into()]);
        spec.title = "eoftest".into();
        let id = sup.create(spec).expect("create");

        let start = Instant::now();
        loop {
            let m = sup.meta(&id).expect("meta");
            if !m.is_active {
                match m.lifecycle {
                    bpa_protocol::SessionLifecycle::Exited { code, signal } => {
                        assert_eq!(code, Some(7), "exit code masked to u8");
                        assert_eq!(signal, None);
                    }
                    other => panic!("expected Exited, got {other:?}"),
                }
                break;
            }
            assert!(
                start.elapsed() < Duration::from_secs(5),
                "child never reached EOF/exit"
            );
            std::thread::sleep(Duration::from_millis(50));
        }
        // Clean up (session was auto-inactive but still mapped until kill()).
        let _ = sup.kill(&id);
    }

    // ---- §14.1: kill → reap (no zombie). ----
    #[test]
    fn kill_reaps_no_zombie() {
        let sup = Supervisor::new();
        let id = sup
            .create(spec_for("/bin/sh", vec!["-c".into(), "sleep 100".into()]))
            .expect("create");
        std::thread::sleep(Duration::from_millis(300));
        let m = sup.meta(&id).expect("meta");
        assert!(m.is_active, "session should be active before kill");
        sup.kill(&id).expect("kill");
        // After kill() the wait thread has joined (reaped). The session is removed from the map,
        // so meta() now reports NoSuchSession — proof it was fully torn down.
        assert!(
            matches!(sup.meta(&id), Err(SupervisorError::NoSuchSession(_))),
            "session must be gone (reaped + removed) after kill"
        );
    }

    // ---- §14.1: PROCESS-GROUP kill (forked grandchildren also die). ----
    #[test]
    fn kill_terminates_whole_process_group() {
        let sup = Supervisor::new();
        // The shell forks two background children in the SAME process group (no setsid), prints
        // both pids, then waits. killpg must take the whole group out.
        let script =
            "sleep 100 & child=$!; sleep 100 & gchild=$!; printf 'PIDS %d %d\\n' $child $gchild; wait";
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        let id = sup
            .create(spec_for("/bin/sh", vec!["-c".into(), script.into()]))
            .expect("create");
        sup.subscribe_output(&id, 1, tx).expect("subscribe");

        let out = drain_until(&rx, b"PIDS ", Duration::from_secs(5));
        let text = String::from_utf8_lossy(&out);
        let line = text
            .lines()
            .find(|l| l.contains("PIDS "))
            .expect("PIDS line");
        let nums: Vec<i32> = line
            .trim()
            .rsplit(' ')
            .take(2)
            .filter_map(|s| s.trim().parse::<i32>().ok())
            .collect();
        assert_eq!(nums.len(), 2, "expected two pids, got line: {line:?}");
        let (a, b) = (nums[0], nums[1]);
        assert!(
            pid_alive(a) && pid_alive(b),
            "both background children should be alive pre-kill"
        );

        sup.kill(&id).expect("kill");

        let start = Instant::now();
        while (pid_alive(a) || pid_alive(b)) && start.elapsed() < Duration::from_secs(4) {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(
            !pid_alive(a),
            "grandchild {a} must be killed by process-group kill"
        );
        assert!(
            !pid_alive(b),
            "grandchild {b} must be killed by process-group kill"
        );
    }

    // ---- §14.1: resize delivers SIGWINCH ($COLUMNS / stty size updates). ----
    #[test]
    fn resize_delivers_sigwinch_updated_columns() {
        let sup = Supervisor::new();
        // Trap WINCH and print the real termios window size via `stty size` ("rows cols").
        let script = "trap 'stty size' WINCH; printf READY\\n; while :; do sleep 0.2; done";
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        let id = sup
            .create(spec_for("/bin/sh", vec!["-c".into(), script.into()]))
            .expect("create");
        sup.subscribe_output(&id, 1, tx).expect("subscribe");

        let _ = drain_until(&rx, b"READY", Duration::from_secs(5));
        sup.resize(&id, 132, 40).expect("resize");

        let out = drain_until(&rx, b"132", Duration::from_secs(5));
        let text = String::from_utf8_lossy(&out);
        assert!(
            text.contains("40 132"),
            "expected 'rows cols' = '40 132' after resize, got: {text:?}"
        );
        sup.kill(&id).expect("kill");
    }

    // ---- §14.1 / §16: env hygiene — planted DAEMON_SECRET absent; allowlist present. ----
    #[test]
    fn env_clear_hides_daemon_secret_keeps_allowlist() {
        std::env::set_var("DAEMON_SECRET", "topsecret-should-not-leak");
        let sup = Supervisor::new();

        let mut spec = spec_for(
            "/bin/sh",
            vec![
                "-c".into(),
                "printf 'SECRET=[%s]\\n' \"$DAEMON_SECRET\"; printf 'TERM=[%s]\\n' \"$TERM\""
                    .into(),
            ],
        );
        spec.title = "envtest".into();

        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        let id = sup.create(spec).expect("create");
        sup.subscribe_output(&id, 1, tx).expect("subscribe");

        let out = drain_until(&rx, b"TERM=[xterm-256color]", Duration::from_secs(5));
        let text = String::from_utf8_lossy(&out);
        assert!(
            text.contains("SECRET=[]"),
            "DAEMON_SECRET must be cleared in the child env, got: {text:?}"
        );
        assert!(
            !text.contains("topsecret-should-not-leak"),
            "planted secret leaked into child env: {text:?}"
        );
        assert!(
            text.contains("TERM=[xterm-256color]"),
            "allowlisted TERM must be present, got: {text:?}"
        );
        sup.kill(&id).expect("kill");
        std::env::remove_var("DAEMON_SECRET");
    }

    // ---- §14.1 / §10.4: waiting-for-input — a shell at a partial prompt, blocked on read → true.
    //
    // A shell is the faithful "waiting for input" case: it emits a REAL `ESC ] 133 ; C BEL` into
    // its own output (so the reader parses it → Running — unlike `cat`, whose canonical ECHOCTL
    // would render our ESC as the visible caret `^[` and never reach the parser as a real mark),
    // prints a partial prompt with no trailing newline (cursor off column 0), and then blocks in
    // `read` (canonical line mode, ECHO on). That satisfies every §10.4 conjunct:
    // Running ∧ ICANON&ECHO ∧ ¬alt-screen ∧ quiescent≥150 ms ∧ cursor-col≠0. The ticker thread
    // surfaces the flip after the 150 ms quiescence with no further bytes.
    #[test]
    fn waiting_for_input_true_for_partial_prompt() {
        let sup = Supervisor::new();
        // \033]133;C\007 = the C mark; then a partial prompt (no newline); then block on read.
        let script = "printf '\\033]133;C\\007Password: '; read x";
        let id = sup
            .create(spec_for("/bin/sh", vec!["-c".into(), script.into()]))
            .expect("create");

        let sup_ref = &sup;
        let idc = id.clone();
        let ok = wait_for(
            || {
                sup_ref
                    .meta(&idc)
                    .map(|m| m.waiting_for_input)
                    .unwrap_or(false)
            },
            Duration::from_secs(3),
        );
        assert!(
            ok,
            "shell blocked at a partial prompt should be waiting_for_input"
        );
        sup.kill(&id).expect("kill");
    }

    // ---- §14.1 / §10.4: alt-screen (vim/less/top class) → false, even while Running. ----
    //
    // The child emits a REAL C mark (→ Running) AND enters the alt-screen buffer (`ESC[?1049h`),
    // with the cursor off column 0, then blocks on read. Every OTHER §10.4 conjunct holds — only
    // the alt-screen exclusion should force `false`. This is the genuine vim/less/top case (a shell
    // faithfully emits the real ESC sequences into its output; `cat` would mangle them via ECHOCTL).
    #[test]
    fn waiting_for_input_false_for_alt_screen() {
        let sup = Supervisor::new();
        let script = "printf '\\033]133;C\\007\\033[?1049hX'; read x";
        let id = sup
            .create(spec_for("/bin/sh", vec!["-c".into(), script.into()]))
            .expect("create");

        // Let the shell reach Running + alt-screen + block, and pass the quiescence window.
        std::thread::sleep(Duration::from_millis(700));
        let m = sup.meta(&id).expect("meta");
        assert_eq!(
            m.lifecycle,
            bpa_protocol::SessionLifecycle::Running,
            "precondition: the C mark must have driven lifecycle to Running"
        );
        assert!(
            !m.waiting_for_input,
            "alt-screen session must NOT be waiting_for_input even while Running"
        );
        sup.kill(&id).expect("kill");
    }

    // ---- §14.1 / §10.4: idle at prompt (lifecycle AtPrompt, not Running) → false. ----
    #[test]
    fn waiting_for_input_false_when_idle_at_prompt() {
        let sup = Supervisor::new();
        let id = sup.create(spec_for("/bin/cat", vec![])).expect("create");
        // No C mark ⇒ lifecycle stays AtPrompt ⇒ heuristic false regardless of tty mode.
        sup.write_stdin(&id, b"idle text ").expect("write");
        std::thread::sleep(Duration::from_millis(400));
        let m = sup.meta(&id).expect("meta");
        assert!(!m.waiting_for_input, "AtPrompt (not Running) must be false");
        sup.kill(&id).expect("kill");
    }

    // ---- §C: the live output sink receives verbatim bytes (parser does not filter). ----
    // The scrollback ring sanitizes (strips OSC-133/OSC-7/alt-screen for corruption-free replay);
    // the live sink must instead pass everything through so vim/less/etc. work for the client.
    #[test]
    fn live_sink_passes_bytes_through_verbatim_but_ring_sanitizes() {
        let sup = Supervisor::new();
        // Use a shell `printf` so REAL ESC bytes land in the child's output (canonical ECHOCTL
        // on `cat` would mangle written ESC into the caret `^[`). The child waits for a go-signal
        // on stdin FIRST (so we can subscribe before any output is produced — no lost-race), then
        // emits: an OSC-133 C mark (which the ring strips), an SGR-colored word + tail (kept
        // everywhere). `read` also disables ECHO of the go-signal is irrelevant — we match `tail`.
        let script = "read _go; printf '\\033]133;C\\007\\033[31mRED\\033[0mtail\\n'";
        let id = sup
            .create(spec_for("/bin/sh", vec!["-c".into(), script.into()]))
            .expect("create");
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        sup.subscribe_output(&id, 1, tx).expect("subscribe");
        // Release the child now that the sink is attached.
        sup.write_stdin(&id, b"go\n").expect("write go");

        // The live sink must deliver the SGR + text verbatim.
        let out = drain_until(&rx, b"tail", Duration::from_secs(5));
        assert!(
            out.windows(b"\x1b[31mRED\x1b[0m".len())
                .any(|w| w == b"\x1b[31mRED\x1b[0m"),
            "live sink must pass SGR through verbatim, got: {out:?}"
        );

        // Give the reader a moment to fold everything into the ring, then snapshot it.
        std::thread::sleep(Duration::from_millis(200));
        let (_c, _r, ring) = sup.snapshot_scrollback(&id).expect("snapshot");
        // The ring sanitizes: the OSC-133 mark is stripped for replay...
        let osc133 = {
            let mut m = vec![ESC, b']'];
            m.extend_from_slice(b"133;");
            m
        };
        assert!(
            !ring.windows(osc133.len()).any(|w| w == osc133.as_slice()),
            "scrollback ring must strip OSC-133 marks (replay hygiene)"
        );
        // ...but keeps the SGR + visible text.
        assert!(
            ring.windows(b"\x1b[31mRED\x1b[0m".len())
                .any(|w| w == b"\x1b[31mRED\x1b[0m"),
            "scrollback ring must keep SGR + text"
        );
        sup.kill(&id).expect("kill");
    }

    // ---- Callbacks fire for the broker (on_created / on_exited / on_status). ----
    #[test]
    fn callbacks_fire_for_broker() {
        use std::sync::atomic::{AtomicBool, Ordering};
        let sup = Supervisor::new();

        let created = Arc::new(AtomicBool::new(false));
        let exited = Arc::new(AtomicBool::new(false));
        let created_c = created.clone();
        let exited_c = exited.clone();
        sup.on_created(move |_meta| created_c.store(true, Ordering::SeqCst));
        sup.on_exited(move |_id, code, _sig| {
            if code == Some(0) {
                exited_c.store(true, Ordering::SeqCst);
            }
        });

        let id = sup
            .create(spec_for("/bin/sh", vec!["-c".into(), "exit 0".into()]))
            .expect("create");
        assert!(
            created.load(Ordering::SeqCst),
            "on_created must fire during create"
        );
        assert!(
            wait_for(|| exited.load(Ordering::SeqCst), Duration::from_secs(5)),
            "on_exited must fire when the child exits 0"
        );
        let _ = sup.kill(&id);
    }

    // ---- Supervisor is Send + Sync so the broker can hold Arc<Supervisor>. ----
    #[test]
    fn supervisor_is_send_and_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<Supervisor>();
    }

    // ---- Pv2 §5.1: N independent subscribers per session — both receive every chunk. ----
    #[test]
    fn two_subscribers_both_receive_output() {
        let sup = Supervisor::new();
        let id = sup
            .create(spec_for("/bin/sh", vec!["-c".into(), "echo hi; sleep 1".into()]))
            .expect("create");
        let (tx1, rx1) = mpsc::channel::<Vec<u8>>();
        let (tx2, rx2) = mpsc::channel::<Vec<u8>>();
        sup.subscribe_output(&id, 1, tx1).expect("subscribe 1");
        sup.subscribe_output(&id, 2, tx2).expect("subscribe 2");

        let out1 = drain_until(&rx1, b"hi", Duration::from_secs(5));
        let out2 = drain_until(&rx2, b"hi", Duration::from_secs(5));
        assert!(
            out1.windows(2).any(|w| w == b"hi"),
            "subscriber 1 must receive output, got: {out1:?}"
        );
        assert!(
            out2.windows(2).any(|w| w == b"hi"),
            "subscriber 2 must receive output, got: {out2:?}"
        );
        sup.kill(&id).expect("kill");
    }

    // ---- Pv2 §5.1: unsubscribe removes only that sink; the other keeps receiving. ----
    #[test]
    fn unsubscribe_stops_only_that_sink() {
        let sup = Supervisor::new();
        let script = "read _go; echo one; read _go2; echo two";
        let id = sup
            .create(spec_for("/bin/sh", vec!["-c".into(), script.into()]))
            .expect("create");
        let (tx1, rx1) = mpsc::channel::<Vec<u8>>();
        let (tx2, rx2) = mpsc::channel::<Vec<u8>>();
        sup.subscribe_output(&id, 1, tx1).expect("subscribe 1");
        sup.subscribe_output(&id, 2, tx2).expect("subscribe 2");

        sup.write_stdin(&id, b"go\n").expect("write go");
        let out1_first = drain_until(&rx1, b"one", Duration::from_secs(5));
        let out2_first = drain_until(&rx2, b"one", Duration::from_secs(5));
        assert!(out1_first.windows(3).any(|w| w == b"one"));
        assert!(out2_first.windows(3).any(|w| w == b"one"));

        sup.unsubscribe_output(&id, 1);
        sup.write_stdin(&id, b"go\n").expect("write go2");

        let out2_second = drain_until(&rx2, b"two", Duration::from_secs(5));
        assert!(
            out2_second.windows(3).any(|w| w == b"two"),
            "subscriber 2 must still receive output after 1 unsubscribes, got: {out2_second:?}"
        );

        // Subscriber 1 was unsubscribed BEFORE "two" was ever written, so it must never see it —
        // and since the sink is dropped, the sender-side `send` calls stop being attempted for id
        // 1, so this recv should time out (channel: no more producers, no more data).
        let late = rx1.recv_timeout(Duration::from_millis(500));
        if let Ok(chunk) = late {
            assert!(
                !chunk.windows(3).any(|w| w == b"two"),
                "unsubscribed sink 1 must not receive output sent after unsubscribe"
            );
        }
        sup.kill(&id).expect("kill");
    }

    // ---- Task 11 / spec §7: OSC-133 C/D marks accumulate into drainable CommandEvents. ----
    //
    // The child emits a real OSC-133 C mark, then a D;7 mark (exit code 7), matching the
    // shell-integration protocol end to end. After the marks are parsed, draining the session
    // must return `started` then `finished{exit_code: Some(7)}` in order, and draining again must
    // be empty (drain takes, does not peek).
    #[test]
    fn command_start_and_end_marks_accumulate_and_drain() {
        let sup = Supervisor::new();
        let script = "printf '\\033]133;C\\007'; printf '\\033]133;D;7\\007'; read x";
        let id = sup
            .create(spec_for("/bin/sh", vec!["-c".into(), script.into()]))
            .expect("create");

        // Poll lifecycle (not drain, to avoid consuming events before we're ready to assert on
        // them) until the D mark has landed and flipped it to Exited(Some(7)).
        let idc = id.clone();
        let ok = wait_for(
            || {
                matches!(
                    sup.meta(&idc).map(|m| m.lifecycle),
                    Ok(bpa_protocol::SessionLifecycle::Exited { code: Some(7), .. })
                )
            },
            Duration::from_secs(5),
        );
        assert!(ok, "expected lifecycle to reach Exited(Some(7)) via the D mark");

        let events = sup.drain_command_events(&id);
        assert_eq!(
            events.len(),
            2,
            "expected exactly a started+finished pair, got: {events:?}"
        );
        assert_eq!(events[0].kind, "started");
        assert_eq!(events[0].exit_code, None);
        assert_eq!(events[1].kind, "finished");
        assert_eq!(events[1].exit_code, Some(7));
        assert!(
            events[1].seq > events[0].seq,
            "seq must be monotonic: {events:?}"
        );

        // Draining again returns nothing — drain takes, it does not peek.
        assert!(sup.drain_command_events(&id).is_empty());

        sup.kill(&id).expect("kill");
    }

    // ---- Task 11: an unknown/gone session drains empty rather than erroring. ----
    #[test]
    fn drain_command_events_on_unknown_session_is_empty() {
        let sup = Supervisor::new();
        assert!(sup.drain_command_events("no-such-session").is_empty());
    }

    // ---- Pv2 §5.1: subscribe on an exited session still refuses (TOCTOU guard preserved). ----
    #[test]
    fn subscribe_on_exited_session_errors() {
        let sup = Supervisor::new();
        let id = sup
            .create(spec_for("/bin/sh", vec!["-c".into(), "exit 0".into()]))
            .expect("create");
        assert!(
            wait_for(
                || !sup.meta(&id).map(|m| m.is_active).unwrap_or(true),
                Duration::from_secs(5)
            ),
            "session must become inactive after exit"
        );
        let (tx, _rx) = mpsc::channel::<Vec<u8>>();
        let err = sup.subscribe_output(&id, 99, tx);
        assert!(
            matches!(err, Err(SupervisorError::NoSuchSession(_))),
            "subscribe on an exited session must return NoSuchSession, got: {err:?}"
        );
        let _ = sup.kill(&id);
    }

    // ---- Task 12r (Pv2 §7, BL-7 cold-rehydrate): `rehydrate_inactive` builds a tracked,
    // INACTIVE, PTY-less entry directly from persisted state — `meta()` reports the persisted
    // fields with `is_active=false`, and `snapshot_scrollback` returns the pre-filled bytes even
    // though no reader thread ever produced them. ----
    #[test]
    fn rehydrate_inactive_builds_tracked_inactive_entry_with_scrollback() {
        let sup = Supervisor::new();
        let meta_in = bpa_protocol::SessionMeta {
            id: "rehydrated-1".into(),
            workspace_id: "ws-test".into(),
            title: "old session".into(),
            shell: "/bin/zsh".into(),
            cwd: "/tmp/work".into(),
            cols: 100,
            rows: 40,
            lifecycle: bpa_protocol::SessionLifecycle::Exited {
                code: Some(0),
                signal: None,
            },
            waiting_for_input: true, // must be forced false by rehydrate regardless of input
            is_active: true,         // must be forced false by rehydrate regardless of input
            created_at: 1_700_000_000,
        };
        sup.rehydrate_inactive(meta_in.clone(), b"persisted scrollback\n".to_vec())
            .expect("rehydrate_inactive must succeed for a fresh id");

        let meta_out = sup.meta("rehydrated-1").expect("meta after rehydrate");
        assert!(!meta_out.is_active, "rehydrated entry must be inactive");
        assert!(
            !meta_out.waiting_for_input,
            "rehydrated entry must not be waiting_for_input"
        );
        assert_eq!(meta_out.cols, 100);
        assert_eq!(meta_out.rows, 40);
        assert_eq!(meta_out.workspace_id, "ws-test");
        assert_eq!(meta_out.title, "old session");
        assert_eq!(meta_out.shell, "/bin/zsh");
        assert_eq!(meta_out.cwd, "/tmp/work");
        assert_eq!(meta_out.created_at, 1_700_000_000);
        assert_eq!(meta_out.lifecycle, meta_in.lifecycle);

        let (cols, rows, bytes) = sup
            .snapshot_scrollback("rehydrated-1")
            .expect("snapshot_scrollback on a rehydrated entry must succeed");
        assert_eq!((cols, rows), (100, 40));
        assert_eq!(bytes, b"persisted scrollback\n".to_vec());
    }

    // ---- Task 12r: write_stdin/resize/kill on a rehydrated (PTY-less) entry must fail honestly
    // with SupervisorError::NoSuchSession — never panic, never silently no-op. ----
    #[test]
    fn rehydrated_entry_write_resize_kill_all_fail_honestly() {
        let sup = Supervisor::new();
        let meta = bpa_protocol::SessionMeta {
            id: "rehydrated-2".into(),
            workspace_id: "ws-test".into(),
            title: "t".into(),
            shell: "/bin/sh".into(),
            cwd: "/tmp".into(),
            cols: 80,
            rows: 24,
            lifecycle: bpa_protocol::SessionLifecycle::AtPrompt,
            waiting_for_input: false,
            is_active: false,
            created_at: 1,
        };
        sup.rehydrate_inactive(meta, Vec::new()).expect("rehydrate");

        assert!(
            matches!(
                sup.write_stdin("rehydrated-2", b"hi"),
                Err(SupervisorError::NoSuchSession(_))
            ),
            "write_stdin on a rehydrated entry must fail honestly, not panic/no-op"
        );
        assert!(
            matches!(
                sup.resize("rehydrated-2", 100, 30),
                Err(SupervisorError::NoSuchSession(_))
            ),
            "resize on a rehydrated entry must fail honestly, not panic/no-op"
        );
        assert!(
            matches!(
                sup.kill("rehydrated-2"),
                Err(SupervisorError::NoSuchSession(_))
            ),
            "kill on a rehydrated entry must fail honestly, not panic/no-op"
        );
        // The entry must still be present (kill's honest failure must not have removed it — that
        // would destroy the only surviving copy of its scrollback before a future attach replays
        // it).
        assert!(
            sup.meta("rehydrated-2").is_ok(),
            "a failed kill on a rehydrated entry must not remove it from the map"
        );
    }

    // ---- Task 12r: rehydrate is idempotent-ish — a LIVE entry always wins over a stale persisted
    // copy with the same id (boot-time defense in depth; the real boot loop never races a live
    // create, but future callers might). ----
    #[test]
    fn rehydrate_inactive_does_not_overwrite_a_live_entry_with_the_same_id() {
        let sup = Supervisor::new();
        let id = sup
            .create(spec_for("/bin/sh", vec!["-c".into(), "sleep 5".into()]))
            .expect("create");
        assert!(sup.meta(&id).unwrap().is_active);

        let stale_meta = bpa_protocol::SessionMeta {
            id: id.clone(),
            workspace_id: "ws-stale".into(),
            title: "stale".into(),
            shell: "/bin/stale-shell".into(),
            cwd: "/stale".into(),
            cols: 1,
            rows: 1,
            lifecycle: bpa_protocol::SessionLifecycle::AtPrompt,
            waiting_for_input: false,
            is_active: false,
            created_at: 0,
        };
        sup.rehydrate_inactive(stale_meta, b"stale bytes".to_vec())
            .expect("rehydrate_inactive must not error even when it's a no-op");

        let meta_after = sup.meta(&id).expect("meta after no-op rehydrate attempt");
        assert!(
            meta_after.is_active,
            "a live entry must never be overwritten by a stale rehydrate"
        );
        assert_ne!(
            meta_after.workspace_id, "ws-stale",
            "the live entry's real fields must survive untouched"
        );

        sup.kill(&id).expect("kill");
    }

    // ---- Task 12r: a persisted scrollback blob larger than the 256 KiB ring cap is truncated to
    // its tail on rehydrate, matching the ring's own live-session semantics (never silently keep
    // an unbounded blob in memory). ----
    #[test]
    fn rehydrate_inactive_truncates_oversized_scrollback_to_tail() {
        let sup = Supervisor::new();
        let meta = bpa_protocol::SessionMeta {
            id: "rehydrated-3".into(),
            workspace_id: "ws-test".into(),
            title: "t".into(),
            shell: "/bin/sh".into(),
            cwd: "/tmp".into(),
            cols: 80,
            rows: 24,
            lifecycle: bpa_protocol::SessionLifecycle::AtPrompt,
            waiting_for_input: false,
            is_active: false,
            created_at: 1,
        };
        let mut oversized = vec![b'A'; SCROLLBACK_CAP - 4];
        oversized.extend_from_slice(b"TAIL");
        oversized.extend_from_slice(&[b'B'; 100]); // push well past the cap
        sup.rehydrate_inactive(meta, oversized.clone())
            .expect("rehydrate");

        let (_c, _r, bytes) = sup
            .snapshot_scrollback("rehydrated-3")
            .expect("snapshot_scrollback");
        assert_eq!(
            bytes.len(),
            SCROLLBACK_CAP,
            "rehydrated ring must respect the same cap a live ring would"
        );
        assert_eq!(
            &bytes[bytes.len() - 100..],
            vec![b'B'; 100].as_slice(),
            "only the tail of an oversized persisted blob must survive"
        );
    }
}
