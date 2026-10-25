//! The archival rewrite, from GB/T 42133-2022 clause 6.2.2. Every case here
//! takes a container apart, rewrites it and checks what came back.

use sofd_archive::{archive, audit};
use sofd_package::Package;

/// A one page document that still carries every optional structure clause
/// 6.2.2 wants gone, so a case can watch them come out.
fn carrying_optional() -> Vec<(&'static str, &'static str)> {
    vec![
        (
            "OFD.xml",
            r#"<ofd:OFD xmlns:ofd="http://www.ofdspec.org/2016" Version="1.0" DocType="OFD">
  <ofd:DocBody>
    <ofd:DocInfo><ofd:DocID>urn:test:1</ofd:DocID></ofd:DocInfo>
    <ofd:DocRoot>Doc_0/Document.xml</ofd:DocRoot>
    <ofd:Signatures>Doc_0/Signs/Signatures.xml</ofd:Signatures>
  </ofd:DocBody>
</ofd:OFD>"#,
        ),
        (
            "Doc_0/Document.xml",
            r#"<ofd:Document xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:CommonData>
    <ofd:MaxUnitID>1</ofd:MaxUnitID>
    <ofd:PageArea><ofd:PhysicalBox>0 0 210 297</ofd:PhysicalBox></ofd:PageArea>
    <ofd:DocumentRes>Res/DocumentRes.xml</ofd:DocumentRes>
    <ofd:VPreferences><ofd:PageMode>None</ofd:PageMode></ofd:VPreferences>
    <ofd:Permissions><ofd:Edit>false</ofd:Edit></ofd:Permissions>
  </ofd:CommonData>
  <ofd:Pages>
    <ofd:Page ID="1" BaseLoc="Pages/Page_0/Content.xml"/>
  </ofd:Pages>
  <ofd:Attachments><ofd:Attachment ID="1" Name="note.txt" FileLoc="Attachments/note.txt"/></ofd:Attachments>
</ofd:Document>"#,
        ),
        (
            "Doc_0/Signs/Signatures.xml",
            r#"<ofd:Signatures xmlns:ofd="http://www.ofdspec.org/2016"/>"#,
        ),
        (
            "Doc_0/Res/DocumentRes.xml",
            r#"<ofd:Res xmlns:ofd="http://www.ofdspec.org/2016" BaseLoc="Res"/>"#,
        ),
        (
            "Doc_0/Pages/Page_0/Content.xml",
            r#"<ofd:Page xmlns:ofd="http://www.ofdspec.org/2016">
  <ofd:Content>
    <ofd:Layer ID="1"><ofd:PathObject ID="2" Boundary="0 0 10 10"/></ofd:Layer>
  </ofd:Content>
</ofd:Page>"#,
        ),
        ("Doc_0/Attachments/note.txt", "a note"),
    ]
}

/// Rewrites a container and returns the result bytes plus the change record.
fn rewrite(entries: &[(&str, &str)]) -> (Vec<u8>, sofd_archive::Archived) {
    let owned: Vec<(&str, &[u8])> = entries
        .iter()
        .map(|(name, body)| (*name, body.as_bytes()))
        .collect();
    archive(&owned).expect("rewrite")
}

/// Opens a container and reads one entry.
fn read_entry(bytes: &[u8], path: &str) -> String {
    let mut package = Package::from_bytes(bytes).expect("open");
    package.read_string(path).expect("entry")
}

#[test]
fn gb_t_42133_6_2_2_optional_structures_are_stripped() {
    let (bytes, change) = rewrite(&carrying_optional());
    let document = read_entry(&bytes, "Doc_0/Document.xml");
    assert!(
        !document.contains("VPreferences"),
        "view preferences must be gone, got {document}"
    );
    assert!(
        !document.contains("Permissions"),
        "permissions must be gone, got {document}"
    );
    assert!(
        !document.contains("Attachments"),
        "the attachment list must be gone, got {document}"
    );
    assert_eq!(change.stripped_elements, 3);
    assert!(
        change
            .stripped_documents
            .contains(&"Doc_0/Document.xml".to_string())
    );
}

#[test]
fn gb_t_42133_6_2_2_attachment_files_leave_with_their_list() {
    let (bytes, change) = rewrite(&carrying_optional());
    let package = Package::from_bytes(&bytes).expect("open");
    assert!(
        !package.contains("Doc_0/Attachments/note.txt"),
        "an attachment file with no list entry is unreachable and must go"
    );
    assert!(
        change
            .removed_entries
            .contains(&"Doc_0/Attachments/note.txt".to_string()),
        "the removal is reported, got {:?}",
        change.removed_entries
    );
}

#[test]
fn gb_t_42133_6_2_2_the_content_around_the_optional_elements_survives() {
    let (bytes, _change) = rewrite(&carrying_optional());
    let root = read_entry(&bytes, "OFD.xml");
    assert!(root.contains("DocID"), "the durable id must stay");
    let document = read_entry(&bytes, "Doc_0/Document.xml");
    assert!(document.contains("Content.xml"), "pages stay");
    assert!(document.contains("DocumentRes"), "resources stay");
}

