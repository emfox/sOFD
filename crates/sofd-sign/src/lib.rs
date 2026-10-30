//! Electronic signature reading and verification (GB/T 33190 clause 18).
//! Subset: signature list, descriptor with references and check value, and
//! SM3 + SM2 verification against an embedded certificate.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod details;
pub mod seal;

pub use seal::{SealPicture, sealed_picture};

use sofd_crypto::{Certificate, TimeStampToken, TrustAnchors, sm2_verify, sm3, validate_path};
use sofd_error::{OfdError, SignatureError};
use sofd_package::Package;
use sofd_xml::cursor::{Cursor, Event};

/// Outcome of verifying one signature. A proof that does not match, one this
/// build could not check and one from a signer outside the caller's anchors call
/// for different reactions, so all three are kept apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerifyStatus {
    /// Signature matches the referenced content.
    Valid,
    /// Content no longer matches the signed digest.
    Tampered,
    /// Nothing was proved either way: the descriptor names a digest this
    /// build does not implement, or a certificate outside its own window.
    Unchecked,
    /// The proof adds up, but its certificate reaches no anchor the caller named.
    Untrusted,
}

/// What the caller asked the engine to settle. 38540 7.3 e) 1) requires a path
/// check and no standard says where the root comes from, so the anchor is an
/// argument rather than a default the engine ships.
#[derive(Debug, Clone, Copy)]
pub enum TrustPolicy<'a> {
    /// The proof, the digest and the certificate's own validity window. The
    /// issuer is not examined, which is what [`verify_signature`] answers.
    Proof,
    /// The proof and a path from the signer certificate to one of `anchors`,
    /// walking through `intermediates` when the signature embeds only the leaf.
    Issuer {
        /// The certificates the caller accepts as the top of a chain.
        anchors: &'a TrustAnchors,
        /// Certificates between the signer and an anchor, in any order.
        intermediates: &'a [Certificate],
    },
}

/// One entry of the signature list file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SignRef {
    /// Identifier of the `Signature` element in the list.
    pub id: Option<String>,
    /// `Seal` for a stamp, `Sign` for a plain signature.
    pub kind: SignatureKind,
    /// Archive path of the signature descriptor file.
    pub base_loc: String,
}

/// GB/T 33190 clause 18.1: whether an entry is a seal or a plain signature.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SignatureKind {
    /// A stamp, drawn over the page (`Seal`, the schema default).
    #[default]
    Seal,
    /// A plain electronic signature with no appearance.
    Sign,
}

/// A reference to one protected file inside the package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reference {
    /// Digest algorithm identifier, expected to be `SM3`.
    pub check_method: String,
    /// Archive path of the referenced file, relative to the package root.
    pub file_ref: String,
    /// Digest of the referenced file, as written.
    pub check_value: Vec<u8>,
}

/// A stamp placement: where a seal is drawn and which page it belongs to.
#[derive(Debug, Clone, PartialEq)]
pub struct StampAnnot {
    /// Identifier of the stamp.
    pub id: String,
    /// Id of the page the stamp is placed on.
    pub page_ref: String,
    /// Where on the page the stamp sits.
    pub boundary: sofd_common::Rect,
    /// Optional region the stamp is clipped to.
    pub clip: Option<sofd_common::Rect>,
}

/// A parsed signature descriptor file.
#[derive(Debug, Clone, Default)]
pub struct Signature {
    /// Identifier of the signature element.
    pub id: Option<String>,
    /// DER-encoded check value (the SM2 signature over the file digests). The
    /// descriptor carries it inline or the signed value file does.
    pub check_value: Vec<u8>,
    /// Archive path of the descriptor this signature was read from, set by
    /// the reader so a `SignedValue` envelope can be matched to it.
    pub descriptor_path: Option<String>,
    /// The digest a `SignedValue` states for the descriptor it covers. When
    /// present the signature is over that digest rather than over the digests
    /// of the files the descriptor names, which is GB/T 38540's arrangement.
    pub sealed_digest: Option<Vec<u8>>,
    /// The bytes the signature in `SignedValue` is over. GB/T 38540 signs the
    /// content SEQUENCE inside the envelope rather than the digest, so a proof
    /// read from one brings its own subject and there is nothing to recompute.
    pub signed_content: Option<Vec<u8>>,
    /// Archive path of the signed value, from `SignedValue`. The proof itself
    /// lives in a separate file there rather than inline in the descriptor.
    pub signed_value_ref: Option<String>,
    /// Protected files in signing order.
    pub references: Vec<Reference>,
    /// DER-encoded signing certificate, when embedded.
    pub certificate: Option<Vec<u8>>,
    /// Name of the signing provider, from the `ProviderName` attribute.
    pub provider: Option<String>,
    /// Signature algorithm name, when present.
    pub signature_method: Option<String>,
    /// Signing time as written, when present.
    pub sign_time: Option<String>,
    /// DER-encoded GB/T 20520 timestamp token, when the signature carries one.
    pub timestamp: Option<Vec<u8>>,
    /// Stamp placements, from `StampAnnot`.
    pub stamps: Vec<StampAnnot>,
    /// Archive path of the seal image, from `Seal/BaseLoc`.
    pub seal: Option<String>,
}

