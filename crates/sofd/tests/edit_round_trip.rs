//! Write, open, compare: an edit only means something once it survives the
//! container. There is no serializer yet, so these tests replace or drop an
//! entry and assert on the document reopened from the result.

use sofd::Document;
use sofd_package::build_archive;
use sofd_testkit::{CONTENT_ENTRY, CONTENT_XML, DOCUMENT_ENTRY, OFD_XML, archive_of, font_bytes};

/// The resource table that ties font id 7 to the embedded face.
const PUBLIC_RES: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Res BaseLoc="Res"><Fonts>
  <Font ID="7" FontName="Orbitron"><FontFile>Orbitron-Bold.ttf</FontFile></Font>
</Fonts></Res>"#;

/// The page content a fixture carries, or a replacement for it.
fn entries_with(content: &str) -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("OFD.xml", OFD_XML.as_bytes().to_vec()),
        (DOCUMENT_ENTRY, fixture_document().into_bytes()),
        ("Doc_0/PublicRes.xml", PUBLIC_RES.as_bytes().to_vec()),
        (CONTENT_ENTRY, content.as_bytes().to_vec()),
        ("Doc_0/Res/Orbitron-Bold.ttf", font_bytes()),
    ]
}

/// The document entry: one A4 page whose content lives in its own file.
fn fixture_document() -> String {
    r#"<?xml version="1.0" encoding="UTF-8"?>
<Document>
  <CommonData>
    <PageArea><PhysicalBox>0 0 210 297</PhysicalBox></PageArea>
  </CommonData>
  <Pages><Page ID="1" BaseLoc="Pages/Page_0/Content.xml"/></Pages>
</Document>"#
        .to_string()
}

/// An archive the reader accepts, used as the baseline every test edits.
fn baseline() -> Vec<u8> {
    archive_of(&entries_with(CONTENT_XML))
}

/// Reopen an archive and report the text on its first page.
fn text_of(bytes: &[u8]) -> String {
    let document = Document::open_bytes(bytes).expect("the archive must open");
    document
        .display_list()
        .pages
        .first()
        .map(|page| {
            page.ops
                .iter()
                .filter_map(|op| match op {
                    sofd_display::DrawOp::Text(run) => Some(run.text.clone().unwrap_or_default()),
                    _ => None,
                })
                .collect::<Vec<String>>()
                .join("")
        })
        .unwrap_or_default()
}

/// Reopen an archive and report the page box it lays out.
fn page_height(bytes: &[u8]) -> f64 {
    Document::open_bytes(bytes)
        .expect("open")
        .page(0)
        .expect("one page")
        .page_box
        .h
}

/// Reopen an archive and report how many paths its first page fills.
/// Text is left out: a backend splits a run into state changes around it, so
/// counting every operation would count the wrapper rather than the content.
fn path_count(bytes: &[u8]) -> usize {
    Document::open_bytes(bytes)
        .expect("open")
        .display_list()
        .pages
        .first()
        .map(|page| {
            page.ops
                .iter()
                .filter(|op| matches!(op, sofd_display::DrawOp::Path { .. }))
                .count()
        })
        .unwrap_or(0)
}

#[test]
fn what_the_baseline_holds_is_what_a_reader_reports() {
    // Every later test compares against this, so it states the fixture's own
    // content: one path, one run of text, an A4 page.
    let bytes = baseline();
    assert_eq!(page_height(&bytes), 297.0);
    assert_eq!(text_of(&bytes), "sOFD");
    assert_eq!(path_count(&bytes), 1);
}

#[test]
fn a_replaced_page_content_changes_what_the_reader_draws() {
    // The page file is the unit a writer replaces, so an edit that changes
    // what is drawn must show up as a different list after the round trip.
    let edited = CONTENT_XML.replace("sOFD", "sOFA");
    assert_ne!(edited, CONTENT_XML, "the replacement must change the file");
    let bytes = archive_of(&entries_with(&edited));
    assert_eq!(text_of(&bytes), "sOFA");
    // The path is untouched, so what is filled must not move either.
    assert_eq!(path_count(&bytes), 1);
}

