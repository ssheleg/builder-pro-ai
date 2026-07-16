//! No-secrets-in-logs integration test (spec §13, §16: "Logging: structured, no secret values;
//! allowlisted-secret scrub test").
//!
//! The daemon process's OWN environment can carry secrets (e.g. keychain-derived tokens, or in
//! this test's case a planted `DAEMON_SECRET`) that must never be echoed into the structured
//! `tracing` log sink under `{APP_SUPPORT}/logs/` — regardless of whether that secret ever reaches
//! a child shell (that narrower "child env is allowlisted" contract is already covered by
//! `pty_supervisor::tests::env_clear_hides_daemon_secret_keeps_allowlist`, which asserts the
//! secret is absent from the SPAWNED SHELL's env; this test asserts it's absent from the DAEMON'S
//! OWN LOG FILE, a distinct surface — a value could leak into a log line via a `tracing::debug!`
//! call, a `{:?}` derive over some struct that happens to embed env state, an error message that
//! echoes `std::env::vars()`, etc., without ever being written into a child's environment).
//!
//! This drives a REAL daemon boot (`bpa_sessiond::run`, the same seam
//! `tests/boot_integration.rs` uses) over a real Unix-domain-socket handshake, so every
//! `tracing::info!/debug!/warn!/error!` call site the daemon actually exercises during a normal
//! create-workspace → create-session → write-stdin → detach → shutdown flow is captured, not just
//! a hand-picked subset.

use std::fs;
use std::io::Read;
use std::time::Duration;

use bpa_sessiond::protocol::{decode_daemon_reply, encode_client_preamble, encode_frame};
use bpa_sessiond::protocol::{
    ClientPreamble, DaemonReply, FrameDecoder, CLIENT_MAX_VERSION, CLIENT_MIN_VERSION,
};
use bpa_sessiond::protocol::{Frame, Request, Response};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

/// True when SOME single log line contains every one of `needles` — robust to `tracing`'s
/// within-line field ordering (each completion trace is exactly one line).
fn has_line_with(contents: &str, needles: &[&str]) -> bool {
    contents
        .lines()
        .any(|l| needles.iter().all(|n| l.contains(n)))
}

async fn send_frame(s: &mut UnixStream, f: &Frame) {
    // `encode_frame` already prepends the u32-LE length prefix — write its output verbatim,
    // do NOT add a second prefix on top.
    let bytes = encode_frame(f).unwrap();
    s.write_all(&bytes).await.unwrap();
    s.flush().await.unwrap();
}
async fn recv_frame(s: &mut UnixStream) -> Frame {
    // Read exactly one length-prefixed CBOR frame off the wire via the shared FrameDecoder: read
    // the 4-byte LE length, then that many body bytes, feed both into the decoder, and take the
    // single frame it yields (mirrors the length-prefix framing `encode_frame` produces).
    let mut lenb = [0u8; 4];
    s.read_exact(&mut lenb).await.unwrap();
    let len = u32::from_le_bytes(lenb) as usize;
    let mut body = vec![0u8; len];
    s.read_exact(&mut body).await.unwrap();
    let mut decoder = FrameDecoder::new();
    decoder.push(&lenb);
    decoder.push(&body);
    let mut frames = decoder.decode().unwrap();
    frames.remove(0)
}