#[test]
fn gb_t_42133_6_2_1_a_the_result_declares_the_archival_profile() {
    let (bytes, _change) = rewrite(&carrying_optional());
    let root = read_entry(&bytes, "OFD.xml");
    assert!(
        root.contains("DocType=\"OFD-A\""),
        "the rewrite declares the profile, got {root}"
    );
}

#[test]
fn the_rewritten_container_passes_the_archival_audit() {
    let (bytes, _change) = rewrite(&carrying_optional());
    let mut package = Package::from_bytes(&bytes).expect("open");
    let report = audit(&mut package).expect("audit");
    let errors: Vec<&str> = report
        .findings
        .iter()
        .filter(|f| f.severity == sofd_archive::Severity::Error)
        .map(|f| f.code)
        .collect();
    assert!(
        errors.is_empty(),
        "the rewrite must not leave errors, got {errors:?}"
    );
}

#[test]
fn a_clean_container_is_rewritten_only_where_the_profile_asks() {
    // A container that already carries nothing optional still gains its
    // profile declaration and keeps every other byte.
    let entries = vec![
        (
            "OFD.xml",
            r#"<ofd:OFD xmlns:ofd="http://www.ofdspec.org/2016" Version="1.0"><ofd:DocBody><ofd:DocRoot>Doc_0/Document.xml</ofd:DocRoot></ofd:DocBody></ofd:OFD>"#,
        ),
        ("Doc_0/Document.xml", "<ofd:Document/>"),
    ];
    let (bytes, change) = rewrite(&entries);
    assert_eq!(change.stripped_elements, 0, "nothing optional to strip");
    assert!(change.removed_entries.is_empty(), "no entry to remove");
    assert!(
        !change.is_noop(),
        "the profile declaration is still a change"
    );
    let root = read_entry(&bytes, "OFD.xml");
    assert!(
        root.contains("DocType=\"OFD-A\""),
        "the profile is declared"
    );
    assert_eq!(
        read_entry(&bytes, "Doc_0/Document.xml"),
        "<ofd:Document/>",
        "a non-optional document is untouched"
    );
}

/// The rewritten document for a single document container, so a case can look
/// at the bytes rather than at a parse of them.
fn rewrite_document(document: &str) -> String {
    let entries = vec![
        (
            "OFD.xml",
            r#"<ofd:OFD xmlns:ofd="http://www.ofdspec.org/2016" DocType="OFD"><ofd:DocBody><ofd:DocRoot>Doc_0/Document.xml</ofd:DocRoot></ofd:DocBody></ofd:OFD>"#,
        ),
        ("Doc_0/Document.xml", document),
    ];
    let (bytes, _change) = rewrite(&entries);
    read_entry(&bytes, "Doc_0/Document.xml")
}

#[test]
fn a_less_than_inside_an_attribute_value_is_not_a_tag_boundary() {
    // `&lt;` is a character of the value, so the element after it starts at its
    // own `<`. Reading every `<` as markup instead drops the content of
    // `Permissions` into the document as loose text.
    let document = r#"<ofd:Document xmlns:ofd="http://www.ofdspec.org/2016"><ofd:T Text="a &lt; b"/><ofd:Permissions><ofd:Edit>false</ofd:Edit></ofd:Permissions><ofd:Keep/></ofd:Document>"#;
    let out = rewrite_document(document);
    assert!(!out.contains("Permissions"), "the element is gone, got {out}");
    assert!(!out.contains("false"), "its content left with it, got {out}");
    assert!(out.contains(r#"Text="a &lt; b""#), "the value is intact, got {out}");
    assert!(out.contains("<ofd:Keep/>"), "the sibling survives, got {out}");
}

#[test]
fn a_less_than_inside_a_comment_is_not_a_tag_boundary() {
    let document = r#"<ofd:Document xmlns:ofd="http://www.ofdspec.org/2016"><!-- use <ofd:Permissions> to lock --><ofd:Keep/></ofd:Document>"#;
    let out = rewrite_document(document);
    assert!(
        out.contains("<!-- use <ofd:Permissions> to lock -->"),
        "a comment is text, not structure, got {out}"
    );
    assert!(out.contains("<ofd:Keep/>"), "the sibling survives, got {out}");
}

#[test]
fn markup_inside_cdata_is_text_and_survives() {
    let document = r#"<ofd:Document xmlns:ofd="http://www.ofdspec.org/2016"><ofd:T><![CDATA[<ofd:Permissions/>]]></ofd:T></ofd:Document>"#;
    let out = rewrite_document(document);
    assert_eq!(out, document, "nothing optional is at the top level here");
}

#[test]
fn the_bytes_outside_a_removed_element_are_untouched() {
    let document = "<ofd:Document xmlns:ofd=\"http://www.ofdspec.org/2016\">\n  <ofd:VPreferences><ofd:PageMode>None</ofd:PageMode></ofd:VPreferences>\n  <ofd:Pages>\n    <ofd:Page ID=\"1\"/>\n  </ofd:Pages>\n</ofd:Document>";
    let out = rewrite_document(document);
    let expected = "<ofd:Document xmlns:ofd=\"http://www.ofdspec.org/2016\">\n  \n  <ofd:Pages>\n    <ofd:Page ID=\"1\"/>\n  </ofd:Pages>\n</ofd:Document>";
    assert_eq!(out, expected, "the rewrite removes one span and changes nothing else");
}
