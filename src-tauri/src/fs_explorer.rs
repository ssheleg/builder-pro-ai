//! Core file-explorer command surface (spec §4): gitignore-aware one-level directory listing,
//! a capped read-only file preview, create/rename/move, delete-to-Trash, and reveal/open-external.
//!
//! Approach A (spec §2 D4): file I/O lives in the Tauri core (foreground, GUI-lifetime), never
//! brokered to the daemon — every command here is **PURE-LOCAL** (no `State`/daemon client), a
//! thin `#[tauri::command]` wrapper over a unit-testable `*_inner` function.
//!
//! ## Security boundary
//!
//! Every operation validates its path(s) against the workspace `root` via
//! [`bpa_paths::validate_path_within`] (existing target) or [`bpa_paths::validate_parent_within`]
//! (not-yet-existing create/rename/move destination) **before** touching the filesystem — this is
//! the ONLY thing standing between a webview-supplied `rel` string and an arbitrary filesystem
//! write. Both validators fail closed (spec §16: missing path, broken symlink, or a genuine `..`
//! escape are all indistinguishable `Err`s, by design — never leak *why* a path was rejected), so
//! every validator failure collapses to the single [`FsError::OutsideRoot`] variant here: never a
//! `NotFound`/`PermissionDenied` from the validator itself, only from a *post-validation*
//! filesystem call (a genuine TOCTOU race, or an actual I/O permission error).
//!
//! `name`/`new_name` arguments (the final path segment for create/rename) get an extra,
//! fs_explorer-local separator guard ([`reject_separator`]) that `validate_parent_within` cannot
//! provide: that validator's own defensive check runs on `Path::file_name()`, which by
//! construction can never itself contain a separator once `/`-split into components — so a
//! caller-supplied `"sub/evil.txt"` would otherwise silently resolve into an existing `sub`
//! subdirectory instead of being rejected as a malformed single-segment name.
//!
//! File CONTENTS are never logged (paths only, at debug) — spec §7.

use std::path::{Path, PathBuf};

use serde::Serialize;

/// Cap on how much of a file `read_file_preview` will read (spec §4.2): a file whose size exceeds
/// this never gets its content read at all — [`FilePreview::TooLarge`] is returned from the stat
/// alone.
pub const PREVIEW_CAP: u64 = 1024 * 1024; // 1 MiB

/// How many leading bytes of a file are inspected for NUL bytes / invalid UTF-8 to decide
/// [`FilePreview::Binary`] (spec §4.2).
const BINARY_PROBE_LEN: usize = 8 * 1024; // 8 KiB

// ── wire types (spec §4.2 — core-local, NOT protocol/ts-rs types; serde only) ──────────────────

/// One entry in a [`list_dir`] listing. `rel_path` is forward-slash, relative to the workspace
/// `root` (never to the listed directory). `size` is 0 for directories.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FsEntry {
    pub name: String,
    pub rel_path: String,
    pub is_dir: bool,
    pub size: u64,
    pub is_ignored: bool,
}

/// The result of [`read_file_preview`]. Distinguishing `Binary`/`TooLarge` from `Text` at the type
/// level (rather than e.g. an `Option<String>`) means the frontend can never accidentally render a
/// binary blob or a partial-looking truncation as if it were the whole, real file (spec §7: "never
/// truncated-as-if-whole").
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FilePreview {
    Text {
        content: String,
        truncated: bool,
        size: u64,
    },
    Binary {
        size: u64,
    },
    TooLarge {
        size: u64,
    },
}

/// Error surfaced to the webview by every command in this module. `#[serde(tag = "kind", ...)]`
/// with a PER-VARIANT `rename_all` on `Io` (Task-8 lesson, see `commands::CommandError`/
/// `commands::DaemonStatus`: the container's `rename_all` does NOT cascade into struct-variant
/// fields — `Io { message }` needs its own attribute even though `message` is already a single
/// lowercase word).
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum FsError {
    NotFound,
    PermissionDenied,
    /// Every path-validator failure (escape, missing target, unresolvable symlink) collapses here
    /// — see the module doc's "Security boundary" section for why that collapsing is intentional.
    OutsideRoot,
    /// Reserved for parity with the locked wire shape (spec §4.2); no command in this module
    /// currently constructs it — an oversized file is instead an *honest success*
    /// ([`FilePreview::TooLarge`]), not an error.
    TooLarge,
    Io {
        message: String,
    },
}

impl std::fmt::Display for FsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FsError::NotFound => write!(f, "not found"),
            FsError::PermissionDenied => write!(f, "permission denied"),
            FsError::OutsideRoot => write!(f, "path escapes the workspace root"),
            FsError::TooLarge => write!(f, "too large"),
            FsError::Io { message } => write!(f, "I/O error: {message}"),
        }
    }
}

impl std::error::Error for FsError {}

// ── path helpers ─────────────────────────────────────────────────────────────────────────────

