// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, fireEvent, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const mcpSetToolEnabledMock = vi.fn();
const mcpCallToolMock = vi.fn();
const describeOrchdErrorMock = vi.fn((..._a: unknown[]) => "orchestrator: error");
// Faithful reimplementation of the real `isConsentError` (a rejected daemon error whose Debug
// `code` is "Consent") so the consent-recovery path can be exercised against the mocked module.
vi.mock("../../ipc/orchd", () => ({
  mcpSetToolEnabled: (...a: unknown[]) => mcpSetToolEnabledMock(...a),
  mcpCallTool: (...a: unknown[]) => mcpCallToolMock(...a),
  describeOrchdError: (...a: unknown[]) => describeOrchdErrorMock(...a),
  isConsentError: (e: unknown) =>
    (e as { kind?: string; code?: string })?.kind === "daemon" &&
    (e as { code?: string })?.code === "Consent",
}));

import { ToolsBrowser } from "./ToolsBrowser";
import { useAppStore } from "../../store/store";
import type { McpServer, McpTool } from "../../ipc/orchd-types";
import { strings } from "../../strings";

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
    description: "Full-text search",
    inputSchemaJson: '{"type":"object"}',
    enabled: true,
    fetchedAt: 1,
    ...over,
  };
}

afterEach(cleanup);
beforeEach(() => {
  mcpSetToolEnabledMock.mockReset().mockResolvedValue(makeTool());
  mcpCallToolMock.mockReset().mockResolvedValue({
    artifactId: "a1",
    invocationId: "i1",
    contentJson: '{"ok":true}',
    isError: false,
  });
  describeOrchdErrorMock.mockReset().mockReturnValue("orchestrator: error");
  useAppStore.setState(
    {
      mcpServers: [makeServer()],
      mcpToolsByServer: { s1: [makeTool()] },
      mcpArtifacts: [],
      orchdDown: false,
    },
    false,
  );
});

