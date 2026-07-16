import "@xterm/xterm/css/xterm.css";
import {
  Terminal,
  type ITerminalOptions,
  type IDisposable,
  type ILink,
  type ILinkProvider,
} from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";
import { open as openUrl } from "@tauri-apps/plugin-shell";
import type { TerminalEvent } from "../ipc/types";
import type { SessionId } from "../ipc/commands";
import { writeStdin, resize, attachSession } from "../ipc/commands";
import { newTerminalChannel } from "../ipc/channel";
import { useAppStore } from "../store/store";
import { findFileLinks, matchWorkspaceRoot } from "./link-provider";
import { strings } from "../strings";

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

/**
 * Decode a `file://` URL to a filesystem path (spec §6.5 OSC-8 handling). Returns `""` (which
 * `matchWorkspaceRoot` will never resolve under any root) on a malformed URL rather than
 * throwing -- OSC-8 payloads originate from arbitrary shell output the terminal does not trust.
 */
function fileUrlToPath(url: string): string {
  try {
    return decodeURIComponent(new URL(url).pathname);
  } catch {
    return "";
  }
}

const TERMINAL_OPTIONS: ITerminalOptions = {
  convertEol: false, // real PTY (termios) handles \n -> \r\n; do not double it up
  scrollback: 10_000,
  cursorBlink: true,
  fontFamily: 'Menlo, "SF Mono", "JetBrains Mono", ui-monospace, monospace',
  fontSize: 13,
  allowProposedApi: true,
};

/**
 * Per-session attach lifecycle (spec §12 / A1 + A2 coalescing).
 * - `detached`  — no firehose wired; `attach()` will start one.
 * - `attaching` — an `attach_session` IPC is IN FLIGHT; concurrent `attach()` callers
 *   coalesce onto the SAME in-flight promise (no second IPC / Channel).
 * - `attached`  — the firehose is live (a resolved `attach_session`).
 */
type AttachState = "detached" | "attaching" | "attached";

interface TerminalEntry {
  term: Terminal;
  fit: FitAddon;
  webgl: WebglAddon | undefined;
  resizeObserver: ResizeObserver | undefined;
  resizeTimer: ReturnType<typeof setTimeout> | undefined;
  /** True once `open()` has been called at least once (survives `hide()`/re-`open()`). */
  opened: boolean;
  /**
   * Attach lifecycle for this session (spec §12 / A1 + A2). Owned per-session by the
   * manager, NOT by the React pane — panes are reused across tab switches (App renders a
   * single `TerminalPane` with no `key`), so a pane-instance guard would latch on the first
   * session and leave every later tab a dead pane. `attaching` is set SYNCHRONOUSLY (before
   * any await) so two `attach()` calls issued before the first resolves — React 19
   * StrictMode's double-invoke of the pane effect, a tab-away/back within one round-trip, or
   * reconnect's eager re-attach racing a pane-effect attach — coalesce onto the one in-flight
   * promise instead of each firing their own `attach_session` (which duplicated the daemon
   * Replay into the single xterm). A rejected attach returns to `detached` (retryable).
   * Reset back to `detached` by `resetAttachment`/`resetAllAttachments` (reconnect) and by
   * `dispose` (real close), so a re-shown or reconnected session re-attaches with a fresh
   * Replay.
   */
  attach: AttachState;
  /**
   * The in-flight `attach()` promise while `attach === "attaching"` (undefined otherwise).
   * Concurrent callers return THIS so all of them observe the one round-trip's success or
   * failure — no caller starts a second IPC.
   */
  attachInFlight: Promise<void> | undefined;
  /**
   * Monotonic generation for this session, bumped by every reset/dispose. An in-flight
   * `attach()` captures the generation at fire time; if a reset (reconnect) or dispose races
   * the round-trip and bumps it, the stale completion refuses to mark `attached` — the next
   * `attach()` re-attempts with a fresh Replay. Without this a reconnect's
   * `resetAllAttachments()` during an in-flight attach would record a stale attachment.
   */
  attachGeneration: number;
  /** Bytes handed to `term.write()` whose flush callback has not fired yet. */
  pendingBytes: number;
  /** Latched true once pendingBytes crosses HIGH, cleared once it drops back below LOW. */
  overWatermark: boolean;
  /**
   * Disposable returned by `term.registerLinkProvider` (spec §6.5/D9 — terminal file links).
   * Registered once in `ensure()` alongside `term.options.linkHandler`, released in `dispose()`
   * so an unregistered provider callback is never retained by a torn-down Terminal (no leak).
   */
  linkProviderDisposable: IDisposable;
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

