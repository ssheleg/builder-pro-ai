import { describe, it, expect, beforeEach } from "vitest";
import { useAppStore } from "./store";
import type { SessionMeta, Workspace } from "../ipc/types";
import type { StateChangedPayload, ExitedPayload } from "../ipc/events";

const meta = (over: Partial<SessionMeta> = {}): SessionMeta => ({
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
  ...over,
});

const initial = useAppStore.getState();

describe("useAppStore", () => {
  beforeEach(() => {
    useAppStore.setState(
      {
        sessions: {},
        workspaces: {},
        activeSessionId: null,
        daemonConnected: false,
      },
      false,
    );
  });

  it("has the spec §12 initial shape", () => {
    const s = useAppStore.getState();
    expect(s.sessions).toEqual({});
    expect(s.workspaces).toEqual({});
    expect(s.activeSessionId).toBeNull();
    expect(s.daemonConnected).toBe(false);
    expect(typeof initial.upsertSession).toBe("function");
  });

  it("upsertSession adds then replaces by id", () => {
    useAppStore.getState().upsertSession(meta());
    expect(useAppStore.getState().sessions["s1"].title).toBe("zsh");
    useAppStore.getState().upsertSession(meta({ title: "bash" }));
    expect(Object.keys(useAppStore.getState().sessions)).toHaveLength(1);
    expect(useAppStore.getState().sessions["s1"].title).toBe("bash");
  });

  it("upsertSession is idempotent: repeated upsert of an identical meta yields one entry with identical fields", () => {
    const m = meta();
    useAppStore.getState().upsertSession(m);
    useAppStore.getState().upsertSession(m);
    useAppStore.getState().upsertSession(m);
    const sessions = useAppStore.getState().sessions;
    expect(Object.keys(sessions)).toHaveLength(1);
    expect(sessions["s1"]).toEqual(m);
  });

  it("removeSession deletes and clears activeSessionId if it matched", () => {
    useAppStore.getState().upsertSession(meta());
    useAppStore.getState().setActiveSession("s1");
    useAppStore.getState().removeSession("s1");
    expect(useAppStore.getState().sessions["s1"]).toBeUndefined();
    expect(useAppStore.getState().activeSessionId).toBeNull();
  });

  it("removeSession keeps a non-matching activeSessionId", () => {
    useAppStore.getState().upsertSession(meta());
    useAppStore.getState().upsertSession(meta({ id: "s2" }));
    useAppStore.getState().setActiveSession("s2");
    useAppStore.getState().removeSession("s1");
    expect(useAppStore.getState().activeSessionId).toBe("s2");
  });

  it("removeSession on an unknown id is a no-op", () => {
    useAppStore.getState().upsertSession(meta());
    useAppStore.getState().removeSession("ghost");
    expect(Object.keys(useAppStore.getState().sessions)).toHaveLength(1);
  });

  it("setLifecycle updates lifecycle/waitingForInput/cwd of an existing session", () => {
    useAppStore.getState().upsertSession(meta());
    const p: StateChangedPayload = {
      sessionId: "s1",
      lifecycle: { kind: "running" },
      waitingForInput: true,
      cwd: "/work",
    };
    useAppStore.getState().setLifecycle(p);
    const s = useAppStore.getState().sessions["s1"];
    expect(s.lifecycle).toEqual({ kind: "running" });
    expect(s.waitingForInput).toBe(true);
    expect(s.cwd).toBe("/work");
    // untouched fields preserved
    expect(s.title).toBe("zsh");
    expect(s.cols).toBe(80);
  });

  it("setLifecycle is a no-op for an unknown session id", () => {
    const p: StateChangedPayload = {
      sessionId: "ghost",
      lifecycle: { kind: "running" },
      waitingForInput: false,
      cwd: "/x",
    };
    useAppStore.getState().setLifecycle(p);
    expect(useAppStore.getState().sessions["ghost"]).toBeUndefined();
  });

  it("markExited sets isActive:false and an exited lifecycle for an existing session", () => {
    useAppStore.getState().upsertSession(meta());
    const p: ExitedPayload = { sessionId: "s1", code: 0, signal: null };
    useAppStore.getState().markExited(p);
    const s = useAppStore.getState().sessions["s1"];
    expect(s.isActive).toBe(false);
    expect(s.lifecycle).toEqual({ kind: "exited", code: 0, signal: null });
    // untouched fields preserved
    expect(s.title).toBe("zsh");
    expect(s.cwd).toBe("/tmp");
  });

  it("markExited handles a null code/signal (killed by signal, or unknown)", () => {
    useAppStore.getState().upsertSession(meta());
    const p: ExitedPayload = { sessionId: "s1", code: null, signal: "SIGKILL" };
    useAppStore.getState().markExited(p);
    const s = useAppStore.getState().sessions["s1"];
    expect(s.lifecycle).toEqual({ kind: "exited", code: null, signal: "SIGKILL" });
  });

  it("markExited is a no-op for an unknown session id", () => {
    const p: ExitedPayload = { sessionId: "ghost", code: 0, signal: null };
    useAppStore.getState().markExited(p);
    expect(useAppStore.getState().sessions["ghost"]).toBeUndefined();
  });

  it("setDaemonConnected toggles the flag", () => {
    useAppStore.getState().setDaemonConnected(true);
    expect(useAppStore.getState().daemonConnected).toBe(true);
    useAppStore.getState().setDaemonConnected(false);
    expect(useAppStore.getState().daemonConnected).toBe(false);
  });

  it("upsertWorkspace adds then replaces by id", () => {
    const w: Workspace = { id: "w1", name: "proj", rootPath: "/p" };
    useAppStore.getState().upsertWorkspace(w);
    expect(useAppStore.getState().workspaces["w1"].name).toBe("proj");
    useAppStore.getState().upsertWorkspace({ ...w, name: "renamed" });
    expect(useAppStore.getState().workspaces["w1"].name).toBe("renamed");
    expect(Object.keys(useAppStore.getState().workspaces)).toHaveLength(1);
  });

  it("setActiveSession sets and clears the active session id", () => {
    useAppStore.getState().upsertSession(meta());
    useAppStore.getState().setActiveSession("s1");
    expect(useAppStore.getState().activeSessionId).toBe("s1");
    useAppStore.getState().setActiveSession(null);
    expect(useAppStore.getState().activeSessionId).toBeNull();
  });

  it("never stores raw bytes: session values are exactly SessionMeta keys", () => {
    useAppStore.getState().upsertSession(meta());
    const keys = Object.keys(useAppStore.getState().sessions["s1"]).sort();
    expect(keys).toEqual(
      [
        "cols",
        "cwd",
        "createdAt",
        "id",
        "isActive",
        "lifecycle",
        "rows",
        "shell",
        "title",
        "waitingForInput",
        "workspaceId",
      ].sort(),
    );
  });
});
