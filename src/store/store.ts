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
  setUpgradeDialogOpen: (v) => set({ upgradeDialogOpen: v }),

  upsertWorkspace: (ws) =>
    set((s) => ({ workspaces: { ...s.workspaces, [ws.id]: ws } })),

  setActiveSession: (id) => set({ activeSessionId: id }),
}));
