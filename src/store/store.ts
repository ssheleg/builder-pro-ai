import { create } from "zustand";
import type { SessionMeta, Workspace } from "../ipc/types";
import type { SessionId, WorkspaceId } from "../ipc/commands";
import type { StateChangedPayload, ExitedPayload } from "../ipc/events";
import type { FsEntry } from "../ipc/fs";

/**
 * Global app state (spec §12). METADATA ONLY — PTY bytes never enter this store;
 * they are written straight to xterm via the terminal Channel (see terminal-manager,
 * Task 21).
 *
 * `upsertSession`/`upsertWorkspace` are idempotent by id: the daemon broadcasts
 * `session://created` to ALL clients including the create originator, so a duplicate
 * upsert of the same (or updated) record must be harmless — insert-or-replace, never
 * throw or duplicate.
 */
export interface AppState {
  sessions: Record<SessionId, SessionMeta>;
  workspaces: Record<WorkspaceId, Workspace>;
  activeSessionId: SessionId | null;
  daemonConnected: boolean;
  /**
   * Honest daemon state (Pv2 §6.2-6.3): set `true` by `daemon://incompatible`, which is FATAL
   * (the client's connection task has exited and will NOT reconnect, unlike a plain disconnect).
   * Stays `true` until the app restarts (a successful upgrade resets everything via
   * `app.restart()`) — Cancel on the upgrade dialog must NOT clear it, or `DaemonBanner` would
   * revert to its "reconnecting…" copy, which would be a lie.
   */
  daemonIncompatible: boolean;
  /** Pure UI visibility for `UpgradeDialog`. `true` when the event fires; `false` on Cancel;
   * re-openable from `DaemonBanner`'s action. Independent of `daemonIncompatible` (see above). */
  upgradeDialogOpen: boolean;
  /**
   * Honest failure surface for the upgrade flow (finding [13], spec §6.2.4): `upgradeDaemon()`
   * never resolves on the happy path (the daemon restart kills this webview), but a REJECTED
   * promise (e.g. `CommandError::UpgradeFailed` from a TCC/MDM-denied `launchctl kickstart`) must
   * not vanish silently. `null` = no error to show. Cleared whenever the dialog (re)opens fresh
   * (`setUpgradeDialogOpen(true)`) or the user retries, so a stale error never lingers past a new
   * attempt.
   */
  upgradeError: string | null;
  /**
   * Set `true` only after the FIRST successful hydrate (`list_sessions`/`list_workspaces` both
   * resolving) — finding [14]: while `false`, `sessions` may simply not have been populated yet
   * (e.g. the client slot is `None` at boot-incompatible, so hydrate can never succeed), and any
   * "N live sessions" count derived from the store would silently understate reality. Consumers
   * (e.g. `UpgradeDialog`) must branch on this flag before trusting a session count. Never reset
   * back to `false` once true (a later disconnect doesn't un-hydrate the snapshot already held).
   */
  hydrated: boolean;

  /**
   * Top-level navigation (spec §6.6/§6.2): `"home"` is the attention-first Home view over ALL
   * terminals across workspaces; `"workspace"` is the existing per-workspace terminal layout.
   * Defaults to `"home"` — the owner's daily loop starts there, never mid-workspace.
   */
  view: "home" | "workspace";

