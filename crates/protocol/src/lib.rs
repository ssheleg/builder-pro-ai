//! bpa-protocol — SHARED Hop-B wire types (serde + ts-rs). Source of truth for TS types.
//! S0 skeleton: only the locked wire constants. Full types (spec §5–§7) land in Task 3.

/// Hop-B handshake magic — ASCII "BPA1". Locked (spec §7 / Global Constraints).
pub const MAGIC: u32 = 0x4250_4131;
/// Hop-B protocol version. Locked (spec §7 / Global Constraints).
pub const PROTO_VERSION: u16 = 1;

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
