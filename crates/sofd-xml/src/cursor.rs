//! Streaming element cursor.
//! [`Cursor`] turns the flat event stream into an element-oriented view, so
//! callers write document logic instead of event matching.

use sofd_error::{OfdError, Result};

use crate::attribute::Attributes;
use crate::{local_name, map_error};

/// One node in the document, with attributes already decoded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    /// Element local name, prefix removed.
    local: String,
    /// Qualified name as written, kept for diagnostics.
    qualified: String,
    /// Decoded attributes.
    attributes: Attributes,
    /// Whether the tag closed itself (`<Foo/>`), so no `End` will follow.
    self_closing: bool,
}

impl Node {
    /// Local name with any namespace prefix removed.
    pub fn local(&self) -> &str {
        &self.local
    }

    /// Qualified name as it appeared in the source.
    pub fn qualified(&self) -> &str {
        &self.qualified
    }

    /// Attributes of this element.
    pub fn attributes(&self) -> &Attributes {
        &self.attributes
    }

    /// Whether the local name matches.
    pub fn is(&self, name: &str) -> bool {
        self.local == name
    }

    /// Whether the local name is any of the candidates.
    pub fn is_any(&self, names: &[&str]) -> bool {
        names.contains(&self.local.as_str())
    }

    /// Whether this element closed itself and has no matching end tag.
    pub fn is_self_closing(&self) -> bool {
        self.self_closing
    }
}

/// A parsed event.
#[derive(Debug, PartialEq, Eq)]
pub enum Event {
    /// An opening tag, or a self-closing tag.
    Start(Node),
    /// A closing tag for the named element.
    End(String),
    /// Character data, already unescaped. One entity is one event, so a run
    /// written around `&lt;` arrives in pieces; `read_text` joins them and
    /// trims the ends.
    Text(String),
    /// The document ended.
    Eof,
}

/// Default nesting ceiling; OFD templates never nest this deeply, so exceeding
/// it means a malformed document or an attempt to exhaust the stack.
pub const DEFAULT_MAX_DEPTH: usize = 64;

/// Byte offset to a one-based line and column.
/// The reader reports offsets, not positions, so this is an approximation that
/// keeps errors useful without retaining the whole document.
fn position_of(offset: u64) -> (u64, u64) {
    (offset.saturating_div(40) + 1, 1)
}

/// Build a node from a start tag, decoding its attributes.
fn build_node(start: &quick_xml::events::BytesStart<'_>, self_closing: bool) -> Result<Node> {
    let qualified = start.name().as_ref().to_string();
    let local = local_name(&qualified).to_string();
    let attributes = read_attributes(start)?;
    Ok(Node {
        local,
        qualified,
        attributes,
        self_closing,
    })
}

/// Decode every attribute of a start tag.
fn read_attributes(start: &quick_xml::events::BytesStart<'_>) -> Result<Attributes> {
    let mut out = Attributes::default();
    for attr in start.attributes() {
        let attr = attr.map_err(|e| {
            OfdError::Xml(sofd_error::XmlError::Syntax {
                line: 0,
                col: 0,
                msg: format!("malformed attribute: {e}"),
            })
        })?;
        let key = local_name(attr.key.as_ref()).to_string();
        // `value` is the raw text between the quotes; entities must be resolved
        // explicitly or `&amp;` would reach callers still escaped.
        let value = attr
            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .map_err(|e| {
                OfdError::Xml(sofd_error::XmlError::Syntax {
                    line: 0,
                    col: 0,
                    msg: format!("undecodable value for attribute {key}: {e}"),
                })
            })?
            .to_string();
        out.insert(key, value);
    }
    Ok(out)
}

/// A pull cursor over an XML document.
#[derive(Clone)]
pub struct Cursor<'a> {
    reader: quick_xml::Reader<&'a [u8]>,
    buf: Vec<u8>,
    depth: usize,
    max_depth: usize,
    /// Where the tag the last event read began.
    tag_start: usize,
    /// Elements opened but not yet closed, so a truncated document is caught.
    open: Vec<String>,
}

