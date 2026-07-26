// FE-1 / UX-1 / FE-2 / FE-6 / FS-8 — store-level tests for the 2026-07-24 audit remediation:
// the generalized `refresh*` race guard, the per-slice first-fetch flags, verbatim string
// errors in `reportError`, the toast tone queue, and the FileTree invalidation epochs.
// (The flip of /tmp/bpa-probes/fe/fe1-refresh-race.probe.test.ts lives in the first describe.)
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import type { Goal } from "../ipc/orchd-types";

const orchdListProjectsMock = vi.fn();
const orchdListGoalsMock = vi.fn();
const orchdListIdeasMock = vi.fn();
const orchdListInsightsMock = vi.fn();
const orchdListTasksMock = vi.fn();
const researchListRunsMock = vi.fn();
const mcpListServersMock = vi.fn();
const mcpListArtifactsMock = vi.fn();
const mcpListInvocationsMock = vi.fn();
vi.mock("../ipc/orchd", () => ({
  orchdListProjects: (...a: unknown[]) => orchdListProjectsMock(...a),
  orchdListGoals: (...a: unknown[]) => orchdListGoalsMock(...a),
  orchdListIdeas: (...a: unknown[]) => orchdListIdeasMock(...a),
  orchdListInsights: (...a: unknown[]) => orchdListInsightsMock(...a),
  orchdListTasks: (...a: unknown[]) => orchdListTasksMock(...a),
  researchListRuns: (...a: unknown[]) => researchListRunsMock(...a),
  orchdGraphListProject: vi.fn(),
  orchdGetRuleset: vi.fn(),
  orchdListDocs: vi.fn(),
  orchdGetDoc: vi.fn(),
  mcpListServers: (...a: unknown[]) => mcpListServersMock(...a),
  mcpListTools: vi.fn(),
  mcpListArtifacts: (...a: unknown[]) => mcpListArtifactsMock(...a),
  mcpListInvocations: (...a: unknown[]) => mcpListInvocationsMock(...a),
  connectorListAccounts: vi.fn(),
  skillList: vi.fn(),
  trustListPolicies: vi.fn(),
  trustListAudit: vi.fn(),
  orchdStorageStatus: vi.fn(),
  describeOrchdError: (e: unknown) => `mapped: ${JSON.stringify(e)}`,
  isNotFoundError: () => false,
}));
vi.mock("../ipc/power", () => ({
  powerSetEnabled: vi.fn(),
  powerSyncSessions: vi.fn(),
  powerStatus: vi.fn(),
}));
vi.mock("../ipc/commands", () => ({ removeWorkspace: vi.fn() }));

import { useAppStore } from "./store";

