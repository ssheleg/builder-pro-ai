// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, act } from "@testing-library/react";

// ---- capture each subscriber callback so tests can fire daemon events ----
const cbs: Record<string, (p: unknown) => void> = {};
const unlisten = vi.fn();
vi.mock("./ipc/events", () => ({
  onSessionCreated: (cb: (p: unknown) => void) => {
    cbs.created = cb;
    return Promise.resolve(unlisten);
  },
  onSessionStateChanged: (cb: (p: unknown) => void) => {
    cbs.state = cb;
    return Promise.resolve(unlisten);
  },
  onSessionExited: (cb: (p: unknown) => void) => {
    cbs.exited = cb;
    return Promise.resolve(unlisten);
  },
  onWorkspaceCreated: (cb: (p: unknown) => void) => {
    cbs.wsCreated = cb;
    return Promise.resolve(unlisten);
  },
  onDaemonDisconnected: (cb: (p: unknown) => void) => {
    cbs.disc = cb;
    return Promise.resolve(unlisten);
  },
  onDaemonReconnected: (cb: (p: unknown) => void) => {
    cbs.recon = cb;
    return Promise.resolve(unlisten);
  },
}));

const listSessionsMock = vi.fn().mockResolvedValue([]);
const listWorkspacesMock = vi.fn().mockResolvedValue([]);
const createSessionMock = vi.fn().mockResolvedValue(undefined);
const killSessionMock = vi.fn().mockResolvedValue(undefined);
const createWorkspaceMock = vi.fn().mockResolvedValue(undefined);
const pickFolderMock = vi.fn().mockResolvedValue(null);
vi.mock("./ipc/commands", () => ({
  listSessions: (...a: unknown[]) => listSessionsMock(...a),
  listWorkspaces: (...a: unknown[]) => listWorkspacesMock(...a),
  createSession: (...a: unknown[]) => createSessionMock(...a),
  killSession: (...a: unknown[]) => killSessionMock(...a),
  createWorkspace: (...a: unknown[]) => createWorkspaceMock(...a),
  pickFolder: (...a: unknown[]) => pickFolderMock(...a),
}));

const disposeMock = vi.fn();
const openMock = vi.fn();
const hideMock = vi.fn();
const resetAllAttachmentsMock = vi.fn();
const resetAttachmentMock = vi.fn();
const fakeManager = {
  ensure: vi.fn(),
  has: vi.fn(() => true),
  get: vi.fn(),
  isOpened: vi.fn(() => true),
  isAttached: vi.fn(() => false),
  pendingBytes: vi.fn(() => 0),
  attach: vi.fn().mockResolvedValue(undefined),
  resetAttachment: resetAttachmentMock,
  resetAllAttachments: resetAllAttachmentsMock,
  applyReplay: vi.fn(),
  writeOutput: vi.fn(),
  open: openMock,
  hide: hideMock,
  dispose: disposeMock,
  disposeAll: vi.fn(),
} as unknown as import("./terminal/terminal-manager").TerminalManager;

import { App } from "./App";
import { useAppStore } from "./store/store";
import type { SessionMeta } from "./ipc/types";

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

afterEach(cleanup);
beforeEach(() => {
  for (const k of Object.keys(cbs)) delete cbs[k];
  unlisten.mockClear();
  disposeMock.mockReset();
  openMock.mockReset();
  hideMock.mockReset();
  resetAllAttachmentsMock.mockReset();
  resetAttachmentMock.mockReset();
  (fakeManager.attach as unknown as ReturnType<typeof vi.fn>)
    .mockReset()
    .mockResolvedValue(undefined);
  listSessionsMock.mockReset().mockResolvedValue([]);
  listWorkspacesMock.mockReset().mockResolvedValue([]);
  createSessionMock.mockClear();
  killSessionMock.mockClear();
  createWorkspaceMock.mockClear();
  pickFolderMock.mockClear();
  useAppStore.setState(
    { sessions: {}, workspaces: {}, activeSessionId: null, daemonConnected: false },
    false,
  );
});