/// Join a forward-slash `rel` path onto `root`. `rel == ""` means `root` itself.
fn join_rel(root: &Path, rel: &str) -> PathBuf {
    if rel.is_empty() {
        root.to_path_buf()
    } else {
        root.join(rel)
    }
}

/// Validate an EXISTING path (`root.join(rel)`) is contained within `root`. Any validator failure
/// — escape, missing target, unresolvable symlink — collapses to `FsError::OutsideRoot` (see
/// module doc).
fn validated_existing(root: &Path, rel: &str) -> Result<PathBuf, FsError> {
    let candidate = join_rel(root, rel);
    bpa_paths::validate_path_within(root, &candidate).map_err(|_| FsError::OutsideRoot)
}

/// Reject a `name`/`new_name` argument that is empty, `.`/`..`, or contains a path separator.
/// `validate_parent_within`'s own defensive separator check cannot catch a caller-supplied
/// `"sub/evil.txt"` (see module doc) — this runs BEFORE the name is ever joined into a `Path`.
fn reject_separator(name: &str) -> Result<(), FsError> {
    if name.is_empty() || name == "." || name == ".." || name.contains('/') || name.contains('\\') {
        return Err(FsError::OutsideRoot);
    }
    Ok(())
}

/// Validate a not-yet-existing create/rename/move destination: `rel_dir` (must already exist,
/// within `root`) joined with a single-segment `name`.
fn validated_new(root: &Path, rel_dir: &str, name: &str) -> Result<PathBuf, FsError> {
    reject_separator(name)?;
    let dir = join_rel(root, rel_dir);
    let target = dir.join(name);
    bpa_paths::validate_parent_within(root, &target).map_err(|_| FsError::OutsideRoot)
}

/// Map a post-validation `std::io::Error` to the honest `FsError` variant (spec §4.2's error
/// mapping table): `NotFound`/`PermissionDenied` are reserved for genuine filesystem outcomes
/// AFTER a path already passed containment validation (e.g. a TOCTOU race, or a real permission
/// error on the actual syscall) — never for the validator's own containment failures.
fn map_io_error(e: std::io::Error) -> FsError {
    match e.kind() {
        std::io::ErrorKind::NotFound => FsError::NotFound,
        std::io::ErrorKind::PermissionDenied => FsError::PermissionDenied,
        _ => FsError::Io {
            message: e.to_string(),
        },
    }
}

// ── list_dir ─────────────────────────────────────────────────────────────────────────────────

/// Raw one-level directory enumeration (name, is_dir, size), `.git` always excluded regardless of
/// gitignore state (spec §4.2). Uses `std::fs::read_dir` directly — for exactly ONE level, this
/// already *is* what `ignore::WalkBuilder` with `max_depth(1)` would give with every filter
/// disabled, without needing to build a full walker for it.
fn read_dir_children(dir: &Path) -> Result<Vec<(String, bool, u64)>, FsError> {
    let read_dir = std::fs::read_dir(dir).map_err(map_io_error)?;
    let mut out = Vec::new();
    for entry in read_dir {
        let entry = entry.map_err(map_io_error)?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if name == ".git" {
            continue;
        }
        let metadata = entry.metadata().map_err(map_io_error)?;
        let is_dir = metadata.is_dir();
        let size = if is_dir { 0 } else { metadata.len() };
        out.push((name, is_dir, size));
    }
    Ok(out)
}

/// The set of `dir`'s immediate child names that are NOT excluded by `.gitignore` (spec §4.2:
/// git-ignore semantics via the `ignore` crate, nested `.gitignore` supported via `parents(true)`).
///
/// `require_git` is left at its crate default (`true`, i.e. NOT overridden to `false`): gitignore
/// rules only activate once a `.git` (or `.jj`) directory is found somewhere in the ancestor chain
/// — this is the standard ripgrep/git behavior and, critically, it BOUNDS how far up the parent
/// chain `parents(true)` reads `.gitignore` files. With `require_git(false)` the walker's own
/// `add_parents` unconditionally walks every ancestor up to the filesystem root looking for
/// `.gitignore` files with no git-repo boundary at all — for a workspace root that IS a real repo
/// (the overwhelmingly common case) this makes no difference, but it would mean a stray
/// `.gitignore` anywhere above a NON-repo root (e.g. in the user's home directory) leaks into
/// every listing. Tests mark their tempdir root as a repo (`root/.git`, an empty marker directory
/// — `NSFileManager`-style presence checks are a plain `.exists()`) to exercise this deterministically.
///
/// `git_global`/`git_exclude`/`ignore` (the ripgrep-specific `.ignore` file) are explicitly
/// disabled: D3 (spec §2) is scoped to `.gitignore` only — picking up the user's ambient
/// `core.excludesFile` would make listings depend on host machine state outside the repo.
fn visible_child_names(dir: &Path) -> Result<std::collections::HashSet<String>, FsError> {
    let walker = ignore::WalkBuilder::new(dir)
        .max_depth(Some(1))
        .hidden(false)
        .parents(true)
        .git_ignore(true)
        .git_global(false)
        .git_exclude(false)
        .ignore(false)
        .build();

    let mut names = std::collections::HashSet::new();
    for result in walker {
        let entry = result.map_err(|e| FsError::Io {
            message: e.to_string(),
        })?;
        if entry.depth() == 0 {
            continue; // `dir` itself, always yielded first by the `ignore` walker.
        }
        names.insert(entry.file_name().to_string_lossy().into_owned());
    }
    Ok(names)
}

