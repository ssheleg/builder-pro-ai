//! Integration proofs for the workspace guard rails (audit 2026-07-24):
//!
//! * **SES-4** (probe p4): `CreateSession` with a `workspace_id` that does not exist is REJECTED
//!   up front with the typed `NoSuchWorkspace` error — previously the create succeeded, only the
//!   best-effort persist failed (silently, FK, log-only), and the session vanished on the next
//!   restart with no client-visible error anywhere on the path.
//! * **SES-1** (probe p5): a `CreateSession` racing `RemoveWorkspace` can no longer survive as an
//!   orphaned live shell inside a deleted workspace — the closing gate (same `NoSuchWorkspace`)
//!   plus the removal's post-delete stray sweep are exercised by a create-STORM against a real
//!   removal: afterwards, no session of the removed workspace may still be listed or live.
//! * **SES-6**: `RemoveWorkspaceRoot` on the LAST root returns the typed `LastRoot` error and
//!   leaves the workspace's roots untouched (the in-module unit test pins the code; this proves
//!   the no-side-effect half end to end).
//!
//! ## Why this lives in `tests/` (mirrors `remove_workspace.rs`'s rationale)
//!
//! The storm drives several real `/bin/sh` PTYs plus real `killpg → grace → SIGKILL → reap`
//! teardowns; integration files link as their own binaries, keeping that load out of the lib
//! test binary (whose `attach` timing assertions are known load-sensitive, BL-108).
//!
//! ## `$HOME` isolation (D6)
//!
//! Identical to `remove_workspace.rs`: `ServerDeps` is built around `Db::open_in_memory()` (no
//! file is ever opened), and `HOME` is pinned to a tempdir for the duration anyway.

use std::sync::Arc;

use bpa_sessiond::attach::AttachRegistry;
use bpa_sessiond::persistence::Db;
use bpa_sessiond::protocol::{decode_daemon_reply, encode_client_preamble, encode_frame};
use bpa_sessiond::protocol::{
    ClientPreamble, DaemonReply, FrameDecoder, CLIENT_MAX_VERSION, CLIENT_MIN_VERSION,
};
use bpa_sessiond::protocol::{Frame, Push, Request, Response};
use bpa_sessiond::pty_supervisor::{Supervisor, SupervisorError};
use bpa_sessiond::socket_server::{serve, ServerDeps};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Mutex;

// `HOME` is process-global mutable state; serialize every `HOME`-touching test in this file —
// mirrors `remove_workspace.rs`'s `HOME_LOCK` rationale verbatim.
static HOME_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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

// ---- wire helpers (mirror `remove_workspace.rs`'s, same codec on the client side) ----

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

/// Bounded [`recv_frame`]: panics on a 5 s timeout so a regression fails fast instead of hanging.
async fn recv_frame_t(s: &mut UnixStream) -> Frame {
    match tokio::time::timeout(std::time::Duration::from_secs(5), recv_frame(s)).await {
        Ok(f) => f,
        Err(_) => panic!("timed out waiting for a frame"),
    }
}

/// Drain frames until the response correlated to `id` arrives, skipping interleaved pushes.
async fn recv_response_for(c: &mut UnixStream, id: u64) -> Response {
    for _ in 0..64 {
        if let Frame::Response { id: rid, res } = recv_frame_t(c).await {
            if rid == id {
                return res;
            }
        }
    }
    panic!("no response for request id {id} after 64 frames");
}

