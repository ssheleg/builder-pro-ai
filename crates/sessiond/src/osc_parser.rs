//! Streaming OSC-133/OSC-7 tokenizer + lifecycle state machine (spec §10.3).
//! Buffers partial OSC across `feed` boundaries; accepts BEL/ST/implicit-ESC terminators;
//! caps the OSC buffer at 8 KiB; hardened against forged/oversized/interleaved input.
//!
//! `feed` is a SIDE-CHANNEL event extractor: it returns only the recognized `OscEvent`s,
//! never a filtered byte stream. The caller (T9 `pty_supervisor`) feeds the same raw PTY
//! bytes to the ring/grid/output separately; nothing here suppresses or rewrites the
//! original stream.
//!
//! Lifecycle states reuse `bpa_protocol::SessionLifecycle` (the locked wire contract) —
//! this module does not redefine or re-derive `Serialize`/`Deserialize` on it.

use bpa_protocol::SessionLifecycle;

const BEL: u8 = 0x07;
const ESC: u8 = 0x1B;
const OSC_INTRODUCER: u8 = b']';
const ST_FINAL: u8 = b'\\';
const OSC_BUF_CAP: usize = 8 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// Normal pass-through scanning.
    Ground,
    /// Saw ESC, awaiting the next byte (could be `]` → OSC, or anything else).
    Escape,
    /// Inside an OSC payload, accumulating until a terminator.
    Osc,
    /// Inside OSC, saw ESC — awaiting `\` (ST) or a new sequence start.
    OscEsc,
}

/// A recognized OSC-133/OSC-7 event extracted from the PTY stream (spec §10.1, §10.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OscEvent {
    /// OSC 133 ; A — prompt start.
    PromptStart,
    /// OSC 133 ; B — prompt end / command start.
    PromptEnd,
    /// OSC 133 ; C — output start (command running).
    CommandStart,
    /// OSC 133 ; D ; `<code>` — command finished. `None` = unknown/aborted, never coerced to 0.
    CommandEnd(Option<u8>),
    /// OSC 7 — cwd report (`file://` or `kitty-shell-cwd://`), percent-decoded, host stripped.
    Cwd(String),
}

/// Streaming OSC tokenizer. Owns partial-sequence buffer state across `feed` calls.
pub struct OscParser {
    state: State,
    buf: Vec<u8>,
    /// True once the in-progress OSC exceeded the cap; the sequence is abandoned.
    overflowed: bool,
}

impl Default for OscParser {
    fn default() -> Self {
        Self::new()
    }
}

impl OscParser {
    pub fn new() -> Self {
        OscParser {
            state: State::Ground,
            buf: Vec::new(),
            overflowed: false,
        }
    }

    /// Feed a chunk of raw PTY bytes; returns the recognized OSC events in order.
    /// Never returns the pass-through bytes — this is a side-channel event extractor
    /// (spec §10.3); the caller feeds the same raw chunk to the ring/grid separately.
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<OscEvent> {
        let mut out = Vec::new();
        for &b in chunk {
            match self.state {
                State::Ground => {
                    if b == ESC {
                        self.state = State::Escape;
                    }
                    // all other Ground bytes are pass-through (no event)
                }
                State::Escape => {
                    if b == OSC_INTRODUCER {
                        self.state = State::Osc;
                        self.buf.clear();
                        self.overflowed = false;
                    } else if b == ESC {
                        // Another ESC restarts the escape.
                        self.state = State::Escape;
                    } else {
                        // Not an OSC (e.g. CSI `[`); back to ground.
                        self.state = State::Ground;
                    }
                }
                State::Osc => {
                    if b == BEL {
                        self.finish_osc(&mut out);
                        self.state = State::Ground;
                    } else if b == ESC {
                        self.state = State::OscEsc;
                    } else {
                        self.push_osc_byte(b);
                    }
                }
                State::OscEsc => {
                    if b == ST_FINAL {
                        // ESC \ = ST terminator.
                        self.finish_osc(&mut out);
                        self.state = State::Ground;
                    } else if b == OSC_INTRODUCER {
                        // Implicit-ESC terminator: current OSC ends, a NEW OSC begins.
                        self.finish_osc(&mut out);
                        self.state = State::Osc;
                        self.buf.clear();
                        self.overflowed = false;
                    } else if b == ESC {
                        // ESC ESC: stay awaiting a final.
                        self.state = State::OscEsc;
                    } else {
                        // Implicit-ESC terminator into a non-OSC sequence: end current OSC,
                        // treat the ESC+b as a fresh escape sequence (ground for our purposes).
                        self.finish_osc(&mut out);
                        self.state = State::Ground;
                    }
                }
            }
        }
        out
    }

