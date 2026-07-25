// @vitest-environment jsdom
import { describe, it, expect, afterEach } from "vitest";
import { render, screen, cleanup } from "@testing-library/react";
import { StatusDot, dotStateOf } from "./StatusDot";
import { strings } from "../strings";
import type { SessionLifecycle } from "../ipc/types";

afterEach(cleanup);

describe("dotStateOf", () => {
  it("atPrompt (not waiting, live) -> idle", () => {
    expect(dotStateOf({ kind: "atPrompt" }, false, true)).toBe("idle");
  });

  it("typing (not waiting, live) -> idle (Typing maps to AtPrompt color per spec §5)", () => {
    expect(dotStateOf({ kind: "typing" }, false, true)).toBe("idle");
  });

  it("running (not waiting, live) -> running", () => {
    expect(dotStateOf({ kind: "running" }, false, true)).toBe("running");
  });

  it("running + waitingForInput -> waiting (overrides running)", () => {
    expect(dotStateOf({ kind: "running" }, true, true)).toBe("waiting");
  });

  it("exited (not waiting) -> exited", () => {
    const lc: SessionLifecycle = { kind: "exited", code: 0, signal: null };
    expect(dotStateOf(lc, false, false)).toBe("exited");
  });

  it("exited + waitingForInput -> exited (exit wins over stale waiting flag)", () => {
    const lc: SessionLifecycle = { kind: "exited", code: 1, signal: null };
    expect(dotStateOf(lc, true, false)).toBe("exited");
  });

  it("atPrompt + waitingForInput -> idle (waiting only applies while running)", () => {
    expect(dotStateOf({ kind: "atPrompt" }, true, true)).toBe("idle");
  });

  // FE-7: a not-exited session with isActive === false is RESTORED (its PTY is gone after a
  // daemon restart) — never "idle", which would imply a live shell at its prompt.
  it("atPrompt + isActive:false -> restored (not idle)", () => {
    expect(dotStateOf({ kind: "atPrompt" }, false, false)).toBe("restored");
  });

  it("running + isActive:false (not waiting) -> restored", () => {
    expect(dotStateOf({ kind: "running" }, false, false)).toBe("restored");
  });

  it("running + waiting + isActive:false -> waiting (waiting still wins over liveness)", () => {
    expect(dotStateOf({ kind: "running" }, true, false)).toBe("waiting");
  });
});

describe("StatusDot rendering", () => {
  it("renders the idle color + data-state for atPrompt", () => {
    render(<StatusDot lifecycle={{ kind: "atPrompt" }} waitingForInput={false} isActive={true} />);
    const dot = screen.getByRole("img", { name: /idle/i });
    expect(dot.getAttribute("data-state")).toBe("idle");
    // idle → neutral tone (statusTone("pending") === "muted")
    expect(dot.style.backgroundColor).toContain("--muted");
  });

  it("renders the running color for running", () => {
    render(<StatusDot lifecycle={{ kind: "running" }} waitingForInput={false} isActive={true} />);
    const dot = screen.getByRole("img", { name: /running/i });
    expect(dot.getAttribute("data-state")).toBe("running");
    // running → in-progress tone (statusTone("running") === "info")
    expect(dot.style.backgroundColor).toContain("--info");
  });

  it("renders the waiting color for running+waitingForInput", () => {
    render(<StatusDot lifecycle={{ kind: "running" }} waitingForInput={true} isActive={true} />);
    const dot = screen.getByRole("img", { name: /waiting for input/i });
    expect(dot.getAttribute("data-state")).toBe("waiting");
    // waiting → needs-you tone (statusTone("waiting") === "warn")
    expect(dot.style.backgroundColor).toContain("--warn");
  });

  it("renders the exited color for exited", () => {
    render(
      <StatusDot
        lifecycle={{ kind: "exited", code: 0, signal: null }}
        waitingForInput={false}
        isActive={false}
      />,
    );
    const dot = screen.getByRole("img", { name: /exited/i });
    expect(dot.getAttribute("data-state")).toBe("exited");
    // exited → terminal-failure tone (statusTone("failed") === "danger")
    expect(dot.style.backgroundColor).toContain("--danger");
  });

  it("FE-7: restored renders as a hollow ring with the strings label, not as idle", () => {
    render(<StatusDot lifecycle={{ kind: "atPrompt" }} waitingForInput={false} isActive={false} />);
    const dot = screen.getByRole("img", { name: strings.sessions.restoredDotLabel });
    expect(dot.getAttribute("data-state")).toBe("restored");
    // Hollow ring: no filled background (a filled dot would claim a live process state), the
    // muted token color carried by the inset ring instead.
    expect(dot.style.backgroundColor).toBe("transparent");
    expect(dot.style.boxShadow).toContain("--muted");
  });
});