/// Read the signature list file, returning one entry per signature. Clause 18.1
/// names the entry element `Signature`, on top of the older `Sign` spelling.
pub fn read_sign_list(package: &mut Package, loc: &str) -> Result<Vec<SignRef>, OfdError> {
    let xml = package.read_string(loc)?;
    // A `BaseLoc` inside the list is written relative to the list itself, the
    // same rule `Signature.seal` follows, so `Sign_0/Signature.xml` in a list
    // at `Doc_0/Signs/Signatures.xml` names `Doc_0/Signs/Sign_0/...`.
    let dir = sofd_package::parent_of(loc);
    let mut cursor = Cursor::new(&xml);
    let mut signs = Vec::new();
    loop {
        match cursor.next_event()? {
            Event::Start(node) if node.is("Sign") || node.is("Signature") => {
                let a = node.attributes();
                let base = a.get("BaseLoc").unwrap_or_default();
                if !base.is_empty() {
                    signs.push(SignRef {
                        id: a.get("ID").map(str::to_string),
                        kind: match a.get("Type") {
                            Some(t) if t.eq_ignore_ascii_case("Sign") => SignatureKind::Sign,
                            _ => SignatureKind::Seal,
                        },
                        base_loc: signature_path(package, &dir, base),
                    });
                }
            }
            Event::Start(node) if node.is("Signs") || node.is("Signatures") => {}
            Event::Start(other) => cursor.skip_element(&other)?,
            Event::End(_) => {}
            Event::Eof => break,
            Event::Text(_) => {}
        }
    }
    Ok(signs)
}

/// The entry a `BaseLoc` in the list names.
/// Clause 18.1 fixes no base for it and writers disagree, so the entry that
/// exists decides between the list's own directory and the document root. When
/// neither exists the reference is left as written: resolving it would invent a
/// path the file never named, and the reader reports the miss by that name.
fn signature_path(package: &Package, dir: &str, base: &str) -> String {
    let beside = sofd_package::resolve(dir, base);
    if package.contains(&beside) {
        return beside;
    }
    let from_document = sofd_package::resolve(&document_dir(dir), base);
    if package.contains(&from_document) {
        return from_document;
    }
    base.to_string()
}

/// The `Doc_N` directory a path sits under, or the root above it.
fn document_dir(path: &str) -> String {
    match path.split_once('/') {
        Some((head, _)) if head.starts_with(sofd_package::DOCS_PREFIX) => head.to_string(),
        _ => String::new(),
    }
}

