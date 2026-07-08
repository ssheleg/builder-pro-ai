// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, fireEvent } from "@testing-library/react";

import { HomeView } from "./HomeView";
import { useAppStore } from "../store/store";
import type { SessionMeta, Workspace } from "../ipc/types";
import type { TerminalManager } from "../terminal/terminal-manager";

const wsA: Workspace = { id: "w1", name: "alpha", rootPath: "/p/alpha", roots: ["/p/alpha"] };
const wsB: Workspace = { id: "w2", name: "beta", rootPath: "/p/beta", roots: ["/p/beta"] };

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

const focusMock = vi.fn();
const fakeManager = { focus: focusMock } as unknown as TerminalManager;

afterEach(cleanup);
beforeEach(() => {
  focusMock.mockReset();
  useAppStore.setState(
    {
      sessions: {},
      workspaces: {},
      activeSessionId: null,
      view: "home",
    },
    false,
  );
});

describe("HomeView", () => {
  it("pins a waitingForInput session above a running one, regardless of insertion order", () => {
    // Insert the RUNNING session first, the WAITING one second — DOM order must still put
    // waiting above running (spec §6.2: attention beats chronology / insertion order).
    useAppStore.setState({
      workspaces: { w1: wsA },
      sessions: {
        "s-run": meta({ id: "s-run", title: "running-one", lifecycle: { kind: "running" }, waitingForInput: false }),
        "s-wait": meta({ id: "s-wait", title: "waiting-one", lifecycle: { kind: "running" }, waitingForInput: true }),
      },
    });
    render(<HomeView manager={fakeManager} setActiveWorkspaceId={() => {}} />);
    const rows = Array.from(document.querySelectorAll('[data-testid^="home-row-"]')).map(
      (el) => el.getAttribute("data-testid"),
    );
    expect(rows.indexOf("home-row-s-wait")).toBeGreaterThanOrEqual(0);
    expect(rows.indexOf("home-row-s-run")).toBeGreaterThanOrEqual(0);
    expect(rows.indexOf("home-row-s-wait")).toBeLessThan(rows.indexOf("home-row-s-run"));
  });

  it('"Пройти" navigates: setActiveWorkspaceId + setView("workspace") + setActiveSession + manager.focus', () => {
    const setActiveWorkspaceId = vi.fn();
    useAppStore.setState({
      workspaces: { w1: wsA },
      sessions: {
        s1: meta({ id: "s1", workspaceId: "w1", waitingForInput: true, lifecycle: { kind: "running" } }),
      },
    });
    render(<HomeView manager={fakeManager} setActiveWorkspaceId={setActiveWorkspaceId} />);

    fireEvent.click(screen.getByRole("button", { name: /пройти/i }));

    expect(setActiveWorkspaceId).toHaveBeenCalledWith("w1");
    expect(useAppStore.getState().view).toBe("workspace");
    expect(useAppStore.getState().activeSessionId).toBe("s1");
    expect(focusMock).toHaveBeenCalledWith("s1");
  });

  it("clicking a running row does the same navigation as Пройти", () => {
    const setActiveWorkspaceId = vi.fn();
    useAppStore.setState({
      workspaces: { w1: wsA },
      sessions: {
        s1: meta({ id: "s1", workspaceId: "w1", isActive: true, waitingForInput: false, lifecycle: { kind: "running" } }),
      },
    });
    render(<HomeView manager={fakeManager} setActiveWorkspaceId={setActiveWorkspaceId} />);

    fireEvent.click(screen.getByTestId("home-row-s1"));

    expect(setActiveWorkspaceId).toHaveBeenCalledWith("w1");
    expect(useAppStore.getState().view).toBe("workspace");
    expect(useAppStore.getState().activeSessionId).toBe("s1");
    expect(focusMock).toHaveBeenCalledWith("s1");
  });

  it("a group-header click navigates to the workspace WITHOUT selecting a session", () => {
    const setActiveWorkspaceId = vi.fn();
    useAppStore.setState({
      workspaces: { w1: wsA },
      sessions: {
        s1: meta({ id: "s1", workspaceId: "w1", waitingForInput: true, lifecycle: { kind: "running" } }),
      },
      activeSessionId: null,
    });
    render(<HomeView manager={fakeManager} setActiveWorkspaceId={setActiveWorkspaceId} />);

    fireEvent.click(screen.getByRole("button", { name: "alpha" }));

    expect(setActiveWorkspaceId).toHaveBeenCalledWith("w1");
    expect(useAppStore.getState().view).toBe("workspace");
    expect(useAppStore.getState().activeSessionId).toBeNull();
    expect(focusMock).not.toHaveBeenCalled();
  });

  it("stats strip shows accurate workspace/live/waiting counts", () => {
    useAppStore.setState({
      workspaces: { w1: wsA, w2: wsB },
      sessions: {
        s1: meta({ id: "s1", workspaceId: "w1", waitingForInput: true, lifecycle: { kind: "running" } }),
        s2: meta({ id: "s2", workspaceId: "w1", isActive: true, waitingForInput: false, lifecycle: { kind: "running" } }),
        s3: meta({
          id: "s3",
          workspaceId: "w2",
          isActive: false,
          waitingForInput: false,
          lifecycle: { kind: "exited", code: 0, signal: null },
        }),
      },
    });
    render(<HomeView manager={fakeManager} setActiveWorkspaceId={() => {}} />);
    // 2 workspaces total; live = waiting(1) + running(1) = 2; waiting = 1. The exited session
    // does not count toward "live".
    expect(screen.getByTestId("home-stats").textContent).toBe("2 workspaces · 2 live · 1 waiting");
  });

  it("exited rows show ✓ for a zero exit code and ✗ (red) for a non-zero one", () => {
    useAppStore.setState({
      workspaces: { w1: wsA },
      sessions: {
        ok: meta({
          id: "ok",
          workspaceId: "w1",
          isActive: false,
          waitingForInput: false,
          lifecycle: { kind: "exited", code: 0, signal: null },
        }),
        bad: meta({
          id: "bad",
          workspaceId: "w1",
          title: "bad-one",
          isActive: false,
          waitingForInput: false,
          lifecycle: { kind: "exited", code: 1, signal: null },
        }),
      },
    });
    render(<HomeView manager={fakeManager} setActiveWorkspaceId={() => {}} />);
    expect(screen.getByTestId("home-row-ok").textContent).toContain("✓");
    expect(screen.getByTestId("home-row-ok").textContent).toContain("code 0");
    expect(screen.getByTestId("home-row-bad").textContent).toContain("✗");
    expect(screen.getByTestId("home-row-bad").textContent).toContain("code 1");
  });

  it("clicking an exited row also navigates to its workspace + session", () => {
    const setActiveWorkspaceId = vi.fn();
    useAppStore.setState({
      workspaces: { w1: wsA },
      sessions: {
        ok: meta({
          id: "ok",
          workspaceId: "w1",
          isActive: false,
          waitingForInput: false,
          lifecycle: { kind: "exited", code: 0, signal: null },
        }),
      },
    });
    render(<HomeView manager={fakeManager} setActiveWorkspaceId={setActiveWorkspaceId} />);
    fireEvent.click(screen.getByTestId("home-row-ok"));
    expect(setActiveWorkspaceId).toHaveBeenCalledWith("w1");
    expect(useAppStore.getState().view).toBe("workspace");
    expect(useAppStore.getState().activeSessionId).toBe("ok");
  });

  it("empty state: no sessions at all shows a dim sentence + an action to open the first workspace", () => {
    const setActiveWorkspaceId = vi.fn();
    useAppStore.setState({ workspaces: { w1: wsA }, sessions: {} });
    render(<HomeView manager={fakeManager} setActiveWorkspaceId={setActiveWorkspaceId} />);
    expect(screen.getByTestId("home-empty")).toBeTruthy();
    fireEvent.click(screen.getByRole("button", { name: /alpha/i }));
    expect(setActiveWorkspaceId).toHaveBeenCalledWith("w1");
    expect(useAppStore.getState().view).toBe("workspace");
  });

  it("empty state with zero workspaces shows only the dim sentence (no action)", () => {
    useAppStore.setState({ workspaces: {}, sessions: {} });
    render(<HomeView manager={fakeManager} setActiveWorkspaceId={() => {}} />);
    expect(screen.getByTestId("home-empty")).toBeTruthy();
    expect(screen.queryByRole("button")).toBeNull();
  });

  it("F1: an exited session that still carries a stale waitingForInput:true renders EXACTLY ONCE, in «Завершились недавно» (exited wins), never duplicated into «Нужен ты», and is excluded from the waiting/live stats", () => {
    useAppStore.setState({
      workspaces: { w1: wsA },
      sessions: {
        s1: meta({
          id: "s1",
          workspaceId: "w1",
          waitingForInput: true,
          isActive: false,
          lifecycle: { kind: "exited", code: 1, signal: null },
        }),
      },
    });
    render(<HomeView manager={fakeManager} setActiveWorkspaceId={() => {}} />);

    // exactly one row for this session, anywhere in the DOM
    expect(screen.getAllByTestId("home-row-s1")).toHaveLength(1);

    // it lives in "Завершились недавно", not "Нужен ты"
    expect(screen.queryByText("Нужен ты")).toBeNull();
    const row = screen.getByTestId("home-row-s1");
    expect(row.textContent).toContain("✗");
    expect(row.textContent).not.toContain("ждёт ввода");

    // the whole-store stats strip must not count a dead session as waiting or live
    expect(screen.getByTestId("home-stats").textContent).toBe("1 workspaces · 0 live · 0 waiting");
  });

  it("F2: an exited row's ✓/✗ glyph exposes an accessible name distinguishing success from failure", () => {
    useAppStore.setState({
      workspaces: { w1: wsA },
      sessions: {
        ok: meta({
          id: "ok",
          workspaceId: "w1",
          isActive: false,
          waitingForInput: false,
          lifecycle: { kind: "exited", code: 0, signal: null },
        }),
        bad: meta({
          id: "bad",
          workspaceId: "w1",
          title: "bad-one",
          isActive: false,
          waitingForInput: false,
          lifecycle: { kind: "exited", code: 1, signal: null },
        }),
      },
    });
    render(<HomeView manager={fakeManager} setActiveWorkspaceId={() => {}} />);

    expect(screen.getByRole("button", { name: /успешно/i })).toBe(screen.getByTestId("home-row-ok"));
    expect(screen.getByRole("button", { name: /с ошибкой/i })).toBe(screen.getByTestId("home-row-bad"));
  });

  it("a section is omitted entirely when it has no sessions", () => {
    useAppStore.setState({
      workspaces: { w1: wsA },
      sessions: {
        s1: meta({ id: "s1", workspaceId: "w1", isActive: true, waitingForInput: false, lifecycle: { kind: "running" } }),
      },
    });
    render(<HomeView manager={fakeManager} setActiveWorkspaceId={() => {}} />);
    expect(screen.queryByRole("region", { name: "Нужен ты" })).toBeNull();
    expect(screen.queryByText("Нужен ты")).toBeNull();
  });
});
