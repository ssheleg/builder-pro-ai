// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, fireEvent } from "@testing-library/react";
import { RunWorkflowPicker } from "./RunWorkflowPicker";
import { useAppStore } from "../../store/store";
import { strings } from "../../strings";
import type { SupervisorConfig, Workflow } from "../../ipc/orchd-types";

function supervisor(over: Partial<SupervisorConfig> = {}): SupervisorConfig {
  return {
    enabled: over.enabled ?? false,
    delegatedClasses: over.delegatedClasses ?? [],
    instruction: over.instruction ?? "",
    customRules: over.customRules ?? [],
  };
}

function wf(over: Partial<Workflow> = {}): Workflow {
  return {
    id: over.id ?? "wf-1",
    name: over.name ?? "Ship it",
    description: "",
    scope: over.scope ?? "global",
    projectId: over.projectId ?? null,
    defaultAgent: "claude-code",
    stages: over.stages ?? [],
    globalSkillIds: [],
    supervisor: over.supervisor ?? supervisor(),
    fileState: "present",
    jsonPath: "/tmp/wf.json",
    hash: "h",
    createdAt: 1,
    updatedAt: 1,
  };
}

afterEach(cleanup);

beforeEach(() => {
  useAppStore.setState({ workflows: [] }, false);
});

describe("RunWorkflowPicker (SCR-04) — the honest trigger stub", () => {
  it("titles the modal with the project and ALWAYS shows the S6b pending note", () => {
    useAppStore.setState({ workflows: [wf({ id: "g", scope: "global" })] }, false);
    render(<RunWorkflowPicker open onClose={vi.fn()} projectName="Acme" />);
    expect(screen.getByText(strings.workflows.run.title("Acme"))).toBeTruthy();
    expect(screen.getByTestId("run-workflow-pending").textContent).toBe(
      strings.workflows.run.pendingNote,
    );
  });

  it("lists only GLOBAL workflows as radio options", () => {
    useAppStore.setState(
      {
        workflows: [
          wf({ id: "g", scope: "global", name: "Global one" }),
          wf({ id: "p", scope: "project", projectId: "p1", name: "Project one" }),
        ],
      },
      false,
    );
    render(<RunWorkflowPicker open onClose={vi.fn()} projectName="Acme" />);
    expect(screen.getByTestId("run-workflow-option-g")).toBeTruthy();
    expect(screen.queryByTestId("run-workflow-option-p")).toBeNull();
  });

  it("clicking Run FABRICATES NO execution — it is inert, the picker stays open on its pending note", () => {
    useAppStore.setState({ workflows: [wf({ id: "g", scope: "global" })] }, false);
    const onClose = vi.fn();
    render(<RunWorkflowPicker open onClose={onClose} projectName="Acme" />);

    const runBtn = screen.getByTestId("run-workflow-run");
    fireEvent.click(runBtn);

    // No run was created: nothing closed the modal, nothing mutated the store, the pending note is
    // still the whole story (there is no run ipc to call — the executor lands in S6b).
    expect(onClose).not.toHaveBeenCalled();
    expect(screen.getByTestId("run-workflow-pending")).toBeTruthy();
    expect(useAppStore.getState().workflows).toHaveLength(1);
  });

  it("shows the empty note (and disables Run) when there are no global workflows", () => {
    useAppStore.setState({ workflows: [wf({ id: "p", scope: "project", projectId: "p1" })] }, false);
    render(<RunWorkflowPicker open onClose={vi.fn()} projectName="Acme" />);
    expect(screen.getByTestId("run-workflow-empty").textContent).toBe(
      strings.workflows.run.noGlobalWorkflows,
    );
    expect((screen.getByTestId("run-workflow-run") as HTMLButtonElement).disabled).toBe(true);
  });

  it("Cancel closes the picker", () => {
    const onClose = vi.fn();
    render(<RunWorkflowPicker open onClose={onClose} projectName="Acme" />);
    fireEvent.click(screen.getByTestId("run-workflow-cancel"));
    expect(onClose).toHaveBeenCalled();
  });
});
