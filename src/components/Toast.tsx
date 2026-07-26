import type { JSX } from "react";
import { useAppStore } from "../store/store";
import { strings } from "../strings";

/**
 * Toast atom (design-system.md, spec §7 "honest error surface" — the error-surfacing contract: a
 * toast with the mapped human message, never console-only). A pure reader of the store's `toast`
 * (the visible head of the FIFO `toastQueue`) — it never owns the auto-advance timer itself (that
 * lives in `showToast`/`dismissToast`, see `store.ts`), so the queue still advances on schedule
 * even if `<Toast/>` were to unmount/remount mid-flight.
 *
 * The manual close button (BL-97, spec D8) calls `dismissToast`, which advances the queue to the
 * next pending toast (or clears it when empty) — so a burst of failures can be read and dismissed
 * one at a time rather than clobbering each other.
 *
 * `statusExited` (red) is the DEFAULT left-edge accent — this atom exists to surface failures
 * honestly, so `"error"` is the common case. FE-6 adds the opt-in `"success"` tone (`var(--ok)`
 * accent) for positive confirmations (saved/created/copied), driven by the store's `toastTone`
 * (kept in lockstep with the visible head of the queue, see `store.ts::showToast`).
 */
export function Toast(): JSX.Element | null {
  const toast = useAppStore((s) => s.toast);
  const toastTone = useAppStore((s) => s.toastTone);
  const dismissToast = useAppStore((s) => s.dismissToast);
  if (toast === null) return null;

  const accent = toastTone === "success" ? "var(--ok)" : "var(--danger)";

  return (
    <div
      role="alert"
      data-tone={toastTone}
      style={{
        position: "fixed",
        left: "50%",
        bottom: "var(--sp-5)",
        transform: "translateX(-50%)",
        maxWidth: 480,
        display: "flex",
        alignItems: "center",
        gap: "var(--sp-3)",
        padding: "var(--sp-3) var(--sp-4)",
        borderRadius: "var(--r-md)",
        background: "var(--panel)",
        color: "var(--ink)",
        fontSize: "var(--fs-md)",
        lineHeight: 1.5,
        // The tone edge is an inset shadow, not a border-left: a 3px border under a 14px radius
        // renders as a curved wedge, while an inset shadow follows the corner cleanly.
        boxShadow: `inset 3px 0 0 ${accent}, var(--shadow-1)`,
        zIndex: 1100,
      }}
    >
      <span style={{ flex: 1, minWidth: 0 }}>{toast}</span>
      <button
        type="button"
        data-testid="toast-dismiss"
        aria-label={strings.common.close}
        onClick={() => dismissToast()}
        style={{
          flexShrink: 0,
          border: "none",
          background: "transparent",
          color: "var(--muted)",
          cursor: "pointer",
          fontSize: "var(--fs-lg)",
          lineHeight: 1,
          padding: 0,
        }}
      >
        ×
      </button>
    </div>
  );
}
