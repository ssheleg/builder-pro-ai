# Builder Pro AI — Vision-Alignment Docs Pass (Cycle 3) — Design

**Date:** 2026-07-06
**Status:** approved direction (day-long brainstorm: vision v2→v4 + audit + owner confirmations)
**Inputs (all committed):**
- Vision v2+v3+v4 + pain + home screen: [`research/2026-07-06-product-vision-v2.md`](../research/2026-07-06-product-vision-v2.md)
- Audit (36 findings V1–V36, 6 lenses, adversarially merged) + correction plan (25 items) + v3/v4
  addenda + 4 step-by-step UX scenarios: [`research/2026-07-06-vision-v2-correction-plan.md`](../research/2026-07-06-vision-v2-correction-plan.md)
- Machine-readable audit: [`research/2026-07-06-vision-v2-audit.json`](../research/2026-07-06-vision-v2-audit.json)
- Design system (landed 2026-07-06): [`docs/design-system.md`](../../design-system.md)

**The correction-plan document is the authoritative per-item content source** (where / why /
from→to / connects, per item). This spec LOCKS the decisions, the file map, the ordering, and the
DoD; the implementation plan will embed exact edit content per task, as in Cycle 1.

## 0. Owner decisions (locked)

| # | Decision | Choice |
|---|----------|--------|
| DA1 | **ADR-HOST** (unattended execution + app-domain store host) | **(a) Headless `bpa-orchd`** — a second launchd-managed daemon (mirror of the proven `bpa-sessiond` pattern) owning the app-domain store, scheduler, workflow-engine runtime, and agent runtime; the GUI is its client. sessiond's charter is UNCHANGED (terminal domain only). Upgrade/consent choreography reuses D4/Pv2 patterns. Accepted by owner («давай» on the recommendation, 2026-07-06). |
| DA2 | Roadmap re-cut | Approved as audited: S-IDEA (ideas+research) ships BEFORE the agent org; SW1 (workflow engine) sits between S6a and S6b; S-EXT (MCP+connectors+skills/plugins) runs parallel to S4/S5; S6b executes VIA the engine. |
| DA3 | Scope of this cycle | **Docs-only pass** (overview, charter, §16, Pv2 amendments, backlog, CONTRIBUTING, architecture echo). No product code. Per-slice specs (S3, S-EXT, SW1…) are separate future cycles. |
| DA4 | Pv2 | Receives its 3 amendments in this pass, then unfreezes for owner review → implementation cycle. |

## 0.1 Open decisions — recorded with DEFAULTS (owner may override at spec review or any time)

These 13 register questions (Q1→DA1, Q4→DA2 above) are slice-level; this pass records each in its owning roadmap row as
`Open decision — default: …` so no future spec starts ambiguous. Defaults:

