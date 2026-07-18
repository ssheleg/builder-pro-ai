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
import { useSubmitGuard } from "../hooks/useSubmitGuard";
import { Badge, Button, EmptyState, Panel } from "../ui/primitives";
import { strings } from "../strings";

/** Confirm copy for the delete-subtree action (S3 spec §10, task-14 brief verbatim). Deleting a
 * goal deletes its whole subtree server-side — this text says so up front, never a silent
 * cascading delete. */
const DELETE_CONFIRM_TEXT = strings.goals.deleteConfirm;

/** "new goal" is the seed title for a freshly created subgoal (task-14 brief verbatim) — the
 * owner renames it inline immediately after, same UX as FileTree's inline-rename-after-create. */
const NEW_SUBGOAL_TITLE = strings.goals.newSubgoal;

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

/** The outer treeitem: carries the depth indent (the paddingLeft the indent test asserts — kept as
 * a literal px so `el.style.paddingLeft` reads back exactly `8/24/40px`), the row's bottom border,
 * and the shared mono font — a column so the metric-refs editor (P4b, D7) stacks under the main
 * control line rather than overflowing the fixed-height line. */
function rowContainerStyle(depth: number): CSSProperties {
  return {
    paddingLeft: 8 + depth * 16,
    fontFamily: "var(--font-mono)",
    fontSize: "var(--fs-sm)",
    borderBottom: "1px solid var(--border)",
  };
}

/** The main control line (title input, status, moves, + subgoal, delete) — the original 32px flex
 * row, now nested inside {@link rowContainerStyle}. */
const mainLineStyle: CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: 6,
  paddingRight: 8,
  height: 32,
};

/** The metric_refs editor line (P4b, D7): the row's chips + the add input, wrapping under the main
 * control line. */
const metricLineStyle: CSSProperties = {
  display: "flex",
  alignItems: "center",
  flexWrap: "wrap",
  gap: 4,
  paddingRight: 8,
  paddingBottom: 6,
};

const chipRemoveButtonStyle: CSSProperties = {
  border: "none",
  background: "transparent",
  color: "var(--muted)",
  cursor: "pointer",
  fontSize: "var(--fs-sm)",
  lineHeight: 1,
  padding: "0 2px",
  flexShrink: 0,
};

const metricInputStyle: CSSProperties = {
  fontFamily: "var(--font-mono)",
  fontSize: "var(--fs-xs)",
  color: "var(--ink)",
  background: "transparent",
  border: "1px solid var(--border-strong)",
  borderRadius: "var(--r-sm)",
  padding: "2px 6px",
  width: 100,
  flexShrink: 0,
};

const titleInputStyle: CSSProperties = {
  flex: 1,
  minWidth: 0,
  fontFamily: "var(--font-mono)",
  fontSize: "var(--fs-sm)",
  color: "var(--ink)",
  background: "transparent",
  border: "1px solid transparent",
  borderRadius: "var(--r-sm)",
  padding: "3px 6px",
};

const selectStyle: CSSProperties = {
  fontFamily: "var(--font-mono)",
  fontSize: "var(--fs-xs)",
  color: "var(--ink)",
  background: "var(--panel)",
  border: "1px solid var(--border-strong)",
  borderRadius: "var(--r-sm)",
  padding: "2px 4px",
  flexShrink: 0,
};

const iconButtonStyle: CSSProperties = {
  border: "none",
  background: "transparent",
  color: "var(--muted)",
  cursor: "pointer",
  fontSize: "var(--fs-sm)",
  lineHeight: 1,
  padding: "2px 4px",
  flexShrink: 0,
};

