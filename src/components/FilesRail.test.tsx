// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, act, fireEvent } from "@testing-library/react";

const listDirMock = vi.fn().mockResolvedValue([]);
const startWorkspaceWatchMock = vi.fn().mockResolvedValue(undefined);
vi.mock("../ipc/fs", () => ({
  listDir: (...a: unknown[]) => listDirMock(...a),
  readFilePreview: vi.fn().mockResolvedValue({ kind: "text", content: "", truncated: false, size: 0 }),
  createFile: vi.fn(),
  createDir: vi.fn(),
  renameEntry: vi.fn(),
  deleteEntry: vi.fn(),
  revealInFinder: vi.fn(),
  openExternal: vi.fn(),
  startWorkspaceWatch: (...a: unknown[]) => startWorkspaceWatchMock(...a),
  stopWorkspaceWatch: vi.fn().mockResolvedValue(undefined),
}));
vi.mock("../ipc/commands", () => ({
  pickFolder: vi.fn(),
  addWorkspaceRoot: vi.fn(),
}));

import { FilesRail } from "./FilesRail";
import { useAppStore } from "../store/store";
import type { Workspace } from "../ipc/types";

const ws: Workspace = { id: "w1", name: "proj", rootPath: "/proj", roots: ["/proj"] };

afterEach(cleanup);
beforeEach(() => {
  listDirMock.mockReset().mockResolvedValue([]);
  startWorkspaceWatchMock.mockReset().mockResolvedValue(undefined);
  useAppStore.setState(
    {
      expanded: {},
      treeCache: {},
      selectedFile: null,
      showIgnored: false,
      filesRailOpen: false,
      watchPaused: false,
      toast: null,
      workspaces: {},
    },
    false,
  );
});

describe("FilesRail", () => {
  it("renders nothing when there is no active workspace", () => {
    const { container } = render(<FilesRail workspace={undefined} />);
    expect(container.innerHTML).toBe("");
  });

  it("renders collapsed (a reopen affordance, not the tree) when filesRailOpen is false", () => {
    render(<FilesRail workspace={ws} />);
    expect(screen.queryByRole("tree")).toBeNull();
    expect(screen.getByRole("button", { name: /открыть панель файлов/i })).toBeTruthy();
  });

  it("clicking the reopen affordance opens the rail (mounts FileTree)", async () => {
    render(<FilesRail workspace={ws} />);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /открыть панель файлов/i }));
    });
    expect(screen.getByRole("tree")).toBeTruthy();
  });

  it("renders FileTree + FilePreview stacked when filesRailOpen is true", () => {
    useAppStore.setState({ filesRailOpen: true }, false);
    render(<FilesRail workspace={ws} />);
    expect(screen.getByRole("tree")).toBeTruthy();
    expect(screen.getByText(/выберите файл/i)).toBeTruthy();
  });

  it("the collapse toggle sets filesRailOpen back to false", async () => {
    useAppStore.setState({ filesRailOpen: true }, false);
    render(<FilesRail workspace={ws} />);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /свернуть панель файлов/i }));
    });
    expect(useAppStore.getState().filesRailOpen).toBe(false);
  });

  it("the show-ignored toggle flips store state and invalidates cached dirs", async () => {
    useAppStore.setState(
      {
        filesRailOpen: true,
        expanded: { "/proj\t": true },
        treeCache: { "/proj\t": [] },
      },
      false,
    );
    render(<FilesRail workspace={ws} />);
    await act(async () => {
      fireEvent.click(screen.getByRole("checkbox"));
    });
    expect(useAppStore.getState().showIgnored).toBe(true);
    expect(useAppStore.getState().treeCache["/proj\t"]).toBeUndefined();
  });

  it("shows the watch-paused affordance only when watchPaused is true", () => {
    useAppStore.setState({ filesRailOpen: true, watchPaused: false }, false);
    const { rerender } = render(<FilesRail workspace={ws} />);
    expect(screen.queryByText(/live-обновления на паузе/i)).toBeNull();

    act(() => useAppStore.getState().setWatchPaused(true));
    rerender(<FilesRail workspace={ws} />);
    expect(screen.getByText(/live-обновления на паузе/i)).toBeTruthy();
  });

  it("clicking the watch-paused affordance restarts the watch and clears watchPaused", async () => {
    useAppStore.setState(
      {
        filesRailOpen: true,
        watchPaused: true,
        expanded: { "/proj\t": true },
        treeCache: { "/proj\t": [] },
      },
      false,
    );
    render(<FilesRail workspace={ws} />);
    await act(async () => {
      fireEvent.click(screen.getByText(/live-обновления на паузе/i));
    });
    expect(startWorkspaceWatchMock).toHaveBeenCalledWith(["/proj"], false);
    expect(useAppStore.getState().watchPaused).toBe(false);
    expect(useAppStore.getState().treeCache["/proj\t"]).toBeUndefined();
  });
});
