# Builder Pro AI — Design System v2 «Soft Control Room»

Written 2026-07-20 (spec: `docs/superpowers/specs/2026-07-20-soft-control-room-design.md`).
**Binds every feature designed after this date**; supersedes v1 «Calm Control Room»
(2026-07-06). Extends [`frontend-conventions.md`](frontend-conventions.md) (store/events/
testing architecture); this doc owns the VISUAL and UX language. Tokens live in
`src/ui/tokens.css` (CSS-variable palette, light **and** dark), `src/ui/theme.ts`
(`Theme`/`Tone` types, theme resolve/apply, `statusTone()`), and the token-only building
blocks in `src/ui/primitives.tsx` (`Panel`/`Stat`/`Sparkline`/`Badge`/`Button`/`Field`/
`EmptyState`/`Dialog`/`SegmentedPill`/`Heatmap`). This doc is their contract.

## 1. Vision & principles (in priority order)

Warm paper neutrality fused with control-room density: the app reads as a calm printed
dashboard, not an OS window. **Depth = fill, not lines:** three fill steps
(`--bg` → `--panel` → `--panel-2`) replace borders and shadows entirely.

The core structural rule: **container = fill, rows inside = hairline.** Any surface that
groups content is a borderless fill one step deeper than its parent; repeated dense rows
inside it (table rows, tree rows, list items) separate with the near-invisible `--hairline`.
Outer borders on cards/panels/inputs are a design defect.

1. **Density with air.** Compact layouts — the owner runs 5–6 projects on one screen — but
   never cramped: whitespace from the spacing scale, not from removing information.
2. **Glanceability beats completeness.** Every surface answers its question in one glance
   (state → color chip; trend → sparkline; density → heatmap; delta → one line). Detail is
   one drill-down away, never on the first screen.
3. **Honest state, always.** No fake "connected", no optimistic spinners, no silent
   failures. Degraded = visibly degraded (dimmed pane, banner, chip).
4. **The system pulls, the owner never polls.** Attention-worthy events surface as deltas
   and inbox items. No badge without an action.
5. **Minimal chrome.** No decorative containers, no gradients, no shadows-as-decoration,
   no icon noise. Two color voices only (accent = action, blue = data); if an element
   doesn't inform or act, it doesn't exist.
6. **Keyboard-first.** Every primary action reachable via keyboard (⌘K command palette is
   the front door); mouse is the alternative, not the requirement.

## 2. Color (tokens — `src/ui/tokens.css` is the source of truth)

Consume tokens as `var(--…)` — never a raw hex. The single exception: the terminal pane
ground `#010409`, intentionally theme-independent (a window into the machine).

**Surfaces & structure**

| Token | Light | Dark | Use |
|---|---|---|---|
| `--bg` | `#faf9f5` | `#1b1a18` | app ground (ivory / warm charcoal) |
| `--panel` | `#f1efe9` | `#242220` | cards, bars, dialogs — one fill step deeper than bg |
| `--panel-2` | `#e7e4dc` | `#2e2b28` | nested tiles, insets, inputs, table headers |
| `--ink` | `#1f1e1c` | `#ece9e2` | primary text |
| `--muted` | `#625e55` | `#a39d92` | secondary text, labels, metadata |
| `--hairline` | `#dcd9d1` | `#3a3733` | 1px separators INSIDE dense containers only |
| `--border` / `--border-strong` | = hairline / `#cfccc2` | = hairline / `#46423d` | **deprecated aliases** — migration bridge for views not yet on the fill model; new code never uses them |
| `--shadow-1` | subtle warm | subtle | true overlays ONLY (dialog, toast, popover) |

**Two color voices**

| Token | Light | Dark | Use |
|---|---|---|---|
| `--accent` (`--accent-weak`, `--on-accent`) | `#944527` (`#f3ddd2`, `#ffffff`) | `#e0805c` (`#3b271e`, `#201310`) | THE action voice: primary buttons, active states, focus ring, logo. Never for data. |
| `--data` (`--data-weak`) | `#2b66d8` (`#eaf0fe`) | `#6f9dff` (`#1e2a44`) | THE data voice: charts, sparklines, heatmap. Never for actions. |

**Status / semantic language (locked product vocabulary — reuse everywhere).**
`statusTone(status)` in `theme.ts` maps an entity status to a tone; `Badge` renders it.
Each tone has a `-weak` background for tinted fills (badges, banners).

