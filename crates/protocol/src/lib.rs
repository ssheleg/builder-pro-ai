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
/// Poison-tolerant `std::sync` lock acquisition (BL-124) — plain functions, no wire/TS surface.
pub mod sync;
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
    /// → `Response::Ack` + broadcasts [`Push::WorkspaceRemoved`]. Permanently and TOTALLY removes
    /// one workspace: its `workspace_root` rows, EVERY session that belongs to it (the live PTY is
    /// killed through the same `KillSession` machinery first, so a removal can never leave an
    /// orphaned/zombie child), those sessions' `session` rows, and their dependent `scrollback` +
    /// `command_events` rows — all of it, in one transaction (`Db::delete_workspace`). There is no
    /// soft-delete and no "detach only" mode: the verb exists because a workspace whose roots have
    /// been deleted off disk was previously UNDELETABLE, so it must actually delete.
    ///
    /// An unknown `workspace_id` ⇒ the SAME not-found error shape the other workspace verbs already
    /// return for an unknown id (`RemoveWorkspaceRoot`'s `PersistError::Sql`, wire code `"DbSql"`,
    /// message `db sql error: workspace <id> not found`) — deliberately mirrored rather than given a
    /// new code, so a client needs no new error handling for this verb.
    ///
    /// Appended at the enum TAIL (append-only wire rule — same rule `bpa-orchd-proto` states in its
    /// module doc: variant order is FROZEN, new verbs only ever go last, so a peer decoding an
    /// older/newer build never re-reads one verb as another).
    RemoveWorkspace {
        workspace_id: WorkspaceId,
    },
}

impl Request {
    /// A stable, low-cardinality `&'static str` name for this request's variant — the ONLY
    /// request-derived value allowed into the daemon's structured completion-trace field
    /// (spec D4, O-6), consumed by the single per-verb choke-point in `socket_server::dispatch`.
    ///
    /// The match is deliberately **exhaustive with no `_` wildcard**: a future `Request` variant
    /// fails to compile until named here, so a new verb can never ship silently untraced. Fields
    /// are matched with `{ .. }` and never bound, so no payload value (notably `WriteStdin`'s raw
    /// terminal `bytes`) can ever be captured into the name — a completion trace carries verb +
    /// outcome + error_code + elapsed only.
    pub fn verb_name(&self) -> &'static str {
        match self {
            Self::ListWorkspaces => "ListWorkspaces",
            Self::CreateWorkspace { .. } => "CreateWorkspace",
            Self::ListSessions => "ListSessions",
            Self::CreateSession { .. } => "CreateSession",
            Self::AttachSession { .. } => "AttachSession",
            Self::DetachSession { .. } => "DetachSession",
            Self::WriteStdin { .. } => "WriteStdin",
            Self::Resize { .. } => "Resize",
            Self::KillSession { .. } => "KillSession",
            Self::GetSessionState { .. } => "GetSessionState",
            Self::DaemonShutdown { .. } => "DaemonShutdown",
            Self::AddWorkspaceRoot { .. } => "AddWorkspaceRoot",
            Self::RemoveWorkspaceRoot { .. } => "RemoveWorkspaceRoot",
            Self::GetCommandEvents { .. } => "GetCommandEvents",
            Self::RemoveWorkspace { .. } => "RemoveWorkspace",
        }
    }
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
    /// Emitted (broadcast to EVERY connected client) after a successful [`Request::RemoveWorkspace`].
    ///
    /// This is deliberately NOT [`Push::WorkspaceUpdated`]: that variant carries a whole
    /// `Workspace` and every consumer of it *upserts* that payload into its store (see
    /// `src-tauri`'s `map_push` → `workspace://updated`), so reusing it for a removal would
    /// re-insert the very workspace the user just deleted — the daemon would be lying about what
    /// happened. A removal has no surviving `Workspace` value to send, only an id, so it gets its
    /// own variant carrying exactly that.
    ///
    /// Appended at the enum TAIL (append-only wire rule), like [`Request::RemoveWorkspace`].
    WorkspaceRemoved {
        workspace_id: WorkspaceId,
    },
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

    /// `verb_name`'s match is compile-enforced exhaustive, so a new verb cannot ship UNnamed —
    /// but nothing checks the string itself, and the daemon's per-request completion trace (spec
    /// D4/O-6) is keyed on it. Pin the new verb's name, and pin a few pre-existing ones to catch
    /// an append that accidentally disturbed them.
    #[test]
    fn remove_workspace_has_a_stable_trace_verb_name() {
        assert_eq!(
            Request::RemoveWorkspace {
                workspace_id: "ws-1".into()
            }
            .verb_name(),
            "RemoveWorkspace"
        );
        assert_eq!(Request::ListWorkspaces.verb_name(), "ListWorkspaces");
        assert_eq!(
            Request::KillSession {
                session_id: "sess-1".into()
            }
            .verb_name(),
            "KillSession"
        );
        assert_eq!(
            Request::RemoveWorkspaceRoot {
                workspace_id: "ws-1".into(),
                path: "/tmp".into()
            }
            .verb_name(),
            "RemoveWorkspaceRoot"
        );
    }

    /// The append-only rule's payoff, asserted rather than assumed: ciborium encodes an
    /// externally-tagged enum by variant NAME, not by index, so appending `RemoveWorkspace` /
    /// `WorkspaceRemoved` at the TAIL is wire-transparent — a peer built before the append still
    /// reads every pre-existing verb as itself, never as its neighbour.
    #[test]
    fn appending_a_tail_variant_cannot_renumber_the_existing_ones() {
        for (frame, name) in [
            (
                Frame::Request {
                    id: 7,
                    req: Request::ListWorkspaces,
                },
                "ListWorkspaces",
            ),
            (
                Frame::Request {
                    id: 8,
                    req: Request::KillSession {
                        session_id: "sess-1".into(),
                    },
                },
                "KillSession",
            ),
            (
                Frame::Push(Push::WorkspaceCreated {
                    workspace: Workspace {
                        id: "ws-1".into(),
                        name: "W".into(),
                        root_path: "/tmp".into(),
                        roots: vec!["/tmp".into()],
                    },
                }),
                "WorkspaceCreated",
            ),
        ] {
            let bytes = encode_frame(&frame).expect("encode");
            let text = String::from_utf8_lossy(&bytes).into_owned();
            assert!(
                text.contains(name),
                "{name} must still be encoded by its own name (index-based encoding would make \
                 a tail append a silent wire break); got {bytes:?}"
            );
        }
    }
}
