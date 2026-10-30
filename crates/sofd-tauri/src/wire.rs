//! A page as a 2D context takes it: shapes in page millimetres, paths as the
//! `d` an SVG writes, and paint flattened to what a canvas understands.
//!
//! Nothing here decides how a page looks. The walk in `sofd-render` resolved
//! transforms, clips and outlines; this re-expresses them for `Path2D`, and
//! names in `degraded` whatever it could only approximate.

use std::collections::BTreeMap;

use serde::Serialize;
use sofd_common::Rect;
use sofd_display::ops::{Color, LineCap, LineJoin, Paint, PaintRule, StrokeStyle};
use sofd_display::{DrawOp, ImageCodec, PageDisplay};
use sofd_render::{Shape, VectorPage};

/// One page, ready to draw.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PageGeometry {
    /// Which page this is.
    pub page: usize,
    /// The page's width in millimetres.
    pub width_mm: f64,
    /// The page's height in millimetres.
    pub height_mm: f64,
    /// Runs that lost at least one glyph.
    pub missing_glyph_runs: usize,
    /// Glyph positions no face could draw.
    pub missing_glyphs: usize,
    /// Runs left out because no face loaded.
    pub skipped_font_runs: usize,
    /// Shapes in drawing order.
    pub shapes: Vec<WireShape>,
    /// What was drawn as an approximation, once each, for the status bar.
    pub degraded: Vec<String>,
}

/// One thing to draw, in the order a canvas must draw it.
#[derive(Debug, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum WireShape {
    /// Fill and/or stroke one path.
    #[serde(rename_all = "camelCase")]
    Ink {
        /// Path data, in millimetres.
        d: String,
        /// Fill paint, absent when the interior is untouched.
        fill: Option<WirePaint>,
        /// Stroke paint, absent when the outline is untouched.
        stroke: Option<WirePaint>,
        /// Stroke geometry, in millimetres.
        style: WireStroke,
        /// `nonzero` or `evenodd`.
        rule: &'static str,
    },
    /// Place a bitmap at the transform that puts it on the page.
    #[serde(rename_all = "camelCase")]
    Image {
        /// A `data:` URL, absent when the codec is one a browser cannot read.
        src: Option<String>,
        /// Pixel space to millimetres, as `[a, b, c, d, e, f]`.
        to_page: [f64; 6],
        /// Intrinsic pixel size, which is what `drawImage` is given.
        pixel_width: u32,
        /// Intrinsic pixel height.
        pixel_height: u32,
    },
    /// Narrow the clip for the shapes that follow.
    #[serde(rename_all = "camelCase")]
    Clip {
        /// Clip geometry, in millimetres.
        d: String,
        /// `nonzero` or `evenodd`.
        rule: &'static str,
    },
    /// Put the clip back the way it was.
    ClipReset,
}

/// A paint, in a form a canvas can set.
#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum WirePaint {
    /// One colour, as a CSS colour string.
    Solid {
        /// `rgba(r, g, b, a)`.
        css: String,
    },
    /// A linear shading, in millimetres.
    #[serde(rename_all = "camelCase")]
    Linear {
        /// Start of the gradient axis.
        x0: f64,
        /// Start of the gradient axis.
        y0: f64,
        /// End of the gradient axis.
        x1: f64,
        /// End of the gradient axis.
        y1: f64,
        /// Colour stops, at least two.
        stops: Vec<WireStop>,
    },
    /// A radial shading: the cone between two circles, in millimetres.
    #[serde(rename_all = "camelCase")]
    Radial {
        /// Start circle centre.
        x0: f64,
        /// Start circle centre.
        y0: f64,
        /// Start circle radius.
        r0: f64,
        /// End circle centre.
        x1: f64,
        /// End circle centre.
        y1: f64,
        /// End circle radius.
        r1: f64,
        /// Colour stops, at least two.
        stops: Vec<WireStop>,
    },
}

/// One colour stop of a gradient.
#[derive(Debug, Serialize)]
pub struct WireStop {
    /// Position along the gradient, `0.0..=1.0`.
    pub offset: f64,
    /// The colour there, as a CSS colour string.
    pub css: String,
}

/// How a stroke is drawn, in millimetres.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WireStroke {
    /// Line width in millimetres.
    pub width: f64,
    /// Dash pattern in millimetres; empty means solid.
    pub dash: Vec<f64>,
    /// Offset into the dash pattern.
    pub dash_offset: f64,
    /// `butt`, `round` or `square`.
    pub cap: &'static str,
    /// `miter`, `round` or `bevel`.
    pub join: &'static str,
    /// Miter length limit before a join is bevelled.
    pub miter_limit: f64,
}

