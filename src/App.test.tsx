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
  onDaemonIncompatible: (cb: (p: unknown) => void) => {
    cbs.incompatible = cb;
    return Promise.resolve(unlisten);
  },
  onWorkspaceUpdated: (cb: (p: unknown) => void) => {
    cbs.wsUpdated = cb;
    return Promise.resolve(unlisten);
  },
  onFsChanged: (cb: (p: unknown) => void) => {
    cbs.fsChanged = cb;
    return Promise.resolve(unlisten);
  },
  onFsWatchError: (cb: (p: unknown) => void) => {
    cbs.fsWatchError = cb;
    return Promise.resolve(unlisten);
  },
}));

const listSessionsMock = vi.fn().mockResolvedValue([]);
const listWorkspacesMock = vi.fn().mockResolvedValue([]);
const createSessionMock = vi.fn().mockResolvedValue(undefined);
const killSessionMock = vi.fn().mockResolvedValue(undefined);
const createWorkspaceMock = vi.fn().mockResolvedValue(undefined);
const pickFolderMock = vi.fn().mockResolvedValue(null);
const daemonStatusMock = vi.fn().mockResolvedValue({ kind: "disconnected" });
const getCommandEventsMock = vi.fn().mockResolvedValue([]);
vi.mock("./ipc/commands", () => ({
  listSessions: (...a: unknown[]) => listSessionsMock(...a),
  listWorkspaces: (...a: unknown[]) => listWorkspacesMock(...a),
  createSession: (...a: unknown[]) => createSessionMock(...a),
  killSession: (...a: unknown[]) => killSessionMock(...a),
  createWorkspace: (...a: unknown[]) => createWorkspaceMock(...a),
  pickFolder: (...a: unknown[]) => pickFolderMock(...a),
  daemonStatus: (...a: unknown[]) => daemonStatusMock(...a),
  getCommandEvents: (...a: unknown[]) => getCommandEventsMock(...a),
}));

// Only override the two watch-lifecycle functions App wires up directly — every OTHER export
// (`listDir`, `readFilePreview`, ...) stays the REAL implementation, since FileTree/FilePreview
// (rendered transitively via FilesRail once a workspace is active) import from this same module
// and must keep working exactly as they do in their own, unmocked test suites (a real `invoke()`
// call rejects harmlessly in jsdom — no Tauri runtime — which those components already treat as
// an honest error, spec §7; nothing here needs to change that).
const startWorkspaceWatchMock = vi.fn().mockResolvedValue(undefined);
const stopWorkspaceWatchMock = vi.fn().mockResolvedValue(undefined);
vi.mock("./ipc/fs", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./ipc/fs")>();
  return {
    ...actual,
    startWorkspaceWatch: (...a: unknown[]) => startWorkspaceWatchMock(...a),
    stopWorkspaceWatch: (...a: unknown[]) => stopWorkspaceWatchMock(...a),
  };
});

const disposeMock = vi.fn();
const openMock = vi.fn();
const hideMock = vi.fn();
const focusMock = vi.fn();
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
  focus: focusMock,
  dispose: disposeMock,
  disposeAll: vi.fn(),
} as unknown as import("./terminal/terminal-manager").TerminalManager;

import { App, changedPathsToParentDirs } from "./App";
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
  focusMock.mockReset();
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
  daemonStatusMock.mockReset().mockResolvedValue({ kind: "disconnected" });
  getCommandEventsMock.mockReset().mockResolvedValue([]);
  startWorkspaceWatchMock.mockReset().mockResolvedValue(undefined);
  stopWorkspaceWatchMock.mockReset().mockResolvedValue(undefined);
  useAppStore.setState(
    {
      sessions: {},
      workspaces: {},
      activeSessionId: null,
      daemonConnected: false,
      daemonIncompatible: false,
      upgradeDialogOpen: false,
      upgradeError: null,
      hydrated: false,
      // Most tests below exercise session/terminal behavior that predates the Home screen (T11)
      // and assume the workspace layout (TerminalTabs/TerminalPane) is on screen; Home-specific
      // behavior gets its own `describe` block below that sets `view: "home"` explicitly.
      view: "workspace",
      treeCache: {},
      watchPaused: false,
      showIgnored: false,
    },
    false,
  );
});

