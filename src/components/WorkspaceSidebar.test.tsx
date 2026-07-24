// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, act, fireEvent, within, waitFor } from "@testing-library/react";

const pickFolderMock = vi.fn();
const createWorkspaceMock = vi.fn();
const createSessionMock = vi.fn();
// SCN-058: the store's `removeWorkspace` action calls straight through to this wrapper.
const removeWorkspaceMock = vi.fn();
// SCN-059: the sidebar's LOCAL root-presence check. Defaults to "every root is there".
const pathsExistMock = vi.fn(async (paths: string[]) => paths.map(() => true));
vi.mock("../ipc/commands", () => ({
  pickFolder: (...a: unknown[]) => pickFolderMock(...a),
  createWorkspace: (...a: unknown[]) => createWorkspaceMock(...a),
  createSession: (...a: unknown[]) => createSessionMock(...a),
  removeWorkspace: (...a: unknown[]) => removeWorkspaceMock(...a),
  pathsExist: (...a: [string[]]) => pathsExistMock(...a),
}));

const orchdAddProjectWorkspaceMock = vi.fn();
const orchdCreateProjectMock = vi.fn();
const describeOrchdErrorMock = vi.fn((..._a: unknown[]) => "orchestrator: error");
vi.mock("../ipc/orchd", () => ({
  orchdAddProjectWorkspace: (...a: unknown[]) => orchdAddProjectWorkspaceMock(...a),
  orchdCreateProject: (...a: unknown[]) => orchdCreateProjectMock(...a),
  orchdListProjects: vi.fn().mockResolvedValue([]),
  describeOrchdError: (...a: unknown[]) => describeOrchdErrorMock(...a),
}));

// SCN-045: the keep-awake pill's toggle goes through the store's `setKeepAwakeEnabled`, which
// calls straight through to `../ipc/power` — mocked so a click never hits the real `invoke()`.
const powerSetEnabledMock = vi.fn();
const powerSyncSessionsMock = vi.fn();
vi.mock("../ipc/power", () => ({
  powerSetEnabled: (...a: unknown[]) => powerSetEnabledMock(...a),
  powerSyncSessions: (...a: unknown[]) => powerSyncSessionsMock(...a),
  powerStatus: vi.fn(),
}));

import { WorkspaceSidebar } from "./WorkspaceSidebar";
import { useAppStore } from "../store/store";
import { strings } from "../strings";
import type { Workspace } from "../ipc/types";
import type { WorkspaceId } from "../ipc/commands";
import type { Project } from "../ipc/orchd-types";

const wsA: Workspace = { id: "w1", name: "alpha", rootPath: "/p/alpha", roots: ["/p/alpha"] };
const wsB: Workspace = { id: "w2", name: "beta", rootPath: "/p/beta", roots: ["/p/beta"] };

/** Destructive flows go through `window.confirm` here, matching every other destructive surface
 * in the repo (FileTree delete, TasksList delete, ProjectPanel archive). */
const confirmMock = vi.fn((_message?: string) => true);

function makeProject(over: Partial<Project> = {}): Project {
  return {
    id: "p1",
    name: "Proj A",
    description: "",
    status: "active",
    workspaceIds: [],
    createdAt: 1,
    updatedAt: 1,
    ...over,
  };
}

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});
beforeEach(() => {
  // In-memory localStorage stub (repo convention — see `ThemeToggle.test.tsx`): the keep-awake
  // toggle persists its preference here (SCN-045 "persisted"), and this jsdom setup provides no
  // real localStorage of its own.
  const persisted = new Map<string, string>();
  vi.stubGlobal("localStorage", {
    getItem: (k: string) => persisted.get(k) ?? null,
    setItem: (k: string, v: string) => void persisted.set(k, v),
    removeItem: (k: string) => void persisted.delete(k),
    clear: () => persisted.clear(),
  });
  pickFolderMock.mockReset();
  createWorkspaceMock.mockReset();
  createWorkspaceMock.mockResolvedValue({ id: "w3", name: "gamma", rootPath: "/p/gamma", roots: ["/p/gamma"] });
  createSessionMock.mockReset().mockResolvedValue({ id: "s-auto" });
  removeWorkspaceMock.mockReset().mockResolvedValue(undefined);
  pathsExistMock.mockReset().mockImplementation(async (paths: string[]) => paths.map(() => true));
  confirmMock.mockReset().mockReturnValue(true);
  vi.stubGlobal("confirm", confirmMock);
  orchdAddProjectWorkspaceMock.mockReset().mockResolvedValue(makeProject());
  orchdCreateProjectMock.mockReset().mockResolvedValue(makeProject());
  describeOrchdErrorMock.mockReset().mockReturnValue("orchestrator: error");
  powerSetEnabledMock.mockReset().mockResolvedValue({ enabled: true, active: false, error: null });
  powerSyncSessionsMock
    .mockReset()
    .mockResolvedValue({ enabled: true, active: false, error: null });
  useAppStore.setState(
    {
      sessions: {},
      workspaces: { w1: wsA, w2: wsB },
      activeSessionId: null,
      daemonConnected: true,
      view: "home",
      projects: [],
      activeProjectId: null,
      toast: null, toastQueue: [],
      keepAwake: { enabled: true, active: false, error: null },
    },
    false,
  );
});

