import { useEffect, useRef, useState, type CSSProperties, type JSX } from "react";
import { useAppStore } from "../store/store";
import type { Goal, GoalStatus, Project } from "../ipc/orchd-types";
import { Badge } from "../ui/primitives";
import type { Tone } from "../ui/theme";
import { strings } from "../strings";

const STATUS_LABEL: Record<GoalStatus, string> = {
  active: strings.goals.status.active,
  achieved: strings.goals.status.achieved,
  dropped: strings.goals.status.dropped,
};

/** Status-chip tone (design-system.md "Lifecycle chip" atom, read-only variant here — no warn,
 * amber stays reserved for "needs you"): `active` is the neutral default, `achieved` gets the
 * success tone, `dropped` gets the danger tone — a glance tells state without reading the label. */
const STATUS_TONE: Record<GoalStatus, Tone> = {
  active: "muted",
  achieved: "ok",
  dropped: "danger",
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

const projectBlockStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  alignItems: "flex-start",
  gap: "var(--sp-2)",
  width: "100%",
  padding: "var(--sp-3)",
  borderRadius: "var(--r-lg)",
  background: "var(--panel)",
  cursor: "pointer",
  textAlign: "left",
  font: "inherit",
};

const projectNameStyle: CSSProperties = {
  fontFamily: "var(--font-mono)",
  fontSize: "var(--fs-xs)",
  textTransform: "uppercase",
  letterSpacing: 0.5,
  color: "var(--muted)",
};

const strategicTitleStyle: CSSProperties = {
  fontSize: "var(--fs-lg)",
  fontWeight: 600,
  color: "var(--ink)",
};

const chipRowStyle: CSSProperties = {
  display: "flex",
  flexWrap: "wrap",
  gap: "var(--sp-1)",
};

const emptyNoteStyle: CSSProperties = {
  color: "var(--muted)",
  fontSize: "var(--fs-md)",
};

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
 * ON-07: the "Goals are loading…" line must not persist after a *failed* fetch. `refreshGoals`
 * catches its own error (toasts + leaves the entry absent) and always resolves, so absence alone
 * can't tell "still in flight" from "failed". A local, presentational `settling` flag — flipped
 * true when we dispatch fetches and back to false once they all settle (success OR failure) —
 * gates the line, so a failed load shows the toast and then a quiet (not perpetually "loading")
 * panel. No new store state, no new IPC, no new copy.
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

  const [settling, setSettling] = useState(false);
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);

  const activeProjects = activeProjectsOf(projects);

  useEffect(() => {
    const current = useAppStore.getState().goalsByProject;
    const toFetch = activeProjectsOf(useAppStore.getState().projects).filter(
      (p) => !(p.id in current),
    );
    if (toFetch.length === 0) return;
    let remaining = toFetch.length;
    setSettling(true);
    for (const p of toFetch) {
      // refreshGoals always resolves (it catches + toasts internally), so `.finally` fires on both
      // the loaded and the failed path — that is exactly what lets the loading line clear.
      void Promise.resolve(refreshGoals(p.id)).finally(() => {
        remaining -= 1;
        if (remaining <= 0 && mounted.current) setSettling(false);
      });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [projects]);

  if (activeProjects.length === 0) return null;

  return (
    <section aria-label={strings.home.goals} data-testid="home-goals" style={sectionStyle}>
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
                  <Badge
                    key={g.id}
                    data-testid={`home-goals-chip-${g.id}`}
                    tone={STATUS_TONE[g.status]}
                  >
                    {g.title} · {STATUS_LABEL[g.status]}
                  </Badge>
                ))}
              </span>
            )}
          </button>
        );
      })}
      {settling && activeProjects.every((p) => !goalsByProject[p.id]) && (
        <div data-testid="home-goals-empty" style={emptyNoteStyle}>
          {strings.home.goalsLoading}
        </div>
      )}
    </section>
  );
}