describe("App", () => {
  it("registers all ten IPC subscriptions on mount", async () => {
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    for (const key of [
      "created",
      "state",
      "exited",
      "wsCreated",
      "wsUpdated",
      "fsChanged",
      "fsWatchError",
      "disc",
      "recon",
      "incompatible",
    ]) {
      expect(typeof cbs[key]).toBe("function");
    }
  });

  it("hydrates on mount: listWorkspaces+listSessions succeed -> daemonConnected true, hydrated true", async () => {
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    expect(listWorkspacesMock).toHaveBeenCalled();
    expect(listSessionsMock).toHaveBeenCalled();
    expect(useAppStore.getState().daemonConnected).toBe(true);
    expect(useAppStore.getState().hydrated).toBe(true);
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

  it("daemon://incompatible sets BOTH daemonIncompatible and upgradeDialogOpen", async () => {
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    await act(async () => {
      cbs.incompatible(null);
    });
    expect(useAppStore.getState().daemonIncompatible).toBe(true);
    expect(useAppStore.getState().upgradeDialogOpen).toBe(true);
  });

  describe("finding [12]/[F3]: daemon_status pull fallback closes the lost-event race", () => {
    it("hydrate failure (disconnected) + daemonStatus incompatible -> sets both flags once, opens dialog on first detection", async () => {
      vi.useFakeTimers();
      listWorkspacesMock.mockRejectedValue(new Error("disconnected"));
      daemonStatusMock.mockResolvedValue({ kind: "incompatible", daemonMin: 3, daemonMax: 4 });

      render(<App manager={fakeManager} />);
      await act(async () => {
        await Promise.resolve();
        await Promise.resolve();
        await Promise.resolve();
      });

      expect(useAppStore.getState().daemonIncompatible).toBe(true);
      expect(useAppStore.getState().upgradeDialogOpen).toBe(true);
      expect(daemonStatusMock).toHaveBeenCalled();

      // Close the dialog (simulate user Cancel) and let another poll cycle happen — it must NOT
      // reopen the dialog, only the FIRST detection opens it.
      act(() => useAppStore.getState().setUpgradeDialogOpen(false));
      await act(async () => {
        await vi.advanceTimersByTimeAsync(6000);
      });
      expect(useAppStore.getState().daemonIncompatible).toBe(true);
      expect(useAppStore.getState().upgradeDialogOpen).toBe(false);

      vi.useRealTimers();
    });

    it("hydrate failure + daemonStatus disconnected (not incompatible) -> flags stay false, no dialog", async () => {
      vi.useFakeTimers();
      listWorkspacesMock.mockRejectedValue(new Error("disconnected"));
      daemonStatusMock.mockResolvedValue({ kind: "disconnected" });

      render(<App manager={fakeManager} />);
      await act(async () => {
        await Promise.resolve();
        await Promise.resolve();
        await Promise.resolve();
      });

      expect(useAppStore.getState().daemonIncompatible).toBe(false);
      expect(useAppStore.getState().upgradeDialogOpen).toBe(false);

      vi.useRealTimers();
    });

    it("daemonStatus rejecting (best-effort) does not crash the hydrate retry loop", async () => {
      vi.useFakeTimers();
      listWorkspacesMock.mockRejectedValue(new Error("disconnected"));
      daemonStatusMock.mockRejectedValue(new Error("ipc not ready"));

      render(<App manager={fakeManager} />);
      await act(async () => {
        await Promise.resolve();
        await Promise.resolve();
        await Promise.resolve();
      });

      expect(useAppStore.getState().daemonIncompatible).toBe(false);
      expect(useAppStore.getState().daemonConnected).toBe(false);

      vi.useRealTimers();
    });

    it("connected path unaffected: successful hydrate never calls daemonStatus", async () => {
      await act(async () => {
        render(<App manager={fakeManager} />);
      });
      expect(useAppStore.getState().daemonConnected).toBe(true);
      expect(daemonStatusMock).not.toHaveBeenCalled();
    });
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
    listWorkspacesMock.mockResolvedValue([{ id: "w1", name: "proj", rootPath: "/p", roots: ["/p"] }]);
    listSessionsMock.mockResolvedValue([]);
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    const newTerminalBtn = screen.getByRole("button", { name: /new terminal/i });
    expect(newTerminalBtn).not.toHaveProperty("disabled", true);
    await act(async () => {
      newTerminalBtn.click();
    });
    // Root-aware cwd (spec §6.3): no file selected -> falls back to the workspace's roots[0].
    expect(createSessionMock).toHaveBeenCalledWith("w1", { cwd: "/p", cols: 80, rows: 24 });
  });

  it("clicking a workspace in the sidebar sets it as the active workspace for new-terminal", async () => {
    useAppStore.setState({
      sessions: {},
      workspaces: { w1: { id: "w1", name: "proj", rootPath: "/p", roots: ["/p"] } },
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
    expect(createSessionMock).toHaveBeenCalledWith("w1", { cwd: "/p", cols: 80, rows: 24 });
  });
});

describe("S2 final review A2: changedPathsToParentDirs (pure helper)", () => {
  it("maps each changed rel-path to its parent directory, deduped", () => {
    expect(changedPathsToParentDirs(["src/new.ts", "top.txt"])).toEqual(["src", ""]);
  });

  it("dedups multiple changed files sharing the same parent directory", () => {
    expect(changedPathsToParentDirs(["src/a.ts", "src/b.ts", "src/c.ts"])).toEqual(["src"]);
  });

  it("maps a nested path to its immediate parent only, not the root", () => {
    expect(changedPathsToParentDirs(["a/b/c.ts"])).toEqual(["a/b"]);
  });

  it("passes the \"*\" overflow sentinel through unchanged", () => {
    expect(changedPathsToParentDirs(["*"])).toEqual(["*"]);
  });

  it("returns an empty array for an empty input", () => {
    expect(changedPathsToParentDirs([])).toEqual([]);
  });
});

describe("T11: attention-first Home, view switch, watch + fs/workspace event wiring", () => {
  it("view='home' renders HomeView instead of the terminal layout, and hides FilesRail even with an active workspace", async () => {
    listWorkspacesMock.mockResolvedValue([{ id: "w1", name: "proj", rootPath: "/p", roots: ["/p"] }]);
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    // hydrate() auto-selects "w1" as the active workspace regardless of `view`.
    await act(async () => {
      useAppStore.getState().setView("home");
    });
    expect(screen.getByTestId("home-stats")).toBeTruthy();
    expect(screen.queryByRole("tablist")).toBeNull();
    expect(screen.queryByLabelText("Files")).toBeNull();
  });

  it("view='workspace' (default) renders the terminal tab strip, not HomeView", async () => {
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    expect(screen.getByRole("tablist")).toBeTruthy();
    expect(screen.queryByTestId("home-stats")).toBeNull();
  });

  it("clicking ⌂ Home in the sidebar switches away from the terminal layout to HomeView", async () => {
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    expect(screen.getByRole("tablist")).toBeTruthy();
    await act(async () => {
      screen.getByRole("button", { name: "Home" }).click();
    });
    expect(screen.getByTestId("home-stats")).toBeTruthy();
    expect(screen.queryByRole("tablist")).toBeNull();
  });

  it("end-to-end: Пройти from Home switches to the workspace view with that session active and focuses its terminal", async () => {
    listWorkspacesMock.mockResolvedValue([{ id: "w1", name: "proj", rootPath: "/p", roots: ["/p"] }]);
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    await act(async () => {
      cbs.created(
        meta({ id: "s1", workspaceId: "w1", waitingForInput: true, lifecycle: { kind: "running" } }),
      );
    });
    await act(async () => {
      useAppStore.getState().setView("home");
    });
    expect(screen.getByTestId("home-stats")).toBeTruthy();

    await act(async () => {
      screen.getByRole("button", { name: /пройти/i }).click();
    });

    expect(useAppStore.getState().view).toBe("workspace");
    expect(useAppStore.getState().activeSessionId).toBe("s1");
    expect(focusMock).toHaveBeenCalledWith("s1");
    expect(screen.getByTestId("terminal-pane-s1")).toBeTruthy();
  });

  it("starts the live watch for the active workspace's roots once view='workspace'; stops it on unmount", async () => {
    useAppStore.setState({
      sessions: {},
      workspaces: { w1: { id: "w1", name: "proj", rootPath: "/p", roots: ["/p", "/p/sub"] } },
      activeSessionId: null,
      daemonConnected: true,
    });
    let utils!: ReturnType<typeof render>;
    await act(async () => {
      utils = render(<App manager={fakeManager} />);
    });
    await act(async () => {
      screen.getByText("proj").click(); // selects the workspace (App-local) -> watch effect fires
    });
    expect(startWorkspaceWatchMock).toHaveBeenCalledWith(["/p", "/p/sub"], false);
    utils.unmount();
    expect(stopWorkspaceWatchMock).toHaveBeenCalled();
  });

  it("never starts the watch on Home; switching to Home stops an active watch", async () => {
    useAppStore.setState({
      sessions: {},
      workspaces: { w1: { id: "w1", name: "proj", rootPath: "/p", roots: ["/p"] } },
      activeSessionId: null,
      daemonConnected: true,
    });
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    await act(async () => {
      screen.getByText("proj").click();
    });
    expect(startWorkspaceWatchMock).toHaveBeenCalledTimes(1);
    startWorkspaceWatchMock.mockClear();
    stopWorkspaceWatchMock.mockClear();

    await act(async () => {
      useAppStore.getState().setView("home");
    });
    expect(stopWorkspaceWatchMock).toHaveBeenCalled();
    expect(startWorkspaceWatchMock).not.toHaveBeenCalled();
  });

  it("a workspace://updated push for the ACTIVE workspace restarts the watch with the fresh roots", async () => {
    useAppStore.setState({
      sessions: {},
      workspaces: { w1: { id: "w1", name: "proj", rootPath: "/p", roots: ["/p"] } },
      activeSessionId: null,
      daemonConnected: true,
    });
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    await act(async () => {
      screen.getByText("proj").click();
    });
    expect(startWorkspaceWatchMock).toHaveBeenLastCalledWith(["/p"], false);
    startWorkspaceWatchMock.mockClear();

    await act(async () => {
      cbs.wsUpdated({ id: "w1", name: "proj", rootPath: "/p", roots: ["/p", "/p2"] });
    });
    expect(startWorkspaceWatchMock).toHaveBeenLastCalledWith(["/p", "/p2"], false);
  });

  // S2 final review A2: `fs://changed`'s `changedRelPaths` are the CHANGED ENTRIES' own paths
  // (e.g. `"src/new.ts"` for a file created inside `src`) — `treeCache` is keyed by the
  // CONTAINING DIRECTORY's listing, so handing the entry's own path straight to `invalidateDirs`
  // was a silent no-op (a file key that was never cached) for anything short of the `["*"]`
  // overflow sentinel. `onFsChanged` must map each path to its parent directory first.
  it("onFsChanged (spec §5, A2) invalidates the CONTAINING directory of a changed file, not the file's own key", async () => {
    useAppStore.setState({ treeCache: { "/p\tsrc": [{ name: "a", relPath: "a", isDir: false, size: 1, isIgnored: false }] } }, false);
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    await act(async () => {
      cbs.fsChanged({ root: "/p", changedRelPaths: ["src/new.ts"] });
    });
    expect(useAppStore.getState().treeCache["/p\tsrc"]).toBeUndefined();
  });

  it("onFsChanged (A2) maps a top-level changed file to the ROOT listing's cache key (\"\")", async () => {
    useAppStore.setState({ treeCache: { "/p\t": [{ name: "a", relPath: "a", isDir: false, size: 1, isIgnored: false }] } }, false);
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    await act(async () => {
      cbs.fsChanged({ root: "/p", changedRelPaths: ["top.txt"] });
    });
    expect(useAppStore.getState().treeCache["/p\t"]).toBeUndefined();
  });

  it("onFsChanged (A2) passes the \"*\" overflow sentinel through unchanged (drops every cached dir under the root)", async () => {
    useAppStore.setState(
      {
        treeCache: {
          "/p\t": [],
          "/p\tsrc": [],
          "/p2\tsrc": [{ name: "x", relPath: "x", isDir: false, size: 1, isIgnored: false }],
        },
      },
      false,
    );
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    await act(async () => {
      cbs.fsChanged({ root: "/p", changedRelPaths: ["*"] });
    });
    const cache = useAppStore.getState().treeCache;
    expect(cache["/p\t"]).toBeUndefined();
    expect(cache["/p\tsrc"]).toBeUndefined();
    // A different root's cache entries must be left untouched.
    expect(cache["/p2\tsrc"]).toBeDefined();
  });

  it("onFsWatchError (spec §5/§7) pauses the watch honestly", async () => {
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    expect(useAppStore.getState().watchPaused).toBe(false);
    await act(async () => {
      cbs.fsWatchError({ root: "/p", reason: "watcher died" });
    });
    expect(useAppStore.getState().watchPaused).toBe(true);
  });

  // S2 final review A4: workspace A's watch dying sets `watchPaused` true; switching to a
  // DIFFERENT, healthy workspace B must clear that stale banner rather than leaving B falsely
  // showing "live updates paused" forever.
  it("a successful (re)start of the workspace watch clears a stale watchPaused (A4)", async () => {
    useAppStore.setState({
      sessions: {},
      workspaces: { w1: { id: "w1", name: "proj", rootPath: "/p", roots: ["/p"] } },
      activeSessionId: null,
      daemonConnected: true,
    });
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    await act(async () => {
      useAppStore.getState().setWatchPaused(true);
    });
    expect(useAppStore.getState().watchPaused).toBe(true);

    await act(async () => {
      screen.getByText("proj").click(); // selects the workspace -> watch (re)start effect fires
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(startWorkspaceWatchMock).toHaveBeenCalled();
    expect(useAppStore.getState().watchPaused).toBe(false);
  });

  it("onWorkspaceUpdated (spec §3.3/§6.6) upserts the workspace", async () => {
    useAppStore.setState(
      { workspaces: { w1: { id: "w1", name: "proj", rootPath: "/p", roots: ["/p"] } } },
      false,
    );
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    await act(async () => {
      cbs.wsUpdated({ id: "w1", name: "proj", rootPath: "/p", roots: ["/p", "/p2"] });
    });
    expect(useAppStore.getState().workspaces["w1"].roots).toEqual(["/p", "/p2"]);
  });

  it("renders <Toast/> near the root: a store toast message becomes visible as role=alert", async () => {
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    await act(async () => {
      useAppStore.getState().showToast("что-то пошло не так");
    });
    const alerts = screen.getAllByRole("alert");
    expect(alerts.some((el) => el.textContent?.includes("что-то пошло не так"))).toBe(true);
  });
});

describe("T12: workspace view — stat chips + command strip + root-aware cwd", () => {
  it("stat chips show correct live/waiting/exited/roots counts, scoped to the active workspace only", async () => {
    useAppStore.setState(
      {
        sessions: {
          // w1: 1 waiting, 1 live, 1 exited
          s1: meta({ id: "s1", workspaceId: "w1", waitingForInput: true, lifecycle: { kind: "running" } }),
          s2: meta({ id: "s2", workspaceId: "w1", isActive: true, waitingForInput: false, lifecycle: { kind: "running" } }),
          s3: meta({
            id: "s3",
            workspaceId: "w1",
            isActive: false,
            waitingForInput: false,
            lifecycle: { kind: "exited", code: 1, signal: null },
          }),
          // w2: must NOT be counted into w1's chips
          s4: meta({ id: "s4", workspaceId: "w2", isActive: true, waitingForInput: false, lifecycle: { kind: "running" } }),
        },
        workspaces: {
          w1: { id: "w1", name: "proj", rootPath: "/p", roots: ["/p", "/p2"] },
          w2: { id: "w2", name: "other", rootPath: "/o", roots: ["/o"] },
        },
        activeSessionId: null,
        daemonConnected: true,
      },
      false,
    );
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    await act(async () => {
      screen.getByText("proj").click(); // selects w1 as the active workspace
    });
    expect(screen.getByTestId("workspace-stats")).toBeTruthy();
    expect(screen.getByTestId("workspace-stat-live").textContent).toBe("1 live");
    expect(screen.getByTestId("workspace-stat-waiting").textContent).toBe("1 waiting");
    expect(screen.getByTestId("workspace-stat-exited").textContent).toBe("1 exited");
    expect(screen.getByTestId("workspace-stat-roots").textContent).toBe("2 roots");
  });

  it("clicking a stat chip toggles an inline detail list; clicking it again closes it", async () => {
    useAppStore.setState(
      {
        sessions: {
          s1: meta({ id: "s1", workspaceId: "w1", title: "waiting-one", waitingForInput: true, lifecycle: { kind: "running" } }),
        },
        workspaces: { w1: { id: "w1", name: "proj", rootPath: "/p", roots: ["/p"] } },
        activeSessionId: null,
        daemonConnected: true,
      },
      false,
    );
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    await act(async () => {
      screen.getByText("proj").click();
    });
    expect(screen.queryByTestId("workspace-stat-detail")).toBeNull();

    await act(async () => {
      screen.getByTestId("workspace-stat-waiting").click();
    });
    expect(screen.getByTestId("workspace-stat-detail").textContent).toContain("waiting-one");

    await act(async () => {
      screen.getByTestId("workspace-stat-waiting").click();
    });
    expect(screen.queryByTestId("workspace-stat-detail")).toBeNull();
  });

  it("stat chips render nothing while no workspace is active", async () => {
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    expect(screen.queryByTestId("workspace-stats")).toBeNull();
  });

  it("renders the CommandStrip for the active session once one exists, fetching its command history", async () => {
    getCommandEventsMock.mockResolvedValue([]);
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    await act(async () => {
      cbs.created(meta()); // s1 becomes active
    });
    expect(getCommandEventsMock).toHaveBeenCalledWith("s1", 10);
    expect(screen.getByTestId("command-strip-empty")).toBeTruthy();
  });

  it("no CommandStrip is rendered while there is no active session", async () => {
    await act(async () => {
      render(<App manager={fakeManager} />);
    });
    expect(getCommandEventsMock).not.toHaveBeenCalled();
    expect(screen.queryByTestId("command-strip")).toBeNull();
    expect(screen.queryByTestId("command-strip-empty")).toBeNull();
  });
});
