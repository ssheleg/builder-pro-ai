// @vitest-environment jsdom
import { describe, it, expect, beforeAll, afterEach } from "vitest";
import { render, screen, cleanup, waitFor } from "@testing-library/react";
import { ReactFlow, ReactFlowProvider, type Node, type NodeTypes } from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import { mockReactFlow } from "./mockReactFlow";
import { EntityRefNode, DomainNode } from "./GraphCanvas";
import { theme } from "../../theme";

/**
 * S4 final-review render tests (D3/D10): `GraphCanvas.test.tsx` stubs `<ReactFlow>` wholesale
 * (its own doc comment explains why — jsdom can't drive real pointer/D3-drag physics), which
 * means `nodeTypes` is never actually exercised there and the D3 honesty signal
 * («источник удалён»), the match-highlight ring, and the external/ghost dimming were UNTESTED
 * render output. This file closes that gap the other way: it mounts a REAL (unmocked)
 * `<ReactFlow nodeTypes={{...}}>` — `mockReactFlow()` (xyflow's own documented jsdom testing
 * recipe, see that module's doc comment) is enough to let a real `<ReactFlow>` mount under jsdom,
 * so `EntityRefNode`/`DomainNode` render through xyflow's actual node-wrapper machinery
 * (including `Handle`), not a hand-rolled stand-in.
 */

beforeAll(() => {
  mockReactFlow();
});

afterEach(cleanup);

const nodeTypes: NodeTypes = {
  entityRef: EntityRefNode,
  concept: DomainNode,
};

/** Mounts ONE node (`type` defaults to `entityRef`) through a real `<ReactFlow>` and returns
 * once its label text is in the DOM (xyflow's `ResizeObserver`-driven measurement, shimmed by
 * `mockReactFlow()`, settles via a macrotask — `waitFor` below rides that out). */
async function renderNode(
  data: Record<string, unknown>,
  type: "entityRef" | "concept" = "entityRef",
): Promise<void> {
  const nodes: Node[] = [
    {
      id: "n1",
      type,
      position: { x: 0, y: 0 },
      data,
    },
  ];
  render(
    <ReactFlowProvider>
      <div style={{ width: "400px", height: "300px" }}>
        <ReactFlow nodes={nodes} edges={[]} nodeTypes={nodeTypes} />
      </div>
    </ReactFlowProvider>,
  );
  await waitFor(() => {
    expect(document.querySelector(".react-flow__node")).toBeTruthy();
  });
}

describe("graph node renderers (S4 final review — D3 honesty signal + match/ghost styling)", () => {
  it("an orphaned entityRef node renders «источник удалён» instead of its stale stored label", async () => {
    await renderNode({
      label: "Stale label from a deleted idea",
      kind: "entityRef",
      entityType: "idea",
      isExternal: false,
      isOrphan: true,
      projectId: "p1",
    });

    expect(screen.getByText("источник удалён")).toBeTruthy();
    expect(screen.queryByText("Stale label from a deleted idea")).toBeNull();
  });

  it("a live (non-orphan) entityRef node renders its live label, NOT the orphan copy", async () => {
    await renderNode({
      label: "A live idea",
      kind: "entityRef",
      entityType: "idea",
      isExternal: false,
      isOrphan: false,
      projectId: "p1",
    });

    expect(screen.getByText("A live idea")).toBeTruthy();
    expect(screen.queryByText("источник удалён")).toBeNull();
  });

  it("a node with data.isMatch:true gets the match-highlight ring (boxShadow accent ring)", async () => {
    await renderNode(
      {
        label: "Matched node",
        kind: "concept",
        isExternal: false,
        isOrphan: false,
        projectId: "p1",
        isMatch: true,
      },
      "concept",
    );

    const label = screen.getByText("Matched node");
    const card = label.parentElement as HTMLElement;
    expect(card.style.boxShadow).toBe(`0 0 0 2px ${theme.colors.accent}`);
  });

  it("a node with data.isMatch:false (or unset) has NO match-highlight ring", async () => {
    await renderNode(
      {
        label: "Unmatched node",
        kind: "concept",
        isExternal: false,
        isOrphan: false,
        projectId: "p1",
        isMatch: false,
      },
      "concept",
    );

    const label = screen.getByText("Unmatched node");
    const card = label.parentElement as HTMLElement;
    expect(card.style.boxShadow).toBe("");
  });

  it("an external/ghost node (data.isExternal:true) renders its distinct dimmed+dashed styling", async () => {
    await renderNode(
      {
        label: "Ghost from another project",
        kind: "concept",
        isExternal: true,
        isOrphan: false,
        projectId: "p-other",
      },
      "concept",
    );

    const label = screen.getByText("Ghost from another project");
    const card = label.parentElement as HTMLElement;
    expect(card.style.opacity).toBe("0.6");
    expect(card.style.borderStyle).toBe("dashed");
  });

  it("a local (non-external) node has full opacity and a solid border, NOT ghost styling", async () => {
    await renderNode(
      {
        label: "Local node",
        kind: "concept",
        isExternal: false,
        isOrphan: false,
        projectId: "p1",
      },
      "concept",
    );

    const label = screen.getByText("Local node");
    const card = label.parentElement as HTMLElement;
    expect(card.style.opacity).toBe("1");
    expect(card.style.borderStyle).toBe("solid");
  });
});
