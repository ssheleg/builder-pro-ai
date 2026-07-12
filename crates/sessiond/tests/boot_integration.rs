//! Integration: boot the daemon `run()` on a temp socket, complete the handshake,
//! create a session, then trigger clean shutdown (spec §14.1 boot integration).

use std::time::Duration;

use bpa_sessiond::protocol::{decode_daemon_reply, encode_client_preamble, encode_frame};
use bpa_sessiond::protocol::{
    ClientPreamble, DaemonReply, FrameDecoder, CLIENT_MAX_VERSION, CLIENT_MIN_VERSION,
};
use bpa_sessiond::protocol::{Frame, Push, Request, Response, SessionLifecycle};
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
/// codec-agnostic client preamble (`CLIENT_MIN_VERSION..=CLIENT_MAX_VERSION`, so it tracks the
/// current wire version and can never desync from a version bump), then read + `decode_daemon_reply` the fixed
/// 9-byte reply header (plus the trailing build string when accepted) and require `Accepted`.
/// Distinct from `send_frame`/`recv_frame`: the preamble is a raw byte layout, not a CBOR `Frame`.
// `boot::run` resolves its on-disk DB path from `$HOME` (see `boot::app_support_dir`), and
// `HOME` is process-global mutable state. THREE tests in this file boot the real daemon core and
// must isolate `HOME` under their own tempdir (D6: never touch the developer's real app-support
// DB); `cargo test` runs tests from the same file/binary concurrently by default, so those tests
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

