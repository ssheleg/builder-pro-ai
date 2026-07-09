import { useEffect, useRef, useState, type CSSProperties, type JSX } from "react";
import { useAppStore } from "../store/store";
import { getCommandEvents } from "../ipc/commands";
import type { SessionId } from "../ipc/commands";
import type { CommandEvent } from "../ipc/types";
import { StatusDot } from "./StatusDot";
import { theme } from "../theme";

const MONO_FONT = 'ui-monospace, SFMono-Regular, "SF Mono", Menlo, monospace';

/** Matches the `limit` spec §6.3 calls out ("last ~10 command_events"). */
const COMMAND_STRIP_LIMIT = 10;

type StripItem =
  | { key: string; kind: "outcome"; ok: boolean; exitCode: number | null }
  | { key: string; kind: "running" }
  | { key: string; kind: "interrupted" };

/**
 * Pair NEWEST-FIRST `CommandEvent`s (Pv2 §7 `command_events`, OSC-133 lifecycle) into strip items
 * (spec §6.3). Pairing decision (documented per the task brief — "if pairing is ambiguous, pick
 * the simplest honest rendering"):
 *
 * A session's PTY drives ONE interactive shell — foreground commands never overlap — so a single
 * command contributes exactly two rows: `started` then, later, `finished`. In NEWEST-FIRST order
 * a command's `finished` row therefore always sits immediately BEFORE (lower index than) its own
 * `started` row. Walking the array greedily, position-based (no field ties a pair together beyond
 * this adjacency — there is no command id in the table):
 *   - a `finished` event always becomes an outcome chip (✓ when `exitCode===0`, else ✗ + the
 *     code); if the event immediately after it is that command's `started`, both are consumed
 *     together (the `started` carries no extra information once the outcome is known, so it does
 *     not render its own chip);
 *   - a `started` event that does NOT immediately follow its own `finished` has no known outcome
 *     yet — this only happens for the newest entry (the command currently in flight). Whether that
 *     renders as "running" depends on `isLive` (the session's own `isActive`, honest-state rule —
 *     see `StatusDot.tsx`'s "exited always wins"): while the session is live, this genuinely is the
 *     command currently in flight (spec: "a trailing lone started renders as running-dot"); once
 *     the session is no longer live, the OSC-133 `finished` mark for it will NEVER arrive (a
 *     `kill -9`, a machine sleep, or a daemon crash all skip the shell's own exit trap), so a live
 *     "running" dot would lie about a dead process — it becomes an honest "interrupted" marker
 *     instead.
 * A `finished` with no adjacent `started` (e.g. its `started` fell off the `limit` page boundary)
 * still renders correctly on its own — only the cosmetic grouping is lost, never the outcome.
 */
function pairCommandEvents(events: CommandEvent[], isLive: boolean): StripItem[] {
  const items: StripItem[] = [];
  let i = 0;
  while (i < events.length) {
    const ev = events[i];
    if (ev.kind === "finished") {
      items.push({
        key: `f${ev.seq}`,
        kind: "outcome",
        ok: ev.exitCode === 0,
        exitCode: ev.exitCode,
      });
      i += events[i + 1]?.kind === "started" ? 2 : 1;
    } else if (ev.kind === "started") {
      items.push(
        isLive
          ? { key: `s${ev.seq}`, kind: "running" }
          : { key: `s${ev.seq}`, kind: "interrupted" },
      );
      i += 1;
    } else {
      // Defensive: an unrecognized `kind` (the Pv2 writer only ever persists "started"/"finished")
      // is skipped rather than mis-rendered as either known state.
      i += 1;
    }
  }
  return items;
}

/** Chip atom (design-system.md §5: "mono 11px, 1px border, radius 999; optional status dot;
 * counts use tabular-nums"). No shared `Chip.tsx` component exists yet in this codebase — this
 * style object is deliberately local (same precedent as `FileTree.tsx`/`FilePreview.tsx`'s
 * duplicated `describeFsError`: each component stays independently deployable). */
const chipBaseStyle: CSSProperties = {
  display: "inline-flex",
  alignItems: "center",
  gap: 4,
  padding: "2px 8px",
  borderRadius: 999,
  border: `1px solid ${theme.colors.border}`,
  fontFamily: MONO_FONT,
  fontSize: 11,
  fontVariantNumeric: "tabular-nums",
  background: theme.colors.bgElevated,
  flexShrink: 0,
};

const stripContainerStyle: CSSProperties = {
  display: "flex",
  alignItems: "center",
  gap: 6,
  padding: "6px 12px",
  overflowX: "auto",
  borderTop: `1px solid ${theme.colors.border}`,
  background: theme.colors.bgElevated,
};

const emptyStyle: CSSProperties = {
  padding: "6px 12px",
  fontSize: 11,
  fontFamily: MONO_FONT,
  color: theme.colors.textDim,
  borderTop: `1px solid ${theme.colors.border}`,
  background: theme.colors.bgElevated,
};

