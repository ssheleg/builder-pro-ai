# Builder Pro AI — Platform Overview & Roadmap

**Date:** 2026-07-01
**Status:** Approved (decomposition + global decisions)
**Scope:** This is the *map*. Each subsystem below gets its own `spec → plan → implementation`
cycle. Detailed contracts live in per-slice specs, not here.

---

## 1. Product

> (amended 2026-07-06, vision v2–v4 — see `docs/superpowers/research/2026-07-06-product-vision-v2.md`)

A lightweight macOS control panel whose job is to **take any idea to a working project and
manage/organize the vibecoding process**. The pain it exists to kill: with 5–6 projects in flight
at different speeds and stages, goals and context get lost and the owner degrades into a poller
and button-presser — **the attention tax is the enemy**. Everything that can proceed under policy
proceeds without the owner; the system pulls the owner in (one batched inbox, morning summaries) —
the owner never polls.

Opening the app answers, for ALL projects at once and in under 30 seconds: where is each one,
what moved since I last looked, and what — if anything — needs ME. The home screen shows: a live
agent feed (who is doing what, on which stage, in which project), per-project progress,
idea quick-capture into a backlog, an Insights lane (market-sourced opportunities with
fit-verdicts against goals/metrics), agent-org settings (the CEO and the other organization
agents are configurable), vector steering (edit goals/priorities — agents pick it up), and the
hot-questions inbox. Underneath sit real terminals running off-the-shelf coding agents
(claude-code, hermes, opencode, kilo, …) driven by app-native meta-agents, plus workspaces of
repos, a live file explorer, a workspace-wide knowledge graph, a kanban, editable workflows, and
observability.

**North-star metrics**

1. **Time-to-context** on app open: < 30 s for all projects.
2. **Owner interventions per shipped task**: → 1 (the one approval that matters).
3. **Unattended progress**: hours agents advance without stalling on a button.

**Design tenets**

- **Production-grade, no MVP half-states.** Every slice ships finished: TDD tests, error handling
  + honest degradation, structured logging, docs. No "TODO later", no stubs.
- **Max autonomy, min human-in-the-loop.** Humans set goals + quality; agents self-decide
  everywhere they safely can. Escalation is the exception, batched and validated with the user.
- **Minimalist but functional UI.** The panel is light; the power is in the terminals + agents
  (visual/UX rules: `docs/design-system.md`).
- **Honest about state.** The app never fakes session/agent status.
- **Additive, constructor-style growth.** The system extends cube-by-cube; schema and
  architecture grow by addition, never rebuild.

---

## 2. Global locked decisions (apply to every slice)

| Decision | Choice |
|---|---|
| Desktop framework | **Tauri 2** (Rust core + web UI) |
| Distribution | **Universal macOS binary** (arm64 + x86_64), signed + notarized |
| Frontend | **React 19 + Vite + TypeScript**, Zustand for state |
| Strategist role ("SEO" in the brief) | **CEO / strategy orchestrator** — reads goals + graph (+ future metrics), decides what to build |
| Meta-agent execution | **App-native agents** (CEO, PM, engineering specialists) run *inside* the app brain |
| LLM provider for meta-agents | **OpenRouter-first** provider abstraction (also OpenAI / GLM / others behind one interface) |
| Custom agents | User can **add extra app-native agents** at any stage |
| External coding agents | Run as **terminal workers** (claude-code, hermes, opencode, kilo, …) driven by the app brain |
| Terminal control surface | The programmatic terminal API built in S1 **is** the surface the agent brain drives in S6 |
| Methodology | **TDD + DDD**; Superpowers planning cycle (brainstorm → spec → plan → subagent-driven dev) |
| Unattended execution + app-domain store host | **A second launchd-managed daemon `bpa-orchd`** (mirror of the `bpa-sessiond` pattern) owns the app-domain store, the scheduler, the workflow-engine runtime, and the agent runtime; the GUI is its client. `bpa-sessiond`'s charter is unchanged (terminal domain only). See ADR-HOST below. |

**Locks added 2026-07-06 (vision v2–v4):**

- **Multi-project from day one:** every S2+ data model, store, and event stream is multi-project;
  UI panels are per-project VIEWS over multi-project data.
