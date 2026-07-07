//! Integration: boot the daemon `run()` on a temp socket, complete the handshake,
//! create a session, then trigger clean shutdown (spec §14.1 boot integration).

use std::time::Duration;

use bpa_sessiond::protocol::{decode_daemon_reply, encode_client_preamble, encode_frame};
use bpa_sessiond::protocol::{ClientPreamble, DaemonReply, FrameDecoder};
use bpa_sessiond::protocol::{Frame, Request, Response};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

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
/// codec-agnostic client preamble (`min:2, max:2`), then read + `decode_daemon_reply` the fixed
/// 9-byte reply header (plus the trailing build string when accepted) and require `Accepted`.
/// Distinct from `send_frame`/`recv_frame`: the preamble is a raw byte layout, not a CBOR `Frame`.
// `boot::run` resolves its on-disk DB path from `$HOME` (see `boot::app_support_dir`), and
// `HOME` is process-global mutable state. TWO tests in this file boot the real daemon core and
// must isolate `HOME` under their own tempdir (D6: never touch the developer's real app-support
// DB); `cargo test` runs tests from the same file/binary concurrently by default, so both tests
// mutating `HOME` at once would race each other's set/read/restore sequence. This lock serializes
// them — mirrors `singleton.rs`'s `ENV_LOCK`, which documents the identical hazard.
static HOME_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Set `HOME` to `dir` under [`HOME_LOCK`] and return a guard that restores the prior value (and
/// releases the lock) on drop — so every HOME-touching test in this file is fully serialized
/// against the others even across a panic.
struct HomeGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    prior: Option<std::ffi::OsString>,
}

impl HomeGuard {
    fn set(dir: &std::path::Path) -> Self {
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

async fn preamble_handshake(s: &mut UnixStream) {
    let bytes = encode_client_preamble(&ClientPreamble {
        min: 2,
        max: 2,
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

#[tokio::test]
async fn boot_handshake_create_session_and_clean_shutdown() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("d.sock");

    // Isolate `$HOME` (`boot::app_support_dir` resolves the on-disk DB path under it — see
    // `boot.rs::app_support_dir`) under this test's own tempdir so `bpa_sessiond::run`, which
    // boots the REAL daemon core below, never reads/writes the developer's actual
    // `~/Library/Application Support/.../bpa.db` (D6: ambient-$HOME test pollution).
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());
    assert_eq!(
        bpa_sessiond::app_support_dir_for_test(),
        home_dir
            .path()
            .join("Library/Application Support/ai.builderpro.desktop"),
        "HOME isolation must actually redirect the daemon's app-support/DB path under the tempdir"
    );

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

    // Boot the daemon core on the temp socket.
    let socket_for_task = socket.clone();
    let shutdown_tx_for_task = shutdown_tx.clone();
    let boot = tokio::spawn(async move {
        bpa_sessiond::run(socket_for_task, shutdown_tx_for_task, shutdown_rx).await
    });

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

    // Handshake (Pv2 §4.2/§4.4 preamble, replacing the v1 Hello→Welcome frame exchange).
    preamble_handshake(&mut c).await;

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
    // `_home_guard` restores the prior `HOME` (and releases `HOME_LOCK`) on drop here.
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

    // Isolate `$HOME` (D6): this test also boots the real daemon core via `bpa_sessiond::run`,
    // so its on-disk DB path must be redirected under a tempdir rather than the developer's real
    // app-support dir.
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let socket_for_task = socket.clone();
    let shutdown_tx_for_task = shutdown_tx.clone();
    let boot = tokio::spawn(async move {
        bpa_sessiond::run(socket_for_task, shutdown_tx_for_task, shutdown_rx).await
    });

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
