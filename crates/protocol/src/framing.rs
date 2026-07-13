//! Hop-B framing: `u32` little-endian length prefix + CBOR body.
//! CBOR body via ciborium (self-describing, RFC 8949); `u32`-LE length prefix unchanged.
//!
//! Generalized (S3 phase 1, spec §4.1) into a `T: Serialize`/`T: DeserializeOwned` core —
//! [`encode_cbor_frame`]/[`CborFrameDecoder<T>`] — so a second daemon's own frame enum (e.g. a
//! later `bpa-orchd` phase) can reuse the exact same length-prefix + oversize-reject rules
//! without depending on this crate's `Frame` type. [`encode_frame`]/[`FrameDecoder`] — the Hop-B
//! `Frame` instantiation every existing caller uses — are now thin wrappers over the generic
//! core; their public API, behavior, and tests (`tests/framing.rs`) are unchanged.

use std::fmt;
use std::marker::PhantomData;

use serde::de::DeserializeOwned;
use serde::Serialize;

use crate::Frame;

/// Hard cap on a single frame body (16 MiB). A larger declared length is treated
/// as garbage/DoS and rejected rather than allocated.
pub const MAX_FRAME_LEN: u32 = 16 * 1024 * 1024;

#[derive(Debug)]
pub enum FrameError {
    /// declared length prefix exceeds `MAX_FRAME_LEN`
    Oversized(u32),
    /// CBOR failed to decode a complete, correctly-sized body
    Decode(String),
    /// CBOR failed to encode
    Encode(String),
}

impl fmt::Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FrameError::Oversized(n) => write!(f, "frame length {n} exceeds max {MAX_FRAME_LEN}"),
            FrameError::Decode(e) => write!(f, "frame decode error: {e}"),
            FrameError::Encode(e) => write!(f, "frame encode error: {e}"),
        }
    }
}

impl std::error::Error for FrameError {}

/// Serialize `v` as CBOR and prepend a `u32`-LE length prefix. Generic core behind
/// [`encode_frame`] (spec §4.1): any `T: Serialize` can be framed the same way, not just this
/// crate's `Frame`.
pub fn encode_cbor_frame<T: Serialize>(v: &T) -> Result<Vec<u8>, FrameError> {
    let mut body = Vec::new();
    ciborium::into_writer(v, &mut body).map_err(|e| FrameError::Encode(e.to_string()))?;
    if body.len() as u64 > MAX_FRAME_LEN as u64 {
        return Err(FrameError::Oversized(body.len() as u32));
    }
    let mut out = Vec::with_capacity(4 + body.len());
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    Ok(out)
}

/// Serialize `frame` as CBOR and prepend a `u32`-LE length prefix. Thin instantiation of
/// [`encode_cbor_frame`] over this crate's `Frame` (spec §4.1) — signature and behavior
/// unchanged from before the generalization.
pub fn encode_frame(frame: &Frame) -> Result<Vec<u8>, FrameError> {
    encode_cbor_frame(frame)
}

/// Buffers raw socket bytes and drains complete, length-prefixed CBOR values of type `T`. A
/// partial frame (prefix not yet complete, or body not fully arrived) stays buffered until the
/// next `push`. An oversized declared length is a hard error (the stream is corrupt). Generic
/// core behind [`FrameDecoder`] (spec §4.1).
pub struct CborFrameDecoder<T> {
    buf: Vec<u8>,
    _marker: PhantomData<T>,
}

impl<T: DeserializeOwned> CborFrameDecoder<T> {
    pub fn new() -> Self {
        CborFrameDecoder {
            buf: Vec::new(),
            _marker: PhantomData,
        }
    }

    /// Append newly-read bytes to the internal buffer.
    pub fn push(&mut self, chunk: &[u8]) {
        self.buf.extend_from_slice(chunk);
    }

    /// Drain and return every complete value currently buffered. Leaves any trailing partial
    /// frame in the buffer. Returns `Err` (without consuming the offending bytes) on an
    /// oversized length prefix or a body decode failure.
    pub fn decode(&mut self) -> Result<Vec<T>, FrameError> {
        let mut items = Vec::new();
        let mut offset = 0usize;
        loop {
            if self.buf.len() - offset < 4 {
                break; // not enough for a length prefix yet
            }
            let mut len_bytes = [0u8; 4];
            len_bytes.copy_from_slice(&self.buf[offset..offset + 4]);
            let len = u32::from_le_bytes(len_bytes);
            if len > MAX_FRAME_LEN {
                return Err(FrameError::Oversized(len));
            }
            let len = len as usize;
            if self.buf.len() - offset - 4 < len {
                break; // body not fully arrived; keep buffered
            }
            let body = &self.buf[offset + 4..offset + 4 + len];
            let item: T =
                ciborium::from_reader(body).map_err(|e| FrameError::Decode(e.to_string()))?;
            items.push(item);
            offset += 4 + len;
        }
        if offset > 0 {
            self.buf.drain(0..offset);
        }
        Ok(items)
    }
}

/// Hand-written (NOT `#[derive(Default)]`): the derive macro would add a spurious `T: Default`
/// bound even though `T` only ever appears inside `PhantomData<T>` — that would wrongly force
/// every instantiation's `T` (e.g. `Frame`, which has no `Default` impl) to implement `Default`
/// just to construct an empty decoder.
impl<T: DeserializeOwned> Default for CborFrameDecoder<T> {
    fn default() -> Self {
        Self::new()
    }
}

/// Thin instantiation of [`CborFrameDecoder`] over this crate's `Frame` (spec §4.1) — same
/// struct name, `Default` derive, and method signatures as before the generalization; every
/// existing caller and test (`tests/framing.rs`) is unaffected.
#[derive(Default)]
pub struct FrameDecoder {
    inner: CborFrameDecoder<Frame>,
}

impl FrameDecoder {
    pub fn new() -> Self {
        FrameDecoder {
            inner: CborFrameDecoder::new(),
        }
    }

    /// Append newly-read bytes to the internal buffer.
    pub fn push(&mut self, chunk: &[u8]) {
        self.inner.push(chunk);
    }

    /// Drain and return every complete frame currently buffered. Leaves any
    /// trailing partial frame in the buffer. Returns `Err` (without consuming the
    /// offending bytes) on an oversized length prefix or a body decode failure.
    pub fn decode(&mut self) -> Result<Vec<Frame>, FrameError> {
        self.inner.decode()
    }
}
