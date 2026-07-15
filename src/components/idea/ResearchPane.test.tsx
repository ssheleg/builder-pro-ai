// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, fireEvent, waitFor } from "@testing-library/react";

const mcpGetArtifactMock = vi.fn();
const describeOrchdErrorMock = vi.fn((..._a: unknown[]) => "оркестратор: ошибка");

vi.mock("../../ipc/orchd", () => ({
  mcpGetArtifact: (...a: unknown[]) => mcpGetArtifactMock(...a),
  describeOrchdError: (...a: unknown[]) => describeOrchdErrorMock(...a),
}));

// FormInsightDialog is exercised in its own test file; here it's mocked so ResearchPane's own
// tests only assert THAT it opens with the right props, not its internals.
const formInsightDialogPropsLog: unknown[] = [];
vi.mock("./FormInsightDialog", () => ({
  FormInsightDialog: (props: unknown) => {
    formInsightDialogPropsLog.push(props);
    return <div data-testid="form-insight-dialog-mock" />;
  },
}));

import { ResearchPane } from "./ResearchPane";
import { useAppStore } from "../../store/store";
import type { Idea, McpArtifact, McpServer, ResearchRun } from "../../ipc/orchd-types";

const idea: Idea = {
  id: "idea-1",
  projectId: "p1",
  title: "Проверить спрос",
  body: "",
  lifecycle: "researching",
  createdAt: 1,
  updatedAt: 1,
};

