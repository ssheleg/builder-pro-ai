import { useEffect, useState, type CSSProperties, type JSX } from "react";
import { useAppStore } from "../store/store";
import {
  orchdAddProjectWorkspace,
  orchdRemoveProjectWorkspace,
  orchdArchiveProject,
  orchdUnarchiveProject,
  orchdExportProject,
  orchdExportToFile,
  orchdImportFromFile,
  describeOrchdError,
} from "../ipc/orchd";
import { pickFolder } from "../ipc/commands";
import type { WorkspaceId } from "../ipc/commands";
import { listDir, type FsEntry } from "../ipc/fs";
import { useSubmitGuard } from "../hooks/useSubmitGuard";
import { GoalTree } from "./GoalTree";
import { IdeasList } from "./IdeasList";
import { TasksList } from "./TasksList";
import { InsightsList } from "./InsightsList";
import { RulesetPanel } from "./RulesetPanel";
import { OrchdDownBanner } from "./OrchdDownBanner";
import { GraphCanvas } from "./graph/GraphCanvas";
import { Badge, Button, Panel, Stat } from "../ui/primitives";
import { strings } from "../strings";

type TabKey = "overview" | "goals" | "ideas" | "tasks" | "insights" | "rules" | "graph";

const TABS: { key: TabKey; label: string }[] = [
  { key: "overview", label: strings.project.tabs.overview },
  { key: "goals", label: strings.project.tabs.goals },
  { key: "ideas", label: strings.project.tabs.ideas },
  { key: "tasks", label: strings.project.tabs.tasks },
  { key: "insights", label: strings.project.tabs.insights },
  { key: "rules", label: strings.project.tabs.rules },
  { key: "graph", label: strings.project.tabs.graph },
];

const panelStyle: CSSProperties = {
  flex: 1,
  minWidth: 0,
  display: "flex",
  flexDirection: "column",
  overflow: "hidden",
  color: "var(--ink)",
};

const headerStyle: CSSProperties = {
  padding: "var(--sp-3) var(--sp-4)",
  borderBottom: "1px solid var(--border)",
};

const tabBarStyle: CSSProperties = {
  display: "flex",
  gap: "var(--sp-1)",
  padding: "var(--sp-2) var(--sp-4)",
  borderBottom: "1px solid var(--border)",
};

const contentStyle: CSSProperties = {
  flex: 1,
  minHeight: 0,
  overflowY: "auto",
  padding: "var(--sp-4)",
};

const selectStyle: CSSProperties = {
  fontFamily: "var(--font-ui)",
  fontSize: "var(--fs-sm)",
  color: "var(--ink)",
  background: "var(--panel)",
  border: "1px solid var(--border-strong)",
  borderRadius: "var(--r-sm)",
  padding: "var(--sp-1) var(--sp-2)",
};

/**
 * Every workspace id linked to at least one project (spec §10) — mirrors
 * `WorkspaceSidebar.tsx`'s / `CreateProjectDialog.tsx`'s identical helper so the three surfaces
 * agree on what "unlinked" means.
 */
function linkedWorkspaceIds(projects: { workspaceIds: string[] }[]): Set<string> {
  return new Set(projects.flatMap((p) => p.workspaceIds));
}

/**
 * Project workspace/detail panel (S3 spec §10, task-18; S4 §7 T7 added the 7th "Graph" tab). Seven
 * tabs; ONE is mounted at a time
 * (unmounting the others, not just hiding them — cheapest way to keep each T14-T17 component's own
 * mount-fetch effect honest about "did I already load this project's data").
 *
 * Overview eagerly refreshes goals/tasks (project-scoped) and ideas/insights (whole-store, client-
 * filtered by every consumer) on mount — mirrors EXACTLY the set `App.tsx`'s `onOrchdUp` handler
 * refreshes "if a project panel is currently open" (see that handler's doc comment). Without this,
 * the entity counters below would silently read `0`/stale for ideas/insights (`IdeasList`/
 * `InsightsList` have no mount-fetch of their own — they only ever populate via a push or this
 * exact refresh set) — honest counters demand this panel drive the fetch itself, not merely read
 * whatever the store happens to already hold (design-system.md §1 "Honest state, always").
 *
 * Workspace management: `project.workspaceIds` entries are resolved to names via the sessiond
 * `workspaces` store slice (client-side soft-ref join, spec §10); an id that resolves to nothing
 * (the workspace was deleted/never existed — a soft ref, not a foreign key) renders the
 * "workspace unavailable" chip instead of silently dropping the row. The add-workspace `<select>`
 * only ever lists workspaces linked to NO project (the `linkedWorkspaceIds` complement).
 *
 * Every mutation (detach/attach/export/import) is wrapped in try/catch -> `showToast
 * (describeOrchdError(e))` (spec §7 honest error surface) and, where it changes `project.
 * workspaceIds`, explicitly `refreshProjects()`s afterward rather than waiting on the
 * `orchd://projects-changed` push (same defensive-refresh discipline as `GoalTree`/`TasksList`'s
 * own mutations).
 *
 * Honest degradation (spec §10): the shared `<OrchdDownBanner/>` renders above the tab bar
 * whenever the store's `orchdDown` is `true` — this is the panel-level half of "every domain
 * surface shows the banner"; each of the six tab bodies (`GoalTree`/`IdeasList`/`TasksList`/
 * `InsightsList`/`RulesetPanel`/`GraphCanvas`) independently disables its own mutating controls
 * off the same flag.
 */
