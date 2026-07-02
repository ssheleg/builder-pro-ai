import { useAppStore } from "../store/store";
import { createSession, killSession } from "../ipc/commands";
import type { WorkspaceId } from "../ipc/commands";
import type { TerminalManager } from "../terminal/terminal-manager";
import { StatusDot } from "./StatusDot";
import { theme } from "../theme";

/**
 * Tab strip: one tab per live session (active switch is metadata-only — `setActiveSession`
 * — so hidden terminals stay alive, spec §12 keep-alive). "New terminal" creates a session
 * in the App-level active workspace (disabled if none is selected). Closing a tab kills the
 * session and disposes its Terminal (the only place `dispose()` is called for a user close).
 */
export function TerminalTabs(props: {
  manager: TerminalManager;
  activeWorkspaceId: WorkspaceId | null;
}): JSX.Element {
  const { manager, activeWorkspaceId } = props;
  const sessions = useAppStore((s) => s.sessions);
  const activeSessionId = useAppStore((s) => s.activeSessionId);
  const setActiveSession = useAppStore((s) => s.setActiveSession);

  const list = Object.values(sessions).sort((a, b) => a.createdAt - b.createdAt);

  async function onNewTerminal(): Promise<void> {
    if (!activeWorkspaceId) return; // no workspace selected -> sidebar must create/select one first
    // create_session pushes session://created; App's subscription upserts + activates it.
    await createSession(activeWorkspaceId, { cols: 80, rows: 24 });
  }

  async function onClose(sessionId: string): Promise<void> {
    await killSession(sessionId); // daemon kills the PTY + emits session://exited
    manager.dispose(sessionId); // tear down the xterm instance (real close)
  }

  return (
    <div
      style={{
        display: "flex",
        alignItems: "stretch",
        gap: 2,
        background: theme.colors.bgElevated,
        borderBottom: `1px solid ${theme.colors.border}`,
      }}
    >
      <div role="tablist" style={{ display: "flex", alignItems: "stretch", flex: 1, minWidth: 0 }}>
        {list.map((s) => {
          const selected = s.id === activeSessionId;
          return (
            <div
              key={s.id}
              role="tab"
              aria-selected={selected}
              tabIndex={0}
              onClick={() => setActiveSession(s.id)}
              style={{
                display: "flex",
                alignItems: "center",
                gap: 6,
                padding: "6px 10px",
                cursor: "pointer",
                color: selected ? theme.colors.text : theme.colors.textDim,
                background: selected ? theme.colors.bg : "transparent",
                borderRight: `1px solid ${theme.colors.border}`,
                fontSize: 13,
              }}
            >
              <StatusDot lifecycle={s.lifecycle} waitingForInput={s.waitingForInput} />
              <span>{s.title}</span>
              <button
                type="button"
                aria-label={`Close ${s.title}`}
                onClick={(e) => {
                  e.stopPropagation();
                  void onClose(s.id);
                }}
                style={{
                  border: "none",
                  background: "transparent",
                  color: theme.colors.textDim,
                  cursor: "pointer",
                  fontSize: 13,
                  lineHeight: 1,
                }}
              >
                ×
              </button>
            </div>
          );
        })}
      </div>
      <button
        type="button"
        aria-label="New terminal"
        disabled={!activeWorkspaceId}
        onClick={() => void onNewTerminal()}
        style={{
          border: "none",
          background: "transparent",
          color: activeWorkspaceId ? theme.colors.text : theme.colors.textDim,
          cursor: activeWorkspaceId ? "pointer" : "not-allowed",
          padding: "6px 12px",
          fontSize: 16,
        }}
      >
        + New terminal
      </button>
    </div>
  );
}
