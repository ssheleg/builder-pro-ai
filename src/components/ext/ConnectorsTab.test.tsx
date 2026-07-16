// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, fireEvent, waitFor, act } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

const connectorBeginOAuthMock = vi.fn();
const connectorCompleteOAuthMock = vi.fn();
const connectorAddApiKeyMock = vi.fn();
const connectorDeleteAccountMock = vi.fn();
const connectorListOpsMock = vi.fn();
const connectorListProvidersMock = vi.fn();
const connectorInvokeMock = vi.fn();
// `refreshAccounts` (store.ts) calls `connectorListAccounts` straight through — mocked here too
// (same module) so ConnectorsTab's mount-time fetch resolves deterministically, mirroring
// ServersTab.test.tsx's relationship with `mcpListServers`/`refreshMcpServers`.
const connectorListAccountsMock = vi.fn();
const describeOrchdErrorMock = vi.fn((..._a: unknown[]) => "orchestrator: error");
vi.mock("../../ipc/orchd", () => ({
  connectorBeginOAuth: (...a: unknown[]) => connectorBeginOAuthMock(...a),
  connectorCompleteOAuth: (...a: unknown[]) => connectorCompleteOAuthMock(...a),
  connectorAddApiKey: (...a: unknown[]) => connectorAddApiKeyMock(...a),
  connectorListAccounts: (...a: unknown[]) => connectorListAccountsMock(...a),
  connectorDeleteAccount: (...a: unknown[]) => connectorDeleteAccountMock(...a),
  connectorListOps: (...a: unknown[]) => connectorListOpsMock(...a),
  connectorListProviders: (...a: unknown[]) => connectorListProvidersMock(...a),
  connectorInvoke: (...a: unknown[]) => connectorInvokeMock(...a),
  describeOrchdError: (...a: unknown[]) => describeOrchdErrorMock(...a),
  // Faithful reimplementation of the real `isConsentError` (daemon error, Debug `code` "Consent").
  isConsentError: (e: unknown) =>
    (e as { kind?: string; code?: string })?.kind === "daemon" &&
    (e as { code?: string })?.code === "Consent",
}));

const openUrlMock = vi.fn().mockResolvedValue(undefined);
vi.mock("@tauri-apps/plugin-shell", () => ({
  open: (...a: unknown[]) => openUrlMock(...a),
}));

import { ConnectorsTab } from "./ConnectorsTab";
import { useAppStore } from "../../store/store";
import type { Account } from "../../ipc/orchd-types";
import { strings } from "../../strings";

function makeAccount(over: Partial<Account> = {}): Account {
  return {
    id: "a1",
    provider: "generic-rest",
    label: "My API",
    authKind: "apikey",
    scopes: [],
    expiresAt: null,
    createdAt: 1,
    updatedAt: 1,
    ...over,
  };
}

afterEach(cleanup);
beforeEach(() => {
  connectorBeginOAuthMock.mockReset();
  connectorCompleteOAuthMock.mockReset();
  connectorAddApiKeyMock.mockReset().mockResolvedValue(makeAccount());
  connectorListAccountsMock.mockReset().mockResolvedValue([]);
  connectorDeleteAccountMock.mockReset().mockResolvedValue(undefined);
  connectorListOpsMock.mockReset().mockResolvedValue([]);
  // Default: one configured OAuth provider so the general OAuth-flow tests can select it. Tests
  // asserting the empty-state override this with `mockResolvedValue([])`.
  connectorListProvidersMock.mockReset().mockResolvedValue(["github"]);
  connectorInvokeMock.mockReset();
  describeOrchdErrorMock.mockReset().mockReturnValue("orchestrator: error");
  openUrlMock.mockReset().mockResolvedValue(undefined);
  vi.spyOn(window, "confirm").mockReturnValue(true);
  useAppStore.setState({ accounts: [], orchdDown: false }, false);
});

