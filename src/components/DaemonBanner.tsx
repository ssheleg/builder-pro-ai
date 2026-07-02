import type { JSX } from "react";
import { useAppStore } from "../store/store";
import { theme } from "../theme";

/**
 * Shown on `daemon://disconnected` (store `daemonConnected=false`), hidden on
 * `daemon://reconnected` (store flips back true). Spec §13: never fake a
 * "connected" state; tell the user honestly the session service is unreachable.
 */
export function DaemonBanner(): JSX.Element | null {
  const connected = useAppStore((s) => s.daemonConnected);
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
