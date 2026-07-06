// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, fireEvent } from "@testing-library/react";

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

  it("shows the consent copy with N = count of isActive sessions", () => {
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
});
