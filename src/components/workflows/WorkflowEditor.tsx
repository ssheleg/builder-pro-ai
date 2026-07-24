import { useMemo, useRef, useState, type CSSProperties, type JSX } from "react";
import { useAppStore } from "../../store/store";
import { describeOrchdError } from "../../ipc/orchd";
import type { Skill, Stage, SupervisorConfig, WorkflowScope } from "../../ipc/orchd-types";
import { Badge, Button, Field, Input, SegmentedPill, TextArea } from "../../ui/primitives";
import { strings } from "../../strings";
import { StageDetail } from "./StageDetail";
import { KNOWN_AGENTS, agentLabel, computeTerminalGroups, newStageId } from "./agents";

/**
 * The editor's working draft (SW1). `id: ""` ⇒ create; a non-empty `id` ⇒ save in place. Built by
 * `WorkflowsView` from a blank template ("+ New"), an existing workflow ("Open"), or a "(copy)"
 * ("Duplicate"). The editor holds a mutable copy in local state and never mutates the store's row
 * until Save round-trips (mirrors `RulesetPanel`'s draft discipline).
 */
export interface WorkflowDraft {
  id: string;
  name: string;
  description: string;
  scope: WorkflowScope;
  projectId: string | null;
  defaultAgent: string;
  stages: Stage[];
  globalSkillIds: string[];
  supervisor: SupervisorConfig;
}

/** SCN-046 "Recommended scope" preset (reused verbatim from the ruleset CEO section): the two safe
 * classes it seeds into the CEO's delegation scope. */
const RECOMMENDED_SCOPE_CLASSES = ["safe-shell", "file-write"] as const;

const wrapStyle: CSSProperties = {
  flex: 1,
  minWidth: 0,
  overflowY: "auto",
  padding: "var(--sp-5)",
  display: "flex",
  flexDirection: "column",
  gap: "var(--sp-4)",
  fontFamily: "var(--font-ui)",
};

const labelStyle: CSSProperties = {
  fontSize: "var(--fs-xs)",
  fontWeight: 600,
  color: "var(--muted)",
  textTransform: "uppercase",
  letterSpacing: "0.05em",
};

const cardStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: "var(--sp-3)",
  padding: "var(--sp-4)",
  background: "var(--panel)",
  borderRadius: "var(--r-md)",
};

const terminalHeaderStyle: CSSProperties = {
  fontSize: "var(--fs-sm)",
  fontWeight: 700,
  color: "var(--ink)",
  padding: "var(--sp-1) var(--sp-2)",
  background: "var(--panel-2)",
  borderRadius: "var(--r-sm)",
  marginTop: "var(--sp-3)",
  marginBottom: "var(--sp-2)",
};

const stageRowStyle: CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: "var(--sp-2)",
  padding: "var(--sp-2) var(--sp-3)",
  background: "var(--panel-2)",
  borderRadius: "var(--r-sm)",
  marginBottom: "var(--sp-2)",
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

const chipInputStyle: CSSProperties = {
  flex: "1 1 140px",
  minWidth: 100,
  fontFamily: "var(--font-mono)",
  fontSize: "var(--fs-sm)",
  color: "var(--ink)",
  background: "var(--panel-2)",
  border: "1px solid transparent",
  borderRadius: "var(--r-sm)",
  padding: "3px 6px",
};

const supervisorSectionStyle: CSSProperties = {
  display: "flex",
  flexDirection: "column",
  gap: "var(--sp-2)",
  marginTop: "var(--sp-2)",
  paddingTop: "var(--sp-3)",
  borderTop: "1px solid var(--line)",
};

const mutedLineStyle: CSSProperties = {
  fontSize: "var(--fs-xs)",
  color: "var(--muted)",
  lineHeight: 1.5,
};

const pendingNoteStyle: CSSProperties = {
  padding: "var(--sp-2) var(--sp-3)",
  boxShadow: "inset 3px 0 0 var(--info)",
  background: "var(--info-weak)",
  color: "var(--ink)",
  fontSize: "var(--fs-xs)",
  borderRadius: "var(--r-sm)",
  lineHeight: 1.5,
};

/** Small chip/list input (CEO delegated classes + custom rules) — a trimmed, non-empty, non-dup
 * draft is committed via the button or Enter; an empty draft is a silent no-op. Local to this
 * editor (a per-surface copy of the ruleset `ChipList`, same as the repo's other per-surface
 * copies). */
