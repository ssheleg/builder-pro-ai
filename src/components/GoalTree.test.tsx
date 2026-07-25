// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, fireEvent, waitFor, within, act } from "@testing-library/react";

const orchdCreateGoalMock = vi.fn();
const orchdUpdateGoalMock = vi.fn();
const orchdMoveGoalMock = vi.fn();
const orchdDeleteGoalMock = vi.fn();
const orchdListGoalsMock = vi.fn();
const describeOrchdErrorMock = vi.fn((..._a: unknown[]) => "orchestrator: error");

vi.mock("../ipc/orchd", () => ({
  orchdCreateGoal: (...a: unknown[]) => orchdCreateGoalMock(...a),
  orchdUpdateGoal: (...a: unknown[]) => orchdUpdateGoalMock(...a),
  orchdMoveGoal: (...a: unknown[]) => orchdMoveGoalMock(...a),
  orchdDeleteGoal: (...a: unknown[]) => orchdDeleteGoalMock(...a),
  orchdListGoals: (...a: unknown[]) => orchdListGoalsMock(...a),
  describeOrchdError: (...a: unknown[]) => describeOrchdErrorMock(...a),
}));

import { GoalTree } from "./GoalTree";
import { useAppStore } from "../store/store";
import { strings } from "../strings";
import type { Goal } from "../ipc/orchd-types";

const projectId = "proj-1";

function makeGoal(over: Partial<Goal> & { id: string }): Goal {
  return {
    projectId,
    parentId: null,
    kind: "additional",
    title: "goal",
    body: "",
    ord: 0,
    status: "active",
    metricRefs: [],
    createdAt: 1,
    updatedAt: 1,
    ...over,
  };
}

const root: Goal = makeGoal({
  id: "root",
  kind: "strategic",
  parentId: null,
  title: "Strategic goal",
  ord: 0,
});

afterEach(cleanup);

beforeEach(() => {
  orchdCreateGoalMock.mockReset().mockResolvedValue(makeGoal({ id: "new-goal" }));
  orchdUpdateGoalMock.mockReset().mockResolvedValue(root);
  orchdMoveGoalMock.mockReset().mockResolvedValue(root);
  orchdDeleteGoalMock.mockReset().mockResolvedValue(undefined);
  orchdListGoalsMock.mockReset().mockResolvedValue([]);
  describeOrchdErrorMock.mockReset().mockReturnValue("orchestrator: error");
  useAppStore.setState(
    { goalsByProject: {}, goalsFetched: {}, toast: null, toastQueue: [], orchdDown: false },
    false,
  );
});

