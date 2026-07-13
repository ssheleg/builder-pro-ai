// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, fireEvent, waitFor } from "@testing-library/react";

const orchdCreateInsightMock = vi.fn();
const orchdSetInsightFitVerdictMock = vi.fn();
const orchdSetInsightStatusMock = vi.fn();
const orchdListInsightsMock = vi.fn();
const describeOrchdErrorMock = vi.fn((..._a: unknown[]) => "оркестратор: ошибка");

vi.mock("../ipc/orchd", () => ({
  orchdCreateInsight: (...a: unknown[]) => orchdCreateInsightMock(...a),
  orchdSetInsightFitVerdict: (...a: unknown[]) => orchdSetInsightFitVerdictMock(...a),
  orchdSetInsightStatus: (...a: unknown[]) => orchdSetInsightStatusMock(...a),
  orchdListInsights: (...a: unknown[]) => orchdListInsightsMock(...a),
  describeOrchdError: (...a: unknown[]) => describeOrchdErrorMock(...a),
}));

import { InsightsList } from "./InsightsList";
import { useAppStore } from "../store/store";
import type { Insight } from "../ipc/orchd-types";

const projectId = "proj-1";

function makeInsight(over: Partial<Insight> & { id: string }): Insight {
  return {
    projectId,
    source: "issledovanie",
    title: "инсайт",
    body: "тело инсайта",
    fitVerdict: null,
    fitReasoning: "",
    status: "new",
    resolutionReasoning: "",
    createdAt: 1,
    updatedAt: 1,
    ...over,
  };
}

afterEach(cleanup);

beforeEach(() => {
  orchdCreateInsightMock.mockReset().mockResolvedValue(makeInsight({ id: "new-insight" }));
  orchdSetInsightFitVerdictMock.mockReset().mockResolvedValue(makeInsight({ id: "in1" }));
  orchdSetInsightStatusMock.mockReset().mockResolvedValue(makeInsight({ id: "in1" }));
  orchdListInsightsMock.mockReset().mockResolvedValue([]);
  describeOrchdErrorMock.mockReset().mockReturnValue("оркестратор: ошибка");
  useAppStore.setState({ insights: [], toast: null }, false);
});

