import { useEffect, useState, type CSSProperties, type JSX } from "react";
import { useAppStore } from "../../store/store";
import { mcpSetToolEnabled, mcpCallTool, describeOrchdError } from "../../ipc/orchd";
import type { McpTool } from "../../ipc/orchd-types";
import { theme } from "../../theme";

const MONO_FONT = 'ui-monospace, SFMono-Regular, "SF Mono", Menlo, monospace';

const toolRowStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: 4,
  padding: "8px 12px",
  marginBottom: 8,
  border: `1px solid ${theme.colors.border}`,
  borderRadius: 8,
};

const headerRowStyle: CSSProperties = {
  display: "flex",
  justifyContent: "space-between",
  alignItems: "flex-start",
  gap: 8,
};

const metaTextStyle: CSSProperties = {
  fontSize: 11,
  fontFamily: MONO_FONT,
  color: theme.colors.textDim,
};

const descTextStyle: CSSProperties = {
  fontSize: 12,
  color: theme.colors.text,
};

const schemaStyle: CSSProperties = {
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
};

const invokeRowStyle: CSSProperties = {
  display: "flex",
  gap: 6,
  alignItems: "flex-start",
};

const textareaStyle: CSSProperties = {
  flex: 1,
  minWidth: 0,
  fontFamily: MONO_FONT,
  fontSize: 11,
  color: theme.colors.text,
  background: theme.colors.bg,
  border: `1px solid ${theme.colors.border}`,
  borderRadius: 4,
  padding: "3px 6px",
  resize: "vertical",
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
  alignSelf: "flex-start",
};

