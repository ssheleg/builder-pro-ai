import { useCallback, useEffect, useState, type CSSProperties, type JSX } from "react";
import { open as openUrl } from "@tauri-apps/plugin-shell";
import { useAppStore } from "../../store/store";
import {
  connectorAddApiKey,
  connectorBeginOAuth,
  connectorCompleteOAuth,
  connectorDeleteAccount,
  connectorInvoke,
  connectorListOps,
  connectorListProviders,
  describeOrchdError,
  isConsentError,
} from "../../ipc/orchd";
import type { Account, AccountAuthKind, ConnectorOp, OAuthChallenge } from "../../ipc/orchd-types";
import { useSubmitGuard } from "../../hooks/useSubmitGuard";
import { Badge, Button, Input, Select, TextArea, Panel, EmptyState } from "../../ui/primitives";
import { strings } from "../../strings";

const AUTH_LABEL: Record<AccountAuthKind, string> = {
  oauth: "OAuth",
  apikey: strings.ext.connectors.apiKeyLabel,
};

const createFormStyle: CSSProperties = {
  display: "flex",
  flexWrap: "wrap",
  gap: "var(--sp-2)",
};

const createInputStyle: CSSProperties = {
  flex: "1 1 160px",
  minWidth: 0,
};

const rowStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: "var(--sp-2)",
  padding: "var(--sp-3)",
  marginBottom: "var(--sp-2)",
  borderRadius: "var(--r-md)",
  background: "var(--panel-2)",
};

const rowHeaderStyle: CSSProperties = {
  display: "flex",
  flexWrap: "wrap",
  alignItems: "center",
  gap: "var(--sp-2)",
  fontFamily: "var(--font-mono)",
  fontSize: "var(--fs-sm)",
};

const titleTextStyle: CSSProperties = {
  color: "var(--ink)",
  fontWeight: 600,
};

const metaStyle: CSSProperties = {
  color: "var(--muted)",
  fontSize: "var(--fs-xs)",
  fontVariantNumeric: "tabular-nums",
};

const linkStyle: CSSProperties = {
  color: "var(--accent)",
  fontSize: "var(--fs-sm)",
};

const invokeRowStyle: CSSProperties = {
  display: "flex",
  gap: "var(--sp-2)",
  alignItems: "flex-start",
  marginTop: "var(--sp-1)",
};

const textareaStyle: CSSProperties = {
  flex: 1,
  minWidth: 0,
  fontFamily: "var(--font-mono)",
  fontSize: "var(--fs-xs)",
};

const selectStyle: CSSProperties = {
  flexShrink: 0,
};

const preStyle: CSSProperties = {
  fontFamily: "var(--font-mono)",
  fontSize: "var(--fs-xs)",
  color: "var(--muted)",
  background: "var(--panel)",
  borderRadius: "var(--r-sm)",
  padding: "var(--sp-2)",
  margin: 0,
  whiteSpace: "pre-wrap",
  wordBreak: "break-all",
};

const inlineErrorStyle: CSSProperties = {
  fontSize: "var(--fs-sm)",
  lineHeight: 1.4,
  color: "var(--danger)",
  borderLeft: "3px solid var(--danger)",
  paddingLeft: "var(--sp-2)",
};

/** `provider === "generic-rest"` is the one reference `ConnectorAdapter` this Phase-1 slice ships
 * (spec §7: "one reference adapter"; §8: "per generic-rest account, an ops runner") — the ops
 * runner section only renders for accounts of this provider. */
const GENERIC_REST_PROVIDER = "generic-rest";

interface OpsCallResult {
  contentJson: string;
  isError: boolean;
}

/** Per-account ops-list fetch state (P-15): `failed` is distinct from a `ready` account whose op
 * catalog is genuinely empty — the select alone cannot tell "load broke" from "no ops". */
type OpsLoadStatus = "loading" | "ready" | "failed";

/** Human message for a rejected orchd call, with the consent-recovery hint appended when the
 * rejection is a `Consent` denial (P-20) — `ConnectDialog` is reachable only from the Servers tab,
 * so a bare "consent required" toast would dead-end. Mirrors `ToolsBrowser`'s identical helper. */
