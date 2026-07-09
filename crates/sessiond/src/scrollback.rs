//! Sanitizing scrollback ring — the replay source (spec §11).
//! Stores the PTY's normal-buffer output with side-effecting control sequences neutralized
//! (alt-screen, title OSC, bracketed-paste, OSC-133/OSC-7 marks) while KEEPING SGR + text.
//! Replaying `snapshot()` into a fresh terminal cannot re-trigger those side effects.

use std::collections::VecDeque;

/// Cap the in-progress escape carry so a malicious/garbled stream can't grow it unbounded.
/// This bound applies to sequences NOT YET identified as a recognized strippable OSC (see
/// `RECOGNIZED_OSC_CAP` below for those) — once a sequence exceeds this cap without being
/// classified, it fails open (flushed verbatim) so genuine long user output is never lost.
const CARRY_CAP: usize = 256;

/// Once a partial sequence is identified as a recognized strippable OSC (title 0/1/2,
/// OSC-7 cwd, OSC-133 semantic marks — spec §11), it must never fail open: leaking it would
/// let a replayed snapshot re-trigger the title/cwd side effect on a reattached terminal.
/// Instead we keep consuming and dropping bytes until the terminator, bounded only by this
/// larger safety cap (matches `osc_parser`'s cap) to protect against adversarial/unterminated
/// input growing the carry unboundedly.
const RECOGNIZED_OSC_CAP: usize = 8192;

pub struct ScrollbackRing {
    cap: usize,
    buf: VecDeque<u8>,
    filter: Sanitizer,
}

impl ScrollbackRing {
    pub fn new(cap: usize) -> Self {
        ScrollbackRing {
            cap,
            buf: VecDeque::new(),
            filter: Sanitizer::new(),
        }
    }

    /// Append a chunk, sanitizing side-effecting sequences, then enforce the byte cap.
    pub fn push(&mut self, chunk: &[u8]) {
        let kept = self.filter.filter(chunk);
        self.buf.extend(kept);
        self.prune();
    }

    /// Current sanitized contents, oldest → newest. This is the `Replay` payload (spec §6.2).
    pub fn snapshot(&self) -> Vec<u8> {
        self.buf.iter().copied().collect()
    }

    /// Drop oldest bytes until the ring is within `cap`.
    pub fn prune(&mut self) {
        while self.buf.len() > self.cap {
            self.buf.pop_front();
        }
    }
}

const ESC: u8 = 0x1B;
const BEL: u8 = 0x07;

/// Streaming filter that drops the enumerated side-effecting sequences (spec §11) and
/// passes everything else (SGR, cursor ops, text) through verbatim. Handles sequences
/// split across `filter` calls via an internal carry buffer.
struct Sanitizer {
    /// Bytes of an in-progress escape sequence not yet classified.
    carry: Vec<u8>,
    /// Set once a recognized strippable OSC's carry has exceeded `RECOGNIZED_OSC_CAP` without
    /// finding its terminator. While true, we have abandoned tracking the sequence's exact
    /// bytes (the carry itself is cleared to bound memory) but we must NOT fail open like the
    /// unrecognized-sequence path does — that would leak the tail of a recognized OSC
    /// (including its terminator) into `out`/`snapshot()` (spec §11). Instead every
    /// subsequent byte, in this call and any later `push()` calls, is dropped — never pushed
    /// to `carry` or `out` — until the terminator (BEL or ESC `\`) is found, at which point we
    /// drop it too and return to ground state. This mirrors the `overflowed` flag pattern in
    /// the sibling `osc_parser.rs`.
    discarding_until_terminator: bool,
}

#[derive(PartialEq)]
enum Verdict {
    /// The carry is a complete sequence to DROP.
    Drop,
    /// The carry is a complete sequence to KEEP (flush carry verbatim).
    Keep,
    /// Need more bytes to decide.
    Incomplete,
}

impl Sanitizer {
    fn new() -> Self {
        Sanitizer {
            carry: Vec::new(),
            discarding_until_terminator: false,
        }
    }

