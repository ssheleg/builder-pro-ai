// src/theme/theme.ts — S-UXR theme control (spec B1). Resolves + applies the light/dark theme and
// maps entity statuses to semantic tones. Persistence is localStorage (synchronous, no IPC — read
// on boot before React renders, so there is no flash of the wrong theme); the spec's tauri-store
// option was dropped in favour of this simpler, FOUC-free path.

export type Theme = "light" | "dark" | "system";
export type Tone = "ink" | "muted" | "accent" | "info" | "ok" | "warn" | "danger";

const STORAGE_KEY = "bpa-theme";

/** The resolved (concrete) appearance, after collapsing "system" via the OS preference. */
export type ResolvedTheme = "light" | "dark";

function prefersDark(): boolean {
  return (
    typeof window !== "undefined" &&
    typeof window.matchMedia === "function" &&
    window.matchMedia("(prefers-color-scheme: dark)").matches
  );
}

/** Collapse a Theme to the concrete light/dark it renders as right now. */
export function resolveTheme(theme: Theme): ResolvedTheme {
  if (theme === "system") return prefersDark() ? "dark" : "light";
  return theme;
}

/** Read the persisted preference; defaults to "system". */
export function readTheme(): Theme {
  try {
    const v = localStorage.getItem(STORAGE_KEY);
    if (v === "light" || v === "dark" || v === "system") return v;
  } catch {
    // localStorage can throw in a locked-down webview; fall through to the default.
  }
  return "system";
}

/** Stamp the resolved theme onto the document root so tokens.css picks the right palette. */
export function applyTheme(theme: Theme): void {
  const resolved = resolveTheme(theme);
  const root = document.documentElement;
  if (resolved === "dark") root.setAttribute("data-theme", "dark");
  else root.removeAttribute("data-theme"); // light is the :root default
}

/** Persist + apply. Used by the store's setTheme. */
export function setThemePref(theme: Theme): void {
  try {
    localStorage.setItem(STORAGE_KEY, theme);
  } catch {
    // best-effort persistence; applying still works this session.
  }
  applyTheme(theme);
}

let systemListenerBound = false;

/**
 * Boot-time init: apply the stored preference immediately, and — while the preference is "system" —
 * keep the applied theme in sync with OS appearance changes. Call once from main.tsx BEFORE render.
 * Returns the preference that was applied.
 */
export function initTheme(): Theme {
  const pref = readTheme();
  applyTheme(pref);
  if (
    !systemListenerBound &&
    typeof window !== "undefined" &&
    typeof window.matchMedia === "function"
  ) {
    systemListenerBound = true;
    window
      .matchMedia("(prefers-color-scheme: dark)")
      .addEventListener("change", () => {
        // Only "system" follows the OS; an explicit light/dark pref is left untouched.
        if (readTheme() === "system") applyTheme("system");
      });
  }
  return pref;
}

/** Map an entity status string to a semantic tone (spec B1 status→tone table). */
export function statusTone(status: string): Tone {
  switch (status) {
    case "running":
      return "info";
    case "waiting":
      return "warn";
    case "done":
    case "accepted":
    case "shipped":
      return "ok";
    case "failed":
    case "interrupted":
      return "danger";
    default:
      // pending / archived / new / captured / … — a calm neutral.
      return "muted";
  }
}
