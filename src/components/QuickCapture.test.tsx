// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, fireEvent, act } from "@testing-library/react";

const orchdCreateIdeaMock = vi.fn();
const describeOrchdErrorMock = vi.fn((..._a: unknown[]) => "orchestrator: error");
vi.mock("../ipc/orchd", () => ({
  orchdCreateIdea: (...a: unknown[]) => orchdCreateIdeaMock(...a),
  describeOrchdError: (...a: unknown[]) => describeOrchdErrorMock(...a),
}));

import { QuickCapture } from "./QuickCapture";
import { useAppStore } from "../store/store";
import { strings } from "../strings";
import type { Project } from "../ipc/orchd-types";

function makeProject(over: Partial<Project> = {}): Project {
  return {
    id: "p1",
    name: "Proj",
    description: "",
    status: "active",
    workspaceIds: [],
    createdAt: 1,
    updatedAt: 1,
    ...over,
  };
}

function pressCmdK(): void {
  fireEvent.keyDown(window, { key: "k", metaKey: true });
}

afterEach(cleanup);

beforeEach(() => {
  orchdCreateIdeaMock.mockReset().mockResolvedValue({
    id: "i1",
    projectId: null,
    title: "t",
    body: "",
    lifecycle: "captured",
    createdAt: 1,
    updatedAt: 1,
  });
  describeOrchdErrorMock.mockReset().mockReturnValue("orchestrator: error");
  useAppStore.setState(
    {
      projects: [],
      orchdDown: false,
      toast: null,
      daemonIncompatible: false,
      upgradeDialogOpen: false,
      orchdIncompatible: false,
      orchdUpgradeDialogOpen: false,
    },
    false,
  );
});

