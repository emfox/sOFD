//! Host-side coverage for the browser bindings: the plain data shapes and
//! the methods that never build a JS value. The JS-returning methods only
//! run under wasm, where a browser supplies the runtime.

use sofd::{Document, SearchOptions};
use sofd_testkit::{SIGN_FILE, fixture_bytes};
use sofd_wasm::OfdDocument;
use sofd_wasm::shapes::{
    DocumentInfo, SearchHitData, SignatureData, search_hits, signature_statuses,
};

// The fixture follows the spec form of ST_Box (element text), so the runs
// exercise the same paths real documents take.

fn open_document(signed: bool) -> Document {
    Document::open_bytes(&fixture_bytes(signed)).expect("document")
}

/// Search options that match the way the bindings call into the engine.
fn search_options() -> SearchOptions {
    sofd_bindings::DEFAULT_SEARCH
}

#[test]
fn document_info_reports_the_declared_facts() {
    let info: DocumentInfo = DocumentInfo::of(&mut open_document(false));
    assert_eq!(info.version, "1.0");
    assert_eq!(info.pages, 1);
    assert_eq!(info.width_mm, 210.0);
    assert_eq!(info.height_mm, 297.0);
    assert_eq!(info.embedded_fonts, 1, "the container ships one face");
    assert_eq!(info.signatures, 0);
}

#[test]
fn search_hits_trim_to_page_and_snippet() {
    let doc = open_document(false);
    let hits: Vec<SearchHitData> = search_hits(&doc, "sofd", search_options());
    assert_eq!(hits.len(), 1, "the page carries the text once");
    assert_eq!(hits[0].page, 0);
    assert!(
        hits[0].snippet.contains("sOFD"),
        "the snippet keeps the matched text"
    );
    assert!(
        search_hits(&doc, "absent", search_options()).is_empty(),
        "a needle nothing matches yields no hits"
    );
}

#[test]
fn signature_statuses_require_a_signature_list() {
    let mut doc = open_document(false);
    assert!(
        signature_statuses(&mut doc).is_err(),
        "a document that declares no signatures is an error, matching the CLI"
    );
}

#[test]
fn signature_statuses_report_the_valid_signature() {
    let mut doc = open_document(true);
    let statuses: Vec<SignatureData> = signature_statuses(&mut doc).expect("statuses");
    assert_eq!(statuses.len(), 1);
    assert_eq!(statuses[0].location, SIGN_FILE);
    assert!(statuses[0].valid, "the embedded certificate verifies");
}

#[test]
fn open_reports_the_page_count() {
    let doc = OfdDocument::open(&fixture_bytes(false)).expect("open");
    assert_eq!(doc.page_count(), 1);
}

#[test]
fn render_png_returns_image_bytes() {
    let doc = OfdDocument::open(&fixture_bytes(false)).expect("open");
    let png = doc.render_png(0, 96.0).expect("png");
    assert!(png.starts_with(b"\x89PNG\r\n\x1a\n"), "PNG magic");
    assert!(png.len() > 1000, "a rendered A4 page is not a stray byte");
}

#[test]
fn to_svg_returns_a_document() {
    let doc = OfdDocument::open(&fixture_bytes(false)).expect("open");
    let svg = doc.to_svg(0).expect("svg");
    assert!(svg.contains("<svg"), "an SVG document string");
    assert!(svg.contains("210") && svg.contains("297"), "page size");
}
