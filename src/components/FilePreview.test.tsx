// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, cleanup, act } from "@testing-library/react";

const readFilePreviewMock = vi.fn();
vi.mock("../ipc/fs", () => ({
  readFilePreview: (...a: unknown[]) => readFilePreviewMock(...a),
}));

import { FilePreview } from "./FilePreview";
import { useAppStore } from "../store/store";

afterEach(cleanup);
beforeEach(() => {
  readFilePreviewMock.mockReset();
  useAppStore.setState({ selectedFile: null, toast: null }, false);
});

describe("FilePreview", () => {
  it("renders an empty-state placeholder when nothing is selected", () => {
    render(<FilePreview />);
    expect(screen.getByText(/выберите файл/i)).toBeTruthy();
    expect(readFilePreviewMock).not.toHaveBeenCalled();
  });

  it("fetches readFilePreview when a file becomes selected", async () => {
    readFilePreviewMock.mockResolvedValue({ kind: "text", content: "hi", truncated: false, size: 2 });
    await act(async () => {
      useAppStore.setState({ selectedFile: { root: "/proj", rel: "a.txt" } }, false);
    });
    render(<FilePreview />);
    await act(async () => {
      await Promise.resolve();
    });
    expect(readFilePreviewMock).toHaveBeenCalledWith("/proj", "a.txt");
  });

  it("renders text preview read-only monospace, no editing affordance", async () => {
    readFilePreviewMock.mockResolvedValue({
      kind: "text",
      content: "line one\nline two",
      truncated: false,
      size: 18,
    });
    useAppStore.setState({ selectedFile: { root: "/proj", rel: "a.txt" } }, false);
    render(<FilePreview />);
    const pre = await screen.findByText((content) => content.includes("line one"));
    expect(pre.tagName.toLowerCase()).toBe("pre");
    expect(screen.queryByRole("textbox")).toBeNull();
  });

  it("renders a truncated-content caveat when the read raced a shrinking file", async () => {
    readFilePreviewMock.mockResolvedValue({
      kind: "text",
      content: "partial",
      truncated: true,
      size: 1000,
    });
    useAppStore.setState({ selectedFile: { root: "/proj", rel: "a.txt" } }, false);
    render(<FilePreview />);
    await screen.findByText((content) => content.includes("partial"));
    expect(screen.getByText(/неполн/i)).toBeTruthy();
  });

  it("renders a binary placeholder with a humanized size", async () => {
    readFilePreviewMock.mockResolvedValue({ kind: "binary", size: 2048 });
    useAppStore.setState({ selectedFile: { root: "/proj", rel: "bin.dat" } }, false);
    render(<FilePreview />);
    expect(await screen.findByText(/Бинарный файл/)).toBeTruthy();
    expect(screen.getByText(/2(\.0)? KB/)).toBeTruthy();
  });

  it("renders a too-large placeholder with a humanized size", async () => {
    readFilePreviewMock.mockResolvedValue({ kind: "tooLarge", size: 5 * 1024 * 1024 });
    useAppStore.setState({ selectedFile: { root: "/proj", rel: "big.bin" } }, false);
    render(<FilePreview />);
    expect(await screen.findByText(/слишком большой/)).toBeTruthy();
    expect(screen.getByText(/5(\.0)? MB/)).toBeTruthy();
  });

  it("an FsError renders an honest placeholder AND fires a toast (never console-only)", async () => {
    readFilePreviewMock.mockRejectedValue({ kind: "permissionDenied" });
    useAppStore.setState({ selectedFile: { root: "/proj", rel: "secret" } }, false);
    render(<FilePreview />);
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(screen.getByText(/не удалось открыть файл/i)).toBeTruthy();
    expect(useAppStore.getState().toast).toMatch(/не удалось открыть файл/i);
  });

  it("re-fetches when selectedFile changes to a different file", async () => {
    readFilePreviewMock
      .mockResolvedValueOnce({ kind: "text", content: "A", truncated: false, size: 1 })
      .mockResolvedValueOnce({ kind: "text", content: "B", truncated: false, size: 1 });
    useAppStore.setState({ selectedFile: { root: "/proj", rel: "a.txt" } }, false);
    render(<FilePreview />);
    await screen.findByText("A");

    await act(async () => {
      useAppStore.setState({ selectedFile: { root: "/proj", rel: "b.txt" } }, false);
    });
    await screen.findByText("B");
    expect(readFilePreviewMock).toHaveBeenCalledTimes(2);
    expect(readFilePreviewMock).toHaveBeenNthCalledWith(2, "/proj", "b.txt");
  });

  it("clearing selectedFile returns to the empty-state placeholder", async () => {
    readFilePreviewMock.mockResolvedValue({ kind: "text", content: "hi", truncated: false, size: 2 });
    useAppStore.setState({ selectedFile: { root: "/proj", rel: "a.txt" } }, false);
    render(<FilePreview />);
    await screen.findByText("hi");
    await act(async () => {
      useAppStore.setState({ selectedFile: null }, false);
    });
    expect(screen.getByText(/выберите файл/i)).toBeTruthy();
  });

  it("a late-resolving stale request never clobbers a newer selection's preview", async () => {
    let resolveFirst: (v: unknown) => void = () => {};
    const first = new Promise((resolve) => {
      resolveFirst = resolve;
    });
    readFilePreviewMock.mockReturnValueOnce(first);
    readFilePreviewMock.mockResolvedValueOnce({ kind: "text", content: "SECOND", truncated: false, size: 6 });

    useAppStore.setState({ selectedFile: { root: "/proj", rel: "a.txt" } }, false);
    render(<FilePreview />);

    await act(async () => {
      useAppStore.setState({ selectedFile: { root: "/proj", rel: "b.txt" } }, false);
    });
    await screen.findByText("SECOND");

    // The stale first request now resolves — must NOT overwrite the current (second) preview.
    await act(async () => {
      resolveFirst({ kind: "text", content: "STALE", truncated: false, size: 5 });
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(screen.queryByText("STALE")).toBeNull();
    expect(screen.getByText("SECOND")).toBeTruthy();
  });
});
