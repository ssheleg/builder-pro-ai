# Architecture (S0+S1)

Authoritative source: `docs/superpowers/specs/2026-07-01-builderpro-s0s1-foundation-terminal-design.md`
(§2 Architecture, §4 module layout, §7 Hop-B protocol, §8 socket/launchd, §16 trust model). This
document is a navigable summary; if it ever drifts from the spec, the spec wins.

## Three processes, two Hop-B connections

Builder Pro AI is split into **three OS processes**: the GUI app (webview + Rust core) and TWO
independently `launchd`-supervised daemons — `bpa-sessiond` (terminal domain: PTYs, scrollback,
`command_events`) and `bpa-orchd` (app domain: projects/goals/ideas/insights/tasks/rulesets,
shipped S3, `[0.4.0]`). Closing (or crashing) the GUI never kills a running shell or loses domain
data — each daemon owns its own durable state and survives independently of the app AND of the
other daemon, supervised by `launchd`, not by the app. The core holds one socket connection to
EACH daemon; a failure of either degrades only that daemon's own panels (see "Honest-degradation"
in the S3 spec §11).

```
┌───────────────────────────── Builder Pro AI.app ─────────────────────────────┐
│  React webview (UI)                       Rust core (broker)                  │
│  • xterm panes, project rail/panel        • #[tauri::command] surface         │
│  • ⌘K quick capture, Home goals   ◄─Hop A─►  • socket_client.rs  (→ sessiond) │
│  • status dots               Tauri IPC       • orchd_client.rs   (→ orchd)    │
│  • Zustand (metadata only)                • app settings (tauri-plugin-store) │
└───────────────────────┬────────────────────────────────────┬─────────────────┘
        Hop B (UDS): preamble handshake, then u32-LE length + CBOR Frame
                        │ chosen == 3                         │ chosen == 1
             ┌──────────▼─────────────┐          ┌─────────────▼────────────┐
             │  bpa-sessiond (daemon) │          │  bpa-orchd (daemon)       │
             │  • PTY supervisor      │          │  • project/goal/idea/    │
             │  • OSC-133 parser + SM │          │    insight/task/ruleset  │
             │  • sanitized byte ring │          │    CRUD + export/import  │
             │  • alacritty live grid │          │  • ruleset_files.rs — the│
             │  • rusqlite (WAL,      │          │    ONE file family orchd │
             │    bpa.db)             │          │    touches (D4, narrow)  │
             │  ◄─ launchd LaunchAgent│          │  • rusqlite (WAL,        │
             │   (KeepAlive{Crashed}) │          │    orchd.db)             │
             │   owns ALL PTYs +      │          │  ◄─ launchd LaunchAgent  │
             │   ALL terminal-domain  │          │   (KeepAlive{Crashed})   │
             │   durable state        │          │   owns ALL app-domain    │
             └──────────┬─────────────┘          │   durable state          │
       PTYs via portable-pty                     └───────────────────────────┘
       (child setsid'd, own pgrp)
             ┌──────────▼─────────────┐
             │ zsh / bash / agent CLI │
             └────────────────────────┘
```

`bpa-sessiond` negotiates `chosen == 3` (bumped S2, `[0.3.0]`); `bpa-orchd` negotiates its OWN,
independent version space `[1,1]` (S3 spec D8) — same `BPAA` preamble magic, daemons are
distinguished by socket path, not by preamble content, so the two version spaces never collide.

### Hop A — webview ⇄ core (Tauri IPC)

The React webview never touches either raw socket. It calls typed `#[tauri::command]`s
(`src-tauri/src/commands.rs`) for request/response operations — sessiond verbs (`create_workspace`,
`create_session`, `write_stdin`, `resize`, `kill_session`, `get_session_state`, …) AND, since S3,
orchd verbs named `orchd_` + snake_case (`orchd_create_project`, `orchd_set_idea_project`,
`orchd_export_all`, …) — and receives three kinds of push data from the core:

- A **`Channel<TerminalEvent>`** per attached terminal (`attach_session`) — the high-frequency
  `Replay`/`Output` byte stream, written straight into the xterm.js instance without ever passing
  through the Zustand store (bytes are never state).
- **Global `emit`/`listen` events** (low-frequency) — `session://state-changed`,
  `session://created`, `daemon://disconnected`, `daemon://reconnected`, `workspace://created` — the
  metadata-only signals that drive the sidebar, tabs, and status dots.
- **Orchd coarse-invalidation events** (S3) — `orchd://projects-changed`, `orchd://goals-changed`,
  `orchd://ideas-changed`, `orchd://insights-changed`, `orchd://tasks-changed`,
  `orchd://ruleset-changed`, `orchd://down`, `orchd://up`, `orchd://incompatible` — the GUI
  re-fetches only the affected list on each; no full-entity push mirroring (S3 spec D6).

### Hop B — core ⇄ daemons (two independent Unix domain sockets)

The core holds ONE Hop-B connection to EACH daemon; both speak the SAME codec-agnostic wire
mechanics, just with independent version spaces and separate frame enums:

| | `bpa-sessiond` | `bpa-orchd` |
|---|---|---|
| Client | `src-tauri/src/socket_client.rs` | `src-tauri/src/orchd_client.rs` (mirrors socket_client.rs's structure) |
| Server | `crates/sessiond/src/socket_server.rs` | `crates/orchd/src/socket_server.rs` |
| Frame enums | `crates/protocol` (`bpa-protocol`) | `crates/orchd-proto` (`bpa-orchd-proto`) |
| Negotiated version | `chosen == 3` | `chosen == 1` |

Every connection opens with a **codec-agnostic preamble** (fixed-format raw LE bytes, magic
`BPAA`, client `[min,max]` version range → daemon `Accepted{chosen}`/`Incompatible{min,max}`,
bounded read timeout, 256-byte build-string cap) — version negotiation happens before either side
commits to a codec, so a future codec change can never be misparsed as a frame. The preamble
mechanics live in `bpa-protocol` (shared by both wires — no "neutral wire-core crate" churn, S3
spec D8) and, daemon-side, in the shared `bpa-daemon-core::handshake` module both `bpa-sessiond`
and `bpa-orchd` call into. sessiond's version was bumped `2 → 3` in S2 (`[0.3.0]`: multi-root
`Workspace` + new verbs are a planned wire break, so an old v2 daemon negotiates `Incompatible`
against a v3 client rather than silently misdecoding); orchd's `[1,1]` is its OWN independent
version space, unrelated to sessiond's — the preamble carries no single-daemon assumption, and the
two daemons are distinguished by socket path, not by preamble content (S3 spec D8). Once
negotiated, both sides switch to `u32`-LE length-prefixed **CBOR** (`ciborium`)-encoded frames —
sessiond's `Frame`/`Request`/`Response`/`Push` from `crates/protocol`, orchd's `OrchdFrame`/
`OrchdRequest`/`OrchdResponse`/`OrchdPush` from `crates/orchd-proto` — each defined once and shared
by its own client+server pair, so the shared crate prevents *type* drift within each wire. CBOR is
self-describing, so every tagged enum on both wires is a plain `#[derive(Serialize, Deserialize)]`
enum — the v1 dual-codec bridge (bincode 1.3 cannot deserialize serde-tagged enums; the old
workaround hand-wrote impls that tunneled a JSON string inside the bincode frame) was retired in
Pv2 (`[0.2.0]`) and orchd never had it to begin with. The append-only wire-discipline rule (enum
variant order frozen, fields added additively, every change ships a cross-version decode test)
applies to BOTH wires — see the Pv2.1 reserved-batch amendment in
`docs/superpowers/specs/2026-07-06-protocol-v2-design.md` for sessiond's history.

