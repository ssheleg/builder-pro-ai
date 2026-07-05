//! Test-only structured-logging seam (Task 25, spec §13/§16 no-secrets-in-logs evidence).
//!
//! `main.rs` owns the daemon's REAL production logging init (`init_tracing`, private to that
//! binary): it installs a global `tracing_subscriber` writing to
//! `{APP_SUPPORT}/logs/sessiond.tracing.log` exactly once, at process start. That function is not
//! reusable from an integration test binary: `tracing_subscriber::registry()...init()` panics if
//! a global default is already set, and a test has no access to `main.rs`'s private items or to
//! the resolved `{APP_SUPPORT}` path (tests must not read/write the real user's app-support dir).
//!
//! This module gives `crates/sessiond/tests/*` a small, explicit, test-only equivalent: point the
//! SAME kind of `tracing_subscriber::fmt` file layer at an arbitrary temp path, as a **global**
//! default. It is safe to call from an integration test file because each `tests/*.rs` file
//! compiles to its own separate test binary/process — as long as at most one `#[test]` per binary
//! calls `init_to_file`, there is no "already initialized" conflict. This module is not linked
//! into `main.rs` and does not change the real daemon boot's logging behavior in any way.
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};

use tracing_subscriber::prelude::*;

/// Shared handle to the currently-installed test log sink, so [`flush`] can reach it without a
/// second global (tests only ever install one; `OnceLock` refuses a second `init_to_file` in the
/// same process, matching `tracing`'s own single-global-default rule and failing loudly rather
/// than silently double-installing).
static SINK: std::sync::OnceLock<Arc<Mutex<File>>> = std::sync::OnceLock::new();

/// A `Clone`-able writer that appends to the shared, mutex-guarded log [`File`] — the
/// `tracing_subscriber::fmt::MakeWriter` contract requires `Clone + 'static`, and `fmt::Layer`
/// calls `make_writer()` once per emitted event, so cloning must be cheap (it's an `Arc` clone).
#[derive(Clone)]
struct SharedFileWriter(Arc<Mutex<File>>);

impl Write for SharedFileWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.0.lock().unwrap_or_else(|p| p.into_inner()).flush()
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for SharedFileWriter {
    type Writer = SharedFileWriter;
    fn make_writer(&'a self) -> Self::Writer {
        self.clone()
    }
}

/// Install a **global** `tracing` subscriber (this process's only one — see module docs) that
/// writes plain, non-ANSI structured log lines to `path`, and enables `debug`-level output so
/// every `tracing::debug!`/`info!`/`warn!`/`error!` call site the daemon core exercises during the
/// test is captured (production defaults to `info` via `RUST_LOG`; tests want the fuller picture
/// so a leak hiding behind a `debug!` call is not silently missed).
///
/// Returns an error if `path` cannot be created/opened, or if a subscriber was already installed
/// in this process (only one `#[test]` per integration-test binary may call this).
pub fn init_to_file(path: &Path) -> io::Result<()> {
    let file = OpenOptions::new().create(true).append(true).open(path)?;
    let shared = Arc::new(Mutex::new(file));
    SINK.set(shared.clone()).map_err(|_| {
        io::Error::other("logging::init_to_file called more than once in this process")
    })?;

    let writer = SharedFileWriter(shared);
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("debug"));
    tracing_subscriber::registry()
        .with(filter)
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(writer)
                .with_ansi(false),
        )
        .try_init()
        .map_err(io::Error::other)
}

/// Flush the installed test log sink so every event emitted so far is durably on disk before the
/// caller reads the file back. A no-op if [`init_to_file`] was never called.
pub fn flush() {
    if let Some(sink) = SINK.get() {
        let _ = sink.lock().unwrap_or_else(|p| p.into_inner()).flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_file_writer_write_and_flush_do_not_panic_on_a_real_file() {
        // A narrow unit test of the `Write` plumbing itself (not the global-subscriber install,
        // which the integration test `no_secrets_in_logs.rs` exercises for real — installing a
        // second global default in this same `--lib` test binary would conflict with any other
        // test that also calls `init_to_file`/`tracing_subscriber::registry().init()`).
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("w.log");
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .unwrap();
        let mut w = SharedFileWriter(Arc::new(Mutex::new(file)));
        w.write_all(b"hello\n").unwrap();
        w.flush().unwrap();
        let contents = std::fs::read_to_string(&path).unwrap();
        assert_eq!(contents, "hello\n");
    }
}
