import type { JSX } from "react";
import { useAppStore } from "../store/store";
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
          padding: "var(--sp-2) var(--sp-3)",
          borderLeft: "3px solid var(--warn)",
          background: "var(--warn-weak)",
          color: "var(--warn)",
          fontSize: "var(--fs-md)",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          gap: "var(--sp-3)",
        }}
      >
        <span>{strings.chrome.daemonOutdated}</span>
        <button
          type="button"
          onClick={() => setUpgradeDialogOpen(true)}
          style={{
            padding: "var(--sp-1) var(--sp-3)",
            borderRadius: "var(--r-md)",
            border: "1px solid var(--border-strong)",
            background: "transparent",
            color: "var(--ink)",
            fontSize: "var(--fs-sm)",
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
        padding: "var(--sp-2) var(--sp-3)",
        borderLeft: "3px solid var(--danger)",
        background: "var(--danger-weak)",
        color: "var(--danger)",
        fontSize: "var(--fs-md)",
        textAlign: "center",
      }}
    >
      {strings.chrome.daemonDisconnected}
    </div>
  );
}