/// One rectangle drawn over the page, in page millimetres.
#[derive(Debug, Serialize)]
pub struct WireRect {
    /// Left edge.
    pub x: f64,
    /// Top edge.
    pub y: f64,
    /// Width.
    pub w: f64,
    /// Height.
    pub h: f64,
    /// Whether this is the one the reader is visiting, which draws stronger.
    pub current: bool,
    /// The sheet token that colours it, spelled as a custom property.
    pub token: &'static str,
}

/// The rectangles a page carries above its content: search hits and the
/// boundaries of annotations.
#[derive(Debug, Serialize)]
pub struct WireMarks {
    /// Every hit on the page, in the order search found them.
    pub hits: Vec<WireRect>,
    /// The boundary of every visible annotation on the page.
    pub marks: Vec<WireRect>,
}

/// The overlays of one page, ready to draw over its canvas.
///
/// The colour is a token's name rather than a value because both of these are
/// drawn on white paper: the theme's surface tokens would vanish there, so the
/// sheet holds the two that survive both columns and the window resolves them.
pub fn marks(hits: &[(Rect, bool)], boundaries: &[(Rect, bool)]) -> WireMarks {
    let rect = |(rect, current): &(Rect, bool), token: &'static str| WireRect {
        x: rect.x,
        y: rect.y,
        w: rect.w,
        h: rect.h,
        current: *current,
        token,
    };
    WireMarks {
        hits: hits.iter().map(|hit| rect(hit, "--hit")).collect(),
        marks: boundaries.iter().map(|mark| rect(mark, "--mark")).collect(),
    }
}

