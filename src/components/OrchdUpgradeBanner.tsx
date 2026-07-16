import type { CSSProperties, JSX } from "react";
import { useAppStore } from "../store/store";
import { theme } from "../theme";
import { strings } from "../strings";

const bannerStyle: CSSProperties = {
  padding: "6px 12px",
  borderLeft: `3px solid ${theme.colors.statusWaiting}`,
  background: theme.colors.bgElevated,
  color: theme.colors.text,
  fontSize: 13,
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
  gap: 12,
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
 * Re-entry banner for a CANCELLED orchestrator upgrade (BL-96, spec D8), mirroring the sessiond
 * `DaemonBanner`'s incompatible branch for the second daemon.
 *
 * `orchdIncompatible` is FATAL and never auto-clears (the orchd client's connection task exited and
 * will not reconnect on its own) — the mandatory `UpgradeDialog` opens on the event. If the owner
 * CANCELS that dialog (`orchdUpgradeDialogOpen` back to `false`) they would otherwise be stranded
 * with a permanently-disconnected orchestrator and no way back to the upgrade flow (the dead-end
 * BL-96 reports). This banner fills that gap: it shows exactly while `orchdIncompatible &&
 * !orchdUpgradeDialogOpen` and its "Update" button re-opens the dialog. While the dialog is open it
 * renders nothing (the dialog itself is the surface); once orchd upgrades, `app.restart()` resets
 * every flag.
 *
 * Amber (`statusWaiting`) left-edge accent — this IS a "needs you" action, matching `DaemonBanner`'s
 * incompatible branch (design-system.md §2), unlike the red connectivity/degradation banners.
 */
export function OrchdUpgradeBanner(): JSX.Element | null {
  const incompatible = useAppStore((s) => s.orchdIncompatible);
  const dialogOpen = useAppStore((s) => s.orchdUpgradeDialogOpen);
  const setOrchdUpgradeDialogOpen = useAppStore((s) => s.setOrchdUpgradeDialogOpen);

  if (!incompatible || dialogOpen) return null;

  return (
    <div role="alert" data-testid="orchd-upgrade-banner" style={bannerStyle}>
      <span>{strings.chrome.orchdOutdated}</span>
      <button
        type="button"
        data-testid="orchd-upgrade-reopen"
        onClick={() => setOrchdUpgradeDialogOpen(true)}
        style={buttonStyle}
      >
        {strings.common.update}
      </button>
    </div>
  );
}
