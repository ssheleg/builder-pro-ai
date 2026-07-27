//! App-support directory resolution (spec §3, §8.1): durable state — DB, settings, logs — lives
//! here, never next to the short daemon socket path. Shared by every daemon built on
//! `bpa-daemon-core`. MOVED from `bpa-sessiond::boot::app_support_dir` (S3 phase 1 extraction);
//! body unchanged, only visibility widened from `pub(crate)` to `pub`.

use std::path::PathBuf;

/// Resolve `~/Library/Application Support/ai.builderpro.desktop`.
pub fn app_support_dir() -> PathBuf {
    home_dir().join("Library/Application Support/ai.builderpro.desktop")
}

/// Resolve the user's home directory: `$HOME` → the passwd-db entry for the real uid
/// (`getpwuid(getuid())`) → `/tmp` (last resort, logged loudly). macOS `launchd` always sets `$HOME`
/// for user agents, so the fallbacks are defense-in-depth — but a misconfigured / `env -i` launch
/// without HOME previously landed the durable DB + logs in volatile `/tmp` (BL-196), silently
/// losing state across reboot. The passwd-db fallback closes that hole.
fn home_dir() -> PathBuf {
    if let Some(h) = std::env::var_os("HOME").filter(|h| !h.is_empty()) {
        return PathBuf::from(h);
    }
    if let Some(h) = home_from_passwd() {
        return h;
    }
    tracing::warn!(
        "HOME is unset and the passwd db has no entry for the uid — durable state (DB, settings, \
         logs) will land in volatile /tmp and be LOST across reboot (BL-196)"
    );
    PathBuf::from("/tmp")
}

/// `getpwuid(getuid())` → the user's home from the passwd database (authoritative on macOS when
/// `$HOME` is absent). `None` if the lookup fails or the entry's `pw_dir` is null/empty.
fn home_from_passwd() -> Option<PathBuf> {
    // SAFETY: `getpwuid` is a read-only libc lookup; the returned `*passwd` is valid until the next
    // libc call that overwrites it (we copy the strings out immediately, single-threaded use here).
    unsafe {
        let uid = libc::getuid();
        let pw = libc::getpwuid(uid);
        if pw.is_null() {
            return None;
        }
        let dir = (*pw).pw_dir;
        if dir.is_null() {
            return None;
        }
        let s = std::ffi::CStr::from_ptr(dir).to_str().ok()?;
        if s.is_empty() {
            None
        } else {
            Some(PathBuf::from(s))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_support_dir_is_under_home() {
        let dir = app_support_dir();
        assert!(dir.ends_with("Library/Application Support/ai.builderpro.desktop"));
    }
}