/**
 * Per-session command history strip (spec §6.3): the first honest consumer of the Pv2
 * `command_events` table. Fetches the active session's last `COMMAND_STRIP_LIMIT` events and
 * renders them as ✓/✗ outcome chips (see `pairCommandEvents` above for the started/finished
 * pairing rule), plus a running dot for the in-flight command, if any.
 *
 * Refetches whenever `sessionId` changes OR the session's own store entry gets a new object
 * reference — `setLifecycle`/`markExited` (App's `session://state-changed`/`session://exited`
 * subscriptions, spec §6.3 "refetch on state-changed/exited") always replace the whole `sessions`
 * entry with a fresh object, even when the visible fields end up equal, so selecting the entry
 * itself (not just `sessionId`) is a correct and sufficient refetch trigger with no separate IPC
 * subscription needed here.
 *
 * Error handling (spec §7): a rejected `getCommandEvents` fires a toast and renders nothing (never
 * a silent blank AND never a raw error dump). An empty result (no events yet, or a session
 * rehydrated from before the v2 `command_events` table existed) is NOT an error — spec §7 calls
 * this out explicitly ("honest, not an error") — so it renders a calm dim placeholder instead.
 */
export function CommandStrip(props: { sessionId: SessionId }): JSX.Element | null {
  const { sessionId } = props;
  const sessionMeta = useAppStore((s) => s.sessions[sessionId]);
  const showToast = useAppStore((s) => s.showToast);

  const [events, setEvents] = useState<CommandEvent[]>([]);
  const [failed, setFailed] = useState(false);
  // Token guard (same pattern as FilePreview.tsx's `requestRef`): a fast lifecycle churn (or a
  // tab switch) can leave an earlier request resolving AFTER a later one; only the latest
  // request's own token may still apply its result.
  const requestRef = useRef(0);

  useEffect(() => {
    const token = ++requestRef.current;
    // Clear synchronously so a session switch never flashes the PREVIOUS session's command
    // history while the new fetch is in flight (mirrors FilePreview's clear-on-selection-change).
    setEvents([]);
    setFailed(false);
    getCommandEvents(sessionId, COMMAND_STRIP_LIMIT)
      .then((evts) => {
        if (requestRef.current !== token) return;
        setEvents(evts);
      })
      .catch(() => {
        if (requestRef.current !== token) return;
        setFailed(true);
        showToast("Не удалось загрузить историю команд");
      });
  }, [sessionId, sessionMeta, showToast]);

  // An error already told the owner via the toast — no redundant inline error surface.
  if (failed) return null;

  // Honest-state input for pairing (see `pairCommandEvents` above): a lone `started` on a session
  // that is no longer live must never render as a live "running" dot. `sessionMeta` can be
  // momentarily undefined (e.g. the very first render before the store hydrates); default to
  // `true` (live) rather than `false` — the fallback is the pre-existing "running" behavior, so an
  // unresolved session never gets mis-flagged as interrupted.
  const isLive = sessionMeta?.isActive ?? true;
  const items = pairCommandEvents(events, isLive);

  if (items.length === 0) {
    return <div data-testid="command-strip-empty" style={emptyStyle}>Пока нет команд</div>;
  }

  return (
    <div
      data-testid="command-strip"
      role="list"
      aria-label="История команд"
      style={stripContainerStyle}
    >
      {items.map((item) =>
        item.kind === "running" ? (
          <span
            key={item.key}
            role="listitem"
            data-testid="command-chip-running"
            style={{ ...chipBaseStyle, color: theme.colors.textDim }}
          >
            <StatusDot lifecycle={{ kind: "running" }} waitingForInput={false} />
            running
          </span>
        ) : item.kind === "interrupted" ? (
          // Honest terminal marker (not a live "running" dot) for a lone `started` on a session
          // that is no longer live — the OSC-133 `finished` mark for it will never arrive, so this
          // is rendered as a distinct, exited-styled outcome rather than a claim the command is
          // still in flight. Accessible label carries the "прервано" (interrupted) semantics.
          <span
            key={item.key}
            role="listitem"
            aria-label="прервано"
            data-testid="command-chip-interrupted"
            title="Прервано — сессия завершилась до конца команды"
            style={{ ...chipBaseStyle, color: theme.colors.statusExited }}
          >
            <StatusDot
              lifecycle={{ kind: "exited", code: null, signal: null }}
              waitingForInput={false}
            />
            прервано
          </span>
        ) : (
          <span
            key={item.key}
            role="listitem"
            data-testid={`command-chip-${item.ok ? "ok" : "fail"}`}
            title={item.ok ? "exit 0" : `exit ${item.exitCode ?? "?"}`}
            style={{
              ...chipBaseStyle,
              color: item.ok ? theme.colors.statusRunning : theme.colors.statusExited,
            }}
          >
            {item.ok ? "✓" : `✗ ${item.exitCode ?? "?"}`}
          </span>
        ),
      )}
    </div>
  );
}
