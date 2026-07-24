import { describe, it, expect, beforeEach, vi } from "vitest";
import type { SupervisorConfig, Workflow } from "../ipc/orchd-types";

// SW1: mock only the workflow wrappers the store's workflow actions call + the error mappers
// `reportError` needs. Other `orchd_*` wrappers the store imports stay undefined here — the workflow
// actions never touch them, so a partial mock is safe (same discipline as the sibling store tests).
const orchdListWorkflowsMock = vi.fn();
const orchdUpsertWorkflowMock = vi.fn();
const orchdDeleteWorkflowMock = vi.fn();
vi.mock("../ipc/orchd", () => ({
  orchdListWorkflows: (...a: unknown[]) => orchdListWorkflowsMock(...a),
  orchdUpsertWorkflow: (...a: unknown[]) => orchdUpsertWorkflowMock(...a),
  orchdDeleteWorkflow: (...a: unknown[]) => orchdDeleteWorkflowMock(...a),
  describeOrchdError: (e: unknown) => `mapped: ${JSON.stringify(e)}`,
  isNotFoundError: () => false,
}));

// The keep-awake + commands mocks the store module also wires (kept out of the way — the workflow
// tests never exercise them).
vi.mock("../ipc/power", () => ({
  powerSetEnabled: vi.fn(),
  powerSyncSessions: vi.fn(),
  powerStatus: vi.fn(),
}));
vi.mock("../ipc/commands", () => ({ removeWorkspace: vi.fn() }));

import { useAppStore } from "./store";

function makeSupervisor(over: Partial<SupervisorConfig> = {}): SupervisorConfig {
  return {
    enabled: over.enabled ?? false,
    delegatedClasses: over.delegatedClasses ?? [],
    instruction: over.instruction ?? "",
    customRules: over.customRules ?? [],
  };
}

function wf(over: Partial<Workflow> = {}): Workflow {
  return {
    id: over.id ?? "wf-1",
    name: over.name ?? "Ship it",
    description: over.description ?? "",
    scope: over.scope ?? "global",
    projectId: over.projectId ?? null,
    defaultAgent: over.defaultAgent ?? "claude-code",
    stages: over.stages ?? [],
    globalSkillIds: over.globalSkillIds ?? [],
    supervisor: over.supervisor ?? makeSupervisor(),
    fileState: over.fileState ?? "present",
    jsonPath: over.jsonPath ?? "/tmp/rules/workflows/global/wf-1.json",
    hash: over.hash ?? "deadbeef",
    createdAt: over.createdAt ?? 1,
    updatedAt: over.updatedAt ?? 1,
  };
}