- **Additive-only schema evolution:** entities grow by adding tables/columns; migrations are
  append-only by policy — never a rebuild (vision-v4 §7, «конструктор: кубик за кубиком»).

### ADR-HOST — the two-daemon topology (locked 2026-07-06)

**Problem.** Vision v2's recurring capabilities (the 24h prod-error self-heal loop, metrics
watching, scheduled research) must fire with the GUI closed, survive restarts, and never silently
stop. But app-native agents were slated to run in the core (GUI) process, which dies on window
close; and the Data-layer charter deliberately bars `bpa-sessiond` — the only always-on process —
from becoming the app's general engine or database. As architected, unattended execution was
impossible (audit V2).

**Decision.** A second headless, launchd-supervised daemon — **`bpa-orchd`** — owns: the
app-domain store (all charter entities), the scheduler + event triggers (SW2), the workflow-engine
runtime (SW1), and the agent runtime (S6b+). The GUI attaches to it as a client, exactly as it
attaches to `bpa-sessiond`. `bpa-orchd` drives terminals through the same Hop-B socket protocol as
any client (Pv2 multi-subscriber attach lets orchd and the GUI co-watch one session).

**Rejected alternatives.** (b) Widening `bpa-sessiond`'s charter with an orchestration domain —
riskiest option: couples the hardened, stable terminal daemon to the fastest-moving domain logic.
(c) Honest v0 degradation («schedules fire only while the app runs», catch-up on launch) — fails
the attention-tax mission; kept only as the documented interim behavior until `bpa-orchd` ships.

**Consequences.** orchd reuses the proven patterns verbatim: launchd LaunchAgent lifecycle +
runbook, fail-closed forward-only migrations, the D4/Pv2 drain-and-consent upgrade choreography.
Open sub-question routed to the S-EXT/security spec: Keychain access for unattended runs while
the screen is locked (BL row added this cycle).

### Session survival truth table (canonical — slices reference this, must not drift)

The terminal engine (S1) keeps sessions alive in a detached daemon. What actually survives:

| Event | Sessions |
|---|---|
| GUI close / crash / restart | live shells **keep running** (daemon-owned); reattach + replay |
| Daemon restart / upgrade / crash | live shells **end**; session records + scrollback survive (up to the last ~1 s flush) and rehydrate as **inactive** sessions |
| **macOS logout** | die (per-user LaunchAgent torn down); cross-logout survival is out of scope (needs a root LaunchDaemon) |
| Workflow / agent runs | hosted by `bpa-orchd` (launchd-supervised, survives GUI close); a run's step state persists in the app-domain store and resumes/catches-up on orchd restart (catch-up policy per SW2) |

This is stated honestly in-app; the product never claims sessions "survive anything."
(Amended 2026-07-04, A12: the earlier "daemon restart → survive" row overstated — live PTYs die
with the daemon; only records + scrollback survive.)

---

## Protocol evolution & upgrade policy (added 2026-07-04 — A3/A14; owner decisions D3/D4)

The Hop-B wire (core ⇄ daemon) outlives app updates by design, so its evolution is policy, not
improvisation:

- **Wire discipline:** enum variant order is FROZEN (append-only); new requests/pushes are
  appended; fields are added additively. Every protocol change ships a cross-version decode test.
- **Version negotiation (protocol v2, Cycle 2):** the client sends its supported range; the daemon
  answers in-range or `Incompatible{min,max}`, and the GUI shows a remediation dialog (below) —
  never a silent misparse. Today PROTO_VERSION=1 is exact-match; negotiation lands with v2.
- **Daemon upgrade choreography (owner decision D4):** when a new app version carries a new
  daemon, the GUI shows a consent dialog — *"Update background service — N live sessions will
  end; records + scrollback survive"* — then drains via SIGTERM and replaces the daemon with
  `launchctl kickstart -k`. `DaemonShutdown{drain}` is currently a no-op Ack and is **reserved**
  until protocol v2 defines real drain semantics.
- **Release channel:** manual notarized DMG for v0.x; Tauri auto-updater is a named roadmap item
  (BL-19, `docs/backlog.md`).
