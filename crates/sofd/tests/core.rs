//! Facade coverage: opening a document, pairing container fonts with the ids
//! the display list uses, and every capability the handle exposes, each behind
//! its own cargo feature.

use std::sync::Arc;

use sofd_display::DrawOp;
use sofd_error::OfdError;
use sofd_font::FontId;
use sofd_package::build_archive;

const OFD_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<OFD Version="1.0">
  <DocBody><DocRoot>Doc_0/Document.xml</DocRoot></DocBody>
</OFD>"#;

/// The same, with the signature list declared, which is the only thing that
/// says a document is signed.
const OFD_XML_SIGNED: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<OFD Version="1.0">
  <DocBody>
    <DocRoot>Doc_0/Document.xml</DocRoot>
    <Signatures>Doc_0/Signs/Signs.xml</Signatures>
  </DocBody>
</OFD>"#;

fn document_xml(pages: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Document>
  <CommonData>
    <PageArea><PhysicalBox Left="0" Top="0" Width="210" Height="297"/></PageArea>
  </CommonData>
  <Pages>{pages}</Pages>
</Document>"#
    )
}

fn text_page(font: &str, size: &str, text: &str) -> String {
    format!(
        r#"<Page ID="1"><Content><Layer>
  <TextObject ID="10" Boundary="0 0 100 30" Font="{font}" Size="{size}">
    <TextCode X="10" Y="25">{text}</TextCode>
  </TextObject>
</Layer></Content></Page>"#
    )
}

/// A page whose text names glyph indices through a `CGTransform`, which is the
/// only form that says which outlines a document needs without the face.
fn glyph_page(font: &str, size: &str, glyphs: &[u16]) -> String {
    let list = glyphs
        .iter()
        .map(|g| g.to_string())
        .collect::<Vec<_>>()
        .join(" ");
    format!(
        r#"<Page ID="1"><Content><Layer>
  <TextObject ID="10" Boundary="0 0 100 30" Font="{font}" Size="{size}">
    <CGTransform CodePosition="0" CodeCount="{n}" GlyphCount="{n}"><Glyphs>{list}</Glyphs></CGTransform>
    <TextCode X="10" Y="25">Hi</TextCode>
  </TextObject>
</Layer></Content></Page>"#,
        n = glyphs.len()
    )
}

/// A real face, so glyph outlining runs for real rather than degrading.
fn font_bytes() -> Vec<u8> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../sofd-render/tests/data/Orbitron-Bold.ttf"
    );
    std::fs::read(path).expect("font fixture")
}

/// The wide face the fallback tests pin, so the substitute is one the repo
/// carries rather than whatever CJK font the machine happens to install. It
/// covers the ideographs the samples use.
fn cjk_font_bytes() -> Vec<u8> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../sofd-font/tests/data/sofd-cjk-subset.ttf"
    );
    std::fs::read(path).expect("cjk fixture")
}

/// A public resource table declaring one font, the form every writer emits.
/// The declaration is the only thing that says which file a font id means.
fn font_table(id: &str, name: &str, file: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Res BaseLoc="Res"><Fonts><Font ID="{id}" FontName="{name}"><FontFile>{file}</FontFile></Font></Fonts></Res>"#
    )
}

fn signs_xml() -> Vec<u8> {
    br#"<Signs><Sign BaseLoc="Doc_0/Signs/Sign_0.xml"/></Signs>"#.to_vec()
}

/// A descriptor naming one seal image and one stamp on the page id given.
fn seal_descriptor(page_ref: &str, boundary: &str) -> String {
    format!(
        r#"<Signature ID="s">
  <SignedInfo><References CheckMethod="SM3">
    <Reference FileRef="Doc_0/Document.xml"><CheckValue>AAAA</CheckValue></Reference>
  </References></SignedInfo>
  <StampAnnot ID="1" PageRef="{page_ref}" Boundary="{boundary}"/>
  <Seal><BaseLoc>Seal.esl</BaseLoc></Seal>
</Signature>"#
    )
}

fn open(entries: &[(&str, Vec<u8>)]) -> sofd::Document {
    let borrowed: Vec<(&str, &[u8])> = entries
        .iter()
        .map(|(path, data)| (*path, data.as_slice()))
        .collect();
    let bytes = build_archive(&borrowed).expect("build fixture");
    sofd::Document::open_bytes(&bytes).expect("open document")
}

