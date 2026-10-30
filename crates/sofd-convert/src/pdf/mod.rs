//! PDF backend.
//! Every transform is baked into the content stream, so the output is a flat
//! sequence of absolute painting operators that any viewer can replay.
//!
//! One page in, one page out: [`PdfSink::emit_page`] hands back the object it
//! wrote, so a document-level export composes pages above this crate.

use std::fmt::Write as _;

use lopdf::{Dictionary, Document, Object, ObjectId, Stream};
use sofd_common::Rect;
use sofd_display::ops::{Color, GradientStop, LineCap, LineJoin, PaintRule, StrokeStyle};
use sofd_display::{GlyphRun, ImageRef, LinearGradientSpec, MeshSpec, RadialGradientSpec};
use sofd_error::{ConvertError, OfdError, Result};
use sofd_font::FontManager;
use sofd_geometry::{Affine, Path, PathSegment, arc_cubics};

use crate::replay::{PageSink, PaintShape, PathShape, replay};
use crate::{ConversionStats, TextExport};

/// Points per millimetre.
const PT_PER_MM: f64 = 72.0 / 25.4;

/// Options for PDF export.
#[derive(Debug, Clone, Default)]
pub struct PdfOptions<'a> {
    /// How glyph runs become PDF content.
    pub text: TextExport,
    /// Font source for outlined text; without one, runs fall back to live text.
    pub fonts: Option<&'a FontManager>,
}

/// An image waiting to become an XObject.
struct PendingImage {
    /// Encoded bytes.
    data: std::sync::Arc<Vec<u8>>,
    /// Intrinsic pixel dimensions.
    pixels: (u32, u32),
    /// Encoding the bytes are stored in.
    codec: sofd_display::ImageCodec,
}

/// A gradient waiting to become a shading pattern.
struct PendingShading {
    /// Whether the gradient runs out of a circle rather than along an axis.
    radial: bool,
    /// PDF `Coords`: two points, or two circle centres with radii.
    coords: Vec<f64>,
    /// Colour stops.
    stops: Vec<GradientStop>,
    /// A Gouraud mesh's triangles, each vertex a position and a colour. When
    /// present the shading is written as PDF `ShadingType 4`, not a gradient.
    mesh: Option<MeshShading>,
}

/// A Gouraud mesh ready to be written as a PDF free-form triangle shading.
struct MeshShading {
    /// Vertices, each `(x, y, r, g, b)` in the page's point space.
    vertices: Vec<(f64, f64, u8, u8, u8)>,
    /// Vertex indices, three per triangle.
    triangles: Vec<[usize; 3]>,
}

/// Builds one PDF page by replaying a display list into it.
pub struct PdfSink<'a> {
    ops: String,
    page_box: Rect,
    text: TextExport,
    fonts: Option<&'a FontManager>,
    ctm: Affine,
    stack: Vec<Affine>,
    /// Open clip groups per save level, closed by the matching restore.
    clips: Vec<usize>,
    images: Vec<PendingImage>,
    /// All shadings in creation order; the name is the position.
    shadings: Vec<PendingShading>,
    /// Distinct alpha values, each becoming one ExtGState.
    alphas: Vec<f64>,
    /// Whether live text was emitted, which needs the Helvetica resource.
    live_text: bool,
    stats: ConversionStats,
}

/// What outlining one glyph run produced.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Outlined {
    /// At least one glyph reached the page.
    Drew,
    /// Every code named a glyph the face or a substitute could draw.
    Blank,
    /// At least one code had no glyph anywhere, which the stats already hold.
    Missing,
    /// The run had no face or no fill, so none of it was drawn.
    Dropped,
}

impl<'a> PdfSink<'a> {
    /// Start a page of the given size.
    pub fn new(page_box: Rect, options: &PdfOptions<'a>) -> Self {
        Self {
            ops: String::new(),
            page_box,
            text: options.text,
            fonts: options.fonts,
            ctm: Affine::IDENTITY,
            stack: Vec::new(),
            clips: vec![0],
            images: Vec::new(),
            shadings: Vec::new(),
            alphas: Vec::new(),
            live_text: false,
            stats: ConversionStats::default(),
        }
    }

    /// What the page contained, for callers that report conversion quality.
    pub fn stats(&self) -> &ConversionStats {
        &self.stats
    }

    /// Add this page's content and resources to a document.
    ///
    /// The one entry point a caller above this crate needs to assemble a
    /// multi-page file: it hands back the page object, and the tree over those
    /// objects is the caller's to build.
    pub fn emit_page(&mut self, doc: &mut Document) -> Result<ObjectId> {
        let mut resources = Dictionary::new();
        if !self.images.is_empty() {
            let mut xobjects = Dictionary::new();
            for (index, pending) in self.images.iter().enumerate() {
                let mut painted = image_xobject(pending)?;
                // The mask has to be an indirect object before the image can
                // name it, and the image is not one yet — so the mask goes in
                // first and its number is written back as `/SMask`.
                if let Some(mask) = painted.mask.take() {
                    let mask_id = doc.add_object(mask);
                    painted.image.dict.set("SMask", Object::Reference(mask_id));
                }
                xobjects.set(
                    format!("Im{index}"),
                    Object::Reference(doc.add_object(painted.image)),
                );
            }
            resources.set("XObject", Object::Dictionary(xobjects));
        }
        if !self.shadings.is_empty() {
            let mut patterns = Dictionary::new();
            for (index, shading) in self.shadings.iter().enumerate() {
                let stream = shading_pattern(shading, doc)?;
                patterns.set(
                    format!("Sh{index}"),
                    Object::Reference(doc.add_object(stream)),
                );
            }
            resources.set("Pattern", Object::Dictionary(patterns));
        }
        if !self.alphas.is_empty() {
            let mut states = Dictionary::new();
            for (index, alpha) in self.alphas.iter().enumerate() {
                let mut state = Dictionary::new();
                state.set("Type", Object::Name(b"ExtGState".to_vec()));
                state.set("ca", Object::Real(*alpha as f32));
                state.set("CA", Object::Real(*alpha as f32));
                states.set(format!("GS{index}"), Object::Dictionary(state));
            }
            resources.set("ExtGState", Object::Dictionary(states));
        }
        if self.live_text {
            let mut fonts = Dictionary::new();
            let mut font = Dictionary::new();
            font.set("Type", Object::Name(b"Font".to_vec()));
            font.set("Subtype", Object::Name(b"Type1".to_vec()));
            font.set("BaseFont", Object::Name(b"Helvetica".to_vec()));
            font.set("Encoding", Object::Name(b"WinAnsiEncoding".to_vec()));
            fonts.set("F1", Object::Dictionary(font));
            resources.set("Font", Object::Dictionary(fonts));
        }

        let mut content = Stream::new(
            Dictionary::new(),
            std::mem::take(&mut self.ops).into_bytes(),
        );
        content
            .compress()
            .map_err(|e| convert_error("content stream", e.to_string()))?;
        let content_id = doc.add_object(content);

        let mut page = Dictionary::new();
        page.set("Type", Object::Name(b"Page".to_vec()));
        page.set(
            "MediaBox",
            Object::Array(vec![
                Object::Real(0.0),
                Object::Real(0.0),
                Object::Real((self.page_box.w * PT_PER_MM) as f32),
                Object::Real((self.page_box.h * PT_PER_MM) as f32),
            ]),
        );
        page.set("Contents", Object::Reference(content_id));
        page.set("Resources", Object::Dictionary(resources));
        Ok(doc.add_object(page))
    }

    /// Page millimetres to PDF points, page box origin and y flip included.
    /// `local` acts first, then the current transform, matching how the raster
    /// backend composes the same matrices.
    fn to_pt(&self, local: Affine) -> Affine {
        // Scale first, then translate: composing the other way round would
        // scale the page offset by `PT_PER_MM` a second time.
        let base = Affine::scale(PT_PER_MM, -PT_PER_MM).then(Affine::translate(
            -self.page_box.x * PT_PER_MM,
            (self.page_box.y + self.page_box.h) * PT_PER_MM,
        ));
        local.then(self.ctm).then(base)
    }

    fn op(&mut self, text: String) {
        self.ops.push_str(&text);
        self.ops.push('\n');
    }

