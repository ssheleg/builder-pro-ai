# Builder Pro AI — original product vision (user's brain-dump, verbatim intent)

The FULL product the user described at project start. The S0–S8 overview + S0+S1 spec were derived
from this. Auditors: judge the docs against THIS — anything here that is missing, under-specified,
or architecturally unprepared-for in the docs is a finding.

## Product
A lightweight macOS desktop control panel (Intel + Apple Silicon) to orchestrate AI coding agents
that work through terminals. "Лёгкая панель, но функциональная" — minimal-but-functional UI,
maximum autonomy, minimum human-in-the-loop. Production-grade always — NO MVP half-states.

## Core concepts (all first-class features)
1. **Workspaces (projects)**: each workspace = a set of repos/folders for one project. Minimal
   grouping first, but the concept must scale.
2. **Live file explorer** per workspace: file tree of the project, live-updating.
3. **Knowledge graphs**: per-project knowledge graph/tree; graphs LINKED across projects
   (cross-project knowledge). Feeds agent decision-making.
4. **Goals/objectives** per project: the strategic layer agents work from.
5. **Kanban board**: task states, agents move cards as they work.
6. **CEO strategist agent** (the user's "SEO" slip = CEO): decides WHAT to build next from
   goals + knowledge graph (+ future: product metrics). Top of the agent hierarchy.
7. **PM agent**: works TDD + DDD; gap-analyzes "current state vs 100%-done"; writes specs;
   decomposes into tasks; drives engineering subagents; answers their questions; escalates only
   unanswerable questions up (PM → CEO → user).
8. **Engineering subagents**: ML-engineer, data-scientist, security, backend, frontend, testing —
   specialized roles driven by the PM.
9. **Terminals**: the execution substrate. Agents create/run/monitor terminal sessions; sessions
   survive app restart (detached, tmux-like); OSC-133 shell integration for status detection
   (running / waiting-for-input / done). The panel shows live terminals (xterm.js).
10. **External coding agents as terminal workers**: claude-code, hermes, opencode, kilo run INSIDE
    terminals as workers; the app-native agents (CEO/PM/eng) orchestrate them.
11. **App-native agents via provider abstraction**: CEO/PM/eng are OUR agents, powered by direct
    model calls through **OpenRouter-first** provider abstraction (also OpenAI, GLM). User can add
    CUSTOM app-native agents. ("Стратегики должны быть у нас самостоятельными... под капотом
    директ-модели через OpenRouter... сразу же закладывай туда")
12. **Stats/observability**: agent activity stats, terminal stats, project progress.

## Quality bar (user's global rules — bind the whole project)
- Production-grade from the first pass: no TODO-later, no stubs, no happy-path-only.
- TDD by default; tests are part of Definition of Done.
- Every external call (network/DB/API/files): error handling, retries where apt, HONEST degradation
  (never lie, never swallow errors silently).
- Structured logs, no secret leakage.
- Docs updated in the SAME change.
- Adjacent problems: fix now or file to backlog explicitly — never ignore.

## Locked platform decisions (already made with user)
- Tauri 2 (Rust core + React 19/Vite/TS webview), macOS universal binary.
- Terminal status via OSC-133 shell integration.
- Sessions detached + survive-restart (separate daemon `bpa-sessiond`, launchd-managed).
- Build order: S0 foundation + S1 terminal core FIRST (done), then further slices.
