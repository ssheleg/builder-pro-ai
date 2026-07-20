// src/ui/ThemeToggle.tsx — S-UXR B1. A compact control that cycles the app theme
// system → light → dark → system, driven by the store's theme/setTheme. Styled from design tokens
// (var(--…)), so it renders correctly in both palettes.
import { useAppStore } from "../store/store";
import type { Theme } from "./theme";
import { strings } from "../strings";

const ORDER: Theme[] = ["system", "light", "dark"];
const ICON: Record<Theme, string> = { system: "◐", light: "☀", dark: "☾" };

export function ThemeToggle() {
  const theme = useAppStore((s) => s.theme);
  const setTheme = useAppStore((s) => s.setTheme);
  const label = strings.chrome.theme[theme];

  const next = () => {
    const i = ORDER.indexOf(theme);
    setTheme(ORDER[(i + 1) % ORDER.length]);
  };

  return (
    <button
      type="button"
      data-testid="theme-toggle"
      aria-label={strings.chrome.theme.toggleAria(label)}
      title={strings.chrome.theme.toggleAria(label)}
      onClick={next}
      style={{
        display: "flex",
        alignItems: "center",
        gap: "var(--sp-2)",
        width: "calc(100% - 2 * var(--sp-2))",
        margin: "var(--sp-2)",
        marginTop: 0,
        padding: "var(--sp-1) var(--sp-3)",
        border: "none",
        borderRadius: "var(--r-sm)",
        background: "var(--panel-2)",
        color: "var(--muted)",
        cursor: "pointer",
        fontSize: "var(--fs-sm)",
        fontFamily: "var(--font-ui)",
      }}
    >
      <span aria-hidden="true" style={{ fontSize: "var(--fs-md)" }}>
        {ICON[theme]}
      </span>
      <span>{label}</span>
    </button>
  );
}
