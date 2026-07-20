# Soft Control Room (design v2) Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace the visual base with the approved «Soft Control Room» language: warm borderless fills, terracotta accent + blue data voice, Space Grotesk display face — tokens, primitives, two new atoms, guidebook v2.

**Architecture:** Single CSS-variable layer (`src/ui/tokens.css`) is rewritten in place; `src/ui/theme.ts` API untouched. Primitives restyle without prop changes; `--border`/`--border-strong` stay as hairline aliases so unmigrated views degrade gracefully. Contrast tests are the enforcement gate.

**Tech Stack:** React 19 + Vite + Vitest (jsdom), Tauri 2, `@fontsource/space-grotesk` (new dep; import contract `@fontsource/space-grotesk/500.css`+`700.css`, family `"Space Grotesk"` — verified via Context7 2026-07-20).

**Spec:** `docs/superpowers/specs/2026-07-20-soft-control-room-design.md`

## Global Constraints

- Components consume tokens ONLY as `var(--…)` — never raw hex (except terminal ground `#010409`, which is intentionally theme-independent).
- Every text pairing must clear WCAG AA 4.5:1 in both themes — enforced by `src/ui/contrast.test.ts`.
- One warm accent (`--accent` terracotta) for actions; `--data` blue for charts/heatmap; data is never terracotta; amber reserved for «a human is needed».
- Fill-depth model: container = fill (`--bg` → `--panel` → `--panel-2`), rows inside dense containers = `--hairline`; no outer borders, no decorative shadows (`--shadow-1` = overlays only).
- Primitives keep their exact existing props APIs.
- All copy English; conventional commits; `npm test` green before every commit.

---

### Task 1: Rewrite `tokens.css` (new palette, TDD via contrast tests)

**Files:**
- Modify: `src/ui/contrast.test.ts`
- Modify: `src/ui/tokens.css`

**Interfaces:**
- Produces: CSS variables consumed by every later task: `--bg --panel --panel-2 --ink --muted --hairline --border --border-strong --accent --accent-weak --on-accent --data --data-weak --ok/--warn/--danger/--info (+ -weak) --sp-1..6 --r-sm(10px) --r-md(14px) --r-lg(18px) --font-ui --font-display --font-mono --fs-xs..2xl --fs-3xl --shadow-1`, in `:root` and `:root[data-theme="dark"]`.

- [ ] **Step 1: Extend the contrast test with the new pairs**

In `src/ui/contrast.test.ts` replace the `TONES` line and the two `it` blocks after it with:

```ts
const TONES = ["ok", "warn", "danger", "info", "accent", "data"] as const;

describe.each(THEMES)("tokens.css AA legibility — %s theme", (_name, t) => {
  it("primary + secondary ink clear AA on their surfaces", () => {
    expect(contrastRatio(t["--ink"], t["--bg"])).toBeGreaterThanOrEqual(AA_TEXT);
    expect(contrastRatio(t["--ink"], t["--panel"])).toBeGreaterThanOrEqual(AA_TEXT);
    expect(contrastRatio(t["--ink"], t["--panel-2"])).toBeGreaterThanOrEqual(AA_TEXT);
    expect(contrastRatio(t["--muted"], t["--bg"])).toBeGreaterThanOrEqual(AA_TEXT);
    expect(contrastRatio(t["--muted"], t["--panel"])).toBeGreaterThanOrEqual(AA_TEXT);
    expect(contrastRatio(t["--muted"], t["--panel-2"])).toBeGreaterThanOrEqual(AA_TEXT);
  });

  it.each(TONES)("tone %s clears AA as text on its -weak background AND on --panel", (tone) => {
    expect(contrastRatio(t[`--${tone}`], t[`--${tone}-weak`])).toBeGreaterThanOrEqual(AA_TEXT);
    expect(contrastRatio(t[`--${tone}`], t["--panel"])).toBeGreaterThanOrEqual(AA_TEXT);
  });

  it("on-accent label clears AA on the accent fill", () => {
    expect(contrastRatio(t["--on-accent"], t["--accent"])).toBeGreaterThanOrEqual(AA_TEXT);
  });

  it("declares the Soft Control Room structural tokens", () => {
    expect(t["--hairline"]).toBeTruthy();
    expect(t["--border"]).toBe(t["--hairline"]); // legacy alias contract
  });
});
```

