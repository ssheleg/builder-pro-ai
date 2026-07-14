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
  orchdGraphMoveNode,
  orchdGraphDeleteNode,
  orchdGraphAddEdge,
  orchdGraphDeleteEdge,
  orchdGraphSearch,
  describeOrchdError,
} from "../../ipc/orchd";
import type { GraphNodeKind } from "../../ipc/orchd-types";
import {
  toFlowNodes,
  toFlowEdges,
  flowPositionChangeToMove,
  dedupeMovesById,
  type GraphNodeMove,
} from "./graphMapping";
import { theme } from "../../theme";

const MONO_FONT = 'ui-monospace, SFMono-Regular, "SF Mono", Menlo, monospace';

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

const NEW_NODE_LABEL = "Новый узел";

/** Locked confirm copy (mirrors `GoalTree.tsx`/`TasksList.tsx`/`IdeasList.tsx`'s identical
 * `window.confirm` guard before a destructive delete — same terse-question register). */
const DELETE_CONFIRM_TEXT = "удалить выбранное?";

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

const flowWrapStyle: CSSProperties = {
  position: "relative",
  width: "100%",
  height: 520,
  border: `1px solid ${theme.colors.border}`,
  borderRadius: 8,
  background: theme.colors.bg,
};

const emptyStateStyle: CSSProperties = {
  position: "absolute",
  inset: 0,
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
  color: theme.colors.textDim,
  fontSize: 13,
  pointerEvents: "none",
};

const toolbarStyle: CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: 8,
  flexWrap: "wrap",
  marginBottom: 8,
};

const selectStyle: CSSProperties = {
  fontFamily: "inherit",
  fontSize: 12,
  color: theme.colors.text,
  background: theme.colors.bg,
  border: `1px solid ${theme.colors.border}`,
  borderRadius: 4,
  padding: "4px 6px",
};

const buttonStyle: CSSProperties = {
  border: `1px solid ${theme.colors.border}`,
  background: "transparent",
  color: theme.colors.text,
  cursor: "pointer",
  fontSize: 12,
  borderRadius: 4,
  padding: "4px 8px",
};

/** Primary (accent-fill) button — design-system.md §5: "primary = accent fill (one per view
 * maximum)". The toolbar's one primary action is adding a node. */
const primaryButtonStyle: CSSProperties = {
  ...buttonStyle,
  color: theme.colors.bg,
  background: theme.colors.accent,
  borderColor: theme.colors.accent,
};

/** Destructive (red-ghost) button — design-system.md §5: "destructive = red border ghost with
 * confirm", mirrors `GoalTree.tsx`/`TasksList.tsx`/`IdeasList.tsx`'s identical `deleteButtonStyle`. */
const deleteButtonStyle: CSSProperties = {
  ...buttonStyle,
  color: theme.colors.statusExited,
  borderColor: theme.colors.statusExited,
};

const searchInputStyle: CSSProperties = {
  fontFamily: MONO_FONT,
  fontSize: 12,
  color: theme.colors.text,
  background: "transparent",
  border: `1px solid ${theme.colors.border}`,
  borderRadius: 4,
  padding: "4px 8px",
  minWidth: 180,
  marginLeft: "auto",
};

const kindLabelStyle: CSSProperties = {
  fontSize: 10,
  fontWeight: 600,
  textTransform: "uppercase",
  letterSpacing: "0.05em",
  color: theme.colors.textDim,
};

const nodeLabelStyle: CSSProperties = {
  fontSize: 12,
  color: theme.colors.text,
  marginTop: 2,
  wordBreak: "break-word",
};

/** Base node card style, layered with per-state accents (external/orphan/match) below — mirrors
 * `design-system.md`'s "Card" atom (`bgElevated` + 1px `border` + radius) shrunk to node scale. */
function nodeCardStyle(data: GraphNodeData, selected: boolean | undefined): CSSProperties {
  return {
    padding: "6px 10px",
    borderRadius: 6,
    background: theme.colors.bgElevated,
    border: `1px solid ${
      data.isOrphan ? theme.colors.statusExited : selected ? theme.colors.accent : theme.colors.border
    }`,
    boxShadow: data.isMatch ? `0 0 0 2px ${theme.colors.accent}` : undefined,
    opacity: data.isExternal ? 0.6 : 1,
    borderStyle: data.isExternal ? "dashed" : "solid",
    minWidth: 96,
    cursor: "pointer",
  };
}

