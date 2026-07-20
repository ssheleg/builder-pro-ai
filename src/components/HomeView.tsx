import type { CSSProperties, JSX } from "react";
import { useAppStore } from "../store/store";
import type { SessionMeta, Workspace } from "../ipc/types";
import type { SessionId, WorkspaceId } from "../ipc/commands";
import type { TerminalManager } from "../terminal/terminal-manager";
import { StatusDot } from "./StatusDot";
import { HomeGoals } from "./HomeGoals";
import { Panel, Stat, Badge, Button, EmptyState } from "../ui/primitives";
import { strings } from "../strings";

interface WorkspaceGroup {
  workspaceId: WorkspaceId;
  workspaceName: string;
  sessions: SessionMeta[];
}

/**
 * Group a flat session list by workspace (spec §6.2 "three sections... grouped by workspace"),
 * sorted by workspace name then session title. The store's `Record` key order is not a contract
 * this view relies on — sorting here is what makes the render order deterministic and independent
 * of insertion order (spec §6.2 test: waiting-vs-running ordering must hold "regardless of
 * insertion order").
 */
function groupByWorkspace(
  list: SessionMeta[],
  workspaces: Record<WorkspaceId, Workspace>,
): WorkspaceGroup[] {
  const byWorkspace = new Map<WorkspaceId, SessionMeta[]>();
  for (const meta of list) {
    const existing = byWorkspace.get(meta.workspaceId);
    if (existing) existing.push(meta);
    else byWorkspace.set(meta.workspaceId, [meta]);
  }
  return Array.from(byWorkspace.entries())
    .map(([workspaceId, sessions]) => ({
      workspaceId,
      workspaceName: workspaces[workspaceId]?.name ?? workspaceId,
      sessions: [...sessions].sort((a, b) => a.title.localeCompare(b.title)),
    }))
    .sort((a, b) => a.workspaceName.localeCompare(b.workspaceName));
}

/**
 * Plain-text "current action" for a non-waiting Agent row (design-system.md §5 Agent-row atom).
 * Waiting rows never call this — their text is always literally "waiting for input" by
 * construction (they only exist in the waiting section because `waitingForInput === true`).
 */
function lifecycleText(meta: SessionMeta): string {
  switch (meta.lifecycle.kind) {
    case "running":
      return strings.home.running;
    case "atPrompt":
    case "typing":
      return strings.home.atPrompt;
    case "exited":
      return meta.lifecycle.code === 0 ? strings.home.exited : strings.home.exitedWithError;
  }
}

// ── token-only style atoms (Calm Control Room, S-UXR B) ──────────────────────────────────────

const containerStyle: CSSProperties = {
  flex: 1,
  minWidth: 0,
  minHeight: 0,
  overflowY: "auto",
  padding: "var(--sp-4)",
  display: "flex",
  flexDirection: "column",
  gap: "var(--sp-5)",
};

const statsRowStyle: CSSProperties = {
  display: "flex",
  gap: "var(--sp-3)",
  flexWrap: "wrap",
};

const sectionStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: "var(--sp-2)",
};

const sectionHeadingStyle: CSSProperties = {
  fontSize: "var(--fs-md)",
  fontWeight: 600,
  margin: 0,
  color: "var(--ink)",
};

/** «Needs you» leads with the warn tone — amber stays reserved for "a human is needed". */
const needsYouHeadingStyle: CSSProperties = { ...sectionHeadingStyle, color: "var(--warn)" };

const groupStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: "var(--sp-1)",
};

const groupHeaderStyle: CSSProperties = {
  display: "block",
  width: "100%",
  textAlign: "left",
  background: "transparent",
  border: "none",
  color: "var(--muted)",
  fontFamily: "var(--font-mono)",
  fontSize: "var(--fs-xs)",
  textTransform: "uppercase",
  letterSpacing: 0.5,
  padding: "var(--sp-1) 0",
  cursor: "pointer",
};

const rowBaseStyle: CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: "var(--sp-2)",
  width: "100%",
  padding: "var(--sp-2) var(--sp-3)",
  borderRadius: "var(--r-md)",
  background: "var(--panel)",
  textAlign: "left",
  color: "var(--ink)",
  font: "inherit",
};

/** design-system.md §5 "Inbox item": amber left-edge = "a human is needed". */
const waitingRowStyle: CSSProperties = {
  ...rowBaseStyle,
  borderLeft: "3px solid var(--warn)",
};

const clickableRowStyle: CSSProperties = {
  ...rowBaseStyle,
  cursor: "pointer",
};

const monoNameStyle: CSSProperties = {
  flex: 1,
  minWidth: 0,
  fontFamily: "var(--font-mono)",
  fontSize: "var(--fs-md)",
  color: "var(--ink)",
  whiteSpace: "nowrap",
  overflow: "hidden",
  textOverflow: "ellipsis",
};

