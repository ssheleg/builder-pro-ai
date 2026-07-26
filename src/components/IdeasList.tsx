import { useEffect, useState, type CSSProperties, type JSX } from "react";
import { useAppStore } from "../store/store";
import {
  orchdCreateIdea,
  orchdUpdateIdea,
  orchdSetIdeaProject,
  orchdSetIdeaLifecycle,
  orchdDeleteIdea,
  describeOrchdError,
} from "../ipc/orchd";
import type { Idea, IdeaLifecycle, ResearchRun, ResearchStatus } from "../ipc/orchd-types";
import { useSubmitGuard } from "../hooks/useSubmitGuard";
import { ResearchRunDialog } from "./idea/ResearchRunDialog";
import { ResearchPane } from "./idea/ResearchPane";
import { SpawnProjectFromIdea } from "./idea/SpawnProjectFromIdea";
import { Badge, Button, EmptyState } from "../ui/primitives";
import { strings } from "../strings";

/** Locked enum order (spec §4.2 `IdeaLifecycle`) — the lifecycle chip cycles exactly these six
 * values, never re-orders or filters them (design-system.md "Lifecycle chip" atom). */
const LIFECYCLE_VALUES: IdeaLifecycle[] = [
  "captured",
  "researching",
  "specced",
  "inDev",
  "shipped",
  "archived",
];

const LIFECYCLE_LABEL: Record<IdeaLifecycle, string> = {
  captured: strings.ideas.lifecycle.captured,
  researching: strings.ideas.lifecycle.researching,
  specced: strings.ideas.lifecycle.specced,
  inDev: strings.ideas.lifecycle.inDev,
  shipped: strings.ideas.lifecycle.shipped,
  archived: strings.ideas.lifecycle.archived,
};

/** Mirrors `ResearchPane`'s identical label map (S-IDEA §7) — each component keeps its own copy,
 * matching this codebase's established per-component-label convention (no shared labels module,
 * see e.g. `LIFECYCLE_LABEL`/`FIT_VERDICT_LABEL` precedents). */
const RESEARCH_STATUS_LABEL: Record<ResearchStatus, string> = {
  pending: strings.research.runStatus.pending,
  running: strings.research.runStatus.running,
  done: strings.research.runStatus.done,
  failed: strings.research.runStatus.failed,
};

const listStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: "var(--sp-2)",
};

const rowStyle: CSSProperties = {
  display: "flex",
  flexWrap: "wrap",
  alignItems: "center",
  gap: "var(--sp-2)",
  padding: "var(--sp-2) var(--sp-3)",
  fontFamily: "var(--font-ui)",
  fontSize: "var(--fs-sm)",
  borderRadius: "var(--r-md)",
  background: "var(--panel)",
};

const titleInputStyle: CSSProperties = {
  flex: "1 1 160px",
  minWidth: 0,
  fontFamily: "var(--font-ui)",
  fontSize: "var(--fs-sm)",
  color: "var(--ink)",
  background: "transparent",
  border: "1px solid transparent",
  borderRadius: "var(--r-sm)",
  padding: "var(--sp-1) var(--sp-2)",
};

const bodyInputStyle: CSSProperties = {
  flex: "1 1 100%",
  minWidth: 0,
  fontFamily: "var(--font-ui)",
  fontSize: "var(--fs-sm)",
  color: "var(--muted)",
  background: "transparent",
  border: "1px solid transparent",
  borderRadius: "var(--r-sm)",
  padding: "var(--sp-1) var(--sp-2)",
  resize: "vertical",
};

const selectStyle: CSSProperties = {
  fontFamily: "var(--font-ui)",
  fontSize: "var(--fs-xs)",
  color: "var(--ink)",
  background: "var(--panel-2)",
  border: "none",
  borderRadius: 999,
  padding: "var(--sp-1) var(--sp-2)",
  flexShrink: 0,
};

