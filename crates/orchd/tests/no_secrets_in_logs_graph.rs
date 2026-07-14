//! No-secrets-in-logs test for the knowledge-graph node content layer (spec §8: extend the
//! `no_secrets_in_logs` coverage "to plant a marker in a graph node's `label`/`body` and assert it
//! never reaches the tracing log ... or its graph-covering sibling"). Closes `docs/backlog.md`
//! BL-62 — that extension was documented as a gap and deferred out of S4, never actually written.
//!
//! A SIBLING file, not an addition to `no_secrets_in_logs.rs`: `bpa_daemon_core::logging::init_to_file`
//! installs a **global** `tracing` subscriber and refuses a second call in the same process (see
//! that module's doc + its own `init_to_file_twice_in_the_same_process_errors` test), and
//! `cargo test` runs every `#[test]` fn inside ONE integration-test FILE in the SAME process (only
//! separate `tests/*.rs` FILES get separate processes) — so a second `#[test]` here reusing
//! `planted_ruleset_secrets_never_appear_in_logs`'s file would collide on that single-global-init
//! rule. This file mirrors that test's shape exactly (log-sink setup, hermetic HOME/app-support
//! repointing, the flush-then-read-then-assert-absent pattern) in its own process instead.
//!
//! `Db::add_node`/`update_node`/`add_edge`/`delete_node` were `pub(crate)` in `crates/orchd/src/
//! graph.rs` before this task; they are bumped to `pub` (this task, see each method's doc note in
//! `graph.rs`) so this integration test — compiled as a separate crate, like every `tests/*.rs`
//! file — can drive them directly, exactly like `planted_ruleset_secrets_never_appear_in_logs`
//! drives `Db::upsert_ruleset`/`get_ruleset`/`acknowledge_rule_file`. The bump is not a new
//! security exposure: `socket_server.rs`'s dispatch table has exposed the identical behavior over
//! the Unix socket (`GraphAddNode`/`GraphUpdateNode`/`GraphAddEdge`/`GraphDeleteNode`) since the
//! T10 dispatch-wiring task, so the crate-private restriction was never a real boundary — it just
//! happened to block this test from driving the Db layer directly.

use std::fs;
use std::io::Read;
use std::path::Path;

use bpa_orchd::persistence::{Db, OrchdPersistError};
use bpa_orchd::protocol::{GraphEdgeKind, GraphNodeKind};

// `Db::create_project` computes (but, called directly like this, never WRITES) a ruleset
// `md_path` string via `bpa_daemon_core::dirs::app_support_dir()` (reads process-global `$HOME`)
// for the row it inserts — mirrors `boot_integration.rs`'s/`dispatch_integration.rs`'s HOME_LOCK/
// HomeGuard pattern byte-for-byte as defense-in-depth, so this test's DB path string never
// resolves under the real `~/Library/Application Support/ai.builderpro.desktop` tree even though
// nothing in this test's call sequence actually performs I/O against that path.
static HOME_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

struct HomeGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    prior: Option<std::ffi::OsString>,
}

impl HomeGuard {
    fn set(dir: &Path) -> Self {
        let lock = HOME_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        let prior = std::env::var_os("HOME");
        std::env::set_var("HOME", dir);
        HomeGuard { _lock: lock, prior }
    }
}

impl Drop for HomeGuard {
    fn drop(&mut self) {
        match self.prior.take() {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }
    }
}

