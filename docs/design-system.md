# Builder Pro AI — Design System

Written 2026-07-06 (vision v4 §4: «super minimalist, light, modern, pleasant font,
compact»). **Binds every feature designed after this date.** Extends
[`frontend-conventions.md`](frontend-conventions.md) (store/events/testing architecture); this doc
owns the VISUAL and UX language. Since [0.9.0] (S-UXR) the tokens live in `src/ui/tokens.css` (the
CSS-variable palette, light **and** dark), `src/ui/theme.ts` (the `Theme`/`Tone` types, theme
resolve/apply, and `statusTone()`), and the reusable, token-only building blocks that consume them
in `src/ui/primitives.tsx` (`Panel`/`Stat`/`Sparkline`/`Badge`/`Button`/`Field`/`EmptyState`/
`Dialog`). This doc is their contract.

## 1. Design principles (in priority order)

1. **Density with air.** Compact layouts — the owner runs 5-6 projects on one screen — but never
   cramped: whitespace comes from a consistent spacing scale, not from removing information.
2. **Glanceability beats completeness.** Every surface answers its question in one glance
   (state → color chip; trend → sparkline; delta → one line). Detail is one drill-down away,
   never on the first screen.
3. **Honest state, always.** No fake "connected", no optimistic spinners, no silent failures.
   Degraded = visibly degraded (dimmed pane, banner, chip). This is the §13 degradation contract
   made visual.
4. **The system pulls, the owner never polls.** Attention-worthy events surface as deltas and
   inbox items — never as something the owner must go find. No badge without an action.
5. **Minimal chrome.** No decorative containers, no gradients, no shadows-as-decoration, no icon
   noise. One accent color. If an element doesn't inform or act, it doesn't exist.
6. **Keyboard-first.** Every primary action reachable via keyboard (⌘K command palette is the
   front door); mouse is the alternative, not the requirement.

## 2. Color (tokens — `src/ui/tokens.css` is the source of truth)

Light **and** dark since [0.9.0] (S-UXR). The palette is defined as CSS variables on `:root`
(light) and `:root[data-theme="dark"]` (dark) in `src/ui/tokens.css`; `src/ui/theme.ts` resolves
`light`/`dark`/`system` and applies `data-theme`. Consume tokens as `var(--…)` — never a raw hex.

**Surfaces & text**

| Token | Light | Dark | Use |
|---|---|---|---|
| `--bg` | `#f7f8fa` | `#0f1218` | app ground |
| `--panel` | `#ffffff` | `#161b24` | cards, bars, dialogs, inputs |
| `--panel-2` | `#f7f8fa` | `#1b212c` | inset / secondary / table headers |
| `--ink` | `#1a1f2b` | `#e8ecf3` | primary text |
| `--muted` | `#5b6472` | `#8a93a6` | secondary text, labels, metadata |
| `--border` / `--border-strong` | `#e6e9ef` / `#d7dce4` | `#232a36` / `#2c3441` | 1px lines / stronger edges |
| `--accent` (`--accent-weak`) | `#2f6feb` (`#eaf0fe`) | `#4b8bff` (`#1b2740`) | THE one accent + its tint |
| `--shadow-1` | subtle | subtle | the ONE elevation shadow (dialogs/panels); elevation is otherwise `--border` |

**Status / semantic language (locked product vocabulary — reuse everywhere, never re-invent).**
`statusTone(status)` in `theme.ts` maps an entity status to one of these tones; `Badge` and
`StatusDot` render it. Each semantic tone also has a `-weak` background token (`--ok-weak`,
`--warn-weak`, `--danger-weak`, `--info-weak`, `--accent-weak`) for tinted fills (badges, banners).

| Tone (token) | Meaning — everywhere in the product |
|---|---|
| `info` (`--info`) blue | running / working (`running`) |
| `ok` (`--ok`) green | done / accepted / shipped / healthy / success |
| `warn` (`--warn`) amber | needs a human / waiting / gate / warning (`waiting`) |
| `danger` (`--danger`) red | failed / interrupted / error / prod incident |
| `muted` (`--muted`) grey | idle / at prompt / nothing happening |

Rules: semantic colors are STATE ONLY — never decoration; amber is reserved for
"a human is needed" (the hot-questions color); one accent — a second accent hue is a design
defect. New tokens are added to `tokens.css` first, this doc second, usage third (via `var(--…)`
or a primitive). (Legacy token names used elsewhere in this doc — `bgElevated`, `text`, `textDim`,
`statusRunning`/`statusWaiting`/`statusExited`/`statusIdle`, `theme.shadow` — map to the new names
above: `--panel`, `--ink`, `--muted`, the `info`/`warn`/`danger`/`muted` tones, and `--shadow-1`.)

## 3. Typography

- **UI face:** system stack (`-apple-system, "SF Pro Display/Text"`). Native to macOS, modern,
  zero load cost. Weights: 400 body · 600 emphasis/labels · 700-800 headings only.
