import { useCallback, useEffect, useMemo, useRef, useState, type CSSProperties, type JSX } from "react";
import {
  ReactFlow,
  ReactFlowProvider,
  addEdge,
  applyNodeChanges,
  applyEdgeChanges,
  Handle,
  Position,
  type Node,
  type Edge,
  type NodeChange,
  type EdgeChange,
  type Connection,
  type NodeMouseHandler,
  type NodeProps,
  type NodeTypes,
} from "@xyflow/react";
import "@xyflow/react/dist/style.css";
import { useAppStore } from "../../store/store";
import {
  orchdGraphAddNode,
  orchdGraphUpdateNode,
  orchdGraphMoveNode,
  orchdGraphDeleteNode,
  orchdGraphAddEdge,
  orchdGraphUpdateEdge,
  orchdGraphDeleteEdge,
  orchdGraphSearch,
  describeOrchdError,
} from "../../ipc/orchd";
import type { GraphNodeKind, GraphEdgeKind } from "../../ipc/orchd-types";
import { useSubmitGuard } from "../../hooks/useSubmitGuard";
import {
  toFlowNodes,
  toFlowEdges,
  flowPositionChangeToMove,
  dedupeMovesById,
  type GraphNodeMove,
} from "./graphMapping";
import { strings } from "../../strings";
import { Badge, Button, EmptyState, Input, Select, TextArea } from "../../ui/primitives";
import type { Tone } from "../../ui/theme";

/** Per-id position-move debounce (S4 §7 brief, D8/D10): a drag emits many intermediate `position`
 * `NodeChange`s — only the LAST one per node, `MOVE_DEBOUNCE_MS` after the last change in a
 * batch, is ever sent to `orchdGraphMoveNode`. */
const MOVE_DEBOUNCE_MS = 400;

/** Search-input debounce — mirrors the move debounce's constant so the panel has one consistent
 * "settle time" feel; `orchdGraphSearch` is a read, so this is purely a rate-limit, not a
 * data-integrity concern like the move flush. */
const SEARCH_DEBOUNCE_MS = 400;

/** Every `GraphNodeKind` the toolbar's add-node `<select>` may create MINUS `entityRef` — an
 * `entityRef` node is always a soft reference resolved server-side from an existing domain
 * entity (goal/idea/insight/task); the UI never hand-creates one (brief: "you don't hand-create
 * entity refs"). */
const ADDABLE_KINDS: GraphNodeKind[] = ["concept", "fact", "artifact", "decision", "note"];

/** Every `GraphEdgeKind` the edge-editing `<select>` may set — the FULL wire enum (unlike nodes,
 * there is no excluded kind: an edge's kind is always user-chosen). Rendering an edge's kind IS its
 * label (spec D7), so this select is the whole edge-editing surface. */
const EDGE_KINDS: GraphEdgeKind[] = [
  "relates",
  "depends",
  "derives",
  "supports",
  "contradicts",
  "parent",
];

/** Locked confirm copy (mirrors `GoalTree.tsx`/`TasksList.tsx`/`IdeasList.tsx`'s identical
 * `window.confirm` guard before a destructive delete — same terse-question register). */
const DELETE_CONFIRM_TEXT = strings.graph.deleteConfirm;

/** Node-kind → semantic tone for the per-node kind `Badge` — one calm accent plus the semantic
 * palette (design-system "Calm Control Room"), so a kind reads at a glance in BOTH themes (every
 * tone resolves through `tokens.css`). `entityRef` keeps the neutral-`info` ref register. */
const KIND_TONE: Record<string, Tone> = {
  concept: "info",
  fact: "ok",
  artifact: "warn",
  decision: "accent",
  note: "muted",
  entityRef: "info",
};

function kindTone(kind: string): Tone {
  return KIND_TONE[kind] ?? "muted";
}

/** A simple deterministic grid default for a freshly added node's position — avoids every new
 * node landing exactly on top of the last one without resorting to non-deterministic randomness
 * (keeps this testable). Wraps every 5 columns, 160px/120px apart. */
