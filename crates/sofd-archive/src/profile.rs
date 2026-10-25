//! Producing an OFD-A container. GB/T 42133-2022 asks an authoring tool for
//! things a read-only checker cannot decide: strip the optional structures
//! clause 6.2.2 names. This module does exactly that, and nothing else.

use std::ops::Range;

use sofd_error::Result;
use sofd_package::{build_archive, resolve};
use sofd_xml::Cursor;
use sofd_xml::cursor::Event;

use crate::audit::OFD_A_DOC_TYPE;

/// Optional structures clause 6.2.2 wants out of an archived document. Each is
/// an element the 2016 schema allows but the archival profile drops.
const OPTIONAL_ELEMENTS: &[&str] = &[
    "Permissions",
    "VPreferences",
    "Extensions",
    "Keywords",
    "Attachments",
];

/// The container entries clause 6.2.2 wants gone: attachments and their list.
const OPTIONAL_ENTRIES: &[&str] = &["Attachments", "Attachment"];

/// The attribute `OFD.xml` declares its profile in, clause 6.2.1 a).
const DOC_TYPE: &str = "DocType";

/// What one call to [`archive`] changed, so a caller can log or assert on it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Archived {
    /// Container entries removed, in path order.
    pub removed_entries: Vec<String>,
    /// XML documents whose text had optional elements stripped.
    pub stripped_documents: Vec<String>,
    /// How many optional elements were removed across those documents.
    pub stripped_elements: usize,
    /// Whether the container's `DocType` had to be rewritten to `OFD-A`.
    pub declared_profile: bool,
}

impl Archived {
    /// Whether the container came back byte-identical.
    pub fn is_noop(&self) -> bool {
        self.removed_entries.is_empty() && self.stripped_elements == 0 && !self.declared_profile
    }
}

/// Rewrite `entries` into an OFD-A container and return its bytes. The caller
/// supplies the source container as `(path, bytes)` pairs; the result is a
/// fresh archive an archiver can write out.
pub fn archive(entries: &[(&str, &[u8])]) -> Result<(Vec<u8>, Archived)> {
    let mut change = Archived::default();
    let mut kept: Vec<(String, Vec<u8>)> = Vec::new();
    for (path, data) in entries {
        if is_optional_entry(path) {
            change.removed_entries.push((*path).to_string());
            continue;
        }
        if is_xml(path) {
            let (rewritten, removed) = strip_optional(data)?;
            if removed > 0 {
                change.stripped_documents.push((*path).to_string());
                change.stripped_elements += removed;
                kept.push(((*path).to_string(), rewritten));
                continue;
            }
        }
        kept.push(((*path).to_string(), data.to_vec()));
    }
    // Clause 6.2.1 a): the profile has to be declared, or the result is a
    // plain OFD by the standard's own test.
    if let Some(entry) = kept.iter_mut().find(|(path, _)| path == "OFD.xml") {
        let rewritten = declare_profile(&entry.1);
        change.declared_profile = rewritten != entry.1;
        entry.1 = rewritten;
    }
    let borrowed: Vec<(&str, &[u8])> = kept
        .iter()
        .map(|(path, data)| (path.as_str(), data.as_slice()))
        .collect();
    Ok((build_archive(&borrowed)?, change))
}

/// Whether a container entry is one clause 6.2.2 strips.
fn is_optional_entry(path: &str) -> bool {
    let name = path.rsplit('/').next().unwrap_or(path);
    let stem = name.split('.').next().unwrap_or(name);
    OPTIONAL_ENTRIES.contains(&stem) || path.contains("/Attachments/")
}

/// Whether a path names an XML document to rewrite.
fn is_xml(path: &str) -> bool {
    path.to_ascii_lowercase().ends_with(".xml")
}

/// Remove the optional elements from one XML document, keeping everything else
/// byte-for-byte. Returns the rewritten document and the count of removals.
///
/// The tag boundaries come from the parser rather than from a search for `<`:
/// a `<` inside a comment, a CDATA section or an attribute value is not a tag,
/// and counting those as one cuts an element in half.
fn strip_optional(data: &[u8]) -> Result<(Vec<u8>, usize)> {
    let Ok(text) = std::str::from_utf8(data) else {
        return Ok((data.to_vec(), 0));
    };
    let spans = spans_of_dropped(text);
    if spans.is_empty() {
        return Ok((data.to_vec(), 0));
    }
    let mut out = String::with_capacity(text.len());
    let mut from = 0;
    for span in &spans {
        let Some(head) = text.get(from..span.start) else {
            return Ok((data.to_vec(), 0));
        };
        out.push_str(head);
        from = span.end;
    }
    out.push_str(&text[from..]);
    Ok((out.into_bytes(), spans.len()))
}