function describeWithRecovery(e: unknown): string {
  const message = describeOrchdError(e);
  return isConsentError(e) ? `${message} ${strings.errors.consentRecovery}` : message;
}

function formatExpiry(expiresAt: number | null): string {
  return expiresAt === null ? "—" : new Date(expiresAt).toLocaleString();
}

function formatScopes(scopes: string[]): string {
  return scopes.length === 0 ? "—" : scopes.join(", ");
}

/**
 * Connectors tab (S-EXT §8, T13b): the connector-account registry — accounts list + an
 * "Add API key" form (masked, never echoed back) + a "Connect OAuth" form + (per
 * `generic-rest` account) an ops runner. Mirrors `ServersTab`/`ToolsBrowser`'s conventions
 * exactly: on mount `refreshAccounts()`, every mutating control `disabled={orchdDown}`, every
 * async failure -> `showToast(describeOrchdError(e))` rather than a silent no-op.
 *
 * **OAuth flow (spec §8/§10, D5/D14 Phase 2, honest v1)**: `connectorBeginOAuth` returns an
 * `OAuthChallenge{authorizeUrl,state}`. This component both (a) best-effort auto-opens
 * `authorizeUrl` via `@tauri-apps/plugin-shell`'s `open` — the SAME mechanism
 * `terminal-manager.ts`'s OSC-8 `linkHandler` already uses for http(s) links, the app's one
 * existing open-external-URL path — and (b) renders it as a clickable link the owner can click
 * again if a pop-up blocker or similar swallowed the auto-open. There is no OS-level redirect
 * capture in this Phase (that is a §10 human/browser-integration step, out of scope here): once
 * the provider redirects the owner's browser back with a `code` query param, the owner copies it
 * and pastes it into the "paste code" field, which calls `connectorCompleteOAuth({state, code})`
 * to finish the PKCE exchange. This paste-the-code step is the deliberately honest v1 UX, not a
 * placeholder — it works today against any standard OAuth 2.1 authorization-code provider without
 * a custom URL-scheme/loopback-listener registration.
 *
 * **Ops runner (spec §7/§8)**: for every `generic-rest` account, this component eagerly
 * `connectorListOps({accountId})`s once (mirrors `ToolsBrowser`'s per-server `refreshMcpTools`
 * auto-fetch-on-mount) into local `opsByAccount` state — there is no store slice for ops (an
 * account's op catalog is small/static per provider, unlike the tool-cache-with-list_changed
 * MCP case), so this component owns that cache itself. Selecting an op + a JSON args textarea +
 * "invoke" calls `connectorInvoke`, trust-gated identically to `mcpCallTool` (spec §6/§7); the
 * result is rendered with an unconditional "unverified data" banner — EVERY artifact this
 * slice creates is `is_untrusted:true` by construction (spec D9), mirrors `ToolsBrowser`'s own
 * banner exactly.
 */
