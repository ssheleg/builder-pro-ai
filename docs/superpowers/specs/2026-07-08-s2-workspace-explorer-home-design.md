# S2 — Workspace multi-root + File Explorer + Attention-first Home (design)

**Status:** approved-pending-user-review (brainstorm 2026-07-08)
**Seed:** overview §3 roadmap row S2; vision laws (research/2026-07-06-product-vision-v2.md §"attention tax", §"home screen"); owner session 2026-07-08.
**Context7 verification (2026-07-08):** `notify-debouncer-full::new_debouncer(timeout, tick_rate, handler) -> Debouncer<RecommendedWatcher, RecommendedCache>` + `watcher().watch(path, RecursiveMode::Recursive)` confirmed current; xterm.js `Terminal.registerLinkProvider(ILinkProvider)` (`provideLinks(bufferLineNumber, cb)` → `ILink { range, text, activate(event, text) }`) and OSC-8 via `Terminal.options.linkHandler: ILinkHandler` confirmed current. `ignore` (ripgrep walker), `trash`, `opener` — exact versions pinned at plan time.

## 1. Goal

One slice that delivers the owner's core daily loop on today's real data:

> Зашёл → за секунды вижу все терминалы во всех workspace: где нужен я (амбер), что работает,
> что завершилось → «Пройти» одним кликом в терминал, ждущий ввода → файлы, которые терминалы
> создают, смотрю кликом по ссылке прямо из терминала — в правой файловой панели.

Three product laws this design serves (vision doc): attention tax is the enemy (never poll);
context <30 s across all projects on open; autonomy default — where a human is needed is visible
instantly.

**DoD (roadmap):** create/open a multi-repo workspace ≤3 clicks; file tree reflects an external
`touch` <1 s; explorer responsive at 10k files. **Metric:** time-to-first-terminal in a fresh
workspace.

## 2. Owner decisions (locked)

| # | Decision | Choice |
|---|---|---|
| D1 | File open scope | Explorer + **read-only preview**. No in-app editing (command center, not a code editor). |
| D2 | Multi-root model | **Equal ordered roots** (`roots: Vec<String>`); first root = default terminal cwd; `root_path` stays as compat mirror = `roots[0]`. |
| D3 | Tree visibility | **Respect `.gitignore`** (crate `ignore`), always hide `.git`; "show ignored" toggle. |
| D4 | Architecture | **Approach A:** file I/O + watch live in the Tauri core (foreground, GUI-lifetime). Daemon keeps owning the Workspace data model only. No file I/O over Hop-B. |
| D5 | Files panel | **Right rail** (file-manager style): tree + preview, collapsible. |
| D6 | Home | **Attention-first queue**, not stats: «нужен ты» (amber) pinned top with [Пройти →]; then running; then recently exited (✓/✗ exit codes). Stats = thin strip. |
| D7 | Progress v1 | «Что закрыто/не закрыто» = **command history** (`command_events`, already persisted since Pv2) — per-session strip of recent commands with exit codes. Task-level progress is S3, not faked here. |
| D8 | Delete | Always **to Trash** (crate `trash`) — reversible, honest. |
| D9 | Terminal file links | Click a path in terminal output → opens in the right-rail preview. OSC-8 hyperlinks + regex path detection; relative paths resolve against the session's live cwd (OSC-7 tracked). |

## 3. Data model + migration (daemon-owned)

### 3.1 Protocol `Workspace` (additive)
```rust
pub struct Workspace {
    pub id: WorkspaceId,
    pub name: String,
    /// Compat mirror, always == roots[0].
    pub root_path: String,
    /// Ordered, equal roots; canonical absolute paths; len >= 1.
    pub roots: Vec<String>,
}
```
ts-rs regenerates `types.ts` (parity gate). All existing `root_path` consumers keep working.

