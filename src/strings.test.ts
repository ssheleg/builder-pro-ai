import { describe, it, expect } from "vitest";
import { strings } from "./strings";

describe("strings", () => {
  it("resolves plain English leaf values", () => {
    expect(strings.common.cancel).toBe("Cancel");
    expect(strings.errors.notFound).toBe("not found");
    expect(strings.errors.unavailable).toBe("orchestrator unavailable");
    expect(strings.chrome.orchdUnavailable).toBe("Orchestrator unavailable");
  });

  it("interpolates parameterized copy in English", () => {
    expect(strings.errors.invariant("last workspace")).toBe("invalid operation: last workspace");
    expect(strings.research.dialogTitle("My idea")).toBe('Research "My idea"');
    expect(strings.home.openWorkspace("acme")).toBe("Open acme");
  });

  it("carries no Cyrillic in any leaf value", () => {
    const cyrillic = /[\u0400-\u04FF]/;
    const walk = (node: unknown): void => {
      if (typeof node === "string") {
        expect(cyrillic.test(node)).toBe(false);
      } else if (typeof node === "function") {
        expect(cyrillic.test((node as (...a: string[]) => string)("x", "y"))).toBe(false);
      } else if (node && typeof node === "object") {
        for (const value of Object.values(node)) walk(value);
      }
    };
    walk(strings);
  });
});
