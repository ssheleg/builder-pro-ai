// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, fireEvent, act } from "@testing-library/react";

const upgradeDaemonMock = vi.fn();
vi.mock("../ipc/commands", () => ({
  upgradeDaemon: (...a: unknown[]) => upgradeDaemonMock(...a),
}));

const orchdUpgradeMock = vi.fn();
vi.mock("../ipc/orchd", () => ({
  orchdUpgrade: (...a: unknown[]) => orchdUpgradeMock(...a),
}));

import { UpgradeDialog } from "./UpgradeDialog";
import { useAppStore } from "../store/store";
import type { SessionMeta } from "../ipc/types";
import { strings } from "../strings";

const meta = (over: Partial<SessionMeta> = {}): SessionMeta => ({
  id: "s1",
  workspaceId: "w1",
  title: "zsh",
  shell: "/bin/zsh",
  cwd: "/tmp",
  cols: 80,
  rows: 24,
  lifecycle: { kind: "atPrompt" },
  waitingForInput: false,
  isActive: true,
  createdAt: 1,
  ...over,
});

afterEach(cleanup);
beforeEach(() => {
  upgradeDaemonMock.mockReset();
  upgradeDaemonMock.mockReturnValue(new Promise(() => {})); // never resolves, like the real command
  orchdUpgradeMock.mockReset();
  orchdUpgradeMock.mockReturnValue(new Promise(() => {})); // never resolves, mirrors upgradeDaemon
  useAppStore.setState(
    {
      sessions: {},
      workspaces: {},
      activeSessionId: null,
      daemonConnected: true,
      daemonIncompatible: false,
      upgradeDialogOpen: false,
      upgradeError: null,
      hydrated: false,
      orchdIncompatible: false,
      orchdUpgradeDialogOpen: false,
    },
    false,
  );
});

