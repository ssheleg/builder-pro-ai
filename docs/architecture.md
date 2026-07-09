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
                                     │ (codec-agnostic preamble, then u32-LE length prefix + CBOR Frame)
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
server) speak the same wire protocol. Every connection opens with a **codec-agnostic preamble**
(fixed-format raw LE bytes, magic `BPAA`, client `[min,max]` version range → daemon
`Accepted{chosen}`/`Incompatible{min,max}`, bounded read timeout, 256-byte build-string cap) —
version negotiation happens before either side commits to a codec, so a future codec change can
never be misparsed as a frame. Once negotiated (`chosen == 3` today — bumped from `2` in S2,
`[0.3.0]`: multi-root `Workspace` + new verbs are a planned wire break, so an old v2 daemon
negotiates `Incompatible` against a v3 client rather than silently misdecoding), both sides switch
to `u32`-LE
length-prefixed **CBOR** (`ciborium`)-encoded `Frame` (`Request { id, req }` / `Response { id, res
}` / `Push(..)`), defined once in `crates/protocol` and shared by both binaries — the shared crate
prevents *type* drift. CBOR is self-describing, so `SessionLifecycle` and `TerminalEvent` are plain
`#[derive(Serialize, Deserialize)]` tagged enums — the v1 dual-codec bridge (bincode 1.3 cannot
deserialize serde-tagged enums; the old workaround hand-wrote impls that tunneled a JSON string
inside the bincode frame) was retired in Pv2 (`[0.2.0]`). The append-only wire-discipline rule
(enum variant order frozen, fields added additively, every change ships a cross-version decode
test) remains — see the Pv2.1 reserved-batch amendment in
`docs/superpowers/specs/2026-07-06-protocol-v2-design.md`.

Every request/response is correlated by a client-chosen `id`; pushes (state changes, output,
replay) are unsolicited and fan out to every client attached to that session (Pv2 multi-subscriber
attach, §5 below).

The daemon verifies the peer's effective uid via `getpeereid` (`crates/sessiond/src/singleton.rs`)
during the preamble exchange, and gives each connection a bounded outbound queue — a slow or dead
client is dropped without stalling the daemon or any other session (spec §13).

## Module ownership map (spec §4)

```
src/                              # React frontend
├─ ipc/
│  ├─ types.ts     # GENERATED from crates/protocol via ts-rs — never hand-edit
│  ├─ commands.ts  # typed invoke() wrappers (Hop A) — S2 adds addWorkspaceRoot/
│  │                 removeWorkspaceRoot/getCommandEvents
│  ├─ fs.ts        # S2: typed wrappers for the fs_explorer commands (listDir, readFilePreview,
│  │                 create/rename/move/delete, reveal/openExternal)
│  ├─ channel.ts   # attach_session Channel<TerminalEvent> plumbing
│  └─ events.ts    # global emit/listen subscriptions — S2 adds onFsChanged/onFsWatchError/
│                    onWorkspaceUpdated
├─ store/          # Zustand: sessions/workspaces METADATA ONLY — no bytes; S2 adds the fs-slice
│                    (expanded/treeCache/selectedFile/showIgnored/filesRailOpen/watchPaused) and
│                    the `view: "home" | "workspace"` navigation slice
├─ terminal/       # terminal-manager.ts — non-reactive Map<SessionId, Terminal>, keep-alive;
│                    S2 adds link-provider.ts (pure regex file-link resolver, spec §6.5/D9)
└─ components/     # TerminalPane, TerminalTabs, WorkspaceSidebar, StatusDot, DaemonBanner;
                     S2 adds HomeView, FileTree, FilePreview, FilesRail, CommandStrip, Toast

src-tauri/src/                    # Tauri core (the broker)
├─ main.rs / lib.rs               # Tauri Builder, plugin init, managed state, setup
├─ commands.rs                    # #[tauri::command] surface (Hop A)
├─ broker.rs                      # daemon frames → Channel/global events; Promise correlation
├─ socket_client.rs               # connect/handshake/reconnect (Hop B client, bounded backoff)
├─ paths.rs                       # thin re-export of bpa-paths (fail-fast pre-flights, spec §16)
├─ fs_explorer.rs                 # S2: core-local file I/O — listDir/readFilePreview/create/
│                                  # rename/move/delete(→Trash)/reveal/openExternal over the
│                                  # `ignore`/`trash`/`opener` crates; every op validated by
│                                  # bpa_paths::validate_path_within first (spec §4, §16)
├─ fs_watcher.rs                  # S2: debounced FSEvents watch (`notify`/`notify-debouncer-full`)
│                                  # per active workspace root, gitignore-filtered → `fs://changed`
│                                  # / `fs://watch-error`; GUI-lifetime only (spec §5)
└─ launchd.rs                     # install/bootstrap/kickstart the per-user LaunchAgent