/// The byte spans of the optional elements in `text`, in document order. A
/// document the parser refuses yields nothing: the caller keeps it whole rather
/// than half stripped, since a rewrite that cannot be finished is worse than an
/// element the profile wanted gone.
fn spans_of_dropped(text: &str) -> Vec<Range<usize>> {
    let mut spans: Vec<Range<usize>> = Vec::new();
    let mut cursor = Cursor::new(text);
    loop {
        match cursor.next_event() {
            Ok(Event::Start(node)) => {
                let start = tag_start(text, cursor.tag_start(), cursor.offset());
                if !node.is_any(OPTIONAL_ELEMENTS) {
                    continue;
                }
                // A self-closing element is complete at its own `>`; one with
                // content runs to the `>` of its end tag.
                if !node.is_self_closing() && cursor.skip_element(&node).is_err() {
                    spans.clear();
                    return spans;
                }
                spans.push(start..cursor.offset());
            }
            Ok(Event::End(_)) | Ok(Event::Text(_)) => {}
            Ok(Event::Eof) => return spans,
            Err(_) => return Vec::new(),
        }
    }
}

/// The `<` of the start tag between `from` and `end`. The parser reports where
/// the previous event ended, blanks included, so the search starts at the part
/// that is the tag rather than at the whitespace in front of it.
fn tag_start(text: &str, from: usize, end: usize) -> usize {
    let Some(span) = text.get(from..end) else {
        return from;
    };
    match span.find('<') {
        Some(at) => from + at,
        None => from,
    }
}

/// Rewrite the `DocType` of `OFD.xml` to the archival profile, which is the
/// attribute the container already uses to name its type.
fn declare_profile(data: &[u8]) -> Vec<u8> {
    let Ok(text) = std::str::from_utf8(data) else {
        return data.to_vec();
    };
    let Some((at, cut)) = root_tag_span(text) else {
        return data.to_vec();
    };
    let rewritten = match doc_type_value(text, at, cut) {
        Some((open, close)) => {
            let mut out = String::with_capacity(text.len() + 24);
            out.push_str(&text[..open]);
            out.push_str(OFD_A_DOC_TYPE);
            out.push_str(&text[close..]);
            out
        }
        None => {
            let mut out = String::with_capacity(text.len() + 24);
            out.push_str(&text[..cut - 1]);
            out.push_str(&format!(" {DOC_TYPE}=\"{OFD_A_DOC_TYPE}\""));
            out.push_str(&text[cut - 1..]);
            out
        }
    };
    rewritten.into_bytes()
}

/// The byte span of the root start tag, `<` through `>` inclusive, or `None`
/// when the document has no root this can find.
fn root_tag_span(text: &str) -> Option<(usize, usize)> {
    let at = find_root_tag(text)?;
    let gt = text[at..].find('>')?;
    Some((at, at + gt + 1))
}

/// The byte span of the value `DocType` carries inside the tag at `at..cut`,
/// quotes excluded, or `None` when the tag does not carry the attribute. The
/// name is read off the tag's own attribute list, so a value that happens to
/// spell `DocType` is not mistaken for the attribute.
fn doc_type_value(text: &str, at: usize, cut: usize) -> Option<(usize, usize)> {
    let open = at + 1 + text[at + 1..cut].find(DOC_TYPE)?;
    let after = open + DOC_TYPE.len();
    let tail = &text[after..cut];
    let lead = tail.len() - tail.trim_start().len();
    let eq = after + lead + tail[lead..].find('=')?;
    let quoted = &text[eq + 1..cut];
    let quote_at = eq + 1 + quoted.find('"')?;
    let end = text[quote_at + 1..cut].find('"')?;
    Some((quote_at + 1, quote_at + 1 + end))
}

/// The byte offset of the `OFD` root tag, whatever namespace prefix it wears.
/// Only the first element counts: a comment or a declaration above it is not
/// the root, and the parser tells the two apart where a search for `<OFD` cannot.
fn find_root_tag(text: &str) -> Option<usize> {
    let mut cursor = Cursor::new(text);
    loop {
        match cursor.next_event().ok()? {
            Event::Start(node) if node.is("OFD") => {
                return Some(tag_start(text, cursor.tag_start(), cursor.offset()));
            }
            Event::Start(_) | Event::End(_) | Event::Text(_) => {}
            Event::Eof => return None,
        }
    }
}

/// Resolve an attachment reference to its container path, so a caller can name
/// the entry clause 6.2.2 removes for a given document.
pub fn attachment_path(base: &str, reference: &str) -> String {
    resolve(base, reference)
}