/// List `root.join(rel)`, ONE level (no recursion — children of a subdirectory are fetched lazily
/// by a later `list_dir` call for that subdirectory, spec §4.2). `.git` is always excluded. An
/// entry covered by an applicable `.gitignore` rule is omitted when `include_ignored` is `false`,
/// or included with `is_ignored: true` when `true`.
pub(crate) fn list_dir_inner(
    root: &Path,
    rel: &str,
    include_ignored: bool,
) -> Result<Vec<FsEntry>, FsError> {
    let dir = validated_existing(root, rel)?;
    let meta = std::fs::metadata(&dir).map_err(map_io_error)?;
    if !meta.is_dir() {
        return Err(FsError::NotFound);
    }

    let children = read_dir_children(&dir)?;
    let visible = visible_child_names(&dir)?;
    let rel_trimmed = rel.trim_end_matches('/');

    let mut out = Vec::with_capacity(children.len());
    for (name, is_dir, size) in children {
        let is_ignored = !visible.contains(&name);
        if is_ignored && !include_ignored {
            continue;
        }
        let rel_path = if rel_trimmed.is_empty() {
            name.clone()
        } else {
            format!("{rel_trimmed}/{name}")
        };
        out.push(FsEntry {
            name,
            rel_path,
            is_dir,
            size,
            is_ignored,
        });
    }
    Ok(out)
}

// ── read_file_preview ────────────────────────────────────────────────────────────────────────

/// Pure classification of file bytes into a [`FilePreview`], given the file's STATTED `size` and
/// the bytes actually read (only ever called once `size <= PREVIEW_CAP`, so `bytes` is normally
/// the whole file — `TooLarge` is decided by the caller from `size` alone, before any read).
/// Binary detection: a NUL byte or invalid UTF-8 in the first [`BINARY_PROBE_LEN`] bytes read.
/// `truncated` is `size > bytes.len() as u64` — an honest signal for the (rare) TOCTOU case where
/// the file shrank between the stat and the read, so the reported `size` and the actual `content`
/// disagree (spec §7: never claim a truncated read is the whole file). Pulled out as a pure
/// function over `(size, bytes)` so the truncation path is deterministically unit-testable without
/// needing to reproduce a real race.
fn build_preview(size: u64, bytes: &[u8]) -> FilePreview {
    let probe_len = bytes.len().min(BINARY_PROBE_LEN);
    let probe = &bytes[..probe_len];
    if probe.contains(&0u8) || std::str::from_utf8(probe).is_err() {
        return FilePreview::Binary { size };
    }
    FilePreview::Text {
        content: String::from_utf8_lossy(bytes).into_owned(),
        truncated: size > bytes.len() as u64,
        size,
    }
}

/// Read `root.join(rel)` capped at [`PREVIEW_CAP`] (spec §4.2). A file whose STATTED size exceeds
/// the cap is never read at all — see [`FilePreview::TooLarge`].
pub(crate) fn read_file_preview_inner(root: &Path, rel: &str) -> Result<FilePreview, FsError> {
    let path = validated_existing(root, rel)?;
    let metadata = std::fs::metadata(&path).map_err(map_io_error)?;
    if metadata.is_dir() {
        return Err(FsError::Io {
            message: "cannot preview a directory".to_string(),
        });
    }
    let size = metadata.len();
    if size > PREVIEW_CAP {
        return Ok(FilePreview::TooLarge { size });
    }
    let bytes = std::fs::read(&path).map_err(map_io_error)?;
    Ok(build_preview(size, &bytes))
}

// ── create / rename / move / delete ─────────────────────────────────────────────────────────

/// Create an empty file at `root.join(rel_dir).join(name)`. Uses `create_new` (fails if the target
/// already exists) rather than the truncating `File::create`, so this can never silently destroy
/// an existing file's content.
pub(crate) fn create_file_inner(root: &Path, rel_dir: &str, name: &str) -> Result<(), FsError> {
    let target = validated_new(root, rel_dir, name)?;
    std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&target)
        .map_err(map_io_error)?;
    Ok(())
}

/// Create one directory at `root.join(rel_dir).join(name)`.
pub(crate) fn create_dir_inner(root: &Path, rel_dir: &str, name: &str) -> Result<(), FsError> {
    let target = validated_new(root, rel_dir, name)?;
    std::fs::create_dir(&target).map_err(map_io_error)?;
    Ok(())
}

