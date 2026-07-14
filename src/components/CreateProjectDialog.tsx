import { useState, type CSSProperties, type JSX } from "react";
import { useAppStore } from "../store/store";
import { orchdCreateProject, describeOrchdError } from "../ipc/orchd";
import { pickFolder, createWorkspace } from "../ipc/commands";
import type { WorkspaceId } from "../ipc/commands";
import type { Project } from "../ipc/orchd-types";
import { theme } from "../theme";

/** Mirrors `WorkspaceSidebar.tsx`'s identical helper (same tiny, self-contained pattern — see
 * that file's doc comment; not worth a shared module for four lines). */
function basename(path: string): string {
  const parts = path.replace(/\/+$/, "").split("/");
  return parts[parts.length - 1] || path;
}

/** Every workspace id linked to at least one project (spec §10) — the dialog's multi-select
 * offers only the complement (unlinked workspaces), matching `ProjectPanel`'s add-workspace
 * select and `WorkspaceSidebar`'s «Без проекта» group. */
function linkedWorkspaceIds(projects: Project[]): Set<string> {
  return new Set(projects.flatMap((p) => p.workspaceIds));
}

/** Locked inline-block copy (task-18 brief verbatim): shown whenever fewer than one workspace is
 * selected, and doubles as the reason the primary button stays disabled. */
const BLOCKED_TEXT = "нужен хотя бы один workspace";

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

/**
 * «+ проект» dialog (S3 spec §10, task-18 brief). Design-system "Dialog / modal overlay" atom:
 * fixed dim backdrop + centered `bgElevated` card, `role="dialog"` + `aria-modal` + labelled
 * title.
 *
 * Name (required) + description + a multi-select of the workspaces linked to NO project yet
 * (`linkedWorkspaceIds` complement — the same computation `ProjectPanel`'s add-workspace select
 * and `WorkspaceSidebar`'s «Без проекта» group use, so the three surfaces never disagree about
 * what "unlinked" means). The inline «+ создать workspace» affordance reuses the EXACT
 * `pickFolder` -> `createWorkspace` flow `WorkspaceSidebar`'s own "+ Add workspace" button uses —
 * the new workspace is immediately upserted into the store (so it renders in the list without
 * waiting on the `workspace://created` push, which never fires in a unit test) and pre-selected.
 *
 * Submit is BLOCKED at 0 selected workspaces (D7-adjacent invariant mirror of
 * `CreateProject requires >=1 workspace_ids`, spec §5.2) — both a disabled primary button AND an
 * honest inline reason, never a doomed round-trip. A create failure surfaces via `showToast`
 * (spec §7) and leaves the dialog open so the owner can fix the input and retry; the sessiond
 * `createWorkspace` failure path (not an `orchd_*` call) gets a lighter, non-`describeOrchdError`
 * fallback message since that helper is documented as orchd-specific.
 */
export function CreateProjectDialog(props: { onClose: () => void }): JSX.Element {
  const { onClose } = props;

  const projects = useAppStore((s) => s.projects);
  const workspaces = useAppStore((s) => s.workspaces);
  const showToast = useAppStore((s) => s.showToast);
  const upsertWorkspace = useAppStore((s) => s.upsertWorkspace);

  const [name, setName] = useState("");
  const [description, setDescription] = useState("");
  const [selectedIds, setSelectedIds] = useState<WorkspaceId[]>([]);

  const linkedIds = linkedWorkspaceIds(projects);
  const unlinked = Object.values(workspaces)
    .filter((w) => !linkedIds.has(w.id))
    .sort((a, b) => a.name.localeCompare(b.name));
  const blocked = selectedIds.length === 0;

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
      showToast(e instanceof Error ? e.message : "не удалось создать workspace");
    }
  }

  async function handleSubmit(): Promise<void> {
    if (blocked || name.trim() === "") return;
    try {
      await orchdCreateProject(name.trim(), description, selectedIds);
      showToast("Проект создан");
      onClose();
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

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
          Новый проект
        </div>

        <label style={fieldLabelStyle}>
          Название
          <input
            data-testid="create-project-name"
            aria-label="Название проекта"
            required
            value={name}
            onChange={(e) => setName(e.target.value)}
            style={inputStyle}
          />
        </label>

        <label style={fieldLabelStyle}>
          Описание
          <textarea
            data-testid="create-project-description"
            aria-label="Описание проекта"
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
              <span style={{ fontSize: 12, color: theme.colors.textDim }}>нет свободных workspace</span>
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
            + создать workspace
          </button>
        </div>

        {blocked && (
          <div data-testid="create-project-blocked" role="alert" style={errorTextStyle}>
            {BLOCKED_TEXT}
          </div>
        )}

        <div style={{ display: "flex", justifyContent: "flex-end", gap: 8, marginTop: 4 }}>
          <button type="button" data-testid="create-project-cancel" onClick={onClose} style={secondaryButtonStyle}>
            Отмена
          </button>
          <button
            type="button"
            data-testid="create-project-submit"
            disabled={blocked || name.trim() === ""}
            onClick={() => void handleSubmit()}
            style={{ ...primaryButtonStyle, opacity: blocked || name.trim() === "" ? 0.5 : 1 }}
          >
            Создать
          </button>
        </div>
      </div>
    </div>
  );
}
