//! X.509 certificate parsing for signature verification support: what a
//! certification path needs, which GM/T 0015-2023 5.2.4 makes mandatory reading
//! for a verifying application.

use sofd_error::{CryptoError, OfdError};
use x509_parser::extensions::ParsedExtension;
use x509_parser::prelude::{FromDer, X509Certificate};

/// The signature algorithm GM/T 0015-2023 certificates carry: SM2 over SM3.
/// Objects that mean "SM2 with SM3" rather than "SM2" carry it, so a
/// certificate naming it is the one a path verifier expects to walk.
pub const SM2_WITH_SM3: &str = "1.2.156.10197.1.501";

/// A parsed X.509 certificate kept together with its raw DER encoding.
#[derive(Debug, Clone)]
pub struct Certificate {
    raw_der: Vec<u8>,
    subject: String,
    issuer: String,
    serial: String,
    not_before: i64,
    not_after: i64,
    public_key: Vec<u8>,
    signature_algorithm: String,
    subject_key_id: Option<Vec<u8>>,
    authority_key_id: Option<Vec<u8>>,
    is_ca: bool,
    path_len: Option<u32>,
    key_usage: Option<KeyUsage>,
    tbs_range: std::ops::Range<usize>,
    signature: Vec<u8>,
}

/// The bits GM/T 0015-2023 5.2.4.4 names, in the order the ASN.1 gives them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct KeyUsage {
    /// `digitalSignature` (0), a signature over anything but a certificate or CRL.
    pub digital_signature: bool,
    /// `nonRepudiation` (1), a signature that supports non-repudiation.
    pub non_repudiation: bool,
    /// `keyEncipherment` (2).
    pub key_encipherment: bool,
    /// `dataEncipherment` (3).
    pub data_encipherment: bool,
    /// `keyAgreement` (4).
    pub key_agreement: bool,
    /// `keyCertSign` (5), a signature over a certificate.
    pub key_cert_sign: bool,
    /// `cRLSign` (6), a signature over a revocation list.
    pub crl_sign: bool,
    /// `encipherOnly` (7), meaningful only beside `keyAgreement`.
    pub encipher_only: bool,
    /// `decipherOnly` (8), meaningful only beside `keyAgreement`.
    pub decipher_only: bool,
}

impl KeyUsage {
    /// Whether the key may sign a document: 0015 5.2.4.4 b) marks
    /// `nonRepudiation` for signatures that cannot be denied, and recommends the
    /// pair appear together rather than as one certificate for both uses.
    pub fn may_sign(&self) -> bool {
        self.digital_signature || self.non_repudiation
    }

    /// Whether the key may sign another certificate or a revocation list.
    pub fn may_certify(&self) -> bool {
        self.key_cert_sign
    }
}

