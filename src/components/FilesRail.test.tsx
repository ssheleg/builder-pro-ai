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
import { strings } from "../strings";

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
      toast: null, toastQueue: [],
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
    expect(screen.getByRole("button", { name: strings.files.openPanel })).toBeTruthy();
  });

  it("clicking the reopen affordance opens the rail (mounts FileTree)", async () => {
    render(<FilesRail workspace={ws} />);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: strings.files.openPanel }));
    });
    expect(screen.getByRole("tree")).toBeTruthy();
  });

  it("renders FileTree + FilePreview stacked when filesRailOpen is true", () => {
    useAppStore.setState({ filesRailOpen: true }, false);
    render(<FilesRail workspace={ws} />);
    expect(screen.getByRole("tree")).toBeTruthy();
    expect(screen.getByText(strings.files.selectFile)).toBeTruthy();
  });

  it("the collapse toggle sets filesRailOpen back to false", async () => {
    useAppStore.setState({ filesRailOpen: true }, false);
    render(<FilesRail workspace={ws} />);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: strings.files.collapsePanel }));
    });
    expect(useAppStore.getState().filesRailOpen).toBe(false);
  });

  it("the show-ignored toggle flips store state and invalidates cached dirs, which the still-expanded FileTree immediately refetches", async () => {
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
    // The dir stays expanded — invalidation is a refetch-in-place, not a collapse — so the
    // mounted FileTree's own effect immediately re-fetches it with the NEW showIgnored value
    // rather than leaving the cache empty until some later user action.
    expect(useAppStore.getState().expanded["/proj\t"]).toBe(true);
    expect(listDirMock).toHaveBeenCalledWith("/proj", "", true);
  });

  it("shows the watch-paused affordance only when watchPaused is true", () => {
    useAppStore.setState({ filesRailOpen: true, watchPaused: false }, false);
    const { rerender } = render(<FilesRail workspace={ws} />);
    expect(screen.queryByText(strings.files.liveUpdatesPaused)).toBeNull();

    act(() => useAppStore.getState().setWatchPaused(true));
    rerender(<FilesRail workspace={ws} />);
    expect(screen.getByText(strings.files.liveUpdatesPaused)).toBeTruthy();
  });

  it("clicking the watch-paused affordance restarts the watch, clears watchPaused, and the still-expanded FileTree refetches", async () => {
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
      fireEvent.click(screen.getByText(strings.files.liveUpdatesPaused));
    });
    expect(startWorkspaceWatchMock).toHaveBeenCalledWith(["/proj"], false);
    expect(useAppStore.getState().watchPaused).toBe(false);
    // The dir stays expanded — invalidation is a refetch-in-place, not a collapse — so the
    // mounted FileTree's own effect immediately re-fetches it rather than leaving a dead cache.
    expect(useAppStore.getState().expanded["/proj\t"]).toBe(true);
    expect(listDirMock).toHaveBeenCalledWith("/proj", "", false);
  });

  it("re-flags the watch as paused when restarting it rejects, so the tree never falsely reads 'live' (C2)", async () => {
    startWorkspaceWatchMock.mockReset().mockRejectedValue(new Error("daemon down"));
    useAppStore.setState({ filesRailOpen: true, watchPaused: true, expanded: {}, treeCache: {} }, false);
    render(<FilesRail workspace={ws} />);
    await act(async () => {
      fireEvent.click(screen.getByText(strings.files.liveUpdatesPaused));
    });
    expect(startWorkspaceWatchMock).toHaveBeenCalled();
    // The optimistic setWatchPaused(false) is corrected back to true by the rejection handler.
    expect(useAppStore.getState().watchPaused).toBe(true);
  });
});

describe("collapsed-rail watch-paused cue (AUD-2026-07-19-08)", () => {
  it("shows a warn dot on the collapsed strip while watchPaused", () => {
    useAppStore.setState({ filesRailOpen: false, watchPaused: true }, false);
    render(<FilesRail workspace={ws} />);
    expect(screen.getByTestId("files-rail-collapsed-paused")).toBeTruthy();
  });

  it("no dot on the collapsed strip while the watch is healthy", () => {
    useAppStore.setState({ filesRailOpen: false, watchPaused: false }, false);
    render(<FilesRail workspace={ws} />);
    expect(screen.queryByTestId("files-rail-collapsed-paused")).toBeNull();
  });
});
