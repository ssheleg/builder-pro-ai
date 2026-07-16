import { useEffect, useRef, useState, type CSSProperties, type JSX } from "react";
import { useAppStore } from "../store/store";
import { orchdCreateProject, describeOrchdError } from "../ipc/orchd";
import { pickFolder, createWorkspace } from "../ipc/commands";
import type { WorkspaceId } from "../ipc/commands";
import type { Project } from "../ipc/orchd-types";
import { useSubmitGuard } from "../hooks/useSubmitGuard";
import { theme } from "../theme";
import { strings } from "../strings";

/** Mirrors `WorkspaceSidebar.tsx`'s identical helper (same tiny, self-contained pattern — see
 * that file's doc comment; not worth a shared module for four lines). */
function basename(path: string): string {
  const parts = path.replace(/\/+$/, "").split("/");
  return parts[parts.length - 1] || path;
}

/** Every workspace id linked to at least one project (spec §10) — the dialog's multi-select
 * offers only the complement (unlinked workspaces), matching `ProjectPanel`'s add-workspace
 * select and `WorkspaceSidebar`'s "No project" group. */
function linkedWorkspaceIds(projects: Project[]): Set<string> {
  return new Set(projects.flatMap((p) => p.workspaceIds));
}

/** Locked inline-block copy (task-18 brief verbatim): shown whenever fewer than one workspace is
 * selected, and doubles as the reason the primary button stays disabled. */
const BLOCKED_TEXT = strings.project.workspaceRequired;

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
  width: 380,
  maxHeight: "80vh",
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
  minHeight: 60,
};

const workspaceListStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: 4,
  maxHeight: 140,
  overflowY: "auto",
};

const checkboxRowStyle: CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: 6,
  fontSize: 13,
  color: theme.colors.text,
};

