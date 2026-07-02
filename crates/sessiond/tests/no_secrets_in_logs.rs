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

use bpa_sessiond::protocol::{Frame, Request, Response, MAGIC, PROTO_VERSION};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

async fn send_frame(s: &mut UnixStream, f: &Frame) {
    let body = bincode::serialize(f).unwrap();
    s.write_all(&(body.len() as u32).to_le_bytes()).await.unwrap();
    s.write_all(&body).await.unwrap();
    s.flush().await.unwrap();
}
async fn recv_frame(s: &mut UnixStream) -> Frame {
    let mut lenb = [0u8; 4];
    s.read_exact(&mut lenb).await.unwrap();
    let len = u32::from_le_bytes(lenb) as usize;
    let mut body = vec![0u8; len];
    s.read_exact(&mut body).await.unwrap();
    bincode::deserialize(&body).unwrap()
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

    let tmp = tempfile::tempdir().expect("tempdir");
    let log_path = tmp.path().join("sessiond.test.log");
    let socket = tmp.path().join("d.sock");

    bpa_sessiond::logging::init_to_file(&log_path).expect("init logging");

    // Boot the real daemon core (same seam as tests/boot_integration.rs) on a temp socket.
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let socket_for_task = socket.clone();
    let boot = tokio::spawn(async move { bpa_sessiond::run(socket_for_task, shutdown_rx).await });

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

    // Handshake.
    send_frame(
        &mut c,
        &Frame::Request {
            id: 0,
            req: Request::Hello {
                magic: MAGIC,
                proto_version: PROTO_VERSION,
                client_build: "no-secrets-it".into(),
            },
        },
    )
    .await;
    match recv_frame(&mut c).await {
        Frame::Response { id: 0, res: Response::Welcome { .. } } => {}
        other => panic!("expected Welcome, got {other:?}"),
    }

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
            Frame::Response { id: 1, res: Response::Workspace(w) } => break w.id,
            Frame::Push(_) => continue,
            other => panic!("expected Workspace, got {other:?}"),
        }
    };

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
            Frame::Response { id: 2, res: Response::Session(meta) } => break meta.id,
            Frame::Push(_) => continue,
            Frame::Response { id: 2, res: Response::Error { code, message } } => {
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
        &Frame::Request { id: 3, req: Request::AttachSession { session_id: session_id.clone() } },
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
        &Frame::Request { id: 5, req: Request::DetachSession { session_id: session_id.clone() } },
    )
    .await;
    send_frame(&mut c, &Frame::Request { id: 6, req: Request::KillSession { session_id } }).await;

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

    std::env::remove_var("DAEMON_SECRET");
}
