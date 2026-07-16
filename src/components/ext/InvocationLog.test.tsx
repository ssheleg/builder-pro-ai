// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, fireEvent, waitFor, act } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const mcpListInvocationsMock = vi.fn();
const trustListAuditMock = vi.fn();
const trustListPoliciesMock = vi.fn();
const trustSetPolicyMock = vi.fn();
const describeOrchdErrorMock = vi.fn((..._a: unknown[]) => "orchestrator: error");
vi.mock("../../ipc/orchd", () => ({
  mcpListInvocations: (...a: unknown[]) => mcpListInvocationsMock(...a),
  trustListAudit: (...a: unknown[]) => trustListAuditMock(...a),
  trustListPolicies: (...a: unknown[]) => trustListPoliciesMock(...a),
  trustSetPolicy: (...a: unknown[]) => trustSetPolicyMock(...a),
  describeOrchdError: (...a: unknown[]) => describeOrchdErrorMock(...a),
}));

import { InvocationLog } from "./InvocationLog";
import { useAppStore } from "../../store/store";
import type { AuditRow, McpInvocation, McpServer, Policy } from "../../ipc/orchd-types";
import { strings } from "../../strings";

function makeInvocation(over: Partial<McpInvocation> = {}): McpInvocation {
  return {
    id: "inv-1",
    serverId: "s1",
    accountId: null,
    toolName: "search",
    projectId: null,
    requestHash: "deadbeef",
    ok: true,
    errorKind: null,
    latencyMs: 42,
    costUsd: 0.01,
    inputTokens: null,
    outputTokens: null,
    startedAt: 1_720_000_000_000,
    ...over,
  };
}

function makeAuditRow(over: Partial<AuditRow> = {}): AuditRow {
  return {
    id: "audit-1",
    at: 1_720_000_000_000,
    action: "tool_call",
    serverId: "s1",
    toolName: "search",
    projectId: null,
    decision: "allow",
    reason: null,
    invocationId: "inv-1",
    ...over,
  };
}

function makePolicy(over: Partial<Policy> = {}): Policy {
  return {
    id: "policy-1",
    scope: "global",
    refId: null,
    spendCapUsd: 10,
    ratePerMin: 30,
    createdAt: 1,
    updatedAt: 1,
    ...over,
  };
}

function makeServer(over: Partial<McpServer> = {}): McpServer {
  return {
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
    protocolVersion: "2025-11-25",
    createdAt: 1,
    updatedAt: 1,
    ...over,
  };
}

afterEach(cleanup);
beforeEach(() => {
  mcpListInvocationsMock.mockReset().mockResolvedValue([]);
  trustListAuditMock.mockReset().mockResolvedValue([]);
  trustListPoliciesMock.mockReset().mockResolvedValue([]);
  trustSetPolicyMock.mockReset().mockResolvedValue(makePolicy());
  describeOrchdErrorMock.mockReset().mockReturnValue("orchestrator: error");
  useAppStore.setState(
    {
      invocations: [],
      auditRows: [],
      policies: [],
      mcpServers: [],
      orchdDown: false,
    },
    false,
  );
});

