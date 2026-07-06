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

### Session survival truth table (canonical — slices reference this, must not drift)

The terminal engine (S1) keeps sessions alive in a detached daemon. What actually survives:

| Event | Sessions |
|---|---|
| GUI close / crash / restart | live shells **keep running** (daemon-owned); reattach + replay |
| Daemon restart / upgrade / crash | live shells **end**; session records + scrollback survive (up to the last ~1 s flush) and rehydrate as **inactive** sessions |
| **macOS logout** | die (per-user LaunchAgent torn down); cross-logout survival is out of scope (needs a root LaunchDaemon) |
| Agent runs (S6+) | **undefined until S6b** — agent state is NOT covered by the daemon survival model (honest placeholder) |

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
| **S6a** | **LLM provider layer** — provider trait; OpenRouter/OpenAI/GLM adapters; routing + fallback; streaming; retries; **per-call cost/token/latency capture from the first call** | S4, Pv2 | Keys in Keychain (BL-20); egress core-only. DoD: same prompt runs on 2 providers via config only; provider outage fails over with an honest event; every call logged with cost. Metric: $/task visible per run. |
| **S6b** | **Agent runtime + ONE role end-to-end** (PM gap-analysis loop against a real repo) | S6a | Agent state survival defined here (survival-table row resolves). DoD: PM produces a spec+task list from goals+graph on a live repo; run resumable after app restart; audit log complete. Metric: tasks completed without human touch. |
| **S6c** | **Escalation loop + approval inbox UI** | S6b | Trust layer lands here (spec §16: approval classes P1, batched inbox, audit). DoD: gated action blocks until approval; batch-approve works; every agent command in the audit log. Metric: escalations per completed task (target ↓). |
| **S6d** | **External worker adapters** (claude-code, hermes, opencode, kilo) | S6b | Per-CLI liveness/stuck detection (raw-mode TUIs are invisible to `waiting_for_input` — adapters own this). DoD: adapter detects worker-waiting within 5 s across all 4 CLIs; wedged worker auto-escalates. Metric: mean time-to-unstick. |
| **S6e** | **Custom-agent authoring** (user-defined app-native agents) | S6c | DoD: user creates a custom role (prompt+model+tools) in UI; it runs under the same trust layer + audit. Metric: custom agents in weekly use. |
| **S7** | Stats / observability — time worked, task counts, stages, errors, logs, **LLM traces, spend/budgets, evals** | S6b | `recharts`. DoD: per-project spend dashboard; per-agent success/error rates; budget alert fires. Metric: % runs within budget. |
| **S8** | *(future)* Analytics integrations (Google Analytics, Mixpanel) → data-driven decisions | S7 | Feeds the CEO agent. DoD: at least one external metric visible to the CEO with provenance. Metric: CEO decisions citing product metrics. |

```
S0 ─┬─ S1 ─ Pv2 ───────────┐
    ├─ S2                   │
    └─ S3 ─┬─ S4 ───────────┤   (S6a depends on S4 + Pv2 exactly;
           └─ S5            │    S4 hard-blocks S6 — owner decision D6.
                            ▼    S5 kanban is consumed by agents at RUNTIME,
              S6a ─ S6b ─┬─ S6c ─ S6e        not a build dependency.)
                         ├─ S6d
                         └─ S7 ─ S8
```

**Current slice:** S0 + S1 — **DONE** (merged @ 285cb2e). Next: docs-truth/CI pass (this cycle),
then Protocol v2 (Cycle 2), then S2.
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

## Data-layer charter (added 2026-07-04 — A13/A25/A26/A27; owner decision D6)

- **The daemon owns terminal-domain durable state ONLY** (sessions, scrollback, workspaces-as-
  terminal-roots in `bpa.db`). It must never become the app's general database.
- **S3+ domain data** (projects, goals, kanban, knowledge graph) lives in a **core-owned store**;
  the concrete engine (SQLite in the core process vs embedded alternative) is decided in the S3
  spec — NOT inherited by default from the daemon.
- **Project ⇄ Workspace (locked):** *a Project is the planning entity that owns goals / graph /
  kanban and maps to 1..N Workspaces (repo roots).*
- **S4 owns:** graph storage decision, UUID node identity, cross-project link model, and the
  **agent retrieval API**. S4 **hard-blocks S6** (owner decision D6).
- **Historical telemetry:** worker transcript log + `command_events` table land at the next daemon
  schema bump (protocol v2 or S3) — today's discarded history (pruned rings) is a known limit.

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
