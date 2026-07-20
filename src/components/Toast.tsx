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
 * `statusExited` (red) is the DEFAULT left-edge accent, not opt-in: this atom exists to surface
 * failures honestly, so a caller wanting a neutral/success notice is the exception, not the
 * common case (no such variant exists yet — add one only when a real caller needs it, per the
 * design system's "build once, reuse everywhere" rule).
 */
export function Toast(): JSX.Element | null {
  const toast = useAppStore((s) => s.toast);
  const dismissToast = useAppStore((s) => s.dismissToast);
  if (toast === null) return null;

  return (
    <div
      role="alert"
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
        borderLeft: "3px solid var(--danger)",
        background: "var(--panel)",
        color: "var(--ink)",
        fontSize: "var(--fs-md)",
        lineHeight: 1.5,
        boxShadow: "var(--shadow-1)",
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
