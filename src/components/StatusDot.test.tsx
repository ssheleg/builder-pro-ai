// @vitest-environment jsdom
import { describe, it, expect, afterEach } from "vitest";
import { render, screen, cleanup } from "@testing-library/react";
import { StatusDot, dotStateOf } from "./StatusDot";
import type { SessionLifecycle } from "../ipc/types";

afterEach(cleanup);

describe("dotStateOf", () => {
  it("atPrompt (not waiting) -> idle", () => {
    expect(dotStateOf({ kind: "atPrompt" }, false)).toBe("idle");
  });

  it("typing (not waiting) -> idle (Typing maps to AtPrompt color per spec §5)", () => {
    expect(dotStateOf({ kind: "typing" }, false)).toBe("idle");
  });

  it("running (not waiting) -> running", () => {
    expect(dotStateOf({ kind: "running" }, false)).toBe("running");
  });

  it("running + waitingForInput -> waiting (overrides running)", () => {
    expect(dotStateOf({ kind: "running" }, true)).toBe("waiting");
  });

  it("exited (not waiting) -> exited", () => {
    const lc: SessionLifecycle = { kind: "exited", code: 0, signal: null };
    expect(dotStateOf(lc, false)).toBe("exited");
  });

  it("exited + waitingForInput -> exited (exit wins over stale waiting flag)", () => {
    const lc: SessionLifecycle = { kind: "exited", code: 1, signal: null };
    expect(dotStateOf(lc, true)).toBe("exited");
  });

  it("atPrompt + waitingForInput -> idle (waiting only applies while running)", () => {
    expect(dotStateOf({ kind: "atPrompt" }, true)).toBe("idle");
  });
});

describe("StatusDot rendering", () => {
  it("renders the idle color + data-state for atPrompt", () => {
    render(<StatusDot lifecycle={{ kind: "atPrompt" }} waitingForInput={false} />);
    const dot = screen.getByRole("img", { name: /idle/i });
    expect(dot.getAttribute("data-state")).toBe("idle");
    // idle → neutral tone (statusTone("pending") === "muted")
    expect(dot.style.backgroundColor).toContain("--muted");
  });

  it("renders the running color for running", () => {
    render(<StatusDot lifecycle={{ kind: "running" }} waitingForInput={false} />);
    const dot = screen.getByRole("img", { name: /running/i });
    expect(dot.getAttribute("data-state")).toBe("running");
    // running → in-progress tone (statusTone("running") === "info")
    expect(dot.style.backgroundColor).toContain("--info");
  });

  it("renders the waiting color for running+waitingForInput", () => {
    render(<StatusDot lifecycle={{ kind: "running" }} waitingForInput={true} />);
    const dot = screen.getByRole("img", { name: /waiting for input/i });
    expect(dot.getAttribute("data-state")).toBe("waiting");
    // waiting → needs-you tone (statusTone("waiting") === "warn")
    expect(dot.style.backgroundColor).toContain("--warn");
  });

  it("renders the exited color for exited", () => {
    render(
      <StatusDot lifecycle={{ kind: "exited", code: 0, signal: null }} waitingForInput={false} />,
    );
    const dot = screen.getByRole("img", { name: /exited/i });
    expect(dot.getAttribute("data-state")).toBe("exited");
    // exited → terminal-failure tone (statusTone("failed") === "danger")
    expect(dot.style.backgroundColor).toContain("--danger");
  });
});