describe("WorkspaceSidebar", () => {
  it("renders one entry per workspace", () => {
    render(<WorkspaceSidebar activeWorkspaceId={null} onSelectWorkspace={() => {}} />);
    expect(screen.getByText("alpha")).toBeTruthy();
    expect(screen.getByText("beta")).toBeTruthy();
  });

  it("shows a dim empty-state sentence when there are zero projects AND zero workspaces (P-11)", () => {
    useAppStore.setState({ workspaces: {}, projects: [] }, false);
    render(<WorkspaceSidebar activeWorkspaceId={null} onSelectWorkspace={() => {}} />);
    expect(screen.getByTestId("sidebar-empty").textContent).toBe(strings.chrome.sidebar.emptyState);
    // The CTAs are still there — it's onboarding copy, not a dead end.
    expect(screen.getByText(strings.chrome.sidebar.addWorkspace)).toBeTruthy();
    expect(screen.getByText(strings.chrome.sidebar.addProject)).toBeTruthy();
  });

  it("does NOT show the empty-state once at least one workspace exists", () => {
    // default beforeEach state has w1/w2
    render(<WorkspaceSidebar activeWorkspaceId={null} onSelectWorkspace={() => {}} />);
    expect(screen.queryByTestId("sidebar-empty")).toBeNull();
  });

  it("clicking a workspace calls onSelectWorkspace with its id and switches view to workspace", () => {
    const onSelect = vi.fn();
    render(<WorkspaceSidebar activeWorkspaceId={null} onSelectWorkspace={onSelect} />);
    fireEvent.click(screen.getByText("alpha"));
    expect(onSelect).toHaveBeenCalledWith("w1");
    expect(useAppStore.getState().view).toBe("workspace");
  });

  it("renders a ⌂ Home item; clicking it sets view to home and highlights it", () => {
    useAppStore.setState({ view: "workspace" }, false);
    render(<WorkspaceSidebar activeWorkspaceId="w1" onSelectWorkspace={() => {}} />);
    const home = screen.getByRole("button", { name: /home/i });
    expect(home).toBeTruthy();
    fireEvent.click(home);
    expect(useAppStore.getState().view).toBe("home");
  });

  it("a workspace item is only shown as selected while view is workspace (not while on Home)", () => {
    useAppStore.setState({ view: "home" }, false);
    render(<WorkspaceSidebar activeWorkspaceId="w1" onSelectWorkspace={() => {}} />);
    const alpha = screen.getByText("alpha").closest("button")!;
    // Unselected rows render a transparent background (`theme.colors.bg` marks "selected" —
    // see WorkspaceSidebar's `selected` computation gating on `view === "workspace"`).
    expect(alpha.getAttribute("style")).toContain("background: transparent");
  });

  it('"Add workspace" opens the folder picker then creates a workspace named after the basename', async () => {
    pickFolderMock.mockResolvedValue("/Users/me/projects/my-app");
    render(<WorkspaceSidebar activeWorkspaceId={null} onSelectWorkspace={() => {}} />);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /add workspace/i }));
    });
    expect(pickFolderMock).toHaveBeenCalledTimes(1);
    expect(createWorkspaceMock).toHaveBeenCalledWith("my-app", "/Users/me/projects/my-app");
    expect(useAppStore.getState().view).toBe("workspace");
  });

  it("is a no-op when the folder picker is cancelled (null)", async () => {
    pickFolderMock.mockResolvedValue(null);
    render(<WorkspaceSidebar activeWorkspaceId={null} onSelectWorkspace={() => {}} />);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /add workspace/i }));
    });
    expect(pickFolderMock).toHaveBeenCalledTimes(1);
    expect(createWorkspaceMock).not.toHaveBeenCalled();
    expect(useAppStore.getState().toast).toBeNull(); // cancel is not an error
  });

  it("a failed add-workspace surfaces an honest toast instead of a silent no-op (BL-93 / P-03)", async () => {
    pickFolderMock.mockResolvedValue("/Users/me/projects/my-app");
    createWorkspaceMock.mockReset().mockRejectedValueOnce({ kind: "disconnected" });
    render(<WorkspaceSidebar activeWorkspaceId={null} onSelectWorkspace={() => {}} />);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /add workspace/i }));
      await Promise.resolve();
    });
    expect(useAppStore.getState().toast).toBe(
      strings.chrome.sidebar.addWorkspaceFailed(strings.errors.command.disconnected),
    );
  });

  // First-run fast-path (SCN-056 / IMP-01): cold start + add workspace => the first terminal is
  // auto-spawned so the aha (a live terminal) needs no manual "+ New terminal" click.
  it("SCN-056: cold start (zero sessions) — adding a workspace auto-spawns the first terminal", async () => {
    pickFolderMock.mockResolvedValue("/Users/me/projects/my-app");
    render(<WorkspaceSidebar activeWorkspaceId={null} onSelectWorkspace={() => {}} />);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /add workspace/i }));
      await Promise.resolve();
    });
    expect(createSessionMock).toHaveBeenCalledTimes(1);
    // SCN-056: the fast-path terminal spawns with cwd = the new workspace's root (AUD-2026-07-23-17),
    // not an omitted cwd (which sessiond would default to $HOME).
    expect(createSessionMock).toHaveBeenCalledWith("w3", { cwd: "/p/gamma", cols: 80, rows: 24 });
    expect(useAppStore.getState().view).toBe("workspace");
  });

  it("SCN-056: steady state (sessions exist) — adding a workspace spawns NO surprise terminal", async () => {
    useAppStore.setState(
      { sessions: { s1: { id: "s1" } as unknown as never } },
      false,
    );
    pickFolderMock.mockResolvedValue("/Users/me/projects/my-app");
    render(<WorkspaceSidebar activeWorkspaceId={null} onSelectWorkspace={() => {}} />);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /add workspace/i }));
      await Promise.resolve();
    });
    expect(createSessionMock).not.toHaveBeenCalled();
    expect(useAppStore.getState().view).toBe("workspace");
  });

  it("SCN-056: a failed auto-spawn degrades to the manual path — workspace opens, honest toast, never a blocked first run", async () => {
    pickFolderMock.mockResolvedValue("/Users/me/projects/my-app");
    createSessionMock.mockReset().mockRejectedValueOnce({ kind: "disconnected" });
    render(<WorkspaceSidebar activeWorkspaceId={null} onSelectWorkspace={() => {}} />);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /add workspace/i }));
      await Promise.resolve();
    });
    // navigation already happened — the failure only costs the shortcut
    expect(useAppStore.getState().view).toBe("workspace");
    expect(useAppStore.getState().toast).toBe(
      strings.terminal.tabs.newTerminalFailed(strings.errors.command.disconnected),
    );
  });

  it("groups linked workspaces under their project header; the remainder lands in «No project»", () => {
    useAppStore.setState({ projects: [makeProject({ id: "p1", name: "Proj A", workspaceIds: ["w1"] })] }, false);
    render(<WorkspaceSidebar activeWorkspaceId={null} onSelectWorkspace={() => {}} />);

    const group = within(screen.getByTestId("project-group-p1"));
    expect(group.getByText("Proj A")).toBeTruthy();
    expect(group.getByText("alpha")).toBeTruthy();
    expect(group.queryByText("beta")).toBeNull();

    const unassigned = within(screen.getByTestId("project-group-unassigned"));
    expect(unassigned.getByText("beta")).toBeTruthy();
    expect(unassigned.queryByText("alpha")).toBeNull();
  });

  it("clicking a project header opens the project (openProject)", () => {
    useAppStore.setState({ projects: [makeProject({ id: "p1", name: "Proj A", workspaceIds: ["w1"] })] }, false);
    render(<WorkspaceSidebar activeWorkspaceId={null} onSelectWorkspace={() => {}} />);

    fireEvent.click(screen.getByTestId("project-group-header-p1"));

    expect(useAppStore.getState().view).toBe("project");
    expect(useAppStore.getState().activeProjectId).toBe("p1");
  });

  it("clicking a workspace nested under a project keeps the existing select+navigate behavior", () => {
    useAppStore.setState({ projects: [makeProject({ id: "p1", name: "Proj A", workspaceIds: ["w1"] })] }, false);
    const onSelect = vi.fn();
    render(<WorkspaceSidebar activeWorkspaceId={null} onSelectWorkspace={onSelect} />);

    fireEvent.click(within(screen.getByTestId("project-group-p1")).getByText("alpha"));

    expect(onSelect).toHaveBeenCalledWith("w1");
    expect(useAppStore.getState().view).toBe("workspace");
  });

  it('an unlinked workspace\'s "link…" select calls orchdAddProjectWorkspace with the chosen project', async () => {
    useAppStore.setState({ projects: [makeProject({ id: "p1", name: "Proj A", workspaceIds: [] })] }, false);
    render(<WorkspaceSidebar activeWorkspaceId={null} onSelectWorkspace={() => {}} />);

    const select = screen.getByTestId("attach-workspace-w1") as HTMLSelectElement;
    fireEvent.change(select, { target: { value: "p1" } });

    await waitFor(() => {
      expect(orchdAddProjectWorkspaceMock).toHaveBeenCalledWith("p1", "w1");
    });
  });

  it('"+ project" opens CreateProjectDialog', () => {
    render(<WorkspaceSidebar activeWorkspaceId={null} onSelectWorkspace={() => {}} />);

    fireEvent.click(screen.getByTestId("create-project-open"));

    expect(screen.getByTestId("create-project-dialog")).toBeTruthy();
  });

  // ---- archived projects group (O-3, spec D7) ----

  it("no «Archived» group renders when every project is active", () => {
    useAppStore.setState({ projects: [makeProject({ id: "p1", status: "active" })] }, false);
    render(<WorkspaceSidebar activeWorkspaceId={null} onSelectWorkspace={() => {}} />);
    expect(screen.queryByTestId("archived-projects-group")).toBeNull();
  });

  it("an archived project lands in a collapsed, dimmed «Archived» group — not in the main groups", () => {
    useAppStore.setState(
      {
        projects: [
          makeProject({ id: "p1", name: "Live", status: "active" }),
          makeProject({ id: "p2", name: "Old", status: "archived" }),
        ],
      },
      false,
    );
    render(<WorkspaceSidebar activeWorkspaceId={null} onSelectWorkspace={() => {}} />);

    // Active project keeps its first-class group; the archived one does NOT.
    expect(screen.getByTestId("project-group-p1")).toBeTruthy();
    expect(screen.queryByTestId("project-group-p2")).toBeNull();

    // The group exists and is dimmed; collapsed by default (archived row not yet rendered).
    const group = screen.getByTestId("archived-projects-group");
    expect(group.getAttribute("style")).toContain("opacity: 0.6");
    expect(screen.getByTestId("archived-projects-toggle").textContent).toContain(
      strings.chrome.sidebar.archivedGroup(1),
    );
    expect(screen.queryByTestId("archived-project-p2")).toBeNull();
  });

  it("expanding «Archived» reveals the archived project; clicking it opens the project", () => {
    useAppStore.setState(
      { projects: [makeProject({ id: "p2", name: "Old", status: "archived" })] },
      false,
    );
    render(<WorkspaceSidebar activeWorkspaceId={null} onSelectWorkspace={() => {}} />);

    fireEvent.click(screen.getByTestId("archived-projects-toggle"));
    const row = screen.getByTestId("archived-project-p2");
    expect(row.textContent).toBe("Old");

    fireEvent.click(row);
    expect(useAppStore.getState().view).toBe("project");
    expect(useAppStore.getState().activeProjectId).toBe("p2");
  });

  // ---- S-EXT §8, T8: «Extensions» nav button ----

  it("renders a «Extensions» nav item; clicking it sets view to \"ext\" and highlights it", () => {
    useAppStore.setState({ view: "workspace" }, false);
    render(<WorkspaceSidebar activeWorkspaceId="w1" onSelectWorkspace={() => {}} />);
    const ext = screen.getByTestId("ext-nav-button");
    expect(ext).toBeTruthy();
    fireEvent.click(ext);
    expect(useAppStore.getState().view).toBe("ext");
  });
});

