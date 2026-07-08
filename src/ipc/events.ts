import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { SessionMeta, SessionLifecycle, Workspace } from "./types";
import type { SessionId } from "./commands";

/**
 * Payload of `session://state-changed` (spec §6.3), reshaped by
 * `src-tauri/src/broker.rs::map_push` from `Push::StateChanged`'s snake_case daemon fields to
 * camelCase (`{ sessionId, lifecycle, waitingForInput, cwd }`).
 */
export interface StateChangedPayload {
  sessionId: SessionId;
  lifecycle: SessionLifecycle;
  waitingForInput: boolean;
  cwd: string;
}

/**
 * Payload of `session://exited` (spec §6.3), reshaped by `map_push` from `Push::ChildExited`.
 * `code`/`signal` are `null` (never coerced to a default) when the daemon didn't report one.
 */
export interface ExitedPayload {
  sessionId: SessionId;
  code: number | null;
  signal: string | null;
}

/**
 * Typed `listen()` subscription helpers for the global event bus (spec §6.3). Each wraps
 * `@tauri-apps/api/event`'s `listen<T>`, subscribing to the exact `session://*` / `workspace://*`
 * / `daemon://*` string the broker emits (`src-tauri/src/broker.rs::EV_*` constants) and unwrapping
 * `event.payload` before handing it to the caller's callback.
 */

export function onSessionCreated(cb: (m: SessionMeta) => void): Promise<UnlistenFn> {
  return listen<SessionMeta>("session://created", (e) => cb(e.payload));
}

export function onSessionStateChanged(
  cb: (p: StateChangedPayload) => void,
): Promise<UnlistenFn> {
  return listen<StateChangedPayload>("session://state-changed", (e) => cb(e.payload));
}

export function onSessionExited(cb: (p: ExitedPayload) => void): Promise<UnlistenFn> {
  return listen<ExitedPayload>("session://exited", (e) => cb(e.payload));
}

export function onWorkspaceCreated(cb: (w: Workspace) => void): Promise<UnlistenFn> {
  return listen<Workspace>("workspace://created", (e) => cb(e.payload));
}

/**
 * Payload of `fs://changed` (spec §5), emitted by `src-tauri/src/fs_watcher.rs`'s
 * `FsEventSink for AppHandle` impl. `changedRelPaths` is already deduped and capped at 500
 * (`fs_watcher::WATCH_PATH_CAP`): `["*"]` means "refresh everything expanded under this root".
 */
export interface FsChangedPayload {
  root: string;
  changedRelPaths: string[];
}

/** Payload of `fs://watch-error` (spec §5): the live watch for `root` failed/died. The frontend
 * shows a "live updates paused" affordance and re-calls `startWorkspaceWatch` on next activation —
 * this never panics the app, it is a signal, not a fatal error. */
export interface FsWatchErrorPayload {
  root: string;
  reason: string;
}

/** Subscribe to `fs://changed` (spec §5) — see `FsChangedPayload`. */
export function onFsChanged(cb: (p: FsChangedPayload) => void): Promise<UnlistenFn> {
  return listen<FsChangedPayload>("fs://changed", (e) => cb(e.payload));
}

/** Subscribe to `fs://watch-error` (spec §5) — see `FsWatchErrorPayload`. */
export function onFsWatchError(cb: (p: FsWatchErrorPayload) => void): Promise<UnlistenFn> {
  return listen<FsWatchErrorPayload>("fs://watch-error", (e) => cb(e.payload));
}

/**
 * Subscribe to `workspace://updated` (spec §3.3/§6.6), emitted by `src-tauri/src/broker.rs`'s
 * `EV_WORKSPACE_UPDATED` mapping of `Push::WorkspaceUpdated`. Fires whenever a workspace's roots
 * change (`addWorkspaceRoot`/`removeWorkspaceRoot`, from ANY connected client) — the payload is
 * the raw, already-updated `Workspace`, upsertable directly via the store's `upsertWorkspace`.
 */
export function onWorkspaceUpdated(cb: (w: Workspace) => void): Promise<UnlistenFn> {
  return listen<Workspace>("workspace://updated", (e) => cb(e.payload));
}

/** `daemon://disconnected` carries no payload (the core emits `()`, which decodes as `null`). */
export function onDaemonDisconnected(cb: () => void): Promise<UnlistenFn> {
  return listen<null>("daemon://disconnected", () => cb());
}

/** `daemon://reconnected` carries no payload (the core emits `()`, which decodes as `null`). */
export function onDaemonReconnected(cb: () => void): Promise<UnlistenFn> {
  return listen<null>("daemon://reconnected", () => cb());
}

/**
 * `daemon://incompatible` carries no payload (the core emits `()`, which decodes as `null`).
 * Unlike `daemon://disconnected`, this is FATAL (Pv2 §6.2): the client's connection task has
 * exited and will NOT reconnect on its own — the frontend must offer the user an upgrade
 * (`upgradeDaemon`) rather than waiting.
 */
export function onDaemonIncompatible(cb: () => void): Promise<UnlistenFn> {
  return listen<null>("daemon://incompatible", () => cb());
}
