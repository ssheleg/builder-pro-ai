// src/ui/contrast.ts — WCAG 2.x relative-luminance + contrast-ratio math, so the token palette's
// legibility is a VERIFIED invariant (see tokens.contrast.test.ts) rather than an eyeball guess.
// Pure, dependency-free.

export type Rgb = { r: number; g: number; b: number };

/** Parse `#rgb` or `#rrggbb` (with/without leading `#`) into 0–255 channels. Throws on anything
 * else so a malformed token fails loudly in the test rather than silently scoring 0. */
export function hexToRgb(hex: string): Rgb {
  const h = hex.trim().replace(/^#/, "");
  const full = h.length === 3 ? h.split("").map((c) => c + c).join("") : h;
  if (!/^[0-9a-fA-F]{6}$/.test(full)) throw new Error(`not a hex color: ${hex}`);
  return {
    r: parseInt(full.slice(0, 2), 16),
    g: parseInt(full.slice(2, 4), 16),
    b: parseInt(full.slice(4, 6), 16),
  };
}

function channel(c: number): number {
  const s = c / 255;
  return s <= 0.03928 ? s / 12.92 : Math.pow((s + 0.055) / 1.055, 2.4);
}

/** WCAG relative luminance of an sRGB color (0 = black, 1 = white). */
export function relativeLuminance(rgb: Rgb): number {
  return 0.2126 * channel(rgb.r) + 0.7152 * channel(rgb.g) + 0.0722 * channel(rgb.b);
}

/** WCAG contrast ratio between two hex colors, in `[1, 21]`. Symmetric in its arguments. */
export function contrastRatio(fg: string, bg: string): number {
  const l1 = relativeLuminance(hexToRgb(fg));
  const l2 = relativeLuminance(hexToRgb(bg));
  const hi = Math.max(l1, l2);
  const lo = Math.min(l1, l2);
  return (hi + 0.05) / (lo + 0.05);
}

/** WCAG AA thresholds: 4.5:1 for normal-sized text, 3:1 for large text / non-text UI. */
export const AA_TEXT = 4.5;
export const AA_LARGE = 3;
