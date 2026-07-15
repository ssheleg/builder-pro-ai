import { describe, it, expect, beforeEach, vi } from "vitest";
import type { SessionMeta, Workspace } from "../ipc/types";
import type { StateChangedPayload, ExitedPayload } from "../ipc/events";
import type { FsEntry } from "../ipc/fs";
import type {
  Account,
  AuditRow,
  DomainTask,
  Goal,
  GraphView,
  Idea,
  Insight,
  McpArtifact,
  McpInvocation,
  McpServer,
  McpTool,
  Policy,
  Project,
  ResearchRun,
  RuleSetView,
  Skill,
} from "../ipc/orchd-types";

const orchdListProjectsMock = vi.fn();
const orchdListGoalsMock = vi.fn();
const orchdListIdeasMock = vi.fn();
const orchdListInsightsMock = vi.fn();
const orchdListTasksMock = vi.fn();
const researchListRunsMock = vi.fn();
const orchdGraphListProjectMock = vi.fn();
const orchdGetRulesetMock = vi.fn();
const mcpListServersMock = vi.fn();
const mcpListToolsMock = vi.fn();
const mcpListArtifactsMock = vi.fn();
const mcpListInvocationsMock = vi.fn();
const connectorListAccountsMock = vi.fn();
const skillListMock = vi.fn();
const trustListPoliciesMock = vi.fn();
const trustListAuditMock = vi.fn();
vi.mock("../ipc/orchd", () => ({
  orchdListProjects: (...a: unknown[]) => orchdListProjectsMock(...a),
  orchdListGoals: (...a: unknown[]) => orchdListGoalsMock(...a),
  orchdListIdeas: (...a: unknown[]) => orchdListIdeasMock(...a),
  orchdListInsights: (...a: unknown[]) => orchdListInsightsMock(...a),
  orchdListTasks: (...a: unknown[]) => orchdListTasksMock(...a),
  researchListRuns: (...a: unknown[]) => researchListRunsMock(...a),
  orchdGraphListProject: (...a: unknown[]) => orchdGraphListProjectMock(...a),
  orchdGetRuleset: (...a: unknown[]) => orchdGetRulesetMock(...a),
  mcpListServers: (...a: unknown[]) => mcpListServersMock(...a),
  mcpListTools: (...a: unknown[]) => mcpListToolsMock(...a),
  mcpListArtifacts: (...a: unknown[]) => mcpListArtifactsMock(...a),
  mcpListInvocations: (...a: unknown[]) => mcpListInvocationsMock(...a),
  connectorListAccounts: (...a: unknown[]) => connectorListAccountsMock(...a),
  skillList: (...a: unknown[]) => skillListMock(...a),
  trustListPolicies: (...a: unknown[]) => trustListPoliciesMock(...a),
  trustListAudit: (...a: unknown[]) => trustListAuditMock(...a),
  describeOrchdError: (e: unknown) => `mapped: ${JSON.stringify(e)}`,
}));

