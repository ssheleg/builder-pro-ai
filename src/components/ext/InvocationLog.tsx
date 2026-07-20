import { useEffect, useState, type CSSProperties, type JSX } from "react";
import { useAppStore } from "../../store/store";
import { trustSetPolicy, describeOrchdError } from "../../ipc/orchd";
import type { McpInvocation, Policy, PolicyScope } from "../../ipc/orchd-types";
import { useSubmitGuard } from "../../hooks/useSubmitGuard";
import { Badge, Button, Input, Select, Panel, EmptyState } from "../../ui/primitives";
import { strings } from "../../strings";

const tableStyle: CSSProperties = {
  width: "100%",
  borderCollapse: "collapse",
  fontFamily: "var(--font-mono)",
  fontSize: "var(--fs-xs)",
  fontVariantNumeric: "tabular-nums",
};

const thStyle: CSSProperties = {
  textAlign: "left",
  padding: "var(--sp-1) var(--sp-2)",
  borderBottom: "1px solid var(--hairline)",
  background: "var(--panel-2)",
  color: "var(--muted)",
  fontWeight: 600,
  whiteSpace: "nowrap",
};

const thNumStyle: CSSProperties = {
  ...thStyle,
  textAlign: "right",
};

const tdStyle: CSSProperties = {
  padding: "var(--sp-1) var(--sp-2)",
  borderBottom: "1px solid var(--hairline)",
  color: "var(--ink)",
  whiteSpace: "nowrap",
  overflow: "hidden",
  textOverflow: "ellipsis",
  maxWidth: 220,
};

const tdNumStyle: CSSProperties = {
  ...tdStyle,
  textAlign: "right",
};

const createFormStyle: CSSProperties = {
  display: "flex",
  flexWrap: "wrap",
  gap: "var(--sp-2)",
  marginBottom: "var(--sp-3)",
  alignItems: "center",
};

const createInputStyle: CSSProperties = {
  flex: "1 1 140px",
  minWidth: 0,
};

const selectStyle: CSSProperties = {
  flexShrink: 0,
};

const SCOPE_LABEL: Record<PolicyScope, string> = {
  global: strings.common.scope.global,
  project: strings.common.scope.project,
  server: strings.common.scope.server,
};

function formatTimestamp(ms: number): string {
  return new Date(ms).toLocaleString();
}

function sourceLabel(inv: McpInvocation, serverNames: Record<string, string>): string {
  if (inv.serverId !== null) return serverNames[inv.serverId] ?? inv.serverId;
  if (inv.accountId !== null) return inv.accountId;
  return "—";
}

/**
 * Log tab (S-EXT §8, T18): the invocation log (`mcpListInvocations`, spec §5) + the
 * append-only audit log (`trustListAudit`, spec §4/§6, BL-22) + a spend/rate policy-cap editor
 * (`trustListPolicies`/`trustSetPolicy`).
 *
 * On mount, eagerly `refreshInvocations()`/`refreshAuditRows()`/`refreshPolicies()` (mirrors
 * `ExtPanel`'s own mount-fetch discipline for `mcpServers` — spec §10 "honest state, always").
 * Re-fetches invocations on `orchd://mcp-invocation-logged` (bound in `App.tsx`) and policies on
 * `orchd://policies-changed`.
 *
 * The policy editor's `trustSetPolicy` control is `disabled={orchdDown}` (spec §8 honest
 * degradation, mirrors every other mutating control in this panel) — the invocation/audit tables
 * are read-only and unaffected by `orchdDown`.
 */