impl std::fmt::Debug for Cursor<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Cursor")
            .field("depth", &self.depth)
            .field("max_depth", &self.max_depth)
            .finish()
    }
}

impl<'a> Cursor<'a> {
    /// Start reading a document.
    pub fn new(xml: &'a str) -> Self {
        Self::with_max_depth(xml, DEFAULT_MAX_DEPTH)
    }

    /// Start reading with an explicit nesting ceiling.
    pub fn with_max_depth(xml: &'a str, max_depth: usize) -> Self {
        let mut reader = quick_xml::Reader::from_str(xml);
        let config = reader.config_mut();
        // Blanks are left where the producer put them: a `TextCode` may open
        // with one and the offsets below count characters, so trimming here
        // would shift every glyph. `read_text` trims the ends of the value.
        config.trim_text(false);
        config.expand_empty_elements = false;
        Self {
            reader,
            buf: Vec::new(),
            depth: 0,
            max_depth,
            tag_start: 0,
            open: Vec::new(),
        }
    }

    /// Current nesting depth.
    pub fn depth(&self) -> usize {
        self.depth
    }

    /// Byte offset just past the event [`next_event`](Self::next_event) last
    /// returned. Zero before the first event.
    pub fn offset(&self) -> usize {
        self.reader.buffer_position() as usize
    }

    /// Byte offset where the tag the last [`next_event`] read began, preceding
    /// whitespace included. A caller rewriting a document in place needs both
    /// ends of what it cuts, and a search for `<` finds the wrong one across an
    /// end tag in between, so this is taken before the tag is parsed.
    ///
    /// [`next_event`]: Self::next_event
    pub fn tag_start(&self) -> usize {
        self.tag_start
    }

    /// Read the next event.
    pub fn next_event(&mut self) -> Result<Event> {
        let before = self.reader.buffer_position() as usize;
        let event = self.next_inner();
        if event.is_ok() {
            self.tag_start = before;
        }
        event
    }

    /// The event loop proper, split out so [`next_event`](Self::next_event) can
    /// bracket it with the offsets the tag occupies.
    fn next_inner(&mut self) -> Result<Event> {
        loop {
            let pos = self.reader.buffer_position();
            let event = match self.reader.read_event_into(&mut self.buf) {
                Ok(e) => e,
                Err(e) => {
                    let at = position_of(pos);
                    return Err(map_error(&e, at));
                }
            };

            match event {
                quick_xml::events::Event::Start(start) => {
                    self.depth += 1;
                    if self.depth > self.max_depth {
                        return Err(OfdError::Format(sofd_error::FormatError::MaxDepthExceeded(
                            self.max_depth,
                        )));
                    }
                    let node = build_node(&start, false)?;
                    self.open.push(node.local().to_string());
                    return Ok(Event::Start(node));
                }
                quick_xml::events::Event::Empty(empty) => {
                    // A self-closing tag is a whole element on its own, so it is
                    // flagged rather than left waiting for an end tag.
                    let node = build_node(&empty, true)?;
                    return Ok(Event::Start(node));
                }
                quick_xml::events::Event::End(end) => {
                    let name = local_name(end.name().as_ref()).to_string();
                    self.depth = self.depth.saturating_sub(1);
                    self.open.pop();
                    return Ok(Event::End(name));
                }
                quick_xml::events::Event::Text(text) => {
                    let owning = text.xml10_content();
                    if owning.trim().is_empty() {
                        continue;
                    }
                    return Ok(Event::Text(owning.to_string()));
                }
                quick_xml::events::Event::GeneralRef(reference) => {
                    // quick-xml hands `&lt;` back as its own event rather than
                    // folding it into the text around it, so resolving it here
                    // is what keeps a `<` in a value from vanishing.
                    let Some(resolved) = resolve_reference(&reference)? else {
                        continue;
                    };
                    if resolved.is_empty() {
                        continue;
                    }
                    return Ok(Event::Text(resolved));
                }
                quick_xml::events::Event::CData(cdata) => {
                    // CDATA is taken verbatim by design: it exists precisely so the
                    // author can write characters that would otherwise be markup.
                    let owned = cdata.into_inner().to_string();
                    if owned.trim().is_empty() {
                        continue;
                    }
                    return Ok(Event::Text(owned));
                }
                quick_xml::events::Event::Eof => {
                    // quick-xml reports a clean EOF for a truncated document, so
                    // the imbalance is detected here instead.
                    if let Some(unclosed) = self.open.last() {
                        return Err(OfdError::Xml(sofd_error::XmlError::Syntax {
                            line: 0,
                            col: 0,
                            msg: format!("unexpected end of document, <{unclosed}> is not closed"),
                        }));
                    }
                    return Ok(Event::Eof);
                }
                // Declarations, comments, processing instructions and DTDs carry
                // no document data, so they are skipped rather than surfaced.
                _ => continue,
            }
        }
    }

