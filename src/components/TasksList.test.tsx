// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, fireEvent, waitFor, within, act } from "@testing-library/react";

const orchdCreateTaskMock = vi.fn();
const orchdUpdateTaskMock = vi.fn();
const orchdSetTaskStatusMock = vi.fn();
const orchdSetTaskRankMock = vi.fn();
const orchdSetTaskPriorityMock = vi.fn();
const orchdDeleteTaskMock = vi.fn();
const orchdListTasksMock = vi.fn();
const describeOrchdErrorMock = vi.fn((..._a: unknown[]) => "orchestrator: error");

vi.mock("../ipc/orchd", () => ({
  orchdCreateTask: (...a: unknown[]) => orchdCreateTaskMock(...a),
  orchdUpdateTask: (...a: unknown[]) => orchdUpdateTaskMock(...a),
  orchdSetTaskStatus: (...a: unknown[]) => orchdSetTaskStatusMock(...a),
  orchdSetTaskRank: (...a: unknown[]) => orchdSetTaskRankMock(...a),
  orchdSetTaskPriority: (...a: unknown[]) => orchdSetTaskPriorityMock(...a),
  orchdDeleteTask: (...a: unknown[]) => orchdDeleteTaskMock(...a),
  orchdListTasks: (...a: unknown[]) => orchdListTasksMock(...a),
  describeOrchdError: (...a: unknown[]) => describeOrchdErrorMock(...a),
}));

import { TasksList } from "./TasksList";
import { useAppStore } from "../store/store";
import { strings } from "../strings";
import type { DomainTask } from "../ipc/orchd-types";

const projectId = "proj-1";

