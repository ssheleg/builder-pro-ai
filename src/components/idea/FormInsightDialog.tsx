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
import { useSubmitGuard } from "../../hooks/useSubmitGuard";
import { Button } from "../../ui/primitives";
import { strings } from "../../strings";

const FIT_VERDICT_VALUES: FitVerdict[] = ["fit", "noFit", "unknown"];

const FIT_VERDICT_LABEL: Record<FitVerdict, string> = {
  fit: strings.insights.fitVerdict.fit,
  noFit: strings.insights.fitVerdict.noFit,
  unknown: strings.insights.fitVerdict.unknown,
};

const overlayStyle: CSSProperties = {
  position: "fixed",
  inset: 0,
  background: "rgba(0, 0, 0, 0.4)",
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
  background: "var(--panel)",
  border: "1px solid var(--border)",
  borderRadius: "var(--r-lg)",
  boxShadow: "var(--shadow-1)",
  padding: "var(--sp-4)",
  display: "flex",
  flexDirection: "column",
  gap: "var(--sp-3)",
};

const titleStyle: CSSProperties = {
  fontSize: "var(--fs-lg)",
  fontWeight: 600,
  color: "var(--ink)",
};

const bodyLayoutStyle: CSSProperties = {
  display: "flex",
  gap: "var(--sp-4)",
  flexWrap: "wrap",
};

const formColumnStyle: CSSProperties = {
  flex: "1 1 320px",
  display: "flex",
  flexDirection: "column",
  gap: "var(--sp-3)",
  minWidth: 0,
};

const fitContextColumnStyle: CSSProperties = {
  flex: "1 1 240px",
  minWidth: 0,
  display: "flex",
  flexDirection: "column",
  gap: "var(--sp-2)",
  fontSize: "var(--fs-sm)",
  color: "var(--muted)",
  border: "1px solid var(--border)",
  borderRadius: "var(--r-md)",
  padding: "var(--sp-3)",
  background: "var(--panel-2)",
};

const fitContextTitleStyle: CSSProperties = {
  fontSize: "var(--fs-xs)",
  fontWeight: 600,
  color: "var(--muted)",
  textTransform: "uppercase",
  letterSpacing: "0.05em",
};

const fieldLabelStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: "var(--sp-1)",
  fontSize: "var(--fs-sm)",
  fontWeight: 600,
  color: "var(--muted)",
  textTransform: "uppercase",
  letterSpacing: "0.05em",
};

const inputStyle: CSSProperties = {
  fontFamily: "var(--font-ui)",
  fontSize: "var(--fs-md)",
  fontWeight: 400,
  textTransform: "none",
  letterSpacing: "normal",
  color: "var(--ink)",
  background: "var(--panel-2)",
  border: "1px solid var(--border-strong)",
  borderRadius: "var(--r-md)",
  padding: "var(--sp-2) var(--sp-3)",
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
  fontSize: "var(--fs-sm)",
};

const inlineErrorStyle: CSSProperties = {
  fontSize: "var(--fs-md)",
  lineHeight: 1.5,
  color: "var(--danger)",
  borderLeft: "3px solid var(--danger)",
  paddingLeft: "var(--sp-2)",
};

const statusLineStyle: CSSProperties = {
  fontSize: "var(--fs-sm)",
  color: "var(--muted)",
};

