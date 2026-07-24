// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, fireEvent, waitFor } from "@testing-library/react";

vi.mock("../../ipc/orchd", () => ({
  orchdUpsertWorkflow: vi.fn(),
  describeOrchdError: (e: unknown) => `mapped: ${JSON.stringify(e)}`,
}));

import { WorkflowEditor, validateWorkflow, type WorkflowDraft } from "./WorkflowEditor";
import { useAppStore } from "../../store/store";
import { strings } from "../../strings";
import type { Stage } from "../../ipc/orchd-types";

function stage(over: Partial<Stage> = {}): Stage {
  return {
    id: over.id ?? `st-${Math.random()}`,
    name: over.name ?? "Draft",
    prompt: over.prompt ?? "do it",
    skillIds: over.skillIds ?? [],
    agent: over.agent ?? null,
    contextScope: over.contextScope ?? "inherit",
    outputs: over.outputs ?? [],
    gate: over.gate ?? "auto",
  };
}

function draft(over: Partial<WorkflowDraft> = {}): WorkflowDraft {
  return {
    id: over.id ?? "",
    name: over.name ?? "Ship it",
    description: over.description ?? "",
    scope: over.scope ?? "global",
    projectId: over.projectId ?? null,
    defaultAgent: over.defaultAgent ?? "claude-code",
    stages: over.stages ?? [stage()],
    globalSkillIds: over.globalSkillIds ?? [],
    supervisor: over.supervisor ?? { enabled: false, delegatedClasses: [], instruction: "", customRules: [] },
  };
}

afterEach(cleanup);

let upsertSpy: ReturnType<typeof vi.fn>;

beforeEach(() => {
  upsertSpy = vi.fn().mockResolvedValue(undefined);
  useAppStore.setState(
    { skills: [], projects: [], orchdDown: false, toast: null, toastQueue: [], upsertWorkflow: upsertSpy },
    false,
  );
});

describe("validateWorkflow (client twin of the daemon guard)", () => {
  it("blocks an empty name", () => {
    expect(validateWorkflow(draft({ name: "  " }))).toBe(strings.workflows.editor.errNameRequired);
  });
  it("blocks a workflow with no stages", () => {
    expect(validateWorkflow(draft({ stages: [] }))).toBe(strings.workflows.editor.errNoStages);
  });
  it("blocks a stage missing its prompt", () => {
    expect(validateWorkflow(draft({ stages: [stage({ name: "A", prompt: "" })] }))).toContain(
      "needs a name and a prompt",
    );
  });
  it("blocks an enabled CEO with an empty delegation scope", () => {
    expect(
      validateWorkflow(
        draft({ supervisor: { enabled: true, delegatedClasses: [], instruction: "", customRules: [] } }),
      ),
    ).toBe(strings.workflows.editor.errCeoNoClasses);
  });
  it("blocks a project-scoped workflow with no project", () => {
    expect(validateWorkflow(draft({ scope: "project", projectId: null }))).toBe(
      strings.workflows.editor.errProjectRequired,
    );
  });
  it("passes a complete global workflow", () => {
    expect(validateWorkflow(draft())).toBeNull();
  });
});

describe("WorkflowEditor (SCR-02) — terminal grouping", () => {
  it("an all-one-agent workflow renders exactly ONE terminal bracket", () => {
    render(
      <WorkflowEditor
        draft={draft({ stages: [stage({ id: "a" }), stage({ id: "b" })] })}
        onDone={vi.fn()}
      />,
    );
    expect(screen.getByTestId("workflow-terminal-0")).toBeTruthy();
    expect(screen.queryByTestId("workflow-terminal-1")).toBeNull();
    expect(screen.getByTestId("workflow-terminal-header-0").textContent).toBe(
      strings.workflows.editor.terminalHeader(1, "Claude Code", 2),
    );
  });

  it("an agent change opens a SECOND terminal bracket (boundary)", () => {
    render(
      <WorkflowEditor
        draft={draft({ stages: [stage({ id: "a", agent: null }), stage({ id: "b", agent: "hermes" })] })}
        onDone={vi.fn()}
      />,
    );
    expect(screen.getByTestId("workflow-terminal-0")).toBeTruthy();
    expect(screen.getByTestId("workflow-terminal-1")).toBeTruthy();
    expect(screen.getByTestId("workflow-terminal-header-1").textContent).toBe(
      strings.workflows.editor.terminalHeader(2, "Hermes", 1),
    );
  });
});

describe("WorkflowEditor (SCR-02) — Save validation", () => {
  it("an empty name blocks Save and shows the error; upsert is never called", () => {
    render(<WorkflowEditor draft={draft({ name: "" })} onDone={vi.fn()} />);
    fireEvent.click(screen.getByTestId("workflow-editor-save"));
    expect(screen.getByTestId("workflow-editor-error").textContent).toBe(
      strings.workflows.editor.errNameRequired,
    );
    expect(upsertSpy).not.toHaveBeenCalled();
  });

  it("an enabled CEO with no delegated classes blocks Save (the empty-scope guard)", () => {
    render(
      <WorkflowEditor
        draft={draft({ supervisor: { enabled: true, delegatedClasses: [], instruction: "", customRules: [] } })}
        onDone={vi.fn()}
      />,
    );
    fireEvent.click(screen.getByTestId("workflow-editor-save"));
    expect(screen.getByTestId("workflow-editor-error").textContent).toBe(
      strings.workflows.editor.errCeoNoClasses,
    );
    expect(upsertSpy).not.toHaveBeenCalled();
  });

  it("a valid workflow Saves via upsertWorkflow and calls onDone", async () => {
    const onDone = vi.fn();
    render(<WorkflowEditor draft={draft({ name: "Ship it" })} onDone={onDone} />);
    fireEvent.click(screen.getByTestId("workflow-editor-save"));
    await waitFor(() => expect(upsertSpy).toHaveBeenCalledTimes(1));
    expect(upsertSpy.mock.calls[0][0]).toMatchObject({ id: "", name: "Ship it", scope: "global" });
    await waitFor(() => expect(onDone).toHaveBeenCalled());
  });

  it("Save is disabled while orchdDown (honest degradation)", () => {
    useAppStore.setState({ orchdDown: true }, false);
    render(<WorkflowEditor draft={draft()} onDone={vi.fn()} />);
    expect((screen.getByTestId("workflow-editor-save") as HTMLButtonElement).disabled).toBe(true);
  });
});
