# Changelog

All notable changes to Builder Pro AI. Format: keepachangelog.com; versioning: semver.

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
