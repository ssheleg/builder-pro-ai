import { useState, type CSSProperties, type JSX } from "react";
import { useAppStore } from "../../store/store";
import { mcpGetArtifact, describeOrchdError } from "../../ipc/orchd";
import type { Idea, McpArtifact, ResearchRun, ResearchStatus } from "../../ipc/orchd-types";
import { ArtifactViewer } from "../ext/ArtifactsTab";
import { FormInsightDialog } from "./FormInsightDialog";
import { theme } from "../../theme";

const MONO_FONT = 'ui-monospace, SFMono-Regular, "SF Mono", Menlo, monospace';

const RESEARCH_STATUS_LABEL: Record<ResearchStatus, string> = {
  pending: "ожидание",
  running: "выполняется",
  done: "готово",
  failed: "ошибка",
};

const paneStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: 8,
  marginTop: 6,
  padding: "8px 10px",
  border: `1px dashed ${theme.colors.border}`,
  borderRadius: 8,
};

const runRowStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: 4,
};

const runHeaderStyle: CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: 8,
  fontFamily: MONO_FONT,
  fontSize: 11,
};

const badgeStyle: CSSProperties = {
  fontFamily: MONO_FONT,
  fontSize: 11,
  padding: "1px 6px",
  borderRadius: 999,
  border: `1px solid ${theme.colors.border}`,
  color: theme.colors.text,
};

const errorKindStyle: CSSProperties = {
  fontSize: 11,
  color: theme.colors.statusExited,
};

const textButtonStyle: CSSProperties = {
  border: `1px solid ${theme.colors.border}`,
  background: "transparent",
  color: theme.colors.text,
  cursor: "pointer",
  fontSize: 11,
  borderRadius: 4,
  padding: "2px 8px",
  flexShrink: 0,
  whiteSpace: "nowrap",
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
 * Per run: a status badge. A `done` run additionally offers «показать артефакт», which fetches
 * `mcpGetArtifact(artifactId)` on first click and renders it via the REUSED `ArtifactViewer`
 * (`../ext/ArtifactsTab.tsx`, S-EXT's untrusted-banner viewer — not a re-implementation) plus
 * «Сформировать insight» (fetches the artifact too, if not already cached, then opens
 * `FormInsightDialog` prefilled from it). A `failed` run shows its `errorKind` plus a
 * «сформировать insight без ресёрча» affordance (Q8: the degraded path) that opens
 * `FormInsightDialog` with a `null` artifact — never fetches (there is nothing to fetch, a failed
 * run has no `artifactId`).
 *
 * Honest degradation (spec §10, T8 discipline): the two insight-forming affordances — the ones
 * that lead into a MUTATING flow (`FormInsightDialog`) — are `disabled={disabled}` (the caller
 * passes `orchdDown`), mirroring `IdeasList`'s own choice to disable even its dialog-opening
 * triggers while the daemon is down. «показать артефакт» is a plain read and stays enabled — a
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
    return (
      <div data-testid="research-pane-empty" style={{ color: theme.colors.textDim, fontSize: 12 }}>
        исследований по этой идее пока нет
      </div>
    );
  }

  return (
    <div data-testid="research-pane" style={paneStyle}>
      {runs.map((run) => {
        const artifact = run.artifactId !== null ? artifacts[run.artifactId] : undefined;
        return (
          <div key={run.id} data-testid={`research-run-row-${run.id}`} style={runRowStyle}>
            <div style={runHeaderStyle}>
              <span data-testid={`research-run-status-${run.id}`} style={badgeStyle}>
                {RESEARCH_STATUS_LABEL[run.status]}
              </span>
              <span style={{ color: theme.colors.textDim }}>
                {serverNames[run.serverId] ?? run.serverId} · {run.toolName}
              </span>

              {run.status === "done" && (
                <>
                  <button
                    type="button"
                    data-testid={`research-run-show-artifact-${run.id}`}
                    onClick={() => void handleShowArtifact(run)}
                    style={textButtonStyle}
                  >
                    показать артефакт
                  </button>
                  <button
                    type="button"
                    data-testid={`research-run-form-insight-${run.id}`}
                    disabled={disabled}
                    onClick={() => void handleFormInsightFromDone(run)}
                    style={textButtonStyle}
                  >
                    Сформировать insight
                  </button>
                </>
              )}

              {run.status === "failed" && (
                <button
                  type="button"
                  data-testid={`research-run-no-research-${run.id}`}
                  disabled={disabled}
                  onClick={() => handleFormInsightWithoutResearch(run)}
                  style={textButtonStyle}
                >
                  сформировать insight без ресёрча
                </button>
              )}
            </div>

            {run.status === "failed" && (
              <span data-testid={`research-run-error-kind-${run.id}`} style={errorKindStyle}>
                {run.errorKind ?? "неизвестная ошибка"}
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
