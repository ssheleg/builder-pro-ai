//! Integration: boot the daemon `run()` on a temp socket, complete the handshake, Ping/Pong,
//! trigger clean shutdown, and prove the boot-time schema/global-ruleset invariants (spec §5,
//! §5.1, §5.2). Mirrors `bpa_sessiond`'s `tests/boot_integration.rs` shape (minus PTY/session
//! concerns).

use std::path::Path;
use std::time::Duration;

use bpa_orchd::protocol::{
    encode_orchd_frame, OrchdFrame, OrchdFrameDecoder, OrchdRequest, OrchdResponse,
    ORCHD_CLIENT_MAX_VERSION, ORCHD_CLIENT_MIN_VERSION, ORCHD_DAEMON_MAX_VERSION,
};
use bpa_protocol::{decode_daemon_reply, encode_client_preamble, ClientPreamble, DaemonReply};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

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

// `boot::run` resolves its on-disk DB/rules path from `$HOME` (see `boot::app_support_dir`), and
// `HOME` is process-global mutable state. Every test in this file that boots the real daemon core
// must isolate `HOME` under its own tempdir (D6: never touch the developer's real app-support
// DB); `cargo test` runs tests from the same file/binary concurrently by default, so tests
// mutating `HOME` at once would race each other's set/read/restore sequence. This lock serializes
// them — mirrors sessiond's `HOME_LOCK`/`singleton.rs`'s `ENV_LOCK`, which document the identical
// hazard.
static HOME_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// Set `HOME` to `dir` under [`HOME_LOCK`] and return a guard that restores the prior value (and
/// releases the lock) on drop.
struct HomeGuard {
    _lock: std::sync::MutexGuard<'static, ()>,
    prior: Option<std::ffi::OsString>,
}

impl HomeGuard {
    fn set(dir: &Path) -> Self {
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

/// Perform the preamble handshake using the orchd wire's own `ORCHD_CLIENT_MIN/MAX_VERSION`
/// consts (never hardcoded literals, so this can never desync from a version bump) and return the
/// daemon's chosen version.
async fn preamble_handshake(s: &mut UnixStream) -> u16 {
    let bytes = encode_client_preamble(&ClientPreamble {
        min: ORCHD_CLIENT_MIN_VERSION,
        max: ORCHD_CLIENT_MAX_VERSION,
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
        DaemonReply::Accepted { chosen, .. } => chosen,
        other => panic!("expected Accepted, got {other:?}"),
    }
}

/// Wait for `socket` to appear and accept a connection (the daemon binds it asynchronously after
/// `run()` is spawned).
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

#[tokio::test]
async fn boot_handshake_ping_pong_and_clean_shutdown() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");

    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());
    assert_eq!(
        bpa_orchd::app_support_dir_for_test(),
        home_dir
            .path()
            .join("Library/Application Support/ai.builderpro.desktop"),
        "HOME isolation must actually redirect the daemon's app-support/DB path under the tempdir"
    );

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let socket_for_task = socket.clone();
    let shutdown_tx_for_task = shutdown_tx.clone();
    let boot = tokio::spawn(async move {
        bpa_orchd::run(socket_for_task, shutdown_tx_for_task, shutdown_rx).await
    });

    let mut c = connect_when_ready(&socket).await;

    let chosen = preamble_handshake(&mut c).await;
    assert_eq!(
        chosen, ORCHD_DAEMON_MAX_VERSION,
        "a client speaking exactly the daemon's own range must be offered the daemon's max"
    );

    send_frame(
        &mut c,
        &OrchdFrame::Request {
            id: 1,
            req: OrchdRequest::Ping,
        },
    )
    .await;
    match recv_frame(&mut c).await {
        OrchdFrame::Response {
            id: 1,
            res: OrchdResponse::Pong,
        } => {}
        other => panic!("expected Pong, got {other:?}"),
    }

    // Real domain dispatch (T10): a fresh boot's project list is honestly empty, not a stub error.
    send_frame(
        &mut c,
        &OrchdFrame::Request {
            id: 2,
            req: OrchdRequest::ListProjects,
        },
    )
    .await;
    match recv_frame(&mut c).await {
        OrchdFrame::Response {
            id: 2,
            res: OrchdResponse::Projects(projects),
        } => {
            assert!(projects.is_empty(), "a fresh boot has no projects yet");
        }
        other => panic!("expected Projects([]), got {other:?}"),
    }

    // Real OrchdShutdown{drain:true} semantics: Ack over the wire, then `run()` returns and the
    // socket is unlinked.
    send_frame(
        &mut c,
        &OrchdFrame::Request {
            id: 3,
            req: OrchdRequest::OrchdShutdown { drain: true },
        },
    )
    .await;
    match recv_frame(&mut c).await {
        OrchdFrame::Response {
            id: 3,
            res: OrchdResponse::Ack,
        } => {}
        other => panic!("expected Ack for OrchdShutdown, got {other:?}"),
    }

    let res = tokio::time::timeout(Duration::from_secs(5), boot)
        .await
        .expect("run() did not return after OrchdShutdown")
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
    let lockfile = dir.path().join("orchd.lock");

    let g1 = bpa_daemon_core::singleton::acquire_lock_at(&lockfile)
        .expect("first lock acquisition succeeds");
    let err = bpa_daemon_core::singleton::acquire_lock_at(&lockfile)
        .expect_err("second lock acquisition on the same file must fail");
    assert_eq!(err.kind(), std::io::ErrorKind::WouldBlock);
    drop(g1);
}

#[tokio::test]
async fn fresh_boot_creates_schema_v1_and_global_ruleset() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
    let socket_for_task = socket.clone();
    let shutdown_tx_for_task = shutdown_tx.clone();
    let boot = tokio::spawn(async move {
        bpa_orchd::run(socket_for_task, shutdown_tx_for_task, shutdown_rx).await
    });

