# Architecture (S0+S1)

Authoritative source: `docs/superpowers/specs/2026-07-01-builderpro-s0s1-foundation-terminal-design.md`
(§2 Architecture, §4 module layout, §7 Hop-B protocol, §8 socket/launchd, §16 trust model). This
document is a navigable summary; if it ever drifts from the spec, the spec wins.

## Three processes, two Hop-B connections

Builder Pro AI is split into **three OS processes**: the GUI app (webview + Rust core) and TWO
independently `launchd`-supervised daemons — `bpa-sessiond` (terminal domain: PTYs, scrollback,
`command_events`) and `bpa-orchd` (app domain: projects/goals/ideas/insights/tasks/rulesets,
shipped S3, `[0.4.0]`; plus a knowledge graph — nodes/edges + a workspace-wide retrieval API,
shipped S4, `[0.5.0]`; plus an MCP client + OAuth/api-key connectors + a skills registry — the
app's first outbound network egress and macOS Keychain surface, shipped S-EXT, `[0.6.0]`; plus a
research pipeline — the idea→research→insight→task loop, `research_run` schema v4, and orchd's
first long-lived background run driver, shipped S-IDEA, `[0.7.0]`). Closing
(or crashing) the GUI never kills a running shell or loses domain
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
                     S3 adds ProjectPanel (Overview/Goals/Ideas/Tasks/Insights/Rules tabs),
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
                                  # {MIN,MAX}_VERSION = 1), ts-rs export (spec §4.2); S-EXT appends
                                  # Mcp*/Connector*/Skill*/Trust* entities+verbs+pushes at the END
                                  # of every enum (append-only — the version space stays [1,1]);
                                  # S-IDEA appends ResearchStartRun/ResearchListRuns/ResearchGetRun
                                  # + entity ResearchRun/enum ResearchStatus + push
                                  # ResearchRunsChanged at the END too — still [1,1]
crates/secrets/src/lib.rs         # NEW (S-EXT, bpa-secrets): the ONLY Keychain caller in the app —
                                  # a thin `security-framework::passwords` wrapper (set/get/delete
                                  # generic password), fixed service prefix "ai.builderpro.desktop";
                                  # never logs the secret bytes (BL-20)
crates/mcp/src/                   # NEW (S-EXT, bpa-mcp): thin wrapper over the official `rmcp`
│                                  # SDK — protocol isolation; orchd domain code never imports rmcp
│                                  # types directly
├─ client.rs                      # connect(TransportConfig, Option<Bearer>) -> McpSession;
│                                  # McpSession::{list_tools, call_tool, protocol_version, close}
├─ transport.rs                   # TransportConfig::{Http{url}, Stdio{command,args,env}} → rmcp's
│                                  # StreamableHttpClientTransport / TokioChildProcess; the stdio
│                                  # child is `env_clear()`'d — the CALLER (orchd) must supply the
│                                  # complete, already-filtered child env, no ambient inheritance
├─ types.rs                       # McpTool / McpToolResult / Usage — maps rmcp's types to this
│                                  # crate's own project-shaped types
└─ error.rs                       # McpError { Transport, Protocol, Timeout, ToolError, Auth }
crates/orchd/src/                 # NEW (S3, bpa-orchd): the app-domain daemon binary
├─ main.rs                        # arg parse, tracing init, flock, socket bind, SIGTERM drain —
│                                  # mirrors sessiond's main.rs shape exactly, on daemon-core
├─ boot.rs                        # testable boot core: bind → wire deps → serve → drain; ensures
│                                  # the global ruleset row + rules/global.md at every boot; S-IDEA
│                                  # adds a boot-reconcile step right after open_db that flips any
│                                  # research_run stuck pending/running to failed{interrupted} (D11)
├─ persistence.rs                 # rusqlite (WAL), orchd.db schema v1 (S4: v2; S-EXT: v3,
│                                  # additive) — CRUD × 6 entity families + the S-EXT MCP/connector/
│                                  # skill/trust tables, every invariant/cascade, migration runner
│                                  # (daemon-core)
├─ socket_server.rs               # tokio UnixListener; dispatch every OrchdRequest verb (S4: the 9
│                                  # graph verbs; S-EXT: every Mcp*/Connector*/Skill*/Trust* verb
│                                  # too; S-IDEA: the 3 Research* verbs); peer-cred; bounded outq;
│                                  # Broadcaster<OrchdFrame> push fan-out (S4: GraphChanged to every
│                                  # affected project, deduped; S-IDEA: ResearchRunsChanged)
├─ ruleset_files.rs                # the ONE file family orchd touches via THIS module (D4, narrow
│                                  # exception) — atomic-write (tmp+rename) + sha256 hash +
│                                  # fresh-read-on-Get (S-EXT's skills/registry.rs is a second,
│                                  # equally narrow SKILL.md file-read surface, see below)
├─ graph.rs                       # NEW (S4): graph_node/graph_edge CRUD + the workspace-wide
│                                  # retrieval API (list_project_graph/neighborhood/search_nodes) —
│                                  # see "Knowledge graph" below
├─ research/mod.rs                 # NEW (S-IDEA): research_run CRUD + the async run-driver
│                                  # (start_run spawns a tokio task that calls the SHIPPED
│                                  # mcp::invoke::call_tool, 3-phase-locked) + the boot-reconcile
│                                  # query (reconcile_interrupted_research_runs, D11) — see
│                                  # "Research pipeline" below
├─ export.rs                      # per-project + whole-store JSON export/import, bundleFormat: 1,
│                                  # field-verbatim preservation, 16 MiB frame-cap guard (spec §8)
├─ mcp/                            # NEW (S-EXT): registry.rs (mcp_server/mcp_tool CRUD, global +
│                                  # per-project scope), lifecycle.rs (connect/disconnect, tool-
│                                  # cache refresh), invoke.rs (trust-authorize → bpa_mcp::call_tool
│                                  # → invocation+artifact rows; retry/timeout/honest degradation),
│                                  # cache.rs — see "Extensions" below
├─ connectors/                     # NEW (S-EXT): accounts.rs (OAuth 2.1 PKCE via `oauth2`, tokens
│                                  # in Keychain via bpa-secrets, refresh; api-key accounts),
│                                  # adapter.rs (ConnectorAdapter trait + the one reference
│                                  # `generic-rest` adapter + ConnectorInvoke — the SAME trust +
│                                  # invocation/artifact path an MCP tool call uses)
├─ skills/                         # NEW (S-EXT): registry.rs — SKILL.md CRUD, minimal frontmatter
│                                  # parse, files-as-truth (Present/Modified/Missing, mirrors
│                                  # ruleset_files.rs's own honest-degradation pattern)
└─ trust.rs                        # NEW (S-EXT): the single pre-dispatch choke-point — authorize()
                                   # for connect / stdio-spawn / tool-call / connector-invoke;
                                   # persisted consent grants, spend/rate policy caps, an
                                   # append-only audit log — see "Extensions" below
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
│ ⌂ Home   │ Home: attention queue («needs you» → running │ FILES (right,  │
│ • ws-... │  → exited, «Go →» jumps+focuses)             │ collapsible):  │
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

