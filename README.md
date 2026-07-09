# Builder Pro AI

![ci](https://github.com/sshlg/builder-pro-ai/actions/workflows/ci.yml/badge.svg)

A lightweight macOS desktop workspace for **orchestrating AI coding agents** (claude-code,
hermes, opencode, kilo, …) that do their work through terminals — plus app-native meta-agents
(a CEO strategist, a TDD/DDD project manager, and engineering specialists) that decide *what*
to build, run the plan, drive the terminals, and escalate only what they can't resolve.

Built with **Tauri 2** (Rust core + React/TypeScript UI). Ships as a universal macOS binary.

## Status

**S0+S1+Pv2+S2 implemented.** The foundation slice, the terminal core (daemon-owned PTYs,
OSC-driven status, sanitized scrollback replay, SQLite persistence, launchd-supervised survival),
Protocol v2 (CBOR wire, version negotiation, multi-subscriber attach), and S2 (multi-root
workspaces, a core-owned file explorer + read-only preview + live watch, an attention-first Home,
an OSC-133 command strip, terminal file links) are done, tested, and documented. See
[`docs/superpowers/specs/`](docs/superpowers/specs/) for the specs this implementation is derived
from and [`docs/traceability.md`](docs/traceability.md) for the contract → test matrix.

- **Platform overview & roadmap:** [`2026-07-01-builderpro-platform-overview.md`](docs/superpowers/specs/2026-07-01-builderpro-platform-overview.md)
- **S0+S1 spec:** [`2026-07-01-builderpro-s0s1-foundation-terminal-design.md`](docs/superpowers/specs/2026-07-01-builderpro-s0s1-foundation-terminal-design.md)
- **S2 spec (workspace multi-root + file explorer + attention-first Home):** [`2026-07-08-s2-workspace-explorer-home-design.md`](docs/superpowers/specs/2026-07-08-s2-workspace-explorer-home-design.md)
- **Architecture summary:** [`docs/architecture.md`](docs/architecture.md)
- **Contract → test traceability:** [`docs/traceability.md`](docs/traceability.md)
- **Release build/sign/notarize runbook:** [`docs/build-macos.md`](docs/build-macos.md)

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

## Principles

- **Production-grade, no MVP half-states.** Each slice is finished: tests (TDD), error handling
  and honest degradation, structured logging, and docs are part of Definition of Done.
- **Max autonomy, min human-in-the-loop.** Humans set goals and quality; agents decide the rest.
- **Honest about boundaries.** The app never lies about session/agent state.

## Architecture

Two OS processes, two IPC hops. The daemon owns every PTY so the GUI can close, crash, or restart
without killing a running shell (tmux/re-attach model). File I/O + live watch (S2) live in the
Tauri core instead — GUI-lifetime, never over Hop-B, so the daemon's charter stays terminal-domain
only — full detail (incl. the three-rail UI) in [`docs/architecture.md`](docs/architecture.md).

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

`launchd` — not Tauri — owns the daemon's lifecycle: the app bundles `bpa-sessiond` as a signed
`externalBin` sidecar, and a per-user `LaunchAgent` (`KeepAlive.Crashed = true`) supervises the
actual process. The GUI only ever holds a socket connection to it, never a process handle.

## Survival truth table (spec §13)

| Event | Sessions |
|---|---|
| GUI close / crash / restart | Live shells **keep running** (daemon-owned) — reattach + scrollback replay |
| Daemon restart / upgrade / crash | Live shells **end**; session records + scrollback survive (up to the last ~1 s flush) and rehydrate as **inactive** sessions |
| **macOS logout** | Sessions **die** — the per-user LaunchAgent is torn down with the login session |

This is an honest boundary, not a bug: any daemon stop (restart, upgrade, or crash) takes its live
child processes down with it, and logging out tears down every per-user LaunchAgent along with
everything it supervises. What *does* survive — GUI restart with live shells, and daemon restart
for records + scrollback (rehydrated as inactive) — is stated in the table above.
`npm run e2e:survive` proves both halves end-to-end: phases 0-4 the client-restart half, phase 5
the daemon-restart half (SIGTERM-equivalent drain → relaunch → rehydrated inactive + scrollback
intact — Pv2 §9.8, closes BL-7 in [`docs/backlog.md`](docs/backlog.md)).

## Quickstart

```sh
# Prerequisites
npm install
export PATH="$HOME/.cargo/bin:$PATH"
rustup target add aarch64-apple-darwin x86_64-apple-darwin

# Build the daemon first — dev mode looks for bpa-sessiond BESIDE the app binary
# (target/debug/) and fails with an actionable error if it's missing. The Tauri
# build script ALSO requires the sidecar staged under src-tauri/binaries/ (with
# the target-triple suffix) in any fresh checkout:
cargo build -p bpa-sessiond
mkdir -p src-tauri/binaries
cp target/debug/bpa-sessiond "src-tauri/binaries/bpa-sessiond-$(rustc -vV | sed -n 's/host: //p')"

# Run the app in dev mode
npm run tauri dev

# Run the full test + traceability + coverage + e2e gate (spec §14.3 Definition of Done)
bash scripts/final-suite.sh

# Build a signed, notarized, universal release .app (see docs/build-macos.md for credentials)
bash scripts/build-universal.sh
```

## Running the tests

| Suite | Command | What it covers |
|---|---|---|
| Rust workspace | `cargo test --workspace` | daemon (`bpa-sessiond`), shared protocol (`bpa-protocol`), path validation (`bpa-paths`), Tauri core (`builder-pro-ai`) — 384 tests as of the last full run (S2, `[0.3.0]`) |
| TypeScript | `npx vitest run` (or `npm test`) | Zustand store, terminal-manager (attach state machine), IPC wrappers, components — 297 tests, 22 files (S2, `[0.3.0]`) |
| End-to-end | `npm run e2e:survive` | create terminal → run a command → observe OSC-driven status → quit the CLIENT → daemon+shell survive → reattach + scrollback intact (phases 0-4, the core S1 promise, spec §14.1); phase 5 restarts the DAEMON itself and asserts rehydrated inactive sessions + scrollback (Pv2 §9.8, closes BL-7) |
| Coverage gate | `bash scripts/coverage-gate.sh` | `cargo llvm-cov --package bpa-sessiond --fail-under-lines 80` — a real, enforcing ≥80% line-coverage gate on the daemon crate (requires `cargo install cargo-llvm-cov`) |
| Everything, in order | `bash scripts/final-suite.sh` | 8 stages: Rust suite → clippy `-D warnings` → `cargo fmt --check` → TS suite → `tsc --noEmit` → ts-rs type-parity diff → coverage gate → e2e; exits 0 with `ALL GATES PASSED` only if every stage passes. CI runs the same set (see [`CONTRIBUTING.md`](CONTRIBUTING.md)); daemon ops live in [`docs/runbook-daemon.md`](docs/runbook-daemon.md) |

See [`docs/traceability.md`](docs/traceability.md) for the full contract → test matrix (every
locked spec §14.2 contract mapped to the concrete test(s) proving it), and
[`tests/e2e/README.md`](tests/e2e/README.md) for the three fidelity levels of the survive-restart
proof (socket harness, launchd-managed variant, full-GUI manual/CI confirmation).

## Building a release

See [`docs/build-macos.md`](docs/build-macos.md) for the full runbook: prerequisites, Apple
Developer credential setup, the `build-universal.sh` → `sign-verify.sh` → `smoke-clean-vm.sh`
pipeline, and its honest-degradation behavior when signing/notarization credentials aren't present
(builds a dev-signed artifact and says so loudly, rather than silently claiming a notarized result).