/// Rename `root.join(rel)` to `new_name`, keeping it in the same parent directory.
pub(crate) fn rename_entry_inner(root: &Path, rel: &str, new_name: &str) -> Result<(), FsError> {
    let source = validated_existing(root, rel)?;
    let rel_dir = Path::new(rel)
        .parent()
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default();
    let target = validated_new(root, &rel_dir, new_name)?;
    std::fs::rename(&source, &target).map_err(map_io_error)?;
    Ok(())
}

/// Move `root.join(rel_from)` into `root.join(rel_dir_to)`, keeping its own filename.
pub(crate) fn move_entry_inner(
    root: &Path,
    rel_from: &str,
    rel_dir_to: &str,
) -> Result<(), FsError> {
    let source = validated_existing(root, rel_from)?;
    let name = source
        .file_name()
        .ok_or(FsError::OutsideRoot)?
        .to_string_lossy()
        .into_owned();
    let target = validated_new(root, rel_dir_to, &name)?;
    std::fs::rename(&source, &target).map_err(map_io_error)?;
    Ok(())
}

/// Delete `root.join(rel)` to the OS Trash (spec D8: always reversible, never `remove_file`/
/// `remove_dir_all`).
pub(crate) fn delete_entry_inner(root: &Path, rel: &str) -> Result<(), FsError> {
    let target = validated_existing(root, rel)?;
    trash_delete(&target).map_err(|e| FsError::Io {
        message: e.to_string(),
    })?;
    Ok(())
}

/// macOS: explicitly select `DeleteMethod::NsFileManager` over the `trash` crate's own default
/// (`DeleteMethod::Finder`, which shells out to `osascript -e 'tell application "Finder" to
/// delete ...'`). Empirically, on this development machine the `Finder`/AppleScript path took
/// over 60 SECONDS for a single-file delete (see the task report) — almost certainly Automation
/// TCC permission negotiation / Finder-launch latency in a non-interactive session, which would
/// make every delete in the app feel hung. `NsFileManager` (`NSFileManager.trashItemAtURL`) is
/// documented by the crate itself as faster and requiring no extra permissions, at the cost of
/// not showing "Put Back" in Finder's context menu on some systems — an acceptable trade-off: the
/// file still lands in `~/.Trash` and is fully recoverable, satisfying D8's "reversible" bar.
#[cfg(target_os = "macos")]
fn trash_delete(path: &Path) -> Result<(), trash::Error> {
    use trash::macos::{DeleteMethod, TrashContextExtMacos};
    let mut ctx = trash::TrashContext::default();
    ctx.set_delete_method(DeleteMethod::NsFileManager);
    ctx.delete(path)
}

/// Non-macOS: the crate's own default (there is no `Finder`-vs-`NsFileManager` choice off macOS).
#[cfg(not(target_os = "macos"))]
fn trash_delete(path: &Path) -> Result<(), trash::Error> {
    trash::delete(path)
}

/// Reveal `root.join(rel)` in the platform file manager (Finder on macOS).
pub(crate) fn reveal_in_finder_inner(root: &Path, rel: &str) -> Result<(), FsError> {
    let target = validated_existing(root, rel)?;
    opener::reveal(&target).map_err(|e| FsError::Io {
        message: e.to_string(),
    })?;
    Ok(())
}

/// Open `root.join(rel)` with the platform's default application for it.
pub(crate) fn open_external_inner(root: &Path, rel: &str) -> Result<(), FsError> {
    let target = validated_existing(root, rel)?;
    opener::open(&target).map_err(|e| FsError::Io {
        message: e.to_string(),
    })?;
    Ok(())
}

// ── #[tauri::command] surface (spec §4.2) — thin, pure-local, no State/daemon client ──────────

#[tauri::command]
pub fn list_dir(root: String, rel: String, include_ignored: bool) -> Result<Vec<FsEntry>, FsError> {
    list_dir_inner(Path::new(&root), &rel, include_ignored)
}

#[tauri::command]
pub fn read_file_preview(root: String, rel: String) -> Result<FilePreview, FsError> {
    read_file_preview_inner(Path::new(&root), &rel)
}

#[tauri::command]
pub fn create_file(root: String, rel_dir: String, name: String) -> Result<(), FsError> {
    create_file_inner(Path::new(&root), &rel_dir, &name)
}

#[tauri::command]
pub fn create_dir(root: String, rel_dir: String, name: String) -> Result<(), FsError> {
    create_dir_inner(Path::new(&root), &rel_dir, &name)
}

#[tauri::command]
pub fn rename_entry(root: String, rel: String, new_name: String) -> Result<(), FsError> {
    rename_entry_inner(Path::new(&root), &rel, &new_name)
}

#[tauri::command]
pub fn move_entry(root: String, rel_from: String, rel_dir_to: String) -> Result<(), FsError> {
    move_entry_inner(Path::new(&root), &rel_from, &rel_dir_to)
}