/// What a page walk could not draw exactly, by kind, so one page reports each
/// approximation once rather than once per shape.
#[derive(Debug, Default)]
pub struct Degraded(BTreeMap<&'static str, usize>);

impl Degraded {
    /// Note one shape drawn as an approximation.
    fn note(&mut self, what: &'static str) {
        *self.0.entry(what).or_default() += 1;
    }

    /// The sentences to show, one per kind.
    fn into_messages(self) -> Vec<String> {
        self.0
            .into_iter()
            .map(|(what, count)| format!("{what} ({count})"))
            .collect()
    }
}

/// Turn a walked page into the shapes a canvas draws.
pub fn geometry(page: usize, walked: &VectorPage) -> PageGeometry {
    let mut degraded = Degraded::default();
    let shapes = walked
        .shapes
        .iter()
        .filter_map(|shape| wire(shape, &mut degraded))
        .collect();
    PageGeometry {
        page,
        width_mm: walked.page_box.w,
        height_mm: walked.page_box.h,
        missing_glyph_runs: walked.missing_glyph_runs,
        missing_glyphs: walked.missing_glyphs,
        skipped_font_runs: walked.skipped_font_runs,
        shapes,
        degraded: degraded.into_messages(),
    }
}

/// One shape, or `None` when it draws nothing a canvas can take.
fn wire(shape: &Shape, degraded: &mut Degraded) -> Option<WireShape> {
    match shape {
        Shape::Path {
            path,
            fill,
            stroke,
            stroke_style,
            rule,
        } => Some(WireShape::Ink {
            d: sofd_convert::svg::path_data(path),
            fill: paint(fill.as_ref(), degraded),
            stroke: paint(stroke.as_ref(), degraded),
            style: stroke_style_of(stroke_style),
            rule: rule_of(*rule),
        }),
        // A text run arrives resolved to outlines, so a canvas draws it as a
        // path. OFD states an outline for text as on or off rather than how
        // wide, so the default is what an outlined glyph means.
        Shape::Text { path, fill, stroke } => Some(WireShape::Ink {
            d: sofd_convert::svg::path_data(path),
            fill: paint(fill.as_ref(), degraded),
            stroke: paint(stroke.as_ref(), degraded),
            style: stroke_style_of(&StrokeStyle::default()),
            rule: "nonzero",
        }),
        Shape::Image { image, to_page } => Some(image_shape(image, *to_page, degraded)),
        Shape::Clip { path, rule } => Some(WireShape::Clip {
            d: sofd_convert::svg::path_data(path),
            rule: rule_of(*rule),
        }),
        Shape::ClipReset => Some(WireShape::ClipReset),
    }
}

/// A bitmap, as a `data:` URL the webview can load.
fn image_shape(
    image: &sofd_display::ImageRef,
    to_page: sofd_geometry::Affine,
    degraded: &mut Degraded,
) -> WireShape {
    let mime = match image.codec {
        ImageCodec::Jpeg => Some("image/jpeg"),
        ImageCodec::Png => Some("image/png"),
        ImageCodec::Gif => Some("image/gif"),
        ImageCodec::Bmp => Some("image/bmp"),
        // A webview decodes none of these, and guessing at the bytes would
        // paint noise where a photograph belongs. An absent `src` is a frame
        // the reader can see is missing rather than one that lies.
        ImageCodec::Tiff | ImageCodec::Jbig2 | ImageCodec::Raw => None,
    };
    let src = mime.map(|mime| {
        format!(
            "data:{mime};base64,{}",
            base64_encode(image.data.as_slice())
        )
    });
    if src.is_none() {
        degraded.note("an image in a format a webview cannot decode");
    }
    WireShape::Image {
        src,
        to_page: [
            to_page.a, to_page.b, to_page.c, to_page.d, to_page.e, to_page.f,
        ],
        pixel_width: image.pixel_size.0,
        pixel_height: image.pixel_size.1,
    }
}

/// A paint, or `None` when there is nothing to paint.
fn paint(source: Option<&Paint>, degraded: &mut Degraded) -> Option<WirePaint> {
    let source = source?;
    match source {
        Paint::Solid(colour) => Some(WirePaint::Solid { css: css(*colour) }),
        Paint::LinearGradient(spec) => Some(WirePaint::Linear {
            x0: spec.start.x,
            y0: spec.start.y,
            x1: spec.end.x,
            y1: spec.end.y,
            stops: stops(&spec.stops),
        }),
        Paint::RadialGradient(spec) => Some(WirePaint::Radial {
            x0: spec.center.x,
            y0: spec.center.y,
            r0: spec.radius,
            x1: spec.end.x,
            y1: spec.end.y,
            r1: spec.end_radius,
            stops: stops(&spec.stops),
        }),
        // A Gouraud mesh is a colour per vertex, which `createLinearGradient`
        // cannot follow across triangles. The first vertex's colour over the
        // whole area is the nearest flat answer, and it is named as one.
        Paint::Mesh(spec) => {
            degraded.note("a mesh shading drawn as one colour");
            Some(WirePaint::Solid {
                css: css(spec.points.first().map(|v| v.color).unwrap_or(Color::BLACK)),
            })
        }
        // A pattern repeats a cell of vector content, and a canvas pattern can
        // only repeat an image. Until the cell is drawn offscreen, the area
        // keeps its own colour rather than filling with a wrong one.
        Paint::Pattern(_) => {
            degraded.note("a pattern fill left unfilled");
            None
        }
    }
}

/// The stops of a gradient, in CSS order.
fn stops(source: &[sofd_display::ops::GradientStop]) -> Vec<WireStop> {
    source
        .iter()
        .map(|stop| WireStop {
            offset: stop.offset.clamp(0.0, 1.0),
            css: css(stop.color),
        })
        .collect()
}

/// Stroke geometry as a canvas takes it.
fn stroke_style_of(style: &StrokeStyle) -> WireStroke {
    WireStroke {
        width: style.width,
        dash: style.dash.clone(),
        dash_offset: style.dash_offset,
        cap: match style.cap {
            LineCap::Butt => "butt",
            LineCap::Round => "round",
            LineCap::Square => "square",
        },
        join: match style.join {
            LineJoin::Miter => "miter",
            LineJoin::Round => "round",
            LineJoin::Bevel => "bevel",
        },
        miter_limit: style.miter_limit,
    }
}

/// A fill rule, spelled the way `CanvasFillRule` spells it.
fn rule_of(rule: PaintRule) -> &'static str {
    match rule {
        PaintRule::NonZero => "nonzero",
        PaintRule::EvenOdd => "evenodd",
    }
}

/// A colour as CSS `rgba()`. A canvas takes a float alpha, so the byte is
/// divided rather than rounded: 0x80 keeps its half.
pub fn css(colour: Color) -> String {
    format!(
        "rgba({}, {}, {}, {})",
        colour.r,
        colour.g,
        colour.b,
        f64::from(colour.a) / 255.0
    )
}

/// Standard base64 with padding, which is what a `data:` URL carries.
fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            chunk.get(1).copied().unwrap_or(0),
            chunk.get(2).copied().unwrap_or(0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[(n >> 6) as usize & 63] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[n as usize & 63] as char
        } else {
            '='
        });
    }
    out
}

/// How many shapes of each kind a display list holds, which is what a test
/// needs to say the walk reached everything without a window.
pub fn count_ops(page: &PageDisplay) -> BTreeMap<&'static str, usize> {
    let mut counts = BTreeMap::new();
    for op in &page.ops {
        let name = match op {
            DrawOp::Path { .. } => "path",
            DrawOp::Text(_) => "text",
            DrawOp::Image { .. } => "image",
            DrawOp::ClipPath { .. } => "clip",
            DrawOp::ClipText(_) => "clipText",
            _ => "other",
        };
        *counts.entry(name).or_insert(0) += 1;
    }
    counts
}
