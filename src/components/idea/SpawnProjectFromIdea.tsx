import { useState, type CSSProperties, type JSX } from "react";
import { useAppStore } from "../../store/store";
import { orchdCreateProject, orchdSetIdeaProject, describeOrchdError } from "../../ipc/orchd";
import { pickFolder, createWorkspace } from "../../ipc/commands";
import type { Idea } from "../../ipc/orchd-types";
import { theme } from "../../theme";
import { strings } from "../../strings";

/** Mirrors `WorkspaceSidebar.tsx`'s / `CreateProjectDialog.tsx`'s identical helper (same tiny,
 * self-contained pattern — not worth a shared module for four lines). */
function basename(path: string): string {
  const parts = path.replace(/\/+$/, "").split("/");
  return parts[parts.length - 1] || path;
}

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

const errorTextStyle: CSSProperties = {
  fontSize: 11,
  color: theme.colors.statusExited,
};

/**
 * "Create project from idea" (S-IDEA spec §7): the spawn flow for a project-less idea —
 * `pickFolder` -> `createWorkspace` (sessiond) -> `orchdCreateProject{name:idea.title,
 * workspaceIds:[newWorkspaceId]}` -> `orchdSetIdeaProject(idea.id, project.id)`, run as ONE
 * sequential chain from a single click (unlike `CreateProjectDialog`, there is no owner-typed
 * form here — every input is either derived from the idea or the OS folder picker).
 *
 * Mirrors `CreateProjectDialog`'s inline "+ create workspace" affordance: the new workspace is
 * upserted into the store immediately (`upsertWorkspace`) rather than waiting on the
 * `workspace://created` push (which never fires in a unit test, and would otherwise leave the
 * store momentarily stale even in production between this call and that push's arrival).
 *
 * Error handling is split like `CreateProjectDialog`'s `handleCreateWorkspace`/`handleSubmit`:
 * a `pickFolder`/`createWorkspace` failure (sessiond, not `orchd_*`) uses a lighter, non-
 * `describeOrchdError` message; an `orchdCreateProject`/`orchdSetIdeaProject` failure uses
 * `describeOrchdError` (spec §7 honest error surface). Either failure is shown both as a toast
 * and inline next to the button (`spawn-project-error-${idea.id}`) — the toast alone could be
 * clobbered by a concurrent one before the owner notices.
 *
 * Honest degradation (spec §10, T8 discipline): the button is `disabled={orchdDown}` — mirrors
 * `IdeasList`'s own choice to disable even its dialog/flow-opening triggers while the daemon is
 * down.
 */
export function SpawnProjectFromIdea(props: { idea: Idea }): JSX.Element {
  const { idea } = props;

  const orchdDown = useAppStore((s) => s.orchdDown);
  const showToast = useAppStore((s) => s.showToast);
  const upsertWorkspace = useAppStore((s) => s.upsertWorkspace);
  const refreshProjects = useAppStore((s) => s.refreshProjects);
  const refreshIdeas = useAppStore((s) => s.refreshIdeas);

  const [error, setError] = useState<string | null>(null);

  async function handleSpawn(): Promise<void> {
    setError(null);

    let dir: string | null;
    try {
      dir = await pickFolder();
    } catch (e) {
      const message = e instanceof Error ? e.message : strings.ideas.spawn.folderPickerFailed;
      setError(message);
      showToast(message);
      return;
    }
    if (dir === null) return; // cancelled -> no-op, mirrors CreateProjectDialog/WorkspaceSidebar

    let workspaceId: string;
    try {
      const ws = await createWorkspace(basename(dir), dir);
      upsertWorkspace(ws);
      workspaceId = ws.id;
    } catch (e) {
      const message = e instanceof Error ? e.message : strings.project.createWorkspaceFailed;
      setError(message);
      showToast(message);
      return;
    }

    try {
      const project = await orchdCreateProject(idea.title, "", [workspaceId]);
      await orchdSetIdeaProject(idea.id, project.id);
      await refreshProjects();
      await refreshIdeas();
      showToast(strings.ideas.spawn.createdFromIdea);
    } catch (e) {
      const message = describeOrchdError(e);
      setError(message);
      showToast(message);
    }
  }

  return (
    <>
      <button
        type="button"
        data-testid={`spawn-project-${idea.id}`}
        disabled={orchdDown}
        onClick={() => void handleSpawn()}
        style={textButtonStyle}
      >
        {strings.ideas.spawn.createProject}
      </button>
      {error !== null && (
        <span data-testid={`spawn-project-error-${idea.id}`} style={errorTextStyle}>
          {error}
        </span>
      )}
    </>
  );
}
