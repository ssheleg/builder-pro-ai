# S2 — Workspace multi-root + File Explorer + Attention-first Home: Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Ship the owner's daily loop: attention-first Home over all terminals ([Пройти →] into any waiting terminal), multi-root workspaces, a right-rail file explorer with read-only preview + live watch, clickable file paths in terminal output, and a per-session command-history strip.

**Architecture:** Daemon keeps owning the Workspace data model (schema v3 `workspace_root` table + 3 additive Hop-B verbs + `workspace://updated` push). ALL file I/O + watch live in the Tauri core (`fs_explorer.rs`, `fs_watcher.rs`) guarded by a new shared `bpa_paths::validate_path_within`; frontend gets three rails (nav | terminals/Home | files) — spec: `docs/superpowers/specs/2026-07-08-s2-workspace-explorer-home-design.md` (authoritative; D1-D9 locked there).

**Tech Stack:** existing workspace + new crates: `ignore` (gitignore walker), `notify` + `notify-debouncer-full` (FSEvents watch), `trash` (reversible delete), `opener` (reveal/open); xterm.js `registerLinkProvider` + `options.linkHandler` (OSC-8). Pin current versions via `cargo add` at task time.

## Global Constraints

- Wire changes are ADDITIVE ONLY (append-only discipline; no `deny_unknown_fields` anywhere — verified). New protocol variants go at the END of enums.
- `Workspace.roots: Vec<String>` (NOT `workspace_roots`, NOT `PathBuf` — spec §3.1 naming note); `root_path` always mirrors `roots[0]`.
- `command_events.kind` literals are exactly `"started"` / `"finished"` (what the Pv2 writer persists).
- Schema migration v2→v3: fail-closed, forward-only, one transaction (Pv2 policy).
- Daemon = security authority for ROOTS (`validate_dir`); core validates every file op via `bpa_paths::validate_path_within` (defense in depth). File contents never logged.
- Delete = Trash only. Preview cap = 1 MiB. Watch debounce = 250 ms. Watch event path cap = 500 (overflow ⇒ `["*"]`).
- Every `#[tauri::command]` = thin wrapper over a unit-testable inner fn. Typed errors camelCase per-variant (`CommandError` pattern; Task-8 lesson: container `rename_all` does not cascade into struct-variant fields).
- Design-system: new atom ⇒ new row in `docs/design-system.md` in the SAME task. Amber only for "human needed". One accent.
- Gate: `bash scripts/final-suite.sh` → `ALL GATES PASSED` (8 stages incl. ts-rs parity + coverage ≥80% + e2e).
- Commits: conventional, trailer `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`.

## Task graph

