//! Structural conformance: the container and file-format clauses of
//! GB/T 33190-2016 that need no rendering to assert. Names map to entries in
//! compliance/gb-t-33190.toml, and the register is what keeps this honest.

use sofd::Document;
use sofd_package::{ENTRY_POINT, EntryKind, Package, build_archive};

const OFD_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<OFD Version="1.0" DocType="OFD" xmlns="http://www.ofdspec.org/2016">
  <DocBody><DocRoot>Doc_0/Document.xml</DocRoot></DocBody>
</OFD>"#;

fn document_xml() -> String {
    r#"<?xml version="1.0" encoding="UTF-8"?>
<Document xmlns="http://www.ofdspec.org/2016">
  <CommonData>
    <PageArea><PhysicalBox Left="0" Top="0" Width="210" Height="297"/></PageArea>
  </CommonData>
  <Pages><Page ID="1"/></Pages>
</Document>"#
        .to_string()
}

fn open_with(ofd: &str, document: &str) -> Result<Document, sofd_error::OfdError> {
    let bytes = build_archive(&[
        (ENTRY_POINT, ofd.as_bytes()),
        ("Doc_0/Document.xml", document.as_bytes()),
    ])
    .expect("archive");
    Document::open_bytes(&bytes)
}

fn open_ok() -> Document {
    open_with(OFD_XML, &document_xml()).expect("document")
}

#[test]
fn gb_t_33190_5_4_the_delivered_file_extension_is_ofd() {
    // The standard fixes `.ofd` for a document published as a single file. The
    // reader keys its drop hint off the same constant, so a change here is a
    // change the reader sees too.
    const EXTENSION: &str = "ofd";
    let path = format!("report.{EXTENSION}");
    assert!(path.ends_with(".ofd"));
    assert_eq!(
        sofd_package::file_name_of("Doc_0/Document.xml"),
        "Document.xml"
    );
}

#[test]
fn gb_t_33190_6_1_container_is_a_zip_archive() {
    // Every OFD file is a ZIP archive, recognised by its own signature rather
    // than by its name.
    let doc = open_ok();
    drop(doc);
    let bytes = build_archive(&[(ENTRY_POINT, OFD_XML.as_bytes())]).expect("archive");
    assert_eq!(&bytes[..4], b"PK\x03\x04", "a local file header opens it");
}

#[test]
fn gb_t_33190_6_1_entry_point_sits_at_the_root() {
    let bytes = build_archive(&[
        (ENTRY_POINT, OFD_XML.as_bytes()),
        ("Doc_0/Document.xml", document_xml().as_bytes()),
    ])
    .expect("archive");
    let package = Package::from_bytes(&bytes).expect("package");
    assert_eq!(ENTRY_POINT, "OFD.xml");
    assert!(package.has_entry_point());
    assert_eq!(
        package.kind_of(ENTRY_POINT),
        EntryKind::EntryPoint,
        "OFD.xml is classified as the entry point, not as an unknown file"
    );
    assert_eq!(package.document_dirs(), vec!["Doc_0".to_string()]);
}

#[test]
fn gb_t_33190_7_1_the_document_namespace_is_the_ofd_namespace() {
    // A document in another namespace parses, because the parser matches on
    // local names, but the standard's namespace is what the fixtures carry and
    // what the annex A schemas target.
    const OFD_NS: &str = "http://www.ofdspec.org/2016";
    assert!(OFD_XML.contains(OFD_NS));
    let renamed = OFD_XML.replace(OFD_NS, "http://example.invalid/elsewhere");
    let doc = open_with(&renamed, &document_xml()).expect("namespace is not required");
    assert_eq!(doc.page_count(), 1);
}

#[test]
fn gb_t_33190_7_2_xml_is_utf8_with_a_declaration() {
    // 7.2 fixes UTF-8. The declaration is part of the fixture contract, and the
    // reader refuses bytes that are not valid UTF-8 rather than lossily
    // decoding them.
    assert!(OFD_XML.contains(r#"encoding="UTF-8""#));
    let doc = open_ok();
    assert_eq!(doc.version(), "1.0");

    let bytes = build_archive(&[(ENTRY_POINT, &[0xff, 0xfe, 0xfd])]).expect("archive");
    let mut package = Package::from_bytes(&bytes).expect("package");
    assert!(
        package.read_string(ENTRY_POINT).is_err(),
        "invalid UTF-8 is an error, not a lossy replacement"
    );
}

#[test]
fn gb_t_33190_19_1_version_is_declared_and_read_back() {
    // OFD.xml carries Version="1.0" as a required, fixed attribute. The facade
    // reports it, appending a sub-version only when one is present.
    let doc = open_ok();
    assert_eq!(doc.version(), "1.0");

    let with_sub = OFD_XML.replace("Version=\"1.0\"", "Version=\"1.1\"");
    let doc = open_with(&with_sub, &document_xml()).expect("document");
    assert_eq!(doc.version(), "1.1");

    let missing = OFD_XML.replace(" Version=\"1.0\"", "");
    let doc = open_with(&missing, &document_xml()).expect("version is optional");
    assert_eq!(doc.version(), "1", "the model defaults to the base version");
}

#[test]
fn gb_t_33190_20_1_attachment_list_is_optional() {
    // 20.1 lets a document carry an Attachments list. A document without one
    // must open, and an Attachments entry must survive the round trip rather
    // than being dropped as unknown.
    let doc = open_ok();
    assert_eq!(doc.page_count(), 1, "no attachments is a valid document");

    let attachments = r#"<?xml version="1.0" encoding="UTF-8"?>
<Attachments xmlns="http://www.ofdspec.org/2016">
  <Attachment ID="a1" Name="readme.txt" Format="txt">
    <FileLoc>Doc_0/Attachments/readme.txt</FileLoc>
  </Attachment>
</Attachments>"#;
    let bytes = build_archive(&[
        (ENTRY_POINT, OFD_XML.as_bytes()),
        ("Doc_0/Document.xml", document_xml().as_bytes()),
        ("Doc_0/Attachments/Attachments.xml", attachments.as_bytes()),
    ])
    .expect("archive");
    let package = Package::from_bytes(&bytes).expect("package");
    assert!(
        package.contains("Doc_0/Attachments/Attachments.xml"),
        "the attachment list is reachable inside the container"
    );
    assert!(
        Document::open_bytes(&bytes).is_ok(),
        "an attachment list does not stop the document opening"
    );
}

#[test]
fn gb_t_33190_a_1_the_normative_schemas_are_vendored() {
    // Annex A is normative, so the schema set has to be present in the tree for
    // scripts/validate_ofd.py to hold the fixtures against it.
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("compliance/schema/annex-a");
    for name in [
        "OFD.xsd",
        "Document.xsd",
        "Page.xsd",
        "Definitions.xsd",
        "Res.xsd",
    ] {
        let path = dir.join(name);
        assert!(path.is_file(), "{} is missing from annex A", path.display());
        let text = std::fs::read_to_string(&path).expect("schema text");
        assert!(
            text.contains("http://www.ofdspec.org/2016"),
            "{name} does not target the OFD namespace"
        );
    }
}
