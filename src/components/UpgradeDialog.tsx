import { useEffect, useRef, type JSX } from "react";
import { useAppStore } from "../store/store";
import { upgradeDaemon } from "../ipc/commands";
import { theme } from "../theme";

/**
 * Consent dialog for the daemon upgrade (Pv2 §6.2-6.3, design-system "Dialog / modal overlay"
 * atom). Self-gated on the TWO store flags (see `store.ts` doc comment for the honesty
 * invariant): only visible when `daemonIncompatible && upgradeDialogOpen`. Cancel closes the
 * dialog (`upgradeDialogOpen=false`) but never touches `daemonIncompatible` — the daemon really
 * is incompatible until the app restarts, and the banner must keep saying so.
 *
 * `upgradeDaemon()` is invoked fire-and-forget with respect to the SUCCESS path: the daemon
 * kickstart ends in `app.restart()`, which kills this webview process, so the returned promise
 * never resolves when it works — this component must never `await`-block on it. But a REJECTION
 * (finding [13]: e.g. `CommandError::UpgradeFailed` from a TCC/MDM-denied `launchctl kickstart`)
 * is the one honest failure this flow can surface (spec §6.2.4) — it is caught via `.catch` and
 * stored in `upgradeError` so the dialog can render it, stay open, and let the user retry.
 */
export function UpgradeDialog(): JSX.Element | null {
  const daemonIncompatible = useAppStore((s) => s.daemonIncompatible);
  const upgradeDialogOpen = useAppStore((s) => s.upgradeDialogOpen);
  const sessions = useAppStore((s) => s.sessions);
  const hydrated = useAppStore((s) => s.hydrated);
  const upgradeError = useAppStore((s) => s.upgradeError);
  const setUpgradeDialogOpen = useAppStore((s) => s.setUpgradeDialogOpen);
  const setUpgradeError = useAppStore((s) => s.setUpgradeError);

  const open = daemonIncompatible && upgradeDialogOpen;
  const primaryRef = useRef<HTMLButtonElement>(null);

  /**
   * Retry-safe click handler (finding [13]): attach `.catch` — never `await` — so a rejection
   * surfaces as `upgradeError` instead of vanishing, while a successful kickstart's
   * never-resolving promise never blocks anything here. Clears any previous error up front so a
   * retry doesn't briefly show a stale message before the new attempt settles.
   */
  const handleUpgradeClick = (): void => {
    setUpgradeError(null);
    upgradeDaemon().catch((err: unknown) => {
      const reason =
        err && typeof err === "object" && "reason" in err && typeof err.reason === "string"
          ? err.reason
          : String(err);
      setUpgradeError(reason);
    });
  };

  useEffect(() => {
    if (!open) return;
    primaryRef.current?.focus();

    const onKeyDown = (e: KeyboardEvent): void => {
      if (e.key === "Escape") setUpgradeDialogOpen(false);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [open, setUpgradeDialogOpen]);

  if (!open) return null;

  // Finding [14]: `sessions` can only be populated by a successful hydrate. In the DOMINANT
  // boot-incompatible scenario the client slot is `None`, so hydrate can never succeed and
  // `sessions` stays `{}` forever — reporting "0 живых сессий" there would materially understate
  // the destruction (the OLD daemon may hold N live shells this store has never seen). Only claim
  // a count once `hydrated` proves the store reflects a real snapshot.
  const n = Object.values(sessions).filter((s) => s.isActive).length;
  const copy = hydrated
    ? `Обновить фоновый сервис — ${n} живых сессий завершатся. Их записи и scrollback сохранены и появятся снова как неактивные.`
    : "Обновить фоновый сервис — все его живые сессии завершатся. Их записи и scrollback сохранены и появятся снова как неактивные.";

  return (
    <div
      style={{
        position: "fixed",
        inset: 0,
        background: "rgba(1, 4, 9, 0.6)",
        display: "flex",
        alignItems: "center",
        justifyContent: "center",
        zIndex: 1000,
      }}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-labelledby="upgrade-dialog-title"
        style={{
          width: 360,
          background: theme.colors.bgElevated,
          border: `1px solid ${theme.colors.border}`,
          borderTop: `2px solid ${theme.colors.statusWaiting}`,
          borderRadius: 10,
          boxShadow: theme.shadow,
          padding: 16,
          display: "flex",
          flexDirection: "column",
          gap: 12,
        }}
      >
        <div
          id="upgrade-dialog-title"
          style={{
            fontSize: 15,
            fontWeight: 600,
            color: theme.colors.statusWaiting,
          }}
        >
          Требуется обновление
        </div>
        <div style={{ fontSize: 13, lineHeight: 1.5, color: theme.colors.text }}>{copy}</div>
        {upgradeError !== null && (
          // Error state (finding [13], spec §6.2.4 "honest failure"): red statusExited accent —
          // never amber (amber is reserved for "a human is needed", not for a failure that
          // already happened). Reason + an actionable hint so the user knows what to check.
          <div
            role="alert"
            style={{
              fontSize: 13,
              lineHeight: 1.5,
              color: theme.colors.statusExited,
              borderLeft: `3px solid ${theme.colors.statusExited}`,
              paddingLeft: 8,
            }}
          >
            {`Не удалось перезапустить фоновый сервис: ${upgradeError}. Проверьте разрешения (launchctl) и повторите.`}
          </div>
        )}
        <div style={{ display: "flex", justifyContent: "flex-end", gap: 8, marginTop: 4 }}>
          <button
            type="button"
            onClick={() => setUpgradeDialogOpen(false)}
            style={{
              padding: "6px 12px",
              borderRadius: 6,
              border: `1px solid ${theme.colors.border}`,
              background: "transparent",
              color: theme.colors.text,
              fontSize: 13,
              cursor: "pointer",
            }}
          >
            Отмена
          </button>
          <button
            ref={primaryRef}
            type="button"
            onClick={handleUpgradeClick}
            style={{
              padding: "6px 12px",
              borderRadius: 6,
              border: "none",
              background: theme.colors.accent,
              color: theme.colors.text,
              fontSize: 13,
              fontWeight: 600,
              cursor: "pointer",
            }}
          >
            Обновить
          </button>
        </div>
      </div>
    </div>
  );
}