- **Workspace evolution (A14):** multi-root workspaces arrive as an additive
  `workspace_roots: Vec<PathBuf>` alongside the existing `root_path` (compat preserved), slice S2.

---

## 3. Subsystem decomposition & dependency order

Build spine-first. Each row is an independent spec/plan/build unit.

| # | Subsystem | Depends on | Notes · product DoD · north-star metric |
|---|---|---|---|
| **S0** | App shell + foundation (window, theme, settings) | — | Skeleton everything plugs into. *(Amended: durable storage is DAEMON-owned SQLite, built in S1 — see Data-layer charter.)* **DONE.** |
| **S1** | **Terminal engine** — real PTYs, multi-terminal, lifecycle states, survive-restart, reattach | S0 | Highest technical risk; heart of the product. **DONE** (merged @ 285cb2e). |
| **Pv2** | **Protocol v2** — codec migration (tagged-enum-safe), version-range negotiation, wire-level **multi-subscriber attach** (D5), real `DaemonShutdown{drain}`, `bpa.db` schema-migration policy + `command_events` | S1 | One planned wire break before the protocol grows. DoD: old GUI vs new daemon shows the remediation dialog (never misparses); two subscribers stream one session; cross-version decode tests green. Metric: zero silent protocol failures. |
| **S2** | Workspace + file explorer (multi-root repos, open/create files & folders, live file-watch) | S0 | React to files agents create; additive `workspace_roots`. DoD: create/open a multi-repo workspace ≤3 clicks; file tree reflects an external `touch` <1 s; explorer stays responsive at 10k files. Metric: time-to-first-terminal in a fresh workspace. |
| **S3** | Projects + Goals/Context data model | S0 | Foundation for the knowledge layer; core-owned store (Data-layer charter). DoD: goals CRUD survives app restart; Project⇄Workspace mapping enforced; export/import round-trips. Metric: goals actively referenced by S6 runs (>0 per run). |
| **S4** | Knowledge graph (per-project + cross-project links, viz) | S3 | `@xyflow/react`; storage + UUID node identity + **agent retrieval API** are S4-spec decisions. **Hard-blocks S6 (owner decision D6).** DoD: cross-project link survives both projects' restarts; retrieval API returns a goal's subgraph <100 ms; graph editable in UI. Metric: graph nodes retrieved per CEO decision. |
| **S5** | Kanban (backlog / todo / waiting / progress / testing / done) | S3 | `@dnd-kit`; minimalist. DoD: drag persists; agent-driven card moves render live; column WIP counts. Metric: cards moved by agents vs by hand. |
| **S-EXT** | **Extensions: MCP client + connectors + skills/plugins** (the Claude Code format) — MCP server registry (add/enable/disable, global + per-project), auth (API key / OAuth 2.1, Keychain), stdio + Streamable HTTP transports, tool discovery + list-changed notifications, typed invoke with retries/timeouts/honest degradation, per-call cost/latency capture, connector accounts (e.g. social networks), skills, management UI | S3 (registry storage), BL-20 Keychain pattern. NOT behind Pv2 or S6a — MCP is orchd/core-outbound JSON-RPC, never Hop-B | Builds on: §16 trust layer + BL-20 Keychain generalize verbatim (V34). DoD: prowl.chat connected; tools listed; one research tool invoked; result persisted as a durable artifact. Metric: MCP tools invocable from workflow steps. Open decision — default: MCP v1 surface = tools + auth only; sampling DISABLED by default; resources/prompts → backlog (Q6). Open decision — default: skills adopt the Claude Code SKILL.md format for portability (Q14). |
| **S-IDEA** | **Ideas + research pipeline** — quick-capture UI (⌘K), idea inbox + lifecycle chips, «add to project» / «spawn project from idea» (creates Project+Workspace scaffolding via S2/S3), research trigger (prowl via S-EXT) with inline spend approval, streaming research pane, ResearchArtifact viewer, **insight fit-test vs goals/metrics**, task formation + decomposition handoff. **Ships BEFORE the agent org** — the first user-visible v2 value | S3, S-EXT, S4 | DoD: idea → prowl research → evaluated insight → task lands in the backlog, end-to-end, without the S6 agent org. Metric: ideas reaching «specced» per week. Open decision — default: one prowl session_id per research-run (Q5). Open decision — default: prowl down ⇒ inline «сформировать задачу без ресёрча» with an honest-degradation note on the artifact chain (Q8). Open decision — default: insight fit-verdict computed by an agent WITH one-click owner override; no auto-accept (auto-archive of clear non-fits allowed, reasoning kept) (Q10). |
| **S5** | Kanban **as a VIEW over the Task entity** (backlog / todo / waiting / progress / testing / done) | S3 | `@dnd-kit`; minimalist. A card projects a Task/Subtask row; column derives from task/workflow-run status; agent- and engine-driven moves render live. DoD: drag persists; agent-driven card moves render live; column WIP counts. Metric: cards moved by agents vs by hand. Open decision — default: per-project stack rank (plan+bug mixed) + panel-level cross-project rank; owner ranks HARD-override agent ranks (suggestions with visible reasoning) (Q9). |
| **S6a** | **LLM provider layer + tool-calling** — provider trait; OpenRouter/OpenAI/GLM adapters; routing + fallback; streaming; retries; **tool-use/function-calling in the trait** (tool defs in requests, streamed tool_call deltas, cross-provider normalization); **per-call cost/token/latency capture from the first call** | S4, Pv2 | Keys in Keychain (BL-20); egress core/orchd-only. LLM model-providers (this) and MCP tool-providers (S-EXT) are two DIFFERENT abstractions. DoD: same prompt runs on 2 providers via config only; a tool-call round-trip works on 2 providers; provider outage fails over with an honest event; every call logged with cost. Metric: $/task visible per run. |
| **SW1** | **Workflow engine core (workflow-as-data)** — versioned WorkflowDefinition (format_version, append-only evolution; runs pin their version), ordered/DAG steps, step kinds: agent-turn \| terminal-command \| mcp-tool \| data-fetch \| data-load \| data-process \| insight-extract \| human-approval; step executor; manual trigger; **run-observability contract**: every StepRun records {input ref, request, response ref, processing actions, timings, cost} | S3, Pv2, S6a, ADR-HOST | Builds on: Hop-B + command_events + multi-subscriber attach = the terminal-step substrate — reuse, don't reinvent (V32). DoD: a 3-step definition (mcp-tool → data-process → human-approval) authors, runs, and its run history shows per-step I/O drill-down. Metric: % of orchestration flowing through definitions (target: 100% by S6b). |
| **S6b** | **Agent runtime + ONE role end-to-end, running VIA the engine** — the CEO→PM→eng loop is the DEFAULT WorkflowDefinition executed by SW1, not bespoke control flow; PM gap-analysis on a real repo; agents operate managed projects by the meta-process (§6) | SW1 | Hosted by `bpa-orchd` (survival row resolves). DoD: PM produces a spec+task list from goals+graph on a live repo via the default definition; run survives GUI close and orchd restart; audit log complete. Metric: tasks completed without human touch. |
| **S6c** | **Approval inbox — the ONE generic gate** — intake: agent escalations AND workflow human-approval steps AND policy gates (spend, destructive, deploy, spec approve); single queue across ALL projects; batch-approve | S6b (UI), SW1 (gate semantics) | Builds on: this inbox IS the home screen's hot-questions queue (V33) — bind, don't fork. DoD: gated action blocks until approval; batch-approve works; every agent command in the audit log. Metric: escalations per completed task (target ↓). Open decision — default: hard gates = task-breakdown approve + spec approve + deploy; deploy auto-passes LOW-RISK diffs = no DB migrations AND diff < 300 LOC AND paths within the project allowlist — deterministic rules, no LLM classifier in v1; per-project policy may tighten/loosen (Q2). Open decision — default: away-from-Mac = macOS notification only in v0.x; remote answering = backlog (Q3). |
| **SW2** | **Scheduler + event triggers + domain event bus** — cron \| event \| manual triggers on definitions; persisted schedules with next_fire_at rehydration; missed-run catch-up policy; overlap control; approval-suspend (a gated run parks without blocking the scheduler) | SW1, ADR-HOST (host), S6c (suspend/alert routing) | DoD: a 24h schedule fires with the GUI closed; a missed window catches up per policy; two overlapping fires resolve per policy; run pauses on a gate and resumes on approval. Metric: scheduled-run reliability (fired/expected). |
| **SW3** | **Visual workflow editor** — canvas definition editor (add/reorder/edit steps), three-tab tool-binding picker (Agents / Terminals / MCP tools), trigger + gate/policy editors, validation, «run now», run-history view with per-step I/O | SW1, SW2, S-EXT | DoD: the owner composes the 24h self-heal chain end-to-end in the editor, enables it, edits it later (adds a step) without breaking pinned runs. Metric: definitions authored/edited by the owner (not shipped defaults). |
| **S6d** | **External worker adapters** (claude-code, hermes, opencode, kilo) — per-CLI liveness/stuck detection (raw-mode TUIs are invisible to `waiting_for_input` — adapters own this) as step workers | S6b | DoD: adapter detects worker-waiting within 5 s across all 4 CLIs; wedged worker auto-escalates. Metric: mean time-to-unstick. |
| **S6e** | **Custom-agent authoring + configurable agent org** (roles, models, policies, prompts for the CEO and every org agent) | S6c | 'Tools' = Tool Registry ids (S-EXT + terminals + agents); custom agents/chains run under the same trust layer + audit. DoD: owner creates a custom role in UI; it runs gated + audited. Metric: custom agents in weekly use. |
| **S9a** | **Project telemetry ingestion + study folder** — per-project LogSourceConfig registry (error-tracker APIs, hosting log pulls, file tails, MCP-backed connectors; credentials in Keychain), pull/tail into ErrorGroups (dedup), StudyItem triage queue | S3 (project entity), ADR-HOST | DoD: a real project's prod errors land as deduped ErrorGroups; new groups enqueue StudyItems; retention/redaction per charter telemetry class. Metric: time from prod error to StudyItem. Open decision — default: enabling the self-heal workflow REQUIRES a tested rollback recipe in the deploy config — refused with an actionable message otherwise; run-budget breach ⇒ pause as gated escalation (Q7). |
| **S9b** | **Delivery/deploy step + verification** — per-project deploy config (command/target), execution via terminal workers (command_events = the run record; not a CD system), deploy gate (S6c), post-deploy verify step (error-rate/smoke re-check; a run may not mark itself successful without it) | SW1, SW2, S9a, S6c | DoD: the self-heal definition deploys a fix and verification confirms the error group went quiet — or the run honestly reports otherwise. Metric: verified fixes shipped per week. |
| **S7** | Stats / observability — time worked, task counts, stages, errors, logs, LLM traces, spend/budgets, evals, **workflow-run health** (last success, consecutive failures, missed runs, per-run cost rollups, dead-loop alerts) | S6b, SW2 (run history) | `recharts`. DoD: per-project spend dashboard; per-agent success/error rates; budget alert fires; scheduled-run health visible. Metric: % runs within budget. |
| **S8** | **Metrics ingestion + metrics→sprint** — per-project external metrics/analytics via the S9a connector substrate; MetricPoint timeseries (append-only, retention/downsample); **MetricDefinition: owner-declared MAIN metrics, mutable**; the metrics-watch→sprint-planning workflow (SW1 definition); insight evaluation reads metrics | S3 (data), SW1/SW2, S9a (connectors); S7 only for dashboards | Builds on: S6a/MCP call records are the first MetricPoint sources (V35). DoD: a project declares main metrics; a metrics-watch run drafts a sprint plan citing them. Metric: CEO/sprint decisions citing declared metrics. Open decision — default: metrics are goal-attached (each goal names the metric(s) it moves); project settings is the editor (Q12). Open decision — default: experiments = Tasks tagged `experiment` (hypothesis/metric/verdict in body); first-class entity deferred (Q11). |
| **SH** | **Mission-control home (capstone)** — six lanes per project (goals, plan tasks, bug-fix tasks, prioritization, delivery/workflow state, monitoring) + live agent feed + «what moved» deltas + Insights lane + idea quick-capture + hot-questions inbox + agent-org settings + vector steering | S6c, S7, SW1 run state, S-IDEA, S9a/S9b | DoD: north-star metric (a) holds — open the app → full 5-6-project context < 30 s; every element live, no manual refresh. Metric: time-to-context (measured). |