describe("useAppStore — workflows slice (SW1)", () => {
  beforeEach(() => {
    orchdListWorkflowsMock.mockReset();
    orchdUpsertWorkflowMock.mockReset();
    orchdDeleteWorkflowMock.mockReset();
    useAppStore.setState({ workflows: [], toast: null, toastQueue: [], diagEvents: [] }, false);
  });

  it("starts empty", () => {
    expect(useAppStore.getState().workflows).toEqual([]);
  });

  it("refreshWorkflows replaces the slice wholesale (all scopes)", async () => {
    const rows = [wf({ id: "a" }), wf({ id: "b", scope: "project", projectId: "p1" })];
    orchdListWorkflowsMock.mockResolvedValue(rows);

    await useAppStore.getState().refreshWorkflows();

    expect(orchdListWorkflowsMock).toHaveBeenCalledWith(null, null);
    expect(useAppStore.getState().workflows).toEqual(rows);
  });

  it("refreshWorkflows on failure toasts the mapped message and leaves the slice untouched", async () => {
    useAppStore.setState({ workflows: [wf({ id: "keep" })] }, false);
    orchdListWorkflowsMock.mockRejectedValue({ kind: "disconnected" });

    await useAppStore.getState().refreshWorkflows();

    expect(useAppStore.getState().workflows.map((w) => w.id)).toEqual(["keep"]);
    expect(useAppStore.getState().toast).toContain("mapped:");
  });

  it("upsertWorkflow sends every field, upserts the returned row by id, and resolves with it", async () => {
    const saved = wf({ id: "wf-new", name: "Created" });
    orchdUpsertWorkflowMock.mockResolvedValue(saved);

    const result = await useAppStore.getState().upsertWorkflow({
      id: "",
      name: "Created",
      description: "d",
      scope: "global",
      projectId: null,
      defaultAgent: "hermes",
      stages: [],
      globalSkillIds: ["gsk"],
      supervisor: makeSupervisor(),
    });

    expect(orchdUpsertWorkflowMock).toHaveBeenCalledWith(
      "",
      "Created",
      "d",
      "global",
      null,
      "hermes",
      [],
      ["gsk"],
      makeSupervisor(),
    );
    expect(result).toBe(saved);
    expect(useAppStore.getState().workflows).toEqual([saved]);
  });

  it("upsertWorkflow updating an existing id REPLACES its row (no duplicate)", async () => {
    useAppStore.setState({ workflows: [wf({ id: "wf-1", name: "old" })] }, false);
    const saved = wf({ id: "wf-1", name: "new" });
    orchdUpsertWorkflowMock.mockResolvedValue(saved);

    await useAppStore.getState().upsertWorkflow({
      id: "wf-1",
      name: "new",
      description: "",
      scope: "global",
      projectId: null,
      defaultAgent: "claude-code",
      stages: [],
      globalSkillIds: [],
      supervisor: makeSupervisor(),
    });

    const rows = useAppStore.getState().workflows;
    expect(rows).toHaveLength(1);
    expect(rows[0].name).toBe("new");
  });

  it("upsertWorkflow rejection PROPAGATES and leaves the slice untouched", async () => {
    useAppStore.setState({ workflows: [wf({ id: "wf-1" })] }, false);
    orchdUpsertWorkflowMock.mockRejectedValue({ kind: "daemon", code: "Validation" });

    await expect(
      useAppStore.getState().upsertWorkflow({
        id: "",
        name: "x",
        description: "",
        scope: "global",
        projectId: null,
        defaultAgent: "claude-code",
        stages: [],
        globalSkillIds: [],
        supervisor: makeSupervisor(),
      }),
    ).rejects.toMatchObject({ code: "Validation" });

    expect(useAppStore.getState().workflows.map((w) => w.id)).toEqual(["wf-1"]);
  });

  it("deleteWorkflow removes the row on success (daemon-first)", async () => {
    useAppStore.setState({ workflows: [wf({ id: "a" }), wf({ id: "b" })] }, false);
    orchdDeleteWorkflowMock.mockResolvedValue(undefined);

    await useAppStore.getState().deleteWorkflow("a");

    expect(orchdDeleteWorkflowMock).toHaveBeenCalledWith("a");
    expect(useAppStore.getState().workflows.map((w) => w.id)).toEqual(["b"]);
  });

  it("deleteWorkflow rejection PROPAGATES and leaves the row in place", async () => {
    useAppStore.setState({ workflows: [wf({ id: "a" })] }, false);
    orchdDeleteWorkflowMock.mockRejectedValue({ kind: "daemon", code: "NotFound" });

    await expect(useAppStore.getState().deleteWorkflow("a")).rejects.toMatchObject({
      code: "NotFound",
    });
    expect(useAppStore.getState().workflows.map((w) => w.id)).toEqual(["a"]);
  });

  it("acts as the workflows-changed handler: a second refresh re-fetches wholesale", async () => {
    orchdListWorkflowsMock.mockResolvedValueOnce([wf({ id: "a" })]);
    await useAppStore.getState().refreshWorkflows();
    expect(useAppStore.getState().workflows.map((w) => w.id)).toEqual(["a"]);

    // Simulate an `orchd://workflows-changed` push landing → the App handler calls refreshWorkflows,
    // which replaces the slice from the daemon (here the row was deleted upstream).
    orchdListWorkflowsMock.mockResolvedValueOnce([]);
    await useAppStore.getState().refreshWorkflows();
    expect(useAppStore.getState().workflows).toEqual([]);
  });
});
