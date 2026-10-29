//! Font face wrapping the shaping backend with a stable metrics and outline API.

use std::panic::AssertUnwindSafe;
use std::sync::Arc;

use skrifa::MetadataProvider;
use skrifa::raw::TableProvider;
use sofd_error::{FontError, Result};
use sofd_geometry::{Path, PathSegment};

use crate::cff;
use crate::guard;
use crate::metrics::{FontMetrics, GlyphOutline};
use crate::registry::FontId;
use crate::sfnt;

/// A usable font face.
/// `ttf_parser::Face` wants a `'static` borrow, so bytes live behind an
/// `Arc<Vec<u8>>` and each query builds a temporary `Face` from them.
pub struct FontFace {
    id: FontId,
    family: String,
    data: Arc<Vec<u8>>,
    /// Which face of the file this is: zero for a single font, and the index
    /// the collection header lists it under otherwise.
    face_index: u32,
    is_fallback: bool,
    metrics: FontMetrics,
    units_per_em: f64,
    /// Whether the face is keyed by CID, which decides how the glyph codes
    /// of a `CGTransform` run are read.
    cid_keyed: bool,
    /// CID to glyph id, sorted by CID and empty for every other face.
    cid_to_gid: Vec<(u16, u16)>,
}

impl std::fmt::Debug for FontFace {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FontFace")
            .field("id", &self.id)
            .field("family", &self.family)
            .field("bytes", &self.data.len())
            .field("face_index", &self.face_index)
            .field("is_fallback", &self.is_fallback)
            .field("cid_keyed", &self.cid_keyed)
            .finish()
    }
}

impl FontFace {
    /// Validate the font data and precompute metrics, so a bad font fails at
    /// load time. A bare CFF is wrapped in an sfnt directory before the backend reads it.
    pub fn new(id: FontId, family: String, data: Arc<Vec<u8>>, is_fallback: bool) -> Result<Self> {
        let (data, cid_keyed, cid_to_gid) = if cff::is_bare_cff(&data) {
            let cids = cff::cid_charset(&data);
            let wrapped = cff::wrap_bare_cff(&data).ok_or_else(|| {
                FontError::Parse("a bare CFF font could not be wrapped".to_string())
            })?;
            (
                Arc::new(wrapped),
                cids.is_some(),
                cid_to_gid(cids.as_deref()),
            )
        } else {
            (data, false, Vec::new())
        };
        // The directory is read here first: the backend asserts on one that
        // does not fit, and a panic is a crash where unwinding is compiled out.
        let Some(index) = sfnt::first_face(data.as_slice()) else {
            return Err(FontError::Parse("the table directory does not fit".to_string()).into());
        };
        let (units_per_em, metrics) = guard::guard("the face", || {
            let font = skrifa::FontRef::from_index(data.as_slice(), index)
                .map_err(|e| FontError::Parse(format!("font parse failed: {e:?}")))?;
            let head = font
                .head()
                .map_err(|e| FontError::Parse(format!("the head table is unreadable: {e:?}")))?;
            let hhea = font
                .hhea()
                .map_err(|e| FontError::Parse(format!("the hhea table is unreadable: {e:?}")))?;

            let upem = head.units_per_em() as f64;
            if upem <= 0.0 {
                return Err(FontError::Parse("font units_per_em is zero".to_string()).into());
            }
            let k = 1000.0 / upem;
            // `OS/2` is optional in the format, and the two heights it carries
            // are version 2 additions, so either absence is a face that states
            // no cap or x height rather than a broken file.
            let os2 = font.os2().ok();
            let ascent = hhea.ascender().to_i16();
            let cap_height = os2
                .as_ref()
                .and_then(|t| t.s_cap_height())
                .unwrap_or(ascent);
            let x_height = os2.as_ref().and_then(|t| t.sx_height()).unwrap_or(0);

            let m = FontMetrics {
                units_per_em: head.units_per_em(),
                ascender: ascent as f64 * k,
                descender: hhea.descender().to_i16() as f64 * k,
                line_gap: hhea.line_gap().to_i16() as f64 * k,
                cap_height: cap_height as f64 * k,
                x_height: x_height as f64 * k,
            };
            Ok((upem, m))
        })?;

        Ok(Self {
            id,
            family,
            data,
            face_index: index,
            is_fallback,
            metrics,
            units_per_em,
            cid_keyed,
            cid_to_gid,
        })
    }

