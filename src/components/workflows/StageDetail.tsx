import { useState, type CSSProperties, type JSX } from "react";
import type { ContextScope, Gate, Skill, Stage } from "../../ipc/orchd-types";
import { Badge, Button, Field, Input, SegmentedPill, TextArea } from "../../ui/primitives";
import { strings } from "../../strings";
import { KNOWN_AGENTS, agentLabel, effectiveSkillIds, isKnownAgent } from "./agents";

const sectionStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: "var(--sp-3)",
  padding: "var(--sp-4)",
  background: "var(--panel-2)",
  borderRadius: "var(--r-md)",
};

const labelStyle: CSSProperties = {
  fontSize: "var(--fs-xs)",
  fontWeight: 600,
  color: "var(--muted)",
  textTransform: "uppercase",
  letterSpacing: "0.05em",
};

const chipRowStyle: CSSProperties = {
  display: "flex",
  flexWrap: "wrap",
  alignItems: "center",
  gap: "var(--sp-2)",
};

const chipRemoveStyle: CSSProperties = {
  border: "none",
  background: "transparent",
  color: "var(--muted)",
  cursor: "pointer",
  fontSize: "var(--fs-sm)",
  lineHeight: 1,
  padding: 0,
};

const skillCheckStyle: CSSProperties = {
  display: "inline-flex",
  alignItems: "center",
  gap: "var(--sp-1)",
  fontSize: "var(--fs-sm)",
  color: "var(--ink)",
  cursor: "pointer",
};

const agentPanelStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: "var(--sp-3)",
  marginTop: "var(--sp-2)",
  paddingTop: "var(--sp-3)",
  borderTop: "1px solid var(--line)",
};

const noteStyle: CSSProperties = {
  fontSize: "var(--fs-xs)",
  color: "var(--muted)",
  lineHeight: 1.5,
};

/** Inline agent-select option value for "inherit the workflow default" (a `null` stage agent). A
 * sentinel because `<option value="">` is fine but reads clearer named. */
const INHERIT_VALUE = "";

/**
 * Stage detail (SCR-03, SW1) — the editor for one stage. Prompt/command editor, a stage-skills
 * picker (from `store.skills`), an effective-skills summary (global ∪ stage, deduped), a gate
 * segmented control, and the Agent & context panel (agent picker with Inherit + known agents; a
 * context-scope segmented control where `selected` reveals a subset note; outputs chips).
 *
 * HONEST MARKERS: a bound skill id NOT present in `skills` renders a "missing skill" marker (the
 * binding is dangling — it would block a clean run); an `agent` pinned to an id that is not one of
 * the known/launchable agents renders an "unknown agent" marker. Neither is hidden — the honesty
 * boundary must be visible. All edits flow up via `onChange(nextStage)`; the component holds no
 * stage state of its own (it is a controlled editor over the parent's draft), so a reorder or an
 * external change never desyncs it.
 */