- [ ] **Step 2: Run — expect FAIL** (`--data`/`--hairline` missing in current palette)

Run: `npx vitest run src/ui/contrast.test.ts`
Expected: FAIL — `--data` pairs undefined (`contrastRatio` throws `not a hex color: undefined`) / alias assertion fails.

- [ ] **Step 3: Rewrite `src/ui/tokens.css`**

Replace the `:root` and `:root[data-theme="dark"]` blocks (keep the file's `html,body,#root` baseline and both focus rules verbatim — `tokens.css.test.ts` guards them):

```css
/*
 * src/ui/tokens.css — «Soft Control Room» design tokens (spec 2026-07-20).
 *
 * Warm paper neutrality: depth = three fill steps (--bg → --panel → --panel-2), NOT borders or
 * shadows. --hairline separates dense rows INSIDE containers only; --border/--border-strong are
 * legacy aliases of it so unmigrated views degrade gracefully. --accent (terracotta) = actions;
 * --data (blue) = charts/heatmap — data is never terracotta. Every text pairing below is verified
 * AA by contrast.test.ts. Light on :root; dark on :root[data-theme="dark"] (theme.ts stamps it).
 */

:root {
  color-scheme: light;

  /* Surfaces + ink — fill steps ARE the borders */
  --bg: #faf9f5;
  --panel: #f1efe9;
  --panel-2: #e7e4dc;
  --ink: #1f1e1c;
  --muted: #625e55;
  --hairline: #dcd9d1;
  --border: #dcd9d1;        /* legacy alias = hairline */
  --border-strong: #cfccc2; /* legacy alias */

  /* Brand accent (terracotta) — actions, active states, focus */
  --accent: #944527;
  --accent-weak: #f3ddd2;
  --on-accent: #ffffff;

  /* Data voice (blue) — charts, heatmap, sparklines */
  --data: #2b66d8;
  --data-weak: #eaf0fe;

  /* Semantic tones (meanings unchanged; warm-shifted weaks) */
  --ok: #157239;
  --ok-weak: #e3f1e6;
  --warn: #8a5d08;
  --warn-weak: #f7ead0;
  --danger: #b83232;
  --danger-weak: #f9e4e0;
  --info: #2b66d8;
  --info-weak: #eaf0fe;

  /* Spacing scale (4px grid) */
  --sp-1: 4px;
  --sp-2: 8px;
  --sp-3: 12px;
  --sp-4: 16px;
  --sp-5: 24px;
  --sp-6: 32px;

  /* Radius — the soft signature */
  --r-sm: 10px;
  --r-md: 14px;
  --r-lg: 18px;

  /* Type */
  --font-ui: -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
  --font-display: "Space Grotesk", -apple-system, BlinkMacSystemFont, "Segoe UI", Roboto, sans-serif;
  --font-mono: ui-monospace, "SF Mono", Menlo, monospace;
  --fs-xs: 11px;
  --fs-sm: 12px;
  --fs-md: 13px;
  --fs-lg: 15px;
  --fs-xl: 19px;
  --fs-2xl: 26px;
  --fs-3xl: 28px;

  --shadow-1: 0 1px 2px rgba(31, 30, 28, 0.06);
}

:root[data-theme="dark"] {
  color-scheme: dark;

  --bg: #1b1a18;
  --panel: #242220;
  --panel-2: #2e2b28;
  --ink: #ece9e2;
  --muted: #a39d92;
  --hairline: #3a3733;
  --border: #3a3733;
  --border-strong: #46423d;

  --accent: #e0805c;
  --accent-weak: #3b271e;
  /* Light coral accent → the label ON an accent fill must be dark to clear AA. */
  --on-accent: #201310;

  --data: #6f9dff;
  --data-weak: #1e2a44;

  --ok: #48c98a;
  --ok-weak: #14311f;
  --warn: #e0a83a;
  --warn-weak: #382a10;
  --danger: #f06b6b;
  --danger-weak: #3a1d1a;
  --info: #6f9dff;
  --info-weak: #1e2a44;

  --shadow-1: 0 1px 2px rgba(0, 0, 0, 0.45);
}
```

- [ ] **Step 4: Run contrast + tokens tests — expect PASS**

Run: `npx vitest run src/ui/contrast.test.ts src/ui/tokens.css.test.ts`
Expected: PASS (all pairs pre-verified by WCAG math: tightest are light `--warn`/`--warn-weak` ≈4.8, `--ok`/`--ok-weak` ≈5.1 after the #157239 darkening, dark `--accent`/`--accent-weak` ≈5.0).

- [ ] **Step 5: Commit**

```bash
git add src/ui/tokens.css src/ui/contrast.test.ts
git commit -m "feat(design): Soft Control Room token palette — warm fills, terracotta accent, data voice"
```

### Task 2: Bundle Space Grotesk

**Files:**
- Modify: `package.json` (+ lockfile via npm)
- Modify: `src/main.tsx`
- Modify: `src/ui/tokens.css.test.ts`

**Interfaces:**
- Consumes: `--font-display` token (Task 1).
- Produces: `"Space Grotesk"` available at weights 500/700 offline.

- [ ] **Step 1: Extend `tokens.css.test.ts` with a font-import assertion** (append inside the existing describe):

```ts
  it("main.tsx bundles Space Grotesk 500/700 for --font-display (spec 2026-07-20 §3)", () => {
    const mainPath = fileURLToPath(new URL("../main.tsx", import.meta.url));
    const src = readFileSync(mainPath, "utf-8");
    expect(src).toMatch(/import\s+["']@fontsource\/space-grotesk\/500\.css["'];?/);
    expect(src).toMatch(/import\s+["']@fontsource\/space-grotesk\/700\.css["'];?/);
    expect(readCss()).toContain('--font-display: "Space Grotesk"');
  });
```

- [ ] **Step 2: Run — expect FAIL** — `npx vitest run src/ui/tokens.css.test.ts` (imports missing)

- [ ] **Step 3: Install + import**

Run: `npm install @fontsource/space-grotesk`
In `src/main.tsx` add above the tokens import:

```tsx
import "@fontsource/space-grotesk/500.css";
import "@fontsource/space-grotesk/700.css";
import "./ui/tokens.css";
```

- [ ] **Step 4: Run — expect PASS** — `npx vitest run src/ui/tokens.css.test.ts`

- [ ] **Step 5: Commit**

```bash
git add package.json package-lock.json src/main.tsx src/ui/tokens.css.test.ts
git commit -m "feat(design): bundle Space Grotesk 500/700 as the display face"
```

### Task 3: Restyle existing primitives (no API changes)

**Files:**
- Modify: `src/ui/primitives.tsx`

**Interfaces:**
- Consumes: Task 1 tokens. Props APIs unchanged — `primitives.test.tsx` must stay green untouched.
- Produces: restyled `Panel, Stat, Sparkline, Badge, Button, Field, Input, TextArea, Select, EmptyState, Dialog`.

- [ ] **Step 1: Apply the visual changes** (style objects only):

- `Panel` outer div: `background: "var(--panel)"`, drop `border` and `boxShadow`, `borderRadius: "var(--r-md)"`; header row: replace `borderBottom: "1px solid var(--border)"` with `borderBottom: "1px solid var(--hairline)"`.
- `Stat` (becomes the screenshot stat tile): container `background: "var(--panel-2)"`, no border, `borderRadius: "var(--r-md)"`, `padding: "var(--sp-3) var(--sp-4)"`; value span `fontFamily: "var(--font-display)"`, `fontWeight: 700`, keep `fontVariantNumeric: "tabular-nums"` (add `fontFeatureSettings: '"tnum"'` NOT needed — keep as is).
- `Sparkline`: change default stroke usage so the line uses the data voice — replace `TONE_FG[tone]` default rendering by adding `data` handling: change the `tone` prop default to `"accent"` as now, but in `Stat` pass `tone === "ink" ? "info" : tone` (blue data voice instead of terracotta for the default spark).
- `Button` variants:

```ts
const variants: Record<string, CSSProperties> = {
  primary: { background: "var(--accent)", color: "var(--on-accent)", borderColor: "transparent" },
  ghost: { background: "var(--panel-2)", color: "var(--ink)", borderColor: "transparent" },
  danger: { background: "var(--danger-weak)", color: "var(--danger)", borderColor: "transparent" },
};
```

  and `base.borderRadius: "var(--r-sm)"`.
- `controlStyle` (Input/TextArea/Select): `background: "var(--panel-2)"`, `border: "1px solid transparent"`, `borderRadius: "var(--r-sm)"`.
- `Dialog` card: drop `border`, `borderRadius: "var(--r-lg)"`, keep `--shadow-1`; header/footer separators → `var(--hairline)`.
- File header comment: update to «Soft Control Room primitives kit (spec 2026-07-20)».

- [ ] **Step 2: Run — expect PASS** — `npx vitest run src/ui/primitives.test.tsx`

- [ ] **Step 3: Commit**

```bash
git add src/ui/primitives.tsx
git commit -m "feat(design): restyle primitives to Soft Control Room — borderless fills, soft radii"
```

### Task 4: `SegmentedPill` atom (TDD)

**Files:**
- Modify: `src/ui/primitives.tsx`
- Modify: `src/ui/primitives.test.tsx`

**Interfaces:**
- Produces: `export function SegmentedPill<T extends string>(props: { options: readonly { value: T; label: string }[]; value: T; onChange: (value: T) => void; ariaLabel: string; "data-testid"?: string })`.

- [ ] **Step 1: Write failing tests** (append to `primitives.test.tsx`; add `SegmentedPill` to the import list):

```tsx
  it("SegmentedPill renders radiogroup semantics and switches on click", () => {
    const onChange = vi.fn();
    render(
      <SegmentedPill
        ariaLabel="Range"
        options={[{ value: "all", label: "All" }, { value: "30d", label: "30d" }] as const}
        value="all"
        onChange={onChange}
      />,
    );
    const group = screen.getByRole("radiogroup", { name: "Range" });
    expect(group).toBeTruthy();
    const radios = screen.getAllByRole("radio");
    expect(radios).toHaveLength(2);
    expect(radios[0].getAttribute("aria-checked")).toBe("true");
    fireEvent.click(radios[1]);
    expect(onChange).toHaveBeenCalledWith("30d");
  });

  it("SegmentedPill moves selection with arrow keys", () => {
    const onChange = vi.fn();
    render(
      <SegmentedPill
        ariaLabel="Range"
        options={[{ value: "all", label: "All" }, { value: "30d", label: "30d" }] as const}
        value="all"
        onChange={onChange}
      />,
    );
    fireEvent.keyDown(screen.getByRole("radiogroup", { name: "Range" }), { key: "ArrowRight" });
    expect(onChange).toHaveBeenCalledWith("30d");
  });
```

- [ ] **Step 2: Run — expect FAIL** — `npx vitest run src/ui/primitives.test.tsx` («SegmentedPill is not exported»)

- [ ] **Step 3: Implement** (append to `primitives.tsx`):

```tsx
// ---- SegmentedPill (view switcher — «Overview | Models», «All | 30d | 7d») -------------------

export function SegmentedPill<T extends string>({
  options,
  value,
  onChange,
  ariaLabel,
  "data-testid": testId,
}: {
  options: readonly { value: T; label: string }[];
  value: T;
  onChange: (value: T) => void;
  ariaLabel: string;
  "data-testid"?: string;
}) {
  const idx = options.findIndex((o) => o.value === value);
  const move = (delta: number) => {
    if (options.length === 0) return;
    const next = options[(idx + delta + options.length) % options.length];
    if (next.value !== value) onChange(next.value);
  };
  return (
    <div
      data-testid={testId}
      role="radiogroup"
      aria-label={ariaLabel}
      tabIndex={0}
      onKeyDown={(e) => {
        if (e.key === "ArrowRight" || e.key === "ArrowDown") { e.preventDefault(); move(1); }
        if (e.key === "ArrowLeft" || e.key === "ArrowUp") { e.preventDefault(); move(-1); }
      }}
      style={{
        display: "inline-flex",
        gap: 2,
        padding: 2,
        background: "var(--panel-2)",
        borderRadius: 999,
      }}
    >
      {options.map((o) => {
        const active = o.value === value;
        return (
          <button
            key={o.value}
            type="button"
            role="radio"
            aria-checked={active}
            tabIndex={-1}
            onClick={() => { if (!active) onChange(o.value); }}
            style={{
              border: "none",
              cursor: active ? "default" : "pointer",
              padding: "var(--sp-1) var(--sp-3)",
              fontSize: "var(--fs-sm)",
              fontFamily: "var(--font-ui)",
              fontWeight: 600,
              borderRadius: 999,
              background: active ? "var(--panel)" : "transparent",
              color: active ? "var(--ink)" : "var(--muted)",
            }}
          >
            {o.label}
          </button>
        );
      })}
    </div>
  );
}
```

- [ ] **Step 4: Run — expect PASS**, **Step 5: Commit**

```bash
git add src/ui/primitives.tsx src/ui/primitives.test.tsx
git commit -m "feat(design): SegmentedPill atom — pill view switcher with radiogroup keyboard nav"
```

### Task 5: `Heatmap` atom (TDD)

**Files:**
- Modify: `src/ui/primitives.tsx`
- Modify: `src/ui/primitives.test.tsx`

**Interfaces:**
- Produces: `export function Heatmap(props: { values: readonly number[]; columns: number; max?: number; ariaLabel: string; "data-testid"?: string })`. Level math: `level = value <= 0 ? 0 : Math.min(4, Math.ceil((4 * value) / effectiveMax))` where `effectiveMax = max ?? Math.max(...values, 0)`; `effectiveMax <= 0` → every cell level 0 (no division by zero).

- [ ] **Step 1: Write failing tests** (append; add `Heatmap` to imports):

```tsx
  it("Heatmap renders one cell per value with 5-level buckets", () => {
    render(<Heatmap values={[0, 1, 2, 3, 4]} columns={5} max={4} ariaLabel="Activity" data-testid="h" />);
    const grid = screen.getByTestId("h");
    expect(grid.getAttribute("aria-label")).toBe("Activity");
    const cells = grid.querySelectorAll("[data-level]");
    expect(cells).toHaveLength(5);
    expect(Array.from(cells).map((c) => c.getAttribute("data-level"))).toEqual(["0", "1", "2", "3", "4"]);
  });

  it("Heatmap survives all-zero and empty inputs (no division by zero)", () => {
    const { rerender } = render(<Heatmap values={[0, 0, 0]} columns={3} ariaLabel="A" data-testid="h" />);
    expect(
      Array.from(screen.getByTestId("h").querySelectorAll("[data-level]")).every(
        (c) => c.getAttribute("data-level") === "0",
      ),
    ).toBe(true);
    rerender(<Heatmap values={[]} columns={3} ariaLabel="A" data-testid="h" />);
    expect(screen.getByTestId("h").querySelectorAll("[data-level]")).toHaveLength(0);
  });

  it("Heatmap clamps negatives to level 0 and overshoot to level 4", () => {
    render(<Heatmap values={[-5, 99]} columns={2} max={4} ariaLabel="A" data-testid="h" />);
    const levels = Array.from(screen.getByTestId("h").querySelectorAll("[data-level]")).map((c) =>
      c.getAttribute("data-level"),
    );
    expect(levels).toEqual(["0", "4"]);
  });
```

- [ ] **Step 2: Run — expect FAIL**

- [ ] **Step 3: Implement** (append to `primitives.tsx`):

```tsx
// ---- Heatmap (blue density grid — the data voice) --------------------------------------------

const HEATMAP_LEVEL_BG = [
  "var(--panel-2)",
  "color-mix(in srgb, var(--data) 25%, var(--panel-2))",
  "color-mix(in srgb, var(--data) 50%, var(--panel-2))",
  "color-mix(in srgb, var(--data) 75%, var(--panel-2))",
  "var(--data)",
] as const;

export function Heatmap({
  values,
  columns,
  max,
  ariaLabel,
  "data-testid": testId,
}: {
  values: readonly number[];
  columns: number;
  max?: number;
  ariaLabel: string;
  "data-testid"?: string;
}) {
  const effectiveMax = max ?? Math.max(...values, 0);
  return (
    <div
      data-testid={testId}
      role="img"
      aria-label={ariaLabel}
      style={{
        display: "grid",
        gridTemplateColumns: `repeat(${Math.max(1, columns)}, 12px)`,
        gap: 4,
        width: "fit-content",
      }}
    >
      {values.map((v, i) => {
        const level =
          effectiveMax <= 0 || v <= 0 ? 0 : Math.min(4, Math.ceil((4 * v) / effectiveMax));
        return (
          <div
            key={i}
            data-level={level}
            style={{ width: 12, height: 12, borderRadius: 4, background: HEATMAP_LEVEL_BG[level] }}
          />
        );
      })}
    </div>
  );
}
```

- [ ] **Step 4: Run — expect PASS**, **Step 5: Commit**

```bash
git add src/ui/primitives.tsx src/ui/primitives.test.tsx
git commit -m "feat(design): Heatmap atom — 5-level blue density grid"
```

### Task 6: Guidebook v2 — rewrite `docs/design-system.md` + changelog

**Files:**
- Modify: `docs/design-system.md` (full rewrite per spec §5 structure)
- Modify: `CHANGELOG.md` (Unreleased/new entry)

**Interfaces:**
- Consumes: final token values from Task 1 (record `--ok: #157239` etc. as shipped).

- [ ] **Step 1: Rewrite `docs/design-system.md`** with structure: 1 Vision & principles (Soft Control Room, fill-depth model, container/hairline rule) · 2 Color (final token tables light+dark incl. `--hairline/--data/--font-display`; tone vocabulary unchanged; `--border` deprecated-alias note) · 3 Typography (Space Grotesk display / system UI / mono data, roles + sizes incl. `--fs-3xl`) · 4 Surface & shape (radii 10/14/18, spacing, shadow=overlays-only) · 5 Atoms (updated table: Stat=stat tile, Button fills, Field fills, Dialog r-lg; + rows for SegmentedPill, Heatmap, Banner pattern, Nav item pattern) · 6 UX laws (verbatim carry-over) · 7 Writing rules (verbatim) · 8 Process rule (verbatim) · 9 Migration status (views still on legacy `--border` semantics: every file in `src/components/` until migrated; tracked next cycle).
- [ ] **Step 2: Add CHANGELOG entry** under a new `## [Unreleased]` section: `### Changed — Soft Control Room design v2: new warm token palette (light+dark), Space Grotesk display face, restyled primitives, new SegmentedPill/Heatmap atoms; --border is now a deprecated alias of --hairline.`
- [ ] **Step 3: Commit**

```bash
git add docs/design-system.md CHANGELOG.md
git commit -m "docs(design): guidebook v2 — Soft Control Room visual language"
```

### Task 7: Full verification + push

- [ ] **Step 1: Full suite** — Run: `npm test` — Expected: all TS tests pass (932+ baseline + new).
- [ ] **Step 2: Type check** — Run: `npx tsc --noEmit` — Expected: no errors.
- [ ] **Step 3: Push** — Run: `git push origin nightbuild`.
