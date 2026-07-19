# UX Scenarios

<!-- Managed with super-ux (scenario-format v1). Update in the same change as any user-facing behavior change. -->

## Index

| ID | Title | Feature | Persona | Status | Last audit |
|----|-------|---------|---------|--------|------------|
| SCN-001 | First launch — empty app | onboarding | new-user | implemented | 2026-07-19 PASS |
| SCN-002 | Add first workspace | onboarding | new-user | implemented | 2026-07-19 PASS |
| SCN-003 | Capture first idea with ⌘K | capture | new-user | implemented | 2026-07-19 PASS |
| SCN-004 | Home attention triage | home | owner | implemented | 2026-07-19 PASS |
| SCN-005 | Home goals overview | home | owner | implemented | 2026-07-19 PASS |
| SCN-006 | Theme toggle cycle | chrome | owner | implemented | 2026-07-19 PASS |
| SCN-007 | Sidebar navigation | chrome | owner | implemented | 2026-07-19 PASS |
| SCN-008 | Link workspace to project from sidebar | chrome | owner | implemented | 2026-07-19 PASS |
| SCN-009 | Create project | projects | owner | implemented | 2026-07-19 PASS |
| SCN-010 | Project overview & workspace management | projects | owner | implemented | 2026-07-19 PASS |
| SCN-011 | Export / import project | projects | owner | implemented | 2026-07-19 PASS |
| SCN-012 | Archive and un-archive project | projects | owner | implemented | 2026-07-19 PASS |
| SCN-013 | Open a new terminal | terminals | owner | implemented | 2026-07-19 PASS |
| SCN-014 | Switch terminal tabs (keep-alive) | terminals | owner | implemented | 2026-07-19 PASS |
| SCN-015 | Close a terminal | terminals | owner | implemented | 2026-07-19 PASS |
| SCN-016 | Session lifecycle indicators | terminals | owner | implemented | 2026-07-19 PASS |
| SCN-017 | Command history strip | terminals | owner | implemented | 2026-07-19 PASS |
| SCN-018 | Click a link in terminal output | terminals | owner | implemented | 2026-07-19 PASS |
| SCN-019 | Daemon restart reconnect | terminals | returning-owner | implemented | 2026-07-19 PASS |
| SCN-020 | Browse the file tree | files | owner | implemented | 2026-07-19 PASS |
| SCN-021 | Preview a file | files | owner | implemented | 2026-07-19 PASS |
| SCN-022 | Create / rename files and folders | files | owner | implemented | 2026-07-19 PASS |
| SCN-023 | Delete a file or folder | files | owner | implemented | 2026-07-19 PASS |
| SCN-024 | Live watch degradation and refresh | files | owner | implemented | 2026-07-19 PASS |
| SCN-025 | Manage ideas in a project | ideas | owner | implemented | 2026-07-19 PASS |
| SCN-026 | Run research on an idea | research | owner | implemented | 2026-07-19 PASS |
| SCN-027 | Form an insight from research | research | owner | implemented | 2026-07-19 PASS |
| SCN-028 | Orphan idea flows (Inbox: spawn project, link) | ideas | owner | implemented | 2026-07-19 PASS |
| SCN-029 | Manage insights | insights | owner | implemented | 2026-07-19 PASS |
| SCN-030 | Manage tasks | tasks | owner | implemented | 2026-07-19 PASS |
| SCN-031 | Manage the goal tree | goals | owner | implemented | 2026-07-19 PASS |
| SCN-032 | Build the knowledge graph | graph | owner | implemented | 2026-07-19 PASS |
| SCN-033 | MCP server lifecycle and consent | extensions | owner | implemented | 2026-07-19 PASS |
| SCN-034 | Invoke tools and connectors | extensions | owner | implemented | 2026-07-19 PASS |
| SCN-035 | Limits, call log, artifacts, skills | extensions | owner | implemented | 2026-07-19 PASS |
| SCN-036 | Edit rules and policy | rules | owner | implemented | 2026-07-19 PASS |
| SCN-037 | Sessiond disconnect and reconnect | system-status | owner | implemented | 2026-07-19 PASS |
| SCN-038 | Sessiond upgrade required | system-status | owner | implemented | 2026-07-19 PASS |
| SCN-039 | Orchd down degradation | system-status | owner | implemented | 2026-07-19 PASS |
| SCN-040 | Orchd upgrade and cancel re-entry | system-status | owner | implemented | 2026-07-19 PASS |
| SCN-041 | Storage degradation banners | system-status | owner | implemented | 2026-07-19 PASS |
| SCN-042 | Diagnostics panel | diagnostics | owner | implemented | 2026-07-19 PASS |
| SCN-043 | Render crash recovery | error-recovery | owner | implemented | 2026-07-19 PASS |
| SCN-044 | Terminal attach failure surfaced | terminals | owner | implemented | 2026-07-19 PASS |

