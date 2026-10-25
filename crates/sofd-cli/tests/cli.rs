//! CLI coverage: every subcommand driven in-process against a built fixture.
//! The fixture follows the spec form of ST_Box (element text), so the runs
//! exercise the same paths real documents take.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU32, Ordering};

use sofd_cli::Report;
use sofd_package::build_archive;

const OFD_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<OFD Version="1.0">
  <DocBody><DocRoot>Doc_0/Document.xml</DocRoot></DocBody>
</OFD>"#;

const DOCUMENT_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Document>
  <CommonData>
    <PageArea><PhysicalBox>0 0 210 297</PhysicalBox></PageArea>
  </CommonData>
  <Pages><Page ID="1" BaseLoc="Pages/Page_0/Content.xml"/></Pages>
</Document>"#;

const CONTENT_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Page>
  <Area><PhysicalBox>0 0 210 297</PhysicalBox></Area>
  <Content>
    <Layer ID="2" Type="Body">
      <PathObject ID="3" Boundary="10 10 60 20" Fill="true">
        <FillColor Value="255 0 0"/>
        <AbbreviatedData>M 10 10 L 70 10 L 70 30 L 10 30 L 10 10</AbbreviatedData>
      </PathObject>
      <TextObject ID="4" Boundary="10 40 90 20" Font="7" Size="3.5">
        <FillColor Value="0 0 0"/>
        <TextCode X="10" Y="50">sOFD</TextCode>
      </TextObject>
    </Layer>
  </Content>
</Page>"#;

/// A real face, so glyph outlining runs for real rather than degrading.
fn font_bytes() -> Vec<u8> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../sofd-render/tests/data/Orbitron-Bold.ttf"
    );
    std::fs::read(path).expect("font fixture")
}

/// A private scratch directory per call, so parallel tests never collide.
fn scratch(name: &str) -> PathBuf {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "sofd-cli-{}-{}-{}",
        std::process::id(),
        name,
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir_all(&dir).expect("scratch dir");
    dir
}

/// The fixture document on disk, ready to name on a command line.
fn fixture_path() -> PathBuf {
    let entries = [
        ("OFD.xml", OFD_XML.as_bytes().to_vec()),
        ("Doc_0/Document.xml", DOCUMENT_XML.as_bytes().to_vec()),
        (
            "Doc_0/Pages/Page_0/Content.xml",
            CONTENT_XML.as_bytes().to_vec(),
        ),
        (
            "Doc_0/PublicRes.xml",
            br#"<?xml version="1.0" encoding="UTF-8"?>
<Res BaseLoc="Res"><Fonts><Font ID="7" FontName="Orbitron"><FontFile>Orbitron-Bold.ttf</FontFile></Font></Fonts></Res>"#
                .to_vec(),
        ),
        ("Doc_0/Res/Orbitron-Bold.ttf", font_bytes()),
    ];
    let borrowed: Vec<(&str, &[u8])> = entries
        .iter()
        .map(|(path, data)| (*path, data.as_slice()))
        .collect();
    let bytes = build_archive(&borrowed).expect("build fixture");
    let path = scratch("fixture").join("fixture.ofd");
    std::fs::write(&path, bytes).expect("write fixture");
    path
}

/// A shipped fixture by its path under `tests/fixtures`, for the commands
/// whose answer only a real document can give.
fn fixture(relative: &str) -> PathBuf {
    let path =
        PathBuf::from(concat!(env!("CARGO_MANIFEST_DIR"), "/../../tests/fixtures")).join(relative);
    assert!(path.exists(), "fixture missing: {}", path.display());
    path
}

fn report_for(args: &[&str]) -> Report {
    let cli = sofd_cli::parse(args).expect("parse");
    let i18n = sofd_cli::i18n_for(&cli);
    sofd_cli::run(&cli, &i18n).expect("run")
}

fn error_for(args: &[&str]) -> Report {
    let cli = sofd_cli::parse(args).expect("parse");
    let i18n = sofd_cli::i18n_for(&cli);
    let err = sofd_cli::run(&cli, &i18n).expect_err("must fail");
    sofd_cli::error_report(&err, &i18n)
}

#[test]
fn info_reports_what_the_document_declares() {
    let path = fixture_path();
    let report = report_for(&["info", "--locale", "en-US", path.to_str().unwrap()]);
    assert!(report.ok);
    assert!(report.text.contains("Version: 1.0"), "{}", report.text);
    assert!(report.text.contains("Pages: 1"), "{}", report.text);
    assert!(
        report.text.contains("Page size: 210.0 x 297.0 mm"),
        "{}",
        report.text
    );
    assert!(report.text.contains("Embedded fonts: 1"), "{}", report.text);
    assert!(report.text.contains("Signatures: 0"), "{}", report.text);
}