    let _c = connect_when_ready(&socket).await;

    shutdown_tx.send(true).unwrap();
    let res = tokio::time::timeout(Duration::from_secs(5), boot)
        .await
        .expect("run() did not return after shutdown")
        .expect("join");
    assert!(res.is_ok(), "run() returned error: {res:?}");

    // Reopen the DB directly (the daemon's own connection closed with `run()`) and assert the
    // spec §5.1 schema landed.
    let db_path = bpa_orchd::app_support_dir_for_test().join("orchd.db");
    let db = bpa_orchd::persistence::Db::open(&db_path).expect("reopen orchd.db");

    let user_version: i64 = db
        .conn()
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(user_version, 1);

    for table in [
        "project",
        "project_workspace",
        "goal",
        "idea",
        "insight",
        "task",
        "ruleset",
    ] {
        let exists: bool = db
            .conn()
            .query_row(
                "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = ?1",
                [table],
                |_| Ok(true),
            )
            .unwrap_or(false);
        assert!(exists, "missing table {table}");
    }

    // FK effective: a `goal` row referencing a nonexistent `project` must be rejected.
    let fk_err = db.conn().execute(
        "INSERT INTO goal (id, project_id, kind, title, created_at, updated_at)
         VALUES ('g1', 'no-such-project', 'strategic', 't', 0, 0)",
        [],
    );
    assert!(
        fk_err.is_err(),
        "foreign_keys=ON must reject a goal row referencing a nonexistent project"
    );

    // Global ruleset row, ensured at boot.
    let (scope, project_id, policy): (String, Option<String>, String) = db
        .conn()
        .query_row(
            "SELECT scope, project_id, policy FROM ruleset WHERE scope = 'global'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .expect("global ruleset row must exist after boot");
    assert_eq!(scope, "global");
    assert_eq!(project_id, None);
    assert_eq!(policy, "{}");

    // `rules/global.md` on disk with the locked template content.
    let md_path = bpa_orchd::app_support_dir_for_test().join("rules/global.md");
    let content = std::fs::read_to_string(&md_path).expect("rules/global.md must exist");
    assert_eq!(content, "# Глобальные правила\n");
}

#[tokio::test]
async fn double_boot_does_not_duplicate_global_ruleset_row() {
    let dir = tempfile::tempdir().unwrap();
    let socket = dir.path().join("orchd.sock");
    let home_dir = tempfile::tempdir().unwrap();
    let _home_guard = HomeGuard::set(home_dir.path());

    for _ in 0..2 {
        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);
        let socket_for_task = socket.clone();
        let shutdown_tx_for_task = shutdown_tx.clone();
        let boot = tokio::spawn(async move {
            bpa_orchd::run(socket_for_task, shutdown_tx_for_task, shutdown_rx).await
        });

        let _c = connect_when_ready(&socket).await;

        shutdown_tx.send(true).unwrap();
        let res = tokio::time::timeout(Duration::from_secs(5), boot)
            .await
            .expect("run() did not return after shutdown")
            .expect("join");
        assert!(res.is_ok(), "run() returned error: {res:?}");
    }

    let db_path = bpa_orchd::app_support_dir_for_test().join("orchd.db");
    let db = bpa_orchd::persistence::Db::open(&db_path).expect("reopen orchd.db");
    let count: i64 = db
        .conn()
        .query_row(
            "SELECT COUNT(*) FROM ruleset WHERE scope = 'global'",
            [],
            |r| r.get(0),
        )
        .unwrap();
    assert_eq!(
        count, 1,
        "a second boot against the same app-support dir must not duplicate the global ruleset row"
    );
}
