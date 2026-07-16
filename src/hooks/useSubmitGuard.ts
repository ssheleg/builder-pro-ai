import { useCallback, useRef, useState } from "react";

/**
 * Return shape of {@link useSubmitGuard}. `submitting` drives `disabled` on the guarded control;
 * `guard` wraps a mutating handler so it can't be double-fired.
 */
export interface SubmitGuard {
  /** `true` from the moment a guarded handler starts until its `await` settles (ok OR error). */
  submitting: boolean;
  /**
   * Wrap an async submit handler. The returned handler:
   *  - **no-ops** while a previous invocation is still in flight (the synchronous re-entry lock is a
   *    `ref`, NOT the `submitting` state — React batches the `setState`, so two clicks in the same
   *    tick would both see the stale `false` and both fire; the ref is updated synchronously and so
   *    blocks the second call, which is the whole point, spec D6 / P-19); and
   *  - **toggles `submitting`** `true` before the `await` and back to `false` in a `finally` (so a
   *    rejecting handler still releases the lock and re-enables the control — mirrors
   *    `ConnectDialog`'s `busy` try/finally).
   *
   * The wrapped `fn` keeps its own error handling (every domain handler already try/catch →
   * `showToast`); the guard deliberately does NOT swallow — it re-throws via `finally`-only so a
   * caller that awaits the returned promise still observes the rejection.
   */
  guard: <A extends unknown[]>(
    fn: (...args: A) => Promise<void> | void,
  ) => (...args: A) => Promise<void>;
}

/**
 * Double-submit guard for every mutating submit in the webview (spec D6, BL-95 / P-19). A rapid
 * double click/Enter on a create/connect/run control fires the handler twice before React re-renders
 * the `disabled` state, producing duplicate rows / duplicate external calls / duplicate spend (UX
 * findings E-08, F-08, G-08, H-01, J-03..05). This hook is the single shared fix: `guard(handler)`
 * returns a wrapped handler that runs at most one invocation at a time, and `submitting` lets the
 * control render `disabled={… || submitting}` for the visible affordance.
 *
 * The lock is a `useRef` (synchronous) so the second same-tick call is blocked immediately; the
 * `submitting` `useState` exists only to drive the UI and re-render the control disabled.
 */
export function useSubmitGuard(): SubmitGuard {
  const [submitting, setSubmitting] = useState(false);
  const inFlight = useRef(false);

  const guard = useCallback(
    <A extends unknown[]>(fn: (...args: A) => Promise<void> | void) =>
      async (...args: A): Promise<void> => {
        if (inFlight.current) return; // a call is already in flight — drop this one (double-fire)
        inFlight.current = true;
        setSubmitting(true);
        try {
          await fn(...args);
        } finally {
          inFlight.current = false;
          setSubmitting(false);
        }
      },
    [],
  );

  return { submitting, guard };
}
