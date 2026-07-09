# Builder Pro AI — S0+S1 Spec: Foundation + Terminal Core

**Date:** 2026-07-01
**Status:** Approved design → locked contracts (ready for `writing-plans`). v2 (post adversarial review).
**Slice:** S0 (app foundation) + S1 (terminal engine), built together.
**Parent:** `2026-07-01-builderpro-platform-overview.md`

> This spec locks **shared contracts** (versions, types, wire protocol, IPC signatures, DB schema,
> file layout) so a zero-context implementer and parallel subagents agree. External-library facts
> were verified against current docs (Context7 + web, 2026-06/07) and hardened by an adversarial
> completeness review. Re-verify pins at plan time (§15) before writing code.

## Post-implementation amendments (2026-07-04)

The S0+S1 implementation (merged @ 285cb2e) diverged from this spec in specific,
audited ways. The sections below were amended IN PLACE so this document remains the
single executable truth for zero-context implementers. Full audit:
`docs/superpowers/research/2026-07-03-docs-spec-audit.json`.

| Amendment | Sections | Finding |
|-----------|----------|---------|
| Codec truth: dual-codec bridge + never-re-derive contract | §3, §5, §7 | A2 |
| §15 verify-at-lock resolution table | §15 | A2/A30 |
| Persistence truth: 1000 ms cadence, no size trigger, seq-0 semantics, 256 KiB cap | §11 | A24 |
| Attach/reconnect + error-surfacing contracts | §12, §13 | A22/A23 |
| Trust model rewrite: agent trust layer, keys, CSP, data-at-rest, env_overrides | §16 | A7/A9/A11/A18/A19 |
| Resource model honest posture + planned cap | §9, §13 | A21 |
| Survival truth wording | §13 | A12 |

## Post-implementation amendments (2026-07-07, Protocol v2 / Pv2, `[0.2.0]`)

Protocol v2 (`docs/superpowers/specs/2026-07-06-protocol-v2-design.md`) shipped in this cycle,
superseding the codec and attach-model sections below. Amended IN PLACE per the same convention;
see the Pv2 design doc for the full contract and `CHANGELOG.md` `[0.2.0]` for the shipped summary.

| Amendment | Sections | Finding |
|-----------|----------|---------|
| Codec: bincode dual-codec bridge → CBOR (ciborium) plain derives; never-re-derive box retired | §3, §5, §7 | Pv2 §3 |
| Handshake: `Hello`/`Welcome`/`MAGIC`/`PROTO_VERSION` frames → codec-agnostic preamble + version-range negotiation | §7 | Pv2 §4 |
| Attach model: single-attach-supersede → multi-subscriber attach registry keyed `(session, conn)` | §4 (file tree), §7 | Pv2 §5 |
| `DaemonShutdown{drain}`: reserved no-op → real flush + graceful exit | §7, §13 | Pv2 §6.1 |
| Rehydrate: cold-rehydrate at boot as inactive replay-only entries; attach-inactive replays scrollback (closes BL-7) | §11 | Pv2 §9.8 (execution: Task 12r) |
| Schema v2: `command_events` table + `origin` attribution column | §11 | Pv2 §7 |

---

## 1. Goals / non-goals

**Goals (ship, production-grade):**

1. Tauri 2 app shell (universal macOS binary): window, theme, settings, local persistence.
2. A **detached session daemon** (`bpa-sessiond`) that owns real PTYs and **survives GUI
   restart/crash**; the GUI reattaches and replays scrollback.
3. Create / attach / detach / write / resize / kill multiple terminals, each rooted in a
   **workspace folder**.
4. **OSC-133 shell integration** → per-terminal lifecycle status and a **waiting-for-input** heuristic.
5. A **programmatic terminal API** (the Tauri command surface) that S6 agents will later drive.
6. TDD tests, full error handling + honest degradation, structured logging (no secret leakage), docs.

**Non-goals (deferred):** file explorer (S2), project/goals/graph/kanban (S3–S5), agents (S6), stats
(S7), analytics (S8), Windows/Linux packaging (code stays cross-platform-friendly; only macOS is
shipped/tested), cross-logout session survival (needs a root LaunchDaemon).

**Definition of Done:** §14.

---

## 2. Architecture

Two processes, two IPC hops. The **daemon owns all PTYs** so the GUI can die without killing
sessions (tmux/retach model).

```
┌──────────────────────── Builder Pro AI.app ────────────────────────┐
│  React webview (UI)                Rust core (broker)               │
│  • xterm panes                     • #[tauri::command] surface      │
│  • workspace sidebar      ◄──Hop A──►  • UDS client to daemon       │
│  • status dots             Tauri IPC   • maps daemon frames ⇄ UI    │
│  • Zustand (metadata only)         • app settings (tauri-plugin-store)│
└───────────────────────────────────│────────────────────────────────┘
                                     │ Hop B: Unix domain socket
                                     │ (preamble handshake, then u32-LE length prefix + CBOR Frame
                                     │  — Pv2 `[0.2.0]`; was bincode Frame as originally built)
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

**Load-bearing decisions (and why):**

- **launchd owns the daemon lifecycle, not Tauri.** The naive Tauri pattern (spawn sidecar in
  `.setup()`, `child.kill()` on exit) makes the daemon an app child → kills sessions on app close,
  defeating survive-restart. Instead: Tauri **bundles** the daemon (`externalBin`, so it's
  signed + lipo'd), and a per-user **LaunchAgent** supervises it. The GUI connects only over the
  socket and never holds the daemon's process handle.
- **The daemon is the single source of truth** for all durable domain state (workspaces, sessions,
  scrollback) via `rusqlite`. The core does **not** use a SQL plugin; it persists only UI settings.
- **Webview never touches the socket.** The Rust core brokers Hop B (keeps WKWebView out of raw
  socket access; centralizes validation + peer-cred checks).
- **The command surface IS the agent control API.** `create/attach/write_stdin/resize/kill/
  get_session_state` are exactly what S6 agents will call.

---

## 3. Locked versions

| Layer | Package | Version |
|---|---|---|
| Core | `tauri` (crate) + `@tauri-apps/cli` + `tauri-cli` | 2.11.x (pin `^2`, record 2.11.4) |
| Core | `@tauri-apps/api` | 2.11.x (`^2`) |
| Plugin | `tauri-plugin-store` + `@tauri-apps/plugin-store` | major `2` — **UI settings only** |
| Plugin | `tauri-plugin-dialog` + `@tauri-apps/plugin-dialog` | major `2` — folder picker |
| Plugin | `tauri-plugin-fs` + `@tauri-apps/plugin-fs` | major `2` — scoped (used from S2; workspace-root read now) |
| Plugin | `tauri-plugin-shell` + `@tauri-apps/plugin-shell` | major `2` — **bundling only** |
| Toolchain | Rust | ≥ **1.88.0** *(amended 2026-07-09, S2 docs-truth pass: originally locked 1.77.2 here; the real floor of the resolved `Cargo.lock` dep graph — `plist`/`time`/`darling`/`serde_with`, transitively via `tauri`, pre-dating S2 — is 1.88.0, higher than the S2-added `trash` crate's own 1.85.0. `rust-toolchain.toml` pins 1.92, so this was never a build-breaking gap in practice — only a false floor claim. See `CHANGELOG.md` `[0.3.0]`.)* |
| PTY | `portable-pty` (wezterm) | **0.9.0** |
| VT (live grid state) | `alacritty_terminal` | pin exact (0.24/0.25) — used for cursor/alt-screen/size, **not** serialized |
| DB | `rusqlite` (bundled sqlite) | 0.32 (≥0.31) |
| Codec | `ciborium` (CBOR, RFC 8949) | current — superseded 2026-07-07, Pv2 `[0.2.0]`; was `bincode` **1.3.3** (fixint LE framing; tagged enums via hand-written dual-codec impls) as originally locked here — see the codec-decision box below |
| Async | `tokio` | 1.x (`net, io-util, rt-multi-thread, macros, sync, time`) |
| Lock/creds | `rustix` (flock, `getsockopt`/peer creds) | current |
| Detach fallback | `daemonize` (dev/non-launchd only, feature-flagged) | 0.5 |
| Logging | `tracing` + `tracing-subscriber` | current |
| IDs | `uuid` (v4) | current |
| UI | React | **19.x** |
| UI | Vite | 6/7-class |
| UI | TypeScript | 5.x |
| UI | Zustand | **5.x** |
| Term | `@xterm/xterm` | **6.0.0** (`^6`) — scoped pkg only, never legacy `xterm` |
| Term addons | `@xterm/addon-fit` / `-webgl` / `-search` / `-web-links` / `-serialize` | 0.11 / 0.19 / 0.15 / 0.11 / 0.13 |
| Type parity | `ts-rs` (Rust→TS type gen for `crates/protocol`) | current |

**Codec decision (superseded 2026-07-07, Pv2 `[0.2.0]` — was amended 2026-07-04, A2, for the
original bincode 1.3.3 dual-codec bridge described below).** Hop-B codec is **CBOR** (`ciborium`);
tagged enums are plain `#[derive(Serialize, Deserialize)]`. The v1 dual-codec bridge — bincode
1.3.3 could not deserialize serde internally/adjacently-tagged enums, so `SessionLifecycle` and
`TerminalEvent` shipped hand-written `Serialize`/`Deserialize` impls branching on
`is_human_readable()` (JSON/ts-rs saw the tagged shape; bincode tunneled a JSON string inside the
binary frame) — was retired in Pv2. See `docs/superpowers/specs/2026-07-06-protocol-v2-design.md`
§3 for the migration design and `crates/protocol/src/lib.rs` for the current plain derives.

