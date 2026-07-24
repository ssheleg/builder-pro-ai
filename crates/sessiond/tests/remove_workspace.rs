//! Integration proof for `Request::RemoveWorkspace` (spec §3.3): a workspace removal is
//! DESTRUCTIVE, TOTAL and HONEST.
//!
//! This is the capability the protocol was missing. Without it a workspace whose root paths had
//! been deleted off disk was permanently undeletable, so real databases accumulated hundreds of
//! dead workspaces the sidebar still rendered. "Total" therefore has to mean all of it — the
//! workspace row, its `workspace_root` rows, every session that belongs to it, and those sessions'
//! `scrollback` + `command_events` rows — and "honest" has to mean no orphaned child process is
//! left running behind the deleted rows, and no push is emitted that would make a client re-insert
//! what it was just told to drop.
//!
//! ## Why this one lives in `tests/` and not in `socket_server.rs`'s unit-test module
//!
//! It drives TWO real `/bin/sh` PTYs (a live session in the doomed workspace and one in an
//! untouched bystander workspace) and then blocks on `Supervisor::kill`'s
//! `killpg → grace → SIGKILL → reap` sequence. `cargo test` runs the tests inside ONE binary
//! concurrently, and that PTY/thread burst measurably destabilised a pre-existing, load-sensitive
//! timing assertion elsewhere in the lib binary
//! (`attach::tests::natural_exit_final_output_reaches_attached_client_and_entry_is_reaped`, whose
//! own comments record that it "failed sporadically" under "parallel-test load"). Integration test
//! files compile into their own binaries, which `cargo test` runs one at a time — so the heavy
//! case keeps its full end-to-end coverage without making an unrelated, already-fragile test flake.
//! The two cheap `RemoveWorkspace` cases (unknown id, failed-removal atomicity) stay as in-module
//! unit tests next to the dispatch arm they cover.
//!
//! ## `$HOME` isolation (D6)
//!
//! This test does NOT boot the daemon core (`bpa_sessiond::run`), so it never goes through
//! `boot::app_support_dir()`: it builds `ServerDeps` directly around `Db::open_in_memory()` (no
//! file is ever opened) and a tempdir runtime root, then drives `serve()` on a tempdir socket.
//! `HOME` is nevertheless pinned to a tempdir for the duration, mirroring
//! `boot_integration.rs`'s `HOME_LOCK`/`HomeGuard` verbatim: leaking into the developer's real
//! `~/Library/Application Support/ai.builderpro.desktop/bpa.db` is precisely the mess this feature
//! exists to clean up, and a test for a DESTRUCTIVE verb is the last place to rely on "no code path
//! under here reads `$HOME` today".

use std::sync::Arc;

use bpa_sessiond::attach::AttachRegistry;
use bpa_sessiond::persistence::Db;
use bpa_sessiond::protocol::{decode_daemon_reply, encode_client_preamble, encode_frame};
use bpa_sessiond::protocol::{
    ClientPreamble, DaemonReply, FrameDecoder, CLIENT_MAX_VERSION, CLIENT_MIN_VERSION,
};
use bpa_sessiond::protocol::{Frame, Push, Request, Response, SessionLifecycle, SessionMeta};
use bpa_sessiond::pty_supervisor::{Supervisor, SupervisorError};
use bpa_sessiond::socket_server::{serve, ServerDeps};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::Mutex;

