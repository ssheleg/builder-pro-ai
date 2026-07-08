// @vitest-environment jsdom
import { describe, it, expect, afterEach, beforeEach, vi } from "vitest";
import { render, screen, cleanup, act } from "@testing-library/react";
import { Toast } from "./Toast";
import { useAppStore } from "../store/store";

afterEach(cleanup);
beforeEach(() => {
  useAppStore.setState({ toast: null }, false);
});

describe("Toast (S2 T9, design-system.md Toast atom, spec §7 honest error surface)", () => {
  it("renders nothing when there is no toast", () => {
    render(<Toast />);
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("renders the current toast message with role=alert when showToast is called", () => {
    render(<Toast />);
    act(() => useAppStore.getState().showToast("Не удалось подключиться к демону"));
    expect(screen.getByRole("alert").textContent).toBe("Не удалось подключиться к демону");
  });

  it("clears when dismissToast is called", () => {
    render(<Toast />);
    act(() => useAppStore.getState().showToast("boom"));
    expect(screen.getByRole("alert")).toBeTruthy();
    act(() => useAppStore.getState().dismissToast());
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("auto-dismisses ~4s after showToast (fake timers)", () => {
    vi.useFakeTimers();
    try {
      render(<Toast />);
      act(() => useAppStore.getState().showToast("will vanish"));
      expect(screen.getByRole("alert")).toBeTruthy();
      act(() => {
        vi.advanceTimersByTime(4000);
      });
      expect(screen.queryByRole("alert")).toBeNull();
    } finally {
      vi.useRealTimers();
    }
  });

  it("reactively swaps to a newer message when showToast is called again", () => {
    render(<Toast />);
    act(() => useAppStore.getState().showToast("first"));
    expect(screen.getByRole("alert").textContent).toBe("first");
    act(() => useAppStore.getState().showToast("second"));
    expect(screen.getByRole("alert").textContent).toBe("second");
    expect(screen.getAllByRole("alert")).toHaveLength(1);
  });
});
