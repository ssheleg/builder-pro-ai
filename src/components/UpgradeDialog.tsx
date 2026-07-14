import { useEffect, useRef, useState, type JSX } from "react";
import { useAppStore } from "../store/store";
import { upgradeDaemon } from "../ipc/commands";
import { orchdUpgrade } from "../ipc/orchd";
import { theme } from "../theme";

/** Extracts `CommandError::UpgradeFailed`'s `reason` field from a rejected upgrade promise (S3
 * T19: shared between `upgradeDaemon()`'s and `orchdUpgrade()`'s identical rejection shape —
 * `orchd_upgrade_core` "mirrors `upgrade_daemon_core` verbatim", spec §9/§10 — so the extraction
 * logic itself never needed to differ per daemon). Falls back to `String(err)` for anything else,
 * same honesty-over-guessing default `describeOrchdError` uses. */
function extractUpgradeFailureReason(err: unknown): string {
  return err && typeof err === "object" && "reason" in err && typeof err.reason === "string"
    ? err.reason
    : String(err);
}

/**
 * Consent dialog for a daemon upgrade — GENERALIZED (S3 T19, spec §10/§11) to cover BOTH
 * daemons this app talks to. Self-gated on store flags (see `store.ts`'s doc comments for the
 * honesty invariant each flag pair shares): renders exactly ONE dialog at a time.
 *
 * - `daemonIncompatible && upgradeDialogOpen` ⇒ the ORIGINAL sessiond dialog, byte-for-byte
 *   unchanged (copy, `upgradeDaemon()` call, live-session count, `upgradeError` store field).
 * - else `orchdIncompatible && orchdUpgradeDialogOpen` ⇒ the NEW orchd variant: locked copy
 *   «Обновить фоновый сервис оркестратора — записи (проекты, цели, задачи) сохранены» (no
 *   live-session warning — orchd has no PTYs, so there is nothing analogous to count), confirm
 *   calls `orchdUpgrade()`.
 * - If BOTH are incompatible at once, SESSIOND TAKES PRECEDENCE (spec §11: "sequential dialogs,
 *   sessiond first — no combined flow"): the `sessiondOpen` branch is checked FIRST and, when
 *   true, unconditionally wins — after its kickstart + `app.restart()`, the orchd incompatibility
 *   re-detects on the fresh relaunch and shows its own dialog then, never both at once.
 *
 * Cancel on either variant closes ONLY that variant's own `*UpgradeDialogOpen` flag — neither
 * ever touches `daemonIncompatible`/`orchdIncompatible` (the daemon really is incompatible until
 * the app restarts, and the corresponding banner must keep saying so).
 *
 * Each variant's "Обновить"/confirm is fire-and-forget with respect to the SUCCESS path (the
 * kickstart ends in `app.restart()`, which kills this webview process, so the returned promise
 * never resolves when it works) but attaches a `.catch` (finding [13], mirrored for orchd): a
 * REJECTED promise is the one honest failure either flow can surface. The sessiond variant stores
 * its error in the shared `upgradeError` store field (unchanged); the orchd variant uses its OWN
 * local `orchdUpgradeError` state — there is no dedicated store field for it (T19 does not modify
 * `store.ts`), which is fine since only one dialog is ever visible at a time and this component is
 * the sole reader of either.
 */
export function UpgradeDialog(): JSX.Element | null {
  const daemonIncompatible = useAppStore((s) => s.daemonIncompatible);
  const upgradeDialogOpen = useAppStore((s) => s.upgradeDialogOpen);
  const sessions = useAppStore((s) => s.sessions);
  const hydrated = useAppStore((s) => s.hydrated);
  const upgradeError = useAppStore((s) => s.upgradeError);
  const setUpgradeDialogOpen = useAppStore((s) => s.setUpgradeDialogOpen);
  const setUpgradeError = useAppStore((s) => s.setUpgradeError);

  const orchdIncompatible = useAppStore((s) => s.orchdIncompatible);
  const orchdUpgradeDialogOpen = useAppStore((s) => s.orchdUpgradeDialogOpen);
  const setOrchdUpgradeDialogOpen = useAppStore((s) => s.setOrchdUpgradeDialogOpen);
  const [orchdUpgradeError, setOrchdUpgradeError] = useState<string | null>(null);

  // Precedence (spec §11): sessiond's own gate is checked FIRST and wins outright when true —
  // the orchd branch is only even eligible once the sessiond dialog isn't showing.
  const sessiondOpen = daemonIncompatible && upgradeDialogOpen;
  const orchdOpen = !sessiondOpen && orchdIncompatible && orchdUpgradeDialogOpen;
  const open = sessiondOpen || orchdOpen;

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
      setUpgradeError(extractUpgradeFailureReason(err));
    });
  };

  /** Same retry-safe shape as `handleUpgradeClick`, targeting `orchdUpgrade()` + the local
   * `orchdUpgradeError` state instead of the store's sessiond-specific field. */
  const handleOrchdUpgradeClick = (): void => {
    setOrchdUpgradeError(null);
    orchdUpgrade().catch((err: unknown) => {
      setOrchdUpgradeError(extractUpgradeFailureReason(err));
    });
  };

  useEffect(() => {
    if (!open) return;
    primaryRef.current?.focus();

    const onKeyDown = (e: KeyboardEvent): void => {
      if (e.key !== "Escape") return;
      if (sessiondOpen) setUpgradeDialogOpen(false);
      else setOrchdUpgradeDialogOpen(false);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [open, sessiondOpen, setUpgradeDialogOpen, setOrchdUpgradeDialogOpen]);

  if (!open) return null;

  if (sessiondOpen) {
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

  // orchd variant (S3 T19, spec §10): same dialog-atom shell as the sessiond branch above, but
  // orchd's locked copy has NO live-session warning — orchd has no PTYs, so there is no
  // "N живых сессий" to count.
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
        aria-labelledby="orchd-upgrade-dialog-title"
        data-testid="orchd-upgrade-dialog"
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
          id="orchd-upgrade-dialog-title"
          style={{
            fontSize: 15,
            fontWeight: 600,
            color: theme.colors.statusWaiting,
          }}
        >
          Требуется обновление
        </div>
        <div style={{ fontSize: 13, lineHeight: 1.5, color: theme.colors.text }}>
          Обновить фоновый сервис оркестратора — записи (проекты, цели, задачи) сохранены
        </div>
        {orchdUpgradeError !== null && (
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
            {`Не удалось перезапустить фоновый сервис оркестратора: ${orchdUpgradeError}. Проверьте разрешения (launchctl) и повторите.`}
          </div>
        )}
        <div style={{ display: "flex", justifyContent: "flex-end", gap: 8, marginTop: 4 }}>
          <button
            type="button"
            onClick={() => setOrchdUpgradeDialogOpen(false)}
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
            onClick={handleOrchdUpgradeClick}
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
