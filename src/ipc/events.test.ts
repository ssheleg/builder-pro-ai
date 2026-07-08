import { describe, it, expect, vi, beforeEach } from "vitest";

type Listener = (e: { payload: unknown }) => void;
const registered = new Map<string, Listener>();
const unlisten = vi.fn();
const listenMock = vi.fn(async (event: string, handler: Listener) => {
  registered.set(event, handler);
  return unlisten;
});

vi.mock("@tauri-apps/api/event", () => ({
  listen: (event: string, handler: Listener) => listenMock(event, handler),
}));

import {
  onSessionCreated,
  onSessionStateChanged,
  onSessionExited,
  onWorkspaceCreated,
  onDaemonDisconnected,
  onDaemonReconnected,
  onFsChanged,
  onFsWatchError,
  onWorkspaceUpdated,
} from "./events";
import type { SessionMeta, Workspace } from "./types";
import type {
  StateChangedPayload,
  ExitedPayload,
  FsChangedPayload,
  FsWatchErrorPayload,
} from "./events";

describe("ipc/events", () => {
  beforeEach(() => {
    registered.clear();
    listenMock.mockClear();
    unlisten.mockClear();
  });

  it("onSessionCreated subscribes to session://created and unwraps payload", async () => {
    const cb = vi.fn();
    const un = await onSessionCreated(cb);
    expect(listenMock).toHaveBeenCalledWith("session://created", expect.any(Function));
    const meta: SessionMeta = {
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
      createdAt: 1,
    };
    registered.get("session://created")!({ payload: meta });
    expect(cb).toHaveBeenCalledWith(meta);
    expect(un).toBe(unlisten);
  });

  it("onSessionStateChanged subscribes to session://state-changed", async () => {
    const cb = vi.fn();
    await onSessionStateChanged(cb);
    expect(listenMock).toHaveBeenCalledWith("session://state-changed", expect.any(Function));
    const p: StateChangedPayload = {
      sessionId: "s1",
      lifecycle: { kind: "running" },
      waitingForInput: false,
      cwd: "/tmp",
    };
    registered.get("session://state-changed")!({ payload: p });
    expect(cb).toHaveBeenCalledWith(p);
  });

  it("onSessionExited subscribes to session://exited", async () => {
    const cb = vi.fn();
    await onSessionExited(cb);
    expect(listenMock).toHaveBeenCalledWith("session://exited", expect.any(Function));
    const p: ExitedPayload = { sessionId: "s1", code: 0, signal: null };
    registered.get("session://exited")!({ payload: p });
    expect(cb).toHaveBeenCalledWith(p);
  });

  it("onWorkspaceCreated subscribes to workspace://created", async () => {
    const cb = vi.fn();
    await onWorkspaceCreated(cb);
    expect(listenMock).toHaveBeenCalledWith("workspace://created", expect.any(Function));
    const w: Workspace = { id: "w1", name: "p", rootPath: "/p", roots: ["/p"] };
    registered.get("workspace://created")!({ payload: w });
    expect(cb).toHaveBeenCalledWith(w);
  });

  it("onDaemonDisconnected subscribes to daemon://disconnected and calls cb (no payload)", async () => {
    const cb = vi.fn();
    await onDaemonDisconnected(cb);
    expect(listenMock).toHaveBeenCalledWith("daemon://disconnected", expect.any(Function));
    registered.get("daemon://disconnected")!({ payload: null });
    expect(cb).toHaveBeenCalledTimes(1);
  });

  it("onDaemonReconnected subscribes to daemon://reconnected and calls cb", async () => {
    const cb = vi.fn();
    await onDaemonReconnected(cb);
    expect(listenMock).toHaveBeenCalledWith("daemon://reconnected", expect.any(Function));
    registered.get("daemon://reconnected")!({ payload: null });
    expect(cb).toHaveBeenCalledTimes(1);
  });

  it("onFsChanged subscribes to fs://changed and unwraps {root, changedRelPaths}", async () => {
    const cb = vi.fn();
    const un = await onFsChanged(cb);
    expect(listenMock).toHaveBeenCalledWith("fs://changed", expect.any(Function));
    const p: FsChangedPayload = { root: "/root", changedRelPaths: ["a.txt", "sub/b.txt"] };
    registered.get("fs://changed")!({ payload: p });
    expect(cb).toHaveBeenCalledWith(p);
    expect(un).toBe(unlisten);
  });

  it("onFsChanged carries the [\"*\"] refresh-everything sentinel through untouched", async () => {
    const cb = vi.fn();
    await onFsChanged(cb);
    const p: FsChangedPayload = { root: "/root", changedRelPaths: ["*"] };
    registered.get("fs://changed")!({ payload: p });
    expect(cb).toHaveBeenCalledWith(p);
  });

  it("onFsWatchError subscribes to fs://watch-error and unwraps {root, reason}", async () => {
    const cb = vi.fn();
    await onFsWatchError(cb);
    expect(listenMock).toHaveBeenCalledWith("fs://watch-error", expect.any(Function));
    const p: FsWatchErrorPayload = { root: "/root", reason: "backend died" };
    registered.get("fs://watch-error")!({ payload: p });
    expect(cb).toHaveBeenCalledWith(p);
  });

  it("onWorkspaceUpdated subscribes to workspace://updated and unwraps the raw Workspace", async () => {
    const cb = vi.fn();
    await onWorkspaceUpdated(cb);
    expect(listenMock).toHaveBeenCalledWith("workspace://updated", expect.any(Function));
    const w: Workspace = { id: "w1", name: "p", rootPath: "/p", roots: ["/p", "/q"] };
    registered.get("workspace://updated")!({ payload: w });
    expect(cb).toHaveBeenCalledWith(w);
  });
});
