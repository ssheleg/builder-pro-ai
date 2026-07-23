// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, act, fireEvent, waitFor } from "@testing-library/react";

// SCN-052/053: both wire calls are mocked — per-source failures must be independently
// controllable (usage fails while git succeeds and vice versa).
const statsUsageMock = vi.fn();
const statsGitMock = vi.fn();
vi.mock("../ipc/stats", () => ({
  statsUsage: (...a: unknown[]) => statsUsageMock(...a),
  statsGit: (...a: unknown[]) => statsGitMock(...a),
}));

import { StatsView, attributeCwd, fmtTokens } from "./StatsView";
import { useAppStore } from "../store/store";
import { strings } from "../strings";
import type { Project } from "../ipc/orchd-types";
import type { Workspace } from "../ipc/types";
import type { UsageStats, GitStats } from "../ipc/stats";

const wsA: Workspace = { id: "w1", name: "alpha", rootPath: "/p/alpha", roots: ["/p/alpha"] };
const wsNested: Workspace = {
  id: "w2",
  name: "nested",
  rootPath: "/p/alpha/sub",
  roots: ["/p/alpha/sub"],
};

function proj(over: Partial<Project>): Project {
  return {
    id: "p1",
    name: "Proj A",
    description: "",
    status: "active",
    workspaceIds: [],
    createdAt: 1,
    updatedAt: 1,
    ...over,
  };
}

const PROJECTS = [
  proj({ id: "p1", name: "alpha-proj", workspaceIds: ["w1"] }),
  proj({ id: "p2", name: "nested-proj", workspaceIds: ["w2"] }),
];
const WORKSPACES = { w1: wsA, w2: wsNested };

function usage(days: UsageStats["days"], error: string | null = null): UsageStats {
  return { asOf: 1_753_228_800_000, days, error };
}
function day(over: Partial<UsageStats["days"][0]>): UsageStats["days"][0] {
  return {
    day: "2026-07-20",
    cwd: "/p/alpha",
    tokensIn: 100,
    tokensOut: 200,
    cacheWrite: 0,
    cacheRead: 0,
    estCostUsd: 1.5,
    costComplete: true,
    sessions: 2,
    ...over,
  };
}
function gitRow(over: Partial<GitStats>): GitStats {
  return { root: "/p/alpha", commits: 3, added: 10, deleted: 4, available: true, reason: null, ...over };
}

afterEach(cleanup);
beforeEach(() => {
  statsUsageMock.mockReset().mockResolvedValue(usage([day({})]));
  statsGitMock.mockReset().mockResolvedValue([gitRow({})]);
  useAppStore.setState(
    {
      projects: PROJECTS,
      workspaces: WORKSPACES,
      stats: { range: "30d", usage: null, git: null, usageError: null, gitError: null, loading: false },
      toast: null,
      toastQueue: [],
      diagEvents: [],
    },
    false,
  );
});

describe("attributeCwd (SCN-052 attribution)", () => {
  it("longest workspace-root prefix wins — a nested workspace beats its parent", () => {
    expect(attributeCwd("/p/alpha/src", PROJECTS, WORKSPACES)).toBe("alpha-proj");
    expect(attributeCwd("/p/alpha/sub/deep", PROJECTS, WORKSPACES)).toBe("nested-proj");
    expect(attributeCwd("/p/alpha", PROJECTS, WORKSPACES)).toBe("alpha-proj");
  });

  it("unmatched cwd lands in the honest 'other' bucket — never dropped", () => {
    expect(attributeCwd("/elsewhere/x", PROJECTS, WORKSPACES)).toBe(strings.stats.otherBucket);
    // prefix must respect path boundaries: /p/alphabet is NOT under /p/alpha
    expect(attributeCwd("/p/alphabet", PROJECTS, WORKSPACES)).toBe(strings.stats.otherBucket);
  });
});

describe("fmtTokens", () => {
  it("formats compactly", () => {
    expect(fmtTokens(999)).toBe("999");
    expect(fmtTokens(1_200)).toBe("1.2k");
    expect(fmtTokens(2_400_000)).toBe("2.4M");
  });
});

describe("StatsView (SCN-052/053)", () => {
  it("fetches on first open and renders tiles + per-project row", async () => {
    render(<StatsView />);
    await waitFor(() => expect(statsUsageMock).toHaveBeenCalledWith("30d"));
    await waitFor(() => expect(screen.getByTestId("stats-tokens").textContent).toContain("300"));
    expect(screen.getByTestId("stats-cost").textContent).toContain("$1.50");
    const row = screen.getByTestId("stats-row-alpha-proj");
    expect(row.textContent).toContain("3 · +10 −4");
  });

  it("range pill switch refetches with the new range", async () => {
    render(<StatsView />);
    await waitFor(() => expect(statsUsageMock).toHaveBeenCalledTimes(1));
    await act(async () => {
      fireEvent.click(screen.getByRole("radio", { name: strings.stats.range7d }));
    });
    await waitFor(() => expect(statsUsageMock).toHaveBeenCalledWith("7d"));
    expect(useAppStore.getState().stats.range).toBe("7d");
  });

  it("workspace without git shows 'no git data' — never fabricated zeros (SCN-053)", async () => {
    statsGitMock.mockResolvedValue([gitRow({ available: false, reason: "not a git repository", commits: 0 })]);
    render(<StatsView />);
    await waitFor(() =>
      expect(screen.getByTestId("stats-row-alpha-proj").textContent).toContain(strings.stats.noGit),
    );
  });

  it("usage failure renders its note while the git section keeps rendering (per-source honesty)", async () => {
    statsUsageMock.mockRejectedValue(new Error("scan broke"));
    render(<StatsView />);
    await waitFor(() =>
      expect(screen.getByText(strings.stats.usageUnavailable("scan broke"))).toBeTruthy(),
    );
    // git survived: its per-project row is still on screen
    expect(screen.getByTestId("stats-row-alpha-proj").textContent).toContain("3 · +10 −4");
  });

  it("git failure renders its note while usage tiles keep rendering", async () => {
    statsGitMock.mockRejectedValue(new Error("git worker died"));
    render(<StatsView />);
    await waitFor(() =>
      expect(screen.getByText(strings.stats.gitUnavailable("git worker died"))).toBeTruthy(),
    );
    expect(screen.getByTestId("stats-tokens").textContent).toContain("300");
  });

  it("no data in range → honest empty state, never zeros styled as data", async () => {
    statsUsageMock.mockResolvedValue(usage([]));
    statsGitMock.mockResolvedValue([]);
    render(<StatsView />);
    await waitFor(() => expect(screen.getByText(strings.stats.emptyTitle)).toBeTruthy());
    expect(screen.queryByTestId("stats-tiles")).toBeNull();
  });

  it("partial pricing (fable et al) labels the cost tile and marks the row", async () => {
    statsUsageMock.mockResolvedValue(
      usage([day({ estCostUsd: 0.5, costComplete: false })]),
    );
    render(<StatsView />);
    await waitFor(() =>
      expect(screen.getByTestId("stats-cost").textContent).toContain(strings.stats.costPartialLabel),
    );
    expect(screen.getByTestId("stats-row-alpha-proj").textContent).toContain("$0.50*");
  });
});
