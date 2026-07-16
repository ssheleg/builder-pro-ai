import { useEffect, useRef, useState, type CSSProperties, type JSX } from "react";
import { useAppStore } from "../../store/store";
import { researchStartRun, describeOrchdError } from "../../ipc/orchd";
import type { Idea, Policy } from "../../ipc/orchd-types";
import { useSubmitGuard } from "../../hooks/useSubmitGuard";
import { theme } from "../../theme";
import { strings } from "../../strings";

const MONO_FONT = 'ui-monospace, SFMono-Regular, "SF Mono", Menlo, monospace';

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
  width: 460,
  maxHeight: "85vh",
  overflowY: "auto",
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

const fieldLabelStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: 4,
  fontSize: 12,
  fontWeight: 600,
  color: theme.colors.textDim,
  textTransform: "uppercase",
  letterSpacing: "0.05em",
};

const selectStyle: CSSProperties = {
  fontFamily: "inherit",
  fontSize: 13,
  fontWeight: 400,
  textTransform: "none",
  letterSpacing: "normal",
  color: theme.colors.text,
  background: theme.colors.bg,
  border: `1px solid ${theme.colors.border}`,
  borderRadius: 6,
  padding: "6px 8px",
};

const textareaStyle: CSSProperties = {
  fontFamily: MONO_FONT,
  fontSize: 12,
  color: theme.colors.text,
  background: theme.colors.bg,
  border: `1px solid ${theme.colors.border}`,
  borderRadius: 6,
  padding: "6px 8px",
  resize: "vertical",
  minHeight: 90,
};

const preflightStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: 4,
  fontSize: 12,
  color: theme.colors.textDim,
  border: `1px solid ${theme.colors.border}`,
  borderRadius: 6,
  padding: "8px 10px",
  background: theme.colors.bg,
};

const noteStyle: CSSProperties = {
  fontSize: 11,
  color: theme.colors.statusWaiting,
};

const errorTextStyle: CSSProperties = {
  fontSize: 12,
  color: theme.colors.statusExited,
};