    /// Font identifier.
    pub fn id(&self) -> FontId {
        self.id
    }

    /// Family name.
    pub fn family(&self) -> &str {
        &self.family
    }

    /// Whether this face substitutes for a missing font.
    pub fn is_fallback(&self) -> bool {
        self.is_fallback
    }

    /// Font units per em.
    pub fn units_per_em(&self) -> f64 {
        self.units_per_em
    }

    /// Cached global metrics.
    pub fn metrics(&self) -> FontMetrics {
        self.metrics
    }

    /// Raw font bytes. A bare CFF arrives as one and is stored as the sfnt
    /// wrapper the backend reads, since that is what every query runs on.
    pub fn data(&self) -> &Arc<Vec<u8>> {
        &self.data
    }

    /// Glyph id for a CID, the index space a CID-keyed font's `CGTransform`
    /// names. A face that is not keyed by CID has no map and answers `None`.
    pub fn glyph_for_cid(&self, cid: u16) -> Option<u16> {
        let at = self
            .cid_to_gid
            .binary_search_by(|(c, _)| c.cmp(&cid))
            .ok()?;
        Some(self.cid_to_gid[at].1)
    }

    /// Whether the face has an entry of its own for one run code, whatever
    /// outline that entry holds. A CID zero names no glyph at all, so only a
    /// real glyph id counts as an answer.
    pub fn codes_glyph(&self, code: u16) -> bool {
        let found = if self.cid_keyed {
            self.glyph_for_cid(code)
        } else {
            Some(code)
        };
        found.is_some_and(|gid| {
            self.with_face(|f| f.maxp().map(|t| t.num_glyphs()).unwrap_or(0))
                .is_some_and(|n| gid < n)
        })
    }

    /// Outline for one entry of a display-list glyph run: a `CGTransform` run
    /// holds glyph indices, any other run Unicode code points.
    pub fn outline_for_run_code(&self, code: u16, glyph_index: bool) -> Option<Path> {
        if !glyph_index {
            return self.outline_for_codepoint(u32::from(code));
        }
        if self.cid_keyed {
            return self.glyph_for_cid(code).and_then(|gid| self.outline(gid));
        }
        self.outline(code)
    }

