import type { JSX } from "react";
import { useAppStore } from "../store/store";
import { startWorkspaceWatch } from "../ipc/fs";
import type { Workspace } from "../ipc/types";
import { FileTree } from "./FileTree";
import { FilePreview } from "./FilePreview";
import { theme } from "../theme";

/**
 * Right, collapsible files rail (spec §6.4): `FileTree` (lazy, virtualized) stacked over
 * `FilePreview` (read-only). A DOCKED panel, not an overlay — separated by `border`/`bgElevated`,
 * never `theme.shadow` (design-system §4 "elevation = border, not shadow... shadows only for
 * true overlays"; the floating context-menu/inline-form popovers INSIDE `FileTree` are the true
 * overlays and do use it).
 *
 * Renders nothing while no workspace is active (`FileTree` needs `workspace.roots` to have
 * anything to show — App only ever passes a real `Workspace` once one is selected). While
 * `!filesRailOpen` it still renders a narrow reopen strip rather than disappearing outright — the
 * owner needs SOME way back in, since nothing else in scope (T10) toggles `filesRailOpen` from
 * outside `FileTree`'s own file-click (which only ever OPENS it).
 */
export function FilesRail(props: { workspace: Workspace | undefined }): JSX.Element | null {
  const { workspace } = props;
  const open = useAppStore((s) => s.filesRailOpen);
  const showIgnored = useAppStore((s) => s.showIgnored);
  const watchPaused = useAppStore((s) => s.watchPaused);
  const setFilesRailOpen = useAppStore((s) => s.setFilesRailOpen);
  const toggleShowIgnored = useAppStore((s) => s.toggleShowIgnored);
  const setWatchPaused = useAppStore((s) => s.setWatchPaused);
  const invalidateDirs = useAppStore((s) => s.invalidateDirs);

  if (!workspace) return null;

  if (!open) {
    return (
      <div
        style={{
          width: 28,
          flexShrink: 0,
          borderLeft: `1px solid ${theme.colors.border}`,
          background: theme.colors.bgElevated,
          display: "flex",
          alignItems: "flex-start",
          justifyContent: "center",
          paddingTop: 8,
        }}
      >
        <button
          type="button"
          aria-label="Открыть панель файлов"
          onClick={() => setFilesRailOpen(true)}
          style={{
            border: "none",
            background: "transparent",
            color: theme.colors.textDim,
            cursor: "pointer",
            fontSize: 14,
          }}
        >
          ⟨
        </button>
      </div>
    );
  }

  // Captured as a plain `string[]` (not re-read off `workspace` inside the closures below): TS's
  // control-flow narrowing of the `if (!workspace) return null` guard above does not extend into
  // nested function declarations, so `workspace` itself would still type as possibly-`undefined`
  // there even though it provably isn't at this point in render.
  const roots = workspace.roots;

  // Toggling what's included changes the wire request (`listDir(..., includeIgnored)`); any
  // cached listing fetched under the OLD flag would silently misrepresent the new one, so every
  // cached dir for this workspace is dropped — a re-expand (or the still-expanded root, via
  // FileTree's own auto-refetch effect) pulls a fresh, honest listing.
  function onToggleShowIgnored(): void {
    toggleShowIgnored();
    for (const root of roots) {
      invalidateDirs(root, ["*"]);
    }
  }

  // "Refresh" (spec §7 "Watcher dies -> ... manual refresh; auto-retry on re-activation"):
  // restart the live watch (fire-and-forget — a renewed failure re-fires `fs://watch-error`,
  // a later task's concern) and drop every cached dir so the visible tree re-pulls honestly
  // rather than keep showing whatever was last seen before the watch died.
  function onRefreshWatch(): void {
    void startWorkspaceWatch(roots, showIgnored);
    for (const root of roots) {
      invalidateDirs(root, ["*"]);
    }
    setWatchPaused(false);
  }

  return (
    <aside
      aria-label="Files"
      style={{
        width: 320,
        flexShrink: 0,
        display: "flex",
        flexDirection: "column",
        minHeight: 0,
        borderLeft: `1px solid ${theme.colors.border}`,
        background: theme.colors.bgElevated,
        color: theme.colors.text,
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: 8,
          padding: "6px 8px",
          borderBottom: `1px solid ${theme.colors.border}`,
        }}
      >
        <button
          type="button"
          aria-label="Свернуть панель файлов"
          onClick={() => setFilesRailOpen(false)}
          style={{
            border: "none",
            background: "transparent",
            color: theme.colors.textDim,
            cursor: "pointer",
            fontSize: 14,
          }}
        >
          ⟩
        </button>
        <span
          style={{
            fontSize: 12,
            textTransform: "uppercase",
            letterSpacing: 0.5,
            color: theme.colors.textDim,
            flex: 1,
          }}
        >
          Файлы
        </span>
        <label
          style={{
            display: "flex",
            alignItems: "center",
            gap: 4,
            fontSize: 11,
            color: theme.colors.textDim,
            cursor: "pointer",
          }}
        >
          <input type="checkbox" checked={showIgnored} onChange={onToggleShowIgnored} />
          показывать игнорируемые
        </label>
      </div>
      {watchPaused && (
        <button
          type="button"
          onClick={onRefreshWatch}
          style={{
            display: "block",
            width: "100%",
            textAlign: "left",
            padding: "6px 8px",
            border: "none",
            borderBottom: `1px solid ${theme.colors.border}`,
            borderLeft: `3px solid ${theme.colors.statusWaiting}`,
            background: "transparent",
            color: theme.colors.text,
            fontSize: 12,
            cursor: "pointer",
          }}
        >
          live-обновления на паузе — обновить
        </button>
      )}
      <div
        style={{
          flex: "1 1 60%",
          minHeight: 0,
          borderBottom: `1px solid ${theme.colors.border}`,
          overflow: "hidden",
        }}
      >
        <FileTree workspace={workspace} />
      </div>
      <div style={{ flex: "1 1 40%", minHeight: 0, overflow: "hidden" }}>
        <FilePreview />
      </div>
    </aside>
  );
}
