// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, fireEvent, waitFor } from "@testing-library/react";

const orchdCreateIdeaMock = vi.fn();
const orchdUpdateIdeaMock = vi.fn();
const orchdSetIdeaProjectMock = vi.fn();
const orchdSetIdeaLifecycleMock = vi.fn();
const orchdDeleteIdeaMock = vi.fn();
const orchdListIdeasMock = vi.fn();
const describeOrchdErrorMock = vi.fn((..._a: unknown[]) => "orchestrator: error");
// S-IDEA §7, T6: IdeasList now transitively mounts ResearchRunDialog/ResearchPane/
// FormInsightDialog/SpawnProjectFromIdea (research button, badge/pane toggle, orphan spawn
// button) — every ipc wrapper THOSE components import must resolve here too, even though most of
// IdeasList's OWN tests below never exercise those code paths (mirrors `App.test.tsx`'s
// whole-descendant-tree mock discipline).
const researchStartRunMock = vi.fn();
const researchListRunsMock = vi.fn();
const mcpListToolsMock = vi.fn();
const mcpGetArtifactMock = vi.fn();
const trustListPoliciesMock = vi.fn();
const orchdCreateInsightMock = vi.fn();
const orchdSetInsightFitVerdictMock = vi.fn();
const orchdSetInsightStatusMock = vi.fn();
const orchdCreateTaskMock = vi.fn();
const orchdGraphNeighborhoodMock = vi.fn();
const orchdListGoalsMock = vi.fn();
const orchdGraphListProjectMock = vi.fn();
const orchdCreateProjectMock = vi.fn();

vi.mock("../ipc/orchd", () => ({
  orchdCreateIdea: (...a: unknown[]) => orchdCreateIdeaMock(...a),
  orchdUpdateIdea: (...a: unknown[]) => orchdUpdateIdeaMock(...a),
  orchdSetIdeaProject: (...a: unknown[]) => orchdSetIdeaProjectMock(...a),
  orchdSetIdeaLifecycle: (...a: unknown[]) => orchdSetIdeaLifecycleMock(...a),
  orchdDeleteIdea: (...a: unknown[]) => orchdDeleteIdeaMock(...a),
  orchdListIdeas: (...a: unknown[]) => orchdListIdeasMock(...a),
  researchStartRun: (...a: unknown[]) => researchStartRunMock(...a),
  researchListRuns: (...a: unknown[]) => researchListRunsMock(...a),
  mcpListTools: (...a: unknown[]) => mcpListToolsMock(...a),
  mcpGetArtifact: (...a: unknown[]) => mcpGetArtifactMock(...a),
  trustListPolicies: (...a: unknown[]) => trustListPoliciesMock(...a),
  orchdCreateInsight: (...a: unknown[]) => orchdCreateInsightMock(...a),
  orchdSetInsightFitVerdict: (...a: unknown[]) => orchdSetInsightFitVerdictMock(...a),
  orchdSetInsightStatus: (...a: unknown[]) => orchdSetInsightStatusMock(...a),
  orchdCreateTask: (...a: unknown[]) => orchdCreateTaskMock(...a),
  orchdGraphNeighborhood: (...a: unknown[]) => orchdGraphNeighborhoodMock(...a),
  orchdListGoals: (...a: unknown[]) => orchdListGoalsMock(...a),
  orchdGraphListProject: (...a: unknown[]) => orchdGraphListProjectMock(...a),
  orchdCreateProject: (...a: unknown[]) => orchdCreateProjectMock(...a),
  describeOrchdError: (...a: unknown[]) => describeOrchdErrorMock(...a),
}));

const pickFolderMock = vi.fn();
const createWorkspaceMock = vi.fn();
vi.mock("../ipc/commands", () => ({
  pickFolder: (...a: unknown[]) => pickFolderMock(...a),
  createWorkspace: (...a: unknown[]) => createWorkspaceMock(...a),
}));