/** Renderer for every non-`entityRef` `GraphNodeKind` (concept/fact/artifact/decision/note) —
 * `graphMapping.ts`'s module doc documents this exact split ("T7 registers an entityRef-specific
 * xyflow node renderer... while every other kind falls through to a shared 'domain node'
 * renderer"). */
function DomainNode({ data, selected }: NodeProps<GraphFlowNode>): JSX.Element {
  return (
    <div style={nodeCardStyle(data, selected)}>
      <Handle type="target" position={Position.Top} />
      <div style={kindLabelStyle}>{data.kind}</div>
      <div style={nodeLabelStyle}>{data.label}</div>
      <Handle type="source" position={Position.Bottom} />
    </div>
  );
}

/** Renderer for `entityRef` nodes: a soft reference to a goal/idea/insight/task. An orphaned
 * reference (D3 — `data.isOrphan`, set by the server's read-time resolver) renders the locked
 * «источник удалён» copy instead of the (now meaningless) stale label. */
function EntityRefNode({ data, selected }: NodeProps<GraphFlowNode>): JSX.Element {
  return (
    <div style={nodeCardStyle(data, selected)}>
      <Handle type="target" position={Position.Top} />
      <div style={kindLabelStyle}>ref · {data.entityType ?? "?"}</div>
      <div style={nodeLabelStyle}>{data.isOrphan ? "источник удалён" : data.label}</div>
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
 * `onConnect` is optimistic (`addEdge` into local state immediately) + `orchdGraphAddEdge(source,
 * target, "relates", "")`; deliberately NOT followed by an explicit `refreshGraph` — the
 * `orchd://graph-changed` push (App.tsx) reconciles the real server-assigned edge id, same as the
 * brief's documented "coarse refresh reconciles" contract for this one path. Every OTHER mutation
 * below (add-node, delete-selected) DOES explicitly `refreshGraph` after success, mirroring every
 * other domain surface's convention (`GoalTree`/`TasksList`/`RulesetPanel`: explicit refresh after
 * a structural mutation, never waiting on the push alone).
 *
 * Node click navigation: an EXTERNAL ghost node (`data.isExternal`, `data.projectId` is the
 * FOREIGN project it lives in — `graphMapping.ts`'s `toFlowNodes`) navigates there concretely via
 * `openProject(data.projectId)`. A LOCAL `entityRef` node click is deliberately left as an honest
 * no-op for now: the project panel has no deep-link infra from the graph tab into a specific
 * goal/idea/insight/task row yet (Цели/Идеи/Задачи/Инсайты are separate tabs with no
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

  const [nodes, setNodes] = useState<GraphFlowNode[]>([]);
  const [edges, setEdges] = useState<GraphFlowEdge[]>([]);
  const [addKind, setAddKind] = useState<GraphNodeKind>(ADDABLE_KINDS[0]);
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
      setEdges((eds) => addEdge<GraphFlowEdge>(connection, eds));
      void orchdGraphAddEdge(connection.source, connection.target, "relates", "").catch((e: unknown) =>
        showToast(describeOrchdError(e)),
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

  async function handleAddNode(): Promise<void> {
    const { posX, posY } = nextNewNodePosition(nodes.length);
    try {
      await orchdGraphAddNode(projectId, addKind, NEW_NODE_LABEL, "", posX, posY);
      await refreshGraph(projectId);
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

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

  return (
    <div data-testid="graph-canvas">
      <div style={toolbarStyle}>
        <select
          data-testid="graph-add-kind-select"
          aria-label="Тип нового узла"
          value={addKind}
          disabled={orchdDown}
          onChange={(e) => setAddKind(e.target.value as GraphNodeKind)}
          style={selectStyle}
        >
          {ADDABLE_KINDS.map((k) => (
            <option key={k} value={k}>
              {k}
            </option>
          ))}
        </select>
        <button
          type="button"
          data-testid="graph-add-node-button"
          disabled={orchdDown}
          onClick={() => void handleAddNode()}
          style={primaryButtonStyle}
        >
          Добавить
        </button>
        <button
          type="button"
          data-testid="graph-delete-selected-button"
          disabled={orchdDown}
          onClick={() => void handleDeleteSelected()}
          style={deleteButtonStyle}
        >
          Удалить выбранное
        </button>
        <input
          data-testid="graph-search-input"
          aria-label="Поиск по графу"
          placeholder="поиск…"
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
            fitView
          />
        </ReactFlowProvider>
        {isEmpty && (
          <div data-testid="graph-empty-state" style={emptyStateStyle}>
            пусто
          </div>
        )}
      </div>
    </div>
  );
}
