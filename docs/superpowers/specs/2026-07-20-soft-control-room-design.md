# Soft Control Room — design vision & guidebook spec

Date: 2026-07-20 · Status: approved by owner · Supersedes the visual language of
`docs/design-system.md` («Calm Control Room», direction A). Architecture rules that doc
inherits from `frontend-conventions.md` (store/events/testing) are untouched.

## 0. Decision record

| Question | Decision |
|---|---|
| Reference | Owner's screenshot (Claude usage panel): warm soft-gray surfaces, borderless cards, large radii, blue heatmap. **Replaces** the current cool-gray bordered language as the new base. |
| Dark theme | Kept. Translated into the same soft language (warm charcoal fills, no borders). |
| Accent | Two voices: warm terracotta = brand/action accent; blue = data voice (charts, heatmap, info). Data is never terracotta. |
| Typography | System stack stays for UI; **Space Grotesk** added as display face (headings + large stat values), bundled locally. |
| Approach | **B «Soft Control Room»**: containers separate by fill (no outer borders); dense data zones (tables, trees, terminal chrome, row lists) keep 1px hairline separators inside. |
| Scope this cycle | Guidebook v2 (`docs/design-system.md` rewrite) + `src/ui/tokens.css` rewrite + `src/ui/primitives.tsx` restyle + 2 new atoms + font bundling + updated contrast tests. Screen-by-screen migration is the NEXT cycle. |

## 1. Vision

Warm paper neutrality (Anthropic print aesthetic) fused with control-room density. The app
reads as a calm printed dashboard, not an OS window: an ivory ground, cards that are
*darker* fills (never white cards with shadows, never outlines), generous radii, one warm
accent for actions, blue reserved for data. Depth = three fill steps, not lines or shadows.

Core rule of the language: **container = fill, rows inside = hairline.** Any surface that
groups content is a borderless fill one step deeper than its parent; any repeated dense row
inside it (table row, tree row, list item) separates with a near-invisible hairline.

## 2. Tokens — `src/ui/tokens.css` (locked contract)

Same single CSS-variable layer, same `data-theme` mechanism (`src/ui/theme.ts` API
unchanged). Values below are locked; the contrast test (§6) is the enforcement gate — if a
pair fails AA by measurement, darken the foreground token minimally and record the final
value in the guidebook in the same change.

### 2.1 Light (`:root`)

```css
/* Surfaces + ink — three fill steps ARE the borders */
--bg: #faf9f5;            /* ivory ground */
--panel: #f1efe9;         /* card / bar / dialog fill (darker than bg — never white-on-gray) */
--panel-2: #e7e4dc;       /* nested tile / inset / table header */
--ink: #1f1e1c;
--muted: #625e55;         /* warm gray; AA on --panel-2 (measured ≥5.0) */
--hairline: #dcd9d1;      /* in-container row separators ONLY — never outer borders */
--border: #dcd9d1;        /* legacy alias = hairline (migration bridge for unmigrated views) */
--border-strong: #cfccc2; /* legacy alias, stronger edge where still consumed */

/* Brand accent (terracotta) — actions, active states, focus, logo */
--accent: #944527;        /* AA as text on --accent-weak (measured ≥5.1) */
--accent-weak: #f3ddd2;
--on-accent: #ffffff;

/* Data voice (blue) — charts, heatmap, sparklines; alias of info */
--data: #2b66d8;
--data-weak: #eaf0fe;

/* Semantic tones — meanings unchanged (design-system §2 vocabulary), warm-shifted weaks */
--ok: #187c43;      --ok-weak: #e3f1e6;
--warn: #8a5d08;    --warn-weak: #f7ead0;
--danger: #b83232;  --danger-weak: #f9e4e0;
--info: #2b66d8;    --info-weak: #eaf0fe;

/* Spacing — 4px grid unchanged */
--sp-1..6: 4 / 8 / 12 / 16 / 24 / 32;

/* Radius — larger, the soft signature */
--r-sm: 10px;   /* controls, inputs, small tiles */
--r-md: 14px;   /* cards, stat tiles */
--r-lg: 18px;   /* page-level containers, dialogs */
/* chips stay 999px */

/* Type */
--font-ui: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
--font-display: "Space Grotesk", var(--font-ui);   /* headings + large stat values only */
--font-mono: ui-monospace, "SF Mono", Menlo, monospace;
--fs-*: unchanged scale (11/12/13/15/19/26) + --fs-3xl: 28px (page titles);

--shadow-1: 0 1px 2px rgba(31, 30, 28, 0.06);  /* true overlays only (dialog, toast, popover) */
```

### 2.2 Dark (`:root[data-theme="dark"]`)

Same language, warm charcoal:

```css
--bg: #1b1a18;  --panel: #242220;  --panel-2: #2e2b28;
--ink: #ece9e2; --muted: #a39d92;
--hairline: #3a3733; --border: #3a3733; --border-strong: #46423d;
--accent: #e0805c;  --accent-weak: #3b271e;  --on-accent: #201310;
--data: #6f9dff;    --data-weak: #1e2a44;
--ok: #48c98a;   --ok-weak: #14311f;
--warn: #e0a83a;  --warn-weak: #382a10;
--danger: #f06b6b; --danger-weak: #3a1d1a;
--info: #6f9dff;  --info-weak: #1e2a44;
--shadow-1: 0 1px 2px rgba(0, 0, 0, 0.45);
```

### 2.3 Token rules