    /// Register an ExtGState for a translucent alpha, reusing what exists.
    fn alpha_state(&mut self, alpha: f64) -> String {
        let found = self.alphas.iter().position(|a| (*a - alpha).abs() < 1e-6);
        let index = match found {
            Some(i) => i,
            None => {
                self.alphas.push(alpha);
                self.alphas.len() - 1
            }
        };
        format!("/GS{index} gs")
    }

    /// Emit a path, flattening arcs and elevating curves into cubics.
    fn write_path(&mut self, path: &Path, total: Affine) {
        let mut cur = (0.0, 0.0);
        for seg in &path.segments {
            match *seg {
                PathSegment::MoveTo(x, y) => {
                    cur = (x, y);
                    self.op(point_op(total.apply(x, y), "m"));
                }
                PathSegment::LineTo(x, y) => {
                    cur = (x, y);
                    self.op(point_op(total.apply(x, y), "l"));
                }
                PathSegment::CurveTo { c1, c2, to } => {
                    cur = to;
                    self.op(curve_op(total, c1, c2, to));
                }
                PathSegment::QuadTo { c, to } => {
                    // Elevating a quadratic to a cubic is exact.
                    let c1 = (
                        cur.0 + 2.0 / 3.0 * (c.0 - cur.0),
                        cur.1 + 2.0 / 3.0 * (c.1 - cur.1),
                    );
                    let c2 = (
                        to.0 + 2.0 / 3.0 * (c.0 - to.0),
                        to.1 + 2.0 / 3.0 * (c.1 - to.1),
                    );
                    cur = to;
                    self.op(curve_op(total, c1, c2, to));
                }
                PathSegment::Arc {
                    rx,
                    ry,
                    x_axis_rotation,
                    large_arc,
                    sweep,
                    to,
                } => {
                    for (c1, c2, end) in
                        arc_cubics(rx, ry, x_axis_rotation, large_arc, sweep, cur, to)
                    {
                        self.op(curve_op(total, c1, c2, end));
                    }
                    cur = to;
                }
                PathSegment::Close => self.op("h".to_string()),
            }
        }
    }

    /// Fill a region with a shading, clipped to the path by construction.
    fn fill_shading(&mut self, path: &Path, name: &str) {
        let total = self.to_pt(Affine::IDENTITY);
        let bbox = path.tight_bounding_box();
        let (x0, y0) = total.apply(bbox.x, bbox.y);
        let (x1, y1) = total.apply(bbox.x + bbox.w, bbox.y + bbox.h);
        self.write_path(path, total);
        self.op(format!(
            "W n /Pattern cs /{name} scn {} {} {} {} re f",
            num(x0),
            num(y0),
            num(x1),
            num(y1)
        ));
    }

    /// Outline every glyph into vector paths, reporting what became of them.
    fn outline_text(&mut self, run: &GlyphRun) -> Outlined {
        let Some(fonts) = self.fonts else {
            return Outlined::Dropped;
        };
        let Some(face) = sofd_display::run_face(fonts, run) else {
            return Outlined::Dropped;
        };
        let fill = run.fill.as_ref();
        let mut drawn = 0usize;
        let mut missing = 0usize;
        for (&gid, offset) in run.glyphs.iter().zip(run.glyph_offsets()) {
            let Some((outline, _source)) = fonts.outline_for_code(&face, gid, run.codes_are_glyphs)
            else {
                if fonts.is_missing_code(&face, gid, run.codes_are_glyphs) {
                    missing += 1;
                }
                continue;
            };
            // Outlines are normalized to 1/1000 em with y pointing up: scale
            // to millimetres first, then position, or the origin gets scaled.
            let glyph = run.glyph_matrix(offset);
            let total = self.to_pt(glyph);
            self.write_path(&outline, total);
            let mut setup = String::new();
            if let Some(fill) = fill {
                setup.push_str(&glyph_colour(fill, self));
            }
            // The path is still current after `f` only for a stroke, so a run
            // with both paints writes its outline once and paints it twice.
            if let Some(stroke) = &run.stroke {
                if fill.is_some() {
                    setup.push(' ');
                }
                setup.push_str(&glyph_colour(stroke, self));
                setup.push(' ');
                setup.push_str(&stroke_state(&run.stroke_style));
                setup.push_str(" S");
            } else {
                setup.push_str(" f");
            }
            self.op(setup);
            drawn += 1;
        }
        if missing > 0 {
            self.stats.missing_glyph_runs += 1;
            self.stats.missing_glyphs += missing;
        }
        if fill.is_none() && run.stroke.is_some() && drawn > 0 {
            self.stats.stroke_only_glyph_runs += 1;
        }
        if drawn > 0 {
            self.stats.outlined_glyphs += drawn;
            return Outlined::Drew;
        }
        if missing > 0 {
            return Outlined::Missing;
        }
        // Every code was blank on purpose, so nothing is missing here.
        Outlined::Blank
    }

    /// Emit the run's Unicode text against a standard font.
    fn live_text(&mut self, run: &GlyphRun) {
        let Some(text) = run.text.as_deref().filter(|t| !t.is_empty()) else {
            self.stats.dropped_runs += 1;
            return;
        };
        self.live_text = true;
        self.stats.live_text_runs += 1;
        let mut out = format!("BT /F1 {} Tf", num(run.size.value() * PT_PER_MM));
        let mut offset = 0.0;
        for (i, ch) in text.chars().enumerate() {
            if i > 0 {
                offset += run.advances.get(i - 1).copied().unwrap_or(0.0);
            }
            offset += run.letter_spacing;
            let local = run.transform.then(Affine::translate(offset, 0.0));
            let (e, f) = self.to_pt(local).apply(0.0, 0.0);
            let composed = self.ctm.then(local);
            let _ = write!(
                out,
                " {} {} {} {} {} {} Tm ({}) Tj",
                num(composed.a),
                num(-composed.b),
                num(-composed.c),
                num(composed.d),
                num(e),
                num(f),
                pdf_string(ch.encode_utf8(&mut [0u8; 4]))
            );
        }
        out.push_str(" ET");
        self.op(out);
    }
}