crates/protocol/src/lib.rs        # SHARED Hop-B wire types (serde + ts-rs) — source of truth
crates/paths/src/lib.rs           # SHARED bpa-paths: workspace-root/cwd validation incl.
                                  # symlink-escape — one impl for core AND daemon (spec §16).
                                  # S2 adds `validate_path_within`/`validate_parent_within`
                                  # (canonicalize + `starts_with` root; the per-op guard every
                                  # fs_explorer command runs before touching disk, spec S2 §4.1)
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
├─ attach.rs                      # multi-subscriber attach registry + replay + live OSC strip
├─ singleton.rs                   # flock single-instance + socket path resolution + perms
└─ logging.rs                     # test-only structured-log-to-file seam (Task 25)
```

`crates/protocol` is the one genuinely shared file: `src/ipc/types.ts` is *generated* from it
(`ts-rs`), never hand-edited, and CI/`scripts/final-suite.sh` fails the build if the generated file
drifts from what's committed. Every other module above is owned by exactly one file-disjoint slice
of the daemon, the core, or the frontend — this is what let S0+S1's 25 tasks run largely in
parallel (see the plan's dependency graph).

## Three-rail UI + the core-owned file I/O boundary (S2, shipped `[0.3.0]`)

S2 (`docs/superpowers/specs/2026-07-08-s2-workspace-explorer-home-design.md`) adds multi-root
workspaces, a file explorer + read-only preview, live file-watch, and an attention-first Home. The
webview grew from a two-pane (sidebar + terminal) layout to three rails:

```
┌ DaemonBanner ───────────────────────────────────────────────────────────┐
├──────────┬─────────────────────────────────────────────┬────────────────┤
│ ⌂ Home   │ Home: attention queue («нужен ты» → работают │ FILES (right,  │
│ • ws-... │  → завершились, «Пройти →» jumps+focuses)    │ collapsible):  │
│ (nav     │  | Workspace: stat chips + TerminalTabs +    │ FileTree       │
│  only)   │  per-session OSC-133 command strip           │ + FilePreview  │
└──────────┴─────────────────────────────────────────────┴────────────────┘
```

Left rail is pure navigation (`⌂ Home` + the existing workspace list). The center pane is either
`HomeView` (attention-first queue over the whole store, spec §6.2) or the workspace view (stat
chips + `TerminalTabs`/`TerminalPane` + `CommandStrip`, spec §6.3). The right rail (`FilesRail` →
`FileTree` + `FilePreview`) is hidden on Home, collapsible on a workspace.

**File I/O ownership (owner decision D4, "Approach A" — locked, not the only option considered):**
file listing, preview, create/rename/move/delete, and live watch run **in the Tauri core**
(`src-tauri/src/fs_explorer.rs`, `fs_watcher.rs`), not the daemon, and not over Hop-B. This keeps
`bpa-sessiond`'s charter unchanged (Data-layer charter, overview doc: *terminal-domain durable
state ONLY*) — the daemon still owns the `Workspace` row (`name`, `roots`) as data, but never reads
a byte of file content. Consequences of this split:

- File I/O is **GUI-lifetime only**: watchers start on workspace activation, stop on
  switch/unmount, and nothing is watched while the app is closed (D4) — unlike terminals, which the
  daemon keeps alive independent of the GUI.
- Every `fs_explorer` op validates its target against one of the active workspace's roots via the
  shared `bpa_paths::validate_path_within`/`validate_parent_within` (same crate, same
  canonicalize-then-`starts_with` pattern the daemon already used for `create_workspace`/
  `create_session` cwd checks — spec §16 "one impl for core AND daemon" extends to this new path
  class) — defense in depth on top of the daemon having already validated the root itself at
  `AddWorkspaceRoot` time.
- `fs_watcher.rs` uses `notify` + `notify-debouncer-full` (macOS → FSEvents, 250 ms debounce) per
  active root, filtered through the same `ignore`-crate gitignore matcher `fs_explorer` uses for
  listing, emitting `fs://changed { root, changedRelPaths }` (capped/deduped, `["*"]` on overflow
  meaning "refresh everything expanded") or `fs://watch-error { root, reason }` on failure — the
  frontend point-refreshes only affected expanded directories, never a full re-list.
