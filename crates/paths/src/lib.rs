//! Shared directory validation for workspace roots and session cwd. This is the SINGLE
//! implementation used by BOTH the Tauri core (Hop-A) and the `bpa-sessiond` daemon (Hop-B), so
//! the two surfaces enforce byte-for-byte the same rule (spec §16) and can never drift again. S6
//! agents drive the same CreateWorkspace/CreateSession surface, so the daemon is the
//! security-authoritative validator; the core validates too for fail-fast defense in depth.
//! Canonicalize + absolute + exists + is-dir + no symlink-escape.

use std::path::{Path, PathBuf};

/// Typed reason a directory is invalid. `code()` yields the wire code string the
/// broker/daemon surface uses in `Response::Error { code, .. }` (spec §13).
#[derive(Debug, thiserror::Error)]
pub enum PathError {
    #[error("path is not absolute: {0}")]
    NotAbsolute(String),
    #[error("path does not exist: {0}")]
    Missing(String),
    #[error("path is not a directory: {0}")]
    NotADirectory(String),
    #[error("path escapes via symlink: {0}")]
    SymlinkEscape(String),
    #[error("cannot canonicalize {path}: {source}")]
    Canonicalize {
        path: String,
        source: std::io::Error,
    },
}

impl PathError {
    /// Stable wire code for `Response::Error { code, .. }`.
    pub fn code(&self) -> &'static str {
        match self {
            PathError::NotAbsolute(_) => "RelativePath",
            PathError::Missing(_) => "CwdMissing",
            PathError::NotADirectory(_) => "NotADirectory",
            PathError::SymlinkEscape(_) => "SymlinkEscape",
            PathError::Canonicalize { .. } => "InvalidWorkspaceRoot",
        }
    }
}

/// Validate a workspace root or session cwd: must be absolute, exist, be a real
/// directory, and not escape its own lexical parent via symlink. Returns the
/// canonicalized (realpath) absolute `PathBuf`.
pub fn validate_dir(path: &Path) -> Result<PathBuf, PathError> {
    let display = || path.display().to_string();

    // 1. absolute, checked before any filesystem access.
    if !path.is_absolute() {
        return Err(PathError::NotAbsolute(display()));
    }

    // 2. canonicalize (realpath): resolves symlinks + `.`/`..`.
    let canonical = match std::fs::canonicalize(path) {
        Ok(c) => c,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Err(PathError::Missing(display()));
        }
        Err(source) => {
            return Err(PathError::Canonicalize {
                path: display(),
                source,
            });
        }
    };

    // 3. must be a directory.
    let meta = std::fs::metadata(&canonical).map_err(|source| PathError::Canonicalize {
        path: display(),
        source,
    })?;
    if !meta.is_dir() {
        return Err(PathError::NotADirectory(display()));
    }

    // 4. symlink-escape: the canonicalized result must stay within the
    //    canonicalized *lexical parent* of the input. If the input is a root
    //    (no parent), accept. If the parent cannot be canonicalized, the input's
    //    lineage is unresolvable -> treat as escape (fail closed).
    match path.parent() {
        None => Ok(canonical), // root
        Some(parent) if parent.as_os_str().is_empty() => Ok(canonical),
        Some(parent) => {
            let canonical_parent =
                std::fs::canonicalize(parent).map_err(|_| PathError::SymlinkEscape(display()))?;
            if canonical.starts_with(&canonical_parent) {
                Ok(canonical)
            } else {
                Err(PathError::SymlinkEscape(display()))
            }
        }
    }
}