impl PageSink for PdfSink<'_> {
    fn save(&mut self) {
        self.stack.push(self.ctm);
        self.clips.push(0);
    }

    fn restore(&mut self) {
        self.ctm = self.stack.pop().unwrap_or(Affine::IDENTITY);
        let open = if self.clips.len() > 1 {
            self.clips.pop().unwrap_or(0)
        } else {
            0
        };
        for _ in 0..open {
            self.op("Q".to_string());
        }
    }

    fn transform(&mut self, matrix: Affine) {
        self.ctm = self.ctm.then(matrix);
    }

    fn clip(&mut self, path: &Path, rule: PaintRule) {
        let total = self.to_pt(Affine::IDENTITY);
        self.write_path(path, total);
        // `W n` ends the path but leaves the clip in force until `Q`.
        let operator = match rule {
            PaintRule::NonZero => "W n",
            PaintRule::EvenOdd => "W* n",
        };
        self.op(format!("q {operator}"));
        if let Some(top) = self.clips.last_mut() {
            *top += 1;
        }
    }

    fn clip_text(&mut self, run: &GlyphRun) {
        let Some(fonts) = self.fonts else {
            return;
        };
        let Some(face) = sofd_display::run_face(fonts, run) else {
            return;
        };
        // Every glyph outline goes into one path, joined implicitly, so the
        // glyphs intersect the clip as their union rather than one at a time.
        let mut wrote = false;
        let mut offset = 0.0;
        for (i, &gid) in run.glyphs.iter().enumerate() {
            if i > 0 {
                offset += run.advances.get(i - 1).copied().unwrap_or(0.0);
            }
            offset += run.letter_spacing;
            let Some(outline) = face.outline_for_run_code(gid, run.codes_are_glyphs) else {
                continue;
            };
            let glyph = run.glyph_matrix(offset);
            self.write_path(&outline, self.to_pt(glyph));
            wrote = true;
        }
        if !wrote {
            return;
        }
        // `W n` ends the path but leaves the clip in force until `Q`.
        self.op("q W n".to_string());
        if let Some(top) = self.clips.last_mut() {
            *top += 1;
        }
    }

    fn path(&mut self, shape: PathShape<'_>) {
        let total = self.to_pt(Affine::IDENTITY);
        let mut setup = String::new();
        let mut operator = String::new();
        // A gradient carries its own axis, so it is registered into the
        // document the moment it is painted: no earlier op has to be matched.
        let gradient = match shape.fill {
            Some(PaintShape::Linear(spec)) => Some(self.register_linear(spec, total)),
            Some(PaintShape::Radial(spec)) => Some(self.register_radial(spec, total)),
            Some(PaintShape::Mesh(spec)) => Some(self.register_mesh(spec, total)),
            _ => None,
        };
        let mut shaded = false;
        if let Some(index) = gradient {
            // A shading paints its own region, so a stroke has to follow it
            // rather than share one painting operator.
            self.fill_shading(shape.path, &format!("Sh{index}"));
            shaded = true;
            if shape.stroke.is_none() {
                return;
            }
        } else {
            match shape.fill {
                None => {}
                Some(PaintShape::Solid(c)) => {
                    let _ = write!(setup, "{} rg", rgb(c));
                    if c.a != 255 {
                        setup.push(' ');
                        setup.push_str(&self.alpha_state(c.a as f64 / 255.0));
                    }
                    operator.push_str(match shape.rule {
                        PaintRule::NonZero => " f",
                        PaintRule::EvenOdd => " f*",
                    });
                }
                // A tiling pattern is not written yet; the cell's first fill
                // keeps the region visible, and the stand-in is counted rather
                // than passed off as the document's own paint.
                Some(PaintShape::Pattern(spec)) => {
                    self.stats.paints_approximated += 1;
                    let _ = write!(setup, "{} rg", pattern_rgb(spec));
                    operator.push_str(" f");
                }
                Some(_) => {
                    setup.push_str("0 0 0 rg");
                    operator.push_str(" f");
                }
            }
        }
        if shape.stroke.is_some() {
            setup.push(' ');
            setup.push_str(&stroke_prefix(shape.stroke_style, &self.ctm));
            if let Some(paint) = shape.stroke {
                match paint {
                    PaintShape::Solid(c) => {
                        let _ = write!(setup, " {} RG", rgb(c));
                        if c.a != 255 {
                            setup.push(' ');
                            setup.push_str(&self.alpha_state(c.a as f64 / 255.0));
                        }
                    }
                    _ => setup.push_str(" 0 0 0 RG"),
                }
            }
            operator = if shaded {
                " S".to_string()
            } else if shape.fill.is_some() {
                match shape.rule {
                    PaintRule::NonZero => " B".to_string(),
                    PaintRule::EvenOdd => " B*".to_string(),
                }
            } else {
                " S".to_string()
            };
        }
        if operator.trim().is_empty() {
            return;
        }
        self.write_path(shape.path, total);
        self.op(format!("{setup}{operator}"));
    }

    fn text(&mut self, run: &GlyphRun) {
        if run.glyphs.is_empty() {
            return;
        }
        match self.text {
            TextExport::Outline => match self.outline_text(run) {
                Outlined::Drew | Outlined::Blank => {}
                // With no face to outline, the run's own text is still worth
                // writing when it spells characters rather than glyph ids.
                Outlined::Dropped => self.live_text(run),
                // A face was there and could not draw these codes, so its text
                // is no substitute: a viewer would set glyph ids as characters
                // and put marks on the page that never were.
                Outlined::Missing => self.stats.dropped_runs += 1,
            },
            TextExport::LiveText => self.live_text(run),
        }
    }

    fn image(&mut self, image: &ImageRef, dst: Rect) {
        if image.data.is_empty() {
            return;
        }
        // `Do` paints across the unit square, so the rect is the placement:
        // dividing by the pixel count too would shrink a 159-px seal to a dot.
        let placement = Affine::new(dst.w, 0.0, 0.0, dst.h, dst.x, dst.y);
        let total = self.to_pt(placement);
        let index = self.images.len();
        self.images.push(PendingImage {
            data: image.data.clone(),
            pixels: image.pixel_size,
            codec: image.codec,
        });
        self.stats.images += 1;
        self.op(format!(
            "q {} {} {} {} {} {} cm /Im{index} Do Q",
            num(total.a),
            num(total.b),
            num(total.c),
            num(total.d),
            num(total.e),
            num(total.f)
        ));
    }
}

impl PdfSink<'_> {
    /// Register a linear shading and return the name `Sh{index}` it gets.
    fn register_linear(&mut self, spec: &LinearGradientSpec, total: Affine) -> String {
        let (x0, y0) = total.apply(spec.start.x, spec.start.y);
        let (x1, y1) = total.apply(spec.end.x, spec.end.y);
        self.stats.gradients += 1;
        let index = self.shadings.len();
        self.shadings.push(PendingShading {
            radial: false,
            coords: vec![x0, y0, x1, y1],
            stops: spec.stops.clone(),
            mesh: None,
        });
        format!("Sh{index}")
    }

    /// Register the clause's two-circle cone as a radial shading. PDF's radial
    /// shading is that same cone, so the geometry travels as written.
    fn register_radial(&mut self, spec: &RadialGradientSpec, total: Affine) -> String {
        let (cx, cy) = total.apply(spec.center.x, spec.center.y);
        let (ex, ey) = total.apply(spec.end.x, spec.end.y);
        // The page transform is axis preserving up to the y flip, so a radius
        // scales with the mean factor.
        let scale = total.scale_factor();
        self.stats.gradients += 1;
        let index = self.shadings.len();
        self.shadings.push(PendingShading {
            radial: true,
            coords: vec![cx, cy, spec.radius * scale, ex, ey, spec.end_radius * scale],
            stops: spec.stops.clone(),
            mesh: None,
        });
        format!("Sh{index}")
    }

    /// Register a Gouraud mesh as a PDF free-form triangle shading. Each
    /// vertex keeps its own colour, so the triangles interpolate exactly as
    /// the raster backend does, rather than collapsing to one mean colour.
    fn register_mesh(&mut self, spec: &MeshSpec, total: Affine) -> String {
        self.stats.gradients += 1;
        let vertices: Vec<(f64, f64, u8, u8, u8)> = spec
            .points
            .iter()
            .map(|v| {
                let (x, y) = total.apply(v.x, v.y);
                (x, y, v.color.r, v.color.g, v.color.b)
            })
            .collect();
        let triangles: Vec<[usize; 3]> = spec
            .triangles
            .iter()
            .filter(|t: &&[usize; 3]| t.iter().all(|i| *i < vertices.len()))
            .copied()
            .collect();
        let index = self.shadings.len();
        self.shadings.push(PendingShading {
            radial: false,
            coords: Vec::new(),
            stops: Vec::new(),
            mesh: Some(MeshShading {
                vertices,
                triangles,
            }),
        });
        format!("Sh{index}")
    }
}

/// Convert one page into a single-page PDF document.
pub fn page_to_pdf(page: &sofd_display::PageDisplay, options: &PdfOptions<'_>) -> Result<Vec<u8>> {
    Ok(page_to_pdf_with_stats(page, options)?.0)
}

/// The same conversion, with what it did to the page. A PDF that dropped a
/// run for want of a font is still a valid PDF, so the counts are the only
/// way a caller learns the page is not the document it came from.
pub fn page_to_pdf_with_stats(
    page: &sofd_display::PageDisplay,
    options: &PdfOptions<'_>,
) -> Result<(Vec<u8>, crate::ConversionStats)> {
    let mut sink = PdfSink::new(page.page_box, options);
    replay(page, &mut sink)?;
    let stats = sink.stats.clone();
    let mut doc = Document::new();
    let page_id = sink.emit_page(&mut doc)?;
    Ok((finish_one_page_document(&mut doc, page_id)?, stats))
}

/// The colour operator for `paint`, with an alpha state when it is
/// translucent. A paint with no colour operator here falls back to black.
fn glyph_colour(paint: &sofd_display::Paint, sink: &mut PdfSink<'_>) -> String {
    match crate::replay::text_paint_shape(paint) {
        PaintShape::Solid(c) => {
            let mut s = format!("{} rg", rgb(c));
            if c.a != 255 {
                s.push(' ');
                s.push_str(&sink.alpha_state(c.a as f64 / 255.0));
            }
            s
        }
        _ => "0 0 0 rg".to_string(),
    }
}