  /**
   * File-explorer slice (spec §6.6/§6.4). Every keyed map here uses the SAME key format:
   * `` `${root}\t${rel}` `` (tab-separated — `rel` itself may legitimately contain `/`, so a
   * tab avoids any ambiguity a `/`-joined key would have between a root boundary and a path
   * separator). `rel === ""` addresses the root directory itself.
   */
  /** Which directories are expanded in the tree, keyed `` `${root}\t${rel}` ``. A `Record` of
   * `true` (not `boolean`) so "expanded" is exactly "key present" — no stale `false` entries to
   * prune. */
  expanded: Record<string, true>;
  /** Lazily-fetched `listDir` results, keyed `` `${root}\t${rel}` ``. Absence means "not yet
   * fetched" (or invalidated) — never distinguished from an empty directory by anything other
   * than key presence. */
  treeCache: Record<string, FsEntry[]>;
  /** The file currently shown in the preview pane, or `null` when nothing is selected. */
  selectedFile: { root: string; rel: string } | null;
  /** Whether gitignored entries are shown (dimmed) in the tree. Defaults to `false` (spec §4.2:
   * ignored entries omitted by default). */
  showIgnored: boolean;
  /** Right-rail (files) visibility. Defaults to `false` (collapsed). */
  filesRailOpen: boolean;
  /** `true` while the live watch is paused after an `fs://watch-error` (spec §5/§7): the UI shows
   * a "live updates paused — refresh" affordance. Cleared on reactivation. */
  watchPaused: boolean;

  /**
   * Queue-of-ONE toast message (design-system.md Toast atom, spec §7 "honest error surface" —
   * every async failure is a toast with the mapped human message, never console-only). `null`
   * means no toast is showing. `showToast` REPLACES whatever is currently shown — there is no
   * queue behind it, matching the design-system's "one inbox" spirit applied to transient
   * notices: at most one thing asks for the owner's attention via a toast at a time.
   */
  toast: string | null;

  /** Insert or replace a session by `meta.id`. Idempotent. */
  upsertSession: (meta: SessionMeta) => void;
  /** Delete a session; clears `activeSessionId` if it pointed at the removed session. */
  removeSession: (id: SessionId) => void;
  /**
   * Apply a `session://state-changed` payload: updates `lifecycle`/`waitingForInput`/`cwd`
   * on the matching session. No-op if the session isn't in the map (e.g. a stale/late
   * event after removal).
   */
  setLifecycle: (p: StateChangedPayload) => void;
  /**
   * Apply a `session://exited` payload: sets `isActive:false`, clears `waitingForInput` (a
   * finished process is never waiting for input — the honest state for stats/StatusDot/HomeView;
   * the live event carries no such field, so a session that exited/crashed while blocked on stdin
   * must not keep a stale `true` forever), and an `{kind:"exited"}` lifecycle carrying the exit
   * code/signal. No-op if the session isn't in the map.
   */
  markExited: (p: ExitedPayload) => void;
  setDaemonConnected: (connected: boolean) => void;
  setDaemonIncompatible: (v: boolean) => void;
  setUpgradeDialogOpen: (v: boolean) => void;
  /** Set the upgrade-failure message (or clear it with `null`). See `upgradeError` doc above. */
  setUpgradeError: (v: string | null) => void;
  /** Set `true` after the first successful hydrate. See `hydrated` doc above. */
  setHydrated: (v: boolean) => void;
  /** Insert or replace a workspace by `ws.id`. Idempotent. Also the `workspace://updated`
   * listener's handler (spec §6.6): that event's payload IS a `Workspace`, so wiring it is
   * literally `onWorkspaceUpdated(upsertWorkspace)` — no separate action needed. */
  upsertWorkspace: (ws: Workspace) => void;
  setActiveSession: (id: SessionId | null) => void;

