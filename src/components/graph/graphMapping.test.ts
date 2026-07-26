import { describe, it, expect } from "vitest";
import type { GraphView, GraphNode } from "../../ipc/orchd-types";
import {
  toFlowNodes,
  toFlowEdges,
  flowPositionChangeToMove,
  dedupeMovesById,
} from "./graphMapping";

const node = (over: Partial<GraphNode> = {}): GraphNode => ({
  id: "n1",
  projectId: "p1",
  kind: "concept",
  entityType: null,
  entityId: null,
  label: "Node label",
  body: "body",
  posX: 10,
  posY: 20,
  createdAt: 1,
  updatedAt: 1,
  isOrphan: false,
  ...over,
});

const emptyView: GraphView = { nodes: [], edges: [], externalNodes: [] };

describe("graphMapping", () => {
  describe("toFlowNodes", () => {
    it("maps posX/posY to position.x/position.y and kind to type", () => {
      const view: GraphView = { ...emptyView, nodes: [node({ posX: 42, posY: -7 })] };
      const flowNodes = toFlowNodes(view);
      expect(flowNodes).toHaveLength(1);
      expect(flowNodes[0].id).toBe("n1");
      expect(flowNodes[0].position).toEqual({ x: 42, y: -7 });
      expect(flowNodes[0].type).toBe("concept");
      expect(flowNodes[0].data.label).toBe("Node label");
      expect(flowNodes[0].data.kind).toBe("concept");
    });

    it("sets isExternal: false for view.nodes and isExternal: true for view.externalNodes", () => {
      const view: GraphView = {
        nodes: [node({ id: "local1" })],
        edges: [],
        externalNodes: [node({ id: "ext1" })],
      };
      const flowNodes = toFlowNodes(view);
      expect(flowNodes).toHaveLength(2);
      const local = flowNodes.find((n) => n.id === "local1");
      const ext = flowNodes.find((n) => n.id === "ext1");
      expect(local?.data.isExternal).toBe(false);
      expect(ext?.data.isExternal).toBe(true);
    });

    it("marks external ghosts non-draggable/non-selectable, local nodes interactive (GRAPH-1)", () => {
      const view: GraphView = {
        nodes: [node({ id: "local1" })],
        edges: [],
        externalNodes: [node({ id: "ext1" })],
      };
      const flowNodes = toFlowNodes(view);
      const local = flowNodes.find((n) => n.id === "local1");
      const ext = flowNodes.find((n) => n.id === "ext1");
      expect(local?.draggable).toBe(true);
      expect(local?.selectable).toBe(true);
      expect(ext?.draggable).toBe(false);
      expect(ext?.selectable).toBe(false);
    });

    it("passes isOrphan through unchanged", () => {
      const view: GraphView = {
        ...emptyView,
        nodes: [node({ id: "a", isOrphan: false }), node({ id: "b", isOrphan: true })],
      };
      const flowNodes = toFlowNodes(view);
      expect(flowNodes.find((n) => n.id === "a")?.data.isOrphan).toBe(false);
      expect(flowNodes.find((n) => n.id === "b")?.data.isOrphan).toBe(true);
    });

    it("carries the domain node's own projectId through in data.projectId (T7 ghost-click nav seam)", () => {
      const view: GraphView = {
        nodes: [node({ id: "local1", projectId: "p1" })],
        edges: [],
        externalNodes: [node({ id: "ext1", projectId: "p-other" })],
      };
      const flowNodes = toFlowNodes(view);
      expect(flowNodes.find((n) => n.id === "local1")?.data.projectId).toBe("p1");
      expect(flowNodes.find((n) => n.id === "ext1")?.data.projectId).toBe("p-other");
    });

    it("carries entityType/entityId through for an entityRef node, and leaves them undefined otherwise", () => {
      const view: GraphView = {
        ...emptyView,
        nodes: [
          node({
            id: "ref1",
            kind: "entityRef",
            entityType: "goal",
            entityId: "g1",
          }),
          node({ id: "plain1", kind: "note", entityType: null, entityId: null }),
        ],
      };
      const flowNodes = toFlowNodes(view);
      const ref = flowNodes.find((n) => n.id === "ref1");
      const plain = flowNodes.find((n) => n.id === "plain1");
      expect(ref?.type).toBe("entityRef");
      expect(ref?.data.entityType).toBe("goal");
      expect(ref?.data.entityId).toBe("g1");
      expect(plain?.data.entityType).toBeUndefined();
      expect(plain?.data.entityId).toBeUndefined();
    });
  });

  describe("toFlowEdges", () => {
    it("maps source/target/kind/label", () => {
      const view: GraphView = {
        ...emptyView,
        edges: [
          {
            id: "e1",
            sourceNodeId: "n1",
            targetNodeId: "n2",
            kind: "depends",
            label: "blocks",
            createdAt: 1,
          },
        ],
      };
      const flowEdges = toFlowEdges(view);
      expect(flowEdges).toEqual([
        {
          id: "e1",
          source: "n1",
          target: "n2",
          label: "blocks",
          data: { kind: "depends" },
        },
      ]);
    });
  });

  describe("flowPositionChangeToMove", () => {
    it("returns the move for a type:'position' change with a position", () => {
      const move = flowPositionChangeToMove({
        type: "position",
        id: "n1",
        position: { x: 5, y: 9 },
        dragging: true,
      });
      expect(move).toEqual({ id: "n1", posX: 5, posY: 9 });
    });

    it("returns null for a type:'select' change", () => {
      expect(flowPositionChangeToMove({ type: "select", id: "n1" })).toBeNull();
    });

    it("returns null for a type:'dimensions' change", () => {
      expect(
        flowPositionChangeToMove({ type: "dimensions", id: "n1" }),
      ).toBeNull();
    });

    it("returns null for a type:'position' change with no position", () => {
      expect(flowPositionChangeToMove({ type: "position", id: "n1" })).toBeNull();
    });
  });

  describe("dedupeMovesById", () => {
    it("keeps the LAST occurrence's value per id, preserving first-seen order", () => {
      const result = dedupeMovesById([
        { id: "a", posX: 1, posY: 1 },
        { id: "a", posX: 2, posY: 2 },
        { id: "b", posX: 3, posY: 3 },
      ]);
      expect(result).toEqual([
        { id: "a", posX: 2, posY: 2 },
        { id: "b", posX: 3, posY: 3 },
      ]);
    });

    it("returns an empty array for an empty input", () => {
      expect(dedupeMovesById([])).toEqual([]);
    });
  });
});