- **Data face:** `ui-monospace / SF Mono` — ALL data: ids, metrics, timestamps, logs, terminal
  content, counters, chips. `font-variant-numeric: tabular-nums` wherever digits align.
- **Scale (compact):** 11px chips/meta · 12px labels (uppercase + `letter-spacing: .1em`) ·
  13px body/dense-UI · 15px card titles · 20px section headings · 28px page titles. Line-height
  1.4-1.55 body, 1.1-1.2 headings.
- Running text ≤ 65ch. Headings get `text-wrap: balance`.

## 4. Space, layout, shape

- **4px base grid.** Spacing steps: 4 / 8 / 12 / 16 / 24 / 32. Component padding defaults:
  chips 2×8, dense rows 8×12, cards 12-16.
- **Radii:** 6px controls · 8-10px cards/windows · 999px chips. Nothing else.
- **Elevation = border, not shadow.** Layers are separated by `border` + `bgElevated`. Shadows
  only for true overlays (dialogs, popovers) — one soft shadow token.
- **Layout primitives:** flex/grid + `gap` only (no margin stacks — frontend-conventions rule).
  Wide content (tables, timelines, terminals) scrolls inside its own `overflow-x: auto` container.
- **Density modes:** default is compact; no "comfortable" mode in v0.x (YAGNI).

## 5. Component language (canonical atoms — build ONCE, reuse everywhere)

