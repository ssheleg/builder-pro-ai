import { invoke } from "@tauri-apps/api/core";

/**
 * Wire types for the `src-tauri/src/fs_explorer.rs` command surface (spec §4.2). These are
 * **core-local** (`#[derive(Serialize)]` only, NOT `#[derive(TS)]`/ts-rs, and NOT part of the
 * daemon protocol) — so, unlike everything in `./types.ts`, they are hand-mirrored here rather
 * than generated. Field shapes are locked 1:1 against the Rust structs/enums (confirmed by
 * reading `fs_explorer.rs` directly, including its `#[cfg(test)]` wire-shape assertions):
 *
 * - `FsEntry`: `#[serde(rename_all = "camelCase")]` struct — every field renames.
 * - `FilePreview`/`FsError`: `#[serde(tag = "kind", rename_all = "camelCase")]` — the container's
 *   `rename_all` does NOT cascade into a struct-variant's OWN fields (e.g. `Io { message }`,
 *   `Text { content, truncated, size }` keep their already-lowercase field names verbatim; only
 *   the `kind` tag values themselves get camelCased, e.g. `PermissionDenied` -> `"permissionDenied"`).
 */

/** One entry in a `list_dir` listing. `relPath` is forward-slash, relative to `root` (never to the
 * listed directory) — mirrors `fs_explorer::FsEntry`. `size` is `0` for directories. */
export interface FsEntry {
  name: string;
  relPath: string;
  isDir: boolean;
  size: number;
  isIgnored: boolean;
}

/** The result of `read_file_preview` — mirrors `fs_explorer::FilePreview`. Distinguishing
 * `binary`/`tooLarge` from `text` at the type level means the frontend can never accidentally
 * render a binary blob, or a partial read, as if it were the whole real file (spec §7). */
export type FilePreview =
  | { kind: "text"; content: string; truncated: boolean; size: number }
  | { kind: "binary"; size: number }
  | { kind: "tooLarge"; size: number };

/** Error surfaced by every command in this module — mirrors `fs_explorer::FsError`. Every
 * path-validator failure (escape, missing target, unresolvable symlink) collapses to
 * `outsideRoot`; `notFound`/`permissionDenied` are reserved for genuine POST-validation
 * filesystem outcomes. */
export type FsError =
  | { kind: "notFound" }
  | { kind: "permissionDenied" }
  | { kind: "outsideRoot" }
  | { kind: "tooLarge" }
  | { kind: "io"; message: string };

/**
 * Typed `invoke()` wrappers for the `fs_explorer.rs` / `fs_watcher.rs` `#[tauri::command]` surface
 * (spec §4.2/§5). Every fs_explorer command is PURE-LOCAL (no daemon round-trip, spec §2 D4) — a
 * rejected promise here carries an [`FsError`], never a `CommandError`. `start_workspace_watch`/
 * `stop_workspace_watch` never reject in practice (every failure mode surfaces as an honest
 * `fs://watch-error` event instead, spec §5) and resolve `void`.
 *
 * Arg object keys match the Rust parameter names verbatim (Tauri maps JS camelCase -> Rust
 * snake_case automatically), mirroring `./commands.ts`'s wrapper style.
 */

export function listDir(
  root: string,
  rel: string,
  includeIgnored: boolean,
): Promise<FsEntry[]> {
  return invoke<FsEntry[]>("list_dir", { root, rel, includeIgnored });
}

export function readFilePreview(root: string, rel: string): Promise<FilePreview> {
  return invoke<FilePreview>("read_file_preview", { root, rel });
}

export function createFile(root: string, relDir: string, name: string): Promise<void> {
  return invoke<void>("create_file", { root, relDir, name });
}

export function createDir(root: string, relDir: string, name: string): Promise<void> {
  return invoke<void>("create_dir", { root, relDir, name });
}

export function renameEntry(root: string, rel: string, newName: string): Promise<void> {
  return invoke<void>("rename_entry", { root, rel, newName });
}

export function moveEntry(root: string, relFrom: string, relDirTo: string): Promise<void> {
  return invoke<void>("move_entry", { root, relFrom, relDirTo });
}

export function deleteEntry(root: string, rel: string): Promise<void> {
  return invoke<void>("delete_entry", { root, rel });
}

export function revealInFinder(root: string, rel: string): Promise<void> {
  return invoke<void>("reveal_in_finder", { root, rel });
}

export function openExternal(root: string, rel: string): Promise<void> {
  return invoke<void>("open_external", { root, rel });
}

/**
 * Start (or replace — spec §5: "ONE active watch set at a time... starting again replaces the
 * previous") the live FSEvents watch over `roots`. Fire-and-forget from the caller's perspective:
 * every subsequent signal (changes, errors) arrives via the `fs://changed`/`fs://watch-error`
 * events (see `./events.ts`), never via this promise's resolution.
 */
export function startWorkspaceWatch(roots: string[], showIgnored: boolean): Promise<void> {
  return invoke<void>("start_workspace_watch", { roots, showIgnored });
}

/** Stop the active watch, if any (spec §5: "stop on switch/unmount"). A harmless no-op when
 * nothing is currently being watched. */
export function stopWorkspaceWatch(): Promise<void> {
  return invoke<void>("stop_workspace_watch");
}
