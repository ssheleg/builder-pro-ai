# UX test results — audit pass 1

> Companion to [`ux-scenarios.md`](./ux-scenarios.md). Each scenario in the catalog is traced against the
> current code (`main`) — handler → IPC verb → store slice → rendered UI — and given a verdict. This is
> **pass 1**, run before the Part-B UI redesign; pass 2 re-runs against the redesigned UI (Part C).

**Run:** 2026-07-18 · code at `501bbe3` · catalog at `78af949` (re-sync due — see below).
**Method:** 4 parallel Opus auditors, one per epic-group, read-only over `src/**` + `crates/**`. Each verdict
cites `file:line`. Adversarial-but-fair: 🔴 only for a reproducible defect pointed at in code.

## Verdict scheme

| | Meaning |
|---|---|
| ✅ OK | Behaves as the catalog's Expected/States/Errors columns say; error-handling present. |
| 🟡 UX-GAP | Works, but a polish/affordance/consistency gap (severity Minor/Important). |
| 🔴 BUG | Reproducible defect — wrong result, crash, or missing error-handling. |
| 📄 DOC-GAP | Code is correct; the **catalog row** is stale/inaccurate and must be fixed. |

## Overall result

**181 scenarios · 169 ✅ · 10 🟡 · 0 🔴 · 2 📄.**

Zero reproducible bugs. The webview is unusually disciplined: every audited async path (workspace/session
create, `listDir`/`readFilePreview`, upgrades, research runs, MCP connect/invoke, all store `refresh*`) has
real `try/catch` → honest toast/inline surfacing; banners mount globally off honest store flags; degradation
(orchd-down gating of every mutation, storage-fallback banners, reconnect rehydration, token guards against
stale responses, the unverified-data banner on every external payload) is genuinely implemented, not
happy-path-only. All 10 🟡 are Minor except one Important (FI-02); most were already tracked as accepted
by-design gaps whose catalog notes match reality. The 2 📄 are the same finding — a **positive correction**:
the catalog's only Tier-0 Critical (BL-89, "MCP connect hangs forever") is **fixed** and its rows are stale.

| Group (epics) | Scenarios | ✅ | 🟡 | 🔴 | 📄 |
|---|---|---|---|---|---|
| 1 — ON DA WS TE FI | 48 | 47 | 1 | 0 | 0 |
| 2 — PR GO ID | 44 | 43 | 1 | 0 | 0 |
| 3 — RE IN TA GR | 46 | 40 | 5 | 0 | 1 |
| 4 — EX DG XC | 43 | 39 | 3 | 0 | 1 |
| **Total** | **181** | **169** | **10** | **0** | **2** |

## Confirmed findings → actions

| ID | Verdict | Sev | Finding | Action |
|---|---|---|---|---|
| FI-02 | 🟡 | **Important** | Failed `listDir` reuses the synthetic "Loading…" row forever (`treeCache` never populated, `pending` ref stable → no auto-retry); only a 4 s toast. Failed ≡ loading visually; recovery is non-obvious (collapse/re-expand). | **Fix in B-T5**: distinct failed row + inline Retry in `FileTree` (mirror `CommandStrip`). Correct the FI-02 catalog claim ("loading/empty/failed distinct" — only empty is). |
| ID-05 | 🟡 | Minor | "+ idea" (`IdeasList.tsx:495`) and orphan "link to project" (`:335`) are correctly `disabled` while `orchdDown`, but their `opacity` expression omits `orchdDown`, so a typed title renders them full-strength — a disabled control that looks enabled. | **Fix in B-T7**: fold `orchdDown` into the opacity expression (match `QuickCapture`). |
| EX-04 / RE-12 | 📄 | — | Catalog brands MCP connect a Tier-0 Critical (BL-89, "no timeout at any layer"). Code bounds both the handshake and `list_tools` with `tokio::time::timeout(server.timeout_ms)` (default 30 s), backstopped by a 30 s webview transport timeout (`crates/orchd/src/mcp/lifecycle.rs`). | **Retire BL-89**; correct EX-04 + RE-12 catalog rows to "timeout-bounded". |
| XC-03 | 🟡 | Minor | Theme toggle exists + is wired, but light/dark is not yet applied to app chrome — the app still consumes the static dark palette (`src/theme.ts`). | **Resolved by Part B B3** (per-view token refactor) — the redesign is exactly this. |
| GR-02 / GR-12 | 🟡 | Minor | Optimistic graph edge is not rolled back when the add is rejected (self-loop/dup/failure); lingers until the next `graph://changed` push reconciles (P-08). | B-T8 polish or accept per P-08. |
| TA-06 | 🟡 | Minor | Task `source`/provenance is not rendered on the row at all (H-02). | B-T6 — surface provenance chip. |
| IN-07 | 🟡 | Minor | Cancelling the task-form after Create does not roll back the created insight (P-26, by design). | Accept (documented). |
| DG-12 | 🟡 | Minor | Connector-ops `<select>` has no loading affordance while ops load. | B-T9 polish. |
| XC-09 | 🟡 | Minor | Only `FileTree` is windowed; other long lists are un-virtualized (documented). | Backlog (perf). |
| RE-01 | 🟡 | Minor | 0 servers → Run correctly disabled, but no CTA back to Extensions (F-06). | B-T7 polish. |

Lesser observations left ✅ but noted: **TE-04** terminal tab has no `onKeyDown` (real keyboard-a11y gap,
catalog-assigned to XC-06 → fix in B-T5); **ON-07/HomeGoals** shares FI-02's milder "loading label after a
failed fetch" pattern (catalog-documented → same fix family in B-T4).

## Catalog maintenance due — done

- Catalog re-synced to `ac581d4` (post Part-B). Rows **EX-04** / **RE-12** corrected in A-T3
  (BL-89 was already `done` in `docs/backlog.md`, shipped `[0.8.0]`). The **FI-02** row's promise
  ("loading / empty / failed are visually distinct") is now satisfied by the B-T5 code.

## Pass 2 — post-redesign verification (2026-07-18, code at `ac581d4`)

Part B restyled every view onto the token system + primitives kit. Pass 2 verifies the redesign
did not regress behavior and that every actionable pass-1 finding landed:

- **Behavior preserved.** The 886-test suite is green; an adversarial whole-branch review of the
  8 redesign commits (`63a4857..ac581d4`) confirmed no `data-testid` / `role` / `aria` / handler
  was dropped and no test assertion was weakened (test diffs are strengthened). **Verdict: SHIP**
  — 0 Critical, 0 Important, 2 non-blocking Minor (six dialogs still inline the scrim/on-accent
  literals → **BL-98**; one bare-digit HomeView assertion — cosmetic).
- **Findings resolved:** FI-02 (distinct failed-dir row + Retry), TE-04 (terminal-tab keyboard
  activation), TA-06 (task-provenance Badge), ID-05 (disabled-look while orchd down), GR-02/GR-12
  (optimistic-edge rollback), ON-07 (loading line clears after a failed fetch). **XC-03** (theme
  not applied to chrome) is resolved by the token migration itself.
- **Deferred (accepted / backlog):** RE-01 (Extensions CTA), DG-12 (ops-select loading affordance)
  — both need new copy, out of a behavior-preserving restyle; IN-07/P-26, XC-09 (windowing) —
  accepted per catalog.

**Net: 0 reproducible bugs across 181 scenarios; the redesign shipped behavior-preserving.**

---

_Full per-scenario evidence (each with a `file:line → handler → ipc → state` trace) follows, one section per
audit group._

---


# Audit group 1 — full trace

# UX audit — batch 1 (epics ON, DA, WS, TE, FI) @ `main` (5202c14)

Method: for each scenario, traced control (file:line) → handler → ipc verb → store state →
rendered UI, and verified error-handling/degradation actually exist (not just happy path).
Adversarial-but-fair: 🔴 only for a reproducible defect pointed at in code.

Counts: **✅ 47 · 🟡 1 · 🔴 0 · 📄 0** (48 scenarios). The one non-✅ is **FI-02** (failed dir-load
reuses the "Loading…" placeholder → dishonest stuck state; also the catalog's "3-way distinct"
claim is inaccurate).

---

## ON — First-run / onboarding

| ID | Verdict | Sev | One-liner |
|---|---|---|---|
| ON-01 | ✅ | — | Default `view:"home"`, bounded hydrate-retry `[500,1000,2000,5000]`, banners mount globally |
| ON-02 | ✅ | — | `sidebar-empty` shows only when 0 projects AND 0 workspaces; both CTAs stay visible |
| ON-03 | ✅ | — | `home-empty` + optional "Open {name}"; stats strip "N workspaces · N live · N waiting" |
| ON-04 | ✅ | — | `pickFolder`→`createWorkspace`→select+`view:"workspace"`; cancel no-op; reject→toast |
| ON-05 | ✅ | — | `create-project-open` opens `CreateProjectDialog` |
| ON-06 | ✅ | — | `ext-nav-button` → `setView("ext")` |
| ON-07 | ✅ | — | `HomeGoals` null when 0 active projects; `home-goals-empty` "Goals are loading…" (note below) |

### ON-01
- Verdict: ✅ OK
- Traced: `App.tsx:335-402` `hydrate(0)` retry loop (`HYDRATE_RETRY_MS=[500,1000,2000,5000]`) → `listWorkspaces`/`listSessions`; `setHydrated(true)` only on first success; default `view:"home"` (`store.ts:447`); `DaemonBanner`/`StorageBanner`/`OrchdDownBanner` mounted globally (`App.tsx:469-481`).
- Error handling: present — hydrate catch sets `daemonConnected=false` + reschedules; a bring-up failure keeps banners up honestly.
- What user sees: Home renders; no banners once connected.
- Delta: none. (launchd agent install is Rust/launchd-side — not auditable from the webview; frontend Checks all hold.)
- Action: none.

### ON-02
- Verdict: ✅ OK
- Traced: `WorkspaceSidebar.tsx:221-233` `sidebar-empty` gated `sortedProjects.length===0 && list.length===0`; copy `strings.chrome.sidebar.emptyState`. CTAs `create-project-open` (376) + Add workspace (394) render unconditionally.
- Error handling: n/a (onboarding copy).
- What user sees: "No workspaces yet — add a workspace or create a project to begin." + both CTAs.
- Delta: none.
- Action: none.

### ON-03
- Verdict: ✅ OK
- Traced: `HomeView.tsx:231-248` `home-empty` gated `all.length===0`; "Open {name}" gated on `firstWorkspace`; `goTo` sets `view:"workspace"`. Stats `home-stats` (219-229) counts whole store.
- Error handling: n/a.
- What user sees: "No active sessions." + Open button when a workspace exists.
- Delta: none.
- Action: none.

### ON-04
- Verdict: ✅ OK
- Traced: `WorkspaceSidebar.tsx:105-117` `onAdd` → `pickFolder`→`createWorkspace`→`onSelectWorkspaceAndNavigate` (`view:"workspace"`).
- Error handling: present — `dir===null` early return (silent cancel); try/catch → `showToast(addWorkspaceFailed(describeCommandError(e)))`.
- What user sees: new selected row + workspace view, or a toast on failure.
- Delta: none.
- Action: none.

### ON-05
- Verdict: ✅ OK
- Traced: `WorkspaceSidebar.tsx:376-393` `setShowCreateDialog(true)` → `CreateProjectDialog` (412).
- Error handling: n/a (submit paths are PR-01..05).
- What user sees: dialog opens.
- Delta: none (catalog cites `:374-391`; actual `:376-393`).
- Action: none.

### ON-06
- Verdict: ✅ OK
- Traced: `WorkspaceSidebar.tsx:197-218` `ext-nav-button` `onClick={()=>setView("ext")}`; `App.tsx:502-504` renders `ExtPanel`.
- Error handling: n/a.
- What user sees: Extensions panel.
- Delta: none.
- Action: none.

### ON-07
- Verdict: ✅ OK (noted concern)
- Traced: `HomeGoals.tsx:127-189` — `if (activeProjects.length===0) return null`; mount effect (135-143) fetches goals for any active project absent from `goalsByProject`; `home-goals-empty` (182-186) when every active project is still un-fetched; `refreshGoals` (store.ts:630-637) try/catch → toast.
- Error handling: present — `refreshGoals` rejection toasts (not swallowed).
- What user sees: nothing (no projects) / "Goals are loading…" (fetch pending) / project blocks with strategic title + chips.
- Delta: minor honesty concern — on a `refreshGoals` **rejection** the row keeps saying "Goals are loading…" (the effect deps are `[projects]`, so no auto-retry until a `projects-changed` push re-runs it). Catalog documents this as toast-only, so code matches catalog; the incidental recovery (projects-changed → `refreshProjects` changes `projects` ref → effect re-runs) makes it milder than FI-02. Same root pattern as FI-02.
- Action: none (candidate: reuse a future distinct failed/retry state).

---

## DA — Daemon lifecycle

| ID | Verdict | Sev | One-liner |
|---|---|---|---|
| DA-01 | ✅ | — | Red `DaemonBanner`; `onDaemonReconnected` resets attachments + eager re-attach visible |
| DA-02 | ✅ | — | Cold rehydrate; `markExited` clears stale waiting; StatusDot distinguishes inactive |
| DA-03 | ✅ | — | `{orchdDown && <OrchdDownBanner/>}`; mutations gated across domain surfaces |
| DA-04 | ✅ | — | Retry → `orchdReconnect()` fire-and-forget; heals on `orchd://up` |
| DA-05 | ✅ | — | Sessiond `UpgradeDialog`; body `daemonDetail(n)` vs `daemonDetailAll` on `hydrated` |
| DA-06 | ✅ | — | Orchd variant `orchd-upgrade-dialog`; `orchdBody`; `orchdUpgrade()` |
| DA-07 | ✅ | — | `orchdOpen = !sessiondOpen && …` (`UpgradeDialog.tsx:64-66`) — sessiond wins |
| DA-08 | ✅ | — | Rejected upgrade → inline error; sessiond→store `upgradeError`, orchd→local state; cleared on retry |
| DA-09 | ✅ | — | Cancel keeps `daemonIncompatible`; `DaemonBanner` "outdated" + Update re-opens |
| DA-10 | ✅ | — | `OrchdUpgradeBanner` rescues cancelled orchd upgrade (BL-96) |
| DA-11 | ✅ | — | `StorageBanner` "Database was corrupted…" for `recovered_from_corruption` |
| DA-12 | ✅ | — | `StorageBanner` "running in memory…" for `in_memory_fallback`; no dismiss by design |
| DA-13 | ✅ | — | Hydrate-fallback `daemonStatus()` opens dialog once (first detection only) |

### DA-01
- Verdict: ✅ OK
- Traced: `DaemonBanner.tsx:62-77` renders red "Daemon disconnected — reconnecting…" when `!connected && !incompatible`; `App.tsx:162-183` `onDaemonReconnected` → `setDaemonConnected(true)` + `manager.resetAllAttachments()` + `hydrate(0)` + eager `attach(activeSessionId)`.
- Error handling: present — hidden sessions re-attach lazily on tab switch; hydrate has its own retry.
- What user sees: red banner while down; sessions re-attach on reconnect.
- Delta: none.
- Action: none.

### DA-02
- Verdict: ✅ OK
- Traced: `TerminalPane.tsx:44-60` attach on mount (Replay-before-open); `store.markExited` (508-527) clears `waitingForInput` so a live event can't resurrect a stale waiting flag; `StatusDot` shows inactive.
- Error handling: replay-only up to last flush is inherent (documented); StatusDot honest.
- What user sees: replayed scrollback, inactive dot.
- Delta: none.
- Action: none.

### DA-03
- Verdict: ✅ OK
- Traced: `App.tsx:481` `{orchdDown && <OrchdDownBanner/>}`; `store.orchdDown` set by `onOrchdDown` (249). Mutating controls read `orchdDown` (verified in Goals/Tasks/Ideas/etc. `disabled` props — out-of-batch surfaces).
- Error handling: present — reads (lists/files/search) stay live.
- What user sees: "Orchestrator unavailable" + Retry; disabled mutations.
- Delta: none.
- Action: none.

### DA-04
- Verdict: ✅ OK
- Traced: `OrchdDownBanner.tsx:48-56` `orchd-down-retry` → `void orchdReconnect()`; heals via `onOrchdUp` (`App.tsx:250-304`) which rehydrates every live slice.
- Error handling: present (by design fire-and-forget; outcome via `orchd://down`/`up`).
- What user sees: Retry with no busy spinner (catalog-documented).
- Delta: none.
- Action: none.

