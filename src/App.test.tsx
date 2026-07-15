// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, act, within } from "@testing-library/react";

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
  onDaemonIncompatible: (cb: (p: unknown) => void) => {
    cbs.incompatible = cb;
    return Promise.resolve(unlisten);
  },
  onWorkspaceUpdated: (cb: (p: unknown) => void) => {
    cbs.wsUpdated = cb;
    return Promise.resolve(unlisten);
  },
  onFsChanged: (cb: (p: unknown) => void) => {
    cbs.fsChanged = cb;
    return Promise.resolve(unlisten);
  },
  onFsWatchError: (cb: (p: unknown) => void) => {
    cbs.fsWatchError = cb;
    return Promise.resolve(unlisten);
  },
  onOrchdProjectsChanged: (cb: (p: unknown) => void) => {
    cbs.orchdProjectsChanged = cb;
    return Promise.resolve(unlisten);
  },
  onOrchdGoalsChanged: (cb: (p: unknown) => void) => {
    cbs.orchdGoalsChanged = cb;
    return Promise.resolve(unlisten);
  },
  onOrchdIdeasChanged: (cb: (p: unknown) => void) => {
    cbs.orchdIdeasChanged = cb;
    return Promise.resolve(unlisten);
  },
  onOrchdInsightsChanged: (cb: (p: unknown) => void) => {
    cbs.orchdInsightsChanged = cb;
    return Promise.resolve(unlisten);
  },
  onOrchdTasksChanged: (cb: (p: unknown) => void) => {
    cbs.orchdTasksChanged = cb;
    return Promise.resolve(unlisten);
  },
  onOrchdRulesetChanged: (cb: (p: unknown) => void) => {
    cbs.orchdRulesetChanged = cb;
    return Promise.resolve(unlisten);
  },
  onOrchdGraphChanged: (cb: (p: unknown) => void) => {
    cbs.orchdGraphChanged = cb;
    return Promise.resolve(unlisten);
  },
  onOrchdDown: (cb: (p: unknown) => void) => {
    cbs.orchdDown = cb;
    return Promise.resolve(unlisten);
  },
  onOrchdUp: (cb: (p: unknown) => void) => {
    cbs.orchdUp = cb;
    return Promise.resolve(unlisten);
  },
  onOrchdIncompatible: (cb: (p: unknown) => void) => {
    cbs.orchdIncompatible = cb;
    return Promise.resolve(unlisten);
  },
  onOrchdMcpServersChanged: (cb: (p: unknown) => void) => {
    cbs.orchdMcpServersChanged = cb;
    return Promise.resolve(unlisten);
  },
  onOrchdMcpToolsChanged: (cb: (p: unknown) => void) => {
    cbs.orchdMcpToolsChanged = cb;
    return Promise.resolve(unlisten);
  },
  onOrchdMcpArtifactsChanged: (cb: (p: unknown) => void) => {
    cbs.orchdMcpArtifactsChanged = cb;
    return Promise.resolve(unlisten);
  },
  onOrchdMcpInvocationLogged: (cb: (p: unknown) => void) => {
    cbs.orchdMcpInvocationLogged = cb;
    return Promise.resolve(unlisten);
  },
  onOrchdConnectorsChanged: (cb: (p: unknown) => void) => {
    cbs.orchdConnectorsChanged = cb;
    return Promise.resolve(unlisten);
  },
  onOrchdSkillsChanged: (cb: (p: unknown) => void) => {
    cbs.orchdSkillsChanged = cb;
    return Promise.resolve(unlisten);
  },
  onOrchdPoliciesChanged: (cb: (p: unknown) => void) => {
    cbs.orchdPoliciesChanged = cb;
    return Promise.resolve(unlisten);
  },
  onOrchdResearchRunsChanged: (cb: (p: unknown) => void) => {
    cbs.orchdResearchRunsChanged = cb;
    return Promise.resolve(unlisten);
  },
}));

// T8 (S-EXT §8): ExtPanel is mocked here — its own tests (`components/ext/ExtPanel.test.tsx`)
// cover its content; App only needs to prove it renders on `view === "ext"`.
vi.mock("./components/ext/ExtPanel", () => ({
  ExtPanel: () => <div data-testid="ext-panel-mock" />,
}));

// The store's `refresh*` actions (S3 T13) call straight through to `./ipc/orchd`; mocked here so
// App's initial `refreshProjects()` call (and any orchd://*-changed-driven refresh) resolves
// deterministically instead of hitting the real `invoke()` (which would reject in jsdom — no
// Tauri runtime — and, being routed through `showToast`, could spuriously surface a second
// role="alert" element that would break the DaemonBanner-focused assertions below).
const orchdListProjectsMock = vi.fn().mockResolvedValue([]);
const orchdListGoalsMock = vi.fn().mockResolvedValue([]);
const orchdListIdeasMock = vi.fn().mockResolvedValue([]);
const orchdListInsightsMock = vi.fn().mockResolvedValue([]);
const orchdListTasksMock = vi.fn().mockResolvedValue([]);
const researchListRunsMock = vi.fn().mockResolvedValue([]);
const orchdGraphListProjectMock = vi.fn().mockResolvedValue({ nodes: [], edges: [], externalNodes: [] });
const orchdGetRulesetMock = vi.fn();
// T18: ProjectPanel (rendered once view === "project") imports these directly from the same
// module — mocked here too so a project-view test never hits the real `invoke()`.
const orchdAddProjectWorkspaceMock = vi.fn();
const orchdRemoveProjectWorkspaceMock = vi.fn();
const orchdExportProjectMock = vi.fn();
const orchdExportToFileMock = vi.fn();
const orchdImportFromFileMock = vi.fn();
// T8 (S-EXT §8): the MCP slice's `refresh*` actions (`store.ts`) call straight through to these —
// mocked here for the same reason as the sessiond-domain wrappers above (deterministic resolution
// instead of a real invoke() reject in jsdom).
const mcpListServersMock = vi.fn().mockResolvedValue([]);
const mcpListToolsMock = vi.fn().mockResolvedValue([]);
const mcpListArtifactsMock = vi.fn().mockResolvedValue([]);
const mcpListInvocationsMock = vi.fn().mockResolvedValue([]);
// S-EXT §8 T13b: the Connectors slice's `refreshAccounts` (`store.ts`) calls straight through to
// this — mocked here for the same reason as the MCP wrappers above.
const connectorListAccountsMock = vi.fn().mockResolvedValue([]);
// S-EXT §8, D11, T17: the Skills slice's `refreshSkills` (`store.ts`) calls straight through to
// this — mocked here for the same reason as the MCP/Connectors wrappers above.
const skillListMock = vi.fn().mockResolvedValue([]);
// S-EXT §4/§6/§8, BL-22, T18: the Trust slice's `refreshPolicies`/`refreshAuditRows` (`store.ts`)
// call straight through to these — mocked here for the same reason as the wrappers above.
const trustListPoliciesMock = vi.fn().mockResolvedValue([]);
const trustListAuditMock = vi.fn().mockResolvedValue([]);
vi.mock("./ipc/orchd", () => ({
  orchdListProjects: (...a: unknown[]) => orchdListProjectsMock(...a),
  orchdListGoals: (...a: unknown[]) => orchdListGoalsMock(...a),
  orchdListIdeas: (...a: unknown[]) => orchdListIdeasMock(...a),
  orchdListInsights: (...a: unknown[]) => orchdListInsightsMock(...a),
  orchdListTasks: (...a: unknown[]) => orchdListTasksMock(...a),
  researchListRuns: (...a: unknown[]) => researchListRunsMock(...a),
  orchdGraphListProject: (...a: unknown[]) => orchdGraphListProjectMock(...a),
  orchdGetRuleset: (...a: unknown[]) => orchdGetRulesetMock(...a),
  orchdAddProjectWorkspace: (...a: unknown[]) => orchdAddProjectWorkspaceMock(...a),
  orchdRemoveProjectWorkspace: (...a: unknown[]) => orchdRemoveProjectWorkspaceMock(...a),
  orchdExportProject: (...a: unknown[]) => orchdExportProjectMock(...a),
  orchdExportToFile: (...a: unknown[]) => orchdExportToFileMock(...a),
  orchdImportFromFile: (...a: unknown[]) => orchdImportFromFileMock(...a),
  mcpListServers: (...a: unknown[]) => mcpListServersMock(...a),
  mcpListTools: (...a: unknown[]) => mcpListToolsMock(...a),
  mcpListArtifacts: (...a: unknown[]) => mcpListArtifactsMock(...a),
  mcpListInvocations: (...a: unknown[]) => mcpListInvocationsMock(...a),
  connectorListAccounts: (...a: unknown[]) => connectorListAccountsMock(...a),
  skillList: (...a: unknown[]) => skillListMock(...a),
  trustListPolicies: (...a: unknown[]) => trustListPoliciesMock(...a),
  trustListAudit: (...a: unknown[]) => trustListAuditMock(...a),
  describeOrchdError: (e: unknown) => `mapped: ${JSON.stringify(e)}`,
}));

