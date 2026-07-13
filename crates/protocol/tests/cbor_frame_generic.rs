//! RED (S3 phase 1, spec §4.1): generic `encode_cbor_frame`/`CborFrameDecoder<T>` round-trip over
//! a toy enum unrelated to this crate's `Frame` — proves the generic core works for ANY
//! `Serialize`/`DeserializeOwned` type, not just `Frame`. Mirrors `tests/framing.rs`'s coverage
//! (split-buffer push, oversized reject) at the generic layer; `tests/framing.rs` itself is left
//! completely untouched by this generalization (its `Frame`-specific tests keep proving
//! `encode_frame`/`FrameDecoder` are unaffected thin instantiations).

use bpa_protocol::{encode_cbor_frame, CborFrameDecoder, FrameError, MAX_FRAME_LEN};
use serde::{Deserialize, Serialize};

/// A toy enum with no relationship to `bpa_protocol::Frame` — proves the generic core is not
/// secretly still coupled to `Frame`.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
enum Toy {
    Unit,
    Num(u32),
    Named { label: String, values: Vec<u8> },
}

fn toy() -> Toy {
    Toy::Named {
        label: "hello".into(),
        values: vec![1, 2, 3, 4, 5],
    }
}

#[test]
fn single_value_encodes_and_decodes() {
    let bytes = encode_cbor_frame(&toy()).expect("encode");
    let declared = u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]) as usize;
    assert_eq!(
        declared,
        bytes.len() - 4,
        "length prefix must equal body length"
    );

    let mut dec: CborFrameDecoder<Toy> = CborFrameDecoder::new();
    dec.push(&bytes);
    let items = dec.decode().expect("decode");
    assert_eq!(items, vec![toy()]);
}

#[test]
fn split_buffer_push_across_reads_buffers_then_completes() {
    let bytes = encode_cbor_frame(&toy()).expect("encode");
    let split = bytes.len() / 2;
    let mut dec: CborFrameDecoder<Toy> = CborFrameDecoder::new();

    dec.push(&bytes[..split]);
    assert_eq!(
        dec.decode().expect("decode-1"),
        vec![],
        "half a value yields nothing"
    );

    dec.push(&bytes[split..]);
    assert_eq!(
        dec.decode().expect("decode-2"),
        vec![toy()],
        "second half completes it"
    );
}

#[test]
fn length_prefix_split_across_reads() {
    let bytes = encode_cbor_frame(&toy()).expect("encode");
    let mut dec: CborFrameDecoder<Toy> = CborFrameDecoder::new();
    // deliver only 2 of the 4 prefix bytes first
    dec.push(&bytes[..2]);
    assert_eq!(
        dec.decode().expect("d1"),
        vec![],
        "incomplete prefix yields nothing"
    );
    dec.push(&bytes[2..]);
    assert_eq!(dec.decode().expect("d2"), vec![toy()]);
}

#[test]
fn two_values_in_one_read_both_decode() {
    let mut buf = encode_cbor_frame(&Toy::Unit).expect("e1");
    buf.extend_from_slice(&encode_cbor_frame(&Toy::Num(42)).expect("e2"));
    let mut dec: CborFrameDecoder<Toy> = CborFrameDecoder::new();
    dec.push(&buf);
    assert_eq!(dec.decode().expect("decode"), vec![Toy::Unit, Toy::Num(42)]);
}

#[test]
fn oversized_length_prefix_is_rejected() {
    let mut dec: CborFrameDecoder<Toy> = CborFrameDecoder::new();
    let bogus = MAX_FRAME_LEN + 1;
    dec.push(&bogus.to_le_bytes());
    dec.push(&[0u8; 8]);
    match dec.decode() {
        Err(FrameError::Oversized(n)) => assert_eq!(n, bogus),
        other => panic!("expected Oversized, got {other:?}"),
    }
}

#[test]
fn garbage_body_of_valid_length_is_a_decode_error() {
    let mut dec: CborFrameDecoder<Toy> = CborFrameDecoder::new();
    let len: u32 = 6;
    dec.push(&len.to_le_bytes());
    dec.push(&[0xff, 0xff, 0xff, 0xff, 0xff, 0xff]);
    match dec.decode() {
        Err(FrameError::Decode(_)) => {}
        other => panic!("expected Decode error, got {other:?}"),
    }
}

#[test]
fn decoder_default_is_equivalent_to_new() {
    let mut dec: CborFrameDecoder<Toy> = Default::default();
    dec.push(&encode_cbor_frame(&Toy::Unit).unwrap());
    assert_eq!(dec.decode().unwrap(), vec![Toy::Unit]);
}
