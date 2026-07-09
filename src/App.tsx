import { useEffect, useState, type CSSProperties, type JSX } from "react";
import { useAppStore } from "./store/store";
import {
  onSessionCreated,
  onSessionStateChanged,
  onSessionExited,
  onWorkspaceCreated,
  onDaemonDisconnected,
  onDaemonReconnected,
  onDaemonIncompatible,
  onFsChanged,
  onFsWatchError,
  onWorkspaceUpdated,
} from "./ipc/events";
import { listSessions, listWorkspaces, daemonStatus } from "./ipc/commands";
import type { WorkspaceId } from "./ipc/commands";
import type { SessionMeta, Workspace } from "./ipc/types";
import { startWorkspaceWatch, stopWorkspaceWatch } from "./ipc/fs";
import { TerminalManager } from "./terminal/terminal-manager";
import { WorkspaceSidebar } from "./components/WorkspaceSidebar";
import { TerminalTabs } from "./components/TerminalTabs";
import { TerminalPane } from "./components/TerminalPane";
import { CommandStrip } from "./components/CommandStrip";
import { DaemonBanner } from "./components/DaemonBanner";
import { UpgradeDialog } from "./components/UpgradeDialog";
import { FilesRail } from "./components/FilesRail";
import { HomeView } from "./components/HomeView";
import { Toast } from "./components/Toast";
import { theme } from "./theme";
import type { UnlistenFn } from "@tauri-apps/api/event";

/** Module singleton (used by main.tsx). Tests inject a fake via the `manager` prop. */
export const terminalManager = new TerminalManager();

/** Bounded backoff for the initial-hydrate retry loop (spec §13 "no fake connected state"). */
const HYDRATE_RETRY_MS = [500, 1000, 2000, 5000];

/** Posix dirname (forward-slash only — matches `fs_explorer::FsEntry::rel_path`'s wire convention,
 * see `FileTree.tsx`'s identical helper). `""` (no `/`) means "the root directory itself". */
function dirnameOf(rel: string): string {
  const idx = rel.lastIndexOf("/");
  return idx === -1 ? "" : rel.slice(0, idx);
}

/**
 * Map each `fs://changed` entry's own rel-path (spec §5: the watcher emits the CHANGED ENTRY's
 * own path, e.g. `"src/new.ts"` for a file created inside `src`) to its CONTAINING DIRECTORY's
 * rel-path — `treeCache`/`invalidateDirs` are keyed by the directory a *listing* covers (see
 * `store.ts`'s `invalidateDirs` doc: it drops exact key matches only, no path arithmetic of its
 * own), never by the changed entry's own path. Without this mapping, `invalidateDirs` is handed a
 * FILE key that was never cached (`treeCache` only ever holds directory listings), so the
 * containing directory's listing is never dropped and the new/deleted/renamed entry never appears
 * until an unrelated manual collapse/expand — the whole live-watch DoD silently regressing to a
 * no-op for anything short of the `["*"]` overflow sentinel.
 *
 * `"src/new.ts"` -> `"src"`; a top-level `"top.txt"` -> `""` (the root listing itself, since a
 * top-level entry appearing/disappearing changes what the ROOT's own listing shows). The `"*"`
 * overflow sentinel (spec §5: >500 distinct paths in one debounced batch) passes through
 * UNCHANGED — it already means "everything under this root", not a real path to take a parent of.
 * Deduped so a batch with many siblings changing in the same directory invalidates that directory
 * exactly once.
 */
