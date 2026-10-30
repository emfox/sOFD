//! The shapes both bindings share, exercised on the host so a change to them
//! needs no browser and no C caller.

use sofd::Document;
use sofd_bindings::{
    DEFAULT_SEARCH, DocumentInfo, SearchHitData, SignatureData, search_hits, signature_statuses,
};
use sofd_testkit::{SIGN_FILE, fixture_bytes};

fn open_document(signed: bool) -> Document {
    Document::open_bytes(&fixture_bytes(signed)).expect("document")
}

#[test]
fn document_info_reports_the_declared_facts() {
    let info = DocumentInfo::of(&mut open_document(false));
    assert_eq!(info.version, "1.0");
    assert_eq!(info.pages, 1);
    assert_eq!(info.width_mm, 210.0);
    assert_eq!(info.height_mm, 297.0);
    assert_eq!(info.embedded_fonts, 1, "the container ships one face");
    assert_eq!(info.signatures, 0);
}

#[test]
fn the_page_box_comes_from_the_facade_not_the_model() {
    let doc = open_document(false);
    assert_eq!(doc.page_box().w, 210.0);
    assert_eq!(doc.page_box().h, 297.0);
}

#[test]
fn search_hits_trim_to_page_and_snippet() {
    let doc = open_document(false);
    let hits: Vec<SearchHitData> = search_hits(&doc, "sofd", DEFAULT_SEARCH);
    assert_eq!(hits.len(), 1, "the page carries the text once");
    assert_eq!(hits[0].page, 0);
    assert!(
        hits[0].snippet.contains("sOFD"),
        "the snippet keeps the match"
    );
    assert!(
        search_hits(&doc, "absent", DEFAULT_SEARCH).is_empty(),
        "a needle nothing matches yields no hits"
    );
}

#[test]
fn a_document_without_signatures_is_an_error() {
    let mut doc = open_document(false);
    assert!(signature_statuses(&mut doc).is_err());
}

#[test]
fn a_signed_document_reports_one_verified_signature() {
    let mut doc = open_document(true);
    let statuses: Vec<SignatureData> = signature_statuses(&mut doc).expect("signatures");
    assert_eq!(statuses.len(), 1);
    assert!(statuses[0].location.contains(SIGN_FILE), "got {statuses:?}");
    assert!(statuses[0].valid);
    assert!(!statuses[0].unchecked);
}
