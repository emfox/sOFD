//! Archival readiness checks derived from GB/T 33190-2016 and GB/T 42133-2022.
//! Nothing here claims OFD/A conformance: this reports only what the standards
//! themselves require of a document kept for the long term.

use sofd_error::Result;
use sofd_package::{ENTRY_POINT, EntryKind, Package, parent_of, resolve};
use sofd_xml::Cursor;
use sofd_xml::cursor::Event;

use crate::report::{Finding, Report, Severity};

/// The document type an archival document declares, clause 6.2.1 a).
pub const OFD_A_DOC_TYPE: &str = "OFD-A";

/// The only raster formats clause 6.2.6 e) lets an archived page hold. Anything
/// else has to be converted before archiving, so seeing another one is worth a
/// warning rather than a refusal: the bytes may still be readable.
const ARCHIVAL_IMAGE_EXTENSIONS: &[&str] = &[
    "bmp", "jpg", "jpeg", "png", "jbig2", "jb2", "jp2", "jpx", "jpf", "tif", "tiff",
];

/// Where a single reference to another entry was found.
struct Reference {
    element: String,
    value: String,
}

/// Elements whose content or `BaseLoc` points at another container entry.
const REFERENCING: &[&str] = &[
    "DocRoot",
    "Signatures",
    "PublicRes",
    "DocumentRes",
    "PageRes",
    "FileLoc",
    "Page",
];

/// Resources looked for inside a page. `Page` is left out on purpose: it is
/// the root element there, and reading its text would swallow the resources.
const PAGE_RESOURCES: &[&str] = &["PageRes", "FileLoc"];

/// Resource tables whose entries name the files they hold. Clause 7.9 is
/// about reachability, so a table that points at a file the container does not
/// carry has to be followed as far as the file name, not just to the table.
const RESOURCE_TABLES: &[&str] = &["PublicRes", "DocumentRes", "PageRes", "FileRes"];

/// Elements inside a resource table whose text is a file the table claims.
const RESOURCE_FILES: &[&str] = &["MediaFile", "FontFile", "FileLoc", "BaseLoc"];

/// Audit a container for archival readiness.
pub struct Auditor;

impl Auditor {
    /// Prepare an auditor with the default checks.
    pub fn new() -> Self {
        Self
    }

    /// Run every check over one container.
    pub fn audit(&self, package: &mut Package) -> Result<Report> {
        let mut report = Report::new();
        if !package.has_entry_point() {
            report.push(Finding::new(
                "ARCH-001",
                Severity::Error,
                "7.4",
                ENTRY_POINT,
                "the container has no main entry, so it is not an OFD document",
            ));
            return Ok(report);
        }

        let root = read(package, ENTRY_POINT)?;
        self.check_archival_profile(package, &root, &mut report)?;
        let references = references(&root, REFERENCING)?;
        // Clause 7.4: DocInfo lives under OFD.xml, not under Document.xml.
        if text_of(&root, "DocID").is_none_or(|id| id.is_empty()) {
            report.push(Finding::new(
                "ARCH-005",
                Severity::Warning,
                "7.5",
                ENTRY_POINT,
                "no DocID, so the archived document carries no durable identifier",
            ));
        }
        if !references.iter().any(|r| r.element == "Signatures") {
            report.push(Finding::new(
                "ARCH-006",
                Severity::Warning,
                "18.1",
                ENTRY_POINT,
                "no signature list, so the archive cannot be integrity checked",
            ));
        }

        for reference in &references {
            self.check(package, "", ENTRY_POINT, reference, &mut report);
        }

        for document in references.iter().filter(|r| r.element == "DocRoot") {
            self.audit_document(package, &document.value, &mut report)?;
        }
        Ok(report)
    }

    /// The checks that come from GB/T 42133-2022 rather than GB/T 33190:
    /// clause 6.2.1 a) wants the profile declared, 6.16 forbids encryption,
    /// and 6.2.6 e) narrows the raster formats a page may hold.
    fn check_archival_profile(
        &self,
        package: &Package,
        root: &str,
        report: &mut Report,
    ) -> Result<()> {
        match doc_type(root) {
            Some(value) if value == OFD_A_DOC_TYPE => {}
            Some(value) => report.push(Finding::new(
                "ARCH-101",
                Severity::Warning,
                "6.2.1 a)",
                ENTRY_POINT,
                format!("DocType is {value:?}, but an archival document declares \"OFD-A\""),
            )),
            None => report.push(Finding::new(
                "ARCH-101",
                Severity::Warning,
                "6.2.1 a)",
                ENTRY_POINT,
                "no DocType, so the document does not declare the archival profile",
            )),
        }
        // Clause 6.16: a document kept for the long term is stored in the
        // clear, so an encrypted container cannot be archived as it stands.
        if package.is_encrypted() {
            report.push(Finding::new(
                "ARCH-102",
                Severity::Error,
                "6.16",
                ENTRY_POINT,
                "the container carries ZIP encryption, but an archived document is plain",
            ));
        }
        // Clause 6.2.6 e): any raster image is one of six formats. The
        // extension is the only hint available without decoding the bytes.
        for path in package.entries().iter() {
            if package.kind_of(path) != EntryKind::Image {
                continue;
            }
            if is_archival_image(path) {
                continue;
            }
            report.push(Finding::new(
                "ARCH-104",
                Severity::Warning,
                "6.2.6 e)",
                path.clone(),
                "the image is not one of the six formats an archived page may hold",
            ));
        }
        Ok(())
    }

