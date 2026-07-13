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
import type { Idea, IdeaLifecycle } from "../ipc/orchd-types";
import { theme } from "../theme";

const MONO_FONT = 'ui-monospace, SFMono-Regular, "SF Mono", Menlo, monospace';

/** Confirm copy for idea delete (mirrors GoalTree's `DELETE_CONFIRM_TEXT` honesty pattern). */
const DELETE_CONFIRM_TEXT = "удалить идею?";

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
  captured: "зафиксирована",
  researching: "в исследовании",
  specced: "специфицирована",
  inDev: "в разработке",
  shipped: "выпущена",
  archived: "архив",
};

const listStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: 8,
};

const rowStyle: CSSProperties = {
  display: "flex",
  flexWrap: "wrap",
  alignItems: "center",
  gap: 6,
  padding: "8px 12px",
  fontFamily: MONO_FONT,
  fontSize: 12,
  border: `1px solid ${theme.colors.border}`,
  borderRadius: 8,
  background: theme.colors.bgElevated,
};

const titleInputStyle: CSSProperties = {
  flex: "1 1 160px",
  minWidth: 0,
  fontFamily: MONO_FONT,
  fontSize: 12,
  color: theme.colors.text,
  background: "transparent",
  border: "1px solid transparent",
  borderRadius: 4,
  padding: "3px 6px",
};

const bodyInputStyle: CSSProperties = {
  flex: "1 1 100%",
  minWidth: 0,
  fontFamily: MONO_FONT,
  fontSize: 12,
  color: theme.colors.textDim,
  background: "transparent",
  border: "1px solid transparent",
  borderRadius: 4,
  padding: "3px 6px",
  resize: "vertical",
};

const selectStyle: CSSProperties = {
  fontFamily: MONO_FONT,
  fontSize: 11,
  color: theme.colors.text,
  background: theme.colors.bg,
  border: `1px solid ${theme.colors.border}`,
  borderRadius: 999,
  padding: "2px 8px",
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

const primaryButtonStyle: CSSProperties = {
  ...textButtonStyle,
  color: theme.colors.bg,
  background: theme.colors.accent,
  borderColor: theme.colors.accent,
};

const orphanRowStyle: CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: 6,
  flex: "1 1 100%",
};

const createFormStyle: CSSProperties = {
  display: "flex",
  flexWrap: "wrap",
  gap: 6,
  padding: "8px 12px",
  border: `1px dashed ${theme.colors.border}`,
  borderRadius: 8,
};

interface IdeaRowProps {
  idea: Idea;
  isOrphan: boolean;
  projects: { id: string; name: string }[];
  onTitleCommit: (id: string, title: string) => Promise<void>;
  onBodyCommit: (id: string, body: string) => Promise<void>;
  onLifecycleChange: (id: string, lifecycle: IdeaLifecycle) => void;
  onDelete: (id: string) => void;
  onAttach: (id: string, projectId: string) => void;
}

/** One idea row (design-system.md "Lifecycle chip" atom). Title/body edit state is local
 * (in-flight, not-yet-committed) exactly like `GoalTree`'s `GoalRow` — a rejected mutation reverts
 * to the store's copy rather than lying about what was saved. */
function IdeaRow(props: IdeaRowProps): JSX.Element {
  const { idea, isOrphan, projects, onTitleCommit, onBodyCommit, onLifecycleChange, onDelete, onAttach } =
    props;

  const [title, setTitle] = useState(idea.title);
  const [body, setBody] = useState(idea.body);
  const [attachTo, setAttachTo] = useState("");

  useEffect(() => {
    setTitle(idea.title);
  }, [idea.title]);
  useEffect(() => {
    setBody(idea.body);
  }, [idea.body]);

  async function commitTitle(): Promise<void> {
    const trimmed = title.trim();
    if (trimmed === "" || trimmed === idea.title) {
      setTitle(idea.title);
      return;
    }
    await onTitleCommit(idea.id, trimmed);
  }

  async function commitBody(): Promise<void> {
    if (body === idea.body) return;
    await onBodyCommit(idea.id, body);
  }

  return (
    <div data-testid={`idea-row-${idea.id}`} style={rowStyle}>
      <input
        data-testid={`idea-title-input-${idea.id}`}
        aria-label="Название идеи"
        value={title}
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
        aria-label="Стадия идеи"
        value={idea.lifecycle}
        onChange={(e) => onLifecycleChange(idea.id, e.target.value as IdeaLifecycle)}
        style={selectStyle}
      >
        {LIFECYCLE_VALUES.map((v) => (
          <option key={v} value={v}>
            {LIFECYCLE_LABEL[v]}
          </option>
        ))}
      </select>
      <button
        type="button"
        data-testid={`idea-delete-${idea.id}`}
        onClick={() => onDelete(idea.id)}
        style={deleteButtonStyle}
      >
        Удалить
      </button>
      <textarea
        data-testid={`idea-body-input-${idea.id}`}
        aria-label="Описание идеи"
        value={body}
        onChange={(e) => setBody(e.target.value)}
        onBlur={() => void commitBody()}
        rows={2}
        style={bodyInputStyle}
      />
      {isOrphan && (
        <div style={orphanRowStyle}>
          <select
            data-testid={`idea-attach-select-${idea.id}`}
            aria-label="Привязать к проекту"
            value={attachTo}
            onChange={(e) => setAttachTo(e.target.value)}
            style={selectStyle}
          >
            <option value="">выбрать проект…</option>
            {projects.map((p) => (
              <option key={p.id} value={p.id}>
                {p.name}
              </option>
            ))}
          </select>
          <button
            type="button"
            data-testid={`idea-attach-button-${idea.id}`}
            disabled={attachTo === ""}
            onClick={() => onAttach(idea.id, attachTo)}
            style={{ ...textButtonStyle, opacity: attachTo === "" ? 0.5 : 1 }}
          >
            привязать к проекту
          </button>
        </div>
      )}
    </div>
  );
}

