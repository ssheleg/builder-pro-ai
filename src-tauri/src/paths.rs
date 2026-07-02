//! Directory validation for workspace roots and session cwd. The implementation lives in the
//! shared `bpa-paths` crate so the core (Hop-A) and the daemon (Hop-B) enforce byte-for-byte the
//! same rule (spec §16) and can never drift again. The daemon is the security-authoritative
//! surface (S6 agents drive it); the core validates too for fail-fast defense in depth.
//! Re-exported here so existing `crate::paths::…` call sites are unchanged.
pub use bpa_paths::{validate_dir, PathError};
