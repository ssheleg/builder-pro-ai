// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, fireEvent } from "@testing-library/react";

import { HomeGoals } from "./HomeGoals";
import { useAppStore } from "../store/store";
import type { Goal, Project } from "../ipc/orchd-types";
import { strings } from "../strings";

function makeProject(over: Partial<Project> = {}): Project {
  return {
    id: "p1",
    name: "Proj",
    description: "",
    status: "active",
    workspaceIds: [],
    createdAt: 1,
    updatedAt: 1,
    ...over,
  };
}

function makeGoal(over: Partial<Goal> = {}): Goal {
  return {
    id: "g1",
    projectId: "p1",
    parentId: null,
    kind: "strategic",
    title: "Strategic goal",
    body: "",
    ord: 0,
    status: "active",
    metricRefs: [],
    createdAt: 1,
    updatedAt: 1,
    ...over,
  };
}

const refreshGoalsMock = vi.fn(async () => {});

afterEach(cleanup);

beforeEach(() => {
  refreshGoalsMock.mockReset().mockResolvedValue(undefined);
  useAppStore.setState(
    {
      projects: [],
      goalsByProject: {},
      refreshGoals: refreshGoalsMock,
      view: "home",
      activeProjectId: null,
    },
    false,
  );
});

describe("HomeGoals", () => {
  it("renders nothing when there are no active projects", () => {
    const { container } = render(<HomeGoals />);
    expect(container.firstChild).toBeNull();
  });

  it("renders nothing for an archived-only project set", () => {
    useAppStore.setState({ projects: [makeProject({ status: "archived" })] }, false);
    const { container } = render(<HomeGoals />);
    expect(container.firstChild).toBeNull();
  });

  it("calls refreshGoals(projectId) on mount for an active project whose goals aren't loaded yet", () => {
    useAppStore.setState({ projects: [makeProject({ id: "p1" })], goalsByProject: {} }, false);
    render(<HomeGoals />);
    expect(refreshGoalsMock).toHaveBeenCalledWith("p1");
  });

  it("does NOT re-fetch a project whose goals are already loaded (even if empty)", () => {
    useAppStore.setState(
      { projects: [makeProject({ id: "p1" })], goalsByProject: { p1: [] } },
      false,
    );
    render(<HomeGoals />);
    expect(refreshGoalsMock).not.toHaveBeenCalled();
  });

  it("renders the strategic goal's title + direct additional children with status chips, per active project", () => {
    useAppStore.setState(
      {
        projects: [makeProject({ id: "p1", name: "Proj" })],
        goalsByProject: {
          p1: [
            makeGoal({ id: "strat", projectId: "p1", kind: "strategic", parentId: null, title: "Ship v1" }),
            makeGoal({
              id: "child1",
              projectId: "p1",
              kind: "additional",
              parentId: "strat",
              title: "Onboarding",
              status: "active",
              ord: 0,
            }),
            makeGoal({
              id: "child2",
              projectId: "p1",
              kind: "additional",
              parentId: "strat",
              title: "Billing",
              status: "achieved",
              ord: 1,
            }),
            // a GRANDCHILD (parentId is child1, not the strategic root) must NOT be rendered —
            // "direct additional children" only.
            makeGoal({
              id: "grandchild",
              projectId: "p1",
              kind: "additional",
              parentId: "child1",
              title: "Grandchild",
              ord: 0,
            }),
          ],
        },
      },
      false,
    );
    render(<HomeGoals />);

    const block = screen.getByTestId("home-goals-project-p1");
    expect(block.textContent).toContain("Ship v1");
    expect(screen.getByTestId("home-goals-chip-child1").textContent).toContain("Onboarding");
    expect(screen.getByTestId("home-goals-chip-child1").textContent).toContain(strings.goals.status.active);
    expect(screen.getByTestId("home-goals-chip-child2").textContent).toContain("Billing");
    expect(screen.getByTestId("home-goals-chip-child2").textContent).toContain(strings.goals.status.achieved);
    expect(screen.queryByTestId("home-goals-chip-grandchild")).toBeNull();
    expect(block.textContent).not.toContain("Grandchild");
  });

  it("clicking a project's block navigates via openProject(projectId)", () => {
    useAppStore.setState(
      {
        projects: [makeProject({ id: "p1", name: "Proj" })],
        goalsByProject: {
          p1: [makeGoal({ id: "strat", projectId: "p1", kind: "strategic", parentId: null, title: "Ship v1" })],
        },
      },
      false,
    );
    render(<HomeGoals />);
    fireEvent.click(screen.getByTestId("home-goals-project-p1"));
    expect(useAppStore.getState().view).toBe("project");
    expect(useAppStore.getState().activeProjectId).toBe("p1");
  });

  it("clicking a goal chip inside a project's block ALSO navigates to that project (bubbles to the block)", () => {
    useAppStore.setState(
      {
        projects: [makeProject({ id: "p1", name: "Proj" })],
        goalsByProject: {
          p1: [
            makeGoal({ id: "strat", projectId: "p1", kind: "strategic", parentId: null, title: "Ship v1" }),
            makeGoal({
              id: "child1",
              projectId: "p1",
              kind: "additional",
              parentId: "strat",
              title: "Onboarding",
            }),
          ],
        },
      },
      false,
    );
    render(<HomeGoals />);
    fireEvent.click(screen.getByTestId("home-goals-chip-child1"));
    expect(useAppStore.getState().activeProjectId).toBe("p1");
  });

  it("archived projects are excluded even when mixed with active ones", () => {
    useAppStore.setState(
      {
        projects: [
          makeProject({ id: "p1", name: "Active Proj", status: "active" }),
          makeProject({ id: "p2", name: "Archived Proj", status: "archived" }),
        ],
        goalsByProject: {
          p1: [makeGoal({ id: "strat1", projectId: "p1", title: "Goal 1" })],
          p2: [makeGoal({ id: "strat2", projectId: "p2", title: "Goal 2" })],
        },
      },
      false,
    );
    render(<HomeGoals />);
    expect(screen.getByTestId("home-goals-project-p1")).toBeTruthy();
    expect(screen.queryByTestId("home-goals-project-p2")).toBeNull();
  });
});