/**
 * Ideas inbox (S3 spec §10). `projectId === null` addresses the orphan bucket (`Idea.projectId`
 * is nullable — D3/D11); every row shown in that view IS an orphan, so the «привязать к проекту»
 * affordance renders unconditionally there. A concrete `projectId` filters to that project's own
 * ideas, matching `ideas.filter(i => i.projectId === projectId)` for both cases uniformly.
 *
 * Structural mutations (create/delete/attach — anything that changes which rows belong in THIS
 * view) explicitly `refreshIdeas()` after a successful round-trip, mirroring `GoalTree`'s split
 * between structural and field-level mutations; a lifecycle/title/body edit relies on the shared
 * `orchd://ideas-changed` → `refreshIdeas` pipe wired in App.tsx. Every mutating call is wrapped in
 * try/catch → `showToast(describeOrchdError(e))` (spec §7 honest error surface).
 */
export function IdeasList(props: { projectId: string | null }): JSX.Element {
  const { projectId } = props;

  const ideas = useAppStore((s) => s.ideas);
  const projects = useAppStore((s) => s.projects);
  const refreshIdeas = useAppStore((s) => s.refreshIdeas);
  const showToast = useAppStore((s) => s.showToast);

  const [createTitle, setCreateTitle] = useState("");
  const [createBody, setCreateBody] = useState("");

  const rows = ideas
    .filter((i) => i.projectId === projectId)
    .sort((a, b) => b.createdAt - a.createdAt);

  const isOrphanView = projectId === null;

  async function handleTitleCommit(id: string, title: string): Promise<void> {
    try {
      await orchdUpdateIdea(id, title, null);
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

  async function handleBodyCommit(id: string, body: string): Promise<void> {
    try {
      await orchdUpdateIdea(id, null, body);
    } catch (e) {
      showToast(describeOrchdError(e));
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
    if (!window.confirm(DELETE_CONFIRM_TEXT)) return;
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

  return (
    <div data-testid="ideas-list" style={listStyle}>
      <div style={createFormStyle}>
        <input
          data-testid="idea-create-title"
          aria-label="Название новой идеи"
          placeholder="название идеи"
          value={createTitle}
          onChange={(e) => setCreateTitle(e.target.value)}
          style={titleInputStyle}
        />
        <textarea
          data-testid="idea-create-body"
          aria-label="Описание новой идеи"
          placeholder="описание (необязательно)"
          value={createBody}
          onChange={(e) => setCreateBody(e.target.value)}
          rows={2}
          style={bodyInputStyle}
        />
        <button
          type="button"
          data-testid="idea-create-submit"
          disabled={createTitle.trim() === ""}
          onClick={() => void handleCreate()}
          style={{ ...primaryButtonStyle, opacity: createTitle.trim() === "" ? 0.5 : 1 }}
        >
          + идея
        </button>
      </div>

      {rows.length === 0 ? (
        <div
          data-testid="ideas-list-empty"
          style={{ color: theme.colors.textDim, fontSize: 13 }}
        >
          {isOrphanView ? "Нет идей без проекта." : "В этом проекте пока нет идей."}
        </div>
      ) : (
        rows.map((idea) => (
          <IdeaRow
            key={idea.id}
            idea={idea}
            isOrphan={isOrphanView}
            projects={projects}
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
