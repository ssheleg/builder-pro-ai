import { useState, type JSX } from "react";
import { useAppStore } from "../store/store";
import { pickFolder, createWorkspace } from "../ipc/commands";
import type { WorkspaceId } from "../ipc/commands";
import { orchdAddProjectWorkspace, describeOrchdError } from "../ipc/orchd";
import type { Project } from "../ipc/orchd-types";
import type { Workspace } from "../ipc/types";
import { CreateProjectDialog } from "./CreateProjectDialog";
import { DiagnosticsPanel } from "./DiagnosticsPanel";
import { ThemeToggle } from "../ui/ThemeToggle";
import { strings } from "../strings";

function basename(path: string): string {
  const parts = path.replace(/\/+$/, "").split("/");
  return parts[parts.length - 1] || path;
}

/**
 * Honest message for a rejected sessiond `CommandError` (`create_session`/`create_workspace` —
 * `src-tauri/src/commands.rs::CommandError`). Deliberately duplicated per-surface, mirroring
 * `FileTree.tsx`/`TerminalTabs.tsx` (the repo keeps one copy per component so each stays
 * independently deployable — same rationale as `describeFsError`/`FilePreview`). Vocabulary is
 * `strings.errors.command.*`.
 */
function describeCommandError(err: unknown): string {
  const e = err as { kind?: string; message?: string; code?: string; reason?: string } | undefined;
  switch (e?.kind) {
    case "daemon":
      return e.message ?? e.code ?? strings.errors.command.daemon;
    case "disconnected":
      return strings.errors.command.disconnected;
    case "internal":
      return e.message ?? strings.errors.command.internal;
    case "incompatibleDaemon":
      return strings.errors.command.incompatible;
    case "upgradeFailed":
      return e.reason ?? strings.errors.command.failed;
    case "tooLarge":
      return strings.errors.command.tooLarge;
    default:
      return err instanceof Error ? err.message : String(err);
  }
}

/** Every workspace id linked to at least one project (spec §10) — mirrors `ProjectPanel.tsx`'s /
 * `CreateProjectDialog.tsx`'s identical helper so the three surfaces agree on what "unlinked"
 * means. The complement is the «No project» group below. */
function linkedWorkspaceIds(projects: Project[]): Set<string> {
  return new Set(projects.flatMap((p) => p.workspaceIds));
}

/**
 * Left rail: pure navigation (spec §6.1 "slimmed to pure navigation"). A `⌂ Home` item on top
 * (sets the top-level `view` to `"home"`, spec §6.2 attention-first Home), then GROUPED workspaces
 * (S3 spec §10, task-18): one section per project (bold header row that opens the project panel,
 * its linked workspaces nested underneath) followed by a «No project» section for every workspace
 * linked to no project (each with an inline [link…] project `<select>`), then a «+ project»
 * button opening `CreateProjectDialog`. `pickFolder` is the CORE-ONLY native dialog (spec §6.1); on
 * a chosen dir we create a workspace named after its basename. The daemon validates the root
 * (spec §16) and pushes workspace://created, which App's subscription upserts into the store.
 *
 * Clicking a workspace row selects it as the App-level "active workspace" that new terminals are
 * created under (App owns this piece of state, not the store, since it is purely a UI selection
 * — not session/workspace data from the daemon) AND switches `view` to `"workspace"` — selecting
 * a workspace is how the owner leaves Home (spec §6.1 "workspace list... selecting a workspace
 * sets activeWorkspaceId AND view=\"workspace\""). This click behavior is UNCHANGED by the S3
 * grouping — `renderWorkspaceButton` below is the exact same button that used to sit directly in
 * the flat list, just called from two group contexts now instead of one inline `.map`.
 */
