import { useEffect, type JSX } from "react";
import { useAppStore } from "../store/store";
import { strings } from "../strings";
import { IdeasList } from "./IdeasList";
import { InsightsList } from "./InsightsList";
import { OrchdDownBanner } from "./OrchdDownBanner";

/**
 * Inbox — the orphan bucket (AUD-2026-07-19-11 / SCN-028). The ONLY production surface that
 * mounts `IdeasList`/`InsightsList` with `projectId={null}`: every ⌘K capture saved with
 * "no project" lands here, where the per-row "link to project" affordance and
 * `SpawnProjectFromIdea` (both long-implemented inside `IdeasList`) finally become reachable.
 *
 * Mount-refresh mirrors `ProjectPanel`'s eager refresh role: `IdeasList`/`InsightsList` have no
 * mount-fetch of their own (their doc comments — they rely on pushes or the parent's refresh),
 * so this panel pulls both slices wholesale on open. Honest degradation matches `ProjectPanel`:
 * the shared `<OrchdDownBanner/>` renders above the content while `orchdDown`; the lists own
 * disabling their mutating controls.
 */
export function InboxPanel(): JSX.Element {
  const orchdDown = useAppStore((s) => s.orchdDown);
  const refreshIdeas = useAppStore((s) => s.refreshIdeas);
  const refreshInsights = useAppStore((s) => s.refreshInsights);

  useEffect(() => {
    void refreshIdeas();
    void refreshInsights();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <div
      data-testid="inbox-panel"
      style={{
        flex: 1,
        minWidth: 0,
        overflowY: "auto",
        padding: "var(--sp-4)",
        display: "flex",
        flexDirection: "column",
        gap: "var(--sp-4)",
        background: "var(--bg)",
      }}
    >
      <div>
        <h2 style={{ margin: 0, fontSize: "var(--fs-lg)", fontWeight: 700, color: "var(--ink)" }}>
          {strings.inbox.title}
        </h2>
        <p style={{ margin: "var(--sp-1) 0 0", fontSize: "var(--fs-sm)", color: "var(--muted)" }}>
          {strings.inbox.subtitle}
        </p>
      </div>
      {orchdDown && <OrchdDownBanner />}
      <section aria-label={strings.inbox.ideasSection}>
        <h3 style={{ margin: "0 0 var(--sp-2)", fontSize: "var(--fs-md)", fontWeight: 600, color: "var(--ink)" }}>
          {strings.inbox.ideasSection}
        </h3>
        <IdeasList projectId={null} />
      </section>
      <section aria-label={strings.inbox.insightsSection}>
        <h3 style={{ margin: "0 0 var(--sp-2)", fontSize: "var(--fs-md)", fontWeight: 600, color: "var(--ink)" }}>
          {strings.inbox.insightsSection}
        </h3>
        <InsightsList projectId={null} />
      </section>
    </div>
  );
}
