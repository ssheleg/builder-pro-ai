import { create } from "zustand";
import type { SessionMeta, Workspace } from "../ipc/types";
import type { SessionId, WorkspaceId } from "../ipc/commands";
import type { StateChangedPayload, ExitedPayload } from "../ipc/events";

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
   * Apply a `session://exited` payload: sets `isActive:false` and an `{kind:"exited"}`
   * lifecycle carrying the exit code/signal. No-op if the session isn't in the map.
   */
  markExited: (p: ExitedPayload) => void;
  setDaemonConnected: (connected: boolean) => void;
  setDaemonIncompatible: (v: boolean) => void;
  setUpgradeDialogOpen: (v: boolean) => void;
  /** Set the upgrade-failure message (or clear it with `null`). See `upgradeError` doc above. */
  setUpgradeError: (v: string | null) => void;
  /** Set `true` after the first successful hydrate. See `hydrated` doc above. */
  setHydrated: (v: boolean) => void;
  /** Insert or replace a workspace by `ws.id`. Idempotent. */
  upsertWorkspace: (ws: Workspace) => void;
  setActiveSession: (id: SessionId | null) => void;
}

export const useAppStore = create<AppState>((set) => ({
  sessions: {},
  workspaces: {},
  activeSessionId: null,
  daemonConnected: false,
  daemonIncompatible: false,
  upgradeDialogOpen: false,
  upgradeError: null,
  hydrated: false,

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
}));
