// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, fireEvent, waitFor, within } from "@testing-library/react";

const orchdCreateTaskMock = vi.fn();
const orchdUpdateTaskMock = vi.fn();
const orchdSetTaskStatusMock = vi.fn();
const orchdSetTaskRankMock = vi.fn();
const orchdDeleteTaskMock = vi.fn();
const orchdListTasksMock = vi.fn();
const describeOrchdErrorMock = vi.fn((..._a: unknown[]) => "orchestrator: error");

vi.mock("../ipc/orchd", () => ({
  orchdCreateTask: (...a: unknown[]) => orchdCreateTaskMock(...a),
  orchdUpdateTask: (...a: unknown[]) => orchdUpdateTaskMock(...a),
  orchdSetTaskStatus: (...a: unknown[]) => orchdSetTaskStatusMock(...a),
  orchdSetTaskRank: (...a: unknown[]) => orchdSetTaskRankMock(...a),
  orchdDeleteTask: (...a: unknown[]) => orchdDeleteTaskMock(...a),
  orchdListTasks: (...a: unknown[]) => orchdListTasksMock(...a),
  describeOrchdError: (...a: unknown[]) => describeOrchdErrorMock(...a),
}));

import { TasksList } from "./TasksList";
import { useAppStore } from "../store/store";
import type { DomainTask } from "../ipc/orchd-types";

const projectId = "proj-1";

function makeTask(over: Partial<DomainTask> & { id: string }): DomainTask {
  return {
    projectId,
    parentId: null,
    title: "task",
    body: "",
    status: "backlog",
    source: "idea",
    sourceId: null,
    tags: [],
    rank: 0,
    rankAgent: null,
    rankAgentReasoning: "",
    createdAt: 1,
    updatedAt: 1,
    ...over,
  };
}

afterEach(cleanup);

beforeEach(() => {
  orchdCreateTaskMock.mockReset().mockResolvedValue(makeTask({ id: "new-task" }));
  orchdUpdateTaskMock.mockReset().mockResolvedValue(makeTask({ id: "updated" }));
  orchdSetTaskStatusMock.mockReset().mockResolvedValue(makeTask({ id: "status-changed" }));
  orchdSetTaskRankMock.mockReset().mockResolvedValue(makeTask({ id: "rank-changed" }));
  orchdDeleteTaskMock.mockReset().mockResolvedValue(undefined);
  orchdListTasksMock.mockReset().mockResolvedValue([]);
  describeOrchdErrorMock.mockReset().mockReturnValue("orchestrator: error");
  useAppStore.setState({ tasksByProject: {}, toast: null, orchdDown: false }, false);
});

