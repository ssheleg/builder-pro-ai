//! Codec-agnostic preamble handshake gate (S3 phase 1, spec §3), extracted verbatim from
//! `bpa-sessiond::socket_server`'s accept path so a second daemon (`bpa-orchd`) can reuse it
//! without depending on the sessiond crate. [`server_handshake`] owns the WHOLE handshake:
//! reading the client's fixed, codec-independent preamble (Pv2 §4.2/§4.4 — not a CBOR frame, so
//! a version-incompatible peer can always be told so even if it can't decode this daemon's CBOR
//! at all), negotiating a version via [`bpa_protocol::negotiate`], and writing the
//! `Accepted`/`Incompatible` reply — all bounded by [`bpa_protocol::PREAMBLE_TIMEOUT`] so a
//! stuck, silent, or garbage-writing peer can never hang the caller's connection task
//! indefinitely (fail closed).
//!
//! `min`/`max` and `build` are caller-supplied so this same function serves ANY daemon's own
//! version range and build string — sessiond passes `(DAEMON_MIN_VERSION, DAEMON_MAX_VERSION,
//! &deps.daemon_build)`; a future `bpa-orchd` passes its own independent `[1,1]` range (spec
//! §4.1 D8) and build string.

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

use bpa_protocol::{
    decode_client_preamble, encode_daemon_reply, negotiate, ClientPreamble, DaemonReply,
    MAX_PREAMBLE_BUILD_LEN, PREAMBLE_TIMEOUT,
};

/// Fixed length of the client preamble's header, before the trailing `build` string (Pv2 §4.2):
/// `magic:u32 | min:u16 | max:u16 | build_len:u16`.
const CLIENT_PREAMBLE_HEADER_LEN: usize = 4 + 2 + 2 + 2;

/// Read and decode one [`ClientPreamble`] off `stream` (Pv2 §4.2/§4.4): the fixed 10-byte header
/// first, then exactly `build_len` more bytes for the trailing `build` string — never more, so a
/// peer that declares an oversized `build_len` is rejected by [`decode_client_preamble`] (via
/// the header's own bound check) before any attempt to read/allocate that many bytes. Callers
/// are expected to wrap this in a [`PREAMBLE_TIMEOUT`]; this function itself has no timeout.
async fn read_client_preamble(stream: &mut UnixStream) -> std::io::Result<ClientPreamble> {
    let mut header = [0u8; CLIENT_PREAMBLE_HEADER_LEN];
    stream.read_exact(&mut header).await?;
    let build_len = u16::from_le_bytes(header[8..10].try_into().unwrap()) as usize;
    if build_len > MAX_PREAMBLE_BUILD_LEN {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "preamble build string exceeds MAX_PREAMBLE_BUILD_LEN",
        ));
    }
    let mut buf = Vec::with_capacity(CLIENT_PREAMBLE_HEADER_LEN + build_len);
    buf.extend_from_slice(&header);
    if build_len > 0 {
        let mut build = vec![0u8; build_len];
        stream.read_exact(&mut build).await?;
        buf.extend_from_slice(&build);
    }
    decode_client_preamble(&buf).map_err(to_io)
}

/// Convert any `Display` error into an `InvalidData` `io::Error`.
fn to_io<E: std::fmt::Display>(e: E) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, e.to_string())
}

/// Run the server side of the Pv2 preamble handshake on an already-accepted `stream` (spec §3):
/// read the client's `[min, max]` + build string within [`PREAMBLE_TIMEOUT`], negotiate a
/// version via [`bpa_protocol::negotiate`] against the caller's own `(min, max)` range, fill the
/// `Accepted` reply's `build` field from the caller-supplied `build` (`negotiate` itself never
/// knows the real build string — it's a pure version-arithmetic function), and write the reply
/// — also within [`PREAMBLE_TIMEOUT`].
///
/// - `Ok(Some(chosen))`: a mutually supported version was found, `Accepted { chosen, build }`
///   was written, and the caller should proceed to the CBOR frame dispatch loop using `chosen`.
/// - `Ok(None)`: no overlap; `Incompatible { min, max }` (the caller's own range) was already
///   written on the wire. The caller must close the connection without entering the dispatch
///   loop — this function does not close `stream` itself.
/// - `Err(_)`: the preamble never arrived, was garbage/malformed, or the reply write stalled —
///   all within `PREAMBLE_TIMEOUT`. Nothing further was written in this case (the read failed) or
///   was already best-effort attempted (the write failed); either way the caller should close the
///   connection quietly, exactly like `Ok(None)`.
pub async fn server_handshake(
    stream: &mut UnixStream,
    min: u16,
    max: u16,
    build: &str,
) -> std::io::Result<Option<u16>> {
    let client = tokio::time::timeout(PREAMBLE_TIMEOUT, read_client_preamble(stream))
        .await
        .map_err(|_| {
            std::io::Error::new(std::io::ErrorKind::TimedOut, "preamble read timed out")
        })??;

    let mut reply = negotiate(client.min, client.max, min, max);
    let chosen = if let DaemonReply::Accepted {
        chosen,
        build: reply_build,
    } = &mut reply
    {
        *reply_build = build.to_string();
        Some(*chosen)
    } else {
        None
    };

    let out = encode_daemon_reply(&reply);
    tokio::time::timeout(PREAMBLE_TIMEOUT, async {
        stream.write_all(&out).await?;
        stream.flush().await
    })
    .await
    .map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "preamble reply write timed out",
        )
    })??;

    Ok(chosen)
}

