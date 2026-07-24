import { useEffect, useState, type JSX } from "react";
import { useAppStore } from "../store/store";
import { pickFolder, createWorkspace, createSession, pathsExist } from "../ipc/commands";
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
  /** `null` clears the selection — used after removing the workspace currently on screen
   * (SCN-058: the app must fall back to Home rather than keep a dead view). */
  onSelectWorkspace: (id: WorkspaceId | null) => void;
}): JSX.Element {
  const { activeWorkspaceId, onSelectWorkspace } = props;
  const workspaces = useAppStore((s) => s.workspaces);
  const removeWorkspace = useAppStore((s) => s.removeWorkspace);
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
  // Keep-awake pill state (SCN-045): `active`/`error` are the core's reconciled truth (the
  // assertion's REAL held-state), never the toggle intent — see the store slice's doc.
  const keepAwake = useAppStore((s) => s.keepAwake);
  const setKeepAwakeEnabled = useAppStore((s) => s.setKeepAwakeEnabled);
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

  /**
   * SCN-059 root-presence map, `path -> exists`. A path is present unless the local check said
   * DEFINITELY not (`paths_exist` reports an unreadable path as present — never remove on a
   * guess), and a path absent from this map is treated as present too: an unknown answer (check
   * not run yet, or the whole check failed) must never mark a healthy workspace as missing.
   */
  const [rootPresence, setRootPresence] = useState<Record<string, boolean>>({});

  const list = Object.values(workspaces).sort((a, b) => a.name.localeCompare(b.name));
  const sortedProjects = [...projects].sort((a, b) => a.name.localeCompare(b.name));
  // Stable primitive dependency for the presence effect: re-check when the SET of roots changes
  // (workspace added/removed, root added/removed), not on every unrelated store update.
  const rootsKey = JSON.stringify(list.map((w) => w.roots));

  useEffect(() => {
    let cancelled = false;
    const paths = Array.from(new Set(Object.values(workspaces).flatMap((w) => w.roots)));
    if (paths.length === 0) {
      setRootPresence({});
      return;
    }
    void (async () => {
      try {
        const flags = await pathsExist(paths);
        if (cancelled) return;
        const next: Record<string, boolean> = {};
        paths.forEach((p, i) => {
          // A short/garbled reply defaults to PRESENT for the same reason a permission error does.
          next[p] = flags[i] ?? true;
        });
        setRootPresence(next);
      } catch {
        // The check itself failed (core down, IPC error). Honest degradation: forget every verdict
        // so no row is marked missing and the bulk clean-up offers nothing — never the reverse.
        if (!cancelled) setRootPresence({});
      }
    })();
    return () => {
      cancelled = true;
    };
    // `rootsKey` is the value-identity of `workspaces`' roots; `workspaces` itself is read inside.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [rootsKey]);

  /**
   * A workspace counts as MISSING only when EVERY one of its roots is definitely gone (SCN-059).
   * A multi-root workspace with one surviving root still has a folder to work in, so it is
   * untouched — the conservative reading of "the workspace's folder is gone", matching the
   * scenario's "never removed on a guess".
   */
  function isMissing(w: Workspace): boolean {
    return w.roots.length > 0 && w.roots.every((r) => rootPresence[r] === false);
  }

  const missingWorkspaces = list.filter(isMissing);

  /** SCN-058: state the consequence, take the confirmation, then (and only then) remove. */
  async function onRemoveWorkspace(w: Workspace): Promise<void> {
    if (!window.confirm(strings.chrome.sidebar.removeWorkspaceConfirm(w.name))) return;
    try {
      await removeWorkspace(w.id);
    } catch (e) {
      // Rejected ⇒ nothing was removed and the row stays exactly where it was.
      showToast(strings.chrome.sidebar.removeWorkspaceFailed(describeCommandError(e)));
      return;
    }
    // Never leave a dead view behind. (`workspace://removed` reaches App with the same fix for
    // every other window — and for this one, should this local path ever be beaten to it.)
    if (activeWorkspaceId === w.id) {
      onSelectWorkspace(null);
      setView("home");
    }
  }

  /**
   * SCN-059 bulk clean-up: remove ONLY the workspaces whose folders are definitely gone, after
   * confirming the exact count. Each removal is independent — the successes stand and a single
   * toast names how many failed (no silent partial success, and no toast storm either).
   */
  async function onCleanupMissing(): Promise<void> {
    const doomed = missingWorkspaces;
    if (doomed.length === 0) return; // defensive: the control is not rendered in this state
    if (!window.confirm(strings.chrome.sidebar.cleanupMissingConfirm(doomed.length))) return;
    const results = await Promise.allSettled(doomed.map((w) => removeWorkspace(w.id)));
    const failed = results.filter((r) => r.status === "rejected").length;
    if (failed > 0) {
      showToast(strings.chrome.sidebar.cleanupMissingPartial(failed, doomed.length));
    }
    // The active workspace may have been one of the removed ones (a missing folder does not stop
    // it from being on screen) — same dead-view rule as the single removal above.
    if (activeWorkspaceId !== null && doomed.some((w) => w.id === activeWorkspaceId)) {
      const stillThere = useAppStore.getState().workspaces[activeWorkspaceId] !== undefined;
      if (!stillThere) {
        onSelectWorkspace(null);
        setView("home");
      }
    }
  }

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
    //
    // First-run fast-path (SCN-056 / IMP-01): when the app holds ZERO sessions anywhere, the only
    // reason to add a workspace is to reach a terminal — so we auto-spawn the first one instead of
    // demanding a manual "+ New terminal" click (peak-end: aha one step earlier, JRN-01/#4). The
    // count is captured BEFORE the round-trips so a concurrently-pushed session cannot flip the
    // decision mid-flight. Steady state (any session already exists) is untouched — no surprise
    // terminals. The spawned session announces itself via session://created (App upserts +
    // auto-activates); a failed spawn degrades to today's manual path: the workspace view is
    // already open, we only surface the same honest toast "+ New terminal" would.
    const hadSessions = Object.keys(useAppStore.getState().sessions).length > 0;
    try {
      const dir = await pickFolder();
      if (dir === null) return; // cancelled -> no-op
      const ws = await createWorkspace(basename(dir), dir);
      onSelectWorkspaceAndNavigate(ws.id);
      if (!hadSessions) {
        try {
          // cwd = the workspace's canonical root (just created from `dir`). Without it the
          // omitted cwd makes sessiond default to $HOME, landing the fast-path terminal
          // outside the repo (AUD-2026-07-23-17) — the very folder the user just picked.
          // Mirrors the manual "+ New terminal" root-aware spawn (TerminalTabs.onNewTerminal).
          await createSession(ws.id, { cwd: ws.roots[0] ?? dir, cols: 80, rows: 24 });
        } catch (e) {
          showToast(strings.terminal.tabs.newTerminalFailed(describeCommandError(e)));
        }
      }
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
   * group can render it (task-18: "reuse the current row JSX, don't rewrite it"). Now sits in a
   * row alongside the SCN-059 "folder missing" marker and the SCN-058 remove control, which both
   * belong to the same workspace and must appear wherever that workspace is listed. */
  function renderWorkspaceButton(w: Workspace): JSX.Element {
    const selected = view === "workspace" && w.id === activeWorkspaceId;
    const missing = isMissing(w);
    return (
      <div style={{ display: "flex", alignItems: "center", minWidth: 0 }}>
        <button
          type="button"
          title={w.rootPath}
          onClick={() => onSelectWorkspaceAndNavigate(w.id)}
          style={{
            display: "block",
            flex: 1,
            minWidth: 0,
            textAlign: "left",
            padding: "var(--sp-2) var(--sp-3)",
            fontSize: "var(--fs-md)",
            whiteSpace: "nowrap",
            overflow: "hidden",
            textOverflow: "ellipsis",
            border: "none",
            cursor: "pointer",
            color: selected ? "var(--ink)" : "var(--muted)",
            background: selected ? "var(--panel-2)" : "transparent",
          }}
        >
          {w.name}
        </button>
        {missing && (
          // SCN-059 step 1: a missing workspace is visibly distinct from a healthy one, with the
          // gone path spelled out on hover. Marker only — nothing is removed without a
          // confirmation.
          <span
            data-testid={`workspace-missing-${w.id}`}
            title={strings.chrome.sidebar.rootMissingTitle(w.roots.join(", "))}
            style={{
              flexShrink: 0,
              fontSize: "var(--fs-xs)",
              color: "var(--warn)",
              whiteSpace: "nowrap",
            }}
          >
            {strings.chrome.sidebar.rootMissing}
          </span>
        )}
        <button
          type="button"
          data-testid={`remove-workspace-${w.id}`}
          aria-label={strings.chrome.sidebar.removeWorkspaceAria(w.name)}
          title={strings.chrome.sidebar.removeWorkspaceAria(w.name)}
          onClick={() => void onRemoveWorkspace(w)}
          style={{
            flexShrink: 0,
            border: "none",
            background: "transparent",
            color: "var(--muted)",
            cursor: "pointer",
            padding: "0 var(--sp-2)",
            fontSize: "var(--fs-md)",
            lineHeight: 1,
          }}
        >
          ×
        </button>
      </div>
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
          borderRight: "1px solid var(--hairline)",
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
            borderBottom: "1px solid var(--hairline)",
            cursor: "pointer",
            color: view === "home" ? "var(--ink)" : "var(--muted)",
            background: view === "home" ? "var(--panel-2)" : "transparent",
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
            borderBottom: "1px solid var(--hairline)",
            cursor: "pointer",
            color: view === "ext" ? "var(--ink)" : "var(--muted)",
            background: view === "ext" ? "var(--panel-2)" : "transparent",
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
            borderBottom: "1px solid var(--hairline)",
            cursor: "pointer",
            color: view === "inbox" ? "var(--ink)" : "var(--muted)",
            background: view === "inbox" ? "var(--panel-2)" : "transparent",
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

        <button
          type="button"
          data-testid="stats-nav-button"
          aria-label={strings.stats.title}
          aria-current={view === "stats" ? "true" : undefined}
          onClick={() => setView("stats")}
          style={{
            display: "block",
            width: "100%",
            textAlign: "left",
            padding: "var(--sp-2) var(--sp-3)",
            fontSize: "var(--fs-md)",
            fontWeight: 600,
            border: "none",
            borderBottom: "1px solid var(--hairline)",
            cursor: "pointer",
            color: view === "stats" ? "var(--ink)" : "var(--muted)",
            background: view === "stats" ? "var(--panel-2)" : "transparent",
          }}
        >
          {strings.stats.nav}
        </button>

        <button
          type="button"
          data-testid="workflows-nav-button"
          aria-label={strings.workflows.title}
          aria-current={view === "workflows" ? "true" : undefined}
          onClick={() => setView("workflows")}
          style={{
            display: "block",
            width: "100%",
            textAlign: "left",
            padding: "var(--sp-2) var(--sp-3)",
            fontSize: "var(--fs-md)",
            fontWeight: 600,
            border: "none",
            borderBottom: "1px solid var(--hairline)",
            cursor: "pointer",
            color: view === "workflows" ? "var(--ink)" : "var(--muted)",
            background: view === "workflows" ? "var(--panel-2)" : "transparent",
          }}
        >
          {strings.workflows.nav}
        </button>

        {/* Scroll region for the workspace/project list. `minHeight: 0` is what actually lets it
            shrink: without it a flex item's `min-height: auto` keeps it at its CONTENT height, so
            a long list pushed the footer controls off the bottom of the window instead of
            scrolling — and at small window heights the region collapsed while the footer walked
            out of view. `paddingBottom` keeps the last row off the footer's hairline (it used to
            be sliced mid-row, flush against it) and `scrollbarGutter` reserves the scrollbar's
            width so rows don't shift sideways the moment the list becomes scrollable. */}
        <div
          data-testid="sidebar-scroll"
          style={{
            flex: 1,
            minHeight: 0,
            overflowY: "auto",
            paddingBottom: "var(--sp-2)",
            scrollbarGutter: "stable",
          }}
        >
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
                    borderTop: "1px solid var(--hairline)",
                    cursor: "pointer",
                    color: "var(--ink)",
                    background: projectActive ? "var(--panel-2)" : "transparent",
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
                borderTop: "1px solid var(--hairline)",
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
                  borderTop: "1px solid var(--hairline)",
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
                            color: projectActive ? "var(--ink)" : "var(--muted)",
                            background: projectActive ? "var(--panel-2)" : "transparent",
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

        {/* Footer block. One wrapper around every footer control so the group is a SINGLE
            non-shrinking flex item with one separating hairline: as bare siblings each control was
            individually shrinkable, which at small window heights squeezed them (and eventually
            walked them out of the window) while the list above kept its content height. The
            children's own styles are deliberately untouched. */}
        <div
          data-testid="sidebar-footer"
          style={{ flexShrink: 0, borderTop: "1px solid var(--hairline)" }}
        >
          {/* SCN-059 bulk clean-up: rendered ONLY while at least one workspace's folder is
              definitely gone — no dead control when there is nothing to clean up — and it names
              the exact count it would remove, both here and in the confirmation. */}
          {missingWorkspaces.length > 0 && (
            <button
              type="button"
              data-testid="cleanup-missing-workspaces"
              aria-label={strings.chrome.sidebar.cleanupMissing(missingWorkspaces.length)}
              onClick={() => void onCleanupMissing()}
              style={{
                display: "block",
                width: "calc(100% - 2 * var(--sp-2))",
                margin: "var(--sp-2)",
                marginBottom: 0,
                padding: "var(--sp-2) var(--sp-3)",
                border: "none",
                background: "var(--panel-2)",
                color: "var(--warn)",
                cursor: "pointer",
                fontSize: "var(--fs-sm)",
                borderRadius: "var(--r-sm)",
                textAlign: "left",
              }}
            >
              {strings.chrome.sidebar.cleanupMissing(missingWorkspaces.length)}
            </button>
          )}
          {/* Keep-awake pill (SCN-045 / FLW-18, footer next to ThemeToggle/Diagnostics): click
            toggles the persisted preference; the dot is the honest assertion indicator — ok
            (green) only while the OS assertion is GENUINELY held, danger on an OS denial with
            the "keep-awake unavailable: {reason}" copy, muted for idle/off. Tone tokens follow
            `StatusDot.tsx`'s `var(--ok)`/`var(--muted)`/`var(--danger)` convention. */}
          <KeepAwakePill
            enabled={keepAwake.enabled}
            active={keepAwake.active}
            error={keepAwake.error}
            onToggle={() => void setKeepAwakeEnabled(!keepAwake.enabled)}
          />
          <ThemeToggle />
          <button
            type="button"
            data-testid="create-project-open"
            onClick={() => setShowCreateDialog(true)}
            style={{
              margin: "var(--sp-2)",
              marginBottom: 0,
              padding: "var(--sp-2) var(--sp-3)",
              border: "none",
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
              border: "none",
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
              border: "none",
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
        </div>
      </aside>
      {showCreateDialog && <CreateProjectDialog onClose={() => setShowCreateDialog(false)} />}
      <DiagnosticsPanel open={showDiag} onClose={() => setShowDiag(false)} />
    </>
  );
}

/**
 * Sidebar-footer keep-awake pill (SCN-045 / FLW-18): toggle + active-assertion indicator in one
 * control. Purely presentational over the store's `keepAwake` slice — the honest state machine
 * (want/held/denied) lives in `src-tauri/src/power.rs`; the once-per-streak toast/Diag surfacing
 * lives in `store.ts::syncKeepAwake`. Label/dot resolution, most-severe first:
 * - `error` (OS denied the assertion) → danger dot + "keep-awake unavailable: {reason}" — the
 *   pill-level failure state SCN-045 requires alongside the toast, never a silent fake "awake";
 * - `active` (assertion genuinely held) → ok dot + "keep-awake · on";
 * - enabled but idle (zero live sessions — nothing to hold) → muted dot + "keep-awake · idle";
 * - disabled → muted dot + "keep-awake · off".
 */
function KeepAwakePill(props: {
  enabled: boolean;
  active: boolean;
  error: string | null;
  onToggle: () => void;
}): JSX.Element {
  const { enabled, active, error, onToggle } = props;
  const label =
    error !== null
      ? strings.power.keepAwakeFailed(error)
      : !enabled
        ? strings.power.keepAwakeOff
        : active
          ? strings.power.keepAwakeOn
          : strings.power.keepAwakeIdle;
  // Same semantic tone tokens `StatusDot.tsx` resolves (theme-aware, light+dark valid): the dot
  // never shows ok unless the assertion is really held.
  const dotTone = error !== null ? "var(--danger)" : active ? "var(--ok)" : "var(--muted)";
  return (
    <button
      type="button"
      data-testid="keep-awake-pill"
      aria-pressed={enabled}
      aria-label={label}
      title={label}
      onClick={onToggle}
      style={{
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        gap: "var(--sp-2)",
        margin: "var(--sp-2)",
        marginBottom: 0,
        padding: "var(--sp-2) var(--sp-3)",
        border: "none",
        background: "var(--panel-2)",
        color: "var(--muted)",
        cursor: "pointer",
        fontSize: "var(--fs-sm)",
        borderRadius: "var(--r-sm)",
      }}
    >
      <span
        data-testid="keep-awake-dot"
        role="img"
        aria-hidden="true"
        style={{
          display: "inline-block",
          width: 8,
          height: 8,
          borderRadius: "50%",
          backgroundColor: dotTone,
          flexShrink: 0,
        }}
      />
      {/* A long denial reason must not blow the 200px rail open — ellipsize; `title` above
          carries the full copy. */}
      <span style={{ overflow: "hidden", whiteSpace: "nowrap", textOverflow: "ellipsis" }}>
        {label}
      </span>
    </button>
  );
}
