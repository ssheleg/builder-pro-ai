import { useEffect, useMemo, useState, type CSSProperties, type JSX } from "react";
import { useAppStore } from "../../store/store";
import { Button, Dialog } from "../../ui/primitives";
import { strings } from "../../strings";
import type { Workflow } from "../../ipc/orchd-types";

const radioRowStyle: CSSProperties = {
  display: "flex",
  alignItems: "flex-start",
  gap: "var(--sp-2)",
  padding: "var(--sp-2) var(--sp-3)",
  borderRadius: "var(--r-sm)",
  cursor: "pointer",
  fontSize: "var(--fs-md)",
  color: "var(--ink)",
};

const metaStyle: CSSProperties = {
  fontSize: "var(--fs-xs)",
  color: "var(--muted)",
  marginTop: 2,
};

/** Honesty-boundary pending note (S6b) — the calm `--info` inset-edge register (same as the ruleset
 * CEO section's pending note and the Skills tab's registry banner): the run does not fake
 * execution. */
const pendingNoteStyle: CSSProperties = {
  padding: "var(--sp-2) var(--sp-3)",
  boxShadow: "inset 3px 0 0 var(--info)",
  background: "var(--info-weak)",
  color: "var(--ink)",
  fontSize: "var(--fs-xs)",
  borderRadius: "var(--r-sm)",
  lineHeight: 1.5,
  marginTop: "var(--sp-3)",
};

/**
 * Run-workflow picker (SCR-04, SW1) — the run TRIGGER, and ONLY the trigger. Lists the GLOBAL
 * workflows (reusable across every project) as a radio group, with a primary "Run workflow" button.
 *
 * HONESTY BOUNDARY (S6b): this build fabricates NO execution. "Run workflow" is deliberately inert —
 * it spawns no run, creates no run record, calls no ipc; the always-visible pending note is the
 * whole story (the S6b orchestrator-agent runtime that would actually execute a workflow does not
 * exist yet). Authoring, saving and this trigger are the live surface; the run is not. The picker
 * reads `store.workflows` directly (already loaded by the library) and filters to `global` scope.
 */
export function RunWorkflowPicker(props: {
  open: boolean;
  onClose: () => void;
  projectName: string;
}): JSX.Element {
  const { open, onClose, projectName } = props;
  const workflows = useAppStore((s) => s.workflows);

  const globalWorkflows = useMemo(
    () => workflows.filter((w) => w.scope === "global"),
    [workflows],
  );

  const [selectedId, setSelectedId] = useState<string | null>(null);

  // Default the selection to the first global workflow whenever the picker (re)opens or the list
  // changes — but never clobber a still-valid explicit pick.
  useEffect(() => {
    if (!open) return;
    setSelectedId((cur) => {
      if (cur !== null && globalWorkflows.some((w) => w.id === cur)) return cur;
      return globalWorkflows[0]?.id ?? null;
    });
  }, [open, globalWorkflows]);

  const hasWorkflows = globalWorkflows.length > 0;

  return (
    <Dialog
      open={open}
      title={strings.workflows.run.title(projectName)}
      onClose={onClose}
      data-testid="run-workflow-picker"
      footer={
        <>
          <Button type="button" variant="ghost" size="sm" data-testid="run-workflow-cancel" onClick={onClose}>
            {strings.workflows.run.cancel}
          </Button>
          <Button
            type="button"
            variant="primary"
            size="sm"
            data-testid="run-workflow-run"
            disabled={!hasWorkflows || selectedId === null}
            // INERT (S6b honesty boundary): running a workflow needs the orchestrator agent runtime
            // that does not exist yet. This handler MUST NOT spawn a run, create a run record, or
            // call any ipc — the pending note below is the honest explanation. It is a no-op.
            onClick={() => {
              /* no-op: the run does not fake execution (S6b) — see the pending note. */
            }}
          >
            {strings.workflows.run.runButton}
          </Button>
        </>
      }
    >
      {hasWorkflows ? (
        <div role="radiogroup" aria-label={strings.workflows.run.pickAria} data-testid="run-workflow-list">
          {globalWorkflows.map((w: Workflow) => {
            const name = w.name.trim() === "" ? strings.workflows.library.untitled : w.name;
            return (
              <label key={w.id} style={radioRowStyle} data-testid={`run-workflow-option-${w.id}`}>
                <input
                  type="radio"
                  name="run-workflow"
                  checked={selectedId === w.id}
                  onChange={() => setSelectedId(w.id)}
                  aria-label={name}
                  style={{ marginTop: 3 }}
                />
                <span style={{ flex: 1 }}>
                  <span style={{ fontWeight: 600 }}>{name}</span>
                  <span style={metaStyle} data-testid={`run-workflow-meta-${w.id}`}>
                    {" "}
                    {strings.workflows.run.rowMeta(w.stages.length, w.supervisor.enabled)}
                  </span>
                </span>
              </label>
            );
          })}
        </div>
      ) : (
        <div data-testid="run-workflow-empty" style={{ color: "var(--muted)", fontSize: "var(--fs-md)" }}>
          {strings.workflows.run.noGlobalWorkflows}
        </div>
      )}

      <div data-testid="run-workflow-pending" role="note" style={pendingNoteStyle}>
        {strings.workflows.run.pendingNote}
      </div>
    </Dialog>
  );
}