impl Certificate {
    /// Parse a DER-encoded certificate, recording where its `tbsCertificate`
    /// sits: the signature covers those bytes as encoded, and a re-encoding
    /// round trip is only byte-identical for the DER this build produces.
    pub fn parse_der(der: &[u8]) -> Result<Self, OfdError> {
        let (rest, cert) = X509Certificate::from_der(der).map_err(|e| {
            OfdError::Crypto(CryptoError::Operation(format!(
                "certificate parse failed: {e}"
            )))
        })?;
        let _ = &rest;
        // The parsed nodes borrow `der`, so the TBS is located by counting
        // forward from the start rather than by asking a node for its address.
        let begin = 0;
        let tbs_range = tbs_range(der, begin).ok_or_else(|| {
            OfdError::Crypto(CryptoError::Operation(
                "certificate TBS offset is not within its encoding".into(),
            ))
        })?;
        Ok(Self {
            raw_der: der.to_vec(),
            subject: cert.subject().to_string(),
            issuer: cert.issuer().to_string(),
            serial: hex_of(cert.tbs_certificate.serial.to_bytes_be()),
            not_before: cert.validity().not_before.timestamp(),
            not_after: cert.validity().not_after.timestamp(),
            public_key: cert.public_key().subject_public_key.data.to_vec(),
            signature_algorithm: cert.signature_algorithm.algorithm.to_id_string(),
            tbs_range,
            signature: cert.signature_value.data.to_vec(),
            subject_key_id: extension(&cert, |e| match e {
                ParsedExtension::SubjectKeyIdentifier(id) => Some(id.0.to_vec()),
                _ => None,
            }),
            authority_key_id: extension(&cert, |e| match e {
                ParsedExtension::AuthorityKeyIdentifier(aki) => {
                    aki.key_identifier.as_ref().map(|k| k.0.to_vec())
                }
                _ => None,
            }),
            is_ca: extension(&cert, |e| match e {
                ParsedExtension::BasicConstraints(bc) => Some(bc.ca),
                _ => None,
            })
            .unwrap_or(false),
            path_len: extension(&cert, |e| match e {
                ParsedExtension::BasicConstraints(bc) => bc.path_len_constraint,
                _ => None,
            }),
            key_usage: extension(&cert, |e| match e {
                ParsedExtension::KeyUsage(ku) => Some(KeyUsage {
                    digital_signature: ku.digital_signature(),
                    non_repudiation: ku.non_repudiation(),
                    key_encipherment: ku.key_encipherment(),
                    data_encipherment: ku.data_encipherment(),
                    key_agreement: ku.key_agreement(),
                    key_cert_sign: ku.key_cert_sign(),
                    crl_sign: ku.crl_sign(),
                    encipher_only: ku.encipher_only(),
                    decipher_only: ku.decipher_only(),
                }),
                _ => None,
            }),
        })
    }

    /// Parse a PEM-encoded certificate (the first BEGIN CERTIFICATE block).
    pub fn parse_pem(pem: &str) -> Result<Self, OfdError> {
        let text = strip_pem_armor(pem)?;
        let der = sofd_common::base64::decode(text).ok_or_else(|| {
            OfdError::Crypto(CryptoError::Operation("invalid base64 in PEM".into()))
        })?;
        Self::parse_der(&der)
    }

    /// The raw DER bytes.
    pub fn raw_der(&self) -> &[u8] {
        &self.raw_der
    }

    /// Subject public key bytes, exactly as the certificate encodes them:
    /// for an SM2 certificate, the uncompressed EC point.
    pub fn public_key(&self) -> &[u8] {
        &self.public_key
    }

    /// Distinguished name of the subject.
    pub fn subject(&self) -> &str {
        &self.subject
    }

    /// Distinguished name of the issuer.
    pub fn issuer(&self) -> &str {
        &self.issuer
    }

    /// Serial number as lowercase hex.
    pub fn serial_hex(&self) -> &str {
        &self.serial
    }

    /// Not-before time as a Unix timestamp.
    pub fn not_before(&self) -> i64 {
        self.not_before
    }

    /// Not-after time as a Unix timestamp.
    pub fn not_after(&self) -> i64 {
        self.not_after
    }

    /// Whether the certificate is outside its window at the given timestamp.
    pub fn is_expired_at(&self, unix_seconds: i64) -> bool {
        unix_seconds < self.not_before || unix_seconds >= self.not_after
    }

    /// The algorithm this certificate's own signature uses, which is not the
    /// algorithm of the key it carries: a path is walked by verifying each link
    /// with the one that link names.
    pub fn signature_algorithm(&self) -> &str {
        &self.signature_algorithm
    }

    /// The subject key identifier extension of GM/T 0015-2023 5.2.4.3.
    pub fn subject_key_id(&self) -> Option<&[u8]> {
        self.subject_key_id.as_deref()
    }

    /// The authority key identifier extension of GM/T 0015-2023 5.2.4.2,
    /// which names the key that signed this certificate.
    pub fn authority_key_id(&self) -> Option<&[u8]> {
        self.authority_key_id.as_deref()
    }

    /// Whether `basicConstraints` marks this certificate as a CA. `cA` defaults
    /// to false and 0015 5.2.4.12 only says a CA *should* carry the extension,
    /// so an absent one makes an end entity.
    pub fn is_ca(&self) -> bool {
        self.is_ca
    }

    /// The `pathLenConstraint` of `basicConstraints`, when the certificate is
    /// a CA and states a limit.
    pub fn path_len(&self) -> Option<u32> {
        self.path_len
    }

