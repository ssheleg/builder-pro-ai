//! Per-request completion-tracing evidence (spec D4, O-6) + no-secrets discipline for the new
//! `socket_server::dispatch` choke-point.
//!
//! Sibling of `no_secrets_in_logs.rs` (same `no_secrets_in_logs*` family the P1-T5 plan names).
//! Where that file drives the ruleset persistence layer directly, this one boots the REAL daemon
//! (`bpa_orchd::run`, the same seam `dispatch_integration.rs` uses) over a Unix-domain socket and
//! drives real `OrchdRequest`s so the ONE completion-trace line `socket_server::dispatch` now
//! emits per request is exercised on the production path. It asserts the three D4 guarantees:
//!   (a) a successful mutating verb logs `verb="<Verb>" outcome="ok"` (+ `elapsed_ms`),
//!   (b) a failing verb logs `outcome="err" error_code=<OrchdErrorCode>` (+ the verb + elapsed),
//!   (c) a request carrying a secret-ish body (`UpsertRuleSet.md_content`) leaves that value
//!       absent from every log line — the completion trace records verb/outcome/code/elapsed
//!       only, never args/bodies/tokens/PII.
//!
//! Distinct binary from `no_secrets_in_logs.rs`, so its own process ⇒ its own single
//! `logging::init_to_file` (that seam refuses a second install per process — see its docs).

use std::path::Path;
use std::time::Duration;

use bpa_orchd::protocol::{
    encode_orchd_frame, OrchdErrorCode, OrchdFrame, OrchdFrameDecoder, OrchdRequest, OrchdResponse,
    Project, RuleScope, ORCHD_CLIENT_MAX_VERSION, ORCHD_CLIENT_MIN_VERSION,
};
use bpa_protocol::{decode_daemon_reply, encode_client_preamble, ClientPreamble, DaemonReply};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;
use tokio::sync::watch;

async fn send_frame(s: &mut UnixStream, f: &OrchdFrame) {
    let bytes = encode_orchd_frame(f).unwrap();
    s.write_all(&bytes).await.unwrap();
    s.flush().await.unwrap();
}

async fn recv_frame(s: &mut UnixStream) -> OrchdFrame {
    let mut lenb = [0u8; 4];
    s.read_exact(&mut lenb).await.unwrap();
    let len = u32::from_le_bytes(lenb) as usize;
    let mut body = vec![0u8; len];
    s.read_exact(&mut body).await.unwrap();
    let mut decoder = OrchdFrameDecoder::new();
    decoder.push(&lenb);
    decoder.push(&body);
    let mut frames = decoder.decode().unwrap();
    frames.remove(0)
}

async fn preamble_handshake(s: &mut UnixStream) {
    let bytes = encode_client_preamble(&ClientPreamble {
        min: ORCHD_CLIENT_MIN_VERSION,
        max: ORCHD_CLIENT_MAX_VERSION,
        build: "test".into(),
    });
    s.write_all(&bytes).await.unwrap();
    s.flush().await.unwrap();

    let mut header = [0u8; 9];
    s.read_exact(&mut header).await.unwrap();
    let mut buf = header.to_vec();
    if header[4] == 1 {
        let build_len = u16::from_le_bytes(header[7..9].try_into().unwrap()) as usize;
        let mut build = vec![0u8; build_len];
        s.read_exact(&mut build).await.unwrap();
        buf.extend_from_slice(&build);
    }
    match decode_daemon_reply(&buf).expect("valid daemon reply") {
        DaemonReply::Accepted { .. } => {}
        other => panic!("expected Accepted, got {other:?}"),
    }
}

/// One request/one correlated response, skipping any interleaved push (a mutating verb's own
/// connection sees its own coarse push before the reply — mirrors `dispatch_integration.rs`).
async fn request(s: &mut UnixStream, id: u64, req: OrchdRequest) -> OrchdResponse {
    send_frame(s, &OrchdFrame::Request { id, req }).await;
    loop {
        match recv_frame(s).await {
            OrchdFrame::Response { id: rid, res } => {
                assert_eq!(rid, id, "response id must correlate with the request id");
                return res;
            }
            OrchdFrame::Push(_) => continue,
            other => panic!("expected a Response or Push frame, got {other:?}"),
        }
    }
}

