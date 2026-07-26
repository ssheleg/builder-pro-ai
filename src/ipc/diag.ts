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
   * kind (`"disconnected"`, `"incompatibleOrchd"`), `"render"` for a React crash, `"message"` for a
   * deliberate human-readable string error (FE-2), or `"unknown"`. */
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
 * A plain non-empty string classifies as `"message"` (FE-2: it IS the human message, not an
 * unclassifiable throw); anything else is `"unknown"`. `detail` is the best raw text available
 * (never a secret — caller scrubs, but this also avoids leaking obvious token fields).
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
  if (typeof e === "string" && e) {
    // FE-2: a string error IS the final human message (e.g. a per-source failure reason from
    // `refreshStats`) — `reportError` shows it verbatim, so the event's `message` already carries
    // everything; kind `"message"` (not `"unknown"`) records that this was a deliberate,
    // already-human string rather than an unclassifiable throw.
    return { kind: "message", detail: null };
  }
  return { kind: "unknown", detail: null };
}

// Secret-scrubbing (spec: "structured logs without leaking secrets"). A copyable support bundle must
// never carry a live credential or the operator's home-dir username. Order matters — specific
// key/value and token shapes first, then the home-path collapse. REL-4 widened the net after a
// differential corpus showed what slipped through: JSON-quoted keys, bare JWTs, common vendor token
// prefixes, URL userinfo, Cookie headers, PEM blocks, and multi-word quoted values.
const SCRUBBERS: Array<[RegExp, string]> = [
  // `Bearer <token>` in any casing.
  [/\bBearer\s+[A-Za-z0-9._~+/-]+=*/gi, "Bearer «redacted»"],
  // `key = value` / `token: value` / `authorization=value` / `password: value` / `secret=value`.
  // REL-4: quotes are tolerated around the key AND the value, and a quoted value is consumed through
  // its closing quote so multi-word secrets and JSON shapes (`password: "two words"`,
  // `"access_token": "abc123"`) are redacted whole, not just up to the first space.
  [
    /\b(authorization|api[_-]?key|access[_-]?token|refresh[_-]?token|token|secret|password|passwd|pwd)\b(["']?)(\s*[:=]\s*)(?:"[^"\n]*"?|'[^'\n]*'?|[^\s"'&]+)/gi,
    "$1$2$3«redacted»",
  ],
  // PEM private-key material: a full BEGIN…END block first (multiline, the END type must match the
  // BEGIN type), then any bare BEGIN header left over from a truncated block (REL-4).
  [/-----BEGIN ([A-Z0-9 ]*PRIVATE KEY)-----[\s\S]*?-----END \1-----/g, "«redacted-key»"],
  [/-----BEGIN [A-Z0-9 ]*PRIVATE KEY-----/g, "«redacted-key»"],
  // Bare JWTs (header.payload.signature — every JWT opens with `eyJ`, the base64url of `{"`).
  [/\beyJ[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\.[A-Za-z0-9_-]+\b/g, "«redacted-key»"],
  // Known credential prefixes (Linear, OpenAI, Slack, GitHub PAT/OAuth/server/refresh/fine-grained,
  // GitLab, AWS, Google API key, npm, PyPI) — REL-4 added everything after `ghp_`.
  [/\blin_api_[A-Za-z0-9]+/g, "«redacted-key»"],
  [/\bsk-[A-Za-z0-9_-]{10,}/g, "«redacted-key»"],
  [/\bxox[baprs]-[A-Za-z0-9-]{10,}/g, "«redacted-key»"],
  [/\bghp_[A-Za-z0-9]{20,}/g, "«redacted-key»"],
  [/\bgh[ousr]_[A-Za-z0-9]{20,}/g, "«redacted-key»"],
  [/\bgithub_pat_[A-Za-z0-9_]{20,}/g, "«redacted-key»"],
  [/\bglpat-[A-Za-z0-9_-]{8,}/g, "«redacted-key»"],
  [/\b(?:AKIA|ASIA)[0-9A-Z]{16}\b/g, "«redacted-key»"],
  [/\bAIza[0-9A-Za-z_-]{35,}\b/g, "«redacted-key»"],
  [/\bnpm_[A-Za-z0-9]{20,}/g, "«redacted-key»"],
  [/\bpypi-[A-Za-z0-9_-]{20,}/g, "«redacted-key»"],
  // Slack webhook URL — keep the well-known host visible, redact the secret path (REL-4).
  [
    /https:\/\/hooks\.slack\.com\/services\/[^\s"']+/g,
    "https://hooks.slack.com/services/«redacted»",
  ],
  // URL userinfo (`scheme://user:pass@host`) — redact the credential between scheme and `@` (REL-4).
  [/([a-z][a-z0-9+.-]*:\/\/)[^\s/@:]+:[^\s/@]+@/gi, "$1«redacted»@"],
  // A Cookie / Set-Cookie header line is session material; the `=` lookahead keeps the word
  // "cookie:" in ordinary prose (no name=value pairs) untouched (REL-4).
  [/\b(Set-)?Cookie(\s*:\s*)(?=[^\n]*=)[^\n]+/gi, "$1Cookie$2«redacted»"],
  // Apple app-specific password (xxxx-xxxx-xxxx-xxxx). REL-4: the bare shape collided with ordinary
  // hyphenated English ("this-word-four-times"), so it is redacted ONLY with `apple` /
  // `app-specific` nearby (either side) — narrowed rather than dropped to keep the real credential
  // covered without false positives on prose.
  [/\b(apple|app-specific)\b([^\n]{0,60}?)\b[a-z]{4}(?:-[a-z]{4}){3}\b/gi, "$1$2«redacted-pw»"],
  [/\b[a-z]{4}(?:-[a-z]{4}){3}\b([^\n]{0,60}?)\b(apple|app-specific)\b/gi, "«redacted-pw»$1$2"],
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