describe("keep-awake pill (SCN-045 / FLW-18)", () => {
  function pill(): HTMLElement {
    return screen.getByTestId("keep-awake-pill");
  }
  function dotStyle(): string {
    return screen.getByTestId("keep-awake-dot").getAttribute("style") ?? "";
  }

  it("renders in the footer as enabled-but-idle by default: idle label + muted dot", () => {
    render(<WorkspaceSidebar activeWorkspaceId={null} onSelectWorkspace={() => {}} />);
    expect(pill().textContent).toContain(strings.power.keepAwakeIdle);
    expect(pill().getAttribute("aria-pressed")).toBe("true");
    expect(dotStyle()).toContain("var(--muted)");
  });

  it("shows the active state (assertion held) with the ok-tone dot", () => {
    useAppStore.setState({ keepAwake: { enabled: true, active: true, error: null } }, false);
    render(<WorkspaceSidebar activeWorkspaceId={null} onSelectWorkspace={() => {}} />);
    expect(pill().textContent).toContain(strings.power.keepAwakeOn);
    expect(dotStyle()).toContain("var(--ok)");
  });

  it("shows the disabled state with a muted dot", () => {
    useAppStore.setState({ keepAwake: { enabled: false, active: false, error: null } }, false);
    render(<WorkspaceSidebar activeWorkspaceId={null} onSelectWorkspace={() => {}} />);
    expect(pill().textContent).toContain(strings.power.keepAwakeOff);
    expect(pill().getAttribute("aria-pressed")).toBe("false");
    expect(dotStyle()).toContain("var(--muted)");
  });

  it("shows the honest failure state: danger dot + \"keep-awake unavailable: {reason}\" label", () => {
    useAppStore.setState(
      { keepAwake: { enabled: true, active: false, error: "os denied" } },
      false,
    );
    render(<WorkspaceSidebar activeWorkspaceId={null} onSelectWorkspace={() => {}} />);
    expect(pill().textContent).toContain(strings.power.keepAwakeFailed("os denied"));
    expect(dotStyle()).toContain("var(--danger)");
  });

  it("clicking the pill toggles: powerSetEnabled(false) is called, state mirrors the reply, and the preference persists", async () => {
    powerSetEnabledMock.mockResolvedValueOnce({ enabled: false, active: false, error: null });
    render(<WorkspaceSidebar activeWorkspaceId={null} onSelectWorkspace={() => {}} />);
    await act(async () => {
      fireEvent.click(pill());
    });
    expect(powerSetEnabledMock).toHaveBeenCalledWith(false);
    expect(useAppStore.getState().keepAwake.enabled).toBe(false);
    // SCN-045 "persisted": survives a restart via localStorage (same path the theme uses).
    expect(localStorage.getItem("bpa-keep-awake")).toBe("off");
  });

  it("clicking again re-enables (round-trip)", async () => {
    useAppStore.setState({ keepAwake: { enabled: false, active: false, error: null } }, false);
    powerSetEnabledMock.mockResolvedValueOnce({ enabled: true, active: true, error: null });
    render(<WorkspaceSidebar activeWorkspaceId={null} onSelectWorkspace={() => {}} />);
    await act(async () => {
      fireEvent.click(pill());
    });
    expect(powerSetEnabledMock).toHaveBeenCalledWith(true);
    expect(useAppStore.getState().keepAwake).toEqual({
      enabled: true,
      active: true,
      error: null,
    });
    expect(localStorage.getItem("bpa-keep-awake")).toBe("on");
  });
});