### 3.2 Persistence — schema v3
New table:
```sql
CREATE TABLE workspace_root (
  workspace_id TEXT NOT NULL REFERENCES workspace(id),
  ord INTEGER NOT NULL,
  path TEXT NOT NULL,
  PRIMARY KEY (workspace_id, ord)
);
```
Migration v2→v3 (fail-closed, forward-only, in one transaction — Pv2 policy): for every
existing workspace insert its `root_path` as `ord = 0`. `workspace.root_path` column stays,
always mirrors `ord = 0`. `list_workspaces` joins + assembles `roots` ordered by `ord`.
`Db::delete_session` untouched; workspace deletion is still out of scope (no such verb today).

### 3.3 Wire additions (additive batch, Pv2.1-style — no break)
```rust
Request::AddWorkspaceRoot    { workspace_id: WorkspaceId, path: String }, // validate_dir → append ord=max+1
Request::RemoveWorkspaceRoot { workspace_id: WorkspaceId, path: String }, // reject removing the LAST root
Request::GetCommandEvents    { session_id: SessionId, limit: u32 },       // newest-first
Response::CommandEvents(Vec<CommandEvent>),
Push::WorkspaceUpdated(Workspace),        // emitted after Add/RemoveWorkspaceRoot
```
```rust
pub struct CommandEvent {  // mirrors the command_events row (Pv2 §7); ts-rs exported
    pub session_id: SessionId,
    pub seq: i64,
    pub ts: i64,            // unix seconds
    pub kind: String,       // "started" | "finished" — the exact literals the Pv2 writer persists (pty_supervisor.rs)
    pub exit_code: Option<u8>,
    pub origin: String,
}
```
Broker maps `Push::WorkspaceUpdated` → global event `workspace://updated` (payload `Workspace`);
frontend upserts. `CreateWorkspace` stays single-root; extra roots via AddWorkspaceRoot
(create ≤3 clicks: pickFolder→create, then per root: "+ Add root"→pickFolder).
Both new workspace verbs run `bpa_paths::validate_dir` (daemon = security authority, §16).
`GetCommandEvents` is the first consumer of `command_events` (closes the "no UI" note).

## 4. Core file-ops (`src-tauri/src/fs_explorer.rs`)

### 4.1 Path safety (shared crate)
New `bpa_paths::validate_path_within(root: &Path, candidate: &Path) -> Result<PathBuf, PathError>`:
canonicalize both; the canonical candidate must `starts_with` the canonical root (kills `..`,
symlink escapes). For not-yet-existing targets (create/rename destination): canonicalize the
parent, validate parent-within-root, then append the (single, separator-free) final component.
Every core file op validates against ONE of the active workspace's roots first. Defense in
depth: roots themselves were daemon-validated at Add time.

### 4.2 Commands (thin `#[tauri::command]` wrappers over unit-testable inner fns)
```ts
listDir(root: string, rel: string): Promise<FsEntry[]>          // ONE level, lazy
readFilePreview(root: string, rel: string): Promise<FilePreview> // cap 1 MiB
createFile(root, relDir, name) / createDir(root, relDir, name)
renameEntry(root, rel, newName) / moveEntry(root, relFrom, relDirTo)
deleteEntry(root, rel)                                           // → Trash
revealInFinder(root, rel) / openExternal(root, rel)              // crate `opener`
```
```ts
interface FsEntry   { name: string; relPath: string; isDir: boolean; size: number; isIgnored: boolean }
type FilePreview =
  | { kind: "text"; content: string; truncated: boolean; size: number }
  | { kind: "binary"; size: number }
  | { kind: "tooLarge"; size: number }
```
`listDir` uses the `ignore` walker scoped to the level (git semantics, nested `.gitignore`);
`.git` never listed. Binary detection: invalid-UTF8 or NUL in the first 8 KiB.
Typed error (camelCase serde, `CommandError` pattern):
`FsError = NotFound | PermissionDenied | OutsideRoot | TooLarge | Io { message }`.

