// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, act, fireEvent } from "@testing-library/react";

const listDirMock = vi.fn();
const createFileMock = vi.fn();
const createDirMock = vi.fn();
const renameEntryMock = vi.fn();
const deleteEntryMock = vi.fn();
const revealInFinderMock = vi.fn();
const openExternalMock = vi.fn();
vi.mock("../ipc/fs", () => ({
  listDir: (...a: unknown[]) => listDirMock(...a),
  createFile: (...a: unknown[]) => createFileMock(...a),
  createDir: (...a: unknown[]) => createDirMock(...a),
  renameEntry: (...a: unknown[]) => renameEntryMock(...a),
  deleteEntry: (...a: unknown[]) => deleteEntryMock(...a),
  revealInFinder: (...a: unknown[]) => revealInFinderMock(...a),
  openExternal: (...a: unknown[]) => openExternalMock(...a),
}));

const pickFolderMock = vi.fn();
const addWorkspaceRootMock = vi.fn();
vi.mock("../ipc/commands", () => ({
  pickFolder: (...a: unknown[]) => pickFolderMock(...a),
  addWorkspaceRoot: (...a: unknown[]) => addWorkspaceRootMock(...a),
}));

import { FileTree } from "./FileTree";
import { useAppStore } from "../store/store";
import type { Workspace } from "../ipc/types";
import type { FsEntry } from "../ipc/fs";

const ws: Workspace = { id: "w1", name: "proj", rootPath: "/proj", roots: ["/proj"] };

function resetStore(): void {
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
}

afterEach(cleanup);
beforeEach(() => {
  listDirMock.mockReset();
  createFileMock.mockReset().mockResolvedValue(undefined);
  createDirMock.mockReset().mockResolvedValue(undefined);
  renameEntryMock.mockReset().mockResolvedValue(undefined);
  deleteEntryMock.mockReset().mockResolvedValue(undefined);
  revealInFinderMock.mockReset().mockResolvedValue(undefined);
  openExternalMock.mockReset().mockResolvedValue(undefined);
  pickFolderMock.mockReset();
  addWorkspaceRootMock.mockReset();
  resetStore();
});

