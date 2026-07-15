// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, fireEvent, waitFor, act } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const mcpAddServerMock = vi.fn();
const mcpSetServerEnabledMock = vi.fn();
const mcpDeleteServerMock = vi.fn();
const mcpSetServerBearerMock = vi.fn();
const mcpConnectMock = vi.fn();
const mcpDisconnectMock = vi.fn();
const trustGrantConsentMock = vi.fn();
const describeOrchdErrorMock = vi.fn((..._a: unknown[]) => "оркестратор: ошибка");
vi.mock("../../ipc/orchd", () => ({
  mcpAddServer: (...a: unknown[]) => mcpAddServerMock(...a),
  mcpSetServerEnabled: (...a: unknown[]) => mcpSetServerEnabledMock(...a),
  mcpDeleteServer: (...a: unknown[]) => mcpDeleteServerMock(...a),
  mcpSetServerBearer: (...a: unknown[]) => mcpSetServerBearerMock(...a),
  mcpConnect: (...a: unknown[]) => mcpConnectMock(...a),
  mcpDisconnect: (...a: unknown[]) => mcpDisconnectMock(...a),
  trustGrantConsent: (...a: unknown[]) => trustGrantConsentMock(...a),
  describeOrchdError: (...a: unknown[]) => describeOrchdErrorMock(...a),
}));

import { ServersTab } from "./ServersTab";
import { useAppStore } from "../../store/store";
import type { McpServer } from "../../ipc/orchd-types";

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
    protocolVersion: null,
    createdAt: 1,
    updatedAt: 1,
    ...over,
  };
}

afterEach(cleanup);
beforeEach(() => {
  mcpAddServerMock.mockReset().mockResolvedValue(makeServer());
  mcpSetServerEnabledMock.mockReset().mockResolvedValue(makeServer());
  mcpDeleteServerMock.mockReset().mockResolvedValue(undefined);
  mcpSetServerBearerMock.mockReset().mockResolvedValue(undefined);
  mcpConnectMock.mockReset().mockResolvedValue({ protocolVersion: "2025-11-25", toolCount: 3 });
  mcpDisconnectMock.mockReset().mockResolvedValue(undefined);
  trustGrantConsentMock.mockReset().mockResolvedValue(undefined);
  describeOrchdErrorMock.mockReset().mockReturnValue("оркестратор: ошибка");
  vi.spyOn(window, "confirm").mockReturnValue(true);
  useAppStore.setState(
    {
      mcpServers: [],
      mcpToolsByServer: {},
      mcpArtifacts: [],
      orchdDown: false,
    },
    false,
  );
});