const metaTextStyle: CSSProperties = {
  fontSize: "var(--fs-sm)",
  color: "var(--muted)",
  flexShrink: 0,
};

const codeTextStyle: CSSProperties = {
  ...metaTextStyle,
  fontFamily: "var(--font-mono)",
  fontVariantNumeric: "tabular-nums",
};

function glyphStyle(ok: boolean): CSSProperties {
  return {
    fontFamily: "var(--font-mono)",
    fontSize: "var(--fs-md)",
    color: ok ? "var(--ok)" : "var(--danger)",
    width: 14,
    flexShrink: 0,
    textAlign: "center",
  };
}

/**
 * Attention-first Home (spec §6.2, design decision D6): a pure composition over the existing
 * store (sessions + workspaces + lifecycle) — no new backend, no polling. Three sections in a
 * FIXED order (attention beats chronology, design-system.md §1 "glanceability beats
 * completeness"):
 *   ① «Needs you»          — `waitingForInput` sessions, amber Inbox-item rows, pinned top.
 *   ② «Running»            — active, non-waiting sessions; the whole row is the navigation target.
 *   ③ «Recently finished»  — exited sessions, ✓/✗ by exit code.
 * Every section groups its rows by workspace (a clickable group header jumps to that workspace
 * with no session selected). A whole-store metrics strip counts across ALL workspaces/sessions,
 * not just what's rendered below it — surfaced as `Stat` tiles (mono, tabular-nums) so the numbers
 * have a real home instead of a raw inline sentence (S-UXR B, "metrics-forward").
 *
 * Navigation (`goTo`, spec §6.2 "Go"): `setActiveWorkspaceId` (App-owned UI selection, not
 * store data) -> `setView("workspace")` ->, for a specific session, `setActiveSession` ->
 * `manager.focus`. `manager.focus` is best-effort (TerminalManager.focus is a no-op on a pane
 * that has never been opened) — BL-14's reset-before-replay (T9) is what actually guarantees a
 * clean re-attach when the newly active pane mounts on the next render, `focus()` merely saves a
 * click when the pane was already open (e.g. jumping back to a workspace visited earlier).
 */
