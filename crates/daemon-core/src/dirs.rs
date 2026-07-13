//! App-support directory resolution (spec §3, §8.1): durable state — DB, settings, logs — lives
//! here, never next to the short daemon socket path. Shared by every daemon built on
//! `bpa-daemon-core`. MOVED from `bpa-sessiond::boot::app_support_dir` (S3 phase 1 extraction);
//! body unchanged, only visibility widened from `pub(crate)` to `pub`.

use std::path::PathBuf;

/// Resolve `~/Library/Application Support/ai.builderpro.desktop`.
pub fn app_support_dir() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"));
    home.join("Library/Application Support/ai.builderpro.desktop")
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
