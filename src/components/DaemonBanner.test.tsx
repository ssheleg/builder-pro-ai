// @vitest-environment jsdom
import { describe, it, expect, afterEach, beforeEach } from "vitest";
import { render, screen, cleanup, act } from "@testing-library/react";
import { DaemonBanner } from "./DaemonBanner";
import { useAppStore } from "../store/store";

afterEach(cleanup);
beforeEach(() => {
  useAppStore.setState(
    { sessions: {}, workspaces: {}, activeSessionId: null, daemonConnected: true },
    false,
  );
});

describe("DaemonBanner", () => {
  it("hides when the daemon is connected", () => {
    act(() => useAppStore.getState().setDaemonConnected(true));
    render(<DaemonBanner />);
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("shows when the daemon is disconnected", () => {
    act(() => useAppStore.getState().setDaemonConnected(false));
    render(<DaemonBanner />);
    const banner = screen.getByRole("alert");
    expect(banner.textContent).toMatch(/reconnect/i);
  });

  it("reactively appears then disappears as the flag flips (disconnected -> reconnected)", () => {
    act(() => useAppStore.getState().setDaemonConnected(false));
    render(<DaemonBanner />);
    expect(screen.getByRole("alert")).toBeTruthy();
    act(() => useAppStore.getState().setDaemonConnected(true));
    expect(screen.queryByRole("alert")).toBeNull();
  });
});
