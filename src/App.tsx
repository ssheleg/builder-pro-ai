import { useEffect, useState, type JSX } from "react";
import { useAppStore } from "./store/store";
import {
  onSessionCreated,
  onSessionStateChanged,
  onSessionExited,
  onWorkspaceCreated,
  onDaemonDisconnected,
  onDaemonReconnected,
  onDaemonIncompatible,
} from "./ipc/events";
import { listSessions, listWorkspaces, daemonStatus } from "./ipc/commands";
import type { WorkspaceId } from "./ipc/commands";
import { TerminalManager } from "./terminal/terminal-manager";
import { WorkspaceSidebar } from "./components/WorkspaceSidebar";
import { TerminalTabs } from "./components/TerminalTabs";
import { TerminalPane } from "./components/TerminalPane";
import { DaemonBanner } from "./components/DaemonBanner";
import { UpgradeDialog } from "./components/UpgradeDialog";
import { FilesRail } from "./components/FilesRail";
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
  const workspaces = useAppStore((s) => s.workspaces);
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
        // A daemon crash kills every shell; scrollback replay only covers up to the last
        // flush, so EVERY session needs a FRESH attach (new Replay + live Output) — not
        // just the visible one. Attach state now lives per-session in the manager (A1), so:
        //   1. resetAllAttachments() clears every session's attach flag up front, and
        //   2. we eagerly re-attach the VISIBLE session here (its pane never unmounts, so
        //      nothing else would trigger it). manager.attach() is a no-op if the flag were
        //      still set, hence the reset MUST precede it.
        // HIDDEN sessions re-attach LAZILY: their next tab-switch re-mounts the pane, whose
        // effect calls attach() unconditionally and the manager (flag now cleared) honors it.
        manager.resetAllAttachments();
        void hydrate(0).then(() => {
          if (disposed) return;
          const id = useAppStore.getState().activeSessionId;
          if (id !== null && manager.isOpened(id)) {
            void manager.attach(id);
          }
        });
      }),
    );
    track(
      onDaemonIncompatible(() => {
        // FATAL (Pv2 §6.2): the client's connection task has exited and will NOT reconnect —
        // set BOTH flags (see store.ts's doc comment for why they're separate); the dialog
        // opens immediately, and daemonIncompatible outlives it even if the user cancels.
        const s = useAppStore.getState();
        s.setDaemonIncompatible(true);
        s.setUpgradeDialogOpen(true);
      }),
    );

    /**
     * `list_workspaces`/`list_sessions` reject while the core hasn't finished connecting to
     * the daemon yet (no event marks that first success). Retry with a bounded backoff until
     * it succeeds; each attempt sets `daemonConnected` honestly (spec §13: never fake it).
     *
     * Finding [14]: only a SUCCESSFUL hydrate proves `sessions` reflects reality — `setHydrated`
     * flips to `true` here and nowhere else, so `UpgradeDialog` can tell "count known" from
     * "count never populated" (e.g. boot-incompatible, where the client slot is `None` and this
     * branch can never run).
     *
     * Finding [12]/F3: on a rejection, ALSO pull `daemon_status()` as a best-effort fallback for
     * the single-shot `daemon://incompatible` event, which can race webview `listen()`
     * registration and be lost forever. If the pull reveals `kind:"incompatible"`, set both store
     * flags — but only open the dialog on the FIRST detection (don't re-open it every retry tick
     * if the user already dismissed it) — mirroring exactly what `onDaemonIncompatible` does.
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
        s.setHydrated(true);
        // Default the active workspace so "+ New terminal" isn't stuck disabled after a
        // fresh launch that restores existing workspaces (no sidebar click has happened
        // yet). Prefer the active session's workspace; else the first hydrated workspace.
        setActiveWorkspaceId((current) => {
          if (current !== null) return current;
          const activeId = useAppStore.getState().activeSessionId;
          const activeMeta = activeId ? useAppStore.getState().sessions[activeId] : undefined;
          if (activeMeta) return activeMeta.workspaceId;
          return ws.length > 0 ? ws[0].id : null;
        });
      } catch {
        if (disposed) return;
        useAppStore.getState().setDaemonConnected(false);

        // Best-effort pull fallback (finding [12]/F3): never let a failure here break the
        // existing retry cadence — swallow it exactly like the event-driven path would simply
        // not fire.
        try {
          const status = await daemonStatus();
          if (disposed) return;
          if (status.kind === "incompatible") {
            const s = useAppStore.getState();
            const alreadyDetected = s.daemonIncompatible;
            s.setDaemonIncompatible(true);
            // Open the dialog only on the FIRST detection: if it was already flagged
            // incompatible (e.g. a prior poll tick, or the user already Cancel'd), a later poll
            // must not spam it back open.
            if (!alreadyDetected) {
              s.setUpgradeDialogOpen(true);
            }
          }
        } catch {
          // best-effort only — the bounded hydrate retry loop below is the honest fallback.
        }
        if (disposed) return;

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
  // `FilesRail` needs a real `Workspace` (its `roots`) to have anything to show; `undefined`
  // while no workspace is selected makes it render nothing (spec §6.4 — T11 will later also gate
  // it on `view === "workspace"` to hide it on Home; for now it tracks the active workspace only).
  const activeWorkspace = activeWorkspaceId ? workspaces[activeWorkspaceId] : undefined;

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
      <UpgradeDialog />
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
        <FilesRail workspace={activeWorkspace} />
      </div>
    </div>
  );
}
