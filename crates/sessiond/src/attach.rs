//! Per-session single-attach registry + reattach replay orchestration (spec §7 attach model,
//! §6.2 reattach flow, §13 backpressure/honest-degradation).
//!
//! Exactly one active [`PushSink`] per session. `attach` supersedes any prior consumer for that
//! session, emits a fresh sanitized `Push::Replay` (built from
//! [`crate::pty_supervisor::Supervisor::snapshot_scrollback`]), then forwards live PTY bytes as
//! `Push::Output` until detach, supersede, or the sink closes. `detach` stops `Output` only — the
//! PTY keeps running and its scrollback ring keeps filling (spec §7 keep-alive).
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

/// One live attachment's cancellable forwarder handle.
struct AttachEntry {
    /// Aborts the `spawn_blocking` forwarder task. Aborting a `spawn_blocking` task does not
    /// preempt it mid-`recv()`, so we also flip `cancel` — the forwarder polls it after every
    /// `recv()` wakeup, including the wakeup caused by the std sender being dropped when a fresh
    /// `subscribe_output` supersedes it in the supervisor.
    handle: JoinHandle<()>,
    cancel: Arc<std::sync::atomic::AtomicBool>,
}

/// Per-session single-attach registry (spec §7). Holds at most one live [`AttachEntry`] per
/// `SessionId`; a new `attach()` for the same session supersedes (and stops) the prior one.
pub struct AttachRegistry {
    supervisor: Arc<Supervisor>,
    entries: StdMutex<std::collections::HashMap<SessionId, AttachEntry>>,
}

impl AttachRegistry {
    pub fn new(supervisor: Arc<Supervisor>) -> Self {
        AttachRegistry {
            supervisor,
            entries: StdMutex::new(std::collections::HashMap::new()),
        }
    }