/// Read one signature descriptor file.
pub fn read_signature(package: &mut Package, base_loc: &str) -> Result<Signature, OfdError> {
    let xml = package.read_string(base_loc)?;
    let mut signature = Signature {
        descriptor_path: Some(base_loc.to_string()),
        ..Signature::default()
    };
    // Clause 18.2.2: CheckMethod sits on References once and governs every
    // Reference below it. Clause 18.2.3: the proof itself is not inline; the
    // descriptor points at it with SignedValue, a path.
    let mut method = String::new();
    let mut pending: Option<usize> = None;
    let mut depth = 0usize;
    let mut in_ref_digest = false;
    let mut in_cert = false;
    let mut in_method = false;
    let mut in_time = false;
    let mut in_seal = false;
    let mut in_signed_value = false;
    let mut in_timestamp = false;
    let mut cursor = Cursor::new(&xml);
    loop {
        match cursor.next_event()? {
            Event::Start(node) => {
                depth += 1;
                let a = node.attributes();
                if node.is("Signature") {
                    signature.id = a.get("ID").map(str::to_string);
                } else if node.is("References") {
                    method = a.get("CheckMethod").unwrap_or_default().to_string();
                } else if node.is("Reference") {
                    signature.references.push(Reference {
                        check_method: method.clone(),
                        file_ref: a.get("FileRef").unwrap_or_default().to_string(),
                        check_value: Vec::new(),
                    });
                    pending = Some(signature.references.len() - 1);
                } else if node.is("CheckValue") && pending.is_some() {
                    in_ref_digest = true;
                } else if node.is("Cert") {
                    in_cert = true;
                } else if node.is("Provider") {
                    signature.provider = a.get("ProviderName").map(str::to_string);
                } else if node.is("SignatureMethod") {
                    in_method = true;
                } else if node.is("SignatureDateTime") {
                    in_time = true;
                } else if node.is("StampAnnot") {
                    signature.stamps.push(StampAnnot {
                        id: a.get("ID").unwrap_or_default().to_string(),
                        page_ref: a.get("PageRef").unwrap_or_default().to_string(),
                        boundary: read_box(a.get("Boundary")),
                        clip: a.get("Clip").map(|c| read_box(Some(c))),
                    });
                } else if node.is("SignedValue") {
                    in_signed_value = true;
                } else if node.is("Timestamp") || node.is("TimeStamp") {
                    in_timestamp = true;
                } else if node.is("Seal") {
                    in_seal = true;
                }
            }
            Event::End(_) => {
                depth = depth.saturating_sub(1);
                pending = None;
                in_ref_digest = false;
                in_cert = false;
                in_method = false;
                in_time = false;
                in_seal = false;
                in_signed_value = false;
                in_timestamp = false;
                if depth == 0 {
                    break;
                }
            }
            Event::Eof => break,
            Event::Text(text) if depth >= 1 => {
                let text = text.trim();
                if text.is_empty() {
                    continue;
                }
                absorb_text(
                    text,
                    &mut signature,
                    pending,
                    in_ref_digest,
                    in_cert,
                    in_method,
                    in_time,
                    in_seal,
                    in_signed_value,
                    in_timestamp,
                );
            }
            Event::Text(_) => {}
        }
    }
    Ok(signature)
}

impl Signature {
    /// Read the signed value the descriptor points at, if it has one. The proof
    /// lives in its own file rather than inline, so it takes a second read.
    pub fn read_check_value(&mut self, package: &mut Package) -> Result<(), OfdError> {
        let Some(loc) = self.signed_value_ref.clone() else {
            return Ok(());
        };
        let Some(bytes) = package.read_optional(&loc)? else {
            return Ok(());
        };
        // GB/T 38540 packs the certificate, the digest of the signed
        // descriptor and the signature into one envelope, so all three arrive
        // together. A file holding only the signature is taken as it stands.
        match sofd_crypto::split_signed_value(&bytes) {
            Some((signature, certificate)) => {
                self.check_value = signature;
                if self.certificate.is_none() {
                    self.certificate = certificate;
                }
                // The envelope names the descriptor it covers, so its digest
                // is the signed content only when that name is this file.
                let covers_self = self
                    .descriptor_path
                    .as_deref()
                    .is_none_or(|path| sofd_crypto::signed_value_covers(&bytes, path));
                if covers_self {
                    self.sealed_digest = sofd_crypto::signed_value_digest(&bytes);
                }
                // The signature the envelope carries is over its own content
                // SEQUENCE, so the bytes to check against come with it.
                self.signed_content = sofd_crypto::signed_value_content(&bytes).map(<[u8]>::to_vec);
            }
            None => self.check_value = b64_ish(&bytes),
        }
        Ok(())
    }

    /// The GB/T 20520 timestamp the signature carries, already parsed.
    pub fn timestamp_token(&self) -> Result<Option<TimeStampToken>, OfdError> {
        match &self.timestamp {
            Some(der) if !der.is_empty() => Ok(Some(TimeStampToken::parse_der(der)?)),
            _ => Ok(None),
        }
    }
}

/// What a signature's timestamp says, once checked against the signature it
/// countersigns. `Bound` means the imprint is this signature's own digest and
/// the TSA signed the token; `Unbound` means the token is about something else.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimestampStatus {
    /// No timestamp is attached, which is lawful.
    Absent,
    /// The timestamp binds this signature and its TSA proof verifies.
    Bound,
    /// The timestamp parses and verifies but timestamps other bytes.
    Unbound,
    /// The timestamp does not verify against its own certificate.
    Unverified,
}