describe("ConnectorsTab", () => {
  it("fetches accounts (refreshAccounts) on mount", async () => {
    render(<ConnectorsTab />);
    await waitFor(() => {
      expect(connectorListAccountsMock).toHaveBeenCalledWith();
    });
  });

  it("renders a stubbed accounts list (provider, label, authKind, scopes, expiry)", () => {
    useAppStore.setState(
      {
        accounts: [
          makeAccount({
            id: "a1",
            provider: "github",
            label: "My GitHub",
            authKind: "oauth",
            scopes: ["repo", "user"],
            expiresAt: null,
          }),
        ],
      },
      false,
    );
    render(<ConnectorsTab />);
    expect(screen.getByTestId("account-row-a1")).toBeTruthy();
    expect(screen.getByTestId("account-provider-a1").textContent).toBe("github");
    expect(screen.getByTestId("account-label-a1").textContent).toBe("My GitHub");
    expect(screen.getByTestId("account-authkind-a1").textContent).toBe("OAuth");
    expect(screen.getByTestId("account-scopes-a1").textContent).toContain("repo, user");
    expect(screen.getByTestId("account-expiry-a1").textContent).toContain("—");
    expect(screen.queryByTestId("accounts-empty")).toBeNull();
  });

  it("renders an empty-state message when there are no accounts", () => {
    render(<ConnectorsTab />);
    expect(screen.getByTestId("accounts-empty")).toBeTruthy();
  });

  // ---- add-api-key form ----

  it("the add-api-key form calls connectorAddApiKey with camelCase args; the key input is masked and cleared after submit", async () => {
    render(<ConnectorsTab />);
    const keyInput = screen.getByTestId("apikey-key") as HTMLInputElement;
    expect(keyInput.type).toBe("password");

    fireEvent.change(screen.getByTestId("apikey-provider"), { target: { value: "generic-rest" } });
    fireEvent.change(screen.getByTestId("apikey-label"), { target: { value: "My API" } });
    fireEvent.change(keyInput, { target: { value: "sekret-key" } });
    fireEvent.click(screen.getByTestId("apikey-submit"));

    await waitFor(() => {
      expect(connectorAddApiKeyMock).toHaveBeenCalledWith({
        provider: "generic-rest",
        label: "My API",
        apiKey: "sekret-key",
      });
    });
    // Never echoed back — the key input clears itself on submit.
    await waitFor(() => {
      expect((screen.getByTestId("apikey-key") as HTMLInputElement).value).toBe("");
    });
  });

  it("two rapid '+ API key' clicks add ONCE (double-submit guard, spec D6 / J-03)", async () => {
    let resolveAdd!: (v: unknown) => void;
    connectorAddApiKeyMock.mockReset().mockImplementation(
      () => new Promise((res) => (resolveAdd = res)),
    );
    render(<ConnectorsTab />);
    fireEvent.change(screen.getByTestId("apikey-provider"), { target: { value: "generic-rest" } });
    fireEvent.change(screen.getByTestId("apikey-label"), { target: { value: "My API" } });
    fireEvent.change(screen.getByTestId("apikey-key"), { target: { value: "sekret-key" } });

    const submit = screen.getByTestId("apikey-submit");
    fireEvent.click(submit);
    fireEvent.click(submit);

    expect(connectorAddApiKeyMock).toHaveBeenCalledTimes(1);
    await act(async () => {
      resolveAdd({ id: "acc9" });
    });
  });

  it("add-api-key submit stays disabled while any field is empty", () => {
    render(<ConnectorsTab />);
    expect(screen.getByTestId("apikey-submit")).toHaveProperty("disabled", true);
    fireEvent.change(screen.getByTestId("apikey-provider"), { target: { value: "generic-rest" } });
    expect(screen.getByTestId("apikey-submit")).toHaveProperty("disabled", true);
    fireEvent.change(screen.getByTestId("apikey-label"), { target: { value: "My API" } });
    expect(screen.getByTestId("apikey-submit")).toHaveProperty("disabled", true);
    fireEvent.change(screen.getByTestId("apikey-key"), { target: { value: "sekret" } });
    expect(screen.getByTestId("apikey-submit")).toHaveProperty("disabled", false);
  });

  // ---- delete ----

  it("clicking delete confirms then calls connectorDeleteAccount", async () => {
    connectorListAccountsMock.mockResolvedValue([makeAccount()]);
    useAppStore.setState({ accounts: [makeAccount()] }, false);
    render(<ConnectorsTab />);
    fireEvent.click(screen.getByTestId("account-delete-a1"));
    await waitFor(() => {
      expect(connectorDeleteAccountMock).toHaveBeenCalledWith({ id: "a1" });
    });
  });

  it("declining the confirm dialog skips connectorDeleteAccount", () => {
    (window.confirm as ReturnType<typeof vi.fn>).mockReturnValue(false);
    connectorListAccountsMock.mockResolvedValue([makeAccount()]);
    useAppStore.setState({ accounts: [makeAccount()] }, false);
    render(<ConnectorsTab />);
    fireEvent.click(screen.getByTestId("account-delete-a1"));
    expect(connectorDeleteAccountMock).not.toHaveBeenCalled();
  });

  // ---- OAuth begin -> paste-code -> complete ----

  it("\"Connect OAuth\": begin opens the authorize URL and shows a paste-code field; completing calls connectorCompleteOAuth", async () => {
    connectorBeginOAuthMock.mockResolvedValue({
      authorizeUrl: "https://example.com/authorize?x=1",
      state: "st-1",
    });
    connectorCompleteOAuthMock.mockResolvedValue(makeAccount({ id: "a2", authKind: "oauth" }));
    render(<ConnectorsTab />);

    // The provider dropdown is fed by connectorListProviders(); wait for it to populate + enable.
    await waitFor(() => {
      expect((screen.getByTestId("oauth-provider") as HTMLSelectElement).disabled).toBe(false);
    });
    fireEvent.change(screen.getByTestId("oauth-provider"), { target: { value: "github" } });
    fireEvent.change(screen.getByTestId("oauth-label"), { target: { value: "My GitHub" } });
    fireEvent.click(screen.getByTestId("oauth-begin-submit"));

    await waitFor(() => {
      expect(connectorBeginOAuthMock).toHaveBeenCalledWith({
        provider: "github",
        label: "My GitHub",
        scopes: undefined,
      });
    });

    await waitFor(() => {
      expect(screen.getByTestId("oauth-authorize-link")).toBeTruthy();
    });
    // Auto-opened via the app's existing open-url mechanism (@tauri-apps/plugin-shell's `open`,
    // the same one terminal-manager.ts's OSC-8 linkHandler uses).
    expect(openUrlMock).toHaveBeenCalledWith("https://example.com/authorize?x=1");
    expect(screen.getByTestId("oauth-authorize-link")).toHaveProperty(
      "href",
      "https://example.com/authorize?x=1",
    );

    fireEvent.change(screen.getByTestId("oauth-code-input"), { target: { value: "code-xyz" } });
    fireEvent.click(screen.getByTestId("oauth-complete-submit"));

    await waitFor(() => {
      expect(connectorCompleteOAuthMock).toHaveBeenCalledWith({ state: "st-1", code: "code-xyz" });
    });
  });

  it("begin-OAuth submit stays disabled while provider or label is empty", async () => {
    render(<ConnectorsTab />);
    // Wait for the provider dropdown to populate + enable.
    await waitFor(() => {
      expect((screen.getByTestId("oauth-provider") as HTMLSelectElement).disabled).toBe(false);
    });
    expect(screen.getByTestId("oauth-begin-submit")).toHaveProperty("disabled", true);
    fireEvent.change(screen.getByTestId("oauth-provider"), { target: { value: "github" } });
    expect(screen.getByTestId("oauth-begin-submit")).toHaveProperty("disabled", true);
    fireEvent.change(screen.getByTestId("oauth-label"), { target: { value: "My GitHub" } });
    expect(screen.getByTestId("oauth-begin-submit")).toHaveProperty("disabled", false);
  });

  it("the paste-code field is not rendered before a challenge exists", () => {
    render(<ConnectorsTab />);
    expect(screen.queryByTestId("oauth-code-input")).toBeNull();
    expect(screen.queryByTestId("oauth-authorize-link")).toBeNull();
  });

  // ---- config-backed OAuth provider dropdown + empty-state (spec D7, O-5) ----

  it("the provider dropdown lists the providers from connectorListProviders", async () => {
    connectorListProvidersMock.mockResolvedValue(["github", "prowl"]);
    render(<ConnectorsTab />);

    await waitFor(() => {
      expect(connectorListProvidersMock).toHaveBeenCalledWith();
    });

    const select = screen.getByTestId("oauth-provider") as HTMLSelectElement;
    await waitFor(() => {
      // placeholder option + the two configured providers
      expect(select.querySelectorAll("option").length).toBe(3);
    });
    const optionValues = Array.from(select.querySelectorAll("option")).map((o) => o.value);
    expect(optionValues).toContain("github");
    expect(optionValues).toContain("prowl");
    // No empty-state when providers exist.
    expect(screen.queryByTestId("oauth-no-providers")).toBeNull();
  });

  it("an empty provider registry shows the honest empty-state and disables begin", async () => {
    connectorListProvidersMock.mockResolvedValue([]);
    render(<ConnectorsTab />);

    await waitFor(() => {
      expect(screen.getByTestId("oauth-no-providers")).toBeTruthy();
    });
    expect(screen.getByTestId("oauth-no-providers").textContent).toBe(
      strings.ext.connectors.noProviders,
    );
    // The dropdown offers no real options, and begin is disabled in the empty-state.
    expect((screen.getByTestId("oauth-provider") as HTMLSelectElement).disabled).toBe(true);
    expect(screen.getByTestId("oauth-begin-submit")).toHaveProperty("disabled", true);
  });

  it("orchdDown disables the provider dropdown and the begin button", async () => {
    useAppStore.setState({ orchdDown: true }, false);
    connectorListProvidersMock.mockResolvedValue(["github"]);
    render(<ConnectorsTab />);

    // While orchd is down the dropdown is disabled regardless of any cached providers, and begin
    // stays disabled (mutating control guard).
    expect((screen.getByTestId("oauth-provider") as HTMLSelectElement).disabled).toBe(true);
    expect(screen.getByTestId("oauth-begin-submit")).toHaveProperty("disabled", true);
  });

  // ---- generic-rest ops runner ----

  it("a generic-rest account's ops runner lists ops then invokes, showing the untrusted banner", async () => {
    connectorListOpsMock.mockResolvedValue([
      { name: "get", description: "GET request" },
      { name: "post", description: null },
    ]);
    connectorInvokeMock.mockResolvedValue({
      artifactId: "art1",
      invocationId: "i1",
      contentJson: '{"ok":true}',
      isError: false,
    });
    // `refreshAccounts` (mount effect) re-fetches from `connectorListAccounts` and REPLACES
    // whatever the store already holds — mock its resolution to match, so the account set below
    // survives past the first `await` in this test (which lets that mount-time fetch resolve).
    connectorListAccountsMock.mockResolvedValue([
      makeAccount({ id: "a1", provider: "generic-rest" }),
    ]);
    useAppStore.setState(
      { accounts: [makeAccount({ id: "a1", provider: "generic-rest" })] },
      false,
    );
    render(<ConnectorsTab />);

    await waitFor(() => {
      expect(connectorListOpsMock).toHaveBeenCalledWith({ accountId: "a1" });
    });

    await waitFor(() => {
      expect(screen.getByTestId("ops-select-a1")).toBeTruthy();
    });
    fireEvent.change(screen.getByTestId("ops-select-a1"), { target: { value: "get" } });
    fireEvent.change(screen.getByTestId("ops-args-a1"), { target: { value: '{"path":"/x"}' } });
    fireEvent.click(screen.getByTestId("ops-invoke-a1"));

    await waitFor(() => {
      expect(connectorInvokeMock).toHaveBeenCalledWith({
        accountId: "a1",
        op: "get",
        argsJson: '{"path":"/x"}',
      });
    });

    await waitFor(() => {
      expect(screen.getByTestId("ops-result-untrusted-a1")).toBeTruthy();
    });
  });

  it("a FAILED ops-list load shows a retry affordance distinct from 'no ops' (P-15)", async () => {
    connectorListOpsMock.mockReset().mockRejectedValueOnce({ kind: "daemon", code: "Io" });
    connectorListAccountsMock.mockResolvedValue([
      makeAccount({ id: "a1", provider: "generic-rest" }),
    ]);
    useAppStore.setState(
      { accounts: [makeAccount({ id: "a1", provider: "generic-rest" })] },
      false,
    );
    render(<ConnectorsTab />);

    await waitFor(() => expect(screen.getByTestId("ops-load-failed-a1")).toBeTruthy());
    expect(screen.getByTestId("ops-load-failed-a1").textContent).toContain(
      strings.ext.connectors.opsLoadFailed,
    );
    expect(screen.getByTestId("ops-retry-a1")).toBeTruthy();

    // [Retry] re-fetches; on success the failed marker clears and the ops select is populated.
    connectorListOpsMock.mockResolvedValueOnce([{ name: "get", description: null }]);
    await act(async () => {
      fireEvent.click(screen.getByTestId("ops-retry-a1"));
    });
    await waitFor(() => expect(screen.queryByTestId("ops-load-failed-a1")).toBeNull());
    expect(connectorListOpsMock).toHaveBeenCalledTimes(2);
  });

  it("a ready account with an empty op catalog shows NO load-failed marker (empty ≠ failed, P-15)", async () => {
    connectorListOpsMock.mockReset().mockResolvedValue([]); // genuinely empty, not a failure
    connectorListAccountsMock.mockResolvedValue([
      makeAccount({ id: "a1", provider: "generic-rest" }),
    ]);
    useAppStore.setState(
      { accounts: [makeAccount({ id: "a1", provider: "generic-rest" })] },
      false,
    );
    render(<ConnectorsTab />);
    await waitFor(() => expect(screen.getByTestId("ops-select-a1")).toBeTruthy());
    expect(screen.queryByTestId("ops-load-failed-a1")).toBeNull();
  });

  it("a Consent-kind invoke failure appends the recovery hint (Servers → Connect, P-20)", async () => {
    connectorListOpsMock.mockResolvedValue([{ name: "get", description: null }]);
    connectorInvokeMock.mockRejectedValueOnce({ kind: "daemon", code: "Consent", message: "stale" });
    connectorListAccountsMock.mockResolvedValue([
      makeAccount({ id: "a1", provider: "generic-rest" }),
    ]);
    useAppStore.setState(
      { accounts: [makeAccount({ id: "a1", provider: "generic-rest" })] },
      false,
    );
    render(<ConnectorsTab />);
    await waitFor(() => screen.getByTestId("ops-select-a1"));
    useAppStore.setState({ toast: null, toastQueue: [] }, false); // FIFO queue: start clean
    fireEvent.change(screen.getByTestId("ops-select-a1"), { target: { value: "get" } });
    fireEvent.click(screen.getByTestId("ops-invoke-a1"));

    await waitFor(() => expect(screen.getByTestId("ops-call-error-a1")).toBeTruthy());
    expect(screen.getByTestId("ops-call-error-a1").textContent).toContain(
      strings.errors.consentRecovery,
    );
    expect(useAppStore.getState().toast).toContain(strings.errors.consentRecovery);
  });

  it("a non-generic-rest account renders no ops runner", () => {
    useAppStore.setState(
      { accounts: [makeAccount({ id: "a1", provider: "github", authKind: "oauth" })] },
      false,
    );
    render(<ConnectorsTab />);
    expect(screen.queryByTestId("ops-select-a1")).toBeNull();
    expect(connectorListOpsMock).not.toHaveBeenCalled();
  });

  it("invalid JSON args show an inline error instead of calling connectorInvoke", async () => {
    connectorListOpsMock.mockResolvedValue([{ name: "get", description: null }]);
    connectorListAccountsMock.mockResolvedValue([
      makeAccount({ id: "a1", provider: "generic-rest" }),
    ]);
    useAppStore.setState(
      { accounts: [makeAccount({ id: "a1", provider: "generic-rest" })] },
      false,
    );
    render(<ConnectorsTab />);
    await waitFor(() => screen.getByTestId("ops-select-a1"));
    fireEvent.change(screen.getByTestId("ops-select-a1"), { target: { value: "get" } });
    fireEvent.change(screen.getByTestId("ops-args-a1"), { target: { value: "not json" } });
    fireEvent.click(screen.getByTestId("ops-invoke-a1"));

    await waitFor(() => {
      expect(screen.getByTestId("ops-call-error-a1")).toBeTruthy();
    });
    expect(connectorInvokeMock).not.toHaveBeenCalled();
  });

  // ---- honest degradation ----

  it("orchdDown:true disables ALL mutating controls, and clicking each never calls its wrapper (spec §8, ServersTab precedent)", async () => {
    connectorListOpsMock.mockResolvedValue([{ name: "get", description: null }]);
    connectorBeginOAuthMock.mockResolvedValue({
      authorizeUrl: "https://example.com/authorize",
      state: "st-1",
    });
    connectorListAccountsMock.mockResolvedValue([
      makeAccount({ id: "a1", provider: "generic-rest" }),
    ]);
    useAppStore.setState(
      {
        accounts: [makeAccount({ id: "a1", provider: "generic-rest" })],
        orchdDown: false,
      },
      false,
    );
    render(<ConnectorsTab />);

    // Populate every field with orchdDown:false FIRST so each control's OWN disable-condition
    // (empty required field, no op selected, no challenge yet) is already satisfied — the later
    // assertion is then provably owed to orchdDown ALONE (mirrors ServersTab.test.tsx's pattern).
    fireEvent.change(screen.getByTestId("apikey-provider"), { target: { value: "generic-rest" } });
    fireEvent.change(screen.getByTestId("apikey-label"), { target: { value: "x" } });
    fireEvent.change(screen.getByTestId("apikey-key"), { target: { value: "k" } });

    // Wait for the provider dropdown (fed by connectorListProviders) to populate + enable.
    await waitFor(() => {
      expect((screen.getByTestId("oauth-provider") as HTMLSelectElement).disabled).toBe(false);
    });
    fireEvent.change(screen.getByTestId("oauth-provider"), { target: { value: "github" } });
    fireEvent.change(screen.getByTestId("oauth-label"), { target: { value: "y" } });
    fireEvent.click(screen.getByTestId("oauth-begin-submit"));
    await waitFor(() => screen.getByTestId("oauth-code-input"));
    fireEvent.change(screen.getByTestId("oauth-code-input"), { target: { value: "code" } });

    await waitFor(() => screen.getByTestId("ops-select-a1"));
    fireEvent.change(screen.getByTestId("ops-select-a1"), { target: { value: "get" } });

    connectorBeginOAuthMock.mockClear();

    act(() => useAppStore.setState({ orchdDown: true }, false));

    const controls = [
      screen.getByTestId("apikey-submit"),
      screen.getByTestId("oauth-begin-submit"),
      screen.getByTestId("oauth-complete-submit"),
      screen.getByTestId("account-delete-a1"),
      screen.getByTestId("ops-invoke-a1"),
    ];
    for (const c of controls) expect(c).toHaveProperty("disabled", true);

    // `user.click` faithfully emulates a real user click, which the browser suppresses on a
    // disabled control (plain `fireEvent.click` does not gate on `disabled` in jsdom).
    const user = userEvent.setup();
    for (const c of controls) await user.click(c);

    expect(connectorAddApiKeyMock).not.toHaveBeenCalled();
    expect(connectorBeginOAuthMock).not.toHaveBeenCalled();
    expect(connectorCompleteOAuthMock).not.toHaveBeenCalled();
    expect(connectorDeleteAccountMock).not.toHaveBeenCalled();
    expect(connectorInvokeMock).not.toHaveBeenCalled();
  });
});
