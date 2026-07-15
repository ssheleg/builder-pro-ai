import { useEffect, useRef, useState, type CSSProperties, type JSX } from "react";
import { useAppStore } from "../../store/store";
import {
  orchdCreateInsight,
  orchdSetInsightFitVerdict,
  orchdSetInsightStatus,
  orchdCreateTask,
  orchdSetIdeaLifecycle,
  orchdGraphNeighborhood,
  describeOrchdError,
} from "../../ipc/orchd";
import type { FitVerdict, GraphNeighborhood, Idea, Insight, McpArtifact } from "../../ipc/orchd-types";
import { theme } from "../../theme";

const FIT_VERDICT_VALUES: FitVerdict[] = ["fit", "noFit", "unknown"];

const FIT_VERDICT_LABEL: Record<FitVerdict, string> = {
  fit: "подходит",
  noFit: "не подходит",
  unknown: "неясно",
};

const overlayStyle: CSSProperties = {
  position: "fixed",
  inset: 0,
  background: "rgba(1, 4, 9, 0.6)",
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
  zIndex: 1000,
};

const cardStyle: CSSProperties = {
  width: 720,
  maxWidth: "95vw",
  maxHeight: "85vh",
  overflowY: "auto",
  background: theme.colors.bgElevated,
  border: `1px solid ${theme.colors.border}`,
  borderRadius: 10,
  boxShadow: theme.shadow,
  padding: 16,
  display: "flex",
  flexDirection: "column",
  gap: 12,
};

const titleStyle: CSSProperties = {
  fontSize: 15,
  fontWeight: 600,
  color: theme.colors.text,
};

const bodyLayoutStyle: CSSProperties = {
  display: "flex",
  gap: 16,
  flexWrap: "wrap",
};

const formColumnStyle: CSSProperties = {
  flex: "1 1 320px",
  display: "flex",
  flexDirection: "column",
  gap: 12,
  minWidth: 0,
};

const fitContextColumnStyle: CSSProperties = {
  flex: "1 1 240px",
  minWidth: 0,
  display: "flex",
  flexDirection: "column",
  gap: 10,
  fontSize: 12,
  color: theme.colors.textDim,
  border: `1px solid ${theme.colors.border}`,
  borderRadius: 8,
  padding: 10,
  background: theme.colors.bg,
};

const fitContextTitleStyle: CSSProperties = {
  fontSize: 11,
  fontWeight: 600,
  color: theme.colors.textDim,
  textTransform: "uppercase",
  letterSpacing: "0.05em",
};

const fieldLabelStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: 4,
  fontSize: 12,
  fontWeight: 600,
  color: theme.colors.textDim,
  textTransform: "uppercase",
  letterSpacing: "0.05em",
};

const inputStyle: CSSProperties = {
  fontFamily: "inherit",
  fontSize: 13,
  fontWeight: 400,
  textTransform: "none",
  letterSpacing: "normal",
  color: theme.colors.text,
  background: theme.colors.bg,
  border: `1px solid ${theme.colors.border}`,
  borderRadius: 6,
  padding: "6px 8px",
};

const textareaStyle: CSSProperties = {
  ...inputStyle,
  resize: "vertical",
  minHeight: 90,
};

const selectStyle: CSSProperties = {
  ...inputStyle,
};

const goalRowStyle: CSSProperties = {
  fontSize: 12,
};

const secondaryButtonStyle: CSSProperties = {
  padding: "6px 12px",
  borderRadius: 6,
  border: `1px solid ${theme.colors.border}`,
  background: "transparent",
  color: theme.colors.text,
  fontSize: 13,
  cursor: "pointer",
};

const primaryButtonStyle: CSSProperties = {
  padding: "6px 12px",
  borderRadius: 6,
  border: "none",
  background: theme.colors.accent,
  color: theme.colors.text,
  fontSize: 13,
  fontWeight: 600,
  cursor: "pointer",
};

const inlineErrorStyle: CSSProperties = {
  fontSize: 13,
  lineHeight: 1.5,
  color: theme.colors.statusExited,
  borderLeft: `3px solid ${theme.colors.statusExited}`,
  paddingLeft: 8,
};

const statusLineStyle: CSSProperties = {
  fontSize: 12,
  color: theme.colors.textDim,
};

