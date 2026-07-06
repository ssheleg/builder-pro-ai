# Vision-Alignment Docs Pass (Cycle 3) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Rewrite the docs so a zero-context implementer would build the vision v2–v4 product — mission, the two-daemon (`bpa-orchd`) topology, the full entity map, the reshaped roadmap (ideas → research → workflow engine → agents → prod self-heal → mission control), the MCP/extension trust boundary, and the meta-process — with all 13 open-decision defaults recorded.

**Architecture:** Pure docs pass on a worktree off `main`. No product code. Authoritative content sources: the spec [`2026-07-06-vision-alignment-docs-pass-design.md`](../specs/2026-07-06-vision-alignment-docs-pass-design.md) (§0 decisions, §0.1 defaults, §2 file map) and the correction plan [`2026-07-06-vision-v2-correction-plan.md`](../research/2026-07-06-vision-v2-correction-plan.md) (per-item where/why/from→to/connects + v3/v4 tables + 4 UX scenarios). Implementers READ those two for full content; this plan gives exact locations, the locked strings, and grep-verifiable DoD.

**Tech Stack:** Markdown only. Verification: `grep`, a markdown link check, and `bash scripts/final-suite.sh` (must stay green — docs must not break it).

## Global Constraints

- Branch: `worktree-vision-alignment` off `main` (current tip). Worktree via superpowers:using-git-worktrees.
- `cargo` at `$HOME/.cargo/bin` — `export PATH="$HOME/.cargo/bin:$PATH"` for the final-suite check only.
- **No product code, no code-config changes.** Only the 12 doc files in the file map may change.
- Conventional commits; EVERY commit trailer: `Co-Authored-By: Claude Opus 4.8 <noreply@anthropic.com>`.
- Commit messages cite the correction-plan items / audit finding ids they close.
- Line numbers captured at branch base; ALWAYS re-locate by grepping quoted text before editing.
- **Locked strings (spelled exactly, cross-referenced across files):**
  - Second daemon binary: `bpa-orchd` (never "orchd" alone in prose without the binary name once).
  - Locked headings to CREATE: `## Data-layer charter` already exists — EXTEND it; new overview sections keep existing heading text. New spec §16 subsection heading: `### MCP & extension boundary`.
  - Roadmap slice ids (exact tokens): `S-EXT`, `S-IDEA`, `SW1`, `SW2`, `SW3`, `S9a`, `S9b`, `SH`.
  - Open-decision marker in roadmap rows: `Open decision — default: …` (em dash, exact).
  - Amendment marker on every edited section: `(amended 2026-07-06, vision v2–v4)`.
  - Mission sentence (F1): `take any idea to a working project and manage/organize the vibecoding process`.
- **Content-source rule:** where this plan says "per correction-plan item N" or "per spec §0.1 Qn", the implementer copies that content into the doc in full prose — NOT a pointer. The two source docs are the script; the target docs must stand alone.
- Link-check helper (run from repo root, expect empty output):
  ```bash
  check_links() { f="$1"; d=$(dirname "$f"); grep -oE '\]\(([.A-Za-z0-9_/-]+\.(md|sh|toml|yml|json))' "$f" | sed 's/](//' | while read -r p; do [ -e "$d/$p" ] || [ -e "$p" ] || echo "BROKEN in $f: $p"; done; }
  ```

---

### Task 1: Overview §1 — mission rewrite (F1)

**Files:**
- Modify: `docs/superpowers/specs/2026-07-01-builderpro-platform-overview.md` (§1 "## 1. Product", lines ~10-28)

**Interfaces:**
- Produces: the v2 mission framing every later task and slice references; the north-star metrics quoted by F4 rows and the DoD.

