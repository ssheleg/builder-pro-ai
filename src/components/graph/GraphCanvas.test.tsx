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
        {/* A NON-duplicate connection (n2→n1, opposite direction to fixture edge e1) so xyflow's real
            `addEdge` actually appends an optimistic edge — the seam the GR-02/GR-12 rollback test needs. */}
        <button
          type="button"
          data-testid="stub-connect-fresh"
          onClick={() =>
            props.onConnect?.({ source: "n2", target: "n1", sourceHandle: null, targetHandle: null })
          }
        >
          stub-connect-fresh
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
        {/* GRAPH-1: a position change for the GHOST id — real xyflow never dispatches one (the
            ghost maps to `draggable: false`), so this simulates a ghost that slipped through and
            exercises flushMoves' defensive ghost filter. */}
        <button
          type="button"
          data-testid="stub-move-ghost"
          onClick={() =>
            props.onNodesChange?.([
              { type: "position", id: "ext1", position: { x: 55, y: 66 }, dragging: false },
            ])
          }
        >
          stub-move-ghost
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
        {/* GRAPH-1: selects a local node AND the ghost — real xyflow never dispatches the ghost's
            select change (`selectable: false`), and `applyNodeChanges` itself doesn't honor that
            flag, so this puts a genuinely-selected ghost into local state and exercises
            handleDeleteSelected's defensive filter (not a vacuous pass). */}
        <button
          type="button"
          data-testid="stub-select-n1-and-ghost"
          onClick={() =>
            props.onNodesChange?.([
              { type: "select", id: "n1", selected: true },
              { type: "select", id: "ext1", selected: true },
            ])
          }
        >
          stub-select-n1-and-ghost
        </button>
        <button
          type="button"
          data-testid="stub-select-edge-e1"
          onClick={() => props.onEdgesChange?.([{ type: "select", id: "e1", selected: true }])}
        >
          stub-select-edge-e1
        </button>
        {/* eslint-disable-next-line @typescript-eslint/no-explicit-any */}
        {(props.nodes ?? []).map((n: any) => (
          <div
            key={n.id}
            data-testid={`stub-node-${n.id}`}
            data-match={String(Boolean(n.data?.isMatch))}
            onClick={(e) => props.onNodeClick?.(e, n)}
            onDoubleClick={(e) => props.onNodeDoubleClick?.(e, n)}
          >
            {n.data?.label}
          </div>
        ))}
        {/* Edge seam — one element per edge so a test can count local edges (used by the
            optimistic-edge rollback test). eslint-disable-next-line @typescript-eslint/no-explicit-any */}
        {(props.edges ?? []).map((e: any) => (
          <div key={e.id} data-testid={`stub-edge-${e.id}`} />
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
const orchdGraphUpdateEdgeMock = vi.fn();
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
  orchdGraphUpdateEdge: (...a: unknown[]) => orchdGraphUpdateEdgeMock(...a),
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
  orchdGraphUpdateEdgeMock.mockReset().mockResolvedValue({});
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

  it("the add-node form sends the TYPED title/body (not a hardcoded 'New node') + selected kind", async () => {
    render(<GraphCanvas projectId="p1" />);

    fireEvent.change(screen.getByTestId("graph-add-title-input"), {
      target: { value: "My concept" },
    });
    fireEvent.change(screen.getByTestId("graph-add-body-input"), {
      target: { value: "some detail" },
    });
    const select = screen.getByTestId("graph-add-kind-select") as HTMLSelectElement;
    fireEvent.change(select, { target: { value: "decision" } });
    fireEvent.click(screen.getByTestId("graph-add-node-button"));

    await waitFor(() => {
      expect(orchdGraphAddNodeMock).toHaveBeenCalledTimes(1);
      const call = orchdGraphAddNodeMock.mock.calls[0];
      expect(call[0]).toBe("p1");
      expect(call[1]).toBe("decision");
      expect(call[2]).toBe("My concept"); // the TYPED title, not "New node"
      expect(call[3]).toBe("some detail"); // the TYPED body
      expect(typeof call[4]).toBe("number");
      expect(typeof call[5]).toBe("number");
    });
  });

  it("the add-node button is disabled until a (non-blank) title is typed (title REQUIRED)", () => {
    render(<GraphCanvas projectId="p1" />);
    const button = screen.getByTestId("graph-add-node-button") as HTMLButtonElement;
    // Empty title -> disabled.
    expect(button.disabled).toBe(true);
    // Whitespace-only title -> still disabled.
    fireEvent.change(screen.getByTestId("graph-add-title-input"), { target: { value: "   " } });
    expect(button.disabled).toBe(true);
    // A real title -> enabled.
    fireEvent.change(screen.getByTestId("graph-add-title-input"), { target: { value: "Idea" } });
    expect(button.disabled).toBe(false);
  });

  it("two rapid Add clicks create the node ONCE (double-submit guard, spec D6)", async () => {
    let resolveAdd!: (v: unknown) => void;
    orchdGraphAddNodeMock.mockReset().mockImplementation(
      () =>
        new Promise((res) => {
          resolveAdd = res;
        }),
    );
    render(<GraphCanvas projectId="p1" />);
    fireEvent.change(screen.getByTestId("graph-add-title-input"), { target: { value: "Dup" } });

    const button = screen.getByTestId("graph-add-node-button");
    fireEvent.click(button);
    fireEvent.click(button);

    expect(orchdGraphAddNodeMock).toHaveBeenCalledTimes(1);
    await act(async () => {
      resolveAdd({});
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

  // ── inline rename (double-click a LOCAL node) ────────────────────────────────────────────────

  it("double-clicking a LOCAL node opens a rename input pre-filled with its label, and committing fires orchdGraphUpdateNode(id, newLabel, null)", async () => {
    const refreshGraphMock = vi.fn().mockResolvedValue(undefined);
    useAppStore.setState({ refreshGraph: refreshGraphMock }, false);
    render(<GraphCanvas projectId="p1" />);

    // No rename bar until a double-click.
    expect(screen.queryByTestId("graph-rename-bar")).toBeNull();

    fireEvent.doubleClick(screen.getByTestId("stub-node-n1"));

    const input = screen.getByTestId("graph-rename-input") as HTMLInputElement;
    expect(input.value).toBe("Node one"); // pre-filled with the node's current label

    fireEvent.change(input, { target: { value: "Renamed node" } });
    fireEvent.keyDown(input, { key: "Enter" });

    await waitFor(() => {
      expect(orchdGraphUpdateNodeMock).toHaveBeenCalledWith("n1", "Renamed node", null);
      expect(refreshGraphMock).toHaveBeenCalledWith("p1");
    });
    // The bar closes after a successful commit.
    await waitFor(() => expect(screen.queryByTestId("graph-rename-bar")).toBeNull());
  });

  it("the rename Save button also commits, and Cancel closes the bar without a wire call", async () => {
    render(<GraphCanvas projectId="p1" />);

    fireEvent.doubleClick(screen.getByTestId("stub-node-n1"));
    fireEvent.change(screen.getByTestId("graph-rename-input"), { target: { value: "Via save" } });
    fireEvent.click(screen.getByTestId("graph-rename-save"));
    await waitFor(() =>
      expect(orchdGraphUpdateNodeMock).toHaveBeenCalledWith("n1", "Via save", null),
    );

    // Re-open, then Cancel: no additional wire call, bar closes.
    orchdGraphUpdateNodeMock.mockClear();
    fireEvent.doubleClick(screen.getByTestId("stub-node-n1"));
    fireEvent.click(screen.getByTestId("graph-rename-cancel"));
    expect(screen.queryByTestId("graph-rename-bar")).toBeNull();
    expect(orchdGraphUpdateNodeMock).not.toHaveBeenCalled();
  });

  it("double-clicking an entityRef node does NOT open a rename bar (entityRef is not renameable)", () => {
    render(<GraphCanvas projectId="p1" />);

    // n2 is an entityRef node (see the fixture `view`).
    fireEvent.doubleClick(screen.getByTestId("stub-node-n2"));

    expect(screen.queryByTestId("graph-rename-bar")).toBeNull();
    expect(orchdGraphUpdateNodeMock).not.toHaveBeenCalled();
  });

  it("a blank rename is a silent no-op: Save is disabled and Enter fires no wire call", () => {
    render(<GraphCanvas projectId="p1" />);
    fireEvent.doubleClick(screen.getByTestId("stub-node-n1"));

    const input = screen.getByTestId("graph-rename-input");
    fireEvent.change(input, { target: { value: "   " } });
    expect((screen.getByTestId("graph-rename-save") as HTMLButtonElement).disabled).toBe(true);

    fireEvent.keyDown(input, { key: "Enter" });
    expect(orchdGraphUpdateNodeMock).not.toHaveBeenCalled();
  });

  it("while orchdDown: the rename input and Save are disabled", () => {
    useAppStore.setState({ orchdDown: true }, false);
    render(<GraphCanvas projectId="p1" />);
    // Double-click still opens the bar (no orchd call needed to open it) — its controls disabled.
    fireEvent.doubleClick(screen.getByTestId("stub-node-n1"));
    expect((screen.getByTestId("graph-rename-input") as HTMLInputElement).disabled).toBe(true);
    expect((screen.getByTestId("graph-rename-save") as HTMLButtonElement).disabled).toBe(true);
  });

  // ── edge kind editing (select an edge) ───────────────────────────────────────────────────────

  it("selecting exactly one edge reveals a kind select pre-set to the edge's kind; changing it fires orchdGraphUpdateEdge(id, kind)", async () => {
    const refreshGraphMock = vi.fn().mockResolvedValue(undefined);
    useAppStore.setState({ refreshGraph: refreshGraphMock }, false);
    render(<GraphCanvas projectId="p1" />);

    // Hidden until an edge is selected.
    expect(screen.queryByTestId("graph-edge-kind-select")).toBeNull();

    fireEvent.click(screen.getByTestId("stub-select-edge-e1"));

    const select = screen.getByTestId("graph-edge-kind-select") as HTMLSelectElement;
    expect(select.value).toBe("relates"); // e1's current kind (see fixture)

    fireEvent.change(select, { target: { value: "depends" } });

    await waitFor(() => {
      expect(orchdGraphUpdateEdgeMock).toHaveBeenCalledWith("e1", "depends");
      expect(refreshGraphMock).toHaveBeenCalledWith("p1");
    });
  });

  it("while orchdDown: the edge-kind select is disabled", () => {
    useAppStore.setState({ orchdDown: true }, false);
    render(<GraphCanvas projectId="p1" />);
    fireEvent.click(screen.getByTestId("stub-select-edge-e1"));

    const select = screen.getByTestId("graph-edge-kind-select") as HTMLSelectElement;
    expect(select.disabled).toBe(true);
  });

  it("a failed orchdGraphUpdateEdge surfaces the mapped error via a toast", async () => {
    orchdGraphUpdateEdgeMock.mockRejectedValue({ kind: "daemon", code: "Conflict", message: "boom" });
    render(<GraphCanvas projectId="p1" />);
    fireEvent.click(screen.getByTestId("stub-select-edge-e1"));
    fireEvent.change(screen.getByTestId("graph-edge-kind-select"), { target: { value: "depends" } });

    await waitFor(() => {
      expect(describeOrchdErrorMock).toHaveBeenCalled();
      expect(useAppStore.getState().toast).toBe("orchestrator: error");
    });
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

  it("onConnect optimistically adds a fresh edge and KEEPS it when the add succeeds (no false rollback)", async () => {
    orchdGraphAddEdgeMock.mockResolvedValue({});
    render(<GraphCanvas projectId="p1" />);

    // Fixture starts with exactly one edge (e1).
    expect(screen.getAllByTestId(/^stub-edge-/)).toHaveLength(1);

    fireEvent.click(screen.getByTestId("stub-connect-fresh"));

    await waitFor(() =>
      expect(orchdGraphAddEdgeMock).toHaveBeenCalledWith("n2", "n1", "relates", ""),
    );
    // The optimistic edge stays: a successful add is reconciled by the push, never rolled back.
    expect(screen.getAllByTestId(/^stub-edge-/)).toHaveLength(2);
  });

  it("a REJECTED onConnect rolls the optimistic edge back out AND toasts (GR-02/GR-12 audit fix)", async () => {
    orchdGraphAddEdgeMock.mockRejectedValue({ kind: "daemon", code: "Invariant", message: "boom" });
    render(<GraphCanvas projectId="p1" />);

    expect(screen.getAllByTestId(/^stub-edge-/)).toHaveLength(1);

    fireEvent.click(screen.getByTestId("stub-connect-fresh"));

    // After the daemon refuses the add, the optimistically-added edge is removed (back to just e1)
    // and the failure is surfaced — no phantom edge lingers until the next graph://changed push.
    await waitFor(() => {
      expect(screen.getAllByTestId(/^stub-edge-/)).toHaveLength(1);
      expect(useAppStore.getState().toast).toBe("orchestrator: error");
    });
  });
});