function nextNewNodePosition(existingCount: number): { posX: number; posY: number } {
  const col = existingCount % 5;
  const row = Math.floor(existingCount / 5);
  return { posX: 40 + col * 160, posY: 40 + row * 120 };
}

/** GraphCanvas's own `data` shape for xyflow `Node`s — a superset of `graphMapping.ts`'s pure
 * `FlowNode['data']` plus `isMatch` (client-side search-highlight state, layered on top of the
 * store's view locally — never round-tripped). `extends Record<string, unknown>` is required for
 * xyflow's `Node<NodeData>` generic constraint (a plain interface isn't accepted there without
 * it — a known xyflow+TS wrinkle). */
interface GraphNodeData extends Record<string, unknown> {
  label: string;
  kind: string;
  entityType?: string;
  entityId?: string;
  isExternal: boolean;
  isOrphan: boolean;
  projectId: string;
  isMatch?: boolean;
}

interface GraphEdgeData extends Record<string, unknown> {
  kind: string;
}

type GraphFlowNode = Node<GraphNodeData>;
type GraphFlowEdge = Edge<GraphEdgeData>;

/** The flow canvas frame — a token-filled surface so it flips with the theme (a recessed `--bg`
 * well, rounded to `--r-lg`). */
const flowWrapStyle: CSSProperties = {
  position: "relative",
  width: "100%",
  height: 520,
  borderRadius: "var(--r-lg)",
  background: "var(--bg)",
  overflow: "hidden",
};

/** Non-interactive overlay that centres the empty affordance over the (still-mounted) canvas —
 * `pointerEvents: none` so it never eats a click meant for the flow beneath it. */
const emptyOverlayStyle: CSSProperties = {
  position: "absolute",
  inset: 0,
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
  pointerEvents: "none",
};

const toolbarStyle: CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: "var(--sp-2)",
  flexWrap: "wrap",
  marginBottom: "var(--sp-2)",
};

/** Add-node form row — the title input + body textarea + kind select + "Add node" button (spec
 * D7): a group of controls (not an HTML `<form>` — the codebase submits via a guarded button
 * `onClick`, mirroring `CreateProjectDialog`). Aligns its controls to the top so a multi-line body
 * textarea grows downward without stretching its neighbours. */
const formStyle: CSSProperties = {
  display: "flex",
  alignItems: "flex-start",
  gap: "var(--sp-2)",
  flexWrap: "wrap",
  marginBottom: "var(--sp-2)",
};

/** Inline-rename bar — appears (below the add-node form) only while a LOCAL node is being renamed
 * after a double-click. Mirrors `GoalRow`'s inline title-edit (input + Enter-to-commit). */
const renameBarStyle: CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: "var(--sp-2)",
  marginBottom: "var(--sp-2)",
};

/** Wraps the edge-kind `<select>` with its "edge:" caption when exactly one edge is selected. */
const edgeEditLabelStyle: CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: "var(--sp-1)",
  fontSize: "var(--fs-xs)",
  fontWeight: 600,
  textTransform: "uppercase",
  letterSpacing: "0.05em",
  color: "var(--muted)",
};

/** Per-control style overrides layered on top of the primitives' shared control style — only the
 * min sizes / mono numerals that differ from the default, never a repeat of the token base. */
const titleInputStyle: CSSProperties = { minWidth: 160 };
const bodyTextareaStyle: CSSProperties = {
  minWidth: 200,
  minHeight: 30,
  fontFamily: "var(--font-mono)",
};
const searchInputStyle: CSSProperties = {
  minWidth: 180,
  marginLeft: "auto",
  fontFamily: "var(--font-mono)",
};

const nodeLabelStyle: CSSProperties = {
  fontSize: "var(--fs-sm)",
  color: "var(--ink)",
  wordBreak: "break-word",
};

/** Base node card style, layered with per-state accents (external/orphan/match) below — mirrors
 * the primitives' "Card" surface (`--panel` fill + 1px `--hairline` + `--r-md`) shrunk to node
 * scale, so every node stroke/fill resolves through `tokens.css` and reads in light AND dark. */