- [ ] **Step 1: Replace the §1 opening paragraph.** Locate `turns the user into a **director of AI development
teams**` and replace the whole first paragraph with a v2–v4 mission statement built from correction-plan item 1 + vision-v2 "Mission" + "The pain this product exists to kill" + "The home screen". It MUST contain, in prose:
  - the mission sentence (Global Constraints locked string): the product's job is to **take any idea to a working project and manage/organize the vibecoding process**;
  - the pain: 5-6 parallel projects, different speeds/stages, lost goals+context, the owner reduced to polling and button-pressing — **the attention tax is the enemy**;
  - the home-screen promise: open the app → in <30s see, for all projects, where each is / what moved / what needs the owner; a live agent feed, per-project progress, idea quick-capture, agent-org settings, vector steering, hot-questions inbox, and an Insights lane.
- [ ] **Step 2: Add the three north-star metrics** as a labeled list under the mission (verbatim intent from vision-v2 "product law"): (a) time-to-context on app open < 30s for all projects; (b) owner interventions per shipped task → 1; (c) hours agents progress unattended without stalling on a button.
- [ ] **Step 3: Keep the four design tenets** but update "director of AI development teams" residue: the tenets list stays; adjust any wording that still frames the product as only an agent-orchestration shell. Add a fifth tenet: **Additive, constructor-style growth — the system extends cube-by-cube; the schema and architecture grow by addition, never rebuild** (vision-v4 §7/§8).
- [ ] **Step 4: Mark the amendment.** Add `> (amended 2026-07-06, vision v2–v4 — see docs/superpowers/research/2026-07-06-product-vision-v2.md)` under the `## 1. Product` heading.
- [ ] **Step 5: Verify**
```bash
cd docs/superpowers/specs
grep -c "take any idea to a working project" 2026-07-01-builderpro-platform-overview.md   # >=1
grep -c "attention tax" 2026-07-01-builderpro-platform-overview.md                          # >=1
grep -c "director of AI development teams" 2026-07-01-builderpro-platform-overview.md       # 0
grep -c "time-to-context" 2026-07-01-builderpro-platform-overview.md                        # >=1
```
- [ ] **Step 6: Commit**
```bash
git add docs/superpowers/specs/2026-07-01-builderpro-platform-overview.md
git commit -m "docs(overview): §1 mission rewrite — any idea→working project, attention-tax pain, north-star metrics (V30; item 1)"
```

---

### Task 2: Overview §2 — ADR-HOST + locks + survival row (F2)

**Files:**
- Modify: `docs/superpowers/specs/2026-07-01-builderpro-platform-overview.md` (§2 "## 2. Global locked decisions" + its survival truth table)

**Interfaces:**
- Consumes: nothing. Produces: the `bpa-orchd` topology + additive-schema + multi-project locks that F3/F4/F5 and every future slice depend on.

- [ ] **Step 1: Add `bpa-orchd` to the locked-decisions table.** After the existing rows, add a row: | Unattended execution + app-domain store host | **A second launchd-managed daemon `bpa-orchd`** (mirror of the `bpa-sessiond` pattern) owns the app-domain store, the scheduler, the workflow-engine runtime, and the agent runtime; the GUI is its client. `bpa-sessiond`'s charter is unchanged (terminal domain only). |
- [ ] **Step 2: Add a full ADR block** below the table titled `### ADR-HOST — the two-daemon topology (locked 2026-07-06)`. Content from correction-plan item 2 + audit V2: the problem (agents in GUI die on window close; charter bars sessiond from domain logic; nothing runs the 24h loop unattended); the decision (`bpa-orchd`); rejected alternatives ((b) widen sessiond — riskiest for the hardened daemon; (c) app-open-only degradation — fails the attention-tax mission); consequences (orchd reuses the launchd + Pv2 drain/consent upgrade patterns; GUI and orchd co-attach terminal sessions via Pv2 multi-subscriber attach; Keychain access under a locked screen is an open sub-question routed to the S-EXT/security spec).
- [ ] **Step 2b: Add two one-line locks** under §2: `**Multi-project from day one:** every S2+ data model, store, and event stream is multi-project; UI panels are per-project VIEWS over multi-project data.` and `**Additive-only schema evolution:** entities grow by adding tables/columns; migrations are append-only by policy — never a rebuild (vision-v4 §7).`
- [ ] **Step 3: Update the survival truth table.** Locate the `| Agent runs (S6+) | **undefined until S6b**` row and replace with `| Workflow / agent runs | hosted by \`bpa-orchd\` (launchd-supervised, survives GUI close); a run's step state persists in the app-domain store and resumes/catches-up on orchd restart (catch-up policy per SW2) |`.
- [ ] **Step 4: Verify**
```bash
grep -c "bpa-orchd" docs/superpowers/specs/2026-07-01-builderpro-platform-overview.md          # >=3
grep -c "ADR-HOST" docs/superpowers/specs/2026-07-01-builderpro-platform-overview.md            # >=1
grep -c "Additive-only schema evolution" docs/superpowers/specs/2026-07-01-builderpro-platform-overview.md  # >=1
grep -c "undefined until S6b" docs/superpowers/specs/2026-07-01-builderpro-platform-overview.md  # 0
```
- [ ] **Step 5: Commit**
```bash
git add docs/superpowers/specs/2026-07-01-builderpro-platform-overview.md
git commit -m "docs(overview): §2 ADR-HOST (bpa-orchd two-daemon), multi-project + additive-schema locks, survival row (V2; item 2)"
```

