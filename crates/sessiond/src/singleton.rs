//! Single-instance lock, socket path resolution, dir/socket permissions, peer-cred (spec §8.1–§8.2, §16).
use std::fs::{DirBuilder, File, OpenOptions};
use std::io::{self, Error, ErrorKind};
use std::os::fd::{AsFd, BorrowedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use rustix::fs::{flock, FlockOperation};

/// macOS `sun_path` is 104 bytes including NUL; usable length is strictly < 104.
const SUN_PATH_MAX: usize = 104;

fn socket_dir() -> PathBuf {
    match std::env::var_os("XDG_RUNTIME_DIR") {
        Some(x) if !x.is_empty() => PathBuf::from(x).join("bpa"),
        _ => {
            let uid = rustix::process::geteuid().as_raw();
            PathBuf::from(format!("/tmp/bpa-{uid}"))
        }
    }
}

/// Resolve the daemon's Unix-domain-socket path (`<dir>/d.sock`). Never panics.
pub fn resolve_socket_path() -> PathBuf {
    socket_dir().join("d.sock")
}

/// Resolve the single-instance lockfile path (`<dir>/d.lock`).
pub fn resolve_lockfile() -> PathBuf {
    socket_dir().join("d.lock")
}

/// Hard-fail (spec §8.1) if the socket path would overflow `sun_path`.
pub fn assert_socket_path_len(p: &Path) -> io::Result<()> {
    if p.as_os_str().as_bytes().len() >= SUN_PATH_MAX {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            format!(
                "socket path length {} >= sun_path max {SUN_PATH_MAX}: {}",
                p.as_os_str().as_bytes().len(),
                p.display()
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::ffi::OsStrExt;

    fn with_env<F: FnOnce()>(key: &str, val: Option<&str>, f: F) {
        let prev = std::env::var_os(key);
        match val {
            Some(v) => std::env::set_var(key, v),
            None => std::env::remove_var(key),
        }
        f();
        match prev {
            Some(p) => std::env::set_var(key, p),
            None => std::env::remove_var(key),
        }
    }

    #[test]
    fn socket_path_uses_xdg_runtime_dir_when_set() {
        with_env("XDG_RUNTIME_DIR", Some("/run/user/501"), || {
            let sock = resolve_socket_path();
            assert_eq!(sock, std::path::PathBuf::from("/run/user/501/bpa/d.sock"));
            let lock = resolve_lockfile();
            assert_eq!(lock, std::path::PathBuf::from("/run/user/501/bpa/d.lock"));
        });
    }

    #[test]
    fn socket_path_falls_back_to_tmp_with_uid_when_xdg_unset() {
        with_env("XDG_RUNTIME_DIR", None, || {
            let sock = resolve_socket_path();
            let uid = rustix::process::geteuid().as_raw();
            let expected = std::path::PathBuf::from(format!("/tmp/bpa-{uid}/d.sock"));
            assert_eq!(sock, expected);
        });
    }

    #[test]
    fn socket_path_falls_back_to_tmp_when_xdg_empty() {
        with_env("XDG_RUNTIME_DIR", Some(""), || {
            let sock = resolve_socket_path();
            let uid = rustix::process::geteuid().as_raw();
            assert_eq!(sock, std::path::PathBuf::from(format!("/tmp/bpa-{uid}/d.sock")));
        });
    }

    #[test]
    fn socket_path_len_under_104_passes_and_over_fails() {
        assert!(assert_socket_path_len(std::path::Path::new("/tmp/bpa-501/d.sock")).is_ok());
        let long = std::path::PathBuf::from(format!("/tmp/{}/d.sock", "x".repeat(120)));
        assert!(long.as_os_str().as_bytes().len() >= 104);
        let err = assert_socket_path_len(&long).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::InvalidInput);
    }
}
