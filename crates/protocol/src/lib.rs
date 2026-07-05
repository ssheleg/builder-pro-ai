//! Shared Hop-B wire protocol + Rust⇄TS domain types for Builder Pro AI.
//!
//! Source of truth for `src/ipc/types.ts` (generated via ts-rs; never hand-edited).
//! Codec is bincode 1.3.3 (fixint, little-endian, deterministic). Framing lives in
//! `framing.rs`. Every type here implements serde `Serialize`/`Deserialize` (derived,
//! except `SessionLifecycle`/`TerminalEvent` — see the note below); the types that
//! cross into TypeScript also derive `ts_rs::TS`.
//!
//! ## Dual-codec note (`SessionLifecycle` / `TerminalEvent`)
//!
//! Spec §5/§6.2 mandate that these two enums serialize as internally-/adjacently-
//! tagged JSON over Hop-A (`{"kind":"atPrompt"}`, `{"event":"replay","data":{...}}`),
//! which is also the shape `ts-rs` must generate for `src/ipc/types.ts`. But bincode
//! 1.3.3's `Deserializer` does not implement `deserialize_any`/`deserialize_identifier`,
//! so it cannot deserialize serde's internally- or adjacently-tagged representations
//! (`#[serde(tag = "..")]` / `#[serde(tag = "..", content = "..")]`) even though it CAN
//! serialize them — the round trip is asymmetric and fails on the way back. Since
//! `SessionLifecycle` also crosses Hop-B (embedded in `SessionMeta` and
//! `Push::StateChanged`, both bincode-framed per spec §7), it needs a wire
//! representation that works under both codecs from a single `Serialize`/
//! `Deserialize` impl.
//!
//! The fix: both enums get a **hand-written** `Serialize`/`Deserialize` that branches
//! on `Serializer::is_human_readable()` / `Deserializer::is_human_readable()`:
//! - human-readable (`serde_json`, i.e. Hop-A / Tauri IPC): delegates to a private
//!   shadow enum carrying the real `#[serde(tag = .., rename_all = ..)]` derive, so
//!   the JSON shape is byte-for-byte what spec §5/§6.2 specify.
//! - non-human-readable (`bincode`, i.e. Hop-B): serializes the shadow enum to a JSON
//!   *string* and writes that string (a plain bincode-native type), then reverses this
//!   on deserialize. This keeps the wire format symmetric under bincode without
//!   changing the JSON shape at all.
//!
//! `#[ts(tag = .., rename_all = ..)]` (ts-rs's own attribute namespace, independent of
//! serde-compat) is applied directly to the public enums, so the generated TypeScript
//! union is unaffected by this workaround and matches spec §5/§6.2 exactly.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use ts_rs::TS;

mod framing;
pub use framing::{encode_frame, FrameDecoder, FrameError, MAX_FRAME_LEN};

/// Hop-B handshake magic — ASCII "BPA1". Locked (spec §7 / Global Constraints).
pub const MAGIC: u32 = 0x4250_4131;
/// Hop-B protocol version. Locked (spec §7 / Global Constraints).
pub const PROTO_VERSION: u16 = 1;

pub type SessionId = String; // UUID v4
pub type WorkspaceId = String; // UUID v4

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export_to = "types.ts")]
pub struct Workspace {
    pub id: WorkspaceId,
    pub name: String,
    pub root_path: String,
}

/// Internally tagged on `kind` (tag only, no content) — matches the TS discriminated
/// union in spec §5. Unit variants carry only `{ kind }`; the struct variant `Exited`
/// carries its fields flattened next to the tag.
///
/// See the module-level "Dual-codec note" for why `Serialize`/`Deserialize` are
/// hand-written instead of derived: bincode cannot deserialize an internally-tagged
/// enum directly, but this type also crosses the bincode-framed Hop-B wire (nested in
/// `SessionMeta` / `Push::StateChanged`).
#[derive(Clone, Debug, PartialEq, TS)]
#[ts(tag = "kind", rename_all = "camelCase")]
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

/// Private shadow of [`SessionLifecycle`] carrying the real serde tag derive; used
/// only by the hand-written `Serialize`/`Deserialize` impls below (see the
/// module-level "Dual-codec note").
#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum SessionLifecycleShape {
    AtPrompt,
    Typing,
    Running,
    Exited {
        code: Option<u8>,
        signal: Option<String>,
    },
}

impl From<&SessionLifecycle> for SessionLifecycleShape {
    fn from(v: &SessionLifecycle) -> Self {
        match v {
            SessionLifecycle::AtPrompt => SessionLifecycleShape::AtPrompt,
            SessionLifecycle::Typing => SessionLifecycleShape::Typing,
            SessionLifecycle::Running => SessionLifecycleShape::Running,
            SessionLifecycle::Exited { code, signal } => SessionLifecycleShape::Exited {
                code: *code,
                signal: signal.clone(),
            },
        }
    }
}

