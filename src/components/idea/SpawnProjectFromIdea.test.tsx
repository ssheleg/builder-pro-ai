// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, fireEvent, waitFor } from "@testing-library/react";

const orchdCreateProjectMock = vi.fn();
const orchdSetIdeaProjectMock = vi.fn();
const describeOrchdErrorMock = vi.fn((..._a: unknown[]) => "orchestrator: error");
vi.mock("../../ipc/orchd", () => ({
  orchdCreateProject: (...a: unknown[]) => orchdCreateProjectMock(...a),
  orchdSetIdeaProject: (...a: unknown[]) => orchdSetIdeaProjectMock(...a),
  describeOrchdError: (...a: unknown[]) => describeOrchdErrorMock(...a),
}));

const pickFolderMock = vi.fn();
const createWorkspaceMock = vi.fn();
vi.mock("../../ipc/commands", () => ({
  pickFolder: (...a: unknown[]) => pickFolderMock(...a),
  createWorkspace: (...a: unknown[]) => createWorkspaceMock(...a),
}));

import { SpawnProjectFromIdea } from "./SpawnProjectFromIdea";
import { useAppStore } from "../../store/store";
import type { Idea } from "../../ipc/orchd-types";

const idea: Idea = {
  id: "idea-1",
  projectId: null,
  title: "Validate demand",
  body: "",
  lifecycle: "captured",
  createdAt: 1,
  updatedAt: 1,
};

afterEach(cleanup);

beforeEach(() => {
  orchdCreateProjectMock.mockReset();
  orchdSetIdeaProjectMock.mockReset();
  describeOrchdErrorMock.mockReset().mockReturnValue("orchestrator: error");
  pickFolderMock.mockReset();
  createWorkspaceMock.mockReset();
  useAppStore.setState({ workspaces: {}, projects: [], ideas: [], toast: null, orchdDown: false }, false);
});

describe("SpawnProjectFromIdea", () => {
  it("calls pickFolder -> createWorkspace -> orchdCreateProject -> orchdSetIdeaProject IN ORDER, with the idea's title as the project name", async () => {
    pickFolderMock.mockResolvedValue("/Users/me/projects/demand-check");
    createWorkspaceMock.mockResolvedValue({
      id: "w1",
      name: "demand-check",
      rootPath: "/Users/me/projects/demand-check",
      roots: ["/Users/me/projects/demand-check"],
    });
    orchdCreateProjectMock.mockResolvedValue({
      id: "p1",
      name: idea.title,
      description: "",
      status: "active",
      workspaceIds: ["w1"],
      createdAt: 1,
      updatedAt: 1,
    });
    orchdSetIdeaProjectMock.mockResolvedValue({ ...idea, projectId: "p1" });

    render(<SpawnProjectFromIdea idea={idea} />);
    fireEvent.click(screen.getByTestId(`spawn-project-${idea.id}`));

    await waitFor(() => expect(orchdSetIdeaProjectMock).toHaveBeenCalled());

    expect(pickFolderMock).toHaveBeenCalled();
    expect(createWorkspaceMock).toHaveBeenCalledWith("demand-check", "/Users/me/projects/demand-check");
    expect(orchdCreateProjectMock).toHaveBeenCalledWith(idea.title, "", ["w1"]);
    expect(orchdSetIdeaProjectMock).toHaveBeenCalledWith(idea.id, "p1");

    const pickOrder = pickFolderMock.mock.invocationCallOrder[0]!;
    const wsOrder = createWorkspaceMock.mock.invocationCallOrder[0]!;
    const projectOrder = orchdCreateProjectMock.mock.invocationCallOrder[0]!;
    const setIdeaOrder = orchdSetIdeaProjectMock.mock.invocationCallOrder[0]!;
    expect(pickOrder).toBeLessThan(wsOrder);
    expect(wsOrder).toBeLessThan(projectOrder);
    expect(projectOrder).toBeLessThan(setIdeaOrder);
  });

  it("upserts the freshly-created workspace into the store immediately (no wait on workspace://created)", async () => {
    pickFolderMock.mockResolvedValue("/p/x");
    createWorkspaceMock.mockResolvedValue({ id: "w9", name: "x", rootPath: "/p/x", roots: ["/p/x"] });
    orchdCreateProjectMock.mockResolvedValue({
      id: "p9", name: idea.title, description: "", status: "active", workspaceIds: ["w9"], createdAt: 1, updatedAt: 1,
    });
    orchdSetIdeaProjectMock.mockResolvedValue({ ...idea, projectId: "p9" });

    render(<SpawnProjectFromIdea idea={idea} />);
    fireEvent.click(screen.getByTestId(`spawn-project-${idea.id}`));

    await waitFor(() => expect(useAppStore.getState().workspaces["w9"]).toBeTruthy());
  });

  it("is a no-op when the folder picker is cancelled: no workspace/project created", async () => {
    pickFolderMock.mockResolvedValue(null);
    render(<SpawnProjectFromIdea idea={idea} />);
    fireEvent.click(screen.getByTestId(`spawn-project-${idea.id}`));

    await waitFor(() => expect(pickFolderMock).toHaveBeenCalledTimes(1));
    expect(createWorkspaceMock).not.toHaveBeenCalled();
    expect(orchdCreateProjectMock).not.toHaveBeenCalled();
    expect(orchdSetIdeaProjectMock).not.toHaveBeenCalled();
  });

  it("a failed orchdCreateProject surfaces the mapped error and never calls orchdSetIdeaProject", async () => {
    pickFolderMock.mockResolvedValue("/p/x");
    createWorkspaceMock.mockResolvedValue({ id: "w1", name: "x", rootPath: "/p/x", roots: ["/p/x"] });
    orchdCreateProjectMock.mockRejectedValue({ kind: "daemon", code: "Validation", message: "bad" });

    render(<SpawnProjectFromIdea idea={idea} />);
    fireEvent.click(screen.getByTestId(`spawn-project-${idea.id}`));

    await waitFor(() => expect(describeOrchdErrorMock).toHaveBeenCalled());
    expect(orchdSetIdeaProjectMock).not.toHaveBeenCalled();
    expect(useAppStore.getState().toast).toBe("orchestrator: error");
  });

  it("while orchdDown: the button is disabled and clicking it never calls any wrapper", () => {
    useAppStore.setState({ orchdDown: true }, false);
    render(<SpawnProjectFromIdea idea={idea} />);

    const button = screen.getByTestId(`spawn-project-${idea.id}`) as HTMLButtonElement;
    expect(button.disabled).toBe(true);

    fireEvent.click(button);
    expect(pickFolderMock).not.toHaveBeenCalled();
    expect(orchdCreateProjectMock).not.toHaveBeenCalled();
  });
});
