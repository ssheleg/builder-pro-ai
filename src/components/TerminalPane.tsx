import { useEffect, useRef, type JSX } from "react";
import type { SessionId } from "../ipc/commands";
import type { TerminalManager } from "../terminal/terminal-manager";
import { theme } from "../theme";

/**
 * Hosts one session's xterm Terminal. App mounts exactly one `TerminalPane` — the ACTIVE
 * session's — at a time, rendered WITHOUT a React `key`, so switching tabs does NOT
 * unmount/remount a fresh instance: React REUSES this single component instance and only
 * this effect re-runs (its `sessionId` dep changes). The underlying `Terminal` is not tied
 * to this component's lifecycle either — it lives in the non-reactive `TerminalManager`
 * map, so a hidden pane's terminal keeps buffering incoming bytes (spec §12 keep-alive) and
 * is instantly re-shown (scrollback + all) when its tab becomes active again.
 *
 * On every effect run (initial mount AND each tab switch): `ensure()` (idempotent — returns
 * the existing Terminal if already created), then `attach()` UNCONDITIONALLY, then `open()`
 * into this pane's container. Attach dedup is owned by the manager per-SESSION (A1) — a
 * component-instance guard would latch on the first session shown and leave every later tab a
 * dead pane (no Replay, no Output), because the reused instance's ref never resets across the
 * `sessionId` change.
 *
 * This effect is NOT StrictMode-safe on its own: React 19 double-invokes it on mount, firing
 * two synchronous `attach()` calls for the same session before the first round-trip resolves
 * (same shape as a rapid tab-away/back, or reconnect's eager re-attach racing this effect).
 * Safety lives ENTIRELY in the manager: `attach()` marks the session `attaching`
 * synchronously and coalesces concurrent callers onto the ONE in-flight promise — a second
 * call wires no second Channel and fires no second `attach_session`, so the daemon Replay is
 * written into the single xterm exactly once (see `TerminalManager.attach` — the coalescing
 * contract). `attach()` wires the Channel<TerminalEvent> firehose (Replay-before-open + live
 * Output); it is a resolved no-op once the session is already attached.
 *
 * On effect cleanup (tab switch away, or real unmount): `hide()` — KEEP-ALIVE. This does
 * NOT dispose the Terminal; the instance stays in the manager's non-reactive map so
 * scrollback + the PTY binding survive. `dispose()` is only ever called from TerminalTabs'
 * close button (a real session close), never here.
 */
export function TerminalPane(props: {
  sessionId: SessionId;
  manager: TerminalManager;
}): JSX.Element {
  const { sessionId, manager } = props;
  const containerRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    manager.ensure(sessionId);
    // Call unconditionally: the manager dedupes per-session (A1). This is what makes the
    // second-and-later tabs live — the old per-instance latch never reset across the
    // reused instance's sessionId change, so later tabs were dead panes.
    void manager.attach(sessionId); // wires Replay-before-open + Output firehose (deduped)
    manager.open(sessionId, container);

    return () => {
      // Keep-alive (spec §12): do NOT dispose on unmount. The Terminal survives in
      // the manager; only a real close (TerminalTabs) calls dispose().
      manager.hide(sessionId);
    };
  }, [sessionId, manager]);

  return (
    <div
      data-testid={`terminal-pane-${sessionId}`}
      ref={containerRef}
      style={{
        width: "100%",
        height: "100%",
        background: theme.colors.bg,
      }}
    />
  );
}
