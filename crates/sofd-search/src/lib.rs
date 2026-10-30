//! Full-text search over flattened display lists.
//! The index is built once per document and answers queries with page, run
//! and rectangle, so a viewer can highlight hits without re-flattening.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use sofd_common::Rect;
use sofd_display::text::{GlyphRun, run_face};
use sofd_display::{DisplayList, DrawOp};
use sofd_font::{FontFace, FontManager};

/// The text a run stands for, from its own field or from the face's `cmap`.
fn text_of(run: &GlyphRun, fonts: Option<&FontManager>) -> String {
    if let Some(text) = &run.text {
        return text.clone();
    }
    if run.codes_are_glyphs {
        // Glyph indices mean nothing without the face that names them.
        let Some(face) = fonts.and_then(|fonts| run_face(fonts, run)) else {
            return String::new();
        };
        return run
            .glyphs
            .iter()
            .filter_map(|&glyph| face.reverse_cmap(glyph))
            .filter_map(char::from_u32)
            .collect();
    }
    run.glyphs
        .iter()
        .filter_map(|&code| char::from_u32(u32::from(code)))
        .collect()
}

/// One indexed text run on one page.
#[derive(Debug, Clone)]
pub struct IndexedRun {
    /// Page the run belongs to.
    pub page: usize,
    /// Position of the run inside the page's op vector.
    pub op_index: usize,
    /// Unicode text of the run, empty when the run carries none.
    pub text: String,
    /// Page-space bounds of the whole run.
    pub bbox: Rect,
    /// Per-glyph advances, used to narrow hit rectangles.
    advances: Vec<f64>,
}

/// Searchable text of one page.
#[derive(Debug, Clone, Default)]
pub struct IndexedPage {
    /// Page index.
    pub page: usize,
    /// Runs in drawing order.
    pub runs: Vec<IndexedRun>,
}

impl IndexedPage {
    /// Concatenated text of every run, separated by newlines.
    pub fn text(&self) -> String {
        let mut out = String::new();
        for run in &self.runs {
            if !out.is_empty() {
                out.push('\n');
            }
            out.push_str(&run.text);
        }
        out
    }
}

/// Document-wide text index.
#[derive(Debug, Clone, Default)]
pub struct SearchIndex {
    pages: Vec<IndexedPage>,
}

impl SearchIndex {
    /// Build an index from a display list.
    pub fn build(list: &DisplayList) -> Self {
        Self::collect(list, None)
    }

    /// Build an index, reading text from the font for runs that carry only
    /// glyph ids: a subset font with no ToUnicode map still names its glyphs,
    /// and the face can turn them back into characters.
    pub fn build_with_fonts(list: &DisplayList, fonts: &FontManager) -> Self {
        Self::collect(list, Some(fonts))
    }

    fn collect(list: &DisplayList, fonts: Option<&FontManager>) -> Self {
        let mut pages = Vec::new();
        for page in &list.pages {
            let mut indexed = IndexedPage {
                page: page.index,
                runs: Vec::new(),
            };
            for (op_index, op) in page.ops.iter().enumerate() {
                if let DrawOp::Text(run) = op {
                    indexed.runs.push(IndexedRun {
                        page: page.index,
                        op_index,
                        text: text_of(run, fonts),
                        bbox: run.bbox,
                        advances: run.advances.clone(),
                    });
                }
            }
            pages.push(indexed);
        }
        Self { pages }
    }

    /// Pages in index order.
    pub fn pages(&self) -> &[IndexedPage] {
        &self.pages
    }

    /// Text of one page, or empty when the page is unknown.
    pub fn page_text(&self, index: usize) -> String {
        self.pages
            .iter()
            .find(|p| p.page == index)
            .map(IndexedPage::text)
            .unwrap_or_default()
    }

