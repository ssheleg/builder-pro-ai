# Changelog

All notable changes to Builder Pro AI. Format: keepachangelog.com; versioning: semver.

## [0.3.0] — 2026-07-09

### Added
- **Multi-root workspaces:** a workspace is now an ordered list of equal repo roots
  (`Workspace.roots: Vec<String>`; `root_path` stays a compat mirror, always `roots[0]`). Daemon
  schema v3 adds `workspace_root(workspace_id, ord, path)` behind a fail-closed forward-only
  migration; new wire requests `AddWorkspaceRoot`/`RemoveWorkspaceRoot` (validated, last-root
  removal rejected) broadcast `Push::WorkspaceUpdated` → `workspace://updated` to every attached
  client (Pv2 multi-subscriber).
- **File explorer + read-only preview:** `listDir`/`readFilePreview`/`createFile`/`createDir`/
  `renameEntry`/`moveEntry`/`deleteEntry`(→Trash)/`revealInFinder`/`openExternal`, all core-local
  (`src-tauri/src/fs_explorer.rs`), gitignore-aware (`ignore` crate, `.git` always hidden),
  1 MiB-capped preview with honest binary/too-large/truncated placeholders — never a silent
  truncated-as-whole read. Every op validated against the active workspace's roots first
  (`bpa_paths::validate_path_within`/`validate_parent_within`, new shared-crate functions).
- **Live file watch:** debounced FSEvents watch (`notify` + `notify-debouncer-full`, 250 ms) per
  active workspace root, gitignore-filtered, capped/deduped `fs://changed{root,changedRelPaths}`
  (`["*"]` sentinel on overflow) or honest `fs://watch-error{root,reason}` — GUI-lifetime only
  (starts on activation, stops on switch/unmount).
- **Attention-first Home:** on open, sessions waiting for input are pinned first (amber) with a
  one-click «Пройти →» that navigates, activates, and focuses that terminal; then running; then
  recently exited (✓/✗ by exit code) — across every workspace, computed from the existing store,
  never polled.
- **OSC-133 command strip:** per-session recent-command chips (✓/✗ by exit code, running-dot for
  an in-flight command) sourced from `GetCommandEvents` (newest-first) — the first real UI
  consumer of the `command_events` table persisted since Pv2.
- **Terminal file links:** a pure, store-free regex resolver (`src/terminal/link-provider.ts`)
  lexically detects path-like tokens in terminal output (absolute/dot-relative/extensioned-relative,
  optional `:line[:col]` suffix) and an xterm `ILinkProvider` + OSC-8 `linkHandler` open a match in
  the right-rail preview on click, authoritatively re-validated against the workspace's roots at
  click time — a miss is a quiet toast, never a silent no-op.
- Three-rail UI: `⌂ Home` navigation rail, center Home/Workspace view, collapsible right FILES
  rail (`FileTree` + `FilePreview`).
- New design-system atoms: `Toast` (queue-of-one, `role="alert"`), `File tree`, `Preview pane`,
  `Command strip` (`docs/design-system.md` §5).

### Changed
- **Three-rail layout** replaces the two-pane (sidebar + terminal) shell; left rail is pure
  navigation, file explorer lives in a new collapsible right rail, hidden on Home.
- **MSRV: `rust-version` 1.77.2 → 1.88.0.** The declared floor was already false before this
  cycle: it never matched the resolved `Cargo.lock` graph (`plist`/`time`/`darling`/`serde_with`,
  pulled in transitively via `tauri`, declare 1.88.0 — verified against every locked crate's own
  `rust-version` field on both macOS targets). This cycle's own `trash` 5.2.6 addition (file
  delete → Trash) declares a lower 1.85.0, so it wasn't the binding constraint; 1.88.0 is. The
  pinned toolchain (`rust-toolchain.toml`) is 1.92, so this was never a build-breaking gap in
  practice on this repo's own CI/dev machines — only a false floor claim for anyone building on an
  older, "supported" Rust. Fixed in `Cargo.toml` and the S0+S1 spec's locked-versions table.

### Fixed
- **BL-14:** `applyReplay` now calls `term.reset()` before every Replay (including re-attach) —
  a re-attach no longer duplicates scrollback into the xterm buffer.
- **BL-29:** app-wide explicit `:focus-visible` (2px accent ring, `src/index.css`) — every
  interactive element now shows a visible focus ring on keyboard navigation, matching what
  `docs/design-system.md` already promised.

## [0.2.0] — 2026-07-07

### Changed
- **Hop-B wire codec: bincode → CBOR (ciborium).** Tagged enums are plain serde derives; the v1
  dual-codec bridge (`*Shape` mirrors, `is_human_readable` split) is retired. One planned,
  non-silent wire break (see the upgrade flow below).
- Version negotiation: codec-agnostic preamble (`BPAA`, client `[min,max]` → daemon
  `Accepted{chosen}`/`Incompatible{range}`, 5s bound, 256B build-string cap) replaces the
  in-band `Hello`/`Welcome` frames.

### Added
- Multi-subscriber attach: N independent subscribers per session at the wire/daemon level
  (per-subscriber replay + backpressure; GUI stays a single subscriber for now).
- Real `DaemonShutdown{drain}`: flush scrollback + command events, ack, graceful exit —
  same path as SIGTERM; launchd does not auto-restart a clean exit.
- Upgrade consent flow: incompatible-daemon detection (typed, fatal, never auto-retried) →
  honest banner + consent dialog (N live sessions counted) → best-effort drain →
  `launchctl kickstart -k` → app relaunch; kickstart failure surfaces honestly.
- Schema v2: `command_events` table (best-effort from OSC-133 C/D marks, `origin` column),
  fail-closed forward-only migration from v1.
- Cold-rehydrate: at boot the daemon loads every persisted session as an inactive replay-only
  entry; attaching an inactive session replays its scrollback (no new wire request needed).
- E2E: harness speaks the v2 wire (preamble + hand-rolled standard CBOR); new phase 5 —
  drain → daemon exit → relaunch same state dir → rehydrated `isActive:false` + scrollback
  marker intact (closes BL-7).

### Fixed
- Daemon per-connection writer-task hang on a client peer that stops reading (bounded 200ms
  join + abort).
- E2E preamble reader `sock.unshift()` race (phase-0 hang).
- E2E harness wrote the real user DB (`HOME` now isolated per run).
- `CommandError` struct-variant fields now serialize camelCase to the webview
  (container-level `rename_all` does not cascade).

## [0.1.0] — 2026-07-04

### Added
- S0+S1 foundation + terminal core: launchd-managed `bpa-sessiond` daemon owning PTYs
  (survive-GUI-restart), OSC-133/7 shell integration, sanitized scrollback replay,
  SQLite persistence, React/xterm.js frontend with per-session attach state machine.
- Shared `bpa-protocol` + `bpa-paths` crates; ts-rs generated TS types (diff-gated).
- Gates: workspace tests, clippy -D warnings, rustfmt, vitest, tsc, ts-rs parity,
  daemon coverage ≥80 %, e2e survive-restart.
