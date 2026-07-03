// @vitest-environment jsdom
import { describe, it, expect, vi, beforeEach } from "vitest";

// ---- record ordering across all terminal instances ----
const calls: string[] = [];

const writeStdinMock = vi.fn();
const resizeMock = vi.fn();
const attachSessionMock = vi.fn().mockResolvedValue(undefined);

vi.mock("../ipc/commands", () => ({
  writeStdin: (...a: unknown[]) => writeStdinMock(...a),
  resize: (...a: unknown[]) => resizeMock(...a),
  attachSession: (...a: unknown[]) => attachSessionMock(...a),
}));

let lastChannelHandler: ((e: unknown) => void) | undefined;
const newTerminalChannelMock = vi.fn((onEvent: (e: unknown) => void) => {
  lastChannelHandler = onEvent;
  return { onmessage: onEvent };
});
vi.mock("../ipc/channel", () => ({
  newTerminalChannel: (onEvent: (e: unknown) => void) =>
    newTerminalChannelMock(onEvent),
}));

vi.mock("@xterm/xterm/css/xterm.css", () => ({}));

class FakeTerminal {
  options: Record<string, unknown>;
  disposed = false;
  onDataCb: ((d: string) => void) | undefined;
  onResizeCb: ((s: { cols: number; rows: number }) => void) | undefined;
  cols = 80;
  rows = 24;
  constructor(opts: Record<string, unknown>) {
    this.options = opts;
  }
  loadAddon = vi.fn();
  open = vi.fn(() => calls.push("open"));
  write = vi.fn((_d: unknown, cb?: () => void) => {
    calls.push("write");
    cb?.();
  });
  resize = vi.fn((c: number, r: number) => {
    this.cols = c;
    this.rows = r;
    calls.push("resize");
  });
  onData = vi.fn((cb: (d: string) => void) => {
    this.onDataCb = cb;
    return { dispose: vi.fn() };
  });
  onResize = vi.fn((cb: (s: { cols: number; rows: number }) => void) => {
    this.onResizeCb = cb;
    return { dispose: vi.fn() };
  });
  dispose = vi.fn(() => {
    this.disposed = true;
    calls.push("dispose");
  });
}
const terminals: FakeTerminal[] = [];
vi.mock("@xterm/xterm", () => ({
  Terminal: vi.fn((opts: Record<string, unknown>) => {
    const t = new FakeTerminal(opts);
    terminals.push(t);
    return t;
  }),
}));

const fitMock = vi.fn();
vi.mock("@xterm/addon-fit", () => ({
  FitAddon: vi.fn(() => ({ fit: fitMock, proposeDimensions: vi.fn() })),
}));

const webglDispose = vi.fn();
let contextLossCb: (() => void) | undefined;
vi.mock("@xterm/addon-webgl", () => ({
  WebglAddon: vi.fn(() => ({
    dispose: webglDispose,
    onContextLoss: (cb: () => void) => {
      contextLossCb = cb;
    },
  })),
}));

import { TerminalManager } from "./terminal-manager";
import type { TerminalEvent } from "../ipc/types";

function makeContainer(): HTMLElement {
  const el = document.createElement("div");
  Object.defineProperty(el, "clientWidth", { value: 800, configurable: true });
  Object.defineProperty(el, "clientHeight", { value: 600, configurable: true });
  document.body.appendChild(el);
  return el;
}

// jsdom lacks ResizeObserver
let resizeObserverCb: (() => void) | undefined;
beforeEach(() => {
  calls.length = 0;
  terminals.length = 0;
  contextLossCb = undefined;
  lastChannelHandler = undefined;
  resizeObserverCb = undefined;
  writeStdinMock.mockReset();
  resizeMock.mockReset();
  attachSessionMock.mockClear();
  newTerminalChannelMock.mockClear();
  fitMock.mockReset();
  webglDispose.mockReset();
  (globalThis as unknown as { ResizeObserver: unknown }).ResizeObserver = class {
    constructor(cb: () => void) {
      resizeObserverCb = cb;
    }
    observe = vi.fn();
    disconnect = vi.fn();
  };
});