describe("TasksList", () => {
  it("renders the six status groups in spec §4.2 order, rank-ordered ascending within each group", () => {
    const t1 = makeTask({ id: "t1", status: "todo", rank: 20, title: "todo-second" });
    const t2 = makeTask({ id: "t2", status: "backlog", rank: 5, title: "backlog-only" });
    const t3 = makeTask({ id: "t3", status: "todo", rank: 10, title: "todo-first" });
    // Deliberately scrambled relative to both group order and rank order.
    useAppStore.setState({ tasksByProject: { [projectId]: [t1, t2, t3] } }, false);

    render(<TasksList projectId={projectId} />);

    const groupEls = Array.from(
      document.querySelectorAll('[data-testid^="task-status-group-"]'),
    ).map((el) => el.getAttribute("data-testid"));
    expect(groupEls).toEqual([
      "task-status-group-backlog",
      "task-status-group-todo",
      "task-status-group-waiting",
      "task-status-group-progress",
      "task-status-group-testing",
      "task-status-group-done",
    ]);

    const todoGroup = screen.getByTestId("task-status-group-todo");
    const rowsInTodo = Array.from(todoGroup.querySelectorAll('[data-testid^="task-row-"]')).map(
      (el) => el.getAttribute("data-testid"),
    );
    // t3 (rank 10) before t1 (rank 20)
    expect(rowsInTodo).toEqual(["task-row-t3", "task-row-t1"]);
  });

  it("▲ on a middle row calls orchdSetTaskRank with the exact midpoint of its two new neighbors", async () => {
    const a = makeTask({ id: "a", status: "backlog", rank: 0 });
    const b = makeTask({ id: "b", status: "backlog", rank: 10 });
    const c = makeTask({ id: "c", status: "backlog", rank: 20 });
    const d = makeTask({ id: "d", status: "backlog", rank: 30 });
    useAppStore.setState({ tasksByProject: { [projectId]: [a, b, c, d] } }, false);

    render(<TasksList projectId={projectId} />);
    // Click ▲ on c (rank 20): new neighbors become a (0) and b (10) -> midpoint 5.
    fireEvent.click(screen.getByTestId("task-move-up-c"));

    await waitFor(() => expect(orchdSetTaskRankMock).toHaveBeenCalledWith("c", 5));
  });

  it("▲ on the row adjacent to the top uses firstRank - 1024 (no prevPrev to average with)", async () => {
    const a = makeTask({ id: "a", status: "backlog", rank: 0 });
    const b = makeTask({ id: "b", status: "backlog", rank: 10 });
    useAppStore.setState({ tasksByProject: { [projectId]: [a, b] } }, false);

    render(<TasksList projectId={projectId} />);
    // Click ▲ on b: prev is a (the first row), no prevPrev -> firstRank(0) - 1024.
    fireEvent.click(screen.getByTestId("task-move-up-b"));

    await waitFor(() => expect(orchdSetTaskRankMock).toHaveBeenCalledWith("b", 0 - 1024));
  });

  it("▼ on the row adjacent to the bottom uses lastRank + 1024 (no nextNext to average with)", async () => {
    const a = makeTask({ id: "a", status: "backlog", rank: 0 });
    const b = makeTask({ id: "b", status: "backlog", rank: 10 });
    useAppStore.setState({ tasksByProject: { [projectId]: [a, b] } }, false);

    render(<TasksList projectId={projectId} />);
    // Click ▼ on a: next is b (the last row), no nextNext -> lastRank(10) + 1024.
    fireEvent.click(screen.getByTestId("task-move-down-a"));

    await waitFor(() => expect(orchdSetTaskRankMock).toHaveBeenCalledWith("a", 10 + 1024));
  });

  it("edge: ▲ on the first row and ▼ on the last row are disabled and never call orchdSetTaskRank", () => {
    const a = makeTask({ id: "a", status: "backlog", rank: 0 });
    const b = makeTask({ id: "b", status: "backlog", rank: 10 });
    useAppStore.setState({ tasksByProject: { [projectId]: [a, b] } }, false);

    render(<TasksList projectId={projectId} />);

    expect((screen.getByTestId("task-move-up-a") as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByTestId("task-move-down-b") as HTMLButtonElement).disabled).toBe(true);

    fireEvent.click(screen.getByTestId("task-move-up-a"));
    fireEvent.click(screen.getByTestId("task-move-down-b"));
    expect(orchdSetTaskRankMock).not.toHaveBeenCalled();
  });

  it("a subtask row is indented (greater paddingLeft) relative to its parent's row", () => {
    const parent = makeTask({ id: "parent", status: "backlog", rank: 0, parentId: null });
    const child = makeTask({ id: "child", status: "backlog", rank: 10, parentId: "parent" });
    useAppStore.setState({ tasksByProject: { [projectId]: [parent, child] } }, false);

    render(<TasksList projectId={projectId} />);

    const parentPad = parseInt(screen.getByTestId("task-row-parent").style.paddingLeft, 10);
    const childPad = parseInt(screen.getByTestId("task-row-child").style.paddingLeft, 10);
    expect(childPad).toBeGreaterThan(parentPad);
  });

  it('delete on a task with children shows the "will delete N subtasks" warning and only calls orchdDeleteTask after confirm', async () => {
    const parent = makeTask({ id: "parent", status: "backlog", rank: 0 });
    const child1 = makeTask({ id: "child1", status: "backlog", rank: 10, parentId: "parent" });
    const child2 = makeTask({ id: "child2", status: "todo", rank: 0, parentId: "parent" });
    useAppStore.setState(
      { tasksByProject: { [projectId]: [parent, child1, child2] } },
      false,
    );
    orchdListTasksMock.mockResolvedValue([parent]);

    render(<TasksList projectId={projectId} />);
    const deleteButton = screen.getByTestId("task-delete-parent");

    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(false);
    fireEvent.click(deleteButton);
    expect(confirmSpy).toHaveBeenCalledWith(
      expect.stringContaining("will delete 2 subtasks"),
    );
    expect(orchdDeleteTaskMock).not.toHaveBeenCalled();

    confirmSpy.mockReturnValue(true);
    fireEvent.click(deleteButton);
    await waitFor(() => expect(orchdDeleteTaskMock).toHaveBeenCalledWith("parent"));
    await waitFor(() => expect(orchdListTasksMock).toHaveBeenCalledWith(projectId));

    confirmSpy.mockRestore();
  });

  it("delete on a task with no children confirms without naming a subtask count", () => {
    const solo = makeTask({ id: "solo", status: "backlog", rank: 0 });
    useAppStore.setState({ tasksByProject: { [projectId]: [solo] } }, false);

    render(<TasksList projectId={projectId} />);
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(false);
    fireEvent.click(screen.getByTestId("task-delete-solo"));

    expect(confirmSpy).toHaveBeenCalledWith(expect.not.stringContaining("subtasks"));
    confirmSpy.mockRestore();
  });

  it("the create dialog passes source, parent, and comma-split tags correctly to orchdCreateTask", async () => {
    const existing = makeTask({ id: "existing", status: "backlog", rank: 0, title: "existing task" });
    useAppStore.setState({ tasksByProject: { [projectId]: [existing] } }, false);
    orchdListTasksMock.mockResolvedValue([existing]);

    render(<TasksList projectId={projectId} />);

    fireEvent.change(screen.getByTestId("task-create-title"), {
      target: { value: "new task" },
    });
    fireEvent.change(screen.getByTestId("task-create-body"), {
      target: { value: "description" },
    });
    fireEvent.change(screen.getByTestId("task-create-source"), {
      target: { value: "bug" },
    });
    fireEvent.change(screen.getByTestId("task-create-parent"), {
      target: { value: "existing" },
    });
    fireEvent.change(screen.getByTestId("task-create-tags"), {
      target: { value: "one, two ,three" },
    });

    fireEvent.click(screen.getByTestId("task-create-submit"));

    await waitFor(() =>
      expect(orchdCreateTaskMock).toHaveBeenCalledWith(
        projectId,
        "existing",
        "new task",
        "description",
        null,
        "bug",
        null,
        ["one", "two", "three"],
      ),
    );
    await waitFor(() => expect(orchdListTasksMock).toHaveBeenCalledWith(projectId));
  });

  it("the create submit button is disabled while title is blank", () => {
    useAppStore.setState({ tasksByProject: { [projectId]: [] } }, false);
    render(<TasksList projectId={projectId} />);
    expect((screen.getByTestId("task-create-submit") as HTMLButtonElement).disabled).toBe(true);
  });

  it("a rejecting status-change mutation surfaces via showToast", async () => {
    const a = makeTask({ id: "a", status: "backlog", rank: 0 });
    useAppStore.setState({ tasksByProject: { [projectId]: [a] } }, false);
    const commandError = { kind: "daemon", code: "Invariant", message: "not allowed" };
    orchdSetTaskStatusMock.mockRejectedValueOnce(commandError);

    render(<TasksList projectId={projectId} />);
    const select = within(screen.getByTestId("task-row-a")).getByTestId(
      "task-status-select-a",
    ) as HTMLSelectElement;
    fireEvent.change(select, { target: { value: "todo" } });

    await waitFor(() => expect(describeOrchdErrorMock).toHaveBeenCalledWith(commandError));
    await waitFor(() => expect(useAppStore.getState().toast).toBe("orchestrator: error"));
  });

  it("mounts empty: fetches the task list via refreshTasks when nothing is cached for this project yet", async () => {
    orchdListTasksMock.mockResolvedValue([]);
    render(<TasksList projectId={projectId} />);
    await waitFor(() => expect(orchdListTasksMock).toHaveBeenCalledWith(projectId));
  });

  it("while orchdDown: every mutating control is disabled and clicking one never calls the orchd wrapper (spec §10)", () => {
    const a = makeTask({ id: "a", status: "backlog", rank: 0 });
    const b = makeTask({ id: "b", status: "backlog", rank: 10 });
    useAppStore.setState(
      { tasksByProject: { [projectId]: [a, b] }, orchdDown: true },
      false,
    );

    render(<TasksList projectId={projectId} />);

    const statusSelect = within(screen.getByTestId("task-row-a")).getByTestId(
      "task-status-select-a",
    ) as HTMLSelectElement;
    const moveDownButton = screen.getByTestId("task-move-down-a") as HTMLButtonElement; // otherwise movable
    const moveUpButton = screen.getByTestId("task-move-up-b") as HTMLButtonElement; // otherwise movable
    const deleteButton = screen.getByTestId("task-delete-a") as HTMLButtonElement;

    expect(statusSelect.disabled).toBe(true);
    expect(moveDownButton.disabled).toBe(true);
    expect(moveUpButton.disabled).toBe(true);
    expect(deleteButton.disabled).toBe(true);

    // create-submit's PRE-EXISTING disable condition is "title blank" — fill the title first so
    // the assertion below proves orchdDown ALONE still keeps it disabled, not the blank-title path.
    fireEvent.change(screen.getByTestId("task-create-title"), { target: { value: "x" } });
    const submitButton = screen.getByTestId("task-create-submit") as HTMLButtonElement;
    expect(submitButton.disabled).toBe(true);

    vi.spyOn(window, "confirm").mockReturnValue(true);
    fireEvent.click(deleteButton);
    fireEvent.click(moveDownButton);
    fireEvent.click(moveUpButton);
    fireEvent.click(submitButton);

    expect(orchdDeleteTaskMock).not.toHaveBeenCalled();
    expect(orchdSetTaskRankMock).not.toHaveBeenCalled();
    expect(orchdCreateTaskMock).not.toHaveBeenCalled();
  });
});