function makeRun(over: Partial<ResearchRun> & { id: string }): ResearchRun {
  return {
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

function makeArtifact(over: Partial<McpArtifact> = {}): McpArtifact {
  return {
    id: "art-1",
    invocationId: "inv-1",
    serverId: "s1",
    accountId: null,
    toolName: "search",
    projectId: null,
    contentJson: '{"ok":true}',
    contentText: "findings here",
    isUntrusted: true,
    createdAt: 1,
    ...over,
  };
}

afterEach(cleanup);

beforeEach(() => {
  mcpGetArtifactMock.mockReset().mockResolvedValue(makeArtifact());
  describeOrchdErrorMock.mockReset().mockReturnValue("оркестратор: ошибка");
  formInsightDialogPropsLog.length = 0;
  useAppStore.setState(
    { researchRunsByIdea: {}, mcpServers: [makeServer()], toast: null, orchdDown: false },
    false,
  );
});

describe("ResearchPane", () => {
  it("renders an empty state when the idea has no research runs", () => {
    render(<ResearchPane idea={idea} disabled={false} />);
    expect(screen.getByTestId("research-pane-empty")).toBeTruthy();
  });

  it("renders a status badge per run", () => {
    useAppStore.setState(
      {
        researchRunsByIdea: {
          [idea.id]: [
            makeRun({ id: "r1", status: "pending" }),
            makeRun({ id: "r2", status: "running" }),
          ],
        },
      },
      false,
    );
    render(<ResearchPane idea={idea} disabled={false} />);
    expect(screen.getByTestId("research-run-status-r1").textContent).toMatch(/ожидан/i);
    expect(screen.getByTestId("research-run-status-r2").textContent).toMatch(/выполня/i);
  });

  it("a done run: clicking «показать артефакт» fetches via mcpGetArtifact and shows the untrusted banner", async () => {
    useAppStore.setState(
      {
        researchRunsByIdea: {
          [idea.id]: [makeRun({ id: "r1", status: "done", artifactId: "art-1" })],
        },
      },
      false,
    );
    render(<ResearchPane idea={idea} disabled={false} />);

    expect(screen.queryByTestId("artifact-untrusted-art-1")).toBeNull();
    fireEvent.click(screen.getByTestId("research-run-show-artifact-r1"));

    await waitFor(() => expect(mcpGetArtifactMock).toHaveBeenCalledWith("art-1"));
    await waitFor(() => {
      expect(screen.getByTestId("artifact-untrusted-art-1").textContent).toContain(
        "непроверенные данные",
      );
    });
  });

  it("a done run: «Сформировать insight» opens FormInsightDialog with the fetched artifact", async () => {
    useAppStore.setState(
      {
        researchRunsByIdea: {
          [idea.id]: [makeRun({ id: "r1", status: "done", artifactId: "art-1" })],
        },
      },
      false,
    );
    render(<ResearchPane idea={idea} disabled={false} />);

    fireEvent.click(screen.getByTestId("research-run-form-insight-r1"));

    await waitFor(() => expect(mcpGetArtifactMock).toHaveBeenCalledWith("art-1"));
    await waitFor(() => expect(screen.getByTestId("form-insight-dialog-mock")).toBeTruthy());
    const lastProps = formInsightDialogPropsLog[formInsightDialogPropsLog.length - 1] as {
      runId: string;
      artifact: McpArtifact | null;
      idea: Idea;
    };
    expect(lastProps.runId).toBe("r1");
    expect(lastProps.artifact).toEqual(makeArtifact());
    expect(lastProps.idea).toEqual(idea);
  });

  it("a failed run: shows error_kind and a «сформировать insight без ресёрча» affordance that opens FormInsightDialog with a null artifact (Q8)", () => {
    useAppStore.setState(
      {
        researchRunsByIdea: {
          [idea.id]: [makeRun({ id: "r1", status: "failed", errorKind: "policy_cap_exceeded" })],
        },
      },
      false,
    );
    render(<ResearchPane idea={idea} disabled={false} />);

    expect(screen.getByTestId("research-run-error-kind-r1").textContent).toContain(
      "policy_cap_exceeded",
    );
    fireEvent.click(screen.getByTestId("research-run-no-research-r1"));

    expect(screen.getByTestId("form-insight-dialog-mock")).toBeTruthy();
    const lastProps = formInsightDialogPropsLog[formInsightDialogPropsLog.length - 1] as {
      runId: string;
      artifact: McpArtifact | null;
    };
    expect(lastProps.runId).toBe("r1");
    expect(lastProps.artifact).toBeNull();
    expect(mcpGetArtifactMock).not.toHaveBeenCalled();
  });

  it("a pending/running run shows no artifact/insight affordances", () => {
    useAppStore.setState(
      { researchRunsByIdea: { [idea.id]: [makeRun({ id: "r1", status: "running" })] } },
      false,
    );
    render(<ResearchPane idea={idea} disabled={false} />);
    expect(screen.queryByTestId("research-run-show-artifact-r1")).toBeNull();
    expect(screen.queryByTestId("research-run-form-insight-r1")).toBeNull();
    expect(screen.queryByTestId("research-run-no-research-r1")).toBeNull();
  });

  it("an mcpGetArtifact failure surfaces via showToast and never renders the viewer", async () => {
    mcpGetArtifactMock.mockRejectedValueOnce({ kind: "daemon", code: "NotFound" });
    useAppStore.setState(
      {
        researchRunsByIdea: {
          [idea.id]: [makeRun({ id: "r1", status: "done", artifactId: "art-1" })],
        },
      },
      false,
    );
    render(<ResearchPane idea={idea} disabled={false} />);

    fireEvent.click(screen.getByTestId("research-run-show-artifact-r1"));

    await waitFor(() => expect(describeOrchdErrorMock).toHaveBeenCalled());
    expect(useAppStore.getState().toast).toBe("оркестратор: ошибка");
    expect(screen.queryByTestId("artifact-row-art-1")).toBeNull();
  });

  it("while disabled (orchdDown): the insight-forming affordances are disabled and never open the dialog on click", () => {
    useAppStore.setState(
      {
        researchRunsByIdea: {
          [idea.id]: [
            makeRun({ id: "r1", status: "done", artifactId: "art-1" }),
            makeRun({ id: "r2", status: "failed", errorKind: "timeout" }),
          ],
        },
      },
      false,
    );
    render(<ResearchPane idea={idea} disabled={true} />);

    const formInsightBtn = screen.getByTestId("research-run-form-insight-r1") as HTMLButtonElement;
    const noResearchBtn = screen.getByTestId("research-run-no-research-r2") as HTMLButtonElement;
    expect(formInsightBtn.disabled).toBe(true);
    expect(noResearchBtn.disabled).toBe(true);

    fireEvent.click(formInsightBtn);
    fireEvent.click(noResearchBtn);
    expect(screen.queryByTestId("form-insight-dialog-mock")).toBeNull();
  });
});
