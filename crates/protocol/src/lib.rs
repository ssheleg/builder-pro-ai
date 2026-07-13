//! Shared Hop-B wire protocol + Rust⇄TS domain types for Builder Pro AI.
//!
//! Source of truth for `src/ipc/types.ts` (generated via ts-rs; never hand-edited).
//! Codec is CBOR (RFC 8949, via ciborium) for Hop-B; framing lives in `framing.rs`.
//! Every type here derives serde `Serialize`/`Deserialize`; the types that cross
//! into TypeScript also derive `ts_rs::TS`.
//!
//! `SessionLifecycle` (internally tagged, `tag = "kind"`) and `TerminalEvent`
//! (adjacently tagged, `tag = "event", content = "data"`) match the discriminated
//! unions spec §5/§6.2 require in `src/ipc/types.ts`, and derive plainly: CBOR
//! (unlike the bincode codec used before Pv2 §3.1) supports serde's internally- and
//! adjacently-tagged representations natively on both serialize and deserialize, so
//! no hand-written dual-codec shim is needed (Pv2 §3.2 retired the shim + its
//! `*Shape` shadow structs). Their own `///` doc comments below still describe the
//! retired bincode/dual-codec rationale verbatim — that text is copied byte-for-byte
//! into `src/ipc/types.ts` by ts-rs, and the parity test asserts that file is
//! unchanged across this refactor, so it's kept as-is rather than "corrected" out of
//! sync with the generated output. Treat this module doc as the current, accurate
//! account; the per-item docs on those two enums are frozen for output parity.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

mod framing;
pub use framing::{
    encode_cbor_frame, encode_frame, CborFrameDecoder, FrameDecoder, FrameError, MAX_FRAME_LEN,
};

pub mod preamble;
pub use preamble::{
    decode_client_preamble, decode_daemon_reply, encode_client_preamble, encode_daemon_reply,
    negotiate, ClientPreamble, DaemonReply, PreambleError, CLIENT_MAX_VERSION, CLIENT_MIN_VERSION,
    DAEMON_MAX_VERSION, DAEMON_MIN_VERSION, MAX_PREAMBLE_BUILD_LEN, PREAMBLE_MAGIC,
    PREAMBLE_TIMEOUT,
};

pub type SessionId = String; // UUID v4
pub type WorkspaceId = String; // UUID v4

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "types.ts")]
pub struct Workspace {
    pub id: WorkspaceId,
    pub name: String,
    /// Compat mirror, always == roots[0].
    pub root_path: String,
    /// Ordered, equal roots; canonical absolute paths; len >= 1.
    pub roots: Vec<String>,
}

/// Internally tagged on `kind` (tag only, no content) — matches the TS discriminated
/// union in spec §5. Unit variants carry only `{ kind }`; the struct variant `Exited`
/// carries its fields flattened next to the tag.
///
/// See the module-level "Dual-codec note" for why `Serialize`/`Deserialize` are
/// hand-written instead of derived: bincode cannot deserialize an internally-tagged
/// enum directly, but this type also crosses the bincode-framed Hop-B wire (nested in
/// `SessionMeta` / `Push::StateChanged`).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[serde(tag = "kind", rename_all = "camelCase")]
#[ts(export_to = "types.ts")]
pub enum SessionLifecycle {
    /// idle at shell prompt (after OSC 133 B, before C)
    AtPrompt,
    /// NEVER emitted in S1; UI maps to AtPrompt color
    Typing,
    /// command executing (after C, before D)
    Running,
    /// finished; code None = unknown/aborted
    Exited {
        code: Option<u8>,
        signal: Option<String>,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "types.ts")]
pub struct SessionMeta {
    pub id: SessionId,
    pub workspace_id: WorkspaceId,
    pub title: String,
    pub shell: String,
    pub cwd: String,
    pub cols: u16,
    pub rows: u16,
    pub lifecycle: SessionLifecycle,
    pub waiting_for_input: bool,
    pub is_active: bool,
    /// unix seconds. `i64` maps to TS `bigint` by default in ts-rs; overridden to
    /// `number` per spec §5 (safe for unix-second timestamps, well under 2^53).
    #[ts(type = "number")]
    pub created_at: i64,
}

/// Mirrors a `command_events` row (Pv2 §7); ts-rs exported. First consumer is
/// `Request::GetCommandEvents` (spec §3.3), returned newest-first.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "types.ts")]
pub struct CommandEvent {
    pub session_id: SessionId,
    /// monotonic per-session sequence number. `i64` maps to TS `bigint` by
    /// default in ts-rs; overridden to `number` (matches `SessionMeta::created_at`).
    #[ts(type = "number")]
    pub seq: i64,
    /// unix seconds. Overridden to TS `number` (see `seq` above).
    #[ts(type = "number")]
    pub ts: i64,
    /// "started" | "finished" — the exact literals the Pv2 writer persists (pty_supervisor.rs)
    pub kind: String,
    pub exit_code: Option<u8>,
    pub origin: String,
}