describe("ToolsBrowser", () => {
  it("renders tools across mcpServers/mcpToolsByServer (name, description, server)", () => {
    render(<ToolsBrowser />);
    expect(screen.getByTestId("tool-row-t1")).toBeTruthy();
    expect(screen.getByText("Search")).toBeTruthy();
    expect(screen.getByText("Full-text search")).toBeTruthy();
  });

  it("renders an empty-state message when there are no cached tools", () => {
    useAppStore.setState({ mcpServers: [], mcpToolsByServer: {} }, false);
    render(<ToolsBrowser />);
    expect(screen.getByTestId("tools-empty")).toBeTruthy();
  });

  it("toggling a tool's checkbox calls mcpSetToolEnabled with the flipped flag", async () => {
    render(<ToolsBrowser />);
    fireEvent.click(screen.getByTestId("tool-enabled-t1"));
    await waitFor(() => {
      expect(mcpSetToolEnabledMock).toHaveBeenCalledWith("t1", false);
    });
  });

  it("a failed toggle shows an on-row error signal (J-01), not just a toast; the checkbox stays at the server value", async () => {
    mcpSetToolEnabledMock.mockReset().mockRejectedValueOnce({ kind: "daemon", code: "Io" });
    render(<ToolsBrowser />);
    useAppStore.setState({ toast: null, toastQueue: [] }, false); // FIFO queue: start clean
    const checkbox = screen.getByTestId("tool-enabled-t1") as HTMLInputElement;
    expect(checkbox.checked).toBe(true); // enabled tool

    fireEvent.click(checkbox);
    await waitFor(() => expect(screen.getByTestId("tool-toggle-error-t1")).toBeTruthy());
    expect(screen.getByTestId("tool-toggle-error-t1").textContent).toBe("orchestrator: error");
    // Controlled by `tool.enabled`, no optimistic flip — it stays at the (unchanged) server value.
    expect(checkbox.checked).toBe(true);
    expect(useAppStore.getState().toast).toBe("orchestrator: error");
  });

  it("a Consent-kind call failure appends the recovery hint pointing to Servers → Connect (P-20)", async () => {
    mcpCallToolMock.mockReset().mockRejectedValueOnce({ kind: "daemon", code: "Consent", message: "stale grant" });
    render(<ToolsBrowser />);
    useAppStore.setState({ toast: null, toastQueue: [] }, false); // FIFO queue: start clean
    fireEvent.click(screen.getByTestId("tool-call-t1"));

    await waitFor(() => expect(screen.getByTestId("tool-call-error-t1")).toBeTruthy());
    expect(screen.getByTestId("tool-call-error-t1").textContent).toContain(
      strings.errors.consentRecovery,
    );
    expect(useAppStore.getState().toast).toContain(strings.errors.consentRecovery);
  });

  it("a non-Consent call failure does NOT append the recovery hint", async () => {
    mcpCallToolMock.mockReset().mockRejectedValueOnce({ kind: "daemon", code: "Io" });
    render(<ToolsBrowser />);
    fireEvent.click(screen.getByTestId("tool-call-t1"));

    await waitFor(() => expect(screen.getByTestId("tool-call-error-t1")).toBeTruthy());
    expect(screen.getByTestId("tool-call-error-t1").textContent).not.toContain(
      strings.errors.consentRecovery,
    );
  });

  it("\"invoke\" calls mcpCallTool with the entered JSON args and renders the result with the untrusted banner", async () => {
    render(<ToolsBrowser />);
    fireEvent.change(screen.getByTestId("tool-args-t1"), {
      target: { value: '{"q":"hello"}' },
    });
    fireEvent.click(screen.getByTestId("tool-call-t1"));

    await waitFor(() => {
      expect(mcpCallToolMock).toHaveBeenCalledWith("s1", "search", '{"q":"hello"}', null);
    });

    expect(screen.getByTestId("tool-result-t1")).toBeTruthy();
    expect(screen.getByTestId("tool-result-untrusted-t1").textContent).toContain(
      strings.ext.unverified,
    );
    expect(screen.getByText('{"ok":true}')).toBeTruthy();
  });

  it("an empty args textarea defaults to \"{}\" on call", async () => {
    render(<ToolsBrowser />);
    fireEvent.click(screen.getByTestId("tool-call-t1"));
    await waitFor(() => {
      expect(mcpCallToolMock).toHaveBeenCalledWith("s1", "search", "{}", null);
    });
  });

  it("invalid JSON args shows an inline error and never calls mcpCallTool", () => {
    render(<ToolsBrowser />);
    fireEvent.change(screen.getByTestId("tool-args-t1"), { target: { value: "{not json" } });
    fireEvent.click(screen.getByTestId("tool-call-t1"));
    expect(screen.getByTestId("tool-call-error-t1")).toBeTruthy();
    expect(mcpCallToolMock).not.toHaveBeenCalled();
  });

  it("a disabled tool disables both the args textarea and the call button", () => {
    useAppStore.setState({ mcpToolsByServer: { s1: [makeTool({ enabled: false })] } }, false);
    render(<ToolsBrowser />);
    expect(screen.getByTestId("tool-args-t1")).toHaveProperty("disabled", true);
    expect(screen.getByTestId("tool-call-t1")).toHaveProperty("disabled", true);
  });

  it("orchdDown:true disables the enable toggle and the call button, and neither fires its call", async () => {
    useAppStore.setState({ orchdDown: true }, false);
    render(<ToolsBrowser />);
    expect(screen.getByTestId("tool-enabled-t1")).toHaveProperty("disabled", true);
    expect(screen.getByTestId("tool-call-t1")).toHaveProperty("disabled", true);

    // `fireEvent.click` dispatches a raw click event that bypasses the browser's own
    // disabled-checkbox activation gate (jsdom does not suppress it, unlike a real user click or
    // React's own click-suppression for disabled BUTTONs) — `user-event` is the one that
    // faithfully emulates "a disabled control cannot be interacted with" for a checkbox.
    const user = userEvent.setup();
    await user.click(screen.getByTestId("tool-enabled-t1"));
    expect(mcpSetToolEnabledMock).not.toHaveBeenCalled();

    fireEvent.click(screen.getByTestId("tool-call-t1"));
    expect(mcpCallToolMock).not.toHaveBeenCalled();
  });
});
