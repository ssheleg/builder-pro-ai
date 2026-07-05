# Contributing to Builder Pro AI

## Dev setup

- **Rust:** the toolchain is pinned by [`rust-toolchain.toml`](rust-toolchain.toml) (stable 1.92 +
  rustfmt + clippy) — rustup honors it automatically; no manual install step.
- **Node:** `>= 24` (enforced via `package.json` `engines`). Then `npm ci`.
- **Daemon dev build:** `cargo build -p bpa-sessiond` (dev mode and the e2e harness spawn
  `target/debug/bpa-sessiond`).

## Gates

One command gates the whole Definition of Done:

```bash
bash scripts/final-suite.sh
```

Its 8 stages, in order: Rust workspace tests · clippy `-D warnings` · `cargo fmt --check` ·
TypeScript tests (vitest) · `tsc --noEmit` · ts-rs type parity (regenerate + diff
`src/ipc/types.ts`) · daemon coverage gate (≥ 80 % line coverage on `bpa-sessiond`) ·
e2e survive-restart.

CI ([`.github/workflows/ci.yml`](.github/workflows/ci.yml)) runs the same set on every push/PR.
**Local and CI gates must never diverge** — if you change one, change both in the same commit.

## TDD + Definition of Done

- Failing test first (RED), minimal implementation (GREEN), then refactor. A change without a
  test that would have failed before it is not done.
- Every external interaction (network / DB / IPC / files) ships with error handling and **honest
  degradation** — never fake success, never swallow an error silently.
- Docs are updated **in the same change** that alters behavior they describe.
- **Backlog rule (normative):**

  > Any accepted-deferred finding MUST land in [`docs/backlog.md`](docs/backlog.md) in the same
  > change that defers it. Gitignored ledgers (`.superpowers/…`) are working notes, never the
  > record.

Open deferred work lives in [`docs/backlog.md`](docs/backlog.md) — check it before starting a
slice; your slice may own queued items.

## Planning cycle

Non-trivial work follows the full cycle: brainstorm → design spec committed to
`docs/superpowers/specs/` → implementation plan in `docs/superpowers/plans/` → subagent-driven
execution with two-stage review per task (spec compliance, then code quality) → whole-branch
final review → merge. Specs lock shared contracts (types, wire shapes, file ownership) so a
zero-context implementer can execute any single task.

## Commit conventions

Conventional commits (`feat:`, `fix:`, `docs:`, `chore:`, `test:`, `style:`, `ci:`). Agent-authored
commits carry the agent trailer line used throughout this repo's history.

## Protocol change rules (Hop-B wire)

- **Append-only wire discipline:** enum variant order is frozen; new requests/pushes are appended;
  fields are added additively. Every protocol change ships a cross-version decode test.
- **Locked contract:** DO NOT re-derive Serialize/Deserialize on SessionLifecycle or
  TerminalEvent, and DO NOT add new serde-tagged enums to the Hop-B protocol, until protocol v2
  replaces the codec.
- Background: the built wire uses bincode 1.3.3 with a hand-written dual-codec bridge for tagged
  enums — see the amended codec section of
  [`docs/superpowers/specs/2026-07-01-builderpro-s0s1-foundation-terminal-design.md`](docs/superpowers/specs/2026-07-01-builderpro-s0s1-foundation-terminal-design.md)
  (§3) and [`docs/architecture.md`](docs/architecture.md).