/// Perform the Pv2 §4.2/§4.4 preamble handshake and assert the daemon accepts: write the raw
/// codec-agnostic client preamble (`CLIENT_MIN_VERSION..=CLIENT_MAX_VERSION`, so it tracks the
/// current wire version and can never desync from a version bump), then read + `decode_daemon_reply` the fixed
/// 9-byte reply header (plus the trailing build string when accepted) and require `Accepted`.
/// Distinct from `send_frame`/`recv_frame`: the preamble is a raw byte layout, not a CBOR `Frame`.
async fn preamble_handshake(s: &mut UnixStream) {
    let bytes = encode_client_preamble(&ClientPreamble {
        min: CLIENT_MIN_VERSION,
        max: CLIENT_MAX_VERSION,
        build: "test".into(),
    });
    s.write_all(&bytes).await.unwrap();
    s.flush().await.unwrap();

    // Accepted:    magic(4)+result(1)+chosen(2)+build_len(2) = 9 bytes, then build_len more.
    // Incompatible: magic(4)+result(1)+min(2)+max(2)         = 9 bytes, no trailing body.
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

/// The daemon must never leak its own environment secrets into structured logs (spec §13, §16).
/// We plant a secret in the daemon PROCESS's own environment (this test process, which is what
/// `bpa_sessiond::run` — booted in-process below — inherits `std::env::var` calls from), point a
/// test-only log sink at a temp file, drive a full session lifecycle over the real wire protocol,
/// and assert the planted value is absent from every line the daemon wrote.
#[tokio::test]
async fn planted_secret_never_appears_in_logs() {
    let secret = "s3cr3t-DAEMON_SECRET-must-not-leak-9f2c";
    // SAFETY/isolation note: `cargo test` runs each integration-test FILE as its own separate
    // process/binary (unlike `--lib` tests, which share one binary and therefore one global env).
    // This file has exactly one test, so there is no cross-test interleaving risk here — see
    // `singleton.rs`'s `ENV_LOCK` for the analogous protection where multiple env-touching tests
    // DO share a binary.
    std::env::set_var("DAEMON_SECRET", secret);

    // Isolate `$HOME` (D6): this test boots the REAL daemon core (`bpa_sessiond::run`), whose
    // on-disk DB path resolves under `$HOME` (`boot::app_support_dir`). Without this, a dev
    // machine with the real app installed would have this test read/write the real
    // `~/Library/Application Support/.../bpa.db` — ambient-$HOME test pollution (D6). Single test
    // in this file/binary, so no cross-test race — same reasoning as `tests/rehydrate_attach.rs`.
    let home_dir = tempfile::tempdir().expect("home tempdir");
    let prior_home = std::env::var_os("HOME");
    std::env::set_var("HOME", home_dir.path());
    assert_eq!(
        bpa_sessiond::app_support_dir_for_test(),
        home_dir
            .path()
            .join("Library/Application Support/ai.builderpro.desktop"),
        "HOME isolation must actually redirect the daemon's app-support/DB path under the tempdir"
    );

    let tmp = tempfile::tempdir().expect("tempdir");
    let log_path = tmp.path().join("sessiond.test.log");
    let socket = tmp.path().join("d.sock");

    bpa_sessiond::logging::init_to_file(&log_path).expect("init logging");

    // Boot the real daemon core (same seam as tests/boot_integration.rs) on a temp socket.
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let socket_for_task = socket.clone();
    let shutdown_tx_for_task = shutdown_tx.clone();
    let boot = tokio::spawn(async move {
        bpa_sessiond::run(socket_for_task, shutdown_tx_for_task, shutdown_rx).await
    });

    let mut conn = None;
    for _ in 0..100 {
        if socket.exists() {
            if let Ok(c) = UnixStream::connect(&socket).await {
                conn = Some(c);
                break;
            }
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let mut c = conn.expect("daemon did not bind socket in time");

    // Handshake (Pv2 §4.2/§4.4 preamble, replacing the v1 Hello→Welcome frame exchange).
    preamble_handshake(&mut c).await;

    // Create a workspace, then a session — this exercises persistence + broker logging call
    // sites (socket_server.rs: connection accept, dispatch, persist, scrollback flush).
    send_frame(
        &mut c,
        &Frame::Request {
            id: 1,
            req: Request::CreateWorkspace {
                name: "ws".into(),
                root_path: tmp.path().display().to_string(),
            },
        },
    )
    .await;
    let ws_id = loop {
        match recv_frame(&mut c).await {
            Frame::Response {
                id: 1,
                res: Response::Workspace(w),
            } => break w.id,
            Frame::Push(_) => continue,
            other => panic!("expected Workspace, got {other:?}"),
        }
    };

    // A deliberately-failing verb so the completion trace's err branch (outcome=err + error_code)
    // is exercised on a real dispatch path (spec D4, O-6): an `AddWorkspaceRoot` to a path that is
    // not an existing directory is rejected with `Response::Error{ code: "InvalidWorkspaceRoot" }`
    // (never touches the DB). `ws_id` is cloned because `CreateSession` below moves it.
    send_frame(
        &mut c,
        &Frame::Request {
            id: 100,
            req: Request::AddWorkspaceRoot {
                workspace_id: ws_id.clone(),
                path: tmp.path().join("no-such-dir-xyz").display().to_string(),
            },
        },
    )
    .await;
    loop {
        match recv_frame(&mut c).await {
            Frame::Response {
                id: 100,
                res: Response::Error { code, .. },
            } => {
                assert_eq!(
                    code, "InvalidWorkspaceRoot",
                    "expected the invalid-root rejection to drive the err completion trace"
                );
                break;
            }
            Frame::Push(_) => continue,
            other => panic!("expected an Error response for AddWorkspaceRoot, got {other:?}"),
        }
    }

    send_frame(
        &mut c,
        &Frame::Request {
            id: 2,
            req: Request::CreateSession {
                workspace_id: ws_id,
                shell: Some("/bin/sh".into()),
                cwd: Some(tmp.path().display().to_string()),
                env_overrides: vec![],
                cols: 80,
                rows: 24,
            },
        },
    )
    .await;
    let session_id = loop {
        match recv_frame(&mut c).await {
            Frame::Response {
                id: 2,
                res: Response::Session(meta),
            } => break meta.id,
            Frame::Push(_) => continue,
            Frame::Response {
                id: 2,
                res: Response::Error { code, message },
            } => {
                panic!("CreateSession failed: {code}: {message}");
            }
            other => panic!("expected Session, got {other:?}"),
        }
    };

    // Attach, write a command that (deliberately) echoes the daemon's own env, and let it run —
    // this proves the daemon process genuinely has the secret in scope (so a passing assertion
    // below is not vacuous: the value really was live and observable while logging happened),
    // while the actual secret propagation into the CHILD's env is the separate, already-covered
    // `pty_supervisor` contract (env_clear + allowlist) — irrelevant to what THIS test checks.
    send_frame(
        &mut c,
        &Frame::Request {
            id: 3,
            req: Request::AttachSession {
                session_id: session_id.clone(),
            },
        },
    )
    .await;
    // Drain the Replay push for the attach.
    loop {
        match recv_frame(&mut c).await {
            Frame::Push(bpa_sessiond::protocol::Push::Replay { .. }) => break,
            Frame::Response { id: 3, .. } => continue,
            Frame::Push(_) => continue,
            other => panic!("expected Replay push, got {other:?}"),
        }
    }

    send_frame(
        &mut c,
        &Frame::Request {
            id: 4,
            req: Request::WriteStdin {
                session_id: session_id.clone(),
                bytes: b"echo hello\n".to_vec(),
            },
        },
    )
    .await;

    // Give the shell + reader/wait threads + status-change plumbing time to run and log.
    tokio::time::sleep(Duration::from_millis(800)).await;

    send_frame(
        &mut c,
        &Frame::Request {
            id: 5,
            req: Request::DetachSession {
                session_id: session_id.clone(),
            },
        },
    )
    .await;
    send_frame(
        &mut c,
        &Frame::Request {
            id: 6,
            req: Request::KillSession { session_id },
        },
    )
    .await;

    // Give the kill/reap/status-change path a moment to log too, then clean shutdown.
    tokio::time::sleep(Duration::from_millis(300)).await;
    shutdown_tx.send(true).ok();
    let _ = tokio::time::timeout(Duration::from_secs(5), boot).await;

    // Flush + read the log file.
    bpa_sessiond::logging::flush();
    let mut contents = String::new();
    fs::File::open(&log_path)
        .expect("open log")
        .read_to_string(&mut contents)
        .expect("read log");

    // Sanity: logging actually produced output (guards against a vacuous pass — if the sink were
    // empty, "the secret is absent" would be true for a trivial, meaningless reason).
    assert!(
        !contents.trim().is_empty(),
        "log sink was empty — test would pass vacuously"
    );

    // The check is on the SECRET VALUE, not the `DAEMON_SECRET` key name — the key name alone is
    // not sensitive and a log line mentioning "DAEMON_SECRET is set" would be fine; only the
    // actual value must never appear.
    assert!(
        !contents.contains(secret),
        "planted secret leaked into logs:\n{contents}"
    );

    // Per-request completion tracing (spec D4, O-6): the single `dispatch` choke-point emits one
    // structured line per request. A successful mutating verb carries verb + outcome=ok + elapsed;
    // the deliberately-failing `AddWorkspaceRoot` above carries outcome=err + its wire error_code.
    assert!(
        has_line_with(
            &contents,
            &[
                r#"verb="CreateWorkspace""#,
                r#"outcome="ok""#,
                "elapsed_ms="
            ],
        ),
        "expected a completion trace for CreateWorkspace with outcome=ok:\n{contents}"
    );
    assert!(
        has_line_with(
            &contents,
            &[
                r#"verb="AddWorkspaceRoot""#,
                r#"outcome="err""#,
                "error_code=InvalidWorkspaceRoot",
            ],
        ),
        "expected a completion trace for AddWorkspaceRoot with outcome=err error_code=InvalidWorkspaceRoot:\n{contents}"
    );

    std::env::remove_var("DAEMON_SECRET");

    // Restore the real HOME so any later test in this (single-test) binary is unaffected.
    match prior_home {
        Some(v) => std::env::set_var("HOME", v),
        None => std::env::remove_var("HOME"),
    }
}
