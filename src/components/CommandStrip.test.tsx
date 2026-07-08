// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, act } from "@testing-library/react";

const getCommandEventsMock = vi.fn();
vi.mock("../ipc/commands", () => ({
  getCommandEvents: (...a: unknown[]) => getCommandEventsMock(...a),
}));

import { CommandStrip } from "./CommandStrip";
import { useAppStore } from "../store/store";
import type { SessionMeta, CommandEvent } from "../ipc/types";

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

const ev = (over: Partial<CommandEvent>): CommandEvent => ({
  sessionId: "s1",
  seq: 0,
  ts: 0,
  kind: "started",
  exitCode: null,
  origin: "typed",
  ...over,
});

afterEach(cleanup);
beforeEach(() => {
  getCommandEventsMock.mockReset();
  useAppStore.setState({ sessions: { s1: meta() }, toast: null }, false);
});

describe("CommandStrip", () => {
  it("renders a ✓ chip for a finished/exitCode:0 event, paired with its adjacent started (no separate running dot)", async () => {
    // newest-first: finished(seq=2) immediately followed by its own started(seq=1).
    getCommandEventsMock.mockResolvedValue([
      ev({ seq: 2, kind: "finished", exitCode: 0 }),
      ev({ seq: 1, kind: "started" }),
    ]);
    await act(async () => {
      render(<CommandStrip sessionId="s1" />);
    });
    expect(screen.getByTestId("command-strip")).toBeTruthy();
    expect(screen.getAllByRole("listitem")).toHaveLength(1);
    expect(screen.getByTestId("command-chip-ok").textContent).toBe("✓");
    expect(screen.queryByTestId("command-chip-running")).toBeNull();
  });

  it("renders a ✗ chip with the exit code for a finished/exitCode!=0 event", async () => {
    getCommandEventsMock.mockResolvedValue([
      ev({ seq: 2, kind: "finished", exitCode: 1 }),
      ev({ seq: 1, kind: "started" }),
    ]);
    await act(async () => {
      render(<CommandStrip sessionId="s1" />);
    });
    expect(screen.getByTestId("command-chip-fail").textContent).toBe("✗ 1");
  });

  it("renders a running dot for a lone unmatched started (the in-flight command)", async () => {
    getCommandEventsMock.mockResolvedValue([ev({ seq: 3, kind: "started" })]);
    await act(async () => {
      render(<CommandStrip sessionId="s1" />);
    });
    expect(screen.getByTestId("command-chip-running")).toBeTruthy();
    expect(screen.getByRole("img", { name: /running/i })).toBeTruthy();
  });

  it("newest-first: an in-flight command plus a completed one both render, in fetch order", async () => {
    getCommandEventsMock.mockResolvedValue([
      ev({ seq: 4, kind: "started" }), // currently running, no finished yet
      ev({ seq: 3, kind: "finished", exitCode: 0 }),
      ev({ seq: 2, kind: "started" }),
    ]);
    await act(async () => {
      render(<CommandStrip sessionId="s1" />);
    });
    const items = screen.getAllByRole("listitem");
    expect(items).toHaveLength(2);
    expect(items[0].getAttribute("data-testid")).toBe("command-chip-running");
    expect(items[1].getAttribute("data-testid")).toBe("command-chip-ok");
  });

  it("empty events: renders a calm dim placeholder, not an error, and never toasts", async () => {
    getCommandEventsMock.mockResolvedValue([]);
    await act(async () => {
      render(<CommandStrip sessionId="s1" />);
    });
    expect(screen.getByTestId("command-strip-empty")).toBeTruthy();
    expect(useAppStore.getState().toast).toBeNull();
  });

  it("a rejected getCommandEvents fires a toast and renders nothing", async () => {
    getCommandEventsMock.mockRejectedValue(new Error("boom"));
    const { container } = await act(async () => render(<CommandStrip sessionId="s1" />));
    expect(container.innerHTML).toBe("");
    expect(useAppStore.getState().toast).toMatch(/истори/i);
  });

  it("refetches when the active session's lifecycle changes (state-changed)", async () => {
    getCommandEventsMock.mockResolvedValue([]);
    await act(async () => {
      render(<CommandStrip sessionId="s1" />);
    });
    expect(getCommandEventsMock).toHaveBeenCalledTimes(1);

    await act(async () => {
      useAppStore.getState().setLifecycle({
        sessionId: "s1",
        lifecycle: { kind: "running" },
        waitingForInput: false,
        cwd: "/tmp",
      });
    });
    expect(getCommandEventsMock).toHaveBeenCalledTimes(2);
  });

  it("refetches when the active session exits (session://exited)", async () => {
    getCommandEventsMock.mockResolvedValue([]);
    await act(async () => {
      render(<CommandStrip sessionId="s1" />);
    });
    expect(getCommandEventsMock).toHaveBeenCalledTimes(1);

    await act(async () => {
      useAppStore.getState().markExited({ sessionId: "s1", code: 0, signal: null });
    });
    expect(getCommandEventsMock).toHaveBeenCalledTimes(2);
  });

  it("does not refetch for an unrelated session's lifecycle change", async () => {
    getCommandEventsMock.mockResolvedValue([]);
    useAppStore.setState({ sessions: { s1: meta(), s2: meta({ id: "s2" }) } }, false);
    await act(async () => {
      render(<CommandStrip sessionId="s1" />);
    });
    expect(getCommandEventsMock).toHaveBeenCalledTimes(1);

    await act(async () => {
      useAppStore.getState().setLifecycle({
        sessionId: "s2",
        lifecycle: { kind: "running" },
        waitingForInput: false,
        cwd: "/tmp",
      });
    });
    expect(getCommandEventsMock).toHaveBeenCalledTimes(1);
  });

  it("fetches with the documented limit (10)", async () => {
    getCommandEventsMock.mockResolvedValue([]);
    await act(async () => {
      render(<CommandStrip sessionId="s1" />);
    });
    expect(getCommandEventsMock).toHaveBeenCalledWith("s1", 10);
  });
});