function makeTask(over: Partial<DomainTask> & { id: string }): DomainTask {
  return {
    projectId,
    parentId: null,
    title: "task",
    body: "",
    status: "backlog",
    priority: "normal",
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
  orchdSetTaskPriorityMock.mockReset().mockResolvedValue(makeTask({ id: "priority-changed" }));
  orchdDeleteTaskMock.mockReset().mockResolvedValue(undefined);
  orchdListTasksMock.mockReset().mockResolvedValue([]);
  describeOrchdErrorMock.mockReset().mockReturnValue("orchestrator: error");
  useAppStore.setState({ tasksByProject: {}, toast: null, toastQueue: [], orchdDown: false }, false);
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

  it("renders a task's source/provenance as a chip on its row (TA-06)", () => {
    const t = makeTask({ id: "t1", status: "backlog", rank: 0, source: "insight" });
    useAppStore.setState({ tasksByProject: { [projectId]: [t] } }, false);

    render(<TasksList projectId={projectId} />);

    const chip = within(screen.getByTestId("task-row-t1")).getByTestId("task-source-t1");
    expect(chip.textContent).toContain(strings.tasks.source.insight);
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
        "normal", // priority untouched ⇒ the SCN-051 default
      ),
    );
    await waitFor(() => expect(orchdListTasksMock).toHaveBeenCalledWith(projectId));
  });

  it("two rapid '+ task' clicks create ONCE (double-submit guard, spec D6 / H-01)", async () => {
    useAppStore.setState({ tasksByProject: { [projectId]: [] } }, false);
    orchdListTasksMock.mockResolvedValue([]);
    let resolveCreate!: (v: unknown) => void;
    orchdCreateTaskMock.mockReset().mockImplementation(
      () => new Promise((res) => (resolveCreate = res)),
    );
    render(<TasksList projectId={projectId} />);
    fireEvent.change(screen.getByTestId("task-create-title"), { target: { value: "Dup task" } });

    const submit = screen.getByTestId("task-create-submit");
    fireEvent.click(submit);
    fireEvent.click(submit);

    expect(orchdCreateTaskMock).toHaveBeenCalledTimes(1);
    await act(async () => {
      resolveCreate(makeTask({ id: "t9", title: "Dup task", status: "backlog", rank: 0 }));
    });
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
    const prioritySelect = within(screen.getByTestId("task-row-a")).getByTestId(
      "task-priority-select-a",
    ) as HTMLSelectElement;
    const moveDownButton = screen.getByTestId("task-move-down-a") as HTMLButtonElement; // otherwise movable
    const moveUpButton = screen.getByTestId("task-move-up-b") as HTMLButtonElement; // otherwise movable
    const deleteButton = screen.getByTestId("task-delete-a") as HTMLButtonElement;

    expect(statusSelect.disabled).toBe(true);
    expect(prioritySelect.disabled).toBe(true); // SCN-051: gated like every other mutation
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

  // ---- SCN-051 (ST-037): task priority — urgent / normal ----

  it("the create form sends the selected priority to orchdCreateTask (SCN-051)", async () => {
    useAppStore.setState({ tasksByProject: { [projectId]: [] } }, false);
    orchdListTasksMock.mockResolvedValue([]);

    render(<TasksList projectId={projectId} />);

    fireEvent.change(screen.getByTestId("task-create-title"), {
      target: { value: "urgent task" },
    });
    fireEvent.change(screen.getByTestId("task-create-priority"), {
      target: { value: "urgent" },
    });
    fireEvent.click(screen.getByTestId("task-create-submit"));

    await waitFor(() =>
      expect(orchdCreateTaskMock).toHaveBeenCalledWith(
        projectId,
        null,
        "urgent task",
        "",
        null,
        "idea",
        null,
        [],
        "urgent",
      ),
    );
    // The form resets to the SCN-051 default after a successful create.
    await waitFor(() =>
      expect(
        (screen.getByTestId("task-create-priority") as HTMLSelectElement).value,
      ).toBe("normal"),
    );
  });

  it("urgent tasks sort ahead of normal within their status group despite a lower rank (SCN-051)", () => {
    const normalLow = makeTask({ id: "nl", status: "todo", rank: 0, title: "normal-low" });
    const urgentHigh = makeTask({
      id: "uh",
      status: "todo",
      rank: 100,
      priority: "urgent",
      title: "urgent-high",
    });
    const urgentLow = makeTask({
      id: "ul",
      status: "todo",
      rank: 50,
      priority: "urgent",
      title: "urgent-low",
    });
    useAppStore.setState(
      { tasksByProject: { [projectId]: [normalLow, urgentHigh, urgentLow] } },
      false,
    );

    render(<TasksList projectId={projectId} />);

    const todoGroup = screen.getByTestId("task-status-group-todo");
    const rows = Array.from(todoGroup.querySelectorAll('[data-testid^="task-row-"]')).map(
      (el) => el.getAttribute("data-testid"),
    );
    // urgent first (rank-ordered within the urgent segment), then normal — NOT flat rank order.
    expect(rows).toEqual(["task-row-ul", "task-row-uh", "task-row-nl"]);
  });

  it("an urgent row renders the danger marker and the urgent chip; a normal row renders neither (SCN-051)", () => {
    const urgent = makeTask({ id: "u", status: "backlog", rank: 0, priority: "urgent" });
    const normal = makeTask({ id: "n", status: "backlog", rank: 10 });
    useAppStore.setState({ tasksByProject: { [projectId]: [urgent, normal] } }, false);

    render(<TasksList projectId={projectId} />);

    const urgentRow = screen.getByTestId("task-row-u");
    expect(within(urgentRow).getByTestId("task-urgent-marker-u")).toBeTruthy();
    expect(within(urgentRow).getByTestId("task-urgent-chip-u").textContent).toContain(
      strings.tasks.priority.urgent,
    );
    const normalRow = screen.getByTestId("task-row-n");
    expect(within(normalRow).queryByTestId("task-urgent-marker-n")).toBeNull();
    expect(within(normalRow).queryByTestId("task-urgent-chip-n")).toBeNull();
  });

  it("changing a row's priority select calls orchdSetTaskPriority (SCN-051)", async () => {
    const a = makeTask({ id: "a", status: "backlog", rank: 0 });
    useAppStore.setState({ tasksByProject: { [projectId]: [a] } }, false);

    render(<TasksList projectId={projectId} />);
    fireEvent.change(screen.getByTestId("task-priority-select-a"), {
      target: { value: "urgent" },
    });

    await waitFor(() => expect(orchdSetTaskPriorityMock).toHaveBeenCalledWith("a", "urgent"));
  });

  it("a rejecting priority save reverts the select to the stored value and shows a toast (SCN-051)", async () => {
    const a = makeTask({ id: "a", status: "backlog", rank: 0, priority: "normal" });
    useAppStore.setState({ tasksByProject: { [projectId]: [a] } }, false);
    const commandError = { kind: "daemon", code: "Invariant", message: "not allowed" };
    orchdSetTaskPriorityMock.mockRejectedValueOnce(commandError);

    render(<TasksList projectId={projectId} />);
    const select = screen.getByTestId("task-priority-select-a") as HTMLSelectElement;
    fireEvent.change(select, { target: { value: "urgent" } });

    await waitFor(() => expect(describeOrchdErrorMock).toHaveBeenCalledWith(commandError));
    await waitFor(() => expect(useAppStore.getState().toast).toBe("orchestrator: error"));
    // Revert: the select is controlled by the STORE value, which the rejected save never
    // changed — so it must still read the stored "normal", not the attempted "urgent".
    expect(select.value).toBe("normal");
  });

  it("▲ is disabled at a priority-segment boundary and rank math stays within the segment (SCN-051)", async () => {
    // Group order renders urgent-first: [uA(50), nB(0), nC(10)]. nB is the FIRST row of the
    // normal segment — its ▲ must be disabled (rank alone can never lift a normal task above
    // an urgent one). nC's ▲ computes against its same-priority neighbors only.
    const uA = makeTask({ id: "uA", status: "backlog", rank: 50, priority: "urgent" });
    const nB = makeTask({ id: "nB", status: "backlog", rank: 0 });
    const nC = makeTask({ id: "nC", status: "backlog", rank: 10 });
    useAppStore.setState({ tasksByProject: { [projectId]: [uA, nB, nC] } }, false);

    render(<TasksList projectId={projectId} />);

    expect((screen.getByTestId("task-move-up-nB") as HTMLButtonElement).disabled).toBe(true);
    // uA is alone in the urgent segment: both arrows disabled.
    expect((screen.getByTestId("task-move-up-uA") as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByTestId("task-move-down-uA") as HTMLButtonElement).disabled).toBe(true);
    // nB cannot move down past the segment? It CAN — nC is its same-priority neighbor below.
    expect((screen.getByTestId("task-move-down-nB") as HTMLButtonElement).disabled).toBe(false);

    // nC ▲: prev within the NORMAL segment is nB (segment-first) → firstRank(0) - 1024,
    // never a midpoint against the urgent uA's rank.
    fireEvent.click(screen.getByTestId("task-move-up-nC"));
    await waitFor(() => expect(orchdSetTaskRankMock).toHaveBeenCalledWith("nC", 0 - 1024));
  });
});