export function WorkspaceSidebar(props: {
  activeWorkspaceId: WorkspaceId | null;
  onSelectWorkspace: (id: WorkspaceId) => void;
}): JSX.Element {
  const { activeWorkspaceId, onSelectWorkspace } = props;
  const workspaces = useAppStore((s) => s.workspaces);
  const view = useAppStore((s) => s.view);
  const setView = useAppStore((s) => s.setView);
  const projects = useAppStore((s) => s.projects);
  const activeProjectId = useAppStore((s) => s.activeProjectId);
  const openProject = useAppStore((s) => s.openProject);
  const refreshProjects = useAppStore((s) => s.refreshProjects);
  const showToast = useAppStore((s) => s.showToast);

  const [showCreateDialog, setShowCreateDialog] = useState(false);
  const [showDiag, setShowDiag] = useState(false);
  const diagCount = useAppStore((s) => s.diagEvents.length);
  // Orphan (`projectId === null`) ideas + insights — the Inbox nav badge (AUD-2026-07-19-11).
  // Select the two stable arrays and derive the count outside the selector: a computed-number
  // selector would be fine, but this mirrors the "select stable slices" convention used above.
  const ideas = useAppStore((s) => s.ideas);
  const insights = useAppStore((s) => s.insights);
  const orphanCount =
    ideas.filter((i) => i.projectId === null).length +
    insights.filter((i) => i.projectId === null).length;
  const [attachSelection, setAttachSelection] = useState<Record<WorkspaceId, string>>({});
  // Archived projects live in a collapsed, dimmed group (O-3, spec D7) — collapsed by default so
  // the active-project navigation stays uncluttered.
  const [showArchived, setShowArchived] = useState(false);

  const list = Object.values(workspaces).sort((a, b) => a.name.localeCompare(b.name));
  const sortedProjects = [...projects].sort((a, b) => a.name.localeCompare(b.name));
  // Only ACTIVE projects get a first-class navigation group; archived ones are relegated to the
  // dimmed «Archived» group below (spec D7). `linkedIds` still counts EVERY project (incl.
  // archived), so an archived project's workspaces never leak into the «No project» group.
  const activeProjects = sortedProjects.filter((p) => p.status === "active");
  const archivedProjects = sortedProjects.filter((p) => p.status === "archived");
  const linkedIds = linkedWorkspaceIds(projects);
  const unlinkedWorkspaces = list.filter((w) => !linkedIds.has(w.id));

  function onSelectWorkspaceAndNavigate(id: WorkspaceId): void {
    onSelectWorkspace(id);
    setView("workspace");
  }

  async function onAdd(): Promise<void> {
    // `pickFolder`/`createWorkspace` are sessiond round-trips — a rejection (daemon down, invalid
    // root) must surface an honest toast, never a silent no-op (BL-93 / P-03). A cancelled picker
    // (`null`) is NOT an error — it returns quietly before the catch.
    try {
      const dir = await pickFolder();
      if (dir === null) return; // cancelled -> no-op
      const ws = await createWorkspace(basename(dir), dir);
      onSelectWorkspaceAndNavigate(ws.id);
    } catch (e) {
      showToast(strings.chrome.sidebar.addWorkspaceFailed(describeCommandError(e)));
    }
  }

  async function handleAttach(wsId: WorkspaceId, projectId: string): Promise<void> {
    if (projectId === "") return;
    try {
      await orchdAddProjectWorkspace(projectId, wsId);
      await refreshProjects();
      setAttachSelection((prev) => ({ ...prev, [wsId]: "" }));
    } catch (e) {
      showToast(describeOrchdError(e));
      setAttachSelection((prev) => ({ ...prev, [wsId]: "" }));
    }
  }

  /** The exact row button that used to be the whole of the flat `list.map` body — unchanged
   * style/click behavior, just factored out so both the project groups and the «No project»
   * group can render it (task-18: "reuse the current row JSX, don't rewrite it"). */
  function renderWorkspaceButton(w: Workspace): JSX.Element {
    const selected = view === "workspace" && w.id === activeWorkspaceId;
    return (
      <button
        type="button"
        title={w.rootPath}
        onClick={() => onSelectWorkspaceAndNavigate(w.id)}
        style={{
          display: "block",
          width: "100%",
          textAlign: "left",
          padding: "var(--sp-2) var(--sp-3)",
          fontSize: "var(--fs-md)",
          whiteSpace: "nowrap",
          overflow: "hidden",
          textOverflow: "ellipsis",
          border: "none",
          cursor: "pointer",
          color: selected ? "var(--accent)" : "var(--muted)",
          background: selected ? "var(--accent-weak)" : "transparent",
        }}
      >
        {w.name}
      </button>
    );
  }

  return (
    <>
      <aside
        aria-label="Workspaces"
        style={{
          width: 200,
          flexShrink: 0,
          background: "var(--panel)",
          borderRight: "1px solid var(--border)",
          color: "var(--ink)",
          display: "flex",
          flexDirection: "column",
        }}
      >
        <button
          type="button"
          aria-label="Home"
          aria-current={view === "home" ? "true" : undefined}
          onClick={() => setView("home")}
          style={{
            display: "block",
            width: "100%",
            textAlign: "left",
            padding: "var(--sp-2) var(--sp-3)",
            fontSize: "var(--fs-md)",
            fontWeight: 600,
            border: "none",
            borderBottom: "1px solid var(--border)",
            cursor: "pointer",
            color: view === "home" ? "var(--accent)" : "var(--muted)",
            background: view === "home" ? "var(--accent-weak)" : "transparent",
          }}
        >
          ⌂ Home
        </button>

        <button
          type="button"
          data-testid="ext-nav-button"
          aria-label={strings.chrome.sidebar.extensions}
          aria-current={view === "ext" ? "true" : undefined}
          onClick={() => setView("ext")}
          style={{
            display: "block",
            width: "100%",
            textAlign: "left",
            padding: "var(--sp-2) var(--sp-3)",
            fontSize: "var(--fs-md)",
            fontWeight: 600,
            border: "none",
            borderBottom: "1px solid var(--border)",
            cursor: "pointer",
            color: view === "ext" ? "var(--accent)" : "var(--muted)",
            background: view === "ext" ? "var(--accent-weak)" : "transparent",
          }}
        >
          {strings.chrome.sidebar.extensionsNav}
        </button>

        <button
          type="button"
          data-testid="inbox-nav-button"
          aria-label={strings.chrome.sidebar.inbox}
          aria-current={view === "inbox" ? "true" : undefined}
          onClick={() => setView("inbox")}
          style={{
            display: "flex",
            alignItems: "center",
            gap: "var(--sp-2)",
            width: "100%",
            textAlign: "left",
            padding: "var(--sp-2) var(--sp-3)",
            fontSize: "var(--fs-md)",
            fontWeight: 600,
            border: "none",
            borderBottom: "1px solid var(--border)",
            cursor: "pointer",
            color: view === "inbox" ? "var(--accent)" : "var(--muted)",
            background: view === "inbox" ? "var(--accent-weak)" : "transparent",
          }}
        >
          <span style={{ flex: 1 }}>{strings.chrome.sidebar.inboxNav}</span>
          {orphanCount > 0 && (
            <span
              data-testid="inbox-count"
              style={{
                fontSize: "var(--fs-xs)",
                fontWeight: 700,
                color: "var(--on-accent)",
                background: "var(--accent)",
                borderRadius: 999,
                padding: "0 var(--sp-2)",
                lineHeight: 1.6,
              }}
            >
              {orphanCount}
            </span>
          )}
        </button>

        <div style={{ flex: 1, overflowY: "auto" }}>
          {sortedProjects.length === 0 && list.length === 0 && (
            <div
              data-testid="sidebar-empty"
              style={{
                padding: "var(--sp-3)",
                fontSize: "var(--fs-sm)",
                lineHeight: 1.5,
                color: "var(--muted)",
              }}
            >
              {strings.chrome.sidebar.emptyState}
            </div>
          )}
          {activeProjects.map((project) => {
            const projectWorkspaces = project.workspaceIds
              .map((id) => workspaces[id])
              .filter((w): w is Workspace => w !== undefined);
            const projectActive = view === "project" && activeProjectId === project.id;
            return (
              <div key={project.id} data-testid={`project-group-${project.id}`}>
                <button
                  type="button"
                  data-testid={`project-group-header-${project.id}`}
                  aria-current={projectActive ? "true" : undefined}
                  onClick={() => openProject(project.id)}
                  style={{
                    display: "block",
                    width: "100%",
                    textAlign: "left",
                    padding: "var(--sp-2) var(--sp-3)",
                    fontSize: "var(--fs-md)",
                    fontWeight: 700,
                    border: "none",
                    borderTop: "1px solid var(--border)",
                    cursor: "pointer",
                    color: projectActive ? "var(--accent)" : "var(--ink)",
                    background: projectActive ? "var(--accent-weak)" : "transparent",
                  }}
                >
                  {project.name}
                </button>
                <ul style={{ listStyle: "none", margin: 0, padding: 0 }}>
                  {projectWorkspaces.map((w) => (
                    <li key={w.id}>{renderWorkspaceButton(w)}</li>
                  ))}
                </ul>
              </div>
            );
          })}

          <div data-testid="project-group-unassigned">
            <div
              style={{
                padding: "var(--sp-2) var(--sp-3)",
                fontSize: "var(--fs-sm)",
                textTransform: "uppercase",
                color: "var(--muted)",
                letterSpacing: 0.5,
                borderTop: "1px solid var(--border)",
              }}
            >
              {strings.chrome.sidebar.noProject}
            </div>
            <ul style={{ listStyle: "none", margin: 0, padding: 0 }}>
              {unlinkedWorkspaces.map((w) => (
                <li key={w.id} style={{ display: "flex", alignItems: "center", gap: 4 }}>
                  <div style={{ flex: 1, minWidth: 0 }}>{renderWorkspaceButton(w)}</div>
                  {activeProjects.length > 0 && (
                    <select
                      data-testid={`attach-workspace-${w.id}`}
                      aria-label={strings.chrome.sidebar.linkToProject(w.name)}
                      value={attachSelection[w.id] ?? ""}
                      onChange={(e) => {
                        const projectId = e.target.value;
                        setAttachSelection((prev) => ({ ...prev, [w.id]: projectId }));
                        void handleAttach(w.id, projectId);
                      }}
                      style={{ fontSize: "var(--fs-xs)", marginRight: "var(--sp-2)", maxWidth: 90 }}
                    >
                      <option value="">{strings.chrome.sidebar.linkPlaceholder}</option>
                      {activeProjects.map((p) => (
                        <option key={p.id} value={p.id}>
                          {p.name}
                        </option>
                      ))}
                    </select>
                  )}
                </li>
              ))}
            </ul>
          </div>

          {archivedProjects.length > 0 && (
            <div data-testid="archived-projects-group" style={{ opacity: 0.6 }}>
              <button
                type="button"
                data-testid="archived-projects-toggle"
                aria-expanded={showArchived}
                aria-label={strings.chrome.sidebar.archivedGroupToggleAria}
                onClick={() => setShowArchived((v) => !v)}
                style={{
                  display: "block",
                  width: "100%",
                  textAlign: "left",
                  padding: "var(--sp-2) var(--sp-3)",
                  fontSize: "var(--fs-sm)",
                  textTransform: "uppercase",
                  letterSpacing: 0.5,
                  border: "none",
                  borderTop: "1px solid var(--border)",
                  cursor: "pointer",
                  color: "var(--muted)",
                  background: "transparent",
                }}
              >
                {showArchived ? "▾" : "▸"} {strings.chrome.sidebar.archivedGroup(archivedProjects.length)}
              </button>
              {showArchived && (
                <ul style={{ listStyle: "none", margin: 0, padding: 0 }}>
                  {archivedProjects.map((project) => {
                    const projectActive = view === "project" && activeProjectId === project.id;
                    return (
                      <li key={project.id}>
                        <button
                          type="button"
                          data-testid={`archived-project-${project.id}`}
                          aria-current={projectActive ? "true" : undefined}
                          onClick={() => openProject(project.id)}
                          style={{
                            display: "block",
                            width: "100%",
                            textAlign: "left",
                            padding: "var(--sp-2) var(--sp-3)",
                            fontSize: "var(--fs-md)",
                            whiteSpace: "nowrap",
                            overflow: "hidden",
                            textOverflow: "ellipsis",
                            border: "none",
                            cursor: "pointer",
                            color: projectActive ? "var(--accent)" : "var(--muted)",
                            background: projectActive ? "var(--accent-weak)" : "transparent",
                          }}
                        >
                          {project.name}
                        </button>
                      </li>
                    );
                  })}
                </ul>
              )}
            </div>
          )}
        </div>

        <ThemeToggle />
        <button
          type="button"
          data-testid="create-project-open"
          onClick={() => setShowCreateDialog(true)}
          style={{
            margin: "var(--sp-2)",
            marginBottom: 0,
            padding: "var(--sp-2) var(--sp-3)",
            border: "1px solid var(--border-strong)",
            background: "var(--panel-2)",
            color: "var(--ink)",
            cursor: "pointer",
            fontSize: "var(--fs-md)",
            borderRadius: "var(--r-sm)",
          }}
        >
          {strings.chrome.sidebar.addProject}
        </button>
        <button
          type="button"
          aria-label={strings.chrome.sidebar.addWorkspaceAria}
          onClick={() => void onAdd()}
          style={{
            margin: "var(--sp-2)",
            padding: "var(--sp-2) var(--sp-3)",
            border: "1px solid var(--border-strong)",
            background: "var(--panel-2)",
            color: "var(--ink)",
            cursor: "pointer",
            fontSize: "var(--fs-md)",
            borderRadius: "var(--r-sm)",
          }}
        >
          {strings.chrome.sidebar.addWorkspace}
        </button>
        <button
          type="button"
          data-testid="diag-open"
          aria-label="Open diagnostics"
          onClick={() => setShowDiag(true)}
          style={{
            display: "flex",
            alignItems: "center",
            justifyContent: "center",
            gap: "var(--sp-2)",
            margin: "var(--sp-2)",
            padding: "var(--sp-2) var(--sp-3)",
            border: "1px solid var(--border-strong)",
            background: "var(--panel-2)",
            color: "var(--muted)",
            cursor: "pointer",
            fontSize: "var(--fs-sm)",
            borderRadius: "var(--r-sm)",
          }}
        >
          Diagnostics
          {diagCount > 0 && (
            <span
              data-testid="diag-count"
              style={{
                fontFamily: "var(--font-mono)",
                fontVariantNumeric: "tabular-nums",
                fontSize: "var(--fs-xs)",
                fontWeight: 600,
                color: "var(--danger)",
                background: "var(--danger-weak)",
                borderRadius: 999,
                padding: "0 var(--sp-2)",
              }}
            >
              {diagCount}
            </span>
          )}
        </button>
      </aside>
      {showCreateDialog && <CreateProjectDialog onClose={() => setShowCreateDialog(false)} />}
      <DiagnosticsPanel open={showDiag} onClose={() => setShowDiag(false)} />
    </>
  );
}
