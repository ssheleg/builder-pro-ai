//! Single-instance lock, socket path resolution, dir/socket permissions, peer-cred (spec §8.1–§8.2, §16).
use std::fs::{DirBuilder, File, OpenOptions};
use std::io::{self, Error, ErrorKind};
use std::os::fd::{AsFd, BorrowedFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt};
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

/// Verify an existing socket dir is a directory owned by the current euid with mode 0700,
/// or create it fresh with mode 0700. Guards the `/tmp` squatting race (spec §8.2).
fn ensure_dir(dir: &Path) -> io::Result<()> {
    match std::fs::symlink_metadata(dir) {
        Ok(md) => {
            let euid = rustix::process::geteuid().as_raw();
            if !md.is_dir() {
                return Err(Error::new(
                    ErrorKind::PermissionDenied,
                    format!("socket dir path is not a directory: {}", dir.display()),
                ));
            }
            if md.uid() != euid {
                return Err(Error::new(
                    ErrorKind::PermissionDenied,
                    format!("socket dir {} not owned by uid {euid}", dir.display()),
                ));
            }
            if md.mode() & 0o777 != 0o700 {
                return Err(Error::new(
                    ErrorKind::PermissionDenied,
                    format!(
                        "socket dir {} mode {:o} != 0700",
                        dir.display(),
                        md.mode() & 0o777
                    ),
                ));
            }
            Ok(())
        }
        Err(e) if e.kind() == ErrorKind::NotFound => {
            DirBuilder::new().mode(0o700).create(dir)?;
            Ok(())
        }
        Err(e) => Err(e),
    }
}

/// Ensure the resolved socket directory exists at mode 0700 owned by us (spec §8.1–§8.2).
pub fn ensure_socket_dir() -> io::Result<()> {
    ensure_dir(&socket_dir())
}

/// Owns the exclusively-flocked lockfile for the daemon's whole lifetime.
/// Dropping the guard releases the advisory lock.
#[derive(Debug)]
pub struct LockGuard {
    _file: File,
}

fn acquire_lock_at(path: &Path) -> io::Result<LockGuard> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(path)?;
    match flock(file.as_fd(), FlockOperation::NonBlockingLockExclusive) {
        Ok(()) => Ok(LockGuard { _file: file }),
        Err(e) if e == rustix::io::Errno::WOULDBLOCK || e == rustix::io::Errno::AGAIN => Err(
            Error::new(ErrorKind::WouldBlock, "another daemon holds the single-instance lock"),
        ),
        Err(e) => Err(Error::from_raw_os_error(e.raw_os_error())),
    }
}

/// Acquire the single-instance advisory lock at the resolved lockfile (spec §8.2).
/// A second daemon that cannot take the lock gets `ErrorKind::WouldBlock` and must exit.
pub fn acquire_single_instance_lock() -> io::Result<LockGuard> {
    acquire_lock_at(&resolve_lockfile())
}

/// Set the bound socket file to mode 0600 (spec §8.2).
pub fn set_socket_mode(sock: &Path) -> io::Result<()> {
    use std::fs::Permissions;
    std::fs::set_permissions(sock, Permissions::from_mode(0o600))
}

/// Read the effective uid of the peer connected to `fd` via `getpeereid(2)` (POSIX/macOS).
fn peer_euid(fd: BorrowedFd<'_>) -> io::Result<u32> {
    use std::os::fd::AsRawFd;
    let mut uid: libc::uid_t = 0;
    let mut gid: libc::gid_t = 0;
    // SAFETY: fd is a valid borrowed AF_UNIX socket fd for the duration of the call;
    // uid/gid are valid out-pointers.
    let rc = unsafe { libc::getpeereid(fd.as_raw_fd(), &mut uid, &mut gid) };
    if rc != 0 {
        return Err(Error::last_os_error());
    }
    Ok(uid as u32)
}