export function StageDetail(props: {
  stage: Stage;
  skills: Skill[];
  defaultAgent: string;
  globalSkillIds: string[];
  onChange: (stage: Stage) => void;
  onRemove: () => void;
  onDone: () => void;
}): JSX.Element {
  const { stage, skills, defaultAgent, globalSkillIds, onChange, onRemove, onDone } = props;
  const [outputDraft, setOutputDraft] = useState("");

  const skillById = new Map(skills.map((s) => [s.id, s]));
  const skillLabel = (id: string): string => skillById.get(id)?.name ?? id;

  // Bound stage skill ids that are not in the registry (dangling bindings) — surfaced as markers.
  const missingBindings = stage.skillIds.filter((id) => !skillById.has(id));

  // Effective skills the S6b runtime would load (global ∪ this stage, deduped) — a pure view.
  const effective = effectiveSkillIds(globalSkillIds, stage.skillIds);

  const gateOptions: readonly { value: Gate; label: string }[] = [
    { value: "auto", label: strings.workflows.gates.auto },
    { value: "manual", label: strings.workflows.gates.manual },
  ];
  const contextOptions: readonly { value: ContextScope; label: string }[] = [
    { value: "inherit", label: strings.workflows.contextScopes.inherit },
    { value: "handoff", label: strings.workflows.contextScopes.handoff },
    { value: "project", label: strings.workflows.contextScopes.project },
    { value: "selected", label: strings.workflows.contextScopes.selected },
  ];

  function toggleSkill(id: string, checked: boolean): void {
    const next = checked
      ? [...stage.skillIds, id]
      : stage.skillIds.filter((s) => s !== id);
    onChange({ ...stage, skillIds: next });
  }

  function removeSkill(id: string): void {
    onChange({ ...stage, skillIds: stage.skillIds.filter((s) => s !== id) });
  }

  function commitOutput(): void {
    const trimmed = outputDraft.trim();
    if (trimmed === "" || stage.outputs.includes(trimmed)) {
      setOutputDraft("");
      return;
    }
    onChange({ ...stage, outputs: [...stage.outputs, trimmed] });
    setOutputDraft("");
  }

  const agentPinnedUnknown = stage.agent !== null && !isKnownAgent(stage.agent);
  const effectiveAgent = stage.agent ?? defaultAgent;

  return (
    <div data-testid={`stage-detail-${stage.id}`} style={sectionStyle}>
      <Field label={strings.workflows.stage.nameLabel}>
        <Input
          data-testid="stage-name"
          aria-label={strings.workflows.stage.nameAria}
          placeholder={strings.workflows.stage.namePlaceholder}
          value={stage.name}
          onChange={(e) => onChange({ ...stage, name: e.target.value })}
        />
      </Field>

      <Field label={strings.workflows.stage.promptLabel}>
        <TextArea
          data-testid="stage-prompt"
          aria-label={strings.workflows.stage.promptAria}
          placeholder={strings.workflows.stage.promptPlaceholder}
          value={stage.prompt}
          onChange={(e) => onChange({ ...stage, prompt: e.target.value })}
          rows={4}
        />
      </Field>

      {/* Stage-skills picker (from store.skills) + missing-binding markers */}
      <div>
        <span style={labelStyle}>{strings.workflows.stage.skillsLabel}</span>
        {skills.length === 0 ? (
          <div style={{ ...noteStyle, marginTop: "var(--sp-1)" }} data-testid="stage-no-skills">
            {strings.workflows.stage.noSkillsAvailable}
          </div>
        ) : (
          <div style={{ ...chipRowStyle, marginTop: "var(--sp-1)" }} data-testid="stage-skills">
            {skills.map((skill) => (
              <label key={skill.id} style={skillCheckStyle}>
                <input
                  type="checkbox"
                  data-testid={`stage-skill-${skill.id}`}
                  aria-label={skill.name}
                  checked={stage.skillIds.includes(skill.id)}
                  onChange={(e) => toggleSkill(skill.id, e.target.checked)}
                />
                <span>{skill.name}</span>
              </label>
            ))}
          </div>
        )}
        {missingBindings.length > 0 && (
          <div style={{ ...chipRowStyle, marginTop: "var(--sp-2)" }} data-testid="stage-missing-bindings">
            {missingBindings.map((id) => (
              <Badge key={id} tone="danger" data-testid={`stage-missing-${id}`}>
                {strings.workflows.stage.missingBinding(id)}
                <button
                  type="button"
                  aria-label={strings.workflows.stage.removeOutput(id)}
                  onClick={() => removeSkill(id)}
                  style={chipRemoveStyle}
                >
                  ×
                </button>
              </Badge>
            ))}
          </div>
        )}
      </div>

      {/* Effective-skills summary (global ∪ stage, deduped) */}
      <div data-testid="stage-effective-skills">
        <span style={labelStyle}>{strings.workflows.stage.effectiveSkillsLabel}</span>
        <div style={{ ...noteStyle, marginTop: "var(--sp-1)" }}>
          {effective.length === 0
            ? strings.workflows.stage.noEffectiveSkills
            : effective.map(skillLabel).join(", ")}
          <span style={{ marginLeft: "var(--sp-2)", opacity: 0.7 }}>
            ({strings.workflows.stage.effectiveSkillsHint})
          </span>
        </div>
      </div>

      {/* Gate */}
      <div>
        <span style={labelStyle}>{strings.workflows.stage.gateLabel}</span>
        <div style={{ marginTop: "var(--sp-1)" }}>
          <SegmentedPill
            options={gateOptions}
            value={stage.gate}
            onChange={(gate) => onChange({ ...stage, gate })}
            ariaLabel={strings.workflows.stage.gateAria}
            data-testid="stage-gate"
          />
        </div>
      </div>

      {/* Agent & context panel */}
      <div style={agentPanelStyle} data-testid="stage-agent-panel">
        <span style={labelStyle}>{strings.workflows.agentPanel.sectionLabel}</span>

        <Field label={strings.workflows.agentPanel.agentLabel}>
          <select
            data-testid="stage-agent"
            aria-label={strings.workflows.agentPanel.agentAria}
            value={stage.agent ?? INHERIT_VALUE}
            onChange={(e) =>
              onChange({ ...stage, agent: e.target.value === INHERIT_VALUE ? null : e.target.value })
            }
            style={{
              padding: "var(--sp-2) var(--sp-3)",
              fontSize: "var(--fs-md)",
              fontFamily: "var(--font-ui)",
              color: "var(--ink)",
              background: "var(--panel)",
              border: "1px solid transparent",
              borderRadius: "var(--r-sm)",
            }}
          >
            <option value={INHERIT_VALUE}>{strings.workflows.agentPanel.inherit}</option>
            {KNOWN_AGENTS.map((a) => (
              <option key={a} value={a}>
                {agentLabel(a)}
              </option>
            ))}
            {/* Preserve a pinned-but-unknown agent as a selectable option so opening a legacy
                workflow never silently drops it — paired with the marker below. */}
            {agentPinnedUnknown && stage.agent !== null && (
              <option value={stage.agent}>{stage.agent}</option>
            )}
          </select>
        </Field>

        {stage.agent === null ? (
          <span style={noteStyle} data-testid="stage-agent-inherited">
            {strings.workflows.agentPanel.inheritedLabel(agentLabel(effectiveAgent))}
          </span>
        ) : agentPinnedUnknown ? (
          <Badge tone="danger" data-testid="stage-agent-unavailable">
            {strings.workflows.stage.agentUnavailable(stage.agent)}
          </Badge>
        ) : null}

        <div>
          <span style={labelStyle}>{strings.workflows.agentPanel.contextLabel}</span>
          <div style={{ marginTop: "var(--sp-1)" }}>
            <SegmentedPill
              options={contextOptions}
              value={stage.contextScope}
              onChange={(contextScope) => onChange({ ...stage, contextScope })}
              ariaLabel={strings.workflows.agentPanel.contextAria}
              data-testid="stage-context-scope"
            />
          </div>
          {stage.contextScope === "selected" && (
            <div style={{ ...noteStyle, marginTop: "var(--sp-2)" }} data-testid="stage-selected-note">
              {strings.workflows.agentPanel.selectedNote}
            </div>
          )}
        </div>

        {/* Outputs chips */}
        <div>
          <span style={labelStyle}>{strings.workflows.stage.outputsLabel}</span>
          <div style={{ ...chipRowStyle, marginTop: "var(--sp-1)" }} data-testid="stage-outputs">
            {stage.outputs.map((out) => (
              <Badge key={out} tone="muted" data-testid={`stage-output-${out}`}>
                {out}
                <button
                  type="button"
                  aria-label={strings.workflows.stage.removeOutput(out)}
                  onClick={() =>
                    onChange({ ...stage, outputs: stage.outputs.filter((o) => o !== out) })
                  }
                  style={chipRemoveStyle}
                >
                  ×
                </button>
              </Badge>
            ))}
            <input
              data-testid="stage-output-input"
              aria-label={strings.workflows.stage.outputsAria}
              placeholder={strings.workflows.stage.outputsPlaceholder}
              value={outputDraft}
              onChange={(e) => setOutputDraft(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") {
                  e.preventDefault();
                  commitOutput();
                }
              }}
              style={{
                flex: "1 1 120px",
                minWidth: 100,
                fontFamily: "var(--font-mono)",
                fontSize: "var(--fs-sm)",
                color: "var(--ink)",
                background: "var(--panel)",
                border: "1px solid transparent",
                borderRadius: "var(--r-sm)",
                padding: "3px 6px",
              }}
            />
            <Button
              type="button"
              variant="ghost"
              size="sm"
              data-testid="stage-output-add"
              onClick={commitOutput}
              style={{ flexShrink: 0, whiteSpace: "nowrap" }}
            >
              {strings.workflows.stage.addOutput}
            </Button>
          </div>
        </div>
      </div>

      <div style={{ display: "flex", gap: "var(--sp-2)", justifyContent: "space-between" }}>
        <Button type="button" variant="danger" size="sm" data-testid="stage-remove" onClick={onRemove}>
          {strings.workflows.stage.remove}
        </Button>
        <Button type="button" variant="ghost" size="sm" data-testid="stage-done" onClick={onDone}>
          {strings.workflows.stage.done}
        </Button>
      </div>
    </div>
  );
}
