//! The seal a signed value carries, GB/T 38540 6.1.2.5.
//! A seal is business content, not cryptography: the encoding is walked with
//! `sofd_crypto::der`, and what comes out is the picture a viewer draws.

use sofd_crypto::der::{be_integer, nth_tlv, tlv};

/// What a sealed signature says the seal looks like: a type, the bytes and the
/// size. The type is usually `ofd`, a whole document holding the seal as text
/// and paths, and sometimes `png`. Either way the bytes are the seal.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealPicture {
    /// `ofd`, `png`, `gif`, `svg` and so on, as the seal states it.
    pub kind: String,
    /// The seal itself: an OFD or image file, byte for byte.
    pub data: Vec<u8>,
    /// Width to draw it at, in millimetres.
    pub width: u32,
    /// Height to draw it at, in millimetres.
    pub height: u32,
}

/// The seal a signed value carries, when it carries one.
///
/// The path is fixed by the standard rather than searched for:
/// `SES_Signature.toSign.eseal` is a `SES_Seal`, whose first element is its
/// info and whose fourth is the picture. Searching for a `PK` header instead
/// finds one document's seal and the next document's embedded object.
pub fn sealed_picture(der: &[u8]) -> Option<SealPicture> {
    // SES_Signature: SEQUENCE { toSign, cert, signAlgID, signedValue }
    let outer = tlv(der, 0)?;
    let to_sign = tlv(der, outer.content_at)?;
    // TBS_Sign: SEQUENCE { version, eseal, timeInfo, data, ... }
    let eseal = nth_tlv(der, to_sign.content_at, to_sign.content_len, 1)?;
    // SESeal: SEQUENCE { eSealInfo, cert, signAlgID, signedValue }
    let info = tlv(der, eseal.content_at)?;
    // SES_SealInfo: SEQUENCE { header, esID, property, picture, extDatas }
    let picture = nth_tlv(der, info.content_at, info.content_len, 3)?;
    let mut kind = None;
    let mut data = None;
    let mut size = Vec::new();
    let end = picture.content_at + picture.content_len;
    let mut at = picture.content_at;
    while at < end {
        let item = tlv(der, at)?;
        let body = der.get(item.content_at..item.content_at + item.content_len)?;
        match (der[at], kind.as_ref(), data.as_ref()) {
            // The type comes first, the bytes second; what follows is the width
            // and then the height, both plain integers in millimetres.
            (0x16, None, _) => kind = Some(String::from_utf8_lossy(body).into_owned()),
            (0x04, Some(_), None) => data = Some(body.to_vec()),
            (0x02, Some(_), Some(_)) => size.push(be_integer(body)),
            _ => {}
        }
        at = item.content_at + item.content_len;
    }
    let (kind, data) = (kind?, data?);
    let (width, height) = match size.as_slice() {
        [w, h, ..] => (*w, *h),
        // A seal that states no size is drawn at the size the annot gives it,
        // which the caller knows and this does not.
        _ => (0, 0),
    };
    if data.is_empty() {
        return None;
    }
    Some(SealPicture {
        kind,
        data,
        width,
        height,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Encode one TLV: a tag, the length in its shortest definite form, and the
    /// body, which is what the parser has to walk back through.
    fn tlv_bytes(tag: u8, body: &[u8]) -> Vec<u8> {
        let mut out = vec![tag];
        if body.len() < 0x80 {
            out.push(body.len() as u8);
        } else {
            out.push(0x82);
            out.push((body.len() >> 8) as u8);
            out.push(body.len() as u8);
        }
        out.extend_from_slice(body);
        out
    }

    fn seq(parts: &[Vec<u8>]) -> Vec<u8> {
        tlv_bytes(0x30, &parts.concat())
    }

    /// A `SES_Signature` whose seal states a `png`, its bytes and a size.
    fn signature_with_seal() -> Vec<u8> {
        let picture = seq(&[
            tlv_bytes(0x16, b"png"),
            tlv_bytes(0x04, b"\x89PNG\r\n\x1a\nseal"),
            tlv_bytes(0x02, &[20]),
            tlv_bytes(0x02, &[20]),
        ]);
        let info = seq(&[
            tlv_bytes(0x30, &[]),
            tlv_bytes(0x04, b"es-1"),
            tlv_bytes(0x04, b"prop"),
            picture,
        ]);
        let eseal = seq(&[
            info,
            tlv_bytes(0x30, &[]),
            tlv_bytes(0x06, &[0x2a]),
            tlv_bytes(0x03, &[0x00]),
        ]);
        let to_sign = seq(&[tlv_bytes(0x02, &[1]), eseal, tlv_bytes(0x04, b"data")]);
        seq(&[
            to_sign,
            tlv_bytes(0x30, &[]),
            tlv_bytes(0x06, &[0x2a]),
            tlv_bytes(0x04, b"sig"),
        ])
    }

    #[test]
    fn a_seal_picture_is_read_from_its_fixed_path() {
        let seal = sealed_picture(&signature_with_seal()).expect("the seal is there");
        assert_eq!(seal.kind, "png");
        assert_eq!(seal.data, b"\x89PNG\r\n\x1a\nseal");
        assert_eq!((seal.width, seal.height), (20, 20));
    }

    #[test]
    fn a_signature_without_a_seal_answers_nothing() {
        let bare = seq(&[seq(&[tlv_bytes(0x02, &[1])]), tlv_bytes(0x04, b"sig")]);
        assert_eq!(sealed_picture(&bare), None);
        assert_eq!(sealed_picture(&[]), None, "an empty blob has no path");
    }
}
