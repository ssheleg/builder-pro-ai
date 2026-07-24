import { describe, it, expect, vi, beforeEach } from "vitest";
import type { Stage, SupervisorConfig } from "./orchd-types";

const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...a: unknown[]) => invokeMock(...a),
}));

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
  orchdListWorkflows,
  orchdGetWorkflow,
  orchdUpsertWorkflow,
  orchdDeleteWorkflow,
} from "./orchd";
import { onOrchdWorkflowsChanged } from "./events";

const supervisor: SupervisorConfig = {
  enabled: false,
  delegatedClasses: [],
  instruction: "",
  customRules: [],
};

const stage: Stage = {
  id: "st-1",
  name: "Draft",
  prompt: "write it",
  skillIds: ["sk-1"],
  agent: null,
  contextScope: "inherit",
  outputs: ["draft.md"],
  gate: "auto",
};

describe("ipc/orchd — workflow wrappers (SW1)", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue(undefined);
  });

  it("orchdListWorkflows sends scope/projectId (both nullable)", async () => {
    await orchdListWorkflows(null, null);
    expect(invokeMock).toHaveBeenCalledWith("workflow_list", { scope: null, projectId: null });
    await orchdListWorkflows("project", "p1");
    expect(invokeMock).toHaveBeenCalledWith("workflow_list", { scope: "project", projectId: "p1" });
  });

  it("orchdGetWorkflow sends id", async () => {
    await orchdGetWorkflow("wf-1");
    expect(invokeMock).toHaveBeenCalledWith("workflow_get", { id: "wf-1" });
  });

  it("orchdUpsertWorkflow sends every field verbatim (empty id = create)", async () => {
    await orchdUpsertWorkflow(
      "",
      "Ship it",
      "end to end",
      "global",
      null,
      "claude-code",
      [stage],
      ["gsk-1"],
      supervisor,
    );
    expect(invokeMock).toHaveBeenCalledWith("workflow_upsert", {
      id: "",
      name: "Ship it",
      description: "end to end",
      scope: "global",
      projectId: null,
      defaultAgent: "claude-code",
      stages: [stage],
      globalSkillIds: ["gsk-1"],
      supervisor,
    });
  });

  it("orchdDeleteWorkflow sends id", async () => {
    await orchdDeleteWorkflow("wf-1");
    expect(invokeMock).toHaveBeenCalledWith("workflow_delete", { id: "wf-1" });
  });
});

describe("ipc/events — onOrchdWorkflowsChanged (SW1)", () => {
  beforeEach(() => {
    registered.clear();
    listenMock.mockClear();
    unlisten.mockClear();
  });

  it("subscribes to orchd://workflows-changed and fires the (payload-less) callback", async () => {
    const cb = vi.fn();
    await onOrchdWorkflowsChanged(cb);
    expect(listenMock).toHaveBeenCalledWith("orchd://workflows-changed", expect.any(Function));
    registered.get("orchd://workflows-changed")!({ payload: null });
    expect(cb).toHaveBeenCalledTimes(1);
  });
});
