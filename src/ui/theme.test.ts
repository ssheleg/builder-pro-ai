// @vitest-environment jsdom
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import {
  applyTheme,
  resolveTheme,
  readTheme,
  setThemePref,
  statusTone,
} from "./theme";

function mockMatchMedia(dark: boolean) {
  vi.stubGlobal("matchMedia", (q: string) => ({
    matches: dark && q.includes("dark"),
    media: q,
    addEventListener: () => {},
    removeEventListener: () => {},
  }));
}

describe("theme", () => {
  beforeEach(() => {
    const store = new Map<string, string>();
    vi.stubGlobal("localStorage", {
      getItem: (k: string) => store.get(k) ?? null,
      setItem: (k: string, v: string) => void store.set(k, v),
      removeItem: (k: string) => void store.delete(k),
      clear: () => store.clear(),
    });
    document.documentElement.removeAttribute("data-theme");
  });
  afterEach(() => vi.unstubAllGlobals());

  it("applyTheme('dark') sets data-theme=dark; 'light' removes it", () => {
    applyTheme("dark");
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
    applyTheme("light");
    expect(document.documentElement.getAttribute("data-theme")).toBeNull();
  });

  it("resolveTheme('system') follows the OS preference", () => {
    mockMatchMedia(true);
    expect(resolveTheme("system")).toBe("dark");
    mockMatchMedia(false);
    expect(resolveTheme("system")).toBe("light");
  });

  it("applyTheme('system') applies the resolved OS palette", () => {
    mockMatchMedia(true);
    applyTheme("system");
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
  });

  it("readTheme defaults to 'system' and round-trips a persisted pref", () => {
    expect(readTheme()).toBe("system");
    setThemePref("dark");
    expect(readTheme()).toBe("dark");
    expect(localStorage.getItem("bpa-theme")).toBe("dark");
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
  });

  it("statusTone maps statuses to semantic tones", () => {
    expect(statusTone("running")).toBe("info");
    expect(statusTone("waiting")).toBe("warn");
    expect(statusTone("done")).toBe("ok");
    expect(statusTone("accepted")).toBe("ok");
    expect(statusTone("failed")).toBe("danger");
    expect(statusTone("interrupted")).toBe("danger");
    expect(statusTone("pending")).toBe("muted");
    expect(statusTone("archived")).toBe("muted");
  });
});
