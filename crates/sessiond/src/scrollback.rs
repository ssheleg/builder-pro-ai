//! Sanitizing scrollback ring — the replay source (spec §11).
//! Stores the PTY's normal-buffer output with side-effecting control sequences neutralized
//! (alt-screen, title OSC, bracketed-paste, OSC-133/OSC-7 marks) while KEEPING SGR + text.
//! Replaying `snapshot()` into a fresh terminal cannot re-trigger those side effects.

use std::collections::VecDeque;

/// Cap the in-progress escape carry so a malicious/garbled stream can't grow it unbounded.
const CARRY_CAP: usize = 256;

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
        Sanitizer { carry: Vec::new() }
    }

    fn filter(&mut self, chunk: &[u8]) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::with_capacity(chunk.len());
        for &b in chunk {
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
                    if self.carry.len() > CARRY_CAP {
                        // Give up on this sequence: fail open, flush verbatim, reset.
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
    let terminated_st = body.len() >= 2 && body[body.len() - 2] == ESC && body[body.len() - 1] == b'\\';
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
}