const STATUS_LABEL: Record<GoalStatus, string> = {
  active: strings.goals.status.active,
  achieved: strings.goals.status.achieved,
  dropped: strings.goals.status.dropped,
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
  /** metric_refs chip editor (P4b, O-4, D7): persists the row's full next `metricRefs` array
   * (add appends, remove filters) via the goal-update verb. Double-submit-guarded by the caller. */
  onMetricRefsChange: (id: string, metricRefs: string[]) => void;
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
    onMetricRefsChange,
  } = props;

  const [title, setTitle] = useState(goal.title);
  // The in-flight (not yet committed) new-metric text, same "input reflects intent, store reflects
  // truth" model as `title` above — the chips render straight off `goal.metricRefs`, so a rejected
  // add never leaves a phantom chip (the store simply never gained it).
  const [metricInput, setMetricInput] = useState("");

  /** Add the typed metric on Enter: trim, ignore blank, dedupe against the current refs, then
   * persist the appended array. `disabled`-guarded up front so a keydown on the (disabled-while-
   * `orchdDown`/submitting) input is a hard no-op, never a wire call. */
  function commitMetric(): void {
    if (disabled) return;
    const trimmed = metricInput.trim();
    if (trimmed === "") return; // blank -> ignore, keep whatever is typed
    setMetricInput(""); // consumed either way (added or already-present)
    if (goal.metricRefs.includes(trimmed)) return; // dedupe -> no redundant round-trip
    onMetricRefsChange(goal.id, [...goal.metricRefs, trimmed]);
  }

  function removeMetric(ref: string): void {
    onMetricRefsChange(
      goal.id,
      goal.metricRefs.filter((r) => r !== ref),
    );
  }

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
    <div data-testid={`goal-row-${goal.id}`} role="treeitem" style={rowContainerStyle(depth)}>
      <div style={mainLineStyle}>
        <input
          data-testid={`goal-title-input-${goal.id}`}
          aria-label={strings.goals.titleAria}
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
          aria-label={strings.goals.statusAria}
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
            aria-label={strings.common.moveUp}
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
            aria-label={strings.common.moveDown}
            disabled={disabled || !canMoveDown}
            onClick={() => onMoveDown(goal)}
            style={{ ...iconButtonStyle, opacity: canMoveDown ? 1 : 0.35 }}
          >
            ▼
          </button>
        )}
        <Button
          type="button"
          variant="ghost"
          size="sm"
          disabled={disabled}
          onClick={() => onAddSubgoal(goal.id)}
          style={{ flexShrink: 0, whiteSpace: "nowrap" }}
        >
          {strings.goals.addSubgoal}
        </Button>
        {!isStrategic && (
          <Button
            type="button"
            variant="danger"
            size="sm"
            data-testid={`goal-delete-${goal.id}`}
            disabled={disabled}
            onClick={() => onDelete(goal.id)}
            style={{ flexShrink: 0 }}
          >
            {strings.common.delete}
          </Button>
        )}
      </div>
      <div
        data-testid={`goal-metrics-${goal.id}`}
        role="group"
        aria-label={strings.goals.metricRefsAria}
        style={metricLineStyle}
      >
        {goal.metricRefs.map((ref) => (
          <Badge key={ref} data-testid={`goal-metric-chip-${goal.id}-${ref}`} tone="muted">
            {ref}
            <button
              type="button"
              data-testid={`goal-metric-remove-${goal.id}-${ref}`}
              aria-label={strings.goals.removeMetricAria(ref)}
              disabled={disabled}
              onClick={() => removeMetric(ref)}
              style={chipRemoveButtonStyle}
            >
              ×
            </button>
          </Badge>
        ))}
        <input
          data-testid={`goal-metric-input-${goal.id}`}
          aria-label={strings.goals.addMetricAria}
          placeholder={strings.goals.addMetricPlaceholder}
          value={metricInput}
          disabled={disabled}
          onChange={(e) => setMetricInput(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              commitMetric();
            }
          }}
          style={metricInputStyle}
        />
      </div>
    </div>
  );
}

/**
 * Goal-hierarchy editor (S3 spec §10, D5). Renders `goalsByProject[projectId]` as an indent tree
 * — the strategic goal is the pinned root (`parentId: null`, never deletable/movable per the
 * server-side invariant); every `additional` goal always has a parent, so there is no "add
 * top-level goal" affordance, only per-row "+ subgoal" targeting that row as the new parent.
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
 * on every row (title input, status select, move ▲/▼, "+ subgoal", Delete) is disabled — reads
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
  const { submitting, guard } = useSubmitGuard();

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

  // Double-submit guard (spec D6): a rapid second "+ subgoal" click must NOT create two subgoals
  // (finding P-19). Shared across every row's add button — while one add is in flight the row
  // controls render disabled (below).
  const addSubgoal = guard(handleAddSubgoal);

  /**
   * Persist a row's next `metricRefs` array (P4b, O-4, D7). Uses the pre-existing goal-update verb
   * with `title`/`body`/`status` left `null` (D11 "null = unchanged"), same partial-update shape as
   * `handleTitleCommit`/`handleStatusChange` above — the store's own `orchd://goals-changed`
   * invalidation pipe re-renders the chips once the round-trip lands, so there is no explicit
   * `refreshGoals` here (a field edit, not a structural mutation). One try/catch → one honest toast.
   */
  async function handleMetricRefsChange(id: string, metricRefs: string[]): Promise<void> {
    try {
      await orchdUpdateGoal(id, null, null, null, metricRefs);
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

  // Double-submit-guarded exactly like `addSubgoal` (spec D6): a double Enter on the add-metric
  // input, or a double-click on a chip ×, fires the update verb at most once. Shares the same
  // in-flight lock, so `submitting` disables every mutating control on every row while one is live.
  const changeMetricRefs = guard(handleMetricRefsChange);

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
    return <EmptyState data-testid="goal-tree-empty" title={strings.goals.empty} />;
  }

  return (
    <Panel
      padded={false}
      title={strings.project.tabs.goals}
      actions={<Badge tone="muted">{goals.length}</Badge>}
    >
      <div data-testid="goal-tree" role="tree" aria-label={strings.goals.treeAria}>
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
            disabled={orchdDown || submitting}
            onTitleCommit={handleTitleCommit}
            onStatusChange={handleStatusChange}
            onAddSubgoal={addSubgoal}
            onDelete={handleDelete}
            onMoveUp={handleMoveUp}
            onMoveDown={handleMoveDown}
            onMetricRefsChange={changeMetricRefs}
            />
          );
        })}
      </div>
    </Panel>
  );
}