    /// Follow the entries a document points at. A reference to a resource
    /// table is one hop; the files that table names are the hop after it, and
    /// clause 7.9 is not satisfied until they resolve too.
    fn audit_resource_tables(
        &self,
        package: &mut Package,
        found_in: &str,
        report: &mut Report,
    ) -> Result<()> {
        let xml = read(package, found_in)?;
        // A table is namespaced, so the local name has to be matched rather
        // than the prefixed spelling the file happens to use.
        for reference in references(&xml, RESOURCE_TABLES)? {
            let table_path = resolve(&base_of(found_in, &xml), &reference.value);
            if !package.contains(&table_path) {
                continue;
            }
            let table = read(package, &table_path)?;
            let table_base = base_of(&table_path, &table);
            for file in references(&table, RESOURCE_FILES)? {
                self.check(package, &table_base, &table_path, &file, report);
            }
        }
        Ok(())
    }

    /// Walk one document: its pages and their resources.
    fn audit_document(&self, package: &mut Package, path: &str, report: &mut Report) -> Result<()> {
        if !package.contains(path) {
            return Ok(());
        }
        // The document-level tables name the files they hold, so follow them
        // before descending into pages.
        self.audit_resource_tables(package, path, report)?;
        let xml = read(package, path)?;
        let base = base_of(path, &xml);
        for reference in references(&xml, REFERENCING)? {
            if reference.element == "DocRoot" {
                continue;
            }
            self.check(package, &base, path, &reference, report);
            if reference.element != "Page" {
                continue;
            }
            let page_path = resolve(&base, &reference.value);
            if !package.contains(&page_path) {
                continue;
            }
            let res_base = parent_of(&page_path);
            // Clause 7.9 wants every file the page's own resource tables name,
            // and each table keeps a `BaseLoc` of its own that the names inside
            // it are relative to.
            let page_xml = read(package, &page_path)?;
            self.audit_resource_tables(package, &page_path, report)?;
            for nested in references(&page_xml, PAGE_RESOURCES)? {
                self.check(package, &res_base, &page_path, &nested, report);
            }
        }
        Ok(())
    }

    /// Classify one reference, recording any finding it raises.
    fn check(
        &self,
        package: &Package,
        base: &str,
        found_in: &str,
        reference: &Reference,
        report: &mut Report,
    ) {
        let value = reference.value.trim();
        if value.is_empty() {
            return;
        }
        if is_external(value) {
            report.push(Finding::new(
                "ARCH-007",
                Severity::Error,
                "6.2",
                found_in,
                format!(
                    "{} points outside the container at {value:?}, so the archive is not self-contained",
                    reference.element
                ),
            ));
            // Clause 6.9 a) forbids actions that reach an external resource,
            // and 6.11 says the same of indexing content, so the same
            // reference is reported under the archival clause as well.
            report.push(Finding::new(
                "ARCH-103",
                Severity::Error,
                "6.2, 6.9 a), 6.11",
                found_in,
                format!(
                    "{} reaches {value:?} outside the container, which an archived document may not do",
                    reference.element
                ),
            ));
            return;
        }
        if escapes(value) {
            report.push(Finding::new(
                "ARCH-008",
                Severity::Error,
                "6.2",
                found_in,
                format!("{} escapes the container with {value:?}", reference.element),
            ));
            return;
        }
        let resolved = resolve(base, value);
        if package.contains(&resolved) {
            return;
        }
        let (code, clause) = match reference.element.as_str() {
            "DocRoot" => ("ARCH-002", "7.4"),
            "Page" => ("ARCH-003", "7.6"),
            _ => ("ARCH-004", "7.9"),
        };
        report.push(Finding::new(
            code,
            Severity::Error,
            clause,
            found_in,
            format!(
                "{} refers to {value:?}, which the container does not hold; it wanted {resolved:?}",
                reference.element
            ),
        ));
    }
}

impl Default for Auditor {
    fn default() -> Self {
        Self::new()
    }
}

/// Audit a container with the default checks.
pub fn audit(package: &mut Package) -> Result<Report> {
    Auditor::new().audit(package)
}

/// A reference is external when it names a scheme.
fn is_external(value: &str) -> bool {
    value.contains("://")
}

/// A reference escapes when normalisation leaves a parent step in it, or when
/// it is rooted. OFD writers commonly write container paths as `/Doc_0/x.xml`,
/// which the reader resolves against the archive root, so a leading slash is a
/// spelling rather than an escape; one that walks above that root is not.
fn escapes(value: &str) -> bool {
    let mut depth = 0i32;
    for part in value.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                depth -= 1;
                if depth < 0 {
                    return true;
                }
            }
            _ => depth += 1,
        }
    }
    false
}

