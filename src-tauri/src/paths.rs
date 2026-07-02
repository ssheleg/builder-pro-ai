//! Daemon-and-core-shared directory validation for workspace roots and session cwd.
//! Enforced in the core (Hop-A) AND the daemon (Hop-B) because S6 agents drive the
//! same surface (spec §16). Canonicalize + absolute + exists + is-dir + no symlink-escape.

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
}