describe("GoalTree", () => {
  it("renders a 3-level tree with correct indent and parent-before-children order, regardless of store array order", () => {
    const child = makeGoal({ id: "child", parentId: "root", title: "Subgoal", ord: 0 });
    const grandchild = makeGoal({ id: "grandchild", parentId: "child", title: "Sub-subgoal", ord: 0 });
    // Deliberately scrambled relative to tree order — the component must sort defensively.
    useAppStore.setState({ goalsByProject: { [projectId]: [grandchild, root, child] } }, false);

    render(<GoalTree projectId={projectId} />);

    const rows = Array.from(document.querySelectorAll('[data-testid^="goal-row-"]')).map((el) =>
      el.getAttribute("data-testid"),
    );
    expect(rows).toEqual(["goal-row-root", "goal-row-child", "goal-row-grandchild"]);

    expect(screen.getByTestId("goal-row-root").style.paddingLeft).toBe("8px");
    expect(screen.getByTestId("goal-row-child").style.paddingLeft).toBe("24px");
    expect(screen.getByTestId("goal-row-grandchild").style.paddingLeft).toBe("40px");
  });

  it("the strategic root row has no delete and no move controls; a non-root row has both", () => {
    const child = makeGoal({ id: "child", parentId: "root", title: "Subgoal", ord: 0 });
    useAppStore.setState({ goalsByProject: { [projectId]: [root, child] } }, false);

    render(<GoalTree projectId={projectId} />);

    expect(screen.queryByTestId("goal-delete-root")).toBeNull();
    expect(screen.queryByTestId("goal-move-up-root")).toBeNull();
    expect(screen.queryByTestId("goal-move-down-root")).toBeNull();

    expect(screen.getByTestId("goal-delete-child")).toBeTruthy();
    expect(screen.getByTestId("goal-move-up-child")).toBeTruthy();
    expect(screen.getByTestId("goal-move-down-child")).toBeTruthy();
  });

  it('"+ subgoal" calls orchdCreateGoal with this row\'s id as parentId and kind "additional", then refreshes the tree', async () => {
    const child = makeGoal({ id: "child", parentId: "root", title: "Subgoal", ord: 0 });
    useAppStore.setState({ goalsByProject: { [projectId]: [root, child] } }, false);
    orchdListGoalsMock.mockResolvedValue([root, child]);

    render(<GoalTree projectId={projectId} />);

    const childRow = screen.getByTestId("goal-row-child");
    fireEvent.click(within(childRow).getByRole("button", { name: strings.goals.addSubgoal }));

    await waitFor(() =>
      expect(orchdCreateGoalMock).toHaveBeenCalledWith(
        projectId,
        "child",
        "additional",
        strings.goals.newSubgoal,
        "",
      ),
    );
    await waitFor(() => expect(orchdListGoalsMock).toHaveBeenCalledWith(projectId));
  });

  it("two rapid '+ subgoal' clicks add ONE subgoal (double-submit guard, spec D6 / P-19)", async () => {
    const child = makeGoal({ id: "child", parentId: "root", title: "Subgoal", ord: 0 });
    useAppStore.setState({ goalsByProject: { [projectId]: [root, child] } }, false);
    orchdListGoalsMock.mockResolvedValue([root, child]);
    let resolveCreate!: (v: unknown) => void;
    orchdCreateGoalMock.mockReset().mockImplementation(
      () => new Promise((res) => (resolveCreate = res)),
    );

    render(<GoalTree projectId={projectId} />);
    const childRow = screen.getByTestId("goal-row-child");
    const addButton = within(childRow).getByRole("button", { name: strings.goals.addSubgoal });
    fireEvent.click(addButton);
    fireEvent.click(addButton);

    expect(orchdCreateGoalMock).toHaveBeenCalledTimes(1);
    await act(async () => {
      resolveCreate(makeGoal({ id: "new-goal" }));
    });
  });

  it("delete asks for confirmation and only calls orchdDeleteGoal after it is accepted", async () => {
    const child = makeGoal({ id: "child", parentId: "root", title: "Subgoal", ord: 0 });
    useAppStore.setState({ goalsByProject: { [projectId]: [root, child] } }, false);

    render(<GoalTree projectId={projectId} />);
    const deleteButton = screen.getByTestId("goal-delete-child");

    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(false);
    fireEvent.click(deleteButton);
    expect(confirmSpy).toHaveBeenCalledWith(strings.goals.deleteConfirm);
    expect(orchdDeleteGoalMock).not.toHaveBeenCalled();

    confirmSpy.mockReturnValue(true);
    orchdListGoalsMock.mockResolvedValue([root]);
    fireEvent.click(deleteButton);
    await waitFor(() => expect(orchdDeleteGoalMock).toHaveBeenCalledWith("child"));
    await waitFor(() => expect(orchdListGoalsMock).toHaveBeenCalledWith(projectId));

    confirmSpy.mockRestore();
  });

  it("a sibling ▲ TRUE-SWAPS ords with the previous sibling via TWO orchdMoveGoal calls, then refreshes once", async () => {
    const child1 = makeGoal({ id: "child1", parentId: "root", title: "First", ord: 0 });
    const child2 = makeGoal({ id: "child2", parentId: "root", title: "Second", ord: 1 });
    useAppStore.setState({ goalsByProject: { [projectId]: [root, child1, child2] } }, false);
    orchdListGoalsMock.mockResolvedValue([root, child1, child2]);

    render(<GoalTree projectId={projectId} />);
    const callsBefore = orchdListGoalsMock.mock.calls.length; // mount may fetch once
    fireEvent.click(screen.getByTestId("goal-move-up-child2"));

    // TWO moves: the moved row takes the neighbor's old ord, the neighbor takes the moved row's
    // old ord — after both, the two ords are swapped and each stays unique (no shared-ord bug).
    await waitFor(() => expect(orchdMoveGoalMock).toHaveBeenCalledTimes(2));
    expect(orchdMoveGoalMock).toHaveBeenNthCalledWith(1, "child2", "root", 0); // moved: neighbor's ord
    expect(orchdMoveGoalMock).toHaveBeenNthCalledWith(2, "child1", "root", 1); // neighbor: moved's ord

    // exactly ONE refresh, issued AFTER both swap calls resolved (not one per call)
    await waitFor(() => expect(orchdListGoalsMock.mock.calls.length).toBe(callsBefore + 1));
    expect(orchdListGoalsMock).toHaveBeenLastCalledWith(projectId);
  });

  it("a sibling ▼ TRUE-SWAPS with the NEXT sibling (first row takes next's ord, next takes first's)", async () => {
    const child1 = makeGoal({ id: "child1", parentId: "root", title: "First", ord: 0 });
    const child2 = makeGoal({ id: "child2", parentId: "root", title: "Second", ord: 1 });
    useAppStore.setState({ goalsByProject: { [projectId]: [root, child1, child2] } }, false);
    orchdListGoalsMock.mockResolvedValue([root, child1, child2]);

    render(<GoalTree projectId={projectId} />);
    fireEvent.click(screen.getByTestId("goal-move-down-child1"));

    await waitFor(() => expect(orchdMoveGoalMock).toHaveBeenCalledTimes(2));
    expect(orchdMoveGoalMock).toHaveBeenNthCalledWith(1, "child1", "root", 1); // moved: next's ord
    expect(orchdMoveGoalMock).toHaveBeenNthCalledWith(2, "child2", "root", 0); // next: moved's ord
  });

  it("edge: ▲ on the FIRST sibling and ▼ on the LAST sibling are disabled and never call orchdMoveGoal", async () => {
    const child1 = makeGoal({ id: "child1", parentId: "root", title: "First", ord: 0 });
    const child2 = makeGoal({ id: "child2", parentId: "root", title: "Second", ord: 1 });
    useAppStore.setState({ goalsByProject: { [projectId]: [root, child1, child2] } }, false);

    render(<GoalTree projectId={projectId} />);

    // first sibling can't go up; last sibling can't go down
    expect((screen.getByTestId("goal-move-up-child1") as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByTestId("goal-move-down-child2") as HTMLButtonElement).disabled).toBe(true);

    // clicking the disabled controls is a no-op — zero move calls
    fireEvent.click(screen.getByTestId("goal-move-up-child1"));
    fireEvent.click(screen.getByTestId("goal-move-down-child2"));
    expect(orchdMoveGoalMock).not.toHaveBeenCalled();
  });

  it("an Invariant error from a mutating call surfaces via showToast", async () => {
    const child = makeGoal({ id: "child", parentId: "root", title: "Subgoal", ord: 0 });
    useAppStore.setState({ goalsByProject: { [projectId]: [root, child] } }, false);
    const commandError = { kind: "daemon", code: "Invariant", message: "cannot rename" };
    orchdUpdateGoalMock.mockRejectedValueOnce(commandError);

    render(<GoalTree projectId={projectId} />);
    const input = screen.getByTestId("goal-title-input-child") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "New name" } });
    fireEvent.blur(input);

    await waitFor(() => expect(describeOrchdErrorMock).toHaveBeenCalledWith(commandError));
    await waitFor(() => expect(useAppStore.getState().toast).toBe("orchestrator: error"));
  });

  it("mounts empty: fetches the tree via refreshGoals when nothing is cached for this project yet", async () => {
    orchdListGoalsMock.mockResolvedValue([root]);
    // goalsByProject has no entry at all for this project (never fetched).
    render(<GoalTree projectId={projectId} />);
    await waitFor(() => expect(orchdListGoalsMock).toHaveBeenCalledWith(projectId));
  });

  it("UX-1: while the first fetch is in flight the loading placeholder shows — the false empty state never does", async () => {
    // orchdListGoals stays pending: the FIRST fetch for this project has not settled, so
    // `goalsFetched[projectId]` is unset and the cache is still empty — exactly the window in
    // which the pre-fix component flashed strings.goals.empty at a user who HAS goals.
    let resolveList!: (goals: Goal[]) => void;
    orchdListGoalsMock.mockReset().mockImplementation(
      () => new Promise<Goal[]>((res) => (resolveList = res)),
    );

    render(<GoalTree projectId={projectId} />);

    expect(orchdListGoalsMock).toHaveBeenCalledTimes(1); // the fetch IS in flight meanwhile
    expect(screen.queryByText(strings.goals.empty)).toBeNull(); // no false empty flash
    expect(screen.getByTestId("goal-tree-loading").textContent).toBe(strings.goals.loading);

    // Once the fetch settles with a real goal, the goal shows and neither loading nor empty
    // copy remains.
    await act(async () => {
      resolveList([root]);
    });
    expect(screen.queryByTestId("goal-tree-loading")).toBeNull();
    expect(screen.queryByText(strings.goals.empty)).toBeNull();
    expect(screen.queryByDisplayValue("Strategic goal")).toBeTruthy();
  });

  it("UX-1: after a SETTLED first fetch with an empty result, the honest empty state shows (no loading row)", async () => {
    orchdListGoalsMock.mockResolvedValue([]);
    render(<GoalTree projectId={projectId} />);

    await waitFor(() => expect(screen.queryByTestId("goal-tree-empty")).toBeTruthy());
    expect(screen.queryByTestId("goal-tree-loading")).toBeNull();
  });

  it("while orchdDown: every mutating control is disabled and clicking one never calls the orchd wrapper (spec §10)", () => {
    const child1 = makeGoal({ id: "child1", parentId: "root", title: "First", ord: 0 });
    const child2 = makeGoal({ id: "child2", parentId: "root", title: "Second", ord: 1 });
    useAppStore.setState(
      { goalsByProject: { [projectId]: [root, child1, child2] }, orchdDown: true },
      false,
    );

    render(<GoalTree projectId={projectId} />);

    const titleInput = screen.getByTestId("goal-title-input-child1") as HTMLInputElement;
    const statusSelect = screen.getByTestId("goal-status-child1") as HTMLSelectElement;
    const deleteButton = screen.getByTestId("goal-delete-child1") as HTMLButtonElement;
    const moveUpButton = screen.getByTestId("goal-move-up-child2") as HTMLButtonElement; // otherwise movable
    const moveDownButton = screen.getByTestId("goal-move-down-child1") as HTMLButtonElement; // otherwise movable
    const addSubgoalButton = within(screen.getByTestId("goal-row-child1")).getByRole("button", {
      name: strings.goals.addSubgoal,
    }) as HTMLButtonElement;

    expect(titleInput.disabled).toBe(true);
    expect(statusSelect.disabled).toBe(true);
    expect(deleteButton.disabled).toBe(true);
    expect(moveUpButton.disabled).toBe(true);
    expect(moveDownButton.disabled).toBe(true);
    expect(addSubgoalButton.disabled).toBe(true);

    vi.spyOn(window, "confirm").mockReturnValue(true);
    fireEvent.click(deleteButton);
    fireEvent.click(addSubgoalButton);
    fireEvent.click(moveUpButton);
    fireEvent.click(moveDownButton);

    expect(orchdDeleteGoalMock).not.toHaveBeenCalled();
    expect(orchdCreateGoalMock).not.toHaveBeenCalled();
    expect(orchdMoveGoalMock).not.toHaveBeenCalled();
  });

  // ── metric_refs chip editor (O-4, spec D7) ───────────────────────────────────────────────────

  it("renders a goal's existing metricRefs as chips", () => {
    const child = makeGoal({
      id: "child",
      parentId: "root",
      title: "Subgoal",
      ord: 0,
      metricRefs: ["dau", "retention_d7"],
    });
    useAppStore.setState({ goalsByProject: { [projectId]: [root, child] } }, false);

    render(<GoalTree projectId={projectId} />);

    const childRow = screen.getByTestId("goal-row-child");
    expect(within(childRow).getByTestId("goal-metric-chip-child-dau").textContent).toContain("dau");
    expect(
      within(childRow).getByTestId("goal-metric-chip-child-retention_d7").textContent,
    ).toContain("retention_d7");
    // a goal with no refs renders no chips
    const rootRow = screen.getByTestId("goal-row-root");
    expect(rootRow.querySelectorAll('[data-testid^="goal-metric-chip-"]').length).toBe(0);
  });

  it("adding a metric via the input + Enter calls orchdUpdateGoal with the metricRefs array including the new entry", async () => {
    const child = makeGoal({
      id: "child",
      parentId: "root",
      title: "Subgoal",
      ord: 0,
      metricRefs: ["dau"],
    });
    useAppStore.setState({ goalsByProject: { [projectId]: [root, child] } }, false);

    render(<GoalTree projectId={projectId} />);
    const input = screen.getByTestId("goal-metric-input-child") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "retention_d7" } });
    fireEvent.keyDown(input, { key: "Enter" });

    await waitFor(() =>
      expect(orchdUpdateGoalMock).toHaveBeenCalledWith("child", null, null, null, [
        "dau",
        "retention_d7",
      ]),
    );
    // the input is cleared after a submit
    expect(input.value).toBe("");
  });

  it("a blank / whitespace-only metric entry is ignored and never calls orchdUpdateGoal", () => {
    const child = makeGoal({ id: "child", parentId: "root", title: "Subgoal", ord: 0 });
    useAppStore.setState({ goalsByProject: { [projectId]: [root, child] } }, false);

    render(<GoalTree projectId={projectId} />);
    const input = screen.getByTestId("goal-metric-input-child") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "   " } });
    fireEvent.keyDown(input, { key: "Enter" });

    expect(orchdUpdateGoalMock).not.toHaveBeenCalled();
  });

  it("adding a metric that already exists is a no-op (dedupe) — never calls orchdUpdateGoal", () => {
    const child = makeGoal({
      id: "child",
      parentId: "root",
      title: "Subgoal",
      ord: 0,
      metricRefs: ["dau"],
    });
    useAppStore.setState({ goalsByProject: { [projectId]: [root, child] } }, false);

    render(<GoalTree projectId={projectId} />);
    const input = screen.getByTestId("goal-metric-input-child") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "dau" } });
    fireEvent.keyDown(input, { key: "Enter" });

    expect(orchdUpdateGoalMock).not.toHaveBeenCalled();
    expect(input.value).toBe(""); // still cleared
  });

  it("removing a metric via its chip × calls orchdUpdateGoal with the array WITHOUT that entry", async () => {
    const child = makeGoal({
      id: "child",
      parentId: "root",
      title: "Subgoal",
      ord: 0,
      metricRefs: ["dau", "retention_d7"],
    });
    useAppStore.setState({ goalsByProject: { [projectId]: [root, child] } }, false);

    render(<GoalTree projectId={projectId} />);
    fireEvent.click(screen.getByTestId("goal-metric-remove-child-dau"));

    await waitFor(() =>
      expect(orchdUpdateGoalMock).toHaveBeenCalledWith("child", null, null, null, ["retention_d7"]),
    );
  });

  it("two rapid Enters on the metric input add the metric only ONCE (double-submit guard)", async () => {
    const child = makeGoal({ id: "child", parentId: "root", title: "Subgoal", ord: 0 });
    useAppStore.setState({ goalsByProject: { [projectId]: [root, child] } }, false);
    let resolveUpdate!: (v: unknown) => void;
    orchdUpdateGoalMock
      .mockReset()
      .mockImplementation(() => new Promise((res) => (resolveUpdate = res)));

    render(<GoalTree projectId={projectId} />);
    const input = screen.getByTestId("goal-metric-input-child") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "dau" } });
    fireEvent.keyDown(input, { key: "Enter" });
    fireEvent.keyDown(input, { key: "Enter" });

    expect(orchdUpdateGoalMock).toHaveBeenCalledTimes(1);
    await act(async () => {
      resolveUpdate(makeGoal({ id: "child", metricRefs: ["dau"] }));
    });
  });

  it("while orchdDown: the metric input and every chip × are disabled and never call orchdUpdateGoal", () => {
    const child = makeGoal({
      id: "child",
      parentId: "root",
      title: "Subgoal",
      ord: 0,
      metricRefs: ["dau"],
    });
    useAppStore.setState(
      { goalsByProject: { [projectId]: [root, child] }, orchdDown: true },
      false,
    );

    render(<GoalTree projectId={projectId} />);
    const input = screen.getByTestId("goal-metric-input-child") as HTMLInputElement;
    const removeButton = screen.getByTestId("goal-metric-remove-child-dau") as HTMLButtonElement;

    expect(input.disabled).toBe(true);
    expect(removeButton.disabled).toBe(true);

    fireEvent.keyDown(input, { key: "Enter" });
    fireEvent.click(removeButton);
    expect(orchdUpdateGoalMock).not.toHaveBeenCalled();
  });
});
