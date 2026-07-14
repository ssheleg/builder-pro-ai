import { useEffect, useState, type CSSProperties, type JSX } from "react";
import { useAppStore } from "../store/store";
import {
  orchdSetInsightFitVerdict,
  orchdSetInsightStatus,
  describeOrchdError,
} from "../ipc/orchd";
import type { FitVerdict, Insight, InsightStatus } from "../ipc/orchd-types";
import { theme } from "../theme";

const MONO_FONT = 'ui-monospace, SFMono-Regular, "SF Mono", Menlo, monospace';

/** Inline block message shown when the owner tries to archive an insight without a reasoning
 * (spec §10/§5.2 — the server enforces this too; the UI refuses it up front rather than round-
 * tripping a doomed request). */
const ARCHIVE_REASONING_REQUIRED_TEXT = "нужна причина архивации";

const FIT_VERDICT_VALUES: FitVerdict[] = ["fit", "noFit", "unknown"];

const FIT_VERDICT_LABEL: Record<FitVerdict, string> = {
  fit: "подходит",
  noFit: "не подходит",
  unknown: "неясно",
};

const STATUS_VALUES: InsightStatus[] = ["new", "accepted", "archived"];

const STATUS_LABEL: Record<InsightStatus, string> = {
  new: "новый",
  accepted: "принят",
  archived: "архив",
};

const listStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: 8,
};

const rowStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: 6,
  padding: "8px 12px",
  fontFamily: MONO_FONT,
  fontSize: 12,
  border: `1px solid ${theme.colors.border}`,
  borderRadius: 8,
  background: theme.colors.bgElevated,
};

const headerRowStyle: CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: 8,
  flexWrap: "wrap",
};

const titleStyle: CSSProperties = {
  flex: "1 1 auto",
  fontSize: 13,
  fontWeight: 600,
  color: theme.colors.text,
};

const captionStyle: CSSProperties = {
  fontSize: 11,
  color: theme.colors.textDim,
};

const bodyStyle: CSSProperties = {
  fontSize: 12,
  color: theme.colors.textDim,
};

const badgeStyle: CSSProperties = {
  fontFamily: MONO_FONT,
  fontSize: 11,
  padding: "2px 8px",
  borderRadius: 999,
  border: `1px solid ${theme.colors.border}`,
  color: theme.colors.text,
  flexShrink: 0,
};

const selectStyle: CSSProperties = {
  fontFamily: MONO_FONT,
  fontSize: 11,
  color: theme.colors.text,
  background: theme.colors.bg,
  border: `1px solid ${theme.colors.border}`,
  borderRadius: 999,
  padding: "2px 8px",
  flexShrink: 0,
};

const inputStyle: CSSProperties = {
  flex: "1 1 160px",
  minWidth: 0,
  fontFamily: MONO_FONT,
  fontSize: 12,
  color: theme.colors.text,
  background: "transparent",
  border: `1px solid ${theme.colors.border}`,
  borderRadius: 4,
  padding: "3px 6px",
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

const errorTextStyle: CSSProperties = {
  fontSize: 11,
  color: theme.colors.statusExited,
};

const rowGroupStyle: CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: 6,
  flexWrap: "wrap",
};

function fitBadgeText(v: FitVerdict | null): string {
  return v === null ? "—" : FIT_VERDICT_LABEL[v];
}

interface InsightRowProps {
  insight: Insight;
  onVerdictApply: (id: string, verdict: FitVerdict | null, reasoning: string) => void;
  onStatusApply: (id: string, status: InsightStatus, resolutionReasoning: string | null) => void;
}

/** One insight row (design-system.md, spec §10). Archiving requires a non-empty
 * `resolutionReasoning` server-side (spec §5.2) — the row collects it inline and blocks the call
 * client-side with an honest message rather than round-tripping a doomed request. */
function InsightRow(props: InsightRowProps): JSX.Element {
  const { insight, onVerdictApply, onStatusApply } = props;

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
        <span data-testid={`insight-fit-badge-${insight.id}`} style={badgeStyle}>
          {fitBadgeText(insight.fitVerdict)}
        </span>
        <select
          data-testid={`insight-status-${insight.id}`}
          aria-label="Статус инсайта"
          value={pendingStatus}
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
        источник: {insight.source || "—"}
      </span>
      {insight.body !== "" && <p style={bodyStyle}>{insight.body}</p>}

      {pendingStatus === "archived" && insight.status !== "archived" && (
        <div style={rowGroupStyle}>
          <input
            data-testid={`insight-archive-reasoning-${insight.id}`}
            aria-label="Причина архивации"
            placeholder="причина архивации"
            value={archiveReasoning}
            onChange={(e) => {
              setArchiveReasoning(e.target.value);
              setArchiveError(false);
            }}
            style={inputStyle}
          />
          <button
            type="button"
            data-testid={`insight-archive-confirm-${insight.id}`}
            onClick={handleArchiveConfirm}
            style={textButtonStyle}
          >
            подтвердить архивацию
          </button>
          {archiveError && (
            <span data-testid={`insight-archive-error-${insight.id}`} style={errorTextStyle}>
              {ARCHIVE_REASONING_REQUIRED_TEXT}
            </span>
          )}
        </div>
      )}

      <div style={rowGroupStyle}>
        <select
          data-testid={`insight-verdict-select-${insight.id}`}
          aria-label="Вердикт владельца"
          value={verdict}
          onChange={(e) => setVerdict(e.target.value as FitVerdict | "")}
          style={selectStyle}
        >
          <option value="">— без вердикта —</option>
          {FIT_VERDICT_VALUES.map((v) => (
            <option key={v} value={v}>
              {FIT_VERDICT_LABEL[v]}
            </option>
          ))}
        </select>
        <input
          data-testid={`insight-verdict-reasoning-${insight.id}`}
          aria-label="Обоснование вердикта"
          placeholder="обоснование"
          value={verdictReasoning}
          onChange={(e) => setVerdictReasoning(e.target.value)}
          style={inputStyle}
        />
        <button
          type="button"
          data-testid={`insight-verdict-apply-${insight.id}`}
          onClick={() =>
            onVerdictApply(insight.id, verdict === "" ? null : verdict, verdictReasoning)
          }
          style={textButtonStyle}
        >
          применить вердикт
        </button>
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
 */
export function InsightsList(props: { projectId: string | null }): JSX.Element {
  const { projectId } = props;

  const insights = useAppStore((s) => s.insights);
  const showToast = useAppStore((s) => s.showToast);

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
        <div
          data-testid="insights-list-empty"
          style={{ color: theme.colors.textDim, fontSize: 13 }}
        >
          {isOrphanView ? "Нет инсайтов без проекта." : "В этом проекте пока нет инсайтов."}
        </div>
      ) : (
        rows.map((insight) => (
          <InsightRow
            key={insight.id}
            insight={insight}
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