// ── SCN-058: remove a workspace ────────────────────────────────────────────────────────────────

describe("remove a workspace (SCN-058)", () => {
  /** Let the mount-time `pathsExist` effect settle before asserting. */
  async function renderSidebar(
    activeWorkspaceId: WorkspaceId | null = null,
    onSelect: (id: WorkspaceId | null) => void = () => {},
  ): Promise<void> {
    await act(async () => {
      render(
        <WorkspaceSidebar activeWorkspaceId={activeWorkspaceId} onSelectWorkspace={onSelect} />,
      );
    });
  }

  it("every workspace row carries a «Remove workspace» control", async () => {
    await renderSidebar();
    expect(screen.getByTestId("remove-workspace-w1")).toBeTruthy();
    expect(screen.getByTestId("remove-workspace-w2")).toBeTruthy();
    expect(
      screen.getByRole("button", { name: strings.chrome.sidebar.removeWorkspaceAria("alpha") }),
    ).toBeTruthy();
  });

  it("confirms FIRST, naming the workspace and the live-terminal consequence, and only then removes", async () => {
    await renderSidebar();
    await act(async () => {
      fireEvent.click(screen.getByTestId("remove-workspace-w1"));
    });
    expect(confirmMock).toHaveBeenCalledWith(
      strings.chrome.sidebar.removeWorkspaceConfirm("alpha"),
    );
    // The confirmation states the consequence before anything is committed.
    const shown = String(confirmMock.mock.calls[0][0]);
    expect(shown).toContain("alpha");
    expect(shown).toContain("terminals will be closed");
    expect(shown).toContain("scrollback");
    expect(removeWorkspaceMock).toHaveBeenCalledWith("w1");
  });

  it("a successful removal drops the workspace and its sessions from the store", async () => {
    useAppStore.setState(
      {
        sessions: {
          s1: { id: "s1", workspaceId: "w1" } as unknown as never,
          s2: { id: "s2", workspaceId: "w2" } as unknown as never,
        },
      },
      false,
    );
    await renderSidebar();
    await act(async () => {
      fireEvent.click(screen.getByTestId("remove-workspace-w1"));
    });
    expect(Object.keys(useAppStore.getState().workspaces)).toEqual(["w2"]);
    expect(Object.keys(useAppStore.getState().sessions)).toEqual(["s2"]);
  });

  it("cancelling the confirmation removes nothing and has no side effects", async () => {
    confirmMock.mockReturnValue(false);
    await renderSidebar();
    await act(async () => {
      fireEvent.click(screen.getByTestId("remove-workspace-w1"));
    });
    expect(removeWorkspaceMock).not.toHaveBeenCalled();
    expect(Object.keys(useAppStore.getState().workspaces).sort()).toEqual(["w1", "w2"]);
    expect(useAppStore.getState().toast).toBeNull();
  });

  it("a rejected removal keeps the row and surfaces an honest toast", async () => {
    removeWorkspaceMock.mockRejectedValueOnce({ kind: "disconnected" });
    await renderSidebar();
    await act(async () => {
      fireEvent.click(screen.getByTestId("remove-workspace-w1"));
      await Promise.resolve();
    });
    expect(useAppStore.getState().workspaces["w1"]).toBeTruthy();
    expect(screen.getByTestId("remove-workspace-w1")).toBeTruthy();
    expect(useAppStore.getState().toast).toBe(
      strings.chrome.sidebar.removeWorkspaceFailed(strings.errors.command.disconnected),
    );
  });

  it("removing the workspace currently on screen falls back to Home — never a dead view", async () => {
    const onSelect = vi.fn();
    useAppStore.setState({ view: "workspace" }, false);
    await renderSidebar("w1", onSelect);
    await act(async () => {
      fireEvent.click(screen.getByTestId("remove-workspace-w1"));
    });
    expect(onSelect).toHaveBeenCalledWith(null);
    expect(useAppStore.getState().view).toBe("home");
  });

  it("removing a DIFFERENT workspace leaves the current view alone", async () => {
    const onSelect = vi.fn();
    useAppStore.setState({ view: "workspace" }, false);
    await renderSidebar("w1", onSelect);
    await act(async () => {
      fireEvent.click(screen.getByTestId("remove-workspace-w2"));
    });
    expect(onSelect).not.toHaveBeenCalled();
    expect(useAppStore.getState().view).toBe("workspace");
  });
});

