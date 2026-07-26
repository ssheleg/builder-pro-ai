import { useEffect, useMemo, useRef, useState, type CSSProperties, type JSX } from "react";
import { useAppStore } from "../store/store";
import {
  listDir,
  createFile,
  createDir,
  renameEntry,
  deleteEntry,
  revealInFinder,
  openExternal,
} from "../ipc/fs";
import type { FsEntry, FsError } from "../ipc/fs";
import { pickFolder, addWorkspaceRoot } from "../ipc/commands";
import type { Workspace } from "../ipc/types";
import { Button, Input } from "../ui/primitives";
import { strings } from "../strings";

/** Fixed row height the windowing math is built on (spec §6.4 "plain scroll-offset windowing,
 * no new dependency"). A real DOM height, not measured — every row (loading, dir, file, failed)
 * renders at exactly this height so the spacer math below stays exact. */
const ROW_HEIGHT = 22;
/** Deterministic fallback viewport (px) used until/unless a real `ResizeObserver` measurement is
 * available (jsdom has none — tests get this exact, reproducible window). */
const DEFAULT_VIEWPORT_HEIGHT = 480;
/** Extra rows rendered above/below the visible window so a fast scroll never flashes blank. */
const OVERSCAN = 8;

/** Matches `store.ts`'s `fsKey` exactly (`expanded`/`treeCache` are keyed this way) — not
 * exported from the store, so mirrored here (tab-separated: `rel` may contain `/`). */
function fsKey(root: string, rel: string): string {
  return `${root}\t${rel}`;
}

function basename(path: string): string {
  const parts = path.replace(/\/+$/, "").split("/");
  return parts[parts.length - 1] || path;
}

function dirnameOf(rel: string): string {
  const idx = rel.lastIndexOf("/");
  return idx === -1 ? "" : rel.slice(0, idx);
}

/** Honest message for a rejected `FsError` (spec §7 — never console-only). `FilePreview` keeps
 * its own copy (see that file's doc comment) rather than importing this one, so the two
 * components stay independently deployable — the vocabulary is deliberately identical. */
export function describeFsError(err: unknown): string {
  const e = err as Partial<FsError> | undefined;
  switch (e?.kind) {
    case "notFound":
      return strings.errors.fs.notFound;
    case "permissionDenied":
      return strings.errors.fs.noAccess;
    case "outsideRoot":
      return strings.errors.fs.outsideRoot;
    case "tooLarge":
      return strings.errors.fs.tooLarge;
    case "alreadyExists":
      return strings.errors.fs.alreadyExists;
    case "io":
      return e.message ?? strings.errors.fs.io;
    default:
      return err instanceof Error ? err.message : String(err);
  }
}

/** Honest message for a rejected `CommandError` (`addWorkspaceRoot` is a daemon round-trip, spec
 * §3.3 — a different error union than `FsError`, see `src-tauri/src/commands.rs::CommandError`). */
function describeCommandError(err: unknown): string {
  const e = err as { kind?: string; message?: string; code?: string; reason?: string } | undefined;
  switch (e?.kind) {
    case "daemon":
      return e.message ?? e.code ?? strings.errors.command.daemon;
    case "disconnected":
      return strings.errors.command.disconnected;
    case "internal":
      return e.message ?? strings.errors.command.internal;
    case "incompatibleDaemon":
      return strings.errors.command.incompatible;
    case "upgradeFailed":
      return e.reason ?? strings.errors.command.failed;
    case "tooLarge":
      return strings.errors.command.tooLarge;
    default:
      return err instanceof Error ? err.message : String(err);
  }
}

interface FlatNode {
  /** React list key — distinct from `targetKey` for the synthetic "loading" placeholder so it
   * never collides with its own (uncached) directory's row. */
  id: string;
  root: string;
  rel: string;
  name: string;
  isDir: boolean;
  isIgnored: boolean;
  depth: number;
  size: number;
  loading?: boolean;
  /** Synthetic marker for an expanded directory that loaded successfully but has zero visible
   * entries (P-12) — rendered non-interactively, distinct from the `loading` placeholder. */
  empty?: boolean;
  /** Synthetic marker for an expanded directory whose `listDir` FAILED (FI-02) — rendered as a
   * DISTINCT failed row with an inline Retry, never left indistinguishable from a still-loading
   * one. The dir is NOT re-added to `pending` (no silent auto-retry loop); Retry clears the
   * failed flag, which re-enters it into `pending` for a fresh fetch. */
  failed?: boolean;
}