| Atom | Contract |
|---|---|
| **Status dot** | 7-8px circle, status token; the smallest unit of "what state is this" |
| **Chip** | mono 11px, 1px border, radius 999; optional status dot; counts use tabular-nums. **Debt (`docs/backlog.md` BL-41):** S2 shipped three independent inline implementations of this atom (`App.tsx`'s `statChipStyle`, `CommandStrip.tsx`'s `chipBaseStyle`, and the `MONO_FONT` constant redefined in 5 files) instead of one shared `Chip` component — the contract above is what any future extraction must match, not yet what the code reuses |
| **Card** | `bgElevated` + 1px border + radius 9; title row = 13-15px/600 + right-aligned chip |
| **Delta line** | one line under a card title: «what changed since you last looked»; bold the verb |
| **Agent row** | status dot + mono agent name + plain-text current action |
| **Inbox item** | amber left-edge, question text, action buttons inline; ONE inbox pattern app-wide |
| **Progress bar** | 4-5px height, `border` track, `accent` fill; no percentage text unless asked |
| **Sparkline** | bars/line, `border` color, highlight extremes with status colors; no axes |
| **Step card** (flows) | kind label (mono uppercase) + name + tool-binding chip (agent=accent, terminal=green, MCP=purple `#bc8cff`) |
| **Terminal pane** | `#010409` ground, xterm defaults, mono 11-12px chrome |
| **Empty state** | one dim sentence + one primary action; no illustrations |
| **Dialog / modal overlay** | fixed full-viewport dim backdrop + centered `bgElevated` card, 1px `border`, radius 10, the ONE soft-shadow token (`--shadow-1`, introduced by this atom — the first true overlay in the app); amber top-edge accent marks "a human is needed" (never amber fill decoration elsewhere); one primary (accent-fill) + one secondary (1px-border ghost) button; `role="dialog"` + `aria-modal` + labelled title, focus the primary button on open, `Escape` = the secondary (cancel) action, visible 2px accent focus ring; an in-dialog failure (an action the user took just failed) renders as a `role="alert"` line with `statusExited` (red) text + left-edge, reason + actionable hint, below the body copy — distinct from the amber top-edge, which marks the dialog's own trigger condition, not an in-flight action's outcome; the dialog stays open so the primary button can be retried |
| **Toast** | queue-of-ONE (`showToast` replaces, never queues — the "one inbox" spirit applied to transient notices), bottom-anchored fixed overlay, `role="alert"`, `statusExited` (red) left-edge accent as the DEFAULT (this atom exists to surface failures honestly, spec §7 error-surfacing contract — not console-only); auto-dismisses ~4s or `dismissToast()`; `--panel` + 1px `--border` + the shared `--shadow-1` token |
| **Tree row** | indent level × 16px, inline title edit, status select; strategic root pinned, no delete/move |
| **File tree** | lazy per-level fetch (`listDir` on expand, cached in the store until invalidated — re-expanding never re-fetches; a cache entry vanishing out from under a still-expanded dir auto-refetches with no click needed) + plain scroll-offset windowing over the flattened visible-node list (fixed row height, no virtualization dependency — stays smooth at 10k+ entries, DoD: <500 DOM rows rendered at any time); dirs-first sort then name; ignored entries hidden by default, dimmed (`textDim`) and shown only behind the "show ignored" toggle; per-row `⋯`/right-click context menu (new file/new folder → inline name input, rename, delete → Trash with `confirm`, reveal in Finder, open external); root nodes get New File/Folder only — no rename/delete on a workspace root (§9 "workspace deletion verb" out of scope) |
| **Preview pane** | read-only mono text under the tree, no editing, no syntax highlighting (YAGNI v1, spec §9); `binary`/`tooLarge`/error each render an explicit placeholder card with a humanized size — never a truncated read presented as the whole file (spec §7); an error placeholder ALSO fires a toast, never console-only |
| **Command strip** | horizontal row of Chip atoms under the active terminal, one per recent shell command (OSC-133 `command_events`, spec §6.3): a finished command renders ✓ (`statusRunning` green, `exitCode===0`) or ✗ + the code (`statusExited` red); a command still in flight (a `started` with no matching `finished` yet) renders a Status-dot atom + "running" instead of an outcome glyph; no events yet (or a session that predates the `command_events` table) is NOT an error — a calm dim one-liner, never a blank gap or a toast; a fetch failure fires a toast (spec §7 error-surfacing contract) and renders nothing |
| **Lifecycle chip** | mono 11px `<select>` styled as a pill (999px radius, 1px `border`), cycling a LOCKED enum in its declared order only (`IdeaLifecycle`'s six values, `FitVerdict`'s three, `InsightStatus`'s three — spec §4.2); one accent on hover/focus, no amber (amber stays reserved for "needs you"); an archiving transition that has a server-side precondition (Insight `status → archived` requires non-empty `resolutionReasoning`) is never fired straight from the select — it opens an inline reasoning field + confirm button below the row instead, blocking with a `statusExited` inline message on an empty reasoning rather than round-tripping a doomed request (`IdeasList.tsx`/`InsightsList.tsx`, S3 §10) |
| **Policy form** | numeric cap (empty=∞), chip inputs for classes/allowlist; client mirrors server validation |
| **File-state banner** | info banner (not amber): ExternallyModified → [Accept]; Missing → [Recreate] |
| **Project group row** | bold project header + nested workspace rows; «No project» group last |
| **Quick-capture overlay** | ⌘K portal; title+body+project select; Enter submits; Esc closes |
| **Graph node card** | `bgElevated` + 1px `border`, radius 6, mono-uppercase kind label above a 12px body-face title; an `entityRef` node reads «ref · {entityType}» instead of its own kind and shows «source deleted» in place of the label when orphaned (D3); an `isExternal` (cross-project ghost) node is dimmed (60% opacity) with a dashed border — read-only, click navigates to its own project (`openProject`), never mutated from the panel it's dimmed into; orphaned nodes get a `statusExited` border instead of the default; a search-matched node gets a 2px `accent` outer ring, never a fill change |
| **Graph toolbar** | one row above the canvas: kind `<select>` (every `GraphNodeKind` except `entityRef` — never hand-created) + [Add] primary button; [Delete selected] secondary button acting on the canvas's own multi-select; a right-aligned search `<input>`, debounced, highlighting matches via the match-ring above — never a separate results list; every mutating control (kind select, [Add], [Delete selected]) is `disabled` while `orchdDown`, mirroring `RulesetPanel`'s degradation contract exactly — the search input stays live (it's a read) |

Buttons: primary = accent fill (one per view maximum), secondary = 1px border ghost; destructive
= red border ghost with confirm. Toggles, not checkboxes, for enable/disable.

## 6. UX laws

1. **Delta-first:** every returning view leads with what changed («+4 tasks done · fix
   deployed overnight»), then current state.
2. **One inbox:** all human-needed decisions (escalations, gates, approvals) land in THE inbox —
   never scattered as per-view dialogs. Badge count = actionable items only.
3. **Drill-down, never pogo:** grid → project → artifact in place (panels/sheets), preserving
   context; back is always one gesture.
4. **Every async action shows its truth:** started (immediate optimistic chip) → running (live
   status from events, not spinners) → result (delta line). Failures use the error-surfacing
   contract (spec §13 table) — a toast with the mapped human message, never console-only.
5. **Observability is a first-class screen, not a debug view:** flow-run history renders with the
   same care as the home screen (step timeline, per-step I/O drill-down, cost/duration chips).
6. **Quick capture from anywhere:** ⌘K palette — idea capture, project jump, run workflow. Zero
   navigation cost for the highest-frequency action.
7. **Reduced motion respected;** animation only where it carries meaning (state transition,
   attention pull to a new inbox item) — 150-200ms ease-out, nothing looping.
8. **Focus visible** on every interactive element (2px accent outline offset 2px); contrast ≥
   WCAG AA against `bg`/`bgElevated`.

## 7. Writing rules (copy is design)

- Name things by what the owner recognizes: «Hot questions», not «escalation queue»; «To backlog»,
  not «accept insight». UI copy language: English everywhere — the owner-facing app, code, logs,
  and ids are all English (O-2).
- A control says what happens («Deploy», toast «Deployed»). Errors say what broke and what
  to do next — no apologies, no vagueness.
- Numbers carry units and context («$1.87 · 42 min», «214 users affected»).

## 8. Process rule

Every new feature spec includes a «Design» section that references THIS doc and lists: which
canonical atoms it reuses, which new atoms it introduces (new atom = this doc gets a row in the
same change), and its keyboard path. A feature that invents a parallel visual language fails
review.