// ── SCN-059: clear out workspaces whose folder is gone ─────────────────────────────────────────

describe("stale workspace clean-up (SCN-059)", () => {
  async function renderSidebar(
    activeWorkspaceId: WorkspaceId | null = null,
    onSelect: (id: WorkspaceId | null) => void = () => {},
  ): Promise<void> {
    await act(async () => {
      render(
        <WorkspaceSidebar activeWorkspaceId={activeWorkspaceId} onSelectWorkspace={onSelect} />,
      );
    });
  }

  /** `paths_exist` verdicts by path — anything unlisted counts as present. */
  function presence(map: Record<string, boolean>): void {
    pathsExistMock.mockImplementation(async (paths: string[]) =>
      paths.map((p) => map[p] ?? true),
    );
  }

  it("marks only the workspaces whose folder is definitely gone", async () => {
    presence({ "/p/alpha": false });
    await renderSidebar();
    expect(screen.getByTestId("workspace-missing-w1").textContent).toBe(
      strings.chrome.sidebar.rootMissing,
    );
    expect(screen.queryByTestId("workspace-missing-w2")).toBeNull();
  });

  it("offers NO clean-up control when every folder is there (no dead control)", async () => {
    await renderSidebar();
    expect(screen.queryByTestId("cleanup-missing-workspaces")).toBeNull();
  });

  it("the clean-up control names the exact count, and so does its confirmation", async () => {
    presence({ "/p/alpha": false, "/p/beta": false });
    await renderSidebar();
    const button = screen.getByTestId("cleanup-missing-workspaces");
    expect(button.textContent).toBe(strings.chrome.sidebar.cleanupMissing(2));
    await act(async () => {
      fireEvent.click(button);
    });
    expect(confirmMock).toHaveBeenCalledWith(strings.chrome.sidebar.cleanupMissingConfirm(2));
    expect(String(confirmMock.mock.calls[0][0])).toContain("2");
  });

  it("removes ONLY the missing workspaces — a healthy one is never touched", async () => {
    presence({ "/p/alpha": false });
    await renderSidebar();
    await act(async () => {
      fireEvent.click(screen.getByTestId("cleanup-missing-workspaces"));
    });
    expect(removeWorkspaceMock).toHaveBeenCalledTimes(1);
    expect(removeWorkspaceMock).toHaveBeenCalledWith("w1");
    expect(Object.keys(useAppStore.getState().workspaces)).toEqual(["w2"]);
  });

  it("cancelling the clean-up removes nothing", async () => {
    presence({ "/p/alpha": false });
    confirmMock.mockReturnValue(false);
    await renderSidebar();
    await act(async () => {
      fireEvent.click(screen.getByTestId("cleanup-missing-workspaces"));
    });
    expect(removeWorkspaceMock).not.toHaveBeenCalled();
  });

  it("a partial failure keeps the successes and names how many failed (no silent partial success)", async () => {
    presence({ "/p/alpha": false, "/p/beta": false });
    removeWorkspaceMock.mockImplementation(async (id: string) => {
      if (id === "w1") throw { kind: "disconnected" };
    });
    await renderSidebar();
    await act(async () => {
      fireEvent.click(screen.getByTestId("cleanup-missing-workspaces"));
      await Promise.resolve();
    });
    expect(Object.keys(useAppStore.getState().workspaces)).toEqual(["w1"]); // w2 removed, w1 stands
    expect(useAppStore.getState().toast).toBe(
      strings.chrome.sidebar.cleanupMissingPartial(1, 2),
    );
  });

  it("a multi-root workspace with ONE surviving root is NOT missing (never removed on a guess)", async () => {
    useAppStore.setState(
      {
        workspaces: {
          w1: { id: "w1", name: "alpha", rootPath: "/p/alpha", roots: ["/p/alpha", "/p/second"] },
        },
      },
      false,
    );
    presence({ "/p/alpha": false, "/p/second": true });
    await renderSidebar();
    expect(screen.queryByTestId("workspace-missing-w1")).toBeNull();
    expect(screen.queryByTestId("cleanup-missing-workspaces")).toBeNull();
  });

  it("an unreadable root (reported present by the core) leaves the row healthy", async () => {
    // `paths_exist` maps a permission error to `true` — the frontend must simply believe it.
    presence({ "/p/alpha": true });
    await renderSidebar();
    expect(screen.queryByTestId("workspace-missing-w1")).toBeNull();
  });

  it("a FAILED presence check marks nothing missing and offers no clean-up", async () => {
    pathsExistMock.mockRejectedValue(new Error("core down"));
    await renderSidebar();
    expect(screen.queryByTestId("workspace-missing-w1")).toBeNull();
    expect(screen.queryByTestId("cleanup-missing-workspaces")).toBeNull();
  });
});

