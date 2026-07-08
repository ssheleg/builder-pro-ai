import { describe, it, expect, beforeEach, vi } from "vitest";
import { useAppStore } from "./store";
import type { SessionMeta, Workspace } from "../ipc/types";
import type { StateChangedPayload, ExitedPayload } from "../ipc/events";
import type { FsEntry } from "../ipc/fs";

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
        daemonIncompatible: false,
        upgradeDialogOpen: false,
        upgradeError: null,
        hydrated: false,
        view: "home",
        expanded: {},
        treeCache: {},
        selectedFile: null,
        showIgnored: false,
        filesRailOpen: false,
        watchPaused: false,
        toast: null,
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
    expect(s.daemonIncompatible).toBe(false);
    expect(s.upgradeDialogOpen).toBe(false);
    expect(s.upgradeError).toBeNull();
    expect(s.hydrated).toBe(false);
    expect(typeof initial.upsertSession).toBe("function");
  });

  it("has the spec §6.6 navigation + fs-slice initial shape, defaulting view to \"home\"", () => {
    const s = useAppStore.getState();
    expect(s.view).toBe("home");
    expect(s.expanded).toEqual({});
    expect(s.treeCache).toEqual({});
    expect(s.selectedFile).toBeNull();
    expect(s.showIgnored).toBe(false);
    expect(s.filesRailOpen).toBe(false);
    expect(s.watchPaused).toBe(false);
    expect(typeof initial.setView).toBe("function");
    expect(typeof initial.setExpanded).toBe("function");
    expect(typeof initial.cacheDir).toBe("function");
    expect(typeof initial.invalidateDirs).toBe("function");
    expect(typeof initial.setSelectedFile).toBe("function");
    expect(typeof initial.toggleShowIgnored).toBe("function");
    expect(typeof initial.setFilesRailOpen).toBe("function");
    expect(typeof initial.setWatchPaused).toBe("function");
  });

  it("setView flips between \"home\" and \"workspace\"", () => {
    expect(useAppStore.getState().view).toBe("home");
    useAppStore.getState().setView("workspace");
    expect(useAppStore.getState().view).toBe("workspace");
    useAppStore.getState().setView("home");
    expect(useAppStore.getState().view).toBe("home");
  });

  it("setExpanded sets a keyed entry to true, then collapses (removes) it", () => {
    useAppStore.getState().setExpanded("/root", "sub", true);
    expect(useAppStore.getState().expanded).toEqual({ "/root\tsub": true });
    useAppStore.getState().setExpanded("/root", "sub", false);
    expect(useAppStore.getState().expanded).toEqual({});
  });

  it("setExpanded keys independently by root+rel — same rel under a different root is distinct", () => {
    useAppStore.getState().setExpanded("/root-a", "sub", true);
    useAppStore.getState().setExpanded("/root-b", "sub", true);
    expect(useAppStore.getState().expanded).toEqual({
      "/root-a\tsub": true,
      "/root-b\tsub": true,
    });
    useAppStore.getState().setExpanded("/root-a", "sub", false);
    expect(useAppStore.getState().expanded).toEqual({ "/root-b\tsub": true });
  });

  it("cacheDir stores entries keyed by root+rel, then replaces on a repeat call", () => {
    const entries: FsEntry[] = [
      { name: "a.txt", relPath: "sub/a.txt", isDir: false, size: 3, isIgnored: false },
    ];
    useAppStore.getState().cacheDir("/root", "sub", entries);
    expect(useAppStore.getState().treeCache["/root\tsub"]).toEqual(entries);

    const replaced: FsEntry[] = [];
    useAppStore.getState().cacheDir("/root", "sub", replaced);
    expect(useAppStore.getState().treeCache["/root\tsub"]).toEqual(replaced);
  });

  it("invalidateDirs(root, [rel]) drops only that dir's CACHE entry but leaves `expanded` (a point refresh keeps dirs open, spec §5/§6.4)", () => {
    const entriesA: FsEntry[] = [
      { name: "x", relPath: "a/x", isDir: false, size: 1, isIgnored: false },
    ];
    const entriesB: FsEntry[] = [
      { name: "y", relPath: "b/y", isDir: false, size: 1, isIgnored: false },
    ];
    useAppStore.getState().cacheDir("/root", "a", entriesA);
    useAppStore.getState().cacheDir("/root", "b", entriesB);
    useAppStore.getState().setExpanded("/root", "a", true);
    useAppStore.getState().setExpanded("/root", "b", true);

    useAppStore.getState().invalidateDirs("/root", ["a"]);

    expect(useAppStore.getState().treeCache["/root\ta"]).toBeUndefined();
    expect(useAppStore.getState().treeCache["/root\tb"]).toEqual(entriesB);
    // `expanded` is untouched by invalidation — a dir the owner had open stays open, and
    // FileTree's own auto-refetch effect (spec §6.4) re-fetches it since it's now uncached.
    expect(useAppStore.getState().expanded["/root\ta"]).toBe(true);
    expect(useAppStore.getState().expanded["/root\tb"]).toBe(true);
  });

  it('invalidateDirs(root, ["*"]) drops ALL cache entries for that root but leaves `expanded` (every root) untouched', () => {
    const entries: FsEntry[] = [
      { name: "x", relPath: "a/x", isDir: false, size: 1, isIgnored: false },
    ];
    useAppStore.getState().cacheDir("/root-a", "", entries);
    useAppStore.getState().cacheDir("/root-a", "sub", entries);
    useAppStore.getState().cacheDir("/root-b", "", entries);
    useAppStore.getState().setExpanded("/root-a", "sub", true);
    useAppStore.getState().setExpanded("/root-b", "sub", true);

    useAppStore.getState().invalidateDirs("/root-a", ["*"]);

    expect(useAppStore.getState().treeCache["/root-a\t"]).toBeUndefined();
    expect(useAppStore.getState().treeCache["/root-a\tsub"]).toBeUndefined();
    // other root's cache untouched
    expect(useAppStore.getState().treeCache["/root-b\t"]).toEqual(entries);
    // `expanded` is untouched for BOTH the invalidated root and every other root
    expect(useAppStore.getState().expanded["/root-a\tsub"]).toBe(true);
    expect(useAppStore.getState().expanded["/root-b\tsub"]).toBe(true);
  });

  it("setSelectedFile sets and clears (null) the selection", () => {
    expect(useAppStore.getState().selectedFile).toBeNull();
    useAppStore.getState().setSelectedFile({ root: "/root", rel: "a.txt" });
    expect(useAppStore.getState().selectedFile).toEqual({ root: "/root", rel: "a.txt" });
    useAppStore.getState().setSelectedFile(null);
    expect(useAppStore.getState().selectedFile).toBeNull();
  });

  it("toggleShowIgnored flips the flag from the false default", () => {
    expect(useAppStore.getState().showIgnored).toBe(false);
    useAppStore.getState().toggleShowIgnored();
    expect(useAppStore.getState().showIgnored).toBe(true);
    useAppStore.getState().toggleShowIgnored();
    expect(useAppStore.getState().showIgnored).toBe(false);
  });

  it("setFilesRailOpen sets the rail visibility from the false default", () => {
    expect(useAppStore.getState().filesRailOpen).toBe(false);
    useAppStore.getState().setFilesRailOpen(true);
    expect(useAppStore.getState().filesRailOpen).toBe(true);
    useAppStore.getState().setFilesRailOpen(false);
    expect(useAppStore.getState().filesRailOpen).toBe(false);
  });

  it("setWatchPaused sets the paused flag (fs://watch-error) and clears it on resume", () => {
    expect(useAppStore.getState().watchPaused).toBe(false);
    useAppStore.getState().setWatchPaused(true);
    expect(useAppStore.getState().watchPaused).toBe(true);
    useAppStore.getState().setWatchPaused(false);
    expect(useAppStore.getState().watchPaused).toBe(false);
  });

  it("setUpgradeError sets and clears the error message (finding [13])", () => {
    expect(useAppStore.getState().upgradeError).toBeNull();
    useAppStore.getState().setUpgradeError("Operation not permitted");
    expect(useAppStore.getState().upgradeError).toBe("Operation not permitted");
    useAppStore.getState().setUpgradeError(null);
    expect(useAppStore.getState().upgradeError).toBeNull();
  });

  it("setHydrated flips the flag from the false default (finding [14])", () => {
    expect(useAppStore.getState().hydrated).toBe(false);
    useAppStore.getState().setHydrated(true);
    expect(useAppStore.getState().hydrated).toBe(true);
  });

  it("setUpgradeDialogOpen(true) clears a stale upgradeError; setUpgradeDialogOpen(false) leaves it untouched (finding [13])", () => {
    useAppStore.getState().setUpgradeError("Operation not permitted");
    useAppStore.getState().setUpgradeDialogOpen(true);
    expect(useAppStore.getState().upgradeError).toBeNull();

    useAppStore.getState().setUpgradeError("Operation not permitted");
    useAppStore.getState().setUpgradeDialogOpen(false);
    expect(useAppStore.getState().upgradeError).toBe("Operation not permitted");
  });

  it("setDaemonIncompatible and setUpgradeDialogOpen flip their flags from the false default", () => {
    expect(useAppStore.getState().daemonIncompatible).toBe(false);
    expect(useAppStore.getState().upgradeDialogOpen).toBe(false);
    useAppStore.getState().setDaemonIncompatible(true);
    expect(useAppStore.getState().daemonIncompatible).toBe(true);
    useAppStore.getState().setUpgradeDialogOpen(true);
    expect(useAppStore.getState().upgradeDialogOpen).toBe(true);
  });

  it("honesty invariant: setUpgradeDialogOpen(false) (Cancel) leaves daemonIncompatible untouched", () => {
    useAppStore.getState().setDaemonIncompatible(true);
    useAppStore.getState().setUpgradeDialogOpen(true);
    useAppStore.getState().setUpgradeDialogOpen(false);
    expect(useAppStore.getState().upgradeDialogOpen).toBe(false);
    expect(useAppStore.getState().daemonIncompatible).toBe(true);
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

  it("markExited clears waitingForInput — a finished process is not waiting for input, the honest state for every consumer (stats/StatusDot/HomeView) per review finding F1", () => {
    useAppStore.getState().upsertSession(meta({ waitingForInput: true }));
    const p: ExitedPayload = { sessionId: "s1", code: 1, signal: null };
    useAppStore.getState().markExited(p);
    const s = useAppStore.getState().sessions["s1"];
    expect(s.waitingForInput).toBe(false);
    expect(s.isActive).toBe(false);
    expect(s.lifecycle).toEqual({ kind: "exited", code: 1, signal: null });
  });

  it("setDaemonConnected toggles the flag", () => {
    useAppStore.getState().setDaemonConnected(true);
    expect(useAppStore.getState().daemonConnected).toBe(true);
    useAppStore.getState().setDaemonConnected(false);
    expect(useAppStore.getState().daemonConnected).toBe(false);
  });

  it("upsertWorkspace adds then replaces by id", () => {
    const w: Workspace = { id: "w1", name: "proj", rootPath: "/p", roots: ["/p"] };
    useAppStore.getState().upsertWorkspace(w);
    expect(useAppStore.getState().workspaces["w1"].name).toBe("proj");
    useAppStore.getState().upsertWorkspace({ ...w, name: "renamed" });
    expect(useAppStore.getState().workspaces["w1"].name).toBe("renamed");
    expect(Object.keys(useAppStore.getState().workspaces)).toHaveLength(1);
  });

  it("upsertWorkspace applied for a workspace://updated payload (added root) keeps sessions/activeSessionId/other state untouched", () => {
    const w: Workspace = { id: "w1", name: "proj", rootPath: "/p", roots: ["/p"] };
    useAppStore.getState().upsertWorkspace(w);
    useAppStore.getState().upsertSession(meta());
    useAppStore.getState().setActiveSession("s1");
    useAppStore.getState().setDaemonConnected(true);

    // workspace://updated's payload IS a Workspace (spec §6.6) — the listener just calls
    // upsertWorkspace with it directly, e.g. after `addWorkspaceRoot("w1", "/q")`.
    const updated: Workspace = { id: "w1", name: "proj", rootPath: "/p", roots: ["/p", "/q"] };
    useAppStore.getState().upsertWorkspace(updated);

    expect(useAppStore.getState().workspaces["w1"]).toEqual(updated);
    expect(Object.keys(useAppStore.getState().workspaces)).toHaveLength(1);
    // untouched
    expect(useAppStore.getState().sessions["s1"]).toEqual(meta());
    expect(useAppStore.getState().activeSessionId).toBe("s1");
    expect(useAppStore.getState().daemonConnected).toBe(true);
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

  // ---- Toast atom (S2 T9, spec §7 honest error surface) ----

  it("has toast=null by default and exposes showToast/dismissToast", () => {
    const s = useAppStore.getState();
    expect(s.toast).toBeNull();
    expect(typeof initial.showToast).toBe("function");
    expect(typeof initial.dismissToast).toBe("function");
  });

  it("showToast sets the current toast message", () => {
    useAppStore.getState().showToast("Не удалось подключиться к демону");
    expect(useAppStore.getState().toast).toBe("Не удалось подключиться к демону");
  });

  it("dismissToast clears the toast", () => {
    useAppStore.getState().showToast("boom");
    useAppStore.getState().dismissToast();
    expect(useAppStore.getState().toast).toBeNull();
  });

  it("showToast replaces the message when called again (queue-of-one — no queueing)", () => {
    useAppStore.getState().showToast("first");
    expect(useAppStore.getState().toast).toBe("first");
    useAppStore.getState().showToast("second");
    expect(useAppStore.getState().toast).toBe("second");
  });

  it("auto-dismisses ~4s after showToast (fake timers)", () => {
    vi.useFakeTimers();
    try {
      useAppStore.getState().showToast("will vanish");
      expect(useAppStore.getState().toast).toBe("will vanish");
      vi.advanceTimersByTime(3999);
      expect(useAppStore.getState().toast).toBe("will vanish");
      vi.advanceTimersByTime(1);
      expect(useAppStore.getState().toast).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  it("a later showToast's auto-dismiss timer does not clear an even-later toast (stale-timer guard)", () => {
    vi.useFakeTimers();
    try {
      useAppStore.getState().showToast("first");
      vi.advanceTimersByTime(2000); // first is at 2s of its 4s life, not yet dismissed
      useAppStore.getState().showToast("second"); // restarts the window
      vi.advanceTimersByTime(2000); // first's original 4s deadline passes; second is at 2s
      expect(useAppStore.getState().toast).toBe("second"); // must NOT have been cleared
      vi.advanceTimersByTime(2000); // second's own 4s deadline passes
      expect(useAppStore.getState().toast).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  it("dismissToast before the auto-dismiss timer fires prevents a later stray clear", () => {
    vi.useFakeTimers();
    try {
      useAppStore.getState().showToast("first");
      useAppStore.getState().dismissToast();
      useAppStore.getState().showToast("second");
      vi.advanceTimersByTime(4000); // only "second"'s own timer should fire
      expect(useAppStore.getState().toast).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });
});
