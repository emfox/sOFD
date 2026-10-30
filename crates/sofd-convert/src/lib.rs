//! sofd-convert: one page's op stream, written as another format.
//! A new target format means one [`replay::PageSink`] and nothing else.
//!
//! A page, not a document: assembling pages into one file belongs to whoever
//! holds the document, a layer above. What this crate hands that caller — the
//! facade `sofd` — is the per-page sink, and nothing here opens a document.
#![forbid(unsafe_code)]
#![warn(missing_docs)]

use sofd_font::Unresolved;

pub mod png;
pub mod replay;
pub mod svg;

#[cfg(feature = "pdf")]
pub mod pdf;

pub use png::{encode_png, page_to_png};
pub use replay::{PageSink, PaintShape, PathShape, paint_shape, replay};
pub use svg::{SvgOptions, SvgSink, page_to_svg, page_to_svg_with_stats};

#[cfg(feature = "pdf")]
pub use pdf::{PdfOptions, PdfSink, page_to_pdf, page_to_pdf_with_stats};

/// How glyph runs become target-format content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextExport {
    /// Outline each glyph into vector paths, which needs a font source.
    #[default]
    Outline,
    /// Emit the run's Unicode text and let the viewer choose a font.
    LiveText,
}

/// What a conversion actually produced.
/// A converter can silently degrade, so callers that care about fidelity read
/// this instead of guessing from the output.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConversionStats {
    /// Glyphs turned into vector outlines.
    pub outlined_glyphs: usize,
    /// Runs emitted as live text rather than outlines.
    pub live_text_runs: usize,
    /// Runs dropped: no font data and no Unicode text to fall back to.
    pub dropped_runs: usize,
    /// Runs that reached the output with at least one glyph missing from the
    /// face, so the page shows fewer marks than it should. Kept apart from
    /// `dropped_runs`, which counts whole runs that never appear at all.
    pub missing_glyph_runs: usize,
    /// Glyph positions that resolved to no outline at all, so the output is
    /// short of marks even where a run made it through.
    pub missing_glyphs: usize,
    /// Faces the document asked for and no host face answered, one entry per
    /// face. A converter that drops a whole run for want of a font reports it
    /// here instead of only counting a drop.
    pub missing_fonts: Vec<Unresolved>,
    /// Images embedded in the output.
    pub images: usize,
    /// Images skipped because the target cannot carry their encoding.
    pub images_skipped: usize,
    /// Gradients emitted.
    pub gradients: usize,
    /// Fills painted as a stand-in rather than as the document states: a PDF
    /// tiling pattern flattened to its first colour, an SVG mesh with no
    /// triangles left. Counted so a caller can say what the export simplified.
    pub paints_approximated: usize,
}

impl ConversionStats {
    /// Merge another page's counts into this one.
    pub fn absorb(&mut self, other: &Self) {
        self.outlined_glyphs += other.outlined_glyphs;
        self.live_text_runs += other.live_text_runs;
        self.dropped_runs += other.dropped_runs;
        self.missing_glyph_runs += other.missing_glyph_runs;
        self.missing_glyphs += other.missing_glyphs;
        self.missing_fonts
            .extend(other.missing_fonts.iter().cloned());
        self.missing_fonts.sort_by_key(|f| f.id);
        self.missing_fonts.dedup_by_key(|f| f.id);
        self.images += other.images;
        self.images_skipped += other.images_skipped;
        self.gradients += other.gradients;
        self.paints_approximated += other.paints_approximated;
    }

    /// Whether nothing had to be dropped or skipped. A missing face counts
    /// even when its runs were drawn with a substitute: the output is not the
    /// document it came from.
    pub fn is_lossless(&self) -> bool {
        self.dropped_runs == 0
            && self.images_skipped == 0
            && self.missing_glyph_runs == 0
            && self.missing_glyphs == 0
            && self.missing_fonts.is_empty()
    }
}
