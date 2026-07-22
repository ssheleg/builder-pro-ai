# UX Scenarios

<!-- Managed with super-ux (ux-contract v2). Update in the same change as any user-facing behavior change. -->

## Index

| ID | Title | Feature | Persona | Traces | Status | Last audit |
|----|-------|---------|---------|--------|--------|------------|
| SCN-001 | First launch — empty app | onboarding | P-02 | ST-001 | implemented | 2026-07-22 PASS |
| SCN-002 | Add first workspace | onboarding | P-02 | ST-002 | implemented | 2026-07-22 PASS |
| SCN-003 | Capture first idea with ⌘K | capture | P-02 | ST-003 | implemented | 2026-07-22 PASS |
| SCN-004 | Home attention triage | home | P-01 | ST-004 | implemented | 2026-07-22 PASS |
| SCN-005 | Home goals overview | home | P-01 | ST-005 | implemented | 2026-07-22 PASS |
| SCN-006 | Theme toggle cycle | chrome | P-01 | ST-006 | implemented | 2026-07-22 PASS |
| SCN-007 | Sidebar navigation | chrome | P-01 | ST-007 | implemented | 2026-07-22 PASS |
| SCN-008 | Link workspace to project from sidebar | chrome | P-01 | ST-007 | implemented | 2026-07-22 PASS |
| SCN-009 | Create project | projects | P-01 | ST-008 | implemented | 2026-07-22 PASS |
| SCN-010 | Project overview & workspace management | projects | P-01 | ST-008 | implemented | 2026-07-22 PASS |
| SCN-011 | Export / import project | projects | P-01 | ST-009 | implemented | 2026-07-22 PASS |
| SCN-012 | Archive and un-archive project | projects | P-01 | ST-009 | implemented | 2026-07-22 PASS |
| SCN-013 | Open a new terminal | terminals | P-01 | ST-010 | implemented | 2026-07-22 PASS |
| SCN-014 | Switch terminal tabs (keep-alive) | terminals | P-01 | ST-010 | implemented | 2026-07-22 PASS |
| SCN-015 | Close a terminal | terminals | P-01 | ST-010 | implemented | 2026-07-22 PASS |
| SCN-016 | Session lifecycle indicators | terminals | P-01 | ST-011 | implemented | 2026-07-22 PASS |
| SCN-017 | Command history strip | terminals | P-01 | ST-012 | implemented | 2026-07-22 PASS |
| SCN-018 | Click a link in terminal output | terminals | P-01 | ST-013 | implemented | 2026-07-22 PASS |
| SCN-019 | Daemon restart reconnect | terminals | P-03 | ST-014 | implemented | 2026-07-22 PASS |
| SCN-020 | Browse the file tree | files | P-01 | ST-015 | implemented | 2026-07-22 PASS |
| SCN-021 | Preview a file | files | P-01 | ST-015 | implemented | 2026-07-22 PASS |
| SCN-022 | Create / rename files and folders | files | P-01 | ST-016 | implemented | 2026-07-22 PASS |
| SCN-023 | Delete a file or folder | files | P-01 | ST-016 | implemented | 2026-07-22 PASS |
| SCN-024 | Live watch degradation and refresh | files | P-01 | ST-017 | implemented | 2026-07-22 PASS |
| SCN-025 | Manage ideas in a project | ideas | P-01 | ST-018 | implemented | 2026-07-22 PASS |
| SCN-026 | Run research on an idea | research | P-01 | ST-020 | implemented | 2026-07-22 PASS |
| SCN-027 | Form an insight from research | research | P-01 | ST-021 | implemented | 2026-07-22 PASS |
| SCN-028 | Orphan idea flows (Inbox: spawn project, link) | ideas | P-01 | ST-019 | implemented | 2026-07-22 PASS |
| SCN-029 | Manage insights | insights | P-01 | ST-022 | implemented | 2026-07-22 PASS |
| SCN-030 | Manage tasks | tasks | P-01 | ST-023 | implemented | 2026-07-22 PASS |
| SCN-031 | Manage the goal tree | goals | P-01 | ST-024 | implemented | 2026-07-22 PASS |
| SCN-032 | Build the knowledge graph | graph | P-01 | ST-025 | implemented | 2026-07-22 PASS |
| SCN-033 | MCP server lifecycle and consent | extensions | P-01 | ST-026 | implemented | 2026-07-22 PASS |
| SCN-034 | Invoke tools and connectors | extensions | P-01 | ST-027 | implemented | 2026-07-22 PASS |
| SCN-035 | Limits, call log, artifacts, skills | extensions | P-01 | ST-028 | implemented | 2026-07-22 PASS |
| SCN-036 | Edit rules and policy | rules | P-01 | ST-029 | implemented | 2026-07-22 PASS |
| SCN-037 | Sessiond disconnect and reconnect | system-status | P-01 | ST-030 | implemented | 2026-07-22 PASS |
| SCN-038 | Sessiond upgrade required | system-status | P-01 | ST-031 | implemented | 2026-07-22 PASS |
| SCN-039 | Orchd down degradation | system-status | P-01 | ST-030 | implemented | 2026-07-22 PASS |
| SCN-040 | Orchd upgrade and cancel re-entry | system-status | P-01 | ST-031 | implemented | 2026-07-22 PASS |
| SCN-041 | Storage degradation banners | system-status | P-01 | ST-030 | implemented | 2026-07-22 PASS |
| SCN-042 | Diagnostics panel | diagnostics | P-01 | ST-032 | implemented | 2026-07-22 PASS |
| SCN-043 | Render crash recovery | error-recovery | P-01 | ST-032 | implemented | 2026-07-22 PASS |
| SCN-044 | Terminal attach failure surfaced | terminals | P-01 | ST-014 | implemented | 2026-07-22 PASS |
| SCN-045 | Keep the machine awake while sessions run | power | P-01 | ST-033 | validated | — |
| SCN-046 | Enable the CEO and set the delegation scope | supervisor | P-01 | ST-034 | validated | — |
| SCN-047 | CEO answers an agent's question autonomously | supervisor | P-01 | ST-034 | validated | — |
| SCN-048 | CEO escalates an out-of-authority question | supervisor | P-01 | ST-034 | validated | — |
| SCN-049 | Workflow continuation after a task ends | supervisor | P-01 | ST-035 | validated | — |
| SCN-050 | Review decisions made while away | supervisor | P-03 | ST-036 | validated | — |
| SCN-051 | Set task priority (urgent / normal) | tasks | P-01 | ST-037 | validated | — |
| SCN-052 | Usage stats dashboard — tokens, cost, activity | analytics | P-01 | ST-038 | validated | — |
| SCN-053 | Output stats — commits and code per project | analytics | P-01 | ST-039 | validated | — |
| SCN-054 | Project documentation | docs | P-01 | ST-041 | draft | — |
| SCN-055 | Home v2 — attention hub | home | P-01 | ST-042 | draft | — |

## Personas

Defined in the WHY layer — see [foundation.md](foundation.md) §1. IDs:
**P-01** Solo Builder (steady state), **P-02** First-run Builder (cold start),
**P-03** Returning Builder (post-restart). All three are the same solo operator
in different lifecycle states.

## Scenarios

## onboarding

