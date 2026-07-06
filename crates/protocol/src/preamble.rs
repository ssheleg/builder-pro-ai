//! Codec-agnostic handshake preamble (Pv2 §4.2).
//!
//! v1's handshake was itself a bincode `Request::Hello` frame: a peer speaking a
//! different codec could never decode it, so it could never even reply
//! `Incompatible`. Pv2 fixes this by making the *first* bytes on a connection a
//! fixed, codec-independent preamble — encoded here with raw little-endian
//! primitives, not CBOR/bincode/any self-describing format. Only after a
//! version is agreed does the CBOR frame stream (see `framing.rs`) begin.
//!
//! Wire format (all integers little-endian):
//!
//! - Client preamble: `magic:u32 | min:u16 | max:u16 | build_len:u16 | build[build_len]`
//! - Daemon reply, accepted: `magic:u32 | result:u8=1 | chosen:u16 | build_len:u16 | build[build_len]`
//! - Daemon reply, incompatible: `magic:u32 | result:u8=0 | daemon_min:u16 | daemon_max:u16`
//!
//! This module provides only the pure byte↔struct codec and the `negotiate()`
//! rule, which is what's unit-tested here. The async socket readers (which own
//! the connection and its read timeout) live per-side in the daemon and core
//! crates and read the fixed-size header first, then exactly `build_len` more
//! bytes for the trailing `build` string.

use std::fmt;

/// Handshake magic — ASCII "BPAA" read big-endian; encoded little-endian on the
/// wire (raw bytes `b"AAPB"`). Locked (Pv2 §4.2).
pub const PREAMBLE_MAGIC: u32 = 0x4250_4141;

/// Lowest protocol version this client build can speak.
pub const CLIENT_MIN_VERSION: u16 = 2;
/// Highest protocol version this client build can speak.
pub const CLIENT_MAX_VERSION: u16 = 2;
/// Lowest protocol version this daemon build can speak.
pub const DAEMON_MIN_VERSION: u16 = 2;
/// Highest protocol version this daemon build can speak.
pub const DAEMON_MAX_VERSION: u16 = 2;

/// Hard cap on the trailing `build` string length, in bytes. A larger declared
/// `build_len` is treated as garbage/DoS and rejected rather than allocated/read.
pub const MAX_PREAMBLE_BUILD_LEN: usize = 256;

/// Hard bound on how long either side of a connection will wait for the peer's
/// preamble bytes before giving up and closing the connection. A stuck or
/// malicious peer that writes a partial/garbage preamble and then goes silent
/// must not be able to hang a server task or client connect call forever
/// (Pv2 §4.4: fail closed, not open). Applies to both the client read of the
/// daemon's reply and the daemon read of the client's preamble.
pub const PREAMBLE_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Client → daemon preamble (first bytes on every connection).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ClientPreamble {
    pub min: u16,
    pub max: u16,
    pub build: String,
}

/// Daemon → client preamble reply.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DaemonReply {
    /// A mutually supported version was found; `chosen` is the version both
    /// sides will speak for the CBOR frame stream that follows.
    Accepted { chosen: u16, build: String },
    /// No overlap between the client's `[min, max]` and the daemon's; carries
    /// the daemon's own supported range so the client can report a useful error.
    Incompatible { min: u16, max: u16 },
}

/// Errors decoding a preamble (client or daemon side) from raw bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreambleError {
    /// Leading `magic` field didn't match `PREAMBLE_MAGIC`.
    BadMagic,
    /// Declared `build_len` exceeds `MAX_PREAMBLE_BUILD_LEN`.
    BuildTooLong,
    /// Not enough bytes to decode the fixed header (or the declared `build`).
    ShortRead,
    /// Bytes present but not a well-formed preamble (e.g. non-UTF-8 `build`, or
    /// an unrecognized `result` discriminant in a daemon reply).
    Malformed,
}

impl fmt::Display for PreambleError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PreambleError::BadMagic => write!(f, "preamble magic mismatch"),
            PreambleError::BuildTooLong => {
                write!(
                    f,
                    "preamble build string exceeds {MAX_PREAMBLE_BUILD_LEN} bytes"
                )
            }
            PreambleError::ShortRead => write!(f, "preamble buffer too short"),
            PreambleError::Malformed => write!(f, "preamble malformed"),
        }
    }
}

impl std::error::Error for PreambleError {}

const HEADER_LEN: usize = 4 + 2 + 2 + 2; // magic + min + max + build_len
const REPLY_ACCEPTED_HEADER_LEN: usize = 4 + 1 + 2 + 2; // magic + result + chosen + build_len
const REPLY_INCOMPATIBLE_LEN: usize = 4 + 1 + 2 + 2; // magic + result + daemon_min + daemon_max

/// Encode a `ClientPreamble` as `magic | min | max | build_len | build[..]`, all LE.
pub fn encode_client_preamble(p: &ClientPreamble) -> Vec<u8> {
    let build_bytes = p.build.as_bytes();
    let mut out = Vec::with_capacity(HEADER_LEN + build_bytes.len());
    out.extend_from_slice(&PREAMBLE_MAGIC.to_le_bytes());
    out.extend_from_slice(&p.min.to_le_bytes());
    out.extend_from_slice(&p.max.to_le_bytes());
    out.extend_from_slice(&(build_bytes.len() as u16).to_le_bytes());
    out.extend_from_slice(build_bytes);
    out
}