#[test]
fn an_emptied_page_file_leaves_a_page_with_nothing_on_it() {
    // A page that the caller cleared is still a page: the archive must open and
    // report an empty list rather than fail or keep the old content.
    let empty = r#"<?xml version="1.0" encoding="UTF-8"?>
<Page>
  <Area><PhysicalBox>0 0 210 297</PhysicalBox></Area>
  <Content><Layer ID="2" Type="Body"/></Content>
</Page>"#;
    let bytes = archive_of(&entries_with(empty));
    assert_eq!(path_count(&bytes), 0);
    assert!(text_of(&bytes).is_empty());
}

#[test]
fn a_page_area_replacement_reaches_the_reopened_page() {
    // The page declares its own extent; a reader that laid out 297mm after the
    // edit never read the file the edit wrote.
    let taller = CONTENT_XML.replace("0 0 210 297", "0 0 210 420");
    assert_ne!(taller, CONTENT_XML, "the extent must actually change");
    let bytes = archive_of(&entries_with(&taller));
    assert_eq!(page_height(&bytes), 420.0);
}

#[test]
fn dropping_the_font_the_page_names_keeps_the_document_readable() {
    // A missing face is a resolver problem, not a broken container: the page
    // still lays out with a substitute rather than rejecting the archive.
    let mut entries = entries_with(CONTENT_XML);
    entries.retain(|(name, _)| *name != "Doc_0/Res/Orbitron-Bold.ttf");
    let bytes = archive_of(&entries);
    let document = Document::open_bytes(&bytes).expect("the container is intact");
    assert_eq!(document.page_count(), 1);
    // The text is still searchable, which is what a caller notices first.
    assert_eq!(text_of(&bytes), "sOFD");
}

#[test]
fn a_page_file_that_no_longer_parses_is_refused() {
    // The failure has to land on the edit, not on a later render: a caller that
    // writes a malformed page file must hear about it when the archive opens.
    let broken = CONTENT_XML.replace("</Layer>", "");
    let bytes = archive_of(&entries_with(&broken));
    assert!(
        Document::open_bytes(&bytes).is_err(),
        "an unclosed layer must not open as a document"
    );
}

#[test]
fn a_written_archive_is_byte_stable_across_two_builds() {
    // Two writes of one document must match byte for byte, or a caller cannot
    // compare what it saved by digest. Waiting past a clock tick is what
    // proves the timestamp is not the clock's.
    let entries = entries_with(CONTENT_XML);
    let build = || {
        build_archive(
            &entries
                .iter()
                .map(|(name, data)| (*name, data.as_slice()))
                .collect::<Vec<_>>(),
        )
        .expect("write")
    };
    let first = build();
    std::thread::sleep(std::time::Duration::from_millis(2100));
    let second = build();
    assert_eq!(first, second);
}

#[test]
fn an_archive_that_lost_its_document_entry_is_refused() {
    // Removing the file the container points at is the one edit that must
    // never pass: nothing downstream can lay out a document that is not there.
    let entries = entries_with(CONTENT_XML);
    let without: Vec<(&str, &[u8])> = entries
        .iter()
        .filter(|(name, _)| *name != DOCUMENT_ENTRY)
        .map(|(name, data)| (*name, data.as_slice()))
        .collect();
    let bytes = build_archive(&without).expect("the archive itself is well formed");
    assert!(Document::open_bytes(&bytes).is_err());
}

#[test]
fn the_facade_rewrites_the_container_as_an_archival_archive() {
    let bytes = baseline();
    let mut document = Document::open_bytes(&bytes).expect("the archive must open");
    let (rewritten, report) = document.archive().expect("the rewrite must succeed");
    // The fixture carries nothing the profile forbids, so nothing is stripped:
    // the rewrite declares the profile and the document survives the round trip.
    assert!(report.declared_profile);
    Document::open_bytes(&rewritten).expect("the rewritten archive must reopen");
}
