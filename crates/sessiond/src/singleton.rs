//! Single-instance lock, socket path resolution, dir/socket permissions, peer-cred (spec §8.1–
//! §8.2, §16). Thin `bpa-sessiond`-specific wrapper over `bpa_daemon_core::singleton` (S3 phase 1
//! extraction, spec §3): pins the on-disk leaf names (`d.sock`/`d.lock`) sessiond has always
//! used, so on-disk paths and the daemon's real public API stay byte-identical / unchanged.
use std::io;
use std::path::{Path, PathBuf};

pub use bpa_daemon_core::singleton::{
    assert_socket_path_len, check_peer_cred, ensure_socket_dir, set_socket_mode, LockGuard,
};

const SOCKET_FILE_NAME: &str = "d.sock";
const LOCK_FILE_NAME: &str = "d.lock";

/// Resolve the daemon's Unix-domain-socket path (`<dir>/d.sock`). Never panics.
pub fn resolve_socket_path() -> PathBuf {
    bpa_daemon_core::singleton::resolve_socket_path(SOCKET_FILE_NAME)
}

/// Resolve the single-instance lockfile path (`<dir>/d.lock`).
pub fn resolve_lockfile() -> PathBuf {
    bpa_daemon_core::singleton::resolve_lockfile(LOCK_FILE_NAME)
}

/// Acquire the single-instance advisory lock at the resolved lockfile (spec §8.2).
/// A second daemon that cannot take the lock gets `ErrorKind::WouldBlock` and must exit.
pub fn acquire_single_instance_lock() -> io::Result<LockGuard> {
    bpa_daemon_core::singleton::acquire_single_instance_lock(LOCK_FILE_NAME)
}

/// Test-support wrapper exposing [`bpa_daemon_core::singleton::acquire_lock_at`] to integration
/// tests (Task 13 boot tests) without widening the crate's real single-instance entry point
/// beyond [`acquire_single_instance_lock`]. Not part of the daemon boot contract.
#[doc(hidden)]
pub fn acquire_lock_at_for_test(path: &Path) -> io::Result<LockGuard> {
    bpa_daemon_core::singleton::acquire_lock_at(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// New (S3 phase 1, Task 1): asserts the sessiond wrapper's resolved socket/lock paths still
    /// end with the exact on-disk leaf names sessiond has always used, post re-seat onto
    /// `bpa_daemon_core::singleton` — the byte-identical-path guarantee (spec §3 rule).
    #[test]
    fn resolved_socket_and_lock_paths_end_with_sessiond_leaf_names() {
        let sock = resolve_socket_path();
        assert!(
            sock.ends_with("d.sock"),
            "socket path {} must end with d.sock",
            sock.display()
        );
        let lock = resolve_lockfile();
        assert!(
            lock.ends_with("d.lock"),
            "lock path {} must end with d.lock",
            lock.display()
        );
    }
}