export function ConnectorsTab(): JSX.Element {
  const accounts = useAppStore((s) => s.accounts);
  const orchdDown = useAppStore((s) => s.orchdDown);
  const refreshAccounts = useAppStore((s) => s.refreshAccounts);
  const showToast = useAppStore((s) => s.showToast);
  // Independent double-submit guards (spec D6) — each "add account" submit is its own form, so they
  // must not cross-disable each other (cross-cutting P-19, findings J-03..J-05). OAuth begin/complete
  // additionally protect a real external round-trip.
  const apiKeyForm = useSubmitGuard();
  const oauthBeginForm = useSubmitGuard();
  const oauthCompleteForm = useSubmitGuard();

  // ---- add-api-key form ----
  const [apiKeyProvider, setApiKeyProvider] = useState("");
  const [apiKeyLabel, setApiKeyLabel] = useState("");
  const [apiKeyValue, setApiKeyValue] = useState("");

  // ---- begin/complete-OAuth form ----
  const [oauthProvider, setOauthProvider] = useState("");
  const [oauthLabel, setOauthLabel] = useState("");
  const [oauthScopes, setOauthScopes] = useState("");
  const [oauthChallenge, setOauthChallenge] = useState<OAuthChallenge | null>(null);
  const [oauthCode, setOauthCode] = useState("");

  // ---- config-backed OAuth provider registry (spec D7, O-5) ----
  // The provider free-text input is a dropdown fed by `connectorListProviders()`. `providersLoaded`
  // distinguishes "still loading" from a genuinely empty registry, so the honest empty-state
  // ("No OAuth providers configured …") shows only once we KNOW the registry is empty (mirrors the
  // ops runner's `loading`/`ready`-vs-empty distinction). Providers are boot-static config with no
  // push, so this fetches on mount and again whenever orchd reconnects (`orchdDown` → false).
  const [oauthProviders, setOauthProviders] = useState<string[]>([]);
  const [providersLoaded, setProvidersLoaded] = useState(false);

  // ---- per-account ops runner (generic-rest only) ----
  const [opsByAccount, setOpsByAccount] = useState<Record<string, ConnectorOp[]>>({});
  const [opsStatus, setOpsStatus] = useState<Record<string, OpsLoadStatus>>({});
  const [selectedOp, setSelectedOp] = useState<Record<string, string>>({});
  const [opsArgsDraft, setOpsArgsDraft] = useState<Record<string, string>>({});
  const [opsCallError, setOpsCallError] = useState<Record<string, string | null>>({});
  const [opsResult, setOpsResult] = useState<Record<string, OpsCallResult | undefined>>({});

  useEffect(() => {
    void refreshAccounts();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Fetch the config-backed OAuth provider registry on mount and on every orchd reconnect. While
  // orchd is down there is nothing to fetch (the request would fail); once it's back up we (re)load.
  // A failure marks `providersLoaded` anyway so the UI settles on the honest empty-state rather than
  // an indefinite "loading" the user can't tell apart from "no providers".
  useEffect(() => {
    if (orchdDown) return;
    let cancelled = false;
    connectorListProviders()
      .then((names) => {
        if (cancelled) return;
        setOauthProviders(names);
        setProvidersLoaded(true);
      })
      .catch((e: unknown) => {
        if (cancelled) return;
        setProvidersLoaded(true);
        showToast(describeOrchdError(e));
      });
    return () => {
      cancelled = true;
    };
  }, [orchdDown, showToast]);

  const genericRestAccountIds = accounts
    .filter((a) => a.provider === GENERIC_REST_PROVIDER)
    .map((a) => a.id)
    .join(",");

  // Fetch (or re-fetch, via the [Retry] button) one account's op catalog, tracking a per-account
  // status so a FAILED load (P-15) is distinguishable from a `ready` account with zero ops — the
  // select alone cannot tell them apart. A failure records `failed` (not just a toast) so the
  // effect below won't loop on it and the row can offer an inline retry.
  const loadOps = useCallback(
    (accountId: string) => {
      setOpsStatus((prev) => ({ ...prev, [accountId]: "loading" }));
      connectorListOps({ accountId })
        .then((ops) => {
          setOpsByAccount((prev) => ({ ...prev, [accountId]: ops }));
          setOpsStatus((prev) => ({ ...prev, [accountId]: "ready" }));
        })
        .catch((e: unknown) => {
          setOpsStatus((prev) => ({ ...prev, [accountId]: "failed" }));
          showToast(describeWithRecovery(e));
        });
    },
    [showToast],
  );

  useEffect(() => {
    for (const account of accounts) {
      if (account.provider !== GENERIC_REST_PROVIDER) continue;
      if (account.id in opsStatus) continue; // already loading / loaded / failed
      loadOps(account.id);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [genericRestAccountIds]);

  const apiKeyBlocked =
    apiKeyProvider.trim() === "" || apiKeyLabel.trim() === "" || apiKeyValue.trim() === "";

  async function handleAddApiKey(): Promise<void> {
    if (apiKeyBlocked) return;
    try {
      await connectorAddApiKey({
        provider: apiKeyProvider.trim(),
        label: apiKeyLabel.trim(),
        apiKey: apiKeyValue.trim(),
      });
      setApiKeyProvider("");
      setApiKeyLabel("");
      // Never keep the key in local state once submitted (masked, never echoed back — spec §8).
      setApiKeyValue("");
      await refreshAccounts();
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

  // An empty registry hard-blocks begin (spec D7: the begin button is disabled in the empty-state)
  // in addition to the usual "a provider + a label must be chosen".
  const oauthBeginBlocked =
    oauthProviders.length === 0 || oauthProvider.trim() === "" || oauthLabel.trim() === "";

  async function handleBeginOAuth(): Promise<void> {
    if (oauthBeginBlocked) return;
    const scopes = oauthScopes
      .split(",")
      .map((s) => s.trim())
      .filter((s) => s !== "");
    try {
      const challenge = await connectorBeginOAuth({
        provider: oauthProvider.trim(),
        label: oauthLabel.trim(),
        scopes: scopes.length > 0 ? scopes : undefined,
      });
      setOauthChallenge(challenge);
      // Best-effort auto-open; the rendered link (below) is the actual affordance if this is
      // blocked or unsupported, so a rejection here is silently swallowed rather than toasted.
      openUrl(challenge.authorizeUrl).catch(() => {});
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

  const oauthCompleteBlocked = oauthChallenge === null || oauthCode.trim() === "";

  async function handleCompleteOAuth(): Promise<void> {
    if (oauthChallenge === null || oauthCompleteBlocked) return;
    try {
      await connectorCompleteOAuth({ state: oauthChallenge.state, code: oauthCode.trim() });
      setOauthProvider("");
      setOauthLabel("");
      setOauthScopes("");
      setOauthChallenge(null);
      setOauthCode("");
      await refreshAccounts();
      showToast(strings.ext.connectors.accountConnected, "success");
    } catch (e) {
      // Keep the challenge/code fields as-is so the owner can retry the paste (spec §8 v1 flow).
      showToast(describeOrchdError(e));
    }
  }

  const submitAddApiKey = apiKeyForm.guard(handleAddApiKey);
  const submitBeginOAuth = oauthBeginForm.guard(handleBeginOAuth);
  const submitCompleteOAuth = oauthCompleteForm.guard(handleCompleteOAuth);

  async function handleDeleteAccount(account: Account): Promise<void> {
    if (!window.confirm(strings.ext.connectors.deleteConfirm(account.label))) return;
    try {
      await connectorDeleteAccount({ id: account.id });
      await refreshAccounts();
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

  async function handleInvoke(accountId: string): Promise<void> {
    const op = selectedOp[accountId];
    if (op === undefined || op === "") return;
    const raw = (opsArgsDraft[accountId] ?? "").trim();
    const argsJson = raw === "" ? "{}" : raw;
    try {
      JSON.parse(argsJson);
    } catch {
      setOpsCallError((prev) => ({ ...prev, [accountId]: strings.common.argsInvalidJson }));
      return;
    }
    setOpsCallError((prev) => ({ ...prev, [accountId]: null }));
    try {
      const res = await connectorInvoke({ accountId, op, argsJson });
      setOpsResult((prev) => ({
        ...prev,
        [accountId]: { contentJson: res.contentJson, isError: res.isError },
      }));
    } catch (e) {
      // A stale/URL-changed consent grant surfaces here as a `Consent` denial — append the recovery
      // hint pointing at the Servers-tab connect flow (P-20), the only place consent is re-granted.
      const message = describeWithRecovery(e);
      setOpsCallError((prev) => ({ ...prev, [accountId]: message }));
      showToast(message);
    }
  }

  return (
    <div data-testid="connectors-tab" style={{ display: "flex", flexDirection: "column", gap: "var(--sp-4)" }}>
      <Panel title={strings.ext.connectors.accountsTitle}>
        {accounts.length === 0 ? (
          <EmptyState data-testid="accounts-empty" title={strings.ext.connectors.noAccounts} />
        ) : (
          <div role="list">
            {accounts.map((account) => {
              const isGenericRest = account.provider === GENERIC_REST_PROVIDER;
              const ops = opsByAccount[account.id] ?? [];
              const selected = selectedOp[account.id] ?? "";
              const invokeDisabled = orchdDown || selected === "";
              const result = opsResult[account.id];
              return (
                <div
                  key={account.id}
                  data-testid={`account-row-${account.id}`}
                  role="listitem"
                  style={rowStyle}
                >
                  <div style={rowHeaderStyle}>
                    <span data-testid={`account-provider-${account.id}`} style={titleTextStyle}>
                      {account.provider}
                    </span>
                    <span data-testid={`account-label-${account.id}`} style={metaStyle}>
                      {account.label}
                    </span>
                    <Badge tone="muted" data-testid={`account-authkind-${account.id}`}>
                      {AUTH_LABEL[account.authKind]}
                    </Badge>
                    <span data-testid={`account-scopes-${account.id}`} style={metaStyle}>
                      {strings.ext.connectors.scopesLabel} {formatScopes(account.scopes)}
                    </span>
                    <span data-testid={`account-expiry-${account.id}`} style={metaStyle}>
                      {strings.ext.connectors.expiresLabel} {formatExpiry(account.expiresAt)}
                    </span>
                    <Button
                      type="button"
                      variant="danger"
                      size="sm"
                      data-testid={`account-delete-${account.id}`}
                      disabled={orchdDown}
                      onClick={() => void handleDeleteAccount(account)}
                    >
                      {strings.ext.delete}
                    </Button>
                  </div>

                  {isGenericRest && (
                    <div>
                      {opsStatus[account.id] === "failed" && (
                        <div
                          data-testid={`ops-load-failed-${account.id}`}
                          style={{
                            display: "flex",
                            alignItems: "center",
                            gap: "var(--sp-2)",
                            marginBottom: "var(--sp-1)",
                          }}
                        >
                          <span style={{ color: "var(--danger)", fontSize: "var(--fs-sm)" }}>
                            {strings.ext.connectors.opsLoadFailed}
                          </span>
                          <Button
                            type="button"
                            variant="ghost"
                            size="sm"
                            data-testid={`ops-retry-${account.id}`}
                            disabled={orchdDown}
                            onClick={() => loadOps(account.id)}
                          >
                            {strings.common.retry}
                          </Button>
                        </div>
                      )}
                      <div style={invokeRowStyle}>
                        <Select
                          data-testid={`ops-select-${account.id}`}
                          aria-label={strings.ext.connectors.operationFor(account.label)}
                          value={selected}
                          disabled={orchdDown}
                          onChange={(e) =>
                            setSelectedOp((prev) => ({ ...prev, [account.id]: e.target.value }))
                          }
                          style={selectStyle}
                        >
                          <option value="">{strings.ext.connectors.operationOption}</option>
                          {ops.map((op) => (
                            <option key={op.name} value={op.name}>
                              {op.name}
                            </option>
                          ))}
                        </Select>
                        <TextArea
                          data-testid={`ops-args-${account.id}`}
                          aria-label={strings.ext.connectors.argsFor(account.label)}
                          placeholder="{}"
                          disabled={invokeDisabled}
                          value={opsArgsDraft[account.id] ?? ""}
                          onChange={(e) =>
                            setOpsArgsDraft((prev) => ({ ...prev, [account.id]: e.target.value }))
                          }
                          rows={2}
                          style={textareaStyle}
                        />
                        <Button
                          type="button"
                          variant="ghost"
                          size="sm"
                          data-testid={`ops-invoke-${account.id}`}
                          disabled={invokeDisabled}
                          onClick={() => void handleInvoke(account.id)}
                        >
                          {strings.ext.invoke}
                        </Button>
                      </div>

                      {opsCallError[account.id] != null && (
                        <div
                          role="alert"
                          data-testid={`ops-call-error-${account.id}`}
                          style={inlineErrorStyle}
                        >
                          {opsCallError[account.id]}
                        </div>
                      )}

                      {result && (
                        <div
                          data-testid={`ops-result-${account.id}`}
                          style={{ display: "flex", flexDirection: "column", gap: "var(--sp-1)", marginTop: "var(--sp-1)" }}
                        >
                          <Badge tone="warn" data-testid={`ops-result-untrusted-${account.id}`}>
                            {strings.ext.unverified}
                          </Badge>
                          {result.isError && (
                            <span style={{ fontSize: "var(--fs-sm)", color: "var(--danger)" }}>
                              {strings.ext.connectors.operationError}
                            </span>
                          )}
                          <pre style={preStyle}>{result.contentJson}</pre>
                        </div>
                      )}
                    </div>
                  )}
                </div>
              );
            })}
          </div>
        )}
      </Panel>

      <Panel title={strings.ext.connectors.addApiKeyTitle}>
        <div style={createFormStyle}>
          <Input
            data-testid="apikey-provider"
            aria-label={strings.ext.connectors.providerAria}
            placeholder={strings.ext.connectors.providerPlaceholder}
            value={apiKeyProvider}
            onChange={(e) => setApiKeyProvider(e.target.value)}
            style={createInputStyle}
          />
          <Input
            data-testid="apikey-label"
            aria-label={strings.ext.connectors.labelAria}
            placeholder={strings.ext.connectors.labelPlaceholder}
            value={apiKeyLabel}
            onChange={(e) => setApiKeyLabel(e.target.value)}
            style={createInputStyle}
          />
          <Input
            type="password"
            data-testid="apikey-key"
            aria-label={strings.ext.connectors.apiKeyAria}
            placeholder={strings.ext.connectors.apiKeyPlaceholder}
            value={apiKeyValue}
            onChange={(e) => setApiKeyValue(e.target.value)}
            style={createInputStyle}
          />
          <Button
            type="button"
            variant="primary"
            size="sm"
            data-testid="apikey-submit"
            disabled={orchdDown || apiKeyBlocked || apiKeyForm.submitting}
            onClick={() => void submitAddApiKey()}
          >
            {strings.ext.connectors.addApiKey}
          </Button>
        </div>
      </Panel>

      <Panel title={strings.ext.connectors.connectOAuthTitle}>
        {providersLoaded && oauthProviders.length === 0 ? (
          <div
            data-testid="oauth-no-providers"
            style={{ color: "var(--muted)", fontSize: "var(--fs-sm)", marginBottom: "var(--sp-2)" }}
          >
            {strings.ext.connectors.noProviders}
          </div>
        ) : null}
        <div style={createFormStyle}>
          <Select
            data-testid="oauth-provider"
            aria-label={strings.ext.connectors.oauthProviderAria}
            value={oauthProvider}
            disabled={orchdDown || oauthProviders.length === 0}
            onChange={(e) => setOauthProvider(e.target.value)}
            style={selectStyle}
          >
            <option value="">{strings.ext.connectors.oauthProviderOption}</option>
            {oauthProviders.map((p) => (
              <option key={p} value={p}>
                {p}
              </option>
            ))}
          </Select>
          <Input
            data-testid="oauth-label"
            aria-label={strings.ext.connectors.labelAria}
            placeholder={strings.ext.connectors.labelPlaceholder}
            value={oauthLabel}
            onChange={(e) => setOauthLabel(e.target.value)}
            style={createInputStyle}
          />
          <Input
            data-testid="oauth-scopes"
            aria-label={strings.ext.connectors.scopesAria}
            placeholder={strings.ext.connectors.scopesPlaceholder}
            value={oauthScopes}
            onChange={(e) => setOauthScopes(e.target.value)}
            style={createInputStyle}
          />
          <Button
            type="button"
            variant="primary"
            size="sm"
            data-testid="oauth-begin-submit"
            disabled={orchdDown || oauthBeginBlocked || oauthBeginForm.submitting}
            onClick={() => void submitBeginOAuth()}
          >
            {strings.ext.connectors.startOAuth}
          </Button>

          {oauthChallenge && (
            <div style={{ display: "flex", flexDirection: "column", gap: "var(--sp-2)", width: "100%" }}>
              <a
                data-testid="oauth-authorize-link"
                href={oauthChallenge.authorizeUrl}
                target="_blank"
                rel="noreferrer"
                style={linkStyle}
              >
                {strings.ext.connectors.openAuthPage}
              </a>
              <div style={{ display: "flex", gap: "var(--sp-2)" }}>
                <Input
                  data-testid="oauth-code-input"
                  aria-label={strings.ext.connectors.codeAria}
                  placeholder={strings.ext.connectors.codePlaceholder}
                  value={oauthCode}
                  onChange={(e) => setOauthCode(e.target.value)}
                  style={createInputStyle}
                />
                <Button
                  type="button"
                  variant="primary"
                  size="sm"
                  data-testid="oauth-complete-submit"
                  disabled={orchdDown || oauthCompleteBlocked || oauthCompleteForm.submitting}
                  onClick={() => void submitCompleteOAuth()}
                >
                  {strings.ext.connectors.finish}
                </Button>
              </div>
            </div>
          )}
        </div>
      </Panel>
    </div>
  );
}
