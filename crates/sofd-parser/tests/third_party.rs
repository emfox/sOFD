//! Regressions found by opening another implementation's test documents: six
//! constructs a real producer emits were refused outright, each as a hard
//! parse error rather than a page that merely looked wrong.

use sofd_display::DrawOp;
use sofd_package::{Package, build_archive};
use sofd_parser::{ParsedDocument, Parser};

const OFD_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<OFD Version="1.0">
  <DocBody><DocRoot>Doc_0/Document.xml</DocRoot></DocBody>
</OFD>"#;

fn document_xml(common: &str, pages: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Document>
  <CommonData>
    <PageArea><PhysicalBox>0 0 210 297</PhysicalBox></PageArea>
    {common}
  </CommonData>
  <Pages>{pages}</Pages>
</Document>"#
    )
}

fn content_xml(body: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Page>
  <Content><Layer ID="1">{body}</Layer></Content>
</Page>"#
    )
}

/// A parser over a package built from string entries, kept alive for the test.
/// The parser borrows its package for as long as it lives, so each fixture
/// leaks one; a leak per test is cheaper than the lifetime plumbing.
struct Fixture(Parser<'static>);

impl Fixture {
    fn build(entries: &[(&str, String)]) -> Self {
        let borrowed: Vec<(&str, &[u8])> = entries
            .iter()
            .map(|(path, text)| (*path, text.as_bytes()))
            .collect();
        let bytes = build_archive(&borrowed).expect("archive");
        let package = Package::from_bytes(&bytes).expect("package");
        Fixture(Parser::new(Box::leak(Box::new(package))))
    }

    /// A one-page document carrying `body` in its only layer.
    fn page(common: &str, body: &str) -> Self {
        Fixture::build(&[
            ("OFD.xml", OFD_XML.to_string()),
            (
                "Doc_0/Document.xml",
                document_xml(
                    common,
                    r#"<Page ID="1" BaseLoc="Pages/Page_0/Content.xml"/>"#,
                ),
            ),
            ("Doc_0/Pages/Page_0/Content.xml", content_xml(body)),
        ])
    }

    fn parse(&mut self) -> ParsedDocument {
        self.0.open_document().expect("the document parses")
    }

    /// Flatten page 0, which is what every one of these shapes has to survive,
    /// and return the text it placed.
    fn flatten(&mut self, doc: &ParsedDocument) -> String {
        let page = self.0.page_display(doc, 0).expect("page display");
        let mut out = String::new();
        for op in &page.ops {
            if let DrawOp::Text(run) = op
                && let Some(text) = &run.text
            {
                out.push_str(text);
                out.push('\n');
            }
        }
        out
    }
}

/// A list may state one offset many times with `g count value`. A Chinese
/// invoice writes `DeltaX="g 11 1.5875 -17.4625"` for a line of digits, so
/// refusing the token refused the document.
#[test]
fn a_delta_list_may_repeat_one_offset_with_g() {
    let mut fixture = Fixture::page(
        "",
        r#"<TextObject ID="2" Boundary="0 0 100 10" Font="3" Size="3">
             <TextCode X="0" Y="5" DeltaX="g 7 1.5875 -11.1125">Zhejiang</TextCode>
           </TextObject>"#,
    );
    let doc = fixture.parse();
    let text = fixture.flatten(&doc);
    assert!(text.contains("Zhejiang"), "the run is placed, got {text:?}");
}

/// A `g` with no count states nothing, and that is an error rather than a
/// guess: a list this file cannot read is a placement nobody can vouch for.
#[test]
fn a_g_without_its_count_is_refused() {
    let mut fixture = Fixture::page(
        "",
        r#"<TextObject ID="2" Boundary="0 0 100 10" Font="3" Size="3">
             <TextCode X="0" Y="5" DeltaX="2.5 g">abc</TextCode>
           </TextObject>"#,
    );
    let doc = fixture.parse();
    match fixture.0.page_display(&doc, 0) {
        Err(err) => assert!(err.to_string().contains("DeltaX"), "got {err}"),
        Ok(_) => panic!("a g with no count must not be read as an offset"),
    }
}

/// `B` is a cubic Bezier from the current point, so six numbers where `C`
/// carries the same six plus the repeated start. A Chinese invoice draws its
/// supervisory seal almost entirely with it.
#[test]
fn a_path_accepts_the_implicit_start_bezier_command() {
    let mut fixture = Fixture::page(
        "",
        r#"<PathObject ID="2" Boundary="0 0 100 100" LineWidth="0.5">
             <AbbreviatedData>M 10 5 B 10 3 8 1 5.5 1 B 3 1 1 3 1 5</AbbreviatedData>
           </PathObject>"#,
    );
    let doc = fixture.parse();
    assert!(fixture.0.page_display(&doc, 0).is_ok());
}

/// `TemplatePage` hangs off `CommonData` with no wrapper element; only the
/// wrapped form was accepted, so a templated document failed to open.
#[test]
fn a_template_page_is_declared_straight_into_common_data() {
    let mut fixture = Fixture::build(&[
        ("OFD.xml", OFD_XML.to_string()),
        (
            "Doc_0/Document.xml",
            document_xml(
                r#"<TemplatePage ID="2" BaseLoc="Tpls/Tpl_0/Content.xml"/>"#,
                r#"<Page ID="1" BaseLoc="Pages/Page_0/Content.xml"/>"#,
            ),
        ),
        (
            "Doc_0/Pages/Page_0/Content.xml",
            r#"<?xml version="1.0" encoding="UTF-8"?>
<Page>
  <Template TemplateID="2" ZOrder="Background"/>
  <Content><Layer ID="1">
    <TextObject ID="6" Boundary="0 0 100 10" Font="3" Size="3">
      <TextCode X="0" Y="5" DeltaX="3">own layer</TextCode>
    </TextObject>
  </Layer></Content>
</Page>"#
                .to_string(),
        ),
        (
            "Doc_0/Tpls/Tpl_0/Content.xml",
            r#"<?xml version="1.0" encoding="UTF-8"?>
<Page>
  <Content><Layer ID="1">
    <TextObject ID="4" Boundary="0 0 100 10" Font="3" Size="3">
      <TextCode X="0" Y="5" DeltaX="3">template text</TextCode>
    </TextObject>
  </Layer></Content>
</Page>"#
                .to_string(),
        ),
    ]);
    let doc = fixture.parse();
    let text = fixture.flatten(&doc);
    assert!(
        text.contains("template text"),
        "the template's own content is drawn underneath, got {text:?}"
    );
    assert!(text.contains("own layer"), "the page keeps its own content");
}

/// `DocRoot` is package-root relative and producers write it both with and
/// without a leading slash. The slash names the same entry, and reading it as
/// written made the document look like it were missing its content.
#[test]
fn a_doc_root_may_carry_a_leading_slash() {
    let mut fixture = Fixture::build(&[
        (
            "OFD.xml",
            r#"<?xml version="1.0" encoding="UTF-8"?>
<OFD Version="1.0"><DocBody><DocRoot>/Doc_0/Document.xml</DocRoot></DocBody></OFD>"#
                .to_string(),
        ),
        (
            "Doc_0/Document.xml",
            document_xml("", r#"<Page ID="1" BaseLoc="Pages/Page_0/Content.xml"/>"#),
        ),
        (
            "Doc_0/Pages/Page_0/Content.xml",
            content_xml(
                r#"<TextObject ID="2" Boundary="0 0 100 10" Font="3" Size="3">
                     <TextCode X="0" Y="5" DeltaX="3">invoice</TextCode>
                   </TextObject>"#,
            ),
        ),
    ]);
    let doc = fixture.parse();
    assert!(fixture.flatten(&doc).contains("invoice"));
}

/// GB/T 33190 6.2.003 writes `DocRoot` as `./Doc_0/Document.xml`, a relative
/// form with an explicit current-directory segment. Read verbatim it named an
/// entry no archive holds, so the document was refused as if corrupted.
#[test]
fn a_doc_root_may_be_written_with_a_current_directory_segment() {
    let mut fixture = Fixture::build(&[
        (
            "OFD.xml",
            r#"<?xml version="1.0" encoding="UTF-8"?>
<OFD Version="1.0"><DocBody><DocRoot>./Doc_0/Document.xml</DocRoot></DocBody></OFD>"#
                .to_string(),
        ),
        (
            "Doc_0/Document.xml",
            document_xml("", r#"<Page ID="1" BaseLoc="Pages/Page_0/Content.xml"/>"#),
        ),
        (
            "Doc_0/Pages/Page_0/Content.xml",
            content_xml(
                r#"<TextObject ID="2" Boundary="0 0 100 10" Font="3" Size="3">
                     <TextCode X="0" Y="5" DeltaX="3">invoice</TextCode>
                   </TextObject>"#,
            ),
        ),
    ]);
    let doc = fixture.parse();
    assert_eq!(doc.doc_dir, "Doc_0");
    assert!(fixture.flatten(&doc).contains("invoice"));
}
/// `#ee #20 #25`. Read as one six-digit hex string, it failed -- while the
/// same document's `#0 #0 #0` passed, which is how the bug hid.
#[test]
fn a_colour_may_be_written_as_hash_prefixed_hex_channels() {
    use sofd_model::color::ColorRef;

    let marked = ColorRef::from_hex("#ee #20 #25");
    assert_eq!(marked.to_rgb().expect("marked channels"), (238, 32, 37));

    // The marker is what picks the base, so the bare form stays decimal.
    let bare = ColorRef::from_hex("156 82 35");
    assert_eq!(bare.to_rgb().expect("bare channels"), (156, 82, 35));

    // Both spellings of black, which is what hid the bug.
    assert_eq!(
        ColorRef::from_hex("#0 #0 #0").to_rgb().expect("hex black"),
        (0, 0, 0)
    );
    assert_eq!(
        ColorRef::from_hex("0 0 0").to_rgb().expect("decimal black"),
        (0, 0, 0)
    );
}

/// A colour carrying a gradient has no flat value: the gradient is the
/// colour. Text skips the shading-aware path, so such a run read as an
/// invalid colour and refused the whole document.
#[test]
fn a_gradient_filled_text_run_is_not_an_invalid_colour() {
    let mut fixture = Fixture::page(
        "",
        r#"<TextObject ID="2" Boundary="0 0 100 10" Font="3" Size="3">
             <FillColor>
               <AxialShd StartPoint="0 0" EndPoint="100 0">
                 <Segment Position="0"><Color Value="0 140 207"/></Segment>
                 <Segment Position="1"><Color Value="100 192 171"/></Segment>
               </AxialShd>
             </FillColor>
             <TextCode X="0" Y="5" DeltaX="3">gradient</TextCode>
           </TextObject>"#,
    );
    let doc = fixture.parse();
    assert!(
        fixture.0.page_display(&doc, 0).is_ok(),
        "a gradient-filled run flattens rather than failing on the flat value"
    );
}

/// An empty `<Color>` is the OFD default, not an error. Alpha defaults to 255
/// on the way in, and counting that alpha as a colour made an omitted value
/// parse as an invalid one.
#[test]
fn a_colour_without_a_value_is_the_default_not_an_error() {
    use sofd_model::color::ColorRef;

    let omitted = ColorRef::from_hex("").with_alpha(255);
    assert!(omitted.is_default(), "alpha alone is not a colour");
    assert_eq!(omitted.to_rgb().expect("default black"), (0, 0, 0));
}

/// Open a document another implementation wrote, kept in the tree under
/// `compliance/fixtures` so the annex A gate validates it beside ours.
fn committed_fixture(name: &str) -> Fixture {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../compliance/fixtures")
        .join(name);
    let bytes = std::fs::read(&path).expect("the fixture is committed");
    let package = Package::from_bytes(&bytes).expect("open the fixture package");
    Fixture(Parser::new(Box::leak(Box::new(package))))
}

#[test]
fn a_document_another_producer_wrote_survives_to_the_page() {
    // Suwell OFD convertor writes one text object per phrase, each stating its
    // own origin, and mixes an image in between. None of that is what our own
    // writer emits, so this reads another producer's shape rather than ours.
    let mut fixture = committed_fixture("suwell.ofd");
    let document = fixture.parse();
    let page = fixture.0.page_display(&document, 0).expect("page display");
    let texts: Vec<_> = page
        .ops
        .iter()
        .filter_map(|op| match op {
            DrawOp::Text(run) => Some(run),
            _ => None,
        })
        .collect();
    assert_eq!(texts.len(), 36, "one run per text object the page states");
    assert!(
        texts
            .iter()
            .all(|run| !run.codes_are_glyphs && run.len() == run.plain_text().chars().count()),
        "every run spells characters, and the glyphs match the text"
    );
    // The heading opens the document, so it is the first run on the page.
    assert!(
        texts[0].plain_text().starts_with("关于"),
        "got {:?}",
        texts[0].plain_text()
    );
    assert!(
        page.ops.iter().any(|op| matches!(op, DrawOp::Image { .. })),
        "and the background image is drawn, not dropped"
    );
    assert!(!page.is_blank(), "the page carries ink");
}
