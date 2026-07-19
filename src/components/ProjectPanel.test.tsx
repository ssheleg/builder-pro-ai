// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, fireEvent, waitFor } from "@testing-library/react";

vi.mock("./GoalTree", () => ({
  GoalTree: (props: { projectId: string }) => <div data-testid="marker-goals">{props.projectId}</div>,
}));
vi.mock("./IdeasList", () => ({
  IdeasList: (props: { projectId: string | null }) => <div data-testid="marker-ideas">{String(props.projectId)}</div>,
}));
vi.mock("./TasksList", () => ({
  TasksList: (props: { projectId: string }) => <div data-testid="marker-tasks">{props.projectId}</div>,
}));
vi.mock("./InsightsList", () => ({
  InsightsList: (props: { projectId: string | null }) => (
    <div data-testid="marker-insights">{String(props.projectId)}</div>
  ),
}));
vi.mock("./RulesetPanel", () => ({
  RulesetPanel: (props: { scope: string; projectId: string | null }) => (
    <div data-testid="marker-rules">{`${props.scope}:${String(props.projectId)}`}</div>
  ),
}));
vi.mock("./graph/GraphCanvas", () => ({
  GraphCanvas: (props: { projectId: string }) => <div data-testid="marker-graph">{props.projectId}</div>,
}));

const orchdListGoalsMock = vi.fn();
const orchdListTasksMock = vi.fn();
const orchdListIdeasMock = vi.fn();
const orchdListInsightsMock = vi.fn();
const orchdListProjectsMock = vi.fn();
const orchdAddProjectWorkspaceMock = vi.fn();
const orchdRemoveProjectWorkspaceMock = vi.fn();
const orchdArchiveProjectMock = vi.fn();
const orchdUnarchiveProjectMock = vi.fn();
const orchdExportProjectMock = vi.fn();
const orchdExportToFileMock = vi.fn();
const orchdImportFromFileMock = vi.fn();
const describeOrchdErrorMock = vi.fn((..._a: unknown[]) => "orchestrator: error");
vi.mock("../ipc/orchd", () => ({
  orchdListGoals: (...a: unknown[]) => orchdListGoalsMock(...a),
  orchdListTasks: (...a: unknown[]) => orchdListTasksMock(...a),
  orchdListIdeas: (...a: unknown[]) => orchdListIdeasMock(...a),
  orchdListInsights: (...a: unknown[]) => orchdListInsightsMock(...a),
  orchdListProjects: (...a: unknown[]) => orchdListProjectsMock(...a),
  orchdAddProjectWorkspace: (...a: unknown[]) => orchdAddProjectWorkspaceMock(...a),
  orchdRemoveProjectWorkspace: (...a: unknown[]) => orchdRemoveProjectWorkspaceMock(...a),
  orchdArchiveProject: (...a: unknown[]) => orchdArchiveProjectMock(...a),
  orchdUnarchiveProject: (...a: unknown[]) => orchdUnarchiveProjectMock(...a),
  orchdExportProject: (...a: unknown[]) => orchdExportProjectMock(...a),
  orchdExportToFile: (...a: unknown[]) => orchdExportToFileMock(...a),
  orchdImportFromFile: (...a: unknown[]) => orchdImportFromFileMock(...a),
  describeOrchdError: (...a: unknown[]) => describeOrchdErrorMock(...a),
}));

const pickFolderMock = vi.fn();
vi.mock("../ipc/commands", () => ({
  pickFolder: (...a: unknown[]) => pickFolderMock(...a),
}));

const listDirMock = vi.fn();
vi.mock("../ipc/fs", () => ({
  listDir: (...a: unknown[]) => listDirMock(...a),
}));

import { ProjectPanel } from "./ProjectPanel";
import { useAppStore } from "../store/store";
import { strings } from "../strings";
import type { Workspace } from "../ipc/types";
import type { Project, Idea, Insight } from "../ipc/orchd-types";

const wsA: Workspace = { id: "w1", name: "alpha", rootPath: "/p/alpha", roots: ["/p/alpha"] };
const wsB: Workspace = { id: "w2", name: "beta", rootPath: "/p/beta", roots: ["/p/beta"] };