    fn push_osc_byte(&mut self, b: u8) {
        if self.overflowed {
            return;
        }
        if self.buf.len() >= OSC_BUF_CAP {
            // Abandon this oversized OSC; keep scanning for its terminator.
            self.overflowed = true;
            self.buf.clear();
            return;
        }
        self.buf.push(b);
    }

    fn finish_osc(&mut self, out: &mut Vec<OscEvent>) {
        if self.overflowed {
            self.buf.clear();
            self.overflowed = false;
            return;
        }
        if let Some(ev) = parse_osc_payload(&self.buf) {
            out.push(ev);
        }
        self.buf.clear();
    }
}

/// Parse a complete OSC payload (bytes after `ESC ]`, before the terminator).
fn parse_osc_payload(payload: &[u8]) -> Option<OscEvent> {
    // OSC 133 ; <letter> [ ; <args> ]
    if let Some(rest) = payload.strip_prefix(b"133;") {
        return parse_133(rest);
    }
    // OSC 7 ; <uri>
    if let Some(rest) = payload.strip_prefix(b"7;") {
        return parse_osc7(rest);
    }
    None
}

fn parse_133(rest: &[u8]) -> Option<OscEvent> {
    let letter = *rest.first()?;
    match letter {
        b'A' => Some(OscEvent::PromptStart),
        b'B' => Some(OscEvent::PromptEnd),
        b'C' => Some(OscEvent::CommandStart),
        b'D' => Some(OscEvent::CommandEnd(parse_exit_code(&rest[1..]))),
        _ => None,
    }
}

/// Exit-code rule (spec §10.3): `D` then optional `;<code>`; base-10 in 0..=255 → Some,
/// empty / non-numeric / out-of-range → None. Never coerce to 0. Ignore trailing `;aid=..`.
fn parse_exit_code(after_d: &[u8]) -> Option<u8> {
    // after_d is either empty (bare D) or starts with ';' then the code (and maybe more args).
    let s = after_d.strip_prefix(b";")?;
    // Take the first field up to the next ';'.
    let field: &[u8] = match s.iter().position(|&c| c == b';') {
        Some(i) => &s[..i],
        None => s,
    };
    if field.is_empty() {
        return None;
    }
    let text = std::str::from_utf8(field).ok()?;
    match text.parse::<u32>() {
        Ok(n) if n <= 255 => Some(n as u8),
        _ => None,
    }
}

/// Decode an OSC 7 payload (the bytes after `7;`): accept `file://host/path` and
/// `kitty-shell-cwd://host/path` only, percent-decode, strip host, bound length.
/// Malformed input → None (dropped safely). cwd is advisory display data (spec §10.3, §16).
fn parse_osc7(rest: &[u8]) -> Option<OscEvent> {
    if rest.len() > OSC_BUF_CAP {
        return None;
    }
    let s = std::str::from_utf8(rest).ok()?;
    let after_scheme = s
        .strip_prefix("file://")
        .or_else(|| s.strip_prefix("kitty-shell-cwd://"))?;
    // after_scheme = "host/abs/path" or "/abs/path" (empty host). Path starts at the first '/'.
    let slash = after_scheme.find('/')?;
    let raw_path = &after_scheme[slash..];
    let decoded = percent_decode(raw_path)?;
    if !decoded.starts_with('/') {
        return None;
    }
    Some(OscEvent::Cwd(decoded))
}