| Q | Topic | Default locked into the docs |
|---|---|---|
| Q2 | Approval gates vs intervention budget | Hard by default: task-breakdown approve + spec approve + deploy gate. Deploy auto-passes for **low-risk** diffs = no DB migrations AND diff < 300 LOC AND changed paths within the project's configured allowlist — deterministic rules, no LLM classifier in v1. Per-project policy can loosen/tighten. |
| Q3 | Away-from-Mac escalation | v0.x: macOS notification only; the inbox answers on the Mac. Remote answering (Telegram/mobile) = named backlog item, not designed now. |
| Q5 | prowl session scope | One session_id per **research-run** (clean isolation; cache loss accepted). |
| Q6 | MCP v1 surface | Tools + auth only. Sampling **disabled by default** (remote server spending owner's LLM keys). Resources/prompts → backlog. |
| Q7 | Self-heal prerequisites | Enabling the self-heal workflow REQUIRES a tested rollback recipe in the project's deploy config — refused (with an actionable message) at enable-time otherwise. Run-budget breach ⇒ **pause as gated escalation**, never silent fail. |
| Q8 | Idea pipeline degradation | prowl down/unauthorized ⇒ inline «сформировать задачу без ресёрча» with an honest-degradation note recorded on the artifact chain. |
| Q9 | Prioritization semantics | Per-project stack rank (plan + bug tasks mixed) + a panel-level cross-project rank. Owner-set ranks HARD-override agent-computed priorities; agent ranks are suggestions with visible reasoning. |
| Q10 | Insight fit-verdict authority | Agent computes the verdict + reasoning; owner has one-click override; NO auto-accept into the backlog (auto-archive of clear non-fits is allowed, reasoning kept). |
| Q11 | Experiments | v1: Tasks tagged `experiment` (hypothesis/metric/verdict in the task body). First-class Experiment entity deferred to S8 maturity. |
| Q12 | Main-metric declaration | Goal-attached: each goal names the metric(s) it moves; the project settings page is the editor surface. MetricDefinition is owner-mutable. |
| Q13 | Rules format | BOTH layers: markdown rules (LLM-read, injected into agent context — Claude-Code-style global + per-project) AND a typed policy table (machine-enforced gates: spend caps, approval classes, path allowlists). |
| Q14 | Skills format | Adopt the Claude Code SKILL.md format for portability (owner's existing skills carry over). |
| Q15 | Run-log retention | StepRun payloads stored FULL for 14 days, then thinned to metadata (request/response summaries + sizes + hashes); 50 MB per-run payload cap with honest truncation markers. |

## 1. Goals / non-goals

**Goals.**
1. The overview states the v2–v4 mission and roadmap: every vision concept has an owned slice,
   correct dependencies, per-slice DoD + metric + open-decision rows (kills V1–V36 at the
   roadmap/contract level).
2. The two-daemon topology (ADR-HOST) is a recorded, locked ADR before any S3+ spec starts.
3. The Data-layer charter carries the full entity map (14 families), the additive-only schema
   law, the global storage architecture map, migration + soft-reference + telemetry policies.
4. §16 covers the MCP/extension trust boundary; credentials posture generalized.
5. Pv2 amended (3 items) and unfrozen.
6. Process laws (meta-process; design-section rule) land in CONTRIBUTING/overview §6.

**Non-goals.** No product code. No per-slice specs (S3/S-EXT/SW1 et al. are next cycles). No UI
work (design-system.md already landed). No new audit.

## 2. Deliverables (file-ownership map)

| # | File | Action | Correction-plan items |
|---|------|--------|--------|
| F1 | `docs/superpowers/specs/2026-07-01-builderpro-platform-overview.md` §1 | Mission rewrite: v2 mission sentence, the pain paragraph, 3 north-star metrics, home-screen promise (6+1 elements incl. Insights lane) | item 1; V30 |
| F2 | same, §2 | **ADR-HOST recorded** (DA1, full rationale + rejected alternatives); two locks: «multi-project from day one», «additive-only schema evolution»; survival table gains `workflow/agent runs — hosted by bpa-orchd` row | items 2-3; V2 |
| F3 | same, Data-layer charter | Full entity map with owning slices + stores (Project, Workspace, Goal hierarchy, Idea, Insight, Task/Subtask, ResearchArtifact, WorkflowDefinition/Run/StepRun, Schedule/Trigger, RuleSet, MCP/Connector registry, MetricDefinition/MetricPoint, ErrorGroup/StudyItem, Deploy record); app-store migration policy (user_version, fail-closed, forward-only, **additive-only law**); soft cross-store references rule; ingested-telemetry data class (retention/redaction); **global storage architecture** section (orchd store · sessiond bpa.db · run/step logs · artifact blobs — one map); Q15 retention default | items 4-7; V14-V16, V26; v3/v4 tables |
| F4 | same, §3 roadmap table + diagram | Full reshape: S3 expanded (Projects+Goals+Ideas+Tasks/Subtasks+RuleSet), S4 amended (read+write graph, workspace-wide agent retrieval = DoD), **S-EXT** (MCP client + connectors + skills/plugins, Claude-Code format), **S-IDEA/INSIGHT**, S5 reframed (kanban = view), S6a + tool-calling, **SW1/SW2/SW3**, S6b-e re-scoped, **S9a/S9b**, S7 extended (run observability), S8 re-scoped (MetricDefinition + metrics→sprint), **SH** mission control; each row: purpose, deps, DoD, metric, `Open decision — default` entries from §0.1 | items 11, 15-19; V4-V13, V17-V29; v3/v4 |
| F5 | same, §4 | Rewrite: CEO→PM→eng loop = the DEFAULT WorkflowDefinition executed by SW1 (not code); built-in definition #2 = goal-driven research/refresh (channels example); step kinds incl. data-fetch/process/insight-extract; run-observability contract sentence | item 12; V1, V6; v3/v4 |
| F6 | same, §5 + §6 | §5: human steps += workflow/schedule/rules authoring; §6: the **meta-process law** (end goal → live plan → architecture-first → minimum → constructor) applied to the platform AND to managed projects | items 13, 25; V36; v4 §8 |
| F7 | `docs/superpowers/specs/2026-07-01-builderpro-s0s1-foundation-terminal-design.md` §16 | «MCP & extension boundary» subsection (egress core/orchd-only; per-server consent; tool-result prompt-injection posture; stdio-server execution consent; spend caps via trust layer; Q6 defaults); credentials posture generalized (Keychain for ALL provider/connector/MCP secrets); trust-layer cross-references | items 8-10; V12, V13, V24 |
| F8 | `docs/superpowers/specs/2026-07-06-protocol-v2-design.md` | 3 amendments: named Pv2.1 additive batch (command+argv spawn, typed exit-wait, ReadOutput{since_seq}, text snapshot — reserved, not built now); command_events attribution field decision (which actor/workflow-run a command belongs to — schema hook now); frozen-preamble note referencing orchd as a future second client | item 14; V31 |
| F9 | `docs/backlog.md` | New rows: MCP prompt-injection/consent hardening (P1), remote escalation channel (P3), telemetry retention/redaction impl (P2), Experiment entity (P3), payload-thinning job (P2); update BL-4/BL-7 cross-refs to orchd naming | items across; V13, Q3, Q11, Q15 |
| F10 | `CONTRIBUTING.md` | Meta-process law section; design-section rule (from design-system.md §8) added to the DoD checklist | item 25; v4 §8 |
| F11 | `docs/architecture.md` | Two-daemon topology echo (orchd alongside sessiond, one diagram block + charter pointer); design-system.md added to related docs | item 2 echo |
| F12 | `docs/traceability.md` | Refresh rows that reference overview section names changed by F1–F6 (mechanical sweep) | item «process» |

Parallel-safety: one owner task per file; F1–F6 are ONE file (overview) → sequential within one
task-group or a single large task, decided in the plan.

## 3. Content authority & style

- Per-item normative content = the correction plan (items 1–25) + v3/v4 addendum tables +
  §0/§0.1 of THIS spec. The implementation plan embeds exact edit text per task (Cycle-1 style,
  grep-verifiable DoD per task).
- Every amended section carries a dated amendment marker, as established in Cycle 1.
- Wording follows the honesty register locked in Cycle 1 (no aspirational claims stated as fact;
  unbuilt = «roadmap/planned», defaults = «Open decision — default»).

## 4. Traceability

- Every audit finding V1–V36 maps to ≥1 deliverable F1–F12 (matrix embedded in the plan; final
  review checks it). V32–V35 (reusable) become explicit «builds on» lines in F4 rows.
- Every §0.1 default appears verbatim in its owning F4 row or F7 subsection.

## 5. Execution shape

- Worktree branch `worktree-vision-alignment` off `main`; subagent-driven or inline (owner picks
  at handoff); ~10 tasks (overview F1–F6 split into 3–4 tasks; F7; F8; F9+F10; F11+F12).
- Gates: `bash scripts/final-suite.sh` stays green (docs-only must not break it); markdown link
  check; banned-phrase sweep (no stale pre-v2 mission claims left: grep «director of AI
  development teams» framing residue in §1).
- Final whole-branch review (truth-vs-decisions, cross-doc consistency, correction-plan
  compliance) → PR → live CI green → merge (Cycle-1/2 pattern).

## 6. Definition of Done (cycle)

1. A zero-context implementer reading ONLY the overview would build the v2–v4 product (mission,
   slices, dependencies, decisions all present).
2. V1–V36 traceable to landed edits; §0.1 defaults all recorded in owning rows.
3. ADR-HOST present in §2 with rationale; charter's entity map complete; §16 MCP boundary present.
4. Pv2 amended + status flips to «unfrozen — awaiting owner review».
5. CI green on the PR; merged to main.

## 7. Human steps

**None.** (Owner review of THIS spec is the only gate before writing-plans.)

## 8. What unlocks after this cycle

Pv2 review → Pv2 implementation cycle (protocol v2 + orchd-aware preamble note). Then S3 spec
(first slice built on the corrected map: Projects+Goals+Ideas+Tasks+RuleSet in the orchd store).
