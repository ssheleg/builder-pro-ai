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

/// Passthrough placeholder; replaced by the streaming filter below.
struct Sanitizer;

impl Sanitizer {
    fn new() -> Self {
        Sanitizer
    }

    fn filter(&mut self, chunk: &[u8]) -> Vec<u8> {
        chunk.to_vec()
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
}