async fn connect_when_ready(socket: &Path) -> UnixStream {
    for _ in 0..100 {
        if socket.exists() {
            if let Ok(c) = UnixStream::connect(socket).await {
                return c;
            }
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("daemon did not bind socket in time");
}

/// True when SOME single log line contains every one of `needles` — robust to `tracing`'s field
/// ordering within a line (each completion trace is exactly one line).
fn has_line_with(contents: &str, needles: &[&str]) -> bool {
    contents
        .lines()
        .any(|l| needles.iter().all(|n| l.contains(n)))
}

/// Boots the real daemon, drives one ok mutating verb, one secret-carrying mutating verb, and one
/// failing verb, then asserts the D4 completion-trace fields are present and the planted secret is
/// not. `$HOME` is isolated so `app_support_dir()` (re-read per request) resolves under this
/// test's tempdir, never the real user's app-support tree.
#[tokio::test]
async fn dispatch_emits_completion_trace_without_leaking_secrets() {
    let secret = "s3cr3t-RULESET-BODY-must-not-reach-any-log-line-4b7e";

    let home = tempfile::tempdir().expect("home tempdir");
    let prior_home = std::env::var_os("HOME");
    std::env::set_var("HOME", home.path());

    let tmp = tempfile::tempdir().expect("tempdir");
    let log_path = tmp.path().join("orchd.test.log");
    let socket = tmp.path().join("orchd.sock");
    let rules_path = tmp.path().join("project-rules.md");

    bpa_daemon_core::logging::init_to_file(&log_path).expect("init logging");

    let (shutdown_tx, shutdown_rx) = watch::channel(false);
    let socket_for_task = socket.clone();
    let boot =
        tokio::spawn(
            async move { bpa_orchd::run(socket_for_task, shutdown_tx, shutdown_rx).await },
        );

    let mut c = connect_when_ready(&socket).await;
    preamble_handshake(&mut c).await;

    // (a) A successful mutating verb.
    let project = match request(
        &mut c,
        1,
        OrchdRequest::CreateProject {
            name: "Acme".into(),
            description: "desc".into(),
            workspace_ids: vec!["w1".into()],
        },
    )
    .await
    {
        OrchdResponse::Project(p) => p,
        other => panic!("expected Project, got {other:?}"),
    };
    let _: &Project = &project;

    // (c) A mutating verb whose body carries a secret — the completion trace must not echo it, and
    // no other log line on the dispatch path may either.
    match request(
        &mut c,
        2,
        OrchdRequest::UpsertRuleSet {
            scope: RuleScope::Project,
            project_id: Some(project.id.clone()),
            md_content: Some(secret.into()),
            md_path: Some(rules_path.to_str().unwrap().into()),
            policy: None,
        },
    )
    .await
    {
        OrchdResponse::RuleSetView(_) => {}
        other => panic!("expected RuleSetView, got {other:?}"),
    }

    // (b) A failing verb — an unknown project id ⇒ `OrchdErrorCode::NotFound`.
    match request(
        &mut c,
        3,
        OrchdRequest::ArchiveProject {
            id: "does-not-exist".into(),
        },
    )
    .await
    {
        OrchdResponse::Error {
            code: OrchdErrorCode::NotFound,
            ..
        } => {}
        other => panic!("expected Error{{NotFound}}, got {other:?}"),
    }

    // Clean shutdown so the daemon task drains and returns.
    match request(&mut c, 4, OrchdRequest::OrchdShutdown { drain: false }).await {
        OrchdResponse::Ack => {}
        other => panic!("expected Ack, got {other:?}"),
    }
    let _ = tokio::time::timeout(Duration::from_secs(5), boot).await;

    bpa_daemon_core::logging::flush();
    let contents = std::fs::read_to_string(&log_path).expect("read log");

    // Sanity: the sink is non-empty, so the assertions below are meaningful (not vacuously true).
    assert!(
        !contents.trim().is_empty(),
        "log sink was empty — test would pass vacuously"
    );

    // (a) mutating verb ⇒ verb + outcome=ok + elapsed_ms on one line.
    assert!(
        has_line_with(
            &contents,
            &[r#"verb="CreateProject""#, r#"outcome="ok""#, "elapsed_ms="],
        ),
        "expected a completion trace for CreateProject with outcome=ok:\n{contents}"
    );
    assert!(
        has_line_with(&contents, &[r#"verb="UpsertRuleSet""#, r#"outcome="ok""#],),
        "expected a completion trace for UpsertRuleSet with outcome=ok:\n{contents}"
    );

    // (b) failing verb ⇒ outcome=err + error_code on one line, still naming the verb.
    assert!(
        has_line_with(
            &contents,
            &[
                r#"verb="ArchiveProject""#,
                r#"outcome="err""#,
                "error_code=NotFound",
            ],
        ),
        "expected a completion trace for ArchiveProject with outcome=err error_code=NotFound:\n{contents}"
    );

    // (c) the secret body is nowhere in the log — not in the completion trace, not anywhere else.
    assert!(
        !contents.contains(secret),
        "planted ruleset body leaked into logs:\n{contents}"
    );

    match prior_home {
        Some(v) => std::env::set_var("HOME", v),
        None => std::env::remove_var("HOME"),
    }
}
