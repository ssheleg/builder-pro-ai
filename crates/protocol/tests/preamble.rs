use bpa_protocol::preamble::*;

#[test]
fn negotiate_equal_ranges_accepts_chosen_2() {
    match negotiate(2, 2, 2, 2) {
        DaemonReply::Accepted { chosen, .. } => assert_eq!(chosen, 2),
        _ => panic!(),
    }
}

#[test]
fn negotiate_disjoint_is_incompatible() {
    assert!(matches!(
        negotiate(3, 3, 2, 2),
        DaemonReply::Incompatible { min: 2, max: 2 }
    ));
}

/// S2 (`[0.3.0]`) bumped `CLIENT_MIN_VERSION`/`CLIENT_MAX_VERSION` v2 → v3 (multi-root
/// `Workspace.roots` + new verbs are not v2-decodable — see `preamble.rs`'s "Version history"
/// doc). This is the exact real-world upgrade trigger: an updated (v3) app talking to a
/// still-running OLD (v2) daemon must negotiate `Incompatible`, not silently "succeed" and then
/// fail to decode `Workspace`/new verbs later. `Incompatible` is what drives the existing
/// upgrade-consent dialog + `kickstart -k` (D4) — never a version this client build actually
/// speaks gets chosen against a stale daemon.
#[test]
fn new_v3_client_rejects_stale_v2_daemon_triggers_upgrade_dialog() {
    let old_v2_daemon = (2u16, 2u16);
    match negotiate(
        CLIENT_MIN_VERSION,
        CLIENT_MAX_VERSION,
        old_v2_daemon.0,
        old_v2_daemon.1,
    ) {
        DaemonReply::Incompatible { min, max } => assert_eq!((min, max), old_v2_daemon),
        other => panic!("expected Incompatible{{2,2}} against a stale v2 daemon, got {other:?}"),
    }
}

#[test]
fn negotiate_overlap_picks_min_of_maxes() {
    match negotiate(1, 3, 2, 4) {
        DaemonReply::Accepted { chosen, .. } => assert_eq!(chosen, 3),
        _ => panic!(),
    }
}

#[test]
fn client_preamble_round_trips_through_bytes() {
    let p = ClientPreamble {
        min: 2,
        max: 2,
        build: "gui".into(),
    };
    let bytes = encode_client_preamble(&p);
    let got = decode_client_preamble(&bytes).unwrap();
    assert_eq!((got.min, got.max, got.build), (2, 2, "gui".to_string()));
}

#[test]
fn bad_magic_is_rejected() {
    let mut bytes = encode_client_preamble(&ClientPreamble {
        min: 2,
        max: 2,
        build: "x".into(),
    });
    bytes[0] ^= 0xFF;
    assert!(matches!(
        decode_client_preamble(&bytes),
        Err(PreambleError::BadMagic)
    ));
}

#[test]
fn oversized_build_len_is_rejected() {
    // hand-build a preamble claiming build_len=300 (> MAX_PREAMBLE_BUILD_LEN)
    let mut b = PREAMBLE_MAGIC.to_le_bytes().to_vec();
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&300u16.to_le_bytes());
    assert!(matches!(
        decode_client_preamble(&b),
        Err(PreambleError::BuildTooLong)
    ));
}
