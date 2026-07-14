import { useEffect, useState, type CSSProperties, type JSX } from "react";
import { useAppStore } from "../store/store";
import {
  orchdAddProjectWorkspace,
  orchdRemoveProjectWorkspace,
  orchdExportProject,
  orchdExportToFile,
  orchdImportFromFile,
  describeOrchdError,
} from "../ipc/orchd";
import { pickFolder } from "../ipc/commands";
import type { WorkspaceId } from "../ipc/commands";
import { listDir, type FsEntry } from "../ipc/fs";
import { GoalTree } from "./GoalTree";
import { IdeasList } from "./IdeasList";
import { TasksList } from "./TasksList";
import { InsightsList } from "./InsightsList";
import { RulesetPanel } from "./RulesetPanel";
import { theme } from "../theme";

const MONO_FONT = 'ui-monospace, SFMono-Regular, "SF Mono", Menlo, monospace';

type TabKey = "overview" | "goals" | "ideas" | "tasks" | "insights" | "rules";

const TABS: { key: TabKey; label: string }[] = [
  { key: "overview", label: "Обзор" },
  { key: "goals", label: "Цели" },
  { key: "ideas", label: "Идеи" },
  { key: "tasks", label: "Задачи" },
  { key: "insights", label: "Инсайты" },
  { key: "rules", label: "Правила" },
];

const panelStyle: CSSProperties = {
  flex: 1,
  minWidth: 0,
  display: "flex",
  flexDirection: "column",
  overflow: "hidden",
  color: theme.colors.text,
};

const headerStyle: CSSProperties = {
  padding: "10px 16px",
  borderBottom: `1px solid ${theme.colors.border}`,
};

const tabBarStyle: CSSProperties = {
  display: "flex",
  gap: 4,
  padding: "6px 16px",
  borderBottom: `1px solid ${theme.colors.border}`,
};

const contentStyle: CSSProperties = {
  flex: 1,
  minHeight: 0,
  overflowY: "auto",
  padding: 16,
};

const sectionLabelStyle: CSSProperties = {
  fontSize: 11,
  fontWeight: 600,
  color: theme.colors.textDim,
  textTransform: "uppercase",
  letterSpacing: "0.05em",
  marginBottom: 6,
};

const textButtonStyle: CSSProperties = {
  border: `1px solid ${theme.colors.border}`,
  background: "transparent",
  color: theme.colors.text,
  cursor: "pointer",
  fontSize: 12,
  borderRadius: 4,
  padding: "4px 8px",
};

