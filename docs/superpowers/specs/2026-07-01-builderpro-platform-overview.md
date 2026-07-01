# Builder Pro AI — Platform Overview & Roadmap

**Date:** 2026-07-01
**Status:** Approved (decomposition + global decisions)
**Scope:** This is the *map*. Each subsystem below gets its own `spec → plan → implementation`
cycle. Detailed contracts live in per-slice specs, not here.

---

## 1. Product

A lightweight macOS desktop control panel that turns the user into a **director of AI development
teams**. The user sets goals and quality bars; the app's meta-agents decide *what* to build and
drive off-the-shelf coding agents (claude-code, hermes, opencode, kilo, …) that run in real
terminals. Around the terminals sits everything needed to run a real dev shop: workspaces of
repos, a live file explorer, a knowledge graph (linked across projects), a Kanban, goals, and
observability.

**Design tenets**

- **Production-grade, no MVP half-states.** Every slice ships finished: TDD tests, error handling
  + honest degradation, structured logging, docs. No "TODO later", no stubs.
- **Max autonomy, min human-in-the-loop.** Humans set goals + quality; agents self-decide
  everywhere they safely can. Escalation is the exception, batched and validated with the user.
- **Minimalist but functional UI.** The panel is light; the power is in the terminals + agents.
- **Honest about state.** The app never fakes session/agent status.

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
| GUI close / crash / restart | **survive** (reattach + replay) |
| Daemon restart | survive (SQLite rehydrate; scrollback up to last flush ≈500 ms) |
| **Daemon crash** | live shells die (they are the daemon's children); scrollback replays up to last flush |
| **macOS logout** | die (per-user LaunchAgent torn down); cross-logout survival is out of scope (needs a root LaunchDaemon) |

This is stated honestly in-app; the product never claims sessions "survive anything."

---

## 3. Subsystem decomposition & dependency order

Build spine-first. Each row is an independent spec/plan/build unit.

| # | Subsystem | Depends on | Notes |
|---|---|---|---|
| **S0** | App shell + foundation (window, theme, settings, local SQLite) | — | Skeleton everything plugs into |
| **S1** | **Terminal engine** — real PTYs, multi-terminal, lifecycle states, survive-restart, reattach | S0 | Highest technical risk; heart of the product |
| **S2** | Workspace + file explorer (repos, open/create files & folders, live file-watch) | S0 | React to files agents create |
| **S3** | Projects + Goals/Context data model | S0 | Foundation for the knowledge layer |
| **S4** | Knowledge graph (per-project + cross-project links, viz) | S3 | `@xyflow/react` |
| **S5** | Kanban (backlog / todo / waiting / progress / testing / done) | S3 | `@dnd-kit`; minimalist |
| **S6** | **Agent orchestration** — CEO + PM + eng specialists, terminal control, escalation loop | S1, S3, S5 | The brain; OpenRouter-backed |
| **S7** | Stats / observability (time worked, task counts, stages, errors, logs) | S6 | `recharts` |
| **S8** | *(future)* Analytics integrations (Google Analytics, Mixpanel) → data-driven decisions | S7 | Feeds the CEO agent |

```
S0 ─┬─ S1 ───────────────┐
    ├─ S2                 │
    └─ S3 ─┬─ S4          │
           ├─ S5 ─────────┤
           └──────────────┤   (S3 goals/graph feed S6 directly, per §4)
                          ▼
                    S6 ─ S7 ─ S8
```

**Current slice:** S0 + S1, combined (Foundation is inseparable from proving the terminal engine).
Spec: `2026-07-01-builderpro-s0s1-foundation-terminal-design.md`.

---

## 4. How the agent layer will use the terminal engine (forward contract)

S1 must not just *render* terminals interactively — it must expose a **programmatic API**
(create / attach / write-stdin / stream-output / resize / kill / query-state). In S6 the app-native
agents call exactly these to:

1. **CEO** picks the next objective from goals + knowledge graph (+ future S8 metrics).
2. **PM** (TDD+DDD) does gap analysis (current vs 100%-done), writes specs, decomposes into tasks,
   assigns each to an engineering specialist (ML / data-sci / security / backend / frontend / testing).
3. Each specialist **spawns / drives a terminal worker** (claude-code, etc.), watches its
   OSC-133-derived lifecycle, and detects when the worker is **waiting for input** (stuck).
4. Unresolvable questions bubble PM → CEO, which batches them and **validates with the human**,
   then feeds answers back to the workers.
5. Kanban (S5) reflects the live state; Stats (S7) records it.

This is why S1's **status detection** (waiting-for-input) and **programmatic control** are
first-class in the S0+S1 spec even though the agents come later.

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
