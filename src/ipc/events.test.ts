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
  onOrchdProjectsChanged,
  onOrchdGoalsChanged,
  onOrchdIdeasChanged,
  onOrchdInsightsChanged,
  onOrchdTasksChanged,
  onOrchdRulesetChanged,
  onOrchdGraphChanged,
  onOrchdDown,
  onOrchdUp,
  onOrchdIncompatible,
  onOrchdMcpServersChanged,
  onOrchdMcpToolsChanged,
  onOrchdMcpArtifactsChanged,
  onOrchdMcpInvocationLogged,
  onOrchdConnectorsChanged,
} from "./events";
import type { SessionMeta, Workspace } from "./types";
import type {
  StateChangedPayload,
  ExitedPayload,
  FsChangedPayload,
  FsWatchErrorPayload,
  GoalsChangedPayload,
  TasksChangedPayload,
  RulesetChangedPayload,
  GraphChangedPayload,
  McpServersChangedPayload,
  McpToolsChangedPayload,
  McpArtifactsChangedPayload,
  McpInvocationLoggedPayload,
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

  // ── orchd coarse-invalidation + connection events (S3 T13) ─────────────────────────────────

  it("onOrchdProjectsChanged subscribes to orchd://projects-changed and calls cb (no payload)", async () => {
    const cb = vi.fn();
    const un = await onOrchdProjectsChanged(cb);
    expect(listenMock).toHaveBeenCalledWith("orchd://projects-changed", expect.any(Function));
    registered.get("orchd://projects-changed")!({ payload: null });
    expect(cb).toHaveBeenCalledTimes(1);
    expect(un).toBe(unlisten);
  });

  it("onOrchdGoalsChanged subscribes to orchd://goals-changed and unwraps {projectId}", async () => {
    const cb = vi.fn();
    await onOrchdGoalsChanged(cb);
    expect(listenMock).toHaveBeenCalledWith("orchd://goals-changed", expect.any(Function));
    const p: GoalsChangedPayload = { projectId: "p1" };
    registered.get("orchd://goals-changed")!({ payload: p });
    expect(cb).toHaveBeenCalledWith(p);
  });

  it("onOrchdIdeasChanged subscribes to orchd://ideas-changed and calls cb (no payload)", async () => {
    const cb = vi.fn();
    await onOrchdIdeasChanged(cb);
    expect(listenMock).toHaveBeenCalledWith("orchd://ideas-changed", expect.any(Function));
    registered.get("orchd://ideas-changed")!({ payload: null });
    expect(cb).toHaveBeenCalledTimes(1);
  });

  it("onOrchdInsightsChanged subscribes to orchd://insights-changed and calls cb (no payload)", async () => {
    const cb = vi.fn();
    await onOrchdInsightsChanged(cb);
    expect(listenMock).toHaveBeenCalledWith("orchd://insights-changed", expect.any(Function));
    registered.get("orchd://insights-changed")!({ payload: null });
    expect(cb).toHaveBeenCalledTimes(1);
  });

  it("onOrchdTasksChanged subscribes to orchd://tasks-changed and unwraps {projectId}", async () => {
    const cb = vi.fn();
    await onOrchdTasksChanged(cb);
    expect(listenMock).toHaveBeenCalledWith("orchd://tasks-changed", expect.any(Function));
    const p: TasksChangedPayload = { projectId: "p1" };
    registered.get("orchd://tasks-changed")!({ payload: p });
    expect(cb).toHaveBeenCalledWith(p);
  });

  it("onOrchdRulesetChanged subscribes to orchd://ruleset-changed and unwraps {scope, projectId}", async () => {
    const cb = vi.fn();
    await onOrchdRulesetChanged(cb);
    expect(listenMock).toHaveBeenCalledWith("orchd://ruleset-changed", expect.any(Function));
    const p: RulesetChangedPayload = { scope: "project", projectId: "p1" };
    registered.get("orchd://ruleset-changed")!({ payload: p });
    expect(cb).toHaveBeenCalledWith(p);
  });

  it("onOrchdRulesetChanged carries a null projectId through for the global scope", async () => {
    const cb = vi.fn();
    await onOrchdRulesetChanged(cb);
    const p: RulesetChangedPayload = { scope: "global", projectId: null };
    registered.get("orchd://ruleset-changed")!({ payload: p });
    expect(cb).toHaveBeenCalledWith(p);
  });

  it("onOrchdGraphChanged subscribes to orchd://graph-changed and unwraps {projectId}", async () => {
    const cb = vi.fn();
    await onOrchdGraphChanged(cb);
    expect(listenMock).toHaveBeenCalledWith("orchd://graph-changed", expect.any(Function));
    const p: GraphChangedPayload = { projectId: "p1" };
    registered.get("orchd://graph-changed")!({ payload: p });
    expect(cb).toHaveBeenCalledWith(p);
  });

  it("onOrchdDown subscribes to orchd://down and calls cb (no payload)", async () => {
    const cb = vi.fn();
    await onOrchdDown(cb);
    expect(listenMock).toHaveBeenCalledWith("orchd://down", expect.any(Function));
    registered.get("orchd://down")!({ payload: null });
    expect(cb).toHaveBeenCalledTimes(1);
  });

  it("onOrchdUp subscribes to orchd://up and calls cb (no payload)", async () => {
    const cb = vi.fn();
    await onOrchdUp(cb);
    expect(listenMock).toHaveBeenCalledWith("orchd://up", expect.any(Function));
    registered.get("orchd://up")!({ payload: null });
    expect(cb).toHaveBeenCalledTimes(1);
  });

  it("onOrchdIncompatible subscribes to orchd://incompatible and calls cb (no payload)", async () => {
    const cb = vi.fn();
    await onOrchdIncompatible(cb);
    expect(listenMock).toHaveBeenCalledWith("orchd://incompatible", expect.any(Function));
    registered.get("orchd://incompatible")!({ payload: null });
    expect(cb).toHaveBeenCalledTimes(1);
  });

  // ── MCP coarse-invalidation events (S-EXT §8, T8) ──────────────────────────────────────────

  it("onOrchdMcpServersChanged subscribes to orchd://mcp-servers-changed and unwraps {projectId}", async () => {
    const cb = vi.fn();
    const un = await onOrchdMcpServersChanged(cb);
    expect(listenMock).toHaveBeenCalledWith("orchd://mcp-servers-changed", expect.any(Function));
    const p: McpServersChangedPayload = { projectId: "p1" };
    registered.get("orchd://mcp-servers-changed")!({ payload: p });
    expect(cb).toHaveBeenCalledWith(p);
    expect(un).toBe(unlisten);
  });

  it("onOrchdMcpServersChanged carries a null projectId through for the global scope", async () => {
    const cb = vi.fn();
    await onOrchdMcpServersChanged(cb);
    const p: McpServersChangedPayload = { projectId: null };
    registered.get("orchd://mcp-servers-changed")!({ payload: p });
    expect(cb).toHaveBeenCalledWith(p);
  });

  it("onOrchdMcpToolsChanged subscribes to orchd://mcp-tools-changed and unwraps {serverId}", async () => {
    const cb = vi.fn();
    await onOrchdMcpToolsChanged(cb);
    expect(listenMock).toHaveBeenCalledWith("orchd://mcp-tools-changed", expect.any(Function));
    const p: McpToolsChangedPayload = { serverId: "s1" };
    registered.get("orchd://mcp-tools-changed")!({ payload: p });
    expect(cb).toHaveBeenCalledWith(p);
  });

  it("onOrchdMcpArtifactsChanged subscribes to orchd://mcp-artifacts-changed and unwraps {projectId}", async () => {
    const cb = vi.fn();
    await onOrchdMcpArtifactsChanged(cb);
    expect(listenMock).toHaveBeenCalledWith("orchd://mcp-artifacts-changed", expect.any(Function));
    const p: McpArtifactsChangedPayload = { projectId: null };
    registered.get("orchd://mcp-artifacts-changed")!({ payload: p });
    expect(cb).toHaveBeenCalledWith(p);
  });

  it("onOrchdMcpInvocationLogged subscribes to orchd://mcp-invocation-logged and unwraps {serverId}", async () => {
    const cb = vi.fn();
    await onOrchdMcpInvocationLogged(cb);
    expect(listenMock).toHaveBeenCalledWith("orchd://mcp-invocation-logged", expect.any(Function));
    const p: McpInvocationLoggedPayload = { serverId: "s1" };
    registered.get("orchd://mcp-invocation-logged")!({ payload: p });
    expect(cb).toHaveBeenCalledWith(p);
  });

  // ── Connectors coarse-invalidation event (S-EXT §8, T13b) ──────────────────────────────────

  it("onOrchdConnectorsChanged subscribes to orchd://connectors-changed and calls cb (no payload)", async () => {
    const cb = vi.fn();
    const un = await onOrchdConnectorsChanged(cb);
    expect(listenMock).toHaveBeenCalledWith("orchd://connectors-changed", expect.any(Function));
    registered.get("orchd://connectors-changed")!({ payload: null });
    expect(cb).toHaveBeenCalledTimes(1);
    expect(un).toBe(unlisten);
  });
});