| Tone | Light | Dark | Meaning — everywhere in the product |
|---|---|---|---|
| `info` (`--info`) blue | `#2b66d8` | `#6f9dff` | running / working |
| `ok` (`--ok`) green | `#157239` | `#48c98a` | done / accepted / shipped / healthy |
| `warn` (`--warn`) amber | `#8a5d08` | `#e0a83a` | needs a human / waiting / gate |
| `danger` (`--danger`) red | `#b83232` | `#f06b6b` | failed / interrupted / error / incident |
| `muted` (`--muted`) grey | `#625e55` | `#a39d92` | idle / at prompt / nothing happening |

Rules: semantic colors are STATE ONLY — never decoration; amber is reserved for «a human
is needed»; a third color voice is a design defect. Every text pairing (each tone on its
`-weak` and on `--panel`; ink/muted on all surfaces; on-accent on accent) is enforced ≥
WCAG AA 4.5:1 by `src/ui/contrast.test.ts` — new tokens land in `tokens.css` + that test
first, this doc second, usage third.

## 3. Typography (three faces, strict roles)

- **Display: Space Grotesk** (`--font-display`, weights 500/700, bundled via
  `@fontsource/space-grotesk` in `main.tsx` — offline-safe, falls back to the system stack
  if the asset fails). ONLY page titles (28px `--fs-3xl`), section headings, and large
  stat values. Body text in Space Grotesk is a defect.
- **UI: system stack** (`--font-ui`): all controls, body, labels. 400 body · 600 emphasis.
- **Data: mono** (`--font-mono` + `font-variant-numeric: tabular-nums`): ALL data — ids,
  metrics, timestamps, logs, terminal, counters, chips.
- **Scale:** 11px chips/meta · 12px labels (uppercase + `letter-spacing: .1em`) · 13px
  body/dense-UI · 15px card titles · 19–26px section headings · 28px page titles.
  Line-height 1.4–1.55 body, 1.1–1.2 headings. Running text ≤ 65ch; headings
  `text-wrap: balance`.

## 4. Surface, space, shape

- **4px base grid.** Steps: 4 / 8 / 12 / 16 / 24 / 32. Padding defaults: chips 2×8, dense
  rows 8×12, cards 16–20, stat tiles 12×16.
- **Radii:** 10px (`--r-sm`) controls/inputs · 14px (`--r-md`) cards/tiles · 18px
  (`--r-lg`) page-level containers/dialogs · 999px chips/pills. Nothing else.
- **Elevation = fill step, not border, not shadow.** `--shadow-1` only on true overlays.
- **Layout primitives:** flex/grid + `gap` only (no margin stacks). Wide content scrolls
  inside its own `overflow-x: auto` container.
- **Density modes:** compact only (YAGNI).

## 5. Component language (canonical atoms — build ONCE, reuse everywhere)