// ── sidebar layout contract (Part 3; jsdom does no layout, so the STYLE contract is asserted) ──

describe("WorkspaceSidebar layout", () => {
  it("the list region can actually shrink and does not sit flush against the footer", async () => {
    await act(async () => {
      render(<WorkspaceSidebar activeWorkspaceId={null} onSelectWorkspace={() => {}} />);
    });
    const region = screen.getByTestId("sidebar-scroll");
    expect(region.style.flex).toContain("1");
    expect(region.style.minHeight).toBe("0"); // without it, a long list pushes the footer away
    expect(region.style.overflowY).toBe("auto");
    expect(region.style.paddingBottom).not.toBe("");
    expect(region.style.scrollbarGutter).toBe("stable");
  });

  it("the footer is one non-shrinking block with a separating hairline", async () => {
    await act(async () => {
      render(<WorkspaceSidebar activeWorkspaceId={null} onSelectWorkspace={() => {}} />);
    });
    const footer = screen.getByTestId("sidebar-footer");
    expect(footer.style.flexShrink).toBe("0");
    expect(footer.style.borderTop).toContain("var(--hairline)");
    // Every footer control lives inside it — none is a bare, individually-shrinkable sibling.
    expect(footer.contains(screen.getByTestId("keep-awake-pill"))).toBe(true);
    expect(footer.contains(screen.getByTestId("create-project-open"))).toBe(true);
    expect(footer.contains(screen.getByTestId("diag-open"))).toBe(true);
    expect(
      footer.contains(screen.getByRole("button", { name: /add workspace/i })),
    ).toBe(true);
  });
});

