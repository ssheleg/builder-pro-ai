// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { render, screen, cleanup, fireEvent, act } from "@testing-library/react";

import { TerminalPane } from "./TerminalPane";
import { useAppStore } from "../store/store";
import type { TerminalManager } from "../terminal/terminal-manager";
import type { SessionMeta } from "../ipc/types";
import { strings } from "../strings";

afterEach(cleanup);
beforeEach(() => {
  useAppStore.setState({ sessions: {}, toast: null, toastQueue: [] }, false);
});

/**
 * Structural stub of the manager surface TerminalPane touches. Attach-error state is a plain
 * variable + subscriber set mirroring the real `subscribeAttachErrors` contract, so the
 * `useSyncExternalStore` wiring is exercised for real (subscribe → notify → re-read). `ensure`
 * returns a minimal xterm stub whose `onData` listener set is emittable by tests (FE-7 drives
 * keystrokes through it).
 */
function makeManagerStub() {
  let attachError: string | undefined;
  const listeners = new Set<() => void>();
  const dataListeners = new Set<(data: string) => void>();
  const termStub = {
    onData: (cb: (data: string) => void) => {
      dataListeners.add(cb);
      return { dispose: () => dataListeners.delete(cb) };
    },
  };
  const stub = {
    ensure: vi.fn(() => termStub),
    attach: vi.fn(() => Promise.resolve()),
    open: vi.fn(),
    hide: vi.fn(),
    getAttachError: vi.fn(() => attachError),
    subscribeAttachErrors: (cb: () => void) => {
      listeners.add(cb);
      return () => listeners.delete(cb);
    },
    setAttachError(msg: string | undefined) {
      attachError = msg;
      for (const cb of listeners) cb();
    },
    /** Simulate the owner typing into the xterm (fires every registered onData listener). */
    type(data: string) {
      for (const cb of dataListeners) cb(data);
    },
  };
  return stub;
}

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

describe("TerminalPane attach-failure overlay (SCN-044 / AUD-2026-07-19-01)", () => {
  it("renders no overlay while healthy, and mounts the terminal lifecycle", () => {
    const m = makeManagerStub();
    render(<TerminalPane sessionId="s1" manager={m as unknown as TerminalManager} />);
    expect(screen.queryByTestId("terminal-attach-error")).toBeNull();
    expect(m.ensure).toHaveBeenCalledWith("s1");
    expect(m.attach).toHaveBeenCalledWith("s1");
    expect(m.open).toHaveBeenCalled();
  });

  it("shows the honest error note when the manager reports a failed attach", () => {
    const m = makeManagerStub();
    render(<TerminalPane sessionId="s1" manager={m as unknown as TerminalManager} />);
    act(() => m.setAttachError("daemon disconnected"));
    const overlay = screen.getByTestId("terminal-attach-error");
    expect(overlay.textContent).toContain(
      strings.terminal.attachFailed("daemon disconnected"),
    );
  });

  it("Retry re-invokes manager.attach for THIS session, and the overlay clears on recovery", () => {
    const m = makeManagerStub();
    render(<TerminalPane sessionId="s1" manager={m as unknown as TerminalManager} />);
    act(() => m.setAttachError("boom"));
    m.attach.mockClear();
    fireEvent.click(screen.getByTestId("terminal-attach-retry"));
    expect(m.attach).toHaveBeenCalledWith("s1");
    act(() => m.setAttachError(undefined));
    expect(screen.queryByTestId("terminal-attach-error")).toBeNull();
  });
});

describe("TerminalPane restored-session input hint (FE-7)", () => {
  it("typing into a RESTORED session (isActive:false, not exited) shows the hint ONCE per session", () => {
    const m = makeManagerStub();
    useAppStore.setState(
      { sessions: { s1: meta({ isActive: false }) }, toast: null, toastQueue: [] },
      false,
    );
    render(<TerminalPane sessionId="s1" manager={m as unknown as TerminalManager} />);

    act(() => m.type("l"));
    expect(useAppStore.getState().toastQueue).toEqual([strings.terminal.restoredInputHint]);

    // A second keystroke in the SAME session is not re-announced.
    act(() => m.type("s"));
    expect(useAppStore.getState().toastQueue).toHaveLength(1);
  });

  it("typing into a LIVE session never shows the hint", () => {
    const m = makeManagerStub();
    useAppStore.setState(
      { sessions: { s1: meta({ isActive: true }) }, toast: null, toastQueue: [] },
      false,
    );
    render(<TerminalPane sessionId="s1" manager={m as unknown as TerminalManager} />);
    act(() => m.type("ls"));
    expect(useAppStore.getState().toastQueue).toHaveLength(0);
  });

  it("typing into an EXITED session never shows the hint (the exited surface already says so)", () => {
    const m = makeManagerStub();
    useAppStore.setState(
      {
        sessions: {
          s1: meta({ isActive: false, lifecycle: { kind: "exited", code: 0, signal: null } }),
        },
        toast: null,
        toastQueue: [],
      },
      false,
    );
    render(<TerminalPane sessionId="s1" manager={m as unknown as TerminalManager} />);
    act(() => m.type("x"));
    expect(useAppStore.getState().toastQueue).toHaveLength(0);
  });
});