#[test]
fn info_localizes_to_chinese() {
    let path = fixture_path();
    let report = report_for(&["info", "--locale", "zh-CN", path.to_str().unwrap()]);
    assert!(report.text.contains("页数：1"), "{}", report.text);
    assert!(
        report.text.contains("页面尺寸：210.0 x 297.0 毫米"),
        "{}",
        report.text
    );
}

#[test]
fn render_writes_a_png() {
    let path = fixture_path();
    let out = scratch("render").join("page.png");
    let report = report_for(&[
        "render",
        "--locale",
        "en-US",
        path.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);
    assert!(report.ok);
    assert!(
        report.text.contains("Rendered page 0 at 150 dpi"),
        "{}",
        report.text
    );
    assert!(report.text.contains("Wrote "), "{}", report.text);
    let png = std::fs::read(&out).expect("read png");
    assert!(png.starts_with(&[0x89, b'P', b'N', b'G']), "not a PNG");
    assert!(png.len() > 1000, "the page rendered to almost nothing");
}

/// A document whose only image is bytes no decoder accepts.
fn broken_image_path() -> PathBuf {
    const PAGE: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<Page>
  <Area><PhysicalBox>0 0 210 297</PhysicalBox></Area>
  <Content>
    <Layer ID="2" Type="Body">
      <ImageObject ID="3" Boundary="10 10 50 50" ResourceID="9">
        <ImageClip ClipArea="10 10 50 50"/>
      </ImageObject>
    </Layer>
  </Content>
</Page>"#;
    let entries = [
        ("OFD.xml", OFD_XML.as_bytes().to_vec()),
        ("Doc_0/Document.xml", DOCUMENT_XML.as_bytes().to_vec()),
        ("Doc_0/Pages/Page_0/Content.xml", PAGE.as_bytes().to_vec()),
        (
            "Doc_0/DocumentRes.xml",
            br#"<?xml version="1.0" encoding="UTF-8"?>
<Res BaseLoc="Res"><MultiMedias><MultiMedia ID="9" Type="Image"><MediaFile>gone.png</MediaFile></MultiMedia></MultiMedias></Res>"#
                .to_vec(),
        ),
        ("Doc_0/Res/gone.png", b"not a PNG".to_vec()),
    ];
    let borrowed: Vec<(&str, &[u8])> = entries
        .iter()
        .map(|(path, data)| (*path, data.as_slice()))
        .collect();
    let bytes = build_archive(&borrowed).expect("build fixture");
    let path = scratch("broken-image").join("broken.ofd");
    std::fs::write(&path, bytes).expect("write fixture");
    path
}

#[test]
fn render_reports_an_image_it_could_not_draw() {
    // The page still writes, so the warning is the only sign that part of it
    // never made it into the picture.
    let path = broken_image_path();
    let out = scratch("broken-render").join("page.png");
    let report = report_for(&[
        "render",
        "--locale",
        "en-US",
        path.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);
    assert!(out.is_file(), "the page was written anyway");
    assert!(
        !report.ok,
        "a page short of an image must not report success: {}",
        report.text
    );
    assert!(
        report
            .warnings
            .iter()
            .any(|w| w.contains("1 image could not be drawn")),
        "warnings were {:?}",
        report.warnings,
    );
}

#[test]
fn render_says_nothing_about_images_when_all_of_them_draw() {
    let path = fixture_path();
    let out = scratch("clean-render").join("page.png");
    let report = report_for(&[
        "render",
        path.to_str().unwrap(),
        "-o",
        out.to_str().unwrap(),
    ]);
    assert!(
        !report.warnings.iter().any(|w| w.contains("image")),
        "warnings were {:?}",
        report.warnings,
    );
}

#[test]
fn render_rejects_an_out_of_range_page() {
    let path = fixture_path();
    let report = error_for(&[
        "render",
        "--locale",
        "en-US",
        path.to_str().unwrap(),
        "-p",
        "9",
    ]);
    assert!(!report.ok);
    assert!(
        report.text.contains("Page index out of range"),
        "{}",
        report.text
    );
    assert!(report.text.starts_with("error:"), "{}", report.text);
}

#[test]
fn convert_to_pdf_holds_the_whole_document() {
    let path = fixture_path();
    let dir = scratch("pdf");
    let report = report_for(&[
        "convert",
        "--locale",
        "en-US",
        path.to_str().unwrap(),
        "-f",
        "pdf",
        "-o",
        dir.to_str().unwrap(),
    ]);
    assert!(report.ok);
    assert!(
        report.text.contains("Converted 1 page to pdf"),
        "{}",
        report.text
    );
    let pdf = std::fs::read(dir.join("fixture.pdf")).expect("read pdf");
    assert!(pdf.starts_with(b"%PDF"), "not a PDF");
}

/// A three-page document whose first page is a short receipt and whose
/// continuations are A4, written out ready to name on a command line.
fn mixed_page_path() -> PathBuf {
    let document = DOCUMENT_XML
        .replace(
            r#"<Pages><Page ID="1" BaseLoc="Pages/Page_0/Content.xml"/></Pages>"#,
            r#"<Pages>
    <Page ID="1" BaseLoc="Pages/Page_0/Content.xml"/>
    <Page ID="2" BaseLoc="Pages/Page_1/Content.xml"/>
    <Page ID="3" BaseLoc="Pages/Page_2/Content.xml"/>
  </Pages>"#,
        )
        .replace(
            "0 0 210 297</PhysicalBox></PageArea>",
            "0 0 210 140</PhysicalBox></PageArea>",
        );
    let entries = [
        ("OFD.xml", OFD_XML.as_bytes().to_vec()),
        ("Doc_0/Document.xml", document.into_bytes()),
        (
            "Doc_0/Pages/Page_0/Content.xml",
            CONTENT_XML.as_bytes().to_vec(),
        ),
        (
            "Doc_0/Pages/Page_1/Content.xml",
            CONTENT_XML.as_bytes().to_vec(),
        ),
        (
            "Doc_0/Pages/Page_2/Content.xml",
            CONTENT_XML.as_bytes().to_vec(),
        ),
        (
            "Doc_0/PublicRes.xml",
            br#"<?xml version="1.0" encoding="UTF-8"?>
<Res BaseLoc="Res"><Fonts><Font ID="7" FontName="Orbitron"><FontFile>Orbitron-Bold.ttf</FontFile></Font></Fonts></Res>"#
                .to_vec(),
        ),
        ("Doc_0/Res/Orbitron-Bold.ttf", font_bytes()),
    ];
    let borrowed: Vec<(&str, &[u8])> = entries
        .iter()
        .map(|(path, data)| (*path, data.as_slice()))
        .collect();
    let bytes = build_archive(&borrowed).expect("build fixture");
    let path = scratch("mixed").join("mixed.ofd");
    std::fs::write(&path, bytes).expect("write fixture");
    path
}

#[test]
fn convert_to_pdf_writes_every_page_at_its_own_size() {
    // Converting one page at a time and keeping the last writes a single page
    // under the document's name, which reads as a success. Each page also
    // keeps its own box, so a short receipt is not padded out to A4.
    let path = mixed_page_path();
    let dir = scratch("pdf-multi");
    let report = report_for(&[
        "convert",
        "--locale",
        "en-US",
        path.to_str().unwrap(),
        "-f",
        "pdf",
        "-o",
        dir.to_str().unwrap(),
    ]);
    assert!(report.ok, "{}", report.text);
    assert!(
        report.text.contains("Converted 3 pages to pdf"),
        "{}",
        report.text
    );
    let pdf = std::fs::read(dir.join("mixed.pdf")).expect("read pdf");
    let text = String::from_utf8_lossy(&pdf);
    assert!(
        text.contains("/Count 3"),
        "the page tree does not hold three pages"
    );
    assert_eq!(
        text.matches("/MediaBox").count(),
        3,
        "one MediaBox per page"
    );
}

#[test]
fn convert_to_svg_and_png_writes_per_page_files() {
    let path = fixture_path();
    let dir = scratch("svg-png");
    let report = report_for(&[
        "convert",
        "--locale",
        "en-US",
        path.to_str().unwrap(),
        "-f",
        "svg",
        "-o",
        dir.to_str().unwrap(),
    ]);
    assert!(report.ok);
    let svg = std::fs::read(dir.join("fixture-0.svg")).expect("read svg");
    let svg = String::from_utf8(svg).expect("utf8 svg");
    assert!(svg.contains("<svg"), "{}", svg);

    let report = report_for(&[
        "convert",
        "--locale",
        "en-US",
        path.to_str().unwrap(),
        "-f",
        "png",
        "-o",
        dir.to_str().unwrap(),
    ]);
    assert!(report.ok);
    let png = std::fs::read(dir.join("fixture-0.png")).expect("read png");
    assert!(png.starts_with(&[0x89, b'P', b'N', b'G']), "not a PNG");
}

#[test]
fn search_finds_the_text_on_its_page() {
    let path = fixture_path();
    let report = report_for(&[
        "search",
        "--locale",
        "en-US",
        path.to_str().unwrap(),
        "sOFD",
    ]);
    assert!(report.ok);
    assert!(report.text.contains("page 0:"), "{}", report.text);
    assert!(report.text.contains("sOFD"), "{}", report.text);
}

#[test]
fn search_without_hits_is_empty_but_ok() {
    let path = fixture_path();
    let report = report_for(&[
        "search",
        "--locale",
        "en-US",
        path.to_str().unwrap(),
        "no-such-text",
    ]);
    assert!(report.ok);
    assert!(report.text.is_empty(), "{}", report.text);
}

#[test]
fn audit_reports_findings_without_failing() {
    let path = fixture_path();
    let report = report_for(&["audit", "--locale", "en-US", path.to_str().unwrap()]);
    assert!(report.ok, "warnings must not fail the run: {}", report.text);
    assert!(report.text.contains("ARCH-006"), "{}", report.text);
}

#[test]
fn verify_without_signatures_fails_the_run() {
    let path = fixture_path();
    let report = report_for(&["verify", "--locale", "en-US", path.to_str().unwrap()]);
    assert!(!report.ok);
    assert!(
        report.text.contains("The document declares no signatures"),
        "{}",
        report.text
    );
}

#[test]
fn verify_names_the_seal_an_invoice_was_issued_under() {
    // A real invoice: its seal is inside the signed value rather than in the
    // container. `verify` should say whose seal it is, where it is drawn and how
    // much the proof covers, since a bare "valid" says none of that.
    let path = fixture("ofdrw/999.ofd");
    let report = report_for(&["verify", "--locale", "zh-CN", path.to_str().unwrap()]);
    assert!(report.ok, "{}", report.text);
    assert!(
        report
            .text
            .contains("监制章：全国统一发票监制章／国家税务总局／重庆市税务局"),
        "{}",
        report.text
    );
    assert!(
        report.text.contains("印章位置：90 8 30 20"),
        "{}",
        report.text
    );
    assert!(
        report.text.contains("签署工具：KingGrid"),
        "{}",
        report.text
    );
    assert!(report.text.contains("有效"), "{}", report.text);
}

#[test]
fn print_plans_the_job_over_the_whole_document() {
    let path = fixture_path();
    let report = report_for(&[
        "print",
        "--locale",
        "en-US",
        path.to_str().unwrap(),
        "--paper",
        "a4",
        "--copies",
        "2",
    ]);
    assert!(report.ok);
    // Simplex by default, so each copy lands on its own sheet.
    assert!(
        report.text.contains("2 pages on 2 sheets, A4 at 300 dpi"),
        "{}",
        report.text
    );
}

#[test]
fn a_missing_file_reports_an_error() {
    let dir = scratch("missing");
    let report = error_for(&[
        "info",
        "--locale",
        "en-US",
        dir.join("nope.ofd").to_str().unwrap(),
    ]);
    assert!(!report.ok);
    assert!(report.text.starts_with("error:"), "{}", report.text);
    assert!(report.text.contains("I/O"), "{}", report.text);
}

#[test]
fn a_container_without_the_entry_point_is_rejected() {
    // A valid archive that is not an OFD: no OFD.xml to enter through.
    let bytes =
        build_archive(&[("Doc_0/Document.xml", b"<Document/>".as_slice())]).expect("build archive");
    let path = scratch("no-entry").join("no-entry.ofd");
    std::fs::write(&path, bytes).expect("write archive");
    let report = error_for(&["info", "--locale", "en-US", path.to_str().unwrap()]);
    assert!(!report.ok);
    assert!(report.text.contains("not a valid OFD"), "{}", report.text);
}

#[test]
fn bytes_that_are_not_a_container_are_rejected() {
    let path = scratch("garbage").join("garbage.ofd");
    std::fs::write(&path, b"this is not an ofd package").expect("write garbage");
    let report = error_for(&["info", "--locale", "en-US", path.to_str().unwrap()]);
    assert!(!report.ok);
    assert!(report.text.contains("corrupted"), "{}", report.text);
}
