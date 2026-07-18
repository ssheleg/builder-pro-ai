import { useEffect, useState, type CSSProperties, type JSX } from "react";
import { useAppStore } from "../../store/store";
import { mcpSetToolEnabled, mcpCallTool, describeOrchdError, isConsentError } from "../../ipc/orchd";
import type { McpTool } from "../../ipc/orchd-types";
import { Badge, Button, TextArea, EmptyState } from "../../ui/primitives";
import { strings } from "../../strings";

const toolRowStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: "var(--sp-1)",
  padding: "var(--sp-3)",
  marginBottom: "var(--sp-2)",
  border: "1px solid var(--border)",
  borderRadius: "var(--r-md)",
  background: "var(--panel)",
};

const headerRowStyle: CSSProperties = {
  display: "flex",
  justifyContent: "space-between",
  alignItems: "flex-start",
  gap: "var(--sp-2)",
};

const metaTextStyle: CSSProperties = {
  fontSize: "var(--fs-xs)",
  fontFamily: "var(--font-mono)",
  color: "var(--muted)",
};

const descTextStyle: CSSProperties = {
  fontSize: "var(--fs-sm)",
  color: "var(--ink)",
};

const schemaStyle: CSSProperties = {
  fontFamily: "var(--font-mono)",
  fontSize: "var(--fs-xs)",
  color: "var(--muted)",
  background: "var(--panel-2)",
  border: "1px solid var(--border)",
  borderRadius: "var(--r-sm)",
  padding: "var(--sp-2)",
  margin: 0,
  whiteSpace: "pre-wrap",
  wordBreak: "break-all",
};

const invokeRowStyle: CSSProperties = {
  display: "flex",
  gap: "var(--sp-2)",
  alignItems: "flex-start",
};

const textareaStyle: CSSProperties = {
  flex: 1,
  minWidth: 0,
  fontFamily: "var(--font-mono)",
  fontSize: "var(--fs-xs)",
};

const inlineErrorStyle: CSSProperties = {
  fontSize: "var(--fs-sm)",
  lineHeight: 1.4,
  color: "var(--danger)",
  borderLeft: "3px solid var(--danger)",
  paddingLeft: "var(--sp-2)",
};

interface ToolCallResult {
  contentJson: string;
  isError: boolean;
}

/** Human message for a rejected orchd call, with the consent-recovery hint appended when the
 * rejection is a `Consent` denial (P-20) — `ConnectDialog` is only reachable from the Servers tab,
 * so a bare "consent required" toast would dead-end. Shared by the toggle and invoke handlers. */
function describeWithRecovery(e: unknown): string {
  const message = describeOrchdError(e);
  return isConsentError(e) ? `${message} ${strings.errors.consentRecovery}` : message;
}