describe("QuickCapture", () => {
  it("renders nothing until ⌘K is pressed", () => {
    const { container } = render(<QuickCapture />);
    expect(container.firstChild).toBeNull();
  });

  it("⌘K (metaKey+K) opens the overlay", () => {
    render(<QuickCapture />);
    pressCmdK();
    expect(screen.getByTestId("quick-capture-overlay")).toBeTruthy();
    expect(screen.getByRole("dialog")).toBeTruthy();
  });

  it("⌘K is IGNORED while an <input> is focused elsewhere on the page", () => {
    render(
      <div>
        <input data-testid="outside-input" />
        <QuickCapture />
      </div>,
    );
    const outside = screen.getByTestId("outside-input");
    outside.focus();
    expect(document.activeElement).toBe(outside);
    pressCmdK();
    expect(screen.queryByTestId("quick-capture-overlay")).toBeNull();
  });

  it("⌘K is IGNORED while a <textarea> is focused elsewhere on the page", () => {
    render(
      <div>
        <textarea data-testid="outside-textarea" />
        <QuickCapture />
      </div>,
    );
    const outside = screen.getByTestId("outside-textarea");
    outside.focus();
    pressCmdK();
    expect(screen.queryByTestId("quick-capture-overlay")).toBeNull();
  });

  it("⌘K is IGNORED while focus is inside an .xterm ancestor", () => {
    render(
      <div>
        <div className="xterm">
          <div tabIndex={-1} data-testid="xterm-inner" />
        </div>
        <QuickCapture />
      </div>,
    );
    const inner = screen.getByTestId("xterm-inner");
    (inner as HTMLElement).focus();
    expect(document.activeElement).toBe(inner);
    pressCmdK();
    expect(screen.queryByTestId("quick-capture-overlay")).toBeNull();
  });

  it("⌘K is IGNORED while the daemon upgrade dialog is a mandatory blocker (daemonIncompatible && upgradeDialogOpen)", () => {
    useAppStore.setState({ daemonIncompatible: true, upgradeDialogOpen: true }, false);
    render(<QuickCapture />);
    pressCmdK();
    expect(screen.queryByTestId("quick-capture-overlay")).toBeNull();
  });

  it("⌘K is IGNORED while the orchd upgrade dialog is a mandatory blocker (orchdIncompatible && orchdUpgradeDialogOpen)", () => {
    useAppStore.setState({ orchdIncompatible: true, orchdUpgradeDialogOpen: true }, false);
    render(<QuickCapture />);
    pressCmdK();
    expect(screen.queryByTestId("quick-capture-overlay")).toBeNull();
  });

  it("Esc closes the overlay", () => {
    render(<QuickCapture />);
    pressCmdK();
    expect(screen.getByTestId("quick-capture-overlay")).toBeTruthy();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByTestId("quick-capture-overlay")).toBeNull();
  });

  it('submit calls orchdCreateIdea with null projectId for "no project", shows the saved toast, and closes', async () => {
    useAppStore.setState({ projects: [makeProject({ id: "p1", name: "Proj" })] }, false);
    render(<QuickCapture />);
    pressCmdK();

    fireEvent.change(screen.getByTestId("quick-capture-title-input"), {
      target: { value: "My idea" },
    });
    fireEvent.change(screen.getByTestId("quick-capture-body-input"), {
      target: { value: "body text" },
    });
    // leave the project select at its default "no project" (empty string) value

    await act(async () => {
      fireEvent.click(screen.getByTestId("quick-capture-submit"));
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(orchdCreateIdeaMock).toHaveBeenCalledWith(null, "My idea", "body text");
    expect(useAppStore.getState().toast).toBe(strings.capture.ideaSaved);
    expect(screen.queryByTestId("quick-capture-overlay")).toBeNull();
  });

  it("submit calls orchdCreateIdea with the chosen project id when a project is selected", async () => {
    useAppStore.setState(
      { projects: [makeProject({ id: "p1", name: "Proj" }), makeProject({ id: "p2", name: "Other" })] },
      false,
    );
    render(<QuickCapture />);
    pressCmdK();

    fireEvent.change(screen.getByTestId("quick-capture-title-input"), {
      target: { value: "My idea" },
    });
    fireEvent.change(screen.getByTestId("quick-capture-project-select"), {
      target: { value: "p2" },
    });

    await act(async () => {
      fireEvent.click(screen.getByTestId("quick-capture-submit"));
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(orchdCreateIdeaMock).toHaveBeenCalledWith("p2", "My idea", "");
  });

  it("Enter in the title field submits (same as the Save button)", async () => {
    render(<QuickCapture />);
    pressCmdK();
    fireEvent.change(screen.getByTestId("quick-capture-title-input"), {
      target: { value: "Enter idea" },
    });

    await act(async () => {
      fireEvent.keyDown(screen.getByTestId("quick-capture-title-input"), { key: "Enter" });
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(orchdCreateIdeaMock).toHaveBeenCalledWith(null, "Enter idea", "");
  });

  it("two rapid Enters call orchdCreateIdea ONCE (double-submit guard, spec D6 / E-08)", async () => {
    // Hold the create in flight so the second Enter fires while the first is still pending.
    let resolveCreate!: (v: unknown) => void;
    orchdCreateIdeaMock.mockReset().mockImplementation(
      () => new Promise((res) => (resolveCreate = res)),
    );
    render(<QuickCapture />);
    pressCmdK();
    fireEvent.change(screen.getByTestId("quick-capture-title-input"), {
      target: { value: "Dup idea" },
    });

    await act(async () => {
      const input = screen.getByTestId("quick-capture-title-input");
      fireEvent.keyDown(input, { key: "Enter" });
      fireEvent.keyDown(input, { key: "Enter" });
      await Promise.resolve();
    });

    expect(orchdCreateIdeaMock).toHaveBeenCalledTimes(1);

    await act(async () => {
      resolveCreate({ id: "i1", projectId: null, title: "Dup idea", body: "", lifecycle: "captured", createdAt: 1, updatedAt: 1 });
      await Promise.resolve();
    });
  });

  it("submit is a no-op with an empty (whitespace-only) title", () => {
    render(<QuickCapture />);
    pressCmdK();
    fireEvent.change(screen.getByTestId("quick-capture-title-input"), {
      target: { value: "   " },
    });
    expect(screen.getByTestId("quick-capture-submit")).toHaveProperty("disabled", true);
    fireEvent.click(screen.getByTestId("quick-capture-submit"));
    expect(orchdCreateIdeaMock).not.toHaveBeenCalled();
  });

  it("while orchdDown: submit is disabled, an inline note shows, and orchdCreateIdea is NEVER called", () => {
    useAppStore.setState({ orchdDown: true }, false);
    render(<QuickCapture />);
    pressCmdK();

    fireEvent.change(screen.getByTestId("quick-capture-title-input"), {
      target: { value: "My idea" },
    });

    expect(screen.getByTestId("quick-capture-orchd-down")).toBeTruthy();
    expect(screen.getByText(strings.errors.unavailable)).toBeTruthy();
    expect(screen.getByTestId("quick-capture-submit")).toHaveProperty("disabled", true);

    fireEvent.click(screen.getByTestId("quick-capture-submit"));
    expect(orchdCreateIdeaMock).not.toHaveBeenCalled();
  });

  it("a rejected orchdCreateIdea surfaces the mapped honest message via toast and the overlay stays open", async () => {
    orchdCreateIdeaMock.mockReset().mockRejectedValue({ kind: "daemon", code: "Validation", message: "bad" });
    render(<QuickCapture />);
    pressCmdK();
    fireEvent.change(screen.getByTestId("quick-capture-title-input"), {
      target: { value: "My idea" },
    });

    await act(async () => {
      fireEvent.click(screen.getByTestId("quick-capture-submit"));
      await Promise.resolve();
      await Promise.resolve();
    });

    expect(describeOrchdErrorMock).toHaveBeenCalled();
    expect(useAppStore.getState().toast).toBe("orchestrator: error");
    expect(screen.getByTestId("quick-capture-overlay")).toBeTruthy();
  });

  it("closing and reopening clears the previous draft (title/body/project)", async () => {
    useAppStore.setState({ projects: [makeProject({ id: "p1", name: "Proj" })] }, false);
    render(<QuickCapture />);
    pressCmdK();
    fireEvent.change(screen.getByTestId("quick-capture-title-input"), {
      target: { value: "draft title" },
    });
    fireEvent.keyDown(window, { key: "Escape" });

    pressCmdK();
    expect((screen.getByTestId("quick-capture-title-input") as HTMLInputElement).value).toBe("");
  });
});
