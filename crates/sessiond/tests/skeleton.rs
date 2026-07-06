// Daemon skeleton test: the sessiond crate depends on bpa-protocol and sees the wire constants.
// Pv2 §4.2 replaced v1's single `MAGIC`/`PROTO_VERSION` scalars with the codec-agnostic preamble
// magic (`PREAMBLE_MAGIC`) and a supported-version *range* (`CLIENT_MIN..=CLIENT_MAX` /
// `DAEMON_MIN..=DAEMON_MAX`), all locked at 2. This is a "constants link and are v2" smoke check.
#[test]
fn daemon_links_protocol_constants() {
    assert_eq!(bpa_protocol::PREAMBLE_MAGIC, 0x4250_4141);
    assert_eq!(bpa_protocol::CLIENT_MIN_VERSION, 2);
    assert_eq!(bpa_protocol::CLIENT_MAX_VERSION, 2);
    assert_eq!(bpa_protocol::DAEMON_MIN_VERSION, 2);
    assert_eq!(bpa_protocol::DAEMON_MAX_VERSION, 2);
}