function makeProject(over: Partial<Project> = {}): Project {
  return {
    id: "p1",
    name: "Proj",
    description: "some description",
    status: "active",
    workspaceIds: ["w1"],
    createdAt: 1,
    updatedAt: 1,
    ...over,
  };
}

function makeIdea(over: Partial<Idea> = {}): Idea {
  return {
    id: "i1",
    projectId: "p1",
    title: "idea",
    body: "",
    lifecycle: "captured",
    createdAt: 1,
    updatedAt: 1,
    ...over,
  };
}

function makeInsight(over: Partial<Insight> = {}): Insight {
  return {
    id: "in1",
    projectId: "p1",
    source: "",
    title: "insight",
    body: "",
    fitVerdict: null,
    fitReasoning: "",
    status: "new",
    resolutionReasoning: "",
    createdAt: 1,
    updatedAt: 1,
    ...over,
  };
}

afterEach(cleanup);

beforeEach(() => {
  orchdListGoalsMock.mockReset().mockResolvedValue([]);
  orchdListTasksMock.mockReset().mockResolvedValue([]);
  orchdListIdeasMock.mockReset().mockResolvedValue([]);
  orchdListInsightsMock.mockReset().mockResolvedValue([]);
  orchdListProjectsMock.mockReset().mockResolvedValue([makeProject()]);
  orchdAddProjectWorkspaceMock.mockReset().mockResolvedValue(makeProject());
  orchdRemoveProjectWorkspaceMock.mockReset().mockResolvedValue(makeProject());
  orchdArchiveProjectMock.mockReset().mockResolvedValue(makeProject({ status: "archived" }));
  orchdUnarchiveProjectMock.mockReset().mockResolvedValue(makeProject({ status: "active" }));
  orchdExportProjectMock.mockReset().mockResolvedValue("{}");
  orchdExportToFileMock.mockReset().mockResolvedValue("/dest/p1-export.json");
  orchdImportFromFileMock.mockReset().mockResolvedValue({
    projects: 1,
    goals: 1,
    ideas: 0,
    insights: 0,
    tasks: 0,
    rulesets: 1,
  });
  describeOrchdErrorMock.mockReset().mockReturnValue("orchestrator: error");
  pickFolderMock.mockReset();
  listDirMock.mockReset();

  useAppStore.setState(
    {
      projects: [makeProject()],
      workspaces: { w1: wsA, w2: wsB },
      goalsByProject: {},
      tasksByProject: {},
      ideas: [],
      insights: [],
      toast: null, toastQueue: [],
      orchdDown: false,
    },
    false,
  );
});

