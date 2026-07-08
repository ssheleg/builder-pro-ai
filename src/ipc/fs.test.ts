import { describe, it, expect, vi, beforeEach } from "vitest";

const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...a: unknown[]) => invokeMock(...a),
}));

import {
  listDir,
  readFilePreview,
  createFile,
  createDir,
  renameEntry,
  moveEntry,
  deleteEntry,
  revealInFinder,
  openExternal,
  startWorkspaceWatch,
  stopWorkspaceWatch,
} from "./fs";
import type { FsEntry, FilePreview, FsError } from "./fs";

describe("ipc/fs", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue(undefined);
  });

  it("listDir sends root/rel/includeIgnored, resolves FsEntry[]", async () => {
    const entries: FsEntry[] = [
      { name: "a.txt", relPath: "a.txt", isDir: false, size: 3, isIgnored: false },
    ];
    invokeMock.mockResolvedValueOnce(entries);
    const res = await listDir("/root", "", true);
    expect(invokeMock).toHaveBeenCalledWith("list_dir", {
      root: "/root",
      rel: "",
      includeIgnored: true,
    });
    expect(res).toEqual(entries);
  });

  it("readFilePreview sends root/rel, resolves a text FilePreview", async () => {
    const preview: FilePreview = { kind: "text", content: "hi", truncated: false, size: 2 };
    invokeMock.mockResolvedValueOnce(preview);
    const res = await readFilePreview("/root", "a.txt");
    expect(invokeMock).toHaveBeenCalledWith("read_file_preview", { root: "/root", rel: "a.txt" });
    expect(res).toEqual(preview);
  });

  it("readFilePreview resolves a binary FilePreview", async () => {
    const preview: FilePreview = { kind: "binary", size: 4 };
    invokeMock.mockResolvedValueOnce(preview);
    expect(await readFilePreview("/root", "bin.dat")).toEqual(preview);
  });

  it("readFilePreview resolves a tooLarge FilePreview", async () => {
    const preview: FilePreview = { kind: "tooLarge", size: 99999 };
    invokeMock.mockResolvedValueOnce(preview);
    expect(await readFilePreview("/root", "big.bin")).toEqual(preview);
  });

  it("readFilePreview propagates a rejected FsError as-is", async () => {
    const err: FsError = { kind: "outsideRoot" };
    invokeMock.mockRejectedValueOnce(err);
    await expect(readFilePreview("/root", "../secret")).rejects.toEqual(err);
  });

  it("readFilePreview propagates an Io FsError with message", async () => {
    const err: FsError = { kind: "io", message: "boom" };
    invokeMock.mockRejectedValueOnce(err);
    await expect(readFilePreview("/root", "x")).rejects.toEqual(err);
  });

  it("createFile sends root/relDir/name", async () => {
    await createFile("/root", "sub", "new.txt");
    expect(invokeMock).toHaveBeenCalledWith("create_file", {
      root: "/root",
      relDir: "sub",
      name: "new.txt",
    });
  });

  it("createDir sends root/relDir/name", async () => {
    await createDir("/root", "sub", "newdir");
    expect(invokeMock).toHaveBeenCalledWith("create_dir", {
      root: "/root",
      relDir: "sub",
      name: "newdir",
    });
  });

  it("renameEntry sends root/rel/newName", async () => {
    await renameEntry("/root", "old.txt", "renamed.txt");
    expect(invokeMock).toHaveBeenCalledWith("rename_entry", {
      root: "/root",
      rel: "old.txt",
      newName: "renamed.txt",
    });
  });

  it("moveEntry sends root/relFrom/relDirTo", async () => {
    await moveEntry("/root", "a.txt", "dest");
    expect(invokeMock).toHaveBeenCalledWith("move_entry", {
      root: "/root",
      relFrom: "a.txt",
      relDirTo: "dest",
    });
  });

  it("deleteEntry sends root/rel", async () => {
    await deleteEntry("/root", "a.txt");
    expect(invokeMock).toHaveBeenCalledWith("delete_entry", { root: "/root", rel: "a.txt" });
  });

  it("revealInFinder sends root/rel", async () => {
    await revealInFinder("/root", "a.txt");
    expect(invokeMock).toHaveBeenCalledWith("reveal_in_finder", { root: "/root", rel: "a.txt" });
  });

  it("openExternal sends root/rel", async () => {
    await openExternal("/root", "a.txt");
    expect(invokeMock).toHaveBeenCalledWith("open_external", { root: "/root", rel: "a.txt" });
  });

  it("startWorkspaceWatch sends roots[]/showIgnored", async () => {
    await startWorkspaceWatch(["/root-a", "/root-b"], true);
    expect(invokeMock).toHaveBeenCalledWith("start_workspace_watch", {
      roots: ["/root-a", "/root-b"],
      showIgnored: true,
    });
  });

  it("stopWorkspaceWatch calls stop_workspace_watch with no args", async () => {
    await stopWorkspaceWatch();
    expect(invokeMock).toHaveBeenCalledWith("stop_workspace_watch");
  });
});