---

### Task 3: Overview Data-layer charter — full entity map + storage architecture (F3)

**Files:**
- Modify: `docs/superpowers/specs/2026-07-01-builderpro-platform-overview.md` (`## Data-layer charter` section, ~line 163)

**Interfaces:**
- Consumes: `bpa-orchd` (Task 2). Produces: the 14-entity map + store assignments + policies every future per-slice spec builds on.

- [ ] **Step 1: Add the entity map** as a table under the charter: `Entity | Store | Owning slice`. The 14 families (from correction-plan items 4-7 + v3/v4 tables): Project, Workspace (both: Project in orchd store, Workspace root paths bridged to sessiond); Goal (hierarchy: 1 strategic + N additional); Idea; Insight; Task/Subtask; ResearchArtifact; WorkflowDefinition; WorkflowRun/StepRun; Schedule/Trigger; RuleSet (global + per-project); MCP/Connector/Skill registry; MetricDefinition + MetricPoint; ErrorGroup/StudyItem; Deploy record. Store column = `orchd` (app-domain) for all except sessions/scrollback/command_events which stay `sessiond bpa.db`.
- [ ] **Step 2: Add the storage-architecture map** subsection `### Global storage architecture`: four stores, one map — (1) `bpa-sessiond` `bpa.db` (terminal domain: sessions, scrollback, command_events — unchanged); (2) `bpa-orchd` app-domain store (all 14 entity families above); (3) run/step logs (StepRun I/O — see retention below); (4) artifact blobs (research artifacts, large payloads). State which physical engine each uses is decided in that store's slice spec, but the OWNERSHIP boundary is locked here.
- [ ] **Step 3: Add the three policies** (correction-plan items 5-7):
  - `**App-store migration policy:** the orchd store ships schema v1 with its own \`user_version\`, fail-closed forward-only single-transaction migrations (mirrors sessiond persistence). Additive-only (§2 lock): new tables/columns only.`
  - `**Cross-store references are SOFT:** references across stores are UUID strings with no FK integrity assumption; the daemon's retention (BL-4) must not be assumed to preserve rows another store points at — orchd keeps its own copy of anything it must not lose.`
  - `**Ingested telemetry is a distinct data class:** prod logs/errors and metrics may carry PII/secrets; store home + redaction + retention decided in the S9a spec; default retention per §0.1 Q15 (StepRun payloads full 14 days → thinned to metadata, 50MB/run cap).`