/// `w` and `J`/`j`/`M` operands for an outlined run, in points.
fn stroke_state(style: &StrokeStyle) -> String {
    let cap = match style.cap {
        LineCap::Butt => 0,
        LineCap::Round => 1,
        LineCap::Square => 2,
    };
    let join = match style.join {
        LineJoin::Miter => 0,
        LineJoin::Round => 1,
        LineJoin::Bevel => 2,
    };
    let mut s = format!(
        "{} w {cap} J {join} j {} M",
        num(style.width.max(0.0) * PT_PER_MM),
        num(style.miter_limit)
    );
    if !style.dash.is_empty() {
        let pattern: Vec<String> = style.dash.iter().map(|d| num(*d * PT_PER_MM)).collect();
        let _ = write!(
            s,
            " [{}] {} d",
            pattern.join(" "),
            num(style.dash_offset * PT_PER_MM)
        );
    }
    s
}

fn finish_one_page_document(doc: &mut Document, page_id: ObjectId) -> Result<Vec<u8>> {
    let mut pages = Dictionary::new();
    pages.set("Type", Object::Name(b"Pages".to_vec()));
    pages.set("Kids", Object::Array(vec![Object::Reference(page_id)]));
    pages.set("Count", 1i64);
    let pages_id = doc.add_object(pages);
    if let Ok(dict) = doc.get_dictionary_mut(page_id) {
        dict.set("Parent", Object::Reference(pages_id));
    }
    let mut catalog = Dictionary::new();
    catalog.set("Type", Object::Name(b"Catalog".to_vec()));
    catalog.set("Pages", Object::Reference(pages_id));
    let catalog_id = doc.add_object(catalog);
    doc.trailer.set("Root", Object::Reference(catalog_id));

    let mut out = Vec::new();
    doc.save_to(&mut out)
        .map_err(|e| convert_error("page", e.to_string()))?;
    Ok(out)
}

/// Build the shading pattern that paints one gradient.
fn shading_pattern(shading: &PendingShading, doc: &mut Document) -> Result<Stream> {
    if let Some(mesh) = &shading.mesh {
        return mesh_pattern(mesh);
    }
    let mut dict = Dictionary::new();
    dict.set("PatternType", Object::Integer(2));
    let mut inner = Dictionary::new();
    inner.set(
        "ShadingType",
        Object::Integer(if shading.radial { 3 } else { 2 }),
    );
    inner.set("ColorSpace", Object::Name(b"DeviceRGB".to_vec()));
    inner.set(
        "Coords",
        Object::Array(
            shading
                .coords
                .iter()
                .map(|c| Object::Real(*c as f32))
                .collect(),
        ),
    );
    inner.set(
        "Extend",
        Object::Array(vec![Object::Boolean(true), Object::Boolean(true)]),
    );
    inner.set("Function", Object::Reference(stops_function(shading, doc)?));
    dict.set("Shading", Object::Dictionary(inner));
    Ok(Stream::new(dict, Vec::new()))
}

/// Build a shading pattern that paints one Gouraud mesh. PDF's `ShadingType 4`
/// is a free-form triangle mesh with a colour per vertex, which is exactly the
/// clause's model, so every triangle is written as one shading-4 element.
fn mesh_pattern(mesh: &MeshShading) -> Result<Stream> {
    let mut data: Vec<u8> = Vec::new();
    for tri in &mesh.triangles {
        write_mesh_triangle(&mut data, mesh, tri);
    }
    let mut inner = Dictionary::new();
    inner.set("ShadingType", Object::Integer(4));
    inner.set("ColorSpace", Object::Name(b"DeviceRGB".to_vec()));
    inner.set("BitsPerCoordinate", Object::Integer(16));
    inner.set("BitsPerComponent", Object::Integer(8));
    inner.set("BitsPerFlag", Object::Integer(8));
    let xs: Vec<f64> = mesh.vertices.iter().map(|v| v.0).collect();
    let ys: Vec<f64> = mesh.vertices.iter().map(|v| v.1).collect();
    let (min_x, max_x) = extent(&xs);
    let (min_y, max_y) = extent(&ys);
    inner.set(
        "Decode",
        Object::Array(vec![
            Object::Real(min_x as f32),
            Object::Real(max_x as f32),
            Object::Real(min_y as f32),
            Object::Real(max_y as f32),
            Object::Real(0.0),
            Object::Real(1.0),
            Object::Real(0.0),
            Object::Real(1.0),
            Object::Real(0.0),
            Object::Real(1.0),
        ]),
    );
    let mut dict = Dictionary::new();
    dict.set("PatternType", Object::Integer(2));
    dict.set("Shading", Object::Dictionary(inner));
    Ok(Stream::new(dict, data))
}

/// One triangle as a shading-4 element: flag 0, then each vertex as
/// 16-bit x, 16-bit y and three 8-bit colour channels, all big-endian bytes.
fn write_mesh_triangle(out: &mut Vec<u8>, mesh: &MeshShading, tri: &[usize; 3]) {
    for (position, index) in tri.iter().enumerate() {
        let (x, y, r, g, b) = mesh.vertices[*index];
        let flag = if position == 0 { 0u8 } else { 2u8 };
        out.push(flag);
        out.extend_from_slice(&quantise(x).to_be_bytes());
        out.extend_from_slice(&quantise(y).to_be_bytes());
        out.extend_from_slice(&[r, g, b]);
    }
}

/// A coordinate in the 16-bit range PDF's `Decode` maps onto the axis.
fn quantise(value: f64) -> u16 {
    let clamped = value.clamp(0.0, 1.0);
    (clamped * 65535.0).round() as u16
}

/// The smallest and largest of a coordinate list, defaulting to the unit range
/// for an empty mesh so the `Decode` array stays well-formed.
fn extent(values: &[f64]) -> (f64, f64) {
    let mut min = f64::INFINITY;
    let mut max = f64::NEG_INFINITY;
    for v in values {
        min = min.min(*v);
        max = max.max(*v);
    }
    if min.is_finite() && max.is_finite() && min < max {
        (min, max)
    } else {
        (0.0, 1.0)
    }
}

/// Build the colour function of a gradient: one exponential interval per pair
/// of stops, stitched together so piecewise linear interpolation is exact.
fn stops_function(shading: &PendingShading, doc: &mut Document) -> Result<ObjectId> {
    let stops = &shading.stops;
    let intervals: Vec<ObjectId> = stops
        .windows(2)
        .map(|pair| {
            let mut interval = Dictionary::new();
            interval.set("FunctionType", Object::Integer(2));
            interval.set(
                "Domain",
                Object::Array(vec![Object::Real(0.0), Object::Real(1.0)]),
            );
            interval.set("C0", channel_array(&pair[0].color));
            interval.set("C1", channel_array(&pair[1].color));
            interval.set("N", Object::Integer(1));
            doc.add_object(interval)
        })
        .collect();
    if intervals.len() == 1 {
        return Ok(intervals[0]);
    }
    // A lone stop degenerates to a constant colour; two or more stops stitch
    // their intervals at the stop offsets between them.
    let (first, last) = match (stops.first(), stops.last()) {
        (Some(first), Some(last)) => (first.color, last.color),
        _ => {
            return Err(convert_error(
                "gradient",
                "a gradient needs at least one stop".into(),
            ));
        }
    };
    if intervals.is_empty() {
        let mut constant = Dictionary::new();
        constant.set("FunctionType", Object::Integer(2));
        constant.set(
            "Domain",
            Object::Array(vec![Object::Real(0.0), Object::Real(1.0)]),
        );
        constant.set("C0", channel_array(&first));
        constant.set("C1", channel_array(&last));
        constant.set("N", Object::Integer(1));
        return Ok(doc.add_object(constant));
    }
    let mut stitched = Dictionary::new();
    stitched.set("FunctionType", Object::Integer(3));
    stitched.set(
        "Domain",
        Object::Array(vec![Object::Real(0.0), Object::Real(1.0)]),
    );
    stitched.set(
        "Functions",
        Object::Array(intervals.iter().copied().map(Object::Reference).collect()),
    );
    let bounds: Vec<Object> = stops[1..stops.len() - 1]
        .iter()
        .map(|s| Object::Real(s.offset.clamp(0.0, 1.0) as f32))
        .collect();
    stitched.set("Bounds", Object::Array(bounds));
    let encode: Vec<Object> = (0..intervals.len())
        .flat_map(|_| [Object::Real(0.0), Object::Real(1.0)])
        .collect();
    stitched.set("Encode", Object::Array(encode));
    Ok(doc.add_object(stitched))
}