// ── PRN-06: labeled selects — every unlabeled select carries a `title` mirroring its aria-label so
// a sighted user gets a hover tooltip of its purpose (the current option text alone is ambiguous).
describe("TasksList — labeled selects (PRN-06)", () => {
  it("row selects (priority, status) expose a title equal to their aria-label", () => {
    const a = makeTask({ id: "a", status: "backlog" });
    useAppStore.setState({ tasksByProject: { [projectId]: [a] } }, false);

    render(<TasksList projectId={projectId} />);

    expect(screen.getByTestId("task-priority-select-a").getAttribute("title")).toBe(
      strings.tasks.priorityAria,
    );
    expect(screen.getByTestId("task-status-select-a").getAttribute("title")).toBe(
      strings.tasks.statusAria,
    );
  });

  it("create-form selects (source, priority, parent) expose a title equal to their aria-label", () => {
    render(<TasksList projectId={projectId} />);

    expect(screen.getByTestId("task-create-source").getAttribute("title")).toBe(
      strings.tasks.newSourceAria,
    );
    expect(screen.getByTestId("task-create-priority").getAttribute("title")).toBe(
      strings.tasks.newPriorityAria,
    );
    expect(screen.getByTestId("task-create-parent").getAttribute("title")).toBe(
      strings.tasks.parentAria,
    );
  });
});