/// Check a signature's timestamp: GB/T 38540-2020 clause 7.2 makes the imprint
/// the SM3 digest of the signature value, and GM/T 0099-2020 clause 7.2.5 sends
/// the token through GB/T 20520, so both the binding and the TSA proof matter.
pub fn verify_timestamp(sig: &Signature) -> Result<TimestampStatus, OfdError> {
    let Some(token) = sig.timestamp_token()? else {
        return Ok(TimestampStatus::Absent);
    };
    if !token.covers(&sig.check_value) {
        return Ok(TimestampStatus::Unbound);
    }
    let key = match &token.certificate {
        Some(der) => Certificate::parse_der(der)?.public_key().to_vec(),
        None => sig
            .certificate
            .as_ref()
            .map(|der| Certificate::parse_der(der))
            .transpose()?
            .map(|cert| cert.public_key().to_vec())
            .ok_or_else(|| {
                OfdError::Signature(SignatureError::CertificateChain(
                    "timestamp names no TSA certificate".into(),
                ))
            })?,
    };
    Ok(if token.verify_tsa_signature(&key)? {
        TimestampStatus::Bound
    } else {
        TimestampStatus::Unverified
    })
}

/// One seal as a page should draw it: where the stamp sits and the image bytes
/// the seal names, already read from the package.
#[derive(Debug, Clone, PartialEq)]
pub struct SealPlacement {
    /// Id of the stamp, from `StampAnnot`.
    pub id: String,
    /// Id of the page the stamp sits on, as the descriptor writes it.
    pub page_ref: String,
    /// Where on the page the seal is drawn, in page space.
    pub boundary: sofd_common::Rect,
    /// Region the seal is clipped to, when one is declared.
    pub clip: Option<sofd_common::Rect>,
    /// The seal image bytes, empty when the descriptor names none or the file
    /// is missing, so a caller can still place a placeholder.
    pub image: Vec<u8>,
    /// Archive path of the seal image, when one is named.
    pub image_ref: Option<String>,
    /// The seal a sealed signature carries, when it carries one. GB/T 38540
    /// puts it inside the signature: `SES_Signature.toSign.eseal` holds a
    /// `SES_Seal` whose picture is an image or, more often, a whole OFD
    /// document. An invoice has no `Seal` element to point at, so looking for
    /// one is how a reader ends up drawing nothing.
    pub sealed: Option<SealPicture>,
}

/// The three lines an invoice seal prints, read from the certificate. Notice
/// 2018 no. 41 fixes the ring: [`SEAL_TOP`], then [`SEAL_MIDDLE`], then the
/// bureau. An issuer writes all three into one `CN`, and the text travels
/// inside the certificate, so changing a character breaks the signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealText {
    /// Upper arc.
    pub top: String,
    /// Middle line.
    pub middle: String,
    /// Lower arc, the province bureau, as the certificate spells it.
    pub bottom: String,
}

/// Upper arc of the ring, as notice 2018 no. 41 prints it.
pub const SEAL_TOP: &str = "全国统一发票监制章";

/// Middle line of the seal.
pub const SEAL_MIDDLE: &str = "国家税务总局";

/// What separates the three lines when they are set on one row. A full-width
/// solidus: one line in a narrow panel, and not a word a translator would
/// change.
const SEAL_LINE_BREAK: char = '／';

impl SealText {
    /// The three lines on one row, for a panel or a command line too narrow to
    /// stack them.
    pub fn row(&self) -> String {
        format!(
            "{}{SEAL_LINE_BREAK}{}{SEAL_LINE_BREAK}{}",
            self.top, self.middle, self.bottom
        )
    }

    /// Split an invoice seal's `CN` into its three lines. `None` when the name
    /// is not one: a seal that some other issuer drew has its own text, and
    /// printing the fixed ring over it would be a claim that issuer never made.
    pub fn from_subject_cn(cn: &str) -> Option<Self> {
        let rest = cn.strip_prefix(SEAL_TOP)?;
        let bottom = rest.strip_prefix(SEAL_MIDDLE)?;
        if bottom.is_empty() {
            return None;
        }
        Some(Self {
            top: SEAL_TOP.to_string(),
            middle: SEAL_MIDDLE.to_string(),
            bottom: bottom.to_string(),
        })
    }
}