describe("ProjectPanel", () => {
  it("shows a loading state when the project is not (yet) in the store", () => {
    useAppStore.setState({ projects: [] }, false);
    render(<ProjectPanel projectId="missing" />);
    expect(screen.getByTestId("project-panel-loading")).toBeTruthy();
  });

  it("does NOT render the orchd-down banner while orchd is up", () => {
    render(<ProjectPanel projectId="p1" />);
    expect(screen.queryByTestId("orchd-down-banner")).toBeNull();
  });

  it("while orchdDown: renders the shared OrchdDownBanner above the tab bar (spec §10)", () => {
    useAppStore.setState({ orchdDown: true }, false);
    render(<ProjectPanel projectId="p1" />);
    const banner = screen.getByTestId("orchd-down-banner");
    expect(banner).toBeTruthy();
    expect(banner.getAttribute("role")).toBe("alert");
    expect(screen.getByText(strings.chrome.orchdUnavailable)).toBeTruthy();
  });

  it("renders the Overview tab by default with honest entity counters", async () => {
    orchdListGoalsMock.mockResolvedValue([
      { id: "g1", projectId: "p1", parentId: null, kind: "strategic", title: "t", body: "", ord: 0, status: "active", metricRefs: [], createdAt: 1, updatedAt: 1 },
      { id: "g2", projectId: "p1", parentId: "g1", kind: "additional", title: "t2", body: "", ord: 1, status: "active", metricRefs: [], createdAt: 1, updatedAt: 1 },
    ]);
    orchdListTasksMock.mockResolvedValue([
      { id: "tk1", projectId: "p1", parentId: null, title: "task", body: "", status: "backlog", source: "plan", sourceId: null, tags: [], rank: 1024, rankAgent: null, rankAgentReasoning: "", createdAt: 1, updatedAt: 1 },
    ]);
    orchdListIdeasMock.mockResolvedValue([makeIdea({ id: "i1", projectId: "p1" }), makeIdea({ id: "i2", projectId: "other" })]);
    orchdListInsightsMock.mockResolvedValue([makeInsight({ id: "in1", projectId: "p1" })]);

    render(<ProjectPanel projectId="p1" />);

    expect(screen.getByTestId("project-overview")).toBeTruthy();
    await waitFor(() => {
      expect(screen.getByTestId("project-counter-goals").textContent).toContain("2");
      expect(screen.getByTestId("project-counter-tasks").textContent).toContain("1");
      expect(screen.getByTestId("project-counter-ideas").textContent).toContain("1");
      expect(screen.getByTestId("project-counter-insights").textContent).toContain("1");
    });
  });

  it("tab switching renders each tab's (mocked) child and only that one", () => {
    render(<ProjectPanel projectId="p1" />);

    fireEvent.click(screen.getByTestId("project-tab-goals"));
    expect(screen.getByTestId("marker-goals").textContent).toBe("p1");
    expect(screen.queryByTestId("marker-ideas")).toBeNull();

    fireEvent.click(screen.getByTestId("project-tab-ideas"));
    expect(screen.getByTestId("marker-ideas")).toBeTruthy();
    expect(screen.queryByTestId("marker-goals")).toBeNull();

    fireEvent.click(screen.getByTestId("project-tab-tasks"));
    expect(screen.getByTestId("marker-tasks").textContent).toBe("p1");
    expect(screen.queryByTestId("marker-ideas")).toBeNull();

    fireEvent.click(screen.getByTestId("project-tab-insights"));
    expect(screen.getByTestId("marker-insights")).toBeTruthy();
    expect(screen.queryByTestId("marker-tasks")).toBeNull();

    fireEvent.click(screen.getByTestId("project-tab-rules"));
    expect(screen.getByTestId("marker-rules").textContent).toBe("project:p1");
    expect(screen.queryByTestId("marker-insights")).toBeNull();

    fireEvent.click(screen.getByTestId("project-tab-graph"));
    expect(screen.getByTestId("marker-graph").textContent).toBe("p1");
    expect(screen.queryByTestId("marker-rules")).toBeNull();

    fireEvent.click(screen.getByTestId("project-tab-overview"));
    expect(screen.getByTestId("project-overview")).toBeTruthy();
    expect(screen.queryByTestId("marker-graph")).toBeNull();
  });

  it('renders the "Graph" tab button and selecting it shows the GraphCanvas stub', () => {
    render(<ProjectPanel projectId="p1" />);
    const tabButton = screen.getByTestId("project-tab-graph");
    expect(tabButton.textContent).toBe(strings.project.tabs.graph);
    expect(screen.queryByTestId("marker-graph")).toBeNull();

    fireEvent.click(tabButton);

    expect(screen.getByTestId("marker-graph")).toBeTruthy();
  });

  it("an unresolvable workspace id renders the \"unavailable\" chip; Unlink calls orchdRemoveProjectWorkspace", async () => {
    useAppStore.setState({ projects: [makeProject({ workspaceIds: ["w1", "ghost-ws"] })] }, false);
    render(<ProjectPanel projectId="p1" />);

    expect(screen.getByTestId("project-workspace-unresolved-ghost-ws").textContent).toContain(
      strings.project.workspaceUnavailable,
    );

    fireEvent.click(screen.getByTestId("project-workspace-detach-ghost-ws"));

    await waitFor(() => {
      expect(orchdRemoveProjectWorkspaceMock).toHaveBeenCalledWith("p1", "ghost-ws");
    });
  });

  it("the add-workspace select lists only unlinked workspaces and links the chosen one", async () => {
    render(<ProjectPanel projectId="p1" />);

    const select = screen.getByTestId("project-add-workspace-select") as HTMLSelectElement;
    // w1 is already linked to p1 -> must not appear as an option; w2 is unlinked -> must appear.
    expect(Array.from(select.options).some((o) => o.value === "w1")).toBe(false);
    expect(Array.from(select.options).some((o) => o.value === "w2")).toBe(true);

    fireEvent.change(select, { target: { value: "w2" } });

    await waitFor(() => {
      expect(orchdAddProjectWorkspaceMock).toHaveBeenCalledWith("p1", "w2");
    });
  });

  it('"Copy JSON" writes the exported project to the clipboard and toasts', async () => {
    orchdExportProjectMock.mockResolvedValue('{"bundleFormat":1}');
    const writeText = vi.fn().mockResolvedValue(undefined);
    Object.assign(navigator, { clipboard: { writeText } });

    render(<ProjectPanel projectId="p1" />);
    fireEvent.click(screen.getByTestId("project-export-copy"));

    await waitFor(() => {
      expect(orchdExportProjectMock).toHaveBeenCalledWith("p1");
      expect(writeText).toHaveBeenCalledWith('{"bundleFormat":1}');
      expect(useAppStore.getState().toast).toBe(strings.project.jsonCopied);
    });
  });

  it('"Save to file…" picks a folder then calls orchdExportToFile with the picked dir', async () => {
    pickFolderMock.mockResolvedValue("/Users/me/exports");
    render(<ProjectPanel projectId="p1" />);

    fireEvent.click(screen.getByTestId("project-export-file"));

    await waitFor(() => {
      expect(orchdExportToFileMock).toHaveBeenCalledWith("p1", "/Users/me/exports");
    });
  });

  it('"Save to file…" is a no-op when the folder picker is cancelled', async () => {
    pickFolderMock.mockResolvedValue(null);
    render(<ProjectPanel projectId="p1" />);

    fireEvent.click(screen.getByTestId("project-export-file"));

    await waitFor(() => expect(pickFolderMock).toHaveBeenCalledTimes(1));
    expect(orchdExportToFileMock).not.toHaveBeenCalled();
  });

  it("import: browses a folder, lists only .json files, and imports the chosen one", async () => {
    pickFolderMock.mockResolvedValue("/Users/me/imports");
    listDirMock.mockResolvedValue([
      { name: "a.json", relPath: "a.json", isDir: false, size: 10, isIgnored: false },
      { name: "notes.txt", relPath: "notes.txt", isDir: false, size: 5, isIgnored: false },
      { name: "sub", relPath: "sub", isDir: true, size: 0, isIgnored: false },
    ]);

    render(<ProjectPanel projectId="p1" />);
    fireEvent.click(screen.getByTestId("project-import-browse"));

    await waitFor(() => {
      expect(listDirMock).toHaveBeenCalledWith("/Users/me/imports", "", false);
    });
    expect(screen.getByTestId("project-import-file-a.json")).toBeTruthy();
    expect(screen.queryByTestId("project-import-file-notes.txt")).toBeNull();
    expect(screen.queryByTestId("project-import-file-sub")).toBeNull();

    fireEvent.click(screen.getByTestId("project-import-file-a.json"));

    await waitFor(() => {
      expect(orchdImportFromFileMock).toHaveBeenCalledWith("/Users/me/imports/a.json");
    });
  });

  it("a failed mutation shows the mapped error via a toast", async () => {
    orchdRemoveProjectWorkspaceMock.mockRejectedValue({ kind: "daemon", code: "Invariant", message: "the project must keep at least one workspace" });
    useAppStore.setState({ projects: [makeProject({ workspaceIds: ["w1"] })] }, false);
    render(<ProjectPanel projectId="p1" />);

    fireEvent.click(screen.getByTestId("project-workspace-detach-w1"));

    await waitFor(() => {
      expect(describeOrchdErrorMock).toHaveBeenCalled();
      expect(useAppStore.getState().toast).toBe("orchestrator: error");
    });
  });

  // ---- archive / un-archive (O-3, spec D7) ----

  it('the "Archive project" button asks for confirmation; cancelling is a no-op', () => {
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(false);
    render(<ProjectPanel projectId="p1" />);
    const archiveBtn = screen.getByTestId("project-archive");
    expect(archiveBtn.textContent).toBe(strings.project.archive);

    fireEvent.click(archiveBtn);
    expect(confirmSpy).toHaveBeenCalledWith(strings.project.archiveConfirm);
    expect(orchdArchiveProjectMock).not.toHaveBeenCalled();
    confirmSpy.mockRestore();
  });

  it('an accepted confirm fires orchdArchiveProject with the project id', async () => {
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(true);
    render(<ProjectPanel projectId="p1" />);

    fireEvent.click(screen.getByTestId("project-archive"));
    await waitFor(() => expect(orchdArchiveProjectMock).toHaveBeenCalledWith("p1"));
    confirmSpy.mockRestore();
  });

  it('the "Archive project" button is disabled while orchdDown', () => {
    useAppStore.setState({ orchdDown: true }, false);
    render(<ProjectPanel projectId="p1" />);
    expect((screen.getByTestId("project-archive") as HTMLButtonElement).disabled).toBe(true);
  });

  it("a rapid double-click on Archive fires the round-trip only once (submit guard, spec D6)", async () => {
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(true);
    // A never-settling promise keeps `submitting`/the ref lock engaged across both clicks.
    orchdArchiveProjectMock.mockReset().mockReturnValue(new Promise<never>(() => {}));
    render(<ProjectPanel projectId="p1" />);

    const archiveBtn = screen.getByTestId("project-archive");
    fireEvent.click(archiveBtn);
    fireEvent.click(archiveBtn);

    await waitFor(() => expect(orchdArchiveProjectMock).toHaveBeenCalledTimes(1));
    confirmSpy.mockRestore();
  });

  it("an archived project renders a read-only banner and NO archive button", () => {
    useAppStore.setState({ projects: [makeProject({ status: "archived" })] }, false);
    render(<ProjectPanel projectId="p1" />);
    expect(screen.getByTestId("project-archived-banner").textContent).toContain(
      strings.project.archivedBanner,
    );
    expect(screen.queryByTestId("project-archive")).toBeNull();
  });

  it('the archived banner\'s "Un-archive" button fires orchdUnarchiveProject', async () => {
    useAppStore.setState({ projects: [makeProject({ status: "archived" })] }, false);
    render(<ProjectPanel projectId="p1" />);
    const unarchiveBtn = screen.getByTestId("project-unarchive");
    expect(unarchiveBtn.textContent).toBe(strings.project.unarchive);

    fireEvent.click(unarchiveBtn);
    await waitFor(() => expect(orchdUnarchiveProjectMock).toHaveBeenCalledWith("p1"));
  });

  it('the "Un-archive" button is disabled while orchdDown', () => {
    useAppStore.setState({ projects: [makeProject({ status: "archived" })], orchdDown: true }, false);
    render(<ProjectPanel projectId="p1" />);
    expect((screen.getByTestId("project-unarchive") as HTMLButtonElement).disabled).toBe(true);
  });
});