    /// Build a temporary backend face for one query. A panic in the backend
    /// answers the query with nothing rather than ending the process.
    fn with_face<T>(&self, f: impl FnOnce(&skrifa::FontRef<'_>) -> T) -> Option<T> {
        // The closure borrows `f`, which is not unwind safe, so the call is
        // wrapped rather than asserted safe. The index was read at load time.
        let queried = AssertUnwindSafe(|| {
            let font = skrifa::FontRef::from_index(self.data.as_slice(), self.face_index)
                .map_err(|e| FontError::Parse(format!("font parse failed: {e:?}")))?;
            Ok(f(&font))
        });
        guard::guard("a glyph", queried).ok()
    }

    /// Glyph id for a Unicode code point.
    pub fn glyph_id(&self, codepoint: u32) -> Option<u16> {
        self.with_face(|f| {
            let (_, _, sub) = f.cmap().ok()?.best_subtable()?;
            sub.map_codepoint(codepoint).map(|g| g.to_u32() as u16)
        })
        .flatten()
    }

    /// Reverse lookup from glyph id to Unicode, needed for search and copy.
    /// Walks the `cmap` subtables at O(cmap size), so callers should cache.
    pub fn reverse_cmap(&self, glyph: u16) -> Option<u32> {
        self.with_face(|font| {
            let cmap = font.cmap().ok()?;
            // Every subtable, not just the best one: a glyph a symbol or a
            // Mac Roman subtable names is still a glyph the face maps.
            for record in cmap.encoding_records() {
                let Ok(sub) = record.subtable(cmap.offset_data()) else {
                    continue;
                };
                for (cp, g) in sub.iter() {
                    if g.to_u32() == u32::from(glyph) {
                        return Some(cp);
                    }
                }
            }
            None
        })
        .flatten()
    }

    /// Horizontal advance in 1/1000 em, the OFD convention.
    pub fn advance_width(&self, glyph: u16) -> f64 {
        self.with_face(|f| {
            let upem = f.head().map(|h| h.units_per_em()).unwrap_or(1).max(1) as f64;
            let gid = skrifa::GlyphId::new(u32::from(glyph));
            let units = f.hmtx().ok().and_then(|h| h.advance(gid)).unwrap_or(0) as f64;
            units * 1000.0 / upem
        })
        .unwrap_or(0.0)
    }

    /// Glyph outline normalized to the 1/1000 em space, scaled by the render
    /// layer's CTM rather than sized here. No ink answers `None`, not an empty
    /// path: a caller transforming what it gets back sees the difference.
    pub fn outline(&self, glyph: u16) -> Option<Path> {
        self.with_face(|font| {
            let upem = font.head().map(|h| h.units_per_em()).unwrap_or(0) as f64;
            if upem <= 0.0 {
                return None;
            }
            let mut builder = OutlineBuilder {
                path: Path::new(),
                scale: 1000.0 / upem,
            };
            let gid = skrifa::GlyphId::new(u32::from(glyph));
            let outline = font.outline_glyphs().get(gid)?;
            outline
                .draw(
                    skrifa::outline::DrawSettings::unhinted(
                        skrifa::instance::Size::unscaled(),
                        &[] as &[skrifa::instance::NormalizedCoord],
                    ),
                    &mut builder,
                )
                .ok()?;
            if builder.path.is_empty() {
                return None;
            }
            Some(builder.path)
        })
        .flatten()
    }

    /// Outline result that distinguishes an empty glyph such as a space.
    pub fn glyph_outline(&self, glyph: u16) -> GlyphOutline {
        match self.outline(glyph) {
            Some(p) if !p.is_empty() => GlyphOutline::Path(p),
            _ => GlyphOutline::Empty,
        }
    }

    /// Outline for a Unicode code point, mapped through the cmap first.
    /// Display lists carry code points in place of glyph ids until shaping
    /// lands, so consumers must not treat the raw value as a glyph index.
    pub fn outline_for_codepoint(&self, codepoint: u32) -> Option<Path> {
        let glyph = self.glyph_id(codepoint)?;
        self.outline(glyph)
    }

    /// Whether the face covers a code point.
    pub fn has_codepoint(&self, codepoint: u32) -> bool {
        self.glyph_id(codepoint).is_some()
    }

    /// Whether the face covers every code point given. Whitespace is skipped:
    /// a space is often drawn from the fallback rather than mapped, and a run
    /// whose only unmapped character is a space still reads correctly.
    pub fn covers(&self, codepoints: &[u32]) -> bool {
        self.with_face(|font| {
            let Some((_, _, sub)) = font.cmap().ok().and_then(|c| c.best_subtable()) else {
                return false;
            };
            codepoints.iter().all(|&cp| {
                if cp < 0x20 || is_space(cp) {
                    return true;
                }
                sub.map_codepoint(cp).is_some()
            })
        })
        .unwrap_or(false)
    }
}

/// Whitespace a face need not map to count as covering a run.
pub(crate) fn is_space(cp: u32) -> bool {
    matches!(cp, 0x20 | 0xA0 | 0x3000)
}

/// Turn a charset read off a CID-keyed CFF into the reverse map a glyph
/// lookup wants: CID to glyph id, sorted by CID, with `.notdef` left out.
/// A CID the charset names twice keeps its first glyph id.
fn cid_to_gid(cids: Option<&[u16]>) -> Vec<(u16, u16)> {
    let mut out: Vec<(u16, u16)> = cids
        .unwrap_or(&[])
        .iter()
        .enumerate()
        .skip(1)
        .map(|(gid, cid)| (*cid, gid as u16))
        .collect();
    out.sort_unstable_by_key(|(cid, _)| *cid);
    out.dedup_by_key(|(cid, _)| *cid);
    out
}

/// Adapts the backend outline callbacks into this crate's `Path`.
struct OutlineBuilder {
    path: Path,
    scale: f64,
}

impl skrifa::outline::OutlinePen for OutlineBuilder {
    fn move_to(&mut self, x: f32, y: f32) {
        self.path.segments.push(PathSegment::MoveTo(
            x as f64 * self.scale,
            y as f64 * self.scale,
        ));
    }

