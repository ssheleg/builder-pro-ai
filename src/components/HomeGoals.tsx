import { useEffect, type CSSProperties, type JSX } from "react";
import { useAppStore } from "../store/store";
import type { Goal, GoalStatus, Project } from "../ipc/orchd-types";
import { theme } from "../theme";
import { strings } from "../strings";

const MONO_FONT = 'ui-monospace, SFMono-Regular, "SF Mono", Menlo, monospace';

const STATUS_LABEL: Record<GoalStatus, string> = {
  active: strings.goals.status.active,
  achieved: strings.goals.status.achieved,
  dropped: strings.goals.status.dropped,
};

/** Status-chip accent (design-system.md "Lifecycle chip" atom, read-only variant here — no amber,
 * amber stays reserved for "needs you"): `active` is the neutral default, `achieved` gets the
 * green success color, `dropped` gets the red exited color — a glance tells state without reading
 * the label. */
const STATUS_COLOR: Record<GoalStatus, string> = {
  active: theme.colors.textDim,
  achieved: theme.colors.statusRunning,
  dropped: theme.colors.statusExited,
};

const sectionHeadingStyle: CSSProperties = {
  fontSize: 13,
  fontWeight: 600,
  margin: "0 0 8px 0",
  color: theme.colors.text,
};

const projectBlockStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  alignItems: "flex-start",
  gap: 6,
  width: "100%",
  padding: "10px 12px",
  marginBottom: 8,
  borderRadius: 8,
  border: `1px solid ${theme.colors.border}`,
  background: theme.colors.bgElevated,
  cursor: "pointer",
  textAlign: "left",
};

const projectNameStyle: CSSProperties = {
  fontFamily: MONO_FONT,
  fontSize: 11,
  textTransform: "uppercase",
  letterSpacing: 0.5,
  color: theme.colors.textDim,
};

const strategicTitleStyle: CSSProperties = {
  fontSize: 14,
  fontWeight: 600,
  color: theme.colors.text,
};

const chipRowStyle: CSSProperties = {
  display: "flex",
  flexWrap: "wrap",
  gap: 6,
};

const emptyNoteStyle: CSSProperties = {
  color: theme.colors.textDim,
  fontSize: 13,
};

function chipStyle(status: GoalStatus): CSSProperties {
  return {
    display: "inline-flex",
    alignItems: "center",
    gap: 4,
    fontFamily: MONO_FONT,
    fontSize: 11,
    padding: "2px 8px",
    borderRadius: 999,
    border: `1px solid ${theme.colors.border}`,
    color: STATUS_COLOR[status],
  };
}

/** Active (non-archived) projects, sorted by name for a deterministic render order — mirrors
 * `groupByWorkspace`'s sort convention in `HomeView.tsx`. */
function activeProjectsOf(projects: Project[]): Project[] {
  return projects
    .filter((p) => p.status !== "archived")
    .sort((a, b) => a.name.localeCompare(b.name));
}

/** The project's single strategic root (`kind: "strategic"`, `parentId: null` — server-side
 * invariant guarantees exactly one, auto-created with every project, spec §5.2). `undefined`
 * only while the project's goal tree hasn't finished its first fetch's result shape (defensive —
 * should never actually happen once `goals` is non-empty). */
function strategicOf(goals: Goal[]): Goal | undefined {
  return goals.find((g) => g.kind === "strategic" && g.parentId === null);
}

/** DIRECT `additional` children of the strategic root only (task-19 brief: "direct additional
 * children" — NOT the whole subtree), sorted by `ord` ascending. */
function directChildrenOf(goals: Goal[], strategicId: string): Goal[] {
  return goals
    .filter((g) => g.parentId === strategicId)
    .sort((a, b) => a.ord - b.ord);
}

/**
 * Home goals panel (spec §10, task-19): per ACTIVE project, the strategic goal's title + its
 * direct `additional` children as status chips. Mounts BELOW the three S2 attention sections in
 * `HomeView.tsx` — the amber "Needs you" block keeps its pinned-top position (S2 §6.2 rule wins
 * over goals prominence, spec §10 verbatim).
 *
 * Reads `goalsByProject` from the store; a project whose goals haven't been fetched yet
 * (`projectId` absent from the map — the same "absence means not yet fetched" convention every
 * other `refresh*`-backed slice in `store.ts` uses) triggers exactly one `refreshGoals(projectId)`
 * on mount / whenever the `projects` list itself changes (e.g. a newly created active project
 * appearing). A rejection is NOT swallowed here — `refreshGoals` itself already surfaces the
 * mapped honest message as a toast (spec §7), so this effect doesn't need its own try/catch.
 *
 * Clicking anywhere in a project's block navigates to that project's panel (`openProject`) — the
 * whole block is one `<button>`, matching `HomeView`'s own clickable-row convention, since every
 * click target inside it (title, any goal chip) leads to the exact same destination.
 */
export function HomeGoals(): JSX.Element | null {
  const projects = useAppStore((s) => s.projects);
  const goalsByProject = useAppStore((s) => s.goalsByProject);
  const refreshGoals = useAppStore((s) => s.refreshGoals);
  const openProject = useAppStore((s) => s.openProject);

  const activeProjects = activeProjectsOf(projects);

  useEffect(() => {
    const current = useAppStore.getState().goalsByProject;
    for (const p of activeProjectsOf(useAppStore.getState().projects)) {
      if (!(p.id in current)) {
        void refreshGoals(p.id);
      }
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [projects]);

  if (activeProjects.length === 0) return null;

  return (
    <section aria-label={strings.home.goals} data-testid="home-goals">
      <h2 style={sectionHeadingStyle}>{strings.home.goals}</h2>
      {activeProjects.map((p) => {
        const goals = goalsByProject[p.id];
        if (!goals) return null; // not fetched yet — the effect above is already chasing it
        const strategic = strategicOf(goals);
        if (!strategic) return null; // defensive; the server invariant guarantees this exists
        const children = directChildrenOf(goals, strategic.id);
        return (
          <button
            key={p.id}
            type="button"
            data-testid={`home-goals-project-${p.id}`}
            onClick={() => openProject(p.id)}
            style={projectBlockStyle}
          >
            <span style={projectNameStyle}>{p.name}</span>
            <span style={strategicTitleStyle}>{strategic.title}</span>
            {children.length > 0 && (
              <span style={chipRowStyle}>
                {children.map((g) => (
                  <span
                    key={g.id}
                    data-testid={`home-goals-chip-${g.id}`}
                    style={chipStyle(g.status)}
                  >
                    {g.title} · {STATUS_LABEL[g.status]}
                  </span>
                ))}
              </span>
            )}
          </button>
        );
      })}
      {activeProjects.every((p) => !goalsByProject[p.id]) && (
        <div data-testid="home-goals-empty" style={emptyNoteStyle}>
          {strings.home.goalsLoading}
        </div>
      )}
    </section>
  );
}
