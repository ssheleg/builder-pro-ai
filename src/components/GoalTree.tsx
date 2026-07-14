import { useEffect, useMemo, useState, type CSSProperties, type JSX } from "react";
import { useAppStore } from "../store/store";
import {
  orchdCreateGoal,
  orchdUpdateGoal,
  orchdMoveGoal,
  orchdDeleteGoal,
  describeOrchdError,
} from "../ipc/orchd";
import type { Goal, GoalStatus } from "../ipc/orchd-types";
import { theme } from "../theme";

const MONO_FONT = 'ui-monospace, SFMono-Regular, "SF Mono", Menlo, monospace';

/** Confirm copy for the delete-subtree action (S3 spec §10, task-14 brief verbatim). Deleting a
 * goal deletes its whole subtree server-side — this text says so up front, never a silent
 * cascading delete. */
const DELETE_CONFIRM_TEXT = "удалить ветку целиком?";

/** «новая цель» is the seed title for a freshly created subgoal (task-14 brief verbatim) — the
 * owner renames it inline immediately after, same UX as FileTree's inline-rename-after-create. */
const NEW_SUBGOAL_TITLE = "новая цель";

interface TreeRow {
  goal: Goal;
  depth: number;
}

/**
 * Order goals parent-before-children (DFS pre-order), sorted defensively by `ord` at every level
 * — the store list already arrives ordered this way (server-side), but this component never
 * trusts array order as a contract (spec §10 "sort defensively for display"). The strategic root
 * is pinned first among any group that (incorrectly) contained more than one root-level entry,
 * belt-and-suspenders for the "exactly one strategic goal, `parentId: null`" invariant.
 */
function buildRows(goals: Goal[]): TreeRow[] {
  const byParent = new Map<string | null, Goal[]>();
  for (const g of goals) {
    const list = byParent.get(g.parentId);
    if (list) list.push(g);
    else byParent.set(g.parentId, [g]);
  }
  for (const list of byParent.values()) {
    list.sort((a, b) => {
      if (a.kind !== b.kind) return a.kind === "strategic" ? -1 : 1;
      return a.ord - b.ord;
    });
  }

  const rows: TreeRow[] = [];
  function visit(parentId: string | null, depth: number): void {
    const kids = byParent.get(parentId);
    if (!kids) return;
    for (const g of kids) {
      rows.push({ goal: g, depth });
      visit(g.id, depth + 1);
    }
  }
  visit(null, 0);
  return rows;
}

/** Siblings of `goal` (same `parentId`), sorted by `ord` ascending — the set ▲/▼ swap within. */
function siblingsOf(goals: Goal[], goal: Goal): Goal[] {
  return goals.filter((g) => g.parentId === goal.parentId).sort((a, b) => a.ord - b.ord);
}

function rowStyle(depth: number): CSSProperties {
  return {
    display: "flex",
    alignItems: "center",
    gap: 6,
    paddingLeft: 8 + depth * 16,
    paddingRight: 8,
    height: 32,
    fontFamily: MONO_FONT,
    fontSize: 12,
    borderBottom: `1px solid ${theme.colors.border}`,
  };
}

const titleInputStyle: CSSProperties = {
  flex: 1,
  minWidth: 0,
  fontFamily: MONO_FONT,
  fontSize: 12,
  color: theme.colors.text,
  background: "transparent",
  border: "1px solid transparent",
  borderRadius: 4,
  padding: "3px 6px",
};

const selectStyle: CSSProperties = {
  fontFamily: MONO_FONT,
  fontSize: 11,
  color: theme.colors.text,
  background: theme.colors.bg,
  border: `1px solid ${theme.colors.border}`,
  borderRadius: 4,
  padding: "2px 4px",
  flexShrink: 0,
};

const iconButtonStyle: CSSProperties = {
  border: "none",
  background: "transparent",
  color: theme.colors.textDim,
  cursor: "pointer",
  fontSize: 12,
  lineHeight: 1,
  padding: "2px 4px",
  flexShrink: 0,
};

const textButtonStyle: CSSProperties = {
  border: `1px solid ${theme.colors.border}`,
  background: "transparent",
  color: theme.colors.text,
  cursor: "pointer",
  fontSize: 11,
  borderRadius: 4,
  padding: "2px 6px",
  flexShrink: 0,
  whiteSpace: "nowrap",
};

const deleteButtonStyle: CSSProperties = {
  ...textButtonStyle,
  color: theme.colors.statusExited,
  borderColor: theme.colors.statusExited,
};

const STATUS_LABEL: Record<GoalStatus, string> = {
  active: "активна",
  achieved: "достигнута",
  dropped: "брошена",
};