describe("ServersTab", () => {
  it("renders a stubbed server list (name, transport, scope, status)", () => {
    useAppStore.setState({ mcpServers: [makeServer({ name: "Prowl" })] }, false);
    render(<ServersTab />);
    expect(screen.getByTestId("server-row-s1")).toBeTruthy();
    expect(screen.getByTestId("server-name-s1").textContent).toBe("Prowl");
    expect(screen.queryByTestId("servers-empty")).toBeNull();
  });

  it("renders an empty-state message when there are no servers", () => {
    render(<ServersTab />);
    expect(screen.getByTestId("servers-empty")).toBeTruthy();
  });

  it("the add-server form calls mcpAddServer with camelCase args matching T7's param order", async () => {
    render(<ServersTab />);
    fireEvent.change(screen.getByTestId("server-create-name"), { target: { value: "Prowl" } });
    fireEvent.change(screen.getByTestId("server-create-url"), {
      target: { value: "https://prowl.chat/mcp" },
    });
    fireEvent.click(screen.getByTestId("server-create-submit"));

    await waitFor(() => {
      expect(mcpAddServerMock).toHaveBeenCalledWith(
        "Prowl",
        "http",
        "https://prowl.chat/mcp",
        null,
        null,
        null,
        "global",
        null,
        "none",
        null,
        null,
      );
    });
  });

  it("add-server submit stays disabled while the name or url field is empty", () => {
    render(<ServersTab />);
    expect(screen.getByTestId("server-create-submit")).toHaveProperty("disabled", true);
    fireEvent.change(screen.getByTestId("server-create-name"), { target: { value: "Prowl" } });
    expect(screen.getByTestId("server-create-submit")).toHaveProperty("disabled", true);
    fireEvent.change(screen.getByTestId("server-create-url"), {
      target: { value: "https://prowl.chat/mcp" },
    });
    expect(screen.getByTestId("server-create-submit")).toHaveProperty("disabled", false);
  });

  it("clicking «включить/выключить» calls mcpSetServerEnabled with the flipped flag", async () => {
    useAppStore.setState({ mcpServers: [makeServer({ enabled: true })] }, false);
    render(<ServersTab />);
    fireEvent.click(screen.getByTestId("server-toggle-enabled-s1"));
    await waitFor(() => {
      expect(mcpSetServerEnabledMock).toHaveBeenCalledWith("s1", false);
    });
  });

  it("clicking «удалить» confirms then calls mcpDeleteServer", async () => {
    useAppStore.setState({ mcpServers: [makeServer()] }, false);
    render(<ServersTab />);
    fireEvent.click(screen.getByTestId("server-delete-s1"));
    await waitFor(() => {
      expect(mcpDeleteServerMock).toHaveBeenCalledWith("s1");
    });
  });

  it("declining the confirm dialog skips mcpDeleteServer", () => {
    (window.confirm as ReturnType<typeof vi.fn>).mockReturnValue(false);
    useAppStore.setState({ mcpServers: [makeServer()] }, false);
    render(<ServersTab />);
    fireEvent.click(screen.getByTestId("server-delete-s1"));
    expect(mcpDeleteServerMock).not.toHaveBeenCalled();
  });

  it("clicking «отключить» calls mcpDisconnect directly (no consent gate on disconnect)", async () => {
    useAppStore.setState({ mcpServers: [makeServer()] }, false);
    render(<ServersTab />);
    fireEvent.click(screen.getByTestId("server-disconnect-s1"));
    await waitFor(() => {
      expect(mcpDisconnectMock).toHaveBeenCalledWith("s1");
    });
  });

  it("the masked bearer input submits mcpSetServerBearer then clears itself (never echoed back)", async () => {
    useAppStore.setState({ mcpServers: [makeServer()] }, false);
    render(<ServersTab />);
    const input = screen.getByTestId("server-bearer-input-s1") as HTMLInputElement;
    expect(input.type).toBe("password");
    fireEvent.change(input, { target: { value: "sekret-token" } });
    fireEvent.click(screen.getByTestId("server-bearer-submit-s1"));
    await waitFor(() => {
      expect(mcpSetServerBearerMock).toHaveBeenCalledWith("s1", "sekret-token");
    });
    await waitFor(() => {
      expect((screen.getByTestId("server-bearer-input-s1") as HTMLInputElement).value).toBe("");
    });
  });

  // ---- connect + consent flow ----

  it("connect on a not-yet-consented server shows ConnectDialog; confirming grants consent then connects", async () => {
    useAppStore.setState({ mcpServers: [makeServer()] }, false);
    render(<ServersTab />);

    expect(screen.queryByTestId("connect-dialog")).toBeNull();
    fireEvent.click(screen.getByTestId("server-connect-s1"));
    expect(screen.getByTestId("connect-dialog")).toBeTruthy();
    expect(screen.getByTestId("connect-dialog-url").textContent).toBe("https://prowl.chat/mcp");

    fireEvent.click(screen.getByTestId("connect-dialog-confirm"));

    await waitFor(() => {
      expect(trustGrantConsentMock).toHaveBeenCalledWith("s1", "connect");
      expect(mcpConnectMock).toHaveBeenCalledWith("s1");
    });
    // trustGrantConsent must happen before mcpConnect (mcpConnect is trust-gated, D10).
    const grantOrder = trustGrantConsentMock.mock.invocationCallOrder[0];
    const connectOrder = mcpConnectMock.mock.invocationCallOrder[0];
    expect(grantOrder).toBeLessThan(connectOrder);

    await waitFor(() => {
      expect(screen.queryByTestId("connect-dialog")).toBeNull();
    });
  });

  it("Отмена in ConnectDialog closes it without calling trustGrantConsent/mcpConnect", () => {
    useAppStore.setState({ mcpServers: [makeServer()] }, false);
    render(<ServersTab />);
    fireEvent.click(screen.getByTestId("server-connect-s1"));
    fireEvent.click(screen.getByTestId("connect-dialog-cancel"));
    expect(screen.queryByTestId("connect-dialog")).toBeNull();
    expect(trustGrantConsentMock).not.toHaveBeenCalled();
    expect(mcpConnectMock).not.toHaveBeenCalled();
  });

  it("a consent/connect failure shows an in-dialog error and keeps the dialog open", async () => {
    trustGrantConsentMock.mockRejectedValueOnce({ kind: "daemon", code: "Consent", message: "denied" });
    describeOrchdErrorMock.mockReturnValue("требуется согласие: denied");
    useAppStore.setState({ mcpServers: [makeServer()] }, false);
    render(<ServersTab />);
    fireEvent.click(screen.getByTestId("server-connect-s1"));
    fireEvent.click(screen.getByTestId("connect-dialog-confirm"));

    await waitFor(() => {
      expect(screen.getByTestId("connect-dialog-error").textContent).toBe("требуется согласие: denied");
    });
    expect(screen.getByTestId("connect-dialog")).toBeTruthy();
    expect(mcpConnectMock).not.toHaveBeenCalled();
  });

  // ---- honest degradation ----

  it("orchdDown:true disables ALL SIX mutating controls, and clicking each never calls its wrapper (spec §8, TasksList precedent)", async () => {
    // Render with orchdDown:false FIRST so the fields whose OTHER disable-condition would keep a
    // control disabled regardless of orchdDown can be populated — add-submit needs name+url, and
    // bearer-submit needs a non-empty token whose own input is itself disabled once orchdDown.
    // Then flip orchdDown:true, so every `disabled` below is provably owed to orchdDown ALONE
    // rather than a co-condition (mirrors TasksList.test.tsx's "fill the title first" pattern).
    useAppStore.setState({ mcpServers: [makeServer()], orchdDown: false }, false);
    render(<ServersTab />);

    fireEvent.change(screen.getByTestId("server-create-name"), { target: { value: "Prowl" } });
    fireEvent.change(screen.getByTestId("server-create-url"), {
      target: { value: "https://prowl.chat/mcp" },
    });
    fireEvent.change(screen.getByTestId("server-bearer-input-s1"), {
      target: { value: "sekret-token" },
    });

    act(() => useAppStore.setState({ orchdDown: true }, false));

    const controls = [
      screen.getByTestId("server-create-submit"),
      screen.getByTestId("server-connect-s1"),
      screen.getByTestId("server-toggle-enabled-s1"),
      screen.getByTestId("server-disconnect-s1"),
      screen.getByTestId("server-delete-s1"),
      screen.getByTestId("server-bearer-submit-s1"),
    ];
    for (const c of controls) expect(c).toHaveProperty("disabled", true);
    // the bearer INPUT is disabled too (so its token can no longer be edited while down)
    expect(screen.getByTestId("server-bearer-input-s1")).toHaveProperty("disabled", true);

    // Clicking a disabled control must be an inert no-op. `window.confirm` is stubbed to `true`
    // up front (beforeEach) so that IF the delete handler wrongly fired, it would proceed to the
    // wrapper — making a "not called" assertion meaningful rather than blocked by a false confirm.
    const user = userEvent.setup();
    for (const c of controls) {
      // `user.click` faithfully emulates a real user click, which the browser suppresses on a
      // disabled control — the honest emulation of "cannot interact". (Plain `fireEvent.click`
      // dispatches a raw event that jsdom does not gate on `disabled` for every element type.)
      await user.click(c);
    }

    expect(mcpAddServerMock).not.toHaveBeenCalled();
    expect(mcpConnectMock).not.toHaveBeenCalled();
    expect(screen.queryByTestId("connect-dialog")).toBeNull();
    expect(mcpSetServerEnabledMock).not.toHaveBeenCalled();
    expect(mcpDisconnectMock).not.toHaveBeenCalled();
    expect(mcpDeleteServerMock).not.toHaveBeenCalled();
    expect(mcpSetServerBearerMock).not.toHaveBeenCalled();
  });
});