/// Read an entry as text, treating a missing one as empty.
fn read(package: &mut Package, path: &str) -> Result<String> {
    package.read_string(path)
}

/// Collect the references the wanted elements make.
fn references(xml: &str, wanted: &[&str]) -> Result<Vec<Reference>> {
    let mut found = Vec::new();
    let mut cursor = Cursor::new(xml);
    loop {
        match cursor.next_event()? {
            Event::Start(node) if wanted.contains(&node.local()) => {
                let element = node.local().to_string();
                if let Some(value) = node.attributes().get("BaseLoc") {
                    found.push(Reference {
                        element,
                        value: value.to_string(),
                    });
                } else if !node.is_self_closing() {
                    let value = cursor.read_text()?;
                    found.push(Reference {
                        element,
                        value: value.trim().to_string(),
                    });
                }
            }
            Event::Eof => break,
            _ => {}
        }
    }
    Ok(found)
}

/// The directory a file's references are relative to.
/// Clause 7.8 makes `BaseLoc` optional; an absent one means the file's own
/// directory. A declared one is written from the document directory, not from
/// the file, so `Doc_0/Res/DocumentRes.xml` with `BaseLoc="Res"` names
/// `Doc_0/Res` rather than `Doc_0/Res/Res`.
fn base_of(path: &str, xml: &str) -> String {
    let here = parent_of(path);
    match base_loc_of(xml) {
        Some(loc) if !loc.trim().is_empty() => resolve(&document_dir(path), &loc),
        _ => here,
    }
}

/// The `Doc_N` directory a path sits under, or the root when it sits above
/// every document.
fn document_dir(path: &str) -> String {
    match path.split_once('/') {
        Some((head, _)) if head.starts_with("Doc_") => head.to_string(),
        _ => String::new(),
    }
}

/// `BaseLoc` of the root element, or `None` when it declares none.
fn base_loc_of(xml: &str) -> Option<String> {
    let mut cursor = Cursor::new(xml);
    match cursor.next_event().ok()? {
        Event::Start(node) => node.attributes().get("BaseLoc").map(str::to_string),
        _ => None,
    }
}

/// Text of the first occurrence of an element, trimmed.
fn text_of(xml: &str, name: &str) -> Option<String> {
    let mut cursor = Cursor::new(xml);
    loop {
        match cursor.next_event().ok()? {
            Event::Start(node) if node.is(name) => {
                if node.is_self_closing() {
                    return Some(String::new());
                }
                return cursor.read_text().ok().map(|t| t.trim().to_string());
            }
            Event::Eof => return None,
            _ => {}
        }
    }
}

/// `DocType` of the root element, or `None` when it is absent.
fn doc_type(xml: &str) -> Option<String> {
    let mut cursor = Cursor::new(xml);
    loop {
        match cursor.next_event().ok()? {
            Event::Start(node) if node.local() == "OFD" => {
                return node.attributes().get("DocType").map(|v| v.to_string());
            }
            Event::Eof => return None,
            _ => {}
        }
    }
}

/// Whether an image entry carries one of the formats clause 6.2.6 e) allows.
fn is_archival_image(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    let extension = lower.rsplit('.').next().unwrap_or("");
    ARCHIVAL_IMAGE_EXTENSIONS.contains(&extension)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn external_references_are_detected() {
        assert!(is_external("http://example.com/a.xml"));
        assert!(is_external("file:///etc/passwd"));
        assert!(!is_external("Doc_0/Document.xml"));
    }

    #[test]
    fn a_rooted_container_path_is_not_an_escape() {
        // OFD writers spell container paths from the root, and the reader
        // resolves them there, so the slash names an entry rather than leaving.
        assert!(!escapes("/Doc_0/Signs/Signatures.xml"));
        assert_eq!(
            resolve("", "/Doc_0/Signs/Signatures.xml"),
            "Doc_0/Signs/Signatures.xml"
        );
        assert!(escapes("/../outside.xml"));
        assert!(escapes("/Doc_0/../../out.xml"));
    }

    #[test]
    fn parent_steps_are_detected() {
        assert!(escapes("../outside.xml"));
        assert!(escapes("Doc_0/../../out.xml"));
        assert!(!escapes("Doc_0/Pages/Page_0.xml"));
    }

    #[test]
    fn references_are_read_from_both_attributes_and_text() {
        let xml =
            r#"<Doc><Page BaseLoc="Pages/Page_0.xml"/><DocRoot>Doc_0/Document.xml</DocRoot></Doc>"#;
        let found = references(xml, REFERENCING).expect("scan");
        let values: Vec<&str> = found.iter().map(|r| r.value.as_str()).collect();
        assert!(values.contains(&"Pages/Page_0.xml"));
        assert!(values.contains(&"Doc_0/Document.xml"));
    }

    #[test]
    fn element_text_is_found_by_name() {
        let xml = r#"<DocInfo><DocID>urn:x</DocID></DocInfo>"#;
        assert_eq!(text_of(xml, "DocID").as_deref(), Some("urn:x"));
        assert_eq!(text_of(xml, "Title"), None);
    }
}