## Knowledge graph — SHIPPED in S4 (`[0.5.0]`)

S4 (`docs/superpowers/specs/2026-07-14-s4-knowledge-graph-design.md`) adds a knowledge graph to
the `bpa-orchd` store — `orchd.db` schema v2 (additive, forward-only migration from v1) adds two
tables: `graph_node` (typed `kind`: `concept | fact | artifact | decision | note | entity_ref`)
and `graph_edge` (typed `kind`: `relates | depends | derives | supports | contradicts | parent`),
both owned entirely by `crates/orchd/src/graph.rs` (new module, no new crate). No sessiond change.

- **`entityRef` nodes are soft-refs (D3):** an `entityRef` node stores `entity_type` +
  `entity_id` (goal/idea/insight/task) with NO foreign key into the domain tables. Deleting the
  referenced goal/idea/insight/task does not delete or corrupt the graph node — the node
  persists and a read-time resolver looks up the live domain row's title on every
  `list_project_graph`/`neighborhood` call; if the row is gone, the node keeps its last-known
  stored `label` and the UI renders it with `isOrphan: true` («source deleted»). Exactly one
  `entityRef` node exists per `(entity_type, entity_id)` (partial unique index). A strategic-goal
  `entityRef` node is auto-seeded in the same transaction as `CreateProject` (D6), so a project's
  graph is never empty; the schema-v2 migration backfills one for every pre-S4 project on upgrade.
- **Cross-project edges (D4):** a `graph_edge` may link nodes belonging to different projects —
  legal because both live in the one `orchd.db` store, with `ON DELETE CASCADE` removing a node's
  incident edges automatically. A cross-project edge survives BOTH projects' daemon restarts (S4
  spec DoD; proven by `tests/e2e/orchd-survive.mjs` phase 5).
- **Workspace-wide retrieval API (D5 — the S6-agent contract):** three read verbs, none scoped to
  a single project — this is the interface future S6 agents call to read across the whole
  workspace, not just their own project:
  - `GraphListProject { project_id }` → the project's own nodes + every edge incident to them +
    the foreign endpoint nodes of any cross-project edge as read-only `external_nodes` "ghosts".
  - `GraphNeighborhood { node_id, depth }` → a bidirectional recursive-CTE traversal up to `depth`
    hops (clamped to 6), crossing project boundaries freely. This is the `<100 ms` DoD query: a
    depth-3 neighborhood rooted at a project's strategic-goal node on a synthetic 500-node/
    1000-edge graph measures ~51 ms (`cargo test -p bpa-orchd --lib
    graph::tests::neighborhood_depth_3_on_500_node_1000_edge_graph_is_under_100ms_rooted_at_goal_node`).
  - `GraphSearch { query, project_id: Option<..> }` → `label`/`body` substring search,
    workspace-wide when `project_id` is `None`, capped at 200 rows.
  Every mutating graph verb honors the S3 archived-project guard (either endpoint's project
  archived ⇒ `Invariant`) and broadcasts a coarse `orchd://graph-changed { projectId }` push to
  **every project the change actually touches** — a cross-project edge mutation pushes to BOTH
  endpoint projects, not just the mutated row's own project, so a stale `external_nodes` ghost
  elsewhere is never left un-invalidated.