describe("TerminalManager", () => {
  it("ensure/create is idempotent and StrictMode-safe (one Terminal per id)", () => {
    const m = new TerminalManager();
    const a = m.ensure("s1");
    const b = m.ensure("s1");
    expect(a).toBe(b);
    expect(terminals).toHaveLength(1);
    expect(m.has("s1")).toBe(true);
  });

  it("sets convertEol off on the Terminal", () => {
    const m = new TerminalManager();
    m.ensure("s1");
    expect(terminals[0].options.convertEol).toBe(false);
  });

  it("sets a sane scrollback value", () => {
    const m = new TerminalManager();
    m.ensure("s1");
    expect(typeof terminals[0].options.scrollback).toBe("number");
    expect(terminals[0].options.scrollback as number).toBeGreaterThan(0);
  });

  it("applyReplay writes replay content BEFORE open() (replay-before-open ordering)", () => {
    const m = new TerminalManager();
    m.ensure("s1");
    m.applyReplay("s1", 100, 40, new Uint8Array([104, 105]));
    const container = makeContainer();
    m.open("s1", container);
    const firstWrite = calls.indexOf("write");
    const firstOpen = calls.indexOf("open");
    expect(firstWrite).toBeGreaterThanOrEqual(0);
    expect(firstOpen).toBeGreaterThanOrEqual(0);
    expect(firstWrite).toBeLessThan(firstOpen);
    // replay resized to snapshot dims before writing
    expect(terminals[0].resize).toHaveBeenCalledWith(100, 40);
  });

  it("attach() wires the Channel and applies Replay before Output, never touching the store", () => {
    const m = new TerminalManager();
    m.ensure("s1");
    void m.attach("s1");
    expect(attachSessionMock).toHaveBeenCalledTimes(1);
    expect(attachSessionMock.mock.calls[0][0]).toBe("s1");
    expect(newTerminalChannelMock).toHaveBeenCalledTimes(1);
    expect(lastChannelHandler).toBeTypeOf("function");

    const replay: TerminalEvent = {
      event: "replay",
      data: { cols: 90, rows: 30, content: [104, 105] },
    };
    lastChannelHandler!(replay);
    expect(terminals[0].resize).toHaveBeenCalledWith(90, 30);
    expect(terminals[0].write).toHaveBeenCalledWith(new Uint8Array([104, 105]), expect.any(Function));

    const output: TerminalEvent = { event: "output", data: { bytes: [10, 20] } };
    lastChannelHandler!(output);
    expect(terminals[0].write).toHaveBeenCalledWith(new Uint8Array([10, 20]), expect.any(Function));

    // replay happened strictly before open() when open() is called afterwards
    const container = makeContainer();
    m.open("s1", container);
    const opensAfterFirstWrite = calls.indexOf("open") > calls.indexOf("write");
    expect(opensAfterFirstWrite).toBe(true);
  });

  it("isOpened reflects whether open() has been called, and survives hide()", () => {
    const m = new TerminalManager();
    m.ensure("s1");
    expect(m.isOpened("s1")).toBe(false);
    m.open("s1", makeContainer());
    expect(m.isOpened("s1")).toBe(true);
    m.hide("s1");
    expect(m.isOpened("s1")).toBe(true);
  });

  it("keep-alive: nothing is disposed when a panel merely unmounts (no dispose call)", () => {
    const m = new TerminalManager();
    m.ensure("s1");
    m.open("s1", makeContainer());
    // simulate unmount by simply NOT calling dispose; instance stays alive
    m.hide("s1");
    expect(terminals[0].disposed).toBe(false);
    expect(m.has("s1")).toBe(true);
    // re-open into a new container reuses the same instance
    m.open("s1", makeContainer());
    expect(terminals).toHaveLength(1);
    expect(terminals[0].open).toHaveBeenCalledTimes(2);
  });

  it("hide() disposes the WebGL addon but not the Terminal (frees the WebGL context budget)", () => {
    const m = new TerminalManager();
    m.ensure("s1");
    m.open("s1", makeContainer());
    expect(contextLossCb).toBeTypeOf("function");
    m.hide("s1");
    expect(webglDispose).toHaveBeenCalledTimes(1);
    expect(terminals[0].disposed).toBe(false);
    expect(m.has("s1")).toBe(true);
  });

  it("dispose() only on real close: tears the instance down and forgets it", () => {
    const m = new TerminalManager();
    m.ensure("s1");
    m.open("s1", makeContainer());
    m.dispose("s1");
    expect(terminals[0].disposed).toBe(true);
    expect(m.has("s1")).toBe(false);
    expect(m.get("s1")).toBeUndefined();
  });

  it("onData forwards keystrokes to write_stdin IPC", () => {
    const m = new TerminalManager();
    m.ensure("s1");
    m.open("s1", makeContainer());
    terminals[0].onDataCb!("l");
    expect(writeStdinMock).toHaveBeenCalledWith("s1", "l");
  });

  it("onResize forwards fitted dims to resize IPC", () => {
    const m = new TerminalManager();
    m.ensure("s1");
    m.open("s1", makeContainer());
    terminals[0].onResizeCb!({ cols: 120, rows: 30 });
    expect(resizeMock).toHaveBeenCalledWith("s1", 120, 30);
  });

  it("debounced ResizeObserver triggers fitAddon.fit() after the debounce window", () => {
    vi.useFakeTimers();
    try {
      const m = new TerminalManager();
      m.ensure("s1");
      m.open("s1", makeContainer());
      fitMock.mockClear();
      expect(resizeObserverCb).toBeTypeOf("function");
      resizeObserverCb!();
      resizeObserverCb!();
      resizeObserverCb!();
      expect(fitMock).not.toHaveBeenCalled();
      vi.runAllTimers();
      expect(fitMock).toHaveBeenCalledTimes(1);
    } finally {
      vi.useRealTimers();
    }
  });

  it("guards against opening/fitting a zero-dimension container", () => {
    const m = new TerminalManager();
    m.ensure("s1");
    const el = document.createElement("div");
    Object.defineProperty(el, "clientWidth", { value: 0, configurable: true });
    Object.defineProperty(el, "clientHeight", { value: 0, configurable: true });
    document.body.appendChild(el);
    fitMock.mockClear();
    m.open("s1", el);
    expect(terminals[0].open).toHaveBeenCalledTimes(1);
    expect(fitMock).not.toHaveBeenCalled();
  });

  it("WebGL context loss disposes the webgl addon (DOM fallback), NOT the Terminal", () => {
    const m = new TerminalManager();
    m.ensure("s1");
    m.open("s1", makeContainer());
    expect(contextLossCb).toBeTypeOf("function");
    contextLossCb!();
    expect(webglDispose).toHaveBeenCalledTimes(1);
    expect(terminals[0].disposed).toBe(false);
  });

  it("writeOutput goes straight to term.write (bytes never returned/stored)", () => {
    const m = new TerminalManager();
    m.ensure("s1");
    const ret = m.writeOutput("s1", new Uint8Array([65, 66]));
    expect(terminals[0].write).toHaveBeenCalledWith(
      new Uint8Array([65, 66]),
      expect.any(Function),
    );
    expect(ret).toBeUndefined();
  });

  it("tracks pending bytes via the write callback and clears them once flushed", () => {
    const m = new TerminalManager();
    m.ensure("s1");
    // FakeTerminal.write invokes the callback synchronously, so pending returns to 0
    m.writeOutput("s1", new Uint8Array(50));
    expect(m.pendingBytes("s1")).toBe(0);
  });

  it("logs at the HIGH watermark when pending bytes are not flushed synchronously", () => {
    const consoleWarn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const m = new TerminalManager();
    m.ensure("s1");
    // make write() NOT invoke its callback (simulates a slow/backed-up renderer)
    terminals[0].write = vi.fn((_d: unknown) => {
      calls.push("write");
    });
    const big = new Uint8Array(120_000); // > 100 KB HIGH watermark
    m.writeOutput("s1", big);
    expect(m.pendingBytes("s1")).toBeGreaterThanOrEqual(100_000);
    expect(consoleWarn).toHaveBeenCalled();
    consoleWarn.mockRestore();
  });

  it("disposeAll tears down every instance and empties the map", () => {
    const m = new TerminalManager();
    m.ensure("s1");
    m.ensure("s2");
    m.disposeAll();
    expect(m.has("s1")).toBe(false);
    expect(m.has("s2")).toBe(false);
    expect(terminals.every((t) => t.disposed)).toBe(true);
  });

  // ---- A1: per-session attach tracking (dead-pane fix) ----

  it("attach() is idempotent per session: a second attach for the same id is a no-op", async () => {
    const m = new TerminalManager();
    m.ensure("s1");
    await m.attach("s1");
    await m.attach("s1");
    // deduped: only ONE daemon-side attach / one Channel wired for s1
    expect(attachSessionMock).toHaveBeenCalledTimes(1);
    expect(newTerminalChannelMock).toHaveBeenCalledTimes(1);
    expect(m.isAttached("s1")).toBe(true);
  });

  it("attach() tracks each session independently (s2 attaches even though s1 already did)", async () => {
    const m = new TerminalManager();
    m.ensure("s1");
    m.ensure("s2");
    await m.attach("s1");
    await m.attach("s2");
    expect(attachSessionMock).toHaveBeenCalledTimes(2);
    expect(attachSessionMock.mock.calls.map((c) => c[0])).toEqual(["s1", "s2"]);
    expect(m.isAttached("s1")).toBe(true);
    expect(m.isAttached("s2")).toBe(true);
  });

  it("a FAILED attach is not recorded and is retryable", async () => {
    const m = new TerminalManager();
    m.ensure("s1");
    attachSessionMock.mockRejectedValueOnce(new Error("daemon down"));
    await expect(m.attach("s1")).rejects.toThrow("daemon down");
    expect(m.isAttached("s1")).toBe(false);
    // retry succeeds and this time records the attachment
    await m.attach("s1");
    expect(m.isAttached("s1")).toBe(true);
    expect(attachSessionMock).toHaveBeenCalledTimes(2);
  });

  it("resetAttachment(id) clears one session's flag so the next attach re-runs (fresh Replay)", async () => {
    const m = new TerminalManager();
    m.ensure("s1");
    await m.attach("s1");
    expect(attachSessionMock).toHaveBeenCalledTimes(1);
    m.resetAttachment("s1");
    expect(m.isAttached("s1")).toBe(false);
    await m.attach("s1");
    expect(attachSessionMock).toHaveBeenCalledTimes(2); // re-attached after reset
  });

  it("resetAllAttachments() clears every session's flag (reconnect: all re-attach fresh)", async () => {
    const m = new TerminalManager();
    m.ensure("s1");
    m.ensure("s2");
    await m.attach("s1");
    await m.attach("s2");
    m.resetAllAttachments();
    expect(m.isAttached("s1")).toBe(false);
    expect(m.isAttached("s2")).toBe(false);
    // both re-attach (visible one eagerly, hidden one lazily) with a fresh Replay
    await m.attach("s1");
    await m.attach("s2");
    expect(attachSessionMock).toHaveBeenCalledTimes(4);
  });

  it("dispose() clears attach state so a same-id session recreated later does not false-dedup", async () => {
    const m = new TerminalManager();
    m.ensure("s1");
    await m.attach("s1");
    m.dispose("s1");
    expect(m.isAttached("s1")).toBe(false);
    // recreate a session that happens to reuse the id -> must attach again, not dedup
    m.ensure("s1");
    await m.attach("s1");
    expect(attachSessionMock).toHaveBeenCalledTimes(2);
    expect(m.isAttached("s1")).toBe(true);
  });

  it("attach() on an unknown (never-ensured / disposed) session is a no-op, records nothing", async () => {
    const m = new TerminalManager();
    await m.attach("ghost");
    expect(attachSessionMock).not.toHaveBeenCalled();
    expect(m.isAttached("ghost")).toBe(false);
  });
});