interface GoalRowProps {
  goal: Goal;
  depth: number;
  isStrategic: boolean;
  canMoveUp: boolean;
  canMoveDown: boolean;
  /** `orchdDown` (spec §10): while `true`, every mutating control on this row is disabled — see
   * `GoalTree`'s own doc comment for the "banner in ProjectPanel, disable here" split. */
  disabled: boolean;
  onTitleCommit: (id: string, title: string) => Promise<boolean>;
  onStatusChange: (id: string, status: GoalStatus) => void;
  onAddSubgoal: (parentId: string) => void;
  onDelete: (id: string) => void;
  onMoveUp: (goal: Goal) => void;
  onMoveDown: (goal: Goal) => void;
}

/**
 * One goal-tree row (design-system.md "Tree row" atom, S3 §10). Owns only the in-flight (not yet
 * committed) title edit as local state — every other field is a direct read of `goal` from the
 * store, so a rejected mutation never lies about what was actually saved (the select simply
 * reflects whatever `goal.status` still is; the title reverts to `goal.title` on a rejected
 * commit).
 */
function GoalRow(props: GoalRowProps): JSX.Element {
  const {
    goal,
    depth,
    isStrategic,
    canMoveUp,
    canMoveDown,
    disabled,
    onTitleCommit,
    onStatusChange,
    onAddSubgoal,
    onDelete,
    onMoveUp,
    onMoveDown,
  } = props;

  const [title, setTitle] = useState(goal.title);

  // The store's copy of the title only changes once a refresh lands (e.g. the shared
  // `orchd://goals-changed` invalidation elsewhere) — sync local edit state to it whenever that
  // happens, so an external update is never shadowed by a stale local draft.
  useEffect(() => {
    setTitle(goal.title);
  }, [goal.title]);

  async function commit(): Promise<void> {
    const trimmed = title.trim();
    if (trimmed === "") {
      setTitle(goal.title); // blank -> silent revert, never a malformed empty-title save
      return;
    }
    if (trimmed === goal.title) return;
    const ok = await onTitleCommit(goal.id, trimmed);
    if (!ok) setTitle(goal.title);
  }

  return (
    <div data-testid={`goal-row-${goal.id}`} role="treeitem" style={rowStyle(depth)}>
      <input
        data-testid={`goal-title-input-${goal.id}`}
        aria-label="Название цели"
        value={title}
        disabled={disabled}
        onChange={(e) => setTitle(e.target.value)}
        onBlur={() => void commit()}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            (e.target as HTMLInputElement).blur();
          }
        }}
        style={titleInputStyle}
      />
      <select
        data-testid={`goal-status-${goal.id}`}
        aria-label="Статус цели"
        value={goal.status}
        disabled={disabled}
        onChange={(e) => onStatusChange(goal.id, e.target.value as GoalStatus)}
        style={selectStyle}
      >
        {(Object.keys(STATUS_LABEL) as GoalStatus[]).map((s) => (
          <option key={s} value={s}>
            {STATUS_LABEL[s]}
          </option>
        ))}
      </select>
      {!isStrategic && (
        <button
          type="button"
          data-testid={`goal-move-up-${goal.id}`}
          aria-label="Переместить вверх"
          disabled={disabled || !canMoveUp}
          onClick={() => onMoveUp(goal)}
          style={{ ...iconButtonStyle, opacity: canMoveUp ? 1 : 0.35 }}
        >
          ▲
        </button>
      )}
      {!isStrategic && (
        <button
          type="button"
          data-testid={`goal-move-down-${goal.id}`}
          aria-label="Переместить вниз"
          disabled={disabled || !canMoveDown}
          onClick={() => onMoveDown(goal)}
          style={{ ...iconButtonStyle, opacity: canMoveDown ? 1 : 0.35 }}
        >
          ▼
        </button>
      )}
      <button
        type="button"
        disabled={disabled}
        onClick={() => onAddSubgoal(goal.id)}
        style={textButtonStyle}
      >
        + подцель
      </button>
      {!isStrategic && (
        <button
          type="button"
          data-testid={`goal-delete-${goal.id}`}
          disabled={disabled}
          onClick={() => onDelete(goal.id)}
          style={deleteButtonStyle}
        >
          Удалить
        </button>
      )}
    </div>
  );
}

/**
 * Goal-hierarchy editor (S3 spec §10, D5). Renders `goalsByProject[projectId]` as an indent tree
 * — the strategic goal is the pinned root (`parentId: null`, never deletable/movable per the
 * server-side invariant); every `additional` goal always has a parent, so there is no "add
 * top-level goal" affordance, only per-row "+ подцель" targeting that row as the new parent.
 *
 * Structural mutations (create/delete/move) explicitly `refreshGoals(projectId)` after a
 * successful round-trip so the tree's SHAPE updates immediately rather than waiting on the
 * shared `orchd://goals-changed` push; a plain title/status edit does not — the store's own
 * invalidation pipe (wired in App.tsx, spec §10) is what keeps those fields eventually
 * consistent, same as every other domain surface. Every mutating call is wrapped in try/catch:
 * a rejection never throws unhandled, it surfaces as `showToast(describeOrchdError(e))` (spec §7
 * honest error surface).
 *
 * Honest degradation (spec §10): while the store's `orchdDown` is `true`, every mutating control
 * on every row (title input, status select, move ▲/▼, "+ подцель", Удалить) is disabled — reads
 * (the tree itself) stay live. `ProjectPanel` owns the shared banner; this component only owns
 * disabling its own controls, composed with the pre-existing per-row disable logic (e.g. ▲ on the
 * first sibling) via `disabled || <existing condition>`, never replacing it.
 */
