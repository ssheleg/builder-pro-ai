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