export function changedPathsToParentDirs(changedRelPaths: string[]): string[] {
  const out: string[] = [];
  const seen = new Set<string>();
  for (const rel of changedRelPaths) {
    const parent = rel === "*" ? "*" : dirnameOf(rel);
    if (!seen.has(parent)) {
      seen.add(parent);
      out.push(parent);
    }
  }
  return out;
}

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
  const view = useAppStore((s) => s.view);
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
    // `workspace://updated` (spec §3.3/§6.6) fires whenever a workspace's roots change
    // (add/removeWorkspaceRoot, from ANY client, including this one) — its payload IS a
    // Workspace, so upserting it is a direct passthrough.
    track(onWorkspaceUpdated((w) => useAppStore.getState().upsertWorkspace(w)));
    // Live file-watch signals (spec §5): a debounced batch of changed dirs is a POINT REFRESH
    // (`invalidateDirs` never touches `expanded` — a still-open directory just re-fetches), and
    // a dead watcher pauses live updates honestly rather than failing silently (spec §7).
    // `changedRelPaths` are the CHANGED ENTRIES' own paths (files or dirs); `invalidateDirs`
    // drops directory-listing cache keys, so every path must be mapped to its containing
    // directory first — see `changedPathsToParentDirs`'s doc for why (S2 final review A2: without
    // this mapping the fix was a silent no-op for anything short of the `["*"]` overflow case).
    track(
      onFsChanged((p) =>
        useAppStore
          .getState()
          .invalidateDirs(p.root, changedPathsToParentDirs(p.changedRelPaths)),
      ),
    );
    track(onFsWatchError(() => useAppStore.getState().setWatchPaused(true)));
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
  // while no workspace is selected makes it render nothing. Also gated on `view === "workspace"`
  // below (spec §6.1 "hidden on Home") — it never renders while the owner is on the Home screen.
  const activeWorkspace = activeWorkspaceId ? workspaces[activeWorkspaceId] : undefined;
  // Feeds `WorkspaceStatsChips` (spec §6.3): this workspace's own sessions only, not the whole
  // store (HomeView's stats strip is the whole-store equivalent — this one is scoped).
  const workspaceSessions = activeWorkspaceId
    ? Object.values(sessions).filter((m) => m.workspaceId === activeWorkspaceId)
    : [];

  /**
   * Live file-watch lifecycle (spec §5 "start on workspace activation, stop on switch/unmount;
   * nothing watched while the app is closed"). Only ever watches the CURRENTLY active workspace's
   * roots, and only while the owner is actually looking at a workspace (`view === "workspace"`) —
   * Home never triggers a watch, matching D4/D6 (no polling, no background work the owner cannot
   * see the point of). Depends on the `activeWorkspace` OBJECT (not just its id): `upsertWorkspace`
   * replaces the whole entry, so a `workspace://updated` push for the ACTIVE workspace (e.g. a
   * root added via `addWorkspaceRoot`) produces a new reference here too — the watch restarts
   * with the fresh `roots` list rather than silently missing the new root (spec §5 "starting
   * again replaces the previous"). `showIgnored` is read via `getState()` rather than as a
   * reactive dependency: a live toggle mid-watch does not itself restart the watch (FilesRail's
   * own toggle handler only invalidates the cache, matching its existing T10 behavior) — this
   * effect only needs the CURRENT value at the moment a new watch actually starts.
   *
   * S2 final review A4: a successful (re)start clears a stale `watchPaused` — a PRIOR workspace's
   * (or this same workspace's earlier) watch dying sets `watchPaused` true (the `onFsWatchError`
   * handler above), and without this, switching to a healthy workspace kept showing the amber
   * "live updates paused" banner forever, even though the NEW watch is fine. `cancelled` guards
   * against a fast unmount/root-swap: if this effect's cleanup already ran before the promise
   * settles, a DIFFERENT (newer) watch attempt owns `watchPaused` now and this stale resolution
   * must not clear it out from under that newer attempt.
   */
  useEffect(() => {
    if (view !== "workspace" || !activeWorkspace) return;
    let cancelled = false;
    const showIgnored = useAppStore.getState().showIgnored;
    void startWorkspaceWatch(activeWorkspace.roots, showIgnored).then(() => {
      if (!cancelled) useAppStore.getState().setWatchPaused(false);
    });
    return () => {
      cancelled = true;
      void stopWorkspaceWatch();
    };
  }, [view, activeWorkspace]);

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
      <Toast />
      <div style={{ display: "flex", flex: 1, minHeight: 0 }}>
        <WorkspaceSidebar
          activeWorkspaceId={activeWorkspaceId}
          onSelectWorkspace={setActiveWorkspaceId}
        />
        {view === "home" ? (
          // Attention-first Home (spec §6.2): sessions across ALL workspaces, "Пройти" jumps
          // straight into a waiting terminal. `setActiveWorkspaceId` is threaded down so that
          // jump can select the target workspace the same way the sidebar does.
          <HomeView manager={manager} setActiveWorkspaceId={setActiveWorkspaceId} />
        ) : (
          <>
            <div style={{ display: "flex", flexDirection: "column", flex: 1, minWidth: 0 }}>
              {/* Stat chips row (spec §6.1/§6.3): "Workspace: чипы + terminal tabs + command
                  strip" — sits above the tab strip. Renders nothing while no workspace is
                  active (mirrors FilesRail's own `!workspace` guard). */}
              <WorkspaceStatsChips
                workspace={activeWorkspace}
                sessions={workspaceSessions}
              />
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
              {/* Per-session OSC-133 command strip (spec §6.3): "strip under active terminal" —
                  only rendered once a session is active (mirrors TerminalPane's own gating). */}
              {activeSession && <CommandStrip sessionId={activeSession.id} />}
            </div>
            {/* Right rail: hidden on Home (spec §6.1) — only rendered in the workspace view. */}
            <FilesRail workspace={activeWorkspace} />
          </>
        )}
      </div>
    </div>
  );
}

