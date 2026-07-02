//! bpa-sessiond — Builder Pro AI session daemon (library surface for integration tests).
//! S0 skeleton: re-export the shared protocol crate. Modules land in Task 4–Task 13.

pub use bpa_protocol as protocol;

pub mod live_grid;
pub mod osc_parser;
pub mod persistence;
pub mod scrollback;
// TEMP (review-only, will revert): pub mod shell_integration;
pub mod singleton;
