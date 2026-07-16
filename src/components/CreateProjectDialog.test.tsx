// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, fireEvent, waitFor, within, act } from "@testing-library/react";

const orchdCreateProjectMock = vi.fn();
const describeOrchdErrorMock = vi.fn((..._a: unknown[]) => "orchestrator: error");
vi.mock("../ipc/orchd", () => ({
  orchdCreateProject: (...a: unknown[]) => orchdCreateProjectMock(...a),
  describeOrchdError: (...a: unknown[]) => describeOrchdErrorMock(...a),
}));

const pickFolderMock = vi.fn();
const createWorkspaceMock = vi.fn();
vi.mock("../ipc/commands", () => ({
  pickFolder: (...a: unknown[]) => pickFolderMock(...a),
  createWorkspace: (...a: unknown[]) => createWorkspaceMock(...a),
}));

import { CreateProjectDialog } from "./CreateProjectDialog";
import { useAppStore } from "../store/store";
import { strings } from "../strings";
import type { Workspace } from "../ipc/types";
import type { Project } from "../ipc/orchd-types";

const wsA: Workspace = { id: "w1", name: "alpha", rootPath: "/p/alpha", roots: ["/p/alpha"] };
const wsB: Workspace = { id: "w2", name: "beta", rootPath: "/p/beta", roots: ["/p/beta"] };

function makeProject(over: Partial<Project> = {}): Project {
  return {
    id: "p1",
    name: "Proj",
    description: "",
    status: "active",
    workspaceIds: ["w1"],
    createdAt: 1,
    updatedAt: 1,
    ...over,
  };
}

afterEach(cleanup);

beforeEach(() => {
  orchdCreateProjectMock.mockReset();
  describeOrchdErrorMock.mockReset().mockReturnValue("orchestrator: error");
  pickFolderMock.mockReset();
  createWorkspaceMock.mockReset();
  useAppStore.setState({ projects: [], workspaces: { w2: wsB }, toast: null, toastQueue: [] }, false);
});