## Personas

### new-user
Opens Builder Pro AI for the first time. No workspaces, no projects, no daemon
history. Wants to reach a working terminal + captured idea with zero reading.

### owner
The solo builder running 5–6 projects through AI coding agents. Knows the
product; opens the app to answer "where is each project, what moved, what
needs me" in under 30 seconds.

### returning-owner
Same person reopening the app (or after a daemon restart) with live sessions,
projects, and scrollback that must reappear intact.

## Scenarios

## onboarding

### SCN-001: First launch — empty app
- **Persona:** new-user
- **Feature:** onboarding
- **Entry point:** first launch, no saved state
- **Preconditions:** none
- **Steps:**
  1. User opens the app for the first time
- **Expected result:** Home view opens by default; sidebar shows "No workspaces yet — add a workspace or create a project to begin."; Home shows stat tiles (workspaces/live/waiting, all 0) and "No active sessions." with no action button; "+ project" and "+ Add workspace" CTAs visible in sidebar footer
- **UI elements:** sidebar empty-state sentence, stat tiles, EmptyState "No active sessions.", "+ project" button, "+ Add workspace" button, ThemeToggle, Diagnostics button
- **States covered:** empty
- **Errors & recovery:** daemon not yet connected → red "Daemon disconnected — reconnecting…" banner, auto-retries with backoff [500,1000,2000,5000]ms
- **Status:** implemented
- **Coverage:** src/store/store.ts:470, src/components/WorkspaceSidebar.tsx:223-235, src/strings.ts:106, src/components/HomeView.tsx:240-269, src/App.tsx:334-388

### SCN-002: Add first workspace
- **Persona:** new-user
- **Feature:** onboarding
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
- **Coverage:** src/components/WorkspaceSidebar.tsx:107-119,396-412, src/strings.ts:102,59-66

### SCN-003: Capture first idea with ⌘K
- **Persona:** new-user
- **Feature:** capture
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
- **Coverage:** src/components/QuickCapture.tsx:24-29,144-184,192,250-273, src/strings.ts:261-269

## home

### SCN-004: Home attention triage
- **Persona:** owner
- **Feature:** home
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
- **Coverage:** src/components/HomeView.tsx:205-212,240-379, src/strings.ts:120-121

### SCN-005: Home goals overview
- **Persona:** owner
- **Feature:** home
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
- **Coverage:** src/components/HomeGoals.tsx:141-201, src/strings.ts:129

## chrome

### SCN-006: Theme toggle cycle
- **Persona:** owner
- **Feature:** chrome
- **Entry point:** sidebar footer ThemeToggle button
- **Preconditions:** none
- **Steps:**
  1. User clicks the toggle repeatedly
- **Expected result:** theme cycles system → light → dark → system (icons ◐/☀/☾, aria "Theme: {current}. Click to switch."); persisted in localStorage `bpa-theme`; applied before first paint (no flash); OS appearance change tracked only in "system"
- **UI elements:** ThemeToggle button
- **States covered:** success
- **Errors & recovery:** nothing can fail (local only)
- **Status:** implemented
- **Coverage:** src/ui/ThemeToggle.tsx:8-26, src/ui/theme.ts:6-81, src/main.tsx:10

### SCN-007: Sidebar navigation
- **Persona:** owner
- **Feature:** chrome
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
- **Persona:** owner
- **Feature:** chrome
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
- **Coverage:** src/components/WorkspaceSidebar.tsx:127-130,273-313, src/strings.ts:96-97

## projects

### SCN-009: Create project
- **Persona:** owner
- **Feature:** projects
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
- **Coverage:** src/components/CreateProjectDialog.tsx:209-380, src/components/WorkspaceSidebar.tsx:378-395,453, src/strings.ts:198-207

### SCN-010: Project overview & workspace management
- **Persona:** owner
- **Feature:** projects
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
- **Coverage:** src/components/ProjectPanel.tsx:118-205,274-475, src/strings.ts:208-225