```
S0 ─┬─ S1 ─ Pv2 ─────────────────────────────┐
    ├─ S2                                     │
    └─ S3 ─┬─ S4 ────────┬────────────────────┤
           ├─ S5         │                    │
           ├─ S-EXT ─────┴─ S-IDEA            │   (S-IDEA = first v2 value,
           │                                  │    BEFORE the agent org)
           └──────────────────────────────────┤
                                              ▼
                    S6a ─ SW1 ─┬─ S6b ─┬─ S6c ─ S6e
                               │       ├─ S6d
                               │       └─ S7
                    SW1 ─ SW2 ─┴─ SW3  S9a ─ S9b
                    (SW2 deps SW1+ADR-HOST+S6c; S9a deps S3+ADR-HOST;
                     S8 deps S3+SW1/SW2+S9a; SH = capstone over
                     S6c + S7 + SW1 run state + S-IDEA + S9a/S9b)
```

**Current slice:** S0+S1 **DONE**; docs-truth/CI pass **DONE**; vision-alignment pass (this
cycle). Next: Pv2 implementation → S2 → S3 → S4 ∥ S-EXT ∥ S5 → S-IDEA → S6a → SW1 → …
Spec: `2026-07-01-builderpro-s0s1-foundation-terminal-design.md`.

---