    /// The `keyUsage` extension, or `None` when it is omitted: 38540 7.3 e)
    /// makes key usage part of the check, so an absent extension is a fact to
    /// report rather than a default to invent.
    pub fn key_usage(&self) -> Option<KeyUsage> {
        self.key_usage
    }

    /// Whether the certificate was issued by itself. A name match is not enough:
    /// 0015 5.2.3.6 lets the subject be empty, so two empty strings are not
    /// evidence that a certificate signed itself.
    pub fn is_self_signed(&self) -> bool {
        !self.subject.is_empty() && self.subject == self.issuer
    }

    /// Whether `issuer_key` signed this certificate. 38540 7.3 e) makes a
    /// link's own signature part of the check; one naming an algorithm this
    /// build lacks reports an error rather than `false`.
    pub fn verify_signature(&self, issuer_key: &[u8]) -> Result<bool, OfdError> {
        if self.signature_algorithm != SM2_WITH_SM3 {
            return Err(OfdError::Crypto(CryptoError::UnsupportedAlgorithm(
                format!("certificate signature uses {}", self.signature_algorithm),
            )));
        }
        let tbs = &self.raw_der[self.tbs_range.clone()];
        crate::asymmetric::sm2_verify(issuer_key, tbs, &self.signature)
    }
}