describe("App", () => {
  it("registers all six IPC subscriptions on mount", async () => {
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    for (const key of ["created", "state", "exited", "wsCreated", "disc", "recon"]) {
      expect(typeof cbs[key]).toBe("function");
    }
  });

  it("hydrates on mount: listWorkspaces+listSessions succeed -> daemonConnected true", async () => {
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    expect(listWorkspacesMock).toHaveBeenCalled();
    expect(listSessionsMock).toHaveBeenCalled();
    expect(useAppStore.getState().daemonConnected).toBe(true);
  });

  it("hydrate retries on rejection then succeeds (bounded backoff)", async () => {
    vi.useFakeTimers();
    listWorkspacesMock.mockRejectedValueOnce(new Error("daemon not connected yet"));
    listWorkspacesMock.mockResolvedValue([]);
    render(<App manager={fakeManager} />);

    // First attempt happens synchronously on mount; let its microtasks flush.
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(useAppStore.getState().daemonConnected).toBe(false);
    expect(listWorkspacesMock).toHaveBeenCalledTimes(1);

    // Advance past the retry delay; the retry should succeed this time.
    await act(async () => {
      await vi.advanceTimersByTimeAsync(2000);
    });
    expect(listWorkspacesMock.mock.calls.length).toBeGreaterThanOrEqual(2);
    expect(useAppStore.getState().daemonConnected).toBe(true);
    vi.useRealTimers();
  });

  it("session://created upserts the session and activates the first one", async () => {
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    await act(async () => {
      cbs.created(meta());
    });
    expect(useAppStore.getState().sessions["s1"]).toBeTruthy();
    expect(useAppStore.getState().activeSessionId).toBe("s1");
  });

  it("session://state-changed updates lifecycle in the store", async () => {
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    await act(async () => {
      cbs.created(meta());
    });
    await act(async () => {
      cbs.state({ sessionId: "s1", lifecycle: { kind: "running" }, waitingForInput: false, cwd: "/tmp" });
    });
    expect(useAppStore.getState().sessions["s1"].lifecycle).toEqual({ kind: "running" });
  });

  it("session://exited marks the session inactive+exited (does not remove it, does not dispose the pane)", async () => {
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    await act(async () => {
      cbs.created(meta());
    });
    await act(async () => {
      cbs.exited({ sessionId: "s1", code: 0, signal: null });
    });
    const s = useAppStore.getState().sessions["s1"];
    expect(s.isActive).toBe(false);
    expect(s.lifecycle).toEqual({ kind: "exited", code: 0, signal: null });
    expect(disposeMock).not.toHaveBeenCalled();
  });

  it("daemon disconnect shows the banner; reconnect hides it and re-hydrates", async () => {
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    await act(async () => {
      cbs.disc(null);
    });
    expect(screen.getByRole("alert")).toBeTruthy();
    listWorkspacesMock.mockClear();
    listSessionsMock.mockClear();
    await act(async () => {
      cbs.recon(null);
    });
    expect(screen.queryByRole("alert")).toBeNull();
    expect(listWorkspacesMock).toHaveBeenCalled();
    expect(listSessionsMock).toHaveBeenCalled();
  });

  it("keep-alive: only the active session's pane is mounted; switching active hides (not disposes) the old pane", async () => {
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    await act(async () => {
      cbs.created(meta()); // s1 becomes active
    });
    await act(async () => {
      cbs.created(meta({ id: "s2", title: "bash" }));
    });
    // Only the active pane (s1) is mounted.
    expect(screen.getByTestId("terminal-pane-s1")).toBeTruthy();
    expect(screen.queryByTestId("terminal-pane-s2")).toBeNull();

    act(() => useAppStore.getState().setActiveSession("s2"));

    expect(screen.queryByTestId("terminal-pane-s1")).toBeNull();
    expect(screen.getByTestId("terminal-pane-s2")).toBeTruthy();
    // Unmounting s1's pane calls hide(), never dispose().
    expect(hideMock).toHaveBeenCalledWith("s1");
    expect(disposeMock).not.toHaveBeenCalled();
  });

  it("A1: switching to a second tab attaches that session's terminal (no dead pane)", async () => {
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    const attachMock = fakeManager.attach as unknown as ReturnType<typeof vi.fn>;
    await act(async () => {
      cbs.created(meta()); // s1 active -> pane mounts -> attach('s1')
    });
    expect(attachMock.mock.calls.filter((c) => c[0] === "s1").length).toBe(1);

    await act(async () => {
      cbs.created(meta({ id: "s2", title: "bash" })); // s2 exists but s1 stays active
    });
    // Switch the active tab to s2: the reused pane instance must attach s2.
    await act(async () => {
      useAppStore.getState().setActiveSession("s2");
    });
    // Pre-fix this is 0 (attachedRef latched on the first mount -> dead pane).
    expect(attachMock.mock.calls.filter((c) => c[0] === "s2").length).toBe(1);

    // Switching back to s1 re-mounts the pane; TerminalPane calls attach('s1')
    // unconditionally and the manager dedupes. With the fake manager there is no
    // real dedup, so we only assert the pane issues the call (dedup is proven in
    // terminal-manager.test.ts against the real manager).
    await act(async () => {
      useAppStore.getState().setActiveSession("s1");
    });
    expect(attachMock.mock.calls.filter((c) => c[0] === "s1").length).toBeGreaterThanOrEqual(1);
    // A tab switch must never dispose a keep-alive pane.
    expect(disposeMock).not.toHaveBeenCalled();
  });

  it("daemon reconnect resets ALL attach flags then re-attaches the visible session (spec §13)", async () => {
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    await act(async () => {
      cbs.created(meta()); // s1 becomes active; TerminalPane mounts + attach()'s once
    });
    const attachMock = fakeManager.attach as unknown as ReturnType<typeof vi.fn>;
    const callsBeforeReconnect = attachMock.mock.calls.filter((c) => c[0] === "s1").length;
    expect(callsBeforeReconnect).toBeGreaterThan(0);

    await act(async () => {
      cbs.disc(null);
    });
    await act(async () => {
      cbs.recon(null);
    });

    // New mechanism: reconnect clears EVERY session's attach flag (so hidden ones
    // re-attach lazily when next shown) BEFORE eagerly re-attaching the visible one.
    expect(resetAllAttachmentsMock).toHaveBeenCalledTimes(1);
    const callsAfterReconnect = attachMock.mock.calls.filter((c) => c[0] === "s1").length;
    expect(callsAfterReconnect).toBeGreaterThan(callsBeforeReconnect);
    // The active pane must NOT have been disposed/remounted to achieve this.
    expect(disposeMock).not.toHaveBeenCalled();
  });

  it("daemon reconnect: a hidden session lazily re-attaches when its tab is next shown (spec §13)", async () => {
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    await act(async () => {
      cbs.created(meta()); // s1 active (visible)
    });
    await act(async () => {
      cbs.created(meta({ id: "s2", title: "bash" })); // s2 hidden
    });
    // Show s2 once so its pane has mounted+attached at least once before reconnect.
    await act(async () => {
      useAppStore.getState().setActiveSession("s2");
    });
    await act(async () => {
      useAppStore.getState().setActiveSession("s1"); // back to s1 visible; s2 hidden
    });
    const attachMock = fakeManager.attach as unknown as ReturnType<typeof vi.fn>;

    await act(async () => {
      cbs.disc(null);
    });
    await act(async () => {
      cbs.recon(null); // resets all flags; eagerly re-attaches visible s1 only
    });
    expect(resetAllAttachmentsMock).toHaveBeenCalledTimes(1);
    const s2Before = attachMock.mock.calls.filter((c) => c[0] === "s2").length;

    // Now switch to the hidden session -> its pane re-mounts and re-attaches lazily.
    await act(async () => {
      useAppStore.getState().setActiveSession("s2");
    });
    const s2After = attachMock.mock.calls.filter((c) => c[0] === "s2").length;
    expect(s2After).toBeGreaterThan(s2Before);
    expect(disposeMock).not.toHaveBeenCalled();
  });

  it("hydrate restores an active workspace so + New terminal is enabled without a sidebar click", async () => {
    listWorkspacesMock.mockResolvedValue([{ id: "w1", name: "proj", rootPath: "/p" }]);
    listSessionsMock.mockResolvedValue([]);
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    const newTerminalBtn = screen.getByRole("button", { name: /new terminal/i });
    expect(newTerminalBtn).not.toHaveProperty("disabled", true);
    await act(async () => {
      newTerminalBtn.click();
    });
    expect(createSessionMock).toHaveBeenCalledWith("w1", { cols: 80, rows: 24 });
  });

  it("clicking a workspace in the sidebar sets it as the active workspace for new-terminal", async () => {
    useAppStore.setState({
      sessions: {},
      workspaces: { w1: { id: "w1", name: "proj", rootPath: "/p" } },
      activeSessionId: null,
      daemonConnected: true,
    });
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    await act(async () => {
      screen.getByText("proj").click();
    });
    await act(async () => {
      screen.getByRole("button", { name: /new terminal/i }).click();
    });
    expect(createSessionMock).toHaveBeenCalledWith("w1", { cols: 80, rows: 24 });
  });
});
