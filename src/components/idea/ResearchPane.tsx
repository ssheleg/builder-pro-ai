import { useEffect, useState, type CSSProperties, type JSX } from "react";
import { useAppStore } from "../../store/store";
import { mcpGetArtifact, describeOrchdError } from "../../ipc/orchd";
import type { Idea, McpArtifact, ResearchRun, ResearchStatus } from "../../ipc/orchd-types";
import { ArtifactViewer } from "../ext/ArtifactsTab";
import { FormInsightDialog } from "./FormInsightDialog";
import { Badge, Button, EmptyState } from "../../ui/primitives";
import { strings } from "../../strings";

/** Self-poll cadence (spec D8, BL-92): while a non-terminal run is on screen, re-list the idea's
 * runs on this interval so a lost `orchd://research-runs-changed` push (or a boot-reconcile) can
 * never leave a run permanently stuck at `pending`/`running`. */
const RESEARCH_POLL_MS = 2000;

/** A run is TERMINAL once it has reached `done` or `failed`; `pending`/`running` are the two
 * non-terminal states that keep the self-poll alive. */
function isNonTerminal(status: ResearchStatus): boolean {
  return status === "pending" || status === "running";
}

const RESEARCH_STATUS_LABEL: Record<ResearchStatus, string> = {
  pending: strings.research.runStatus.pending,
  running: strings.research.runStatus.running,
  done: strings.research.runStatus.done,
  failed: strings.research.runStatus.failed,
};

const paneStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: "var(--sp-2)",
  marginTop: "var(--sp-1)",
  padding: "var(--sp-2) var(--sp-3)",
  border: "1px dashed var(--border-strong)",
  borderRadius: "var(--r-md)",
};

const runRowStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: "var(--sp-1)",
};

const runHeaderStyle: CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: "var(--sp-2)",
  fontFamily: "var(--font-ui)",
  fontSize: "var(--fs-xs)",
};

const errorKindStyle: CSSProperties = {
  fontSize: "var(--fs-xs)",
  color: "var(--danger)",
};

interface OpenInsightTarget {
  runId: string;
  artifact: McpArtifact | null;
}

/**
 * Per-idea research-run pane (S-IDEA spec §7). A pure reader of the store's
 * `researchRunsByIdea[idea.id]` slice (`IdeasList` owns eagerly populating it, mirroring
 * `ProjectPanel`'s own-mount-fetch role for `IdeasList`/`InsightsList` — see that component's doc
 * comment) — this component never fetches the run LIST itself, only individual artifact CONTENT
 * on demand.
 *
 * Per run: a status badge. A `done` run additionally offers "show artifact", which fetches
 * `mcpGetArtifact(artifactId)` on first click and renders it via the REUSED `ArtifactViewer`
 * (`../ext/ArtifactsTab.tsx`, S-EXT's untrusted-banner viewer — not a re-implementation) plus
 * "Form insight" (fetches the artifact too, if not already cached, then opens
 * `FormInsightDialog` prefilled from it). A `failed` run shows its `errorKind` plus a
 * "form insight without research" affordance (Q8: the degraded path) that opens
 * `FormInsightDialog` with a `null` artifact — never fetches (there is nothing to fetch, a failed
 * run has no `artifactId`).
 *
 * Honest degradation (spec §10, T8 discipline): the two insight-forming affordances — the ones
 * that lead into a MUTATING flow (`FormInsightDialog`) — are `disabled={disabled}` (the caller
 * passes `orchdDown`), mirroring `IdeasList`'s own choice to disable even its dialog-opening
 * triggers while the daemon is down. "show artifact" is a plain read and stays enabled — a
 * failed attempt while down surfaces the same honest toast every other read failure does.
 */