describe("CreateProjectDialog", () => {
  it("renders name/description fields and only UNLINKED workspaces (w1 is linked to p1, so hidden)", () => {
    useAppStore.setState({ projects: [makeProject({ workspaceIds: ["w1"] })], workspaces: { w1: wsA, w2: wsB } }, false);
    render(<CreateProjectDialog onClose={() => {}} />);

    expect(screen.getByTestId("create-project-name")).toBeTruthy();
    expect(screen.getByTestId("create-project-description")).toBeTruthy();
    expect(screen.queryByTestId("create-project-ws-w1")).toBeNull();
    expect(screen.getByTestId("create-project-ws-w2")).toBeTruthy();
  });

  it("blocks submit at 0 selected workspaces: inline message shown, orchdCreateProject not called", () => {
    render(<CreateProjectDialog onClose={() => {}} />);

    expect(screen.getByTestId("create-project-blocked")).toBeTruthy();
    fireEvent.change(screen.getByTestId("create-project-name"), { target: { value: "New Proj" } });
    fireEvent.click(screen.getByTestId("create-project-submit"));

    expect(orchdCreateProjectMock).not.toHaveBeenCalled();
  });

  it("enables submit once >=1 workspace is selected and creates the project", async () => {
    orchdCreateProjectMock.mockResolvedValue(makeProject());
    const onClose = vi.fn();
    render(<CreateProjectDialog onClose={onClose} />);

    fireEvent.change(screen.getByTestId("create-project-name"), { target: { value: "New Proj" } });
    fireEvent.change(screen.getByTestId("create-project-description"), { target: { value: "desc" } });
    fireEvent.click(screen.getByTestId("create-project-ws-w2"));

    expect(screen.queryByTestId("create-project-blocked")).toBeNull();

    fireEvent.click(screen.getByTestId("create-project-submit"));

    await waitFor(() => {
      expect(orchdCreateProjectMock).toHaveBeenCalledWith("New Proj", "desc", ["w2"]);
      expect(onClose).toHaveBeenCalled();
    });
  });

  it("two rapid submit clicks create the project ONCE (double-submit guard, spec D6)", async () => {
    let resolveCreate!: (v: unknown) => void;
    orchdCreateProjectMock.mockImplementation(() => new Promise((res) => (resolveCreate = res)));
    render(<CreateProjectDialog onClose={() => {}} />);
    fireEvent.change(screen.getByTestId("create-project-name"), { target: { value: "Dup" } });
    fireEvent.click(screen.getByTestId("create-project-ws-w2"));

    const submit = screen.getByTestId("create-project-submit");
    fireEvent.click(submit);
    fireEvent.click(submit);

    expect(orchdCreateProjectMock).toHaveBeenCalledTimes(1);
    await act(async () => {
      resolveCreate(makeProject());
    });
  });

  it('inline "create workspace": pickFolder -> createWorkspace -> adds the new workspace to the selection', async () => {
    pickFolderMock.mockResolvedValue("/Users/me/projects/gamma");
    createWorkspaceMock.mockResolvedValue({ id: "w3", name: "gamma", rootPath: "/p/gamma", roots: ["/p/gamma"] });
    render(<CreateProjectDialog onClose={() => {}} />);

    fireEvent.click(screen.getByTestId("create-project-new-workspace"));

    await waitFor(() => {
      expect(createWorkspaceMock).toHaveBeenCalledWith("gamma", "/Users/me/projects/gamma");
    });
    await waitFor(() => {
      const cb = screen.getByTestId("create-project-ws-w3") as HTMLInputElement;
      expect(cb.checked).toBe(true);
    });
    expect(screen.queryByTestId("create-project-blocked")).toBeNull();
  });

  it("is a no-op when the folder picker is cancelled during inline workspace creation", async () => {
    pickFolderMock.mockResolvedValue(null);
    render(<CreateProjectDialog onClose={() => {}} />);

    fireEvent.click(screen.getByTestId("create-project-new-workspace"));

    await waitFor(() => expect(pickFolderMock).toHaveBeenCalledTimes(1));
    expect(createWorkspaceMock).not.toHaveBeenCalled();
  });

  it("cancel closes the dialog without creating a project", () => {
    const onClose = vi.fn();
    render(<CreateProjectDialog onClose={onClose} />);
    fireEvent.click(screen.getByTestId("create-project-cancel"));
    expect(onClose).toHaveBeenCalled();
    expect(orchdCreateProjectMock).not.toHaveBeenCalled();
  });

  it("a failed create shows the mapped error via a toast and keeps the dialog open", async () => {
    orchdCreateProjectMock.mockRejectedValue({ kind: "daemon", code: "Validation", message: "bad" });
    const onClose = vi.fn();
    render(<CreateProjectDialog onClose={onClose} />);

    fireEvent.change(screen.getByTestId("create-project-name"), { target: { value: "New Proj" } });
    fireEvent.click(screen.getByTestId("create-project-ws-w2"));
    fireEvent.click(screen.getByTestId("create-project-submit"));

    await waitFor(() => expect(describeOrchdErrorMock).toHaveBeenCalled());
    expect(onClose).not.toHaveBeenCalled();
    expect(useAppStore.getState().toast).toBe("orchestrator: error");
  });

  it("a failed create renders an in-dialog role=alert with the mapped message AND stays open", async () => {
    orchdCreateProjectMock.mockRejectedValue({ kind: "daemon", code: "Validation", message: "bad" });
    describeOrchdErrorMock.mockReturnValue("invalid data: bad");
    const onClose = vi.fn();
    render(<CreateProjectDialog onClose={onClose} />);

    fireEvent.change(screen.getByTestId("create-project-name"), { target: { value: "New Proj" } });
    fireEvent.click(screen.getByTestId("create-project-ws-w2"));
    fireEvent.click(screen.getByTestId("create-project-submit"));

    // Load-bearing surface: an in-dialog error line, visible while the dialog is still open, so a
    // concurrent toast clobbering the global queue-of-one never hides the failure.
    const alert = await screen.findByTestId("create-project-error");
    expect(alert.getAttribute("role")).toBe("alert");
    expect(alert.textContent).toContain("invalid data: bad");
    // The dialog itself must still be on screen (not close-and-toast-into-the-void).
    expect(screen.getByTestId("create-project-dialog")).toBeTruthy();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("focuses the name input when the dialog opens (dialog-atom initial focus)", () => {
    render(<CreateProjectDialog onClose={() => {}} />);
    expect(document.activeElement).toBe(screen.getByTestId("create-project-name"));
  });

  it("Escape fires the cancel/close path (same as the Cancel button)", () => {
    const onClose = vi.fn();
    render(<CreateProjectDialog onClose={onClose} />);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalled();
    expect(orchdCreateProjectMock).not.toHaveBeenCalled();
  });

  it("renders as a labelled dialog", () => {
    render(<CreateProjectDialog onClose={() => {}} />);
    const dialog = within(screen.getByTestId("create-project-dialog"));
    expect(dialog.getByText(strings.project.newProject)).toBeTruthy();
  });
});
