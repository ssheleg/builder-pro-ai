# Builder Pro AI

![ci](https://github.com/sshlg/builder-pro-ai/actions/workflows/ci.yml/badge.svg)

A lightweight macOS desktop workspace for **orchestrating AI coding agents** (claude-code,
hermes, opencode, kilo, …) that do their work through terminals — plus app-native meta-agents
(a CEO strategist, a TDD/DDD project manager, and engineering specialists) that decide *what*
to build, run the plan, drive the terminals, and escalate only what they can't resolve.

Built with **Tauri 2** (Rust core + React/TypeScript UI). Ships as a universal macOS binary.

## Status

**S0+S1+Pv2+S2+S3+S4 implemented.** The foundation slice, the terminal core (daemon-owned PTYs,
OSC-driven status, sanitized scrollback replay, SQLite persistence, launchd-supervised survival),
Protocol v2 (CBOR wire, version negotiation, multi-subscriber attach), S2 (multi-root
workspaces, a core-owned file explorer + read-only preview + live watch, an attention-first Home,
an OSC-133 command strip, terminal file links), S3 (a SECOND launchd daemon `bpa-orchd`
hosting the app-domain store — projects, goals, ideas, insights, tasks, rulesets — with full CRUD,
export/import, and an owner-facing UI), and S4 (a knowledge graph in that same `bpa-orchd` store —
typed nodes/edges, cross-project links, a workspace-wide agent retrieval API, and an editable
`@xyflow/react` graph canvas) are done, tested, and documented. See
[`docs/superpowers/specs/`](docs/superpowers/specs/) for the specs this implementation is derived
from and [`docs/traceability.md`](docs/traceability.md) for the contract → test matrix.

- **Platform overview & roadmap:** [`2026-07-01-builderpro-platform-overview.md`](docs/superpowers/specs/2026-07-01-builderpro-platform-overview.md)
- **S0+S1 spec:** [`2026-07-01-builderpro-s0s1-foundation-terminal-design.md`](docs/superpowers/specs/2026-07-01-builderpro-s0s1-foundation-terminal-design.md)
- **S2 spec (workspace multi-root + file explorer + attention-first Home):** [`2026-07-08-s2-workspace-explorer-home-design.md`](docs/superpowers/specs/2026-07-08-s2-workspace-explorer-home-design.md)
- **S3 spec (`bpa-orchd` + app-domain foundation):** [`2026-07-13-s3-orchd-domain-foundation-design.md`](docs/superpowers/specs/2026-07-13-s3-orchd-domain-foundation-design.md)
- **S4 spec (knowledge graph + workspace-wide retrieval API):** [`2026-07-14-s4-knowledge-graph-design.md`](docs/superpowers/specs/2026-07-14-s4-knowledge-graph-design.md)
- **Architecture summary:** [`docs/architecture.md`](docs/architecture.md)
- **Contract → test traceability:** [`docs/traceability.md`](docs/traceability.md)
- **Release build/sign/notarize runbook:** [`docs/build-macos.md`](docs/build-macos.md)
- **Daemon ops runbooks:** [`docs/runbook-daemon.md`](docs/runbook-daemon.md) (`bpa-sessiond`) ·
  [`docs/runbook-orchd.md`](docs/runbook-orchd.md) (`bpa-orchd`)

### Features shipped so far

- **Terminal engine (S1):** real PTYs, multi-terminal per workspace, OSC-133/OSC-7 shell
  integration, sanitized scrollback replay, launchd-supervised daemon survives GUI close/crash.
- **Protocol v2:** CBOR wire codec, `[min,max]` version negotiation, multi-subscriber attach,
  real drain-on-upgrade with owner consent, cold-rehydrate of past sessions as inactive.
- **Multi-root workspaces (S2):** a workspace is an ordered list of equal repo roots
  (`Workspace.roots: Vec<String>`; `root_path` stays a compat mirror of `roots[0]`); add/remove a
  root without recreating the workspace.
- **File explorer + read-only preview (S2):** gitignore-aware lazy tree (`ignore` crate, `.git`
  always hidden), a 1 MiB-capped read-only preview (binary/too-large/error render honest
  placeholders, never a silent truncation), create/rename/move/delete (always to Trash), reveal in
  Finder / open externally — all core-local (`src-tauri/src/fs_explorer.rs`), path-validated
  against the active workspace's roots before any disk access.
- **Live file watch (S2):** debounced FSEvents watch (`notify`/`notify-debouncer-full`) per active
  root, gitignore-filtered, point-refreshing only the affected expanded tree nodes.
- **Attention-first Home (S2):** on open, sessions waiting for input are pinned first (amber) with
  a one-click «Пройти →» that jumps to and focuses that terminal, then running, then recently
  exited (✓/✗ by exit code) — across every workspace, no polling.
- **OSC-133 command strip (S2):** per-session recent-command chips (✓/✗ by exit code) sourced from
  `command_events` — the first real UI consumer of that table (persisted since Pv2).
- **Terminal file links (S2):** click a path printed in terminal output to open it in the
  right-rail preview (regex detection + OSC-8 hyperlinks; validated against the workspace's roots
  on click, never a silent no-op on a miss).
- **`bpa-orchd`, the second daemon (S3):** a launchd-supervised app-domain daemon, independent of
  `bpa-sessiond`, with its own SQLite store (`orchd.db`), its own Hop-B socket + version space
  (`[1,1]`), and the same fail-closed migrations / drain-and-consent upgrade patterns sessiond
  proved out (`docs/runbook-orchd.md`).
- **Six domain entity families (S3):** Project (⇄ Workspace links), Goal (full tree — one
  strategic root per project + arbitrary-depth additional subgoals), Idea (lifecycle
  captured→…→shipped/archived, nullable project — quick-capture inbox), Insight (fit-verdict vs
  goals), Task/Subtask (unified model, kanban is a future view), RuleSet (global + per-project;
  markdown is the source of truth, DB stores `md_path`+`md_hash`) — full CRUD, invariants, and
  cascades, plus per-project/whole-store JSON export/import with field-verbatim round-trips.
- **Project management UI (S3):** left-rail project groups, a tabbed `ProjectPanel` (Обзор · Цели
  · Идеи · Задачи · Инсайты · Правила), ⌘K quick-capture for ideas, and a `HomeGoals` panel (below
  the S2 attention queue) showing each active project's strategic goal + children.
- **Knowledge graph (S4):** `orchd.db` schema v2 adds `graph_node`/`graph_edge` — typed nodes
  (concept/fact/artifact/decision/note + `entityRef` soft-refs onto a goal/idea/insight/task, no
  FK — a deleted domain entity leaves its node behind flagged `isOrphan`, rendered «источник
  удалён») and typed edges that may link nodes across DIFFERENT projects (a cross-project edge
  survives BOTH projects' daemon restarts). A strategic-goal `entityRef` node is auto-seeded for
  every project.
- **Workspace-wide graph retrieval API (S4):** the S6-agent contract — `list_project_graph` (a
  project's own nodes + incident edges + cross-project "ghost" endpoints, read-time entityRef
  label resolution), `neighborhood` (bidirectional recursive-CTE traversal, cross-project, depth
  capped at 6), and `search_nodes` (workspace-wide or per-project, capped at 200 rows) — NOT
  project-scoped, so an agent working project A can query project B's knowledge. A depth-3
  neighborhood rooted at a project's strategic goal on a synthetic 500-node/1000-edge graph
  measures ~51 ms (DoD: <100 ms).
- **Graph canvas (S4):** a 7th `ProjectPanel` tab, «Граф», an editable `@xyflow/react` canvas —
  drag debounced-persists a node's position, connecting two nodes adds an edge, a toolbar
  adds/deletes nodes and searches (a match gets an accent ring), every mutating control disabled
  while `orchd://down`. A cross-project ghost node click navigates to its own project; a local
  `entityRef` node click is currently an honest no-op (no deep-link seam yet into a specific
  goal/idea/insight/task row in another tab).

## Principles

- **Production-grade, no MVP half-states.** Each slice is finished: tests (TDD), error handling
  and honest degradation, structured logging, and docs are part of Definition of Done.
- **Max autonomy, min human-in-the-loop.** Humans set goals and quality; agents decide the rest.
- **Honest about boundaries.** The app never lies about session/agent state.

## Architecture

**Three OS processes, two independent Hop-B connections** (as of S4, `[0.5.0]`): the GUI app and
TWO launchd-supervised daemons — `bpa-sessiond` (terminal domain) and `bpa-orchd` (app domain:
projects/goals/ideas/insights/tasks/rulesets, plus a knowledge graph as of S4). The diagram below
shows the terminal-domain half only (`bpa-sessiond`'s daemon owns every PTY so the GUI can close,
crash, or restart without
killing a running shell — tmux/re-attach model; File I/O + live watch (S2) live in the Tauri core
instead — GUI-lifetime, never over Hop-B, so the daemon's charter stays terminal-domain only) —
full detail on BOTH daemons (incl. the three-rail UI and the two-daemon topology) in
[`docs/architecture.md`](docs/architecture.md).

```
┌──────────────────────── Builder Pro AI.app ────────────────────────┐
│  React webview (UI)                Rust core (broker)               │
│  • xterm panes (⌂ Home |           • #[tauri::command] surface      │
│    workspace | FILES rail)         • fs_explorer.rs (listDir/       │
│  • workspace sidebar      ◄──Hop A──►  preview/create/rename/       │
│  • status dots             Tauri IPC   move/delete→Trash/reveal)    │
│  • Zustand (metadata only)         • fs_watcher.rs (FSEvents watch) │
│                                     • UDS client to daemon          │
│                                     • app settings (tauri-plugin-store)│
└───────────────────────────────────│────────────────────────────────┘
                                     │ Hop B: Unix domain socket
                                     │ (codec-agnostic preamble handshake, then u32-LE length
                                     │  prefix + CBOR Frame)
                          ┌──────────▼────────────┐
                          │  bpa-sessiond (daemon) │ ◄─ launchd LaunchAgent
                          │  • PTY supervisor      │    (KeepAlive{Crashed:true})
                          │  • OSC-133 parser + SM │
                          │  • sanitized byte ring │   owns ALL PTYs +
                          │  • alacritty live grid │   ALL durable terminal-
                          │  • rusqlite (WAL,       │   domain state (incl.
                          │    workspace roots,     │   multi-root workspaces,
                          │    command_events)      │   command_events) — NEVER
                          └──────────┬─────────────┘   file content
                     PTYs via portable-pty (child setsid'd, own pgrp)
                          ┌──────────▼─────────────┐
                          │ zsh / bash / agent CLI │
                          └────────────────────────┘
```

`launchd` — not Tauri — owns each daemon's lifecycle: the app bundles `bpa-sessiond` AND
`bpa-orchd` (S3) as signed `externalBin` sidecars, and a per-user `LaunchAgent`
(`KeepAlive.Crashed = true`) supervises each process. The GUI only ever holds a socket connection
to each, never a process handle.

## Survival truth table (spec §13)

| Event | Sessions (`bpa-sessiond`) |
|---|---|
| GUI close / crash / restart | Live shells **keep running** (daemon-owned) — reattach + scrollback replay |
| Daemon restart / upgrade / crash | Live shells **end**; session records + scrollback survive (up to the last ~1 s flush) and rehydrate as **inactive** sessions |
| **macOS logout** | Sessions **die** — the per-user LaunchAgent is torn down with the login session |
| **`bpa-orchd` restart / upgrade (S3, `[0.4.0]`; graph added S4, `[0.5.0]`)** | Domain data (projects/goals/ideas/insights/tasks/rules) **fully survives** — it's all SQLite (`orchd.db`); there is no live runtime state to lose in S3/S4 (no scheduler/workflow/agent runtime yet — those are roadmap, SW1/SW2/S6b+), so a restart is a non-event beyond a brief `orchd://down` banner while it reconnects. The S4 knowledge graph is the same durable-store guarantee: a graph edge that links nodes in TWO DIFFERENT projects survives a restart intact on BOTH sides (proven by `npm run e2e:orchd` phase 5 below) |

This is an honest boundary, not a bug: any daemon stop (restart, upgrade, or crash) takes its live
child processes down with it, and logging out tears down every per-user LaunchAgent along with
everything it supervises. What *does* survive — GUI restart with live shells, daemon restart for
records + scrollback (rehydrated as inactive), and `bpa-orchd` restart for every domain row
(incl. the graph) — is stated in the table above.
`npm run e2e:survive` proves the sessiond half end-to-end: phases 0-4 the client-restart half,
phase 5 the daemon-restart half (SIGTERM-equivalent drain → relaunch → rehydrated inactive +
scrollback intact — Pv2 §9.8, closes BL-7 in [`docs/backlog.md`](docs/backlog.md)).
`npm run e2e:orchd` proves the orchd half: create data → drain-restart → data intact →
export → wipe the DB → re-import → re-export equals the original (S3 spec §12); phase 5 (S4)
creates a cross-project graph edge, restarts the daemon, and asserts it survives on both projects'
sides — the S4 spec §8 DoD proof.

## Quickstart

```sh
# Prerequisites
npm install
export PATH="$HOME/.cargo/bin:$PATH"
rustup target add aarch64-apple-darwin x86_64-apple-darwin

# Build BOTH daemons first — dev mode (`npm run tauri dev`) resolves each daemon binary as a
# sibling of the running core binary (target/debug/), so building each with `cargo build -p`
# is enough for dev; it fails with an actionable error if either is missing:
cargo build -p bpa-sessiond -p bpa-orchd

# BOTH daemons are declared in tauri.conf.json's `externalBin`, so Tauri's build.rs requires
# BOTH staged under src-tauri/binaries/ (target-triple suffix) even for `tauri dev` in a fresh
# checkout — stage each one:
mkdir -p src-tauri/binaries
TRIPLE="$(rustc -vV | sed -n 's/host: //p')"
cp target/debug/bpa-sessiond "src-tauri/binaries/bpa-sessiond-$TRIPLE"
cp target/debug/bpa-orchd "src-tauri/binaries/bpa-orchd-$TRIPLE"

# Run the app in dev mode
npm run tauri dev

# Run the full test + traceability + coverage + e2e gate (spec §14.3 Definition of Done)
bash scripts/final-suite.sh

# Build a signed, notarized, universal release .app (see docs/build-macos.md for credentials).
# BOTH daemons ship in the bundle: `scripts/build-universal.sh` builds bpa-sessiond AND bpa-orchd
# for arm64 + x86_64, lipo-merges each, and Tauri embeds both signed sidecars (BL-59, closed).
bash scripts/build-universal.sh
```

## Running the tests

| Suite | Command | What it covers |
|---|---|---|
| Rust workspace | `cargo test --workspace` | two daemons (`bpa-sessiond`, `bpa-orchd`), shared daemon infra (`bpa-daemon-core`), shared protocols (`bpa-protocol`, `bpa-orchd-proto`), path validation (`bpa-paths`), Tauri core (`builder-pro-ai`) — **726 tests** as of the last full run (S4, `[0.5.0]`) |
| TypeScript | `npx vitest run` (or `npm test`) | Zustand store (incl. `domainSlice`/`graphByProject`), terminal-manager (attach state machine), IPC wrappers (incl. `orchd.ts`), components (incl. `ProjectPanel`/`GoalTree`/`IdeasList`/`TasksList`/`InsightsList`/`RulesetPanel`/`QuickCapture`/`HomeGoals`/`GraphCanvas`/`graphMapping`) — **559 tests, 35 files** (S4, `[0.5.0]`) |
| End-to-end (sessiond) | `npm run e2e:survive` | create terminal → run a command → observe OSC-driven status → quit the CLIENT → daemon+shell survive → reattach + scrollback intact (phases 0-4, the core S1 promise, spec §14.1); phase 5 restarts the DAEMON itself and asserts rehydrated inactive sessions + scrollback (Pv2 §9.8, closes BL-7) |
| End-to-end (orchd) | `npm run e2e:orchd` | boot on a temp HOME → handshake `[1,1]` → create a project (+2 goals, an idea, a task) → `OrchdShutdown{drain:true}` → relaunch → data intact → `ExportAll` → shutdown → delete `orchd.db*` → relaunch (fresh v1) → `ImportBundle` → re-export equals the original modulo `exportedAt` (S3 spec §12 — the roadmap DoD proof); phase 5 creates two projects + a cross-project graph edge, restarts the daemon, and asserts the edge survives on both projects' sides (S4 spec §8 DoD proof) |
| Coverage gate | `bash scripts/coverage-gate.sh` | `cargo llvm-cov --package bpa-sessiond --fail-under-lines 80` AND `cargo llvm-cov --package bpa-orchd --fail-under-lines 80` — real, enforcing ≥80% line-coverage gates on BOTH daemon crates (requires `cargo install cargo-llvm-cov`) |
| Everything, in order | `bash scripts/final-suite.sh` | 9 stages: Rust suite → clippy `-D warnings` → `cargo fmt --check` → TS suite → `tsc --noEmit` → ts-rs type-parity diff (`bpa-protocol` + `bpa-orchd-proto`) → coverage gate (both daemons) → e2e:survive → e2e:orchd; exits 0 with `ALL GATES PASSED` only if every stage passes. CI runs the same set (see [`CONTRIBUTING.md`](CONTRIBUTING.md)); daemon ops live in [`docs/runbook-daemon.md`](docs/runbook-daemon.md) / [`docs/runbook-orchd.md`](docs/runbook-orchd.md) |

See [`docs/traceability.md`](docs/traceability.md) for the full contract → test matrix (every
locked spec §14.2 contract mapped to the concrete test(s) proving it), and
[`tests/e2e/README.md`](tests/e2e/README.md) for the three fidelity levels of the survive-restart
proof (socket harness, launchd-managed variant, full-GUI manual/CI confirmation).

## Building a release

See [`docs/build-macos.md`](docs/build-macos.md) for the full runbook: prerequisites, Apple
Developer credential setup, the `build-universal.sh` → `sign-verify.sh` → `smoke-clean-vm.sh`
pipeline, and its honest-degradation behavior when signing/notarization credentials aren't present
(builds a dev-signed artifact and says so loudly, rather than silently claiming a notarized result).