// `HOME` is process-global mutable state; `cargo test` runs the tests inside one binary
// concurrently. This lock serializes every `HOME`-touching test in this file against the others —
// mirrors `boot_integration.rs`'s `HOME_LOCK` (and `singleton.rs`'s `ENV_LOCK`), which document the
// identical hazard.
static HOME_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Set `HOME` to `dir` under [`HOME_LOCK`] and return a guard that restores the prior value (and
/// releases the lock) on drop, even across a panic.
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

// ---- wire helpers (mirror the server codec on the client side, same as the other tests/*.rs) ----

async fn send_frame(s: &mut UnixStream, f: &Frame) {
    // `encode_frame` already prepends the u32-LE length prefix — write its output verbatim.
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

/// Drain frames until the response correlated to `id` arrives, skipping interleaved pushes — a
/// `RemoveWorkspace` that kills sessions also broadcasts `ChildExited`/`StateChanged` on the
/// requester's own connection.
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

/// Create a workspace over the wire and drain BOTH resulting frames (the `Response::Workspace` and
/// its `Push::WorkspaceCreated`), so no stray push is left to desync a later id-keyed read.
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

async fn create_session(c: &mut UnixStream, id: u64, workspace_id: &str) -> String {
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
    match recv_response_for(c, id).await {
        Response::Session(m) => m.id,
        other => panic!("CreateSession failed: {other:?}"),
    }
}

#[tokio::test]
async fn remove_workspace_deletes_everything_kills_the_pty_and_broadcasts_workspace_removed() {
    let home = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home.path());

    let dir = tempfile::tempdir().unwrap();
    let sock = dir.path().join("d.sock");
    let listener = UnixListener::bind(&sock).unwrap();
    let (tx, rx) = tokio::sync::watch::channel(false);

    let supervisor = Arc::new(Supervisor::new());
    // In-memory DB: this test never opens a file, so it can never touch the developer's real
    // app-support database (D6) — the very database this feature exists to let a user clean up.
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

    let mut c1 = UnixStream::connect(&sock).await.unwrap();
    preamble_handshake(&mut c1).await;

    // Two workspaces: the doomed one, and a bystander that must come through untouched.
    let doomed = create_workspace(&mut c1, 1, "doomed", "/tmp").await;
    let keeper = create_workspace(&mut c1, 2, "keeper", "/tmp").await;

    // A REAL live session in each — so "the PTY is killed, not orphaned" and "an unrelated
    // workspace's PTY keeps running" are both asserted against actual child processes.
    let live_id = create_session(&mut c1, 3, &doomed).await;
    let keeper_session = create_session(&mut c1, 4, &keeper).await;
    assert!(deps.supervisor.meta(&live_id).unwrap().is_active);
    assert!(deps.supervisor.meta(&keeper_session).unwrap().is_active);

    // Plus a rehydrated (PTY-less, inactive) session in the doomed workspace: the sweep must close
    // that kind honestly too, not just live ones.
    let dead_id = "doomed-rehydrated".to_string();
    let dead_meta = SessionMeta {
        id: dead_id.clone(),
        workspace_id: doomed.clone(),
        title: "t".into(),
        shell: "/bin/sh".into(),
        cwd: "/tmp".into(),
        cols: 80,
        rows: 24,
        lifecycle: SessionLifecycle::Exited {
            code: Some(0),
            signal: None,
        },
        waiting_for_input: false,
        is_active: false,
        created_at: 1_700_000_000,
    };
    {
        let db = deps.db.lock().await;
        db.upsert_session(&dead_meta).unwrap();
        for sid in [&live_id, &dead_id, &keeper_session] {
            db.append_scrollback(sid, 0, b"SCROLLBACK", 1).unwrap();
            db.append_command_event(sid, 0, 1_700_000_000, "started", None, "gui")
                .unwrap();
        }
    }
    deps.supervisor
        .rehydrate_inactive(dead_meta, b"SCROLLBACK".to_vec())
        .expect("rehydrate_inactive");

    // A second, unrelated connection, registered BEFORE the request, observes the broadcast.
    let mut c2 = UnixStream::connect(&sock).await.unwrap();
    preamble_handshake(&mut c2).await;

    send_frame(
        &mut c1,
        &Frame::Request {
            id: 5,
            req: Request::RemoveWorkspace {
                workspace_id: doomed.clone(),
            },
        },
    )
    .await;
    match recv_response_for(&mut c1, 5).await {
        Response::Ack => {}
        other => panic!("RemoveWorkspace must Ack, got {other:?}"),
    }

    // ---- The push: WorkspaceRemoved, on an unrelated connection. Explicitly NOT
    // WorkspaceUpdated — every consumer upserts that payload into its store, so reusing it here
    // would re-insert the workspace the user just deleted. ----
    let mut saw_removed = false;
    for _ in 0..64 {
        match recv_frame_t(&mut c2).await {
            Frame::Push(Push::WorkspaceRemoved { workspace_id }) => {
                assert_eq!(workspace_id, doomed);
                saw_removed = true;
                break;
            }
            Frame::Push(Push::WorkspaceUpdated(w)) if w.id == doomed => {
                panic!("a removal must never emit WorkspaceUpdated for the removed workspace")
            }
            _ => continue,
        }
    }
    assert!(
        saw_removed,
        "every connected client must observe Push::WorkspaceRemoved"
    );

    // ---- The PTYs: the doomed workspace's are gone, the bystander's still runs. ----
    assert!(
        matches!(
            deps.supervisor.meta(&live_id),
            Err(SupervisorError::NoSuchSession(_))
        ),
        "the workspace's live session must be fully killed and reaped, never left running"
    );
    assert!(
        matches!(
            deps.supervisor.meta(&dead_id),
            Err(SupervisorError::NoSuchSession(_))
        ),
        "the workspace's rehydrated (PTY-less) session must be honestly closed too"
    );
    assert!(
        deps.supervisor.meta(&keeper_session).unwrap().is_active,
        "an unrelated workspace's live session must keep running"
    );

    // ---- The rows: workspace, roots, sessions and every dependent row are gone; the bystander's
    // are all intact. ----
    {
        let db = deps.db.lock().await;
        assert!(
            db.list_workspaces().unwrap().iter().all(|w| w.id != doomed),
            "the workspace row must be gone"
        );
        assert!(
            db.workspace_session_ids(&doomed).is_err(),
            "the workspace must no longer exist at all"
        );
        for sid in [&live_id, &dead_id] {
            assert!(
                db.list_sessions().unwrap().iter().all(|m| &m.id != sid),
                "session {sid} row must be gone"
            );
            assert_eq!(
                db.load_scrollback(sid).unwrap(),
                Vec::<u8>::new(),
                "session {sid} scrollback must be gone — no orphans"
            );
            assert!(
                db.list_command_events(sid, 10).unwrap().is_empty(),
                "session {sid} command_events must be gone — no orphans"
            );
        }

        assert!(
            db.list_workspaces().unwrap().iter().any(|w| w.id == keeper),
            "an unrelated workspace must survive"
        );
        assert_eq!(
            db.workspace_session_ids(&keeper).unwrap(),
            vec![keeper_session.clone()]
        );
        assert_eq!(db.load_scrollback(&keeper_session).unwrap(), b"SCROLLBACK");
        assert_eq!(
            db.list_command_events(&keeper_session, 10).unwrap().len(),
            1
        );
    }

    // ---- ListWorkspaces over the wire no longer surfaces it (the sidebar symptom). ----
    send_frame(
        &mut c1,
        &Frame::Request {
            id: 6,
            req: Request::ListWorkspaces,
        },
    )
    .await;
    match recv_response_for(&mut c1, 6).await {
        Response::Workspaces(v) => {
            assert!(v.iter().all(|w| w.id != doomed));
            assert!(v.iter().any(|w| w.id == keeper));
        }
        other => panic!("expected Workspaces, got {other:?}"),
    }

    // ---- A restart-equivalent cold rehydrate must not resurrect any of it. ----
    let fresh = Arc::new(Supervisor::new());
    {
        let db = deps.db.lock().await;
        for m in db.list_sessions().unwrap() {
            let sb = db.load_scrollback(&m.id).unwrap_or_default();
            let _ = fresh.rehydrate_inactive(m, sb);
        }
    }
    for sid in [&live_id, &dead_id] {
        assert!(
            matches!(fresh.meta(sid), Err(SupervisorError::NoSuchSession(_))),
            "a removed workspace's session {sid} must never come back on restart"
        );
    }

    tx.send(true).unwrap();
    let _ = tokio::time::timeout(std::time::Duration::from_secs(5), jh).await;
    deps.supervisor.shutdown_all();
}