const inlineErrorStyle: CSSProperties = {
  fontSize: 13,
  lineHeight: 1.5,
  color: theme.colors.statusExited,
  borderLeft: `3px solid ${theme.colors.statusExited}`,
  paddingLeft: 8,
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

/** Seed JSON for the args textarea (owner-editable) — a reasonable generic starting point for a
 * research tool, not any particular tool's real schema (the owner picks the tool AFTER the
 * server, so this component has no schema to seed against yet). */
function seedArgs(idea: Idea): string {
  const payload = idea.body.trim() === "" ? { query: idea.title } : { query: idea.title, context: idea.body };
  return JSON.stringify(payload, null, 2);
}

/** Most-specific-wins effective policy for the spend-preflight (mirrors `trust::resolve_policy`'s
 * documented precedence, `orchd-types.ts`'s `PolicyScope` doc: server > project > global). `null`
 * means no policy applies at any scope — an honest "no cap configured" state, not "unlimited"
 * conflated with "unset". */
function effectivePolicy(policies: Policy[], serverId: string, projectId: string | null): Policy | null {
  const server = policies.find((p) => p.scope === "server" && p.refId === serverId);
  if (server) return server;
  if (projectId !== null) {
    const project = policies.find((p) => p.scope === "project" && p.refId === projectId);
    if (project) return project;
  }
  return policies.find((p) => p.scope === "global") ?? null;
}

/**
 * "Research" dialog (S-IDEA spec §7): pick a connected+enabled MCP server → fetch its tools →
 * pick a tool → an owner-editable args JSON field (seeded from the idea's title/body) → a
 * spend-approval preflight (the effective `trustListPolicies` row for the picked server's scope +
 * an honest "cost unknown before the call" note, spec §7) → "Run" fires `researchStartRun`.
 *
 * "Connected" is approximated the same way `ServersTab` does (`server.protocolVersion !== null` —
 * set on first successful `McpConnect`, `null` until then): a server that has never connected has
 * no cached tool list to pick from anyway.
 *
 * Dialog-atom parity with `CreateProjectDialog`/`UpgradeDialog` (design-system.md "Dialog / modal
 * overlay"): overlay + centered card, `role="dialog"`, an in-dialog `role="alert"` failure line
 * that survives a concurrent toast clobbering the global queue-of-one, and the dialog stays open
 * on failure so the owner can retry.
 *
 * Honest degradation (spec §10, T8 discipline): "Run" is `disabled={orchdDown}`
 * independently of whatever gates the button that opened this dialog (`IdeasList`'s own
 * `disabled={orchdDown}` on its "Research" trigger) — this component must never fire
 * `researchStartRun` while the daemon is down even if reached some other way.
 */
export function ResearchRunDialog(props: { idea: Idea; onClose: () => void }): JSX.Element {
  const { idea, onClose } = props;

  const mcpServers = useAppStore((s) => s.mcpServers);
  const mcpToolsByServer = useAppStore((s) => s.mcpToolsByServer);
  const refreshMcpTools = useAppStore((s) => s.refreshMcpTools);
  const policies = useAppStore((s) => s.policies);
  const refreshPolicies = useAppStore((s) => s.refreshPolicies);
  const orchdDown = useAppStore((s) => s.orchdDown);
  const showToast = useAppStore((s) => s.showToast);
  const refreshResearchRuns = useAppStore((s) => s.refreshResearchRuns);
  const { submitting, guard } = useSubmitGuard();

  const [serverId, setServerId] = useState("");
  const [toolName, setToolName] = useState("");
  const [argsDraft, setArgsDraft] = useState(() => seedArgs(idea));
  const [argsError, setArgsError] = useState<string | null>(null);
  const [submitError, setSubmitError] = useState<string | null>(null);

  const nameRef = useRef<HTMLSelectElement>(null);

  useEffect(() => {
    void refreshPolicies();
    nameRef.current?.focus();
    const onKeyDown = (e: KeyboardEvent): void => {
      if (e.key === "Escape") onClose();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [onClose]);

  useEffect(() => {
    if (serverId !== "" && !(serverId in mcpToolsByServer)) void refreshMcpTools(serverId);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [serverId]);

  const connectedServers = mcpServers.filter((s) => s.enabled && s.protocolVersion !== null);
  const tools = mcpToolsByServer[serverId] ?? [];

  function handleServerChange(next: string): void {
    setServerId(next);
    setToolName("");
  }

  async function handleSubmit(): Promise<void> {
    if (serverId === "" || toolName === "") return;
    const raw = argsDraft.trim() === "" ? "{}" : argsDraft;
    try {
      JSON.parse(raw);
    } catch {
      setArgsError(strings.common.argsInvalidJson);
      return;
    }
    setArgsError(null);
    setSubmitError(null);
    try {
      await researchStartRun(idea.id, serverId, toolName, raw);
      await refreshResearchRuns(idea.id);
      showToast(strings.research.runStarted);
      onClose();
    } catch (e) {
      const message = describeOrchdError(e);
      setSubmitError(message);
      showToast(message);
    }
  }

  // Double-submit guard (spec D6): a rapid second "Run" click before `researchStartRun` resolves
  // must NOT start a second run — the cost here is a real external call + double spend (finding
  // F-08).
  const submit = guard(handleSubmit);

  const policy = serverId === "" ? null : effectivePolicy(policies, serverId, idea.projectId);
  const submitBlocked = orchdDown || serverId === "" || toolName === "" || submitting;

  return (
    <div style={overlayStyle}>
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="research-run-title"
        data-testid="research-run-dialog"
        style={cardStyle}
      >
        <div id="research-run-title" style={titleStyle}>
          {strings.research.dialogTitle(idea.title)}
        </div>

        <label style={fieldLabelStyle}>
          {strings.research.serverLabel}
          <select
            ref={nameRef}
            data-testid="research-run-server-select"
            aria-label={strings.research.serverAria}
            value={serverId}
            onChange={(e) => handleServerChange(e.target.value)}
            style={selectStyle}
          >
            <option value="">{strings.research.selectServer}</option>
            {connectedServers.map((s) => (
              <option key={s.id} value={s.id}>
                {s.name}
              </option>
            ))}
          </select>
        </label>

        <label style={fieldLabelStyle}>
          {strings.research.toolLabel}
          <select
            data-testid="research-run-tool-select"
            aria-label={strings.research.toolAria}
            value={toolName}
            disabled={serverId === ""}
            onChange={(e) => setToolName(e.target.value)}
            style={selectStyle}
          >
            <option value="">{strings.research.selectTool}</option>
            {tools.map((t) => (
              <option key={t.id} value={t.name}>
                {t.title ?? t.name}
              </option>
            ))}
          </select>
        </label>

        <label style={fieldLabelStyle}>
          {strings.research.argsLabel}
          <textarea
            data-testid="research-run-args"
            aria-label={strings.research.argsAria}
            value={argsDraft}
            onChange={(e) => {
              setArgsDraft(e.target.value);
              setArgsError(null);
            }}
            style={textareaStyle}
          />
        </label>
        {argsError !== null && (
          <div data-testid="research-run-args-error" role="alert" style={errorTextStyle}>
            {argsError}
          </div>
        )}

        {serverId !== "" && (
          <div style={preflightStyle}>
            <div data-testid="research-run-policy-scope">
              {strings.research.limitScope} {policy === null ? strings.research.notSet : SCOPE_LABEL[policy.scope]}
            </div>
            <div data-testid="research-run-policy-spend-cap">
              {strings.research.spendCap} {policy?.spendCapUsd != null ? `$${policy.spendCapUsd}` : strings.research.notSet}
            </div>
            <div data-testid="research-run-policy-rate">
              {strings.research.callsPerMin} {policy?.ratePerMin ?? strings.research.notSet}
            </div>
            <div data-testid="research-run-policy-note" style={noteStyle}>
              {strings.research.costNote}
            </div>
          </div>
        )}

        {submitError !== null && (
          <div data-testid="research-run-error" role="alert" style={inlineErrorStyle}>
            {submitError}
          </div>
        )}

        <div style={{ display: "flex", justifyContent: "flex-end", gap: 8, marginTop: 4 }}>
          <button
            type="button"
            data-testid="research-run-cancel"
            onClick={onClose}
            style={secondaryButtonStyle}
          >
            {strings.common.cancel}
          </button>
          <button
            type="button"
            data-testid="research-run-submit"
            disabled={submitBlocked}
            onClick={() => void submit()}
            style={{ ...primaryButtonStyle, opacity: submitBlocked ? 0.5 : 1 }}
          >
            {strings.research.run}
          </button>
        </div>
      </div>
    </div>
  );
}

const SCOPE_LABEL: Record<Policy["scope"], string> = {
  global: strings.common.scope.global,
  project: strings.common.scope.project,
  server: strings.common.scope.server,
};
