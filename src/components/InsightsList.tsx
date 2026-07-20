import { useEffect, useState, type CSSProperties, type JSX } from "react";
import { useAppStore } from "../store/store";
import {
  orchdSetInsightFitVerdict,
  orchdSetInsightStatus,
  describeOrchdError,
} from "../ipc/orchd";
import type { FitVerdict, Insight, InsightStatus } from "../ipc/orchd-types";
import { Badge, Button, EmptyState } from "../ui/primitives";
import type { Tone } from "../ui/theme";
import { strings } from "../strings";

const FIT_VERDICT_VALUES: FitVerdict[] = ["fit", "noFit", "unknown"];

const FIT_VERDICT_LABEL: Record<FitVerdict, string> = {
  fit: strings.insights.fitVerdict.fit,
  noFit: strings.insights.fitVerdict.noFit,
  unknown: strings.insights.fitVerdict.unknown,
};

/** Fit-verdict → semantic tone for the read-only badge (owner-facing signal, not a status enum
 * `statusTone` covers): a fit reads as "ok", a non-fit as "danger", unknown/absent as neutral. */
const FIT_VERDICT_TONE: Record<FitVerdict, Tone> = {
  fit: "ok",
  noFit: "danger",
  unknown: "muted",
};

function fitBadgeTone(v: FitVerdict | null): Tone {
  return v === null ? "muted" : FIT_VERDICT_TONE[v];
}

const STATUS_VALUES: InsightStatus[] = ["new", "accepted", "archived"];

const STATUS_LABEL: Record<InsightStatus, string> = {
  new: strings.insights.status.new,
  accepted: strings.insights.status.accepted,
  archived: strings.insights.status.archived,
};

const listStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: "var(--sp-2)",
};

const rowStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: "var(--sp-2)",
  padding: "var(--sp-2) var(--sp-3)",
  fontFamily: "var(--font-ui)",
  fontSize: "var(--fs-sm)",
  borderRadius: "var(--r-md)",
  background: "var(--panel)",
};

const headerRowStyle: CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: "var(--sp-2)",
  flexWrap: "wrap",
};

const titleStyle: CSSProperties = {
  flex: "1 1 auto",
  fontSize: "var(--fs-md)",
  fontWeight: 600,
  color: "var(--ink)",
};

const captionStyle: CSSProperties = {
  fontSize: "var(--fs-xs)",
  color: "var(--muted)",
};

const bodyStyle: CSSProperties = {
  fontSize: "var(--fs-sm)",
  color: "var(--muted)",
};

const selectStyle: CSSProperties = {
  fontFamily: "var(--font-ui)",
  fontSize: "var(--fs-xs)",
  color: "var(--ink)",
  background: "var(--panel-2)",
  border: "none",
  borderRadius: 999,
  padding: "var(--sp-1) var(--sp-2)",
  flexShrink: 0,
};

const inputStyle: CSSProperties = {
  flex: "1 1 160px",
  minWidth: 0,
  fontFamily: "var(--font-ui)",
  fontSize: "var(--fs-sm)",
  color: "var(--ink)",
  background: "var(--panel-2)",
  border: "none",
  borderRadius: "var(--r-sm)",
  padding: "var(--sp-1) var(--sp-2)",
};

const errorTextStyle: CSSProperties = {
  fontSize: "var(--fs-xs)",
  color: "var(--danger)",
};

const rowGroupStyle: CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: "var(--sp-2)",
  flexWrap: "wrap",
};

function fitBadgeText(v: FitVerdict | null): string {
  return v === null ? "—" : FIT_VERDICT_LABEL[v];
}

interface InsightRowProps {
  insight: Insight;
  /** `orchdDown` (spec §10): while `true`, every mutating control on this row is disabled — see
   * `InsightsList`'s own doc comment. */
  disabled: boolean;
  onVerdictApply: (id: string, verdict: FitVerdict | null, reasoning: string) => void;
  onStatusApply: (id: string, status: InsightStatus, resolutionReasoning: string | null) => void;
}

