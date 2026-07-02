import { useEffect, useState, type JSX } from "react";
import { useAppStore } from "./store/store";
import {
  onSessionCreated,
  onSessionStateChanged,
  onSessionExited,
  onWorkspaceCreated,
  onDaemonDisconnected,
  onDaemonReconnected,
} from "./ipc/events";
import { listSessions, listWorkspaces } from "./ipc/commands";
import type { WorkspaceId } from "./ipc/commands";
import { TerminalManager } from "./terminal/terminal-manager";
import { WorkspaceSidebar } from "./components/WorkspaceSidebar";
import { TerminalTabs } from "./components/TerminalTabs";
import { TerminalPane } from "./components/TerminalPane";
import { DaemonBanner } from "./components/DaemonBanner";
import { theme } from "./theme";
import type { UnlistenFn } from "@tauri-apps/api/event";

/** Module singleton (used by main.tsx). Tests inject a fake via the `manager` prop. */
export const terminalManager = new TerminalManager();

/** Bounded backoff for the initial-hydrate retry loop (spec §13 "no fake connected state"). */
const HYDRATE_RETRY_MS = [500, 1000, 2000, 5000];

/**
 * App shell (spec §2 UI, §12 frontend contract). Wires:
 * - IPC subscriptions (`session://*`, `workspace://*`, `daemon://*`) into the store,
 * - an initial `list_sessions` / `list_workspaces` hydrate with retry (the core connects to
 *   the daemon on setup but emits NO event on the initial successful connect — only
 *   `daemon://disconnected` on failure or `daemon://reconnected` on a LATER reconnect — so a
 *   command invoked before that connect completes rejects; hydrate retries until it succeeds),
 * - layout: `DaemonBanner` above `WorkspaceSidebar` | (`TerminalTabs` + the active pane).
 */
export function App(props?: { manager?: TerminalManager }): JSX.Element {
  const manager = props?.manager ?? terminalManager;

  const sessions = useAppStore((s) => s.sessions);
  const activeSessionId = useAppStore((s) => s.activeSessionId);
  const [activeWorkspaceId, setActiveWorkspaceId] = useState<WorkspaceId | null>(null);

  useEffect(() => {
    let disposed = false;
    const unlisteners: UnlistenFn[] = [];
    let retryTimer: ReturnType<typeof setTimeout> | undefined;

    const track = (p: Promise<UnlistenFn>): void => {
      void p.then((un) => {
        if (disposed) un();
        else unlisteners.push(un);
      });
    };

    track(
      onSessionCreated((m) => {
        const s = useAppStore.getState();
        s.upsertSession(m);
        if (s.activeSessionId === null) s.setActiveSession(m.id);
      }),
    );
    track(onSessionStateChanged((p) => useAppStore.getState().setLifecycle(p)));
    track(onSessionExited((p) => useAppStore.getState().markExited(p)));
    track(onWorkspaceCreated((w) => useAppStore.getState().upsertWorkspace(w)));
    track(onDaemonDisconnected(() => useAppStore.getState().setDaemonConnected(false)));
    track(
      onDaemonReconnected(() => {
        useAppStore.getState().setDaemonConnected(true);
        // Re-hydrate after a reconnect (spec §13: re-list sessions/workspaces).
        void hydrate(0);
      }),
    );

    /**
     * `list_workspaces`/`list_sessions` reject while the core hasn't finished connecting to
     * the daemon yet (no event marks that first success). Retry with a bounded backoff until
     * it succeeds; each attempt sets `daemonConnected` honestly (spec §13: never fake it).
     */
    async function hydrate(attempt: number): Promise<void> {
      if (disposed) return;
      try {
        const [ws, ss] = await Promise.all([listWorkspaces(), listSessions()]);
        if (disposed) return;
        const s = useAppStore.getState();
        for (const w of ws) s.upsertWorkspace(w);
        for (const m of ss) s.upsertSession(m);
        if (s.activeSessionId === null && ss.length > 0) {
          s.setActiveSession(ss[0].id);
        }
        s.setDaemonConnected(true);
      } catch {
        if (disposed) return;
        useAppStore.getState().setDaemonConnected(false);
        const delay = HYDRATE_RETRY_MS[Math.min(attempt, HYDRATE_RETRY_MS.length - 1)];
        retryTimer = setTimeout(() => void hydrate(attempt + 1), delay);
      }
    }

    void hydrate(0);

    return () => {
      disposed = true;
      if (retryTimer) clearTimeout(retryTimer);
      for (const un of unlisteners) un();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const activeSession = activeSessionId ? sessions[activeSessionId] : undefined;

  return (
    <div
      style={{
        display: "flex",
        flexDirection: "column",
        height: "100vh",
        background: theme.colors.bg,
        color: theme.colors.text,
        fontFamily: 'system-ui, -apple-system, "Segoe UI", Roboto, sans-serif',
      }}
    >
      <DaemonBanner />
      <div style={{ display: "flex", flex: 1, minHeight: 0 }}>
        <WorkspaceSidebar
          activeWorkspaceId={activeWorkspaceId}
          onSelectWorkspace={setActiveWorkspaceId}
        />
        <div style={{ display: "flex", flexDirection: "column", flex: 1, minWidth: 0 }}>
          <TerminalTabs manager={manager} activeWorkspaceId={activeWorkspaceId} />
          <div style={{ position: "relative", flex: 1, minHeight: 0 }}>
            {activeSession ? (
              // Only the ACTIVE session's pane is mounted; TerminalPane's unmount effect
              // calls manager.hide() (keep-alive) when a different tab becomes active.
              <TerminalPane sessionId={activeSession.id} manager={manager} />
            ) : (
              <div
                style={{
                  display: "flex",
                  alignItems: "center",
                  justifyContent: "center",
                  height: "100%",
                  color: theme.colors.textDim,
                  fontSize: 13,
                }}
              >
                {Object.keys(sessions).length === 0
                  ? "No terminals yet — pick a workspace and press + New terminal."
                  : "Select a terminal tab."}
              </div>
            )}
          </div>
        </div>
      </div>
    </div>
  );
}
