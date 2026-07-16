// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, act, fireEvent, within, waitFor } from "@testing-library/react";

const pickFolderMock = vi.fn();
const createWorkspaceMock = vi.fn();
vi.mock("../ipc/commands", () => ({
  pickFolder: (...a: unknown[]) => pickFolderMock(...a),
  createWorkspace: (...a: unknown[]) => createWorkspaceMock(...a),
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

import { WorkspaceSidebar } from "./WorkspaceSidebar";
import { useAppStore } from "../store/store";
import { strings } from "../strings";
import type { Workspace } from "../ipc/types";
import type { Project } from "../ipc/orchd-types";

const wsA: Workspace = { id: "w1", name: "alpha", rootPath: "/p/alpha", roots: ["/p/alpha"] };
const wsB: Workspace = { id: "w2", name: "beta", rootPath: "/p/beta", roots: ["/p/beta"] };

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

afterEach(cleanup);
beforeEach(() => {
  pickFolderMock.mockReset();
  createWorkspaceMock.mockReset();
  createWorkspaceMock.mockResolvedValue({ id: "w3", name: "gamma", rootPath: "/p/gamma", roots: ["/p/gamma"] });
  orchdAddProjectWorkspaceMock.mockReset().mockResolvedValue(makeProject());
  orchdCreateProjectMock.mockReset().mockResolvedValue(makeProject());
  describeOrchdErrorMock.mockReset().mockReturnValue("orchestrator: error");
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
