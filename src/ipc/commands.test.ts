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
  daemonStatus,
} from "./commands";
import { Channel } from "@tauri-apps/api/core";
import type { SessionMeta, Workspace, TerminalEvent } from "./types";

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
    const ws: Workspace[] = [{ id: "w1", name: "proj", rootPath: "/p" }];
    invokeMock.mockResolvedValueOnce(ws);
    const res = await listWorkspaces();
    expect(invokeMock).toHaveBeenCalledWith("list_workspaces");
    expect(res).toEqual(ws);
  });

  it("createWorkspace sends name + rootPath", async () => {
    const w: Workspace = { id: "w1", name: "proj", rootPath: "/p" };
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
});
