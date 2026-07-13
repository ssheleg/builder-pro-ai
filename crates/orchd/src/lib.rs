//! bpa-orchd — Builder Pro AI orchestration daemon (library surface for integration tests).
//! Mirrors `bpa_sessiond`'s crate-root shape minus PTY concerns (spec §5).

pub use bpa_orchd_proto as protocol;

pub mod persistence;
pub mod socket_server;

mod boot;
/// RuleSet markdown FILE layer (spec §7, D4): atomic writes + read-fresh state classification.
/// Crate-private — `persistence::Db`'s ruleset methods are the public surface that builds on it.
mod ruleset_files;
/// Test-support hook so integration tests can assert their `$HOME` isolation actually redirects
/// the daemon's on-disk DB/rules path (mirrors `bpa_sessiond::app_support_dir_for_test`). See
/// [`boot::app_support_dir_for_test`].
pub use boot::app_support_dir_for_test;
/// Testable daemon boot core (spec §5): bind, open the DB, ensure the global ruleset, run
/// `serve` until shutdown, then drain. `main.rs` is a thin process-concerns wrapper over this.
pub use boot::run;
