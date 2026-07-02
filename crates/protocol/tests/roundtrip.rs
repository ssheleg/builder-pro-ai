use bpa_protocol::*;

/// bincode 1.3.3 default serialize/deserialize must round-trip every wire type.
fn assert_frame_roundtrip(frame: Frame) {
    let bytes = bincode::serialize(&frame).expect("serialize");
    let back: Frame = bincode::deserialize(&bytes).expect("deserialize");
    assert_eq!(
        bincode::serialize(&back).expect("re-serialize"),
        bytes,
        "frame did not round-trip byte-identically"
    );
}

fn sample_workspace() -> Workspace {
    Workspace {
        id: "ws-1".into(),
        name: "Demo".into(),
        root_path: "/tmp/demo".into(),
    }
}

fn sample_meta(lifecycle: SessionLifecycle) -> SessionMeta {
    SessionMeta {
        id: "sess-1".into(),
        workspace_id: "ws-1".into(),
        title: "zsh".into(),
        shell: "/bin/zsh".into(),
        cwd: "/tmp/demo".into(),
        cols: 80,
        rows: 24,
        lifecycle,
        waiting_for_input: false,
        is_active: true,
        created_at: 1_720_000_000,
    }
}

fn all_lifecycles() -> Vec<SessionLifecycle> {
    vec![
        SessionLifecycle::AtPrompt,
        SessionLifecycle::Typing,
        SessionLifecycle::Running,
        SessionLifecycle::Exited {
            code: Some(0),
            signal: None,
        },
        SessionLifecycle::Exited {
            code: Some(137),
            signal: None,
        },
        SessionLifecycle::Exited {
            code: None,
            signal: Some("SIGKILL".into()),
        },
        SessionLifecycle::Exited {
            code: None,
            signal: None,
        },
    ]
}

fn all_requests() -> Vec<Request> {
    vec![
        Request::Hello {
            magic: MAGIC,
            proto_version: PROTO_VERSION,
            client_build: "test".into(),
        },
        Request::ListWorkspaces,
        Request::CreateWorkspace {
            name: "W".into(),
            root_path: "/tmp/w".into(),
        },
        Request::ListSessions,
        Request::CreateSession {
            workspace_id: "ws-1".into(),
            shell: Some("/bin/bash".into()),
            cwd: Some("/tmp/demo".into()),
            env_overrides: vec![("FOO".into(), "bar".into())],
            cols: 120,
            rows: 40,
        },
        Request::CreateSession {
            workspace_id: "ws-1".into(),
            shell: None,
            cwd: None,
            env_overrides: vec![],
            cols: 80,
            rows: 24,
        },
        Request::AttachSession {
            session_id: "sess-1".into(),
        },
        Request::DetachSession {
            session_id: "sess-1".into(),
        },
        Request::WriteStdin {
            session_id: "sess-1".into(),
            bytes: vec![0, 27, 91, 65, 255],
        },
        Request::Resize {
            session_id: "sess-1".into(),
            cols: 100,
            rows: 30,
        },
        Request::KillSession {
            session_id: "sess-1".into(),
        },
        Request::GetSessionState {
            session_id: "sess-1".into(),
        },
        Request::DaemonShutdown { drain: true },
        Request::DaemonShutdown { drain: false },
    ]
}

fn all_responses() -> Vec<Response> {
    let mut v = vec![
        Response::Welcome {
            proto_version: PROTO_VERSION,
            daemon_build: "d".into(),
        },
        Response::Incompatible { min: 1, max: 1 },
        Response::Workspaces(vec![sample_workspace()]),
        Response::Workspace(sample_workspace()),
        Response::Ack,
        Response::Error {
            code: "InvalidWorkspaceRoot".into(),
            message: "gone".into(),
        },
    ];
    for lc in all_lifecycles() {
        v.push(Response::Sessions(vec![sample_meta(lc.clone())]));
        v.push(Response::Session(sample_meta(lc)));
    }
    v
}

fn all_pushes() -> Vec<Push> {
    let mut v = vec![
        Push::Replay {
            session_id: "sess-1".into(),
            cols: 80,
            rows: 24,
            content: vec![1, 2, 3, 255, 0],
        },
        Push::Output {
            session_id: "sess-1".into(),
            bytes: vec![97, 98, 99],
        },
        Push::ChildExited {
            session_id: "sess-1".into(),
            code: Some(42),
            signal: None,
        },
        Push::ChildExited {
            session_id: "sess-1".into(),
            code: None,
            signal: Some("SIGTERM".into()),
        },
        Push::SessionCreated {
            meta: sample_meta(SessionLifecycle::AtPrompt),
        },
        Push::WorkspaceCreated {
            workspace: sample_workspace(),
        },
        Push::Error {
            session_id: Some("sess-1".into()),
            code: "PtySpawn".into(),
            message: "boom".into(),
        },
        Push::Error {
            session_id: None,
            code: "Internal".into(),
            message: "x".into(),
        },
    ];
    for lc in all_lifecycles() {
        v.push(Push::StateChanged {
            session_id: "sess-1".into(),
            lifecycle: lc,
            waiting_for_input: true,
            cwd: "/tmp/demo".into(),
        });
    }
    v
}

#[test]
fn every_request_variant_roundtrips() {
    for (i, req) in all_requests().into_iter().enumerate() {
        assert_frame_roundtrip(Frame::Request { id: i as u64, req });
    }
}

#[test]
fn every_response_variant_roundtrips() {
    for (i, res) in all_responses().into_iter().enumerate() {
        assert_frame_roundtrip(Frame::Response { id: i as u64, res });
    }
}

#[test]
fn every_push_variant_roundtrips() {
    for push in all_pushes() {
        assert_frame_roundtrip(Frame::Push(push));
    }
}

#[test]
fn every_terminal_event_roundtrips() {
    // TerminalEvent is Serialize-only over Hop A, but must still bincode-round-trip
    // via serde for parity coverage; it derives Deserialize too for symmetry.
    for ev in [
        TerminalEvent::Replay {
            cols: 80,
            rows: 24,
            content: vec![9, 8, 7],
        },
        TerminalEvent::Output {
            bytes: vec![1, 2, 3],
        },
    ] {
        let bytes = bincode::serialize(&ev).expect("serialize");
        let back: TerminalEvent = bincode::deserialize(&bytes).expect("deserialize");
        assert_eq!(bincode::serialize(&back).expect("re"), bytes);
    }
}

#[test]
fn constants_are_locked() {
    assert_eq!(MAGIC, 0x4250_4131);
    assert_eq!(PROTO_VERSION, 1);
}