    fn line_to(&mut self, x: f32, y: f32) {
        self.path.segments.push(PathSegment::LineTo(
            x as f64 * self.scale,
            y as f64 * self.scale,
        ));
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        self.path.segments.push(PathSegment::QuadTo {
            c: (x1 as f64 * self.scale, y1 as f64 * self.scale),
            to: (x as f64 * self.scale, y as f64 * self.scale),
        });
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        self.path.segments.push(PathSegment::CurveTo {
            c1: (x1 as f64 * self.scale, y1 as f64 * self.scale),
            c2: (x2 as f64 * self.scale, y2 as f64 * self.scale),
            to: (x as f64 * self.scale, y as f64 * self.scale),
        });
    }

    fn close(&mut self) {
        self.path.segments.push(PathSegment::Close);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cff::tests::synthetic_cff;

    fn face_of(data: &[u8]) -> FontFace {
        FontFace::new(
            FontId(1),
            "TestCFF".to_string(),
            Arc::new(data.to_vec()),
            false,
        )
        .expect("the face loads")
    }

    #[test]
    fn a_bare_cff_face_loads_and_keeps_its_cid_map() {
        let face = face_of(&synthetic_cff());
        assert_eq!(face.units_per_em(), 1000.0);
        assert_eq!(face.glyph_for_cid(963), Some(1));
        assert_eq!(face.glyph_for_cid(1217), Some(2));
        assert_eq!(face.glyph_for_cid(0), None, ".notdef has no CID");
        assert_eq!(face.glyph_for_cid(1), None, "a glyph id is not a CID");
    }

    #[test]
    fn a_cid_run_resolves_through_the_charset() {
        let face = face_of(&synthetic_cff());
        let outline = face
            .outline_for_run_code(963, true)
            .expect("the CID names a glyph");
        // A square is a move, four lines and a close. The closing line back to
        // the start is not one of them: `Close` already returns to it, and a
        // repeated `LineTo` only re-states the point a fill ignores.
        assert_eq!(outline.segments.len(), 5, "a square is four lines");
        assert!(matches!(outline.segments.last(), Some(PathSegment::Close)));
        // The other direction is a code point, and a CFF has no cmap, so it
        // finds nothing rather than the wrong glyph.
        assert!(face.outline_for_run_code(963, false).is_none());
    }

    #[test]
    fn a_glyph_with_no_ink_answers_with_nothing() {
        // A space maps to a glyph the face carries and draws nothing for, and
        // the parsers this layer has used disagree on that answer's shape.
        let face = face_of(&latin_face());
        let space = face.glyph_id(u32::from(' ')).expect("a space is mapped");
        assert!(face.outline(space).is_none(), "no ink, no outline");
        assert!(matches!(face.glyph_outline(space), GlyphOutline::Empty));
        let ink = face.glyph_id(u32::from('A')).expect("a letter is mapped");
        assert!(face.outline(ink).is_some_and(|p| !p.is_empty()));
    }

    /// The Latin matrix fixture, which has both ink and a mapped space.
    fn latin_face() -> Vec<u8> {
        std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/data/sofd-latin-subset.ttf"
        ))
        .expect("font fixture")
    }

    #[test]
    fn an_sfnt_face_answers_cid_lookups_with_nothing() {
        let orbitron = std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../sofd-render/tests/data/Orbitron-Bold.ttf"
        ))
        .expect("font fixture");
        let face = face_of(&orbitron);
        assert!(face.glyph_for_cid(963).is_none());
        // Without a CGTransform the codes stay code points, which is the
        // path every document without one already takes.
        assert!(face.outline_for_run_code('A' as u16, false).is_some());
    }
}
