import { invoke, Channel } from "@tauri-apps/api/core";
import type { SessionMeta, Workspace, TerminalEvent } from "./types";

/**
 * `SessionMeta["id"]` / `Workspace["id"]` — mirrors the `bpa_protocol::SessionId` /
 * `bpa_protocol::WorkspaceId` Rust type aliases (both plain `String`), which ts-rs does not emit
 * as standalone TS type aliases (only `#[derive(TS)]` structs/enums get a generated export).
 * Kept local to the IPC layer rather than hand-added to the generated `types.ts`.
 */
export type SessionId = string;
export type WorkspaceId = string;

/**
 * Options for `create_session` (spec §6.1). Matches `src-tauri/src/commands.rs::CreateOpts`
 * (`#[serde(rename_all = "camelCase")]`) field-for-field; every field is optional on the wire
 * (the core fills in the default: no shell override, current cwd, no extra env, 80x24 sizing).
 */
export interface CreateSessionOpts {
  shell?: string;
  cwd?: string;
  envOverrides?: [string, string][];
  cols?: number;
  rows?: number;
}

/**
 * Typed `invoke()` wrappers for the `#[tauri::command]` surface (spec §6.1). One wrapper per
 * command; each arg object's keys match the Rust parameter names verbatim (Tauri maps JS
 * camelCase -> Rust snake_case automatically, so `workspaceId` here reaches `workspace_id` there).
 *
 * Rejections propagate as-is (a rejected `CommandError` from `src-tauri/src/commands.rs`,
 * including the case where the daemon socket isn't connected yet — `CommandError::Disconnected`).
 * Handling/gating those rejections is the frontend store's job (Task 22), not this layer's.
 */

export function createSession(
  workspaceId: WorkspaceId,
  opts?: CreateSessionOpts,
): Promise<SessionMeta> {
  return invoke<SessionMeta>("create_session", { workspaceId, opts });
}

export function listSessions(): Promise<SessionMeta[]> {
  return invoke<SessionMeta[]>("list_sessions");
}

export function attachSession(
  sessionId: SessionId,
  onEvent: Channel<TerminalEvent>,
): Promise<void> {
  return invoke<void>("attach_session", { sessionId, onEvent });
}

export function detachSession(sessionId: SessionId): Promise<void> {
  return invoke<void>("detach_session", { sessionId });
}

export function writeStdin(sessionId: SessionId, data: string): Promise<void> {
  return invoke<void>("write_stdin", { sessionId, data });
}

export function resize(sessionId: SessionId, cols: number, rows: number): Promise<void> {
  return invoke<void>("resize", { sessionId, cols, rows });
}

export function killSession(sessionId: SessionId): Promise<void> {
  return invoke<void>("kill_session", { sessionId });
}

export function listWorkspaces(): Promise<Workspace[]> {
  return invoke<Workspace[]>("list_workspaces");
}

export function createWorkspace(name: string, rootPath: string): Promise<Workspace> {
  return invoke<Workspace>("create_workspace", { name, rootPath });
}

export function getSessionState(sessionId: SessionId): Promise<SessionMeta> {
  return invoke<SessionMeta>("get_session_state", { sessionId });
}

/**
 * CORE-ONLY (spec §6.1): the native folder picker never reaches the daemon
 * (`src-tauri/src/commands.rs::pick_folder`). Resolves the chosen absolute path, or `null` if the
 * user canceled the dialog.
 */
export function pickFolder(): Promise<string | null> {
  return invoke<string | null>("pick_folder");
}

/**
 * Triggers the daemon upgrade (Pv2 §6.2, `src-tauri/src/commands.rs::upgrade_daemon`). The core
 * kickstarts a new daemon and then calls `app.restart()`, which kills this webview process —
 * so this promise NEVER resolves on the happy path. Callers MUST treat this as fire-and-forget
 * (`void upgradeDaemon()`), never `await` it, and never treat non-resolution as failure.
 */
export function upgradeDaemon(): Promise<void> {
  return invoke<void>("upgrade_daemon");
}