### DA-05
- Verdict: ✅ OK
- Traced: `UpgradeDialog.tsx:107-211` sessiond branch; `n = Object.values(sessions).filter(isActive)`; copy `hydrated ? daemonDetail(n) : daemonDetailAll`; "Update" → `handleUpgradeClick`→`upgradeDaemon().catch`.
- Error handling: present — `.catch` never `await`; success path never resolves (webview killed).
- What user sees: "Update required" + N-live-sessions warning.
- Delta: none.
- Action: none.

### DA-06
- Verdict: ✅ OK
- Traced: `UpgradeDialog.tsx:217-310` orchd branch; `orchd-upgrade-dialog`; body `orchdBody`; "Update" → `orchdUpgrade().catch`.
- Error handling: present (local `orchdUpgradeError`).
- What user sees: "Update required" + orchd copy (no session count).
- Delta: none.
- Action: none.

### DA-07
- Verdict: ✅ OK
- Traced: `UpgradeDialog.tsx:64-66` `sessiondOpen` checked first; `orchdOpen = !sessiondOpen && orchdIncompatible && orchdUpgradeDialogOpen`.
- Error handling: n/a.
- What user sees: sessiond dialog first when both incompatible.
- Delta: none.
- Action: none.

### DA-08
- Verdict: ✅ OK
- Traced: `UpgradeDialog.tsx:76-90` both handlers clear error first (`setUpgradeError(null)`/`setOrchdUpgradeError(null)`) then `.catch(extractUpgradeFailureReason)`; inline `role="alert"` (158-174 / 260-273); sessiond error in store, orchd in local state.
- Error handling: present + honest (mapped reason + launchctl hint).
- What user sees: red inline "Failed to restart… Check permissions (launchctl)…".
- Delta: none.
- Action: none.

### DA-09
- Verdict: ✅ OK
- Traced: `UpgradeDialog.tsx:178` Cancel → `setUpgradeDialogOpen(false)` only (never clears `daemonIncompatible`); `DaemonBanner.tsx:26-59` incompatible branch + Update re-opens.
- Error handling: n/a (flag honesty invariant preserved).
- What user sees: banner "Background service is outdated — update required" + Update.
- Delta: none.
- Action: none.

### DA-10
- Verdict: ✅ OK
- Traced: `OrchdUpgradeBanner.tsx:44-64` renders while `orchdIncompatible && !orchdUpgradeDialogOpen`; `orchd-upgrade-reopen` → `setOrchdUpgradeDialogOpen(true)`.
- Error handling: n/a.
- What user sees: "Orchestrator service is outdated — update required" + Update (no dead-end).
- Delta: none.
- Action: none.

### DA-11
- Verdict: ✅ OK
- Traced: `StorageBanner.tsx:33-46` reads `storageStatus.storageMode`; `recovered_from_corruption` → `strings.storage.recovered(quarantinedPath)`.
- Error handling: honest persistent banner (no dismiss; mode fixed at boot).
- What user sees: "Database was corrupted and has been reset. The damaged copy was saved to {path}."
- Delta: catalog Checks column names the field `storageStatus.mode`; the actual field is `storageStatus.storageMode` (`orchd-types.ts:229`). Behavior correct — trivial checks-column shorthand inaccuracy only.
- Action: none (optional catalog-fix of the field name).

### DA-12
- Verdict: ✅ OK
- Traced: `StorageBanner.tsx:35-40` `in_memory_fallback` → `strings.storage.inMemory`.
- Error handling: present (surfaced, not silent).
- What user sees: "Storage unavailable — running in memory. Changes will NOT survive a restart."
- Delta: none.
- Action: none.

### DA-13
- Verdict: ✅ OK
- Traced: `App.tsx:358-386` hydrate catch → best-effort `daemonStatus()`; `kind==="incompatible"` sets `daemonIncompatible`, opens dialog only if `!alreadyDetected`.
- Error handling: present + double-swallow guarded; only first detection opens dialog.
- What user sees: upgrade dialog even when the single-shot event was lost.
- Delta: none.
- Action: none.

---

## WS — Workspaces

| ID | Verdict | Sev | One-liner |
|---|---|---|---|
| WS-01 | ✅ | — | Same as ON-04; not orchd-gated (sessiond command) |
| WS-02 | ✅ | — | `dir===null` early return → silent no-op |
| WS-03 | ✅ | — | FileTree `+ Add root` → `addWorkspaceRoot`→`upsertWorkspace`; reject→`addRootFailed` toast |
| WS-04 | ✅ | — | `removeWorkspaceRoot` exists (`commands.ts:99`) but no UI control — matches documented gap |
| WS-05 | ✅ | — | Link select → `orchdAddProjectWorkspace`; empty id early-return; reject→toast; not orchd-gated |
| WS-06 | ✅ | — | Row click → `onSelectWorkspace`+`view:"workspace"`; `title={rootPath}` hover |
| WS-07 | ✅ | — | `workspace-stats` chips `N live·K waiting·M exited·R roots`, one-open drill-down, "—" empty |
| WS-08 | ✅ | — | "No project" header always renders; archived projects' workspaces never leak |

### WS-01
- Verdict: ✅ OK
- Traced: `WorkspaceSidebar.tsx:105-117` (identical to ON-04); sessiond command, not orchd-gated.
- Error handling: present (toast + cancel no-op).
- What user sees: selected row + workspace view.
- Delta: none.
- Action: none.

### WS-02
- Verdict: ✅ OK
- Traced: `WorkspaceSidebar.tsx:111` `if (dir===null) return`.
- Error handling: acceptable silence for an explicit cancel.
- What user sees: nothing changes.
- Delta: none.
- Action: none.

### WS-03
- Verdict: ✅ OK
- Traced: `FileTree.tsx:415-424` `onAddRoot` → `pickFolder`→`addWorkspaceRoot(workspace.id,dir)`→`upsertWorkspace`; cancel `dir===null` return; catch → `addRootFailed(describeCommandError)`.
- Error handling: present (mapped command error toast; duplicate/symlink/network → daemon reject → toast).
- What user sees: new root node, or a toast on failure.
- Delta: none.
- Action: none.

### WS-04
- Verdict: ✅ OK (documented capability-without-UI gap)
- Traced: `commands.ts:90-99` exposes `addWorkspaceRoot`/`removeWorkspaceRoot` (LastRoot guard noted at 96); grep confirms **no component calls `removeWorkspaceRoot`**; FileTree `Rename`/`Delete` menu items gated `!isRoot` (`FileTree.tsx:496,509`, `isRoot = node.rel===""`).
- Error handling: n/a (no reachable control).
- What user sees: no remove-root button; root rows have no Delete/Rename.
- Delta: none — catalog Expected explicitly says "Capability exists but no UI control at this commit"; code conforms.
- Action: none (candidate backlog if remove-root UI is later desired).

### WS-05
- Verdict: ✅ OK
- Traced: `WorkspaceSidebar.tsx:119-129,288-307` — select `attach-workspace-{id}` → `handleAttach` → `if (projectId==="") return` else `orchdAddProjectWorkspace`+`refreshProjects`; catch → `describeOrchdError` toast; select rendered only when `activeProjects.length>0`.
- Error handling: present; NOT orchd-down-gated → honest toast on failure (P-06).
- What user sees: workspace re-groups under the project; toast on failure.
- Delta: none.
- Action: none.

### WS-06
- Verdict: ✅ OK
- Traced: `WorkspaceSidebar.tsx:134-158` `renderWorkspaceButton` `onClick={onSelectWorkspaceAndNavigate}` (`onSelectWorkspace(id)`+`setView("workspace")`); `title={w.rootPath}`.
- Error handling: n/a.
- What user sees: navigates to workspace; root path on hover.
- Delta: none.
- Action: none.

### WS-07
- Verdict: ✅ OK
- Traced: `App.tsx:579-640` `WorkspaceStatsChips` (`workspace-stats`); waiting/live/exited split mirrors HomeView + `roots.length`; one-open-at-a-time detail; empty detail → "—".
- Error handling: n/a (pure presentational; scoped to active workspace).
- What user sees: clickable chips + drill-down list.
- Delta: none.
- Action: none.

### WS-08
- Verdict: ✅ OK
- Traced: `WorkspaceSidebar.tsx:271-311` `project-group-unassigned` header always renders; `unlinkedWorkspaces = list.filter(!linkedIds.has)`; `linkedWorkspaceIds(projects)` counts ALL projects (incl. archived) so archived workspaces don't leak.
- Error handling: n/a.
- What user sees: "No project" header + every unlinked workspace.
- Delta: none.
- Action: none.

---

## TE — Terminals

| ID | Verdict | Sev | One-liner |
|---|---|---|---|
| TE-01 | ✅ | — | `+ New terminal` guarded `!activeWorkspaceId` (dim, not-allowed); not orchd-gated |
| TE-02 | ✅ | — | `create_session` reject → `newTerminalFailed` toast (no silent no-op) |
| TE-03 | ✅ | — | Close → `killSession`; `dispose`+`removeSession` in `finally`; toast on fail (no zombie) |
| TE-04 | ✅ | — | Tab click → `setActiveSession`; keyboard-activation gap real but XC-06-owned |
| TE-05 | ✅ | — | Cold rehydrate; attach fire-and-forget (no `.catch`, documented) |
| TE-06 | ✅ | — | `command-strip` loading/empty/failed(+Retry) distinct; token guard; lone-started→interrupted |
| TE-07 | ✅ | — | Placeholder "No terminals yet…" (0 sessions) vs "Select a terminal tab." |
| TE-08 | ✅ | — | `StatusDot` idle/running/waiting/exited; exited wins; `data-state` |

### TE-01
- Verdict: ✅ OK
- Traced: `TerminalTabs.tsx:58-82,156-171` `onNewTerminal` guard `if(!activeWorkspaceId) return`; button `disabled={!activeWorkspaceId}` + not-allowed cursor; `createSession`.
- Error handling: present (see TE-02).
- What user sees: new active tab, or disabled + button when no workspace.
- Delta: none.
- Action: none.

### TE-02
- Verdict: ✅ OK
- Traced: `TerminalTabs.tsx:77-81` try/catch → `showToast(newTerminalFailed(describeCommandError(e)))`.
- Error handling: present, honest.
- What user sees: toast on a rejected create.
- Delta: none.
- Action: none.

### TE-03
- Verdict: ✅ OK
- Traced: `TerminalTabs.tsx:84-98` `onClose` try→`killSession`; catch→`closeTerminalFailed` toast; `finally`→`manager.dispose`+`removeSession`.
- Error handling: present; no zombie tab even on kill failure.
- What user sees: tab removed always; toast on failure.
- Delta: none.
- Action: none.

### TE-04
- Verdict: ✅ OK (a11y gap confirmed, XC-06-owned)
- Traced: `TerminalTabs.tsx:114-133` `role="tab" tabIndex={0} onClick={setActiveSession}` — **no `onKeyDown`**.
- Error handling: n/a.
- What user sees: click activates the tab; a keyboard-focused tab does not activate on Enter/Space.
- Delta: the keyboard-activation gap is real and reproducible, but the catalog assigns it to XC-06 and TE-04's Expected (click) is met.
- Action: none in this batch (cross-ref XC-06 fix).

### TE-05
- Verdict: ✅ OK
- Traced: `TerminalPane.tsx:52` `void manager.attach(sessionId)` (fire-and-forget, no `.catch`); StatusDot marks inactive.
- Error handling: attach coalescing in manager; a rejected attach is silent (documented) but disconnect honesty is carried by `DaemonBanner`.
- What user sees: replayed scrollback; inactive until re-attach.
- Delta: none (matches documented behavior).
- Action: none.

### TE-06
- Verdict: ✅ OK
- Traced: `CommandStrip.tsx:144-278` three pre-strip states (`command-strip-loading`/`-empty`/`-failed`+`command-strip-retry`); `requestRef` token guard; `pairCommandEvents(events,isLive)` renders lone-`started`→`interrupted` when `!isActive`; `role="list"` "Command history".
- Error handling: present — failure toasts AND offers inline Retry (never null-forever).
- What user sees: outcome chips ✓/✗{code}, running dot, interrupted marker; distinct loading/empty/failed.
- Delta: none.
- Action: none.

### TE-07
- Verdict: ✅ OK
- Traced: `App.tsx:531-535` — `Object.keys(sessions).length===0` → "No terminals yet — pick a workspace and press + New terminal." else "Select a terminal tab."
- Error handling: n/a.
- What user sees: honest placeholder per case.
- Delta: none.
- Action: none.

### TE-08
- Verdict: ✅ OK
- Traced: `StatusDot.tsx:13-62` `dotStateOf` — exited wins; running+waiting→waiting; `role="img"` aria-labels; `data-state`.
- Error handling: n/a (pure).
- What user sees: colored dot with correct aria label.
- Delta: none.
- Action: none.

---

## FI — Files

| ID | Verdict | Sev | One-liner |
|---|---|---|---|
| FI-01 | ✅ | — | `filesRailOpen` flips; null when no workspace; 28px reopen strip when collapsed |
| **FI-02** | **🟡** | **Important** | Failed dir-load reuses "Loading…" placeholder (stuck) + transient toast; no distinct failed row / no retry |
| FI-03 | ✅ | — | File click → `setSelectedFile`+`setFilesRailOpen(true)`; "Select a file to preview" empty |
| FI-04 | ✅ | — | Preview binary/tooLarge/truncated cards; read fail → inline + toast; token guard |
| FI-05 | ✅ | — | New file/folder; blank→silent cancel; exists→`createFailed` toast; dirs-only menu items |
| FI-06 | ✅ | — | Rename (non-root); reject→toast; renaming selected re-points selection |
| FI-07 | ✅ | — | Delete (non-root) after `window.confirm`; reject→toast; clears preview if selected |
| FI-08 | ✅ | — | Reveal/Open external; reject→specific toast |
| FI-09 | ✅ | — | `toggleShowIgnored` + `invalidateDirs(root,["*"])` per root; default off |
| FI-10 | ✅ | — | `watchPaused` amber "live updates paused — refresh"; refresh restarts watch (fire-and-forget) |
| FI-11 | ✅ | — | OSC file link → preview if inside workspace; outside → `fileOutsideWorkspace` toast |
| FI-12 | ✅ | — | Windowed render (start/end/overscan, ~38 rows) — the one virtualized surface |

### FI-01
- Verdict: ✅ OK
- Traced: `FilesRail.tsx:23-65` — `null` when `!workspace`; `!open` renders 28px strip with `⟨` → `setFilesRailOpen(true)`; open renders `⟩`/tree/preview.
- Error handling: n/a; reopen strip guarantees a way back.
- What user sees: expand/collapse with a persistent reopen affordance.
- Delta: none.
- Action: none.