/// Decode a `ClientPreamble` from raw bytes per the wire format above.
pub fn decode_client_preamble(bytes: &[u8]) -> Result<ClientPreamble, PreambleError> {
    if bytes.len() < HEADER_LEN {
        return Err(PreambleError::ShortRead);
    }
    let magic = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
    if magic != PREAMBLE_MAGIC {
        return Err(PreambleError::BadMagic);
    }
    let min = u16::from_le_bytes(bytes[4..6].try_into().unwrap());
    let max = u16::from_le_bytes(bytes[6..8].try_into().unwrap());
    let build_len = u16::from_le_bytes(bytes[8..10].try_into().unwrap()) as usize;
    if build_len > MAX_PREAMBLE_BUILD_LEN {
        return Err(PreambleError::BuildTooLong);
    }
    if bytes.len() < HEADER_LEN + build_len {
        return Err(PreambleError::ShortRead);
    }
    let build = String::from_utf8(bytes[HEADER_LEN..HEADER_LEN + build_len].to_vec())
        .map_err(|_| PreambleError::Malformed)?;
    Ok(ClientPreamble { min, max, build })
}

/// Encode a `DaemonReply` per the wire format above (Accepted vs Incompatible
/// have distinct fixed layouts, discriminated by the `result` byte).
pub fn encode_daemon_reply(r: &DaemonReply) -> Vec<u8> {
    match r {
        DaemonReply::Accepted { chosen, build } => {
            let build_bytes = build.as_bytes();
            let mut out = Vec::with_capacity(REPLY_ACCEPTED_HEADER_LEN + build_bytes.len());
            out.extend_from_slice(&PREAMBLE_MAGIC.to_le_bytes());
            out.push(1u8);
            out.extend_from_slice(&chosen.to_le_bytes());
            out.extend_from_slice(&(build_bytes.len() as u16).to_le_bytes());
            out.extend_from_slice(build_bytes);
            out
        }
        DaemonReply::Incompatible { min, max } => {
            let mut out = Vec::with_capacity(REPLY_INCOMPATIBLE_LEN);
            out.extend_from_slice(&PREAMBLE_MAGIC.to_le_bytes());
            out.push(0u8);
            out.extend_from_slice(&min.to_le_bytes());
            out.extend_from_slice(&max.to_le_bytes());
            out
        }
    }
}

/// Decode a `DaemonReply` from raw bytes per the wire format above.
pub fn decode_daemon_reply(bytes: &[u8]) -> Result<DaemonReply, PreambleError> {
    if bytes.len() < 4 + 1 {
        return Err(PreambleError::ShortRead);
    }
    let magic = u32::from_le_bytes(bytes[0..4].try_into().unwrap());
    if magic != PREAMBLE_MAGIC {
        return Err(PreambleError::BadMagic);
    }
    let result = bytes[4];
    match result {
        1 => {
            if bytes.len() < REPLY_ACCEPTED_HEADER_LEN {
                return Err(PreambleError::ShortRead);
            }
            let chosen = u16::from_le_bytes(bytes[5..7].try_into().unwrap());
            let build_len = u16::from_le_bytes(bytes[7..9].try_into().unwrap()) as usize;
            if build_len > MAX_PREAMBLE_BUILD_LEN {
                return Err(PreambleError::BuildTooLong);
            }
            if bytes.len() < REPLY_ACCEPTED_HEADER_LEN + build_len {
                return Err(PreambleError::ShortRead);
            }
            let build = String::from_utf8(
                bytes[REPLY_ACCEPTED_HEADER_LEN..REPLY_ACCEPTED_HEADER_LEN + build_len].to_vec(),
            )
            .map_err(|_| PreambleError::Malformed)?;
            Ok(DaemonReply::Accepted { chosen, build })
        }
        0 => {
            if bytes.len() < REPLY_INCOMPATIBLE_LEN {
                return Err(PreambleError::ShortRead);
            }
            let min = u16::from_le_bytes(bytes[5..7].try_into().unwrap());
            let max = u16::from_le_bytes(bytes[7..9].try_into().unwrap());
            Ok(DaemonReply::Incompatible { min, max })
        }
        _ => Err(PreambleError::Malformed),
    }
}

/// Negotiate a mutually supported version given both sides' `[min, max]`
/// ranges. `chosen = min(client_max, daemon_max)`; accepted iff
/// `max(client_min, daemon_min) <= chosen`. `build` is left empty here — the
/// caller (the daemon's connection handler, which owns the actual build
/// string) fills it in on `Accepted` before encoding the reply.
pub fn negotiate(
    client_min: u16,
    client_max: u16,
    daemon_min: u16,
    daemon_max: u16,
) -> DaemonReply {
    let chosen = client_max.min(daemon_max);
    let floor = client_min.max(daemon_min);
    if floor <= chosen {
        DaemonReply::Accepted {
            chosen,
            build: String::new(),
        }
    } else {
        DaemonReply::Incompatible {
            min: daemon_min,
            max: daemon_max,
        }
    }
}