const orphanRowStyle: CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: "var(--sp-2)",
  flex: "1 1 100%",
};

const createFormStyle: CSSProperties = {
  display: "flex",
  flexWrap: "wrap",
  gap: "var(--sp-2)",
  padding: "var(--sp-2) var(--sp-3)",
  border: "1px dashed var(--hairline)",
  borderRadius: "var(--r-md)",
};

/** UX-1 first-fetch placeholder — the same dim muted register as GoalTree's `loadingTextStyle`
 * (and DocsPanel's `docs-loading` row before it), so a still-loading inbox never reads as a
 * genuinely empty one. */
const loadingTextStyle: CSSProperties = {
  color: "var(--muted)",
  fontSize: "var(--fs-md)",
};

interface IdeaRowProps {
  idea: Idea;
  isOrphan: boolean;
  projects: { id: string; name: string }[];
  /** `orchdDown` (spec §10): while `true`, every mutating control on this row is disabled — see
   * `IdeasList`'s own doc comment. */
  disabled: boolean;
  /** This idea's research runs, newest-first (S-IDEA §7, T6) — `IdeasList` derives it from the
   * store's `researchRunsByIdea` map and passes it down as a plain prop (never re-selected here
   * via `useAppStore`, which would risk the same fresh-array-per-render infinite-loop pitfall
   * `ResearchPane` guards against — see that component's own doc comment). */
  researchRuns: ResearchRun[];
  /** Resolves `true` on a committed edit, `false` on a rejected one — the row reverts the local
   * draft to the store's value on `false` (mirrors `GoalTree`'s `GoalRow.commit`, P-27). */
  onTitleCommit: (id: string, title: string) => Promise<boolean>;
  onBodyCommit: (id: string, body: string) => Promise<boolean>;
  onLifecycleChange: (id: string, lifecycle: IdeaLifecycle) => void;
  onDelete: (id: string) => void;
  onAttach: (id: string, projectId: string) => void;
}

/** One idea row (design-system.md "Lifecycle chip" atom). Title/body edit state is local
 * (in-flight, not-yet-committed) exactly like `GoalTree`'s `GoalRow` — a rejected mutation reverts
 * to the store's copy rather than lying about what was saved.
 *
 * S-IDEA §7 (T6) additions: the "Research" button opens `ResearchRunDialog`; a toggle
 * reveals/hides the per-idea `ResearchPane`; a research-run status badge shows the LATEST (index
 * 0 — the backend returns runs newest-first) run's status, omitted entirely when there are no
 * runs yet (absence, not a placeholder dash, per this codebase's "absence means not yet
 * meaningful" convention). An orphan row (no project) additionally renders
 * `SpawnProjectFromIdea`'s own self-contained button. Honest degradation (T8 discipline): the
 * "Research" button is `disabled={disabled}` — mirrors every other mutating control on this row;
 * the pane-visibility toggle is a pure view toggle (no wrapper call of its own), so it stays
 * enabled even while the daemon is down.
 */
