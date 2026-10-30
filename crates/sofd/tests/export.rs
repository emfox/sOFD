//! What exporting a whole document produces, driven through the facade.
//!
//! A document is what the facade hands out, so a test about "the document as
//! one PDF" belongs here. `sofd-package` builds the archives, not a fixture one:
//! several cases need pages of *different* sizes, which one box cannot express.

/// The entry point naming one document, the shape every OFD container needs.
const OFD_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<OFD Version="1.0">
  <DocBody><DocRoot>Doc_0/Document.xml</DocRoot></DocBody>
</OFD>"#;

/// A document whose `CommonData` states one physical box and whose `Pages`
/// element lists the pages given.
fn document_xml(width: f64, height: f64, pages: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Document>
  <CommonData>
    <PageArea><PhysicalBox Left="0" Top="0" Width="{width}" Height="{height}"/></PageArea>
  </CommonData>
  <Pages>{pages}</Pages>
</Document>"#
    )
}

/// Open a document of `count` blank A4 pages.
fn document_of(count: usize) -> sofd::Document {
    let pages: String = (1..=count)
        .map(|i| format!(r#"<Page ID="{i}"/>"#))
        .collect();
    let body = document_xml(210.0, 297.0, &pages);
    let bytes = sofd_package::build_archive(&[
        ("OFD.xml", OFD_XML.as_bytes()),
        ("Doc_0/Document.xml", body.as_bytes()),
    ])
    .expect("build fixture");
    sofd::Document::open_bytes(&bytes).expect("open document")
}

/// The MediaBox of every page in the file, in the order written.
fn media_boxes(pdf: &[u8]) -> Vec<(f64, f64)> {
    let text = String::from_utf8_lossy(pdf);
    let mut boxes = Vec::new();
    for chunk in text.split("/MediaBox").skip(1) {
        let Some(open) = chunk.find('[') else {
            continue;
        };
        let Some(close) = chunk.find(']') else {
            continue;
        };
        let nums: Vec<f64> = chunk[open + 1..close]
            .split_whitespace()
            .filter_map(|n| n.parse().ok())
            .collect();
        if nums.len() == 4 {
            boxes.push((nums[2], nums[3]));
        }
    }
    boxes
}

#[test]
fn converting_a_document_yields_one_svg_document_per_page() {
    let doc = document_of(2);
    let pages = doc.to_svg_pages().expect("convert");
    assert_eq!(pages.len(), 2);
    for (index, svg) in pages.iter().enumerate() {
        assert!(
            svg.starts_with("<?xml version"),
            "page {index} is not a document"
        );
    }
}

#[test]
fn a_multi_page_pdf_counts_every_page() {
    let pdf = document_of(3).to_pdf().expect("convert");
    let text = String::from_utf8_lossy(&pdf);
    assert!(
        text.contains("/Count 3"),
        "page tree does not count three pages"
    );
}

/// The one part of a page that decides how big it is on screen, before and
/// after the export. A display list carries it per page, not per document.
fn page_sizes(doc: &sofd::Document) -> Vec<(f64, f64)> {
    doc.display_list()
        .pages
        .iter()
        .map(|p| (p.page_box.w, p.page_box.h))
        .collect()
}

#[test]
fn a_page_keeps_the_size_the_document_states_for_it() {
    // A receipt's first page is short and its continuations are A4, so one
    // page size for the whole file would either crop a continuation or leave
    // a band of white under the first page.
    let doc = document_of(2);
    let declared = page_sizes(&doc);
    assert_eq!(declared.len(), 2);
    let pdf = doc.to_pdf_pages(&[0, 1]).expect("convert");
    let boxes = media_boxes(&pdf);
    assert_eq!(boxes.len(), 2, "one MediaBox per page: {boxes:?}");
    let mm = |pt: f64| pt / (72.0 / 25.4);
    for (index, ((w, h), boxed)) in declared.iter().zip(&boxes).enumerate() {
        assert!((mm(boxed.0) - w).abs() < 0.01, "page {index} width");
        assert!((mm(boxed.1) - h).abs() < 0.01, "page {index} height");
    }
}

#[test]
fn the_counts_cover_every_page_the_file_holds() {
    // The counts are the only sign a conversion lost a glyph, so a multi-page
    // file reporting one page's worth would hide three quarters of the loss.
    let doc = document_of(3);
    let (_, stats) = doc.to_pdf_pages_with_stats(&[0, 1, 2]).expect("convert");
    assert_eq!(stats.images, 0, "these pages hold none");
    assert_eq!(
        stats.dropped_runs + stats.live_text_runs + stats.outlined_glyphs,
        0,
        "no page has text, so nothing could be converted"
    );
}

#[test]
fn a_selected_subset_of_pages_becomes_a_pdf_of_just_those_pages() {
    // A reader exports what it is looking at, which is a subset rather than a
    // span: facing mode shows page 2 beside page 3 and skips nothing between
    // them, but a search can leave gaps, so the selection is a list.
    let pdf = document_of(3).to_pdf_pages(&[0, 2]).expect("convert");
    let text = String::from_utf8_lossy(&pdf);
    assert!(
        text.contains("/Count 2"),
        "a two-page selection produced a tree that counts something else"
    );
}

#[test]
fn a_selection_may_name_the_same_page_twice() {
    // Repeating a page is a legitimate ask — a reader may want one page in the
    // export twice — and silently collapsing it would lose a page the caller
    // asked for.
    let pdf = document_of(2).to_pdf_pages(&[1, 1]).expect("convert");
    assert!(
        String::from_utf8_lossy(&pdf).contains("/Count 2"),
        "a repeated page was collapsed"
    );
}

#[test]
fn a_page_the_document_does_not_have_is_an_error_not_a_short_pdf() {
    // Failing loudly beats writing a PDF that quietly omits what the reader
    // asked for.
    let err = document_of(2)
        .to_pdf_pages(&[0, 8])
        .expect_err("a page outside the document must not convert");
    assert!(
        err.to_string().contains("out of range"),
        "unhelpful error: {err}"
    );
}

#[test]
fn an_empty_selection_still_produces_a_readable_pdf() {
    let pdf = document_of(1).to_pdf_pages(&[]).expect("convert");
    assert!(pdf.starts_with(b"%PDF-"));
    assert!(
        String::from_utf8_lossy(&pdf).contains("/Count 0"),
        "an empty selection should count zero pages, not fall back to all of them"
    );
}

#[test]
fn a_document_with_no_pages_still_produces_a_valid_pdf() {
    // Zero pages is a legitimate input, not a panic: the catalog and page tree
    // are still written, and the result is readable by a PDF consumer.
    let doc = document_of(0);
    assert_eq!(doc.page_count(), 0);
    let pdf = doc.to_pdf().expect("convert");
    assert!(pdf.starts_with(b"%PDF-"));
    let text = String::from_utf8_lossy(&pdf);
    assert!(
        text.contains("/Type /Catalog") || text.contains("/Type/Catalog"),
        "no catalog in {text}"
    );
}

/// The merged account of a conversion has to say how many pages it covered,
/// or a caller reading "no losses" cannot tell it was over an empty selection.
#[test]
fn a_pdf_outcome_counts_the_pages_it_converted() {
    let doc = document_of(3);
    let (bytes, outcome) = doc.pdf_outcome(&[0, 1, 2]).expect("convert");
    assert!(!bytes.is_empty(), "the file was written");
    assert_eq!(outcome.pages, 3);
    assert!(
        outcome.is_lossless(),
        "blank pages lose nothing: {outcome:?}"
    );
}

/// Two pages short one glyph each are two glyphs short, not one.
#[test]
fn an_outcome_merges_page_by_page() {
    let mut total = sofd::ConversionOutcome::default();
    for _ in 0..2 {
        let page = sofd::ConversionOutcome {
            pages: 1,
            missing_glyphs: 1,
            ..Default::default()
        };
        total.merge(&page);
    }
    assert_eq!(total.pages, 2);
    assert_eq!(total.missing_glyphs, 2);
    assert!(!total.is_lossless());
}

/// A selection names pages by index, and one out of range fails rather than
/// silently converting fewer.
#[test]
fn a_page_past_the_end_fails_the_conversion() {
    let doc = document_of(1);
    assert!(doc.pdf_outcome(&[0, 3]).is_err());
}
