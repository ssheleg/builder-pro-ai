# Architecture (S0+S1)

Authoritative source: `docs/superpowers/specs/2026-07-01-builderpro-s0s1-foundation-terminal-design.md`
(§2 Architecture, §4 module layout, §7 Hop-B protocol, §8 socket/launchd, §16 trust model). This
document is a navigable summary; if it ever drifts from the spec, the spec wins.

## Two processes, two IPC hops

Builder Pro AI is split into two OS processes so that closing (or crashing) the GUI never kills a
running shell — the daemon owns every PTY and survives independently, supervised by `launchd`, not
by the app.

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

### Hop A — webview ⇄ core (Tauri IPC)

The React webview never touches the raw socket. It calls typed `#[tauri::command]`s
(`src-tauri/src/commands.rs`) for request/response operations (`create_workspace`,
`create_session`, `write_stdin`, `resize`, `kill_session`, `get_session_state`, …) and receives two
kinds of push data from the core:

- A **`Channel<TerminalEvent>`** per attached terminal (`attach_session`) — the high-frequency
  `Replay`/`Output` byte stream, written straight into the xterm.js instance without ever passing
  through the Zustand store (bytes are never state).
- **Global `emit`/`listen` events** (low-frequency) — `session://state-changed`,
  `session://created`, `daemon://disconnected`, `daemon://reconnected`, `workspace://created` — the
  metadata-only signals that drive the sidebar, tabs, and status dots.

### Hop B — core ⇄ daemon (Unix domain socket)

`src-tauri/src/socket_client.rs` (Hop-B client) and `crates/sessiond/src/socket_server.rs` (Hop-B
server) speak the same wire protocol: a `u32`-LE length prefix followed by a `bincode` 1.3.3
(fixint, deterministic little-endian)-encoded `Frame` (`Request { id, req }` /
`Response { id, res }` / `Push(..)`), defined once in `crates/protocol` and shared by both binaries
— the shared crate prevents *type* drift. The codec itself carries one non-obvious mechanism:
bincode 1.3 cannot deserialize serde-tagged enums, so `SessionLifecycle` and `TerminalEvent` use
hand-written dual-codec impls (JSON shape for ts-rs/Channel, a JSON string tunneled inside the
bincode frame on the wire — see the spec §3 amendment).

> **Locked contract:** DO NOT re-derive Serialize/Deserialize on SessionLifecycle or
> TerminalEvent, and DO NOT add new serde-tagged enums to the Hop-B protocol, until
> protocol v2 replaces the codec.

Every request/response is correlated by a client-chosen `id`; pushes (state changes, output,
replay) are unsolicited and fan out to whichever client currently has that session attached.

The daemon accepts a connection, requires a `Hello`/`Welcome` handshake (magic + protocol version;
mismatches are refused, never misparsed), verifies the peer's effective uid via `getpeereid`
(`crates/sessiond/src/singleton.rs`), and gives each connection a bounded outbound queue — a slow
or dead client is dropped without stalling the daemon or any other session (spec §13).

## Module ownership map (spec §4)

```
src/                              # React frontend
├─ ipc/
│  ├─ types.ts     # GENERATED from crates/protocol via ts-rs — never hand-edit
│  ├─ commands.ts  # typed invoke() wrappers (Hop A)
│  ├─ channel.ts   # attach_session Channel<TerminalEvent> plumbing
│  └─ events.ts    # global emit/listen subscriptions
├─ store/          # Zustand: sessions/workspaces METADATA ONLY — no bytes
├─ terminal/       # terminal-manager.ts — non-reactive Map<SessionId, Terminal>, keep-alive
└─ components/     # TerminalPane, TerminalTabs, WorkspaceSidebar, StatusDot, DaemonBanner

src-tauri/src/                    # Tauri core (the broker)
├─ main.rs / lib.rs               # Tauri Builder, plugin init, managed state, setup
├─ commands.rs                    # #[tauri::command] surface (Hop A)
├─ broker.rs                      # daemon frames → Channel/global events; Promise correlation
├─ socket_client.rs               # connect/handshake/reconnect (Hop B client, bounded backoff)
├─ paths.rs                       # thin re-export of bpa-paths (fail-fast pre-flights, spec §16)
└─ launchd.rs                     # install/bootstrap/kickstart the per-user LaunchAgent

crates/protocol/src/lib.rs        # SHARED Hop-B wire types (serde + ts-rs) — source of truth
crates/paths/src/lib.rs           # SHARED bpa-paths: workspace-root/cwd validation incl.
                                  # symlink-escape — one impl for core AND daemon (spec §16)
crates/sessiond/src/              # the daemon binary
├─ main.rs                        # arg parse, tracing init, flock, socket bind, SIGTERM drain
├─ boot.rs                        # testable boot core: bind → wire deps → serve → drain
├─ socket_server.rs               # tokio UnixListener; per-client task; peer-cred; bounded outq
├─ pty_supervisor.rs              # portable-pty lifecycle, threads, process-group kill, env allowlist
├─ live_grid.rs                   # alacritty_terminal Term: cursor/alt-screen/size (status source)
├─ scrollback.rs                  # sanitized raw-byte ring (replay source) + prune
├─ osc_parser.rs                  # OSC-133/OSC-7 streaming tokenizer + lifecycle state machine
├─ shell_integration/             # zsh + bash injection assets + installer
├─ persistence.rs                 # rusqlite (WAL), schema, migrations, degradation, rehydrate
├─ attach.rs                      # per-session single-attach registry + replay + live OSC strip
├─ singleton.rs                   # flock single-instance + socket path resolution + perms
└─ logging.rs                     # test-only structured-log-to-file seam (Task 25)
```

`crates/protocol` is the one genuinely shared file: `src/ipc/types.ts` is *generated* from it
(`ts-rs`), never hand-edited, and CI/`scripts/final-suite.sh` fails the build if the generated file
drifts from what's committed. Every other module above is owned by exactly one file-disjoint slice
of the daemon, the core, or the frontend — this is what let S0+S1's 25 tasks run largely in
parallel (see the plan's dependency graph).

## The survival model, restated

The daemon is deliberately NOT a Tauri sidecar child process — `launchd` supervises it
(`~/Library/LaunchAgents/ai.builderpro.desktop.sessiond.plist`, `KeepAlive.Crashed = true`), and
the Tauri core only ever holds a *socket connection* to it, never a process handle. See the
"Survival truth table" in `README.md` for the exact guarantees and honest boundaries: GUI
close/crash/restart keeps live shells running; ANY daemon stop (restart, upgrade, crash) ends
live shells — records + scrollback rehydrate as inactive sessions (spec §13).

## Resource envelope (per session)

Each live session costs **3 OS threads** (reader / wait / ticker) + **1 forwarder thread per live
attachment** + PTY file descriptors. There is currently **no enforced cap** on concurrent
sessions; a configurable cap + typed `SessionLimitReached` error is planned (BL-4,
`docs/backlog.md`).

## Related docs

- `docs/traceability.md` — every spec §14.2 contract row mapped to the concrete test(s) that cover
  it (no uncovered rows as of Task 25).
- `docs/build-macos.md` — the release build/sign/notarize/verify pipeline
  (`scripts/build-universal.sh`, `scripts/sign-verify.sh`, `scripts/smoke-clean-vm.sh`).
- `docs/superpowers/specs/2026-07-01-builderpro-s0s1-foundation-terminal-design.md` — the locked
  spec this whole implementation is derived from.
- `tests/e2e/README.md` — the three ways to exercise the survive-restart property (socket harness,
  launchd-managed variant, full-GUI manual/CI confirmation).
