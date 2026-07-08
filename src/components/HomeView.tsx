import type { CSSProperties, JSX } from "react";
import { useAppStore } from "../store/store";
import type { SessionMeta, Workspace } from "../ipc/types";
import type { SessionId, WorkspaceId } from "../ipc/commands";
import type { TerminalManager } from "../terminal/terminal-manager";
import { StatusDot } from "./StatusDot";
import { theme } from "../theme";

const MONO_FONT = 'ui-monospace, SFMono-Regular, "SF Mono", Menlo, monospace';

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
 * Waiting rows never call this — their text is always literally "ждёт ввода" by construction
 * (they only exist in the waiting section because `waitingForInput === true`).
 */
function lifecycleText(meta: SessionMeta): string {
  switch (meta.lifecycle.kind) {
    case "running":
      return "выполняется";
    case "atPrompt":
    case "typing":
      return "на месте";
    case "exited":
      return meta.lifecycle.code === 0 ? "завершён" : "завершён с ошибкой";
  }
}

const sectionHeadingStyle: CSSProperties = {
  fontSize: 13,
  fontWeight: 600,
  margin: "0 0 8px 0",
  color: theme.colors.text,
};

const groupHeaderStyle: CSSProperties = {
  display: "block",
  width: "100%",
  textAlign: "left",
  background: "transparent",
  border: "none",
  color: theme.colors.textDim,
  fontFamily: MONO_FONT,
  fontSize: 11,
  textTransform: "uppercase",
  letterSpacing: 0.5,
  padding: "4px 0",
  cursor: "pointer",
};

const monoNameStyle: CSSProperties = {
  fontFamily: MONO_FONT,
  fontSize: 13,
  color: theme.colors.text,
  whiteSpace: "nowrap",
  overflow: "hidden",
  textOverflow: "ellipsis",
};

const dimTextStyle: CSSProperties = {
  fontSize: 12,
  color: theme.colors.textDim,
  flex: 1,
  textAlign: "left",
};

const rowBaseStyle: CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: 8,
  width: "100%",
  padding: "8px 12px",
  marginBottom: 4,
  borderRadius: 6,
  background: theme.colors.bgElevated,
  border: "none",
  textAlign: "left",
};

/** design-system.md §5 "Inbox item": amber left-edge = "a human is needed". */
const waitingRowStyle: CSSProperties = {
  ...rowBaseStyle,
  borderLeft: `3px solid ${theme.colors.statusWaiting}`,
};

const clickableRowStyle: CSSProperties = {
  ...rowBaseStyle,
  borderLeft: `3px solid transparent`,
  cursor: "pointer",
};

const proceedButtonStyle: CSSProperties = {
  background: theme.colors.accent,
  color: "#fff",
  border: "none",
  borderRadius: 6,
  padding: "4px 10px",
  fontSize: 12,
  fontWeight: 600,
  cursor: "pointer",
  flexShrink: 0,
};

const primaryButtonStyle: CSSProperties = {
  ...proceedButtonStyle,
  padding: "6px 12px",
  fontSize: 13,
  alignSelf: "flex-start",
};