/** One insight row (design-system.md, spec §10). Archiving requires a non-empty
 * `resolutionReasoning` server-side (spec §5.2) — the row collects it inline and blocks the call
 * client-side with an honest message rather than round-tripping a doomed request. */
function InsightRow(props: InsightRowProps): JSX.Element {
  const { insight, disabled, onVerdictApply, onStatusApply } = props;

  const [verdict, setVerdict] = useState<FitVerdict | "">(insight.fitVerdict ?? "");
  const [verdictReasoning, setVerdictReasoning] = useState(insight.fitReasoning);

  const [pendingStatus, setPendingStatus] = useState<InsightStatus>(insight.status);
  const [archiveReasoning, setArchiveReasoning] = useState("");
  const [archiveError, setArchiveError] = useState(false);

  // Same "external update wins over a stale local draft" discipline as `GoalTree`'s `GoalRow`
  // (spec §10): a fresh `insight` from the store (e.g. after the `orchd://insights-changed` push
  // lands) always re-syncs these fields, including reverting an unconfirmed pending archive if
  // some OTHER change refreshed this row in the meantime.
  useEffect(() => {
    setVerdict(insight.fitVerdict ?? "");
  }, [insight.fitVerdict]);
  useEffect(() => {
    setVerdictReasoning(insight.fitReasoning);
  }, [insight.fitReasoning]);
  useEffect(() => {
    setPendingStatus(insight.status);
    setArchiveError(false);
  }, [insight.status]);

  function handleStatusChange(next: InsightStatus): void {
    setPendingStatus(next);
    setArchiveError(false);
    if (next !== "archived") {
      setArchiveReasoning("");
      onStatusApply(insight.id, next, null);
      return;
    }
    // "archived" is never fired straight from the select — the inline reasoning field + confirm
    // button below appears instead (spec §10: block the call client-side until a reasoning is
    // present).
  }

  function handleArchiveConfirm(): void {
    const trimmed = archiveReasoning.trim();
    if (trimmed === "") {
      setArchiveError(true);
      return;
    }
    setArchiveError(false);
    onStatusApply(insight.id, "archived", trimmed);
  }

  return (
    <div data-testid={`insight-row-${insight.id}`} style={rowStyle}>
      <div style={headerRowStyle}>
        <span style={titleStyle}>{insight.title}</span>
        <Badge
          tone={fitBadgeTone(insight.fitVerdict)}
          data-testid={`insight-fit-badge-${insight.id}`}
        >
          {fitBadgeText(insight.fitVerdict)}
        </Badge>
        <select
          data-testid={`insight-status-${insight.id}`}
          aria-label={strings.insights.statusAria}
          value={pendingStatus}
          disabled={disabled}
          onChange={(e) => handleStatusChange(e.target.value as InsightStatus)}
          style={selectStyle}
        >
          {STATUS_VALUES.map((s) => (
            <option key={s} value={s}>
              {STATUS_LABEL[s]}
            </option>
          ))}
        </select>
      </div>

      <span data-testid={`insight-source-${insight.id}`} style={captionStyle}>
        {strings.insights.sourceLabel} {insight.source || "—"}
      </span>
      {insight.body !== "" && <p style={bodyStyle}>{insight.body}</p>}

      {pendingStatus === "archived" && insight.status !== "archived" && (
        <div style={rowGroupStyle}>
          <input
            data-testid={`insight-archive-reasoning-${insight.id}`}
            aria-label={strings.insights.archiveReasonAria}
            placeholder={strings.insights.archiveReasonPlaceholder}
            value={archiveReasoning}
            onChange={(e) => {
              setArchiveReasoning(e.target.value);
              setArchiveError(false);
            }}
            style={inputStyle}
          />
          <Button
            variant="ghost"
            size="sm"
            type="button"
            data-testid={`insight-archive-confirm-${insight.id}`}
            disabled={disabled}
            onClick={handleArchiveConfirm}
          >
            {strings.insights.confirmArchival}
          </Button>
          {archiveError && (
            <span data-testid={`insight-archive-error-${insight.id}`} style={errorTextStyle}>
              {strings.insights.archiveReasonRequired}
            </span>
          )}
        </div>
      )}

      <div style={rowGroupStyle}>
        <select
          data-testid={`insight-verdict-select-${insight.id}`}
          aria-label={strings.insights.ownerVerdictAria}
          value={verdict}
          onChange={(e) => setVerdict(e.target.value as FitVerdict | "")}
          style={selectStyle}
        >
          <option value="">{strings.common.noVerdict}</option>
          {FIT_VERDICT_VALUES.map((v) => (
            <option key={v} value={v}>
              {FIT_VERDICT_LABEL[v]}
            </option>
          ))}
        </select>
        <input
          data-testid={`insight-verdict-reasoning-${insight.id}`}
          aria-label={strings.insights.verdictReasoningAria}
          placeholder={strings.insights.verdictReasoningPlaceholder}
          value={verdictReasoning}
          onChange={(e) => setVerdictReasoning(e.target.value)}
          style={inputStyle}
        />
        <Button
          variant="ghost"
          size="sm"
          type="button"
          data-testid={`insight-verdict-apply-${insight.id}`}
          disabled={disabled}
          onClick={() =>
            onVerdictApply(insight.id, verdict === "" ? null : verdict, verdictReasoning)
          }
        >
          {strings.insights.applyVerdict}
        </Button>
      </div>
    </div>
  );
}

