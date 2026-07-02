# Builder Pro AI

A lightweight macOS desktop workspace for **orchestrating AI coding agents** (claude-code,
hermes, opencode, kilo, …) that do their work through terminals — plus app-native meta-agents
(a CEO strategist, a TDD/DDD project manager, and engineering specialists) that decide *what*
to build, run the plan, drive the terminals, and escalate only what they can't resolve.

Built with **Tauri 2** (Rust core + React/TypeScript UI). Ships as a universal macOS binary.

## Status

**S0+S1 implemented.** The foundation slice (app scaffold, shared wire protocol) and the terminal
core slice (daemon-owned PTYs, OSC-driven status, sanitized scrollback replay, SQLite persistence,
launchd-supervised survival) are done, tested, and documented. See
[`docs/superpowers/specs/`](docs/superpowers/specs/) for the specs this implementation is derived
from and [`docs/traceability.md`](docs/traceability.md) for the contract → test matrix.

- **Platform overview & roadmap:** [`2026-07-01-builderpro-platform-overview.md`](docs/superpowers/specs/2026-07-01-builderpro-platform-overview.md)
- **S0+S1 spec (this slice):** [`2026-07-01-builderpro-s0s1-foundation-terminal-design.md`](docs/superpowers/specs/2026-07-01-builderpro-s0s1-foundation-terminal-design.md)
- **Architecture summary:** [`docs/architecture.md`](docs/architecture.md)
- **Contract → test traceability:** [`docs/traceability.md`](docs/traceability.md)
- **Release build/sign/notarize runbook:** [`docs/build-macos.md`](docs/build-macos.md)

## Principles

- **Production-grade, no MVP half-states.** Each slice is finished: tests (TDD), error handling
  and honest degradation, structured logging, and docs are part of Definition of Done.
- **Max autonomy, min human-in-the-loop.** Humans set goals and quality; agents decide the rest.
- **Honest about boundaries.** The app never lies about session/agent state.

## Architecture (S0+S1)

Two OS processes, two IPC hops. The daemon owns every PTY so the GUI can close, crash, or restart
without killing a running shell (tmux/re-attach model) — full detail in
[`docs/architecture.md`](docs/architecture.md).

```
┌──────────────────────── Builder Pro AI.app ────────────────────────┐
│  React webview (UI)                Rust core (broker)               │
│  • xterm panes                     • #[tauri::command] surface      │
│  • workspace sidebar      ◄──Hop A──►  • UDS client to daemon       │
│  • status dots             Tauri IPC   • maps daemon frames ⇄ UI    │
│  • Zustand (metadata only)         • app settings (tauri-plugin-store)│
└───────────────────────────────────│────────────────────────────────┘
                                     │ Hop B: Unix domain socket
                                     │ (u32-LE length prefix + bincode Frame)
                          ┌──────────▼────────────┐
                          │  bpa-sessiond (daemon) │ ◄─ launchd LaunchAgent
                          │  • PTY supervisor      │    (KeepAlive{Crashed:true})
                          │  • OSC-133 parser + SM │
                          │  • sanitized byte ring │   owns ALL PTYs +
                          │  • alacritty live grid │   ALL durable state
                          │  • rusqlite (WAL)      │
                          └──────────┬─────────────┘
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
| GUI close / crash / restart | **Survive** — reattach + scrollback replay |
| Daemon restart | **Survive** — SQLite rehydrate; scrollback intact up to the last flush (≈500 ms) |
| **Daemon crash** | Live shells **die** (they're children of the daemon's process group); scrollback replays up to the last flush |
| **macOS logout** | Sessions **die** — the per-user LaunchAgent is torn down with the login session |

This is an honest boundary, not a bug: the daemon crashing (as opposed to being cleanly restarted
by `launchd`) takes its live child processes down with it, and logging out tears down every
per-user LaunchAgent along with everything it supervises. What *does* survive — GUI restarts and
daemon restarts — is proven end-to-end by `npm run e2e:survive` (see below).

## Quickstart

```sh
# Prerequisites
npm install
export PATH="$HOME/.cargo/bin:$PATH"
rustup target add aarch64-apple-darwin x86_64-apple-darwin

# Run the app in dev mode (spawns the daemon's dev build automatically)
npm run tauri dev

# Run the full test + traceability + coverage + e2e gate (spec §14.3 Definition of Done)
bash scripts/final-suite.sh

# Build a signed, notarized, universal release .app (see docs/build-macos.md for credentials)
bash scripts/build-universal.sh
```

## Running the tests

| Suite | Command | What it covers |
|---|---|---|
| Rust workspace | `cargo test --workspace` | daemon (`bpa-sessiond`), shared protocol (`bpa-protocol`), Tauri core (`builder-pro-ai`) — 194 tests as of the last full run |
| TypeScript | `npx vitest run` (or `npm test`) | Zustand store, terminal-manager, IPC wrappers, components — 92 tests |
| End-to-end | `npm run e2e:survive` | create terminal → run a command → observe OSC-driven status → quit → daemon+shell survive → relaunch → reattach + scrollback intact (the core S1 promise, spec §14.1) |
| Coverage gate | `bash scripts/coverage-gate.sh` | `cargo llvm-cov --package bpa-sessiond --fail-under-lines 80` — a real, enforcing ≥80% line-coverage gate on the daemon crate (requires `cargo install cargo-llvm-cov`) |
| Everything, in order | `bash scripts/final-suite.sh` | Rust suite → TS suite → ts-rs type-parity diff → coverage gate → e2e; exits 0 with `ALL GATES PASSED` only if every stage passes |

See [`docs/traceability.md`](docs/traceability.md) for the full contract → test matrix (every
locked spec §14.2 contract mapped to the concrete test(s) proving it), and
[`tests/e2e/README.md`](tests/e2e/README.md) for the three fidelity levels of the survive-restart
proof (socket harness, launchd-managed variant, full-GUI manual/CI confirmation).

## Building a release

See [`docs/build-macos.md`](docs/build-macos.md) for the full runbook: prerequisites, Apple
Developer credential setup, the `build-universal.sh` → `sign-verify.sh` → `smoke-clean-vm.sh`
pipeline, and its honest-degradation behavior when signing/notarization credentials aren't present
(builds a dev-signed artifact and says so loudly, rather than silently claiming a notarized result).