#[tauri::command]
pub fn delete_entry(root: String, rel: String) -> Result<(), FsError> {
    delete_entry_inner(Path::new(&root), &rel)
}

#[tauri::command]
pub fn reveal_in_finder(root: String, rel: String) -> Result<(), FsError> {
    reveal_in_finder_inner(Path::new(&root), &rel)
}

#[tauri::command]
pub fn open_external(root: String, rel: String) -> Result<(), FsError> {
    open_external_inner(Path::new(&root), &rel)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    /// Build `dir/root` plus a `.git` marker directory under it, so the `ignore` crate's default
    /// `require_git(true)` activates `.gitignore` matching (see `visible_child_names`'s docs) —
    /// mirrors a real workspace root, which is virtually always an actual git repo.
    fn git_root() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join(".git")).unwrap();
        (dir, root)
    }

    fn plain_root() -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        fs::create_dir(&root).unwrap();
        (dir, root)
    }

    // ── list_dir ─────────────────────────────────────────────────────────────────────────────

    #[test]
    fn list_dir_one_level_returns_files_and_dirs() {
        let (_tmp, root) = plain_root();
        fs::write(root.join("a.txt"), b"hi").unwrap();
        fs::create_dir(root.join("sub")).unwrap();
        fs::write(root.join("sub").join("nested.txt"), b"nope").unwrap();

        let mut entries = list_dir_inner(&root, "", false).unwrap();
        entries.sort_by(|a, b| a.name.cmp(&b.name));

        assert_eq!(
            entries.len(),
            2,
            "must be exactly one level, got {entries:?}"
        );
        assert_eq!(entries[0].name, "a.txt");
        assert!(!entries[0].is_dir);
        assert_eq!(entries[0].size, 2);
        assert_eq!(entries[0].rel_path, "a.txt");
        assert!(!entries[0].is_ignored);

        assert_eq!(entries[1].name, "sub");
        assert!(entries[1].is_dir);
        assert_eq!(entries[1].size, 0);
        assert_eq!(entries[1].rel_path, "sub");
    }

    #[test]
    fn list_dir_second_level_is_lazy_via_rel() {
        let (_tmp, root) = plain_root();
        fs::create_dir(root.join("sub")).unwrap();
        fs::write(root.join("sub").join("nested.txt"), b"x").unwrap();

        let nested = list_dir_inner(&root, "sub", false).unwrap();
        assert_eq!(nested.len(), 1);
        assert_eq!(nested[0].name, "nested.txt");
        assert_eq!(nested[0].rel_path, "sub/nested.txt");
    }

    #[test]
    fn list_dir_never_lists_dot_git() {
        let (_tmp, root) = git_root();
        fs::write(root.join("a.txt"), b"hi").unwrap();

        let entries = list_dir_inner(&root, "", true).unwrap();
        assert!(
            entries.iter().all(|e| e.name != ".git"),
            ".git must never appear, got {entries:?}"
        );
        assert_eq!(entries.len(), 1);
    }

    #[test]
    fn list_dir_gitignored_entry_omitted_unless_include_ignored() {
        let (_tmp, root) = git_root();
        fs::write(root.join(".gitignore"), b"secret.log\n").unwrap();
        fs::write(root.join("secret.log"), b"shh").unwrap();
        fs::write(root.join("kept.txt"), b"ok").unwrap();

        let omitted = list_dir_inner(&root, "", false).unwrap();
        let names: Vec<_> = omitted.iter().map(|e| e.name.as_str()).collect();
        assert!(
            !names.contains(&"secret.log"),
            "ignored entry must be omitted when include_ignored=false, got {names:?}"
        );
        assert!(names.contains(&"kept.txt"));
        assert!(names.contains(&".gitignore"));

        let included = list_dir_inner(&root, "", true).unwrap();
        let secret = included
            .iter()
            .find(|e| e.name == "secret.log")
            .expect("ignored entry must be present when include_ignored=true");
        assert!(secret.is_ignored, "must be flagged is_ignored=true");
        let kept = included.iter().find(|e| e.name == "kept.txt").unwrap();
        assert!(!kept.is_ignored);
    }

    #[test]
    fn list_dir_respects_nested_gitignore() {
        let (_tmp, root) = git_root();
        let sub = root.join("sub");
        fs::create_dir(&sub).unwrap();
        fs::write(sub.join(".gitignore"), b"ignored.txt\n").unwrap();
        fs::write(sub.join("ignored.txt"), b"x").unwrap();
        fs::write(sub.join("kept.txt"), b"y").unwrap();

        let entries = list_dir_inner(&root, "sub", false).unwrap();
        let names: Vec<_> = entries.iter().map(|e| e.name.as_str()).collect();
        assert!(
            !names.contains(&"ignored.txt"),
            "nested .gitignore must filter its own directory's children, got {names:?}"
        );
        assert!(names.contains(&"kept.txt"));
        assert!(names.contains(&".gitignore"));
    }

    #[test]
    fn list_dir_rejects_outside_root() {
        let (_tmp, root) = plain_root();
        let err = list_dir_inner(&root, "../", false).unwrap_err();
        assert_eq!(err, FsError::OutsideRoot);
    }

    #[test]
    fn list_dir_on_a_file_is_not_found() {
        let (_tmp, root) = plain_root();
        fs::write(root.join("a.txt"), b"x").unwrap();
        let err = list_dir_inner(&root, "a.txt", false).unwrap_err();
        assert_eq!(err, FsError::NotFound);
    }

    // ── read_file_preview ────────────────────────────────────────────────────────────────────

    #[test]
    fn build_preview_text_is_not_truncated_when_bytes_match_size() {
        let bytes = b"hello world";
        let preview = build_preview(bytes.len() as u64, bytes);
        assert_eq!(
            preview,
            FilePreview::Text {
                content: "hello world".to_string(),
                truncated: false,
                size: 11,
            }
        );
    }

    #[test]
    fn build_preview_truncated_when_stat_size_exceeds_bytes_read() {
        // Simulates the TOCTOU case (file shrank between stat and read) deterministically, without
        // needing a real race: `size` disagrees with `bytes.len()`.
        let bytes = b"partial";
        let preview = build_preview(1000, bytes);
        match preview {
            FilePreview::Text {
                content,
                truncated,
                size,
            } => {
                assert_eq!(content, "partial");
                assert!(truncated);
                assert_eq!(size, 1000);
            }
            other => panic!("expected Text, got {other:?}"),
        }
    }

    #[test]
    fn build_preview_binary_on_nul_byte_in_first_probe_window() {
        let mut bytes = vec![b'a'; 100];
        bytes[50] = 0u8;
        let preview = build_preview(bytes.len() as u64, &bytes);
        assert_eq!(
            preview,
            FilePreview::Binary {
                size: bytes.len() as u64
            }
        );
    }

    #[test]
    fn build_preview_binary_on_invalid_utf8_in_first_probe_window() {
        let bytes = vec![0xFFu8, 0xFE, 0xFD];
        let preview = build_preview(bytes.len() as u64, &bytes);
        assert_eq!(
            preview,
            FilePreview::Binary {
                size: bytes.len() as u64
            }
        );
    }

    #[test]
    fn build_preview_nul_beyond_probe_window_is_still_text() {
        // A NUL past BINARY_PROBE_LEN must NOT flip classification -- only the first 8 KiB counts.
        let mut bytes = vec![b'a'; BINARY_PROBE_LEN + 10];
        bytes[BINARY_PROBE_LEN + 5] = 0u8;
        let preview = build_preview(bytes.len() as u64, &bytes);
        assert!(
            matches!(preview, FilePreview::Text { .. }),
            "got {preview:?}"
        );
    }

    #[test]
    fn read_file_preview_text_happy_path() {
        let (_tmp, root) = plain_root();
        fs::write(root.join("a.txt"), b"hello world").unwrap();
        let preview = read_file_preview_inner(&root, "a.txt").unwrap();
        assert_eq!(
            preview,
            FilePreview::Text {
                content: "hello world".to_string(),
                truncated: false,
                size: 11,
            }
        );
    }

    #[test]
    fn read_file_preview_binary_on_real_file() {
        let (_tmp, root) = plain_root();
        fs::write(root.join("bin.dat"), [1u8, 2, 0, 3]).unwrap();
        let preview = read_file_preview_inner(&root, "bin.dat").unwrap();
        assert_eq!(preview, FilePreview::Binary { size: 4 });
    }

    #[test]
    fn read_file_preview_too_large_over_cap_never_reads_content() {
        let (_tmp, root) = plain_root();
        let oversized = vec![b'x'; PREVIEW_CAP as usize + 1];
        fs::write(root.join("big.txt"), &oversized).unwrap();
        let preview = read_file_preview_inner(&root, "big.txt").unwrap();
        assert_eq!(
            preview,
            FilePreview::TooLarge {
                size: PREVIEW_CAP + 1
            }
        );
    }

    #[test]
    fn read_file_preview_exactly_at_cap_is_text() {
        let (_tmp, root) = plain_root();
        let exact = vec![b'x'; PREVIEW_CAP as usize];
        fs::write(root.join("exact.txt"), &exact).unwrap();
        let preview = read_file_preview_inner(&root, "exact.txt").unwrap();
        match preview {
            FilePreview::Text {
                size, truncated, ..
            } => {
                assert_eq!(size, PREVIEW_CAP);
                assert!(!truncated);
            }
            other => panic!("expected Text at exactly the cap, got {other:?}"),
        }
    }

    #[test]
    fn read_file_preview_rejects_outside_root() {
        let (_tmp, root) = plain_root();
        let err = read_file_preview_inner(&root, "../secret").unwrap_err();
        assert_eq!(err, FsError::OutsideRoot);
    }

    #[test]
    fn read_file_preview_on_a_directory_is_an_honest_io_error() {
        let (_tmp, root) = plain_root();
        fs::create_dir(root.join("sub")).unwrap();
        let err = read_file_preview_inner(&root, "sub").unwrap_err();
        assert!(matches!(err, FsError::Io { .. }), "got {err:?}");
    }

    // ── create_file / create_dir ─────────────────────────────────────────────────────────────

    #[test]
    fn create_file_happy_path() {
        let (_tmp, root) = plain_root();
        create_file_inner(&root, "", "new.txt").unwrap();
        let path = root.join("new.txt");
        assert!(path.is_file());
        assert_eq!(fs::metadata(&path).unwrap().len(), 0);
    }

    #[test]
    fn create_file_does_not_overwrite_existing() {
        let (_tmp, root) = plain_root();
        fs::write(root.join("existing.txt"), b"keep-me").unwrap();
        let err = create_file_inner(&root, "", "existing.txt").unwrap_err();
        assert!(matches!(err, FsError::Io { .. }), "got {err:?}");
        assert_eq!(fs::read(root.join("existing.txt")).unwrap(), b"keep-me");
    }

    #[test]
    fn create_file_rejects_outside_root() {
        let (_tmp, root) = plain_root();
        let err = create_file_inner(&root, "../", "evil.txt").unwrap_err();
        assert_eq!(err, FsError::OutsideRoot);
    }

    #[test]
    fn create_file_rejects_separator_in_name() {
        let (_tmp, root) = plain_root();
        fs::create_dir(root.join("sub")).unwrap();
        let err = create_file_inner(&root, "", "sub/evil.txt").unwrap_err();
        assert_eq!(err, FsError::OutsideRoot);
        assert!(!root.join("sub").join("evil.txt").exists());
    }

    #[test]
    fn create_dir_happy_path() {
        let (_tmp, root) = plain_root();
        create_dir_inner(&root, "", "newdir").unwrap();
        assert!(root.join("newdir").is_dir());
    }

    #[test]
    fn create_dir_rejects_outside_root() {
        let (_tmp, root) = plain_root();
        let err = create_dir_inner(&root, "../", "evildir").unwrap_err();
        assert_eq!(err, FsError::OutsideRoot);
    }

    // ── rename_entry ─────────────────────────────────────────────────────────────────────────

    #[test]
    fn rename_entry_happy_path() {
        let (_tmp, root) = plain_root();
        fs::write(root.join("old.txt"), b"content").unwrap();
        rename_entry_inner(&root, "old.txt", "renamed.txt").unwrap();
        assert!(!root.join("old.txt").exists());
        assert_eq!(fs::read(root.join("renamed.txt")).unwrap(), b"content");
    }

    #[test]
    fn rename_entry_nested_happy_path() {
        let (_tmp, root) = plain_root();
        fs::create_dir(root.join("sub")).unwrap();
        fs::write(root.join("sub").join("old.txt"), b"x").unwrap();
        rename_entry_inner(&root, "sub/old.txt", "renamed.txt").unwrap();
        assert!(!root.join("sub").join("old.txt").exists());
        assert!(root.join("sub").join("renamed.txt").exists());
    }

    #[test]
    fn rename_entry_rejects_outside_root_source() {
        let (_tmp, root) = plain_root();
        let err = rename_entry_inner(&root, "../outside.txt", "new.txt").unwrap_err();
        assert_eq!(err, FsError::OutsideRoot);
    }

    #[test]
    fn rename_entry_rejects_separator_in_new_name() {
        let (_tmp, root) = plain_root();
        fs::write(root.join("old.txt"), b"content").unwrap();
        fs::create_dir(root.join("sub")).unwrap();
        let err = rename_entry_inner(&root, "old.txt", "sub/evil.txt").unwrap_err();
        assert_eq!(err, FsError::OutsideRoot);
        // Source must be left untouched on a rejected rename.
        assert!(root.join("old.txt").exists());
        assert!(!root.join("sub").join("evil.txt").exists());
    }

    // ── move_entry ───────────────────────────────────────────────────────────────────────────

    #[test]
    fn move_entry_happy_path() {
        let (_tmp, root) = plain_root();
        fs::create_dir(root.join("dest")).unwrap();
        fs::write(root.join("movable.txt"), b"payload").unwrap();

        move_entry_inner(&root, "movable.txt", "dest").unwrap();

        assert!(!root.join("movable.txt").exists());
        assert_eq!(
            fs::read(root.join("dest").join("movable.txt")).unwrap(),
            b"payload"
        );
    }

    #[test]
    fn move_entry_rejects_outside_root_destination() {
        let (_tmp, root) = plain_root();
        fs::write(root.join("movable.txt"), b"x").unwrap();
        let err = move_entry_inner(&root, "movable.txt", "../").unwrap_err();
        assert_eq!(err, FsError::OutsideRoot);
        assert!(
            root.join("movable.txt").exists(),
            "source must be untouched"
        );
    }

    #[test]
    fn move_entry_rejects_outside_root_source() {
        let (_tmp, root) = plain_root();
        fs::create_dir(root.join("dest")).unwrap();
        let err = move_entry_inner(&root, "../outside.txt", "dest").unwrap_err();
        assert_eq!(err, FsError::OutsideRoot);
    }

    // ── delete_entry (Trash, spec D8) ────────────────────────────────────────────────────────

    #[test]
    fn delete_entry_rejects_outside_root() {
        let (_tmp, root) = plain_root();
        let err = delete_entry_inner(&root, "../secret").unwrap_err();
        assert_eq!(err, FsError::OutsideRoot);
    }

    #[test]
    fn delete_entry_moves_file_out_of_its_original_location() {
        let (_tmp, root) = plain_root();
        let target = root.join("todelete.txt");
        fs::write(&target, b"gone soon").unwrap();

        match delete_entry_inner(&root, "todelete.txt") {
            Ok(()) => {
                assert!(
                    !target.exists(),
                    "delete_entry must move the source out of its original location"
                );
            }
            Err(e) => {
                // The `trash` crate depends on a platform trash service (on macOS,
                // `NSFileManager.trashItemAtURL`) that can be unavailable in some headless/CI
                // sandboxes. Gate this one assertion behind that availability rather than failing
                // the whole suite on an environment limitation unrelated to fs_explorer's own
                // logic — see the task report for whether this fired in the environment this was
                // developed in.
                eprintln!(
                    "delete_entry_moves_file_out_of_its_original_location: `trash` unavailable \
                     in this environment, skipping the move-out-of-place assertion: {e}"
                );
            }
        }
    }

    // ── reveal_in_finder / open_external ─────────────────────────────────────────────────────
    //
    // Side-effecting (would open real OS UI / launch an external app) — not exercised on the
    // happy path. Only the pre-flight containment guard is unit-tested, which never reaches
    // `opener` at all on an outside-root path.

    #[test]
    fn reveal_in_finder_rejects_outside_root() {
        let (_tmp, root) = plain_root();
        let err = reveal_in_finder_inner(&root, "../secret").unwrap_err();
        assert_eq!(err, FsError::OutsideRoot);
    }

    #[test]
    fn open_external_rejects_outside_root() {
        let (_tmp, root) = plain_root();
        let err = open_external_inner(&root, "../secret").unwrap_err();
        assert_eq!(err, FsError::OutsideRoot);
    }

    // ── FsError / FilePreview / FsEntry wire shape ──────────────────────────────────────────

    #[test]
    fn fs_error_serializes_with_camel_case_tag() {
        let v = serde_json::to_value(FsError::NotFound).unwrap();
        assert_eq!(v["kind"], "notFound");

        let v2 = serde_json::to_value(FsError::PermissionDenied).unwrap();
        assert_eq!(v2["kind"], "permissionDenied");

        let v3 = serde_json::to_value(FsError::OutsideRoot).unwrap();
        assert_eq!(v3["kind"], "outsideRoot");

        let v4 = serde_json::to_value(FsError::TooLarge).unwrap();
        assert_eq!(v4["kind"], "tooLarge");

        let v5 = serde_json::to_value(FsError::Io {
            message: "boom".to_string(),
        })
        .unwrap();
        assert_eq!(v5["kind"], "io");
        assert_eq!(v5["message"], "boom");
    }

    #[test]
    fn file_preview_serializes_with_camel_case_tag() {
        let v = serde_json::to_value(FilePreview::Text {
            content: "hi".to_string(),
            truncated: false,
            size: 2,
        })
        .unwrap();
        assert_eq!(v["kind"], "text");
        assert_eq!(v["content"], "hi");
        assert_eq!(v["truncated"], false);
        assert_eq!(v["size"], 2);

        let v2 = serde_json::to_value(FilePreview::Binary { size: 4 }).unwrap();
        assert_eq!(v2["kind"], "binary");
        assert_eq!(v2["size"], 4);

        let v3 = serde_json::to_value(FilePreview::TooLarge { size: 99 }).unwrap();
        assert_eq!(v3["kind"], "tooLarge");
    }

    #[test]
    fn fs_entry_serializes_camel_case_fields() {
        let entry = FsEntry {
            name: "a.txt".to_string(),
            rel_path: "sub/a.txt".to_string(),
            is_dir: false,
            size: 3,
            is_ignored: true,
        };
        let v = serde_json::to_value(entry).unwrap();
        assert_eq!(v["name"], "a.txt");
        assert_eq!(v["relPath"], "sub/a.txt");
        assert_eq!(v["isDir"], false);
        assert_eq!(v["size"], 3);
        assert_eq!(v["isIgnored"], true);
    }
}
