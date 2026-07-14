import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { SessionMeta, SessionLifecycle, Workspace } from "./types";
import type { SessionId } from "./commands";
import type { RuleScope } from "./orchd-types";

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

// ── orchd coarse-invalidation + connection events (spec §9/§10, D6/D10, S3 T13) ────────────────
//
// `src-tauri/src/broker.rs`'s `EV_ORCHD_*` consts, produced by `map_orchd_push`/
// `map_orchd_conn_state`. Every `orchd://*-changed` push names ONLY what changed (never the
// updated entity itself — D6 "coarse-grained invalidation... GUI re-fetches lists"); the frontend
// store's matching `refresh*` action (`store.ts`) does the actual re-fetch via `./orchd.ts`.

/** Payload of `orchd://goals-changed`: the ONE project whose goal list changed — `refreshGoals`
 * must re-fetch only that project, never every project's goals. */
export interface GoalsChangedPayload {
  projectId: string;
}

/** Payload of `orchd://tasks-changed` — mirrors `GoalsChangedPayload` exactly, for tasks. */
export interface TasksChangedPayload {
  projectId: string;
}

/** Payload of `orchd://ruleset-changed`: `projectId` is `null` for the global scope (mirrors
 * `map_orchd_push`'s `RuleSetChanged` arm — `scope`/`projectId` are the raw fields, already
 * camelCase-reshaped by the broker, same as `StateChangedPayload`'s `sessionId`). */
export interface RulesetChangedPayload {
  scope: RuleScope;
  projectId: string | null;
}

/** Subscribe to `orchd://projects-changed`. Carries no payload — there is nothing to name. */
export function onOrchdProjectsChanged(cb: () => void): Promise<UnlistenFn> {
  return listen<null>("orchd://projects-changed", () => cb());
}

/** Subscribe to `orchd://goals-changed` — see `GoalsChangedPayload`. */
export function onOrchdGoalsChanged(cb: (p: GoalsChangedPayload) => void): Promise<UnlistenFn> {
  return listen<GoalsChangedPayload>("orchd://goals-changed", (e) => cb(e.payload));
}

/** Subscribe to `orchd://ideas-changed`. Carries no payload — this store slice's `ideas: Idea[]`
 * is not project-scoped, so a full `refreshIdeas` is the only meaningful reaction. */
export function onOrchdIdeasChanged(cb: () => void): Promise<UnlistenFn> {
  return listen<null>("orchd://ideas-changed", () => cb());
}

/** Subscribe to `orchd://insights-changed`. Mirrors `onOrchdIdeasChanged`, for insights. */
export function onOrchdInsightsChanged(cb: () => void): Promise<UnlistenFn> {
  return listen<null>("orchd://insights-changed", () => cb());
}

/** Subscribe to `orchd://tasks-changed` — see `TasksChangedPayload`. */
export function onOrchdTasksChanged(cb: (p: TasksChangedPayload) => void): Promise<UnlistenFn> {
  return listen<TasksChangedPayload>("orchd://tasks-changed", (e) => cb(e.payload));
}

/** Subscribe to `orchd://ruleset-changed` — see `RulesetChangedPayload`. */
export function onOrchdRulesetChanged(
  cb: (p: RulesetChangedPayload) => void,
): Promise<UnlistenFn> {
  return listen<RulesetChangedPayload>("orchd://ruleset-changed", (e) => cb(e.payload));
}

/** Payload of `orchd://graph-changed` (S4 §7, T5's `EV_ORCHD_GRAPH_CHANGED`): the ONE project
 * whose knowledge graph changed — mirrors `GoalsChangedPayload`/`TasksChangedPayload` exactly.
 * `refreshGraph` (`store.ts`) must re-fetch only that project, never every project's graph. Note a
 * cross-project edge's `GraphChanged` push fires once per endpoint project (`broker.rs`'s
 * `map_orchd_push` — "both projects for cross-project edges"), so a single edit can trigger two of
 * these events; each is still handled the same way, one project at a time. */
export interface GraphChangedPayload {
  projectId: string;
}

/** Subscribe to `orchd://graph-changed` — see `GraphChangedPayload`. */
export function onOrchdGraphChanged(cb: (p: GraphChangedPayload) => void): Promise<UnlistenFn> {
  return listen<GraphChangedPayload>("orchd://graph-changed", (e) => cb(e.payload));
}

/**
 * `orchd://down` carries no payload. Unlike the sessiond `daemon://disconnected`/`reconnected`
 * pair (which tracks "have we seen a disconnect yet" to decide whether a later connect counts as
 * a "reconnect"), orchd's mapping is a DIRECT 1:1 from `OrchdClient`'s connection state
 * (`broker.rs::map_orchd_conn_state`'s doc) — every `Disconnected` fires this event, every
 * `Connected` fires `orchd://up`.
 */
export function onOrchdDown(cb: () => void): Promise<UnlistenFn> {
  return listen<null>("orchd://down", () => cb());
}

/** `orchd://up` carries no payload — fires on every successful (re)connect. See `onOrchdDown`'s
 * doc for why this is a direct 1:1 mapping rather than a reconnect-tracking scheme. */
export function onOrchdUp(cb: () => void): Promise<UnlistenFn> {
  return listen<null>("orchd://up", () => cb());
}

/**
 * `orchd://incompatible` carries no payload. FATAL like `daemon://incompatible` (Pv2 §6.2): the
 * orchd client's connection task has exited and will NOT reconnect on its own — the frontend must
 * offer the upgrade flow (`orchdUpgrade`, `./orchd.ts`) rather than waiting.
 */
export function onOrchdIncompatible(cb: () => void): Promise<UnlistenFn> {
  return listen<null>("orchd://incompatible", () => cb());
}