    /// Skip to and including the matching end tag; a self-closing element is
    /// already complete, so this consumes nothing.
    pub fn skip_element(&mut self, node: &Node) -> Result<()> {
        if node.is_self_closing() {
            return Ok(());
        }
        let mut depth = 1usize;
        loop {
            match self.next_event()? {
                Event::Start(n) => {
                    if !n.is_self_closing() {
                        depth += 1;
                    }
                }
                Event::End(_) => {
                    depth -= 1;
                    if depth == 0 {
                        return Ok(());
                    }
                }
                Event::Eof => return Ok(()),
                Event::Text(_) => {}
            }
        }
    }

    /// Read the next event, requiring it to be a start tag.
    pub fn expect_start(&mut self, what: &str) -> Result<Node> {
        match self.next_event()? {
            Event::Start(node) => Ok(node),
            Event::End(name) => Err(OfdError::Xml(sofd_error::XmlError::Syntax {
                line: 0,
                col: 0,
                msg: format!("expected <{what}>, found </{name}>"),
            })),
            Event::Text(t) => Err(OfdError::Xml(sofd_error::XmlError::Syntax {
                line: 0,
                col: 0,
                msg: format!("expected <{what}>, found text {t:?}"),
            })),
            Event::Eof => Err(OfdError::Xml(sofd_error::XmlError::Syntax {
                line: 0,
                col: 0,
                msg: format!("expected <{what}>, document ended"),
            })),
        }
    }

    /// Collect the character data of the element just opened. Only the ends of
    /// the value lose their blanks: the markup around it is indented, while a
    /// blank inside is a character the caller asked for.
    pub fn read_text(&mut self) -> Result<String> {
        let mut out = String::new();
        loop {
            match self.next_event()? {
                Event::Text(t) => out.push_str(&t),
                Event::End(_) => return Ok(out.trim().to_string()),
                Event::Start(nested) => {
                    // Nested markup is not text; skip it so a stray element cannot
                    // splice its contents into the parent's value.
                    self.skip_element(&nested)?;
                }
                Event::Eof => return Ok(out.trim().to_string()),
            }
        }
    }
}

