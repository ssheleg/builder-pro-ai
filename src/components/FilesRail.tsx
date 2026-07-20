import type { JSX } from "react";
import { useAppStore } from "../store/store";
import { startWorkspaceWatch } from "../ipc/fs";
import type { Workspace } from "../ipc/types";
import { FileTree } from "./FileTree";
import { FilePreview } from "./FilePreview";
import { strings } from "../strings";

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
          borderLeft: "1px solid var(--hairline)",
          background: "var(--panel)",
          display: "flex",
          alignItems: "flex-start",
          justifyContent: "center",
          paddingTop: "var(--sp-2)",
        }}
      >
        <button
          type="button"
          aria-label={strings.files.openPanel}
          title={watchPaused ? strings.files.liveUpdatesPaused : undefined}
          onClick={() => setFilesRailOpen(true)}
          style={{
            display: "flex",
            flexDirection: "column",
            alignItems: "center",
            gap: "var(--sp-1)",
            border: "none",
            background: "transparent",
            color: "var(--muted)",
            cursor: "pointer",
            fontSize: "var(--fs-md)",
          }}
        >
          <span>⟨</span>
          {watchPaused && (
            // Degradation cue survives the collapsed rail (AUD-2026-07-19-08): without it a
            // dead watcher was invisible until the owner happened to reopen the panel.
            <span
              data-testid="files-rail-collapsed-paused"
              aria-label={strings.files.liveUpdatesPaused}
              style={{
                width: 8,
                height: 8,
                borderRadius: "50%",
                background: "var(--warn)",
              }}
            />
          )}
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
    // Optimistically clear the paused flag, but re-set it if the restart itself rejects (C2) so
    // the tree never falsely reads "live" — and the rejection never escapes as unhandled.
    void startWorkspaceWatch(roots, showIgnored).catch(() => setWatchPaused(true));
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
        borderLeft: "1px solid var(--hairline)",
        background: "var(--panel)",
        color: "var(--ink)",
      }}
    >
      <div
        style={{
          display: "flex",
          alignItems: "center",
          gap: "var(--sp-2)",
          padding: "var(--sp-2) var(--sp-2)",
          borderBottom: "1px solid var(--hairline)",
        }}
      >
        <button
          type="button"
          aria-label={strings.files.collapsePanel}
          onClick={() => setFilesRailOpen(false)}
          style={{
            border: "none",
            background: "transparent",
            color: "var(--muted)",
            cursor: "pointer",
            fontSize: "var(--fs-md)",
          }}
        >
          ⟩
        </button>
        <span
          style={{
            fontSize: "var(--fs-sm)",
            textTransform: "uppercase",
            letterSpacing: 0.5,
            color: "var(--muted)",
            flex: 1,
          }}
        >
          {strings.files.title}
        </span>
        <label
          style={{
            display: "flex",
            alignItems: "center",
            gap: "var(--sp-1)",
            fontSize: "var(--fs-xs)",
            color: "var(--muted)",
            cursor: "pointer",
          }}
        >
          <input type="checkbox" checked={showIgnored} onChange={onToggleShowIgnored} />
          {strings.files.showIgnored}
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
            padding: "var(--sp-2)",
            border: "none",
            borderBottom: "1px solid var(--hairline)",
            borderLeft: "3px solid var(--warn)",
            background: "var(--warn-weak)",
            color: "var(--ink)",
            fontSize: "var(--fs-sm)",
            cursor: "pointer",
          }}
        >
          {strings.files.liveUpdatesPaused}
        </button>
      )}
      <div
        style={{
          flex: "1 1 60%",
          minHeight: 0,
          borderBottom: "1px solid var(--hairline)",
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
