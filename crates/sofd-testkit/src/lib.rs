//! Fixtures shared by the test suites, so a document shape is defined once.
//! Four of the suites used to carry private copies of the same zip builder, the
//! same base64 and DER helpers and the same signed document. A dev-dependency only.

#![warn(missing_docs)]

mod bench;
mod chain;
mod der;
mod document;
mod golden;
#[cfg(feature = "sign")]
mod signed;
mod timestamp;

pub use bench::{OBJECTS_PER_PAGE, bench_archive};
pub use chain::{BasicConstraints, CertSpec, KeyUsageBits, TestChain, cert_der, chain, key_id};
pub use der::{base64_of, oid, sm2_cert_der, sm2_cert_der_over, tlv};
pub use document::{
    CONTENT_ENTRY, CONTENT_XML, DOCUMENT_ENTRY, DOCUMENT_XML, OFD_XML, OFD_XML_SIGNED, SIGN_FILE,
    SIGN_VALUE_FILE, SIGNS_ENTRY, SIGNS_XML, annotated_archive, archive_of, base_entries,
    font_bytes, multipage_archive, multipage_document_xml, open_package, open_package_bytes,
};
pub use golden::{Content, DOCS, GoldenDoc, golden_archive, golden_book};
#[cfg(feature = "sign")]
pub use signed::{
    descriptor, descriptor_with_proof, fixture_bytes, signed_descriptor_entries, signed_fixture,
    signed_fixture_over, signed_fixture_with, unsigned_descriptor_entries,
};
pub use timestamp::{
    SM2_SM3_OID, SM3_OID, TST_INFO_OID, TimestampSpec, timestamp_token, tst_info_der,
};
