// src/ipc/diag.ts — S-DIAG: a small, pure diagnostics core so the frontend can RECONSTRUCT the
// cause of a failure after the fact. Errors were previously surfaced only as a 4s toast (spec §7
// honest-surface) and then lost; this records a bounded, structured, secret-scrubbed event log the
// store keeps and `DiagnosticsPanel` renders. No React, no I/O — pure functions + types, unit-tested.

/** One recorded failure. `detail` is the scrubbed raw error text (never a secret); `kind` is the
 * machine-classifiable cause so a support bundle groups by it. */
export type DiagEvent = {
  /** Monotonic per-session id (the store owns the counter) — stable React key + ordering tiebreak. */
  id: number;
  /** Epoch ms when recorded (the store stamps it; injected in tests for determinism). */
  ts: number;
  /** Logical operation that failed, e.g. `"refreshProjects"`, `"createWorkspace"`, `"render"`. */
  op: string;
  /** Machine cause: an orchd `OrchdErrorCode` Debug string (`"Invariant"`, `"Conflict"`, …), a wire
   * kind (`"disconnected"`, `"incompatibleOrchd"`), `"render"` for a React crash, or `"unknown"`. */
  kind: string;
  /** The human message already shown to the user (mirrors `describeOrchdError`) — kept so the log
   * reads the same as the toast did. */
  message: string;
  /** Scrubbed raw detail (error `message`/stack first line, or a render component stack) for cause
   * reconstruction — `null` when there was nothing beyond `message`. Always secret-scrubbed. */
  detail: string | null;
};

/** Max events retained in the ring (oldest dropped). Bounded so a long noisy session can't grow
 * memory without limit; large enough to hold a realistic incident's worth of failures. */
export const DIAG_CAP = 200;

/**
 * Classify an unknown thrown value into a machine `kind` + a raw `detail` string for the log. This
 * reads the SAME `CommandError` shape `describeOrchdError` (`./orchd.ts`) maps to a human message —
 * `{ kind: "daemon", code, message }` (code = Rust `Debug` of `OrchdErrorCode`, e.g. `"Invariant"`),
 * or `{ kind: "disconnected" | "incompatibleOrchd" }` — but returns the classification, not copy.
 * Anything else is `"unknown"`. `detail` is the best raw text available (never a secret — caller
 * scrubs, but this also avoids leaking obvious token fields).
 */
export function classifyError(e: unknown): { kind: string; detail: string | null } {
  if (e !== null && typeof e === "object") {
    const err = e as { kind?: unknown; code?: unknown; message?: unknown; stack?: unknown };
    if (err.kind === "daemon") {
      const code = typeof err.code === "string" && err.code ? err.code : "orchd";
      const detail = typeof err.message === "string" && err.message ? err.message : null;
      return { kind: code, detail };
    }
    if (err.kind === "disconnected" || err.kind === "incompatibleOrchd") {
      return { kind: err.kind, detail: null };
    }
    // A real Error (or Error-like): first line of the stack is the most useful cause anchor.
    if (typeof err.message === "string" && err.message) {
      const stack = typeof err.stack === "string" ? err.stack.split("\n")[0] : "";
      return { kind: "unknown", detail: stack && stack !== err.message ? stack : err.message };
    }
  }
  if (typeof e === "string" && e) return { kind: "unknown", detail: e };
  return { kind: "unknown", detail: null };
}

// Secret-scrubbing (spec: "structured logs without leaking secrets"). A copyable support bundle must
// never carry a live credential or the operator's home-dir username. Order matters — specific
// key/value and token shapes first, then the home-path collapse.
const SCRUBBERS: Array<[RegExp, string]> = [
  // `Bearer <token>` in any casing.
  [/\bBearer\s+[A-Za-z0-9._~+/-]+=*/gi, "Bearer «redacted»"],
  // `key = value` / `token: value` / `authorization=value` / `password: value` / `secret=value`.
  [
    /\b(authorization|api[_-]?key|access[_-]?token|refresh[_-]?token|token|secret|password|passwd|pwd)\b(\s*[:=]\s*)("?)[^\s"'&]+\3/gi,
    "$1$2«redacted»",
  ],
  // Known credential prefixes (Linear, OpenAI, Slack, GitHub PAT, Apple app-specific password).
  [/\blin_api_[A-Za-z0-9]+/g, "«redacted-key»"],
  [/\bsk-[A-Za-z0-9_-]{10,}/g, "«redacted-key»"],
  [/\bxox[baprs]-[A-Za-z0-9-]{10,}/g, "«redacted-key»"],
  [/\bghp_[A-Za-z0-9]{20,}/g, "«redacted-key»"],
  [/\b[a-z]{4}-[a-z]{4}-[a-z]{4}-[a-z]{4}\b/g, "«redacted-pw»"],
  // Collapse the operator's home dir so a bundle never leaks the macOS username.
  [/\/Users\/[^/\s"']+/g, "/Users/«user»"],
];

/** Redact obvious secrets and the home-dir username from a log/detail string before it can be
 * displayed or copied into a support bundle. Best-effort defense-in-depth — the classifier already
 * avoids capturing token fields, this catches anything that slips into a raw message/stack. */
export function scrubSecrets(text: string): string {
  let out = text;
  for (const [re, repl] of SCRUBBERS) out = out.replace(re, repl);
  return out;
}

/** Append `item` to a bounded ring kept NEWEST-FIRST (index 0 = most recent), dropping the oldest
 * beyond `cap`. Returns a new array (never mutates the input) so it composes with immutable state. */
export function pushCapped<T>(list: readonly T[], item: T, cap: number): T[] {
  return [item, ...list].slice(0, cap);
}

/** Serialize the log as a pretty JSON support bundle for the clipboard. Assumes events are already
 * scrubbed (they are, at record time) — this only shapes them for copy/paste into an issue. */
export function toSupportBundle(events: readonly DiagEvent[]): string {
  return JSON.stringify(
    { tool: "builder-pro-ai", kind: "diagnostics", count: events.length, events },
    null,
    2,
  );
}