- [ ] **Step 4: Amendment marker** `(amended 2026-07-06, vision v2–v4)` on the charter heading.
- [ ] **Step 5: Verify**
```bash
grep -c "Global storage architecture" docs/superpowers/specs/2026-07-01-builderpro-platform-overview.md  # 1
grep -c "Cross-store references are SOFT" docs/superpowers/specs/2026-07-01-builderpro-platform-overview.md  # 1
for e in Idea Insight WorkflowDefinition StepRun RuleSet MetricDefinition ErrorGroup ResearchArtifact; do grep -q "$e" docs/superpowers/specs/2026-07-01-builderpro-platform-overview.md || echo "MISSING entity: $e"; done
```
- [ ] **Step 6: Commit**
```bash
git add docs/superpowers/specs/2026-07-01-builderpro-platform-overview.md
git commit -m "docs(overview): Data-layer charter — 14-entity map, storage architecture, migration/soft-ref/telemetry policies (V14-V16,V26; items 4-7)"
```

---

### Task 4: Overview §3 — roadmap re-cut (F4)

**Files:**
- Modify: `docs/superpowers/specs/2026-07-01-builderpro-platform-overview.md` (§3 "## 3. Subsystem decomposition & dependency order" — slice table + ASCII diagram, ~lines 84-121)

**Interfaces:**
- Consumes: charter entities (Task 3), `bpa-orchd` (Task 2). Produces: the slice list + dependency graph every future spec cycle picks work from.

- [ ] **Step 1: Replace the slice table** with the revised roadmap from the correction plan's "Revised roadmap" section. Rows (each: Slice | Depends on | Purpose · DoD · north-star metric · `Open decision — default`): S0/S1/Pv2 (done/in-flight); S2 (workspace/files, unchanged); S3 EXPANDED (Projects+Goals-hierarchy+Ideas+Tasks/Subtasks+RuleSet); S4 AMENDED (knowledge graph read+WRITE, **workspace-wide cross-project agent retrieval = DoD**); **S-EXT** (MCP client + connectors + skills/plugins, Claude-Code format, global+local settings; deps S3+BL-20; Open decision Q6 default: tools+auth only, sampling off; Q14 default: SKILL.md format); **S-IDEA** (idea+research pipeline, ships BEFORE agent org; deps S3+S-EXT+S4; Open decision Q5 default per-run session, Q8 default degrade-inline, Q10 default agent-verdict+owner-override); S5 REFRAMED (kanban = view over Task); S6a AMENDED (LLM provider + tool-calling); **SW1** (workflow engine core, between S6a and S6b; step kinds incl. data-fetch/process/insight-extract + run-observability contract); S6b RE-SCOPED (agent runtime runs VIA SW1, the CEO→PM→eng loop = default WorkflowDefinition); S6c WIDENED (one generic approval inbox: escalations + workflow gates + policy gates; Open decision Q2 default low-risk auto-deploy rule); **SW2** (scheduler + event triggers + domain event bus); **SW3** (visual workflow editor); S6d/S6e (workers/custom-agents, largely unchanged); **S9a** (prod telemetry ingestion + study folder; Open decision Q7 default rollback-required-to-enable); **S9b** (deploy step + verification); S7 EXTENDED (observability incl. workflow-run health); S8 RE-SCOPED (MetricDefinition + metrics→sprint; Open decision Q11 default experiment-as-tag, Q12 default goal-attached metrics); **SH** (mission-control home, capstone). Each row's purpose/DoD text comes from the correction plan's revisedRoadmap entries — copy in full.
- [ ] **Step 2: Replace the ASCII dependency diagram** to match: `S0→S1→Pv2`; `ADR-HOST` gates the orchd-hosted slices; `S3` feeds S4/S5/S-EXT/S-IDEA; `S-IDEA` deps S3+S-EXT+S4; `S6a→SW1→S6b→S6c`; `SW2,SW3` after SW1; `S9a→S9b`; `S8` deps S3+SW1/SW2+S9a; `SH` capstone deps S6c+S7+SW1-run-state+S-IDEA+S9a/S9b. Ensure the diagram and the table agree (the prior audit caught a diagram/table mismatch — do not repeat).
- [ ] **Step 3: Add "builds on" lines** for the reusable findings (V32-V35): SW1's terminal-step substrate = Hop-B + command_events + multi-subscriber attach (V32); S6c inbox = the hot-questions queue (V33); §16 trust layer + S6a cost-capture + BL-20 Keychain generalize to MCP (V34); S6a/MCP call records are the first MetricPoint sources (V35).
- [ ] **Step 4: Verify**
```bash
for s in S-EXT S-IDEA SW1 SW2 SW3 S9a S9b SH; do grep -q "$s" docs/superpowers/specs/2026-07-01-builderpro-platform-overview.md || echo "MISSING slice: $s"; done
grep -c "Open decision — default" docs/superpowers/specs/2026-07-01-builderpro-platform-overview.md   # >=6
grep -c "workspace-wide" docs/superpowers/specs/2026-07-01-builderpro-platform-overview.md            # >=1 (S4 cross-project retrieval)
```
- [ ] **Step 5: Commit**
```bash
git add docs/superpowers/specs/2026-07-01-builderpro-platform-overview.md
git commit -m "docs(overview): §3 roadmap re-cut — S-EXT/S-IDEA/SW1-3/S9a-b/SH, reshaped deps, open-decision defaults, reusable builds-on (V1,V3-V29; items 11,15-19)"
```