function sortEntries(entries: FsEntry[]): FsEntry[] {
  return [...entries].sort((a, b) => {
    if (a.isDir !== b.isDir) return a.isDir ? -1 : 1;
    return a.name.localeCompare(b.name);
  });
}

/**
 * Flatten the visible (expanded) tree into a linear array, windowed by the caller (spec §6.4).
 * A directory that is expanded but not yet cached contributes a synthetic "loading" row instead
 * of being silently indistinguishable from an empty directory (design-system §1 "honest state,
 * always") and its `(root, rel)` is collected into `pending` for the caller to fetch.
 */
function computeFlatten(
  roots: string[],
  expanded: Record<string, true>,
  treeCache: Record<string, FsEntry[]>,
  showIgnored: boolean,
  failedDirs: Record<string, string>,
): { nodes: FlatNode[]; pending: { root: string; rel: string }[] } {
  const nodes: FlatNode[] = [];
  const pending: { root: string; rel: string }[] = [];

  function visitChildren(root: string, rel: string, depth: number): void {
    const key = fsKey(root, rel);
    const cached = treeCache[key];
    if (cached === undefined) {
      // A prior `listDir` for this dir FAILED (FI-02): render a DISTINCT failed row instead of a
      // "Loading…" one, and do NOT re-add it to `pending` — an auto-retry against a stable error
      // would just re-toast forever. Retry (the row's own control) clears the failed flag, which
      // lets this dir fall back into the `pending` branch below for a genuine re-fetch.
      if (failedDirs[key] !== undefined) {
        nodes.push({
          id: `${key}::failed`,
          root,
          rel,
          name: strings.files.readFolderFailed(failedDirs[key]),
          isDir: false,
          isIgnored: false,
          depth,
          size: 0,
          failed: true,
        });
        return;
      }
      pending.push({ root, rel });
      nodes.push({
        id: `${key}::loading`,
        root,
        rel,
        name: strings.files.loading,
        isDir: false,
        isIgnored: false,
        depth,
        size: 0,
        loading: true,
      });
      return;
    }
    const visibleChildren = sortEntries(cached).filter((e) => showIgnored || !e.isIgnored);
    if (visibleChildren.length === 0) {
      // Loaded, genuinely empty (P-12): a distinct dim marker rather than rendering nothing —
      // so an empty dir never looks like a dir that failed to load or is still fetching.
      nodes.push({
        id: `${key}::empty`,
        root,
        rel,
        name: strings.files.emptyFolder,
        isDir: false,
        isIgnored: false,
        depth,
        size: 0,
        empty: true,
      });
      return;
    }
    for (const e of visibleChildren) {
      const childKey = fsKey(root, e.relPath);
      nodes.push({
        id: childKey,
        root,
        rel: e.relPath,
        name: e.name,
        isDir: e.isDir,
        isIgnored: e.isIgnored,
        depth,
        size: e.size,
      });
      if (e.isDir && expanded[childKey]) {
        visitChildren(root, e.relPath, depth + 1);
      }
    }
  }

  for (const root of roots) {
    const rootKey = fsKey(root, "");
    nodes.push({
      id: rootKey,
      root,
      rel: "",
      name: basename(root),
      isDir: true,
      isIgnored: false,
      depth: 0,
      size: 0,
    });
    if (expanded[rootKey]) {
      visitChildren(root, "", 1);
    }
  }

  return { nodes, pending };
}

type FormState =
  | { mode: "new-file"; root: string; relDir: string; anchorKey: string }
  | { mode: "new-dir"; root: string; relDir: string; anchorKey: string }
  | { mode: "rename"; root: string; rel: string; anchorKey: string };

