# Contributing to Builder Pro AI

## Language: English only

All code, comments, identifiers, UI copy, commit messages, and docs are written in **English** —
no other natural language anywhere in the tree. The `scripts/check-english.sh` gate machine-enforces
this (it runs first in `scripts/final-suite.sh` and in CI): it fails on any Cyrillic (`[Ѐ-ӿ]`)
outside a single closed allowlist. The **only** exception is the pre-existing frozen historical
records — the superseded per-slice specs, plans, QA investigations, and product-vision research
under `docs/superpowers/{specs,plans,research}` and `docs/qa/` (listed exactly in
[`scripts/english-allowlist.txt`](scripts/english-allowlist.txt)) — which stay verbatim because
retroactively rewriting them would falsify history. Every **new** file, anywhere, must be English:
the allowlist is a closed list of exact paths, so anything new is enforced automatically.

## Docs & the README

Docs are part of Definition of Done — update them in the same change that changes reality, never as
a follow-up. The [`README.md`](README.md) is the project's front door and has its own
[**Maintaining this README**](README.md#maintaining-this-readme) rules (truth over polish, move
shipped slices out of *Planned*, measure numbers instead of guessing, keep the quick start runnable
on a clean checkout). When a slice ships, its PR updates the README roadmap + version, the
[`CHANGELOG.md`](CHANGELOG.md), the roadmap in the platform overview, and
[`docs/traceability.md`](docs/traceability.md) alongside the code.

## UX scenarios

[`docs/qa/ux-scenarios.md`](docs/qa/ux-scenarios.md) is the maintained catalog of every user-facing
scenario (all features / buttons / states / worked-or-not / errors / results) and the base for UX
testing. **Rule:** any change that adds, changes, or removes a user-facing control, view, or state
— or a wire verb the UI consumes — MUST update `docs/qa/ux-scenarios.md` in the **same change**
(add/edit the affected rows + bump the `synced @ <commit>` header). This is part of Definition of
Done. The advisory `scripts/check-ux-scenarios.sh` (a non-blocking stage in `final-suite.sh` and a
`continue-on-error` CI step) warns when `src/components/**`, `src/App.tsx`, or `src/store/**`
changed without the catalog — it reminds, it never fails the build. UX-test findings land in
[`docs/qa/ux-test-results.md`](docs/qa/ux-test-results.md).

## Dev setup

- **Rust:** the toolchain is pinned by [`rust-toolchain.toml`](rust-toolchain.toml) (stable 1.92 +
  rustfmt + clippy) — rustup honors it automatically; no manual install step.
- **Node:** `>= 24` (enforced via `package.json` `engines`). Then `npm ci`.
- **Daemon dev build + sidecar staging** (required once per fresh checkout — the Tauri build
  script checks the sidecar exists):
  ```sh
  cargo build -p bpa-sessiond
  mkdir -p src-tauri/binaries
  cp target/debug/bpa-sessiond "src-tauri/binaries/bpa-sessiond-$(rustc -vV | sed -n 's/host: //p')"
  ```
  (dev mode and the e2e harness spawn `target/debug/bpa-sessiond`).

## Gates

One command gates the whole Definition of Done:

```bash
bash scripts/final-suite.sh
```

Its 10 stages, in order: English-only gate (`scripts/check-english.sh`) · Rust workspace tests ·
clippy `-D warnings` · `cargo fmt --check` · TypeScript tests (vitest) · `tsc --noEmit` · ts-rs
type parity (regenerate + diff `src/ipc/types.ts` and `src/ipc/orchd-types.ts`) · daemon coverage
gate (≥ 80 % line coverage on `bpa-sessiond` and `bpa-orchd`) · e2e survive-restart · e2e orchd
survive-restart + export/import round-trip.

CI ([`.github/workflows/ci.yml`](.github/workflows/ci.yml)) runs the same set on every push/PR.
**Local and CI gates must never diverge** — if you change one, change both in the same commit.

## TDD + Definition of Done

- Failing test first (RED), minimal implementation (GREEN), then refactor. A change without a
  test that would have failed before it is not done.
- Every external interaction (network / DB / IPC / files) ships with error handling and **honest
  degradation** — never fake success, never swallow an error silently.
- Docs are updated **in the same change** that alters behavior they describe.
- **Design-section rule (normative):** every feature spec includes a `Design` section referencing
  [`docs/design-system.md`](docs/design-system.md) that lists which canonical atoms it reuses,
  which new atoms it introduces (a new atom = `design-system.md` gets a row in the same change),
  and the feature's keyboard path. A feature that invents a parallel visual language fails review.
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

## Meta-process

For the platform AND for every project this platform manages (vision v4 §8):

1. The **end goal is always visible and editable**; editing it triggers re-planning.
2. A **live step-plan to that goal** is always kept, re-actualized whenever the goal changes.
3. **Architecture and data structures are designed first**; then a minimum is defined and
   extended constructor-style — cube by cube, additive, never a rebuild (the additive-only schema
   lock in the overview §2). New cubes slot in; existing cubes are not reassembled.

The CEO/PM agents operate managed projects by this same method.

## Commit conventions

Conventional commits (`feat:`, `fix:`, `docs:`, `chore:`, `test:`, `style:`, `ci:`). Agent-authored
commits carry the agent trailer line used throughout this repo's history.

## Protocol change rules (Hop-B wire)

- **Append-only wire discipline:** enum variant order is frozen; new requests/pushes are appended;
  fields are added additively. Every protocol change ships a cross-version decode test. (Still true
  — see the Pv2.1 reserved-batch amendment in
  [`docs/superpowers/specs/2026-07-06-protocol-v2-design.md`](docs/superpowers/specs/2026-07-06-protocol-v2-design.md)
  §"Vision v2–v4 amendments": future request variants are named and order-reserved now, implemented
  later, so indices are never reused.)
- Hop-B codec is CBOR (`ciborium`); tagged enums (`SessionLifecycle`, `TerminalEvent`) are plain
  `#[derive(Serialize, Deserialize)]`. The v1 dual-codec bridge (bincode 1.3.3 + the
  `is_human_readable()`-branching hand-written impls) was retired in Pv2 (`[0.2.0]`) — see the
  amended codec section of
  [`docs/superpowers/specs/2026-07-01-builderpro-s0s1-foundation-terminal-design.md`](docs/superpowers/specs/2026-07-01-builderpro-s0s1-foundation-terminal-design.md)
  (§3) and [`docs/architecture.md`](docs/architecture.md).
