//! What a verification result is worth telling a reader.
//!
//! `VerifyStatus` says whether the proof holds; a reader deciding to trust a
//! document also asks who signed it, when, and how much of the file it covers.
//! This gathers those so a command line and a window say the same thing. It
//! draws nothing: the seal is a picture the issuer placed.

use sofd_crypto::Certificate;

use crate::{SealText, Signature, read_coverage};

/// One line of a distinguished name, as a certificate spells it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamePart {
    /// The attribute type, `CN`, `O`, `C` and so on.
    pub kind: String,
    /// The value, verbatim.
    pub value: String,
}

/// A subject or issuer, split into its attributes. The split is on the
/// separator `sofd-crypto` joins with, and each part is read at its first `=`,
/// so a value holding a comma is not torn into extra attributes.
pub fn name_parts(name: &str) -> Vec<NamePart> {
    name.split(',')
        .filter_map(|part| {
            let (kind, value) = part.split_once('=')?;
            let kind = kind.trim();
            if kind.is_empty() {
                return None;
            }
            Some(NamePart {
                kind: kind.to_string(),
                value: value.trim().to_string(),
            })
        })
        .collect()
}

/// The value of one attribute of a name, when it states one.
pub fn name_value(name: &str, kind: &str) -> Option<String> {
    name_parts(name)
        .into_iter()
        .find(|part| part.kind.eq_ignore_ascii_case(kind))
        .map(|part| part.value)
}

/// What a signed value's own clock says the signing took place. GB/T 38540
/// writes `timeInfo` as `UTCTime` or `GeneralizedTime`, `YYMMDDHHMMSSZ` or
/// `YYYYMMDDHHMMSSZ`, sometimes with a fraction. Text that reads as neither is
/// passed through: a wrong time on a signature is worse than a raw one.
pub fn signing_time(der_or_text: &[u8]) -> Option<String> {
    // A DER time carries a tag and a length before its digits, and reading
    // from offset zero would put the length byte into the year. A leading tag
    // is stripped first; what is left is a bare string or a DER body.
    let body = match der_or_text.first() {
        Some(0x17 | 0x18) => {
            let rest = &der_or_text[1..];
            let (length, at) = der_length(rest)?;
            rest.get(at..at + length)?
        }
        _ => der_or_text,
    };
    let text = std::str::from_utf8(body).ok()?;
    let text = text.trim();
    // `GeneralizedTime` with a fraction or a zone offset still starts with the
    // fourteen digits this reader wants.
    let digits: String = text
        .chars()
        .take_while(|c| c.is_ascii_digit() || *c == '.')
        .collect();
    let whole: String = digits.split('.').next().unwrap_or_default().to_string();
    let (year, rest) = match whole.len() {
        14 => (whole[0..4].to_string(), whole[4..].to_string()),
        12 => {
            // A two-digit year is 19xx after 50 and 20xx before it, which is
            // the reading `UTCTime` itself prescribes.
            let two: u32 = whole[0..2].parse().ok()?;
            let century = if two >= 50 { "19" } else { "20" };
            (format!("{century}{}", &whole[0..2]), whole[2..].to_string())
        }
        _ => return Some(text.to_string()),
    };
    let (month, day) = (rest.get(0..2)?, rest.get(2..4)?);
    let (hour, minute) = (rest.get(4..6)?, rest.get(6..8)?);
    let second = rest.get(8..10).unwrap_or("00");
    Some(format!("{year}-{month}-{day} {hour}:{minute}:{second}Z"))
}

/// One ECDSA-style DER length byte: short form below 128, one long-form byte
/// above it, which is all a time string ever needs.
fn der_length(bytes: &[u8]) -> Option<(usize, usize)> {
    match *bytes.first()? {
        short @ 0x00..=0x7f => Some((usize::from(short), 1)),
        0x81 => Some((usize::from(*bytes.get(1)?), 2)),
        _ => None,
    }
}

/// Where one stamp puts the seal. The page is still the descriptor's id here;
/// joining it to a page index needs the document and is the caller's job.
#[derive(Debug, Clone, PartialEq)]
pub struct SealBox {
    /// The stamp's own identifier.
    pub id: String,
    /// The page id the stamp names, which is not an index.
    pub page_ref: String,
    /// Where on that page the seal sits, in millimetres.
    pub boundary: sofd_common::Rect,
}

impl std::fmt::Display for SealBox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{} {} {} {}",
            self.boundary.x, self.boundary.y, self.boundary.w, self.boundary.h
        )
    }
}

