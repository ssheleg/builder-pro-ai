import { useEffect, useRef, useState, type CSSProperties, type JSX } from "react";
import { useAppStore } from "../../store/store";
import { trustGrantConsent, mcpConnect, describeOrchdError } from "../../ipc/orchd";
import type { McpServer } from "../../ipc/orchd-types";
import { theme } from "../../theme";

const overlayStyle: CSSProperties = {
  position: "fixed",
  inset: 0,
  background: "rgba(1, 4, 9, 0.6)",
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
  zIndex: 1000,
};

const cardStyle: CSSProperties = {
  width: 380,
  background: theme.colors.bgElevated,
  border: `1px solid ${theme.colors.border}`,
  borderRadius: 10,
  boxShadow: theme.shadow,
  padding: 16,
  display: "flex",
  flexDirection: "column",
  gap: 12,
};

const titleStyle: CSSProperties = {
  fontSize: 15,
  fontWeight: 600,
  color: theme.colors.text,
};

const secondaryButtonStyle: CSSProperties = {
  padding: "6px 12px",
  borderRadius: 6,
  border: `1px solid ${theme.colors.border}`,
  background: "transparent",
  color: theme.colors.text,
  fontSize: 13,
  cursor: "pointer",
};

const primaryButtonStyle: CSSProperties = {
  padding: "6px 12px",
  borderRadius: 6,
  border: "none",
  background: theme.colors.accent,
  color: theme.colors.text,
  fontSize: 13,
  fontWeight: 600,
  cursor: "pointer",
};

const inlineErrorStyle: CSSProperties = {
  fontSize: 13,
  lineHeight: 1.5,
  color: theme.colors.statusExited,
  borderLeft: `3px solid ${theme.colors.statusExited}`,
  paddingLeft: 8,
};

/**
 * First-connect consent dialog (S-EXT §8/§10 D10 trust choke-point, T8). Design-system "Dialog /
 * modal overlay" atom, mirrors `CreateProjectDialog`/`UpgradeDialog` byte-for-byte (dim backdrop +
 * centered `bgElevated` card, `role="dialog"` + `aria-modal` + labelled title, focus the primary
 * button on open, `Escape` runs the same cancel path as the Cancel button).
 *
 * Shows the server's endpoint (its `url` for an http server; `command` for a future stdio one —
 * Phase 1 ships http only, spec D6) so the owner can see exactly what they are about to let the
 * app talk to. «Подключиться» runs `trustGrantConsent(serverId, "connect")` THEN `mcpConnect(id)`
 * — in that order, since `mcpConnect` is trust-gated and rejects with `Error{Consent}` until a
 * grant exists (spec D10). `trustGrantConsent` is idempotent (`Db::grant_consent` upserts on
 * `(server_id, kind)`), so this dialog is safe to show again for a server the owner already
 * consented to (e.g. after a URL change re-triggers the fingerprint mismatch, spec D10
 * "re-prompt if the URL changes") — there is no separate "already consented" signal on the wire
 * `McpServer` entity for the frontend to gate on, so `ServersTab` always routes every connect
 * attempt through this dialog rather than guessing.
 *
 * A failure (network, consent, policy — `describeOrchdError`) is shown IN-DIALOG (`role="alert"`,
 * survives a concurrent toast clobbering the global queue-of-one) and the dialog STAYS OPEN so the
 * owner can retry, mirroring `CreateProjectDialog`'s `createError` contract. Success closes the
 * dialog and re-fetches `mcpServers` (belt-and-suspenders with the `orchd://mcp-servers-changed`
 * push `TrustGrantConsent` fires — see `broker.rs`).
 */
export function ConnectDialog(props: { server: McpServer; onClose: () => void }): JSX.Element {
  const { server, onClose } = props;

  const showToast = useAppStore((s) => s.showToast);
  const refreshMcpServers = useAppStore((s) => s.refreshMcpServers);

  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const confirmRef = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    confirmRef.current?.focus();
    const onKeyDown = (e: KeyboardEvent): void => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onClose]);

  async function handleConfirm(): Promise<void> {
    setError(null);
    setBusy(true);
    try {
      await trustGrantConsent(server.id, "connect");
      await mcpConnect(server.id);
      await refreshMcpServers();
      onClose();
    } catch (e) {
      const message = describeOrchdError(e);
      setError(message);
      showToast(message);
    } finally {
      setBusy(false);
    }
  }

  const endpoint = server.url ?? server.command ?? "";

  return (
    <div style={overlayStyle}>
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="connect-dialog-title"
        data-testid="connect-dialog"
        style={cardStyle}
      >
        <div id="connect-dialog-title" style={titleStyle}>
          Подключиться к серверу «{server.name}»
        </div>

        <div data-testid="connect-dialog-url" style={{ fontSize: 13, color: theme.colors.textDim }}>
          {endpoint}
        </div>

        <div style={{ fontSize: 12, color: theme.colors.textDim }}>
          Приложение подключится к этому MCP-серверу и получит доступ к его инструментам.
        </div>

        {error !== null && (
          <div role="alert" data-testid="connect-dialog-error" style={inlineErrorStyle}>
            {error}
          </div>
        )}

        <div style={{ display: "flex", justifyContent: "flex-end", gap: 8, marginTop: 4 }}>
          <button
            type="button"
            data-testid="connect-dialog-cancel"
            onClick={onClose}
            style={secondaryButtonStyle}
          >
            Отмена
          </button>
          <button
            ref={confirmRef}
            type="button"
            data-testid="connect-dialog-confirm"
            disabled={busy}
            onClick={() => void handleConfirm()}
            style={{ ...primaryButtonStyle, opacity: busy ? 0.6 : 1 }}
          >
            Подключиться
          </button>
        </div>
      </div>
    </div>
  );
}