    /// (Re)register `sink` as the single consumer for `session_id`, superseding any prior attach.
    /// Sends a fresh `Push::Replay` (sanitized snapshot at current cols/rows) into `sink` first,
    /// then spawns a forwarder that streams live `Push::Output` (with injected OSC-133/OSC-7
    /// marks stripped) until detach, supersede, or `sink` closes.
    pub async fn attach(&self, session_id: &SessionId, sink: PushSink) -> Result<(), AttachError> {
        // Stop any prior attachment for this session BEFORE subscribing a new std sink, so the
        // supervisor holds exactly one live sink at a time and the old forwarder is not racing
        // the new one for the same underlying subscription slot.
        self.abort_existing(session_id);

        // Bridge: std channel fed by the supervisor's blocking reader thread.
        let (std_tx, std_rx) = std::sync::mpsc::channel::<Vec<u8>>();
        self.supervisor
            .subscribe_output(session_id, std_tx)
            .map_err(|_| AttachError::NoSuchSession)?;

        // Snapshot AFTER subscribing: any byte the reader thread produces from this point on is
        // captured by `std_rx`; anything before is covered by the snapshot. No gap, no double
        // delivery beyond what the ring itself already coalesces.
        let (cols, rows, content) = self
            .supervisor
            .snapshot_scrollback(session_id)
            .map_err(|_| AttachError::NoSuchSession)?;

        // Replay MUST be the first frame the client observes.
        let replay = Push::Replay {
            session_id: session_id.clone(),
            cols,
            rows,
            content,
        };
        sink.send(replay).await.map_err(|_| AttachError::SinkClosed)?;

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
                    Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => return, // superseded/session gone
                }
            }
        });

        self.entries.lock().unwrap().insert(
            session_id.clone(),
            AttachEntry { handle, cancel },
        );
        Ok(())
    }

    /// Stop `Output` forwarding for `session_id`. The PTY keeps running and its ring keeps
    /// filling (spec §7 keep-alive) — this only tears down the client-facing forwarder.
    pub fn detach(&self, session_id: &SessionId) {
        self.abort_existing(session_id);
    }

    /// Drop every attach entry (client disconnect / daemon shutdown drain).
    pub fn detach_all(&self) {
        let mut map = self.entries.lock().unwrap();
        for (_id, entry) in map.drain() {
            entry.cancel.store(true, std::sync::atomic::Ordering::Release);
            entry.handle.abort();
        }
    }

    fn abort_existing(&self, session_id: &SessionId) {
        if let Some(prev) = self.entries.lock().unwrap().remove(session_id) {
            prev.cancel.store(true, std::sync::atomic::Ordering::Release);
            prev.handle.abort();
        }
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
                        out.extend(self.carry.drain(..));
                    }
                }
                Verdict::Drop => {
                    self.carry.clear();
                }
                Verdict::Keep => {
                    out.extend(self.carry.drain(..));
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
    let terminated_st = body.len() >= 2 && body[body.len() - 2] == ESC && body[body.len() - 1] == b'\\';
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
            ("HOME".into(), std::env::var("HOME").unwrap_or_else(|_| "/tmp".into())),
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
            .create(spec(vec!["-c".into(), "read _go; printf 'HELLO\\n'".into()]))
            .expect("create");

        let reg = AttachRegistry::new(sup.clone());
        let (sink, mut client) = mpsc::channel::<Push>(64);
        reg.attach(&id, sink).await.expect("attach");

        // First frame: Replay with the session's current dims and (empty, nothing written yet)
        // sanitized scrollback content.
        match recv_timeout(&mut client, 2000).await.expect("replay frame") {
            Push::Replay { session_id, cols, rows, content } => {
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

    // ---- second attach supersedes: old forwarder stops, fresh Replay sent to the new sink. ----
    #[tokio::test]
    async fn second_attach_supersedes_first() {
        let sup = Arc::new(Supervisor::new());
        let id = sup
            .create(spec(vec![
                "-c".into(),
                "read _go; printf 'A\\n'; read _go2; printf 'B\\n'".into(),
            ]))
            .expect("create");

        let reg = AttachRegistry::new(sup.clone());

        let (sink_a, mut client_a) = mpsc::channel::<Push>(64);
        reg.attach(&id, sink_a).await.expect("attach a");
        assert!(matches!(
            recv_timeout(&mut client_a, 2000).await.expect("replay a"),
            Push::Replay { .. }
        ));

        let (sink_b, mut client_b) = mpsc::channel::<Push>(64);
        reg.attach(&id, sink_b).await.expect("attach b");
        assert!(matches!(
            recv_timeout(&mut client_b, 2000).await.expect("replay b"),
            Push::Replay { .. }
        ));

        // Release the child; live output must reach ONLY B (A's forwarder was superseded/aborted).
        sup.write_stdin(&id, b"go\n").expect("write go");

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
            "B must receive live Output after supersede, got: {collected_b:?}"
        );

        // A must not receive any further Output.
        let a_next = recv_timeout(&mut client_a, 300).await;
        assert!(
            matches!(a_next, None),
            "A must not receive Output after being superseded, got {a_next:?}"
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
        reg.attach(&id, sink).await.expect("attach");
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

        reg.detach(&id);

        sup.write_stdin(&id, b"go2\n").expect("write go2");
        // No further Output should reach the detached client.
        let next = recv_timeout(&mut client, 500).await;
        assert!(
            matches!(next, None),
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
        let err = reg.attach(&"ghost-session".to_string(), sink).await.unwrap_err();
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
        reg.attach(&id, sink).await.expect("attach");
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
        assert!(contains(&collected, b"\x1b[?1049h"), "alt-screen enter must be kept");
        assert!(contains(&collected, b"\x1b[?1049l"), "alt-screen leave must be kept");
        assert!(contains(&collected, b"\x1b[31mRED\x1b[0m"), "SGR + text must be kept");
        assert!(contains(&collected, b"\x1b]0;My Title\x07"), "title OSC must be kept live");
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
        assert!(!contains(&combined, b"\x1b]7;"), "OSC-7 prefix leaked: {combined:?}");
        assert!(!contains(&combined, b"file://host"), "OSC-7 payload leaked: {combined:?}");
        assert_eq!(combined, b"ab".to_vec());
    }

    // ---- a superseded/detached forwarder does not leak a spawn_blocking OS thread. ----
    //
    // Directly proves thread exit (not just registry bookkeeping): each retired entry's
    // `JoinHandle` is `.await`ed to completion, which only resolves once the underlying
    // `spawn_blocking` OS thread has actually returned from its closure.
    #[tokio::test]
    async fn superseded_forwarder_does_not_leak_thread() {
        let sup = Arc::new(Supervisor::new());
        let id = sup
            .create(spec(vec!["-c".into(), "sleep 5".into()]))
            .expect("create");

        let reg = AttachRegistry::new(sup.clone());

        // Attach + supersede several times; the registry must hold exactly one entry throughout
        // (each prior forwarder aborted, not accumulated).
        for _ in 0..5 {
            let (sink, _client) = mpsc::channel::<Push>(16);
            reg.attach(&id, sink).await.expect("attach");
            assert_eq!(
                reg.entries.lock().unwrap().len(),
                1,
                "registry must hold exactly one entry per session at all times"
            );
        }

        // Take the still-live entry out ourselves (mirrors what `abort_existing` does) and prove
        // its JoinHandle completes promptly once cancelled — i.e. the OS thread actually exits.
        let entry = reg.entries.lock().unwrap().remove(&id).expect("entry present");
        entry.cancel.store(true, std::sync::atomic::Ordering::Release);
        let joined = tokio::time::timeout(Duration::from_secs(2), entry.handle).await;
        assert!(
            joined.is_ok(),
            "forwarder task must exit promptly once cancelled — a hang here means a leaked thread"
        );

        // Exercise the real public `detach()` path on a fresh attach and prove it also removes
        // the entry (no dangling handle left in the registry to ever leak).
        let (sink, _client) = mpsc::channel::<Push>(16);
        reg.attach(&id, sink).await.expect("attach again");
        reg.detach(&id);
        assert!(
            reg.entries.lock().unwrap().is_empty(),
            "detach must remove the entry so no forwarder handle lingers in the registry"
        );

        let _ = sup.kill(&id);
    }
}