export function HomeView(props: {
  manager: TerminalManager;
  setActiveWorkspaceId: (id: WorkspaceId) => void;
}): JSX.Element {
  const { manager, setActiveWorkspaceId } = props;
  const sessions = useAppStore((s) => s.sessions);
  const workspaces = useAppStore((s) => s.workspaces);
  const setView = useAppStore((s) => s.setView);
  const setActiveSession = useAppStore((s) => s.setActiveSession);

  function goTo(workspaceId: WorkspaceId, sessionId?: SessionId): void {
    setActiveWorkspaceId(workspaceId);
    setView("workspace");
    if (sessionId) {
      setActiveSession(sessionId);
      manager.focus(sessionId);
    }
  }

  const all = Object.values(sessions);
  // Exited always wins (mirrors StatusDot.dotStateOf, spec §5/§10.4): belt-and-suspenders against
  // a stale `waitingForInput:true` on a session whose process has already finished — `markExited`
  // is the root fix (clears the flag), this guard is defense-in-depth so no other path can ever
  // surface a dead session in the amber "Needs you" section (review finding F1).
  const waiting = all.filter((m) => m.waitingForInput && m.lifecycle.kind !== "exited");
  const running = all.filter((m) => m.isActive && !m.waitingForInput);
  const exited = all.filter((m) => !m.isActive && m.lifecycle.kind === "exited");

  const waitingGroups = groupByWorkspace(waiting, workspaces);
  const runningGroups = groupByWorkspace(running, workspaces);
  const exitedGroups = groupByWorkspace(exited, workspaces);

  // Stats count the WHOLE store, not just what's rendered below (spec §6.2 "N workspaces
  // · M live · K waiting" — the owner's context across ALL projects, not a filtered subset).
  const workspaceCount = Object.keys(workspaces).length;
  const waitingCount = waiting.length;
  const liveCount = waitingCount + running.length;

  const orderedWorkspaces = Object.values(workspaces).sort((a, b) =>
    a.name.localeCompare(b.name),
  );
  const firstWorkspace = orderedWorkspaces[0];

  return (
    <div style={containerStyle}>
      <Panel data-testid="home-stats">
        <div style={statsRowStyle}>
          <Stat data-testid="home-stat-workspaces" label="workspaces" value={workspaceCount} />
          <Stat
            data-testid="home-stat-live"
            label="live"
            value={liveCount}
            tone={liveCount > 0 ? "info" : "ink"}
          />
          <Stat
            data-testid="home-stat-waiting"
            label="waiting"
            value={waitingCount}
            tone={waitingCount > 0 ? "warn" : "ink"}
          />
        </div>
      </Panel>

      {all.length === 0 ? (
        <EmptyState
          data-testid="home-empty"
          title={strings.home.noActiveSessions}
          action={
            firstWorkspace ? (
              <Button onClick={() => goTo(firstWorkspace.id)}>
                {strings.home.openWorkspace(firstWorkspace.name)}
              </Button>
            ) : undefined
          }
        />
      ) : (
        <>
          {waitingGroups.length > 0 && (
            <section aria-label={strings.home.needsYou} style={sectionStyle}>
              <h2 style={needsYouHeadingStyle}>{strings.home.needsYou}</h2>
              {waitingGroups.map((group) => (
                <div key={group.workspaceId} style={groupStyle}>
                  <button
                    type="button"
                    style={groupHeaderStyle}
                    onClick={() => goTo(group.workspaceId)}
                  >
                    {group.workspaceName}
                  </button>
                  {group.sessions.map((meta) => (
                    <div
                      key={meta.id}
                      data-testid={`home-row-${meta.id}`}
                      style={waitingRowStyle}
                    >
                      <StatusDot lifecycle={meta.lifecycle} waitingForInput={meta.waitingForInput} />
                      <span style={monoNameStyle}>
                        {group.workspaceName}/{meta.title}
                      </span>
                      <Badge status="waiting">{strings.home.waitingForInput}</Badge>
                      <Button
                        size="sm"
                        style={{ flexShrink: 0 }}
                        onClick={() => goTo(meta.workspaceId, meta.id)}
                      >
                        {strings.home.go}
                      </Button>
                    </div>
                  ))}
                </div>
              ))}
            </section>
          )}

          {runningGroups.length > 0 && (
            <section aria-label={strings.home.runningSection} style={sectionStyle}>
              <h2 style={sectionHeadingStyle}>{strings.home.runningSection}</h2>
              {runningGroups.map((group) => (
                <div key={group.workspaceId} style={groupStyle}>
                  <button
                    type="button"
                    style={groupHeaderStyle}
                    onClick={() => goTo(group.workspaceId)}
                  >
                    {group.workspaceName}
                  </button>
                  {group.sessions.map((meta) => (
                    <button
                      type="button"
                      key={meta.id}
                      data-testid={`home-row-${meta.id}`}
                      style={clickableRowStyle}
                      onClick={() => goTo(meta.workspaceId, meta.id)}
                    >
                      <StatusDot lifecycle={meta.lifecycle} waitingForInput={meta.waitingForInput} />
                      <span style={monoNameStyle}>
                        {group.workspaceName}/{meta.title}
                      </span>
                      <span style={metaTextStyle}>{lifecycleText(meta)}</span>
                    </button>
                  ))}
                </div>
              ))}
            </section>
          )}

          {exitedGroups.length > 0 && (
            <section aria-label={strings.home.recentlyFinished} style={sectionStyle}>
              <h2 style={sectionHeadingStyle}>{strings.home.recentlyFinished}</h2>
              {exitedGroups.map((group) => (
                <div key={group.workspaceId} style={groupStyle}>
                  <button
                    type="button"
                    style={groupHeaderStyle}
                    onClick={() => goTo(group.workspaceId)}
                  >
                    {group.workspaceName}
                  </button>
                  {group.sessions.map((meta) => {
                    const code = meta.lifecycle.kind === "exited" ? meta.lifecycle.code : null;
                    const ok = code === 0;
                    return (
                      <button
                        type="button"
                        key={meta.id}
                        data-testid={`home-row-${meta.id}`}
                        style={clickableRowStyle}
                        onClick={() => goTo(meta.workspaceId, meta.id)}
                      >
                        <span
                          aria-label={ok ? strings.home.ok : strings.home.withError}
                          style={glyphStyle(ok)}
                        >
                          {ok ? "✓" : "✗"}
                        </span>
                        <span style={monoNameStyle}>
                          {group.workspaceName}/{meta.title}
                        </span>
                        <span style={codeTextStyle}>code {code ?? "—"}</span>
                      </button>
                    );
                  })}
                </div>
              ))}
            </section>
          )}
        </>
      )}

      {/* Home goals panel (spec §10, task-19): mounts BELOW the three attention sections above —
          the amber "Needs you" block keeps its pinned-top position (S2 §6.2 rule wins over goals
          prominence, spec §10 verbatim). Renders unconditionally here (outside the `all.length`
          empty-state branch above) — an empty terminal store has nothing to say about whether the
          owner has active projects with goals, so goals visibility is independent of session
          count; `HomeGoals` itself is the one that decides to render nothing when there are no
          active projects. */}
      <HomeGoals />
    </div>
  );
}