import { IdeasList } from "./IdeasList";
import { useAppStore } from "../store/store";
import type { Idea, Project, ResearchRun } from "../ipc/orchd-types";

const projectId = "proj-1";

function makeIdea(over: Partial<Idea> & { id: string }): Idea {
  return {
    projectId,
    title: "idea",
    body: "idea body",
    lifecycle: "captured",
    createdAt: 1,
    updatedAt: 1,
    ...over,
  };
}

function makeResearchRun(over: Partial<ResearchRun> & { id: string; ideaId: string }): ResearchRun {
  return {
    serverId: "s1",
    toolName: "search",
    argsJson: "{}",
    status: "pending",
    invocationId: null,
    artifactId: null,
    errorKind: null,
    createdAt: 1,
    updatedAt: 1,
    ...over,
  };
}

function makeProject(over: Partial<Project> & { id: string; name: string }): Project {
  return {
    description: "",
    status: "active",
    workspaceIds: ["ws-1"],
    createdAt: 1,
    updatedAt: 1,
    ...over,
  };
}

afterEach(cleanup);

beforeEach(() => {
  orchdCreateIdeaMock.mockReset().mockResolvedValue(makeIdea({ id: "new-idea" }));
  orchdUpdateIdeaMock.mockReset().mockResolvedValue(makeIdea({ id: "i1" }));
  orchdSetIdeaProjectMock.mockReset().mockResolvedValue(makeIdea({ id: "i1" }));
  orchdSetIdeaLifecycleMock.mockReset().mockResolvedValue(makeIdea({ id: "i1" }));
  orchdDeleteIdeaMock.mockReset().mockResolvedValue(undefined);
  orchdListIdeasMock.mockReset().mockResolvedValue([]);
  describeOrchdErrorMock.mockReset().mockReturnValue("оркестратор: ошибка");
  researchStartRunMock.mockReset();
  researchListRunsMock.mockReset().mockResolvedValue([]);
  mcpListToolsMock.mockReset().mockResolvedValue([]);
  mcpGetArtifactMock.mockReset();
  trustListPoliciesMock.mockReset().mockResolvedValue([]);
  orchdCreateInsightMock.mockReset();
  orchdSetInsightFitVerdictMock.mockReset();
  orchdSetInsightStatusMock.mockReset();
  orchdCreateTaskMock.mockReset();
  orchdGraphNeighborhoodMock.mockReset().mockResolvedValue({ rootId: "n", nodes: [], edges: [] });
  orchdListGoalsMock.mockReset().mockResolvedValue([]);
  orchdGraphListProjectMock
    .mockReset()
    .mockResolvedValue({ nodes: [], edges: [], externalNodes: [] });
  orchdCreateProjectMock.mockReset();
  pickFolderMock.mockReset();
  createWorkspaceMock.mockReset();
  useAppStore.setState(
    { ideas: [], projects: [], toast: null, orchdDown: false, researchRunsByIdea: {}, mcpServers: [] },
    false,
  );
});

