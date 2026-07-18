import type { JSX } from "react";
import { useAppStore } from "../store/store";
import { createSession, killSession } from "../ipc/commands";
import type { CreateSessionOpts, WorkspaceId } from "../ipc/commands";
import type { TerminalManager } from "../terminal/terminal-manager";
import { StatusDot } from "./StatusDot";
import { strings } from "../strings";

/**
 * Honest message for a rejected sessiond `CommandError` (a daemon round-trip — `create_session` /
 * `kill_session`, `src-tauri/src/commands.rs::CommandError`). A deliberately-duplicated local copy
 * mirroring `FileTree.tsx`'s identical helper (the repo keeps one per surface so each component
 * stays independently deployable — same rationale as `describeFsError`/`FilePreview`). The
 * vocabulary is `strings.errors.command.*`, kept in lockstep.
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
  const removeSession = useAppStore((s) => s.removeSession);
  const showToast = useAppStore((s) => s.showToast);

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
    // create_session pushes session://created; App's subscription upserts + activates it. A rejected
    // create (daemon down, spawn failure) is surfaced honestly instead of a silent no-op (BL-93).
    try {
      await createSession(activeWorkspaceId, opts);
    } catch (e) {
      showToast(strings.terminal.tabs.newTerminalFailed(describeCommandError(e)));
    }
  }

  async function onClose(sessionId: string): Promise<void> {
    // The owner asked to close this tab. `kill_session` may reject (daemon down / already gone), but
    // the UI must not leave a zombie tab (BL-93 / P-02: the old code disposed only on success and
    // NEVER removed the tab — `removeSession` was dead code, so a "closed" tab lingered as an exited
    // session). So: on failure surface an honest toast, and ALWAYS dispose the xterm instance and
    // drop the tab in a `finally` — whether the kill succeeded or not.
    try {
      await killSession(sessionId); // daemon kills the PTY + emits session://exited
    } catch (e) {
      showToast(strings.terminal.tabs.closeTerminalFailed(describeCommandError(e)));
    } finally {
      manager.dispose(sessionId); // tear down the xterm instance (real close)
      removeSession(sessionId); // remove the tab from the store — no zombie tab
    }
  }

  return (
    <div
      style={{
        display: "flex",
        alignItems: "stretch",
        gap: "var(--sp-1)",
        background: "var(--panel)",
        borderBottom: "1px solid var(--border)",
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
              // TE-04 (a11y): a focusable tab must also activate from the keyboard. Enter/Space
              // select the tab, matching the click. Guarded to the tab element itself so an
              // Enter/Space on the nested close button (which natively fires its own click) never
              // double-fires an activation.
              onKeyDown={(e) => {
                if (e.target !== e.currentTarget) return;
                if (e.key === "Enter" || e.key === " ") {
                  e.preventDefault();
                  setActiveSession(s.id);
                }
              }}
              style={{
                display: "flex",
                alignItems: "center",
                gap: "var(--sp-2)",
                padding: "var(--sp-2) var(--sp-3)",
                cursor: "pointer",
                color: selected ? "var(--ink)" : "var(--muted)",
                background: selected ? "var(--bg)" : "transparent",
                borderRight: "1px solid var(--border)",
                fontSize: "var(--fs-md)",
              }}
            >
              <StatusDot lifecycle={s.lifecycle} waitingForInput={s.waitingForInput} />
              <span>{s.title}</span>
              <button
                type="button"
                aria-label={strings.terminal.tabs.closeAria(s.title)}
                onClick={(e) => {
                  e.stopPropagation();
                  void onClose(s.id);
                }}
                style={{
                  border: "none",
                  background: "transparent",
                  color: "var(--muted)",
                  cursor: "pointer",
                  fontSize: "var(--fs-md)",
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
        aria-label={strings.terminal.tabs.newTerminalAria}
        disabled={!activeWorkspaceId}
        onClick={() => void onNewTerminal()}
        style={{
          border: "none",
          background: "transparent",
          color: activeWorkspaceId ? "var(--ink)" : "var(--muted)",
          cursor: activeWorkspaceId ? "pointer" : "not-allowed",
          padding: "var(--sp-2) var(--sp-3)",
          fontSize: "var(--fs-lg)",
        }}
      >
        {strings.terminal.tabs.newTerminal}
      </button>
    </div>
  );
}
