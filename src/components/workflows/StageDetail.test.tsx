// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, cleanup, fireEvent } from "@testing-library/react";
import { StageDetail } from "./StageDetail";
import { strings } from "../../strings";
import type { Skill, Stage } from "../../ipc/orchd-types";

function stage(over: Partial<Stage> = {}): Stage {
  return {
    id: over.id ?? "st-1",
    name: over.name ?? "Draft",
    prompt: over.prompt ?? "do it",
    skillIds: over.skillIds ?? [],
    agent: over.agent ?? null,
    contextScope: over.contextScope ?? "inherit",
    outputs: over.outputs ?? [],
    gate: over.gate ?? "auto",
  };
}

function skill(id: string, name: string): Skill {
  return {
    id,
    name,
    description: "",
    mdPath: `/tmp/${id}/SKILL.md`,
    mdHash: "h",
    scope: "global",
    projectId: null,
    fileState: "present",
    createdAt: 1,
    updatedAt: 1,
  };
}

afterEach(cleanup);

function renderStage(over: Partial<Stage> = {}, opts: { skills?: Skill[]; globalSkillIds?: string[] } = {}) {
  const onChange = vi.fn();
  const s = stage(over);
  render(
    <StageDetail
      stage={s}
      skills={opts.skills ?? []}
      defaultAgent="claude-code"
      globalSkillIds={opts.globalSkillIds ?? []}
      onChange={onChange}
      onRemove={vi.fn()}
      onDone={vi.fn()}
    />,
  );
  return { onChange, stage: s };
}

describe("StageDetail (SCR-03)", () => {
  it("the agent picker pins a known agent via onChange", () => {
    const { onChange } = renderStage();
    fireEvent.change(screen.getByTestId("stage-agent"), { target: { value: "hermes" } });
    expect(onChange).toHaveBeenCalledWith(expect.objectContaining({ agent: "hermes" }));
  });

  it("a null agent shows the inherited-default label", () => {
    renderStage({ agent: null });
    expect(screen.getByTestId("stage-agent-inherited").textContent).toContain("Claude Code");
  });

  it("a pinned UNKNOWN agent renders the agent-unavailable marker", () => {
    renderStage({ agent: "legacy-bot" });
    expect(screen.getByTestId("stage-agent-unavailable").textContent).toContain("legacy-bot");
  });

  it('context scope "selected" reveals the subset note', () => {
    const { onChange } = renderStage({ contextScope: "inherit" });
    // Click the "Selected" segment.
    fireEvent.click(screen.getByRole("radio", { name: strings.workflows.contextScopes.selected }));
    expect(onChange).toHaveBeenCalledWith(expect.objectContaining({ contextScope: "selected" }));
    cleanup();
    // Re-render already on "selected" to assert the note is shown.
    renderStage({ contextScope: "selected" });
    expect(screen.getByTestId("stage-selected-note")).toBeTruthy();
  });

  it("outputs: typing + add appends via onChange", () => {
    const { onChange } = renderStage({ outputs: [] });
    fireEvent.change(screen.getByTestId("stage-output-input"), { target: { value: "report.md" } });
    fireEvent.click(screen.getByTestId("stage-output-add"));
    expect(onChange).toHaveBeenCalledWith(expect.objectContaining({ outputs: ["report.md"] }));
  });

  it("a bound skill id that is NOT registered renders a missing-binding marker", () => {
    renderStage({ skillIds: ["ghost"] }, { skills: [skill("real", "Real Skill")] });
    expect(screen.getByTestId("stage-missing-ghost").textContent).toContain("missing skill: ghost");
  });

  it("effective skills summary is global ∪ stage, deduped", () => {
    renderStage(
      { skillIds: ["g2", "s1"] },
      { skills: [skill("g1", "G1"), skill("g2", "G2"), skill("s1", "S1")], globalSkillIds: ["g1", "g2"] },
    );
    const summary = screen.getByTestId("stage-effective-skills").textContent ?? "";
    expect(summary).toContain("G1");
    expect(summary).toContain("G2");
    expect(summary).toContain("S1");
  });

  it("registered skills render as checkboxes that toggle stage.skillIds", () => {
    const { onChange } = renderStage({ skillIds: [] }, { skills: [skill("sk", "My Skill")] });
    fireEvent.click(screen.getByTestId("stage-skill-sk"));
    expect(onChange).toHaveBeenCalledWith(expect.objectContaining({ skillIds: ["sk"] }));
  });
});