async fn preamble_handshake(s: &mut UnixStream) {
    let bytes = encode_client_preamble(&ClientPreamble {
        min: CLIENT_MIN_VERSION,
        max: CLIENT_MAX_VERSION,
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

/// Create a workspace over the wire, draining BOTH resulting frames (response + push).
async fn create_workspace(c: &mut UnixStream, id: u64, name: &str, root: &str) -> String {
    send_frame(
        c,
        &Frame::Request {
            id,
            req: Request::CreateWorkspace {
                name: name.into(),
                root_path: root.into(),
            },
        },
    )
    .await;
    let mut got: Option<String> = None;
    for _ in 0..2 {
        match recv_frame_t(c).await {
            Frame::Response {
                id: rid,
                res: Response::Workspace(w),
            } if rid == id => got = Some(w.id),
            Frame::Push(Push::WorkspaceCreated { .. }) => {}
            other => panic!("unexpected frame while creating workspace: {other:?}"),
        }
    }
    got.expect("expected a Workspace response")
}

/// One `CreateSession` round-trip whose outcome is RETURNED, not asserted — the storm records
/// both successes and the typed rejections.
async fn create_session_roundtrip(c: &mut UnixStream, id: u64, workspace_id: &str) -> Response {
    send_frame(
        c,
        &Frame::Request {
            id,
            req: Request::CreateSession {
                workspace_id: workspace_id.into(),
                shell: Some("/bin/sh".into()),
                cwd: Some("/tmp".into()),
                env_overrides: vec![],
                cols: 80,
                rows: 24,
            },
        },
    )
    .await;
    recv_response_for(c, id).await
}

/// Boot a real `serve()` on a tempdir socket over an in-memory DB; returns the pieces a test
/// tears down via [`shutdown_server`].
async fn boot_server() -> (
    std::path::PathBuf,
    tokio::sync::watch::Sender<bool>,
    tokio::task::JoinHandle<()>,
    tempfile::TempDir,
    tempfile::TempDir,
    Arc<ServerDeps>,
) {
    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("d.sock");
    let listener = UnixListener::bind(&sock).unwrap();
    let (tx, rx) = tokio::sync::watch::channel(false);
    let supervisor = Arc::new(Supervisor::new());
    let db = Arc::new(Mutex::new(Db::open_in_memory().unwrap()));
    let attach = Arc::new(AttachRegistry::new(supervisor.clone()));
    let runtime = tempfile::tempdir().unwrap();
    let deps = Arc::new(ServerDeps::new(
        supervisor,
        db,
        attach,
        "test".into(),
        runtime.path().to_path_buf(),
        tx.clone(),
    ));
    let jh = tokio::spawn({
        let deps = deps.clone();
        async move {
            let _ = serve(listener, deps, rx).await;
        }
    });
    (sock, tx, jh, dir, runtime, deps)
}

async fn shutdown_server(
    tx: tokio::sync::watch::Sender<bool>,
    jh: tokio::task::JoinHandle<()>,
    deps: &Arc<ServerDeps>,
) {
    tx.send(true).unwrap();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), jh).await;
    deps.supervisor.shutdown_all();
}

// ---- SES-4: unknown workspace id is rejected up front ----

#[tokio::test]
async fn create_session_with_unknown_workspace_is_rejected_no_such_workspace() {
    let home = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home.path());
    let (sock, tx, jh, _d, _r, deps) = boot_server().await;

    let mut c = UnixStream::connect(&sock).await.unwrap();
    preamble_handshake(&mut c).await;

    match create_session_roundtrip(&mut c, 1, "no-such-workspace-id").await {
        Response::Error { code, message } => {
            assert_eq!(code, "NoSuchWorkspace");
            assert!(
                message.contains("no-such-workspace-id"),
                "the error must name the offending id, got: {message}"
            );
        }
        other => panic!("expected NoSuchWorkspace, got {other:?}"),
    }

    // Nothing was spawned or persisted as a side effect of the rejected create.
    {
        let db = deps.db.lock().await;
        assert!(
            db.list_sessions().unwrap().is_empty(),
            "a rejected create must persist no session row"
        );
    }
    send_frame(
        &mut c,
        &Frame::Request {
            id: 2,
            req: Request::ListSessions,
        },
    )
    .await;
    match recv_response_for(&mut c, 2).await {
        Response::Sessions(v) => assert!(v.is_empty(), "no session may be listed, got {v:?}"),
        other => panic!("expected Sessions, got {other:?}"),
    }

    // Sanity: the SAME daemon still creates fine for a real workspace (the gate is not a
    // blanket refusal).
    let ws = create_workspace(&mut c, 3, "real", "/tmp").await;
    let sid = match create_session_roundtrip(&mut c, 4, &ws).await {
        Response::Session(m) => m.id,
        other => panic!("create for a real workspace must succeed, got {other:?}"),
    };
    assert!(deps.supervisor.meta(&sid).unwrap().is_active);

    shutdown_server(tx, jh, &deps).await;
}

// ---- SES-1: create-storm x RemoveWorkspace -> no surviving session/shell ----

