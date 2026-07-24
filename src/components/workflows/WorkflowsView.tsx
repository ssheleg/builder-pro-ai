import { useEffect, useMemo, useState, type CSSProperties, type JSX } from "react";
import { useAppStore } from "../../store/store";
import { describeOrchdError } from "../../ipc/orchd";
import type { SupervisorConfig, Workflow } from "../../ipc/orchd-types";
import { Badge, Button, EmptyState, SegmentedPill } from "../../ui/primitives";
import { strings } from "../../strings";
import { WorkflowEditor, type WorkflowDraft } from "./WorkflowEditor";
import { RunWorkflowPicker } from "./RunWorkflowPicker";

/** The library's scope filter (SCR-01): a pure client-side view over `store.workflows` — the list
 * already carries full definitions of BOTH scopes (`WorkflowList` returns them). */
type ScopeFilter = "all" | "global" | "project";

/** The disabled/empty CEO config every fresh workflow carries (mirrors the ruleset policy's
 * `SupervisorConfig` default). */
function blankSupervisor(): SupervisorConfig {
  return { enabled: false, delegatedClasses: [], instruction: "", customRules: [] };
}

/** A brand-new blank draft (the "+ New workflow" seed): empty id ⇒ create, global scope, the first
 * known agent as the default, no stages/skills, a disabled CEO. */
function blankDraft(): WorkflowDraft {
  return {
    id: "",
    name: "",
    description: "",
    scope: "global",
    projectId: null,
    defaultAgent: "claude-code",
    stages: [],
    globalSkillIds: [],
    supervisor: blankSupervisor(),
  };
}

/** Draft for OPENING an existing workflow — every field copied, `id` kept so Save updates in
 * place. `stages`/`globalSkillIds`/`supervisor` are deep-copied so the editor's local edits never
 * mutate the store's row before a Save round-trips. */
function draftFromWorkflow(w: Workflow): WorkflowDraft {
  return {
    id: w.id,
    name: w.name,
    description: w.description,
    scope: w.scope,
    projectId: w.projectId,
    defaultAgent: w.defaultAgent,
    stages: w.stages.map((s) => ({ ...s, skillIds: [...s.skillIds], outputs: [...s.outputs] })),
    globalSkillIds: [...w.globalSkillIds],
    supervisor: {
      enabled: w.supervisor.enabled,
      delegatedClasses: [...w.supervisor.delegatedClasses],
      instruction: w.supervisor.instruction,
      customRules: [...w.supervisor.customRules],
    },
  };
}

/** Draft for DUPLICATING a workflow — same as opening but with an EMPTY id (⇒ create a new row)
 * and a "(copy)" name, so Save mints a fresh workflow instead of overwriting the source. */
function draftFromDuplicate(w: Workflow): WorkflowDraft {
  const base = draftFromWorkflow(w);
  return { ...base, id: "", name: strings.workflows.library.duplicateSuffix(w.name) };
}

const headerRowStyle: CSSProperties = {
  display: "flex",
  alignItems: "center",
  justifyContent: "space-between",
  gap: "var(--sp-3)",
  flexWrap: "wrap",
  marginBottom: "var(--sp-4)",
};

const rowStyle: CSSProperties = {
  display: "flex",
  alignItems: "flex-start",
  gap: "var(--sp-3)",
  padding: "var(--sp-3) var(--sp-4)",
  background: "var(--panel)",
  borderRadius: "var(--r-md)",
  marginBottom: "var(--sp-2)",
};

const metaRowStyle: CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: "var(--sp-2)",
  flexWrap: "wrap",
  marginTop: "var(--sp-1)",
};

/**
 * Workflows library (SCR-01, SW1). Lists every reusable workflow-as-data definition with a scope
 * filter (All | Global | Project), per-row actions (Run → / Open / Duplicate / Delete) and a
 * "+ New workflow" primary. Opening/duplicating/creating swaps this list for the `WorkflowEditor`;
 * "Run →" opens the `RunWorkflowPicker` — the honest S6b trigger stub that fabricates no execution.
 *
 * Invalidation-driven (mirrors every other domain surface): refreshes `store.workflows` on mount;
 * the `orchd://workflows-changed` push (App.tsx) keeps it live thereafter. Delete confirms via
 * `window.confirm` and surfaces a rejection as a toast (`describeOrchdError`), never a silent
 * no-op; the mutating "+ New workflow"/Run/Open/Duplicate/Delete controls are disabled while
 * `orchdDown` (honest degradation) — Open/Duplicate stay live only in that they open a local editor
 * whose own Save is gated.
 */