export function ProjectPanel(props: { projectId: string }): JSX.Element {
  const { projectId } = props;

  const projects = useAppStore((s) => s.projects);
  const orchdDown = useAppStore((s) => s.orchdDown);
  const workspaces = useAppStore((s) => s.workspaces);
  const goalsByProject = useAppStore((s) => s.goalsByProject);
  const tasksByProject = useAppStore((s) => s.tasksByProject);
  const ideas = useAppStore((s) => s.ideas);
  const insights = useAppStore((s) => s.insights);
  const refreshProjects = useAppStore((s) => s.refreshProjects);
  const refreshGoals = useAppStore((s) => s.refreshGoals);
  const refreshTasks = useAppStore((s) => s.refreshTasks);
  const refreshIdeas = useAppStore((s) => s.refreshIdeas);
  const refreshInsights = useAppStore((s) => s.refreshInsights);
  const showToast = useAppStore((s) => s.showToast);

  const [activeTab, setActiveTab] = useState<TabKey>("overview");
  const [addWorkspaceSelection, setAddWorkspaceSelection] = useState("");
  const [importDir, setImportDir] = useState<string | null>(null);
  const [importFiles, setImportFiles] = useState<FsEntry[]>([]);
  const { submitting, guard } = useSubmitGuard();

  useEffect(() => {
    void refreshGoals(projectId);
    void refreshTasks(projectId);
    void refreshIdeas();
    void refreshInsights();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [projectId]);

  // Switching to a different project must never keep showing the previous project's import
  // file-picker state.
  useEffect(() => {
    setImportDir(null);
    setImportFiles([]);
    setAddWorkspaceSelection("");
  }, [projectId]);

  const project = projects.find((p) => p.id === projectId);

  if (!project) {
    return (
      <div data-testid="project-panel-loading" style={{ ...panelStyle, padding: "var(--sp-4)", color: "var(--muted)", fontSize: "var(--fs-md)" }}>
        {strings.project.loading}
      </div>
    );
  }

  const goalsCount = (goalsByProject[projectId] ?? []).length;
  const tasksCount = (tasksByProject[projectId] ?? []).length;
  const ideasCount = ideas.filter((i) => i.projectId === projectId).length;
  const insightsCount = insights.filter((i) => i.projectId === projectId).length;

  const linkedIds = linkedWorkspaceIds(projects);
  const unlinkedWorkspaces = Object.values(workspaces)
    .filter((w) => !linkedIds.has(w.id))
    .sort((a, b) => a.name.localeCompare(b.name));

  async function handleDetachWorkspace(wsId: WorkspaceId): Promise<void> {
    try {
      await orchdRemoveProjectWorkspace(projectId, wsId);
      await refreshProjects();
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

  async function handleAddWorkspace(wsId: WorkspaceId): Promise<void> {
    if (wsId === "") return;
    try {
      await orchdAddProjectWorkspace(projectId, wsId);
      await refreshProjects();
      setAddWorkspaceSelection("");
    } catch (e) {
      showToast(describeOrchdError(e));
      setAddWorkspaceSelection("");
    }
  }

  async function handleCopyJson(): Promise<void> {
    try {
      const json = await orchdExportProject(projectId);
      await navigator.clipboard.writeText(json);
      showToast(strings.project.jsonCopied);
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

  async function handleExportToFile(): Promise<void> {
    try {
      const dir = await pickFolder();
      if (dir === null) return; // cancelled -> no-op
      await orchdExportToFile(projectId, dir);
      showToast(strings.project.exportedToFile);
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

  async function handleBrowseImport(): Promise<void> {
    try {
      const dir = await pickFolder();
      if (dir === null) return; // cancelled -> no-op
      const entries = await listDir(dir, "", false);
      setImportDir(dir);
      setImportFiles(entries.filter((e) => !e.isDir && e.name.toLowerCase().endsWith(".json")));
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

  async function handleImportFile(entry: FsEntry): Promise<void> {
    if (importDir === null) return;
    try {
      const report = await orchdImportFromFile(`${importDir}/${entry.relPath}`);
      showToast(strings.project.importSummary(report));
      setImportDir(null);
      setImportFiles([]);
      await refreshProjects();
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

  // Archive / un-archive (O-3, spec D7). Both are mutating round-trips wrapped in the shared
  // double-submit `guard` (spec D6) and explicitly `refreshProjects()` afterward rather than
  // waiting on the `orchd://projects-changed` push (same defensive-refresh discipline as the
  // workspace mutations above). A cancelled confirm returns before the round-trip — not an error.
  async function handleArchive(): Promise<void> {
    if (!window.confirm(strings.project.archiveConfirm)) return;
    try {
      await orchdArchiveProject(projectId);
      await refreshProjects();
      showToast(strings.project.archived);
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

  async function handleUnarchive(): Promise<void> {
    try {
      await orchdUnarchiveProject(projectId);
      await refreshProjects();
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

  const archiveProject = guard(handleArchive);
  const unarchiveProject = guard(handleUnarchive);
  const isArchived = project.status === "archived";

  return (
    <div data-testid="project-panel" style={panelStyle}>
      <div style={headerStyle}>
        <div style={{ fontSize: "var(--fs-lg)", fontWeight: 700, color: "var(--ink)" }}>{project.name}</div>
        {project.description !== "" && (
          <div style={{ fontSize: "var(--fs-sm)", color: "var(--muted)", marginTop: 2 }}>{project.description}</div>
        )}
      </div>

      {orchdDown && <OrchdDownBanner />}

      <div role="tablist" style={tabBarStyle}>
        {TABS.map((t) => (
          <button
            key={t.key}
            type="button"
            role="tab"
            aria-selected={activeTab === t.key}
            data-testid={`project-tab-${t.key}`}
            onClick={() => setActiveTab(t.key)}
            style={{
              padding: "var(--sp-2) var(--sp-3)",
              fontSize: "var(--fs-md)",
              fontFamily: "var(--font-ui)",
              fontWeight: activeTab === t.key ? 600 : 400,
              border: "none",
              borderBottom: activeTab === t.key ? "2px solid var(--accent)" : "2px solid transparent",
              background: "transparent",
              color: activeTab === t.key ? "var(--ink)" : "var(--muted)",
              cursor: "pointer",
            }}
          >
            {t.label}
          </button>
        ))}
      </div>

      <div style={contentStyle}>
        {activeTab === "overview" && (
          <div data-testid="project-overview" style={{ display: "flex", flexDirection: "column", gap: 16 }}>
            {isArchived && (
              <div
                data-testid="project-archived-banner"
                role="status"
                style={{
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "space-between",
                  gap: "var(--sp-3)",
                  padding: "var(--sp-2) var(--sp-3)",
                  borderRadius: "var(--r-md)",
                  border: "1px solid var(--warn)",
                  background: "var(--warn-weak)",
                  color: "var(--warn)",
                  fontSize: "var(--fs-md)",
                }}
              >
                <span>{strings.project.archivedBanner}</span>
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  data-testid="project-unarchive"
                  onClick={() => void unarchiveProject()}
                  disabled={orchdDown || submitting}
                  style={{ whiteSpace: "nowrap" }}
                >
                  {strings.project.unarchive}
                </Button>
              </div>
            )}
            <div
              data-testid="project-counters"
              style={{
                display: "grid",
                gridTemplateColumns: "repeat(auto-fit, minmax(96px, 1fr))",
                gap: "var(--sp-3)",
              }}
            >
              <Stat data-testid="project-counter-goals" label={strings.project.tabs.goals} value={goalsCount} />
              <Stat data-testid="project-counter-ideas" label={strings.project.tabs.ideas} value={ideasCount} />
              <Stat data-testid="project-counter-tasks" label={strings.project.tabs.tasks} value={tasksCount} />
              <Stat
                data-testid="project-counter-insights"
                label={strings.project.tabs.insights}
                value={insightsCount}
              />
            </div>

            <Panel title="Workspaces">
              <ul
                data-testid="project-workspaces"
                style={{ listStyle: "none", margin: 0, padding: 0, display: "flex", flexDirection: "column", gap: "var(--sp-2)" }}
              >
                {project.workspaceIds.map((wsId) => {
                  const ws = workspaces[wsId];
                  return (
                    <li key={wsId} style={{ display: "flex", alignItems: "center", gap: "var(--sp-2)" }}>
                      {ws ? (
                        <span style={{ fontSize: "var(--fs-md)", color: "var(--ink)", flex: 1 }}>{ws.name}</span>
                      ) : (
                        <Badge data-testid={`project-workspace-unresolved-${wsId}`} tone="danger">
                          {strings.project.workspaceUnavailable}
                        </Badge>
                      )}
                      <Button
                        type="button"
                        variant="ghost"
                        size="sm"
                        data-testid={`project-workspace-detach-${wsId}`}
                        onClick={() => void handleDetachWorkspace(wsId)}
                        style={{ flexShrink: 0 }}
                      >
                        {strings.project.unlink}
                      </Button>
                    </li>
                  );
                })}
              </ul>
              {unlinkedWorkspaces.length > 0 && (
                <select
                  data-testid="project-add-workspace-select"
                  aria-label={strings.project.addWorkspaceAria}
                  value={addWorkspaceSelection}
                  onChange={(e) => {
                    const wsId = e.target.value;
                    setAddWorkspaceSelection(wsId);
                    void handleAddWorkspace(wsId);
                  }}
                  style={{ ...selectStyle, marginTop: "var(--sp-2)" }}
                >
                  <option value="">{strings.project.addWorkspaceOption}</option>
                  {unlinkedWorkspaces.map((w) => (
                    <option key={w.id} value={w.id}>
                      {w.name}
                    </option>
                  ))}
                </select>
              )}
            </Panel>

            <Panel title={strings.project.exportLabel}>
              <div style={{ display: "flex", gap: "var(--sp-2)" }}>
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  data-testid="project-export-copy"
                  onClick={() => void handleCopyJson()}
                >
                  {strings.project.copyJson}
                </Button>
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  data-testid="project-export-file"
                  onClick={() => void handleExportToFile()}
                >
                  {strings.project.saveToFile}
                </Button>
              </div>
            </Panel>

            <Panel title={strings.project.importLabel}>
              <Button
                type="button"
                variant="ghost"
                size="sm"
                data-testid="project-import-browse"
                onClick={() => void handleBrowseImport()}
              >
                {strings.project.importFromFile}
              </Button>
              {importDir !== null && (
                <div data-testid="project-import-files" style={{ marginTop: "var(--sp-2)", display: "flex", flexDirection: "column", gap: "var(--sp-1)" }}>
                  {importFiles.length === 0 ? (
                    <span style={{ fontSize: "var(--fs-sm)", color: "var(--muted)" }}>{strings.project.noJsonFiles}</span>
                  ) : (
                    importFiles.map((f) => (
                      <Button
                        key={f.relPath}
                        type="button"
                        variant="ghost"
                        size="sm"
                        data-testid={`project-import-file-${f.name}`}
                        onClick={() => void handleImportFile(f)}
                        style={{ alignSelf: "flex-start" }}
                      >
                        {f.name}
                      </Button>
                    ))
                  )}
                </div>
              )}
            </Panel>

            {!isArchived && (
              <Panel title={strings.project.dangerLabel}>
                <Button
                  type="button"
                  variant="danger"
                  size="sm"
                  data-testid="project-archive"
                  onClick={() => void archiveProject()}
                  disabled={orchdDown || submitting}
                >
                  {strings.project.archive}
                </Button>
              </Panel>
            )}
          </div>
        )}

        {activeTab === "goals" && <GoalTree projectId={projectId} />}
        {activeTab === "ideas" && <IdeasList projectId={projectId} />}
        {activeTab === "tasks" && <TasksList projectId={projectId} />}
        {activeTab === "insights" && <InsightsList projectId={projectId} />}
        {activeTab === "rules" && <RulesetPanel scope="project" projectId={projectId} />}
        {activeTab === "graph" && <GraphCanvas projectId={projectId} />}
      </div>
    </div>
  );
}