const listSessionsMock = vi.fn().mockResolvedValue([]);
const listWorkspacesMock = vi.fn().mockResolvedValue([]);
const createSessionMock = vi.fn().mockResolvedValue(undefined);
const killSessionMock = vi.fn().mockResolvedValue(undefined);
const createWorkspaceMock = vi.fn().mockResolvedValue(undefined);
const pickFolderMock = vi.fn().mockResolvedValue(null);
const daemonStatusMock = vi.fn().mockResolvedValue({ kind: "disconnected" });
const getCommandEventsMock = vi.fn().mockResolvedValue([]);
vi.mock("./ipc/commands", () => ({
  listSessions: (...a: unknown[]) => listSessionsMock(...a),
  listWorkspaces: (...a: unknown[]) => listWorkspacesMock(...a),
  createSession: (...a: unknown[]) => createSessionMock(...a),
  killSession: (...a: unknown[]) => killSessionMock(...a),
  createWorkspace: (...a: unknown[]) => createWorkspaceMock(...a),
  pickFolder: (...a: unknown[]) => pickFolderMock(...a),
  daemonStatus: (...a: unknown[]) => daemonStatusMock(...a),
  getCommandEvents: (...a: unknown[]) => getCommandEventsMock(...a),
}));

// Only override the two watch-lifecycle functions App wires up directly — every OTHER export
// (`listDir`, `readFilePreview`, ...) stays the REAL implementation, since FileTree/FilePreview
// (rendered transitively via FilesRail once a workspace is active) import from this same module
// and must keep working exactly as they do in their own, unmocked test suites (a real `invoke()`
// call rejects harmlessly in jsdom — no Tauri runtime — which those components already treat as
// an honest error, spec §7; nothing here needs to change that).
const startWorkspaceWatchMock = vi.fn().mockResolvedValue(undefined);
const stopWorkspaceWatchMock = vi.fn().mockResolvedValue(undefined);
vi.mock("./ipc/fs", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./ipc/fs")>();
  return {
    ...actual,
    startWorkspaceWatch: (...a: unknown[]) => startWorkspaceWatchMock(...a),
    stopWorkspaceWatch: (...a: unknown[]) => stopWorkspaceWatchMock(...a),
  };
});

const disposeMock = vi.fn();
const openMock = vi.fn();
const hideMock = vi.fn();
const focusMock = vi.fn();
const resetAllAttachmentsMock = vi.fn();
const resetAttachmentMock = vi.fn();
const fakeManager = {
  ensure: vi.fn(),
  has: vi.fn(() => true),
  get: vi.fn(),
  isOpened: vi.fn(() => true),
  isAttached: vi.fn(() => false),
  pendingBytes: vi.fn(() => 0),
  attach: vi.fn().mockResolvedValue(undefined),
  resetAttachment: resetAttachmentMock,
  resetAllAttachments: resetAllAttachmentsMock,
  applyReplay: vi.fn(),
  writeOutput: vi.fn(),
  open: openMock,
  hide: hideMock,
  focus: focusMock,
  dispose: disposeMock,
  disposeAll: vi.fn(),
} as unknown as import("./terminal/terminal-manager").TerminalManager;

