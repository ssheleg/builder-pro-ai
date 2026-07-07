// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, fireEvent, act } from "@testing-library/react";

const upgradeDaemonMock = vi.fn();
vi.mock("../ipc/commands", () => ({
  upgradeDaemon: (...a: unknown[]) => upgradeDaemonMock(...a),
}));

import { UpgradeDialog } from "./UpgradeDialog";
import { useAppStore } from "../store/store";
import type { SessionMeta } from "../ipc/types";

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
      screen.getByText(
        "Обновить фоновый сервис — 3 живых сессий завершатся. Их записи и scrollback сохранены и появятся снова как неактивные.",
      ),
    ).toBeTruthy();
  });

  describe("finding [14]: unknown-count consent copy when the store never hydrated", () => {
    const COUNTED_PREFIX = "Обновить фоновый сервис — ";
    const UNCOUNTED_COPY =
      "Обновить фоновый сервис — все его живые сессии завершатся. Их записи и scrollback сохранены и появятся снова как неактивные.";

    it("NOT hydrated + empty sessions -> uncounted honest copy (counted string absent)", () => {
      useAppStore.setState(
        { sessions: {}, daemonIncompatible: true, upgradeDialogOpen: true, hydrated: false },
        false,
      );
      render(<UpgradeDialog />);
      expect(screen.getByText(UNCOUNTED_COPY)).toBeTruthy();
      expect(screen.queryByText(/живых сессий завершатся\./)).toBeNull();
      expect(screen.queryByText(new RegExp(`^${COUNTED_PREFIX}\\d`))).toBeNull();
    });

    it("hydrated + 0 active sessions -> counted copy with 0 (genuinely honest, distinct from uncounted)", () => {
      useAppStore.setState(
        { sessions: {}, daemonIncompatible: true, upgradeDialogOpen: true, hydrated: true },
        false,
      );
      render(<UpgradeDialog />);
      expect(
        screen.getByText(
          "Обновить фоновый сервис — 0 живых сессий завершатся. Их записи и scrollback сохранены и появятся снова как неактивные.",
        ),
      ).toBeTruthy();
      expect(screen.queryByText(UNCOUNTED_COPY)).toBeNull();
    });
  });

  it('"Обновить" calls upgradeDaemon once, fire-and-forget (not awaited)', () => {
    useAppStore.setState({ daemonIncompatible: true, upgradeDialogOpen: true }, false);
    render(<UpgradeDialog />);
    fireEvent.click(screen.getByRole("button", { name: "Обновить" }));
    expect(upgradeDaemonMock).toHaveBeenCalledTimes(1);
    expect(upgradeDaemonMock).toHaveBeenCalledWith();
  });

  it('"Отмена" closes the dialog but does NOT clear daemonIncompatible (honesty invariant)', () => {
    useAppStore.setState({ daemonIncompatible: true, upgradeDialogOpen: true }, false);
    render(<UpgradeDialog />);
    fireEvent.click(screen.getByRole("button", { name: "Отмена" }));
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
        fireEvent.click(screen.getByRole("button", { name: "Обновить" }));
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

      const btn = screen.getByRole("button", { name: "Обновить" });
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
        fireEvent.click(screen.getByRole("button", { name: "Обновить" }));
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
});
