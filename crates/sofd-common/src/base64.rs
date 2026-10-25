//! Base64, standard alphabet with padding.
//! The encoding is the one a PEM body, an SVG data URI and an OFD `CheckValue`
//! all use, so the four readers that need it share this rather than each
//! carrying its own table.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Encode bytes as standard base64, padded to a multiple of four characters.
pub fn encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let n = (u32::from(chunk[0]) << 16)
            | (u32::from(*chunk.get(1).unwrap_or(&0)) << 8)
            | u32::from(*chunk.get(2).unwrap_or(&0));
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// Decode standard base64, ignoring padding and any whitespace.
/// `None` means a character outside the alphabet: callers treat that as a
/// malformed value rather than as a short one, since silently dropping bytes
/// from a signature is worse than refusing it.
pub fn decode(text: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(text.len() / 4 * 3);
    let mut acc: u32 = 0;
    let mut bits: u32 = 0;
    for ch in text.bytes() {
        if ch == b'=' || ch.is_ascii_whitespace() {
            continue;
        }
        let v = ALPHABET.iter().position(|c| *c == ch)? as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_stays_empty() {
        assert_eq!(encode(&[]), "");
        assert_eq!(decode("").as_deref(), Some(&[][..]));
    }

    #[test]
    fn one_byte_pads_with_two_equals() {
        assert_eq!(encode(&[0x00]), "AA==");
        assert_eq!(encode(&[0xff]), "/w==");
    }

    #[test]
    fn two_bytes_pad_with_one_equals() {
        assert_eq!(encode(&[0x00, 0x00]), "AAA=");
        assert_eq!(encode(&[0xff, 0xff]), "//8=");
    }

    #[test]
    fn three_bytes_need_no_padding() {
        assert_eq!(encode(&[0x00, 0x00, 0x00]), "AAAA");
        assert_eq!(encode(&[0xff, 0xff, 0xff]), "////");
    }

    #[test]
    fn longer_inputs_split_on_three_byte_boundaries() {
        assert_eq!(encode(&[1, 2, 3, 4]), "AQIDBA==");
        assert_eq!(encode(b"sOFD"), "c09GRA==");
    }

    #[test]
    fn output_length_is_always_a_multiple_of_four() {
        for len in 0..64usize {
            let data: Vec<u8> = (0..len).map(|i| i as u8).collect();
            let encoded = encode(&data);
            assert_eq!(encoded.len() % 4, 0);
            assert_eq!(encoded.len(), len.div_ceil(3) * 4);
        }
    }

    #[test]
    fn a_round_trip_returns_the_original_bytes() {
        for len in 0..64usize {
            let data: Vec<u8> = (0..len).map(|i| (i * 7) as u8).collect();
            assert_eq!(decode(&encode(&data)).as_deref(), Some(data.as_slice()));
        }
    }

    #[test]
    fn wrapping_and_padding_are_both_accepted() {
        // A PEM body arrives in lines, and a hand-written producer may omit
        // the padding; neither is a reason to refuse the value.
        let wrapped = "c09G\r\nRA==\n";
        assert_eq!(decode(wrapped).as_deref(), Some(&b"sOFD"[..]));
        assert_eq!(decode("c09GRA").as_deref(), Some(&b"sOFD"[..]));
    }

    #[test]
    fn a_character_outside_the_alphabet_is_refused() {
        assert!(decode("c09G*A==").is_none(), "no silent truncation");
        assert!(decode("not base64!").is_none());
    }
}
