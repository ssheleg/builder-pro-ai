import type { GraphView, GraphNode, GraphEdge } from "../../ipc/orchd-types";

/**
 * PURE domain → xyflow mapping module (S4 §7, D10's "testable seam" — no xyflow import, no React,
 * renderless-testable). `FlowNode`/`FlowEdge` below are LOCAL types, deliberately shape-compatible
 * with xyflow's own `Node`/`Edge` (same field names: `id`/`type`/`position`/`data` for nodes,
 * `id`/`source`/`target`/`type`/`label`/`data` for edges) so T7's `GraphCanvas` component can hand
 * this module's output straight to `<ReactFlow nodes=… edges=… />` without another adapter layer —
 * but this file itself never imports `@xyflow/react` (or React), so it stays trivially unit-testable
 * in a plain `node` test environment with no renderer involved.
 *
 * `type` scheme (documented once, here): a `FlowNode`'s `type` is set to the domain node's `kind`
 * verbatim (`GraphNodeKind`: `"concept" | "fact" | "artifact" | "decision" | "note" | "entityRef"`).
 * `entityRef` nodes therefore already get their own distinct `type` string ("entityRef") for free —
 * T7 registers an `entityRef`-specific xyflow node renderer (soft-ref label + orphan styling) keyed
 * off that exact string, while every other kind falls through to a shared "domain node" renderer.
 */

/** A node ready for `<ReactFlow nodes=… />` — see the module doc above for the `type` scheme. */
export interface FlowNode {
  id: string;
  type: string;
  position: { x: number; y: number };
  data: {
    label: string;
    kind: string;
    /** Only set (non-`undefined`) for `entityRef` nodes — mirrors `GraphNode.entityType`'s
     * `T | null` wire shape collapsed to `undefined` (never `null`) for xyflow's `data` bag. */
    entityType?: string;
    /** Only set (non-`undefined`) for `entityRef` nodes — mirrors `entityType` above. */
    entityId?: string;
    /** `true` for a node that came from `GraphView.externalNodes` (cross-project edge endpoint
     * rendered read-only/dimmed by T7), `false` for a node that came from `GraphView.nodes`. */
    isExternal: boolean;
    /** Passthrough of `GraphNode.isOrphan` (D3 soft-ref orphan — set by the read-time resolver,
     * never computed here). */
    isOrphan: boolean;
  };
}

/** An edge ready for `<ReactFlow edges=… />`. */
export interface FlowEdge {
  id: string;
  source: string;
  target: string;
  /** The domain edge's label, if any — surfaced at the top level (xyflow renders `label` directly
   * on the edge), NOT inside `data`. */
  label?: string;
  data: {
    /** The domain `GraphEdgeKind` (`"relates" | "depends" | …), carried through for T7's
     * per-kind edge styling (e.g. dashed for `contradicts`). Deliberately NOT xyflow's own `type`
     * field — that's a rendering-strategy slot (default/step/smoothstep/custom), unrelated to this
     * semantic kind. */
    kind: string;
  };
}

/** `flowPositionChangeToMove`'s input — the minimal shape of an xyflow `NodeChange` this module
 * cares about (deliberately NOT importing `@xyflow/react`'s own `NodeChange` union — see the
 * module doc above for why this file has zero xyflow imports). Extra fields on a real xyflow
 * `NodeChange` (e.g. `dragging`) are simply ignored. */
export interface FlowNodeChangeLike {
  type: string;
  id?: string;
  position?: { x: number; y: number };
  dragging?: boolean;
}

/** The debounce-flush contract's move record (T7 batches drag moves, then calls
 * `orchdGraphMoveNode` once per node via this shape). */
export interface GraphNodeMove {
  id: string;
  posX: number;
  posY: number;
}

/**
 * Map one project's `GraphView` into xyflow-ready nodes. Both `view.nodes` (project-local,
 * `isExternal: false`) and `view.externalNodes` (cross-project edge endpoints rendered read-only,
 * `isExternal: true`) are flattened into a single array — xyflow has no separate "external" node
 * concept, so T7 tells them apart purely via `data.isExternal`.
 */
export function toFlowNodes(view: GraphView): FlowNode[] {
  const map = (node: GraphNode, isExternal: boolean): FlowNode => ({
    id: node.id,
    type: node.kind,
    position: { x: node.posX, y: node.posY },
    data: {
      label: node.label,
      kind: node.kind,
      entityType: node.entityType ?? undefined,
      entityId: node.entityId ?? undefined,
      isExternal,
      isOrphan: node.isOrphan,
    },
  });

  return [
    ...view.nodes.map((n) => map(n, false)),
    ...view.externalNodes.map((n) => map(n, true)),
  ];
}

/** Map one project's `GraphView` edges into xyflow-ready edges. */
export function toFlowEdges(view: GraphView): FlowEdge[] {
  return view.edges.map(
    (edge: GraphEdge): FlowEdge => ({
      id: edge.id,
      source: edge.sourceNodeId,
      target: edge.targetNodeId,
      label: edge.label,
      data: { kind: edge.kind },
    }),
  );
}

/**
 * Turn ONE xyflow `NodeChange` into a `GraphNodeMove`, or `null` when the change isn't a position
 * change (or a position change with no `position`/`id`, which xyflow never actually emits but this
 * stays defensive rather than throwing). Select/dimension/remove/etc. changes always map to `null`
 * — this is the ONLY change kind `orchdGraphMoveNode` cares about (T7 filters an xyflow
 * `onNodesChange` batch through this before queuing the debounce-flush).
 */
export function flowPositionChangeToMove(change: FlowNodeChangeLike): GraphNodeMove | null {
  if (change.type !== "position" || !change.position || change.id === undefined) return null;
  return { id: change.id, posX: change.position.x, posY: change.position.y };
}

/**
 * Collapse a batch of moves down to ONE move per node id, keeping the LAST occurrence's
 * position but the FIRST occurrence's slot in the output order (a `Map` re-`set` on an existing
 * key updates its value without moving it — exactly this contract). This is the debounce-flush
 * contract: a drag can emit many intermediate `position` changes for the same node before T7
 * flushes to `orchdGraphMoveNode`, and only the final position should ever be sent.
 */
export function dedupeMovesById(moves: GraphNodeMove[]): GraphNodeMove[] {
  const byId = new Map<string, GraphNodeMove>();
  for (const move of moves) byId.set(move.id, move);
  return Array.from(byId.values());
}
