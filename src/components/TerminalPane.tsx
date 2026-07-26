import { useEffect, useRef, useSyncExternalStore, type JSX } from "react";
import type { SessionId } from "../ipc/commands";
import type { TerminalManager } from "../terminal/terminal-manager";
import { useAppStore } from "../store/store";
import { strings } from "../strings";

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
  // Session ids this pane already showed the FE-7 restored-input hint for — keyed by id because
  // this ONE instance is reused across tab switches (no React `key`), so a plain boolean would
  // latch on the first restored session and mute the hint for every later one.
  const restoredHintedRef = useRef<Set<SessionId>>(new Set());

  // Honest attach-failure surface (AUD-2026-07-19-01): the manager records the last failed
  // `attach_session` per session; this subscription re-renders ONLY on error/clear transitions
  // (never per byte). Non-null → the overlay note + Retry below.
  const attachError = useSyncExternalStore(manager.subscribeAttachErrors, () =>
    manager.getAttachError(sessionId),
  );

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

  // FE-7: typing into a RESTORED session (`isActive === false`, not exited — the PTY is gone
  // after a daemon restart) is silently swallowed by `writeStdin`, so surface a hint the FIRST
  // time the owner types, once per session. The store is read via `getState()` INSIDE the xterm
  // callback — deliberately NO subscription — so live sessions see zero extra renders and zero
  // behavior change. The pane instance is reused across tab switches (no React `key`), so the
  // "already hinted" memory must be keyed by session id, not a per-instance boolean.
  useEffect(() => {
    const term = manager.ensure(sessionId); // idempotent — the attach effect created it already
    const sub = term.onData(() => {
      const s = useAppStore.getState();
      const meta = s.sessions[sessionId];
      if (
        meta !== undefined &&
        !meta.isActive &&
        meta.lifecycle.kind !== "exited" &&
        !restoredHintedRef.current.has(sessionId)
      ) {
        restoredHintedRef.current.add(sessionId);
        s.showToast(strings.terminal.restoredInputHint);
      }
    });
    return () => sub.dispose();
  }, [sessionId, manager]);

  return (
    <div style={{ position: "relative", width: "100%", height: "100%" }}>
      <div
        data-testid={`terminal-pane-${sessionId}`}
        ref={containerRef}
        style={{
          width: "100%",
          height: "100%",
          background: "var(--bg)",
        }}
      />
      {attachError !== undefined && (
        <div
          role="alert"
          data-testid="terminal-attach-error"
          style={{
            position: "absolute",
            top: "var(--sp-3)",
            left: "50%",
            transform: "translateX(-50%)",
            display: "flex",
            alignItems: "center",
            gap: "var(--sp-3)",
            padding: "var(--sp-2) var(--sp-3)",
            // Tone edge as an inset shadow — a border-left under a radius renders as a wedge.
            boxShadow: "inset 3px 0 0 var(--danger)",
            borderRadius: "var(--r-sm)",
            background: "var(--danger-weak)",
            color: "var(--danger)",
            fontSize: "var(--fs-sm)",
            maxWidth: "90%",
          }}
        >
          <span>{strings.terminal.attachFailed(attachError)}</span>
          <button
            type="button"
            data-testid="terminal-attach-retry"
            onClick={() => void manager.attach(sessionId)}
            style={{
              border: "none",
              borderRadius: "var(--r-sm)",
              background: "var(--panel)",
              color: "var(--danger)",
              cursor: "pointer",
              fontSize: "var(--fs-sm)",
              padding: "0 var(--sp-2)",
              flexShrink: 0,
            }}
          >
            {strings.terminal.attachRetry}
          </button>
        </div>
      )}
    </div>
  );
}
