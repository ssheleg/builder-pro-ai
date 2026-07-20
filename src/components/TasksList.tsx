import { useEffect, useMemo, useState, type CSSProperties, type JSX } from "react";
import { useAppStore } from "../store/store";
import {
  orchdCreateTask,
  orchdSetTaskStatus,
  orchdSetTaskRank,
  orchdDeleteTask,
  describeOrchdError,
} from "../ipc/orchd";
import type { DomainTask, TaskSource, TaskStatus } from "../ipc/orchd-types";
import { useSubmitGuard } from "../hooks/useSubmitGuard";
import { Badge, Button } from "../ui/primitives";
import { strings } from "../strings";

/** Locked enum order (spec §4.2 `TaskStatus`) — the status groups render in exactly this order,
 * always, even for an empty group (design-system.md "Status group" atom, S3 §10). */
const STATUS_VALUES: TaskStatus[] = ["backlog", "todo", "waiting", "progress", "testing", "done"];

const STATUS_LABEL: Record<TaskStatus, string> = {
  backlog: strings.tasks.status.backlog,
  todo: strings.tasks.status.todo,
  waiting: strings.tasks.status.waiting,
  progress: strings.tasks.status.progress,
  testing: strings.tasks.status.testing,
  done: strings.tasks.status.done,
};

/** Locked enum order (spec §4.2 `TaskSource`) — the create dialog's source select. */
const SOURCE_VALUES: TaskSource[] = ["idea", "insight", "bug", "plan"];

const SOURCE_LABEL: Record<TaskSource, string> = {
  idea: strings.tasks.source.idea,
  insight: strings.tasks.source.insight,
  bug: strings.tasks.source.bug,
  plan: strings.tasks.source.plan,
};

/**
 * Fractional-rank move increment (task-16 brief "CRITICAL rank lesson"). `orchd_set_task_rank`
 * sets an EXACT f64 rank with no server-side shift of neighbors — moving a row to either end of
 * its group must land it strictly outside the current min/max, so `firstRank - RANK_GAP` /
 * `lastRank + RANK_GAP` is used instead of a midpoint (there is no "far" neighbor to average
 * with). Moving a row BETWEEN two existing neighbors always uses their midpoint, which — for
 * distinct f64 ranks — is itself distinct from both, so a SINGLE `orchdSetTaskRank` call per move
 * is correct (unlike `GoalTree`'s ordinal `ord` swap, which needs two calls to avoid a collision).
 */
const RANK_GAP = 1024;

/** Confirm copy for task delete (mirrors `GoalTree`/`IdeasList`'s honesty pattern). When the task
 * has descendants the message names the exact cascade count up front — never a silent cascading
 * delete (task-16 brief "will delete N subtasks" verbatim). */
function deleteConfirmText(descendantCount: number): string {
  if (descendantCount === 0) return strings.tasks.deleteConfirm;
  return strings.tasks.deleteConfirmWithChildren(descendantCount);
}

/** Every descendant (children, grandchildren, …) of `id` within `tasks` — the count the delete
 * confirm names. Recursive rather than direct-children-only so a deeper subtask chain is still
 * reported honestly, even though the current domain model typically nests one level deep. */
function countDescendants(tasks: DomainTask[], id: string): number {
  let count = 0;
  const stack: string[] = [id];
  while (stack.length > 0) {
    const current = stack.pop() as string;
    for (const t of tasks) {
      if (t.parentId === current) {
        count += 1;
        stack.push(t.id);
      }
    }
  }
  return count;
}

/**
 * Groups `tasks` by status (spec §4.2 order, §10) and sorts each group by `rank` ascending — the
 * single flat order that both drives the group's on-screen row order AND is what ▲/▼'s midpoint
 * math operates against (task-16 brief: "the adjacent same-group task"). Subtasks are NOT
 * re-nested into a parent-relative tree here — indentation is a pure rendering detail applied per
 * row (`rowDepth` below), so a subtask still competes for rank position with every other task in
 * its status group exactly like a top-level task.
 */
function groupByStatus(tasks: DomainTask[]): Map<TaskStatus, DomainTask[]> {
  const groups = new Map<TaskStatus, DomainTask[]>();
  for (const s of STATUS_VALUES) groups.set(s, []);
  for (const t of tasks) {
    groups.get(t.status)?.push(t);
  }
  for (const list of groups.values()) {
    list.sort((a, b) => a.rank - b.rank);
  }
  return groups;
}