The append-only wire-discipline rule (enum variant order frozen; new requests/pushes appended;
fields added additively; every protocol change ships a cross-version decode test) REMAINS in
force — see the Pv2.1 reserved-batch amendment (2026-07-06-protocol-v2-design.md, "Vision v2–v4
amendments") for how future request variants are named and order-reserved without another wire
break. Only the now-false "never re-derive Serialize/Deserialize" box (below, historical) is
retired: CBOR's self-describing encoding removed the reason for the ban.

> *(Historical, pre-Pv2 contract — no longer in force:* DO NOT re-derive Serialize/Deserialize on
> SessionLifecycle or TerminalEvent, and DO NOT add new serde-tagged enums to the Hop-B protocol,
> until protocol v2 replaces the codec. *Protocol v2 shipped in `[0.2.0]`; this box is retired.)*

Renderer chain is **WebGL → DOM** (guaranteed); do not depend on a canvas addon.

---

## 4. Repository & module layout (file-ownership map for parallel tasks)

```
builder-pro-ai/
├─ package.json  vite.config.ts  tsconfig.json  index.html
├─ src/                              # React frontend
│  ├─ main.tsx  App.tsx  theme.ts
│  ├─ ipc/
│  │  ├─ types.ts     # SHARED TS types (generated from crates/protocol via ts-rs) — DO NOT hand-edit
│  │  ├─ commands.ts  # typed invoke() wrappers
│  │  ├─ channel.ts   # attach_session Channel<TerminalEvent> plumbing
│  │  └─ events.ts    # global emit/listen subscriptions
│  ├─ store/          # store.ts — Zustand: sessions/workspaces metadata ONLY (no bytes)
│  ├─ terminal/       # terminal-manager.ts (non-reactive Map<SessionId, Terminal>)
│  └─ components/     # TerminalPane, TerminalTabs, WorkspaceSidebar, StatusDot, DaemonBanner
├─ src-tauri/                        # Tauri core (the broker)
│  ├─ Cargo.toml  tauri.conf.json  build.rs
│  ├─ capabilities/default.json
│  ├─ binaries/                      # bpa-sessiond-<triple> (built from crates/sessiond)
│  ├─ entitlements.plist  icons/
│  └─ src/
│     ├─ main.rs  lib.rs             # Tauri Builder, plugin init, setup
│     ├─ commands.rs                 # #[tauri::command] surface (Hop A)
│     ├─ broker.rs                   # daemon frames → Channel/global events; Promise correlation
│     ├─ socket_client.rs            # connect/handshake/reconnect (Hop B client)
│     ├─ paths.rs                    # workspace-root/cwd canonicalize + validate
│     └─ launchd.rs                  # install/bootstrap/kickstart LaunchAgent
├─ crates/
│  ├─ protocol/        # SHARED Hop-B wire types (serde + ts-rs) — source of truth for TS types
│  │  └─ src/lib.rs
│  └─ sessiond/        # the daemon binary
│     └─ src/
│        ├─ main.rs
│        ├─ socket_server.rs         # tokio UnixListener; per-client task; peer-cred check; bounded outq
│        ├─ pty_supervisor.rs        # portable-pty lifecycle, threads, process-group kill
│        ├─ live_grid.rs             # alacritty_terminal Term: cursor/alt-screen/size (status source)
│        ├─ scrollback.rs            # sanitized raw-byte ring (replay source) + prune
│        ├─ osc_parser.rs            # OSC-133/OSC-7 streaming tokenizer + lifecycle state machine
│        ├─ shell_integration/       # zsh + bash injection assets + installer
│        ├─ persistence.rs           # rusqlite (WAL), schema, migrations, degradation, rehydrate
│        ├─ attach.rs                # multi-subscriber attach registry + replay (Pv2 §5)
│        └─ singleton.rs             # flock single-instance + socket path resolution + perms
├─ docs/superpowers/{specs,plans}/
└─ .gitignore  README.md
```

**Parallel-safety:** `crates/protocol` is written first (sequential); TS `src/ipc/types.ts` is
**generated** from it (ts-rs) and owned by no task. Daemon / core / frontend module files are
disjoint → parallelizable after the protocol + generated types land.

---

## 5. Shared types (authoritative — Rust ⇄ TS)

`crates/protocol` derives `Serialize, Deserialize` (serde) **and** `ts-rs::TS`; `src/ipc/types.ts` is
generated from it (CI checks it is in sync — §14). All payloads use `rename_all = "camelCase"`. IDs
are UUID v4 strings.

```rust
pub type SessionId = String;   // UUID v4
pub type WorkspaceId = String; // UUID v4

#[derive(Serialize, Deserialize, Clone, Debug, TS)]
#[serde(rename_all = "camelCase")]
pub struct Workspace { pub id: WorkspaceId, pub name: String, pub root_path: String }

// Internally tagged (tag only, no content) — works for unit + struct variants, matches TS below.
// CURRENT (Pv2, [0.2.0]): this IS the real derive now — CBOR is self-describing, so the plain
// #[derive] below is exactly what ships in crates/protocol/src/lib.rs. (Historical: pre-Pv2, this
// derive was only the LOGICAL shape ts-rs exported; the real impls were hand-written, dual-codec,
// §3 — that bridge is retired.)
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum SessionLifecycle {
    AtPrompt,                                    // idle at shell prompt (after OSC 133 B, before C)
    Typing,                                      // NEVER emitted in S1; UI maps to AtPrompt color
    Running,                                     // command executing (after C, before D)
    Exited { code: Option<u8>, signal: Option<String> }, // finished; code None = unknown/aborted
}

#[derive(Serialize, Deserialize, Clone, Debug, TS)]
#[serde(rename_all = "camelCase")]
pub struct SessionMeta {
    pub id: SessionId,
    pub workspace_id: WorkspaceId,
    pub title: String,
    pub shell: String,
    pub cwd: String,
    pub cols: u16,
    pub rows: u16,
    pub lifecycle: SessionLifecycle,   // carries exit code + signal when Exited
    pub waiting_for_input: bool,
    pub is_active: bool,               // PTY child alive?
    pub created_at: i64,               // unix seconds
}
```

**TS mirror (generated; shown for reference):**

```ts
export type SessionId = string; export type WorkspaceId = string;
export interface Workspace { id: WorkspaceId; name: string; rootPath: string }
export type SessionLifecycle =
  | { kind: "atPrompt" } | { kind: "typing" } | { kind: "running" }
  | { kind: "exited"; code: number | null; signal: string | null };
export interface SessionMeta {
  id: SessionId; workspaceId: WorkspaceId; title: string; shell: string; cwd: string;
  cols: number; rows: number; lifecycle: SessionLifecycle;
  waitingForInput: boolean; isActive: boolean; createdAt: number;
}
// Hop-A Channel payload (see §6.2). Vec<u8> serializes over Tauri IPC as number[].
export type TerminalEvent =
  | { event: "replay"; data: { cols: number; rows: number; content: number[] } }
  | { event: "output"; data: { bytes: number[] } };
```

**Exit-code note:** POSIX exit codes are 8-bit; `portable-pty ExitStatus::exit_code()` returns `u32`
and is masked to `u8` (`(c & 0xff) as u8`). Signal-terminated children carry `code = None` and the
signal name in `signal`.

---

## 6. Hop A — webview ⇄ core (Tauri IPC)

**Split by frequency:** firehose on `Channel`; everything else on commands + the global event bus.

### 6.1 Command classification

| Command | Served by | Notes |
|---|---|---|
| `pick_folder()` | **CORE-ONLY** (`tauri-plugin-dialog`) | native dialog must run in the GUI process |
| all others below | **DAEMON-BROKERED** | core forwards to daemon; daemon SQLite is source of truth |

```ts
create_session(workspaceId, opts?: { shell?: string; cwd?: string;
               envOverrides?: [string,string][]; cols?: number; rows?: number }): Promise<SessionMeta>
list_sessions(): Promise<SessionMeta[]>
attach_session(sessionId, onEvent: Channel<TerminalEvent>): Promise<void>
detach_session(sessionId): Promise<void>
write_stdin(sessionId, data: string): Promise<void>
resize(sessionId, cols, rows): Promise<void>
kill_session(sessionId): Promise<void>
list_workspaces(): Promise<Workspace[]>
create_workspace(name, rootPath): Promise<Workspace>
get_session_state(sessionId): Promise<SessionMeta>
pick_folder(): Promise<string | null>   // CORE-ONLY
```

- `create_session` size defaults: if `cols`/`rows` omitted the core sends **80×24**; the frontend
  passes a real size after the first `fitAddon.fit()`, then calls `resize()`.
- `envOverrides` defaults to `[]`. It is exposed because S6 agents drive this surface; the frontend
  normally omits it.

### 6.2 `Channel<TerminalEvent>` (high-frequency, per attached terminal)

```rust
#[derive(Serialize, Clone, Debug, TS)]
#[serde(tag = "event", content = "data", rename_all = "camelCase")]
pub enum TerminalEvent {
    Replay { cols: u16, rows: u16, content: Vec<u8> }, // FIRST msg on attach; write BEFORE term.open()
    Output { bytes: Vec<u8> },                          // incremental live PTY bytes
}
```

- `Output.bytes` → `ch.onmessage = m => term.write(new Uint8Array(m.data.bytes))`. **Bytes never
  enter React/Zustand state.**
- `Replay` = the session's **sanitized** scrollback ring (scrollback + current screen, §11);
  written to a fresh `Terminal` at `{cols, rows}` **before** `term.open()`. `cols/rows` are the
  session's current dimensions (tracked by the daemon).
- Over Tauri IPC `Vec<u8>` arrives as `number[]`. **Plan-time optimization (§15):** if the pinned
  Tauri 2 supports a binary Channel payload, send bytes as `ArrayBuffer` to hit the firehose target.

### 6.3 Global events (`emit`/`listen`, low-frequency)

| Event | Payload | Source |
|---|---|---|
| `session://created` | `SessionMeta` | daemon `SessionMeta` push |
| `session://state-changed` | `{ sessionId, lifecycle, waitingForInput, cwd }` | daemon `StateChanged` (broker renames snake→camel) |
| `session://exited` | `{ sessionId, code: number\|null, signal: string\|null }` | daemon `ChildExited` (broker reshape) |
| `workspace://created` | `Workspace` | daemon push |
| `daemon://disconnected` | *no payload* | core socket_client |
| `daemon://reconnected` | *no payload* | core socket_client |

`lifecycle` carries exit `code`/`signal` when `Exited`, so `session://exited` is a convenience
duplicate of the terminal `Exited` transition; both are emitted.

---

## 7. Hop B — core ⇄ daemon (Unix domain socket)

> **Superseded 2026-07-07 (Pv2, `[0.2.0]`) — historical section, kept for the original S0+S1
> contract shape.** The `Frame`/`Request`/`Response`/`Push` enum shapes below are still accurate
> (Pv2 is additive: no variant removed except `Hello`/`Welcome`/`Incompatible` on `Request`/
> `Response`, which moved OUT of the frame entirely into the preamble). What changed: the codec is
> now CBOR (`ciborium`), not `bincode`; the handshake is now a **codec-agnostic preamble**
> exchanged BEFORE any `Frame` is sent (not a `Hello`/`Welcome` `Frame` pair); `MAGIC`/
> `PROTO_VERSION` constants were replaced by `PREAMBLE_MAGIC`/`CLIENT_MIN_VERSION`/
> `CLIENT_MAX_VERSION`/`DAEMON_MIN_VERSION`/`DAEMON_MAX_VERSION`; attach is multi-subscriber, not
> single-attach-supersede (see the amendment further down this section); `DaemonShutdown{drain}`
> now has real semantics. Full current contract:
> `docs/superpowers/specs/2026-07-06-protocol-v2-design.md` §3-§6.

**Framing (byte-for-byte, AS ORIGINALLY BUILT — see the superseded-box above for what's current):**
every message = `u32` little-endian length prefix + `bincode(Frame)`. No separate raw preamble.
`bincode` = 1.3.3 defaults (fixint, little-endian), identical both sides.

```rust
pub const MAGIC: u32 = 0x4250_4131;     // "BPA1"
pub const PROTO_VERSION: u16 = 1;

#[derive(Serialize, Deserialize)]
pub enum Frame {
    Request  { id: u64, req: Request },   // core → daemon; id correlates the reply
    Response { id: u64, res: Response },   // daemon → core; echoes the request id
    Push(Push),                            // daemon → core; unsolicited (id-less)
}
```

**Handshake (AS ORIGINALLY BUILT — superseded by the Pv2 preamble, see the box above):** the
client's **first** Frame MUST be `Request{ id: 0, req: Hello{ magic: MAGIC,
proto_version: PROTO_VERSION, client_build } }`. The daemon validates `magic` + `proto_version` and
replies `Response{ id: 0, res: Welcome{ proto_version, daemon_build } }` **or**
`Response{ id: 0, res: Incompatible{ min, max } }` and closes. On any mismatch: **refuse**, never
misparse. *(Current: this whole exchange happens BEFORE any `Frame` — a raw codec-agnostic
preamble — not as a `Hello`/`Welcome` `Frame` pair; see Pv2 §4.)*

```rust
#[derive(Serialize, Deserialize)]
pub enum Request {
    Hello { magic: u32, proto_version: u16, client_build: String },
    ListWorkspaces,
    CreateWorkspace { name: String, root_path: String },
    ListSessions,
    CreateSession { workspace_id: WorkspaceId, shell: Option<String>, cwd: Option<String>,
                    env_overrides: Vec<(String,String)>, cols: u16, rows: u16 },
    AttachSession { session_id: SessionId },
    DetachSession { session_id: SessionId },
    WriteStdin { session_id: SessionId, bytes: Vec<u8> },
    Resize { session_id: SessionId, cols: u16, rows: u16 },
    KillSession { session_id: SessionId },
    GetSessionState { session_id: SessionId },
    DaemonShutdown { drain: bool },
}

