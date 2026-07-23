import { invoke } from "@tauri-apps/api/core";

/**
 * Usage/output statistics wire types (SCN-052/053, FLW-20). Hand-mirrored from
 * `src-tauri/src/stats.rs` (`#[serde(rename_all = "camelCase")]`) — deliberately NOT ts-rs
 * (these types never cross the orchd wire; same rationale as `./power.ts`).
 */

/** One day × one workspace cwd of aggregated Claude Code usage (A-8 contract). */
export interface DayUsage {
  /** `YYYY-MM-DD` (UTC). */
  day: string;
  /** The session's real working directory — the attribution key the store maps to projects. */
  cwd: string;
  tokensIn: number;
  tokensOut: number;
  cacheWrite: number;
  cacheRead: number;
  /** Estimated USD for the PRICED share of the bucket; `null` when no family had pricing. */
  estCostUsd: number | null;
  /** `false` when some contributing model family lacks a pricing row — "partial estimate". */
  costComplete: boolean;
  /** Distinct session files that touched this (day, cwd). */
  sessions: number;
}

/** One model FAMILY of aggregated usage across the range (SCN-052 "per model family" cut —
 * the only agent-side dimension the session logs expose; there is no per-instance agent id). */
export interface FamilyUsage {
  /** "opus" | "sonnet" | "haiku" | "fable" | "other" | … */
  family: string;
  tokensIn: number;
  tokensOut: number;
  cacheWrite: number;
  cacheRead: number;
  /** Estimated USD for this family; `null` when the family has no pricing row. */
  estCostUsd: number | null;
  /** Distinct session files that touched this family in range. */
  sessions: number;
}

export interface UsageStats {
  /** Unix ms at scan completion — the "as of" stamp (SCN-053 freshness rule). */
  asOf: number;
  days: DayUsage[];
  /** Per-model-family cut for the range (largest spend first). */
  families: FamilyUsage[];
  /** Whole-scan failure (projects dir unreadable, worker died) — the per-source honesty note. */
  error: string | null;
}

/** Git output stats for one workspace root (SCN-053). Counts are meaningful ONLY when
 * `available` — a missing binary / non-repo root reports `false` + `reason`, never zeros. */
export interface GitStats {
  root: string;
  commits: number;
  added: number;
  deleted: number;
  available: boolean;
  reason: string | null;
}

export type StatsRange = "all" | "30d" | "7d";

/** Scan Claude Code usage for the range. Infallible at the wire layer — failures arrive IN
 * `UsageStats.error`; a rejection here means the IPC/runtime itself broke (store's job). */
export function statsUsage(range: StatsRange): Promise<UsageStats> {
  return invoke<UsageStats>("stats_usage", { range });
}

/** Git output stats for the given workspace roots — one honest entry per root. */
export function statsGit(roots: string[], range: StatsRange): Promise<GitStats[]> {
  return invoke<GitStats[]>("stats_git", { roots, range });
}
