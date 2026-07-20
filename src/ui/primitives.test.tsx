// @vitest-environment jsdom
import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, fireEvent, cleanup } from "@testing-library/react";

// Vitest runs without globals, so testing-library's auto-cleanup never registers — unmount
// explicitly or every render accumulates in the shared jsdom and role queries find duplicates.
afterEach(cleanup);
import {
  Panel,
  Stat,
  Sparkline,
  Badge,
  Button,
  EmptyState,
  Dialog,
  Field,
  Input,
  SegmentedPill,
  Heatmap,
} from "./primitives";

describe("primitives", () => {
  it("Panel renders a title, actions and children", () => {
    render(
      <Panel title="Overview" actions={<button>Act</button>} data-testid="p">
        <span>body</span>
      </Panel>,
    );
    expect(screen.getByText("Overview")).toBeTruthy();
    expect(screen.getByText("Act")).toBeTruthy();
    expect(screen.getByText("body")).toBeTruthy();
  });

  it("Stat shows label, mono value, unit and delta", () => {
    render(<Stat label="Tasks" value={42} unit="open" delta={{ value: "+3", tone: "ok" }} data-testid="s" />);
    const tile = screen.getByTestId("s");
    expect(tile.textContent).toContain("Tasks");
    expect(tile.textContent).toContain("42");
    expect(tile.textContent).toContain("open");
    expect(tile.textContent).toContain("+3");
  });

  it("Sparkline renders a polyline for >=2 points and nothing for fewer", () => {
    const { container, rerender } = render(<Sparkline points={[1, 4, 2, 6]} />);
    expect(container.querySelector("path")).toBeTruthy();
    rerender(<Sparkline points={[1]} />);
    expect(container.querySelector("path")).toBeNull();
  });

  it("Badge derives its tone from a status", () => {
    render(<Badge status="failed" data-testid="b">failed</Badge>);
    const el = screen.getByTestId("b");
    expect(el.textContent).toBe("failed");
    // danger tone → danger fg colour var
    expect((el as HTMLElement).style.color).toContain("--danger");
  });

  it("Button is disabled while loading and enabled otherwise", () => {
    const onClick = vi.fn();
    const { rerender } = render(
      <Button loading onClick={onClick} data-testid="tbtn">
        Save
      </Button>,
    );
    // A disabled button is how the guard is enforced — a real user cannot click it.
    expect((screen.getByTestId("tbtn") as HTMLButtonElement).disabled).toBe(true);

    rerender(
      <Button onClick={onClick} data-testid="tbtn">
        Save
      </Button>,
    );
    const btn = screen.getByTestId("tbtn") as HTMLButtonElement;
    expect(btn.disabled).toBe(false);
    fireEvent.click(btn);
    expect(onClick).toHaveBeenCalledTimes(1);
  });

  it("Field shows an error over a hint", () => {
    render(
      <Field label="Name" hint="the hint" error="the error">
        <Input />
      </Field>,
    );
    expect(screen.getByRole("alert").textContent).toBe("the error");
    expect(screen.queryByText("the hint")).toBeNull();
  });

  it("EmptyState renders title, hint and action", () => {
    render(<EmptyState title="Nothing here" hint="add one" action={<button>Add</button>} data-testid="e" />);
    const el = screen.getByTestId("e");
    expect(el.textContent).toContain("Nothing here");
    expect(el.textContent).toContain("add one");
    expect(screen.getByText("Add")).toBeTruthy();
  });

  it("Dialog renders when open, hides when closed, and closes on Escape + overlay + ✕", () => {
    const onClose = vi.fn();
    const { rerender } = render(
      <Dialog open={false} title="T" onClose={onClose} data-testid="d">
        body
      </Dialog>,
    );
    expect(screen.queryByTestId("d")).toBeNull();

    rerender(
      <Dialog open title="T" onClose={onClose} data-testid="d">
        body
      </Dialog>,
    );
    expect(screen.getByRole("dialog")).toBeTruthy();

    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByLabelText("Close"));
    expect(onClose).toHaveBeenCalledTimes(2);

    fireEvent.click(screen.getByTestId("d")); // overlay click (target === currentTarget)
    expect(onClose).toHaveBeenCalledTimes(3);
  });

  it("SegmentedPill renders radiogroup semantics and switches on click", () => {
    const onChange = vi.fn();
    render(
      <SegmentedPill
        ariaLabel="Range"
        options={[{ value: "all", label: "All" }, { value: "30d", label: "30d" }] as const}
        value="all"
        onChange={onChange}
      />,
    );
    const group = screen.getByRole("radiogroup", { name: "Range" });
    expect(group).toBeTruthy();
    const radios = screen.getAllByRole("radio");
    expect(radios).toHaveLength(2);
    expect(radios[0].getAttribute("aria-checked")).toBe("true");
    fireEvent.click(radios[1]);
    expect(onChange).toHaveBeenCalledWith("30d");
  });

  it("SegmentedPill moves selection with arrow keys", () => {
    const onChange = vi.fn();
    render(
      <SegmentedPill
        ariaLabel="Range"
        options={[{ value: "all", label: "All" }, { value: "30d", label: "30d" }] as const}
        value="all"
        onChange={onChange}
      />,
    );
    fireEvent.keyDown(screen.getByRole("radiogroup", { name: "Range" }), { key: "ArrowRight" });
    expect(onChange).toHaveBeenCalledWith("30d");
  });

  it("Heatmap renders one cell per value with 5-level buckets", () => {
    render(<Heatmap values={[0, 1, 2, 3, 4]} columns={5} max={4} ariaLabel="Activity" data-testid="h" />);
    const grid = screen.getByTestId("h");
    expect(grid.getAttribute("aria-label")).toBe("Activity");
    const cells = grid.querySelectorAll("[data-level]");
    expect(cells).toHaveLength(5);
    expect(Array.from(cells).map((c) => c.getAttribute("data-level"))).toEqual(["0", "1", "2", "3", "4"]);
  });

  it("Heatmap survives all-zero and empty inputs (no division by zero)", () => {
    const { rerender } = render(<Heatmap values={[0, 0, 0]} columns={3} ariaLabel="A" data-testid="h" />);
    expect(
      Array.from(screen.getByTestId("h").querySelectorAll("[data-level]")).every(
        (c) => c.getAttribute("data-level") === "0",
      ),
    ).toBe(true);
    rerender(<Heatmap values={[]} columns={3} ariaLabel="A" data-testid="h" />);
    expect(screen.getByTestId("h").querySelectorAll("[data-level]")).toHaveLength(0);
  });

  it("Heatmap clamps negatives to level 0 and overshoot to level 4", () => {
    render(<Heatmap values={[-5, 99]} columns={2} max={4} ariaLabel="A" data-testid="h" />);
    const levels = Array.from(screen.getByTestId("h").querySelectorAll("[data-level]")).map((c) =>
      c.getAttribute("data-level"),
    );
    expect(levels).toEqual(["0", "4"]);
  });
});
