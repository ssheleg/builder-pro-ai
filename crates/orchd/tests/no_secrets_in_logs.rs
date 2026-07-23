//! No-secrets-in-logs test for the RuleSet file layer (spec §5: "Rules md content is never logged
//! (no-secrets discipline; enforced by an orchd `no_secrets_in_logs`-style test)"; task-8 brief's
//! second deferred-from-T5 item). Mirrors `crates/sessiond/tests/no_secrets_in_logs.rs`'s shape
//! and intent (plant a secret, drive real code against a real tracing log sink, assert it's
//! absent) but does NOT need a full daemon boot over the wire: `OrchdRequest::UpsertRuleSet` /
//! `GetRuleSet` / `AcknowledgeRuleFile` dispatch wiring lands in a later task (T10 — see
//! `socket_server.rs`'s dispatch doc, which stubs every non-Ping/Shutdown verb today). The surface
//! THIS task actually adds is `persistence::Db`'s ruleset methods plus `ruleset_files`, and
//! driving those directly already exercises every place ruleset markdown content flows through
//! real orchd code today: atomic write (`upsert_ruleset` → `ruleset_files::write_atomic`),
//! read-back (`get_ruleset`), and a re-read after an external hand-edit
//! (`acknowledge_rule_file`).

use std::fs;
use std::io::Read;

use bpa_orchd::persistence::Db;
use bpa_orchd::protocol::{PolicyRules, RuleScope};

/// The daemon's ruleset markdown content must never leak into structured logs (spec §5, §7: "File
/// content is never logged"). We plant TWO distinct secret markers — one written via
/// `upsert_ruleset` (an atomic `write_atomic` call over real bytes on a real file), one written by
/// simulating an external hand-edit that `acknowledge_rule_file` then re-reads — point a
/// test-only log sink at a temp file, drive both through real `persistence::Db` methods, and
/// assert neither planted value is anywhere in what the daemon wrote to its log.
#[test]
fn planted_ruleset_secrets_never_appear_in_logs() {
    let secret_v1 = "s3cr3t-RULESET-CONTENT-v1-must-not-leak-7a1f";
    let secret_v2 = "s3cr3t-RULESET-CONTENT-v2-hand-edited-must-not-leak-c92e";

    // Single test in this file/binary (like sessiond's version) — `cargo test` runs each
    // integration-test FILE as its own process, so there is no cross-test global-subscriber
    // conflict here (see `bpa_daemon_core::logging::init_to_file`'s doc: at most one call per
    // process).
    let tmp = tempfile::tempdir().expect("tempdir");
    let log_path = tmp.path().join("orchd.test.log");
    let db_path = tmp.path().join("orchd.db");

    bpa_daemon_core::logging::init_to_file(&log_path).expect("init logging");

    // Real on-disk DB (not in-memory) + real files under the SAME tempdir, so this exercises the
    // exact `Db::open` → `create_project` → `upsert_ruleset` → `get_ruleset` →
    // `acknowledge_rule_file` path a production boot would drive, all while the secret content is
    // genuinely live and flowing through real code (not a vacuous "nothing ran" pass).
    let db = Db::open(&db_path).expect("open db");
    let project = db
        .create_project("Acme", "desc", &["w1".to_string()])
        .expect("create project");

    // HERMETICITY: a project row's default md_path resolves under the REAL `app_support_dir()`
    // tree — writing the SECRET marker there with `md_path: None` would drop it into the
    // production app-support dir (a real secret-to-disk leak, ironic for this very test). Repoint
    // to a path under `tmp` so the secret content only ever lives under this test's own tempdir,
    // deleted on drop.
    let rules_path = tmp.path().join("project-rules.md");

    let written = db
        .upsert_ruleset(
            RuleScope::Project,
            Some(&project.id),
            Some(secret_v1),
            Some(rules_path.to_str().unwrap()),
            Some(&PolicyRules {
                spend_cap_usd: Some(1.0),
                approval_classes: vec!["deploy".to_string()],
                path_allowlist: vec!["/tmp".to_string()],
                supervisor: Default::default(),
            }),
        )
        .expect("upsert ruleset with secret content");

    let fetched = db
        .get_ruleset(RuleScope::Project, Some(&project.id))
        .expect("get ruleset");
    assert_eq!(
        fetched.md_hash, written.md_hash,
        "sanity: the write actually landed and GetRuleSet reads the same row back"
    );

    // Simulate an external hand-edit with a SECOND, distinct secret, then Acknowledge — which
    // re-reads the file's full content off disk.
    std::fs::write(&written.md_path, secret_v2).expect("hand-edit rules file");
    let acknowledged = db
        .acknowledge_rule_file(&written.id)
        .expect("acknowledge rule file");
    assert_ne!(
        acknowledged.md_hash, written.md_hash,
        "sanity: acknowledge actually re-read the hand-edited content, not the stale row"
    );

    bpa_daemon_core::logging::flush();
    let mut contents = String::new();
    fs::File::open(&log_path)
        .expect("open log")
        .read_to_string(&mut contents)
        .expect("read log");

    // Sanity: logging actually produced output (guards against a vacuous pass — if the sink were
    // empty, "the secret is absent" would be true for a trivial, meaningless reason). `Db::open`
    // unconditionally logs an `info!` line on a successful open.
    assert!(
        !contents.trim().is_empty(),
        "log sink was empty — test would pass vacuously"
    );

    assert!(
        !contents.contains(secret_v1),
        "planted ruleset content (v1, via upsert_ruleset) leaked into logs:\n{contents}"
    );
    assert!(
        !contents.contains(secret_v2),
        "planted ruleset content (v2, hand-edited + acknowledged) leaked into logs:\n{contents}"
    );
}
