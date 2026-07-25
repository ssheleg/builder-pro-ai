// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, fireEvent, waitFor, within } from "@testing-library/react";

vi.mock("../../ipc/orchd", () => ({
  orchdListWorkflows: vi.fn().mockResolvedValue([]),
  orchdUpsertWorkflow: vi.fn(),
  orchdDeleteWorkflow: vi.fn(),
  describeOrchdError: (e: unknown) => `mapped: ${JSON.stringify(e)}`,
}));

import { WorkflowsView } from "./WorkflowsView";
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
    description: over.description ?? "does the thing",
    scope: over.scope ?? "global",
    projectId: over.projectId ?? null,
    defaultAgent: over.defaultAgent ?? "claude-code",
    stages: over.stages ?? [],
    globalSkillIds: over.globalSkillIds ?? [],
    supervisor: over.supervisor ?? supervisor(),
    fileState: over.fileState ?? "present",
    jsonPath: over.jsonPath ?? "/tmp/wf.json",
    hash: over.hash ?? "h",
    createdAt: 1,
    updatedAt: 1,
  };
}

afterEach(cleanup);

beforeEach(() => {
  useAppStore.setState(
    {
      workflows: [],
      projects: [],
      skills: [],
      activeProjectId: null,
      orchdDown: false,
      toast: null,
      toastQueue: [],
      // Deterministic action spies — isolate the view from the real ipc round-trip.
      refreshWorkflows: vi.fn().mockResolvedValue(undefined),
      deleteWorkflow: vi.fn().mockResolvedValue(undefined),
    },
    false,
  );
});

describe("WorkflowsView (SCR-01)", () => {
  it("renders the empty state when there are no workflows", () => {
    render(<WorkflowsView />);
    expect(screen.getByTestId("workflows-empty")).toBeTruthy();
    expect(screen.getByText(strings.workflows.library.emptyTitle)).toBeTruthy();
  });

  it("refreshes the workflow list on mount", () => {
    render(<WorkflowsView />);
    expect(useAppStore.getState().refreshWorkflows).toHaveBeenCalled();
  });

  it("renders one row per workflow with stage- and skills-count and scope badge", () => {
    useAppStore.setState(
      {
        workflows: [
          wf({ id: "a", name: "Alpha", scope: "global", stages: [{ id: "s", name: "x", prompt: "y", skillIds: [], agent: null, contextScope: "inherit", outputs: [], gate: "auto" }], globalSkillIds: ["g1"] }),
          wf({ id: "b", name: "Beta", scope: "project", projectId: "p1" }),
        ],
      },
      false,
    );
    render(<WorkflowsView />);
    expect(screen.getByTestId("workflow-row-a")).toBeTruthy();
    expect(screen.getByTestId("workflow-row-b")).toBeTruthy();
    expect(screen.getByText(strings.workflows.library.stagesCount(1))).toBeTruthy();
    expect(screen.getByTestId("workflow-scope-a").textContent).toContain("global");
    expect(screen.getByTestId("workflow-scope-b").textContent).toContain("project");
  });

  it("scope filter narrows the list to a single scope", () => {
    useAppStore.setState(
      {
        workflows: [
          wf({ id: "g", name: "Global one", scope: "global" }),
          wf({ id: "p", name: "Project one", scope: "project", projectId: "p1" }),
        ],
      },
      false,
    );
    render(<WorkflowsView />);
    // Both shown under "All".
    expect(screen.getByTestId("workflow-row-g")).toBeTruthy();
    expect(screen.getByTestId("workflow-row-p")).toBeTruthy();
    // Switch to "Project" → only the project workflow remains.
    fireEvent.click(screen.getByRole("radio", { name: strings.workflows.library.scopeProject }));
    expect(screen.queryByTestId("workflow-row-g")).toBeNull();
    expect(screen.getByTestId("workflow-row-p")).toBeTruthy();
  });

  it("delete confirms first: a cancelled confirm does NOT call deleteWorkflow", () => {
    useAppStore.setState({ workflows: [wf({ id: "a", name: "Alpha" })] }, false);
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(false);
    render(<WorkflowsView />);

    fireEvent.click(screen.getByTestId("workflow-delete-a"));

    expect(confirmSpy).toHaveBeenCalledWith(strings.workflows.library.deleteConfirm("Alpha"));
    expect(useAppStore.getState().deleteWorkflow).not.toHaveBeenCalled();
    confirmSpy.mockRestore();
  });

  it("delete calls deleteWorkflow when the confirm is accepted", async () => {
    useAppStore.setState({ workflows: [wf({ id: "a", name: "Alpha" })] }, false);
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(true);
    render(<WorkflowsView />);

    fireEvent.click(screen.getByTestId("workflow-delete-a"));

    await waitFor(() =>
      expect(useAppStore.getState().deleteWorkflow).toHaveBeenCalledWith("a"),
    );
    confirmSpy.mockRestore();
  });

  it('"Open" swaps the list for the editor', () => {
    useAppStore.setState({ workflows: [wf({ id: "a", name: "Alpha" })] }, false);
    render(<WorkflowsView />);
    fireEvent.click(screen.getByTestId("workflow-open-a"));
    expect(screen.getByTestId("workflow-editor")).toBeTruthy();
    expect((screen.getByTestId("workflow-name") as HTMLInputElement).value).toBe("Alpha");
  });

  it('WIP-4: a global row\'s "Run →" opens the picker with THAT workflow preselected, not the first', () => {
    useAppStore.setState(
      {
        workflows: [
          wf({ id: "a", name: "Alpha", scope: "global" }),
          wf({ id: "b", name: "Beta", scope: "global" }),
        ],
      },
      false,
    );
    render(<WorkflowsView />);

    fireEvent.click(screen.getByTestId("workflow-run-b"));

    // The clicked row's workflow wins over the positional first-in-list default.
    const radioA = within(screen.getByTestId("run-workflow-option-a")).getByRole(
      "radio",
    ) as HTMLInputElement;
    const radioB = within(screen.getByTestId("run-workflow-option-b")).getByRole(
      "radio",
    ) as HTMLInputElement;
    expect(radioB.checked).toBe(true);
    expect(radioA.checked).toBe(false);
    // The honesty boundary rides along: the run is still a stub — the pending note, no execution.
    expect(screen.getByTestId("run-workflow-pending").textContent).toBe(
      strings.workflows.run.pendingNote,
    );
  });

  it('WIP-4: a project-scoped row gets NO "Run →" (the picker lists global workflows only); other actions stay', () => {
    useAppStore.setState(
      {
        workflows: [
          wf({ id: "g", name: "Global one", scope: "global" }),
          wf({ id: "p", name: "Project one", scope: "project", projectId: "p1" }),
        ],
      },
      false,
    );
    render(<WorkflowsView />);

    expect(screen.getByTestId("workflow-run-g")).toBeTruthy();
    expect(screen.queryByTestId("workflow-run-p")).toBeNull();
    // Open/Duplicate/Delete are unaffected — only the Run action is scope-gated.
    expect(screen.getByTestId("workflow-open-p")).toBeTruthy();
    expect(screen.getByTestId("workflow-duplicate-p")).toBeTruthy();
    expect(screen.getByTestId("workflow-delete-p")).toBeTruthy();
  });

  it('"+ New workflow" opens a blank editor', () => {
    render(<WorkflowsView />);
    fireEvent.click(screen.getByTestId("workflows-new"));
    expect(screen.getByTestId("workflow-editor")).toBeTruthy();
    expect((screen.getByTestId("workflow-name") as HTMLInputElement).value).toBe("");
  });
});