#[cfg(test)]
mod tests {
    use super::*;
    use bpa_protocol::{decode_daemon_reply, encode_client_preamble};

    /// Read and decode one [`DaemonReply`] off the wire, bounded so a regression that never
    /// replies fails the test fast instead of hanging the suite.
    async fn recv_daemon_reply(s: &mut UnixStream) -> DaemonReply {
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            // Accepted: magic(4)+result(1)+chosen(2)+build_len(2) = 9 bytes, then build_len more.
            // Incompatible: magic(4)+result(1)+min(2)+max(2) = 9 bytes, no trailing body.
            let mut header = [0u8; 9];
            s.read_exact(&mut header).await.unwrap();
            let result = header[4];
            let mut buf = header.to_vec();
            if result == 1 {
                let build_len = u16::from_le_bytes(header[7..9].try_into().unwrap()) as usize;
                let mut build = vec![0u8; build_len];
                s.read_exact(&mut build).await.unwrap();
                buf.extend_from_slice(&build);
            }
            decode_daemon_reply(&buf).expect("valid daemon reply")
        })
        .await
        .expect("timed out waiting for daemon reply")
    }

    async fn send_preamble(s: &mut UnixStream, min: u16, max: u16, build: &str) {
        let bytes = encode_client_preamble(&ClientPreamble {
            min,
            max,
            build: build.into(),
        });
        s.write_all(&bytes).await.unwrap();
        s.flush().await.unwrap();
    }

    #[tokio::test]
    async fn compatible_versions_accept_and_echo_the_passed_build() {
        let (server_stream, mut client_stream) = UnixStream::pair().unwrap();
        let daemon = tokio::spawn(async move {
            let mut s = server_stream;
            server_handshake(&mut s, 3, 3, "daemon-build-xyz").await
        });

        send_preamble(&mut client_stream, 3, 3, "core-build").await;
        let reply = recv_daemon_reply(&mut client_stream).await;
        let chosen = daemon.await.unwrap().expect("server_handshake ok");

        assert_eq!(chosen, Some(3));
        match reply {
            DaemonReply::Accepted { chosen: c, build } => {
                assert_eq!(c, 3);
                assert_eq!(build, "daemon-build-xyz");
            }
            other => panic!("expected Accepted, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn disjoint_ranges_are_incompatible_and_daemon_range_is_reported() {
        let (server_stream, mut client_stream) = UnixStream::pair().unwrap();
        let daemon = tokio::spawn(async move {
            let mut s = server_stream;
            server_handshake(&mut s, 3, 3, "daemon-build").await
        });

        // Client only speaks [9, 10]; daemon speaks [3, 3] — no overlap.
        send_preamble(&mut client_stream, 9, 10, "core-build").await;
        let reply = recv_daemon_reply(&mut client_stream).await;
        let chosen = daemon.await.unwrap().expect("server_handshake ok");

        assert_eq!(chosen, None);
        match reply {
            DaemonReply::Incompatible { min, max } => {
                assert_eq!((min, max), (3, 3));
            }
            other => panic!("expected Incompatible, got {other:?}"),
        }
    }

    #[tokio::test]
    async fn garbage_magic_returns_err() {
        let (mut server_stream, mut client_stream) = UnixStream::pair().unwrap();
        // A full 10-byte header, but not a valid magic ⇒ fast decode failure, not a timeout.
        client_stream
            .write_all(&[0xDE, 0xAD, 0xBE, 0xEF, 0, 0, 0, 0, 0, 0])
            .await
            .unwrap();
        client_stream.flush().await.unwrap();

        let result = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            server_handshake(&mut server_stream, 3, 3, "daemon-build"),
        )
        .await
        .expect("must not hang on plain garbage (no timeout needed to detect bad magic)");

        assert!(result.is_err(), "garbage magic must yield Err");
    }

    #[tokio::test]
    async fn stalled_client_times_out_with_err_within_preamble_timeout() {
        let (mut server_stream, client_stream) = UnixStream::pair().unwrap();
        // Keep the client end open but silent — a genuine stall, not an EOF.
        let start = std::time::Instant::now();

        let result = tokio::time::timeout(
            PREAMBLE_TIMEOUT + std::time::Duration::from_secs(2),
            server_handshake(&mut server_stream, 3, 3, "daemon-build"),
        )
        .await
        .expect("server_handshake must return within PREAMBLE_TIMEOUT + slack, not hang forever");

        assert!(result.is_err(), "a stalled client must yield Err");
        assert!(
            start.elapsed() >= PREAMBLE_TIMEOUT,
            "must wait out the full PREAMBLE_TIMEOUT, not return early"
        );
        drop(client_stream);
    }
}
