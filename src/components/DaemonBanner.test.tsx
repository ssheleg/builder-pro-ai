// @vitest-environment jsdom
import { describe, it, expect, afterEach, beforeEach } from "vitest";
import { render, screen, cleanup, act, fireEvent } from "@testing-library/react";
import { DaemonBanner } from "./DaemonBanner";
import { useAppStore } from "../store/store";
import { strings } from "../strings";

afterEach(cleanup);
beforeEach(() => {
  useAppStore.setState(
    {
      sessions: {},
      workspaces: {},
      activeSessionId: null,
      daemonConnected: true,
      daemonIncompatible: false,
      upgradeDialogOpen: false,
    },
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

  it('shows an honest "outdated / update required" message (NOT "reconnecting…") when daemonIncompatible is true', () => {
    act(() => useAppStore.setState({ daemonIncompatible: true, daemonConnected: false }, false));
    render(<DaemonBanner />);
    const banner = screen.getByRole("alert");
    expect(banner.textContent).toMatch(/outdated/i);
    expect(banner.textContent).not.toMatch(/reconnect/i);
  });

  it('exposes an "Update" action that calls setUpgradeDialogOpen(true) when daemonIncompatible', () => {
    act(() => useAppStore.setState({ daemonIncompatible: true, daemonConnected: false }, false));
    render(<DaemonBanner />);
    fireEvent.click(screen.getByRole("button", { name: strings.common.update }));
    expect(useAppStore.getState().upgradeDialogOpen).toBe(true);
  });

  it('falls back to the existing "reconnecting…" copy when daemonIncompatible is false and daemonConnected is false', () => {
    act(() =>
      useAppStore.setState({ daemonIncompatible: false, daemonConnected: false }, false),
    );
    render(<DaemonBanner />);
    expect(screen.getByRole("alert").textContent).toMatch(/reconnect/i);
  });

  it("renders nothing when connected and not incompatible", () => {
    act(() =>
      useAppStore.setState({ daemonIncompatible: false, daemonConnected: true }, false),
    );
    render(<DaemonBanner />);
    expect(screen.queryByRole("alert")).toBeNull();
  });
});