function ChipList(props: {
  testIdPrefix: string;
  ariaLabel: string;
  placeholder: string;
  values: string[];
  onAdd: (v: string) => void;
  onRemove: (v: string) => void;
}): JSX.Element {
  const { testIdPrefix, ariaLabel, placeholder, values, onAdd, onRemove } = props;
  const [draft, setDraft] = useState("");
  function commit(): void {
    const trimmed = draft.trim();
    if (trimmed === "") return;
    onAdd(trimmed);
    setDraft("");
  }
  return (
    <div style={chipRowStyle}>
      {values.map((v) => (
        <Badge key={v} data-testid={`${testIdPrefix}-chip-${v}`} tone="muted">
          {v}
          <button
            type="button"
            data-testid={`${testIdPrefix}-remove-${v}`}
            aria-label={strings.workflows.ceo.deleteEntry(v)}
            onClick={() => onRemove(v)}
            style={chipRemoveStyle}
          >
            ×
          </button>
        </Badge>
      ))}
      <input
        data-testid={`${testIdPrefix}-input`}
        aria-label={ariaLabel}
        placeholder={placeholder}
        value={draft}
        onChange={(e) => setDraft(e.target.value)}
        onKeyDown={(e) => {
          if (e.key === "Enter") {
            e.preventDefault();
            commit();
          }
        }}
        style={chipInputStyle}
      />
      <Button
        type="button"
        variant="ghost"
        size="sm"
        data-testid={`${testIdPrefix}-add`}
        onClick={commit}
        style={{ flexShrink: 0, whiteSpace: "nowrap" }}
      >
        {strings.workflows.ceo.addEntry}
      </Button>
    </div>
  );
}

/**
 * Client twin of the daemon's fail-closed workflow validation (mirrors `RulesetPanel`'s
 * `validatePolicy`): blocks a doomed Save before it round-trips. The daemon's `validate` is the
 * authoritative guard — this is a UX nicety, never the only line of defense (a server-side
 * `Validation` still surfaces as a toast). Checks: a non-empty name; a project-scoped workflow has a
 * project; at least one stage; every stage has a name AND a prompt; an enabled CEO has ≥1 delegated
 * class.
 */
export function validateWorkflow(draft: WorkflowDraft): string | null {
  if (draft.name.trim() === "") return strings.workflows.editor.errNameRequired;
  if (draft.scope === "project" && (draft.projectId === null || draft.projectId === "")) {
    return strings.workflows.editor.errProjectRequired;
  }
  if (draft.stages.length === 0) return strings.workflows.editor.errNoStages;
  for (const stage of draft.stages) {
    if (stage.name.trim() === "" || stage.prompt.trim() === "") {
      const label =
        stage.name.trim() === "" ? strings.workflows.editor.stageUnnamedFallback : stage.name;
      return strings.workflows.editor.errStageIncomplete(label);
    }
  }
  if (draft.supervisor.enabled && draft.supervisor.delegatedClasses.length === 0) {
    return strings.workflows.editor.errCeoNoClasses;
  }
  return null;
}

/** A fresh blank stage (defaults: inherit the workflow agent, inherit context, auto gate). */
function newStage(): Stage {
  return {
    id: newStageId(),
    name: "",
    prompt: "",
    skillIds: [],
    agent: null,
    contextScope: "inherit",
    outputs: [],
    gate: "auto",
  };
}

/**
 * Workflow editor (SCR-02, SW1). Authors a workflow's default agent, ordered stages (grouped into
 * terminal brackets — a pure view over each stage's effective agent), global skills and CEO
 * oversight, then Saves via `orchdUpsertWorkflow`.
 *
 * Terminal brackets (the SCR-02 grouping): `computeTerminalGroups(stages, defaultAgent)` walks the
 * stages and accretes consecutive same-effective-agent stages into one terminal; a single-agent
 * workflow is one terminal, and every agent change is a boundary. Reorder (up/down) and the
 * default-agent picker both re-derive the grouping live.
 *
 * CEO oversight reuses the `RulesetPanel` supervisor pattern (SCN-046 register): an enable toggle,
 * delegated-class chips, the "Recommended scope" preset (seeds safe-shell + file-write), an
 * instruction textarea, custom-rules chips, and the S6b pending note. Save is blocked when the CEO
 * is enabled with an empty scope (the client twin of the daemon's guard). PLUMBING ONLY — persisting
 * the config never starts a CEO.
 *
 * Honest degradation: while `orchdDown`, "Save workflow" is disabled (drafts stay editable); a
 * rejected Save keeps the draft on screen and toasts the mapped message.
 */