- **UI:** a 7th `ProjectPanel` tab, «Graph», renders an editable `@xyflow/react` canvas
  (`src/components/graph/GraphCanvas.tsx`) — drag moves a node (debounced `GraphMoveNode`),
  connecting two nodes adds an edge, a toolbar adds/deletes nodes and searches (match ⇒ accent
  ring), and every mutating control is `disabled` while `orchd://down`. Clicking a cross-project
  ghost node navigates to its own project (`openProject`); clicking a LOCAL `entityRef` node is
  currently an honest no-op — the panel has no deep-link seam yet from the graph tab into a
  specific goal/idea/insight/task row in another tab, so S4 does not fake a navigation that
  wouldn't actually land on the referenced entity (tracked as follow-up, not silently dropped).
- **Out of scope in S4 (by design, D12):** no agent runtime uses this API yet (S6 is what
  hard-blocks on it existing); no auto-population of the graph beyond the D6 strategic seed;
  retrieval is structural + `LIKE`-text only, no embeddings/semantic search (see
  `docs/backlog.md`).

## Extensions: MCP client + connectors + skills — SHIPPED in S-EXT (`[0.6.0]`)

S-EXT (`docs/superpowers/specs/2026-07-15-s-ext-mcp-connectors-design.md`) gives the app an
**outbound extension layer**: connect to external MCP servers, discover and invoke their tools,
hold external OAuth/api-key accounts ("connectors"), and register portable skills — all managed
from a UI, all gated by a single trust choke-point. This is the **first application-driven
egress + Keychain surface** in the product (`reqwest` was already latent in the build graph via
Tauri, but no BuilderProAI code previously performed outbound network I/O; `security-framework`/
Keychain is genuinely first-time). Egress and Keychain access live ENTIRELY in `bpa-orchd` —
never Hop-B, never sessiond, never the GUI core.

**New crates:** `bpa-secrets` (the only Keychain caller — `security-framework::passwords`
set/get/delete, fixed service prefix `ai.builderpro.desktop`, never logs the secret bytes — BL-20)
and `bpa-mcp` (a thin wrapper over the official `rmcp = "2.2"` SDK — JSON-RPC, transports,
`initialize` negotiation, `tools/*` — so orchd domain code never imports rmcp types directly).
Two `reqwest` instances exist in the dependency graph (`bpa-mcp`'s own `reqwest 0.13` + `oauth2`
5.0's pinned `reqwest 0.12`, a known/documented upstream constraint) — **both are rustls**, no
OpenSSL/native-tls anywhere in the egress path (keeps the notarized build unaffected).

**`orchd.db` schema v3 (additive, `SCHEMA_VERSION` 2→3):** `mcp_server`/`mcp_tool` (the MCP
registry + cached tool descriptors, global or per-project scope), `account` (OAuth/api-key
connector accounts — a Keychain *ref*, never the token bytes), `mcp_invocation`/`mcp_artifact`
(per-call records + durable results — `server_id` XOR `account_id`, so an `McpCallTool` and a
`ConnectorInvoke` share exactly one persistence path), `skill` (SKILL.md registry), and the trust
layer's own `consent_grant`/`policy`/`audit_log`.

**MCP client:** a registry (add/enable/disable, global + per-project), both transports —
**Streamable HTTP** (remote servers, e.g. prowl.chat) and **stdio** (local child processes,
behind the `stdio_exec` consent gate below) — tool discovery cached per server, a **per-tool
allowlist** (a disabled tool is rejected pre-dispatch, `Error{Policy}`), and typed `tools/call`
with per-server timeout + bounded retry (retried ONLY on a transport-level pre-dispatch failure —
never a blind re-invoke of a possibly side-effecting tool) and honest degradation on every
terminal failure. Every successful call persists a durable `mcp_artifact` row (`is_untrusted=1` —
the S6b agent-boundary mediation flag) that survives an orchd restart — proven by the e2e harness
(below). Cost/token fields on `mcp_invocation` are `Option` and populated only when the MCP
server itself reports usage — most tool results carry none, so the fields are honestly `null`
rather than a fabricated estimate.

**Connectors** are an OAuth-account layer, decoupled from MCP: **OAuth 2.1** (authorization-code +
PKCE via the `oauth2` crate, an SSRF-guarded token-exchange client — `redirect::Policy::none()` —
and refresh-on-expiry) or a static **api-key** account, tokens/keys always in Keychain via
`bpa-secrets`, `orchd.db` holding only the ref. One reference direct-API adapter ships —
`GenericRestAdapter` (`provider="generic-rest"`, `get`/`post` against an account-scoped URL with
the account's bearer) — proving the `ConnectorAdapter` trait seam without over-committing to a
specific social network's churn (a named adapter, e.g. X/LinkedIn, is backlog). `ConnectorInvoke`
routes through the identical trust-choke-point + invocation/artifact persistence path as
`McpCallTool` (same `is_untrusted=1` contract, same durable-artifact-across-restart guarantee).

**Trust layer (BL-22), a single pre-dispatch choke-point (`trust::authorize`) in `bpa-orchd`:**
- **connect consent:** first connect to an MCP server requires an owner-granted `consent_grant`
  (kind `connect`, fingerprint = URL); the URL changing re-prompts (fingerprint mismatch).
- **stdio-exec consent:** spawning a stdio server's local process requires a DISTINCT
  `stdio_exec` grant, fingerprinted by hashing the *resolved binary's bytes* (falls back to
  hashing the command string when the binary can't be resolved) — a binary swapped in place at
  the same path changes the fingerprint and re-prompts (a residual TOCTOU window between
  authorize and spawn is accepted-deferred, BL-68).
- **per-tool allowlist:** enforced pre-dispatch (above).
- **spend/rate policy caps:** a rolling 60 s window; the most-specific configured scope wins
  outright (server > project > global — never a per-field merge); spend caps bind ONLY when a
  call's cost is actually known (an unreported cost never trips a cap — honest, not silently
  permissive-by-omission when the server IS reporting cost).
- **untrusted tagging:** every `mcp_artifact`, from BOTH `McpCallTool` and `ConnectorInvoke`, is
  `is_untrusted=1` — the flag an eventual S6b agent-boundary mediation step will read; this slice
  sets and stores it, it does not yet mediate/feed it anywhere.
- **append-only audit log:** every connect / stdio-spawn / tool-call / connector-invoke / consent /
  policy-deny appends an `audit_log` row (`action`, `decision`, `reason` — reason is NEVER a
  secret or tool argument, only e.g. `"consent_required"`/`"spend_cap_exceeded"`).
- **`DYLD_*`/`LD_*` env denylist (closes BL-1):** a shared `bpa_daemon_core::env_filter` helper
  strips any `DYLD_*`/`LD_*`-prefixed key (case-sensitive) BEFORE a stdio MCP child's env is
  built; the SAME helper now also filters sessiond's `env_overrides` (previously applied
  unfiltered — the original BL-1 gap). A stdio child's env is `env_clear()`'d and built entirely
  by the caller (filtered orchd-ambient env merged with the DB's `server.env`, server wins on
  collision) — no ambient inheritance leak from either source.