const chipStyle: CSSProperties = {
  display: "inline-flex",
  alignItems: "center",
  fontFamily: MONO_FONT,
  fontSize: 11,
  padding: "2px 8px",
  borderRadius: 999,
  border: `1px solid ${theme.colors.statusExited}`,
  color: theme.colors.statusExited,
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

/**
 * Every workspace id linked to at least one project (spec §10) — mirrors
 * `WorkspaceSidebar.tsx`'s / `CreateProjectDialog.tsx`'s identical helper so the three surfaces
 * agree on what "unlinked" means.
 */
function linkedWorkspaceIds(projects: { workspaceIds: string[] }[]): Set<string> {
  return new Set(projects.flatMap((p) => p.workspaceIds));
}

/**
 * Project workspace/detail panel (S3 spec §10, task-18). Six tabs; ONE is mounted at a time
 * (unmounting the others, not just hiding them — cheapest way to keep each T14-T17 component's own
 * mount-fetch effect honest about "did I already load this project's data").
 *
 * Обзор eagerly refreshes goals/tasks (project-scoped) and ideas/insights (whole-store, client-
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
 * «workspace недоступен» chip instead of silently dropping the row. The add-workspace `<select>`
 * only ever lists workspaces linked to NO project (the `linkedWorkspaceIds` complement).
 *
 * Every mutation (detach/attach/export/import) is wrapped in try/catch -> `showToast
 * (describeOrchdError(e))` (spec §7 honest error surface) and, where it changes `project.
 * workspaceIds`, explicitly `refreshProjects()`s afterward rather than waiting on the
 * `orchd://projects-changed` push (same defensive-refresh discipline as `GoalTree`/`TasksList`'s
 * own mutations).
 */
export function ProjectPanel(props: { projectId: string }): JSX.Element {
  const { projectId } = props;

  const projects = useAppStore((s) => s.projects);
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
      <div data-testid="project-panel-loading" style={{ ...panelStyle, padding: 16, color: theme.colors.textDim, fontSize: 13 }}>
        Загрузка проекта…
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
      showToast("JSON скопирован");
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

  async function handleExportToFile(): Promise<void> {
    try {
      const dir = await pickFolder();
      if (dir === null) return; // cancelled -> no-op
      await orchdExportToFile(projectId, dir);
      showToast("Экспортировано в файл");
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
      showToast(
        `Импортировано: проекты ${report.projects}, цели ${report.goals}, идеи ${report.ideas}, ` +
          `инсайты ${report.insights}, задачи ${report.tasks}`,
      );
      setImportDir(null);
      setImportFiles([]);
      await refreshProjects();
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

  return (
    <div data-testid="project-panel" style={panelStyle}>
      <div style={headerStyle}>
        <div style={{ fontSize: 16, fontWeight: 700 }}>{project.name}</div>
        {project.description !== "" && (
          <div style={{ fontSize: 12, color: theme.colors.textDim, marginTop: 2 }}>{project.description}</div>
        )}
      </div>

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
              padding: "6px 10px",
              fontSize: 13,
              border: "none",
              borderBottom: activeTab === t.key ? `2px solid ${theme.colors.accent}` : "2px solid transparent",
              background: "transparent",
              color: activeTab === t.key ? theme.colors.text : theme.colors.textDim,
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
            <div
              data-testid="project-counters"
              style={{ display: "flex", gap: 16, fontSize: 13, color: theme.colors.textDim }}
            >
              <span data-testid="project-counter-goals">Цели: {goalsCount}</span>
              <span data-testid="project-counter-ideas">Идеи: {ideasCount}</span>
              <span data-testid="project-counter-tasks">Задачи: {tasksCount}</span>
              <span data-testid="project-counter-insights">Инсайты: {insightsCount}</span>
            </div>

            <div>
              <div style={sectionLabelStyle}>Workspaces</div>
              <ul
                data-testid="project-workspaces"
                style={{ listStyle: "none", margin: 0, padding: 0, display: "flex", flexDirection: "column", gap: 6 }}
              >
                {project.workspaceIds.map((wsId) => {
                  const ws = workspaces[wsId];
                  return (
                    <li key={wsId} style={{ display: "flex", alignItems: "center", gap: 8 }}>
                      {ws ? (
                        <span style={{ fontSize: 13 }}>{ws.name}</span>
                      ) : (
                        <span data-testid={`project-workspace-unresolved-${wsId}`} style={chipStyle}>
                          workspace недоступен
                        </span>
                      )}
                      <button
                        type="button"
                        data-testid={`project-workspace-detach-${wsId}`}
                        onClick={() => void handleDetachWorkspace(wsId)}
                        style={textButtonStyle}
                      >
                        Отвязать
                      </button>
                    </li>
                  );
                })}
              </ul>
              {unlinkedWorkspaces.length > 0 && (
                <select
                  data-testid="project-add-workspace-select"
                  aria-label="Добавить workspace"
                  value={addWorkspaceSelection}
                  onChange={(e) => {
                    const wsId = e.target.value;
                    setAddWorkspaceSelection(wsId);
                    void handleAddWorkspace(wsId);
                  }}
                  style={{ ...selectStyle, marginTop: 8 }}
                >
                  <option value="">+ добавить workspace…</option>
                  {unlinkedWorkspaces.map((w) => (
                    <option key={w.id} value={w.id}>
                      {w.name}
                    </option>
                  ))}
                </select>
              )}
            </div>

            <div>
              <div style={sectionLabelStyle}>Экспорт</div>
              <div style={{ display: "flex", gap: 8 }}>
                <button type="button" data-testid="project-export-copy" onClick={() => void handleCopyJson()} style={textButtonStyle}>
                  Скопировать JSON
                </button>
                <button
                  type="button"
                  data-testid="project-export-file"
                  onClick={() => void handleExportToFile()}
                  style={textButtonStyle}
                >
                  Сохранить в файл…
                </button>
              </div>
            </div>

            <div>
              <div style={sectionLabelStyle}>Импорт</div>
              <button
                type="button"
                data-testid="project-import-browse"
                onClick={() => void handleBrowseImport()}
                style={textButtonStyle}
              >
                Импорт из файла…
              </button>
              {importDir !== null && (
                <div data-testid="project-import-files" style={{ marginTop: 8, display: "flex", flexDirection: "column", gap: 4 }}>
                  {importFiles.length === 0 ? (
                    <span style={{ fontSize: 12, color: theme.colors.textDim }}>Нет .json файлов в выбранной папке</span>
                  ) : (
                    importFiles.map((f) => (
                      <button
                        key={f.relPath}
                        type="button"
                        data-testid={`project-import-file-${f.name}`}
                        onClick={() => void handleImportFile(f)}
                        style={{ ...textButtonStyle, alignSelf: "flex-start" }}
                      >
                        {f.name}
                      </button>
                    ))
                  )}
                </div>
              )}
            </div>
          </div>
        )}

        {activeTab === "goals" && <GoalTree projectId={projectId} />}
        {activeTab === "ideas" && <IdeasList projectId={projectId} />}
        {activeTab === "tasks" && <TasksList projectId={projectId} />}
        {activeTab === "insights" && <InsightsList projectId={projectId} />}
        {activeTab === "rules" && <RulesetPanel scope="project" projectId={projectId} />}
      </div>
    </div>
  );
}
