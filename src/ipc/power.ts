import { invoke } from "@tauri-apps/api/core";

/**
 * Keep-awake status snapshot (SCN-045, FLW-18). Matches
 * `src-tauri/src/power.rs::PowerStatus` (`#[serde(rename_all = "camelCase")]`) field-for-field:
 * - `enabled` — the persisted toggle intent;
 * - `active` — the macOS power assertion is GENUINELY held right now (`SleepAsserter::is_held`),
 *   never the intent — the honest source for the pill's green dot;
 * - `error` — the most recent OS acquire denial while an assertion is still wanted-but-unheld
 *   (Rust `Option<String>` → `null` when clear), the honest "keep-awake unavailable: {reason}"
 *   surface — never a silent fake "awake" (SCN-045 "Errors & recovery").
 */
export interface PowerStatus {
  enabled: boolean;
  active: boolean;
  error: string | null;
}

/**
 * Typed `invoke()` wrappers for the keep-awake `#[tauri::command]` surface (SCN-045,
 * `src-tauri/src/power.rs`). Same discipline as `./commands.ts`: one wrapper per command, arg
 * keys match the Rust parameter names, and the commands themselves are infallible at the wire
 * layer — every failure mode arrives IN the resolved `PowerStatus.error`, so a rejection here
 * only ever means the IPC/runtime itself broke. Handling either is the store's job
 * (`store.ts::syncKeepAwake`/`setKeepAwakeEnabled`), not this layer's.
 */

/** Set the keep-awake toggle (the sidebar pill's click) and get the reconciled status back. */
export function powerSetEnabled(enabled: boolean): Promise<PowerStatus> {
  return invoke<PowerStatus>("power_set_enabled", { enabled });
}

/**
 * Sync the live-session count (`App.tsx` calls this whenever the number of
 * `lifecycle.kind !== "exited"` sessions changes) and get the reconciled status back.
 */
export function powerSyncSessions(live: number): Promise<PowerStatus> {
  return invoke<PowerStatus>("power_sync_sessions", { live });
}

/**
 * Pull the current truth without mutating anything — the pull-based fallback mirror of
 * `daemonStatus` (finding [12]): a remounting webview can re-read the pill state instead of
 * trusting a possibly-stale store snapshot.
 */
export function powerStatus(): Promise<PowerStatus> {
  return invoke<PowerStatus>("power_status");
}
