//! Test-only structured-logging seam (Task 25, spec §13/§16 no-secrets-in-logs evidence). Thin
//! `bpa-sessiond` re-export of `bpa_daemon_core::logging`'s test seam (S3 phase 1 extraction,
//! spec §3) — `crates/sessiond/tests/*` keeps importing `bpa_sessiond::logging::{init_to_file,
//! flush}` unchanged.
//!
//! The daemon's REAL production logging init now lives in `bpa_daemon_core::logging::init_tracing`
//! (called from `main.rs`, parameterized with `"sessiond.tracing.log"`); it installs a global
//! `tracing_subscriber` writing to `{APP_SUPPORT}/logs/sessiond.tracing.log` exactly once, at
//! process start. That function is not reusable from an integration test binary:
//! `tracing_subscriber::registry()...init()` panics if a global default is already set, and a
//! test has no access to `main.rs`'s private items or to the resolved `{APP_SUPPORT}` path (tests
//! must not read/write the real user's app-support dir).
//!
//! [`init_to_file`]/[`flush`] give `crates/sessiond/tests/*` a small, explicit, test-only
//! equivalent instead: point the SAME kind of `tracing_subscriber::fmt` file layer at an
//! arbitrary temp path, as a **global** default. Safe to call from an integration test file
//! because each `tests/*.rs` file compiles to its own separate test binary/process — as long as
//! at most one `#[test]` per binary calls `init_to_file`, there is no "already initialized"
//! conflict. Not linked into `main.rs` and does not change the real daemon boot's logging
//! behavior in any way.
pub use bpa_daemon_core::logging::{flush, init_to_file};