const popoverStyle: CSSProperties = {
  position: "absolute",
  top: "100%",
  left: "var(--sp-2)",
  zIndex: 20,
  minWidth: 180,
  background: "var(--panel)",
  borderRadius: "var(--r-md)",
  boxShadow: "var(--shadow-1)",
  padding: "var(--sp-1)",
  display: "flex",
  flexDirection: "column",
  gap: 2,
};

const menuItemStyle: CSSProperties = {
  textAlign: "left",
  padding: "var(--sp-2)",
  border: "none",
  background: "transparent",
  color: "var(--ink)",
  fontSize: "var(--fs-sm)",
  cursor: "pointer",
  borderRadius: "var(--r-sm)",
};

/** Compact inline Retry for a failed dir row (FI-02) — sized to sit inside the fixed ROW_HEIGHT
 * without breaking the windowing math (so it can't be the taller `Button` primitive). */
const retryRowButtonStyle: CSSProperties = {
  border: "none",
  background: "var(--panel-2)",
  color: "var(--ink)",
  cursor: "pointer",
  fontFamily: "var(--font-mono)",
  fontSize: "var(--fs-xs)",
  borderRadius: "var(--r-sm)",
  padding: "0 var(--sp-2)",
  lineHeight: "16px",
  flexShrink: 0,
};

function rowBaseStyle(depth: number): CSSProperties {
  return {
    display: "flex",
    alignItems: "center",
    gap: "var(--sp-1)",
    height: ROW_HEIGHT,
    lineHeight: `${ROW_HEIGHT}px`,
    paddingLeft: 8 + depth * 14,
    paddingRight: 8,
    cursor: "pointer",
    fontFamily: "var(--font-mono)",
    fontSize: "var(--fs-sm)",
    position: "relative",
    whiteSpace: "nowrap",
  };
}

/**
 * Lazy, virtualized file tree (spec §6.4). Roots = `workspace.roots`' top-level nodes; expanding
 * a directory fetches `listDir` once and caches it in the store (`expanded`/`treeCache`, spec
 * §6.6) — re-expanding serves the cache, and the cache going missing while a dir stays expanded
 * (an `fs://changed` invalidation, wired by a later task) auto-refetches via the effect below,
 * with no user action required. Rows are windowed over a flattened visible-node array using plain
 * scroll-offset math (no virtualization dependency) so 10k+ entries stay smooth (DoD: <500 DOM
 * rows rendered at any time).
 */
