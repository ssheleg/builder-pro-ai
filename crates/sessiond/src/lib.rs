//! bpa-sessiond — Builder Pro AI session daemon (library surface for integration tests).

pub use bpa_protocol as protocol;

pub mod attach;
pub mod live_grid;
pub mod logging;
pub mod osc_parser;
pub mod persistence;
pub mod pty_supervisor;
pub mod scrollback;
pub mod shell_integration;
pub mod singleton;
pub mod socket_server;

mod boot;
/// Test-support hook (D6) so integration tests can assert their `$HOME` isolation actually
/// redirects the daemon's on-disk DB path. See [`boot::app_support_dir_for_test`].
pub use boot::app_support_dir_for_test;
/// Testable daemon boot core (spec §8.1-8.3, §13): bind, wire deps, run `serve` until
/// shutdown, then drain. `main.rs` is a thin process-concerns wrapper over this.
pub use boot::run;