/// Channel triplet of a colour, for gradient function dictionaries.
mod content;

use content::{
    channel_array, convert_error, curve_op, image_xobject, num, pattern_rgb, pdf_string, point_op,
    rgb, stroke_prefix,
};

#[cfg(test)]
mod tests {
    use super::*;
    use sofd_common::{Millimeter, Point};
    use sofd_display::MeshVertex;
    use sofd_display::ops::Paint;
    use sofd_font::FontId;

    /// A real face, so outlining runs for real rather than degrading.
    fn font_bytes() -> Vec<u8> {
        std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../sofd-render/tests/data/Orbitron-Bold.ttf"
        ))
        .expect("font fixture")
    }

    fn rect(x: f64, y: f64, w: f64, h: f64) -> Path {
        Path::build(|b| {
            b.rect(x, y, w, h);
        })
    }

    fn page(ops: Vec<sofd_display::DrawOp>) -> sofd_display::PageDisplay {
        sofd_display::PageDisplay {
            index: 0,
            page_box: Rect::A4_PORTRAIT,
            ops,
            content_bbox: Rect::EMPTY,
        }
    }

    fn content_of(bytes: &[u8]) -> String {
        let doc = Document::load_from(bytes).expect("the pdf we just wrote must parse");
        let pages = doc.get_pages();
        let page_id = *pages.values().next().expect("one page");
        let content = doc.get_page_content(page_id);
        String::from_utf8(content).expect("utf8 content")
    }

    #[test]
    fn a_rect_becomes_a_fill_operator_in_points() {
        // 10mm at 72/25.4 points per millimetre is 28.3465pt.
        let p = page(vec![sofd_display::DrawOp::Path {
            path: rect(10.0, 10.0, 10.0, 10.0),
            fill: Some(Paint::Solid(Color::rgba(255, 0, 0, 255))),
            stroke: None,
            stroke_style: StrokeStyle::default(),
            rule: PaintRule::NonZero,
        }]);
        let pdf = page_to_pdf(&p, &PdfOptions::default()).expect("convert");
        assert!(pdf.starts_with(b"%PDF-"));
        let content = content_of(&pdf);
        assert!(content.contains("1 0 0 rg"));
        // x runs 10..30mm; y is measured from the bottom, so 10..20mm from the
        // top of a 297mm page lands at 756.8504..813.5433 points.
        assert!(content.contains("28.3465 813.5433 m"));
        assert!(content.contains("56.6929 813.5433 l"));
        assert!(content.trim_end().ends_with("f"));
    }

    #[test]
    fn the_y_axis_flips_into_pdf_space() {
        // A rect at the top of a 297mm page lands near the top in points.
        let p = page(vec![sofd_display::DrawOp::Path {
            path: rect(0.0, 0.0, 10.0, 10.0),
            fill: Some(Paint::Solid(Color::BLACK)),
            stroke: None,
            stroke_style: StrokeStyle::default(),
            rule: PaintRule::NonZero,
        }]);
        let content = content_of(&page_to_pdf(&p, &PdfOptions::default()).expect("convert"));
        assert!(
            content.contains("0 841.8898 m"),
            "the origin maps to the top"
        );
    }

    #[test]
    fn the_even_odd_rule_selects_the_starred_fill() {
        let p = page(vec![sofd_display::DrawOp::Path {
            path: rect(0.0, 0.0, 10.0, 10.0),
            fill: Some(Paint::Solid(Color::BLACK)),
            stroke: None,
            stroke_style: StrokeStyle::default(),
            rule: PaintRule::EvenOdd,
        }]);
        let content = content_of(&page_to_pdf(&p, &PdfOptions::default()).expect("convert"));
        assert!(content.contains("f*"));
    }

    #[test]
    fn a_stroke_carries_width_dash_and_joins() {
        let style = StrokeStyle {
            width: 0.5,
            dash: vec![3.0, 1.0],
            dash_offset: 0.25,
            cap: sofd_display::ops::LineCap::Round,
            join: sofd_display::ops::LineJoin::Bevel,
            miter_limit: 2.0,
        };
        let p = page(vec![sofd_display::DrawOp::Path {
            path: rect(0.0, 0.0, 10.0, 10.0),
            fill: None,
            stroke: Some(Paint::Solid(Color::BLACK)),
            stroke_style: style,
            rule: PaintRule::NonZero,
        }]);
        let content = content_of(&page_to_pdf(&p, &PdfOptions::default()).expect("convert"));
        assert!(content.contains("1.4173 w"));
        assert!(content.contains("[8.5039 2.8346] 0.7087 d"));
        assert!(content.contains("1 J 2 j 2 M"));
        assert!(content.trim_end().ends_with("S"));
    }

    #[test]
    fn a_transform_scales_geometry_and_stroke_together() {
        let p = page(vec![
            sofd_display::DrawOp::Save,
            sofd_display::DrawOp::Transform(Affine::scale(2.0, 2.0)),
            sofd_display::DrawOp::Path {
                path: rect(0.0, 0.0, 10.0, 10.0),
                fill: None,
                stroke: Some(Paint::Solid(Color::BLACK)),
                stroke_style: StrokeStyle::default(),
                rule: PaintRule::NonZero,
            },
            sofd_display::DrawOp::Restore,
        ]);
        let content = content_of(&page_to_pdf(&p, &PdfOptions::default()).expect("convert"));
        // The rect doubles to 56.69pt along the top edge and the 1mm stroke to
        // 5.6693pt.
        assert!(content.contains("56.6929 841.8898 l"));
        assert!(content.contains("5.6693 w"));
    }

    #[test]
    fn a_clip_opens_a_q_group_the_restore_closes() {
        let p = page(vec![
            sofd_display::DrawOp::Save,
            sofd_display::DrawOp::ClipPath {
                path: rect(0.0, 0.0, 10.0, 10.0),
                rule: PaintRule::NonZero,
            },
            sofd_display::DrawOp::Restore,
        ]);
        let content = content_of(&page_to_pdf(&p, &PdfOptions::default()).expect("convert"));
        assert!(content.contains("q "), "the clip opens a group");
        assert!(content.contains("W n"), "the path becomes a clip");
        assert!(content.contains("Q"), "the restore closes the group");
    }

    #[test]
    fn an_even_odd_clip_uses_the_starred_operator() {
        let p = page(vec![
            sofd_display::DrawOp::Save,
            sofd_display::DrawOp::ClipPath {
                path: rect(0.0, 0.0, 10.0, 10.0),
                rule: PaintRule::EvenOdd,
            },
            sofd_display::DrawOp::Restore,
        ]);
        let content = content_of(&page_to_pdf(&p, &PdfOptions::default()).expect("convert"));
        assert!(content.contains("W* n"));
    }

    #[test]
    fn a_linear_gradient_becomes_a_shading_pattern() {
        let p = page(vec![sofd_display::DrawOp::Path {
            path: rect(0.0, 0.0, 10.0, 10.0),
            fill: Some(sofd_display::ops::Paint::LinearGradient(Box::new(
                LinearGradientSpec {
                    start: Point::new(0.0, 0.0),
                    end: Point::new(10.0, 0.0),
                    stops: vec![
                        GradientStop {
                            offset: 0.0,
                            color: Color::WHITE,
                        },
                        GradientStop {
                            offset: 1.0,
                            color: Color::BLACK,
                        },
                    ],
                },
            ))),
            stroke: None,
            stroke_style: Default::default(),
            rule: PaintRule::NonZero,
        }]);
        let bytes = page_to_pdf(&p, &PdfOptions::default()).expect("convert");
        let doc = Document::load_from(&bytes[..]).expect("parse");
        let page_id = *doc.get_pages().values().next().expect("page");
        let resources = doc.get_page_resources(page_id).expect("resources");
        let patterns = resources
            .0
            .expect("pattern dict")
            .get(b"Pattern")
            .expect("patterns");
        assert!(
            patterns.as_dict().is_ok(),
            "a shading pattern is registered"
        );
        let content = content_of(&bytes);
        assert!(content.contains("/Pattern cs"));
        assert!(content.contains("scn"));
        assert!(content.contains("re f"));
    }

    #[test]
    fn a_pattern_is_painted_as_its_first_colour_and_counted() {
        // A tiling pattern needs a PDF tiling pattern built from the cell. Until
        // one is written, the cell's first fill keeps the region visible, and
        // the stand-in is counted so a caller can say the export simplified it.
        let cell = sofd_display::PageDisplay {
            index: 0,
            page_box: Rect::new(0.0, 0.0, 4.0, 4.0),
            ops: vec![sofd_display::DrawOp::Path {
                path: rect(0.0, 0.0, 4.0, 4.0),
                fill: Some(sofd_display::ops::Paint::Solid(Color::rgba(255, 0, 0, 255))),
                stroke: None,
                stroke_style: Default::default(),
                rule: PaintRule::NonZero,
            }],
            content_bbox: Rect::EMPTY,
        };
        let p = page(vec![sofd_display::DrawOp::Path {
            path: rect(0.0, 0.0, 20.0, 20.0),
            fill: Some(sofd_display::ops::Paint::Pattern(Box::new(
                sofd_display::PatternSpec {
                    cell: Box::new(cell),
                    width: 4.0,
                    height: 4.0,
                    step_x: 4.0,
                    step_y: 4.0,
                    reflect: sofd_display::Reflect::Normal,
                },
            ))),
            stroke: None,
            stroke_style: Default::default(),
            rule: PaintRule::NonZero,
        }]);
        let (bytes, stats) = page_to_pdf_with_stats(&p, &PdfOptions::default()).expect("convert");
        let content = content_of(&bytes);
        assert!(
            content.contains("1 0 0 rg"),
            "the first colour fills: {content}"
        );
        assert_eq!(stats.paints_approximated, 1, "the stand-in is counted");
        assert_eq!(stats.gradients, 0, "a pattern is not a gradient");
    }

    #[test]
    fn a_radial_gradient_uses_shading_type_three() {
        let p = page(vec![sofd_display::DrawOp::Path {
            path: rect(0.0, 0.0, 10.0, 10.0),
            fill: Some(sofd_display::ops::Paint::RadialGradient(Box::new(
                RadialGradientSpec {
                    center: Point::new(5.0, 5.0),
                    radius: 1.0,
                    end: Point::new(8.0, 5.0),
                    end_radius: 5.0,
                    stops: vec![GradientStop {
                        offset: 0.0,
                        color: Color::WHITE,
                    }],
                },
            ))),
            stroke: None,
            stroke_style: Default::default(),
            rule: PaintRule::NonZero,
        }]);
        let bytes = page_to_pdf(&p, &PdfOptions::default()).expect("convert");
        let doc = Document::load_from(&bytes[..]).expect("parse");
        // The shading dictionary sits inside the pattern stream, so look one
        // level down rather than at the top of every object.
        let found = doc.objects.values().any(|o| {
            let dict = match o {
                Object::Stream(stream) => Some(&stream.dict),
                Object::Dictionary(dict) => Some(dict),
                _ => None,
            };
            dict.and_then(|d| d.get(b"Shading").ok())
                .and_then(|s| s.as_dict().ok())
                .and_then(|d| d.get(b"ShadingType").ok())
                .and_then(|t| t.as_i64().ok())
                == Some(3)
        });
        assert!(found, "a radial shading is type 3");
        // Both circles travel: PDF's type 3 is the cone the clause describes,
        // so the two centres and both radii reach the shading as written.
        let coords = doc.objects.values().find_map(|o| match o {
            Object::Stream(stream) => stream
                .dict
                .get(b"Shading")
                .ok()
                .and_then(|s| s.as_dict().ok())
                .and_then(|d| d.get(b"Coords").ok())
                .and_then(|c| c.as_array().ok())
                .map(|a| a.iter().filter_map(|v| v.as_f32().ok()).collect::<Vec<_>>()),
            _ => None,
        });
        let coords = coords.expect("the shading carries its two circles");
        assert_eq!(coords.len(), 6, "two circles, each a centre and a radius");
        let pt = (72.0 / 25.4) as f32;
        assert!((coords[2] - pt).abs() < 1e-3, "start radius: {:?}", coords);
        assert!(
            (coords[5] - 5.0 * pt).abs() < 1e-3,
            "end radius: {:?}",
            coords
        );
        assert!(
            coords[0] < coords[3],
            "the centres stay apart: {:?}",
            coords
        );
    }

    #[test]
    fn a_gouraud_mesh_becomes_a_type_four_shading() {
        // PDF's ShadingType 4 is a free-form triangle mesh with a colour at
        // every vertex, which is the clause's model, so a mesh reaches the
        // file with its vertices rather than collapsing to one mean colour.
        let p = page(vec![sofd_display::DrawOp::Path {
            path: rect(0.0, 0.0, 10.0, 10.0),
            fill: Some(sofd_display::ops::Paint::Mesh(Box::new(MeshSpec {
                points: vec![
                    MeshVertex {
                        x: 0.0,
                        y: 0.0,
                        color: Color::rgba(255, 0, 0, 255),
                    },
                    MeshVertex {
                        x: 10.0,
                        y: 0.0,
                        color: Color::rgba(0, 0, 255, 255),
                    },
                    MeshVertex {
                        x: 0.0,
                        y: 10.0,
                        color: Color::rgba(0, 255, 0, 255),
                    },
                ],
                triangles: vec![[0, 1, 2]],
            }))),
            stroke: None,
            stroke_style: Default::default(),
            rule: PaintRule::NonZero,
        }]);
        let bytes = page_to_pdf(&p, &PdfOptions::default()).expect("convert");
        let doc = Document::load_from(&bytes[..]).expect("parse");
        let shading = doc.objects.values().find_map(|o| match o {
            Object::Stream(stream) => stream
                .dict
                .get(b"Shading")
                .ok()
                .and_then(|s| s.as_dict().ok())
                .cloned(),
            _ => None,
        });
        let shading = shading.expect("the pattern carries a shading");
        let kind = shading
            .get(b"ShadingType")
            .ok()
            .and_then(|t| t.as_i64().ok());
        assert_eq!(kind, Some(4), "a mesh is a free-form triangle shading");
        // The three vertex colours travel: red, blue and green, in order.
        let data = doc.objects.values().find_map(|o| match o {
            Object::Stream(stream) if stream.dict.get(b"Shading").is_ok() => {
                Some(stream.content.clone())
            }
            _ => None,
        });
        let data = data.expect("the shading stream has content");
        // The stream is the raw bytes the format asks for: red, blue and
        // green, one per vertex, each a triple.
        let has = |tri: [u8; 3]| data.windows(3).any(|w| w == tri);
        assert!(has([0xff, 0x00, 0x00]), "the first vertex keeps its red");
        assert!(has([0x00, 0x00, 0xff]), "the second vertex keeps its blue");
        assert!(has([0x00, 0xff, 0x00]), "the third vertex keeps its green");
    }

    #[test]
    fn a_multi_stop_gradient_stitches_its_intervals() {
        let p = page(vec![sofd_display::DrawOp::Path {
            path: rect(0.0, 0.0, 10.0, 10.0),
            fill: Some(sofd_display::ops::Paint::LinearGradient(Box::new(
                LinearGradientSpec {
                    start: Point::new(0.0, 0.0),
                    end: Point::new(10.0, 0.0),
                    stops: vec![
                        GradientStop {
                            offset: 0.0,
                            color: Color::WHITE,
                        },
                        GradientStop {
                            offset: 0.5,
                            color: Color::rgba(255, 0, 0, 255),
                        },
                        GradientStop {
                            offset: 1.0,
                            color: Color::BLACK,
                        },
                    ],
                },
            ))),
            stroke: None,
            stroke_style: Default::default(),
            rule: PaintRule::NonZero,
        }]);
        let bytes = page_to_pdf(&p, &PdfOptions::default()).expect("convert");
        let doc = Document::load_from(&bytes[..]).expect("parse");
        let stitched = doc.objects.values().any(|o| {
            o.as_dict()
                .ok()
                .and_then(|d| d.get(b"FunctionType").ok())
                .and_then(|t| t.as_i64().ok())
                == Some(3)
        });
        assert!(stitched, "three stops stitch two intervals");
    }

    /// A 100x100 QR code as an embedded JBIG2 stream: the encoding a scanned
    /// invoice carries, which `image` cannot read. Spelled out as bytes so the
    /// test needs no encoder of its own and no fixture outside the crate.
    const QR_JBIG2: &[u8] = &[
        151, 74, 66, 50, 13, 10, 26, 10, 1, 0, 0, 0, 1, 0, 0, 0, 0, 48, 0, 1, 0, 0, 0, 19, 0, 0, 0,
        100, 0, 0, 0, 100, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1, 38, 0, 1, 0, 0, 1, 57, 0,
        0, 0, 100, 0, 0, 0, 100, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8, 3, 255, 253, 255, 2, 254, 254, 254,
        4, 40, 191, 174, 189, 134, 171, 6, 215, 182, 1, 249, 238, 232, 235, 4, 204, 6, 130, 37, 10,
        182, 77, 3, 245, 98, 87, 2, 254, 86, 139, 187, 66, 236, 122, 204, 117, 189, 217, 54, 21,
        169, 173, 42, 178, 128, 197, 216, 20, 219, 121, 130, 14, 98, 9, 144, 181, 233, 16, 85, 223,
        68, 18, 216, 41, 109, 137, 37, 76, 36, 148, 110, 93, 155, 243, 164, 108, 208, 105, 107, 2,
        195, 242, 185, 115, 54, 185, 146, 48, 189, 120, 95, 84, 6, 177, 121, 95, 145, 203, 129,
        254, 155, 101, 188, 30, 204, 212, 119, 62, 84, 118, 154, 193, 73, 255, 90, 180, 122, 97,
        52, 191, 105, 101, 115, 219, 172, 186, 204, 65, 120, 24, 48, 200, 1, 199, 107, 203, 156,
        118, 216, 69, 144, 2, 158, 236, 196, 91, 247, 56, 228, 135, 246, 202, 29, 115, 168, 151,
        197, 177, 152, 165, 30, 150, 114, 8, 42, 84, 19, 94, 150, 56, 246, 131, 15, 102, 57, 26,
        24, 168, 211, 147, 184, 94, 245, 84, 92, 189, 192, 95, 125, 216, 184, 11, 177, 216, 204,
        12, 154, 241, 124, 234, 55, 44, 90, 253, 120, 167, 221, 158, 187, 168, 217, 25, 72, 146,
        90, 194, 149, 37, 2, 43, 230, 196, 2, 16, 105, 110, 63, 168, 76, 175, 112, 34, 165, 76, 79,
        112, 84, 74, 136, 158, 166, 235, 30, 115, 184, 164, 2, 252, 54, 166, 59, 102, 4, 127, 33,
        159, 11, 31, 248, 71, 62, 86, 98, 223, 249, 173, 215, 167, 187, 39, 119, 44, 133, 34, 171,
        132, 153, 131, 206, 128, 64, 99, 226, 63, 255, 172, 0, 0, 0, 2, 49, 0, 1, 0, 0, 0, 0, 0, 0,
        0, 3, 51, 0, 1, 0, 0, 0, 0,
    ];

    #[test]
    fn a_jbig2_image_becomes_an_xobject_of_its_own_pixels() {
        // `image` reads no JBIG2, so the bytes used to fail the whole document
        // here while the raster backend painted them; both now decode the same
        // way, and the XObject carries the dimensions the stream declares.
        let p = page(vec![sofd_display::DrawOp::Image {
            image: ImageRef::new(
                "qr",
                std::sync::Arc::new(QR_JBIG2.to_vec()),
                (100, 100),
                sofd_display::ImageCodec::Jbig2,
            ),
            dst: Rect::new(20.0, 20.0, 50.0, 50.0),
        }]);
        let bytes = page_to_pdf(&p, &PdfOptions::default()).expect("convert");
        let doc = Document::load_from(&bytes[..]).expect("parse");
        let page_id = *doc.get_pages().values().next().expect("page");
        let images = doc.get_page_images(page_id).expect("images");
        assert_eq!(images.len(), 1, "the bi-level image becomes one xobject");
        assert_eq!(images[0].width, 100);
        assert_eq!(images[0].height, 100);
    }

    #[test]
    fn an_image_becomes_an_xobject_drawn_with_a_matrix() {
        let mut png = Vec::new();
        let solid = image::RgbaImage::from_pixel(2, 2, image::Rgba([10, 20, 30, 255]));
        solid
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .expect("encode the fixture");
        let p = page(vec![sofd_display::DrawOp::Image {
            image: ImageRef::new(
                "i",
                std::sync::Arc::new(png),
                (2, 2),
                sofd_display::ImageCodec::Png,
            ),
            dst: Rect::new(5.0, 5.0, 10.0, 10.0),
        }]);
        let bytes = page_to_pdf(&p, &PdfOptions::default()).expect("convert");
        let doc = Document::load_from(&bytes[..]).expect("parse");
        let page_id = *doc.get_pages().values().next().expect("page");
        let images = doc.get_page_images(page_id).expect("images");
        assert_eq!(images.len(), 1, "one image xobject");
        let content = content_of(&bytes);
        assert!(content.contains("/Im0 Do"));
        // The placement is 10 mm square, so the matrix spans 10 mm in points
        // whatever the image's own pixel count is: `Do` paints across the unit
        // square, not across one pixel.
        assert!(
            content.contains("28.3465 0 0 -28.3465 14.1732 827.7165 cm"),
            "{content}"
        );
    }

    #[test]
    fn live_text_positions_each_character_with_its_own_matrix() {
        let run = GlyphRun {
            font: FontId(1),
            size: Millimeter(10.0),
            glyphs: vec![1, 2],
            advances: vec![4.0, 4.0],
            text: Some("ab".into()),
            cluster_map: None,
            codes_are_glyphs: false,
            transform: Affine::translate(20.0, 30.0),
            glyph_angle: 0.0,
            fill: Some(Paint::Solid(Color::BLACK)),
            stroke: None,
            stroke_style: Default::default(),
            letter_spacing: 0.0,
            word_spacing: 0.0,
            bbox: Rect::new(0.0, 0.0, 8.0, 10.0),
        };
        let content = content_of(
            &page_to_pdf(
                &page(vec![sofd_display::DrawOp::Text(run)]),
                &PdfOptions::default(),
            )
            .expect("convert"),
        );
        assert!(content.contains("BT /F1 28.3465 Tf"));
        assert!(content.contains("1 0 0 1 56.6929 756.8504 Tm (a) Tj"));
        assert!(content.contains("1 0 0 1 68.0315 756.8504 Tm (b) Tj"));
        assert!(content.contains("ET"));
    }

    #[test]
    fn parentheses_in_text_are_escaped() {
        assert_eq!(pdf_string("a(b)c\\d"), "a\\(b\\)c\\\\d");
        assert_eq!(pdf_string("line\nbreak"), "line\\nbreak");
    }

    #[test]
    fn a_run_without_text_and_without_fonts_is_counted_as_dropped() {
        let run = GlyphRun {
            font: FontId(9),
            size: Millimeter(3.0),
            glyphs: vec![1],
            advances: vec![3.0],
            text: None,
            cluster_map: None,
            codes_are_glyphs: false,
            transform: Affine::IDENTITY,
            glyph_angle: 0.0,
            fill: None,
            stroke: None,
            stroke_style: Default::default(),
            letter_spacing: 0.0,
            word_spacing: 0.0,
            bbox: Rect::EMPTY,
        };
        let mut sink = PdfSink::new(Rect::A4_PORTRAIT, &PdfOptions::default());
        replay(&page(vec![sofd_display::DrawOp::Text(run)]), &mut sink).expect("replay");
        assert_eq!(sink.stats().dropped_runs, 1);
        assert!(!sink.ops.contains("BT"), "nothing was emitted");
    }

    #[test]
    fn a_run_missing_a_glyph_is_counted_and_not_dropped() {
        // Orbitron draws 'A'; a code point far outside its coverage resolves
        // to no outline, so the run reaches the output short one mark.
        let mut manager = FontManager::new();
        manager.registry_mut().register_embedded(
            "tests/data/Orbitron-Bold.ttf",
            "Orbitron",
            std::sync::Arc::new(font_bytes()),
        );
        let run = GlyphRun {
            font: FontId(0),
            size: Millimeter(3.0),
            glyphs: vec![65, 60000],
            advances: vec![3.0, 3.0],
            text: Some("Ax".into()),
            cluster_map: None,
            codes_are_glyphs: false,
            transform: Affine::IDENTITY,
            glyph_angle: 0.0,
            fill: Some(Paint::Solid(Color::BLACK)),
            stroke: None,
            stroke_style: Default::default(),
            letter_spacing: 0.0,
            word_spacing: 0.0,
            bbox: Rect::new(0.0, 0.0, 6.0, 3.0),
        };
        let options = PdfOptions {
            text: TextExport::Outline,
            fonts: Some(&manager),
        };
        let mut sink = PdfSink::new(Rect::A4_PORTRAIT, &options);
        replay(&page(vec![sofd_display::DrawOp::Text(run)]), &mut sink).expect("replay");
        let stats = sink.stats();
        assert_eq!(stats.missing_glyph_runs, 1);
        assert_eq!(stats.outlined_glyphs, 1, "the drawable glyph still landed");
        assert_eq!(stats.dropped_runs, 0, "the run did reach the output");
        assert!(!stats.is_lossless(), "a short run is not a lossless page");
    }

    /// Outlined text in PDF is a path like any other, so a stroked run writes
    /// its colour and its width and is stroked rather than filled.
    #[test]
    fn an_outlined_run_writes_its_stroke_state() {
        let mut manager = FontManager::new();
        manager.registry_mut().register_embedded(
            "tests/data/Orbitron-Bold.ttf",
            "Orbitron",
            std::sync::Arc::new(font_bytes()),
        );
        let run = GlyphRun {
            font: FontId(0),
            size: Millimeter(5.0),
            glyphs: vec![65],
            advances: vec![5.0],
            text: Some("A".into()),
            cluster_map: None,
            codes_are_glyphs: false,
            transform: Affine::IDENTITY,
            glyph_angle: 0.0,
            fill: Some(Paint::Solid(Color::BLACK)),
            stroke: Some(Paint::Solid(Color::rgba(255, 0, 0, 255))),
            stroke_style: StrokeStyle {
                width: 1.0,
                ..StrokeStyle::default()
            },
            letter_spacing: 0.0,
            word_spacing: 0.0,
            bbox: Rect::new(0.0, 0.0, 5.0, 5.0),
        };
        let options = PdfOptions {
            text: TextExport::Outline,
            fonts: Some(&manager),
        };
        let p = page(vec![sofd_display::DrawOp::Text(run)]);
        let (bytes, stats) = page_to_pdf_with_stats(&p, &options).expect("convert");
        let content = content_of(&bytes);
        assert!(content.contains("1 0 0 rg"), "stroke colour, got {content}");
        // One millimetre is 72/25.4 points, and the width goes with it.
        assert!(content.contains("2.8346 w"), "stroke width, got {content}");
        assert!(
            content.contains(" S"),
            "the glyph is stroked, got {content}"
        );
        assert_eq!(stats.stroke_only_glyph_runs, 0);
    }

    /// A stroke-only run keeps its outline: nothing fills it, but every viewer
    /// strokes it, so the text is still on the page.
    #[test]
    fn a_stroke_only_run_is_still_drawn() {
        let mut manager = FontManager::new();
        manager.registry_mut().register_embedded(
            "tests/data/Orbitron-Bold.ttf",
            "Orbitron",
            std::sync::Arc::new(font_bytes()),
        );
        let run = GlyphRun {
            font: FontId(0),
            size: Millimeter(5.0),
            glyphs: vec![65],
            advances: vec![5.0],
            text: None,
            cluster_map: None,
            codes_are_glyphs: false,
            transform: Affine::IDENTITY,
            glyph_angle: 0.0,
            fill: None,
            stroke: Some(Paint::Solid(Color::BLACK)),
            stroke_style: StrokeStyle::default(),
            letter_spacing: 0.0,
            word_spacing: 0.0,
            bbox: Rect::new(0.0, 0.0, 5.0, 5.0),
        };
        let options = PdfOptions {
            text: TextExport::Outline,
            fonts: Some(&manager),
        };
        let mut sink = PdfSink::new(Rect::A4_PORTRAIT, &options);
        replay(&page(vec![sofd_display::DrawOp::Text(run)]), &mut sink).expect("replay");
        let stats = sink.stats();
        assert_eq!(stats.outlined_glyphs, 1);
        assert_eq!(stats.dropped_runs, 0);
        assert_eq!(stats.stroke_only_glyph_runs, 1);
        assert!(!stats.is_lossless());
    }

    #[test]
    fn a_translucent_fill_registers_one_extgstate() {
        let p = page(vec![
            sofd_display::DrawOp::Path {
                path: rect(0.0, 0.0, 10.0, 10.0),
                fill: Some(Paint::Solid(Color::rgba(0, 0, 0, 128))),
                stroke: None,
                stroke_style: StrokeStyle::default(),
                rule: PaintRule::NonZero,
            },
            sofd_display::DrawOp::Path {
                path: rect(20.0, 20.0, 10.0, 10.0),
                fill: Some(Paint::Solid(Color::rgba(0, 0, 0, 128))),
                stroke: None,
                stroke_style: StrokeStyle::default(),
                rule: PaintRule::NonZero,
            },
        ]);
        let bytes = page_to_pdf(&p, &PdfOptions::default()).expect("convert");
        let doc = Document::load_from(&bytes[..]).expect("parse");
        let page_id = *doc.get_pages().values().next().expect("page");
        // The graphics states live inline in the page resources, so count them
        // there rather than among the top level objects.
        let resources = doc.get_page_resources(page_id).expect("resources");
        let states = resources
            .0
            .expect("resource dict")
            .get(b"ExtGState")
            .expect("extgstate dict")
            .as_dict()
            .expect("state dict");
        assert_eq!(states.len(), 1, "one alpha, one state");
        let content = content_of(&bytes);
        assert!(content.contains("/GS0 gs"));
    }

    #[test]
    fn an_unbalanced_restore_is_an_error() {
        let err = page_to_pdf(
            &page(vec![sofd_display::DrawOp::Restore]),
            &PdfOptions::default(),
        )
        .expect_err("an unbalanced restore must fail");
        assert!(matches!(
            err,
            OfdError::Format(sofd_error::FormatError::CircularReference { .. })
        ));
    }

    #[test]
    fn a_trailing_save_still_writes_a_balanced_document() {
        let bytes = page_to_pdf(
            &page(vec![sofd_display::DrawOp::Save]),
            &PdfOptions::default(),
        )
        .expect("convert");
        let doc = Document::load_from(&bytes[..]).expect("parse");
        assert_eq!(doc.get_pages().len(), 1);
    }

    #[test]
    fn an_arc_is_flattened_into_cubics() {
        let mut path = Path::new();
        path.segments.push(PathSegment::MoveTo(0.0, 0.0));
        path.segments.push(PathSegment::Arc {
            rx: 5.0,
            ry: 5.0,
            x_axis_rotation: 0.0,
            large_arc: false,
            sweep: true,
            to: (10.0, 0.0),
        });
        let p = page(vec![sofd_display::DrawOp::Path {
            path,
            fill: Some(Paint::Solid(Color::BLACK)),
            stroke: None,
            stroke_style: StrokeStyle::default(),
            rule: PaintRule::NonZero,
        }]);
        let content = content_of(&page_to_pdf(&p, &PdfOptions::default()).expect("convert"));
        assert!(content.contains(" c"), "arcs flatten to cubic operators");
        assert!(!content.contains('A'), "no arc operator survives");
    }

    #[test]
    fn numbers_strip_trailing_zeros_and_negative_zero() {
        assert_eq!(num(2.0), "2");
        assert_eq!(num(0.5), "0.5");
        assert_eq!(num(-0.00001), "0");
        assert_eq!(num(f64::NAN), "0");
    }
}