/// Everything worth showing about one verified signature.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SignatureDetails {
    /// The signature's identifier, when the document gives it one.
    pub id: Option<String>,
    /// The validator that wrote the signature, from `Provider`.
    pub provider: Option<String>,
    /// The signing certificate's subject, verbatim.
    pub subject: Option<String>,
    /// The common name alone, which for an invoice is the seal's own text.
    pub common_name: Option<String>,
    /// The certificate's issuer, verbatim.
    pub issuer: Option<String>,
    /// When the certificate is valid from, as `YYYY-MM-DD`.
    pub not_before: Option<String>,
    /// When it stops being valid, as `YYYY-MM-DD`.
    pub not_after: Option<String>,
    /// The digest algorithm the references name.
    pub check_method: Option<String>,
    /// The signature algorithm, from `SignatureMethod`.
    pub signature_method: Option<String>,
    /// When the issuer says it signed, from `SignatureDateTime`, as a time a
    /// reader can compare against the certificate's window. This is the
    /// issuer's own claim and not a timestamp: only a token proves when.
    pub signed_at: Option<String>,
    /// The signing certificate's serial number, in hex, which is how an
    /// operator looks a certificate up on a revocation list.
    pub serial: Option<String>,
    /// How many files the signature covers.
    pub covered: usize,
    /// Files in the container that no signature covers.
    pub uncovered: Vec<String>,
    /// Where the seals are drawn, in page millimetres.
    pub stamps: Vec<SealBox>,
    /// The three lines an invoice seal prints, when the certificate is one.
    pub seal: Option<SealText>,
}

/// Read the details of one signature out of the certificate it carries. The
/// certificate is the authority for identity and the descriptor for scope,
/// and both are in hand after a verification.
pub fn describe(
    signature: &Signature,
    certificate: Option<&Certificate>,
    coverage: Option<&CoverageSummary>,
) -> SignatureDetails {
    let mut details = SignatureDetails {
        id: signature.id.clone(),
        provider: signature.provider.clone(),
        signature_method: signature.signature_method.clone(),
        // `SignatureDateTime` is written as text, in the same two spellings a
        // signed value uses. A time this does not recognise is passed through
        // rather than turned into a plausible one.
        signed_at: signature
            .sign_time
            .as_deref()
            .and_then(|time| signing_time(time.as_bytes())),
        covered: signature.references.len(),
        ..Default::default()
    };
    details.check_method = signature
        .references
        .first()
        .map(|reference| reference.check_method.clone())
        .filter(|method| !method.is_empty());
    details.stamps = signature
        .stamps
        .iter()
        .map(|stamp| SealBox {
            id: stamp.id.clone(),
            page_ref: stamp.page_ref.clone(),
            boundary: stamp.boundary,
        })
        .collect();
    if let Some(coverage) = coverage {
        if let Some((_, count)) = coverage
            .covered
            .iter()
            .find(|(path, _)| Some(path.as_str()) == signature.descriptor_path.as_deref())
        {
            details.covered = *count;
        }
        details.uncovered.clone_from(&coverage.uncovered);
    }
    let Some(certificate) = certificate else {
        return details;
    };
    details.subject = Some(certificate.subject().to_string());
    details.issuer = Some(certificate.issuer().to_string());
    details.common_name = name_value(certificate.subject(), "CN");
    details.serial = Some(certificate.serial_hex().to_string()).filter(|serial| !serial.is_empty());
    details.not_before = Some(signature_date(certificate.not_before()));
    details.not_after = Some(signature_date(certificate.not_after()));
    details.seal = details
        .common_name
        .as_deref()
        .and_then(SealText::from_subject_cn);
    details
}

/// What files no signature in the list covers, and how many each does. A file
/// any signature protects has not been added behind the reader's back, so the
/// union is what matters; this reads the list once and answers both.
pub fn coverage_summary(
    package: &mut sofd_package::Package,
    list_loc: &str,
) -> Result<CoverageSummary, sofd_error::OfdError> {
    let reports = read_coverage(package, list_loc)?;
    // Per descriptor, not summed: a reader looking at one signature asks how
    // much that signature covers, and a total over three would answer about the
    // document instead.
    let covered = reports
        .iter()
        .map(|report| (report.base_loc.clone(), report.covered.len()))
        .collect();
    let uncovered = reports
        .first()
        .map(|report| report.uncovered.clone())
        .unwrap_or_default();
    Ok(CoverageSummary { covered, uncovered })
}