import { useAppStore } from "./store";

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
    orchdListProjectsMock.mockReset();
    orchdListGoalsMock.mockReset();
    orchdListIdeasMock.mockReset();
    orchdListInsightsMock.mockReset();
    orchdListTasksMock.mockReset();
    researchListRunsMock.mockReset();
    orchdGraphListProjectMock.mockReset();
    orchdGetRulesetMock.mockReset();
    mcpListServersMock.mockReset();
    mcpListToolsMock.mockReset();
    mcpListArtifactsMock.mockReset();
    mcpListInvocationsMock.mockReset();
    connectorListAccountsMock.mockReset();
    skillListMock.mockReset();
    trustListPoliciesMock.mockReset();
    trustListAuditMock.mockReset();
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
        activeProjectId: null,
        projects: [],
        goalsByProject: {},
        ideas: [],
        insights: [],
        tasksByProject: {},
        researchRunsByIdea: {},
        graphByProject: {},
        rulesets: {},
        mcpServers: [],
        mcpToolsByServer: {},
        mcpArtifacts: [],
        accounts: [],
        skills: [],
        invocations: [],
        auditRows: [],
        policies: [],
        orchdDown: false,
        orchdIncompatible: false,
        orchdUpgradeDialogOpen: false,
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

  it('setView flips between "home", "workspace", "project" and "ext" (spec §10/S-EXT §8 widened union)', () => {
    expect(useAppStore.getState().view).toBe("home");
    useAppStore.getState().setView("workspace");
    expect(useAppStore.getState().view).toBe("workspace");
    useAppStore.getState().setView("project");
    expect(useAppStore.getState().view).toBe("project");
    useAppStore.getState().setView("ext");
    expect(useAppStore.getState().view).toBe("ext");
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

  // ---- App-domain slice (S3 T13, spec §10) ----

  const project = (over: Partial<Project> = {}): Project => ({
    id: "p1",
    name: "Acme",
    description: "desc",
    status: "active",
    workspaceIds: ["w1"],
    createdAt: 1,
    updatedAt: 1,
    ...over,
  });

  const goal = (over: Partial<Goal> = {}): Goal => ({
    id: "g1",
    projectId: "p1",
    parentId: null,
    kind: "strategic",
    title: "Ship v1",
    body: "",
    ord: 0,
    status: "active",
    metricRefs: [],
    createdAt: 1,
    updatedAt: 1,
    ...over,
  });

  const idea = (over: Partial<Idea> = {}): Idea => ({
    id: "i1",
    projectId: null,
    title: "An idea",
    body: "",
    lifecycle: "captured",
    createdAt: 1,
    updatedAt: 1,
    ...over,
  });

  const insight = (over: Partial<Insight> = {}): Insight => ({
    id: "in1",
    projectId: null,
    source: "interview",
    title: "An insight",
    body: "",
    fitVerdict: null,
    fitReasoning: "",
    status: "new",
    resolutionReasoning: "",
    createdAt: 1,
    updatedAt: 1,
    ...over,
  });

  const task = (over: Partial<DomainTask> = {}): DomainTask => ({
    id: "t1",
    projectId: "p1",
    parentId: null,
    title: "A task",
    body: "",
    status: "todo",
    source: "idea",
    sourceId: null,
    tags: [],
    rank: 0,
    rankAgent: null,
    rankAgentReasoning: "",
    createdAt: 1,
    updatedAt: 1,
    ...over,
  });

  const researchRun = (over: Partial<ResearchRun> = {}): ResearchRun => ({
    id: "r1",
    ideaId: "i1",
    serverId: "s1",
    toolName: "search",
    argsJson: "{}",
    status: "pending",
    invocationId: null,
    artifactId: null,
    errorKind: null,
    createdAt: 1,
    updatedAt: 1,
    ...over,
  });

  const graphView = (over: Partial<GraphView> = {}): GraphView => ({
    nodes: [],
    edges: [],
    externalNodes: [],
    ...over,
  });

  const mcpServer = (over: Partial<McpServer> = {}): McpServer => ({
    id: "s1",
    name: "Prowl",
    transport: "http",
    url: "https://prowl.chat/mcp",
    command: null,
    args: [],
    env: {},
    scope: "global",
    projectId: null,
    authKind: "none",
    secretRef: null,
    accountId: null,
    enabled: true,
    timeoutMs: 30000,
    maxRetries: 2,
    protocolVersion: null,
    createdAt: 1,
    updatedAt: 1,
    ...over,
  });

  const mcpTool = (over: Partial<McpTool> = {}): McpTool => ({
    id: "t1",
    serverId: "s1",
    name: "search",
    title: null,
    description: null,
    inputSchemaJson: "{}",
    enabled: true,
    fetchedAt: 1,
    ...over,
  });

  const mcpArtifact = (over: Partial<McpArtifact> = {}): McpArtifact => ({
    id: "a1",
    invocationId: "i1",
    serverId: "s1",
    accountId: null,
    toolName: "search",
    projectId: null,
    contentJson: "{}",
    contentText: null,
    isUntrusted: true,
    createdAt: 1,
    ...over,
  });

  const account = (over: Partial<Account> = {}): Account => ({
    id: "a1",
    provider: "generic-rest",
    label: "My API",
    authKind: "apikey",
    scopes: [],
    expiresAt: null,
    createdAt: 1,
    updatedAt: 1,
    ...over,
  });

  const skill = (over: Partial<Skill> = {}): Skill => ({
    id: "sk1",
    name: "My Skill",
    description: "does a thing",
    mdPath: "/Users/demo/skills/my-skill/SKILL.md",
    mdHash: "deadbeef",
    scope: "global",
    projectId: null,
    fileState: "present",
    createdAt: 1,
    updatedAt: 1,
    ...over,
  });

  const mcpInvocation = (over: Partial<McpInvocation> = {}): McpInvocation => ({
    id: "inv1",
    serverId: "s1",
    accountId: null,
    toolName: "search",
    projectId: null,
    requestHash: "deadbeef",
    ok: true,
    errorKind: null,
    latencyMs: 10,
    costUsd: null,
    inputTokens: null,
    outputTokens: null,
    startedAt: 1,
    ...over,
  });

  const auditRow = (over: Partial<AuditRow> = {}): AuditRow => ({
    id: "audit1",
    at: 1,
    action: "tool_call",
    serverId: "s1",
    toolName: "search",
    projectId: null,
    decision: "allow",
    reason: null,
    invocationId: "inv1",
    ...over,
  });

  const policy = (over: Partial<Policy> = {}): Policy => ({
    id: "policy1",
    scope: "global",
    refId: null,
    spendCapUsd: null,
    ratePerMin: null,
    createdAt: 1,
    updatedAt: 1,
    ...over,
  });

  const rulesetView = (over: Partial<RuleSetView> = {}): RuleSetView => ({
    rule: {
      id: "r1",
      scope: "global",
      projectId: null,
      mdPath: "/rules.md",
      mdHash: "abc",
      policy: { spendCapUsd: null, approvalClasses: [], pathAllowlist: [] },
      createdAt: 1,
      updatedAt: 1,
    },
    mdContent: "# rules",
    fileState: "ok",
    ...over,
  });

  it("has the spec §10 initial shape (empty/false everywhere)", () => {
    const s = useAppStore.getState();
    expect(s.activeProjectId).toBeNull();
    expect(s.projects).toEqual([]);
    expect(s.goalsByProject).toEqual({});
    expect(s.ideas).toEqual([]);
    expect(s.insights).toEqual([]);
    expect(s.tasksByProject).toEqual({});
    expect(s.graphByProject).toEqual({});
    expect(s.rulesets).toEqual({});
    expect(s.mcpServers).toEqual([]);
    expect(s.mcpToolsByServer).toEqual({});
    expect(s.mcpArtifacts).toEqual([]);
    expect(s.accounts).toEqual([]);
    expect(s.orchdDown).toBe(false);
    expect(s.orchdIncompatible).toBe(false);
    expect(s.orchdUpgradeDialogOpen).toBe(false);
  });

  it("openProject sets view to \"project\" and activeProjectId to the given id", () => {
    useAppStore.getState().openProject("p1");
    expect(useAppStore.getState().view).toBe("project");
    expect(useAppStore.getState().activeProjectId).toBe("p1");
  });

  it("refreshProjects replaces the projects list from orchdListProjects", async () => {
    orchdListProjectsMock.mockResolvedValueOnce([project()]);
    await useAppStore.getState().refreshProjects();
    expect(useAppStore.getState().projects).toEqual([project()]);

    orchdListProjectsMock.mockResolvedValueOnce([project({ id: "p2", name: "Other" })]);
    await useAppStore.getState().refreshProjects();
    // REPLACED, not merged/appended — only the new list survives.
    expect(useAppStore.getState().projects).toEqual([project({ id: "p2", name: "Other" })]);
  });

  it("refreshProjects surfaces a rejection as a toast via describeOrchdError", async () => {
    const err = { kind: "disconnected" };
    orchdListProjectsMock.mockRejectedValueOnce(err);
    await useAppStore.getState().refreshProjects();
    expect(useAppStore.getState().projects).toEqual([]);
    expect(useAppStore.getState().toast).toBe(`mapped: ${JSON.stringify(err)}`);
  });

  it("refreshGoals(projectId) updates ONLY the named project's goals, leaving others untouched", async () => {
    useAppStore.setState({ goalsByProject: { p2: [goal({ id: "g2", projectId: "p2" })] } }, false);

    orchdListGoalsMock.mockResolvedValueOnce([goal({ id: "g1", projectId: "p1" })]);
    await useAppStore.getState().refreshGoals("p1");

    expect(orchdListGoalsMock).toHaveBeenCalledWith("p1");
    expect(useAppStore.getState().goalsByProject["p1"]).toEqual([
      goal({ id: "g1", projectId: "p1" }),
    ]);
    // A DIFFERENT project's entry (p2) must be untouched by a p1 refresh.
    expect(useAppStore.getState().goalsByProject["p2"]).toEqual([
      goal({ id: "g2", projectId: "p2" }),
    ]);
  });

  it("refreshIdeas replaces the whole-store ideas list, calling orchdListIdeas(null)", async () => {
    orchdListIdeasMock.mockResolvedValueOnce([idea()]);
    await useAppStore.getState().refreshIdeas();
    expect(orchdListIdeasMock).toHaveBeenCalledWith(null);
    expect(useAppStore.getState().ideas).toEqual([idea()]);
  });

  it("refreshInsights replaces the whole-store insights list, calling orchdListInsights(null)", async () => {
    orchdListInsightsMock.mockResolvedValueOnce([insight()]);
    await useAppStore.getState().refreshInsights();
    expect(orchdListInsightsMock).toHaveBeenCalledWith(null);
    expect(useAppStore.getState().insights).toEqual([insight()]);
  });

  it("refreshTasks(projectId) updates ONLY the named project's tasks, leaving others untouched", async () => {
    useAppStore.setState({ tasksByProject: { p2: [task({ id: "t2", projectId: "p2" })] } }, false);

    orchdListTasksMock.mockResolvedValueOnce([task({ id: "t1", projectId: "p1" })]);
    await useAppStore.getState().refreshTasks("p1");

    expect(orchdListTasksMock).toHaveBeenCalledWith("p1");
    expect(useAppStore.getState().tasksByProject["p1"]).toEqual([
      task({ id: "t1", projectId: "p1" }),
    ]);
    expect(useAppStore.getState().tasksByProject["p2"]).toEqual([
      task({ id: "t2", projectId: "p2" }),
    ]);
  });

  it("refreshResearchRuns(ideaId) updates ONLY the named idea's runs, leaving others untouched", async () => {
    useAppStore.setState(
      { researchRunsByIdea: { i2: [researchRun({ id: "r2", ideaId: "i2" })] } },
      false,
    );

    researchListRunsMock.mockResolvedValueOnce([researchRun({ id: "r1", ideaId: "i1" })]);
    await useAppStore.getState().refreshResearchRuns("i1");

    expect(researchListRunsMock).toHaveBeenCalledWith("i1");
    expect(useAppStore.getState().researchRunsByIdea["i1"]).toEqual([
      researchRun({ id: "r1", ideaId: "i1" }),
    ]);
    // A DIFFERENT idea's entry (i2) must be untouched by an i1 refresh.
    expect(useAppStore.getState().researchRunsByIdea["i2"]).toEqual([
      researchRun({ id: "r2", ideaId: "i2" }),
    ]);
  });

  it("refreshResearchRuns surfaces a rejection as a toast, leaving researchRunsByIdea untouched", async () => {
    const err = { kind: "disconnected" };
    researchListRunsMock.mockRejectedValueOnce(err);
    await useAppStore.getState().refreshResearchRuns("i1");
    expect(useAppStore.getState().researchRunsByIdea["i1"]).toBeUndefined();
    expect(useAppStore.getState().toast).toBe(`mapped: ${JSON.stringify(err)}`);
  });

  it("refreshGraph(projectId) replaces ONLY the named project's graph, leaving others untouched", async () => {
    const p2Graph = graphView({ nodes: [{ id: "n2", projectId: "p2", kind: "note", entityType: null, entityId: null, label: "n2", body: "", posX: 0, posY: 0, createdAt: 1, updatedAt: 1, isOrphan: false }] });
    useAppStore.setState({ graphByProject: { p2: p2Graph } }, false);

    const p1Graph = graphView({ nodes: [{ id: "n1", projectId: "p1", kind: "concept", entityType: null, entityId: null, label: "n1", body: "", posX: 1, posY: 2, createdAt: 1, updatedAt: 1, isOrphan: false }] });
    orchdGraphListProjectMock.mockResolvedValueOnce(p1Graph);
    await useAppStore.getState().refreshGraph("p1");

    // graph-changed for project P must re-fetch ONLY P.
    expect(orchdGraphListProjectMock).toHaveBeenCalledWith("p1");
    expect(useAppStore.getState().graphByProject["p1"]).toEqual(p1Graph);
    // A DIFFERENT project's entry (p2) must be untouched by a p1 refresh.
    expect(useAppStore.getState().graphByProject["p2"]).toEqual(p2Graph);
  });

  it("refreshGraph surfaces a rejection as a toast via describeOrchdError", async () => {
    const err = { kind: "disconnected" };
    orchdGraphListProjectMock.mockRejectedValueOnce(err);
    await useAppStore.getState().refreshGraph("p1");
    expect(useAppStore.getState().graphByProject["p1"]).toBeUndefined();
    expect(useAppStore.getState().toast).toBe(`mapped: ${JSON.stringify(err)}`);
  });

  it('refreshRuleset("global") calls orchdGetRuleset("global", null) and keys the result "global"', async () => {
    orchdGetRulesetMock.mockResolvedValueOnce(rulesetView());
    await useAppStore.getState().refreshRuleset("global");
    expect(orchdGetRulesetMock).toHaveBeenCalledWith("global", null);
    expect(useAppStore.getState().rulesets["global"]).toEqual(rulesetView());
  });

  it('refreshRuleset("project:<id>") calls orchdGetRuleset("project", id) and keys by the full string, leaving other keys untouched', async () => {
    useAppStore.setState({ rulesets: { global: rulesetView() } }, false);
    const projectView = rulesetView({
      rule: { ...rulesetView().rule, id: "r2", scope: "project", projectId: "p1" },
    });
    orchdGetRulesetMock.mockResolvedValueOnce(projectView);

    await useAppStore.getState().refreshRuleset("project:p1");

    expect(orchdGetRulesetMock).toHaveBeenCalledWith("project", "p1");
    expect(useAppStore.getState().rulesets["project:p1"]).toEqual(projectView);
    // The pre-existing "global" entry must be untouched by a project-scoped refresh.
    expect(useAppStore.getState().rulesets["global"]).toEqual(rulesetView());
  });

  it("setOrchdDown and setOrchdIncompatible flip their flags independently from the false default", () => {
    expect(useAppStore.getState().orchdDown).toBe(false);
    expect(useAppStore.getState().orchdIncompatible).toBe(false);
    useAppStore.getState().setOrchdDown(true);
    expect(useAppStore.getState().orchdDown).toBe(true);
    expect(useAppStore.getState().orchdIncompatible).toBe(false);
    useAppStore.getState().setOrchdIncompatible(true);
    expect(useAppStore.getState().orchdIncompatible).toBe(true);
    useAppStore.getState().setOrchdDown(false);
    expect(useAppStore.getState().orchdDown).toBe(false);
    // setOrchdDown(false) must not touch orchdIncompatible (independent flags).
    expect(useAppStore.getState().orchdIncompatible).toBe(true);
  });

  it("setOrchdUpgradeDialogOpen flips the flag from the false default", () => {
    expect(useAppStore.getState().orchdUpgradeDialogOpen).toBe(false);
    useAppStore.getState().setOrchdUpgradeDialogOpen(true);
    expect(useAppStore.getState().orchdUpgradeDialogOpen).toBe(true);
    useAppStore.getState().setOrchdUpgradeDialogOpen(false);
    expect(useAppStore.getState().orchdUpgradeDialogOpen).toBe(false);
  });

  // ---- MCP slice (S-EXT §8, T8) ----

  it("refreshMcpServers replaces mcpServers from mcpListServers(null)", async () => {
    mcpListServersMock.mockResolvedValueOnce([mcpServer()]);
    await useAppStore.getState().refreshMcpServers();
    expect(mcpListServersMock).toHaveBeenCalledWith(null);
    expect(useAppStore.getState().mcpServers).toEqual([mcpServer()]);

    mcpListServersMock.mockResolvedValueOnce([mcpServer({ id: "s2", name: "Other" })]);
    await useAppStore.getState().refreshMcpServers();
    // REPLACED, not merged/appended — only the new list survives.
    expect(useAppStore.getState().mcpServers).toEqual([mcpServer({ id: "s2", name: "Other" })]);
  });

  it("refreshMcpServers surfaces a rejection as a toast via describeOrchdError", async () => {
    const err = { kind: "disconnected" };
    mcpListServersMock.mockRejectedValueOnce(err);
    await useAppStore.getState().refreshMcpServers();
    expect(useAppStore.getState().mcpServers).toEqual([]);
    expect(useAppStore.getState().toast).toBe(`mapped: ${JSON.stringify(err)}`);
  });

  it("refreshMcpTools(serverId) updates ONLY the named server's tools, leaving others untouched", async () => {
    useAppStore.setState(
      { mcpToolsByServer: { s2: [mcpTool({ id: "t2", serverId: "s2" })] } },
      false,
    );

    mcpListToolsMock.mockResolvedValueOnce([mcpTool({ id: "t1", serverId: "s1" })]);
    await useAppStore.getState().refreshMcpTools("s1");

    expect(mcpListToolsMock).toHaveBeenCalledWith("s1");
    expect(useAppStore.getState().mcpToolsByServer["s1"]).toEqual([
      mcpTool({ id: "t1", serverId: "s1" }),
    ]);
    // A DIFFERENT server's entry (s2) must be untouched by an s1 refresh.
    expect(useAppStore.getState().mcpToolsByServer["s2"]).toEqual([
      mcpTool({ id: "t2", serverId: "s2" }),
    ]);
  });

  it("refreshMcpTools surfaces a rejection as a toast via describeOrchdError", async () => {
    const err = { kind: "daemon", code: "Policy", message: "tool_disabled" };
    mcpListToolsMock.mockRejectedValueOnce(err);
    await useAppStore.getState().refreshMcpTools("s1");
    expect(useAppStore.getState().mcpToolsByServer["s1"]).toBeUndefined();
    expect(useAppStore.getState().toast).toBe(`mapped: ${JSON.stringify(err)}`);
  });

  it("refreshMcpArtifacts replaces the whole-store mcpArtifacts list, calling mcpListArtifacts(null, null, null)", async () => {
    mcpListArtifactsMock.mockResolvedValueOnce([mcpArtifact()]);
    await useAppStore.getState().refreshMcpArtifacts();
    expect(mcpListArtifactsMock).toHaveBeenCalledWith(null, null, null);
    expect(useAppStore.getState().mcpArtifacts).toEqual([mcpArtifact()]);
  });

  it("refreshMcpArtifacts surfaces a rejection as a toast via describeOrchdError", async () => {
    const err = { kind: "disconnected" };
    mcpListArtifactsMock.mockRejectedValueOnce(err);
    await useAppStore.getState().refreshMcpArtifacts();
    expect(useAppStore.getState().mcpArtifacts).toEqual([]);
    expect(useAppStore.getState().toast).toBe(`mapped: ${JSON.stringify(err)}`);
  });

  // ---- Connectors slice (S-EXT §8, T13b) ----

  it("refreshAccounts replaces accounts from connectorListAccounts()", async () => {
    connectorListAccountsMock.mockResolvedValueOnce([account()]);
    await useAppStore.getState().refreshAccounts();
    expect(connectorListAccountsMock).toHaveBeenCalledWith();
    expect(useAppStore.getState().accounts).toEqual([account()]);

    connectorListAccountsMock.mockResolvedValueOnce([account({ id: "a2", label: "Other" })]);
    await useAppStore.getState().refreshAccounts();
    // REPLACED, not merged/appended — only the new list survives.
    expect(useAppStore.getState().accounts).toEqual([account({ id: "a2", label: "Other" })]);
  });

  it("refreshAccounts surfaces a rejection as a toast via describeOrchdError", async () => {
    const err = { kind: "disconnected" };
    connectorListAccountsMock.mockRejectedValueOnce(err);
    await useAppStore.getState().refreshAccounts();
    expect(useAppStore.getState().accounts).toEqual([]);
    expect(useAppStore.getState().toast).toBe(`mapped: ${JSON.stringify(err)}`);
  });

  // ---- Skills slice (S-EXT §8, D11, Q14, T17) ----

  it("refreshSkills replaces skills from skillList(null)", async () => {
    skillListMock.mockResolvedValueOnce([skill()]);
    await useAppStore.getState().refreshSkills();
    expect(skillListMock).toHaveBeenCalledWith(null);
    expect(useAppStore.getState().skills).toEqual([skill()]);

    skillListMock.mockResolvedValueOnce([skill({ id: "sk2", name: "Other" })]);
    await useAppStore.getState().refreshSkills();
    // REPLACED, not merged/appended — only the new list survives.
    expect(useAppStore.getState().skills).toEqual([skill({ id: "sk2", name: "Other" })]);
  });

  it("refreshSkills surfaces a rejection as a toast via describeOrchdError", async () => {
    const err = { kind: "disconnected" };
    skillListMock.mockRejectedValueOnce(err);
    await useAppStore.getState().refreshSkills();
    expect(useAppStore.getState().skills).toEqual([]);
    expect(useAppStore.getState().toast).toBe(`mapped: ${JSON.stringify(err)}`);
  });

  // ---- Trust slice (S-EXT §4/§6/§8, BL-22, T18) ----

  it("refreshInvocations replaces the whole-store invocations list, calling mcpListInvocations(null, null, null)", async () => {
    mcpListInvocationsMock.mockResolvedValueOnce([mcpInvocation()]);
    await useAppStore.getState().refreshInvocations();
    expect(mcpListInvocationsMock).toHaveBeenCalledWith(null, null, null);
    expect(useAppStore.getState().invocations).toEqual([mcpInvocation()]);
  });

  it("refreshInvocations surfaces a rejection as a toast via describeOrchdError", async () => {
    const err = { kind: "disconnected" };
    mcpListInvocationsMock.mockRejectedValueOnce(err);
    await useAppStore.getState().refreshInvocations();
    expect(useAppStore.getState().invocations).toEqual([]);
    expect(useAppStore.getState().toast).toBe(`mapped: ${JSON.stringify(err)}`);
  });

  it("refreshAuditRows replaces the whole-store auditRows list, calling trustListAudit(null)", async () => {
    trustListAuditMock.mockResolvedValueOnce([auditRow()]);
    await useAppStore.getState().refreshAuditRows();
    expect(trustListAuditMock).toHaveBeenCalledWith(null);
    expect(useAppStore.getState().auditRows).toEqual([auditRow()]);
  });

  it("refreshAuditRows surfaces a rejection as a toast via describeOrchdError", async () => {
    const err = { kind: "disconnected" };
    trustListAuditMock.mockRejectedValueOnce(err);
    await useAppStore.getState().refreshAuditRows();
    expect(useAppStore.getState().auditRows).toEqual([]);
    expect(useAppStore.getState().toast).toBe(`mapped: ${JSON.stringify(err)}`);
  });

  it("refreshPolicies replaces policies from trustListPolicies()", async () => {
    trustListPoliciesMock.mockResolvedValueOnce([policy()]);
    await useAppStore.getState().refreshPolicies();
    expect(trustListPoliciesMock).toHaveBeenCalledWith();
    expect(useAppStore.getState().policies).toEqual([policy()]);

    trustListPoliciesMock.mockResolvedValueOnce([policy({ id: "policy2", scope: "server" })]);
    await useAppStore.getState().refreshPolicies();
    // REPLACED, not merged/appended — only the new list survives.
    expect(useAppStore.getState().policies).toEqual([policy({ id: "policy2", scope: "server" })]);
  });

  it("refreshPolicies surfaces a rejection as a toast via describeOrchdError", async () => {
    const err = { kind: "disconnected" };
    trustListPoliciesMock.mockRejectedValueOnce(err);
    await useAppStore.getState().refreshPolicies();
    expect(useAppStore.getState().policies).toEqual([]);
    expect(useAppStore.getState().toast).toBe(`mapped: ${JSON.stringify(err)}`);
  });
});