const inlineErrorStyle: CSSProperties = {
  fontSize: 12,
  lineHeight: 1.4,
  color: theme.colors.statusExited,
  borderLeft: `3px solid ${theme.colors.statusExited}`,
  paddingLeft: 8,
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

interface ToolCallResult {
  contentJson: string;
  isError: boolean;
}

/**
 * «Инструменты» tab (S-EXT §8, T8): tools across every registered server's cached tool list
 * (`mcpToolsByServer`, fetched via `refreshMcpTools` — this component eagerly fetches any server's
 * list it hasn't cached yet, on mount and whenever the server set changes). Per tool:
 * name/description/input-schema (collapsed behind a `<details>` — the schema is often long JSON,
 * design-system §1 "detail is one drill-down away"), an enable/disable toggle
 * (`mcpSetToolEnabled` — the per-tool allowlist, spec §6), and a "вызвать" form (a JSON args
 * textarea, disabled for a disabled tool per spec §8) that calls `mcpCallTool` and renders the
 * result with an «непроверенные данные» banner — EVERY `mcp_artifact` this slice creates is
 * `is_untrusted:true` by construction (spec D9), so the banner is unconditional, not derived from
 * anything the server itself claims.
 *
 * Args validation: the textarea's content must parse as JSON before `mcpCallTool` is called (an
 * empty textarea defaults to `"{}"`) — an invalid-JSON draft shows an inline error and never
 * reaches the wire, rather than sending a malformed `argsJson` the daemon would reject anyway.
 *
 * Honest degradation (spec §8/§10): the enable toggle and "вызвать" are `disabled` while the
 * store's `orchdDown` is `true` (mirrors `TasksList`'s per-row `disabled` composition) — `ExtPanel`
 * owns the shared `<OrchdDownBanner/>`.
 */
export function ToolsBrowser(): JSX.Element {
  const servers = useAppStore((s) => s.mcpServers);
  const toolsByServer = useAppStore((s) => s.mcpToolsByServer);
  const orchdDown = useAppStore((s) => s.orchdDown);
  const refreshMcpTools = useAppStore((s) => s.refreshMcpTools);
  const showToast = useAppStore((s) => s.showToast);

  const [argsDraft, setArgsDraft] = useState<Record<string, string>>({});
  const [callError, setCallError] = useState<Record<string, string | null>>({});
  const [result, setResult] = useState<Record<string, ToolCallResult | undefined>>({});

  const serverIds = servers.map((s) => s.id).join(",");

  useEffect(() => {
    for (const server of servers) {
      if (!(server.id in toolsByServer)) void refreshMcpTools(server.id);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [serverIds]);

  async function handleToggle(tool: McpTool): Promise<void> {
    try {
      await mcpSetToolEnabled(tool.id, !tool.enabled);
      await refreshMcpTools(tool.serverId);
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

  async function handleCall(tool: McpTool): Promise<void> {
    const raw = (argsDraft[tool.id] ?? "").trim();
    const argsJson = raw === "" ? "{}" : raw;
    try {
      JSON.parse(argsJson);
    } catch {
      setCallError((prev) => ({ ...prev, [tool.id]: "аргументы должны быть валидным JSON" }));
      return;
    }
    setCallError((prev) => ({ ...prev, [tool.id]: null }));
    try {
      const res = await mcpCallTool(tool.serverId, tool.name, argsJson, null);
      setResult((prev) => ({
        ...prev,
        [tool.id]: { contentJson: res.contentJson, isError: res.isError },
      }));
    } catch (e) {
      const message = describeOrchdError(e);
      setCallError((prev) => ({ ...prev, [tool.id]: message }));
      showToast(message);
    }
  }

  const rows = servers.flatMap((server) =>
    (toolsByServer[server.id] ?? []).map((tool) => ({ server, tool })),
  );

  return (
    <div data-testid="tools-browser">
      {rows.length === 0 ? (
        <div data-testid="tools-empty" style={{ color: theme.colors.textDim, fontSize: 12 }}>
          нет инструментов
        </div>
      ) : (
        rows.map(({ server, tool }) => {
          const callDisabled = orchdDown || !tool.enabled;
          const toolResult = result[tool.id];
          return (
            <div key={tool.id} data-testid={`tool-row-${tool.id}`} style={toolRowStyle}>
              <div style={headerRowStyle}>
                <div>
                  <div style={{ fontWeight: 600, fontSize: 13 }}>{tool.title ?? tool.name}</div>
                  <div style={metaTextStyle}>
                    {server.name} · {tool.name}
                  </div>
                  {tool.description !== null && tool.description !== "" && (
                    <div style={descTextStyle}>{tool.description}</div>
                  )}
                </div>
                <label style={{ display: "flex", alignItems: "center", gap: 4, fontSize: 11 }}>
                  <input
                    type="checkbox"
                    data-testid={`tool-enabled-${tool.id}`}
                    aria-label={`Включить ${tool.name}`}
                    checked={tool.enabled}
                    disabled={orchdDown}
                    onChange={() => void handleToggle(tool)}
                  />
                  включён
                </label>
              </div>

              <details>
                <summary style={{ fontSize: 11, color: theme.colors.textDim, cursor: "pointer" }}>
                  схема
                </summary>
                <pre data-testid={`tool-schema-${tool.id}`} style={schemaStyle}>
                  {tool.inputSchemaJson}
                </pre>
              </details>

              <div style={invokeRowStyle}>
                <textarea
                  data-testid={`tool-args-${tool.id}`}
                  aria-label={`Аргументы для ${tool.name}`}
                  placeholder="{}"
                  disabled={callDisabled}
                  value={argsDraft[tool.id] ?? ""}
                  onChange={(e) => setArgsDraft((prev) => ({ ...prev, [tool.id]: e.target.value }))}
                  rows={2}
                  style={textareaStyle}
                />
                <button
                  type="button"
                  data-testid={`tool-call-${tool.id}`}
                  disabled={callDisabled}
                  onClick={() => void handleCall(tool)}
                  style={textButtonStyle}
                >
                  вызвать
                </button>
              </div>

              {callError[tool.id] != null && (
                <div role="alert" data-testid={`tool-call-error-${tool.id}`} style={inlineErrorStyle}>
                  {callError[tool.id]}
                </div>
              )}

              {toolResult && (
                <div data-testid={`tool-result-${tool.id}`} style={{ display: "flex", flexDirection: "column", gap: 4 }}>
                  <span data-testid={`tool-result-untrusted-${tool.id}`} style={untrustedBannerStyle}>
                    ⚠ непроверенные данные
                  </span>
                  {toolResult.isError && (
                    <span style={{ fontSize: 12, color: theme.colors.statusExited }}>
                      инструмент вернул ошибку
                    </span>
                  )}
                  <pre style={schemaStyle}>{toolResult.contentJson}</pre>
                </div>
              )}
            </div>
          );
        })
      )}
    </div>
  );
}
