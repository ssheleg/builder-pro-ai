// Daemon skeleton test: the sessiond crate depends on bpa-protocol and sees the wire constants.
#[test]
fn daemon_links_protocol_constants() {
    assert_eq!(bpa_protocol::MAGIC, 0x4250_4131);
    assert_eq!(bpa_protocol::PROTO_VERSION, 1);
}