impl From<SessionLifecycleShape> for SessionLifecycle {
    fn from(v: SessionLifecycleShape) -> Self {
        match v {
            SessionLifecycleShape::AtPrompt => SessionLifecycle::AtPrompt,
            SessionLifecycleShape::Typing => SessionLifecycle::Typing,
            SessionLifecycleShape::Running => SessionLifecycle::Running,
            SessionLifecycleShape::Exited { code, signal } => {
                SessionLifecycle::Exited { code, signal }
            }
        }
    }
}

impl Serialize for SessionLifecycle {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let shape: SessionLifecycleShape = self.into();
        if serializer.is_human_readable() {
            shape.serialize(serializer)
        } else {
            let json = serde_json::to_string(&shape).map_err(serde::ser::Error::custom)?;
            serializer.serialize_str(&json)
        }
    }
}

impl<'de> Deserialize<'de> for SessionLifecycle {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if deserializer.is_human_readable() {
            let shape = SessionLifecycleShape::deserialize(deserializer)?;
            Ok(shape.into())
        } else {
            let s = String::deserialize(deserializer)?;
            let shape: SessionLifecycleShape =
                serde_json::from_str(&s).map_err(serde::de::Error::custom)?;
            Ok(shape.into())
        }
    }
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

/// Hop-A Channel payload (spec §6.2). Adjacently tagged (`event`/`data`).
/// `Vec<u8>` serializes over Tauri IPC as `number[]`; ts-rs emits `Array<number>`.
///
/// Purely a Hop-A (Tauri `Channel<TerminalEvent>`, JSON) type in production — it never
/// appears inside `Frame`/`Request`/`Response`/`Push` (the bincode-framed Hop-B wire;
/// see spec §7). It still gets the same dual-codec `Serialize`/`Deserialize` treatment
/// as `SessionLifecycle` (see the module-level "Dual-codec note") purely so it also
/// round-trips through bincode directly, for test parity / defense in depth.
#[derive(Clone, Debug, PartialEq, TS)]
#[ts(tag = "event", content = "data", rename_all = "camelCase")]
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

/// Private shadow of [`TerminalEvent`] carrying the real serde tag/content derive;
/// used only by the hand-written `Serialize`/`Deserialize` impls below.
#[derive(Serialize, Deserialize)]
#[serde(tag = "event", content = "data", rename_all = "camelCase")]
enum TerminalEventShape {
    Replay {
        cols: u16,
        rows: u16,
        content: Vec<u8>,
    },
    Output {
        bytes: Vec<u8>,
    },
}

impl From<&TerminalEvent> for TerminalEventShape {
    fn from(v: &TerminalEvent) -> Self {
        match v {
            TerminalEvent::Replay {
                cols,
                rows,
                content,
            } => TerminalEventShape::Replay {
                cols: *cols,
                rows: *rows,
                content: content.clone(),
            },
            TerminalEvent::Output { bytes } => TerminalEventShape::Output {
                bytes: bytes.clone(),
            },
        }
    }
}

impl From<TerminalEventShape> for TerminalEvent {
    fn from(v: TerminalEventShape) -> Self {
        match v {
            TerminalEventShape::Replay {
                cols,
                rows,
                content,
            } => TerminalEvent::Replay {
                cols,
                rows,
                content,
            },
            TerminalEventShape::Output { bytes } => TerminalEvent::Output { bytes },
        }
    }
}

impl Serialize for TerminalEvent {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let shape: TerminalEventShape = self.into();
        if serializer.is_human_readable() {
            shape.serialize(serializer)
        } else {
            let json = serde_json::to_string(&shape).map_err(serde::ser::Error::custom)?;
            serializer.serialize_str(&json)
        }
    }
}

impl<'de> Deserialize<'de> for TerminalEvent {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        if deserializer.is_human_readable() {
            let shape = TerminalEventShape::deserialize(deserializer)?;
            Ok(shape.into())
        } else {
            let s = String::deserialize(deserializer)?;
            let shape: TerminalEventShape =
                serde_json::from_str(&s).map_err(serde::de::Error::custom)?;
            Ok(shape.into())
        }
    }
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
    Hello {
        magic: u32,
        proto_version: u16,
        client_build: String,
    },
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
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub enum Response {
    Welcome {
        proto_version: u16,
        daemon_build: String,
    },
    Incompatible {
        min: u16,
        max: u16,
    },
    Workspaces(Vec<Workspace>),
    Workspace(Workspace),
    Sessions(Vec<SessionMeta>),
    Session(SessionMeta),
    Ack,
    Error {
        code: String,
        message: String,
    },
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wire_constants_match_spec() {
        // "BPA1" big-endian ASCII: 0x42='B',0x50='P',0x41='A',0x31='1'.
        assert_eq!(MAGIC, 0x4250_4131);
        assert_eq!(&MAGIC.to_be_bytes(), b"BPA1");
        assert_eq!(PROTO_VERSION, 1);
    }
}
