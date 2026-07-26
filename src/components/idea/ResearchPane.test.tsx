// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, fireEvent, waitFor, act } from "@testing-library/react";

const mcpGetArtifactMock = vi.fn();
const describeOrchdErrorMock = vi.fn((..._a: unknown[]) => "orchestrator: error");
// The self-poll (spec D8, BL-92) drives the store's real `refreshResearchRuns`, which calls this
// wrapper — mocked here so the poll resolves deterministically under fake timers.
const researchListRunsMock = vi.fn();

vi.mock("../../ipc/orchd", () => ({
  mcpGetArtifact: (...a: unknown[]) => mcpGetArtifactMock(...a),
  researchListRuns: (...a: unknown[]) => researchListRunsMock(...a),
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
import { strings } from "../../strings";
import type { Idea, McpArtifact, McpServer, ResearchRun } from "../../ipc/orchd-types";

const idea: Idea = {
  id: "idea-1",
  projectId: "p1",
  title: "Validate demand",
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
  describeOrchdErrorMock.mockReset().mockReturnValue("orchestrator: error");
  researchListRunsMock.mockReset().mockResolvedValue([]);
  formInsightDialogPropsLog.length = 0;
  useAppStore.setState(
    {
      researchRunsByIdea: {},
      // `researchRunsFetched` pre-set (UX-1): these tests exercise the post-first-fetch render
      // paths — with the flag unset an empty run list now shows the loading placeholder instead
      // of the empty state.
      researchRunsFetched: { [idea.id]: true },
      mcpServers: [makeServer()],
      toast: null,
      toastQueue: [],
      orchdDown: false,
    },
    false,
  );
});

describe("ResearchPane", () => {
  it("renders an empty state when the idea has no research runs", () => {
    render(<ResearchPane idea={idea} disabled={false} />);
    expect(screen.getByTestId("research-pane-empty")).toBeTruthy();
  });

  it("UX-1: until the first fetch settles, the loading placeholder shows — never the false empty state", async () => {
    // `researchRunsFetched[idea.id]` unset (the beforeEach pre-sets it for the post-fetch paths;
    // here we want the pre-settle window) + an empty cache — exactly the window in which the
    // pre-fix component flashed the false empty state at a user whose idea HAS runs.
    useAppStore.setState({ researchRunsByIdea: {}, researchRunsFetched: {} }, false);

    render(<ResearchPane idea={idea} disabled={false} />);

    expect(screen.getByTestId("research-pane-loading").textContent).toBe(
      strings.research.loadingRuns,
    );
    expect(screen.queryByTestId("research-pane-empty")).toBeNull(); // no false empty flash

    // Flag set + still-empty data → the honest empty state shows, the loading row is gone.
    await act(async () => {
      useAppStore.setState({ researchRunsFetched: { [idea.id]: true } }, false);
    });
    expect(screen.queryByTestId("research-pane-loading")).toBeNull();
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
    expect(screen.getByTestId("research-run-status-r1").textContent).toMatch(/pending/i);
    expect(screen.getByTestId("research-run-status-r2").textContent).toMatch(/running/i);
  });

  it('a done run: clicking "show artifact" fetches via mcpGetArtifact and shows the untrusted banner', async () => {
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
        "unverified data",
      );
    });
  });

  it('a done run: "Form insight" opens FormInsightDialog with the fetched artifact', async () => {
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

  it('a failed run: shows error_kind and a "form insight without research" affordance that opens FormInsightDialog with a null artifact (Q8)', () => {
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
    expect(useAppStore.getState().toast).toBe("orchestrator: error");
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

  // ── self-poll (spec D8, BL-92): stuck-run self-heal without a wire push ──────────────────────

  it("polls researchListRuns every 2s while a run is non-terminal, and stops once all runs are terminal", async () => {
    vi.useFakeTimers();
    try {
      researchListRunsMock.mockResolvedValue([makeRun({ id: "r1", status: "running" })]);
      useAppStore.setState(
        { researchRunsByIdea: { [idea.id]: [makeRun({ id: "r1", status: "running" })] } },
        false,
      );
      render(<ResearchPane idea={idea} disabled={false} />);
      // No immediate poll on mount — only on the 2s cadence.
      expect(researchListRunsMock).not.toHaveBeenCalled();

      await act(async () => {
        await vi.advanceTimersByTimeAsync(2000);
      });
      expect(researchListRunsMock).toHaveBeenCalledTimes(1);
      expect(researchListRunsMock).toHaveBeenCalledWith(idea.id);

      await act(async () => {
        await vi.advanceTimersByTimeAsync(2000);
      });
      expect(researchListRunsMock).toHaveBeenCalledTimes(2);

      // The next poll returns a TERMINAL run -> the store flips to done -> polling must stop.
      researchListRunsMock.mockResolvedValue([
        makeRun({ id: "r1", status: "done", artifactId: "art-1" }),
      ]);
      await act(async () => {
        await vi.advanceTimersByTimeAsync(2000);
      });
      expect(researchListRunsMock).toHaveBeenCalledTimes(3);

      // Well past several more intervals: no further polls once terminal.
      await act(async () => {
        await vi.advanceTimersByTimeAsync(6000);
      });
      expect(researchListRunsMock).toHaveBeenCalledTimes(3);
    } finally {
      vi.useRealTimers();
    }
  });

  it("does not poll when the only run is already terminal", async () => {
    vi.useFakeTimers();
    try {
      useAppStore.setState(
        {
          researchRunsByIdea: {
            [idea.id]: [makeRun({ id: "r1", status: "done", artifactId: "art-1" })],
          },
        },
        false,
      );
      render(<ResearchPane idea={idea} disabled={false} />);
      await act(async () => {
        await vi.advanceTimersByTimeAsync(6000);
      });
      expect(researchListRunsMock).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });

  it("does not poll while orchd is down (disabled) — no toast-spam against a known-down daemon", async () => {
    vi.useFakeTimers();
    try {
      useAppStore.setState(
        { researchRunsByIdea: { [idea.id]: [makeRun({ id: "r1", status: "running" })] } },
        false,
      );
      render(<ResearchPane idea={idea} disabled={true} />);
      await act(async () => {
        await vi.advanceTimersByTimeAsync(6000);
      });
      expect(researchListRunsMock).not.toHaveBeenCalled();
    } finally {
      vi.useRealTimers();
    }
  });
});
