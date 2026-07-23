import { describe, it, expect, vi, beforeEach } from "vitest";

const invokeMock = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({
  invoke: (...a: unknown[]) => invokeMock(...a),
}));

import { powerSetEnabled, powerSyncSessions, powerStatus } from "./power";
import type { PowerStatus } from "./power";

const active: PowerStatus = { enabled: true, active: true, error: null };
const denied: PowerStatus = { enabled: true, active: false, error: "os denied" };

describe("ipc/power (SCN-045)", () => {
  beforeEach(() => {
    invokeMock.mockReset();
    invokeMock.mockResolvedValue(active);
  });

  it("powerSetEnabled sends the toggle and resolves the reconciled PowerStatus", async () => {
    invokeMock.mockResolvedValueOnce({ enabled: false, active: false, error: null });
    const res = await powerSetEnabled(false);
    expect(invokeMock).toHaveBeenCalledWith("power_set_enabled", { enabled: false });
    expect(res).toEqual({ enabled: false, active: false, error: null });
  });

  it("powerSyncSessions sends the live count and resolves the reconciled PowerStatus", async () => {
    const res = await powerSyncSessions(2);
    expect(invokeMock).toHaveBeenCalledWith("power_sync_sessions", { live: 2 });
    expect(res).toEqual(active);
  });

  it("powerStatus is a pure read (no args) resolving the current PowerStatus", async () => {
    invokeMock.mockResolvedValueOnce(denied);
    const res = await powerStatus();
    expect(invokeMock).toHaveBeenCalledWith("power_status");
    expect(res).toEqual(denied);
  });

  it("a rejected invoke propagates as-is (handling is the store's job, not this layer's)", async () => {
    invokeMock.mockRejectedValueOnce({ kind: "internal", message: "boom" });
    await expect(powerSyncSessions(1)).rejects.toEqual({ kind: "internal", message: "boom" });
  });
});