### SCN-001: First launch — empty app
- **Persona:** P-02
- **Feature:** onboarding
- **Traces:** ST-001 (JTBD-09, JRN-01/#2)
- **Entry point:** first launch, no saved state
- **Preconditions:** none
- **Steps:**
  1. User opens the app for the first time
- **Expected result:** Home view opens by default; sidebar shows "No workspaces yet — add a workspace or create a project to begin."; Home shows stat tiles (workspaces/live/waiting, all 0) and "No active sessions." with no action button; "+ project" and "+ Add workspace" CTAs visible in sidebar footer
- **UI elements:** sidebar empty-state sentence, stat tiles, EmptyState "No active sessions.", "+ project" button, "+ Add workspace" button, ThemeToggle, Diagnostics button
- **States covered:** empty
- **Errors & recovery:** daemon not yet connected → red "Daemon disconnected — reconnecting…" banner, auto-retries with backoff [500,1000,2000,5000]ms
- **Status:** implemented
- **Coverage:** src/store/store.ts:470, src/components/WorkspaceSidebar.tsx:271-284, src/strings.ts:112, src/components/HomeView.tsx:240-269, src/App.tsx:335-388

### SCN-002: Add first workspace
- **Persona:** P-02
- **Feature:** onboarding
- **Traces:** ST-002 (JTBD-09, JRN-01/#3)
- **Entry point:** sidebar "+ Add workspace" button
- **Preconditions:** none
- **Steps:**
  1. User clicks "+ Add workspace"
  2. User picks a folder in the OS folder picker
- **Expected result:** workspace created from folder basename, app navigates straight into the workspace view; Home "Open {name}" action now available when sessions are empty
- **UI elements:** "+ Add workspace" button, OS folder picker, workspace row in sidebar
- **States covered:** success
- **Errors & recovery:** picker cancelled → silent no-op; createWorkspace rejects → toast "Failed to add workspace: {msg}" via describeCommandError (disconnected / incompatible / too large / internal)
- **Status:** implemented
- **Coverage:** src/components/WorkspaceSidebar.tsx:110-127,445-461, src/strings.ts:59-66,105

### SCN-003: Capture first idea with ⌘K
- **Persona:** P-02
- **Feature:** capture
- **Traces:** ST-003 (JTBD-04, JRN-01/#5)
- **Entry point:** global ⌘K hotkey (any view)
- **Preconditions:** orchd connected
- **Steps:**
  1. User presses ⌘K
  2. User types a title (Enter submits; textarea Enter inserts newline)
  3. User optionally picks a project ("no project" default) and clicks "Save"
- **Expected result:** dialog "New idea" opens focused on title; on save, toast "idea saved" and the dialog closes; idea lands in the chosen project (or as orphan)
- **UI elements:** QuickCapture dialog, title input, description textarea, project select, "Cancel" / "Save" buttons
- **States covered:** success, error
- **Errors & recovery:** ⌘K ignored while typing in input/textarea/.xterm or while an upgrade dialog is open; empty title → Save disabled; orchd down → inline "orchestrator unavailable" note, Save disabled; save rejects → toast, dialog stays open for retry
- **Status:** implemented
- **Coverage:** src/components/QuickCapture.tsx:24-29,143-183,191,249-273, src/strings.ts:271-279

## home

### SCN-004: Home attention triage
- **Persona:** P-01
- **Feature:** home
- **Traces:** ST-004 (JTBD-01, JRN-02/#2)
- **Entry point:** app launch or sidebar "⌂ Home"
- **Preconditions:** sessions exist across workspaces
- **Steps:**
  1. User opens Home
  2. User scans "Needs you" (waiting), "Running", "Recently finished" groups
  3. User clicks "Go →" on a waiting row (or any running/finished row)
- **Expected result:** stat tiles show workspaces/live/waiting counts (live/waiting tone changes when > 0); clicking a row navigates to that workspace, activates the session, and focuses its terminal
- **UI elements:** stat tiles, group headers, StatusDot, "waiting for input" badge, "Go →" button, running rows, finished rows with ✓/✗ glyph + "code {n}"
- **States covered:** empty, success
- **Errors & recovery:** no sessions → "No active sessions." + "Open {name}" button when a workspace exists (no button with zero workspaces)
- **Status:** implemented
- **Coverage:** src/components/HomeView.tsx:204-211,240-378, src/strings.ts:125,129

### SCN-005: Home goals overview
- **Persona:** P-01
- **Feature:** home
- **Traces:** ST-005 (JTBD-06, JRN-07/#4)
- **Entry point:** Home view, "Goals" section below attention groups
- **Preconditions:** at least one active (non-archived) project
- **Steps:**
  1. User scans per-project blocks (project name, strategic goal title, child-goal status chips)
  2. User clicks a project block
- **Expected result:** project panel opens for that project
- **UI elements:** "Goals" heading, project block buttons, status chips "{title} · {status}"
- **States covered:** loading, empty, success
- **Errors & recovery:** goals still fetching → "Goals are loading…" (only while all active projects unfetched); no active projects → section renders nothing; fetch failures toast via refreshGoals
- **Status:** implemented
- **Coverage:** src/components/HomeGoals.tsx:141-201, src/strings.ts:134-135

## chrome

### SCN-006: Theme toggle cycle
- **Persona:** P-01
- **Feature:** chrome
- **Traces:** ST-006 (JTBD-06)
- **Entry point:** sidebar footer ThemeToggle button
- **Preconditions:** none
- **Steps:**
  1. User clicks the toggle repeatedly
- **Expected result:** theme cycles system → light → dark → system (icons ◐/☀/☾, aria "Theme: {current}. Click to switch."); persisted in localStorage `bpa-theme`; applied before first paint (no flash); OS appearance change tracked only in "system"
- **UI elements:** ThemeToggle button
- **States covered:** success
- **Errors & recovery:** nothing can fail (local only)
- **Status:** implemented
- **Coverage:** src/ui/ThemeToggle.tsx:8-26, src/ui/theme.ts:6-81, src/main.tsx:8,12

### SCN-007: Sidebar navigation
- **Persona:** P-01
- **Feature:** chrome
- **Traces:** ST-007 (JTBD-01, JTBD-06)
- **Entry point:** left sidebar (always visible)
- **Preconditions:** projects and workspaces exist
- **Steps:**
  1. User clicks "⌂ Home" / "⚙ Extensions" / a project header / a workspace row
  2. User expands "Archived (N)" and opens an archived project
- **Expected result:** view switches accordingly; project header opens the project panel; workspace row activates the workspace and shows terminals; archived group is collapsed by default and toggles with ▸/▾
- **UI elements:** "⌂ Home", "⚙ Extensions", "✉ Inbox" (+ orphan count badge), project group headers, workspace rows, "Archived (N)" toggle, archived project rows
- **States covered:** empty, success
- **Errors & recovery:** zero projects and workspaces → empty-state sentence (SCN-001); nothing else can fail (navigation is local)
- **Status:** implemented
- **Coverage:** src/components/WorkspaceSidebar.tsx:102-105,177-374, src/store/store.ts:101,571-573,760

### SCN-008: Link workspace to project from sidebar
- **Persona:** P-01
- **Feature:** chrome
- **Traces:** ST-007 (JTBD-06)
- **Entry point:** «No project» sidebar group, inline link select on an unlinked workspace
- **Preconditions:** an unlinked workspace and at least one active project exist
- **Steps:**
  1. User opens the link select on an unlinked workspace row
  2. User picks a project
- **Expected result:** workspace attaches to the project (orchdAddProjectWorkspace), projects refresh, row moves under the project group
- **UI elements:** «No project» group, link select ("link…" placeholder + project names)
- **States covered:** success, error
- **Errors & recovery:** attach rejects → toast describeOrchdError, selection resets
- **Status:** implemented
- **Coverage:** src/components/WorkspaceSidebar.tsx:129-139,322-362, src/strings.ts:102-103

## projects

### SCN-009: Create project
- **Persona:** P-01
- **Feature:** projects
- **Traces:** ST-008 (JTBD-06, JRN-07/#1)
- **Entry point:** sidebar "+ project" button
- **Preconditions:** none (workspace can be created inline)
- **Steps:**
  1. User clicks "+ project" → dialog "New project" opens focused on Name
  2. User types name and optional description
  3. User checks at least one free workspace (or clicks "+ create workspace" and picks a folder)
  4. User clicks "Create"
- **Expected result:** toast "Project created", dialog closes, project appears in sidebar
- **UI elements:** "New project" dialog, Name input, Description textarea, workspace checkbox list, "no available workspaces" note, "+ create workspace" button, blocked alert "at least one workspace is required", inline error line, "Cancel" / "Create" buttons
- **States covered:** empty, error, success
- **Errors & recovery:** 0 workspaces selected → blocked alert, Create disabled; empty name → Create disabled; create rejects → dialog stays open, inline error + toast; inline workspace-create failure → same error line via describeCommandError, fallback "failed to create workspace"; folder pick cancelled → silent no-op; double-submit guarded; Escape = Cancel
- **Status:** implemented
- **Coverage:** src/components/CreateProjectDialog.tsx:209-380, src/components/WorkspaceSidebar.tsx:427-448,502, src/strings.ts:208-217

### SCN-010: Project overview & workspace management
- **Persona:** P-01
- **Feature:** projects
- **Traces:** ST-008 (JTBD-06, JRN-07/#2)
- **Entry point:** project header in sidebar → project panel
- **Preconditions:** project exists
- **Steps:**
  1. User opens the project; Overview tab shows Goals/Ideas/Tasks/Insights counters and the workspaces panel
  2. User switches among 7 tabs (Overview/Goals/Ideas/Tasks/Insights/Rules/Graph)
  3. User detaches a workspace ("Unlink") or attaches one via "+ add workspace…" select
- **Expected result:** counters populate after eager refresh; unlink/attach update the list after refreshProjects
- **UI elements:** header (name + description), tab bar, counter tiles, workspaces panel, "Unlink" buttons, "+ add workspace…" select, "workspace unavailable" badge
- **States covered:** loading, error, success
- **Errors & recovery:** unknown/loading id → "Loading project…"; unresolved workspace id → danger badge "workspace unavailable" instead of dropped row; mutations reject → toast describeOrchdError; orchd down → OrchdDownBanner above tabs, Unlink/add-workspace/import disabled
- **Status:** implemented
- **Coverage:** src/components/ProjectPanel.tsx:118-205,274-475, src/strings.ts:218-235

### SCN-011: Export / import project
- **Persona:** P-01
- **Feature:** projects
- **Traces:** ST-009 (JTBD-06, JRN-07/#5)
- **Entry point:** project panel Overview tab, export/import controls
- **Preconditions:** project exists
- **Steps:**
  1. User clicks "Copy JSON" → project JSON copied to clipboard
  2. Or "Save to file…" → picks a folder → export written
  3. Or "Import from file…" → picks a folder → clicks a listed .json file
- **Expected result:** toasts "JSON copied" / "Exported to file" / import summary
- **UI elements:** "Copy JSON", "Save to file…", "Import from file…" buttons, .json file buttons, "No .json files in the selected folder" note
- **States covered:** empty, error, success
- **Errors & recovery:** folder pick cancelled → no-op; no .json files → honest empty note; any step rejects → toast describeOrchdError
- **Status:** implemented
- **Coverage:** src/components/ProjectPanel.tsx:198-242,417-475, src/strings.ts:228-241

### SCN-012: Archive and un-archive project
- **Persona:** P-01
- **Feature:** projects
- **Traces:** ST-009 (JTBD-06, JRN-07/#5)
- **Entry point:** project panel "Danger zone" → "Archive project"
- **Preconditions:** project not archived
- **Steps:**
  1. User clicks "Archive project"
  2. User confirms "Archive this project? It becomes read-only until you un-archive it."
  3. Later, from the archived banner, user clicks "Un-archive"
- **Expected result:** on archive: toast "Project archived", project moves to sidebar "Archived (N)" group, panel shows read-only banner and disables Unlink/add-workspace/import (export stays live — it is a read); un-archive restores it (no confirm — non-destructive)
- **UI elements:** "Archive project" danger button, window.confirm, archived role=status banner, "Un-archive" button
- **States covered:** success, error
- **Errors & recovery:** confirm cancelled → no-op; archive/unarchive reject → toast; orchd down → both buttons disabled; double-submit guarded
- **Status:** implemented
- **Coverage:** src/components/ProjectPanel.tsx:248-268,312-342,477-490, src/strings.ts:243-249

## terminals

### SCN-013: Open a new terminal
- **Persona:** P-01
- **Feature:** terminals
- **Traces:** ST-010 (JTBD-02, JRN-03/#1)
- **Entry point:** workspace view tab strip "+ New terminal"
- **Preconditions:** a workspace is active
- **Steps:**
  1. User clicks "+ New terminal"
- **Expected result:** session spawns (cwd = selected file's root or roots[0]); tab appears via session://created event and auto-activates if none active; terminal pane opens
- **UI elements:** "+ New terminal" button, session tab (StatusDot + title + ×), terminal pane
- **States covered:** empty, error, success
- **Errors & recovery:** no active workspace → button disabled (not-allowed cursor); create_session rejects → toast "Failed to open a new terminal: {msg}", no tab; zero sessions → pane placeholder "No terminals yet — pick a workspace and press + New terminal."
- **Status:** implemented
- **Coverage:** src/components/TerminalTabs.tsx:57-81,166-181, src/App.tsx:133-137,525-546, src/strings.ts:198,201

### SCN-014: Switch terminal tabs (keep-alive)
- **Persona:** P-01
- **Feature:** terminals
- **Traces:** ST-010 (JTBD-02, JRN-03/#2)
- **Entry point:** tab strip with 2+ sessions
- **Preconditions:** multiple live sessions
- **Steps:**
  1. User clicks another tab (or Enter/Space on it)
- **Expected result:** pane shows that session with full scrollback preserved; hidden sessions keep buffering output; no re-spawn, no duplicated replay
- **UI elements:** session tabs (role=tab, aria-selected), terminal pane
- **States covered:** success
- **Errors & recovery:** sessions exist but none active → "Select a terminal tab." placeholder; nothing else can fail (switch is local)
- **Status:** implemented
- **Coverage:** src/components/TerminalTabs.tsx:109-137, src/components/TerminalPane.tsx:51-67, src/terminal/terminal-manager.ts:113-122,413-419,523-544,584-589

### SCN-015: Close a terminal
- **Persona:** P-01
- **Feature:** terminals
- **Traces:** ST-010 (JTBD-02, JRN-03/#5)
- **Entry point:** × button on a session tab
- **Preconditions:** session exists
- **Steps:**
  1. User clicks × on the tab
- **Expected result:** PTY killed, xterm disposed, tab removed; active session cleared if it was this one
- **UI elements:** tab × close button
- **States covered:** success, error
- **Errors & recovery:** kill_session rejects → toast "Failed to close the terminal: {msg}", but tab is still removed and xterm disposed (no zombie tab)
- **Status:** implemented
- **Coverage:** src/components/TerminalTabs.tsx:83-97,144-161, src/terminal/terminal-manager.ts:627-641, src/store/store.ts:505-513, src/strings.ts:200-202

### SCN-016: Session lifecycle indicators
- **Persona:** P-01
- **Feature:** terminals
- **Traces:** ST-011 (JTBD-01, JRN-02/#3)
- **Entry point:** any surface with StatusDot (tabs, Home rows)
- **Preconditions:** session running
- **Steps:**
  1. User watches the dot as the session runs, waits for input, and exits
- **Expected result:** running → info dot; running + waitingForInput → warn dot "waiting for input"; atPrompt/typing → muted idle dot; exited → danger dot "exited"; exited tab stays with last scrollback until closed; a late state event cannot resurrect an exited session
- **UI elements:** StatusDot (aria labels idle/running/exited/"waiting for input"), session tab, Home rows
- **States covered:** success
- **Errors & recovery:** nothing can fail (display of pushed state); exited always wins over stale updates
- **Status:** implemented
- **Coverage:** src/components/StatusDot.tsx:13-57, src/store/store.ts:515-556, src/App.tsx:139-140

### SCN-017: Command history strip
- **Persona:** P-01
- **Feature:** terminals
- **Traces:** ST-012 (JTBD-02, JRN-03/#3)
- **Entry point:** workspace view, strip under the terminal (per active session)
- **Preconditions:** shell integration emits OSC-133 events
- **Steps:**
  1. User runs commands in the terminal and glances at the strip
- **Expected result:** last 10 commands as chips: ✓ (exit 0), ✗ {code}, "running" (live trailing command), "interrupted" (session ended mid-command)
- **UI elements:** command strip (role=list "Command history"), chips, "[Retry]" button
- **States covered:** loading, empty, error, success
- **Errors & recovery:** loading → "Loading command history…"; fetch fails → "Failed to load command history" + Retry + toast; no events → "No commands yet"
- **Status:** implemented
- **Coverage:** src/components/CommandStrip.tsx:44-71,114-270, src/strings.ts:187-194

### SCN-018: Click a link in terminal output
- **Persona:** P-01
- **Feature:** terminals
- **Traces:** ST-013 (JTBD-03, JRN-03/#4)
- **Entry point:** terminal output containing paths or OSC-8 hyperlinks
- **Preconditions:** session with output
- **Steps:**
  1. User clicks a path-like token (/a/b, ./a, a/b.ext) or an OSC-8 link
- **Expected result:** workspace file → file preview opens in the right rail; http(s) → OS default browser; file:// inside a root → preview
- **UI elements:** underlined links in terminal, FilesRail preview
- **States covered:** success, error
- **Errors & recovery:** file:// outside roots → toast "file is outside the workspace or not found"; non-existent lexical path → honest "not found" from the preview read; other schemes ignored
- **Status:** implemented
- **Coverage:** src/terminal/terminal-manager.ts:242-312, src/terminal/link-provider.ts:15-70,127-139,147-176, src/strings.ts:182

### SCN-019: Daemon restart reconnect
- **Persona:** P-03
- **Feature:** terminals
- **Traces:** ST-014 (JTBD-02, JRN-04/#3)
- **Entry point:** daemon restarts (or app reopens) with live sessions
- **Preconditions:** daemon-side sessions exist
- **Steps:**
  1. Daemon reconnects (daemon://reconnected) or app cold-boots
- **Expected result:** sessions re-hydrate as tabs; visible session eagerly re-attaches with a fresh full replay (term.reset first — no duplicated scrollback); hidden ones re-attach lazily on tab switch
- **UI elements:** session tabs, terminal pane
- **States covered:** loading, success
- **Errors & recovery:** while disconnected → red DaemonBanner (SCN-037); hydrate retries on backoff until success
- **Status:** implemented
- **Coverage:** src/App.tsx:162-183,335-345, src/terminal/terminal-manager.ts:372-374,509-520

## files

### SCN-020: Browse the file tree
- **Persona:** P-01
- **Feature:** files
- **Traces:** ST-015 (JTBD-03, JRN-06/#1)
- **Entry point:** workspace view right rail "Files"
- **Preconditions:** workspace with roots
- **Steps:**
  1. User expands a directory (click or Enter/Space)
  2. User toggles "show ignored"; collapses/reopens the rail with ⟩/⟨
  3. User clicks "+ Add root" and picks a folder
- **Expected result:** lazy fetch per dir with cache; dirs first, locale-sorted; ignored entries dimmed and hidden unless toggled; new root appears
- **UI elements:** rail header (⟩ collapse, "Files", "show ignored"), tree rows (role=treeitem), "Loading…" row, "empty folder" row, failed row + Retry, "+ Add root" button, ⟨ reopen strip
- **States covered:** loading, empty, error, success
- **Errors & recovery:** listDir fails → danger row "Failed to read folder: {msg}" + inline Retry (no auto-retry loop) + toast; add-root fails → toast "Failed to add root: {msg}"; no workspace → rail renders nothing
- **Status:** implemented
- **Coverage:** src/components/FileTree.tsx:111-168,298-415,482-491,658-707,714-772,786-805, src/components/FilesRail.tsx:22-152, src/strings.ts:140-152,161

### SCN-021: Preview a file
- **Persona:** P-01
- **Feature:** files
- **Traces:** ST-015 (JTBD-03, JRN-06/#2)
- **Entry point:** file row click in the tree (or terminal link, SCN-018)
- **Preconditions:** workspace with files
- **Steps:**
  1. User clicks a file
- **Expected result:** preview pane (40% of rail) renders text content; binary → "Binary file · {size}"; > 1 MiB → "File too large to preview · {size}"; changed-under-read → truncation banner
- **UI elements:** preview pane, "Select a file to preview" placeholder, "Loading…", truncation banner, error card
- **States covered:** loading, empty, error, success
- **Errors & recovery:** read fails → danger card + toast "Failed to open file: {msg}" (not found / access denied / outside root / too large / io); stale responses dropped by token guard
- **Status:** implemented
- **Coverage:** src/components/FilePreview.tsx:10-26,60-160, src/ipc/fs.ts:30-45, src-tauri/src/fs_explorer.rs:37-41,294-359,482-485, src/strings.ts:50-57,172-177

### SCN-022: Create / rename files and folders
- **Persona:** P-01
- **Feature:** files
- **Traces:** ST-016 (JTBD-03, JRN-06/#3)
- **Entry point:** right-click (or ⋯ button) on a tree row → menu
- **Preconditions:** workspace with roots
- **Steps:**
  1. User opens the row menu and picks New file / New folder / Rename
  2. User types the name in the inline form; Enter submits, Escape cancels
- **Expected result:** entry created/renamed; parent dir re-listed
- **UI elements:** row menu (New file, New folder, Rename, Delete, Show in Finder, Open in external app), inline form popover with autofocus input
- **States covered:** success, error
- **Errors & recovery:** blank name → silent cancel; create fails → toast "Failed to create file|folder: {msg}" (incl. "a file with this name already exists"); rename fails → toast "Failed to rename: {msg}"; outside-click closes the popover
- **Status:** implemented
- **Coverage:** src/components/FileTree.tsx:380-389,422-449,514-656,729-769, src-tauri/src/fs_explorer.rs:366-368, src/strings.ts:154-155,164-168

### SCN-023: Delete a file or folder
- **Persona:** P-01
- **Feature:** files
- **Traces:** ST-016 (JTBD-03, JRN-06/#3)
- **Entry point:** tree row menu → Delete (non-root rows)
- **Preconditions:** entry exists
- **Steps:**
  1. User picks Delete
  2. User confirms `Delete <file|folder> "<rel>"? It will be moved to the Trash.`
- **Expected result:** entry moved to Trash; parent re-listed; if the deleted file was previewed, preview clears
- **UI elements:** Delete menu item, window.confirm
- **States covered:** success, error
- **Errors & recovery:** confirm cancelled → no-op; delete fails → toast "Failed to delete: {msg}"
- **Status:** implemented
- **Coverage:** src/components/FileTree.tsx:451-464,576-589, src-tauri/src/fs_explorer.rs:424-445, src/strings.ts:156-158

### SCN-024: Live watch degradation and refresh
- **Persona:** P-01
- **Feature:** files
- **Traces:** ST-017 (JTBD-03, JRN-06/#4)
- **Entry point:** workspace view with the watcher dead (fs://watch-error)
- **Preconditions:** file watch previously started
- **Steps:**
  1. Watcher dies → amber "live updates paused — refresh" button appears in the rail
  2. User clicks it
- **Expected result:** watch restarts, every cached dir dropped so the tree re-pulls honestly; banner clears on success
- **UI elements:** "live updates paused — refresh" amber button
- **States covered:** error, success
- **Errors & recovery:** restart rejects → paused flag re-set (never falsely "live"); switching to a healthy workspace clears a stale flag; collapsed rail shows a warn dot on the ⟨ strip so degradation stays visible
- **Status:** implemented
- **Coverage:** src/components/FilesRail.tsx:51,66-79,106-114,178-198, src/App.tsx:160,445-462, src/strings.ts:144

## ideas

### SCN-025: Manage ideas in a project
- **Persona:** P-01
- **Feature:** ideas
- **Traces:** ST-018 (JTBD-05, JRN-05/#1)
- **Entry point:** project panel "Ideas" tab
- **Preconditions:** project exists
- **Steps:**
  1. User creates an idea via the dashed form ("idea title", optional description, "+ idea")
  2. User edits title/body inline (blur/Enter commits)
  3. User changes lifecycle via select (captured/researching/specced/in development/shipped/archived)
  4. User deletes an idea and confirms "delete idea?"
- **Expected result:** list updates after each action, sorted newest-first; research count badge/status shown per idea
- **UI elements:** create form, idea rows, inline title input, body textarea, lifecycle select, "Delete" danger button, "Research" button, "research (N)"/"hide research" toggle
- **States covered:** empty, error, success
- **Errors & recovery:** empty title → "+ idea" disabled; blank/unchanged edit → silent revert; rejected save → revert to store value + toast; delete confirm cancelled → no-op; orchd down → all mutating controls disabled, reads stay live; empty list → "No ideas in this project yet."
- **Status:** implemented
- **Coverage:** src/components/IdeasList.tsx:21-28,154-314,333-492

### SCN-028: Orphan idea flows (Inbox: spawn project, link)
- **Persona:** P-01
- **Feature:** ideas
- **Traces:** ST-019 (JTBD-04, JRN-05/#2)
- **Entry point:** sidebar "✉ Inbox" nav (orphan count badge when > 0) → Inbox panel
- **Preconditions:** an idea captured with "no project"
- **Steps:**
  1. User clicks "✉ Inbox" in the sidebar and sees the orphan Ideas/Insights sections
  2. User either links the idea to a project (select + "link to project") or clicks "Create project" (folder pick → workspace → project → link, resumable on partial failure without duplicates)
- **Expected result:** orphan idea becomes part of a project; partial failure shows honest resume message and "Retry linking"; Inbox badge count drops
- **UI elements:** "✉ Inbox" nav button, orphan count badge, Inbox panel (title, subtitle, Ideas/Insights sections), orphan idea rows, project select, "link to project" button, SpawnProjectFromIdea button, inline spawn error
- **States covered:** empty, error, success
- **Errors & recovery:** folder cancel → no-op; picker/workspace/link failures → inline + toast with exact resume semantics; orchd down → OrchdDownBanner in the panel, mutating controls disabled; empty → "No ideas without a project." / "No insights without a project."
- **Status:** implemented
- **Coverage:** src/components/InboxPanel.tsx, src/components/WorkspaceSidebar.tsx:229-269, src/App.tsx:511-514, src/components/IdeasList.tsx:280-307, src/components/idea/SpawnProjectFromIdea.tsx:62-124, src/strings.ts:98-100,282-289

## research

### SCN-026: Run research on an idea
- **Persona:** P-01
- **Feature:** research
- **Traces:** ST-020 (JTBD-05, JRN-05/#3)
- **Entry point:** idea row "Research" button → ResearchRunDialog
- **Preconditions:** a connected MCP server with tools; orchd up
- **Steps:**
  1. User picks a server (connected only), then a tool
  2. User reviews/edits JSON args (seeded from the idea) and the spend preflight (cap, calls/min, cost note)
  3. User clicks "Run"
  4. User expands the research pane to watch statuses
- **Expected result:** toast "Research run started"; run appears with status badge pending/running/done/failed; pane self-polls every 2s until terminal; "show artifact" renders the result for a done run
- **UI elements:** ResearchRunDialog (server select, tool select, args textarea, preflight rows, "Run"), ResearchPane run rows, status badges, "show artifact" / "Form insight" / "form insight without research" buttons
- **States covered:** loading, empty, error, success
- **Errors & recovery:** invalid JSON → inline "arguments must be valid JSON"; start rejects → inline alert + toast, dialog stays open; failed run shows errorKind (or "unknown error") and still offers insight-forming; no runs → "no research for this idea yet"
- **Status:** implemented
- **Coverage:** src/components/idea/ResearchRunDialog.tsx:108-334, src/components/idea/ResearchPane.tsx:110-221

### SCN-027: Form an insight from research
- **Persona:** P-01
- **Feature:** research
- **Traces:** ST-021 (JTBD-05, JRN-05/#4)
- **Entry point:** ResearchPane "Form insight" (done run) or "form insight without research" (failed run)
- **Preconditions:** research run exists
- **Steps:**
  1. User reviews the fit-context panel (project goals + graph neighborhood) and fills name/description/verdict/reasoning
  2. User clicks "Create" → insight created (toast "Insight created")
  3. User clicks "Accept"
  4. User clicks "To backlog" → task created, idea moves to "specced" (toast "Task added to backlog")
- **Expected result:** three-stage progression with status line "insight status: {status}"; dialog closes after backlog
- **UI elements:** FormInsightDialog (name, description, verdict select, reasoning), fit-context panel ("Project goals", "Related graph", orphan note), "Create"/"Accept"/"To backlog" buttons, inline alert line
- **States covered:** empty, error, success
- **Errors & recovery:** orphan idea → context note "the idea is not linked to a project — context unavailable" + "To backlog" blocked; any step rejects → inline + toast, dialog stays open; partial backlog failure → resume message, retry never duplicates (createdTaskId/createdInsightId)
- **Status:** implemented
- **Coverage:** src/components/idea/FormInsightDialog.tsx (three-stage flow with resume state)

## insights

### SCN-029: Manage insights
- **Persona:** P-01
- **Feature:** insights
- **Traces:** ST-022 (JTBD-05)
- **Entry point:** project panel "Insights" tab
- **Preconditions:** insights exist (created only via research pipeline — no create form)
- **Steps:**
  1. User changes status via select (new/accepted/archived)
  2. For "archived": user types a reason and clicks "confirm archival"
  3. User overrides the fit verdict (select + reasoning + "apply verdict")
- **Expected result:** non-archive statuses apply immediately; archive applies only with a reason; verdict badge updates (fit/no fit/unclear/—)
- **UI elements:** insight rows, status select, archive reason input, "confirm archival" button, verdict select, reasoning input, "apply verdict" button, source caption
- **States covered:** empty, error, success
- **Errors & recovery:** empty archive reason → inline "an archive reason is required"; mutations reject → toast; orchd down → status/archival/apply disabled; empty list → "No insights in this project yet."
- **Status:** implemented
- **Coverage:** src/components/InsightsList.tsx:33,117-281,339-363

## tasks

### SCN-030: Manage tasks
- **Persona:** P-01
- **Feature:** tasks
- **Traces:** ST-023 (JTBD-06, JRN-07/#2)
- **Entry point:** project panel "Tasks" tab
- **Preconditions:** project exists
- **Steps:**
  1. User creates a task (title, description, source idea/insight/bug/plan, optional parent, comma-separated tags, "+ task")
  2. User moves a task between the six status groups via select
  3. User reorders within a group with ▲/▼
  4. User deletes a task; confirm names the cascade ("delete task? will delete N subtasks")
- **Expected result:** six groups always rendered (backlog/to do/waiting/in progress/testing/done) with counts; reorder is a single fractional-rank call; delete removes the subtree
- **UI elements:** create form, group headers "{label} ({count})", "no tasks" note, task rows (source badge), status select, ▲/▼ buttons, "Delete" button, window.confirm
- **States covered:** empty, error, success
- **Errors & recovery:** empty title → "+ task" disabled; ▲ disabled on first, ▼ on last row; confirm cancelled → no-op; mutations reject → toast; orchd down → mutating controls disabled
- **Status:** implemented
- **Coverage:** src/components/TasksList.tsx:52-73,202-264,296-355,461-492

### SCN-051: Set task priority (urgent / normal)
- **Persona:** P-01
- **Feature:** tasks
- **Traces:** ST-037 (JTBD-06, JTBD-10, JRN-10/#4)
- **Entry point:** project panel "Tasks" tab — create form and task rows
- **Preconditions:** project exists
- **Steps:**
  1. User sets priority (urgent/normal, default normal) in the create form or on an existing task row
- **Expected result:** priority persists; urgent tasks render visually distinct (danger-tone marker) and sort ahead of normal within their status group; workflow continuation (SCN-049) consumes urgent first
- **UI elements:** priority control in create form, priority control on task row, urgent marker on task rows
- **States covered:** success, error
- **Errors & recovery:** priority save rejects → revert to stored value + toast; orchd down → priority control disabled like other mutations
- **Status:** validated
- **Coverage:** none yet

## goals

### SCN-031: Manage the goal tree
- **Persona:** P-01
- **Feature:** goals
- **Traces:** ST-024 (JTBD-06, JRN-07/#2)
- **Entry point:** project panel "Goals" tab
- **Preconditions:** project exists (one strategic root goal)
- **Steps:**
  1. User adds a subgoal via per-row "+ subgoal" (seeded "new goal" for immediate rename)
  2. User renames inline, changes status (active/achieved/dropped)
  3. User moves a non-strategic goal with ▲/▼ (sibling swap)
  4. User deletes a branch; confirms "delete the entire branch?"
  5. User edits metric chips (add via "+ metric" input Enter, remove via ×)
- **Expected result:** DFS-indented tree updates after each mutation; strategic root never movable/deletable; no top-level add
- **UI elements:** "Goals" panel + count badge, tree rows (role=treeitem), title input, status select, ▲/▼, "+ subgoal", Delete, metric chip editor, window.confirm
- **States covered:** empty, error, success
- **Errors & recovery:** blank/unchanged rename → revert; rejected save → revert + toast; confirm cancelled → no-op; orchd down → all row controls disabled; empty tree → "The goal tree is empty."
- **Status:** implemented
- **Coverage:** src/components/GoalTree.tsx:36-61,192-371,404-536, src/components/HomeGoals.tsx:141-201

## graph

### SCN-032: Build the knowledge graph
- **Persona:** P-01
- **Feature:** graph
- **Traces:** ST-025 (JTBD-06, JRN-07/#3)
- **Entry point:** project panel "Graph" tab
- **Preconditions:** project exists
- **Steps:**
  1. User adds a node (title, optional body, kind concept/fact/artifact/decision/note, "Add node")
  2. User drags nodes (debounced 400ms move persist) and drags handles to connect (optimistic edge, kind "relates")
  3. User double-clicks a local node to rename (Enter/Escape)
  4. User selects an edge and changes its kind (relates/depends/derives/supports/contradicts/parent)
  5. User selects nodes/edges and clicks "Delete selection"; confirms "delete selection?"
  6. User searches (debounced) — matches highlighted
- **Expected result:** canvas reflects each change; entityRef nodes show "ref · {type}" (or "source removed" when orphaned); external ghost node click opens the foreign project; local entityRef click is an honest no-op
- **UI elements:** add-node form, canvas nodes/edges, rename bar, edge-kind select, "Delete selection" danger button, search input, empty overlay
- **States covered:** empty, error, success
- **Errors & recovery:** rejected edge add (self-loop/duplicate/failure) → optimistic edge rolled back + toast; deletes reconcile via refresh even on partial failure; stale search responses dropped; orchd down → mutating controls disabled, move/connect early-return; empty graph → "empty" overlay
- **Status:** implemented
- **Coverage:** src/components/graph/GraphCanvas.tsx:59-71,212-332,382-490,499-592,755-759, src/components/graph/graphMapping.ts

## extensions

### SCN-033: MCP server lifecycle and consent
- **Persona:** P-01
- **Feature:** extensions
- **Traces:** ST-026 (JTBD-07, JRN-08/#2)
- **Entry point:** sidebar "⚙ Extensions" → Servers tab
- **Preconditions:** orchd up
- **Steps:**
  1. User adds an MCP server via the add form (endpoint, auth: no authorization / bearer token; "OAuth (soon)" and "stdio (soon)" disabled)
  2. User clicks "connect" → consent gate: ConnectDialog "Connect to server "{name}"" showing the endpoint and access note
  3. User confirms → consent granted, connection established
  4. User sets a bearer token ("Token saved"), disconnects, disables, or deletes the server
- **Expected result:** server states update; connected servers become available in research/tools
- **UI elements:** Servers tab, add form, per-row set-bearer input, connect/disconnect/enable/disable/delete buttons, ConnectDialog (confirm/cancel)
- **States covered:** empty, error, success
- **Errors & recovery:** consent-kind rejection routes to ConnectDialog; dialog errors inline + toast; consent-denial toasts append "To reconnect, open Extensions → Servers → Connect."; orchd down → OrchdDownBanner above tabs, controls disabled
- **Status:** implemented
- **Coverage:** src/components/ext/ServersTab.tsx:174-341, src/components/ext/ConnectDialog.tsx:110-153, src/strings.ts:45,497-501,540-562

### SCN-034: Invoke tools and connectors
- **Persona:** P-01
- **Feature:** extensions
- **Traces:** ST-027 (JTBD-07, JRN-08/#3)
- **Entry point:** Extensions → Tools / Connectors tabs
- **Preconditions:** connected server / configured connector account
- **Steps:**
  1. Tools: user toggles per-tool allowlist, views schema, invokes a tool with JSON args
  2. Connectors: user adds an API-key account, or runs OAuth (start → open authorization page → paste code → finish), then invokes operations
- **Expected result:** results render with an unconditional "⚠ unverified data" banner; tool errors surface as "the tool returned an error"
- **UI elements:** tool list, allowlist toggles, invoke forms, result panes with unverified banner, connector account forms, OAuth provider dropdown
- **States covered:** empty, error, success
- **Errors & recovery:** no OAuth providers → "No OAuth providers configured — add one in oauth_providers.json (see runbook)."; invocation errors surfaced honestly; orchd down → controls disabled
- **Status:** implemented
- **Coverage:** src/components/ext/ToolsBrowser.tsx:79-82,140-269, src/components/ext/ConnectorsTab.tsx:128-131,287-338,456-655, src/strings.ts:479,516,537-538,569

### SCN-035: Limits, call log, artifacts, skills
- **Persona:** P-01
- **Feature:** extensions
- **Traces:** ST-028 (JTBD-07, JRN-08/#4)
- **Entry point:** Extensions → Log / Artifacts / Skills tabs
- **Preconditions:** some invocations exist
- **Steps:**
  1. Log: user sets spend/rate caps (scope, refId, spend cap, calls/min, "set limit"); reviews Calls and Audit tables
  2. Artifacts: user shows/hides saved tool results (each with "⚠ unverified data")
  3. Skills: user adds/deletes skills from SKILL.md; sees "modified"/"file missing" badges
- **Expected result:** limits persist; tables list invocations (source/tool/status/latency/cost/time) and audit decisions; skills registry updates
- **UI elements:** limits editor, Calls table, Audit table, artifact rows + unverified banners, skills list, plumbing note "Skills are a registry; they run once an orchestrator agent exists (S6b)."
- **States covered:** empty, error, success
- **Errors & recovery:** "no limits set" / "no artifacts" honest empty states; mutations reject → toast; orchd down → controls disabled
- **Status:** implemented
- **Coverage:** src/components/ext/InvocationLog.tsx:124-297, src/components/ext/ArtifactsTab.tsx:98-154, src/components/ext/SkillsTab.tsx:101-296, src/strings.ts:495,571-611

## rules

### SCN-036: Edit rules and policy
- **Persona:** P-01
- **Feature:** rules
- **Traces:** ST-029 (JTBD-06, JTBD-07)
- **Entry point:** project panel "Rules" tab
- **Preconditions:** project exists
- **Steps:**
  1. User edits the markdown rules textarea and clicks "Save"
  2. User edits the policy form: "Spend cap, $", confirmation classes chips, allowed-path chips; clicks "Save policy"
  3. If the file changed externally → user clicks "Accept"; if the file is lost → user clicks "Recreate"
  4. User clicks "reveal file" to open it in Finder
- **Expected result:** rules and policy persist; file-state banners (blue info accent) clear after Accept/Recreate
- **UI elements:** "Loading rules…" line, mdPath row + "reveal file", "file changed externally" + Accept, "file lost" + Recreate, markdown textarea + Save, policy form (number input, chip lists, "+ add", ×), "Save policy"
- **States covered:** loading, error, success
- **Errors & recovery:** inline validation "spend cap must be a number" / "spend cap cannot be negative" / "empty entries are not allowed"; mutations reject → toast; orchd down → Save/Accept/Recreate disabled ("reveal file" stays live)
- **Status:** implemented
- **Coverage:** src/components/RulesetPanel.tsx:36-57,321-515, src/components/ProjectPanel.tsx:35,498, src/strings.ts:291-313

## system-status

### SCN-037: Sessiond disconnect and reconnect
- **Persona:** P-01
- **Feature:** system-status
- **Traces:** ST-030 (JTBD-08, JRN-09/#1)
- **Entry point:** top-of-shell DaemonBanner
- **Preconditions:** sessiond connection drops
- **Steps:**
  1. Daemon disconnects
- **Expected result:** red banner "Daemon disconnected — reconnecting…" (no action needed); auto-reconnect; banner disappears on success and sessions re-attach (SCN-019)
- **UI elements:** DaemonBanner (red, role=alert, no dismiss)
- **States covered:** error, success
- **Errors & recovery:** self-healing; nothing to click
- **Status:** implemented
- **Coverage:** src/components/DaemonBanner.tsx:61-77, src/App.tsx:161-183,475, src/strings.ts:79

### SCN-038: Sessiond upgrade required
- **Persona:** P-01
- **Feature:** system-status
- **Traces:** ST-031 (JTBD-08, JRN-09/#2)
- **Entry point:** daemon://incompatible event → UpgradeDialog (auto-open)
- **Preconditions:** installed sessiond older than the app requires
- **Steps:**
  1. Dialog "Update required" opens: "Update the background service — {n} live sessions will end. Their records and scrollback are saved and will reappear as inactive." (or the "all of its live sessions" variant pre-hydration)
  2. User clicks "Update" (or "Cancel")
- **Expected result:** Update → daemon kickstart, app restarts (success never returns); Cancel → dialog closes but amber banner "Background service is outdated — update required" + "Update" persists until upgrade
- **UI elements:** UpgradeDialog (title, body, "Cancel"/"Update"), amber DaemonBanner + "Update" button, inline dialog error line
- **States covered:** error, success
- **Errors & recovery:** upgrade fails → inline "Failed to restart the background service: {err}. Check permissions (launchctl) and try again."; retry clears prior error; flag is fatal until app restart
- **Status:** implemented
- **Coverage:** src/components/UpgradeDialog.tsx:141,153-158,190-218, src/components/DaemonBanner.tsx:25-59, src/App.tsx:184-193,365-378, src/strings.ts:77,82-89

### SCN-039: Orchd down degradation
- **Persona:** P-01
- **Feature:** system-status
- **Traces:** ST-030 (JTBD-08, JRN-09/#1)
- **Entry point:** orchd://down event
- **Preconditions:** orchestrator socket drops
- **Steps:**
  1. Orchd goes down
  2. User clicks "Retry" (optional)
- **Expected result:** red "Orchestrator unavailable" banner globally and above project/extensions tab bars; every orchd-mutating control across ideas/insights/tasks/goals/graph/rules/extensions disables; reads stay live; Retry fires orchdReconnect, outcome arrives via orchd://up (banners clear, data reloads)
- **UI elements:** OrchdDownBanner + "Retry" button, disabled mutating controls everywhere
- **States covered:** error, success
- **Errors & recovery:** capture and dialogs show inline "orchestrator unavailable" instead of doomed round-trips; lost cold-boot race self-heals on orchd://up
- **Status:** implemented
- **Coverage:** src/components/OrchdDownBanner.tsx:43-56, src/App.tsx:249-304,487, src/components/ProjectPanel.tsx:281, src/components/ext/ExtPanel.tsx:88, src/components/InboxPanel.tsx:53

### SCN-040: Orchd upgrade and cancel re-entry
- **Persona:** P-01
- **Feature:** system-status
- **Traces:** ST-031 (JTBD-08, JRN-09/#2)
- **Entry point:** orchd://incompatible event → UpgradeDialog (orchd variant)
- **Preconditions:** installed orchd older than required
- **Steps:**
  1. Dialog opens: "Update the orchestrator background service — records (projects, goals, tasks) are saved"
  2. User clicks "Update" (or "Cancel" → amber re-entry banner "Orchestrator service is outdated — update required" + "Update")
- **Expected result:** Update → restart; Cancel keeps a persistent path back to the dialog (no dead-end); sessiond variant takes precedence if both daemons are incompatible
- **UI elements:** UpgradeDialog orchd variant, OrchdUpgradeBanner + "Update"
- **States covered:** error, success
- **Errors & recovery:** upgrade fails → inline "Failed to restart the orchestrator background service: {err}. Check permissions (launchctl) and try again."
- **Status:** implemented
- **Coverage:** src/components/UpgradeDialog.tsx:141-143,242-259, src/components/OrchdUpgradeBanner.tsx:48-61, src/App.tsx:311-317, src/strings.ts:80,90-92

### SCN-041: Storage degradation banners
- **Persona:** P-01
- **Feature:** system-status
- **Traces:** ST-030 (JTBD-08, JRN-09/#3)
- **Entry point:** orchd storage status on connect/reconnect
- **Preconditions:** orchd database degraded
- **Steps:**
  1. App connects to orchd running degraded
- **Expected result:** red banner — in-memory: "Storage unavailable — running in memory. Changes will NOT survive a restart."; corruption: "Database was corrupted and has been reset. The damaged copy was saved to {path}."; persists until a healthy daemon restart (no dismiss, no in-app recovery)
- **UI elements:** StorageBanner (red, role=alert)
- **States covered:** error
- **Errors & recovery:** honest permanent warning; recovery is external (restart daemon healthy)
- **Status:** implemented
- **Coverage:** src/components/StorageBanner.tsx:32-45, src/App.tsx:302,402,478, src/strings.ts:617-619

## diagnostics

### SCN-042: Diagnostics panel
- **Persona:** P-01
- **Feature:** diagnostics
- **Traces:** ST-032 (JTBD-08, JRN-09/#4)
- **Entry point:** sidebar footer "Diagnostics" button (red count badge when events exist)
- **Preconditions:** none
- **Steps:**
  1. User clicks "Diagnostics"
  2. User reviews the event ring (time, kind badge, op, message, scrubbed detail)
  3. User clicks "Copy support bundle" or "Clear"
- **Expected result:** panel lists up to 200 newest-first events; copy puts pretty JSON bundle on clipboard; clear empties the ring and badge
- **UI elements:** "Diagnostics" button + count badge, panel rows, "Copy support bundle", "Clear" (danger), empty state "No errors recorded" + hint
- **States covered:** empty, success
- **Errors & recovery:** secrets/home paths scrubbed at record time; every toast-surfaced failure also lands here so causes survive the 4s toast
- **Status:** implemented
- **Coverage:** src/components/DiagnosticsPanel.tsx:18-86, src/ipc/diag.ts:28,62-103, src/components/WorkspaceSidebar.tsx:86,462-503, src/store/store.ts:642-678

## error-recovery

### SCN-043: Render crash recovery
- **Persona:** P-01
- **Feature:** error-recovery
- **Traces:** ST-032 (JTBD-08, JRN-09/#5)
- **Entry point:** unexpected React render crash anywhere
- **Preconditions:** none
- **Steps:**
  1. A view throws during render
  2. User clicks "Reload app" (or "Copy details")
- **Expected result:** instead of a white screen: full-viewport card "Something broke" with explanation, `{name}: {message}` line, and the crash recorded in Diagnostics; reload recovers
- **UI elements:** ErrorBoundary card, "Reload app" primary button, "Copy details" ghost button
- **States covered:** error, success
- **Errors & recovery:** crash auto-recorded as render-kind DiagEvent (scrubbed, 6-line stack cap); copy puts name/message/component stack on clipboard
- **Status:** implemented
- **Coverage:** src/components/ErrorBoundary.tsx:28-86, src/main.tsx:17-19, src/store/store.ts:667-678

## terminals (gaps)

### SCN-044: Terminal attach failure surfaced
- **Persona:** P-01
- **Feature:** terminals
- **Traces:** ST-014 (JTBD-08, JRN-04/#4)
- **Entry point:** terminal pane when attach_session rejects (daemon hiccup mid-attach)
- **Preconditions:** session exists, attach fails
- **Steps:**
  1. Attach fails while opening/re-attaching a terminal
  2. User clicks "Retry" in the pane overlay
- **Expected result:** overlay note "Terminal could not attach: {msg}" (role=alert) appears over the pane; Retry re-attaches; overlay clears the moment the fresh attempt starts
- **UI elements:** pane error overlay, "Retry" button
- **States covered:** error, success
- **Errors & recovery:** manager records the mapped failure per session (describeAttachError → strings.errors.command.*) and notifies subscribers; a fresh attach clears it; dispose drops it
- **Status:** implemented
- **Coverage:** src/terminal/terminal-manager.ts:51-69,133,427-431,460-483,627-641,643-661, src/components/TerminalPane.tsx:43-49,80-121, src/strings.ts:183-186

## power

### SCN-045: Keep the machine awake while sessions run
- **Persona:** P-01
- **Feature:** power
- **Traces:** ST-033 (JTBD-10, JRN-10/#5)
- **Entry point:** keep-awake toggle in app chrome (sidebar footer, near ThemeToggle/Diagnostics)
- **Preconditions:** keep-awake enabled (default on)
- **Steps:**
  1. User starts a long agent run and walks away; ≥1 session is live
  2. Later, all sessions end (or user disables the toggle)
- **Expected result:** while ≥1 live session exists, a system sleep assertion is held and an "awake" indicator shows; when the last session ends or the toggle turns off, the assertion releases and normal power behavior resumes
- **UI elements:** keep-awake toggle, active-assertion indicator, failure banner/toast
- **States covered:** success, error
- **Errors & recovery:** OS denies the assertion → honest banner/toast "keep-awake unavailable: {reason}" + Diagnostics record — never a silent fake "awake"; app quit/crash → assertion released by OS (no orphan lock)
- **Status:** validated
- **Coverage:** none yet

## supervisor

### SCN-046: Enable the CEO and set the delegation scope
- **Persona:** P-01
- **Feature:** supervisor
- **Traces:** ST-034 (JTBD-10, JRN-10/#1)
- **Entry point:** project panel "Rules" tab — supervisor section (extends SCN-036 policy)
- **Preconditions:** project exists; orchd up
- **Steps:**
  1. User enables the CEO for the project
  2. User selects which confirmation classes are delegated and reviews the effective caps (spend, calls/min) the CEO inherits from policy
  3. User writes the CEO instruction text (free-form markdown the CEO must follow) and optional custom CEO rules
  4. User saves
- **Expected result:** delegation scope, instruction text, and custom rules persist with the project policy; an information-access summary states what the CEO reads ("CEO reads: project goals, tasks, ideas, insights, graph, rules + your instruction"); a scope summary states exactly what it may decide ("CEO may: {classes} within {caps}"); disabled by default until explicitly enabled
- **UI elements:** CEO enable toggle, delegated-class checkboxes, inherited-caps summary, CEO instruction textarea, custom-rules editor, info-access summary line, scope summary line, "MCP tools for the CEO — soon" placeholder note, "Save policy" (shared)
- **States covered:** success, error
- **Errors & recovery:** save rejects → inline + toast (policy form pattern); orchd down → controls disabled; empty delegation scope with CEO on → blocked alert "delegate at least one class or disable the CEO"
- **Status:** validated
- **Coverage:** none yet

### SCN-047: CEO answers an agent's question autonomously
- **Persona:** P-01
- **Feature:** supervisor
- **Traces:** ST-034 (JTBD-10, JRN-10/#2)
- **Entry point:** a terminal session enters waiting-for-input with a question matching a delegated class (operator may be absent)
- **Preconditions:** CEO enabled with a delegation scope (SCN-046); session running
- **Steps:**
  1. Agent asks a question; CEO classifies it against the delegation scope
  2. CEO answers within authority; the session continues
  3. Operator (whenever present) sees the decision reflected in session state and the decision log
- **Expected result:** the session never parks as "needs you" for in-scope questions; the answer, its basis (rule/policy line), and timestamp land in the decision log (SCN-050); the session's StatusDot returns to running
- **UI elements:** StatusDot transition (waiting → running without operator input), decision-log entry, per-session "answered by CEO" marker
- **States covered:** success, error
- **Errors & recovery:** CEO cannot ground an answer in rules/policy → treats it as out-of-scope (SCN-048), never guesses; CEO backend failure → session parks as ordinary "needs you" + Diagnostics record — degradation equals the current manual behavior, nothing worse
- **Status:** validated
- **Coverage:** none yet

### SCN-048: CEO escalates an out-of-authority question
- **Persona:** P-01
- **Feature:** supervisor
- **Traces:** ST-034 (JTBD-10, JRN-10/#3)
- **Entry point:** a waiting session's question falls outside the delegated scope (or over a cap)
- **Preconditions:** CEO enabled; question out of scope
- **Steps:**
  1. CEO declines to answer and marks the session escalated
  2. Operator sees a persistent "needs you" signal (visible outside the Home view) and opens the session
  3. Operator answers in the terminal; escalation clears
- **Expected result:** out-of-scope questions are never answered autonomously; the escalation is visible app-wide (persistent count/badge — A-1 confirmed), listed in the decision log as "escalated: {reason}", and clears when the session resumes
- **UI elements:** persistent needs-you badge/count (app chrome), escalated marker on session tab and Home row, decision-log escalation entry
- **States covered:** success
- **Errors & recovery:** nothing can fail visibly — escalation IS the safe path; a lost signal would be the failure, so the badge derives from live session state, not a separate event that can drop
- **Status:** validated
- **Coverage:** none yet

### SCN-049: Workflow continuation after a task ends
- **Persona:** P-01
- **Feature:** supervisor
- **Traces:** ST-035 (JTBD-10, JRN-10/#4)
- **Entry point:** an agent completes its task in a session under CEO supervision
- **Preconditions:** CEO enabled; project has a workflow and a backlog (SCN-030)
- **Steps:**
  1. Agent finishes; CEO consults the project workflow and backlog
  2. CEO picks the next task (urgent first — SCN-051), starts the agent on it, and records the hand-off
- **Expected result:** the pipeline continues without operator input; the hand-off (finished task → next task) appears in the decision log; task statuses update (done → in progress) per the existing tasks flow
- **UI elements:** decision-log hand-off entry, task status changes in the Tasks tab, session continues under a new task marker
- **States covered:** success, empty, error
- **Errors & recovery:** empty backlog / no unambiguous next task → session parks in honest idle "needs you: no next task" (never fake busy, never silent stop); hand-off failure → toast + Diagnostics + decision-log failure entry, session recoverable manually
- **Status:** validated
- **Coverage:** none yet

### SCN-050: Review decisions made while away
- **Persona:** P-03
- **Feature:** supervisor
- **Traces:** ST-036 (JTBD-10, JRN-10/#6)
- **Entry point:** decision-log surface (app chrome or Home) after time away
- **Preconditions:** autonomous decisions/escalations occurred
- **Steps:**
  1. User opens the app after being away and sees a "since you left" digest (decisions made, tasks completed/started, open escalations)
  2. User opens the decision log and reviews entries (question, answer, basis, outcome, timestamp, newest-first)
- **Expected result:** every autonomous decision is auditable; open escalations are one click from their sessions; the digest clears once seen
- **UI elements:** "since you left" digest, decision-log list, per-entry basis/outcome detail, link from escalation entry to its session
- **States covered:** empty, success
- **Errors & recovery:** no decisions while away → honest "nothing happened while you were away"; log storage degraded → StorageBanner rules apply (SCN-041) — the log never silently truncates
- **Status:** validated
- **Coverage:** none yet

## analytics

### SCN-052: Usage stats dashboard — tokens, cost, activity
- **Persona:** P-01
- **Feature:** analytics
- **Traces:** ST-038, ST-040 (JTBD-11, JRN-11/#2, JRN-11/#3)
- **Entry point:** stats view in app chrome (sidebar nav)
- **Preconditions:** usage data exists (agent sessions and/or MCP invocations)
- **Steps:**
  1. User opens the stats view
  2. User picks a range with the SegmentedPill (All | 30d | 7d)
  3. User reads tokens/cost per project and per agent, and the activity Heatmap
- **Expected result:** figures render for the chosen range; activity density shows as a Heatmap; per-project and per-agent cuts are visible; the orphan atoms SegmentedPill + Heatmap ship here (closes COV-01)
- **UI elements:** stats nav item, SegmentedPill range switcher, per-project/per-agent stat tiles, activity Heatmap, honest empty state
- **States covered:** loading, empty, error, success
- **Errors & recovery:** no data in range → honest empty state (never zeros styled as data); collection source unavailable (A-8) → per-source "data unavailable: {source}" note, remaining sources still render; fetch fails → error card + Retry + toast
- **Status:** validated
- **Coverage:** none yet

### SCN-053: Output stats — commits and code per project
- **Persona:** P-01
- **Feature:** analytics
- **Traces:** ST-039 (JTBD-11, JRN-11/#4)
- **Entry point:** stats view — output section
- **Preconditions:** workspaces with git history
- **Steps:**
  1. User opens the output section and picks a range
  2. User reads commits count and code delta per project
- **Expected result:** commit and code-change figures per project for the range, derived from workspace git; cached honestly with a "as of {time}" stamp
- **UI elements:** output stat tiles per project, freshness stamp, per-project "no git data" note
- **States covered:** loading, empty, error, success
- **Errors & recovery:** workspace without git or git read fails → that project shows honest "no git data" (never fabricated zeros); slow scan → visible loading, cancellable; scan failure → error + Retry
- **Status:** validated
- **Coverage:** none yet

## docs

### SCN-054: Project documentation
- **Persona:** P-01
- **Feature:** docs
- **Traces:** ST-041 (JTBD-06, JRN-07/#2)
- **Entry point:** project panel "Docs" tab (8th tab)
- **Preconditions:** project exists
- **Steps:**
  1. User opens the Docs tab and sees the project's document list
  2. User creates a doc (name → markdown editor), writes, saves
  3. User switches a doc between edit and rendered-preview modes
  4. User deletes a doc and confirms "delete document?"
- **Expected result:** docs persist with the project (file-backed, like rules.md) and render as formatted markdown; the list shows name + last-modified; agents can read the same files from the project directory
- **UI elements:** Docs tab, document list, "+ doc" button, name input, markdown editor, edit/preview toggle, "Save" button, "Delete" + window.confirm, "reveal file" button, empty state "No documents in this project yet."
- **States covered:** loading, empty, error, success
- **Errors & recovery:** empty name → "+ doc" blocked; save rejects → inline + toast, editor content preserved; file changed externally → "file changed externally" + Accept banner; file lost → "file lost" + Recreate (SCN-036 pattern); orchd down → Save/Delete/Accept/Recreate disabled, reading and "reveal file" stay live
- **Status:** draft
- **Coverage:** none yet

## home (v2)

### SCN-055: Home v2 — attention hub
- **Persona:** P-01
- **Feature:** home
- **Traces:** ST-042 (JTBD-01, JTBD-10, JRN-02/#2, JRN-10/#6)
- **Entry point:** app launch or sidebar "⌂ Home"; supersedes SCN-004 on ship (SCN-004 → retired), SCN-005 goals glance folds in unchanged
- **Preconditions:** sessions exist; CEO may be enabled (all blocks degrade honestly when it is not)
- **Steps:**
  1. User opens Home after time away and reads the "Since you left" strip (decisions / hand-offs / open escalations; "open log" link)
  2. User scans "Needs you" — every escalation card shows the agent's question text and a reason badge ("out of scope: {class}" / "no next task" / "waiting for input")
  3. User clicks "Go →" on the top card, answers in the terminal, returns
  4. User scans "Running" — each row shows project · current task · elapsed
  5. User scans "Continued by CEO" hand-offs ("done {task} → started {next}") and finished rows
  6. User glances the Goals section (as SCN-005)
- **Expected result:** attention is ranked: escalations first (question visible without navigation), then running with progress, then hand-offs/finished; stat tiles read needs-you / running / CEO answered today / spend today (tone shifts when needs-you > 0); the digest clears once seen; CEO-off degrades to plain waiting rows (SCN-004 behavior) with no dead chrome
- **UI elements:** "Since you left" strip + "open log" link, escalation cards (question preview, reason badge, "Go →"), waiting rows, running rows (project · task · elapsed), hand-off rows, finished rows, stat tiles ×4, Goals section, empty state "Nothing needs you."
- **States covered:** loading, empty, success
- **Errors & recovery:** orchd down → digest/CEO/task data show "orchestrator unavailable" note while session rows (sessiond) stay live — degradation equals current Home; no CEO activity while away → digest suppressed entirely (no empty ceremony); stale session state impossible (exited-wins, SCN-016)
- **Status:** draft
- **Coverage:** none yet
