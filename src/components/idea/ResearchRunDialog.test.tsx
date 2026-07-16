// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, fireEvent, waitFor, act } from "@testing-library/react";

const researchStartRunMock = vi.fn();
const mcpListToolsMock = vi.fn();
const trustListPoliciesMock = vi.fn();
const describeOrchdErrorMock = vi.fn((..._a: unknown[]) => "orchestrator: error");

vi.mock("../../ipc/orchd", () => ({
  researchStartRun: (...a: unknown[]) => researchStartRunMock(...a),
  mcpListTools: (...a: unknown[]) => mcpListToolsMock(...a),
  trustListPolicies: (...a: unknown[]) => trustListPoliciesMock(...a),
  describeOrchdError: (...a: unknown[]) => describeOrchdErrorMock(...a),
}));

import { ResearchRunDialog } from "./ResearchRunDialog";
import { useAppStore } from "../../store/store";
import type { Idea, McpServer, McpTool, Policy } from "../../ipc/orchd-types";

const idea: Idea = {
  id: "idea-1",
  projectId: "p1",
  title: "Validate demand",
  body: "need to understand market size",
  lifecycle: "captured",
  createdAt: 1,
  updatedAt: 1,
};

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

function makeTool(over: Partial<McpTool> = {}): McpTool {
  return {
    id: "t1",
    serverId: "s1",
    name: "search",
    title: "Search",
    description: null,
    inputSchemaJson: "{}",
    enabled: true,
    fetchedAt: 1,
    ...over,
  };
}

function makePolicy(over: Partial<Policy> = {}): Policy {
  return {
    id: "pol1",
    scope: "global",
    refId: null,
    spendCapUsd: 5,
    ratePerMin: 10,
    createdAt: 1,
    updatedAt: 1,
    ...over,
  };
}

afterEach(cleanup);

beforeEach(() => {
  researchStartRunMock.mockReset().mockResolvedValue({
    id: "r1",
    ideaId: idea.id,
    serverId: "s1",
    toolName: "search",
    argsJson: "{}",
    status: "pending",
    invocationId: null,
    artifactId: null,
    errorKind: null,
    createdAt: 1,
    updatedAt: 1,
  });
  mcpListToolsMock.mockReset().mockResolvedValue([makeTool()]);
  trustListPoliciesMock.mockReset().mockResolvedValue([makePolicy()]);
  describeOrchdErrorMock.mockReset().mockReturnValue("orchestrator: error");
  useAppStore.setState(
    {
      mcpServers: [makeServer()],
      mcpToolsByServer: {},
      policies: [],
      orchdDown: false,
      toast: null, toastQueue: [],
      researchRunsByIdea: {},
    },
    false,
  );
});

