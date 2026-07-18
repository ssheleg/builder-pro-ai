# S-UXR — UX-scenario base + testing loop + UI redesign — Design

**Date:** 2026-07-18
**Status:** Approved (design direction A "Calm Control Room"; audit-first sequencing; tokens+primitives depth)
**Target version:** `[0.9.0]`
**Supersedes for QA:** the Russian first-session catalog `docs/qa/ux-first-session-scenarios.md` +
`docs/qa/ux-investigation-report.md` (kept as frozen history in the English-gate allowlist; the new
English base below replaces them going forward).

## 0. Goal & order

Two deliverables, executed in this order:

1. **Part A — UX-scenario base + maintenance rule + testing loop.** A single maintained English
   catalog of every user-facing scenario (all features / buttons / states / worked-or-not / errors
   / results), a rule that every change updates it, and a repeatable batched loop that audits the
   code against each scenario into a results file. This is the durable regression foundation and it
   runs FIRST — its findings tell the redesign exactly what is broken/missing.
2. **Part B — UI redesign** (design direction A: neutral slate, one calm-blue accent, mono numbers,
   metrics-forward, light+dark). A design-token layer + a small primitives kit, then a
   behavior-preserving refactor of each view's inline styles onto tokens/primitives. The 870 TS
   tests stay green (visual refactor, not behavior change).

After Part B, the Part A loop re-runs against the new UI (the base is the regression harness).

## Global constraints

- **English only** — the `scripts/check-english.sh` no-Cyrillic gate stays green (the new catalog +
  results files are English; the old RU catalogs stay in the allowlist as frozen history).
- **Production-grade** — TDD for primitives + the maintenance-rule script; behavior-preserving
  refactors verified by the existing suite; docs updated in the same change.
- **Wire frozen** — this slice adds NO orchd/sessiond wire verbs and NO schema migration. Pure
  frontend + docs + QA tooling. `bpa-orchd` stays `[1,1]`.
- **No new heavy deps** — tokens are plain CSS variables; primitives are hand-rolled React (no
  Tailwind/styled-components/icon-lib/chart-lib). Sparklines are inline SVG. (An icon set may be
  added ONLY as inlined SVG components under `src/ui/icons/`, no runtime dep.)
- Per-view redesign keeps every existing `data-testid` and the `orchdDown`-disabled discipline
  intact, so vitest stays green.

---

## Part A — UX-scenario base + testing loop

### A1 — The catalog: `docs/qa/ux-scenarios.md`

One English file, the single source of truth for scenarios. Header carries `synced @ <commit7>`.
Organized by **epic**; each scenario is a table row with these exact columns:

| Col | Meaning |
|---|---|
| **ID** | `<EPIC>-NN`, e.g. `ON-01` (epics below). Stable; never renumber, append. |
| **Entry** | Where it starts (view/control). |
| **Steps** | The user actions. |
| **Expected** | Result + exactly what the user should SEE. |
| **States** | Which of: worked / failed / empty / loading / disabled apply, and what each renders. |
| **Errors/edge** | The error + how it must be handled (toast/inline/banner), and edge cases. |
| **Checks** | What the auditor verifies in code (control exists, handler fires, state rendered, error handled, no dead end). |

**Epics (the ID prefixes) — full coverage:**

- `ON` — first-run / onboarding (zero projects/workspaces, empty app, first CTA).
- `DA` — daemon lifecycle (bring-up, sessiond/orchd down, incompatible→upgrade, storage-degraded).
- `WS` — workspaces (create, add/remove root, LastRoot guard).
- `TE` — terminals (create, attach, close, rehydrate-inactive, command strip).
- `FI` — files (tree, preview, create/rename/delete, watch, terminal file-links).
- `PR` — projects (create, archive/un-archive, workspaces manage, export/import, **add another project**).
- `GO` — goals + metric_refs editor.
- `ID` — ideas (⌘K capture, inbox, lifecycle, link-to-project, spawn-project-from-idea).
- `RE` — research (run dialog, pane, artifact, failure/degraded, boot-reconcile).
- `IN` — insights (form-insight, fit-verdict, accept, to-backlog, archive).
- `TA` — tasks (create, status, rank, delete, subtasks).
- `GR` — graph (add node/edge, rename, edge-kind, drag, search, ghost/orphan).
- `EX` — extensions (servers add/consent/fingerprint, tools enable/invoke, connectors OAuth/apikey,
  skills, policies, artifacts, audit).