  /** Switch the top-level view. See `view`'s doc above. */
  setView: (v: "home" | "workspace") => void;
  /** Set (`open=true`) or clear (`open=false`) one directory's expanded flag. */
  setExpanded: (root: string, rel: string, open: boolean) => void;
  /** Insert or replace one directory's cached listing. */
  cacheDir: (root: string, rel: string, entries: FsEntry[]) => void;
  /**
   * Apply an `fs://changed` batch (spec §5) to `treeCache` for `root` — a POINT REFRESH, never a
   * collapse: `expanded` is deliberately left UNTOUCHED, so a directory the owner had open stays
   * open and `FileTree`'s own effect (spec §6.4) re-fetches it since it's now uncached, with no
   * explicit re-expand click needed. Clearing `expanded` here would collapse the whole tree on
   * every file an agent writes, which is the opposite of the intended live-refresh UX. `rels` is
   * the event's `changedRelPaths`, treated as literal directory keys to drop (the caller is
   * responsible for mapping a changed FILE path to its containing directory's `rel` first — this
   * action itself does no path arithmetic). `rels === ["*"]` (the watcher's overflow sentinel,
   * spec §5: >500 distinct paths in one debounced batch) drops EVERY `treeCache` entry under
   * `root` — i.e. "refresh everything expanded under this root" — while entries for every OTHER
   * root are left untouched. Otherwise, only the exact `` `${root}\t${rel}` `` keys named in
   * `rels` are dropped.
   */
  invalidateDirs: (root: string, rels: string[]) => void;
  /** Set (or clear, with `null`) the file shown in the preview pane. */
  setSelectedFile: (sel: { root: string; rel: string } | null) => void;
  /** Flip `showIgnored`. */
  toggleShowIgnored: () => void;
  /** Set the files right-rail's open/closed state. */
  setFilesRailOpen: (b: boolean) => void;
  /** Set `watchPaused`. See its doc above. */
  setWatchPaused: (b: boolean) => void;

  /**
   * Show a toast (replacing any current one) and auto-dismiss it after `TOAST_AUTO_DISMISS_MS`.
   * See `toast`'s doc above. `<Toast/>` (`src/components/Toast.tsx`) is a pure reader of `toast`
   * — it never owns this timer itself, so the auto-dismiss fires even across a remount.
   */
  showToast: (message: string) => void;
  /** Clear the current toast immediately (e.g. a manual dismiss action) and cancel its pending
   * auto-dismiss timer so it cannot later clear a DIFFERENT toast shown after this one. */
  dismissToast: () => void;
}

/** Key format shared by `expanded`/`treeCache` — see their docs on `AppState` above. */
function fsKey(root: string, rel: string): string {
  return `${root}\t${rel}`;
}

/** How long a toast stays up before auto-dismissing (Toast atom, spec §7). */
const TOAST_AUTO_DISMISS_MS = 4000;

