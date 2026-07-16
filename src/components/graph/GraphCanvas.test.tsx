// @vitest-environment jsdom
import { describe, it, expect, vi, beforeAll, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, fireEvent, waitFor, act } from "@testing-library/react";
import type { ReactNode } from "react";
import { mockReactFlow } from "./mockReactFlow";
import { strings } from "../../strings";

/**
 * S4 §7 T7 interaction tests (D10, brief-corrected: GraphCanvas RENDERS under jsdom, not just
 * pure-helper unit tests). `<ReactFlow>` itself drives connections/drags via real pointer/D3-drag
 * physics that jsdom doesn't implement even with `mockReactFlow()` (that shim only covers
 * measurement, not synthetic drag gestures) — so `@xyflow/react`'s `ReactFlow`/`ReactFlowProvider`
 * are partially mocked here with a deterministic stub that exposes GraphCanvas's own
 * onConnect/onNodesChange/onNodeClick props as clickable test seams, while every OTHER export
 * (`addEdge`, `applyNodeChanges`, `applyEdgeChanges`, `Handle`, `Position`, all types) stays the
 * REAL xyflow implementation via `importOriginal` — this proves GraphCanvas's OWN wiring
 * (debounce, dedupe, orchdDown guards, wrapper calls), not xyflow's internal drag machinery.
 */
vi.mock("@xyflow/react", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@xyflow/react")>();
  return {
    ...actual,
    ReactFlowProvider: ({ children }: { children: ReactNode }) => <>{children}</>,
    // eslint-disable-next-line @typescript-eslint/no-explicit-any
    ReactFlow: (props: any) => (
      <div data-testid="stub-reactflow">
        <button
          type="button"
          data-testid="stub-connect"
          onClick={() =>
            props.onConnect?.({ source: "n1", target: "n2", sourceHandle: null, targetHandle: null })
          }
        >
          stub-connect
        </button>
        <button
          type="button"
          data-testid="stub-move-a"
          onClick={() =>
            props.onNodesChange?.([
              { type: "position", id: "n1", position: { x: 11, y: 22 }, dragging: true },
            ])
          }
        >
          stub-move-a
        </button>
        <button
          type="button"
          data-testid="stub-move-b"
          onClick={() =>
            props.onNodesChange?.([
              { type: "position", id: "n1", position: { x: 33, y: 44 }, dragging: false },
            ])
          }
        >
          stub-move-b
        </button>
        <button
          type="button"
          data-testid="stub-select-n1"
          onClick={() =>
            props.onNodesChange?.([{ type: "select", id: "n1", selected: true }])
          }
        >
          stub-select-n1
        </button>
        <button
          type="button"
          data-testid="stub-select-both"
          onClick={() =>
            props.onNodesChange?.([
              { type: "select", id: "n1", selected: true },
              { type: "select", id: "n2", selected: true },
            ])
          }
        >
          stub-select-both
        </button>
        {/* eslint-disable-next-line @typescript-eslint/no-explicit-any */}
        {(props.nodes ?? []).map((n: any) => (
          <div
            key={n.id}
            data-testid={`stub-node-${n.id}`}
            data-match={String(Boolean(n.data?.isMatch))}
            onClick={(e) => props.onNodeClick?.(e, n)}
          >
            {n.data?.label}
          </div>
        ))}
      </div>
    ),
  };
});

const orchdGraphAddNodeMock = vi.fn();
const orchdGraphUpdateNodeMock = vi.fn();
const orchdGraphMoveNodeMock = vi.fn();
const orchdGraphDeleteNodeMock = vi.fn();
const orchdGraphAddEdgeMock = vi.fn();
const orchdGraphDeleteEdgeMock = vi.fn();
const orchdGraphSearchMock = vi.fn();
// The store's real `refreshGraph` (which the component calls on mount) invokes this — mocking the
// module WITHOUT it would make the store's mount refresh THROW (undefined is not a function),
// catch, and set the exact "orchestrator: error" toast BEFORE any test acts, silently
// pre-populating the very toast the error-toast test asserts (T7 review #1). It's stubbed here for
// defense-in-depth even though `beforeEach` also swaps the store's `refreshGraph` for a no-op.
const orchdGraphListProjectMock = vi.fn();
const describeOrchdErrorMock = vi.fn((..._a: unknown[]) => "orchestrator: error");