- `DG` — degradation (orchdDown gating everywhere, reconnect rehydration, toast queue, honest
  empty/loading/failed states).
- `XC` — cross-cutting (double-submit guard, theme toggle, keyboard, large inputs, second-instance).

Each row's "States" and "Errors/edge" MUST enumerate the worked / not-worked / error / result
matrix the owner asked for — a scenario is incomplete without them.

### A2 — The maintenance rule

- `CONTRIBUTING.md` gains a **"UX scenarios"** rule: *any change that adds/changes/removes a
  user-facing control, view, state, or a wire verb the UI consumes MUST update
  `docs/qa/ux-scenarios.md` in the same change (add/edit the affected rows + bump the `synced @`
  header).* Part of Definition of Done.
- **Advisory gate** `scripts/check-ux-scenarios.sh` (new): compares the range being gated; if any
  `src/components/**`, `src/App.tsx`, or `src/store/**` file changed but `docs/qa/ux-scenarios.md`
  did not, it prints a loud WARNING naming the changed files and reminding the author to update the
  catalog. It **warns, does not fail** (a hard block would create false friction on pure-logic
  changes) — wired as a non-fatal informational step in `scripts/final-suite.sh` and a
  `continue-on-error` CI step. TDD: a small test proving it warns on component-without-catalog and
  is silent when both changed (or neither).

### A3 — The testing loop → `docs/qa/ux-test-results.md`

The results file. A checklist (all scenario IDs, ⬜→✅/🟡/🔴/📄) + a verdict registry + one section
per scenario using this template:

```
### <ID> — <name>
- Verdict: ✅ OK / 🟡 UX-GAP / 🔴 BUG / 📄 DOC-GAP  (+ severity Critical/Important/Minor for 🟡/🔴)
- Traced: control (file:line) → handler → ipc → state render.
- Error handling: present/absent, honest or swallowed — proof.
- What the user sees: actual on-screen behavior.
- Delta from Expected: (if any).
- Action: none / BL-xx filed / fix in <slice> / doc fix.
```

**Protocol:** scenarios are audited in **batches by epic** (one auditor subagent per batch). Each
auditor traces every scenario in its batch against the code (control exists? handler fires? states
rendered? errors handled? no dead ends?), assigns a verdict + evidence, and its results are merged
into `ux-test-results.md` with the checklist ticked. Confirmed 🔴 → `docs/backlog.md`. The loop runs
until every ⬜ is resolved; a final **overall verdict** section summarizes counts + the must-fix
list. This runs twice: once on the CURRENT UI (drives Part B), once on the REDESIGNED UI (regression).

---

## Part B — UI redesign (direction A "Calm Control Room")

### B1 — Design tokens: `src/theme/tokens.css` + `src/theme/theme.ts`

`tokens.css` defines CSS custom properties on `:root` (light) and `:root[data-theme="dark"]`
(dark). Locked palette (direction A, approved):

**Light** — `--bg:#f7f8fa; --panel:#ffffff; --panel-2:#f7f8fa; --ink:#1a1f2b; --muted:#5b6472;
--border:#e6e9ef; --border-strong:#d7dce4; --accent:#2f6feb; --accent-weak:#eaf0fe; --ok:#1f9d55;
--ok-weak:#e4f5ec; --warn:#a9720a; --warn-weak:#fbecd2; --danger:#d23b3b; --danger-weak:#fbe6e6;
--info:#2f6feb`.

**Dark** — `--bg:#0f1218; --panel:#161b24; --panel-2:#1b212c; --ink:#e8ecf3; --muted:#8a93a6;
--border:#232a36; --border-strong:#2c3441; --accent:#4b8bff; --accent-weak:#1b2740; --ok:#48c98a;
--ok-weak:#123425; --warn:#e0a83a; --warn-weak:#3a2c0e; --danger:#f06b6b; --danger-weak:#3a1e1e;
--info:#4b8bff`.

**Non-color tokens (theme-independent):** space `--sp-1:4px --sp-2:8px --sp-3:12px --sp-4:16px
--sp-5:24px --sp-6:32px`; radius `--r-sm:5px --r-md:7px --r-lg:10px`; type
`--font-ui:-apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,sans-serif;
--font-mono:ui-monospace,"SF Mono",Menlo,monospace`; sizes `--fs-xs:11px --fs-sm:12px --fs-md:13px
--fs-lg:15px --fs-xl:19px --fs-2xl:26px`; `--shadow-1:0 1px 2px rgba(0,0,0,.06)` (dark:
`0 1px 2px rgba(0,0,0,.4)`).