#[derive(Serialize, Deserialize)]
pub enum Response {                          // correlated 1:1 with a Request id
    Welcome { proto_version: u16, daemon_build: String },
    Incompatible { min: u16, max: u16 },
    Workspaces(Vec<Workspace>),
    Workspace(Workspace),
    Sessions(Vec<SessionMeta>),
    Session(SessionMeta),                    // reply to CreateSession / GetSessionState
    Ack,                                     // reply to Detach/WriteStdin/Resize/Kill/Attach/Shutdown
    Error { code: String, message: String }, // command failed; rejects the awaiting Promise
}

#[derive(Serialize, Deserialize)]
pub enum Push {                              // unsolicited daemon → core
    Replay { session_id: SessionId, cols: u16, rows: u16, content: Vec<u8> },
    Output { session_id: SessionId, bytes: Vec<u8> },
    StateChanged { session_id: SessionId, lifecycle: SessionLifecycle,
                   waiting_for_input: bool, cwd: String },
    ChildExited { session_id: SessionId, code: Option<u8>, signal: Option<String> },
    SessionCreated { meta: SessionMeta },    // for events triggered by another client/agent
    WorkspaceCreated { workspace: Workspace },
    Error { session_id: Option<SessionId>, code: String, message: String }, // async, un-correlated
}
```

**Correlation:** the core keeps a `HashMap<u64, oneshot::Sender<Response>>`; each brokered command
allocates a monotonic `id`, sends `Request{id,…}`, and awaits the matching `Response`. `Response::
Error` rejects that Promise. `Push` frames are fanned out to Channels / global events by the broker.

**Broker mapping (core):**

| Daemon → | Hop A |
|---|---|
| `Response::Session/Sessions/Workspace(s)/Ack` | resolves the awaiting command Promise |
| `Response::Error` | rejects the awaiting command Promise |
| `Push::Replay` / `Push::Output` (matching an attached session) | `Channel<TerminalEvent>` `Replay`/`Output` |
| `Push::StateChanged` | `session://state-changed` (rename snake→camel) |
| `Push::ChildExited` | `session://exited { sessionId, code, signal }` |
| `Push::SessionCreated` / `WorkspaceCreated` | `session://created` / `workspace://created` |
| `Push::Error{ session_id }` | log + (if `session_id`) mark that session errored in UI |

