import { useEffect, useState, type CSSProperties, type JSX } from "react";
import { useAppStore } from "../../store/store";
import type { McpArtifact } from "../../ipc/orchd-types";
import { theme } from "../../theme";

const MONO_FONT = 'ui-monospace, SFMono-Regular, "SF Mono", Menlo, monospace';

const rowStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: 6,
  padding: "8px 12px",
  marginBottom: 8,
  border: `1px solid ${theme.colors.border}`,
  borderRadius: 8,
};

const rowHeaderStyle: CSSProperties = {
  display: "flex",
  flexWrap: "wrap",
  alignItems: "center",
  gap: 8,
  fontFamily: MONO_FONT,
  fontSize: 12,
};

const titleTextStyle: CSSProperties = {
  color: theme.colors.text,
  fontWeight: 600,
};

const metaStyle: CSSProperties = {
  color: theme.colors.textDim,
  fontSize: 11,
};

const untrustedBannerStyle: CSSProperties = {
  fontSize: 11,
  fontWeight: 600,
  color: theme.colors.statusWaiting,
  border: `1px solid ${theme.colors.statusWaiting}`,
  borderRadius: 4,
  padding: "2px 8px",
  alignSelf: "flex-start",
};

const textButtonStyle: CSSProperties = {
  border: `1px solid ${theme.colors.border}`,
  background: "transparent",
  color: theme.colors.text,
  cursor: "pointer",
  fontSize: 11,
  borderRadius: 4,
  padding: "2px 8px",
  flexShrink: 0,
  whiteSpace: "nowrap",
};

const preStyle: CSSProperties = {
  fontFamily: MONO_FONT,
  fontSize: 11,
  color: theme.colors.textDim,
  background: theme.colors.bg,
  border: `1px solid ${theme.colors.border}`,
  borderRadius: 4,
  padding: 6,
  margin: 0,
  whiteSpace: "pre-wrap",
  wordBreak: "break-all",
  maxHeight: 300,
  overflowY: "auto",
};

function formatTimestamp(ms: number): string {
  return new Date(ms).toLocaleString();
}

/**
 * One artifact's row + expandable read-only content viewer (S-EXT §8, T18; extracted for S-IDEA
 * §7/T6 reuse — `ResearchPane`'s done-run viewer renders THIS component over its own
 * `mcpGetArtifact(artifact_id)` fetch rather than re-implementing the untrusted-banner/content
 * markup). `source` is caller-resolved (server name, connector account id, or any other caller-
 * chosen label) — this component has no opinion on how it was derived. `defaultOpen` (default
 * `false`) mirrors `ArtifactsTab`'s own collapsed-by-default convention; a caller that already
 * fetched the artifact because the owner explicitly asked to see it (`ResearchPane`) passes `true`
 * so the content renders open on first mount, no extra click needed.
 *
 * `isUntrusted` is unconditional per row — EVERY `mcp_artifact` this codebase creates is
 * `is_untrusted:true` by construction (spec D9) — so the banner is rendered off the artifact's own
 * field, never derived from anything the producing server claims.
 */
export function ArtifactViewer(props: {
  artifact: McpArtifact;
  source: string;
  defaultOpen?: boolean;
  /** Optional ARIA role for the single root element. `ArtifactsTab` passes `"listitem"` (its
   * rows sit inside a `role="list"` container) so the list semantics land on THIS one element —
   * no extra wrapper `<div>` — while `ResearchPane` omits it (its viewer is not inside a list).
   * A nested/duplicate `listitem` is invalid a11y, so the role lives here on the root and nowhere
   * else. */
  role?: string;
}): JSX.Element {
  const { artifact, source, defaultOpen = false, role } = props;
  const [isOpen, setIsOpen] = useState(defaultOpen);

  return (
    <div role={role} data-testid={`artifact-row-${artifact.id}`} style={rowStyle}>
      <div style={rowHeaderStyle}>
        <span data-testid={`artifact-tool-${artifact.id}`} style={titleTextStyle}>
          {artifact.toolName}
        </span>
        <span data-testid={`artifact-source-${artifact.id}`} style={metaStyle}>
          {source}
        </span>
        {artifact.projectId !== null && (
          <span style={metaStyle}>проект: {artifact.projectId}</span>
        )}
        <span style={metaStyle}>{formatTimestamp(artifact.createdAt)}</span>
        {artifact.isUntrusted && (
          <span data-testid={`artifact-untrusted-${artifact.id}`} style={untrustedBannerStyle}>
            ⚠ непроверенные данные
          </span>
        )}
        <button
          type="button"
          data-testid={`artifact-toggle-${artifact.id}`}
          onClick={() => setIsOpen((v) => !v)}
          style={textButtonStyle}
        >
          {isOpen ? "скрыть" : "показать содержимое"}
        </button>
      </div>
      {isOpen && (
        <pre data-testid={`artifact-content-${artifact.id}`} style={preStyle}>
          {artifact.contentText ?? artifact.contentJson}
        </pre>
      )}
    </div>
  );
}

/**
 * «Артефакты» tab (S-EXT §8, T18): the durable `mcp_artifact` list (spec §4/§5) — every result
 * from `McpCallTool`/`ConnectorInvoke` persists here, `isUntrusted:true` by construction (spec
 * D9), so the «непроверенные данные» banner is unconditional per row, mirroring
 * `ToolsBrowser`/`ConnectorsTab`'s own result-banner discipline exactly.
 *
 * Reuses the `mcpArtifacts` store slice + `refreshMcpArtifacts` action T8 already shipped (no
 * new store plumbing needed here) — this component is a pure reader/viewer over that slice.
 * `App.tsx` already refreshes it on `orchd://mcp-artifacts-changed`; this component ALSO
 * eagerly re-fetches on its own mount (mirrors `ExtPanel`'s mount-fetch discipline — spec §10
 * "honest state, always": the list must not silently stay stale just because the tab wasn't
 * mounted when the last push fired).
 *
 * Each row expands («показать содержимое») into a read-only viewer showing `contentText` when
 * present (the flattened preview), else the full `contentJson`.
 */
export function ArtifactsTab(): JSX.Element {
  const artifacts = useAppStore((s) => s.mcpArtifacts);
  const mcpServers = useAppStore((s) => s.mcpServers);
  const refreshMcpArtifacts = useAppStore((s) => s.refreshMcpArtifacts);

  useEffect(() => {
    void refreshMcpArtifacts();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const serverNames: Record<string, string> = {};
  for (const s of mcpServers) serverNames[s.id] = s.name;

  return (
    <div data-testid="artifacts-tab">
      {artifacts.length === 0 ? (
        <div data-testid="artifacts-empty" style={{ color: theme.colors.textDim, fontSize: 12 }}>
          нет артефактов
        </div>
      ) : (
        <div role="list">
          {artifacts.map((artifact) => {
            const source =
              artifact.serverId !== null
                ? (serverNames[artifact.serverId] ?? artifact.serverId)
                : (artifact.accountId ?? "—");
            return (
              <ArtifactViewer
                key={artifact.id}
                artifact={artifact}
                source={source}
                role="listitem"
              />
            );
          })}
        </div>
      )}
    </div>
  );
}
