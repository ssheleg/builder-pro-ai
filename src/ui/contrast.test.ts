import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { hexToRgb, relativeLuminance, contrastRatio, AA_TEXT } from "./contrast";

describe("contrast math", () => {
  it("parses #rgb and #rrggbb", () => {
    expect(hexToRgb("#fff")).toEqual({ r: 255, g: 255, b: 255 });
    expect(hexToRgb("2b66d8")).toEqual({ r: 43, g: 102, b: 216 });
  });

  it("relativeLuminance is 0 for black and 1 for white", () => {
    expect(relativeLuminance(hexToRgb("#000000"))).toBeCloseTo(0, 5);
    expect(relativeLuminance(hexToRgb("#ffffff"))).toBeCloseTo(1, 5);
  });

  it("contrastRatio is 21 for black/white, 1 for identical, and symmetric", () => {
    expect(contrastRatio("#000000", "#ffffff")).toBeCloseTo(21, 1);
    expect(contrastRatio("#3a3a3a", "#3a3a3a")).toBeCloseTo(1, 5);
    expect(contrastRatio("#2b66d8", "#ffffff")).toBeCloseTo(contrastRatio("#ffffff", "#2b66d8"), 5);
  });
});

// ── The palette's AA legibility is a guarded invariant ─────────────────────────────────────────
// Parse the real tokens.css and assert every text pairing the UI actually renders clears WCAG AA
// (4.5:1) in BOTH themes. This is the regression guard for the S-DESIGN contrast fix: a future token
// tweak that dims a tone below AA fails here instead of shipping an illegible badge.

function tokenBlock(css: string, selector: string): Record<string, string> {
  const start = css.indexOf(selector + " {");
  if (start < 0) throw new Error(`selector not found: ${selector}`);
  const open = css.indexOf("{", start);
  const close = css.indexOf("}", open);
  const body = css.slice(open + 1, close);
  const map: Record<string, string> = {};
  for (const m of body.matchAll(/(--[\w-]+):\s*(#[0-9a-fA-F]{3,8})\s*;/g)) map[m[1]] = m[2];
  return map;
}

const css = readFileSync(fileURLToPath(new URL("./tokens.css", import.meta.url)), "utf-8");
const THEMES: Array<[string, Record<string, string>]> = [
  ["light", tokenBlock(css, ":root")],
  ["dark", tokenBlock(css, ':root[data-theme="dark"]')],
];
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
    expect(t["--border"]).toBe(t["--hairline"]); // legacy alias contract (spec 2026-07-20 §2.3)
  });
});