describe("IdeasList", () => {
  it("renders only ideas whose projectId matches the prop, newest-first", () => {
    const older = makeIdea({ id: "old", createdAt: 1 });
    const newer = makeIdea({ id: "new", createdAt: 5 });
    const other = makeIdea({ id: "other", projectId: "proj-2", createdAt: 9 });
    useAppStore.setState({ ideas: [older, newer, other] }, false);

    render(<IdeasList projectId={projectId} />);

    const rows = Array.from(document.querySelectorAll('[data-testid^="idea-row-"]')).map((el) =>
      el.getAttribute("data-testid"),
    );
    expect(rows).toEqual(["idea-row-new", "idea-row-old"]);
  });

  it("the lifecycle chip select fires orchdSetIdeaLifecycle with the chosen value", async () => {
    const idea = makeIdea({ id: "i1", lifecycle: "captured" });
    useAppStore.setState({ ideas: [idea] }, false);

    render(<IdeasList projectId={projectId} />);

    const select = screen.getByTestId("idea-lifecycle-i1") as HTMLSelectElement;
    fireEvent.change(select, { target: { value: "researching" } });

    await waitFor(() =>
      expect(orchdSetIdeaLifecycleMock).toHaveBeenCalledWith("i1", "researching"),
    );
  });

  it("inline title edit commits via orchdUpdateIdea with the trimmed title and null body", async () => {
    const idea = makeIdea({ id: "i1", title: "старое название" });
    useAppStore.setState({ ideas: [idea] }, false);

    render(<IdeasList projectId={projectId} />);

    const input = screen.getByTestId("idea-title-input-i1") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "новое название" } });
    fireEvent.blur(input);

    await waitFor(() =>
      expect(orchdUpdateIdeaMock).toHaveBeenCalledWith("i1", "новое название", null),
    );
  });

  it("delete asks for confirmation and only calls orchdDeleteIdea after it is accepted", async () => {
    const idea = makeIdea({ id: "i1" });
    useAppStore.setState({ ideas: [idea] }, false);

    render(<IdeasList projectId={projectId} />);
    const deleteButton = screen.getByTestId("idea-delete-i1");

    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(false);
    fireEvent.click(deleteButton);
    expect(orchdDeleteIdeaMock).not.toHaveBeenCalled();

    confirmSpy.mockReturnValue(true);
    fireEvent.click(deleteButton);
    await waitFor(() => expect(orchdDeleteIdeaMock).toHaveBeenCalledWith("i1"));
    await waitFor(() => expect(orchdListIdeasMock).toHaveBeenCalledWith(null));

    confirmSpy.mockRestore();
  });

  it('an orphan row (projectId null) shows a "привязать к проекту" affordance that fires orchdSetIdeaProject with the chosen project id', async () => {
    const orphan = makeIdea({ id: "orphan1", projectId: null });
    const project = makeProject({ id: "proj-9", name: "Проект 9" });
    useAppStore.setState({ ideas: [orphan], projects: [project] }, false);

    render(<IdeasList projectId={null} />);

    const select = screen.getByTestId("idea-attach-select-orphan1") as HTMLSelectElement;
    fireEvent.change(select, { target: { value: "proj-9" } });
    fireEvent.click(screen.getByTestId("idea-attach-button-orphan1"));

    await waitFor(() =>
      expect(orchdSetIdeaProjectMock).toHaveBeenCalledWith("orphan1", "proj-9"),
    );
    await waitFor(() => expect(orchdListIdeasMock).toHaveBeenCalledWith(null));
  });

  it("a non-orphan row never renders the attach-to-project affordance", () => {
    const idea = makeIdea({ id: "i1" });
    useAppStore.setState({ ideas: [idea] }, false);

    render(<IdeasList projectId={projectId} />);

    expect(screen.queryByTestId("idea-attach-select-i1")).toBeNull();
  });

  it("the create form calls orchdCreateIdea with the current projectId, title and body, then refreshes", async () => {
    render(<IdeasList projectId={projectId} />);

    fireEvent.change(screen.getByTestId("idea-create-title"), {
      target: { value: "Новая идея" },
    });
    fireEvent.change(screen.getByTestId("idea-create-body"), {
      target: { value: "Описание" },
    });
    fireEvent.click(screen.getByTestId("idea-create-submit"));

    await waitFor(() =>
      expect(orchdCreateIdeaMock).toHaveBeenCalledWith(projectId, "Новая идея", "Описание"),
    );
    await waitFor(() => expect(orchdListIdeasMock).toHaveBeenCalledWith(null));
  });

  it("the create form is a no-op with an empty title", () => {
    render(<IdeasList projectId={projectId} />);

    fireEvent.click(screen.getByTestId("idea-create-submit"));

    expect(orchdCreateIdeaMock).not.toHaveBeenCalled();
  });

  it("an error from a mutating call surfaces via showToast", async () => {
    const idea = makeIdea({ id: "i1" });
    useAppStore.setState({ ideas: [idea] }, false);
    const commandError = { kind: "daemon", code: "Invariant", message: "нельзя" };
    orchdSetIdeaLifecycleMock.mockRejectedValueOnce(commandError);

    render(<IdeasList projectId={projectId} />);
    fireEvent.change(screen.getByTestId("idea-lifecycle-i1"), {
      target: { value: "shipped" },
    });

    await waitFor(() => expect(describeOrchdErrorMock).toHaveBeenCalledWith(commandError));
    await waitFor(() => expect(useAppStore.getState().toast).toBe("оркестратор: ошибка"));
  });

  it("renders an empty state when there are no matching ideas", () => {
    render(<IdeasList projectId={projectId} />);
    expect(screen.getByTestId("ideas-list-empty")).toBeTruthy();
  });

  it("while orchdDown: every mutating control is disabled and clicking one never calls the orchd wrapper (spec §10)", () => {
    const idea = makeIdea({ id: "i1" });
    useAppStore.setState({ ideas: [idea], orchdDown: true }, false);

    render(<IdeasList projectId={projectId} />);

    const titleInput = screen.getByTestId("idea-title-input-i1") as HTMLInputElement;
    const bodyInput = screen.getByTestId("idea-body-input-i1") as HTMLTextAreaElement;
    const lifecycleSelect = screen.getByTestId("idea-lifecycle-i1") as HTMLSelectElement;
    const deleteButton = screen.getByTestId("idea-delete-i1") as HTMLButtonElement;

    expect(titleInput.disabled).toBe(true);
    expect(bodyInput.disabled).toBe(true);
    expect(lifecycleSelect.disabled).toBe(true);
    expect(deleteButton.disabled).toBe(true);

    // create-submit's PRE-EXISTING disable condition is "title blank" — fill the title first so
    // the assertion below proves orchdDown ALONE still keeps it disabled.
    fireEvent.change(screen.getByTestId("idea-create-title"), { target: { value: "x" } });
    const submitButton = screen.getByTestId("idea-create-submit") as HTMLButtonElement;
    expect(submitButton.disabled).toBe(true);

    vi.spyOn(window, "confirm").mockReturnValue(true);
    fireEvent.click(deleteButton);
    fireEvent.click(submitButton);

    expect(orchdDeleteIdeaMock).not.toHaveBeenCalled();
    expect(orchdCreateIdeaMock).not.toHaveBeenCalled();
  });

  it("while orchdDown: an orphan row's «привязать к проекту» button is disabled", () => {
    const idea = makeIdea({ id: "i1", projectId: null });
    useAppStore.setState(
      { ideas: [idea], projects: [{ id: "p1", name: "Proj", description: "", status: "active", workspaceIds: [], createdAt: 1, updatedAt: 1 }], orchdDown: true },
      false,
    );

    render(<IdeasList projectId={null} />);

    fireEvent.change(screen.getByTestId("idea-attach-select-i1"), { target: { value: "p1" } });
    const attachButton = screen.getByTestId("idea-attach-button-i1") as HTMLButtonElement;
    expect(attachButton.disabled).toBe(true);

    fireEvent.click(attachButton);
    expect(orchdSetIdeaProjectMock).not.toHaveBeenCalled();
  });

  // ── S-IDEA §7, T6: research flow ───────────────────────────────────────────────────────────

  it("eagerly fetches research runs (refreshResearchRuns -> researchListRuns) for every rendered idea, once", async () => {
    const idea1 = makeIdea({ id: "i1" });
    const idea2 = makeIdea({ id: "i2" });
    useAppStore.setState({ ideas: [idea1, idea2] }, false);

    render(<IdeasList projectId={projectId} />);

    await waitFor(() => expect(researchListRunsMock).toHaveBeenCalledWith("i1"));
    await waitFor(() => expect(researchListRunsMock).toHaveBeenCalledWith("i2"));
  });

  it("«Исследовать» opens ResearchRunDialog for that idea", () => {
    const idea = makeIdea({ id: "i1" });
    useAppStore.setState({ ideas: [idea] }, false);

    render(<IdeasList projectId={projectId} />);
    expect(screen.queryByTestId("research-run-dialog")).toBeNull();

    fireEvent.click(screen.getByTestId("idea-research-i1"));
    expect(screen.getByTestId("research-run-dialog")).toBeTruthy();
  });

  it("shows a research-run status badge reflecting the latest (newest-first) run, and none when there are no runs", () => {
    const idea = makeIdea({ id: "i1" });
    useAppStore.setState(
      {
        ideas: [idea],
        researchRunsByIdea: {
          i1: [
            makeResearchRun({ id: "r2", ideaId: "i1", status: "done" }),
            makeResearchRun({ id: "r1", ideaId: "i1", status: "pending" }),
          ],
        },
      },
      false,
    );

    render(<IdeasList projectId={projectId} />);
    expect(screen.getByTestId("idea-research-badge-i1").textContent).toMatch(/готово/i);
  });

  it("no research-run badge is rendered when the idea has no runs yet", () => {
    const idea = makeIdea({ id: "i1" });
    useAppStore.setState({ ideas: [idea], researchRunsByIdea: { i1: [] } }, false);

    render(<IdeasList projectId={projectId} />);
    expect(screen.queryByTestId("idea-research-badge-i1")).toBeNull();
  });

  it('the "исследования" toggle shows/hides the ResearchPane for that idea', () => {
    const idea = makeIdea({ id: "i1" });
    useAppStore.setState(
      { ideas: [idea], researchRunsByIdea: { i1: [makeResearchRun({ id: "r1", ideaId: "i1" })] } },
      false,
    );

    render(<IdeasList projectId={projectId} />);
    expect(screen.queryByTestId("research-pane")).toBeNull();

    fireEvent.click(screen.getByTestId("idea-research-toggle-i1"));
    expect(screen.getByTestId("research-pane")).toBeTruthy();

    fireEvent.click(screen.getByTestId("idea-research-toggle-i1"));
    expect(screen.queryByTestId("research-pane")).toBeNull();
  });

  it("an orphan row renders the SpawnProjectFromIdea button; a non-orphan row never does", () => {
    const orphan = makeIdea({ id: "orphan1", projectId: null });
    useAppStore.setState({ ideas: [orphan] }, false);
    render(<IdeasList projectId={null} />);
    expect(screen.getByTestId("spawn-project-orphan1")).toBeTruthy();
    cleanup();

    const owned = makeIdea({ id: "i1" });
    useAppStore.setState({ ideas: [owned] }, false);
    render(<IdeasList projectId={projectId} />);
    expect(screen.queryByTestId("spawn-project-i1")).toBeNull();
  });

  it("while orchdDown: «Исследовать» is disabled and clicking it never opens the dialog", () => {
    const idea = makeIdea({ id: "i1" });
    useAppStore.setState({ ideas: [idea], orchdDown: true }, false);

    render(<IdeasList projectId={projectId} />);
    const researchButton = screen.getByTestId("idea-research-i1") as HTMLButtonElement;
    expect(researchButton.disabled).toBe(true);

    fireEvent.click(researchButton);
    expect(screen.queryByTestId("research-run-dialog")).toBeNull();
  });

  it("while orchdDown: an orphan row's «Создать проект» button is disabled", () => {
    const orphan = makeIdea({ id: "orphan1", projectId: null });
    useAppStore.setState({ ideas: [orphan], orchdDown: true }, false);

    render(<IdeasList projectId={null} />);
    const spawnButton = screen.getByTestId("spawn-project-orphan1") as HTMLButtonElement;
    expect(spawnButton.disabled).toBe(true);

    fireEvent.click(spawnButton);
    expect(pickFolderMock).not.toHaveBeenCalled();
  });
});
