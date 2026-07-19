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
  onOrchdProjectsChanged,
  onOrchdGoalsChanged,
  onOrchdIdeasChanged,
  onOrchdInsightsChanged,
  onOrchdTasksChanged,
  onOrchdRulesetChanged,
  onOrchdGraphChanged,
  onOrchdDown,
  onOrchdUp,
  onOrchdIncompatible,
  onOrchdMcpServersChanged,
  onOrchdMcpToolsChanged,
  onOrchdMcpArtifactsChanged,
  onOrchdMcpInvocationLogged,
  onOrchdConnectorsChanged,
  onOrchdSkillsChanged,
  onOrchdPoliciesChanged,
  onOrchdResearchRunsChanged,
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
import { OrchdDownBanner } from "./components/OrchdDownBanner";
import { OrchdUpgradeBanner } from "./components/OrchdUpgradeBanner";
import { StorageBanner } from "./components/StorageBanner";
import { QuickCapture } from "./components/QuickCapture";
import { FilesRail } from "./components/FilesRail";
import { HomeView } from "./components/HomeView";
import { ProjectPanel } from "./components/ProjectPanel";
import { ExtPanel } from "./components/ext/ExtPanel";
import { Toast } from "./components/Toast";
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
  const activeProjectId = useAppStore((s) => s.activeProjectId);
  const orchdDown = useAppStore((s) => s.orchdDown);
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

    // orchd domain events (spec §9/§10, D6, S3 T13): coarse invalidation pushes — each
    // `orchd://*-changed` names ONLY what changed, so the handler just re-fetches that list from
    // the daemon via the store's matching `refresh*` action. `goals-changed`/`tasks-changed` name
    // the ONE project whose list changed and re-fetch ONLY that project's entry, never every
    // project's (see `store.ts`'s `goalsByProject`/`tasksByProject` docs).
    track(onOrchdProjectsChanged(() => void useAppStore.getState().refreshProjects()));
    track(onOrchdGoalsChanged((p) => void useAppStore.getState().refreshGoals(p.projectId)));
    track(onOrchdIdeasChanged(() => void useAppStore.getState().refreshIdeas()));
    track(onOrchdInsightsChanged(() => void useAppStore.getState().refreshInsights()));
    track(onOrchdTasksChanged((p) => void useAppStore.getState().refreshTasks(p.projectId)));
    track(
      onOrchdRulesetChanged((p) => {
        const key = p.scope === "global" ? "global" : `project:${p.projectId}`;
        void useAppStore.getState().refreshRuleset(key);
      }),
    );
    // Unconditional re-fetch, mirroring `goals-changed`/`tasks-changed` above (audit #5.1): there
    // is no "graph panel currently open" gating to mirror — none of the S3 precedents this event
    // was modeled on gate their refresh on view/activeProjectId, so this doesn't invent one either.
    track(onOrchdGraphChanged((p) => void useAppStore.getState().refreshGraph(p.projectId)));
    // MCP coarse-invalidation events (S-EXT §8, T8): same unconditional-refresh precedent as
    // `orchd://graph-changed` above — no "Extensions view currently open" gating to mirror.
    track(onOrchdMcpServersChanged(() => void useAppStore.getState().refreshMcpServers()));
    track(
      onOrchdMcpToolsChanged((p) => void useAppStore.getState().refreshMcpTools(p.serverId)),
    );
    track(onOrchdMcpArtifactsChanged(() => void useAppStore.getState().refreshMcpArtifacts()));
    // `orchd://mcp-invocation-logged` (S-EXT §8, T18: the «Log» tab) — same unconditional-
    // refresh precedent as the MCP trio above; the store's `invocations` slice is whole-store,
    // un-scoped, so any server's newly-logged invocation refetches the whole list.
    track(onOrchdMcpInvocationLogged(() => void useAppStore.getState().refreshInvocations()));
    // Connectors coarse-invalidation event (S-EXT §8, T13b): same unconditional-refresh
    // precedent as the MCP trio above — no "Connectors tab currently open" gating to mirror.
    track(onOrchdConnectorsChanged(() => void useAppStore.getState().refreshAccounts()));
    // Skills coarse-invalidation event (S-EXT §8, D11, Q14, T17): same unconditional-refresh
    // precedent as the MCP/Connectors events above — no "Skills tab currently open" gating to
    // mirror.
    track(onOrchdSkillsChanged(() => void useAppStore.getState().refreshSkills()));
    // Trust policy-cap coarse-invalidation event (S-EXT §4/§6/§8, BL-22, T18): same
    // unconditional-refresh precedent as `ConnectorsChanged`/`SkillsChanged` above.
    track(onOrchdPoliciesChanged(() => void useAppStore.getState().refreshPolicies()));
    // S-IDEA research coarse-invalidation event (spec §5/§8, task T6): same unconditional-refresh
    // precedent as the MCP/Connectors/Skills/Trust events above — no "idea currently open" gating
    // to mirror. `ideaId` is defensively treated as possibly `null` (see
    // `ResearchRunsChangedPayload`'s doc, `events.ts`) even though the run driver only ever
    // broadcasts a concrete idea id in practice.
    track(
      onOrchdResearchRunsChanged((p) => {
        if (p.ideaId !== null) void useAppStore.getState().refreshResearchRuns(p.ideaId);
      }),
    );
    // orchd connection state (spec §9): a DIRECT 1:1 mapping (see
    // `broker.rs::map_orchd_conn_state`'s doc) — unlike the sessiond trio there is no
    // reconnect-tracking scheme, `orchd://down`/`orchd://up` just flip `orchdDown`.
    track(onOrchdDown(() => useAppStore.getState().setOrchdDown(true)));
    track(
      onOrchdUp(() => {
        // `orchd://up` is the "orchd is now reachable" signal — for the INITIAL connect as well
        // as every later reconnect (broker.rs fires it on every `Connected`). The initial
        // `refreshProjects()` below (fired once, right after these subscriptions register) races
        // orchd's async bring-up: `bring_up_orchd` is spawned and `setup()` returns immediately,
        // and orchd's bounded connect-retry can take up to ~4s — so on a cold boot that race is
        // routinely LOST, that first fetch rejects with `Disconnected`, and without this refetch
        // the slices would stay empty permanently until some unrelated `orchd://*-changed` push
        // happened to fire.
        //
        // Spec D8 (BL-92): a reconnect must rehydrate EVERY LIVE SLICE, not just projects — during
        // the outage any coarse `orchd://*-changed` push is lost, so anything the frontend holds
        // can be stale until an unrelated later change. So refetch: projects; the open project's
        // goals/ideas/insights/tasks/ruleset/graph; the Extensions slices (servers/artifacts/
        // accounts/skills/policies/invocations); research runs for every idea currently holding
        // runs; the global ruleset when its surface has been opened; and the storage-degradation
        // status (D3).
        const s = useAppStore.getState();
        s.setOrchdDown(false);
        void s.refreshProjects();
        const projectId = s.activeProjectId;
        if (projectId !== null) {
          void s.refreshGoals(projectId);
          void s.refreshTasks(projectId);
          void s.refreshIdeas();
          void s.refreshInsights();
          void s.refreshRuleset(`project:${projectId}`);
          // S4 §7 T7 (T6 review must-not-drop item (b)): the graph tab mirrors every sibling
          // domain surface's reconnect-refresh exactly — an open project panel's Graph tab must
          // not stay stale after an orchd reconnect any more than Goals/Tasks/Ideas/Insights do.
          void s.refreshGraph(projectId);
        }
        // Extensions slices (S-EXT §8): whole-store, project-independent — refetch unconditionally
        // (spec D8's "mcp servers + artifacts + accounts + skills + policies + invocations").
        void s.refreshMcpServers();
        void s.refreshMcpArtifacts();
        void s.refreshAccounts();
        void s.refreshSkills();
        void s.refreshPolicies();
        void s.refreshInvocations();
        // Research runs self-heal on reconnect for every idea currently holding runs in the store
        // (spec D8) — the mounted `ResearchPane` self-poll covers the visible pane; this covers
        // every loaded idea whether or not its pane is on screen right now.
        for (const ideaId of Object.keys(s.researchRunsByIdea)) {
          void s.refreshResearchRuns(ideaId);
        }
        // The global ruleset only when its surface has actually been opened — proxied by "already
        // loaded into the store" (there is no always-mounted global-rules surface; a project
        // ruleset is handled above). Mirrors the "for every idea currently holding runs" pattern.
        if ("global" in s.rulesets) void s.refreshRuleset("global");
        // Storage-degradation mode (spec D3, BL-94) — fixed at boot, so pull it on every reconnect.
        void s.refreshStorageStatus();
      }),
    );
    // FATAL (Pv2 §6.2, mirrors `onDaemonIncompatible` above): the orchd client's connection task
    // has exited and will not reconnect on its own. The generalized `UpgradeDialog` (T19, spec
    // §10) reads both daemons' flag pairs and renders whichever is relevant (sessiond wins if
    // both are set) — so, exactly like `onDaemonIncompatible` above, set BOTH the fatal flag AND
    // the dialog's own visibility flag here; `orchdIncompatible` outlives Cancel even if the user
    // dismisses the dialog.
    track(
      onOrchdIncompatible(() => {
        const s = useAppStore.getState();
        s.setOrchdIncompatible(true);
        s.setOrchdUpgradeDialogOpen(true);
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
    // Initial orchd hydrate (spec §10, S3 T13): mirrors `hydrate(0)` above but for the domain
    // slice — fire once, right after every subscription above is registered. This has no
    // bounded-retry loop of its own: a failure (e.g. orchd not yet connected — its async
    // bring-up routinely loses the race with this call on a cold boot) is surfaced honestly via
    // `refreshProjects`'s own toast (`store.ts`), and the `onOrchdUp` handler above re-fires this
    // exact load once orchd actually connects — so a lost initial race self-heals rather than
    // leaving `projects` empty forever. Later `orchd://projects-changed` pushes keep it live
    // thereafter.
    void useAppStore.getState().refreshProjects();
    // Storage-degradation status (spec D3, BL-94): pulled once on the initial connect, mirroring
    // the `refreshProjects()` above. A lost cold-boot race self-heals via `onOrchdUp` (which also
    // refetches it) exactly like the project list does.
    void useAppStore.getState().refreshStorageStatus();

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
    void startWorkspaceWatch(activeWorkspace.roots, showIgnored)
      .then(() => {
        if (!cancelled) useAppStore.getState().setWatchPaused(false);
      })
      .catch(() => {
        // A failed watch-start must surface as paused, never leave the tree falsely reading
        // "live" (C2) — and it must not escape as an unhandledrejection.
        if (!cancelled) useAppStore.getState().setWatchPaused(true);
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
        background: "var(--bg)",
        color: "var(--ink)",
        fontFamily: "var(--font-ui)",
      }}
    >
      <DaemonBanner />
      {/* Persistent honest storage-degradation banner (spec D3, BL-94): self-reads `storageStatus`
          and renders only for the two non-persistent modes — mounted once, globally. */}
      <StorageBanner />
      {/* Re-entry banner for a cancelled orchd upgrade (BL-96, spec D8): self-reads
          `orchdIncompatible && !orchdUpgradeDialogOpen` and re-opens the mandatory dialog — mirrors
          the sessiond `DaemonBanner` pattern for the second daemon. */}
      <OrchdUpgradeBanner />
      {/* Shared orchd connectivity banner (spec §10/§11): "every domain surface" is satisfied by
          mounting it once, globally, next to the sessiond equivalent above — it is purely
          presentational (`OrchdDownBanner.tsx`), so this is the one place that reads `orchdDown`
          and decides whether to render it at all. */}
      {orchdDown && <OrchdDownBanner />}
      <UpgradeDialog />
      {/* Global ⌘K idea-capture overlay (spec §10, task-19): mounted exactly ONCE, app-wide, so
          its internal keydown listener is live regardless of which view is showing. */}
      <QuickCapture />
      <Toast />
      <div style={{ display: "flex", flex: 1, minHeight: 0 }}>
        <WorkspaceSidebar
          activeWorkspaceId={activeWorkspaceId}
          onSelectWorkspace={setActiveWorkspaceId}
        />
        {view === "home" ? (
          // Attention-first Home (spec §6.2): sessions across ALL workspaces, "Go" jumps
          // straight into a waiting terminal. `setActiveWorkspaceId` is threaded down so that
          // jump can select the target workspace the same way the sidebar does.
          <HomeView manager={manager} setActiveWorkspaceId={setActiveWorkspaceId} />
        ) : view === "project" ? (
          // S3 project panel (spec §10, T18): guarded on `activeProjectId` non-null — `openProject`
          // is the only setter of `view: "project"` and always sets both together (store.ts), but
          // this stays defensive rather than assuming that invariant can never be violated.
          activeProjectId !== null && <ProjectPanel projectId={activeProjectId} />
        ) : view === "ext" ? (
          // S-EXT «Extensions» panel (spec §8, T8): MCP servers/tools/connectors/skills.
          <ExtPanel />
        ) : (
          <>
            <div style={{ display: "flex", flexDirection: "column", flex: 1, minWidth: 0 }}>
              {/* Stat chips row (spec §6.1/§6.3): "Workspace: chips + terminal tabs + command
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
                      color: "var(--muted)",
                      fontSize: "var(--fs-md)",
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

const MONO_FONT = "var(--font-mono)";

type StatKey = "live" | "waiting" | "exited" | "roots";

const statChipStyle: CSSProperties = {
  display: "inline-flex",
  alignItems: "center",
  padding: "2px var(--sp-2)",
  borderRadius: 999,
  border: "1px solid var(--border)",
  background: "var(--panel)",
  fontFamily: MONO_FONT,
  fontSize: "var(--fs-xs)",
  fontVariantNumeric: "tabular-nums",
  color: "var(--muted)",
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
        gap: "var(--sp-1)",
        padding: "var(--sp-2) var(--sp-3)",
        borderBottom: "1px solid var(--border)",
        background: "var(--panel-2)",
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
              borderColor: open === chip.key ? "var(--accent)" : "var(--border)",
              color: open === chip.key ? "var(--ink)" : "var(--muted)",
            }}
          >
            {chip.label}
          </button>
        ))}
      </div>
      {openChip && (
        <div
          data-testid="workspace-stat-detail"
          style={{ fontSize: "var(--fs-xs)", fontFamily: MONO_FONT, color: "var(--muted)" }}
        >
          {openChip.items.length === 0 ? "—" : openChip.items.join(" · ")}
        </div>
      )}
    </div>
  );
}
