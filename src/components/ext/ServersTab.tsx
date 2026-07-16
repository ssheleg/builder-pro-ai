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
import { ConnectDialog } from "./ConnectDialog";
import { theme } from "../../theme";
import { strings } from "../../strings";

const MONO_FONT = 'ui-monospace, SFMono-Regular, "SF Mono", Menlo, monospace';

const AUTH_LABEL: Record<McpAuthKind, string> = {
  none: strings.ext.servers.authKind.none,
  bearer: strings.ext.servers.authKind.bearer,
  oauth: strings.ext.servers.authKind.oauth,
};

const createFormStyle: CSSProperties = {
  display: "flex",
  flexWrap: "wrap",
  gap: 6,
  padding: "8px 12px",
  marginBottom: 12,
  border: `1px dashed ${theme.colors.border}`,
  borderRadius: 8,
};

const createInputStyle: CSSProperties = {
  flex: "1 1 160px",
  minWidth: 0,
  fontFamily: MONO_FONT,
  fontSize: 12,
  color: theme.colors.text,
  background: theme.colors.bg,
  border: `1px solid ${theme.colors.border}`,
  borderRadius: 4,
  padding: "3px 6px",
};

const selectStyle: CSSProperties = {
  fontFamily: MONO_FONT,
  fontSize: 11,
  color: theme.colors.text,
  background: theme.colors.bg,
  border: `1px solid ${theme.colors.border}`,
  borderRadius: 4,
  padding: "2px 4px",
  flexShrink: 0,
};

const rowStyle: CSSProperties = {
  display: "flex",
  flexWrap: "wrap",
  alignItems: "center",
  gap: 8,
  padding: "6px 8px",
  fontFamily: MONO_FONT,
  fontSize: 12,
  borderBottom: `1px solid ${theme.colors.border}`,
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
  color: theme.colors.text,
  fontWeight: 600,
};

const metaStyle: CSSProperties = {
  color: theme.colors.textDim,
  fontSize: 11,
};

const textButtonStyle: CSSProperties = {
  border: `1px solid ${theme.colors.border}`,
  background: "transparent",
  color: theme.colors.text,
  cursor: "pointer",
  fontSize: 11,
  borderRadius: 4,
  padding: "2px 6px",
  flexShrink: 0,
  whiteSpace: "nowrap",
};

const deleteButtonStyle: CSSProperties = {
  ...textButtonStyle,
  color: theme.colors.statusExited,
  borderColor: theme.colors.statusExited,
};

const primaryButtonStyle: CSSProperties = {
  ...textButtonStyle,
  color: theme.colors.bg,
  background: theme.colors.accent,
  borderColor: theme.colors.accent,
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
        <input
          data-testid="server-create-name"
          aria-label={strings.ext.servers.nameAria}
          placeholder={strings.ext.servers.namePlaceholder}
          value={name}
          onChange={(e) => setName(e.target.value)}
          style={createInputStyle}
        />
        <select
          data-testid="server-create-transport"
          aria-label={strings.ext.servers.transportAria}
          value="http"
          disabled
          style={selectStyle}
        >
          <option value="http">HTTP</option>
          <option value="stdio">{strings.ext.servers.stdioSoon}</option>
        </select>
        <input
          data-testid="server-create-url"
          aria-label={strings.ext.servers.urlAria}
          placeholder="https://…/mcp"
          value={url}
          onChange={(e) => setUrl(e.target.value)}
          style={createInputStyle}
        />
        <select
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
        </select>
        <select
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
        </select>
        <button
          type="button"
          data-testid="server-create-submit"
          disabled={orchdDown || addBlocked}
          onClick={() => void handleAdd()}
          style={{ ...primaryButtonStyle, opacity: addBlocked ? 0.5 : 1 }}
        >
          {strings.ext.servers.addServer}
        </button>
      </div>

      {servers.length === 0 ? (
        <div data-testid="servers-empty" style={{ color: theme.colors.textDim, fontSize: 12 }}>
          {strings.ext.servers.empty}
        </div>
      ) : (
        <div role="list">
          {servers.map((server) => (
            <div key={server.id} data-testid={`server-row-${server.id}`} role="listitem" style={rowStyle}>
              <span
                data-testid={`server-enabled-dot-${server.id}`}
                style={{
                  ...dotStyle,
                  background: server.enabled ? theme.colors.statusRunning : theme.colors.textDim,
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
              <button
                type="button"
                data-testid={`server-toggle-enabled-${server.id}`}
                disabled={orchdDown}
                onClick={() => void handleToggleEnabled(server)}
                style={textButtonStyle}
              >
                {server.enabled ? strings.ext.servers.disable : strings.ext.servers.enable}
              </button>
              <button
                type="button"
                data-testid={`server-connect-${server.id}`}
                disabled={orchdDown}
                onClick={() => setConnectTarget(server)}
                style={textButtonStyle}
              >
                {strings.ext.servers.connect}
              </button>
              <button
                type="button"
                data-testid={`server-disconnect-${server.id}`}
                disabled={orchdDown}
                onClick={() => void handleDisconnect(server)}
                style={textButtonStyle}
              >
                {strings.ext.servers.disconnect}
              </button>
              <input
                type="password"
                data-testid={`server-bearer-input-${server.id}`}
                aria-label={strings.ext.servers.tokenFor(server.name)}
                placeholder={strings.ext.servers.bearerPlaceholder}
                disabled={orchdDown}
                value={bearerDrafts[server.id] ?? ""}
                onChange={(e) =>
                  setBearerDrafts((prev) => ({ ...prev, [server.id]: e.target.value }))
                }
                style={createInputStyle}
              />
              <button
                type="button"
                data-testid={`server-bearer-submit-${server.id}`}
                disabled={orchdDown || (bearerDrafts[server.id] ?? "").trim() === ""}
                onClick={() => void handleSetBearer(server)}
                style={textButtonStyle}
              >
                {strings.ext.servers.setToken}
              </button>
              <button
                type="button"
                data-testid={`server-delete-${server.id}`}
                disabled={orchdDown}
                onClick={() => void handleDelete(server)}
                style={deleteButtonStyle}
              >
                {strings.ext.delete}
              </button>
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
