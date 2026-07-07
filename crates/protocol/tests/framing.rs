use bpa_protocol::*;

fn frame() -> Frame {
    Frame::Request {
        id: 7,
        req: Request::WriteStdin {
            session_id: "s".into(),
            bytes: vec![1, 2, 3, 4, 5],
        },
    }
}

#[test]
fn single_frame_encodes_and_decodes() {
    let bytes = encode_frame(&frame()).expect("encode");
    // u32-LE length prefix + body
    let declared = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    assert_eq!(
        declared,
        bytes.len() - 4,
        "length prefix must equal body length"
    );

    let mut dec = FrameDecoder::new();
    dec.push(&bytes);
    let frames = dec.decode().expect("decode");
    assert_eq!(frames, vec![frame()]);
}

#[test]
fn partial_frame_across_reads_buffers_then_completes() {
    let bytes = encode_frame(&frame()).expect("encode");
    let split = bytes.len() / 2;
    let mut dec = FrameDecoder::new();

    dec.push(&bytes[..split]);
    assert_eq!(
        dec.decode().expect("decode-1"),
        vec![],
        "half a frame yields nothing"
    );

    dec.push(&bytes[split..]);
    assert_eq!(
        dec.decode().expect("decode-2"),
        vec![frame()],
        "second half completes it"
    );
}

#[test]
fn length_prefix_split_across_reads() {
    let bytes = encode_frame(&frame()).expect("encode");
    let mut dec = FrameDecoder::new();
    // deliver only 2 of the 4 prefix bytes first
    dec.push(&bytes[..2]);
    assert_eq!(
        dec.decode().expect("d1"),
        vec![],
        "incomplete prefix yields nothing"
    );
    dec.push(&bytes[2..]);
    assert_eq!(dec.decode().expect("d2"), vec![frame()]);
}

#[test]
fn two_frames_in_one_read_both_decode() {
    let mut buf = encode_frame(&frame()).expect("e1");
    buf.extend_from_slice(&encode_frame(&frame()).expect("e2"));
    let mut dec = FrameDecoder::new();
    dec.push(&buf);
    assert_eq!(dec.decode().expect("decode"), vec![frame(), frame()]);
}

#[test]
fn oversized_length_prefix_is_rejected() {
    let mut dec = FrameDecoder::new();
    // declare a body far larger than MAX_FRAME_LEN
    let bogus = MAX_FRAME_LEN + 1;
    dec.push(&bogus.to_le_bytes());
    dec.push(&[0u8; 8]); // some body bytes
    match dec.decode() {
        Err(FrameError::Oversized(n)) => assert_eq!(n, bogus),
        other => panic!("expected Oversized, got {other:?}"),
    }
}

#[test]
fn garbage_body_of_valid_length_is_a_decode_error() {
    let mut dec = FrameDecoder::new();
    let len: u32 = 6;
    dec.push(&len.to_le_bytes());
    dec.push(&[0xff, 0xff, 0xff, 0xff, 0xff, 0xff]); // undecodable as Frame
    match dec.decode() {
        Err(FrameError::Decode(_)) => {}
        other => panic!("expected Decode error, got {other:?}"),
    }
}

#[test]
fn encode_matches_manual_prefix() {
    let f = frame();
    let full = encode_frame(&f).expect("encode");
    let declared = u32::from_le_bytes([full[0], full[1], full[2], full[3]]) as usize;
    let body = &full[4..];
    assert_eq!(declared, body.len(), "length prefix must equal body length");

    // encode_frame must be deterministic: re-encoding the same Frame produces
    // byte-for-byte identical output (prefix + body), independent of the codec
    // used internally.
    assert_eq!(encode_frame(&f).expect("encode again"), full);
}