/**
 * «Сформировать insight» dialog (S-IDEA spec §7). Reached either from a `done` research run's
 * «Сформировать insight» (`artifact` set — title stays the idea's own, body prefills from the
 * artifact's flattened content, owner-editable) or the `failed`-run degraded path (Q8,
 * `artifact: null` — the owner types the body by hand). `source` is ALWAYS `research-run:<runId>`
 * regardless of which path reached this dialog — a failed run still has an id, it just has no
 * artifact (spec §7 bullet 3).
 *
 * Fit-context side panel (owner-facing, informational only — never auto-applies anything): the
 * project's goals (with `metricRefs`) plus the idea's `GraphNeighborhood`, when the idea already
 * has a graph node (`entityType:"idea", entityId:idea.id` — most ideas won't, since graph-ingest
 * only happens on insight-ACCEPT, spec D9; an honest "нет данных графа" is shown otherwise). An
 * orphan idea (`projectId: null`) has no project to pull goals from and nowhere to look for a
 * graph node, so the whole panel degrades to a single honest note instead of guessing.
 *
 * Flow (owner-driven throughout — `fitVerdict`/`fitReasoning` are the OWNER's judgment call, never
 * inferred): «Создать» -> `orchdCreateInsight` then `orchdSetInsightFitVerdict` (spec §7 bullet
 * 3); once created, «Принять» -> `orchdSetInsightStatus(accepted)`; once accepted, «В backlog» ->
 * `orchdCreateTask{source:"insight"}` then `orchdSetIdeaLifecycle(idea.id, "specced")` and closes
 * (spec §7 bullet 5/6). «В backlog» is additionally blocked for an orphan idea — `orchdCreateTask`
 * requires a concrete `projectId`, so there is nowhere to file the task.
 *
 * Dialog-atom parity with `CreateProjectDialog`/`ResearchRunDialog`: overlay + centered card,
 * `role="dialog"`, an in-dialog `role="alert"` failure line, stays open on failure.
 *
 * Honest degradation (spec §10, T8 discipline): every mutating button (Создать/Принять/В backlog)
 * is independently `disabled={orchdDown}`.
 */
