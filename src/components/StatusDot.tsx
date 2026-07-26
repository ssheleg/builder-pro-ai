import type { JSX } from "react";
import type { SessionLifecycle } from "../ipc/types";
import { statusTone, type Tone } from "../ui/theme";
import { strings } from "../strings";

export type DotState = "idle" | "running" | "exited" | "waiting" | "restored";

/**
 * Map a session's lifecycle + waiting flag + liveness to a dot state (spec §5, §10.4):
 * - Exited always wins (a stale waiting flag never overrides a finished process).
 * - While Running, waitingForInput surfaces the "waiting" state; otherwise "running".
 * - AtPrompt / Typing are idle (Typing is never emitted in S1; it maps to AtPrompt).
 * - FE-7: a NOT-exited session with `isActive === false` is RESTORED (its PTY is gone after a
 *   daemon restart), never "idle" — idle implies a live shell at its prompt. The precedence
 *   (exited > waiting > live > restored) deliberately mirrors `partitionSessions`' bucket
 *   order in the store, so the dot and the Home sections can never disagree about a session.
 */
export function dotStateOf(
  lifecycle: SessionLifecycle,
  waitingForInput: boolean,
  isActive: boolean,
): DotState {
  switch (lifecycle.kind) {
    case "exited":
      return "exited";
    case "running":
      if (waitingForInput) return "waiting";
      return isActive ? "running" : "restored";
    case "atPrompt":
    case "typing":
      return isActive ? "idle" : "restored";
  }
}

/** Every semantic tone resolves to its foreground token (both light + dark valid). */
const TONE_VAR: Record<Tone, string> = {
  ink: "var(--ink)",
  muted: "var(--muted)",
  accent: "var(--accent)",
  info: "var(--info)",
  ok: "var(--ok)",
  warn: "var(--warn)",
  danger: "var(--danger)",
};

/**
 * Each lifecycle dot state borrows the semantic tone of the entity status it corresponds to, so the
 * dot's colour comes from the shared `statusTone()` table (one source of truth, theme-aware) instead
 * of a private dark-only palette: idle → neutral (muted), running → in-progress (info), waiting →
 * needs-you (warn), exited → terminal failure (danger). Restored (FE-7) also resolves to neutral
 * (muted) — nothing is wrong and nothing is running; what marks it "not live" is the HOLLOW ring
 * rendering below, not a new hue.
 */
const DOT_STATUS: Record<DotState, string> = {
  idle: "pending",
  running: "running",
  waiting: "waiting",
  exited: "failed",
  restored: "pending",
};

const LABEL: Record<DotState, string> = {
  idle: "idle",
  running: "running",
  exited: "exited",
  waiting: "waiting for input",
  // The one label wired to `strings` (FE-7): user-visible copy lives in `strings.ts`.
  restored: strings.sessions.restoredDotLabel,
};

export function StatusDot(props: {
  lifecycle: SessionLifecycle;
  waitingForInput: boolean;
  isActive: boolean;
}): JSX.Element {
  const state = dotStateOf(props.lifecycle, props.waitingForInput, props.isActive);
  const color = TONE_VAR[statusTone(DOT_STATUS[state])];
  return (
    <span
      role="img"
      aria-label={LABEL[state]}
      title={LABEL[state]}
      data-state={state}
      style={{
        display: "inline-block",
        width: 8,
        height: 8,
        borderRadius: "50%",
        // Restored reads as a hollow ring ("no live shell inside") — a filled dot of any colour
        // would claim a live process state the session no longer has (FE-7).
        backgroundColor: state === "restored" ? "transparent" : color,
        boxShadow: state === "restored" ? `inset 0 0 0 1.5px ${color}` : undefined,
        flexShrink: 0,
      }}
    />
  );
}