vi.mock("../../ipc/orchd", () => ({
  orchdGraphAddNode: (...a: unknown[]) => orchdGraphAddNodeMock(...a),
  orchdGraphUpdateNode: (...a: unknown[]) => orchdGraphUpdateNodeMock(...a),
  orchdGraphMoveNode: (...a: unknown[]) => orchdGraphMoveNodeMock(...a),
  orchdGraphDeleteNode: (...a: unknown[]) => orchdGraphDeleteNodeMock(...a),
  orchdGraphAddEdge: (...a: unknown[]) => orchdGraphAddEdgeMock(...a),
  orchdGraphDeleteEdge: (...a: unknown[]) => orchdGraphDeleteEdgeMock(...a),
  orchdGraphListProject: (...a: unknown[]) => orchdGraphListProjectMock(...a),
  orchdGraphSearch: (...a: unknown[]) => orchdGraphSearchMock(...a),
  describeOrchdError: (...a: unknown[]) => describeOrchdErrorMock(...a),
}));

import { GraphCanvas } from "./GraphCanvas";
import { useAppStore } from "../../store/store";
import type { GraphView } from "../../ipc/orchd-types";

const view: GraphView = {
  nodes: [
    {
      id: "n1",
      projectId: "p1",
      kind: "concept",
      entityType: null,
      entityId: null,
      label: "Node one",
      body: "",
      posX: 10,
      posY: 10,
      createdAt: 1,
      updatedAt: 1,
      isOrphan: false,
    },
    {
      id: "n2",
      projectId: "p1",
      kind: "entityRef",
      entityType: "goal",
      entityId: "g1",
      label: "Ref node",
      body: "",
      posX: 100,
      posY: 100,
      createdAt: 1,
      updatedAt: 1,
      isOrphan: false,
    },
  ],
  edges: [
    { id: "e1", sourceNodeId: "n1", targetNodeId: "n2", kind: "relates", label: "", createdAt: 1 },
  ],
  externalNodes: [
    {
      id: "ext1",
      projectId: "p-other",
      kind: "concept",
      entityType: null,
      entityId: null,
      label: "Ghost node",
      body: "",
      posX: 200,
      posY: 200,
      createdAt: 1,
      updatedAt: 1,
      isOrphan: false,
    },
  ],
};

beforeAll(() => {
  mockReactFlow();
});

afterEach(cleanup);

beforeEach(() => {
  orchdGraphAddNodeMock.mockReset().mockResolvedValue({});
  orchdGraphUpdateNodeMock.mockReset().mockResolvedValue({});
  orchdGraphMoveNodeMock.mockReset().mockResolvedValue({});
  orchdGraphDeleteNodeMock.mockReset().mockResolvedValue(undefined);
  orchdGraphAddEdgeMock.mockReset().mockResolvedValue({});
  orchdGraphDeleteEdgeMock.mockReset().mockResolvedValue(undefined);
  orchdGraphListProjectMock.mockReset().mockResolvedValue({ nodes: [], edges: [], externalNodes: [] });
  orchdGraphSearchMock.mockReset().mockResolvedValue([]);
  describeOrchdErrorMock.mockReset().mockReturnValue("orchestrator: error");

  useAppStore.setState(
    {
      graphByProject: { p1: view },
      orchdDown: false,
      toast: null, toastQueue: [],
      // Swap the store's real `refreshGraph` for a stable no-op in EVERY test (T7 review #1): the
      // component calls it on mount, and the real one would (via the mocked module) touch
      // orchd wrappers and could pre-populate a toast. Tests that need to ASSERT on the refresh
      // (mount, delete) override this with their own spy.
      refreshGraph: vi.fn().mockResolvedValue(undefined),
    },
    false,
  );
});

