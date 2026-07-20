import { useState, type CSSProperties, type JSX } from "react";
import { useAppStore } from "../../store/store";
import {
  mcpAddServer,
  mcpSetServerEnabled,
  mcpDeleteServer,
  mcpSetServerBearer,
  mcpDisconnect,
  describeOrchdError,
} from "../../ipc/orchd";
import type { McpAuthKind, McpScope, McpServer } from "../../ipc/orchd-types";
import { useSubmitGuard } from "../../hooks/useSubmitGuard";
import { ConnectDialog } from "./ConnectDialog";
import { Button, Input, Select, EmptyState } from "../../ui/primitives";
import { strings } from "../../strings";

const AUTH_LABEL: Record<McpAuthKind, string> = {
  none: strings.ext.servers.authKind.none,
  bearer: strings.ext.servers.authKind.bearer,
  oauth: strings.ext.servers.authKind.oauth,
};

const createFormStyle: CSSProperties = {
  display: "flex",
  flexWrap: "wrap",
  gap: "var(--sp-2)",
  padding: "var(--sp-3)",
  marginBottom: "var(--sp-3)",
  borderRadius: "var(--r-md)",
  background: "var(--panel-2)",
};

const createInputStyle: CSSProperties = {
  flex: "1 1 160px",
  minWidth: 0,
};

const selectStyle: CSSProperties = {
  flexShrink: 0,
};

const rowStyle: CSSProperties = {
  display: "flex",
  flexWrap: "wrap",
  alignItems: "center",
  gap: "var(--sp-2)",
  padding: "var(--sp-2)",
  fontFamily: "var(--font-mono)",
  fontSize: "var(--fs-sm)",
  borderBottom: "1px solid var(--hairline)",
};

const dotStyle: CSSProperties = {
  display: "inline-block",
  width: 8,
  height: 8,
  borderRadius: "50%",
  flexShrink: 0,
};

const titleTextStyle: CSSProperties = {
  minWidth: 0,
  overflow: "hidden",
  textOverflow: "ellipsis",
  whiteSpace: "nowrap",
  color: "var(--ink)",
  fontWeight: 600,
};

const metaStyle: CSSProperties = {
  color: "var(--muted)",
  fontSize: "var(--fs-xs)",
  fontVariantNumeric: "tabular-nums",
};

const bearerInputStyle: CSSProperties = {
  flex: "1 1 160px",
  minWidth: 0,
  fontFamily: "var(--font-mono)",
};

/**
 * Servers tab (S-EXT §8, T8): MCP server registry — list + add-server form + per-server
 * enable/disable, connect/disconnect, and a masked set-bearer input. Phase 1 ships HTTP transport
 * only (spec D6) — the add form's transport picker is fixed at `"http"`; a `"stdio"` option is
 * present but disabled ("soon"), matching the brief's "a stdio option can be present but
 * disabled".
 *
 * Every connect attempt routes through `ConnectDialog` (see that component's doc for why: there
 * is no "already consented" signal on the wire `McpServer` entity for this tab to gate a
 * dialog-vs-direct-connect choice on, and `trustGrantConsent` is idempotent, so always confirming
 * is both simpler and honest). `mcpDisconnect` IS called directly — Phase 1's trust choke-point
 * only gates connect/tool-call (spec D10), not disconnect.
 *
 * Honest degradation (spec §8/§10): every mutating control (add-server submit, enable/disable,
 * connect, disconnect, set-bearer, delete) is `disabled` while the store's `orchdDown` is `true` —
 * `ExtPanel` owns the shared `<OrchdDownBanner/>`, this tab only owns disabling its own controls.
 */