import { App, changedPathsToParentDirs } from "./App";
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
  hideMock.mockReset();
  focusMock.mockReset();
  resetAllAttachmentsMock.mockReset();
  resetAttachmentMock.mockReset();
  (fakeManager.attach as unknown as ReturnType<typeof vi.fn>)
    .mockReset()
    .mockResolvedValue(undefined);
  listSessionsMock.mockReset().mockResolvedValue([]);
  listWorkspacesMock.mockReset().mockResolvedValue([]);
  createSessionMock.mockClear();
  killSessionMock.mockClear();
  createWorkspaceMock.mockClear();
  pickFolderMock.mockClear();
  daemonStatusMock.mockReset().mockResolvedValue({ kind: "disconnected" });
  getCommandEventsMock.mockReset().mockResolvedValue([]);
  startWorkspaceWatchMock.mockReset().mockResolvedValue(undefined);
  stopWorkspaceWatchMock.mockReset().mockResolvedValue(undefined);
  orchdListProjectsMock.mockReset().mockResolvedValue([]);
  orchdListGoalsMock.mockReset().mockResolvedValue([]);
  orchdListIdeasMock.mockReset().mockResolvedValue([]);
  orchdListInsightsMock.mockReset().mockResolvedValue([]);
  orchdListTasksMock.mockReset().mockResolvedValue([]);
  orchdGraphListProjectMock.mockReset().mockResolvedValue({ nodes: [], edges: [], externalNodes: [] });
  orchdGetRulesetMock.mockReset();
  orchdAddProjectWorkspaceMock.mockReset();
  orchdRemoveProjectWorkspaceMock.mockReset();
  orchdExportProjectMock.mockReset();
  orchdExportToFileMock.mockReset();
  orchdImportFromFileMock.mockReset();
  mcpListServersMock.mockReset().mockResolvedValue([]);
  mcpListToolsMock.mockReset().mockResolvedValue([]);
  mcpListArtifactsMock.mockReset().mockResolvedValue([]);
  mcpListInvocationsMock.mockReset().mockResolvedValue([]);
  connectorListAccountsMock.mockReset().mockResolvedValue([]);
  skillListMock.mockReset().mockResolvedValue([]);
  trustListPoliciesMock.mockReset().mockResolvedValue([]);
  trustListAuditMock.mockReset().mockResolvedValue([]);
  useAppStore.setState(
    {
      sessions: {},
      workspaces: {},
      activeSessionId: null,
      daemonConnected: false,
      daemonIncompatible: false,
      upgradeDialogOpen: false,
      upgradeError: null,
      hydrated: false,
      // Most tests below exercise session/terminal behavior that predates the Home screen (T11)
      // and assume the workspace layout (TerminalTabs/TerminalPane) is on screen; Home-specific
      // behavior gets its own `describe` block below that sets `view: "home"` explicitly.
      view: "workspace",
      treeCache: {},
      watchPaused: false,
      showIgnored: false,
      activeProjectId: null,
      projects: [],
      goalsByProject: {},
      ideas: [],
      insights: [],
      tasksByProject: {},
      researchRunsByIdea: {},
      graphByProject: {},
      rulesets: {},
      mcpServers: [],
      mcpToolsByServer: {},
      mcpArtifacts: [],
      accounts: [],
      invocations: [],
      auditRows: [],
      policies: [],
      orchdDown: false,
      orchdIncompatible: false,
      orchdUpgradeDialogOpen: false,
    },
    false,
  );
});

