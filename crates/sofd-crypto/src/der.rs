//! Reading DER structure: the position and length of a TLV's contents.
//! The sign layer walks a sealed signature with these, so the encoder's view of
//! the encoding is the one a reader of it uses.

/// A TLV's content position and length, and its total encoded length.
pub struct Tlv {
    /// Offset of the contents inside the buffer the header was read from.
    pub content_at: usize,
    /// Length of the contents in bytes.
    pub content_len: usize,
    /// Bytes the header and its contents occupy together.
    pub total: usize,
}

/// Read a TLV header at `at`, checking only that the encoding is definite.
pub fn tlv(der: &[u8], at: usize) -> Option<Tlv> {
    if *der.get(at)? & 0x1F == 0x1F {
        return None;
    }
    let (content_at, content_len) = match *der.get(at + 1)? {
        l if l < 0x80 => (at + 2, l as usize),
        0x81 => (at + 3, usize::from(*der.get(at + 2)?)),
        0x82 => {
            let hi = usize::from(*der.get(at + 2)?);
            let lo = usize::from(*der.get(at + 3)?);
            (at + 4, hi << 8 | lo)
        }
        _ => return None,
    };
    Some(Tlv {
        content_at,
        content_len,
        total: content_at + content_len - at,
    })
}

/// The `nth` element (from zero) of a constructed value's contents.
pub fn nth_tlv(der: &[u8], start: usize, len: usize, nth: usize) -> Option<Tlv> {
    let end = start + len;
    let mut at = start;
    let mut seen = 0;
    while at < end {
        let item = tlv(der, at)?;
        if seen == nth {
            return Some(item);
        }
        seen += 1;
        at = item.content_at + item.content_len;
    }
    None
}

/// A DER INTEGER's body read as an unsigned number, saturating rather than
/// wrapping so a malformed length cannot become a plausible one.
pub fn be_integer(body: &[u8]) -> u32 {
    body.iter().fold(0u32, |acc, &byte| {
        acc.saturating_mul(256).saturating_add(u32::from(byte))
    })
}