describe("workspace-management + import gating (AUD-2026-07-19-03/-04)", () => {
  it("while orchdDown: Unlink, add-workspace select and import browse are disabled", () => {
    useAppStore.setState({ orchdDown: true }, false);
    render(<ProjectPanel projectId="p1" />);
    expect((screen.getByTestId("project-workspace-detach-w1") as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByTestId("project-add-workspace-select") as HTMLSelectElement).disabled).toBe(true);
    expect((screen.getByTestId("project-import-browse") as HTMLButtonElement).disabled).toBe(true);
  });

  it("archived project: read-only banner is honored — Unlink/attach/import disabled, export stays live", () => {
    useAppStore.setState({ projects: [makeProject({ status: "archived" })] }, false);
    render(<ProjectPanel projectId="p1" />);
    expect((screen.getByTestId("project-workspace-detach-w1") as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByTestId("project-add-workspace-select") as HTMLSelectElement).disabled).toBe(true);
    expect((screen.getByTestId("project-import-browse") as HTMLButtonElement).disabled).toBe(true);
    // Export is a read — an archived project must still be exportable.
    expect((screen.getByTestId("project-export-copy") as HTMLButtonElement).disabled).toBe(false);
    expect((screen.getByTestId("project-export-file") as HTMLButtonElement).disabled).toBe(false);
  });

  it("active project + orchd up: the same controls are enabled", () => {
    render(<ProjectPanel projectId="p1" />);
    expect((screen.getByTestId("project-workspace-detach-w1") as HTMLButtonElement).disabled).toBe(false);
    expect((screen.getByTestId("project-add-workspace-select") as HTMLSelectElement).disabled).toBe(false);
    expect((screen.getByTestId("project-import-browse") as HTMLButtonElement).disabled).toBe(false);
  });
});
