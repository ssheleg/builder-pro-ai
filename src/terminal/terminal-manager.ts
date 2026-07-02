import "@xterm/xterm/css/xterm.css";
import { Terminal, type ITerminalOptions } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";
import type { TerminalEvent } from "../ipc/types";
import type { SessionId } from "../ipc/commands";
import { writeStdin, resize, attachSession } from "../ipc/commands";
import { newTerminalChannel } from "../ipc/channel";

/** Debounce window for the container `ResizeObserver` -> `fitAddon.fit()` (spec §12). */
const RESIZE_DEBOUNCE_MS = 100;

/**
 * Flow-control watermarks (spec §12 flow control note). S1 has no protocol "pause PTY"
 * message — the daemon's bounded outq (spec §13) is the real backpressure mechanism.
 * This is a best-effort frontend-side guard: track bytes handed to `term.write()` that
 * have not yet been flushed (via the write callback) and log at the HIGH watermark so a
 * backed-up renderer is visible in the console instead of silently piling up.
 */
const WATERMARK_HIGH_BYTES = 100 * 1024;
const WATERMARK_LOW_BYTES = 10 * 1024;

const TERMINAL_OPTIONS: ITerminalOptions = {
  convertEol: false, // real PTY (termios) handles \n -> \r\n; do not double it up
  scrollback: 10_000,
  cursorBlink: true,
  fontFamily: 'Menlo, "SF Mono", "JetBrains Mono", ui-monospace, monospace',
  fontSize: 13,
  allowProposedApi: true,
};

interface TerminalEntry {
  term: Terminal;
  fit: FitAddon;
  webgl: WebglAddon | undefined;
  resizeObserver: ResizeObserver | undefined;
  resizeTimer: ReturnType<typeof setTimeout> | undefined;
  /** True once `open()` has been called at least once (survives `hide()`/re-`open()`). */
  opened: boolean;
  /** Bytes handed to `term.write()` whose flush callback has not fired yet. */
  pendingBytes: number;
  /** Latched true once pendingBytes crosses HIGH, cleared once it drops back below LOW. */
  overWatermark: boolean;
}

/**
 * Owns xterm `Terminal` instances OUTSIDE React state (spec §12): a non-reactive
 * `Map<SessionId, TerminalEntry>` module/instance field. React components borrow a ref
 * to the Terminal via `ensure()`/`get()`; a Terminal is never held in `useState`.
 *
 * Keep-alive (spec §12): panel unmount/hide never calls `term.dispose()` — the instance
 * stays in the map and keeps buffering bytes. Only a real session close (kill/exit) calls
 * `dispose(sessionId)`, which tears the Terminal (and its addons/listeners) down and
 * removes the map entry.
 */
export class TerminalManager {
  private entries = new Map<SessionId, TerminalEntry>();

  /**
   * Create (or return the existing) Terminal for `sessionId`. Idempotent — calling
   * twice with the same id returns the SAME instance, which is what makes React 19
   * StrictMode's double-invoke of effects safe. Does NOT call `term.open()`.
   */
  ensure(sessionId: SessionId, cols?: number, rows?: number): Terminal {
    const existing = this.entries.get(sessionId);
    if (existing) return existing.term;

    const term = new Terminal(TERMINAL_OPTIONS);
    const fit = new FitAddon();
    term.loadAddon(fit);

    if (typeof cols === "number" && typeof rows === "number") {
      term.resize(cols, rows);
    }

    term.onData((data) => {
      void writeStdin(sessionId, data);
    });
    term.onResize(({ cols: c, rows: r }) => {
      void resize(sessionId, c, r);
    });

    this.entries.set(sessionId, {
      term,
      fit,
      webgl: undefined,
      resizeObserver: undefined,
      resizeTimer: undefined,
      opened: false,
      pendingBytes: 0,
      overWatermark: false,
    });
    return term;
  }

  has(sessionId: SessionId): boolean {
    return this.entries.has(sessionId);
  }

  get(sessionId: SessionId): Terminal | undefined {
    return this.entries.get(sessionId)?.term;
  }

  /** Has `open()` been called at least once for this session (regardless of `hide()`)? */
  isOpened(sessionId: SessionId): boolean {
    return this.entries.get(sessionId)?.opened ?? false;
  }

  /** Bytes handed to `term.write()` that have not been flushed yet (flow-control gauge). */
  pendingBytes(sessionId: SessionId): number {
    return this.entries.get(sessionId)?.pendingBytes ?? 0;
  }

  /**
   * Wire the daemon firehose for this session (spec §6.2): build a `Channel<TerminalEvent>`
   * and `attach_session`. `Replay` (first message) resizes to the snapshot dims and writes
   * its content; `Output` streams straight to xterm. Bytes NEVER touch React/Zustand state
   * — the handler writes directly to the Terminal held in the non-reactive map.
   */
  async attach(sessionId: SessionId): Promise<void> {
    const channel = newTerminalChannel((e: TerminalEvent) => {
      if (e.event === "replay") {
        this.applyReplay(
          sessionId,
          e.data.cols,
          e.data.rows,
          new Uint8Array(e.data.content),
        );
      } else {
        this.writeOutput(sessionId, new Uint8Array(e.data.bytes));
      }
    });
    await attachSession(sessionId, channel);
  }