/// Knowledge-graph node content (`label`/`body`) must never leak into structured logs (spec §8).
/// We plant TWO distinct secret markers — one in a node's `label`, one in its `body` — point a
/// test-only log sink at a temp file (mirrors `planted_ruleset_secrets_never_appear_in_logs`
/// byte-for-byte in technique), then drive enough of the real graph mutation surface that any
/// path logging node content would be caught: `create_project` → `add_node` (INSERT carrying both
/// secrets) → `update_node` (an UPDATE re-planting the body secret into the label field too,
/// exercising a different SQL statement) → `add_edge` (a normal edge from the secret-carrying
/// node) → a duplicate `add_edge` (`Conflict` error path) → a self-loop `add_edge` on the SAME
/// secret-carrying node (`Invariant` error path — error paths are where content most often leaks
/// into a formatted log/error message) → `delete_node` (DELETE, cascading the edge). Finally,
/// flush the sink, read the log file back, and assert NEITHER marker appears anywhere in it.
#[test]
fn planted_graph_node_secrets_never_appear_in_logs() {
    let secret_label = "s3cr3t-GRAPH-LABEL-must-not-leak-4b6d1a";
    let secret_body = "s3cr3t-GRAPH-BODY-must-not-leak-9e2af7";

    // Single test in this file/binary (see module doc above) — no cross-test global-subscriber
    // conflict, mirroring `no_secrets_in_logs.rs`'s own reasoning for the ruleset test.
    let tmp = tempfile::tempdir().expect("tempdir");
    let log_path = tmp.path().join("orchd-graph.test.log");
    let db_path = tmp.path().join("orchd-graph.db");

    bpa_daemon_core::logging::init_to_file(&log_path).expect("init logging");

    // HERMETICITY: repoint `$HOME` under this test's own tempdir before anything reads it (see
    // the `HomeGuard` doc above) — held for the rest of the test via this binding's scope.
    let _home = HomeGuard::set(tmp.path());

    // Real on-disk DB (not in-memory) under this test's own tempdir. `Db::open` runs migrations up
    // to the current schema version (v2, graph tables included) automatically — no separate
    // migration step needed, exactly like every other production/test boot path.
    let db = Db::open(&db_path).expect("open db");
    let project = db
        .create_project("Acme Graph", "desc", &["graph-w1".to_string()])
        .expect("create project");

    // Plant BOTH secrets on one node: label carries secret_label, body carries secret_body. Real
    // INSERT over real bytes on a real on-disk DB file under `tmp` — HERMETICITY: `db_path` is
    // under this test's own tempdir, never the real app-support tree, so the secret content only
    // ever lives under this test's tempdir, deleted on drop.
    let node = db
        .add_node(
            &project.id,
            GraphNodeKind::Concept,
            secret_label,
            secret_body,
            0.0,
            0.0,
        )
        .expect("add_node with planted label+body");
    assert_eq!(
        node.label, secret_label,
        "sanity: the write actually landed"
    );
    assert_eq!(node.body, secret_body, "sanity: the write actually landed");

    // update_node: re-plant secret_body into the LABEL field too (a different node than the one
    // that already carries it in body) — a real UPDATE statement, a different SQL path than the
    // INSERT above, also carrying live secret content.
    let updated = db
        .update_node(&node.id, Some(secret_body), None)
        .expect("update_node changing label to the other secret");
    assert_eq!(
        updated.label, secret_body,
        "sanity: update_node actually re-read the new label, not the stale row"
    );

    // A second, non-secret node to hang edges off of.
    let other = db
        .add_node(
            &project.id,
            GraphNodeKind::Concept,
            "other node",
            "",
            1.0,
            1.0,
        )
        .expect("add second node");

    let edge = db
        .add_edge(&node.id, &other.id, GraphEdgeKind::Relates, "")
        .expect("add_edge from the secret-carrying node");
    assert_eq!(
        edge.source_node_id, node.id,
        "sanity: the edge really originates at the secret-carrying node"
    );

    // ERROR PATH 1: duplicate (source, target, kind) ⇒ Conflict.
    let dup_err = db
        .add_edge(&node.id, &other.id, GraphEdgeKind::Relates, "")
        .expect_err("duplicate edge must be rejected");
    assert!(
        matches!(dup_err, OrchdPersistError::Conflict(_)),
        "sanity: the duplicate really hit the Conflict path, not something else: {dup_err}"
    );

    // ERROR PATH 2: self-loop ⇒ Invariant, using the SAME secret-carrying node as both endpoints
    // — error paths are where content most often leaks into a formatted log/error message.
    let self_loop_err = db
        .add_edge(&node.id, &node.id, GraphEdgeKind::Relates, "")
        .expect_err("self-loop must be rejected");
    assert!(
        matches!(self_loop_err, OrchdPersistError::Invariant(_)),
        "sanity: the self-loop really hit the Invariant path, not something else: {self_loop_err}"
    );

    // delete_node cascades the incident edge created above (FK `ON DELETE CASCADE`, D4) —
    // exercises the DELETE path over the secret-carrying row.
    db.delete_node(&node.id)
        .expect("delete the secret-carrying node");

    bpa_daemon_core::logging::flush();
    let mut contents = String::new();
    fs::File::open(&log_path)
        .expect("open log")
        .read_to_string(&mut contents)
        .expect("read log");

    // Sanity: logging actually produced output (guards against a vacuous pass). `Db::open`
    // unconditionally logs an `info!` line on a successful open, same as the ruleset test relies
    // on.
    assert!(
        !contents.trim().is_empty(),
        "log sink was empty — test would pass vacuously"
    );

    assert!(
        !contents.contains(secret_label),
        "planted graph node LABEL content leaked into logs:\n{contents}"
    );
    assert!(
        !contents.contains(secret_body),
        "planted graph node BODY content leaked into logs:\n{contents}"
    );
}
