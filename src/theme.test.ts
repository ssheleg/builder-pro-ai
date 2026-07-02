import { describe, it, expect } from "vitest";
import { theme } from "./theme";

describe("theme", () => {
  it("exposes the four distinct status colors used by StatusDot", () => {
    const { statusIdle, statusRunning, statusExited, statusWaiting } = theme.colors;
    const set = new Set([statusIdle, statusRunning, statusExited, statusWaiting]);
    expect(set.size).toBe(4); // all four are distinct
    for (const c of [statusIdle, statusRunning, statusExited, statusWaiting]) {
      expect(c).toMatch(/^#[0-9a-fA-F]{6}$/); // concrete hex colors
    }
  });

  it("is a dark theme (bg is dark)", () => {
    // parse #RRGGBB and assert luminance is low
    const hex = theme.colors.bg.replace("#", "");
    const r = parseInt(hex.slice(0, 2), 16);
    const g = parseInt(hex.slice(2, 4), 16);
    const b = parseInt(hex.slice(4, 6), 16);
    const lum = 0.2126 * r + 0.7152 * g + 0.0722 * b;
    expect(lum).toBeLessThan(64); // dark
  });

  it("exposes core panel colors distinct from bg", () => {
    expect(theme.colors.bgElevated).not.toBe(theme.colors.bg);
    expect(theme.colors.border).toMatch(/^#[0-9a-fA-F]{6}$/);
    expect(theme.colors.text).toMatch(/^#[0-9a-fA-F]{6}$/);
    expect(theme.colors.textDim).toMatch(/^#[0-9a-fA-F]{6}$/);
    expect(theme.colors.accent).toMatch(/^#[0-9a-fA-F]{6}$/);
  });
});
