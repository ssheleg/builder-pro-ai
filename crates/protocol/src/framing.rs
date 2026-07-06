//! Hop-B framing: `u32` little-endian length prefix + CBOR(Frame) body.
//! CBOR body via ciborium (self-describing, RFC 8949); `u32`-LE length prefix unchanged.

use std::fmt;

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

/// Serialize `frame` as CBOR and prepend a `u32`-LE length prefix.
pub fn encode_frame(frame: &Frame) -> Result<Vec<u8>, FrameError> {
    let mut body = Vec::new();
    ciborium::into_writer(frame, &mut body).map_err(|e| FrameError::Encode(e.to_string()))?;
    if body.len() as u64 > MAX_FRAME_LEN as u64 {
        return Err(FrameError::Oversized(body.len() as u32));
    }
    let mut out = Vec::with_capacity(4 + body.len());
    out.extend_from_slice(&(body.len() as u32).to_le_bytes());
    out.extend_from_slice(&body);
    Ok(out)
}

/// Buffers raw socket bytes and drains complete frames. A partial frame (prefix
/// not yet complete, or body not fully arrived) stays buffered until the next
/// `push`. An oversized declared length is a hard error (the stream is corrupt).
#[derive(Default)]
pub struct FrameDecoder {
    buf: Vec<u8>,
}

impl FrameDecoder {
    pub fn new() -> Self {
        FrameDecoder { buf: Vec::new() }
    }

    /// Append newly-read bytes to the internal buffer.
    pub fn push(&mut self, chunk: &[u8]) {
        self.buf.extend_from_slice(chunk);
    }

    /// Drain and return every complete frame currently buffered. Leaves any
    /// trailing partial frame in the buffer. Returns `Err` (without consuming the
    /// offending bytes) on an oversized length prefix or a body decode failure.
    pub fn decode(&mut self) -> Result<Vec<Frame>, FrameError> {
        let mut frames = Vec::new();
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
            let frame: Frame =
                ciborium::from_reader(body).map_err(|e| FrameError::Decode(e.to_string()))?;
            frames.push(frame);
            offset += 4 + len;
        }
        if offset > 0 {
            self.buf.drain(0..offset);
        }
        Ok(frames)
    }
}