- Delete always goes to the OS Trash (crate `trash`, `DeleteMethod::NsFileManager` on macOS —
  the default Finder/AppleScript method measured 60+s per delete in CI and was swapped for the
  direct `NSFileManager` API, same reversibility, ~0.15s) — never a permanent unlink.
- `Request::GetCommandEvents` (daemon, reads `command_events` — persisted since Pv2 but unconsumed
  until now) is the one S2 wire addition that *does* cross Hop-B: the per-session OSC-133 command
  strip is the first real UI consumer of that table.

If a future slice needs headless (no-GUI) file reads — e.g. `bpa-orchd` — that is explicitly
out of scope here (S2 spec §9): orchd gets its own file API in S9 when it actually needs it, not by
widening this core-local surface.

## The survival model, restated

The daemon is deliberately NOT a Tauri sidecar child process — `launchd` supervises it
(`~/Library/LaunchAgents/ai.builderpro.desktop.sessiond.plist`, `KeepAlive.Crashed = true`), and
the Tauri core only ever holds a *socket connection* to it, never a process handle. See the
"Survival truth table" in `README.md` for the exact guarantees and honest boundaries: GUI
close/crash/restart keeps live shells running; ANY daemon stop (restart, upgrade, crash) ends
live shells — records + scrollback rehydrate as inactive sessions (spec §13).

## Two-daemon topology (roadmap — ADR-HOST, 2026-07-06)

Vision v2–v4 adds a SECOND launchd-managed daemon, `bpa-orchd`, so unattended work (scheduled
workflows, the 24h prod self-heal loop, agent runs) survives GUI close — the GUI process cannot
host it, and `bpa-sessiond`'s charter bars it from domain logic.

```
┌──────────────┐   Hop-A (Tauri IPC)   ┌───────────────────────────┐
│  Webview UI  │ ◄───────────────────► │  Tauri core (GUI process) │
└──────────────┘                       └────────────┬──────────────┘
                                    Hop-B socket  ┌──┴──────────────┐
                                    (client)      ▼                 ▼
                              ┌───────────────────────┐   ┌─────────────────────────┐
                              │  bpa-sessiond          │   │  bpa-orchd (NEW)         │
                              │  terminal domain:      │   │  app-domain store,       │
                              │  PTYs, scrollback,     │◄──┤  scheduler, workflow     │
                              │  command_events (bpa.db)│  │  engine, agent runtime   │
                              └───────────────────────┘   └─────────────────────────┘
                                 (both launchd LaunchAgents; the GUI AND bpa-orchd are
                                  clients of bpa-sessiond — Pv2 multi-subscriber attach)
```

`bpa-orchd` reuses the `bpa-sessiond` patterns verbatim (launchd lifecycle + runbook, fail-closed
migrations, Pv2 drain/consent upgrade). Ownership boundary + entity map: overview Data-layer
charter. This block is a roadmap target — `bpa-orchd` is not built yet.

## Resource envelope (per session)

Each live session costs **3 OS threads** (reader / wait / ticker) + **1 forwarder thread per live
attachment** + PTY file descriptors. There is currently **no enforced cap** on concurrent
sessions; a configurable cap + typed `SessionLimitReached` error is planned (BL-4,
`docs/backlog.md`).

## Related docs

- `docs/traceability.md` — every spec §14.2 contract row mapped to the concrete test(s) that cover
  it (no uncovered rows as of Task 25).
- `docs/design-system.md` — visual + UX design rules (binds every feature's UI).
- `docs/build-macos.md` — the release build/sign/notarize/verify pipeline
  (`scripts/build-universal.sh`, `scripts/sign-verify.sh`, `scripts/smoke-clean-vm.sh`).
- `docs/superpowers/specs/2026-07-01-builderpro-s0s1-foundation-terminal-design.md` — the locked
  spec this whole implementation is derived from.
- `docs/superpowers/specs/2026-07-08-s2-workspace-explorer-home-design.md` — the S2 spec (multi-root
  workspaces, file explorer + preview + watch, attention-first Home, command strip, terminal file
  links) the "Three-rail UI" section above summarizes.
- `tests/e2e/README.md` — the three ways to exercise the survive-restart property (socket harness,
  launchd-managed variant, full-GUI manual/CI confirmation).