function IdeaRow(props: IdeaRowProps): JSX.Element {
  const {
    idea,
    isOrphan,
    projects,
    disabled,
    researchRuns,
    onTitleCommit,
    onBodyCommit,
    onLifecycleChange,
    onDelete,
    onAttach,
  } = props;

  const [title, setTitle] = useState(idea.title);
  const [body, setBody] = useState(idea.body);
  const [attachTo, setAttachTo] = useState("");
  const [researchDialogOpen, setResearchDialogOpen] = useState(false);
  const [researchExpanded, setResearchExpanded] = useState(false);

  useEffect(() => {
    setTitle(idea.title);
  }, [idea.title]);
  useEffect(() => {
    setBody(idea.body);
  }, [idea.body]);

  async function commitTitle(): Promise<void> {
    const trimmed = title.trim();
    if (trimmed === "" || trimmed === idea.title) {
      setTitle(idea.title); // blank/unchanged -> silent revert, never a malformed empty-title save
      return;
    }
    // A rejected save must REVERT the local draft to the store's value (P-27) — otherwise the stale
    // edit hangs on screen and never self-heals (the `useEffect([idea.title])` above only fires when
    // the store value CHANGES, which a failed save does not). Mirrors `GoalTree`'s `GoalRow.commit`.
    const ok = await onTitleCommit(idea.id, trimmed);
    if (!ok) setTitle(idea.title);
  }

  async function commitBody(): Promise<void> {
    if (body === idea.body) return;
    const ok = await onBodyCommit(idea.id, body);
    if (!ok) setBody(idea.body);
  }

  const latestRun = researchRuns[0] ?? null;

  return (
    <>
    <div data-testid={`idea-row-${idea.id}`} style={rowStyle}>
      <input
        data-testid={`idea-title-input-${idea.id}`}
        aria-label={strings.ideas.titleAria}
        value={title}
        disabled={disabled}
        onChange={(e) => setTitle(e.target.value)}
        onBlur={() => void commitTitle()}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            (e.target as HTMLInputElement).blur();
          }
        }}
        style={titleInputStyle}
      />
      <select
        data-testid={`idea-lifecycle-${idea.id}`}
        aria-label={strings.ideas.stageAria}
        value={idea.lifecycle}
        disabled={disabled}
        onChange={(e) => onLifecycleChange(idea.id, e.target.value as IdeaLifecycle)}
        style={selectStyle}
      >
        {LIFECYCLE_VALUES.map((v) => (
          <option key={v} value={v}>
            {LIFECYCLE_LABEL[v]}
          </option>
        ))}
      </select>
      <Button
        variant="danger"
        size="sm"
        type="button"
        data-testid={`idea-delete-${idea.id}`}
        disabled={disabled}
        onClick={() => onDelete(idea.id)}
      >
        {strings.common.delete}
      </Button>
      <Button
        variant="ghost"
        size="sm"
        type="button"
        data-testid={`idea-research-${idea.id}`}
        disabled={disabled}
        onClick={() => setResearchDialogOpen(true)}
      >
        {strings.ideas.research}
      </Button>
      {latestRun && (
        <Badge status={latestRun.status} data-testid={`idea-research-badge-${idea.id}`}>
          {RESEARCH_STATUS_LABEL[latestRun.status]}
        </Badge>
      )}
      <Button
        variant="ghost"
        size="sm"
        type="button"
        data-testid={`idea-research-toggle-${idea.id}`}
        onClick={() => setResearchExpanded((v) => !v)}
      >
        {researchExpanded ? strings.ideas.hideResearch : strings.ideas.researchCount(researchRuns.length)}
      </Button>
      {isOrphan && <SpawnProjectFromIdea idea={idea} />}
      <textarea
        data-testid={`idea-body-input-${idea.id}`}
        aria-label={strings.ideas.descriptionAria}
        value={body}
        disabled={disabled}
        onChange={(e) => setBody(e.target.value)}
        onBlur={() => void commitBody()}
        rows={2}
        style={bodyInputStyle}
      />
      {isOrphan && (
        <div style={orphanRowStyle}>
          <select
            data-testid={`idea-attach-select-${idea.id}`}
            aria-label={strings.ideas.linkToProjectAria}
            value={attachTo}
            onChange={(e) => setAttachTo(e.target.value)}
            style={selectStyle}
          >
            <option value="">{strings.ideas.selectProject}</option>
            {projects.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
          </select>
          <Button
            variant="ghost"
            size="sm"
            type="button"
            data-testid={`idea-attach-button-${idea.id}`}
            disabled={disabled || attachTo === ""}
            onClick={() => onAttach(idea.id, attachTo)}
          >
            {strings.ideas.linkToProject}
          </Button>
        </div>
      )}
    </div>
    {researchExpanded && <ResearchPane idea={idea} disabled={disabled} />}
    {researchDialogOpen && (
      <ResearchRunDialog idea={idea} onClose={() => setResearchDialogOpen(false)} />
    )}
    </>
  );
}

