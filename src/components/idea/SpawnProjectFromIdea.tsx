import { useState, type CSSProperties, type JSX } from "react";
import { useAppStore } from "../../store/store";
import { orchdCreateProject, orchdSetIdeaProject, describeOrchdError } from "../../ipc/orchd";
import { pickFolder, createWorkspace } from "../../ipc/commands";
import type { Idea } from "../../ipc/orchd-types";
import { useSubmitGuard } from "../../hooks/useSubmitGuard";
import { Button } from "../../ui/primitives";
import { strings } from "../../strings";

/** Mirrors `WorkspaceSidebar.tsx`'s / `CreateProjectDialog.tsx`'s identical helper (same tiny,
 * self-contained pattern — not worth a shared module for four lines). */
function basename(path: string): string {
  const parts = path.replace(/\/+$/, "").split("/");
  return parts[parts.length - 1] || path;
}

const errorTextStyle: CSSProperties = {
  fontSize: "var(--fs-xs)",
  color: "var(--danger)",
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
  const { submitting, guard } = useSubmitGuard();

  const [error, setError] = useState<string | null>(null);
  // Resume-from-failed-step state (spec D6, BL-95/P-09): once a workspace / project has been
  // created, its id is held here so a retry after a mid-chain failure never re-runs a completed
  // step — no orphaned second project/workspace. Cleared on full success.
  const [createdWorkspaceId, setCreatedWorkspaceId] = useState<string | null>(null);
  const [createdProjectId, setCreatedProjectId] = useState<string | null>(null);

  async function handleSpawn(): Promise<void> {
    setError(null);

    // Step 1 — workspace. Skipped entirely on a resume (its id is already held), so neither the
    // folder picker nor `createWorkspace` re-runs.
    let workspaceId = createdWorkspaceId;
    if (workspaceId === null) {
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

      try {
        const ws = await createWorkspace(basename(dir), dir);
        upsertWorkspace(ws);
        workspaceId = ws.id;
        setCreatedWorkspaceId(ws.id);
      } catch (e) {
        const message = e instanceof Error ? e.message : strings.project.createWorkspaceFailed;
        setError(message);
        showToast(message);
        return;
      }
    }

    // Step 2 — project. Skipped on a resume once created (holds its id).
    let projectId = createdProjectId;
    if (projectId === null) {
      try {
        const project = await orchdCreateProject(idea.title, "", [workspaceId]);
        projectId = project.id;
        setCreatedProjectId(project.id);
      } catch (e) {
        const message = describeOrchdError(e);
        setError(message);
        showToast(message);
        return;
      }
    }

    // Step 3 — link the idea to the created project. If THIS fails, the project + workspace already
    // exist (P-09): the honest error names exactly what was created, and the retry resumes here
    // (steps 1+2 skipped) so it never creates a second project.
    try {
      await orchdSetIdeaProject(idea.id, projectId);
      await refreshProjects();
      await refreshIdeas();
      showToast(strings.ideas.spawn.createdFromIdea);
      setCreatedWorkspaceId(null);
      setCreatedProjectId(null);
      setError(null);
    } catch (e) {
      const message = strings.ideas.spawn.linkFailed(idea.title, describeOrchdError(e));
      setError(message);
      showToast(message);
    }
  }

  const spawn = guard(handleSpawn);
  // A partial failure (project created, link pending) turns the primary action into a resume:
  // clicking again finishes the link rather than starting a fresh spawn.
  const resuming = createdProjectId !== null;

  return (
    <>
      <Button
        variant="ghost"
        size="sm"
        type="button"
        data-testid={`spawn-project-${idea.id}`}
        disabled={orchdDown || submitting}
        onClick={() => void spawn()}
      >
        {resuming ? strings.ideas.spawn.retry : strings.ideas.spawn.createProject}
      </Button>
      {error !== null && (
        <span data-testid={`spawn-project-error-${idea.id}`} style={errorTextStyle}>
          {error}
        </span>
      )}
    </>
  );
}