/**
 * "Form insight" dialog (S-IDEA spec §7). Reached either from a `done` research run's
 * "Form insight" (`artifact` set — title stays the idea's own, body prefills from the
 * artifact's flattened content, owner-editable) or the `failed`-run degraded path (Q8,
 * `artifact: null` — the owner types the body by hand). `source` is ALWAYS `research-run:<runId>`
 * regardless of which path reached this dialog — a failed run still has an id, it just has no
 * artifact (spec §7 bullet 3).
 *
 * Fit-context side panel (owner-facing, informational only — never auto-applies anything): the
 * project's goals (with `metricRefs`) plus the idea's `GraphNeighborhood`, when the idea already
 * has a graph node (`entityType:"idea", entityId:idea.id` — most ideas won't, since graph-ingest
 * only happens on insight-ACCEPT, spec D9; an honest "no graph data" is shown otherwise). An
 * orphan idea (`projectId: null`) has no project to pull goals from and nowhere to look for a
 * graph node, so the whole panel degrades to a single honest note instead of guessing.
 *
 * Flow (owner-driven throughout — `fitVerdict`/`fitReasoning` are the OWNER's judgment call, never
 * inferred): "Create" -> `orchdCreateInsight` then `orchdSetInsightFitVerdict` (spec §7 bullet
 * 3); once created, "Accept" -> `orchdSetInsightStatus(accepted)`; once accepted, "To backlog" ->
 * `orchdCreateTask{source:"insight"}` then `orchdSetIdeaLifecycle(idea.id, "specced")` and closes
 * (spec §7 bullet 5/6). "To backlog" is additionally blocked for an orphan idea — `orchdCreateTask`
 * requires a concrete `projectId`, so there is nowhere to file the task.
 *
 * Dialog-atom parity with `CreateProjectDialog`/`ResearchRunDialog`: overlay + centered card,
 * `role="dialog"`, an in-dialog `role="alert"` failure line, stays open on failure.
 *
 * Honest degradation (spec §10, T8 discipline): every mutating button (Create/Accept/To backlog)
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
  const { submitting, guard } = useSubmitGuard();

  const [title, setTitle] = useState(idea.title);
  const [body, setBody] = useState(artifact !== null ? (artifact.contentText ?? artifact.contentJson) : "");
  const [fitVerdict, setFitVerdict] = useState<FitVerdict | "">("");
  const [fitReasoning, setFitReasoning] = useState("");
  const [insight, setInsight] = useState<Insight | null>(null);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [neighborhood, setNeighborhood] = useState<GraphNeighborhood | null>(null);
  // Resume-from-failed-step state for the backlog chain (spec D6, BL-95/G-08): once the task is
  // created, its id is held so a retry after a failed lifecycle flip re-runs ONLY the flip — never
  // a duplicate `orchdCreateTask`.
  const [createdTaskId, setCreatedTaskId] = useState<string | null>(null);
  const [createdInsightId, setCreatedInsightId] = useState<string | null>(null);

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
    // Skip re-creating the insight on a resume (C3): once created, only the fit-verdict step is
    // retried, so a verdict-failure retry can never duplicate the insight (mirrors handleBacklog's
    // createdTaskId guard).
    let insightId = createdInsightId;
    try {
      if (insightId === null) {
        const created = await orchdCreateInsight(idea.projectId, `research-run:${runId}`, title.trim(), body);
        insightId = created.id;
        setCreatedInsightId(created.id);
      }
      const verdict = fitVerdict === "" ? null : fitVerdict;
      const updated = await orchdSetInsightFitVerdict(insightId, verdict, fitReasoning);
      setInsight(updated);
      await refreshInsights();
      showToast(strings.insights.form.created);
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
    const projectId = idea.projectId;
    // Skip re-creating the task on a resume (spec D6): once `createdTaskId` is set, only the
    // lifecycle flip is retried.
    let taskId = createdTaskId;
    try {
      if (taskId === null) {
        const task = await orchdCreateTask(
          projectId,
          null,
          insight.title,
          insight.body,
          null,
          "insight",
          insight.id,
          [],
        );
        taskId = task.id;
        setCreatedTaskId(task.id);
      }
      await orchdSetIdeaLifecycle(idea.id, "specced");
      await refreshTasks(projectId);
      await refreshIdeas();
      showToast(strings.insights.form.addedToBacklog);
      onClose();
    } catch (e) {
      const reason = describeOrchdError(e);
      // `taskId !== null` means the task was created (this attempt or a prior one) and it is the
      // lifecycle flip that failed — name what was created and let the retry resume from the flip
      // (no duplicate task). A createTask failure (`taskId` still null) surfaces the raw reason and
      // leaves `createdTaskId` null, so a retry safely re-creates.
      const message = taskId !== null ? strings.insights.form.backlogResume(reason) : reason;
      setErrorMessage(message);
      showToast(message);
    }
  }

  // Double-submit guards (spec D6): one shared guard for the dialog's three sequential stages —
  // a rapid double click on Create/Accept/To-backlog must fire each mutation at most once (finding
  // G-08 / P-19). The stages are mutually exclusive by insight status, so one shared `submitting`
  // is sufficient and never blocks a legitimately-different next stage.
  const submitCreate = guard(handleCreate);
  const submitAccept = guard(handleAccept);
  const submitBacklog = guard(handleBacklog);

  const createBlocked = orchdDown || title.trim() === "" || insight !== null || submitting;
  const acceptBlocked = orchdDown || insight === null || insight.status !== "new" || submitting;
  const backlogBlocked =
    orchdDown ||
    insight === null ||
    insight.status !== "accepted" ||
    idea.projectId === null ||
    submitting;

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
          {strings.insights.form.title}
        </div>

        <div style={bodyLayoutStyle}>
          <div style={formColumnStyle}>
            <label style={fieldLabelStyle}>
              {strings.insights.form.nameLabel}
              <input
                ref={titleRef}
                data-testid="form-insight-title"
                aria-label={strings.insights.form.nameAria}
                value={title}
                disabled={insight !== null}
                onChange={(e) => setTitle(e.target.value)}
                style={inputStyle}
              />
            </label>

            <label style={fieldLabelStyle}>
              {strings.insights.form.descriptionLabel}
              <textarea
                data-testid="form-insight-body"
                aria-label={strings.insights.form.descriptionAria}
                value={body}
                disabled={insight !== null}
                onChange={(e) => setBody(e.target.value)}
                style={textareaStyle}
              />
            </label>

            <label style={fieldLabelStyle}>
              {strings.insights.form.ownerVerdictLabel}
              <select
                data-testid="form-insight-verdict"
                aria-label={strings.insights.form.ownerVerdictAria}
                value={fitVerdict}
                disabled={insight !== null}
                onChange={(e) => setFitVerdict(e.target.value as FitVerdict | "")}
                style={selectStyle}
              >
                <option value="">{strings.common.noVerdict}</option>
                {FIT_VERDICT_VALUES.map((v) => (
                  <option key={v} value={v}>
                    {FIT_VERDICT_LABEL[v]}
                  </option>
                ))}
              </select>
            </label>

            <label style={fieldLabelStyle}>
              {strings.insights.form.reasoningLabel}
              <input
                data-testid="form-insight-reasoning"
                aria-label={strings.insights.form.reasoningAria}
                value={fitReasoning}
                disabled={insight !== null}
                onChange={(e) => setFitReasoning(e.target.value)}
                style={inputStyle}
              />
            </label>

            {insight !== null && (
              <div data-testid="form-insight-status" style={statusLineStyle}>
                {strings.insights.form.status(insight.status)}
              </div>
            )}
          </div>

          <div style={fitContextColumnStyle}>
            <div style={fitContextTitleStyle}>{strings.insights.form.contextTitle}</div>
            {idea.projectId === null ? (
              <div data-testid="form-insight-no-project">
                {strings.insights.form.ideaNotLinked}
              </div>
            ) : (
              <>
                <div>
                  <div style={fitContextTitleStyle}>{strings.insights.form.projectGoals}</div>
                  {goals.length === 0 ? (
                    <div>{strings.insights.form.noGoals}</div>
                  ) : (
                    goals.map((g) => (
                      <div key={g.id} data-testid={`form-insight-goal-${g.id}`} style={goalRowStyle}>
                        {g.title}
                        {g.metricRefs.length > 0 && strings.insights.form.metrics(g.metricRefs.join(", "))}
                      </div>
                    ))
                  )}
                </div>
                <div>
                  <div style={fitContextTitleStyle}>{strings.insights.form.relatedGraph}</div>
                  {ideaNode === null ? (
                    <div data-testid="form-insight-neighborhood-empty">
                      {strings.insights.form.noGraphNode}
                    </div>
                  ) : neighborhood === null || neighborhood.nodes.length === 0 ? (
                    <div data-testid="form-insight-neighborhood-empty">{strings.insights.form.noRelatedNodes}</div>
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

        <div style={{ display: "flex", justifyContent: "flex-end", gap: "var(--sp-2)", marginTop: "var(--sp-1)" }}>
          <Button
            variant="ghost"
            type="button"
            data-testid="form-insight-cancel"
            onClick={onClose}
          >
            {strings.common.cancel}
          </Button>
          <Button
            variant="primary"
            type="button"
            data-testid="form-insight-create"
            disabled={createBlocked}
            onClick={() => void submitCreate()}
          >
            {strings.common.create}
          </Button>
          {insight !== null && (
            <Button
              variant="primary"
              type="button"
              data-testid="form-insight-accept"
              disabled={acceptBlocked}
              onClick={() => void submitAccept()}
            >
              {strings.common.accept}
            </Button>
          )}
          {insight !== null && insight.status === "accepted" && (
            <Button
              variant="primary"
              type="button"
              data-testid="form-insight-backlog"
              disabled={backlogBlocked}
              onClick={() => void submitBacklog()}
            >
              {strings.insights.form.toBacklog}
            </Button>
          )}
        </div>
      </div>
    </div>
  );
}