## 5. Live watch (core, GUI-lifetime)

- `notify-debouncer-full`: `new_debouncer(Duration::from_millis(250), None, handler)`;
  `watcher().watch(root, RecursiveMode::Recursive)` per root of the ACTIVE workspace
  (macOS → FSEvents). Start on workspace activation, stop on switch/unmount; nothing watched
  while the app is closed (D4).
- Debounced batch → filter through the same `ignore` matcher (ignored paths dropped unless the
  "show ignored" toggle is on — toggle state passed from frontend on watch start) → emit
  `fs://changed { root: string, changedRelPaths: string[] }` (deduped, capped at 500 paths per
  event; overflow ⇒ `changedRelPaths: ["*"]` meaning "refresh everything expanded").
- Frontend invalidates ONLY affected expanded dirs (point refresh — 10k stays responsive).
- Watcher error (root deleted, fd limits) → `fs://watch-error { root, reason }` → UI shows
  "live-обновления на паузе — обновить" affordance (honest); auto-retry on next activation.
- `touch` <1 s: FSEvents latency + 250 ms debounce ≪ 1 s.

## 6. Frontend

### 6.1 Layout (three rails)
```
┌ DaemonBanner ────────────────────────────────────────────────────────────┐
├──────────┬──────────────────────────────────────────────┬────────────────┤
│ ⌂ Home   │ Home: attention queue   | Workspace: чипы    │ FILES (right,  │
│ • ws-... │ (см. 6.2)               | + terminal tabs    │ collapsible):  │
│ (nav     │                         | + command strip    │ FileTree       │
│  only)   │                         | (см. 6.3)          │ + FilePreview  │
└──────────┴──────────────────────────────────────────────┴────────────────┘
```
Left rail: `⌂ Home` on top, then workspace list (existing `WorkspaceSidebar`, slimmed to pure
navigation). Right rail: `FileTree` + `FilePreview` stacked; collapsible; hidden on Home.

### 6.2 Home — attention-first (`src/components/HomeView.tsx`)
Pure frontend composition over the existing store (sessions + workspaces + lifecycle events).
Order: ① amber block «нужен ты» — sessions with `waitingForInput`, each row = agent-row atom
(status dot / `workspace/title` / lifecycle text) + **[Пройти →]** button → navigate to that
workspace, activate that terminal tab, focus the terminal (ready to type); ② «работают» —
running sessions (row click = same navigation); ③ «завершились недавно» — exited sessions,
✓/✗ by exit code. Thin stats strip on top: N workspaces · M live · K waiting. Clicking a
workspace group header opens that workspace. Empty state per design-system (one dim sentence +
one action).

### 6.3 Workspace view
Stat chips row (live/waiting/exited counts, roots count; click expands a small detail panel) →
existing `TerminalTabs` + `TerminalPane`. Per-session **command strip**: last ~10
`command_events` (via `getCommandEvents(sessionId, limit)`) rendered as ✓/✗ chips with the
command's exit code; refreshed on `session://state-changed`/`exited`. New terminal cwd = root
of the tree-selected node, else `roots[0]` (time-to-first-terminal metric).

### 6.4 Files right rail
`FileTree.tsx`: lazy per-level `listDir`, virtualized rows (10k DoD), dirs-first sort, ignored
entries dimmed behind the toggle, context menu (new file/folder, rename, delete→Trash with
confirm, reveal in Finder, open external), roots as top-level nodes, «+ Add root» → pickFolder
→ `addWorkspaceRoot`. `FilePreview.tsx`: read-only mono text; binary/tooLarge/error → honest
placeholder; no syntax highlighting (YAGNI v1).