function nodeCardStyle(data: GraphNodeData, selected: boolean | undefined): CSSProperties {
  const borderColor = data.isOrphan
    ? "var(--danger)"
    : selected
      ? "var(--accent)"
      : "var(--hairline)";
  return {
    display: "flex",
    flexDirection: "column",
    alignItems: "flex-start",
    gap: "var(--sp-1)",
    padding: "var(--sp-2) var(--sp-3)",
    borderRadius: "var(--r-md)",
    background: "var(--panel)",
    border: `1px solid ${borderColor}`,
    boxShadow: data.isMatch ? "0 0 0 2px var(--accent)" : undefined,
    opacity: data.isExternal ? 0.6 : 1,
    borderStyle: data.isExternal ? "dashed" : "solid",
    minWidth: 96,
    cursor: "pointer",
  };
}

/** Renderer for every non-`entityRef` `GraphNodeKind` (concept/fact/artifact/decision/note) —
 * `graphMapping.ts`'s module doc documents this exact split ("T7 registers an entityRef-specific
 * xyflow node renderer... while every other kind falls through to a shared 'domain node'
 * renderer").
 *
 * `export`ed (final review, S4 D3/D10) purely so `nodeRenderers.test.tsx` can mount it directly
 * through a REAL (unmocked) `<ReactFlow nodeTypes={{...}}>` — this file's own `GraphCanvas.test.tsx`
 * stubs `<ReactFlow>` wholesale and therefore never exercises `nodeTypes`, leaving the
 * match-highlight/orphan/ghost render output untested. Pure presentational component: no closure
 * over `GraphCanvas`'s local state, so exporting it is a plain visibility change, not a refactor. */
export function DomainNode({ data, selected }: NodeProps<GraphFlowNode>): JSX.Element {
  return (
    <div style={nodeCardStyle(data, selected)}>
      <Handle type="target" position={Position.Top} />
      <Badge tone={kindTone(data.kind)}>{data.kind}</Badge>
      <div style={nodeLabelStyle}>{data.label}</div>
      <Handle type="source" position={Position.Bottom} />
    </div>
  );
}

/** Renderer for `entityRef` nodes: a soft reference to a goal/idea/insight/task. An orphaned
 * reference (D3 — `data.isOrphan`, set by the server's read-time resolver) renders the locked
 * "source removed" copy instead of the (now meaningless) stale label.
 *
 * `export`ed — see [`DomainNode`]'s doc comment above for why. */
export function EntityRefNode({ data, selected }: NodeProps<GraphFlowNode>): JSX.Element {
  return (
    <div style={nodeCardStyle(data, selected)}>
      <Handle type="target" position={Position.Top} />
      <Badge tone={kindTone("entityRef")}>ref · {data.entityType ?? "?"}</Badge>
      <div style={nodeLabelStyle}>{data.isOrphan ? strings.graph.sourceRemoved : data.label}</div>
      <Handle type="source" position={Position.Bottom} />
    </div>
  );
}

const nodeTypes: NodeTypes = {
  concept: DomainNode,
  fact: DomainNode,
  artifact: DomainNode,
  decision: DomainNode,
  note: DomainNode,
  entityRef: EntityRefNode,
};

/** Token-driven default edge/connection styling so an edge's stroke resolves through `tokens.css`
 * and stays legible in BOTH themes (xyflow's own default `.react-flow__edge-path` grey does not
 * flip with the theme). A calm dimmed `--muted` for resting edges; the one `--accent` for the
 * in-flight connection line the user is dragging. */
const defaultEdgeOptions = { style: { stroke: "color-mix(in srgb, var(--muted) 40%, transparent)" } };
const connectionLineStyle: CSSProperties = { stroke: "var(--accent)" };