    fn filter(&mut self, chunk: &[u8]) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::with_capacity(chunk.len());
        for &b in chunk {
            if self.discarding_until_terminator {
                // Abandoned an over-long recognized OSC: drop every byte (never push to
                // `carry` or `out`) while scanning only for its terminator. BEL is a single
                // terminator byte; ST is the two-byte `ESC \` — since we're not accumulating
                // into `carry` here, we detect ST by looking for `\` immediately after we've
                // already seen an ESC in this discarded stream. We track that with a tiny
                // one-byte lookback via `carry` reused as a 0/1-length scratch buffer.
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
            // We are mid-escape: accumulate and classify.
            self.carry.push(b);
            match classify(&self.carry) {
                Verdict::Incomplete => {
                    if is_recognized_strippable_osc_prefix(&self.carry) {
                        // A recognized side-effecting OSC (title/OSC-7/OSC-133): it must
                        // NEVER fail open, since leaking it would let a replayed snapshot
                        // re-trigger the side effect (spec §11). Keep dropping bytes until
                        // the terminator, bounded only by the larger safety cap.
                        if self.carry.len() > RECOGNIZED_OSC_CAP {
                            // Adversarial/unterminated input: give up tracking the exact
                            // bytes (bound memory by clearing the carry), but do NOT return
                            // to ground state — enter discard-until-terminator mode so every
                            // remaining byte of this same in-flight OSC (including its
                            // eventual terminator), in this call or later push() calls, is
                            // dropped rather than leaked into `out`.
                            self.carry.clear();
                            self.discarding_until_terminator = true;
                        }
                    } else if self.carry.len() > CARRY_CAP {
                        // Not a recognized strippable sequence: give up, fail open, flush
                        // verbatim so genuine long user output is never silently lost.
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

/// True once `seq` is unambiguously the start of a recognized strippable OSC — i.e. it has
/// a complete leading identifier (`0`, `1`, `2`, `7`, `133`) followed by `;`, or is exactly
/// one of those idents so far while still possibly accumulating more ident digits. Used to
/// decide, before termination, whether a not-yet-complete OSC must be held (drop-until-
/// terminator, spec §11) rather than allowed to fail open past the small carry cap.
fn is_recognized_strippable_osc_prefix(seq: &[u8]) -> bool {
    if seq.len() < 2 || seq[0] != ESC || seq[1] != b']' {
        return false;
    }
    let body = &seq[2..];
    let ident_end = body.iter().position(|&c| c == b';');
    let ident = match ident_end {
        Some(end) => &body[..end],
        None => body, // no ';' yet — ident still accumulating
    };
    match ident_end {
        // Identifier terminated by ';': must be an exact recognized match.
        Some(_) => matches!(ident, b"0" | b"1" | b"2" | b"7" | b"133"),
        // Identifier still open: recognized only while it remains a valid prefix of one of
        // the recognized idents (so "13" while awaiting "133;" counts; "9" does not).
        None => {
            !ident.is_empty()
                && [b"0".as_slice(), b"1", b"2", b"7", b"133"]
                    .iter()
                    .any(|full| full.starts_with(ident))
        }
    }
}

/// Classify a candidate escape sequence (always starts with ESC).
/// Returns Drop for the enumerated side-effecting sequences, Keep for a completed
/// sequence to preserve, Incomplete if more bytes are required.
fn classify(seq: &[u8]) -> Verdict {
    // seq[0] == ESC guaranteed by caller.
    if seq.len() < 2 {
        return Verdict::Incomplete;
    }
    match seq[1] {
        b'[' => classify_csi(seq),
        b']' => classify_osc(seq),
        // Any other escape (e.g. ESC ( B charset, ESC = , ESC > ) — 2-byte, keep.
        _ => Verdict::Keep,
    }
}

/// CSI: ESC [ <params/intermediates> <final 0x40..=0x7E>. Drop the private-mode toggles
/// ?1049h/l, ?47h/l, ?2004h/l; keep every other CSI (SGR `m`, cursor, erase, …).
fn classify_csi(seq: &[u8]) -> Verdict {
    // Find the final byte (first byte in 0x40..=0x7E after the '[').
    let body = &seq[2..];
    let mut final_idx = None;
    for (i, &c) in body.iter().enumerate() {
        if (0x40..=0x7e).contains(&c) {
            final_idx = Some(i);
            break;
        }
    }
    let Some(fi) = final_idx else {
        return Verdict::Incomplete; // no final byte yet
    };
    let params = &body[..fi];
    let final_byte = body[fi];
    let is_toggle = final_byte == b'h' || final_byte == b'l';
    if is_toggle && (params == b"?1049" || params == b"?47" || params == b"?2004") {
        return Verdict::Drop;
    }
    Verdict::Keep
}

/// OSC: ESC ] <n> ; <text> <BEL | ESC \>. Drop title (0/1/2) and our marks (133, 7);
/// keep any other OSC once complete.
fn classify_osc(seq: &[u8]) -> Verdict {
    // seq = ESC ] ...
    let body = &seq[2..];
    // Determine if terminated (BEL, or ESC \ as the last two bytes).
    let terminated_bel = body.last() == Some(&BEL);
    let terminated_st =
        body.len() >= 2 && body[body.len() - 2] == ESC && body[body.len() - 1] == b'\\';
    if !terminated_bel && !terminated_st {
        return Verdict::Incomplete;
    }
    // Extract the leading numeric identifier up to the first ';'.
    let ident_end = body.iter().position(|&c| c == b';').unwrap_or(body.len());
    let ident = &body[..ident_end];
    match ident {
        b"0" | b"1" | b"2" | b"133" | b"7" => Verdict::Drop,
        _ => Verdict::Keep,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_round_trips_oldest_to_newest() {
        let mut r = ScrollbackRing::new(1024);
        r.push(b"hello ");
        r.push(b"world");
        assert_eq!(r.snapshot(), b"hello world".to_vec());
    }

    #[test]
    fn ring_enforces_byte_cap_dropping_oldest() {
        let mut r = ScrollbackRing::new(8);
        r.push(b"ABCDE"); // 5 bytes
        r.push(b"FGHIJ"); // total 10 → prune oldest 2 → "CDEFGHIJ"
        let snap = r.snapshot();
        assert_eq!(snap.len(), 8);
        assert_eq!(snap, b"CDEFGHIJ".to_vec());
    }

    #[test]
    fn push_larger_than_cap_keeps_only_tail() {
        let mut r = ScrollbackRing::new(4);
        r.push(b"ABCDEFGH");
        assert_eq!(r.snapshot(), b"EFGH".to_vec());
    }

    #[test]
    fn explicit_prune_is_idempotent() {
        let mut r = ScrollbackRing::new(4);
        r.push(b"ABCDEF");
        r.prune();
        r.prune();
        assert_eq!(r.snapshot(), b"CDEF".to_vec());
    }

    fn strip(input: &[u8]) -> Vec<u8> {
        let mut r = ScrollbackRing::new(1 << 20);
        r.push(input);
        r.snapshot()
    }

    #[test]
    fn keeps_sgr_and_plain_text() {
        // ESC[31m red ESC[0m — SGR must be preserved verbatim.
        let input = b"\x1b[31mred\x1b[0m done";
        assert_eq!(strip(input), input.to_vec());
    }

    #[test]
    fn strips_alt_screen_enter_leave_1049_and_47() {
        let input = b"before\x1b[?1049hMID\x1b[?1049lafter";
        assert_eq!(strip(input), b"beforeMIDafter".to_vec());
        let input47 = b"a\x1b[?47hb\x1b[?47lc";
        assert_eq!(strip(input47), b"abc".to_vec());
    }

    #[test]
    fn strips_bracketed_paste_toggles_2004() {
        let input = b"x\x1b[?2004hy\x1b[?2004lz";
        assert_eq!(strip(input), b"xyz".to_vec());
    }

    #[test]
    fn strips_title_osc_0_1_2_bel_and_st() {
        let bel = b"a\x1b]0;My Title\x07b".to_vec();
        assert_eq!(strip(&bel), b"ab".to_vec());
        let osc1 = b"a\x1b]1;icon\x07b".to_vec();
        assert_eq!(strip(&osc1), b"ab".to_vec());
        // ST-terminated title.
        let st = b"a\x1b]2;t\x1b\\b".to_vec();
        assert_eq!(strip(&st), b"ab".to_vec());
    }

    #[test]
    fn strips_osc_133_and_osc_7_marks() {
        let input = b"\x1b]133;A\x07prompt$ \x1b]133;B\x07cmd\x1b]133;C\x07out\x1b]133;D;0\x07";
        assert_eq!(strip(input), b"prompt$ cmdout".to_vec());
        let osc7 = b"p\x1b]7;file://h/Users/me\x07q".to_vec();
        assert_eq!(strip(&osc7), b"pq".to_vec());
    }

    #[test]
    fn keeps_cursor_moves_and_erases() {
        // ESC[2J (erase), ESC[H (cursor home) are not in the strip list → kept.
        let input = b"\x1b[2J\x1b[Hhome";
        assert_eq!(strip(input), input.to_vec());
    }

    #[test]
    fn split_alt_screen_sequence_across_pushes_is_stripped() {
        let mut r = ScrollbackRing::new(1 << 20);
        r.push(b"pre\x1b[?10"); // sequence split mid-way
        r.push(b"49hpost");
        assert_eq!(r.snapshot(), b"prepost".to_vec());
    }

    #[test]
    fn replay_of_past_vim_session_has_no_side_effecting_sequences() {
        let mut r = ScrollbackRing::new(1 << 20);
        // Simulate: prompt marks, run `vim` (alt-screen + title), quit, back to prompt.
        r.push(b"\x1b]133;A\x07\x1b]7;file://h/home/u\x07me@host:~$ ");
        r.push(b"\x1b]133;B\x07vim file.txt\n\x1b]133;C\x07");
        r.push(b"\x1b]0;VIM - file.txt\x07"); // title set by vim
        r.push(b"\x1b[?1049h"); // enter alt-screen
        r.push(b"\x1b[?2004h~ editing ~\x1b[?2004l"); // paste toggles inside vim
        r.push(b"\x1b[?1049l"); // leave alt-screen
        r.push(b"\x1b]133;D;0\x07"); // command finished
        r.push(b"\x1b]133;A\x07me@host:~$ "); // fresh prompt
        let snap = r.snapshot();

        // None of the side-effecting sequences survive.
        assert!(!contains(&snap, b"\x1b[?1049h"), "alt-screen enter leaked");
        assert!(!contains(&snap, b"\x1b[?1049l"), "alt-screen leave leaked");
        assert!(
            !contains(&snap, b"\x1b[?2004h"),
            "bracketed-paste enter leaked"
        );
        assert!(
            !contains(&snap, b"\x1b[?2004l"),
            "bracketed-paste leave leaked"
        );
        assert!(!contains(&snap, b"\x1b]0;"), "title OSC leaked");
        assert!(!contains(&snap, b"\x1b]133;"), "OSC-133 mark leaked");
        assert!(!contains(&snap, b"\x1b]7;"), "OSC-7 mark leaked");

        // Normal-buffer text + the interior vim text survive.
        assert!(contains(&snap, b"me@host:~$ "), "prompt text lost");
        assert!(contains(&snap, b"vim file.txt"), "command echo lost");
        assert!(contains(&snap, b"~ editing ~"), "interior text lost");
    }

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|w| w == needle)
    }

    #[test]
    fn strips_title_osc_longer_than_carry_cap() {
        // Title OSC (ident "2") whose payload exceeds CARRY_CAP (256 bytes) must still be
        // fully stripped — it must NOT fail-open and leak into the snapshot, since replaying
        // a leaked title would re-trigger the title side effect on reattach (spec §11).
        let mut payload = b"a".repeat(300);
        let mut input = b"pre\x1b]2;".to_vec();
        input.append(&mut payload);
        input.push(BEL);
        input.extend_from_slice(b"post");
        let snap = strip(&input);
        assert!(!contains(&snap, b"\x1b]2;"), "long title OSC prefix leaked");
        assert!(
            !snap.windows(50).any(|w| w.iter().all(|&b| b == b'a')),
            "long title OSC payload leaked"
        );
        assert_eq!(snap, b"prepost".to_vec());
    }

    #[test]
    fn strips_osc_7_longer_than_carry_cap() {
        // OSC-7 (cwd) whose payload exceeds CARRY_CAP (256 bytes) — e.g. a long cwd path —
        // must still be fully stripped; leaking it would re-trigger the cwd side effect.
        let mut payload = b"a".repeat(300);
        let mut input = b"pre\x1b]7;file://host/".to_vec();
        input.append(&mut payload);
        input.push(BEL);
        input.extend_from_slice(b"post");
        let snap = strip(&input);
        assert!(!contains(&snap, b"\x1b]7;"), "long OSC-7 prefix leaked");
        assert!(
            !contains(&snap, b"file://host/"),
            "long OSC-7 payload leaked"
        );
        assert_eq!(snap, b"prepost".to_vec());
    }

    #[test]
    fn recognized_osc_exceeding_cap_across_multiple_pushes_never_leaks_payload_or_terminator() {
        // Regression guard for the "clear() on cap overflow" bug: a recognized strippable
        // OSC (title, ident "0") whose payload exceeds RECOGNIZED_OSC_CAP (8 KiB), fed across
        // MANY push() calls, must be fully discarded — including its eventual terminator —
        // even though the terminator arrives long after the cap was exceeded. If the carry is
        // merely `clear()`-ed at the cap, the per-byte dispatch treats the (now-empty) carry
        // as ground state and starts flushing the rest of the in-flight OSC (payload +
        // terminator) straight into `out`, leaking it into the snapshot. That must not happen.
        let mut r = ScrollbackRing::new(1 << 20);
        r.push(b"\x1b]0;"); // recognized title OSC prefix
                            // 85 * 100 = 8500 bytes, comfortably past the 8192-byte RECOGNIZED_OSC_CAP, fed in
                            // many separate push() calls to exercise the carry-across-push path.
        for _ in 0..85 {
            r.push(&[b'A'; 100]);
        }
        r.push(&[BEL]); // the (still in-flight, now-late) terminator
        r.push(b"prompt$ "); // trailing normal text after the OSC finally terminates
        let snap = r.snapshot();

        assert!(
            !contains(&snap, b"AAAAAAAAAA"),
            "over-long recognized OSC payload leaked into snapshot"
        );
        assert!(
            !snap.contains(&BEL),
            "OSC terminator (BEL) leaked into snapshot"
        );
        assert!(
            !contains(&snap, b"\x1b]0;"),
            "title OSC prefix leaked into snapshot"
        );
        assert!(
            contains(&snap, b"prompt$ "),
            "trailing prompt text after the OSC was lost"
        );
    }

    #[test]
    fn recognized_osc_st_terminator_split_across_two_pushes_in_discard_mode_never_leaks() {
        // Regression guard, committed (a prior reviewer's probe covered this shape but it was
        // never checked in — see Task 25). Distinct from
        // `recognized_osc_exceeding_cap_across_multiple_pushes_never_leaks_payload_or_terminator`
        // above: that test uses a BEL (single-byte) terminator arriving whole in one `push()`.
        // This test specifically targets the ST terminator (`ESC` `\`, TWO bytes) being split
        // across the `discarding_until_terminator` state boundary itself — `ESC` lands in one
        // `push()` call and the closing `\` lands in the NEXT `push()` call. The
        // `discarding_until_terminator` one-byte lookback (`self.carry` reused as a 0/1-length
        // scratch buffer, see `Sanitizer::filter`) must survive across the call boundary so the
        // second call's leading `\` is still recognized as completing the ST terminator rather
        // than being treated as a fresh, unrelated byte (which would either leak it or fail to
        // exit discard mode, corrupting everything after).
        let mut r = ScrollbackRing::new(1 << 20);
        // Enter discard-until-terminator mode: a title OSC (ident "0") whose payload exceeds
        // RECOGNIZED_OSC_CAP (8192 bytes) within a single push().
        r.push(b"pre\x1b]0;");
        r.push(&[b'X'; RECOGNIZED_OSC_CAP + 100]); // carry.clear() + discarding_until_terminator=true
                                                   // Now split the two-byte ST terminator itself across two separate push() calls: ESC in
                                                   // this call, with NOTHING after it (so this push() ends mid-terminator).
        r.push(&[ESC]);
        // The closing '\' arrives in the NEXT push() call, followed by trailing normal text.
        r.push(b"\\post");
        let snap = r.snapshot();

        assert!(
            !contains(&snap, b"\x1b]0;"),
            "title OSC prefix leaked into snapshot: {snap:?}"
        );
        assert!(
            !snap.windows(20).any(|w| w.iter().all(|&b| b == b'X')),
            "over-long recognized OSC payload leaked into snapshot: {snap:?}"
        );
        assert!(
            !contains(&snap, &[ESC, b'\\']),
            "ST terminator (split across two push() calls) leaked into snapshot: {snap:?}"
        );
        assert_eq!(
            snap,
            b"prepost".to_vec(),
            "only the text before and after the discarded OSC must survive, got: {snap:?}"
        );
    }

    #[test]
    fn unrecognized_long_partial_escape_still_fails_open() {
        // A long run of bytes that merely starts with ESC but never resolves to a
        // recognized/complete sequence (never hits '[' or ']') must still fail-open and be
        // preserved verbatim once it exceeds CARRY_CAP — genuine data must not be silently
        // dropped just because it started with an ESC byte.
        let mut r = ScrollbackRing::new(1 << 20);
        let mut chunk = vec![ESC];
        chunk.extend(std::iter::repeat_n(b'Q', 300));
        r.push(&chunk);
        r.push(b"tail");
        let snap = r.snapshot();
        assert!(
            contains(&snap, &vec![b'Q'; 300]),
            "unrecognized long partial escape must fail open (be preserved)"
        );
        assert!(contains(&snap, b"tail"), "trailing text lost");
    }
}