describe("InsightsList", () => {
  it("renders only insights whose projectId matches the prop, newest-first, with a fit badge and source caption", () => {
    const older = makeInsight({ id: "old", createdAt: 1, fitVerdict: "fit" });
    const newer = makeInsight({ id: "new", createdAt: 5, fitVerdict: null });
    const other = makeInsight({ id: "other", projectId: "proj-2", createdAt: 9 });
    useAppStore.setState({ insights: [older, newer, other] }, false);

    render(<InsightsList projectId={projectId} />);

    const rows = Array.from(document.querySelectorAll('[data-testid^="insight-row-"]')).map(
      (el) => el.getAttribute("data-testid"),
    );
    expect(rows).toEqual(["insight-row-new", "insight-row-old"]);

    expect(screen.getByTestId("insight-fit-badge-old").textContent).toContain("fit");
    expect(screen.getByTestId("insight-fit-badge-new").textContent).toBe("—");
    expect(screen.getByTestId("insight-source-new").textContent).toContain("issledovanie");
  });

  it("the owner verdict override control fires orchdSetInsightFitVerdict with the chosen verdict and reasoning", async () => {
    const insight = makeInsight({ id: "in1", fitVerdict: null });
    useAppStore.setState({ insights: [insight] }, false);

    render(<InsightsList projectId={projectId} />);

    fireEvent.change(screen.getByTestId("insight-verdict-select-in1"), {
      target: { value: "fit" },
    });
    fireEvent.change(screen.getByTestId("insight-verdict-reasoning-in1"), {
      target: { value: "соответствует стратегии" },
    });
    fireEvent.click(screen.getByTestId("insight-verdict-apply-in1"));

    await waitFor(() =>
      expect(orchdSetInsightFitVerdictMock).toHaveBeenCalledWith(
        "in1",
        "fit",
        "соответствует стратегии",
      ),
    );
  });

  it("choosing a non-archived status calls orchdSetInsightStatus directly with null reasoning", async () => {
    const insight = makeInsight({ id: "in1", status: "new" });
    useAppStore.setState({ insights: [insight] }, false);

    render(<InsightsList projectId={projectId} />);

    fireEvent.change(screen.getByTestId("insight-status-in1"), {
      target: { value: "accepted" },
    });

    await waitFor(() =>
      expect(orchdSetInsightStatusMock).toHaveBeenCalledWith("in1", "accepted", null),
    );
  });

  it("archiving WITHOUT a reasoning is blocked with an inline message and never calls orchdSetInsightStatus", async () => {
    const insight = makeInsight({ id: "in1", status: "new" });
    useAppStore.setState({ insights: [insight] }, false);

    render(<InsightsList projectId={projectId} />);

    fireEvent.change(screen.getByTestId("insight-status-in1"), {
      target: { value: "archived" },
    });
    fireEvent.click(screen.getByTestId("insight-archive-confirm-in1"));

    expect(screen.getByTestId("insight-archive-error-in1").textContent).toBe(
      "нужна причина архивации",
    );
    expect(orchdSetInsightStatusMock).not.toHaveBeenCalled();
  });

  it("archiving WITH a reasoning calls orchdSetInsightStatus with the reasoning", async () => {
    const insight = makeInsight({ id: "in1", status: "new" });
    useAppStore.setState({ insights: [insight] }, false);

    render(<InsightsList projectId={projectId} />);

    fireEvent.change(screen.getByTestId("insight-status-in1"), {
      target: { value: "archived" },
    });
    fireEvent.change(screen.getByTestId("insight-archive-reasoning-in1"), {
      target: { value: "устарело" },
    });
    fireEvent.click(screen.getByTestId("insight-archive-confirm-in1"));

    await waitFor(() =>
      expect(orchdSetInsightStatusMock).toHaveBeenCalledWith("in1", "archived", "устарело"),
    );
    expect(screen.queryByTestId("insight-archive-error-in1")).toBeNull();
  });

  it("the create form calls orchdCreateInsight with the current projectId, source, title and body, then refreshes", async () => {
    render(<InsightsList projectId={projectId} />);

    fireEvent.change(screen.getByTestId("insight-create-source"), {
      target: { value: "прод-лог" },
    });
    fireEvent.change(screen.getByTestId("insight-create-title"), {
      target: { value: "Новый инсайт" },
    });
    fireEvent.change(screen.getByTestId("insight-create-body"), {
      target: { value: "Описание" },
    });
    fireEvent.click(screen.getByTestId("insight-create-submit"));

    await waitFor(() =>
      expect(orchdCreateInsightMock).toHaveBeenCalledWith(
        projectId,
        "прод-лог",
        "Новый инсайт",
        "Описание",
      ),
    );
    await waitFor(() => expect(orchdListInsightsMock).toHaveBeenCalledWith(null));
  });

  it("the create form is a no-op with an empty title", () => {
    render(<InsightsList projectId={projectId} />);

    fireEvent.click(screen.getByTestId("insight-create-submit"));

    expect(orchdCreateInsightMock).not.toHaveBeenCalled();
  });

  it("an error from a mutating call surfaces via showToast", async () => {
    const insight = makeInsight({ id: "in1" });
    useAppStore.setState({ insights: [insight] }, false);
    const commandError = { kind: "daemon", code: "Validation", message: "плохие данные" };
    orchdSetInsightStatusMock.mockRejectedValueOnce(commandError);

    render(<InsightsList projectId={projectId} />);
    fireEvent.change(screen.getByTestId("insight-status-in1"), {
      target: { value: "accepted" },
    });

    await waitFor(() => expect(describeOrchdErrorMock).toHaveBeenCalledWith(commandError));
    await waitFor(() => expect(useAppStore.getState().toast).toBe("оркестратор: ошибка"));
  });

  it("renders an empty state when there are no matching insights", () => {
    render(<InsightsList projectId={projectId} />);
    expect(screen.getByTestId("insights-list-empty")).toBeTruthy();
  });
});
