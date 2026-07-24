import { describe, it, expect } from "vitest";
import type { Stage } from "../../ipc/orchd-types";
import {
  KNOWN_AGENTS,
  agentLabel,
  computeTerminalGroups,
  effectiveSkillIds,
  isKnownAgent,
} from "./agents";

function stage(over: Partial<Stage> = {}): Stage {
  return {
    id: over.id ?? `st-${Math.random()}`,
    name: over.name ?? "s",
    prompt: over.prompt ?? "p",
    skillIds: over.skillIds ?? [],
    agent: over.agent ?? null,
    contextScope: over.contextScope ?? "inherit",
    outputs: over.outputs ?? [],
    gate: over.gate ?? "auto",
  };
}

describe("computeTerminalGroups (SCR-02 terminal brackets)", () => {
  it("empty stages → no groups", () => {
    expect(computeTerminalGroups([], "claude-code")).toEqual([]);
  });

  it("a single stage inheriting the default → ONE terminal on the default agent", () => {
    const groups = computeTerminalGroups([stage({ id: "a", agent: null })], "claude-code");
    expect(groups).toHaveLength(1);
    expect(groups[0].agent).toBe("claude-code");
    expect(groups[0].stages.map((s) => s.index)).toEqual([0]);
  });

  it("all stages resolving to the SAME effective agent → ONE terminal (single-agent workflow)", () => {
    // Mix of inherit (null → claude-code) and an explicit pin to the SAME agent: still one terminal.
    const groups = computeTerminalGroups(
      [
        stage({ id: "a", agent: null }),
        stage({ id: "b", agent: "claude-code" }),
        stage({ id: "c", agent: null }),
      ],
      "claude-code",
    );
    expect(groups).toHaveLength(1);
    expect(groups[0].agent).toBe("claude-code");
    expect(groups[0].stages.map((s) => s.index)).toEqual([0, 1, 2]);
  });

  it("an agent change is a terminal BOUNDARY (two terminals)", () => {
    const groups = computeTerminalGroups(
      [stage({ id: "a", agent: null }), stage({ id: "b", agent: "hermes" })],
      "claude-code",
    );
    expect(groups).toHaveLength(2);
    expect(groups[0].agent).toBe("claude-code");
    expect(groups[0].stages.map((s) => s.index)).toEqual([0]);
    expect(groups[1].agent).toBe("hermes");
    expect(groups[1].stages.map((s) => s.index)).toEqual([1]);
  });

  it("changing the default agent re-groups inherit stages (A A B B pattern)", () => {
    const stages = [
      stage({ id: "a", agent: null }), // → default
      stage({ id: "b", agent: null }), // → default
      stage({ id: "c", agent: "hermes" }),
      stage({ id: "d", agent: "hermes" }),
    ];
    const groups = computeTerminalGroups(stages, "claude-code");
    expect(groups.map((g) => g.agent)).toEqual(["claude-code", "hermes"]);
    expect(groups.map((g) => g.stages.length)).toEqual([2, 2]);
  });

  it("A B A produces THREE terminals (a return to a prior agent still opens a new bracket)", () => {
    const groups = computeTerminalGroups(
      [
        stage({ id: "a", agent: "claude-code" }),
        stage({ id: "b", agent: "hermes" }),
        stage({ id: "c", agent: "claude-code" }),
      ],
      "claude-code",
    );
    expect(groups.map((g) => g.agent)).toEqual(["claude-code", "hermes", "claude-code"]);
    expect(groups.map((g) => g.stages.map((s) => s.index))).toEqual([[0], [1], [2]]);
  });
});

describe("effectiveSkillIds (global ∪ stage, deduped, order-preserving)", () => {
  it("unions globals then stage additions, de-duplicating overlaps", () => {
    expect(effectiveSkillIds(["g1", "g2"], ["g2", "s1"])).toEqual(["g1", "g2", "s1"]);
  });

  it("empty inputs → empty", () => {
    expect(effectiveSkillIds([], [])).toEqual([]);
  });
});

describe("known-agent helpers", () => {
  it("isKnownAgent recognizes exactly the four launchable agents", () => {
    for (const a of KNOWN_AGENTS) expect(isKnownAgent(a)).toBe(true);
    expect(isKnownAgent("gemini")).toBe(false);
  });

  it("agentLabel maps known ids to labels and returns an unknown id verbatim", () => {
    expect(agentLabel("claude-code")).toBe("Claude Code");
    expect(agentLabel("legacy-bot")).toBe("legacy-bot");
  });
});