/**
 * Project knowledge-graph canvas (S4 §7 T7). Controlled `@xyflow/react` v12 flow: local
 * `nodes`/`edges` `useState` is the xyflow-facing copy (so in-flight drags/selection stay
 * snappy), re-derived from the store's `graphByProject[projectId]` (via T6's pure
 * `toFlowNodes`/`toFlowEdges`) every time that view object changes — `refreshGraph` on mount, a
 * mutation's own explicit refresh, or a future `orchd://graph-changed` push (wired in App.tsx) are
 * the only three ways that view object is ever replaced (D6 coarse-invalidation discipline, same
 * as every other domain slice).
 *
 * `onNodesChange` applies changes to local state immediately (so drags feel live) AND — for
 * `type:"position"` changes only — buffers them in a ref, debounced `MOVE_DEBOUNCE_MS` per flush:
 * `dedupeMovesById` collapses a batch down to one move per node id (keeping the LAST position),
 * so a multi-step drag fires exactly ONE `orchdGraphMoveNode` per node once it settles, not one
 * per intermediate frame.
 *
 * `onConnect` is optimistic (`addEdge` into local state immediately, under a KNOWN id) +
 * `orchdGraphAddEdge(source, target, "relates", "")`. A SUCCESSFUL add is deliberately NOT followed
 * by an explicit `refreshGraph` — the `orchd://graph-changed` push (App.tsx) reconciles the real
 * server-assigned edge id, same as the brief's documented "coarse refresh reconciles" contract for
 * this one path. A REJECTED add (self-loop / duplicate / daemon failure — GR-02/GR-12) rolls the
 * optimistic edge back out of local state immediately, so a refused edge never lingers on the
 * canvas until some later push. Every OTHER mutation below (add-node, delete-selected) explicitly
 * `refreshGraph`es after success, mirroring every other domain surface's convention
 * (`GoalTree`/`TasksList`/`RulesetPanel`: explicit refresh after a structural mutation, never
 * waiting on the push alone).
 *
 * Graph editing (spec D7, O-7): (a) an add-node FORM — a required title `<input>`, an optional
 * body `<textarea>`, and the kind `<select>` — replaces the old hardcoded "New node" placeholder:
 * `handleAddNode` sends the TYPED title/body to `orchdGraphAddNode`. (b) Inline rename —
 * double-clicking a LOCAL (non-`entityRef`, non-external) node opens a rename bar whose input
 * commits via `orchdGraphUpdateNode` on Enter/Save; `entityRef` and external ghost nodes are NOT
 * renameable (an `entityRef`'s label is a server-resolved soft-ref; a ghost belongs to a foreign
 * project). (c) Edge editing — selecting exactly one edge reveals a kind `<select>` firing
 * `orchdGraphUpdateEdge(id, kind)`; the edge's rendered "label" IS its kind, so this select is the
 * whole edge-editing surface. Every one of these mutating controls is `disabled={orchdDown ||
 * submitting}` and routed through a single `useSubmitGuard` (spec D6 double-fire lock).
 *
 * Node click navigation: an EXTERNAL ghost node (`data.isExternal`, `data.projectId` is the
 * FOREIGN project it lives in — `graphMapping.ts`'s `toFlowNodes`) navigates there concretely via
 * `openProject(data.projectId)`. A LOCAL `entityRef` node click is deliberately left as an honest
 * no-op for now: the project panel has no deep-link infra from the graph tab into a specific
 * goal/idea/insight/task row yet (Goals/Ideas/Tasks/Insights are separate tabs with no
 * "scroll-to-and-highlight-this-row" seam) — faking a navigation that doesn't actually land on the
 * referenced entity would be a worse UX than doing nothing, so this stays a no-op until that
 * deep-link seam exists (tracked as follow-up work, not silently forgotten).
 *
 * Honest degradation (spec §10, mirrors `RulesetPanel`/`GoalTree`): while the store's `orchdDown`
 * is `true`, the add-node `<select>` + button and the delete-selected button are all `disabled`;
 * `onConnect` and the move-flush both read `orchdDown` FRESH off the store (not a stale render
 * closure — a drag can start before `orchdDown` flips and only settle/flush after) and early-return
 * without calling any wrapper. The search input stays live (it's a read, same discipline as
 * `RulesetPanel`'s content textarea).
 */