impl Signature {
    /// The seal this signature carries, when it carries one. The seal lives
    /// inside the signed value, a second file, so a caller holding only the
    /// descriptor has not seen it yet; reading the envelope is what makes an
    /// invoice show its seal at all, since they keep everything out of the
    /// descriptor.
    pub fn sealed_picture(&self, package: &mut Package) -> Option<SealPicture> {
        // Read the envelope whatever the descriptor already said: the seal is
        // inside it, not beside it, so the descriptor being complete is no
        // reason to stop short of the file that holds the picture.
        let mut signature = self.clone();
        signature.read_check_value(package).ok()?;
        let loc = signature.signed_value_ref.as_deref()?;
        let bytes = package.read_optional(loc).ok()??;
        seal::sealed_picture(&bytes)
    }

    /// The text the certificate names, when it names an invoice seal. This is
    /// not how a seal is drawn — the seal is a picture the reader pastes. It is
    /// what a dialog can say, and it cannot have been retyped.
    pub fn seal_name(&self) -> Option<SealText> {
        // The envelope is read by `verify_signature` before a verdict is
        // reached, so the certificate is in hand by then; a caller that asks
        // this first still gets an answer when the descriptor embeds one.
        let certificate = Certificate::parse_der(self.certificate.as_deref()?).ok()?;
        SealText::from_subject_cn(common_name(certificate.subject()))
    }
}

/// The common name of a distinguished name. Both spellings of the type are
/// accepted, because `x509-parser` renders `CN=…` in some builds and `cn=…` in
/// others, and a reader that knew one would find no seal in half of them.
fn common_name(subject: &str) -> &str {
    subject
        .split(',')
        .find_map(|part| {
            let (kind, value) = part.split_once('=')?;
            match kind.trim() {
                "CN" | "cn" => Some(value.trim()),
                _ => None,
            }
        })
        .unwrap_or_default()
}

/// Resolve a signature's stamps into drawable placements (clause 18.2.3). The
/// descriptor names the seal image beside itself and each `StampAnnot` says
/// where on which page it goes; a missing image keeps the empty placement.
pub fn read_seals(
    package: &mut Package,
    base_loc: &str,
    signature: &Signature,
) -> Result<Vec<SealPlacement>, OfdError> {
    let dir = sofd_package::parent_of(base_loc);
    let image_ref = signature
        .seal
        .as_ref()
        .map(|loc| sofd_package::resolve(&dir, loc));
    let image = match &image_ref {
        Some(path) => package.read_optional(path)?.unwrap_or_default(),
        None => Vec::new(),
    };
    // Read once rather than per stamp: every placement of this signature is
    // drawn from the same seal.
    let sealed = signature.sealed_picture(package);
    Ok(signature
        .stamps
        .iter()
        .map(|stamp| SealPlacement {
            id: stamp.id.clone(),
            page_ref: stamp.page_ref.clone(),
            boundary: stamp.boundary,
            clip: stamp.clip,
            image: image.clone(),
            image_ref: image_ref.clone(),
            sealed: sealed.clone(),
        })
        .collect())
}

/// What one signature covers, and what the container holds that it does not.
/// Clause 18.2.2 scopes a signature to the files its `References` name, so a
/// document can be extended without moving an earlier seal; this is that scope.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Coverage {
    /// Id of the signature, as its descriptor writes it.
    pub id: Option<String>,
    /// Archive path of the descriptor, so a caller can tell two apart.
    pub base_loc: String,
    /// The files this signature protects, in reference order.
    pub covered: Vec<String>,
    /// Entries the container holds that no reference names: content that
    /// arrived after this signature was applied, when it is the newest one.
    pub uncovered: Vec<String>,
}

/// A path as the container states it: no leading slash. Issuers write `FileRef`
/// with the slash and the container lists without, so one form for the
/// comparison keeps every referenced file from also reading as uncovered.
fn archive_path(path: &str) -> String {
    path.trim_start_matches('/').to_string()
}