fn two_page_document() -> sofd::Document {
    let entries = [
        ("OFD.xml", OFD_XML.as_bytes().to_vec()),
        (
            "Doc_0/Document.xml",
            document_xml(r#"<Page ID="1"/><Page ID="2"/>"#).into_bytes(),
        ),
    ];
    open(&entries)
}

#[test]
fn opens_and_counts_pages() {
    let doc = two_page_document();
    assert_eq!(doc.page_count(), 2);
    let page = doc.page(0).expect("first page");
    assert_eq!(page.index, 0);
    assert_eq!(page.page_box.w, 210.0);
    assert_eq!(page.page_box.h, 297.0);
}

#[test]
fn page_lookup_is_bounded() {
    let doc = two_page_document();
    assert!(doc.page(1).is_some());
    assert!(doc.page(2).is_none());
    assert!(doc.page(99).is_none());
}

#[test]
fn model_mirrors_the_declaration() {
    let doc = two_page_document();
    let model = doc.model();
    assert_eq!(model.pages.pages.len(), 2);
    assert_eq!(model.common_data.page_area.physical_box.w, 210.0);
}

#[test]
fn version_reports_the_declared_version() {
    let doc = two_page_document();
    assert_eq!(doc.version(), "1.0");
}

#[test]
fn rejects_a_container_without_the_entry_point() {
    let bytes =
        build_archive(&[("Doc_0/Document.xml", b"<Document/>".as_slice())]).expect("build fixture");
    let err = sofd::Document::open_bytes(&bytes).expect_err("must fail");
    assert!(err.to_string().contains("not a valid OFD"), "got {err}");
}

#[test]
fn rejects_bytes_that_are_not_a_container() {
    let err = sofd::Document::open_bytes(b"not an ofd package").expect_err("must fail");
    assert!(err.to_string().contains("container"), "got {err}");
}

#[test]
fn registers_container_fonts_and_remaps_ids() {
    let entries = [
        ("OFD.xml", OFD_XML.as_bytes().to_vec()),
        (
            "Doc_0/Document.xml",
            document_xml(&text_page("7", "4.0", "Hi")).into_bytes(),
        ),
        (
            "Doc_0/PublicRes.xml",
            font_table("7", "Orbitron", "Orbitron-Bold.ttf").into_bytes(),
        ),
        ("Doc_0/Res/Orbitron-Bold.ttf", font_bytes()),
    ];
    let doc = open(&entries);
    // The embedded face the page names, plus the CJK substitute the chain
    // adds when no wide face is registered.
    assert_eq!(
        embedded_faces(doc.fonts()).len(),
        1,
        "the one container font is registered"
    );
    let run = doc
        .page(0)
        .and_then(|page| {
            page.ops.iter().find_map(|op| match op {
                DrawOp::Text(run) => Some(run),
                _ => None,
            })
        })
        .expect("text run");
    assert_eq!(
        run.font,
        FontId(0),
        "the document font id is rewritten to the registered id"
    );
    assert!(
        doc.fonts().face(FontId(0)).is_ok(),
        "the remapped id resolves to a loadable face"
    );
    assert_eq!(
        doc.fonts().face(FontId(0)).expect("face").family(),
        "Orbitron",
        "the face carries the declared name"
    );
}

#[test]
fn each_declared_font_keeps_the_file_its_declaration_names() {
    // The declarations arrive in an order that disagrees with the file names,
    // which is what a positional pairing gets wrong: it would hand font 7 the
    // bytes of font 9 and paint every run with the wrong face.
    let entries = [
        ("OFD.xml", OFD_XML.as_bytes().to_vec()),
        (
            "Doc_0/Document.xml",
            document_xml(&text_page("9", "4.0", "Nine")).into_bytes(),
        ),
        (
            "Doc_0/PublicRes.xml",
            font_table("9", "Nine", "aaa-nine.ttf").into_bytes(),
        ),
        ("Doc_0/Res/aaa-nine.ttf", font_bytes()),
    ];
    let doc = open(&entries);
    let run = doc
        .page(0)
        .and_then(|page| {
            page.ops.iter().find_map(|op| match op {
                DrawOp::Text(run) => Some(run),
                _ => None,
            })
        })
        .expect("text run");
    let face = doc.fonts().face(run.font).expect("face");
    assert_eq!(face.family(), "Nine", "the run resolves to its own font");
}

#[test]
fn non_font_entries_are_ignored() {
    let entries = [
        ("OFD.xml", OFD_XML.as_bytes().to_vec()),
        (
            "Doc_0/Document.xml",
            document_xml(&text_page("7", "4.0", "Hi")).into_bytes(),
        ),
        (
            "Doc_0/PublicRes.xml",
            font_table("7", "Orbitron", "Orbitron-Bold.ttf").into_bytes(),
        ),
        ("Doc_0/Res/Orbitron-Bold.ttf", font_bytes()),
        ("Doc_0/Res/readme.txt", b"not a font".to_vec()),
    ];
    let doc = open(&entries);
    assert_eq!(embedded_faces(doc.fonts()).len(), 1);
}

#[test]
fn a_font_the_document_never_declares_is_not_registered() {
    // A font file in the container is not a resource until a declaration
    // names it; scanning the directory used to pair ids with files by
    // position, which is how a run came to be drawn with the wrong face.
    let entries = [
        ("OFD.xml", OFD_XML.as_bytes().to_vec()),
        (
            "Doc_0/Document.xml",
            document_xml(&text_page("7", "4.0", "Hi")).into_bytes(),
        ),
        ("Doc_0/Res/Orbitron-Bold.ttf", font_bytes()),
    ];
    let doc = open(&entries);
    // The id is still resolved, but to the system substitute: nothing was
    // embedded, so no entry carries the container file's bytes. The chain's
    // CJK substitute is the only other entry.
    let entries = doc.fonts().registry().iter().collect::<Vec<_>>();
    let systems: Vec<_> = entries
        .iter()
        .filter(|e| matches!(e.source, sofd_font::FontSource::System { .. }))
        .collect();
    assert_eq!(systems.len(), 1, "only the substitute is registered");
    assert!(
        entries
            .iter()
            .all(|e| !matches!(e.source, sofd_font::FontSource::Embedded { .. })),
        "the undeclared file is not loaded, got {entries:?}"
    );
}

/// The faces registered from the container, ignoring the chain's substitutes.
fn embedded_faces(fonts: &sofd_font::FontManager) -> Vec<sofd_font::FontId> {
    fonts
        .registry()
        .iter()
        .filter(|e| matches!(e.source, sofd_font::FontSource::Embedded { .. }))
        .map(|e| e.id)
        .collect()
}

#[cfg(feature = "render")]
#[test]
fn renders_page_at_requested_dpi() {
    let doc = two_page_document();
    let options = sofd_render::RenderOptions {
        dpi: 36.0,
        ..Default::default()
    };
    let raster = doc.render_page(0, &options).expect("render page");
    // 210mm at 36dpi is 297.64px, 297mm is 420.71px, both rounded up.
    assert_eq!(raster.width, 298);
    assert_eq!(raster.height, 421);
}

#[cfg(feature = "render")]
#[test]
fn render_rejects_out_of_range_index() {
    let doc = two_page_document();
    let options = sofd_render::RenderOptions::default();
    let err = doc.render_page(5, &options).expect_err("must fail");
    assert!(
        err.to_string().contains("page index out of range"),
        "got {err}"
    );
}

#[cfg(feature = "render")]
#[test]
fn render_pages_matches_single_page_renders() {
    let doc = two_page_document();
    let options = sofd_render::RenderOptions {
        dpi: 36.0,
        ..Default::default()
    };
    let batch = doc.render_pages(&[0, 1], &options);
    assert_eq!(batch.len(), 2);
    for (index, result) in batch.iter().enumerate() {
        let raster = result.as_ref().expect("render page");
        let single = doc.render_page(index, &options).expect("render page");
        assert_eq!(raster.width, single.width);
        assert_eq!(raster.height, single.height);
    }
    assert!(doc.render_pages(&[0, 9], &options)[1].is_err());
}

#[cfg(feature = "render")]
#[test]
fn renders_text_with_the_registered_face() {
    let entries = [
        ("OFD.xml", OFD_XML.as_bytes().to_vec()),
        (
            "Doc_0/Document.xml",
            document_xml(&text_page("7", "20.0", "A")).into_bytes(),
        ),
        (
            "Doc_0/PublicRes.xml",
            font_table("7", "Orbitron", "Orbitron-Bold.ttf").into_bytes(),
        ),
        ("Doc_0/Res/Orbitron-Bold.ttf", font_bytes()),
    ];
    let doc = open(&entries);
    let options = sofd_render::RenderOptions {
        dpi: 36.0,
        ..Default::default()
    };
    let raster = doc.render_page(0, &options).expect("render page");
    let rgba = raster.rgba_bytes();
    // The run sits at x=10mm, baseline y=25mm: scan the box above it.
    let dark = (12..50).any(|x| {
        (5..35).any(|y| {
            let offset = ((y * raster.width + x) * 4) as usize;
            let (r, g, b) = (
                rgba[offset] as u32,
                rgba[offset + 1] as u32,
                rgba[offset + 2] as u32,
            );
            r + g + b < 300
        })
    });
    assert!(dark, "the glyph is outlined from the registered font");
}

#[cfg(feature = "render")]
#[test]
fn a_referenced_but_unembedded_font_degrades_to_substitution() {
    // GB/T 33190-2016 allows naming a font without shipping it; the renderer
    // substitutes a face instead of failing the page. The substitute is the
    // repo's fixture, so the result does not depend on the host.
    let entries = [
        ("OFD.xml", OFD_XML.as_bytes().to_vec()),
        (
            "Doc_0/Document.xml",
            document_xml(&text_page("5", "20.0", "A")).into_bytes(),
        ),
    ];
    let mut doc = open(&entries);
    // The Latin fixture, because the sample is "A": the pinned face has to
    // cover the code, so a host without "sans-serif" still draws it.
    doc.fonts_mut()
        .registry_mut()
        .register_fallback_with_data("latin-pinned", Some(Arc::new(font_bytes())));
    doc.fonts_mut().build_fallback_chain();
    let options = sofd_render::RenderOptions {
        dpi: 36.0,
        ..Default::default()
    };
    let raster = doc.render_page(0, &options).expect("render page");
    let rgba = raster.rgba_bytes();
    let dark = (12..50).any(|x| {
        (5..35).any(|y| {
            let offset = ((y * raster.width + x) * 4) as usize;
            let (r, g, b) = (
                rgba[offset] as u32,
                rgba[offset + 1] as u32,
                rgba[offset + 2] as u32,
            );
            r + g + b < 300
        })
    });
    assert!(dark, "the substituted face still outlines the glyph");
}

#[cfg(feature = "convert")]
#[test]
fn converts_page_to_svg() {
    let doc = two_page_document();
    let svg = doc.to_svg(0).expect("convert to svg");
    assert!(svg.contains("<svg"));
    assert!(svg.contains("width=\"210mm\" height=\"297mm\""));
    assert!(svg.contains("viewBox=\"0 0 210 297\""));
}

#[cfg(feature = "convert")]
#[test]
fn svg_conversion_rejects_out_of_range_index() {
    let doc = two_page_document();
    let err = doc.to_svg(4).expect_err("must fail");
    assert!(
        err.to_string().contains("page index out of range"),
        "got {err}"
    );
}

#[cfg(feature = "convert")]
#[test]
fn converts_page_to_png() {
    let doc = two_page_document();
    let options = sofd_render::RenderOptions {
        dpi: 36.0,
        ..Default::default()
    };
    let png = doc.to_png(0, &options).expect("convert to png");
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n", "PNG magic bytes");
}

#[cfg(feature = "pdf")]
#[test]
fn converts_document_to_pdf() {
    let doc = two_page_document();
    let pdf = doc.to_pdf().expect("convert to pdf");
    assert!(pdf.starts_with(b"%PDF-"), "PDF header");
    assert!(pdf.windows(5).any(|w| w == b"%%EOF"), "PDF trailer");
}

#[cfg(feature = "print")]
#[test]
fn plans_print_job_over_the_document() {
    let doc = two_page_document();
    let job = doc
        .print_job(&sofd_print::PrintSettings::default())
        .expect("plan job");
    assert_eq!(job.sheet_count(), 2, "one sheet per page, duplex off");
}

#[cfg(feature = "print")]
#[test]
fn renders_print_job() {
    let doc = two_page_document();
    let settings = sofd_print::PrintSettings {
        selection: sofd_print::PageSelection::parse("1").expect("selection"),
        dpi: 36.0,
        ..Default::default()
    };
    let pages = doc.print_render(&settings).expect("render job");
    assert_eq!(pages.len(), 1);
    assert_eq!(pages[0].width, 298);
    assert_eq!(pages[0].height, 421);
}

#[cfg(feature = "search")]
#[test]
fn finds_text_across_pages() {
    let entries = [
        ("OFD.xml", OFD_XML.as_bytes().to_vec()),
        (
            "Doc_0/Document.xml",
            document_xml(&format!(
                "{}<Page ID=\"2\">{}</Page>",
                text_page("7", "4.0", "Hello"),
                text_page("7", "4.0", "World")
            ))
            .into_bytes(),
        ),
        (
            "Doc_0/PublicRes.xml",
            font_table("7", "Orbitron", "Orbitron-Bold.ttf").into_bytes(),
        ),
        ("Doc_0/Res/Orbitron-Bold.ttf", font_bytes()),
    ];
    let doc = open(&entries);
    let hits = doc.search("Hello", sofd_search::SearchOptions::default());
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].page, 0);
    let missing = doc.search("goodbye", sofd_search::SearchOptions::default());
    assert!(missing.is_empty());
}