---

### Task 5: Overview §4 + §5 + §6 — agent contract, human steps, meta-process (F5+F6)

**Files:**
- Modify: `docs/superpowers/specs/2026-07-01-builderpro-platform-overview.md` (§4 ~122-162, §5 ~179, §6 ~187)

**Interfaces:**
- Consumes: SW1 (Task 4). Produces: the "agent loop = default WorkflowDefinition" contract and the meta-process law bound by CONTRIBUTING (Task 9).

- [ ] **Step 1: Rewrite §4** (correction-plan item 12 + audit V1/V6 + v3/v4): the CEO→PM→eng loop is the **default built-in WorkflowDefinition executed by the SW1 engine**, NOT compiled control flow; built-in definition #2 = goal-driven research/refresh (the "traffic channels" example: goal → research → N candidate channels → N testable items); step kinds include agent-turn, terminal-command, mcp-tool, data-fetch, data-load, data-process, insight-extract, human-approval; every StepRun records {input data ref, outgoing request, received data ref, processing actions, timings, cost} (run-observability contract); Hop-B command surface is the agent API (keep the existing forward-contract wording that still holds).
- [ ] **Step 2: Amend §5** (human-in-the-loop boundary): add sanctioned human steps — **authoring/editing workflow definitions, schedules, step tool-bindings, and rules (global + per-project)**; answering the single inbox (escalations + gates); connecting MCP servers/connectors. Keep the existing "goals + quality + credentials" steps.
- [ ] **Step 3: Amend §6** (process) with the **meta-process law** (vision-v4 §8): for the platform AND for every managed project — the end goal is always visible and editable (edit → re-plan); a live step-plan to the goal is always kept and re-actualized on goal change; architecture and data structures are designed first; then a minimum is defined and extended constructor-style. State that the CEO/PM agents must operate managed projects by this same method (binds the future S6b spec).
- [ ] **Step 4: Verify**
```bash
grep -c "default.*WorkflowDefinition\|WorkflowDefinition executed by" docs/superpowers/specs/2026-07-01-builderpro-platform-overview.md  # >=1
grep -c "meta-process" docs/superpowers/specs/2026-07-01-builderpro-platform-overview.md   # >=1
grep -ci "insight-extract" docs/superpowers/specs/2026-07-01-builderpro-platform-overview.md  # >=1
```
- [ ] **Step 5: Commit**
```bash
git add docs/superpowers/specs/2026-07-01-builderpro-platform-overview.md
git commit -m "docs(overview): §4 agent-loop-as-default-workflow + step kinds + run-observability, §5 human steps, §6 meta-process law (V1,V6,V36; items 12,13,25)"
```

---