/// Report what each signature in the list covers. The union of every
/// signature's covered set is subtracted from the container, so a file that
/// any signature protects is not reported as vulnerable to a later edit.
pub fn read_coverage(package: &mut Package, list_loc: &str) -> Result<Vec<Coverage>, OfdError> {
    let refs = read_sign_list(package, list_loc)?;
    let mut reports = Vec::new();
    let mut protected: std::collections::HashSet<String> = std::collections::HashSet::new();
    for entry in &refs {
        let signature = read_signature(package, &entry.base_loc)?;
        let covered: Vec<String> = signature
            .references
            .iter()
            .map(|reference| reference.file_ref.clone())
            .collect();
        // One spelling for the comparison: issuers write `FileRef` with the
        // leading slash, the container lists without one.
        protected.extend(covered.iter().map(|path| archive_path(path)));
        reports.push(Coverage {
            id: entry.id.clone(),
            base_loc: entry.base_loc.clone(),
            covered,
            uncovered: Vec::new(),
        });
    }
    // The descriptor files themselves are part of the container, and listing
    // one as uncovered would be noise: it is not content a reader draws.
    protected.extend(refs.iter().map(|entry| archive_path(&entry.base_loc)));
    protected.insert(archive_path(list_loc));
    let mut uncovered: Vec<String> = package
        .entries()
        .iter()
        .filter(|path| !protected.contains(path.as_str()))
        .cloned()
        .collect();
    uncovered.sort();
    for report in &mut reports {
        report.uncovered.clone_from(&uncovered);
    }
    Ok(reports)
}

/// A signed-value file holds the proof either as raw DER or as base64 text.
fn b64_ish(bytes: &[u8]) -> Vec<u8> {
    match std::str::from_utf8(bytes) {
        Ok(text) => sofd_common::base64::decode(text).unwrap_or_else(|| bytes.to_vec()),
        Err(_) => bytes.to_vec(),
    }
}

/// `ST_Box` written as `left top width height` in an attribute.
fn read_box(raw: Option<&str>) -> sofd_common::Rect {
    let Some(raw) = raw else {
        return sofd_common::Rect::EMPTY;
    };
    let v: Vec<f64> = raw
        .split_whitespace()
        .filter_map(|t| t.parse::<f64>().ok())
        .collect();
    if v.len() < 4 {
        return sofd_common::Rect::EMPTY;
    }
    sofd_common::Rect::new(v[0], v[1], v[2], v[3])
}

#[allow(clippy::too_many_arguments)]
fn absorb_text(
    text: &str,
    signature: &mut Signature,
    pending: Option<usize>,
    ref_digest: bool,
    cert: bool,
    method: bool,
    time: bool,
    seal: bool,
    signed_value: bool,
    timestamp: bool,
) {
    if let Some(index) = pending {
        if ref_digest && let Some(bytes) = sofd_common::base64::decode(text) {
            signature.references[index].check_value = bytes;
        }
        return;
    }
    if seal {
        signature.seal = Some(text.to_string());
    } else if signed_value {
        signature.signed_value_ref = Some(text.to_string());
    } else if cert {
        if let Some(bytes) = sofd_common::base64::decode(text) {
            signature.certificate = Some(bytes);
        }
    } else if method {
        signature.signature_method = Some(text.to_string());
    } else if time {
        signature.sign_time = Some(text.to_string());
    } else if timestamp {
        // The timestamp travels as base64 DER, like `Cert` and `CheckValue`.
        if let Some(bytes) = sofd_common::base64::decode(text) {
            signature.timestamp = Some(bytes);
        } else {
            signature.timestamp = Some(text.as_bytes().to_vec());
        }
    }
}

/// Verify a signature against an externally supplied public key.
pub fn verify_with_public_key(
    package: &mut Package,
    sig: &Signature,
    public_key: &[u8],
) -> Result<VerifyStatus, OfdError> {
    // A proof read from a `SignedValue` brings the bytes it is over, and those
    // bytes are checked before the signature is, so an edited envelope fails
    // whether or not the seal on the descriptor still matches.
    if let Some(content) = &sig.signed_content {
        if public_key.is_empty() {
            return Ok(VerifyStatus::Unchecked);
        }
        let ok = sm2_verify(public_key, content, &sig.check_value)
            .map_err(|e| OfdError::Signature(SignatureError::UnsupportedFormat(e.to_string())))?;
        return Ok(if ok {
            VerifyStatus::Valid
        } else {
            VerifyStatus::Tampered
        });
    }
    let Some(digest) = signed_digest(package, sig)? else {
        return Ok(VerifyStatus::Unchecked);
    };
    // Without a key nothing can be checked, and reporting an untouched
    // document as tampered says more than the evidence supports.
    if public_key.is_empty() {
        return Ok(VerifyStatus::Unchecked);
    }
    let ok = sm2_verify(public_key, &digest, &sig.check_value)
        .map_err(|e| OfdError::Signature(SignatureError::UnsupportedFormat(e.to_string())))?;
    Ok(if ok {
        VerifyStatus::Valid
    } else {
        VerifyStatus::Tampered
    })
}