describe("App", () => {
  it("registers all IPC subscriptions on mount (ten sessiond/fs + ten orchd + three MCP + one connectors + one skills + one research, S3 T13 + S4 T6 + S-EXT T8/T13b/T17 + S-IDEA T6)", async () => {
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    for (const key of [
      "created",
      "state",
      "exited",
      "wsCreated",
      "wsUpdated",
      "fsChanged",
      "fsWatchError",
      "disc",
      "recon",
      "incompatible",
      "orchdProjectsChanged",
      "orchdGoalsChanged",
      "orchdIdeasChanged",
      "orchdInsightsChanged",
      "orchdTasksChanged",
      "orchdRulesetChanged",
      "orchdGraphChanged",
      "orchdDown",
      "orchdUp",
      "orchdIncompatible",
      "orchdMcpServersChanged",
      "orchdMcpToolsChanged",
      "orchdMcpArtifactsChanged",
      "orchdConnectorsChanged",
      "orchdSkillsChanged",
      "orchdResearchRunsChanged",
    ]) {
      expect(typeof cbs[key]).toBe("function");
    }
  });

  it("calls refreshProjects (via orchd_list_projects) once on mount", async () => {
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    expect(orchdListProjectsMock).toHaveBeenCalled();
  });

  it("hydrates on mount: listWorkspaces+listSessions succeed -> daemonConnected true, hydrated true", async () => {
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    expect(listWorkspacesMock).toHaveBeenCalled();
    expect(listSessionsMock).toHaveBeenCalled();
    expect(useAppStore.getState().daemonConnected).toBe(true);
    expect(useAppStore.getState().hydrated).toBe(true);
  });

  it("hydrate retries on rejection then succeeds (bounded backoff)", async () => {
    vi.useFakeTimers();
    listWorkspacesMock.mockRejectedValueOnce(new Error("daemon not connected yet"));
    listWorkspacesMock.mockResolvedValue([]);
    render(<App manager={fakeManager} />);

    // First attempt happens synchronously on mount; let its microtasks flush.
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(useAppStore.getState().daemonConnected).toBe(false);
    expect(listWorkspacesMock).toHaveBeenCalledTimes(1);

    // Advance past the retry delay; the retry should succeed this time.
    await act(async () => {
      await vi.advanceTimersByTimeAsync(2000);
    });
    expect(listWorkspacesMock.mock.calls.length).toBeGreaterThanOrEqual(2);
    expect(useAppStore.getState().daemonConnected).toBe(true);
    vi.useRealTimers();
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

  it("session://exited marks the session inactive+exited (does not remove it, does not dispose the pane)", async () => {
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
    expect(disposeMock).not.toHaveBeenCalled();
  });

  it("daemon://incompatible sets BOTH daemonIncompatible and upgradeDialogOpen", async () => {
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    await act(async () => {
      cbs.incompatible(null);
    });
    expect(useAppStore.getState().daemonIncompatible).toBe(true);
    expect(useAppStore.getState().upgradeDialogOpen).toBe(true);
  });

  describe("finding [12]/[F3]: daemon_status pull fallback closes the lost-event race", () => {
    it("hydrate failure (disconnected) + daemonStatus incompatible -> sets both flags once, opens dialog on first detection", async () => {
      vi.useFakeTimers();
      listWorkspacesMock.mockRejectedValue(new Error("disconnected"));
      daemonStatusMock.mockResolvedValue({ kind: "incompatible", daemonMin: 3, daemonMax: 4 });

      render(<App manager={fakeManager} />);
      await act(async () => {
        await Promise.resolve();
        await Promise.resolve();
        await Promise.resolve();
      });

      expect(useAppStore.getState().daemonIncompatible).toBe(true);
      expect(useAppStore.getState().upgradeDialogOpen).toBe(true);
      expect(daemonStatusMock).toHaveBeenCalled();

      // Close the dialog (simulate user Cancel) and let another poll cycle happen — it must NOT
      // reopen the dialog, only the FIRST detection opens it.
      act(() => useAppStore.getState().setUpgradeDialogOpen(false));
      await act(async () => {
        await vi.advanceTimersByTimeAsync(6000);
      });
      expect(useAppStore.getState().daemonIncompatible).toBe(true);
      expect(useAppStore.getState().upgradeDialogOpen).toBe(false);

      vi.useRealTimers();
    });

    it("hydrate failure + daemonStatus disconnected (not incompatible) -> flags stay false, no dialog", async () => {
      vi.useFakeTimers();
      listWorkspacesMock.mockRejectedValue(new Error("disconnected"));
      daemonStatusMock.mockResolvedValue({ kind: "disconnected" });

      render(<App manager={fakeManager} />);
      await act(async () => {
        await Promise.resolve();
        await Promise.resolve();
        await Promise.resolve();
      });

      expect(useAppStore.getState().daemonIncompatible).toBe(false);
      expect(useAppStore.getState().upgradeDialogOpen).toBe(false);

      vi.useRealTimers();
    });

    it("daemonStatus rejecting (best-effort) does not crash the hydrate retry loop", async () => {
      vi.useFakeTimers();
      listWorkspacesMock.mockRejectedValue(new Error("disconnected"));
      daemonStatusMock.mockRejectedValue(new Error("ipc not ready"));

      render(<App manager={fakeManager} />);
      await act(async () => {
        await Promise.resolve();
        await Promise.resolve();
        await Promise.resolve();
      });

      expect(useAppStore.getState().daemonIncompatible).toBe(false);
      expect(useAppStore.getState().daemonConnected).toBe(false);

      vi.useRealTimers();
    });

    it("connected path unaffected: successful hydrate never calls daemonStatus", async () => {
      await act(async () => {
        render(<App manager={fakeManager} />);
      });
      expect(useAppStore.getState().daemonConnected).toBe(true);
      expect(daemonStatusMock).not.toHaveBeenCalled();
    });
  });

  it("daemon disconnect shows the banner; reconnect hides it and re-hydrates", async () => {
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    await act(async () => {
      cbs.disc(null);
    });
    expect(screen.getByRole("alert")).toBeTruthy();
    listWorkspacesMock.mockClear();
    listSessionsMock.mockClear();
    await act(async () => {
      cbs.recon(null);
    });
    expect(screen.queryByRole("alert")).toBeNull();
    expect(listWorkspacesMock).toHaveBeenCalled();
    expect(listSessionsMock).toHaveBeenCalled();
  });

  it("keep-alive: only the active session's pane is mounted; switching active hides (not disposes) the old pane", async () => {
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    await act(async () => {
      cbs.created(meta()); // s1 becomes active
    });
    await act(async () => {
      cbs.created(meta({ id: "s2", title: "bash" }));
    });
    // Only the active pane (s1) is mounted.
    expect(screen.getByTestId("terminal-pane-s1")).toBeTruthy();
    expect(screen.queryByTestId("terminal-pane-s2")).toBeNull();

    act(() => useAppStore.getState().setActiveSession("s2"));

    expect(screen.queryByTestId("terminal-pane-s1")).toBeNull();
    expect(screen.getByTestId("terminal-pane-s2")).toBeTruthy();
    // Unmounting s1's pane calls hide(), never dispose().
    expect(hideMock).toHaveBeenCalledWith("s1");
    expect(disposeMock).not.toHaveBeenCalled();
  });

  it("A1: switching to a second tab attaches that session's terminal (no dead pane)", async () => {
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    const attachMock = fakeManager.attach as unknown as ReturnType<typeof vi.fn>;
    await act(async () => {
      cbs.created(meta()); // s1 active -> pane mounts -> attach('s1')
    });
    expect(attachMock.mock.calls.filter((c) => c[0] === "s1").length).toBe(1);

    await act(async () => {
      cbs.created(meta({ id: "s2", title: "bash" })); // s2 exists but s1 stays active
    });
    // Switch the active tab to s2: the reused pane instance must attach s2.
    await act(async () => {
      useAppStore.getState().setActiveSession("s2");
    });
    // Pre-fix this is 0 (attachedRef latched on the first mount -> dead pane).
    expect(attachMock.mock.calls.filter((c) => c[0] === "s2").length).toBe(1);

    // Switching back to s1 re-mounts the pane; TerminalPane calls attach('s1')
    // unconditionally and the manager dedupes. With the fake manager there is no
    // real dedup, so we only assert the pane issues the call (dedup is proven in
    // terminal-manager.test.ts against the real manager).
    await act(async () => {
      useAppStore.getState().setActiveSession("s1");
    });
    expect(attachMock.mock.calls.filter((c) => c[0] === "s1").length).toBeGreaterThanOrEqual(1);
    // A tab switch must never dispose a keep-alive pane.
    expect(disposeMock).not.toHaveBeenCalled();
  });

  it("daemon reconnect resets ALL attach flags then re-attaches the visible session (spec §13)", async () => {
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    await act(async () => {
      cbs.created(meta()); // s1 becomes active; TerminalPane mounts + attach()'s once
    });
    const attachMock = fakeManager.attach as unknown as ReturnType<typeof vi.fn>;
    const callsBeforeReconnect = attachMock.mock.calls.filter((c) => c[0] === "s1").length;
    expect(callsBeforeReconnect).toBeGreaterThan(0);

    await act(async () => {
      cbs.disc(null);
    });
    await act(async () => {
      cbs.recon(null);
    });

    // New mechanism: reconnect clears EVERY session's attach flag (so hidden ones
    // re-attach lazily when next shown) BEFORE eagerly re-attaching the visible one.
    expect(resetAllAttachmentsMock).toHaveBeenCalledTimes(1);
    const callsAfterReconnect = attachMock.mock.calls.filter((c) => c[0] === "s1").length;
    expect(callsAfterReconnect).toBeGreaterThan(callsBeforeReconnect);
    // The active pane must NOT have been disposed/remounted to achieve this.
    expect(disposeMock).not.toHaveBeenCalled();
  });

  it("daemon reconnect: a hidden session lazily re-attaches when its tab is next shown (spec §13)", async () => {
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    await act(async () => {
      cbs.created(meta()); // s1 active (visible)
    });
    await act(async () => {
      cbs.created(meta({ id: "s2", title: "bash" })); // s2 hidden
    });
    // Show s2 once so its pane has mounted+attached at least once before reconnect.
    await act(async () => {
      useAppStore.getState().setActiveSession("s2");
    });
    await act(async () => {
      useAppStore.getState().setActiveSession("s1"); // back to s1 visible; s2 hidden
    });
    const attachMock = fakeManager.attach as unknown as ReturnType<typeof vi.fn>;

    await act(async () => {
      cbs.disc(null);
    });
    await act(async () => {
      cbs.recon(null); // resets all flags; eagerly re-attaches visible s1 only
    });
    expect(resetAllAttachmentsMock).toHaveBeenCalledTimes(1);
    const s2Before = attachMock.mock.calls.filter((c) => c[0] === "s2").length;

    // Now switch to the hidden session -> its pane re-mounts and re-attaches lazily.
    await act(async () => {
      useAppStore.getState().setActiveSession("s2");
    });
    const s2After = attachMock.mock.calls.filter((c) => c[0] === "s2").length;
    expect(s2After).toBeGreaterThan(s2Before);
    expect(disposeMock).not.toHaveBeenCalled();
  });

  it("hydrate restores an active workspace so + New terminal is enabled without a sidebar click", async () => {
    listWorkspacesMock.mockResolvedValue([{ id: "w1", name: "proj", rootPath: "/p", roots: ["/p"] }]);
    listSessionsMock.mockResolvedValue([]);
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    const newTerminalBtn = screen.getByRole("button", { name: /new terminal/i });
    expect(newTerminalBtn).not.toHaveProperty("disabled", true);
    await act(async () => {
      newTerminalBtn.click();
    });
    // Root-aware cwd (spec §6.3): no file selected -> falls back to the workspace's roots[0].
    expect(createSessionMock).toHaveBeenCalledWith("w1", { cwd: "/p", cols: 80, rows: 24 });
  });

  it("clicking a workspace in the sidebar sets it as the active workspace for new-terminal", async () => {
    useAppStore.setState({
      sessions: {},
      workspaces: { w1: { id: "w1", name: "proj", rootPath: "/p", roots: ["/p"] } },
      activeSessionId: null,
      daemonConnected: true,
    });
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    await act(async () => {
      screen.getByText("proj").click();
    });
    await act(async () => {
      screen.getByRole("button", { name: /new terminal/i }).click();
    });
    expect(createSessionMock).toHaveBeenCalledWith("w1", { cwd: "/p", cols: 80, rows: 24 });
  });
});

describe("S2 final review A2: changedPathsToParentDirs (pure helper)", () => {
  it("maps each changed rel-path to its parent directory, deduped", () => {
    expect(changedPathsToParentDirs(["src/new.ts", "top.txt"])).toEqual(["src", ""]);
  });

  it("dedups multiple changed files sharing the same parent directory", () => {
    expect(changedPathsToParentDirs(["src/a.ts", "src/b.ts", "src/c.ts"])).toEqual(["src"]);
  });

  it("maps a nested path to its immediate parent only, not the root", () => {
    expect(changedPathsToParentDirs(["a/b/c.ts"])).toEqual(["a/b"]);
  });

  it("passes the \"*\" overflow sentinel through unchanged", () => {
    expect(changedPathsToParentDirs(["*"])).toEqual(["*"]);
  });

  it("returns an empty array for an empty input", () => {
    expect(changedPathsToParentDirs([])).toEqual([]);
  });
});

describe("T11: attention-first Home, view switch, watch + fs/workspace event wiring", () => {
  it("view='home' renders HomeView instead of the terminal layout, and hides FilesRail even with an active workspace", async () => {
    listWorkspacesMock.mockResolvedValue([{ id: "w1", name: "proj", rootPath: "/p", roots: ["/p"] }]);
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    // hydrate() auto-selects "w1" as the active workspace regardless of `view`.
    await act(async () => {
      useAppStore.getState().setView("home");
    });
    expect(screen.getByTestId("home-stats")).toBeTruthy();
    expect(screen.queryByRole("tablist")).toBeNull();
    expect(screen.queryByLabelText("Files")).toBeNull();
  });

  it("view='workspace' (default) renders the terminal tab strip, not HomeView", async () => {
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    expect(screen.getByRole("tablist")).toBeTruthy();
    expect(screen.queryByTestId("home-stats")).toBeNull();
  });

  it("clicking ⌂ Home in the sidebar switches away from the terminal layout to HomeView", async () => {
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    expect(screen.getByRole("tablist")).toBeTruthy();
    await act(async () => {
      screen.getByRole("button", { name: "Home" }).click();
    });
    expect(screen.getByTestId("home-stats")).toBeTruthy();
    expect(screen.queryByRole("tablist")).toBeNull();
  });

  it("end-to-end: Пройти from Home switches to the workspace view with that session active and focuses its terminal", async () => {
    listWorkspacesMock.mockResolvedValue([{ id: "w1", name: "proj", rootPath: "/p", roots: ["/p"] }]);
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    await act(async () => {
      cbs.created(
        meta({ id: "s1", workspaceId: "w1", waitingForInput: true, lifecycle: { kind: "running" } }),
      );
    });
    await act(async () => {
      useAppStore.getState().setView("home");
    });
    expect(screen.getByTestId("home-stats")).toBeTruthy();

    await act(async () => {
      screen.getByRole("button", { name: /пройти/i }).click();
    });

    expect(useAppStore.getState().view).toBe("workspace");
    expect(useAppStore.getState().activeSessionId).toBe("s1");
    expect(focusMock).toHaveBeenCalledWith("s1");
    expect(screen.getByTestId("terminal-pane-s1")).toBeTruthy();
  });

  it("starts the live watch for the active workspace's roots once view='workspace'; stops it on unmount", async () => {
    useAppStore.setState({
      sessions: {},
      workspaces: { w1: { id: "w1", name: "proj", rootPath: "/p", roots: ["/p", "/p/sub"] } },
      activeSessionId: null,
      daemonConnected: true,
    });
    let utils!: ReturnType<typeof render>;
    await act(async () => {
      utils = render(<App manager={fakeManager} />);
    });
    await act(async () => {
      screen.getByText("proj").click(); // selects the workspace (App-local) -> watch effect fires
    });
    expect(startWorkspaceWatchMock).toHaveBeenCalledWith(["/p", "/p/sub"], false);
    utils.unmount();
    expect(stopWorkspaceWatchMock).toHaveBeenCalled();
  });

  it("never starts the watch on Home; switching to Home stops an active watch", async () => {
    useAppStore.setState({
      sessions: {},
      workspaces: { w1: { id: "w1", name: "proj", rootPath: "/p", roots: ["/p"] } },
      activeSessionId: null,
      daemonConnected: true,
    });
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    await act(async () => {
      screen.getByText("proj").click();
    });
    expect(startWorkspaceWatchMock).toHaveBeenCalledTimes(1);
    startWorkspaceWatchMock.mockClear();
    stopWorkspaceWatchMock.mockClear();

    await act(async () => {
      useAppStore.getState().setView("home");
    });
    expect(stopWorkspaceWatchMock).toHaveBeenCalled();
    expect(startWorkspaceWatchMock).not.toHaveBeenCalled();
  });

  it("a workspace://updated push for the ACTIVE workspace restarts the watch with the fresh roots", async () => {
    useAppStore.setState({
      sessions: {},
      workspaces: { w1: { id: "w1", name: "proj", rootPath: "/p", roots: ["/p"] } },
      activeSessionId: null,
      daemonConnected: true,
    });
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    await act(async () => {
      screen.getByText("proj").click();
    });
    expect(startWorkspaceWatchMock).toHaveBeenLastCalledWith(["/p"], false);
    startWorkspaceWatchMock.mockClear();

    await act(async () => {
      cbs.wsUpdated({ id: "w1", name: "proj", rootPath: "/p", roots: ["/p", "/p2"] });
    });
    expect(startWorkspaceWatchMock).toHaveBeenLastCalledWith(["/p", "/p2"], false);
  });

  // S2 final review A2: `fs://changed`'s `changedRelPaths` are the CHANGED ENTRIES' own paths
  // (e.g. `"src/new.ts"` for a file created inside `src`) — `treeCache` is keyed by the
  // CONTAINING DIRECTORY's listing, so handing the entry's own path straight to `invalidateDirs`
  // was a silent no-op (a file key that was never cached) for anything short of the `["*"]`
  // overflow sentinel. `onFsChanged` must map each path to its parent directory first.
  it("onFsChanged (spec §5, A2) invalidates the CONTAINING directory of a changed file, not the file's own key", async () => {
    useAppStore.setState({ treeCache: { "/p\tsrc": [{ name: "a", relPath: "a", isDir: false, size: 1, isIgnored: false }] } }, false);
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    await act(async () => {
      cbs.fsChanged({ root: "/p", changedRelPaths: ["src/new.ts"] });
    });
    expect(useAppStore.getState().treeCache["/p\tsrc"]).toBeUndefined();
  });

  it("onFsChanged (A2) maps a top-level changed file to the ROOT listing's cache key (\"\")", async () => {
    useAppStore.setState({ treeCache: { "/p\t": [{ name: "a", relPath: "a", isDir: false, size: 1, isIgnored: false }] } }, false);
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    await act(async () => {
      cbs.fsChanged({ root: "/p", changedRelPaths: ["top.txt"] });
    });
    expect(useAppStore.getState().treeCache["/p\t"]).toBeUndefined();
  });

  it("onFsChanged (A2) passes the \"*\" overflow sentinel through unchanged (drops every cached dir under the root)", async () => {
    useAppStore.setState(
      {
        treeCache: {
          "/p\t": [],
          "/p\tsrc": [],
          "/p2\tsrc": [{ name: "x", relPath: "x", isDir: false, size: 1, isIgnored: false }],
        },
      },
      false,
    );
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    await act(async () => {
      cbs.fsChanged({ root: "/p", changedRelPaths: ["*"] });
    });
    const cache = useAppStore.getState().treeCache;
    expect(cache["/p\t"]).toBeUndefined();
    expect(cache["/p\tsrc"]).toBeUndefined();
    // A different root's cache entries must be left untouched.
    expect(cache["/p2\tsrc"]).toBeDefined();
  });

  it("onFsWatchError (spec §5/§7) pauses the watch honestly", async () => {
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    expect(useAppStore.getState().watchPaused).toBe(false);
    await act(async () => {
      cbs.fsWatchError({ root: "/p", reason: "watcher died" });
    });
    expect(useAppStore.getState().watchPaused).toBe(true);
  });

  // S2 final review A4: workspace A's watch dying sets `watchPaused` true; switching to a
  // DIFFERENT, healthy workspace B must clear that stale banner rather than leaving B falsely
  // showing "live updates paused" forever.
  it("a successful (re)start of the workspace watch clears a stale watchPaused (A4)", async () => {
    useAppStore.setState({
      sessions: {},
      workspaces: { w1: { id: "w1", name: "proj", rootPath: "/p", roots: ["/p"] } },
      activeSessionId: null,
      daemonConnected: true,
    });
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    await act(async () => {
      useAppStore.getState().setWatchPaused(true);
    });
    expect(useAppStore.getState().watchPaused).toBe(true);

    await act(async () => {
      screen.getByText("proj").click(); // selects the workspace -> watch (re)start effect fires
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(startWorkspaceWatchMock).toHaveBeenCalled();
    expect(useAppStore.getState().watchPaused).toBe(false);
  });

  it("onWorkspaceUpdated (spec §3.3/§6.6) upserts the workspace", async () => {
    useAppStore.setState(
      { workspaces: { w1: { id: "w1", name: "proj", rootPath: "/p", roots: ["/p"] } } },
      false,
    );
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    await act(async () => {
      cbs.wsUpdated({ id: "w1", name: "proj", rootPath: "/p", roots: ["/p", "/p2"] });
    });
    expect(useAppStore.getState().workspaces["w1"].roots).toEqual(["/p", "/p2"]);
  });

  it("renders <Toast/> near the root: a store toast message becomes visible as role=alert", async () => {
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    await act(async () => {
      useAppStore.getState().showToast("что-то пошло не так");
    });
    const alerts = screen.getAllByRole("alert");
    expect(alerts.some((el) => el.textContent?.includes("что-то пошло не так"))).toBe(true);
  });
});

describe("T12: workspace view — stat chips + command strip + root-aware cwd", () => {
  it("stat chips show correct live/waiting/exited/roots counts, scoped to the active workspace only", async () => {
    useAppStore.setState(
      {
        sessions: {
          // w1: 1 waiting, 1 live, 1 exited
          s1: meta({ id: "s1", workspaceId: "w1", waitingForInput: true, lifecycle: { kind: "running" } }),
          s2: meta({ id: "s2", workspaceId: "w1", isActive: true, waitingForInput: false, lifecycle: { kind: "running" } }),
          s3: meta({
            id: "s3",
            workspaceId: "w1",
            isActive: false,
            waitingForInput: false,
            lifecycle: { kind: "exited", code: 1, signal: null },
          }),
          // w2: must NOT be counted into w1's chips
          s4: meta({ id: "s4", workspaceId: "w2", isActive: true, waitingForInput: false, lifecycle: { kind: "running" } }),
        },
        workspaces: {
          w1: { id: "w1", name: "proj", rootPath: "/p", roots: ["/p", "/p2"] },
          w2: { id: "w2", name: "other", rootPath: "/o", roots: ["/o"] },
        },
        activeSessionId: null,
        daemonConnected: true,
      },
      false,
    );
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    await act(async () => {
      screen.getByText("proj").click(); // selects w1 as the active workspace
    });
    expect(screen.getByTestId("workspace-stats")).toBeTruthy();
    expect(screen.getByTestId("workspace-stat-live").textContent).toBe("1 live");
    expect(screen.getByTestId("workspace-stat-waiting").textContent).toBe("1 waiting");
    expect(screen.getByTestId("workspace-stat-exited").textContent).toBe("1 exited");
    expect(screen.getByTestId("workspace-stat-roots").textContent).toBe("2 roots");
  });

  it("clicking a stat chip toggles an inline detail list; clicking it again closes it", async () => {
    useAppStore.setState(
      {
        sessions: {
          s1: meta({ id: "s1", workspaceId: "w1", title: "waiting-one", waitingForInput: true, lifecycle: { kind: "running" } }),
        },
        workspaces: { w1: { id: "w1", name: "proj", rootPath: "/p", roots: ["/p"] } },
        activeSessionId: null,
        daemonConnected: true,
      },
      false,
    );
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    await act(async () => {
      screen.getByText("proj").click();
    });
    expect(screen.queryByTestId("workspace-stat-detail")).toBeNull();

    await act(async () => {
      screen.getByTestId("workspace-stat-waiting").click();
    });
    expect(screen.getByTestId("workspace-stat-detail").textContent).toContain("waiting-one");

    await act(async () => {
      screen.getByTestId("workspace-stat-waiting").click();
    });
    expect(screen.queryByTestId("workspace-stat-detail")).toBeNull();
  });

  it("stat chips render nothing while no workspace is active", async () => {
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    expect(screen.queryByTestId("workspace-stats")).toBeNull();
  });

  it("renders the CommandStrip for the active session once one exists, fetching its command history", async () => {
    getCommandEventsMock.mockResolvedValue([]);
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    await act(async () => {
      cbs.created(meta()); // s1 becomes active
    });
    expect(getCommandEventsMock).toHaveBeenCalledWith("s1", 10);
    expect(screen.getByTestId("command-strip-empty")).toBeTruthy();
  });

  it("no CommandStrip is rendered while there is no active session", async () => {
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    expect(getCommandEventsMock).not.toHaveBeenCalled();
    expect(screen.queryByTestId("command-strip")).toBeNull();
    expect(screen.queryByTestId("command-strip-empty")).toBeNull();
  });
});

describe("S3 T13: orchd domain event wiring", () => {
  it("orchd://projects-changed re-fetches the project list", async () => {
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    orchdListProjectsMock.mockClear();
    await act(async () => {
      cbs.orchdProjectsChanged(null);
    });
    expect(orchdListProjectsMock).toHaveBeenCalled();
  });

  it("orchd://goals-changed refreshes ONLY the named project's goals", async () => {
    orchdListGoalsMock.mockResolvedValue([
      { id: "g1", projectId: "p1", parentId: null, kind: "strategic", title: "t", body: "",
        ord: 0, status: "active", metricRefs: [], createdAt: 1, updatedAt: 1 },
    ]);
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    await act(async () => {
      cbs.orchdGoalsChanged({ projectId: "p1" });
    });
    expect(orchdListGoalsMock).toHaveBeenCalledWith("p1");
    expect(useAppStore.getState().goalsByProject["p1"]).toHaveLength(1);
  });

  it("orchd://tasks-changed refreshes ONLY the named project's tasks", async () => {
    orchdListTasksMock.mockResolvedValue([
      { id: "t1", projectId: "p1", parentId: null, title: "t", body: "", status: "todo",
        source: "idea", sourceId: null, tags: [], rank: 0, rankAgent: null,
        rankAgentReasoning: "", createdAt: 1, updatedAt: 1 },
    ]);
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    await act(async () => {
      cbs.orchdTasksChanged({ projectId: "p1" });
    });
    expect(orchdListTasksMock).toHaveBeenCalledWith("p1");
    expect(useAppStore.getState().tasksByProject["p1"]).toHaveLength(1);
  });

  it("orchd://graph-changed refreshes ONLY the named project's graph, unconditionally (audit #5.1 — no loaded/active gating)", async () => {
    orchdGraphListProjectMock.mockResolvedValue({
      nodes: [
        { id: "n1", projectId: "p1", kind: "concept", entityType: null, entityId: null,
          label: "n", body: "", posX: 0, posY: 0, createdAt: 1, updatedAt: 1, isOrphan: false },
      ],
      edges: [],
      externalNodes: [],
    });
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    // No project panel open (activeProjectId stays null) — the refresh must still fire, since
    // there is no loaded/active gating to mirror (see App.tsx's comment on this listener).
    expect(useAppStore.getState().activeProjectId).toBeNull();
    await act(async () => {
      cbs.orchdGraphChanged({ projectId: "p1" });
    });
    expect(orchdGraphListProjectMock).toHaveBeenCalledWith("p1");
    expect(useAppStore.getState().graphByProject["p1"]?.nodes).toHaveLength(1);
  });

  it("orchd://ideas-changed and orchd://insights-changed re-fetch their whole-store lists", async () => {
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    await act(async () => {
      cbs.orchdIdeasChanged(null);
    });
    expect(orchdListIdeasMock).toHaveBeenCalledWith(null);
    await act(async () => {
      cbs.orchdInsightsChanged(null);
    });
    expect(orchdListInsightsMock).toHaveBeenCalledWith(null);
  });

  it("orchd://research-runs-changed refreshes ONLY the named idea's runs, unconditionally (S-IDEA §5/§8, T6)", async () => {
    researchListRunsMock.mockResolvedValue([
      {
        id: "r1", ideaId: "i1", serverId: "s1", toolName: "search", argsJson: "{}",
        status: "pending", invocationId: null, artifactId: null, errorKind: null,
        createdAt: 1, updatedAt: 1,
      },
    ]);
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    await act(async () => {
      cbs.orchdResearchRunsChanged({ ideaId: "i1" });
    });
    expect(researchListRunsMock).toHaveBeenCalledWith("i1");
    expect(useAppStore.getState().researchRunsByIdea["i1"]).toHaveLength(1);
  });

  it("orchd://research-runs-changed with a null ideaId is a defensive no-op (never fetches)", async () => {
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    researchListRunsMock.mockClear();
    await act(async () => {
      cbs.orchdResearchRunsChanged({ ideaId: null });
    });
    expect(researchListRunsMock).not.toHaveBeenCalled();
  });

  it('orchd://ruleset-changed builds the "global" key for the global scope', async () => {
    orchdGetRulesetMock.mockResolvedValue({
      rule: {
        id: "r1", scope: "global", projectId: null, mdPath: "/x", mdHash: "h",
        policy: { spendCapUsd: null, approvalClasses: [], pathAllowlist: [] },
        createdAt: 1, updatedAt: 1,
      },
      mdContent: null,
      fileState: "ok",
    });
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    await act(async () => {
      cbs.orchdRulesetChanged({ scope: "global", projectId: null });
    });
    expect(orchdGetRulesetMock).toHaveBeenCalledWith("global", null);
    expect(useAppStore.getState().rulesets["global"]).toBeTruthy();
  });

  it('orchd://ruleset-changed builds the "project:<id>" key for a project scope', async () => {
    orchdGetRulesetMock.mockResolvedValue({
      rule: {
        id: "r2", scope: "project", projectId: "p1", mdPath: "/x", mdHash: "h",
        policy: { spendCapUsd: null, approvalClasses: [], pathAllowlist: [] },
        createdAt: 1, updatedAt: 1,
      },
      mdContent: null,
      fileState: "ok",
    });
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    await act(async () => {
      cbs.orchdRulesetChanged({ scope: "project", projectId: "p1" });
    });
    expect(orchdGetRulesetMock).toHaveBeenCalledWith("project", "p1");
    expect(useAppStore.getState().rulesets["project:p1"]).toBeTruthy();
  });

  it("orchd://down sets orchdDown; orchd://up clears it", async () => {
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    expect(useAppStore.getState().orchdDown).toBe(false);
    await act(async () => {
      cbs.orchdDown(null);
    });
    expect(useAppStore.getState().orchdDown).toBe(true);
    await act(async () => {
      cbs.orchdUp(null);
    });
    expect(useAppStore.getState().orchdDown).toBe(false);
  });

  it("orchd://up re-fetches the project list (self-heals a lost initial-load race, review fix)", async () => {
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    // Simulate the cold-boot race: the initial refreshProjects() lost (orchd not yet connected).
    // Clear the mock so we only observe the up-driven refetch, and confirm nothing has populated.
    orchdListProjectsMock.mockClear();
    orchdListProjectsMock.mockResolvedValue([
      { id: "p1", name: "Acme", description: "", status: "active", workspaceIds: [],
        createdAt: 1, updatedAt: 1 },
    ]);
    expect(useAppStore.getState().projects).toEqual([]);

    await act(async () => {
      cbs.orchdUp(null);
    });

    // orchd://up must re-trigger the project load (not merely clear orchdDown), so the core
    // "open the app → see my projects" path populates once orchd finally connects.
    expect(orchdListProjectsMock).toHaveBeenCalled();
    expect(useAppStore.getState().projects).toHaveLength(1);
  });

  it("orchd://up with an open project also refreshes that project's goals/tasks/ideas/insights/ruleset/graph", async () => {
    orchdGetRulesetMock.mockResolvedValue({
      rule: {
        id: "r1", scope: "project", projectId: "p1", mdPath: "/x", mdHash: "h",
        policy: { spendCapUsd: null, approvalClasses: [], pathAllowlist: [] },
        createdAt: 1, updatedAt: 1,
      },
      mdContent: null,
      fileState: "ok",
    });
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    act(() => useAppStore.getState().openProject("p1"));
    orchdListGoalsMock.mockClear();
    orchdListTasksMock.mockClear();
    orchdListIdeasMock.mockClear();
    orchdListInsightsMock.mockClear();
    orchdGetRulesetMock.mockClear();
    orchdGraphListProjectMock.mockClear();

    await act(async () => {
      cbs.orchdUp(null);
    });

    expect(orchdListGoalsMock).toHaveBeenCalledWith("p1");
    expect(orchdListTasksMock).toHaveBeenCalledWith("p1");
    expect(orchdListIdeasMock).toHaveBeenCalledWith(null);
    expect(orchdListInsightsMock).toHaveBeenCalledWith(null);
    expect(orchdGetRulesetMock).toHaveBeenCalledWith("project", "p1");
    // T6 review must-not-drop item (b): the Граф tab must not stay stale after a reconnect any
    // more than the sibling domain surfaces above do.
    expect(orchdGraphListProjectMock).toHaveBeenCalledWith("p1");
  });

  it("orchd://up with NO project open refreshes only the project list (no per-project fetches)", async () => {
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    orchdListGoalsMock.mockClear();
    orchdListTasksMock.mockClear();
    orchdGraphListProjectMock.mockClear();

    await act(async () => {
      cbs.orchdUp(null);
    });

    expect(orchdListGoalsMock).not.toHaveBeenCalled();
    expect(orchdListTasksMock).not.toHaveBeenCalled();
    expect(orchdGraphListProjectMock).not.toHaveBeenCalled();
  });

  it("orchd://incompatible sets orchdIncompatible (never auto-clears)", async () => {
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    expect(useAppStore.getState().orchdIncompatible).toBe(false);
    await act(async () => {
      cbs.orchdIncompatible(null);
    });
    expect(useAppStore.getState().orchdIncompatible).toBe(true);
  });
});

describe("T18: view='project' renders ProjectPanel", () => {
  it("renders ProjectPanel for the active project once openProject is called", async () => {
    orchdListProjectsMock.mockResolvedValue([
      { id: "p1", name: "Proj A", description: "", status: "active", workspaceIds: [], createdAt: 1, updatedAt: 1 },
    ]);
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    await act(async () => {
      useAppStore.getState().openProject("p1");
    });

    expect(useAppStore.getState().view).toBe("project");
    const panel = screen.getByTestId("project-panel");
    expect(panel).toBeTruthy();
    // "Proj A" also appears as the sidebar's project-group header now (T18) — scope to the panel.
    expect(within(panel).getByText("Proj A")).toBeTruthy();
    expect(screen.queryByTestId("home-stats")).toBeNull();
  });

  it("view='project' with no active project renders nothing (guarded, no crash)", async () => {
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    await act(async () => {
      useAppStore.getState().setView("project");
    });

    expect(useAppStore.getState().activeProjectId).toBeNull();
    expect(screen.queryByTestId("project-panel")).toBeNull();
    expect(screen.queryByTestId("project-panel-loading")).toBeNull();
  });
});

describe("S-EXT §8 T8: «Расширения» view + MCP event wiring", () => {
  it("clicking the «Расширения» sidebar button sets view to \"ext\" and renders ExtPanel", async () => {
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    expect(screen.queryByTestId("ext-panel-mock")).toBeNull();

    await act(async () => {
      screen.getByTestId("ext-nav-button").click();
    });

    expect(useAppStore.getState().view).toBe("ext");
    expect(screen.getByTestId("ext-panel-mock")).toBeTruthy();
  });

  it("orchd://mcp-servers-changed re-fetches the MCP server list", async () => {
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    mcpListServersMock.mockClear();
    await act(async () => {
      cbs.orchdMcpServersChanged({ projectId: null });
    });
    expect(mcpListServersMock).toHaveBeenCalledWith(null);
  });

  it("orchd://mcp-tools-changed refreshes ONLY the named server's tools", async () => {
    mcpListToolsMock.mockResolvedValue([
      {
        id: "t1",
        serverId: "s1",
        name: "search",
        title: null,
        description: null,
        inputSchemaJson: "{}",
        enabled: true,
        fetchedAt: 1,
      },
    ]);
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    await act(async () => {
      cbs.orchdMcpToolsChanged({ serverId: "s1" });
    });
    expect(mcpListToolsMock).toHaveBeenCalledWith("s1");
    expect(useAppStore.getState().mcpToolsByServer["s1"]).toHaveLength(1);
  });

  it("orchd://mcp-artifacts-changed re-fetches the whole-store artifacts list", async () => {
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    mcpListArtifactsMock.mockClear();
    await act(async () => {
      cbs.orchdMcpArtifactsChanged({ projectId: null });
    });
    expect(mcpListArtifactsMock).toHaveBeenCalledWith(null, null, null);
  });

  it("orchd://connectors-changed re-fetches the accounts list (S-EXT §8, T13b)", async () => {
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    connectorListAccountsMock.mockClear();
    await act(async () => {
      cbs.orchdConnectorsChanged(null);
    });
    expect(connectorListAccountsMock).toHaveBeenCalledWith();
  });

  it("orchd://skills-changed re-fetches the skills list (S-EXT §8, D11, T17)", async () => {
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    skillListMock.mockClear();
    await act(async () => {
      cbs.orchdSkillsChanged({ projectId: null });
    });
    expect(skillListMock).toHaveBeenCalledWith(null);
  });

  it("orchd://mcp-invocation-logged re-fetches the whole-store invocations list (S-EXT §8, T18)", async () => {
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    mcpListInvocationsMock.mockClear();
    await act(async () => {
      cbs.orchdMcpInvocationLogged({ serverId: "s1" });
    });
    expect(mcpListInvocationsMock).toHaveBeenCalledWith(null, null, null);
  });

  it("orchd://policies-changed re-fetches the policies list (S-EXT §4/§6/§8, BL-22, T18)", async () => {
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    trustListPoliciesMock.mockClear();
    await act(async () => {
      cbs.orchdPoliciesChanged(null);
    });
    expect(trustListPoliciesMock).toHaveBeenCalledWith();
  });
});
