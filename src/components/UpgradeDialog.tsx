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
 * `upgradeDaemon()` is called fire-and-forget: the daemon kickstart ends in `app.restart()`,
 * which kills this webview process, so the returned promise never resolves on the happy path.
 */
export function UpgradeDialog(): JSX.Element | null {
  const daemonIncompatible = useAppStore((s) => s.daemonIncompatible);
  const upgradeDialogOpen = useAppStore((s) => s.upgradeDialogOpen);
  const sessions = useAppStore((s) => s.sessions);
  const setUpgradeDialogOpen = useAppStore((s) => s.setUpgradeDialogOpen);

  const open = daemonIncompatible && upgradeDialogOpen;
  const primaryRef = useRef<HTMLButtonElement>(null);

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

  const n = Object.values(sessions).filter((s) => s.isActive).length;
  const copy = `Обновить фоновый сервис — ${n} живых сессий завершатся. Их записи и scrollback сохранены и появятся снова как неактивные.`;

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
            onClick={() => void upgradeDaemon()}
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
