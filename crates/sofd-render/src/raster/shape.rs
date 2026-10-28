//! Painting shapes: state, transforms, paints and geometry.

use super::*;
use sofd_geometry::arc_cubics;

#[derive(Clone)]
pub(crate) struct State {
    pub(crate) ctm: Affine,
    pub(crate) clip: Option<Arc<Mask>>,
}

impl State {
    /// The clip as tiny-skia takes it, which is a borrow rather than a clone.
    pub(crate) fn clip_ref(&self) -> Option<&Mask> {
        self.clip.as_deref()
    }
}

/// The mask a clip intersects into, starting opaque on the first use so nested
/// clips compose. One copy of the page arrives here whatever the stack holds,
/// because a clip is shared: saving the state costs a pointer, not a page.
pub(crate) fn clip_mask(state: &State, width: u32, height: u32) -> Result<Mask> {
    let size = || {
        OfdError::Render(RenderError::InvalidSize {
            w: width,
            h: height,
        })
    };
    match state.clip_ref() {
        // The whole page, copied once: the same bytes as turning one opaque
        // mask into another, and a clip is a whole-page operation either way.
        Some(m) if m.width() == width && m.height() == height => Ok(m.clone()),
        _ => {
            let mut mask = Mask::new(width, height).ok_or_else(size)?;
            mask.data_mut().fill(255);
            Ok(mask)
        }
    }
}

/// Per-glyph outlines in page pixel space, in draw order. Outlines are
/// normalized to 1/1000 em with y up, so scale before positioning. A code the
/// run's own face lacks comes from the fallback chain.
pub(crate) fn glyph_outlines(
    fonts: &sofd_font::FontManager,
    run: &GlyphRun,
    face: Arc<sofd_font::FontFace>,
    total: Transform,
) -> Result<Vec<tiny_skia::Path>> {
    let mut offset = 0.0f64;
    let mut paths = Vec::new();
    for (i, &gid) in run.glyphs.iter().enumerate() {
        if i > 0 {
            offset += run.advances.get(i - 1).copied().unwrap_or(0.0);
        }
        offset += run.letter_spacing;
        let Some((outline, _source)) = fonts.outline_for_code(&face, gid, run.codes_are_glyphs)
        else {
            continue;
        };
        let k = run.size.value() / 1000.0;
        let glyph = Affine::new(k, 0.0, 0.0, -k, 0.0, 0.0)
            .then(Affine::translate(offset, 0.0))
            .then(run.transform);
        paths.push(to_ts_path(&outline, total.pre_concat(to_ts(glyph)))?);
    }
    Ok(paths)
}

pub(crate) fn to_color(c: &Color) -> tiny_skia::Color {
    tiny_skia::Color::from_rgba8(c.r, c.g, c.b, c.a)
}

/// Page-space millimetres to pixmap pixels, the raster's origin included.
/// `then` applies `self` first, so the scale goes first and the origin is
/// subtracted afterwards, in pixels: a raster of a window away from the page's
/// corner otherwise moves every mark by the origin scaled twice, which puts
/// all of it outside the bitmap.
pub(crate) fn total_transform(state: &State, origin: &Rect, dpi: f64) -> Transform {
    let s = dpi / 25.4;
    let mm_to_px = Affine::scale(s, s).then(Affine::translate(-origin.x * s, -origin.y * s));
    to_ts(state.ctm.then(mm_to_px))
}

/// Convert an affine matrix into a tiny-skia transform, pixel for pixel.
pub(crate) fn to_ts(a: Affine) -> Transform {
    Transform::from_row(
        a.a as f32, a.b as f32, a.c as f32, a.d as f32, a.e as f32, a.f as f32,
    )
}

pub(crate) fn fill_rule(rule: PaintRule) -> FillRule {
    match rule {
        PaintRule::NonZero => FillRule::Winding,
        PaintRule::EvenOdd => FillRule::EvenOdd,
    }
}

pub(crate) fn solid_paint(paint: &Paint) -> TsPaint<'static> {
    // Anything but a flat colour has no single value to paint here; a pattern
    // falls back to black so a malformed reference stays visible rather than
    // silently dropping the fill. Gradients never reach this function.
    let color = match paint {
        Paint::Solid(c) => to_color(c),
        _ => tiny_skia::Color::BLACK,
    };
    TsPaint {
        shader: Shader::SolidColor(color),
        anti_alias: true,
        ..TsPaint::default()
    }
}

/// The paint for a fill or stroke, resolving a gradient to its shader. A
/// gradient is stated on the object itself, so this needs nothing from the
/// surrounding display list.
pub(crate) fn paint_for(paint: &Paint, total: Transform) -> Option<TsPaint<'static>> {
    match paint {
        Paint::Solid(_) => Some(solid_paint(paint)),
        Paint::LinearGradient(spec) => Some(shader_paint(linear_shader(
            spec.start,
            spec.end,
            &spec.stops,
            total,
        ))),
        Paint::RadialGradient(spec) => {
            let paint = match sofd_display::radial_form(
                spec.center,
                spec.radius,
                spec.end,
                spec.end_radius,
                &spec.stops,
            ) {
                sofd_display::RadialForm::Cone(cone) => shader_paint(focal_shader(&cone, total)),
                // Equal circles: the shading varies along the axis between the
                // centres, which a linear gradient paints.
                sofd_display::RadialForm::Cylinder { start, end } => {
                    shader_paint(linear_shader(start, end, &spec.stops, total))
                }
                // Neither circle has extent: one flat colour keeps the region
                // painted instead of dropping it.
                sofd_display::RadialForm::Flat(color) => TsPaint {
                    shader: Shader::SolidColor(to_color(&color)),
                    anti_alias: true,
                    ..TsPaint::default()
                },
            };
            Some(paint)
        }
        // A mesh is painted per pixel by `mesh::paint_mesh` and a pattern tiled
        // by `pattern::paint_pattern`; both need the pixmap, so neither is a
        // shader `paint_for` can build.
        Paint::Mesh(_) | Paint::Pattern(_) => None,
    }
}

