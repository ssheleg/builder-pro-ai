// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, cleanup, fireEvent, act } from "@testing-library/react";

import { TerminalPane } from "./TerminalPane";
import type { TerminalManager } from "../terminal/terminal-manager";
import { strings } from "../strings";

afterEach(cleanup);

/**
 * Structural stub of the manager surface TerminalPane touches. Attach-error state is a plain
 * variable + subscriber set mirroring the real `subscribeAttachErrors` contract, so the
 * `useSyncExternalStore` wiring is exercised for real (subscribe → notify → re-read).
 */
function makeManagerStub() {
  let attachError: string | undefined;
  const listeners = new Set<() => void>();
  const stub = {
    ensure: vi.fn(),
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
  };
  return stub;
}

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
