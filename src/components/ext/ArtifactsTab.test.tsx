// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, fireEvent, waitFor } from "@testing-library/react";

const mcpListArtifactsMock = vi.fn();
vi.mock("../../ipc/orchd", () => ({
  mcpListArtifacts: (...a: unknown[]) => mcpListArtifactsMock(...a),
}));

import { ArtifactsTab } from "./ArtifactsTab";
import { useAppStore } from "../../store/store";
import type { McpArtifact, McpServer } from "../../ipc/orchd-types";
import { strings } from "../../strings";

function makeArtifact(over: Partial<McpArtifact> = {}): McpArtifact {
  return {
    id: "art-1",
    invocationId: "inv-1",
    serverId: "s1",
    accountId: null,
    toolName: "search",
    projectId: null,
    contentJson: '{"ok":true}',
    contentText: "hello world",
    isUntrusted: true,
    createdAt: 1_720_000_000_000,
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
  mcpListArtifactsMock.mockReset().mockResolvedValue([]);
  useAppStore.setState({ mcpArtifacts: [], mcpServers: [] }, false);
});

describe("ArtifactsTab", () => {
  it("fetches artifacts (refreshMcpArtifacts -> mcpListArtifacts) on mount", async () => {
    render(<ArtifactsTab />);
    await waitFor(() => {
      expect(mcpListArtifactsMock).toHaveBeenCalledWith(null, null, null);
    });
  });

  it("renders an empty-state message when there are no artifacts", () => {
    render(<ArtifactsTab />);
    expect(screen.getByTestId("artifacts-empty")).toBeTruthy();
  });

  it("renders an artifact row: tool name, server name, untrusted banner", () => {
    useAppStore.setState({ mcpArtifacts: [makeArtifact()], mcpServers: [makeServer()] }, false);
    render(<ArtifactsTab />);
    const row = screen.getByTestId("artifact-row-art-1");
    expect(screen.getByTestId("artifact-tool-art-1").textContent).toBe("search");
    expect(screen.getByTestId("artifact-source-art-1").textContent).toBe("Prowl");
    expect(row.textContent).not.toContain("undefined");
    expect(screen.getByTestId("artifact-untrusted-art-1").textContent).toContain(
      strings.ext.unverified,
    );
  });

  it("every artifact carries the untrusted banner unconditionally (D9: is_untrusted always true)", () => {
    useAppStore.setState({ mcpArtifacts: [makeArtifact()] }, false);
    render(<ArtifactsTab />);
    expect(screen.getByTestId("artifact-untrusted-art-1")).toBeTruthy();
  });

  it("a connector-sourced artifact (server_id null) shows the account id as source", () => {
    useAppStore.setState(
      {
        mcpArtifacts: [makeArtifact({ serverId: null, accountId: "acct-1" })],
      },
      false,
    );
    render(<ArtifactsTab />);
    expect(screen.getByTestId("artifact-source-art-1").textContent).toBe("acct-1");
  });

  it("content is hidden until \"show content\" is clicked, then shows content_text", () => {
    useAppStore.setState({ mcpArtifacts: [makeArtifact()] }, false);
    render(<ArtifactsTab />);
    expect(screen.queryByTestId("artifact-content-art-1")).toBeNull();

    fireEvent.click(screen.getByTestId("artifact-toggle-art-1"));
    expect(screen.getByTestId("artifact-content-art-1").textContent).toBe("hello world");
  });

  it("falls back to content_json when content_text is null", () => {
    useAppStore.setState(
      { mcpArtifacts: [makeArtifact({ contentText: null, contentJson: '{"raw":1}' })] },
      false,
    );
    render(<ArtifactsTab />);
    fireEvent.click(screen.getByTestId("artifact-toggle-art-1"));
    expect(screen.getByTestId("artifact-content-art-1").textContent).toBe('{"raw":1}');
  });

  it("toggling again hides the content", () => {
    useAppStore.setState({ mcpArtifacts: [makeArtifact()] }, false);
    render(<ArtifactsTab />);
    fireEvent.click(screen.getByTestId("artifact-toggle-art-1"));
    expect(screen.getByTestId("artifact-content-art-1")).toBeTruthy();
    fireEvent.click(screen.getByTestId("artifact-toggle-art-1"));
    expect(screen.queryByTestId("artifact-content-art-1")).toBeNull();
  });
});
