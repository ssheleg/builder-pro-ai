// @vitest-environment jsdom
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import { ThemeToggle } from "./ThemeToggle";
import { useAppStore } from "../store/store";

describe("ThemeToggle", () => {
  beforeEach(() => {
    const store = new Map<string, string>();
    vi.stubGlobal("localStorage", {
      getItem: (k: string) => store.get(k) ?? null,
      setItem: (k: string, v: string) => void store.set(k, v),
      removeItem: (k: string) => void store.delete(k),
      clear: () => store.clear(),
    });
    document.documentElement.removeAttribute("data-theme");
    useAppStore.setState({ theme: "system" });
  });
  afterEach(() => vi.unstubAllGlobals());

  it("cycles system → light → dark → system on click and applies each", () => {
    render(<ThemeToggle />);
    const btn = screen.getByTestId("theme-toggle");

    fireEvent.click(btn); // system → light
    expect(useAppStore.getState().theme).toBe("light");
    expect(document.documentElement.getAttribute("data-theme")).toBeNull();

    fireEvent.click(btn); // light → dark
    expect(useAppStore.getState().theme).toBe("dark");
    expect(document.documentElement.getAttribute("data-theme")).toBe("dark");
    expect(localStorage.getItem("bpa-theme")).toBe("dark");

    fireEvent.click(btn); // dark → system
    expect(useAppStore.getState().theme).toBe("system");
  });
});