describe("Inbox nav (SCN-028 / AUD-2026-07-19-11)", () => {
  it("renders the Inbox nav and switches view on click", () => {
    render(<WorkspaceSidebar activeWorkspaceId={null} onSelectWorkspace={() => {}} />);
    const btn = screen.getByTestId("inbox-nav-button");
    expect(btn.textContent).toContain(strings.chrome.sidebar.inboxNav);
    fireEvent.click(btn);
    expect(useAppStore.getState().view).toBe("inbox");
  });

  it("shows the orphan count badge only when orphan ideas/insights exist", () => {
    useAppStore.setState({
      ideas: [
        { id: "i1", projectId: null, title: "a", body: "", lifecycle: "captured", createdAt: 1, updatedAt: 1 },
        { id: "i2", projectId: "p1", title: "b", body: "", lifecycle: "captured", createdAt: 2, updatedAt: 2 },
      ],
      insights: [
        { id: "n1", projectId: null, source: "", title: "c", body: "", fitVerdict: null, fitReasoning: "", status: "new", resolutionReasoning: "", createdAt: 3, updatedAt: 3 },
      ],
    });
    render(<WorkspaceSidebar activeWorkspaceId={null} onSelectWorkspace={() => {}} />);
    // 1 orphan idea + 1 orphan insight; the project-linked idea does not count.
    expect(screen.getByTestId("inbox-count").textContent).toBe("2");
  });

  it("hides the badge with zero orphans", () => {
    useAppStore.setState({ ideas: [], insights: [] });
    render(<WorkspaceSidebar activeWorkspaceId={null} onSelectWorkspace={() => {}} />);
    expect(screen.queryByTestId("inbox-count")).toBeNull();
  });
});