/// Whether the signature's own certificate covers the time the document says
/// it was signed. A certificate that had already expired is the one case a
/// reader can settle by itself: the key did not belong to the signer then.
fn certificate_covers_signing_time(sig: &Signature) -> Option<bool> {
    let der = sig.certificate.as_ref()?;
    let at = sofd_common::parse_utc(sig.sign_time.as_deref()?)?;
    let cert = Certificate::parse_der(der).ok()?;
    Some(!cert.is_expired_at(at))
}

/// Verify a signature against its embedded certificate, reading the proof from
/// the file `SignedValue` points at. This answers [`TrustPolicy::Proof`] only:
/// see [`verify_signature_under`].
pub fn verify_signature(package: &mut Package, sig: &Signature) -> Result<VerifyStatus, OfdError> {
    verify_signature_under(package, sig, &TrustPolicy::Proof)
}

/// Verify a signature under a stated trust policy. The proof and the window are
/// checked whatever the policy; a [`TrustPolicy::Issuer`] adds the path 38540
/// 7.3 e) 1) requires, and a broken proof is reported before either.
pub fn verify_signature_under(
    package: &mut Package,
    sig: &Signature,
    policy: &TrustPolicy<'_>,
) -> Result<VerifyStatus, OfdError> {
    // The certificate may sit inline or inside the signed value file, so the
    // proof is read before the key is looked for.
    let mut sig = sig.clone();
    sig.read_check_value(package)?;
    let Some(cert) = sig.certificate.clone() else {
        // Without a key nothing can be proved; the digests still show whether
        // the files were edited, which is what `Unchecked` reports.
        return verify_with_public_key(package, &sig, &[]);
    };
    let parsed = Certificate::parse_der(&cert).map_err(|e| {
        OfdError::Signature(SignatureError::UnsupportedFormat(format!(
            "embedded certificate unreadable: {e}"
        )))
    })?;
    if certificate_covers_signing_time(&sig) == Some(false) {
        // The proof may be perfectly good and still prove nothing: a key that
        // had expired may be used by anyone holding it.
        return Ok(VerifyStatus::Unchecked);
    }
    let proof = verify_with_public_key(package, &sig, parsed.public_key())?;
    if proof != VerifyStatus::Valid {
        return Ok(proof);
    }
    let TrustPolicy::Issuer {
        anchors,
        intermediates,
    } = policy
    else {
        return Ok(proof);
    };
    if !leaf_may_sign(&parsed) {
        // 38540 7.3 e) 1) makes the key usage part of the check, and a
        // certificate issued for encipherment is not one that may sign.
        return Ok(VerifyStatus::Untrusted);
    }
    match validate_path(&parsed, anchors, intermediates) {
        Ok(_) => Ok(VerifyStatus::Valid),
        Err(error) => {
            tracing::debug!(%error, "certification path does not reach an anchor");
            Ok(VerifyStatus::Untrusted)
        }
    }
}

/// Whether the signer certificate's own key usage permits a document signature.
/// 0015 5.2.4.4 marks `digitalSignature`/`nonRepudiation` as the signing pair, so
/// a certificate stating usage without either is one issued for something else.
fn leaf_may_sign(cert: &Certificate) -> bool {
    match cert.key_usage() {
        Some(usage) => usage.may_sign(),
        None => true,
    }
}

/// Same as [`verify_with_public_key`], but reads the proof from the file the
/// descriptor points at first, as a caller holding only the descriptor must.
pub fn verify_descriptor(
    package: &mut Package,
    sig: &Signature,
    public_key: &[u8],
) -> Result<VerifyStatus, OfdError> {
    let mut sig = sig.clone();
    sig.read_check_value(package)?;
    verify_with_public_key(package, &sig, public_key)
}