/// Round-3 hardening (H1): at CLEAN shutdown, every session that is still LIVE when the shutdown
/// watch flips must have its FINAL scrollback tail AND its terminal `Exited` (killed) lifecycle
/// durably persisted BEFORE `run()` returns — via an AWAITED write path, never a detached task
/// that the `#[tokio::main]` runtime drop can silently discard.
///
/// This test reproduces the production runtime semantics EXACTLY, which is what makes it a
/// deterministic RED pre-fix rather than a flaky one:
///
/// - Boot #1 drives `bpa_sessiond::run` as the MAIN future of `Runtime::block_on` on a manually
///   built current-thread runtime — precisely what `#[tokio::main]` desugars to in `main.rs` —
///   with the wire-driving client as a SPAWNED task (it runs during `run()`'s own await points).
/// - The shutdown is triggered through the SAME `watch` flip `main.rs`'s SIGTERM handler uses
///   (`Request::DaemonShutdown{drain}` converges on the identical post-`serve` path in `run()`).
/// - The moment `block_on` returns, the runtime is DROPPED — like the production process exiting —
///   which discards any still-queued detached task unpolled. Pre-fix, the killed session's final
///   flush was exactly such a task: `shutdown_all()`'s kill fires `on_exited`, which only
///   SCHEDULED the DB write via `rt_handle.spawn` and nothing ever awaited it, so the `Exited`
///   lifecycle (and any not-yet-swept scrollback tail) died with the runtime. Post-fix, `run()`
///   awaits every pending final flush before checkpointing, so the write has landed by the time
///   `block_on` returns no matter what happens to the runtime afterwards.
/// - Boot #2 (a fresh runtime, same `$HOME` hence same on-disk DB) then asserts the rehydrated
///   session reports `lifecycle == Exited` and its replayed scrollback carries the marker.
#[test]
fn clean_shutdown_persists_killed_sessions_exited_lifecycle_and_scrollback() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("d.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    const MARKER: &[u8] = b"H1_SHUTDOWN_MARKER";

    // ---- Boot #1: create a session, land the marker in its ring, then flip the shutdown watch
    // while the session is STILL LIVE (no DaemonShutdown{drain}, no KillSession — the kill happens
    // inside `run()`'s own `shutdown_all()`). ----
    let session_id = {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap();

        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let (sid_tx, sid_rx) = std::sync::mpsc::channel::<String>();

        // The client driver runs as a spawned task on the same runtime; `run()` itself is the
        // block_on main future (the production shape — see the test doc comment).
        let socket_for_client = socket.clone();
        let home_for_client = home_dir.path().to_path_buf();
        let shutdown_tx_for_client = shutdown_tx.clone();
        rt.spawn(async move {
            let mut c = None;
            for _ in 0..200 {
                if socket_for_client.exists() {
                    if let Ok(s) = UnixStream::connect(&socket_for_client).await {
                        c = Some(s);
                        break;
                    }
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            let mut c = c.expect("daemon did not bind socket in time");
            preamble_handshake(&mut c).await;

            send_frame(
                &mut c,
                &Frame::Request {
                    id: 1,
                    req: Request::CreateWorkspace {
                        name: "ws".into(),
                        root_path: home_for_client.display().to_string(),
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
                        cwd: Some(home_for_client.display().to_string()),
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

            // Attach so the live Output stream proves the marker reached the in-memory ring
            // before we flip the shutdown watch.
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
                        bytes: b"printf 'H1_SHUTDOWN_MARKER\\n'\n".to_vec(),
                    },
                },
            )
            .await;

            let mut seen_marker = false;
            let deadline = std::time::Instant::now() + Duration::from_secs(5);
            while !seen_marker && std::time::Instant::now() < deadline {
                match tokio::time::timeout(Duration::from_millis(500), recv_frame(&mut c)).await {
                    Ok(Frame::Push(Push::Output { bytes, .. })) => {
                        if bytes.windows(MARKER.len()).any(|w| w == MARKER) {
                            seen_marker = true;
                        }
                    }
                    Ok(_) => continue,
                    Err(_) => continue,
                }
            }
            assert!(
                seen_marker,
                "marker never appeared in live Output before the deadline"
            );

            sid_tx.send(session_id).unwrap();
            // Flip the SAME watch main.rs's SIGTERM handler flips — the session is still live, so
            // `run()`'s post-serve `shutdown_all()` is what kills it.
            shutdown_tx_for_client.send(true).unwrap();
        });

        let res = rt.block_on(bpa_sessiond::run(socket.clone(), shutdown_tx, shutdown_rx));
        assert!(res.is_ok(), "run() returned error: {res:?}");
        // Production semantics: the runtime dies the moment the main future returns. Any final
        // flush still sitting in the ready queue as a detached task is dropped unpolled here —
        // exactly the window H1 closes.
        drop(rt);

        sid_rx
            .recv_timeout(Duration::from_secs(1))
            .expect("client driver must have created a session before shutdown")
    };

    // ---- Boot #2: fresh runtime, same HOME/DB. The rehydrated session must report the terminal
    // Exited lifecycle and replay the marker. ----
    let rt2 = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    rt2.block_on(async {
        let (shutdown_tx2, shutdown_rx2) = tokio::sync::watch::channel(false);
        let socket_for_task2 = socket.clone();
        let shutdown_tx_for_task2 = shutdown_tx2.clone();
        let boot2 = tokio::spawn(async move {
            bpa_sessiond::run(socket_for_task2, shutdown_tx_for_task2, shutdown_rx2).await
        });

        let mut c2 = None;
        for _ in 0..200 {
            if socket.exists() {
                if let Ok(s) = UnixStream::connect(&socket).await {
                    c2 = Some(s);
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let mut c2 = c2.expect("daemon (boot #2) did not bind socket in time");
        preamble_handshake(&mut c2).await;

        // Lifecycle: the persisted row (rehydrated as-is into the fresh supervisor) must carry the
        // terminal Exited (killed-at-shutdown) lifecycle — not the live lifecycle the periodic
        // sweep last saw.
        send_frame(
            &mut c2,
            &Frame::Request {
                id: 20,
                req: Request::GetSessionState {
                    session_id: session_id.clone(),
                },
            },
        )
        .await;
        loop {
            match recv_frame(&mut c2).await {
                Frame::Response {
                    id: 20,
                    res: Response::Session(meta),
                } => {
                    assert_eq!(meta.id, session_id);
                    assert!(
                        matches!(meta.lifecycle, SessionLifecycle::Exited { .. }),
                        "a session killed at clean shutdown must rehydrate with the terminal \
                         Exited lifecycle (awaited final flush), got {:?}",
                        meta.lifecycle
                    );
                    assert!(!meta.is_active, "rehydrated session must be inactive");
                    break;
                }
                Frame::Push(_) => continue,
                other => panic!("expected Session, got {other:?}"),
            }
        }

        // Scrollback: attach and require the replayed content to carry the marker (the final
        // scrollback tail must have been persisted by the awaited flush, not lost with the
        // runtime drop).
        send_frame(
            &mut c2,
            &Frame::Request {
                id: 21,
                req: Request::AttachSession {
                    session_id: session_id.clone(),
                },
            },
        )
        .await;
        let mut replay_content: Option<Vec<u8>> = None;
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while replay_content.is_none() && std::time::Instant::now() < deadline {
            match tokio::time::timeout(Duration::from_millis(500), recv_frame(&mut c2)).await {
                Ok(Frame::Push(Push::Replay {
                    session_id: sid,
                    content,
                    ..
                })) => {
                    assert_eq!(sid, session_id);
                    replay_content = Some(content);
                }
                Ok(Frame::Response {
                    id: 21,
                    res: Response::Error { code, message },
                }) => panic!("AttachSession (boot #2) failed: {code}: {message}"),
                Ok(_) => continue,
                Err(_) => continue,
            }
        }
        let content = replay_content.expect("Push::Replay must arrive for the rehydrated session");
        assert!(
            content.windows(MARKER.len()).any(|w| w == MARKER),
            "replayed scrollback must carry the marker written right before clean shutdown, \
             got: {content:?}"
        );

        shutdown_tx2.send(true).unwrap();
        let res2 = tokio::time::timeout(Duration::from_secs(5), boot2)
            .await
            .expect("run() (boot #2) did not return after shutdown")
            .expect("join");
        assert!(res2.is_ok(), "run() (boot #2) returned error: {res2:?}");
    });
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
