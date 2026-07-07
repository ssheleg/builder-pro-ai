//! Task 12r (Pv2 §7, BL-7): boot integration proof for cold-rehydrate + attach-inactive-replays.
//!
//! Boots the daemon core (`bpa_sessiond::run`) against a REAL on-disk DB, creates a session,
//! writes a marker, lets the periodic scrollback flusher persist it, then shuts the daemon down
//! cleanly. A FRESH boot of `run()` against the SAME `HOME` (hence the same on-disk DB path —
//! `boot::app_support_dir()` resolves under `$HOME`) must cold-rehydrate that session into the new
//! `Supervisor` as an inactive, PTY-less, replay-only entry (`Supervisor::rehydrate_inactive`,
//! called from the boot loop before `serve()` starts accepting), and `AttachSession` on it must
//! succeed via the existing `AttachSession -> Push::Replay` path — no new wire request — with the
//! `Push::Replay` content carrying the marker and the session's `is_active == false`.
//!
//! This is a single, real end-to-end proof that "your records and scrollback reappear" (the
//! daemon-upgrade dialog's promise) is actually reachable: a genuine process-level restart against
//! persisted state, not just a unit-level supervisor call.

use std::time::Duration;

use bpa_sessiond::protocol::{decode_daemon_reply, encode_client_preamble, encode_frame};
use bpa_sessiond::protocol::{ClientPreamble, DaemonReply, FrameDecoder};
use bpa_sessiond::protocol::{Frame, Request, Response, Push};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

// `boot::run` resolves its on-disk DB path from `$HOME` (see `boot::app_support_dir`), and `HOME`
// is process-global mutable state. This integration test file compiles into its OWN test binary
// (a separate OS process from every other `tests/*.rs` file), so no cross-file race is possible;
// within this file only this one test touches `HOME`, so no intra-file race either. If a second
// HOME-touching test is ever added here, serialize both through a shared `std::sync::Mutex` guard
// before mutating the env.

async fn send_frame(s: &mut UnixStream, f: &Frame) {
    let bytes = encode_frame(f).unwrap();
    s.write_all(&bytes).await.unwrap();
    s.flush().await.unwrap();
}

