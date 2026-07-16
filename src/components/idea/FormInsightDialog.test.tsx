// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, fireEvent, waitFor, act } from "@testing-library/react";

const orchdCreateInsightMock = vi.fn();
const orchdSetInsightFitVerdictMock = vi.fn();
const orchdSetInsightStatusMock = vi.fn();
const orchdCreateTaskMock = vi.fn();
const orchdSetIdeaLifecycleMock = vi.fn();
const orchdGraphNeighborhoodMock = vi.fn();
const orchdListGoalsMock = vi.fn();
const orchdGraphListProjectMock = vi.fn();
const describeOrchdErrorMock = vi.fn((..._a: unknown[]) => "orchestrator: error");

vi.mock("../../ipc/orchd", () => ({
  orchdCreateInsight: (...a: unknown[]) => orchdCreateInsightMock(...a),
  orchdSetInsightFitVerdict: (...a: unknown[]) => orchdSetInsightFitVerdictMock(...a),
  orchdSetInsightStatus: (...a: unknown[]) => orchdSetInsightStatusMock(...a),
  orchdCreateTask: (...a: unknown[]) => orchdCreateTaskMock(...a),
  orchdSetIdeaLifecycle: (...a: unknown[]) => orchdSetIdeaLifecycleMock(...a),
  orchdGraphNeighborhood: (...a: unknown[]) => orchdGraphNeighborhoodMock(...a),
  orchdListGoals: (...a: unknown[]) => orchdListGoalsMock(...a),
  orchdGraphListProject: (...a: unknown[]) => orchdGraphListProjectMock(...a),
  describeOrchdError: (...a: unknown[]) => describeOrchdErrorMock(...a),
}));

import { FormInsightDialog } from "./FormInsightDialog";
import { useAppStore } from "../../store/store";
import { strings } from "../../strings";
import type { Goal, GraphNode, Insight, McpArtifact } from "../../ipc/orchd-types";

const ideaWithProject = {
  id: "idea-1",
  projectId: "p1",
  title: "Validate demand",
  body: "",
  lifecycle: "researching" as const,
  createdAt: 1,
  updatedAt: 1,
};

const ideaOrphan = { ...ideaWithProject, id: "idea-orphan", projectId: null };

function makeArtifact(over: Partial<McpArtifact> = {}): McpArtifact {
  return {
    id: "art-1",
    invocationId: "inv-1",
    serverId: "s1",
    accountId: null,
    toolName: "search",
    projectId: null,
    contentJson: '{"raw":true}',
    contentText: "market is large",
    isUntrusted: true,
    createdAt: 1,
    ...over,
  };
}

function makeGoal(over: Partial<Goal> = {}): Goal {
  return {
    id: "g1",
    projectId: "p1",
    parentId: null,
    kind: "strategic",
    title: "Grow by 20%",
    body: "",
    ord: 0,
    status: "active",
    metricRefs: ["mrr"],
    createdAt: 1,
    updatedAt: 1,
    ...over,
  };
}

function makeNode(over: Partial<GraphNode> = {}): GraphNode {
  return {
    id: "n1",
    projectId: "p1",
    kind: "entityRef",
    entityType: "idea",
    entityId: ideaWithProject.id,
    label: "Validate demand",
    body: "",
    posX: 0,
    posY: 0,
    createdAt: 1,
    updatedAt: 1,
    isOrphan: false,
    ...over,
  };
}

function makeInsight(over: Partial<Insight> = {}): Insight {
  return {
    id: "in1",
    projectId: "p1",
    source: `research-run:r1`,
    title: "Validate demand",
    body: "market is large",
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
  orchdCreateInsightMock.mockReset().mockResolvedValue(makeInsight());
  orchdSetInsightFitVerdictMock.mockReset().mockResolvedValue(makeInsight());
  orchdSetInsightStatusMock.mockReset().mockResolvedValue(makeInsight({ status: "accepted" }));
  orchdCreateTaskMock.mockReset().mockResolvedValue({});
  orchdSetIdeaLifecycleMock.mockReset().mockResolvedValue({});
  orchdGraphNeighborhoodMock
    .mockReset()
    .mockResolvedValue({ rootId: "n1", nodes: [], edges: [] });
  orchdListGoalsMock.mockReset().mockResolvedValue([]);
  orchdGraphListProjectMock
    .mockReset()
    .mockResolvedValue({ nodes: [], edges: [], externalNodes: [] });
  describeOrchdErrorMock.mockReset().mockReturnValue("orchestrator: error");
  useAppStore.setState(
    {
      goalsByProject: {},
      graphByProject: {},
      orchdDown: false,
      toast: null,
    },
    false,
  );
});