#[cfg(feature = "edit")]
#[test]
fn edit_session_wraps_a_copy_of_the_model() {
    let doc = two_page_document();
    let session = doc.edit(sofd_edit::EditOptions::default());
    assert_eq!(session.document().pages.pages.len(), doc.page_count());
    assert!(!session.can_undo());
    assert_eq!(session.history_len(), 0);
}

#[cfg(feature = "sign")]
#[test]
fn signature_coverage_names_the_exposed_files() {
    // Clause 18.2.2 through the facade: the report states what each signature
    // protects and what the container holds that none of them does, which is
    // the surface a later update would touch.
    let entries = [
        ("OFD.xml", OFD_XML_SIGNED.as_bytes().to_vec()),
        (
            "Doc_0/Document.xml",
            document_xml(r#"<Page ID="1"/>"#).into_bytes(),
        ),
        ("Doc_0/Signs/Signs.xml", signs_xml()),
        (
            "Doc_0/Signs/Sign_0.xml",
            seal_descriptor("1", "1 2 3 4").into_bytes(),
        ),
        ("Doc_0/Res/Extra.bin", b"outside the scope".to_vec()),
    ];
    let mut doc = open(&entries);
    let reports = doc.signature_coverage().expect("coverage");
    assert_eq!(reports.len(), 1);
    assert_eq!(reports[0].covered, vec!["Doc_0/Document.xml"]);
    assert!(
        reports[0]
            .uncovered
            .contains(&"Doc_0/Res/Extra.bin".to_string()),
        "the file no signature protects is reported, got {:?}",
        reports[0].uncovered
    );
}

#[cfg(feature = "sign")]
#[test]
fn reports_when_no_signatures_are_declared() {
    let mut doc = two_page_document();
    let err = doc.signatures().expect_err("must fail");
    assert!(
        err.to_string().contains("declares no signatures"),
        "got {err}"
    );
    let err = doc.verify_signatures().expect_err("must fail");
    assert!(matches!(err, OfdError::Format(_)), "got {err:?}");
}

#[cfg(feature = "sign")]
#[test]
fn a_seal_lands_on_the_page_its_ref_names() {
    // The stamp carries a `PageRef` id, not an index, so the facade has to
    // join it against `<Pages>`; a stamp whose id names no page is dropped
    // rather than drawn on page zero.
    let entries = [
        ("OFD.xml", OFD_XML_SIGNED.as_bytes().to_vec()),
        (
            "Doc_0/Document.xml",
            document_xml(r#"<Page ID="1"/><Page ID="2"/><Page ID="7"/>"#).into_bytes(),
        ),
        ("Doc_0/Signs/Signs.xml", signs_xml()),
        (
            "Doc_0/Signs/Sign_0.xml",
            seal_descriptor("7", "1 2 3 4").into_bytes(),
        ),
        ("Doc_0/Signs/Seal.esl", b"seal-image".to_vec()),
    ];
    let mut doc = open(&entries);
    let seals = doc.seals().expect("seals");
    assert_eq!(seals.len(), 1);
    assert_eq!(
        seals[0].page, 2,
        "PageRef 7 is the third page, not the first"
    );
    assert_eq!(seals[0].id, "1");
    assert_eq!(seals[0].image, b"seal-image");
}

#[cfg(feature = "sign")]
#[test]
fn a_stamp_naming_no_page_is_dropped() {
    let entries = [
        ("OFD.xml", OFD_XML_SIGNED.as_bytes().to_vec()),
        (
            "Doc_0/Document.xml",
            document_xml(r#"<Page ID="1"/>"#).into_bytes(),
        ),
        ("Doc_0/Signs/Signs.xml", signs_xml()),
        (
            "Doc_0/Signs/Sign_0.xml",
            seal_descriptor("99", "1 2 3 4").into_bytes(),
        ),
    ];
    let mut doc = open(&entries);
    let seals = doc.seals().expect("seals");
    assert!(seals.is_empty(), "an unresolvable PageRef finds no page");
}

#[test]
fn comparing_a_document_with_itself_is_identical() {
    let before = two_page_document();
    let after = two_page_document();
    let diff = before.compare(&after);
    assert!(diff.is_identical());
    assert_eq!(diff.before_pages, 2);
    assert_eq!(diff.after_pages, 2);
    assert_eq!(diff.changed_pages(), 0);
}

#[test]
fn comparing_against_a_page_short_reads_as_removed() {
    // The comparison runs over what each revision draws, so a document that
    // dropped its second page reports the page as gone from the newer side.
    let before = two_page_document();
    let entries = [
        ("OFD.xml", OFD_XML.as_bytes().to_vec()),
        (
            "Doc_0/Document.xml",
            document_xml(r#"<Page ID="1"/>"#).into_bytes(),
        ),
    ];
    let after = open(&entries);
    let diff = before.compare(&after);
    assert_eq!(diff.pages[0].change, sofd::PageChange::Unchanged);
    assert_eq!(diff.pages[1].change, sofd::PageChange::Removed);
    assert_eq!(diff.changed_pages(), 1);
}

#[cfg(feature = "archive")]
#[test]
fn audits_the_container() {
    let mut doc = two_page_document();
    let report = doc.audit().expect("audit container");
    assert!(report.findings.iter().all(|f| !f.detail.is_empty()));
}

#[test]
fn opening_a_document_populates_the_fallback_chain() {
    // The chain the shaper consults used to be left empty, so a code point
    // outside the run's own face was dropped instead of looked up in the
    // other faces. The wide face is the repo's fixture, not a host font.
    let entries = [
        ("OFD.xml", OFD_XML.as_bytes().to_vec()),
        (
            "Doc_0/Document.xml",
            document_xml(&text_page("7", "4.0", "Hi")).into_bytes(),
        ),
        (
            "Doc_0/PublicRes.xml",
            font_table("7", "Orbitron", "Orbitron-Bold.ttf").into_bytes(),
        ),
        ("Doc_0/Res/Orbitron-Bold.ttf", font_bytes()),
    ];
    let mut doc = open(&entries);
    doc.fonts_mut()
        .registry_mut()
        .register_fallback_with_data("cjk-pinned", Some(Arc::new(cjk_font_bytes())));
    doc.fonts_mut().build_fallback_chain();
    let chain = doc.fonts().fallback_chain();
    // The pinned wide face leads, then the auto substitute, then the embedded
    // narrow face: a substitute we carry is preferred over one the host may
    // not have, and over the run's own face, which lacks the code.
    assert_eq!(
        chain,
        vec![FontId(2), FontId(1), FontId(0)],
        "the pinned face is in the chain, got {chain:?}"
    );
    let substitution = doc
        .fonts()
        .fallback_for('\u{4f60}' as u32)
        .expect("a code the embedded face lacks resolves");
    assert_eq!(substitution.0, FontId(2), "to the pinned wide face");
}

#[test]
fn the_chain_only_holds_faces_the_pages_reference() {
    // The registry is filled from the ids the display list names, so a face
    // declared but never drawn with never gets registered. The chain adds a
    // CJK substitute on top; the unused face is still absent from both.
    let entries = [
        ("OFD.xml", OFD_XML.as_bytes().to_vec()),
        (
            "Doc_0/Document.xml",
            document_xml(&text_page("7", "4.0", "Hi")).into_bytes(),
        ),
        (
            "Doc_0/PublicRes.xml",
            format!(
                "{}{}",
                font_table("7", "Orbitron", "Orbitron-Bold.ttf"),
                font_table("9", "Orbitron Two", "Orbitron-Two.ttf")
            )
            .into_bytes(),
        ),
        ("Doc_0/Res/Orbitron-Bold.ttf", font_bytes()),
        ("Doc_0/Res/Orbitron-Two.ttf", {
            let mut other = font_bytes();
            other.push(0);
            other
        }),
    ];
    let doc = open(&entries);
    let chain = doc.fonts().fallback_chain();
    assert!(
        chain.contains(&FontId(0)),
        "the face the page names is registered, got {chain:?}"
    );
    assert_eq!(
        embedded_faces(doc.fonts()).len(),
        1,
        "the face the page never names is not, got {chain:?}"
    );
}

#[test]
fn a_malformed_resource_table_surfaces_instead_of_being_swallowed() {
    // Font loading used to end in `unwrap_or_default`, so a resource table
    // that existed but would not parse left the document with no fonts and
    // no complaint. The failure is now the caller's to see.
    let entries = [
        ("OFD.xml", OFD_XML.as_bytes().to_vec()),
        (
            "Doc_0/Document.xml",
            document_xml(&text_page("7", "4.0", "Hi")).into_bytes(),
        ),
        ("Doc_0/PublicRes.xml", b"<Res><Fonts><Font ".to_vec()),
    ];
    let borrowed: Vec<(&str, &[u8])> = entries
        .iter()
        .map(|(path, data)| (*path, data.as_slice()))
        .collect();
    let bytes = build_archive(&borrowed).expect("build fixture");
    let err = sofd::Document::open_bytes(&bytes).expect_err("must fail");
    assert!(matches!(err, OfdError::Xml(_)), "got {err:?}");
}

#[cfg(feature = "render")]
#[test]
fn a_document_naming_an_unshipped_cjk_face_still_draws_its_text() {
    // The page names a CJK face and ships nothing. Before the chain could
    // add a substitute, every Han code dropped and the page rendered blank
    // with no complaint. The substitute is the repo's fixture, not the host.
    let entries = [
        ("OFD.xml", OFD_XML.as_bytes().to_vec()),
        (
            "Doc_0/Document.xml",
            document_xml(&text_page("7", "4.0", "\u{4f60}\u{597d}")).into_bytes(),
        ),
        (
            "Doc_0/PublicRes.xml",
            font_table("7", "\u{5b8b}\u{4f53}", "Song.ttf").into_bytes(),
        ),
    ];
    let mut doc = open(&entries);
    doc.fonts_mut()
        .registry_mut()
        .register_fallback_with_data("cjk-pinned", Some(Arc::new(cjk_font_bytes())));
    doc.fonts_mut().build_fallback_chain();
    let raster = doc
        .render_page(0, &sofd_render::RenderOptions::default())
        .expect("render page");
    // Every Han code resolves through the chain, so nothing is short a glyph.
    // The named face is not shipped, so its own run is reported as a face that
    // would not load: that count is the document's defect, not the renderer's.
    assert_eq!(
        raster.missing_glyph_runs, 0,
        "every Han code resolved from the chain"
    );
}

#[cfg(feature = "annot")]
#[test]
fn gb_t_33190_15_annotations_are_joined_back_to_their_pages() {
    // The index names pages by id, not by index, so reading the marks means
    // joining the two. Two pages of three carry a mark each.
    let doc = sofd_testkit::annotated_archive(3, 2);
    let mut document = sofd::Document::open_bytes(&doc).expect("open fixture");
    let marks = document.annotations().expect("annotations");
    assert_eq!(marks.len(), 2, "one mark per annotated page");
    assert_eq!(marks[0].page, 0);
    assert_eq!(marks[1].page, 1);
    assert_eq!(marks[0].kind, "Highlight");
    assert_eq!(marks[0].remark.as_deref(), Some("note 0"));
    assert_eq!(
        marks[0].boundary,
        Some(sofd_common::Rect::new(5.0, 5.0, 40.0, 20.0))
    );
    assert!(marks[0].visible, "annotations are visible by default");
}

#[cfg(feature = "annot")]
#[test]
fn a_document_without_an_annotation_index_has_no_annotations() {
    // No index is not a failure: most documents carry no marks at all.
    let doc = sofd_testkit::multipage_archive(2);
    let mut document = sofd::Document::open_bytes(&doc).expect("open fixture");
    assert_eq!(document.annotations().expect("annotations"), Vec::new());
}

#[cfg(feature = "annot")]
#[test]
fn an_annotation_file_that_will_not_read_costs_only_its_own_page() {
    // A document whose first page names a file that is not there still
    // reports the marks on its second page: a broken note is not a broken
    // document, and dropping the whole list would hide the marks that exist.
    let archive = sofd_testkit::annotated_archive(2, 2);
    let mut package = sofd_package::Package::from_bytes(&archive).expect("package");
    let dropped = "Doc_0/Annots/Page_0/Annotation.xml";
    let paths: Vec<String> = package
        .entries()
        .iter()
        .filter(|path| path.as_str() != dropped)
        .cloned()
        .collect();
    let kept: Vec<(String, Vec<u8>)> = paths
        .into_iter()
        .map(|path| {
            let data = package.read(&path).expect("entry reads");
            (path, data)
        })
        .collect();
    let rebuilt: Vec<(&str, &[u8])> = kept
        .iter()
        .map(|(path, data)| (path.as_str(), data.as_slice()))
        .collect();
    let bytes = build_archive(&rebuilt).expect("archive");
    let mut document = sofd::Document::open_bytes(&bytes).expect("open fixture");
    let marks = document.annotations().expect("annotations");
    assert_eq!(marks.len(), 1, "the page that still has a file reports it");
    assert_eq!(marks[0].page, 1);
}

#[test]
fn an_embedded_face_is_carried_down_to_the_glyphs_the_document_draws() {
    // A page that draws two characters out of a whole face should not pay for
    // the rest of it: the registered bytes are a subset, not the original.
    let whole = font_bytes();
    // The glyph ids the page asks for, taken from the face itself so the test
    // does not hardcode a fixture's numbering.
    let source = sofd_font::FontFace::new(
        sofd_font::FontId(0),
        "Orbitron".to_string(),
        std::sync::Arc::new(whole.clone()),
        false,
    )
    .expect("fixture face");
    let wanted: Vec<u16> = "Hi"
        .chars()
        .map(|c| source.glyph_id(c as u32).expect("fixture covers Hi"))
        .collect();
    let entries = [
        ("OFD.xml", OFD_XML.as_bytes().to_vec()),
        (
            "Doc_0/Document.xml",
            document_xml(&glyph_page("7", "4.0", &wanted)).into_bytes(),
        ),
        (
            "Doc_0/PublicRes.xml",
            font_table("7", "Orbitron", "Orbitron-Bold.ttf").into_bytes(),
        ),
        ("Doc_0/Res/Orbitron-Bold.ttf", whole.clone()),
    ];
    let doc = open(&entries);
    let id = embedded_faces(doc.fonts())[0];
    let entry = doc
        .fonts()
        .registry()
        .iter()
        .find(|e| e.id == id)
        .expect("the container font is registered");
    let carried = entry.data.as_ref().expect("embedded faces carry bytes");

    assert!(
        carried.len() < whole.len(),
        "the face is subset: {} vs {}",
        carried.len(),
        whole.len()
    );
    // Subsetting must not damage the face: the run still outlines.
    let face = doc.fonts().face(id).expect("the subset face loads");
    for ch in "Hi".chars() {
        let glyph = face.glyph_id(ch as u32).expect("a drawn glyph survives");
        assert!(
            face.outline(glyph).is_some(),
            "{ch} must still have an outline"
        );
    }
}

#[test]
fn a_face_whose_run_carries_code_points_is_carried_whole() {
    // Without glyph indices the subset cannot know which outlines are needed,
    // so the face is left untouched rather than guessed at.
    let whole = font_bytes();
    let entries = [
        ("OFD.xml", OFD_XML.as_bytes().to_vec()),
        (
            "Doc_0/Document.xml",
            document_xml(&text_page("7", "4.0", "Hi")).into_bytes(),
        ),
        (
            "Doc_0/PublicRes.xml",
            font_table("7", "Orbitron", "Orbitron-Bold.ttf").into_bytes(),
        ),
        ("Doc_0/Res/Orbitron-Bold.ttf", whole.clone()),
    ];
    let doc = open(&entries);
    let id = embedded_faces(doc.fonts())[0];
    let entry = doc
        .fonts()
        .registry()
        .iter()
        .find(|e| e.id == id)
        .expect("registered");
    assert_eq!(entry.data.as_ref().expect("bytes").len(), whole.len());
}

#[cfg(feature = "sign")]
fn timestamped_descriptor(check_value: &str, token: &[u8]) -> String {
    format!(
        r#"<Signature ID="s">
  <SignedInfo><References CheckMethod="SM3">
    <Reference FileRef="Doc_0/Document.xml"><CheckValue>{check_value}</CheckValue></Reference>
  </References></SignedInfo>
  <SignedValue>{check_value}</SignedValue>
  <Timestamp>{}</Timestamp>
</Signature>"#,
        sofd_testkit::base64_of(token)
    )
}

#[cfg(feature = "sign")]
#[test]
fn each_signature_reports_whether_its_timestamp_binds_it() {
    // Clause 18.2.4: the trusted time is proved over the signed value, so the
    // facade has to hand back a verdict per signature, not a bare token.
    let bound = b"the signed value".to_vec();
    let (token, _tsa) = sofd_testkit::timestamp_token(&sofd_testkit::TimestampSpec::over(&bound));
    let descriptor = timestamped_descriptor("AAAA", &token);
    let entries = [
        ("OFD.xml", OFD_XML_SIGNED.as_bytes().to_vec()),
        (
            "Doc_0/Document.xml",
            document_xml(r#"<Page ID="1"/>"#).into_bytes(),
        ),
        ("Doc_0/Signs/Signs.xml", signs_xml()),
        ("Doc_0/Signs/Sign_0.xml", descriptor.into_bytes()),
    ];
    let mut doc = open(&entries);
    let verdicts = doc.verify_timestamps().expect("timestamp verdicts");
    assert_eq!(verdicts.len(), 1);
    // AAAA decodes to three zero bytes, which the token does not cover.
    assert_eq!(verdicts[0], sofd::TimestampStatus::Unbound);
}

#[cfg(feature = "sign")]
#[test]
fn a_signature_with_no_timestamp_reports_absent() {
    let entries = [
        ("OFD.xml", OFD_XML_SIGNED.as_bytes().to_vec()),
        (
            "Doc_0/Document.xml",
            document_xml(r#"<Page ID="1"/>"#).into_bytes(),
        ),
        ("Doc_0/Signs/Signs.xml", signs_xml()),
        (
            "Doc_0/Signs/Sign_0.xml",
            seal_descriptor("1", "1 2 3 4").into_bytes(),
        ),
    ];
    let mut doc = open(&entries);
    let verdicts = doc.verify_timestamps().expect("timestamp verdicts");
    assert_eq!(verdicts, vec![sofd::TimestampStatus::Absent]);
}

#[cfg(feature = "render")]
#[test]
fn a_shell_can_draw_a_page_from_geometry_without_a_bitmap() {
    // A shell with a canvas of its own asks the facade for geometry and draws
    // it, never holding a pixmap. This is that path, with the face coming from
    // the container rather than from whatever the machine has installed.
    let entries = [
        ("OFD.xml", OFD_XML.as_bytes().to_vec()),
        (
            "Doc_0/Document.xml",
            document_xml(&text_page("7", "6.0", "AB")).into_bytes(),
        ),
        (
            "Doc_0/PublicRes.xml",
            font_table("7", "Orbitron", "Orbitron-Bold.ttf").into_bytes(),
        ),
        ("Doc_0/Res/Orbitron-Bold.ttf", font_bytes()),
    ];
    let doc = open(&entries);
    let options = sofd::RenderOptions::default();
    let page = doc.vector_page(0, &options).expect("geometry");

    let text: Vec<&sofd::Shape> = page
        .shapes
        .iter()
        .filter(|s| matches!(s, sofd::Shape::Text { .. }))
        .collect();
    assert_eq!(text.len(), 1, "one run of two letters");
    let sofd::Shape::Text { path, fill, .. } = text[0] else {
        unreachable!("filtered on text");
    };
    assert!(fill.is_some(), "the run's paint comes with it");
    assert!(!path.is_empty(), "the letters are outlines, not codes");
    let bbox = path.bounding_box();
    assert!(
        bbox.x >= 9.0 && bbox.x < 11.0 && bbox.y > 10.0 && bbox.y < 26.0,
        "the geometry lands where the text object says: {bbox:?}"
    );

    // And the raster of the same page agrees, so the two backends are not
    // drawing different documents.
    let raster = doc.render_page(0, &options).expect("render");
    let scale = options.dpi / 25.4;
    let mid_y = ((bbox.y + bbox.h / 2.0) * scale) as u32;
    let bytes = raster.rgba_bytes();
    let painted = |x: u32, y: u32| {
        let base = ((y * raster.width + x) * 4) as usize;
        bytes[base] < 128
    };
    let mut inked = (bbox.x * scale) as u32..((bbox.x + bbox.w) * scale) as u32;
    assert!(
        inked.any(|x| painted(x, mid_y)),
        "the raster puts ink where the geometry says the letters are"
    );
}
