// src/components/workflows/agents.ts — SW1 workflow-authoring shared helpers (pure, no React) so
// the terminal-grouping algorithm and the known-agent set are unit-testable in isolation and the
// four workflow surfaces agree on them.

import { strings } from "../../strings";
import type { Stage } from "../../ipc/orchd-types";

/**
 * The agents the app launches (locked contract, docs/ux/plans/2026-07-24-workflow-authoring.md):
 * a stage's `agent` is validated against this set (or `null` = inherit the workflow's
 * `defaultAgent`) at save time by the daemon. The frontend uses it to populate the agent pickers
 * and to flag a pinned-but-unknown agent honestly (an unavailable/legacy pin, never silently
 * hidden).
 */
export const KNOWN_AGENTS = ["claude-code", "hermes", "opencode", "kilo"] as const;

export type KnownAgent = (typeof KNOWN_AGENTS)[number];

/** `true` when `agent` is one of the launchable agents above. */
export function isKnownAgent(agent: string): agent is KnownAgent {
  return (KNOWN_AGENTS as readonly string[]).includes(agent);
}

/**
 * Display label for an agent id. A known id maps to its friendly label (`strings.workflows.agents`);
 * an UNKNOWN id (an agent pinned on a stage that is no longer one the app launches) is returned
 * verbatim rather than hidden — the surfaces pair it with an "unknown agent" marker so the honesty
 * boundary is visible, never a silent substitution.
 */
export function agentLabel(agent: string): string {
  return strings.workflows.agents[agent] ?? agent;
}

/**
 * One "terminal bracket" (SCR-02): a run of consecutive stages that all resolve to the SAME
 * effective agent. `agent` is that shared effective agent; `stages` carries each stage paired with
 * its index in the workflow's flat `stages` Vec (order = index).
 */
export interface TerminalGroup {
  agent: string;
  stages: { stage: Stage; index: number }[];
}

/**
 * THE terminal-grouping algorithm (SCR-02) — a PURE VIEW over the per-stage agent, never stored.
 * Walk the stages in order; a stage's EFFECTIVE agent is `stage.agent ?? defaultAgent` (a `null`
 * stage agent inherits the workflow default). Consecutive stages with the same effective agent
 * accrete into one terminal group; the first stage whose effective agent differs from the current
 * group's opens a NEW group. Therefore an all-one-agent workflow is exactly ONE terminal, and every
 * agent change is a terminal boundary — the two invariants the editor renders and the tests lock.
 */
export function computeTerminalGroups(stages: Stage[], defaultAgent: string): TerminalGroup[] {
  const groups: TerminalGroup[] = [];
  for (let index = 0; index < stages.length; index++) {
    const stage = stages[index];
    const agent = stage.agent ?? defaultAgent;
    const last = groups[groups.length - 1];
    if (last !== undefined && last.agent === agent) {
      last.stages.push({ stage, index });
    } else {
      groups.push({ agent, stages: [{ stage, index }] });
    }
  }
  return groups;
}

/**
 * The effective skills the S6b runtime would load for a stage — the workflow's global skills UNION
 * the stage's own, deduped, order-preserving (globals first, then any stage-only additions). A pure
 * view (SCN-061: "never stored"), shared by the editor's per-stage summary and StageDetail.
 */
export function effectiveSkillIds(globalSkillIds: string[], stageSkillIds: string[]): string[] {
  const seen = new Set<string>();
  const out: string[] = [];
  for (const id of [...globalSkillIds, ...stageSkillIds]) {
    if (!seen.has(id)) {
      seen.add(id);
      out.push(id);
    }
  }
  return out;
}

/** Fresh client-side uuid for a new stage's `id` (the daemon does not mint stage ids — they ride
 * inside the workflow JSON). `crypto.randomUUID` is available in the Tauri webview and jsdom. */
export function newStageId(): string {
  return `stage-${crypto.randomUUID()}`;
}