/// How much of the container the signatures protect.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CoverageSummary {
    /// Files covered, per descriptor path.
    pub covered: Vec<(String, usize)>,
    /// Files no signature names, which is what a later hand could have added.
    pub uncovered: Vec<String>,
}

/// A Unix timestamp as a bare date. The detail panel shows a day, not a day and
/// an hour: a certificate is issued for months at a time and the hour it began
/// is noise in a dialog deciding whether a file is genuine today.
fn signature_date(unix_seconds: i64) -> String {
    LocalDate::from_unix(unix_seconds).to_string()
}

/// A civil date, for the two certificate bounds.
struct LocalDate {
    year: i64,
    month: u32,
    day: u32,
}

impl LocalDate {
    /// Howard Hinnant's `civil_from_days`, in UTC.
    fn from_unix(secs: i64) -> Self {
        let days = secs.div_euclid(86_400);
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let y = yoe + era * 400;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
        let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
        let year = if month <= 2 { y + 1 } else { y };
        Self { year, month, day }
    }
}

impl std::fmt::Display for LocalDate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `sofd-crypto` joins a distinguished name with commas, so a subject reads
    /// as one line per attribute once split. An attribute without an `=` is not
    /// an attribute and is dropped rather than reported as one.
    #[test]
    fn a_distinguished_name_splits_into_its_attributes() {
        let parts = name_parts("C=CN,O=国家税务总局,CN=全国统一发票监制章国家税务总局重庆市税务局");
        assert_eq!(parts.len(), 3);
        assert_eq!(parts[0].kind, "C");
        assert_eq!(parts[0].value, "CN");
        assert_eq!(
            name_value("C=CN,CN=全国统一发票监制章国家税务总局重庆市税务局", "cn").as_deref(),
            Some("全国统一发票监制章国家税务总局重庆市税务局"),
            "the type is matched without regard to case"
        );
        assert!(name_parts("").is_empty());
        assert!(name_parts("garbage").is_empty());
    }

    /// GB/T 38540 writes the signing time as `GeneralizedTime` in most issuers
    /// and `UTCTime` in some, and the reader has to read both. A two-digit year
    /// follows the rule `UTCTime` itself states.
    #[test]
    fn a_signing_time_reads_in_either_der_spelling() {
        assert_eq!(
            signing_time(b"20200817111329Z").as_deref(),
            Some("2020-08-17 11:13:29Z")
        );
        assert_eq!(
            signing_time(b"200817111329Z").as_deref(),
            Some("2020-08-17 11:13:29Z")
        );
        assert_eq!(
            signing_time(b"990817111329Z").as_deref(),
            Some("1999-08-17 11:13:29Z"),
            "a two-digit year of 99 is 1999, not 2099"
        );
        assert_eq!(
            signing_time(b"20200817111329.5Z").as_deref(),
            Some("2020-08-17 11:13:29Z"),
            "a fraction of a second is dropped, not the time"
        );
        // Text that is not a time is passed through rather than turned into a
        // plausible one: a wrong timestamp on a signature is worse than a raw
        // one.
        assert_eq!(signing_time(b"whenever").as_deref(), Some("whenever"));
        // A UTCTime as DER writes it: tag, length, digits. Reading the tag off
        // rather than taking the digits from offset zero is what keeps this
        // from printing the length byte into the date.
        assert_eq!(
            signing_time(&[
                0x18, 0x0d, b'2', b'0', b'0', b'8', b'1', b'7', b'1', b'1', b'1', b'3', b'2', b'9',
                b'Z'
            ])
            .as_deref(),
            Some("2020-08-17 11:13:29Z")
        );
        // A bare string is not DER and is read as it stands, which is how a
        // descriptor's `SignatureDateTime` arrives.
        assert_eq!(
            signing_time(b"200817111329Z").as_deref(),
            Some("2020-08-17 11:13:29Z")
        );
    }

    /// The panel shows a day, and the two certificate bounds are what a reader
    /// compares against a calendar.
    #[test]
    fn a_certificate_bound_reads_as_a_date() {
        // 2020-08-17T00:00:00Z
        assert_eq!(
            LocalDate::from_unix(1_597_632_000).to_string(),
            "2020-08-17"
        );
        // The epoch itself, which is where an off-by-one in the algorithm shows.
        assert_eq!(LocalDate::from_unix(0).to_string(), "1970-01-01");
        // A leap day, the day a naive implementation gets wrong.
        assert_eq!(
            LocalDate::from_unix(1_582_934_400).to_string(),
            "2020-02-29"
        );
    }
}
