import { describe, it, expect, vi, beforeEach } from "vitest";

const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => {
  class Channel<T> {
    onmessage: ((m: T) => void) | undefined;
  }
  return { invoke: (...a: unknown[]) => invokeMock(...a), Channel };
});

import {
  createSession,
  listSessions,
  attachSession,
  detachSession,
  writeStdin,
  resize,
  killSession,
  listWorkspaces,
  createWorkspace,
  getSessionState,
  pickFolder,
  pickSkillFile,
  daemonStatus,
  addWorkspaceRoot,
  removeWorkspaceRoot,
  removeWorkspace,
  pathsExist,
  getCommandEvents,
} from "./commands";
import { Channel } from "@tauri-apps/api/core";
import type { SessionMeta, Workspace, TerminalEvent, CommandEvent } from "./types";

const sampleMeta: SessionMeta = {
  id: "s1",
  workspaceId: "w1",
  title: "zsh",
  shell: "/bin/zsh",
  cwd: "/tmp",
  cols: 80,
  rows: 24,
  lifecycle: { kind: "atPrompt" },
  waitingForInput: false,
  isActive: true,
  createdAt: 1000,
};

describe("ipc/commands", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue(undefined);
  });

  it("createSession sends workspaceId + opts, resolves SessionMeta", async () => {
    invokeMock.mockResolvedValueOnce(sampleMeta);
    const res = await createSession("w1", { shell: "/bin/bash", cols: 100, rows: 40 });
    expect(invokeMock).toHaveBeenCalledWith("create_session", {
      workspaceId: "w1",
      opts: { shell: "/bin/bash", cols: 100, rows: 40 },
    });
    expect(res).toEqual(sampleMeta);
  });

  it("createSession omits opts key when not provided", async () => {
    invokeMock.mockResolvedValueOnce(sampleMeta);
    await createSession("w1");
    expect(invokeMock).toHaveBeenCalledWith("create_session", { workspaceId: "w1", opts: undefined });
  });

  it("listSessions calls list_sessions with no args", async () => {
    const arr: SessionMeta[] = [sampleMeta];
    invokeMock.mockResolvedValueOnce(arr);
    const res = await listSessions();
    expect(invokeMock).toHaveBeenCalledWith("list_sessions");
    expect(res).toEqual(arr);
  });

  it("attachSession passes sessionId + Channel as onEvent", async () => {
    const ch = new Channel<TerminalEvent>();
    await attachSession("s1", ch);
    expect(invokeMock).toHaveBeenCalledWith("attach_session", { sessionId: "s1", onEvent: ch });
  });

  it("detachSession sends sessionId", async () => {
    await detachSession("s1");
    expect(invokeMock).toHaveBeenCalledWith("detach_session", { sessionId: "s1" });
  });

  it("writeStdin sends sessionId + data string", async () => {
    await writeStdin("s1", "ls\n");
    expect(invokeMock).toHaveBeenCalledWith("write_stdin", { sessionId: "s1", data: "ls\n" });
  });

  it("resize sends sessionId + cols + rows", async () => {
    await resize("s1", 120, 30);
    expect(invokeMock).toHaveBeenCalledWith("resize", { sessionId: "s1", cols: 120, rows: 30 });
  });

  it("killSession sends sessionId", async () => {
    await killSession("s1");
    expect(invokeMock).toHaveBeenCalledWith("kill_session", { sessionId: "s1" });
  });

  it("listWorkspaces calls list_workspaces", async () => {
    const ws: Workspace[] = [{ id: "w1", name: "proj", rootPath: "/p", roots: ["/p"] }];
    invokeMock.mockResolvedValueOnce(ws);
    const res = await listWorkspaces();
    expect(invokeMock).toHaveBeenCalledWith("list_workspaces");
    expect(res).toEqual(ws);
  });

  it("createWorkspace sends name + rootPath", async () => {
    const w: Workspace = { id: "w1", name: "proj", rootPath: "/p", roots: ["/p"] };
    invokeMock.mockResolvedValueOnce(w);
    const res = await createWorkspace("proj", "/p");
    expect(invokeMock).toHaveBeenCalledWith("create_workspace", { name: "proj", rootPath: "/p" });
    expect(res).toEqual(w);
  });

  it("getSessionState sends sessionId, resolves SessionMeta", async () => {
    invokeMock.mockResolvedValueOnce(sampleMeta);
    const res = await getSessionState("s1");
    expect(invokeMock).toHaveBeenCalledWith("get_session_state", { sessionId: "s1" });
    expect(res).toEqual(sampleMeta);
  });

  it("pickFolder calls pick_folder, resolves string|null", async () => {
    invokeMock.mockResolvedValueOnce("/chosen");
    expect(await pickFolder()).toBe("/chosen");
    invokeMock.mockResolvedValueOnce(null);
    expect(await pickFolder()).toBeNull();
  });

  it("pickSkillFile calls pick_skill_file, resolves string|null (S-EXT §8, D11, T17)", async () => {
    invokeMock.mockResolvedValueOnce("/Users/demo/skills/demo/SKILL.md");
    expect(await pickSkillFile()).toBe("/Users/demo/skills/demo/SKILL.md");
    expect(invokeMock).toHaveBeenCalledWith("pick_skill_file");
    invokeMock.mockResolvedValueOnce(null);
    expect(await pickSkillFile()).toBeNull();
  });

  it("daemonStatus calls daemon_status with no args, resolves the connected variant", async () => {
    invokeMock.mockResolvedValueOnce({ kind: "connected" });
    const res = await daemonStatus();
    expect(invokeMock).toHaveBeenCalledWith("daemon_status");
    expect(res).toEqual({ kind: "connected" });
  });

  it("daemonStatus resolves the disconnected variant", async () => {
    invokeMock.mockResolvedValueOnce({ kind: "disconnected" });
    expect(await daemonStatus()).toEqual({ kind: "disconnected" });
  });

  it("daemonStatus resolves the incompatible variant with daemonMin/daemonMax", async () => {
    invokeMock.mockResolvedValueOnce({ kind: "incompatible", daemonMin: 3, daemonMax: 4 });
    expect(await daemonStatus()).toEqual({ kind: "incompatible", daemonMin: 3, daemonMax: 4 });
  });

  it("addWorkspaceRoot sends workspaceId + path, resolves the updated Workspace", async () => {
    const w: Workspace = { id: "w1", name: "proj", rootPath: "/p", roots: ["/p", "/q"] };
    invokeMock.mockResolvedValueOnce(w);
    const res = await addWorkspaceRoot("w1", "/q");
    expect(invokeMock).toHaveBeenCalledWith("add_workspace_root", { workspaceId: "w1", path: "/q" });
    expect(res).toEqual(w);
  });

  it("removeWorkspaceRoot sends workspaceId + path, resolves the updated Workspace", async () => {
    const w: Workspace = { id: "w1", name: "proj", rootPath: "/p", roots: ["/p"] };
    invokeMock.mockResolvedValueOnce(w);
    const res = await removeWorkspaceRoot("w1", "/q");
    expect(invokeMock).toHaveBeenCalledWith("remove_workspace_root", {
      workspaceId: "w1",
      path: "/q",
    });
    expect(res).toEqual(w);
  });

  it("removeWorkspaceRoot propagates a rejected LastRoot CommandError as-is", async () => {
    const err = { kind: "daemon", code: "LastRoot", message: "cannot remove the last root" };
    invokeMock.mockRejectedValueOnce(err);
    await expect(removeWorkspaceRoot("w1", "/p")).rejects.toEqual(err);
  });

  // ── SCN-058 / SCN-059 ────────────────────────────────────────────────────────────────────────

  it("removeWorkspace sends only the workspaceId and resolves with nothing (daemon replies Ack)", async () => {
    invokeMock.mockResolvedValueOnce(undefined);
    await expect(removeWorkspace("w1")).resolves.toBeUndefined();
    expect(invokeMock).toHaveBeenCalledWith("remove_workspace", { workspaceId: "w1" });
  });

  it("removeWorkspace propagates a rejected CommandError as-is (unknown id ⇒ DbSql)", async () => {
    const err = { kind: "daemon", code: "DbSql", message: "db sql error: workspace w-gone not found" };
    invokeMock.mockRejectedValueOnce(err);
    await expect(removeWorkspace("w-gone")).rejects.toEqual(err);
  });

  it("pathsExist sends the path list and resolves one positional boolean per path", async () => {
    invokeMock.mockResolvedValueOnce([true, false]);
    const res = await pathsExist(["/p", "/gone"]);
    expect(invokeMock).toHaveBeenCalledWith("paths_exist", { paths: ["/p", "/gone"] });
    expect(res).toEqual([true, false]);
  });

  it("getCommandEvents sends sessionId + limit, resolves CommandEvent[] newest-first", async () => {
    const events: CommandEvent[] = [
      { sessionId: "s1", seq: 2, ts: 200, kind: "finished", exitCode: 0, origin: "shell" },
      { sessionId: "s1", seq: 1, ts: 100, kind: "started", exitCode: null, origin: "shell" },
    ];
    invokeMock.mockResolvedValueOnce(events);
    const res = await getCommandEvents("s1", 10);
    expect(invokeMock).toHaveBeenCalledWith("get_command_events", { sessionId: "s1", limit: 10 });
    expect(res).toEqual(events);
  });

  it("getCommandEvents resolves an empty array for an unknown session (honest, not an error)", async () => {
    invokeMock.mockResolvedValueOnce([]);
    expect(await getCommandEvents("ghost", 10)).toEqual([]);
  });
});
