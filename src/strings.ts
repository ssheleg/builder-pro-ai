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
} as const;
