import type { JSX } from "react";
import { useAppStore } from "../store/store";
import { theme } from "../theme";
import { strings } from "../strings";

/**
 * Two independent truths, two branches (Pv2 §6.2-6.3 — see `store.ts`'s doc comment for the
 * full honesty invariant):
 * - `daemonIncompatible` (FATAL, never auto-clears until app restart): an honest
 *   "outdated / update required" message + an inline «Update» action that re-opens
 *   `UpgradeDialog` (`setUpgradeDialogOpen(true)`) — the inbox-item pattern (amber left-edge +
 *   text + inline action).
 * - else `!daemonConnected` (the existing plain-disconnect case, which DOES auto-reconnect):
 *   the pre-existing "reconnecting…" copy.
 * - else (connected, compatible): render nothing.
 *
 * `daemonIncompatible` is checked FIRST and wins even if `daemonConnected` also happens to be
 * `false` (it always is once the event fires — the client's connection task exited) — never show
 * "reconnecting…" once we know reconnecting won't happen.
 */
export function DaemonBanner(): JSX.Element | null {
  const incompatible = useAppStore((s) => s.daemonIncompatible);
  const connected = useAppStore((s) => s.daemonConnected);
  const setUpgradeDialogOpen = useAppStore((s) => s.setUpgradeDialogOpen);

  if (incompatible) {
    return (
      <div
        role="alert"
        style={{
          padding: "6px 12px",
          borderLeft: `3px solid ${theme.colors.statusWaiting}`,
          background: theme.colors.bgElevated,
          color: theme.colors.text,
          fontSize: 13,
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          gap: 12,
        }}
      >
        <span>{strings.chrome.daemonOutdated}</span>
        <button
          type="button"
          onClick={() => setUpgradeDialogOpen(true)}
          style={{
            padding: "2px 10px",
            borderRadius: 6,
            border: `1px solid ${theme.colors.border}`,
            background: "transparent",
            color: theme.colors.text,
            fontSize: 12,
            cursor: "pointer",
          }}
        >
          {strings.common.update}
        </button>
      </div>
    );
  }

  if (connected) return null;

  return (
    <div
      role="alert"
      style={{
        padding: "6px 12px",
        background: theme.colors.statusExited,
        color: theme.colors.text,
        fontSize: 13,
        textAlign: "center",
      }}
    >
      Daemon disconnected — reconnecting…
    </div>
  );
}
