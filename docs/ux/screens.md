<!-- Managed with super-ux (ux-contract v4). The design map: every screen and state with its Figma frame, wireframe, code coverage, and resources. Update in the same change as any interface change; when Figma is enabled, update the frame too. -->

# Screens — The UI Map

Seeded 2026-07-24 with the **workflow feature** (SW1 / SCN-060..064). Figma is
enabled for this feature (foundation → Design tooling); the rest of the app is
still text-only and will be reverse-mapped into this file over time. Each screen
below has a real Figma frame in the project file. The frames are built directly
on the **Soft Control Room** tokens — they are design mockups (`designed`); no
product code exists yet (the runtime is S6b, foundation A-10).

## Index

| ID | Screen | Used by | Figma | Status | Coverage |
|----|--------|---------|-------|--------|----------|
| SCR-01 | Workflows library | FLW-23 | [frame 3-7](https://www.figma.com/design/q3tTcpi60BOCn0VIIiX4wg/Builder-Pro-AI-Workflows?node-id=3-7) | designed | none yet |
| SCR-02 | Workflow editor | FLW-23 | [frame 3-11](https://www.figma.com/design/q3tTcpi60BOCn0VIIiX4wg/Builder-Pro-AI-Workflows?node-id=3-11) | designed | none yet |
| SCR-03 | Stage detail | FLW-23 | [frame 3-15](https://www.figma.com/design/q3tTcpi60BOCn0VIIiX4wg/Builder-Pro-AI-Workflows?node-id=3-15) | designed | none yet |
| SCR-04 | Run workflow picker | FLW-24 | [frame 3-19](https://www.figma.com/design/q3tTcpi60BOCn0VIIiX4wg/Builder-Pro-AI-Workflows?node-id=3-19) | designed | none yet |
| SCR-05 | Run detail | FLW-24 | [frame 3-23](https://www.figma.com/design/q3tTcpi60BOCn0VIIiX4wg/Builder-Pro-AI-Workflows?node-id=3-23) | designed | none yet |
| SCR-06 | Home digest (workflows) | FLW-24 | [frame 3-27](https://www.figma.com/design/q3tTcpi60BOCn0VIIiX4wg/Builder-Pro-AI-Workflows?node-id=3-27) | designed | none yet |
| SCR-07 | Run journal (heartbeat) | FLW-24 | [frame 17-2](https://www.figma.com/design/q3tTcpi60BOCn0VIIiX4wg/Builder-Pro-AI-Workflows?node-id=17-2) | designed | none yet |

## Design system

- **Figma library:** none — frames are built directly on the app's own tokens (no separate Figma component library yet)
- **Tokens in code:** `src/ui/tokens.css` (Soft Control Room — light + dark)
- **Component source:** `src/ui/primitives.tsx` (Panel, Stat, SegmentedPill, Heatmap, Button, Field, EmptyState, Dialog)
- **Assets:** Space Grotesk (display face), Inter (UI), Roboto Mono (code/prompt)

## Screens

### SCR-01: Workflows library
- **Used by:** FLW-23 (step 1)
- **Purpose:** the home of workflow-as-data — list saved workflows, scoped global/project, and start a new one (SCN-060)
- **Elements:** "⚙ Workflows" nav entry, "Workflows" title, scope segmented pill (All | Global | Project), "+ New workflow" *(primary)*, per-workflow row (name, description, stage-count chip, scope chip, skills-count chip, Run → / Open / Duplicate / Delete)
- **States:**
  | State | Trigger | Figma frame | Behavior |
  |-------|---------|-------------|----------|
  | success | ≥1 workflow saved | [3-7](https://www.figma.com/design/q3tTcpi60BOCn0VIIiX4wg/Builder-Pro-AI-Workflows?node-id=3-7) | rows listed, scope filter active |
  | empty | none saved | [3-7](https://www.figma.com/design/q3tTcpi60BOCn0VIIiX4wg/Builder-Pro-AI-Workflows?node-id=3-7) | "No workflows yet — compose one to reuse across projects." (depicted inline) |
- **Coverage:** none yet
- **Scenarios:** SCN-060
- **Resources:** reuses the file-backed scoped-entity pattern of RuleSets/Docs; skills come from the registry (SCN-035)
- **Status:** designed

### SCR-02: Workflow editor
- **Used by:** FLW-23 (steps 2-6)
- **Purpose:** author the workflow — ordered stages grouped into terminals by agent, global skills, CEO oversight (SCN-061/062/065)
- **Elements:** breadcrumb (⚙ Workflows › name › scope), **default-agent chip**, "unsaved changes" hint, "Save workflow" *(primary)*, stage list **grouped into terminal brackets** (Terminal N · agent · stage-count; a boundary where the agent changes), reorderable stage rows (drag handle, order badge, name, skills summary, gate chip auto|manual), "+ Add stage", global-skills picker (chips + add), effective-skills note, CEO oversight panel (enable toggle, delegated gate-class chips, inherited-caps line, S6b pending note)
- **States:**
  | State | Trigger | Figma frame | Behavior |
  |-------|---------|-------------|----------|
  | success | editing a saved workflow | [3-11](https://www.figma.com/design/q3tTcpi60BOCn0VIIiX4wg/Builder-Pro-AI-Workflows?node-id=3-11) | stages + global skills + CEO section |
  | error | invalid (stage without a prompt, empty CEO scope) | [3-11](https://www.figma.com/design/q3tTcpi60BOCn0VIIiX4wg/Builder-Pro-AI-Workflows?node-id=3-11) | inline flag on the offending stage/section, Save blocked |
- **Coverage:** none yet
- **Scenarios:** SCN-061, SCN-062, SCN-065
- **Resources:** CEO section reuses the RulesetPanel supervisor pattern (`SupervisorConfig`, SCN-046); the terminal grouping is derived from the per-stage agent (consecutive same-agent stages = one terminal)
- **Status:** designed

### SCR-03: Stage detail
- **Used by:** FLW-23 (step 3)
- **Purpose:** configure one stage — its prompt/command, bound skills, gate, **agent, context scope, outputs** (SCN-061/065)
- **Elements:** breadcrumb, "Done", prompt/command markdown editor (mono), stage-skills picker (chips + add), effective-skills summary (global ∪ stage, read-only), missing-binding marker (danger), gate segmented (auto | manual) + explanation, **Agent & context panel** — agent picker (inherit + claude-code/hermes/opencode/kilo), context-scope segmented (inherit | handoff | project | selected), outputs field (named artifacts)
- **States:**
  | State | Trigger | Figma frame | Behavior |
  |-------|---------|-------------|----------|
  | success | stage open, all skills present | [3-15](https://www.figma.com/design/q3tTcpi60BOCn0VIIiX4wg/Builder-Pro-AI-Workflows?node-id=3-15) | prompt + skills + gate editable |
  | error | a bound skill was removed from the registry | [3-15](https://www.figma.com/design/q3tTcpi60BOCn0VIIiX4wg/Builder-Pro-AI-Workflows?node-id=3-15) | "missing from the registry — fix or remove before running" (depicted inline) |
- **Coverage:** none yet
- **Scenarios:** SCN-061, SCN-065
- **Resources:** skills referenced by id from the registry (SCN-035); an agent-turn stage (v1); agents are the ones the app already launches; context `handoff` = run journal + declared outputs
- **Status:** designed

### SCR-04: Run workflow picker
- **Used by:** FLW-24 (step 1)
- **Purpose:** start a run — pick a saved workflow to run on the current project (SCN-063)
- **Elements:** modal card over a scrim, "Run workflow" title + "on {project}", radio rows (workflow name, stage-count · CEO on/off), "Run workflow" *(primary)*, Cancel, S6b pending note
- **States:**
  | State | Trigger | Figma frame | Behavior |
  |-------|---------|-------------|----------|
  | success | ≥1 saved workflow | [3-19](https://www.figma.com/design/q3tTcpi60BOCn0VIIiX4wg/Builder-Pro-AI-Workflows?node-id=3-19) | selectable list, Run enabled |
- **Coverage:** none yet
- **Scenarios:** SCN-063
- **Resources:** honest S6b boundary — trigger is live, execution awaits the runtime (A-10)
- **Status:** designed

### SCR-05: Run detail
- **Used by:** FLW-24 (steps 2-3)
- **Purpose:** watch a run advance — terminals, stage progress, CEO decisions, hand-offs, escalations (SCN-063/064/066)
- **Elements:** run title + project chip, status chip (paused · awaiting S6b), **terminal swimlanes** (one per agent block: agent chip, "open terminal →", its stage rows with status dot done/running/waiting/escalated/pending), a **hand-off divider** at each agent boundary (references the run journal + outputs, SCN-066), decision log (CEO/you actor, action, basis citing the journal §, time; escalations in danger)
- **States:**
  | State | Trigger | Figma frame | Behavior |
  |-------|---------|-------------|----------|
  | success | run advancing | [3-23](https://www.figma.com/design/q3tTcpi60BOCn0VIIiX4wg/Builder-Pro-AI-Workflows?node-id=3-23) | rail + decision log reflect live state |
  | loading | run just started | [3-23](https://www.figma.com/design/q3tTcpi60BOCn0VIIiX4wg/Builder-Pro-AI-Workflows?node-id=3-23) | first stage spawning (depicted via running dot) |
  | error | a stage failed/stalled | [3-23](https://www.figma.com/design/q3tTcpi60BOCn0VIIiX4wg/Builder-Pro-AI-Workflows?node-id=3-23) | honest failed/stalled state, never a fake "running" |
- **Coverage:** none yet
- **Scenarios:** SCN-063, SCN-064, SCN-066
- **Resources:** reuses the CEO decision-log surface (SCN-050); terminals map to contiguous same-agent stage blocks; execution is S6b
- **Status:** designed

### SCR-06: Home digest (workflows)
- **Used by:** FLW-24 (step 3)
- **Purpose:** the "since you left" attention surface for workflow runs — hand-offs and escalations (SCN-064)
- **Elements:** "Since you left" heading + count subtitle, "Needs you" escalation card (danger; reason "out of scope: {class}", "open run →" / "Go →"), "Continued by CEO" hand-off rows (done X → started Y, basis, time), "Running" row (project · stage · elapsed, open →), honesty note
- **States:**
  | State | Trigger | Figma frame | Behavior |
  |-------|---------|-------------|----------|
  | success | activity while away | [3-27](https://www.figma.com/design/q3tTcpi60BOCn0VIIiX4wg/Builder-Pro-AI-Workflows?node-id=3-27) | escalations first, then hand-offs, then running |
  | empty | nothing happened | [3-27](https://www.figma.com/design/q3tTcpi60BOCn0VIIiX4wg/Builder-Pro-AI-Workflows?node-id=3-27) | digest suppressed (no empty ceremony) |
- **Coverage:** none yet
- **Scenarios:** SCN-064
- **Resources:** reuses the Home digest + escalation surfaces (SCN-050/055)
- **Status:** designed

### SCR-07: Run journal (heartbeat)
- **Used by:** FLW-24 (steps 1-3)
- **Purpose:** the file-backed hand-off between agents — context crosses a terminal boundary through a human-readable journal, never hidden memory (SCN-066)
- **Elements:** "Run journal" title + `handoff.md` chip + "file-backed · reuses Docs" chip + "open as file →", per-stage entries (stage · name, agent chip, terminal chip, time, status chip; DID / OUTPUTS (mono) / FOR THE NEXT AGENT fields), an honesty note ("no hand-off written — the previous stage left no journal entry", never proceed blind)
- **States:**
  | State | Trigger | Figma frame | Behavior |
  |-------|---------|-------------|----------|
  | success | ≥1 journal entry | [17-2](https://www.figma.com/design/q3tTcpi60BOCn0VIIiX4wg/Builder-Pro-AI-Workflows?node-id=17-2) | entries stacked newest-relevant; next agent reads it |
  | empty | run just started, nothing written | [17-2](https://www.figma.com/design/q3tTcpi60BOCn0VIIiX4wg/Builder-Pro-AI-Workflows?node-id=17-2) | honest "no hand-off written yet" (depicted inline) |
  | error | a boundary crossed with no entry | [17-2](https://www.figma.com/design/q3tTcpi60BOCn0VIIiX4wg/Builder-Pro-AI-Workflows?node-id=17-2) | "no hand-off written — never proceed blind" (depicted inline) |
- **Coverage:** none yet
- **Scenarios:** SCN-066
- **Resources:** the journal is a file the agents read/write — reuses the file-backed Doc machinery (SCN-054); the CEO's decision log links each entry it acted on
- **Status:** designed