export function InvocationLog(): JSX.Element {
  const invocations = useAppStore((s) => s.invocations);
  const auditRows = useAppStore((s) => s.auditRows);
  const policies = useAppStore((s) => s.policies);
  const mcpServers = useAppStore((s) => s.mcpServers);
  const orchdDown = useAppStore((s) => s.orchdDown);
  const refreshInvocations = useAppStore((s) => s.refreshInvocations);
  const refreshAuditRows = useAppStore((s) => s.refreshAuditRows);
  const refreshPolicies = useAppStore((s) => s.refreshPolicies);
  const showToast = useAppStore((s) => s.showToast);
  const { submitting, guard } = useSubmitGuard();

  const [scope, setScope] = useState<PolicyScope>("global");
  const [refId, setRefId] = useState("");
  const [spendCapUsd, setSpendCapUsd] = useState("");
  const [ratePerMin, setRatePerMin] = useState("");

  useEffect(() => {
    void refreshInvocations();
    void refreshAuditRows();
    void refreshPolicies();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const serverNames: Record<string, string> = {};
  for (const s of mcpServers) serverNames[s.id] = s.name;

  const refIdRequired = scope !== "global";
  const setBlocked = refIdRequired && refId.trim() === "";

  async function handleSetPolicy(): Promise<void> {
    if (setBlocked) return;
    const spend = spendCapUsd.trim() === "" ? null : Number(spendCapUsd);
    const rate = ratePerMin.trim() === "" ? null : Number(ratePerMin);
    if ((spend !== null && Number.isNaN(spend)) || (rate !== null && Number.isNaN(rate))) {
      showToast(strings.ext.log.limitMustBeNumber);
      return;
    }
    try {
      await trustSetPolicy(scope, refIdRequired ? refId.trim() : null, spend, rate);
      setRefId("");
      setSpendCapUsd("");
      setRatePerMin("");
      await refreshPolicies();
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

  // Double-submit guard (spec D6): a rapid second "set limit" click must NOT double-apply the
  // policy (cross-cutting P-19).
  const submitSetPolicy = guard(handleSetPolicy);

  return (
    <div data-testid="invocation-log" style={{ display: "flex", flexDirection: "column", gap: "var(--sp-4)" }}>
      <Panel title={strings.ext.log.limitsTitle}>
        <div style={createFormStyle}>
          <Select
            data-testid="policy-scope"
            aria-label={strings.ext.log.scopeAria}
            value={scope}
            onChange={(e) => setScope(e.target.value as PolicyScope)}
            style={selectStyle}
          >
            <option value="global">{strings.common.scope.global}</option>
            <option value="project">{strings.common.scope.project}</option>
            <option value="server">{strings.common.scope.server}</option>
          </Select>
          <Input
            data-testid="policy-ref-id"
            aria-label={strings.ext.log.refIdAria}
            placeholder={scope === "global" ? strings.ext.log.refIdNotRequired : strings.ext.log.refIdPlaceholder}
            value={refId}
            disabled={scope === "global"}
            onChange={(e) => setRefId(e.target.value)}
            style={createInputStyle}
          />
          <Input
            data-testid="policy-spend-cap"
            aria-label={strings.ext.log.spendCapAria}
            placeholder={strings.ext.log.spendCapPlaceholder}
            value={spendCapUsd}
            onChange={(e) => setSpendCapUsd(e.target.value)}
            style={createInputStyle}
          />
          <Input
            data-testid="policy-rate-per-min"
            aria-label={strings.ext.log.ratePerMinAria}
            placeholder={strings.ext.log.ratePerMinPlaceholder}
            value={ratePerMin}
            onChange={(e) => setRatePerMin(e.target.value)}
            style={createInputStyle}
          />
          <Button
            type="button"
            variant="primary"
            size="sm"
            data-testid="policy-set-submit"
            disabled={orchdDown || setBlocked || submitting}
            onClick={() => void submitSetPolicy()}
          >
            {strings.ext.log.setLimit}
          </Button>
        </div>

        {policies.length === 0 ? (
          <EmptyState data-testid="policies-empty" title={strings.ext.log.noLimits} />
        ) : (
          <table style={tableStyle}>
            <thead>
              <tr>
                <th style={thStyle}>{strings.ext.log.thScope}</th>
                <th style={thStyle}>id</th>
                <th style={thNumStyle}>{strings.ext.log.thCap}</th>
                <th style={thNumStyle}>{strings.ext.log.thRate}</th>
              </tr>
            </thead>
            <tbody>
              {policies.map((p: Policy) => (
                <tr key={p.id} data-testid={`policy-row-${p.id}`}>
                  <td style={tdStyle}>{SCOPE_LABEL[p.scope]}</td>
                  <td style={tdStyle}>{p.refId ?? "—"}</td>
                  <td style={tdNumStyle}>{p.spendCapUsd ?? "—"}</td>
                  <td style={tdNumStyle}>{p.ratePerMin ?? "—"}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </Panel>

      <Panel title={strings.ext.log.callsTitle}>
        {invocations.length === 0 ? (
          <EmptyState data-testid="invocations-empty" title={strings.ext.log.noCalls} />
        ) : (
          <table style={tableStyle}>
            <thead>
              <tr>
                <th style={thStyle}>{strings.ext.log.thSource}</th>
                <th style={thStyle}>{strings.ext.log.thTool}</th>
                <th style={thStyle}>{strings.ext.log.thStatus}</th>
                <th style={thNumStyle}>{strings.ext.log.thLatency}</th>
                <th style={thNumStyle}>{strings.ext.log.thCost}</th>
                <th style={thStyle}>{strings.ext.log.thTime}</th>
              </tr>
            </thead>
            <tbody>
              {invocations.map((inv) => (
                <tr key={inv.id} data-testid={`invocation-row-${inv.id}`}>
                  <td style={tdStyle}>{sourceLabel(inv, serverNames)}</td>
                  <td style={tdStyle}>{inv.toolName}</td>
                  <td style={tdStyle}>
                    <Badge
                      data-testid={`invocation-status-${inv.id}`}
                      tone={inv.ok ? "ok" : "danger"}
                    >
                      {inv.ok ? "ok" : (inv.errorKind ?? "err")}
                    </Badge>
                  </td>
                  <td style={tdNumStyle}>{inv.latencyMs}</td>
                  <td style={tdNumStyle} data-testid={`invocation-cost-${inv.id}`}>
                    {inv.costUsd ?? "—"}
                  </td>
                  <td style={tdStyle}>{formatTimestamp(inv.startedAt)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </Panel>

      <Panel title={strings.ext.log.auditTitle}>
        {auditRows.length === 0 ? (
          <EmptyState data-testid="audit-rows-empty" title={strings.ext.log.noAudit} />
        ) : (
          <table style={tableStyle}>
            <thead>
              <tr>
                <th style={thStyle}>{strings.ext.log.thAction}</th>
                <th style={thStyle}>{strings.ext.log.thDecision}</th>
                <th style={thStyle}>{strings.ext.log.thReason}</th>
                <th style={thStyle}>{strings.ext.log.thTime}</th>
              </tr>
            </thead>
            <tbody>
              {auditRows.map((row) => (
                <tr key={row.id} data-testid={`audit-row-${row.id}`}>
                  <td style={tdStyle}>{row.action}</td>
                  <td style={tdStyle}>
                    <Badge
                      data-testid={`audit-decision-${row.id}`}
                      tone={row.decision === "allow" ? "ok" : "danger"}
                    >
                      {row.decision}
                    </Badge>
                  </td>
                  <td style={tdStyle}>{row.reason ?? "—"}</td>
                  <td style={tdStyle}>{formatTimestamp(row.at)}</td>
                </tr>
              ))}
            </tbody>
          </table>
        )}
      </Panel>
    </div>
  );
}
