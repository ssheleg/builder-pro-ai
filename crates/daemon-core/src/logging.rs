//! Structured logging: production tracing init + a test-only seam (spec §3, §13, §16 no-secrets-
//! in-logs evidence).
//!
//! (a) [`init_tracing`] is the daemon's REAL production logging init, EXTRACTED from
//! `bpa-sessiond::main::init_tracing` (S3 phase 1 extraction; was private to that binary): it
//! installs a global `tracing_subscriber` writing to `{app-support}/logs/<log_file_name>` exactly
//! once, at process start. The log **file name** is parameterized so each daemon built on this
//! crate can pick its own on-disk name while sharing the exact same init logic.
//!
//! (b) [`init_to_file`]/[`flush`] is the test-only seam MOVED as-is from `bpa-sessiond::logging`
//! (it was test-only there too — see its original module docs, preserved on the sessiond
//! wrapper). `tracing_subscriber::registry()...init()` panics if a global default is already set,
//! and a test has no access to a daemon binary's private items or to its resolved app-support
//! path (tests must not read/write the real user's app-support dir), so integration tests point
//! this SAME kind of `tracing_subscriber::fmt` file layer at an arbitrary temp path instead, as a
//! **global** default. Safe to call from an integration test file because each `tests/*.rs` file
//! compiles to its own separate test binary/process — as long as at most one `#[test]` per binary
//! calls `init_to_file`, there is no "already initialized" conflict.
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};

use bpa_protocol::sync::lock;
use tracing_subscriber::prelude::*;

/// Initialize a **global** `tracing` subscriber writing structured logs to
/// `{app-support}/logs/<log_file_name>` (spec §13, §16: no secret values are logged — only
/// paths, session ids, and lifecycle events). Falls back to `EnvFilter`'s default (`info`) when
/// `RUST_LOG` is unset. Log-directory creation/chmod failures are best-effort (logged to stderr,
/// not returned as an error) — matching the pre-extraction sessiond behavior exactly; this
/// function only returns `Err` if the global subscriber itself could not be installed (e.g. one
/// was already set in this process).
pub fn init_tracing(log_file_name: &str) -> io::Result<()> {
    use tracing_subscriber::{fmt, EnvFilter};

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let log_dir = crate::dirs::app_support_dir().join("logs");
    if let Err(e) = std::fs::create_dir_all(&log_dir) {
        eprintln!(
            "bpa-daemon-core: failed to create log dir {}: {e}",
            log_dir.display()
        );
    } else {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = std::fs::set_permissions(&log_dir, std::fs::Permissions::from_mode(0o700));
        }
    }

    let file_appender = tracing_appender::rolling::never(&log_dir, log_file_name);
    // `serve`/`run` also emit to stderr indirectly via launchd's StandardOutPath/StandardErrorPath
    // capture (spec §8.3 plist); the file layer is the daemon's own structured log.
    tracing_subscriber::registry()
        .with(filter)
        .with(fmt::layer().with_writer(file_appender).with_ansi(false))
        .try_init()
        .map_err(io::Error::other)
}

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
        lock(&self.0).write(buf)
    }
    fn flush(&mut self) -> io::Result<()> {
        lock(&self.0).flush()
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
        let _ = lock(sink).flush();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shared_file_writer_write_and_flush_do_not_panic_on_a_real_file() {
        use std::fs::OpenOptions;
        use std::io::Write;
        use std::sync::{Arc, Mutex};

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

    #[test]
    fn init_to_file_twice_in_the_same_process_errors() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("t.log");
        init_to_file(&path).expect("first init_to_file succeeds");
        let err = init_to_file(&path).unwrap_err();
        assert_eq!(
            err.to_string(),
            "logging::init_to_file called more than once in this process"
        );
    }
}
