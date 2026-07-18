// @vitest-environment jsdom
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { render, screen, fireEvent, cleanup } from "@testing-library/react";
import { DiagnosticsPanel } from "./DiagnosticsPanel";
import { useAppStore } from "../store/store";
import type { DiagEvent } from "../ipc/diag";

const evt = (over: Partial<DiagEvent> = {}): DiagEvent => ({
  id: 1,
  ts: 1_700_000_000_000,
  op: "refreshProjects",
  kind: "disconnected",
  message: "The orchestrator is unavailable.",
  detail: null,
  ...over,
});

describe("DiagnosticsPanel", () => {
  beforeEach(() => {
    useAppStore.setState({ diagEvents: [] });
  });
  afterEach(() => cleanup());

  it("shows an empty state when the ring is empty", () => {
    render(<DiagnosticsPanel open onClose={() => {}} />);
    expect(screen.getByTestId("diag-empty")).toBeTruthy();
    expect(screen.queryByTestId("diag-list")).toBeNull();
  });

  it("renders one row per event with op, kind and message", () => {
    useAppStore.setState({
      diagEvents: [evt({ id: 2, op: "createWorkspace", kind: "Invariant", message: "last workspace", detail: "ws-1" })],
    });
    render(<DiagnosticsPanel open onClose={() => {}} />);
    const rows = screen.getAllByTestId("diag-row");
    expect(rows.length).toBe(1);
    expect(rows[0].textContent).toContain("createWorkspace");
    expect(rows[0].textContent).toContain("Invariant");
    expect(rows[0].textContent).toContain("last workspace");
    expect(rows[0].textContent).toContain("ws-1"); // detail rendered
  });

  it("Clear empties the ring via the store", () => {
    useAppStore.setState({ diagEvents: [evt()] });
    render(<DiagnosticsPanel open onClose={() => {}} />);
    fireEvent.click(screen.getByTestId("diag-clear"));
    expect(useAppStore.getState().diagEvents).toEqual([]);
  });

  it("Copy support bundle writes scrubbed JSON to the clipboard", () => {
    const writeText = vi.fn();
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    useAppStore.setState({ diagEvents: [evt()] });
    render(<DiagnosticsPanel open onClose={() => {}} />);
    fireEvent.click(screen.getByTestId("diag-copy"));
    expect(writeText).toHaveBeenCalledTimes(1);
    expect(writeText.mock.calls[0][0]).toContain("refreshProjects");
    vi.unstubAllGlobals();
  });
});