/// Validate that `candidate` resolves (after symlink/`.`/`..` resolution) to a path
/// contained within (or equal to) `root`. Both `root` and `candidate` must already exist.
/// Returns the canonicalized `candidate`. This is the containment primitive every
/// file-explorer operation (list/read/rename/move/delete) validates against before
/// touching the filesystem (spec §4.1).
///
/// Fails closed: any canonicalize error (missing path, broken symlink, permission
/// error, ...) or a candidate whose canonical form is not a descendant of the
/// canonical root is `Err(PathError::SymlinkEscape)` -- never an optimistic pass.
pub fn validate_path_within(root: &Path, candidate: &Path) -> Result<PathBuf, PathError> {
    let canonical_root = std::fs::canonicalize(root).map_err(|source| PathError::Canonicalize {
        path: root.display().to_string(),
        source,
    })?;

    let canonical_candidate = std::fs::canonicalize(candidate).map_err(|_| {
        PathError::SymlinkEscape(format!(
            "{} could not be resolved relative to root {}",
            candidate.display(),
            root.display()
        ))
    })?;

    if canonical_candidate.starts_with(&canonical_root) {
        Ok(canonical_candidate)
    } else {
        Err(PathError::SymlinkEscape(format!(
            "{} resolves outside root {}",
            candidate.display(),
            root.display()
        )))
    }
}