- New tokens: `--hairline`, `--data`, `--data-weak`, `--font-display`, `--fs-3xl`.
- `--border`/`--border-strong` become **legacy aliases** of hairline values so every
  unmigrated view degrades gracefully this cycle; the guidebook marks them deprecated —
  new code uses `--hairline` and only inside dense containers.
- Focus ring stays the global `:focus-visible` rule, recolored by `--accent` automatically.
- Terminal pane ground stays `#010409` in both themes (a window into the machine, outside
  the theme).

## 3. Typography

- **Display:** Space Grotesk 500/700 — page titles (28px), section headings (19–26px), and
  the large value line of StatTile. Nothing else. Delivered via `@fontsource/space-grotesk`
  (woff2 bundled by Vite → works offline in Tauri); imported once in `src/main.tsx`.
  Exact import paths verified against current @fontsource docs during planning (Context7).
- **UI:** system stack, weights 400/600 — unchanged.
- **Data:** SF Mono + `tabular-nums` — unchanged, still ALL data.

## 4. Component language (this cycle: primitives only)

Existing primitives keep their **exact props API**; only visuals change:

| Atom | New look |
|---|---|
| `Panel` | `--panel` fill, radius `--r-md`, **no border**, padding 16–20 |
| `Stat` | restyled as the screenshot stat tile: `--panel-2` fill, radius `--r-md`, muted 12px label above, bold display-face value (`--font-display` 600) |
| `Badge` | unchanged contract; weak-fill + tone text, radius 999, **no border** |
| `Button` | primary = `--accent` fill + `--on-accent`; secondary = `--panel-2` fill ghost (**fill, not outline**); destructive = `--danger-weak` fill + `--danger` text + confirm |
| `Field` | input on `--panel-2` fill, radius `--r-sm`, no border; focus = accent ring |
| `EmptyState` | unchanged contract, new tokens |
| `Dialog` | `--panel` fill, radius `--r-lg`, `--shadow-1`; amber top-edge rule and a11y contract unchanged |
| `Sparkline` | bars in `--data-weak`, extremes in semantic tones (was border-gray) |

New atoms (added to `src/ui/primitives.tsx`, exported, tested — the screenshot stat-tile
pattern is NOT a new atom: it is the restyle of the existing `Stat` primitive above):

```tsx
/* Segmented pill — view switchers («Overview | Models», «All | 30d | 7d»).
   Group on --panel-2, radius 999; active segment = --panel fill + --ink;
   inactive = transparent + --muted. Keyboard: radiogroup semantics, arrow keys. */
export function SegmentedPill<T extends string>(props: {
  options: readonly { value: T; label: string }[];
  value: T;
  onChange: (value: T) => void;
  ariaLabel: string;
}): JSX.Element;

/* Heatmap — blue density grid. 5 levels: level 0 = --panel-2, levels 1–4 =
   color-mix(in srgb, var(--data) 25/50/75/100%, var(--panel-2)).
   Cells radius 4px, gap 4px, row-major fill into `columns` columns. */
export function Heatmap(props: {
  values: readonly number[]; /* raw values; level = ceil(4 * value / max), clamped */
  columns: number;
  max?: number;              /* default: max(values); all-zero input renders all level-0 */
  ariaLabel: string;
}): JSX.Element;
```

Everything else in design-system.md §5 (atom contracts), §6 (UX laws), §7 (writing rules),
§8 (process rule) carries over verbatim into guidebook v2 — only the visual vocabulary
sections (§1 principles wording, §2 color, §3 type, §4 shape) are rewritten.

## 5. Guidebook v2 — `docs/design-system.md` rewrite (same change)

New structure: 1 Vision & principles («Soft Control Room») · 2 Color tokens (tables above,
light+dark, tone vocabulary) · 3 Typography (3 faces, roles) · 4 Surface & shape (fill-depth
model, container/hairline rule, radii, spacing) · 5 Atoms (existing table updated + 2 new
rows) · 6 UX laws (unchanged) · 7 Writing rules (unchanged) · 8 Process rule (unchanged) ·
9 Migration status (which views still consume legacy `--border` semantics).

## 6. Error handling, testing, observability

- **Contrast (TDD gate):** extend `src/ui/contrast.test.ts` / `tokens.css.test.ts` to the
  new pairs: `--muted` on `--bg`/`--panel`/`--panel-2`; each tone on its `-weak`;
  `--on-accent` on `--accent`; `--data` on `--data-weak`; both themes. Red first (new
  values), then green.
- **Primitives:** unit tests for the 2 new atoms (render, keyboard semantics of
  SegmentedPill, Heatmap level math incl. empty/all-zero/`max=0` guard — no division by
  zero, clamp negatives to 0).
- **Snapshot safety:** existing primitives tests keep passing (API untouched).
- **Font failure mode:** if the woff2 asset fails to load, `--font-display` falls back to
  the system stack silently (font-family fallback chain) — no blank text, no layout jump
  beyond metric substitution.
- **No behavior change:** pure restyle — `docs/ux/scenarios.md` gains no new scenarios;
  the audit reference of token names in scenarios (if any) is checked and updated.

## 7. Out of scope (next cycles)

- Migrating each view/component in `src/components/` off legacy `--border` visuals onto
  the fill/hairline model (tracked per-view in guidebook §9).
- Kanban/graph/terminal chrome re-polish beyond token inheritance.
- Any comfortable-density mode, illustrations, or additional accent hues (still a defect).

## Human steps

None — implementation, tests, and docs are fully autonomous.
