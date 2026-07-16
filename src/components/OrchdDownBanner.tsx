import type { CSSProperties, JSX } from "react";
import { orchdReconnect } from "../ipc/orchd";
import { theme } from "../theme";
import { strings } from "../strings";

const bannerStyle: CSSProperties = {
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
  gap: 12,
  padding: "6px 12px",
  borderLeft: `3px solid ${theme.colors.statusExited}`,
  background: theme.colors.bgElevated,
  color: theme.colors.text,
  fontSize: 13,
};

const buttonStyle: CSSProperties = {
  padding: "2px 10px",
  borderRadius: 6,
  border: `1px solid ${theme.colors.border}`,
  background: "transparent",
  color: theme.colors.text,
  fontSize: 12,
  cursor: "pointer",
};

/**
 * Shared «Orchestrator unavailable» banner (spec §10/§11 honest-degradation matrix: "orchd socket
 * down / connect refused ⇒ banner + retry; terminals unaffected"). A `statusExited` (red)
 * left-edge accent — NOT amber, which is reserved exclusively for "needs you"
 * (design-system.md §2) — this is a connectivity failure, not a request for the owner's
 * attention.
 *
 * Purely presentational: it does NOT read `orchdDown` itself. Every consumer (App.tsx today; any
 * future domain surface per the spec's "every domain surface" language) is responsible for
 * mounting it conditionally on the store's `orchdDown` flag — this component's only job is the
 * copy + the retry action, so it can be unit-tested in isolation without touching the store.
 *
 * [Retry] fires `orchdReconnect()` fire-and-forget: the call's own doc comment says its
 * outcome is observed via the `orchd://down`/`orchd://up` events (wired in App.tsx), never via
 * this promise resolving/rejecting, so there is nothing useful to `await` or `.catch` here.
 */
export function OrchdDownBanner(): JSX.Element {
  return (
    <div role="alert" data-testid="orchd-down-banner" style={bannerStyle}>
      <span>{strings.chrome.orchdUnavailable}</span>
      <button
        type="button"
        data-testid="orchd-down-retry"
        onClick={() => void orchdReconnect()}
        style={buttonStyle}
      >
        {strings.common.retry}
      </button>
    </div>
  );
}