describe("UpgradeDialog", () => {
  it("renders nothing when daemonIncompatible and upgradeDialogOpen are not both true", () => {
    useAppStore.setState({ daemonIncompatible: false, upgradeDialogOpen: false }, false);
    const { container: c1 } = render(<UpgradeDialog />);
    expect(c1.firstChild).toBeNull();
    cleanup();

    useAppStore.setState({ daemonIncompatible: true, upgradeDialogOpen: false }, false);
    const { container: c2 } = render(<UpgradeDialog />);
    expect(c2.firstChild).toBeNull();
    cleanup();

    useAppStore.setState({ daemonIncompatible: false, upgradeDialogOpen: true }, false);
    const { container: c3 } = render(<UpgradeDialog />);
    expect(c3.firstChild).toBeNull();
  });

  it("shows the consent copy with N = count of isActive sessions when hydrated", () => {
    useAppStore.setState(
      {
        sessions: {
          s1: meta({ id: "s1", isActive: true }),
          s2: meta({ id: "s2", isActive: true }),
          s3: meta({ id: "s3", isActive: true }),
          s4: meta({ id: "s4", isActive: false }),
        },
        daemonIncompatible: true,
        upgradeDialogOpen: true,
        hydrated: true,
      },
      false,
    );
    render(<UpgradeDialog />);
    expect(
      screen.getByText(strings.chrome.upgrade.daemonDetail(3)),
    ).toBeTruthy();
  });

  describe("finding [14]: unknown-count consent copy when the store never hydrated", () => {
    const UNCOUNTED_COPY = strings.chrome.upgrade.daemonDetailAll;

    it("NOT hydrated + empty sessions -> uncounted honest copy (counted string absent)", () => {
      useAppStore.setState(
        { sessions: {}, daemonIncompatible: true, upgradeDialogOpen: true, hydrated: false },
        false,
      );
      render(<UpgradeDialog />);
      expect(screen.getByText(UNCOUNTED_COPY)).toBeTruthy();
      expect(screen.queryByText(/\d+ live sessions will end\./)).toBeNull();
      expect(screen.queryByText(new RegExp(`^Update the background service — \\d`))).toBeNull();
    });

    it("hydrated + 0 active sessions -> counted copy with 0 (genuinely honest, distinct from uncounted)", () => {
      useAppStore.setState(
        { sessions: {}, daemonIncompatible: true, upgradeDialogOpen: true, hydrated: true },
        false,
      );
      render(<UpgradeDialog />);
      expect(
        screen.getByText(strings.chrome.upgrade.daemonDetail(0)),
      ).toBeTruthy();
      expect(screen.queryByText(UNCOUNTED_COPY)).toBeNull();
    });
  });

  it('"Update" calls upgradeDaemon once, fire-and-forget (not awaited)', () => {
    useAppStore.setState({ daemonIncompatible: true, upgradeDialogOpen: true }, false);
    render(<UpgradeDialog />);
    fireEvent.click(screen.getByRole("button", { name: strings.common.update }));
    expect(upgradeDaemonMock).toHaveBeenCalledTimes(1);
    expect(upgradeDaemonMock).toHaveBeenCalledWith();
  });

  it('"Cancel" closes the dialog but does NOT clear daemonIncompatible (honesty invariant)', () => {
    useAppStore.setState({ daemonIncompatible: true, upgradeDialogOpen: true }, false);
    render(<UpgradeDialog />);
    fireEvent.click(screen.getByRole("button", { name: strings.common.cancel }));
    expect(useAppStore.getState().upgradeDialogOpen).toBe(false);
    expect(useAppStore.getState().daemonIncompatible).toBe(true);
  });

  describe("finding [13]: UpgradeFailed must not be silently discarded", () => {
    it("a rejected upgradeDaemon (UpgradeFailed) renders an honest error line, dialog stays open", async () => {
      upgradeDaemonMock.mockReset();
      upgradeDaemonMock.mockRejectedValue({
        kind: "upgradeFailed",
        reason: "Operation not permitted",
      });
      useAppStore.setState({ daemonIncompatible: true, upgradeDialogOpen: true }, false);
      render(<UpgradeDialog />);

      await act(async () => {
        fireEvent.click(screen.getByRole("button", { name: strings.common.update }));
        // let the rejection's microtask settle
        await Promise.resolve();
        await Promise.resolve();
      });

      expect(screen.getByText(/Operation not permitted/)).toBeTruthy();
      // dialog must still be open/visible for retry
      expect(screen.getByRole("dialog")).toBeTruthy();
      expect(useAppStore.getState().upgradeDialogOpen).toBe(true);
    });

    it("second click retries: upgradeDaemon is called twice, primary button stays enabled", async () => {
      upgradeDaemonMock.mockReset();
      upgradeDaemonMock.mockRejectedValue({
        kind: "upgradeFailed",
        reason: "Operation not permitted",
      });
      useAppStore.setState({ daemonIncompatible: true, upgradeDialogOpen: true }, false);
      render(<UpgradeDialog />);

      const btn = screen.getByRole("button", { name: strings.common.update });
      await act(async () => {
        fireEvent.click(btn);
        await Promise.resolve();
        await Promise.resolve();
      });
      expect(btn).not.toHaveProperty("disabled", true);

      await act(async () => {
        fireEvent.click(btn);
        await Promise.resolve();
        await Promise.resolve();
      });
      expect(upgradeDaemonMock).toHaveBeenCalledTimes(2);
    });

    it("a never-resolving upgradeDaemon (happy path) shows no error line and does not crash", async () => {
      upgradeDaemonMock.mockReset();
      upgradeDaemonMock.mockReturnValue(new Promise(() => {}));
      useAppStore.setState({ daemonIncompatible: true, upgradeDialogOpen: true }, false);
      render(<UpgradeDialog />);

      await act(async () => {
        fireEvent.click(screen.getByRole("button", { name: strings.common.update }));
        await Promise.resolve();
        await Promise.resolve();
      });

      expect(screen.queryByText(/Operation not permitted/)).toBeNull();
      expect(useAppStore.getState().upgradeError).toBeNull();
    });

    it("reopening the dialog fresh (setUpgradeDialogOpen(true)) clears a stale upgradeError", () => {
      useAppStore.setState(
        { daemonIncompatible: true, upgradeDialogOpen: false, upgradeError: "stale error" },
        false,
      );
      act(() => {
        useAppStore.getState().setUpgradeDialogOpen(true);
      });
      expect(useAppStore.getState().upgradeError).toBeNull();
    });
  });

  describe("S3 T19: dual-daemon generalization (spec §10/§11)", () => {
    it("renders nothing when orchdIncompatible and orchdUpgradeDialogOpen are not both true", () => {
      useAppStore.setState({ orchdIncompatible: false, orchdUpgradeDialogOpen: false }, false);
      const { container: c1 } = render(<UpgradeDialog />);
      expect(c1.firstChild).toBeNull();
      cleanup();

      useAppStore.setState({ orchdIncompatible: true, orchdUpgradeDialogOpen: false }, false);
      const { container: c2 } = render(<UpgradeDialog />);
      expect(c2.firstChild).toBeNull();
    });

    it("orchd-incompatible ALONE renders the orchd copy (no live-session warning) and confirm calls orchdUpgrade()", () => {
      useAppStore.setState({ orchdIncompatible: true, orchdUpgradeDialogOpen: true }, false);
      render(<UpgradeDialog />);

      expect(screen.getByTestId("orchd-upgrade-dialog")).toBeTruthy();
      expect(
        screen.getByText(strings.chrome.upgrade.orchdBody),
      ).toBeTruthy();
      // no sessiond live-session copy leaks into the orchd variant
      expect(screen.queryByText(/live sessions/)).toBeNull();

      fireEvent.click(screen.getByRole("button", { name: strings.common.update }));
      expect(orchdUpgradeMock).toHaveBeenCalledTimes(1);
      expect(orchdUpgradeMock).toHaveBeenCalledWith();
      expect(upgradeDaemonMock).not.toHaveBeenCalled();
    });

    it("both daemons incompatible at once renders the SESSIOND copy (precedence), never the orchd one", () => {
      useAppStore.setState(
        {
          daemonIncompatible: true,
          upgradeDialogOpen: true,
          orchdIncompatible: true,
          orchdUpgradeDialogOpen: true,
          hydrated: false,
        },
        false,
      );
      render(<UpgradeDialog />);

      expect(screen.queryByTestId("orchd-upgrade-dialog")).toBeNull();
      expect(
        screen.getByText(strings.chrome.upgrade.daemonDetailAll),
      ).toBeTruthy();
      // exactly ONE dialog rendered, not two
      expect(screen.getAllByRole("dialog")).toHaveLength(1);

      fireEvent.click(screen.getByRole("button", { name: strings.common.update }));
      expect(upgradeDaemonMock).toHaveBeenCalledTimes(1);
      expect(orchdUpgradeMock).not.toHaveBeenCalled();
    });

    it('"Cancel" on the orchd variant closes it but does NOT clear orchdIncompatible (honesty invariant)', () => {
      useAppStore.setState({ orchdIncompatible: true, orchdUpgradeDialogOpen: true }, false);
      render(<UpgradeDialog />);
      fireEvent.click(screen.getByRole("button", { name: strings.common.cancel }));
      expect(useAppStore.getState().orchdUpgradeDialogOpen).toBe(false);
      expect(useAppStore.getState().orchdIncompatible).toBe(true);
    });

    it("a rejected orchdUpgrade renders an honest error line on the orchd variant, dialog stays open", async () => {
      orchdUpgradeMock.mockReset();
      orchdUpgradeMock.mockRejectedValue({ kind: "upgradeFailed", reason: "Operation not permitted" });
      useAppStore.setState({ orchdIncompatible: true, orchdUpgradeDialogOpen: true }, false);
      render(<UpgradeDialog />);

      await act(async () => {
        fireEvent.click(screen.getByRole("button", { name: strings.common.update }));
        await Promise.resolve();
        await Promise.resolve();
      });

      expect(screen.getByText(/Operation not permitted/)).toBeTruthy();
      expect(screen.getByTestId("orchd-upgrade-dialog")).toBeTruthy();
      expect(useAppStore.getState().orchdUpgradeDialogOpen).toBe(true);
    });

    it("once the sessiond dialog is dismissed (Cancel), a still-pending orchd incompatibility shows its own dialog next", () => {
      useAppStore.setState(
        {
          daemonIncompatible: true,
          upgradeDialogOpen: true,
          orchdIncompatible: true,
          orchdUpgradeDialogOpen: true,
        },
        false,
      );
      render(<UpgradeDialog />);
      expect(screen.queryByTestId("orchd-upgrade-dialog")).toBeNull();

      fireEvent.click(screen.getByRole("button", { name: strings.common.cancel }));

      expect(screen.getByTestId("orchd-upgrade-dialog")).toBeTruthy();
      // sessiond's own honesty invariant still holds — Cancel never clears daemonIncompatible
      expect(useAppStore.getState().daemonIncompatible).toBe(true);
    });
  });
});