pub(crate) fn shader_paint(shader: Shader<'static>) -> TsPaint<'static> {
    TsPaint {
        shader,
        anti_alias: true,
        ..TsPaint::default()
    }
}

pub(crate) fn to_ts_stroke(style: &StrokeStyle, state: &State, dpi: f64) -> Stroke {
    // Dash lengths and widths scale with the CTM, so a zoomed page keeps
    // dash proportions instead of compressing them.
    let s = dpi / 25.4 * state.ctm.scale_factor();
    let dash = if style.dash.is_empty() {
        None
    } else {
        let pattern: Vec<f32> = style.dash.iter().map(|d| (*d * s) as f32).collect();
        StrokeDash::new(pattern, (style.dash_offset * s) as f32)
    };
    Stroke {
        width: (style.width.max(0.0) * s) as f32,
        miter_limit: style.miter_limit as f32,
        line_cap: match style.cap {
            LineCap::Butt => TsLineCap::Butt,
            LineCap::Round => TsLineCap::Round,
            LineCap::Square => TsLineCap::Square,
        },
        line_join: match style.join {
            LineJoin::Miter => TsLineJoin::Miter,
            LineJoin::Round => TsLineJoin::Round,
            LineJoin::Bevel => TsLineJoin::Bevel,
        },
        dash,
    }
}

/// Whether tiny-skia will paint this stroke as a modulated one-pixel hairline
/// rather than outlining it. The test is the one `treat_as_hairline` applies,
/// restated so this side can tell which painter will run: tiny-skia's hairline
/// scan converter only understands quadratic curve segments, and drops the
/// cubic ones, which turns a circle drawn as cubics into a polygon.
pub(crate) fn is_hairline(paint: &TsPaint<'_>, stroke: &Stroke) -> bool {
    stroke.width > 0.0 && paint.anti_alias && !reaches_a_pixel(stroke.width)
}

fn reaches_a_pixel(width: f32) -> bool {
    fast_len(width, 0.0) > 1.0 || fast_len(0.0, width) > 1.0
}

fn fast_len(x: f32, y: f32) -> f32 {
    let (mut x, mut y) = (x.abs(), y.abs());
    if x < y {
        std::mem::swap(&mut x, &mut y);
    }
    x + y * 0.5
}

/// One device pixel, in the units the stroke width is in — the two went through
/// the same scale, so a width of one draws the same mark the hairline painter
/// would, and now goes through a painter that can outline a curve.
pub(crate) fn hairline_width(_stroke: &Stroke) -> f32 {
    1.0
}

pub(crate) fn push_segment(builder: &mut TsBuilder, seg: &PathSegment, cur: &mut (f64, f64)) {
    match seg {
        PathSegment::MoveTo(x, y) => {
            builder.move_to(*x as f32, *y as f32);
            *cur = (*x, *y);
        }
        PathSegment::LineTo(x, y) => {
            builder.line_to(*x as f32, *y as f32);
            *cur = (*x, *y);
        }
        PathSegment::CurveTo { c1, c2, to } => {
            builder.cubic_to(
                c1.0 as f32,
                c1.1 as f32,
                c2.0 as f32,
                c2.1 as f32,
                to.0 as f32,
                to.1 as f32,
            );
            *cur = *to;
        }
        PathSegment::QuadTo { c, to } => {
            // Elevate the quadratic to a cubic; an exact transform exists.
            let c1 = (
                cur.0 + 2.0 / 3.0 * (c.0 - cur.0),
                cur.1 + 2.0 / 3.0 * (c.1 - cur.1),
            );
            let c2 = (
                to.0 + 2.0 / 3.0 * (c.0 - to.0),
                to.1 + 2.0 / 3.0 * (c.1 - to.1),
            );
            builder.cubic_to(
                c1.0 as f32,
                c1.1 as f32,
                c2.0 as f32,
                c2.1 as f32,
                to.0 as f32,
                to.1 as f32,
            );
            *cur = *to;
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
                arc_cubics(*rx, *ry, *x_axis_rotation, *large_arc, *sweep, *cur, *to)
            {
                builder.cubic_to(
                    c1.0 as f32,
                    c1.1 as f32,
                    c2.0 as f32,
                    c2.1 as f32,
                    end.0 as f32,
                    end.1 as f32,
                );
            }
            *cur = *to;
        }
        PathSegment::Close => builder.close(),
    }
}

pub(crate) fn to_ts_path(path: &Path, transform: Transform) -> Result<TsPath> {
    let mut builder = TsBuilder::new();
    let mut cur = (0.0f64, 0.0f64);
    for seg in &path.segments {
        push_segment(&mut builder, seg, &mut cur);
    }
    builder
        .finish()
        .and_then(|p| p.transform(transform))
        .ok_or_else(|| {
            OfdError::Render(RenderError::BudgetExceeded(
                "path transform was not invertible".to_string(),
            ))
        })
}