async fn recv_frame(s: &mut UnixStream) -> Frame {
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

async fn preamble_handshake(s: &mut UnixStream) {
    let bytes = encode_client_preamble(&ClientPreamble {
        min: 2,
        max: 2,
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

async fn wait_for_socket(socket: &std::path::Path) -> UnixStream {
    for _ in 0..200 {
        if socket.exists() {
            if let Ok(c) = UnixStream::connect(socket).await {
                return c;
            }
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("daemon did not bind socket {socket:?} in time");
}

#[tokio::test]
async fn cold_rehydrate_then_attach_replays_persisted_marker_as_inactive() {
    // Isolate BOTH `$HOME` (daemon's DB path) and the socket dir under one temp root so this test
    // never touches the real user's app-support DB.
    let home_dir = tempfile::tempdir().unwrap();
    let socket_dir = tempfile::tempdir().unwrap();
    let socket = socket_dir.path().join("d.sock");

    let prior_home = std::env::var_os("HOME");
    std::env::set_var("HOME", home_dir.path());

    let session_id = {
        // ---- Boot #1: create a session, write a marker, let the flusher persist it, shut down. ----
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let socket_for_task = socket.clone();
        let shutdown_tx_for_task = shutdown_tx.clone();
        let boot = tokio::spawn(async move {
            bpa_sessiond::run(socket_for_task, shutdown_tx_for_task, shutdown_rx).await
        });

        let mut c = wait_for_socket(&socket).await;
        preamble_handshake(&mut c).await;

        send_frame(
            &mut c,
            &Frame::Request {
                id: 1,
                req: Request::CreateWorkspace {
                    name: "ws".into(),
                    root_path: home_dir.path().display().to_string(),
                },
            },
        )
        .await;
        let workspace_id = loop {
            match recv_frame(&mut c).await {
                Frame::Response {
                    id: 1,
                    res: Response::Workspace(w),
                } => break w.id,
                Frame::Push(_) => continue,
                other => panic!("expected Workspace, got {other:?}"),
            }
        };

        send_frame(
            &mut c,
            &Frame::Request {
                id: 2,
                req: Request::CreateSession {
                    workspace_id,
                    shell: Some("/bin/sh".into()),
                    cwd: Some(home_dir.path().display().to_string()),
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
                } => panic!("CreateSession failed: {code}: {message}"),
                other => panic!("expected Session, got {other:?}"),
            }
        };

        // Attach so the write actually lands in the in-memory scrollback ring (the ring is filled
        // by the reader thread regardless of attach, but attaching lets us also observe the live
        // Output to know the marker has definitely been processed before we ask for a flush).
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
        loop {
            match recv_frame(&mut c).await {
                Frame::Response {
                    id: 3,
                    res: Response::Ack,
                } => break,
                Frame::Push(_) => continue,
                other => panic!("expected Ack for AttachSession, got {other:?}"),
            }
        }

        send_frame(
            &mut c,
            &Frame::Request {
                id: 4,
                req: Request::WriteStdin {
                    session_id: session_id.clone(),
                    bytes: b"printf 'REHYDRATE_MARKER\\n'\n".to_vec(),
                },
            },
        )
        .await;

        // Drain frames until we've seen the marker echoed back as live Output (proof it's in the
        // in-memory ring), ignoring the WriteStdin Ack and any StateChanged pushes along the way.
        let mut seen_marker = false;
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while !seen_marker && std::time::Instant::now() < deadline {
            match tokio::time::timeout(Duration::from_millis(500), recv_frame(&mut c)).await {
                Ok(Frame::Push(Push::Output { bytes, .. })) => {
                    if bytes.windows(16).any(|w| w == b"REHYDRATE_MARKER") {
                        seen_marker = true;
                    }
                }
                Ok(_) => continue,
                Err(_) => continue, // timeout on this poll; loop re-checks the deadline
            }
        }
        assert!(seen_marker, "marker never appeared in live Output before the deadline");

        // Force a synchronous flush via DaemonShutdown{drain:true} (the same best-effort sweep the
        // periodic ticker runs) so the marker is durably in the on-disk DB before we tear down —
        // deterministic, unlike racing the 1s periodic ticker.
        send_frame(
            &mut c,
            &Frame::Request {
                id: 5,
                req: Request::DaemonShutdown { drain: true },
            },
        )
        .await;
        loop {
            match recv_frame(&mut c).await {
                Frame::Response {
                    id: 5,
                    res: Response::Ack,
                } => break,
                Frame::Push(_) => continue,
                other => panic!("expected Ack for DaemonShutdown, got {other:?}"),
            }
        }

        let res = tokio::time::timeout(Duration::from_secs(5), boot)
            .await
            .expect("run() did not return after drain-shutdown")
            .expect("join");
        assert!(res.is_ok(), "run() returned error: {res:?}");
        assert!(!socket.exists(), "socket must be unlinked after shutdown");

        session_id
    };

    // ---- Boot #2: a FRESH process-level boot against the SAME HOME (same on-disk DB). The cold-
    // rehydrate loop in `boot::run` must load the persisted session into the new Supervisor as an
    // inactive, PTY-less, replay-only entry BEFORE `serve()` starts accepting. ----
    let (shutdown_tx2, shutdown_rx2) = tokio::sync::watch::channel(false);
    let socket_for_task2 = socket.clone();
    let shutdown_tx_for_task2 = shutdown_tx2.clone();
    let boot2 = tokio::spawn(async move {
        bpa_sessiond::run(socket_for_task2, shutdown_tx_for_task2, shutdown_rx2).await
    });

    let mut c2 = wait_for_socket(&socket).await;
    preamble_handshake(&mut c2).await;

    // AttachSession on the rehydrated (now cold, PTY-less) session must SUCCEED — not
    // NoSuchSession — via the replay-only branch (attach.rs).
    send_frame(
        &mut c2,
        &Frame::Request {
            id: 10,
            req: Request::AttachSession {
                session_id: session_id.clone(),
            },
        },
    )
    .await;

    let mut got_ack = false;
    let mut replay_content: Option<Vec<u8>> = None;
    let deadline = std::time::Instant::now() + Duration::from_secs(5);
    while (!got_ack || replay_content.is_none()) && std::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_millis(500), recv_frame(&mut c2)).await {
            Ok(Frame::Response {
                id: 10,
                res: Response::Ack,
            }) => got_ack = true,
            Ok(Frame::Response {
                id: 10,
                res: Response::Error { code, message },
            }) => panic!(
                "AttachSession on a cold-rehydrated session must succeed, got Error {code}: {message}"
            ),
            Ok(Frame::Push(Push::Replay {
                session_id: sid,
                content,
                ..
            })) => {
                assert_eq!(sid, session_id);
                replay_content = Some(content);
            }
            Ok(_) => continue,
            Err(_) => continue,
        }
    }
    assert!(got_ack, "AttachSession must Ack for a cold-rehydrated session");
    let content = replay_content.expect("Push::Replay must have been sent for the rehydrated session");
    assert!(
        content.windows(16).any(|w| w == b"REHYDRATE_MARKER"),
        "Replay content must carry the persisted marker, got: {content:?}"
    );

    // No live Output can ever follow — there is no reader thread behind a rehydrated entry.
    let extra = tokio::time::timeout(Duration::from_millis(300), recv_frame(&mut c2)).await;
    if let Ok(Frame::Push(Push::Output { bytes, .. })) = &extra {
        panic!("a rehydrated (PTY-less) session must never produce live Output, got: {bytes:?}");
    }

    // And GetSessionState confirms is_active == false for the rehydrated session.
    send_frame(
        &mut c2,
        &Frame::Request {
            id: 11,
            req: Request::GetSessionState {
                session_id: session_id.clone(),
            },
        },
    )
    .await;
    loop {
        match recv_frame(&mut c2).await {
            Frame::Response {
                id: 11,
                res: Response::Session(meta),
            } => {
                assert_eq!(meta.id, session_id);
                assert!(
                    !meta.is_active,
                    "a cold-rehydrated session must report is_active == false"
                );
                break;
            }
            Frame::Push(_) => continue,
            other => panic!("expected Session, got {other:?}"),
        }
    }

    shutdown_tx2.send(true).unwrap();
    let res2 = tokio::time::timeout(Duration::from_secs(5), boot2)
        .await
        .expect("run() (boot #2) did not return after shutdown")
        .expect("join");
    assert!(res2.is_ok(), "run() (boot #2) returned error: {res2:?}");

    // Restore the real HOME so any later test in this (single-test) binary is unaffected.
    match prior_home {
        Some(v) => std::env::set_var("HOME", v),
        None => std::env::remove_var("HOME"),
    }
}
