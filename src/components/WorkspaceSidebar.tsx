import type { JSX } from "react";
import { useAppStore } from "../store/store";
import { pickFolder, createWorkspace } from "../ipc/commands";
import type { WorkspaceId } from "../ipc/commands";
import { theme } from "../theme";

function basename(path: string): string {
  const parts = path.replace(/\/+$/, "").split("/");
  return parts[parts.length - 1] || path;
}

/**
 * Left rail: pure navigation (spec §6.1 "slimmed to pure navigation"). A `⌂ Home` item on top
 * (sets the top-level `view` to `"home"`, spec §6.2 attention-first Home) followed by the
 * workspace list + folder picker. `pickFolder` is the CORE-ONLY native dialog (spec §6.1); on a
 * chosen dir we create a workspace named after its basename. The daemon validates the root
 * (spec §16) and pushes workspace://created, which App's subscription upserts into the store.
 *
 * Clicking a workspace selects it as the App-level "active workspace" that new terminals are
 * created under (App owns this piece of state, not the store, since it is purely a UI selection
 * — not session/workspace data from the daemon) AND switches `view` to `"workspace"` — selecting
 * a workspace is how the owner leaves Home (spec §6.1 "workspace list... selecting a workspace
 * sets activeWorkspaceId AND view=\"workspace\"").
 */
export function WorkspaceSidebar(props: {
  activeWorkspaceId: WorkspaceId | null;
  onSelectWorkspace: (id: WorkspaceId) => void;
}): JSX.Element {
  const { activeWorkspaceId, onSelectWorkspace } = props;
  const workspaces = useAppStore((s) => s.workspaces);
  const view = useAppStore((s) => s.view);
  const setView = useAppStore((s) => s.setView);
  const list = Object.values(workspaces).sort((a, b) => a.name.localeCompare(b.name));

  function onSelectWorkspaceAndNavigate(id: WorkspaceId): void {
    onSelectWorkspace(id);
    setView("workspace");
  }

  async function onAdd(): Promise<void> {
    const dir = await pickFolder();
    if (dir === null) return; // cancelled -> no-op
    const ws = await createWorkspace(basename(dir), dir);
    onSelectWorkspaceAndNavigate(ws.id);
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
      <button
        type="button"
        aria-label="Home"
        aria-current={view === "home" ? "true" : undefined}
        onClick={() => setView("home")}
        style={{
          display: "block",
          width: "100%",
          textAlign: "left",
          padding: "8px 12px",
          fontSize: 13,
          fontWeight: 600,
          border: "none",
          borderBottom: `1px solid ${theme.colors.border}`,
          cursor: "pointer",
          color: view === "home" ? theme.colors.text : theme.colors.textDim,
          background: view === "home" ? theme.colors.bg : "transparent",
        }}
      >
        ⌂ Home
      </button>
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
          const selected = view === "workspace" && w.id === activeWorkspaceId;
          return (
            <li key={w.id}>
              <button
                type="button"
                title={w.rootPath}
                onClick={() => onSelectWorkspaceAndNavigate(w.id)}
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