### Task 6: S0+S1 spec §16 — MCP & extension trust boundary (F7)

**Files:**
- Modify: `docs/superpowers/specs/2026-07-01-builderpro-s0s1-foundation-terminal-design.md` (§16, after the existing `### Data at rest` subsection ~line 912)

**Interfaces:**
- Consumes: nothing. Produces: the security posture the S-EXT spec implements.

- [ ] **Step 1: Add subsection `### MCP & extension boundary (roadmap — implemented in S-EXT; amended 2026-07-06)`** with content from correction-plan items 8-10 + audit V12/V13/V24 + §0.1 Q6:
  - all MCP/connector egress happens from the core/orchd process only — never the webview;
  - per-server consent on connect; enabled-tools are an explicit allowlist per server;
  - **tool results are untrusted input** — treated as potential prompt-injection; agents must not act on tool-result instructions without policy/gate mediation;
  - stdio MCP servers execute local code — connecting one is a consent-gated action equivalent to installing a plugin;
  - MCP tool invocations pass through the agent trust layer (spend caps, approval classes) — same gate as agent-initiated commands;
  - Q6 defaults: v1 surface = tools + auth only; sampling disabled by default (a remote server must not spend the owner's LLM budget); resources/prompts deferred.
- [ ] **Step 2: Generalize the credential posture.** In the existing `### Provider API keys` subsection, add a line: `This posture generalizes to ALL provider/connector/MCP secrets (BL-20): macOS Keychain only, never SQLite/config/logs; egress core/orchd-only.` Add a research-source note (V24): agents querying production DBs/docs use a registry of read-only, audited connections.
- [ ] **Step 3: Verify**
```bash
grep -c "MCP & extension boundary" docs/superpowers/specs/2026-07-01-builderpro-s0s1-foundation-terminal-design.md  # 1
grep -ci "prompt-injection\|untrusted input" docs/superpowers/specs/2026-07-01-builderpro-s0s1-foundation-terminal-design.md  # >=1
grep -ci "sampling disabled by default" docs/superpowers/specs/2026-07-01-builderpro-s0s1-foundation-terminal-design.md  # >=1
```
- [ ] **Step 4: Commit**
```bash
git add docs/superpowers/specs/2026-07-01-builderpro-s0s1-foundation-terminal-design.md
git commit -m "docs(spec): §16 MCP & extension trust boundary + generalized credential posture (V12,V13,V24; items 8-10)"
```

---

### Task 7: Protocol v2 — 3 amendments + unfreeze (F8)

**Files:**
- Modify: `docs/superpowers/specs/2026-07-06-protocol-v2-design.md`

**Interfaces:**
- Consumes: `bpa-orchd` (Task 2). Produces: the unfrozen Pv2 the next cycle implements.

- [ ] **Step 1: Add a `## Vision v2–v4 amendments (2026-07-06)` section** with the 3 items (correction-plan item 14 + audit V31):
  1. **Pv2.1 additive batch (reserved, not built now):** name the future additive requests the workflow/agent/MCP layers will need — `command+argv spawn` (no shell), `typed exit-status wait`, `ReadOutput{since_seq}` cursor read, rendered-text snapshot. Reserved in the append-only variant order so they slot in without a further break.
  2. **command_events attribution:** the schema-v2 `command_events` table gains an attribution hook now (which actor / workflow-run / step a command belongs to) so S7/S8 observability can roll up per-run cost/history without a later migration. Note it as an additive column decided here.
  3. **Frozen preamble + second client:** the codec-agnostic handshake preamble must anticipate a SECOND core-side client — `bpa-orchd` — connecting alongside the GUI (Pv2 multi-subscriber attach already supports co-attach; the preamble/version negotiation must not assume a single client identity).
- [ ] **Step 2: Flip the status line.** Change the spec's `**Status:**` header to `**Status:** unfrozen — awaiting owner review → implementation cycle (amended 2026-07-06 for vision v2–v4).`
- [ ] **Step 3: Verify**
```bash
grep -c "Pv2.1 additive batch" docs/superpowers/specs/2026-07-06-protocol-v2-design.md   # 1
grep -c "bpa-orchd" docs/superpowers/specs/2026-07-06-protocol-v2-design.md              # >=1
grep -c "unfrozen" docs/superpowers/specs/2026-07-06-protocol-v2-design.md                # >=1
```
- [ ] **Step 4: Commit**
```bash
git add docs/superpowers/specs/2026-07-06-protocol-v2-design.md
git commit -m "docs(pv2): 3 vision-alignment amendments (Pv2.1 additive batch, command_events attribution, orchd second-client) + unfreeze (V31; item 14)"
```

---

### Task 8: Backlog rows (F9)

**Files:**
- Modify: `docs/backlog.md`

**Interfaces:**
- Consumes: nothing. Produces: durable homes for deferred v2–v4 items.

- [ ] **Step 1: Append rows** (continue the BL-N numbering; check current max with `grep -c '^| BL-' docs/backlog.md` first): MCP prompt-injection + consent hardening (P1, S-EXT); remote escalation channel — Telegram/mobile answer surface (P3, post-S6c; Q3); ingested-telemetry retention + redaction impl (P2, S9a; Q15); first-class Experiment entity (P3, S8; Q11); StepRun payload-thinning job — full→metadata at 14d, 50MB cap (P2, SW1; Q15); Keychain-under-locked-screen access story for orchd unattended runs (P2, S-EXT/ADR-HOST sub-question).
- [ ] **Step 2: Update cross-refs** — any BL-4/BL-7 row mentioning the daemon now names `bpa-orchd` where the app-domain/scheduler concern applies (BL-7 daemon-restart e2e stays sessiond; retention BL-4 spans both — note which store).
- [ ] **Step 3: Verify** `grep -c '^| BL-' docs/backlog.md` increased by 6; `grep -c "bpa-orchd" docs/backlog.md` >=1.
- [ ] **Step 4: Commit**
```bash
git add docs/backlog.md
git commit -m "docs(backlog): +6 vision-alignment deferrals (MCP hardening, remote escalation, telemetry retention, experiments, payload-thinning, orchd Keychain) (V13; Q3,Q11,Q15)"
```

---

### Task 9: CONTRIBUTING + architecture + traceability (F10+F11+F12)

**Files:**
- Modify: `CONTRIBUTING.md`, `docs/architecture.md`, `docs/traceability.md`

**Interfaces:**
- Consumes: meta-process (Task 5), design-system.md (already landed). Produces: process laws + doc-consistency.

- [ ] **Step 1: CONTRIBUTING** — add a `## Meta-process` section (vision-v4 §8, same content as overview §6 Task 5) and add to the DoD checklist the **design-section rule** from `docs/design-system.md` §8 (every feature spec declares which canonical atoms it reuses / introduces + its keyboard path). Link `docs/design-system.md`.
- [ ] **Step 2: architecture.md** — add a two-daemon topology block: `bpa-orchd` (app-domain store + scheduler + engine + agent runtime, launchd-supervised) alongside `bpa-sessiond` (terminal domain), GUI as client to both; one ASCII block + a pointer to the overview charter. Add `docs/design-system.md` to "Related docs".
- [ ] **Step 3: traceability.md** — mechanical sweep: any row referencing an overview section whose content changed (§1 mission, §3 roadmap, charter) gets its wording trued up; no fabricated claims introduced. Add `docs/design-system.md` + this cycle's spec to the doc index if one exists.
- [ ] **Step 4: Verify**
```bash
grep -c "Meta-process\|meta-process" CONTRIBUTING.md          # >=1
grep -c "design-system.md" CONTRIBUTING.md docs/architecture.md  # >=1 each
grep -c "bpa-orchd" docs/architecture.md                       # >=1
```
Run `check_links` on all three files → empty.
- [ ] **Step 5: Commit**
```bash
git add CONTRIBUTING.md docs/architecture.md docs/traceability.md
git commit -m "docs: meta-process law in CONTRIBUTING, two-daemon topology in architecture, traceability sweep (item 25)"
```

---

### Task 10: Cycle DoD sweep + final gate (F-verify)

**Files:** none created; verification + inline fixes within prior tasks' files.

- [ ] **Step 1: Finding coverage.** For each audit finding V1–V36, confirm a commit or a landed edit covers it (grep the commit log `git log --oneline main..HEAD` for finding ids + inspect owning rows). Produce a V-id → deliverable table in the task report. Any uncovered → fix in the owning file now.
- [ ] **Step 2: Open-decision coverage.** Confirm each §0.1 default (Q2,Q3,Q5,Q6,Q7,Q8,Q9,Q10,Q11,Q12,Q13,Q14,Q15) appears verbatim-in-substance in its owning roadmap row or §16 subsection:
```bash
O=docs/superpowers/specs/2026-07-01-builderpro-platform-overview.md
grep -c "Open decision — default" $O   # >=10
```
- [ ] **Step 3: Banned-phrase sweep** (no pre-v2 framing residue in tracked docs, excluding research/plan artifacts):
```bash
git grep -n "director of AI development teams" -- '*.md' ':(exclude)docs/superpowers/research/*' ':(exclude)docs/superpowers/plans/*' && echo "FAIL" || echo "OK"
```
- [ ] **Step 4: Link check** — run `check_links` over every edited/created doc → no BROKEN lines.
- [ ] **Step 5: Gate** — `export PATH="$HOME/.cargo/bin:$PATH" && bash scripts/final-suite.sh` → `ALL GATES PASSED` (docs-only pass must not have touched code; this proves it).
- [ ] **Step 6: Commit** any sweep fixes:
```bash
git add -A && git commit -m "docs: DoD sweep — finding + open-decision coverage, banned-phrase + link checks (cycle 3 close-out)"
```

---

## Execution notes (for the controller)

- **Groups:** G1 sequential = Tasks 1→2→3→4→5 (all one file: `platform-overview.md` — MUST be sequential, no parallel edits to it). G2 parallel = Tasks 6, 7, 8 (disjoint files: s0s1 spec / pv2 spec / backlog). G3 = Task 9 (three disjoint files). G4 = Task 10 sweep → final whole-branch review → PR → live CI green → merge (finishing-a-development-branch).
- **Review:** two-stage per task (spec/correction-plan compliance → doc quality); final whole-branch review = truth-vs-decisions + cross-doc consistency + correction-plan compliance (as Cycle 1/2).
- **Content source:** the correction plan + spec §0/§0.1 are the script; implementers copy content in full prose into the target docs (the target must stand alone — no "see the correction plan" pointers in the deliverable docs except as provenance references).
- **Plan-time facts:** overview is one file (§1-§6 + charter) → Tasks 1-5 serialize on it; `gh` authed (`sshlg`) for the PR; final-suite is 8 stages and must stay green.

## Self-review (done)

- Spec coverage: F1→T1, F2→T2, F3→T3, F4→T4, F5+F6→T5, F7→T6, F8→T7, F9→T8, F10+F11+F12→T9, DoD→T10. All 12 deliverables mapped.
- Open decisions: all 13 defaults routed (Q2,Q7→T4 rows; Q3,Q11,Q15→T8 backlog + T4 rows; Q5,Q8,Q10→S-IDEA row T4; Q6→T6 §16; Q9,Q12→T4 rows; Q13→charter T3 RuleSet + T4 S3 row; Q14→S-EXT row T4). T10 Step 2 verifies coverage.
- Findings: V1-V36 → T10 Step 1 verifies the full matrix; reusable V32-V35 → T4 Step 3.
- No placeholders: every task has exact file, exact grep DoD, exact commit message; content sourced from named doc sections.
- Consistency: locked strings (`bpa-orchd`, slice ids, `Open decision — default`, mission sentence) defined once in Global Constraints, reused by grep DoD across tasks.