/** Visual indent for a subtask row — binary (0 or 1 level), not full ancestor-chain depth, since
 * group order is flat rank order rather than a parent-first tree (see `groupByStatus`). */
function rowDepth(task: DomainTask): number {
  return task.parentId !== null ? 1 : 0;
}

const sectionStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: "var(--sp-1)",
  marginBottom: "var(--sp-3)",
};

const sectionHeaderStyle: CSSProperties = {
  fontFamily: "var(--font-mono)",
  fontSize: "var(--fs-xs)",
  fontWeight: 600,
  color: "var(--muted)",
  textTransform: "uppercase",
  letterSpacing: "0.04em",
  padding: "var(--sp-1) var(--sp-2)",
};

function rowStyle(depth: number): CSSProperties {
  return {
    display: "flex",
    alignItems: "center",
    gap: "var(--sp-2)",
    paddingLeft: 8 + depth * 16,
    paddingRight: 8,
    height: 32,
    fontFamily: "var(--font-mono)",
    fontSize: "var(--fs-sm)",
    borderBottom: "1px solid var(--hairline)",
  };
}

const titleTextStyle: CSSProperties = {
  flex: 1,
  minWidth: 0,
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
  color: "var(--ink)",
};

const selectStyle: CSSProperties = {
  fontFamily: "var(--font-mono)",
  fontSize: "var(--fs-xs)",
  color: "var(--ink)",
  background: "var(--panel-2)",
  border: "none",
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

const createFormStyle: CSSProperties = {
  display: "flex",
  flexWrap: "wrap",
  gap: "var(--sp-2)",
  padding: "var(--sp-2) var(--sp-3)",
  marginBottom: "var(--sp-3)",
  border: "1px dashed var(--hairline)",
  borderRadius: "var(--r-lg)",
  background: "var(--panel-2)",
};

const createInputStyle: CSSProperties = {
  flex: "1 1 160px",
  minWidth: 0,
  fontFamily: "var(--font-mono)",
  fontSize: "var(--fs-sm)",
  color: "var(--ink)",
  background: "var(--panel-2)",
  border: "none",
  borderRadius: "var(--r-sm)",
  padding: "3px 6px",
};

interface TaskRowProps {
  task: DomainTask;
  canMoveUp: boolean;
  canMoveDown: boolean;
  /** `orchdDown` (spec §10): while `true`, every mutating control on this row is disabled — see
   * `TasksList`'s own doc comment. */
  disabled: boolean;
  onStatusChange: (id: string, status: TaskStatus) => void;
  onMoveUp: (task: DomainTask) => void;
  onMoveDown: (task: DomainTask) => void;
  onDelete: (task: DomainTask) => void;
}

/** One task row (design-system.md "Task row" atom, S3 §10). Title/tags/source are read-only here
 * — inline title/body editing is out of this task's scope (task-16 brief lists only status
 * select, rank ▲/▼, and delete per row); the create dialog is the only place fields are entered. */
function TaskRow(props: TaskRowProps): JSX.Element {
  const { task, canMoveUp, canMoveDown, disabled, onStatusChange, onMoveUp, onMoveDown, onDelete } =
    props;
  const depth = rowDepth(task);

  return (
    <div data-testid={`task-row-${task.id}`} role="listitem" style={rowStyle(depth)}>
      <span data-testid={`task-title-${task.id}`} style={titleTextStyle}>
        {task.title}
      </span>
      {task.source && (
        <Badge data-testid={`task-source-${task.id}`} tone="muted">
          {SOURCE_LABEL[task.source]}
        </Badge>
      )}
      <select
        data-testid={`task-status-select-${task.id}`}
        aria-label={strings.tasks.statusAria}
        value={task.status}
        disabled={disabled}
        onChange={(e) => onStatusChange(task.id, e.target.value as TaskStatus)}
        style={selectStyle}
      >
        {STATUS_VALUES.map((s) => (
          <option key={s} value={s}>
            {STATUS_LABEL[s]}
          </option>
        ))}
      </select>
      <button
        type="button"
        data-testid={`task-move-up-${task.id}`}
        aria-label={strings.common.moveUp}
        disabled={disabled || !canMoveUp}
        onClick={() => onMoveUp(task)}
        style={{ ...iconButtonStyle, opacity: canMoveUp ? 1 : 0.35 }}
      >
        ▲
      </button>
      <button
        type="button"
        data-testid={`task-move-down-${task.id}`}
        aria-label={strings.common.moveDown}
        disabled={disabled || !canMoveDown}
        onClick={() => onMoveDown(task)}
        style={{ ...iconButtonStyle, opacity: canMoveDown ? 1 : 0.35 }}
      >
        ▼
      </button>
      <Button
        type="button"
        variant="danger"
        size="sm"
        data-testid={`task-delete-${task.id}`}
        disabled={disabled}
        onClick={() => onDelete(task)}
        style={{ flexShrink: 0 }}
      >
        {strings.common.delete}
      </Button>
    </div>
  );
}

/**
 * Status-grouped task list with fractional-rank reordering (S3 spec §10, task-16). Renders the
 * six `TaskStatus` groups in their locked spec §4.2 order, always (even empty), each internally
 * rank-ordered ascending; ▲/▼ moves compute a midpoint against the two would-be neighbors in that
 * SAME flat rank order (see `groupByStatus`/`RANK_GAP` docs above) and issue a single
 * `orchdSetTaskRank` call — no server-side renumbering exists, so the computed rank must already
 * be unique and correctly positioned.
 *
 * Structural mutations (create/delete — anything that changes which rows exist) explicitly
 * `refreshTasks(projectId)` after a successful round-trip, mirroring `GoalTree`/`IdeasList`'s
 * split between structural and field-level mutations; a status or rank edit relies on the shared
 * `orchd://tasks-changed` → `refreshTasks` pipe wired in App.tsx. Every mutating call is wrapped
 * in try/catch → `showToast(describeOrchdError(e))` (spec §7 honest error surface).
 *
 * Honest degradation (spec §10): while the store's `orchdDown` is `true`, every per-row mutating
 * control (status select, move ▲/▼, Delete) and the create form's submit button are disabled —
 * reads (the groups/rows themselves) stay live. `ProjectPanel` owns the shared banner; this
 * component only owns disabling its own controls, composed with the pre-existing per-row disable
 * logic (e.g. ▲ on the first row of a group) via `disabled || <existing condition>`.
 */
export function TasksList(props: { projectId: string }): JSX.Element {
  const { projectId } = props;

  const tasksByProject = useAppStore((s) => s.tasksByProject);
  const refreshTasks = useAppStore((s) => s.refreshTasks);
  const showToast = useAppStore((s) => s.showToast);
  const orchdDown = useAppStore((s) => s.orchdDown);

  const tasks = tasksByProject[projectId] ?? [];

  useEffect(() => {
    const current = useAppStore.getState().tasksByProject[projectId];
    if (!current || current.length === 0) {
      void refreshTasks(projectId);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [projectId]);

  const groups = useMemo(() => groupByStatus(tasks), [tasks]);
  const { submitting, guard } = useSubmitGuard();

  const [createTitle, setCreateTitle] = useState("");
  const [createBody, setCreateBody] = useState("");
  const [createSource, setCreateSource] = useState<TaskSource>("idea");
  const [createParentId, setCreateParentId] = useState("");
  const [createTags, setCreateTags] = useState("");

  async function handleStatusChange(id: string, status: TaskStatus): Promise<void> {
    try {
      await orchdSetTaskStatus(id, status);
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

  async function applyRank(id: string, rank: number): Promise<void> {
    try {
      await orchdSetTaskRank(id, rank);
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

  /**
   * ▲: move `task` above the row currently just before it in its status group's rank order. The
   * new rank is the midpoint of that row's two would-be new neighbors — the row two spots up
   * (`prevPrev`) and the row one spot up (`prev`, whose position `task` is overtaking). When
   * `prev` is already the group's first row (no `prevPrev`), there is nothing to average with, so
   * the new rank goes below the group's floor: `firstRank - RANK_GAP`.
   */
  async function handleMoveUp(group: DomainTask[], task: DomainTask): Promise<void> {
    const idx = group.findIndex((t) => t.id === task.id);
    if (idx <= 0) return;
    const prev = group[idx - 1];
    const prevPrev = idx - 2 >= 0 ? group[idx - 2] : undefined;
    const newRank = prevPrev ? (prevPrev.rank + prev.rank) / 2 : prev.rank - RANK_GAP;
    await applyRank(task.id, newRank);
  }

  /** ▼: symmetric to `handleMoveUp` — midpoint of the row one spot down (`next`) and two spots
   * down (`nextNext`); with no `nextNext` (next is already the group's last row), the new rank
   * goes above the group's ceiling: `lastRank + RANK_GAP`. */
  async function handleMoveDown(group: DomainTask[], task: DomainTask): Promise<void> {
    const idx = group.findIndex((t) => t.id === task.id);
    if (idx < 0 || idx >= group.length - 1) return;
    const next = group[idx + 1];
    const nextNext = idx + 2 < group.length ? group[idx + 2] : undefined;
    const newRank = nextNext ? (next.rank + nextNext.rank) / 2 : next.rank + RANK_GAP;
    await applyRank(task.id, newRank);
  }

  async function handleDelete(task: DomainTask): Promise<void> {
    const descendantCount = countDescendants(tasks, task.id);
    if (!window.confirm(deleteConfirmText(descendantCount))) return;
    try {
      await orchdDeleteTask(task.id);
      await refreshTasks(projectId);
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

  async function handleCreate(): Promise<void> {
    const title = createTitle.trim();
    if (title === "") return;
    const tags = createTags
      .split(",")
      .map((tag) => tag.trim())
      .filter((tag) => tag !== "");
    const parentId = createParentId === "" ? null : createParentId;
    try {
      await orchdCreateTask(projectId, parentId, title, createBody, null, createSource, null, tags);
      setCreateTitle("");
      setCreateBody("");
      setCreateSource("idea");
      setCreateParentId("");
      setCreateTags("");
      await refreshTasks(projectId);
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

  // Double-submit guard (spec D6): a rapid second "+ task" click must NOT create a duplicate task
  // (finding H-01 / P-19).
  const submitCreate = guard(handleCreate);

  return (
    <div data-testid="tasks-list">
      <div style={createFormStyle}>
        <input
          data-testid="task-create-title"
          aria-label={strings.tasks.newTitleAria}
          placeholder={strings.tasks.newTitlePlaceholder}
          value={createTitle}
          onChange={(e) => setCreateTitle(e.target.value)}
          style={createInputStyle}
        />
        <textarea
          data-testid="task-create-body"
          aria-label={strings.tasks.newDescriptionAria}
          placeholder={strings.common.descriptionOptional}
          value={createBody}
          onChange={(e) => setCreateBody(e.target.value)}
          rows={2}
          style={{ ...createInputStyle, flex: "1 1 100%", resize: "vertical" }}
        />
        <select
          data-testid="task-create-source"
          aria-label={strings.tasks.newSourceAria}
          value={createSource}
          onChange={(e) => setCreateSource(e.target.value as TaskSource)}
          style={selectStyle}
        >
          {SOURCE_VALUES.map((s) => (
            <option key={s} value={s}>
              {SOURCE_LABEL[s]}
            </option>
          ))}
        </select>
        <select
          data-testid="task-create-parent"
          aria-label={strings.tasks.parentAria}
          value={createParentId}
          onChange={(e) => setCreateParentId(e.target.value)}
          style={selectStyle}
        >
          <option value="">{strings.tasks.noParent}</option>
          {tasks.map((t) => (
            <option key={t.id} value={t.id}>
              {t.title}
            </option>
          ))}
        </select>
        <input
          data-testid="task-create-tags"
          aria-label={strings.tasks.newTagsAria}
          placeholder={strings.tasks.tagsPlaceholder}
          value={createTags}
          onChange={(e) => setCreateTags(e.target.value)}
          style={createInputStyle}
        />
        <Button
          type="button"
          variant="primary"
          size="sm"
          data-testid="task-create-submit"
          disabled={orchdDown || createTitle.trim() === "" || submitting}
          onClick={() => void submitCreate()}
          style={{ flexShrink: 0 }}
        >
          {strings.tasks.addTask}
        </Button>
      </div>

      {STATUS_VALUES.map((status) => {
        const group = groups.get(status) ?? [];
        return (
          <div key={status} data-testid={`task-status-group-${status}`} style={sectionStyle}>
            <div style={sectionHeaderStyle}>
              {STATUS_LABEL[status]} ({group.length})
            </div>
            {group.length === 0 ? (
              <div
                data-testid={`task-empty-group-${status}`}
                style={{ color: "var(--muted)", fontSize: "var(--fs-sm)", padding: "0 var(--sp-2)" }}
              >
                {strings.tasks.empty}
              </div>
            ) : (
              group.map((task, idx) => (
                <TaskRow
                  key={task.id}
                  task={task}
                  canMoveUp={idx > 0}
                  canMoveDown={idx < group.length - 1}
                  disabled={orchdDown}
                  onStatusChange={(id, s) => void handleStatusChange(id, s)}
                  onMoveUp={(t) => void handleMoveUp(group, t)}
                  onMoveDown={(t) => void handleMoveDown(group, t)}
                  onDelete={(t) => void handleDelete(t)}
                />
              ))
            )}
          </div>
        );
      })}
    </div>
  );
}