/// Read one extension by the value it parses to, or `None` when the
/// certificate omits it. A certificate is allowed to carry an extension this
/// build cannot parse; that is not an error, it just contributes nothing.
fn extension<T>(
    cert: &X509Certificate<'_>,
    read: impl Fn(&ParsedExtension<'_>) -> Option<T>,
) -> Option<T> {
    cert.extensions()
        .iter()
        .find_map(|ext| read(ext.parsed_extension()))
}

fn strip_pem_armor(pem: &str) -> Result<&str, OfdError> {
    let start = pem.find("-----BEGIN CERTIFICATE-----").ok_or_else(|| {
        OfdError::Crypto(CryptoError::Operation("no PEM certificate found".into()))
    })? + "-----BEGIN CERTIFICATE-----".len();
    let body = &pem[start..];
    let end = body.find("-----END CERTIFICATE-----").ok_or_else(|| {
        OfdError::Crypto(CryptoError::Operation(
            "unterminated PEM certificate".into(),
        ))
    })?;
    Ok(body[..end].trim())
}

fn hex_of(bytes: Vec<u8>) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The range of the `tbsCertificate` whose outer SEQUENCE starts at `start`.
/// The first child of that SEQUENCE follows its header directly, and only the
/// definite lengths DER allows are walked.
fn tbs_range(der: &[u8], start: usize) -> Option<std::ops::Range<usize>> {
    let outer = tlv(der, start)?;
    let tbs = tlv(der, outer.content_at)?;
    let end = tbs.content_at + tbs.content_len;
    (tbs.total <= outer.content_len).then_some(outer.content_at..end)
}

/// A TLV's content position and length, and its total encoded length.
struct Tlv {
    content_at: usize,
    content_len: usize,
    total: usize,
}

/// Read a TLV header at `at`, checking only that the encoding is definite.
fn tlv(der: &[u8], at: usize) -> Option<Tlv> {
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

/// Split a `SignedValue` into the certificate and the signature it carries.
///
/// GB/T 35275 makes the signed value a SEQUENCE of the value, the signer's
/// certificate, the algorithm and the signature itself. A producer may instead
/// write the bare signature, so a blob that is not such a SEQUENCE is returned
/// as the signature with no certificate rather than refused.
pub fn split_signed_value(der: &[u8]) -> Option<(Vec<u8>, Option<Vec<u8>>)> {
    let outer = tlv(der, 0)?;
    if outer.content_at + outer.content_len != der.len() || der[0] != 0x30 {
        return None;
    }
    let mut at = outer.content_at;
    let mut certificate = None;
    while at < outer.content_at + outer.content_len {
        let item = tlv(der, at)?;
        let body = der.get(item.content_at..item.content_at + item.content_len)?;
        // The certificate is an OCTET STRING wrapping its DER, which is how
        // 35275 keeps it apart from the value beside it.
        if der[at] == 0x04 && Certificate::parse_der(body).is_ok() {
            certificate = Some(body.to_vec());
        }
        at = item.content_at + item.content_len;
    }
    let signature = last_tlv_body(der, outer)?;
    Some((signature, certificate))
}

/// The bytes a `SignedValue`'s own signature is over.
///
/// GB/T 38540 wraps the signed content one level down: the outer SEQUENCE holds
/// the content SEQUENCE, then the certificate, the algorithm and the signature.
/// The signature covers that content SEQUENCE whole, tag and length included.
pub fn signed_value_content(der: &[u8]) -> Option<&[u8]> {
    let outer = tlv(der, 0)?;
    if outer.content_at + outer.content_len != der.len() || der[0] != 0x30 {
        return None;
    }
    let content = tlv(der, outer.content_at)?;
    if der[outer.content_at] != 0x30 {
        return None;
    }
    let end = content.content_at + content.content_len;
    der.get(outer.content_at..end)
}

/// The SM3 digest a `SignedValue` states for the descriptor it covers, when it
/// states one. GB/T 38540 makes the signed content a digest plus the name of
/// what it digests, so the envelope says what the signature is over rather
/// than repeating it.
pub fn signed_value_digest(der: &[u8]) -> Option<Vec<u8>> {
    let outer = tlv(der, 0)?;
    let content = tlv(der, outer.content_at)?;
    let end = content.content_at + content.content_len;
    let mut at = content.content_at;
    while at < end {
        let item = tlv(der, at)?;
        let body = der.get(item.content_at..item.content_at + item.content_len)?;
        // A BIT STRING names its unused bits in the first octet.
        let bits = match body.split_first() {
            Some((0, rest)) if rest.len() == 32 => rest,
            _ => body,
        };
        if der[at] == 0x03 && bits.len() == 32 {
            return Some(bits.to_vec());
        }
        at = item.content_at + item.content_len;
    }
    None
}

/// Whether the signature is over the descriptor itself, which a `SignedValue`
/// says by naming it.
pub fn signed_value_covers(der: &[u8], path: &str) -> bool {
    der.windows(path.len()).any(|w| w == path.as_bytes())
}

/// What a sealed signature says the seal looks like. GB/T 38540 6.1.2.5 makes
/// it a picture: a type, the bytes and the size. The type is usually `ofd`, a
/// whole document holding the seal as text and paths, and sometimes `png`.
/// Either way the bytes are the seal and nothing is left to lay out.
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

/// The seal a signed value carries, when it carries one. The path is fixed by
/// the standard rather than searched for: `SES_Signature.toSign.eseal` is a
/// `SES_Seal`, whose first element is its info and whose fourth is the picture.
/// Searching for a `PK` header instead finds one document's seal and the next
/// document's embedded object.
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

/// The `nth` element (from zero) of a constructed value's contents.
fn nth_tlv(der: &[u8], start: usize, len: usize, nth: usize) -> Option<Tlv> {
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

/// A non-negative INTEGER as DER writes it.
fn be_integer(body: &[u8]) -> u32 {
    body.iter().fold(0u32, |acc, &byte| {
        acc.saturating_mul(256).saturating_add(u32::from(byte))
    })
}

/// The body of the last child of a SEQUENCE, which 35275 makes the signature.
fn last_tlv_body(der: &[u8], outer: Tlv) -> Option<Vec<u8>> {
    let end = outer.content_at + outer.content_len;
    let mut at = outer.content_at;
    let mut last = None;
    while at < end {
        let item = tlv(der, at)?;
        last = Some(
            der.get(item.content_at..item.content_at + item.content_len)?
                .to_vec(),
        );
        at = item.content_at + item.content_len;
    }
    last.map(|body| match body.split_first() {
        // A BIT STRING names its unused bits in the first octet.
        Some((0, rest)) => rest.to_vec(),
        _ => body,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_signed_value_yields_the_signature_and_the_certificate() {
        // SEQUENCE { SEQUENCE{...}, OCTET STRING(cert), OID, BIT STRING(sig) }
        // is the shape the standard gives the signed value. The certificate is
        // recognised by parsing it, so the case carries a fixture one.
        let cert = include_bytes!("../tests/data/test-cert.der").to_vec();
        let mut body = vec![0x30, 0x03, 0x02, 0x01, 0x04, 0x04, 0x82];
        body.extend_from_slice(&(cert.len() as u16).to_be_bytes());
        body.extend_from_slice(&cert);
        body.extend_from_slice(&[0x06, 0x08, 0x2a, 0x81, 0x1c, 0xcf, 0x55, 0x01, 0x83, 0x75]);
        body.extend_from_slice(&[0x03, 0x04, 0x00, 0x30, 0x02, 0x05]);
        let mut der = vec![0x30, 0x82];
        der.extend_from_slice(&(body.len() as u16).to_be_bytes());
        der.extend_from_slice(&body);

        let (signature, carried) = split_signed_value(&der).expect("a signed value");
        assert_eq!(signature, vec![0x30, 0x02, 0x05], "the bit string's body");
        assert_eq!(
            carried.as_deref(),
            Some(cert.as_slice()),
            "the certificate travels with the signature"
        );
    }

    #[test]
    fn a_signed_value_states_the_digest_it_covers() {
        // SEQUENCE { SEQUENCE{ version, ..., TIME, BIT STRING(digest), IA5(path) } }
        let digest = [0xABu8; 32];
        let mut inner = vec![0x30, 0x25, 0x02, 0x01, 0x04];
        inner.extend_from_slice(&[0x03, 0x21, 0x00]);
        inner.extend_from_slice(&digest);
        let mut body = inner.clone();
        body.extend_from_slice(&[0x03, 0x42, 0x00]);
        body.resize(body.len() + 65, 0x11);
        let mut der = vec![0x30, 0x81];
        der.push(body.len() as u8);
        der.extend_from_slice(&body);

        assert_eq!(
            signed_value_digest(&der).as_deref(),
            Some(digest.as_slice())
        );
        assert!(
            !signed_value_covers(&der, "Signature.xml"),
            "a value naming no path covers no descriptor"
        );
    }

    #[test]
    fn a_value_that_is_not_an_envelope_carries_no_certificate() {
        // A file holding only the signature has no SEQUENCE around it.
        assert!(split_signed_value(&[0x05, 0x00]).is_none());
    }

    #[test]
    fn a_signed_value_names_the_bytes_its_signature_is_over() {
        // SEQUENCE { SEQUENCE{...}, OCTET STRING, OID, BIT STRING }. What the
        // signature covers is the content SEQUENCE whole, tag and length too,
        // which is everything from the outer body's start to its end.
        let mut inner = vec![0x30, 0x05, 0x02, 0x01, 0x04, 0x03, 0x00];
        let content = inner.clone();
        inner.extend_from_slice(&[0x04, 0x02, 0xAA, 0xBB]);
        inner.extend_from_slice(&[0x06, 0x01, 0x01]);
        inner.extend_from_slice(&[0x03, 0x02, 0x00, 0x11]);
        let mut der = vec![0x30, 0x81, inner.len() as u8];
        der.extend_from_slice(&inner);

        assert_eq!(
            signed_value_content(&der),
            Some(content.as_slice()),
            "the content sequence, not the whole envelope"
        );
        assert!(
            signed_value_content(&[0x05, 0x00]).is_none(),
            "a bare signature names no content"
        );
    }

    #[test]
    fn tbs_range_walks_an_outer_sequence() {
        // SEQUENCE(5) { SEQUENCE(3) { 02 01 00 } }: the inner TLV opens at 2.
        let der = [0x30, 0x05, 0x30, 0x03, 0x02, 0x01, 0x00];
        assert_eq!(tbs_range(&der, 0), Some(2..7));
    }

    #[test]
    fn tbs_range_follows_a_long_form_length() {
        // 30 81 83: a 0x83-byte body, whose own first child is 30 81 80.
        let mut der = vec![0x30, 0x81, 0x83, 0x30, 0x81, 0x80];
        der.extend(std::iter::repeat_n(0u8, 0x80));
        assert_eq!(tbs_range(&der, 0), Some(3..der.len()));
    }
}