describe("ResearchRunDialog", () => {
  it("only offers connected+enabled servers in the server select", () => {
    const disconnected = makeServer({ id: "s2", name: "Disconnected", protocolVersion: null });
    const disabled = makeServer({ id: "s3", name: "Disabled", enabled: false });
    useAppStore.setState({ mcpServers: [makeServer(), disconnected, disabled] }, false);

    render(<ResearchRunDialog idea={idea} onClose={() => {}} />);

    const select = screen.getByTestId("research-run-server-select") as HTMLSelectElement;
    const optionValues = Array.from(select.options).map((o) => o.value);
    expect(optionValues).toContain("s1");
    expect(optionValues).not.toContain("s2");
    expect(optionValues).not.toContain("s3");
  });

  it("picking a server fetches its tools (mcpListTools) and populates the tool select", async () => {
    render(<ResearchRunDialog idea={idea} onClose={() => {}} />);

    fireEvent.change(screen.getByTestId("research-run-server-select"), {
      target: { value: "s1" },
    });

    await waitFor(() => expect(mcpListToolsMock).toHaveBeenCalledWith("s1"));
    await waitFor(() => {
      const toolSelect = screen.getByTestId("research-run-tool-select") as HTMLSelectElement;
      const values = Array.from(toolSelect.options).map((o) => o.value);
      expect(values).toContain("search");
    });
  });

  it("seeds the args textarea from the idea's title/body as JSON", () => {
    render(<ResearchRunDialog idea={idea} onClose={() => {}} />);
    const textarea = screen.getByTestId("research-run-args") as HTMLTextAreaElement;
    const parsed = JSON.parse(textarea.value);
    expect(parsed.query).toBe(idea.title);
    expect(parsed.context).toBe(idea.body);
  });

  it("the spend-preflight fetches and shows the effective trustListPolicies for the picked server's scope", async () => {
    trustListPoliciesMock.mockResolvedValue([
      makePolicy({ id: "server-pol", scope: "server", refId: "s1", spendCapUsd: 2, ratePerMin: 3 }),
      makePolicy({ id: "global-pol", scope: "global", refId: null, spendCapUsd: 100, ratePerMin: 100 }),
    ]);
    render(<ResearchRunDialog idea={idea} onClose={() => {}} />);

    await waitFor(() => expect(trustListPoliciesMock).toHaveBeenCalled());

    fireEvent.change(screen.getByTestId("research-run-server-select"), {
      target: { value: "s1" },
    });

    // Most-specific-wins: the server-scoped policy (2) must win over the global one (100).
    await waitFor(() => {
      expect(screen.getByTestId("research-run-policy-spend-cap").textContent).toContain("2");
    });
    expect(screen.getByTestId("research-run-policy-note").textContent).toMatch(
      /cost.*unknown/i,
    );
  });

  it('shows an honest "not set" note when no policy applies to the scope', async () => {
    trustListPoliciesMock.mockResolvedValue([]);
    render(<ResearchRunDialog idea={idea} onClose={() => {}} />);
    fireEvent.change(screen.getByTestId("research-run-server-select"), {
      target: { value: "s1" },
    });
    await waitFor(() => {
      expect(screen.getByTestId("research-run-policy-spend-cap").textContent).toContain(
        "not set",
      );
    });
  });

  it('"Run" fires researchStartRun with the picked server/tool/args and refreshes runs', async () => {
    const onClose = vi.fn();
    render(<ResearchRunDialog idea={idea} onClose={onClose} />);

    fireEvent.change(screen.getByTestId("research-run-server-select"), {
      target: { value: "s1" },
    });
    await waitFor(() => expect(mcpListToolsMock).toHaveBeenCalled());
    fireEvent.change(screen.getByTestId("research-run-tool-select"), {
      target: { value: "search" },
    });
    fireEvent.change(screen.getByTestId("research-run-args"), {
      target: { value: '{"query":"custom"}' },
    });

    fireEvent.click(screen.getByTestId("research-run-submit"));

    await waitFor(() =>
      expect(researchStartRunMock).toHaveBeenCalledWith(
        idea.id,
        "s1",
        "search",
        '{"query":"custom"}',
      ),
    );
    await waitFor(() => expect(onClose).toHaveBeenCalled());
  });

  it("two rapid Run clicks start the run ONCE (double-submit guard, spec D6 / F-08)", async () => {
    let resolveRun!: (v: unknown) => void;
    researchStartRunMock.mockReset().mockImplementation(
      () => new Promise((res) => (resolveRun = res)),
    );
    render(<ResearchRunDialog idea={idea} onClose={() => {}} />);
    fireEvent.change(screen.getByTestId("research-run-server-select"), { target: { value: "s1" } });
    await waitFor(() => expect(mcpListToolsMock).toHaveBeenCalled());
    fireEvent.change(screen.getByTestId("research-run-tool-select"), { target: { value: "search" } });

    const submit = screen.getByTestId("research-run-submit");
    fireEvent.click(submit);
    fireEvent.click(submit);

    expect(researchStartRunMock).toHaveBeenCalledTimes(1);
    await act(async () => {
      resolveRun({ id: "run1", status: "pending" });
    });
  });

  it("the submit button is disabled until BOTH a server and a tool are picked", async () => {
    render(<ResearchRunDialog idea={idea} onClose={() => {}} />);
    const submit = screen.getByTestId("research-run-submit") as HTMLButtonElement;
    expect(submit.disabled).toBe(true);

    fireEvent.change(screen.getByTestId("research-run-server-select"), {
      target: { value: "s1" },
    });
    expect(submit.disabled).toBe(true);

    await waitFor(() => expect(mcpListToolsMock).toHaveBeenCalled());
    fireEvent.change(screen.getByTestId("research-run-tool-select"), {
      target: { value: "search" },
    });
    expect(submit.disabled).toBe(false);
  });

  it("invalid JSON args blocks submit with an inline error and never calls researchStartRun", async () => {
    render(<ResearchRunDialog idea={idea} onClose={() => {}} />);
    fireEvent.change(screen.getByTestId("research-run-server-select"), {
      target: { value: "s1" },
    });
    await waitFor(() => expect(mcpListToolsMock).toHaveBeenCalled());
    fireEvent.change(screen.getByTestId("research-run-tool-select"), {
      target: { value: "search" },
    });
    fireEvent.change(screen.getByTestId("research-run-args"), {
      target: { value: "{not json" },
    });

    fireEvent.click(screen.getByTestId("research-run-submit"));

    expect(screen.getByTestId("research-run-args-error")).toBeTruthy();
    expect(researchStartRunMock).not.toHaveBeenCalled();
  });

  it("a failed start shows the mapped error inline and does not close", async () => {
    researchStartRunMock.mockRejectedValue({ kind: "daemon", code: "Policy", message: "cap" });
    const onClose = vi.fn();
    render(<ResearchRunDialog idea={idea} onClose={onClose} />);
    fireEvent.change(screen.getByTestId("research-run-server-select"), {
      target: { value: "s1" },
    });
    await waitFor(() => expect(mcpListToolsMock).toHaveBeenCalled());
    fireEvent.change(screen.getByTestId("research-run-tool-select"), {
      target: { value: "search" },
    });
    fireEvent.click(screen.getByTestId("research-run-submit"));

    await waitFor(() => expect(screen.getByTestId("research-run-error")).toBeTruthy());
    expect(onClose).not.toHaveBeenCalled();
  });

  it("cancel closes the dialog without starting a run", () => {
    const onClose = vi.fn();
    render(<ResearchRunDialog idea={idea} onClose={onClose} />);
    fireEvent.click(screen.getByTestId("research-run-cancel"));
    expect(onClose).toHaveBeenCalled();
    expect(researchStartRunMock).not.toHaveBeenCalled();
  });

  it("while orchdDown: the submit control is disabled and clicking it never calls researchStartRun", async () => {
    useAppStore.setState({ orchdDown: true }, false);
    render(<ResearchRunDialog idea={idea} onClose={() => {}} />);

    fireEvent.change(screen.getByTestId("research-run-server-select"), {
      target: { value: "s1" },
    });
    await waitFor(() => expect(mcpListToolsMock).toHaveBeenCalled());
    fireEvent.change(screen.getByTestId("research-run-tool-select"), {
      target: { value: "search" },
    });

    const submit = screen.getByTestId("research-run-submit") as HTMLButtonElement;
    expect(submit.disabled).toBe(true);
    fireEvent.click(submit);
    expect(researchStartRunMock).not.toHaveBeenCalled();
  });
});