  /**
   * Replay = sanitized scrollback ring (spec §11): resize to the snapshot dims, then write
   * the bytes. `term.write()` before `term.open()` is buffered by xterm and rendered on
   * open, so calling this before the container mounts satisfies "replay before open"
   * naturally — the Replay's cols/rows size the terminal before its bytes are written.
   */
  applyReplay(
    sessionId: SessionId,
    cols: number,
    rows: number,
    content: Uint8Array,
  ): void {
    const entry = this.entries.get(sessionId);
    if (!entry) return;
    entry.term.resize(cols, rows);
    this.trackedWrite(entry, content);
  }

  /** Live PTY firehose. Bytes go straight to xterm — NEVER into React/Zustand state. */
  writeOutput(sessionId: SessionId, bytes: Uint8Array): void {
    const entry = this.entries.get(sessionId);
    if (!entry) return;
    this.trackedWrite(entry, bytes);
  }

  /** `term.write` with pending-byte tracking for the flow-control watermark (spec §12). */
  private trackedWrite(entry: TerminalEntry, data: Uint8Array): void {
    entry.pendingBytes += data.byteLength;
    if (!entry.overWatermark && entry.pendingBytes >= WATERMARK_HIGH_BYTES) {
      entry.overWatermark = true;
      console.warn(
        `[terminal-manager] pending write bytes crossed HIGH watermark (${entry.pendingBytes} >= ${WATERMARK_HIGH_BYTES}); renderer may be falling behind`,
      );
    }
    entry.term.write(data, () => {
      entry.pendingBytes = Math.max(0, entry.pendingBytes - data.byteLength);
      if (entry.overWatermark && entry.pendingBytes <= WATERMARK_LOW_BYTES) {
        entry.overWatermark = false;
      }
    });
  }

  /**
   * Attach the Terminal to a DOM container. Safe to call again on re-show (keep-alive):
   * re-opens the same instance into the new container and re-arms WebGL + ResizeObserver.
   * Guards against opening/fitting a zero-dimension container.
   */
  open(sessionId: SessionId, container: HTMLElement): void {
    const entry = this.entries.get(sessionId);
    if (!entry) return;

    this.teardownContainer(entry);
    entry.term.open(container);
    entry.opened = true;

    const hasSize = () =>
      container.clientWidth > 0 && container.clientHeight > 0;

    if (hasSize()) {
      this.enableWebgl(entry);
      entry.fit.fit(); // -> term.resize() -> onResize -> resize() IPC
    }

    const observer = new ResizeObserver(() => {
      if (entry.resizeTimer) clearTimeout(entry.resizeTimer);
      entry.resizeTimer = setTimeout(() => {
        if (hasSize()) {
          entry.fit.fit();
        }
      }, RESIZE_DEBOUNCE_MS);
    });
    observer.observe(container);
    entry.resizeObserver = observer;
  }

  /**
   * Panel unmount/hide (keep-alive, spec §12): do NOT dispose the Terminal — only free the
   * WebGL context budget (≤16 contexts/page) by disposing the WebGL addon, and stop the
   * ResizeObserver on the (about to be removed) container. Bytes keep buffering in xterm.
   */
  hide(sessionId: SessionId): void {
    const entry = this.entries.get(sessionId);
    if (!entry) return;
    this.teardownContainer(entry);
    this.disableWebgl(entry);
  }

  /** Lazy WebGL only on a visible/opened terminal; DOM fallback on context loss. */
  private enableWebgl(entry: TerminalEntry): void {
    if (entry.webgl) return;
    try {
      const webgl = new WebglAddon();
      webgl.onContextLoss(() => {
        webgl.dispose(); // -> Terminal falls back to the DOM renderer
        entry.webgl = undefined;
      });
      entry.term.loadAddon(webgl);
      entry.webgl = webgl;
    } catch {
      // WebGL unavailable -> DOM renderer (no-op); honest degradation, no throw.
      entry.webgl = undefined;
    }
  }

  private disableWebgl(entry: TerminalEntry): void {
    if (!entry.webgl) return;
    entry.webgl.dispose();
    entry.webgl = undefined;
  }

  private teardownContainer(entry: TerminalEntry): void {
    if (entry.resizeTimer) {
      clearTimeout(entry.resizeTimer);
      entry.resizeTimer = undefined;
    }
    entry.resizeObserver?.disconnect();
    entry.resizeObserver = undefined;
  }

  /**
   * ONLY on real session close (kill/exit). Disposes the Terminal (and, transitively, its
   * loaded addons) and forgets the entry. NEVER call this on panel unmount/hide.
   */
  dispose(sessionId: SessionId): void {
    const entry = this.entries.get(sessionId);
    if (!entry) return;
    this.teardownContainer(entry);
    entry.webgl = undefined; // disposed transitively by term.dispose()
    entry.term.dispose();
    this.entries.delete(sessionId);
  }

  disposeAll(): void {
    for (const id of Array.from(this.entries.keys())) this.dispose(id);
  }
}
