//! Text types: glyph runs and character-level text spans.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sofd_common::{Millimeter, Rect};
use sofd_font::{FontFace, FontId, FontManager};
use sofd_geometry::Affine;

/// A run of text sharing one font and transform.
/// Carries glyph ids and Unicode text together, since the OFD `TextCode` and
/// `CGTransform` mapping may be many-to-one.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GlyphRun {
    /// Font handle.
    pub font: FontId,
    /// Font size in millimetres.
    pub size: Millimeter,
    /// Glyph ids.
    pub glyphs: Vec<u16>,
    /// Whether `glyphs` holds font glyph indices rather than Unicode code
    /// points; a `CGTransform` names glyphs, every other run code points.
    #[serde(default)]
    pub codes_are_glyphs: bool,
    /// Horizontal advance per glyph in millimetres; same length as `glyphs`.
    pub advances: Vec<f64>,
    /// Original Unicode text; may be absent for subset fonts.
    pub text: Option<String>,
    /// Glyph-to-character index map, same length as `glyphs`.
    pub cluster_map: Option<Vec<u32>>,
    /// Text transform, combining position, rotation and scale.
    pub transform: Affine,
    /// Angle each glyph is turned about its own origin, clockwise from the
    /// run's `+x`. It carries table 47's page angle less the line's own turn.
    #[serde(default)]
    pub glyph_angle: f64,
    /// Fill paint.
    pub fill: Option<crate::ops::Paint>,
    /// Stroke paint, used for outlined glyphs.
    pub stroke: Option<crate::ops::Paint>,
    /// Width, dash and joins for `stroke`, in millimetres.
    #[serde(default)]
    pub stroke_style: crate::ops::StrokeStyle,
    /// Extra spacing between characters, in millimetres.
    pub letter_spacing: f64,
    /// Extra spacing between words, in millimetres.
    pub word_spacing: f64,
    /// Absolute page-space bounds, used for hit testing and search anchoring.
    pub bbox: Rect,
}

impl GlyphRun {
    /// Number of glyphs in the run.
    pub fn len(&self) -> usize {
        self.glyphs.len()
    }

    /// Whether the run contains no glyphs.
    pub fn is_empty(&self) -> bool {
        self.glyphs.is_empty()
    }

    /// Extracted plain text, or an empty string when unavailable.
    pub fn plain_text(&self) -> &str {
        self.text.as_deref().unwrap_or("")
    }

    /// Where each glyph sits along the run, in millimetres.
    ///
    /// The advance of the glyph before it moves the pen, and `letter_spacing`
    /// moves it once per glyph. Every backend places glyphs from this list, so
    /// the offsets cannot drift apart between the raster, vector, SVG and PDF
    /// paths.
    pub fn glyph_offsets(&self) -> Vec<f64> {
        let mut offset = 0.0;
        let mut out = Vec::with_capacity(self.glyphs.len());
        for (i, _) in self.glyphs.iter().enumerate() {
            if i > 0 {
                offset += self.advances.get(i - 1).copied().unwrap_or(0.0);
            }
            offset += self.letter_spacing;
            out.push(offset);
        }
        out
    }

    /// The matrix that draws a glyph, normalized to 1/1000 em with `y` up, at
    /// `offset` along the run. It turns about its origin before the advance
    /// moves it, so the angle faces the glyph without stepping it.
    pub fn glyph_matrix(&self, offset: f64) -> Affine {
        let k = self.size.value() / 1000.0;
        Affine::new(k, 0.0, 0.0, -k, 0.0, 0.0)
            .then(Affine::rotate_deg(self.glyph_angle))
            .then(Affine::translate(offset, 0.0))
            .then(self.transform)
    }
}

/// The face to draw a run with: its own when it loads, otherwise one from the
/// fallback chain, so a document naming a face it never shipped can still draw.
/// A glyph index run is never rescued, since an index means nothing without it.
pub fn run_face(fonts: &FontManager, run: &GlyphRun) -> Option<Arc<FontFace>> {
    run_face_for(fonts, run, &[])
}

/// [`run_face`], told what the run is going to draw, so the substitute is one
/// that covers those characters rather than one that merely exists.
pub fn run_face_for(fonts: &FontManager, run: &GlyphRun, wanted: &[u32]) -> Option<Arc<FontFace>> {
    if let Ok(face) = fonts.face_for(run.font, wanted) {
        return Some(face);
    }
    if run.codes_are_glyphs {
        return None;
    }
    run.glyphs.iter().find_map(|&code| {
        fonts
            .fallback_for_wanted(u32::from(code), wanted)
            .map(|(_, face)| face)
    })
}

/// A [`GlyphRun`] expanded to character granularity.
/// Search hits and annotations need per-character boxes while a run has only
/// one, so search and edit expand on demand.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TextSpan {
    /// Unicode text of this span, usually one character or one word.
    pub text: String,
    /// Absolute page-space bounds.
    pub bbox: Rect,
    /// Page this span belongs to.
    pub page: usize,
    /// Index of the source glyph run, for writing annotations back.
    pub run_index: usize,
    /// First glyph offset within the run.
    pub glyph_start: usize,
    /// Number of glyphs covered.
    pub glyph_count: usize,
}
