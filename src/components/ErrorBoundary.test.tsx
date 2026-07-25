// @vitest-environment jsdom
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { render, screen, fireEvent, cleanup } from "@testing-library/react";
import { ErrorBoundary } from "./ErrorBoundary";
import { useAppStore } from "../store/store";

function Boom(): never {
  throw new Error("kaboom in render");
}

function SecretBoom(): never {
  throw new Error("clone failed with ghp_abcdefghijklmnopqrstuvwxyz under /Users/alice/repo");
}

describe("ErrorBoundary", () => {
  beforeEach(() => {
    useAppStore.setState({ diagEvents: [] });
    // React logs the caught error to console.error; silence it for a clean test run.
    vi.spyOn(console, "error").mockImplementation(() => {});
  });
  afterEach(() => {
    cleanup();
    vi.restoreAllMocks();
  });

  it("renders children when there is no error", () => {
    render(
      <ErrorBoundary>
        <div data-testid="ok">fine</div>
      </ErrorBoundary>,
    );
    expect(screen.getByTestId("ok")).toBeTruthy();
    expect(screen.queryByTestId("error-boundary")).toBeNull();
  });

  it("catches a render crash, shows the recovery card, and records a render diag event", () => {
    render(
      <ErrorBoundary>
        <Boom />
      </ErrorBoundary>,
    );
    expect(screen.getByTestId("error-boundary")).toBeTruthy();
    expect(screen.getByTestId("error-boundary-message").textContent).toContain("kaboom in render");

    const events = useAppStore.getState().diagEvents;
    expect(events.length).toBe(1);
    expect(events[0].kind).toBe("render");
    expect(events[0].message).toContain("kaboom in render");
  });

  it("Reload calls the injected handler", () => {
    const onReload = vi.fn();
    render(
      <ErrorBoundary onReload={onReload}>
        <Boom />
      </ErrorBoundary>,
    );
    fireEvent.click(screen.getByTestId("error-boundary-reload"));
    expect(onReload).toHaveBeenCalledTimes(1);
  });

  it("Copy details writes the SCRUBBED error text to the clipboard (REL-3)", () => {
    render(
      <ErrorBoundary>
        <SecretBoom />
      </ErrorBoundary>,
    );
    // Same clipboard stub as DiagnosticsPanel.test.tsx; installed after render so the crash
    // capture runs with the real globals.
    const writeText = vi.fn();
    vi.stubGlobal("navigator", { clipboard: { writeText } });
    fireEvent.click(screen.getByTestId("error-boundary-copy"));
    expect(writeText).toHaveBeenCalledTimes(1);
    const copied: string = writeText.mock.calls[0][0];
    expect(copied).not.toContain("ghp_abcdefghijklmnopqrstuvwxyz");
    expect(copied).not.toContain("/Users/alice");
    expect(copied).toContain("«redacted-key»");
    expect(copied).toContain("/Users/«user»");
    vi.unstubAllGlobals();
  });
});