export function ServersTab(): JSX.Element {
  const servers = useAppStore((s) => s.mcpServers);
  const orchdDown = useAppStore((s) => s.orchdDown);
  const refreshMcpServers = useAppStore((s) => s.refreshMcpServers);
  const showToast = useAppStore((s) => s.showToast);
  const { submitting, guard } = useSubmitGuard();

  const [name, setName] = useState("");
  const [url, setUrl] = useState("");
  const [scope, setScope] = useState<McpScope>("global");
  const [authKind, setAuthKind] = useState<McpAuthKind>("none");
  const [connectTarget, setConnectTarget] = useState<McpServer | null>(null);
  const [bearerDrafts, setBearerDrafts] = useState<Record<string, string>>({});

  const addBlocked = name.trim() === "" || url.trim() === "";

  async function handleAdd(): Promise<void> {
    if (addBlocked) return;
    try {
      await mcpAddServer(
        name.trim(),
        "http",
        url.trim(),
        null,
        null,
        null,
        scope,
        null,
        authKind,
        null,
        null,
      );
      setName("");
      setUrl("");
      setScope("global");
      setAuthKind("none");
      await refreshMcpServers();
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

  // Double-submit guard (spec D6): a rapid second "+ server" click must NOT create a duplicate
  // server (cross-cutting P-19).
  const submitAdd = guard(handleAdd);

  async function handleToggleEnabled(server: McpServer): Promise<void> {
    try {
      await mcpSetServerEnabled(server.id, !server.enabled);
      await refreshMcpServers();
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

  async function handleDisconnect(server: McpServer): Promise<void> {
    try {
      await mcpDisconnect(server.id);
      await refreshMcpServers();
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

  async function handleDelete(server: McpServer): Promise<void> {
    if (!window.confirm(strings.ext.servers.deleteConfirm(server.name))) return;
    try {
      await mcpDeleteServer(server.id);
      await refreshMcpServers();
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

  async function handleSetBearer(server: McpServer): Promise<void> {
    const token = (bearerDrafts[server.id] ?? "").trim();
    if (token === "") return;
    try {
      await mcpSetServerBearer(server.id, token);
      // Never keep the token in local state once submitted — the input is cleared, matching
      // "masked, never echoed back" (spec §8): even THIS component never re-displays it.
      setBearerDrafts((prev) => ({ ...prev, [server.id]: "" }));
      showToast(strings.ext.servers.tokenSaved);
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

  return (
    <div data-testid="servers-tab">
      <div style={createFormStyle}>
        <Input
          data-testid="server-create-name"
          aria-label={strings.ext.servers.nameAria}
          placeholder={strings.ext.servers.namePlaceholder}
          value={name}
          onChange={(e) => setName(e.target.value)}
          style={createInputStyle}
        />
        <Select
          data-testid="server-create-transport"
          aria-label={strings.ext.servers.transportAria}
          value="http"
          disabled
          style={selectStyle}
        >
          <option value="http">HTTP</option>
          <option value="stdio">{strings.ext.servers.stdioSoon}</option>
        </Select>
        <Input
          data-testid="server-create-url"
          aria-label={strings.ext.servers.urlAria}
          placeholder="https://…/mcp"
          value={url}
          onChange={(e) => setUrl(e.target.value)}
          style={createInputStyle}
        />
        <Select
          data-testid="server-create-scope"
          aria-label={strings.ext.servers.scopeAria}
          value={scope}
          onChange={(e) => setScope(e.target.value as McpScope)}
          style={selectStyle}
        >
          <option value="global">{strings.common.scope.global}</option>
          <option value="project" disabled>
            {strings.ext.projectSoon}
          </option>
        </Select>
        <Select
          data-testid="server-create-auth"
          aria-label={strings.ext.servers.authAria}
          value={authKind}
          onChange={(e) => setAuthKind(e.target.value as McpAuthKind)}
          style={selectStyle}
        >
          <option value="none">{AUTH_LABEL.none}</option>
          <option value="bearer">{AUTH_LABEL.bearer}</option>
          <option value="oauth" disabled>
            {AUTH_LABEL.oauth}
          </option>
        </Select>
        <Button
          type="button"
          variant="primary"
          size="sm"
          data-testid="server-create-submit"
          disabled={orchdDown || addBlocked || submitting}
          onClick={() => void submitAdd()}
        >
          {strings.ext.servers.addServer}
        </Button>
      </div>

      {servers.length === 0 ? (
        <EmptyState data-testid="servers-empty" title={strings.ext.servers.empty} />
      ) : (
        <div role="list">
          {servers.map((server) => (
            <div key={server.id} data-testid={`server-row-${server.id}`} role="listitem" style={rowStyle}>
              <span
                data-testid={`server-enabled-dot-${server.id}`}
                style={{
                  ...dotStyle,
                  background: server.enabled ? "var(--ok)" : "var(--muted)",
                }}
              />
              <span data-testid={`server-name-${server.id}`} style={titleTextStyle}>
                {server.name}
              </span>
              <span style={metaStyle}>{server.transport}</span>
              <span style={metaStyle}>
                {server.scope === "global" ? strings.common.scope.global : strings.common.scope.project}
              </span>
              <span data-testid={`server-status-${server.id}`} style={metaStyle}>
                {server.protocolVersion !== null
                  ? strings.ext.servers.protocol(server.protocolVersion)
                  : strings.ext.servers.notConnected}
              </span>
              <Button
                type="button"
                variant="ghost"
                size="sm"
                data-testid={`server-toggle-enabled-${server.id}`}
                disabled={orchdDown}
                onClick={() => void handleToggleEnabled(server)}
              >
                {server.enabled ? strings.ext.servers.disable : strings.ext.servers.enable}
              </Button>
              <Button
                type="button"
                variant="ghost"
                size="sm"
                data-testid={`server-connect-${server.id}`}
                disabled={orchdDown}
                onClick={() => setConnectTarget(server)}
              >
                {strings.ext.servers.connect}
              </Button>
              <Button
                type="button"
                variant="ghost"
                size="sm"
                data-testid={`server-disconnect-${server.id}`}
                disabled={orchdDown}
                onClick={() => void handleDisconnect(server)}
              >
                {strings.ext.servers.disconnect}
              </Button>
              <Input
                type="password"
                data-testid={`server-bearer-input-${server.id}`}
                aria-label={strings.ext.servers.tokenFor(server.name)}
                placeholder={strings.ext.servers.bearerPlaceholder}
                disabled={orchdDown}
                value={bearerDrafts[server.id] ?? ""}
                onChange={(e) =>
                  setBearerDrafts((prev) => ({ ...prev, [server.id]: e.target.value }))
                }
                style={bearerInputStyle}
              />
              <Button
                type="button"
                variant="ghost"
                size="sm"
                data-testid={`server-bearer-submit-${server.id}`}
                disabled={orchdDown || (bearerDrafts[server.id] ?? "").trim() === ""}
                onClick={() => void handleSetBearer(server)}
              >
                {strings.ext.servers.setToken}
              </Button>
              <Button
                type="button"
                variant="danger"
                size="sm"
                data-testid={`server-delete-${server.id}`}
                disabled={orchdDown}
                onClick={() => void handleDelete(server)}
              >
                {strings.ext.delete}
              </Button>
            </div>
          ))}
        </div>
      )}

      {connectTarget && (
        <ConnectDialog server={connectTarget} onClose={() => setConnectTarget(null)} />
      )}
    </div>
  );
}