    /// Search every page, returning hits in page order.
    pub fn search(&self, needle: &str, options: SearchOptions) -> Vec<SearchHit> {
        let mut hits = Vec::new();
        if needle.is_empty() {
            return hits;
        }
        for page in &self.pages {
            for (run_position, run) in page.runs.iter().enumerate() {
                if run.text.is_empty() {
                    continue;
                }
                let haystack = if options.case_sensitive {
                    run.text.clone()
                } else {
                    run.text.to_lowercase()
                };
                let probe = if options.case_sensitive {
                    needle.to_string()
                } else {
                    needle.to_lowercase()
                };
                let mut from = 0usize;
                while let Some(found) = haystack[from..].find(&probe) {
                    let start = from + found;
                    let end = start + probe.len();
                    from = end;
                    if options.whole_word && !whole_word(&haystack, start, end) {
                        continue;
                    }
                    hits.push(SearchHit {
                        page: page.page,
                        run: run_position,
                        op_index: run.op_index,
                        start,
                        end,
                        rect: run.rect_for(start, end),
                        snippet: snippet(&run.text, start, end),
                    });
                    if let Some(limit) = options.max_hits
                        && hits.len() >= limit
                    {
                        return hits;
                    }
                }
            }
        }
        hits
    }
}

/// Query behaviour.
#[derive(Debug, Default, Clone, Copy)]
pub struct SearchOptions {
    /// Match case exactly.
    pub case_sensitive: bool,
    /// Require the match to be delimited by non-word characters.
    pub whole_word: bool,
    /// Stop after this many hits.
    pub max_hits: Option<usize>,
}

/// One match.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    /// Page containing the match.
    pub page: usize,
    /// Run index inside the page's run list.
    pub run: usize,
    /// Op index inside the page's display list.
    pub op_index: usize,
    /// Byte offset of the first matched character inside the run.
    pub start: usize,
    /// Byte offset one past the last matched character.
    pub end: usize,
    /// Page-space rectangle covering the match.
    pub rect: Rect,
    /// Short text around the match, for result lists.
    pub snippet: String,
}

impl IndexedRun {
    /// Narrow the run's rect to the given character range.
    pub fn rect_for(&self, start: usize, end: usize) -> Rect {
        let total: f64 = self.advances.iter().sum();
        if total <= 0.0 || self.advances.is_empty() {
            return self.bbox;
        }
        let prefix: f64 = self.advances.iter().take(start).sum();
        let matched: f64 = self
            .advances
            .iter()
            .skip(start)
            .take(end.saturating_sub(start).max(1))
            .sum();
        let scale = self.bbox.w / total;
        Rect::new(
            self.bbox.x + prefix * scale,
            self.bbox.y,
            (matched * scale).max(self.bbox.w * 0.01),
            self.bbox.h,
        )
    }
}

/// Reverse lookup: the text covered by one glyph of a run.
pub fn glyph_to_text(run: &GlyphRun, glyph_index: usize) -> Option<String> {
    let text = run.text.as_ref()?;
    let cluster = run.cluster_map.as_ref()?;
    let target = *cluster.get(glyph_index)? as usize;
    let next = cluster
        .get(glyph_index + 1)
        .map(|v| *v as usize)
        .unwrap_or(text.chars().count());
    let mut out = String::new();
    for (position, ch) in text.chars().enumerate() {
        if position >= target && position < next {
            out.push(ch);
        }
    }
    if out.is_empty() {
        text.chars().nth(target).map(|c| c.to_string())
    } else {
        Some(out)
    }
}

/// Reverse lookup with a face behind it, for a run that carries no text.
///
/// The cluster map answers when the run has one; otherwise the glyph id is
/// asked of the face's `cmap`, which is what makes a subset font selectable.
pub fn glyph_to_text_with_face(
    run: &GlyphRun,
    glyph_index: usize,
    face: &FontFace,
) -> Option<String> {
    if let Some(text) = glyph_to_text(run, glyph_index) {
        return Some(text);
    }
    let glyph = *run.glyphs.get(glyph_index)?;
    face.reverse_cmap(glyph)
        .and_then(char::from_u32)
        .map(String::from)
}

fn whole_word(haystack: &str, start: usize, end: usize) -> bool {
    let before = haystack[..start].chars().next_back();
    let after = haystack[end..].chars().next();
    !before.is_some_and(is_word_char) && !after.is_some_and(is_word_char)
}

fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

fn snippet(text: &str, start: usize, end: usize) -> String {
    let pad = 8usize;
    let from = start.saturating_sub(pad);
    let to = (end + pad).min(text.len());
    let mut out = String::new();
    if from > 0 {
        out.push('…');
    }
    out.push_str(&text[from..to]);
    if to < text.len() {
        out.push('…');
    }
    out
}