Sequential contracts first: T1 → T2 → T3 → T4. Then parallel-safe groups (non-overlapping files): {T5→T6}, {T7}, {T8}, {T9}. UI wave: T10, T11, T12, T13 (T11 depends on T9's BL-14 fix). Close: T14 (docs+gate) → T15 (final review + merge).

---

### Task 1: `bpa_paths::validate_path_within`

**Files:** Modify `crates/paths/src/lib.rs` (+ its tests).
**Interfaces — Produces:** `pub fn validate_path_within(root: &Path, candidate: &Path) -> Result<PathBuf, PathError>`; `pub fn validate_parent_within(root: &Path, target: &Path) -> Result<PathBuf, PathError>` (for create/rename destinations: canonicalize parent, validate within root, reject a final component containing a separator or `..`, return joined path).

- [ ] **Step 1: RED.** Tests: candidate inside root → canonical path; `..`-escape → `Err(SymlinkEscape)` (reuse variant; message covers lexical escape); symlink pointing outside root → `Err`; candidate == root → ok; `validate_parent_within` with existing parent + fresh filename → ok; final component `"a/b"` or `".."` → `Err`. Run `cargo test -p bpa-paths` → FAIL.
- [ ] **Step 2: GREEN.** Implement per spec §4.1: canonicalize root; canonicalize candidate (or parent for the `_parent_` variant); `starts_with` check; fail-closed on any canonicalize error.
- [ ] **Step 3:** `cargo test -p bpa-paths` → PASS. Commit `feat(paths): validate_path_within — canonical containment for explorer ops (S2 §4.1)`.

### Task 2: Protocol additions + ts-rs regen

**Files:** Modify `crates/protocol/src/lib.rs`, regenerate `src/ipc/types.ts` (via `cargo test -p bpa-protocol --test ts_export`).
**Interfaces — Produces (locked, spec §3.1/§3.3):** `Workspace.roots: Vec<String>` (+ field on the existing struct, ts-rs exported); `pub struct CommandEvent { session_id, seq: i64, ts: i64, kind: String, exit_code: Option<u8>, origin: String }` (camelCase, TS-exported); enum additions AT THE END: `Request::{AddWorkspaceRoot{workspace_id, path}, RemoveWorkspaceRoot{workspace_id, path}, GetCommandEvents{session_id, limit: u32}}`, `Response::CommandEvents(Vec<CommandEvent>)`, `Push::WorkspaceUpdated(Workspace)`.

- [ ] **Step 1: RED.** Protocol round-trip tests (CBOR encode/decode) for each new variant + `Workspace` with 2 roots; ts_export test asserts `roots`/`CommandEvent` appear in types.ts. FAIL.
- [ ] **Step 2: GREEN.** Add fields/variants (END of enums), plain derives (Pv2 style). Regenerate types.ts; commit the regenerated file.
- [ ] **Step 3:** `cargo test -p bpa-protocol` + `git diff --exit-code src/ipc/types.ts` after regen → PASS. Commit `feat(protocol): Workspace.roots + CommandEvent + Add/RemoveWorkspaceRoot/GetCommandEvents + WorkspaceUpdated (S2 §3, additive)`.

### Task 3: Daemon schema v3 + workspace_root persistence

**Files:** Modify `crates/sessiond/src/persistence.rs`.
**Interfaces — Consumes:** T2 `Workspace.roots`. **Produces:** `SCHEMA_VERSION = 3`; migration step `from_version == 2` creating `workspace_root(workspace_id, ord, path, PRIMARY KEY(workspace_id, ord))` + backfill `INSERT ... SELECT id, 0, root_path FROM workspace`; `upsert_workspace` writes roots (delete+insert rows in one tx; `root_path` column := `roots[0]`); `list_workspaces` joins + orders by `ord`; `add_workspace_root(&WorkspaceId, &str) -> Result<Workspace>`; `remove_workspace_root(&WorkspaceId, &str) -> Result<Workspace, PersistError>` rejecting the LAST root (new `PersistError::LastRoot`); `list_command_events(&SessionId, limit: u32) -> Result<Vec<CommandEvent>>` newest-first (`ORDER BY seq DESC LIMIT ?`).

- [ ] **Step 1: RED.** Tests: fresh DB is v3 with the table; a REAL v2 fixture (create schema-v2 DB in-test, insert a workspace) migrates → `ord=0` backfill present, version bumped, fail-closed on induced error (tx rollback leaves v2); add/remove root ordering; remove-last → `LastRoot`; `list_command_events` limit+ordering+unknown-session→empty. FAIL.
- [ ] **Step 2: GREEN.** Implement. `Workspace` assembly: `roots` from join; `root_path = roots[0]`.
- [ ] **Step 3:** `cargo test -p bpa-sessiond persistence` → PASS. Commit `feat(sessiond): schema v3 workspace_root + multi-root persistence + command_events reader (S2 §3.2)`.

### Task 4: Daemon handlers + push

**Files:** Modify `crates/sessiond/src/socket_server.rs`.
**Interfaces — Consumes:** T3 fns. **Produces:** dispatch arms: `AddWorkspaceRoot` → `bpa_paths::validate_dir` → `db.add_workspace_root` → reply `Response::Workspace(updated)` + broadcast `Push::WorkspaceUpdated(updated)`; `RemoveWorkspaceRoot` → same shape (no validate_dir needed on remove); `GetCommandEvents` → `Response::CommandEvents(db.list_command_events(...))`. Error codes: invalid path → existing path-error code; `LastRoot` → `Response::Error{code:"LastRoot", ...}`.

- [ ] **Step 1: RED.** Socket-level tests (stub-client pattern): add root → Workspace reply with 2 roots + a second connection receives `WorkspaceUpdated`; add with bad path → typed error, nothing persisted; remove-last → `LastRoot`; GetCommandEvents over a real session with OSC-133 marks → started/finished rows, newest-first, limit respected. FAIL.
- [ ] **Step 2: GREEN.** Implement arms (match existing CreateWorkspace arm style; broadcast via the existing broadcaster).
- [ ] **Step 3:** `cargo test -p bpa-sessiond` → PASS (full crate). Commit `feat(sessiond): Add/RemoveWorkspaceRoot + GetCommandEvents handlers + WorkspaceUpdated push (S2 §3.3)`.

### Task 5: Core `fs_explorer`

**Files:** Create `src-tauri/src/fs_explorer.rs`; modify `src-tauri/Cargo.toml` (`cargo add ignore trash opener` — pin what cargo resolves), `src-tauri/src/lib.rs` (mod + register commands), `src-tauri/src/commands.rs` ONLY if `FsError` needs a shared home (keep `FsError` in fs_explorer.rs).
**Interfaces — Consumes:** T1 validators. **Produces (locked, spec §4.2):** commands `list_dir`, `read_file_preview`, `create_file`, `create_dir`, `rename_entry`, `move_entry`, `delete_entry`, `reveal_in_finder`, `open_external`; types `FsEntry{name, rel_path, is_dir, size, is_ignored}` (camelCase serde+TS-ish shape via serde only — NOT a protocol type, no ts-rs), `FilePreview` tagged `kind: "text"|"binary"|"tooLarge"`, `FsError{NotFound|PermissionDenied|OutsideRoot|TooLarge|Io{message}}` (serde tag="kind", per-variant camelCase).

- [ ] **Step 1: RED.** Inner-fn tests on tempdirs: one-level listing, dirs-first not required (frontend sorts), `.git` never listed, `.gitignore`d entry flagged `is_ignored` (and omitted unless `include_ignored`), preview text/truncated-at-1MiB/binary(NUL in first 8KiB)/tooLarge, create/rename/move/delete-to-Trash (assert source gone; Trash restore not asserted — `trash` crate contract), every op rejects an outside-root path with `OutsideRoot`, rename rejects separator in name. FAIL.
- [ ] **Step 2: GREEN.** Implement with `ignore::WalkBuilder` scoped to depth 1; validators from T1 on every op.
- [ ] **Step 3:** `cargo test -p builder-pro-ai fs_explorer` + clippy clean. Commit `feat(core): fs_explorer — gitignore-aware listing, capped preview, Trash delete, reveal/open (S2 §4)`.

### Task 6: Core live watch

**Files:** Create `src-tauri/src/fs_watcher.rs`; modify `src-tauri/Cargo.toml` (`cargo add notify notify-debouncer-full`), `src-tauri/src/lib.rs` (register start/stop commands + state).
**Interfaces — Consumes:** `ignore` matcher (shared helper from T5). **Produces:** `#[tauri::command] start_workspace_watch(roots: Vec<String>, show_ignored: bool)`, `stop_workspace_watch()`; events `fs://changed {root, changedRelPaths}` (dedup, cap 500 → `["*"]`), `fs://watch-error {root, reason}`. One active watch set at a time (switching workspaces restarts it); `Debouncer` stored in managed state (`Mutex<Option<...>>`).

- [ ] **Step 1: RED.** Integration test (real tempdir, real notify): start watch → external `std::fs::write` → debounced event arrives < 2 s (generous cap; DoD <1s is runtime truth, test cap tolerates CI) with the rel path; gitignored path filtered when `show_ignored=false`; stop → no further events; watcher error path emits `fs://watch-error` (simulate by watching a deleted dir). Use a test emitter trait so tests capture events without a Tauri AppHandle (mirror the broker's testable-seam pattern). FAIL.
- [ ] **Step 2: GREEN.** `new_debouncer(Duration::from_millis(250), None, handler)`; per-root `watcher().watch(root, RecursiveMode::Recursive)`.
- [ ] **Step 3:** Tests PASS + clippy. Commit `feat(core): fs_watcher — debounced FSEvents watch with gitignore filter + honest watch-error (S2 §5)`.

### Task 7: Core client wrappers + broker mapping

**Files:** Modify `src-tauri/src/commands.rs` (3 wrappers), `src-tauri/src/broker.rs` (map `Push::WorkspaceUpdated` → emit `workspace://updated` with Workspace payload; add `EV_WORKSPACE_UPDATED`).
**Interfaces — Consumes:** T2 protocol, T4 handlers. **Produces:** `#[tauri::command] add_workspace_root(workspace_id, path) -> Result<Workspace, CommandError>`, `remove_workspace_root(...) -> Result<Workspace, CommandError>`, `get_command_events(session_id, limit) -> Result<Vec<CommandEvent>, CommandError>`; broker mapping test.

- [ ] **Step 1: RED.** commands_over_stub_daemon tests for the three wrappers (incl. LastRoot error mapping to `CommandError::Daemon{code:"LastRoot",..}`); broker unit test `workspace_updated_maps_to_emit`. FAIL.
- [ ] **Step 2: GREEN.** Implement (thin, `state.client()?` pattern). Register in `invoke_handler!`.
- [ ] **Step 3:** `cargo test -p builder-pro-ai` → PASS. Commit `feat(core): workspace-root + command-events wrappers; workspace://updated (S2 §3.3/§6.6)`.

### Task 8: Frontend IPC + store slices

**Files:** Create `src/ipc/fs.ts`; modify `src/ipc/commands.ts`, `src/ipc/events.ts`, `src/store/store.ts` (+ `store.test.ts`).
**Interfaces — Consumes:** T5/T6/T7 command names + event names; types mirrored by hand in `fs.ts` (FsEntry/FilePreview/FsError are core-local, not ts-rs). **Produces (locked):** `fs.ts` typed wrappers for §4.2 commands + `startWorkspaceWatch/stopWorkspaceWatch`; `events.ts`: `onFsChanged`, `onFsWatchError`, `onWorkspaceUpdated`; store: `view: "home"|"workspace"`, `setView`; fs slice `{expanded: Record<string, true>, treeCache: Record<string, FsEntry[]>, selectedFile: {root, rel}|null, showIgnored: boolean, filesRailOpen: boolean, watchPaused: boolean}` + actions incl. `invalidateDirs(root, rels)` (`["*"]` ⇒ clear all for root); key format `` `${root} ${rel}` ``.

- [ ] **Step 1: RED.** store tests: view default "home"; invalidateDirs point + star; selectedFile set/clear; workspaceUpdated upsert keeps existing sessions intact. FAIL.
- [ ] **Step 2: GREEN + Step 3:** `npx vitest run store` + `npx tsc --noEmit` → PASS. Commit `feat(ui): fs/workspace-root ipc + fs store slice + view navigation (S2 §6.6)`.

### Task 9: BL-14 + toast + BL-29 (pre-UI debt that the Пройти flow needs)

**Files:** Modify `src/terminal/terminal-manager.ts` (BL-14: `term.reset()` before applying Replay), `src/theme.ts`/global CSS (`:focus-visible` 2px accent ring app-wide — BL-29), create `src/components/Toast.tsx` (minimal queue-of-one toast atom, honest error surface), modify `docs/design-system.md` (Toast atom row), `docs/backlog.md` (BL-14 → done, BL-29 → done, BL-6 → note "toast infra landed; full sweep still open").
**Interfaces — Produces:** `showToast(message: string)` store action + `<Toast/>` rendered in App (next task wires; here component+store+tests).

- [ ] **Step 1: RED.** terminal-manager test: re-attach applies Replay onto a reset terminal (no duplicated scrollback — assert via mock `term.reset` called before `write`); Toast renders+auto-dismisses; focus-visible CSS present (snapshot of the style block or class assertion). FAIL.
- [ ] **Step 2: GREEN + Step 3:** vitest + tsc PASS. Commit `fix(ui): reset-before-replay (BL-14), :focus-visible ring (BL-29), Toast atom (S2 §7)`.

### Task 10: FileTree + FilePreview + right rail

**Files:** Create `src/components/FileTree.tsx`, `src/components/FilePreview.tsx`, `src/components/FilesRail.tsx` (+ tests); modify `src/App.tsx` (render right rail), `docs/design-system.md` (+2 atom rows: File-tree row, Preview pane).
**Interfaces — Consumes:** T8 fs slice + wrappers; T5 commands. **Produces:** right collapsible rail; lazy tree (roots = top nodes; expand → `listDir` → cache; virtualized rows — plain windowing over a flattened visible list, no new dep); context menu (new file/folder → inline name input; rename; delete→confirm→Trash; reveal; open external); «+ Add root» → `pickFolder` → `addWorkspaceRoot`; ignored dimmed under `showIgnored` toggle; preview pane under tree (text mono read-only / binary / tooLarge / FsError → honest placeholder + toast).

- [ ] **Step 1: RED.** Tests: lazy expand fetches once then caches; `fs://changed` invalidation refetches ONLY affected expanded dir; 10k synthetic flattened list renders windowed subset (<500 DOM rows); context-menu delete calls deleteEntry with confirm; add-root flow calls addWorkspaceRoot; preview kinds render all four states. FAIL.
- [ ] **Step 2: GREEN + Step 3:** vitest + tsc PASS. Commit `feat(ui): files right rail — lazy virtualized tree + read-only preview (S2 §6.4)`.

### Task 11: HomeView + navigation + left-rail restructure

**Files:** Create `src/components/HomeView.tsx` (+ test); modify `src/components/WorkspaceSidebar.tsx` (⌂ Home item on top; nav-only), `src/App.tsx` (view switch: home ⇒ HomeView, workspace ⇒ existing terminal layout; wire watch start/stop on active workspace + `onFsChanged`/`onFsWatchError`/`onWorkspaceUpdated` subscriptions; render `<Toast/>`).
**Interfaces — Consumes:** T8 view state, T9 reset-before-replay (re-attach via Пройти is clean). **Produces:** attention-first Home (spec §6.2): amber «нужен ты» block (Inbox-item pattern) rows with **[Пройти →]** → `setView("workspace")` + `setActiveWorkspace` + `setActiveSession` + terminal focus; «работают» rows (Agent-row atom, click = same nav); «завершились недавно» ✓/✗ by exit code; thin stats strip; empty state.

- [ ] **Step 1: RED.** Tests: ordering (waiting pinned regardless of insertion order); Пройти sets view+activeWorkspace+activeSession (assert store transitions) and requests terminal focus (spy on manager focus hook); group header click navigates; stats counts correct; exited rows show exit code mark. FAIL.
- [ ] **Step 2: GREEN + Step 3:** vitest + tsc PASS. Commit `feat(ui): attention-first Home + [Пройти] one-click jump + nav restructure (S2 §6.2)`.

### Task 12: Workspace view — stat chips + command strip + cwd default

**Files:** Create `src/components/CommandStrip.tsx` (+ test); modify the workspace-view composition in `src/App.tsx`/`TerminalTabs.tsx` area (stat chips row; strip under active terminal), `src/components/WorkspaceSidebar.tsx` or tab-creation site: new-terminal `cwd` = tree-selected node's root (else `roots[0]`).
**Interfaces — Consumes:** `getCommandEvents` (T7), fs slice selection (T8). **Produces:** chips row (live/waiting/exited/roots counts; click toggles a small detail panel); CommandStrip: last 10 events as ✓/✗ chips (`finished` events carry exit_code; `started` without a matching finish renders as running-dot), refetch on `session://state-changed`/`session://exited` for the active session.

- [ ] **Step 1: RED.** Tests: strip renders ✓/✗/running from a mocked event list (incl. newest-first order); refetch fires on state-changed; chips counts; create-terminal uses selected root cwd. FAIL.
- [ ] **Step 2: GREEN + Step 3:** vitest + tsc PASS. Commit `feat(ui): workspace stat chips + OSC-133 command strip + root-aware terminal cwd (S2 §6.3)`.

### Task 13: Terminal file links → preview

**Files:** Create `src/terminal/link-provider.ts` (+ unit test); modify `src/terminal/terminal-manager.ts` (register provider + OSC-8 `linkHandler`).
**Interfaces — Consumes:** session `cwd` from store (`SessionMeta.cwd`), workspace `roots`, `readFilePreview`/fs slice (open in rail). **Produces (spec §6.5):** regex provider — token patterns: `(~?/[\w.@+-]+(?:/[\w.@+-]+)+|\.{0,2}/[\w.@+-][\w./@+-]*|[\w.@+-]+(?:/[\w.@+-]+)+\.\w+)(?::\d+(?::\d+)?)?` refined in-test; strips `:line[:col]`; resolves `~` and relative against the session's live cwd; candidate must lexically fall under one of the active workspace's roots (fast pre-filter) — authoritative check happens in the core command on click; click → `setSelectedFile` + open rail; miss → toast «файл не найден / вне workspace». OSC-8: `options.linkHandler` `allowNonHttpProtocols: true`; `file://` → same path; http(s) → `openExternal`.

- [ ] **Step 1: RED.** Unit tests on the pure resolver: absolute-in-root, relative + cwd, `~`, `:42` strip, `:42:7` strip, outside-root → null, non-path tokens (`a/b` ratio-like inside prose vs real file — accept false positives that fail on click, assert the KNOWN true/false table), OSC-8 file:// and https handling. FAIL.
- [ ] **Step 2: GREEN + Step 3:** vitest + tsc PASS. Commit `feat(ui): terminal file links — regex+OSC-8 → right-rail preview (S2 §6.5, D9)`.

### Task 14: Docs truth + CHANGELOG + full gate

**Files:** Modify `docs/superpowers/specs/2026-07-01-builderpro-platform-overview.md` (line ~152: `workspace_roots: Vec<PathBuf>` sketch → shipped truth `roots: Vec<String>` with the naming-note rationale; S2 roadmap row → shipped + one-line deltas incl. attention-first Home pulled forward from SH), `docs/architecture.md` (three-rail layout + core-owned file I/O boundary), `docs/superpowers/specs/2026-07-01-builderpro-s0s1-foundation-terminal-design.md` (only if §6/§12 claims contradict new events/commands — verify, amend in-place), `README.md` (features + real test counts), `docs/backlog.md` (BL-31 note: GetCommandEvents reads DB directly — in-memory reconcile moot; BL-4/5 remain open with updated target), `docs/traceability.md`, `CHANGELOG.md` (`[0.3.0]`: multi-root workspaces, file explorer + preview + live watch, attention-first Home + Пройти, command strip, terminal file links, BL-14/29 fixes), `docs/design-system.md` sweep (all new atom rows present).
- [ ] **Step 1:** All doc edits; banned-phrase sanity (`git grep -n 'workspace_roots' -- '*.md'` → only historical/spec-note mentions).
- [ ] **Step 2:** `export PATH="$HOME/.cargo/bin:$PATH" && bash scripts/final-suite.sh` → `ALL GATES PASSED` (fix in-scope if a stage fails).
- [ ] **Step 3:** Commit `docs: S2 shipped — multi-root truth, three-rail architecture, CHANGELOG [0.3.0], gate green`.

### Task 15: Whole-branch adversarial review + merge

- [ ] **Step 1:** `scripts/review-package $(git merge-base main HEAD) HEAD`; run the multi-lens adversarial review (lenses: path-security/symlink-escape, watch lifecycle+leaks, migration/data, frontend honesty (Home ordering, Пройти focus, preview truthfulness), link-provider false-positive/securiy, cross-task seams incl. re-attach+reset vs replay). Verify → fix waves (one fixer per wave, complete findings list) → re-gate, per the Pv2 T14 pattern.
- [ ] **Step 2:** `superpowers:finishing-a-development-branch`: verify tests → present the 4 options → on «merge locally»: ff-merge, re-run gate on main, remove worktree, delete branch.

---

## Self-review (done at write time)

Spec→task coverage: §3.1/3.3→T2, §3.2→T3, handlers→T4, §4→T1+T5, §5→T6, §6.6→T7+T8, §6.2→T11, §6.3→T12, §6.4→T10, §6.5→T13, §7 honesty→T5/T6/T9/T10 tests, §8→each task's RED, docs/DoD→T14, review/merge→T15. BL-14/29 pulled in (T9) — load-bearing for the Пройти flow / design-system promise. No placeholders; names consistent (`roots`, `fs://changed`, `workspace://updated`, `FsEntry.relPath` TS-side vs `rel_path` serde — camelCase rename handles it). Parallel groups don't share files.
