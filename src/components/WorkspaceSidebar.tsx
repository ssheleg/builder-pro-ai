import { useAppStore } from "../store/store";
import { pickFolder, createWorkspace } from "../ipc/commands";
import type { WorkspaceId } from "../ipc/commands";
import { theme } from "../theme";

function basename(path: string): string {
  const parts = path.replace(/\/+$/, "").split("/");
  return parts[parts.length - 1] || path;
}

/**
 * Workspace list + folder picker. `pickFolder` is the CORE-ONLY native dialog
 * (spec §6.1); on a chosen dir we create a workspace named after its basename.
 * The daemon validates the root (spec §16) and pushes workspace://created, which
 * App's subscription upserts into the store.
 *
 * Clicking a workspace selects it as the App-level "active workspace" that new
 * terminals are created under (App owns this piece of state, not the store,
 * since it is purely a UI selection — not session/workspace data from the daemon).
 */
export function WorkspaceSidebar(props: {
  activeWorkspaceId: WorkspaceId | null;
  onSelectWorkspace: (id: WorkspaceId) => void;
}): JSX.Element {
  const { activeWorkspaceId, onSelectWorkspace } = props;
  const workspaces = useAppStore((s) => s.workspaces);
  const list = Object.values(workspaces).sort((a, b) => a.name.localeCompare(b.name));

  async function onAdd(): Promise<void> {
    const dir = await pickFolder();
    if (dir === null) return; // cancelled -> no-op
    const ws = await createWorkspace(basename(dir), dir);
    onSelectWorkspace(ws.id);
  }

  return (
    <aside
      aria-label="Workspaces"
      style={{
        width: 200,
        flexShrink: 0,
        background: theme.colors.bgElevated,
        borderRight: `1px solid ${theme.colors.border}`,
        color: theme.colors.text,
        display: "flex",
        flexDirection: "column",
      }}
    >
      <div
        style={{
          padding: "8px 12px",
          fontSize: 12,
          textTransform: "uppercase",
          color: theme.colors.textDim,
          letterSpacing: 0.5,
        }}
      >
        Workspaces
      </div>
      <ul style={{ listStyle: "none", margin: 0, padding: 0, flex: 1, overflowY: "auto" }}>
        {list.map((w) => {
          const selected = w.id === activeWorkspaceId;
          return (
            <li key={w.id}>
              <button
                type="button"
                title={w.rootPath}
                onClick={() => onSelectWorkspace(w.id)}
                style={{
                  display: "block",
                  width: "100%",
                  textAlign: "left",
                  padding: "6px 12px",
                  fontSize: 13,
                  whiteSpace: "nowrap",
                  overflow: "hidden",
                  textOverflow: "ellipsis",
                  border: "none",
                  cursor: "pointer",
                  color: selected ? theme.colors.text : theme.colors.textDim,
                  background: selected ? theme.colors.bg : "transparent",
                }}
              >
                {w.name}
              </button>
            </li>
          );
        })}
      </ul>
      <button
        type="button"
        aria-label="Add workspace"
        onClick={() => void onAdd()}
        style={{
          margin: 8,
          padding: "6px 10px",
          border: `1px solid ${theme.colors.border}`,
          background: theme.colors.bg,
          color: theme.colors.text,
          cursor: "pointer",
          fontSize: 13,
          borderRadius: 4,
        }}
      >
        + Add workspace
      </button>
    </aside>
  );
}