/**
 * Tools tab (S-EXT §8, T8): tools across every registered server's cached tool list
 * (`mcpToolsByServer`, fetched via `refreshMcpTools` — this component eagerly fetches any server's
 * list it hasn't cached yet, on mount and whenever the server set changes). Per tool:
 * name/description/input-schema (collapsed behind a `<details>` — the schema is often long JSON,
 * design-system §1 "detail is one drill-down away"), an enable/disable toggle
 * (`mcpSetToolEnabled` — the per-tool allowlist, spec §6), and an "invoke" form (a JSON args
 * textarea, disabled for a disabled tool per spec §8) that calls `mcpCallTool` and renders the
 * result with an "unverified data" banner — EVERY `mcp_artifact` this slice creates is
 * `is_untrusted:true` by construction (spec D9), so the banner is unconditional, not derived from
 * anything the server itself claims.
 *
 * Args validation: the textarea's content must parse as JSON before `mcpCallTool` is called (an
 * empty textarea defaults to `"{}"`) — an invalid-JSON draft shows an inline error and never
 * reaches the wire, rather than sending a malformed `argsJson` the daemon would reject anyway.
 *
 * Honest degradation (spec §8/§10): the enable toggle and "invoke" are `disabled` while the
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
  const [toggleError, setToggleError] = useState<Record<string, string | null>>({});
  const [result, setResult] = useState<Record<string, ToolCallResult | undefined>>({});

  const serverIds = servers.map((s) => s.id).join(",");

  useEffect(() => {
    for (const server of servers) {
      if (!(server.id in toolsByServer)) void refreshMcpTools(server.id);
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [serverIds]);

  async function handleToggle(tool: McpTool): Promise<void> {
    setToggleError((prev) => ({ ...prev, [tool.id]: null })); // clear any stale failure first
    try {
      await mcpSetToolEnabled(tool.id, !tool.enabled);
      await refreshMcpTools(tool.serverId);
    } catch (e) {
      // The checkbox is controlled by `tool.enabled` (no optimistic flip), so on a rejection it
      // simply stays at the server value — visually UNCHANGED. Without an explicit on-row signal
      // the failure would be a silent no-flip plus a clobber-prone toast (J-01); surface it inline.
      const message = describeWithRecovery(e);
      setToggleError((prev) => ({ ...prev, [tool.id]: message }));
      showToast(message);
    }
  }

  async function handleCall(tool: McpTool): Promise<void> {
    const raw = (argsDraft[tool.id] ?? "").trim();
    const argsJson = raw === "" ? "{}" : raw;
    try {
      JSON.parse(argsJson);
    } catch {
      setCallError((prev) => ({ ...prev, [tool.id]: strings.common.argsInvalidJson }));
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
      // A stale/URL-changed consent grant surfaces here as a `Consent` denial — append the recovery
      // hint pointing at the Servers-tab connect flow (P-20), the only place consent is re-granted.
      const message = describeWithRecovery(e);
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
        <EmptyState data-testid="tools-empty" title={strings.ext.tools.empty} />
      ) : (
        rows.map(({ server, tool }) => {
          const callDisabled = orchdDown || !tool.enabled;
          const toolResult = result[tool.id];
          return (
            <div key={tool.id} data-testid={`tool-row-${tool.id}`} style={toolRowStyle}>
              <div style={headerRowStyle}>
                <div>
                  <div style={{ fontWeight: 600, fontSize: "var(--fs-md)", color: "var(--ink)" }}>
                    {tool.title ?? tool.name}
                  </div>
                  <div style={metaTextStyle}>
                    {server.name} · {tool.name}
                  </div>
                  {tool.description !== null && tool.description !== "" && (
                    <div style={descTextStyle}>{tool.description}</div>
                  )}
                </div>
                <label
                  style={{
                    display: "flex",
                    alignItems: "center",
                    gap: "var(--sp-1)",
                    fontSize: "var(--fs-xs)",
                    color: "var(--muted)",
                  }}
                >
                  <input
                    type="checkbox"
                    data-testid={`tool-enabled-${tool.id}`}
                    aria-label={strings.ext.tools.enableTool(tool.name)}
                    checked={tool.enabled}
                    disabled={orchdDown}
                    onChange={() => void handleToggle(tool)}
                  />
                  {strings.ext.tools.enabled}
                </label>
              </div>

              {toggleError[tool.id] != null && (
                <div
                  role="alert"
                  data-testid={`tool-toggle-error-${tool.id}`}
                  style={inlineErrorStyle}
                >
                  {toggleError[tool.id]}
                </div>
              )}

              <details>
                <summary style={{ fontSize: "var(--fs-xs)", color: "var(--muted)", cursor: "pointer" }}>
                  {strings.ext.tools.schema}
                </summary>
                <pre data-testid={`tool-schema-${tool.id}`} style={schemaStyle}>
                  {tool.inputSchemaJson}
                </pre>
              </details>

              <div style={invokeRowStyle}>
                <TextArea
                  data-testid={`tool-args-${tool.id}`}
                  aria-label={strings.ext.tools.argsFor(tool.name)}
                  placeholder="{}"
                  disabled={callDisabled}
                  value={argsDraft[tool.id] ?? ""}
                  onChange={(e) => setArgsDraft((prev) => ({ ...prev, [tool.id]: e.target.value }))}
                  rows={2}
                  style={textareaStyle}
                />
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  data-testid={`tool-call-${tool.id}`}
                  disabled={callDisabled}
                  onClick={() => void handleCall(tool)}
                >
                  {strings.ext.invoke}
                </Button>
              </div>

              {callError[tool.id] != null && (
                <div role="alert" data-testid={`tool-call-error-${tool.id}`} style={inlineErrorStyle}>
                  {callError[tool.id]}
                </div>
              )}

              {toolResult && (
                <div data-testid={`tool-result-${tool.id}`} style={{ display: "flex", flexDirection: "column", gap: "var(--sp-1)" }}>
                  <Badge tone="warn" data-testid={`tool-result-untrusted-${tool.id}`}>
                    {strings.ext.unverified}
                  </Badge>
                  {toolResult.isError && (
                    <span style={{ fontSize: "var(--fs-sm)", color: "var(--danger)" }}>
                      {strings.ext.tools.toolError}
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
