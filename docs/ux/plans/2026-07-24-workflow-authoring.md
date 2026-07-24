# UX Plan — Workflow authoring (SW1) — 2026-07-24

- **Sources:** foundation ST-045/046 + JTBD-12 + A-10/A-11; flows FLW-23 (author) / FLW-24 (run); scenarios SCN-060..066; screens SCR-01..07 with Figma frames ([file](https://www.figma.com/design/q3tTcpi60BOCn0VIIiX4wg))
- **Goal:** the operator can **author and save a reusable workflow-as-data** — ordered stages, each with an agent, prompt, skills, gate, context scope and outputs; global skills; CEO oversight config — scoped global/project, file-backed, surviving a restart. Runnable across projects.

## Scope

**In (buildable now — no runtime):** the definition + its full authoring surface + the run *trigger* with an honest pending state. This is exactly the SCN-046 boundary: config ships, execution waits.

**Out (S6b runtime — designed, not built here):** the executor that spawns terminals per agent block and runs stages; the CEO advancing/escalating between stages; agents writing/reading the run journal; the live SCR-05 run detail, SCR-06 Home digest, SCR-07 run journal. These are captured in Figma + scenarios (SCN-063/064/066) and gated on A-10/A-11. Everything the authoring stores here is the config the runtime will later consume.

## Locked contract (build to these names)

Rust (`crates/orchd-proto`, `#[serde(rename_all="camelCase")]`, `TS`, `export_to="orchd-types.ts"` — mirror `Skill`):
- `WorkflowScope = Global | Project`   ·   `Gate = Auto | Manual`   ·   `ContextScope = Inherit | Handoff | Project | Selected`
- `Stage { id, name, prompt, skill_ids: Vec<String>, agent: Option<String> (None = inherit the workflow default), context_scope: ContextScope, outputs: Vec<String>, gate: Gate }` — order is the Vec index
- `Workflow { id, name, description, scope: WorkflowScope, project_id: Option<String>, default_agent: String, stages: Vec<Stage>, global_skill_ids: Vec<String>, supervisor: SupervisorConfig, file_state, json_path, hash, created_at, updated_at }`
- Verbs (tail-append, wire stays `[1,1]`, add verb-name arm): `WorkflowList { scope: Option<WorkflowScope>, project_id: Option<String> }`, `WorkflowGet { id }`, `WorkflowUpsert { id (empty ⇒ create), name, description, scope, project_id, default_agent, stages, global_skill_ids, supervisor } → Workflow`, `WorkflowDelete { id } → Ack`; `Push::WorkflowsChanged` (bare invalidation).
- Known agents = the ones the app launches (claude-code / hermes / opencode / kilo); `agent` values validated against that set (or `None`). `default_agent` required.

Persistence (`crates/orchd`, mirror Doc/RuleSet files-as-truth): a `workflow` table (schema-version bump) + the full definition serialized to a JSON file under the app-support rules tree (`rules/workflows/<scope|project>/<id>.json`), hash + `file_state` for external-change honesty, path built through the same validated choke-point Docs use (no traversal from JS names).

TS (generated `src/ipc/orchd-types.ts`) consumed by the frontend; do NOT hand-edit — regenerate via the ts_export test.

## Target interface

### Screen: Workflows library (SCR-01, FLW-23) — [frame 3-7](https://www.figma.com/design/q3tTcpi60BOCn0VIIiX4wg/Builder-Pro-AI-Workflows?node-id=3-7)
- **Purpose:** list + start workflows. **Elements:** ⚙ Workflows top-nav, title, scope pill (All|Global|Project), "+ New workflow" *(primary)*, per-row (name, description, stage-count, scope, skills-count, Run → / Open / Duplicate / Delete). **States:** empty ("No workflows yet — compose one to reuse across projects."), success. **Behavior:** scoped like Skills/RuleSets; delete confirms.

### Screen: Workflow editor (SCR-02, FLW-23) — [frame 3-11](https://www.figma.com/design/q3tTcpi60BOCn0VIIiX4wg/Builder-Pro-AI-Workflows?node-id=3-11)
- **Purpose:** author stages + globals + CEO. **Elements:** breadcrumb, **default-agent picker**, "Save workflow" *(primary)*, stage list **grouped into terminal brackets** (Terminal N · agent · stage-count; a boundary line where the agent changes — derived from each stage's effective agent), reorderable rows, "+ Add stage", global-skills picker, CEO oversight panel (reuses RulesetPanel `SupervisorConfig`; S6b pending note). **States:** error (invalid stage flagged, Save blocked), success. **Behavior:** effective skills = global ∪ stage; the terminal grouping is a pure view over the per-stage agent.

### Screen: Stage detail (SCR-03, FLW-23) — [frame 3-15](https://www.figma.com/design/q3tTcpi60BOCn0VIIiX4wg/Builder-Pro-AI-Workflows?node-id=3-15)
- **Purpose:** one stage. **Elements:** prompt/command editor, stage-skills picker, effective-skills summary, gate segmented (auto|manual), **Agent & context panel** — agent picker (inherit + known agents), context-scope segmented (inherit|handoff|project|selected + subset picker for `selected`), outputs field; missing-binding + agent-unavailable markers. **States:** error, success. **Behavior:** context default = inherit within a terminal, handoff at a boundary.

### Screen: Run workflow picker (SCR-04, FLW-24) — [frame 3-19](https://www.figma.com/design/q3tTcpi60BOCn0VIIiX4wg/Builder-Pro-AI-Workflows?node-id=3-19)
- **Purpose:** the trigger. **Elements:** modal, "Run workflow on {project}", radio rows (name, stages · CEO on/off), "Run workflow" *(primary)*, Cancel, **S6b pending note** ("Workflows run once the orchestrator agent runtime lands (S6b). Authoring, saving and this trigger are live now — the run does not fake execution."). **States:** success. **Behavior:** this build shows the pending note and does NOT spawn a run (no runtime); it never fabricates execution.

## Changes

| # | Action | Object | Details | Traces | Priority |
|---|--------|--------|---------|--------|----------|
| 1 | CREATE | crates/orchd-proto Workflow types + verbs + Push | Workflow/Stage/Gate/WorkflowScope/ContextScope + WorkflowList/Get/Upsert/Delete + WorkflowsChanged, tail-appended; roundtrip + ts_export tests | SCN-060/061/065, contract | P1 |
| 2 | CREATE | crates/orchd persistence | `workflow` table + JSON file (files-as-truth) + validation (name-rule; scope=project⇒project_id; every stage has name+prompt; no empty skill/output ids; enabled CEO ⇒ ≥1 delegated class, reuse validate_policy; agent ∈ known∪None); external-change file_state; CRUD tests + $HOME isolation | SCN-060/061/062/065, ST-045 | P1 |
| 3 | CREATE | crates/orchd socket_server dispatch | WorkflowList/Get/Upsert/Delete arms + emit WorkflowsChanged; build_workflow_view; not-found = same error shape as RuleSet; dispatch tests | SCN-060, contract | P1 |
| 4 | CREATE | src-tauri/src/commands.rs + lib.rs | workflow_list/get/upsert/delete commands (mirror ruleset commands) + registration; broker.rs map WorkflowsChanged → `orchd://workflows-changed` | SCN-060, contract | P1 |
| 5 | CREATE | src/ipc (orchd.ts + events.ts) | orchdListWorkflows/Get/Upsert/Delete wrappers + describeOrchdError; onOrchdWorkflowsChanged; consume generated Workflow types | SCN-060, contract | P1 |
| 6 | CREATE | src/store/store.ts workflows slice | `workflows: Workflow[]` + `refreshWorkflows` (invalidation-driven) + upsert/delete actions; subscribe to workflows-changed in App | SCN-060, ST-045 | P1 |
| 7 | CREATE | src/components/workflows/WorkflowsView.tsx (+test) | SCR-01 library: scope filter, rows, new/open/duplicate/delete + confirm, empty state | SCN-060, SCR-01 | P1 |
| 8 | CREATE | src/components/workflows/WorkflowEditor.tsx (+test) | SCR-02: default-agent picker, stage list with terminal grouping (derive from agents), reorder, global skills, CEO section (reuse RulesetPanel supervisor pattern), Save + validation | SCN-061/062/065, SCR-02 | P1 |
| 9 | CREATE | src/components/workflows/StageDetail.tsx (+test) | SCR-03: prompt editor, skill picker, gate, agent picker, context-scope + subset, outputs; missing-binding + agent-unavailable markers | SCN-061/065, SCR-03 | P1 |
| 10 | CREATE | src/components/workflows/RunWorkflowPicker.tsx (+test) | SCR-04: modal, pick global workflow, Run (S6b pending note — no execution), Cancel; project "Run workflow" entry | SCN-063 (trigger only), SCR-04 | P2 |
| 11 | MODIFY | src/App.tsx | add `"workflows"` to the view union + branch `view==="workflows" ? <WorkflowsView/>`; workspace-aware routing to editor | SCN-060, SCR-01 | P1 |
| 12 | MODIFY | src/components/WorkspaceSidebar.tsx | add "⚙ Workflows" nav button (mirror stats-nav-button) → setView("workflows"); update SCN-007 nav in the same change | SCN-060/007, SCR-01 | P1 |
| 13 | MODIFY | src/strings.ts | `strings.workflows.*` — all copy (library, editor, stage, agents, context scopes, gate, CEO reuse, run pending note) | SCN-060..065 | P1 |
| 14 | MODIFY | docs/ux scenarios + screens | flip SCN-060/061/062/065 draft→validated as they build; SCR-01/02/03 designed→built with Coverage cites; re-run lint | same-change rule | P1 |

## Execution order

Contracts first (sequential): **1 → 2 → 3** (Rust core; run `cargo test -p bpa-orchd-proto -p bpa-orchd`, clippy `-D warnings`, `cargo fmt --check`; regenerate orchd-types.ts). Then **4 → 5 → 6** (bridge + IPC + store). Then the UI in parallel by file ownership: **7**, **9**, then **8** (editor consumes StageDetail), then **10**; glue **11/12/13** between. **14** rides each UI task (same-change). P2 = RunPicker last. No two parallel tasks write the same file.

## Definition of done

- Every change lands with its scenario updated in the same change; `python3 docs/ux/lint.py` green.
- Gates: `cargo test -p bpa-orchd-proto -p bpa-orchd` + `npx vitest run` + `npx tsc --noEmit` + `cargo clippy --workspace --all-targets -- -D warnings` + `cargo fmt --check` + `scripts/check-english.sh` all clean. (Known-unrelated BL-102 `connect_*` failures — a live daemon on the dev box — are the ONLY tolerated failures.)
- A saved workflow round-trips through a restart; a workflow with an agent change shows the terminal grouping; a stage with a missing skill or unavailable agent blocks running honestly; the run trigger shows the S6b pending note and fabricates no execution.
- Post-build `/ux-audit feature:workflows` → PASS on SCN-060/061/062/065 (authoring); SCN-063/064/066 stay BLOCKED-by-design (S6b), verified as honest (no phantom run).

## What you have now

This plan, the scenario chain (SCN-060..066), the flows (FLW-23/24), the screens map (SCR-01..07) and **seven built Figma frames** on the Soft Control Room tokens. The authoring slice is fully specified and buildable; the run slice is fully designed and gated on S6b.

## Recommended: continue with task-pipeline

```
/task-pipeline docs/ux/plans/2026-07-24-workflow-authoring.md
```

Contracts (task 1) are locked here so parallel subagents can build against them; keep the same-change rule (each change updates its scenario + screens.md), and re-run `/ux-audit feature:workflows` after to confirm PASS on the authoring scenarios.