export function WorkflowsView(): JSX.Element {
  const workflows = useAppStore((s) => s.workflows);
  const projects = useAppStore((s) => s.projects);
  const activeProjectId = useAppStore((s) => s.activeProjectId);
  const refreshWorkflows = useAppStore((s) => s.refreshWorkflows);
  const deleteWorkflow = useAppStore((s) => s.deleteWorkflow);
  const showToast = useAppStore((s) => s.showToast);
  const orchdDown = useAppStore((s) => s.orchdDown);

  const [scopeFilter, setScopeFilter] = useState<ScopeFilter>("all");
  const [editing, setEditing] = useState<WorkflowDraft | null>(null);
  const [runOpen, setRunOpen] = useState(false);

  useEffect(() => {
    void refreshWorkflows();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const shown = useMemo(() => {
    if (scopeFilter === "all") return workflows;
    return workflows.filter((w) => w.scope === scopeFilter);
  }, [workflows, scopeFilter]);

  // The project the run trigger names (SCR-04 "Run workflow on {project}"): the active project if
  // one is open, else the first active project, else a neutral label — the run slice is a stub, so
  // this only titles the modal (it never targets an execution).
  const runProjectName = useMemo(() => {
    const active = projects.find((p) => p.id === activeProjectId);
    if (active) return active.name;
    const firstActive = projects.find((p) => p.status === "active");
    return firstActive?.name ?? strings.workflows.run.fallbackProject;
  }, [projects, activeProjectId]);

  if (editing !== null) {
    return <WorkflowEditor draft={editing} onDone={() => setEditing(null)} />;
  }

  async function handleDelete(w: Workflow): Promise<void> {
    const name = w.name.trim() === "" ? strings.workflows.library.untitled : w.name;
    if (!window.confirm(strings.workflows.library.deleteConfirm(name))) return;
    try {
      await deleteWorkflow(w.id);
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

  const scopeOptions: readonly { value: ScopeFilter; label: string }[] = [
    { value: "all", label: strings.workflows.library.scopeAll },
    { value: "global", label: strings.workflows.library.scopeGlobal },
    { value: "project", label: strings.workflows.library.scopeProject },
  ];

  return (
    <div
      data-testid="workflows-view"
      style={{ flex: 1, minWidth: 0, overflowY: "auto", padding: "var(--sp-5)" }}
    >
      <div style={headerRowStyle}>
        <div style={{ display: "flex", alignItems: "center", gap: "var(--sp-3)" }}>
          <h1 style={{ fontSize: "var(--fs-xl)", fontWeight: 700, color: "var(--ink)", margin: 0 }}>
            {strings.workflows.title}
          </h1>
          <SegmentedPill
            options={scopeOptions}
            value={scopeFilter}
            onChange={setScopeFilter}
            ariaLabel={strings.workflows.library.scopeAria}
            data-testid="workflows-scope-filter"
          />
        </div>
        <Button
          type="button"
          variant="primary"
          size="sm"
          data-testid="workflows-new"
          disabled={orchdDown}
          onClick={() => setEditing(blankDraft())}
        >
          {strings.workflows.library.newWorkflow}
        </Button>
      </div>

      {shown.length === 0 ? (
        <EmptyState
          data-testid="workflows-empty"
          title={strings.workflows.library.emptyTitle}
          hint={strings.workflows.library.emptyHint}
          action={
            <Button
              type="button"
              variant="primary"
              size="sm"
              data-testid="workflows-empty-new"
              disabled={orchdDown}
              onClick={() => setEditing(blankDraft())}
            >
              {strings.workflows.library.newWorkflow}
            </Button>
          }
        />
      ) : (
        <div data-testid="workflows-list">
          {shown.map((w) => {
            const name = w.name.trim() === "" ? strings.workflows.library.untitled : w.name;
            const description =
              w.description.trim() === "" ? strings.workflows.library.noDescription : w.description;
            return (
              <div key={w.id} data-testid={`workflow-row-${w.id}`} style={rowStyle}>
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div style={{ fontSize: "var(--fs-md)", fontWeight: 600, color: "var(--ink)" }}>
                    {name}
                  </div>
                  <div
                    style={{
                      fontSize: "var(--fs-sm)",
                      color: "var(--muted)",
                      marginTop: 2,
                      lineHeight: 1.4,
                    }}
                  >
                    {description}
                  </div>
                  <div style={metaRowStyle}>
                    <Badge
                      tone={w.scope === "global" ? "info" : "accent"}
                      data-testid={`workflow-scope-${w.id}`}
                    >
                      {w.scope === "global"
                        ? strings.workflows.library.scopeBadgeGlobal
                        : strings.workflows.library.scopeBadgeProject}
                    </Badge>
                    <span style={{ fontSize: "var(--fs-xs)", color: "var(--muted)" }}>
                      {strings.workflows.library.stagesCount(w.stages.length)}
                    </span>
                    <span style={{ fontSize: "var(--fs-xs)", color: "var(--muted)" }}>
                      {strings.workflows.library.skillsCount(w.globalSkillIds.length)}
                    </span>
                  </div>
                </div>
                <div style={{ display: "flex", gap: "var(--sp-2)", flexShrink: 0, flexWrap: "wrap" }}>
                  <Button
                    type="button"
                    variant="primary"
                    size="sm"
                    data-testid={`workflow-run-${w.id}`}
                    disabled={orchdDown}
                    onClick={() => setRunOpen(true)}
                  >
                    {strings.workflows.library.run}
                  </Button>
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    data-testid={`workflow-open-${w.id}`}
                    onClick={() => setEditing(draftFromWorkflow(w))}
                  >
                    {strings.workflows.library.open}
                  </Button>
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    data-testid={`workflow-duplicate-${w.id}`}
                    disabled={orchdDown}
                    onClick={() => setEditing(draftFromDuplicate(w))}
                  >
                    {strings.workflows.library.duplicate}
                  </Button>
                  <Button
                    type="button"
                    variant="danger"
                    size="sm"
                    data-testid={`workflow-delete-${w.id}`}
                    disabled={orchdDown}
                    onClick={() => void handleDelete(w)}
                  >
                    {strings.workflows.library.delete}
                  </Button>
                </div>
              </div>
            );
          })}
        </div>
      )}

      <RunWorkflowPicker
        open={runOpen}
        onClose={() => setRunOpen(false)}
        projectName={runProjectName}
      />
    </div>
  );
}