/// Compare the peer euid to `expected`; refuse on mismatch.
fn check_peer_cred_against(fd: BorrowedFd<'_>, expected: u32) -> io::Result<()> {
    let peer = peer_euid(fd)?;
    if peer != expected {
        return Err(Error::new(
            ErrorKind::PermissionDenied,
            format!("peer euid {peer} != daemon euid {expected}"),
        ));
    }
    Ok(())
}

/// Verify the connecting peer's effective uid equals the daemon's euid (spec §8.2, §16).
/// Refuse otherwise. `fd` must be an accepted AF_UNIX stream socket.
pub fn check_peer_cred(fd: BorrowedFd<'_>) -> io::Result<()> {
    let euid = rustix::process::geteuid().as_raw();
    check_peer_cred_against(fd, euid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::PermissionsExt;

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

    #[test]
    fn ensure_dir_creates_with_0700() {
        let base = tempfile::tempdir().unwrap();
        let dir = base.path().join("bpa");
        ensure_dir(&dir).expect("create ok");
        let md = std::fs::metadata(&dir).unwrap();
        assert!(md.is_dir());
        assert_eq!(md.permissions().mode() & 0o777, 0o700);
        assert_eq!(md.uid(), rustix::process::geteuid().as_raw());
        // Idempotent: second call on our own 0700 dir succeeds.
        ensure_dir(&dir).expect("idempotent ok");
    }

    #[test]
    fn ensure_dir_refuses_world_writable_squat() {
        let base = tempfile::tempdir().unwrap();
        let dir = base.path().join("bpa");
        std::fs::create_dir(&dir).unwrap();
        std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o777)).unwrap();
        let err = ensure_dir(&dir).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn ensure_dir_refuses_non_directory() {
        let base = tempfile::tempdir().unwrap();
        let path = base.path().join("bpa");
        std::fs::write(&path, b"not a dir").unwrap();
        let err = ensure_dir(&path).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
    }

    #[test]
    fn second_flock_on_same_lockfile_would_block() {
        let base = tempfile::tempdir().unwrap();
        let lock = base.path().join("d.lock");
        let g1 = acquire_lock_at(&lock).expect("first lock ok");
        let err = acquire_lock_at(&lock).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::WouldBlock);
        drop(g1);
        // After the first guard drops, the lock is re-acquirable.
        let _g2 = acquire_lock_at(&lock).expect("re-lock after drop ok");
    }

    #[test]
    fn lockfile_created_mode_0600() {
        let base = tempfile::tempdir().unwrap();
        let lock = base.path().join("d.lock");
        let _g = acquire_lock_at(&lock).unwrap();
        let md = std::fs::metadata(&lock).unwrap();
        assert_eq!(md.permissions().mode() & 0o777, 0o600);
    }

    #[test]
    fn set_socket_mode_applies_0600() {
        let base = tempfile::tempdir().unwrap();
        let sock = base.path().join("d.sock");
        std::fs::write(&sock, b"").unwrap();
        set_socket_mode(&sock).unwrap();
        let md = std::fs::metadata(&sock).unwrap();
        assert_eq!(md.permissions().mode() & 0o777, 0o600);
    }

    #[test]
    fn peer_cred_accepts_same_uid_over_socketpair() {
        use std::os::unix::net::UnixStream;
        let (a, _b) = UnixStream::pair().unwrap();
        // Our own connection: peer euid == our euid → accepted.
        check_peer_cred(a.as_fd()).expect("same-uid peer accepted");
    }

    #[test]
    fn peer_cred_rejects_foreign_uid_simulated() {
        use std::os::unix::net::UnixStream;
        let (a, _b) = UnixStream::pair().unwrap();
        let real = peer_euid(a.as_fd()).expect("read peer euid");
        // Simulate a foreign peer by comparing against a deliberately-wrong expected uid.
        let foreign = real.wrapping_add(1);
        let err = check_peer_cred_against(a.as_fd(), foreign).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::PermissionDenied);
    }
}