export function FormInsightDialog(props: {
  idea: Idea;
  runId: string;
  artifact: McpArtifact | null;
  onClose: () => void;
}): JSX.Element {
  const { idea, runId, artifact, onClose } = props;

  const goalsByProject = useAppStore((s) => s.goalsByProject);
  const graphByProject = useAppStore((s) => s.graphByProject);
  const refreshGoals = useAppStore((s) => s.refreshGoals);
  const refreshGraph = useAppStore((s) => s.refreshGraph);
  const refreshInsights = useAppStore((s) => s.refreshInsights);
  const refreshIdeas = useAppStore((s) => s.refreshIdeas);
  const refreshTasks = useAppStore((s) => s.refreshTasks);
  const orchdDown = useAppStore((s) => s.orchdDown);
  const showToast = useAppStore((s) => s.showToast);

  const [title, setTitle] = useState(idea.title);
  const [body, setBody] = useState(artifact !== null ? (artifact.contentText ?? artifact.contentJson) : "");
  const [fitVerdict, setFitVerdict] = useState<FitVerdict | "">("");
  const [fitReasoning, setFitReasoning] = useState("");
  const [insight, setInsight] = useState<Insight | null>(null);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [neighborhood, setNeighborhood] = useState<GraphNeighborhood | null>(null);

  const titleRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    titleRef.current?.focus();
    const onKeyDown = (e: KeyboardEvent): void => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [onClose]);

  useEffect(() => {
    if (idea.projectId === null) return;
    void refreshGoals(idea.projectId);
    void refreshGraph(idea.projectId);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [idea.projectId]);

  const goals = idea.projectId !== null ? (goalsByProject[idea.projectId] ?? []) : [];
  const ideaNode =
    idea.projectId !== null
      ? (graphByProject[idea.projectId]?.nodes.find(
          (n) => n.entityType === "idea" && n.entityId === idea.id,
        ) ?? null)
      : null;

  useEffect(() => {
    if (ideaNode === null) {
      setNeighborhood(null);
      return;
    }
    let cancelled = false;
    void orchdGraphNeighborhood(ideaNode.id, 1).then((n) => {
      if (!cancelled) setNeighborhood(n);
    });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ideaNode?.id]);

  async function handleCreate(): Promise<void> {
    if (title.trim() === "") return;
    setErrorMessage(null);
    try {
      const created = await orchdCreateInsight(idea.projectId, `research-run:${runId}`, title.trim(), body);
      const verdict = fitVerdict === "" ? null : fitVerdict;
      const updated = await orchdSetInsightFitVerdict(created.id, verdict, fitReasoning);
      setInsight(updated);
      await refreshInsights();
      showToast("Инсайт создан");
    } catch (e) {
      const message = describeOrchdError(e);
      setErrorMessage(message);
      showToast(message);
    }
  }

  async function handleAccept(): Promise<void> {
    if (insight === null) return;
    setErrorMessage(null);
    try {
      const updated = await orchdSetInsightStatus(insight.id, "accepted", null);
      setInsight(updated);
      await refreshInsights();
    } catch (e) {
      const message = describeOrchdError(e);
      setErrorMessage(message);
      showToast(message);
    }
  }

  async function handleBacklog(): Promise<void> {
    if (insight === null || idea.projectId === null) return;
    setErrorMessage(null);
    try {
      await orchdCreateTask(
        idea.projectId,
        null,
        insight.title,
        insight.body,
        null,
        "insight",
        insight.id,
        [],
      );
      await orchdSetIdeaLifecycle(idea.id, "specced");
      await refreshTasks(idea.projectId);
      await refreshIdeas();
      showToast("Задача добавлена в backlog");
      onClose();
    } catch (e) {
      const message = describeOrchdError(e);
      setErrorMessage(message);
      showToast(message);
    }
  }

  const createBlocked = orchdDown || title.trim() === "" || insight !== null;
  const acceptBlocked = orchdDown || insight === null || insight.status !== "new";
  const backlogBlocked =
    orchdDown || insight === null || insight.status !== "accepted" || idea.projectId === null;

  return (
    <div style={overlayStyle}>
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="form-insight-title-heading"
        data-testid="form-insight-dialog"
        style={cardStyle}
      >
        <div id="form-insight-title-heading" style={titleStyle}>
          Сформировать insight
        </div>

        <div style={bodyLayoutStyle}>
          <div style={formColumnStyle}>
            <label style={fieldLabelStyle}>
              Название
              <input
                ref={titleRef}
                data-testid="form-insight-title"
                aria-label="Название инсайта"
                value={title}
                disabled={insight !== null}
                onChange={(e) => setTitle(e.target.value)}
                style={inputStyle}
              />
            </label>

            <label style={fieldLabelStyle}>
              Описание
              <textarea
                data-testid="form-insight-body"
                aria-label="Описание инсайта"
                value={body}
                disabled={insight !== null}
                onChange={(e) => setBody(e.target.value)}
                style={textareaStyle}
              />
            </label>

            <label style={fieldLabelStyle}>
              Вердикт владельца
              <select
                data-testid="form-insight-verdict"
                aria-label="Вердикт владельца"
                value={fitVerdict}
                disabled={insight !== null}
                onChange={(e) => setFitVerdict(e.target.value as FitVerdict | "")}
                style={selectStyle}
              >
                <option value="">— без вердикта —</option>
                {FIT_VERDICT_VALUES.map((v) => (
                  <option key={v} value={v}>
                    {FIT_VERDICT_LABEL[v]}
                  </option>
                ))}
              </select>
            </label>

            <label style={fieldLabelStyle}>
              Обоснование
              <input
                data-testid="form-insight-reasoning"
                aria-label="Обоснование вердикта"
                value={fitReasoning}
                disabled={insight !== null}
                onChange={(e) => setFitReasoning(e.target.value)}
                style={inputStyle}
              />
            </label>

            {insight !== null && (
              <div data-testid="form-insight-status" style={statusLineStyle}>
                статус инсайта: {insight.status}
              </div>
            )}
          </div>

          <div style={fitContextColumnStyle}>
            <div style={fitContextTitleStyle}>Контекст для оценки</div>
            {idea.projectId === null ? (
              <div data-testid="form-insight-no-project">
                идея не привязана к проекту — контекст недоступен
              </div>
            ) : (
              <>
                <div>
                  <div style={fitContextTitleStyle}>Цели проекта</div>
                  {goals.length === 0 ? (
                    <div>целей пока нет</div>
                  ) : (
                    goals.map((g) => (
                      <div key={g.id} data-testid={`form-insight-goal-${g.id}`} style={goalRowStyle}>
                        {g.title}
                        {g.metricRefs.length > 0 && ` — метрики: ${g.metricRefs.join(", ")}`}
                      </div>
                    ))
                  )}
                </div>
                <div>
                  <div style={fitContextTitleStyle}>Связанный граф</div>
                  {ideaNode === null ? (
                    <div data-testid="form-insight-neighborhood-empty">
                      нет узла графа для этой идеи ещё
                    </div>
                  ) : neighborhood === null || neighborhood.nodes.length === 0 ? (
                    <div data-testid="form-insight-neighborhood-empty">нет связанных узлов</div>
                  ) : (
                    neighborhood.nodes
                      .filter((n) => n.id !== ideaNode.id)
                      .map((n) => (
                        <div key={n.id} data-testid={`form-insight-neighborhood-node-${n.id}`}>
                          {n.label}
                        </div>
                      ))
                  )}
                </div>
              </>
            )}
          </div>
        </div>

        {errorMessage !== null && (
          <div data-testid="form-insight-error" role="alert" style={inlineErrorStyle}>
            {errorMessage}
          </div>
        )}

        <div style={{ display: "flex", justifyContent: "flex-end", gap: 8, marginTop: 4 }}>
          <button
            type="button"
            data-testid="form-insight-cancel"
            onClick={onClose}
            style={secondaryButtonStyle}
          >
            Отмена
          </button>
          <button
            type="button"
            data-testid="form-insight-create"
            disabled={createBlocked}
            onClick={() => void handleCreate()}
            style={{ ...primaryButtonStyle, opacity: createBlocked ? 0.5 : 1 }}
          >
            Создать
          </button>
          {insight !== null && (
            <button
              type="button"
              data-testid="form-insight-accept"
              disabled={acceptBlocked}
              onClick={() => void handleAccept()}
              style={{ ...primaryButtonStyle, opacity: acceptBlocked ? 0.5 : 1 }}
            >
              Принять
            </button>
          )}
          {insight !== null && insight.status === "accepted" && (
            <button
              type="button"
              data-testid="form-insight-backlog"
              disabled={backlogBlocked}
              onClick={() => void handleBacklog()}
              style={{ ...primaryButtonStyle, opacity: backlogBlocked ? 0.5 : 1 }}
            >
              В backlog
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