/// The digest that is signed: SM3 over the concatenated per-file digests, in
/// reference order, with each `CheckValue` compared as it goes. A reference
/// naming a digest this build lacks answers `None`, which is not an edit.
pub fn signed_digest(package: &mut Package, sig: &Signature) -> Result<Option<Vec<u8>>, OfdError> {
    if let Some(sealed) = &sig.sealed_digest {
        return Ok(Some(sealed.clone()));
    }
    let mut combined = Vec::new();
    for reference in &sig.references {
        if !is_sm3(&reference.check_method) {
            tracing::debug!(
                method = %reference.check_method,
                "a reference names a digest this build does not compute"
            );
            return Ok(None);
        }
        let data = package
            .read_optional(&reference.file_ref)?
            .ok_or(OfdError::Signature(SignatureError::Tampered))?;
        let digest = sm3(&data);
        if !reference.check_value.is_empty() && reference.check_value != digest.as_slice() {
            // The descriptor's own record of the file no longer matches it.
            return Err(OfdError::Signature(SignatureError::Tampered));
        }
        combined.extend_from_slice(&digest);
    }
    Ok(Some(sm3(&combined).to_vec()))
}

/// Whether `CheckMethod` names SM3, which descriptors spell either as the
/// algorithm name or as its object identifier.
fn is_sm3(method: &str) -> bool {
    method.eq_ignore_ascii_case("SM3") || method == sofd_crypto::SM3_OID
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The `CN` of the certificate in `tests/fixtures/ofdrw/999.ofd`, a real
    /// invoice from Chongqing: the ring, the bureau, then the province. The
    /// subject is written once and the bureau is the tail past the two fixed
    /// parts, so the test cannot pass merely by repeating what it received.
    #[test]
    fn an_invoice_seal_splits_into_its_three_lines() {
        const SUBJECT: &str = "全国统一发票监制章国家税务总局重庆市税务局";
        let text = SealText::from_subject_cn(SUBJECT).expect("an invoice seal names all three");
        assert_eq!(text.top, SEAL_TOP);
        assert_eq!(text.middle, SEAL_MIDDLE);
        let fixed = SEAL_TOP.chars().count() + SEAL_MIDDLE.chars().count();
        let bureau: String = SUBJECT.chars().skip(fixed).collect();
        assert_eq!(text.bottom, bureau);
        assert!(
            !bureau.is_empty(),
            "the province is what the subject ends with"
        );
        assert_eq!(
            text.row(),
            format!("{SEAL_TOP}{SEAL_LINE_BREAK}{SEAL_MIDDLE}{SEAL_LINE_BREAK}{bureau}")
        );
    }

    /// A plain signature and a company seal carry their own subject. Printing
    /// the fixed ring over one would claim what its issuer never claimed.
    #[test]
    fn a_seal_that_is_not_the_invoice_one_prints_nothing() {
        assert!(SealText::from_subject_cn("某某公司合同专用章").is_none());
        assert!(SealText::from_subject_cn("全国统一发票监制章").is_none());
        assert!(SealText::from_subject_cn("").is_none());
        // The ring and the bureau with no province after them is truncated.
        assert!(SealText::from_subject_cn("全国统一发票监制章国家税务总局").is_none());
    }

    /// A province spelled with a region and a city keeps both, because that is
    /// what the certificate says and what the seal on paper reads. A region
    /// that spans several words is the case a split at a fixed offset would cut.
    #[test]
    fn a_bureau_keeps_every_character_the_certificate_states() {
        const SUBJECT: &str = "全国统一发票监制章国家税务总局广西壮族自治区税务局";
        let text = SealText::from_subject_cn(SUBJECT).expect("a region is still a province");
        let bureau = SUBJECT
            .chars()
            .skip(SEAL_TOP.chars().count() + SEAL_MIDDLE.chars().count())
            .collect::<String>();
        assert_eq!(text.bottom, bureau);
        assert!(
            !bureau.is_empty(),
            "a province name follows the two fixed parts"
        );
    }

    /// `sofd-crypto` renders a subject through `x509-parser`, and the attribute
    /// type comes out in either case depending on the build. A reader that knew
    /// only one spelling would find no seal in half the invoices it opened.
    #[test]
    fn the_common_name_is_found_whichever_case_writes_it() {
        assert_eq!(
            common_name("O=重庆,CN=全国统一发票监制章国家税务总局重庆市税务局,OU=测试"),
            "全国统一发票监制章国家税务总局重庆市税务局"
        );
        assert_eq!(
            common_name("cn=全国统一发票监制章国家税务总局重庆市税务局"),
            "全国统一发票监制章国家税务总局重庆市税务局"
        );
        assert_eq!(common_name("O=重庆,OU=测试"), "");
    }
}
