import type { JSX } from "react";
import { useAppStore } from "../store/store";
import { theme } from "../theme";

/**
 * Minimal queue-of-ONE toast (design-system.md Toast atom, spec §7 "honest error surface" — the
 * error-surfacing contract: a toast with the mapped human message, never console-only). A pure
 * reader of the store's `toast` — it never owns the auto-dismiss timer itself (that lives in
 * `showToast`, see `store.ts`), so the toast still disappears on schedule even if `<Toast/>`
 * were to unmount/remount mid-flight.
 *
 * `statusExited` (red) is the DEFAULT left-edge accent, not opt-in: this atom exists to surface
 * failures honestly, so a caller wanting a neutral/success notice is the exception, not the
 * common case (no such variant exists yet — add one only when a real caller needs it, per the
 * design system's "build once, reuse everywhere" rule).
 *
 * Rendered by `App` in a later task (S2); this task ships the component, the store slice, and
 * their tests only.
 */
export function Toast(): JSX.Element | null {
  const toast = useAppStore((s) => s.toast);
  if (toast === null) return null;

  return (
    <div
      role="alert"
      style={{
        position: "fixed",
        left: "50%",
        bottom: 24,
        transform: "translateX(-50%)",
        maxWidth: 480,
        padding: "10px 16px",
        borderRadius: 8,
        border: `1px solid ${theme.colors.border}`,
        borderLeft: `3px solid ${theme.colors.statusExited}`,
        background: theme.colors.bgElevated,
        color: theme.colors.text,
        fontSize: 13,
        lineHeight: 1.5,
        boxShadow: theme.shadow,
        zIndex: 1100,
      }}
    >
      {toast}
    </div>
  );
}