## 4. How the agent layer will use the terminal engine (forward contract)

*(Rewritten 2026-07-04 — A4/A5/A6/A16; owner decision D5.)*

**The canonical programmatic agent API is the Hop-B socket protocol** (create / attach /
write-stdin / stream-output / resize / kill / query-state) — not the Tauri command list, which is
merely the GUI's thin wrapper over it. Agent process model: **app-native agents (CEO/PM/eng) run
in the core process** (S6b) and reach the daemon over the same socket client; **external worker
CLIs run inside PTY sessions** as ordinary children.

The orchestration loop (unchanged vision):

1. **CEO** picks the next objective from goals + knowledge graph (+ future S8 metrics). *(S4 is a
   hard prerequisite — owner decision D6.)*
2. **PM** (TDD+DDD) does gap analysis (current vs 100%-done), writes specs, decomposes into tasks,
   assigns each to an engineering specialist (ML / data-sci / security / backend / frontend / testing).
3. Each specialist **spawns / drives a terminal worker** (claude-code, etc.) and watches it via the
   worker adapter (below).
4. Unresolvable questions bubble PM → CEO, which batches them and **validates with the human**,
   then feeds answers back to the workers.
5. Kanban (S5) reflects the live state; Stats (S7) records it.

**Contract corrections locked now (so S6 doesn't build on false assumptions):**

- **Co-viewing (owner decision D5):** human and agent watching one session simultaneously is
  served by **wire-level multi-subscriber attach** (N subscribers per session, each getting its
  own Replay) — designed and implemented in **protocol v2**. Until then S1's single-attach
  (connection-owned) stands.
- **`waiting_for_input` honest scope:** it is a **canonical-mode line-input heuristic only**. It
  is structurally blind to raw-mode TUIs — which includes every named worker CLI (claude-code,
  opencode, …). Worker liveness / stuck detection is therefore a **named S6d subsystem** (worker
  adapters, per-CLI strategies), NOT this flag.
- **Planned additive agent capabilities** (requests to add, append-only): `ReadOutput{since_seq}`
  cursor read; rendered-text snapshot (grid as text); command+argv spawn (no shell wrapping);
  typed exit-status wait.

This is why S1's programmatic control is first-class in the S0+S1 spec even though the agents
come later.

---

## Data-layer charter (added 2026-07-04; amended 2026-07-06, vision v2–v4)

- **`bpa-sessiond` owns terminal-domain durable state ONLY** (sessions, scrollback,
  workspaces-as-terminal-roots, `command_events` in `bpa.db`). It must never become the app's
  general database.
- **All app-domain entities live in the `bpa-orchd` store** (ADR-HOST, §2). The concrete engine
  is decided in the S3 spec; the OWNERSHIP boundary is locked here.
- **Project ⇄ Workspace (locked):** *a Project is the planning entity that owns goals / graph /
  kanban and maps to 1..N Workspaces (repo roots).*
- **S4 owns:** graph storage decision, UUID node identity, cross-project link model, and the
  **agent retrieval API** — read AND write, **workspace-wide** (an agent working project A can
  query and read project B's knowledge). S4 **hard-blocks S6** (owner decision D6).

### Entity map (owning slice + store)

| Entity family | Store | Owning slice |
|---|---|---|
| Project | orchd | S3 |
| Workspace (repo roots; bridged to sessiond terminal-roots) | orchd (+ sessiond bridge) | S2/S3 |
| Goal — hierarchy: 1 strategic + N additional, owner-editable (vector steering) | orchd | S3 |
| Idea — lifecycle captured→researching→specced→in-dev→shipped; nullable project_id (spawn-project flow) | orchd | S3 |
| Insight — market-sourced; fit-verdict vs goals/metrics; accepted→backlog \| archived-with-reasoning | orchd | S3 (+S-IDEA) |
| Task / Subtask — one unified model; kanban is a VIEW; sources: idea \| insight \| bug \| plan | orchd | S3 |
| RuleSet — global + per-project rules; markdown layer (agent-read) + typed policy layer (gate-enforced) | orchd | S3 |
| ResearchArtifact — durable research output; provenance links (idea, run, session); graph-ingested | orchd (+ artifact blobs) | S-IDEA/S4 |
| WorkflowDefinition — versioned, format_version, append-only evolution | orchd | SW1 |
| WorkflowRun / StepRun — run history; StepRun records {input ref, request, response ref, processing, timings, cost} | orchd (+ run/step logs) | SW1 |
| Schedule / Trigger — cron \| event \| manual; next_fire_at; catch-up + overlap policy | orchd | SW2 |
| MCP / Connector / Skill registry — servers, transports, enabled tools, accounts; global + per-project | orchd (secrets → Keychain) | S-EXT |
| MetricDefinition (owner-declared MAIN metrics, mutable) + MetricPoint (timeseries) | orchd | S8 |
| ErrorGroup / StudyItem (prod telemetry triage) + Deploy record | orchd | S9a/S9b |

- **Historical telemetry:** worker transcript log + `command_events` land at the daemon schema-v2
  bump (Pv2) — with a workflow-run attribution hook (Pv2 amendment).

### Global storage architecture

One map, four stores — growth-ready, additive-only (§2 lock):

1. **`bpa-sessiond` `bpa.db`** — terminal domain (sessions, scrollback, command_events). Unchanged.
2. **`bpa-orchd` app-domain store** — the 14 entity families above; ships schema v1 with its own
   `user_version`, **fail-closed, forward-only, single-transaction migrations** (mirrors sessiond
   persistence); additive-only by policy.
3. **Run/step logs** — StepRun I/O payloads. Default retention (Q15): full payloads 14 days →
   thinned to metadata (summaries + sizes + hashes); 50 MB per-run cap with honest truncation
   markers.
4. **Artifact blobs** — research artifacts and large payloads, referenced by id from orchd rows.

- **Cross-store references are SOFT:** UUID strings, no FK-integrity assumption across stores;
  a store must keep its own copy of anything it cannot afford to lose to another store's
  retention (e.g. BL-4 scrollback pruning must not destroy workflow-run evidence — orchd keeps
  what it needs).
- **Ingested telemetry is a distinct data class:** prod logs/errors and metrics may carry
  PII/secrets; store home + redaction + retention are decided in the S9a spec; purge-on-delete
  ties into project deletion.

---

## 5. Human-in-the-loop boundary

Autonomy is the default. The only sanctioned human steps: setting/adjusting **goals + quality
bars**, answering **batched escalations** the agents genuinely can't resolve, and providing
**credentials/access** (API keys, repo access). Everything else is agent-decided.

---

## 6. Process

Every slice follows the Superpowers cycle: `brainstorming → spec → writing-plans →
subagent-driven-development`, with contracts locked in the spec, non-overlapping file ownership
for parallel tasks, TDD throughout, and a per-task Definition of Done. External-library contracts
are verified against current docs (Context7 + web) before locking — never from memory.