const MONO_FONT = 'ui-monospace, SFMono-Regular, "SF Mono", Menlo, monospace';

type StatKey = "live" | "waiting" | "exited" | "roots";

const statChipStyle: CSSProperties = {
  display: "inline-flex",
  alignItems: "center",
  padding: "2px 8px",
  borderRadius: 999,
  border: `1px solid ${theme.colors.border}`,
  background: theme.colors.bg,
  fontFamily: MONO_FONT,
  fontSize: 11,
  fontVariantNumeric: "tabular-nums",
  color: theme.colors.textDim,
  cursor: "pointer",
};

/**
 * Stat chips row for the workspace view (spec §6.3): `N live · K waiting · M exited · R roots`
 * for the ACTIVE workspace's sessions. Mirrors HomeView's three-way session split — waiting
 * (`waitingForInput`, exited always wins over a stale flag), live (`isActive` and not waiting,
 * i.e. actually running), exited (`!isActive` with an `exited` lifecycle) — scoped to ONE
 * workspace's sessions instead of the whole store, plus the workspace's roots count (multi-root,
 * spec §3.3). Clicking a chip toggles a minimal inline detail list (session titles, or root
 * paths for the roots chip); only one chip's detail is open at a time (design-system.md §1
 * "detail is one drill-down away, never on the first screen").
 */
function WorkspaceStatsChips(props: {
  workspace: Workspace | undefined;
  sessions: SessionMeta[];
}): JSX.Element | null {
  const { workspace, sessions } = props;
  const [open, setOpen] = useState<StatKey | null>(null);

  if (!workspace) return null;

  const waiting = sessions.filter((m) => m.waitingForInput && m.lifecycle.kind !== "exited");
  const live = sessions.filter((m) => m.isActive && !m.waitingForInput);
  const exited = sessions.filter((m) => !m.isActive && m.lifecycle.kind === "exited");

  const chips: { key: StatKey; label: string; items: string[] }[] = [
    { key: "live", label: `${live.length} live`, items: live.map((m) => m.title) },
    { key: "waiting", label: `${waiting.length} waiting`, items: waiting.map((m) => m.title) },
    { key: "exited", label: `${exited.length} exited`, items: exited.map((m) => m.title) },
    { key: "roots", label: `${workspace.roots.length} roots`, items: workspace.roots },
  ];
  const openChip = chips.find((c) => c.key === open);

  return (
    <div
      data-testid="workspace-stats"
      style={{
        display: "flex",
        flexDirection: "column",
        gap: 4,
        padding: "6px 12px",
        borderBottom: `1px solid ${theme.colors.border}`,
        background: theme.colors.bgElevated,
      }}
    >
      <div style={{ display: "flex", gap: 6 }}>
        {chips.map((chip) => (
          <button
            key={chip.key}
            type="button"
            data-testid={`workspace-stat-${chip.key}`}
            aria-pressed={open === chip.key}
            onClick={() => setOpen((cur) => (cur === chip.key ? null : chip.key))}
            style={{
              ...statChipStyle,
              borderColor: open === chip.key ? theme.colors.accent : theme.colors.border,
              color: open === chip.key ? theme.colors.text : theme.colors.textDim,
            }}
          >
            {chip.label}
          </button>
        ))}
      </div>
      {openChip && (
        <div
          data-testid="workspace-stat-detail"
          style={{ fontSize: 11, fontFamily: MONO_FONT, color: theme.colors.textDim }}
        >
          {openChip.items.length === 0 ? "—" : openChip.items.join(" · ")}
        </div>
      )}
    </div>
  );
}
