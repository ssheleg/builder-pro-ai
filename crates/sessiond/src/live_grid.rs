//! Live terminal grid state (spec §11): a headless `alacritty_terminal::Term`
//! used ONLY for cursor column, alt-screen/raw-mode detection, and cols/rows —
//! the inputs to the waiting-for-input heuristic (spec §10.4) and status.
//! Never serialized; the replay source is the sanitized byte ring (scrollback.rs).
//!
//! Pinned `alacritty_terminal = "=0.25.0"` (spec §3/§15: grid model leaks into
//! behavior, so no `^`/caret range). Verified against the installed crate
//! source under `~/.cargo/registry/src/.../alacritty_terminal-0.25.0`:
//! - `alacritty_terminal::term::Term<T>` — headless grid emulator.
//!   `Term::new(config: Config, dimensions: &D, event_proxy: T) -> Term<T>`
//!   (config is taken **by value**, not `&Config`).
//! - `alacritty_terminal::term::Config` — `Default`-constructible.
//! - `alacritty_terminal::event::VoidListener` — alacritty's own no-op
//!   `EventListener` impl (`impl EventListener for VoidListener {}`, the
//!   trait's `send_event` has a default no-op body) — no need to hand-roll one.
//! - `alacritty_terminal::vte::ansi::Processor` — VT parser (`alacritty_terminal`
//!   re-exports the `vte` crate via `pub use vte;`). `Processor::new()` and
//!   `processor.advance(&mut term, byte)` drive the grid **one byte at a time**
//!   (`fn advance<H: Handler>(&mut self, handler: &mut H, byte: u8)` — takes a
//!   single `u8`, not a slice, at this point release).
//! - `alacritty_terminal::grid::Dimensions` — trait exposing `columns()`,
//!   `screen_lines()`, `total_lines()`; `TermSize` below implements it.
//! - Cursor: `term.grid().cursor.point.column` → `Column(usize)`, `.0` is the
//!   0-based column index.
//! - Alt-screen: `term.mode().contains(TermMode::ALT_SCREEN)`
//!   (`alacritty_terminal::term::TermMode`).
//! - Resize: `term.resize(new_dimensions)` where
//!   `fn resize<S: Dimensions>(&mut self, size: S)` (by value, not `&D`).

use alacritty_terminal::event::VoidListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::{Config, Term, TermMode};
use alacritty_terminal::vte::ansi::Processor;

/// Terminal dimensions implementing `alacritty_terminal::grid::Dimensions`.
///
/// alacritty enforces a minimum grid of `MIN_COLUMNS = 2` / `MIN_SCREEN_LINES =
/// 1` internally; `LiveGrid::new`/`resize` additionally clamp caller-supplied
/// zero dimensions to 1 so a degenerate `(0, 0)` request never reaches the
/// grid constructor with an out-of-range value for `columns`.
#[derive(Clone, Copy, Debug)]
struct TermSize {
    columns: usize,
    screen_lines: usize,
}

impl Dimensions for TermSize {
    fn total_lines(&self) -> usize {
        self.screen_lines
    }

    fn screen_lines(&self) -> usize {
        self.screen_lines
    }

    fn columns(&self) -> usize {
        self.columns
    }
}

/// Thin wrapper around a headless `alacritty_terminal::Term` exposing only
/// the status inputs the daemon needs (spec §11): cursor column, alt-screen
/// state, and size. Never serialized — see module docs.
pub struct LiveGrid {
    term: Term<VoidListener>,
    parser: Processor,
}

impl LiveGrid {
    /// Create a fresh grid of `cols` x `rows`. Zero dimensions are clamped to
    /// 1 so alacritty never sees a degenerate grid.
    pub fn new(cols: u16, rows: u16) -> LiveGrid {
        let size = TermSize {
            columns: cols.max(1) as usize,
            screen_lines: rows.max(1) as usize,
        };
        let term = Term::new(Config::default(), &size, VoidListener);
        LiveGrid {
            term,
            parser: Processor::new(),
        }
    }

    /// Feed raw PTY bytes through the VT parser into the grid.
    pub fn feed(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.parser.advance(&mut self.term, b);
        }
    }

    /// Current cursor column (0-based). Input to the §10.4 heuristic
    /// ("cursor not at column 0").
    pub fn cursor_col(&self) -> u16 {
        self.term.grid().cursor.point.column.0 as u16
    }

    /// Whether the alt-screen buffer is active (vim/less/top) — excluded from
    /// the waiting-for-input heuristic (spec §10.4).
    pub fn is_alt_screen(&self) -> bool {
        self.term.mode().contains(TermMode::ALT_SCREEN)
    }

    /// Resize the grid; clamps zero dimensions to 1.
    pub fn resize(&mut self, cols: u16, rows: u16) {
        let size = TermSize {
            columns: cols.max(1) as usize,
            screen_lines: rows.max(1) as usize,
        };
        self.term.resize(size);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cursor_col_after_plain_writes() {
        let mut g = LiveGrid::new(80, 24);
        assert_eq!(g.cursor_col(), 0);
        g.feed(b"abc");
        // After printing 3 columns the cursor sits at column 3.
        assert_eq!(g.cursor_col(), 3);
        g.feed(b"\r"); // carriage return -> column 0
        assert_eq!(g.cursor_col(), 0);
    }

    #[test]
    fn alt_screen_enter_and_leave() {
        let mut g = LiveGrid::new(80, 24);
        assert!(!g.is_alt_screen());
        // Enter alt-screen (DECSET ?1049h) — what vim/less/top do.
        g.feed(b"\x1b[?1049h");
        assert!(g.is_alt_screen());
        // Leave alt-screen (DECRST ?1049l).
        g.feed(b"\x1b[?1049l");
        assert!(!g.is_alt_screen());
    }

    #[test]
    fn resize_shrinks_and_grows_grid() {
        let mut g = LiveGrid::new(80, 24);
        // Move the cursor near the right edge, then shrink below it.
        g.feed(b"0123456789"); // cursor at column 10
        assert_eq!(g.cursor_col(), 10);

        g.resize(8, 24); // columns now 8 -> cursor must be clamped in-bounds
        assert!(
            g.cursor_col() < 8,
            "cursor col {} not clamped to < 8",
            g.cursor_col()
        );

        // Grow back; writing from a fresh line lands within the wider grid.
        g.resize(120, 40);
        g.feed(b"\r\nx");
        assert_eq!(g.cursor_col(), 1);
    }

    #[test]
    fn new_and_resize_clamp_degenerate_zero_dimensions() {
        // Zero cols/rows must not panic; they clamp to 1.
        let mut g = LiveGrid::new(0, 0);
        assert_eq!(g.cursor_col(), 0);
        assert!(!g.is_alt_screen());
        g.resize(0, 0);
        g.feed(b"x");
        // With a 1-column grid, printing wraps rather than growing unbounded.
        assert!(g.cursor_col() <= 1);
    }
}
