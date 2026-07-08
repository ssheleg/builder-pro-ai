import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

/**
 * BL-29 (docs/backlog.md, design-system.md §5 Dialog atom + §6.8 "Focus visible" UX law): the
 * design system PROMISES a visible 2px accent focus ring app-wide, but until now there was no
 * global stylesheet at all — every component relied on the UA default focus outline. This is a
 * content/presence assertion (a real CSS-application test would need a browser-grade layout
 * engine jsdom doesn't provide, and Vite/Vitest's `?raw` CSS import is stubbed to an empty
 * string under vitest's default SSR transform) — reading the stylesheet's source straight off
 * disk is the honest bar for a rule this simple: it exists, it targets `:focus-visible`, and it
 * uses the theme accent color.
 */
describe("index.css (BL-29 app-wide :focus-visible ring)", () => {
  function readCss(): string {
    const cssPath = fileURLToPath(new URL("./index.css", import.meta.url));
    return readFileSync(cssPath, "utf-8");
  }

  it("defines a 2px accent :focus-visible outline (theme.colors.accent = #2f81f7)", () => {
    const css = readCss();
    const match = css.match(/:focus-visible\s*\{([^}]*)\}/);
    expect(match).not.toBeNull();
    const body = match![1];
    expect(body).toMatch(/outline:\s*2px solid #2f81f7/i);
    expect(body).toMatch(/outline-offset:\s*2px/i);
  });

  it("suppresses the plain :focus outline for mouse interaction (standard :focus-visible pattern)", () => {
    const css = readCss();
    const match = css.match(/:focus:not\(:focus-visible\)\s*\{([^}]*)\}/);
    expect(match).not.toBeNull();
    expect(match![1]).toMatch(/outline:\s*none/i);
  });

  it("main.tsx imports the app-wide stylesheet", () => {
    const mainPath = fileURLToPath(new URL("./main.tsx", import.meta.url));
    const src = readFileSync(mainPath, "utf-8");
    expect(src).toMatch(/import\s+["']\.\/index\.css["'];?/);
  });
});
