import type { CSSProperties, JSX } from "react";
import { useAppStore } from "../store/store";
import { strings } from "../strings";

const bannerStyle: CSSProperties = {
  display: "flex",
  alignItems: "center",
  justifyContent: "center",
  gap: "var(--sp-3)",
  padding: "var(--sp-2) var(--sp-3)",
  borderLeft: "3px solid var(--danger)",
  background: "var(--danger-weak)",
  color: "var(--danger)",
  fontSize: "var(--fs-md)",
};

/**
 * Persistent honest storage-degradation banner (spec D3, BL-94). Reads the store's `storageStatus`
 * (pulled on connect + every `orchd://up`, `store.ts`) and surfaces the two non-`persistent` modes
 * so the owner is never silently working against a database that will not survive a restart:
 * - `in_memory_fallback` → "Storage unavailable — running in memory. Changes will NOT survive a
 *   restart." (the disk was unavailable at boot; every write is ephemeral).
 * - `recovered_from_corruption` → "Database was corrupted and has been reset. The damaged copy was
 *   saved to <path>." (the on-disk image was quarantined aside and a fresh DB opened).
 *
 * `persistent` (or a not-yet-fetched `null`) renders NOTHING — the healthy path shows no chrome.
 * A `statusExited` (red) left-edge accent, matching `OrchdDownBanner` — this is a reliability
 * degradation, not a "needs you" attention request (amber is reserved for that, design-system.md
 * §2). The mode is fixed at boot, so there is no dismiss: it stays until the daemon restarts into a
 * healthy state.
 */
export function StorageBanner(): JSX.Element | null {
  const storageStatus = useAppStore((s) => s.storageStatus);
  if (storageStatus === null || storageStatus.storageMode === "persistent") return null;

  const message =
    storageStatus.storageMode === "recovered_from_corruption"
      ? strings.storage.recovered(storageStatus.quarantinedPath ?? "")
      : strings.storage.inMemory;

  return (
    <div role="alert" data-testid="storage-banner" style={bannerStyle}>
      <span>{message}</span>
    </div>
  );
}