Every request/response is correlated by a client-chosen `id` on its own wire; sessiond pushes
(state changes, output, replay) are unsolicited and fan out to every client attached to that
session (Pv2 multi-subscriber attach, §5 below); orchd pushes are coarse-grained per-family
invalidation signals (S3 spec D6) fanned out via a generic `Broadcaster<OrchdFrame>` client
registry (`crates/daemon-core::broadcast`, the same generic type sessiond's broadcaster was
re-seated on in the S3 daemon-core extraction).

Each daemon independently verifies the peer's effective uid via `getpeereid`
(`crates/sessiond/src/singleton.rs` / the shared `bpa_daemon_core::singleton::check_peer_cred`
sessiond's own module now wraps) during its own preamble exchange, and gives each connection a
bounded outbound queue — a slow or dead client is dropped without stalling that daemon or any
other client/session (spec §13). A failure of ONE daemon's socket never affects the other's — the
core's two client connections are fully independent (S3 spec §11).

## Module ownership map (spec §4)

```
src/                              # React frontend
├─ ipc/
│  ├─ types.ts     # GENERATED from crates/protocol via ts-rs — never hand-edit
│  ├─ orchd-types.ts # S3: GENERATED from crates/orchd-proto via ts-rs — never hand-edit
│  ├─ commands.ts  # typed invoke() wrappers (Hop A) — S2 adds addWorkspaceRoot/
│  │                 removeWorkspaceRoot/getCommandEvents
│  ├─ orchd.ts     # S3: typed invoke() wrappers for every `orchd_*` command (Hop A, orchd side)
│  ├─ fs.ts        # S2: typed wrappers for the fs_explorer commands (listDir, readFilePreview,
│  │                 create/rename/move/delete, reveal/openExternal)
│  ├─ channel.ts   # attach_session Channel<TerminalEvent> plumbing
│  └─ events.ts    # global emit/listen subscriptions — S2 adds onFsChanged/onFsWatchError/
│                    onWorkspaceUpdated; S3 adds onOrchd*Changed/onOrchdDown/onOrchdUp/
│                    onOrchdIncompatible
├─ store/          # Zustand: sessions/workspaces METADATA ONLY — no bytes; S2 adds the fs-slice
│                    (expanded/treeCache/selectedFile/showIgnored/filesRailOpen/watchPaused) and
│                    the `view` navigation slice (S3 widens it to
│                    `"home" | "workspace" | "project"`); S3 adds `domainSlice`
│                    (projects/goalsByProject/ideas/insights/tasksByProject/rulesets/
│                    activeProjectId/orchdDown/orchdIncompatible)
├─ terminal/       # terminal-manager.ts — non-reactive Map<SessionId, Terminal>, keep-alive;
│                    S2 adds link-provider.ts (pure regex file-link resolver, spec §6.5/D9)
└─ components/     # TerminalPane, TerminalTabs, WorkspaceSidebar, StatusDot, DaemonBanner;
                     S2 adds HomeView, FileTree, FilePreview, FilesRail, CommandStrip, Toast;
                     S3 adds ProjectPanel (Обзор/Цели/Идеи/Задачи/Инсайты/Правила tabs),
                     GoalTree, IdeasList, TasksList, InsightsList, RulesetPanel,
                     CreateProjectDialog, QuickCapture (⌘K), HomeGoals, OrchdDownBanner;
                     WorkspaceSidebar restructured into project-group rows (S3 spec §10)

src-tauri/src/                    # Tauri core (the broker)
├─ main.rs / lib.rs               # Tauri Builder, plugin init, managed state, setup;
│                                  # S3: bring_up_orchd (mirrors bring_up_daemon) spawned
│                                  # alongside bring_up_daemon as its own async_runtime task
├─ commands.rs                    # #[tauri::command] surface (Hop A) — S3 adds every `orchd_*`
│                                  # command (one per S3 spec §4.2 verb, thin + inner-fn testable)
├─ broker.rs                      # daemon frames → Channel/global events; Promise correlation;
│                                  # S3 adds map_orchd_push / register_orchd (orchd:// events)
├─ socket_client.rs               # connect/handshake/reconnect (Hop B client, bounded backoff)
├─ orchd_client.rs                # S3: MIRROR of socket_client.rs for bpa-orchd — own
│                                  # OrchdClient/OrchdClientSlot/OrchdClientError, resolves
│                                  # orchd.sock via a LOCAL socket_dir() copy (no bpa-daemon-core
│                                  # dep added to src-tauri, S3 spec §9)
├─ paths.rs                       # thin re-export of bpa-paths (fail-fast pre-flights, spec §16)
├─ fs_explorer.rs                 # S2: core-local file I/O — listDir/readFilePreview/create/
│                                  # rename/move/delete(→Trash)/reveal/openExternal over the
│                                  # `ignore`/`trash`/`opener` crates; every op validated by
│                                  # bpa_paths::validate_path_within first (spec §4, §16)
├─ fs_watcher.rs                  # S2: debounced FSEvents watch (`notify`/`notify-debouncer-full`)
│                                  # per active workspace root, gitignore-filtered → `fs://changed`
│                                  # / `fs://watch-error`; GUI-lifetime only (spec §5)
└─ launchd.rs                     # install/bootstrap/kickstart the per-user LaunchAgent for
                                   # EITHER daemon; S3 parameterizes `LaunchdAgent` additively
                                   # (label/stdout_log_name/stderr_log_name fields) — sessiond
                                   # call sites pass the pre-existing values byte-identically,
                                   # orchd call sites pass ORCHD_LABEL/orchd.out.log/orchd.err.log

crates/protocol/src/lib.rs        # SHARED sessiond Hop-B wire types (serde + ts-rs) — source of
                                   # truth; S3 generalizes the framing primitives additively so
                                   # daemon-core + orchd-proto can depend on them (spec D8)
crates/daemon-core/src/           # NEW (S3, bpa-daemon-core): shared daemon infrastructure BOTH
│                                  # bpa-sessiond and bpa-orchd build on — extracted FIRST, phase
│                                  # gate of the S3 build (spec D2)
├─ dirs.rs                        # app-support dir resolution (shared by both daemons' DB/log paths)
├─ singleton.rs                   # flock single-instance + socket path resolution + perms +
│                                  # peer-cred check — the runtime_dir resolution
│                                  # ($XDG_RUNTIME_DIR/bpa else /tmp/bpa-{uid}) both daemons use
├─ logging.rs                     # tracing init (parameterized log-file name) + test-only seam
├─ migrate.rs                     # generic fail-closed forward-only migration runner (whole-chain,
│                                  # VersionTooNew, per-step rollback) — both daemons' schemas run on it
├─ handshake.rs                   # codec-agnostic preamble accept/negotiate (shared by both wires)
└─ broadcast.rs                   # generic `Broadcaster<F>` client-push registry — sessiond's
                                   # existing broadcaster AND orchd's `Broadcaster<OrchdFrame>`
                                   # both instantiate this one generic type
crates/paths/src/lib.rs           # SHARED bpa-paths: workspace-root/cwd validation incl.
                                  # symlink-escape — one impl for core AND daemon (spec §16).
                                  # S2 adds `validate_path_within`/`validate_parent_within`
                                  # (canonicalize + `starts_with` root; the per-op guard every
                                  # fs_explorer command runs before touching disk, spec S2 §4.1)
crates/sessiond/src/              # the terminal daemon binary — S3 RE-SEATED on daemon-core
                                  # (behavior byte-identical; on-disk paths d.sock/d.lock/bpa.db
                                  # unchanged, proven by test)
├─ main.rs                        # arg parse, tracing init (daemon-core), flock (daemon-core),
│                                  # socket bind, SIGTERM drain
├─ boot.rs                        # testable boot core: bind → wire deps → serve → drain
├─ socket_server.rs               # tokio UnixListener; per-client task; peer-cred; bounded outq
├─ pty_supervisor.rs              # portable-pty lifecycle, threads, process-group kill, env allowlist
├─ live_grid.rs                   # alacritty_terminal Term: cursor/alt-screen/size (status source)
├─ scrollback.rs                  # sanitized raw-byte ring (replay source) + prune
├─ osc_parser.rs                  # OSC-133/OSC-7 streaming tokenizer + lifecycle state machine
├─ shell_integration/             # zsh + bash injection assets + installer
├─ persistence.rs                 # rusqlite (WAL), schema, migrations, degradation, rehydrate
├─ attach.rs                      # multi-subscriber attach registry + replay + live OSC strip
├─ singleton.rs                   # thin bpa-sessiond wrapper over daemon-core::singleton — pins
│                                  # the on-disk leaf names (d.sock/d.lock) unchanged
└─ logging.rs                     # thin re-export of daemon-core's test-only logging seam
crates/orchd-proto/src/lib.rs     # NEW (S3, bpa-orchd-proto): orchd's own wire enums
                                  # (OrchdFrame/OrchdRequest/OrchdResponse/OrchdPush), the 6
                                  # entity structs, version consts (ORCHD_{CLIENT,DAEMON}_
                                  # {MIN,MAX}_VERSION = 1), ts-rs export (spec §4.2)
crates/orchd/src/                 # NEW (S3, bpa-orchd): the app-domain daemon binary
├─ main.rs                        # arg parse, tracing init, flock, socket bind, SIGTERM drain —
│                                  # mirrors sessiond's main.rs shape exactly, on daemon-core
├─ boot.rs                        # testable boot core: bind → wire deps → serve → drain; ensures
│                                  # the global ruleset row + rules/global.md at every boot
├─ persistence.rs                 # rusqlite (WAL), orchd.db schema v1, CRUD × 6 entity families,
│                                  # every invariant/cascade, migration runner (daemon-core)
├─ socket_server.rs               # tokio UnixListener; dispatch every OrchdRequest verb; peer-cred;
│                                  # bounded outq; Broadcaster<OrchdFrame> push fan-out
├─ ruleset_files.rs                # the ONE file family orchd touches (D4, narrow exception) —
│                                  # atomic-write (tmp+rename) + sha256 hash + fresh-read-on-Get
└─ export.rs                      # per-project + whole-store JSON export/import, bundleFormat: 1,
                                   # field-verbatim preservation, 16 MiB frame-cap guard (spec §8)
```

`crates/protocol` and `crates/orchd-proto` are the two genuinely shared files: `src/ipc/types.ts`
is *generated* from the former, `src/ipc/orchd-types.ts` from the latter (both via `ts-rs`), never
hand-edited, and CI/`scripts/final-suite.sh` fails the build if either generated file drifts from
what's committed. Every other module above is owned by exactly one file-disjoint slice
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
out of scope here (S2 spec §9): a GENERAL orchd file API (arbitrary listing/reading/writing over
Hop-B) is not built by widening this core-local surface, and remains S9 work.

**Amended S3 (`[0.4.0]`) — a deliberate NARROW exception, not the general file API above:** orchd
DOES touch exactly ONE file family starting in S3 — RuleSet markdown (`crates/orchd/src/
ruleset_files.rs`, S3 spec D4/§7). DB rows store `md_path` + `md_hash` (sha256); files are the
source of truth; external edits/deletions surface honestly (`RuleFileState::Ok` /
`ExternallyModified` / `Missing`) rather than silently overwritten. This is scoped tight on
purpose: app-support-defaulted paths (owner may repoint to any absolute path), atomic writes
(tmp+rename), no fs-watch (read-on-`GetRuleSet` + explicit `AcknowledgeRuleFile`, YAGNI), and — per
`ruleset_files.rs`'s own module doc — it is the ONLY file I/O anywhere in the `bpa-orchd` crate. A
general file API (arbitrary paths, arbitrary read/write, fs-watch, non-rules content) is still S9;
this exception does not widen into one.

## The survival model, restated

The daemon is deliberately NOT a Tauri sidecar child process — `launchd` supervises it
(`~/Library/LaunchAgents/ai.builderpro.desktop.sessiond.plist`, `KeepAlive.Crashed = true`), and
the Tauri core only ever holds a *socket connection* to it, never a process handle. See the
"Survival truth table" in `README.md` for the exact guarantees and honest boundaries: GUI
close/crash/restart keeps live shells running; ANY daemon stop (restart, upgrade, crash) ends
live shells — records + scrollback rehydrate as inactive sessions (spec §13).

## Two-daemon topology — SHIPPED in S3 (`[0.4.0]`)

ADR-HOST (platform overview §2, 2026-07-06) called for a SECOND launchd-managed daemon,
`bpa-orchd`, so unattended work survives GUI close — the GUI process cannot host it, and
`bpa-sessiond`'s charter bars it from domain logic. S3 shipped that daemon and its topology (see
the top-of-doc diagram + module map above for the concrete shape); this section records what's
actually in place today vs. what ADR-HOST still reserves for later slices.

**Shipped in S3:** the app-domain SQLite store (`orchd.db`, schema v1) and full CRUD for the six
S3 entity families (Project, Goal, Idea, Insight, Task, RuleSet) over its own independent Hop-B
wire (`[1,1]`); per-project + whole-store export/import; the RuleSet markdown file exception
(D4, above); a working owner-facing UI (project rail/panel, goal tree, ideas/tasks/insights
editors, ⌘K quick-capture, Home goals). `bpa-orchd` reuses the `bpa-sessiond` patterns verbatim —
launchd lifecycle + runbook (`docs/runbook-orchd.md`), fail-closed forward-only migrations
(`bpa-daemon-core::migrate`), the drain/consent upgrade choreography (§9 above) — proven true by
the S3 daemon-core extraction (byte-identical sessiond behavior, tested) and the shared modules
both daemons build on.

**Still ADR-HOST roadmap, NOT part of S3:** the scheduler + event-trigger runtime (SW2), the
workflow-engine runtime (SW1), and the agent runtime (S6b+) are future hosts INSIDE `bpa-orchd` —
S3 ships the store and daemon they'll eventually run in, not the schedulers/engines/agents
themselves. Until those land, `bpa-orchd` has no live runtime state of its own to lose on restart
(see the survival-truth-table row in `README.md`/platform overview §2) — every row it owns is
already durable SQLite.

## Resource envelope (per session)

Each live session costs **3 OS threads** (reader / wait / ticker) + **1 forwarder thread per live
attachment** + PTY file descriptors. There is currently **no enforced cap** on concurrent
sessions; a configurable cap + typed `SessionLimitReached` error is planned (BL-4,
`docs/backlog.md`).

## Related docs

- `docs/traceability.md` — every spec §14.2 contract row mapped to the concrete test(s) that cover
  it (no uncovered rows as of Task 25; S3 rows added separately, see that doc's own section).
- `docs/design-system.md` — visual + UX design rules (binds every feature's UI).
- `docs/build-macos.md` — the release build/sign/notarize/verify pipeline
  (`scripts/build-universal.sh`, `scripts/sign-verify.sh`, `scripts/smoke-clean-vm.sh`).
- `docs/runbook-daemon.md` / `docs/runbook-orchd.md` — per-daemon ops runbooks (real paths,
  inspect/restart/reset/uninstall commands, dev-vs-installed notes).
- `docs/superpowers/specs/2026-07-01-builderpro-s0s1-foundation-terminal-design.md` — the locked
  spec this whole implementation is derived from.
- `docs/superpowers/specs/2026-07-08-s2-workspace-explorer-home-design.md` — the S2 spec (multi-root
  workspaces, file explorer + preview + watch, attention-first Home, command strip, terminal file
  links) the "Three-rail UI" section above summarizes.
- `docs/superpowers/specs/2026-07-13-s3-orchd-domain-foundation-design.md` — the locked S3 spec
  (`bpa-orchd` + app-domain foundation) the "Two-daemon topology" and module-map sections above
  summarize.
- `tests/e2e/README.md` — the three ways to exercise the survive-restart property (socket harness,
  launchd-managed variant, full-GUI manual/CI confirmation).
