import { useEffect, useRef } from "react";
import type { SessionId } from "../ipc/commands";
import type { TerminalManager } from "../terminal/terminal-manager";
import { theme } from "../theme";

/**
 * Hosts one session's xterm Terminal. The DOM container is always mounted while its
 * session exists in the store (App only unmounts a pane when the session is removed);
 * only `display` toggles when it isn't the active tab, so a hidden Terminal keeps
 * buffering incoming bytes (spec §12 keep-alive).
 *
 * On mount: `ensure()` (idempotent — returns the existing Terminal if already created,
 * which makes React 19 StrictMode's double-invoke of effects safe), `attach()` once
 * (wires the Channel<TerminalEvent> firehose: Replay-before-open + live Output), then
 * `open()` into this pane's container.
 *
 * On unmount: `hide()` — KEEP-ALIVE. This does NOT dispose the Terminal; the instance
 * stays in the manager's non-reactive map so scrollback + the PTY binding survive
 * across tab switches. `dispose()` is only ever called from TerminalTabs' close button
 * (a real session close), never here.
 */
export function TerminalPane(props: {
  sessionId: SessionId;
  manager: TerminalManager;
}): JSX.Element {
  const { sessionId, manager } = props;
  const containerRef = useRef<HTMLDivElement>(null);
  const attachedRef = useRef(false);

  useEffect(() => {
    const container = containerRef.current;
    if (!container) return;

    manager.ensure(sessionId);
    if (!attachedRef.current) {
      attachedRef.current = true;
      void manager.attach(sessionId); // wires Replay-before-open + Output firehose
    }
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