export const useAppStore = create<AppState>((set) => {
  // Toast auto-dismiss bookkeeping (closure state, not store state — it's write-only plumbing,
  // like terminal-manager's attachGeneration guard). `token` is bumped by every showToast/
  // dismissToast call; a pending timeout only clears the toast if its OWN token still matches the
  // current one, so an earlier toast's timer can never clear a later, different toast (it always
  // can't anyway, since we clearTimeout the previous timer below — the token is defense in depth
  // matching the rest of this codebase's race-guard style).
  let toastTimer: ReturnType<typeof setTimeout> | undefined;
  let toastToken = 0;

  const clearToastTimer = (): void => {
    if (toastTimer !== undefined) {
      clearTimeout(toastTimer);
      toastTimer = undefined;
    }
  };

  return {
    sessions: {},
    workspaces: {},
    activeSessionId: null,
    daemonConnected: false,
    daemonIncompatible: false,
    upgradeDialogOpen: false,
    upgradeError: null,
    hydrated: false,
    view: "home",
    expanded: {},
    treeCache: {},
    selectedFile: null,
    showIgnored: false,
    filesRailOpen: false,
    watchPaused: false,
    toast: null,

    upsertSession: (meta) =>
      set((s) => ({ sessions: { ...s.sessions, [meta.id]: meta } })),

    removeSession: (id) =>
      set((s) => {
        if (!(id in s.sessions)) return {};
        const { [id]: _removed, ...rest } = s.sessions;
        return {
          sessions: rest,
          activeSessionId: s.activeSessionId === id ? null : s.activeSessionId,
        };
      }),

    setLifecycle: (p) =>
      set((s) => {
        const existing = s.sessions[p.sessionId];
        if (!existing) return {};
        return {
          sessions: {
            ...s.sessions,
            [p.sessionId]: {
              ...existing,
              lifecycle: p.lifecycle,
              waitingForInput: p.waitingForInput,
              cwd: p.cwd,
            },
          },
        };
      }),

    markExited: (p) =>
      set((s) => {
        const existing = s.sessions[p.sessionId];
        if (!existing) return {};
        return {
          sessions: {
            ...s.sessions,
            [p.sessionId]: {
              ...existing,
              isActive: false,
              // A finished process is never waiting for input — the honest state for every
              // consumer (stats strip, StatusDot, HomeView filters). The live `session://exited`
              // push carries no `waitingForInput` field, so a session that exited/crashed while
              // blocked on stdin would otherwise keep a stale `true` forever (review finding F1).
              waitingForInput: false,
              lifecycle: { kind: "exited", code: p.code, signal: p.signal },
            },
          },
        };
      }),

    setDaemonConnected: (connected) => set({ daemonConnected: connected }),
    setDaemonIncompatible: (v) => set({ daemonIncompatible: v }),
    // Opening the dialog fresh (v=true) clears any stale upgradeError from a previous attempt
    // (finding [13]): every reopen path (daemon://incompatible, DaemonBanner's "Обновить" action)
    // goes through this setter, so this is the single place that guarantees a fresh open never
    // shows a leftover error from an earlier session/attempt. Closing (v=false) leaves the error
    // untouched — Cancel doesn't need to erase it, only a fresh open does.
    setUpgradeDialogOpen: (v) => set(v ? { upgradeDialogOpen: v, upgradeError: null } : { upgradeDialogOpen: v }),
    setUpgradeError: (v) => set({ upgradeError: v }),
    setHydrated: (v) => set({ hydrated: v }),

    upsertWorkspace: (ws) =>
      set((s) => ({ workspaces: { ...s.workspaces, [ws.id]: ws } })),

    setActiveSession: (id) => set({ activeSessionId: id }),

    setView: (v) => set({ view: v }),

    setExpanded: (root, rel, open) =>
      set((s) => {
        const key = fsKey(root, rel);
        if (open) {
          return { expanded: { ...s.expanded, [key]: true } };
        }
        if (!(key in s.expanded)) return {};
        const rest = { ...s.expanded };
        delete rest[key];
        return { expanded: rest };
      }),

    cacheDir: (root, rel, entries) =>
      set((s) => ({ treeCache: { ...s.treeCache, [fsKey(root, rel)]: entries } })),

    invalidateDirs: (root, rels) =>
      set((s) => {
        const prefix = `${root}\t`;
        const dropAll = rels.includes("*");
        const dropKeys = dropAll ? null : new Set(rels.map((rel) => fsKey(root, rel)));

        const drop = (key: string): boolean =>
          key.startsWith(prefix) && (dropAll || dropKeys!.has(key));

        // `expanded` is deliberately NOT filtered here — see the doc comment on `invalidateDirs`
        // above: this is a point refresh, not a collapse.
        const out: Record<string, FsEntry[]> = {};
        for (const key of Object.keys(s.treeCache)) {
          if (!drop(key)) out[key] = s.treeCache[key];
        }

        return { treeCache: out };
      }),

    setSelectedFile: (sel) => set({ selectedFile: sel }),

    toggleShowIgnored: () => set((s) => ({ showIgnored: !s.showIgnored })),

    setFilesRailOpen: (b) => set({ filesRailOpen: b }),

    setWatchPaused: (b) => set({ watchPaused: b }),

    showToast: (message) => {
      clearToastTimer();
      const token = ++toastToken;
      set({ toast: message });
      toastTimer = setTimeout(() => {
        // Only this call's own token still being current proves no later showToast/dismissToast
        // has superseded it — otherwise this stale timer must not touch whatever toast is showing
        // now (see the doc comment above `toastTimer`).
        if (token === toastToken) set({ toast: null });
        toastTimer = undefined;
      }, TOAST_AUTO_DISMISS_MS);
    },

    dismissToast: () => {
      clearToastTimer();
      toastToken += 1;
      set({ toast: null });
    },
  };
});