/**
 * Ideas inbox (S3 spec §10). `projectId === null` addresses the orphan bucket (`Idea.projectId`
 * is nullable — D3/D11); every row shown in that view IS an orphan, so the "link to project"
 * affordance renders unconditionally there. A concrete `projectId` filters to that project's own
 * ideas, matching `ideas.filter(i => i.projectId === projectId)` for both cases uniformly.
 *
 * Structural mutations (create/delete/attach — anything that changes which rows belong in THIS
 * view) explicitly `refreshIdeas()` after a successful round-trip, mirroring `GoalTree`'s split
 * between structural and field-level mutations; a lifecycle/title/body edit relies on the shared
 * `orchd://ideas-changed` → `refreshIdeas` pipe wired in App.tsx. Every mutating call is wrapped in
 * try/catch → `showToast(describeOrchdError(e))` (spec §7 honest error surface).
 *
 * Honest degradation (spec §10): while the store's `orchdDown` is `true`, every per-row mutating
 * control (title/body input, lifecycle select, Delete, "link to project") and the create
 * form's submit button are disabled — reads (the rows themselves) stay live. `ProjectPanel` owns
 * the shared banner; this component only owns disabling its own controls.
 */
export function IdeasList(props: { projectId: string | null }): JSX.Element {
  const { projectId } = props;

  const ideas = useAppStore((s) => s.ideas);
  const ideasFetched = useAppStore((s) => s.ideasFetched);
  const projects = useAppStore((s) => s.projects);
  const refreshIdeas = useAppStore((s) => s.refreshIdeas);
  const showToast = useAppStore((s) => s.showToast);
  const orchdDown = useAppStore((s) => s.orchdDown);
  // Select the STABLE outer map, then derive each row's array as a plain expression below —
  // never `useAppStore((s) => s.researchRunsByIdea[id] ?? [])` per row, which would return a
  // brand-new `[]` literal every render and infinite-loop `useSyncExternalStore` (see
  // `ResearchPane`'s identical doc comment on this exact pitfall).
  const researchRunsByIdea = useAppStore((s) => s.researchRunsByIdea);
  const refreshResearchRuns = useAppStore((s) => s.refreshResearchRuns);
  const { submitting, guard } = useSubmitGuard();

  const [createTitle, setCreateTitle] = useState("");
  const [createBody, setCreateBody] = useState("");

  const rows = ideas
    .filter((i) => i.projectId === projectId)
    .sort((a, b) => b.createdAt - a.createdAt);

  const isOrphanView = projectId === null;

  // Only ACTIVE projects are offered as idea-attach targets (UX-2): an archived project is
  // read-only history, so attaching a fresh idea to it would be a dead end. Mirrors
  // QuickCapture's `activeProjects` filter and WorkspaceSidebar's (spec D7).
  const activeProjects = projects.filter((p) => p.status === "active");

  // Eagerly populate `researchRunsByIdea` for every idea rendered here (S-IDEA §7, T6) — mirrors
  // `ProjectPanel`'s own-mount-fetch role for `IdeasList`/`InsightsList` (that component's doc
  // comment: those two have no mount-fetch of their own, they only populate via a push or the
  // parent's explicit refresh) and `ToolsBrowser`'s "if not already cached, fetch" per-row guard —
  // without this, the research-run badge/pane would silently stay empty until SOME push happened
  // to fire for an idea nobody had opened `ResearchPane` for yet.
  const rowIds = rows.map((i) => i.id).join(",");
  useEffect(() => {
    for (const idea of rows) {
      if (!(idea.id in researchRunsByIdea)) void refreshResearchRuns(idea.id);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [rowIds]);

  async function handleTitleCommit(id: string, title: string): Promise<boolean> {
    try {
      await orchdUpdateIdea(id, title, null);
      return true;
    } catch (e) {
      showToast(describeOrchdError(e));
      return false;
    }
  }

  async function handleBodyCommit(id: string, body: string): Promise<boolean> {
    try {
      await orchdUpdateIdea(id, null, body);
      return true;
    } catch (e) {
      showToast(describeOrchdError(e));
      return false;
    }
  }

  async function handleLifecycleChange(id: string, lifecycle: IdeaLifecycle): Promise<void> {
    try {
      await orchdSetIdeaLifecycle(id, lifecycle);
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

  async function handleDelete(id: string): Promise<void> {
    if (!window.confirm(strings.ideas.deleteConfirm)) return;
    try {
      await orchdDeleteIdea(id);
      await refreshIdeas();
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

  async function handleAttach(id: string, chosenProjectId: string): Promise<void> {
    try {
      await orchdSetIdeaProject(id, chosenProjectId);
      await refreshIdeas();
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

  async function handleCreate(): Promise<void> {
    const title = createTitle.trim();
    if (title === "") return;
    try {
      await orchdCreateIdea(projectId, title, createBody);
      setCreateTitle("");
      setCreateBody("");
      await refreshIdeas();
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

  // Double-submit guard (spec D6): a rapid second "+ idea" click must NOT create a duplicate idea
  // (finding E-08).
  const submitCreate = guard(handleCreate);

  return (
    <div data-testid="ideas-list" style={listStyle}>
      <div style={createFormStyle}>
        <input
          data-testid="idea-create-title"
          aria-label={strings.ideas.newTitleAria}
          placeholder={strings.ideas.newTitlePlaceholder}
          value={createTitle}
          onChange={(e) => setCreateTitle(e.target.value)}
          style={titleInputStyle}
        />
        <textarea
          data-testid="idea-create-body"
          aria-label={strings.ideas.newDescriptionAria}
          placeholder={strings.common.descriptionOptional}
          value={createBody}
          onChange={(e) => setCreateBody(e.target.value)}
          rows={2}
          style={bodyInputStyle}
        />
        <Button
          variant="primary"
          size="sm"
          type="button"
          data-testid="idea-create-submit"
          disabled={orchdDown || createTitle.trim() === "" || submitting}
          onClick={() => void submitCreate()}
        >
          {strings.ideas.addIdea}
        </Button>
      </div>

      {rows.length === 0 ? (
        // UX-1: an empty cache means "no ideas" only once the FIRST fetch has settled
        // (`ideasFetched` flips on success AND failure and never resets) — before that it just
        // means "not loaded yet", so render the loading placeholder instead of flashing the false
        // empty state at a user who HAS ideas (the GoalTree/DocsPanel loading-vs-empty split).
        // After a FAILED first fetch the empty state is the honest copy, which is exactly what
        // the never-reset flag yields.
        ideasFetched !== true ? (
          <div data-testid="ideas-list-loading" style={loadingTextStyle}>
            {strings.ideas.loading}
          </div>
        ) : (
          <EmptyState
            data-testid="ideas-list-empty"
            title={isOrphanView ? strings.ideas.emptyOrphan : strings.ideas.emptyProject}
          />
        )
      ) : (
        rows.map((idea) => (
          <IdeaRow
            key={idea.id}
            idea={idea}
            isOrphan={isOrphanView}
            projects={activeProjects}
            disabled={orchdDown}
            researchRuns={researchRunsByIdea[idea.id] ?? []}
            onTitleCommit={handleTitleCommit}
            onBodyCommit={handleBodyCommit}
            onLifecycleChange={(id, lifecycle) => void handleLifecycleChange(id, lifecycle)}
            onDelete={(id) => void handleDelete(id)}
            onAttach={(id, chosenProjectId) => void handleAttach(id, chosenProjectId)}
          />
        ))
      )}
    </div>
  );
}