describe("GraphCanvas", () => {
  it("refreshes the graph on mount (T6 review must-not-drop item (a))", async () => {
    const refreshGraphMock = vi.fn().mockResolvedValue(undefined);
    useAppStore.setState({ refreshGraph: refreshGraphMock }, false);

    render(<GraphCanvas projectId="p1" />);

    await waitFor(() => {
      expect(refreshGraphMock).toHaveBeenCalledWith("p1");
    });
  });

  it("renders an empty flow (not a crash) while the graph hasn't loaded yet", () => {
    useAppStore.setState({ graphByProject: {} }, false);
    render(<GraphCanvas projectId="p1" />);
    expect(screen.getByTestId("graph-canvas")).toBeTruthy();
    expect(screen.getByTestId("graph-empty-state")).toBeTruthy();
  });

  it("onConnect calls orchdGraphAddEdge(source, target, 'relates', '')", async () => {
    render(<GraphCanvas projectId="p1" />);

    fireEvent.click(screen.getByTestId("stub-connect"));

    await waitFor(() => {
      expect(orchdGraphAddEdgeMock).toHaveBeenCalledWith("n1", "n2", "relates", "");
    });
  });

  it("onConnect does nothing while orchdDown", async () => {
    useAppStore.setState({ orchdDown: true }, false);
    render(<GraphCanvas projectId="p1" />);

    fireEvent.click(screen.getByTestId("stub-connect"));

    await new Promise((r) => setTimeout(r, 0));
    expect(orchdGraphAddEdgeMock).not.toHaveBeenCalled();
  });

  it("a position onNodesChange debounces (400ms) and dedupes: two moves of the same id -> ONE orchdGraphMoveNode call with the LAST coords", async () => {
    vi.useFakeTimers();
    try {
      render(<GraphCanvas projectId="p1" />);

      fireEvent.click(screen.getByTestId("stub-move-a"));
      fireEvent.click(screen.getByTestId("stub-move-b"));

      expect(orchdGraphMoveNodeMock).not.toHaveBeenCalled();

      await vi.advanceTimersByTimeAsync(400);

      expect(orchdGraphMoveNodeMock).toHaveBeenCalledTimes(1);
      expect(orchdGraphMoveNodeMock).toHaveBeenCalledWith("n1", 33, 44);
    } finally {
      vi.useRealTimers();
    }
  });

  it("the move-flush does NOT call orchdGraphMoveNode while orchdDown", async () => {
    vi.useFakeTimers();
    try {
      useAppStore.setState({ orchdDown: true }, false);
      render(<GraphCanvas projectId="p1" />);

      fireEvent.click(screen.getByTestId("stub-move-a"));
      await vi.advanceTimersByTimeAsync(400);

      expect(orchdGraphMoveNodeMock).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });

  it("a move debounce armed before unmount does NOT fire orchdGraphMoveNode after unmount (T7 review #4 — cleanup clears the timer)", async () => {
    vi.useFakeTimers();
    try {
      const { unmount } = render(<GraphCanvas projectId="p1" />);

      fireEvent.click(screen.getByTestId("stub-move-a")); // arms the 400ms debounce
      unmount();
      await vi.advanceTimersByTimeAsync(400);

      expect(orchdGraphMoveNodeMock).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });

  it('toolbar "Add" calls orchdGraphAddNode with the selected kind', async () => {
    render(<GraphCanvas projectId="p1" />);

    const select = screen.getByTestId("graph-add-kind-select") as HTMLSelectElement;
    fireEvent.change(select, { target: { value: "decision" } });
    fireEvent.click(screen.getByTestId("graph-add-node-button"));

    await waitFor(() => {
      expect(orchdGraphAddNodeMock).toHaveBeenCalledTimes(1);
      const call = orchdGraphAddNodeMock.mock.calls[0];
      expect(call[0]).toBe("p1");
      expect(call[1]).toBe("decision");
      expect(call[2]).toBe(strings.graph.newNodeLabel);
      expect(call[3]).toBe("");
      expect(typeof call[4]).toBe("number");
      expect(typeof call[5]).toBe("number");
    });
  });

  it("the add-node kind select excludes entityRef (never hand-created)", () => {
    render(<GraphCanvas projectId="p1" />);
    const select = screen.getByTestId("graph-add-kind-select") as HTMLSelectElement;
    const values = Array.from(select.options).map((o) => o.value);
    expect(values).not.toContain("entityRef");
    expect(values).toEqual(["concept", "fact", "artifact", "decision", "note"]);
  });

  it("ghost (external) node click navigates via openProject(ghostProjectId)", () => {
    const openProjectMock = vi.fn();
    useAppStore.setState({ openProject: openProjectMock }, false);
    render(<GraphCanvas projectId="p1" />);

    fireEvent.click(screen.getByTestId("stub-node-ext1"));

    expect(openProjectMock).toHaveBeenCalledWith("p-other");
  });

  it("local entityRef node click does NOT call any mutating wrapper or crash (honest no-op MVP)", () => {
    const openProjectMock = vi.fn();
    useAppStore.setState({ openProject: openProjectMock }, false);
    render(<GraphCanvas projectId="p1" />);

    fireEvent.click(screen.getByTestId("stub-node-n2"));

    expect(openProjectMock).not.toHaveBeenCalled();
    expect(orchdGraphUpdateNodeMock).not.toHaveBeenCalled();
  });

  it("while orchdDown: the add and delete buttons and the kind select are disabled", () => {
    useAppStore.setState({ orchdDown: true }, false);
    render(<GraphCanvas projectId="p1" />);

    expect((screen.getByTestId("graph-add-node-button") as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByTestId("graph-delete-selected-button") as HTMLButtonElement).disabled).toBe(
      true,
    );
    expect((screen.getByTestId("graph-add-kind-select") as HTMLSelectElement).disabled).toBe(true);
  });

  it("while orchdDown: clicking the (disabled) add button does not call orchdGraphAddNode", () => {
    useAppStore.setState({ orchdDown: true }, false);
    render(<GraphCanvas projectId="p1" />);

    fireEvent.click(screen.getByTestId("graph-add-node-button"));

    expect(orchdGraphAddNodeMock).not.toHaveBeenCalled();
  });

  it("Delete selection asks for confirmation and only calls orchdGraphDeleteNode after it is accepted", async () => {
    const refreshGraphMock = vi.fn().mockResolvedValue(undefined);
    useAppStore.setState({ refreshGraph: refreshGraphMock }, false);
    render(<GraphCanvas projectId="p1" />);
    fireEvent.click(screen.getByTestId("stub-select-n1"));

    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(false);
    fireEvent.click(screen.getByTestId("graph-delete-selected-button"));
    expect(confirmSpy).toHaveBeenCalledWith(strings.graph.deleteConfirm);
    expect(orchdGraphDeleteNodeMock).not.toHaveBeenCalled();

    confirmSpy.mockReturnValue(true);
    fireEvent.click(screen.getByTestId("graph-delete-selected-button"));
    await waitFor(() => {
      expect(orchdGraphDeleteNodeMock).toHaveBeenCalledWith("n1");
      expect(refreshGraphMock).toHaveBeenCalledWith("p1");
    });

    confirmSpy.mockRestore();
  });

  it("Delete selection with nothing selected is a no-op (no confirm dialog, no wrapper call)", () => {
    const confirmSpy = vi.spyOn(window, "confirm");
    render(<GraphCanvas projectId="p1" />);

    fireEvent.click(screen.getByTestId("graph-delete-selected-button"));

    expect(confirmSpy).not.toHaveBeenCalled();
    expect(orchdGraphDeleteNodeMock).not.toHaveBeenCalled();
    confirmSpy.mockRestore();
  });

  it("a partial multi-delete (2nd id rejects) still deletes the 1st, toasts, AND reconciles via refreshGraph (T7 review #3)", async () => {
    const refreshGraphMock = vi.fn().mockResolvedValue(undefined);
    useAppStore.setState({ refreshGraph: refreshGraphMock }, false);
    orchdGraphDeleteNodeMock
      .mockReset()
      .mockResolvedValueOnce(undefined) // n1 deletes
      .mockRejectedValueOnce({ kind: "daemon", code: "Invariant", message: "boom" }); // n2 fails
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(true);

    render(<GraphCanvas projectId="p1" />);
    fireEvent.click(screen.getByTestId("stub-select-both"));
    fireEvent.click(screen.getByTestId("graph-delete-selected-button"));

    await waitFor(() => {
      expect(orchdGraphDeleteNodeMock).toHaveBeenCalledWith("n1"); // 1st delete happened
      expect(orchdGraphDeleteNodeMock).toHaveBeenCalledWith("n2"); // 2nd attempted (rejected)
      expect(useAppStore.getState().toast).toBe("orchestrator: error"); // failure toasted
      expect(refreshGraphMock).toHaveBeenCalledWith("p1"); // canvas reconciled to server truth
    });

    confirmSpy.mockRestore();
  });

  it("a search query debounces then calls orchdGraphSearch(query, projectId)", async () => {
    vi.useFakeTimers();
    try {
      orchdGraphSearchMock.mockResolvedValue([{ id: "n1" }]);
      render(<GraphCanvas projectId="p1" />);

      const input = screen.getByTestId("graph-search-input");
      fireEvent.change(input, { target: { value: "hello" } });

      expect(orchdGraphSearchMock).not.toHaveBeenCalled();

      await vi.advanceTimersByTimeAsync(400);

      expect(orchdGraphSearchMock).toHaveBeenCalledWith("hello", "p1");
    } finally {
      vi.useRealTimers();
    }
  });

  it("ignores a STALE search response: fire A then B, resolve B then A (A last) -> matches reflect B, not A (T7 review #2)", async () => {
    vi.useFakeTimers();
    try {
      let resolveA!: (v: { id: string }[]) => void;
      let resolveB!: (v: { id: string }[]) => void;
      orchdGraphSearchMock
        .mockReset()
        .mockReturnValueOnce(new Promise((r) => { resolveA = r; }))
        .mockReturnValueOnce(new Promise((r) => { resolveB = r; }));

      render(<GraphCanvas projectId="p1" />);
      const input = screen.getByTestId("graph-search-input");

      // Dispatch search "A" (request id 1).
      fireEvent.change(input, { target: { value: "A" } });
      await vi.advanceTimersByTimeAsync(400);
      // Dispatch search "B" (request id 2).
      fireEvent.change(input, { target: { value: "B" } });
      await vi.advanceTimersByTimeAsync(400);

      expect(orchdGraphSearchMock).toHaveBeenCalledTimes(2);

      // B (newer) resolves FIRST, A (older) resolves LAST — the race the guard must survive.
      await act(async () => {
        resolveB([{ id: "n2" }]);
        resolveA([{ id: "n1" }]);
      });

      // matchIds must reflect B's result (n2 highlighted), NOT the stale A (n1) that resolved last.
      expect(screen.getByTestId("stub-node-n2").getAttribute("data-match")).toBe("true");
      expect(screen.getByTestId("stub-node-n1").getAttribute("data-match")).toBe("false");
    } finally {
      vi.useRealTimers();
    }
  });

  it("a failed orchdGraphAddEdge call shows the mapped error via a toast (genuine — mount refresh is a no-op, see beforeEach)", async () => {
    orchdGraphAddEdgeMock.mockRejectedValue({ kind: "daemon", code: "Invariant", message: "boom" });
    render(<GraphCanvas projectId="p1" />);

    fireEvent.click(screen.getByTestId("stub-connect"));

    await waitFor(() => {
      expect(describeOrchdErrorMock).toHaveBeenCalled();
      expect(useAppStore.getState().toast).toBe("orchestrator: error");
    });
  });
});