**Skills:** a `SKILL.md`-format registry (portable, matches the Claude Code convention) — CRUD +
files-as-truth (`Present`/`Modified`/`Missing`, mirrors `ruleset_files.rs`'s pattern) + a
management tab. **Plumbing only** — there is no runtime consumer yet (the agent org that would
load and execute a skill is S6b); the UI states this honestly («Skills run once an orchestrator
agent exists (S6b) — for now it's a registry»), never presenting the registry as executable.

**UI — «Extensions»,** a new top-level view (alongside Home/Workspace/Project) with tabs:
Servers (MCP server registry + connect/consent), Tools (tool browser + per-tool allowlist +
invoke), Connectors (OAuth/api-key accounts + the generic-rest ops runner), Log (invocation
log + audit log + a spend/rate policy editor), Artifacts (durable results + an untrusted banner
per item), Skills (the skills registry). Every mutating control is `disabled` while `orchd://down`
(the established honest-degradation contract).

**e2e (`npm run e2e:orchd`):** phase 6 registers a local stub HTTP MCP server → grants connect
consent → connects (tools cached) → lists tools → calls the `echo` tool → asserts a durable
artifact → restarts orchd → asserts the artifact survived. Phase 7 does the connector-shaped
analogue — an api-key `generic-rest` account against a local stub REST target → `ConnectorInvoke`
→ artifact survives an orchd restart — but begins with a **Keychain-availability probe** (a
throwaway `ConnectorAddApiKey`) and gracefully, loudly SKIPs the phase (never a silent pass) on a
runner whose login keychain is locked/unavailable, so the gate stays honest in a headless CI
environment without ever masking a real failure when the keychain IS available.

**Deferred / explicitly out of scope this slice** (see `docs/backlog.md` for the filed rows):
MCP **sampling** (server→client LLM calls) — disabled, not advertised; MCP **resources**/
**prompts** — not surfaced; a named social direct-API adapter (X/LinkedIn) beyond the
`generic-rest` reference; active tool-result prompt-injection *mediation* (the tag is set now,
the agent-boundary consumer that reads it is S6b); a bulk MCP-server import (config file); a LIVE
`tools/list_changed` subscription (Phase-1's connect-per-call architecture holds no session open
between calls, so there is nothing to subscribe to yet — the tool cache instead refreshes
wholesale on every `McpConnect`, BL-70); a stdio child's stderr is currently inherited to orchd's
own log stream unredacted (BL-69). **BL-27** (Keychain access while the screen is LOCKED, for a
future *unattended* orchd run) is re-targeted to S6b/SW2 — this slice's flows are all
interactive/screen-unlocked and unaffected. Connecting the *real* prowl.chat server needs the
owner's own account/API key — the autonomous path (above) proves the identical mechanism against
a local stub; wiring a real provider is a documented, non-blocking Human step (owner adds the
server / pastes a key in the «Extensions» UI).

## Research pipeline — SHIPPED in S-IDEA (`[0.7.0]`)

S-IDEA (`docs/superpowers/specs/2026-07-15-s-idea-research-pipeline-design.md`) stitches the
already-shipped primitives — Idea/Insight/Task (S3), the knowledge graph (S4), and the MCP client
(S-EXT) — into ONE owner-driven loop: **idea → research → evaluated insight → task in the
backlog**, entirely inside `bpa-orchd`, **without the S6 agent org**. This is the first slice to
give `bpa-orchd` live runtime state that ISN'T just a durable SQLite row — a background research
run — so it's also the first slice to need a boot-reconcile step (below).

**`orchd.db` schema v4 (additive, `SCHEMA_VERSION` 3→4):** exactly ONE net-new table,
`research_run(id, idea_id, server_id, tool_name, args_json, status, invocation_id?, artifact_id?,
error_kind?, created_at, updated_at)` — `status` is `pending|running|done|failed`, CHECK-enforced
`(status='done') = (artifact_id IS NOT NULL)`. **The "ResearchArtifact" the roadmap named is NOT a
separate table or blob store** (a correction the S-IDEA docs-truth pass makes explicit, per the S3
overview's entity-map row): it is the REUSED S-EXT `mcp_artifact` row the run's tool call produces
— `research_run` is purely a provenance link (idea ↔ MCP invocation ↔ artifact) + a status machine,
no blob duplication, one source of truth for durable research output (spec D2).

**The async run driver — `bpa-orchd`'s FIRST long-lived `tokio::spawn`:**
`research::start_run` (`crates/orchd/src/research/mod.rs`) does the insert (`research_run{pending}`
+ the idea's `captured→researching` lifecycle flip, ONE `unchecked_transaction()`) and returns
immediately after spawning a detached background task. That task follows the SAME 3-phase-locking
discipline the S-EXT `mcp::invoke` path already proved out (never holds the DB mutex across a
network `.await`): lock → mark `running` → push `ResearchRunsChanged` → unlock → call the SHIPPED
`mcp::invoke::call_tool` (unlocked) → lock → a SINGLE `UPDATE` to `done{artifact_id,invocation_id}`
or `failed{error_kind}` → push `ResearchRunsChanged` → unlock. Each transition is one `UPDATE`
statement, never two separate writes, so the schema's `CHECK` invariant can never observe a
half-completed transition. `error_kind` is a typed classification only (`policy_cap_exceeded` |
`timeout` | `tool_error` | `transport` | `interrupted`) — never the tool's args, a secret, or its
output. The frontend's "research pane" is driven entirely by the `ResearchRunsChanged` push, not
polling.

**Boot-reconcile (D11) — the crash/restart safety net a detached background task otherwise
lacks:** the spawned run task is NOT tracked by `socket_server`'s shutdown-drain `JoinSet` (the
same one `OrchdShutdown{drain}` awaits for in-flight connections), so a daemon restart, upgrade, or
crash while a run is `pending`/`running` would leave that row stuck non-terminal forever with no
one left to finish it. The fix is a boot step, `Db::reconcile_interrupted_research_runs`, run right
after `open_db` in `boot::run` — the same "ensured at every boot" shape `ensure_global_ruleset`
already established: `UPDATE research_run SET status='failed', error_kind='interrupted', … WHERE
status IN ('pending','running')`. Any run not `done`/`failed` at boot is stale by construction (the
process that was running it is gone) — the owner re-runs. This is the AUTHORITATIVE guarantee, not
a nice-to-have: proven by a dedicated e2e phase (below) that starts a run against a server whose
tool call deliberately never returns, shuts the daemon down while the run is still `running`, and
asserts the reconcile fires on the next boot.

**Connect-handshake timeout (D12) — a hang-forever fix in the shipped S-EXT invoke path, not
research-specific:** `mcp::invoke::call_tool` previously wrapped only the `tools/call` RPC itself
in `timeout(server.timeout_ms)`, not the preceding `connect_fn(...).await` (the MCP `initialize`
round-trip). A peer that accepts the TCP/stdio connection but never completes `initialize` (a dead
peer, a silent firewall drop, an overloaded stdio child) hung the calling task forever — invisible
to any research-specific code, because the bug lived one layer down in the shared S-EXT path. Now
the connect handshake is bounded by the same per-server `timeout_ms` as the call
(`McpError::Timeout` on elapse) — every MCP call benefits, not just research; proven by
`mcp::invoke::tests::call_tool_connect_that_never_resolves_times_out_not_hangs`.

**Wire protocol — three verbs, append-only:** `ResearchStartRun{idea_id, server_id, tool_name,
args_json} -> ResearchRun` (creates `pending`, spawns the task, replies immediately — the terminal
state arrives via the push); `ResearchListRuns{idea_id} -> Vec<ResearchRun>`; `ResearchGetRun{id}
-> ResearchRun` — appended at the END of `bpa-orchd-proto`'s frozen `OrchdRequest`/`OrchdResponse`
enums, plus `OrchdPush::ResearchRunsChanged{idea_id: Option<String>}` → frontend event
`orchd://research-runs-changed`. The orchd wire version space stays `[1,1]` (additive, same
discipline S-EXT's Mcp*/Connector*/Skill*/Trust* append used). Everything else the flow needs —
spawn-project (`CreateWorkspace`+`CreateProject`+`SetIdeaProject`), insight formation
(`CreateInsight`+`SetInsightFitVerdict`+`SetInsightStatus`), task formation
(`CreateTask{source:Insight}`), spend-preflight (`TrustListPolicies`) — reuses SHIPPED S3/S4/S-EXT
verbs untouched; the three `Research*` verbs are the ONLY net-new wire this slice adds.

**Graph-ingest on insight-accept (D9):** accepting a research-formed insight
(`SetInsightStatus{accepted}`) now additionally seeds one `entity_ref` graph node for that insight
via the existing `add_entity_ref_node` (S4) — new wiring inside the shipped `set_insight_status`
handler, not a new verb. A re-accept after archive hits the graph's own partial-unique-index
`Conflict`, handled as a benign no-op (still exactly one node per insight).

**Owner-driven fit-verdict — deliberately NOT agent-computed (D4, overrides the roadmap's Q10
default):** S6a (the native LLM provider layer) is not built, and the S-IDEA DoD itself requires
the loop to work "WITHOUT the S6 agent org" — S6a is a member of that org. `Insight.fit_verdict`/
`fit_reasoning`/`status`/`resolution_reasoning` (all S3 fields, unchanged shape) are set by the
OWNER, shown beside a fit-context panel — the project's goals (with `metric_refs`) plus a
`GraphNeighborhood` read rooted at the idea/insight. LLM-computed auto-scoring is filed to backlog
for S6a (`docs/backlog.md`), never silently claimed as shipped.

**Frontend — the idea→research→insight→task flow** (`src/components/idea/`): `ResearchRunDialog`
(pick a connected MCP server → `McpListTools` → pick a tool → owner-supplied args JSON → a
spend-approval preflight reusing `TrustListPolicies`, with an honest "cost usually unknown until
after the call" note — the trust layer's existing hard caps are UNCHANGED, a breach at invoke time
surfaces as `failed{policy_cap_exceeded}`, Q8 honest degradation) → `ResearchPane` (per-idea run
list by status; a `done` run reuses the S-EXT artifact viewer + «unverified data» untrusted
banner; **not token-streaming** — MCP `tools/call` is request/response in the shipped
connect-per-call model, so v1 shows run status, not streamed tokens, an honest scope line stated in
the pane itself, not a partial build; a `failed` run offers «form insight without research» so
the owner path never dead-ends) → `FormInsightDialog` (title/body prefilled from the artifact, the
fit-context panel above, owner sets fit-verdict, creates the insight) → «Accept»/«To backlog» forms
the task and flips the idea `researching→specced`. `SpawnProjectFromIdea` closes BL-56 (the
spawn-project-from-idea UI flow S3 shipped only the data enabler for, `Idea.project_id`) — pure
frontend orchestration over the three existing verbs above, no new orchd verb. Every mutating
control is `disabled` while `orchd://down`, the same discipline every prior slice's UI holds to.

**e2e (`npm run e2e:orchd`):** phase 8 registers a local stub MCP research server, drives
`CreateIdea → ResearchStartRun → poll ResearchGetRun until done → CreateInsight +
SetInsightFitVerdict{fit} → SetInsightStatus{accepted} → CreateTask{source:insight}`, restarts the
daemon, and asserts the idea (`specced`), the run (`done`+artifact), the insight (`fit`+accepted),
and the task all survive — the roadmap DoD proof. Phase 9 registers a BLOCKING stub (a tool call
that never returns), starts a run, polls until it's `running` (deliberately not `done`), shuts the
daemon down mid-flight, relaunches, and asserts the run reconciled to `failed{interrupted}` — the
D11 boot-reconcile proof, exercising exactly the in-flight-at-restart race phase 8 avoids by
design.

**Deferred / explicitly out of scope this slice** (see `docs/backlog.md` for the filed rows):
LLM-computed fit-verdict (S6a, D4 above); a token-streaming research pane (needs a persistent MCP
session, aligns with the S-EXT `list_changed` item BL-70); automated agent task-decomposition
(S6b — v1 decomposition is owner-created subtasks via `CreateTask{parent_id}`); a first-class
ResearchArtifact provenance viewer beyond the reused `mcp_artifact` viewer; research-run
cancel/retry controls; real metric timeseries for fit-context (S8 — today `metric_refs` renders as
owner-declared strings only); a prowl-aware convenience adapter that auto-seeds a `session_id` into
a run's `args_json` (Q5 v1-override, D13 — v1 does not hardcode a prowl-specific schema, the owner
supplies `session_id` like any other arg if the picked tool's schema wants one); `JoinHandle`
drain-tracking so `OrchdShutdown{drain}` could best-effort await an in-flight run (D11 nice-to-have
— boot-reconcile is the correctness backstop regardless, so this is a latency polish item, not a
correctness gap).

## Storage-degradation mode + per-request tracing — SHIPPED in S-POLISH (P1)

S-POLISH P1 (`docs/superpowers/plans/2026-07-16-s-polish.md`) is a backend-only reliability +
observability slice — no frontend files, no wire version bump (orchd stays `[1,1]`, append-only).
Two cross-cutting mechanisms land here.

**Storage-degradation mode on the wire (BL-94, spec D3):** `bpa-orchd` already degraded honestly to
an in-memory DB when its disk was unusable and quarantined a corrupt on-disk image aside — but the
resulting mode was invisible to the GUI, which kept telling the owner their data was durable. P1
plumbs that boot fact all the way to the frontend:

- `persistence.rs` gains `Db::open_with_outcome(path) -> Result<(Db, DbOpenOutcome)>` where
  `DbOpenOutcome` is `Clean` or `RecoveredFromCorruption { quarantined_to }`; the existing
  `Db::open` delegates to it and discards the outcome (no behavior change for existing callers).
- `boot::open_db_degrading` maps that outcome (plus the in-memory fallback path) to a
  `StorageStatus { storage_mode, quarantined_path }` — `StorageMode` is `Persistent` /
  `RecoveredFromCorruption` (with the quarantine path) / `InMemoryFallback` — stored in
  `ServerDeps.storage_status` at boot.
- A new append-only wire verb `GetStorageStatus -> OrchdResponse::StorageStatus(StorageStatus)`
  (`crates/orchd-proto/src/lib.rs`; entity + enum are ts-rs-exported, the frame variants are plain
  snake_case per the wire-layering rule) returns `deps.storage_status.clone()` verbatim from the
  dispatch arm — a pure read that broadcasts nothing, since the mode is fixed at boot and only a
  restart can change it.
- The Tauri core exposes it as the `orchd_storage_status` command
  (`src-tauri/src/commands.rs`, `Error → Daemon` mapped like its siblings, registered in
  `lib.rs`'s `generate_handler!`); the GUI pulls it once on connect and on every reconnect (no push)
  to drive an honest banner for the two non-persistent modes. The operational meaning of each mode
  and where the quarantined corrupt DB lands are in `docs/runbook-orchd.md` ("Storage-degradation
  modes"). The frontend banner itself is P3 work (BL-94 frontend); P1 ships only the backend + wire.

**Per-request completion tracing — one choke-point per dispatch layer (O-6, spec D4):** structured
observability without touching a single per-verb handler. Each dispatch layer wraps its dispatch
call ONCE and emits a single completion line carrying a low-cardinality quartet —
`verb` / `outcome` / `error_code` (present only on an error) / `elapsed_ms`:

- `crates/orchd/src/socket_server.rs` — a `dispatch` wrapper around `dispatch_inner`, using an
  exhaustive `OrchdRequest::verb_name()` (a wildcard-free match — a new wire verb fails to compile
  until it is named, so the trace can never silently mislabel a verb).
- `crates/sessiond/src/socket_server.rs` — the identical wrapper over `Request::verb_name()`.
- `src-tauri/src/orchd_client.rs` — the core's own `request` method emits the same quartet (reusing
  the daemon's `verb_name`), so a request can be followed end-to-end across Hop-B by the same field
  names on both the core and the daemon side.

The line NEVER carries args, bodies, tokens, tool output, ids, or PII — only the quartet — enforced
by the extended `crates/orchd/tests/no_secrets_in_logs*.rs` secret-scan tests. Because it is one
wrapper per layer rather than a per-arm edit, adding a verb needs no tracing change; the exhaustive
`verb_name` match is the only place a new verb touches. Field-level operational detail is in
`docs/runbook-orchd.md` ("Per-request tracing fields").

## Tier-2 feature completeness — SHIPPED in S-POLISH (P4)

S-POLISH P4 (`docs/superpowers/plans/2026-07-16-s-polish.md`) closes four Tier-2 gaps the earlier
slices had deliberately left half-built. All four are ADDITIVE — three new wire verbs appended at
the TAIL of `bpa-orchd-proto`'s frozen enums (the orchd version space stays `[1,1]`, same
append-only discipline S-EXT/S-IDEA used) and one pure-frontend feature that reuses a shipped verb.
No schema migration: none of the four adds or alters a column.

**Project un-archive (O-3, closes BL-53):** S3 shipped `ArchiveProject` with no inverse — an
archived project was a one-way trap. P4 adds the exact reverse, `Db::unarchive_project`
(`crates/orchd/src/persistence.rs`) + the append-only wire verb `UnarchiveProject { id } ->
OrchdResponse::Project` that flips `archived → active` and pushes `ProjectsChanged`. Its guards
MIRROR `ArchiveProject`'s: an unknown `id` ⇒ `NotFound`; an already-`active` project ⇒ `Invariant`
(nothing to un-archive — the mirror of `ArchiveProject`'s already-archived `Invariant`). The Tauri
core exposes it as `orchd_unarchive_project`. Frontend controls (an «Archived (N)» collapsed group
in the sidebar, an archived-project read-only banner + «Un-archive» button) are in
`docs/frontend-conventions.md`.

**Graph edge-kind editing + node/rename editor (O-7):** S4's graph canvas could add and delete
edges but never RE-KIND an existing one, and had no node title/body form or rename. P4 adds the
append-only wire verb `GraphUpdateEdge { id, kind } -> OrchdResponse::GraphEdge`
(`crates/orchd/src/graph.rs`) — an edge's rendered "label" IS its `kind`, so changing the kind is
the whole edit; there is no separate label column and therefore **no v5 migration**. It pushes
`GraphChanged` for BOTH endpoint projects (a cross-project edge re-kind invalidates both sides,
same fan-out rule as `GraphAddEdge`); an unknown `id` ⇒ `NotFound`; an archived endpoint project ⇒
`Invariant` — guards mirroring `GraphAddEdge`. Node add (title/body form) and inline rename reuse
the ALREADY-shipped S4 `GraphAddNode`/`GraphUpdateNode` verbs (no new wire for those two); the
editor UI is in `docs/frontend-conventions.md`. The Tauri core exposes `orchd_graph_update_edge`.

**Config-backed OAuth provider registry (O-5):** before P4, `bpa-orchd` booted with an EMPTY OAuth
provider registry, so every `ConnectorBeginOAuth` failed with `UnknownProvider` and the BL-91
token-exchange timeout (P1) sat on an unreachable path. P4 adds the "real IdP config" seam: at boot
(`boot::run`) `connectors/registry_config.rs::load_oauth_providers` reads an OPTIONAL
`<app-support>/oauth_providers.json` and registers every provider it declares into
`ConnectorsState`'s in-memory registry — activating the P1 timeout on a now-reachable path. Honest
degradation matches `boot::open_db_degrading`/`ensure_global_ruleset`: a MISSING file is the normal
default (info-logged, empty registry, boot proceeds); a MALFORMED file (bad JSON, a missing
required field, or a typo'd key — `deny_unknown_fields`) is error-logged and leaves the registry
empty; NEITHER case blocks boot. A new append-only read verb `ConnectorListProviders ->
OrchdResponse::ConnectorProviders` returns the provider NAMES ONLY — a provider's
`client_id`/`client_secret`/endpoint URLs NEVER cross the wire, and `client_secret` lives only in
memory, never in `orchd.db` and never in a log (the boot line logs only the provider count +
names). The Tauri core exposes `orchd_connector_list_providers`; the UI's provider dropdown +
honest empty-state is in `docs/frontend-conventions.md`. File format, shape, and the degradation
table are in `docs/runbook-orchd.md` ("OAuth provider registry — `oauth_providers.json`").

**`metric_refs` owner editor (O-4) — pure frontend, no new wire:** the `Goal.metric_refs` field
and the `UpdateGoal` verb that carries it BOTH already shipped in S3; P4 only builds the missing
owner-facing editor (a chip editor on each goal row, `src/components/GoalTree.tsx`), which persists
the row's full next `metric_refs` array through the existing `orchd_update_goal` command — no
schema change, no new verb, no backend change at all. Detail is in
`docs/frontend-conventions.md`.

## Frontend design system + diagnostics — SHIPPED in S-UXR / S-DIAG / S-DESIGN (`[0.9.x]`)

Frontend-only slices — no wire verb, no schema migration (orchd stays `[1,1]`); the ~925 vitest
tests are the behavior-preservation guarantee.

- **Design tokens + primitives (`src/ui/`, `[0.9.0]`).** A single CSS-variable layer
  (`tokens.css`) defines a light **and** dark palette (neutral slate, one calm-blue accent, `ok`/
  `warn`/`danger`/`info` tones + `-weak` fills) plus space/radius/type/shadow scales; `theme.ts`
  resolves `light`/`dark`/`system` and stamps `data-theme` on the root (FOUC-free at boot).
  `primitives.tsx` is a token-only kit (Panel/Stat/Sparkline/Badge/Button/Field/EmptyState/Dialog)
  every view consumes — no raw hex in the component tree. The legacy static dark-only palette was
  retired. Palette legibility is a VERIFIED invariant: `contrast.ts` + `contrast.test.ts` assert
  every ink/tone/on-accent text pair clears WCAG AA in both themes (`[0.9.1]`, S-DESIGN).
- **UX-scenario base (`docs/qa/`, `[0.9.0]`).** 181 first-session scenarios across 15 epics, a
  maintenance rule (CONTRIBUTING), an advisory CI gate (`check-ux-scenarios.sh`), and a code-traced
  audit-results file — the regression map the redesign was checked against.
- **Diagnostics — reconstructable failures (`src/ipc/diag.ts` + store, `[0.9.1]`, S-DIAG).** Errors
  were previously a 4 s toast and then lost, and a render crash was a white screen. Now
  `store.reportError(op, e)` classifies the error, scrubs secrets (Bearer/token/key/app-password/
  home-dir), records a bounded (200) newest-first ring, `console.error`s a breadcrumb, and toasts —
  every `refresh*` failure routes through it. An `ErrorBoundary` around `<App/>` records render
  crashes and shows a recovery card; a `DiagnosticsPanel` (sidebar footer, with an error-count
  badge) lists the ring and copies a secret-scrubbed support bundle.
- **Boot-race fix (`setup()`, `[0.9.2]`, BL-101).** `AppState` is now `manage`d SYNCHRONOUSLY in
  the Tauri `setup()` closure — the client/status slots + write-lock map are pre-created, both
  launchd agents resolved, and `AppState` registered BEFORE either async bring-up task is spawned.
  Previously the happy-path `manage()` ran inside `bring_up_daemon`, *after* an up-to-~4 s bounded
  connect, so a command fired from the first webview frame hit an unmanaged state and returned the
  raw Tauri "state not managed" error. Now such a command extracts a managed `AppState` and returns
  an honest `Disconnected` (→ the orchestrator-unavailable banner + reconnect, self-healing on
  `orchd://up`). The diagnostics log above is what surfaced this on a live install.

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
- `docs/superpowers/specs/2026-07-14-s4-knowledge-graph-design.md` — the locked S4 spec (knowledge
  graph + workspace-wide retrieval API) the "Knowledge graph" section above summarizes.
- `docs/superpowers/specs/2026-07-15-s-ext-mcp-connectors-design.md` — the locked S-EXT spec (MCP
  client, connectors, skills, trust layer) the "Extensions" section above summarizes.
- `docs/superpowers/specs/2026-07-15-s-idea-research-pipeline-design.md` — the locked S-IDEA spec
  (research pipeline, async run driver, boot-reconcile) the "Research pipeline" section above
  summarizes.
- `tests/e2e/README.md` — the three ways to exercise the survive-restart property (socket harness,
  launchd-managed variant, full-GUI manual/CI confirmation).