#[tokio::test]
async fn remove_workspace_racing_a_create_storm_leaves_no_survivors() {
    let home = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home.path());
    let (sock, tx, jh, _d, _r, deps) = boot_server().await;

    let mut c1 = UnixStream::connect(&sock).await.unwrap();
    preamble_handshake(&mut c1).await;
    let doomed = create_workspace(&mut c1, 1, "doomed", "/tmp").await;
    let keeper = create_workspace(&mut c1, 2, "keeper", "/tmp").await;

    // Seed sessions in the doomed workspace so the removal has real teardown work to do
    // (widening the create/remove overlap window deterministically).
    for i in 0..2 {
        match create_session_roundtrip(&mut c1, 10 + i, &doomed).await {
            Response::Session(_) => {}
            other => panic!("seed create failed: {other:?}"),
        }
    }
    let keeper_session = match create_session_roundtrip(&mut c1, 13, &keeper).await {
        Response::Session(m) => m.id,
        other => panic!("keeper create failed: {other:?}"),
    };

    // The storm: a second connection hammering CreateSession on the doomed workspace while the
    // removal runs on c1. Two SYNCHRONOUS creates first guarantee the race actually happened
    // (they land before the removal starts, so they become live victims the removal must kill);
    // the spawned storm then overlaps the removal and must be gated/swept. Every outcome is
    // recorded.
    let mut c2 = UnixStream::connect(&sock).await.unwrap();
    preamble_handshake(&mut c2).await;
    let mut pre_created = Vec::new();
    for i in 0..2u64 {
        match create_session_roundtrip(&mut c2, 500 + i, &doomed).await {
            Response::Session(m) => pre_created.push(m.id),
            other => panic!("pre-storm create must succeed (removal not started): {other:?}"),
        }
    }
    let storm_doomed = doomed.clone();
    let storm = tokio::spawn(async move {
        let mut outcomes = Vec::new();
        for i in 0..30u64 {
            let res = create_session_roundtrip(&mut c2, 1000 + i, &storm_doomed).await;
            outcomes.push(res);
        }
        outcomes
    });

    // Fire the removal immediately, overlapping the storm.
    send_frame(
        &mut c1,
        &Frame::Request {
            id: 3,
            req: Request::RemoveWorkspace {
                workspace_id: doomed.clone(),
            },
        },
    )
    .await;
    match recv_response_for(&mut c1, 3).await {
        Response::Ack => {}
        other => panic!("RemoveWorkspace must Ack, got {other:?}"),
    }
    let outcomes = storm.await.expect("storm task");

    // ---- The invariant: NOTHING of the doomed workspace survived — not in the supervisor, not
    // in the DB, not over the wire. ----
    let mut created_ids = pre_created.clone();
    let mut rejected = 0usize;
    for res in &outcomes {
        match res {
            Response::Session(m) => created_ids.push(m.id.clone()),
            Response::Error { code, .. } => {
                assert_eq!(
                    code, "NoSuchWorkspace",
                    "a create into a closing/deleted workspace may only fail as NoSuchWorkspace, got {code}"
                );
                rejected += 1;
            }
            other => panic!("unexpected storm outcome: {other:?}"),
        }
    }
    assert!(
        rejected > 0,
        "the closing gate must have rejected at least one create once the workspace was closing/gone"
    );
    for id in &created_ids {
        assert!(
            matches!(
                deps.supervisor.meta(id),
                Err(SupervisorError::NoSuchSession(_))
            ),
            "session {id} of the removed workspace must be killed/reaped, never a surviving shell"
        );
    }
    {
        let db = deps.db.lock().await;
        assert!(
            db.list_sessions()
                .unwrap()
                .iter()
                .all(|m| m.workspace_id != doomed),
            "no session row of the removed workspace may persist"
        );
        assert!(
            db.workspace_session_ids(&doomed).is_err(),
            "the workspace itself must be gone"
        );
    }
    // And a post-removal create is DETERMINISTICALLY rejected (the workspace no longer exists).
    let mut c3 = UnixStream::connect(&sock).await.unwrap();
    preamble_handshake(&mut c3).await;
    match create_session_roundtrip(&mut c3, 1, &doomed).await {
        Response::Error { code, .. } => assert_eq!(code, "NoSuchWorkspace"),
        other => panic!("post-removal create must be NoSuchWorkspace, got {other:?}"),
    }

    // The bystander workspace and its session are untouched.
    assert!(deps.supervisor.meta(&keeper_session).unwrap().is_active);

    shutdown_server(tx, jh, &deps).await;
}

// ---- SES-6: removing the LAST root is a typed error with zero side effects ----

#[tokio::test]
async fn remove_last_workspace_root_returns_last_root_and_keeps_roots_untouched() {
    let home = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home.path());
    let (sock, tx, jh, _d, _r, deps) = boot_server().await;

    let mut c = UnixStream::connect(&sock).await.unwrap();
    preamble_handshake(&mut c).await;
    let ws = create_workspace(&mut c, 1, "w", "/tmp").await;
    let only_root = {
        let db = deps.db.lock().await;
        let w = db
            .list_workspaces()
            .unwrap()
            .into_iter()
            .find(|w| w.id == ws)
            .expect("workspace persisted");
        assert_eq!(w.roots.len(), 1, "precondition: exactly one root");
        w.roots[0].clone()
    };

    send_frame(
        &mut c,
        &Frame::Request {
            id: 2,
            req: Request::RemoveWorkspaceRoot {
                workspace_id: ws.clone(),
                path: only_root.clone(),
            },
        },
    )
    .await;
    match recv_response_for(&mut c, 2).await {
        Response::Error { code, .. } => assert_eq!(code, "LastRoot"),
        other => panic!("expected LastRoot error, got {other:?}"),
    }

    // The roots survived the rejected removal byte-for-byte.
    {
        let db = deps.db.lock().await;
        let w = db
            .list_workspaces()
            .unwrap()
            .into_iter()
            .find(|w| w.id == ws)
            .expect("workspace must still exist");
        assert_eq!(
            w.roots,
            vec![only_root],
            "a rejected last-root removal must leave the roots untouched"
        );
    }

    shutdown_server(tx, jh, &deps).await;
}