export function GraphCanvas(props: { projectId: string }): JSX.Element {
  const { projectId } = props;

  const view = useAppStore((s) => s.graphByProject[projectId]);
  const refreshGraph = useAppStore((s) => s.refreshGraph);
  const orchdDown = useAppStore((s) => s.orchdDown);
  const showToast = useAppStore((s) => s.showToast);
  const openProject = useAppStore((s) => s.openProject);

  const { submitting, guard } = useSubmitGuard();

  const [nodes, setNodes] = useState<GraphFlowNode[]>([]);
  const [edges, setEdges] = useState<GraphFlowEdge[]>([]);
  const [addKind, setAddKind] = useState<GraphNodeKind>(ADDABLE_KINDS[0]);
  const [addTitle, setAddTitle] = useState("");
  const [addBody, setAddBody] = useState("");
  // Inline-rename state: the id of the node currently being renamed (or `null`) + the in-flight
  // edit text. Set on a double-click of a LOCAL node; cleared on commit/cancel.
  const [renamingNodeId, setRenamingNodeId] = useState<string | null>(null);
  const [renameValue, setRenameValue] = useState("");
  const [searchQuery, setSearchQuery] = useState("");
  const [matchIds, setMatchIds] = useState<Set<string>>(new Set());

  const moveBufferRef = useRef<GraphNodeMove[]>([]);
  const moveTimerRef = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const searchTimerRef = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  // Monotonic search request id (T7 review #2): bumped on every dispatched search AND on every
  // clear, so an OLDER `orchdGraphSearch` promise resolving after a NEWER one can never overwrite
  // `matchIds` with stale highlights — only the latest request's result is applied.
  const searchRequestIdRef = useRef(0);

  // Mount-fetch (T6 review must-not-drop item (a)): unconditional, mirrors RulesetPanel's "always
  // re-Get on mount/scope change" discipline (never a cache-hit short-circuit for this tab).
  useEffect(() => {
    void refreshGraph(projectId);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [projectId]);

  // Re-derive the xyflow-facing local copy whenever the store's view object changes (a fresh
  // fetch/push replaces it wholesale, D6) — the store view is the source of truth; local state
  // only carries in-flight drag/selection between those replacements.
  useEffect(() => {
    if (view === undefined) {
      setNodes([]);
      setEdges([]);
      return;
    }
    setNodes(toFlowNodes(view) as GraphFlowNode[]);
    setEdges(toFlowEdges(view) as GraphFlowEdge[]);
  }, [view]);

  // Debounced search (S4 §7 brief: "a search <input> -> debounced orchdGraphSearch(query,
  // projectId) highlighting matches"). An emptied query clears matches immediately (no debounce
  // needed for "nothing to search").
  useEffect(() => {
    if (searchTimerRef.current !== undefined) {
      clearTimeout(searchTimerRef.current);
      searchTimerRef.current = undefined;
    }
    const q = searchQuery.trim();
    if (q === "") {
      // Invalidate any in-flight search too — a stale resolution must not re-highlight after a clear.
      searchRequestIdRef.current += 1;
      setMatchIds(new Set());
      return;
    }
    searchTimerRef.current = setTimeout(() => {
      searchTimerRef.current = undefined;
      const requestId = (searchRequestIdRef.current += 1);
      orchdGraphSearch(q, projectId)
        .then((results) => {
          // Drop a stale response: a newer search (or a clear) has superseded this one.
          if (requestId !== searchRequestIdRef.current) return;
          setMatchIds(new Set(results.map((n) => n.id)));
        })
        .catch((e) => {
          if (requestId !== searchRequestIdRef.current) return;
          showToast(describeOrchdError(e));
        });
    }, SEARCH_DEBOUNCE_MS);
    return () => {
      if (searchTimerRef.current !== undefined) clearTimeout(searchTimerRef.current);
    };
  }, [searchQuery, projectId, showToast]);

  // Clear any pending debounce timers on unmount — a flush/search firing after the panel has
  // navigated away would target a stale projectId's data.
  useEffect(
    () => () => {
      if (moveTimerRef.current !== undefined) clearTimeout(moveTimerRef.current);
      if (searchTimerRef.current !== undefined) clearTimeout(searchTimerRef.current);
    },
    [],
  );

  const flushMoves = useCallback((): void => {
    moveTimerRef.current = undefined;
    const moves = dedupeMovesById(moveBufferRef.current);
    moveBufferRef.current = [];
    if (moves.length === 0) return;
    // Fresh read (not the render-time `orchdDown` closure): a drag can start before orchd goes
    // down and only settle/flush after — the flush itself must honor the CURRENT state.
    if (useAppStore.getState().orchdDown) return;
    for (const move of moves) {
      orchdGraphMoveNode(move.id, move.posX, move.posY).catch((e: unknown) =>
        showToast(describeOrchdError(e)),
      );
    }
  }, [showToast]);

  const onNodesChange = useCallback(
    (changes: NodeChange<GraphFlowNode>[]): void => {
      setNodes((nds) => applyNodeChanges<GraphFlowNode>(changes, nds));

      const moves = changes
        .map((c) => flowPositionChangeToMove(c))
        .filter((m): m is GraphNodeMove => m !== null);
      if (moves.length === 0) return;

      moveBufferRef.current.push(...moves);
      if (moveTimerRef.current !== undefined) clearTimeout(moveTimerRef.current);
      moveTimerRef.current = setTimeout(flushMoves, MOVE_DEBOUNCE_MS);
    },
    [flushMoves],
  );

  const onEdgesChange = useCallback((changes: EdgeChange<GraphFlowEdge>[]): void => {
    setEdges((eds) => applyEdgeChanges<GraphFlowEdge>(changes, eds));
  }, []);

  const onConnect = useCallback(
    (connection: Connection): void => {
      if (useAppStore.getState().orchdDown) return;
      // Optimistic add under a KNOWN id so a rejected add can be rolled back precisely (GR-02/GR-12):
      // `addEdge` still runs its own duplicate-connection guard, but tagging the edge with our own
      // id lets the failure path below filter out exactly this edge (xyflow's auto-generated id
      // would otherwise be opaque to us).
      const optimisticId = `e-optimistic-${connection.source}-${connection.target}-${Date.now()}`;
      setEdges((eds) => addEdge<GraphFlowEdge>({ ...connection, id: optimisticId }, eds));
      void orchdGraphAddEdge(connection.source, connection.target, "relates", "").catch(
        (e: unknown) => {
          // Roll the optimistic edge back out — the daemon refused it (self-loop / duplicate /
          // failure), so it never existed server-side and must not linger until the next push.
          setEdges((eds) => eds.filter((ed) => ed.id !== optimisticId));
          showToast(describeOrchdError(e));
        },
      );
    },
    [showToast],
  );

  const onNodeClick: NodeMouseHandler = useCallback(
    (_event, node) => {
      const data = node.data as GraphNodeData;
      if (data.isExternal) {
        openProject(data.projectId);
        return;
      }
      // Local entityRef click: honest no-op MVP — see the component doc comment above for why.
    },
    [openProject],
  );

  // Double-click → inline rename, but ONLY for a LOCAL, non-`entityRef` node (spec D7): an
  // `entityRef`'s label is a server-resolved soft-ref (renaming it here is meaningless) and an
  // external ghost lives in a foreign project (read-only on this canvas). Non-renameable
  // double-clicks are an honest no-op.
  const onNodeDoubleClick: NodeMouseHandler = useCallback((_event, node) => {
    const data = node.data as GraphNodeData;
    if (data.isExternal || data.kind === "entityRef") return;
    setRenamingNodeId(node.id);
    setRenameValue(data.label);
  }, []);

  async function handleAddNode(): Promise<void> {
    const title = addTitle.trim();
    // Belt-and-braces: the button is already `disabled` on an empty title / orchdDown, but a
    // guarded handler must never send a blank-title node or a wire call while orchd is down.
    if (title === "") return;
    if (useAppStore.getState().orchdDown) return;
    const { posX, posY } = nextNewNodePosition(nodes.length);
    try {
      await orchdGraphAddNode(projectId, addKind, title, addBody, posX, posY);
      setAddTitle("");
      setAddBody("");
      await refreshGraph(projectId);
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

  async function handleRenameCommit(): Promise<void> {
    const id = renamingNodeId;
    if (id === null) return;
    const trimmed = renameValue.trim();
    if (trimmed === "") return; // required — a blank rename is a silent no-op, never a wire call
    if (useAppStore.getState().orchdDown) return;
    try {
      await orchdGraphUpdateNode(id, trimmed, null);
      setRenamingNodeId(null);
      setRenameValue("");
      await refreshGraph(projectId);
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

  function cancelRename(): void {
    setRenamingNodeId(null);
    setRenameValue("");
  }

  async function handleEdgeKindChange(edgeId: string, kind: GraphEdgeKind): Promise<void> {
    if (useAppStore.getState().orchdDown) return;
    try {
      await orchdGraphUpdateEdge(edgeId, kind);
      await refreshGraph(projectId);
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

  // One shared `useSubmitGuard` fronts every mutating editor control (spec D6): a rapid double
  // Add / Save / kind-change fires its verb at most once, and `submitting` disables every such
  // control while any one is in flight (mirrors `GoalTree`'s single-guard-for-all-mutations shape).
  const addNode = guard(handleAddNode);
  const commitRename = guard(handleRenameCommit);
  const changeEdgeKind = guard(handleEdgeKindChange);

  // Exactly-one-edge selection reveals the edge-kind editor (D7). Zero or many selected ⇒ hidden:
  // the kind select edits a single edge, so a multi-select has no single kind to show.
  const selectedEdges = edges.filter((e) => e.selected);
  const selectedEdge = selectedEdges.length === 1 ? selectedEdges[0] : undefined;

  async function handleDeleteSelected(): Promise<void> {
    const selectedNodeIds = nodes.filter((n) => n.selected).map((n) => n.id);
    const selectedEdgeIds = edges.filter((e) => e.selected).map((e) => e.id);
    if (selectedNodeIds.length === 0 && selectedEdgeIds.length === 0) return;
    if (!window.confirm(DELETE_CONFIRM_TEXT)) return;
    try {
      for (const id of selectedNodeIds) await orchdGraphDeleteNode(id);
      for (const id of selectedEdgeIds) await orchdGraphDeleteEdge(id);
    } catch (e) {
      showToast(describeOrchdError(e));
    } finally {
      // Reconcile the canvas to server truth whether the loop completed fully OR aborted on a
      // partial failure (T7 review #3): the ids already deleted before a mid-loop rejection must
      // not linger on the canvas until some later unrelated refresh. `refreshGraph` swallows its
      // own errors into a toast, so awaiting it here can't throw past this handler.
      await refreshGraph(projectId);
    }
  }

  const displayNodes = useMemo<GraphFlowNode[]>(
    () =>
      nodes.map((n) => ({
        ...n,
        data: { ...n.data, isMatch: matchIds.has(n.id) },
      })),
    [nodes, matchIds],
  );

  const isEmpty = displayNodes.length === 0;

  const addTitleEmpty = addTitle.trim() === "";
  const renameValueEmpty = renameValue.trim() === "";

  return (
    <div data-testid="graph-canvas">
      {/* Add-node form (spec D7): required title + optional body + kind + guarded Add button. */}
      <div style={formStyle} data-testid="graph-add-form">
        <Input
          data-testid="graph-add-title-input"
          aria-label={strings.graph.titleAria}
          placeholder={strings.graph.titlePlaceholder}
          value={addTitle}
          disabled={orchdDown || submitting}
          onChange={(e) => setAddTitle(e.target.value)}
          style={titleInputStyle}
        />
        <TextArea
          data-testid="graph-add-body-input"
          aria-label={strings.graph.bodyAria}
          placeholder={strings.graph.bodyPlaceholder}
          value={addBody}
          disabled={orchdDown || submitting}
          rows={2}
          onChange={(e) => setAddBody(e.target.value)}
          style={bodyTextareaStyle}
        />
        <Select
          data-testid="graph-add-kind-select"
          aria-label={strings.graph.newNodeTypeAria}
          value={addKind}
          disabled={orchdDown || submitting}
          onChange={(e) => setAddKind(e.target.value as GraphNodeKind)}
        >
          {ADDABLE_KINDS.map((k) => (
            <option key={k} value={k}>
              {k}
            </option>
          ))}
        </Select>
        <Button
          type="button"
          variant="primary"
          size="sm"
          data-testid="graph-add-node-button"
          disabled={orchdDown || submitting || addTitleEmpty}
          onClick={() => void addNode()}
        >
          {strings.graph.addNode}
        </Button>
      </div>

      {/* Inline-rename bar — only while a local node is being renamed (double-click). */}
      {renamingNodeId !== null && (
        <div style={renameBarStyle} data-testid="graph-rename-bar">
          <Input
            data-testid="graph-rename-input"
            aria-label={strings.graph.renameAria}
            value={renameValue}
            disabled={orchdDown || submitting}
            autoFocus
            onChange={(e) => setRenameValue(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") {
                e.preventDefault();
                void commitRename();
              } else if (e.key === "Escape") {
                e.preventDefault();
                cancelRename();
              }
            }}
            style={titleInputStyle}
          />
          <Button
            type="button"
            variant="primary"
            size="sm"
            data-testid="graph-rename-save"
            disabled={orchdDown || submitting || renameValueEmpty}
            onClick={() => void commitRename()}
          >
            {strings.graph.renameSave}
          </Button>
          <Button
            type="button"
            variant="ghost"
            size="sm"
            data-testid="graph-rename-cancel"
            onClick={cancelRename}
          >
            {strings.graph.renameCancel}
          </Button>
        </div>
      )}

      <div style={toolbarStyle}>
        <Button
          type="button"
          variant="danger"
          size="sm"
          data-testid="graph-delete-selected-button"
          disabled={orchdDown}
          onClick={() => void handleDeleteSelected()}
        >
          {strings.graph.deleteSelection}
        </Button>
        {/* Edge-kind editor — shown only when exactly one edge is selected (spec D7). */}
        {selectedEdge && (
          <label style={edgeEditLabelStyle}>
            {strings.graph.edgeKindLabel}
            <Select
              data-testid="graph-edge-kind-select"
              aria-label={strings.graph.edgeKindAria}
              value={(selectedEdge.data?.kind as GraphEdgeKind | undefined) ?? "relates"}
              disabled={orchdDown || submitting}
              onChange={(e) => void changeEdgeKind(selectedEdge.id, e.target.value as GraphEdgeKind)}
            >
              {EDGE_KINDS.map((k) => (
                <option key={k} value={k}>
                  {k}
                </option>
              ))}
            </Select>
          </label>
        )}
        <Input
          data-testid="graph-search-input"
          aria-label={strings.graph.searchAria}
          placeholder={strings.graph.searchPlaceholder}
          value={searchQuery}
          onChange={(e) => setSearchQuery(e.target.value)}
          style={searchInputStyle}
        />
      </div>

      <div style={flowWrapStyle}>
        <ReactFlowProvider>
          <ReactFlow
            nodes={displayNodes}
            edges={edges}
            nodeTypes={nodeTypes}
            onNodesChange={onNodesChange}
            onEdgesChange={onEdgesChange}
            onConnect={onConnect}
            onNodeClick={onNodeClick}
            onNodeDoubleClick={onNodeDoubleClick}
            defaultEdgeOptions={defaultEdgeOptions}
            connectionLineStyle={connectionLineStyle}
            fitView
          />
        </ReactFlowProvider>
        {isEmpty && (
          <div style={emptyOverlayStyle}>
            <EmptyState data-testid="graph-empty-state" title={strings.graph.empty} />
          </div>
        )}
      </div>
    </div>
  );
}