describe("InvocationLog", () => {
  it("fetches invocations/audit/policies (refreshInvocations/refreshAuditRows/refreshPolicies) on mount", async () => {
    render(<InvocationLog />);
    await waitFor(() => {
      expect(mcpListInvocationsMock).toHaveBeenCalledWith(null, null, null);
      expect(trustListAuditMock).toHaveBeenCalledWith(null);
      expect(trustListPoliciesMock).toHaveBeenCalledWith();
    });
  });

  it("renders an empty state for invocations/audit/policies", () => {
    render(<InvocationLog />);
    expect(screen.getByTestId("invocations-empty")).toBeTruthy();
    expect(screen.getByTestId("audit-rows-empty")).toBeTruthy();
    expect(screen.getByTestId("policies-empty")).toBeTruthy();
  });

  // ---- invocation table ----

  it("renders an invocation row: server name, tool, ok status, latency, cost, time", () => {
    useAppStore.setState(
      { invocations: [makeInvocation()], mcpServers: [makeServer()] },
      false,
    );
    render(<InvocationLog />);
    const row = screen.getByTestId("invocation-row-inv-1");
    expect(row.textContent).toContain("Prowl");
    expect(row.textContent).toContain("search");
    expect(row.textContent).toContain("42");
    expect(screen.getByTestId("invocation-status-inv-1").textContent).toBe("ok");
    expect(screen.getByTestId("invocation-cost-inv-1").textContent).toBe("0.01");
  });

  it("a NULL cost_usd renders as «—», never blank or 0", () => {
    useAppStore.setState({ invocations: [makeInvocation({ costUsd: null })] }, false);
    render(<InvocationLog />);
    expect(screen.getByTestId("invocation-cost-inv-1").textContent).toBe("—");
  });

  it("a failed invocation renders its error_kind in the status column", () => {
    useAppStore.setState(
      { invocations: [makeInvocation({ ok: false, errorKind: "timeout" })] },
      false,
    );
    render(<InvocationLog />);
    expect(screen.getByTestId("invocation-status-inv-1").textContent).toBe("timeout");
  });

  // ---- audit table ----

  it("renders an audit row: action, decision, reason", () => {
    useAppStore.setState(
      {
        auditRows: [
          makeAuditRow({ action: "policy_deny", decision: "deny", reason: "rate_limit_exceeded" }),
        ],
      },
      false,
    );
    render(<InvocationLog />);
    const row = screen.getByTestId("audit-row-audit-1");
    expect(row.textContent).toContain("policy_deny");
    expect(row.textContent).toContain("rate_limit_exceeded");
    expect(screen.getByTestId("audit-decision-audit-1").textContent).toBe("deny");
  });

  it("an allow row's reason column renders «—»", () => {
    useAppStore.setState({ auditRows: [makeAuditRow({ decision: "allow", reason: null })] }, false);
    render(<InvocationLog />);
    expect(screen.getByTestId("audit-row-audit-1").textContent).toContain("—");
  });

  // ---- policy editor ----

  it("renders the configured policies list", () => {
    useAppStore.setState({ policies: [makePolicy()] }, false);
    render(<InvocationLog />);
    const row = screen.getByTestId("policy-row-policy-1");
    expect(row.textContent).toContain(strings.common.scope.global);
    expect(row.textContent).toContain("10");
    expect(row.textContent).toContain("30");
  });

  it("scope=global disables and blanks the ref-id input; submit does not require it", async () => {
    render(<InvocationLog />);
    expect(screen.getByTestId("policy-ref-id")).toHaveProperty("disabled", true);
    fireEvent.change(screen.getByTestId("policy-spend-cap"), { target: { value: "5" } });
    fireEvent.click(screen.getByTestId("policy-set-submit"));
    await waitFor(() => {
      expect(trustSetPolicyMock).toHaveBeenCalledWith("global", null, 5, null);
    });
  });

  it("scope=project requires a ref-id before submit is enabled", async () => {
    render(<InvocationLog />);
    fireEvent.change(screen.getByTestId("policy-scope"), { target: { value: "project" } });
    expect(screen.getByTestId("policy-set-submit")).toHaveProperty("disabled", true);

    fireEvent.change(screen.getByTestId("policy-ref-id"), { target: { value: "proj-1" } });
    fireEvent.change(screen.getByTestId("policy-rate-per-min"), { target: { value: "10" } });
    fireEvent.click(screen.getByTestId("policy-set-submit"));
    await waitFor(() => {
      expect(trustSetPolicyMock).toHaveBeenCalledWith("project", "proj-1", null, 10);
    });
  });

  it("empty spend/rate inputs submit as null (unlimited)", async () => {
    render(<InvocationLog />);
    fireEvent.click(screen.getByTestId("policy-set-submit"));
    await waitFor(() => {
      expect(trustSetPolicyMock).toHaveBeenCalledWith("global", null, null, null);
    });
  });

  it("a successful trustSetPolicy clears the form and re-fetches policies", async () => {
    render(<InvocationLog />);
    fireEvent.change(screen.getByTestId("policy-spend-cap"), { target: { value: "5" } });
    fireEvent.click(screen.getByTestId("policy-set-submit"));
    await waitFor(() => expect(trustSetPolicyMock).toHaveBeenCalled());
    await waitFor(() => {
      expect(trustListPoliciesMock).toHaveBeenCalledTimes(2); // mount + post-set refresh
    });
    expect((screen.getByTestId("policy-spend-cap") as HTMLInputElement).value).toBe("");
  });

  it("a failed trustSetPolicy shows a toast", async () => {
    trustSetPolicyMock.mockRejectedValue(new Error("boom"));
    render(<InvocationLog />);
    fireEvent.click(screen.getByTestId("policy-set-submit"));
    await waitFor(() => {
      expect(describeOrchdErrorMock).toHaveBeenCalled();
    });
  });

  // ---- honest degradation ----

  it("orchdDown:true disables the policy-set submit, and clicking it never calls trustSetPolicy", async () => {
    render(<InvocationLog />);
    // Populate first so the ONLY disabling factor left is orchdDown (mirrors SkillsTab's pattern).
    fireEvent.change(screen.getByTestId("policy-spend-cap"), { target: { value: "5" } });

    act(() => useAppStore.setState({ orchdDown: true }, false));

    const submit = screen.getByTestId("policy-set-submit");
    expect(submit).toHaveProperty("disabled", true);

    const user = userEvent.setup();
    await user.click(submit);
    expect(trustSetPolicyMock).not.toHaveBeenCalled();
  });
});
