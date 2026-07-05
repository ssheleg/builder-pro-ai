//! Integration: boot the daemon `run()` on a temp socket, complete the handshake,
//! create a session, then trigger clean shutdown (spec §14.1 boot integration).

use std::time::Duration;

use bpa_sessiond::protocol::{Frame, Request, Response, MAGIC, PROTO_VERSION};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

async fn send_frame(s: &mut UnixStream, f: &Frame) {
    let body = bincode::serialize(f).unwrap();
    s.write_all(&(body.len() as u32).to_le_bytes())
        .await
        .unwrap();
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

#[tokio::test]
async fn boot_handshake_create_session_and_clean_shutdown() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("d.sock");
    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    // Boot the daemon core on the temp socket.
    let socket_for_task = socket.clone();
    let boot = tokio::spawn(async move { bpa_sessiond::run(socket_for_task, shutdown_rx).await });

    // Wait for the socket to appear + accept a connection.
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
                client_build: "it".into(),
            },
        },
    )
    .await;
    match recv_frame(&mut c).await {
        Frame::Response {
            id: 0,
            res: Response::Welcome { proto_version, .. },
        } => {
            assert_eq!(proto_version, PROTO_VERSION);
        }
        other => panic!("expected Welcome, got {other:?}"),
    }

    // Create a workspace first (session needs a workspace id), then a session.
    send_frame(
        &mut c,
        &Frame::Request {
            id: 1,
            req: Request::CreateWorkspace {
                name: "ws".into(),
                root_path: dir.path().display().to_string(),
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
            Frame::Push(_) => continue, // ignore the WorkspaceCreated push
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
                cwd: Some(dir.path().display().to_string()),
                env_overrides: vec![],
                cols: 80,
                rows: 24,
            },
        },
    )
    .await;
    let created_id = loop {
        match recv_frame(&mut c).await {
            Frame::Response {
                id: 2,
                res: Response::Session(meta),
            } => {
                assert_eq!(meta.cols, 80);
                assert_eq!(meta.rows, 24);
                break meta.id;
            }
            Frame::Push(_) => continue, // ignore SessionCreated push
            Frame::Response {
                id: 2,
                res: Response::Error { code, message },
            } => {
                panic!("CreateSession failed: {code}: {message}");
            }
            other => panic!("expected Session, got {other:?}"),
        }
    };
    assert!(!created_id.is_empty());

    // Clean shutdown: signal the watch, expect run() to return Ok and the socket to be removed.
    shutdown_tx.send(true).unwrap();
    let res = tokio::time::timeout(Duration::from_secs(5), boot)
        .await
        .expect("run() did not return after shutdown")
        .expect("join");
    assert!(res.is_ok(), "run() returned error: {res:?}");
    assert!(
        !socket.exists(),
        "socket should be unlinked on clean shutdown"
    );
}

#[tokio::test]
async fn second_instance_flock_refusal() {
    let dir = tempfile::tempdir().unwrap();
    let lockfile = dir.path().join("d.lock");

    let g1 = bpa_sessiond::singleton::acquire_lock_at_for_test(&lockfile)
        .expect("first lock acquisition succeeds");
    let err = bpa_sessiond::singleton::acquire_lock_at_for_test(&lockfile)
        .expect_err("second lock acquisition on the same file must fail");
    assert_eq!(err.kind(), std::io::ErrorKind::WouldBlock);
    drop(g1);
}

#[tokio::test]
async fn stale_socket_file_is_unlinked_and_rebound() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("d.sock");

    // Simulate a stale socket file left behind by a crashed daemon: a bare regular
    // file at the socket path (not a live listening socket).
    std::fs::write(&socket, b"").unwrap();
    assert!(socket.exists());

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let socket_for_task = socket.clone();
    let boot = tokio::spawn(async move { bpa_sessiond::run(socket_for_task, shutdown_rx).await });

    let mut connected = false;
    for _ in 0..100 {
        if UnixStream::connect(&socket).await.is_ok() {
            connected = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(connected, "daemon did not rebind the stale socket in time");

    shutdown_tx.send(true).unwrap();
    let res = tokio::time::timeout(Duration::from_secs(5), boot)
        .await
        .expect("run() did not return after shutdown")
        .expect("join");
    assert!(res.is_ok(), "run() returned error: {res:?}");
}