    // Terminal file links (spec §6.5/D9) -- register right after construction, once per session
    // (NOT in `open()`, which re-runs on every re-show/keep-alive cycle and would re-register a
    // duplicate provider each time).
    const linkProviderDisposable = this.wireFileLinks(sessionId, term);

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
      attach: "detached",
      attachInFlight: undefined,
      attachGeneration: 0,
      pendingBytes: 0,
      overWatermark: false,
      linkProviderDisposable,
    });
    return term;
  }

  /**
   * Register the file-link provider + OSC-8 `linkHandler` for one terminal (spec §6.5/D9). Two
   * independent surfaces, both resolving against THIS session's live cwd/roots (re-read fresh
   * from the store on every call, so a cwd change picked up by `setLifecycle`/OSC-7 or a
   * `workspace://updated` root change apply on the very next repaint/click with no extra wiring):
   *
   *  - `registerLinkProvider`: xterm calls `provideLinks(y, cb)` per visible buffer line. Reads
   *    that line as a string, runs the pure `findFileLinks` resolver (session `cwd` + the
   *    session's workspace `roots`), and maps each `ResolvedFileLink` to an `ILink` whose
   *    `activate` opens the file in the right-rail preview.
   *  - `options.linkHandler` (`allowNonHttpProtocols: true`): xterm's OWN OSC-8 hyperlink
   *    machinery (real `ESC]8;;URL BEL` escapes some shell tools emit, e.g. `ls --hyperlink`)
   *    hands the whole URL string to `activate`. A `file://` URL goes through the SAME
   *    validate-then-preview path via `matchWorkspaceRoot` (no cwd involved -- the URL is already
   *    absolute); `http(s)://` opens via the OS default handler (`@tauri-apps/plugin-shell`'s
   *    `open`, already covered by the `shell:default` capability's bundled `allow-open` scope --
   *    no Rust/capability change needed for this task); any other scheme is ignored (spec §6.5
   *    only names these two).
   *
   * Neither surface duplicates the authoritative outside-root/not-found check: `openFileLink`
   * below only sets `selectedFile` + opens the rail -- `FilePreview` (mounted by
   * `setFilesRailOpen(true)`) owns the real `readFilePreview` round-trip and already shows the
   * honest toast on a rejected `FsError` (see that component's doc comment). The ONE place this
   * method shows its OWN toast is the OSC-8 `file://` branch, whose `matchWorkspaceRoot` check
   * can reject a link BEFORE `openFileLink` is ever called (spec §6.5: "Paths outside roots ...
   * a failed click shows a quiet toast").
   *
   * Returns the `IDisposable` from `registerLinkProvider` so `dispose()` can release it.
   */
  private wireFileLinks(sessionId: SessionId, term: Terminal): IDisposable {
    const openFileLink = (root: string, rel: string): void => {
      const s = useAppStore.getState();
      s.setSelectedFile({ root, rel });
      s.setFilesRailOpen(true);
    };

    const provider: ILinkProvider = {
      provideLinks: (bufferLineNumber, callback) => {
        const bufferLine = term.buffer.active.getLine(bufferLineNumber - 1);
        if (!bufferLine) {
          callback(undefined);
          return;
        }
        const state = useAppStore.getState();
        const session = state.sessions[sessionId];
        const roots = session ? state.workspaces[session.workspaceId]?.roots : undefined;
        if (!session || !roots || roots.length === 0) {
          callback(undefined);
          return;
        }
        const lineText = bufferLine.translateToString(true);
        const resolved = findFileLinks(lineText, session.cwd, roots);
        if (resolved.length === 0) {
          callback(undefined);
          return;
        }
        const links: ILink[] = resolved.map((link) => ({
          range: {
            start: { x: link.startCol, y: bufferLineNumber },
            // `link.endCol` is 1-based EXCLUSIVE (one past the last char — see
            // `ResolvedFileLink.endCol`'s doc comment). xterm's `IBufferRange.end.x` is 1-based
            // INCLUSIVE (its internal link-lookup/removal both loop `x <= end.x`), so the
            // exclusive resolver column must be converted with a `-1` here or every link's
            // clickable/underlined hit-box extends one column past the token (e.g. into the
            // trailing space after a `path` token).
            end: { x: link.endCol - 1, y: bufferLineNumber },
          },
          text: lineText.slice(link.startCol - 1, link.endCol - 1),
          activate: () => openFileLink(link.root, link.rel),
        }));
        callback(links);
      },
    };

    term.options.linkHandler = {
      allowNonHttpProtocols: true,
      activate: (_event, text) => {
        if (/^https?:\/\//i.test(text)) {
          void openUrl(text);
          return;
        }
        if (text.startsWith("file://")) {
          const state = useAppStore.getState();
          const session = state.sessions[sessionId];
          const roots = session ? state.workspaces[session.workspaceId]?.roots : undefined;
          const path = fileUrlToPath(text);
          const match = path && roots ? matchWorkspaceRoot(path, roots) : null;
          if (match) {
            openFileLink(match.root, match.rel);
          } else {
            useAppStore.getState().showToast(strings.terminal.fileOutsideWorkspace);
          }
          return;
        }
        // non-file, non-http(s) scheme -> ignore (spec §6.5 only handles these two).
      },
    };

    return term.registerLinkProvider(provider);
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

  /**
   * Focus this session's Terminal (spec §6.2 "Go" — one-click jump into a waiting
   * terminal, ready to type). No-op for an unknown session or one that has never been
   * `open()`ed: xterm's own `focus()` needs a live DOM container, and a fresh Home "Go"
   * jump is often called BEFORE the newly-active pane's mount effect has run `open()` (React
   * batches the store updates that drive that render). This is best-effort — the pane's own
   * mount + attach() is what guarantees a correct, freshly-replayed terminal either way (BL-14
   * reset-before-replay); `focus()` merely saves a click for a session whose pane was already
   * open (e.g. jumping back into a workspace visited earlier in this session).
   */
  focus(sessionId: SessionId): void {
    const entry = this.entries.get(sessionId);
    if (entry?.opened) entry.term.focus();
  }

  /**
   * Has this session's daemon firehose been wired via a SUCCESSFUL `attach()`? (A1)
   * Per-session, manager-owned attach state — the source of truth panes and the reconnect
   * flow both consult. Only the settled `attached` state counts: an `attaching` (in-flight)
   * or `detached` session reports false. False for an unknown/disposed session.
   */
  isAttached(sessionId: SessionId): boolean {
    return this.entries.get(sessionId)?.attach === "attached";
  }

  /**
   * Force one session back to `detached` so the next `attach()` re-wires the firehose (fresh
   * Replay). Called on `daemon://reconnected` (per-session) and any place a single session
   * must be forced to re-attach. Bumps the session's generation so an attach that is IN
   * FLIGHT right now cannot later mark itself `attached` (the stale completion is invalidated
   * — see `attach`). No-op for an unknown session.
   */
  resetAttachment(sessionId: SessionId): void {
    const entry = this.entries.get(sessionId);
    if (entry) this.invalidateAttach(entry);
  }

  /**
   * Force EVERY session back to `detached`. Called on `daemon://reconnected` BEFORE the
   * re-attach pass: a daemon restart kills the shells and only replays scrollback up to the
   * last flush, so every session — the visible one (re-attached eagerly by App) and every
   * hidden one (re-attached lazily when its tab is next shown) — must re-attach with a
   * fresh Replay + live Output. Any in-flight attach is invalidated (generation bumped) so a
   * completion that lands after the reset does NOT record a stale attachment.
   */
  resetAllAttachments(): void {
    for (const entry of this.entries.values()) this.invalidateAttach(entry);
  }

  /**
   * Reset an entry's attach lifecycle to `detached` and bump its generation, invalidating any
   * in-flight `attach()` so its (stale) completion refuses to mark `attached`. Shared by
   * `resetAttachment`/`resetAllAttachments`/`dispose`.
   */
  private invalidateAttach(entry: TerminalEntry): void {
    entry.attach = "detached";
    entry.attachInFlight = undefined;
    entry.attachGeneration += 1;
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
   *
   * Coalescing contract (spec §12 / A1 + A2): the pane effect calls this UNCONDITIONALLY on
   * every mount and a pane instance is reused across tab switches, so the manager — not the
   * pane — owns dedup, and it does so per SESSION not per call:
   *   - already `attached`  -> resolved no-op (returns immediately).
   *   - `attaching`         -> returns the SAME in-flight promise; no second IPC / Channel.
   *   - unknown/disposed    -> resolved no-op, records nothing.
   *   - otherwise           -> mark `attaching` SYNCHRONOUSLY (before any await), fire ONE
   *                            `attach_session`, and on resolve mark `attached`.
   * Marking `attaching` before the first await is what makes two calls issued before the
   * first resolves (React 19 StrictMode's double-invoke, a tab-away/back within one
   * round-trip, or reconnect's eager re-attach racing a pane-effect attach) coalesce instead
   * of each duplicating the daemon Replay into the single xterm. A rejected `attach_session`
   * returns the session to `detached` (retryable). A reset/dispose that races the round-trip
   * bumps the session's generation, so a stale completion refuses to mark `attached`.
   */
  attach(sessionId: SessionId): Promise<void> {
    const entry = this.entries.get(sessionId);
    if (!entry) return Promise.resolve(); // unknown/disposed session -> record nothing
    if (entry.attach === "attached") return Promise.resolve(); // already live -> no-op
    if (entry.attach === "attaching" && entry.attachInFlight) {
      return entry.attachInFlight; // in flight -> coalesce onto the ONE round-trip
    }

    // Capture the generation SYNCHRONOUSLY so a reset/dispose during the await can invalidate
    // this specific attempt.
    const generation = entry.attachGeneration;
    entry.attach = "attaching";

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

    const inFlight = attachSession(sessionId, channel).then(
      () => {
        // Re-fetch: a dispose() could have replaced/removed the entry during the await.
        const live = this.entries.get(sessionId);
        // Only settle to `attached` if THIS attempt is still current — same live entry, same
        // generation (no reset/dispose raced), and still `attaching` (not already reset).
        if (
          live === entry &&
          live.attachGeneration === generation &&
          live.attach === "attaching"
        ) {
          live.attach = "attached";
          live.attachInFlight = undefined;
        }
      },
      (err: unknown) => {
        // Failed attach -> back to `detached` so it stays retryable, but never clobber a state
        // a concurrent reset/dispose already moved on to.
        const live = this.entries.get(sessionId);
        if (
          live === entry &&
          live.attachGeneration === generation &&
          live.attach === "attaching"
        ) {
          live.attach = "detached";
          live.attachInFlight = undefined;
        }
        // Propagate so callers CAN handle. NOTE: both production call sites currently `void`
        // this promise (TerminalPane effect, App reconnect), so a failed attach surfaces only
        // as an unhandledrejection warning and a detached-but-retryable pane; the user-visible
        // error surface is a known gap tracked in the backlog (error-surfacing contract).
        throw err;
      },
    );

    entry.attachInFlight = inFlight;
    return inFlight;
  }

  /**
   * Replay = sanitized scrollback ring (spec §11): reset the terminal, resize to the snapshot
   * dims, then write the bytes. `term.write()` before `term.open()` is buffered by xterm and
   * rendered on open, so calling this before the container mounts satisfies "replay before
   * open" naturally — the Replay's cols/rows size the terminal before its bytes are written.
   *
   * `term.reset()` FIRST (BL-14): every `attach()` — the initial mount AND any re-attach
   * (reconnect's `resetAllAttachments()`, or the Home "Go" hide/re-show -> fresh
   * `attach_session`) — receives a fresh FULL-scrollback Replay from the daemon, not a delta.
   * Without a reset, that fresh Replay lands on an xterm buffer that may already hold the
   * previous attach's content and is simply APPENDED, so the pane shows the scrollback twice
   * (BL-14; A1 verification, chip task_ada4835d). `reset()` clears the buffer + scrollback so
   * the incoming Replay REPLACES rather than appends. It runs before `trackedWrite` (the
   * pending-bytes/watermark gauge below), so the flow-control accounting for THIS write is
   * untouched — `reset()` is synchronous and does not itself go through `trackedWrite`, and any
   * earlier write's flush callback (already in flight before this reset) still fires on its own
   * schedule and decrements `pendingBytes` as normal; it is a best-effort telemetry gauge, not a
   * correctness-critical count, so a reset racing a slow-to-flush prior chunk is harmless.
   */
  applyReplay(
    sessionId: SessionId,
    cols: number,
    rows: number,
    content: Uint8Array,
  ): void {
    const entry = this.entries.get(sessionId);
    if (!entry) return;
    entry.term.reset();
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
    // Invalidate any in-flight attach (bump generation) so a completion that lands after this
    // dispose cannot resurrect attach state on a re-created same-id entry.
    this.invalidateAttach(entry);
    this.teardownContainer(entry);
    entry.webgl = undefined; // disposed transitively by term.dispose()
    entry.linkProviderDisposable.dispose();
    entry.term.dispose();
    this.entries.delete(sessionId);
  }

  disposeAll(): void {
    for (const id of Array.from(this.entries.keys())) this.dispose(id);
  }
}