**Attach model (superseded 2026-07-07, Pv2 §5 — was single-attach as originally built):** the
daemon supports **N independent subscribers per session**, keyed `(session_id, conn_id)`
(`crates/sessiond/src/attach.rs`, `AttachRegistry`). `AttachSession` registers a NEW subscriber
entry and triggers a fresh `Replay` (that subscriber's own snapshot) followed by live `Output`
fanned out to it. A second `AttachSession` for the same session from a different connection **no
longer supersedes** the first — both stream independently, each with its own bounded outbound
queue and backpressure. The GUI remains a single subscriber today (no co-view UI consumer until
S6); the wire/daemon capacity is what changed. `DetachSession` stops `Output` for that one
subscriber only (the PTY keeps running, the ring keeps filling, and any other subscriber's stream
is unaffected — see §12 keep-alive). Backpressure watermark is per-PTY per-subscriber (§12).

**Attach on an inactive session (added Pv2/Task 12r — cold-rehydrate):** at boot the daemon
cold-rehydrates every persisted session as an inactive, PTY-less, replay-only `Supervisor` entry
(`crates/sessiond/src/boot.rs`, `cold_rehydrate_sessions`). `AttachSession` on such a session — or
on any session whose PTY has already exited — now **succeeds**: it replays scrollback (no live
subscription, since there is no PTY to stream from) rather than erroring. Only a truly unknown
session id (never created, or fully reaped with no persisted row) returns `NoSuchSession`.

**Reattach flow:** `attach_session(id, channel)` → core `AttachSession` → daemon `Response::Ack`,
then `Push::Replay` (sanitized ring at current cols/rows), then streamed `Push::Output`. Flow control:
xterm `write()` callback + watermark — pause the PTY when pending > **100 KB**, resume < **10 KB**
(HIGH ≤ 500 KB).

---

## 8. Socket path, single-instance, permissions, launchd

### 8.1 Socket path resolution (macOS `sun_path` is 104 bytes incl. NUL — usable < 104)

```
if $XDG_RUNTIME_DIR set & non-empty:  dir = "$XDG_RUNTIME_DIR/bpa"
else:                                  dir = "/tmp/bpa-<uid>"
mkdir dir with mode 0700; if it already exists, assert it is owned by <uid> and is 0700 (else refuse)
socket   = dir + "/d.sock"     ; assert socket.len() < 104  (hard fail otherwise)
lockfile = dir + "/d.lock"
```

Durable state (DB, settings, logs) lives under `~/Library/Application Support/ai.builderpro.desktop/`
— **never** the socket (that path overflows the 104-byte limit).

### 8.2 Single-instance + socket permissions

- Daemon holds `flock(lockfile, LOCK_EX | LOCK_NB)` for its whole lifetime; a second daemon that
  can't take the lock exits immediately.
- Socket file mode = **0600**; parent dir mode 0700 owned by `<uid>` (verified before `bind`).
- `bind` uses a fresh path: if a stale `d.sock` exists (file present, `connect` → `ECONNREFUSED`),
  unlink and re-bind. Guard the `/tmp` squatting race: create the dir with `O_NOFOLLOW` semantics
  and re-verify owner+mode after creation; prefer `$XDG_RUNTIME_DIR` when available.
- **Peer-cred check:** on `accept`, verify the connecting peer's effective uid equals the daemon's
  uid (`LOCAL_PEERCRED`/`getpeereid`); refuse otherwise.

### 8.3 LaunchAgent (`~/Library/LaunchAgents/ai.builderpro.desktop.sessiond.plist`)

```xml
<key>Label</key>              <string>ai.builderpro.desktop.sessiond</string>
<key>ProgramArguments</key>   <array>
  <string>{ABS_PATH_TO_BUNDLED_bpa-sessiond}</string>
  <string>--socket</string><string>{RESOLVED_SOCKET_PATH}</string>
</array>
<key>KeepAlive</key>          <dict><key>Crashed</key><true/></dict>  <!-- NEVER bare true -->
<key>RunAtLoad</key>          <false/>
<key>ThrottleInterval</key>   <integer>10</integer>
<key>ProcessType</key>        <string>Background</string>
<key>StandardOutPath</key>    <string>{APP_SUPPORT}/logs/sessiond.out.log</string>
<key>StandardErrorPath</key>  <string>{APP_SUPPORT}/logs/sessiond.err.log</string>
```

- **Socket activation is intentionally NOT used.** The daemon `bind`s its own socket at the known
  short path (§8.1) and owns stale-socket cleanup (§8.2). This avoids the launchd fd-passing dance;
  the tradeoff is deliberate and documented here.
- Installer (`launchd.rs`) resolves the daemon's absolute path from the running app bundle
  (`current_exe()` → sibling), ensures `~/Library/LaunchAgents` exists, writes the plist, and
  `launchctl bootstrap gui/$UID <plist>`.
- GUI starts the daemon on demand: `launchctl kickstart gui/$UID/<label>`, then connects with
  bounded backoff. launchd restarts the daemon **only on crash** (`KeepAlive{Crashed}`); a clean idle
  self-exit (exit 0) stays down.
- The daemon must **not** double-fork/`setsid` under launchd (launchd would think it died). The
  `daemonize` path is a dev-only, feature-flagged fallback for running outside a bundle.
- **launchd failure degradation (§13):** treat "already bootstrapped" as success (idempotent);
  `bootout`+re-`bootstrap` on plist drift; if `bootstrap`/`kickstart` hard-fails (TCC/permissions),
  surface an actionable "could not install background service" banner (and, in dev, fall back to the
  feature-flagged `daemonize` path) — never hang silently.

---

## 9. PTY threading contract (`pty_supervisor.rs`)

Per session the supervisor owns:
`{ master, writer: Box<dyn Write+Send>, killer: Box<dyn ChildKiller+Send+Sync>, pgid: Option<i32>,
reader_thread, wait_thread }`.

Rules (all locked — violating any is a known hang/leak):

1. `native_pty_system()` **once** at startup.
2. Per session: `openpty(PtySize{rows,cols,0,0})` → `slave.spawn_command(cmd)` → capture
   `pgid = master.process_group_leader()` → **`drop(pair.slave)` immediately** (else master `read()`
   never sees EOF).
3. `CommandBuilder`: `env_clear()` then set the **allowlist** — `TERM=xterm-256color`, `PATH`,
   `HOME`, `USER`, `LOGNAME`, `SHELL`, `LANG`, `LC_ALL`, `LC_CTYPE`, `TMPDIR`, `TERM_PROGRAM=BuilderProAI`,
   `SSH_AUTH_SOCK` (forwarded so git-over-ssh works; documented tradeoff), plus the shell-integration
   var (`ZDOTDIR` or `BPA_INJECTION`). Never forward daemon-internal/secret env. `cwd(validated_dir)`
   (§16).
4. One **blocking OS reader thread** per PTY (portable-pty has no async API); 4–64 KiB buffered reads;
   `read() == Ok(0)` ⇒ EOF ⇒ tear down. Feed bytes to: the OSC parser, the live grid, and the
   sanitized scrollback ring.
5. `writer = master.take_writer()` (take-once) owned by the supervisor; `flush()` after writes.
6. `killer = child.clone_killer()` captured **before** any blocking wait. `wait()`/`try_wait()` run on
   the single owning `wait_thread`. `Child` is `Send` not `Sync` — never share it across threads.
7. `resize` calls `master.resize(PtySize{..})` (delivers SIGWINCH) and updates tracked cols/rows.
8. **Teardown / kill signals the whole process group** (not just the shell — otherwise long-lived
   agent CLIs / dev servers orphan): if `pgid` is set, `killpg(pgid, SIGTERM)` → grace (≈2 s) →
   `killpg(pgid, SIGKILL)`; always call `killer.kill()` + `wait()` afterward to reap the zombie.
   (POSIX only; `process_group_leader()` returns `None` under Windows ConPTY — fall back to
   `killer.kill()` there.)

Shells spawn with `setsid` (portable-pty does this) → children survive the GUI.

---

### Resource model (amended 2026-07-04, A21)

Per-session cost envelope: **3 OS threads** (reader / wait / ticker) + **1 forwarder thread per
live attachment** + PTY fds. There is currently **NO enforced cap** on concurrent sessions;
exhaustion behavior is the OS's. Planned additive remedy: configurable cap + typed
`SessionLimitReached` error (**BL-4**).

---

## 10. Shell integration & status (`osc_parser.rs`, `shell_integration/`)

Spawn the user's **real** shell; inject a tiny integration **non-invasively** (no rc edits); emit
**OSC 133 A/B/C/D + OSC 7**; parse from the PTY stream. We do **not** adopt OSC 633.

### 10.1 Emitted sequences (BEL-terminated; parser also accepts ST + implicit-ESC)

| Seq | Meaning |
|---|---|
| `ESC ] 133 ; A BEL` | prompt start |
| `ESC ] 133 ; B BEL` | command start (end of prompt) — embedded at end of `PS1`, zero-width-wrapped |
| `ESC ] 133 ; C BEL` | output start (command running) |
| `ESC ] 133 ; D ; <code> BEL` | command finished; `<code>` = the previous command's exit status |
| `ESC ] 7 ; file://host/<abs> BEL` | cwd report |

### 10.2 Injection (contract — port mechanics from the cited references; emit-order locked here)

**Emit-order per hook (locked; osc_parser tests + scripts must agree):**
- `precmd` / `PROMPT_COMMAND` emits, in order: capture `code=$?` **first**, then `D;<code>` (closes
  the previous command), then `A`, then `OSC 7`.
- `preexec` / `DEBUG`-trap emits: `C` **exactly once** per command.
- `B` is embedded at the end of `PS1`, zero-width-wrapped.

**zsh:** `ZDOTDIR` temp-dir redirect. Temp `.zshenv` restores the user's original `ZDOTDIR`,
re-sources their real startup files, then `autoload -Uz add-zsh-hook; add-zsh-hook precmd
_bpa_precmd; add-zsh-hook preexec _bpa_preexec`. `B` mark wrapped in `%{…%}`. Temp dir removed after
first load. Port mechanics from kitty `shell-integration/zsh`.

**bash:** launch with `--init-file <bpa-bash.sh>` and env `BPA_INJECTION=1`. Script sources the
user's rc **first**, then **wraps** (never clobbers) `PROMPT_COMMAND` (saved to a backup var); `B`
wrapped in `\[ \]`. `preexec` via **bash-preexec** if present, else a **guarded DEBUG trap** emitting
`C` once per command (suppressed during `PROMPT_COMMAND`, chains any pre-existing trap). Port
mechanics from VS Code `shellIntegration-bash.sh` + `rcaloras/bash-preexec`.

Env flag = `BPA_INJECTION`; hook fns = `_bpa_precmd` / `_bpa_preexec`. Integration scripts ship as
daemon assets, written to a per-session runtime dir.

### 10.3 OSC parser + state machine

Streaming tokenizer: buffers partial OSC sequences across `read()` boundaries; accepts `BEL`, `ST`
(`ESC \`), and implicit-`ESC` terminators; caps the OSC buffer (8 KiB) against garbage; emits
`{PromptStart, PromptEnd, CommandStart, CommandEnd(exit?), Cwd(path)}`. **Everything not inside a
recognized OSC passes through verbatim** to the live grid + client `Output`.

**Exit-code parse rule:** `D;<code>` where `<code>` is base-10 in `0..=255` → `Exited{code:Some,…}`;
empty / non-numeric / out-of-range → `Exited{code:None,…}` (unknown/aborted). **Never coerce to 0.**

**OSC 7 decode + hardening:** accept `file://host/path` and `kitty-shell-cwd://host/path` only,
percent-decoded, host stripped; bound path length; a reported cwd is treated as **advisory display
data** — it never changes any privileged behavior and is not blindly trusted for spawns.

**Transitions:** `A` = prompt drawing; `B` → `AtPrompt` (idle); `C` → `Running`; `D;code` →
`Exited{code}` then back toward `AtPrompt` on the next `A/B`. **Empty-command rule:** `B → A` with no
`C/D` is a no-op (no phantom `Running`). `Typing` is never produced.

**Trust model:** OSC-133/7 are emitted on the PTY stream and are **child-forgeable** (any program can
print them). The derived lifecycle/cwd is **advisory**, not a security boundary — S6 agents must not
treat it as authoritative for security decisions. Parser is hardened against forged/oversized/
interleaved sequences (bounded buffer, safe decode) so a malicious child can at worst mislead status,
never crash the daemon or escape the cwd contract.

### 10.4 Waiting-for-input (heuristic — documented as such; no protocol marker exists)

```
waiting_for_input =
      lifecycle == Running
  &&  tcgetattr(master_fd) has ICANON & ECHO
  &&  NOT raw / alt-screen mode      // excludes vim, less, top
  &&  output quiescent >= 150 ms
  &&  cursor not at column 0         // cursor column from the live grid
```

Surfaced via `StateChanged` so S6 agents can detect "worker is stuck asking a question." Explicitly
best-effort; UI/agents must not present it as certain.

---

## 11. Persistence & replay (`scrollback.rs`, `live_grid.rs`, `persistence.rs`)

Two in-memory structures per session, fed by the reader thread, plus durable SQLite:

- **Sanitized scrollback ring (replay source).** A bounded ring of the PTY's **normal-buffer** output
  with side-effecting control sequences neutralized — alt-screen enter/leave (`?1049h/l`, `?47h/l`),
  window-title OSC (`OSC 0/1/2`), bracketed-paste toggles (`?2004h/l`), and our own OSC-133/OSC-7
  marks are **stripped**; SGR color/attribute sequences are **kept**. This is the source of `Replay`
  (and `History`): replaying it via `term.write` re-paints scrollback + the visible screen without
  re-triggering title/alt-screen/paste side effects (the retach approach). Kept current even with no
  client attached.
- **Live grid state (`alacritty_terminal::Term`).** Used only for cursor position/column, alt-screen
  / raw-mode detection, and cols/rows — the inputs to the §10.4 heuristic and status. It is **never
  serialized** (alacritty has no grid→ANSI encoder; we don't need one because the sanitized ring is
  the replay source).

**Durable SQLite (WAL), best-effort:**

```sql
PRAGMA journal_mode = WAL;
PRAGMA busy_timeout = 5000;
PRAGMA user_version = 1;                       -- migration/schema version
CREATE TABLE workspace (
  id TEXT PRIMARY KEY, name TEXT NOT NULL, root_path TEXT NOT NULL);
CREATE TABLE session (
  id TEXT PRIMARY KEY, workspace_id TEXT NOT NULL REFERENCES workspace(id),
  title TEXT NOT NULL, shell TEXT NOT NULL, cwd TEXT NOT NULL,
  cols INTEGER NOT NULL, rows INTEGER NOT NULL,
  lifecycle TEXT NOT NULL,                     -- 'atPrompt'|'typing'|'running'|'exited'
  exit_code INTEGER, exit_signal TEXT,         -- present iff lifecycle='exited'
  created_at INTEGER NOT NULL);
CREATE TABLE scrollback (
  session_id TEXT NOT NULL REFERENCES session(id),
  seq INTEGER NOT NULL, bytes BLOB NOT NULL, ts INTEGER NOT NULL,
  PRIMARY KEY (session_id, seq));              -- ring-pruned per session
```

- **Cadence (amended 2026-07-04, A24):** scrollback persisted **batched** — every **1000 ms**
  (`SCROLLBACK_FLUSH_INTERVAL`, `crates/sessiond/src/socket_server.rs`) per session; there is
  **NO size-based trigger**. `flush + WAL checkpoint` on graceful shutdown. Durability bound: on
  `SIGKILL` you lose at most the last unflushed window (bounded by the flush interval, ≤ ~1 s of
  tail output); WAL guarantees row-atomicity (no half-written rows).
- **Cap & semantics (amended, A24):** replay/persist cap = **256 KiB** per session ring (locked
  constant `SCROLLBACK_CAP`, `crates/sessiond/src/pty_supervisor.rs`). Snapshot-replace semantics:
  each sweep REPLACES the stored blob at `seq 0` (no append log). Dirty-check + write-architecture
  review: BL-8 (`docs/backlog.md`).
- **Rehydrate on restart (NOW IMPLEMENTED, Pv2/Task 12r — was planned-but-unbuilt as originally
  written):** at boot the daemon cold-rehydrates **every** persisted session from `scrollback`,
  registering each as an inactive, PTY-less, replay-only `Supervisor` entry
  (`crates/sessiond/src/boot.rs::cold_rehydrate_sessions`) — not merely on first attach. Every
  rehydrated session is `is_active = false` and `waiting_for_input = false` (its PTY is gone);
  `lifecycle` = stored value (typically `Exited`). Round-trips each `SessionLifecycle` variant
  losslessly (test in §14). `AttachSession` on a rehydrated (or otherwise exited) session
  **succeeds** and replays scrollback — no live subscription is created, and no error is returned;
  only an unknown session id errors `NoSuchSession`. Proven end-to-end by the e2e phase 5
  daemon-restart test (closes BL-7, `docs/backlog.md`).
- **Degradation (honest):** the in-memory ring is the Layer-1 source of truth. If the DB is
  unavailable (locked past `busy_timeout`, disk-full, read-only), the daemon **logs and keeps serving
  live sessions** — persistence is best-effort. On corruption/malformed-image at open, **quarantine**
  (`rename bpa.db → bpa.db.corrupt-<ts>`) and recreate rather than crash. Migrations run in a
  transaction keyed by `user_version`; failure fails **closed** with an actionable `Error`, not a
  panic. DB path: `{APP_SUPPORT}/bpa.db`.

---

## 12. Frontend contract (`src/terminal`, `src/store`)

- **Zustand shape:** `{ sessions: Record<SessionId, SessionMeta>, workspaces: Record<WorkspaceId,
  Workspace>, activeSessionId, daemonConnected }`. **Metadata only — never bytes.**
- **Terminal ownership:** `Terminal` instances live in a **non-reactive** `Map<SessionId, Terminal>`
  in `terminal-manager.ts`. React components borrow a ref; a Terminal is **never** in React state.
- **Keep-alive:** on panel unmount do **not** `dispose()` — keep the instance, re-`open()` into the
  new container when shown. `dispose()` **only** on real session close (kill/exit). While hidden the
  GUI stays attached (bytes keep arriving and buffer in xterm).
- **Renderer:** load `WebglAddon` lazily on **visible** terminals only (≤16 WebGL contexts/page);
  `onContextLoss(() => webgl.dispose())` → DOM fallback. Dispose the WebGL addon (not the Terminal)
  when hidden.
- **Init:** StrictMode-safe (`term.dispose()` in cleanup, or a ref guard). Import
  `@xterm/xterm/css/xterm.css`. `convertEol` **off** (real PTY handles `\n`).
- **Create size:** create the Terminal, `fit()` after `open()`, pass the fitted `cols/rows` to
  `create_session` (default 80×24 if not yet laid out), then `resize()` on subsequent `fit()`s.
- **Resize:** debounced `ResizeObserver` → `fitAddon.fit()` → `term.onResize` → `resize()` IPC. Never
  `open()`/`fit()` a zero-dimension container.
- **Input:** `term.onData(d => write_stdin(id, d))`.
- **Status dots** read `SessionMeta.lifecycle` + `waitingForInput` (updated by `session://state-changed`).

### Attach state contract (as built @ 4c91dfe; amended 2026-07-04, A22)

- Attach state is a **per-session** state machine `detached | attaching | attached` owned by
  `TerminalManager` (`src/terminal/terminal-manager.ts`) — never by a pane component (a pane
  instance is REUSED across tab switches; a per-instance guard makes later tabs dead panes).
- **Coalescing:** concurrent `attach()` callers for one session share the single in-flight promise
  (exactly one IPC / one Channel / one Replay); StrictMode's double effect-invoke coalesces.
- **Generation guard:** `resetAttachment` / `resetAllAttachments` / `dispose` bump a per-session
  generation; a stale in-flight completion refuses to record state.
- **Reconnect:** on `daemon://reconnected` → reset ALL attach state → eagerly re-attach the visible
  session → hidden sessions re-attach lazily on next show, each with a fresh Replay. Stale panes
  are visually marked until re-attached (**implementation BL-5**).

---

## 13. Error handling & honest degradation

- **Daemon unreachable:** `socket_client` retries connect (bounded exponential backoff, cap ~5 s),
  emits `daemon://disconnected`; UI banner, terminals greyed. On reconnect → `daemon://reconnected`,
  re-`list_sessions` + re-`attach` visible sessions. **No fake "connected" state.**
- **Stale socket** (file exists, `ECONNREFUSED`): unlink, `launchctl kickstart`, retry.
- **Version mismatch:** handshake `Incompatible` → refuse with an actionable "app/daemon out of sync"
  error; never misparse.
- **launchd failures:** per §8.3 (idempotent bootstrap, bootout+rebootstrap on drift, actionable
  banner on hard failure).
- **Socket write failure / slow client (Hop B):** each client has a **bounded outbound queue**; on
  overflow the daemon **drops + disconnects** that client (never unbounded buffering → no memory-DoS);
  on `EPIPE`/write error, treat the client as detached and clean up its attach entry. One slow/dead
  client must **not** stall the daemon or pause an unrelated session's PTY.
- **PTY spawn failure / child crash:** `Response::Error` (rejects the command Promise) or
  `Push::ChildExited`; session marked `is_active=false`, scrollback retained; UI shows exit status.
- **Workspace root gone:** on create/rehydrate when the validated root no longer exists → typed
  `Error{code:"InvalidWorkspaceRoot"|"CwdMissing"}`; do **not** silently spawn in an unexpected dir.
- **SQLite failures:** §11 (best-effort, quarantine-on-corruption, fail-closed migrations). A
  `persistence-degraded` (quarantine / repeated flush-failure) Push event + UI indicator is a
  specced future event — **BL-7**.
- **Logging:** `tracing` structured logs (session id, transitions, errors); **no secrets** (env is
  allowlisted; a scrub test asserts no allowlisted-secret value appears in logs). Logs under
  `{APP_SUPPORT}/logs/`.

**Survival truth table** — the canonical statement lives in the platform overview §2; restated here
for locality and must not drift from it:

| Event | Sessions |
|---|---|
| GUI close / crash / restart | live shells **keep running** (daemon-owned); reattach + replay |
| Daemon restart / upgrade / crash | live shells **end**; session records + scrollback survive (up to the last ~1 s flush) and rehydrate as **inactive** sessions |
| **macOS logout** | die (per-user LaunchAgent torn down) |

### Error-surfacing contract (normative; implementation BL-6) — amended 2026-07-04, A23

**Invariant: no command rejection is silent.** Every rejected invoke reaches the user as a
readable message (toast/banner), mapped from the typed error:

| Error (kind / daemon code) | User-facing message |
|---|---|
| `Disconnected` | "Daemon disconnected — reconnecting… The action didn't run; retry when the banner clears." |
| `Internal` | "Something went wrong inside the app (not the daemon). Details were logged." |
| `NoSuchSession` | "That terminal no longer exists — it may have been closed or the daemon restarted." |
| `InvalidWorkspaceRoot` | "That folder can't be a workspace root: it must exist and be a real directory." |
| `CwdMissing` | "Starting folder doesn't exist (or isn't a directory)." |
| `RelativePath` | "Path must be absolute." |
| `SymlinkEscape` | "That path is a symlink pointing outside its folder — not allowed." |
| `NotADirectory` | "That path exists but isn't a directory." |
| `InvalidShell` | "Shell path must be absolute (e.g. /bin/zsh)." |
| `CreateSessionFailed` / `SpawnError` | "Couldn't start the terminal: {message}." |
| `PtyError` / `IoError` | "Terminal I/O error: {message}." |
| `DbError` | "Saved-state database error: {message}. Live terminals keep working." |
| `SinkClosed` | "The terminal's output channel closed — reopen the tab to re-attach." |
| `UnexpectedHello` | (protocol bug — log only, generic Internal message to the user) |

Fire-and-forget call sites (`void manager.attach(...)`, etc.) must route rejections into this
surface — tracked as BL-6.

---

## 14. Testing (TDD) & Definition of Done

### 14.1 Test suites

**Daemon (Rust, `cargo test`):**
- PTY spawn + echo roundtrip; `drop(slave)` → EOF; kill → reap (no zombie); **process-group kill**
  orphans nothing (spawn a child that forks a grandchild; assert both gone after kill).
- `resize` → SIGWINCH (child prints `$COLUMNS` on WINCH; assert new size).
- **Env hygiene:** plant `DAEMON_SECRET`; assert absent in the child env; assert the allowlist present.
- OSC parser: split-across-`read()` sequences; `BEL`/`ST`/implicit-ESC terminators; buffer cap;
  `D;<code>` in-range/empty/non-numeric/out-of-range; OSC 7 `file://` + `kitty-shell-cwd://` decode;
  forged/oversized/interleaved OSC handled safely.
- State machine: full transition table; empty-command `B→A` no-op; `D`-without-code → `Exited{None}`.
- Waiting-for-input: `cat` (canonical) → true; `vim`/`less` (alt-screen) → false; running
  non-interactive → false; idle at col 0 → false.
- Scrollback ring: bounds/prune; **sanitization** (alt-screen/title/paste stripped, SGR kept);
  Replay of a past `vim`/title-setting session does not corrupt a fresh terminal.
- Persistence: WAL persist + rehydrate; each `SessionLifecycle` variant round-trips; **corrupt-db
  quarantine + recreate**; `busy_timeout` under concurrent access; migration on old `user_version`;
  kill -9 mid-write → restart opens + rehydrates committed rows.
- Protocol: table-driven codec round-trip (originally `bincode`, now CBOR/`ciborium` post-Pv2 —
  same table-driven shape, `crates/protocol/tests/roundtrip.rs`) for **every**
  `Request`/`Response`/`Push` variant; framing (u32-LE length, partial frame across reads, oversized/garbage-frame rejection, magic +
  version mismatch → refuse); request/response **correlation** (concurrent in-flight ids resolve
  correctly).
- Socket: `flock` single-instance (second daemon exits); **peer-cred** rejection (wrong uid refused);
  socket mode 0600; **backpressure** — attach, stop reading, assert daemon disconnects that client and
  other sessions keep flowing.
- Attach: single-attach supersede + fresh Replay; DetachSession stops Output, PTY keeps running.
- Path validation (§16): missing dir / file-not-dir / relative / symlink-escape / root-deleted-before-create.
- launchd (`launchd.rs`, mock/inject `launchctl`): bootstrap idempotency ("already bootstrapped"
  = success); kickstart-on-demand; dir-missing creates it; hard-failure surfaces error.
- **Detach integration:** kill the client, assert child alive (`pgrep`); reconnect; assert scrollback replays.

**Frontend (TS, vitest + RTL):**
- Zustand reducers; `session://state-changed` → status-dot update; `daemon://disconnected` → banner.
- terminal-manager keep-alive (no dispose on unmount; dispose on close); StrictMode double-init guard.
- Channel → `term.write` path; assert bytes never enter the store; Replay-before-open ordering.

**E2E (macOS):** launch → create terminal → run a command → observe OSC-driven status → quit app →
assert daemon + shell survive (`pgrep`) → relaunch → reattach + scrollback intact.

### 14.2 Contract → test traceability matrix (every locked contract has a test)

| Contract (§) | Test |
|---|---|
| Shared types / Rust⇄TS parity (§5) | CI `ts-rs` generate + `git diff --exit-code` on `src/ipc/types.ts` |
| Hop-B framing + correlation + handshake (§7) | protocol round-trip + framing + correlation tests |
| PTY threading + pgroup kill + env (§9) | echo/EOF/zombie/pgroup-kill/SIGWINCH/env-hygiene tests |
| OSC parser + state machine + parse rule (§10) | parser + state-machine + exit-code tests |
| Waiting-for-input (§10.4) | heuristic tests |
| Scrollback sanitize + replay (§11) | ring + sanitization + replay-no-corrupt tests |
| SQLite degradation + rehydrate (§11) | persist/rehydrate/corrupt/busy/migration/kill-9 tests |
| Socket path/perms/single-instance/peer-cred (§8) | flock/peer-cred/mode/stale-socket tests |
| Backpressure / slow-client (§13) | backpressure disconnect test |
| Attach model (§7) | single-attach supersede + detach tests |
| Path validation (§16) | path-validation tests |
| launchd install/degradation (§8.3, §13) | launchd mock tests + E2E survive test |
| Frontend keep-alive/renderer/state (§12) | terminal-manager + store tests |

### 14.3 Definition of Done

- All 14.1 suites green; the 14.2 matrix has no uncovered row; daemon-crate line coverage ≥ 80 %.
- Universal binary builds; the embedded `bpa-sessiond` is **deep-signed** (Developer ID + hardened
  runtime + `entitlements.plist`) and the notarization ticket is stapled to the `.app`; verified by
  `codesign --verify --deep --strict` + `spctl --assess` + a **first-launch-on-clean-macOS-VM** smoke
  test (create terminal → quit → relaunch → reattach).
- A **no-secrets-in-logs** test passes; a **backpressure/bounded-memory** acceptance test passes.
- This spec's contracts implemented exactly; README + module docs updated in the same change.

---

## 15. Re-verify at plan time (assumptions to confirm before code)

1. Exact current pins for every §3 row (Context7 + registry); record concrete versions in
   `package.json` + `Cargo.toml` together.
2. `alacritty_terminal` public API for the live-grid queries we use (cursor column, alt-screen/mode
   flags, resize) at the pinned version. (We no longer depend on grid→ANSI — replay uses the
   sanitized byte ring, §11.)
3. Whether the pinned **Tauri 2 `Channel`** supports a binary/`ArrayBuffer` payload; if so, send
   `Output`/`Replay` bytes as binary instead of `number[]` for the firehose (§6.2).
4. `ts-rs` config to emit the exact TS in §5 (tag/content encodings, `Vec<u8> → number[]`).
5. Confirm `bincode` 1.3.3 + serde covers all `Frame`/`Request`/`Response`/`Push` variants (enums,
   `Vec<(String,String)>`, `Option`, `Vec<u8>`); it does, but verify at lock.

### Resolution table (amended 2026-07-04 — what plan-time verification actually found)

| §15 item | Resolution |
|---|---|
| 15.1 exact pins | Verified & recorded: bincode 1.3.3, portable-pty 0.9.0, rusqlite 0.32, alacritty_terminal =0.25.0 (exact pin), xterm 6.0.0, Zustand 5, React 19 (`Cargo.toml` / `package.json`). |
| 15.2 alacritty live-grid API | Verified at 0.25.0 — cursor column, alt-screen/mode flags, resize all used in `crates/sessiond/src/live_grid.rs`. |
| 15.3 binary Channel payload | **Resolved: NOT binary.** `Channel<TerminalEvent>` serializes via serde; `Vec<u8>` crosses Hop-A as `number[]`. Acceptable at S1 throughput; Pv2 shipped without revisiting this (Hop-A/Channel is unchanged by Pv2 — only Hop-B's wire codec changed) — still a possible future optimization if profiling demands. |
| 15.4 ts-rs emission | Verified — `crates/protocol/tests/ts_export.rs` regenerates and asserts `src/ipc/types.ts`; parity is a final-suite + CI gate. |
| 15.5 bincode covers all variants | **DIVERGED** — bincode 1.3 cannot DEserialize tagged enums; resolved by the hand-written dual-codec impls (§3 amendment, A2). |
| §3 `daemonize` dev fallback row | **NOT BUILT** — no `daemonize` dependency exists. Dev mode uses the SAME launchd path as production (the plist just points at `target/debug/bpa-sessiond`); only the e2e harness spawns the daemon directly. Row kept in §3 for history; treat as dropped (A30). |

---

## 16. Trust & security model (consolidated) — rewritten 2026-07-04 (A7/A9/A11/A18/A19)

### As built (verified against `main` @ 285cb2e)

- **Path validation (shared `bpa-paths` crate — core AND daemon):** `create_workspace.root_path`
  and `CreateSession.cwd` are canonicalized (realpath), required to be **absolute + existing + a
  directory**, symlink-escape rejected (canonicalize and re-check vs the lexical parent) — one
  implementation (`crates/paths`), byte-for-byte identical on both sides, with core-side
  pre-flights (`preflight_cwd`/`preflight_workspace_root`) for fail-fast. This surface is also
  driven by S6 agents, so the daemon is the authoritative validator.
- **Env hygiene:** `env_clear()` + explicit allowlist (§9.3); daemon-internal/secret env never
  reaches child shells; a test asserts a planted secret is absent.
- **Socket:** dir 0700 owned by uid, socket 0600, peer-cred (`getpeereid`) euid check, stale-socket
  unlink, `/tmp` squatting guard (§8.2).
- **OSC trust:** OSC-133/7-derived status/cwd is **child-forgeable and advisory** — never a security
  boundary; parser hardened against forged/oversized input (§10.3).
- **Logging:** structured, no secret values; allowlisted-secret scrub test (§13).
- **Signing:** deep-signed + notarized sidecar so Gatekeeper doesn't kill the daemon (§14.3); TCC /
  launchd-permission failures degrade to an actionable banner (§8.3).

### env_overrides (decision recorded; enforcement BL-1)

`env_overrides` are applied AFTER the allowlist and can currently set any variable, including
loader vars — the §9.3 allowlist is a **default, not a ceiling**. Decision: the daemon WILL
enforce a `DYLD_*`/`LD_*` denylist before applying overrides (**BL-1**). Same-uid peer-cred means
this is not a privilege boundary today; it becomes one when agent-initiated sessions land (S6).

### Agent execution trust layer (roadmap — implemented in S6c)

Autonomous agents executing terminal commands is this product's central risk. Locked posture:

- **Approval-gate classes** (owner decision P1): destructive-fs outside the workspace,
  `git push --force`, package publish, spend/payment, production deploy. Approvals are batched
  via the escalation inbox; **synchronous confirm** only for irreversibly destructive actions.
- **Audit log:** append-only record of every agent-initiated command (who/what/where/when).
- **Caller identity:** same-uid peer-cred today; per-agent identity tokens are an S6 design item.

### SSH agent (owner decision P2)

`SSH_AUTH_SOCK` is **withheld by default** from agent-spawned sessions, grantable per-task via
approval. Human GUI sessions keep current behavior (forwarded per the §9.3 allowlist).

### Provider API keys (S6a; BL-20)

macOS **Keychain only** — never SQLite, config files, or logs. LLM egress happens from the core
process only; the webview never talks to model providers.

This posture generalizes to **ALL provider / connector / MCP secrets** (BL-20): Keychain only;
egress core/`bpa-orchd`-only. Research-source connections (agents querying production databases,
docs stores, log systems — audit V24) live in a registry of **read-only, audited** connections
with their own Keychain credentials — never ad-hoc connection strings in agent context.

### Webview boundary (BL-2)

Target: restrictive CSP in `tauri.conf.json`. Current state, stated honestly: `csp: null` until
BL-2 lands. The capabilities file already scopes the IPC surface.

### Data at rest (BL-3, BL-4; retention = owner decision P3)

`bpa.db` contains raw terminal scrollback ⇒ **secret-bearing** (env dumps, tokens, command
output). Required: 0600 file mode + purge-on-delete (**BL-3**), tied into retention (**BL-4**).
Retention defaults: keep the last **20 exited sessions per workspace** / **30-day TTL** (config
later); deleting a workspace cascades its live sessions **with consent**.

### MCP & extension boundary (roadmap — implemented in S-EXT; amended 2026-07-06)

The product connects external MCP servers, connectors, and skills (vision v2–v4). An MCP server
is a remote code/data boundary; the posture (details in the S-EXT spec):

- **Egress:** all MCP/connector traffic originates from the core/`bpa-orchd` process only — never
  the webview.
- **Consent:** connecting a server is a consent-gated action; enabled tools are an explicit
  per-server allowlist. A **stdio** MCP server executes local code — connecting one is equivalent
  to installing a plugin and is gated accordingly.
- **Tool results are untrusted input.** Treated as potential prompt-injection: agents must not
  act on instructions embedded in tool results without policy/gate mediation (the same trust
  layer that gates agent-initiated commands).
- **Spend:** MCP tool invocations pass through the agent trust layer — spend caps and approval
  classes apply exactly as for agent commands; per-call cost/latency is recorded (S6a's capture
  rule generalized).
- **v1 surface (owner default, Q6):** tools + auth only; **sampling disabled by default** (a
  remote server must not spend the owner's LLM budget); resources/prompts deferred to backlog.
- **Unattended access:** Keychain access for `bpa-orchd` runs under a locked screen is an open
  sub-question (backlog row) — resolved in the S-EXT spec before the first unattended MCP call.