export function GoalTree(props: { projectId: string }): JSX.Element {
  const { projectId } = props;

  const goalsByProject = useAppStore((s) => s.goalsByProject);
  const refreshGoals = useAppStore((s) => s.refreshGoals);
  const showToast = useAppStore((s) => s.showToast);
  const orchdDown = useAppStore((s) => s.orchdDown);

  const goals = goalsByProject[projectId] ?? [];

  useEffect(() => {
    const current = useAppStore.getState().goalsByProject[projectId];
    if (!current || current.length === 0) {
      void refreshGoals(projectId);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [projectId]);

  const rows = useMemo(() => buildRows(goals), [goals]);

  async function handleTitleCommit(id: string, title: string): Promise<boolean> {
    try {
      await orchdUpdateGoal(id, title, null, null, null);
      return true;
    } catch (e) {
      showToast(describeOrchdError(e));
      return false;
    }
  }

  async function handleStatusChange(id: string, status: GoalStatus): Promise<void> {
    try {
      await orchdUpdateGoal(id, null, null, status, null);
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

  async function handleAddSubgoal(parentId: string): Promise<void> {
    try {
      await orchdCreateGoal(projectId, parentId, "additional", NEW_SUBGOAL_TITLE, "");
      await refreshGoals(projectId);
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

  async function handleDelete(id: string): Promise<void> {
    if (!window.confirm(DELETE_CONFIRM_TEXT)) return;
    try {
      await orchdDeleteGoal(id);
      await refreshGoals(projectId);
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

  /**
   * True swap of two adjacent siblings (`goal` and its display `neighbor`). The backend's
   * `move_goal` does a raw `UPDATE goal SET ord=?` with no shift of the displaced row and there
   * is NO `UNIQUE(parent_id, ord)` constraint (crates/orchd/src/persistence.rs) — so a single
   * one-sided move would leave BOTH siblings holding the same `ord` with no `list_goals`
   * tiebreaker, an unspecified order that never self-heals. Capturing BOTH ords up front and
   * issuing two moves (each row takes the OTHER's old ord) keeps every `ord` unique after the
   * round-trip. One `refreshGoals` after both, one try/catch → one honest toast on any failure.
   */
  async function swapWithNeighbor(goal: Goal, neighbor: Goal): Promise<void> {
    const goalOrd = goal.ord;
    const neighborOrd = neighbor.ord;
    try {
      await orchdMoveGoal(goal.id, goal.parentId, neighborOrd);
      await orchdMoveGoal(neighbor.id, neighbor.parentId, goalOrd);
      await refreshGoals(projectId);
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

  async function handleMoveUp(goal: Goal): Promise<void> {
    const siblings = siblingsOf(goals, goal);
    const idx = siblings.findIndex((g) => g.id === goal.id);
    const prev = idx > 0 ? siblings[idx - 1] : undefined;
    if (!prev) return;
    await swapWithNeighbor(goal, prev);
  }

  async function handleMoveDown(goal: Goal): Promise<void> {
    const siblings = siblingsOf(goals, goal);
    const idx = siblings.findIndex((g) => g.id === goal.id);
    const next = idx >= 0 && idx < siblings.length - 1 ? siblings[idx + 1] : undefined;
    if (!next) return;
    await swapWithNeighbor(goal, next);
  }

  if (rows.length === 0) {
    return (
      <div data-testid="goal-tree-empty" style={{ color: theme.colors.textDim, fontSize: 13 }}>
        Дерево целей пусто.
      </div>
    );
  }

  return (
    <div data-testid="goal-tree" role="tree" aria-label="Дерево целей">
      {rows.map(({ goal, depth }) => {
        const isStrategic = goal.kind === "strategic";
        const siblings = siblingsOf(goals, goal);
        const idx = siblings.findIndex((g) => g.id === goal.id);
        return (
          <GoalRow
            key={goal.id}
            goal={goal}
            depth={depth}
            isStrategic={isStrategic}
            canMoveUp={!isStrategic && idx > 0}
            canMoveDown={!isStrategic && idx >= 0 && idx < siblings.length - 1}
            disabled={orchdDown}
            onTitleCommit={handleTitleCommit}
            onStatusChange={handleStatusChange}
            onAddSubgoal={handleAddSubgoal}
            onDelete={handleDelete}
            onMoveUp={handleMoveUp}
            onMoveDown={handleMoveDown}
          />
        );
      })}
    </div>
  );
}