describe("FileTree", () => {
  it("renders one collapsed root row per workspace root, named after its basename", () => {
    render(<FileTree workspace={ws} />);
    expect(screen.getByText("proj")).toBeTruthy();
    expect(listDirMock).not.toHaveBeenCalled();
  });

  it("(a) expanding a dir calls listDir ONCE, then serves cached children on collapse+re-expand", async () => {
    const entries: FsEntry[] = [
      { name: "sub", relPath: "sub", isDir: true, size: 0, isIgnored: false },
      { name: "a.txt", relPath: "a.txt", isDir: false, size: 3, isIgnored: false },
    ];
    listDirMock.mockResolvedValueOnce(entries);
    render(<FileTree workspace={ws} />);

    await act(async () => {
      fireEvent.click(screen.getByText("proj"));
    });
    expect(listDirMock).toHaveBeenCalledWith("/proj", "", false);
    expect(listDirMock).toHaveBeenCalledTimes(1);
    expect(screen.getByText("sub")).toBeTruthy();
    expect(screen.getByText("a.txt")).toBeTruthy();

    // collapse
    await act(async () => {
      fireEvent.click(screen.getByText("proj"));
    });
    expect(screen.queryByText("sub")).toBeNull();

    // re-expand — served from cache, no second fetch
    await act(async () => {
      fireEvent.click(screen.getByText("proj"));
    });
    expect(screen.getByText("sub")).toBeTruthy();
    expect(listDirMock).toHaveBeenCalledTimes(1);
  });

  it("(b) clearing a still-expanded dir's cache entry (simulating invalidation) triggers a refetch", async () => {
    const firstEntries: FsEntry[] = [
      { name: "a.txt", relPath: "a.txt", isDir: false, size: 3, isIgnored: false },
    ];
    const secondEntries: FsEntry[] = [
      { name: "b.txt", relPath: "b.txt", isDir: false, size: 5, isIgnored: false },
    ];
    listDirMock.mockResolvedValueOnce(firstEntries).mockResolvedValueOnce(secondEntries);
    render(<FileTree workspace={ws} />);

    await act(async () => {
      fireEvent.click(screen.getByText("proj"));
    });
    expect(listDirMock).toHaveBeenCalledTimes(1);
    expect(screen.getByText("a.txt")).toBeTruthy();

    // Directly clear the cache entry for the still-expanded root dir (spec §6.6 invalidation —
    // the containing-directory portion of an `fs://changed` batch), leaving `expanded` untouched.
    await act(async () => {
      useAppStore.setState((s) => {
        const rest = { ...s.treeCache };
        delete rest["/proj\t"];
        return { treeCache: rest };
      });
    });

    expect(listDirMock).toHaveBeenCalledTimes(2);
    expect(screen.getByText("b.txt")).toBeTruthy();
  });

  it("(b2) invalidateDirs on an expanded dir keeps it expanded and triggers a live refetch (not a collapse)", async () => {
    const firstEntries: FsEntry[] = [
      { name: "a.txt", relPath: "a.txt", isDir: false, size: 3, isIgnored: false },
    ];
    const secondEntries: FsEntry[] = [
      { name: "b.txt", relPath: "b.txt", isDir: false, size: 5, isIgnored: false },
    ];
    listDirMock.mockResolvedValueOnce(firstEntries).mockResolvedValueOnce(secondEntries);
    render(<FileTree workspace={ws} />);

    await act(async () => {
      fireEvent.click(screen.getByText("proj"));
    });
    expect(listDirMock).toHaveBeenCalledTimes(1);
    expect(screen.getByText("a.txt")).toBeTruthy();
    expect(useAppStore.getState().expanded["/proj\t"]).toBe(true);

    // The real store action (spec §5, wired to `fs://changed` by a later task) — must keep the
    // dir expanded (point refresh), not collapse the tree.
    await act(async () => {
      useAppStore.getState().invalidateDirs("/proj", ["*"]);
    });

    expect(useAppStore.getState().expanded["/proj\t"]).toBe(true);
    expect(listDirMock).toHaveBeenCalledTimes(2);
    expect(screen.getByText("b.txt")).toBeTruthy();
  });

  it("(c) a 10k-entry flattened list renders only a windowed subset (<500 DOM rows)", () => {
    const entries: FsEntry[] = Array.from({ length: 10_000 }, (_, i) => ({
      name: `file-${String(i).padStart(5, "0")}.txt`,
      relPath: `file-${String(i).padStart(5, "0")}.txt`,
      isDir: false,
      size: 10,
      isIgnored: false,
    }));
    useAppStore.setState({ expanded: { "/proj\t": true }, treeCache: { "/proj\t": entries } }, false);

    render(<FileTree workspace={ws} />);
    const rows = screen.getAllByTestId("file-row");
    expect(rows.length).toBeGreaterThan(0);
    expect(rows.length).toBeLessThan(500);
    expect(listDirMock).not.toHaveBeenCalled();
  });

  it("(d) clicking a file row selects it and opens the files rail", async () => {
    const entries: FsEntry[] = [
      { name: "a.txt", relPath: "a.txt", isDir: false, size: 3, isIgnored: false },
    ];
    listDirMock.mockResolvedValueOnce(entries);
    render(<FileTree workspace={ws} />);

    await act(async () => {
      fireEvent.click(screen.getByText("proj"));
    });
    await act(async () => {
      fireEvent.click(screen.getByText("a.txt"));
    });

    expect(useAppStore.getState().selectedFile).toEqual({ root: "/proj", rel: "a.txt" });
    expect(useAppStore.getState().filesRailOpen).toBe(true);
  });

  it("dirs sort before files, both alphabetically", async () => {
    const entries: FsEntry[] = [
      { name: "z.txt", relPath: "z.txt", isDir: false, size: 1, isIgnored: false },
      { name: "beta", relPath: "beta", isDir: true, size: 0, isIgnored: false },
      { name: "a.txt", relPath: "a.txt", isDir: false, size: 1, isIgnored: false },
      { name: "alpha", relPath: "alpha", isDir: true, size: 0, isIgnored: false },
    ];
    listDirMock.mockResolvedValueOnce(entries);
    render(<FileTree workspace={ws} />);
    await act(async () => {
      fireEvent.click(screen.getByText("proj"));
    });
    const rows = screen.getAllByTestId("file-row");
    const names = rows.map((r) => r.textContent);
    // root "proj" first, then dirs alpha/beta, then files a.txt/z.txt
    expect(names[0]).toContain("proj");
    const alphaIdx = names.findIndex((n) => n?.includes("alpha"));
    const betaIdx = names.findIndex((n) => n?.includes("beta"));
    const aTxtIdx = names.findIndex((n) => n?.includes("a.txt"));
    const zTxtIdx = names.findIndex((n) => n?.includes("z.txt"));
    expect(alphaIdx).toBeLessThan(betaIdx);
    expect(betaIdx).toBeLessThan(aTxtIdx);
    expect(aTxtIdx).toBeLessThan(zTxtIdx);
  });

  it("ignored entries are hidden by default and dimmed when showIgnored is on", async () => {
    const entries: FsEntry[] = [
      { name: "visible.txt", relPath: "visible.txt", isDir: false, size: 1, isIgnored: false },
      { name: "node_modules", relPath: "node_modules", isDir: true, size: 0, isIgnored: true },
    ];
    listDirMock.mockResolvedValueOnce(entries);
    render(<FileTree workspace={ws} />);
    await act(async () => {
      fireEvent.click(screen.getByText("proj"));
    });
    expect(screen.getByText("visible.txt")).toBeTruthy();
    expect(screen.queryByText("node_modules")).toBeNull();

    listDirMock.mockResolvedValueOnce(entries);
    await act(async () => {
      useAppStore.getState().toggleShowIgnored();
      // toggling clears caches so entries are refetched with includeIgnored=true (own component
      // concern is just to render what treeCache gives it — simulate the refetch directly here).
      const rest = { ...useAppStore.getState().treeCache };
      delete rest["/proj\t"];
      useAppStore.setState({ treeCache: rest }, false);
    });
    expect(await screen.findByText("node_modules")).toBeTruthy();
  });

  it("(e) context-menu Delete confirms then calls deleteEntry, and refreshes the parent dir", async () => {
    const entries: FsEntry[] = [
      { name: "a.txt", relPath: "a.txt", isDir: false, size: 3, isIgnored: false },
    ];
    listDirMock.mockResolvedValueOnce(entries);
    listDirMock.mockResolvedValueOnce([]); // post-delete refresh of the parent dir
    render(<FileTree workspace={ws} />);
    await act(async () => {
      fireEvent.click(screen.getByText("proj"));
    });

    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(true);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /Действия: a.txt/i }));
    });
    await act(async () => {
      fireEvent.click(screen.getByRole("menuitem", { name: /Удалить/i }));
    });

    expect(confirmSpy).toHaveBeenCalled();
    expect(deleteEntryMock).toHaveBeenCalledWith("/proj", "a.txt");
    expect(listDirMock).toHaveBeenCalledTimes(2);
    expect(listDirMock).toHaveBeenLastCalledWith("/proj", "", false);
    confirmSpy.mockRestore();
  });

  it("context-menu Delete is a no-op when the confirm is declined", async () => {
    const entries: FsEntry[] = [
      { name: "a.txt", relPath: "a.txt", isDir: false, size: 3, isIgnored: false },
    ];
    listDirMock.mockResolvedValueOnce(entries);
    render(<FileTree workspace={ws} />);
    await act(async () => {
      fireEvent.click(screen.getByText("proj"));
    });

    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(false);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /Действия: a.txt/i }));
    });
    await act(async () => {
      fireEvent.click(screen.getByRole("menuitem", { name: /Удалить/i }));
    });

    expect(deleteEntryMock).not.toHaveBeenCalled();
    confirmSpy.mockRestore();
  });

  it("context menu New File creates inside the target directory then refreshes it", async () => {
    const rootEntries: FsEntry[] = [
      { name: "sub", relPath: "sub", isDir: true, size: 0, isIgnored: false },
    ];
    listDirMock.mockResolvedValueOnce(rootEntries);
    listDirMock.mockResolvedValueOnce([]); // sub expand (empty)
    listDirMock.mockResolvedValueOnce([
      { name: "new.txt", relPath: "sub/new.txt", isDir: false, size: 0, isIgnored: false },
    ]); // refresh after create
    render(<FileTree workspace={ws} />);
    await act(async () => {
      fireEvent.click(screen.getByText("proj"));
    });
    await act(async () => {
      fireEvent.click(screen.getByText("sub"));
    });

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /Действия: sub/i }));
    });
    await act(async () => {
      fireEvent.click(screen.getByRole("menuitem", { name: /Новый файл/i }));
    });
    const input = screen.getByRole("textbox");
    fireEvent.change(input, { target: { value: "new.txt" } });
    await act(async () => {
      fireEvent.keyDown(input, { key: "Enter" });
    });

    expect(createFileMock).toHaveBeenCalledWith("/proj", "sub", "new.txt");
    expect(listDirMock).toHaveBeenCalledTimes(3);
    expect(await screen.findByText("new.txt")).toBeTruthy();
  });

  it("context menu Rename renames then refreshes the parent dir", async () => {
    const entries: FsEntry[] = [
      { name: "a.txt", relPath: "a.txt", isDir: false, size: 3, isIgnored: false },
    ];
    listDirMock.mockResolvedValueOnce(entries);
    listDirMock.mockResolvedValueOnce([
      { name: "b.txt", relPath: "b.txt", isDir: false, size: 3, isIgnored: false },
    ]);
    render(<FileTree workspace={ws} />);
    await act(async () => {
      fireEvent.click(screen.getByText("proj"));
    });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /Действия: a.txt/i }));
    });
    await act(async () => {
      fireEvent.click(screen.getByRole("menuitem", { name: /Переименовать/i }));
    });
    const input = screen.getByRole("textbox") as HTMLInputElement;
    expect(input.value).toBe("a.txt");
    fireEvent.change(input, { target: { value: "b.txt" } });
    await act(async () => {
      fireEvent.keyDown(input, { key: "Enter" });
    });

    expect(renameEntryMock).toHaveBeenCalledWith("/proj", "a.txt", "b.txt");
    expect(await screen.findByText("b.txt")).toBeTruthy();
  });

  it("root nodes offer New File/Folder but not Rename/Delete (workspace deletion is out of scope)", async () => {
    listDirMock.mockResolvedValueOnce([]);
    render(<FileTree workspace={ws} />);
    await act(async () => {
      fireEvent.click(screen.getByText("proj"));
    });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /Действия: proj/i }));
    });
    expect(screen.getByRole("menuitem", { name: /Новый файл/i })).toBeTruthy();
    expect(screen.getByRole("menuitem", { name: /Новая папка/i })).toBeTruthy();
    expect(screen.queryByRole("menuitem", { name: /Переименовать/i })).toBeNull();
    expect(screen.queryByRole("menuitem", { name: /Удалить/i })).toBeNull();
  });

  it("Reveal in Finder and Open External call their IPC wrappers", async () => {
    const entries: FsEntry[] = [
      { name: "a.txt", relPath: "a.txt", isDir: false, size: 3, isIgnored: false },
    ];
    listDirMock.mockResolvedValueOnce(entries);
    render(<FileTree workspace={ws} />);
    await act(async () => {
      fireEvent.click(screen.getByText("proj"));
    });
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /Действия: a.txt/i }));
    });
    await act(async () => {
      fireEvent.click(screen.getByRole("menuitem", { name: /Показать в Finder/i }));
    });
    expect(revealInFinderMock).toHaveBeenCalledWith("/proj", "a.txt");

    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /Действия: a.txt/i }));
    });
    await act(async () => {
      fireEvent.click(screen.getByRole("menuitem", { name: /внешн/i }));
    });
    expect(openExternalMock).toHaveBeenCalledWith("/proj", "a.txt");
  });

  it("(f) + Add root calls addWorkspaceRoot with the pickFolder result and upserts the workspace", async () => {
    pickFolderMock.mockResolvedValue("/other");
    addWorkspaceRootMock.mockResolvedValue({ ...ws, roots: ["/proj", "/other"] });
    render(<FileTree workspace={ws} />);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /add root/i }));
    });
    expect(pickFolderMock).toHaveBeenCalledTimes(1);
    expect(addWorkspaceRootMock).toHaveBeenCalledWith("w1", "/other");
    expect(useAppStore.getState().workspaces["w1"]?.roots).toEqual(["/proj", "/other"]);
  });

  it('"+ Add root" is a no-op when the folder picker is cancelled', async () => {
    pickFolderMock.mockResolvedValue(null);
    render(<FileTree workspace={ws} />);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /add root/i }));
    });
    expect(pickFolderMock).toHaveBeenCalledTimes(1);
    expect(addWorkspaceRootMock).not.toHaveBeenCalled();
    expect(useAppStore.getState().toast).toBeNull();
  });

  it("(f2) a rejected pickFolder() fires a toast instead of an unhandled rejection", async () => {
    pickFolderMock.mockRejectedValue({ kind: "internal", message: "picker unavailable" });
    render(<FileTree workspace={ws} />);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /add root/i }));
    });
    expect(pickFolderMock).toHaveBeenCalledTimes(1);
    expect(addWorkspaceRootMock).not.toHaveBeenCalled();
    expect(useAppStore.getState().toast).toMatch(/picker unavailable/);
  });

  it("(h) an FsError from listDir on expand fires a toast", async () => {
    listDirMock.mockRejectedValueOnce({ kind: "permissionDenied" });
    render(<FileTree workspace={ws} />);
    await act(async () => {
      fireEvent.click(screen.getByText("proj"));
    });
    expect(useAppStore.getState().toast).toBeTruthy();
    expect(useAppStore.getState().toast).toMatch(/доступ/i);
  });

  it("(h) an FsError from deleteEntry fires a toast (never a silent failure)", async () => {
    const entries: FsEntry[] = [
      { name: "a.txt", relPath: "a.txt", isDir: false, size: 3, isIgnored: false },
    ];
    listDirMock.mockResolvedValueOnce(entries);
    deleteEntryMock.mockRejectedValueOnce({ kind: "io", message: "disk full" });
    render(<FileTree workspace={ws} />);
    await act(async () => {
      fireEvent.click(screen.getByText("proj"));
    });
    const confirmSpy = vi.spyOn(window, "confirm").mockReturnValue(true);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /Действия: a.txt/i }));
    });
    await act(async () => {
      fireEvent.click(screen.getByRole("menuitem", { name: /Удалить/i }));
    });
    expect(useAppStore.getState().toast).toMatch(/disk full/);
    confirmSpy.mockRestore();
  });

  it("addWorkspaceRoot rejection fires a toast instead of failing silently", async () => {
    pickFolderMock.mockResolvedValue("/other");
    addWorkspaceRootMock.mockRejectedValueOnce({ kind: "daemon", code: "Duplicate", message: "already a root" });
    render(<FileTree workspace={ws} />);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /add root/i }));
    });
    expect(useAppStore.getState().toast).toMatch(/already a root/);
  });
});
