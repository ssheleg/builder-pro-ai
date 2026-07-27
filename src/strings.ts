/**
 * Central UI copy registry (spec D1, O-2). Single source of every user-facing string in the
 * webview — English-only, no runtime i18n framework, no locale switch (YAGNI: the standing rule is
 * "always English", not "translatable"). Every component references a key here
 * (`strings.research.run`) instead of an inline literal, and component tests assert against these
 * same keys so copy changes never silently break a test.
 *
 * Grouped by surface area. Leaf values are English strings; parameterized copy is a function
 * `(x) => `…${x}…``. Rust-side user-facing strings have no equivalent here — they are translated in
 * place (spec D1).
 */
export const strings = {
  // ── generic, reused across surfaces ──────────────────────────────────────────────────────────
  common: {
    cancel: "Cancel",
    save: "Save",
    delete: "Delete",
    create: "Create",
    add: "Add",
    retry: "Retry",
    close: "Close",
    update: "Update",
    accept: "Accept",
    moveUp: "Move up",
    moveDown: "Move down",
    descriptionOptional: "description (optional)",
    noVerdict: "— no verdict —",
    argsInvalidJson: "arguments must be valid JSON",
    /** Policy scope labels, shared by the research run dialog and the extensions log tab. */
    scope: { global: "global", project: "project", server: "server" },
  },

  // ── self-update (Tauri updater plugin → GitHub Releases) ─────────────────────────────────────
  updater: {
    available: (version: string) =>
      `Builder Pro AI ${version} is available.\n\nInstall now? The app will restart and any running terminals will be interrupted (their history is preserved).`,
    availableWithNotes: (version: string, notes: string) =>
      `Builder Pro AI ${version} is available.\n\nInstall now? The app will restart and any running terminals will be interrupted (their history is preserved).\n\nRelease notes:\n${notes}`,
    installFailed:
      "The update could not be installed. Check your connection and try again, or download it manually from the GitHub Releases page.",
  },

  // ── error mapping: orchd, filesystem, and daemon command errors ──────────────────────────────
  errors: {
    invariant: (msg: string) => `invalid operation: ${msg}`,
    conflict: (msg: string) => `conflict: ${msg}`,
    notFound: "not found",
    validation: (msg: string) => `invalid data: ${msg}`,
    io: (msg: string) => `service error: ${msg}`,
    consent: (msg: string) => `connection consent required: ${msg}`,
    policy: (msg: string) => `blocked by policy: ${msg}`,
    /** Recovery hint appended to a `Consent`-kind toast (P-20): `ConnectDialog` is only reachable
     * from the Servers tab, so a consent denial must point there instead of dead-ending. Consumed
     * by `ToolsBrowser`/`ConnectorsTab` via `isConsentError` (see `ipc/orchd.ts`). */
    consentRecovery: "To reconnect, open Extensions → Servers → Connect.",
    orchdError: "orchestrator error",
    unavailable: "orchestrator unavailable",
    unknown: "unknown orchestrator error",
    /** `describeFsError` branches (FilePreview, FileTree). */
    fs: {
      notFound: "file not found",
      noAccess: "access denied",
      outsideRoot: "path is outside the workspace root",
      tooLarge: "file is too large",
      alreadyExists: "a file with this name already exists",
      io: "I/O error",
      disconnected: "daemon disconnected",
    },
    /** `describeCommandError` branches (FileTree). */
    command: {
      daemon: "daemon error",
      disconnected: "daemon disconnected",
      internal: "internal error",
      incompatible: "daemon incompatible — update required",
      failed: "operation failed",
      tooLarge: "request too large",
    },
  },

  // ── chrome: banners, upgrade dialog, sidebar, toasts ─────────────────────────────────────────
  chrome: {
    theme: {
      system: "System theme",
      light: "Light theme",
      dark: "Dark theme",
      toggleAria: (current: string) => `Theme: ${current}. Click to switch.`,
    },
    daemonOutdated: "Background service is outdated — update required",
    /** Red auto-reconnect banner (AUD-2026-07-19-05 — was an inline literal in DaemonBanner). */
    daemonDisconnected: "Daemon disconnected — reconnecting…",
    orchdOutdated: "Orchestrator service is outdated — update required",
    orchdUnavailable: "Orchestrator unavailable",
    upgrade: {
      required: "Update required",
      daemonDetail: (n: number) =>
        `Update the background service — ${n} live sessions will end. Their records and scrollback are saved and will reappear as inactive.`,
      daemonDetailAll:
        "Update the background service — all of its live sessions will end. Their records and scrollback are saved and will reappear as inactive.",
      daemonRestartFailed: (err: string) =>
        `Failed to restart the background service: ${err}. Check permissions (launchctl) and try again.`,
      orchdBody: "Update the orchestrator background service — records (projects, goals, tasks) are saved",
      orchdRestartFailed: (err: string) =>
        `Failed to restart the orchestrator background service: ${err}. Check permissions (launchctl) and try again.`,
    },
    sidebar: {
      extensions: "Extensions",
      extensionsNav: "⚙ Extensions",
      /** Orphan-idea inbox nav (AUD-2026-07-19-11): the surface for ⌘K captures with "no
       * project". Count badge = orphan ideas + orphan insights, shown only when > 0. */
      inbox: "Inbox",
      inboxNav: "✉ Inbox",
      noProject: "No project",
      linkToProject: (name: string) => `Link ${name} to a project`,
      linkPlaceholder: "link…",
      addProject: "+ project",
      addWorkspace: "+ Add workspace",
      addWorkspaceAria: "Add workspace",
      /** Honest toast for a rejected `pickFolder`/`createWorkspace` (BL-93 — no more silent no-op). */
      addWorkspaceFailed: (msg: string) => `Failed to add workspace: ${msg}`,
      /** Dim empty-state sentence shown when there are zero projects AND zero workspaces (P-11) —
       * the bare «No project» header is otherwise the only thing on screen. The CTAs below it
       * ("+ project" / "+ Add workspace") stay visible; this is onboarding copy, not a dead end. */
      emptyState: "No workspaces yet — add a workspace or create a project to begin.",
      /** Header of the collapsed, dimmed group of archived projects (O-3, spec D7). */
      archivedGroup: (count: number) => `Archived (${count})`,
      archivedGroupToggleAria: "Toggle archived projects",

      // ── remove a workspace (SCN-058) ───────────────────────────────────────────────────────
      /** Per-row control. The glyph is the affordance; this is its accessible name. */
      removeWorkspaceAria: (name: string) => `Remove workspace ${name}`,
      /**
       * SCN-058 requires the CONSEQUENCE to be stated before anything is committed: the workspace
       * is named, and the fact that its live terminals get closed and their scrollback discarded
       * is spelled out in the confirmation itself, not discovered afterwards.
       */
      removeWorkspaceConfirm: (name: string) =>
        `Remove workspace "${name}"? Its terminals will be closed and their scrollback discarded. This cannot be undone.`,
      /** Honest toast for a rejected `remove_workspace` — the row stays exactly where it was. */
      removeWorkspaceFailed: (msg: string) => `Failed to remove workspace: ${msg}`,

      // ── clear out workspaces whose folder is gone (SCN-059) ────────────────────────────────
      /** Row marker: this workspace's folder(s) no longer exist on disk. */
      rootMissing: "folder missing",
      /** Full explanation behind the marker (title attribute) — names the path that is gone. */
      rootMissingTitle: (path: string) => `Folder no longer exists on disk: ${path}`,
      /** Bulk clean-up control, shown ONLY when at least one workspace is missing (no dead
       * control) — states the exact count it would remove. */
      cleanupMissing: (count: number) =>
        `Clean up ${count} missing ${count === 1 ? "workspace" : "workspaces"}`,
      /** SCN-059 step 2: the confirmation states exactly how many will be removed. */
      cleanupMissingConfirm: (count: number) =>
        `Remove ${count} ${count === 1 ? "workspace whose folder" : "workspaces whose folders"} no longer ${count === 1 ? "exists" : "exist"}? Their terminals will be closed and their scrollback discarded. This cannot be undone.`,
      /** SCN-059 "no silent partial success": the successes stand, and the toast names how many
       * of the attempted removals failed. */
      cleanupMissingPartial: (failed: number, total: number) =>
        `Removed ${total - failed} of ${total} missing workspaces — ${failed} failed`,
    },
  },

  // ── Home (attention-first view + goals rail) ─────────────────────────────────────────────────
  home: {
    running: "running",
    atPrompt: "at prompt",
    exited: "exited",
    exitedWithError: "exited with error",
    waitingForInput: "waiting for input",
    noActiveSessions: "No active sessions.",
    openWorkspace: (name: string) => `Open ${name}`,
    needsYou: "Needs you",
    go: "Go →",
    runningSection: "Running",
    recentlyFinished: "Recently finished",
    ok: "success",
    withError: "with error",
    goals: "Goals",
    goalsLoading: "Goals are loading…",
  },

  /**
   * Session-accounting vocabulary shared by Home and the workspace stat chips (SCN-004/SCN-016).
   * The four buckets are an exhaustive partition (`store.ts::partitionSessions`); this is the
   * copy for the one bucket that had no name before — a session restored from the daemon's store
   * whose shell is gone. It is deliberately NOT called "live": there is no PTY behind it.
   */
  sessions: {
    /** Workspace stat chip (spec §6.3), alongside "N live · K waiting · M exited". */
    restoredChip: (count: number) => `${count} restored`,
    /** Home section heading for the same bucket. */
    restoredSection: "Restored (no live shell)",
    /** Per-row meta text on Home — what the owner can still expect from such a session. */
    restoredNote: "restored — scrollback only",
    /** StatusDot state for the same bucket (FE-7): a restored session is NOT "idle" — idle implies
     * a live shell at its prompt. */
    restoredDotLabel: "restored — no live shell",
    /** One dim line under the section heading, so the state is explained where it is shown. */
    restoredHint:
      "These came back after a restart: their scrollback was kept, but the shell that produced it is gone.",
  },

  // ── files: rail, tree, preview ───────────────────────────────────────────────────────────────
  files: {
    openPanel: "Open the file panel",
    collapsePanel: "Collapse the file panel",
    title: "Files",
    showIgnored: "show ignored",
    liveUpdatesPaused: "live updates paused — refresh",
    loading: "Loading…",
    /** Marker for an expanded directory that loaded successfully but has no entries (P-12) —
     * distinct from the "Loading…" placeholder (a still-fetching dir) and from a failed load
     * (which toasts). Renders as a dim, non-interactive row under the empty directory. */
    emptyFolder: "empty folder",
    fileWord: "file",
    folderWord: "folder",
    readFolderFailed: (msg: string) => `Failed to read folder: ${msg}`,
    refreshFolderFailed: (msg: string) => `Failed to refresh folder: ${msg}`,
    createFailed: (what: string, msg: string) => `Failed to create ${what}: ${msg}`,
    renameFailed: (msg: string) => `Failed to rename: ${msg}`,
    deleteConfirm: (label: string, rel: string) =>
      `Delete ${label} "${rel}"? It will be moved to the Trash.`,
    deleteFailed: (msg: string) => `Failed to delete: ${msg}`,
    revealFailed: (msg: string) => `Failed to reveal in Finder: ${msg}`,
    openExternalFailed: (msg: string) => `Failed to open in external app: ${msg}`,
    addRootFailed: (msg: string) => `Failed to add root: ${msg}`,
    menuAria: (name: string) => `Menu: ${name}`,
    actionsAria: (name: string) => `Actions: ${name}`,
    newFile: "New file",
    newFolder: "New folder",
    rename: "Rename",
    reveal: "Show in Finder",
    openExternal: "Open in external app",
    newFileNamePlaceholder: "New file name",
    newFolderNamePlaceholder: "New folder name",
    newNamePlaceholder: "New name",
    openFileFailed: (msg: string) => `Failed to open file: ${msg}`,
    selectFile: "Select a file to preview",
    binaryFile: (size: string) => `Binary file · ${size}`,
    tooLargePreview: (size: string) => `File too large to preview · ${size}`,
    contentMayHaveChanged:
      "The content may have changed while reading — a partial result is shown.",
  },

  // ── terminal + command-history strip ─────────────────────────────────────────────────────────
  terminal: {
    fileOutsideWorkspace: "file is outside the workspace or not found",
    /** Pane-level honest surface for a rejected `attach_session` (AUD-2026-07-19-01) — without
     * it a failed attach was a silently blank terminal (both call sites `void` the promise). */
    attachFailed: (msg: string) => `Terminal could not attach: ${msg}`,
    attachRetry: "Retry",
    /** One-time-per-session hint (FE-7) when the owner types into a RESTORED session (no live PTY
     * behind it): the keystroke has nowhere to go, so the input must not be swallowed silently. */
    restoredInputHint:
      "This session was restored — its shell is gone. Start a new terminal to keep working.",
    commandHistory: "Command history",
    noCommands: "No commands yet",
    /** In-flight placeholder while the first `getCommandEvents` is still resolving (P-13) — kept
     * DISTINCT from `noCommands` so a loading strip never reads as a genuinely empty one. */
    loadingCommands: "Loading command history…",
    loadHistoryFailed: "Failed to load command history",
    interrupted: "interrupted",
    interruptedTitle: "Interrupted — the session ended before the command finished",
    /** Terminal-tab strip (new/close). `newTerminalFailed`/`closeTerminalFailed` are the honest
     * toasts for a rejected `create_session`/`kill_session` (BL-93 — no more silent no-op). */
    tabs: {
      newTerminal: "+ New terminal",
      newTerminalAria: "New terminal",
      closeAria: (title: string) => `Close ${title}`,
      newTerminalFailed: (msg: string) => `Failed to open a new terminal: ${msg}`,
      closeTerminalFailed: (msg: string) => `Failed to close the terminal: ${msg}`,
    },
    /** Pane-level empty/loading placeholders (AUD2-2026-07-19-04: were inline literals in App.tsx,
     *  outside the central strings catalog). */
    noTerminals: "No terminals yet — pick a workspace and press + New terminal.",
    selectTerminalTab: "Select a terminal tab.",
  },

  // ── project panel + create-project dialog ────────────────────────────────────────────────────
  project: {
    workspaceRequired: "at least one workspace is required",
    createWorkspaceFailed: "failed to create workspace",
    projectCreated: "Project created",
    newProject: "New project",
    nameLabel: "Name",
    nameAria: "Project name",
    descriptionLabel: "Description",
    descriptionAria: "Project description",
    noFreeWorkspaces: "no available workspaces",
    createWorkspace: "+ create workspace",
    tabs: {
      overview: "Overview",
      goals: "Goals",
      ideas: "Ideas",
      tasks: "Tasks",
      insights: "Insights",
      rules: "Rules",
      graph: "Graph",
      docs: "Docs",
    },
    loading: "Loading project…",
    jsonCopied: "JSON copied",
    exportedToFile: "Exported to file",
    importSummary: (r: { projects: number; goals: number; ideas: number; insights: number; tasks: number }) =>
      `Imported: projects ${r.projects}, goals ${r.goals}, ideas ${r.ideas}, insights ${r.insights}, tasks ${r.tasks}`,
    workspaceUnavailable: "workspace unavailable",
    unlink: "Unlink",
    addWorkspaceAria: "Add workspace",
    addWorkspaceOption: "+ add workspace…",
    exportLabel: "Export",
    copyJson: "Copy JSON",
    saveToFile: "Save to file…",
    importLabel: "Import",
    importFromFile: "Import from file…",
    noJsonFiles: "No .json files in the selected folder",
    // ── archive / un-archive (O-3, spec D7) ──
    dangerLabel: "Danger zone",
    archive: "Archive project",
    archiveConfirm: "Archive this project? It becomes read-only until you un-archive it.",
    archived: "Project archived",
    /** Read-only banner shown at the top of an archived project's Overview (spec D7). */
    archivedBanner: "This project is archived and read-only. Un-archive it to make changes.",
    unarchive: "Un-archive",
  },

  // ── goal tree ────────────────────────────────────────────────────────────────────────────────
  goals: {
    status: { active: "active", achieved: "achieved", dropped: "dropped" },
    deleteConfirm: "delete the entire branch?",
    newSubgoal: "new goal",
    titleAria: "Goal title",
    statusAria: "Goal status",
    addSubgoal: "+ subgoal",
    empty: "The goal tree is empty.",
    /** First-fetch placeholder (UX-1): shown until `goalsFetched[projectId]` flips — kept DISTINCT
     * from `empty` so a still-loading tree never flashes the false empty state. */
    loading: "Loading goals…",
    treeAria: "Goal tree",
    /** metric_refs chip editor (O-4, spec D7): the row's success-metric references — added via the
     * text input + Enter, each removable via its chip's ×. */
    metricRefsAria: "Metric references",
    addMetricAria: "Add metric reference",
    addMetricPlaceholder: "+ metric",
    removeMetricAria: (ref: string) => `Remove metric ${ref}`,
  },

  // ── quick capture (⌘K new idea) ──────────────────────────────────────────────────────────────
  capture: {
    ideaSaved: "idea saved",
    newIdea: "New idea",
    titleAria: "Idea title",
    titlePlaceholder: "title",
    descriptionAria: "Idea description",
    projectAria: "Project",
    noProject: "no project",
  },

  // ── Inbox: orphan ideas/insights captured with "no project" (AUD-2026-07-19-11) ─────────────
  inbox: {
    title: "Inbox",
    /** One-line explainer so the surface is self-describing on first visit. */
    subtitle: "Ideas and insights captured without a project. Link them or spawn a project.",
    ideasSection: "Ideas",
    insightsSection: "Insights",
  },

  // ── ruleset panel ────────────────────────────────────────────────────────────────────────────
  rules: {
    missingBanner: "file lost",
    modifiedBanner: "file changed externally",
    spendCapNotNumber: "spend cap must be a number",
    spendCapNegative: "spend cap cannot be negative",
    emptyEntry: "empty entries are not allowed",
    deleteEntry: (v: string) => `Delete ${v}`,
    addEntry: "+ add",
    loading: "Loading rules…",
    revealFile: "reveal file",
    recreate: "Recreate",
    contentAria: "Rules content",
    spendCapLabel: "Spend cap, $",
    spendCapAria: "Spend cap in dollars, empty — no limit",
    spendCapPlaceholder: "no limit",
    /** SEC-4 honesty note under every spend-cap control: the cap is INERT until MCP servers
     * report per-call costs (the orchestrator cannot stop what it cannot price). */
    spendCapInertHint:
      "Enforced once servers report call costs — until then the cap is stored but does not block calls.",
    confirmClassesLabel: "Classes requiring confirmation",
    confirmClassAria: "New confirmation class",
    confirmClassPlaceholder: "class",
    allowedPathsLabel: "Allowed paths",
    allowedPathAria: "New allowed path",
    allowedPathPlaceholder: "path",
    savePolicy: "Save policy",

    // ── CEO supervisor section (SCN-046, FLW-19, A-7) — the per-project delegation config that
    // rides inside PolicyRules. PLUMBING ONLY: `pendingNote` is the honesty-boundary equivalent of
    // the Skills tab's registry banner — persisting this config does not start a CEO; the
    // orchestrator-agent runtime that acts on it lands in S6b (SCN-047/049). ──
    supervisor: {
      sectionLabel: "CEO supervisor",
      /** Verbatim honesty-boundary note (S6b) — mirrors the Skills tab's registry banner register.
       * Persisting the config never makes a CEO act; the runtime that reads it lands in S6b. */
      pendingNote: "The CEO acts on this once the orchestrator agent runtime lands (S6b).",
      enableLabel: "Enable the CEO",
      enableAria: "Enable the CEO supervisor",
      /** Progressive disclosure (PRN-11): the only detail shown while the CEO toggle is OFF — the
       * delegation/scope controls stay collapsed until enabled so a disabled CEO never reads as an
       * active grant. */
      disabledHint: "Enable to configure delegation.",
      delegatedLabel: "Delegated confirmation classes",
      /** Empty-universe hint: no confirmation classes exist yet to delegate (define some in the
       * policy above, or seed the recommended scope). */
      noClasses: "No confirmation classes yet — add classes above or use the recommended scope.",
      delegateClassAria: (c: string) => `Delegate the "${c}" class to the CEO`,
      inheritedCapsLabel: "Inherited caps",
      inheritedSpendCap: (cap: string) => `spend cap $${cap}`,
      inheritedNoSpendCap: "no spend cap",
      recommendedScope: "Recommended scope",
      instructionLabel: "CEO instruction",
      instructionAria: "CEO instruction (markdown the CEO must follow)",
      instructionPlaceholder: "What the CEO must always follow (markdown)…",
      customRulesLabel: "Custom rules",
      customRuleAria: "New custom CEO rule",
      customRulePlaceholder: "rule",
      /** SCN-046 locked info-access summary line. */
      infoAccess:
        "CEO reads: project goals, tasks, ideas, insights, graph, rules + your instruction",
      /** SCN-046 scope summary: "CEO may: {classes} within {caps}". */
      scopeSummary: (classes: string, caps: string) => `CEO may: ${classes} within ${caps}`,
      scopeSummaryNoClasses: "no classes delegated",
      /** Placeholder for the not-yet-built MCP-tool delegation (S6b-adjacent). */
      mcpSoon: "MCP tools for the CEO — soon",
      /** SCN-046 blocked alert: enabled CEO with an empty delegation scope. */
      blockedNoClasses: "delegate at least one class or disable the CEO",
    },
  },

  // ── docs panel (SCN-054: per-project markdown documents, the 8th project tab) ────────────────
  docs: {
    /** Locked banner copy — SCN-054 reuses the rules-file pattern's exact register ("file lost" /
     * "file changed externally", see `rules` above); duplicated rather than cross-referenced so
     * either surface's copy can evolve without silently rewording the other. */
    missingBanner: "file lost",
    modifiedBanner: "file changed externally",
    recreate: "Recreate",
    revealFile: "reveal file",
    loading: "Loading documents…",
    loadingDoc: "Loading document…",
    /** SCN-054 locked empty-state copy. */
    empty: "No documents in this project yet.",
    addDoc: "+ doc",
    nameAria: "New document name",
    namePlaceholder: "doc-name",
    /** Client-side mirror of the daemon's `validate_doc_name` character class — shown inline
     * when the typed name would be rejected (the daemon stays the authoritative validator). */
    invalidName: "name may only contain a-z, 0-9, '.', '_' and '-'",
    /** "+ doc" guard: `UpsertDoc` is deliberately upsert-shaped on the wire (the rules-template
     * minimal verb set), so creating over an existing name would blank that doc's file — the
     * client blocks it against the (push-fresh) list instead. */
    duplicateName: "a document with this name already exists",
    listAria: "Documents",
    docRowAria: (name: string) => `Open ${name}`,
    deleteConfirm: "delete document?",
    editorAria: "Document content",
    modeAria: "Editor mode",
    modeEdit: "Edit",
    modePreview: "Preview",
    selectPrompt: "Select a document or create one.",
    /** Relative last-modified stamps for the doc list (SCN-054 "name + last-modified"). */
    justNow: "just now",
    minutesAgo: (n: number) => `${n}m ago`,
    hoursAgo: (n: number) => `${n}h ago`,
    daysAgo: (n: number) => `${n}d ago`,
  },

  // ── ideas list (+ spawn-project flow) ────────────────────────────────────────────────────────
  ideas: {
    deleteConfirm: "delete idea?",
    lifecycle: {
      captured: "captured",
      researching: "researching",
      specced: "specced",
      inDev: "in development",
      shipped: "shipped",
      archived: "archived",
    },
    titleAria: "Idea title",
    stageAria: "Idea stage",
    research: "Research",
    hideResearch: "hide research",
    researchCount: (n: number) => `research (${n})`,
    descriptionAria: "Idea description",
    linkToProjectAria: "Link to project",
    selectProject: "select a project…",
    linkToProject: "link to project",
    newTitleAria: "New idea title",
    newTitlePlaceholder: "idea title",
    newDescriptionAria: "New idea description",
    addIdea: "+ idea",
    emptyOrphan: "No ideas without a project.",
    emptyProject: "No ideas in this project yet.",
    /** First-fetch placeholder (UX-1): shown until `ideasFetched` flips — kept DISTINCT from the
     * two empty states so a still-loading list never flashes a false empty state. */
    loading: "Loading ideas…",
    spawn: {
      folderPickerFailed: "failed to open the folder picker",
      createdFromIdea: "Project created from idea",
      createProject: "Create project",
      /** Resume label after a partial failure — the retry resumes from the failed step (spec D6). */
      retry: "Retry linking",
      /** Partial-failure message (spec D6, BL-95/P-09): the project + workspace WERE created, but
       * linking the idea failed. Names what was created + why + that retry won't duplicate. */
      linkFailed: (title: string, reason: string) =>
        `Project "${title}" and its workspace were created, but linking the idea to it failed: ${reason}. Retry to finish — it will not create a second project.`,
    },
  },

  // ── research (run dialog + status pane) ──────────────────────────────────────────────────────
  research: {
    runStatus: { pending: "pending", running: "running", done: "done", failed: "failed" },
    runStarted: "Research run started",
    dialogTitle: (title: string) => `Research "${title}"`,
    serverLabel: "Server",
    serverAria: "MCP server",
    selectServer: "select a server…",
    toolLabel: "Tool",
    toolAria: "Tool",
    selectTool: "select a tool…",
    argsLabel: "Arguments (JSON)",
    argsAria: "Call arguments",
    limitScope: "limit scope:",
    notSet: "not set",
    spendCap: "spend cap:",
    callsPerMin: "calls/min limit:",
    costNote:
      "the cost of an external call is usually unknown in advance — the orchestrator will stop the call only if it exceeds the current limit.",
    run: "Run",
    emptyRuns: "no research for this idea yet",
    /** First-fetch placeholder (UX-1): shown until `researchRunsFetched[ideaId]` flips — kept
     * DISTINCT from `emptyRuns` so a still-loading pane never flashes the false empty state. */
    loadingRuns: "Loading research…",
    showArtifact: "show artifact",
    formInsight: "Form insight",
    formInsightNoResearch: "form insight without research",
    unknownError: "unknown error",
  },

  // ── insights list + form-insight dialog ──────────────────────────────────────────────────────
  insights: {
    archiveReasonRequired: "an archive reason is required",
    fitVerdict: { fit: "fit", noFit: "no fit", unknown: "unclear" },
    status: { new: "new", accepted: "accepted", archived: "archived" },
    statusAria: "Insight status",
    sourceLabel: "source:",
    archiveReasonAria: "Archive reason",
    archiveReasonPlaceholder: "archive reason",
    confirmArchival: "confirm archival",
    ownerVerdictAria: "Owner verdict",
    verdictReasoningAria: "Verdict reasoning",
    verdictReasoningPlaceholder: "reasoning",
    applyVerdict: "apply verdict",
    emptyOrphan: "No insights without a project.",
    emptyProject: "No insights in this project yet.",
    /** First-fetch placeholder (UX-1): shown until `insightsFetched` flips — kept DISTINCT from
     * the two empty states so a still-loading list never flashes a false empty state. */
    loading: "Loading insights…",
    form: {
      created: "Insight created",
      addedToBacklog: "Task added to backlog",
      title: "Form insight",
      nameLabel: "Name",
      nameAria: "Insight title",
      descriptionLabel: "Description",
      descriptionAria: "Insight description",
      ownerVerdictLabel: "Owner verdict",
      ownerVerdictAria: "Owner verdict",
      reasoningLabel: "Reasoning",
      reasoningAria: "Verdict reasoning",
      status: (status: string) => `insight status: ${status}`,
      contextTitle: "Assessment context",
      ideaNotLinked: "the idea is not linked to a project — context unavailable",
      projectGoals: "Project goals",
      noGoals: "no goals yet",
      metrics: (m: string) => ` — metrics: ${m}`,
      relatedGraph: "Related graph",
      noGraphNode: "no graph node for this idea yet",
      noRelatedNodes: "no related nodes",
      toBacklog: "To backlog",
      /** Partial-failure message (spec D6, BL-95/G-08): the task WAS created, but the idea
       * lifecycle flip to "specced" failed. Names what was created + that retry won't duplicate. */
      backlogResume: (reason: string) =>
        `The task was created, but moving the idea to "specced" failed: ${reason}. Retry to finish — it will not create a duplicate task.`,
    },
  },

  // ── tasks list ───────────────────────────────────────────────────────────────────────────────
  tasks: {
    status: {
      backlog: "backlog",
      todo: "to do",
      waiting: "waiting",
      progress: "in progress",
      testing: "testing",
      done: "done",
    },
    source: { idea: "idea", insight: "insight", bug: "bug", plan: "plan" },
    // SCN-051 (ST-037): urgent/normal priority — the row/create-form select options and the
    // danger-tone chip label on urgent rows.
    priority: { urgent: "urgent", normal: "normal" },
    priorityAria: "Task priority",
    newPriorityAria: "New task priority",
    deleteConfirm: "delete task?",
    deleteConfirmWithChildren: (n: number) => `delete task? will delete ${n} subtasks`,
    statusAria: "Task status",
    newTitleAria: "New task title",
    newTitlePlaceholder: "task title",
    newDescriptionAria: "New task description",
    newSourceAria: "New task source",
    parentAria: "Parent task",
    noParent: "no parent",
    newTagsAria: "New task tags (comma-separated)",
    tagsPlaceholder: "comma-separated tags",
    addTask: "+ task",
    empty: "no tasks",
    /** First-fetch placeholder (UX-1): shown until `tasksFetched[projectId]` flips — kept DISTINCT
     * from `empty` so still-loading groups never flash the false empty state. */
    loading: "Loading tasks…",
  },

  // ── graph canvas ─────────────────────────────────────────────────────────────────────────────
  graph: {
    deleteConfirm: "delete selection?",
    sourceRemoved: "source removed",
    newNodeTypeAria: "New node type",
    deleteSelection: "Delete selection",
    searchAria: "Search the graph",
    searchPlaceholder: "search…",
    empty: "empty",
    // add-node form (P4-T5, spec D7 O-7)
    titleAria: "New node title",
    titlePlaceholder: "node title",
    bodyAria: "New node body",
    bodyPlaceholder: "body (optional)",
    addNode: "Add node",
    // inline rename (double-click a local node)
    renameAria: "Rename node",
    renameSave: "Save",
    renameCancel: "Cancel",
    // edge kind editing (select an edge)
    edgeKindAria: "Edge kind",
    edgeKindLabel: "edge:",
  },

  // ── extensions view (all ext/* tabs) ─────────────────────────────────────────────────────────
  ext: {
    panelTitle: "Extensions",
    unverified: "⚠ unverified data",
    invoke: "invoke",
    delete: "delete",
    projectSoon: "project (soon)",
    tabs: {
      servers: "Servers",
      tools: "Tools",
      connectors: "Connectors",
      log: "Log",
      artifacts: "Artifacts",
      skills: "Skills",
    },
    artifacts: {
      projectLabel: "project:",
      toggleHide: "hide",
      toggleShow: "show content",
      empty: "no artifacts",
      /** First-fetch placeholder (UX-1): shown until `mcpArtifactsFetched` flips — kept DISTINCT
       * from `empty` so a still-loading tab never flashes the false empty state. */
      loading: "Loading artifacts…",
    },
    connectDialog: {
      title: (name: string) => `Connect to server "${name}"`,
      body: "The app will connect to this MCP server and gain access to its tools.",
      connect: "Connect",
    },
    connectors: {
      apiKeyLabel: "API key",
      accountConnected: "Account connected",
      deleteConfirm: (label: string) => `delete account "${label}"?`,
      accountsTitle: "Accounts",
      noAccounts: "no accounts",
      scopesLabel: "scopes:",
      expiresLabel: "expires:",
      operationFor: (label: string) => `Operation for ${label}`,
      operationOption: "— operation —",
      /** Ops-list load failed (P-15): distinguishes a failed `connectorListOps` (retryable) from a
       * genuinely empty op catalog — the select alone can't tell the two apart. */
      opsLoadFailed: "Failed to load operations.",
      argsFor: (label: string) => `Arguments for ${label}`,
      operationError: "the operation returned an error",
      addApiKeyTitle: "Add API key",
      providerAria: "Provider",
      providerPlaceholder: "provider",
      labelAria: "Label",
      labelPlaceholder: "label",
      apiKeyAria: "API key",
      apiKeyPlaceholder: "API key",
      addApiKey: "+ API key",
      connectOAuthTitle: "Connect OAuth",
      scopesAria: "Scopes",
      scopesPlaceholder: "scopes, comma-separated (optional)",
      startOAuth: "start OAuth",
      openAuthPage: "open authorization page",
      codeAria: "Authorization code",
      codePlaceholder: "paste the code from the redirect",
      finish: "finish",
      /** OAuth provider dropdown (spec D7, O-5): fed by connectorListProviders(). */
      oauthProviderAria: "OAuth provider",
      oauthProviderOption: "— provider —",
      /** Honest empty-state when the config-backed provider registry is empty. */
      noProviders:
        "No OAuth providers configured — add one in oauth_providers.json (see runbook).",
    },
    servers: {
      authKind: { none: "no authorization", bearer: "bearer token", oauth: "OAuth (soon)" },
      deleteConfirm: (name: string) => `delete server "${name}"?`,
      tokenSaved: "Token saved",
      nameAria: "Server name",
      namePlaceholder: "name",
      transportAria: "Transport",
      stdioSoon: "stdio (soon)",
      urlAria: "Server URL",
      scopeAria: "Scope",
      authAria: "Authorization",
      addServer: "+ server",
      empty: "no servers",
      /** First-fetch placeholder (UX-1): shown until `mcpServersFetched` flips — kept DISTINCT
       * from `empty` so a still-loading tab never flashes the false empty state. */
      loading: "Loading servers…",
      protocol: (v: string) => `protocol ${v}`,
      notConnected: "not yet connected",
      disable: "disable",
      enable: "enable",
      connect: "connect",
      disconnect: "disconnect",
      tokenFor: (name: string) => `Token for ${name}`,
      bearerPlaceholder: "bearer token",
      setToken: "set token",
    },
    tools: {
      empty: "no tools",
      enableTool: (name: string) => `Enable ${name}`,
      enabled: "enabled",
      schema: "schema",
      argsFor: (name: string) => `Arguments for ${name}`,
      toolError: "the tool returned an error",
    },
    skills: {
      badge: { modified: "modified", missing: "file missing" },
      deleteConfirm: (name: string) => `delete skill "${name}"?`,
      registryBanner: "Skills are a registry; they run once an orchestrator agent exists (S6b).",
      nameAria: "Skill name",
      namePlaceholder: "name (optional — otherwise from SKILL.md)",
      descriptionAria: "Skill description",
      scopeAria: "Scope",
      chooseSkillMd: "choose SKILL.md",
      addSkill: "+ skill",
      empty: "no skills",
    },
    log: {
      limitMustBeNumber: "the limit must be a number",
      limitsTitle: "Limits (spend/rate)",
      scopeAria: "Scope",
      refIdAria: "Project or server ID",
      refIdNotRequired: "not required",
      refIdPlaceholder: "project/server id",
      spendCapAria: "Spend cap, USD",
      spendCapPlaceholder: "cap $ (empty = no limit)",
      /** SEC-4 honesty note (same register as `rules.spendCapInertHint`): the cap is INERT until
       * MCP servers report per-call costs. */
      spendCapInertHint:
        "Enforced once servers report call costs — until then the cap is stored but does not block calls.",
      ratePerMinAria: "Calls-per-minute limit",
      ratePerMinPlaceholder: "calls/min (empty = no limit)",
      setLimit: "set limit",
      noLimits: "no limits set",
      thScope: "scope",
      thCap: "cap $",
      thRate: "calls/min",
      callsTitle: "Calls",
      noCalls: "no calls",
      thSource: "source",
      thTool: "tool",
      thStatus: "status",
      thLatency: "latency, ms",
      thCost: "cost, $",
      thTime: "time",
      auditTitle: "Audit",
      noAudit: "no audit records",
      thAction: "action",
      thDecision: "decision",
      thReason: "reason",
    },
  },

  // ── keep-awake pill (SCN-045 / FLW-18; sidebar footer) ───────────────────────────────────────
  power: {
    /** Toggle ON and the assertion is genuinely held (≥1 live session) — green dot. */
    keepAwakeOn: "keep-awake · on",
    /** Toggle ON but nothing to hold (zero live sessions) — muted dot. */
    keepAwakeIdle: "keep-awake · idle",
    /** Toggle OFF — muted dot; the machine sleeps on the OS's normal schedule. */
    keepAwakeOff: "keep-awake · off",
    /** Honest OS-denial surface (SCN-045 "keep-awake unavailable: {reason}") — the toast, the
     * Diagnostics record message, AND the pill's failure label. Never a silent fake "awake". */
    keepAwakeFailed: (msg: string) => `keep-awake unavailable: ${msg}`,
  },

  // ── Stats view (SCN-052/053 / FLW-20; "✦ Stats" nav) ─────────────────────────────────────────
  stats: {
    nav: "✦  Stats",
    title: "Stats",
    rangeAria: "Stats range",
    rangeAll: "All",
    range30d: "30d",
    range7d: "7d",
    loading: "Scanning usage…",
    cancel: "Cancel",
    retry: "Retry",
    tokens: "tokens",
    /** Cost is an ESTIMATE from a public-price table (stats.rs::PRICING) — always labeled. */
    costLabel: "est. cost",
    /** Some contributing model family has no pricing row — the figure under-counts honestly. */
    costPartialLabel: "est. cost (partial)",
    partialMark: "*",
    estimatedNote:
      "Cost is estimated from public API prices; * marks projects where some models had no pricing row.",
    sessions: "agent sessions",
    commits: "commits",
    code: "code",
    activity: (days: number) => `Activity — tokens per day (last ${days} days)`,
    activityAria: (days: number) => `Token activity heatmap, last ${days} days`,
    byProject: "By project",
    byModel: "By model family",
    /** Per-model-family cut header note: the only agent-side dimension the logs expose. */
    byModelHint: "grouped by model family — session logs carry no per-agent id",
    asOf: (t: string) => `as of ${t}`,
    refresh: "Refresh",
    noGit: "no git data",
    otherBucket: "other",
    emptyTitle: "No usage in this range.",
    emptyHint: "Run agent sessions (or widen the range) and refresh.",
    usageUnavailable: (msg: string) => `usage data unavailable: ${msg}`,
    gitUnavailable: (msg: string) => `git data unavailable: ${msg}`,
  },

  // ── storage-degraded banners (spec D3 wire; consumed by the P3 banner) ───────────────────────
  storage: {
    recovered: (path: string) =>
      `Database was corrupted and has been reset. The damaged copy was saved to ${path}.`,
    inMemory: "Storage unavailable — running in memory. Changes will NOT survive a restart.",
  },

  // ── Workflows (SW1, docs/ux/plans/2026-07-24-workflow-authoring.md; "Workflows" nav, SCR-01..04)
  // AUTHORING/CONFIG ONLY: `run.pendingNote` is the honesty boundary — saving/triggering never
  // starts a run; the S6b executor that consumes these definitions does not exist yet. ──────────
  workflows: {
    nav: "⛓  Workflows",
    title: "Workflows",

    // ── library (SCR-01) ──
    library: {
      scopeAria: "Workflow scope filter",
      scopeAll: "All",
      scopeGlobal: "Global",
      scopeProject: "Project",
      newWorkflow: "+ New workflow",
      loading: "Loading workflows…",
      emptyTitle: "No workflows yet",
      emptyHint: "Compose one to reuse across projects.",
      stagesCount: (n: number) => `${n} ${n === 1 ? "stage" : "stages"}`,
      skillsCount: (n: number) => `${n} ${n === 1 ? "skill" : "skills"}`,
      scopeBadgeGlobal: "global",
      scopeBadgeProject: "project",
      run: "Run →",
      open: "Open",
      duplicate: "Duplicate",
      delete: "Delete",
      deleteConfirm: (name: string) => `Delete workflow "${name}"? This cannot be undone.`,
      duplicateSuffix: (name: string) => `${name} (copy)`,
      untitled: "Untitled workflow",
      noDescription: "No description.",
    },

    // ── editor (SCR-02) ──
    editor: {
      backToLibrary: "← Workflows",
      newTitle: "New workflow",
      nameLabel: "Name",
      nameAria: "Workflow name",
      namePlaceholder: "workflow name",
      descriptionLabel: "Description",
      descriptionAria: "Workflow description",
      descriptionPlaceholder: "what this workflow does (optional)",
      scopeLabel: "Scope",
      defaultAgentLabel: "Default agent",
      defaultAgentAria: "Default agent for stages that inherit",
      globalSkillsLabel: "Global skills",
      globalSkillsHint: "Loaded into every stage, on top of each stage's own skills.",
      noSkillsAvailable: "No skills registered yet.",
      stagesLabel: "Stages",
      addStage: "+ Add stage",
      noStages: "No stages yet — add the first stage.",
      save: "Save workflow",
      saving: "Saving…",
      unsavedHint: "Unsaved changes.",
      /** Terminal-bracket header (SCR-02): consecutive same-effective-agent stages group into one
       * terminal; a header names the terminal, its agent and its stage count. */
      terminalHeader: (n: number, agent: string, count: number) =>
        `Terminal ${n} · ${agent} · ${count} ${count === 1 ? "stage" : "stages"}`,
      stageInherits: (agent: string) => `inherits ${agent}`,
      editStage: "Edit",
      // validation (client twin of the daemon's fail-closed guard)
      errNameRequired: "name the workflow before saving",
      errProjectRequired: "pick a project for a project-scoped workflow",
      errStageIncomplete: (name: string) => `stage "${name}" needs a name and a prompt`,
      errNoStages: "add at least one stage before saving",
      errCeoNoClasses: "delegate at least one class or disable the CEO",
      stageUnnamedFallback: "Untitled stage",
    },

    // ── stage detail (SCR-03) ──
    stage: {
      nameLabel: "Stage name",
      nameAria: "Stage name",
      namePlaceholder: "stage name",
      promptLabel: "Prompt / command",
      promptAria: "Stage prompt or command",
      promptPlaceholder: "what this stage's agent should do…",
      skillsLabel: "Stage skills",
      noSkillsAvailable: "No skills registered yet.",
      effectiveSkillsLabel: "Effective skills",
      effectiveSkillsHint: "global plus this stage, deduped",
      noEffectiveSkills: "none",
      gateLabel: "Gate",
      gateAria: "Stage gate",
      outputsLabel: "Outputs",
      outputsAria: "New output",
      outputsPlaceholder: "output name",
      addOutput: "+ add",
      removeOutput: (v: string) => `Remove ${v}`,
      /** A bound skill id that is not in the registry (missing binding) — honest, blocks a clean
       * run. */
      missingBinding: (id: string) => `missing skill: ${id}`,
      /** A pinned agent that is not one of the known/launchable agents. */
      agentUnavailable: (agent: string) => `unknown agent: ${agent}`,
      done: "Done",
      remove: "Remove stage",
    },

    // ── agent & context panel (SCR-03) ──
    agentPanel: {
      sectionLabel: "Agent & context",
      agentLabel: "Agent",
      agentAria: "Stage agent",
      inherit: "Inherit default",
      inheritedLabel: (agent: string) => `inherits ${agent}`,
      contextLabel: "Context scope",
      contextAria: "Context scope",
      selectedNote: "Starts from an owner-picked subset (chosen per run).",
    },

    /** Known launchable agents — display labels for the four ids the app launches (contract). An
     * agent id not in this map is shown verbatim (an unavailable/legacy pin), never hidden. */
    agents: {
      "claude-code": "Claude Code",
      hermes: "Hermes",
      opencode: "OpenCode",
      kilo: "Kilo",
    } as Record<string, string>,

    contextScopes: {
      inherit: "Inherit",
      handoff: "Handoff",
      project: "Project",
      selected: "Selected",
    },

    gates: {
      auto: "Auto",
      manual: "Manual",
    },

    // ── CEO oversight (reuses the RulesetPanel supervisor pattern, SCN-046 register) — PLUMBING
    // ONLY: `pendingNote` is the honesty boundary, same register as the Skills tab's banner. ──
    ceo: {
      sectionLabel: "CEO oversight",
      pendingNote: "The CEO acts on this once the orchestrator agent runtime lands (S6b).",
      enableLabel: "Enable the CEO",
      enableAria: "Enable the CEO oversight",
      disabledHint: "Enable to configure delegation.",
      delegatedLabel: "Delegated confirmation classes",
      noClasses: "No classes delegated — add one or use the recommended scope.",
      recommendedScope: "Recommended scope",
      instructionLabel: "CEO instruction",
      instructionAria: "CEO instruction (markdown the CEO must follow)",
      instructionPlaceholder: "What the CEO must always follow (markdown)…",
      customRulesLabel: "Custom rules",
      customRuleAria: "New custom CEO rule",
      customRulePlaceholder: "rule",
      classAria: "New delegated confirmation class",
      classPlaceholder: "class",
      addEntry: "+ add",
      deleteEntry: (v: string) => `Delete ${v}`,
      /** Blocked alert: enabled CEO with an empty delegation scope (client twin of the daemon's
       * validate guard). */
      blockedNoClasses: "delegate at least one class or disable the CEO",
    },

    // ── run picker (SCR-04) — the trigger stub. It NEVER spawns a run in this slice. ──
    run: {
      openPicker: "Run workflow",
      title: (project: string) => `Run workflow on ${project}`,
      /** Neutral target label when no project is open (the run slice is a stub; this only titles the
       * modal). */
      fallbackProject: "your project",
      runButton: "Run workflow",
      cancel: "Cancel",
      noGlobalWorkflows: "No global workflows to run yet.",
      rowMeta: (stages: number, ceo: boolean) =>
        `${stages} ${stages === 1 ? "stage" : "stages"} · CEO ${ceo ? "on" : "off"}`,
      pickAria: "Workflow to run",
      /** The honesty boundary (S6b): this build does NOT run anything — it shows this note only. */
      pendingNote:
        "Workflows run once the orchestrator agent runtime lands (S6b). Authoring, saving and this trigger are live now — the run does not fake execution.",
    },
  },
} as const;