/// Validate a not-yet-existing create/rename destination under `root`. The PARENT of
/// `target` must already exist and resolve within `root`; the final path component of
/// `target` must be a single real path segment -- not `.`/`..`, and (defensively) free of
/// embedded path separators. Returns the canonicalized parent joined with that final
/// component, i.e. the canonical path `target` would have once created.
///
/// Fails closed: an unresolvable/foreign parent, or a final component that is not a
/// plain path segment, is `Err(PathError::SymlinkEscape)`.
pub fn validate_parent_within(root: &Path, target: &Path) -> Result<PathBuf, PathError> {
    let final_component = target.file_name().ok_or_else(|| {
        PathError::SymlinkEscape(format!(
            "{} has no valid final path segment (ends in '.', '..', or is empty/root)",
            target.display()
        ))
    })?;

    // Defensive: `file_name()` can never itself contain a separator on a well-formed
    // `Path` (a separator always starts a new component), but this is the
    // security-authoritative check, so guard explicitly rather than relying on that.
    if final_component
        .to_string_lossy()
        .contains(std::path::MAIN_SEPARATOR)
    {
        return Err(PathError::SymlinkEscape(format!(
            "{} final component contains a path separator",
            target.display()
        )));
    }

    let parent = target.parent().ok_or_else(|| {
        PathError::SymlinkEscape(format!("{} has no parent directory", target.display()))
    })?;

    let canonical_parent = validate_path_within(root, parent)?;
    Ok(canonical_parent.join(final_component))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::{Path, PathBuf};

    #[test]
    fn ok_real_directory_canonicalizes() {
        let dir = tempfile::tempdir().unwrap();
        let sub = dir.path().join("ws");
        fs::create_dir(&sub).unwrap();
        let got = validate_dir(&sub).expect("valid dir");
        // canonicalized result is absolute, is a dir, and ends with our component
        assert!(got.is_absolute());
        assert!(got.is_dir());
        assert_eq!(got, fs::canonicalize(&sub).unwrap());
    }

    #[test]
    fn relative_path_is_rejected_before_fs() {
        let rel = Path::new("some/relative/dir");
        let err = validate_dir(rel).unwrap_err();
        assert!(matches!(err, PathError::NotAbsolute(_)), "got {err:?}");
        assert_eq!(err.code(), "RelativePath");
    }

    #[test]
    fn missing_path_is_missing() {
        let dir = tempfile::tempdir().unwrap();
        let gone = dir.path().join("does-not-exist");
        let err = validate_dir(&gone).unwrap_err();
        assert!(matches!(err, PathError::Missing(_)), "got {err:?}");
        assert_eq!(err.code(), "CwdMissing");
    }

    #[test]
    fn file_is_not_a_directory() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("afile");
        fs::write(&file, b"x").unwrap();
        let err = validate_dir(&file).unwrap_err();
        assert!(matches!(err, PathError::NotADirectory(_)), "got {err:?}");
        assert_eq!(err.code(), "NotADirectory");
    }

    #[test]
    fn symlink_escaping_parent_is_rejected() {
        // layout:
        //   base/outside/         (real target dir, OUTSIDE `named`)
        //   base/named/link -> ../outside
        // validate_dir(base/named/link) canonicalizes to base/outside, whose
        // parent (base) != canonical parent of the input (base/named) -> escape.
        let base = tempfile::tempdir().unwrap();
        let outside = base.path().join("outside");
        let named = base.path().join("named");
        fs::create_dir(&outside).unwrap();
        fs::create_dir(&named).unwrap();
        let link: PathBuf = named.join("link");
        std::os::unix::fs::symlink(&outside, &link).unwrap();

        let err = validate_dir(&link).unwrap_err();
        assert!(matches!(err, PathError::SymlinkEscape(_)), "got {err:?}");
        assert_eq!(err.code(), "SymlinkEscape");
    }

    #[test]
    fn symlink_within_parent_is_allowed() {
        // base/target/  and  base/link -> target  : realpath stays under base -> OK
        let base = tempfile::tempdir().unwrap();
        let target = base.path().join("target");
        fs::create_dir(&target).unwrap();
        let link = base.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        let got = validate_dir(&link).expect("sibling symlink under same parent is allowed");
        assert_eq!(got, fs::canonicalize(&target).unwrap());
    }

    #[test]
    fn root_path_is_allowed() {
        let got = validate_dir(Path::new("/")).expect("root is a valid directory");
        assert_eq!(got, fs::canonicalize("/").unwrap());
    }

    // -- validate_path_within -------------------------------------------------------

    #[test]
    fn candidate_inside_root_returns_canonical_path() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        fs::create_dir(&root).unwrap();
        let sub = root.join("sub");
        fs::create_dir(&sub).unwrap();

        let got = validate_path_within(&root, &sub).expect("candidate inside root");
        assert_eq!(got, fs::canonicalize(&sub).unwrap());
    }

    #[test]
    fn candidate_equal_to_root_is_ok() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        fs::create_dir(&root).unwrap();

        let got = validate_path_within(&root, &root).expect("candidate == root");
        assert_eq!(got, fs::canonicalize(&root).unwrap());
    }

    #[test]
    fn dotdot_escape_is_rejected() {
        // root/../outside canonicalizes to a real, existing sibling dir -> outside root.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        let outside = dir.path().join("outside");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&outside).unwrap();

        let candidate = root.join("..").join("outside");
        let err = validate_path_within(&root, &candidate).unwrap_err();
        assert!(matches!(err, PathError::SymlinkEscape(_)), "got {err:?}");
        assert_eq!(err.code(), "SymlinkEscape");
    }

    #[test]
    fn symlink_pointing_outside_root_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        let outside = dir.path().join("outside");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&outside).unwrap();
        let link = root.join("link");
        std::os::unix::fs::symlink(&outside, &link).unwrap();

        let err = validate_path_within(&root, &link).unwrap_err();
        assert!(matches!(err, PathError::SymlinkEscape(_)), "got {err:?}");
        assert_eq!(err.code(), "SymlinkEscape");
    }

    #[test]
    fn candidate_that_does_not_exist_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        fs::create_dir(&root).unwrap();
        let gone = root.join("does-not-exist");

        let err = validate_path_within(&root, &gone).unwrap_err();
        assert!(matches!(err, PathError::SymlinkEscape(_)), "got {err:?}");
    }

    // -- validate_parent_within ------------------------------------------------------

    #[test]
    fn parent_within_root_with_fresh_filename_is_ok() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        fs::create_dir(&root).unwrap();
        let target = root.join("new-file.txt"); // does not exist yet

        let got = validate_parent_within(&root, &target).expect("fresh filename under root");
        assert_eq!(got, fs::canonicalize(&root).unwrap().join("new-file.txt"));
    }

    #[test]
    fn parent_within_nested_existing_dir_with_fresh_filename_is_ok() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        let nested = root.join("nested");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&nested).unwrap();
        let target = nested.join("new-file.txt");

        let got = validate_parent_within(&root, &target).expect("fresh filename under nested dir");
        assert_eq!(got, fs::canonicalize(&nested).unwrap().join("new-file.txt"));
    }

    #[test]
    fn parent_within_multi_segment_final_component_is_rejected() {
        // "a" does not exist yet, so the parent of target ("root/a") cannot canonicalize.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        fs::create_dir(&root).unwrap();
        let target = root.join("a").join("b");

        let err = validate_parent_within(&root, &target).unwrap_err();
        assert!(matches!(err, PathError::SymlinkEscape(_)), "got {err:?}");
        assert_eq!(err.code(), "SymlinkEscape");
    }

    #[test]
    fn parent_within_dotdot_final_component_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        fs::create_dir(&root).unwrap();
        let target = root.join("..");

        let err = validate_parent_within(&root, &target).unwrap_err();
        assert!(matches!(err, PathError::SymlinkEscape(_)), "got {err:?}");
        assert_eq!(err.code(), "SymlinkEscape");
    }

    #[test]
    fn parent_within_dot_final_component_is_rejected() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        fs::create_dir(&root).unwrap();
        let target = root.join(".");

        let err = validate_parent_within(&root, &target).unwrap_err();
        assert!(matches!(err, PathError::SymlinkEscape(_)), "got {err:?}");
    }

    #[test]
    fn parent_within_root_but_symlink_escaping_parent_is_rejected() {
        // root/escape -> ../outside ; target = root/escape/new-file.txt
        // parent (root/escape) resolves outside root -> rejected.
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("root");
        let outside = dir.path().join("outside");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&outside).unwrap();
        let escape = root.join("escape");
        std::os::unix::fs::symlink(&outside, &escape).unwrap();
        let target = escape.join("new-file.txt");

        let err = validate_parent_within(&root, &target).unwrap_err();
        assert!(matches!(err, PathError::SymlinkEscape(_)), "got {err:?}");
    }

    // -- SUS-5 audit probes: the containment boundary has no workspace allowlist -----------------

    /// pin (BL-109, resolved at the command layer): `validate_path_within` ITSELF stays a PURE
    /// canonical-containment check against whatever `root` the caller supplies — with `root ==
    /// "/"` ANY existing absolute path is "within" it. That is deliberate: the registered-
    /// workspace allowlist this probe asked for ("desired: `root` validated against a
    /// registered-workspace allowlist before any fs operation honors it") is NOT this function's
    /// job — it now lives one layer up, in `src-tauri`'s `commands::ensure_registered_root`
    /// (BL-109): every fs_explorer/fs_watcher command first requires `root` to be contained in a
    /// daemon-REGISTERED workspace root (fail-closed `Disconnected` when the roots cache has
    /// never loaded), and only then do these lexical validators run, unchanged. This pin keeps
    /// the lexical primitive's exact behavior locked so the two layers can't be conflated.
    #[test]
    fn pin_sus5_validate_path_within_with_root_slash_accepts_etc_passwd() {
        let got = validate_path_within(Path::new("/"), Path::new("/etc/passwd"))
            .expect("pin: lexical-only by design — root \"/\" trivially contains everything");
        assert!(got.ends_with("passwd"), "got {got:?}");
    }

    /// Control for SUS-5: within a REAL workspace root the same function is still fail-closed —
    /// a symlink inside the root resolving outside it is rejected (pin of the good behavior, so
    /// the SUS-5 finding is scoped precisely to "no allowlist", not "containment broken").
    #[test]
    fn pin_sus5_control_real_root_symlink_escape_fails_closed() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("workspace");
        let outside = dir.path().join("outside");
        fs::create_dir(&root).unwrap();
        fs::create_dir(&outside).unwrap();
        fs::write(outside.join("secret.txt"), b"top").unwrap();
        let link = root.join("link");
        std::os::unix::fs::symlink(&outside, &link).unwrap();

        let err = validate_path_within(&root, &link.join("secret.txt")).unwrap_err();
        assert!(matches!(err, PathError::SymlinkEscape(_)), "got {err:?}");
    }
}
