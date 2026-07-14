// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach, beforeEach } from "vitest";
import { render, screen, cleanup, fireEvent } from "@testing-library/react";

const orchdReconnectMock = vi.fn();
vi.mock("../ipc/orchd", () => ({
  orchdReconnect: (...a: unknown[]) => orchdReconnectMock(...a),
}));

import { OrchdDownBanner } from "./OrchdDownBanner";

afterEach(cleanup);
beforeEach(() => {
  orchdReconnectMock.mockReset().mockResolvedValue(undefined);
});

describe("OrchdDownBanner", () => {
  it('renders the «Оркестратор недоступен» copy + a [Повторить] button', () => {
    render(<OrchdDownBanner />);
    expect(screen.getByText("Оркестратор недоступен")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Повторить" })).toBeTruthy();
    expect(screen.getByRole("alert")).toBeTruthy();
  });

  it("clicking [Повторить] calls orchdReconnect()", () => {
    render(<OrchdDownBanner />);
    fireEvent.click(screen.getByRole("button", { name: "Повторить" }));
    expect(orchdReconnectMock).toHaveBeenCalledTimes(1);
    expect(orchdReconnectMock).toHaveBeenCalledWith();
  });
});