/**
 * Insights list (S3 spec §10). `projectId === null` addresses the orphan bucket (`Insight.
 * projectId` is nullable, same D11 shape as `Idea`); a concrete `projectId` filters to that
 * project's own insights — `insights.filter(i => i.projectId === projectId)` handles both
 * uniformly, mirroring `IdeasList`.
 *
 * All mutations here are field-level (verdict override, status change) — there is deliberately NO
 * insight-create form: insights are populated by the S-IDEA research pipeline / agents in a later
 * slice, and spec §10's QuickCapture wires only `CreateIdea`, never `CreateInsight`. So this
 * component relies entirely on the shared `orchd://insights-changed` → `refreshInsights` pipe
 * wired in App.tsx (same as `GoalTree`'s title/status edits) for reconciliation; it never calls
 * `refreshInsights` itself. Every mutating call is wrapped in try/catch →
 * `showToast(describeOrchdError(e))` (spec §7 honest error surface).
 *
 * Honest degradation (spec §10): while the store's `orchdDown` is `true`, every mutating control
 * (status select, "confirm archival", "apply verdict") is disabled — reads (the rows
 * themselves) stay live. `ProjectPanel` owns the shared banner; this component only owns
 * disabling its own controls.
 */
export function InsightsList(props: { projectId: string | null }): JSX.Element {
  const { projectId } = props;

  const insights = useAppStore((s) => s.insights);
  const showToast = useAppStore((s) => s.showToast);
  const orchdDown = useAppStore((s) => s.orchdDown);

  const rows = insights
    .filter((i) => i.projectId === projectId)
    .sort((a, b) => b.createdAt - a.createdAt);

  const isOrphanView = projectId === null;

  async function handleVerdictApply(
    id: string,
    verdict: FitVerdict | null,
    reasoning: string,
  ): Promise<void> {
    try {
      await orchdSetInsightFitVerdict(id, verdict, reasoning);
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

  async function handleStatusApply(
    id: string,
    status: InsightStatus,
    resolutionReasoning: string | null,
  ): Promise<void> {
    try {
      await orchdSetInsightStatus(id, status, resolutionReasoning);
    } catch (e) {
      showToast(describeOrchdError(e));
    }
  }

  return (
    <div data-testid="insights-list" style={listStyle}>
      {rows.length === 0 ? (
        <EmptyState
          data-testid="insights-list-empty"
          title={isOrphanView ? strings.insights.emptyOrphan : strings.insights.emptyProject}
        />
      ) : (
        rows.map((insight) => (
          <InsightRow
            key={insight.id}
            insight={insight}
            disabled={orchdDown}
            onVerdictApply={(id, verdict, reasoning) =>
              void handleVerdictApply(id, verdict, reasoning)
            }
            onStatusApply={(id, status, resolutionReasoning) =>
              void handleStatusApply(id, status, resolutionReasoning)
            }
          />
        ))
      )}
    </div>
  );
}