| Atom | Contract |
|---|---|
| **Panel** | `--panel` fill, radius `--r-md`, no border/shadow; title row 13px/600 separated by `--hairline` |
| **Stat tile** (`Stat`) | the canonical overview atom: `--panel-2` fill, radius `--r-md`; muted 11px uppercase label above a bold display-face value (tabular-nums); optional unit/delta/spark; default spark speaks the data voice |
| **Sparkline** | inline SVG line in a tone color (default data-blue); extremes may use semantic tones; no axes |
| **Heatmap** | 5-level blue density grid: level 0 = `--panel-2`, 1–3 = `color-mix` of `--data` at 25/50/75%, 4 = `--data`; 12px cells, 4px gap, radius 4; `role="img"` + label; guarded against empty/zero/negative input |
| **SegmentedPill** | view switcher («Overview | Models», «All | 30d | 7d»): group on `--panel-2` radius 999, active segment = `--panel` fill + `--ink`, inactive = transparent + `--muted`; radiogroup semantics + arrow keys |
| **Status dot** | 7–8px circle, status tone; the smallest unit of "what state is this" |
| **Chip** | mono 11px, radius 999, `--panel-2` fill (no border); counts tabular-nums. Debt BL-41 (three inline implementations) unchanged — this contract is the extraction target |
| **Card** | = Panel; title row = 13–15px/600 + right-aligned chip |
| **Delta line** | one line under a card title: «what changed since you last looked»; bold the verb |
| **Agent row** | status dot + mono agent name + plain-text current action |
| **Inbox item** | amber left-edge, question text, action buttons inline; ONE inbox pattern app-wide |
| **Banner** | tone `-weak` fill, radius `--r-sm`, no border, 3px left-edge bar in the tone color, body `--ink`, inline secondary-button actions. Info banners = `--info-weak`; amber only for «needs you» (daemon/orchd/storage/file-state banners are info/danger, not amber) |
| **Nav item** | selected = `--panel-2` fill pill (radius `--r-sm`) + `--ink`; unselected = transparent + `--muted`; hover = half-step fill (`color-mix(in srgb, var(--panel-2) 50%, transparent)`). Accent is NOT used for selection |
| **Progress bar** | 4–5px height, `--panel-2` track, `--accent` fill; no percentage text unless asked |
| **Button** | primary = `--accent` fill + `--on-accent` (one per view max); secondary/ghost = `--panel-2` fill (never an outline); destructive = `--danger-weak` fill + `--danger` text with confirm; radius `--r-sm` |
| **Field / Input** | inputs on `--panel-2` fill, radius `--r-sm`, no border; focus = global accent ring; error line `role="alert"` in `--danger` |
| **Empty state** | one dim sentence + one primary action; no illustrations |
| **Dialog** | `--panel` fill, radius `--r-lg`, the ONE `--shadow-1`; amber top-edge marks «a human is needed» (never amber fill elsewhere); `role="dialog"` + `aria-modal` + labelled title; Escape = cancel; in-dialog failure = `role="alert"` red line below body (dialog stays open for retry) |
| **Toast** | queue-of-ONE (`showToast` replaces, never queues), bottom-anchored, `role="alert"`, red left-edge default (exists to surface failures honestly); auto-dismiss ~4s; `--panel` fill + `--shadow-1` |
| **Step card** (flows) | kind label (mono uppercase) + name + tool-binding chip (agent=accent, terminal=green, MCP=purple `#bc8cff`) |
| **Terminal pane** | `#010409` ground in BOTH themes, xterm defaults, mono 11–12px chrome |
| **Tree row / File tree / Preview pane / Command strip / Lifecycle chip / Policy form / Project group row / Quick-capture overlay / Graph node card / Graph toolbar** | behavior contracts unchanged from v1 (see git history of this doc for the full text); visual vocabulary migrates to fill/hairline: row separators = `--hairline`, dimming = `--muted`, error edges = `--danger`, match ring = 2px `--accent` |

## 6. UX laws

1. **Delta-first:** every returning view leads with what changed, then current state.
2. **One inbox:** all human-needed decisions land in THE inbox. Badge count = actionable
   items only.
3. **Drill-down, never pogo:** grid → project → artifact in place; back is one gesture.
4. **Every async action shows its truth:** started → running (live status, not spinners) →
   result (delta line). Failures use the error-surfacing contract — a toast with the
   mapped human message, never console-only.
5. **Observability is a first-class screen:** flow-run history renders with the same care
   as the home screen.
6. **Quick capture from anywhere:** ⌘K palette — idea capture, project jump, run workflow.
7. **Reduced motion respected;** animation only where it carries meaning, 150–200ms
   ease-out, nothing looping.
8. **Focus visible** on every interactive element (2px accent outline offset 2px, global
   `:focus-visible` rule in `tokens.css`); contrast ≥ WCAG AA enforced by test.

## 7. Writing rules (copy is design)

- Name things by what the owner recognizes: «Hot questions», not «escalation queue». UI
  copy language: English everywhere (O-2).
- A control says what happens («Deploy», toast «Deployed»). Errors say what broke and what
  to do next — no apologies, no vagueness.
- Numbers carry units and context («$1.87 · 42 min», «214 users affected»).

## 8. Process rule

Every new feature spec includes a «Design» section that references THIS doc and lists:
which canonical atoms it reuses, which new atoms it introduces (new atom = this doc gets a
row in the same change), and its keyboard path. A feature that invents a parallel visual
language fails review.

## 9. Migration status (v1 → v2)

Shipped on the new language: `tokens.css`, `theme.ts`, all `primitives.tsx` atoms.
Everything in `src/components/` and `src/App.tsx` still renders legacy `--border`
semantics (gracefully — the alias resolves to hairline values) until migrated view-by-view
in the next cycle. Migration DoD per view: no `--border`/`--border-strong` reads, outer
borders removed in favor of fill steps, dense rows on `--hairline`, view switchers on
`SegmentedPill`.