export function ResearchPane(props: { idea: Idea; disabled: boolean }): JSX.Element {
  const { idea, disabled } = props;

  // NOTE: select the STABLE outer map, then derive the per-idea array as a plain expression —
  // `useAppStore((s) => s.researchRunsByIdea[idea.id] ?? [])` would return a brand-new `[]`
  // literal every render whenever the key is absent, which breaks zustand's reference-equality
  // snapshot check and infinite-loops `useSyncExternalStore` (mirrors `ToolsBrowser`'s identical
  // `mcpToolsByServer`-then-derive pattern).
  const researchRunsByIdea = useAppStore((s) => s.researchRunsByIdea);
  const runs = researchRunsByIdea[idea.id] ?? [];
  const mcpServers = useAppStore((s) => s.mcpServers);
  const showToast = useAppStore((s) => s.showToast);

  const [artifacts, setArtifacts] = useState<Record<string, McpArtifact>>({});
  const [shown, setShown] = useState<Record<string, boolean>>({});
  const [openInsight, setOpenInsight] = useState<OpenInsightTarget | null>(null);

  // Research-run self-heal (spec D8, BL-92): the run driver's terminal `orchd://research-runs-
  // changed` push can be lost (a disconnect mid-run, or a boot-reconcile where the run finished
  // while the client was away), leaving a run visibly stuck at `pending`/`running` forever. While
  // this pane is mounted AND at least one run is non-terminal, re-list the idea's runs every 2s via
  // the store's own `refreshResearchRuns` (which replaces the slice, so a now-terminal run lands and
  // the badge updates), and STOP the moment every run is terminal — the interval is cleared on
  // unmount and whenever `hasNonTerminal` flips false. Skipped while `disabled` (orchd is down): the
  // shared down-banner already tells the truth and `onOrchdUp` refetches on reconnect, so polling a
  // known-down daemon would only spam failure toasts to no end.
  const hasNonTerminal = runs.some((run) => isNonTerminal(run.status));
  useEffect(() => {
    if (disabled || !hasNonTerminal) return;
    const timer = setInterval(() => {
      void useAppStore.getState().refreshResearchRuns(idea.id);
    }, RESEARCH_POLL_MS);
    return () => clearInterval(timer);
  }, [disabled, hasNonTerminal, idea.id]);

  const serverNames: Record<string, string> = {};
  for (const s of mcpServers) serverNames[s.id] = s.name;

  async function ensureArtifact(run: ResearchRun): Promise<McpArtifact | null> {
    if (run.artifactId === null) return null;
    const cached = artifacts[run.artifactId];
    if (cached) return cached;
    try {
      const fetched = await mcpGetArtifact(run.artifactId);
      setArtifacts((prev) => ({ ...prev, [run.artifactId!]: fetched }));
      return fetched;
    } catch (e) {
      showToast(describeOrchdError(e));
      return null;
    }
  }

  async function handleShowArtifact(run: ResearchRun): Promise<void> {
    const artifact = await ensureArtifact(run);
    if (artifact !== null) setShown((prev) => ({ ...prev, [run.id]: true }));
  }

  async function handleFormInsightFromDone(run: ResearchRun): Promise<void> {
    const artifact = await ensureArtifact(run);
    setOpenInsight({ runId: run.id, artifact });
  }

  function handleFormInsightWithoutResearch(run: ResearchRun): void {
    setOpenInsight({ runId: run.id, artifact: null });
  }

  if (runs.length === 0) {
    return <EmptyState data-testid="research-pane-empty" title={strings.research.emptyRuns} />;
  }

  return (
    <div data-testid="research-pane" style={paneStyle}>
      {runs.map((run) => {
        const artifact = run.artifactId !== null ? artifacts[run.artifactId] : undefined;
        return (
          <div key={run.id} data-testid={`research-run-row-${run.id}`} style={runRowStyle}>
            <div style={runHeaderStyle}>
              <Badge status={run.status} data-testid={`research-run-status-${run.id}`}>
                {RESEARCH_STATUS_LABEL[run.status]}
              </Badge>
              <span style={{ color: "var(--muted)" }}>
                {serverNames[run.serverId] ?? run.serverId} · {run.toolName}
              </span>

              {run.status === "done" && (
                <>
                  <Button
                    variant="ghost"
                    size="sm"
                    type="button"
                    data-testid={`research-run-show-artifact-${run.id}`}
                    onClick={() => void handleShowArtifact(run)}
                  >
                    {strings.research.showArtifact}
                  </Button>
                  <Button
                    variant="ghost"
                    size="sm"
                    type="button"
                    data-testid={`research-run-form-insight-${run.id}`}
                    disabled={disabled}
                    onClick={() => void handleFormInsightFromDone(run)}
                  >
                    {strings.research.formInsight}
                  </Button>
                </>
              )}

              {run.status === "failed" && (
                <Button
                  variant="ghost"
                  size="sm"
                  type="button"
                  data-testid={`research-run-no-research-${run.id}`}
                  disabled={disabled}
                  onClick={() => handleFormInsightWithoutResearch(run)}
                >
                  {strings.research.formInsightNoResearch}
                </Button>
              )}
            </div>

            {run.status === "failed" && (
              <span data-testid={`research-run-error-kind-${run.id}`} style={errorKindStyle}>
                {run.errorKind ?? strings.research.unknownError}
              </span>
            )}

            {run.status === "done" && shown[run.id] === true && artifact && (
              <ArtifactViewer
                artifact={artifact}
                source={serverNames[run.serverId] ?? run.serverId}
                defaultOpen
              />
            )}
          </div>
        );
      })}

      {openInsight !== null && (
        <FormInsightDialog
          idea={idea}
          runId={openInsight.runId}
          artifact={openInsight.artifact}
          onClose={() => setOpenInsight(null)}
        />
      )}
    </div>
  );
}