/// Minimal, strict percent-decoder. Returns None on a malformed escape.
fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                if i + 2 >= bytes.len() {
                    return None;
                }
                let hi = (bytes[i + 1] as char).to_digit(16)?;
                let lo = (bytes[i + 2] as char).to_digit(16)?;
                out.push((hi * 16 + lo) as u8);
                i += 3;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}

/// Advance a `SessionLifecycle` by one `OscEvent` (spec §10.3 transition table).
///
/// `PromptStart` (`A`) only draws the prompt: from `Exited`, it moves back toward the
/// prompt (`AtPrompt`); from `AtPrompt`/`Running` it is a no-op on its own — this is what
/// makes the empty-command `PromptEnd → PromptStart` sequence (`B → A` with no `C`/`D`) a
/// no-op overall (stays `AtPrompt`, no phantom `Running`). `PromptEnd` (`B`) → `AtPrompt`.
/// `CommandStart` (`C`) → `Running`. `CommandEnd` (`D;code`) → `Exited{code, signal:None}`
/// (the parser never observes a signal; that's a supervisor-level fact). `Cwd` never
/// changes lifecycle. `Typing` is never produced by this state machine.
pub fn advance_lifecycle(lifecycle: &mut SessionLifecycle, ev: &OscEvent) {
    match ev {
        OscEvent::PromptStart => {
            if let SessionLifecycle::Exited { .. } = lifecycle {
                *lifecycle = SessionLifecycle::AtPrompt;
            }
            // From AtPrompt/Running/Typing, `A` alone changes nothing.
        }
        OscEvent::PromptEnd => {
            *lifecycle = SessionLifecycle::AtPrompt;
        }
        OscEvent::CommandStart => {
            *lifecycle = SessionLifecycle::Running;
        }
        OscEvent::CommandEnd(code) => {
            *lifecycle = SessionLifecycle::Exited {
                code: *code,
                signal: None,
            };
        }
        OscEvent::Cwd(_) => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const BEL: u8 = 0x07;
    const ESC: u8 = 0x1B;
    const ST: [u8; 2] = [ESC, b'\\'];

    fn osc(body: &str, term: &[u8]) -> Vec<u8> {
        let mut v = vec![ESC, b']'];
        v.extend_from_slice(body.as_bytes());
        v.extend_from_slice(term);
        v
    }

    #[test]
    fn parses_full_133_lifecycle_bel_terminated() {
        let mut p = OscParser::new();
        let mut stream = Vec::new();
        stream.extend_from_slice(&osc("133;A", &[BEL]));
        stream.extend_from_slice(b"user@host $ ");
        stream.extend_from_slice(&osc("133;B", &[BEL]));
        stream.extend_from_slice(b"ls -la\n");
        stream.extend_from_slice(&osc("133;C", &[BEL]));
        stream.extend_from_slice(b"file1 file2\n");
        stream.extend_from_slice(&osc("133;D;0", &[BEL]));
        let events = p.feed(&stream);
        assert_eq!(
            events,
            vec![
                OscEvent::PromptStart,
                OscEvent::PromptEnd,
                OscEvent::CommandStart,
                OscEvent::CommandEnd(Some(0)),
            ]
        );
    }

    #[test]
    fn non_osc_bytes_produce_no_events() {
        let mut p = OscParser::new();
        assert_eq!(
            p.feed(b"plain text with \x1b[31mSGR\x1b[0m color"),
            Vec::<OscEvent>::new()
        );
    }

    #[test]
    fn osc_split_across_feeds_is_buffered() {
        let mut p = OscParser::new();
        // Split "ESC ] 1 3 3 ; D ; 4 2 BEL" across three feeds.
        assert_eq!(p.feed(&[ESC, b']', b'1', b'3']), Vec::<OscEvent>::new());
        assert_eq!(p.feed(b"3;D;4"), Vec::<OscEvent>::new());
        assert_eq!(p.feed(&[b'2', BEL]), vec![OscEvent::CommandEnd(Some(42))]);
    }

    #[test]
    fn st_terminator_accepted() {
        let mut p = OscParser::new();
        assert_eq!(p.feed(&osc("133;C", &ST)), vec![OscEvent::CommandStart]);
    }

    #[test]
    fn implicit_esc_terminator_ends_and_starts_new_osc() {
        let mut p = OscParser::new();
        // OSC 133;A (no BEL) immediately followed by ESC ] starting OSC 133;B BEL.
        let mut stream = vec![ESC, b']'];
        stream.extend_from_slice(b"133;A");
        stream.extend_from_slice(&[ESC, b']']); // implicit terminator + new OSC start
        stream.extend_from_slice(b"133;B");
        stream.push(BEL);
        assert_eq!(
            p.feed(&stream),
            vec![OscEvent::PromptStart, OscEvent::PromptEnd]
        );
    }

    #[test]
    fn exit_code_edges_empty_nonnumeric_out_of_range() {
        let mut p = OscParser::new();
        assert_eq!(
            p.feed(&osc("133;D", &[BEL])),
            vec![OscEvent::CommandEnd(None)]
        );
        assert_eq!(
            p.feed(&osc("133;D;", &[BEL])),
            vec![OscEvent::CommandEnd(None)]
        );
        assert_eq!(
            p.feed(&osc("133;D;abc", &[BEL])),
            vec![OscEvent::CommandEnd(None)]
        );
        assert_eq!(
            p.feed(&osc("133;D;256", &[BEL])),
            vec![OscEvent::CommandEnd(None)]
        );
        assert_eq!(
            p.feed(&osc("133;D;255", &[BEL])),
            vec![OscEvent::CommandEnd(Some(255))]
        );
        // Trailing aid= arg ignored.
        assert_eq!(
            p.feed(&osc("133;D;7;aid=99", &[BEL])),
            vec![OscEvent::CommandEnd(Some(7))]
        );
    }

    #[test]
    fn oversized_osc_is_dropped_not_crashed() {
        let mut p = OscParser::new();
        let mut stream = vec![ESC, b']'];
        stream.extend_from_slice(b"133;");
        stream.extend(std::iter::repeat_n(b'x', 9000)); // exceeds 8 KiB cap
        stream.push(BEL);
        // Oversized OSC yields no event; parser recovers to Ground.
        assert_eq!(p.feed(&stream), Vec::<OscEvent>::new());
        // A subsequent valid OSC still parses.
        assert_eq!(p.feed(&osc("133;B", &[BEL])), vec![OscEvent::PromptEnd]);
    }

    #[test]
    fn osc7_file_scheme_decodes_and_strips_host() {
        let mut p = OscParser::new();
        let ev = p.feed(&osc("7;file://myhost/Users/me/projects", &[BEL]));
        assert_eq!(ev, vec![OscEvent::Cwd("/Users/me/projects".to_string())]);
    }

    #[test]
    fn osc7_kitty_scheme_decodes() {
        let mut p = OscParser::new();
        let ev = p.feed(&osc("7;kitty-shell-cwd://host/home/u/dir", &[BEL]));
        assert_eq!(ev, vec![OscEvent::Cwd("/home/u/dir".to_string())]);
    }

    #[test]
    fn osc7_percent_decodes_spaces_and_unicode() {
        let mut p = OscParser::new();
        // "/Users/me/My%20Docs" → "/Users/me/My Docs"
        let ev = p.feed(&osc("7;file://h/Users/me/My%20Docs", &[BEL]));
        assert_eq!(ev, vec![OscEvent::Cwd("/Users/me/My Docs".to_string())]);
    }

    #[test]
    fn osc7_empty_host_still_yields_absolute_path() {
        let mut p = OscParser::new();
        let ev = p.feed(&osc("7;file:///var/tmp", &[BEL]));
        assert_eq!(ev, vec![OscEvent::Cwd("/var/tmp".to_string())]);
    }

    #[test]
    fn osc7_unknown_scheme_dropped() {
        let mut p = OscParser::new();
        assert_eq!(
            p.feed(&osc("7;http://evil/", &[BEL])),
            Vec::<OscEvent>::new()
        );
    }

    #[test]
    fn osc7_bad_percent_escape_dropped() {
        let mut p = OscParser::new();
        assert_eq!(
            p.feed(&osc("7;file://h/a%ZZb", &[BEL])),
            Vec::<OscEvent>::new()
        );
    }

    #[test]
    fn lifecycle_full_transition_table() {
        let mut lc = SessionLifecycle::AtPrompt;
        assert_eq!(lc, SessionLifecycle::AtPrompt);
        advance_lifecycle(&mut lc, &OscEvent::PromptStart); // A: prompt drawing, no state change
        assert_eq!(lc, SessionLifecycle::AtPrompt);
        advance_lifecycle(&mut lc, &OscEvent::PromptEnd); // B → AtPrompt (idle)
        assert_eq!(lc, SessionLifecycle::AtPrompt);
        advance_lifecycle(&mut lc, &OscEvent::CommandStart); // C → Running
        assert_eq!(lc, SessionLifecycle::Running);
        advance_lifecycle(&mut lc, &OscEvent::CommandEnd(Some(0))); // D;0 → Exited(Some(0))
        assert_eq!(
            lc,
            SessionLifecycle::Exited {
                code: Some(0),
                signal: None
            }
        );
        advance_lifecycle(&mut lc, &OscEvent::PromptStart); // A after exit → back toward prompt
        advance_lifecycle(&mut lc, &OscEvent::PromptEnd);
        assert_eq!(lc, SessionLifecycle::AtPrompt);
    }

    #[test]
    fn lifecycle_empty_command_b_to_a_is_noop() {
        let mut lc = SessionLifecycle::AtPrompt;
        advance_lifecycle(&mut lc, &OscEvent::PromptEnd); // B → AtPrompt
        advance_lifecycle(&mut lc, &OscEvent::PromptStart); // A (user hit Enter on empty line): no phantom Running
        assert_eq!(lc, SessionLifecycle::AtPrompt);
        advance_lifecycle(&mut lc, &OscEvent::PromptEnd);
        assert_eq!(lc, SessionLifecycle::AtPrompt);
    }

    #[test]
    fn lifecycle_d_without_code_is_exited_none() {
        let mut lc = SessionLifecycle::AtPrompt;
        advance_lifecycle(&mut lc, &OscEvent::CommandStart);
        advance_lifecycle(&mut lc, &OscEvent::CommandEnd(None));
        assert_eq!(
            lc,
            SessionLifecycle::Exited {
                code: None,
                signal: None
            }
        );
    }

    #[test]
    fn lifecycle_cwd_event_does_not_change_state() {
        let mut lc = SessionLifecycle::AtPrompt;
        advance_lifecycle(&mut lc, &OscEvent::CommandStart);
        advance_lifecycle(&mut lc, &OscEvent::Cwd("/tmp".into()));
        assert_eq!(lc, SessionLifecycle::Running);
    }

    #[test]
    fn forged_and_interleaved_osc_never_panics_and_recovers() {
        let mut p = OscParser::new();
        // Garbage OSC introducer with junk, unterminated, then a real one via implicit ESC.
        let mut stream = vec![ESC, b']'];
        stream.extend_from_slice(b"999;garbage;;;");
        stream.extend_from_slice(&[ESC, b']']); // implicit terminate + new OSC
        stream.extend_from_slice(b"133;A");
        stream.push(BEL);
        // Unknown OSC 999 → no event; the real 133;A → PromptStart.
        assert_eq!(p.feed(&stream), vec![OscEvent::PromptStart]);

        // Interleave SGR + partial OSC + text; must not corrupt or panic.
        let ev = p.feed(b"\x1b[1mbold\x1b[0m \x1b]133;C\x07running");
        assert_eq!(ev, vec![OscEvent::CommandStart]);

        // A lone ESC at end of chunk buffers cleanly and continues next feed.
        assert_eq!(p.feed(&[ESC]), Vec::<OscEvent>::new());
        assert_eq!(p.feed(b"]"), Vec::<OscEvent>::new());
        assert_eq!(p.feed(b"133;B\x07"), vec![OscEvent::PromptEnd]);
    }
}