### FI-02
- Verdict: 🟡 UX-GAP — **Important**
- Traced: `FileTree.tsx:293-308` fetch effect → `listDir(root,rel,showIgnored)`; `.catch` → `showToast(readFolderFailed(describeFsError))`; **on failure `cacheDir` is never called**, so `treeCache[key]` stays `undefined` → `computeFlatten` (`:133-146`) keeps emitting the synthetic `loading` row (`file-row`, "Loading…") for that dir. The effect deps are `[pending, showIgnored]`; after a failed fetch none of `workspace.roots/expanded/treeCache/showIgnored` changed, so `useMemo` returns the same `pending` reference and the effect **does not re-run** → no auto-retry.
- Error handling: partial — the toast fires (honest for ~4s), but the tree itself has **no distinct failed state and no retry**. Confirmed intentional/tested: `FileTree.test.tsx:119` "a FAILED dir load keeps the loading placeholder + toasts — NOT the empty marker".
- What user sees: after the 4s toast auto-dismisses, a folder whose read failed shows a permanent "Loading…" row — indistinguishable from a still-loading dir. Recovery (collapse + re-expand) is non-obvious.
- Delta from Expected: the catalog's Errors/edge claim "Loading vs empty vs failed are **visually distinct** (fixes P-12)" is only 2/3 true — *empty* is distinct (`file-row-empty` "empty folder"), but *failed* reuses the *loading* row. So a failed read is dishonestly rendered as "Loading…".
- Action: **fix-in-Part-B** — add a distinct failed row + inline [Retry] in the tree (mirror `CommandStrip`'s failed state), OR catalog-fix the "3-way distinct" claim to "empty-vs-loading distinct + toast". File BL if not folded into Part B/B4.

### FI-03
- Verdict: ✅ OK
- Traced: `FileTree.tsx:350-353` `selectFile` → `setSelectedFile({root,rel})`+`setFilesRailOpen(true)`; `FilePreview.tsx:103-104` "Select a file to preview" when none.
- Error handling: n/a.
- What user sees: preview + rail opens.
- Delta: none.
- Action: none.

### FI-04
- Verdict: ✅ OK
- Traced: `FilePreview.tsx:63-157` — `readFilePreview`; `requestRef` token guard; `binary`/`tooLarge` cards with `formatBytes`; `truncated` → amber "content may have changed"; catch → inline red + `openFileFailed` toast.
- Error handling: present, honest, no raw dump.
- What user sees: correct card per kind; error inline + toast.
- Delta: none.
- Action: none.

### FI-05
- Verdict: ✅ OK
- Traced: `FileTree.tsx:355-368,447-456` — New file/folder items gated `node.isDir`; `submitForm` `if(value==="") return` (silent cancel); `doCreate` catch → `createFailed(fileWord/folderWord, describeFsError)` (alreadyExists mapped).
- Error handling: present.
- What user sees: new row; toast on exists; silent cancel on blank.
- Delta: none.
- Action: none.

### FI-06
- Verdict: ✅ OK
- Traced: `FileTree.tsx:370-382,496-508` — Rename gated `!isRoot`; `doRename` catch → `renameFailed`; re-points `selectedFile` when the renamed file was selected.
- Error handling: present.
- What user sees: renamed row; selection follows.
- Delta: none.
- Action: none.

### FI-07
- Verdict: ✅ OK
- Traced: `FileTree.tsx:384-397,509-522` — Delete gated `!isRoot`; `window.confirm(deleteConfirm(label,rel))`; `deleteEntry`; catch → `deleteFailed`; clears preview if the deleted file was selected.
- Error handling: present.
- What user sees: confirm → row gone / toast; preview cleared.
- Delta: none.
- Action: none.

### FI-08
- Verdict: ✅ OK
- Traced: `FileTree.tsx:399-413,523-546` — `doReveal`/`doOpenExternal`; catch → `revealFailed`/`openExternalFailed`.
- Error handling: present.
- What user sees: OS reveal/open; specific toast on failure.
- Delta: none.
- Action: none.

### FI-09
- Verdict: ✅ OK
- Traced: `FilesRail.tsx:77-82,154` — `onToggleShowIgnored` → `toggleShowIgnored`+`invalidateDirs(root,["*"])` per root; default `showIgnored=false` (`store.ts:451`).
- Error handling: n/a (stale-listing safety via invalidation).
- What user sees: ignored entries appear dimmed; fresh honest listings.
- Delta: none.
- Action: none.

### FI-10
- Verdict: ✅ OK
- Traced: `FilesRail.tsx:88-94,158-178` — `watchPaused` amber button `liveUpdatesPaused`; `onRefreshWatch` → `startWorkspaceWatch` (fire-and-forget) + `invalidateDirs(*)` + `setWatchPaused(false)`; `App.tsx:160` `onFsWatchError` → `setWatchPaused(true)` re-fires on renewed failure.
- Error handling: present (optimistic clear; re-pause on renewed error).
- What user sees: "live updates paused — refresh" → refresh restarts watch.
- Delta: none.
- Action: none.

### FI-11
- Verdict: ✅ OK
- Traced: `terminal-manager.ts:201-270` `wireFileLinks` — `provideLinks` resolves via `findFileLinks(lineText,cwd,roots)`; `activate`→`openFileLink(root,rel)` (`setSelectedFile`+`setFilesRailOpen`); `linkHandler.activate` for `file://` → `matchWorkspaceRoot`; miss → `showToast(fileOutsideWorkspace)`.
- Error handling: present (outside/not-found toast).
- What user sees: in-workspace link opens preview; outside → "file is outside the workspace or not found".
- Delta: none.
- Action: none.

### FI-12
- Verdict: ✅ OK
- Traced: `FileTree.tsx:681-700` — `startIndex/visibleCount/endIndex` window (`ROW_HEIGHT=22`, `OVERSCAN=8`); spacer divs; `visible = nodes.slice(startIndex,endIndex)` (~38 rows).
- Error handling: n/a.
- What user sees: smooth scroll over large dirs; <500 DOM rows.
- Delta: none.
- Action: none.


# Audit group 2 — full trace

# UX-scenario audit — Batch 2 (epics PR, GO, ID)

Audited against CURRENT code on `main` (read-only). Verdicts per A3 template.
Verdict key: ✅ OK · 🟡 UX-GAP · 🔴 BUG · 📄 DOC-GAP (severity Critical/Important/Minor on 🟡/🔴).

**Counts:** ✅ 43 · 🟡 1 · 🔴 0 · 📄 0  (44 scenarios total).

---

## PR — Projects (incl. Rules tab)

| ID | Verdict | Sev | One-liner |
|---|---|---|---|
| PR-01 | ✅ | — | Create project: guard + disabled(blocked/empty/submitting), toast "Project created", closes |
| PR-02 | ✅ | — | 0 workspaces → `create-project-blocked` role=alert + submit disabled |
| PR-03 | ✅ | — | "+ create workspace" upserts+auto-selects; failure → inline (describeCommandError) + toast |
| PR-04 | ✅ | — | orchd reject → `create-project-error` role=alert + toast, dialog stays open, cleared before retry |
| PR-05 | ✅ | — | Dialog never reads orchdDown; submit fires → honest "orchestrator unavailable" toast/inline |
| PR-06 | ✅ | — | Add-workspace select → link + refreshProjects; not orchd-gated → honest toast on fail |
| PR-07 | ✅ | — | Unlink → refreshProjects; last workspace → "invalid operation:" Invariant toast |
| PR-08 | ✅ | — | Copy JSON → clipboard + "JSON copied"; both fail paths → one toast (P-28) |
| PR-09 | ✅ | — | Save to file: pickFolder→export; cancel no-op; >16MiB → "service error:" Io |
| PR-10 | ✅ | — | Browse import (read-only listDir), `.json` filter, "No .json files…" empty |
| PR-11 | ✅ | — | Import → importSummary toast; conflict/validation mapped; BL-90 latent (backend) |
| PR-12 | ✅ | — | Archive: guard + disabled(orchdDown‖submitting), confirm text exact, toast "Project archived" |
| PR-13 | ✅ | — | Un-archive: `project-archived-banner` + [Un-archive], guard + disabled |
| PR-14 | ✅ | — | `archived-projects-toggle` reveals dimmed group; hidden at count 0 |
| PR-15 | ✅ | — | Second project via same dialog; unlinked-only picker; race → conflict |
| PR-16 | ✅ | — | Unresolvable ws → `workspace unavailable` chip + [Unlink] |
| PR-17 | ✅ | — | 7 tabs, one mounts at a time; `project-panel-loading`; counters row |
| PR-18 | ✅ | — | Rules Save: disabled=orchdDown; hidden when file missing; fail → toast |
| PR-19 | ✅ | — | Policy: client validatePolicy inline errors; Save policy disabled=orchdDown |
| PR-20 | ✅ | — | `ruleset-banner-missing`/`-modified` + [Recreate]/[Accept] disabled=orchdDown; reveal live |

### PR-01 — Create project
- Verdict: ✅ OK
- Traced: `create-project-submit` button `CreateProjectDialog.tsx:367-375` → `submit=guard(handleSubmit)` (`:277`) → `handleSubmit` `:261-273` → `orchdCreateProject(name,desc,selectedIds)` → `showToast(strings.project.projectCreated)` + `onClose()`. `strings.project.projectCreated="Project created"`.
- Error handling: present/honest — catch `:268-272` sets inline `create-project-error` (role=alert) AND toast via `describeOrchdError(e)`; dialog stays open.
- What the user sees: disabled Create until name+≥1 ws; on success dialog closes + "Project created" toast; sidebar group appears via `orchd://projects-changed`→refreshProjects (App.tsx:200).
- Delta: none.
- Action: none.

### PR-02 — Zero workspaces block
- Verdict: ✅ OK
- Traced: `blocked=selectedIds.length===0` `:226`; renders `create-project-blocked` role=alert `:346-350` with `strings.project.workspaceRequired="at least one workspace is required"`; submit `disabled={blocked||empty||submitting}` `:370`.
- Error handling: n/a (validation gate).
- What the user sees: inline red block line + greyed Create.
- Delta: none. Action: none.

### PR-03 — Inline create workspace
- Verdict: ✅ OK
- Traced: `create-project-new-workspace` `:336-343` → `handleCreateWorkspace` `:245-259` → `pickFolder`→`createWorkspace`→`upsertWorkspace`→auto-select. Empty list → `strings.project.noFreeWorkspaces="no available workspaces"` `:320-321`.
- Error handling: present/honest — catch maps sessiond CommandError via `describeCommandError` (P-17) into inline `create-project-error` + toast `:252-258`.
- What the user sees: new checked checkbox appears; on failure a mapped inline+toast message.
- Delta: none. Action: none.

### PR-04 — orchd rejects create
- Verdict: ✅ OK
- Traced: catch `:268-272`; `createError` cleared before each submit `:263`; inline `create-project-error` role=alert `:352-361`.
- Error handling: present/honest, dialog stays open to retry (mirrors UpgradeDialog).
- What the user sees: mapped error inline (survives toast clobber) + toast.
- Delta: none. Action: none.

### PR-05 — orchd down, dialog does not gate
- Verdict: ✅ OK
- Traced: grep confirms NO `orchdDown` read in `CreateProjectDialog.tsx`. Submit gated only on `blocked||name empty||submitting` `:370`. On submit, `orchdCreateProject` rejects `disconnected` → `describeOrchdError`→`strings.errors.unavailable="orchestrator unavailable"`.
- Error handling: present/honest — lands on inline+toast "orchestrator unavailable", never fake success.
- What the user sees: Create is clickable, fires, then honest failure surface.
- Delta: none (residual gating gap is intentional per catalog/DG-03). Action: none.

### PR-06 — Overview add workspace
- Verdict: ✅ OK
- Traced: `project-add-workspace-select` `:416-435` → `handleAddWorkspace` `:218-228` → `orchdAddProjectWorkspace`→`refreshProjects`. Select hidden when `unlinkedWorkspaces.length===0`.
- Error handling: present — not orchd-gated; catch → `showToast(describeOrchdError)` (honest toast on fail).
- What the user sees: workspace row appears; on fail a toast.
- Delta: none. Action: none.

### PR-07 — Overview unlink workspace
- Verdict: ✅ OK
- Traced: `project-workspace-detach-${wsId}` `:404-411` → `handleDetachWorkspace` `:209-216` → `orchdRemoveProjectWorkspace`→`refreshProjects`.
- Error handling: present — not orchd-gated; last-workspace removal → server Invariant → `strings.errors.invariant`="invalid operation: {msg}".
- What the user sees: row gone; last-ws attempt → "invalid operation:" toast.
- Delta: none. Action: none.

### PR-08 — Copy JSON
- Verdict: ✅ OK
- Traced: `project-export-copy` `:441` → `handleCopyJson` `:230-238` → `orchdExportProject`→`navigator.clipboard.writeText`→`showToast(strings.project.jsonCopied)`.
- Error handling: present — single catch `showToast(describeOrchdError(e))` covers both export and clipboard-permission failure (P-28, documented shared path).
- What the user sees: "JSON copied" toast on success; a toast on failure.
- Delta: minor observation — a clipboard-permission DOMException isn't `{kind}`-shaped, so it maps to `strings.errors.unknown="unknown orchestrator error"`, slightly misleading copy for a clipboard error. Catalog explicitly accepts this as the shared P-28 path; Expected ("failed: toast") is met.
- Action: none (documented; optional future copy tweak).

### PR-09 — Save to file
- Verdict: ✅ OK
- Traced: `project-export-file` `:444-451` → `handleExportToFile` `:240-249` → `pickFolder` (cancel→no-op `:243`) → `orchdExportToFile`→`showToast(strings.project.exportedToFile)`.
- Error handling: present — >16MiB → Io "service error:" via describeOrchdError toast.
- What the user sees: folder picker; "Exported to file"; cancel = nothing.
- Delta: none. Action: none.

### PR-10 — Import browse
- Verdict: ✅ OK
- Traced: `project-import-browse` `:457-464` → `handleBrowseImport` `:251-261` → `pickFolder`→`listDir`→filter `.json`. Empty → `strings.project.noJsonFiles` `:467-468`.
- Error handling: present — read-only browse (no orchd verb); catch → toast.
- What the user sees: per-file buttons or "No .json files in the selected folder".
- Delta: none. Action: none.

### PR-11 — Import file
- Verdict: ✅ OK
- Traced: `project-import-file-${name}` `:470-480` → `handleImportFile` `:263-274` → `orchdImportFromFile`→`showToast(strings.project.importSummary(report))` (`Imported: projects X, goals Y, ideas Z, insights W, tasks V`)→`refreshProjects`.
- Error handling: present — id collision → Conflict "conflict:", malformed → Validation "invalid data:" via describeOrchdError toast.
- What the user sees: summary toast + full refresh, or mapped error toast.
- Delta: none (BL-90 latent ruleset-`.md`-survives-rollback is a backend concern, already tracked). Action: none.

### PR-12 — Archive project
- Verdict: ✅ OK
- Traced: `project-archive` `:489-503` (rendered only when `!isArchived`) → `archiveProject=guard(handleArchive)` `:300` → `handleArchive` `:280-289` → confirm `strings.project.archiveConfirm` (exact) → `orchdArchiveProject`→`refreshProjects`→`showToast(strings.project.archived="Project archived")`. `disabled={orchdDown||submitting}`.
- Error handling: present — catch → toast; cancelled confirm returns before round-trip.
- What the user sees: confirm dialog, then archived + toast.
- Delta: none. Action: none.

### PR-13 — Un-archive
- Verdict: ✅ OK
- Traced: `project-archived-banner` role=status `:342-374` with `strings.project.archivedBanner` + `project-unarchive` → `unarchiveProject=guard(handleUnarchive)` `:301` → `orchdUnarchiveProject`→`refreshProjects`. `disabled={orchdDown||submitting}`.
- Error handling: present — catch → toast.
- What the user sees: red banner + [Un-archive]; project editable after.
- Delta: minor — success path shows no toast (archive shows one). Catalog PR-13 does not require a success toast; acceptable. Action: none.

### PR-14 — Archived sidebar group
- Verdict: ✅ OK
- Traced: `archived-projects-group` gated `archivedProjects.length>0` `WorkspaceSidebar.tsx:313`; `archived-projects-toggle` `:317-337` (aria-expanded) reveals dimmed (opacity 0.6) `archived-project-${id}` buttons → `openProject`.
- Error handling: n/a (pure nav).
- What the user sees: collapsed "Archived (N)" group; expands to dimmed project list; hidden when none.
- Delta: none. Action: none.

### PR-15 — Add another project
- Verdict: ✅ OK
- Traced: `create-project-open` `WorkspaceSidebar.tsx:376-393` → dialog; picker filters unlinked-only via `linkedWorkspaceIds` complement (`CreateProjectDialog.tsx:222-225`); `orchdCreateProject`.
- Error handling: present — a workspace linked elsewhere between open/submit → server Conflict "conflict:" toast.
- What the user sees: second sidebar group; unlinked-only picker.
- Delta: none. Action: none.

### PR-16 — Unresolvable workspace
- Verdict: ✅ OK
- Traced: `project.workspaceIds.map` `:393-414`; when `workspaces[wsId]` undefined → `project-workspace-unresolved-${wsId}` chip `strings.project.workspaceUnavailable` `:400-402` + [Unlink] still rendered.
- Error handling: n/a (honest soft-ref join).
- What the user sees: "workspace unavailable" chip + working Unlink.
- Delta: none. Action: none.

### PR-17 — Project tabs
- Verdict: ✅ OK
- Traced: `TABS` 7 keys `:31-39`; `activeTab` switch `:340-514` mounts exactly one; `project-panel-loading` `:191-197` when `!project`; `project-counters` `:377-385`.
- Error handling: pure view switch (not orchd-gated).
- What the user sees: one tab body at a time; loading line; Goals/Ideas/Tasks/Insights counters.
- Delta: trivial — code tab order is overview/goals/ideas/tasks/insights/rules/graph (catalog prose lists Tasks before Ideas). Order is not a contract; all 7 present. Action: none.

### PR-18 — Rules content save
- Verdict: ✅ OK
- Traced: `ruleset-content` textarea + `ruleset-save-content` `RulesetPanel.tsx:458-478` (rendered only `fileState!=="missing"`) `disabled={orchdDown}` → `handleSaveContent` `:362-369` → `orchdUpsertRuleset(scope,pid,content,null,null)`→refresh.
- Error handling: present — catch → `showToast(describeOrchdError)`.
- What the user sees: editable textarea; Save disabled while orchd down; textarea/Save hidden when file missing.
- Delta: none. Action: none.

### PR-19 — Policy save
- Verdict: ✅ OK
- Traced: `ruleset-save-policy` `:527-535` `disabled={orchdDown}` → `handleSavePolicy` `:397-410` → `validatePolicy` `:38-59` (inline `ruleset-policy-error`: spend-cap-not-number / negative / empty-entry) → `orchdUpsertRuleset(...,policy)`.
- Error handling: present — client validation blocks doomed round-trip; server Validation still → toast.
- What the user sees: inline validation errors; Save disabled while orchd down.
- Delta: none. Action: none.

### PR-20 — Rules file banners
- Verdict: ✅ OK
- Traced: `ruleset-banner-modified` `:428-441` (`strings.rules.modifiedBanner="file changed externally"` + [Accept]→`handleAcknowledge`→`orchdAcknowledgeRuleFile`); `ruleset-banner-missing` `:443-456` (`strings.rules.missingBanner="file lost"` + [Recreate]→`handleRecreate`→`orchdUpsertRuleset(...,"")`). Both action buttons `disabled={orchdDown}`; `ruleset-reveal` stays live.
- Error handling: present — each handler catch → toast.
- What the user sees: honest banner + recovery action; reveal-file works even offline.
- Delta: none. Action: none.

---

## GO — Goals + metric_refs

| ID | Verdict | Sev | One-liner |
|---|---|---|---|
| GO-01 | ✅ | — | Strategic root pinned (no move/delete); `goal-tree-empty` if none |
| GO-02 | ✅ | — | "+ subgoal" seed "new goal", guard + disabled(orchdDown‖submitting) |
| GO-03 | ✅ | — | Title edit optimistic; revert on fail/blank; toast on reject |
| GO-04 | ✅ | — | Status non-optimistic (renders off store); disabled; toast on fail |
| GO-05 | ✅ | — | ▲/▼ two-move swap; disabled at ends (opacity 0.35) + strategic root |
| GO-06 | ✅ | — | Delete subtree after confirm "delete the entire branch?"; hidden on root |
| GO-07 | ✅ | — | Metric chip add on Enter; dedupe/blank-ignore; input disabled when orchdDown |
| GO-08 | ✅ | — | Metric chip × removes (persisted); disabled |
| GO-09 | ✅ | — | Empty tree → "The goal tree is empty." |
| GO-10 | ✅ | — | Every control `disabled=orchdDown‖submitting`; reads live; banner in ProjectPanel |

### GO-01 — Strategic root present
- Verdict: ✅ OK
- Traced: `goal-tree` role=tree `GoalTree.tsx:553-579`; `buildRows` pins strategic first `:38-63`; strategic row hides move/delete via `!isStrategic` `:316-358`. Empty → `goal-tree-empty` `:545-551` `strings.goals.empty="The goal tree is empty."`.
- Error handling: n/a (read). What the user sees: pinned root, no move/delete controls on it.
- Delta: none. Action: none.

### GO-02 — Add subgoal
- Verdict: ✅ OK
- Traced: "+ subgoal" button `:340-347` `disabled={disabled}` → `addSubgoal=guard(handleAddSubgoal)` `:476` → `orchdCreateGoal(pid,parentId,"additional",NEW_SUBGOAL_TITLE,"")` (`strings.goals.newSubgoal="new goal"`) → refreshGoals.
- Error handling: present — catch → toast; guard blocks double-create (E-08/P-19).
- What the user sees: new "new goal" row.
- Delta: none. Action: none.

### GO-03 — Edit title
- Verdict: ✅ OK
- Traced: `goal-title-input-${id}` `:287-301` → `commit` `:273-282`; blank→`setTitle(goal.title)` silent revert `:276`; on `!ok`→revert `:281`; `handleTitleCommit` `:446-454` returns bool.
- Error handling: present/honest — reject → toast + revert to store value; optimistic local title.
- What the user sees: title persists, reverts on failure, no phantom.
- Delta: none. Action: none.

### GO-04 — Change status
- Verdict: ✅ OK
- Traced: `goal-status-${id}` select `:302-315` value=`goal.status` (non-optimistic, off store) → `handleStatusChange` `:456-462` → `orchdUpdateGoal(id,null,null,status,null)`.
- Error handling: present — catch → toast, no phantom (select reflects store).
- Delta: none. Action: none.

### GO-05 — Reorder ▲/▼
- Verdict: ✅ OK
- Traced: `goal-move-up/down-${id}` `:316-339` `disabled={disabled||!canMove*}`, opacity 0.35 at ends, hidden on strategic; → `swapWithNeighbor` `:517-527` issues two `orchdMoveGoal` (each takes other's ord) + one refresh.
- Error handling: present — single try/catch → one toast. Note: a mid-swap failure (2nd move rejects) leaves server with duplicate `ord` until next refresh — documented backend limitation (no UNIQUE(parent_id,ord)); catalog acknowledges the two-move mitigation. Not a frontend defect.
- What the user sees: swapped siblings; single toast on failure.
- Delta: none. Action: none.

### GO-06 — Delete branch
- Verdict: ✅ OK
- Traced: `goal-delete-${id}` `:348-358` (hidden on strategic) `disabled={disabled}` → `handleDelete` `:498-506` → confirm `strings.goals.deleteConfirm="delete the entire branch?"` → `orchdDeleteGoal`→refreshGoals.
- Error handling: present — catch → toast; cancel returns.
- Delta: none. Action: none.

### GO-07 — Add metric ref
- Verdict: ✅ OK
- Traced: `goal-metric-input-${id}` `:385-399` `disabled={disabled}` Enter→`commitMetric` `:250-257`: `if(disabled)return`, trim, blank-ignore, dedupe, → `changeMetricRefs=guard(handleMetricRefsChange)` `:496` → `orchdUpdateGoal(id,null,null,null,[...refs,new])`.
- Error handling: present — guarded; catch → toast; chips render off `goal.metricRefs` (no phantom).
- What the user sees: chip appears; blank/dupe do nothing; input disabled offline.
- Delta: none. Action: none.

### GO-08 — Remove metric ref
- Verdict: ✅ OK
- Traced: `goal-metric-remove-${id}-${ref}` `:373-382` `disabled={disabled}` → `removeMetric` `:259-264` → guarded `orchdUpdateGoal` with filtered array.
- Error handling: present — catch → toast.
- Delta: none. Action: none.

### GO-09 — Empty tree
- Verdict: ✅ OK
- Traced: `goal-tree-empty` `:545-551`.
- Delta: none. Action: none.

### GO-10 — orchd down gating
- Verdict: ✅ OK
- Traced: `disabled={orchdDown||submitting}` passed to every `GoalRow` `:567`; each control ANDs it with its own condition; reads (tree) live; banner owned by ProjectPanel (`OrchdDownBanner` at `ProjectPanel.tsx:313`).
- Error handling: n/a (gating).
- Delta: none. Action: none.

---

## ID — Ideas

| ID | Verdict | Sev | One-liner |
|---|---|---|---|
| ID-01 | ✅ | — | ⌘K capture: guard, disabled(empty/submitting), toast "idea saved" + close |
| ID-02 | ✅ | — | ⌘K blocked in INPUT/TEXTAREA/`.xterm` via isTypingTarget |
| ID-03 | ✅ | — | ⌘K blocked while a mandatory upgrade dialog open (both daemon pairs) |
| ID-04 | ✅ | — | orchd down: overlay opens, Save blocked, `quick-capture-orchd-down` note; submit early-returns |
| ID-05 | 🟡 | Minor | Create form disabled correctly, but button stays full-opacity while orchdDown (looks enabled) |
| ID-06 | ✅ | — | Row title/body blur → save; revert to store on failure |
| ID-07 | ✅ | — | Lifecycle select (6 values incl. "in development"); disabled; toast on fail |
| ID-08 | ✅ | — | Delete after confirm "delete idea?"; disabled; toast on fail |
| ID-09 | ✅ | — | Orphan link-to-project → setIdeaProject + refresh; button disabled until chosen |
| ID-10 | ✅ | — | Spawn project from idea; cancel no-op; disabled(orchdDown‖submitting); toast |
| ID-11 | ✅ | — | Link-step failure → resume (Retry linking), ids held, no duplicate project |
| ID-12 | ✅ | — | Research dialog + latest-run badge + pane toggle (pure view) |
| ID-13 | ✅ | — | Empty: "No ideas without a project." / "No ideas in this project yet." |
| ID-14 | ✅ | — | Create double-submit guarded (⌘K + inbox); per-row edits use disabled+revert |

### ID-01 — ⌘K capture
- Verdict: ✅ OK
- Traced: `quick-capture-submit` `QuickCapture.tsx:288-296` / Enter `:236-244` → `submit=guard(handleSubmit)` `:211` → `handleSubmit` `:197-207`: guard `trimmed==="" || orchdDown` → `orchdCreateIdea(pid||null,title,body)` → `showToast(strings.capture.ideaSaved="idea saved")` + `close()`. Idea lands in list via `orchd://ideas-changed`→refreshIdeas (App.tsx:202).
- Error handling: present/honest — catch → toast; dialog stays open; draft retained (close only on success).
- What the user sees: "idea saved" toast, overlay closes, idea in inbox/project.
- Delta: none. Action: none.

### ID-02 — ⌘K in typing target
- Verdict: ✅ OK
- Traced: global keydown `:167-183`; `isTypingTarget(document.activeElement)` `:26-31` → true for INPUT/TEXTAREA or `.xterm` ancestor → early return (no open).
- Error handling: n/a. What the user sees: overlay does NOT open while typing.
- Delta: none. Action: none.

### ID-03 — ⌘K during upgrade dialog
- Verdict: ✅ OK
- Traced: `:175-177` reads store live: `daemonIncompatible&&upgradeDialogOpen` or `orchdIncompatible&&orchdUpgradeDialogOpen` → return.
- Delta: none. Action: none.

### ID-04 — orchd down capture
- Verdict: ✅ OK
- Traced: overlay opens; `blocked=orchdDown||empty||submitting` `:215` disables Save; `quick-capture-orchd-down` role=status `:273-277` `strings.errors.unavailable="orchestrator unavailable"`; `handleSubmit` early-returns on `orchdDown` `:199`.
- Error handling: present/honest — never a doomed send.
- What the user sees: overlay + honest disabled-Save note.
- Delta: none. Action: none.

### ID-05 — Ideas list create form
- Verdict: 🟡 UX-GAP — Minor
- Traced: `idea-create-submit` `IdeasList.tsx:490-498` `disabled={orchdDown||createTitle.trim()===""||submitting}` → `submitCreate=guard(handleCreate)` `:468` → `handleCreate` `:453-464` → `orchdCreateIdea(projectId,title,body)`→refreshIdeas. Fields retained on failure.
- Error handling: present/honest — guarded; catch → toast; fields kept.
- What the user sees: functionally correct — button IS disabled while orchd down. BUT the inline `opacity` `:495` is computed from `createTitle.trim()===""||submitting ? 0.5 : 1` and OMITS `orchdDown`, so with orchd down + a non-empty title the button renders at full strength (opacity 1) while being disabled — a disabled control that doesn't look disabled and gives no click feedback. QuickCapture's own button (ID-01/04) folds orchdDown into its opacity via `blocked`; this form is inconsistent with it. The same pattern exists on the orphan attach button (ID-09, `:335` opacity ignores orchdDown).
- Delta from Expected: none functionally (catalog "States: disabled: `orchdDown||empty title||submitting`" is satisfied); the gap is purely the missing dimmed affordance.
- Action: fix-in-Part-B (B4 empty/density/affordance polish) — one-line opacity fold-in of `orchdDown` at `IdeasList.tsx:495` (and `:335`). Not backlog-worthy; no wire/state impact.

### ID-06 — Edit idea title/body
- Verdict: ✅ OK
- Traced: `idea-title-input`/`idea-body-input` `:243-257`,`:304-313` `disabled={disabled}` → `commitTitle` `:219-230` / `commitBody` `:232-236` → `handleTitleCommit`/`handleBodyCommit` `:406-424` (return bool); revert to store on `!ok`.
- Error handling: present/honest — toast + revert (P-27).
- Delta: none. Action: none.

### ID-07 — Lifecycle stage
- Verdict: ✅ OK
- Traced: `idea-lifecycle-${id}` `:258-271` value=`idea.lifecycle` disabled=`disabled` → `handleLifecycleChange` `:426-432` → `orchdSetIdeaLifecycle`. `LIFECYCLE_VALUES` = captured/researching/specced/inDev(label "in development")/shipped/archived.
- Error handling: present — catch → toast.
- Delta: none. Action: none.

### ID-08 — Delete idea
- Verdict: ✅ OK
- Traced: `idea-delete-${id}` `:272-280` disabled=`disabled` → `handleDelete` `:434-442` → confirm `strings.ideas.deleteConfirm="delete idea?"` → `orchdDeleteIdea`→refreshIdeas.
- Error handling: present — catch → toast; cancel returns.
- Delta: none. Action: none.

### ID-09 — Orphan link-to-project
- Verdict: ✅ OK
- Traced: orphan block `:314-340`: `idea-attach-select` (not orchd-gated) + `idea-attach-button-${id}` `disabled={disabled||attachTo===""}` → `handleAttach` `:444-451` → `orchdSetIdeaProject`→refreshIdeas. Empty option `strings.ideas.selectProject="select a project…"`.
- Error handling: present — catch → toast.
- What the user sees: idea moves into project; button disabled until a project is picked.
- Delta: none functionally. (Same cosmetic opacity-vs-orchdDown nuance as ID-05 applies to the attach button; folded into the ID-05 finding.) Action: see ID-05.

### ID-10 — Spawn project from idea
- Verdict: ✅ OK
- Traced: `spawn-project-${id}` `SpawnProjectFromIdea.tsx:145-153` `disabled={orchdDown||submitting}` → `spawn=guard(handleSpawn)` `:138` → chain pickFolder→createWorkspace(upsert)→`orchdCreateProject(idea.title,"",[wsId])`→`orchdSetIdeaProject`→refreshProjects+refreshIdeas→`showToast(strings.ideas.spawn.createdFromIdea="Project created from idea")`. Cancel picker → return `:90`.
- Error handling: present/honest — split messages: picker/workspace failures use lighter copy `:85,:98`, orchd failures use `describeOrchdError`; failures shown inline `spawn-project-error-${id}` + toast.
- What the user sees: new project in sidebar + toast; cancel = nothing.
- Delta: none. Action: none.

### ID-11 — Spawn resume after link failure
- Verdict: ✅ OK
- Traced: `createdWorkspaceId`/`createdProjectId` held in state `:71-72`; steps 1+2 skipped when ids present `:79-118`; link-fail catch `:131-135` sets inline+toast `strings.ideas.spawn.linkFailed(title,reason)` (exact catalog copy) without tearing down project/workspace; `resuming=createdProjectId!==null` `:141` flips button to `strings.ideas.spawn.retry="Retry linking"`.
- Error handling: present/honest — retry resumes at step 3, never a second project.
- What the user sees: honest "…linking failed… Retry to finish — it will not create a second project." + "Retry linking" button.
- Delta: none (BL-95/P-09 fixed). Action: none.

### ID-12 — Research dialog/badge/toggle
- Verdict: ✅ OK
- Traced: `idea-research-${id}` `:281-289` disabled=`disabled` → `setResearchDialogOpen(true)` → `ResearchRunDialog` `:343-345`; `idea-research-badge-${id}` `:290-294` shows `latestRun=researchRuns[0]` status (pending/running/done/failed), omitted when no runs; `idea-research-toggle-${id}` `:295-302` NOT gated, toggles `ResearchPane` `:342` ("hide research"/"research (N)").
- Error handling: research runs eagerly fetched `:399-404`; deeper handling → RE epic.
- Delta: none. Action: none (see RE epic).

### ID-13 — Empty ideas list
- Verdict: ✅ OK
- Traced: `ideas-list-empty` `:501-507`; orphan → `strings.ideas.emptyOrphan="No ideas without a project."`, project → `strings.ideas.emptyProject="No ideas in this project yet."`.
- Delta: none. Action: none.

### ID-14 — Double-submit
- Verdict: ✅ OK
- Traced: create guarded — `submitCreate=guard(handleCreate)` `IdeasList.tsx:468` + `submit=guard(handleSubmit)` `QuickCapture.tsx:211`; `useSubmitGuard` is ref-based (blocks same-tick re-entry). Per-row edits rely on `disabled`+revert, not the guard (IdeaRow `disabled={orchdDown}`).
- Error handling: n/a (guard). What the user sees: exactly one mutation.
- Delta: none. Action: none.

---

## Summary

PR (20) and GO (10) are fully green: every control exists, every handler fires the expected orchd
verb, states render, errors are caught and surfaced honestly (inline role=alert for dialogs, toast
for lists, banners for file/archive state), and `orchdDown` gating + empty states are consistent.
All string copy matches the catalog verbatim (archive confirm, importSummary, linkFailed,
workspaceRequired, empty lines, error map). ID (14) is 13 green + one **Minor UX-GAP (ID-05)**: the
Ideas-list "+ idea" create button (and, identically, the orphan "link to project" button, ID-09) is
correctly `disabled` while orchd is down but its inline `opacity` omits `orchdDown`, so with a typed
title it renders at full strength — a disabled control that doesn't look disabled and yields no
click feedback, inconsistent with QuickCapture's own button. Purely cosmetic; fold `orchdDown` into
the opacity expressions at `IdeasList.tsx:495` and `:335` during Part-B B4 polish. No 🔴 BUGs and no
📄 DOC-GAPs found across the batch; the one documented latent (GO-05 duplicate-`ord` on mid-swap
failure, PR-11 BL-90) are backend concerns already tracked, not frontend defects.


# Audit group 3 — full trace

# UX audit — Batch 3: epics RE / IN / TA / GR

Audited against CURRENT code on `main` (read-only). Catalog: `docs/qa/ux-scenarios.md`
(synced @ 78af949). Verdict legend: ✅ OK · 🟡 UX-GAP · 🔴 BUG · 📄 DOC-GAP.

**Counts:** ✅ 40 · 🟡 5 · 🔴 0 · 📄 1  (total 46: RE 12, IN 13, TA 8, GR 13)

Headline: no reproducible defect (🔴 = 0). One notable DOC-GAP — the catalog's RE-12
still labels MCP connect a "Tier-0 Critical BUG (BL-89, no timeout at any layer)", but the
backend now bounds both the handshake and `list_tools` with `tokio::time::timeout` — the bug
is FIXED and the catalog row is stale. The five 🟡s are all pre-documented, accepted-by-design
gaps (F-06, P-26, H-02, P-08) whose code faithfully matches the catalog's own notes.

---

## RE — Research

| ID | Verdict | Sev | One-liner |
|---|---|---|---|
| RE-01 | 🟡 UX-GAP | Minor | 0 servers → Run correctly disabled, but no CTA back to Extensions (F-06). |
| RE-02 | ✅ OK | — | server→tool→args cascade; tool select gated on server; args seeded. |
| RE-03 | ✅ OK | — | invalid-JSON args → inline `role="alert"`, no wire call. |
| RE-04 | ✅ OK | — | preflight shows effective policy (server>project>global) + cost note. |
| RE-05 | ✅ OK | — | Run → toast + close; badge advances via push + 2s self-poll. |
| RE-06 | ✅ OK | — | `useSubmitGuard` blocks a double Run (double-spend). |
| RE-07 | ✅ OK | — | failed run shows raw errorKind (snake_case token, F-09 by design). |
| RE-08 | ✅ OK | — | "show artifact" → ArtifactViewer + unconditional unverified banner; read not gated. |
| RE-09 | ✅ OK | — | self-poll every 2000 ms while non-terminal heals lost push / boot-reconcile. |
| RE-10 | ✅ OK | — | Run/form-insight gated when down; read affordances live; poll skipped when down. |
| RE-11 | ✅ OK | — | over-cap run fails honestly; kind shown inline; preflight showed the limit. |
| RE-12 | 📄 DOC-GAP | — | Catalog claims "no timeout" BUG; code HAS the timeout — catalog stale. |

### RE-01 — Run dialog, 0 connected servers
- Verdict: 🟡 UX-GAP (Minor)
- Traced: `ResearchRunDialog.tsx:207` `connectedServers = mcpServers.filter(enabled && protocolVersion!==null)` → `:270` server `<option>`s → `:244` `submitBlocked = orchdDown || serverId==="" || toolName==="" || submitting` → `:351` Run `disabled`.
- Error handling: n/a (no wire call reachable).
- What the user sees: an empty server `<select>` (only the "select a server…" placeholder), Run permanently disabled. Cancel is available (not a hard dead end), but there is no link/button to open Extensions to connect a server.
- Delta: none vs the Expected — the catalog itself documents the "NO CTA back to Extensions (F-06 gap)". Code matches the documented gap.
- Action: fix-in-Part-B (EmptyState with an "Open Extensions" action) — tracked under F-06.

### RE-02 — pick server → tool → args
- Verdict: ✅ OK
- Traced: `ResearchRunDialog.tsx:210` `handleServerChange` resets toolName → `:202-205` effect fetches `refreshMcpTools(serverId)` when uncached → `:284` tool `<select disabled={serverId===""}>` → `:185` `argsDraft` seeded via `seedArgs(idea)` (`:131`).
- Error handling: tool-list fetch failures surface via the store's `refreshMcpTools` toast path.
- What the user sees: tool select enabled once a server is chosen; args textarea pre-filled with `{query,[context]}` JSON.
- Delta: none.
- Action: none.

### RE-03 — invalid JSON args
- Verdict: ✅ OK
- Traced: `ResearchRunDialog.tsx:217-223` `JSON.parse(raw)` inside try/catch → sets `argsError` and `return` BEFORE `researchStartRun`; `:310` renders `research-run-args-error` `role="alert"`.
- Error handling: present, honest, pre-wire.
- What the user sees: "arguments must be valid JSON" inline; no run started.
- Delta: none.
- Action: none.

### RE-04 — preflight panel
- Verdict: ✅ OK
- Traced: `ResearchRunDialog.tsx:140` `effectivePolicy(policies, serverId, idea.projectId)` (server>project>global) → `:316-331` renders scope/spend-cap/rate + cost note; `null` → "not set".
- Error handling: n/a (pure read of `policies` refreshed on mount, `:192`).
- What the user sees: honest effective-policy rows + "cost is usually unknown in advance" note.
- Delta: none.
- Action: none.

### RE-05 — Run (happy path)
- Verdict: ✅ OK
- Traced: `ResearchRunDialog.tsx:227` `researchStartRun` → `:228` `refreshResearchRuns` → `:229` toast → `:230` `onClose`. Badge advances via `App.tsx:242` `onOrchdResearchRunsChanged` push + `ResearchPane.tsx:134-140` self-poll.
- Error handling: `:231-235` catch → in-dialog `research-run-error` `role="alert"` + toast, dialog stays open.
- What the user sees: "Research run started", dialog closes, badge pending→running→done.
- Delta: none.
- Action: none.

### RE-06 — double-click Run
- Verdict: ✅ OK
- Traced: `ResearchRunDialog.tsx:241` `submit = guard(handleSubmit)`; `submitBlocked` includes `submitting` (`:244`).
- Error handling: n/a.
- What the user sees: exactly one run / one external call.
- Delta: none.
- Action: none.

### RE-07 — open a failed run
- Verdict: ✅ OK
- Traced: `ResearchPane.tsx:230-234` renders `research-run-error-kind-{id}` = `run.errorKind ?? strings.research.unknownError`.
- Error handling: honest; raw snake_case kind (no localization) — documented F-09.
- What the user sees: e.g. `policy_cap_exceeded` / `timeout` / "unknown error".
- Delta: none (F-09 is a known cosmetic gap, not scored here).
- Action: none.

### RE-08 — done run → show artifact
- Verdict: ✅ OK
- Traced: `ResearchPane.tsx:159-162` `handleShowArtifact` → `ensureArtifact` → `mcpGetArtifact` (`:150`) → `:236-242` `ArtifactViewer defaultOpen`. Not orchd-gated (plain read).
- Error handling: `:153-156` catch → `showToast(describeOrchdError(e))`, screen unchanged.
- What the user sees: artifact content + unconditional "⚠ unverified data" banner (from reused `ArtifactViewer`).
- Delta: none. (Failed-run "form insight without research" opens `artifact:null`, indistinguishable in the dialog from IN-02 — documented P-10, benign.)
- Action: none.

### RE-09 — running run, push lost / boot-reconcile
- Verdict: ✅ OK
- Traced: `ResearchPane.tsx:133` `hasNonTerminal` → `:134-140` `setInterval(refreshResearchRuns, 2000)` while `!disabled && hasNonTerminal`, cleared on unmount / when all terminal.
- Error handling: `refreshResearchRuns` swallows to toast; poll skipped while `disabled`(orchdDown) to avoid spam.
- What the user sees: a stuck run self-updates within ~2 s; a boot-reconciled run shows `failed{interrupted}`.
- Delta: none — fixes BL-92/P-24 as claimed.
- Action: none.

### RE-10 — orchd down, whole flow
- Verdict: ✅ OK
- Traced: `ResearchRunDialog.tsx:244` `submitBlocked` includes `orchdDown`; `ResearchPane.tsx:208,221` form-insight buttons `disabled={disabled}`; `:135` poll early-return when `disabled`; "show artifact" (`:200`) stays enabled (read).
- Error handling: honest — reads degrade to the same toast, mutations blocked.
- What the user sees: Run + form-insight disabled; artifact view + research toggle usable.
- Delta: none.
- Action: none.

### RE-11 — policy cap exceeded
- Verdict: ✅ OK
- Traced: preflight limit shown (`ResearchRunDialog.tsx:321`); server rejects the run → `ResearchPane.tsx:230-232` shows `failed{policy_cap_exceeded}`.
- Error handling: honest kind surfaced.
- What the user sees: the run fails with the policy kind; the preflight had shown the cap.
- Delta: none (the "why-failed ↔ limit" link is inferable but not spelled out — catalog's own note, not scored).
- Action: none.

### RE-12 — dead / hanging endpoint (connect)
- Verdict: 📄 DOC-GAP (catalog-fix)
- Traced: catalog claims `mcp/lifecycle.rs` has "no `.timeout()` at any layer" → connect hangs forever (BL-89, Tier-0 Critical). CURRENT code `crates/orchd/src/mcp/lifecycle.rs:74` `let timeout = Duration::from_millis(server.timeout_ms…)`, `:77-81` wraps `connect_fn(...)` in `tokio::time::timeout` → `McpError::Timeout` on elapse, `:83-87` wraps `session.list_tools()` the same way. The in-code comment at `:67-73` explicitly cites "BL-89, spec D5" as fixed. Client: `ConnectDialog.tsx:112` `await mcpConnect` with `busy` (`:95`) now resolves/ rejects within `timeout_ms`, so the busy state cannot hang indefinitely.
- Error handling: present at the connect layer (bounded handshake + list_tools).
- What the user sees: a dead peer now surfaces a Timeout error toast after `timeout_ms`, not a permanent hang.
- Delta: the catalog is STALE — it describes a Critical open bug that the code has already closed.
- Action: catalog-fix — rewrite RE-12 to reflect the bounded-connect behavior; drop the "Tier-0 Critical / only an orchd restart recovers" language and the BL-89 open-bug reference. (If BL-89 is still open in `docs/backlog.md`, close it.)

---

## IN — Insights

| ID | Verdict | Sev | One-liner |
|---|---|---|---|
| IN-01 | ✅ OK | — | done-run form: prefilled body + goals/metrics/graph fit-context, honestly empty labels. |
| IN-02 | ✅ OK | — | failed-run path opens same dialog with `artifact:null`, empty body, source kept. |
| IN-03 | ✅ OK | — | Create → insight+verdict; inputs lock; gated on down/empty/exists/submitting. |
| IN-04 | ✅ OK | — | Accept → status accepted (quiet, no toast); enabled only while status new. |
| IN-05 | ✅ OK | — | To backlog → task + idea→specced + close; shown only when accepted. |
| IN-06 | ✅ OK | — | flip-fails-after-create → resume message; `createdTaskId` prevents dup task. |
| IN-07 | 🟡 UX-GAP | Minor | Cancel after Create does not roll back the created insight (P-26, by design). |
| IN-08 | ✅ OK | — | orphan idea → To-backlog disabled (`idea.projectId===null`). |
| IN-09 | ✅ OK | — | status select updates; gated when down; archived not fired from select. |
| IN-10 | ✅ OK | — | archive requires a reason (client block, no round-trip). |
| IN-11 | ✅ OK | — | owner fit-verdict + reasoning → apply; only Apply is gated. |
| IN-12 | ✅ OK | — | graph node ingested on accept server-side (warn-only); frontend correct. |
| IN-13 | ✅ OK | — | honest empty line; no create form (push-driven). |

### IN-01 — done run → Form insight
- Verdict: ✅ OK
- Traced: `FormInsightDialog.tsx:212` body = `artifact.contentText ?? artifact.contentJson`; `:235-240` refreshes goals+graph; `:242-248` derive `goals`/`ideaNode`; `:256` `orchdGraphNeighborhood`; `:431-467` render context with honest empties (`noGoals` / `noGraphNode` / `noRelatedNodes`).
- Error handling: neighborhood fetch guarded by a `cancelled` flag; goals/graph via store toasts.
- What the user sees: title = idea title, body prefilled + editable, "Assessment context" with goals (+metrics) and related graph nodes or honest empties.
- Delta: none.
- Action: none.

### IN-02 — failed run → form insight without research
- Verdict: ✅ OK
- Traced: `ResearchPane.tsx:169` `handleFormInsightWithoutResearch` → `setOpenInsight({runId, artifact:null})`; `FormInsightDialog.tsx:212` body = "" when `artifact===null`; source still `research-run:{runId}` (`:269`).
- Error handling: n/a until Create.
- What the user sees: same dialog, empty body, honest not-broken.
- Delta: none.
- Action: none.

### IN-03 — Create
- Verdict: ✅ OK
- Traced: `FormInsightDialog.tsx:265-280` `handleCreate` → `orchdCreateInsight` then `orchdSetInsightFitVerdict`; `:343` `createBlocked = orchdDown || title empty || insight!==null || submitting`; inputs `disabled={insight!==null}` (`:374,386,398,417`).
- Error handling: `:275-279` catch → `form-insight-error` `role="alert"` + toast, dialog open.
- What the user sees: "Insight created", fields lock.
- Delta: none.
- Action: none.

### IN-04 — Accept
- Verdict: ✅ OK
- Traced: `FormInsightDialog.tsx:282-294` `handleAccept` → `orchdSetInsightStatus(accepted)` (no toast); button rendered `{insight!==null}` (`:498`), `acceptBlocked` requires `insight.status==="new"` (`:344`).
- Error handling: `:289-293` catch → inline + toast.
- What the user sees: status → accepted. (Minor: the Accept button stays visible-but-disabled after accept, since it renders on `insight!==null`; the catalog phrasing "shown only when new" is looser than the code, but disabled-logic is correct — cosmetic, not a defect.)
- Delta: none material.
- Action: none.

### IN-05 — To backlog
- Verdict: ✅ OK
- Traced: `FormInsightDialog.tsx:296-333` `handleBacklog` → `orchdCreateTask(source insight)` → `orchdSetIdeaLifecycle(specced)` → refresh + toast + close; button rendered only `insight.status==="accepted"` (`:509`).
- Error handling: try/catch with resume messaging (see IN-06).
- What the user sees: "Task added to backlog", idea flips to specced, dialog closes.
- Delta: none.
- Action: none.

### IN-06 — lifecycle flip fails after task create
- Verdict: ✅ OK
- Traced: `FormInsightDialog.tsx:302-317` holds `createdTaskId`; on retry `taskId!==null` skips `orchdCreateTask`; `:329` `backlogResume(reason)` message.
- Error handling: honest "task created, moving to specced failed… Retry — will not create a duplicate task."
- What the user sees: resume path, no duplicate task.
- Delta: none — BL-95/G-08 fixed as claimed.
- Action: none.

### IN-07 — Cancel after Create
- Verdict: 🟡 UX-GAP (Minor)
- Traced: `FormInsightDialog.tsx:481-487` Cancel → `onClose()` only; no compensating delete of the created (possibly accepted) insight.
- Error handling: n/a.
- What the user sees: the insight persists in the Insights tab even though the dialog was "cancelled".
- Delta: none vs the Expected — catalog documents this as P-26 ("Cancel does not compensate the create").
- Action: none / tracked as P-26 (accepted trade-off; a rollback-on-cancel would need a compensating delete).

### IN-08 — orphan idea → To backlog
- Verdict: ✅ OK
- Traced: `FormInsightDialog.tsx:345-350` `backlogBlocked` includes `idea.projectId===null`.
- What the user sees: To-backlog disabled for an unlinked idea.
- Delta: none.
- Action: none.

### IN-09 — status change
- Verdict: ✅ OK
- Traced: `InsightsList.tsx:170-181` `handleStatusChange` (archived NOT fired from select) → `:334-344` `handleStatusApply` → `orchdSetInsightStatus`; select `disabled={disabled}` (`:204`, `disabled=orchdDown`).
- Error handling: `:341-343` catch → toast.
- What the user sees: new/accepted apply immediately; archived defers to the reason field.
- Delta: none.
- Action: none.

### IN-10 — archive requires a reason
- Verdict: ✅ OK
- Traced: `InsightsList.tsx:183-191` `handleArchiveConfirm` blocks on blank reasoning (`archiveError`), no round-trip; `:221-249` inline reason field + `insight-archive-confirm` + `insight-archive-error`.
- Error handling: client block honest; server rejection → toast.
- What the user sees: "an archive reason is required" inline until a reason is typed.
- Delta: none.
- Action: none.

### IN-11 — owner fit-verdict
- Verdict: ✅ OK
- Traced: `InsightsList.tsx:251-285` verdict select + reasoning input (not gated) + Apply `disabled={disabled}` → `:322-332` `handleVerdictApply` → `orchdSetInsightFitVerdict`.
- Error handling: `:329-331` catch → toast.
- What the user sees: verdict (fit/no fit/unclear/— no verdict —) applied; reasoning optional.
- Delta: none.
- Action: none.

### IN-12 — accepted insight → graph node
- Verdict: ✅ OK
- Traced: graph ingest happens server-side on accept (spec D9, warn-only); frontend reconciles via `App.tsx:214` `onOrchdGraphChanged → refreshGraph`. Not a frontend control; nothing to gate.
- Error handling: server warn-only → node silently absent on ingest failure (matches catalog edge).
- What the user sees: an `entity_ref(insight)` node appears on the Graph tab after accept.
- Delta: none observable in the webview (backend behavior; not frontend-traceable beyond the refresh wire).
- Action: none.

### IN-13 — empty insights list
- Verdict: ✅ OK
- Traced: `InsightsList.tsx:348-354` `insights-list-empty` = orphan vs project copy; no create form (`:290-308` doc: push-driven).
- What the user sees: honest empty line.
- Delta: none.
- Action: none.

---

## TA — Tasks

| ID | Verdict | Sev | One-liner |
|---|---|---|---|
| TA-01 | ✅ OK | — | create form → `orchdCreateTask` + refresh; gated on down/empty; guarded. |
| TA-02 | ✅ OK | — | status any→any via `orchdSetTaskStatus`; no state machine. |
| TA-03 | ✅ OK | — | ▲/▼ fractional-rank `orchdSetTaskRank`; ends disabled; LWW race documented. |
| TA-04 | ✅ OK | — | delete → cascade-count confirm → `orchdDeleteTask` + refresh. |
| TA-05 | ✅ OK | — | parent=self/descendant → server Invariant; no client cycle guard (by design). |
| TA-06 | 🟡 UX-GAP | Minor | a task's source/provenance is not rendered on the row at all (H-02). |
| TA-07 | ✅ OK | — | six locked-order groups always render (empty → "no tasks"). |
| TA-08 | ✅ OK | — | orchd-down disables status/▲/▼/Delete/create; reads live. |

### TA-01 — create task
- Verdict: ✅ OK
- Traced: `TasksList.tsx:387-406` `handleCreate` (title-trim guard, tag split) → `orchdCreateTask(projectId,parent,title,body,null,source,null,tags)` → clears fields → `refreshTasks`; `:410` `submitCreate = guard(handleCreate)`; `:470` submit `disabled={orchdDown || title empty || submitting}`.
- Error handling: `:403-405` catch → toast, fields kept.
- What the user sees: a new row in the target status group (backlog by default).
- Delta: none.
- Action: none.

### TA-02 — status change
- Verdict: ✅ OK
- Traced: `TaskRow` `:238-251` status select → `:332-338` `handleStatusChange` → `orchdSetTaskStatus`; `disabled={disabled}` (orchdDown).
- Error handling: `:335-337` catch → toast.
- What the user sees: backlog→done in one click (intentional, no state machine).
- Delta: none.
- Action: none.

### TA-03 — rank reorder
- Verdict: ✅ OK
- Traced: `TasksList.tsx:355-374` midpoint / `±RANK_GAP`; single `orchdSetTaskRank` per move; `:497-498` `canMoveUp/Down` disable at group ends (opacity 0.35). Structural refresh via `onOrchdTasksChanged` push.
- Error handling: `:340-346` `applyRank` catch → toast.
- What the user sees: row moves within its group; two concurrent reorders race LWW (documented).
- Delta: none.
- Action: none.

### TA-04 — delete with subtasks
- Verdict: ✅ OK
- Traced: `TasksList.tsx:376-385` `countDescendants` (recursive) → `deleteConfirmText` → `orchdDeleteTask` → `refreshTasks` in the same try.
- Error handling: `:382-384` catch → toast.
- What the user sees: "delete task? will delete N subtasks" confirm, then cascade removal.
- Delta: none.
- Action: none.

### TA-05 — cycle parent
- Verdict: ✅ OK
- Traced: no client cycle guard; `orchdCreateTask` relies on server Invariant → "invalid operation: {msg}".
- Error handling: server error → toast (create catch).
- What the user sees: rejection toast; message may embed a raw UUID (O-2, catalog note).
- Delta: none (by design).
- Action: none.

### TA-06 — task created from an insight
- Verdict: 🟡 UX-GAP (Minor)
- Traced: `TaskRow` render (`TasksList.tsx:233-282`) shows ONLY title, status select, ▲/▼, Delete. `source`/`sourceId`/tags/body are never rendered on an existing row; the source `<select>` is in the CREATE form only (`:432-444`). The `TaskRow` doc comment at `:225-227` says "Title/tags/source are read-only here", but tags/source are in fact not painted at all.
- Error handling: n/a.
- What the user sees: for a task created from an insight, no on-row indication of its `source=insight` or a link to the originating insight.
- Delta: catalog's "not surfaced prominently in the row (H-02 gap)" understates it — it is not surfaced at all. Behavior otherwise matches the documented gap.
- Action: fix-in-Part-B (surface a source chip / provenance link on the row) — tracked under H-02. Consider also correcting the misleading `TaskRow` doc comment.

### TA-07 — empty groups
- Verdict: ✅ OK
- Traced: `TasksList.tsx:478-491` maps `STATUS_VALUES` (locked order) → each group renders header + `task-empty-group-{status}` "no tasks" when empty; mount-fetch `:315-321`.
- What the user sees: all six groups always present.
- Delta: none.
- Action: none.

### TA-08 — orchd down
- Verdict: ✅ OK
- Traced: per-row `disabled={orchdDown}` (`:499`); create submit adds `orchdDown` (`:470`).
- What the user sees: mutations disabled, rows/groups still readable.
- Delta: none.
- Action: none.

---

## GR — Graph

| ID | Verdict | Sev | One-liner |
|---|---|---|---|
| GR-01 | ✅ OK | — | add-node form (title+body+kind) → `orchdGraphAddNode` + refresh; gated. |
| GR-02 | 🟡 UX-GAP | Minor | edge add is optimistic; on failure the edge is NOT rolled back (P-08). |
| GR-03 | ✅ OK | — | debounced move; flush reads live orchdDown and drops silently, self-heals. |
| GR-04 | ✅ OK | — | double-click rename bar → `orchdGraphUpdateNode`; local nodes only. |
| GR-05 | ✅ OK | — | single-edge select → kind `<select>` → `orchdGraphUpdateEdge`. |
| GR-06 | ✅ OK | — | delete-selected per-id + `finally` refresh; confirm-gated (no submit guard). |
| GR-07 | ✅ OK | — | search highlights `matchIds`; stays live while orchdDown (read). |
| GR-08 | ✅ OK | — | external ghost click → `openProject(foreign projectId)`. |
| GR-09 | ✅ OK | — | local entity_ref click → honest no-op (by design). |
| GR-10 | ✅ OK | — | orphaned entity_ref renders "source removed". |
| GR-11 | ✅ OK | — | zero nodes → "empty" overlay. |
| GR-12 | 🟡 UX-GAP | Minor | self-loop/dup edge rejected server-side; optimistic edge lingers (P-08). |
| GR-13 | ✅ OK | — | orchd-down gating; search + local drag live; flush/edge-add early-return. |

### GR-01 — add node
- Verdict: ✅ OK
- Traced: `GraphCanvas.tsx:528-543` `handleAddNode` (title-trim + fresh-orchdDown guard) → `nextNewNodePosition(nodes.length)` → `orchdGraphAddNode(projectId,addKind,title,body,posX,posY)` → clear + `refreshGraph`; form `:624-667`, button `disabled={orchdDown||submitting||addTitleEmpty}`.
- Error handling: `:540-542` catch → toast.
- What the user sees: node placed on the grid with typed title/body/kind (hardcoded "New node" gone — P-22 fixed).
- Delta: none.
- Action: none.

### GR-02 — drag-connect an edge
- Verdict: 🟡 UX-GAP (Minor)
- Traced: `GraphCanvas.tsx:494-503` `onConnect` → `addEdge` into local state (optimistic), then `orchdGraphAddEdge(...,"relates","")` with `.catch(showToast)`. On rejection there is no local-edge removal; reconciliation waits for the next `orchd://graph-changed` push (`App.tsx:214`).
- Error handling: failure → toast, but the optimistic edge is NOT removed.
- What the user sees: on a failed add, the edge lingers on the canvas (visually implying success) until the next push refreshes the view.
- Delta: none vs Expected — catalog documents this as P-08.
- Action: none / P-08 (a rollback-on-reject would be the fix; currently accepted since the push reconciles).

### GR-03 — drag to reposition
- Verdict: ✅ OK
- Traced: `GraphCanvas.tsx:474-488` `onNodesChange` buffers position changes, debounced `flushMoves` (`:459-472`); `dedupeMovesById` → one `orchdGraphMoveNode` per node; `:466` reads `useAppStore.getState().orchdDown` FRESH and early-returns.
- Error handling: `:468` per-move `.catch` → toast; a down-at-flush drop self-heals on next refresh (P-07, documented).
- What the user sees: a settled drag persists once; while down, buffered moves are dropped silently and heal on refresh.
- Delta: none.
- Action: none.

### GR-04 — rename node
- Verdict: ✅ OK
- Traced: `onNodeDoubleClick` `:521-526` opens rename bar for a LOCAL non-entityRef node; `:545-559` `handleRenameCommit` (blank guard + fresh-orchdDown guard) → `orchdGraphUpdateNode` → `refreshGraph`; bar `:670-708`, input/Save `disabled={orchdDown||submitting}`, Save also on `renameValueEmpty`; Cancel always available.
- Error handling: `:556-558` catch → toast.
- What the user sees: rename bar on double-click; Save persists; Esc/Cancel aborts.
- Delta: none (P-22 fixed).
- Action: none.

### GR-05 — change edge kind
- Verdict: ✅ OK
- Traced: `:585-586` exactly-one-edge selection reveals the editor (`:721-738`); `:566-574` `handleEdgeKindChange` → `orchdGraphUpdateEdge(id,kind)` → `refreshGraph`; `disabled={orchdDown||submitting}`.
- Error handling: `:571-573` catch → toast.
- What the user sees: the edge's rendered label (its kind) updates; no free-text label editor (kind only).
- Delta: none.
- Action: none.

### GR-06 — delete selection
- Verdict: ✅ OK
- Traced: `:588-605` `handleDeleteSelected` → confirm → per-id `orchdGraphDeleteNode`/`orchdGraphDeleteEdge` → `refreshGraph` in `finally`; button `disabled={orchdDown}` (`:714`).
- Error handling: `:596-597` catch → toast; `finally` refresh reconciles even on a partial mid-loop failure.
- What the user sees: selection deleted after confirm; a partial failure still reconciles to server truth.
- Delta: none. (Minor: no `submitting` guard, so a rapid second click could start a second delete loop, but the synchronous `window.confirm` and the finally-refresh make this benign — not scored.)
- Action: none.

### GR-07 — search
- Verdict: ✅ OK
- Traced: `:418-447` debounced `orchdGraphSearch(q,projectId)` with a monotonic `searchRequestIdRef` stale-guard → `setMatchIds`; `displayNodes` (`:607-614`) sets `isMatch`; input has no orchdDown gate (read).
- Error handling: `:439-442` catch (stale-guarded) → toast.
- What the user sees: matching nodes get an accent ring; works even while orchdDown.
- Delta: none.
- Action: none.

### GR-08 — ghost node click
- Verdict: ✅ OK
- Traced: `:505-515` `onNodeClick` → `if (data.isExternal) openProject(data.projectId)`.
- What the user sees: navigation into the foreign project.
- Delta: none.
- Action: none.

### GR-09 — local entity_ref click
- Verdict: ✅ OK
- Traced: `:505-515` falls through (no branch) for a local node → honest no-op (documented: no deep-link seam yet).
- What the user sees: nothing (intentional).
- Delta: none.
- Action: none.

### GR-10 — orphaned entity_ref
- Verdict: ✅ OK
- Traced: `EntityRefNode` `:294-303` renders `data.isOrphan ? strings.graph.sourceRemoved : data.label`.
- What the user sees: "source removed" instead of a stale label.
- Delta: none.
- Action: none.

### GR-11 — empty graph
- Verdict: ✅ OK
- Traced: `:616` `isEmpty = displayNodes.length===0` → `:764-768` `graph-empty-state` overlay "empty".
- What the user sees: honest empty overlay (no spinner; relies on refresh).
- Delta: none.
- Action: none.

### GR-12 — self-loop / duplicate edge
- Verdict: 🟡 UX-GAP (Minor)
- Traced: same `onConnect` path as GR-02; server rejects self-loop (Invariant) / dup (Conflict) → `.catch` toast, but the optimistic edge is not removed.
- Error handling: honest toast; optimistic edge lingers until the next push.
- What the user sees: a rejected self-loop/dup edge briefly remains on the canvas.
- Delta: none vs Expected (same P-08 as GR-02).
- Action: none / P-08.

### GR-13 — orchd down, graph edits
- Verdict: ✅ OK
- Traced: add title/body/kind + Add `disabled={orchdDown||submitting}` (`:630-661`); rename bar `disabled` (`:676,693`); edge-kind `disabled` (`:728`); delete `disabled={orchdDown}` (`:714`); `onConnect`/`flushMoves` read live orchdDown and early-return (`:496,466`); search + local drag stay live.
- What the user sees: mutations gated; reads and local drag still work.
- Delta: none.
- Action: none.

---

## Notes for the results file / backlog

- **RE-12 catalog-fix (priority):** the catalog's most severe RE claim (Tier-0 Critical, BL-89)
  is obsolete — the connect timeout exists in `crates/orchd/src/mcp/lifecycle.rs:74-87`. Update
  the row and reconcile `docs/backlog.md` (close BL-89 if still open).
- **TA-06 doc nuance:** `TaskRow`'s doc comment (`TasksList.tsx:225-227`) claims source/tags are
  "read-only here" implying they render; they do not. Either surface them (H-02) or fix the comment.
- The five 🟡s (RE-01/F-06, IN-07/P-26, TA-06/H-02, GR-02+GR-12/P-08) are all pre-tracked,
  accepted-by-design gaps; none is a regression. No 🔴 in this batch.


# Audit group 4 — full trace

# UX audit — Batch 4: EX (Extensions/MCP/connectors), DG (Degradation), XC (Cross-cutting)

**Audited against:** HEAD `501bbe3` (catalog `docs/qa/ux-scenarios.md` synced @ `78af949` — code is AHEAD of catalog).
**Auditor mode:** read-only trace (control exists → handler → ipc verb → store state → render → error path). Adversarial-but-fair: 🔴 only for a reproducible defect pointed at in code.

## Headline

- **The single Critical the catalog carries (EX-04 / RE-12 / BL-89 "connect hangs forever") is FIXED in current code.** `crates/orchd/src/mcp/lifecycle.rs:67-87` now wraps BOTH the connect handshake and `list_tools` in `tokio::time::timeout(server.timeout_ms)` (default **30 000 ms**, `persistence.rs:436` + `socket_server.rs:1488` `unwrap_or(30_000)`), and the webview transport has its own 30 s `REQUEST_TIMEOUT` (`orchd_client.rs:61,429`). A dead/hanging MCP endpoint now degrades to a bounded busy wait → `McpError::Timeout` → in-dialog error + toast, dialog stays open. It no longer wedges the shared orchd connection. Catalog rows EX-04 and RE-12 are **stale (📄)**.
- **No new reproducible 🔴 found** in EX/DG/XC.
- Theme toggle (XC-03) now EXISTS and is wired (sidebar footer, persisted, `data-theme`), but the app chrome still consumes the old static dark palette (`src/theme.ts`) — light/dark not yet applied app-wide (Part B B3 pending).

---

## EX — Extensions

| ID | Verdict | Sev | One-liner |
|---|---|---|---|
| EX-01 | ✅ OK | — | Six tabs, one mounts at a time, OrchdDownBanner above the tab bar |
| EX-02 | ✅ OK | — | Add-server http-only, soon-options disabled, gated, empty-blocked, failure toast |
| EX-03 | ✅ OK | — | Consent-then-connect, in-dialog error, dialog stays open |
| EX-04 | 📄 DOC-GAP | — | BL-89 FIXED: connect+list_tools now timeout-bounded (30 s default); catalog stale |
| EX-05 | ✅ OK | — | Every connect routes through ConnectDialog (idempotent re-consent) |
| EX-06 | ✅ OK | — | Set-bearer masked, cleared, toast; no persistent indicator (P-25, accepted) |
| EX-07 | ✅ OK | — | Tool toggle non-optimistic + inline error + toast; gated; unguarded (XC-02) |
| EX-08 | ✅ OK | — | Tool invoke + unconditional unverified banner; gated on down/!enabled |
| EX-09 | ✅ OK | — | Invalid JSON → inline error, no wire call |
| EX-10 | ✅ OK | — | Consent/Policy denial toast + recovery hint (text-only nav, P-20 accepted) |
| EX-11 | ✅ OK | — | Add API key masked/cleared, gated, guarded |
| EX-12 | ✅ OK | — | Begin OAuth + best-effort openUrl; honest empty-providers state |
| EX-13 | ✅ OK | — | Complete OAuth, toast; finish gated, code field NOT gated (P-06) |
| EX-14 | ✅ OK | — | Connector op invoke + unverified; ops-load-failed + Retry; unguarded (XC-02) |
| EX-15 | ✅ OK | — | Delete server/account/skill with confirm, gated; unguarded |
| EX-16 | ✅ OK | — | Skill add via native picker (describeCommandError), registry banner, filestate badge |
| EX-17 | ✅ OK | — | Set policy; refId disabled for global; non-number toast pre-wire; gated+guarded |
| EX-18 | ✅ OK | — | Audit table refreshes on mount only (no audit-changed event) — matches P-23 |
| EX-19 | ✅ OK | — | Calls table read-only, live via mcp-invocation-logged; honest empties |
| EX-20 | ✅ OK | — | Artifacts show-content, unverified off isUntrusted, read-only |
| EX-21 | ✅ OK | — | Disconnect direct (no consent gate), gated; unguarded |

### EX-01 — Extensions tabs
- Verdict: ✅ OK
- Traced: `ExtPanel.tsx:88-118` tablist → `setActiveTab` → conditional mount of ServersTab/ToolsBrowser/ConnectorsTab/InvocationLog/ArtifactsTab/SkillsTab. `{orchdDown && <OrchdDownBanner/>}` at `:86`, above the tab bar. `useEffect` mount-fetch `refreshMcpServers()` (`:75-78`).
- Error handling: n/a (tab switch is pure). Banner is presentational.
- What the user sees: 6 tabs, one panel at a time; red "Orchestrator unavailable" bar above tabs when down.
- Delta: none.
- Action: none.

### EX-02 — Add MCP server
- Verdict: ✅ OK
- Traced: `ServersTab.tsx:149-177` `handleAdd`→`mcpAddServer(name,"http",url,…null…,scope,null,authKind,null,null)` (`ipc/orchd.ts:488`). transport `<select disabled value="http">` with `stdio (soon)` option (`:232-241`); scope `project (soon)` disabled (`:258`); auth `OAuth (soon)` disabled (`:271`). Submit `disabled={orchdDown||addBlocked||submitting}` (`:278`), `addBlocked = name==""||url==""`. On success form reset + `refreshMcpServers`; on failure `showToast(describeOrchdError(e))`.
- Error handling: present, honest toast; fields kept.
- What the user sees: new server row; empty-field submit blocked; failure toast.
- Delta: none (stdio overclaim in some docs = F-3, doc-level, not this UI).
- Action: none.

### EX-03 — Connect via ConnectDialog
- Verdict: ✅ OK
- Traced: `ServersTab.tsx:322-330` connect button (gated `orchdDown`) → `setConnectTarget` → `ConnectDialog.tsx`. `handleConfirm` (`:107-122`): `trustGrantConsent(id,"connect")` → `mcpConnect(id)` → `refreshMcpServers` → `onClose`. Failure → `setError`+`showToast`, dialog stays open (`role="alert" connect-dialog-error`). Consent copy `strings.ext.connectDialog.body`.
- Error handling: present; in-dialog inline + toast; survives toast clobber.
- What the user sees: endpoint URL + consent copy; on failure a red inline error and the dialog remains for retry.
- Delta: consent grant not written to audit_log (B-03) — daemon-side, accepted; not a webview defect.
- Action: none.

### EX-04 — Dead / hanging endpoint on Connect
- Verdict: 📄 DOC-GAP (was catalog 🔴 Critical BL-89 — now fixed)
- Traced: webview `mcpConnect(id)` (`ipc/orchd.ts:563`) → `mcp_connect` command (`commands.rs:2294`) → `state.orchd()?.request(..)` → `orchd_client.rs:429` wraps reply in `tokio::time::timeout(REQUEST_TIMEOUT=30s)`. Orchd-side `crates/orchd/src/mcp/lifecycle.rs:67-87`: `let timeout = Duration::from_millis(server.timeout_ms.max(0))` then `tokio::time::timeout(timeout, connect_fn(..))` AND `tokio::time::timeout(timeout, session.list_tools())`, elapsed → `McpError::Timeout`. Default `timeout_ms=30000` (`persistence.rs:436` NOT NULL DEFAULT 30000; UI passes `null` → `socket_server.rs:1488` `unwrap_or(30_000)`).
- Error handling: present at BOTH layers now — the hung connect is bounded and surfaced as `McpError::Timeout` → in-dialog error + toast; `ConnectDialog` `busy` clears in `finally`.
- What the user sees: up to ~30 s of a disabled/dimmed "Connect" button, then an honest error (no permanent hang; no orchd restart needed).
- Delta from catalog: the "hangs forever / whole pipeline stalls / only orchd restart recovers / Tier-0 Critical" claim is OUTDATED. Residual is minor: no in-dialog spinner/progress during the ≤30 s wait, and ConnectDialog still uses state-`busy` not the ref guard (XC-02).
- Action: catalog-fix (mark EX-04 + RE-12 resolved; retire BL-89); optional Part-B polish = add a busy affordance / shorter default connect timeout.

### EX-05 — URL changed → re-consent
- Verdict: ✅ OK
- Traced: `ServersTab.tsx:375-377` always renders `ConnectDialog` for any connect; `trustGrantConsent` idempotent (doc `ConnectDialog.tsx:78-80`).
- Error handling: n/a.
- What the user sees: the same consent dialog re-appears; no distinct "URL changed" copy.
- Delta: no URL-changed-specific copy (F-04) — accepted, security-correct.
- Action: none.

### EX-06 — Set bearer token
- Verdict: ✅ OK
- Traced: `ServersTab.tsx:207-219` `handleSetBearer`→`mcpSetServerBearer(id,token)`; `<input type="password">` (`:340-351`), gated `orchdDown`; success clears draft + `strings.ext.servers.tokenSaved` toast; Keychain failure → `describeOrchdError` (Io) toast.
- Error handling: present, honest.
- What the user sees: "Token saved" toast, input cleared, never echoed.
- Delta: no persistent "bearer set" indicator beyond the toast (P-25) — accepted.
- Action: none.

### EX-07 — Tool enable/disable
- Verdict: ✅ OK
- Traced: `ToolsBrowser.tsx:155-168` `handleToggle`→`mcpSetToolEnabled(tool.id,!enabled)`→`refreshMcpTools`. Checkbox `checked={tool.enabled}` (no optimistic flip), `disabled={orchdDown}`. Failure → inline `tool-toggle-error-{id}` (`role="alert"`) + toast (with consent-recovery via `describeWithRecovery`).
- Error handling: present, inline + toast, no silent no-flip.
- What the user sees: checkbox reflects server state; on failure a red inline note.
- Delta: not `useSubmitGuard`-guarded → rapid toggle can double-fire (XC-02) — documented.
- Action: none.

### EX-08 — Tool invoke
- Verdict: ✅ OK
- Traced: `ToolsBrowser.tsx:170-193` `handleCall`→`mcpCallTool(serverId,name,argsJson,null)`; result → unconditional `tool-result-untrusted` banner (`strings.ext.unverified`), `toolError` shown on `isError`. args/invoke `disabled={orchdDown||!tool.enabled}` (`:207`).
- Error handling: present; inline `tool-call-error` + toast.
- What the user sees: JSON result under "⚠ unverified data"; disabled tool can't be invoked.
- Delta: unguarded → double-invoke duplicates the external call/spend (XC-02) — documented.
- Action: none.

### EX-09 — Invalid tool args JSON
- Verdict: ✅ OK
- Traced: `ToolsBrowser.tsx:173-178` `JSON.parse` guard → `setCallError(argsInvalidJson)`, returns before `mcpCallTool`.
- Error handling: present; no wire call.
- What the user sees: inline "arguments must be valid JSON".
- Delta: none.
- Action: none.

### EX-10 — Consent / Policy denial recovery hint
- Verdict: ✅ OK
- Traced: `ToolsBrowser.tsx:109-112` / `ConnectorsTab.tsx:198-201` `describeWithRecovery` = `describeOrchdError(e)` + (`isConsentError` ? ` ${strings.errors.consentRecovery}`). `isConsentError`/mapping `ipc/orchd.ts:810-843`. Hint copy = "To reconnect, open Extensions → Servers → Connect." (`strings.ts:45`).
- Error handling: present; consent/policy mapped, hint appended for consent.
- What the user sees: toast with mapped message + a text nav hint.
- Delta: hint is text-only, no clickable nav (P-20 partial) — accepted.
- Action: none.

### EX-11 — Add connector API key
- Verdict: ✅ OK
- Traced: `ConnectorsTab.tsx:347-363` `handleAddApiKey`→`connectorAddApiKey`; `<input type="password">` (`:625-633`), cleared on success; submit `disabled={orchdDown||apiKeyBlocked||submitting}` (guard `apiKeyForm`). Keychain failure → Io toast.
- Error handling: present, honest.
- What the user sees: account row added, key masked & never re-shown.
- Delta: none.
- Action: none.

### EX-12 — Begin OAuth
- Verdict: ✅ OK
- Traced: `ConnectorsTab.tsx:370-389` `handleBeginOAuth`→`connectorBeginOAuth` then best-effort `openUrl(authorizeUrl).catch(()=>{})`; provider `<select>` fed by `connectorListProviders()` (`:291-308`); `oauth-no-providers` empty-state shows only when `providersLoaded && length===0` (`:648-655`); begin blocked when registry empty.
- Error handling: present; begin failure → toast; auto-open failure silently swallowed (link is the real affordance).
- What the user sees: authorize link + code field; honest "No OAuth providers configured…" when empty.
- Delta: none (F-04/O-5 improved).
- Action: none.

### EX-13 — Complete OAuth
- Verdict: ✅ OK
- Traced: `ConnectorsTab.tsx:393-408` `handleCompleteOAuth`→`connectorCompleteOAuth({state,code})`→toast `accountConnected`; finish `disabled={orchdDown||oauthCompleteBlocked||submitting}` (`:721`); code `<input>` (`:710-717`) has NO `disabled` (not orchd-gated, P-06).
- Error handling: present; failure keeps fields for retry + toast.
- What the user sees: "Account connected" toast; code field editable even while down (submit blocked).
- Delta: token-exchange HTTP client timeout latent (BL-91) — daemon-side, unreachable while registry empty; not a webview defect.
- Action: none.

### EX-14 — Connector op invoke
- Verdict: ✅ OK
- Traced: `ConnectorsTab.tsx:424-449` `handleInvoke`→JSON guard→`connectorInvoke({accountId,op,argsJson})`→unconditional `ops-result-untrusted` banner. `loadOps` (`:319-333`) tracks `loading/ready/failed`; failed → `ops-load-failed-{id}` + `ops-retry-{id}` (`:503-526`). Invoke error → inline `ops-call-error` + toast.
- Error handling: present; load-failed distinct from empty (P-15 fixed), Retry offered.
- What the user sees: result under unverified banner; a load failure shows "Failed to load operations." + Retry, not empty-forever.
- Delta: op invoke unguarded → double-fire (XC-02) — documented.
- Action: none.

### EX-15 — Delete server / account / skill
- Verdict: ✅ OK
- Traced: `ServersTab.tsx:197-205` / `ConnectorsTab.tsx:414-422` / `SkillsTab.tsx:231-239` — each `window.confirm(deleteConfirm(name))` then the delete verb + refresh, gated `orchdDown`.
- Error handling: present, toast on failure.
- What the user sees: confirm then row removed.
- Delta: not `useSubmitGuard`-guarded (XC-02) — documented, idempotent-ish.
- Action: none.

### EX-16 — Add skill
- Verdict: ✅ OK
- Traced: `SkillsTab.tsx:196-229` `handlePickFile`→`pickSkillFile()` (sessiond, cancel→no-op) mapped by LOCAL `describeCommandError` (`:17-35`, NOT the orchd mapper, P-16); `handleAdd`→`skillAdd(name||null,desc||null,mdPath,"global",null)`. `skills-banner` registry note (`:243-245`); filestate badge modified/missing (`:150-153,331-335`); scope project disabled.
- Error handling: present; picker errors preserve real message; malformed frontmatter → Validation toast.
- What the user sees: skill row + honest "registry, runs once an agent exists" banner + file-state badge.
- Delta: none.
- Action: none.

### EX-17 — Set spend/rate policy
- Verdict: ✅ OK
- Traced: `InvocationLog.tsx:176-197` `handleSetPolicy`→`trustSetPolicy(scope,refId|null,spend,rate)`; ref-id `disabled={scope==="global"}` (`:220`); non-number → `strings.ext.log.limitMustBeNumber` toast BEFORE the wire (`:180-183`); submit gated+guarded.
- Error handling: present; client NaN check pre-wire, server reject → toast.
- What the user sees: policy row; non-number blocked with a toast.
- Delta: negative values not client-checked here (only RulesetPanel/PR-19 does) — server enforces; minor, out of this row's checks.
- Action: none.

### EX-18 — Audit table not live
- Verdict: ✅ OK
- Traced: `InvocationLog.tsx:163-168` mount-fetch `refreshAuditRows`; NO `orchd://audit-changed` event exists (`App.tsx` binds invocation-logged + policies-changed only; grep of `events.ts`/`App.tsx` = none). `audit-rows-empty` empty-state (`:325`).
- Error handling: n/a (read).
- What the user sees: audit updates only on (re)mount; honest "no audit records" when empty.
- Delta: refreshes on mount only (P-23) — accepted; invocations/policies ARE push-live.
- Action: none.

### EX-19 — Calls table
- Verdict: ✅ OK
- Traced: `InvocationLog.tsx:279-320` source/tool/status/latency/cost/time; `invocations-empty` (`:282`); live via `onOrchdMcpInvocationLogged→refreshInvocations` (`App.tsx:225`). Policies "no limits set" (`:252`).
- Error handling: n/a (read).
- What the user sees: read-only invocation log, honest empties.
- Delta: none.
- Action: none.

### EX-20 — Artifacts show content
- Verdict: ✅ OK
- Traced: `ArtifactsTab.tsx:159-197` mount-fetch `refreshMcpArtifacts`; `ArtifactViewer` (`:93-141`) toggle `artifact-content-{id}` = `contentText ?? contentJson`; unverified banner rendered off `artifact.isUntrusted` (unconditional in practice, all `is_untrusted:true`); `artifacts-empty` (`:175`).
- Error handling: n/a (read-only, no mutating verbs).
- What the user sees: durable artifact rows, expandable content, "⚠ unverified data".
- Delta: none.
- Action: none.

### EX-21 — Disconnect server
- Verdict: ✅ OK
- Traced: `ServersTab.tsx:188-195,331-339` `handleDisconnect`→`mcpDisconnect(id)` (direct, no consent gate) → refresh; gated `orchdDown`.
- Error handling: present, toast.
- What the user sees: server disconnected.
- Delta: not `useSubmitGuard`-guarded (XC-02) — documented.
- Action: none.

---

## DG — Degradation

| ID | Verdict | Sev | One-liner |
|---|---|---|---|
| DG-01 | ✅ OK | — | OrchdDownBanner mounted globally (App:481) + inside ExtPanel/ProjectPanel |
| DG-02 | ✅ OK | — | Every ext mutating control gated on orchdDown; reads live |
| DG-03 | ✅ OK | — | Ungated residuals fall to honest "orchestrator unavailable" toast |
| DG-04 | ✅ OK | — | onOrchdUp rehydrates every live slice (projects+open project+ext+research+storage) |
| DG-05 | ✅ OK | — | Toast FIFO cap 5, 4000 ms auto-dismiss, manual ×, no clock reset |
| DG-06 | ✅ OK | — | loading/empty/failed distinct across strips/trees/ops |
| DG-07 | ✅ OK | — | Cached reads render while down; only mutations disabled |
| DG-08 | ✅ OK | — | Retry fires orchdReconnect fire-and-forget; heals on orchd://up |
| DG-09 | ✅ OK | — | Push::Error warn-only in broker (B-10) — documented honest gap |
| DG-10 | ✅ OK | — | Every ext list renders a dedicated empty-state |
| DG-11 | ✅ OK | — | StorageBanner recovered/in-memory, persistent, no dismiss |
| DG-12 | 🟡 UX-GAP | Minor | Connector ops select is bare (no loading affordance) while listing — matches doc |

### DG-01 — Shared banner everywhere
- Verdict: ✅ OK
- Traced: `App.tsx:481` `{orchdDown && <OrchdDownBanner/>}` global; `ExtPanel.tsx:86` `{orchdDown && <OrchdDownBanner/>}`; ProjectPanel mounts it per catalog. `OrchdDownBanner.tsx` presentational (reads nothing).
- Error handling: n/a.
- What the user sees: "Orchestrator unavailable" + [Retry] wherever a caller mounts it.
- Delta: none.
- Action: none.

### DG-02 — Mutations disabled across surfaces
- Verdict: ✅ OK
- Traced: ext controls verified `disabled` on `orchdDown` (ServersTab add/toggle/connect/disconnect/bearer/delete; ToolsBrowser toggle/invoke; ConnectorsTab all; SkillsTab add/delete; InvocationLog set-policy). Reads (lists/artifacts/search) not gated.
- Error handling: n/a (disabled).
- What the user sees: greyed mutating controls; lists still readable.
- Delta: none (Goals/Tasks/Insights/Research/Graph/Project/Rules verified in other batches).
- Action: none.

### DG-03 — Ungated residuals → honest toast
- Verdict: ✅ OK
- Traced: `ConnectDialog.handleConfirm` (opened while up, orchd drops mid-dialog) → `trustGrantConsent` rejects `disconnected` → `describeOrchdError` → "orchestrator unavailable" in-dialog + toast; OAuth code field editable but finish gated. (ProjectPanel/sidebar/CreateProject residuals are other batches.)
- Error handling: present; never a fake success.
- What the user sees: an honest "orchestrator unavailable" toast, not silence.
- Delta: none.
- Action: none.

### DG-04 — Reconnect rehydration
- Verdict: ✅ OK
- Traced: `App.tsx:250-304` `onOrchdUp`: `setOrchdDown(false)` + refreshProjects + open-project goals/tasks/ideas/insights/ruleset/graph + ext servers/artifacts/accounts/skills/policies/invocations + research runs per loaded idea + global ruleset (if loaded) + storage status.
- Error handling: each `refresh*` toasts on failure (`store.ts:621+`).
- What the user sees: every live slice repopulates on reconnect.
- Delta: none (BL-92 fixed).
- Action: none.

### DG-05 — Toast queue
- Verdict: ✅ OK
- Traced: `store.ts:589-612` showToast/dismissToast; `TOAST_QUEUE_CAP=5` (`:412`), `TOAST_AUTO_DISMISS_MS=4000` (`:408`), armToastTimer only when head changes (no clock reset on burst); `Toast.tsx:52-69` manual `toast-dismiss` (aria "Close") → advances queue.
- Error handling: n/a.
- What the user sees: two errors readable in turn; × closes to the next.
- Delta: none (BL-97/P-21 fixed).
- Action: none.

### DG-06 — Loading/empty/failed distinct
- Verdict: ✅ OK
- Traced: ext surfaces — servers/tools/accounts/skills/artifacts/policies/invocations/audit all have `*-empty`; ConnectorsTab ops `loading/ready/failed` with Retry; CommandStrip/FileTree/FilePreview/ResearchPane per catalog (other batches).
- Error handling: failure distinct from empty (ops Retry; tool/ops inline errors).
- What the user sees: no list conflates loading with empty.
- Delta: connector ops LOADING has no affordance (see DG-12).
- Action: none.

### DG-07 — Degraded reads
- Verdict: ✅ OK
- Traced: store slices are plain reads not gated by `orchdDown`; ext tabs render cached `mcpServers`/`accounts`/`invocations` etc. while down; graph search stays enabled (other batch).
- Error handling: n/a.
- What the user sees: last-known data visible; only mutations disabled.
- Delta: none.
- Action: none.

### DG-08 — Retry button
- Verdict: ✅ OK
- Traced: `OrchdDownBanner.tsx:48-56` `orchd-down-retry`→`void orchdReconnect()` (fire-and-forget, no `.catch`); outcome via `orchd://down`/`up`. `orchdReconnect` defined `ipc/orchd.ts:451`.
- Error handling: outcome observed via events, not the promise (documented).
- What the user sees: click Retry, UI heals when orchd://up fires; no busy affordance (P-05).
- Delta: none.
- Action: none.

### DG-09 — sessiond Push::Error not surfaced
- Verdict: ✅ OK (documented gap)
- Traced: catalog notes `broker.rs` warn-only (B-10); no webview path consumes a session-level error event.
- Error handling: honest known gap, no UI dead-end.
- What the user sees: nothing for a `Push::Error`.
- Delta: matches documented gap.
- Action: none.

### DG-10 — Real empty-states
- Verdict: ✅ OK
- Traced: ext `servers-empty`/`tools-empty`/`accounts-empty`/`skills-empty`/`artifacts-empty`/`policies-empty`/`invocations-empty`/`audit-rows-empty` all present; sidebar/ideas/insights/tasks/graph per other batches.
- Error handling: n/a.
- What the user sees: a calm empty line, not debug-looking blanks.
- Delta: none (P-11 fixed for ext).
- Action: none.

### DG-11 — Storage degraded banner
- Verdict: ✅ OK
- Traced: `StorageBanner.tsx:33-46` reads `storageStatus`; renders recovered/in-memory copy, red accent, no dismiss; refetched on connect + every orchd://up (`App.tsx:302`).
- Error handling: n/a (honest persistent surface).
- What the user sees: a persistent banner whenever data won't persist.
- Delta: none.
- Action: none.

### DG-12 — Connector ops loading, no spinner
- Verdict: 🟡 UX-GAP (Minor)
- Traced: `ConnectorsTab.tsx:527-544` — while `opsStatus==="loading"` the op `<select>` simply has no options; there is no distinct loading affordance (failed IS distinct via Retry `:503-526`).
- Error handling: failure distinct; only the loading state is bare.
- What the user sees: a momentarily empty op dropdown while `connectorListOps` is in flight.
- Delta: matches the catalog's own "minor rough edge" note.
- Action: none (accepted); optional Part-B polish.

---

## XC — Cross-cutting

| ID | Verdict | Sev | One-liner |
|---|---|---|---|
| XC-01 | ✅ OK | — | useSubmitGuard ref-lock + `disabled=…||submitting` on every guarded submit |
| XC-02 | ✅ OK | — | Per-row/toggle/invoke/disconnect + ConnectDialog use weaker guards — as documented |
| XC-03 | 🟡 UX-GAP | Minor | Theme toggle now EXISTS/wired, but light/dark not applied to app chrome yet (Part B B3) |
| XC-04 | ✅ OK | — | No client length caps anywhere; long/emoji/RTL passed through — as documented |
| XC-05 | ✅ OK | — | Daemon idempotent; no webview single-instance guard — documented open question |
| XC-06 | ✅ OK | — | Dialogs bind Enter/Escape; terminal-tab no onKeyDown (TE-04) documented |
| XC-07 | ✅ OK | — | describeOrchdError maps all 7 codes exactly to English copy |
| XC-08 | ✅ OK | — | Coarse orchd://*-changed pushes re-fetch; screens converge (LWW) |
| XC-09 | 🟡 UX-GAP | Minor | Only FileTree virtualized; long ext/domain lists render un-windowed — documented |
| XC-10 | ✅ OK | — | Unverified-data banner on every external payload (Tools/Connectors/Artifacts) |

### XC-01 — Guarded double-submit
- Verdict: ✅ OK
- Traced: `useSubmitGuard.ts:40-61` `inFlight` ref (synchronous) + `submitting` state; used by ServersTab add, SkillsTab add, InvocationLog setPolicy, ConnectorsTab apiKey/oauthBegin/oauthComplete (3 independent guards `:249-251`), QuickCapture submit (`:211`), plus CreateProject/Ideas/Tasks/Research/FormInsight/Graph (other batches).
- Error handling: guard re-releases in `finally`; wrapped fn keeps its own try/catch.
- What the user sees: exactly one mutation on double-click; control shows disabled while submitting.
- Delta: none.
- Action: none.

### XC-02 — Unguarded controls
- Verdict: ✅ OK (accurately documented)
- Traced: ToolsBrowser toggle/invoke, ConnectorsTab op invoke, ServersTab disconnect/toggle/delete, ConnectDialog `busy` (state, not ref, `:95,166`) — none use `useSubmitGuard`.
- Error handling: present per-handler; some idempotent, some push-reconciled.
- What the user sees: a same-tick double-fire is possible; a double tool/op invoke duplicates the external call/spend.
- Delta: matches documented XC-02; the double-spend on tool/op invoke is a real (Minor, accepted) residual.
- Action: none (candidate Part-B hardening: guard tool/op invoke too).

### XC-03 — Theme toggle
- Verdict: 🟡 UX-GAP (Minor) + 📄 catalog stale
- Traced: `src/ui/ThemeToggle.tsx` mounted at `WorkspaceSidebar.tsx:375` (sidebar footer); cycles system→light→dark via store `theme`/`setTheme` (`store.ts:789-792`); `src/ui/theme.ts` `initTheme()` called in `main.tsx:10` (FOUC-free), persists to localStorage `bpa-theme`, stamps `data-theme` on root; `tokens.css` imported `main.tsx:3`. `statusTone` mapping present.
- Error handling: localStorage wrapped in try/catch (locked-down webview safe).
- What the user sees: a working, discoverable toggle with an icon + aria-label — BUT the app chrome (App/sidebar/ExtPanel/banners/Toast/all ext tabs/dialogs) still consumes the OLD static dark palette (`src/theme.ts`, hardcoded `#0d1117…`); only `ThemeToggle` + tokens.css consumers respond to `data-theme`. So "Light theme" leaves the app essentially dark.
- Delta: catalog row says "FUTURE — not present at 78af949"; it IS present now (📄), and the light palette is not yet applied app-wide (the deliberately-sequenced Part B B3 per-view refactor is pending — only B1 landed in commit 5202c14).
- Action: catalog-fix (update XC-03) + fix-in-Part-B (B3 token refactor makes the toggle visibly re-theme).

### XC-04 — Large / emoji / RTL input
- Verdict: ✅ OK (documented gap)
- Traced: no `maxLength` on any ext form field (ServersTab/ConnectorsTab/SkillsTab/InvocationLog inputs) nor QuickCapture; values passed straight to the wire.
- Error handling: server may reject/truncate.
- What the user sees: long/emoji/RTL renders; daemon decides validity.
- Delta: matches documented K-04.
- Action: none (open item).

### XC-05 — Second instance
- Verdict: ✅ OK (documented open question)
- Traced: daemon bootstrap idempotent (`launchd.rs` per catalog); no webview single-instance guard found (grep: no `tauri-plugin-single-instance`/window guard).
- Error handling: daemon side safe.
- What the user sees: unverified window-side behavior.
- Delta: matches documented K-05.
- Action: none (verify item).

### XC-06 — Keyboard
- Verdict: ✅ OK
- Traced: `QuickCapture.tsx:186-195` Escape close + Enter submit (title field `:236-244`); `ConnectDialog.tsx:100-105` Escape; `UpgradeDialog.tsx:96-103` Escape; focus set on open in each. Terminal tab `tabIndex=0` but no `onKeyDown` (TE-04, other batch).
- Error handling: n/a.
- What the user sees: dialogs submit on Enter / close on Escape.
- Delta: terminal tab not keyboard-activatable (documented, TE epic).
- Action: none.

### XC-07 — Server error mapping
- Verdict: ✅ OK
- Traced: `ipc/orchd.ts:793-823` maps Invariant/Conflict/NotFound/Validation/Io/Consent/Policy + disconnected/incompatibleOrchd→"orchestrator unavailable"; `strings.errors.*`.
- Error handling: exact; English-only.
- What the user sees: mapped English copy (may embed raw UUIDs, O-2 accepted).
- Delta: none.
- Action: none.

### XC-08 — Racing clients
- Verdict: ✅ OK
- Traced: `App.tsx:200-245` every `orchd://*-changed` handler re-fetches the affected list (coarse invalidation); no client-side merge (LWW).
- Error handling: refresh toasts on failure.
- What the user sees: both screens converge after a coarse push.
- Delta: none.
- Action: none.

### XC-09 — Large lists
- Verdict: 🟡 UX-GAP (Minor, documented)
- Traced: ext lists render via plain `.map` (ServersTab `:292`, ToolsBrowser `:206`, ConnectorsTab `:461`, InvocationLog invocations/audit tables) with no windowing; only `FileTree.tsx:681-700` is virtualized. Research runs refetch N+1 per idea on reconnect (`App.tsx:294-296`).
- Error handling: n/a.
- What the user sees: acceptable now; degrades with 100+ rows.
- Delta: matches documented K-06.
- Action: none (performance backlog).

### XC-10 — Unverified-data discipline
- Verdict: ✅ OK
- Traced: `ToolsBrowser.tsx:283`, `ConnectorsTab.tsx:583-588`, `ArtifactsTab.tsx:120-124` all render `strings.ext.unverified` ("⚠ unverified data") unconditionally on any external payload.
- Error handling: n/a.
- What the user sees: consistent honesty marker on every external result.
- Delta: none.
- Action: none.

---

## Counts

- ✅ OK: **39**
- 🟡 UX-GAP: **3** (all Minor — DG-12, XC-03, XC-09)
- 🔴 BUG: **0**
- 📄 DOC-GAP: **1** (EX-04 — BL-89 fixed, catalog stale)

Total scenarios audited: **43** (EX 21, DG 12, XC 10).

## Must-note for the catalog maintainer
1. **EX-04 + RE-12 + BL-89 are resolved** — retire the Tier-0 Critical; `crates/orchd/src/mcp/lifecycle.rs:67-87` timeout-bounds connect + list_tools (default 30 s), webview transport backstops at 30 s.
2. **XC-03** — the theme toggle is now shipped (sidebar footer, persisted, `data-theme`); update the row; the visible-effect gap is the pending Part B B3 token refactor.
3. Catalog `synced @ 78af949` lags HEAD `501bbe3`; a re-sync is due.

