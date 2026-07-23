import { useEffect, type CSSProperties, type JSX } from "react";
import { useAppStore } from "../store/store";
import { Panel, Stat, Button, SegmentedPill, Heatmap, EmptyState } from "../ui/primitives";
import { strings } from "../strings";
import type { DayUsage, GitStats, StatsRange } from "../ipc/stats";
import type { Project } from "../ipc/orchd-types";
import type { Workspace } from "../ipc/types";

/**
 * Stats view (SCN-052/053, FLW-20) — "where did the week go" in one glance: range pill
 * (SegmentedPill — the atom's first real surface, closes COV-01 together with Heatmap below),
 * triage tiles, tokens/day density Heatmap, and a per-project table joining BOTH sources.
 *
 * Attribution (A-8): usage rows carry the session's real `cwd`; a row belongs to the project
 * whose workspace root is the LONGEST prefix of that cwd (a nested workspace must win over a
 * parent dir), everything unmatched lands in the honest "other" bucket — never dropped.
 * Per-source honesty: usage and git fail independently; each failure renders its own
 * "data unavailable" note while the surviving source keeps rendering (SCN-052 E&R).
 */

/** Longest-prefix cwd → project-name attribution. Exported for tests. */
export function attributeCwd(
  cwd: string,
  projects: Project[],
  workspaces: Record<string, Workspace>,
): string {
  let best: { name: string; len: number } | null = null;
  for (const p of projects) {
    for (const wid of p.workspaceIds) {
      const w = workspaces[wid];
      if (!w) continue;
      for (const root of w.roots ?? [w.rootPath]) {
        if (cwd === root || cwd.startsWith(root.endsWith("/") ? root : root + "/")) {
          if (!best || root.length > best.len) best = { name: p.name, len: root.length };
        }
      }
    }
  }
  return best?.name ?? strings.stats.otherBucket;
}

/** Root → project name via the same longest-prefix rule (git rows key by root). */
function attributeRoot(root: string, projects: Project[], workspaces: Record<string, Workspace>): string {
  return attributeCwd(root, projects, workspaces);
}