`theme.ts`: `type Theme = 'light'|'dark'|'system'`; `applyTheme(t)` sets `data-theme` on the root
(resolving `system` via `matchMedia('(prefers-color-scheme: dark)')` + a listener); default
`system`; persisted via `tauri-plugin-store` key `theme`; a store slice `theme` + `setTheme`. A
toggle control lives in the sidebar footer.

**Status→token mapping** (used by `Badge` + everywhere status shows): `running`→info,
`waiting`→warn, `done`/`accepted`/`shipped`→ok, `failed`→danger, `pending`/`archived`/`new`→muted,
`interrupted`→danger. Codified in `theme.ts` as `statusTone(status): Tone`.

### B2 — Primitives: `src/ui/`

Small, tested React components consuming ONLY tokens (no inline hex). Each with a co-located test.

- `Panel({title?, actions?, padded=true, children})` — bordered surface (`--panel`, `--border`,
  `--r-lg`), optional header row.
- `Stat({label, value, unit?, tone='ink', delta?, spark?})` — metric tile: uppercase muted label,
  big `--font-mono --fs-2xl` value (`font-variant-numeric:tabular-nums`), optional unit/delta/`<Sparkline>`.
- `Sparkline({points:number[], tone='accent', width=52, height=16})` — inline SVG polyline.
- `Button({variant='primary'|'ghost'|'danger', size='md'|'sm', loading?, disabled, onClick, children})`.
- `Badge({tone|status, children})` — pill, weak-bg + strong-fg from the tone.
- `Field({label, hint?, error?, children})`, `Input`, `TextArea`, `Select` — token-styled form atoms.
- `EmptyState({title, hint?, action?})` — one calm line + one action (kills the "looks like debug
  output" empty screens).
- `Dialog({open, title, onClose, footer?, children})` — modal shell (overlay + panel + Esc/close),
  reused by every existing dialog.
- `Toast` restyle (keep the FIFO queue behavior from `[0.8.0]`, restyle onto tokens + tones).

### B3 — Per-view application (behavior-preserving)

Refactor each view's inline styles to tokens + primitives. **No behavior change** — same handlers,
same ipc, same `data-testid`s, same `orchdDown` gating, so vitest stays green; visual only. Group
into slices: (1) app shell + sidebar + theme toggle + tokens/primitives foundation; (2) Home +
HomeGoals (metric tiles via `Stat`, attention queue); (3) workspace/terminal/files chrome +
CommandStrip; (4) ProjectPanel shell + Overview (project stats as `Stat`s) + Goals/Tasks; (5)
Ideas/Research/Insights (the idea flow dialogs onto `Dialog`); (6) Graph canvas chrome; (7)
Extensions tabs + dialogs. Each slice: refactor → `npx vitest run` + `npx tsc --noEmit` green +
`check-english` green → commit.

### B4 — Empty/loading/density polish

Every list/panel gets a real `EmptyState` (no bare headers), a loading affordance distinct from
empty, and honest information density — prioritized by the Part-A audit's findings (which screens
read worst). This is where "too much test data / looks bad" is concretely fixed.

---

## Testing

- Primitives: unit tests (render + variant + token-class presence + a11y focus-visible).
- `theme.ts`: tests for resolve-system, toggle, persist, `statusTone` mapping.
- `check-ux-scenarios.sh`: a test for warn/silent behavior.
- Per-view refactors: the existing 870 vitest tests must stay green (regression guarantee).
- The A3 loop is the end-to-end UX verification, run before AND after the redesign.

## Release

`[0.9.0]`: CHANGELOG + roadmap (S-UXR shipped) + README (screenshot can be re-captured from the new
UI; theme mention) + traceability + the two QA docs. Then the existing **manual** release workflow
(`workflow_dispatch`) builds the signed+notarized universal `.dmg` into a draft Release for the
owner to publish (publishing stays the owner's action).

## Self-review notes

- No placeholders; token values, primitive APIs, epic list, file paths, and the results template
  are all concrete.
- Wire/schema untouched → no migration risk; consistent with the append-only discipline.
- The maintenance rule is advisory (warn) by deliberate choice to avoid false friction — stated.
- Behavior-preserving refactor keeps the 870-test regression guarantee — stated per slice.