function deferred<T>() {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

/** Flush the microtask queue so a settled attempt's trailing re-run can start (real timers). */
async function tick(): Promise<void> {
  await new Promise((r) => setTimeout(r, 0));
}

const goal = (id: string, title: string): Goal => ({
  id,
  projectId: "p1",
  parentId: null,
  kind: "additional",
  title,
  body: "",
  ord: 0,
  status: "active",
  metricRefs: [],
  createdAt: 1,
  updatedAt: 1,
});

beforeEach(() => {
  orchdListProjectsMock.mockReset();
  orchdListGoalsMock.mockReset();
  orchdListIdeasMock.mockReset();
  orchdListInsightsMock.mockReset();
  orchdListTasksMock.mockReset();
  researchListRunsMock.mockReset();
  mcpListServersMock.mockReset();
  mcpListArtifactsMock.mockReset();
  mcpListInvocationsMock.mockReset();
  useAppStore.setState(
    {
      projects: [],
      goalsByProject: {},
      ideas: [],
      insights: [],
      tasksByProject: {},
      researchRunsByIdea: {},
      mcpServers: [],
      mcpArtifacts: [],
      invocations: [],
      projectsFetched: false,
      goalsFetched: {},
      ideasFetched: false,
      insightsFetched: false,
      tasksFetched: {},
      researchRunsFetched: {},
      mcpServersFetched: false,
      mcpArtifactsFetched: false,
      expanded: {},
      treeCache: {},
      treeEpochs: {},
      toast: null,
      toastQueue: [],
      toastTone: "error",
      toastToneQueue: [],
      diagEvents: [],
    },
    false,
  );
});

afterEach(() => {
  vi.useRealTimers();
});

describe("FE-1 refresh race guard (flipped probe fe1-refresh-race)", () => {
  it("A: an out-of-order stale response is dropped; the trailing re-run's fresher data wins", async () => {
    const d1 = deferred<Goal[]>(); // first fetch — resolves with STALE data
    const d2 = deferred<Goal[]>(); // the trailing re-run's fetch — FRESH data
    orchdListGoalsMock.mockReturnValueOnce(d1.promise).mockReturnValueOnce(d2.promise);

    const p1 = useAppStore.getState().refreshGoals("p1");
    const p2 = useAppStore.getState().refreshGoals("p1");
    // Dedup: the second call did NOT stack a parallel invoke while the first was in flight.
    expect(orchdListGoalsMock).toHaveBeenCalledTimes(1);

    // The first (now stale — a newer refresh was requested after it started) response lands...
    d1.resolve([goal("g-stale", "STALE")]);
    await tick();
    // ...and is dropped: the store never shows the stale payload...
    expect(useAppStore.getState().goalsByProject["p1"]).toBeUndefined();
    // ...instead the dirty guard fired exactly ONE trailing re-run.
    expect(orchdListGoalsMock).toHaveBeenCalledTimes(2);

    d2.resolve([goal("g-fresh", "FRESH")]);
    await p1;
    await p2;
    expect(useAppStore.getState().goalsByProject["p1"]?.map((g) => g.title)).toEqual(["FRESH"]);
  });

  it("B: three back-to-back refreshInvocations collapse into ONE trailing fetch (300ms debounce)", async () => {
    vi.useFakeTimers();
    mcpListInvocationsMock.mockResolvedValue([]);
    const w1 = useAppStore.getState().refreshInvocations();
    const w2 = useAppStore.getState().refreshInvocations();
    const w3 = useAppStore.getState().refreshInvocations();
    expect(mcpListInvocationsMock).not.toHaveBeenCalled(); // still inside the debounce window

    await vi.advanceTimersByTimeAsync(350);
    await Promise.all([w1, w2, w3]); // every collapsed caller's promise resolves after the fetch
    expect(mcpListInvocationsMock).toHaveBeenCalledTimes(1);
    expect(mcpListInvocationsMock).toHaveBeenCalledWith(null, null, null);
  });

  it("keyed guards are independent: goals of p1 and p2 fetch in parallel", async () => {
    orchdListGoalsMock.mockResolvedValue([]);
    await Promise.all([
      useAppStore.getState().refreshGoals("p1"),
      useAppStore.getState().refreshGoals("p2"),
    ]);
    expect(orchdListGoalsMock).toHaveBeenCalledTimes(2);
    expect(orchdListGoalsMock).toHaveBeenCalledWith("p1");
    expect(orchdListGoalsMock).toHaveBeenCalledWith("p2");
  });

  it("an error in the in-flight attempt still lets the queued re-run happen", async () => {
    const d1 = deferred<Goal[]>();
    orchdListGoalsMock.mockReturnValueOnce(d1.promise).mockResolvedValueOnce([goal("g", "ok")]);
    const p1 = useAppStore.getState().refreshGoals("p1");
    void useAppStore.getState().refreshGoals("p1"); // dirty while d1 in flight
    d1.reject({ kind: "disconnected" });
    await p1;
    expect(orchdListGoalsMock).toHaveBeenCalledTimes(2);
    expect(useAppStore.getState().goalsByProject["p1"]?.map((g) => g.title)).toEqual(["ok"]);
    expect(useAppStore.getState().toast).toBe(`mapped: {"kind":"disconnected"}`);
  });
});

describe("UX-1 first-fetch flags", () => {
  it("refreshProjects sets projectsFetched on success", async () => {
    expect(useAppStore.getState().projectsFetched).toBe(false);
    orchdListProjectsMock.mockResolvedValueOnce([]);
    await useAppStore.getState().refreshProjects();
    expect(useAppStore.getState().projectsFetched).toBe(true);
  });

  it("refreshProjects sets projectsFetched on FAILURE too (no eternal loading)", async () => {
    orchdListProjectsMock.mockRejectedValueOnce({ kind: "disconnected" });
    await useAppStore.getState().refreshProjects();
    expect(useAppStore.getState().projectsFetched).toBe(true);
    expect(useAppStore.getState().toast).toBe(`mapped: {"kind":"disconnected"}`);
  });

  it("keyed flags: refreshGoals/refreshTasks/refreshResearchRuns flip only their own key", async () => {
    orchdListGoalsMock.mockResolvedValueOnce([]);
    orchdListTasksMock.mockRejectedValueOnce({ kind: "disconnected" }); // failure still flips
    researchListRunsMock.mockResolvedValueOnce([]);
    await useAppStore.getState().refreshGoals("p1");
    await useAppStore.getState().refreshTasks("p1");
    await useAppStore.getState().refreshResearchRuns("idea-1");
    const s = useAppStore.getState();
    expect(s.goalsFetched).toEqual({ p1: true });
    expect(s.tasksFetched).toEqual({ p1: true });
    expect(s.researchRunsFetched).toEqual({ "idea-1": true });
  });

  it("refreshIdeas/refreshInsights/refreshMcpServers/refreshMcpArtifacts flip their flags", async () => {
    orchdListIdeasMock.mockResolvedValueOnce([]);
    orchdListInsightsMock.mockResolvedValueOnce([]);
    mcpListServersMock.mockResolvedValueOnce([]);
    mcpListArtifactsMock.mockRejectedValueOnce({ kind: "disconnected" }); // failure still flips
    await useAppStore.getState().refreshIdeas();
    await useAppStore.getState().refreshInsights();
    await useAppStore.getState().refreshMcpServers();
    await useAppStore.getState().refreshMcpArtifacts();
    const s = useAppStore.getState();
    expect(s.ideasFetched).toBe(true);
    expect(s.insightsFetched).toBe(true);
    expect(s.mcpServersFetched).toBe(true);
    expect(s.mcpArtifactsFetched).toBe(true);
  });
});

describe("FE-2 reportError passes string errors through verbatim", () => {
  it('reportError("refreshStats", "scan worker died: X") toasts the string itself', () => {
    const shown = useAppStore.getState().reportError("refreshStats", "scan worker died: X");
    expect(shown).toBe("scan worker died: X");
    expect(useAppStore.getState().toast).toBe("scan worker died: X");
    const event = useAppStore.getState().diagEvents[0];
    expect(event.kind).toBe("message"); // a deliberate human string, not "unknown"
    expect(event.message).toBe("scan worker died: X");
  });

  it("string errors are still secret-scrubbed before display", () => {
    useAppStore.getState().reportError("op", "scan failed for /Users/alice/x");
    expect(useAppStore.getState().toast).toBe("scan failed for /Users/«user»/x");
  });
});

describe("FE-6 toast tone", () => {
  it("defaults to the error tone and keeps tones in lockstep with the queue", () => {
    vi.useFakeTimers();
    const s = useAppStore.getState();
    s.showToast("broken");
    s.showToast("saved", "success");
    expect(useAppStore.getState().toast).toBe("broken");
    expect(useAppStore.getState().toastTone).toBe("error");
    expect(useAppStore.getState().toastToneQueue).toEqual(["error", "success"]);
    useAppStore.getState().dismissToast();
    expect(useAppStore.getState().toast).toBe("saved");
    expect(useAppStore.getState().toastTone).toBe("success");
    useAppStore.getState().dismissToast();
    expect(useAppStore.getState().toast).toBeNull();
    expect(useAppStore.getState().toastTone).toBe("error"); // drained — back to the default
  });
});

describe("FS-8 treeEpochs invalidation epochs", () => {
  it('invalidateDirs "* " bumps every tracked key under the root and drops the cache', () => {
    useAppStore.setState(
      {
        expanded: { "/proj\t": true, "/proj\tsrc": true },
        treeCache: { "/proj\t": [], "/proj\tsrc": [], "/other\t": [] },
        treeEpochs: { "/proj\t": 2, "/other\t": 7 },
      },
      false,
    );
    useAppStore.getState().invalidateDirs("/proj", ["*"]);
    const s = useAppStore.getState();
    // Cache under /proj dropped, the other root untouched (pre-existing behavior)...
    expect(s.treeCache).toEqual({ "/other\t": [] });
    // ...and every key that could have an in-flight fetch under /proj got its epoch bumped —
    // including the expanded-but-never-cached "/proj\tsrc" (a first fetch in flight for it).
    expect(s.treeEpochs["/proj\t"]).toBe(3);
    expect(s.treeEpochs["/proj\tsrc"]).toBe(1);
    expect(s.treeEpochs["/other\t"]).toBe(7);
  });

  it("invalidateDirs with explicit rels bumps exactly those keys", () => {
    useAppStore.setState({ treeEpochs: { "/proj\ta": 1, "/proj\tb": 5 } }, false);
    useAppStore.getState().invalidateDirs("/proj", ["a"]);
    const s = useAppStore.getState();
    expect(s.treeEpochs["/proj\ta"]).toBe(2);
    expect(s.treeEpochs["/proj\tb"]).toBe(5);
  });
});
