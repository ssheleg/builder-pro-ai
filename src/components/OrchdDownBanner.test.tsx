// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { render, screen, cleanup, fireEvent } from "@testing-library/react";

const orchdReconnectMock = vi.fn();
vi.mock("../ipc/orchd", () => ({
  orchdReconnect: (...a: unknown[]) => orchdReconnectMock(...a),
}));

import { OrchdDownBanner } from "./OrchdDownBanner";
import { strings } from "../strings";

afterEach(cleanup);
beforeEach(() => {
  orchdReconnectMock.mockReset().mockResolvedValue(undefined);
});

describe("OrchdDownBanner", () => {
  it('renders the «Orchestrator unavailable» copy + a [Retry] button', () => {
    render(<OrchdDownBanner />);
    expect(screen.getByText(strings.chrome.orchdUnavailable)).toBeTruthy();
    expect(screen.getByRole("button", { name: strings.common.retry })).toBeTruthy();
    expect(screen.getByRole("alert")).toBeTruthy();
  });

  it("clicking [Retry] calls orchdReconnect()", () => {
    render(<OrchdDownBanner />);
    fireEvent.click(screen.getByRole("button", { name: strings.common.retry }));
    expect(orchdReconnectMock).toHaveBeenCalledTimes(1);
    expect(orchdReconnectMock).toHaveBeenCalledWith();
  });
});