const textButtonStyle: CSSProperties = {
  border: `1px solid ${theme.colors.border}`,
  background: "transparent",
  color: theme.colors.text,
  cursor: "pointer",
  fontSize: 12,
  borderRadius: 4,
  padding: "4px 8px",
  alignSelf: "flex-start",
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

const errorTextStyle: CSSProperties = {
  fontSize: 12,
  color: theme.colors.statusExited,
};

/** In-dialog failure line (design-system.md dialog atom: red statusExited text + left-edge accent,
 * distinct from the amber trigger-condition marker). Mirrors `UpgradeDialog`'s in-dialog error. */
const inlineErrorStyle: CSSProperties = {
  fontSize: 13,
  lineHeight: 1.5,
  color: theme.colors.statusExited,
  borderLeft: `3px solid ${theme.colors.statusExited}`,
  paddingLeft: 8,
};

/**
 * "+ project" dialog (S3 spec §10, task-18 brief). Design-system "Dialog / modal overlay" atom:
 * fixed dim backdrop + centered `bgElevated` card, `role="dialog"` + `aria-modal` + labelled
 * title.
 *
 * Name (required) + description + a multi-select of the workspaces linked to NO project yet
 * (`linkedWorkspaceIds` complement — the same computation `ProjectPanel`'s add-workspace select
 * and `WorkspaceSidebar`'s "No project" group use, so the three surfaces never disagree about
 * what "unlinked" means). The inline "+ create workspace" affordance reuses the EXACT
 * `pickFolder` -> `createWorkspace` flow `WorkspaceSidebar`'s own "+ Add workspace" button uses —
 * the new workspace is immediately upserted into the store (so it renders in the list without
 * waiting on the `workspace://created` push, which never fires in a unit test) and pre-selected.
 *
 * Submit is BLOCKED at 0 selected workspaces (D7-adjacent invariant mirror of
 * `CreateProject requires >=1 workspace_ids`, spec §5.2) — both a disabled primary button AND an
 * honest inline reason, never a doomed round-trip.
 *
 * Dialog-atom parity with `UpgradeDialog` (design-system.md "Dialog / modal overlay"): focuses the
 * name input on open and closes on `Escape` (same cancel path as the Cancel button). A create
 * failure is surfaced by an IN-DIALOG `role="alert"` line (the load-bearing failure surface —
 * `describeOrchdError(e)`) rather than only the global queue-of-one toast, which a concurrent toast
 * could clobber while the dialog is still open; the dialog STAYS open on failure so the owner can
 * fix the input and retry, mirroring `UpgradeDialog`'s "stays open so the primary button can be
 * retried" contract. The toast is still fired too (belt-and-suspenders), but the inline alert is
 * the one that must never vanish. The sessiond `createWorkspace` failure path (not an `orchd_*`
 * call) reuses the same inline-alert surface with a lighter, non-`describeOrchdError` message since
 * that helper is documented as orchd-specific.
 */
export function CreateProjectDialog(props: { onClose: () => void }): JSX.Element {
  const { onClose } = props;

  const projects = useAppStore((s) => s.projects);
  const workspaces = useAppStore((s) => s.workspaces);
  const showToast = useAppStore((s) => s.showToast);
  const upsertWorkspace = useAppStore((s) => s.upsertWorkspace);
  const { submitting, guard } = useSubmitGuard();

  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [selectedIds, setSelectedIds] = useState<WorkspaceId[]>([]);
  const [createError, setCreateError] = useState<string | null>(null);

  const nameRef = useRef<HTMLInputElement>(null);

  const linkedIds = linkedWorkspaceIds(projects);
  const unlinked = Object.values(workspaces)
    .filter((w) => !linkedIds.has(w.id))
    .sort((a, b) => a.name.localeCompare(b.name));
  const blocked = selectedIds.length === 0;

  // Initial focus + Escape-to-cancel (dialog-atom parity, mirrors UpgradeDialog's effect): focus
  // the name input (the first thing the owner types) on open, and let Escape run the SAME cancel
  // path as the Cancel button. Mounted once — the dialog is only ever rendered while open (its
  // parent gates it), so there is no open/closed toggle to depend on here.
  useEffect(() => {
    nameRef.current?.focus();
    const onKeyDown = (e: KeyboardEvent): void => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onClose]);

  function toggleWorkspace(id: WorkspaceId): void {
    setSelectedIds((prev) => (prev.includes(id) ? prev.filter((x) => x !== id) : [...prev, id]));
  }

  async function handleCreateWorkspace(): Promise<void> {
    try {
      const dir = await pickFolder();
      if (dir === null) return; // cancelled -> no-op, mirrors WorkspaceSidebar's onAdd
      const ws = await createWorkspace(basename(dir), dir);
      upsertWorkspace(ws);
      setSelectedIds((prev) => (prev.includes(ws.id) ? prev : [...prev, ws.id]));
    } catch (e) {
      const message = e instanceof Error ? e.message : strings.project.createWorkspaceFailed;
      setCreateError(message);
      showToast(message);
    }
  }

  async function handleSubmit(): Promise<void> {
    if (blocked || name.trim() === "") return;
    setCreateError(null); // clear any stale failure before a fresh attempt (UpgradeDialog parity)
    try {
      await orchdCreateProject(name.trim(), description, selectedIds);
      showToast(strings.project.projectCreated);
      onClose();
    } catch (e) {
      const message = describeOrchdError(e);
      setCreateError(message);
      showToast(message);
    }
  }

  // Double-submit guard (spec D6): a rapid second click before `orchdCreateProject` resolves must
  // NOT create a second project (finding P-19).
  const submit = guard(handleSubmit);

  return (
    <div style={overlayStyle}>
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="create-project-title"
        data-testid="create-project-dialog"
        style={cardStyle}
      >
        <div id="create-project-title" style={titleStyle}>
          {strings.project.newProject}
        </div>

        <label style={fieldLabelStyle}>
          {strings.project.nameLabel}
          <input
            ref={nameRef}
            data-testid="create-project-name"
            aria-label={strings.project.nameAria}
            required
            value={name}
            onChange={(e) => setName(e.target.value)}
            style={inputStyle}
          />
        </label>

        <label style={fieldLabelStyle}>
          {strings.project.descriptionLabel}
          <textarea
            data-testid="create-project-description"
            aria-label={strings.project.descriptionAria}
            value={description}
            onChange={(e) => setDescription(e.target.value)}
            rows={3}
            style={textareaStyle}
          />
        </label>

        <div>
          <div style={fieldLabelStyle}>Workspaces</div>
          <div data-testid="create-project-workspaces" style={workspaceListStyle}>
            {unlinked.length === 0 ? (
              <span style={{ fontSize: 12, color: theme.colors.textDim }}>{strings.project.noFreeWorkspaces}</span>
            ) : (
              unlinked.map((w) => (
                <label key={w.id} style={checkboxRowStyle}>
                  <input
                    type="checkbox"
                    data-testid={`create-project-ws-${w.id}`}
                    checked={selectedIds.includes(w.id)}
                    onChange={() => toggleWorkspace(w.id)}
                  />
                  {w.name}
                </label>
              ))
            )}
          </div>
          <button
            type="button"
            data-testid="create-project-new-workspace"
            onClick={() => void handleCreateWorkspace()}
            style={{ ...textButtonStyle, marginTop: 6 }}
          >
            {strings.project.createWorkspace}
          </button>
        </div>

        {blocked && (
          <div data-testid="create-project-blocked" role="alert" style={errorTextStyle}>
            {BLOCKED_TEXT}
          </div>
        )}

        {createError !== null && (
          // In-dialog failure surface (design-system.md "Dialog / modal overlay" atom: an action
          // the user just took failed → role="alert", statusExited red + left-edge, distinct from
          // the trigger-condition amber). This is the LOAD-BEARING failure indicator — it survives
          // a concurrent toast clobbering the global queue-of-one, and the dialog stays open so the
          // owner can fix the input and retry (mirrors UpgradeDialog's retry-in-place behavior).
          <div data-testid="create-project-error" role="alert" style={inlineErrorStyle}>
            {createError}
          </div>
        )}

        <div style={{ display: "flex", justifyContent: "flex-end", gap: 8, marginTop: 4 }}>
          <button type="button" data-testid="create-project-cancel" onClick={onClose} style={secondaryButtonStyle}>
            {strings.common.cancel}
          </button>
          <button
            type="button"
            data-testid="create-project-submit"
            disabled={blocked || name.trim() === "" || submitting}
            onClick={() => void submit()}
            style={{ ...primaryButtonStyle, opacity: blocked || name.trim() === "" || submitting ? 0.5 : 1 }}
          >
            {strings.common.create}
          </button>
        </div>
      </div>
    </div>
  );
}
