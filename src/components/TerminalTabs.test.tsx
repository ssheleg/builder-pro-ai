// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, act, fireEvent } from "@testing-library/react";

const createSessionMock = vi.fn();
const killSessionMock = vi.fn().mockResolvedValue(undefined);
vi.mock("../ipc/commands", () => ({
  createSession: (...a: unknown[]) => createSessionMock(...a),
  killSession: (...a: unknown[]) => killSessionMock(...a),
}));

import { TerminalTabs } from "./TerminalTabs";
import { useAppStore } from "../store/store";
import type { SessionMeta, Workspace } from "../ipc/types";

const disposeMock = vi.fn();
const fakeManager = {
  ensure: vi.fn(),
  has: vi.fn(() => true),
  get: vi.fn(),
  isOpened: vi.fn(() => true),
  pendingBytes: vi.fn(() => 0),
  attach: vi.fn().mockResolvedValue(undefined),
  applyReplay: vi.fn(),
  writeOutput: vi.fn(),
  open: vi.fn(),
  hide: vi.fn(),
  dispose: disposeMock,
  disposeAll: vi.fn(),
} as unknown as import("../terminal/terminal-manager").TerminalManager;

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
const ws: Workspace = { id: "w1", name: "proj", rootPath: "/p" };

afterEach(cleanup);
beforeEach(() => {
  createSessionMock.mockReset();
  createSessionMock.mockResolvedValue(meta({ id: "s3" }));
  disposeMock.mockReset();
  killSessionMock.mockClear();
  useAppStore.setState(
    {
      sessions: { s1: meta(), s2: meta({ id: "s2", title: "bash" }) },
      workspaces: { w1: ws },
      activeSessionId: "s1",
      daemonConnected: true,
    },
    false,
  );
});

describe("TerminalTabs", () => {
  it("renders one tab per session with its title", () => {
    render(<TerminalTabs manager={fakeManager} activeWorkspaceId="w1" />);
    expect(screen.getByRole("tab", { name: /zsh/i })).toBeTruthy();
    expect(screen.getByRole("tab", { name: /bash/i })).toBeTruthy();
  });

  it("marks the active session's tab aria-selected", () => {
    render(<TerminalTabs manager={fakeManager} activeWorkspaceId="w1" />);
    expect(screen.getByRole("tab", { name: /zsh/i }).getAttribute("aria-selected")).toBe("true");
    expect(screen.getByRole("tab", { name: /bash/i }).getAttribute("aria-selected")).toBe("false");
  });

  it("clicking a tab sets it active (and does NOT dispose the other terminal — keep-alive)", () => {
    render(<TerminalTabs manager={fakeManager} activeWorkspaceId="w1" />);
    act(() => {
      fireEvent.click(screen.getByRole("tab", { name: /bash/i }));
    });
    expect(useAppStore.getState().activeSessionId).toBe("s2");
    expect(disposeMock).not.toHaveBeenCalled();
  });

  it("new-terminal button calls createSession with the active workspace id", async () => {
    render(<TerminalTabs manager={fakeManager} activeWorkspaceId="w1" />);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /new terminal/i }));
    });
    expect(createSessionMock).toHaveBeenCalledTimes(1);
    expect(createSessionMock).toHaveBeenCalledWith("w1", { cols: 80, rows: 24 });
  });

  it("new-terminal button is disabled when there is no active workspace", () => {
    render(<TerminalTabs manager={fakeManager} activeWorkspaceId={null} />);
    const button = screen.getByRole("button", { name: /new terminal/i }) as HTMLButtonElement;
    expect(button.disabled).toBe(true);
  });

  it("closing a tab kills the session and disposes its terminal", async () => {
    render(<TerminalTabs manager={fakeManager} activeWorkspaceId="w1" />);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: /close zsh/i }));
    });
    expect(killSessionMock).toHaveBeenCalledWith("s1");
    expect(disposeMock).toHaveBeenCalledWith("s1");
  });
});