/// Hop-A Channel payload (spec §6.2). Adjacently tagged (`event`/`data`).
/// `Vec<u8>` serializes over Tauri IPC as `number[]`; ts-rs emits `Array<number>`.
///
/// Purely a Hop-A (Tauri `Channel<TerminalEvent>`, JSON) type in production — it never
/// appears inside `Frame`/`Request`/`Response`/`Push` (the bincode-framed Hop-B wire;
/// see spec §7). It still gets the same dual-codec `Serialize`/`Deserialize` treatment
/// as `SessionLifecycle` (see the module-level "Dual-codec note") purely so it also
/// round-trips through bincode directly, for test parity / defense in depth.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[serde(tag = "event", content = "data", rename_all = "camelCase")]
#[ts(export_to = "types.ts")]
pub enum TerminalEvent {
    /// FIRST msg on attach; write BEFORE term.open()
    Replay {
        cols: u16,
        rows: u16,
        content: Vec<u8>,
    },
    /// incremental live PTY bytes
    Output { bytes: Vec<u8> },
}

// ---- Hop-B wire frame (core ⇄ daemon). NOT exported to TS. ----

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum Frame {
    /// core → daemon; id correlates the reply
    Request { id: u64, req: Request },
    /// daemon → core; echoes the request id
    Response { id: u64, res: Response },
    /// daemon → core; unsolicited (id-less)
    Push(Push),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum Request {
    ListWorkspaces,
    CreateWorkspace {
        name: String,
        root_path: String,
    },
    ListSessions,
    CreateSession {
        workspace_id: WorkspaceId,
        shell: Option<String>,
        cwd: Option<String>,
        env_overrides: Vec<(String, String)>,
        cols: u16,
        rows: u16,
    },
    AttachSession {
        session_id: SessionId,
    },
    DetachSession {
        session_id: SessionId,
    },
    WriteStdin {
        session_id: SessionId,
        bytes: Vec<u8>,
    },
    Resize {
        session_id: SessionId,
        cols: u16,
        rows: u16,
    },
    KillSession {
        session_id: SessionId,
    },
    GetSessionState {
        session_id: SessionId,
    },
    DaemonShutdown {
        drain: bool,
    },
    AddWorkspaceRoot {
        workspace_id: WorkspaceId,
        path: String,
    },
    RemoveWorkspaceRoot {
        workspace_id: WorkspaceId,
        path: String,
    },
    GetCommandEvents {
        session_id: SessionId,
        limit: u32,
    },
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum Response {
    Workspaces(Vec<Workspace>),
    Workspace(Workspace),
    Sessions(Vec<SessionMeta>),
    Session(SessionMeta),
    Ack,
    Error { code: String, message: String },
    CommandEvents(Vec<CommandEvent>),
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum Push {
    Replay {
        session_id: SessionId,
        cols: u16,
        rows: u16,
        content: Vec<u8>,
    },
    Output {
        session_id: SessionId,
        bytes: Vec<u8>,
    },
    StateChanged {
        session_id: SessionId,
        lifecycle: SessionLifecycle,
        waiting_for_input: bool,
        cwd: String,
    },
    ChildExited {
        session_id: SessionId,
        code: Option<u8>,
        signal: Option<String>,
    },
    SessionCreated {
        meta: SessionMeta,
    },
    WorkspaceCreated {
        workspace: Workspace,
    },
    Error {
        session_id: Option<SessionId>,
        code: String,
        message: String,
    },
    /// emitted after Add/RemoveWorkspaceRoot
    WorkspaceUpdated(Workspace),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preamble_constants_match_spec() {
        // "BPAA" ASCII, big-endian reading order (Pv2 §4.2); on the wire the u32 is
        // encoded little-endian, so the raw bytes are b"AAPB".
        assert_eq!(PREAMBLE_MAGIC, 0x4250_4141);
        assert_eq!(&PREAMBLE_MAGIC.to_be_bytes(), b"BPAA");
        assert_eq!(&PREAMBLE_MAGIC.to_le_bytes(), b"AAPB");
        // v3 (S2, `[0.3.0]`): multi-root Workspace + new verbs are a planned wire
        // break from v2 — see `preamble.rs`'s "Version history" doc.
        assert_eq!(CLIENT_MIN_VERSION, 3);
        assert_eq!(CLIENT_MAX_VERSION, 3);
        assert_eq!(DAEMON_MIN_VERSION, 3);
        assert_eq!(DAEMON_MAX_VERSION, 3);
        assert_eq!(MAX_PREAMBLE_BUILD_LEN, 256);
    }
}