export function FileTree(props: { workspace: Workspace }): JSX.Element {
  const { workspace } = props;

  const expanded = useAppStore((s) => s.expanded);
  const treeCache = useAppStore((s) => s.treeCache);
  const showIgnored = useAppStore((s) => s.showIgnored);
  const selectedFile = useAppStore((s) => s.selectedFile);
  const setExpanded = useAppStore((s) => s.setExpanded);
  const cacheDir = useAppStore((s) => s.cacheDir);
  const setSelectedFile = useAppStore((s) => s.setSelectedFile);
  const setFilesRailOpen = useAppStore((s) => s.setFilesRailOpen);
  const showToast = useAppStore((s) => s.showToast);

  const [scrollTop, setScrollTop] = useState(0);
  const [viewportHeight, setViewportHeight] = useState(DEFAULT_VIEWPORT_HEIGHT);
  const [menuFor, setMenuFor] = useState<string | null>(null);
  const [formFor, setFormFor] = useState<FormState | null>(null);
  const [formValue, setFormValue] = useState("");
  // Dirs whose `listDir` FAILED (FI-02), keyed by `fsKey` → the honest error message. A failed dir
  // renders a distinct failed row (not the "Loading…" placeholder) and is kept OUT of `pending`
  // until Retry clears its entry here, which re-enters it for a fresh fetch.
  const [failedDirs, setFailedDirs] = useState<Record<string, string>>({});
  // FS-8: bumped when an in-flight `listDir` response is dropped as stale (its dir was
  // invalidated mid-flight — see `treeEpochs` in the store). Added to the fetch effect's deps so
  // the still-uncached dir is re-fetched immediately; without it the invalidation would be LOST
  // (the fetchingRef guard below suppresses the effect's own re-entry while a fetch runs).
  const [fetchNonce, setFetchNonce] = useState(0);

  const scrollRef = useRef<HTMLDivElement | null>(null);
  const popoverRef = useRef<HTMLDivElement | null>(null);
  const fetchingRef = useRef<Set<string>>(new Set());

  const { nodes, pending } = useMemo(
    () => computeFlatten(workspace.roots, expanded, treeCache, showIgnored, failedDirs),
    [workspace.roots, expanded, treeCache, showIgnored, failedDirs],
  );

  // Fetch every expanded-but-uncached directory. Re-runs only when `pending` (derived from
  // `expanded`/`treeCache`) actually changes — a cache entry disappearing out from under a still
  // -expanded dir (invalidation) re-adds it to `pending` and this effect fetches it again, with
  // no explicit re-expand click needed (test (b)).
  useEffect(() => {
    for (const { root, rel } of pending) {
      const key = fsKey(root, rel);
      if (fetchingRef.current.has(key)) continue;
      fetchingRef.current.add(key);
      // FS-8: capture the dir's invalidation epoch BEFORE the fetch; an `fs://changed`
      // invalidation landing mid-flight bumps it (`invalidateDirs`), and the response below —
      // captured before the on-disk change — must be dropped + re-fetched, not cached as truth.
      const epochAtStart = useAppStore.getState().treeEpochs[key] ?? 0;
      listDir(root, rel, showIgnored)
        .then((entries) => {
          if ((useAppStore.getState().treeEpochs[key] ?? 0) !== epochAtStart) {
            // Stale: invalidated while in flight. Drop the response and re-enter the fetch loop
            // (the dir is still uncached, so the nonce re-run picks it up again). The key is
            // released HERE, not only in `finally`: React may run the nonce-triggered re-render
            // between the `.then` and `.finally` microtasks, and the effect must already see
            // the dir as fetchable (Set.delete is idempotent, so `finally` stays harmless).
            fetchingRef.current.delete(key);
            setFetchNonce((n) => n + 1);
            return;
          }
          cacheDir(root, rel, entries);
          // A retry (or a first success) clears any prior failed marker so a later invalidation
          // re-fetches honestly rather than surfacing the stale failed row again.
          setFailedDirs((prev) => {
            if (!(key in prev)) return prev; // no-op → React bails out, no extra render
            const next = { ...prev };
            delete next[key];
            return next;
          });
        })
        .catch((err: unknown) => {
          // FI-02: record the failure so this dir renders a distinct, retryable failed row (and is
          // NOT re-added to `pending`), instead of an indistinguishable, never-retried "Loading…".
          const msg = describeFsError(err);
          setFailedDirs((prev) => ({ ...prev, [key]: msg }));
          showToast(strings.files.readFolderFailed(msg));
        })
        .finally(() => {
          fetchingRef.current.delete(key);
        });
    }
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [pending, showIgnored, fetchNonce]);

  // Real-browser responsiveness: measure the scroll container once ResizeObserver is available
  // (jsdom has none, so tests deterministically stay at DEFAULT_VIEWPORT_HEIGHT).
  useEffect(() => {
    const el = scrollRef.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver((entries) => {
      const h = entries[0]?.contentRect.height;
      if (h && h > 0) setViewportHeight(h);
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  // Close any open popover on an outside click; the popover itself owns stopPropagation on its
  // own interactions so this never fires for a click INSIDE it.
  useEffect(() => {
    if (menuFor === null && formFor === null) return;
    function onDocMouseDown(e: MouseEvent): void {
      if (popoverRef.current && popoverRef.current.contains(e.target as Node)) return;
      setMenuFor(null);
      setFormFor(null);
    }
    document.addEventListener("mousedown", onDocMouseDown);
    return () => document.removeEventListener("mousedown", onDocMouseDown);
  }, [menuFor, formFor]);

  async function refreshDir(root: string, rel: string): Promise<void> {
    try {
      const entries = await listDir(root, rel, showIgnored);
      cacheDir(root, rel, entries);
    } catch (err) {
      showToast(strings.files.refreshFolderFailed(describeFsError(err)));
    }
  }

  function toggleDir(node: FlatNode): void {
    const key = fsKey(node.root, node.rel);
    setExpanded(node.root, node.rel, !expanded[key]);
  }

  // FI-02: clear the dir's failed marker so `computeFlatten` re-adds it to `pending` and the fetch
  // effect runs a genuine re-fetch (mirrors CommandStrip's failed→Retry recovery path).
  function retryDir(root: string, rel: string): void {
    const key = fsKey(root, rel);
    setFailedDirs((prev) => {
      if (!(key in prev)) return prev;
      const next = { ...prev };
      delete next[key];
      return next;
    });
  }

  function selectFile(node: FlatNode): void {
    setSelectedFile({ root: node.root, rel: node.rel });
    setFilesRailOpen(true);
  }

  async function doCreate(root: string, relDir: string, name: string, kind: "file" | "dir"): Promise<void> {
    try {
      if (kind === "file") await createFile(root, relDir, name);
      else await createDir(root, relDir, name);
      await refreshDir(root, relDir);
    } catch (err) {
      showToast(
        strings.files.createFailed(
          kind === "file" ? strings.files.fileWord : strings.files.folderWord,
          describeFsError(err),
        ),
      );
    }
  }

  async function doRename(root: string, rel: string, newName: string): Promise<void> {
    try {
      await renameEntry(root, rel, newName);
      await refreshDir(root, dirnameOf(rel));
      const sel = useAppStore.getState().selectedFile;
      if (sel && sel.root === root && sel.rel === rel) {
        const parent = dirnameOf(rel);
        setSelectedFile({ root, rel: parent === "" ? newName : `${parent}/${newName}` });
      }
    } catch (err) {
      showToast(strings.files.renameFailed(describeFsError(err)));
    }
  }

  async function doDelete(root: string, rel: string, isDir: boolean): Promise<void> {
    const label = isDir ? strings.files.folderWord : strings.files.fileWord;
    if (!window.confirm(strings.files.deleteConfirm(label, rel))) return;
    try {
      await deleteEntry(root, rel);
      await refreshDir(root, dirnameOf(rel));
      const sel = useAppStore.getState().selectedFile;
      if (sel && sel.root === root && sel.rel === rel) {
        setSelectedFile(null);
      }
    } catch (err) {
      showToast(strings.files.deleteFailed(describeFsError(err)));
    }
  }

  async function doReveal(root: string, rel: string): Promise<void> {
    try {
      await revealInFinder(root, rel);
    } catch (err) {
      showToast(strings.files.revealFailed(describeFsError(err)));
    }
  }

  async function doOpenExternal(root: string, rel: string): Promise<void> {
    try {
      await openExternal(root, rel);
    } catch (err) {
      showToast(strings.files.openExternalFailed(describeFsError(err)));
    }
  }

  async function onAddRoot(): Promise<void> {
    try {
      const dir = await pickFolder();
      if (dir === null) return;
      const ws = await addWorkspaceRoot(workspace.id, dir);
      useAppStore.getState().upsertWorkspace(ws);
    } catch (err) {
      showToast(strings.files.addRootFailed(describeCommandError(err)));
    }
  }

  function openNewFileForm(node: FlatNode): void {
    const anchorKey = fsKey(node.root, node.rel);
    setMenuFor(null);
    setFormValue("");
    setFormFor({ mode: "new-file", root: node.root, relDir: node.rel, anchorKey });
  }

  function openNewDirForm(node: FlatNode): void {
    const anchorKey = fsKey(node.root, node.rel);
    setMenuFor(null);
    setFormValue("");
    setFormFor({ mode: "new-dir", root: node.root, relDir: node.rel, anchorKey });
  }

  function openRenameForm(node: FlatNode): void {
    const anchorKey = fsKey(node.root, node.rel);
    setMenuFor(null);
    setFormValue(node.name);
    setFormFor({ mode: "rename", root: node.root, rel: node.rel, anchorKey });
  }

  async function submitForm(): Promise<void> {
    if (!formFor) return;
    const value = formValue.trim();
    const current = formFor;
    setFormFor(null);
    if (value === "") return; // blank name -> silent cancel, never a malformed create/rename
    if (current.mode === "new-file") await doCreate(current.root, current.relDir, value, "file");
    else if (current.mode === "new-dir") await doCreate(current.root, current.relDir, value, "dir");
    else await doRename(current.root, current.rel, value);
  }

  function renderMenu(node: FlatNode): JSX.Element {
    const isRoot = node.rel === "";
    return (
      <div
        ref={popoverRef}
        role="menu"
        aria-label={strings.files.menuAria(node.name)}
        style={popoverStyle}
        onMouseDown={(e) => e.stopPropagation()}
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.stopPropagation()}
      >
        {node.isDir && (
          <button
            type="button"
            role="menuitem"
            style={menuItemStyle}
            onClick={(e) => {
              e.stopPropagation();
              openNewFileForm(node);
            }}
          >
            {strings.files.newFile}
          </button>
        )}
        {node.isDir && (
          <button
            type="button"
            role="menuitem"
            style={menuItemStyle}
            onClick={(e) => {
              e.stopPropagation();
              openNewDirForm(node);
            }}
          >
            {strings.files.newFolder}
          </button>
        )}
        {!isRoot && (
          <button
            type="button"
            role="menuitem"
            style={menuItemStyle}
            onClick={(e) => {
              e.stopPropagation();
              openRenameForm(node);
            }}
          >
            {strings.files.rename}
          </button>
        )}
        {!isRoot && (
          <button
            type="button"
            role="menuitem"
            style={menuItemStyle}
            onClick={(e) => {
              e.stopPropagation();
              setMenuFor(null);
              void doDelete(node.root, node.rel, node.isDir);
            }}
          >
            {strings.common.delete}
          </button>
        )}
        <button
          type="button"
          role="menuitem"
          style={menuItemStyle}
          onClick={(e) => {
            e.stopPropagation();
            setMenuFor(null);
            void doReveal(node.root, node.rel);
          }}
        >
          {strings.files.reveal}
        </button>
        <button
          type="button"
          role="menuitem"
          style={menuItemStyle}
          onClick={(e) => {
            e.stopPropagation();
            setMenuFor(null);
            void doOpenExternal(node.root, node.rel);
          }}
        >
          {strings.files.openExternal}
        </button>
      </div>
    );
  }

  function renderForm(): JSX.Element | null {
    if (!formFor) return null;
    const label =
      formFor.mode === "new-file"
        ? strings.files.newFileNamePlaceholder
        : formFor.mode === "new-dir"
          ? strings.files.newFolderNamePlaceholder
          : strings.files.newNamePlaceholder;
    return (
      <div
        ref={popoverRef}
        style={popoverStyle}
        onMouseDown={(e) => e.stopPropagation()}
        onClick={(e) => e.stopPropagation()}
        onKeyDown={(e) => e.stopPropagation()}
      >
        <Input
          autoFocus
          aria-label={label}
          value={formValue}
          onChange={(e) => setFormValue(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              void submitForm();
            } else if (e.key === "Escape") {
              e.preventDefault();
              setFormFor(null);
            }
          }}
          style={{
            fontSize: "var(--fs-sm)",
            padding: "var(--sp-1) var(--sp-2)",
            fontFamily: "var(--font-mono)",
          }}
        />
      </div>
    );
  }

  function renderRow(node: FlatNode): JSX.Element {
    if (node.failed) {
      // FI-02: a directory whose listing FAILED — a distinct, honest row (never mistaken for a
      // still-loading one) carrying the error message + an inline Retry that re-fetches.
      return (
        <div
          key={node.id}
          data-testid="file-row-failed"
          style={{
            ...rowBaseStyle(node.depth),
            color: "var(--danger)",
            cursor: "default",
          }}
        >
          <span aria-hidden style={{ width: 12, flexShrink: 0 }} />
          <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", flex: 1 }}>
            {node.name}
          </span>
          <button
            type="button"
            data-testid="file-row-retry"
            onClick={(e) => {
              e.stopPropagation();
              retryDir(node.root, node.rel);
            }}
            style={retryRowButtonStyle}
          >
            {strings.common.retry}
          </button>
        </div>
      );
    }

    if (node.loading || node.empty) {
      return (
        <div
          key={node.id}
          data-testid={node.empty ? "file-row-empty" : "file-row"}
          style={{
            ...rowBaseStyle(node.depth),
            color: "var(--muted)",
            cursor: "default",
            fontStyle: node.empty ? "italic" : undefined,
          }}
        >
          <span aria-hidden style={{ width: 12, flexShrink: 0 }} />
          <span>{node.name}</span>
        </div>
      );
    }

    const targetKey = fsKey(node.root, node.rel);
    const isExpanded = !!expanded[targetKey];
    const selected =
      !!selectedFile && selectedFile.root === node.root && selectedFile.rel === node.rel;

    return (
      <div
        key={node.id}
        data-testid="file-row"
        role="treeitem"
        tabIndex={0}
        aria-selected={selected}
        aria-expanded={node.isDir ? isExpanded : undefined}
        onClick={() => (node.isDir ? toggleDir(node) : selectFile(node))}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === " ") {
            e.preventDefault();
            node.isDir ? toggleDir(node) : selectFile(node);
          }
        }}
        onContextMenu={(e) => {
          e.preventDefault();
          setFormFor(null);
          setMenuFor((cur) => (cur === targetKey ? null : targetKey));
        }}
        style={{
          ...rowBaseStyle(node.depth),
          color: node.isIgnored ? "var(--muted)" : "var(--ink)",
          background: selected ? "var(--accent-weak)" : "transparent",
        }}
      >
        <span
          aria-hidden
          style={{ width: 12, textAlign: "center", color: "var(--muted)", flexShrink: 0 }}
        >
          {node.isDir ? (isExpanded ? "▾" : "▸") : ""}
        </span>
        <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", flex: 1 }}>
          {node.name}
        </span>
        <button
          type="button"
          aria-label={strings.files.actionsAria(node.name)}
          onClick={(e) => {
            e.stopPropagation();
            setFormFor(null);
            setMenuFor((cur) => (cur === targetKey ? null : targetKey));
          }}
          style={{
            border: "none",
            background: "transparent",
            color: "var(--muted)",
            cursor: "pointer",
            fontSize: "var(--fs-md)",
            lineHeight: 1,
            padding: "0 var(--sp-1)",
            flexShrink: 0,
          }}
        >
          ⋯
        </button>
        {menuFor === targetKey && renderMenu(node)}
        {formFor?.anchorKey === targetKey && renderForm()}
      </div>
    );
  }

  const total = nodes.length;
  const startIndex = Math.max(0, Math.floor(scrollTop / ROW_HEIGHT) - OVERSCAN);
  const visibleCount = Math.ceil(viewportHeight / ROW_HEIGHT) + OVERSCAN * 2;
  const endIndex = Math.min(total, startIndex + visibleCount);
  const visible = nodes.slice(startIndex, endIndex);

  return (
    <div style={{ display: "flex", flexDirection: "column", height: "100%", minHeight: 0 }}>
      <div
        ref={scrollRef}
        role="tree"
        aria-label="Files"
        data-testid="file-tree-scroll"
        onScroll={(e) => setScrollTop(e.currentTarget.scrollTop)}
        style={{ flex: 1, minHeight: 0, overflowY: "auto" }}
      >
        <div style={{ height: startIndex * ROW_HEIGHT }} />
        {visible.map((n) => renderRow(n))}
        <div style={{ height: Math.max(0, (total - endIndex) * ROW_HEIGHT) }} />
      </div>
      <Button
        type="button"
        variant="ghost"
        size="sm"
        aria-label="Add root"
        onClick={() => void onAddRoot()}
        style={{ margin: "var(--sp-2)", flexShrink: 0 }}
      >
        + Add root
      </Button>
    </div>
  );
}
