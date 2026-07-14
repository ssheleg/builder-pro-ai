# Changelog

All notable changes to Builder Pro AI. Format: keepachangelog.com; versioning: semver.

## [0.4.0] — 2026-07-14

### Added
- **`bpa-orchd`, the second launchd daemon:** a per-user LaunchAgent
  (`ai.builderpro.desktop.orchd`) hosting the app-domain store — projects, goals, ideas, insights,
  tasks, rulesets. Reuses `bpa-sessiond`'s patterns verbatim: fail-closed forward-only migrations,
  flock single-instance, peer-cred (`getpeereid`) refusal, drain/consent upgrade choreography. Its
  own Hop-B socket (`orchd.sock`/`orchd.lock`), own SQLite DB (`orchd.db`), own logs
  (`orchd.tracing.log`/`orchd.out.log`/`orchd.err.log`), own independent wire version space
  `[1,1]` (same `BPAA` preamble magic as sessiond — daemons distinguished by socket path, not by
  preamble content). Ops runbook: `docs/runbook-orchd.md`.
- **`bpa-daemon-core` extraction:** six shared modules (`dirs`, `singleton`, `logging`, `migrate`,
  `handshake`, `broadcast`) factored out of `bpa-sessiond` FIRST, then `bpa-sessiond` re-seated on
  them with behavior byte-identical (on-disk socket/lock/plist paths asserted unchanged by test)
  before `bpa-orchd` was built on the same foundation — final architecture immediately, no
  "duplicate now, refactor later".
- **Domain schema v1 + full CRUD for six entity families:** Project (workspace links, archive),
  Goal (full tree — exactly one `strategic` root per project, `additional` subgoals at arbitrary
  depth via `parent_id`, move/reorder, delete-subtree cascade), Idea (lifecycle
  captured→researching→specced→in-dev→shipped→archived, nullable `project_id` for orphan/inbox
  ideas, `SetIdeaProject` to attach/detach), Insight (fit-verdict fit/no-fit/unknown vs
  goals/metrics, owner override via `SetInsightFitVerdict`, archive requires non-empty
  `resolutionReasoning`), Task/Subtask (unified model — kanban is a future VIEW over it — status
  groups backlog/todo/waiting/progress/testing/done, `rank` reordering via midpoint math), RuleSet
  (global + per-project). Every create/update/delete replies the updated entity (or `Ack`) AND
  broadcasts a coarse `orchd://*-changed` push ONLY on success — failed requests broadcast
  nothing.
- **RuleSet markdown files — the source of truth (D4):** DB stores `md_path` + `md_hash`
  (sha256); files are atomic-written (tmp+rename); external edits/deletions surface honestly
  (`Ok` / `ExternallyModified` / `Missing`) instead of silently overwriting or hiding drift. A
  deliberate NARROW exception to "orchd gets its own file API in S9" (architecture.md amended) —
  this is the ONLY file I/O anywhere in the `bpa-orchd` crate, not a general file API.
- **Export / import:** per-project and whole-store JSON bundles (`bundleFormat: 1`), every row
  field preserved verbatim on import (ids, `created_at`/`updated_at`, `rank`, `md_hash` — never
  re-stamped), id collisions rejected as a typed `Conflict` with the whole transaction rolled
  back, round-trip proven (import into an empty store → re-export equals the original modulo
  `exportedAt`). A 16 MiB frame-cap guard answers a typed `Io` error instead of attempting a
  doomed oversized send (chunked export tracked as a backlog row).
- **Frontend — project management UI:** left-rail project groups (project header + nested
  workspace rows, «Без проекта» group, create-project dialog); a tabbed `ProjectPanel` (Обзор ·
  Цели `GoalTree` · Идеи `IdeasList` · Задачи `TasksList` · Инсайты `InsightsList` · Правила
  `RulesetPanel`); ⌘K quick-capture (`QuickCapture`) — global overlay, title/body/project select,
  `CreateIdea` on Enter, disabled with an honest inline note while orchd is down; `HomeGoals`
  mounted below the S2 attention sections (the amber «Нужен ты» block keeps its pinned-top spot)
  showing each active project's strategic goal + direct children with status chips.
- **Honest degradation for the second daemon:** `orchd://down` → shared banner + [Повторить]
  (`orchd_reconnect`) on every domain surface, mutating controls disabled; `orchd://incompatible`
  → the existing `UpgradeDialog` generalized to read both daemons' flag pairs, rendering one
  dialog at a time (sessiond first if both are incompatible — no combined choreography); orchd's
  own upgrade copy is honest that no live session is at risk (no PTYs to lose).
- New design-system atoms: Tree row, Lifecycle chip, Policy form, File-state banner, Project group
  row, Quick-capture overlay (`docs/design-system.md` §5).
- **E2E (`npm run e2e:orchd`, `tests/e2e/orchd-survive.mjs`):** boot on a temp HOME → handshake
  `[1,1]` → create a project (+2 goals, an idea, a task) → `OrchdShutdown{drain:true}` → relaunch
  → data intact → `ExportAll` → shutdown → delete `orchd.db*` → relaunch (fresh v1) →
  `ImportBundle` → re-export equals the original modulo `exportedAt` — the roadmap DoD proof
  (goals+ideas+tasks CRUD survive restart; export/import round-trips).

### Changed
- **Gate: 8 stages → 9.** `scripts/final-suite.sh` adds `bpa-orchd` to the ts-rs type-parity diff
  (`src/ipc/orchd-types.ts`) and the coverage gate (`cargo llvm-cov --package bpa-orchd
  --fail-under-lines 80`, alongside `bpa-sessiond`'s existing gate), and a new stage 9
  `npm run e2e:orchd`. `.github/workflows/ci.yml` updated in lockstep.
- `src-tauri/src/launchd.rs`'s `LaunchdAgent` parameterized ADDITIVELY (`label`,
  `stdout_log_name`, `stderr_log_name` fields) so the same install/bootstrap/kickstart machinery
  renders either daemon's plist; sessiond call sites pass the pre-existing values byte-identically
  (asserted by test), orchd call sites pass its own identity.

### Fixed
- `crates/orchd/src/socket_server.rs`'s module doc overclaimed it was the only place in the crate
  calling `SystemTime::now()` — `persistence.rs` also does, for row `created_at`/`updated_at`.
  Reworded to scope the claim to the `exported_at` stamp specifically (T10 Minor).

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
- **Protocol v2 → v3** (one planned wire break: S2's multi-root `Workspace` + new verbs are not
  v2-decodable). An old v2 daemon negotiates `Incompatible` → the upgrade-consent dialog +
  `kickstart -k` restart the bundled v3 daemon; existing 0.2.0 installs upgrade through the dialog
  and live sessions rehydrate inactive (D4).

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
