//! Just enough DER and base64 to build an SM2 certificate a parser accepts.
//! The engine reads a certificate out of a signature, so the tests need real
//! DER rather than a stub, and hand-rolling it avoids an encoder dependency.

/// Base64, the encoding `CheckValue` and `Cert` use on the wire. The engine's
/// own encoder is used so a fixture cannot agree with a reader that is wrong.
pub fn base64_of(bytes: &[u8]) -> String {
    sofd_common::base64::encode(bytes)
}

/// One DER tag-length-value triple.
pub fn tlv(tag: u8, content: &[u8]) -> Vec<u8> {
    let mut out = vec![tag];
    let len = content.len();
    if len < 0x80 {
        out.push(len as u8);
    } else {
        let be = len.to_be_bytes();
        let first = be.iter().position(|b| *b != 0).unwrap_or(be.len() - 1);
        out.push((be.len() - first) as u8 | 0x80);
        out.extend_from_slice(&be[first..]);
    }
    out.extend_from_slice(content);
    out
}

/// A DER object identifier from its arcs. The first two share one sub-identifier
/// and every arc after them is base-128, high bit set on all but the last group;
/// arc 501 is therefore the two groups `83 75`.
pub fn oid(arcs: &[u64]) -> Vec<u8> {
    let mut content = Vec::new();
    content.extend(base128(arcs[0] * 40 + arcs[1]));
    for &arc in &arcs[2..] {
        content.extend(base128(arc));
    }
    tlv(0x06, &content)
}

fn base128(mut value: u64) -> Vec<u8> {
    let mut groups = vec![(value & 0x7f) as u8];
    value >>= 7;
    while value > 0 {
        groups.push(((value & 0x7f) as u8) | 0x80);
        value >>= 7;
    }
    groups.reverse();
    groups
}

/// A minimal X.509 certificate carrying one SM2 public key: just enough
/// structure for a parser to read the SubjectPublicKeyInfo back out.
pub fn sm2_cert_der(public_key: &[u8]) -> Vec<u8> {
    sm2_cert_der_over(public_key, "260101000000Z", "991231235959Z")
}

/// The same certificate with the validity window a test needs: `not_before`
/// and `not_after` are UTCTime, so both must fall before 2050.
pub fn sm2_cert_der_over(public_key: &[u8], not_before: &str, not_after: &str) -> Vec<u8> {
    let ec_public_key = oid(&[1, 2, 840, 10045, 2, 1]);
    let sm2_curve = oid(&[1, 2, 156, 10197, 1, 301]);
    let algorithm = tlv(0x30, &[ec_public_key, sm2_curve].concat());
    let empty_name = tlv(0x30, &[]);
    let validity = tlv(
        0x30,
        &[
            tlv(0x17, not_before.as_bytes()),
            tlv(0x17, not_after.as_bytes()),
        ]
        .concat(),
    );
    let mut bit_string = vec![0x00];
    bit_string.extend_from_slice(public_key);
    let spki = tlv(0x30, &[algorithm.clone(), tlv(0x03, &bit_string)].concat());

    let mut tbs = tlv(0xA0, &tlv(0x02, &[0x02]));
    tbs.extend(tlv(0x02, &[0x01]));
    tbs.extend(&algorithm);
    tbs.extend(&empty_name);
    tbs.extend(&validity);
    tbs.extend(&empty_name);
    tbs.extend(&spki);

    let mut cert = tlv(0x30, &tbs);
    cert.extend(&algorithm);
    cert.extend(tlv(0x03, &[0x00]));
    tlv(0x30, &cert)
}
