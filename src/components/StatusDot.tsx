import type { JSX } from "react";
import type { SessionLifecycle } from "../ipc/types";
import { theme } from "../theme";

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

const COLOR: Record<DotState, string> = {
  idle: theme.colors.statusIdle,
  running: theme.colors.statusRunning,
  exited: theme.colors.statusExited,
  waiting: theme.colors.statusWaiting,
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
        backgroundColor: COLOR[state],
        flexShrink: 0,
      }}
    />
  );
}
