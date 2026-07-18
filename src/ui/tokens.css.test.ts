import { describe, it, expect } from "vitest";
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";

/**
 * BL-29 (design-system.md §6.8 "Focus visible" UX law): the design system promises a visible 2px
 * accent focus ring app-wide. Since the S-UXR token redesign this lives in the single global
 * stylesheet `src/ui/tokens.css` and is theme-aware (`var(--accent)`, so it recolours in light and
 * dark) rather than a hardcoded hex. This is a content/presence assertion — reading the stylesheet
 * off disk is the honest bar for a rule this simple (jsdom has no layout engine and Vitest stubs
 * `?raw` CSS): it exists, targets `:focus-visible`, uses the accent token, and suppresses the plain
 * `:focus` ring for mouse interaction.
 */
describe("tokens.css (BL-29 app-wide :focus-visible ring)", () => {
  function readCss(): string {
    return readFileSync(fileURLToPath(new URL("./tokens.css", import.meta.url)), "utf-8");
  }

  it("defines a 2px accent-token :focus-visible outline offset 2px", () => {
    const css = readCss();
    const match = css.match(/:focus-visible\s*\{([^}]*)\}/);
    expect(match).not.toBeNull();
    const body = match![1];
    expect(body).toMatch(/outline:\s*2px solid var\(--accent\)/i);
    expect(body).toMatch(/outline-offset:\s*2px/i);
  });

  it("suppresses the plain :focus outline for mouse interaction (standard :focus-visible pattern)", () => {
    const css = readCss();
    const match = css.match(/:focus:not\(:focus-visible\)\s*\{([^}]*)\}/);
    expect(match).not.toBeNull();
    expect(match![1]).toMatch(/outline:\s*none/i);
  });

  it("main.tsx imports the token stylesheet", () => {
    const mainPath = fileURLToPath(new URL("../main.tsx", import.meta.url));
    const src = readFileSync(mainPath, "utf-8");
    expect(src).toMatch(/import\s+["']\.\/ui\/tokens\.css["'];?/);
  });
});