/**
 * Attention-first Home (spec §6.2, design decision D6): a pure composition over the existing
 * store (sessions + workspaces + lifecycle) — no new backend, no polling. Three sections in a
 * FIXED order (attention beats chronology, design-system.md §1 "glanceability beats
 * completeness"):
 *   ① «Нужен ты»    — `waitingForInput` sessions, amber Inbox-item rows, pinned top.
 *   ② «Работают»    — active, non-waiting sessions; the whole row is the navigation target.
 *   ③ «Завершились» — exited sessions, ✓/✗ by exit code.
 * Every section groups its rows by workspace (a clickable group header jumps to that workspace
 * with no session selected). A thin stats strip counts across ALL workspaces/sessions, not just
 * what's rendered below it.
 *
 * Navigation (`goTo`, spec §6.2 "Пройти"): `setActiveWorkspaceId` (App-owned UI selection, not
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
  const waiting = all.filter((m) => m.waitingForInput);
  const running = all.filter((m) => m.isActive && !m.waitingForInput);
  const exited = all.filter((m) => !m.isActive && m.lifecycle.kind === "exited");

  const waitingGroups = groupByWorkspace(waiting, workspaces);
  const runningGroups = groupByWorkspace(running, workspaces);
  const exitedGroups = groupByWorkspace(exited, workspaces);

  // Stats strip counts the WHOLE store, not just what's rendered below (spec §6.2 "N workspaces
  // · M live · K waiting" — the owner's context across ALL projects, not a filtered subset).
  const workspaceCount = Object.keys(workspaces).length;
  const waitingCount = waiting.length;
  const liveCount = waitingCount + running.length;

  const orderedWorkspaces = Object.values(workspaces).sort((a, b) =>
    a.name.localeCompare(b.name),
  );
  const firstWorkspace = orderedWorkspaces[0];

  return (
    <div
      style={{
        flex: 1,
        minWidth: 0,
        minHeight: 0,
        overflowY: "auto",
        padding: 16,
        display: "flex",
        flexDirection: "column",
        gap: 20,
      }}
    >
      <div
        data-testid="home-stats"
        style={{
          fontFamily: MONO_FONT,
          fontSize: 11,
          color: theme.colors.textDim,
          letterSpacing: 0.3,
        }}
      >
        {workspaceCount} workspaces · {liveCount} live · {waitingCount} waiting
      </div>

      {all.length === 0 ? (
        <div
          data-testid="home-empty"
          style={{ display: "flex", flexDirection: "column", gap: 8 }}
        >
          <div style={{ color: theme.colors.textDim, fontSize: 13 }}>
            Нет активных сессий.
          </div>
          {firstWorkspace && (
            <button
              type="button"
              style={primaryButtonStyle}
              onClick={() => goTo(firstWorkspace.id)}
            >
              Открыть {firstWorkspace.name}
            </button>
          )}
        </div>
      ) : (
        <>
          {waitingGroups.length > 0 && (
            <section aria-label="Нужен ты">
              <h2 style={{ ...sectionHeadingStyle, color: theme.colors.statusWaiting }}>
                Нужен ты
              </h2>
              {waitingGroups.map((group) => (
                <div key={group.workspaceId}>
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
                      <span style={dimTextStyle}>ждёт ввода</span>
                      <button
                        type="button"
                        style={proceedButtonStyle}
                        onClick={() => goTo(meta.workspaceId, meta.id)}
                      >
                        Пройти →
                      </button>
                    </div>
                  ))}
                </div>
              ))}
            </section>
          )}

          {runningGroups.length > 0 && (
            <section aria-label="Работают">
              <h2 style={sectionHeadingStyle}>Работают</h2>
              {runningGroups.map((group) => (
                <div key={group.workspaceId}>
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
                      <span style={dimTextStyle}>{lifecycleText(meta)}</span>
                    </button>
                  ))}
                </div>
              ))}
            </section>
          )}

          {exitedGroups.length > 0 && (
            <section aria-label="Завершились недавно">
              <h2 style={sectionHeadingStyle}>Завершились недавно</h2>
              {exitedGroups.map((group) => (
                <div key={group.workspaceId}>
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
                          aria-hidden
                          style={{
                            fontFamily: MONO_FONT,
                            fontSize: 13,
                            color: ok ? theme.colors.statusRunning : theme.colors.statusExited,
                            width: 14,
                            flexShrink: 0,
                          }}
                        >
                          {ok ? "✓" : "✗"}
                        </span>
                        <span style={monoNameStyle}>
                          {group.workspaceName}/{meta.title}
                        </span>
                        <span style={dimTextStyle}>code {code ?? "—"}</span>
                      </button>
                    );
                  })}
                </div>
              ))}
            </section>
          )}
        </>
      )}
    </div>
  );
}