describe("FormInsightDialog", () => {
  it("prefills title from the idea and body from the artifact's contentText", () => {
    render(
      <FormInsightDialog
        idea={ideaWithProject}
        runId="r1"
        artifact={makeArtifact()}
        onClose={() => {}}
      />,
    );
    expect((screen.getByTestId("form-insight-title") as HTMLInputElement).value).toBe(
      ideaWithProject.title,
    );
    expect((screen.getByTestId("form-insight-body") as HTMLTextAreaElement).value).toBe(
      "market is large",
    );
  });

  it("body is empty when there is no artifact (Q8 degraded path)", () => {
    render(
      <FormInsightDialog idea={ideaWithProject} runId="r1" artifact={null} onClose={() => {}} />,
    );
    expect((screen.getByTestId("form-insight-body") as HTMLTextAreaElement).value).toBe("");
  });

  it("fit-context: fetches and renders the project's goals with metric_refs", async () => {
    orchdListGoalsMock.mockResolvedValue([makeGoal()]);
    render(
      <FormInsightDialog idea={ideaWithProject} runId="r1" artifact={null} onClose={() => {}} />,
    );
    await waitFor(() => expect(orchdListGoalsMock).toHaveBeenCalledWith("p1"));
    await waitFor(() => {
      const row = screen.getByTestId("form-insight-goal-g1");
      expect(row.textContent).toContain("Grow by 20%");
      expect(row.textContent).toContain("mrr");
    });
  });

  it("fit-context: finds the idea's graph node and fetches its GraphNeighborhood", async () => {
    orchdGraphListProjectMock.mockResolvedValue({
      nodes: [makeNode()],
      edges: [],
      externalNodes: [],
    });
    orchdGraphNeighborhoodMock.mockResolvedValue({
      rootId: "n1",
      nodes: [makeNode({ id: "n2", label: "Related concept" })],
      edges: [],
    });
    render(
      <FormInsightDialog idea={ideaWithProject} runId="r1" artifact={null} onClose={() => {}} />,
    );
    await waitFor(() => expect(orchdGraphListProjectMock).toHaveBeenCalledWith("p1"));
    await waitFor(() => expect(orchdGraphNeighborhoodMock).toHaveBeenCalledWith("n1", 1));
    await waitFor(() => {
      expect(screen.getByTestId("form-insight-neighborhood-node-n2").textContent).toContain(
        "Related concept",
      );
    });
  });

  it("an orphan idea (no project) shows a degraded fit-context note and fetches nothing", () => {
    render(<FormInsightDialog idea={ideaOrphan} runId="r1" artifact={null} onClose={() => {}} />);
    expect(screen.getByTestId("form-insight-no-project")).toBeTruthy();
    expect(orchdListGoalsMock).not.toHaveBeenCalled();
    expect(orchdGraphListProjectMock).not.toHaveBeenCalled();
  });

  it('"Create" is blocked with an empty title', () => {
    render(
      <FormInsightDialog idea={ideaWithProject} runId="r1" artifact={null} onClose={() => {}} />,
    );
    fireEvent.change(screen.getByTestId("form-insight-title"), { target: { value: "" } });
    fireEvent.click(screen.getByTestId("form-insight-create"));
    expect(orchdCreateInsightMock).not.toHaveBeenCalled();
  });

  it('"Create" fires orchdCreateInsight then orchdSetInsightFitVerdict, in order, with source research-run:<id>', async () => {
    render(
      <FormInsightDialog
        idea={ideaWithProject}
        runId="r1"
        artifact={makeArtifact()}
        onClose={() => {}}
      />,
    );

    fireEvent.change(screen.getByTestId("form-insight-verdict"), { target: { value: "fit" } });
    fireEvent.change(screen.getByTestId("form-insight-reasoning"), {
      target: { value: "good market" },
    });
    fireEvent.click(screen.getByTestId("form-insight-create"));

    await waitFor(() =>
      expect(orchdCreateInsightMock).toHaveBeenCalledWith(
        "p1",
        "research-run:r1",
        ideaWithProject.title,
        "market is large",
      ),
    );
    await waitFor(() =>
      expect(orchdSetInsightFitVerdictMock).toHaveBeenCalledWith("in1", "fit", "good market"),
    );
    expect(
      orchdCreateInsightMock.mock.invocationCallOrder[0]!,
    ).toBeLessThan(orchdSetInsightFitVerdictMock.mock.invocationCallOrder[0]!);
  });

  it("two rapid Create clicks form the insight ONCE (double-submit guard, spec D6 / G-08)", async () => {
    let resolveCreate!: (v: unknown) => void;
    orchdCreateInsightMock.mockReset().mockImplementation(
      () => new Promise((res) => (resolveCreate = res)),
    );
    render(
      <FormInsightDialog
        idea={ideaWithProject}
        runId="r1"
        artifact={makeArtifact()}
        onClose={() => {}}
      />,
    );
    const create = screen.getByTestId("form-insight-create");
    fireEvent.click(create);
    fireEvent.click(create);

    expect(orchdCreateInsightMock).toHaveBeenCalledTimes(1);
    await act(async () => {
      resolveCreate(makeInsight());
    });
  });

  it('once created, "Accept" fires orchdSetInsightStatus(id, accepted, null)', async () => {
    render(
      <FormInsightDialog idea={ideaWithProject} runId="r1" artifact={null} onClose={() => {}} />,
    );
    fireEvent.click(screen.getByTestId("form-insight-create"));
    await waitFor(() => expect(screen.getByTestId("form-insight-accept")).toBeTruthy());

    fireEvent.click(screen.getByTestId("form-insight-accept"));
    await waitFor(() =>
      expect(orchdSetInsightStatusMock).toHaveBeenCalledWith("in1", "accepted", null),
    );
  });

  it('once accepted, "To backlog" fires orchdCreateTask then orchdSetIdeaLifecycle(specced), then closes', async () => {
    const onClose = vi.fn();
    render(
      <FormInsightDialog idea={ideaWithProject} runId="r1" artifact={null} onClose={onClose} />,
    );
    fireEvent.click(screen.getByTestId("form-insight-create"));
    await waitFor(() => expect(screen.getByTestId("form-insight-accept")).toBeTruthy());
    fireEvent.click(screen.getByTestId("form-insight-accept"));
    await waitFor(() => expect(screen.getByTestId("form-insight-backlog")).toBeTruthy());

    fireEvent.click(screen.getByTestId("form-insight-backlog"));

    await waitFor(() =>
      expect(orchdCreateTaskMock).toHaveBeenCalledWith(
        "p1",
        null,
        "Validate demand",
        "market is large",
        null,
        "insight",
        "in1",
        [],
      ),
    );
    await waitFor(() =>
      expect(orchdSetIdeaLifecycleMock).toHaveBeenCalledWith(ideaWithProject.id, "specced"),
    );
    expect(
      orchdCreateTaskMock.mock.invocationCallOrder[0]!,
    ).toBeLessThan(orchdSetIdeaLifecycleMock.mock.invocationCallOrder[0]!);
    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });

  it("backlog: setIdeaLifecycle fails after createTask — names the created task, retry re-runs ONLY the lifecycle flip (no duplicate task) — BL-95 / G-08", async () => {
    const onClose = vi.fn();
    orchdCreateTaskMock.mockReset().mockResolvedValue({ id: "task-9" });
    orchdSetIdeaLifecycleMock
      .mockReset()
      .mockRejectedValueOnce({ kind: "daemon", code: "Io", message: "disk" })
      .mockResolvedValueOnce({ ...ideaWithProject, lifecycle: "specced" });

    render(
      <FormInsightDialog idea={ideaWithProject} runId="r1" artifact={null} onClose={onClose} />,
    );
    fireEvent.click(screen.getByTestId("form-insight-create"));
    await waitFor(() => expect(screen.getByTestId("form-insight-accept")).toBeTruthy());
    fireEvent.click(screen.getByTestId("form-insight-accept"));
    await waitFor(() => expect(screen.getByTestId("form-insight-backlog")).toBeTruthy());

    // First "To backlog": createTask succeeds, lifecycle flip fails.
    fireEvent.click(screen.getByTestId("form-insight-backlog"));
    await waitFor(() =>
      expect(screen.getByTestId("form-insight-error").textContent).toBe(
        strings.insights.form.backlogResume("orchestrator: error"),
      ),
    );
    expect(orchdCreateTaskMock).toHaveBeenCalledTimes(1);
    expect(onClose).not.toHaveBeenCalled();

    // Retry: resumes at the lifecycle flip only — no duplicate task.
    await act(async () => {
      fireEvent.click(screen.getByTestId("form-insight-backlog"));
      await Promise.resolve();
    });
    await waitFor(() => expect(onClose).toHaveBeenCalled());

    expect(orchdCreateTaskMock).toHaveBeenCalledTimes(1); // NOT re-created
    expect(orchdSetIdeaLifecycleMock).toHaveBeenCalledTimes(2); // only the failed step re-ran
  });

  it('"To backlog" is disabled for an orphan idea (no project to file the task under)', async () => {
    render(<FormInsightDialog idea={ideaOrphan} runId="r1" artifact={null} onClose={() => {}} />);
    fireEvent.click(screen.getByTestId("form-insight-create"));
    await waitFor(() => expect(screen.getByTestId("form-insight-accept")).toBeTruthy());
    fireEvent.click(screen.getByTestId("form-insight-accept"));

    await waitFor(() => {
      const backlogButton = screen.getByTestId("form-insight-backlog") as HTMLButtonElement;
      expect(backlogButton.disabled).toBe(true);
    });
    fireEvent.click(screen.getByTestId("form-insight-backlog"));
    expect(orchdCreateTaskMock).not.toHaveBeenCalled();
  });

  it("a failed create shows the mapped error inline and keeps the dialog open", async () => {
    orchdCreateInsightMock.mockRejectedValue({ kind: "daemon", code: "Validation", message: "bad" });
    render(
      <FormInsightDialog idea={ideaWithProject} runId="r1" artifact={null} onClose={() => {}} />,
    );
    fireEvent.click(screen.getByTestId("form-insight-create"));

    await waitFor(() => expect(screen.getByTestId("form-insight-error")).toBeTruthy());
    expect(screen.getByTestId("form-insight-dialog")).toBeTruthy();
  });

  it("cancel closes without creating anything", () => {
    const onClose = vi.fn();
    render(
      <FormInsightDialog idea={ideaWithProject} runId="r1" artifact={null} onClose={onClose} />,
    );
    fireEvent.click(screen.getByTestId("form-insight-cancel"));
    expect(onClose).toHaveBeenCalled();
    expect(orchdCreateInsightMock).not.toHaveBeenCalled();
  });

  it("while orchdDown: create/accept/backlog are all disabled and never call their wrappers", async () => {
    useAppStore.setState({ orchdDown: true }, false);
    render(
      <FormInsightDialog idea={ideaWithProject} runId="r1" artifact={null} onClose={() => {}} />,
    );

    const createButton = screen.getByTestId("form-insight-create") as HTMLButtonElement;
    expect(createButton.disabled).toBe(true);
    fireEvent.click(createButton);
    expect(orchdCreateInsightMock).not.toHaveBeenCalled();
  });

  // Regression guard (T6 review, Finding 1): "Accept"/"To backlog" are gated behind
  // `insight !== null` (and, for backlog, `status === "accepted"`), so the create-only orchdDown
  // test above can NEVER reach them — a regression dropping `orchdDown` from `acceptBlocked`/
  // `backlogBlocked` would go undetected. This test first drives past those gates (create, then
  // accept) so each button renders in the state where `orchdDown` is its ONLY remaining blocking
  // term, then flips orchdDown and asserts the disabled + click-not-called invariant for BOTH.
  it('while orchdDown after create/accept: "Accept" (status new) and "To backlog" (status accepted) are disabled and click NEITHER wrapper', async () => {
    render(
      <FormInsightDialog idea={ideaWithProject} runId="r1" artifact={null} onClose={() => {}} />,
    );

    // Get past the `insight !== null` gate: create the insight (resolves to status "new").
    fireEvent.click(screen.getByTestId("form-insight-create"));
    await waitFor(() => expect(screen.getByTestId("form-insight-accept")).toBeTruthy());

    // Phase A — "Accept" with status "new": `acceptBlocked = orchdDown || insight===null ||
    // insight.status!=="new"`. insight is non-null and status is "new", so orchdDown is the ONLY
    // term that can block it here — a dropped-orchdDown regression flips this assertion.
    act(() => {
      useAppStore.setState({ orchdDown: true }, false);
    });
    const acceptButton = screen.getByTestId("form-insight-accept") as HTMLButtonElement;
    expect(acceptButton.disabled).toBe(true);
    fireEvent.click(acceptButton);
    expect(orchdSetInsightStatusMock).not.toHaveBeenCalled();

    // Restore and actually accept (resolves to status "accepted") so "To backlog" renders.
    act(() => {
      useAppStore.setState({ orchdDown: false }, false);
    });
    fireEvent.click(screen.getByTestId("form-insight-accept"));
    await waitFor(() => expect(screen.getByTestId("form-insight-backlog")).toBeTruthy());
    orchdSetInsightStatusMock.mockClear(); // drop the successful accept call before Phase B

    // Phase B — "To backlog" with status "accepted" and a concrete projectId: `backlogBlocked =
    // orchdDown || insight===null || insight.status!=="accepted" || idea.projectId===null`. All
    // three non-orchdDown terms are satisfied (non-null, accepted, project set), so orchdDown is
    // again the ONLY term that can block it — the regression guard for the backlog expression.
    act(() => {
      useAppStore.setState({ orchdDown: true }, false);
    });
    const backlogButton = screen.getByTestId("form-insight-backlog") as HTMLButtonElement;
    expect(backlogButton.disabled).toBe(true);
    fireEvent.click(backlogButton);
    expect(orchdCreateTaskMock).not.toHaveBeenCalled();
    expect(orchdSetIdeaLifecycleMock).not.toHaveBeenCalled();
  });
});
