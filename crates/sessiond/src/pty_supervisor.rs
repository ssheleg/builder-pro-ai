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
//! - [`Supervisor::subscribe_output`] — register an [`OutputSink`] (an `mpsc::Sender<Vec<u8>>`)
//!   that receives live PTY bytes verbatim; the attach layer forwards these as `Push::Output`.
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
//!    ring, and any registered live [`OutputSink`].
//! 5. `writer = master.take_writer()` (take-once) owned behind a `Mutex`; `flush()` after writes.
//! 6. `killer = child.clone_killer()` captured **before** the wait thread; `wait()` runs only on
//!    the single owning wait thread (`Child` is `Send` not `Sync` — never shared).
//! 7. `resize` → `master.resize` (delivers SIGWINCH) + tracked cols/rows + `LiveGrid::resize`.
//! 8. `kill`/teardown signals the whole **process group** via `libc::killpg` (long-lived agent
//!    CLIs / dev servers would otherwise orphan), then always `killer.kill()` + reap.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::fd::RawFd;
use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use portable_pty::{
    native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize, PtySystem,
};
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
    sink: Mutex<Option<OutputSink>>,
    last_output: Mutex<Instant>,
    master_fd: Option<RawFd>,
    waiting: Mutex<bool>,
    created_at: i64,
}

/// Owned per-session runtime state (the PTY master, writer, killer, and worker threads).
struct Session {
    shared: Arc<Shared>,
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
            sink: Mutex::new(None),
            last_output: Mutex::new(Instant::now()),
            master_fd,
            waiting: Mutex::new(false),
            created_at: now_secs(),
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
                                if let OscEvent::Cwd(path) = ev {
                                    *reader_shared.cwd.lock().unwrap() = path.clone();
                                }
                                status_dirty = true;
                            }

                            // (b) live grid, (c) sanitized scrollback ring.
                            reader_shared.grid.lock().unwrap().feed(chunk);
                            reader_shared.scrollback.lock().unwrap().push(chunk);

                            // (d) live broadcast — verbatim bytes to the attached client. The
                            // parser is a side-channel extractor and does NOT filter the stream,
                            // so the client gets everything (alt-screen, SGR, title, text) exactly
                            // as the child emitted it.
                            if let Some(sink) = reader_shared.sink.lock().unwrap().clone() {
                                let _ = sink.send(chunk.to_vec());
                            }

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
            master: Mutex::new(pair.master),
            writer: Mutex::new(writer),
            killer: Mutex::new(killer),
            pgid,
            reader_thread: Mutex::new(Some(reader_thread)),
            wait_thread: Mutex::new(Some(wait_thread)),
            ticker_stop,
            ticker_thread: Mutex::new(Some(ticker_thread)),
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
    /// (spec §9.5).
    pub fn write_stdin(&self, id: &str, bytes: &[u8]) -> Result<(), SupervisorError> {
        let s = self.get(id)?;
        let mut w = s.writer.lock().unwrap();
        w.write_all(bytes)
            .map_err(|e| SupervisorError::Io(format!("write_all: {e}")))?;
        w.flush().map_err(|e| SupervisorError::Io(format!("flush: {e}")))?;
        Ok(())
    }

    /// Resize the PTY (delivers SIGWINCH to the child) and update tracked cols/rows + the live
    /// grid (spec §9.7).
    pub fn resize(&self, id: &str, cols: u16, rows: u16) -> Result<(), SupervisorError> {
        let s = self.get(id)?;
        s.master
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

    /// Register (or replace) the live output sink for this session. Single-attach: a new sink
    /// supersedes any previous one (spec §7 attach model).
    pub fn subscribe_output(&self, id: &str, sink: OutputSink) -> Result<(), SupervisorError> {
        let s = self.get(id)?;
        *s.shared.sink.lock().unwrap() = Some(sink);
        Ok(())
    }

    /// `(cols, rows, sanitized_scrollback_bytes)` — the payload for `Push::Replay` (spec §6.2/§11).
    pub fn snapshot_scrollback(
        &self,
        id: &str,
    ) -> Result<(u16, u16, Vec<u8>), SupervisorError> {
        let s = self.get(id)?;
        let cols = *s.shared.cols.lock().unwrap();
        let rows = *s.shared.rows.lock().unwrap();
        let bytes = s.shared.scrollback.lock().unwrap().snapshot();
        Ok((cols, rows, bytes))
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
    pub fn kill(&self, id: &str) -> Result<(), SupervisorError> {
        let s = self.get(id)?;
        if let Some(pgid) = s.pgid {
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
        let _ = s.killer.lock().unwrap().kill();
        *s.ticker_stop.lock().unwrap() = true;
        if let Some(h) = s.wait_thread.lock().unwrap().take() {
            let _ = h.join();
        }
        if let Some(h) = s.reader_thread.lock().unwrap().take() {
            let _ = h.join();
        }
        if let Some(h) = s.ticker_thread.lock().unwrap().take() {
            let _ = h.join();
        }
        // Drop the session from the live map: its PTY is gone and it must not be re-driven.
        self.sessions.lock().unwrap().remove(id);
        Ok(())
    }

    /// Kill every live session (SIGTERM → grace → SIGKILL per group, then reap). Used on daemon
    /// shutdown (spec §9.8 / §13). Idempotent; safe to call with no live sessions.
    pub fn shutdown_all(&self) {
        let ids: Vec<SessionId> = self.sessions.lock().unwrap().keys().cloned().collect();
        for id in ids {
            let _ = self.kill(&id);
        }
    }
}

impl Default for Supervisor {
    fn default() -> Self {
        Self::new()
    }
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
            ("HOME".into(), std::env::var("HOME").unwrap_or_else(|_| "/tmp".into())),
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
        sup.subscribe_output(&id, tx).expect("subscribe");
        sup.write_stdin(&id, b"printf BPA_MARKER_OK\n").expect("write");
        let out = drain_until(&rx, b"BPA_MARKER_OK", Duration::from_secs(5));
        assert!(
            out.windows(b"BPA_MARKER_OK".len()).any(|w| w == b"BPA_MARKER_OK"),
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
        sup.subscribe_output(&id, tx).expect("subscribe");

        let out = drain_until(&rx, b"PIDS ", Duration::from_secs(5));
        let text = String::from_utf8_lossy(&out);
        let line = text.lines().find(|l| l.contains("PIDS ")).expect("PIDS line");
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
        assert!(!pid_alive(a), "grandchild {a} must be killed by process-group kill");
        assert!(!pid_alive(b), "grandchild {b} must be killed by process-group kill");
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
        sup.subscribe_output(&id, tx).expect("subscribe");

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
        sup.subscribe_output(&id, tx).expect("subscribe");

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
            || sup_ref.meta(&idc).map(|m| m.waiting_for_input).unwrap_or(false),
            Duration::from_secs(3),
        );
        assert!(ok, "shell blocked at a partial prompt should be waiting_for_input");
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
        sup.subscribe_output(&id, tx).expect("subscribe");
        // Release the child now that the sink is attached.
        sup.write_stdin(&id, b"go\n").expect("write go");

        // The live sink must deliver the SGR + text verbatim.
        let out = drain_until(&rx, b"tail", Duration::from_secs(5));
        assert!(
            out.windows(b"\x1b[31mRED\x1b[0m".len()).any(|w| w == b"\x1b[31mRED\x1b[0m"),
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
            ring.windows(b"\x1b[31mRED\x1b[0m".len()).any(|w| w == b"\x1b[31mRED\x1b[0m"),
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
        assert!(created.load(Ordering::SeqCst), "on_created must fire during create");
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
}

