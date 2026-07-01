# Builder Pro AI

A lightweight macOS desktop workspace for **orchestrating AI coding agents** (claude-code,
hermes, opencode, kilo, …) that do their work through terminals — plus app-native meta-agents
(a CEO strategist, a TDD/DDD project manager, and engineering specialists) that decide *what*
to build, run the plan, drive the terminals, and escalate only what they can't resolve.

Built with **Tauri 2** (Rust core + React/TypeScript UI). Ships as a universal macOS binary.

## Status

**Planning.** No application code yet — the project is being built in production-grade vertical
slices, each with its own spec → plan → implementation cycle. See
[`docs/superpowers/specs/`](docs/superpowers/specs/).

- **Platform overview & roadmap:** [`2026-07-01-builderpro-platform-overview.md`](docs/superpowers/specs/2026-07-01-builderpro-platform-overview.md)
- **Current slice — S0+S1 (Foundation + Terminal core):** [`2026-07-01-builderpro-s0s1-foundation-terminal-design.md`](docs/superpowers/specs/2026-07-01-builderpro-s0s1-foundation-terminal-design.md)

## Principles

- **Production-grade, no MVP half-states.** Each slice is finished: tests (TDD), error handling
  and honest degradation, structured logging, and docs are part of Definition of Done.
- **Max autonomy, min human-in-the-loop.** Humans set goals and quality; agents decide the rest.
- **Honest about boundaries.** The app never lies about session/agent state.