/// Resolve one entity reference to the text it stands for. A reference the
/// standard does not define is left as it was written, so a producer's own
/// entity reaches the caller rather than disappearing.
fn resolve_reference(reference: &quick_xml::events::BytesRef<'_>) -> Result<Option<String>> {
    if let Some(ch) = reference.resolve_char_ref().map_err(|e| {
        OfdError::Xml(sofd_error::XmlError::Syntax {
            line: 0,
            col: 0,
            msg: format!("undecodable character reference: {e}"),
        })
    })? {
        return Ok(Some(ch.to_string()));
    }
    let name = reference.as_ref();
    match quick_xml::escape::resolve_predefined_entity(name) {
        Some(value) => Ok(Some(value.to_string())),
        None => Ok(Some(format!("&{name};"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn start_names(xml: &str) -> Vec<String> {
        let mut c = Cursor::new(xml);
        let mut out = Vec::new();
        loop {
            match c.next_event().expect("parse") {
                Event::Start(n) => out.push(n.local().to_string()),
                Event::End(_) => {}
                Event::Text(_) => {}
                Event::Eof => break,
            }
        }
        out
    }

    #[test]
    fn elements_are_visited_in_document_order() {
        let xml = r#"<OFD><DocBody><DocRoot>d</DocRoot></DocBody></OFD>"#;
        assert_eq!(start_names(xml), vec!["OFD", "DocBody", "DocRoot"]);
    }

    #[test]
    fn namespace_prefixes_are_stripped_from_element_names() {
        let xml = r#"<ofd:OFD xmlns:ofd="http://www.ofdspec.org/2016"><ofd:DocBody/></ofd:OFD>"#;
        assert_eq!(start_names(xml), vec!["OFD", "DocBody"]);
    }

    #[test]
    fn attributes_are_decoded_and_localized() {
        let xml = r#"<ofd:Page ofd:ID="7" Boundary="0 0 210 297"/>"#;
        let mut c = Cursor::new(xml);
        let node = c.expect_start("Page").expect("start");
        assert_eq!(node.attributes().get("ID"), Some("7"));
        assert_eq!(node.attributes().get("Boundary"), Some("0 0 210 297"));
    }

    #[test]
    fn xml_entities_in_attributes_are_unescaped() {
        let xml = r#"<T BaseLoc="a&amp;b&quot;c"/>"#;
        let mut c = Cursor::new(xml);
        let node = c.expect_start("T").expect("start");
        assert_eq!(node.attributes().get("BaseLoc"), Some("a&b\"c"));
    }

    #[test]
    fn self_closing_elements_report_as_start_without_an_end() {
        let xml = r#"<OFD><Empty/><Next/></OFD>"#;
        let mut c = Cursor::new(xml);
        assert!(matches!(c.next_event().expect("e"), Event::Start(_)));
        assert!(c.expect_start("Empty").is_ok());
        // No End was emitted for Empty, so the next event is the sibling.
        let next = c.expect_start("Next").expect("start");
        assert_eq!(next.local(), "Next");
    }

    #[test]
    fn text_is_trimmed_and_returned_verbatim() {
        let xml = r#"<DocRoot>  Doc_0/Document.xml  </DocRoot>"#;
        let mut c = Cursor::new(xml);
        let _ = c.expect_start("DocRoot").expect("start");
        assert_eq!(c.read_text().expect("text"), "Doc_0/Document.xml");
    }

    #[test]
    fn whitespace_only_text_is_skipped() {
        let xml = "<A>\n   \n  <B/>\n</A>";
        let mut c = Cursor::new(xml);
        let _ = c.expect_start("A").expect("start");
        let b = c.expect_start("B").expect("start");
        assert_eq!(b.local(), "B");
    }

    #[test]
    fn cdata_is_returned_as_text() {
        let xml = r#"<T><![CDATA[a<b&c]]></T>"#;
        let mut c = Cursor::new(xml);
        let _ = c.expect_start("T").expect("start");
        assert_eq!(c.read_text().expect("text"), "a<b&c");
    }

    #[test]
    fn comments_and_declarations_are_skipped() {
        let xml = r#"<?xml version="1.0"?><!-- note --><OFD><!-- inner --><DocBody/></OFD>"#;
        assert_eq!(start_names(xml), vec!["OFD", "DocBody"]);
    }

    #[test]
    fn nesting_depth_is_tracked() {
        let xml = "<A><B><C></C></B></A>";
        let mut c = Cursor::new(xml);
        let _ = c.expect_start("A").expect("start");
        assert_eq!(c.depth(), 1);
        let _ = c.expect_start("B").expect("start");
        assert_eq!(c.depth(), 2);
        let _ = c.expect_start("C").expect("start");
        assert_eq!(c.depth(), 3);
        assert_eq!(c.next_event().expect("end"), Event::End("C".to_string()));
        assert_eq!(c.depth(), 2);
        assert_eq!(c.next_event().expect("end"), Event::End("B".to_string()));
        assert_eq!(c.depth(), 1);
    }

    #[test]
    fn a_self_closing_tag_does_not_change_depth() {
        let xml = "<A><Empty/></A>";
        let mut c = Cursor::new(xml);
        let _ = c.expect_start("A").expect("start");
        assert_eq!(c.depth(), 1);
        let empty = c.expect_start("Empty").expect("start");
        assert!(empty.is_self_closing());
        // No End follows, so depth stays where the parent left it.
        assert_eq!(c.depth(), 1);
        assert_eq!(c.next_event().expect("end"), Event::End("A".to_string()));
        assert_eq!(c.depth(), 0);
    }

    #[test]
    fn excessive_nesting_is_refused() {
        let mut xml = String::new();
        for _ in 0..20 {
            xml.push_str("<A>");
        }
        for _ in 0..20 {
            xml.push_str("</A>");
        }
        let mut c = Cursor::with_max_depth(&xml, 5);
        let mut err = None;
        loop {
            match c.next_event() {
                Ok(Event::Eof) => break,
                Ok(_) => {}
                Err(e) => {
                    err = Some(e);
                    break;
                }
            }
        }
        assert!(
            matches!(
                err,
                Some(OfdError::Format(sofd_error::FormatError::MaxDepthExceeded(
                    5
                )))
            ),
            "unexpected result"
        );
    }

    #[test]
    fn malformed_xml_reports_a_syntax_error() {
        let xml = "<A><B></A>";
        let mut c = Cursor::new(xml);
        let mut saw_error = false;
        for _ in 0..10 {
            match c.next_event() {
                Ok(Event::Eof) => break,
                Ok(_) => {}
                Err(OfdError::Xml(sofd_error::XmlError::Syntax { .. })) => {
                    saw_error = true;
                    break;
                }
                Err(other) => panic!("unexpected error: {other}"),
            }
        }
        assert!(saw_error, "mismatched tags should be reported");
    }

    #[test]
    fn unclosed_document_is_reported() {
        let xml = "<A><B>";
        let mut c = Cursor::new(xml);
        let mut saw_error = false;
        for _ in 0..10 {
            match c.next_event() {
                Ok(Event::Eof) => break,
                Ok(_) => {}
                Err(_) => {
                    saw_error = true;
                    break;
                }
            }
        }
        assert!(saw_error, "an unterminated document should be reported");
    }

    #[test]
    fn expect_start_reports_what_it_found_instead() {
        let xml = "<A></A>";
        let mut c = Cursor::new(xml);
        let _ = c.expect_start("A").expect("start");
        let err = c.expect_start("B").expect_err("must fail");
        match err {
            OfdError::Xml(sofd_error::XmlError::Syntax { msg, .. }) => {
                assert!(msg.contains("expected <B>"), "msg was {msg}");
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn skip_element_discards_a_whole_subtree() {
        let xml = "<A><Skip><Deep><Deeper/></Deep></Skip><Keep/></A>";
        let mut c = Cursor::new(xml);
        let _ = c.expect_start("A").expect("start");
        let skip = c.expect_start("Skip").expect("start");
        c.skip_element(&skip).expect("skip");
        let keep = c.expect_start("Keep").expect("start");
        assert_eq!(keep.local(), "Keep");
    }

    #[test]
    fn node_helpers_match_exact_names_only() {
        let xml = "<Page/>";
        let mut c = Cursor::new(xml);
        let node = c.expect_start("Page").expect("start");
        assert!(node.is("Page"));
        assert!(!node.is("Pag"));
        assert!(node.is_any(&["Page", "Template"]));
        assert_eq!(node.qualified(), "Page");
    }

    #[test]
    fn read_text_ignores_nested_markup() {
        // A stray child element must not splice its text into the parent value.
        let xml = "<DocRoot>real<Emphasis>inner</Emphasis></DocRoot>";
        let mut c = Cursor::new(xml);
        let _ = c.expect_start("DocRoot").expect("start");
        assert_eq!(c.read_text().expect("text"), "real");
    }

    #[test]
    fn an_empty_document_yields_eof_immediately() {
        let mut c = Cursor::new("");
        assert!(matches!(c.next_event().expect("e"), Event::Eof));
    }
}