export function WorkflowEditor(props: { draft: WorkflowDraft; onDone: () => void }): JSX.Element {
  const skills = useAppStore((s) => s.skills);
  const projects = useAppStore((s) => s.projects);
  const orchdDown = useAppStore((s) => s.orchdDown);
  const upsertWorkflow = useAppStore((s) => s.upsertWorkflow);
  const showToast = useAppStore((s) => s.showToast);

  const [draft, setDraft] = useState<WorkflowDraft>(props.draft);
  const [openStageIndex, setOpenStageIndex] = useState<number | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  const initialJson = useRef(JSON.stringify(props.draft));

  const dirty = JSON.stringify(draft) !== initialJson.current;

  const terminals = useMemo(
    () => computeTerminalGroups(draft.stages, draft.defaultAgent),
    [draft.stages, draft.defaultAgent],
  );

  function update(partial: Partial<WorkflowDraft>): void {
    setDraft((d) => ({ ...d, ...partial }));
    setError(null);
  }

  function updateStage(index: number, stage: Stage): void {
    setDraft((d) => ({ ...d, stages: d.stages.map((s, i) => (i === index ? stage : s)) }));
    setError(null);
  }

  function addStage(): void {
    setDraft((d) => ({ ...d, stages: [...d.stages, newStage()] }));
    setOpenStageIndex(draft.stages.length);
    setError(null);
  }

  function removeStage(index: number): void {
    setDraft((d) => ({ ...d, stages: d.stages.filter((_, i) => i !== index) }));
    setOpenStageIndex(null);
    setError(null);
  }

  function moveStage(index: number, dir: -1 | 1): void {
    const target = index + dir;
    if (target < 0 || target >= draft.stages.length) return;
    setDraft((d) => {
      const stages = [...d.stages];
      [stages[index], stages[target]] = [stages[target], stages[index]];
      return { ...d, stages };
    });
    // Keep the same stage open if it was the one being moved.
    setOpenStageIndex((cur) => (cur === index ? target : cur === target ? index : cur));
    setError(null);
  }

  function toggleGlobalSkill(id: string, checked: boolean): void {
    setDraft((d) => ({
      ...d,
      globalSkillIds: checked
        ? [...d.globalSkillIds, id]
        : d.globalSkillIds.filter((s) => s !== id),
    }));
    setError(null);
  }

  function setSupervisor(partial: Partial<SupervisorConfig>): void {
    setDraft((d) => ({ ...d, supervisor: { ...d.supervisor, ...partial } }));
    setError(null);
  }

  function toggleDelegatedClass(cls: string, checked: boolean): void {
    setDraft((d) => ({
      ...d,
      supervisor: {
        ...d.supervisor,
        delegatedClasses: checked
          ? Array.from(new Set([...d.supervisor.delegatedClasses, cls]))
          : d.supervisor.delegatedClasses.filter((c) => c !== cls),
      },
    }));
    setError(null);
  }

  function applyRecommendedScope(): void {
    setDraft((d) => ({
      ...d,
      supervisor: {
        ...d.supervisor,
        delegatedClasses: Array.from(
          new Set([...d.supervisor.delegatedClasses, ...RECOMMENDED_SCOPE_CLASSES]),
        ),
      },
    }));
    setError(null);
  }

  async function handleSave(): Promise<void> {
    const validationError = validateWorkflow(draft);
    if (validationError !== null) {
      setError(validationError);
      return;
    }
    setError(null);
    setSaving(true);
    try {
      await upsertWorkflow({
        id: draft.id,
        name: draft.name,
        description: draft.description,
        scope: draft.scope,
        projectId: draft.scope === "project" ? draft.projectId : null,
        defaultAgent: draft.defaultAgent,
        stages: draft.stages,
        globalSkillIds: draft.globalSkillIds,
        supervisor: draft.supervisor,
      });
      props.onDone();
    } catch (e) {
      showToast(describeOrchdError(e));
      setSaving(false);
    }
  }

  const skillById = new Map<string, Skill>(skills.map((s) => [s.id, s]));
  const missingGlobalBindings = draft.globalSkillIds.filter((id) => !skillById.has(id));

  const scopeOptions: readonly { value: WorkflowScope; label: string }[] = [
    { value: "global", label: strings.workflows.library.scopeGlobal },
    { value: "project", label: strings.workflows.library.scopeProject },
  ];

  const activeProjects = projects.filter((p) => p.status === "active");

  return (
    <div data-testid="workflow-editor" style={wrapStyle}>
      <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between", gap: "var(--sp-3)", flexWrap: "wrap" }}>
        <Button type="button" variant="ghost" size="sm" data-testid="workflow-editor-back" onClick={props.onDone}>
          {strings.workflows.editor.backToLibrary}
        </Button>
        <div style={{ display: "flex", alignItems: "center", gap: "var(--sp-3)" }}>
          {dirty && (
            <span data-testid="workflow-editor-unsaved" style={mutedLineStyle}>
              {strings.workflows.editor.unsavedHint}
            </span>
          )}
          <Button
            type="button"
            variant="primary"
            size="sm"
            data-testid="workflow-editor-save"
            disabled={orchdDown || saving}
            loading={saving}
            onClick={() => void handleSave()}
          >
            {strings.workflows.editor.save}
          </Button>
        </div>
      </div>

      {/* Identity + scope + default agent */}
      <div style={cardStyle}>
        <Field label={strings.workflows.editor.nameLabel}>
          <Input
            data-testid="workflow-name"
            aria-label={strings.workflows.editor.nameAria}
            placeholder={strings.workflows.editor.namePlaceholder}
            value={draft.name}
            onChange={(e) => update({ name: e.target.value })}
          />
        </Field>
        <Field label={strings.workflows.editor.descriptionLabel}>
          <TextArea
            data-testid="workflow-description"
            aria-label={strings.workflows.editor.descriptionAria}
            placeholder={strings.workflows.editor.descriptionPlaceholder}
            value={draft.description}
            onChange={(e) => update({ description: e.target.value })}
            rows={2}
          />
        </Field>

        <div>
          <span style={labelStyle}>{strings.workflows.editor.scopeLabel}</span>
          <div style={{ marginTop: "var(--sp-1)" }}>
            <SegmentedPill
              options={scopeOptions}
              value={draft.scope}
              onChange={(scope) => update({ scope, projectId: scope === "global" ? null : draft.projectId })}
              ariaLabel={strings.workflows.editor.scopeLabel}
              data-testid="workflow-scope"
            />
          </div>
          {draft.scope === "project" && (
            <div style={{ marginTop: "var(--sp-2)" }}>
              <select
                data-testid="workflow-project"
                aria-label={strings.workflows.library.scopeProject}
                value={draft.projectId ?? ""}
                onChange={(e) => update({ projectId: e.target.value === "" ? null : e.target.value })}
                style={{
                  padding: "var(--sp-2) var(--sp-3)",
                  fontSize: "var(--fs-md)",
                  fontFamily: "var(--font-ui)",
                  color: "var(--ink)",
                  background: "var(--panel-2)",
                  border: "1px solid transparent",
                  borderRadius: "var(--r-sm)",
                }}
              >
                <option value="">—</option>
                {activeProjects.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.name}
                  </option>
                ))}
              </select>
            </div>
          )}
        </div>

        <Field label={strings.workflows.editor.defaultAgentLabel}>
          <select
            data-testid="workflow-default-agent"
            aria-label={strings.workflows.editor.defaultAgentAria}
            value={draft.defaultAgent}
            onChange={(e) => update({ defaultAgent: e.target.value })}
            style={{
              padding: "var(--sp-2) var(--sp-3)",
              fontSize: "var(--fs-md)",
              fontFamily: "var(--font-ui)",
              color: "var(--ink)",
              background: "var(--panel-2)",
              border: "1px solid transparent",
              borderRadius: "var(--r-sm)",
            }}
          >
            {KNOWN_AGENTS.map((a) => (
              <option key={a} value={a}>
                {agentLabel(a)}
              </option>
            ))}
          </select>
        </Field>
      </div>

      {/* Stages, grouped into terminal brackets */}
      <div style={cardStyle}>
        <div style={{ display: "flex", alignItems: "center", justifyContent: "space-between" }}>
          <span style={labelStyle}>{strings.workflows.editor.stagesLabel}</span>
          <Button type="button" variant="ghost" size="sm" data-testid="workflow-add-stage" onClick={addStage}>
            {strings.workflows.editor.addStage}
          </Button>
        </div>

        {draft.stages.length === 0 ? (
          <div style={mutedLineStyle} data-testid="workflow-no-stages">
            {strings.workflows.editor.noStages}
          </div>
        ) : (
          <div data-testid="workflow-terminals">
            {terminals.map((group, gi) => (
              <div key={gi} data-testid={`workflow-terminal-${gi}`}>
                <div style={terminalHeaderStyle} data-testid={`workflow-terminal-header-${gi}`}>
                  {strings.workflows.editor.terminalHeader(
                    gi + 1,
                    agentLabel(group.agent),
                    group.stages.length,
                  )}
                </div>
                {group.stages.map(({ stage, index }) => {
                  const stageName =
                    stage.name.trim() === ""
                      ? strings.workflows.editor.stageUnnamedFallback
                      : stage.name;
                  return (
                    <div key={stage.id}>
                      <div style={stageRowStyle} data-testid={`workflow-stage-row-${index}`}>
                        <span style={{ flex: 1, fontSize: "var(--fs-md)", color: "var(--ink)" }}>
                          {stageName}
                          {stage.agent === null && (
                            <span style={{ marginLeft: "var(--sp-2)", ...mutedLineStyle }}>
                              {strings.workflows.editor.stageInherits(agentLabel(draft.defaultAgent))}
                            </span>
                          )}
                        </span>
                        <Button
                          type="button"
                          variant="ghost"
                          size="sm"
                          data-testid={`workflow-stage-up-${index}`}
                          aria-label={strings.common.moveUp}
                          disabled={index === 0}
                          onClick={() => moveStage(index, -1)}
                        >
                          ↑
                        </Button>
                        <Button
                          type="button"
                          variant="ghost"
                          size="sm"
                          data-testid={`workflow-stage-down-${index}`}
                          aria-label={strings.common.moveDown}
                          disabled={index === draft.stages.length - 1}
                          onClick={() => moveStage(index, 1)}
                        >
                          ↓
                        </Button>
                        <Button
                          type="button"
                          variant="ghost"
                          size="sm"
                          data-testid={`workflow-stage-edit-${index}`}
                          onClick={() =>
                            setOpenStageIndex((cur) => (cur === index ? null : index))
                          }
                        >
                          {strings.workflows.editor.editStage}
                        </Button>
                      </div>
                      {openStageIndex === index && (
                        <div style={{ marginBottom: "var(--sp-3)" }}>
                          <StageDetail
                            stage={stage}
                            skills={skills}
                            defaultAgent={draft.defaultAgent}
                            globalSkillIds={draft.globalSkillIds}
                            onChange={(next) => updateStage(index, next)}
                            onRemove={() => removeStage(index)}
                            onDone={() => setOpenStageIndex(null)}
                          />
                        </div>
                      )}
                    </div>
                  );
                })}
              </div>
            ))}
          </div>
        )}
      </div>

      {/* Global skills */}
      <div style={cardStyle}>
        <span style={labelStyle}>{strings.workflows.editor.globalSkillsLabel}</span>
        <span style={mutedLineStyle}>{strings.workflows.editor.globalSkillsHint}</span>
        {skills.length === 0 ? (
          <div style={mutedLineStyle} data-testid="workflow-no-skills">
            {strings.workflows.editor.noSkillsAvailable}
          </div>
        ) : (
          <div style={chipRowStyle} data-testid="workflow-global-skills">
            {skills.map((skill) => (
              <label
                key={skill.id}
                style={{ display: "inline-flex", alignItems: "center", gap: "var(--sp-1)", fontSize: "var(--fs-sm)", color: "var(--ink)", cursor: "pointer" }}
              >
                <input
                  type="checkbox"
                  data-testid={`workflow-global-skill-${skill.id}`}
                  aria-label={skill.name}
                  checked={draft.globalSkillIds.includes(skill.id)}
                  onChange={(e) => toggleGlobalSkill(skill.id, e.target.checked)}
                />
                <span>{skill.name}</span>
              </label>
            ))}
          </div>
        )}
        {missingGlobalBindings.length > 0 && (
          <div style={chipRowStyle} data-testid="workflow-global-missing">
            {missingGlobalBindings.map((id) => (
              <Badge key={id} tone="danger" data-testid={`workflow-global-missing-${id}`}>
                {strings.workflows.stage.missingBinding(id)}
                <button
                  type="button"
                  aria-label={strings.workflows.ceo.deleteEntry(id)}
                  onClick={() => toggleGlobalSkill(id, false)}
                  style={chipRemoveStyle}
                >
                  ×
                </button>
              </Badge>
            ))}
          </div>
        )}
      </div>

      {/* CEO oversight (reuse of the RulesetPanel supervisor pattern) */}
      <div style={cardStyle}>
        <div style={supervisorSectionStyle} data-testid="workflow-ceo">
          <span style={labelStyle}>{strings.workflows.ceo.sectionLabel}</span>

          <label style={{ display: "flex", alignItems: "center", gap: "var(--sp-2)", fontSize: "var(--fs-md)", color: "var(--ink)", cursor: "pointer" }}>
            <input
              type="checkbox"
              data-testid="workflow-ceo-enable"
              aria-label={strings.workflows.ceo.enableAria}
              checked={draft.supervisor.enabled}
              onChange={(e) => setSupervisor({ enabled: e.target.checked })}
            />
            <span>{strings.workflows.ceo.enableLabel}</span>
          </label>

          {draft.supervisor.enabled ? (
            <>
              <span style={labelStyle}>{strings.workflows.ceo.delegatedLabel}</span>
              {draft.supervisor.delegatedClasses.length === 0 ? (
                <span style={mutedLineStyle} data-testid="workflow-ceo-no-classes">
                  {strings.workflows.ceo.noClasses}
                </span>
              ) : (
                <div style={chipRowStyle} data-testid="workflow-ceo-classes">
                  {draft.supervisor.delegatedClasses.map((cls) => (
                    <Badge key={cls} tone="muted" data-testid={`workflow-ceo-class-${cls}`}>
                      {cls}
                      <button
                        type="button"
                        aria-label={strings.workflows.ceo.deleteEntry(cls)}
                        onClick={() => toggleDelegatedClass(cls, false)}
                        style={chipRemoveStyle}
                      >
                        ×
                      </button>
                    </Badge>
                  ))}
                </div>
              )}

              <ChipList
                testIdPrefix="workflow-ceo-class"
                ariaLabel={strings.workflows.ceo.classAria}
                placeholder={strings.workflows.ceo.classPlaceholder}
                values={[]}
                onAdd={(v) => toggleDelegatedClass(v, true)}
                onRemove={() => {
                  /* removal handled by the chips above */
                }}
              />

              <Button
                type="button"
                variant="ghost"
                size="sm"
                data-testid="workflow-ceo-recommended"
                onClick={applyRecommendedScope}
                style={{ alignSelf: "flex-start" }}
              >
                {strings.workflows.ceo.recommendedScope}
              </Button>

              <span style={labelStyle}>{strings.workflows.ceo.instructionLabel}</span>
              <TextArea
                data-testid="workflow-ceo-instruction"
                aria-label={strings.workflows.ceo.instructionAria}
                placeholder={strings.workflows.ceo.instructionPlaceholder}
                value={draft.supervisor.instruction}
                onChange={(e) => setSupervisor({ instruction: e.target.value })}
                rows={3}
              />

              <span style={labelStyle}>{strings.workflows.ceo.customRulesLabel}</span>
              <ChipList
                testIdPrefix="workflow-ceo-rule"
                ariaLabel={strings.workflows.ceo.customRuleAria}
                placeholder={strings.workflows.ceo.customRulePlaceholder}
                values={draft.supervisor.customRules}
                onAdd={(v) =>
                  setSupervisor({
                    customRules: Array.from(new Set([...draft.supervisor.customRules, v])),
                  })
                }
                onRemove={(v) =>
                  setSupervisor({
                    customRules: draft.supervisor.customRules.filter((r) => r !== v),
                  })
                }
              />
            </>
          ) : (
            <span style={mutedLineStyle} data-testid="workflow-ceo-disabled-hint">
              {strings.workflows.ceo.disabledHint}
            </span>
          )}

          <div data-testid="workflow-ceo-pending" role="note" style={pendingNoteStyle}>
            {strings.workflows.ceo.pendingNote}
          </div>
        </div>
      </div>

      {error !== null && (
        <span data-testid="workflow-editor-error" role="alert" style={{ fontSize: "var(--fs-sm)", color: "var(--danger)" }}>
          {error}
        </span>
      )}
    </div>
  );
}
