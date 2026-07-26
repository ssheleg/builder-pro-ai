// @vitest-environment jsdom
import { describe, it, expect, afterEach, beforeEach, vi } from "vitest";
import { render, screen, cleanup, act, fireEvent } from "@testing-library/react";
import { Toast } from "./Toast";
import { useAppStore } from "../store/store";

afterEach(cleanup);
beforeEach(() => {
  useAppStore.setState({ toast: null, toastQueue: [], toastTone: "error", toastToneQueue: [] }, false);
});

describe("Toast (S2 T9, design-system.md Toast atom, spec §7 honest error surface)", () => {
  it("renders nothing when there is no toast", () => {
    render(<Toast />);
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("renders the current toast message with role=alert when showToast is called", () => {
    render(<Toast />);
    act(() => useAppStore.getState().showToast("Failed to connect to the daemon"));
    // The alert also carries the manual-dismiss "×" button now (BL-97), so match on containment.
    expect(screen.getByRole("alert").textContent).toContain("Failed to connect to the daemon");
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

  it("keeps showing the first message when a second is queued behind it (BL-97 — not clobbered)", () => {
    render(<Toast />);
    act(() => useAppStore.getState().showToast("first"));
    expect(screen.getByRole("alert").textContent).toContain("first");
    act(() => useAppStore.getState().showToast("second"));
    // The visible toast stays "first"; "second" waits in the queue behind it.
    expect(screen.getByRole("alert").textContent).toContain("first");
    expect(screen.getAllByRole("alert")).toHaveLength(1);
  });

  it("the close button advances the queue to the next toast, then clears it (BL-97)", () => {
    render(<Toast />);
    act(() => useAppStore.getState().showToast("first"));
    act(() => useAppStore.getState().showToast("second"));
    expect(screen.getByRole("alert").textContent).toContain("first");

    fireEvent.click(screen.getByTestId("toast-dismiss"));
    expect(screen.getByRole("alert").textContent).toContain("second");

    fireEvent.click(screen.getByTestId("toast-dismiss"));
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("FE-6: the default tone is the danger accent; an explicit success tone uses the ok accent", () => {
    render(<Toast />);
    act(() => useAppStore.getState().showToast("broke"));
    let alert = screen.getByRole("alert");
    expect(alert.getAttribute("data-tone")).toBe("error");
    expect(alert.style.boxShadow).toContain("var(--danger)");

    act(() => useAppStore.getState().dismissToast());
    act(() => useAppStore.getState().showToast("idea saved", "success"));
    alert = screen.getByRole("alert");
    expect(alert.getAttribute("data-tone")).toBe("success");
    expect(alert.style.boxShadow).toContain("var(--ok)");
  });

  it("FE-6: the tone advances with the queue head, not just the message", () => {
    render(<Toast />);
    act(() => useAppStore.getState().showToast("saved", "success"));
    act(() => useAppStore.getState().showToast("broke"));
    expect(screen.getByRole("alert").getAttribute("data-tone")).toBe("success");
    fireEvent.click(screen.getByTestId("toast-dismiss"));
    expect(screen.getByRole("alert").getAttribute("data-tone")).toBe("error");
  });
});
