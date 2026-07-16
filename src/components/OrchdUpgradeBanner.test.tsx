// @vitest-environment jsdom
import { describe, it, expect, afterEach, beforeEach } from "vitest";
import { render, screen, cleanup, act, fireEvent } from "@testing-library/react";
import { OrchdUpgradeBanner } from "./OrchdUpgradeBanner";
import { useAppStore } from "../store/store";
import { strings } from "../strings";

afterEach(cleanup);
beforeEach(() => {
  useAppStore.setState({ orchdIncompatible: false, orchdUpgradeDialogOpen: false }, false);
});

describe("OrchdUpgradeBanner (BL-96, spec D8)", () => {
  it("renders nothing when orchd is compatible", () => {
    render(<OrchdUpgradeBanner />);
    expect(screen.queryByRole("alert")).toBeNull();
  });

  it("renders nothing while the upgrade dialog is still open (dialog is the surface)", () => {
    act(() =>
      useAppStore.setState({ orchdIncompatible: true, orchdUpgradeDialogOpen: true }, false),
    );
    render(<OrchdUpgradeBanner />);
    expect(screen.queryByTestId("orchd-upgrade-banner")).toBeNull();
  });

  it("shows the outdated banner after the mandatory dialog was cancelled (incompatible && !dialogOpen)", () => {
    act(() =>
      useAppStore.setState({ orchdIncompatible: true, orchdUpgradeDialogOpen: false }, false),
    );
    render(<OrchdUpgradeBanner />);
    const banner = screen.getByTestId("orchd-upgrade-banner");
    expect(banner.textContent).toContain(strings.chrome.orchdOutdated);
  });

  it('exposes an "Update" button that re-opens the upgrade dialog', () => {
    act(() =>
      useAppStore.setState({ orchdIncompatible: true, orchdUpgradeDialogOpen: false }, false),
    );
    render(<OrchdUpgradeBanner />);
    fireEvent.click(screen.getByRole("button", { name: strings.common.update }));
    expect(useAppStore.getState().orchdUpgradeDialogOpen).toBe(true);
  });

  it("re-opening the dialog via the button hides the banner again (dialog re-shown)", () => {
    act(() =>
      useAppStore.setState({ orchdIncompatible: true, orchdUpgradeDialogOpen: false }, false),
    );
    render(<OrchdUpgradeBanner />);
    expect(screen.getByTestId("orchd-upgrade-banner")).toBeTruthy();
    act(() => useAppStore.getState().setOrchdUpgradeDialogOpen(true));
    expect(screen.queryByTestId("orchd-upgrade-banner")).toBeNull();
  });
});
