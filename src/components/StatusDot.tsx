import type { JSX } from "react";
import type { SessionLifecycle } from "../ipc/types";
import { statusTone, type Tone } from "../ui/theme";

export type DotState = "idle" | "running" | "exited" | "waiting";

/**
 * Map a session's lifecycle + waiting flag to a dot state (spec §5, §10.4):
 * - Exited always wins (a stale waiting flag never overrides a finished process).
 * - While Running, waitingForInput surfaces the "waiting" state; otherwise "running".
 * - AtPrompt / Typing are idle (Typing is never emitted in S1; it maps to AtPrompt).
 */
export function dotStateOf(
  lifecycle: SessionLifecycle,
  waitingForInput: boolean,
): DotState {
  switch (lifecycle.kind) {
    case "exited":
      return "exited";
    case "running":
      return waitingForInput ? "waiting" : "running";
    case "atPrompt":
    case "typing":
      return "idle";
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
 * needs-you (warn), exited → terminal failure (danger).
 */
const DOT_STATUS: Record<DotState, string> = {
  idle: "pending",
  running: "running",
  waiting: "waiting",
  exited: "failed",
};

const LABEL: Record<DotState, string> = {
  idle: "idle",
  running: "running",
  exited: "exited",
  waiting: "waiting for input",
};

export function StatusDot(props: {
  lifecycle: SessionLifecycle;
  waitingForInput: boolean;
}): JSX.Element {
  const state = dotStateOf(props.lifecycle, props.waitingForInput);
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
        backgroundColor: TONE_VAR[statusTone(DOT_STATUS[state])],
        flexShrink: 0,
      }}
    />
  );
}