/** Compact token formatter: 1234 → "1.2k", 2_400_000 → "2.4M". */
export function fmtTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}k`;
  return String(n);
}

/** Last `days` calendar days ending today (UTC), oldest first — the heatmap's x-axis. */
function lastDays(days: number, nowMs: number): string[] {
  const out: string[] = [];
  for (let i = days - 1; i >= 0; i--) {
    out.push(new Date(nowMs - i * 86_400_000).toISOString().slice(0, 10));
  }
  return out;
}

interface ProjectRow {
  name: string;
  tokens: number;
  cost: number;
  costPartial: boolean;
  costAny: boolean;
  sessions: number;
  commits: number;
  added: number;
  deleted: number;
  gitAvailable: boolean;
  gitReason: string | null;
}

function buildRows(
  days: DayUsage[],
  git: GitStats[],
  projects: Project[],
  workspaces: Record<string, Workspace>,
): ProjectRow[] {
  const rows = new Map<string, ProjectRow>();
  const row = (name: string): ProjectRow => {
    let r = rows.get(name);
    if (!r) {
      r = {
        name,
        tokens: 0,
        cost: 0,
        costPartial: false,
        costAny: false,
        sessions: 0,
        commits: 0,
        added: 0,
        deleted: 0,
        gitAvailable: false,
        gitReason: null,
      };
      rows.set(name, r);
    }
    return r;
  };
  for (const d of days) {
    const r = row(attributeCwd(d.cwd, projects, workspaces));
    r.tokens += d.tokensIn + d.tokensOut;
    r.sessions += d.sessions;
    if (d.estCostUsd !== null) {
      r.cost += d.estCostUsd;
      r.costAny = true;
    }
    if (!d.costComplete) r.costPartial = true;
  }
  for (const g of git) {
    const r = row(attributeRoot(g.root, projects, workspaces));
    if (g.available) {
      r.gitAvailable = true;
      r.commits += g.commits;
      r.added += g.added;
      r.deleted += g.deleted;
    } else if (!r.gitAvailable) {
      r.gitReason = g.reason;
    }
  }
  return [...rows.values()].sort((a, b) => b.tokens - a.tokens);
}

export function StatsView(): JSX.Element {
  const stats = useAppStore((s) => s.stats);
  const setStatsRange = useAppStore((s) => s.setStatsRange);
  const refreshStats = useAppStore((s) => s.refreshStats);
  const projects = useAppStore((s) => s.projects);
  const workspaces = useAppStore((s) => s.workspaces);

  // First open fetches; later opens reuse the slice until Refresh / range change (the scan is
  // disk-heavy by design — A-8 corpus size — so no polling loop here).
  useEffect(() => {
    if (stats.usage === null && stats.git === null && !stats.loading) void refreshStats();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const days = stats.usage?.days ?? [];
  const git = (stats.git ?? []).filter(Boolean);
  const rows = buildRows(days, git, projects, workspaces);

  const totTokens = days.reduce((a, d) => a + d.tokensIn + d.tokensOut, 0);
  const totCost = rows.reduce((a, r) => a + r.cost, 0);
  const anyCost = rows.some((r) => r.costAny);
  const anyPartial = rows.some((r) => r.costPartial);
  const totSessions = days.reduce((a, d) => a + d.sessions, 0);
  const totCommits = rows.reduce((a, r) => a + r.commits, 0);
  const totAdded = rows.reduce((a, r) => a + r.added, 0);
  const totDeleted = rows.reduce((a, r) => a + r.deleted, 0);

  const now = Date.now();
  const heatDays = lastDays(30, now);
  const byDay = new Map<string, number>();
  for (const d of days) byDay.set(d.day, (byDay.get(d.day) ?? 0) + d.tokensIn + d.tokensOut);
  const heatValues = heatDays.map((d) => byDay.get(d) ?? 0);

  const empty = !stats.loading && days.length === 0 && !stats.usageError;

  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 16, padding: 24 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 16 }}>
        <h1 style={{ margin: 0, font: "700 26px var(--font-display)", color: "var(--ink)" }}>
          {strings.stats.title}
        </h1>
        <div style={{ flex: 1 }} />
        <SegmentedPill<StatsRange>
          data-testid="stats-range"
          ariaLabel={strings.stats.rangeAria}
          value={stats.range}
          onChange={(r) => void setStatsRange(r)}
          options={[
            { value: "all", label: strings.stats.rangeAll },
            { value: "30d", label: strings.stats.range30d },
            { value: "7d", label: strings.stats.range7d },
          ]}
        />
      </div>

      {stats.usageError !== null && (
        <div role="alert" style={noteStyle}>
          {strings.stats.usageUnavailable(stats.usageError)}
        </div>
      )}
      {stats.gitError !== null && (
        <div role="alert" style={noteStyle}>
          {strings.stats.gitUnavailable(stats.gitError)}
        </div>
      )}

      {stats.loading && <div style={{ color: "var(--muted)" }}>{strings.stats.loading}</div>}

      {empty ? (
        <EmptyState title={strings.stats.emptyTitle} hint={strings.stats.emptyHint} />
      ) : (
        <>
          <Panel data-testid="stats-tiles">
            <div style={{ display: "flex", gap: 14 }}>
              <Stat data-testid="stats-tokens" label={strings.stats.tokens} value={fmtTokens(totTokens)} />
              <Stat
                data-testid="stats-cost"
                label={anyPartial ? strings.stats.costPartialLabel : strings.stats.costLabel}
                value={anyCost ? `$${totCost.toFixed(2)}` : "—"}
              />
              <Stat label={strings.stats.sessions} value={String(totSessions)} />
              <Stat label={strings.stats.commits} value={String(totCommits)} />
              <Stat label={strings.stats.code} value={`+${fmtTokens(totAdded)} −${fmtTokens(totDeleted)}`} />
            </div>
          </Panel>

          <Panel>
            <div style={{ display: "flex", flexDirection: "column", gap: 10 }}>
              <span style={{ font: "500 13px var(--font-ui)", color: "var(--ink)" }}>
                {strings.stats.activity}
              </span>
              <Heatmap
                data-testid="stats-heatmap"
                values={heatValues}
                columns={30}
                ariaLabel={strings.stats.activityAria}
              />
            </div>
          </Panel>

          <Panel data-testid="stats-table">
            <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
              <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
                <span style={{ font: "500 13px var(--font-ui)", color: "var(--ink)" }}>
                  {strings.stats.byProject}
                </span>
                <div style={{ flex: 1 }} />
                {stats.usage !== null && (
                  <span style={{ font: "400 11px var(--font-ui)", color: "var(--muted)" }}>
                    {strings.stats.asOf(new Date(stats.usage.asOf).toLocaleTimeString())}
                  </span>
                )}
                <Button variant="ghost" onClick={() => void refreshStats()}>
                  {strings.stats.refresh}
                </Button>
              </div>
              {rows.map((r) => (
                <div
                  key={r.name}
                  data-testid={`stats-row-${r.name}`}
                  style={{ display: "flex", gap: 14, padding: "7px 0", borderTop: "1px solid var(--hairline)" }}
                >
                  <span style={{ font: "500 12px var(--font-ui)", color: "var(--ink)", minWidth: 180 }}>
                    {r.name}
                  </span>
                  <span style={cell}>{fmtTokens(r.tokens)}</span>
                  <span style={cell}>
                    {r.costAny ? `$${r.cost.toFixed(2)}${r.costPartial ? strings.stats.partialMark : ""}` : "—"}
                  </span>
                  <span style={cell}>{r.sessions}</span>
                  {r.gitAvailable ? (
                    <span style={cell}>
                      {r.commits} · +{fmtTokens(r.added)} −{fmtTokens(r.deleted)}
                    </span>
                  ) : (
                    <span style={{ ...cell, color: "var(--muted)" }}>{strings.stats.noGit}</span>
                  )}
                </div>
              ))}
              {anyCost && (
                <span style={{ font: "400 10px var(--font-ui)", color: "var(--muted)" }}>
                  {strings.stats.estimatedNote}
                </span>
              )}
            </div>
          </Panel>
        </>
      )}
    </div>
  );
}

const noteStyle: CSSProperties = {
  background: "var(--warn-weak)",
  color: "var(--warn)",
  borderRadius: 10,
  padding: "8px 12px",
  font: "500 12px var(--font-ui)",
};

const cell: CSSProperties = {
  font: "400 12px var(--font-ui)",
  color: "var(--ink)",
  minWidth: 90,
};
