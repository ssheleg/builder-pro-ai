// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup } from "@testing-library/react";

// The Inbox's job is orchestration: mount the two lists in ORPHAN mode and refresh both slices.
// The lists' own behavior (rows, link-to-project, spawn) is covered by their dedicated suites,
// so they are stubbed here to observe the exact `projectId` they receive.
vi.mock("./IdeasList", () => ({
  IdeasList: (p: { projectId: string | null }) => (
    <div data-testid="ideas-list-stub" data-project-id={String(p.projectId)} />
  ),
}));
vi.mock("./InsightsList", () => ({
  InsightsList: (p: { projectId: string | null }) => (
    <div data-testid="insights-list-stub" data-project-id={String(p.projectId)} />
  ),
}));
vi.mock("../ipc/orchd", () => ({
  orchdListIdeas: vi.fn().mockResolvedValue([]),
  orchdListInsights: vi.fn().mockResolvedValue([]),
  orchdReconnect: vi.fn().mockResolvedValue(undefined),
  describeOrchdError: vi.fn(() => "orchestrator: error"),
}));

import { InboxPanel } from "./InboxPanel";
import { useAppStore } from "../store/store";
import { strings } from "../strings";

afterEach(cleanup);
beforeEach(() => {
  useAppStore.setState({ orchdDown: false, ideas: [], insights: [] });
});

describe("InboxPanel (SCN-028 / AUD-2026-07-19-11)", () => {
  it("renders title, subtitle, and BOTH lists in orphan mode (projectId=null)", () => {
    render(<InboxPanel />);
    expect(screen.getByText(strings.inbox.title)).toBeTruthy();
    expect(screen.getByText(strings.inbox.subtitle)).toBeTruthy();
    expect(screen.getByTestId("ideas-list-stub").getAttribute("data-project-id")).toBe("null");
    expect(screen.getByTestId("insights-list-stub").getAttribute("data-project-id")).toBe("null");
  });

  it("refreshes both slices on mount (eager, mirrors ProjectPanel)", () => {
    const refreshIdeas = vi.fn().mockResolvedValue(undefined);
    const refreshInsights = vi.fn().mockResolvedValue(undefined);
    useAppStore.setState({ refreshIdeas, refreshInsights });
    render(<InboxPanel />);
    expect(refreshIdeas).toHaveBeenCalledTimes(1);
    expect(refreshInsights).toHaveBeenCalledTimes(1);
  });

  it("shows the shared orchd-down banner while degraded", () => {
    useAppStore.setState({ orchdDown: true });
    render(<InboxPanel />);
    expect(screen.getByText(strings.chrome.orchdUnavailable)).toBeTruthy();
  });
});
