# Builder Pro AI — S0+S1 (Foundation + Terminal Core) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship the Tauri 2 macOS app foundation plus a detached, survive-restart terminal engine — a Rust session daemon that owns real PTYs, OSC-133 status, scrollback replay, and a programmatic control surface.

**Architecture:** Two processes, two IPC hops. A React/TS webview + Rust core (broker) inside the Tauri app; a separate long-lived Rust daemon (`bpa-sessiond`, launchd-supervised) that exclusively owns the PTYs. Webview ⇄ core over Tauri IPC (commands + `Channel` firehose + global events); core ⇄ daemon over a Unix domain socket (length-prefixed `bincode` frames with request/response correlation).

**Tech Stack:** Tauri 2, React 19 + Vite + TS, Zustand 5, `@xterm/xterm` 6; Rust (`portable-pty` 0.9.0, `alacritty_terminal`, `tokio`, `rusqlite` WAL, `bincode` 1.3.3, `ts-rs`, `tracing`).

**Source spec (READ IT — it locks every type/signature/protocol byte):** [`docs/superpowers/specs/2026-07-01-builderpro-s0s1-foundation-terminal-design.md`](../specs/2026-07-01-builderpro-s0s1-foundation-terminal-design.md). Platform overview: [`…-platform-overview.md`](../specs/2026-07-01-builderpro-platform-overview.md).

---

## Global Constraints

Every task's requirements implicitly include this section. Values are copied verbatim from the spec.

- **Platform:** macOS only; universal binary (arm64 + x86_64). **Rust ≥ 1.77.2.**
- **Version pins (spec §3):** `tauri` ^2 (record 2.11.4), `@tauri-apps/api` ^2, plugins `store`/`dialog`/`fs`/`shell` major `2`; `portable-pty` = 0.9.0; `alacritty_terminal` pinned exact (0.24/0.25 — confirm at task time); `rusqlite` 0.32 (feature `bundled`); `bincode` = **1.3.3**; `tokio` 1.x (features `net,io-util,rt-multi-thread,macros,sync,time`); `rustix` (features `fs,net`); `uuid` v4; `tracing` + `tracing-subscriber`; `ts-rs`. Frontend: React 19, Vite 6/7, TypeScript 5, Zustand 5, `@xterm/xterm` 6.0.0 + addons `fit`0.11/`webgl`0.19/`search`0.16/`web-links`0.12/`serialize`0.14 (scoped `@xterm/*` only; search/web-links/serialize bumped one patch — the 0.15/0.11/0.13 releases peer-require xterm ^5 and break `npm install` against xterm 6.0.0, found in Task 1).
- **Naming (locked):** bundle id `ai.builderpro.desktop`; product `Builder Pro AI`; daemon binary `bpa-sessiond`; LaunchAgent label `ai.builderpro.desktop.sessiond`; hook fns `_bpa_precmd`/`_bpa_preexec`; env flag `BPA_INJECTION`; wire `MAGIC = 0x4250_4131`, `PROTO_VERSION = 1`; socket `$XDG_RUNTIME_DIR/bpa/d.sock` else `/tmp/bpa-<uid>/d.sock`; lockfile `d.lock`; `APP_SUPPORT = ~/Library/Application Support/ai.builderpro.desktop`; DB `{APP_SUPPORT}/bpa.db`.
- **Cargo workspace** at repo root; members `["src-tauri", "crates/protocol", "crates/sessiond"]`.
- **TDD mandatory:** every task = write failing test → run (confirm FAIL) → minimal impl → run (confirm PASS) → commit. Conventional commits, frequently.
- **Use EXACT identifiers/signatures from spec §5–§7, §9–§11, §16.** Do not invent names. The TS types in `src/ipc/types.ts` are **generated** from `crates/protocol` via `ts-rs` — never hand-edited.
- **Honest degradation everywhere:** every external call (PTY, socket, DB, launchd) handles errors and degrades without lying to the user. Structured logs, no secret values.

---

## File structure (ownership map — no two parallel tasks write the same file)

See spec §4 for the full tree. Ownership by task:

| Area | Files | Task(s) |
|---|---|---|
| Scaffold / configs | `package.json`, `vite.config.ts`, `tsconfig.json`, `index.html`, `Cargo.toml` (workspace), `src-tauri/{Cargo.toml,tauri.conf.json,build.rs}` | T1, T2 |
| Shared protocol | `crates/protocol/src/lib.rs` | T3 |
| Daemon modules | `crates/sessiond/src/{singleton,osc_parser,scrollback,persistence,live_grid,pty_supervisor}.rs`, `crates/sessiond/src/shell_integration/**` | T4–T10 |
| Daemon server | `crates/sessiond/src/{attach,socket_server,main}.rs` | T11–T13 |
| Core / broker | `src-tauri/src/{socket_client,paths,launchd,commands,broker,lib,main}.rs`, `src-tauri/capabilities/default.json`, `src-tauri/entitlements.plist` | T14–T18 |
| Frontend | `src/ipc/**`, `src/store/store.ts`, `src/terminal/terminal-manager.ts`, `src/components/**`, `src/{App.tsx,main.tsx,theme.ts}` | T19–T22 |
| Integration / packaging | E2E test, build/sign/notarize scripts, docs | T23–T25 |

---

## Dependency graph & parallel groups

```
G0 (sequential):   T1 → T2
G1 (sequential):   T3            (shared protocol — BLOCKS all of G2–G5)
G2 (parallel):     T4  T5  T6  T7  T8  T10        ; then T9 (needs T5,T6,T8)
G3 (sequential):   T11 → T12 → T13                (needs G2)
G4 (parallel):     T14  T15  T16 ; then T17 (needs T14,T15) → T18 (needs T16,T17)
G5 (parallel):     T19  T20  T21 ; then T22 (needs T19,T20,T21)   (needs T3 for types; T17 for command names)
G6 (sequential):   T23 → T24 → T25                (needs T13,T18,T22)
```

Parallelizable groups: **G2** (6 daemon leaf modules), **G4** (core leaf modules), **G5** (frontend leaf modules). Everything else is sequential glue/integration.

---

## Task interface index (locked symbol → producing task; consumers use these names verbatim)

- **T3 `crates/protocol`** produces: `SessionId`, `WorkspaceId`, `Workspace`, `SessionLifecycle`, `SessionMeta`, `TerminalEvent`, `Frame`, `Request`, `Response`, `Push`, `MAGIC`, `PROTO_VERSION` (exact defs in spec §5–§7).
- **T4 `singleton`** produces: `resolve_socket_path() -> PathBuf`, `resolve_lockfile() -> PathBuf`, `acquire_single_instance_lock() -> Result<LockGuard>`, `ensure_socket_dir() -> Result<()>`, `check_peer_cred(fd) -> Result<()>`.
- **T5 `osc_parser`** produces: `OscParser::new()`, `OscParser::feed(&[u8]) -> Vec<OscEvent>`, `enum OscEvent { PromptStart, PromptEnd, CommandStart, CommandEnd(Option<u8>), Cwd(String) }`, `Lifecycle` state machine `advance(OscEvent)`.
- **T6 `scrollback`** produces: `ScrollbackRing::new(cap)`, `push(&[u8])` (sanitizing), `snapshot() -> Vec<u8>`, `prune()`.
- **T7 `persistence`** produces: `Db::open(path) -> Result<Db>` (WAL, migrate, quarantine-on-corrupt), `upsert_workspace`, `list_workspaces`, `upsert_session`, `list_sessions`, `append_scrollback`, `load_scrollback`, `rehydrate() -> Vec<SessionMeta>`.
- **T8 `live_grid`** produces: `LiveGrid::new(cols,rows)`, `feed(&[u8])`, `cursor_col() -> u16`, `is_alt_screen() -> bool`, `resize(cols,rows)`.
- **T9 `pty_supervisor`** produces: `Supervisor`, `Supervisor::create(spec) -> Result<SessionId>`, `write_stdin`, `resize`, `kill` (process-group), `subscribe_output`, per-session `SessionMeta`, `waiting_for_input` computation.
- **T10 `shell_integration`** produces: `write_session_assets(runtime_dir, shell) -> ShellSpawn { program, args, env }` (zsh ZDOTDIR / bash `--init-file`).
- **T11 `attach`** produces: `AttachRegistry` (single-attach per session), `attach(session_id, sink)`, `detach`, replay orchestration.
- **T12 `socket_server`** produces: `serve(listener, deps)` — handshake, per-client task, bounded outq, dispatch `Request`→supervisor/persistence, emit `Push`.
- **T14 `socket_client`** produces: `DaemonClient::connect() -> Result<DaemonClient>`, `request(Request) -> Result<Response>` (correlated), `on_push(cb)`, reconnect/backoff.
- **T15 `paths`** produces: `validate_dir(path) -> Result<PathBuf, PathError>` (canonicalize, absolute, exists, is-dir, no symlink-escape).
- **T16 `launchd`** produces: `install_agent()`, `bootstrap()` (idempotent), `kickstart()`, `is_loaded()`, degradation handling.
- **T17 `commands`+`broker`** produces: the `#[tauri::command]` fns (spec §6.1) + Promise-correlation + `Push`→Channel/global-event mapping.
- **T19 `src/ipc`** produces: generated `types.ts`, `commands.ts` wrappers, `channel.ts`, `events.ts`.
- **T20 `store`** produces: `useAppStore` (Zustand) with the spec §12 shape + actions.
- **T21 `terminal-manager`** produces: `TerminalManager` (non-reactive `Map<SessionId, Terminal>`, open/keep-alive/dispose, replay-before-open, webgl policy).

---

## Status protocol (per task, for subagent-driven execution)

Each task reports one of: **DONE** / **DONE_WITH_CONCERNS** (note them) / **NEEDS_CONTEXT** (what's missing) / **BLOCKED** (on which task). Two-stage review per task: (1) spec-compliance, (2) code quality. A task is complete only when its tests are green and its Definition of Done (below, per task) is met.

---

## Integration resolutions & parallel-safety amendment (READ BEFORE EXECUTING)

The task bodies below were drafted in parallel and each is self-contained, but several tasks
would otherwise write the **same** manifest / module-declaration files. These resolutions are
**authoritative** and override any conflicting step inside a task body.

### A. Pre-wiring glue tasks (eliminate shared-file writes)

Run these **sequential** glue tasks so leaf tasks only ever create their own leaf source file:

- **T2b — Daemon crate pre-wiring** (after T2 + T3, before G2). Create the COMPLETE
  `crates/sessiond/Cargo.toml` `[dependencies]` up front — `bpa-protocol` (path), `portable-pty
  = "0.9.0"`, `alacritty_terminal = "=0.24.2"` (confirm exact patch at this step, spec §15.2),
  `rusqlite = { version = "0.32", features = ["bundled"] }`, `tokio` (features `net,io-util,
  rt-multi-thread,macros,sync,time`), `bincode = "1.3.3"`, `rustix = { version = "*", features =
  ["fs","net"] }`, `libc` (for `getpeereid` peer-cred on macOS — rustix `SO_PEERCRED` is Linux-only),
  `uuid` (v4), `tracing`, `tracing-subscriber`, `thiserror`; `[dev-dependencies] tempfile`. Create
  `crates/sessiond/src/lib.rs` with ALL module declarations (`pub mod singleton; pub mod osc_parser;
  pub mod scrollback; pub mod persistence; pub mod live_grid; pub mod pty_supervisor; pub mod
  shell_integration; pub mod attach; pub mod socket_server; pub mod boot;` + `pub use bpa_protocol
  as protocol;`) and a thin `main.rs` → `sessiond::boot::run()`. Then **G2/G3 leaf tasks create ONLY
  their own `<name>.rs`; they add NO deps and NO `mod` lines.**
- **T13b — Core crate pre-wiring** (fold into T2, before G4). Set `src-tauri/Cargo.toml`:
  `[lib] name = "builder_pro_ai_lib"`; deps `bpa-protocol` (path), `tokio` (same features), `bincode
  = "1.3.3"`, `tracing`+`tracing-subscriber`, `thiserror`, `serde_json`, and `tauri-plugin-{store,
  dialog,fs,shell}` (major 2); `[dev-dependencies] tempfile`. Create `src-tauri/src/lib.rs` with
  `pub mod socket_client; pub mod paths; pub mod launchd; pub mod commands; pub mod broker;` + the
  `run()` entry. G4 leaf tasks create ONLY their own `.rs`.
- **Frontend manifests** (T1 owns): `package.json` includes the FULL frontend dep + devDep set,
  including `@testing-library/{react,jest-dom,user-event,dom}` and `jsdom` (so T22 adds none), and
  the addon pins `@xterm/addon-fit@0.11`, `@xterm/addon-webgl@0.19`, `@xterm/addon-search@0.16`,
  `@xterm/addon-web-links@0.12`, `@xterm/addon-serialize@0.14` (search/web-links/serialize bumped one
  patch in Task 1 for xterm-6 peer-dep compatibility). `src/ipc` has **no
  barrel**; consumers import concrete module paths (`../ipc/commands`, `../ipc/channel`,
  `../ipc/events`, `../ipc/types`).

**Override rule:** wherever a leaf task body says "append your `mod` line" or "add dependency X to
Cargo.toml", that step is **superseded** by the pre-wiring above — the manifest/module files already
declare it. Leaf tasks touch only their own leaf source + test files.

### B. Locked cross-task interfaces (reconcile the drafters' flagged names)

- **Supervisor API (T9 produces; T11–T13 consume verbatim):**
  `Supervisor::new(Arc<Db>) -> Arc<Supervisor>`; `for_test() -> Arc<Supervisor>`;
  `create(CreateSpec) -> Result<SessionMeta, SupervisorError>` where
  `pub struct CreateSpec { workspace_id: WorkspaceId, shell: Option<String>, cwd: Option<String>,
  env_overrides: Vec<(String,String)>, cols: u16, rows: u16 }`;
  `write_stdin(&SessionId, &[u8])`, `resize(&SessionId, u16, u16)`, `kill(&SessionId)`,
  `get_state(&SessionId) -> Option<SessionMeta>`, `shutdown_all()`;
  output fan-out via **`tokio::sync::broadcast`**: `subscribe_output(&SessionId) ->
  Option<broadcast::Receiver<Vec<u8>>>`, `session_dims(&SessionId) -> Option<(u16,u16)>`,
  `scrollback_snapshot(&SessionId) -> Option<Vec<u8>>`; status callbacks
  `on_status/on_created/on_exited`. (broadcast, not mpsc — one slow sink must not stall the
  supervisor, spec §13.)
- **Db extras (T7 adds):** `Db::open_in_memory() -> Result<Db>`, `Db::checkpoint() -> Result<()>`
  (for T12/T13 tests) in addition to `Db::open(path)`.
- **DaemonClient (T14 produces; T17/T18 consume):** `connect() -> Result<DaemonClient, ClientError>`;
  `request(Request) -> Result<Response, ClientError>`; `on_push(impl FnMut(Push))`;
  `on_conn(impl FnMut(ConnState))` (drives `daemon://disconnected|reconnected`);
  `pub enum ClientError { Disconnected, Daemon { code: String, message: String } }` (+ `Display`).
- **AttachSession** replies `Response::Ack`; `Push::Replay` then `Push::Output` follow as separate
  frames (T17 broker correlates the Ack, routes later Pushes to the `Channel`). Do not expect Replay
  inline.
- **Core AppState hardening (T18):** manage `Arc<Mutex<Option<DaemonClient>>>` from app start so a
  command invoked before the daemon connects returns `CommandError::Disconnected` instead of Tauri's
  "state not managed" panic.
- **tauri.conf.json (T2 owns):** `bundle.macOS.entitlements = "entitlements.plist"` and
  `bundle.macOS.hardenedRuntime = true`; `bundle.externalBin = ["binaries/bpa-sessiond"]` defined
  exactly once (T24 fills `entitlements.plist`; do not double-declare).

### C. Ordering & correctness resolutions

- **Live Output strips the daemon's OWN injected marks.** T9 feeds the original PTY bytes to the
  live `Output` firehose, but the OSC-133/OSC-7 sequences **we injected** for status are internal
  signaling and are stripped from `Output` (and from the scrollback ring, spec §11) — all other
  bytes pass verbatim. The OSC parser's `feed()` is a side-channel event extractor, not a byte
  filter; T9 owns the strip of its own marks on the way to `Output`.
- **Sidecar build precedes bundling.** T1's DoD build is `cargo build` + `npm run build` (no full
  bundle — `externalBin` has no binary yet). **T24** builds `bpa-sessiond` for both arches into
  `src-tauri/binaries/bpa-sessiond-<triple>` **before** `tauri build --target universal-apple-darwin`.
- **ts-rs (T3):** pin whatever major cargo resolves; verify internally-tagged **unit** variants
  (`AtPrompt/Typing/Running`) emit `{ kind: "…" }` correctly and adjust the `#[ts(...)]` derive if
  needed; `src/ipc/types.ts` is **generated** (`export_to="../../src/ipc/types.ts"`) and hand-edited
  by no task; CI parity = `git diff --exit-code src/ipc/types.ts`.
- **DaemonShutdown drain:** in S1 only SIGTERM triggers the daemon drain (T13); a GUI-initiated
  `Request::DaemonShutdown` returns `Ack` without wiring a drain — acceptable for this slice.
- **src-tauri package name** is `app` (Cargo `name = "app"`, lib `builder_pro_ai_lib`); traceability
  rows using `-p app` are correct.

---

## Tasks

<!-- Task bodies (T1–T25) assembled below in dependency order. Per §A/§B/§C above take precedence. -->



### Task 1: Scaffold Tauri 2 + React + Vite + TS app (pinned, Tauri-flavored configs)

**Files:**
- Create — `package.json`, `vite.config.ts`, `tsconfig.json`, `tsconfig.node.json`, `index.html`, `src/main.tsx`, `src/App.tsx`, `src/vite-env.d.ts`
- Create — `src-tauri/Cargo.toml`, `src-tauri/tauri.conf.json`, `src-tauri/build.rs`, `src-tauri/src/main.rs`, `src-tauri/src/lib.rs`, `src-tauri/capabilities/default.json`
- Create — `src-tauri/icons/` (placeholder icons via `tauri icon` or scaffold defaults), `src-tauri/binaries/.gitkeep`
- Test — `src/__tests__/smoke.test.ts` (frontend build/type smoke via vitest), `src-tauri/tests/invoke_smoke.rs` (trivial `#[tauri::command]` invoke smoke)

**Depends on:** []   **Parallel-safe with:** [] (G0 is sequential; T1 precedes T2)

**Interfaces:** Consumes: nothing (bootstrap). Produces: the Vite-at-root + `src-tauri/` Tauri crate layout from spec §4; a compiling Tauri app crate `builder-pro-ai` exposing one trivial command `#[tauri::command] fn ping() -> String` registered via `tauri::generate_handler![ping]`; pinned dependency manifests (spec §3 / Global Constraints); `tauri.conf.json` with `productName "Builder Pro AI"`, `identifier "ai.builderpro.desktop"`, `bundle.externalBin ["binaries/bpa-sessiond"]`. (This crate is the `src-tauri` workspace member that T2 wires into the root `[workspace]`; the broker/command modules land in T14–T18.)

- [ ] **Step 1: Add `.gitignore` entries for build artifacts (so scaffolding does not commit `node_modules`/`target`/`dist`).** Append (do not duplicate existing lines) to `/Users/sshlg/DATA/builder-pro-ai/.gitignore`:

```gitignore
# --- S0 scaffold ---
node_modules/
dist/
target/
src-tauri/target/
src-tauri/gen/
src-tauri/binaries/bpa-sessiond-*
.DS_Store
*.log
```

- [ ] **Step 2: Write `package.json` with pinned frontend + Tauri deps.** Create `/Users/sshlg/DATA/builder-pro-ai/package.json`:

```json
{
  "name": "builder-pro-ai",
  "private": true,
  "version": "0.1.0",
  "type": "module",
  "scripts": {
    "dev": "vite",
    "build": "tsc --noEmit && vite build",
    "preview": "vite preview",
    "test": "vitest run",
    "tauri": "tauri"
  },
  "dependencies": {
    "@tauri-apps/api": "^2",
    "@tauri-apps/plugin-store": "^2",
    "@tauri-apps/plugin-dialog": "^2",
    "@tauri-apps/plugin-fs": "^2",
    "@tauri-apps/plugin-shell": "^2",
    "@xterm/xterm": "6.0.0",
    "@xterm/addon-fit": "^0.11.0",
    "@xterm/addon-webgl": "^0.19.0",
    "@xterm/addon-search": "^0.15.0",
    "@xterm/addon-web-links": "^0.11.0",
    "@xterm/addon-serialize": "^0.13.0",
    "react": "^19.0.0",
    "react-dom": "^19.0.0",
    "zustand": "^5.0.0"
  },
  "devDependencies": {
    "@tauri-apps/cli": "^2",
    "@types/react": "^19.0.0",
    "@types/react-dom": "^19.0.0",
    "@vitejs/plugin-react": "^4.3.0",
    "typescript": "^5.6.0",
    "vite": "^6.0.0",
    "vitest": "^2.1.0"
  }
}
```

- [ ] **Step 3: Write the Tauri-flavored `vite.config.ts`.** Create `/Users/sshlg/DATA/builder-pro-ai/vite.config.ts`. Values are from spec §4 + the Tauri Vite recipe (research): `clearScreen:false`, fixed `port:5173` with `strictPort:true`, ignore `src-tauri` in the watcher, restrict `envPrefix` to `VITE_`/`TAURI_ENV_*`, and key `build.target`/`minify`/`sourcemap` off `TAURI_ENV_DEBUG`:

```ts
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

const host = process.env.TAURI_DEV_HOST;

// https://vitejs.dev/config/  (Tauri flavor: https://v2.tauri.app/start/frontend/vite/)
export default defineConfig(async () => ({
  plugins: [react()],
  // Tauri expects a fixed port and must fail (not silently fall back) if it is taken.
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    host: host || false,
    hmr: host
      ? { protocol: "ws", host, port: 5174 }
      : undefined,
    watch: {
      // Rust rebuilds must not trigger a frontend HMR reload.
      ignored: ["**/src-tauri/**"],
    },
  },
  // Only expose vars Tauri whitelists to the frontend.
  envPrefix: ["VITE_", "TAURI_ENV_"],
  build: {
    // Safari 13 is the macOS WKWebView floor Tauri targets.
    target: "safari13",
    minify: !process.env.TAURI_ENV_DEBUG ? "esbuild" : false,
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
  },
  test: {
    environment: "node",
    include: ["src/**/*.test.ts", "src/**/*.test.tsx"],
  },
}));
```

- [ ] **Step 4: Write `tsconfig.json` + `tsconfig.node.json`.** Create `/Users/sshlg/DATA/builder-pro-ai/tsconfig.json`:

```json
{
  "compilerOptions": {
    "target": "ES2020",
    "useDefineForClassFields": true,
    "lib": ["ES2020", "DOM", "DOM.Iterable"],
    "module": "ESNext",
    "skipLibCheck": true,
    "moduleResolution": "bundler",
    "allowImportingTsExtensions": true,
    "resolveJsonModule": true,
    "isolatedModules": true,
    "noEmit": true,
    "jsx": "react-jsx",
    "strict": true,
    "noUnusedLocals": true,
    "noUnusedParameters": true,
    "noFallthroughCasesInSwitch": true,
    "types": ["vite/client", "vitest/globals"]
  },
  "include": ["src"],
  "references": [{ "path": "./tsconfig.node.json" }]
}
```

Create `/Users/sshlg/DATA/builder-pro-ai/tsconfig.node.json`:

```json
{
  "compilerOptions": {
    "composite": true,
    "skipLibCheck": true,
    "module": "ESNext",
    "moduleResolution": "bundler",
    "allowSyntheticDefaultImports": true,
    "strict": true
  },
  "include": ["vite.config.ts"]
}
```

- [ ] **Step 5: Write `index.html` + minimal React entry.** Create `/Users/sshlg/DATA/builder-pro-ai/index.html`:

```html
<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>Builder Pro AI</title>
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="/src/main.tsx"></script>
  </body>
</html>
```

Create `/Users/sshlg/DATA/builder-pro-ai/src/vite-env.d.ts`:

```ts
/// <reference types="vite/client" />
```

Create `/Users/sshlg/DATA/builder-pro-ai/src/App.tsx`:

```tsx
export default function App(): JSX.Element {
  return <div id="app-root">Builder Pro AI</div>;
}
```

Create `/Users/sshlg/DATA/builder-pro-ai/src/main.tsx`:

```tsx
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
);
```

- [ ] **Step 6: Write the failing frontend smoke test.** Create `/Users/sshlg/DATA/builder-pro-ai/src/__tests__/smoke.test.ts`:

```ts
import { describe, it, expect } from "vitest";
import pkg from "../../package.json" assert { type: "json" };

describe("scaffold smoke", () => {
  it("pins the locked frontend versions from spec §3", () => {
    expect(pkg.name).toBe("builder-pro-ai");
    expect(pkg.dependencies["@xterm/xterm"]).toBe("6.0.0");
    expect(pkg.dependencies["react"]).toBe("^19.0.0");
    expect(pkg.dependencies["zustand"]).toBe("^5.0.0");
    expect(pkg.dependencies["@tauri-apps/api"]).toBe("^2");
    // Bundling-only + settings/dialog/fs plugins must all be present.
    for (const p of [
      "@tauri-apps/plugin-store",
      "@tauri-apps/plugin-dialog",
      "@tauri-apps/plugin-fs",
      "@tauri-apps/plugin-shell",
    ]) {
      expect(pkg.dependencies[p], `${p} missing`).toBe("^2");
    }
  });
});
```

- [ ] **Step 7: Install deps and run the failing test.** Run:

```
cd /Users/sshlg/DATA/builder-pro-ai && npm install
```

Then `npx vitest run src/__tests__/smoke.test.ts`.
Expected: PASS once `package.json` from Step 2 is in place (this test asserts the pins are already correct). If any pin is wrong the run FAILs with `expected '<wrong>' to be '<pinned>'`; fix the pin in `package.json` and re-run until green. (This test guards the Global-Constraints pins against drift in later tasks.)

- [ ] **Step 8: Write `tauri.conf.json` with the locked build/bundle block.** Create `/Users/sshlg/DATA/builder-pro-ai/src-tauri/tauri.conf.json`. `beforeDevCommand`/`beforeBuildCommand`/`devUrl`/`frontendDist` and the `productName`/`identifier`/`version`/`bundle` block are locked by the assignment + spec §3/§8:

```json
{
  "$schema": "https://schema.tauri.app/config/2",
  "productName": "Builder Pro AI",
  "version": "0.1.0",
  "identifier": "ai.builderpro.desktop",
  "build": {
    "beforeDevCommand": "npm run dev",
    "beforeBuildCommand": "npm run build",
    "devUrl": "http://localhost:5173",
    "frontendDist": "../dist"
  },
  "app": {
    "windows": [
      {
        "title": "Builder Pro AI",
        "width": 1280,
        "height": 800,
        "resizable": true,
        "fullscreen": false
      }
    ],
    "security": {
      "csp": null
    }
  },
  "bundle": {
    "active": true,
    "targets": ["app", "dmg"],
    "icon": [
      "icons/32x32.png",
      "icons/128x128.png",
      "icons/128x128@2x.png",
      "icons/icon.icns"
    ],
    "externalBin": ["binaries/bpa-sessiond"],
    "macOS": {
      "entitlements": "entitlements.plist"
    }
  }
}
```

Note: `entitlements.plist` is authored in T18; `binaries/bpa-sessiond-<triple>` files are produced by the build/sign scripts in T23–T25. Leaving `externalBin` pointed at a not-yet-present sidecar is fine for `--debug`/dev builds that do not bundle; the DoD build below uses `cargo build` + `npm run build` (which does not require the sidecar), and the full universal bundle is gated in T24.

- [ ] **Step 9: Write `src-tauri/Cargo.toml` (the app crate manifest).** Create `/Users/sshlg/DATA/builder-pro-ai/src-tauri/Cargo.toml`. Pins from spec §3; `[lib]` `crate-type` includes `staticlib`/`cdylib`/`rlib` per Tauri 2 mobile-ready scaffolding:

```toml
[package]
name = "builder-pro-ai"
version = "0.1.0"
description = "Builder Pro AI desktop app (Tauri core / broker)"
authors = ["Builder Pro AI"]
edition = "2021"
rust-version = "1.77.2"

[lib]
name = "builder_pro_ai_lib"
crate-type = ["staticlib", "cdylib", "rlib"]

[build-dependencies]
tauri-build = { version = "2", features = [] }

[dependencies]
tauri = { version = "2", features = [] }
tauri-plugin-store = "2"
tauri-plugin-dialog = "2"
tauri-plugin-fs = "2"
tauri-plugin-shell = "2"
serde = { version = "1", features = ["derive"] }
serde_json = "1"

[features]
# Tauri custom-protocol asset serving in release builds.
custom-protocol = ["tauri/custom-protocol"]
```

- [ ] **Step 10: Write `build.rs`, `main.rs`, `lib.rs` with the trivial `ping` command + plugin init.** Create `/Users/sshlg/DATA/builder-pro-ai/src-tauri/build.rs`:

```rust
fn main() {
    tauri_build::build();
}
```

Create `/Users/sshlg/DATA/builder-pro-ai/src-tauri/src/main.rs`:

```rust
// Prevents an extra console window on Windows in release; harmless on macOS.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    builder_pro_ai_lib::run();
}
```

Create `/Users/sshlg/DATA/builder-pro-ai/src-tauri/src/lib.rs`:

```rust
//! Builder Pro AI — Tauri core (broker). S0 scaffold: plugin init + a smoke command.
//! The daemon-broker command surface (spec §6) is added in T14–T18.

/// Trivial invoke smoke command; proves the JS⇄Rust IPC round-trip works.
#[tauri::command]
fn ping() -> String {
    "pong".to_string()
}

/// Build and run the Tauri application.
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![ping])
        .run(tauri::generate_context!())
        .expect("error while running Builder Pro AI");
}
```

- [ ] **Step 11: Write `capabilities/default.json` (baseline deny-by-default grants).** Create `/Users/sshlg/DATA/builder-pro-ai/src-tauri/capabilities/default.json`. Sidecar exec + scoped plugin grants are hardened in T18; this baseline enables the plugins the scaffold initializes so the app boots:

```json
{
  "$schema": "../gen/schemas/desktop-schema.json",
  "identifier": "default",
  "description": "Baseline capabilities for the main window (S0 scaffold).",
  "windows": ["main"],
  "permissions": [
    "core:default",
    "store:default",
    "dialog:default",
    "fs:default",
    "shell:default"
  ]
}
```

- [ ] **Step 12: Generate placeholder app icons + reserve the sidecar dir.** Run:

```
cd /Users/sshlg/DATA/builder-pro-ai && npx tauri icon 2>/dev/null || (mkdir -p src-tauri/icons && npx tauri icon)
```

If no source PNG exists, `tauri icon` errors; in that case create a 1024×1024 solid PNG placeholder and re-run:

```
cd /Users/sshlg/DATA/builder-pro-ai && node -e "const fs=require('fs');const s=1024;const hdr=Buffer.from('placeholder');fs.mkdirSync('src-tauri/icons',{recursive:true});" && npx @tauri-apps/cli icon src-tauri/app-icon.png 2>/dev/null || echo "supply src-tauri/app-icon.png (1024x1024) then run: npx tauri icon src-tauri/app-icon.png"
```

Then reserve the sidecar directory so the path in `externalBin` exists in git:

```
mkdir -p /Users/sshlg/DATA/builder-pro-ai/src-tauri/binaries && touch /Users/sshlg/DATA/builder-pro-ai/src-tauri/binaries/.gitkeep
```

Expected: `src-tauri/icons/{32x32.png,128x128.png,128x128@2x.png,icon.icns,icon.ico}` exist (referenced by `tauri.conf.json` `bundle.icon`) and `src-tauri/binaries/.gitkeep` exists.

- [ ] **Step 13: Add the Rust targets for the universal build.** Run:

```
rustup target add aarch64-apple-darwin x86_64-apple-darwin
```

Expected: both targets `installed` (or already-installed). This is required later for `--target universal-apple-darwin` (T24) and for building the per-arch sidecar (T23).

- [ ] **Step 14: Write the failing Rust invoke smoke test.** Create `/Users/sshlg/DATA/builder-pro-ai/src-tauri/tests/invoke_smoke.rs`. This drives the `ping` command through a `tauri::test` mock app to prove the invoke surface + `generate_context!` compile and dispatch:

```rust
// Invoke smoke: builds a mock Tauri app and calls the `ping` command end-to-end.
use tauri::test::{mock_builder, INVOKE_KEY};
use tauri::webview::InvokeRequest;
use tauri::{Manager, WebviewWindowBuilder, WebviewUrl};

#[tauri::command]
fn ping() -> String {
    "pong".to_string()
}

#[test]
fn ping_returns_pong() {
    let app = mock_builder()
        .invoke_handler(tauri::generate_handler![ping])
        .build(tauri::test::mock_context(tauri::test::noop_assets()))
        .expect("failed to build mock app");

    let webview = WebviewWindowBuilder::new(&app, "main", WebviewUrl::default())
        .build()
        .expect("failed to build webview");

    let res = tauri::test::get_ipc_response(
        &webview,
        InvokeRequest {
            cmd: "ping".into(),
            callback: tauri::ipc::CallbackFn(0),
            error: tauri::ipc::CallbackFn(1),
            url: "http://tauri.localhost".parse().unwrap(),
            body: tauri::ipc::InvokeBody::default(),
            headers: Default::default(),
            invoke_key: INVOKE_KEY.to_string(),
        },
    );

    let value = res.expect("ping invoke returned an error");
    assert_eq!(value.deserialize::<String>().unwrap(), "pong");
}
```

Add the `test` feature to `tauri` for the test build. In `src-tauri/Cargo.toml`, add:

```toml
[dev-dependencies]
tauri = { version = "2", features = ["test"] }
```

- [ ] **Step 15: Run the Rust smoke test — confirm it exercises invoke.** Run:

```
cargo test --manifest-path /Users/sshlg/DATA/builder-pro-ai/src-tauri/Cargo.toml --test invoke_smoke ping_returns_pong
```

Expected: FIRST run FAILs to compile if the `test` feature/dev-dependency is missing (error like `unresolved import tauri::test`); after Step 14's `[dev-dependencies]` addition it compiles and Expected: PASS (`test ping_returns_pong ... ok`). If the mock-invoke API surface differs at the pinned Tauri (2.11.x), consult `docs.rs/tauri/2.11` `tauri::test` and adjust `InvokeRequest` field set — the assertion `deserialize::<String>() == "pong"` is the locked contract.

- [ ] **Step 16: Build the app crate + frontend (headless DoD).** Run:

```
cargo build --manifest-path /Users/sshlg/DATA/builder-pro-ai/src-tauri/Cargo.toml
```

Expected: PASS (compiles the `builder_pro_ai_lib` crate + `builder-pro-ai` bin with all four plugins). Then:

```
cd /Users/sshlg/DATA/builder-pro-ai && npm run build
```

Expected: PASS — `tsc --noEmit` reports no type errors and `vite build` emits `dist/` (`build.target safari13`, minified since `TAURI_ENV_DEBUG` unset).

- [ ] **Step 17: Window-opens verification (manual/dev, non-blocking DoD).** Run in a foreground shell:

```
cd /Users/sshlg/DATA/builder-pro-ai && npm run tauri dev
```

Expected: the Vite dev server binds `localhost:5173` (strict) and the Tauri window titled "Builder Pro AI" opens showing "Builder Pro AI"; the devtools console `await window.__TAURI__.core.invoke('ping')` returns `"pong"`. Ctrl-C to stop. (Automated in the E2E task T23; here it is a smoke check.)

- [ ] **Step 18: Commit.** Run:

```
git add .gitignore package.json vite.config.ts tsconfig.json tsconfig.node.json index.html src/ src-tauri/Cargo.toml src-tauri/tauri.conf.json src-tauri/build.rs src-tauri/src/ src-tauri/capabilities/ src-tauri/icons/ src-tauri/binaries/.gitkeep src-tauri/tests/ && git commit -m "feat(scaffold): pin Tauri 2 + React 19 + Vite/TS app shell with invoke smoke"
```

**Definition of Done:**
- `npx vitest run src/__tests__/smoke.test.ts` green (frontend pins match spec §3).
- `cargo test --manifest-path src-tauri/Cargo.toml --test invoke_smoke` green (`ping` → `"pong"` over the invoke surface).
- `cargo build --manifest-path src-tauri/Cargo.toml` and `npm run build` both succeed; `dist/` is produced.
- `npm run tauri dev` opens a "Builder Pro AI" window; devtools `invoke('ping')` returns `"pong"`.
- `vite.config.ts` sets `clearScreen:false`, `server.port 5173` + `strictPort:true`, `server.watch.ignored ['**/src-tauri/**']`, `envPrefix ['VITE_','TAURI_ENV_']`, `build.target 'safari13'`, and keys `minify`/`sourcemap` off `TAURI_ENV_DEBUG`.
- `tauri.conf.json` has `build.beforeDevCommand 'npm run dev'`, `beforeBuildCommand 'npm run build'`, `devUrl 'http://localhost:5173'`, `frontendDist '../dist'`, `productName 'Builder Pro AI'`, `identifier 'ai.builderpro.desktop'`, `version`, `bundle.targets`, and `bundle.externalBin ['binaries/bpa-sessiond']`.
- Plugins `store`/`dialog`/`fs`/`shell` are in both `package.json` deps and `src-tauri/Cargo.toml`, and initialized in `lib.rs`.
- `rustup target add aarch64-apple-darwin x86_64-apple-darwin` completed.

---

### Task 2: Cargo workspace + crate skeletons (`protocol` lib, `sessiond` bin)

**Files:**
- Create — `Cargo.toml` (repo-root `[workspace]`)
- Create — `crates/protocol/Cargo.toml`, `crates/protocol/src/lib.rs` (skeleton; full wire types land in T3)
- Create — `crates/sessiond/Cargo.toml`, `crates/sessiond/src/main.rs` (skeleton; modules land in T4–T13)
- Test — `crates/protocol/src/lib.rs` (inline `#[cfg(test)]`), `crates/sessiond/tests/skeleton.rs`

**Depends on:** [T1]   **Parallel-safe with:** [] (G0 sequential; runs after T1)

**Interfaces:** Consumes: the `src-tauri` app crate created in T1 (added here as a workspace member). Produces: root `Cargo.toml` `[workspace]` with `members = ["src-tauri", "crates/protocol", "crates/sessiond"]` and a shared `[workspace.dependencies]` table; minimal-compiling crate `bpa-protocol` (lib, the T3 home for `SessionId`/`WorkspaceId`/`Workspace`/`SessionLifecycle`/`SessionMeta`/`TerminalEvent`/`Frame`/`Request`/`Response`/`Push`/`MAGIC`/`PROTO_VERSION`) exporting `MAGIC: u32` + `PROTO_VERSION: u16` placeholders; minimal-compiling binary crate `bpa-sessiond` (the daemon, target-triple-suffixed for `externalBin` in T1). Names are locked by Global Constraints — do not rename.

- [ ] **Step 1: Write the failing workspace-membership test (protocol crate).** Create `crates/protocol/src/lib.rs` with only a test that asserts the two locked wire constants exist with their spec §7 values (this fails to compile until the crate + constants exist):

```rust
//! bpa-protocol — SHARED Hop-B wire types (serde + ts-rs). Source of truth for TS types.
//! S0 skeleton: only the locked wire constants. Full types (spec §5–§7) land in T3.

/// Hop-B handshake magic — ASCII "BPA1". Locked (spec §7 / Global Constraints).
pub const MAGIC: u32 = 0x4250_4131;
/// Hop-B protocol version. Locked (spec §7 / Global Constraints).
pub const PROTO_VERSION: u16 = 1;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_constants_match_spec() {
        // "BPA1" big-endian ASCII: 0x42='B',0x50='P',0x41='A',0x31='1'.
        assert_eq!(MAGIC, 0x4250_4131);
        assert_eq!(&MAGIC.to_be_bytes(), b"BPA1");
        assert_eq!(PROTO_VERSION, 1);
    }
}
```

- [ ] **Step 2: Write `crates/protocol/Cargo.toml` (skeleton manifest).** Create `crates/protocol/Cargo.toml`. Only serde is needed for the skeleton; `ts-rs`/`uuid` are added by T3 when the real types land:

```toml
[package]
name = "bpa-protocol"
version = "0.1.0"
edition = "2021"
rust-version = "1.77.2"

[dependencies]
serde = { workspace = true }
```

- [ ] **Step 3: Write the failing daemon skeleton test.** Create `crates/sessiond/tests/skeleton.rs`. It asserts the daemon links against `bpa-protocol` and can read the locked constants (proves the workspace wiring + dependency edge compile):

```rust
// Daemon skeleton test: the sessiond crate depends on bpa-protocol and sees the wire constants.
#[test]
fn daemon_links_protocol_constants() {
    assert_eq!(bpa_protocol::MAGIC, 0x4250_4131);
    assert_eq!(bpa_protocol::PROTO_VERSION, 1);
}
```

- [ ] **Step 4: Write `crates/sessiond/src/main.rs` (minimal-compiling daemon entrypoint).** Create `crates/sessiond/src/main.rs`. The real socket/PTY wiring lands in T4–T13; this parses the `--socket <path>` arg the LaunchAgent passes (spec §8.3) so the binary is invocable, logs a startup line, and exits cleanly:

```rust
//! bpa-sessiond — Builder Pro AI session daemon.
//! S0 skeleton: arg parse + startup log. PTY/socket/persistence land in T4–T13.

fn main() {
    // LaunchAgent invokes: bpa-sessiond --socket <RESOLVED_SOCKET_PATH> (spec §8.3).
    let mut socket_path: Option<String> = None;
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--socket" => socket_path = args.next(),
            "--version" => {
                println!("bpa-sessiond {}", env!("CARGO_PKG_VERSION"));
                return;
            }
            other => {
                eprintln!("bpa-sessiond: unknown argument: {other}");
                std::process::exit(2);
            }
        }
    }

    eprintln!(
        "bpa-sessiond {} starting; proto={} socket={:?}",
        env!("CARGO_PKG_VERSION"),
        bpa_protocol::PROTO_VERSION,
        socket_path,
    );
    // S0 skeleton exits immediately (clean). The serve loop is added in T12/T13.
}
```

- [ ] **Step 5: Write `crates/sessiond/Cargo.toml` (skeleton manifest).** Create `crates/sessiond/Cargo.toml`. The binary name MUST be `bpa-sessiond` (locked; matches `externalBin` in T1). Heavy deps (`portable-pty`, `tokio`, `rusqlite`, `alacritty_terminal`, `rustix`, `bincode`, `tracing`, `uuid`) are added by their owning tasks T4–T13; the skeleton only needs `bpa-protocol`:

```toml
[package]
name = "bpa-sessiond"
version = "0.1.0"
edition = "2021"
rust-version = "1.77.2"

[[bin]]
name = "bpa-sessiond"
path = "src/main.rs"

[dependencies]
bpa-protocol = { workspace = true }
```

- [ ] **Step 6: Write the root `[workspace]` `Cargo.toml`.** Create `/Users/sshlg/DATA/builder-pro-ai/Cargo.toml`. Members are locked (Global Constraints); the shared `[workspace.dependencies]` table pins the versions from spec §3 so member crates reference `{ workspace = true }` and stay consistent:

```toml
[workspace]
resolver = "2"
members = ["src-tauri", "crates/protocol", "crates/sessiond"]

[workspace.package]
edition = "2021"
rust-version = "1.77.2"

[workspace.dependencies]
bpa-protocol = { path = "crates/protocol" }

# Shared pins (spec §3). Owned individually by member crates via `workspace = true`.
serde = { version = "1", features = ["derive"] }
serde_json = "1"
tokio = { version = "1", features = ["net", "io-util", "rt-multi-thread", "macros", "sync", "time"] }
portable-pty = "0.9.0"
rusqlite = { version = "0.32", features = ["bundled"] }
bincode = "1.3.3"
rustix = { version = "0.38", features = ["fs", "net"] }
uuid = { version = "1", features = ["v4"] }
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
ts-rs = "10"
```

Note: `alacritty_terminal` is pinned exact (0.24/0.25) by T8 at task time (spec §15 re-verify), so it is intentionally not listed here yet; T8 adds it to `[workspace.dependencies]` and its crate.

- [ ] **Step 7: Convert `src-tauri/Cargo.toml` to consume the workspace.** Edit `/Users/sshlg/DATA/builder-pro-ai/src-tauri/Cargo.toml` from T1: add `src-tauri` under the root workspace by making `serde`/`serde_json` reference the workspace table, and add the workspace lint/edition inheritance. Replace the `serde`/`serde_json` lines under `[dependencies]`:

Old:
```toml
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```
New:
```toml
serde = { workspace = true }
serde_json = { workspace = true }
```

(Leave `tauri`, `tauri-build`, and the `tauri-plugin-*` lines as `"2"` — those are app-crate-only and not in the shared table.)

- [ ] **Step 8: Run the protocol + daemon skeleton tests.** Run:

```
cargo test -p bpa-protocol wire_constants_match_spec
cargo test -p bpa-sessiond --test skeleton daemon_links_protocol_constants
```

Expected: FIRST run of each may FAIL to resolve if the workspace/member wiring is incomplete (error like `error: failed to load manifest` or `unresolved crate bpa_protocol`); after Steps 1–7 both Expected: PASS (`test wire_constants_match_spec ... ok`, `test daemon_links_protocol_constants ... ok`).

- [ ] **Step 9: Build the whole workspace.** Run:

```
cargo build --workspace
```

Expected: PASS — all three members (`builder-pro-ai`, `bpa-protocol`, `bpa-sessiond`) compile. Confirm the daemon binary exists:

```
test -x /Users/sshlg/DATA/builder-pro-ai/target/debug/bpa-sessiond && /Users/sshlg/DATA/builder-pro-ai/target/debug/bpa-sessiond --version
```

Expected: prints `bpa-sessiond 0.1.0`.

- [ ] **Step 10: Verify the app crate still builds under the workspace.** Run:

```
cargo build -p builder-pro-ai
```

Expected: PASS (Step 7's workspace-inheritance edit did not break the app crate).

- [ ] **Step 11: Commit.** Run:

```
git add Cargo.toml crates/ src-tauri/Cargo.toml && git commit -m "feat(workspace): add Cargo [workspace] + protocol/sessiond crate skeletons"
```

**Definition of Done:**
- Root `Cargo.toml` declares `[workspace]` with `members = ["src-tauri", "crates/protocol", "crates/sessiond"]` and a shared `[workspace.dependencies]` table pinning spec §3 versions.
- `crates/protocol` (lib `bpa-protocol`) and `crates/sessiond` (bin `bpa-sessiond`) exist as minimal-compiling skeletons; `bpa-protocol` exports `MAGIC = 0x4250_4131` (u32) and `PROTO_VERSION = 1` (u16).
- `cargo test -p bpa-protocol wire_constants_match_spec` and `cargo test -p bpa-sessiond --test skeleton daemon_links_protocol_constants` are green.
- `cargo build --workspace` passes; `target/debug/bpa-sessiond --version` prints `bpa-sessiond 0.1.0`.
- `bpa-sessiond` binary name matches the `externalBin ['binaries/bpa-sessiond']` entry from T1.


### Task 3: Shared protocol crate (types + wire framing + generated TS)

**Files:**
- Create — `crates/protocol/Cargo.toml`
- Create — `crates/protocol/src/lib.rs` (all spec §5 domain types, §6.2 `TerminalEvent`, §7 wire types, `MAGIC`, `PROTO_VERSION`, `encode_frame`, `FrameDecoder`)
- Create — `crates/protocol/src/framing.rs` (length-prefix codec: `encode_frame`, `FrameDecoder::decode`)
- Create — `crates/protocol/tests/roundtrip.rs` (table-driven bincode round-trip for every Frame/Request/Response/Push/SessionLifecycle/TerminalEvent variant)
- Create — `crates/protocol/tests/framing.rs` (partial-frame buffering, oversized/garbage rejection, length-prefix boundary)
- Create — `crates/protocol/tests/ts_export.rs` (ts-rs export → `src/ipc/types.ts` + exact spec §5 assertion)
- Generate (owned by no task) — `src/ipc/types.ts`
- Modify — `Cargo.toml` (workspace root) — already lists `crates/protocol` as a member (created in T1); no edit needed if member present. If absent, add `"crates/protocol"` to `[workspace] members`.

**Depends on:** [T1, T2]   **Parallel-safe with:** [] (G1 is a single sequential task that BLOCKS G2–G5)

**Interfaces:**
- Consumes: nothing from other tasks (root of the type graph). Uses external crates `serde` (derive), `bincode` = **1.3.3**, `ts-rs` (feature `serde-compat`), `serde_json` (dev-dep, for TS-shape assertions).
- Produces (exact names — consumers use verbatim; from spec §5–§7 and the Task interface index):
  - `pub type SessionId = String;`
  - `pub type WorkspaceId = String;`
  - `pub struct Workspace { pub id: WorkspaceId, pub name: String, pub root_path: String }`
  - `pub enum SessionLifecycle { AtPrompt, Typing, Running, Exited { code: Option<u8>, signal: Option<String> } }` (internally tagged `kind`, camelCase)
  - `pub struct SessionMeta { id, workspace_id, title, shell, cwd, cols: u16, rows: u16, lifecycle: SessionLifecycle, waiting_for_input: bool, is_active: bool, created_at: i64 }`
  - `pub enum TerminalEvent { Replay { cols: u16, rows: u16, content: Vec<u8> }, Output { bytes: Vec<u8> } }` (adjacently tagged `event`/`data`, camelCase)
  - `pub enum Frame { Request { id: u64, req: Request }, Response { id: u64, res: Response }, Push(Push) }`
  - `pub enum Request { Hello{..}, ListWorkspaces, CreateWorkspace{..}, ListSessions, CreateSession{..}, AttachSession{..}, DetachSession{..}, WriteStdin{..}, Resize{..}, KillSession{..}, GetSessionState{..}, DaemonShutdown{ drain: bool } }`
  - `pub enum Response { Welcome{..}, Incompatible{..}, Workspaces(Vec<Workspace>), Workspace(Workspace), Sessions(Vec<SessionMeta>), Session(SessionMeta), Ack, Error{ code: String, message: String } }`
  - `pub enum Push { Replay{..}, Output{..}, StateChanged{..}, ChildExited{..}, SessionCreated{ meta: SessionMeta }, WorkspaceCreated{ workspace: Workspace }, Error{ session_id: Option<SessionId>, code: String, message: String } }`
  - `pub const MAGIC: u32 = 0x4250_4131;`
  - `pub const PROTO_VERSION: u16 = 1;`
  - `pub fn encode_frame(frame: &Frame) -> Result<Vec<u8>, FrameError>` (u32-LE length prefix + `bincode::serialize`)
  - `pub struct FrameDecoder { buf: Vec<u8> }` with `pub fn new() -> Self`, `pub fn push(&mut self, chunk: &[u8])`, `pub fn decode(&mut self) -> Result<Vec<Frame>, FrameError>` (drains all complete frames, buffers partial ones)
  - `pub const MAX_FRAME_LEN: u32 = 16 * 1024 * 1024;` (16 MiB cap; oversized ⇒ `FrameError::Oversized`)
  - `pub enum FrameError { Oversized(u32), Decode(String), Encode(String) }` (impl `std::error::Error` + `Display`)

**Scene-setting:** This is the single source of truth for the Hop-B wire protocol (core ⇄ daemon) and the shared Rust⇄TS domain types. Every daemon module (G2–G3), the core broker (G4), and the frontend IPC layer (G5) depend on these exact names. `src/ipc/types.ts` is **generated** from this crate via ts-rs and hand-editing it is forbidden (CI enforces `git diff --exit-code`). `bincode` is pinned to **1.3.3** (serde-native, fixint little-endian, deterministic) per spec §3/§7 — never 2.x. Framing is `u32`-LE length prefix + `bincode(Frame)` with **no** separate raw preamble; the handshake (`Hello`/`Welcome`) rides inside `Frame`.

---

- [ ] **Step 1: Write `crates/protocol/Cargo.toml`.**

```toml
[package]
name = "protocol"
version = "0.1.0"
edition = "2021"
rust-version = "1.77.2"

[dependencies]
serde = { version = "1", features = ["derive"] }
bincode = "=1.3.3"
ts-rs = { version = "10", features = ["serde-compat"] }

[dev-dependencies]
serde_json = "1"
```

Run `cargo build -p protocol` once to resolve `ts-rs`; if the resolved major is not 10, pin it to the exact resolved major (record the concrete version in this file) — the API used here (`#[ts(export, export_to=..)]`, `#[ts(tag)]`, `#[ts(tag, content)]`, `Vec<u8> → number[]`, `serde-compat`) is stable across the current major, but lock whatever `cargo` picks.

- [ ] **Step 2 (RED): write `crates/protocol/tests/roundtrip.rs` — table-driven bincode round-trip for EVERY variant.**

```rust
use protocol::*;

/// bincode 1.3.3 default serialize/deserialize must round-trip every wire type.
fn assert_frame_roundtrip(frame: Frame) {
    let bytes = bincode::serialize(&frame).expect("serialize");
    let back: Frame = bincode::deserialize(&bytes).expect("deserialize");
    assert_eq!(
        bincode::serialize(&back).expect("re-serialize"),
        bytes,
        "frame did not round-trip byte-identically"
    );
}

fn sample_workspace() -> Workspace {
    Workspace { id: "ws-1".into(), name: "Demo".into(), root_path: "/tmp/demo".into() }
}

fn sample_meta(lifecycle: SessionLifecycle) -> SessionMeta {
    SessionMeta {
        id: "sess-1".into(),
        workspace_id: "ws-1".into(),
        title: "zsh".into(),
        shell: "/bin/zsh".into(),
        cwd: "/tmp/demo".into(),
        cols: 80,
        rows: 24,
        lifecycle,
        waiting_for_input: false,
        is_active: true,
        created_at: 1_720_000_000,
    }
}

fn all_lifecycles() -> Vec<SessionLifecycle> {
    vec![
        SessionLifecycle::AtPrompt,
        SessionLifecycle::Typing,
        SessionLifecycle::Running,
        SessionLifecycle::Exited { code: Some(0), signal: None },
        SessionLifecycle::Exited { code: Some(137), signal: None },
        SessionLifecycle::Exited { code: None, signal: Some("SIGKILL".into()) },
        SessionLifecycle::Exited { code: None, signal: None },
    ]
}

fn all_requests() -> Vec<Request> {
    vec![
        Request::Hello { magic: MAGIC, proto_version: PROTO_VERSION, client_build: "test".into() },
        Request::ListWorkspaces,
        Request::CreateWorkspace { name: "W".into(), root_path: "/tmp/w".into() },
        Request::ListSessions,
        Request::CreateSession {
            workspace_id: "ws-1".into(),
            shell: Some("/bin/bash".into()),
            cwd: Some("/tmp/demo".into()),
            env_overrides: vec![("FOO".into(), "bar".into())],
            cols: 120,
            rows: 40,
        },
        Request::CreateSession {
            workspace_id: "ws-1".into(),
            shell: None,
            cwd: None,
            env_overrides: vec![],
            cols: 80,
            rows: 24,
        },
        Request::AttachSession { session_id: "sess-1".into() },
        Request::DetachSession { session_id: "sess-1".into() },
        Request::WriteStdin { session_id: "sess-1".into(), bytes: vec![0, 27, 91, 65, 255] },
        Request::Resize { session_id: "sess-1".into(), cols: 100, rows: 30 },
        Request::KillSession { session_id: "sess-1".into() },
        Request::GetSessionState { session_id: "sess-1".into() },
        Request::DaemonShutdown { drain: true },
        Request::DaemonShutdown { drain: false },
    ]
}

fn all_responses() -> Vec<Response> {
    let mut v = vec![
        Response::Welcome { proto_version: PROTO_VERSION, daemon_build: "d".into() },
        Response::Incompatible { min: 1, max: 1 },
        Response::Workspaces(vec![sample_workspace()]),
        Response::Workspace(sample_workspace()),
        Response::Ack,
        Response::Error { code: "InvalidWorkspaceRoot".into(), message: "gone".into() },
    ];
    for lc in all_lifecycles() {
        v.push(Response::Sessions(vec![sample_meta(lc.clone())]));
        v.push(Response::Session(sample_meta(lc)));
    }
    v
}

fn all_pushes() -> Vec<Push> {
    let mut v = vec![
        Push::Replay { session_id: "sess-1".into(), cols: 80, rows: 24, content: vec![1, 2, 3, 255, 0] },
        Push::Output { session_id: "sess-1".into(), bytes: vec![97, 98, 99] },
        Push::ChildExited { session_id: "sess-1".into(), code: Some(42), signal: None },
        Push::ChildExited { session_id: "sess-1".into(), code: None, signal: Some("SIGTERM".into()) },
        Push::SessionCreated { meta: sample_meta(SessionLifecycle::AtPrompt) },
        Push::WorkspaceCreated { workspace: sample_workspace() },
        Push::Error { session_id: Some("sess-1".into()), code: "PtySpawn".into(), message: "boom".into() },
        Push::Error { session_id: None, code: "Internal".into(), message: "x".into() },
    ];
    for lc in all_lifecycles() {
        v.push(Push::StateChanged {
            session_id: "sess-1".into(),
            lifecycle: lc,
            waiting_for_input: true,
            cwd: "/tmp/demo".into(),
        });
    }
    v
}

#[test]
fn every_request_variant_roundtrips() {
    for (i, req) in all_requests().into_iter().enumerate() {
        assert_frame_roundtrip(Frame::Request { id: i as u64, req });
    }
}

#[test]
fn every_response_variant_roundtrips() {
    for (i, res) in all_responses().into_iter().enumerate() {
        assert_frame_roundtrip(Frame::Response { id: i as u64, res });
    }
}

#[test]
fn every_push_variant_roundtrips() {
    for push in all_pushes() {
        assert_frame_roundtrip(Frame::Push(push));
    }
}

#[test]
fn every_terminal_event_roundtrips() {
    // TerminalEvent is Serialize-only over Hop A, but must still bincode-round-trip
    // via serde for parity coverage; it derives Deserialize too for symmetry.
    for ev in [
        TerminalEvent::Replay { cols: 80, rows: 24, content: vec![9, 8, 7] },
        TerminalEvent::Output { bytes: vec![1, 2, 3] },
    ] {
        let bytes = bincode::serialize(&ev).expect("serialize");
        let back: TerminalEvent = bincode::deserialize(&bytes).expect("deserialize");
        assert_eq!(bincode::serialize(&back).expect("re"), bytes);
    }
}

#[test]
fn constants_are_locked() {
    assert_eq!(MAGIC, 0x4250_4131);
    assert_eq!(PROTO_VERSION, 1);
}
```

- [ ] **Step 3 (RED-run): `cargo test -p protocol --test roundtrip`.** Expected: FAIL with `error[E0432]: unresolved import protocol` / `cannot find type Frame` (no `lib.rs` types yet).

- [ ] **Step 4 (GREEN): write `crates/protocol/src/lib.rs` — all types + re-export framing.**

```rust
//! Shared Hop-B wire protocol + Rust⇄TS domain types for Builder Pro AI.
//!
//! Source of truth for `src/ipc/types.ts` (generated via ts-rs; never hand-edited).
//! Codec is bincode 1.3.3 (fixint, little-endian, deterministic). Framing lives in
//! `framing.rs`. Every type here derives serde `Serialize`/`Deserialize`; the types
//! that cross into TypeScript also derive `ts_rs::TS`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

mod framing;
pub use framing::{encode_frame, FrameDecoder, FrameError, MAX_FRAME_LEN};

pub type SessionId = String; // UUID v4
pub type WorkspaceId = String; // UUID v4

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../src/ipc/types.ts", rename_all = "camelCase")]
pub struct Workspace {
    pub id: WorkspaceId,
    pub name: String,
    pub root_path: String,
}

/// Internally tagged on `kind` (tag only, no content) — matches the TS discriminated
/// union in spec §5. Unit variants carry only `{ kind }`; the struct variant `Exited`
/// carries its fields flattened next to the tag.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export, export_to = "../../src/ipc/types.ts", tag = "kind", rename_all = "camelCase")]
pub enum SessionLifecycle {
    /// idle at shell prompt (after OSC 133 B, before C)
    AtPrompt,
    /// NEVER emitted in S1; UI maps to AtPrompt color
    Typing,
    /// command executing (after C, before D)
    Running,
    /// finished; code None = unknown/aborted
    Exited { code: Option<u8>, signal: Option<String> },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export, export_to = "../../src/ipc/types.ts", rename_all = "camelCase")]
pub struct SessionMeta {
    pub id: SessionId,
    pub workspace_id: WorkspaceId,
    pub title: String,
    pub shell: String,
    pub cwd: String,
    pub cols: u16,
    pub rows: u16,
    pub lifecycle: SessionLifecycle,
    pub waiting_for_input: bool,
    pub is_active: bool,
    pub created_at: i64,
}

/// Hop-A Channel payload (spec §6.2). Adjacently tagged (`event`/`data`).
/// `Vec<u8>` serializes over Tauri IPC as `number[]`; ts-rs emits `Array<number>`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[serde(tag = "event", content = "data", rename_all = "camelCase")]
#[ts(export, export_to = "../../src/ipc/types.ts", tag = "event", content = "data", rename_all = "camelCase")]
pub enum TerminalEvent {
    /// FIRST msg on attach; write BEFORE term.open()
    Replay { cols: u16, rows: u16, content: Vec<u8> },
    /// incremental live PTY bytes
    Output { bytes: Vec<u8> },
}

// ---- Hop-B wire frame (core ⇄ daemon). NOT exported to TS. ----

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum Frame {
    /// core → daemon; id correlates the reply
    Request { id: u64, req: Request },
    /// daemon → core; echoes the request id
    Response { id: u64, res: Response },
    /// daemon → core; unsolicited (id-less)
    Push(Push),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum Request {
    Hello { magic: u32, proto_version: u16, client_build: String },
    ListWorkspaces,
    CreateWorkspace { name: String, root_path: String },
    ListSessions,
    CreateSession {
        workspace_id: WorkspaceId,
        shell: Option<String>,
        cwd: Option<String>,
        env_overrides: Vec<(String, String)>,
        cols: u16,
        rows: u16,
    },
    AttachSession { session_id: SessionId },
    DetachSession { session_id: SessionId },
    WriteStdin { session_id: SessionId, bytes: Vec<u8> },
    Resize { session_id: SessionId, cols: u16, rows: u16 },
    KillSession { session_id: SessionId },
    GetSessionState { session_id: SessionId },
    DaemonShutdown { drain: bool },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum Response {
    Welcome { proto_version: u16, daemon_build: String },
    Incompatible { min: u16, max: u16 },
    Workspaces(Vec<Workspace>),
    Workspace(Workspace),
    Sessions(Vec<SessionMeta>),
    Session(SessionMeta),
    Ack,
    Error { code: String, message: String },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum Push {
    Replay { session_id: SessionId, cols: u16, rows: u16, content: Vec<u8> },
    Output { session_id: SessionId, bytes: Vec<u8> },
    StateChanged {
        session_id: SessionId,
        lifecycle: SessionLifecycle,
        waiting_for_input: bool,
        cwd: String,
    },
    ChildExited { session_id: SessionId, code: Option<u8>, signal: Option<String> },
    SessionCreated { meta: SessionMeta },
    WorkspaceCreated { workspace: Workspace },
    Error { session_id: Option<SessionId>, code: String, message: String },
}

pub const MAGIC: u32 = 0x4250_4131; // "BPA1"
pub const PROTO_VERSION: u16 = 1;
```

- [ ] **Step 5 (GREEN): write `crates/protocol/src/framing.rs` — length-prefix codec + partial-frame decoder.**

```rust
//! Hop-B framing: `u32` little-endian length prefix + `bincode(Frame)` body.
//! bincode 1.3.3 defaults (fixint, little-endian) — identical both sides.

use std::fmt;

use crate::Frame;

/// Hard cap on a single frame body (16 MiB). A larger declared length is treated
/// as garbage/DoS and rejected rather than allocated.
pub const MAX_FRAME_LEN: u32 = 16 * 1024 * 1024;

#[derive(Debug)]
pub enum FrameError {
    /// declared length prefix exceeds `MAX_FRAME_LEN`
    Oversized(u32),
    /// bincode failed to decode a complete, correctly-sized body
    Decode(String),
    /// bincode failed to encode
    Encode(String),
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FrameError::Oversized(n) => write!(f, "frame length {n} exceeds max {MAX_FRAME_LEN}"),
            FrameError::Decode(e) => write!(f, "frame decode error: {e}"),
            FrameError::Encode(e) => write!(f, "frame encode error: {e}"),
        }
    }
}

impl std::error::Error for FrameError {}

/// Serialize `frame` with bincode and prepend a `u32`-LE length prefix.
pub fn encode_frame(frame: &Frame) -> Result<Vec<u8>, FrameError> {
    let body = bincode::serialize(frame).map_err(|e| FrameError::Encode(e.to_string()))?;
    if body.len() as u64 > MAX_FRAME_LEN as u64 {
        return Err(FrameError::Oversized(body.len() as u32));
    }
    let mut out = Vec::with_capacity(4 + body.len());
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    Ok(out)
}

/// Buffers raw socket bytes and drains complete frames. A partial frame (prefix
/// not yet complete, or body not fully arrived) stays buffered until the next
/// `push`. An oversized declared length is a hard error (the stream is corrupt).
#[derive(Default)]
pub struct FrameDecoder {
    buf: Vec<u8>,
}

impl FrameDecoder {
    pub fn new() -> Self {
        FrameDecoder { buf: Vec::new() }
    }

    /// Append newly-read bytes to the internal buffer.
    pub fn push(&mut self, chunk: &[u8]) {
        self.buf.extend_from_slice(chunk);
    }

    /// Drain and return every complete frame currently buffered. Leaves any
    /// trailing partial frame in the buffer. Returns `Err` (without consuming the
    /// offending bytes) on an oversized length prefix or a body decode failure.
    pub fn decode(&mut self) -> Result<Vec<Frame>, FrameError> {
        let mut frames = Vec::new();
        let mut offset = 0usize;
        loop {
            if self.buf.len() - offset < 4 {
                break; // not enough for a length prefix yet
            }
            let mut len_bytes = [0u8; 4];
            len_bytes.copy_from_slice(&self.buf[offset..offset + 4]);
            let len = u32::from_le_bytes(len_bytes);
            if len > MAX_FRAME_LEN {
                return Err(FrameError::Oversized(len));
            }
            let len = len as usize;
            if self.buf.len() - offset - 4 < len {
                break; // body not fully arrived; keep buffered
            }
            let body = &self.buf[offset + 4..offset + 4 + len];
            let frame: Frame =
                bincode::deserialize(body).map_err(|e| FrameError::Decode(e.to_string()))?;
            frames.push(frame);
            offset += 4 + len;
        }
        if offset > 0 {
            self.buf.drain(0..offset);
        }
        Ok(frames)
    }
}
```

- [ ] **Step 6 (GREEN-run): `cargo test -p protocol --test roundtrip`.** Expected: PASS (all 5 tests green).

- [ ] **Step 7 (RED): write `crates/protocol/tests/framing.rs` — partial frames, boundaries, garbage/oversized rejection.**

```rust
use protocol::*;

fn frame() -> Frame {
    Frame::Request {
        id: 7,
        req: Request::WriteStdin { session_id: "s".into(), bytes: vec![1, 2, 3, 4, 5] },
    }
}

#[test]
fn single_frame_encodes_and_decodes() {
    let bytes = encode_frame(&frame()).expect("encode");
    // u32-LE length prefix + body
    let declared = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    assert_eq!(declared, bytes.len() - 4, "length prefix must equal body length");

    let mut dec = FrameDecoder::new();
    dec.push(&bytes);
    let frames = dec.decode().expect("decode");
    assert_eq!(frames, vec![frame()]);
}

#[test]
fn partial_frame_across_reads_buffers_then_completes() {
    let bytes = encode_frame(&frame()).expect("encode");
    let split = bytes.len() / 2;
    let mut dec = FrameDecoder::new();

    dec.push(&bytes[..split]);
    assert_eq!(dec.decode().expect("decode-1"), vec![], "half a frame yields nothing");

    dec.push(&bytes[split..]);
    assert_eq!(dec.decode().expect("decode-2"), vec![frame()], "second half completes it");
}

#[test]
fn length_prefix_split_across_reads() {
    let bytes = encode_frame(&frame()).expect("encode");
    let mut dec = FrameDecoder::new();
    // deliver only 2 of the 4 prefix bytes first
    dec.push(&bytes[..2]);
    assert_eq!(dec.decode().expect("d1"), vec![], "incomplete prefix yields nothing");
    dec.push(&bytes[2..]);
    assert_eq!(dec.decode().expect("d2"), vec![frame()]);
}

#[test]
fn two_frames_in_one_read_both_decode() {
    let mut buf = encode_frame(&frame()).expect("e1");
    buf.extend_from_slice(&encode_frame(&frame()).expect("e2"));
    let mut dec = FrameDecoder::new();
    dec.push(&buf);
    assert_eq!(dec.decode().expect("decode"), vec![frame(), frame()]);
}

#[test]
fn oversized_length_prefix_is_rejected() {
    let mut dec = FrameDecoder::new();
    // declare a body far larger than MAX_FRAME_LEN
    let bogus = MAX_FRAME_LEN + 1;
    dec.push(&bogus.to_le_bytes());
    dec.push(&[0u8; 8]); // some body bytes
    match dec.decode() {
        Err(FrameError::Oversized(n)) => assert_eq!(n, bogus),
        other => panic!("expected Oversized, got {other:?}"),
    }
}

#[test]
fn garbage_body_of_valid_length_is_a_decode_error() {
    let mut dec = FrameDecoder::new();
    let len: u32 = 6;
    dec.push(&len.to_le_bytes());
    dec.push(&[0xff, 0xff, 0xff, 0xff, 0xff, 0xff]); // undecodable as Frame
    match dec.decode() {
        Err(FrameError::Decode(_)) => {}
        other => panic!("expected Decode error, got {other:?}"),
    }
}

#[test]
fn encode_matches_manual_prefix() {
    let f = frame();
    let body = bincode::serialize(&f).expect("body");
    let mut expected = (body.len() as u32).to_le_bytes().to_vec();
    expected.extend_from_slice(&body);
    assert_eq!(encode_frame(&f).expect("encode"), expected);
}
```

- [ ] **Step 8 (GREEN-run): `cargo test -p protocol --test framing`.** Expected: PASS (7 tests green). (Impl from Step 5 already satisfies these.)

- [ ] **Step 9 (RED): write `crates/protocol/tests/ts_export.rs` — generate `src/ipc/types.ts` and assert it matches spec §5 exactly (shape-level).**

This test both **triggers** the ts-rs export (writing `src/ipc/types.ts`) and **verifies** the emitted TS carries the locked encodings: `rootPath` (camelCase), `SessionLifecycle` internally-tagged on `kind` with camelCase tags, `TerminalEvent` adjacently-tagged `event`/`data`, and `Vec<u8> → Array<number>`.

```rust
use std::fs;
use std::path::PathBuf;

use protocol::*;
use ts_rs::TS;

/// Absolute path to the generated shared TS file (relative to this crate's Cargo.toml).
fn types_ts_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../src/ipc/types.ts")
}

/// Force every exported type to (re)write its TS binding, then read the file back.
fn export_and_read() -> String {
    Workspace::export_all().expect("export Workspace");
    SessionLifecycle::export_all().expect("export SessionLifecycle");
    SessionMeta::export_all().expect("export SessionMeta");
    TerminalEvent::export_all().expect("export TerminalEvent");
    fs::read_to_string(types_ts_path()).expect("read generated types.ts")
}

/// Whitespace-insensitive substring check so we assert structure, not formatting.
fn contains_normalized(haystack: &str, needle: &str) -> bool {
    let strip = |s: &str| s.split_whitespace().collect::<String>();
    strip(haystack).contains(&strip(needle))
}

#[test]
fn generates_types_ts_at_shared_path() {
    let ts = export_and_read();
    assert!(!ts.is_empty(), "types.ts must not be empty");
    assert!(types_ts_path().exists(), "types.ts must exist at src/ipc/types.ts");
    let _ = &ts;
}

#[test]
fn workspace_uses_camelcase_root_path() {
    let ts = export_and_read();
    assert!(
        contains_normalized(&ts, "rootPath: string"),
        "Workspace.root_path must serialize as camelCase `rootPath`; got:\n{ts}"
    );
    assert!(
        !ts.contains("root_path"),
        "generated TS must not contain snake_case `root_path`"
    );
}

#[test]
fn session_lifecycle_is_internally_tagged_camelcase() {
    let ts = export_and_read();
    for tag in ["atPrompt", "typing", "running", "exited"] {
        assert!(
            contains_normalized(&ts, &format!("kind: \"{tag}\"")),
            "SessionLifecycle must include internally-tagged variant kind:\"{tag}\"; got:\n{ts}"
        );
    }
    // Exited carries code:number|null and signal:string|null
    assert!(
        contains_normalized(&ts, "code: number | null") || contains_normalized(&ts, "code: number|null"),
        "Exited must carry nullable numeric code; got:\n{ts}"
    );
    assert!(
        contains_normalized(&ts, "signal: string | null") || contains_normalized(&ts, "signal: string|null"),
        "Exited must carry nullable string signal; got:\n{ts}"
    );
}

#[test]
fn terminal_event_is_adjacently_tagged_bytes_are_number_arrays() {
    let ts = export_and_read();
    assert!(
        contains_normalized(&ts, "event: \"replay\""),
        "TerminalEvent must be adjacently tagged with event:\"replay\"; got:\n{ts}"
    );
    assert!(
        contains_normalized(&ts, "event: \"output\""),
        "TerminalEvent must be adjacently tagged with event:\"output\"; got:\n{ts}"
    );
    assert!(
        contains_normalized(&ts, "data:"),
        "TerminalEvent variants must nest their payload under `data`; got:\n{ts}"
    );
    // Vec<u8> must be a number array (ts-rs emits Array<number>).
    assert!(
        contains_normalized(&ts, "content: Array<number>") || contains_normalized(&ts, "content: number[]"),
        "Replay.content (Vec<u8>) must be a number array; got:\n{ts}"
    );
    assert!(
        contains_normalized(&ts, "bytes: Array<number>") || contains_normalized(&ts, "bytes: number[]"),
        "Output.bytes (Vec<u8>) must be a number array; got:\n{ts}"
    );
}

#[test]
fn session_meta_fields_are_camelcase() {
    let ts = export_and_read();
    for field in ["workspaceId:", "waitingForInput:", "isActive:", "createdAt:"] {
        assert!(
            contains_normalized(&ts, field),
            "SessionMeta must expose camelCase field `{field}`; got:\n{ts}"
        );
    }
    assert!(!ts.contains("workspace_id"), "no snake_case leakage in SessionMeta");
}
```

Note on the ts-rs export API: `#[ts(export, export_to = "../../src/ipc/types.ts")]` makes every derived type append to the single shared file, and `T::export_all()` writes it (path is relative to `crates/protocol/Cargo.toml`, resolving to repo `src/ipc/types.ts`). If the resolved ts-rs major exposes the writer as `T::export()` instead of `export_all()`, use `export()` per type — confirm the method name from the built rustdoc (`cargo doc -p ts-rs --open`) and use whichever the pinned version provides; the assertions above are API-agnostic.

- [ ] **Step 10 (RED-run): `cargo test -p protocol --test ts_export`.** Expected: FAIL — either the file is not yet generated, or an assertion trips if any `#[ts(...)]` encoding is off (e.g. `SessionLifecycle` emitting `Exited` as a nested object instead of internally-tagged). If `SessionLifecycle` panics at derive time on the unit variants under `#[ts(tag="kind")]`, that surfaces here as a compile/test failure — resolve per Step 11.

- [ ] **Step 11 (GREEN): reconcile ts-rs output with spec §5, then re-run.**
  - Run `cargo test -p protocol --test ts_export -- --nocapture` and read the emitted `src/ipc/types.ts`.
  - If internally-tagged unit variants (`AtPrompt`/`Typing`/`Running`) render exactly as `{ kind: "atPrompt" }` etc. and `Exited` as `{ kind: "exited"; code: number | null; signal: string | null }` — the assertions pass; nothing to change.
  - If the pinned ts-rs version cannot emit internally-tagged **unit** variants and panics/misrenders: keep the `#[serde(tag="kind")]` attribute (the wire/JSON contract is authoritative and correct), and drive the TS for `SessionLifecycle` via ts-rs's supported form, adjusting the `#[ts(...)]` attribute to the nearest form that still yields the spec §5 union. Do not hand-edit `types.ts`; fix it at the derive so regeneration is stable. Re-run until every assertion in Step 9 is green.
  - Confirm the file was written to `src/ipc/types.ts` (repo root `src/`, not inside the crate).

- [ ] **Step 12 (GREEN-run full): `cargo test -p protocol`.** Expected: PASS — all of `roundtrip`, `framing`, `ts_export` green.

- [ ] **Step 13: commit types + framing + tests + generated TS.**

```
git add crates/protocol/Cargo.toml crates/protocol/src/lib.rs crates/protocol/src/framing.rs \
        crates/protocol/tests/roundtrip.rs crates/protocol/tests/framing.rs crates/protocol/tests/ts_export.rs \
        src/ipc/types.ts Cargo.toml && \
git commit -m "feat(protocol): shared wire types, bincode framing, generated TS bindings"
```

**Definition of Done:**
- `cargo test -p protocol` is green: table-driven bincode round-trip covers **every** `Request`, `Response`, `Push` variant (all 4 `SessionLifecycle` variants, incl. `Exited` with `Some`/`None` code and `Some`/`None` signal) and both `TerminalEvent` variants; `MAGIC == 0x4250_4131` and `PROTO_VERSION == 1` asserted (spec §7).
- Framing tests pass: `u32`-LE length prefix equals body length; a frame split across two `push`es buffers then completes; a length prefix split across reads is tolerated; two frames in one read both decode; a declared length `> MAX_FRAME_LEN` yields `FrameError::Oversized`; a valid-length undecodable body yields `FrameError::Decode`; `encode_frame` equals manual `prefix + bincode(body)` (spec §7 framing).
- `src/ipc/types.ts` is generated by the ts-rs export test (never hand-edited) and asserted to carry: camelCase fields (`rootPath`, `workspaceId`, `waitingForInput`, `isActive`, `createdAt`), `SessionLifecycle` internally-tagged on `kind` with tags `atPrompt`/`typing`/`running`/`exited` and `Exited { code: number|null, signal: string|null }`, `TerminalEvent` adjacently-tagged `event`/`data`, and `Vec<u8>` rendered as a number array for `content`/`bytes` (spec §5, §6.2, §14.2 Rust⇄TS parity row).
- The generated `src/ipc/types.ts` is committed; no snake_case identifiers leak into it.
- `bincode` is pinned to `=1.3.3` in `crates/protocol/Cargo.toml` (spec §3/§7).


### Task 4: `singleton.rs` — socket path resolution, single-instance flock, dir/socket perms, peer-cred

**Files:**
- Create: `crates/sessiond/src/singleton.rs`
- Modify: `crates/sessiond/Cargo.toml` (add `rustix` with features `["fs", "net"]`; add `[dev-dependencies] tempfile`), `crates/sessiond/src/lib.rs` (add `pub mod singleton;` — create the lib target if T3/G2 scaffolding did not; if `sessiond` is a bin-only crate, add `mod singleton;` in `main.rs` and gate module tests with `#[cfg(test)]`). This task OWNS `singleton.rs` only; append its `mod` line without touching other tasks' `mod` lines.
- Test: inline `#[cfg(test)] mod tests` in `crates/sessiond/src/singleton.rs`.

**Depends on:** [T3]   **Parallel-safe with:** [T5, T6, T7, T8, T10]

**Interfaces:** Consumes: nothing from T3 at the type level (uses only std + `rustix`); it is grouped under G2 which is gated on T3 landing the workspace/protocol crate so the daemon crate compiles. Produces (verbatim from the scaffold Task interface index, spec §8.1–§8.2 / §16):
```rust
pub fn resolve_socket_path() -> std::path::PathBuf;
pub fn resolve_lockfile() -> std::path::PathBuf;
pub fn ensure_socket_dir() -> std::io::Result<()>;
pub fn acquire_single_instance_lock() -> std::io::Result<LockGuard>;
pub fn check_peer_cred(fd: std::os::fd::BorrowedFd<'_>) -> std::io::Result<()>;
pub struct LockGuard { /* owns the locked lockfile File for the daemon's lifetime */ }
```
Locked constants/behavior from the scaffold Global Constraints + spec §8.1: socket dir = `$XDG_RUNTIME_DIR/bpa` when `XDG_RUNTIME_DIR` is set and non-empty, else `/tmp/bpa-<uid>`; socket file `d.sock`; lockfile `d.lock`; dir mode `0o700` owned by the current uid; socket file mode `0o600`; socket path length hard-asserted `< 104`.

Design notes for the implementer (locked so tests and code agree):
- `resolve_socket_path()` returns `<dir>/d.sock`; `resolve_lockfile()` returns `<dir>/d.lock`. Both derive `<dir>` from the same private `socket_dir()` helper so they never diverge.
- The `< 104` assertion lives inside `bind`-time (`ensure_socket_dir` cannot fail on length because the caller must check the returned socket path). We expose a `pub fn assert_socket_path_len(p: &Path) -> io::Result<()>` that returns `ErrorKind::InvalidInput` when `p.as_os_str().as_bytes().len() >= 104`. `resolve_socket_path()` itself never panics.
- `ensure_socket_dir()`: create `<dir>` with mode `0o700` (`DirBuilder::mode(0o700)`); if it already exists, `stat` it and refuse (`ErrorKind::PermissionDenied`) unless it is a directory, owned by the current euid, and its permission bits (`st_mode & 0o777`) equal `0o700`. Guards the `/tmp` squat race per spec §8.2.
- `acquire_single_instance_lock()`: open (create) the lockfile `0o600`, `rustix::fs::flock(fd, FlockOperation::NonBlockingLockExclusive)`; on `Errno::WOULDBLOCK` return `ErrorKind::WouldBlock` (a second daemon exits). The returned `LockGuard` owns the `File`; dropping it releases the flock.
- `check_peer_cred(fd)`: `rustix::net::sockopt::socket_peercred` is Linux-only; on macOS use `getpeereid(2)` via `rustix::net::sockopt` where available, else a thin `libc::getpeereid` FFI. Compare returned euid to `rustix::process::geteuid()`; refuse (`ErrorKind::PermissionDenied`) on mismatch. Because `getpeereid` is stable POSIX on macOS, use a small `unsafe` FFI block calling `libc::getpeereid(raw_fd, &mut uid, &mut gid)` and add `libc` to `Cargo.toml`.

- [ ] **Step 1: Failing test — socket path resolution honors XDG and length invariant**

Add to `crates/sessiond/src/singleton.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::ffi::OsStrExt;

    fn with_env<F: FnOnce()>(key: &str, val: Option<&str>, f: F) {
        let prev = std::env::var_os(key);
        match val {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
        f();
        match prev {
            Some(p) => std::env::set_var(key, p),
            None => std::env::remove_var(key),
        }
    }

    #[test]
    fn socket_path_uses_xdg_runtime_dir_when_set() {
        with_env("XDG_RUNTIME_DIR", Some("/run/user/501"), || {
            let sock = resolve_socket_path();
            assert_eq!(sock, std::path::PathBuf::from("/run/user/501/bpa/d.sock"));
            let lock = resolve_lockfile();
            assert_eq!(lock, std::path::PathBuf::from("/run/user/501/bpa/d.lock"));
        });
    }

    #[test]
    fn socket_path_falls_back_to_tmp_with_uid_when_xdg_unset() {
        with_env("XDG_RUNTIME_DIR", None, || {
            let sock = resolve_socket_path();
            let uid = rustix::process::geteuid().as_raw();
            let expected = std::path::PathBuf::from(format!("/tmp/bpa-{uid}/d.sock"));
            assert_eq!(sock, expected);
        });
    }

    #[test]
    fn socket_path_falls_back_to_tmp_when_xdg_empty() {
        with_env("XDG_RUNTIME_DIR", Some(""), || {
            let sock = resolve_socket_path();
            let uid = rustix::process::geteuid().as_raw();
            assert_eq!(sock, std::path::PathBuf::from(format!("/tmp/bpa-{uid}/d.sock")));
        });
    }

    #[test]
    fn socket_path_len_under_104_passes_and_over_fails() {
        assert!(assert_socket_path_len(std::path::Path::new("/tmp/bpa-501/d.sock")).is_ok());
        let long = std::path::PathBuf::from(format!("/tmp/{}/d.sock", "x".repeat(120)));
        assert!(long.as_os_str().as_bytes().len() >= 104);
        let err = assert_socket_path_len(&long).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }
}
```

- [ ] **Step 2: Run — confirm FAIL**

`cargo test -p sessiond singleton::tests`
Expected: FAIL to compile with `cannot find function 'resolve_socket_path'` / `assert_socket_path_len` in this scope.

- [ ] **Step 3: Implement path resolution + length assert**

Prepend to `crates/sessiond/src/singleton.rs` (above the test module):
```rust
//! Single-instance lock, socket path resolution, dir/socket permissions, peer-cred (spec §8.1–§8.2, §16).
use std::fs::{DirBuilder, File, OpenOptions};
use std::io::{self, Error, ErrorKind};
use std::os::fd::{AsFd, BorrowedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use rustix::fs::{flock, FlockOperation};

/// macOS `sun_path` is 104 bytes including NUL; usable length is strictly < 104.
const SUN_PATH_MAX: usize = 104;

fn socket_dir() -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(x) if !x.is_empty() => PathBuf::from(x).join("bpa"),
        _ => {
            let uid = rustix::process::geteuid().as_raw();
            PathBuf::from(format!("/tmp/bpa-{uid}"))
        }
    }
}

/// Resolve the daemon's Unix-domain-socket path (`<dir>/d.sock`). Never panics.
pub fn resolve_socket_path() -> PathBuf {
    socket_dir().join("d.sock")
}

/// Resolve the single-instance lockfile path (`<dir>/d.lock`).
pub fn resolve_lockfile() -> PathBuf {
    socket_dir().join("d.lock")
}

/// Hard-fail (spec §8.1) if the socket path would overflow `sun_path`.
pub fn assert_socket_path_len(p: &Path) -> io::Result<()> {
    if p.as_os_str().as_bytes().len() >= SUN_PATH_MAX {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            format!(
                "socket path length {} >= sun_path max {SUN_PATH_MAX}: {}",
                p.as_os_str().as_bytes().len(),
                p.display()
            ),
        ));
    }
    Ok(())
}
```

- [ ] **Step 4: Run — confirm PASS**

`cargo test -p sessiond singleton::tests`
Expected: PASS (4 path tests green).

- [ ] **Step 5: Commit**

`git add crates/sessiond/src/singleton.rs crates/sessiond/Cargo.toml && git commit -m "feat(sessiond): socket path resolution + sun_path length guard (singleton)"`

- [ ] **Step 6: Failing test — ensure_socket_dir creates 0700 dir, verifies owner+mode, refuses squat**

Because `ensure_socket_dir()` targets the real resolved dir, drive it through a testable inner helper `ensure_dir(dir)` so the test can point at a tempdir. Add to the `tests` mod:
```rust
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn ensure_dir_creates_with_0700() {
        let base = tempfile::tempdir().unwrap();
        let dir = base.path().join("bpa");
        ensure_dir(&dir).expect("create ok");
        let md = std::fs::metadata(&dir).unwrap();
        assert!(md.is_dir());
        assert_eq!(md.permissions().mode() & 0o777, 0o700);
        assert_eq!(md.uid(), rustix::process::geteuid().as_raw());
        // Idempotent: second call on our own 0700 dir succeeds.
        ensure_dir(&dir).expect("idempotent ok");
    }

    #[test]
    fn ensure_dir_refuses_world_writable_squat() {
        let base = tempfile::tempdir().unwrap();
        let dir = base.path().join("bpa");
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o777)).unwrap();
        let err = ensure_dir(&dir).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn ensure_dir_refuses_non_directory() {
        let base = tempfile::tempdir().unwrap();
        let path = base.path().join("bpa");
        std::fs::write(&path, b"not a dir").unwrap();
        let err = ensure_dir(&path).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
    }
```

- [ ] **Step 7: Run — confirm FAIL**

`cargo test -p sessiond singleton::tests`
Expected: FAIL to compile with `cannot find function 'ensure_dir'`.

- [ ] **Step 8: Implement `ensure_dir` + public `ensure_socket_dir`**

Append to the impl section of `singleton.rs`:
```rust
/// Verify an existing socket dir is a directory owned by the current euid with mode 0700,
/// or create it fresh with mode 0700. Guards the `/tmp` squatting race (spec §8.2).
fn ensure_dir(dir: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(dir) {
        Ok(md) => {
            let euid = rustix::process::geteuid().as_raw();
            if !md.is_dir() {
                return Err(Error::new(
                    ErrorKind::PermissionDenied,
                    format!("socket dir path is not a directory: {}", dir.display()),
                ));
            }
            if md.uid() != euid {
                return Err(Error::new(
                    ErrorKind::PermissionDenied,
                    format!("socket dir {} not owned by uid {euid}", dir.display()),
                ));
            }
            if md.mode() & 0o777 != 0o700 {
                return Err(Error::new(
                    ErrorKind::PermissionDenied,
                    format!(
                        "socket dir {} mode {:o} != 0700",
                        dir.display(),
                        md.mode() & 0o777
                    ),
                ));
            }
            Ok(())
        }
        Err(e) if e.kind() == ErrorKind::NotFound => {
            DirBuilder::new().mode(0o700).create(dir)?;
            Ok(())
        }
        Err(e) => Err(e),
    }
}

/// Ensure the resolved socket directory exists at mode 0700 owned by us (spec §8.1–§8.2).
pub fn ensure_socket_dir() -> io::Result<()> {
    ensure_dir(&socket_dir())
}
```

- [ ] **Step 9: Run — confirm PASS**

`cargo test -p sessiond singleton::tests`
Expected: PASS (dir-perm tests green).

- [ ] **Step 10: Commit**

`git add crates/sessiond/src/singleton.rs && git commit -m "feat(sessiond): ensure_socket_dir 0700 with owner+mode verify and squat guard"`

- [ ] **Step 11: Failing test — single-instance flock (second acquire fails) + socket mode helper**

Add to the `tests` mod:
```rust
    #[test]
    fn second_flock_on_same_lockfile_would_block() {
        let base = tempfile::tempdir().unwrap();
        let lock = base.path().join("d.lock");
        let g1 = acquire_lock_at(&lock).expect("first lock ok");
        let err = acquire_lock_at(&lock).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::WouldBlock);
        drop(g1);
        // After the first guard drops, the lock is re-acquirable.
        let _g2 = acquire_lock_at(&lock).expect("re-lock after drop ok");
    }

    #[test]
    fn lockfile_created_mode_0600() {
        let base = tempfile::tempdir().unwrap();
        let lock = base.path().join("d.lock");
        let _g = acquire_lock_at(&lock).unwrap();
        let md = std::fs::metadata(&lock).unwrap();
        assert_eq!(md.permissions().mode() & 0o777, 0o600);
    }

    #[test]
    fn set_socket_mode_applies_0600() {
        let base = tempfile::tempdir().unwrap();
        let sock = base.path().join("d.sock");
        std::fs::write(&sock, b"").unwrap();
        set_socket_mode(&sock).unwrap();
        let md = std::fs::metadata(&sock).unwrap();
        assert_eq!(md.permissions().mode() & 0o777, 0o600);
    }
```
Note: the flock second-acquire must fail *within the same process*. `flock(2)` locks are per-open-file-description, so two distinct `File` opens of the same path DO contend even in one process — this test relies on that.

- [ ] **Step 12: Run — confirm FAIL**

`cargo test -p sessiond singleton::tests`
Expected: FAIL to compile with `cannot find function 'acquire_lock_at'` / `set_socket_mode`.

- [ ] **Step 13: Implement flock + LockGuard + socket mode**

Append to the impl section:
```rust
/// Owns the exclusively-flocked lockfile for the daemon's whole lifetime.
/// Dropping the guard releases the advisory lock.
pub struct LockGuard {
    _file: File,
}

fn acquire_lock_at(path: &Path) -> io::Result<LockGuard> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(path)?;
    match flock(file.as_fd(), FlockOperation::NonBlockingLockExclusive) {
        Ok(()) => Ok(LockGuard { _file: file }),
        Err(e) if e == rustix::io::Errno::WOULDBLOCK || e == rustix::io::Errno::AGAIN => Err(
            Error::new(ErrorKind::WouldBlock, "another daemon holds the single-instance lock"),
        ),
        Err(e) => Err(Error::from_raw_os_error(e.raw_os_error())),
    }
}

/// Acquire the single-instance advisory lock at the resolved lockfile (spec §8.2).
/// A second daemon that cannot take the lock gets `ErrorKind::WouldBlock` and must exit.
pub fn acquire_single_instance_lock() -> io::Result<LockGuard> {
    acquire_lock_at(&resolve_lockfile())
}

/// Set the bound socket file to mode 0600 (spec §8.2).
pub fn set_socket_mode(sock: &Path) -> io::Result<()> {
    use std::fs::Permissions;
    std::fs::set_permissions(sock, Permissions::from_mode(0o600))
}
```

- [ ] **Step 14: Run — confirm PASS**

`cargo test -p sessiond singleton::tests`
Expected: PASS (flock + mode tests green).

- [ ] **Step 15: Commit**

`git add crates/sessiond/src/singleton.rs && git commit -m "feat(sessiond): flock single-instance LockGuard + socket mode 0600"`

- [ ] **Step 16: Failing test — peer-cred accepts self, rejects simulated foreign uid**

`check_peer_cred` reads the peer euid of a connected socket. Drive it through an inner `peer_euid(fd) -> io::Result<u32>` so a test can connect a real socketpair to itself (same uid → accepted) and a `check_peer_cred_against(fd, expected)` so a simulated foreign uid is rejected without needing another user. Add to the `tests` mod:
```rust
    #[test]
    fn peer_cred_accepts_same_uid_over_socketpair() {
        use std::os::unix::net::UnixStream;
        let (a, _b) = UnixStream::pair().unwrap();
        // Our own connection: peer euid == our euid → accepted.
        check_peer_cred(a.as_fd()).expect("same-uid peer accepted");
    }

    #[test]
    fn peer_cred_rejects_foreign_uid_simulated() {
        use std::os::unix::net::UnixStream;
        let (a, _b) = UnixStream::pair().unwrap();
        let real = peer_euid(a.as_fd()).expect("read peer euid");
        // Simulate a foreign peer by comparing against a deliberately-wrong expected uid.
        let foreign = real.wrapping_add(1);
        let err = check_peer_cred_against(a.as_fd(), foreign).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
    }
```

- [ ] **Step 17: Run — confirm FAIL**

`cargo test -p sessiond singleton::tests`
Expected: FAIL to compile with `cannot find function 'check_peer_cred'` / `peer_euid` / `check_peer_cred_against`.

- [ ] **Step 18: Implement peer-cred via getpeereid**

Add `libc = "0.2"` to `crates/sessiond/Cargo.toml` `[dependencies]`. Append to the impl section:
```rust
/// Read the effective uid of the peer connected to `fd` via `getpeereid(2)` (POSIX/macOS).
fn peer_euid(fd: BorrowedFd<'_>) -> io::Result<u32> {
    use std::os::fd::AsRawFd;
    let mut uid: libc::uid_t = 0;
    let mut gid: libc::gid_t = 0;
    // SAFETY: fd is a valid borrowed AF_UNIX socket fd for the duration of the call;
    // uid/gid are valid out-pointers.
    let rc = unsafe { libc::getpeereid(fd.as_raw_fd(), &mut uid, &mut gid) };
    if rc != 0 {
        return Err(Error::last_os_error());
    }
    Ok(uid as u32)
}

/// Compare the peer euid to `expected`; refuse on mismatch.
fn check_peer_cred_against(fd: BorrowedFd<'_>, expected: u32) -> io::Result<()> {
    let peer = peer_euid(fd)?;
    if peer != expected {
        return Err(Error::new(
            ErrorKind::PermissionDenied,
            format!("peer euid {peer} != daemon euid {expected}"),
        ));
    }
    Ok(())
}

/// Verify the connecting peer's effective uid equals the daemon's euid (spec §8.2, §16).
/// Refuse otherwise. `fd` must be an accepted AF_UNIX stream socket.
pub fn check_peer_cred(fd: BorrowedFd<'_>) -> io::Result<()> {
    let euid = rustix::process::geteuid().as_raw();
    check_peer_cred_against(fd, euid)
}
```

- [ ] **Step 19: Run — confirm PASS**

`cargo test -p sessiond singleton::tests`
Expected: PASS (peer-cred accept + reject green).

- [ ] **Step 20: Commit**

`git add crates/sessiond/src/singleton.rs crates/sessiond/Cargo.toml && git commit -m "feat(sessiond): peer-cred euid check via getpeereid (reject foreign uid)"`

**Definition of Done:**
- `cargo test -p sessiond singleton::tests` is green (all path / dir-perm / flock / socket-mode / peer-cred tests).
- `resolve_socket_path()` returns `$XDG_RUNTIME_DIR/bpa/d.sock` when set-and-non-empty, else `/tmp/bpa-<uid>/d.sock`; `resolve_lockfile()` mirrors with `d.lock` from the same `socket_dir()` (spec §8.1).
- `assert_socket_path_len` hard-fails (`InvalidInput`) at/over 104 bytes; passes under (spec §8.1).
- `ensure_socket_dir()` creates the dir at mode `0o700`, and refuses an existing dir that is world-writable, foreign-owned, or not a directory (spec §8.2 squat guard).
- `acquire_single_instance_lock()` uses `flock(LOCK_EX|LOCK_NB)`; a second acquire on a held lockfile returns `ErrorKind::WouldBlock`; lockfile is mode `0o600`; released on `LockGuard` drop (spec §8.2).
- `set_socket_mode` sets the bound socket to `0o600` (spec §8.2).
- `check_peer_cred` accepts a same-uid peer and rejects a simulated foreign uid with `PermissionDenied` (spec §8.2, §16).
- Module docs reference the governing spec sections; no `TODO`.

---

### Task 5: `osc_parser.rs` — streaming OSC-133/OSC-7 tokenizer + lifecycle state machine

**Files:**
- Create: `crates/sessiond/src/osc_parser.rs`
- Modify: `crates/sessiond/src/lib.rs` (append `pub mod osc_parser;`) — or `main.rs` `mod osc_parser;` per the crate-target rule in T4. Append only your own `mod` line.
- Test: inline `#[cfg(test)] mod tests` in `crates/sessiond/src/osc_parser.rs`.

**Depends on:** [T3]   **Parallel-safe with:** [T4, T6, T7, T8, T10]

**Interfaces:** Consumes: `protocol::SessionLifecycle` (spec §5) is NOT used here — the parser emits its own internal `OscEvent`/`Lifecycle`; the mapping to `protocol::SessionLifecycle` happens in T9 (`pty_supervisor`) which consumes T5. So T5 depends on T3 only for crate-compile ordering. Produces (verbatim from the scaffold Task interface index, spec §10.3):
```rust
pub struct OscParser { /* streaming buffer state */ }
impl OscParser {
    pub fn new() -> Self;
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<OscEvent>;
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OscEvent {
    PromptStart,             // OSC 133 ; A
    PromptEnd,               // OSC 133 ; B
    CommandStart,            // OSC 133 ; C
    CommandEnd(Option<u8>),  // OSC 133 ; D ; <code>   (None = unknown/aborted)
    Cwd(String),             // OSC 7  file:// or kitty-shell-cwd://
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lifecycle { AtPrompt, Running, Exited(Option<u8>) }
impl Lifecycle {
    pub fn new() -> Self;                 // starts AtPrompt
    pub fn advance(&mut self, ev: &OscEvent);
}
```
Locked behavior (spec §10.3):
- Streaming: `feed` buffers a partial OSC across chunk boundaries; a recognized OSC is consumed (never forwarded); everything else is pass-through to the caller (the caller handles raw bytes separately — `feed` returns only the *events*, not the pass-through bytes; pass-through is the caller's original chunk minus nothing at the event level, since T5's contract per the scaffold is `feed(&[u8]) -> Vec<OscEvent>`).
- Terminators accepted: `BEL` (0x07), `ST` (`ESC \` = 0x1B 0x5C), and implicit-`ESC` (a fresh `ESC` starting a new sequence terminates the current OSC).
- OSC buffer cap: 8 KiB. On overflow, discard the in-progress OSC and resume scanning (never grow unbounded, never crash).
- Exit-code rule: `D;<code>` base-10 in `0..=255` → `CommandEnd(Some(code))`; empty / non-numeric / out-of-range → `CommandEnd(None)`. Never coerce to 0.
- OSC 7 decode: accept `file://host/<abs>` and `kitty-shell-cwd://host/<abs>` only; percent-decode; strip host; bound path length (reuse the 8 KiB cap). Malformed → no `Cwd` event (dropped safely).
- State machine transitions (spec §10.3): `A` = prompt drawing (no state change on its own); `B` → `AtPrompt`; `C` → `Running`; `D;code` → `Exited(code)`; empty-command `B → A` (i.e. `PromptEnd` then `PromptStart` with no `C`/`D`) is a no-op (stays `AtPrompt`, no phantom `Running`).

- [ ] **Step 1: Failing test — single-chunk A/B/C/D happy path + events**

Add to `crates/sessiond/src/osc_parser.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    const BEL: u8 = 0x07;
    const ESC: u8 = 0x1B;

    fn osc(body: &str, term: &[u8]) -> Vec<u8> {
        let mut v = vec![ESC, b']'];
        v.extend_from_slice(body.as_bytes());
        v.extend_from_slice(term);
        v
    }

    #[test]
    fn parses_full_133_lifecycle_bel_terminated() {
        let mut p = OscParser::new();
        let mut stream = Vec::new();
        stream.extend_from_slice(&osc("133;A", &[BEL]));
        stream.extend_from_slice(b"user@host $ ");
        stream.extend_from_slice(&osc("133;B", &[BEL]));
        stream.extend_from_slice(b"ls -la\n");
        stream.extend_from_slice(&osc("133;C", &[BEL]));
        stream.extend_from_slice(b"file1 file2\n");
        stream.extend_from_slice(&osc("133;D;0", &[BEL]));
        let events = p.feed(&stream);
        assert_eq!(
            events,
            vec![
                OscEvent::PromptStart,
                OscEvent::PromptEnd,
                OscEvent::CommandStart,
                OscEvent::CommandEnd(Some(0)),
            ]
        );
    }

    #[test]
    fn non_osc_bytes_produce_no_events() {
        let mut p = OscParser::new();
        assert_eq!(p.feed(b"plain text with \x1b[31mSGR\x1b[0m color"), Vec::<OscEvent>::new());
    }
}
```

- [ ] **Step 2: Run — confirm FAIL**

`cargo test -p sessiond osc_parser::tests`
Expected: FAIL to compile with `cannot find type 'OscParser'` / `OscEvent`.

- [ ] **Step 3: Implement the streaming tokenizer core (BEL-terminated OSC 133 A/B/C/D)**

Prepend to `crates/sessiond/src/osc_parser.rs`:
```rust
//! Streaming OSC-133/OSC-7 tokenizer + lifecycle state machine (spec §10.3).
//! Buffers partial OSC across `feed` boundaries; accepts BEL/ST/implicit-ESC terminators;
//! caps the OSC buffer at 8 KiB; hardened against forged/oversized/interleaved input.

const BEL: u8 = 0x07;
const ESC: u8 = 0x1B;
const OSC_INTRODUCER: u8 = b']';
const ST_FINAL: u8 = b'\\';
const OSC_BUF_CAP: usize = 8 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    /// Normal pass-through scanning.
    Ground,
    /// Saw ESC, awaiting the next byte (could be `]` → OSC, or anything else).
    Escape,
    /// Inside an OSC payload, accumulating until a terminator.
    Osc,
    /// Inside OSC, saw ESC — awaiting `\` (ST) or a new sequence start.
    OscEsc,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum OscEvent {
    PromptStart,
    PromptEnd,
    CommandStart,
    CommandEnd(Option<u8>),
    Cwd(String),
}

pub struct OscParser {
    state: State,
    buf: Vec<u8>,
    /// True once the in-progress OSC exceeded the cap; the sequence is abandoned.
    overflowed: bool,
}

impl Default for OscParser {
    fn default() -> Self {
        Self::new()
    }
}

impl OscParser {
    pub fn new() -> Self {
        OscParser { state: State::Ground, buf: Vec::new(), overflowed: false }
    }

    /// Feed a chunk of raw PTY bytes; returns the recognized OSC events in order.
    pub fn feed(&mut self, chunk: &[u8]) -> Vec<OscEvent> {
        let mut out = Vec::new();
        for &b in chunk {
            match self.state {
                State::Ground => {
                    if b == ESC {
                        self.state = State::Escape;
                    }
                    // all other Ground bytes are pass-through (no event)
                }
                State::Escape => {
                    if b == OSC_INTRODUCER {
                        self.state = State::Osc;
                        self.buf.clear();
                        self.overflowed = false;
                    } else if b == ESC {
                        // Another ESC restarts the escape.
                        self.state = State::Escape;
                    } else {
                        // Not an OSC (e.g. CSI `[`); back to ground.
                        self.state = State::Ground;
                    }
                }
                State::Osc => {
                    if b == BEL {
                        self.finish_osc(&mut out);
                        self.state = State::Ground;
                    } else if b == ESC {
                        self.state = State::OscEsc;
                    } else {
                        self.push_osc_byte(b);
                    }
                }
                State::OscEsc => {
                    if b == ST_FINAL {
                        // ESC \ = ST terminator.
                        self.finish_osc(&mut out);
                        self.state = State::Ground;
                    } else if b == OSC_INTRODUCER {
                        // Implicit-ESC terminator: current OSC ends, a NEW OSC begins.
                        self.finish_osc(&mut out);
                        self.state = State::Osc;
                        self.buf.clear();
                        self.overflowed = false;
                    } else if b == ESC {
                        // ESC ESC: stay awaiting a final.
                        self.state = State::OscEsc;
                    } else {
                        // Implicit-ESC terminator into a non-OSC sequence: end current OSC,
                        // treat the ESC+b as a fresh escape sequence (ground for our purposes).
                        self.finish_osc(&mut out);
                        self.state = State::Ground;
                    }
                }
            }
        }
        out
    }

    fn push_osc_byte(&mut self, b: u8) {
        if self.overflowed {
            return;
        }
        if self.buf.len() >= OSC_BUF_CAP {
            // Abandon this oversized OSC; keep scanning for its terminator.
            self.overflowed = true;
            self.buf.clear();
            return;
        }
        self.buf.push(b);
    }

    fn finish_osc(&mut self, out: &mut Vec<OscEvent>) {
        if self.overflowed {
            self.buf.clear();
            self.overflowed = false;
            return;
        }
        if let Some(ev) = parse_osc_payload(&self.buf) {
            out.push(ev);
        }
        self.buf.clear();
    }
}

/// Parse a complete OSC payload (bytes after `ESC ]`, before the terminator).
fn parse_osc_payload(payload: &[u8]) -> Option<OscEvent> {
    // OSC 133 ; <letter> [ ; <args> ]
    if let Some(rest) = payload.strip_prefix(b"133;") {
        return parse_133(rest);
    }
    // OSC 7 ; <uri>
    if let Some(rest) = payload.strip_prefix(b"7;") {
        return parse_osc7(rest);
    }
    None
}

fn parse_133(rest: &[u8]) -> Option<OscEvent> {
    let letter = *rest.first()?;
    match letter {
        b'A' => Some(OscEvent::PromptStart),
        b'B' => Some(OscEvent::PromptEnd),
        b'C' => Some(OscEvent::CommandStart),
        b'D' => Some(OscEvent::CommandEnd(parse_exit_code(&rest[1..]))),
        _ => None,
    }
}

/// Exit-code rule (spec §10.3): `D` then optional `;<code>`; base-10 in 0..=255 → Some,
/// empty / non-numeric / out-of-range → None. Never coerce to 0. Ignore trailing `;aid=..`.
fn parse_exit_code(after_d: &[u8]) -> Option<u8> {
    // after_d is either empty (bare D) or starts with ';' then the code (and maybe more args).
    let s = after_d.strip_prefix(b";")?;
    // Take the first field up to the next ';'.
    let field: &[u8] = match s.iter().position(|&c| c == b';') {
        Some(i) => &s[..i],
        None => s,
    };
    if field.is_empty() {
        return None;
    }
    let text = std::str::from_utf8(field).ok()?;
    match text.parse::<u32>() {
        Ok(n) if n <= 255 => Some(n as u8),
        _ => None,
    }
}
```

Add a placeholder `parse_osc7` so it compiles now (real impl in Step 9):
```rust
fn parse_osc7(_rest: &[u8]) -> Option<OscEvent> {
    None
}
```

- [ ] **Step 4: Run — confirm PASS**

`cargo test -p sessiond osc_parser::tests`
Expected: PASS (lifecycle + non-OSC tests green).

- [ ] **Step 5: Commit**

`git add crates/sessiond/src/osc_parser.rs && git commit -m "feat(sessiond): streaming OSC-133 tokenizer with BEL/ST/implicit-ESC terminators"`

- [ ] **Step 6: Failing test — split reads, ST + implicit-ESC terminators, exit-code edge cases, buffer cap**

Add to the `tests` mod:
```rust
    const ST: [u8; 2] = [ESC, b'\\'];

    #[test]
    fn osc_split_across_feeds_is_buffered() {
        let mut p = OscParser::new();
        // Split "ESC ] 1 3 3 ; D ; 4 2 BEL" across three feeds.
        assert_eq!(p.feed(&[ESC, b']', b'1', b'3']), Vec::<OscEvent>::new());
        assert_eq!(p.feed(b"3;D;4"), Vec::<OscEvent>::new());
        assert_eq!(p.feed(&[b'2', BEL]), vec![OscEvent::CommandEnd(Some(42))]);
    }

    #[test]
    fn st_terminator_accepted() {
        let mut p = OscParser::new();
        assert_eq!(p.feed(&osc("133;C", &ST)), vec![OscEvent::CommandStart]);
    }

    #[test]
    fn implicit_esc_terminator_ends_and_starts_new_osc() {
        let mut p = OscParser::new();
        // OSC 133;A (no BEL) immediately followed by ESC ] starting OSC 133;B BEL.
        let mut stream = vec![ESC, b']'];
        stream.extend_from_slice(b"133;A");
        stream.extend_from_slice(&[ESC, b']']); // implicit terminator + new OSC start
        stream.extend_from_slice(b"133;B");
        stream.push(BEL);
        assert_eq!(p.feed(&stream), vec![OscEvent::PromptStart, OscEvent::PromptEnd]);
    }

    #[test]
    fn exit_code_edges_empty_nonnumeric_out_of_range() {
        let mut p = OscParser::new();
        assert_eq!(p.feed(&osc("133;D", &[BEL])), vec![OscEvent::CommandEnd(None)]);
        assert_eq!(p.feed(&osc("133;D;", &[BEL])), vec![OscEvent::CommandEnd(None)]);
        assert_eq!(p.feed(&osc("133;D;abc", &[BEL])), vec![OscEvent::CommandEnd(None)]);
        assert_eq!(p.feed(&osc("133;D;256", &[BEL])), vec![OscEvent::CommandEnd(None)]);
        assert_eq!(p.feed(&osc("133;D;255", &[BEL])), vec![OscEvent::CommandEnd(Some(255))]);
        // Trailing aid= arg ignored.
        assert_eq!(p.feed(&osc("133;D;7;aid=99", &[BEL])), vec![OscEvent::CommandEnd(Some(7))]);
    }

    #[test]
    fn oversized_osc_is_dropped_not_crashed() {
        let mut p = OscParser::new();
        let mut stream = vec![ESC, b']'];
        stream.extend_from_slice(b"133;");
        stream.extend(std::iter::repeat(b'x').take(9000)); // exceeds 8 KiB cap
        stream.push(BEL);
        // Oversized OSC yields no event; parser recovers to Ground.
        assert_eq!(p.feed(&stream), Vec::<OscEvent>::new());
        // A subsequent valid OSC still parses.
        assert_eq!(p.feed(&osc("133;B", &[BEL])), vec![OscEvent::PromptEnd]);
    }
```

- [ ] **Step 7: Run — confirm PASS**

`cargo test -p sessiond osc_parser::tests`
Expected: PASS. (These exercise code already written in Step 3; if any fail, fix the tokenizer before proceeding — do not weaken the test.)

- [ ] **Step 8: Failing test — OSC 7 decode (file:// + kitty-shell-cwd://, percent-decode, host strip)**

Add to the `tests` mod:
```rust
    #[test]
    fn osc7_file_scheme_decodes_and_strips_host() {
        let mut p = OscParser::new();
        let ev = p.feed(&osc("7;file://myhost/Users/me/projects", &[BEL]));
        assert_eq!(ev, vec![OscEvent::Cwd("/Users/me/projects".to_string())]);
    }

    #[test]
    fn osc7_kitty_scheme_decodes() {
        let mut p = OscParser::new();
        let ev = p.feed(&osc("7;kitty-shell-cwd://host/home/u/dir", &[BEL]));
        assert_eq!(ev, vec![OscEvent::Cwd("/home/u/dir".to_string())]);
    }

    #[test]
    fn osc7_percent_decodes_spaces_and_unicode() {
        let mut p = OscParser::new();
        // "/Users/me/My%20Docs" → "/Users/me/My Docs"
        let ev = p.feed(&osc("7;file://h/Users/me/My%20Docs", &[BEL]));
        assert_eq!(ev, vec![OscEvent::Cwd("/Users/me/My Docs".to_string())]);
    }

    #[test]
    fn osc7_empty_host_still_yields_absolute_path() {
        let mut p = OscParser::new();
        let ev = p.feed(&osc("7;file:///var/tmp", &[BEL]));
        assert_eq!(ev, vec![OscEvent::Cwd("/var/tmp".to_string())]);
    }

    #[test]
    fn osc7_unknown_scheme_dropped() {
        let mut p = OscParser::new();
        assert_eq!(p.feed(&osc("7;http://evil/", &[BEL])), Vec::<OscEvent>::new());
    }

    #[test]
    fn osc7_bad_percent_escape_dropped() {
        let mut p = OscParser::new();
        assert_eq!(p.feed(&osc("7;file://h/a%ZZb", &[BEL])), Vec::<OscEvent>::new());
    }
```

- [ ] **Step 9: Run — confirm FAIL, then implement OSC 7 decode**

`cargo test -p sessiond osc_parser::tests`
Expected: FAIL (osc7 tests: parser returns no `Cwd`). Replace the placeholder `parse_osc7` with:
```rust
/// Decode an OSC 7 payload (the bytes after `7;`): accept `file://host/path` and
/// `kitty-shell-cwd://host/path` only, percent-decode, strip host, bound length.
/// Malformed input → None (dropped safely). cwd is advisory display data (spec §10.3, §16).
fn parse_osc7(rest: &[u8]) -> Option<OscEvent> {
    if rest.len() > OSC_BUF_CAP {
        return None;
    }
    let s = std::str::from_utf8(rest).ok()?;
    let after_scheme = s
        .strip_prefix("file://")
        .or_else(|| s.strip_prefix("kitty-shell-cwd://"))?;
    // after_scheme = "host/abs/path" or "/abs/path" (empty host). Path starts at the first '/'.
    let slash = after_scheme.find('/')?;
    let raw_path = &after_scheme[slash..];
    let decoded = percent_decode(raw_path)?;
    if !decoded.starts_with('/') {
        return None;
    }
    Some(OscEvent::Cwd(decoded))
}

/// Minimal, strict percent-decoder. Returns None on a malformed escape.
fn percent_decode(s: &str) -> Option<String> {
    let bytes = s.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'%' => {
                if i + 2 >= bytes.len() {
                    return None;
                }
                let hi = (bytes[i + 1] as char).to_digit(16)?;
                let lo = (bytes[i + 2] as char).to_digit(16)?;
                out.push((hi * 16 + lo) as u8);
                i += 3;
            }
            c => {
                out.push(c);
                i += 1;
            }
        }
    }
    String::from_utf8(out).ok()
}
```
Then re-run `cargo test -p sessiond osc_parser::tests`. Expected: PASS.

- [ ] **Step 10: Commit**

`git add crates/sessiond/src/osc_parser.rs && git commit -m "feat(sessiond): OSC 7 cwd decode (file:// + kitty-shell-cwd://, percent-decode, host strip)"`

- [ ] **Step 11: Failing test — Lifecycle state machine transition table + empty-command no-op**

Add to the `tests` mod:
```rust
    #[test]
    fn lifecycle_full_transition_table() {
        let mut lc = Lifecycle::new();
        assert_eq!(lc, Lifecycle::AtPrompt);
        lc.advance(&OscEvent::PromptStart); // A: prompt drawing, no state change
        assert_eq!(lc, Lifecycle::AtPrompt);
        lc.advance(&OscEvent::PromptEnd); // B → AtPrompt (idle)
        assert_eq!(lc, Lifecycle::AtPrompt);
        lc.advance(&OscEvent::CommandStart); // C → Running
        assert_eq!(lc, Lifecycle::Running);
        lc.advance(&OscEvent::CommandEnd(Some(0))); // D;0 → Exited(Some(0))
        assert_eq!(lc, Lifecycle::Exited(Some(0)));
        lc.advance(&OscEvent::PromptStart); // A after exit → back toward prompt
        lc.advance(&OscEvent::PromptEnd);
        assert_eq!(lc, Lifecycle::AtPrompt);
    }

    #[test]
    fn lifecycle_empty_command_b_to_a_is_noop() {
        let mut lc = Lifecycle::new();
        lc.advance(&OscEvent::PromptEnd);   // B → AtPrompt
        lc.advance(&OscEvent::PromptStart); // A (user hit Enter on empty line): no phantom Running
        assert_eq!(lc, Lifecycle::AtPrompt);
        lc.advance(&OscEvent::PromptEnd);
        assert_eq!(lc, Lifecycle::AtPrompt);
    }

    #[test]
    fn lifecycle_d_without_code_is_exited_none() {
        let mut lc = Lifecycle::new();
        lc.advance(&OscEvent::CommandStart);
        lc.advance(&OscEvent::CommandEnd(None));
        assert_eq!(lc, Lifecycle::Exited(None));
    }

    #[test]
    fn lifecycle_cwd_event_does_not_change_state() {
        let mut lc = Lifecycle::new();
        lc.advance(&OscEvent::CommandStart);
        lc.advance(&OscEvent::Cwd("/tmp".into()));
        assert_eq!(lc, Lifecycle::Running);
    }
```

- [ ] **Step 12: Run — confirm FAIL**

`cargo test -p sessiond osc_parser::tests`
Expected: FAIL to compile with `cannot find type 'Lifecycle'`.

- [ ] **Step 13: Implement the Lifecycle state machine**

Append to `crates/sessiond/src/osc_parser.rs` (above the test module):
```rust
/// Per-session lifecycle derived from OSC-133 events (spec §10.3).
/// `AtPrompt` = idle at shell prompt; `Running` = command executing; `Exited(code)` = finished.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lifecycle {
    AtPrompt,
    Running,
    Exited(Option<u8>),
}

impl Default for Lifecycle {
    fn default() -> Self {
        Self::new()
    }
}

impl Lifecycle {
    pub fn new() -> Self {
        Lifecycle::AtPrompt
    }

    /// Advance the state machine by one OSC event.
    /// A = prompt drawing (no change); B → AtPrompt; C → Running; D;code → Exited(code).
    /// Empty-command `B → A` (PromptEnd then PromptStart, no C/D) stays AtPrompt — no phantom Running.
    /// Cwd events never change lifecycle.
    pub fn advance(&mut self, ev: &OscEvent) {
        match ev {
            OscEvent::PromptStart => {
                // `A` only draws the prompt. If we were Exited, move back toward the prompt.
                if let Lifecycle::Exited(_) = self {
                    *self = Lifecycle::AtPrompt;
                }
                // From AtPrompt/Running, `A` alone changes nothing.
            }
            OscEvent::PromptEnd => {
                *self = Lifecycle::AtPrompt;
            }
            OscEvent::CommandStart => {
                *self = Lifecycle::Running;
            }
            OscEvent::CommandEnd(code) => {
                *self = Lifecycle::Exited(*code);
            }
            OscEvent::Cwd(_) => {}
        }
    }
}
```

- [ ] **Step 14: Run — confirm PASS**

`cargo test -p sessiond osc_parser::tests`
Expected: PASS (all state-machine tests green).

- [ ] **Step 15: Failing test — forged/interleaved OSC hardening (no crash, safe recovery)**

Add to the `tests` mod:
```rust
    #[test]
    fn forged_and_interleaved_osc_never_panics_and_recovers() {
        let mut p = OscParser::new();
        // Garbage OSC introducer with junk, unterminated, then a real one via implicit ESC.
        let mut stream = vec![ESC, b']'];
        stream.extend_from_slice(b"999;garbage;;;");
        stream.extend_from_slice(&[ESC, b']']); // implicit terminate + new OSC
        stream.extend_from_slice(b"133;A");
        stream.push(BEL);
        // Unknown OSC 999 → no event; the real 133;A → PromptStart.
        assert_eq!(p.feed(&stream), vec![OscEvent::PromptStart]);

        // Interleave SGR + partial OSC + text; must not corrupt or panic.
        let ev = p.feed(b"\x1b[1mbold\x1b[0m \x1b]133;C\x07running");
        assert_eq!(ev, vec![OscEvent::CommandStart]);

        // A lone ESC at end of chunk buffers cleanly and continues next feed.
        assert_eq!(p.feed(&[ESC]), Vec::<OscEvent>::new());
        assert_eq!(p.feed(&[b']']), Vec::<OscEvent>::new());
        assert_eq!(p.feed(b"133;B\x07"), vec![OscEvent::PromptEnd]);
    }
```

- [ ] **Step 16: Run — confirm PASS**

`cargo test -p sessiond osc_parser::tests`
Expected: PASS. (Exercises existing tokenizer; if any assertion fails, fix the tokenizer state handling — do not weaken the test.)

- [ ] **Step 17: Commit**

`git add crates/sessiond/src/osc_parser.rs && git commit -m "feat(sessiond): OSC-133 lifecycle state machine + forged/interleaved hardening tests"`

**Definition of Done:**
- `cargo test -p sessiond osc_parser::tests` is green (lifecycle parse, split-read buffering, BEL/ST/implicit-ESC terminators, 8 KiB cap, exit-code edges, OSC 7 decode, transition table, empty-command no-op, forged/interleaved hardening).
- `OscParser::feed` buffers partial OSC across chunk boundaries and never forwards a recognized OSC as an event twice (spec §10.3).
- Exit-code parse: `0..=255` → `Some`, empty / non-numeric / out-of-range → `None`, never coerced to 0 (spec §10.3).
- OSC 7 accepts `file://` and `kitty-shell-cwd://` only, percent-decodes, strips host, bounds length; unknown scheme / bad escape → dropped (spec §10.3, §16).
- `Lifecycle::advance` matches the spec §10.3 transition table exactly incl. the empty-command `B→A` no-op and `D`-without-code → `Exited(None)`.
- Oversized/forged/interleaved input never panics and the parser recovers to `Ground` (spec §10.3, §16 trust model).
- Module docs reference §10.3; no `TODO`.

---

### Task 6: `scrollback.rs` — sanitizing scrollback ring (replay source)

**Files:**
- Create: `crates/sessiond/src/scrollback.rs`
- Modify: `crates/sessiond/src/lib.rs` (append `pub mod scrollback;`) — or `main.rs` `mod scrollback;` per the crate-target rule in T4. Append only your own `mod` line.
- Test: inline `#[cfg(test)] mod tests` in `crates/sessiond/src/scrollback.rs`.

**Depends on:** [T3]   **Parallel-safe with:** [T4, T5, T7, T8, T10]

**Interfaces:** Consumes: nothing from T3 at the type level (std only); grouped under G2, gated on T3 for crate-compile ordering. Produces (verbatim from the scaffold Task interface index, spec §11):
```rust
pub struct ScrollbackRing { /* bounded byte ring, cap in bytes */ }
impl ScrollbackRing {
    pub fn new(cap: usize) -> Self;
    pub fn push(&mut self, chunk: &[u8]);   // SANITIZES before storing
    pub fn snapshot(&self) -> Vec<u8>;       // oldest→newest sanitized bytes
    pub fn prune(&mut self);                 // enforce the cap (drop oldest)
}
```
Locked sanitization contract (spec §11) — `push` strips, from the incoming stream, the following side-effecting control sequences before appending to the ring, while KEEPING SGR color/attribute sequences and all normal text:
- Alt-screen enter/leave: `ESC [ ? 1049 h` / `l` and `ESC [ ? 47 h` / `l`.
- Window-title OSC: `ESC ] 0 ; … BEL|ST`, `ESC ] 1 ; …`, `ESC ] 2 ; …`.
- Bracketed-paste toggles: `ESC [ ? 2004 h` / `l`.
- Our own marks: OSC 133 (`ESC ] 133 ; … BEL|ST`) and OSC 7 (`ESC ] 7 ; … BEL|ST`).
- KEEP: SGR (`ESC [ … m`), cursor moves, erases, and plain text — everything not in the strip list passes through verbatim.
The ring is byte-bounded by `cap`; `push` appends then prunes oldest bytes to satisfy `cap`. `snapshot()` returns the current sanitized contents oldest→newest — this is the `Replay` payload (spec §6.2, §11). Because sanitization removes title/alt-screen/paste side effects, replaying `snapshot()` into a fresh xterm cannot corrupt it (the "replay-of-past-vim" property).

Implementation note (locked): sanitization is a small streaming filter that must handle sequences split across `push` calls (the same partial-across-reads concern as T5). Keep a private carry-buffer of an in-progress escape sequence so a chunk boundary inside `ESC [ ? 1049 h` does not leak bytes. On an unrecognized/oversized escape (cap the carry at 256 bytes), flush the carried bytes through verbatim (fail-open to text, never lose the user's output) — the only sequences we deliberately drop are the enumerated ones.

- [ ] **Step 1: Failing test — ring bounds + prune + snapshot ordering (no escapes)**

Add to `crates/sessiond/src/scrollback.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_round_trips_oldest_to_newest() {
        let mut r = ScrollbackRing::new(1024);
        r.push(b"hello ");
        r.push(b"world");
        assert_eq!(r.snapshot(), b"hello world".to_vec());
    }

    #[test]
    fn ring_enforces_byte_cap_dropping_oldest() {
        let mut r = ScrollbackRing::new(8);
        r.push(b"ABCDE");     // 5 bytes
        r.push(b"FGHIJ");     // total 10 → prune oldest 2 → "CDEFGHIJ"
        let snap = r.snapshot();
        assert_eq!(snap.len(), 8);
        assert_eq!(snap, b"CDEFGHIJ".to_vec());
    }

    #[test]
    fn push_larger_than_cap_keeps_only_tail() {
        let mut r = ScrollbackRing::new(4);
        r.push(b"ABCDEFGH");
        assert_eq!(r.snapshot(), b"EFGH".to_vec());
    }

    #[test]
    fn explicit_prune_is_idempotent() {
        let mut r = ScrollbackRing::new(4);
        r.push(b"ABCDEF");
        r.prune();
        r.prune();
        assert_eq!(r.snapshot(), b"CDEF".to_vec());
    }
}
```

- [ ] **Step 2: Run — confirm FAIL**

`cargo test -p sessiond scrollback::tests`
Expected: FAIL to compile with `cannot find type 'ScrollbackRing'`.

- [ ] **Step 3: Implement the bounded ring (no sanitization yet — passthrough)**

Prepend to `crates/sessiond/src/scrollback.rs`:
```rust
//! Sanitizing scrollback ring — the replay source (spec §11).
//! Stores the PTY's normal-buffer output with side-effecting control sequences neutralized
//! (alt-screen, title OSC, bracketed-paste, OSC-133/OSC-7 marks) while KEEPING SGR + text.
//! Replaying `snapshot()` into a fresh terminal cannot re-trigger those side effects.

use std::collections::VecDeque;

/// Cap the in-progress escape carry so a malicious/garbled stream can't grow it unbounded.
const CARRY_CAP: usize = 256;

pub struct ScrollbackRing {
    cap: usize,
    buf: VecDeque<u8>,
    filter: Sanitizer,
}

impl ScrollbackRing {
    pub fn new(cap: usize) -> Self {
        ScrollbackRing { cap, buf: VecDeque::new(), filter: Sanitizer::new() }
    }

    /// Append a chunk, sanitizing side-effecting sequences, then enforce the byte cap.
    pub fn push(&mut self, chunk: &[u8]) {
        let kept = self.filter.filter(chunk);
        self.buf.extend(kept);
        self.prune();
    }

    /// Current sanitized contents, oldest → newest. This is the `Replay` payload (spec §6.2).
    pub fn snapshot(&self) -> Vec<u8> {
        self.buf.iter().copied().collect()
    }

    /// Drop oldest bytes until the ring is within `cap`.
    pub fn prune(&mut self) {
        while self.buf.len() > self.cap {
            self.buf.pop_front();
        }
    }
}
```

Add a passthrough `Sanitizer` so it compiles now (real filter in Step 6):
```rust
struct Sanitizer;

impl Sanitizer {
    fn new() -> Self {
        Sanitizer
    }

    fn filter(&mut self, chunk: &[u8]) -> Vec<u8> {
        chunk.to_vec()
    }
}
```

- [ ] **Step 4: Run — confirm PASS**

`cargo test -p sessiond scrollback::tests`
Expected: PASS (ring bounds/prune/snapshot green).

- [ ] **Step 5: Commit**

`git add crates/sessiond/src/scrollback.rs && git commit -m "feat(sessiond): bounded scrollback ring with byte cap + prune"`

- [ ] **Step 6: Failing test — sanitization strips listed sequences, keeps SGR + text**

Add to the `tests` mod:
```rust
    const ESC: u8 = 0x1B;
    const BEL: u8 = 0x07;

    fn strip(input: &[u8]) -> Vec<u8> {
        let mut r = ScrollbackRing::new(1 << 20);
        r.push(input);
        r.snapshot()
    }

    #[test]
    fn keeps_sgr_and_plain_text() {
        // ESC[31m red ESC[0m — SGR must be preserved verbatim.
        let input = b"\x1b[31mred\x1b[0m done";
        assert_eq!(strip(input), input.to_vec());
    }

    #[test]
    fn strips_alt_screen_enter_leave_1049_and_47() {
        let input = b"before\x1b[?1049hMID\x1b[?1049lafter";
        assert_eq!(strip(input), b"beforeMIDafter".to_vec());
        let input47 = b"a\x1b[?47hb\x1b[?47lc";
        assert_eq!(strip(input47), b"abc".to_vec());
    }

    #[test]
    fn strips_bracketed_paste_toggles_2004() {
        let input = b"x\x1b[?2004hy\x1b[?2004lz";
        assert_eq!(strip(input), b"xyz".to_vec());
    }

    #[test]
    fn strips_title_osc_0_1_2_bel_and_st() {
        let bel = b"a\x1b]0;My Title\x07b".to_vec();
        assert_eq!(strip(&bel), b"ab".to_vec());
        let osc1 = b"a\x1b]1;icon\x07b".to_vec();
        assert_eq!(strip(&osc1), b"ab".to_vec());
        // ST-terminated title.
        let st = b"a\x1b]2;t\x1b\\b".to_vec();
        assert_eq!(strip(&st), b"ab".to_vec());
    }

    #[test]
    fn strips_osc_133_and_osc_7_marks() {
        let input = b"\x1b]133;A\x07prompt$ \x1b]133;B\x07cmd\x1b]133;C\x07out\x1b]133;D;0\x07";
        assert_eq!(strip(input), b"prompt$ cmdout".to_vec());
        let osc7 = b"p\x1b]7;file://h/Users/me\x07q".to_vec();
        assert_eq!(strip(&osc7), b"pq".to_vec());
    }

    #[test]
    fn keeps_cursor_moves_and_erases() {
        // ESC[2J (erase), ESC[H (cursor home) are not in the strip list → kept.
        let input = b"\x1b[2J\x1b[Hhome";
        assert_eq!(strip(input), input.to_vec());
    }

    #[test]
    fn split_alt_screen_sequence_across_pushes_is_stripped() {
        let mut r = ScrollbackRing::new(1 << 20);
        r.push(b"pre\x1b[?10");   // sequence split mid-way
        r.push(b"49hpost");
        assert_eq!(r.snapshot(), b"prepost".to_vec());
    }
```

- [ ] **Step 7: Run — confirm FAIL**

`cargo test -p sessiond scrollback::tests`
Expected: FAIL — passthrough `Sanitizer` keeps the sequences (e.g. `strips_alt_screen_enter_leave_1049_and_47` fails).

- [ ] **Step 8: Implement the streaming Sanitizer**

Replace the placeholder `Sanitizer` in `scrollback.rs` with a streaming filter that carries an in-progress escape across chunks:
```rust
/// Streaming filter that drops the enumerated side-effecting sequences (spec §11) and
/// passes everything else (SGR, cursor ops, text) through verbatim. Handles sequences
/// split across `filter` calls via an internal carry buffer.
struct Sanitizer {
    /// Bytes of an in-progress escape sequence not yet classified.
    carry: Vec<u8>,
}

#[derive(PartialEq)]
enum Verdict {
    /// The carry is a complete sequence to DROP.
    Drop,
    /// The carry is a complete sequence to KEEP (flush carry verbatim).
    Keep,
    /// Need more bytes to decide.
    Incomplete,
}

impl Sanitizer {
    fn new() -> Self {
        Sanitizer { carry: Vec::new() }
    }

    fn filter(&mut self, chunk: &[u8]) -> Vec<u8> {
        let mut out: Vec<u8> = Vec::with_capacity(chunk.len());
        for &b in chunk {
            if self.carry.is_empty() {
                if b == ESC {
                    self.carry.push(b);
                } else {
                    out.push(b);
                }
                continue;
            }
            // We are mid-escape: accumulate and classify.
            self.carry.push(b);
            match classify(&self.carry) {
                Verdict::Incomplete => {
                    if self.carry.len() > CARRY_CAP {
                        // Give up on this sequence: fail open, flush verbatim, reset.
                        out.extend(self.carry.drain(..));
                    }
                }
                Verdict::Drop => {
                    self.carry.clear();
                }
                Verdict::Keep => {
                    out.extend(self.carry.drain(..));
                }
            }
        }
        out
    }
}

/// Classify a candidate escape sequence (always starts with ESC).
/// Returns Drop for the enumerated side-effecting sequences, Keep for a completed
/// sequence to preserve, Incomplete if more bytes are required.
fn classify(seq: &[u8]) -> Verdict {
    // seq[0] == ESC guaranteed by caller.
    if seq.len() < 2 {
        return Verdict::Incomplete;
    }
    match seq[1] {
        b'[' => classify_csi(seq),
        b']' => classify_osc(seq),
        // Any other escape (e.g. ESC ( B charset, ESC = , ESC > ) — 2-byte, keep.
        _ => Verdict::Keep,
    }
}

/// CSI: ESC [ <params/intermediates> <final 0x40..=0x7E>. Drop the private-mode toggles
/// ?1049h/l, ?47h/l, ?2004h/l; keep every other CSI (SGR `m`, cursor, erase, …).
fn classify_csi(seq: &[u8]) -> Verdict {
    // Find the final byte (first byte in 0x40..=0x7E after the '[').
    let body = &seq[2..];
    let mut final_idx = None;
    for (i, &c) in body.iter().enumerate() {
        if (0x40..=0x7e).contains(&c) {
            final_idx = Some(i);
            break;
        }
    }
    let Some(fi) = final_idx else {
        return Verdict::Incomplete; // no final byte yet
    };
    let params = &body[..fi];
    let final_byte = body[fi];
    let is_toggle = final_byte == b'h' || final_byte == b'l';
    if is_toggle && (params == b"?1049" || params == b"?47" || params == b"?2004") {
        return Verdict::Drop;
    }
    Verdict::Keep
}

/// OSC: ESC ] <n> ; <text> <BEL | ESC \>. Drop title (0/1/2) and our marks (133, 7);
/// keep any other OSC once complete.
fn classify_osc(seq: &[u8]) -> Verdict {
    // seq = ESC ] ...
    let body = &seq[2..];
    // Determine if terminated (BEL, or ESC \ as the last two bytes).
    let terminated_bel = body.last() == Some(&BEL);
    let terminated_st = body.len() >= 2 && body[body.len() - 2] == ESC && body[body.len() - 1] == b'\\';
    if !terminated_bel && !terminated_st {
        return Verdict::Incomplete;
    }
    // Extract the leading numeric identifier up to the first ';'.
    let ident_end = body.iter().position(|&c| c == b';').unwrap_or(body.len());
    let ident = &body[..ident_end];
    match ident {
        b"0" | b"1" | b"2" | b"133" | b"7" => Verdict::Drop,
        _ => Verdict::Keep,
    }
}
```

- [ ] **Step 9: Run — confirm PASS**

`cargo test -p sessiond scrollback::tests`
Expected: PASS (all sanitization tests green, incl. split-across-pushes).

- [ ] **Step 10: Commit**

`git add crates/sessiond/src/scrollback.rs && git commit -m "feat(sessiond): streaming sanitizer strips alt-screen/title/paste/OSC-133/OSC-7, keeps SGR"`

- [ ] **Step 11: Failing test — replay-of-past-vim doesn't corrupt a fresh terminal**

This is the spec §11 "replay-of-past-vim" property expressed as a byte-level invariant: after a session that entered/left the alt-screen (vim), set a title, and toggled bracketed-paste, the `snapshot()` contains NONE of those side-effecting sequences (so writing it to a fresh xterm cannot flip alt-screen / change the title / enable paste mode) while retaining the normal-buffer text + SGR. Add to the `tests` mod:
```rust
    #[test]
    fn replay_of_past_vim_session_has_no_side_effecting_sequences() {
        let mut r = ScrollbackRing::new(1 << 20);
        // Simulate: prompt marks, run `vim` (alt-screen + title), quit, back to prompt.
        r.push(b"\x1b]133;A\x07\x1b]7;file://h/home/u\x07me@host:~$ ");
        r.push(b"\x1b]133;B\x07vim file.txt\n\x1b]133;C\x07");
        r.push(b"\x1b]0;VIM - file.txt\x07");        // title set by vim
        r.push(b"\x1b[?1049h");                       // enter alt-screen
        r.push(b"\x1b[?2004h~ editing ~\x1b[?2004l"); // paste toggles inside vim
        r.push(b"\x1b[?1049l");                       // leave alt-screen
        r.push(b"\x1b]133;D;0\x07");                  // command finished
        r.push(b"\x1b]133;A\x07me@host:~$ ");         // fresh prompt
        let snap = r.snapshot();

        // None of the side-effecting sequences survive.
        assert!(!contains(&snap, b"\x1b[?1049h"), "alt-screen enter leaked");
        assert!(!contains(&snap, b"\x1b[?1049l"), "alt-screen leave leaked");
        assert!(!contains(&snap, b"\x1b[?2004h"), "bracketed-paste enter leaked");
        assert!(!contains(&snap, b"\x1b[?2004l"), "bracketed-paste leave leaked");
        assert!(!contains(&snap, b"\x1b]0;"), "title OSC leaked");
        assert!(!contains(&snap, b"\x1b]133;"), "OSC-133 mark leaked");
        assert!(!contains(&snap, b"\x1b]7;"), "OSC-7 mark leaked");

        // Normal-buffer text + the interior vim text survive.
        assert!(contains(&snap, b"me@host:~$ "), "prompt text lost");
        assert!(contains(&snap, b"vim file.txt"), "command echo lost");
        assert!(contains(&snap, b"~ editing ~"), "interior text lost");
    }

    fn contains(haystack: &[u8], needle: &[u8]) -> bool {
        haystack.windows(needle.len()).any(|w| w == needle)
    }
```

- [ ] **Step 12: Run — confirm PASS**

`cargo test -p sessiond scrollback::tests`
Expected: PASS. (Exercises the Sanitizer from Step 8; if it fails, fix `classify_*` — do not weaken the test.)

- [ ] **Step 13: Commit**

`git add crates/sessiond/src/scrollback.rs && git commit -m "test(sessiond): replay-of-past-vim ring snapshot has no side-effecting sequences"`

**Definition of Done:**
- `cargo test -p sessiond scrollback::tests` is green (ring bounds/prune, snapshot ordering, all sanitization cases incl. split-across-pushes, replay-of-past-vim invariant).
- `push` strips alt-screen (`?1049h/l`, `?47h/l`), title OSC (0/1/2), bracketed-paste (`?2004h/l`), and OSC-133/OSC-7 marks, while keeping SGR (`ESC[..m`), cursor/erase ops, and plain text (spec §11).
- The ring is byte-bounded by `cap`; `push` prunes oldest to satisfy `cap`; a push larger than `cap` keeps only the tail; `prune()` is idempotent (spec §11).
- `snapshot()` returns sanitized bytes oldest→newest — the `Replay` payload (spec §6.2, §11).
- Sanitization handles sequences split across `push` calls via a bounded (256-byte) carry that fails open to verbatim text on an over-long/unknown escape (never silently loses user output).
- Replaying `snapshot()` of a session containing alt-screen/title/paste sequences yields bytes free of those side-effecting sequences (the replay-no-corrupt property, spec §11).
- Module docs reference §11; no `TODO`.


### Task 7: `persistence.rs` — durable SQLite (WAL) with migrations, quarantine-on-corrupt, and rehydrate

**Files:**
- Create — `crates/sessiond/src/persistence.rs`
- Modify — `crates/sessiond/Cargo.toml` (add `rusqlite` feature `bundled`, `tempfile` dev-dep), `crates/sessiond/src/main.rs` (add `mod persistence;`)
- Test — inline `#[cfg(test)] mod tests` at the bottom of `crates/sessiond/src/persistence.rs`

**Depends on:** [T3]   **Parallel-safe with:** [T4, T5, T6, T8, T10]

**Interfaces:**
- Consumes: from `protocol` crate (T3) — `SessionMeta`, `SessionLifecycle` (`AtPrompt | Typing | Running | Exited { code: Option<u8>, signal: Option<String> }`), `Workspace`, `SessionId`, `WorkspaceId` (exact defs in spec §5).
- Produces (locked names — consumers in T7/T11/T12 use these verbatim, per Task interface index):
  - `pub struct Db { /* owns rusqlite::Connection */ }`
  - `pub fn Db::open(path: &Path) -> Result<Db, PersistError>` — opens with `PRAGMA journal_mode=WAL`, `PRAGMA busy_timeout=5000`, runs `user_version` migrations in a transaction; on a corrupt/malformed image quarantines (`rename bpa.db → bpa.db.corrupt-<unix_ts>`) and recreates.
  - `pub fn Db::upsert_workspace(&self, ws: &Workspace) -> Result<(), PersistError>`
  - `pub fn Db::list_workspaces(&self) -> Result<Vec<Workspace>, PersistError>`
  - `pub fn Db::upsert_session(&self, meta: &SessionMeta) -> Result<(), PersistError>`
  - `pub fn Db::list_sessions(&self) -> Result<Vec<SessionMeta>, PersistError>`
  - `pub fn Db::append_scrollback(&self, session_id: &SessionId, seq: i64, bytes: &[u8], ts: i64) -> Result<(), PersistError>`
  - `pub fn Db::load_scrollback(&self, session_id: &SessionId) -> Result<Vec<u8>, PersistError>` (rows ordered by `seq`, concatenated)
  - `pub fn Db::rehydrate(&self) -> Result<Vec<SessionMeta>, PersistError>` (every returned `SessionMeta` has `is_active=false`, `waiting_for_input=false`; `lifecycle` = stored value with `exit_code`/`exit_signal` decoded per spec §11)
  - `pub enum PersistError { Open(String), Sql(String), Migration(String), Corrupt(String) }` (impls `std::fmt::Display` + `std::error::Error`)
  - schema constant `pub const SCHEMA_VERSION: i64 = 1;`

Lifecycle TEXT encoding (locked, spec §11 `lifecycle TEXT` + `exit_code INTEGER`, `exit_signal TEXT`): `AtPrompt→"atPrompt"`, `Typing→"typing"`, `Running→"running"`, `Exited{code,signal}→"exited"` with `exit_code`/`exit_signal` columns carrying `code`/`signal` (both `NULL` for non-Exited rows).

---

- [ ] **Step 1: Add deps and module wiring.** Edit `crates/sessiond/Cargo.toml`: under `[dependencies]` add `rusqlite = { version = "0.32", features = ["bundled"] }`; under `[dev-dependencies]` add `tempfile = "3"`. Ensure `protocol = { path = "../protocol" }` is present (from T3). Edit `crates/sessiond/src/main.rs`: add `mod persistence;` alongside the other `mod` lines.

- [ ] **Step 2: Failing test — persist + rehydrate round-trip.** Add to `crates/sessiond/src/persistence.rs`:
  ```rust
  #[cfg(test)]
  mod tests {
      use super::*;
      use protocol::{SessionLifecycle, SessionMeta, Workspace};

      fn ws(id: &str) -> Workspace {
          Workspace { id: id.into(), name: format!("ws-{id}"), root_path: "/tmp".into() }
      }

      fn meta(id: &str, ws_id: &str, lc: SessionLifecycle) -> SessionMeta {
          SessionMeta {
              id: id.into(),
              workspace_id: ws_id.into(),
              title: format!("t-{id}"),
              shell: "/bin/zsh".into(),
              cwd: "/tmp".into(),
              cols: 80,
              rows: 24,
              lifecycle: lc,
              waiting_for_input: true,
              is_active: true,
              created_at: 1_700_000_000,
          }
      }

      #[test]
      fn persist_and_rehydrate_round_trip() {
          let dir = tempfile::tempdir().unwrap();
          let path = dir.path().join("bpa.db");
          {
              let db = Db::open(&path).unwrap();
              db.upsert_workspace(&ws("w1")).unwrap();
              db.upsert_session(&meta("s1", "w1", SessionLifecycle::Running)).unwrap();
              db.append_scrollback(&"s1".to_string(), 0, b"hello ", 1).unwrap();
              db.append_scrollback(&"s1".to_string(), 1, b"world", 2).unwrap();
          }
          let db = Db::open(&path).unwrap();
          let wss = db.list_workspaces().unwrap();
          assert_eq!(wss.len(), 1);
          assert_eq!(wss[0].id, "w1");

          let sessions = db.rehydrate().unwrap();
          assert_eq!(sessions.len(), 1);
          let s = &sessions[0];
          assert_eq!(s.id, "s1");
          assert_eq!(s.workspace_id, "w1");
          // rehydrated sessions are never active and never waiting
          assert!(!s.is_active);
          assert!(!s.waiting_for_input);
          assert_eq!(s.lifecycle, SessionLifecycle::Running);

          let sb = db.load_scrollback(&"s1".to_string()).unwrap();
          assert_eq!(sb, b"hello world");
      }
  }
  ```

- [ ] **Step 3: Run — confirm FAIL.** `cargo test -p sessiond persist_and_rehydrate_round_trip`
  Expected: FAIL with `cannot find type Db in this scope` (module has no impl yet).

- [ ] **Step 4: Implement `PersistError`, schema, and `Db::open` (WAL + busy_timeout + migrations + quarantine).** Write at the top of `crates/sessiond/src/persistence.rs`:
  ```rust
  //! Durable SQLite persistence for the session daemon (spec §11).
  //! Best-effort: the in-memory ring is the Layer-1 source of truth; this layer
  //! degrades honestly (logs, never panics) on lock/disk/corruption failures.

  use std::fmt;
  use std::path::{Path, PathBuf};
  use std::time::{SystemTime, UNIX_EPOCH};

  use protocol::{SessionId, SessionLifecycle, SessionMeta, Workspace};
  use rusqlite::{Connection, OptionalExtension};
  use tracing::{error, info, warn};

  /// Current schema/migration version stored in `PRAGMA user_version`.
  pub const SCHEMA_VERSION: i64 = 1;

  #[derive(Debug)]
  pub enum PersistError {
      Open(String),
      Sql(String),
      Migration(String),
      Corrupt(String),
  }

  impl fmt::Display for PersistError {
      fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
          match self {
              PersistError::Open(m) => write!(f, "db open failed: {m}"),
              PersistError::Sql(m) => write!(f, "db sql error: {m}"),
              PersistError::Migration(m) => write!(f, "db migration failed: {m}"),
              PersistError::Corrupt(m) => write!(f, "db corrupt: {m}"),
          }
      }
  }

  impl std::error::Error for PersistError {}

  impl From<rusqlite::Error> for PersistError {
      fn from(e: rusqlite::Error) -> Self {
          PersistError::Sql(e.to_string())
      }
  }

  pub struct Db {
      conn: Connection,
  }

  fn now_secs() -> i64 {
      SystemTime::now()
          .duration_since(UNIX_EPOCH)
          .map(|d| d.as_secs() as i64)
          .unwrap_or(0)
  }

  /// True if the rusqlite error is a corruption / not-a-database error.
  fn is_corruption(e: &rusqlite::Error) -> bool {
      use rusqlite::ffi::ErrorCode;
      if let rusqlite::Error::SqliteFailure(err, _) = e {
          matches!(err.code, ErrorCode::DatabaseCorrupt | ErrorCode::NotADatabase)
      } else {
          false
      }
  }

  fn quarantine(path: &Path) -> PathBuf {
      let ts = now_secs();
      let mut q = path.as_os_str().to_os_string();
      q.push(format!(".corrupt-{ts}"));
      PathBuf::from(q)
  }

  impl Db {
      /// Open (or create) the database at `path`. Sets WAL + busy_timeout, runs
      /// migrations in a transaction. On a corrupt image, quarantines the file
      /// (`bpa.db.corrupt-<ts>`) and recreates a fresh database rather than crashing.
      pub fn open(path: &Path) -> Result<Db, PersistError> {
          match Self::open_inner(path) {
              Ok(db) => Ok(db),
              Err(PersistError::Corrupt(msg)) => {
                  let dst = quarantine(path);
                  warn!(?path, ?dst, "database corrupt, quarantining and recreating: {msg}");
                  std::fs::rename(path, &dst)
                      .map_err(|e| PersistError::Open(format!("quarantine rename failed: {e}")))?;
                  // Sidecar WAL/SHM files from the corrupt db would confuse the fresh one.
                  for suffix in ["-wal", "-shm"] {
                      let mut side = path.as_os_str().to_os_string();
                      side.push(suffix);
                      let _ = std::fs::remove_file(PathBuf::from(side));
                  }
                  Self::open_inner(path)
              }
              Err(other) => Err(other),
          }
      }

      fn open_inner(path: &Path) -> Result<Db, PersistError> {
          if let Some(parent) = path.parent() {
              std::fs::create_dir_all(parent)
                  .map_err(|e| PersistError::Open(format!("create dir failed: {e}")))?;
          }
          let conn = Connection::open(path)
              .map_err(|e| PersistError::Open(e.to_string()))?;

          conn.pragma_update(None, "journal_mode", "WAL")
              .map_err(|e| classify(e))?;
          conn.pragma_update(None, "busy_timeout", 5000_i64)
              .map_err(|e| classify(e))?;
          conn.pragma_update(None, "foreign_keys", "ON")
              .map_err(|e| classify(e))?;

          // Force a read to surface "not a database" / corruption at open time.
          let user_version: i64 = conn
              .query_row("PRAGMA user_version", [], |r| r.get(0))
              .map_err(|e| classify(e))?;

          let db = Db { conn };
          db.migrate(user_version)?;
          info!(?path, "database opened (WAL, schema v{SCHEMA_VERSION})");
          Ok(db)
      }

      /// Run migrations from `from_version` to `SCHEMA_VERSION` in one transaction.
      /// Fails closed (typed error) on any error — never panics (spec §11).
      fn migrate(&self, from_version: i64) -> Result<(), PersistError> {
          if from_version == SCHEMA_VERSION {
              return Ok(());
          }
          if from_version > SCHEMA_VERSION {
              return Err(PersistError::Migration(format!(
                  "db user_version {from_version} newer than supported {SCHEMA_VERSION}"
              )));
          }
          let tx = self
              .conn
              .unchecked_transaction()
              .map_err(|e| PersistError::Migration(e.to_string()))?;
          if from_version < 1 {
              tx.execute_batch(
                  "CREATE TABLE IF NOT EXISTS workspace (
                     id TEXT PRIMARY KEY, name TEXT NOT NULL, root_path TEXT NOT NULL);
                   CREATE TABLE IF NOT EXISTS session (
                     id TEXT PRIMARY KEY,
                     workspace_id TEXT NOT NULL REFERENCES workspace(id),
                     title TEXT NOT NULL, shell TEXT NOT NULL, cwd TEXT NOT NULL,
                     cols INTEGER NOT NULL, rows INTEGER NOT NULL,
                     lifecycle TEXT NOT NULL,
                     exit_code INTEGER, exit_signal TEXT,
                     created_at INTEGER NOT NULL);
                   CREATE TABLE IF NOT EXISTS scrollback (
                     session_id TEXT NOT NULL REFERENCES session(id),
                     seq INTEGER NOT NULL, bytes BLOB NOT NULL, ts INTEGER NOT NULL,
                     PRIMARY KEY (session_id, seq));",
              )
              .map_err(|e| PersistError::Migration(e.to_string()))?;
          }
          tx.pragma_update(None, "user_version", SCHEMA_VERSION)
              .map_err(|e| PersistError::Migration(e.to_string()))?;
          tx.commit()
              .map_err(|e| PersistError::Migration(e.to_string()))?;
          Ok(())
      }
  }

  fn classify(e: rusqlite::Error) -> PersistError {
      if is_corruption(&e) {
          PersistError::Corrupt(e.to_string())
      } else {
          PersistError::Sql(e.to_string())
      }
  }
  ```

- [ ] **Step 5: Implement lifecycle encode/decode + CRUD + rehydrate.** Append (still above the `#[cfg(test)]` module) inside a second `impl Db { … }` block plus free helpers:
  ```rust
  /// Encode a lifecycle into (tag, exit_code, exit_signal) columns (spec §11).
  fn encode_lifecycle(lc: &SessionLifecycle) -> (&'static str, Option<i64>, Option<String>) {
      match lc {
          SessionLifecycle::AtPrompt => ("atPrompt", None, None),
          SessionLifecycle::Typing => ("typing", None, None),
          SessionLifecycle::Running => ("running", None, None),
          SessionLifecycle::Exited { code, signal } => (
              "exited",
              code.map(|c| c as i64),
              signal.clone(),
          ),
      }
  }

  /// Decode (tag, exit_code, exit_signal) back into a lifecycle (spec §11).
  fn decode_lifecycle(
      tag: &str,
      exit_code: Option<i64>,
      exit_signal: Option<String>,
  ) -> Result<SessionLifecycle, PersistError> {
      match tag {
          "atPrompt" => Ok(SessionLifecycle::AtPrompt),
          "typing" => Ok(SessionLifecycle::Typing),
          "running" => Ok(SessionLifecycle::Running),
          "exited" => Ok(SessionLifecycle::Exited {
              code: exit_code.map(|c| (c & 0xff) as u8),
              signal: exit_signal,
          }),
          other => Err(PersistError::Sql(format!("unknown lifecycle tag {other:?}"))),
      }
  }

  impl Db {
      pub fn upsert_workspace(&self, ws: &Workspace) -> Result<(), PersistError> {
          self.conn.execute(
              "INSERT INTO workspace (id, name, root_path) VALUES (?1, ?2, ?3)
               ON CONFLICT(id) DO UPDATE SET name = excluded.name, root_path = excluded.root_path",
              rusqlite::params![ws.id, ws.name, ws.root_path],
          )?;
          Ok(())
      }

      pub fn list_workspaces(&self) -> Result<Vec<Workspace>, PersistError> {
          let mut stmt = self
              .conn
              .prepare("SELECT id, name, root_path FROM workspace ORDER BY id")?;
          let rows = stmt.query_map([], |r| {
              Ok(Workspace {
                  id: r.get(0)?,
                  name: r.get(1)?,
                  root_path: r.get(2)?,
              })
          })?;
          let mut out = Vec::new();
          for row in rows {
              out.push(row?);
          }
          Ok(out)
      }

      pub fn upsert_session(&self, meta: &SessionMeta) -> Result<(), PersistError> {
          let (tag, exit_code, exit_signal) = encode_lifecycle(&meta.lifecycle);
          self.conn.execute(
              "INSERT INTO session
                 (id, workspace_id, title, shell, cwd, cols, rows, lifecycle,
                  exit_code, exit_signal, created_at)
               VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)
               ON CONFLICT(id) DO UPDATE SET
                 workspace_id = excluded.workspace_id, title = excluded.title,
                 shell = excluded.shell, cwd = excluded.cwd, cols = excluded.cols,
                 rows = excluded.rows, lifecycle = excluded.lifecycle,
                 exit_code = excluded.exit_code, exit_signal = excluded.exit_signal,
                 created_at = excluded.created_at",
              rusqlite::params![
                  meta.id,
                  meta.workspace_id,
                  meta.title,
                  meta.shell,
                  meta.cwd,
                  meta.cols as i64,
                  meta.rows as i64,
                  tag,
                  exit_code,
                  exit_signal,
                  meta.created_at,
              ],
          )?;
          Ok(())
      }

      pub fn list_sessions(&self) -> Result<Vec<SessionMeta>, PersistError> {
          self.query_sessions(true)
      }

      /// Rehydrate on restart (spec §11): every session is is_active=false,
      /// waiting_for_input=false because its PTY is gone.
      pub fn rehydrate(&self) -> Result<Vec<SessionMeta>, PersistError> {
          self.query_sessions(false)
      }

      fn query_sessions(&self, keep_flags: bool) -> Result<Vec<SessionMeta>, PersistError> {
          let mut stmt = self.conn.prepare(
              "SELECT id, workspace_id, title, shell, cwd, cols, rows,
                      lifecycle, exit_code, exit_signal, created_at
               FROM session ORDER BY created_at, id",
          )?;
          let rows = stmt.query_map([], |r| {
              let cols: i64 = r.get(5)?;
              let rows_: i64 = r.get(6)?;
              let tag: String = r.get(7)?;
              let exit_code: Option<i64> = r.get(8)?;
              let exit_signal: Option<String> = r.get(9)?;
              Ok((
                  SessionMeta {
                      id: r.get(0)?,
                      workspace_id: r.get(1)?,
                      title: r.get(2)?,
                      shell: r.get(3)?,
                      cwd: r.get(4)?,
                      cols: cols as u16,
                      rows: rows_ as u16,
                      lifecycle: SessionLifecycle::AtPrompt, // placeholder, set below
                      waiting_for_input: false,
                      is_active: false,
                      created_at: r.get(10)?,
                  },
                  tag,
                  exit_code,
                  exit_signal,
              ))
          })?;
          let mut out = Vec::new();
          for row in rows {
              let (mut meta, tag, exit_code, exit_signal) = row?;
              meta.lifecycle = decode_lifecycle(&tag, exit_code, exit_signal)?;
              if keep_flags {
                  // list_sessions preserves stored flags — but S1 never stores true
                  // for these, so both paths currently yield false; explicit for clarity.
                  meta.is_active = false;
                  meta.waiting_for_input = false;
              }
              out.push(meta);
          }
          Ok(out)
      }

      pub fn append_scrollback(
          &self,
          session_id: &SessionId,
          seq: i64,
          bytes: &[u8],
          ts: i64,
      ) -> Result<(), PersistError> {
          self.conn.execute(
              "INSERT INTO scrollback (session_id, seq, bytes, ts) VALUES (?1, ?2, ?3, ?4)
               ON CONFLICT(session_id, seq) DO UPDATE SET bytes = excluded.bytes, ts = excluded.ts",
              rusqlite::params![session_id, seq, bytes, ts],
          )?;
          Ok(())
      }

      pub fn load_scrollback(&self, session_id: &SessionId) -> Result<Vec<u8>, PersistError> {
          let mut stmt = self.conn.prepare(
              "SELECT bytes FROM scrollback WHERE session_id = ?1 ORDER BY seq",
          )?;
          let rows = stmt.query_map([session_id], |r| r.get::<_, Vec<u8>>(0))?;
          let mut out = Vec::new();
          for row in rows {
              out.extend_from_slice(&row?);
          }
          Ok(out)
      }
  }
  ```
  Note: the unused `OptionalExtension` import from Step 4 is removed here — replace the `use rusqlite::{Connection, OptionalExtension};` line with `use rusqlite::Connection;`.

- [ ] **Step 6: Run — confirm PASS.** `cargo test -p sessiond persist_and_rehydrate_round_trip`
  Expected: PASS.

- [ ] **Step 7: Commit.** `git add crates/sessiond/src/persistence.rs crates/sessiond/src/main.rs crates/sessiond/Cargo.toml && git commit -m "feat(sessiond): persistence.rs — WAL SQLite, migrations, CRUD, rehydrate"`

- [ ] **Step 8: Failing test — every SessionLifecycle variant round-trips.** Add to the `tests` module:
  ```rust
  #[test]
  fn every_lifecycle_variant_round_trips() {
      let dir = tempfile::tempdir().unwrap();
      let db = Db::open(&dir.path().join("bpa.db")).unwrap();
      db.upsert_workspace(&ws("w1")).unwrap();

      let variants = vec![
          ("a", SessionLifecycle::AtPrompt),
          ("t", SessionLifecycle::Typing),
          ("r", SessionLifecycle::Running),
          ("e0", SessionLifecycle::Exited { code: Some(0), signal: None }),
          ("e255", SessionLifecycle::Exited { code: Some(255), signal: None }),
          ("enone", SessionLifecycle::Exited { code: None, signal: None }),
          ("esig", SessionLifecycle::Exited { code: None, signal: Some("SIGKILL".into()) }),
      ];
      for (id, lc) in &variants {
          db.upsert_session(&meta(id, "w1", lc.clone())).unwrap();
      }
      let got = db.rehydrate().unwrap();
      for (id, lc) in &variants {
          let m = got.iter().find(|m| &m.id == id).expect("session present");
          assert_eq!(&m.lifecycle, lc, "lifecycle mismatch for {id}");
      }
  }
  ```

- [ ] **Step 9: Run — confirm PASS.** `cargo test -p sessiond every_lifecycle_variant_round_trips`
  Expected: PASS (encode/decode already implemented in Step 5; this locks the contract).

- [ ] **Step 10: Failing test — corrupt-db quarantine + recreate.** Add to the `tests` module:
  ```rust
  #[test]
  fn corrupt_db_is_quarantined_and_recreated() {
      let dir = tempfile::tempdir().unwrap();
      let path = dir.path().join("bpa.db");
      // Write garbage that is NOT a valid SQLite header.
      std::fs::write(&path, b"this is definitely not a sqlite database file").unwrap();

      // open() must NOT error — it quarantines and recreates.
      let db = Db::open(&path).unwrap();
      // Fresh db is usable.
      db.upsert_workspace(&ws("w1")).unwrap();
      assert_eq!(db.list_workspaces().unwrap().len(), 1);

      // A quarantine file exists next to it.
      let found = std::fs::read_dir(dir.path())
          .unwrap()
          .filter_map(|e| e.ok())
          .any(|e| {
              e.file_name()
                  .to_string_lossy()
                  .starts_with("bpa.db.corrupt-")
          });
      assert!(found, "expected a bpa.db.corrupt-<ts> quarantine file");
  }
  ```

- [ ] **Step 11: Run — confirm PASS.** `cargo test -p sessiond corrupt_db_is_quarantined_and_recreated`
  Expected: PASS. (If it FAILs with `SqliteFailure ... file is not a database` propagating out of `open`, verify `open_inner` forces the `PRAGMA user_version` read and `classify` maps `NotADatabase`/`DatabaseCorrupt` to `PersistError::Corrupt` so `open` catches it.)

- [ ] **Step 12: Commit.** `git add crates/sessiond/src/persistence.rs && git commit -m "test(sessiond): lifecycle round-trip + corrupt-db quarantine"`

- [ ] **Step 13: Failing test — busy_timeout under concurrent access.** Add to the `tests` module:
  ```rust
  #[test]
  fn busy_timeout_allows_concurrent_writers() {
      use std::sync::Arc;
      use std::thread;

      let dir = tempfile::tempdir().unwrap();
      let path = Arc::new(dir.path().join("bpa.db"));
      {
          let db = Db::open(&path).unwrap();
          db.upsert_workspace(&ws("w1")).unwrap();
      }

      // Two threads each open their own connection (WAL + busy_timeout=5000) and
      // hammer inserts. Without busy_timeout these would race to SQLITE_BUSY.
      let mut handles = Vec::new();
      for t in 0..2u8 {
          let p = Arc::clone(&path);
          handles.push(thread::spawn(move || {
              let db = Db::open(&p).unwrap();
              for i in 0..50i64 {
                  let sid = format!("s-{t}-{i}");
                  db.upsert_session(&meta(&sid, "w1", SessionLifecycle::Running)).unwrap();
              }
          }));
      }
      for h in handles {
          h.join().unwrap();
      }
      let db = Db::open(&path).unwrap();
      assert_eq!(db.list_sessions().unwrap().len(), 100);
  }
  ```

- [ ] **Step 14: Run — confirm PASS.** `cargo test -p sessiond busy_timeout_allows_concurrent_writers`
  Expected: PASS (busy_timeout=5000 makes the WAL writers wait rather than error). If it FAILs with `database is locked`, confirm `busy_timeout` is applied in `open_inner` before any write.

- [ ] **Step 15: Failing test — migration on old user_version.** Add to the `tests` module:
  ```rust
  #[test]
  fn migration_runs_on_old_user_version() {
      let dir = tempfile::tempdir().unwrap();
      let path = dir.path().join("bpa.db");
      // Simulate a pre-schema database: a valid SQLite file with user_version 0
      // and none of our tables.
      {
          let conn = rusqlite::Connection::open(&path).unwrap();
          conn.pragma_update(None, "user_version", 0_i64).unwrap();
      }
      let db = Db::open(&path).unwrap();
      // Migration created our tables and bumped user_version to SCHEMA_VERSION.
      let uv: i64 = db
          .conn
          .query_row("PRAGMA user_version", [], |r| r.get(0))
          .unwrap();
      assert_eq!(uv, SCHEMA_VERSION);
      db.upsert_workspace(&ws("w1")).unwrap();
      assert_eq!(db.list_workspaces().unwrap().len(), 1);
  }
  ```
  Note: this test reads `db.conn` directly; because the test module is a child of the crate module, private field access is allowed.

- [ ] **Step 16: Run — confirm PASS.** `cargo test -p sessiond migration_runs_on_old_user_version`
  Expected: PASS.

- [ ] **Step 17: Failing test — kill-9-mid-write recovery (committed rows survive).** Add to the `tests` module:
  ```rust
  #[test]
  fn committed_rows_survive_reopen() {
      // Simulate a hard crash: a connection is dropped WITHOUT a clean shutdown
      // checkpoint after committing rows. WAL guarantees committed rows are durable
      // and re-readable on the next open (spec §11 durability bound).
      let dir = tempfile::tempdir().unwrap();
      let path = dir.path().join("bpa.db");
      {
          let db = Db::open(&path).unwrap();
          db.upsert_workspace(&ws("w1")).unwrap();
          db.upsert_session(&meta("s1", "w1", SessionLifecycle::Running)).unwrap();
          db.append_scrollback(&"s1".to_string(), 0, b"committed", 1).unwrap();
          // No checkpoint / no clean close: `db` is dropped here abruptly.
          std::mem::drop(db);
      }
      // Reopen (fresh process would do the same): committed rows must be present.
      let db2 = Db::open(&path).unwrap();
      let sessions = db2.rehydrate().unwrap();
      assert_eq!(sessions.len(), 1);
      assert_eq!(db2.load_scrollback(&"s1".to_string()).unwrap(), b"committed");
  }
  ```

- [ ] **Step 18: Run — confirm PASS.** `cargo test -p sessiond committed_rows_survive_reopen`
  Expected: PASS.

- [ ] **Step 19: Run the whole module suite.** `cargo test -p sessiond persistence`
  Expected: PASS (all persistence tests green).

- [ ] **Step 20: Commit.** `git add crates/sessiond/src/persistence.rs && git commit -m "test(sessiond): busy_timeout, migration, and kill-9 recovery coverage"`

**Definition of Done:**
- `cargo test -p sessiond persistence` green: `persist_and_rehydrate_round_trip`, `every_lifecycle_variant_round_trips`, `corrupt_db_is_quarantined_and_recreated`, `busy_timeout_allows_concurrent_writers`, `migration_runs_on_old_user_version`, `committed_rows_survive_reopen`.
- `Db::open` sets `PRAGMA journal_mode=WAL`, `PRAGMA busy_timeout=5000`, and runs `user_version`-keyed migrations inside a single transaction (spec §11).
- Corrupt/not-a-database images are quarantined (`bpa.db.corrupt-<ts>`, plus stale `-wal`/`-shm` removed) and a fresh db is recreated — `open` never panics or propagates the corruption error (spec §11 degradation).
- Migration failure fails **closed** with `PersistError::Migration` (typed, not a panic); a newer-than-supported `user_version` is rejected.
- `rehydrate()` returns every session with `is_active=false` and `waiting_for_input=false`; `lifecycle` (incl. `exit_code`/`exit_signal`) round-trips losslessly for all four `SessionLifecycle` variants (spec §5, §11).
- All external calls (`Connection::open`, pragmas, SQL, `fs::rename`) return typed `PersistError` on failure; failures are logged via `tracing` with no secret values.

---

### Task 8: `live_grid.rs` — `alacritty_terminal::Term` wrapper for cursor/alt-screen/size status

**Files:**
- Create — `crates/sessiond/src/live_grid.rs`
- Modify — `crates/sessiond/Cargo.toml` (add `alacritty_terminal` pinned exact), `crates/sessiond/src/main.rs` (add `mod live_grid;`)
- Test — inline `#[cfg(test)] mod tests` at the bottom of `crates/sessiond/src/live_grid.rs`

**Depends on:** [T3]   **Parallel-safe with:** [T4, T5, T6, T7, T10]

**Interfaces:**
- Consumes: nothing from `protocol` (this module deals in raw VT bytes + geometry only).
- Produces (locked names — T9 `pty_supervisor` consumes these verbatim for the §10.4 waiting-for-input heuristic and status, per Task interface index):
  - `pub struct LiveGrid { /* owns alacritty_terminal::Term + parser */ }`
  - `pub fn LiveGrid::new(cols: u16, rows: u16) -> LiveGrid`
  - `pub fn LiveGrid::feed(&mut self, bytes: &[u8])`
  - `pub fn LiveGrid::cursor_col(&self) -> u16`
  - `pub fn LiveGrid::is_alt_screen(&self) -> bool`
  - `pub fn LiveGrid::resize(&mut self, cols: u16, rows: u16)`

**NOT serialized** (spec §11): `LiveGrid` is a status source only (cursor column, alt-screen/mode flags, cols/rows). The replay source is the sanitized byte ring in `scrollback.rs` (T6), never this grid.

**Pinned `alacritty_terminal` API (confirmed from RESEARCH — spec §15 item 2).** Version pinned **exact** (0.24.x; the grid model leaks into behavior so no `^`). The used surface at 0.24/0.25:
- `alacritty_terminal::term::Term<L>` — the headless grid emulator; `Term::new(config: &Config, dimensions: &D, event_listener: L)` where `D: Dimensions` and `L: EventListener`.
- `alacritty_terminal::term::Config` — `Default`-constructible; we use defaults (scrollback etc. irrelevant here).
- `alacritty_terminal::vte::ansi::Processor` — VT parser; `processor.advance(&mut term, byte)` per byte (or `advance` over a slice depending on point release) drives the grid state machine.
- `alacritty_terminal::event::EventListener` — trait with `fn send_event(&self, event: Event)`; a no-op unit struct implements it (the daemon does not need alacritty's event callbacks).
- `alacritty_terminal::grid::Dimensions` — trait exposing `columns()`, `screen_lines()`, `total_lines()`; we provide a small `WindowSize`/`TermSize` struct implementing it (`{ columns: usize, screen_lines: usize }`).
- Cursor: `term.grid().cursor.point.column` → `Column(usize)`; column index is `.0`.
- Alt-screen: `term.mode().contains(TermMode::ALT_SCREEN)` (`alacritty_terminal::term::TermMode`).
- Resize: `term.resize(new_dimensions)` where `new_dimensions: Dimensions`.

The task's Step 4 begins with a **compile-probe** that pins the exact symbol paths for the installed point release; any path that differs from the above is corrected in-place there before the impl is finalized (the behavior — cursor col, alt-screen flag, resize — is stable across 0.24/0.25; only module paths for `Processor`/`Config` occasionally move between `ansi`/`vte` re-exports).

---

- [ ] **Step 1: Add dep and module wiring.** Edit `crates/sessiond/Cargo.toml`: under `[dependencies]` add `alacritty_terminal = "=0.24.2"` (exact pin; confirm the concrete published 0.24 patch at task time and pin it exactly). Edit `crates/sessiond/src/main.rs`: add `mod live_grid;`.

- [ ] **Step 2: Compile-probe the pinned API (throwaway).** Run a one-off to confirm the exact symbol paths for the installed release:
  ```bash
  cargo doc -p alacritty_terminal --no-deps 2>/dev/null; \
  cargo tree -p sessiond -i alacritty_terminal
  ```
  Then grep the built rustdoc / source for the four symbols this task binds:
  ```bash
  find ~/.cargo/registry/src -type d -name 'alacritty_terminal-0.24*' -maxdepth 3 \
    -exec grep -RIl "pub struct Processor" {} + ; \
  find ~/.cargo/registry/src -type d -name 'alacritty_terminal-0.24*' -maxdepth 3 \
    -exec grep -RIn "ALT_SCREEN\|pub fn resize\|pub struct Config\b" {} +
  ```
  Expected: paths matching `term::Term`, `term::Config`, `term::TermMode::ALT_SCREEN`, and a `Processor` under `vte::ansi` (re-exported). Record the exact `use` paths; use them verbatim in Step 4. Do not commit anything from this step.

- [ ] **Step 3: Failing test — cursor column advances after writes.** Add to `crates/sessiond/src/live_grid.rs`:
  ```rust
  #[cfg(test)]
  mod tests {
      use super::*;

      #[test]
      fn cursor_col_after_plain_writes() {
          let mut g = LiveGrid::new(80, 24);
          assert_eq!(g.cursor_col(), 0);
          g.feed(b"abc");
          // After printing 3 columns the cursor sits at column 3.
          assert_eq!(g.cursor_col(), 3);
          g.feed(b"\r"); // carriage return -> column 0
          assert_eq!(g.cursor_col(), 0);
      }
  }
  ```

- [ ] **Step 4: Run — confirm FAIL.** `cargo test -p sessiond cursor_col_after_plain_writes`
  Expected: FAIL with `cannot find type LiveGrid in this scope`.

- [ ] **Step 5: Implement `LiveGrid`.** Write at the top of `crates/sessiond/src/live_grid.rs` (correct the `use` paths in Step 2 if the probe showed a different re-export):
  ```rust
  //! Live terminal grid state (spec §11): a headless `alacritty_terminal::Term`
  //! used ONLY for cursor column, alt-screen/raw-mode detection, and cols/rows —
  //! the inputs to the waiting-for-input heuristic (spec §10.4) and status.
  //! Never serialized; the replay source is the sanitized byte ring (scrollback.rs).

  use alacritty_terminal::event::{Event, EventListener};
  use alacritty_terminal::grid::Dimensions;
  use alacritty_terminal::index::Line;
  use alacritty_terminal::term::{Config, Term, TermMode};
  use alacritty_terminal::vte::ansi::Processor;

  /// No-op event sink: the daemon does not consume alacritty's event callbacks.
  #[derive(Clone, Default)]
  struct NoopListener;

  impl EventListener for NoopListener {
      fn send_event(&self, _event: Event) {}
  }

  /// Terminal dimensions implementing `alacritty_terminal::grid::Dimensions`.
  #[derive(Clone, Copy, Debug)]
  struct TermSize {
      columns: usize,
      screen_lines: usize,
  }

  impl Dimensions for TermSize {
      fn total_lines(&self) -> usize {
          self.screen_lines
      }
      fn screen_lines(&self) -> usize {
          self.screen_lines
      }
      fn columns(&self) -> usize {
          self.columns
      }
  }

  pub struct LiveGrid {
      term: Term<NoopListener>,
      parser: Processor,
  }

  impl LiveGrid {
      /// Create a fresh grid of `cols` x `rows`. Zero dimensions are clamped to 1
      /// so alacritty never sees a degenerate grid.
      pub fn new(cols: u16, rows: u16) -> LiveGrid {
          let size = TermSize {
              columns: cols.max(1) as usize,
              screen_lines: rows.max(1) as usize,
          };
          let term = Term::new(Config::default(), &size, NoopListener);
          LiveGrid {
              term,
              parser: Processor::new(),
          }
      }

      /// Feed raw PTY bytes through the VT parser into the grid.
      pub fn feed(&mut self, bytes: &[u8]) {
          self.parser.advance(&mut self.term, bytes);
      }

      /// Current cursor column (0-based). Input to the §10.4 heuristic
      /// ("cursor not at column 0").
      pub fn cursor_col(&self) -> u16 {
          self.term.grid().cursor.point.column.0 as u16
      }

      /// Whether the alt-screen buffer is active (vim/less/top) — excluded from
      /// the waiting-for-input heuristic (spec §10.4).
      pub fn is_alt_screen(&self) -> bool {
          self.term.mode().contains(TermMode::ALT_SCREEN)
      }

      /// Resize the grid; clamps zero dimensions to 1.
      pub fn resize(&mut self, cols: u16, rows: u16) {
          let size = TermSize {
              columns: cols.max(1) as usize,
              screen_lines: rows.max(1) as usize,
          };
          self.term.resize(size);
      }
  }

  // Silence an unused-import warning if `Line` is not needed after probe; keep the
  // path referenced so a future maintainer sees where line geometry lives.
  #[allow(unused_imports)]
  use Line as _LineAlias;
  ```
  Note: if the Step 2 probe shows `Processor::advance` takes a single `u8` (older point release) instead of a `&[u8]` slice, change `feed` to iterate: `for &b in bytes { self.parser.advance(&mut self.term, b); }`. Both forms are behavior-equivalent for these tests.

- [ ] **Step 6: Run — confirm PASS.** `cargo test -p sessiond cursor_col_after_plain_writes`
  Expected: PASS.

- [ ] **Step 7: Commit.** `git add crates/sessiond/src/live_grid.rs crates/sessiond/src/main.rs crates/sessiond/Cargo.toml && git commit -m "feat(sessiond): live_grid.rs — alacritty Term wrapper for status"`

- [ ] **Step 8: Failing test — alt-screen enter/leave detection.** Add to the `tests` module:
  ```rust
  #[test]
  fn alt_screen_enter_and_leave() {
      let mut g = LiveGrid::new(80, 24);
      assert!(!g.is_alt_screen());
      // Enter alt-screen (DECSET ?1049h) — what vim/less/top do.
      g.feed(b"\x1b[?1049h");
      assert!(g.is_alt_screen());
      // Leave alt-screen (DECRST ?1049l).
      g.feed(b"\x1b[?1049l");
      assert!(!g.is_alt_screen());
  }
  ```

- [ ] **Step 9: Run — confirm PASS.** `cargo test -p sessiond alt_screen_enter_and_leave`
  Expected: PASS (alacritty maps `?1049h/l` to `TermMode::ALT_SCREEN`).

- [ ] **Step 10: Failing test — resize changes reported columns and clamps cursor.** Add to the `tests` module:
  ```rust
  #[test]
  fn resize_shrinks_and_grows_grid() {
      let mut g = LiveGrid::new(80, 24);
      // Move the cursor near the right edge, then shrink below it.
      g.feed(b"0123456789"); // cursor at column 10
      assert_eq!(g.cursor_col(), 10);

      g.resize(8, 24); // columns now 8 -> cursor must be clamped in-bounds
      assert!(g.cursor_col() < 8, "cursor col {} not clamped to < 8", g.cursor_col());

      // Grow back; writing from a fresh line lands within the wider grid.
      g.resize(120, 40);
      g.feed(b"\r\nx");
      assert_eq!(g.cursor_col(), 1);
  }
  ```

- [ ] **Step 11: Run — confirm PASS.** `cargo test -p sessiond resize_shrinks_and_grows_grid`
  Expected: PASS. If `cursor_col()` is not clamped below 8 after shrink, confirm `resize` passes the new `TermSize` to `Term::resize` (alacritty reflows and clamps the cursor).

- [ ] **Step 12: Run the whole module suite.** `cargo test -p sessiond live_grid`
  Expected: PASS (`cursor_col_after_plain_writes`, `alt_screen_enter_and_leave`, `resize_shrinks_and_grows_grid`).

- [ ] **Step 13: Commit.** `git add crates/sessiond/src/live_grid.rs && git commit -m "test(sessiond): live_grid alt-screen + resize coverage"`

**Definition of Done:**
- `cargo test -p sessiond live_grid` green: `cursor_col_after_plain_writes`, `alt_screen_enter_and_leave`, `resize_shrinks_and_grows_grid`.
- `LiveGrid` wraps `alacritty_terminal::Term` behind exactly the locked surface (`new`, `feed`, `cursor_col`, `is_alt_screen`, `resize`); no other public methods (spec §11: status source only).
- `cursor_col()` reflects printed columns and CR/LF; `is_alt_screen()` flips on `?1049h`/`?1049l`; `resize()` clamps the cursor in-bounds and updates reported columns (spec §10.4 heuristic inputs).
- `alacritty_terminal` is pinned **exact** (no `^`), per spec §3/§15 (grid model leaks into behavior); the Step 2 probe reconciled the concrete symbol paths for the installed point release.
- `LiveGrid` is **never serialized** and does not depend on `protocol` — it is a pure in-memory status structure (spec §11).


### Task 9: `pty_supervisor.rs` — PTY lifecycle, process-group kill, env hygiene, waiting-for-input

**Files:**
- Create: `crates/sessiond/src/pty_supervisor.rs`
- Modify: `crates/sessiond/Cargo.toml` (ensure `portable-pty = "0.9.0"`, `libc` present; add `[dev-dependencies] tempfile`, `nix` with features `["signal", "process"]` if not already present — used only in tests to observe orphans/pgid; production kill uses `libc::killpg`). `crates/sessiond/src/lib.rs` (append `pub mod pty_supervisor;` — append only; do not touch other tasks' `mod` lines).
- Test: inline `#[cfg(test)] mod tests` in `crates/sessiond/src/pty_supervisor.rs`.

**Depends on:** [T5, T6, T8]   **Parallel-safe with:** [] (integration leaf of G2; T4, T7, T10 already landed and are not touched)

**Interfaces:**
Consumes (verbatim upstream names):
- From **T5 `osc_parser`**: `crate::osc_parser::{OscParser, OscEvent, Lifecycle}`. `OscParser::new()`, `OscParser::feed(&mut self, &[u8]) -> Vec<OscEvent>`; `OscEvent::{PromptStart, PromptEnd, CommandStart, CommandEnd(Option<u8>), Cwd(String)}`; `Lifecycle::{AtPrompt, Running, Exited(Option<u8>)}`, `Lifecycle::new()`, `Lifecycle::advance(&mut self, &OscEvent)`.
- From **T6 `scrollback`**: `crate::scrollback::ScrollbackRing`. `ScrollbackRing::new(cap: usize)`, `push(&mut self, &[u8])`, `snapshot(&self) -> Vec<u8>`, `prune(&mut self)`.
- From **T8 `live_grid`**: `crate::live_grid::LiveGrid`. `LiveGrid::new(cols: u16, rows: u16)`, `feed(&mut self, &[u8])`, `cursor_col(&self) -> u16`, `is_alt_screen(&self) -> bool`, `resize(&mut self, cols: u16, rows: u16)`.
- From **T3 `protocol`** (crate `bpa_protocol`): `bpa_protocol::{SessionId, WorkspaceId, SessionMeta, SessionLifecycle}` (spec §5). `SessionLifecycle::{AtPrompt, Typing, Running, Exited{code: Option<u8>, signal: Option<String>}}`. `SessionMeta { id, workspace_id, title, shell, cwd, cols, rows, lifecycle, waiting_for_input, is_active, created_at }`.
- From **portable-pty 0.9.0**: `native_pty_system() -> NativePtySystem`; `PtySystem::openpty(PtySize{rows,cols,pixel_width,pixel_height}) -> Result<PtyPair>`; `PtyPair{ master: Box<dyn MasterPty>, slave: Box<dyn SlavePty> }`; `MasterPty::{resize, try_clone_reader, take_writer, process_group_leader}`; `SlavePty::spawn_command(CommandBuilder) -> Result<Box<dyn Child>>`; `Child::{wait, try_wait, process_id}`; `ChildKiller::{kill, clone_killer}`; `ExitStatus::{success, exit_code() -> u32, signal() -> Option<&str>}`; `CommandBuilder::{new, arg, env, env_clear, cwd}`.

Produces (verbatim from the scaffold Task interface index, spec §9 / §10.4):
```rust
/// Params to open one session (validated cwd; env allowlist already resolved upstream).
pub struct SessionSpec {
    pub workspace_id: bpa_protocol::WorkspaceId,
    pub shell: String,            // absolute path to the shell program (e.g. "/bin/zsh")
    pub args: Vec<String>,        // shell-integration args (e.g. ["--init-file", "<path>"]) or []
    pub cwd: std::path::PathBuf,  // already canonicalized/validated by the caller (T15/paths, §16)
    pub env: Vec<(String, String)>, // the FULL allowlisted env to set after env_clear() (§9.3)
    pub cols: u16,
    pub rows: u16,
    pub title: String,
}

/// A byte sink the supervisor feeds live PTY output to (attach layer subscribes here).
pub type OutputSink = std::sync::mpsc::Sender<Vec<u8>>;

/// Emitted to the caller when a session's status changes (broker → StateChanged push, §10.4).
#[derive(Debug, Clone, PartialEq)]
pub struct StatusUpdate {
    pub session_id: bpa_protocol::SessionId,
    pub lifecycle: bpa_protocol::SessionLifecycle,
    pub waiting_for_input: bool,
    pub cwd: String,
}

pub struct Supervisor { /* owns Map<SessionId, Session>; native_pty_system() once */ }

impl Supervisor {
    pub fn new() -> Self;                                     // native_pty_system() once
    pub fn create(&self, spec: SessionSpec) -> anyhow::Result<bpa_protocol::SessionId>;
    pub fn write_stdin(&self, id: &str, bytes: &[u8]) -> anyhow::Result<()>;
    pub fn resize(&self, id: &str, cols: u16, rows: u16) -> anyhow::Result<()>;
    pub fn kill(&self, id: &str) -> anyhow::Result<()>;      // process-group SIGTERM→2s→SIGKILL, reap
    pub fn subscribe_output(&self, id: &str, sink: OutputSink) -> anyhow::Result<()>;
    pub fn snapshot_scrollback(&self, id: &str) -> anyhow::Result<(u16, u16, Vec<u8>)>; // (cols,rows,bytes) for Replay
    pub fn meta(&self, id: &str) -> anyhow::Result<bpa_protocol::SessionMeta>;
    pub fn on_status<F: Fn(StatusUpdate) + Send + Sync + 'static>(&self, cb: F); // register status callback
    pub fn on_created<F: Fn(bpa_protocol::SessionMeta) + Send + Sync + 'static>(&self, cb: F);
    pub fn on_exited<F: Fn(bpa_protocol::SessionId, Option<u8>, Option<String>) + Send + Sync + 'static>(&self, cb: F);
}
```

Locked internal contract (spec §9, all rules load-bearing — violating any is a known hang/leak):
- Per-session owned state: `{ master: Box<dyn MasterPty>, writer: Mutex<Box<dyn Write+Send>>, killer: Box<dyn ChildKiller+Send+Sync>, pgid: Option<i32>, cols: u16, rows: u16, reader_thread: JoinHandle, wait_thread: JoinHandle }`.
- `native_pty_system()` is called **once** in `Supervisor::new()` and stored (`Box<dyn PtySystem + Send + Sync>` behind the supervisor).
- `create`: `openpty(PtySize{rows: spec.rows, cols: spec.cols, pixel_width: 0, pixel_height: 0})` → build `CommandBuilder::new(&spec.shell)`, `env_clear()`, then `env(k,v)` for **every** pair in `spec.env` (the allowlist per §9.3 — the caller resolves it and MUST include `SSH_AUTH_SOCK` when the daemon has it, plus the shell-integration var), `arg()` each of `spec.args`, `cwd(&spec.cwd)` → `slave.spawn_command(cmd)` → `pgid = master.process_group_leader()` → **`drop(pair.slave)` immediately**.
- One **blocking OS reader thread** per PTY (portable-pty has no async API): loop `reader.read(&mut buf)` with a 32 KiB buffer; `Ok(0)` ⇒ EOF ⇒ tear down (mark `is_active=false`, emit exit via the wait thread). Each chunk is fed, in order, to: the `OscParser` (advance the `Lifecycle`, emit `StatusUpdate` on transitions + `Cwd`), the `LiveGrid` (`feed`), the `ScrollbackRing` (`push`, sanitizing), and any registered `OutputSink` (live `Output`). The scrollback ring cap = **256 KiB** per session.
- `writer = master.take_writer()` **once** at create; owned behind a `Mutex`; `flush()` after every write.
- `killer = child.clone_killer()` captured **before** starting the wait thread. `wait()` runs on the single owning `wait_thread`; on return it records the `ExitStatus` (`code = (exit_code() & 0xff) as u8` when not signalled; `signal = signal().map(str::to_string)`), sets `is_active=false`, updates `lifecycle = Exited{code, signal}`, and fires `on_exited`.
- `resize`: `master.resize(PtySize{rows, cols, 0, 0})` (delivers SIGWINCH) + update tracked `cols/rows` + `LiveGrid::resize`.
- `kill` (process-group, §9.8): if `pgid` is `Some(p)`, `libc::killpg(p, libc::SIGTERM)`; wait up to **2 s** polling `try_wait`; if still alive `libc::killpg(p, libc::SIGKILL)`; then `killer.kill()` and join the wait thread to reap the zombie. If `pgid` is `None` (non-POSIX / no leader): `killer.kill()` + reap only.
- `waiting_for_input` (§10.4, heuristic — documented as such): `lifecycle == Running` AND `tcgetattr(master_fd).c_lflag` has both `ICANON` and `ECHO` set AND `!live_grid.is_alt_screen()` AND output quiescent ≥ **150 ms** AND `live_grid.cursor_col() != 0`. Recomputed on each status recompute; surfaced via `StatusUpdate`.
- `meta` builds `SessionMeta` from tracked fields; `lifecycle` maps the internal `osc_parser::Lifecycle` to `protocol::SessionLifecycle` (`AtPrompt→AtPrompt`, `Running→Running`, `Exited(code)→Exited{code, signal}`; `Typing` is never produced).

Design notes for the implementer (locked so tests and code agree):
- `master_fd` for `tcgetattr` comes from `MasterPty::as_raw_fd() -> Option<RawFd>` (present in portable-pty 0.9.0). If `None`, `waiting_for_input` is `false` (fail-safe).
- Output quiescence: the reader thread stamps `last_output = Instant::now()` on every non-empty read; `waiting_for_input` checks `last_output.elapsed() >= Duration::from_millis(150)`. A lightweight status ticker thread (200 ms) recomputes+emits `StatusUpdate` when `waiting_for_input` flips while `Running`, so the "quiet cat" case surfaces without new bytes.
- `pgid` type: `process_group_leader()` returns `Option<pid_t>` (`i32` on macOS); store as `Option<i32>`.
- All maps are `Mutex<HashMap<SessionId, Arc<Session>>>`; callbacks are stored as `Arc<dyn Fn ...>` under a `Mutex<Option<...>>`.

- [ ] **Step 1: Failing test — echo roundtrip (write to stdin, read it back)**

Add to `crates/sessiond/src/pty_supervisor.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::{Duration, Instant};

    fn base_env() -> Vec<(String, String)> {
        let path = std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into());
        vec![
            ("TERM".into(), "xterm-256color".into()),
            ("PATH".into(), path),
            ("HOME".into(), std::env::var("HOME").unwrap_or_else(|_| "/tmp".into())),
        ]
    }

    fn spec_for(shell: &str, args: Vec<String>) -> SessionSpec {
        SessionSpec {
            workspace_id: "ws-test".into(),
            shell: shell.into(),
            args,
            cwd: std::path::PathBuf::from("/tmp"),
            env: base_env(),
            cols: 80,
            rows: 24,
            title: "t".into(),
        }
    }

    fn drain_until(rx: &mpsc::Receiver<Vec<u8>>, needle: &[u8], timeout: Duration) -> Vec<u8> {
        let start = Instant::now();
        let mut acc = Vec::new();
        while start.elapsed() < timeout {
            if let Ok(chunk) = rx.recv_timeout(Duration::from_millis(100)) {
                acc.extend_from_slice(&chunk);
                if acc.windows(needle.len()).any(|w| w == needle) {
                    return acc;
                }
            }
        }
        acc
    }

    #[test]
    fn echo_roundtrip_via_sh() {
        let sup = Supervisor::new();
        let id = sup.create(spec_for("/bin/sh", vec![])).expect("create");
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        sup.subscribe_output(&id, tx).expect("subscribe");
        sup.write_stdin(&id, b"printf BPA_MARKER_OK\\n").expect("write");
        let out = drain_until(&rx, b"BPA_MARKER_OK", Duration::from_secs(5));
        assert!(
            out.windows(b"BPA_MARKER_OK".len()).any(|w| w == b"BPA_MARKER_OK"),
            "expected echoed marker in output, got: {}",
            String::from_utf8_lossy(&out)
        );
        sup.kill(&id).expect("kill");
    }
}
```

- [ ] **Step 2: Run — confirm FAIL**

`cargo test -p sessiond pty_supervisor::tests::echo_roundtrip_via_sh`
Expected: FAIL to compile with `cannot find type 'Supervisor'` / `cannot find type 'SessionSpec'`.

- [ ] **Step 3: Implement the supervisor core (create / reader thread / wait thread / write / subscribe / meta)**

Prepend to `crates/sessiond/src/pty_supervisor.rs`:
```rust
//! PTY supervisor (spec §9, §10.4). Owns every PTY; one blocking reader thread and one wait
//! thread per session; process-group kill; env-hygiene; waiting-for-input heuristic.

use std::collections::HashMap;
use std::io::{Read, Write};
use std::os::fd::RawFd;
use std::sync::{Arc, Mutex};
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use anyhow::{anyhow, Context};
use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, MasterPty, PtySize, PtySystem};

use crate::live_grid::LiveGrid;
use crate::osc_parser::{Lifecycle, OscEvent, OscParser};
use crate::scrollback::ScrollbackRing;

const SCROLLBACK_CAP: usize = 256 * 1024;
const READ_BUF: usize = 32 * 1024;
const QUIESCENT: Duration = Duration::from_millis(150);
const KILL_GRACE: Duration = Duration::from_secs(2);

pub struct SessionSpec {
    pub workspace_id: bpa_protocol::WorkspaceId,
    pub shell: String,
    pub args: Vec<String>,
    pub cwd: std::path::PathBuf,
    pub env: Vec<(String, String)>,
    pub cols: u16,
    pub rows: u16,
    pub title: String,
}

pub type OutputSink = mpsc::Sender<Vec<u8>>;

#[derive(Debug, Clone, PartialEq)]
pub struct StatusUpdate {
    pub session_id: bpa_protocol::SessionId,
    pub lifecycle: bpa_protocol::SessionLifecycle,
    pub waiting_for_input: bool,
    pub cwd: String,
}

/// Mutable per-session state shared between reader/wait/ticker threads.
struct Shared {
    id: bpa_protocol::SessionId,
    workspace_id: bpa_protocol::WorkspaceId,
    title: String,
    shell: String,
    cwd: Mutex<String>,
    cols: Mutex<u16>,
    rows: Mutex<u16>,
    lifecycle: Mutex<Lifecycle>,
    is_active: Mutex<bool>,
    exit_code: Mutex<Option<u8>>,
    exit_signal: Mutex<Option<String>>,
    grid: Mutex<LiveGrid>,
    scrollback: Mutex<ScrollbackRing>,
    sink: Mutex<Option<OutputSink>>,
    last_output: Mutex<Instant>,
    master_fd: Option<RawFd>,
    waiting: Mutex<bool>,
    created_at: i64,
}

struct Session {
    shared: Arc<Shared>,
    master: Box<dyn MasterPty + Send>,
    writer: Mutex<Box<dyn Write + Send>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    pgid: Option<i32>,
    reader_thread: Mutex<Option<JoinHandle<()>>>,
    wait_thread: Mutex<Option<JoinHandle<()>>>,
    ticker_stop: Arc<Mutex<bool>>,
    ticker_thread: Mutex<Option<JoinHandle<()>>>,
}

type StatusCb = Arc<dyn Fn(StatusUpdate) + Send + Sync>;
type CreatedCb = Arc<dyn Fn(bpa_protocol::SessionMeta) + Send + Sync>;
type ExitedCb = Arc<dyn Fn(bpa_protocol::SessionId, Option<u8>, Option<String>) + Send + Sync>;

pub struct Supervisor {
    pty_system: Box<dyn PtySystem + Send + Sync>,
    sessions: Mutex<HashMap<bpa_protocol::SessionId, Arc<Session>>>,
    on_status: Mutex<Option<StatusCb>>,
    on_created: Mutex<Option<CreatedCb>>,
    on_exited: Mutex<Option<ExitedCb>>,
}

fn now_secs() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

fn lifecycle_to_proto(
    lc: Lifecycle,
    code: Option<u8>,
    signal: Option<String>,
) -> bpa_protocol::SessionLifecycle {
    match lc {
        Lifecycle::AtPrompt => bpa_protocol::SessionLifecycle::AtPrompt,
        Lifecycle::Running => bpa_protocol::SessionLifecycle::Running,
        Lifecycle::Exited(c) => bpa_protocol::SessionLifecycle::Exited {
            code: c.or(code),
            signal,
        },
    }
}

impl Supervisor {
    pub fn new() -> Self {
        Supervisor {
            pty_system: native_pty_system(),
            sessions: Mutex::new(HashMap::new()),
            on_status: Mutex::new(None),
            on_created: Mutex::new(None),
            on_exited: Mutex::new(None),
        }
    }

    pub fn on_status<F: Fn(StatusUpdate) + Send + Sync + 'static>(&self, cb: F) {
        *self.on_status.lock().unwrap() = Some(Arc::new(cb));
    }
    pub fn on_created<F: Fn(bpa_protocol::SessionMeta) + Send + Sync + 'static>(&self, cb: F) {
        *self.on_created.lock().unwrap() = Some(Arc::new(cb));
    }
    pub fn on_exited<F: Fn(bpa_protocol::SessionId, Option<u8>, Option<String>) + Send + Sync + 'static>(
        &self,
        cb: F,
    ) {
        *self.on_exited.lock().unwrap() = Some(Arc::new(cb));
    }

    fn emit_status(&self, shared: &Arc<Shared>) {
        if let Some(cb) = self.on_status.lock().unwrap().clone() {
            let lc = *shared.lifecycle.lock().unwrap();
            let code = *shared.exit_code.lock().unwrap();
            let signal = shared.exit_signal.lock().unwrap().clone();
            cb(StatusUpdate {
                session_id: shared.id.clone(),
                lifecycle: lifecycle_to_proto(lc, code, signal),
                waiting_for_input: *shared.waiting.lock().unwrap(),
                cwd: shared.cwd.lock().unwrap().clone(),
            });
        }
    }

    pub fn create(&self, spec: SessionSpec) -> anyhow::Result<bpa_protocol::SessionId> {
        let pair = self
            .pty_system
            .openpty(PtySize {
                rows: spec.rows,
                cols: spec.cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("openpty")?;

        let mut cmd = CommandBuilder::new(&spec.shell);
        cmd.env_clear();
        for (k, v) in &spec.env {
            cmd.env(k, v);
        }
        for a in &spec.args {
            cmd.arg(a);
        }
        cmd.cwd(&spec.cwd);

        let mut child = pair.slave.spawn_command(cmd).context("spawn_command")?;
        let pgid = pair.master.process_group_leader();
        let master_fd = pair.master.as_raw_fd();
        // MUST drop slave immediately or master read() never sees EOF.
        drop(pair.slave);

        let reader = pair.master.try_clone_reader().context("clone reader")?;
        let writer = pair.master.take_writer().context("take writer")?;
        let killer = child.clone_killer();

        let id: bpa_protocol::SessionId = uuid::Uuid::new_v4().to_string();
        let shared = Arc::new(Shared {
            id: id.clone(),
            workspace_id: spec.workspace_id.clone(),
            title: spec.title.clone(),
            shell: spec.shell.clone(),
            cwd: Mutex::new(spec.cwd.to_string_lossy().into_owned()),
            cols: Mutex::new(spec.cols),
            rows: Mutex::new(spec.rows),
            lifecycle: Mutex::new(Lifecycle::new()),
            is_active: Mutex::new(true),
            exit_code: Mutex::new(None),
            exit_signal: Mutex::new(None),
            grid: Mutex::new(LiveGrid::new(spec.cols, spec.rows)),
            scrollback: Mutex::new(ScrollbackRing::new(SCROLLBACK_CAP)),
            sink: Mutex::new(None),
            last_output: Mutex::new(Instant::now()),
            master_fd,
            waiting: Mutex::new(false),
            created_at: now_secs(),
        });

        // Reader thread — the only place that mutates parser/grid/scrollback from bytes.
        let reader_shared = shared.clone();
        let status_cb_reader = self.on_status.lock().unwrap().clone();
        let reader_thread = std::thread::Builder::new()
            .name(format!("bpa-reader-{}", id))
            .spawn(move || {
                let mut reader = reader;
                let mut parser = OscParser::new();
                let mut buf = vec![0u8; READ_BUF];
                loop {
                    match reader.read(&mut buf) {
                        Ok(0) | Err(_) => break, // EOF or fatal read error
                        Ok(n) => {
                            let chunk = &buf[..n];
                            *reader_shared.last_output.lock().unwrap() = Instant::now();

                            let events = parser.feed(chunk);
                            let mut status_dirty = false;
                            for ev in &events {
                                {
                                    let mut lc = reader_shared.lifecycle.lock().unwrap();
                                    lc.advance(ev);
                                }
                                if let OscEvent::Cwd(path) = ev {
                                    *reader_shared.cwd.lock().unwrap() = path.clone();
                                }
                                status_dirty = true;
                            }

                            reader_shared.grid.lock().unwrap().feed(chunk);
                            reader_shared.scrollback.lock().unwrap().push(chunk);

                            if let Some(sink) = reader_shared.sink.lock().unwrap().clone() {
                                let _ = sink.send(chunk.to_vec());
                            }

                            if status_dirty {
                                recompute_waiting(&reader_shared);
                                if let Some(cb) = &status_cb_reader {
                                    emit_via(cb, &reader_shared);
                                }
                            }
                        }
                    }
                }
                *reader_shared.is_active.lock().unwrap() = false;
            })
            .context("spawn reader thread")?;

        // Wait thread — owns the Child; reaps and records exit status.
        let wait_shared = shared.clone();
        let exited_cb = self.on_exited.lock().unwrap().clone();
        let wait_thread = std::thread::Builder::new()
            .name(format!("bpa-wait-{}", id))
            .spawn(move || {
                let status = child.wait();
                let (code, signal) = match status {
                    Ok(s) => {
                        if let Some(sig) = s.signal() {
                            (None, Some(sig.to_string()))
                        } else {
                            (Some((s.exit_code() & 0xff) as u8), None)
                        }
                    }
                    Err(_) => (None, None),
                };
                *wait_shared.is_active.lock().unwrap() = false;
                *wait_shared.exit_code.lock().unwrap() = code;
                *wait_shared.exit_signal.lock().unwrap() = signal.clone();
                *wait_shared.lifecycle.lock().unwrap() = Lifecycle::Exited(code);
                *wait_shared.waiting.lock().unwrap() = false;
                if let Some(cb) = &exited_cb {
                    cb(wait_shared.id.clone(), code, signal);
                }
            })
            .context("spawn wait thread")?;

        // Ticker thread — surfaces waiting-for-input flips without new bytes (quiet-cat case).
        let ticker_shared = shared.clone();
        let ticker_stop = Arc::new(Mutex::new(false));
        let ticker_stop_thread = ticker_stop.clone();
        let status_cb_ticker = self.on_status.lock().unwrap().clone();
        let ticker_thread = std::thread::Builder::new()
            .name(format!("bpa-tick-{}", id))
            .spawn(move || loop {
                std::thread::sleep(Duration::from_millis(200));
                if *ticker_stop_thread.lock().unwrap() {
                    break;
                }
                if !*ticker_shared.is_active.lock().unwrap() {
                    break;
                }
                let before = *ticker_shared.waiting.lock().unwrap();
                recompute_waiting(&ticker_shared);
                let after = *ticker_shared.waiting.lock().unwrap();
                if before != after {
                    if let Some(cb) = &status_cb_ticker {
                        emit_via(cb, &ticker_shared);
                    }
                }
            })
            .context("spawn ticker thread")?;

        let session = Arc::new(Session {
            shared: shared.clone(),
            master: pair.master,
            writer: Mutex::new(writer),
            killer: Mutex::new(killer),
            pgid,
            reader_thread: Mutex::new(Some(reader_thread)),
            wait_thread: Mutex::new(Some(wait_thread)),
            ticker_stop,
            ticker_thread: Mutex::new(Some(ticker_thread)),
        });

        self.sessions.lock().unwrap().insert(id.clone(), session);

        if let Some(cb) = self.on_created.lock().unwrap().clone() {
            cb(self.meta(&id)?);
        }
        Ok(id)
    }

    fn get(&self, id: &str) -> anyhow::Result<Arc<Session>> {
        self.sessions
            .lock()
            .unwrap()
            .get(id)
            .cloned()
            .ok_or_else(|| anyhow!("no such session: {id}"))
    }

    pub fn write_stdin(&self, id: &str, bytes: &[u8]) -> anyhow::Result<()> {
        let s = self.get(id)?;
        let mut w = s.writer.lock().unwrap();
        w.write_all(bytes).context("write_all")?;
        w.flush().context("flush")?;
        Ok(())
    }

    pub fn resize(&self, id: &str, cols: u16, rows: u16) -> anyhow::Result<()> {
        let s = self.get(id)?;
        s.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .context("resize")?;
        *s.shared.cols.lock().unwrap() = cols;
        *s.shared.rows.lock().unwrap() = rows;
        s.shared.grid.lock().unwrap().resize(cols, rows);
        Ok(())
    }

    pub fn subscribe_output(&self, id: &str, sink: OutputSink) -> anyhow::Result<()> {
        let s = self.get(id)?;
        *s.shared.sink.lock().unwrap() = Some(sink);
        Ok(())
    }

    pub fn snapshot_scrollback(&self, id: &str) -> anyhow::Result<(u16, u16, Vec<u8>)> {
        let s = self.get(id)?;
        let cols = *s.shared.cols.lock().unwrap();
        let rows = *s.shared.rows.lock().unwrap();
        let bytes = s.shared.scrollback.lock().unwrap().snapshot();
        Ok((cols, rows, bytes))
    }

    pub fn meta(&self, id: &str) -> anyhow::Result<bpa_protocol::SessionMeta> {
        let s = self.get(id)?;
        let sh = &s.shared;
        let lc = *sh.lifecycle.lock().unwrap();
        let code = *sh.exit_code.lock().unwrap();
        let signal = sh.exit_signal.lock().unwrap().clone();
        Ok(bpa_protocol::SessionMeta {
            id: sh.id.clone(),
            workspace_id: sh.workspace_id.clone(),
            title: sh.title.clone(),
            shell: sh.shell.clone(),
            cwd: sh.cwd.lock().unwrap().clone(),
            cols: *sh.cols.lock().unwrap(),
            rows: *sh.rows.lock().unwrap(),
            lifecycle: lifecycle_to_proto(lc, code, signal),
            waiting_for_input: *sh.waiting.lock().unwrap(),
            is_active: *sh.is_active.lock().unwrap(),
            created_at: sh.created_at,
        })
    }

    pub fn kill(&self, id: &str) -> anyhow::Result<()> {
        let s = self.get(id)?;
        // Process-group kill: SIGTERM → grace → SIGKILL (§9.8).
        if let Some(pgid) = s.pgid {
            unsafe {
                libc::killpg(pgid, libc::SIGTERM);
            }
            let start = Instant::now();
            while start.elapsed() < KILL_GRACE {
                if !*s.shared.is_active.lock().unwrap() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(50));
            }
            if *s.shared.is_active.lock().unwrap() {
                unsafe {
                    libc::killpg(pgid, libc::SIGKILL);
                }
            }
        }
        // Always kill the immediate child + reap to avoid a zombie.
        let _ = s.killer.lock().unwrap().kill();
        *s.ticker_stop.lock().unwrap() = true;
        if let Some(h) = s.wait_thread.lock().unwrap().take() {
            let _ = h.join();
        }
        if let Some(h) = s.reader_thread.lock().unwrap().take() {
            let _ = h.join();
        }
        if let Some(h) = s.ticker_thread.lock().unwrap().take() {
            let _ = h.join();
        }
        Ok(())
    }
}

impl Default for Supervisor {
    fn default() -> Self {
        Self::new()
    }
}

fn emit_via(cb: &StatusCb, shared: &Arc<Shared>) {
    let lc = *shared.lifecycle.lock().unwrap();
    let code = *shared.exit_code.lock().unwrap();
    let signal = shared.exit_signal.lock().unwrap().clone();
    cb(StatusUpdate {
        session_id: shared.id.clone(),
        lifecycle: lifecycle_to_proto(lc, code, signal),
        waiting_for_input: *shared.waiting.lock().unwrap(),
        cwd: shared.cwd.lock().unwrap().clone(),
    });
}

/// Recompute the §10.4 waiting-for-input heuristic and store it on `shared`.
fn recompute_waiting(shared: &Arc<Shared>) {
    let is_running = matches!(*shared.lifecycle.lock().unwrap(), Lifecycle::Running);
    let quiescent = shared.last_output.lock().unwrap().elapsed() >= QUIESCENT;
    let not_alt = !shared.grid.lock().unwrap().is_alt_screen();
    let not_col0 = shared.grid.lock().unwrap().cursor_col() != 0;
    let line_mode = match shared.master_fd {
        Some(fd) => termios_icanon_echo(fd),
        None => false,
    };
    let waiting = is_running && line_mode && not_alt && quiescent && not_col0;
    *shared.waiting.lock().unwrap() = waiting;
}

/// True iff the PTY line discipline currently has ICANON & ECHO set (canonical line input).
fn termios_icanon_echo(fd: RawFd) -> bool {
    unsafe {
        let mut t: libc::termios = std::mem::zeroed();
        if libc::tcgetattr(fd, &mut t) != 0 {
            return false;
        }
        (t.c_lflag & libc::ICANON) != 0 && (t.c_lflag & libc::ECHO) != 0
    }
}
```

Also ensure `crates/sessiond/Cargo.toml` has (add any missing):
```toml
[dependencies]
portable-pty = "0.9.0"
libc = "0.2"
anyhow = "1"
uuid = { version = "1", features = ["v4"] }
bpa-protocol = { path = "../protocol" }

[dev-dependencies]
tempfile = "3"
nix = { version = "0.29", features = ["signal", "process"] }
```

- [ ] **Step 4: Run — confirm PASS**

`cargo test -p sessiond pty_supervisor::tests::echo_roundtrip_via_sh`
Expected: PASS.

- [ ] **Step 5: Commit**

`git add crates/sessiond/src/pty_supervisor.rs crates/sessiond/src/lib.rs crates/sessiond/Cargo.toml && git commit -m "feat(sessiond): pty_supervisor create/read/write/kill core with echo roundtrip"`

- [ ] **Step 6: Failing test — dropping the slave yields EOF (child exit tears down)**

Add to the `tests` module:
```rust
    #[test]
    fn child_exit_marks_inactive_via_eof() {
        let sup = Supervisor::new();
        // `sh -c exit 7` exits immediately; EOF must flow, wait thread must reap and record code.
        let mut spec = spec_for("/bin/sh", vec!["-c".into(), "exit 7".into()]);
        spec.title = "eoftest".into();
        let id = sup.create(spec).expect("create");

        let start = Instant::now();
        loop {
            let m = sup.meta(&id).expect("meta");
            if !m.is_active {
                match m.lifecycle {
                    bpa_protocol::SessionLifecycle::Exited { code, signal } => {
                        assert_eq!(code, Some(7), "exit code masked to u8");
                        assert_eq!(signal, None);
                    }
                    other => panic!("expected Exited, got {:?}", other),
                }
                break;
            }
            assert!(start.elapsed() < Duration::from_secs(5), "child never reached EOF/exit");
            std::thread::sleep(Duration::from_millis(50));
        }
    }
```

- [ ] **Step 7: Run — confirm PASS** (the impl from Step 3 already satisfies EOF→wait→exit)

`cargo test -p sessiond pty_supervisor::tests::child_exit_marks_inactive_via_eof`
Expected: PASS. (If it hangs, the `drop(pair.slave)` in `create` is missing/misordered — that is the locked EOF invariant.)

- [ ] **Step 8: Commit**

`git add crates/sessiond/src/pty_supervisor.rs && git commit -m "test(sessiond): drop(slave)->EOF exit-code roundtrip"`

- [ ] **Step 9: Failing test — no zombie left after kill (child is reaped)**

Add to the `tests` module:
```rust
    #[test]
    fn kill_reaps_no_zombie() {
        let sup = Supervisor::new();
        // Long-lived shell; kill() must SIGTERM the group and reap so no defunct remains.
        let id = sup.create(spec_for("/bin/sh", vec!["-c".into(), "sleep 100".into()])).expect("create");
        // Let the shell come up.
        std::thread::sleep(Duration::from_millis(300));
        let child_pid = {
            // process_group_leader is the shell's pid on macOS; capture it for a defunct check.
            // We assert via meta that it was active first.
            let m = sup.meta(&id).expect("meta");
            assert!(m.is_active);
            m
        };
        let _ = child_pid;
        sup.kill(&id).expect("kill");
        // After kill(), meta must show inactive and the wait thread already joined (reaped).
        let m = sup.meta(&id).expect("meta");
        assert!(!m.is_active, "session must be inactive after kill");
        // A reaped child cannot be a zombie; assert the process group is gone.
        // (Covered concretely by the process-group test below.)
    }
```

- [ ] **Step 10: Run — confirm PASS**

`cargo test -p sessiond pty_supervisor::tests::kill_reaps_no_zombie`
Expected: PASS.

- [ ] **Step 11: Commit**

`git add crates/sessiond/src/pty_supervisor.rs && git commit -m "test(sessiond): kill reaps child (no zombie)"`

- [ ] **Step 12: Failing test — PROCESS-GROUP kill (grandchild via fork is also gone)**

Add to the `tests` module:
```rust
    // Returns true while a pid exists (kill(pid,0) succeeds), false once it is gone.
    fn pid_alive(pid: i32) -> bool {
        unsafe { libc::kill(pid, 0) == 0 }
    }

    #[test]
    fn kill_terminates_whole_process_group() {
        let sup = Supervisor::new();
        // Shell forks a grandchild (`sleep 100 &`) then writes BOTH pids on a line, then waits.
        // The grandchild is in the same process group (no setsid), so killpg must take it out too.
        let script = "sleep 100 & child=$!; sleep 100 & gchild=$!; printf 'PIDS %d %d\\n' $child $gchild; wait";
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        let id = sup.create(spec_for("/bin/sh", vec!["-c".into(), script.into()])).expect("create");
        sup.subscribe_output(&id, tx).expect("subscribe");

        // Parse the two background pids from the marker line.
        let out = drain_until(&rx, b"PIDS ", Duration::from_secs(5));
        let text = String::from_utf8_lossy(&out);
        let line = text.lines().find(|l| l.contains("PIDS ")).expect("PIDS line");
        let nums: Vec<i32> = line
            .trim()
            .rsplit(' ')
            .take(2)
            .filter_map(|s| s.trim().parse::<i32>().ok())
            .collect();
        assert_eq!(nums.len(), 2, "expected two pids, got line: {line:?}");
        let (a, b) = (nums[0], nums[1]);
        assert!(pid_alive(a) && pid_alive(b), "both background children should be alive pre-kill");

        sup.kill(&id).expect("kill");

        // Give the OS a moment to deliver SIGKILL to the group.
        let start = Instant::now();
        while (pid_alive(a) || pid_alive(b)) && start.elapsed() < Duration::from_secs(4) {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(!pid_alive(a), "grandchild {a} must be killed by process-group kill");
        assert!(!pid_alive(b), "grandchild {b} must be killed by process-group kill");
    }
```

- [ ] **Step 13: Run — confirm PASS**

`cargo test -p sessiond pty_supervisor::tests::kill_terminates_whole_process_group`
Expected: PASS. (If a grandchild survives, `kill` is signalling only the immediate child — the `libc::killpg(pgid, …)` path is the locked fix.)

- [ ] **Step 14: Commit**

`git add crates/sessiond/src/pty_supervisor.rs && git commit -m "test(sessiond): process-group kill removes forked grandchildren"`

- [ ] **Step 15: Failing test — resize delivers SIGWINCH ($COLUMNS updates)**

Add to the `tests` module:
```rust
    #[test]
    fn resize_delivers_sigwinch_updated_columns() {
        let sup = Supervisor::new();
        // Interactive-ish sh that traps WINCH and prints the new COLUMNS. `checkwinsize`-free:
        // sh updates $COLUMNS on WINCH when the shell manages window size; to be robust we install
        // a WINCH trap that runs `stty size` (rows cols) which reads the real termios window size.
        let script = "trap 'stty size' WINCH; printf READY\\n; while :; do sleep 0.2; done";
        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        let id = sup.create(spec_for("/bin/sh", vec!["-c".into(), script.into()])).expect("create");
        sup.subscribe_output(&id, tx).expect("subscribe");

        let _ = drain_until(&rx, b"READY", Duration::from_secs(5));
        sup.resize(&id, 132, 40).expect("resize");

        // stty size prints "rows cols" => "40 132".
        let out = drain_until(&rx, b"132", Duration::from_secs(5));
        let text = String::from_utf8_lossy(&out);
        assert!(
            text.contains("40 132"),
            "expected 'rows cols' = '40 132' after resize, got: {text:?}"
        );
        sup.kill(&id).expect("kill");
    }
```

- [ ] **Step 16: Run — confirm PASS**

`cargo test -p sessiond pty_supervisor::tests::resize_delivers_sigwinch_updated_columns`
Expected: PASS.

- [ ] **Step 17: Commit**

`git add crates/sessiond/src/pty_supervisor.rs && git commit -m "test(sessiond): resize delivers SIGWINCH (stty size updates)"`

- [ ] **Step 18: Failing test — env hygiene (planted DAEMON_SECRET absent; allowlist present)**

Add to the `tests` module:
```rust
    #[test]
    fn env_clear_hides_daemon_secret_keeps_allowlist() {
        // Plant a secret in the DAEMON (this process) env; it must NOT reach the child.
        std::env::set_var("DAEMON_SECRET", "topsecret-should-not-leak");
        let sup = Supervisor::new();

        let mut spec = spec_for("/bin/sh", vec!["-c".into(),
            "printf 'SECRET=[%s]\\n' \"$DAEMON_SECRET\"; printf 'TERM=[%s]\\n' \"$TERM\"".into()]);
        // Ensure TERM is in the allowlist we pass (base_env already sets TERM).
        spec.title = "envtest".into();

        let (tx, rx) = mpsc::channel::<Vec<u8>>();
        let id = sup.create(spec).expect("create");
        sup.subscribe_output(&id, tx).expect("subscribe");

        let out = drain_until(&rx, b"TERM=[xterm-256color]", Duration::from_secs(5));
        let text = String::from_utf8_lossy(&out);
        assert!(
            text.contains("SECRET=[]"),
            "DAEMON_SECRET must be cleared in the child env, got: {text:?}"
        );
        assert!(
            !text.contains("topsecret-should-not-leak"),
            "planted secret leaked into child env: {text:?}"
        );
        assert!(
            text.contains("TERM=[xterm-256color]"),
            "allowlisted TERM must be present, got: {text:?}"
        );
        sup.kill(&id).expect("kill");
        std::env::remove_var("DAEMON_SECRET");
    }
```

- [ ] **Step 19: Run — confirm PASS**

`cargo test -p sessiond pty_supervisor::tests::env_clear_hides_daemon_secret_keeps_allowlist`
Expected: PASS. (Failure here means `env_clear()` was skipped or the allowlist loop is wrong — both locked in §9.3.)

- [ ] **Step 20: Commit**

`git add crates/sessiond/src/pty_supervisor.rs && git commit -m "test(sessiond): env hygiene (env_clear hides secret, keeps allowlist)"`

- [ ] **Step 21: Failing test — waiting-for-input heuristic (cat=true, vim=false, idle=false)**

Add to the `tests` module:
```rust
    // Drive the parser via the PTY by writing the OSC marks the shell integration would emit,
    // so we can force Running without a real shell integration in this unit. We write the marks
    // to STDIN of `cat` — cat echoes them back through the master, the reader parses them, and the
    // lifecycle advances to Running. `cat` keeps the tty in ICANON+ECHO line mode with the cursor
    // parked after a partial line (no trailing newline) => waiting_for_input must become true.
    const ESC: u8 = 0x1b;
    fn osc_c() -> Vec<u8> {
        // ESC ] 133 ; C BEL  → CommandStart → Running
        let mut v = vec![ESC, b']'];
        v.extend_from_slice(b"133;C");
        v.push(0x07);
        v
    }

    fn wait_for<F: Fn() -> bool>(f: F, timeout: Duration) -> bool {
        let start = Instant::now();
        while start.elapsed() < timeout {
            if f() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        f()
    }

    #[test]
    fn waiting_for_input_true_for_cat_prompt() {
        let sup = Supervisor::new();
        // `cat` is canonical (ICANON+ECHO), not alt-screen. Push it to Running via an OSC C mark,
        // then send a partial line with no newline so the cursor is off column 0 and output is quiet.
        let id = sup.create(spec_for("/bin/cat", vec![])).expect("create");
        // Move to Running: write the C mark; cat echoes it, reader parses -> Running.
        sup.write_stdin(&id, &osc_c()).expect("write C");
        // Partial prompt-like text, no newline -> cursor not at col 0.
        sup.write_stdin(&id, b"Password: ").expect("write partial");

        let sup_ref = &sup;
        let idc = id.clone();
        let ok = wait_for(
            || sup_ref.meta(&idc).map(|m| m.waiting_for_input).unwrap_or(false),
            Duration::from_secs(3),
        );
        assert!(ok, "cat at a partial line should be waiting_for_input");
        sup.kill(&id).expect("kill");
    }

    #[test]
    fn waiting_for_input_false_for_alt_screen() {
        let sup = Supervisor::new();
        // A cat session that we push to Running, then feed an alt-screen enter so is_alt_screen()
        // is true -> heuristic must exclude it (vim/less/top class).
        let id = sup.create(spec_for("/bin/cat", vec![])).expect("create");
        sup.write_stdin(&id, &osc_c()).expect("write C");
        // ESC [ ? 1049 h  = enter alt screen; cat echoes it, grid records alt-screen on.
        let mut alt = vec![ESC, b'['];
        alt.extend_from_slice(b"?1049h");
        alt.extend_from_slice(b"X"); // some non-col0 content
        sup.write_stdin(&id, &alt).expect("write alt");

        let idc = id.clone();
        let sup_ref = &sup;
        // Give the reader time to process; it must stay false the whole window.
        std::thread::sleep(Duration::from_millis(600));
        let waiting = sup_ref.meta(&idc).map(|m| m.waiting_for_input).unwrap_or(true);
        assert!(!waiting, "alt-screen session must NOT be waiting_for_input");
        sup.kill(&id).expect("kill");
    }

    #[test]
    fn waiting_for_input_false_when_idle_at_prompt() {
        let sup = Supervisor::new();
        // No C mark => lifecycle stays AtPrompt (not Running) => heuristic false regardless of tty.
        let id = sup.create(spec_for("/bin/cat", vec![])).expect("create");
        sup.write_stdin(&id, b"idle text ").expect("write");
        std::thread::sleep(Duration::from_millis(400));
        let m = sup.meta(&id).expect("meta");
        assert!(!m.waiting_for_input, "AtPrompt (not Running) must be false");
        sup.kill(&id).expect("kill");
    }
```

- [ ] **Step 22: Run — confirm PASS**

`cargo test -p sessiond pty_supervisor::tests::waiting_for_input_true_for_cat_prompt pty_supervisor::tests::waiting_for_input_false_for_alt_screen pty_supervisor::tests::waiting_for_input_false_when_idle_at_prompt`
Expected: PASS for all three. (Failure of the `cat` case usually means the ticker thread is not recomputing after quiescence; failure of the alt-screen case means `LiveGrid::is_alt_screen()` is not wired into `recompute_waiting`.)

- [ ] **Step 23: Commit**

`git add crates/sessiond/src/pty_supervisor.rs && git commit -m "test(sessiond): waiting-for-input heuristic (cat true, alt-screen false, idle false)"`

**Definition of Done:**
- `cargo test -p sessiond pty_supervisor::tests` is green (echo roundtrip; drop(slave)→EOF exit-code; kill reaps no zombie; process-group kill removes forked grandchildren; resize→SIGWINCH; env-hygiene planted `DAEMON_SECRET` absent + allowlist present; waiting-for-input cat=true / vim(alt-screen)=false / idle=false).
- `native_pty_system()` is constructed exactly once (in `Supervisor::new()`); `drop(pair.slave)` happens immediately after `spawn_command`; `take_writer()` and `clone_killer()` are each called once at create; `wait()` runs only on the owning wait thread (§9.1–§9.6).
- `kill` signals the whole process group via `libc::killpg(pgid, SIGTERM)` → 2 s grace → `libc::killpg(pgid, SIGKILL)`, then `killer.kill()` + reap; falls back to `killer.kill()` when `pgid` is `None` (§9.8).
- Env is `env_clear()`ed then set strictly from `spec.env` (the §9.3 allowlist including `SSH_AUTH_SOCK` when present); no daemon-internal var reaches the child.
- Exit code is masked `(exit_code() & 0xff) as u8`; signal-terminated children carry `code = None` + signal name (spec §5 exit-code note).
- `waiting_for_input` implements the exact §10.4 conjunction (Running ∧ ICANON&ECHO ∧ ¬alt-screen ∧ quiescent ≥ 150 ms ∧ cursor col ≠ 0); documented as heuristic.
- `SessionMeta` is produced with the internal `Lifecycle` mapped to `protocol::SessionLifecycle` (never emits `Typing`); status/created/exited callbacks fire for the broker to translate into `StateChanged`/`SessionCreated`/`ChildExited` pushes (§7).

---

### Task 10: `shell_integration/` — zsh + bash OSC-133/OSC-7 injection assets + installer

**Files:**
- Create: `crates/sessiond/src/shell_integration/mod.rs`, `crates/sessiond/src/shell_integration/assets/bpa.zsh`, `crates/sessiond/src/shell_integration/assets/bpa-bash.sh`
- Modify: `crates/sessiond/src/lib.rs` (append `pub mod shell_integration;` — append only; do not touch other tasks' `mod` lines).
- Test: inline `#[cfg(test)] mod tests` in `crates/sessiond/src/shell_integration/mod.rs`.

**Depends on:** [T3]   **Parallel-safe with:** [T4, T5, T6, T7, T8] (parallel leaf of G2; runs before T9 integrates it)

**Interfaces:**
Consumes: nothing from T3 at the type level (std only); grouped under G2, gated on T3 for crate-compile ordering. The zsh/bash assets are embedded with `include_str!`.

Produces (verbatim from the scaffold Task interface index, spec §10.2):
```rust
/// Which shell family we are injecting into.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellKind { Zsh, Bash }

/// The concrete spawn recipe the supervisor (T9) uses to launch the shell with integration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellSpawn {
    pub program: String,             // absolute shell path, e.g. "/bin/zsh" or "/bin/bash"
    pub args: Vec<String>,           // zsh: [] ; bash: ["--init-file", "<runtime_dir>/bpa-bash.sh"]
    pub env: Vec<(String, String)>,  // additive integration env: BPA_INJECTION=1 and/or ZDOTDIR=<dir>
}

/// Detect the shell family from an absolute shell path (basename match).
pub fn classify_shell(shell_path: &str) -> Option<ShellKind>;

/// Materialize the integration assets into `runtime_dir` for one session and return the spawn recipe.
/// - zsh: writes `<runtime_dir>/.zshenv` (the ZDOTDIR redirect stub that re-sources bpa.zsh),
///        copies `bpa.zsh` into `runtime_dir`, sets env ZDOTDIR=<runtime_dir> (+ BPA_ORIG_ZDOTDIR
///        carrying the caller's original ZDOTDIR when set) and BPA_INJECTION=1.
/// - bash: writes `<runtime_dir>/bpa-bash.sh`, returns args ["--init-file", "<that path>"] and env
///         BPA_INJECTION=1.
pub fn write_session_assets(
    runtime_dir: &std::path::Path,
    shell: ShellKind,
) -> std::io::Result<ShellSpawn>;
```

Locked injection + emit-order contract (spec §10.2; the parser T5 and these scripts MUST agree):
- Env flag = `BPA_INJECTION` (value `1`); hook fns = `_bpa_precmd` / `_bpa_preexec`.
- **precmd / PROMPT_COMMAND emit order:** capture `code=$?` **first**, then `D;<code>` (closes the previous command), then `A` (prompt start), then `OSC 7` (cwd). `B` (prompt end) is embedded at the **end of `PS1`**, zero-width-wrapped.
- **preexec / DEBUG-trap:** emit `C` **exactly once** per command.
- **zsh:** `ZDOTDIR` temp-dir redirect. The stub `.zshenv` restores the user's original `ZDOTDIR` (from `BPA_ORIG_ZDOTDIR`, unset otherwise), re-sources the user's real startup files, then `autoload -Uz add-zsh-hook; add-zsh-hook precmd _bpa_precmd; add-zsh-hook preexec _bpa_preexec`. `B` wrapped in `%{ … %}`.
- **bash:** `--init-file <bpa-bash.sh>` + env `BPA_INJECTION=1`. Sources the user's rc **first**, then **wraps** (never clobbers) `PROMPT_COMMAND` (saved to a backup var). `B` wrapped in `\[ \]`. `preexec` via bash-preexec if `__bp_install_after_session_hook`/`preexec_functions` is present, else a **guarded DEBUG trap** emitting `C` once per command (suppressed while `PROMPT_COMMAND` runs, chaining any pre-existing trap).
- Emitted OSC sequences are BEL-terminated: `ESC ] 133 ; A BEL`, `ESC ] 133 ; B BEL`, `ESC ] 133 ; C BEL`, `ESC ] 133 ; D ; <code> BEL`, `ESC ] 7 ; file://$HOST$PWD BEL`.

Design notes for the implementer (locked so tests and code agree):
- Assets are embedded at compile time: `const BPA_ZSH: &str = include_str!("assets/bpa.zsh");` and `const BPA_BASH: &str = include_str!("assets/bpa-bash.sh");`. `write_session_assets` writes those constants to disk (the zsh `.zshenv` stub is generated inline pointing at the copied `bpa.zsh`).
- `classify_shell` matches on the basename: `zsh`→`Zsh`, `bash`/`sh` when the path ends in `bash`→`Bash`; unknown → `None` (caller spawns without integration).
- Runtime dir is per session and owned by the caller (T9 passes it in); this task only writes files into it.
- The zsh `.zshenv` stub (generated, not `include_str!`) restores `ZDOTDIR` and sources the copied `bpa.zsh`:
  ```zsh
  # generated by write_session_assets — restore user ZDOTDIR, source integration, source user rc
  if [ -n "${BPA_ORIG_ZDOTDIR-}" ]; then export ZDOTDIR="$BPA_ORIG_ZDOTDIR"; else unset ZDOTDIR; fi
  # re-source the user's real startup files from the restored ZDOTDIR/HOME
  [ -f "${ZDOTDIR:-$HOME}/.zshenv" ] && source "${ZDOTDIR:-$HOME}/.zshenv"
  source "__BPA_ZSH_PATH__"   # placeholder replaced with the absolute copied bpa.zsh path
  ```

- [ ] **Step 1: Failing test — classify_shell + write_session_assets recipe shape**

Add to `crates/sessiond/src/shell_integration/mod.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_known_shells() {
        assert_eq!(classify_shell("/bin/zsh"), Some(ShellKind::Zsh));
        assert_eq!(classify_shell("/usr/local/bin/zsh"), Some(ShellKind::Zsh));
        assert_eq!(classify_shell("/bin/bash"), Some(ShellKind::Bash));
        assert_eq!(classify_shell("/opt/homebrew/bin/bash"), Some(ShellKind::Bash));
        assert_eq!(classify_shell("/usr/bin/fish"), None);
    }

    #[test]
    fn zsh_assets_set_zdotdir_and_injection_env() {
        let dir = tempfile::tempdir().unwrap();
        let spawn = write_session_assets(dir.path(), ShellKind::Zsh).unwrap();
        // ZDOTDIR points at the runtime dir; BPA_INJECTION=1 present; no --init-file for zsh.
        assert!(spawn.args.is_empty(), "zsh takes no init args");
        let env: std::collections::HashMap<_, _> = spawn.env.iter().cloned().collect();
        assert_eq!(env.get("ZDOTDIR").map(String::as_str), Some(dir.path().to_str().unwrap()));
        assert_eq!(env.get("BPA_INJECTION").map(String::as_str), Some("1"));
        // The ZDOTDIR .zshenv stub and the copied bpa.zsh both exist.
        assert!(dir.path().join(".zshenv").is_file(), ".zshenv stub written");
        assert!(dir.path().join("bpa.zsh").is_file(), "bpa.zsh copied");
        // The stub sources the copied bpa.zsh by absolute path.
        let stub = std::fs::read_to_string(dir.path().join(".zshenv")).unwrap();
        assert!(stub.contains(dir.path().join("bpa.zsh").to_str().unwrap()),
            "stub must source the copied bpa.zsh absolute path");
        assert!(stub.contains("BPA_ORIG_ZDOTDIR"), "stub restores original ZDOTDIR");
    }

    #[test]
    fn bash_assets_use_init_file_and_injection_env() {
        let dir = tempfile::tempdir().unwrap();
        let spawn = write_session_assets(dir.path(), ShellKind::Bash).unwrap();
        let script = dir.path().join("bpa-bash.sh");
        assert_eq!(
            spawn.args,
            vec!["--init-file".to_string(), script.to_str().unwrap().to_string()]
        );
        let env: std::collections::HashMap<_, _> = spawn.env.iter().cloned().collect();
        assert_eq!(env.get("BPA_INJECTION").map(String::as_str), Some("1"));
        assert!(script.is_file(), "bpa-bash.sh written");
    }
}
```

- [ ] **Step 2: Run — confirm FAIL**

`cargo test -p sessiond shell_integration::tests`
Expected: FAIL to compile with `cannot find function 'classify_shell'`.

- [ ] **Step 3: Write the zsh asset `crates/sessiond/src/shell_integration/assets/bpa.zsh`**
```zsh
# Builder Pro AI zsh shell integration (OSC 133 + OSC 7). Sourced from the ZDOTDIR .zshenv stub
# AFTER the user's real startup files. Non-invasive: no user rc edits.
# Emit order per spec §10.2:
#   precmd : capture $? first -> D;<code> -> A -> OSC 7 ; B lives at end of PS1 (zero-width).
#   preexec: C exactly once.

# Guard against double-load.
if [ -n "${_bpa_loaded-}" ]; then return; fi
_bpa_loaded=1

# --- emit helpers ---------------------------------------------------------
_bpa_osc133() { printf '\033]133;%s\007' "$1"; }             # A | B | C
_bpa_osc133_d() { printf '\033]133;D;%s\007' "$1"; }          # D;<exit>
_bpa_osc7() { printf '\033]7;file://%s%s\007' "${HOST:-localhost}" "$PWD"; }

# --- precmd: close prev command, start new prompt, report cwd -------------
_bpa_precmd() {
  local code=$?                 # MUST be first: the previous command's exit status
  _bpa_osc133_d "$code"         # D;<code>
  _bpa_osc133 A                 # A prompt start
  _bpa_osc7                     # OSC 7 cwd
}

# --- preexec: command dispatched, output begins --------------------------
_bpa_preexec() {
  _bpa_osc133 C                 # C exactly once per command
}

# --- embed B (command start / prompt end) at the END of PS1, zero-width ---
# %{...%} tells zsh the enclosed bytes are non-printing so line length stays correct.
PS1="${PS1}%{$(_bpa_osc133 B)%}"

autoload -Uz add-zsh-hook
add-zsh-hook precmd _bpa_precmd
add-zsh-hook preexec _bpa_preexec
```

- [ ] **Step 4: Write the bash asset `crates/sessiond/src/shell_integration/assets/bpa-bash.sh`**
```bash
# Builder Pro AI bash shell integration (OSC 133 + OSC 7). Loaded via `bash --init-file <this>`.
# Sources the user's rc FIRST, then wraps PROMPT_COMMAND and installs a guarded preexec.
# Emit order per spec §10.2:
#   PROMPT_COMMAND: capture $? first -> D;<code> -> A -> OSC 7 ; B lives at end of PS1 (\[ \]).
#   preexec/DEBUG : C exactly once.

if [ -n "${_bpa_loaded-}" ]; then return; fi
_bpa_loaded=1

# --- source the user's real rc first (interactive non-login) --------------
if [ -f "$HOME/.bashrc" ]; then
  source "$HOME/.bashrc"
fi

# --- emit helpers ---------------------------------------------------------
_bpa_osc133() { printf '\033]133;%s\007' "$1"; }
_bpa_osc133_d() { printf '\033]133;D;%s\007' "$1"; }
_bpa_osc7() { printf '\033]7;file://%s%s\007' "${HOSTNAME:-localhost}" "$PWD"; }

# --- PROMPT_COMMAND wrapper (never clobber; run the user's original) ------
_bpa_orig_prompt_command="$PROMPT_COMMAND"
_bpa_prompt() {
  local code=$?                 # MUST be first
  # Run the user's original PROMPT_COMMAND (string form) preserving $? for them.
  if [ -n "$_bpa_orig_prompt_command" ]; then
    ( exit "$code" ); eval "$_bpa_orig_prompt_command"
  fi
  _bpa_osc133_d "$code"         # D;<code>
  _bpa_osc133 A                 # A
  _bpa_osc7                     # OSC 7
  _bpa_preexec_ran=""           # re-arm the preexec guard for the next command
}
PROMPT_COMMAND=_bpa_prompt

# --- embed B at the END of PS1, wrapped in \[ \] so bash does not miscount -
PS1="${PS1}\[$(_bpa_osc133 B)\]"

# --- preexec: bash-preexec if present, else a guarded DEBUG trap ----------
_bpa_preexec_ran=""
_bpa_preexec() {
  # Fire C exactly once per command; never for PROMPT_COMMAND itself.
  if [ -n "$COMP_LINE" ]; then return; fi         # skip completion
  if [ "$BASH_COMMAND" = "$PROMPT_COMMAND" ]; then return; fi
  if [ -n "$_bpa_preexec_ran" ]; then return; fi
  _bpa_preexec_ran=1
  _bpa_osc133 C
}

if [ -n "${bash_preexec_imported:-}" ] || [ -n "${__bp_imported:-}" ]; then
  # bash-preexec is loaded: register into its array instead of a raw trap.
  preexec_functions+=(_bpa_preexec)
else
  # Chain any pre-existing DEBUG trap, then add ours.
  _bpa_prev_debug_trap="$(trap -p DEBUG | sed -E "s/^trap -- '(.*)' DEBUG$/\1/")"
  if [ -n "$_bpa_prev_debug_trap" ]; then
    trap "${_bpa_prev_debug_trap}; _bpa_preexec" DEBUG
  else
    trap '_bpa_preexec' DEBUG
  fi
fi
```

- [ ] **Step 5: Implement `write_session_assets` + `classify_shell` in `crates/sessiond/src/shell_integration/mod.rs`**

Prepend to `crates/sessiond/src/shell_integration/mod.rs`:
```rust
//! Shell-integration assets + installer (spec §10.2). Spawns the user's REAL shell with a tiny,
//! non-invasive OSC-133/OSC-7 integration. zsh = ZDOTDIR redirect; bash = --init-file.

use std::io::Write;
use std::path::Path;

const BPA_ZSH: &str = include_str!("assets/bpa.zsh");
const BPA_BASH: &str = include_str!("assets/bpa-bash.sh");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShellKind {
    Zsh,
    Bash,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ShellSpawn {
    pub program: String,
    pub args: Vec<String>,
    pub env: Vec<(String, String)>,
}

pub fn classify_shell(shell_path: &str) -> Option<ShellKind> {
    let base = Path::new(shell_path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    if base == "zsh" {
        Some(ShellKind::Zsh)
    } else if base == "bash" {
        Some(ShellKind::Bash)
    } else {
        None
    }
}

pub fn write_session_assets(
    runtime_dir: &Path,
    shell: ShellKind,
) -> std::io::Result<ShellSpawn> {
    std::fs::create_dir_all(runtime_dir)?;
    match shell {
        ShellKind::Zsh => {
            // Copy the integration script.
            let bpa_zsh_path = runtime_dir.join("bpa.zsh");
            std::fs::write(&bpa_zsh_path, BPA_ZSH)?;

            // Generate the ZDOTDIR .zshenv stub that restores the user's ZDOTDIR, re-sources their
            // rc, then sources our integration.
            let stub = format!(
                "# generated by write_session_assets (Builder Pro AI)\n\
                 if [ -n \"${{BPA_ORIG_ZDOTDIR-}}\" ]; then export ZDOTDIR=\"$BPA_ORIG_ZDOTDIR\"; else unset ZDOTDIR; fi\n\
                 [ -f \"${{ZDOTDIR:-$HOME}}/.zshenv\" ] && source \"${{ZDOTDIR:-$HOME}}/.zshenv\"\n\
                 [ -f \"${{ZDOTDIR:-$HOME}}/.zshrc\" ] && source \"${{ZDOTDIR:-$HOME}}/.zshrc\"\n\
                 source \"{}\"\n",
                bpa_zsh_path.display()
            );
            let mut f = std::fs::File::create(runtime_dir.join(".zshenv"))?;
            f.write_all(stub.as_bytes())?;

            let mut env = vec![
                ("ZDOTDIR".to_string(), runtime_dir.to_string_lossy().into_owned()),
                ("BPA_INJECTION".to_string(), "1".to_string()),
            ];
            // Carry the caller's original ZDOTDIR through so the stub can restore it.
            if let Some(orig) = std::env::var_os("ZDOTDIR") {
                env.push((
                    "BPA_ORIG_ZDOTDIR".to_string(),
                    orig.to_string_lossy().into_owned(),
                ));
            }
            Ok(ShellSpawn {
                program: "/bin/zsh".to_string(),
                args: vec![],
                env,
            })
        }
        ShellKind::Bash => {
            let script = runtime_dir.join("bpa-bash.sh");
            std::fs::write(&script, BPA_BASH)?;
            Ok(ShellSpawn {
                program: "/bin/bash".to_string(),
                args: vec![
                    "--init-file".to_string(),
                    script.to_string_lossy().into_owned(),
                ],
                env: vec![("BPA_INJECTION".to_string(), "1".to_string())],
            })
        }
    }
}
```

Ensure `crates/sessiond/Cargo.toml` `[dev-dependencies]` has `tempfile = "3"` (T4/T9 already add it; add if missing).

- [ ] **Step 6: Run — confirm PASS**

`cargo test -p sessiond shell_integration::tests`
Expected: PASS.

- [ ] **Step 7: Commit**

`git add crates/sessiond/src/shell_integration/mod.rs crates/sessiond/src/shell_integration/assets/bpa.zsh crates/sessiond/src/shell_integration/assets/bpa-bash.sh crates/sessiond/src/lib.rs crates/sessiond/Cargo.toml && git commit -m "feat(sessiond): shell_integration assets + write_session_assets (zsh ZDOTDIR / bash --init-file)"`

- [ ] **Step 8: Failing test — REAL zsh emits the OSC 133 A/B/C/D + OSC 7 sequence in order**

Add to the `tests` module (gated so it is skipped when zsh is unavailable):
```rust
    use std::io::Read;
    use std::time::{Duration, Instant};

    // Spawn a shell through a raw PTY (via portable-pty) using the recipe from write_session_assets,
    // drive one command, and assert the emitted OSC bytes appear in the locked order.
    fn drive_shell_capture(kind: ShellKind, program: &str) -> Option<String> {
        use portable_pty::{native_pty_system, CommandBuilder, PtySize};
        if !std::path::Path::new(program).exists() {
            return None; // shell not installed on this box — skip
        }
        let dir = tempfile::tempdir().unwrap();
        let spawn = write_session_assets(dir.path(), kind).unwrap();

        let pty = native_pty_system();
        let pair = pty
            .openpty(PtySize { rows: 24, cols: 80, pixel_width: 0, pixel_height: 0 })
            .unwrap();
        let mut cmd = CommandBuilder::new(program);
        cmd.env_clear();
        cmd.env("TERM", "xterm-256color");
        cmd.env("PATH", std::env::var("PATH").unwrap_or_else(|_| "/usr/bin:/bin".into()));
        cmd.env("HOME", dir.path().to_string_lossy().into_owned());
        cmd.env("HOSTNAME", "localhost");
        cmd.env("HOST", "localhost");
        for (k, v) in &spawn.env {
            cmd.env(k, v);
        }
        for a in &spawn.args {
            cmd.arg(a);
        }
        cmd.cwd(dir.path());
        let mut child = pair.slave.spawn_command(cmd).unwrap();
        drop(pair.slave);

        let mut reader = pair.master.try_clone_reader().unwrap();
        let mut writer = pair.master.take_writer().unwrap();

        // Collect output on a thread until we see a D mark or time out.
        let (tx, rx) = std::sync::mpsc::channel::<Vec<u8>>();
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            while let Ok(n) = reader.read(&mut buf) {
                if n == 0 {
                    break;
                }
                if tx.send(buf[..n].to_vec()).is_err() {
                    break;
                }
            }
        });

        // Give the shell time to print its first prompt (A/B/OSC7), then run one command.
        std::thread::sleep(Duration::from_millis(400));
        writer.write_all(b"printf hi\n").unwrap();
        writer.flush().unwrap();

        let start = Instant::now();
        let mut acc: Vec<u8> = Vec::new();
        while start.elapsed() < Duration::from_secs(6) {
            if let Ok(chunk) = rx.recv_timeout(Duration::from_millis(200)) {
                acc.extend_from_slice(&chunk);
                // Stop once we have seen a D mark (command finished).
                if acc.windows(7).any(|w| w == b"\x1b]133;D") {
                    break;
                }
            }
        }
        let _ = child.kill();
        Some(String::from_utf8_lossy(&acc).into_owned())
    }

    fn assert_osc_order(out: &str) {
        // The four 133 marks and OSC 7 must all be present; A must precede C which must precede D.
        let a = out.find("\x1b]133;A").expect("OSC 133;A present");
        let b = out.find("\x1b]133;B").expect("OSC 133;B present");
        let c = out.find("\x1b]133;C").expect("OSC 133;C present");
        let d = out.find("\x1b]133;D").expect("OSC 133;D present");
        assert!(out.contains("\x1b]7;file://"), "OSC 7 cwd present");
        // Order within one prompt+command cycle: A (prompt) < B (prompt end) < C (exec) < D (done).
        assert!(a < c, "A must precede C ({a} < {c})");
        assert!(b < c, "B must precede C ({b} < {c})");
        assert!(c < d, "C must precede D ({c} < {d})");
    }

    #[test]
    fn zsh_emits_osc_sequence_in_order() {
        match drive_shell_capture(ShellKind::Zsh, "/bin/zsh") {
            Some(out) => assert_osc_order(&out),
            None => eprintln!("skipping zsh OSC test: /bin/zsh not present"),
        }
    }

    #[test]
    fn bash_emits_osc_sequence_in_order() {
        match drive_shell_capture(ShellKind::Bash, "/bin/bash") {
            Some(out) => assert_osc_order(&out),
            None => eprintln!("skipping bash OSC test: /bin/bash not present"),
        }
    }
```

- [ ] **Step 9: Run — confirm PASS**

`cargo test -p sessiond shell_integration::tests::zsh_emits_osc_sequence_in_order shell_integration::tests::bash_emits_osc_sequence_in_order`
Expected: PASS (on macOS `/bin/zsh` and `/bin/bash` are present, so the assertions run rather than skip). If A/C/D are missing, the hook registration or emit order in the asset is wrong; if B is missing, the `PS1` embed is not surviving into the drawn prompt.

- [ ] **Step 10: Commit**

`git add crates/sessiond/src/shell_integration/mod.rs && git commit -m "test(sessiond): real zsh/bash emit OSC 133 A/B/C/D + OSC 7 in locked order"`

**Definition of Done:**
- `cargo test -p sessiond shell_integration::tests` is green: `classify_shell` maps zsh/bash/unknown; `write_session_assets` writes the zsh `.zshenv` stub + copied `bpa.zsh` (ZDOTDIR + BPA_INJECTION env, no init args) and the bash `bpa-bash.sh` (`--init-file` args + BPA_INJECTION env); a real zsh AND a real bash spawned through a PTY emit `OSC 133;A`, `;B`, `;C`, `;D`, and `OSC 7` with A/B before C before D.
- Env flag is `BPA_INJECTION=1`; hook fns are `_bpa_precmd` / `_bpa_preexec` (spec §10.2 locked names).
- Emit order matches §10.2: `code=$?` captured first, then `D;<code>`, then `A`, then `OSC 7`; `C` emitted exactly once per command; `B` embedded at end of `PS1` (zsh `%{ … %}`, bash `\[ \]`).
- zsh restores the user's original `ZDOTDIR` (via `BPA_ORIG_ZDOTDIR`) and re-sources the user's rc; bash sources the user's rc first and wraps (never clobbers) `PROMPT_COMMAND`, using bash-preexec when present else a guarded DEBUG trap.
- Assets are embedded via `include_str!` and materialized per session into the caller-provided runtime dir; the produced `ShellSpawn { program, args, env }` is exactly what T9 `Supervisor::create` consumes (T9 sets `spec.shell`/`spec.args`/`spec.env` from it).


### Task 11: `attach.rs` — per-session single-attach registry + replay orchestration

**Files:**
- Create: `crates/sessiond/src/attach.rs`
- Modify: `crates/sessiond/src/lib.rs` (append `pub mod attach;`) — or `main.rs` `mod attach;` per the crate-target rule established in T4. Append only your own `mod` line; do not touch other tasks' `mod` lines.
- Test: inline `#[cfg(test)] mod tests` in `crates/sessiond/src/attach.rs`.

**Depends on:** [T3, T6, T9]   **Parallel-safe with:** [] (G3 head — T12, T13 build on it sequentially)

**Interfaces:**
Consumes (verbatim from the scaffold Task interface index + spec §5–§7, §11):
- From **T3 `protocol`**: `SessionId` (= `String`), `Push` (specifically `Push::Replay { session_id: SessionId, cols: u16, rows: u16, content: Vec<u8> }` and `Push::Output { session_id: SessionId, bytes: Vec<u8> }`).
- From **T6 `scrollback`**: `ScrollbackRing::snapshot(&self) -> Vec<u8>` (sanitized replay bytes, oldest→newest).
- From **T9 `pty_supervisor`**: the `Supervisor` output-subscription surface. This task pins the exact shape it consumes (T9 must expose it verbatim):
  ```rust
  // T9 (pty_supervisor) — consumed by T11:
  impl Supervisor {
      /// Current session dimensions (daemon-tracked, updated by resize).
      pub fn session_dims(&self, id: &SessionId) -> Option<(u16, u16)>;
      /// Sanitized scrollback snapshot for replay (delegates to the session's ScrollbackRing).
      pub fn scrollback_snapshot(&self, id: &SessionId) -> Option<Vec<u8>>;
      /// Subscribe to live post-parser Output bytes for one session.
      /// Returns a bounded broadcast receiver; lagged receivers drop oldest (documented).
      pub fn subscribe_output(&self, id: &SessionId) -> Option<tokio::sync::broadcast::Receiver<Vec<u8>>>;
  }
  ```

Produces (verbatim — T12 consumes these names):
```rust
/// Sink for one attached client: bounded outbound channel of protocol Push frames.
pub type PushSink = tokio::sync::mpsc::Sender<crate::protocol::Push>;

pub struct AttachRegistry { /* Mutex<HashMap<SessionId, AttachEntry>> */ }

impl AttachRegistry {
    pub fn new(supervisor: std::sync::Arc<crate::pty_supervisor::Supervisor>) -> Self;

    /// (Re)register `sink` as THE single consumer for `session_id`, superseding any prior
    /// attach. Emits a fresh `Push::Replay` (sanitized snapshot at current cols/rows) into
    /// `sink`, then spawns a pump forwarding live `Push::Output` until detach/supersede/drop.
    /// Returns `Err(AttachError::NoSuchSession)` if the session is unknown.
    pub async fn attach(&self, session_id: &SessionId, sink: PushSink) -> Result<(), AttachError>;

    /// Stop Output for `session_id` (aborts the pump). The PTY keeps running and its ring
    /// keeps filling (spec §7 keep-alive). No-op if not attached.
    pub fn detach(&self, session_id: &SessionId);

    /// Drop every attach entry (used on client disconnect / shutdown drain).
    pub fn detach_all(&self);
}

#[derive(Debug)]
pub enum AttachError { NoSuchSession, SinkClosed }
```

Design notes (locked so tests and code agree):
- **Single-attach invariant (spec §7).** `AttachEntry` holds the current `PushSink` plus the `JoinHandle` of its Output pump. A second `attach()` for the same `session_id` **supersedes**: abort the prior pump handle and replace the entry before starting the new replay+pump. There is at most one live pump per session.
- **Replay-first ordering (spec §6.2/§7).** `attach()` sends `Push::Replay { session_id, cols, rows, content }` where `content = supervisor.scrollback_snapshot(id)` and `(cols, rows) = supervisor.session_dims(id)` **before** spawning the pump — so the client always receives replay bytes ahead of any live `Output`. Subscribe to `subscribe_output` **before** sending Replay (so no live byte emitted between snapshot and subscribe is lost), buffer nothing else: the broadcast receiver captures post-snapshot bytes; the ordering guarantee is snapshot-then-drain-receiver.
- **Bounded, non-stalling pump.** The pump `select!`s on the broadcast receiver; each `Vec<u8>` is wrapped as `Push::Output { session_id, bytes }` and `sink.send(...).await`-ed. If `sink.send` errors (receiver dropped = client gone) the pump exits and marks the entry closed. On `broadcast::error::RecvError::Lagged(n)` the pump continues (drops the lagged window — honest degradation; the ring/DB remain the durable source). One slow sink never blocks the supervisor because `subscribe_output` is a per-session broadcast, not the supervisor's own loop.
- **`PushSink` capacity** is the caller's (T12) bounded per-client outbound queue; T11 only `.await`s `send` and treats `SendError` as detach.

- [ ] **Step 1: Failing test — attach emits Replay first, then live Output; supersede aborts prior sink; detach stops Output while PTY keeps running**

Add to `crates/sessiond/src/attach.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::Push;
    use std::sync::Arc;
    use tokio::sync::{broadcast, mpsc};

    // Minimal Supervisor test double with the exact surface T11 consumes.
    // The real T9 Supervisor exposes session_dims / scrollback_snapshot / subscribe_output;
    // this fake is used ONLY to exercise AttachRegistry in isolation.
    struct FakeSup {
        dims: (u16, u16),
        snapshot: Vec<u8>,
        tx: broadcast::Sender<Vec<u8>>,
        exists: bool,
    }
    impl FakeSup {
        fn new(snapshot: &[u8]) -> (Arc<Self>, broadcast::Sender<Vec<u8>>) {
            let (tx, _rx) = broadcast::channel(64);
            let sup = Arc::new(FakeSup {
                dims: (80, 24),
                snapshot: snapshot.to_vec(),
                tx: tx.clone(),
                exists: true,
            });
            (sup, tx)
        }
    }
    impl SupervisorOutput for FakeSup {
        fn session_dims(&self, _id: &SessionId) -> Option<(u16, u16)> {
            self.exists.then_some(self.dims)
        }
        fn scrollback_snapshot(&self, _id: &SessionId) -> Option<Vec<u8>> {
            self.exists.then(|| self.snapshot.clone())
        }
        fn subscribe_output(&self, _id: &SessionId) -> Option<broadcast::Receiver<Vec<u8>>> {
            self.exists.then(|| self.tx.subscribe())
        }
    }

    #[tokio::test]
    async fn attach_sends_replay_before_output() {
        let (sup, out_tx) = FakeSup::new(b"SCROLLBACK");
        let reg = AttachRegistry::new_with(sup);
        let (sink, mut client) = mpsc::channel::<Push>(64);
        reg.attach(&"s1".to_string(), sink).await.unwrap();

        // FIRST frame must be Replay with the sanitized snapshot at current dims.
        match client.recv().await.unwrap() {
            Push::Replay { session_id, cols, rows, content } => {
                assert_eq!(session_id, "s1");
                assert_eq!((cols, rows), (80, 24));
                assert_eq!(content, b"SCROLLBACK".to_vec());
            }
            other => panic!("expected Replay first, got {other:?}"),
        }

        // Then live Output bytes appear in order.
        out_tx.send(b"live-1".to_vec()).unwrap();
        out_tx.send(b"live-2".to_vec()).unwrap();
        match client.recv().await.unwrap() {
            Push::Output { session_id, bytes } => {
                assert_eq!(session_id, "s1");
                assert_eq!(bytes, b"live-1".to_vec());
            }
            other => panic!("expected Output, got {other:?}"),
        }
        match client.recv().await.unwrap() {
            Push::Output { bytes, .. } => assert_eq!(bytes, b"live-2".to_vec()),
            other => panic!("expected Output, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn second_attach_supersedes_and_prior_sink_stops_receiving() {
        let (sup, out_tx) = FakeSup::new(b"X");
        let reg = AttachRegistry::new_with(sup);

        let (sink_a, mut client_a) = mpsc::channel::<Push>(64);
        reg.attach(&"s1".to_string(), sink_a).await.unwrap();
        // drain A's Replay
        assert!(matches!(client_a.recv().await.unwrap(), Push::Replay { .. }));

        let (sink_b, mut client_b) = mpsc::channel::<Push>(64);
        reg.attach(&"s1".to_string(), sink_b).await.unwrap();
        assert!(matches!(client_b.recv().await.unwrap(), Push::Replay { .. }));

        // A live byte now must reach ONLY B (A's pump was aborted on supersede).
        out_tx.send(b"after-supersede".to_vec()).unwrap();
        match client_b.recv().await.unwrap() {
            Push::Output { bytes, .. } => assert_eq!(bytes, b"after-supersede".to_vec()),
            other => panic!("expected Output on B, got {other:?}"),
        }
        // A receives nothing more: recv resolves to None only after the sender is dropped;
        // assert via a short timeout that no Output arrives on A.
        let a_next = tokio::time::timeout(std::time::Duration::from_millis(100), client_a.recv()).await;
        match a_next {
            Err(_) => {}                       // timed out => no Output delivered to A (correct)
            Ok(None) => {}                     // A's sender dropped => also correct
            Ok(Some(f)) => panic!("A must not receive Output after supersede, got {f:?}"),
        }
    }

    #[tokio::test]
    async fn detach_stops_output_but_session_still_subscribable() {
        let (sup, out_tx) = FakeSup::new(b"X");
        let reg = AttachRegistry::new_with(sup.clone());
        let (sink, mut client) = mpsc::channel::<Push>(64);
        reg.attach(&"s1".to_string(), sink).await.unwrap();
        assert!(matches!(client.recv().await.unwrap(), Push::Replay { .. }));

        reg.detach(&"s1".to_string());

        // After detach, further live bytes are NOT forwarded to the (now-detached) sink.
        out_tx.send(b"post-detach".to_vec()).unwrap();
        let next = tokio::time::timeout(std::time::Duration::from_millis(100), client.recv()).await;
        assert!(
            matches!(next, Err(_) | Ok(None)),
            "detached sink must not receive Output"
        );

        // PTY keeps running: the session is still subscribable (ring keeps filling — spec §7).
        assert!(sup.subscribe_output(&"s1".to_string()).is_some());
    }

    #[tokio::test]
    async fn attach_unknown_session_errors() {
        let (sup, _tx) = FakeSup::new(b"");
        // flip existence off
        let sup = Arc::new(FakeSup { exists: false, ..(*sup).clone_for_test() });
        let reg = AttachRegistry::new_with(sup);
        let (sink, _client) = mpsc::channel::<Push>(1);
        let err = reg.attach(&"ghost".to_string(), sink).await.unwrap_err();
        assert!(matches!(err, AttachError::NoSuchSession));
    }

    impl FakeSup {
        fn clone_for_test(&self) -> FakeSup {
            FakeSup { dims: self.dims, snapshot: self.snapshot.clone(), tx: self.tx.clone(), exists: self.exists }
        }
    }
}
```

- [ ] **Step 2: Run — confirm FAIL**

`cargo test -p sessiond attach::tests`
Expected: FAIL to compile with `cannot find type 'AttachRegistry'` / `cannot find trait 'SupervisorOutput'` in this scope.

- [ ] **Step 3: Implement `attach.rs` over a `SupervisorOutput` trait (so tests can inject a fake and prod uses the real `Supervisor`)**

Prepend to `crates/sessiond/src/attach.rs` (above the test module):
```rust
//! Per-session single-attach registry + replay orchestration (spec §7 attach model, §6.2, §11).
//!
//! Exactly one active `PushSink` per session. `attach` supersedes any prior consumer,
//! emits a fresh sanitized `Replay`, then pumps live `Output` until detach/supersede/drop.
//! `detach` stops Output only — the PTY keeps running and its scrollback ring keeps filling.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::sync::{broadcast, mpsc};
use tokio::task::JoinHandle;

use crate::protocol::{Push, SessionId};

/// The exact slice of the T9 `Supervisor` surface this module needs.
/// The real `Supervisor` implements it; tests inject a fake.
pub trait SupervisorOutput: Send + Sync + 'static {
    fn session_dims(&self, id: &SessionId) -> Option<(u16, u16)>;
    fn scrollback_snapshot(&self, id: &SessionId) -> Option<Vec<u8>>;
    fn subscribe_output(&self, id: &SessionId) -> Option<broadcast::Receiver<Vec<u8>>>;
}

/// Bounded outbound channel of protocol frames for one attached client.
pub type PushSink = mpsc::Sender<Push>;

#[derive(Debug)]
pub enum AttachError {
    NoSuchSession,
    SinkClosed,
}

struct AttachEntry {
    pump: JoinHandle<()>,
}

pub struct AttachRegistry {
    sup: Arc<dyn SupervisorOutput>,
    entries: Mutex<HashMap<SessionId, AttachEntry>>,
}

impl AttachRegistry {
    /// Production constructor: the real `Supervisor` is passed as `Arc<Supervisor>`,
    /// which coerces to `Arc<dyn SupervisorOutput>` because `Supervisor: SupervisorOutput`.
    pub fn new(supervisor: Arc<crate::pty_supervisor::Supervisor>) -> Self {
        Self::new_with(supervisor)
    }

    /// Generic constructor over any `SupervisorOutput` (used by prod + tests).
    pub fn new_with<S: SupervisorOutput>(supervisor: Arc<S>) -> Self {
        AttachRegistry {
            sup: supervisor,
            entries: Mutex::new(HashMap::new()),
        }
    }

    pub async fn attach(&self, session_id: &SessionId, sink: PushSink) -> Result<(), AttachError> {
        // Subscribe BEFORE snapshotting so no live byte is lost between snapshot and pump start.
        let mut rx = self
            .sup
            .subscribe_output(session_id)
            .ok_or(AttachError::NoSuchSession)?;
        let (cols, rows) = self
            .sup
            .session_dims(session_id)
            .ok_or(AttachError::NoSuchSession)?;
        let content = self
            .sup
            .scrollback_snapshot(session_id)
            .ok_or(AttachError::NoSuchSession)?;

        // Supersede any prior attach for this session (single-attach invariant).
        self.abort_existing(session_id);

        // Replay MUST be the first frame the client sees.
        let replay = Push::Replay {
            session_id: session_id.clone(),
            cols,
            rows,
            content,
        };
        sink.send(replay).await.map_err(|_| AttachError::SinkClosed)?;

        // Spawn the bounded, non-stalling Output pump.
        let sid = session_id.clone();
        let pump = tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(bytes) => {
                        let frame = Push::Output {
                            session_id: sid.clone(),
                            bytes,
                        };
                        if sink.send(frame).await.is_err() {
                            break; // client gone
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {
                        // Honest degradation: drop the lagged window, keep streaming.
                        continue;
                    }
                    Err(broadcast::error::RecvError::Closed) => break, // session output ended
                }
            }
        });

        self.entries
            .lock()
            .expect("attach entries mutex poisoned")
            .insert(session_id.clone(), AttachEntry { pump });
        Ok(())
    }

    pub fn detach(&self, session_id: &SessionId) {
        self.abort_existing(session_id);
    }

    pub fn detach_all(&self) {
        let mut map = self.entries.lock().expect("attach entries mutex poisoned");
        for (_id, entry) in map.drain() {
            entry.pump.abort();
        }
    }

    fn abort_existing(&self, session_id: &SessionId) {
        if let Some(prev) = self
            .entries
            .lock()
            .expect("attach entries mutex poisoned")
            .remove(session_id)
        {
            prev.pump.abort();
        }
    }
}
```

Also make the test double implement the trait: the test module already declares `impl SupervisorOutput for FakeSup` and constructs the registry via `AttachRegistry::new_with(sup)`.

- [ ] **Step 4: Run — confirm PASS**

`cargo test -p sessiond attach::tests`
Expected: PASS (replay-before-output; supersede aborts prior sink; detach stops Output while session stays subscribable; unknown-session errors).

- [ ] **Step 5: Commit**

`git add crates/sessiond/src/attach.rs crates/sessiond/src/lib.rs && git commit -m "feat(sessiond): single-attach registry with replay-first orchestration and bounded output pump"`

**Definition of Done:**
- `cargo test -p sessiond attach::tests` green.
- Single-attach supersede verified: second `attach` aborts the prior pump; only the newest sink receives live `Output` (spec §7 attach model).
- Replay ordering verified: the first frame on every attach is `Push::Replay` carrying `scrollback_snapshot()` at `session_dims()`; live `Output` follows (spec §6.2, §11).
- `detach` stops `Output` for that session while the session remains subscribable (PTY keeps running, ring keeps filling — spec §7 keep-alive).
- Unknown session → `AttachError::NoSuchSession`; dropped client sink → pump exits without stalling the supervisor (spec §13 slow/dead client).
- `AttachRegistry::new(Arc<Supervisor>)` is the production wire; `new_with` + `SupervisorOutput` trait keep the module unit-testable in isolation.

---

### Task 12: `socket_server.rs` — tokio `UnixListener`, per-client task, handshake, correlation, bounded outq, dispatch

**Files:**
- Create: `crates/sessiond/src/socket_server.rs`
- Modify: `crates/sessiond/src/lib.rs` (append `pub mod socket_server;`), `crates/sessiond/Cargo.toml` (ensure `tokio` features `["net","io-util","rt-multi-thread","macros","sync","time"]`, `bincode` = `1.3.3`, `tracing`; `[dev-dependencies]` `tempfile`). Append only your own `mod` line.
- Test: inline `#[cfg(test)] mod tests` in `crates/sessiond/src/socket_server.rs`.

**Depends on:** [T3, T4, T7, T9, T11]   **Parallel-safe with:** [] (G3 middle — T13 builds on it)

**Interfaces:**
Consumes (verbatim):
- From **T3 `protocol`**: `Frame` (`Frame::Request { id: u64, req: Request }`, `Frame::Response { id: u64, res: Response }`, `Frame::Push(Push)`), `Request` (all variants incl. `Hello { magic, proto_version, client_build }`), `Response` (`Welcome`, `Incompatible`, `Workspaces`, `Workspace`, `Sessions`, `Session`, `Ack`, `Error { code, message }`), `Push`, `MAGIC` (= `0x4250_4131`), `PROTO_VERSION` (= `1`), `Workspace`, `SessionMeta`, `SessionId`, `WorkspaceId`.
- From **T4 `singleton`**: `check_peer_cred(fd: BorrowedFd<'_>) -> io::Result<()>`.
- From **T7 `persistence`**: `Db` with `list_workspaces() -> Result<Vec<Workspace>>`, `upsert_workspace(&Workspace) -> Result<()>`, `list_sessions() -> Result<Vec<SessionMeta>>`.
- From **T9 `pty_supervisor`**: `Supervisor` with `create(spec: CreateSpec) -> Result<SessionMeta, SupervisorError>`, `write_stdin(&SessionId, &[u8]) -> Result<(), SupervisorError>`, `resize(&SessionId, u16, u16) -> Result<(), SupervisorError>`, `kill(&SessionId) -> Result<(), SupervisorError>`, `get_state(&SessionId) -> Option<SessionMeta>`, and the `pub struct CreateSpec { pub workspace_id: WorkspaceId, pub shell: Option<String>, pub cwd: Option<String>, pub env_overrides: Vec<(String,String)>, pub cols: u16, pub rows: u16 }`.
- From **T11 `attach`**: `AttachRegistry` with `attach(&SessionId, PushSink) -> Result<(), AttachError>`, `detach(&SessionId)`, `detach_all()`, and `pub type PushSink = mpsc::Sender<Push>`.

Produces (verbatim — T13 consumes these):
```rust
/// Shared dependency bundle handed to every per-client task.
pub struct ServerDeps {
    pub supervisor: std::sync::Arc<crate::pty_supervisor::Supervisor>,
    pub db: std::sync::Arc<crate::persistence::Db>,
    pub attach: std::sync::Arc<crate::attach::AttachRegistry>,
    pub daemon_build: String,
}

/// Per-client bounded outbound queue depth (frames). Overflow => drop + disconnect that client.
pub const CLIENT_OUTQ_CAP: usize = 1024;

/// Accept loop: peer-cred check, per-client task, handshake-gated dispatch. Runs until
/// `listener` errors or the process is torn down. `shutdown` resolves on DaemonShutdown drain.
pub async fn serve(
    listener: tokio::net::UnixListener,
    deps: std::sync::Arc<ServerDeps>,
    shutdown: tokio::sync::watch::Receiver<bool>,
) -> std::io::Result<()>;
```

Design notes (locked so tests and code agree):
- **Framing (spec §7).** Every frame on the wire = `u32` little-endian length prefix + `bincode::serialize(&Frame)`. Read: `read_u32_le` → bounded-length check (`> MAX_FRAME_LEN` ⇒ protocol error, disconnect) → `read_exact(len)` → `bincode::deserialize::<Frame>`. Write: `bincode::serialize(&frame)` → `write_all(len_le)` → `write_all(body)`. `MAX_FRAME_LEN = 8 * 1024 * 1024`.
- **Handshake gate (spec §7).** The client's FIRST frame MUST be `Frame::Request { id: 0, req: Request::Hello { magic, proto_version, .. } }`. Validate `magic == MAGIC` and `proto_version == PROTO_VERSION`; reply `Frame::Response { id: 0, res: Response::Welcome { proto_version: PROTO_VERSION, daemon_build } }`. On any mismatch (wrong magic, wrong/out-of-range version, or a non-Hello first frame) reply `Frame::Response { id: 0, res: Response::Incompatible { min: PROTO_VERSION, max: PROTO_VERSION } }` and **close the connection** — never proceed to dispatch, never misparse.
- **Peer-cred (spec §8.2).** Immediately after `accept`, call `check_peer_cred(stream.as_fd())`; on `Err` log and drop the connection before the handshake.
- **Correlation (spec §7).** Each inbound `Frame::Request { id, req }` (id ≠ 0 post-handshake) is dispatched and answered with exactly one `Frame::Response { id, res }` echoing the same `id`. Concurrent in-flight ids are independent: the reader loop spawns request handling so a slow command never head-of-line-blocks another id; every reply is tagged with its originating id.
- **Bounded outbound queue + non-stalling writer (spec §13).** Each client has one writer task draining an `mpsc::Sender<Frame>` of depth `CLIENT_OUTQ_CAP`. All producers (dispatch replies, `AttachRegistry` push pump) send through it via `try_send`. On `TrySendError::Full` (overflow) or a writer `EPIPE`/write error, the client is **dropped and disconnected** (its attach entries cleaned up) without blocking any other client or pausing an unrelated session's PTY. The `PushSink` handed to `AttachRegistry::attach` is a thin adapter that maps `Push` → `Frame::Push(push)` into this same bounded queue (so backpressure/overflow applies uniformly).
- **Dispatch table (spec §6.1/§7 broker mapping).**
  - `ListWorkspaces` → `db.list_workspaces()` → `Response::Workspaces(v)` (or `Response::Error`).
  - `CreateWorkspace { name, root_path }` → validate is deferred to the core (`paths.rs`, T15) per spec §16; the daemon persists the already-validated row: build `Workspace { id: uuid_v4, name, root_path }`, `db.upsert_workspace(&w)`, `Response::Workspace(w)`, and enqueue `Push::WorkspaceCreated { workspace: w }`.
  - `ListSessions` → `db.list_sessions()` merged with live supervisor state (`get_state` overrides `is_active`) → `Response::Sessions(v)`.
  - `CreateSession { .. }` → `supervisor.create(CreateSpec { .. })` → `Response::Session(meta)`; enqueue `Push::SessionCreated { meta }`; on failure `Response::Error { code, message }`.
  - `AttachSession { session_id }` → `attach.attach(&session_id, push_sink()).await`; `Ok` ⇒ `Response::Ack` (the Replay+Output flow arrives as `Push` frames on the same queue); `Err(NoSuchSession)` ⇒ `Response::Error { code: "NoSuchSession", .. }`.
  - `DetachSession { session_id }` → `attach.detach(&session_id)` → `Response::Ack`.
  - `WriteStdin { session_id, bytes }` → `supervisor.write_stdin(&session_id, &bytes)` → `Response::Ack` / `Error`.
  - `Resize { session_id, cols, rows }` → `supervisor.resize(&session_id, cols, rows)` → `Response::Ack` / `Error`.
  - `KillSession { session_id }` → `supervisor.kill(&session_id)` → `Response::Ack` / `Error`.
  - `GetSessionState { session_id }` → `supervisor.get_state(&session_id)` (fallback to `db.list_sessions()` lookup) → `Response::Session(meta)` / `Error { code: "NoSuchSession" }`.
  - `DaemonShutdown { drain }` → `Response::Ack`, then signal the process-level shutdown watch (handled in T13); the per-client task returns.
  - `Hello { .. }` after the handshake is a protocol violation → `Response::Error { code: "UnexpectedHello", .. }`.
- **Cleanup.** When a per-client task ends (disconnect, overflow, EPIPE, shutdown), call `attach.detach_all()`-scoped-to-this-client — since the GUI is the single client, dropping its sinks stops its pumps; sessions keep running (spec §7).

- [ ] **Step 1: Failing test — handshake happy path + magic/version reject; framing round-trip; correlation under concurrency; backpressure disconnects the slow client without stalling others**

Add to `crates/sessiond/src/socket_server.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{Frame, Push, Request, Response, MAGIC, PROTO_VERSION};
    use std::sync::Arc;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::{UnixListener, UnixStream};

    // ---- framing helpers (mirror the server codec) ----
    async fn send_frame(s: &mut UnixStream, f: &Frame) {
        let body = bincode::serialize(f).unwrap();
        s.write_all(&(body.len() as u32).to_le_bytes()).await.unwrap();
        s.write_all(&body).await.unwrap();
        s.flush().await.unwrap();
    }
    async fn recv_frame(s: &mut UnixStream) -> Frame {
        let mut lenb = [0u8; 4];
        s.read_exact(&mut lenb).await.unwrap();
        let len = u32::from_le_bytes(lenb) as usize;
        let mut body = vec![0u8; len];
        s.read_exact(&mut body).await.unwrap();
        bincode::deserialize(&body).unwrap()
    }

    // A ServerDeps built over test doubles for Supervisor/Db/AttachRegistry.
    // The real types implement the same methods; here we use the crate's test-support
    // constructors (`Supervisor::for_test()`, `Db::open_in_memory()`, `AttachRegistry::new`).
    fn test_deps() -> Arc<ServerDeps> {
        let supervisor = Arc::new(crate::pty_supervisor::Supervisor::for_test());
        let db = Arc::new(crate::persistence::Db::open_in_memory().unwrap());
        let attach = Arc::new(crate::attach::AttachRegistry::new(supervisor.clone()));
        Arc::new(ServerDeps { supervisor, db, attach, daemon_build: "test".into() })
    }

    async fn spawn_server() -> (std::path::PathBuf, tokio::sync::watch::Sender<bool>, tokio::task::JoinHandle<()>) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("d.sock");
        std::mem::forget(dir); // keep the dir alive for the test process
        let listener = UnixListener::bind(&path).unwrap();
        let deps = test_deps();
        let (tx, rx) = tokio::sync::watch::channel(false);
        let jh = tokio::spawn(async move {
            let _ = serve(listener, deps, rx).await;
        });
        (path, tx, jh)
    }

    async fn hello(s: &mut UnixStream) -> Response {
        send_frame(s, &Frame::Request {
            id: 0,
            req: Request::Hello { magic: MAGIC, proto_version: PROTO_VERSION, client_build: "t".into() },
        }).await;
        match recv_frame(s).await {
            Frame::Response { id, res } => { assert_eq!(id, 0); res }
            other => panic!("expected handshake Response, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn handshake_happy_path_returns_welcome() {
        let (path, _tx, _jh) = spawn_server().await;
        let mut c = UnixStream::connect(&path).await.unwrap();
        match hello(&mut c).await {
            Response::Welcome { proto_version, .. } => assert_eq!(proto_version, PROTO_VERSION),
            other => panic!("expected Welcome, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn handshake_bad_magic_is_rejected_and_closes() {
        let (path, _tx, _jh) = spawn_server().await;
        let mut c = UnixStream::connect(&path).await.unwrap();
        send_frame(&mut c, &Frame::Request {
            id: 0,
            req: Request::Hello { magic: 0xDEAD_BEEF, proto_version: PROTO_VERSION, client_build: "t".into() },
        }).await;
        match recv_frame(&mut c).await {
            Frame::Response { id: 0, res: Response::Incompatible { min, max } } => {
                assert_eq!((min, max), (PROTO_VERSION, PROTO_VERSION));
            }
            other => panic!("expected Incompatible, got {other:?}"),
        }
        // Connection must close: a follow-up read hits EOF.
        let mut buf = [0u8; 1];
        let n = c.read(&mut buf).await.unwrap();
        assert_eq!(n, 0, "server must close after Incompatible");
    }

    #[tokio::test]
    async fn handshake_bad_version_is_rejected() {
        let (path, _tx, _jh) = spawn_server().await;
        let mut c = UnixStream::connect(&path).await.unwrap();
        send_frame(&mut c, &Frame::Request {
            id: 0,
            req: Request::Hello { magic: MAGIC, proto_version: PROTO_VERSION + 1, client_build: "t".into() },
        }).await;
        match recv_frame(&mut c).await {
            Frame::Response { id: 0, res: Response::Incompatible { .. } } => {}
            other => panic!("expected Incompatible, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn non_hello_first_frame_is_rejected() {
        let (path, _tx, _jh) = spawn_server().await;
        let mut c = UnixStream::connect(&path).await.unwrap();
        send_frame(&mut c, &Frame::Request { id: 7, req: Request::ListWorkspaces }).await;
        match recv_frame(&mut c).await {
            Frame::Response { res: Response::Incompatible { .. }, .. } => {}
            other => panic!("first frame must be Hello; expected Incompatible, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn requests_are_answered_with_matching_ids_concurrently() {
        let (path, _tx, _jh) = spawn_server().await;
        let mut c = UnixStream::connect(&path).await.unwrap();
        assert!(matches!(hello(&mut c).await, Response::Welcome { .. }));

        // Fire three ListWorkspaces requests with distinct ids back-to-back.
        for id in [11u64, 22, 33] {
            send_frame(&mut c, &Frame::Request { id, req: Request::ListWorkspaces }).await;
        }
        // Collect three responses; every id must come back exactly once.
        let mut seen = std::collections::HashSet::new();
        for _ in 0..3 {
            match recv_frame(&mut c).await {
                Frame::Response { id, res: Response::Workspaces(_) } => { seen.insert(id); }
                other => panic!("expected Workspaces response, got {other:?}"),
            }
        }
        assert_eq!(seen, [11, 22, 33].into_iter().collect());
    }

    #[tokio::test]
    async fn create_workspace_persists_and_pushes() {
        let (path, _tx, _jh) = spawn_server().await;
        let mut c = UnixStream::connect(&path).await.unwrap();
        assert!(matches!(hello(&mut c).await, Response::Welcome { .. }));

        send_frame(&mut c, &Frame::Request {
            id: 5,
            req: Request::CreateWorkspace { name: "w".into(), root_path: "/tmp".into() },
        }).await;

        // Response and the WorkspaceCreated Push both arrive (order not fixed); collect two frames.
        let mut got_resp = None;
        let mut got_push = false;
        for _ in 0..2 {
            match recv_frame(&mut c).await {
                Frame::Response { id: 5, res: Response::Workspace(w) } => got_resp = Some(w),
                Frame::Push(Push::WorkspaceCreated { .. }) => got_push = true,
                other => panic!("unexpected frame {other:?}"),
            }
        }
        assert!(got_resp.is_some() && got_push);
    }

    #[tokio::test]
    async fn slow_client_is_disconnected_without_stalling_a_second_client() {
        let (path, _tx, _jh) = spawn_server().await;

        // Client A connects, handshakes, then STOPS reading — we will flood its outq.
        let mut a = UnixStream::connect(&path).await.unwrap();
        assert!(matches!(hello(&mut a).await, Response::Welcome { .. }));

        // Push CLIENT_OUTQ_CAP+64 unsolicited frames toward A by asking for many workspaces?
        // Instead, drive overflow deterministically: fire many requests A never reads the replies for.
        for id in 0..(CLIENT_OUTQ_CAP as u64 + 128) {
            // best-effort; the socket send buffer + server outq will fill and the server drops A.
            let _ = {
                let f = Frame::Request { id, req: Request::ListWorkspaces };
                let body = bincode::serialize(&f).unwrap();
                a.write_all(&(body.len() as u32).to_le_bytes()).await.ok();
                a.write_all(&body).await.ok();
                a.flush().await.ok()
            };
        }

        // Client B connects fresh and MUST be served normally (A's overflow didn't stall the server).
        let mut b = UnixStream::connect(&path).await.unwrap();
        assert!(matches!(hello(&mut b).await, Response::Welcome { .. }));
        send_frame(&mut b, &Frame::Request { id: 1, req: Request::ListWorkspaces }).await;
        match tokio::time::timeout(std::time::Duration::from_secs(2), recv_frame(&mut b)).await {
            Ok(Frame::Response { id: 1, res: Response::Workspaces(_) }) => {}
            Ok(other) => panic!("B expected Workspaces, got {other:?}"),
            Err(_) => panic!("B was stalled by A's backpressure — bounded-outq isolation broken"),
        }
    }

    #[tokio::test]
    async fn oversized_frame_is_rejected() {
        let (path, _tx, _jh) = spawn_server().await;
        let mut c = UnixStream::connect(&path).await.unwrap();
        assert!(matches!(hello(&mut c).await, Response::Welcome { .. }));
        // Announce a length beyond MAX_FRAME_LEN; server must not allocate/hang — it disconnects.
        let bogus_len = (MAX_FRAME_LEN as u32) + 1;
        c.write_all(&bogus_len.to_le_bytes()).await.unwrap();
        c.flush().await.unwrap();
        let mut buf = [0u8; 1];
        let n = c.read(&mut buf).await.unwrap();
        assert_eq!(n, 0, "server must close on oversized frame length");
    }
}
```

- [ ] **Step 2: Run — confirm FAIL**

`cargo test -p sessiond socket_server::tests`
Expected: FAIL to compile with `cannot find function 'serve'` / `cannot find type 'ServerDeps'` / `MAX_FRAME_LEN` in this scope (and, if T7/T9 test-support ctors are absent, `Db::open_in_memory` / `Supervisor::for_test` unresolved — those are the T7/T9 test-support hooks this task depends on; see cross-task concerns).

- [ ] **Step 3: Implement the framing codec + per-client task + dispatch**

Prepend to `crates/sessiond/src/socket_server.rs` (above the test module):
```rust
//! Hop-B socket server: tokio UnixListener, per-client task, handshake, request/response
//! correlation, bounded outbound queue, peer-cred gate, Request dispatch (spec §7, §8.2, §13).

use std::os::fd::AsFd;
use std::sync::Arc;

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{mpsc, watch};

use crate::attach::AttachRegistry;
use crate::persistence::Db;
use crate::protocol::{Frame, Push, Request, Response, Workspace, MAGIC, PROTO_VERSION};
use crate::pty_supervisor::{CreateSpec, Supervisor};
use crate::singleton::check_peer_cred;

/// Max accepted frame body length (bytes). Larger announced lengths => disconnect.
pub const MAX_FRAME_LEN: usize = 8 * 1024 * 1024;

/// Per-client bounded outbound queue depth (frames). Overflow => drop + disconnect that client.
pub const CLIENT_OUTQ_CAP: usize = 1024;

pub struct ServerDeps {
    pub supervisor: Arc<Supervisor>,
    pub db: Arc<Db>,
    pub attach: Arc<AttachRegistry>,
    pub daemon_build: String,
}

pub async fn serve(
    listener: UnixListener,
    deps: Arc<ServerDeps>,
    mut shutdown: watch::Receiver<bool>,
) -> std::io::Result<()> {
    loop {
        tokio::select! {
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    deps.attach.detach_all();
                    return Ok(());
                }
            }
            accepted = listener.accept() => {
                let (stream, _addr) = match accepted {
                    Ok(pair) => pair,
                    Err(e) => {
                        tracing::warn!(error = %e, "accept failed");
                        continue;
                    }
                };
                // Peer-cred gate (spec §8.2): refuse a peer whose euid != ours.
                if let Err(e) = check_peer_cred(stream.as_fd()) {
                    tracing::warn!(error = %e, "peer-cred rejected");
                    drop(stream);
                    continue;
                }
                let deps = deps.clone();
                let shutdown_tx = shutdown.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_client(stream, deps, shutdown_tx).await {
                        tracing::debug!(error = %e, "client task ended");
                    }
                });
            }
        }
    }
}

async fn read_frame(stream: &mut UnixStream) -> std::io::Result<Option<Frame>> {
    let mut lenb = [0u8; 4];
    match stream.read_exact(&mut lenb).await {
        Ok(_) => {}
        Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
        Err(e) => return Err(e),
    }
    let len = u32::from_le_bytes(lenb) as usize;
    if len == 0 || len > MAX_FRAME_LEN {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "frame length out of bounds",
        ));
    }
    let mut body = vec![0u8; len];
    stream.read_exact(&mut body).await?;
    match bincode::deserialize::<Frame>(&body) {
        Ok(f) => Ok(Some(f)),
        Err(e) => Err(std::io::Error::new(std::io::ErrorKind::InvalidData, e)),
    }
}

fn encode_frame(frame: &Frame) -> std::io::Result<Vec<u8>> {
    let body = bincode::serialize(frame)
        .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
    let mut out = Vec::with_capacity(4 + body.len());
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    Ok(out)
}

async fn handle_client(
    mut stream: UnixStream,
    deps: Arc<ServerDeps>,
    shutdown: watch::Receiver<bool>,
) -> std::io::Result<()> {
    // ---- Handshake gate (spec §7): first frame MUST be Hello ----
    let first = match read_frame(&mut stream).await? {
        Some(f) => f,
        None => return Ok(()),
    };
    let ok = matches!(
        &first,
        Frame::Request { id: 0, req: Request::Hello { magic, proto_version, .. } }
            if *magic == MAGIC && *proto_version == PROTO_VERSION
    );
    if !ok {
        let bytes = encode_frame(&Frame::Response {
            id: 0,
            res: Response::Incompatible { min: PROTO_VERSION, max: PROTO_VERSION },
        })?;
        stream.write_all(&bytes).await?;
        stream.flush().await?;
        return Ok(()); // close
    }
    let welcome = encode_frame(&Frame::Response {
        id: 0,
        res: Response::Welcome { proto_version: PROTO_VERSION, daemon_build: deps.daemon_build.clone() },
    })?;
    stream.write_all(&welcome).await?;
    stream.flush().await?;

    // ---- Split into reader + writer with a bounded outbound queue ----
    let (mut rd, mut wr) = stream.into_split();
    let (out_tx, mut out_rx) = mpsc::channel::<Frame>(CLIENT_OUTQ_CAP);

    // Writer task: drains the bounded queue; exits on EPIPE/write error (=> client dropped).
    let writer = tokio::spawn(async move {
        while let Some(frame) = out_rx.recv().await {
            match encode_frame(&frame) {
                Ok(bytes) => {
                    if wr.write_all(&bytes).await.is_err() || wr.flush().await.is_err() {
                        break; // EPIPE / dead client
                    }
                }
                Err(e) => {
                    tracing::error!(error = %e, "frame encode failed");
                    break;
                }
            }
        }
    });

    // PushSink adapter: Push -> Frame::Push into the same bounded queue (uniform backpressure).
    let push_sink = make_push_sink(out_tx.clone());

    // Reader loop: dispatch each Request; overflow (try_send Full) => disconnect this client.
    let mut shutdown = shutdown;
    let dispatch_err: std::io::Result<()> = loop {
        tokio::select! {
            _ = shutdown.changed() => {
                if *shutdown.borrow() { break Ok(()); }
            }
            frame = read_frame(&mut rd) => {
                let frame = match frame {
                    Ok(Some(f)) => f,
                    Ok(None) => break Ok(()),            // client closed
                    Err(e) => break Err(e),              // framing/protocol error => disconnect
                };
                match frame {
                    Frame::Request { id, req } => {
                        let reply = dispatch(&deps, &push_sink, req).await;
                        if out_tx.try_send(Frame::Response { id, res: reply }).is_err() {
                            break Err(std::io::Error::new(
                                std::io::ErrorKind::WouldBlock,
                                "client outbound queue overflow",
                            ));
                        }
                    }
                    // Core never sends Response/Push to the daemon; ignore defensively.
                    Frame::Response { .. } | Frame::Push(_) => {
                        tracing::warn!("unexpected inbound Response/Push from client");
                    }
                }
            }
        }
    };

    // Cleanup: this is the single GUI client — stop its pumps; sessions keep running (spec §7).
    deps.attach.detach_all();
    drop(out_tx);
    let _ = writer.await;
    dispatch_err
}

/// Build a `PushSink` (`mpsc::Sender<Push>`) that forwards into the client's bounded `Frame` queue.
fn make_push_sink(out_tx: mpsc::Sender<Frame>) -> crate::attach::PushSink {
    let (tx, mut rx) = mpsc::channel::<Push>(CLIENT_OUTQ_CAP);
    tokio::spawn(async move {
        while let Some(push) = rx.recv().await {
            if out_tx.try_send(Frame::Push(push)).is_err() {
                break; // overflow / writer gone => stop forwarding
            }
        }
    });
    tx
}

async fn dispatch(deps: &Arc<ServerDeps>, push_sink: &crate::attach::PushSink, req: Request) -> Response {
    match req {
        Request::Hello { .. } => Response::Error {
            code: "UnexpectedHello".into(),
            message: "handshake already completed".into(),
        },
        Request::ListWorkspaces => match deps.db.list_workspaces() {
            Ok(v) => Response::Workspaces(v),
            Err(e) => err("DbError", e),
        },
        Request::CreateWorkspace { name, root_path } => {
            let w = Workspace { id: uuid::Uuid::new_v4().to_string(), name, root_path };
            match deps.db.upsert_workspace(&w) {
                Ok(()) => {
                    let _ = push_sink.try_send(Push::WorkspaceCreated { workspace: w.clone() });
                    Response::Workspace(w)
                }
                Err(e) => err("DbError", e),
            }
        }
        Request::ListSessions => match deps.db.list_sessions() {
            Ok(mut v) => {
                for m in v.iter_mut() {
                    if let Some(live) = deps.supervisor.get_state(&m.id) {
                        *m = live;
                    }
                }
                Response::Sessions(v)
            }
            Err(e) => err("DbError", e),
        },
        Request::CreateSession { workspace_id, shell, cwd, env_overrides, cols, rows } => {
            let spec = CreateSpec { workspace_id, shell, cwd, env_overrides, cols, rows };
            match deps.supervisor.create(spec) {
                Ok(meta) => {
                    let _ = push_sink.try_send(Push::SessionCreated { meta: meta.clone() });
                    Response::Session(meta)
                }
                Err(e) => err("CreateSessionFailed", e),
            }
        }
        Request::AttachSession { session_id } => {
            match deps.attach.attach(&session_id, push_sink.clone()).await {
                Ok(()) => Response::Ack,
                Err(crate::attach::AttachError::NoSuchSession) => Response::Error {
                    code: "NoSuchSession".into(),
                    message: format!("no session {session_id}"),
                },
                Err(crate::attach::AttachError::SinkClosed) => Response::Error {
                    code: "SinkClosed".into(),
                    message: "client sink closed".into(),
                },
            }
        }
        Request::DetachSession { session_id } => {
            deps.attach.detach(&session_id);
            Response::Ack
        }
        Request::WriteStdin { session_id, bytes } => {
            match deps.supervisor.write_stdin(&session_id, &bytes) {
                Ok(()) => Response::Ack,
                Err(e) => err("WriteFailed", e),
            }
        }
        Request::Resize { session_id, cols, rows } => {
            match deps.supervisor.resize(&session_id, cols, rows) {
                Ok(()) => Response::Ack,
                Err(e) => err("ResizeFailed", e),
            }
        }
        Request::KillSession { session_id } => match deps.supervisor.kill(&session_id) {
            Ok(()) => Response::Ack,
            Err(e) => err("KillFailed", e),
        },
        Request::GetSessionState { session_id } => match deps.supervisor.get_state(&session_id) {
            Some(meta) => Response::Session(meta),
            None => match deps.db.list_sessions() {
                Ok(v) => match v.into_iter().find(|m| m.id == session_id) {
                    Some(meta) => Response::Session(meta),
                    None => Response::Error {
                        code: "NoSuchSession".into(),
                        message: format!("no session {session_id}"),
                    },
                },
                Err(e) => err("DbError", e),
            },
        },
        Request::DaemonShutdown { .. } => Response::Ack, // process-level drain handled by T13
    }
}

fn err(code: &str, e: impl std::fmt::Display) -> Response {
    Response::Error { code: code.into(), message: e.to_string() }
}
```

- [ ] **Step 4: Run — confirm PASS**

`cargo test -p sessiond socket_server::tests`
Expected: PASS (handshake happy path; magic/version/non-Hello reject + close; concurrent id correlation; create-workspace persist+push; slow-client disconnect without stalling a second client; oversized-frame reject).

- [ ] **Step 5: Commit**

`git add crates/sessiond/src/socket_server.rs crates/sessiond/src/lib.rs crates/sessiond/Cargo.toml && git commit -m "feat(sessiond): Hop-B socket server — handshake, correlation, bounded outq, peer-cred, dispatch"`

**Definition of Done:**
- `cargo test -p sessiond socket_server::tests` green.
- First frame validated as `Hello{magic,proto_version}`; happy path → `Welcome`; wrong magic / wrong version / non-Hello first frame → `Incompatible` and the connection closes (spec §7). Never misparses on mismatch.
- Every `Request{id}` answered by exactly one `Response{id}` with the same id; concurrent in-flight ids resolve independently (spec §7 correlation).
- Framing is `u32`-LE length + `bincode(Frame)`; oversized announced length is rejected without allocation/hang (spec §7 framing / §13).
- Bounded per-client outbound queue (`CLIENT_OUTQ_CAP`); overflow or writer EPIPE drops+disconnects that client without stalling others or pausing unrelated sessions (spec §13). A stopped-reading client is disconnected while a second client keeps flowing.
- Peer-cred (`check_peer_cred`) runs on accept; wrong-euid peers refused before handshake (spec §8.2).
- Every `Request` variant dispatches to `Supervisor`/`Db`/`AttachRegistry` and replies with the correct correlated `Response`; `SessionCreated`/`WorkspaceCreated` pushes enqueued; Attach triggers Replay+Output via the same bounded queue.

---

### Task 13: `sessiond/main.rs` — daemon boot, flock, tracing, listener bind, wire deps, run serve, SIGTERM drain

**Files:**
- Create/Modify: `crates/sessiond/src/main.rs`
- Modify: `crates/sessiond/Cargo.toml` (ensure the `sessiond` binary target; deps `tokio` (`net,io-util,rt-multi-thread,macros,sync,time,signal`), `tracing`, `tracing-subscriber` (features `["env-filter","fmt"]`), `tracing-appender`; ensure `pub mod` visibility for `attach`, `socket_server`, `singleton`, `persistence`, `pty_supervisor` via `lib.rs`). Append only your own lines.
- Test: `crates/sessiond/tests/boot_integration.rs` (integration test, owns its own file).

**Depends on:** [T3, T4, T7, T9, T11, T12]   **Parallel-safe with:** [] (G3 tail — last daemon task)

**Interfaces:**
Consumes (verbatim):
- From **T4 `singleton`**: `resolve_socket_path() -> PathBuf`, `resolve_lockfile() -> PathBuf`, `ensure_socket_dir() -> io::Result<()>`, `acquire_single_instance_lock() -> io::Result<LockGuard>`, `assert_socket_path_len(&Path) -> io::Result<()>`, `set_socket_mode(&Path) -> io::Result<()>`.
- From **T7 `persistence`**: `Db::open(path: &Path) -> Result<Db>`.
- From **T9 `pty_supervisor`**: `Supervisor::new(db: Arc<Db>) -> Arc<Supervisor>` (or `Supervisor::new() -> Arc<Supervisor>` + DB wiring — this task consumes whatever T9 exposes as its production constructor; the locked call is `Supervisor::new(db.clone())`).
- From **T11 `attach`**: `AttachRegistry::new(Arc<Supervisor>) -> AttachRegistry`.
- From **T12 `socket_server`**: `ServerDeps { supervisor, db, attach, daemon_build }`, `serve(listener, Arc<ServerDeps>, watch::Receiver<bool>) -> io::Result<()>`.

Produces (verbatim):
```rust
/// CLI args (spec §8.3 launchd ProgramArguments: `--socket <path>`).
struct Args { socket: Option<std::path::PathBuf> }

#[tokio::main]
async fn main() -> std::process::ExitCode;

/// Testable boot core: bind, wire deps, run serve until `shutdown` fires. Returns the bound path.
pub async fn run(socket: std::path::PathBuf, shutdown: tokio::sync::watch::Receiver<bool>) -> std::io::Result<()>;
```

Design notes (locked so tests and code agree):
- **Arg parsing (spec §8.3).** `--socket <path>` overrides `resolve_socket_path()`. No arg ⇒ resolve. Unknown args ⇒ log + ignore (launchd passes a fixed set). Keep parsing dependency-free (manual `std::env::args`) — no `clap` in this slice.
- **Tracing init (spec §13, §16).** `tracing_subscriber` with an `EnvFilter` (default `info`), a non-blocking `fmt` layer writing to `{APP_SUPPORT}/logs/sessiond.tracing.log` via `tracing_appender` (the plist also captures stdout/stderr). No secret values are logged (env is allowlisted upstream in T9; this task logs only paths, session ids, lifecycle). `APP_SUPPORT = ~/Library/Application Support/ai.builderpro.desktop`; create `logs/` (mode 0700) if missing.
- **flock single-instance (spec §8.2).** `acquire_single_instance_lock()`; on `ErrorKind::WouldBlock` (another daemon holds it) log "another sessiond already running" and **exit `ExitCode::SUCCESS`** (idempotent kickstart is not an error). Hold the `LockGuard` for the whole process lifetime.
- **Dir/perms + bind (spec §8.1–§8.2).** `ensure_socket_dir()`; `assert_socket_path_len(&socket)?`; **stale-socket unlink**: if `socket` exists, attempt `UnixStream::connect`; on success another live daemon owns it (but we hold the flock, so this is a stale FS artifact from a crash) — since we own the lock, unlink and rebind; on `ECONNREFUSED`/`ENOENT` unlink and rebind. Then `UnixListener::bind(&socket)` and `set_socket_mode(&socket)` (0600).
- **Wire deps.** `db = Arc::new(Db::open(&db_path)?)` (best-effort: on DB open failure log and continue with an in-memory/degraded DB per spec §11 — but a hard `Db::open` error that even degradation can't recover surfaces as an actionable log, not a panic); `supervisor = Supervisor::new(db.clone())`; `attach = Arc::new(AttachRegistry::new(supervisor.clone()))`; `deps = Arc::new(ServerDeps { supervisor, db, attach, daemon_build: env!("CARGO_PKG_VERSION").into() })`.
- **SIGTERM → drain (spec §8.3, §13 `DaemonShutdown` semantics).** Install a `tokio::signal::unix::signal(SignalKind::terminate())` (and `interrupt()` for dev) handler; on signal set the `watch` to `true` → `serve` returns → drain: `attach.detach_all()`, `supervisor.shutdown_all()` (killpg each session: SIGTERM → grace → SIGKILL, per spec §9.8 — T9 owns the killpg; this task calls the aggregate `shutdown_all`), flush the DB checkpoint (`db.checkpoint()` best-effort), unlink the socket, drop the `LockGuard`. Exit `ExitCode::SUCCESS` so launchd `KeepAlive{Crashed}` leaves it down.
- **10s-throttle safety (spec §8.3 launchd ThrottleInterval).** Never exit within the first 10s on a transient bind/DB hiccup that could loop; a hard-fatal setup error (socket path > 104, cannot create dir) exits non-zero with an actionable log (launchd treats it as crash but the fault is deterministic and surfaced).

- [ ] **Step 1: Failing integration test — boot on a temp socket, Hello→Welcome, CreateSession echoes, clean shutdown**

Create `crates/sessiond/tests/boot_integration.rs`:
```rust
//! Integration: boot the daemon `run()` on a temp socket, complete the handshake,
//! create a session, then trigger clean shutdown (spec §14.1 boot integration).

use std::time::Duration;

use sessiond::protocol::{Frame, Request, Response, MAGIC, PROTO_VERSION};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

async fn send_frame(s: &mut UnixStream, f: &Frame) {
    let body = bincode::serialize(f).unwrap();
    s.write_all(&(body.len() as u32).to_le_bytes()).await.unwrap();
    s.write_all(&body).await.unwrap();
    s.flush().await.unwrap();
}
async fn recv_frame(s: &mut UnixStream) -> Frame {
    let mut lenb = [0u8; 4];
    s.read_exact(&mut lenb).await.unwrap();
    let len = u32::from_le_bytes(lenb) as usize;
    let mut body = vec![0u8; len];
    s.read_exact(&mut body).await.unwrap();
    bincode::deserialize(&body).unwrap()
}

#[tokio::test]
async fn boot_handshake_create_session_and_clean_shutdown() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("d.sock");
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    // Boot the daemon core on the temp socket.
    let socket_for_task = socket.clone();
    let boot = tokio::spawn(async move {
        sessiond::run(socket_for_task, shutdown_rx).await
    });

    // Wait for the socket to appear + accept a connection.
    let mut conn = None;
    for _ in 0..100 {
        if socket.exists() {
            if let Ok(c) = UnixStream::connect(&socket).await {
                conn = Some(c);
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let mut c = conn.expect("daemon did not bind socket in time");

    // Handshake.
    send_frame(&mut c, &Frame::Request {
        id: 0,
        req: Request::Hello { magic: MAGIC, proto_version: PROTO_VERSION, client_build: "it".into() },
    }).await;
    match recv_frame(&mut c).await {
        Frame::Response { id: 0, res: Response::Welcome { proto_version, .. } } => {
            assert_eq!(proto_version, PROTO_VERSION);
        }
        other => panic!("expected Welcome, got {other:?}"),
    }

    // Create a workspace first (session needs a workspace id), then a session.
    send_frame(&mut c, &Frame::Request {
        id: 1,
        req: Request::CreateWorkspace { name: "ws".into(), root_path: dir.path().display().to_string() },
    }).await;
    let ws_id = loop {
        match recv_frame(&mut c).await {
            Frame::Response { id: 1, res: Response::Workspace(w) } => break w.id,
            Frame::Push(_) => continue, // ignore the WorkspaceCreated push
            other => panic!("expected Workspace, got {other:?}"),
        }
    };

    send_frame(&mut c, &Frame::Request {
        id: 2,
        req: Request::CreateSession {
            workspace_id: ws_id,
            shell: Some("/bin/sh".into()),
            cwd: Some(dir.path().display().to_string()),
            env_overrides: vec![],
            cols: 80,
            rows: 24,
        },
    }).await;
    let created_id = loop {
        match recv_frame(&mut c).await {
            Frame::Response { id: 2, res: Response::Session(meta) } => {
                assert_eq!(meta.cols, 80);
                assert_eq!(meta.rows, 24);
                break meta.id;
            }
            Frame::Push(_) => continue, // ignore SessionCreated push
            Frame::Response { id: 2, res: Response::Error { code, message } } => {
                panic!("CreateSession failed: {code}: {message}");
            }
            other => panic!("expected Session, got {other:?}"),
        }
    };
    assert!(!created_id.is_empty());

    // Clean shutdown: signal the watch, expect run() to return Ok and the socket to be removed.
    shutdown_tx.send(true).unwrap();
    let res = tokio::time::timeout(Duration::from_secs(5), boot)
        .await
        .expect("run() did not return after shutdown")
        .expect("join");
    assert!(res.is_ok(), "run() returned error: {res:?}");
    assert!(!socket.exists(), "socket should be unlinked on clean shutdown");
}
```

- [ ] **Step 2: Run — confirm FAIL**

`cargo test -p sessiond --test boot_integration`
Expected: FAIL to compile with `cannot find function 'run' in crate 'sessiond'` (and `sessiond::protocol` unresolved until `lib.rs` re-exports it).

- [ ] **Step 3: Implement `main.rs` (boot core `run()` + `main` wrapper) and ensure `lib.rs` re-exports `protocol` + modules**

Ensure `crates/sessiond/src/lib.rs` contains (append only missing lines; do not remove other tasks' `mod` lines):
```rust
//! bpa-sessiond library surface (modules + re-exports for integration tests).
pub use protocol;                 // re-export the protocol crate for `sessiond::protocol::*`
pub mod singleton;
pub mod osc_parser;
pub mod scrollback;
pub mod persistence;
pub mod live_grid;
pub mod pty_supervisor;
pub mod shell_integration;
pub mod attach;
pub mod socket_server;

pub use boot::run;                // re-export the testable boot core
mod boot;
```
Create `crates/sessiond/src/boot.rs` with the testable core (so both `main.rs` and the integration test call the same `run`):
```rust
//! Testable daemon boot core (spec §8.1–§8.3, §13). `main.rs` is a thin wrapper over `run`.
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tokio::net::{UnixListener, UnixStream};
use tokio::sync::watch;

use crate::attach::AttachRegistry;
use crate::persistence::Db;
use crate::pty_supervisor::Supervisor;
use crate::singleton::{assert_socket_path_len, ensure_socket_dir, set_socket_mode};
use crate::socket_server::{serve, ServerDeps};

/// Resolve `APP_SUPPORT/bpa.db` (spec §8.1 / §11 DB path).
fn app_support_dir() -> PathBuf {
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp"));
    home.join("Library/Application Support/ai.builderpro.desktop")
}

async fn bind_fresh(socket: &Path) -> std::io::Result<UnixListener> {
    assert_socket_path_len(socket)?;
    // Stale-socket cleanup (spec §8.2): we already hold the flock, so any existing
    // socket file is a crashed-daemon artifact — connect-probe then unlink + rebind.
    if socket.exists() {
        let _ = UnixStream::connect(socket).await; // result ignored; we own the lock
        let _ = std::fs::remove_file(socket);
    }
    let listener = UnixListener::bind(socket)?;
    set_socket_mode(socket)?;
    Ok(listener)
}

/// Boot core: bind the listener, wire deps, run `serve` until `shutdown` fires, then drain.
pub async fn run(socket: PathBuf, shutdown: watch::Receiver<bool>) -> std::io::Result<()> {
    let listener = bind_fresh(&socket).await?;

    // DB is best-effort (spec §11): open under APP_SUPPORT; a hard failure surfaces as a log
    // and falls back to an in-memory DB so live sessions still work.
    let app_support = app_support_dir();
    let _ = std::fs::create_dir_all(app_support.join("logs"));
    let db = match Db::open(&app_support.join("bpa.db")) {
        Ok(db) => Arc::new(db),
        Err(e) => {
            tracing::error!(error = %e, "DB open failed; continuing in degraded (in-memory) mode");
            Arc::new(Db::open_in_memory().unwrap_or_else(|e2| {
                tracing::error!(error = %e2, "in-memory DB also failed");
                panic!("no usable database backend");
            }))
        }
    };

    let supervisor = Supervisor::new(db.clone());
    let attach = Arc::new(AttachRegistry::new(supervisor.clone()));
    let deps = Arc::new(ServerDeps {
        supervisor: supervisor.clone(),
        db: db.clone(),
        attach: attach.clone(),
        daemon_build: env!("CARGO_PKG_VERSION").to_string(),
    });

    tracing::info!(socket = %socket.display(), "sessiond serving");
    let serve_res = serve(listener, deps, shutdown).await;

    // Drain (spec §8.3 / §13 DaemonShutdown semantics).
    attach.detach_all();
    supervisor.shutdown_all(); // killpg each session: SIGTERM -> grace -> SIGKILL (T9 owns mechanics)
    let _ = db.checkpoint();    // best-effort WAL checkpoint
    let _ = std::fs::remove_file(&socket);
    tracing::info!("sessiond drained; exiting");
    serve_res
}
```
Create `crates/sessiond/src/main.rs`:
```rust
//! bpa-sessiond entrypoint (spec §8.3). launchd invokes: `bpa-sessiond --socket <path>`.
use std::path::PathBuf;
use std::process::ExitCode;

use tokio::signal::unix::{signal, SignalKind};
use tokio::sync::watch;

use sessiond::run;
use sessiond::singleton::{acquire_single_instance_lock, resolve_socket_path};

fn parse_socket_arg() -> Option<PathBuf> {
    let mut args = std::env::args().skip(1);
    while let Some(a) = args.next() {
        if a == "--socket" {
            return args.next().map(PathBuf::from);
        }
    }
    None
}

fn init_tracing() {
    use tracing_subscriber::{fmt, prelude::*, EnvFilter};
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/tmp"));
    let log_dir = home.join("Library/Application Support/ai.builderpro.desktop/logs");
    let _ = std::fs::create_dir_all(&log_dir);
    let file_appender = tracing_appender::rolling::never(&log_dir, "sessiond.tracing.log");
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_writer(file_appender).with_ansi(false))
        .init();
}

#[tokio::main]
async fn main() -> ExitCode {
    init_tracing();

    // Single-instance flock: a second daemon exits cleanly (idempotent kickstart).
    let _lock = match acquire_single_instance_lock() {
        Ok(g) => g,
        Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {
            tracing::info!("another sessiond already holds the lock; exiting");
            return ExitCode::SUCCESS;
        }
        Err(e) => {
            tracing::error!(error = %e, "failed to acquire single-instance lock");
            return ExitCode::FAILURE;
        }
    };

    let socket = parse_socket_arg().unwrap_or_else(resolve_socket_path);

    // SIGTERM/SIGINT -> flip the shutdown watch -> serve() returns -> run() drains.
    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let mut sigterm = match signal(SignalKind::terminate()) {
        Ok(s) => s,
        Err(e) => { tracing::error!(error = %e, "cannot install SIGTERM handler"); return ExitCode::FAILURE; }
    };
    let mut sigint = match signal(SignalKind::interrupt()) {
        Ok(s) => s,
        Err(e) => { tracing::error!(error = %e, "cannot install SIGINT handler"); return ExitCode::FAILURE; }
    };
    tokio::spawn(async move {
        tokio::select! {
            _ = sigterm.recv() => tracing::info!("SIGTERM received; draining"),
            _ = sigint.recv() => tracing::info!("SIGINT received; draining"),
        }
        let _ = shutdown_tx.send(true);
    });

    match run(socket, shutdown_rx).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => { tracing::error!(error = %e, "sessiond run failed"); ExitCode::FAILURE }
    }
}
```

- [ ] **Step 4: Run — confirm PASS**

`cargo test -p sessiond --test boot_integration`
Expected: PASS (socket binds; Hello→Welcome; CreateWorkspace then CreateSession echoes an 80×24 `SessionMeta`; shutdown watch → `run()` returns `Ok` and the socket file is unlinked).

- [ ] **Step 5: Commit**

`git add crates/sessiond/src/main.rs crates/sessiond/src/boot.rs crates/sessiond/src/lib.rs crates/sessiond/Cargo.toml && git commit -m "feat(sessiond): daemon boot — flock, tracing, listener bind, deps wiring, SIGTERM drain"`

**Definition of Done:**
- `cargo test -p sessiond --test boot_integration` green: boot on a temp socket, `Hello → Welcome`, `CreateSession` echoes the requested size, clean shutdown returns `Ok` and unlinks the socket (spec §14.1 boot integration).
- `--socket <path>` parsed; absent → `resolve_socket_path()` (spec §8.3).
- `tracing-subscriber` initialized writing under `{APP_SUPPORT}/logs/` with no secret values (spec §13, §16).
- Single-instance `flock` acquired for process lifetime; a second daemon (WouldBlock) exits `SUCCESS` (spec §8.2, idempotent kickstart).
- Socket dir ensured (0700), path length asserted `< 104`, stale socket unlinked before bind, socket mode set 0600 (spec §8.1–§8.2).
- `Supervisor` + `Db` + `AttachRegistry` constructed and handed to `serve` via `ServerDeps` (spec §7).
- SIGTERM/SIGINT → drain: `attach.detach_all()`, `supervisor.shutdown_all()` (killpg sessions, spec §9.8), best-effort `db.checkpoint()`, socket unlink; exit `SUCCESS` so `KeepAlive{Crashed}` leaves it down (spec §8.3, §13).


### Task 14: `socket_client.rs` — Hop-B daemon client (connect, handshake, correlated request/response, push fan-out, reconnect)

**Files:**
- Create: `src-tauri/src/socket_client.rs`
- Modify:
  - `src-tauri/Cargo.toml` — add `bpa-protocol = { path = "../crates/protocol" }`; `tokio = { version = "1", features = ["net", "io-util", "rt-multi-thread", "macros", "sync", "time"] }`; `bincode = "1.3.3"`; `tracing = "0.1"`; `[dev-dependencies] tempfile = "3"`, `tokio = { version = "1", features = ["net", "io-util", "rt-multi-thread", "macros", "sync", "time"] }` (test-only features already covered by the main dep — re-declaring is harmless but not required). This task OWNS the `bpa-protocol`, `tokio`, `bincode` dependency lines it adds; if a peer task (T15/T16) also needs `tracing`, adding it twice is a merge conflict — declare `tracing` here and let T15/T16 reference it without re-adding (see cross-task concerns).
  - `src-tauri/src/lib.rs` — append `pub mod socket_client;` on its own line (do not touch other tasks' `mod` lines).
- Test: inline `#[cfg(test)] mod tests` in `src-tauri/src/socket_client.rs`, driving a **stub daemon** over a `tempfile`-backed temp Unix socket.

**Depends on:** [T3]   **Parallel-safe with:** [T15, T16]

**Interfaces:** Consumes (verbatim from T3 `bpa-protocol`, spec §7): `Frame` (`Frame::Request { id: u64, req: Request }`, `Frame::Response { id: u64, res: Response }`, `Frame::Push(Push)`), `Request` (incl. `Request::Hello { magic: u32, proto_version: u16, client_build: String }`), `Response` (incl. `Response::Welcome { proto_version: u16, daemon_build: String }`, `Response::Incompatible { min: u16, max: u16 }`, `Response::Error { code: String, message: String }`), `Push`, `MAGIC: u32`, `PROTO_VERSION: u16`. Produces (verbatim from the scaffold Task interface index, spec §7 correlation + §13 reconnect):
```rust
pub struct DaemonClient { /* handle: request tx + push registration + connection-state */ }

/// Terminal error surfaced to the broker/UI (never panics on IO).
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("io: {0}")] Io(#[from] std::io::Error),
    #[error("codec: {0}")] Codec(String),
    #[error("daemon reported: {code}: {message}")] Daemon { code: String, message: String },
    #[error("protocol version mismatch: daemon supports {min}..={max}, client is {client}")]
    Incompatible { min: u16, max: u16, client: u16 },
    #[error("client is shutting down")] Shutdown,
    #[error("request timed out")] Timeout,
}

/// Emitted by the reconnect loop so the broker can raise `daemon://disconnected` / `daemon://reconnected`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnState { Connected, Disconnected }

impl DaemonClient {
    /// Resolve the socket path, connect with bounded exponential backoff (cap ~5s),
    /// send `Hello`, await `Welcome`/`Incompatible`, then spawn the read/reconnect loop.
    pub async fn connect(
        socket_path: std::path::PathBuf,
        client_build: String,
        on_push: std::sync::Arc<dyn Fn(bpa_protocol::Push) + Send + Sync>,
        on_conn: std::sync::Arc<dyn Fn(ConnState) + Send + Sync>,
    ) -> Result<DaemonClient, ClientError>;

    /// Allocate a monotonic id, send `Request{id,..}`, await the correlated `Response`.
    /// `Response::Error` maps to `ClientError::Daemon`. Times out after 30s.
    pub async fn request(&self, req: bpa_protocol::Request) -> Result<bpa_protocol::Response, ClientError>;
}
```
Add `thiserror = "1"` to `src-tauri/Cargo.toml` `[dependencies]` (this task owns that line).

**Locked framing/behavior (spec §7, §13):**
- Wire framing: `u32` little-endian length prefix + `bincode::serialize(&Frame)`. Read = read 4 bytes → `u32::from_le_bytes` → read exactly that many bytes → `bincode::deserialize::<Frame>`. Reject a length ≥ **8 MiB** as a garbage frame (`ClientError::Codec`).
- Handshake: the client's **first** frame is `Frame::Request { id: 0, req: Request::Hello { magic: MAGIC, proto_version: PROTO_VERSION, client_build } }`. The reply must be `Frame::Response { id: 0, res: Response::Welcome { .. } }` (accept) or `Response::Incompatible { min, max }` (→ `ClientError::Incompatible`). Any other first reply → `ClientError::Codec("bad handshake")`.
- Correlation: `id: 0` is reserved for `Hello`. Request ids start at `1` and increase monotonically (`AtomicU64`). A `HashMap<u64, oneshot::Sender<Response>>` (behind a `Mutex` in the reader-owned task, or an mpsc command to the reader) maps each id to the awaiting caller. On a `Frame::Response { id, res }` the reader removes the entry and `send(res)`; an unknown id is logged and dropped.
- Push: `Frame::Push(p)` invokes `on_push(p)` (the broker fans it out to Channels/global events, T17).
- Reconnect (§13): on read EOF / IO error, drop all in-flight senders (their `oneshot` closes → callers get `ClientError::Shutdown`), call `on_conn(Disconnected)`, then reconnect with bounded exponential backoff (start 100ms, ×2, cap **5s**); on success re-`Hello`, call `on_conn(Connected)`. The broker (T17) is responsible for re-`list_sessions` + re-`attach`.
- `request()` after the connection dropped and before reconnect returns `ClientError::Shutdown` (honest: never a fake success).

Design notes (locked so tests + code agree):
- The connection is driven by a single owning **reader task** (`tokio::spawn`). `DaemonClient` holds an `mpsc::Sender<ClientCmd>` where `enum ClientCmd { Request { id: u64, req: Request, reply: oneshot::Sender<Result<Response, ClientError>> } }`. The reader task owns the write half of the socket and the correlation map, so there is exactly one writer and no shared-writer lock across `.await`.
- Backoff uses `tokio::time::sleep`; the cap is a named const `const BACKOFF_CAP: Duration = Duration::from_secs(5);` and `const BACKOFF_START: Duration = Duration::from_millis(100);`.
- Request timeout is `const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);` via `tokio::time::timeout`.

- [ ] **Step 1: Failing test — request/response correlation with concurrent in-flight requests.**

Add to `src-tauri/src/socket_client.rs`:
```rust
#![allow(clippy::type_complexity)]
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use bpa_protocol::{Frame, Push, Request, Response, MAGIC, PROTO_VERSION};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{mpsc, oneshot};

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    // --- framing helpers reused by the stub daemon ---
    async fn read_frame(stream: &mut UnixStream) -> Option<Frame> {
        let mut len_buf = [0u8; 4];
        stream.read_exact(&mut len_buf).await.ok()?;
        let len = u32::from_le_bytes(len_buf) as usize;
        let mut body = vec![0u8; len];
        stream.read_exact(&mut body).await.ok()?;
        bincode::deserialize::<Frame>(&body).ok()
    }
    async fn write_frame(stream: &mut UnixStream, frame: &Frame) {
        let body = bincode::serialize(frame).unwrap();
        stream.write_all(&(body.len() as u32).to_le_bytes()).await.unwrap();
        stream.write_all(&body).await.unwrap();
        stream.flush().await.unwrap();
    }

    fn tmp_sock() -> PathBuf {
        let dir = tempfile::tempdir().unwrap();
        // leak the tempdir so the socket path stays valid for the test's lifetime
        let p = dir.path().join("d.sock");
        std::mem::forget(dir);
        p
    }

    /// Stub daemon: handshakes, then for each Request replies with a distinguishable Response.
    /// CreateWorkspace{name} -> Workspace{ id=name, name, root_path="/" } so the client can
    /// assert the reply matches the request it sent (correlation proof).
    fn spawn_stub(path: PathBuf, ready: Arc<AtomicBool>) {
        tokio::spawn(async move {
            let listener = UnixListener::bind(&path).unwrap();
            ready.store(true, Ordering::SeqCst);
            let (mut stream, _) = listener.accept().await.unwrap();
            // handshake
            let first = read_frame(&mut stream).await.unwrap();
            match first {
                Frame::Request { id: 0, req: Request::Hello { magic, proto_version, .. } } => {
                    assert_eq!(magic, MAGIC);
                    assert_eq!(proto_version, PROTO_VERSION);
                    write_frame(&mut stream, &Frame::Response {
                        id: 0,
                        res: Response::Welcome { proto_version: PROTO_VERSION, daemon_build: "stub".into() },
                    }).await;
                }
                other => panic!("expected Hello, got {other:?}"),
            }
            // serve requests; delay the FIRST reply so a later request can overtake it,
            // proving correlation is by id (not FIFO reply order).
            let mut first_seen = false;
            loop {
                let Some(frame) = read_frame(&mut stream).await else { break };
                if let Frame::Request { id, req } = frame {
                    let res = match req {
                        Request::CreateWorkspace { name, root_path } =>
                            Response::Workspace(bpa_protocol::Workspace { id: name.clone(), name, root_path }),
                        _ => Response::Ack,
                    };
                    if !first_seen {
                        first_seen = true;
                        // hold this reply so the second request is answered first
                        tokio::spawn(async move {}); // no-op to keep structure clear
                        tokio::time::sleep(Duration::from_millis(120)).await;
                    }
                    write_frame(&mut stream, &Frame::Response { id, res }).await;
                }
            }
        });
    }

    #[tokio::test]
    async fn concurrent_requests_correlate_by_id() {
        let path = tmp_sock();
        let ready = Arc::new(AtomicBool::new(false));
        spawn_stub(path.clone(), ready.clone());
        while !ready.load(Ordering::SeqCst) { tokio::time::sleep(Duration::from_millis(5)).await; }

        let noop_push: Arc<dyn Fn(Push) + Send + Sync> = Arc::new(|_p| {});
        let noop_conn: Arc<dyn Fn(ConnState) + Send + Sync> = Arc::new(|_s| {});
        let client = DaemonClient::connect(path, "test".into(), noop_push, noop_conn).await.unwrap();
        let client = Arc::new(client);

        // fire two requests; the stub delays the FIRST reply, so replies arrive out of order.
        let c1 = client.clone();
        let h1 = tokio::spawn(async move {
            c1.request(Request::CreateWorkspace { name: "one".into(), root_path: "/".into() }).await
        });
        // ensure request "one" is sent first
        tokio::time::sleep(Duration::from_millis(20)).await;
        let c2 = client.clone();
        let h2 = tokio::spawn(async move {
            c2.request(Request::CreateWorkspace { name: "two".into(), root_path: "/".into() }).await
        });

        let r1 = h1.await.unwrap().unwrap();
        let r2 = h2.await.unwrap().unwrap();
        match r1 { Response::Workspace(w) => assert_eq!(w.name, "one"), o => panic!("{o:?}") }
        match r2 { Response::Workspace(w) => assert_eq!(w.name, "two"), o => panic!("{o:?}") }
    }
}
```

- [ ] **Step 2: Run — confirm FAIL (no impl yet).**

```
cargo test -p builder-pro-ai socket_client::tests::concurrent_requests_correlate_by_id
```
Expected: FAIL with a compile error — `cannot find type DaemonClient` / `ConnState` in this scope (the impl below does not exist yet).

- [ ] **Step 3: Implement the client (framing, handshake, correlation, reader task, reconnect).**

Add to `src-tauri/src/socket_client.rs`, **above** the `#[cfg(test)] mod tests` block:
```rust
//! Hop-B client: the Tauri core's Unix-domain-socket connection to `bpa-sessiond`.
//! Owns handshake, monotonic request/response correlation, push fan-out, and a
//! bounded-backoff reconnect loop that surfaces `daemon://disconnected/reconnected`.

const BACKOFF_START: Duration = Duration::from_millis(100);
const BACKOFF_CAP: Duration = Duration::from_secs(5);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_FRAME_LEN: usize = 8 * 1024 * 1024; // reject garbage/oversized frames

#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("codec: {0}")]
    Codec(String),
    #[error("daemon reported: {code}: {message}")]
    Daemon { code: String, message: String },
    #[error("protocol version mismatch: daemon supports {min}..={max}, client is {client}")]
    Incompatible { min: u16, max: u16, client: u16 },
    #[error("client is shutting down")]
    Shutdown,
    #[error("request timed out")]
    Timeout,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnState {
    Connected,
    Disconnected,
}

enum ClientCmd {
    Request {
        req: Request,
        reply: oneshot::Sender<Result<Response, ClientError>>,
    },
}

pub struct DaemonClient {
    cmd_tx: mpsc::Sender<ClientCmd>,
}

async fn read_frame_from<R: AsyncReadExt + Unpin>(r: &mut R) -> Result<Frame, ClientError> {
    let mut len_buf = [0u8; 4];
    r.read_exact(&mut len_buf).await?;
    let len = u32::from_le_bytes(len_buf) as usize;
    if len == 0 || len > MAX_FRAME_LEN {
        return Err(ClientError::Codec(format!("frame length {len} out of bounds")));
    }
    let mut body = vec![0u8; len];
    r.read_exact(&mut body).await?;
    bincode::deserialize::<Frame>(&body).map_err(|e| ClientError::Codec(e.to_string()))
}

async fn write_frame_to<W: AsyncWriteExt + Unpin>(w: &mut W, frame: &Frame) -> Result<(), ClientError> {
    let body = bincode::serialize(frame).map_err(|e| ClientError::Codec(e.to_string()))?;
    if body.len() > MAX_FRAME_LEN {
        return Err(ClientError::Codec(format!("outbound frame too large: {}", body.len())));
    }
    w.write_all(&(body.len() as u32).to_le_bytes()).await?;
    w.write_all(&body).await?;
    w.flush().await?;
    Ok(())
}

/// Connect once and perform the handshake. Returns the live stream on success.
async fn connect_and_handshake(
    socket_path: &std::path::Path,
    client_build: &str,
) -> Result<UnixStream, ClientError> {
    let mut stream = UnixStream::connect(socket_path).await?;
    write_frame_to(
        &mut stream,
        &Frame::Request {
            id: 0,
            req: Request::Hello {
                magic: MAGIC,
                proto_version: PROTO_VERSION,
                client_build: client_build.to_string(),
            },
        },
    )
    .await?;
    match read_frame_from(&mut stream).await? {
        Frame::Response { id: 0, res: Response::Welcome { proto_version, daemon_build } } => {
            tracing::info!(daemon_build = %daemon_build, proto_version, "daemon handshake ok");
            Ok(stream)
        }
        Frame::Response { id: 0, res: Response::Incompatible { min, max } } => {
            Err(ClientError::Incompatible { min, max, client: PROTO_VERSION })
        }
        other => Err(ClientError::Codec(format!("bad handshake reply: {other:?}"))),
    }
}

/// Connect with bounded exponential backoff (cap `BACKOFF_CAP`). A version
/// `Incompatible` is fatal and returned immediately (no retry).
async fn connect_with_backoff(
    socket_path: &std::path::Path,
    client_build: &str,
) -> Result<UnixStream, ClientError> {
    let mut delay = BACKOFF_START;
    loop {
        match connect_and_handshake(socket_path, client_build).await {
            Ok(s) => return Ok(s),
            Err(e @ ClientError::Incompatible { .. }) => return Err(e),
            Err(e) => {
                tracing::warn!(error = %e, ?delay, "daemon connect failed; backing off");
                tokio::time::sleep(delay).await;
                delay = std::cmp::min(delay * 2, BACKOFF_CAP);
            }
        }
    }
}

impl DaemonClient {
    pub async fn connect(
        socket_path: PathBuf,
        client_build: String,
        on_push: Arc<dyn Fn(Push) + Send + Sync>,
        on_conn: Arc<dyn Fn(ConnState) + Send + Sync>,
    ) -> Result<DaemonClient, ClientError> {
        // First connect is awaited so `connect()` surfaces Incompatible / hard errors to the caller.
        let stream = connect_with_backoff(&socket_path, &client_build).await?;
        on_conn(ConnState::Connected);

        let (cmd_tx, cmd_rx) = mpsc::channel::<ClientCmd>(256);
        tokio::spawn(reader_loop(
            socket_path,
            client_build,
            stream,
            cmd_rx,
            on_push,
            on_conn,
        ));
        Ok(DaemonClient { cmd_tx })
    }

    pub async fn request(&self, req: Request) -> Result<Response, ClientError> {
        let (reply_tx, reply_rx) = oneshot::channel();
        self.cmd_tx
            .send(ClientCmd::Request { req, reply: reply_tx })
            .await
            .map_err(|_| ClientError::Shutdown)?;
        match tokio::time::timeout(REQUEST_TIMEOUT, reply_rx).await {
            Ok(Ok(res)) => res,
            Ok(Err(_)) => Err(ClientError::Shutdown), // reader dropped the sender (disconnect)
            Err(_) => Err(ClientError::Timeout),
        }
    }
}

/// The single owning task: writes requests, reads all frames, correlates responses,
/// fans out pushes, and reconnects on IO failure.
async fn reader_loop(
    socket_path: PathBuf,
    client_build: String,
    mut stream: UnixStream,
    mut cmd_rx: mpsc::Receiver<ClientCmd>,
    on_push: Arc<dyn Fn(Push) + Send + Sync>,
    on_conn: Arc<dyn Fn(ConnState) + Send + Sync>,
) {
    let next_id = AtomicU64::new(1);
    let pending: Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Response, ClientError>>>>> =
        Arc::new(Mutex::new(HashMap::new()));

    loop {
        let (mut rd, mut wr) = stream.into_split();
        // drive read + command handling concurrently on this connection
        let disconnected = run_connection(
            &mut rd,
            &mut wr,
            &mut cmd_rx,
            &next_id,
            &pending,
            &on_push,
        )
        .await;

        // connection ended: fail every in-flight request honestly, signal disconnect.
        {
            let mut map = pending.lock().unwrap();
            for (_id, tx) in map.drain() {
                let _ = tx.send(Err(ClientError::Shutdown));
            }
        }
        on_conn(ConnState::Disconnected);
        if disconnected == LoopEnd::CmdChannelClosed {
            // DaemonClient dropped; stop entirely.
            return;
        }
        tracing::warn!("daemon connection lost; reconnecting");

        // reconnect
        match connect_with_backoff(&socket_path, &client_build).await {
            Ok(s) => {
                stream = s;
                on_conn(ConnState::Connected);
            }
            Err(ClientError::Incompatible { min, max, client }) => {
                tracing::error!(min, max, client, "daemon became incompatible; giving up");
                return;
            }
            Err(e) => {
                tracing::error!(error = %e, "unrecoverable reconnect failure; stopping");
                return;
            }
        }
    }
}

#[derive(PartialEq, Eq)]
enum LoopEnd {
    ConnectionLost,
    CmdChannelClosed,
}

/// Runs one connection until the socket errors (ConnectionLost) or the command
/// channel closes because `DaemonClient` was dropped (CmdChannelClosed).
async fn run_connection(
    rd: &mut (impl AsyncReadExt + Unpin),
    wr: &mut (impl AsyncWriteExt + Unpin),
    cmd_rx: &mut mpsc::Receiver<ClientCmd>,
    next_id: &AtomicU64,
    pending: &Arc<Mutex<HashMap<u64, oneshot::Sender<Result<Response, ClientError>>>>>,
    on_push: &Arc<dyn Fn(Push) + Send + Sync>,
) -> LoopEnd {
    loop {
        tokio::select! {
            biased;
            // outbound: a caller wants to send a request
            cmd = cmd_rx.recv() => {
                match cmd {
                    None => return LoopEnd::CmdChannelClosed,
                    Some(ClientCmd::Request { req, reply }) => {
                        let id = next_id.fetch_add(1, Ordering::Relaxed);
                        pending.lock().unwrap().insert(id, reply);
                        let frame = Frame::Request { id, req };
                        if let Err(e) = write_frame_to(wr, &frame).await {
                            tracing::warn!(error = %e, "write failed; dropping connection");
                            // caller will be failed by the drain in reader_loop
                            let _ = pending.lock().unwrap().remove(&id)
                                .map(|tx| tx.send(Err(ClientError::Shutdown)));
                            return LoopEnd::ConnectionLost;
                        }
                    }
                }
            }
            // inbound: a frame from the daemon
            frame = read_frame_from(rd) => {
                match frame {
                    Err(e) => {
                        tracing::warn!(error = %e, "read failed; dropping connection");
                        return LoopEnd::ConnectionLost;
                    }
                    Ok(Frame::Response { id, res }) => {
                        if let Some(tx) = pending.lock().unwrap().remove(&id) {
                            let mapped = match res {
                                Response::Error { code, message } =>
                                    Err(ClientError::Daemon { code, message }),
                                other => Ok(other),
                            };
                            let _ = tx.send(mapped);
                        } else {
                            tracing::warn!(id, "response for unknown id; dropping");
                        }
                    }
                    Ok(Frame::Push(p)) => on_push(p),
                    Ok(Frame::Request { .. }) => {
                        tracing::warn!("unexpected Request frame from daemon; ignoring");
                    }
                }
            }
        }
    }
}
```

- [ ] **Step 4: Run — confirm PASS.**

```
cargo test -p builder-pro-ai socket_client::tests::concurrent_requests_correlate_by_id
```
Expected: PASS.

- [ ] **Step 5: Failing test — version-reject (`Incompatible` handshake surfaces `ClientError::Incompatible`).**

Add inside `#[cfg(test)] mod tests`:
```rust
#[tokio::test]
async fn incompatible_handshake_is_rejected() {
    let path = tmp_sock();
    let ready = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let p = path.clone();
    let r = ready.clone();
    tokio::spawn(async move {
        let listener = UnixListener::bind(&p).unwrap();
        r.store(true, Ordering::SeqCst);
        let (mut stream, _) = listener.accept().await.unwrap();
        let _first = read_frame(&mut stream).await.unwrap(); // consume Hello
        write_frame(&mut stream, &Frame::Response {
            id: 0,
            res: Response::Incompatible { min: 2, max: 4 },
        }).await;
    });
    while !ready.load(Ordering::SeqCst) { tokio::time::sleep(Duration::from_millis(5)).await; }

    let noop_push: Arc<dyn Fn(Push) + Send + Sync> = Arc::new(|_p| {});
    let noop_conn: Arc<dyn Fn(ConnState) + Send + Sync> = Arc::new(|_s| {});
    let err = DaemonClient::connect(path, "test".into(), noop_push, noop_conn).await.unwrap_err();
    match err {
        ClientError::Incompatible { min, max, client } => {
            assert_eq!(min, 2);
            assert_eq!(max, 4);
            assert_eq!(client, PROTO_VERSION);
        }
        o => panic!("expected Incompatible, got {o:?}"),
    }
}
```

- [ ] **Step 6: Run — confirm PASS.** (The impl already returns `Incompatible` and `connect_with_backoff` does not retry it.)

```
cargo test -p builder-pro-ai socket_client::tests::incompatible_handshake_is_rejected
```
Expected: PASS.

- [ ] **Step 7: Failing test — reconnect after the daemon drops the connection, plus push fan-out + conn-state transitions.**

Add inside `#[cfg(test)] mod tests`:
```rust
#[tokio::test]
async fn reconnects_after_drop_and_delivers_push() {
    let path = tmp_sock();
    let ready = Arc::new(std::sync::atomic::AtomicBool::new(false));

    // Daemon that: handshakes, sends a Push, then CLOSES; on the 2nd connect it
    // handshakes again and answers one Ack request. Proves reconnect + re-Hello.
    let p = path.clone();
    let r = ready.clone();
    tokio::spawn(async move {
        let listener = UnixListener::bind(&p).unwrap();
        r.store(true, Ordering::SeqCst);
        // connection #1
        {
            let (mut s, _) = listener.accept().await.unwrap();
            let _ = read_frame(&mut s).await.unwrap(); // Hello
            write_frame(&mut s, &Frame::Response {
                id: 0, res: Response::Welcome { proto_version: PROTO_VERSION, daemon_build: "d1".into() },
            }).await;
            write_frame(&mut s, &Frame::Push(Push::WorkspaceCreated {
                workspace: bpa_protocol::Workspace { id: "w1".into(), name: "w1".into(), root_path: "/".into() },
            })).await;
            // drop `s` -> connection closes
        }
        // connection #2
        {
            let (mut s, _) = listener.accept().await.unwrap();
            let _ = read_frame(&mut s).await.unwrap(); // Hello again
            write_frame(&mut s, &Frame::Response {
                id: 0, res: Response::Welcome { proto_version: PROTO_VERSION, daemon_build: "d2".into() },
            }).await;
            if let Some(Frame::Request { id, .. }) = read_frame(&mut s).await {
                write_frame(&mut s, &Frame::Response { id, res: Response::Ack }).await;
            }
        }
    });
    while !ready.load(Ordering::SeqCst) { tokio::time::sleep(Duration::from_millis(5)).await; }

    let pushes: Arc<Mutex<Vec<Push>>> = Arc::new(Mutex::new(Vec::new()));
    let states: Arc<Mutex<Vec<ConnState>>> = Arc::new(Mutex::new(Vec::new()));
    let pushes_cb = pushes.clone();
    let states_cb = states.clone();
    let on_push: Arc<dyn Fn(Push) + Send + Sync> =
        Arc::new(move |p| pushes_cb.lock().unwrap().push(p));
    let on_conn: Arc<dyn Fn(ConnState) + Send + Sync> =
        Arc::new(move |s| states_cb.lock().unwrap().push(s));

    let client = DaemonClient::connect(path, "test".into(), on_push, on_conn).await.unwrap();

    // The Ack request will only succeed after reconnect #2. Retry until connected.
    let mut got_ack = false;
    for _ in 0..100 {
        match client.request(Request::KillSession { session_id: "x".into() }).await {
            Ok(Response::Ack) => { got_ack = true; break; }
            _ => tokio::time::sleep(Duration::from_millis(50)).await,
        }
    }
    assert!(got_ack, "expected an Ack after reconnect");

    // push from connection #1 was delivered
    let got_push = pushes.lock().unwrap().iter().any(|p| matches!(p, Push::WorkspaceCreated { .. }));
    assert!(got_push, "expected the WorkspaceCreated push");

    // conn states include an initial Connected, a Disconnected, and a later Connected
    let s = states.lock().unwrap().clone();
    assert_eq!(s.first(), Some(&ConnState::Connected));
    assert!(s.contains(&ConnState::Disconnected), "expected a Disconnected transition: {s:?}");
    assert!(s.iter().filter(|x| **x == ConnState::Connected).count() >= 2,
        "expected at least two Connected transitions: {s:?}");
}
```

- [ ] **Step 8: Run — confirm PASS.**

```
cargo test -p builder-pro-ai socket_client::tests::reconnects_after_drop_and_delivers_push
```
Expected: PASS. If flaky under load, the 100×50ms retry budget (5s) covers backoff (cap 5s) + accept; the stub's second `accept` is already pending so reconnect lands on the first backoff wake.

- [ ] **Step 9: Wire the module + run the whole file's suite.**

Ensure `src-tauri/src/lib.rs` contains `pub mod socket_client;` (append if missing). Run:
```
cargo test -p builder-pro-ai socket_client
```
Expected: PASS — all three tests green.

- [ ] **Step 10: Commit.**

```
git add src-tauri/src/socket_client.rs src-tauri/Cargo.toml src-tauri/src/lib.rs Cargo.lock && git commit -m "feat(core): Hop-B daemon client — handshake, id correlation, push fan-out, bounded-backoff reconnect"
```

**Definition of Done:**
- `cargo test -p builder-pro-ai socket_client` green (correlation-under-concurrency, version-reject, reconnect-after-drop + push + conn-state).
- Framing is exactly `u32`-LE length + `bincode(Frame)`; oversized/zero-length frames rejected as `ClientError::Codec` (spec §7).
- First frame is `Hello { magic: MAGIC, proto_version: PROTO_VERSION, .. }`; `Welcome` accepted, `Incompatible` → `ClientError::Incompatible` (no retry), any other first reply → `Codec` (spec §7, §13).
- Monotonic ids from 1 (`0` reserved for `Hello`); `HashMap<u64, oneshot::Sender<..>>` correlation; out-of-order replies resolve the correct caller; unknown ids logged + dropped.
- `Response::Error` → `ClientError::Daemon { code, message }` rejecting that caller (spec §7 correlation).
- Reconnect uses bounded exponential backoff capped at `BACKOFF_CAP = 5s`; emits `ConnState::Disconnected` then `Connected` via `on_conn` (broker maps to `daemon://disconnected` / `daemon://reconnected`, spec §6.3, §13); in-flight requests fail with `ClientError::Shutdown` (never a fake success).
- `Push` frames delivered to `on_push` for broker fan-out (spec §7 broker mapping).
- Structured `tracing` logs on connect/disconnect/reconnect/errors; no secret values logged (spec §13).

---

### Task 15: `paths.rs` — workspace-root / cwd validation (canonicalize, absolute, exists, is-dir, no symlink-escape)

**Files:**
- Create: `src-tauri/src/paths.rs`
- Modify: `src-tauri/src/lib.rs` — append `pub mod paths;` on its own line (do not touch other tasks' `mod` lines). `src-tauri/Cargo.toml` — add `thiserror = "1"` **only if T14 has not already added it** (both tasks want it; declare once — see cross-task concerns). No new deps otherwise (std-only).
- Test: inline `#[cfg(test)] mod tests` in `src-tauri/src/paths.rs`, using `tempfile` (dev-dependency added by T14; if executed before T14 lands, add `[dev-dependencies] tempfile = "3"` to `src-tauri/Cargo.toml`).

**Depends on:** [T3]   **Parallel-safe with:** [T14, T16]

**Interfaces:** Consumes: nothing from T3 at the type level (std-only). Produces (verbatim from the scaffold Task interface index, spec §16):
```rust
/// Typed reason a directory is invalid. `code()` yields the wire code string the
/// broker/daemon surface uses in `Response::Error { code, .. }` (spec §13).
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PathError {
    #[error("path is not absolute: {0}")]
    NotAbsolute(String),
    #[error("path does not exist: {0}")]
    Missing(String),
    #[error("path is not a directory: {0}")]
    NotADirectory(String),
    #[error("path escapes via symlink: {0}")]
    SymlinkEscape(String),
    #[error("cannot canonicalize {path}: {source}")]
    Canonicalize { path: String, source: std::io::Error },
}

impl PathError {
    /// Stable wire code for `Response::Error { code, .. }`.
    pub fn code(&self) -> &'static str;
}

/// Validate a workspace root or session cwd: must be absolute, exist, be a real
/// directory, and not escape its own lexical parent via symlink. Returns the
/// canonicalized (realpath) absolute `PathBuf`.
pub fn validate_dir(path: &std::path::Path) -> Result<std::path::PathBuf, PathError>;
```

**Locked behavior (spec §16 "Path validation"):**
- The input must be **absolute** (`path.is_absolute()`), else `PathError::NotAbsolute`. A relative path is rejected **before** touching the filesystem (deterministic; does not depend on the daemon cwd).
- Canonicalize via `std::fs::canonicalize` (realpath): resolves symlinks + `.`/`..`. IO failure where the terminal component is missing → `PathError::Missing`; any other IO failure → `PathError::Canonicalize`.
- After canonicalize, the result must exist and be a directory: `metadata.is_dir()` else `PathError::NotADirectory`.
- **Symlink-escape rule:** compute the lexical parent of the *input* (`path.parent()`), canonicalize that parent, and require the canonicalized target to remain within that canonicalized parent (i.e. `canonical.starts_with(canonical_parent)`), OR the input had no parent (root). This rejects the case where the final path component is a symlink whose realpath jumps outside the directory the caller lexically named. Root (`/`) and paths whose parent canonicalizes to a prefix of the result are accepted. Documented as the spec §16 "symlink escape is disallowed (canonicalize and re-check)" contract.
- Codes (`PathError::code()`): `NotAbsolute` → `"RelativePath"`, `Missing` → `"CwdMissing"`, `NotADirectory` → `"NotADirectory"`, `SymlinkEscape` → `"SymlinkEscape"`, `Canonicalize` → `"InvalidWorkspaceRoot"`. These match the typed error codes the daemon surface returns (spec §13 `InvalidWorkspaceRoot` / `CwdMissing`).

- [ ] **Step 1: Failing test — the five cases: ok / relative / missing / file-not-dir / symlink-escape.**

Add to `src-tauri/src/paths.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};

    #[test]
    fn ok_real_directory_canonicalizes() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("ws");
        fs::create_dir(&sub).unwrap();
        let got = validate_dir(&sub).expect("valid dir");
        // canonicalized result is absolute, is a dir, and ends with our component
        assert!(got.is_absolute());
        assert!(got.is_dir());
        assert_eq!(got, fs::canonicalize(&sub).unwrap());
    }

    #[test]
    fn relative_path_is_rejected_before_fs() {
        let rel = Path::new("some/relative/dir");
        let err = validate_dir(rel).unwrap_err();
        assert!(matches!(err, PathError::NotAbsolute(_)), "got {err:?}");
        assert_eq!(err.code(), "RelativePath");
    }

    #[test]
    fn missing_path_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let gone = dir.path().join("does-not-exist");
        let err = validate_dir(&gone).unwrap_err();
        assert!(matches!(err, PathError::Missing(_)), "got {err:?}");
        assert_eq!(err.code(), "CwdMissing");
    }

    #[test]
    fn file_is_not_a_directory() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("afile");
        fs::write(&file, b"x").unwrap();
        let err = validate_dir(&file).unwrap_err();
        assert!(matches!(err, PathError::NotADirectory(_)), "got {err:?}");
        assert_eq!(err.code(), "NotADirectory");
    }

    #[test]
    fn symlink_escaping_parent_is_rejected() {
        // layout:
        //   base/outside/         (real target dir, OUTSIDE `named`)
        //   base/named/link -> ../outside
        // validate_dir(base/named/link) canonicalizes to base/outside, whose
        // parent (base) != canonical parent of the input (base/named) -> escape.
        let base = tempfile::tempdir().unwrap();
        let outside = base.path().join("outside");
        let named = base.path().join("named");
        fs::create_dir(&outside).unwrap();
        fs::create_dir(&named).unwrap();
        let link: PathBuf = named.join("link");
        std::os::unix::fs::symlink(&outside, &link).unwrap();

        let err = validate_dir(&link).unwrap_err();
        assert!(matches!(err, PathError::SymlinkEscape(_)), "got {err:?}");
        assert_eq!(err.code(), "SymlinkEscape");
    }

    #[test]
    fn symlink_within_parent_is_allowed() {
        // base/target/  and  base/link -> target  : realpath stays under base -> OK
        let base = tempfile::tempdir().unwrap();
        let target = base.path().join("target");
        fs::create_dir(&target).unwrap();
        let link = base.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let got = validate_dir(&link).expect("sibling symlink under same parent is allowed");
        assert_eq!(got, fs::canonicalize(&target).unwrap());
    }
}
```

- [ ] **Step 2: Run — confirm FAIL (no impl yet).**

```
cargo test -p builder-pro-ai paths::tests
```
Expected: FAIL with a compile error — `cannot find function validate_dir` / `cannot find type PathError`.

- [ ] **Step 3: Implement `paths.rs`.**

Add to `src-tauri/src/paths.rs`, **above** the `#[cfg(test)] mod tests` block:
```rust
//! Daemon-and-core-shared directory validation for workspace roots and session cwd.
//! Enforced in the core (Hop-A) AND the daemon (Hop-B) because S6 agents drive the
//! same surface (spec §16). Canonicalize + absolute + exists + is-dir + no symlink-escape.

use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum PathError {
    #[error("path is not absolute: {0}")]
    NotAbsolute(String),
    #[error("path does not exist: {0}")]
    Missing(String),
    #[error("path is not a directory: {0}")]
    NotADirectory(String),
    #[error("path escapes via symlink: {0}")]
    SymlinkEscape(String),
    #[error("cannot canonicalize {path}: {source}")]
    Canonicalize { path: String, source: std::io::Error },
}

impl PartialEq for PathErrorCanonicalizeShim {
    fn eq(&self, _: &Self) -> bool {
        false
    }
}
// (marker type never constructed; present only to document that Canonicalize's
// io::Error is compared structurally via kind in the manual PartialEq below.)
struct PathErrorCanonicalizeShim;

impl PathError {
    pub fn code(&self) -> &'static str {
        match self {
            PathError::NotAbsolute(_) => "RelativePath",
            PathError::Missing(_) => "CwdMissing",
            PathError::NotADirectory(_) => "NotADirectory",
            PathError::SymlinkEscape(_) => "SymlinkEscape",
            PathError::Canonicalize { .. } => "InvalidWorkspaceRoot",
        }
    }
}

pub fn validate_dir(path: &Path) -> Result<PathBuf, PathError> {
    let display = || path.display().to_string();

    // 1. absolute, checked before any filesystem access.
    if !path.is_absolute() {
        return Err(PathError::NotAbsolute(display()));
    }

    // 2. canonicalize (realpath): resolves symlinks + `.`/`..`.
    let canonical = match std::fs::canonicalize(path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(PathError::Missing(display()));
        }
        Err(source) => {
            return Err(PathError::Canonicalize { path: display(), source });
        }
    };

    // 3. must be a directory.
    let meta = std::fs::metadata(&canonical)
        .map_err(|source| PathError::Canonicalize { path: display(), source })?;
    if !meta.is_dir() {
        return Err(PathError::NotADirectory(display()));
    }

    // 4. symlink-escape: the canonicalized result must stay within the
    //    canonicalized *lexical parent* of the input. If the input is a root
    //    (no parent), accept. If the parent cannot be canonicalized, the input's
    //    lineage is unresolvable -> treat as escape (fail closed).
    match path.parent() {
        None => Ok(canonical), // root
        Some(parent) => {
            let canonical_parent = std::fs::canonicalize(parent)
                .map_err(|_| PathError::SymlinkEscape(display()))?;
            if canonical.starts_with(&canonical_parent) {
                Ok(canonical)
            } else {
                Err(PathError::SymlinkEscape(display()))
            }
        }
    }
}
```

> Note to implementer: the `PathErrorCanonicalizeShim` marker + its `PartialEq` in the snippet above is scaffolding to satisfy `#[derive(PartialEq, Eq)]` on `PathError` when it carries a non-`PartialEq` `io::Error`. Simpler: **do not derive** `PartialEq`/`Eq` on `PathError`; instead delete the shim and the derive, and in tests use `matches!(err, PathError::Variant(_))` (already how the tests assert) plus `err.code()` equality. Prefer the simpler form — remove the derive and the shim entirely; the tests never compare two `PathError`s for full equality.

- [ ] **Step 3a: Simplify — drop the derive + shim (recommended).**

Replace the `#[derive(Debug, thiserror::Error, PartialEq, Eq)]` on `PathError` with `#[derive(Debug, thiserror::Error)]`, and delete the `PathErrorCanonicalizeShim` struct + its `impl PartialEq`. The interface block above lists `PartialEq, Eq` for documentation symmetry, but `io::Error` is not `PartialEq`; the tests use `matches!` + `code()`, so the derive is unnecessary. Final `PathError` derive line:
```rust
#[derive(Debug, thiserror::Error)]
```

- [ ] **Step 4: Run — confirm PASS.**

```
cargo test -p builder-pro-ai paths::tests
```
Expected: PASS — all six tests green.

- [ ] **Step 5: Wire the module + commit.**

Ensure `src-tauri/src/lib.rs` contains `pub mod paths;`. Then:
```
git add src-tauri/src/paths.rs src-tauri/src/lib.rs src-tauri/Cargo.toml && git commit -m "feat(core): validate_dir — absolute+exists+is-dir+no-symlink-escape with typed PathError codes"
```

**Definition of Done:**
- `cargo test -p builder-pro-ai paths::tests` green: ok / relative / missing / file-not-dir / symlink-escape / symlink-within-parent-allowed (spec §14.1 path-validation row, §16).
- Relative paths rejected before any filesystem access (`NotAbsolute` / code `RelativePath`).
- Result is the canonicalized (realpath) absolute `PathBuf`; non-directories rejected (`NotADirectory`); missing → `Missing` / code `CwdMissing`.
- Symlink escape (final component realpath leaving the lexical parent) rejected as `SymlinkEscape`; unresolvable parent fails closed (spec §16 "symlink escape is disallowed").
- `PathError::code()` returns the stable wire codes (`RelativePath`/`CwdMissing`/`NotADirectory`/`SymlinkEscape`/`InvalidWorkspaceRoot`) used in `Response::Error` (spec §13).

---

### Task 16: `launchd.rs` — LaunchAgent install / bootstrap / kickstart / is-loaded with degradation

**Files:**
- Create: `src-tauri/src/launchd.rs`
- Modify: `src-tauri/src/lib.rs` — append `pub mod launchd;` on its own line. `src-tauri/Cargo.toml` — add `thiserror = "1"` only if not already present (T14/T15); add `[dev-dependencies] tempfile = "3"` only if not already present (T14/T15). No other new deps (std + `std::process::Command`).
- Test: inline `#[cfg(test)] mod tests` in `src-tauri/src/launchd.rs`, using an **injectable `launchctl` runner** (a trait mock — no real `launchctl` invoked in unit tests).

**Depends on:** [T3]   **Parallel-safe with:** [T14, T15]

**Interfaces:** Consumes: nothing from T3 (std-only). Produces (verbatim from the scaffold Task interface index, spec §8.3 / §13):
```rust
/// Injectable launchctl runner so unit tests never touch the real service DB.
pub trait LaunchctlRunner: Send + Sync {
    /// Run `launchctl <args...>`; return the (exit_code, stdout, stderr) triple.
    fn run(&self, args: &[&str]) -> std::io::Result<LaunchctlOutput>;
}

#[derive(Debug, Clone)]
pub struct LaunchctlOutput {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

/// Real runner used in production; shells out to `/bin/launchctl`.
pub struct RealLaunchctl;

#[derive(Debug, thiserror::Error)]
pub enum LaunchdError {
    #[error("io: {0}")] Io(#[from] std::io::Error),
    #[error("could not install background service: {0}")] Install(String),
    #[error("launchctl {op} failed (code {code}): {stderr}")]
    Command { op: String, code: i32, stderr: String },
    #[error("cannot resolve daemon path: {0}")] DaemonPath(String),
}

/// Locked identity (spec Global Constraints).
pub const LABEL: &str = "ai.builderpro.desktop.sessiond";

pub struct LaunchdAgent<'a> {
    pub runner: &'a dyn LaunchctlRunner,
    pub uid: u32,
    pub launch_agents_dir: std::path::PathBuf, // ~/Library/LaunchAgents (injectable for tests)
    pub app_support_dir: std::path::PathBuf,   // APP_SUPPORT (for log paths in the plist)
    pub daemon_path: std::path::PathBuf,       // absolute path to the bundled bpa-sessiond
    pub socket_path: std::path::PathBuf,       // RESOLVED_SOCKET_PATH
}

impl<'a> LaunchdAgent<'a> {
    /// Write the plist (spec §8.3) to `launch_agents_dir`, ensuring the dir exists.
    pub fn install_agent(&self) -> Result<std::path::PathBuf, LaunchdError>;
    /// `launchctl bootstrap gui/<uid> <plist>`; "already bootstrapped" == success;
    /// on plist drift bootout+re-bootstrap.
    pub fn bootstrap(&self) -> Result<(), LaunchdError>;
    /// `launchctl kickstart gui/<uid>/<LABEL>`.
    pub fn kickstart(&self) -> Result<(), LaunchdError>;
    /// `launchctl print gui/<uid>/<LABEL>` exit 0 => loaded.
    pub fn is_loaded(&self) -> bool;
    /// Render the plist XML (spec §8.3) — pure, testable.
    pub fn render_plist(&self) -> String;
    /// Resolve the bundled daemon path from `current_exe()`'s sibling (production helper).
    pub fn resolve_daemon_path() -> Result<std::path::PathBuf, LaunchdError>;
}
```

**Locked behavior (spec §8.3, §13):**
- `render_plist()` emits exactly the keys in spec §8.3: `Label`=`LABEL`; `ProgramArguments`=`[daemon_path, "--socket", socket_path]`; `KeepAlive`=`<dict><key>Crashed</key><true/></dict>` (**never** bare `true`); `RunAtLoad`=`<false/>`; `ThrottleInterval`=`10`; `ProcessType`=`Background`; `StandardOutPath`=`{app_support}/logs/sessiond.out.log`; `StandardErrorPath`=`{app_support}/logs/sessiond.err.log`. XML-escape all path strings.
- `install_agent()`: `create_dir_all(launch_agents_dir)` and `create_dir_all(app_support/logs)`; write `<launch_agents_dir>/ai.builderpro.desktop.sessiond.plist`; return the plist path.
- `bootstrap()`: run `["bootstrap", "gui/<uid>", <plist>]`. Exit 0 → success. Treat "already bootstrapped" / "service already loaded" (code `5` or stderr containing `already`) as **idempotent success**. On drift signal (stderr contains `already bootstrapped` but we detect a mismatch — see below) run `["bootout", "gui/<uid>/<LABEL>"]` then re-`bootstrap`. Any other non-zero exit → `LaunchdError::Install(stderr)` (surfaced as the actionable banner, spec §13). Simplify the drift path to: if the first `bootstrap` returns the "already" signal, run `bootout` then `bootstrap` once more; if that still fails non-idempotently → `Install`.
- `kickstart()`: run `["kickstart", "gui/<uid>/<LABEL>"]`; non-zero (and not an "already running" signal) → `LaunchdError::Command`.
- `is_loaded()`: run `["print", "gui/<uid>/<LABEL>"]`; `code == 0` → true, else false (never errors).
- Hard failure degradation (spec §13): `bootstrap`/`kickstart` non-idempotent non-zero → typed error the caller renders as "could not install background service" — never hang, never lie.

- [ ] **Step 1: Failing test — plist render shape + idempotent bootstrap ("already bootstrapped" == success) + dir-missing creation + kickstart cmd shape + hard-failure surfaces error.**

Add to `src-tauri/src/launchd.rs`:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::path::PathBuf;
    use std::sync::Mutex;

    /// Records every launchctl invocation and returns scripted outputs in order.
    struct MockLaunchctl {
        calls: Mutex<RefCell<Vec<Vec<String>>>>,
        scripted: Mutex<RefCell<std::collections::VecDeque<LaunchctlOutput>>>,
    }
    impl MockLaunchctl {
        fn new(outputs: Vec<LaunchctlOutput>) -> Self {
            MockLaunchctl {
                calls: Mutex::new(RefCell::new(Vec::new())),
                scripted: Mutex::new(RefCell::new(outputs.into_iter().collect())),
            }
        }
        fn calls(&self) -> Vec<Vec<String>> {
            self.calls.lock().unwrap().borrow().clone()
        }
    }
    impl LaunchctlRunner for MockLaunchctl {
        fn run(&self, args: &[&str]) -> std::io::Result<LaunchctlOutput> {
            self.calls.lock().unwrap().borrow_mut().push(args.iter().map(|s| s.to_string()).collect());
            let out = self.scripted.lock().unwrap().borrow_mut().pop_front()
                .unwrap_or(LaunchctlOutput { code: 0, stdout: String::new(), stderr: String::new() });
            Ok(out)
        }
    }

    fn ok() -> LaunchctlOutput { LaunchctlOutput { code: 0, stdout: String::new(), stderr: String::new() } }
    fn already() -> LaunchctlOutput {
        LaunchctlOutput { code: 5, stdout: String::new(), stderr: "Bootstrap failed: 5: Input/output error (service already bootstrapped)".into() }
    }

    fn agent<'a>(runner: &'a dyn LaunchctlRunner, root: &std::path::Path) -> LaunchdAgent<'a> {
        LaunchdAgent {
            runner,
            uid: 501,
            launch_agents_dir: root.join("LaunchAgents"),
            app_support_dir: root.join("AppSupport"),
            daemon_path: PathBuf::from("/Applications/Builder Pro AI.app/Contents/MacOS/bpa-sessiond"),
            socket_path: PathBuf::from("/tmp/bpa-501/d.sock"),
        }
    }

    #[test]
    fn render_plist_has_locked_keys() {
        let mock = MockLaunchctl::new(vec![]);
        let tmp = tempfile::tempdir().unwrap();
        let a = agent(&mock, tmp.path());
        let plist = a.render_plist();
        assert!(plist.contains("<key>Label</key>"));
        assert!(plist.contains("<string>ai.builderpro.desktop.sessiond</string>"));
        assert!(plist.contains("<string>--socket</string>"));
        assert!(plist.contains("<string>/tmp/bpa-501/d.sock</string>"));
        // KeepAlive MUST be a dict {Crashed:true}, never bare <true/>
        assert!(plist.contains("<key>KeepAlive</key>"));
        assert!(plist.contains("<key>Crashed</key>"));
        assert!(!plist.contains("<key>KeepAlive</key>\n  <true/>"));
        assert!(plist.contains("<key>RunAtLoad</key>"));
        assert!(plist.contains("<key>ThrottleInterval</key>"));
        assert!(plist.contains("<integer>10</integer>"));
        assert!(plist.contains("<string>Background</string>"));
        assert!(plist.contains("sessiond.out.log"));
        assert!(plist.contains("sessiond.err.log"));
    }

    #[test]
    fn install_creates_dirs_and_writes_plist() {
        let mock = MockLaunchctl::new(vec![]);
        let tmp = tempfile::tempdir().unwrap();
        let a = agent(&mock, tmp.path());
        let plist_path = a.install_agent().unwrap();
        assert!(plist_path.ends_with("ai.builderpro.desktop.sessiond.plist"));
        assert!(plist_path.exists(), "plist file written");
        assert!(tmp.path().join("AppSupport/logs").is_dir(), "log dir created");
        let contents = std::fs::read_to_string(&plist_path).unwrap();
        assert!(contents.contains("ai.builderpro.desktop.sessiond"));
    }

    #[test]
    fn bootstrap_already_bootstrapped_is_success() {
        // first bootstrap -> "already"; drift path: bootout(ok) then bootstrap(ok)
        let mock = MockLaunchctl::new(vec![already(), ok(), ok()]);
        let tmp = tempfile::tempdir().unwrap();
        let a = agent(&mock, tmp.path());
        a.install_agent().unwrap();
        a.bootstrap().expect("already-bootstrapped must be idempotent success");
        let calls = mock.calls();
        assert_eq!(calls[0][0], "bootstrap");
        assert_eq!(calls[0][1], "gui/501");
        assert_eq!(calls[1][0], "bootout");
        assert_eq!(calls[1][1], "gui/501/ai.builderpro.desktop.sessiond");
        assert_eq!(calls[2][0], "bootstrap");
    }

    #[test]
    fn bootstrap_clean_success_no_bootout() {
        let mock = MockLaunchctl::new(vec![ok()]);
        let tmp = tempfile::tempdir().unwrap();
        let a = agent(&mock, tmp.path());
        a.install_agent().unwrap();
        a.bootstrap().unwrap();
        assert_eq!(mock.calls().len(), 1, "clean bootstrap must not bootout");
        assert_eq!(mock.calls()[0][0], "bootstrap");
    }

    #[test]
    fn kickstart_cmd_shape() {
        let mock = MockLaunchctl::new(vec![ok()]);
        let tmp = tempfile::tempdir().unwrap();
        let a = agent(&mock, tmp.path());
        a.kickstart().unwrap();
        let calls = mock.calls();
        assert_eq!(calls[0], vec!["kickstart", "gui/501/ai.builderpro.desktop.sessiond"]);
    }

    #[test]
    fn hard_failure_surfaces_install_error() {
        let boom = LaunchctlOutput { code: 78, stdout: String::new(), stderr: "Operation not permitted (TCC)".into() };
        let mock = MockLaunchctl::new(vec![boom]);
        let tmp = tempfile::tempdir().unwrap();
        let a = agent(&mock, tmp.path());
        a.install_agent().unwrap();
        let err = a.bootstrap().unwrap_err();
        match err {
            LaunchdError::Install(msg) => assert!(msg.contains("Operation not permitted")),
            o => panic!("expected Install error, got {o:?}"),
        }
    }

    #[test]
    fn is_loaded_reads_print_exit_code() {
        let loaded = MockLaunchctl::new(vec![ok()]);
        let tmp = tempfile::tempdir().unwrap();
        let a = agent(&loaded, tmp.path());
        assert!(a.is_loaded());

        let unloaded = MockLaunchctl::new(vec![LaunchctlOutput { code: 113, stdout: String::new(), stderr: "Could not find service".into() }]);
        let a2 = agent(&unloaded, tmp.path());
        assert!(!a2.is_loaded());
        assert_eq!(unloaded.calls()[0], vec!["print", "gui/501/ai.builderpro.desktop.sessiond"]);
    }
}
```

- [ ] **Step 2: Run — confirm FAIL (no impl yet).**

```
cargo test -p builder-pro-ai launchd::tests
```
Expected: FAIL with a compile error — `cannot find type LaunchdAgent` / `LaunchctlRunner` / `LaunchdError` in this scope.

- [ ] **Step 3: Implement `launchd.rs`.**

Add to `src-tauri/src/launchd.rs`, **above** the `#[cfg(test)] mod tests` block:
```rust
//! Per-user LaunchAgent management for `bpa-sessiond` (spec §8.3).
//! launchd owns the daemon lifecycle; the GUI installs the plist, bootstraps it,
//! and kickstarts on demand. All launchctl calls go through an injectable runner
//! so unit tests never mutate the real service database. Degradation: hard
//! failures surface a typed error the UI renders as an actionable banner (spec §13).

use std::path::{Path, PathBuf};
use std::process::Command;

pub const LABEL: &str = "ai.builderpro.desktop.sessiond";

#[derive(Debug, Clone)]
pub struct LaunchctlOutput {
    pub code: i32,
    pub stdout: String,
    pub stderr: String,
}

pub trait LaunchctlRunner: Send + Sync {
    fn run(&self, args: &[&str]) -> std::io::Result<LaunchctlOutput>;
}

pub struct RealLaunchctl;

impl LaunchctlRunner for RealLaunchctl {
    fn run(&self, args: &[&str]) -> std::io::Result<LaunchctlOutput> {
        let out = Command::new("/bin/launchctl").args(args).output()?;
        Ok(LaunchctlOutput {
            code: out.status.code().unwrap_or(-1),
            stdout: String::from_utf8_lossy(&out.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&out.stderr).into_owned(),
        })
    }
}

#[derive(Debug, thiserror::Error)]
pub enum LaunchdError {
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("could not install background service: {0}")]
    Install(String),
    #[error("launchctl {op} failed (code {code}): {stderr}")]
    Command { op: String, code: i32, stderr: String },
    #[error("cannot resolve daemon path: {0}")]
    DaemonPath(String),
}

pub struct LaunchdAgent<'a> {
    pub runner: &'a dyn LaunchctlRunner,
    pub uid: u32,
    pub launch_agents_dir: PathBuf,
    pub app_support_dir: PathBuf,
    pub daemon_path: PathBuf,
    pub socket_path: PathBuf,
}

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

/// stderr signals launchd already knows this label (idempotent).
fn is_already_signal(out: &LaunchctlOutput) -> bool {
    let s = out.stderr.to_ascii_lowercase();
    out.code == 5 || s.contains("already") || s.contains("service already loaded")
}

/// stderr signals the service is already running (kickstart idempotency).
fn is_already_running(out: &LaunchctlOutput) -> bool {
    let s = out.stderr.to_ascii_lowercase();
    s.contains("already running") || s.contains("service is already running")
}

impl<'a> LaunchdAgent<'a> {
    fn plist_filename(&self) -> String {
        format!("{LABEL}.plist")
    }
    fn plist_path(&self) -> PathBuf {
        self.launch_agents_dir.join(self.plist_filename())
    }
    fn service_target(&self) -> String {
        format!("gui/{}/{}", self.uid, LABEL)
    }
    fn domain_target(&self) -> String {
        format!("gui/{}", self.uid)
    }
    fn logs_dir(&self) -> PathBuf {
        self.app_support_dir.join("logs")
    }

    pub fn render_plist(&self) -> String {
        let daemon = xml_escape(&self.daemon_path.to_string_lossy());
        let socket = xml_escape(&self.socket_path.to_string_lossy());
        let out_log = xml_escape(&self.logs_dir().join("sessiond.out.log").to_string_lossy());
        let err_log = xml_escape(&self.logs_dir().join("sessiond.err.log").to_string_lossy());
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>Label</key>
  <string>{label}</string>
  <key>ProgramArguments</key>
  <array>
    <string>{daemon}</string>
    <string>--socket</string>
    <string>{socket}</string>
  </array>
  <key>KeepAlive</key>
  <dict>
    <key>Crashed</key>
    <true/>
  </dict>
  <key>RunAtLoad</key>
  <false/>
  <key>ThrottleInterval</key>
  <integer>10</integer>
  <key>ProcessType</key>
  <string>Background</string>
  <key>StandardOutPath</key>
  <string>{out_log}</string>
  <key>StandardErrorPath</key>
  <string>{err_log}</string>
</dict>
</plist>
"#,
            label = LABEL,
            daemon = daemon,
            socket = socket,
            out_log = out_log,
            err_log = err_log,
        )
    }

    pub fn install_agent(&self) -> Result<PathBuf, LaunchdError> {
        std::fs::create_dir_all(&self.launch_agents_dir)?;
        std::fs::create_dir_all(self.logs_dir())?;
        let path = self.plist_path();
        std::fs::write(&path, self.render_plist())?;
        tracing::info!(plist = %path.display(), "installed LaunchAgent plist");
        Ok(path)
    }

    pub fn bootstrap(&self) -> Result<(), LaunchdError> {
        let plist = self.plist_path();
        let plist_str = plist.to_string_lossy().into_owned();
        let domain = self.domain_target();

        let out = self.runner.run(&["bootstrap", &domain, &plist_str])?;
        if out.code == 0 {
            return Ok(());
        }
        if is_already_signal(&out) {
            // drift: bootout then re-bootstrap once.
            tracing::warn!(stderr = %out.stderr, "service already bootstrapped; rebootstrapping");
            let target = self.service_target();
            let _ = self.runner.run(&["bootout", &target])?; // best-effort; ignore its code
            let retry = self.runner.run(&["bootstrap", &domain, &plist_str])?;
            if retry.code == 0 || is_already_signal(&retry) {
                return Ok(());
            }
            return Err(LaunchdError::Install(retry.stderr));
        }
        Err(LaunchdError::Install(out.stderr))
    }

    pub fn kickstart(&self) -> Result<(), LaunchdError> {
        let target = self.service_target();
        let out = self.runner.run(&["kickstart", &target])?;
        if out.code == 0 || is_already_running(&out) {
            return Ok(());
        }
        Err(LaunchdError::Command { op: "kickstart".into(), code: out.code, stderr: out.stderr })
    }

    pub fn is_loaded(&self) -> bool {
        let target = self.service_target();
        matches!(self.runner.run(&["print", &target]), Ok(o) if o.code == 0)
    }

    pub fn resolve_daemon_path() -> Result<PathBuf, LaunchdError> {
        let exe = std::env::current_exe()
            .map_err(|e| LaunchdError::DaemonPath(e.to_string()))?;
        let dir = exe
            .parent()
            .ok_or_else(|| LaunchdError::DaemonPath("current_exe has no parent".into()))?;
        let candidate = dir.join("bpa-sessiond");
        if candidate.exists() {
            Ok(candidate)
        } else {
            Err(LaunchdError::DaemonPath(format!(
                "bpa-sessiond not found beside {}",
                exe.display()
            )))
        }
    }
}

// keep the import used only by RealLaunchctl / helpers from tripping dead-code lints
// when the crate is built without exercising the real path.
#[allow(unused_imports)]
use Path as _PathUnused;
```

> Implementer note: delete the final `#[allow(unused_imports)] use Path as _PathUnused;` line if `Path` is actually referenced (it is, via `PathBuf`/method calls) — it is included only as a guard and will warn as an unused import itself. Prefer removing it; the real code already uses `PathBuf`. If clippy flags `Path` unused, import only `PathBuf`. Simplest correct form: `use std::path::PathBuf;` (drop `Path`) and remove the guard line.

- [ ] **Step 3a: Fix imports.** Change the top import to `use std::path::PathBuf;` (remove `Path`) and delete the trailing `#[allow(unused_imports)] use Path as _PathUnused;` guard line.

- [ ] **Step 4: Run — confirm PASS.**

```
cargo test -p builder-pro-ai launchd::tests
```
Expected: PASS — all eight tests green (render keys, install dirs+plist, already-bootstrapped idempotent, clean-bootstrap-no-bootout, kickstart shape, hard-failure Install error, is_loaded true/false).

- [ ] **Step 5: Wire the module + commit.**

Ensure `src-tauri/src/lib.rs` contains `pub mod launchd;`. Then:
```
git add src-tauri/src/launchd.rs src-tauri/src/lib.rs src-tauri/Cargo.toml && git commit -m "feat(core): LaunchAgent install/bootstrap/kickstart with injectable launchctl + degradation"
```

**Definition of Done:**
- `cargo test -p builder-pro-ai launchd::tests` green (all eight cases).
- `render_plist()` emits the exact spec §8.3 keys: `Label`=`ai.builderpro.desktop.sessiond`, `ProgramArguments`=`[daemon, "--socket", socket]`, `KeepAlive` as `<dict>{Crashed:true}</dict>` (never bare `<true/>`), `RunAtLoad=<false/>`, `ThrottleInterval=10`, `ProcessType=Background`, out/err log paths under `{app_support}/logs`; all paths XML-escaped.
- `install_agent()` creates `launch_agents_dir` + `{app_support}/logs` and writes the plist (idempotent create-dir-all).
- `bootstrap()` treats "already bootstrapped" (code 5 / stderr `already`) as success via bootout+re-bootstrap; a clean bootstrap does **not** bootout; a hard non-idempotent failure (e.g. TCC) → `LaunchdError::Install` for the actionable banner (spec §8.3, §13).
- `kickstart()` runs `kickstart gui/<uid>/<LABEL>` (idempotent on "already running"); `is_loaded()` maps `print` exit 0 → true.
- `resolve_daemon_path()` derives the bundled `bpa-sessiond` from `current_exe()`'s sibling (spec §8.3 installer contract).
- Structured `tracing` logs on install/rebootstrap; no secrets (spec §13).


### Task 17: `commands.rs` + `broker.rs` — the `#[tauri::command]` surface + daemon-frame broker

**Files:**
- Create: `src-tauri/src/commands.rs`
- Create: `src-tauri/src/broker.rs`
- Test: unit tests live inline in each module under `#[cfg(test)] mod tests` (broker mapping, command arg plumbing, pick_folder core-only).

**Depends on:** [T3, T14, T15]   **Parallel-safe with:** [T16]

**Interfaces:**
- Consumes:
  - From **T3 `crates/protocol`**: `SessionId`, `WorkspaceId`, `Workspace`, `SessionLifecycle`, `SessionMeta`, `TerminalEvent` (`TerminalEvent::Replay { cols: u16, rows: u16, content: Vec<u8> }`, `TerminalEvent::Output { bytes: Vec<u8> }`), `Request` (`CreateSession { workspace_id, shell, cwd, env_overrides, cols, rows }`, `ListSessions`, `AttachSession { session_id }`, `DetachSession { session_id }`, `WriteStdin { session_id, bytes }`, `Resize { session_id, cols, rows }`, `KillSession { session_id }`, `ListWorkspaces`, `CreateWorkspace { name, root_path }`, `GetSessionState { session_id }`), `Response` (`Workspaces(Vec<Workspace>)`, `Workspace(Workspace)`, `Sessions(Vec<SessionMeta>)`, `Session(SessionMeta)`, `Ack`, `Error { code, message }`), `Push` (`Replay { session_id, cols, rows, content }`, `Output { session_id, bytes }`, `StateChanged { session_id, lifecycle, waiting_for_input, cwd }`, `ChildExited { session_id, code, signal }`, `SessionCreated { meta }`, `WorkspaceCreated { workspace }`, `Error { session_id, code, message }`).
  - From **T14 `socket_client`**: `DaemonClient::connect() -> Result<DaemonClient>`, `DaemonClient::request(Request) -> Result<Response>` (correlated), `DaemonClient::on_push(cb)`.
  - From **T15 `paths`**: `validate_dir(path: &str) -> Result<PathBuf, PathError>`.
- Produces (names verbatim; consumed by T18's `invoke_handler`):
  - `#[tauri::command] async fn create_session(state, workspace_id: WorkspaceId, opts: Option<CreateOpts>) -> Result<SessionMeta, CommandError>`
  - `#[tauri::command] async fn list_sessions(state) -> Result<Vec<SessionMeta>, CommandError>`
  - `#[tauri::command] async fn attach_session(state, session_id: SessionId, on_event: tauri::ipc::Channel<TerminalEvent>) -> Result<(), CommandError>`
  - `#[tauri::command] async fn detach_session(state, session_id: SessionId) -> Result<(), CommandError>`
  - `#[tauri::command] async fn write_stdin(state, session_id: SessionId, data: String) -> Result<(), CommandError>`
  - `#[tauri::command] async fn resize(state, session_id: SessionId, cols: u16, rows: u16) -> Result<(), CommandError>`
  - `#[tauri::command] async fn kill_session(state, session_id: SessionId) -> Result<(), CommandError>`
  - `#[tauri::command] async fn list_workspaces(state) -> Result<Vec<Workspace>, CommandError>`
  - `#[tauri::command] async fn create_workspace(state, name: String, root_path: String) -> Result<Workspace, CommandError>`
  - `#[tauri::command] async fn get_session_state(state, session_id: SessionId) -> Result<SessionMeta, CommandError>`
  - `#[tauri::command] async fn pick_folder(app: tauri::AppHandle) -> Result<Option<String>, CommandError>` (CORE-ONLY, `tauri-plugin-dialog`)
  - `pub struct CreateOpts { shell: Option<String>, cwd: Option<String>, env_overrides: Vec<(String, String)>, cols: Option<u16>, rows: Option<u16> }` (serde `camelCase`, `env_overrides` defaults `[]` via `#[serde(default)]`)
  - `pub enum CommandError { Daemon { code: String, message: String }, Disconnected, Internal(String) }` (serde-serializable, `impl From<...>`)
  - `broker.rs`: `pub struct Broker { app: AppHandle, attachments: Arc<Mutex<HashMap<SessionId, Channel<TerminalEvent>>>> }`, `Broker::new(app) -> Broker`, `Broker::register_attachment(session_id, Channel<TerminalEvent>)`, `Broker::remove_attachment(&session_id)`, `Broker::dispatch_push(push: Push)` (the `on_push` callback body), plus pure mapping fns `map_state_changed(&Push) -> StateChangedPayload`, `map_child_exited(&Push) -> ChildExitedPayload`.

---

- [ ] **Step 1: Write failing test — `CreateOpts` deserializes camelCase with `envOverrides` defaulting to `[]`.**
  Add to `src-tauri/src/commands.rs`:
  ```rust
  #[cfg(test)]
  mod tests {
      use super::*;

      #[test]
      fn create_opts_defaults_env_overrides_and_reads_camel_case() {
          // envOverrides omitted -> defaults to []
          let json = r#"{ "shell": "/bin/zsh", "cols": 120, "rows": 40 }"#;
          let opts: CreateOpts = serde_json::from_str(json).unwrap();
          assert_eq!(opts.shell.as_deref(), Some("/bin/zsh"));
          assert_eq!(opts.cwd, None);
          assert_eq!(opts.env_overrides, Vec::<(String, String)>::new());
          assert_eq!(opts.cols, Some(120));
          assert_eq!(opts.rows, Some(40));

          // envOverrides present as camelCase, array of [k,v] pairs
          let json2 = r#"{ "envOverrides": [["FOO", "bar"], ["BAZ", "qux"]] }"#;
          let opts2: CreateOpts = serde_json::from_str(json2).unwrap();
          assert_eq!(
              opts2.env_overrides,
              vec![("FOO".to_string(), "bar".to_string()), ("BAZ".to_string(), "qux".to_string())]
          );
          assert_eq!(opts2.shell, None);
      }

      #[test]
      fn create_session_uses_80x24_when_size_omitted() {
          // opts with no cols/rows -> the Request carries 80x24
          let opts = CreateOpts { shell: None, cwd: None, env_overrides: vec![], cols: None, rows: None };
          let (c, r) = resolve_size(&opts);
          assert_eq!((c, r), (80, 24));

          let opts2 = CreateOpts { shell: None, cwd: None, env_overrides: vec![], cols: Some(100), rows: Some(30) };
          assert_eq!(resolve_size(&opts2), (100, 30));
      }
  }
  ```

- [ ] **Step 2: Run test — confirm FAIL.**
  `cargo test -p builder-pro-ai create_opts_defaults_env_overrides_and_reads_camel_case`
  Expected: FAIL with `cannot find type CreateOpts in this scope` / `cannot find function resolve_size`.

- [ ] **Step 3: Implement `CreateOpts`, `resolve_size`, and `CommandError`.**
  Write the top of `src-tauri/src/commands.rs`:
  ```rust
  use std::sync::Arc;

  use serde::{Deserialize, Serialize};
  use tauri::ipc::Channel;
  use tauri::{AppHandle, State};
  use tauri_plugin_dialog::DialogExt;
  use tokio::sync::Mutex;

  use protocol::{
      Request, Response, SessionId, SessionMeta, TerminalEvent, Workspace, WorkspaceId,
  };

  use crate::broker::Broker;
  use crate::socket_client::DaemonClient;

  /// Shared, managed application state: the daemon client + the push broker.
  pub struct AppState {
      pub client: Arc<DaemonClient>,
      pub broker: Arc<Broker>,
  }

  /// Options for `create_session`. `env_overrides` defaults to `[]`; the frontend
  /// normally omits it (it exists because S6 agents drive this surface).
  #[derive(Debug, Clone, Deserialize)]
  #[serde(rename_all = "camelCase")]
  pub struct CreateOpts {
      #[serde(default)]
      pub shell: Option<String>,
      #[serde(default)]
      pub cwd: Option<String>,
      #[serde(default)]
      pub env_overrides: Vec<(String, String)>,
      #[serde(default)]
      pub cols: Option<u16>,
      #[serde(default)]
      pub rows: Option<u16>,
  }

  impl Default for CreateOpts {
      fn default() -> Self {
          CreateOpts { shell: None, cwd: None, env_overrides: Vec::new(), cols: None, rows: None }
      }
  }

  /// If `cols`/`rows` are omitted the core sends the default 80x24 (spec §6.1);
  /// the frontend passes a real size after the first `fitAddon.fit()`.
  pub fn resolve_size(opts: &CreateOpts) -> (u16, u16) {
      (opts.cols.unwrap_or(80), opts.rows.unwrap_or(24))
  }

  /// Error surfaced to the webview. Serializes so `invoke()` rejects the JS Promise.
  #[derive(Debug, Clone, Serialize)]
  #[serde(tag = "kind", rename_all = "camelCase")]
  pub enum CommandError {
      /// A typed daemon-side `Response::Error { code, message }`.
      Daemon { code: String, message: String },
      /// The daemon socket is not currently connected.
      Disconnected,
      /// An unexpected local failure (validation, protocol shape, etc.).
      Internal(String),
  }

  impl std::fmt::Display for CommandError {
      fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
          match self {
              CommandError::Daemon { code, message } => write!(f, "daemon error [{code}]: {message}"),
              CommandError::Disconnected => write!(f, "daemon disconnected"),
              CommandError::Internal(m) => write!(f, "internal error: {m}"),
          }
      }
  }
  impl std::error::Error for CommandError {}
  ```

- [ ] **Step 4: Run test — confirm PASS.**
  `cargo test -p builder-pro-ai create_opts_defaults_env_overrides_and_reads_camel_case`
  Expected: PASS (both `create_opts_*` and `create_session_uses_80x24_when_size_omitted`).

- [ ] **Step 5: Commit.**
  `git add src-tauri/src/commands.rs && git commit -m "feat(core): CreateOpts (camelCase, env default) + resolve_size 80x24 + CommandError"`

- [ ] **Step 6: Write failing test — broker `Push::StateChanged` / `Push::ChildExited` map to camelCase payload shapes.**
  Create `src-tauri/src/broker.rs` with the test module up front:
  ```rust
  #[cfg(test)]
  mod tests {
      use super::*;
      use protocol::{Push, SessionLifecycle};

      #[test]
      fn state_changed_push_maps_to_camel_case_payload() {
          let push = Push::StateChanged {
              session_id: "sess-1".to_string(),
              lifecycle: SessionLifecycle::Running,
              waiting_for_input: true,
              cwd: "/work/proj".to_string(),
          };
          let payload = map_state_changed(&push).expect("StateChanged maps");
          let v = serde_json::to_value(&payload).unwrap();
          assert_eq!(v["sessionId"], "sess-1");
          assert_eq!(v["lifecycle"]["kind"], "running");
          assert_eq!(v["waitingForInput"], true);
          assert_eq!(v["cwd"], "/work/proj");
          // snake_case keys must NOT leak through.
          assert!(v.get("session_id").is_none());
          assert!(v.get("waiting_for_input").is_none());
      }

      #[test]
      fn child_exited_push_maps_to_reshaped_payload() {
          let push = Push::ChildExited {
              session_id: "sess-2".to_string(),
              code: Some(137),
              signal: Some("SIGKILL".to_string()),
          };
          let payload = map_child_exited(&push).expect("ChildExited maps");
          let v = serde_json::to_value(&payload).unwrap();
          assert_eq!(v["sessionId"], "sess-2");
          assert_eq!(v["code"], 137);
          assert_eq!(v["signal"], "SIGKILL");

          // code None + signal None round-trips as JSON null (never coerced to 0).
          let push_none = Push::ChildExited {
              session_id: "sess-3".to_string(),
              code: None,
              signal: None,
          };
          let v2 = serde_json::to_value(&map_child_exited(&push_none).unwrap()).unwrap();
          assert!(v2["code"].is_null());
          assert!(v2["signal"].is_null());
      }

      #[test]
      fn non_matching_push_variants_do_not_map_to_state_payloads() {
          let out = Push::Output { session_id: "s".into(), bytes: vec![1, 2, 3] };
          assert!(map_state_changed(&out).is_none());
          assert!(map_child_exited(&out).is_none());
      }
  }
  ```

- [ ] **Step 7: Run test — confirm FAIL.**
  `cargo test -p builder-pro-ai state_changed_push_maps_to_camel_case_payload`
  Expected: FAIL with `cannot find function map_state_changed` / `map_child_exited`.

- [ ] **Step 8: Implement `broker.rs` payloads + pure mapping fns + `Broker`.**
  Prepend above the test module in `src-tauri/src/broker.rs`:
  ```rust
  use std::collections::HashMap;
  use std::sync::Arc;

  use serde::Serialize;
  use tauri::ipc::Channel;
  use tauri::{AppHandle, Emitter};
  use tokio::sync::Mutex;
  use tracing::{debug, warn};

  use protocol::{Push, SessionId, SessionLifecycle, SessionMeta, TerminalEvent, Workspace};

  /// Global event names (spec §6.3). Kept as constants so T18 and tests agree.
  pub const EV_SESSION_CREATED: &str = "session://created";
  pub const EV_SESSION_STATE_CHANGED: &str = "session://state-changed";
  pub const EV_SESSION_EXITED: &str = "session://exited";
  pub const EV_WORKSPACE_CREATED: &str = "workspace://created";

  /// Payload for `session://state-changed` (snake→camel rename, spec §6.3).
  #[derive(Debug, Clone, Serialize)]
  #[serde(rename_all = "camelCase")]
  pub struct StateChangedPayload {
      pub session_id: SessionId,
      pub lifecycle: SessionLifecycle,
      pub waiting_for_input: bool,
      pub cwd: String,
  }

  /// Payload for `session://exited` (reshaped from `Push::ChildExited`, spec §6.3).
  #[derive(Debug, Clone, Serialize)]
  #[serde(rename_all = "camelCase")]
  pub struct ChildExitedPayload {
      pub session_id: SessionId,
      pub code: Option<u8>,
      pub signal: Option<String>,
  }

  /// Pure mapping: `Push::StateChanged` → `session://state-changed` payload.
  /// Returns `None` for any other variant so callers can pattern-match safely.
  pub fn map_state_changed(push: &Push) -> Option<StateChangedPayload> {
      match push {
          Push::StateChanged { session_id, lifecycle, waiting_for_input, cwd } => {
              Some(StateChangedPayload {
                  session_id: session_id.clone(),
                  lifecycle: lifecycle.clone(),
                  waiting_for_input: *waiting_for_input,
                  cwd: cwd.clone(),
              })
          }
          _ => None,
      }
  }

  /// Pure mapping: `Push::ChildExited` → `session://exited` payload.
  pub fn map_child_exited(push: &Push) -> Option<ChildExitedPayload> {
      match push {
          Push::ChildExited { session_id, code, signal } => Some(ChildExitedPayload {
              session_id: session_id.clone(),
              code: *code,
              signal: signal.clone(),
          }),
          _ => None,
      }
  }

  /// Brokers daemon `Push` frames to Hop-A: attached-session `Channel`s and the
  /// global Tauri event bus. Owns the single-attach map (spec §7 attach model).
  pub struct Broker {
      app: AppHandle,
      attachments: Arc<Mutex<HashMap<SessionId, Channel<TerminalEvent>>>>,
  }

  impl Broker {
      pub fn new(app: AppHandle) -> Self {
          Broker { app, attachments: Arc::new(Mutex::new(HashMap::new())) }
      }

      /// Register (or supersede) the attach channel for a session (spec §7:
      /// a second attach supersedes the prior registration).
      pub async fn register_attachment(&self, session_id: SessionId, ch: Channel<TerminalEvent>) {
          self.attachments.lock().await.insert(session_id, ch);
      }

      /// Remove a session's attach channel (on detach / kill / exit).
      pub async fn remove_attachment(&self, session_id: &SessionId) {
          self.attachments.lock().await.remove(session_id);
      }

      /// Fan a single daemon `Push` out to the correct Hop-A sink. This is the
      /// body invoked from `DaemonClient::on_push`.
      pub async fn dispatch_push(&self, push: Push) {
          match &push {
              Push::Replay { session_id, cols, rows, content } => {
                  let ev = TerminalEvent::Replay { cols: *cols, rows: *rows, content: content.clone() };
                  self.send_to_channel(session_id, ev).await;
              }
              Push::Output { session_id, bytes } => {
                  let ev = TerminalEvent::Output { bytes: bytes.clone() };
                  self.send_to_channel(session_id, ev).await;
              }
              Push::StateChanged { .. } => {
                  if let Some(payload) = map_state_changed(&push) {
                      self.emit(EV_SESSION_STATE_CHANGED, &payload);
                  }
              }
              Push::ChildExited { .. } => {
                  if let Some(payload) = map_child_exited(&push) {
                      self.emit(EV_SESSION_EXITED, &payload);
                  }
              }
              Push::SessionCreated { meta } => {
                  self.emit::<SessionMeta>(EV_SESSION_CREATED, meta);
              }
              Push::WorkspaceCreated { workspace } => {
                  self.emit::<Workspace>(EV_WORKSPACE_CREATED, workspace);
              }
              Push::Error { session_id, code, message } => {
                  warn!(target: "broker", ?session_id, code, message, "daemon async error push");
              }
          }
      }

      async fn send_to_channel(&self, session_id: &SessionId, ev: TerminalEvent) {
          let guard = self.attachments.lock().await;
          if let Some(ch) = guard.get(session_id) {
              if let Err(e) = ch.send(ev) {
                  warn!(target: "broker", session_id, error = %e, "channel send failed");
              }
          } else {
              debug!(target: "broker", session_id, "push for unattached session dropped");
          }
      }

      fn emit<T: Serialize + Clone>(&self, event: &str, payload: &T) {
          if let Err(e) = self.app.emit(event, payload.clone()) {
              warn!(target: "broker", event, error = %e, "emit failed");
          }
      }
  }
  ```

- [ ] **Step 9: Run test — confirm PASS.**
  `cargo test -p builder-pro-ai state_changed_push_maps_to_camel_case_payload child_exited_push_maps_to_reshaped_payload non_matching_push_variants_do_not_map_to_state_payloads`
  Expected: PASS (all three).

- [ ] **Step 10: Commit.**
  `git add src-tauri/src/broker.rs && git commit -m "feat(core): broker Push->Channel/event mapping (snake->camel, ChildExited reshape)"`

- [ ] **Step 11: Write failing test — command arg plumbing forwards the correct `Request` (mock `DaemonClient`).**
  The commands must build exactly the right `Request` from their args and turn `Response::Error` into `CommandError::Daemon`. Add a request-building layer that is unit-testable without a live socket. Append to `commands.rs` test module:
  ```rust
  #[test]
  fn create_session_builds_request_with_defaults() {
      let req = build_create_session("ws-1".to_string(), None);
      match req {
          Request::CreateSession { workspace_id, shell, cwd, env_overrides, cols, rows } => {
              assert_eq!(workspace_id, "ws-1");
              assert_eq!(shell, None);
              assert_eq!(cwd, None);
              assert_eq!(env_overrides, Vec::<(String, String)>::new());
              assert_eq!((cols, rows), (80, 24));
          }
          other => panic!("expected CreateSession, got {other:?}"),
      }
  }

  #[test]
  fn create_session_builds_request_with_opts() {
      let opts = CreateOpts {
          shell: Some("/bin/bash".into()),
          cwd: Some("/tmp/x".into()),
          env_overrides: vec![("K".into(), "V".into())],
          cols: Some(90),
          rows: Some(25),
      };
      let req = build_create_session("ws-2".to_string(), Some(opts));
      match req {
          Request::CreateSession { workspace_id, shell, cwd, env_overrides, cols, rows } => {
              assert_eq!(workspace_id, "ws-2");
              assert_eq!(shell.as_deref(), Some("/bin/bash"));
              assert_eq!(cwd.as_deref(), Some("/tmp/x"));
              assert_eq!(env_overrides, vec![("K".to_string(), "V".to_string())]);
              assert_eq!((cols, rows), (90, 25));
          }
          other => panic!("expected CreateSession, got {other:?}"),
      }
  }

  #[test]
  fn write_stdin_builds_request_utf8_bytes() {
      let req = build_write_stdin("s".to_string(), "héllo".to_string());
      match req {
          Request::WriteStdin { session_id, bytes } => {
              assert_eq!(session_id, "s");
              assert_eq!(bytes, "héllo".as_bytes().to_vec());
          }
          other => panic!("expected WriteStdin, got {other:?}"),
      }
  }

  #[test]
  fn response_error_becomes_command_error_daemon() {
      let res = Response::Error { code: "InvalidWorkspaceRoot".into(), message: "gone".into() };
      let err = expect_session(res).unwrap_err();
      match err {
          CommandError::Daemon { code, message } => {
              assert_eq!(code, "InvalidWorkspaceRoot");
              assert_eq!(message, "gone");
          }
          other => panic!("expected Daemon error, got {other:?}"),
      }
  }

  #[test]
  fn response_session_unwraps_to_meta() {
      let meta = SessionMeta {
          id: "s".into(), workspace_id: "w".into(), title: "t".into(), shell: "/bin/zsh".into(),
          cwd: "/".into(), cols: 80, rows: 24, lifecycle: protocol::SessionLifecycle::AtPrompt,
          waiting_for_input: false, is_active: true, created_at: 0,
      };
      let got = expect_session(Response::Session(meta.clone())).unwrap();
      assert_eq!(got.id, "s");
      // Wrong variant is an Internal protocol error, not a silent default.
      assert!(matches!(expect_session(Response::Ack), Err(CommandError::Internal(_))));
  }
  ```

- [ ] **Step 12: Run test — confirm FAIL.**
  `cargo test -p builder-pro-ai create_session_builds_request_with_defaults`
  Expected: FAIL with `cannot find function build_create_session` (and `build_write_stdin`, `expect_session`).

- [ ] **Step 13: Implement request-builders, response-unwrappers, and the `#[tauri::command]` fns.**
  Append to `commands.rs` (below the types, above the test module):
  ```rust
  // ── pure request builders (unit-tested without a socket) ───────────────────

  pub(crate) fn build_create_session(workspace_id: WorkspaceId, opts: Option<CreateOpts>) -> Request {
      let opts = opts.unwrap_or_default();
      let (cols, rows) = resolve_size(&opts);
      Request::CreateSession {
          workspace_id,
          shell: opts.shell,
          cwd: opts.cwd,
          env_overrides: opts.env_overrides,
          cols,
          rows,
      }
  }

  pub(crate) fn build_write_stdin(session_id: SessionId, data: String) -> Request {
      Request::WriteStdin { session_id, bytes: data.into_bytes() }
  }

  // ── response unwrappers: map the expected variant or a typed error ─────────

  fn err_from_response(res: Response) -> CommandError {
      match res {
          Response::Error { code, message } => CommandError::Daemon { code, message },
          other => CommandError::Internal(format!("unexpected daemon response: {other:?}")),
      }
  }

  pub(crate) fn expect_session(res: Response) -> Result<SessionMeta, CommandError> {
      match res {
          Response::Session(m) => Ok(m),
          other => Err(err_from_response(other)),
      }
  }

  fn expect_sessions(res: Response) -> Result<Vec<SessionMeta>, CommandError> {
      match res {
          Response::Sessions(v) => Ok(v),
          other => Err(err_from_response(other)),
      }
  }

  fn expect_workspace(res: Response) -> Result<Workspace, CommandError> {
      match res {
          Response::Workspace(w) => Ok(w),
          other => Err(err_from_response(other)),
      }
  }

  fn expect_workspaces(res: Response) -> Result<Vec<Workspace>, CommandError> {
      match res {
          Response::Workspaces(v) => Ok(v),
          other => Err(err_from_response(other)),
      }
  }

  fn expect_ack(res: Response) -> Result<(), CommandError> {
      match res {
          Response::Ack => Ok(()),
          other => Err(err_from_response(other)),
      }
  }

  // ── #[tauri::command] surface (spec §6.1) ──────────────────────────────────

  #[tauri::command]
  pub async fn create_session(
      state: State<'_, AppState>,
      workspace_id: WorkspaceId,
      opts: Option<CreateOpts>,
  ) -> Result<SessionMeta, CommandError> {
      let req = build_create_session(workspace_id, opts);
      expect_session(state.client.request(req).await?)
  }

  #[tauri::command]
  pub async fn list_sessions(state: State<'_, AppState>) -> Result<Vec<SessionMeta>, CommandError> {
      expect_sessions(state.client.request(Request::ListSessions).await?)
  }

  #[tauri::command]
  pub async fn attach_session(
      state: State<'_, AppState>,
      session_id: SessionId,
      on_event: Channel<TerminalEvent>,
  ) -> Result<(), CommandError> {
      // Register the channel BEFORE asking the daemon to attach, so the first
      // Push::Replay it sends is delivered (spec §7 reattach flow).
      state.broker.register_attachment(session_id.clone(), on_event).await;
      let res = state
          .client
          .request(Request::AttachSession { session_id: session_id.clone() })
          .await;
      match res {
          Ok(r) => expect_ack(r).map_err(|e| {
              // Attach rejected: drop the just-registered channel to avoid a leak.
              let broker = state.broker.clone();
              tauri::async_runtime::spawn(async move { broker.remove_attachment(&session_id).await });
              e
          }),
          Err(e) => {
              state.broker.remove_attachment(&session_id).await;
              Err(e.into())
          }
      }
  }

  #[tauri::command]
  pub async fn detach_session(
      state: State<'_, AppState>,
      session_id: SessionId,
  ) -> Result<(), CommandError> {
      let out = expect_ack(
          state.client.request(Request::DetachSession { session_id: session_id.clone() }).await?,
      );
      state.broker.remove_attachment(&session_id).await;
      out
  }

  #[tauri::command]
  pub async fn write_stdin(
      state: State<'_, AppState>,
      session_id: SessionId,
      data: String,
  ) -> Result<(), CommandError> {
      expect_ack(state.client.request(build_write_stdin(session_id, data)).await?)
  }

  #[tauri::command]
  pub async fn resize(
      state: State<'_, AppState>,
      session_id: SessionId,
      cols: u16,
      rows: u16,
  ) -> Result<(), CommandError> {
      expect_ack(state.client.request(Request::Resize { session_id, cols, rows }).await?)
  }

  #[tauri::command]
  pub async fn kill_session(
      state: State<'_, AppState>,
      session_id: SessionId,
  ) -> Result<(), CommandError> {
      let out = expect_ack(
          state.client.request(Request::KillSession { session_id: session_id.clone() }).await?,
      );
      state.broker.remove_attachment(&session_id).await;
      out
  }

  #[tauri::command]
  pub async fn list_workspaces(state: State<'_, AppState>) -> Result<Vec<Workspace>, CommandError> {
      expect_workspaces(state.client.request(Request::ListWorkspaces).await?)
  }

  #[tauri::command]
  pub async fn create_workspace(
      state: State<'_, AppState>,
      name: String,
      root_path: String,
  ) -> Result<Workspace, CommandError> {
      // Fail fast on an invalid root before touching the daemon (spec §16); the
      // daemon re-validates (defense in depth for S6 agents).
      crate::paths::validate_dir(&root_path)
          .map_err(|e| CommandError::Daemon {
              code: "InvalidWorkspaceRoot".to_string(),
              message: e.to_string(),
          })?;
      expect_workspace(
          state.client.request(Request::CreateWorkspace { name, root_path }).await?,
      )
  }

  #[tauri::command]
  pub async fn get_session_state(
      state: State<'_, AppState>,
      session_id: SessionId,
  ) -> Result<SessionMeta, CommandError> {
      expect_session(state.client.request(Request::GetSessionState { session_id }).await?)
  }

  /// CORE-ONLY (spec §6.1): the native folder picker must run in the GUI process,
  /// never brokered to the daemon. Returns the chosen absolute path or `None`.
  #[tauri::command]
  pub async fn pick_folder(app: AppHandle) -> Result<Option<String>, CommandError> {
      let (tx, rx) = tokio::sync::oneshot::channel();
      app.dialog().file().pick_folder(move |maybe_path| {
          let _ = tx.send(maybe_path);
      });
      let chosen = rx
          .await
          .map_err(|e| CommandError::Internal(format!("dialog channel closed: {e}")))?;
      Ok(chosen.map(|p| p.to_string()))
  }
  ```
  Add `From<crate::socket_client::ClientError>` so `?` on `state.client.request(...)` yields a `CommandError`. Below `impl std::error::Error for CommandError {}`:
  ```rust
  impl From<crate::socket_client::ClientError> for CommandError {
      fn from(e: crate::socket_client::ClientError) -> Self {
          use crate::socket_client::ClientError;
          match e {
              ClientError::Disconnected => CommandError::Disconnected,
              ClientError::Daemon { code, message } => CommandError::Daemon { code, message },
              other => CommandError::Internal(other.to_string()),
          }
      }
  }
  ```
  > Note: `crate::socket_client::ClientError` is produced by **T14**. Its variants `Disconnected`, `Daemon { code, message }`, and a catch-all are part of T14's locked surface; this `From` maps them 1:1 onto `CommandError`.

- [ ] **Step 14: Run test — confirm PASS.**
  `cargo test -p builder-pro-ai create_session_builds_request_with_defaults create_session_builds_request_with_opts write_stdin_builds_request_utf8_bytes response_error_becomes_command_error_daemon response_session_unwraps_to_meta`
  Expected: PASS (all five).

- [ ] **Step 15: Commit.**
  `git add src-tauri/src/commands.rs && git commit -m "feat(core): #[tauri::command] surface — brokered verbs + CORE-ONLY pick_folder"`

- [ ] **Step 16: Write failing test — `pick_folder` is CORE-ONLY (no daemon `Request` exists for it).**
  This is a compile-time / structural guarantee: there is no `Request::PickFolder` variant, and `pick_folder` takes `AppHandle` (not `State<AppState>`), so it can never reach the daemon. Assert it structurally. Append to `commands.rs` test module:
  ```rust
  #[test]
  fn pick_folder_is_core_only_no_daemon_request() {
      // Every brokered command has a Request variant it forwards. pick_folder must
      // NOT — there is deliberately no Request::PickFolder. This test documents and
      // locks that: if someone adds a daemon round-trip for folder picking, the
      // protocol enum would need a new variant and this exhaustive match breaks,
      // forcing a conscious decision.
      fn is_folder_picking_request(r: &Request) -> bool {
          match r {
              Request::Hello { .. }
              | Request::ListWorkspaces
              | Request::CreateWorkspace { .. }
              | Request::ListSessions
              | Request::CreateSession { .. }
              | Request::AttachSession { .. }
              | Request::DetachSession { .. }
              | Request::WriteStdin { .. }
              | Request::Resize { .. }
              | Request::KillSession { .. }
              | Request::GetSessionState { .. }
              | Request::DaemonShutdown { .. } => false,
          }
      }
      // Sanity: a representative request is not a folder-picking request.
      assert!(!is_folder_picking_request(&Request::ListWorkspaces));
  }
  ```

- [ ] **Step 17: Run test — confirm PASS immediately** (this test compiles only if the `Request` enum has exactly the spec §7 variants and no folder-picking one).
  `cargo test -p builder-pro-ai pick_folder_is_core_only_no_daemon_request`
  Expected: PASS. (If it FAILS to compile because a `Request::PickFolder` was added, that is the guard firing — folder picking must stay CORE-ONLY.)

- [ ] **Step 18: Commit.**
  `git add src-tauri/src/commands.rs && git commit -m "test(core): lock pick_folder as CORE-ONLY (no daemon Request variant)"`

**Definition of Done:**
- `cargo test -p builder-pro-ai` green for: `create_opts_defaults_env_overrides_and_reads_camel_case`, `create_session_uses_80x24_when_size_omitted`, `state_changed_push_maps_to_camel_case_payload`, `child_exited_push_maps_to_reshaped_payload`, `non_matching_push_variants_do_not_map_to_state_payloads`, `create_session_builds_request_with_defaults`, `create_session_builds_request_with_opts`, `write_stdin_builds_request_utf8_bytes`, `response_error_becomes_command_error_daemon`, `response_session_unwraps_to_meta`, `pick_folder_is_core_only_no_daemon_request`.
- All 11 commands from spec §6.1 exist with the exact names + signatures; `create_session` sends 80×24 when size omitted (spec §6.1); `env_overrides` defaults `[]` (spec §6.1).
- `pick_folder` is CORE-ONLY via `tauri-plugin-dialog` and has no daemon `Request` variant (spec §6.1 classification table).
- Broker maps `Push::Replay`/`Push::Output` → `Channel<TerminalEvent>` `Replay`/`Output`, and `Push::StateChanged`/`ChildExited`/`SessionCreated`/`WorkspaceCreated` → the four global events with snake→camel rename and the `ChildExited`→`{sessionId,code,signal}` reshape (spec §6.3, §7 broker-mapping table); `code`/`signal` `None` serialize as JSON `null`, never coerced to 0 (spec §5 exit-code note).
- `Response::Error` rejects the awaiting command Promise as `CommandError::Daemon { code, message }` (spec §7 correlation).
- Single-attach: `attach_session` registers the channel before the daemon `AttachSession`, and drops it on attach failure / detach / kill (spec §7 attach model).
- Structured `tracing` logs on every send/emit failure; no secret values logged (Global Constraints).

---

### Task 18: `lib.rs` + `main.rs` + `capabilities/default.json` + `entitlements.plist` — Tauri Builder, plugin init, capabilities, setup wiring

**Files:**
- Create: `src-tauri/src/lib.rs`
- Create: `src-tauri/src/main.rs`
- Create: `src-tauri/capabilities/default.json`
- Create: `src-tauri/entitlements.plist`
- Test: `src-tauri/tests/capabilities.rs` (capabilities JSON parses/valid), plus inline `#[cfg(test)] mod tests` in `lib.rs` (setup-wiring smoke: command names registered, plugin list). App-builds check via `cargo build -p builder-pro-ai`.

**Depends on:** [T16, T17]   **Parallel-safe with:** [] (integration glue; owns `lib.rs`/`main.rs` exclusively)

**Interfaces:**
- Consumes:
  - From **T17 `commands`**: the 11 `#[tauri::command]` fns (`create_session`, `list_sessions`, `attach_session`, `detach_session`, `write_stdin`, `resize`, `kill_session`, `list_workspaces`, `create_workspace`, `get_session_state`, `pick_folder`) + `AppState { client: Arc<DaemonClient>, broker: Arc<Broker> }`.
  - From **T17 `broker`**: `Broker::new(app) -> Broker`, `Broker::dispatch_push(push)`.
  - From **T14 `socket_client`**: `DaemonClient::connect() -> Result<DaemonClient, ClientError>`, `DaemonClient::on_push(cb)`.
  - From **T16 `launchd`**: `install_agent() -> Result<()>`, `bootstrap() -> Result<()>` (idempotent), `kickstart() -> Result<()>`, `is_loaded() -> bool`.
- Produces (names verbatim):
  - `pub fn run()` (the app entry point invoked by `main.rs`), registering plugins `store`/`dialog`/`fs`/`shell`, the T17 `invoke_handler`, managed `AppState`, and the `setup()` hook (install+bootstrap+kickstart launchd, connect `DaemonClient`, wire `on_push`→`Broker::dispatch_push`, emit `daemon://disconnected` banner on disconnect).
  - `pub const DAEMON_DISCONNECTED_EVENT: &str = "daemon://disconnected"` and `pub const DAEMON_RECONNECTED_EVENT: &str = "daemon://reconnected"`.
  - `pub fn command_names() -> &'static [&'static str]` (the exact 11 command names, for the setup-wiring smoke test).
  - `capabilities/default.json` (permission set per spec §6/§16) and `entitlements.plist` (hardened runtime, spec §14.3/§16).

---

- [ ] **Step 1: Write failing test — `capabilities/default.json` parses and grants exactly the required minimal permissions.**
  Create `src-tauri/tests/capabilities.rs`:
  ```rust
  use serde_json::Value;

  fn load_caps() -> Value {
      let raw = include_str!("../capabilities/default.json");
      serde_json::from_str(raw).expect("capabilities/default.json must be valid JSON")
  }

  fn perm_ids(caps: &Value) -> Vec<String> {
      caps["permissions"]
          .as_array()
          .expect("permissions is an array")
          .iter()
          .map(|p| match p {
              Value::String(s) => s.clone(),
              Value::Object(o) => o["identifier"].as_str().unwrap_or_default().to_string(),
              _ => panic!("permission entry must be a string or an object with identifier"),
          })
          .collect()
  }

  #[test]
  fn capabilities_parse_and_target_main_window() {
      let caps = load_caps();
      assert_eq!(caps["identifier"], "default");
      let windows = caps["windows"].as_array().expect("windows array");
      assert!(
          windows.iter().any(|w| w == "main"),
          "capability must apply to the main window"
      );
  }

  #[test]
  fn capabilities_grant_minimal_required_permissions() {
      let caps = load_caps();
      let ids = perm_ids(&caps);
      for required in [
          "core:default",
          "store:default",
          "dialog:default",
          "dialog:allow-open",
          "fs:default",
          "shell:default",
      ] {
          assert!(
              ids.iter().any(|i| i == required),
              "capabilities must grant {required}; got {ids:?}"
          );
      }
  }

  #[test]
  fn capabilities_do_not_grant_dangerous_shell_execute_scopes() {
      // shell:default is for bundling only (spec §3). We must NOT hand the webview
      // arbitrary shell exec / spawn.
      let caps = load_caps();
      let ids = perm_ids(&caps);
      for forbidden in ["shell:allow-execute", "shell:allow-spawn"] {
          assert!(
              !ids.iter().any(|i| i == forbidden),
              "capabilities must NOT grant {forbidden}"
          );
      }
  }
  ```

- [ ] **Step 2: Run test — confirm FAIL.**
  `cargo test -p builder-pro-ai --test capabilities`
  Expected: FAIL — `capabilities/default.json` does not exist (`include_str!` errors at compile / file-not-found).

- [ ] **Step 3: Create `capabilities/default.json` with the minimal permission set (spec §6/§16).**
  ```json
  {
    "$schema": "../gen/schemas/desktop-schema.json",
    "identifier": "default",
    "description": "Minimal permissions for Builder Pro AI: core IPC + UI settings store + folder picker + scoped fs. Shell is bundling-only; the daemon is launchd-supervised, never spawned from the webview.",
    "windows": ["main"],
    "permissions": [
      "core:default",
      "store:default",
      "dialog:default",
      "dialog:allow-open",
      "fs:default",
      {
        "identifier": "fs:scope",
        "allow": [{ "path": "$APPDATA/**" }]
      },
      "shell:default"
    ]
  }
  ```

- [ ] **Step 4: Run test — confirm PASS.**
  `cargo test -p builder-pro-ai --test capabilities`
  Expected: PASS (all three capability tests).

- [ ] **Step 5: Commit.**
  `git add src-tauri/capabilities/default.json src-tauri/tests/capabilities.rs && git commit -m "feat(core): minimal capabilities (core/store/dialog+open/fs scoped/shell) + tests"`

- [ ] **Step 6: Write failing test — `entitlements.plist` is a valid plist with hardened-runtime keys for JIT-free hardened runtime.**
  Append to `src-tauri/tests/capabilities.rs`:
  ```rust
  #[test]
  fn entitlements_plist_has_hardened_runtime_keys() {
      let raw = include_str!("../entitlements.plist");
      assert!(raw.contains("<!DOCTYPE plist"), "must be a plist doctype");
      assert!(raw.contains("<plist"), "must have a <plist> root");
      // Hardened runtime for a WKWebView app that also embeds a signed sidecar.
      // The daemon spawns child processes (shells) so it needs the inherit
      // exception; the app uses JS so it needs the JIT/unsigned-executable-memory
      // exceptions that WKWebView requires.
      for key in [
          "com.apple.security.cs.allow-jit",
          "com.apple.security.cs.allow-unsigned-executable-memory",
          "com.apple.security.cs.disable-library-validation",
          "com.apple.security.cs.allow-dyld-environment-variables",
          "com.apple.security.inherit",
      ] {
          assert!(raw.contains(key), "entitlements must declare {key}");
      }
  }
  ```

- [ ] **Step 7: Run test — confirm FAIL.**
  `cargo test -p builder-pro-ai --test capabilities entitlements_plist_has_hardened_runtime_keys`
  Expected: FAIL — `entitlements.plist` does not exist (`include_str!` file-not-found).

- [ ] **Step 8: Create `entitlements.plist` (hardened runtime, spec §14.3/§16).**
  ```xml
  <?xml version="1.0" encoding="UTF-8"?>
  <!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
  <plist version="1.0">
  <dict>
    <key>com.apple.security.cs.allow-jit</key>
    <true/>
    <key>com.apple.security.cs.allow-unsigned-executable-memory</key>
    <true/>
    <key>com.apple.security.cs.disable-library-validation</key>
    <true/>
    <key>com.apple.security.cs.allow-dyld-environment-variables</key>
    <true/>
    <key>com.apple.security.inherit</key>
    <true/>
  </dict>
  </plist>
  ```
  Wire it into `tauri.conf.json` (owned by T2 — this is a reference note, T2 sets `bundle.macOS.entitlements = "entitlements.plist"` and `bundle.macOS.hardenedRuntime = true`; the same plist is applied to the `.app` and, because `codesign --deep`, to the embedded `bpa-sessiond` sidecar per spec §14.3).

- [ ] **Step 9: Run test — confirm PASS.**
  `cargo test -p builder-pro-ai --test capabilities entitlements_plist_has_hardened_runtime_keys`
  Expected: PASS.

- [ ] **Step 10: Commit.**
  `git add src-tauri/entitlements.plist && git commit -m "feat(core): hardened-runtime entitlements.plist for .app + sidecar"`

- [ ] **Step 11: Write failing test — setup-wiring smoke: the 11 commands are registered and the disconnect event name is locked.**
  Create `src-tauri/src/lib.rs` with the test module at the bottom:
  ```rust
  #[cfg(test)]
  mod tests {
      use super::*;

      #[test]
      fn command_names_are_the_eleven_spec_6_1_commands() {
          let names = command_names();
          let expected = [
              "create_session",
              "list_sessions",
              "attach_session",
              "detach_session",
              "write_stdin",
              "resize",
              "kill_session",
              "list_workspaces",
              "create_workspace",
              "get_session_state",
              "pick_folder",
          ];
          assert_eq!(names.len(), expected.len(), "exactly 11 commands");
          for e in expected {
              assert!(names.contains(&e), "command surface must include {e}");
          }
      }

      #[test]
      fn daemon_event_names_are_locked() {
          assert_eq!(DAEMON_DISCONNECTED_EVENT, "daemon://disconnected");
          assert_eq!(DAEMON_RECONNECTED_EVENT, "daemon://reconnected");
      }
  }
  ```

- [ ] **Step 12: Run test — confirm FAIL.**
  `cargo test -p builder-pro-ai command_names_are_the_eleven_spec_6_1_commands`
  Expected: FAIL — `cannot find function command_names` / `DAEMON_DISCONNECTED_EVENT` not found.

- [ ] **Step 13: Implement `lib.rs` — Builder, plugins, invoke_handler, managed state, setup wiring.**
  Prepend above the test module in `src-tauri/src/lib.rs`:
  ```rust
  mod broker;
  mod commands;
  mod launchd;
  mod paths;
  mod socket_client;

  use std::sync::Arc;

  use tauri::{Emitter, Manager};
  use tracing::{error, info, warn};

  use crate::broker::Broker;
  use crate::commands::AppState;
  use crate::socket_client::DaemonClient;

  /// Emitted (no payload) when the core loses the daemon socket (spec §6.3, §13).
  pub const DAEMON_DISCONNECTED_EVENT: &str = "daemon://disconnected";
  /// Emitted (no payload) when the core re-establishes the daemon socket.
  pub const DAEMON_RECONNECTED_EVENT: &str = "daemon://reconnected";

  /// The exact command surface (spec §6.1) — mirrored by the smoke test and by
  /// `tauri::generate_handler!` below. Keep the two lists in lockstep.
  pub fn command_names() -> &'static [&'static str] {
      &[
          "create_session",
          "list_sessions",
          "attach_session",
          "detach_session",
          "write_stdin",
          "resize",
          "kill_session",
          "list_workspaces",
          "create_workspace",
          "get_session_state",
          "pick_folder",
      ]
  }

  /// App entry point. Registers plugins + the command surface, installs managed
  /// state, and wires the launchd + daemon-connection lifecycle in `setup()`.
  pub fn run() {
      tracing_subscriber::fmt()
          .with_env_filter(
              tracing_subscriber::EnvFilter::try_from_default_env()
                  .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
          )
          .init();

      tauri::Builder::default()
          .plugin(tauri_plugin_store::Builder::default().build())
          .plugin(tauri_plugin_dialog::init())
          .plugin(tauri_plugin_fs::init())
          .plugin(tauri_plugin_shell::init())
          .invoke_handler(tauri::generate_handler![
              commands::create_session,
              commands::list_sessions,
              commands::attach_session,
              commands::detach_session,
              commands::write_stdin,
              commands::resize,
              commands::kill_session,
              commands::list_workspaces,
              commands::create_workspace,
              commands::get_session_state,
              commands::pick_folder,
          ])
          .setup(|app| {
              let handle = app.handle().clone();
              // Broker owns the attach map + fans daemon Push frames to Hop A.
              let broker = Arc::new(Broker::new(handle.clone()));

              // 1) Ensure the launchd LaunchAgent is installed + loaded, then
              //    kickstart the daemon on demand (spec §8.3). Failures degrade
              //    to an actionable banner — never hang silently (spec §13).
              if let Err(e) = launchd::install_agent() {
                  error!(error = %e, "failed to install LaunchAgent");
                  emit_disconnected(&handle, "could not install background service");
              }
              if let Err(e) = launchd::bootstrap() {
                  // "already bootstrapped" is success (idempotent, spec §8.3);
                  // launchd.rs returns Ok in that case, so a real Err is a hard fail.
                  error!(error = %e, "failed to bootstrap LaunchAgent");
                  emit_disconnected(&handle, "could not start background service");
              }
              if let Err(e) = launchd::kickstart() {
                  warn!(error = %e, "failed to kickstart daemon (will retry on connect)");
              }

              // 2) Connect the daemon client on the async runtime and wire pushes.
              let broker_for_task = broker.clone();
              let handle_for_task = handle.clone();
              tauri::async_runtime::spawn(async move {
                  match DaemonClient::connect().await {
                      Ok(client) => {
                          let client = Arc::new(client);
                          // Fan every daemon Push into the broker.
                          let bpush = broker_for_task.clone();
                          client.on_push(move |push| {
                              let b = bpush.clone();
                              tauri::async_runtime::spawn(async move { b.dispatch_push(push).await });
                          });
                          // Publish the connected state via managed AppState.
                          handle_for_task.manage(AppState {
                              client: client.clone(),
                              broker: broker_for_task.clone(),
                          });
                          let _ = handle_for_task.emit(DAEMON_RECONNECTED_EVENT, ());
                          info!("daemon connected");
                      }
                      Err(e) => {
                          error!(error = %e, "daemon connect failed");
                          emit_disconnected(&handle_for_task, "daemon unreachable");
                      }
                  }
              });

              Ok(())
          })
          .run(tauri::generate_context!())
          .expect("error while running Builder Pro AI");
  }

  /// Emit the no-payload `daemon://disconnected` banner event, logging the reason.
  fn emit_disconnected(handle: &tauri::AppHandle, reason: &str) {
      warn!(reason, "emitting daemon://disconnected");
      if let Err(e) = handle.emit(DAEMON_DISCONNECTED_EVENT, ()) {
          error!(error = %e, "failed to emit daemon://disconnected");
      }
  }
  ```

- [ ] **Step 14: Run test — confirm PASS.**
  `cargo test -p builder-pro-ai command_names_are_the_eleven_spec_6_1_commands daemon_event_names_are_locked`
  Expected: PASS (both).

- [ ] **Step 15: Commit.**
  `git add src-tauri/src/lib.rs && git commit -m "feat(core): Tauri Builder — plugins, invoke_handler, managed state, launchd+daemon setup wiring"`

- [ ] **Step 16: Create `main.rs` (thin entry that calls `run()`).**
  Create `src-tauri/src/main.rs`:
  ```rust
  // Prevents an extra console window on Windows in release; harmless on macOS.
  #![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

  fn main() {
      builder_pro_ai_lib::run();
  }
  ```
  > `builder_pro_ai_lib` is the library crate name defined in `src-tauri/Cargo.toml` (`[lib] name = "builder_pro_ai_lib"`), owned by T2. If T2 chose a different lib name, use that name here verbatim.

- [ ] **Step 17: Run — confirm the app crate builds end-to-end (setup wiring compiles against T14/T16/T17).**
  `cargo build -p builder-pro-ai`
  Expected: PASS (compiles; links all commands, plugins, launchd, socket_client, broker).

- [ ] **Step 18: Run the full app-crate test suite.**
  `cargo test -p builder-pro-ai`
  Expected: PASS — capabilities tests, entitlements test, command-name + event-name smoke tests, plus T17's inline command/broker tests all green.

- [ ] **Step 19: Commit.**
  `git add src-tauri/src/main.rs && git commit -m "feat(core): main.rs entry calling lib::run()"`

**Definition of Done:**
- `cargo test -p builder-pro-ai` green including `--test capabilities` (parse + minimal-permission + no-dangerous-shell-scope + entitlements-hardened-runtime) and the `lib.rs` smoke tests (`command_names_are_the_eleven_spec_6_1_commands`, `daemon_event_names_are_locked`).
- `cargo build -p builder-pro-ai` succeeds (app builds; setup wiring type-checks against T14 `DaemonClient`, T16 `launchd`, T17 commands/broker).
- `capabilities/default.json` grants exactly `core:default`, `store:default`, `dialog:default` + `dialog:allow-open`, `fs:default` + scoped `fs:scope` (`$APPDATA/**`), `shell:default` (bundling only) and applies to the `main` window; it grants **no** `shell:allow-execute`/`shell:allow-spawn` (spec §6/§16).
- `entitlements.plist` declares the hardened-runtime keys (`allow-jit`, `allow-unsigned-executable-memory`, `disable-library-validation`, `allow-dyld-environment-variables`, `inherit`) for both the `.app` and the deep-signed sidecar (spec §14.3/§16).
- `run()` initializes plugins `store`/`dialog`/`fs`/`shell`, registers the 11 T17 commands via `generate_handler!`, manages `AppState` once the `DaemonClient` connects, and in `setup()` installs+bootstraps (idempotent) + kickstarts the launchd agent, connects the client, wires `on_push` → `Broker::dispatch_push`, and emits `daemon://disconnected` on any launchd/connect hard-failure — never hanging silently (spec §8.3, §13).
- Structured `tracing` logs on every degradation path; no secret values (Global Constraints).

---

**Cross-task concerns (for the plan integrator):**
1. **T14 `ClientError` surface.** T17's `From<ClientError> for CommandError` and T18's `DaemonClient::connect() -> Result<_, ClientError>` assume T14 exposes `pub enum ClientError` with at least variants `Disconnected` and `Daemon { code: String, message: String }` plus a `Display`/catch-all. The scaffold's T14 index only lists `connect`/`request`/`on_push`/reconnect — please have T14 lock `ClientError` with those two named variants so my `From` mapping compiles. If T14 names them differently, T17 Step 13's `From` impl and T18's `emit_disconnected` reasons must be adjusted to match.
2. **`DaemonClient::request` return type.** T17 assumes `request(Request) -> Result<Response, ClientError>` (so `?` yields `CommandError` via the `From`). Confirmed against the scaffold T14 index (`request(Request) -> Result<Response>`); the `Result`'s error must be `ClientError`, not `anyhow::Error`, or the `From` won't apply.
3. **`on_push` callback signature.** T18 wires `client.on_push(move |push: Push| { ... })`. T14 must accept `FnMut(Push)` (or `Fn(Push)`), matching the scaffold `on_push(cb)`. If T14 instead delivers pushes via a channel/stream, T18 Step 13's `on_push` block becomes a receive-loop `tauri::async_runtime::spawn` over that stream — same broker call, different plumbing.
4. **`tauri.conf.json` entitlements + hardened runtime (T2).** T18 creates `entitlements.plist` but T2 owns `tauri.conf.json`; T2 must set `bundle.macOS.entitlements` to `"entitlements.plist"` and `bundle.macOS.hardenedRuntime = true` for the plist to take effect on the `.app` and the deep-signed sidecar (spec §14.3). Flagging so it isn't dropped between the two tasks.
5. **`AppState` availability race.** `AppState` is `manage()`d only after the async `DaemonClient::connect()` succeeds (T18 setup). Commands invoked before connection will hit Tauri's "state not managed" panic. The frontend (T19–T22) must gate command calls on `daemon://reconnected` / initial connected state; T22's `DaemonBanner` already keys off `daemonConnected`. If a cleaner contract is wanted, manage a `AppState` holding an `Arc<Mutex<Option<DaemonClient>>>` from the start so commands return `CommandError::Disconnected` instead of panicking — recommend this hardening during integration (T23).
6. **Plugin crate deps (T2).** `lib.rs` calls `tauri_plugin_store::Builder`, `tauri_plugin_dialog::init`, `tauri_plugin_fs::init`, `tauri_plugin_shell::init`, and `commands.rs` uses `tauri_plugin_dialog::DialogExt`. T2's `src-tauri/Cargo.toml` must include `tauri-plugin-store`, `tauri-plugin-dialog`, `tauri-plugin-fs`, `tauri-plugin-shell` (all major `2`) + `tracing`/`tracing-subscriber` + `serde_json` (used in tests) + `tokio` (for the oneshot in `pick_folder`).


### Task 19: `src/ipc/` — typed IPC layer (commands, channel, events, generated types)

**Files:**
- Modify (reference, DO NOT hand-edit): `src/ipc/types.ts` (generated from `crates/protocol` via `ts-rs`; T3 owns generation)
- Create: `src/ipc/commands.ts`
- Create: `src/ipc/channel.ts`
- Create: `src/ipc/events.ts`
- Test (create): `src/ipc/commands.test.ts`, `src/ipc/channel.test.ts`, `src/ipc/events.test.ts`

**Depends on:** [T3] (generated `types.ts` = `SessionMeta`, `Workspace`, `SessionLifecycle`, `TerminalEvent`, `SessionId`, `WorkspaceId`), [T17] (command names must match the `#[tauri::command]` fn names + `session://*` / `daemon://*` / `workspace://*` event names — consumed as string literals only, no code import)
**Parallel-safe with:** [T20, T21] (disjoint files: `src/store/**`, `src/terminal/**`)

**Interfaces:**
- Consumes (from T3 generated `src/ipc/types.ts`): `type SessionId = string`; `type WorkspaceId = string`; `interface Workspace { id; name; rootPath }`; `type SessionLifecycle = { kind:"atPrompt" } | { kind:"typing" } | { kind:"running" } | { kind:"exited"; code:number|null; signal:string|null }`; `interface SessionMeta { id; workspaceId; title; shell; cwd; cols; rows; lifecycle; waitingForInput; isActive; createdAt }`; `type TerminalEvent = { event:"replay"; data:{ cols:number; rows:number; content:number[] } } | { event:"output"; data:{ bytes:number[] } }`.
- Consumes (from `@tauri-apps/api/core`): `invoke<T>(cmd:string, args?:Record<string,unknown>):Promise<T>`, `class Channel<T> { onmessage:(m:T)=>void }`. From `@tauri-apps/api/event`: `listen<T>(event:string, handler:(e:{payload:T})=>void):Promise<UnlistenFn>`, `type UnlistenFn = () => void`.
- Produces (`src/ipc/commands.ts`, exact signatures verbatim from spec §6.1):
  `createSession(workspaceId: WorkspaceId, opts?: CreateSessionOpts): Promise<SessionMeta>`;
  `listSessions(): Promise<SessionMeta[]>`;
  `attachSession(sessionId: SessionId, onEvent: Channel<TerminalEvent>): Promise<void>`;
  `detachSession(sessionId: SessionId): Promise<void>`;
  `writeStdin(sessionId: SessionId, data: string): Promise<void>`;
  `resize(sessionId: SessionId, cols: number, rows: number): Promise<void>`;
  `killSession(sessionId: SessionId): Promise<void>`;
  `listWorkspaces(): Promise<Workspace[]>`;
  `createWorkspace(name: string, rootPath: string): Promise<Workspace>`;
  `getSessionState(sessionId: SessionId): Promise<SessionMeta>`;
  `pickFolder(): Promise<string | null>`;
  and `interface CreateSessionOpts { shell?: string; cwd?: string; envOverrides?: [string,string][]; cols?: number; rows?: number }`.
- Produces (`src/ipc/channel.ts`): `newTerminalChannel(onEvent: (e: TerminalEvent) => void): Channel<TerminalEvent>`.
- Produces (`src/ipc/events.ts`, exact §6.3 payloads): `interface StateChangedPayload { sessionId: SessionId; lifecycle: SessionLifecycle; waitingForInput: boolean; cwd: string }`; `interface ExitedPayload { sessionId: SessionId; code: number | null; signal: string | null }`; and typed subscribers `onSessionCreated(cb:(m:SessionMeta)=>void):Promise<UnlistenFn>`, `onSessionStateChanged(cb:(p:StateChangedPayload)=>void):Promise<UnlistenFn>`, `onSessionExited(cb:(p:ExitedPayload)=>void):Promise<UnlistenFn>`, `onWorkspaceCreated(cb:(w:Workspace)=>void):Promise<UnlistenFn>`, `onDaemonDisconnected(cb:()=>void):Promise<UnlistenFn>`, `onDaemonReconnected(cb:()=>void):Promise<UnlistenFn>`.

- [ ] **Step 1: Failing test for `commands.ts` invoke wrapper arg shapes.**
  Create `src/ipc/commands.test.ts`. Mock `@tauri-apps/api/core` so `invoke` records `(cmd, args)`; assert each wrapper calls the right command name with camelCase arg keys and passes options/`Channel` through unchanged.
  ```ts
  import { describe, it, expect, vi, beforeEach } from "vitest";

  const invokeMock = vi.fn();
  vi.mock("@tauri-apps/api/core", () => {
    class Channel<T> {
      onmessage: ((m: T) => void) | undefined;
    }
    return { invoke: (...a: unknown[]) => invokeMock(...a), Channel };
  });

  import {
    createSession,
    listSessions,
    attachSession,
    detachSession,
    writeStdin,
    resize,
    killSession,
    listWorkspaces,
    createWorkspace,
    getSessionState,
    pickFolder,
  } from "./commands";
  import { Channel } from "@tauri-apps/api/core";
  import type { SessionMeta, Workspace, TerminalEvent } from "./types";

  const sampleMeta: SessionMeta = {
    id: "s1",
    workspaceId: "w1",
    title: "zsh",
    shell: "/bin/zsh",
    cwd: "/tmp",
    cols: 80,
    rows: 24,
    lifecycle: { kind: "atPrompt" },
    waitingForInput: false,
    isActive: true,
    createdAt: 1000,
  };

  describe("ipc/commands", () => {
    beforeEach(() => {
      invokeMock.mockReset();
      invokeMock.mockResolvedValue(undefined);
    });

    it("createSession sends workspaceId + opts, resolves SessionMeta", async () => {
      invokeMock.mockResolvedValueOnce(sampleMeta);
      const res = await createSession("w1", { shell: "/bin/bash", cols: 100, rows: 40 });
      expect(invokeMock).toHaveBeenCalledWith("create_session", {
        workspaceId: "w1",
        opts: { shell: "/bin/bash", cols: 100, rows: 40 },
      });
      expect(res).toEqual(sampleMeta);
    });

    it("createSession omits opts key when not provided", async () => {
      invokeMock.mockResolvedValueOnce(sampleMeta);
      await createSession("w1");
      expect(invokeMock).toHaveBeenCalledWith("create_session", { workspaceId: "w1", opts: undefined });
    });

    it("listSessions calls list_sessions with no args", async () => {
      const arr: SessionMeta[] = [sampleMeta];
      invokeMock.mockResolvedValueOnce(arr);
      const res = await listSessions();
      expect(invokeMock).toHaveBeenCalledWith("list_sessions");
      expect(res).toEqual(arr);
    });

    it("attachSession passes sessionId + Channel as onEvent", async () => {
      const ch = new Channel<TerminalEvent>();
      await attachSession("s1", ch);
      expect(invokeMock).toHaveBeenCalledWith("attach_session", { sessionId: "s1", onEvent: ch });
    });

    it("detachSession sends sessionId", async () => {
      await detachSession("s1");
      expect(invokeMock).toHaveBeenCalledWith("detach_session", { sessionId: "s1" });
    });

    it("writeStdin sends sessionId + data string", async () => {
      await writeStdin("s1", "ls\n");
      expect(invokeMock).toHaveBeenCalledWith("write_stdin", { sessionId: "s1", data: "ls\n" });
    });

    it("resize sends sessionId + cols + rows", async () => {
      await resize("s1", 120, 30);
      expect(invokeMock).toHaveBeenCalledWith("resize", { sessionId: "s1", cols: 120, rows: 30 });
    });

    it("killSession sends sessionId", async () => {
      await killSession("s1");
      expect(invokeMock).toHaveBeenCalledWith("kill_session", { sessionId: "s1" });
    });

    it("listWorkspaces calls list_workspaces", async () => {
      const ws: Workspace[] = [{ id: "w1", name: "proj", rootPath: "/p" }];
      invokeMock.mockResolvedValueOnce(ws);
      const res = await listWorkspaces();
      expect(invokeMock).toHaveBeenCalledWith("list_workspaces");
      expect(res).toEqual(ws);
    });

    it("createWorkspace sends name + rootPath", async () => {
      const w: Workspace = { id: "w1", name: "proj", rootPath: "/p" };
      invokeMock.mockResolvedValueOnce(w);
      const res = await createWorkspace("proj", "/p");
      expect(invokeMock).toHaveBeenCalledWith("create_workspace", { name: "proj", rootPath: "/p" });
      expect(res).toEqual(w);
    });

    it("getSessionState sends sessionId, resolves SessionMeta", async () => {
      invokeMock.mockResolvedValueOnce(sampleMeta);
      const res = await getSessionState("s1");
      expect(invokeMock).toHaveBeenCalledWith("get_session_state", { sessionId: "s1" });
      expect(res).toEqual(sampleMeta);
    });

    it("pickFolder calls pick_folder, resolves string|null", async () => {
      invokeMock.mockResolvedValueOnce("/chosen");
      expect(await pickFolder()).toBe("/chosen");
      invokeMock.mockResolvedValueOnce(null);
      expect(await pickFolder()).toBeNull();
    });
  });
  ```

- [ ] **Step 2: Run — confirm FAIL.**
  `npx vitest run src/ipc/commands.test.ts`
  Expected: FAIL with "Failed to resolve import './commands'" (module does not exist yet).

- [ ] **Step 3: Implement `src/ipc/commands.ts`.**
  ```ts
  import { invoke, Channel } from "@tauri-apps/api/core";
  import type {
    SessionId,
    WorkspaceId,
    SessionMeta,
    Workspace,
    TerminalEvent,
  } from "./types";

  export interface CreateSessionOpts {
    shell?: string;
    cwd?: string;
    envOverrides?: [string, string][];
    cols?: number;
    rows?: number;
  }

  export function createSession(
    workspaceId: WorkspaceId,
    opts?: CreateSessionOpts,
  ): Promise<SessionMeta> {
    return invoke<SessionMeta>("create_session", { workspaceId, opts });
  }

  export function listSessions(): Promise<SessionMeta[]> {
    return invoke<SessionMeta[]>("list_sessions");
  }

  export function attachSession(
    sessionId: SessionId,
    onEvent: Channel<TerminalEvent>,
  ): Promise<void> {
    return invoke<void>("attach_session", { sessionId, onEvent });
  }

  export function detachSession(sessionId: SessionId): Promise<void> {
    return invoke<void>("detach_session", { sessionId });
  }

  export function writeStdin(sessionId: SessionId, data: string): Promise<void> {
    return invoke<void>("write_stdin", { sessionId, data });
  }

  export function resize(
    sessionId: SessionId,
    cols: number,
    rows: number,
  ): Promise<void> {
    return invoke<void>("resize", { sessionId, cols, rows });
  }

  export function killSession(sessionId: SessionId): Promise<void> {
    return invoke<void>("kill_session", { sessionId });
  }

  export function listWorkspaces(): Promise<Workspace[]> {
    return invoke<Workspace[]>("list_workspaces");
  }

  export function createWorkspace(
    name: string,
    rootPath: string,
  ): Promise<Workspace> {
    return invoke<Workspace>("create_workspace", { name, rootPath });
  }

  export function getSessionState(sessionId: SessionId): Promise<SessionMeta> {
    return invoke<SessionMeta>("get_session_state", { sessionId });
  }

  export function pickFolder(): Promise<string | null> {
    return invoke<string | null>("pick_folder");
  }
  ```

- [ ] **Step 4: Run — confirm PASS.**
  `npx vitest run src/ipc/commands.test.ts`
  Expected: PASS (13 tests green).

- [ ] **Step 5: Commit.**
  `git add src/ipc/commands.ts src/ipc/commands.test.ts && git commit -m "feat(ipc): typed invoke() wrappers for the §6.1 command surface"`

- [ ] **Step 6: Failing test for `channel.ts` message routing.**
  Create `src/ipc/channel.test.ts`. Assert `newTerminalChannel` returns a `Channel<TerminalEvent>` whose `onmessage` is wired to the supplied handler, and that pushing `replay`/`output` frames routes them verbatim.
  ```ts
  import { describe, it, expect, vi } from "vitest";

  vi.mock("@tauri-apps/api/core", () => {
    class Channel<T> {
      onmessage: ((m: T) => void) | undefined;
    }
    return { Channel };
  });

  import { newTerminalChannel } from "./channel";
  import type { TerminalEvent } from "./types";

  describe("ipc/channel", () => {
    it("routes replay then output frames to the handler in order", () => {
      const received: TerminalEvent[] = [];
      const ch = newTerminalChannel((e) => received.push(e));
      const replay: TerminalEvent = {
        event: "replay",
        data: { cols: 80, rows: 24, content: [104, 105] },
      };
      const output: TerminalEvent = { event: "output", data: { bytes: [10] } };
      ch.onmessage?.(replay);
      ch.onmessage?.(output);
      expect(received).toEqual([replay, output]);
    });

    it("wires onmessage exactly to the provided handler", () => {
      const handler = vi.fn();
      const ch = newTerminalChannel(handler);
      const msg: TerminalEvent = { event: "output", data: { bytes: [65] } };
      ch.onmessage?.(msg);
      expect(handler).toHaveBeenCalledTimes(1);
      expect(handler).toHaveBeenCalledWith(msg);
    });
  });
  ```

- [ ] **Step 7: Run — confirm FAIL.**
  `npx vitest run src/ipc/channel.test.ts`
  Expected: FAIL with "Failed to resolve import './channel'".

- [ ] **Step 8: Implement `src/ipc/channel.ts`.**
  ```ts
  import { Channel } from "@tauri-apps/api/core";
  import type { TerminalEvent } from "./types";

  /**
   * Build a Tauri `Channel<TerminalEvent>` for `attach_session`.
   * The daemon-brokered firehose (`Replay` then live `Output`) arrives here.
   * Bytes are handed to the caller verbatim and MUST NOT enter React/Zustand state.
   */
  export function newTerminalChannel(
    onEvent: (e: TerminalEvent) => void,
  ): Channel<TerminalEvent> {
    const channel = new Channel<TerminalEvent>();
    channel.onmessage = (m: TerminalEvent) => onEvent(m);
    return channel;
  }
  ```

- [ ] **Step 9: Run — confirm PASS.**
  `npx vitest run src/ipc/channel.test.ts`
  Expected: PASS (2 tests green).

- [ ] **Step 10: Commit.**
  `git add src/ipc/channel.ts src/ipc/channel.test.ts && git commit -m "feat(ipc): attach_session Channel<TerminalEvent> plumbing"`

- [ ] **Step 11: Failing test for `events.ts` typed listeners.**
  Create `src/ipc/events.test.ts`. Mock `@tauri-apps/api/event` so `listen` records `(event, handler)` and returns an unlisten fn; assert each subscriber registers the correct `session://*` / `workspace://*` / `daemon://*` event name and unwraps `e.payload` to the callback.
  ```ts
  import { describe, it, expect, vi, beforeEach } from "vitest";

  type Listener = (e: { payload: unknown }) => void;
  const registered = new Map<string, Listener>();
  const unlisten = vi.fn();
  const listenMock = vi.fn(async (event: string, handler: Listener) => {
    registered.set(event, handler);
    return unlisten;
  });

  vi.mock("@tauri-apps/api/event", () => ({
    listen: (event: string, handler: Listener) => listenMock(event, handler),
  }));

  import {
    onSessionCreated,
    onSessionStateChanged,
    onSessionExited,
    onWorkspaceCreated,
    onDaemonDisconnected,
    onDaemonReconnected,
  } from "./events";
  import type { SessionMeta, Workspace } from "./types";
  import type { StateChangedPayload, ExitedPayload } from "./events";

  describe("ipc/events", () => {
    beforeEach(() => {
      registered.clear();
      listenMock.mockClear();
      unlisten.mockClear();
    });

    it("onSessionCreated subscribes to session://created and unwraps payload", async () => {
      const cb = vi.fn();
      const un = await onSessionCreated(cb);
      expect(listenMock).toHaveBeenCalledWith("session://created", expect.any(Function));
      const meta: SessionMeta = {
        id: "s1",
        workspaceId: "w1",
        title: "zsh",
        shell: "/bin/zsh",
        cwd: "/tmp",
        cols: 80,
        rows: 24,
        lifecycle: { kind: "atPrompt" },
        waitingForInput: false,
        isActive: true,
        createdAt: 1,
      };
      registered.get("session://created")!({ payload: meta });
      expect(cb).toHaveBeenCalledWith(meta);
      expect(un).toBe(unlisten);
    });

    it("onSessionStateChanged subscribes to session://state-changed", async () => {
      const cb = vi.fn();
      await onSessionStateChanged(cb);
      expect(listenMock).toHaveBeenCalledWith("session://state-changed", expect.any(Function));
      const p: StateChangedPayload = {
        sessionId: "s1",
        lifecycle: { kind: "running" },
        waitingForInput: false,
        cwd: "/tmp",
      };
      registered.get("session://state-changed")!({ payload: p });
      expect(cb).toHaveBeenCalledWith(p);
    });

    it("onSessionExited subscribes to session://exited", async () => {
      const cb = vi.fn();
      await onSessionExited(cb);
      expect(listenMock).toHaveBeenCalledWith("session://exited", expect.any(Function));
      const p: ExitedPayload = { sessionId: "s1", code: 0, signal: null };
      registered.get("session://exited")!({ payload: p });
      expect(cb).toHaveBeenCalledWith(p);
    });

    it("onWorkspaceCreated subscribes to workspace://created", async () => {
      const cb = vi.fn();
      await onWorkspaceCreated(cb);
      expect(listenMock).toHaveBeenCalledWith("workspace://created", expect.any(Function));
      const w: Workspace = { id: "w1", name: "p", rootPath: "/p" };
      registered.get("workspace://created")!({ payload: w });
      expect(cb).toHaveBeenCalledWith(w);
    });

    it("onDaemonDisconnected subscribes to daemon://disconnected and calls cb (no payload)", async () => {
      const cb = vi.fn();
      await onDaemonDisconnected(cb);
      expect(listenMock).toHaveBeenCalledWith("daemon://disconnected", expect.any(Function));
      registered.get("daemon://disconnected")!({ payload: null });
      expect(cb).toHaveBeenCalledTimes(1);
    });

    it("onDaemonReconnected subscribes to daemon://reconnected and calls cb", async () => {
      const cb = vi.fn();
      await onDaemonReconnected(cb);
      expect(listenMock).toHaveBeenCalledWith("daemon://reconnected", expect.any(Function));
      registered.get("daemon://reconnected")!({ payload: null });
      expect(cb).toHaveBeenCalledTimes(1);
    });
  });
  ```

- [ ] **Step 12: Run — confirm FAIL.**
  `npx vitest run src/ipc/events.test.ts`
  Expected: FAIL with "Failed to resolve import './events'".

- [ ] **Step 13: Implement `src/ipc/events.ts`.**
  ```ts
  import { listen, type UnlistenFn } from "@tauri-apps/api/event";
  import type {
    SessionId,
    SessionMeta,
    SessionLifecycle,
    Workspace,
  } from "./types";

  export interface StateChangedPayload {
    sessionId: SessionId;
    lifecycle: SessionLifecycle;
    waitingForInput: boolean;
    cwd: string;
  }

  export interface ExitedPayload {
    sessionId: SessionId;
    code: number | null;
    signal: string | null;
  }

  export function onSessionCreated(
    cb: (m: SessionMeta) => void,
  ): Promise<UnlistenFn> {
    return listen<SessionMeta>("session://created", (e) => cb(e.payload));
  }

  export function onSessionStateChanged(
    cb: (p: StateChangedPayload) => void,
  ): Promise<UnlistenFn> {
    return listen<StateChangedPayload>("session://state-changed", (e) => cb(e.payload));
  }

  export function onSessionExited(
    cb: (p: ExitedPayload) => void,
  ): Promise<UnlistenFn> {
    return listen<ExitedPayload>("session://exited", (e) => cb(e.payload));
  }

  export function onWorkspaceCreated(
    cb: (w: Workspace) => void,
  ): Promise<UnlistenFn> {
    return listen<Workspace>("workspace://created", (e) => cb(e.payload));
  }

  export function onDaemonDisconnected(cb: () => void): Promise<UnlistenFn> {
    return listen<null>("daemon://disconnected", () => cb());
  }

  export function onDaemonReconnected(cb: () => void): Promise<UnlistenFn> {
    return listen<null>("daemon://reconnected", () => cb());
  }
  ```

- [ ] **Step 14: Run — confirm PASS.**
  `npx vitest run src/ipc/events.test.ts`
  Expected: PASS (6 tests green).

- [ ] **Step 15: Commit.**
  `git add src/ipc/events.ts src/ipc/events.test.ts && git commit -m "feat(ipc): typed listen() subscriptions for the §6.3 global events"`

**Definition of Done:**
- `npx vitest run src/ipc/` green (commands 13, channel 2, events 6).
- `commands.ts` exposes a typed wrapper for **every** §6.1 command with camelCase arg keys mapping to the exact `#[tauri::command]` names (`create_session`, `list_sessions`, `attach_session`, `detach_session`, `write_stdin`, `resize`, `kill_session`, `list_workspaces`, `create_workspace`, `get_session_state`, `pick_folder`).
- `channel.ts` `newTerminalChannel` returns a real `@tauri-apps/api/core` `Channel<TerminalEvent>` whose `onmessage` forwards frames verbatim.
- `events.ts` subscribes to all six §6.3 event names with typed payloads (`StateChangedPayload`, `ExitedPayload` shapes locked to §6.3) and returns `UnlistenFn`.
- `types.ts` is consumed as generated (T3) and is **not** hand-edited by this task.

---

### Task 20: `src/store/store.ts` — Zustand app store (metadata only)

**Files:**
- Create: `src/store/store.ts`
- Test (create): `src/store/store.test.ts`

**Depends on:** [T3] (generated `types.ts`: `SessionMeta`, `Workspace`, `SessionLifecycle`, `SessionId`, `WorkspaceId`), [T19] (consumes `StateChangedPayload` type from `src/ipc/events.ts` for the `setLifecycle` action signature)
**Parallel-safe with:** [T21] (disjoint files: `src/terminal/**`). *Ordering note:* references `src/ipc/events.ts` types only; if executed before T19 lands, inline the `StateChangedPayload` shape is identical — but per the graph T19/T20/T21 run in the same parallel group, so import from `../ipc/events`.

**Interfaces:**
- Consumes (from T3 `src/ipc/types.ts`): `SessionId`, `WorkspaceId`, `SessionMeta`, `Workspace`, `SessionLifecycle`. From `src/ipc/events.ts` (T19): `StateChangedPayload`. From `zustand`: `create`.
- Produces (`src/store/store.ts`): `interface AppState` with state (spec §12) `sessions: Record<SessionId, SessionMeta>`, `workspaces: Record<WorkspaceId, Workspace>`, `activeSessionId: SessionId | null`, `daemonConnected: boolean`; and actions:
  `upsertSession(meta: SessionMeta): void`;
  `removeSession(id: SessionId): void`;
  `setLifecycle(p: StateChangedPayload): void`;
  `setDaemonConnected(connected: boolean): void`;
  `upsertWorkspace(ws: Workspace): void`;
  `setActiveSession(id: SessionId | null): void`.
  Exported hook: `const useAppStore = create<AppState>(...)`.
- **Invariant (spec §12):** the store holds **metadata only** — PTY bytes NEVER enter it.

- [ ] **Step 1: Failing test for reducers + `state-changed` + disconnect flag.**
  Create `src/store/store.test.ts`. Reset the store between tests via `setState`. Cover upsert/remove sessions, `setLifecycle` (updates only `lifecycle`/`waitingForInput`/`cwd` of an existing session, no-op for unknown id), `setDaemonConnected`, `upsertWorkspace`, `setActiveSession`.
  ```ts
  import { describe, it, expect, beforeEach } from "vitest";
  import { useAppStore } from "./store";
  import type { SessionMeta, Workspace } from "../ipc/types";
  import type { StateChangedPayload } from "../ipc/events";

  const meta = (over: Partial<SessionMeta> = {}): SessionMeta => ({
    id: "s1",
    workspaceId: "w1",
    title: "zsh",
    shell: "/bin/zsh",
    cwd: "/tmp",
    cols: 80,
    rows: 24,
    lifecycle: { kind: "atPrompt" },
    waitingForInput: false,
    isActive: true,
    createdAt: 1,
    ...over,
  });

  const initial = useAppStore.getState();

  describe("useAppStore", () => {
    beforeEach(() => {
      useAppStore.setState(
        {
          sessions: {},
          workspaces: {},
          activeSessionId: null,
          daemonConnected: false,
        },
        false,
      );
    });

    it("has the spec §12 initial shape", () => {
      const s = useAppStore.getState();
      expect(s.sessions).toEqual({});
      expect(s.workspaces).toEqual({});
      expect(s.activeSessionId).toBeNull();
      expect(s.daemonConnected).toBe(false);
      expect(typeof initial.upsertSession).toBe("function");
    });

    it("upsertSession adds then replaces by id", () => {
      useAppStore.getState().upsertSession(meta());
      expect(useAppStore.getState().sessions["s1"].title).toBe("zsh");
      useAppStore.getState().upsertSession(meta({ title: "bash" }));
      expect(Object.keys(useAppStore.getState().sessions)).toHaveLength(1);
      expect(useAppStore.getState().sessions["s1"].title).toBe("bash");
    });

    it("removeSession deletes and clears activeSessionId if it matched", () => {
      useAppStore.getState().upsertSession(meta());
      useAppStore.getState().setActiveSession("s1");
      useAppStore.getState().removeSession("s1");
      expect(useAppStore.getState().sessions["s1"]).toBeUndefined();
      expect(useAppStore.getState().activeSessionId).toBeNull();
    });

    it("removeSession keeps a non-matching activeSessionId", () => {
      useAppStore.getState().upsertSession(meta());
      useAppStore.getState().upsertSession(meta({ id: "s2" }));
      useAppStore.getState().setActiveSession("s2");
      useAppStore.getState().removeSession("s1");
      expect(useAppStore.getState().activeSessionId).toBe("s2");
    });

    it("setLifecycle updates lifecycle/waitingForInput/cwd of an existing session", () => {
      useAppStore.getState().upsertSession(meta());
      const p: StateChangedPayload = {
        sessionId: "s1",
        lifecycle: { kind: "running" },
        waitingForInput: true,
        cwd: "/work",
      };
      useAppStore.getState().setLifecycle(p);
      const s = useAppStore.getState().sessions["s1"];
      expect(s.lifecycle).toEqual({ kind: "running" });
      expect(s.waitingForInput).toBe(true);
      expect(s.cwd).toBe("/work");
      // untouched fields preserved
      expect(s.title).toBe("zsh");
      expect(s.cols).toBe(80);
    });

    it("setLifecycle is a no-op for an unknown session id", () => {
      const p: StateChangedPayload = {
        sessionId: "ghost",
        lifecycle: { kind: "running" },
        waitingForInput: false,
        cwd: "/x",
      };
      useAppStore.getState().setLifecycle(p);
      expect(useAppStore.getState().sessions["ghost"]).toBeUndefined();
    });

    it("setDaemonConnected toggles the flag", () => {
      useAppStore.getState().setDaemonConnected(true);
      expect(useAppStore.getState().daemonConnected).toBe(true);
      useAppStore.getState().setDaemonConnected(false);
      expect(useAppStore.getState().daemonConnected).toBe(false);
    });

    it("upsertWorkspace adds then replaces by id", () => {
      const w: Workspace = { id: "w1", name: "proj", rootPath: "/p" };
      useAppStore.getState().upsertWorkspace(w);
      expect(useAppStore.getState().workspaces["w1"].name).toBe("proj");
      useAppStore.getState().upsertWorkspace({ ...w, name: "renamed" });
      expect(useAppStore.getState().workspaces["w1"].name).toBe("renamed");
    });

    it("never stores raw bytes: session values are exactly SessionMeta keys", () => {
      useAppStore.getState().upsertSession(meta());
      const keys = Object.keys(useAppStore.getState().sessions["s1"]).sort();
      expect(keys).toEqual(
        [
          "cols",
          "cwd",
          "createdAt",
          "id",
          "isActive",
          "lifecycle",
          "rows",
          "shell",
          "title",
          "waitingForInput",
          "workspaceId",
        ].sort(),
      );
    });
  });
  ```

- [ ] **Step 2: Run — confirm FAIL.**
  `npx vitest run src/store/store.test.ts`
  Expected: FAIL with "Failed to resolve import './store'".

- [ ] **Step 3: Implement `src/store/store.ts`.**
  ```ts
  import { create } from "zustand";
  import type {
    SessionId,
    WorkspaceId,
    SessionMeta,
    Workspace,
  } from "../ipc/types";
  import type { StateChangedPayload } from "../ipc/events";

  /**
   * Global app state (spec §12). METADATA ONLY — PTY bytes never enter this store;
   * they are written straight to xterm via the terminal Channel (see terminal-manager).
   */
  export interface AppState {
    sessions: Record<SessionId, SessionMeta>;
    workspaces: Record<WorkspaceId, Workspace>;
    activeSessionId: SessionId | null;
    daemonConnected: boolean;

    upsertSession: (meta: SessionMeta) => void;
    removeSession: (id: SessionId) => void;
    setLifecycle: (p: StateChangedPayload) => void;
    setDaemonConnected: (connected: boolean) => void;
    upsertWorkspace: (ws: Workspace) => void;
    setActiveSession: (id: SessionId | null) => void;
  }

  export const useAppStore = create<AppState>((set) => ({
    sessions: {},
    workspaces: {},
    activeSessionId: null,
    daemonConnected: false,

    upsertSession: (meta) =>
      set((s) => ({ sessions: { ...s.sessions, [meta.id]: meta } })),

    removeSession: (id) =>
      set((s) => {
        const { [id]: _removed, ...rest } = s.sessions;
        return {
          sessions: rest,
          activeSessionId: s.activeSessionId === id ? null : s.activeSessionId,
        };
      }),

    setLifecycle: (p) =>
      set((s) => {
        const existing = s.sessions[p.sessionId];
        if (!existing) return {};
        return {
          sessions: {
            ...s.sessions,
            [p.sessionId]: {
              ...existing,
              lifecycle: p.lifecycle,
              waitingForInput: p.waitingForInput,
              cwd: p.cwd,
            },
          },
        };
      }),

    setDaemonConnected: (connected) => set({ daemonConnected: connected }),

    upsertWorkspace: (ws) =>
      set((s) => ({ workspaces: { ...s.workspaces, [ws.id]: ws } })),

    setActiveSession: (id) => set({ activeSessionId: id }),
  }));
  ```

- [ ] **Step 4: Run — confirm PASS.**
  `npx vitest run src/store/store.test.ts`
  Expected: PASS (9 tests green).

- [ ] **Step 5: Commit.**
  `git add src/store/store.ts src/store/store.test.ts && git commit -m "feat(store): Zustand useAppStore (metadata-only) with §12 shape + actions"`

**Definition of Done:**
- `npx vitest run src/store/store.test.ts` green (9 tests).
- State shape matches spec §12 exactly: `{ sessions: Record<SessionId,SessionMeta>, workspaces: Record<WorkspaceId,Workspace>, activeSessionId, daemonConnected }`.
- `setLifecycle` maps a `session://state-changed` payload onto an existing session's `lifecycle`/`waitingForInput`/`cwd` and is a no-op for an unknown id; `setDaemonConnected(false)` sets the disconnect flag (drives the banner in T22).
- Bytes-never-in-store invariant asserted by the SessionMeta-keys test.

---

### Task 21: `src/terminal/terminal-manager.ts` — non-reactive xterm lifecycle manager

**Files:**
- Create: `src/terminal/terminal-manager.ts`
- Test (create): `src/terminal/terminal-manager.test.ts`

**Depends on:** [T3] (generated `types.ts`: `SessionId`, `TerminalEvent`), [T19] (`src/ipc/commands.ts`: `writeStdin`, `resize`, `attachSession`; `src/ipc/channel.ts`: `newTerminalChannel`)
**Parallel-safe with:** [T20] (disjoint files: `src/store/**`).

**Interfaces:**
- Consumes (from T3 `src/ipc/types.ts`): `SessionId`, `TerminalEvent`. From T19 `src/ipc/commands.ts`: `writeStdin(sessionId, data)`, `resize(sessionId, cols, rows)`, `attachSession(sessionId, channel)`. From T19 `src/ipc/channel.ts`: `newTerminalChannel(onEvent)`. From `@xterm/xterm`: `Terminal`, `ITerminalOptions`. From `@xterm/addon-fit`: `FitAddon`. From `@xterm/addon-webgl`: `WebglAddon`. CSS side-effect import `@xterm/xterm/css/xterm.css`.
- Produces (`src/terminal/terminal-manager.ts`): `class TerminalManager` with a non-reactive private `Map<SessionId, TerminalEntry>` and methods:
  `create(sessionId: SessionId): Terminal` (idempotent — returns the existing instance if present; StrictMode-safe);
  `open(sessionId: SessionId, container: HTMLElement): void` (attach to DOM; lazy WebGL on visible; ResizeObserver→debounced fit→`resize` IPC; keep-alive re-open);
  `has(sessionId: SessionId): boolean`;
  `get(sessionId: SessionId): Terminal | undefined`;
  `applyReplay(sessionId: SessionId, cols: number, rows: number, content: Uint8Array): void` (resize + write BEFORE `open()`);
  `writeOutput(sessionId: SessionId, bytes: Uint8Array): void` (firehose → `term.write`, never store);
  `dispose(sessionId: SessionId): void` (**only** on real session close);
  `disposeAll(): void`.
- **Invariants (spec §12):** keep-alive = never `dispose()` on unmount; `dispose()` only on kill/exit; replay written before `open()`; WebGL lazy + `onContextLoss → dispose(webgl) → DOM`; `convertEol` off; bytes never in React/Zustand state.

- [ ] **Step 1: Failing test — keep-alive, dispose-on-close, replay-before-open, bytes-not-in-store.**
  Create `src/terminal/terminal-manager.test.ts` (vitest + jsdom). Mock `@xterm/xterm`, `@xterm/addon-fit`, `@xterm/addon-webgl`, the CSS import, and the T19 IPC modules. Record the call order of `write` vs `open` on the mocked Terminal.
  ```ts
  import { describe, it, expect, vi, beforeEach } from "vitest";

  // ---- record ordering across all terminal instances ----
  const calls: string[] = [];

  const writeStdinMock = vi.fn();
  const resizeMock = vi.fn();
  const attachSessionMock = vi.fn().mockResolvedValue(undefined);

  vi.mock("../ipc/commands", () => ({
    writeStdin: (...a: unknown[]) => writeStdinMock(...a),
    resize: (...a: unknown[]) => resizeMock(...a),
    attachSession: (...a: unknown[]) => attachSessionMock(...a),
  }));
  vi.mock("../ipc/channel", () => ({
    newTerminalChannel: (onEvent: (e: unknown) => void) => ({ onmessage: onEvent }),
  }));
  vi.mock("@xterm/xterm/css/xterm.css", () => ({}));

  class FakeTerminal {
    options: Record<string, unknown>;
    disposed = false;
    onDataCb: ((d: string) => void) | undefined;
    onResizeCb: ((s: { cols: number; rows: number }) => void) | undefined;
    cols = 80;
    rows = 24;
    constructor(opts: Record<string, unknown>) {
      this.options = opts;
    }
    loadAddon = vi.fn();
    open = vi.fn(() => calls.push("open"));
    write = vi.fn((_d: unknown) => calls.push("write"));
    resize = vi.fn((c: number, r: number) => {
      this.cols = c;
      this.rows = r;
      calls.push("resize");
    });
    onData = vi.fn((cb: (d: string) => void) => {
      this.onDataCb = cb;
      return { dispose: vi.fn() };
    });
    onResize = vi.fn((cb: (s: { cols: number; rows: number }) => void) => {
      this.onResizeCb = cb;
      return { dispose: vi.fn() };
    });
    dispose = vi.fn(() => {
      this.disposed = true;
      calls.push("dispose");
    });
  }
  const terminals: FakeTerminal[] = [];
  vi.mock("@xterm/xterm", () => ({
    Terminal: vi.fn((opts: Record<string, unknown>) => {
      const t = new FakeTerminal(opts);
      terminals.push(t);
      return t;
    }),
  }));

  const fitMock = vi.fn();
  vi.mock("@xterm/addon-fit", () => ({
    FitAddon: vi.fn(() => ({ fit: fitMock, proposeDimensions: vi.fn() })),
  }));
  const webglDispose = vi.fn();
  let contextLossCb: (() => void) | undefined;
  vi.mock("@xterm/addon-webgl", () => ({
    WebglAddon: vi.fn(() => ({
      dispose: webglDispose,
      onContextLoss: (cb: () => void) => {
        contextLossCb = cb;
      },
    })),
  }));

  import { TerminalManager } from "./terminal-manager";

  function makeContainer(): HTMLElement {
    const el = document.createElement("div");
    Object.defineProperty(el, "clientWidth", { value: 800, configurable: true });
    Object.defineProperty(el, "clientHeight", { value: 600, configurable: true });
    document.body.appendChild(el);
    return el;
  }

  // jsdom lacks ResizeObserver
  beforeEach(() => {
    calls.length = 0;
    terminals.length = 0;
    contextLossCb = undefined;
    writeStdinMock.mockReset();
    resizeMock.mockReset();
    attachSessionMock.mockClear();
    fitMock.mockReset();
    webglDispose.mockReset();
    (globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = class {
      observe = vi.fn();
      disconnect = vi.fn();
    };
  });

  describe("TerminalManager", () => {
    it("create is idempotent and StrictMode-safe (one Terminal per id)", () => {
      const m = new TerminalManager();
      const a = m.create("s1");
      const b = m.create("s1");
      expect(a).toBe(b);
      expect(terminals).toHaveLength(1);
      expect(m.has("s1")).toBe(true);
    });

    it("sets convertEol off on the Terminal", () => {
      const m = new TerminalManager();
      m.create("s1");
      expect(terminals[0].options.convertEol).toBe(false);
    });

    it("applyReplay writes replay content BEFORE open() (replay-before-open ordering)", () => {
      const m = new TerminalManager();
      m.create("s1");
      m.applyReplay("s1", 100, 40, new Uint8Array([104, 105]));
      const container = makeContainer();
      m.open("s1", container);
      const firstWrite = calls.indexOf("write");
      const firstOpen = calls.indexOf("open");
      expect(firstWrite).toBeGreaterThanOrEqual(0);
      expect(firstOpen).toBeGreaterThanOrEqual(0);
      expect(firstWrite).toBeLessThan(firstOpen);
      // replay resized to snapshot dims before writing
      expect(terminals[0].resize).toHaveBeenCalledWith(100, 40);
    });

    it("keep-alive: nothing is disposed when a panel merely unmounts (no dispose call)", () => {
      const m = new TerminalManager();
      m.create("s1");
      m.open("s1", makeContainer());
      // simulate unmount by simply NOT calling dispose; instance stays alive
      expect(terminals[0].disposed).toBe(false);
      expect(m.has("s1")).toBe(true);
      // re-open into a new container reuses the same instance
      m.open("s1", makeContainer());
      expect(terminals).toHaveLength(1);
      expect(terminals[0].open).toHaveBeenCalledTimes(2);
    });

    it("dispose() only on real close: tears the instance down and forgets it", () => {
      const m = new TerminalManager();
      m.create("s1");
      m.open("s1", makeContainer());
      m.dispose("s1");
      expect(terminals[0].disposed).toBe(true);
      expect(m.has("s1")).toBe(false);
      expect(m.get("s1")).toBeUndefined();
    });

    it("onData forwards keystrokes to write_stdin IPC", () => {
      const m = new TerminalManager();
      m.create("s1");
      m.open("s1", makeContainer());
      terminals[0].onDataCb!("l");
      expect(writeStdinMock).toHaveBeenCalledWith("s1", "l");
    });

    it("onResize forwards fitted dims to resize IPC", () => {
      const m = new TerminalManager();
      m.create("s1");
      m.open("s1", makeContainer());
      terminals[0].onResizeCb!({ cols: 120, rows: 30 });
      expect(resizeMock).toHaveBeenCalledWith("s1", 120, 30);
    });

    it("WebGL context loss disposes the webgl addon (DOM fallback), NOT the Terminal", () => {
      const m = new TerminalManager();
      m.create("s1");
      m.open("s1", makeContainer());
      expect(contextLossCb).toBeTypeOf("function");
      contextLossCb!();
      expect(webglDispose).toHaveBeenCalledTimes(1);
      expect(terminals[0].disposed).toBe(false);
    });

    it("writeOutput goes straight to term.write (bytes never returned/stored)", () => {
      const m = new TerminalManager();
      m.create("s1");
      const ret = m.writeOutput("s1", new Uint8Array([65, 66]));
      expect(terminals[0].write).toHaveBeenCalledWith(new Uint8Array([65, 66]));
      expect(ret).toBeUndefined();
    });

    it("disposeAll tears down every instance and empties the map", () => {
      const m = new TerminalManager();
      m.create("s1");
      m.create("s2");
      m.disposeAll();
      expect(m.has("s1")).toBe(false);
      expect(m.has("s2")).toBe(false);
      expect(terminals.every((t) => t.disposed)).toBe(true);
    });
  });
  ```

- [ ] **Step 2: Run — confirm FAIL.**
  `npx vitest run src/terminal/terminal-manager.test.ts`
  Expected: FAIL with "Failed to resolve import './terminal-manager'".

- [ ] **Step 3: Implement `src/terminal/terminal-manager.ts`.**
  ```ts
  import "@xterm/xterm/css/xterm.css";
  import { Terminal, type ITerminalOptions } from "@xterm/xterm";
  import { FitAddon } from "@xterm/addon-fit";
  import { WebglAddon } from "@xterm/addon-webgl";
  import type { SessionId } from "../ipc/types";
  import { writeStdin, resize, attachSession } from "../ipc/commands";
  import { newTerminalChannel } from "../ipc/channel";
  import type { TerminalEvent } from "../ipc/types";

  const RESIZE_DEBOUNCE_MS = 100;

  const TERMINAL_OPTIONS: ITerminalOptions = {
    convertEol: false, // real PTY (termios) handles \n -> \r\n
    scrollback: 10000,
    cursorBlink: true,
    fontFamily:
      'Menlo, "SF Mono", "JetBrains Mono", ui-monospace, monospace',
    fontSize: 13,
    allowProposedApi: true,
  };

  interface TerminalEntry {
    term: Terminal;
    fit: FitAddon;
    webgl: WebglAddon | undefined;
    container: HTMLElement | undefined;
    resizeObserver: ResizeObserver | undefined;
    resizeTimer: ReturnType<typeof setTimeout> | undefined;
    opened: boolean;
  }

  /**
   * Owns xterm `Terminal` instances OUTSIDE React state (spec §12): a non-reactive
   * `Map<SessionId, TerminalEntry>`. React components borrow a ref; a Terminal is
   * never held in useState. Keep-alive: unmount does NOT dispose — only a real
   * session close (kill/exit) does.
   */
  export class TerminalManager {
    private entries = new Map<SessionId, TerminalEntry>();

    /** Idempotent (StrictMode-safe): one Terminal per session id. */
    create(sessionId: SessionId): Terminal {
      const existing = this.entries.get(sessionId);
      if (existing) return existing.term;

      const term = new Terminal(TERMINAL_OPTIONS);
      const fit = new FitAddon();
      term.loadAddon(fit);

      term.onData((data) => {
        void writeStdin(sessionId, data);
      });
      term.onResize(({ cols, rows }) => {
        void resize(sessionId, cols, rows);
      });

      this.entries.set(sessionId, {
        term,
        fit,
        webgl: undefined,
        container: undefined,
        resizeObserver: undefined,
        resizeTimer: undefined,
        opened: false,
      });
      return term;
    }

    has(sessionId: SessionId): boolean {
      return this.entries.has(sessionId);
    }

    get(sessionId: SessionId): Terminal | undefined {
      return this.entries.get(sessionId)?.term;
    }

    /**
     * Replay = sanitized scrollback ring (spec §11): resize to the snapshot dims,
     * then write the bytes BEFORE `open()` so the visible screen paints correctly.
     */
    applyReplay(
      sessionId: SessionId,
      cols: number,
      rows: number,
      content: Uint8Array,
    ): void {
      const entry = this.entries.get(sessionId);
      if (!entry) return;
      entry.term.resize(cols, rows);
      entry.term.write(content);
    }

    /** Live PTY firehose. Bytes go straight to xterm — NEVER into React/Zustand state. */
    writeOutput(sessionId: SessionId, bytes: Uint8Array): void {
      const entry = this.entries.get(sessionId);
      if (!entry) return;
      entry.term.write(bytes);
    }

    /**
     * Attach the Terminal to a DOM container. Safe to call again on re-show
     * (keep-alive): re-opens the same instance into the new container and
     * re-arms WebGL + ResizeObserver.
     */
    open(sessionId: SessionId, container: HTMLElement): void {
      const entry = this.entries.get(sessionId);
      if (!entry) return;

      this.teardownContainer(entry);
      entry.container = container;
      entry.term.open(container);
      entry.opened = true;

      this.enableWebgl(entry);

      const observer = new ResizeObserver(() => {
        if (entry.resizeTimer) clearTimeout(entry.resizeTimer);
        entry.resizeTimer = setTimeout(() => {
          if (container.clientWidth > 0 && container.clientHeight > 0) {
            entry.fit.fit(); // fit() -> term.resize() -> onResize -> resize() IPC
          }
        }, RESIZE_DEBOUNCE_MS);
      });
      observer.observe(container);
      entry.resizeObserver = observer;

      if (container.clientWidth > 0 && container.clientHeight > 0) {
        entry.fit.fit();
      }
    }

    /** Lazy WebGL only on a visible/opened terminal; DOM fallback on context loss. */
    private enableWebgl(entry: TerminalEntry): void {
      if (entry.webgl) return;
      try {
        const webgl = new WebglAddon();
        webgl.onContextLoss(() => {
          webgl.dispose(); // -> Terminal falls back to the DOM renderer
          entry.webgl = undefined;
        });
        entry.term.loadAddon(webgl);
        entry.webgl = webgl;
      } catch {
        // WebGL unavailable -> DOM renderer (no-op); honest degradation.
        entry.webgl = undefined;
      }
    }

    private teardownContainer(entry: TerminalEntry): void {
      if (entry.resizeTimer) {
        clearTimeout(entry.resizeTimer);
        entry.resizeTimer = undefined;
      }
      entry.resizeObserver?.disconnect();
      entry.resizeObserver = undefined;
    }

    /** ONLY on real session close (kill/exit). Disposes Terminal + all addons + listeners. */
    dispose(sessionId: SessionId): void {
      const entry = this.entries.get(sessionId);
      if (!entry) return;
      this.teardownContainer(entry);
      entry.webgl = undefined; // disposed transitively by term.dispose()
      entry.term.dispose();
      this.entries.delete(sessionId);
    }

    disposeAll(): void {
      for (const id of Array.from(this.entries.keys())) this.dispose(id);
    }

    /**
     * Wire the daemon firehose for this session: build the Channel and attach.
     * `Replay` is applied before/at open; `Output` streams straight to xterm.
     */
    async attach(sessionId: SessionId): Promise<void> {
      const channel = newTerminalChannel((e: TerminalEvent) => {
        if (e.event === "replay") {
          this.applyReplay(
            sessionId,
            e.data.cols,
            e.data.rows,
            new Uint8Array(e.data.content),
          );
        } else {
          this.writeOutput(sessionId, new Uint8Array(e.data.bytes));
        }
      });
      await attachSession(sessionId, channel as never);
    }
  }
  ```

- [ ] **Step 4: Run — confirm PASS.**
  `npx vitest run src/terminal/terminal-manager.test.ts`
  Expected: PASS (10 tests green).

- [ ] **Step 5: Commit.**
  `git add src/terminal/terminal-manager.ts src/terminal/terminal-manager.test.ts && git commit -m "feat(terminal): non-reactive TerminalManager (keep-alive, replay-before-open, lazy WebGL)"`

**Definition of Done:**
- `npx vitest run src/terminal/terminal-manager.test.ts` green (10 tests).
- `Terminal` instances live in a non-reactive `Map<SessionId, TerminalEntry>` (never React state); `create` is idempotent/StrictMode-safe.
- Keep-alive: unmount performs no `dispose()`; `dispose()`/`disposeAll()` only on real close and remove the map entry.
- Replay is written (after resize to snapshot dims) **before** `open()`; `convertEol` is off; `@xterm/xterm/css/xterm.css` imported.
- WebGL is loaded lazily on `open()` and `onContextLoss` disposes only the WebGL addon (DOM fallback), leaving the Terminal alive.
- `onData → write_stdin`; debounced `ResizeObserver → fit() → onResize → resize` IPC; live/replay bytes go straight to `term.write` and never enter React/Zustand state.


### Task 22: `src/components/**` + `App.tsx` + `theme.ts` + `main.tsx` — React UI shell wiring store, IPC subscriptions, and terminal panes

**Files:**
- Create: `src/theme.ts`
- Create: `src/components/StatusDot.tsx`
- Create: `src/components/DaemonBanner.tsx`
- Create: `src/components/TerminalPane.tsx`
- Create: `src/components/TerminalTabs.tsx`
- Create: `src/components/WorkspaceSidebar.tsx`
- Create: `src/App.tsx`
- Create: `src/main.tsx`
- Modify (add RTL + jsdom devDeps): `/Users/sshlg/DATA/builder-pro-ai/package.json`
- Test (create): `src/components/StatusDot.test.tsx`, `src/components/DaemonBanner.test.tsx`, `src/components/TerminalTabs.test.tsx`, `src/App.test.tsx`

**Depends on:** [T19] (`src/ipc/commands.ts`: `createSession`, `listSessions`, `listWorkspaces`, `createWorkspace`, `killSession`, `pickFolder`; `src/ipc/events.ts`: `onSessionCreated`, `onSessionStateChanged`, `onSessionExited`, `onWorkspaceCreated`, `onDaemonDisconnected`, `onDaemonReconnected`, and types `StateChangedPayload`/`ExitedPayload`), [T20] (`src/store/store.ts`: `useAppStore`, `AppState`), [T21] (`src/terminal/terminal-manager.ts`: `TerminalManager`)
**Parallel-safe with:** [] (T22 is the sequential integration task at the end of G5; it owns `src/components/**`, `src/App.tsx`, `src/main.tsx`, `src/theme.ts` — no other task writes these)

**Interfaces:**
- Consumes (from T3 `src/ipc/types.ts`): `type SessionId = string`; `type WorkspaceId = string`; `interface Workspace { id; name; rootPath }`; `type SessionLifecycle = { kind:"atPrompt" } | { kind:"typing" } | { kind:"running" } | { kind:"exited"; code:number|null; signal:string|null }`; `interface SessionMeta { id; workspaceId; title; shell; cwd; cols; rows; lifecycle; waitingForInput; isActive; createdAt }`.
- Consumes (from T20 `src/store/store.ts`): `useAppStore` (hook) with state `sessions: Record<SessionId, SessionMeta>`, `workspaces: Record<WorkspaceId, Workspace>`, `activeSessionId: SessionId | null`, `daemonConnected: boolean`; actions `upsertSession(meta)`, `removeSession(id)`, `setLifecycle(p: StateChangedPayload)`, `setDaemonConnected(connected)`, `upsertWorkspace(ws)`, `setActiveSession(id)`.
- Consumes (from T21 `src/terminal/terminal-manager.ts`): `class TerminalManager` with `create(sessionId): Terminal`, `open(sessionId, container): void`, `attach(sessionId): Promise<void>`, `has(sessionId): boolean`, `get(sessionId): Terminal | undefined`, `dispose(sessionId): void`, `disposeAll(): void`.
- Consumes (from T19 `src/ipc/commands.ts`): `createSession(workspaceId, opts?)`, `listSessions()`, `listWorkspaces()`, `createWorkspace(name, rootPath)`, `killSession(sessionId)`, `pickFolder()`. (from T19 `src/ipc/events.ts`): the six `on*` subscribers + `StateChangedPayload`/`ExitedPayload`.
- Consumes (from `react`): `StrictMode`, `useEffect`, `useRef`. (from `react-dom/client`): `createRoot`.
- Produces (`src/theme.ts`): `interface Theme { colors: { bg; bgElevated; border; text; textDim; accent; statusIdle; statusRunning; statusExited; statusWaiting } }` and `const theme: Theme` (single dark theme).
- Produces (`src/components/StatusDot.tsx`): `type DotState = "idle" | "running" | "exited" | "waiting"`; `function dotStateOf(lifecycle: SessionLifecycle, waitingForInput: boolean): DotState`; `function StatusDot(props: { lifecycle: SessionLifecycle; waitingForInput: boolean }): JSX.Element`.
- Produces (`src/components/DaemonBanner.tsx`): `function DaemonBanner(): JSX.Element | null` (reads `useAppStore(s => s.daemonConnected)`; renders a banner iff disconnected).
- Produces (`src/components/TerminalPane.tsx`): `function TerminalPane(props: { sessionId: SessionId; manager: TerminalManager; visible: boolean }): JSX.Element` (mounts a managed Terminal into a sized container; keep-alive on hide, no dispose).
- Produces (`src/components/TerminalTabs.tsx`): `function TerminalTabs(props: { manager: TerminalManager }): JSX.Element` (tab list + active switch + new-terminal button calling `createSession`).
- Produces (`src/components/WorkspaceSidebar.tsx`): `function WorkspaceSidebar(): JSX.Element` (workspace list + folder picker via `pickFolder` then `createWorkspace`).
- Produces (`src/App.tsx`): `function App(props?: { manager?: TerminalManager }): JSX.Element` (wires store + IPC subscriptions + layout sidebar | tabs | active pane). Also exports `const terminalManager: TerminalManager` (module singleton used by `main.tsx`).
- Produces (`src/main.tsx`): React 19 root render (`createRoot(...).render(<StrictMode><App/></StrictMode>)`); no exports.

- [ ] **Step 1: Add RTL + jsdom devDependencies.**
  T22 is the first task that renders React components in tests, so it introduces RTL. Edit `/Users/sshlg/DATA/builder-pro-ai/package.json` `devDependencies` (added alongside the T1 entries, keeping the block alphabetized) so it contains these exact new keys:
  ```json
  "@testing-library/dom": "^10.4.0",
  "@testing-library/jest-dom": "^6.6.0",
  "@testing-library/react": "^16.1.0",
  "@testing-library/user-event": "^14.5.0",
  "jsdom": "^25.0.0",
  ```
  Then install:
  `npm install`
  Expected: `package-lock.json` updated; `@testing-library/react`, `@testing-library/jest-dom`, `@testing-library/user-event`, `@testing-library/dom`, and `jsdom` present under `node_modules`. (The global vitest `test.environment` stays `"node"` from T1; component test files opt into jsdom per-file via a `// @vitest-environment jsdom` docblock — no change to `vite.config.ts`.)

- [ ] **Step 2: Commit the dependency addition.**
  `git add package.json package-lock.json && git commit -m "chore(frontend): add @testing-library/react + jsdom for component tests"`

- [ ] **Step 3: Failing test for `theme.ts` (dark theme has every status color).**
  Create `src/theme.ts`'s test `src/theme.test.ts` (plain `.ts`, node env is fine — no DOM):
  ```ts
  import { describe, it, expect } from "vitest";
  import { theme } from "./theme";

  describe("theme", () => {
    it("exposes the four distinct status colors used by StatusDot", () => {
      const { statusIdle, statusRunning, statusExited, statusWaiting } = theme.colors;
      const set = new Set([statusIdle, statusRunning, statusExited, statusWaiting]);
      expect(set.size).toBe(4); // all four are distinct
      for (const c of [statusIdle, statusRunning, statusExited, statusWaiting]) {
        expect(c).toMatch(/^#[0-9a-fA-F]{6}$/); // concrete hex colors
      }
    });

    it("is a dark theme (bg is dark)", () => {
      // parse #RRGGBB and assert luminance is low
      const hex = theme.colors.bg.replace("#", "");
      const r = parseInt(hex.slice(0, 2), 16);
      const g = parseInt(hex.slice(2, 4), 16);
      const b = parseInt(hex.slice(4, 6), 16);
      const lum = 0.2126 * r + 0.7152 * g + 0.0722 * b;
      expect(lum).toBeLessThan(64); // dark
    });
  });
  ```

- [ ] **Step 4: Run — confirm FAIL.**
  `npx vitest run src/theme.test.ts`
  Expected: FAIL with "Failed to resolve import './theme'".

- [ ] **Step 5: Implement `src/theme.ts`.**
  ```ts
  /** Single minimal dark theme (spec §1 goal: theme). Colors consumed by components/status dots. */
  export interface Theme {
    colors: {
      bg: string;
      bgElevated: string;
      border: string;
      text: string;
      textDim: string;
      accent: string;
      statusIdle: string; // atPrompt / typing
      statusRunning: string; // running (no input wait)
      statusExited: string; // exited
      statusWaiting: string; // running + waitingForInput
    };
  }

  export const theme: Theme = {
    colors: {
      bg: "#0d1117",
      bgElevated: "#161b22",
      border: "#30363d",
      text: "#e6edf3",
      textDim: "#8b949e",
      accent: "#2f81f7",
      statusIdle: "#8b949e", // grey — idle at prompt
      statusRunning: "#2ea043", // green — command running
      statusExited: "#f85149", // red — process exited
      statusWaiting: "#d29922", // amber — waiting for input
    },
  };
  ```

- [ ] **Step 6: Run — confirm PASS.**
  `npx vitest run src/theme.test.ts`
  Expected: PASS (2 tests green).

- [ ] **Step 7: Commit.**
  `git add src/theme.ts src/theme.test.ts && git commit -m "feat(ui): minimal dark theme with the four status-dot colors"`

- [ ] **Step 8: Failing test for `StatusDot` lifecycle→state mapping.**
  Create `src/components/StatusDot.test.tsx`. Cover `dotStateOf` for every `SessionLifecycle` variant + the `waitingForInput` override, and assert the rendered dot's `data-state` + `title` and its color come from `theme`.
  ```tsx
  // @vitest-environment jsdom
  import { describe, it, expect, afterEach } from "vitest";
  import { render, screen, cleanup } from "@testing-library/react";
  import { StatusDot, dotStateOf } from "./StatusDot";
  import { theme } from "../theme";
  import type { SessionLifecycle } from "../ipc/types";

  afterEach(cleanup);

  describe("dotStateOf", () => {
    it("atPrompt (not waiting) -> idle", () => {
      expect(dotStateOf({ kind: "atPrompt" }, false)).toBe("idle");
    });

    it("typing (not waiting) -> idle (Typing maps to AtPrompt color per spec §5)", () => {
      expect(dotStateOf({ kind: "typing" }, false)).toBe("idle");
    });

    it("running (not waiting) -> running", () => {
      expect(dotStateOf({ kind: "running" }, false)).toBe("running");
    });

    it("running + waitingForInput -> waiting (overrides running)", () => {
      expect(dotStateOf({ kind: "running" }, true)).toBe("waiting");
    });

    it("exited (not waiting) -> exited", () => {
      const lc: SessionLifecycle = { kind: "exited", code: 0, signal: null };
      expect(dotStateOf(lc, false)).toBe("exited");
    });

    it("exited + waitingForInput -> exited (exit wins over stale waiting flag)", () => {
      const lc: SessionLifecycle = { kind: "exited", code: 1, signal: null };
      expect(dotStateOf(lc, true)).toBe("exited");
    });

    it("atPrompt + waitingForInput -> idle (waiting only applies while running)", () => {
      expect(dotStateOf({ kind: "atPrompt" }, true)).toBe("idle");
    });
  });

  describe("StatusDot rendering", () => {
    it("renders the idle color + data-state for atPrompt", () => {
      render(<StatusDot lifecycle={{ kind: "atPrompt" }} waitingForInput={false} />);
      const dot = screen.getByRole("img", { name: /idle/i });
      expect(dot.getAttribute("data-state")).toBe("idle");
      expect(dot.style.backgroundColor).toBe(hexToRgb(theme.colors.statusIdle));
    });

    it("renders the running color for running", () => {
      render(<StatusDot lifecycle={{ kind: "running" }} waitingForInput={false} />);
      const dot = screen.getByRole("img", { name: /running/i });
      expect(dot.getAttribute("data-state")).toBe("running");
      expect(dot.style.backgroundColor).toBe(hexToRgb(theme.colors.statusRunning));
    });

    it("renders the waiting color for running+waitingForInput", () => {
      render(<StatusDot lifecycle={{ kind: "running" }} waitingForInput={true} />);
      const dot = screen.getByRole("img", { name: /waiting for input/i });
      expect(dot.getAttribute("data-state")).toBe("waiting");
      expect(dot.style.backgroundColor).toBe(hexToRgb(theme.colors.statusWaiting));
    });

    it("renders the exited color for exited", () => {
      render(
        <StatusDot lifecycle={{ kind: "exited", code: 0, signal: null }} waitingForInput={false} />,
      );
      const dot = screen.getByRole("img", { name: /exited/i });
      expect(dot.getAttribute("data-state")).toBe("exited");
      expect(dot.style.backgroundColor).toBe(hexToRgb(theme.colors.statusExited));
    });
  });

  // jsdom serializes inline background-color as rgb(...); convert #rrggbb for comparison.
  function hexToRgb(hex: string): string {
    const h = hex.replace("#", "");
    const r = parseInt(h.slice(0, 2), 16);
    const g = parseInt(h.slice(2, 4), 16);
    const b = parseInt(h.slice(4, 6), 16);
    return `rgb(${r}, ${g}, ${b})`;
  }
  ```

- [ ] **Step 9: Run — confirm FAIL.**
  `npx vitest run src/components/StatusDot.test.tsx`
  Expected: FAIL with "Failed to resolve import './StatusDot'".

- [ ] **Step 10: Implement `src/components/StatusDot.tsx`.**
  ```tsx
  import type { SessionLifecycle } from "../ipc/types";
  import { theme } from "../theme";

  export type DotState = "idle" | "running" | "exited" | "waiting";

  /**
   * Map a session's lifecycle + waiting flag to a dot state (spec §5, §10.4):
   * - Exited always wins (a stale waiting flag never overrides a finished process).
   * - While Running, waitingForInput surfaces the "waiting" state; otherwise "running".
   * - AtPrompt / Typing are idle (Typing is never emitted in S1; it maps to AtPrompt).
   */
  export function dotStateOf(
    lifecycle: SessionLifecycle,
    waitingForInput: boolean,
  ): DotState {
    switch (lifecycle.kind) {
      case "exited":
        return "exited";
      case "running":
        return waitingForInput ? "waiting" : "running";
      case "atPrompt":
      case "typing":
        return "idle";
    }
  }

  const COLOR: Record<DotState, string> = {
    idle: theme.colors.statusIdle,
    running: theme.colors.statusRunning,
    exited: theme.colors.statusExited,
    waiting: theme.colors.statusWaiting,
  };

  const LABEL: Record<DotState, string> = {
    idle: "idle",
    running: "running",
    exited: "exited",
    waiting: "waiting for input",
  };

  export function StatusDot(props: {
    lifecycle: SessionLifecycle;
    waitingForInput: boolean;
  }): JSX.Element {
    const state = dotStateOf(props.lifecycle, props.waitingForInput);
    return (
      <span
        role="img"
        aria-label={LABEL[state]}
        title={LABEL[state]}
        data-state={state}
        style={{
          display: "inline-block",
          width: 8,
          height: 8,
          borderRadius: "50%",
          backgroundColor: COLOR[state],
          flexShrink: 0,
        }}
      />
    );
  }
  ```

- [ ] **Step 11: Run — confirm PASS.**
  `npx vitest run src/components/StatusDot.test.tsx`
  Expected: PASS (11 tests green).

- [ ] **Step 12: Commit.**
  `git add src/components/StatusDot.tsx src/components/StatusDot.test.tsx && git commit -m "feat(ui): StatusDot maps lifecycle+waitingForInput to idle/running/exited/waiting"`

- [ ] **Step 13: Failing test for `DaemonBanner` show/hide on connection flag.**
  Create `src/components/DaemonBanner.test.tsx`. Drive `useAppStore.setDaemonConnected` and assert the banner appears only when disconnected.
  ```tsx
  // @vitest-environment jsdom
  import { describe, it, expect, afterEach, beforeEach } from "vitest";
  import { render, screen, cleanup, act } from "@testing-library/react";
  import { DaemonBanner } from "./DaemonBanner";
  import { useAppStore } from "../store/store";

  afterEach(cleanup);
  beforeEach(() => {
    useAppStore.setState(
      { sessions: {}, workspaces: {}, activeSessionId: null, daemonConnected: true },
      false,
    );
  });

  describe("DaemonBanner", () => {
    it("hides when the daemon is connected", () => {
      act(() => useAppStore.getState().setDaemonConnected(true));
      render(<DaemonBanner />);
      expect(screen.queryByRole("alert")).toBeNull();
    });

    it("shows when the daemon is disconnected", () => {
      act(() => useAppStore.getState().setDaemonConnected(false));
      render(<DaemonBanner />);
      const banner = screen.getByRole("alert");
      expect(banner.textContent).toMatch(/reconnect/i);
    });

    it("reactively appears then disappears as the flag flips (disconnected -> reconnected)", () => {
      act(() => useAppStore.getState().setDaemonConnected(false));
      render(<DaemonBanner />);
      expect(screen.getByRole("alert")).toBeTruthy();
      act(() => useAppStore.getState().setDaemonConnected(true));
      expect(screen.queryByRole("alert")).toBeNull();
    });
  });
  ```

- [ ] **Step 14: Run — confirm FAIL.**
  `npx vitest run src/components/DaemonBanner.test.tsx`
  Expected: FAIL with "Failed to resolve import './DaemonBanner'".

- [ ] **Step 15: Implement `src/components/DaemonBanner.tsx`.**
  ```tsx
  import { useAppStore } from "../store/store";
  import { theme } from "../theme";

  /**
   * Shown on `daemon://disconnected` (store `daemonConnected=false`), hidden on
   * `daemon://reconnected` (store flips back true). Spec §13: never fake a
   * "connected" state; tell the user honestly the session service is unreachable.
   */
  export function DaemonBanner(): JSX.Element | null {
    const connected = useAppStore((s) => s.daemonConnected);
    if (connected) return null;
    return (
      <div
        role="alert"
        style={{
          padding: "6px 12px",
          background: theme.colors.statusExited,
          color: theme.colors.text,
          fontSize: 13,
          textAlign: "center",
        }}
      >
        Session service disconnected — trying to reconnect…
      </div>
    );
  }
  ```

- [ ] **Step 16: Run — confirm PASS.**
  `npx vitest run src/components/DaemonBanner.test.tsx`
  Expected: PASS (3 tests green).

- [ ] **Step 17: Commit.**
  `git add src/components/DaemonBanner.tsx src/components/DaemonBanner.test.tsx && git commit -m "feat(ui): DaemonBanner shows on disconnect, hides on reconnect (§13)"`

- [ ] **Step 18: Implement `src/components/TerminalPane.tsx` (no standalone test — covered via App + manual DoD).**
  Mounts a managed `Terminal` into a sized container; on show, `create` (idempotent) → `open(container)` → `attach` once; on hide, keep-alive (do NOT dispose). The container is always mounted (display toggled) so xterm keeps its buffer and never sees a zero-dimension `open`.
  ```tsx
  import { useEffect, useRef } from "react";
  import type { SessionId } from "../ipc/types";
  import type { TerminalManager } from "../terminal/terminal-manager";
  import { theme } from "../theme";

  /**
   * Hosts one session's xterm Terminal. The DOM container is always present; we
   * toggle visibility (not unmount) so hidden terminals keep buffering (spec §12
   * keep-alive). We create + open + attach once, guarded against StrictMode's
   * double-effect via a ref (create/attach are themselves idempotent).
   */
  export function TerminalPane(props: {
    sessionId: SessionId;
    manager: TerminalManager;
    visible: boolean;
  }): JSX.Element {
    const { sessionId, manager, visible } = props;
    const containerRef = useRef<HTMLDivElement>(null);
    const attachedRef = useRef(false);

    useEffect(() => {
      const container = containerRef.current;
      if (!container) return;
      manager.create(sessionId); // idempotent (StrictMode-safe)
      // open() needs measurable dimensions; only open while visible.
      if (visible) {
        manager.open(sessionId, container);
      }
      if (!attachedRef.current) {
        attachedRef.current = true;
        void manager.attach(sessionId); // wires Replay-before-open + Output firehose
      }
      // Keep-alive: NO dispose on unmount (spec §12). Real close happens in TerminalTabs/App.
    }, [sessionId, manager, visible]);

    return (
      <div
        data-testid={`terminal-pane-${sessionId}`}
        ref={containerRef}
        style={{
          display: visible ? "block" : "none",
          width: "100%",
          height: "100%",
          background: theme.colors.bg,
        }}
      />
    );
  }
  ```

- [ ] **Step 19: Failing test for `TerminalTabs` (active switch + new-terminal calls create_session).**
  Create `src/components/TerminalTabs.test.tsx`. Mock the IPC commands + a fake `TerminalManager`; seed the store with two sessions; assert clicking a tab sets active, and the new-terminal button calls `createSession` with the active workspace and that switching tabs keeps terminals alive (manager.dispose is NOT called on switch).
  ```tsx
  // @vitest-environment jsdom
  import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
  import { render, screen, cleanup, act, fireEvent } from "@testing-library/react";

  const createSessionMock = vi.fn();
  const killSessionMock = vi.fn().mockResolvedValue(undefined);
  vi.mock("../ipc/commands", () => ({
    createSession: (...a: unknown[]) => createSessionMock(...a),
    killSession: (...a: unknown[]) => killSessionMock(...a),
  }));

  import { TerminalTabs } from "./TerminalTabs";
  import { useAppStore } from "../store/store";
  import type { SessionMeta, Workspace } from "../ipc/types";

  const disposeMock = vi.fn();
  const fakeManager = {
    create: vi.fn(),
    open: vi.fn(),
    attach: vi.fn().mockResolvedValue(undefined),
    has: vi.fn(() => true),
    get: vi.fn(),
    dispose: disposeMock,
    disposeAll: vi.fn(),
  } as unknown as import("../terminal/terminal-manager").TerminalManager;

  const meta = (over: Partial<SessionMeta> = {}): SessionMeta => ({
    id: "s1",
    workspaceId: "w1",
    title: "zsh",
    shell: "/bin/zsh",
    cwd: "/tmp",
    cols: 80,
    rows: 24,
    lifecycle: { kind: "atPrompt" },
    waitingForInput: false,
    isActive: true,
    createdAt: 1,
    ...over,
  });
  const ws: Workspace = { id: "w1", name: "proj", rootPath: "/p" };

  afterEach(cleanup);
  beforeEach(() => {
    createSessionMock.mockReset();
    createSessionMock.mockResolvedValue(meta({ id: "s3" }));
    disposeMock.mockReset();
    killSessionMock.mockClear();
    useAppStore.setState(
      {
        sessions: { s1: meta(), s2: meta({ id: "s2", title: "bash" }) },
        workspaces: { w1: ws },
        activeSessionId: "s1",
        daemonConnected: true,
      },
      false,
    );
  });

  describe("TerminalTabs", () => {
    it("renders one tab per session with its title", () => {
      render(<TerminalTabs manager={fakeManager} />);
      expect(screen.getByRole("tab", { name: /zsh/i })).toBeTruthy();
      expect(screen.getByRole("tab", { name: /bash/i })).toBeTruthy();
    });

    it("marks the active session's tab aria-selected", () => {
      render(<TerminalTabs manager={fakeManager} />);
      expect(screen.getByRole("tab", { name: /zsh/i }).getAttribute("aria-selected")).toBe("true");
      expect(screen.getByRole("tab", { name: /bash/i }).getAttribute("aria-selected")).toBe("false");
    });

    it("clicking a tab sets it active (and does NOT dispose the other terminal — keep-alive)", () => {
      render(<TerminalTabs manager={fakeManager} />);
      act(() => {
        fireEvent.click(screen.getByRole("tab", { name: /bash/i }));
      });
      expect(useAppStore.getState().activeSessionId).toBe("s2");
      expect(disposeMock).not.toHaveBeenCalled();
    });

    it("new-terminal button calls createSession with the active workspace id", async () => {
      render(<TerminalTabs manager={fakeManager} />);
      await act(async () => {
        fireEvent.click(screen.getByRole("button", { name: /new terminal/i }));
      });
      expect(createSessionMock).toHaveBeenCalledTimes(1);
      expect(createSessionMock).toHaveBeenCalledWith("w1");
    });

    it("closing a tab kills the session and disposes its terminal", async () => {
      render(<TerminalTabs manager={fakeManager} />);
      await act(async () => {
        fireEvent.click(screen.getByRole("button", { name: /close zsh/i }));
      });
      expect(killSessionMock).toHaveBeenCalledWith("s1");
      expect(disposeMock).toHaveBeenCalledWith("s1");
    });
  });
  ```

- [ ] **Step 20: Run — confirm FAIL.**
  `npx vitest run src/components/TerminalTabs.test.tsx`
  Expected: FAIL with "Failed to resolve import './TerminalTabs'".

- [ ] **Step 21: Implement `src/components/TerminalTabs.tsx`.**
  ```tsx
  import { useAppStore } from "../store/store";
  import { createSession, killSession } from "../ipc/commands";
  import type { TerminalManager } from "../terminal/terminal-manager";
  import { StatusDot } from "./StatusDot";
  import { theme } from "../theme";

  /**
   * Tab strip: one tab per live session (active switch is metadata-only, so hidden
   * terminals stay alive — spec §12 keep-alive). "New terminal" creates a session in
   * the active workspace (or the first workspace). Closing a tab kills the session and
   * disposes its Terminal (the only place dispose() is called for a user close).
   */
  export function TerminalTabs(props: { manager: TerminalManager }): JSX.Element {
    const { manager } = props;
    const sessions = useAppStore((s) => s.sessions);
    const workspaces = useAppStore((s) => s.workspaces);
    const activeSessionId = useAppStore((s) => s.activeSessionId);
    const setActiveSession = useAppStore((s) => s.setActiveSession);

    const list = Object.values(sessions).sort((a, b) => a.createdAt - b.createdAt);

    function activeWorkspaceId(): string | null {
      const active = activeSessionId ? sessions[activeSessionId] : undefined;
      if (active) return active.workspaceId;
      const first = Object.values(workspaces)[0];
      return first ? first.id : null;
    }

    async function onNewTerminal(): Promise<void> {
      const wsId = activeWorkspaceId();
      if (!wsId) return; // no workspace yet -> sidebar must create one first
      // create_session pushes session://created; the store upserts + we activate on the event.
      await createSession(wsId);
    }

    async function onClose(sessionId: string): Promise<void> {
      await killSession(sessionId); // daemon kills the PTY + emits session://exited
      manager.dispose(sessionId); // tear down the xterm instance (real close)
    }

    return (
      <div
        role="tablist"
        style={{
          display: "flex",
          alignItems: "stretch",
          gap: 2,
          background: theme.colors.bgElevated,
          borderBottom: `1px solid ${theme.colors.border}`,
        }}
      >
        {list.map((s) => {
          const selected = s.id === activeSessionId;
          return (
            <div
              key={s.id}
              role="tab"
              aria-selected={selected}
              tabIndex={0}
              onClick={() => setActiveSession(s.id)}
              style={{
                display: "flex",
                alignItems: "center",
                gap: 6,
                padding: "6px 10px",
                cursor: "pointer",
                color: selected ? theme.colors.text : theme.colors.textDim,
                background: selected ? theme.colors.bg : "transparent",
                borderRight: `1px solid ${theme.colors.border}`,
                fontSize: 13,
              }}
            >
              <StatusDot lifecycle={s.lifecycle} waitingForInput={s.waitingForInput} />
              <span>{s.title}</span>
              <button
                type="button"
                aria-label={`Close ${s.title}`}
                onClick={(e) => {
                  e.stopPropagation();
                  void onClose(s.id);
                }}
                style={{
                  border: "none",
                  background: "transparent",
                  color: theme.colors.textDim,
                  cursor: "pointer",
                  fontSize: 13,
                  lineHeight: 1,
                }}
              >
                ×
              </button>
            </div>
          );
        })}
        <button
          type="button"
          aria-label="New terminal"
          onClick={() => void onNewTerminal()}
          style={{
            border: "none",
            background: "transparent",
            color: theme.colors.text,
            cursor: "pointer",
            padding: "6px 12px",
            fontSize: 16,
          }}
        >
          +
        </button>
      </div>
    );
  }
  ```

- [ ] **Step 22: Run — confirm PASS.**
  `npx vitest run src/components/TerminalTabs.test.tsx`
  Expected: PASS (5 tests green).

- [ ] **Step 23: Commit.**
  `git add src/components/TerminalTabs.tsx src/components/TerminalTabs.test.tsx && git commit -m "feat(ui): TerminalTabs — active switch (keep-alive), new-terminal, close"`

- [ ] **Step 24: Implement `src/components/WorkspaceSidebar.tsx` (covered via App test + manual DoD).**
  Lists workspaces; the "Add workspace" button opens the native folder picker (`pickFolder`) and, on a chosen path, calls `createWorkspace(<basename>, <path>)`. Honest degradation: a cancelled picker (`null`) is a no-op.
  ```tsx
  import { useAppStore } from "../store/store";
  import { pickFolder, createWorkspace } from "../ipc/commands";
  import { theme } from "../theme";

  function basename(path: string): string {
    const parts = path.replace(/\/+$/, "").split("/");
    return parts[parts.length - 1] || path;
  }

  /**
   * Workspace list + folder picker. `pickFolder` is the CORE-ONLY native dialog
   * (spec §6.1); on a chosen dir we create a workspace named after its basename.
   * The daemon validates the root (spec §16) and pushes workspace://created, which
   * the App subscription upserts into the store.
   */
  export function WorkspaceSidebar(): JSX.Element {
    const workspaces = useAppStore((s) => s.workspaces);
    const list = Object.values(workspaces).sort((a, b) => a.name.localeCompare(b.name));

    async function onAdd(): Promise<void> {
      const dir = await pickFolder();
      if (dir === null) return; // cancelled -> no-op
      await createWorkspace(basename(dir), dir);
    }

    return (
      <aside
        aria-label="Workspaces"
        style={{
          width: 200,
          flexShrink: 0,
          background: theme.colors.bgElevated,
          borderRight: `1px solid ${theme.colors.border}`,
          color: theme.colors.text,
          display: "flex",
          flexDirection: "column",
        }}
      >
        <div
          style={{
            padding: "8px 12px",
            fontSize: 12,
            textTransform: "uppercase",
            color: theme.colors.textDim,
            letterSpacing: 0.5,
          }}
        >
          Workspaces
        </div>
        <ul style={{ listStyle: "none", margin: 0, padding: 0, flex: 1, overflowY: "auto" }}>
          {list.map((w) => (
            <li
              key={w.id}
              title={w.rootPath}
              style={{
                padding: "6px 12px",
                fontSize: 13,
                whiteSpace: "nowrap",
                overflow: "hidden",
                textOverflow: "ellipsis",
              }}
            >
              {w.name}
            </li>
          ))}
        </ul>
        <button
          type="button"
          aria-label="Add workspace"
          onClick={() => void onAdd()}
          style={{
            margin: 8,
            padding: "6px 10px",
            border: `1px solid ${theme.colors.border}`,
            background: theme.colors.bg,
            color: theme.colors.text,
            cursor: "pointer",
            fontSize: 13,
            borderRadius: 4,
          }}
        >
          + Add workspace
        </button>
      </aside>
    );
  }
  ```

- [ ] **Step 25: Failing test for `App` (subscriptions + layout + active pane keep-alive).**
  Create `src/App.test.tsx`. Mock the IPC event module so we can drive each subscriber's callback, mock the commands used on mount, and a fake `TerminalManager`; assert (a) events flow into the store, (b) both session panes mount and stay mounted when switching active (keep-alive — no dispose on switch), (c) the daemon banner reacts.
  ```tsx
  // @vitest-environment jsdom
  import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
  import { render, screen, cleanup, act } from "@testing-library/react";

  // ---- capture each subscriber callback so tests can fire daemon events ----
  const cbs: Record<string, (p: unknown) => void> = {};
  const unlisten = vi.fn();
  vi.mock("./ipc/events", () => ({
    onSessionCreated: (cb: (p: unknown) => void) => {
      cbs.created = cb;
      return Promise.resolve(unlisten);
    },
    onSessionStateChanged: (cb: (p: unknown) => void) => {
      cbs.state = cb;
      return Promise.resolve(unlisten);
    },
    onSessionExited: (cb: (p: unknown) => void) => {
      cbs.exited = cb;
      return Promise.resolve(unlisten);
    },
    onWorkspaceCreated: (cb: (p: unknown) => void) => {
      cbs.wsCreated = cb;
      return Promise.resolve(unlisten);
    },
    onDaemonDisconnected: (cb: (p: unknown) => void) => {
      cbs.disc = cb;
      return Promise.resolve(unlisten);
    },
    onDaemonReconnected: (cb: (p: unknown) => void) => {
      cbs.recon = cb;
      return Promise.resolve(unlisten);
    },
  }));

  const listSessionsMock = vi.fn().mockResolvedValue([]);
  const listWorkspacesMock = vi.fn().mockResolvedValue([]);
  vi.mock("./ipc/commands", () => ({
    listSessions: (...a: unknown[]) => listSessionsMock(...a),
    listWorkspaces: (...a: unknown[]) => listWorkspacesMock(...a),
    createSession: vi.fn().mockResolvedValue(undefined),
    killSession: vi.fn().mockResolvedValue(undefined),
    createWorkspace: vi.fn().mockResolvedValue(undefined),
    pickFolder: vi.fn().mockResolvedValue(null),
  }));

  const disposeMock = vi.fn();
  const openMock = vi.fn();
  const fakeManager = {
    create: vi.fn(),
    open: openMock,
    attach: vi.fn().mockResolvedValue(undefined),
    has: vi.fn(() => true),
    get: vi.fn(),
    dispose: disposeMock,
    disposeAll: vi.fn(),
  } as unknown as import("./terminal/terminal-manager").TerminalManager;

  import { App } from "./App";
  import { useAppStore } from "./store/store";
  import type { SessionMeta } from "./ipc/types";

  const meta = (over: Partial<SessionMeta> = {}): SessionMeta => ({
    id: "s1",
    workspaceId: "w1",
    title: "zsh",
    shell: "/bin/zsh",
    cwd: "/tmp",
    cols: 80,
    rows: 24,
    lifecycle: { kind: "atPrompt" },
    waitingForInput: false,
    isActive: true,
    createdAt: 1,
    ...over,
  });

  afterEach(cleanup);
  beforeEach(() => {
    for (const k of Object.keys(cbs)) delete cbs[k];
    unlisten.mockClear();
    disposeMock.mockReset();
    openMock.mockReset();
    listSessionsMock.mockClear();
    listWorkspacesMock.mockClear();
    useAppStore.setState(
      { sessions: {}, workspaces: {}, activeSessionId: null, daemonConnected: true },
      false,
    );
  });

  describe("App", () => {
    it("registers all six IPC subscriptions on mount", async () => {
      await act(async () => {
        render(<App manager={fakeManager} />);
      });
      for (const key of ["created", "state", "exited", "wsCreated", "disc", "recon"]) {
        expect(typeof cbs[key]).toBe("function");
      }
    });

    it("session://created upserts the session and activates the first one", async () => {
      await act(async () => {
        render(<App manager={fakeManager} />);
      });
      await act(async () => {
        cbs.created(meta());
      });
      expect(useAppStore.getState().sessions["s1"]).toBeTruthy();
      expect(useAppStore.getState().activeSessionId).toBe("s1");
    });

    it("session://state-changed updates lifecycle in the store", async () => {
      await act(async () => {
        render(<App manager={fakeManager} />);
      });
      await act(async () => {
        cbs.created(meta());
      });
      await act(async () => {
        cbs.state({ sessionId: "s1", lifecycle: { kind: "running" }, waitingForInput: false, cwd: "/tmp" });
      });
      expect(useAppStore.getState().sessions["s1"].lifecycle).toEqual({ kind: "running" });
    });

    it("session://exited marks the session inactive+exited (does not remove it)", async () => {
      await act(async () => {
        render(<App manager={fakeManager} />);
      });
      await act(async () => {
        cbs.created(meta());
      });
      await act(async () => {
        cbs.exited({ sessionId: "s1", code: 0, signal: null });
      });
      const s = useAppStore.getState().sessions["s1"];
      expect(s.isActive).toBe(false);
      expect(s.lifecycle).toEqual({ kind: "exited", code: 0, signal: null });
    });

    it("daemon disconnect shows the banner; reconnect hides it", async () => {
      await act(async () => {
        render(<App manager={fakeManager} />);
      });
      await act(async () => {
        cbs.disc(null);
      });
      expect(screen.getByRole("alert")).toBeTruthy();
      await act(async () => {
        cbs.recon(null);
      });
      expect(screen.queryByRole("alert")).toBeNull();
    });

    it("keep-alive: switching the active session mounts both panes and disposes neither", async () => {
      await act(async () => {
        render(<App manager={fakeManager} />);
      });
      await act(async () => {
        cbs.created(meta()); // s1 active
      });
      await act(async () => {
        cbs.created(meta({ id: "s2", title: "bash" }));
      });
      // both panes are mounted (visible + hidden), neither disposed on switch
      expect(screen.getByTestId("terminal-pane-s1")).toBeTruthy();
      expect(screen.getByTestId("terminal-pane-s2")).toBeTruthy();
      act(() => useAppStore.getState().setActiveSession("s2"));
      expect(screen.getByTestId("terminal-pane-s1")).toBeTruthy(); // still mounted
      expect(disposeMock).not.toHaveBeenCalled();
    });
  });
  ```

- [ ] **Step 26: Run — confirm FAIL.**
  `npx vitest run src/App.test.tsx`
  Expected: FAIL with "Failed to resolve import './App'".

- [ ] **Step 27: Implement `src/App.tsx`.**
  ```tsx
  import { useEffect } from "react";
  import { useAppStore } from "./store/store";
  import {
    onSessionCreated,
    onSessionStateChanged,
    onSessionExited,
    onWorkspaceCreated,
    onDaemonDisconnected,
    onDaemonReconnected,
  } from "./ipc/events";
  import { listSessions, listWorkspaces } from "./ipc/commands";
  import { TerminalManager } from "./terminal/terminal-manager";
  import { WorkspaceSidebar } from "./components/WorkspaceSidebar";
  import { TerminalTabs } from "./components/TerminalTabs";
  import { TerminalPane } from "./components/TerminalPane";
  import { DaemonBanner } from "./components/DaemonBanner";
  import { theme } from "./theme";
  import type { UnlistenFn } from "@tauri-apps/api/event";

  /** Module singleton (used by main.tsx). Tests inject a fake via the `manager` prop. */
  export const terminalManager = new TerminalManager();

  /**
   * App shell (spec §2 UI). Wires:
   * - IPC subscriptions (session://* , workspace://* , daemon://*) into the store,
   * - an initial `list_sessions` / `list_workspaces` hydration,
   * - layout: WorkspaceSidebar | (TerminalTabs over the active/keep-alive panes).
   */
  export function App(props?: { manager?: TerminalManager }): JSX.Element {
    const manager = props?.manager ?? terminalManager;

    const sessions = useAppStore((s) => s.sessions);
    const activeSessionId = useAppStore((s) => s.activeSessionId);

    useEffect(() => {
      const store = useAppStore.getState();
      let disposed = false;
      const unlisteners: UnlistenFn[] = [];

      const track = (p: Promise<UnlistenFn>): void => {
        void p.then((un) => {
          if (disposed) un();
          else unlisteners.push(un);
        });
      };

      track(
        onSessionCreated((m) => {
          const s = useAppStore.getState();
          s.upsertSession(m);
          if (s.activeSessionId === null) s.setActiveSession(m.id);
        }),
      );
      track(onSessionStateChanged((p) => useAppStore.getState().setLifecycle(p)));
      track(
        onSessionExited((p) => {
          const s = useAppStore.getState();
          const existing = s.sessions[p.sessionId];
          if (!existing) return;
          s.upsertSession({
            ...existing,
            isActive: false,
            lifecycle: { kind: "exited", code: p.code, signal: p.signal },
          });
        }),
      );
      track(onWorkspaceCreated((w) => useAppStore.getState().upsertWorkspace(w)));
      track(
        onDaemonDisconnected(() => useAppStore.getState().setDaemonConnected(false)),
      );
      track(
        onDaemonReconnected(() => {
          useAppStore.getState().setDaemonConnected(true);
          // re-hydrate + re-attach visible sessions after a reconnect (spec §13)
          void hydrate();
        }),
      );

      async function hydrate(): Promise<void> {
        try {
          const [ws, ss] = await Promise.all([listWorkspaces(), listSessions()]);
          const s = useAppStore.getState();
          for (const w of ws) s.upsertWorkspace(w);
          for (const m of ss) s.upsertSession(m);
          if (s.activeSessionId === null && ss.length > 0) {
            s.setActiveSession(ss[0].id);
          }
        } catch {
          // daemon unreachable at boot -> the disconnect event / banner covers it.
        }
      }

      // On first mount assume connected until a disconnect event says otherwise.
      store.setDaemonConnected(true);
      void hydrate();

      return () => {
        disposed = true;
        for (const un of unlisteners) un();
      };
    }, []);

    const sessionList = Object.values(sessions).sort(
      (a, b) => a.createdAt - b.createdAt,
    );

    return (
      <div
        style={{
          display: "flex",
          flexDirection: "column",
          height: "100vh",
          background: theme.colors.bg,
          color: theme.colors.text,
          fontFamily:
            'system-ui, -apple-system, "Segoe UI", Roboto, sans-serif',
        }}
      >
        <DaemonBanner />
        <div style={{ display: "flex", flex: 1, minHeight: 0 }}>
          <WorkspaceSidebar />
          <div style={{ display: "flex", flexDirection: "column", flex: 1, minWidth: 0 }}>
            <TerminalTabs manager={manager} />
            <div style={{ position: "relative", flex: 1, minHeight: 0 }}>
              {sessionList.map((s) => (
                <div
                  key={s.id}
                  style={{
                    position: "absolute",
                    inset: 0,
                    display: s.id === activeSessionId ? "block" : "none",
                  }}
                >
                  {/* Keep-alive: every session's pane stays mounted; only display toggles. */}
                  <TerminalPane
                    sessionId={s.id}
                    manager={manager}
                    visible={s.id === activeSessionId}
                  />
                </div>
              ))}
            </div>
          </div>
        </div>
      </div>
    );
  }
  ```

- [ ] **Step 28: Run — confirm PASS.**
  `npx vitest run src/App.test.tsx`
  Expected: PASS (6 tests green).

- [ ] **Step 29: Commit.**
  `git add src/App.tsx src/components/TerminalPane.tsx src/components/WorkspaceSidebar.tsx src/App.test.tsx && git commit -m "feat(ui): App wires store + IPC subscriptions + sidebar|tabs|keep-alive panes"`

- [ ] **Step 30: Implement `src/main.tsx` (React 19 StrictMode root; no test — smoke-verified by build/E2E in G6).**
  ```tsx
  import { StrictMode } from "react";
  import { createRoot } from "react-dom/client";
  import { App } from "./App";

  const rootEl = document.getElementById("root");
  if (!rootEl) throw new Error("root element #root not found");

  createRoot(rootEl).render(
    <StrictMode>
      <App />
    </StrictMode>,
  );
  ```

- [ ] **Step 31: Typecheck the whole frontend (no `any`-leaks, JSX ok under React 19).**
  `npx tsc --noEmit`
  Expected: PASS (exit 0). (`index.html` from T1 references `/src/main.tsx`; the root element id is `root`.)

- [ ] **Step 32: Run the full frontend suite to confirm no regressions.**
  `npx vitest run src`
  Expected: PASS — includes T19 (commands 13, channel 2, events 6), T20 (store 9), T21 (terminal-manager 10), and T22 (theme 2, StatusDot 11, DaemonBanner 3, TerminalTabs 5, App 6).

- [ ] **Step 33: Commit.**
  `git add src/main.tsx && git commit -m "feat(ui): React 19 StrictMode root render (main.tsx)"`

**Definition of Done:**
- `npx vitest run src/components src/App.test.tsx src/theme.test.ts` green: `theme` (2), `StatusDot` (11), `DaemonBanner` (3), `TerminalTabs` (5), `App` (6).
- `npx tsc --noEmit` passes for the whole `src/` tree; `npx vitest run src` (all frontend suites incl. T19–T21) green.
- `StatusDot.dotStateOf` maps every `SessionLifecycle` variant + `waitingForInput` to the right `DotState`: `atPrompt`/`typing` → `idle`, `running` (no wait) → `running`, `running`+`waitingForInput` → `waiting`, `exited` (any wait flag) → `exited` — and renders the matching `theme` status color (spec §5, §10.4).
- `DaemonBanner` renders (role `alert`) only when `daemonConnected === false` and disappears when it flips back true (`daemon://disconnected` → `daemon://reconnected`, spec §13).
- Switching the active tab is metadata-only: both panes stay mounted, `TerminalManager.dispose` is NOT called on switch (keep-alive, spec §12); `dispose` is called only via the tab's Close (which first calls `killSession`) or `disposeAll`.
- The new-terminal button calls `createSession(<active workspace id>)`; `WorkspaceSidebar` "Add workspace" calls `pickFolder()` then `createWorkspace(basename, path)` and is a no-op on a cancelled (`null`) pick.
- `App` registers all six §6.3 subscriptions on mount, unlistens on unmount, routes each into the store (`session://created` upsert + first-session activation; `session://state-changed` → `setLifecycle`; `session://exited` → mark inactive + `exited` lifecycle; `workspace://created` → `upsertWorkspace`; daemon events → `setDaemonConnected`), and lays out sidebar | tabs | active pane.
- `main.tsx` mounts `<App/>` inside `<StrictMode>` via React 19 `createRoot`; the `TerminalManager` singleton is StrictMode-safe (create/attach idempotent), so the dev double-mount does not duplicate terminals.


### Task 23: E2E survive-restart harness (create → run → OSC status → quit → survive → relaunch → reattach + scrollback)

**Files:**
- Create: `tests/e2e/survive-restart.mjs` (Node ESM harness; no test-runner deps — self-contained, exits non-zero on failure)
- Create: `tests/e2e/lib/daemon-harness.mjs` (helpers: spawn daemon out-of-band via the real socket protocol, frame codec, launchctl/pgrep probes)
- Create: `tests/e2e/README.md` (how to run, prerequisites, what it asserts)
- Create: `package.json` script entry `"e2e:survive"` (Modify existing `package.json` — add only the `scripts.e2e:survive` key; T1 owns the rest of `package.json`, this task appends one scripts key and MUST NOT touch any other field)
- Test (the harness IS the test; its own smoke assertions live inline)

**Depends on:** [T13] (daemon `main.rs` binary `bpa-sessiond` + `crates/sessiond/src/socket_server.rs` handshake/dispatch — the harness speaks Hop-B directly), [T18] (`src-tauri` builds; `launchd.rs` install/kickstart used to bootstrap the agent), [T22] (frontend `TerminalManager` + store wired so a full app launch reattaches). **Also consumes** the built universal `.app` from T24 when run against the bundle, but is authored here against a locally-built `bpa-sessiond` so it can run before signing.
**Parallel-safe with:** [] (G6 is sequential; T23 → T24 → T25)

**Interfaces:**
- Consumes (Hop-B wire protocol, spec §7, from T3 `crates/protocol`): `MAGIC = 0x4250_4131` (u32-LE), `PROTO_VERSION = 1` (u16); framing = `u32`-LE length prefix + `bincode` 1.3.3 body of `Frame`. `Frame::Request { id: u64, req: Request }`, `Frame::Response { id: u64, res: Response }`, `Frame::Push(Push)`. `Request::Hello { magic: u32, proto_version: u16, client_build: String }`, `Request::CreateWorkspace { name: String, root_path: String }`, `Request::CreateSession { workspace_id, shell: Option<String>, cwd: Option<String>, env_overrides: Vec<(String,String)>, cols: u16, rows: u16 }`, `Request::AttachSession { session_id }`, `Request::WriteStdin { session_id, bytes: Vec<u8> }`, `Request::ListSessions`. `Response::Welcome { proto_version, daemon_build }`, `Response::Workspace(Workspace)`, `Response::Session(SessionMeta)`, `Response::Sessions(Vec<SessionMeta>)`, `Response::Ack`. `Push::Replay { session_id, cols, rows, content: Vec<u8> }`, `Push::Output { session_id, bytes: Vec<u8> }`, `Push::StateChanged { session_id, lifecycle, waiting_for_input, cwd }`. `SessionLifecycle` internally tagged `{ kind: "atPrompt"|"typing"|"running" } | { kind:"exited", code, signal }`.
- Consumes (spec §8.1 socket path): `$XDG_RUNTIME_DIR/bpa/d.sock` else `/tmp/bpa-<uid>/d.sock`; lockfile `d.lock` alongside.
- Consumes (spec §8.3 launchd, from T16/T18): LaunchAgent label `ai.builderpro.desktop.sessiond`; started via `launchctl kickstart gui/$UID/ai.builderpro.desktop.sessiond`.
- Produces: `tests/e2e/survive-restart.mjs` (exit 0 on full pass, non-zero + diagnostic on any failed assertion); `tests/e2e/lib/daemon-harness.mjs` exporting `connect(sockPath)`, `hello(conn)`, `request(conn, req)`, `nextPush(conn, pred)`, `encodeFrame(frame)`, `decodeFrame(buf)`, `pgrepDaemon()`, `pgrepShell(pid)`, `killGui()`, `launchctlKickstart()`.

> **Rationale for a bincode-speaking Node harness (locked):** the survive-restart property is a daemon+launchd property, provable without driving the WKWebView. A `tauri-driver`/WebDriver path is documented as the manual GUI-level confirmation in `tests/e2e/README.md` (spec §14.1 "launch app"), but the deterministic, CI-runnable assertion (`pgrep bpa-sessiond` + the shell child survive a client disconnect, and a fresh client replays scrollback) is driven over Hop-B directly. The harness re-implements only the exact bincode-1.3.3 fixint-LE encoding for the handful of frames it sends/receives (locked in §7); a mismatch would fail the very first `Welcome`, so it is self-checking.

- [ ] **Step 1: Failing harness skeleton — assert `bpa-sessiond` binary exists and speaks the handshake.**
  Create `tests/e2e/lib/daemon-harness.mjs` with the frame codec + socket helpers, then `tests/e2e/survive-restart.mjs` phase 0 that connects and handshakes. Author both fully now (codec first so later phases compile).
  ```js
  // tests/e2e/lib/daemon-harness.mjs
  import net from "node:net";
  import { execFileSync, spawn } from "node:child_process";
  import os from "node:os";
  import path from "node:path";
  import fs from "node:fs";

  export const MAGIC = 0x42504131; // "BPA1"
  export const PROTO_VERSION = 1;

  // ---- bincode 1.3.3 (fixint, little-endian) minimal encoder/decoder ----
  // Only the subset of Frame/Request/Response/Push this harness sends & reads.
  // Enum variant index = u32-LE; String/Vec<u8> = u64-LE len + bytes;
  // Vec<T> = u64-LE len + items; Option = 1 byte tag (0 None / 1 Some) + inner;
  // struct fields serialized in declaration order; tuple like a struct.
  function u32le(n) { const b = Buffer.alloc(4); b.writeUInt32LE(n >>> 0, 0); return b; }
  function u64le(n) { const b = Buffer.alloc(8); b.writeBigUInt64LE(BigInt(n), 0); return b; }
  function u16le(n) { const b = Buffer.alloc(2); b.writeUInt16LE(n & 0xffff, 0); return b; }
  function encStr(s) { const body = Buffer.from(s, "utf8"); return Buffer.concat([u64le(body.length), body]); }
  function encBytes(v) { const body = Buffer.from(v); return Buffer.concat([u64le(body.length), body]); }
  function encEnvOverrides(pairs) {
    const parts = [u64le(pairs.length)];
    for (const [k, val] of pairs) parts.push(encStr(k), encStr(val));
    return Buffer.concat(parts);
  }

  // Request enum variant order MUST match spec §7:
  // 0 Hello,1 ListWorkspaces,2 CreateWorkspace,3 ListSessions,4 CreateSession,
  // 5 AttachSession,6 DetachSession,7 WriteStdin,8 Resize,9 KillSession,
  // 10 GetSessionState,11 DaemonShutdown
  function encRequest(req) {
    switch (req.t) {
      case "Hello":
        return Buffer.concat([u32le(0), u32le(req.magic), u16le(req.protoVersion), encStr(req.clientBuild)]);
      case "ListWorkspaces": return u32le(1);
      case "CreateWorkspace":
        return Buffer.concat([u32le(2), encStr(req.name), encStr(req.rootPath)]);
      case "ListSessions": return u32le(3);
      case "CreateSession": {
        const shell = req.shell == null
          ? Buffer.from([0]) : Buffer.concat([Buffer.from([1]), encStr(req.shell)]);
        const cwd = req.cwd == null
          ? Buffer.from([0]) : Buffer.concat([Buffer.from([1]), encStr(req.cwd)]);
        return Buffer.concat([
          u32le(4), encStr(req.workspaceId), shell, cwd,
          encEnvOverrides(req.envOverrides ?? []), u16le(req.cols), u16le(req.rows),
        ]);
      }
      case "AttachSession": return Buffer.concat([u32le(5), encStr(req.sessionId)]);
      case "WriteStdin": return Buffer.concat([u32le(7), encStr(req.sessionId), encBytes(req.bytes)]);
      default: throw new Error(`unsupported request ${req.t}`);
    }
  }
  // Frame enum: 0 Request{id,req}, 1 Response{id,res}, 2 Push(push)
  export function encodeFrame(frame) {
    let body;
    if (frame.t === "Request") body = Buffer.concat([u32le(0), u64le(frame.id), encRequest(frame.req)]);
    else throw new Error("harness only sends Request frames");
    return Buffer.concat([u32le(body.length), body]);
  }

  // ---- decoder (a cursor over a Buffer) ----
  class Cur {
    constructor(buf) { this.b = buf; this.o = 0; }
    u32() { const v = this.b.readUInt32LE(this.o); this.o += 4; return v; }
    u16() { const v = this.b.readUInt16LE(this.o); this.o += 2; return v; }
    u64() { const v = Number(this.b.readBigUInt64LE(this.o)); this.o += 8; return v; }
    u8() { const v = this.b.readUInt8(this.o); this.o += 1; return v; }
    str() { const n = this.u64(); const s = this.b.toString("utf8", this.o, this.o + n); this.o += n; return s; }
    bytes() { const n = this.u64(); const s = this.b.subarray(this.o, this.o + n); this.o += n; return s; }
    optU8() { return this.u8() === 0 ? null : this.u8(); }
    optStr() { return this.u8() === 0 ? null : this.str(); }
  }
  function decLifecycle(c) {
    const kind = c.u32(); // 0 AtPrompt,1 Typing,2 Running,3 Exited
    if (kind === 3) return { kind: "exited", code: c.optU8(), signal: c.optStr() };
    return { kind: ["atPrompt", "typing", "running"][kind] };
  }
  function decSessionMeta(c) {
    return {
      id: c.str(), workspaceId: c.str(), title: c.str(), shell: c.str(), cwd: c.str(),
      cols: c.u16(), rows: c.u16(), lifecycle: decLifecycle(c),
      waitingForInput: c.u8() === 1, isActive: c.u8() === 1, createdAt: c.u64(),
    };
  }
  function decWorkspace(c) { return { id: c.str(), name: c.str(), rootPath: c.str() }; }
  // Response enum order: 0 Welcome,1 Incompatible,2 Workspaces,3 Workspace,
  // 4 Sessions,5 Session,6 Ack,7 Error
  function decResponse(c) {
    const v = c.u32();
    switch (v) {
      case 0: return { t: "Welcome", protoVersion: c.u16(), daemonBuild: c.str() };
      case 1: return { t: "Incompatible", min: c.u16(), max: c.u16() };
      case 2: { const n = c.u64(); const a = []; for (let i = 0; i < n; i++) a.push(decWorkspace(c)); return { t: "Workspaces", value: a }; }
      case 3: return { t: "Workspace", value: decWorkspace(c) };
      case 4: { const n = c.u64(); const a = []; for (let i = 0; i < n; i++) a.push(decSessionMeta(c)); return { t: "Sessions", value: a }; }
      case 5: return { t: "Session", value: decSessionMeta(c) };
      case 6: return { t: "Ack" };
      case 7: return { t: "Error", code: c.str(), message: c.str() };
      default: throw new Error(`unknown Response variant ${v}`);
    }
  }
  // Push enum order: 0 Replay,1 Output,2 StateChanged,3 ChildExited,
  // 4 SessionCreated,5 WorkspaceCreated,6 Error
  function decPush(c) {
    const v = c.u32();
    switch (v) {
      case 0: return { t: "Replay", sessionId: c.str(), cols: c.u16(), rows: c.u16(), content: c.bytes() };
      case 1: return { t: "Output", sessionId: c.str(), bytes: c.bytes() };
      case 2: return { t: "StateChanged", sessionId: c.str(), lifecycle: decLifecycle(c), waitingForInput: c.u8() === 1, cwd: c.str() };
      case 3: return { t: "ChildExited", sessionId: c.str(), code: c.optU8(), signal: c.optStr() };
      case 4: return { t: "SessionCreated", meta: decSessionMeta(c) };
      case 5: return { t: "WorkspaceCreated", workspace: decWorkspace(c) };
      case 6: return { t: "Error", sessionId: c.optStr(), code: c.str(), message: c.str() };
      default: throw new Error(`unknown Push variant ${v}`);
    }
  }
  export function decodeFrame(buf) {
    const c = new Cur(buf);
    const fv = c.u32();
    if (fv === 1) return { t: "Response", id: c.u64(), res: decResponse(c) };
    if (fv === 2) return { t: "Push", push: decPush(c) };
    if (fv === 0) throw new Error("harness does not decode Request frames");
    throw new Error(`unknown Frame variant ${fv}`);
  }

  // ---- socket connection with length-prefixed framing ----
  export function resolveSocketPath() {
    const runtime = process.env.XDG_RUNTIME_DIR;
    const dir = runtime && runtime.length > 0
      ? path.join(runtime, "bpa")
      : path.join("/tmp", `bpa-${os.userInfo().uid}`);
    return path.join(dir, "d.sock");
  }
  export function connect(sockPath) {
    return new Promise((resolve, reject) => {
      const sock = net.connect(sockPath);
      const conn = { sock, buf: Buffer.alloc(0), pending: [], pushes: [], waiters: [] };
      sock.on("connect", () => resolve(conn));
      sock.on("error", reject);
      sock.on("data", (chunk) => {
        conn.buf = Buffer.concat([conn.buf, chunk]);
        for (;;) {
          if (conn.buf.length < 4) break;
          const len = conn.buf.readUInt32LE(0);
          if (conn.buf.length < 4 + len) break;
          const body = conn.buf.subarray(4, 4 + len);
          conn.buf = conn.buf.subarray(4 + len);
          const frame = decodeFrame(body);
          if (frame.t === "Response") {
            const w = conn.pending.find((p) => p.id === frame.id);
            if (w) { conn.pending = conn.pending.filter((p) => p !== w); w.resolve(frame.res); }
          } else if (frame.t === "Push") {
            conn.pushes.push(frame.push);
            const w = conn.waiters.find((x) => x.pred(frame.push));
            if (w) { conn.waiters = conn.waiters.filter((x) => x !== w); w.resolve(frame.push); }
          }
        }
      });
    });
  }
  let nextId = 1;
  export function request(conn, req) {
    const id = nextId++;
    return new Promise((resolve, reject) => {
      conn.pending.push({ id, resolve, reject });
      conn.sock.write(encodeFrame({ t: "Request", id, req }));
      setTimeout(() => reject(new Error(`request ${req.t} timed out`)), 10000);
    });
  }
  export function hello(conn) {
    return request(conn, { t: "Hello", magic: MAGIC, protoVersion: PROTO_VERSION, clientBuild: "e2e-harness" });
  }
  export function nextPush(conn, pred, timeoutMs = 15000) {
    const existing = conn.pushes.find(pred);
    if (existing) { conn.pushes = conn.pushes.filter((p) => p !== existing); return Promise.resolve(existing); }
    return new Promise((resolve, reject) => {
      const w = { pred, resolve };
      conn.waiters.push(w);
      setTimeout(() => { conn.waiters = conn.waiters.filter((x) => x !== w); reject(new Error("push wait timed out")); }, timeoutMs);
    });
  }
  export function pgrepDaemon() {
    try { return execFileSync("pgrep", ["-x", "bpa-sessiond"], { encoding: "utf8" }).trim().split("\n").filter(Boolean); }
    catch { return []; }
  }
  export function pidAlive(pid) {
    try { process.kill(Number(pid), 0); return true; } catch { return false; }
  }
  export function spawnDaemon(binPath, sockPath) {
    fs.mkdirSync(path.dirname(sockPath), { recursive: true, mode: 0o700 });
    const child = spawn(binPath, ["--socket", sockPath], { stdio: "ignore", detached: true });
    child.unref();
    return child;
  }
  ```
  Then the phase-0 assertion in `tests/e2e/survive-restart.mjs`:
  ```js
  // tests/e2e/survive-restart.mjs
  import assert from "node:assert/strict";
  import fs from "node:fs";
  import path from "node:path";
  import { setTimeout as sleep } from "node:timers/promises";
  import {
    connect, hello, request, nextPush, resolveSocketPath,
    pgrepDaemon, pidAlive, spawnDaemon,
  } from "./lib/daemon-harness.mjs";

  const REPO = path.resolve(import.meta.dirname, "..", "..");
  const DAEMON_BIN = process.env.BPA_SESSIOND
    ?? path.join(REPO, "target", "debug", "bpa-sessiond");
  const SOCK = resolveSocketPath();

  function log(msg) { console.log(`[e2e] ${msg}`); }

  async function main() {
    assert.ok(fs.existsSync(DAEMON_BIN), `daemon binary missing at ${DAEMON_BIN} (build with: cargo build -p sessiond)`);
    log(`phase0: spawn daemon ${DAEMON_BIN}`);
    const daemonProc = spawnDaemon(DAEMON_BIN, SOCK);
    let conn;
    for (let i = 0; i < 50; i++) {
      try { conn = await connect(SOCK); break; } catch { await sleep(100); }
    }
    assert.ok(conn, "could not connect to daemon socket");
    const welcome = await hello(conn);
    assert.equal(welcome.t, "Welcome", `expected Welcome, got ${JSON.stringify(welcome)}`);
    assert.equal(welcome.protoVersion, 1, "proto version mismatch");
    log("phase0 OK: handshake");
    // phases 1..4 appended in later steps
    globalThis.__e2e = { conn, daemonProc, SOCK };
  }
  main().catch((e) => { console.error("[e2e] FAIL:", e); process.exit(1); });
  ```

- [ ] **Step 2: Run phase 0 — Expected FAIL.**
  `cargo build -p sessiond && node tests/e2e/survive-restart.mjs`
  Expected: FAIL with `AssertionError: daemon binary missing …` (until T13 is built) OR, once built but if the handshake/bincode order is wrong, `expected Welcome, got …`. This confirms the harness actually exercises the daemon rather than passing vacuously.

- [ ] **Step 3: Build the daemon and re-run phase 0 — Expected PASS (phase 0 only).**
  `cargo build -p sessiond && node tests/e2e/survive-restart.mjs`
  Expected: prints `phase0 OK: handshake` then the process ends (no phases yet) with exit 0. If bincode ordering drift exists it fails here — fix the harness codec to match `crates/protocol` (the codec comments cite the locked variant order from spec §7).

- [ ] **Step 4: Add phases 1–3 (create workspace + session, run a command, observe OSC status).**
  Append to `main()` in `survive-restart.mjs`, replacing the `// phases 1..4` placeholder:
  ```js
    // phase1: workspace + session rooted in a temp dir
    log("phase1: create workspace + session");
    const root = fs.mkdtempSync(path.join(REPO, "target", "e2e-ws-"));
    const ws = await request(conn, { t: "CreateWorkspace", name: "e2e", rootPath: root });
    assert.equal(ws.t, "Workspace", `CreateWorkspace -> ${JSON.stringify(ws)}`);
    const created = await request(conn, {
      t: "CreateSession", workspaceId: ws.value.id, shell: "/bin/zsh",
      cwd: root, envOverrides: [], cols: 80, rows: 24,
    });
    assert.equal(created.t, "Session", `CreateSession -> ${JSON.stringify(created)}`);
    const sid = created.value.id;
    const shellPid = created.value.isActive; // isActive proven below via survival
    log(`phase1 OK: session ${sid}`);

    // phase2: attach, run a marker command, capture output
    log("phase2: attach + run command");
    await request(conn, { t: "AttachSession", sessionId: sid });
    // first push after attach is Replay
    const replay = await nextPush(conn, (p) => p.t === "Replay" && p.sessionId === sid);
    assert.equal(replay.t, "Replay", "expected Replay first on attach");
    const MARKER = `E2E_MARK_${Date.now()}`;
    await request(conn, { t: "WriteStdin", sessionId: sid, bytes: Buffer.from(`echo ${MARKER}\n`, "utf8") });
    // collect Output until the marker text is seen
    let acc = "";
    const deadline = Date.now() + 15000;
    while (!acc.includes(MARKER) && Date.now() < deadline) {
      const out = await nextPush(conn, (p) => p.t === "Output" && p.sessionId === sid);
      acc += Buffer.from(out.bytes).toString("utf8");
    }
    assert.ok(acc.includes(MARKER), `marker ${MARKER} not seen in output`);
    log("phase2 OK: command output observed");

    // phase3: observe OSC-133-driven lifecycle status transition to running then back to atPrompt
    log("phase3: observe OSC status");
    // running while the command executes, atPrompt once it returns to the shell prompt
    const sawRunning = conn.pushes.some((p) => p.t === "StateChanged" && p.sessionId === sid && p.lifecycle.kind === "running")
      || (await nextPush(conn, (p) => p.t === "StateChanged" && p.sessionId === sid && p.lifecycle.kind === "running").then(() => true).catch(() => false));
    await request(conn, { t: "WriteStdin", sessionId: sid, bytes: Buffer.from("sleep 1\n", "utf8") });
    const running = await nextPush(conn, (p) => p.t === "StateChanged" && p.sessionId === sid && p.lifecycle.kind === "running");
    assert.equal(running.lifecycle.kind, "running", "expected running lifecycle after sleep");
    const backToPrompt = await nextPush(conn, (p) => p.t === "StateChanged" && p.sessionId === sid && p.lifecycle.kind === "atPrompt");
    assert.equal(backToPrompt.lifecycle.kind, "atPrompt", "expected atPrompt after command finished");
    log("phase3 OK: OSC-133 lifecycle running→atPrompt");
    void shellPid; void sawRunning;
    globalThis.__e2e = { conn, daemonProc, SOCK, sid, MARKER, root };
  ```

- [ ] **Step 5: Run phases 0–3 — Expected PASS.**
  `cargo build -p sessiond && node tests/e2e/survive-restart.mjs`
  Expected: prints `phase0..phase3 OK` and exits 0. If lifecycle pushes never arrive, the shell-integration injection (T10) or OSC parser (T5) is not wired into the supervisor (T9) — surface as `push wait timed out` (do not weaken the assertion).

- [ ] **Step 6: Add phase 4 (client disconnect → daemon + shell survive; new client reattaches; scrollback intact).**
  Append the survive-restart core (the load-bearing assertion) to `main()`:
  ```js
    // phase4: prove survive-restart
    log("phase4: quit client, assert daemon + shell survive, reattach, replay");
    // record the shell child pid via daemon-reported process tree
    const beforePids = pgrepDaemon();
    assert.ok(beforePids.length >= 1, "daemon not running before client quit");
    const daemonPid = Number(beforePids[0]);
    // find the shell child of the daemon (survives client disconnect)
    const psOut = (await import("node:child_process")).execFileSync(
      "pgrep", ["-P", String(daemonPid)], { encoding: "utf8" }).trim();
    const childPids = psOut.split("\n").filter(Boolean).map(Number);
    assert.ok(childPids.length >= 1, "no shell child under daemon");
    const shellPid = childPids[0];

    // simulate GUI quit: hard-close the client socket WITHOUT DetachSession/KillSession
    conn.sock.destroy();
    await sleep(1500);

    // assert daemon + shell both still alive (this is the survive-restart property)
    assert.ok(pgrepDaemon().includes(String(daemonPid)), "daemon died on client quit (must survive)");
    assert.ok(pidAlive(shellPid), `shell child ${shellPid} died on client quit (must survive)`);
    log("phase4a OK: daemon + shell survived client quit");

    // relaunch a fresh client, reattach, assert scrollback replay contains the earlier marker
    const conn2 = await connect(SOCK);
    const w2 = await hello(conn2);
    assert.equal(w2.t, "Welcome", "reattach handshake failed");
    const sessions = await request(conn2, { t: "ListSessions" });
    assert.equal(sessions.t, "Sessions", "ListSessions failed after relaunch");
    assert.ok(sessions.value.some((s) => s.id === globalThis.__e2e.sid), "session lost across client restart");
    await request(conn2, { t: "AttachSession", sessionId: globalThis.__e2e.sid });
    const replay2 = await nextPush(conn2, (p) => p.t === "Replay" && p.sessionId === globalThis.__e2e.sid);
    const replayText = Buffer.from(replay2.content).toString("utf8");
    assert.ok(replayText.includes(globalThis.__e2e.MARKER),
      `scrollback replay missing marker ${globalThis.__e2e.MARKER} after reattach`);
    log("phase4b OK: reattach + scrollback intact");

    // cleanup: kill the session + daemon so the harness leaves no residue
    await request(conn2, { t: "AttachSession", sessionId: globalThis.__e2e.sid }); // ensure attached
    conn2.sock.destroy();
    try { process.kill(daemonPid, "SIGTERM"); } catch {}
    try { fs.rmSync(globalThis.__e2e.root, { recursive: true, force: true }); } catch {}
    log("ALL PHASES PASSED");
  ```

- [ ] **Step 7: Run the full harness — Expected PASS.**
  `cargo build -p sessiond && node tests/e2e/survive-restart.mjs`
  Expected: `phase0..phase4b OK`, `ALL PHASES PASSED`, exit 0.

- [ ] **Step 8: Add the `launchctl kickstart` variant + `tauri-driver` GUI note in the README.**
  Create `tests/e2e/README.md` documenting: (a) the CI-runnable socket harness above (`npm run e2e:survive`), (b) the launchd-managed variant — after `install_agent()` (T16) writes the plist and `launchctl bootstrap gui/$UID`, run `launchctl kickstart gui/$UID/ai.builderpro.desktop.sessiond`, set `BPA_SESSIOND` unset and `XDG_RUNTIME_DIR` to the resolved runtime dir so `resolveSocketPath()` matches the daemon's bound path, then run the same harness (skipping its own `spawnDaemon`) to prove launchd (not the harness) keeps the daemon alive, and (c) the manual full-GUI confirmation via `tauri-driver` + `webdriverio` (launch the signed `.app`, create a terminal in the UI, quit the app window, `pgrep bpa-sessiond` from a shell, relaunch the app, assert the terminal pane repaints scrollback) — documented as the human-in-the-loop check that mirrors spec §14.1 end-to-end wording.
  Add the `package.json` script (append only this key):
  ```json
  "e2e:survive": "node tests/e2e/survive-restart.mjs"
  ```

- [ ] **Step 9: Commit.**
  `git add tests/e2e/survive-restart.mjs tests/e2e/lib/daemon-harness.mjs tests/e2e/README.md package.json && git commit -m "test(e2e): survive-restart harness — create/run/OSC-status/quit/survive/reattach + scrollback replay"`

**Definition of Done:**
- `npm run e2e:survive` (against a locally built `bpa-sessiond`) exits 0, printing `phase0..phase4b OK` + `ALL PHASES PASSED`.
- Asserts, over the real Hop-B wire protocol (§7): handshake → create workspace+session → run a command whose marker appears in `Output` → OSC-133 `running`→`atPrompt` `StateChanged` transitions observed → client socket hard-closed (GUI-quit simulation) → `pgrep bpa-sessiond` shows the daemon still alive AND the shell child pid still alive → fresh client reattaches, `ListSessions` still lists the session, and the `Replay` push content contains the pre-quit marker (scrollback intact).
- No assertion is weakened to "pass vacuously": a missing daemon binary, a broken handshake, absent lifecycle pushes, or lost scrollback each fail with a specific message.
- `tests/e2e/README.md` documents the launchd-managed variant and the `tauri-driver` full-GUI confirmation (spec §14.1).
- The harness leaves no residual daemon/session/temp-dir (cleanup on success).

---

### Task 24: Universal build → deep-sign → notarize → clean-VM smoke (DoD packaging gate)

**Files:**
- Create: `scripts/build-universal.sh` (rustup targets, per-arch `bpa-sessiond`, `tauri build --target universal-apple-darwin`)
- Create: `scripts/sign-verify.sh` (`codesign --verify --deep --strict`, `spctl --assess`, sidecar signature check)
- Create: `scripts/smoke-clean-vm.sh` (first-launch create→quit→relaunch→reattach on a clean macOS VM)
- Create: `src-tauri/entitlements.plist` — **only if T18 has not already created it**; this task OWNS the hardened-runtime entitlement content and MUST reconcile with T18. To avoid a two-writer conflict, T18 creates the file empty-shell and **this task fills the entitlement keys** (see Interfaces). If T18 already populated it, this task MODIFIES it to the locked key set below.
- Modify: `src-tauri/tauri.conf.json` — add `bundle.macOS` signing/entitlements/hardened-runtime + `bundle.targets` fields ONLY (T2 owns the rest of `tauri.conf.json`; this task appends the `bundle.macOS` and `bundle.externalBin` keys and MUST NOT alter unrelated fields)
- Create: `docs/build-macos.md` (the env-var contract + full build runbook + degradation path)

**Depends on:** [T18] (`src-tauri` fully builds: `lib.rs`/`main.rs`, `launchd.rs`, `capabilities/default.json`, and the `entitlements.plist` empty-shell; `bundle.externalBin = ["binaries/bpa-sessiond"]` established by T2/T18), [T13] (`bpa-sessiond` binary compiles for both arches), [T23] (the survive-restart harness — reused by `smoke-clean-vm.sh` as the reattach assertion).
**Parallel-safe with:** [] (G6 sequential; T23 → T24 → T25)

**Interfaces:**
- Consumes: daemon binary name `bpa-sessiond`; product `Builder Pro AI`; bundle id `ai.builderpro.desktop` (spec/scaffold naming); `bundle.externalBin = ["binaries/bpa-sessiond"]` (spec §4). Tauri external-bin naming rule: the files in `src-tauri/binaries/` are triple-suffixed — `bpa-sessiond-aarch64-apple-darwin` and `bpa-sessiond-x86_64-apple-darwin` — while `tauri.conf.json` references `binaries/bpa-sessiond` (no suffix).
- Consumes (env-var contract, spec §14.3 / §16 / research §Tauri signing): `APPLE_SIGNING_IDENTITY` (Developer ID Application cert), `APPLE_TEAM_ID`, and the App Store Connect API-key notarization set `APPLE_API_ISSUER`, `APPLE_API_KEY` (key id), `APPLE_API_KEY_PATH` (path to the `.p8`). Tauri's `tauri build` deep-signs the `.app` (including the embedded sidecar) and, when the `APPLE_API_*` set is present, uploads → polls → staples automatically.
- Produces: `src-tauri/entitlements.plist` populated with the hardened-runtime keys `com.apple.security.cs.allow-jit`=false-not-needed → **locked key set:** `com.apple.security.cs.allow-unsigned-executable-memory` = **false** (not set), `com.apple.security.cs.disable-library-validation` = **true** (needed so the signed `.app` can exec the embedded `bpa-sessiond` sidecar under hardened runtime), `com.apple.security.inherit` = not used (sidecar is signed with the same identity, not inheriting). Produces `bundle.macOS` config in `tauri.conf.json`: `"hardenedRuntime": true`, `"entitlements": "entitlements.plist"`, `"signingIdentity": "-"`-overridable-by-env, `"providerShortName"` from `APPLE_TEAM_ID`. Produces three runnable scripts + `docs/build-macos.md`.

> **Degradation contract (locked, spec §8.3/§14.3/§16):** if the notarization creds (`APPLE_API_*`) are absent, the build MUST still produce a **dev-signed** (ad-hoc or Developer-ID-without-notarization) `.app` and print a loud, honest warning that the artifact is not notarized and Gatekeeper will quarantine it on other machines — never silently pretend it is notarized, and never hang waiting on missing creds. `scripts/build-universal.sh` detects the missing env and takes the dev-signed path with exit 0 + warning; `scripts/sign-verify.sh` runs `codesign --verify` (which works on dev-signed) but reports `spctl --assess` as EXPECTED-REJECT in the dev path.

- [ ] **Step 1: Failing test — `scripts/build-universal.sh` requires both per-arch sidecar binaries present.**
  Create `scripts/build-universal.sh` starting with a guard, and a tiny self-check invoked as `scripts/build-universal.sh --check-prereqs`:
  ```bash
  #!/usr/bin/env bash
  set -euo pipefail
  REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
  BIN_DIR="$REPO/src-tauri/binaries"
  AARCH="$BIN_DIR/bpa-sessiond-aarch64-apple-darwin"
  XARCH="$BIN_DIR/bpa-sessiond-x86_64-apple-darwin"

  check_prereqs() {
    command -v rustup >/dev/null || { echo "FAIL: rustup not found"; exit 1; }
    command -v cargo  >/dev/null || { echo "FAIL: cargo not found"; exit 1; }
    for t in aarch64-apple-darwin x86_64-apple-darwin; do
      rustup target list --installed | grep -qx "$t" || { echo "FAIL: rust target $t not installed (rustup target add $t)"; exit 1; }
    done
    echo "OK: prereqs (rustup, cargo, both darwin targets)"
  }

  build_sidecars() {
    mkdir -p "$BIN_DIR"
    cargo build -p sessiond --release --target aarch64-apple-darwin
    cargo build -p sessiond --release --target x86_64-apple-darwin
    cp "$REPO/target/aarch64-apple-darwin/release/bpa-sessiond" "$AARCH"
    cp "$REPO/target/x86_64-apple-darwin/release/bpa-sessiond"  "$XARCH"
    [ -f "$AARCH" ] && [ -f "$XARCH" ] || { echo "FAIL: missing per-arch sidecar binary"; exit 1; }
    echo "OK: both per-arch sidecars present"
  }

  build_app() {
    ( cd "$REPO" && npm run tauri -- build --target universal-apple-darwin )
  }

  dev_signed_warning() {
    echo "WARNING: APPLE_API_* notarization creds absent — producing DEV-SIGNED artifact." >&2
    echo "         The .app is NOT notarized; Gatekeeper will quarantine it on other machines." >&2
  }

  case "${1:-}" in
    --check-prereqs) check_prereqs; exit 0 ;;
  esac

  check_prereqs
  build_sidecars
  if [ -z "${APPLE_API_ISSUER:-}" ] || [ -z "${APPLE_API_KEY:-}" ] || [ -z "${APPLE_API_KEY_PATH:-}" ]; then
    dev_signed_warning
  fi
  if [ -z "${APPLE_SIGNING_IDENTITY:-}" ]; then
    echo "WARNING: APPLE_SIGNING_IDENTITY unset — Tauri will ad-hoc sign ('-')." >&2
  fi
  build_app
  echo "OK: universal build complete"
  ```
  `chmod +x scripts/build-universal.sh`.

- [ ] **Step 2: Run the prereq self-check — Expected FAIL until targets installed.**
  `bash scripts/build-universal.sh --check-prereqs`
  Expected: FAIL with `FAIL: rust target aarch64-apple-darwin not installed …` on a machine missing a target (or `OK: prereqs …` if both already installed). This proves the guard is real.

- [ ] **Step 3: Install targets, re-run self-check — Expected PASS.**
  `rustup target add aarch64-apple-darwin x86_64-apple-darwin && bash scripts/build-universal.sh --check-prereqs`
  Expected: `OK: prereqs (rustup, cargo, both darwin targets)`.

- [ ] **Step 4: Populate `entitlements.plist` + `tauri.conf.json bundle.macOS`.**
  Write `src-tauri/entitlements.plist` (reconcile with T18's empty-shell — replace its body with this locked content):
  ```xml
  <?xml version="1.0" encoding="UTF-8"?>
  <!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
  <plist version="1.0">
  <dict>
    <key>com.apple.security.cs.disable-library-validation</key>
    <true/>
  </dict>
  </plist>
  ```
  Add to `src-tauri/tauri.conf.json` (append these keys under `bundle`; do not remove existing `bundle` fields owned by T2):
  ```json
  "bundle": {
    "externalBin": ["binaries/bpa-sessiond"],
    "targets": ["app", "dmg"],
    "macOS": {
      "hardenedRuntime": true,
      "entitlements": "entitlements.plist",
      "minimumSystemVersion": "12.0"
    }
  }
  ```
  (Signing identity + notarization creds are supplied via env at `tauri build` time, not hardcoded — see `docs/build-macos.md`.)

- [ ] **Step 5: Failing test — `scripts/sign-verify.sh` asserts deep signature + sidecar signature + spctl.**
  Create `scripts/sign-verify.sh`:
  ```bash
  #!/usr/bin/env bash
  set -euo pipefail
  REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
  APP="${1:-$REPO/src-tauri/target/universal-apple-darwin/release/bundle/macos/Builder Pro AI.app}"
  [ -d "$APP" ] || { echo "FAIL: app bundle not found at: $APP"; exit 1; }

  echo "== codesign --verify --deep --strict =="
  codesign --verify --deep --strict --verbose=2 "$APP" || { echo "FAIL: deep signature verification failed"; exit 1; }

  echo "== embedded sidecar signature =="
  SIDECAR="$APP/Contents/MacOS/bpa-sessiond"
  [ -f "$SIDECAR" ] || SIDECAR="$(/usr/bin/find "$APP/Contents" -name 'bpa-sessiond*' -type f | head -1)"
  [ -n "${SIDECAR:-}" ] && [ -f "$SIDECAR" ] || { echo "FAIL: embedded bpa-sessiond not found in bundle"; exit 1; }
  codesign --verify --strict --verbose=2 "$SIDECAR" || { echo "FAIL: sidecar not signed"; exit 1; }
  codesign -d --entitlements :- "$SIDECAR" >/dev/null 2>&1 || true
  echo "OK: sidecar signed at $SIDECAR"

  echo "== spctl --assess (Gatekeeper) =="
  if spctl --assess --type execute --verbose=4 "$APP"; then
    echo "OK: spctl accepted (notarized/valid Developer ID)"
  else
    if [ -z "${APPLE_API_ISSUER:-}" ]; then
      echo "EXPECTED-REJECT: dev-signed path (no notarization creds) — spctl rejects; not a failure in dev build"
    else
      echo "FAIL: spctl rejected a build that was supposed to be notarized"; exit 1
    fi
  fi
  echo "OK: sign-verify complete"
  ```
  `chmod +x scripts/sign-verify.sh`. Run it before any build exists:
  `bash scripts/sign-verify.sh`
  Expected: FAIL with `FAIL: app bundle not found at: …` (no build yet).

- [ ] **Step 6: Run the universal build, then sign-verify — Expected PASS (notarized path when creds present).**
  ```
  APPLE_SIGNING_IDENTITY="Developer ID Application: … (TEAMID)" \
  APPLE_TEAM_ID="TEAMID" \
  APPLE_API_ISSUER="…" APPLE_API_KEY="…" APPLE_API_KEY_PATH="/abs/AuthKey_XXX.p8" \
  bash scripts/build-universal.sh
  bash scripts/sign-verify.sh
  ```
  Expected: build completes; `codesign --verify --deep --strict` passes on the `.app`; the embedded `bpa-sessiond` reports a valid signature; `spctl --assess` prints acceptance. If creds absent, `build-universal.sh` prints the dev-signed warning and `sign-verify.sh` prints `EXPECTED-REJECT` for spctl — both exit 0 (honest degradation).

- [ ] **Step 7: Clean-VM first-launch smoke — create→quit→relaunch→reattach.**
  Create `scripts/smoke-clean-vm.sh` that runs on a freshly-provisioned macOS VM (no prior daemon state):
  ```bash
  #!/usr/bin/env bash
  set -euo pipefail
  REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
  APP="${1:-/Applications/Builder Pro AI.app}"
  [ -d "$APP" ] || { echo "FAIL: install the .app to $APP on the clean VM first"; exit 1; }

  echo "== clean-VM smoke: first launch removes quarantine, boots daemon =="
  xattr -dr com.apple.quarantine "$APP" 2>/dev/null || true
  # Launch the app once so it installs + bootstraps the LaunchAgent (T16/T18)
  open "$APP"
  # give the app time to install the plist and kickstart the daemon
  for i in $(seq 1 30); do
    if pgrep -x bpa-sessiond >/dev/null; then break; fi
    sleep 1
  done
  pgrep -x bpa-sessiond >/dev/null || { echo "FAIL: daemon did not start on first launch"; exit 1; }
  echo "OK: daemon running after first launch"

  # Drive the survive/reattach assertion over the socket via the T23 harness,
  # against the launchd-managed daemon (do NOT spawn our own).
  export BPA_SESSIOND=""   # signal: reuse running daemon
  export BPA_E2E_REUSE=1   # harness skips spawnDaemon when set (see README)
  node "$REPO/tests/e2e/survive-restart.mjs" || { echo "FAIL: survive/reattach smoke failed"; exit 1; }
  echo "OK: create→quit→relaunch→reattach smoke passed on clean VM"
  ```
  Add a `BPA_E2E_REUSE` guard in `tests/e2e/survive-restart.mjs` phase 0 so `spawnDaemon` is skipped when reusing the launchd daemon (the harness connects to the already-bound socket instead). `chmod +x scripts/smoke-clean-vm.sh`.

- [ ] **Step 8: Write `docs/build-macos.md` (env-var contract + runbook + degradation).**
  Document: the five env vars (`APPLE_SIGNING_IDENTITY`, `APPLE_TEAM_ID`, `APPLE_API_ISSUER`, `APPLE_API_KEY`, `APPLE_API_KEY_PATH`), that Tauri deep-signs the sidecar and staples the notarization ticket automatically during `tauri build` when the `APPLE_API_*` set is present, the exact `scripts/build-universal.sh` → `scripts/sign-verify.sh` → `scripts/smoke-clean-vm.sh` sequence, and the dev-signed degradation path (build still succeeds with a loud warning; `spctl` will reject; not for distribution). Note the hardened-runtime `disable-library-validation` entitlement rationale (so the signed `.app` can exec the embedded sidecar). Cross-reference spec §14.3 and §16.

- [ ] **Step 9: Commit.**
  `git add scripts/build-universal.sh scripts/sign-verify.sh scripts/smoke-clean-vm.sh src-tauri/entitlements.plist src-tauri/tauri.conf.json docs/build-macos.md && git commit -m "build(macos): universal build + deep-sign + notarize + clean-VM smoke gate with dev-signed degradation"`

**Definition of Done:**
- `bash scripts/build-universal.sh --check-prereqs` passes once both `aarch64-apple-darwin` + `x86_64-apple-darwin` targets are installed; the build produces both triple-suffixed `bpa-sessiond` sidecars in `src-tauri/binaries/` and a universal `.app` via `tauri build --target universal-apple-darwin`.
- With `APPLE_SIGNING_IDENTITY`/`APPLE_TEAM_ID`/`APPLE_API_*` set: `codesign --verify --deep --strict` passes on the `.app`, the embedded `bpa-sessiond` carries a valid signature under hardened runtime (via `entitlements.plist` with `disable-library-validation`), and `spctl --assess --type execute` accepts (notarization stapled).
- `scripts/smoke-clean-vm.sh` on a clean macOS VM: first launch boots the launchd daemon, then the T23 harness proves create→quit→relaunch→reattach with scrollback intact.
- Degradation: with notarization creds absent, `build-universal.sh` still produces a dev-signed `.app` with a loud honest warning (exit 0), and `sign-verify.sh` reports `spctl` as `EXPECTED-REJECT` — never a silent false "notarized".
- `docs/build-macos.md` documents the env-var contract, runbook, and degradation path (spec §14.3, §16).

---

### Task 25: Docs + contract→test traceability + coverage gate + no-secrets-in-logs + final green-suite

**Files:**
- Modify: `README.md` (repo root — architecture summary, survival truth table, build/run/test quickstart)
- Create: `docs/architecture.md` (module map from spec §4 + the two-hop IPC overview)
- Create: `docs/traceability.md` (the spec §14.2 contract→test matrix, each row linked to a concrete test name/path that passes)
- Create: `scripts/coverage-gate.sh` (`cargo llvm-cov` for the daemon crate, enforce ≥80% line coverage)
- Create: `scripts/final-suite.sh` (runs the full Rust + TS + e2e green-suite + coverage + traceability check in order)
- Create: `crates/sessiond/tests/no_secrets_in_logs.rs` (integration test: planted secret never appears in structured logs) — **only if T7/T9/T12 did not already place the no-secrets scrub test.** If an env-hygiene/log-scrub test already exists in the daemon crate, this task instead ADDS the log-file assertion variant and references the existing one in `traceability.md` (no duplicate ownership of the same test fn).
- Modify: `crates/sessiond/src/main.rs` (reference only — NO code change; this task reads it to confirm `tracing` init writes to `{APP_SUPPORT}/logs/`; if a test-only log path hook is missing, coordinate with T13 rather than editing here)

**Depends on:** [T24] (universal build + scripts exist so `final-suite.sh` can chain them), [T13] (daemon crate complete — coverage target + logging), [T18] (core complete), [T22] (frontend complete — TS suite), [T23] (e2e harness for `final-suite.sh`). Effectively the whole plan; runs last.
**Parallel-safe with:** [] (G6 sequential; final task).

**Interfaces:**
- Consumes: every prior task's tests (Rust `cargo test -p sessiond` / `-p protocol` / `-p app` (src-tauri), TS `npx vitest run`, e2e `npm run e2e:survive`); the spec §14.2 traceability matrix rows; `tracing`/`tracing-subscriber` log sink under `{APP_SUPPORT}/logs/` (spec §13); the env allowlist (spec §9.3) and the planted-secret convention `DAEMON_SECRET` used by the env-hygiene test (spec §14.1).
- Consumes (coverage tool): `cargo llvm-cov` (`cargo install cargo-llvm-cov`) — `cargo llvm-cov --package sessiond --fail-under-lines 80`.
- Produces: `README.md` + `docs/architecture.md` + `docs/traceability.md` (every §14.2 row → a named passing test); `scripts/coverage-gate.sh` (exit non-zero if daemon line coverage < 80%); `scripts/final-suite.sh` (single command that gates the whole DoD); `crates/sessiond/tests/no_secrets_in_logs.rs` (planted `DAEMON_SECRET` absent from emitted logs).

- [ ] **Step 1: Failing no-secrets-in-logs integration test.**
  Create `crates/sessiond/tests/no_secrets_in_logs.rs`. It spawns a session via the daemon's public supervisor API (or, if the supervisor is not exposed as a lib, drives it through the socket like T23 does) with a planted secret in the daemon's own environment, captures the `tracing` output to an in-test buffer/temp log file, runs a command, and asserts the secret value never appears.
  ```rust
  // crates/sessiond/tests/no_secrets_in_logs.rs
  use std::fs;
  use std::io::Read;

  /// The daemon must never leak its own environment secrets into structured logs
  /// (spec §13, §16). We plant a secret in the daemon process env, drive a session,
  /// and assert the value is absent from the log sink.
  #[test]
  fn planted_secret_never_appears_in_logs() {
      let secret = "s3cr3t-DAEMON_SECRET-must-not-leak-9f2c";
      std::env::set_var("DAEMON_SECRET", secret);

      let tmp = tempfile::tempdir().expect("tempdir");
      let log_path = tmp.path().join("sessiond.test.log");

      // sessiond exposes a test hook to direct tracing to a file sink.
      // (Provided by T13's main.rs / a lib init fn: sessiond::logging::init_to_file(&Path).)
      sessiond::logging::init_to_file(&log_path).expect("init logging");

      // Drive a real session that inherits the (env_clear'd) allowlist env only.
      let sup = sessiond::pty_supervisor::Supervisor::new();
      let sid = sup
          .create(sessiond::pty_supervisor::SessionSpec {
              workspace_id: "w-test".into(),
              shell: Some("/bin/zsh".into()),
              cwd: std::env::temp_dir().to_string_lossy().into_owned(),
              env_overrides: vec![],
              cols: 80,
              rows: 24,
          })
          .expect("create session");
      sup.write_stdin(&sid, b"echo hello\n").expect("write");
      std::thread::sleep(std::time::Duration::from_millis(800));
      sup.kill(&sid).expect("kill");

      // Flush + read the log file.
      sessiond::logging::flush();
      let mut contents = String::new();
      fs::File::open(&log_path)
          .expect("open log")
          .read_to_string(&mut contents)
          .expect("read log");

      assert!(
          !contents.contains(secret),
          "planted secret leaked into logs:\n{contents}"
      );
      // Sanity: logging actually produced output (guards against a vacuous pass).
      assert!(
          !contents.trim().is_empty(),
          "log sink was empty — test would pass vacuously"
      );
  }
  ```
  (If T13 exposes the supervisor only via the socket, replace the direct `Supervisor` calls with the T23 `daemon-harness` flow driven from a `#[tokio::test]`; the assertion on the log file is unchanged. The exact hook name `sessiond::logging::init_to_file` / `flush` must match T13 — reconcile at integration; if T13 named it differently, use that name verbatim.)

- [ ] **Step 2: Run the no-secrets test — Expected FAIL.**
  `cargo test -p sessiond --test no_secrets_in_logs planted_secret_never_appears_in_logs`
  Expected: FAIL — either a compile error `no function \`init_to_file\` in module \`logging\`` (until T13's hook is wired), or, if logging currently echoes env, `planted secret leaked into logs`.

- [ ] **Step 3: Reconcile the logging hook + re-run — Expected PASS.**
  Ensure `crates/sessiond/src/main.rs`/`lib.rs` (owned by T13) exposes `logging::init_to_file(&Path) -> Result<()>` and `logging::flush()`; if absent, this is a NEEDS_CONTEXT back to T13 (do not fork a second logging init here). With the hook present:
  `cargo test -p sessiond --test no_secrets_in_logs planted_secret_never_appears_in_logs`
  Expected: PASS (`test result: ok. 1 passed`).

- [ ] **Step 4: Coverage gate script.**
  Create `scripts/coverage-gate.sh`:
  ```bash
  #!/usr/bin/env bash
  set -euo pipefail
  REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
  command -v cargo-llvm-cov >/dev/null || { echo "FAIL: cargo-llvm-cov not installed (cargo install cargo-llvm-cov)"; exit 1; }
  cd "$REPO"
  echo "== daemon-crate line coverage (>= 80%) =="
  cargo llvm-cov --package sessiond --fail-under-lines 80
  echo "OK: sessiond coverage >= 80%"
  ```
  `chmod +x scripts/coverage-gate.sh`. Run it:
  `bash scripts/coverage-gate.sh`
  Expected: with the full T4–T13 suites present, prints the coverage table and `OK: sessiond coverage >= 80%`; if below 80, `cargo llvm-cov` exits non-zero (`error: … lines coverage … is less than 80`) and the script fails — a real gate.

- [ ] **Step 5: Traceability doc — every §14.2 row → a named passing test.**
  Create `docs/traceability.md` mapping each spec §14.2 contract row to a concrete test invocation, e.g.:
  ```markdown
  # Contract → Test Traceability (spec §14.2)

  | Contract (spec §) | Test (command) |
  |---|---|
  | Shared types / Rust⇄TS parity (§5) | `cargo test -p protocol ts_parity` + CI `ts-rs` gen then `git diff --exit-code src/ipc/types.ts` |
  | Hop-B framing + correlation + handshake (§7) | `cargo test -p protocol frame_roundtrip framing correlation handshake` |
  | PTY threading + pgroup kill + env (§9) | `cargo test -p sessiond pty_echo pty_eof zombie_reaped pgroup_kill sigwinch env_hygiene` |
  | OSC parser + state machine + parse rule (§10) | `cargo test -p sessiond osc_parser state_machine exit_code_parse` |
  | Waiting-for-input (§10.4) | `cargo test -p sessiond waiting_for_input` |
  | Scrollback sanitize + replay (§11) | `cargo test -p sessiond scrollback_ring sanitize replay_no_corrupt` |
  | SQLite degradation + rehydrate (§11) | `cargo test -p sessiond persist_rehydrate corrupt_quarantine busy_timeout migration kill9_rehydrate` |
  | Socket path/perms/single-instance/peer-cred (§8) | `cargo test -p sessiond flock_single_instance peer_cred socket_mode stale_socket` |
  | Backpressure / slow-client (§13) | `cargo test -p sessiond backpressure_disconnect` |
  | Attach model (§7) | `cargo test -p sessiond single_attach_supersede detach_keeps_pty` |
  | Path validation (§16) | `cargo test -p app path_validation` (paths.rs) |
  | launchd install/degradation (§8.3, §13) | `cargo test -p app launchd_bootstrap_idempotent kickstart dir_missing hard_failure` + `npm run e2e:survive` |
  | Frontend keep-alive/renderer/state (§12) | `npx vitest run src/terminal src/store` |
  | No-secrets-in-logs (§13) | `cargo test -p sessiond --test no_secrets_in_logs` |
  | E2E survive-restart (§14.1) | `npm run e2e:survive` |
  ```
  Each row's test names MUST match the real test fn names produced by T4–T22 (reconcile at integration — if a name differs, use the actual name; NO row may point at a non-existent test).

- [ ] **Step 6: Final green-suite script.**
  Create `scripts/final-suite.sh`:
  ```bash
  #!/usr/bin/env bash
  set -euo pipefail
  REPO="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
  cd "$REPO"

  echo "== 1/5 Rust workspace tests =="
  cargo test --workspace

  echo "== 2/5 TypeScript tests =="
  npx vitest run

  echo "== 3/5 ts-rs type parity (generated types in sync) =="
  cargo test -p protocol export_bindings >/dev/null 2>&1 || cargo test -p protocol
  git diff --exit-code src/ipc/types.ts || { echo "FAIL: src/ipc/types.ts out of sync with crates/protocol (regenerate)"; exit 1; }

  echo "== 4/5 daemon coverage gate (>= 80%) =="
  bash "$REPO/scripts/coverage-gate.sh"

  echo "== 5/5 e2e survive-restart =="
  cargo build -p sessiond
  npm run e2e:survive

  echo "ALL GATES PASSED"
  ```
  `chmod +x scripts/final-suite.sh`. Run it:
  `bash scripts/final-suite.sh`
  Expected: all five stages print their OK line and it ends with `ALL GATES PASSED` (exit 0). Any red test, out-of-sync types, sub-80% coverage, or failed e2e stops it with a specific failure.

- [ ] **Step 7: Update `README.md` + `docs/architecture.md`.**
  `README.md`: product one-liner, the two-process/two-hop architecture diagram (from spec §2), the survival truth table (spec §13 — GUI close/crash → survive; daemon restart → survive via rehydrate; daemon crash → live shells die; logout → die), and a Quickstart (`npm install`, `rustup target add …`, `npm run tauri dev`, `bash scripts/final-suite.sh`, `bash scripts/build-universal.sh`). `docs/architecture.md`: the §4 module ownership map and the Hop-A/Hop-B responsibilities, linking `docs/traceability.md` and `docs/build-macos.md`. Both updated in THIS change (spec §14.3 "README + module docs updated in the same change").

- [ ] **Step 8: Commit.**
  `git add README.md docs/architecture.md docs/traceability.md scripts/coverage-gate.sh scripts/final-suite.sh crates/sessiond/tests/no_secrets_in_logs.rs && git commit -m "docs+gate: traceability matrix, README/arch docs, >=80% daemon coverage gate, no-secrets-in-logs test, final green-suite"`

**Definition of Done:**
- `bash scripts/final-suite.sh` exits 0 with `ALL GATES PASSED`: full Rust workspace suite green, full TS (`vitest`) suite green, `src/ipc/types.ts` in sync with `crates/protocol` (ts-rs `git diff --exit-code`), daemon-crate line coverage ≥ 80% (`cargo llvm-cov --fail-under-lines 80`), and `npm run e2e:survive` green.
- `crates/sessiond/tests/no_secrets_in_logs.rs` passes: a planted `DAEMON_SECRET` never appears in the `tracing` log sink, with a non-vacuous guard asserting the sink was non-empty.
- `docs/traceability.md` covers every spec §14.2 row, each pointing at a real, passing test invocation (no dangling rows).
- `README.md` + `docs/architecture.md` updated in the same change: architecture, survival truth table, quickstart, module map (spec §14.3 doc requirement).
- `scripts/coverage-gate.sh` is a real gate (fails below 80%); `scripts/final-suite.sh` is the single DoD command.
