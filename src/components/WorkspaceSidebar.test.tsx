// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, act, fireEvent } from "@testing-library/react";

const pickFolderMock = vi.fn();
const createWorkspaceMock = vi.fn();
vi.mock("../ipc/commands", () => ({
  pickFolder: (...a: unknown[]) => pickFolderMock(...a),
  createWorkspace: (...a: unknown[]) => createWorkspaceMock(...a),
}));

import { WorkspaceSidebar } from "./WorkspaceSidebar";
import { useAppStore } from "../store/store";
import type { Workspace } from "../ipc/types";

const wsA: Workspace = { id: "w1", name: "alpha", rootPath: "/p/alpha", roots: ["/p/alpha"] };
const wsB: Workspace = { id: "w2", name: "beta", rootPath: "/p/beta", roots: ["/p/beta"] };

afterEach(cleanup);
beforeEach(() => {
  pickFolderMock.mockReset();
  createWorkspaceMock.mockReset();
  createWorkspaceMock.mockResolvedValue({ id: "w3", name: "gamma", rootPath: "/p/gamma", roots: ["/p/gamma"] });
  useAppStore.setState(
    { sessions: {}, workspaces: { w1: wsA, w2: wsB }, activeSessionId: null, daemonConnected: true },
    false,
  );
});

describe("WorkspaceSidebar", () => {
  it("renders one entry per workspace", () => {
    render(<WorkspaceSidebar activeWorkspaceId={null} onSelectWorkspace={() => {}} />);
    expect(screen.getByText("alpha")).toBeTruthy();
    expect(screen.getByText("beta")).toBeTruthy();
  });

  it("clicking a workspace calls onSelectWorkspace with its id", () => {
    const onSelect = vi.fn();
    render(<WorkspaceSidebar activeWorkspaceId={null} onSelectWorkspace={onSelect} />);
    fireEvent.click(screen.getByText("alpha"));
    expect(onSelect).toHaveBeenCalledWith("w1");
  });

  it('"Add workspace" opens the folder picker then creates a workspace named after the basename', async () => {
    pickFolderMock.mockResolvedValue("/Users/me/projects/my-app");
    render(<WorkspaceSidebar activeWorkspaceId={null} onSelectWorkspace={() => {}} />);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /add workspace/i }));
    });
    expect(pickFolderMock).toHaveBeenCalledTimes(1);
    expect(createWorkspaceMock).toHaveBeenCalledWith("my-app", "/Users/me/projects/my-app");
  });

  it("is a no-op when the folder picker is cancelled (null)", async () => {
    pickFolderMock.mockResolvedValue(null);
    render(<WorkspaceSidebar activeWorkspaceId={null} onSelectWorkspace={() => {}} />);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /add workspace/i }));
    });
    expect(pickFolderMock).toHaveBeenCalledTimes(1);
    expect(createWorkspaceMock).not.toHaveBeenCalled();
  });
});