### SCN-011: Export / import project
- **Persona:** owner
- **Feature:** projects
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
- **Coverage:** src/components/ProjectPanel.tsx:198-242,413-467, src/strings.ts:218-221,227-231

### SCN-012: Archive and un-archive project
- **Persona:** owner
- **Feature:** projects
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
- **Coverage:** src/components/ProjectPanel.tsx:248-268,312-342,469-482, src/strings.ts:233-239

## terminals

### SCN-013: Open a new terminal
- **Persona:** owner
- **Feature:** terminals
- **Entry point:** workspace view tab strip "+ New terminal"
- **Preconditions:** a workspace is active
- **Steps:**
  1. User clicks "+ New terminal"
- **Expected result:** session spawns (cwd = selected file's root or roots[0]); tab appears via session://created event and auto-activates if none active; terminal pane opens
- **UI elements:** "+ New terminal" button, session tab (StatusDot + title + ×), terminal pane
- **States covered:** empty, error, success
- **Errors & recovery:** no active workspace → button disabled (not-allowed cursor); create_session rejects → toast "Failed to open a new terminal: {msg}", no tab; zero sessions → pane placeholder "No terminals yet — pick a workspace and press + New terminal."
- **Status:** implemented
- **Coverage:** src/components/TerminalTabs.tsx:57-81,166-181, src/App.tsx:132-137,522-541, src/strings.ts:188,191

### SCN-014: Switch terminal tabs (keep-alive)
- **Persona:** owner
- **Feature:** terminals
- **Entry point:** tab strip with 2+ sessions
- **Preconditions:** multiple live sessions
- **Steps:**
  1. User clicks another tab (or Enter/Space on it)
- **Expected result:** pane shows that session with full scrollback preserved; hidden sessions keep buffering output; no re-spawn, no duplicated replay
- **UI elements:** session tabs (role=tab, aria-selected), terminal pane
- **States covered:** success
- **Errors & recovery:** sessions exist but none active → "Select a terminal tab." placeholder; nothing else can fail (switch is local)
- **Status:** implemented
- **Coverage:** src/components/TerminalTabs.tsx:109-137, src/components/TerminalPane.tsx:5-59, src/terminal/terminal-manager.ts:113-122,531-536

### SCN-015: Close a terminal
- **Persona:** owner
- **Feature:** terminals
- **Entry point:** × button on a session tab
- **Preconditions:** session exists
- **Steps:**
  1. User clicks × on the tab
- **Expected result:** PTY killed, xterm disposed, tab removed; active session cleared if it was this one
- **UI elements:** tab × close button
- **States covered:** success, error
- **Errors & recovery:** kill_session rejects → toast "Failed to close the terminal: {msg}", but tab is still removed and xterm disposed (no zombie tab)
- **Status:** implemented
- **Coverage:** src/components/TerminalTabs.tsx:83-97,142-161, src/terminal/terminal-manager.ts:570-585, src/store/store.ts:505-513, src/strings.ts:192

### SCN-016: Session lifecycle indicators
- **Persona:** owner
- **Feature:** terminals
- **Entry point:** any surface with StatusDot (tabs, Home rows)
- **Preconditions:** session running
- **Steps:**
  1. User watches the dot as the session runs, waits for input, and exits
- **Expected result:** running → info dot; running + waitingForInput → warn dot "waiting for input"; atPrompt/typing → muted idle dot; exited → danger dot "exited"; exited tab stays with last scrollback until closed; a late state event cannot resurrect an exited session
- **UI elements:** StatusDot (aria labels idle/running/exited/"waiting for input"), session tab, Home rows
- **States covered:** success
- **Errors & recovery:** nothing can fail (display of pushed state); exited always wins over stale updates
- **Status:** implemented
- **Coverage:** src/components/StatusDot.tsx:13-57, src/store/store.ts:515-556, src/App.tsx:138-139

### SCN-017: Command history strip
- **Persona:** owner
- **Feature:** terminals
- **Entry point:** workspace view, strip under the terminal (per active session)
- **Preconditions:** shell integration emits OSC-133 events
- **Steps:**
  1. User runs commands in the terminal and glances at the strip
- **Expected result:** last 10 commands as chips: ✓ (exit 0), ✗ {code}, "running" (live trailing command), "interrupted" (session ended mid-command)
- **UI elements:** command strip (role=list "Command history"), chips, "[Retry]" button
- **States covered:** loading, empty, error, success
- **Errors & recovery:** loading → "Loading command history…"; fetch fails → "Failed to load command history" + Retry + toast; no events → "No commands yet"
- **Status:** implemented
- **Coverage:** src/components/CommandStrip.tsx:114-270, src/strings.ts:177-184

### SCN-018: Click a link in terminal output
- **Persona:** owner
- **Feature:** terminals
- **Entry point:** terminal output containing paths or OSC-8 hyperlinks
- **Preconditions:** session with output
- **Steps:**
  1. User clicks a path-like token (/a/b, ./a, a/b.ext) or an OSC-8 link
- **Expected result:** workspace file → file preview opens in the right rail; http(s) → OS default browser; file:// inside a root → preview
- **UI elements:** underlined links in terminal, FilesRail preview
- **States covered:** success, error
- **Errors & recovery:** file:// outside roots → toast "file is outside the workspace or not found"; non-existent lexical path → honest "not found" from the preview read; other schemes ignored
- **Status:** implemented
- **Coverage:** src/terminal/terminal-manager.ts:201-271, src/terminal/link-provider.ts:15-70,159, src/strings.ts:176

### SCN-019: Daemon restart reconnect
- **Persona:** returning-owner
- **Feature:** terminals
- **Entry point:** daemon restarts (or app reopens) with live sessions
- **Preconditions:** daemon-side sessions exist
- **Steps:**
  1. Daemon reconnects (daemon://reconnected) or app cold-boots
- **Expected result:** sessions re-hydrate as tabs; visible session eagerly re-attaches with a fresh full replay (term.reset first — no duplicated scrollback); hidden ones re-attach lazily on tab switch
- **UI elements:** session tabs, terminal pane
- **States covered:** loading, success
- **Errors & recovery:** while disconnected → red DaemonBanner (SCN-037); hydrate retries on backoff until success
- **Status:** implemented
- **Coverage:** src/App.tsx:161-181,334-344, src/terminal/terminal-manager.ts:331-333,456-467

## files

### SCN-020: Browse the file tree
- **Persona:** owner
- **Feature:** files
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
- **Coverage:** src/components/FileTree.tsx:111-168,298-415,482-491,658-700,796-805, src/components/FilesRail.tsx:22-157, src/strings.ts:134-146,155

### SCN-021: Preview a file
- **Persona:** owner
- **Feature:** files
- **Entry point:** file row click in the tree (or terminal link, SCN-018)
- **Preconditions:** workspace with files
- **Steps:**
  1. User clicks a file
- **Expected result:** preview pane (40% of rail) renders text content; binary → "Binary file · {size}"; > 1 MiB → "File too large to preview · {size}"; changed-under-read → truncation banner
- **UI elements:** preview pane, "Select a file to preview" placeholder, "Loading…", truncation banner, error card
- **States covered:** loading, empty, error, success
- **Errors & recovery:** read fails → danger card + toast "Failed to open file: {msg}" (not found / access denied / outside root / too large / io); stale responses dropped by token guard
- **Status:** implemented
- **Coverage:** src/components/FilePreview.tsx:10-26,60-160, src/ipc/fs.ts:30-45, src-tauri/src/fs_explorer.rs:37-41,294-359,482-485, src/strings.ts:50-57,166-171

### SCN-022: Create / rename files and folders
- **Persona:** owner
- **Feature:** files
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
- **Coverage:** src/components/FileTree.tsx:380-389,422-448,514-656, src/strings.ts:148-149,158-162

### SCN-023: Delete a file or folder
- **Persona:** owner
- **Feature:** files
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
- **Coverage:** src/components/FileTree.tsx:451-464, src/strings.ts:150-152

### SCN-024: Live watch degradation and refresh
- **Persona:** owner
- **Feature:** files
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
- **Coverage:** src/components/FilesRail.tsx:87-95,159-199, src/App.tsx:436-461, src/strings.ts:138

## ideas

### SCN-025: Manage ideas in a project
- **Persona:** owner
- **Feature:** ideas
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
- **Coverage:** src/components/IdeasList.tsx:21-28,155-315,334-493

### SCN-028: Orphan idea flows (Inbox: spawn project, link)
- **Persona:** owner
- **Feature:** ideas
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
- **Persona:** owner
- **Feature:** research
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
- **Persona:** owner
- **Feature:** research
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
- **Persona:** owner
- **Feature:** insights
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
- **Coverage:** src/components/InsightsList.tsx:118-282,342-352

## tasks

### SCN-030: Manage tasks
- **Persona:** owner
- **Feature:** tasks
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

## goals

### SCN-031: Manage the goal tree
- **Persona:** owner
- **Feature:** goals
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
- **Persona:** owner
- **Feature:** graph
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
- **Persona:** owner
- **Feature:** extensions
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
- **Coverage:** src/components/ext/ServersTab.tsx, src/components/ext/ConnectDialog.tsx:110-113, src/strings.ts:45,479-481,522

### SCN-034: Invoke tools and connectors
- **Persona:** owner
- **Feature:** extensions
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
- **Coverage:** src/components/ext/ToolsBrowser.tsx:81, src/components/ext/ConnectorsTab.tsx:130, src/strings.ts:460,518-519,550

### SCN-035: Limits, call log, artifacts, skills
- **Persona:** owner
- **Feature:** extensions
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
- **Coverage:** src/components/ext/InvocationLog.tsx, src/components/ext/ArtifactsTab.tsx, src/components/ext/SkillsTab.tsx, src/strings.ts:555,564-593

## rules

### SCN-036: Edit rules and policy
- **Persona:** owner
- **Feature:** rules
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
- **Coverage:** src/components/RulesetPanel.tsx:36-57, src/strings.ts:273-293

## system-status

### SCN-037: Sessiond disconnect and reconnect
- **Persona:** owner
- **Feature:** system-status
- **Entry point:** top-of-shell DaemonBanner
- **Preconditions:** sessiond connection drops
- **Steps:**
  1. Daemon disconnects
- **Expected result:** red banner "Daemon disconnected — reconnecting…" (no action needed); auto-reconnect; banner disappears on success and sessions re-attach (SCN-019)
- **UI elements:** DaemonBanner (red, role=alert, no dismiss)
- **States covered:** error, success
- **Errors & recovery:** self-healing; nothing to click
- **Status:** implemented
- **Coverage:** src/components/DaemonBanner.tsx:61-77, src/App.tsx:160,359

### SCN-038: Sessiond upgrade required
- **Persona:** owner
- **Feature:** system-status
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
- **Coverage:** src/components/UpgradeDialog.tsx:141-225, src/components/DaemonBanner.tsx:25-59, src/App.tsx:183-192,364-377, src/strings.ts:77,81-87

### SCN-039: Orchd down degradation
- **Persona:** owner
- **Feature:** system-status
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
- **Coverage:** src/components/OrchdDownBanner.tsx:45-54, src/App.tsx:248-303,486, src/components/ProjectPanel.tsx:281

### SCN-040: Orchd upgrade and cancel re-entry
- **Persona:** owner
- **Feature:** system-status
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
- **Coverage:** src/components/UpgradeDialog.tsx:141-143,230-267, src/components/OrchdUpgradeBanner.tsx:48-56, src/App.tsx:311-315, src/strings.ts:78,88-90

### SCN-041: Storage degradation banners
- **Persona:** owner
- **Feature:** system-status
- **Entry point:** orchd storage status on connect/reconnect
- **Preconditions:** orchd database degraded
- **Steps:**
  1. App connects to orchd running degraded
- **Expected result:** red banner — in-memory: "Storage unavailable — running in memory. Changes will NOT survive a restart."; corruption: "Database was corrupted and has been reset. The damaged copy was saved to {path}."; persists until a healthy daemon restart (no dismiss, no in-app recovery)
- **UI elements:** StorageBanner (red, role=alert)
- **States covered:** error
- **Errors & recovery:** honest permanent warning; recovery is external (restart daemon healthy)
- **Status:** implemented
- **Coverage:** src/components/StorageBanner.tsx:28-39, src/App.tsx:301,401, src/strings.ts:598-600

## diagnostics

### SCN-042: Diagnostics panel
- **Persona:** owner
- **Feature:** diagnostics
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
- **Coverage:** src/components/DiagnosticsPanel.tsx:20-86, src/ipc/diag.ts:28,62-103, src/components/WorkspaceSidebar.tsx:413-451, src/store/store.ts:642-678

## error-recovery

### SCN-043: Render crash recovery
- **Persona:** owner
- **Feature:** error-recovery
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
- **Persona:** owner
- **Feature:** terminals
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
- **Coverage:** src/terminal/terminal-manager.ts:51-74,133,427-431,470-483,644-668, src/components/TerminalPane.tsx:43-49,80-118, src/strings.ts:183-186