### 6.5 Terminal file links (D9)
In `terminal-manager`: register an `ILinkProvider` per terminal — regex-detect path-like tokens
(`src/x.ts`, `./a/b.rs:42`, `/abs/path`, `~/…`; strip `:line[:col]` suffix) in the line;
`activate` resolves relative paths against the session's live cwd (OSC-7 tracked in
`SessionMeta.cwd`), validates the result is inside one of the active workspace's roots (core
command does the authoritative check), then opens it in the right-rail preview (expanding the
rail if collapsed). Paths outside roots or nonexistent → not linkified (probe on hover is too
chatty: v1 linkifies by pattern, validates on click; a failed click shows a quiet toast).
OSC-8 hyperlinks: `options.linkHandler` with `allowNonHttpProtocols: true`, `file://` URLs go
through the same validate-then-preview path; http(s) → `openExternal`.

### 6.6 Store / IPC additions
`fs`-slice: `expanded: Set<(root, rel)>`, `treeCache: Map<(root, rel), FsEntry[]>`,
`selectedFile: { root, rel } | null`, `showIgnored: boolean`, `filesRailOpen: boolean`,
invalidation on `fs://changed`, `watchPaused: boolean` on `fs://watch-error`.
Navigation slice: `view: "home" | "workspace"`. `ipc/fs.ts` (typed wrappers for §4.2),
`events.ts`: `onFsChanged`, `onFsWatchError`, `onWorkspaceUpdated`. `ipc/commands.ts`:
`addWorkspaceRoot`, `removeWorkspaceRoot`, `getCommandEvents`.

## 7. Error handling & degradation (summary)

| Failure | Behavior |
|---|---|
| Path outside roots (any op / link click) | typed `OutsideRoot`, quiet honest message; never a silent no-op |
| Preview too large / binary | explicit placeholder card (size shown), never truncated-as-if-whole |
| Watcher dies | `fs://watch-error` → "live-обновления на паузе — обновить" + manual refresh; auto-retry on re-activation |
| Delete | Trash only; failure surfaces `Io{message}` |
| Remove last root | daemon rejects (`Response::Error`), UI explains |
| `GetCommandEvents` on unknown session | empty list (honest, not an error — rehydrated sessions may predate v2 rows) |
| Daemon down | existing banner; file explorer still works (core-local), workspace mutations fail with `Disconnected` |

Logging: structured, no secrets; file CONTENTS never logged (paths only).

## 8. Testing (per repo TDD bar)

- `bpa-paths::validate_path_within`: unit — escape via `..`, symlink-out, root itself, non-existent create-parent, separator-in-name rejection.
- Daemon: migration v2→v3 test (existing ws gets `ord=0` row; fail-closed on error); Add/RemoveWorkspaceRoot handlers (validate, ordering, last-root rejection, `WorkspaceUpdated` push); `GetCommandEvents` (limit, newest-first, unknown-session → empty).
- `fs_explorer` inner fns on tempdirs: listDir gitignore/`.git`/one-level; preview text/binary/tooLarge/truncated; create/rename/move/delete(Trash mock or tempdir `trash` behavior); every `FsError` variant reachable.
- Watch integration test: touch inside tempdir root → `fs://changed` (captured via test emitter) within a bound; ignored path filtered.
- Frontend (vitest): HomeView ordering (waiting pinned, Пройти navigates+activates+focuses — assert store transitions), command strip rendering from mocked `getCommandEvents`, FileTree lazy-load/virtualized smoke on 10k synthetic entries, invalidation on `fs://changed`, FilePreview all three kinds, link-provider regex unit (paths + `:line` strip + non-paths).
- Gate: existing 8-stage `final-suite.sh` (ts-rs parity catches `Workspace`/`CommandEvent` drift) + the new tests. e2e survive-restart untouched (multi-root additive: existing flows keep passing).

## 9. Out of scope (explicit)

In-app editing / syntax highlighting; workspace deletion verb; daemon-side file API (orchd gets
its own in S9 when it truly needs headless reads); task-level progress (S3); hot-questions inbox
beyond waiting-terminals (S6c); cross-project idea capture (S-IDEA); watch while app closed.
