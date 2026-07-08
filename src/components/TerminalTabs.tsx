import type { JSX } from "react";
import { useAppStore } from "../store/store";
import { createSession, killSession } from "../ipc/commands";
import type { CreateSessionOpts, WorkspaceId } from "../ipc/commands";
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
  const workspaces = useAppStore((s) => s.workspaces);
  const selectedFile = useAppStore((s) => s.selectedFile);
  const activeSessionId = useAppStore((s) => s.activeSessionId);
  const setActiveSession = useAppStore((s) => s.setActiveSession);

  const list = Object.values(sessions).sort((a, b) => a.createdAt - b.createdAt);

  async function onNewTerminal(): Promise<void> {
    if (!activeWorkspaceId) return; // no workspace selected -> sidebar must create/select one first
    const activeWorkspace = workspaces[activeWorkspaceId];
    // Root-aware cwd (spec §6.3, "time-to-first-terminal in the right repo"): prefer the
    // tree-selected file's root, else the workspace's first root. Guarded against a
    // `selectedFile` left over from a DIFFERENT workspace (the store never clears it on a
    // workspace switch — `FilesRail` just stops rendering it) — only trust the selection when
    // it's actually one of THIS workspace's roots; otherwise fall back to roots[0] same as if
    // nothing were selected. No workspace/roots found (e.g. a hydration race) -> cwd stays
    // `undefined` and is OMITTED from the opts, matching the existing pre-T12 behavior (the
    // daemon defaults to roots[0] server-side).
    const selectedRoot =
      selectedFile && activeWorkspace?.roots.includes(selectedFile.root)
        ? selectedFile.root
        : undefined;
    const cwd = selectedRoot ?? activeWorkspace?.roots[0];
    const opts: CreateSessionOpts = cwd ? { cwd, cols: 80, rows: 24 } : { cols: 80, rows: 24 };
    // create_session pushes session://created; App's subscription upserts + activates it.
    await createSession(activeWorkspaceId, opts);
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
