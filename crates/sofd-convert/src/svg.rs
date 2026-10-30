//! SVG backend.
//! Millimetres double as user units, so the viewBox carries the page box and
//! no coordinate scaling happens anywhere in the output.

use std::fmt::Write as _;

use sofd_common::Rect;
use sofd_display::ops::{Color, LineCap, LineJoin, PaintRule, StrokeStyle};
use sofd_display::{GlyphRun, ImageCodec, ImageRef, LinearGradientSpec, RadialGradientSpec};
use sofd_error::Result;
use sofd_font::FontManager;
use sofd_geometry::{Affine, Path, PathSegment};

use crate::replay::{PageSink, PaintShape, PathShape, replay, text_paint_shape};
use crate::{ConversionStats, TextExport};

/// Options for SVG export.
#[derive(Debug, Clone, Default)]
pub struct SvgOptions<'a> {
    /// How glyph runs become SVG content.
    pub text: TextExport,
    /// Font source for outlined text; without one, runs fall back to live text.
    pub fonts: Option<&'a FontManager>,
}

/// Builds one SVG page by replaying a display list into it.
pub struct SvgSink<'a> {
    body: String,
    page_box: Rect,
    text: TextExport,
    fonts: Option<&'a FontManager>,
    ctm: Affine,
    stack: Vec<Affine>,
    levels: Vec<usize>,
    depth: usize,
    seq: usize,
    stats: ConversionStats,
}

impl<'a> SvgSink<'a> {
    /// Start a page of the given size.
    pub fn new(page_box: Rect, options: &SvgOptions<'a>) -> Self {
        Self {
            // The root group lets a transform at page level nest somewhere.
            body: String::from("  <g>\n"),
            page_box,
            text: options.text,
            fonts: options.fonts,
            ctm: Affine::IDENTITY,
            stack: Vec::new(),
            levels: vec![0],
            depth: 2,
            seq: 0,
            stats: ConversionStats::default(),
        }
    }

    /// Finish the page and return the document plus what it contains.
    pub fn finish(mut self) -> (String, ConversionStats) {
        self.close_levels();
        // Closing the root group lands back inside the svg element.
        self.depth = 0;
        self.line("</svg>".to_string());
        let mut doc = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        let (x, y, w, h) = (
            self.page_box.x,
            self.page_box.y,
            self.page_box.w,
            self.page_box.h,
        );
        let _ = write!(
            doc,
            "<svg xmlns=\"http://www.w3.org/2000/svg\" version=\"1.1\" \
             width=\"{w}mm\" height=\"{h}mm\" viewBox=\"{x} {y} {w} {h}\">"
        );
        doc.push('\n');
        doc.push_str(&self.body);
        (doc, self.stats)
    }

    /// What the page contained, for callers that report conversion quality.
    pub fn stats(&self) -> &ConversionStats {
        &self.stats
    }

    fn line(&mut self, text: String) {
        for _ in 0..self.depth {
            self.body.push_str("  ");
        }
        self.body.push_str(&text);
        self.body.push('\n');
    }

    /// Close every group still open, which an unbalanced save would leave.
    fn close_levels(&mut self) {
        while let Some(open) = self.levels.pop() {
            for _ in 0..=open {
                self.depth -= 1;
                self.line("</g>".to_string());
            }
        }
    }

    /// Open a group that the current level's restore will close.
    fn open_extra(&mut self, attrs: &str) {
        if attrs.is_empty() {
            self.line("<g>".to_string());
        } else {
            self.line(format!("<g {attrs}>"));
        }
        self.depth += 1;
        if let Some(top) = self.levels.last_mut() {
            *top += 1;
        }
    }

    /// The `fill`/`stroke` value for a paint. A gradient is defined inline,
    /// because the paint carries its own axis, so no earlier op is needed.
    /// A mesh is not one: it has no SVG paint to name, so `path` writes it.
    fn paint_ref(&mut self, shape: PaintShape<'_>) -> String {
        match shape {
            PaintShape::Solid(c) => hex(c),
            PaintShape::Linear(spec) => self.define_linear(spec),
            PaintShape::Radial(spec) => self.define_radial(spec),
            // Reached only for a colour that is not a paint, which a mesh never
            // is: the mean keeps the region filled rather than dropping it.
            PaintShape::Mesh(spec) => mesh_average(spec),
            PaintShape::Pattern(spec) => self.define_pattern(spec),
        }
    }

    /// Define a `<pattern>` tiling the cell across the area. The element spans
    /// one step, so `XStep` and `YStep` become its width and height. SVG repeats
    /// a tile unchanged, so a reflecting pattern becomes a 2x2 mirrored block.
    fn define_pattern(&mut self, spec: &sofd_display::PatternSpec) -> String {
        if spec.width <= 0.0 || spec.height <= 0.0 {
            return pattern_average(spec);
        }
        self.seq += 1;
        let id = format!("pat{}", self.seq);
        let step_x = if spec.step_x > 0.0 {
            spec.step_x
        } else {
            spec.width
        };
        let step_y = if spec.step_y > 0.0 {
            spec.step_y
        } else {
            spec.height
        };
        // A cell drawn once is the common case; a reflecting one needs the
        // mirrored neighbours in the block so plain tiling reproduces them.
        let cols = if matches!(
            spec.reflect,
            sofd_display::Reflect::Row | sofd_display::Reflect::RowAndColumn
        ) {
            2
        } else {
            1
        };
        let rows = if matches!(
            spec.reflect,
            sofd_display::Reflect::Column | sofd_display::Reflect::RowAndColumn
        ) {
            2
        } else {
            1
        };
        let mut cell = String::new();
        for row in 0..rows {
            for col in 0..cols {
                let (mx, my) = spec.reflect.mirrors(col, row);
                let transform = match (mx, my) {
                    (false, false) => String::new(),
                    (true, false) => format!(
                        " transform=\"translate({},{}) scale(-1,1)\"",
                        num(step_x * (col as f64 + 1.0)),
                        num(0.0)
                    ),
                    (false, true) => format!(
                        " transform=\"translate({},{}) scale(1,-1)\"",
                        num(0.0),
                        num(step_y * (row as f64 + 1.0))
                    ),
                    (true, true) => format!(
                        " transform=\"translate({},{}) scale(-1,-1)\"",
                        num(step_x * (col as f64 + 1.0)),
                        num(step_y * (row as f64 + 1.0))
                    ),
                };
                let mut placed = String::new();
                for op in &spec.cell.ops {
                    self.pattern_op(op, &mut placed);
                }
                if placed.is_empty() {
                    continue;
                }
                if transform.is_empty() {
                    cell.push_str(&placed);
                } else {
                    cell.push_str(&format!("<g{transform}>{placed}</g>"));
                }
            }
        }
        self.line(format!(
            "<pattern id=\"{id}\" patternUnits=\"userSpaceOnUse\" x=\"0\" y=\"0\" \
             width=\"{w}\" height=\"{h}\">{cell}</pattern>",
            w = num(step_x * cols as f64),
            h = num(step_y * rows as f64),
        ));
        format!("url(#{id})")
    }

    /// Emit one op of a pattern's cell as an SVG element, at cell scale.
    /// A nested gradient or pattern falls back to its flat stand-in, since
    /// re-entering the def machinery from inside a cell is not worth it.
    fn pattern_op(&mut self, op: &sofd_display::DrawOp, out: &mut String) {
        let sofd_display::DrawOp::Path {
            path,
            fill,
            stroke,
            stroke_style,
            rule,
        } = op
        else {
            return;
        };
        let mut attrs = format!("d=\"{}\"", path_data(path));
        match fill {
            Some(sofd_display::Paint::Solid(c)) => {
                let _ = write!(attrs, " fill=\"{}\"", hex(c));
                if c.a != 255 {
                    let _ = write!(attrs, " fill-opacity=\"{}\"", opacity(c));
                }
            }
            Some(_) => attrs.push_str(" fill=\"#000000\""),
            None => attrs.push_str(" fill=\"none\""),
        }
        if *rule == PaintRule::EvenOdd {
            attrs.push_str(" fill-rule=\"evenodd\"");
        }
        match stroke {
            Some(sofd_display::Paint::Solid(c)) => {
                let _ = write!(attrs, " stroke=\"{}\"", hex(c));
                let _ = write!(attrs, " stroke-width=\"{}\"", num(stroke_style.width));
            }
            Some(_) => attrs.push_str(" stroke=\"#000000\""),
            None => attrs.push_str(" stroke=\"none\""),
        }
        let _ = writeln!(out, "<path {attrs}/>");
    }

    /// Define a `<linearGradient>` and return the `url(#id)` that uses it.
    fn define_linear(&mut self, spec: &LinearGradientSpec) -> String {
        if spec.stops.is_empty() {
            return "#000000".to_string();
        }
        self.seq += 1;
        let id = format!("grad{}", self.seq);
        self.stats.gradients += 1;
        self.line(format!(
            "<linearGradient id=\"{id}\" gradientUnits=\"userSpaceOnUse\" x1=\"{}\" y1=\"{}\" x2=\"{}\" y2=\"{}\">{}</linearGradient>",
            num(spec.start.x),
            num(spec.start.y),
            num(spec.end.x),
            num(spec.end.y),
            stops_markup(&spec.stops)
        ));
        format!("url(#{id})")
    }

    /// Define a `<radialGradient>` for the clause's two-circle cone and return
    /// the `url(#id)` that uses it. Equal circles leave no apex, so colour
    /// varies along the axis and a linear gradient paints it.
    fn define_radial(&mut self, spec: &RadialGradientSpec) -> String {
        if spec.stops.is_empty() {
            return "#000000".to_string();
        }
        match sofd_display::radial_form(
            spec.center,
            spec.radius,
            spec.end,
            spec.end_radius,
            &spec.stops,
        ) {
            sofd_display::RadialForm::Cone(cone) => {
                self.seq += 1;
                let id = format!("grad{}", self.seq);
                self.stats.gradients += 1;
                self.line(format!(
                    "<radialGradient id=\"{id}\" gradientUnits=\"userSpaceOnUse\" cx=\"{}\" cy=\"{}\" r=\"{}\" fx=\"{}\" fy=\"{}\">{}</radialGradient>",
                    num(cone.circle.x),
                    num(cone.circle.y),
                    num(cone.radius.max(0.0)),
                    num(cone.apex.x),
                    num(cone.apex.y),
                    stops_markup(&cone.stops)
                ));
                format!("url(#{id})")
            }
            sofd_display::RadialForm::Cylinder { start, end } => {
                let linear = LinearGradientSpec {
                    start,
                    end,
                    stops: spec.stops.clone(),
                };
                self.define_linear(&linear)
            }
            sofd_display::RadialForm::Flat(color) => hex(&color),
        }
    }

    /// Paint a Gouraud mesh as one `<path>` per triangle, filled with the mean
    /// of that triangle's three vertices. SVG 1.1 has no mesh gradient, and a
    /// triangle is the coarsest thing it can fill unevenly: the colour changes
    /// at each edge instead of across the face, so the triangles are visible as
    /// flat facets. That is a finer copy of the page than one colour for the
    /// whole region, and it is what the export promises rather than a stand-in.
    fn mesh_triangles(&mut self, spec: &sofd_display::MeshSpec) {
        for tri in &spec.triangles {
            let Some(v) = tri_vertices(spec, tri) else {
                continue;
            };
            // User space, so the triangle is stated once and the transform the
            // replay opened onto the page places it like any other path.
            self.line(format!(
                "<path d=\"M{} {} L{} {} L{} {} Z\" fill=\"{}\" fill-opacity=\"{}\"/>",
                num(v[0].x),
                num(v[0].y),
                num(v[1].x),
                num(v[1].y),
                num(v[2].x),
                num(v[2].y),
                hex(&triangle_mean(&v)),
                num(triangle_alpha(&v))
            ));
        }
    }
}

impl PageSink for SvgSink<'_> {
    fn save(&mut self) {
        // The group this opens is closed by the matching restore, not counted
        // as an extra of the enclosing level.
        self.line("<g>".to_string());
        self.depth += 1;
        self.stack.push(self.ctm);
        self.levels.push(0);
    }

    fn restore(&mut self) {
        self.ctm = self.stack.pop().unwrap_or(Affine::IDENTITY);
        // The root level is implicit and closed by `finish`; replay refuses an
        // unbalanced restore before it reaches here, so this only guards
        // direct use of the sink.
        let open = if self.levels.len() > 1 {
            self.levels.pop().unwrap_or(0)
        } else {
            0
        };
        for _ in 0..=open {
            self.depth -= 1;
            self.line("</g>".to_string());
        }
    }

    fn transform(&mut self, matrix: Affine) {
        self.ctm = self.ctm.then(matrix);
        self.open_extra(&format!("transform=\"matrix({})\"", matrix_attr(matrix)));
    }

    fn clip(&mut self, path: &Path, rule: PaintRule) {
        self.seq += 1;
        let id = format!("clip{}", self.seq);
        let rule_attr = match rule {
            PaintRule::NonZero => String::new(),
            PaintRule::EvenOdd => " clip-rule=\"evenodd\"".to_string(),
        };
        self.line(format!(
            "<clipPath id=\"{id}\" clipPathUnits=\"userSpaceOnUse\"><path d=\"{}\"{rule_attr}/></clipPath>",
            path_data(path)
        ));
        self.open_extra(&format!("clip-path=\"url(#{id})\""));
    }

    fn clip_text(&mut self, run: &GlyphRun) {
        let Some(fonts) = self.fonts else {
            return;
        };
        let Some(face) = sofd_display::run_face(fonts, run) else {
            return;
        };
        let outlines = glyph_paths(run, &face);
        if outlines.is_empty() {
            return;
        }
        self.seq += 1;
        let id = format!("clip{}", self.seq);
        let mut body = format!("<clipPath id=\"{id}\" clipPathUnits=\"userSpaceOnUse\">");
        for path in &outlines {
            let _ = write!(body, "<path d=\"{}\"/>", path_data(path));
        }
        body.push_str("</clipPath>");
        self.line(body);
        self.open_extra(&format!("clip-path=\"url(#{id})\""));
    }

    fn path(&mut self, shape: PathShape<'_>) {
        let mut attrs = format!("d=\"{}\"", path_data(shape.path));
        // A mesh has no SVG 1.1 paint, so its triangles are drawn under this
        // path's fill and clip and the geometry keeps only its stroke. An empty
        // mesh leaves nothing to draw and keeps the mean colour.
        let mesh = match shape.fill {
            Some(PaintShape::Mesh(spec)) if !spec.triangles.is_empty() => Some(spec),
            _ => None,
        };
        match shape.fill {
            Some(PaintShape::Mesh(spec)) if mesh.is_none() => {
                self.stats.paints_approximated += 1;
                let _ = write!(attrs, " fill=\"{}\"", mesh_average(spec));
            }
            Some(_) if mesh.is_some() => attrs.push_str(" fill=\"none\""),
            Some(paint) => {
                let reference = self.paint_ref(paint);
                let _ = write!(attrs, " fill=\"{reference}\"");
                if let PaintShape::Solid(c) = paint
                    && c.a != 255
                {
                    let _ = write!(attrs, " fill-opacity=\"{}\"", opacity(c));
                }
            }
            None => attrs.push_str(" fill=\"none\""),
        }
        if shape.rule == PaintRule::EvenOdd {
            attrs.push_str(" fill-rule=\"evenodd\"");
        }
        match shape.stroke {
            Some(paint) => {
                let reference = self.paint_ref(paint);
                let _ = write!(attrs, " stroke=\"{reference}\"");
                if let PaintShape::Solid(c) = paint
                    && c.a != 255
                {
                    let _ = write!(attrs, " stroke-opacity=\"{}\"", opacity(c));
                }
                attrs.push_str(&stroke_attrs(shape.stroke_style));
            }
            None => attrs.push_str(" stroke=\"none\""),
        }
        // The triangles come first and the outline over them, so a shape that
        // is both mesh-filled and stroked keeps both, in the order an opaque
        // fill followed by its stroke paints them.
        if let Some(spec) = mesh {
            self.mesh_triangles(spec);
        }
        self.line(format!("<path {attrs}/>"));
    }

    fn text(&mut self, run: &GlyphRun) {
        if run.glyphs.is_empty() {
            return;
        }
        let outlined = match self.text {
            TextExport::Outline => self.outline_text(run),
            TextExport::LiveText => false,
        };
        if !outlined {
            self.live_text(run);
        }
    }

    fn image(&mut self, image: &ImageRef, dst: Rect) {
        let Some(href) = data_uri(image) else {
            self.stats.images_skipped += 1;
            return;
        };
        self.stats.images += 1;
        let (iw, ih) = image.pixel_size;
        let (iw, ih) = (iw.max(1) as f64, ih.max(1) as f64);
        let place = Affine::new(dst.w / iw, 0.0, 0.0, dst.h / ih, dst.x, dst.y);
        // The element spans the whole bitmap, so the rectangle form needs the
        // scaled pixel extent; anything mirrored or rotated takes the matrix
        // form, where the element keeps its natural pixel size.
        if place.b.abs() < 1e-9 && place.c.abs() < 1e-9 && place.a > 0.0 && place.d > 0.0 {
            self.line(format!(
                "<image href=\"{href}\" x=\"{}\" y=\"{}\" width=\"{}\" height=\"{}\"/>",
                num(place.e),
                num(place.f),
                num(place.a * iw),
                num(place.d * ih)
            ));
        } else {
            self.line(format!(
                "<image href=\"{href}\" width=\"{}\" height=\"{}\" transform=\"matrix({})\"/>",
                num(iw),
                num(ih),
                matrix_attr(place)
            ));
        }
    }
}

impl SvgSink<'_> {
    /// Outline every glyph into vector paths, reporting whether any was drawn.
    fn outline_text(&mut self, run: &GlyphRun) -> bool {
        let Some(fonts) = self.fonts else {
            return false;
        };
        let Some(face) = sofd_display::run_face(fonts, run) else {
            return false;
        };
        // Either paint alone is enough: filling needs a fill, stroking needs an
        // outline to stroke along, and a run with both gets one path and two
        // attributes. Only a run with neither has nothing to say.
        if run.fill.is_none() && run.stroke.is_none() {
            return false;
        }
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
            // Outlines are normalized to 1/1000 em with y pointing up, and the
            // run's own matrix scales, turns and places them from there.
            let glyph = run.glyph_matrix(offset);
            let path = outline.transformed(&glyph);
            let mut attrs = format!("d=\"{}\"", path_data(&path));
            match &run.fill {
                Some(paint) => {
                    let shape = text_paint_shape(paint);
                    let reference = self.paint_ref(shape);
                    let _ = write!(attrs, " fill=\"{reference}\"");
                    if let PaintShape::Solid(c) = shape
                        && c.a != 255
                    {
                        let _ = write!(attrs, " fill-opacity=\"{}\"", opacity(c));
                    }
                }
                None => attrs.push_str(" fill=\"none\""),
            }
            match &run.stroke {
                Some(sofd_display::Paint::Solid(c)) => {
                    let _ = write!(attrs, " stroke=\"{}\"", hex(c));
                    if c.a != 255 {
                        let _ = write!(attrs, " stroke-opacity=\"{}\"", opacity(c));
                    }
                    attrs.push_str(&stroke_attrs(&run.stroke_style));
                }
                Some(_) => {
                    attrs.push_str(" stroke=\"#000000\"");
                    attrs.push_str(&stroke_attrs(&run.stroke_style));
                }
                None => attrs.push_str(" stroke=\"none\""),
            }
            self.line(format!("<path {attrs}/>"));
            drawn += 1;
        }
        if missing > 0 {
            self.stats.missing_glyph_runs += 1;
            self.stats.missing_glyphs += missing;
        }
        if run.fill.is_none() && run.stroke.is_some() {
            self.stats.stroke_only_glyph_runs += 1;
        }
        if drawn > 0 {
            self.stats.outlined_glyphs += drawn;
            true
        } else {
            false
        }
    }

    /// Emit the run's Unicode text and let the viewer pick a font.
    fn live_text(&mut self, run: &GlyphRun) {
        let Some(text) = run.text.as_deref().filter(|t| !t.is_empty()) else {
            self.stats.dropped_runs += 1;
            return;
        };
        self.stats.live_text_runs += 1;
        let placement = if run.transform.is_translation_only() {
            format!(
                "x=\"{}\" y=\"{}\"",
                num(run.transform.e),
                num(run.transform.f)
            )
        } else {
            format!("transform=\"matrix({})\"", matrix_attr(run.transform))
        };
        let fill = match &run.fill {
            Some(paint) => self.paint_ref(text_paint_shape(paint)),
            None => "#000000".to_string(),
        };
        self.line(format!(
            "<text {placement} font-size=\"{}\" fill=\"{fill}\">{}</text>",
            num(run.size.value()),
            escape_text(text)
        ));
    }
}

/// `#rrggbb` for a colour.
fn hex(c: &Color) -> String {
    format!("#{:02x}{:02x}{:02x}", c.r, c.g, c.b)
}

/// The mean of a mesh's vertex colours, used where a mesh gradient cannot be
/// written. An empty mesh has no colour to average, so it is left black.
fn mesh_average(spec: &sofd_display::MeshSpec) -> String {
    if spec.points.is_empty() {
        return "#000000".to_string();
    }
    let n = spec.points.len() as u32;
    let sum = spec.points.iter().fold((0u32, 0u32, 0u32), |acc, v| {
        (
            acc.0 + v.color.r as u32,
            acc.1 + v.color.g as u32,
            acc.2 + v.color.b as u32,
        )
    });
    let mean = Color::rgba((sum.0 / n) as u8, (sum.1 / n) as u8, (sum.2 / n) as u8, 255);
    hex(&mean)
}

/// The three vertices of a triangle, or nothing when one index is out of
/// range. A mesh from a damaged document can name a vertex it never defined.
fn tri_vertices(
    spec: &sofd_display::MeshSpec,
    tri: &[usize; 3],
) -> Option<[sofd_display::MeshVertex; 3]> {
    let mut out = [*spec.points.first()?; 3];
    for (slot, index) in out.iter_mut().zip(tri) {
        *slot = *spec.points.get(*index)?;
    }
    Some(out)
}

/// The mean of a triangle's three vertex colours. Nearest-neighbour rather
/// than area-weighted: a mesh from the standard is an even lattice, so the
/// vertices of one triangle are about as far apart as the next one's.
fn triangle_mean(v: &[sofd_display::MeshVertex; 3]) -> Color {
    let mean = |pick: fn(&sofd_display::MeshVertex) -> u8| {
        let sum: u32 = v.iter().map(|p| u32::from(pick(p))).sum();
        (sum / 3) as u8
    };
    Color::rgba(
        mean(|p| p.color.r),
        mean(|p| p.color.g),
        mean(|p| p.color.b),
        255,
    )
}

/// The mean alpha of a triangle's vertices, as a `0..=1` fraction, so a mesh
/// that fades out keeps its transparency once it is one path per triangle.
fn triangle_alpha(v: &[sofd_display::MeshVertex; 3]) -> f64 {
    let sum: u32 = v.iter().map(|p| u32::from(p.color.a)).sum();
    f64::from(sum) / 3.0 / 255.0
}

/// The first opaque fill inside a pattern's cell, used where the repeat cannot
/// be written. An empty cell is left black.
fn pattern_average(spec: &sofd_display::PatternSpec) -> String {
    for op in &spec.cell.ops {
        if let sofd_display::DrawOp::Path {
            fill: Some(sofd_display::Paint::Solid(c)),
            ..
        } = op
        {
            return hex(c);
        }
    }
    "#000000".to_string()
}

/// Alpha as a `0..=1` fraction.
fn opacity(c: &Color) -> String {
    num(c.a as f64 / 255.0)
}

/// Stroke presentation attributes, in millimetres.
fn stroke_attrs(style: &StrokeStyle) -> String {
    let mut attrs = format!(" stroke-width=\"{}\"", num(style.width.max(0.0)));
    if !style.dash.is_empty() {
        let pattern: Vec<String> = style.dash.iter().map(|d| num(*d)).collect();
        let _ = write!(
            attrs,
            " stroke-dasharray=\"{}\" stroke-dashoffset=\"{}\"",
            pattern.join(" "),
            num(style.dash_offset)
        );
    }
    let cap = match style.cap {
        LineCap::Butt => "butt",
        LineCap::Round => "round",
        LineCap::Square => "square",
    };
    let join = match style.join {
        LineJoin::Miter => "miter",
        LineJoin::Round => "round",
        LineJoin::Bevel => "bevel",
    };
    let _ = write!(
        attrs,
        " stroke-linecap=\"{cap}\" stroke-linejoin=\"{join}\" stroke-miterlimit=\"{}\"",
        num(style.miter_limit)
    );
    attrs
}

/// `<stop>` children of a gradient element.
fn stops_markup(stops: &[sofd_display::ops::GradientStop]) -> String {
    let mut out = String::new();
    for stop in stops {
        let _ = write!(
            out,
            "<stop offset=\"{}\" stop-color=\"{}\" stop-opacity=\"{}\"/>",
            num(stop.offset.clamp(0.0, 1.0)),
            hex(&stop.color),
            opacity(&stop.color)
        );
    }
    out
}

/// The outline of every glyph in a run, in page space, in draw order. Outlines
/// are normalized to 1/1000 em with y up, so scale to millimetres then place.
fn glyph_paths(run: &GlyphRun, face: &sofd_font::FontFace) -> Vec<Path> {
    let mut paths = Vec::new();
    for (&gid, offset) in run.glyphs.iter().zip(run.glyph_offsets()) {
        let Some(outline) = face.outline_for_run_code(gid, run.codes_are_glyphs) else {
            continue;
        };
        let glyph = run.glyph_matrix(offset);
        paths.push(outline.transformed(&glyph));
    }
    paths
}

/// SVG path data, millimetre coordinates written verbatim.
///
/// Public because a canvas takes the same `d`: `Path2D` in a browser, and a
/// shell that draws geometry rather than a bitmap needs this without the rest
/// of an SVG document around it.
pub fn path_data(path: &Path) -> String {
    let mut d = String::new();
    for seg in &path.segments {
        match seg {
            PathSegment::MoveTo(x, y) => {
                let _ = write!(d, "M{} {}", num(*x), num(*y));
            }
            PathSegment::LineTo(x, y) => {
                let _ = write!(d, "L{} {}", num(*x), num(*y));
            }
            PathSegment::CurveTo { c1, c2, to } => {
                let _ = write!(
                    d,
                    "C{} {} {} {} {} {}",
                    num(c1.0),
                    num(c1.1),
                    num(c2.0),
                    num(c2.1),
                    num(to.0),
                    num(to.1)
                );
            }
            PathSegment::QuadTo { c, to } => {
                let _ = write!(d, "Q{} {} {} {}", num(c.0), num(c.1), num(to.0), num(to.1));
            }
            PathSegment::Arc {
                rx,
                ry,
                x_axis_rotation,
                large_arc,
                sweep,
                to,
            } => {
                // OFD path data follows SVG arc semantics in the same y-down
                // space, so the flags carry over unchanged.
                let _ = write!(
                    d,
                    "A{} {} {} {} {} {} {}",
                    num(*rx),
                    num(*ry),
                    num(*x_axis_rotation),
                    u8::from(*large_arc),
                    u8::from(*sweep),
                    num(to.0),
                    num(to.1)
                );
            }
            PathSegment::Close => d.push('Z'),
        }
        d.push(' ');
    }
    d.truncate(d.trim_end().len());
    d
}

/// Matrix operands for an SVG `matrix()` transform.
fn matrix_attr(m: Affine) -> String {
    format!(
        "{} {} {} {} {} {}",
        num(m.a),
        num(m.b),
        num(m.c),
        num(m.d),
        num(m.e),
        num(m.f)
    )
}

/// Escape text content for XML.
fn escape_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for ch in text.chars() {
        match ch {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            _ => out.push(ch),
        }
    }
    out
}

/// Embed image bytes as a data URI, or `None` for codecs the web rejects.
fn data_uri(image: &ImageRef) -> Option<String> {
    if image.data.is_empty() {
        return None;
    }
    let mime = match image.codec {
        ImageCodec::Jpeg => "image/jpeg",
        ImageCodec::Png => "image/png",
        ImageCodec::Tiff => "image/tiff",
        ImageCodec::Bmp => "image/bmp",
        ImageCodec::Gif => "image/gif",
        ImageCodec::Jbig2 | ImageCodec::Raw => return None,
    };
    Some(format!(
        "data:{mime};base64,{}",
        sofd_common::base64::encode(&image.data)
    ))
}

/// Compact number: at most four decimals, no trailing zeros, no negative zero.
fn num(v: f64) -> String {
    if !v.is_finite() {
        return "0".to_string();
    }
    let mut s = format!("{v:.4}");
    while s.ends_with('0') {
        s.pop();
    }
    if s.ends_with('.') {
        s.pop();
    }
    if s == "-0" {
        s = "0".to_string();
    }
    s
}

/// Convert one page into a standalone SVG document.
pub fn page_to_svg(page: &sofd_display::PageDisplay, options: &SvgOptions<'_>) -> Result<String> {
    Ok(page_to_svg_with_stats(page, options)?.0)
}

/// The same conversion, with what it did to the page: a caller that wants to
/// warn about a face it could not find needs the counts, not just the bytes.
pub fn page_to_svg_with_stats(
    page: &sofd_display::PageDisplay,
    options: &SvgOptions<'_>,
) -> Result<(String, ConversionStats)> {
    let mut sink = SvgSink::new(page.page_box, options);
    replay(page, &mut sink)?;
    Ok(sink.finish())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sofd_common::{Millimeter, Point};
    use sofd_display::DrawOp;
    use sofd_display::MeshSpec;
    use sofd_display::ops::{GradientStop, Paint};
    use sofd_font::FontId;

    /// A mesh vertex at a position, carrying a colour.
    fn vertex(x: f64, y: f64, color: Color) -> sofd_display::MeshVertex {
        sofd_display::MeshVertex { x, y, color }
    }

    /// The vertex colours a mesh triangle carries, read off the XML.
    fn triangle_fills(svg: &str) -> Vec<String> {
        svg.match_indices("<path d=\"M")
            .filter_map(|(i, _)| {
                let tail = &svg[i..];
                let end = tail.find("/>")?;
                let element = &tail[..end];
                if !element.contains("fill-opacity") {
                    return None;
                }
                let start = element.find("fill=\"#")? + 7;
                Some(element[start - 1..start + 6].to_string())
            })
            .collect()
    }

    #[test]
    fn a_mesh_is_exported_as_one_path_per_triangle() {
        // Two triangles over four vertices, each corner a different colour, so
        // one mean for the whole mesh and one per triangle differ visibly.
        let mut page = page(Vec::new());
        page.ops.push(DrawOp::Path {
            path: rect(0.0, 0.0, 40.0, 40.0),
            fill: Some(Paint::Mesh(Box::new(MeshSpec {
                points: vec![
                    vertex(0.0, 0.0, Color::rgba(255, 0, 0, 255)),
                    vertex(40.0, 0.0, Color::rgba(0, 255, 0, 255)),
                    vertex(0.0, 40.0, Color::rgba(0, 0, 255, 255)),
                    vertex(40.0, 40.0, Color::rgba(255, 255, 0, 255)),
                ],
                triangles: vec![[0, 1, 2], [1, 3, 2]],
            }))),
            stroke: None,
            stroke_style: StrokeStyle::default(),
            rule: PaintRule::NonZero,
        });
        let svg = page_to_svg(&page, &SvgOptions::default()).expect("convert");
        let fills = triangle_fills(&svg);
        assert_eq!(fills.len(), 2, "one path per triangle: {svg}");
        // Red, green and blue average to (85, 85, 85); the other triangle is
        // green, yellow and blue, which averages to (85, 170, 85).
        assert_eq!(fills[0], "#555555", "{svg}");
        assert_eq!(fills[1], "#55aa55", "{svg}");
        // The geometry keeps only its stroke, so it does not paint over them.
        assert!(svg.contains("fill=\"none\""), "{svg}");
    }

    #[test]
    fn a_mesh_triangle_out_of_range_is_skipped_not_fatal() {
        let mut page = page(Vec::new());
        page.ops.push(DrawOp::Path {
            path: rect(0.0, 0.0, 40.0, 40.0),
            fill: Some(Paint::Mesh(Box::new(MeshSpec {
                points: vec![
                    vertex(0.0, 0.0, Color::rgba(255, 0, 0, 255)),
                    vertex(40.0, 0.0, Color::rgba(0, 255, 0, 255)),
                    vertex(0.0, 40.0, Color::rgba(0, 0, 255, 255)),
                ],
                // The second triangle names a vertex that was never written.
                triangles: vec![[0, 1, 2], [0, 1, 9]],
            }))),
            stroke: None,
            stroke_style: StrokeStyle::default(),
            rule: PaintRule::NonZero,
        });
        let svg = page_to_svg(&page, &SvgOptions::default()).expect("convert");
        assert_eq!(triangle_fills(&svg).len(), 1, "{svg}");
    }

    #[test]
    fn a_mesh_with_no_triangles_keeps_the_mean_colour() {
        let mut page = page(Vec::new());
        page.ops.push(DrawOp::Path {
            path: rect(0.0, 0.0, 40.0, 40.0),
            fill: Some(Paint::Mesh(Box::new(MeshSpec {
                points: vec![
                    vertex(0.0, 0.0, Color::rgba(0, 0, 0, 255)),
                    vertex(40.0, 40.0, Color::rgba(255, 255, 255, 255)),
                ],
                triangles: Vec::new(),
            }))),
            stroke: None,
            stroke_style: StrokeStyle::default(),
            rule: PaintRule::NonZero,
        });
        let svg = page_to_svg(&page, &SvgOptions::default()).expect("convert");
        assert!(triangle_fills(&svg).is_empty(), "{svg}");
        assert!(svg.contains("fill=\"#7f7f7f\""), "{svg}");
    }

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

    fn page(ops: Vec<DrawOp>) -> sofd_display::PageDisplay {
        sofd_display::PageDisplay {
            index: 0,
            page_box: Rect::A4_PORTRAIT,
            ops,
            content_bbox: Rect::EMPTY,
        }
    }

    #[test]
    fn the_root_element_carries_the_page_box_in_millimetres() {
        let svg = page_to_svg(&page(Vec::new()), &SvgOptions::default()).expect("convert");
        assert!(svg.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
        assert!(svg.contains("width=\"210mm\" height=\"297mm\""));
        assert!(svg.contains("viewBox=\"0 0 210 297\""));
        assert!(svg.ends_with("</svg>\n"));
    }

    #[test]
    fn a_rect_becomes_one_path_with_its_paint() {
        let p = page(vec![DrawOp::Path {
            path: rect(10.0, 20.0, 30.0, 40.0),
            fill: Some(Paint::Solid(Color::rgba(255, 0, 0, 128))),
            stroke: None,
            stroke_style: StrokeStyle::default(),
            rule: PaintRule::NonZero,
        }]);
        let svg = page_to_svg(&p, &SvgOptions::default()).expect("convert");
        assert!(svg.contains("<path d=\"M10 20 L40 20 L40 60 L10 60 Z\" fill=\"#ff0000\" fill-opacity=\"0.502\" stroke=\"none\"/>"));
    }

    #[test]
    fn a_path_without_fill_says_so_explicitly() {
        let p = page(vec![DrawOp::Path {
            path: rect(0.0, 0.0, 1.0, 1.0),
            fill: None,
            stroke: None,
            stroke_style: StrokeStyle::default(),
            rule: PaintRule::NonZero,
        }]);
        let svg = page_to_svg(&p, &SvgOptions::default()).expect("convert");
        assert!(svg.contains("fill=\"none\""));
        assert!(svg.contains("stroke=\"none\""));
    }

    /// Outlined text keeps both paints: the same path is filled and stroked,
    /// which is what SVG already knows how to show.
    #[test]
    fn an_outlined_run_carries_its_fill_and_its_stroke() {
        let mut manager = FontManager::new();
        manager.registry_mut().register_embedded(
            "tests/data/Orbitron-Bold.ttf",
            "Orbitron",
            std::sync::Arc::new(font_bytes()),
        );
        let style = StrokeStyle {
            width: 0.5,
            dash: vec![2.0, 1.0],
            ..StrokeStyle::default()
        };
        let mut run = outlined_run();
        run.stroke = Some(Paint::Solid(Color::rgba(255, 0, 0, 255)));
        run.stroke_style = style;
        let options = SvgOptions {
            text: TextExport::Outline,
            fonts: Some(&manager),
        };
        let mut sink = SvgSink::new(Rect::A4_PORTRAIT, &options);
        replay(&page(vec![DrawOp::Text(run)]), &mut sink).expect("replay");
        let (svg, stats) = sink.finish();
        assert!(svg.contains("stroke=\"#ff0000\""), "got {svg}");
        assert!(svg.contains("stroke-width=\"0.5\""), "got {svg}");
        assert!(svg.contains("stroke-dasharray=\"2 1\""), "got {svg}");
        assert!(svg.contains("fill=\"#000000\""), "the fill is still there");
        assert_eq!(stats.stroke_only_glyph_runs, 0);
    }

    /// A stroke with no fill is still outlined text, not a dropped run.
    #[test]
    fn an_outlined_run_with_only_a_stroke_is_written() {
        let mut manager = FontManager::new();
        manager.registry_mut().register_embedded(
            "tests/data/Orbitron-Bold.ttf",
            "Orbitron",
            std::sync::Arc::new(font_bytes()),
        );
        let mut run = outlined_run();
        run.fill = None;
        run.stroke = Some(Paint::Solid(Color::BLACK));
        let options = SvgOptions {
            text: TextExport::Outline,
            fonts: Some(&manager),
        };
        let mut sink = SvgSink::new(Rect::A4_PORTRAIT, &options);
        replay(&page(vec![DrawOp::Text(run)]), &mut sink).expect("replay");
        let (svg, stats) = sink.finish();
        assert!(svg.contains("fill=\"none\""), "got {svg}");
        assert!(svg.contains("stroke=\"#000000\""), "got {svg}");
        assert!(!stats.is_lossless(), "a counted loss is still a loss");
    }

    /// A run built for the outline tests, filled in black.
    fn outlined_run() -> GlyphRun {
        GlyphRun {
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
            stroke: None,
            stroke_style: Default::default(),
            letter_spacing: 0.0,
            bbox: Rect::EMPTY,
        }
    }

    #[test]
    fn the_even_odd_rule_reaches_the_fill_rule_attribute() {
        let p = page(vec![DrawOp::Path {
            path: rect(0.0, 0.0, 1.0, 1.0),
            fill: Some(Paint::Solid(Color::BLACK)),
            stroke: None,
            stroke_style: StrokeStyle::default(),
            rule: PaintRule::EvenOdd,
        }]);
        let svg = page_to_svg(&p, &SvgOptions::default()).expect("convert");
        assert!(svg.contains("fill-rule=\"evenodd\""));
    }

    #[test]
    fn stroke_parameters_carry_over_in_millimetres() {
        let style = StrokeStyle {
            width: 0.5,
            dash: vec![3.0, 1.0],
            dash_offset: 0.25,
            cap: LineCap::Round,
            join: LineJoin::Bevel,
            miter_limit: 2.0,
        };
        let p = page(vec![DrawOp::Path {
            path: rect(0.0, 0.0, 1.0, 1.0),
            fill: None,
            stroke: Some(Paint::Solid(Color::BLACK)),
            stroke_style: style,
            rule: PaintRule::NonZero,
        }]);
        let svg = page_to_svg(&p, &SvgOptions::default()).expect("convert");
        assert!(svg.contains("stroke-width=\"0.5\""));
        assert!(svg.contains("stroke-dasharray=\"3 1\""));
        assert!(svg.contains("stroke-dashoffset=\"0.25\""));
        assert!(svg.contains("stroke-linecap=\"round\""));
        assert!(svg.contains("stroke-linejoin=\"bevel\""));
        assert!(svg.contains("stroke-miterlimit=\"2\""));
    }

    #[test]
    fn transforms_nest_as_groups_that_restore_closes() {
        let p = page(vec![
            DrawOp::Save,
            DrawOp::Transform(Affine::translate(5.0, 6.0)),
            DrawOp::Restore,
        ]);
        let svg = page_to_svg(&p, &SvgOptions::default()).expect("convert");
        assert!(svg.contains("<g transform=\"matrix(1 0 0 1 5 6)\">"));
        // The group count balances: every opened group is closed again.
        assert_eq!(svg.matches("<g").count(), svg.matches("</g>").count());
    }

    #[test]
    fn a_clip_emits_a_definition_and_a_referencing_group() {
        let p = page(vec![
            DrawOp::Save,
            DrawOp::ClipPath {
                path: rect(0.0, 0.0, 10.0, 10.0),
                rule: PaintRule::EvenOdd,
            },
            DrawOp::Restore,
        ]);
        let svg = page_to_svg(&p, &SvgOptions::default()).expect("convert");
        assert!(svg.contains("<clipPath id=\"clip1\" clipPathUnits=\"userSpaceOnUse\">"));
        assert!(svg.contains("clip-rule=\"evenodd\""));
        assert!(svg.contains("<g clip-path=\"url(#clip1)\">"));
    }

    #[test]
    fn a_text_clip_emits_the_glyph_outlines_as_a_clip_path() {
        // Clause 8.4: a text area clips to the glyph outlines, so the SVG
        // carries the outlines inside the clipPath rather than a path.
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
            fill: None,
            stroke: None,
            stroke_style: Default::default(),
            letter_spacing: 0.0,
            bbox: Rect::EMPTY,
        };
        let options = SvgOptions {
            text: TextExport::Outline,
            fonts: Some(&manager),
        };
        let mut sink = SvgSink::new(Rect::A4_PORTRAIT, &options);
        replay(
            &page(vec![DrawOp::Save, DrawOp::ClipText(run), DrawOp::Restore]),
            &mut sink,
        )
        .expect("replay");
        let (svg, _) = sink.finish();
        assert!(
            svg.contains("<clipPath id=\"clip1\" clipPathUnits=\"userSpaceOnUse\"><path d=\""),
            "the glyph outlines become the clip path, got {svg}"
        );
        assert!(svg.contains("<g clip-path=\"url(#clip1)\">"));
    }

    #[test]
    fn a_pattern_becomes_a_tiling_definition() {
        // SVG's pattern is the clause's model: the element spans one step and
        // the cell repeats across it, so the cell's own path reaches the def.
        let cell = sofd_display::PageDisplay {
            index: 0,
            page_box: Rect::new(0.0, 0.0, 4.0, 4.0),
            ops: vec![DrawOp::Path {
                path: rect(0.0, 0.0, 2.0, 2.0),
                fill: Some(Paint::Solid(Color::rgba(255, 0, 0, 255))),
                stroke: None,
                stroke_style: Default::default(),
                rule: PaintRule::NonZero,
            }],
            content_bbox: Rect::EMPTY,
        };
        let p = page(vec![DrawOp::Path {
            path: rect(0.0, 0.0, 20.0, 20.0),
            fill: Some(Paint::Pattern(Box::new(sofd_display::PatternSpec {
                cell: Box::new(cell),
                width: 4.0,
                height: 4.0,
                step_x: 6.0,
                step_y: 6.0,
                reflect: sofd_display::Reflect::Normal,
            }))),
            stroke: None,
            stroke_style: Default::default(),
            rule: PaintRule::NonZero,
        }]);
        let svg = page_to_svg(&p, &SvgOptions::default()).expect("convert");
        assert!(
            svg.contains("<pattern id=\"pat1\" patternUnits=\"userSpaceOnUse\""),
            "a pattern def is written: {svg}"
        );
        // The step, not the tile size, is the element's span.
        assert!(
            svg.contains("width=\"6\" height=\"6\""),
            "the step sets the repeat: {svg}"
        );
        assert!(
            svg.contains("fill=\"#ff0000\""),
            "the cell's own fill is inside the def: {svg}"
        );
        assert!(
            svg.contains("fill=\"url(#pat1)\""),
            "the path references the pattern: {svg}"
        );
    }

    #[test]
    fn a_reflecting_pattern_tiles_mirrored_copies() {
        // SVG repeats a tile unchanged, so a reflecting pattern is laid down as
        // a 2x2 block of mirrored copies that then tiles normally: the block is
        // two steps wide and tall, and the copies carry a negative scale.
        let cell = sofd_display::PageDisplay {
            index: 0,
            page_box: Rect::new(0.0, 0.0, 4.0, 4.0),
            ops: vec![DrawOp::Path {
                path: rect(0.0, 0.0, 2.0, 2.0),
                fill: Some(Paint::Solid(Color::rgba(0, 128, 0, 255))),
                stroke: None,
                stroke_style: Default::default(),
                rule: PaintRule::NonZero,
            }],
            content_bbox: Rect::EMPTY,
        };
        let p = page(vec![DrawOp::Path {
            path: rect(0.0, 0.0, 20.0, 20.0),
            fill: Some(Paint::Pattern(Box::new(sofd_display::PatternSpec {
                cell: Box::new(cell),
                width: 4.0,
                height: 4.0,
                step_x: 4.0,
                step_y: 4.0,
                reflect: sofd_display::Reflect::RowAndColumn,
            }))),
            stroke: None,
            stroke_style: Default::default(),
            rule: PaintRule::NonZero,
        }]);
        let svg = page_to_svg(&p, &SvgOptions::default()).expect("convert");
        assert!(
            svg.contains("<pattern id=\"pat1\""),
            "a reflecting pattern still tiles: {svg}"
        );
        // A 2x2 block spans two steps in each direction.
        assert!(
            svg.contains("width=\"8\" height=\"8\""),
            "the block spans two steps: {svg}"
        );
        assert!(
            svg.contains("scale(-1,-1)"),
            "the diagonal copy is mirrored: {svg}"
        );
        assert!(svg.contains("fill=\"#008000\""), "the cell is drawn: {svg}");
    }

    #[test]
    fn a_linear_gradient_becomes_a_def_and_a_fill() {
        let spec = LinearGradientSpec {
            start: Point::new(0.0, 0.0),
            end: Point::new(100.0, 0.0),
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
        };
        let p = page(vec![DrawOp::Path {
            path: rect(0.0, 0.0, 100.0, 50.0),
            fill: Some(Paint::LinearGradient(Box::new(spec))),
            stroke: None,
            stroke_style: Default::default(),
            rule: PaintRule::NonZero,
        }]);
        let svg = page_to_svg(&p, &SvgOptions::default()).expect("convert");
        assert!(svg.contains("<linearGradient id=\"grad1\" gradientUnits=\"userSpaceOnUse\" x1=\"0\" y1=\"0\" x2=\"100\" y2=\"0\">"));
        assert!(svg.contains("<stop offset=\"0\" stop-color=\"#ffffff\" stop-opacity=\"1\"/>"));
        assert!(svg.contains("<stop offset=\"1\" stop-color=\"#000000\" stop-opacity=\"1\"/>"));
        assert!(svg.contains("fill=\"url(#grad1)\""));
    }

    #[test]
    fn a_radial_gradient_becomes_a_def_and_a_fill() {
        let spec = RadialGradientSpec {
            center: Point::new(10.0, 10.0),
            radius: 0.0,
            end: Point::new(10.0, 10.0),
            end_radius: 25.0,
            stops: vec![GradientStop {
                offset: 0.5,
                color: Color::WHITE,
            }],
        };
        let p = page(vec![DrawOp::Path {
            path: rect(0.0, 0.0, 20.0, 20.0),
            fill: Some(Paint::RadialGradient(Box::new(spec))),
            stroke: None,
            stroke_style: Default::default(),
            rule: PaintRule::NonZero,
        }]);
        let svg = page_to_svg(&p, &SvgOptions::default()).expect("convert");
        // The clause's start circle is a point here, so the cone reduces to
        // the plain radial SVG paints: the circle at the far end, the point
        // at the near one.
        assert!(svg.contains("<radialGradient id=\"grad1\" gradientUnits=\"userSpaceOnUse\" cx=\"10\" cy=\"10\" r=\"25\" fx=\"10\" fy=\"10\">"));
        assert!(svg.contains("fill=\"url(#grad1)\""));
    }

    #[test]
    fn an_image_embeds_as_a_data_uri_rectangle() {
        let p = page(vec![DrawOp::Image {
            image: ImageRef::new(
                "i",
                std::sync::Arc::new(vec![1, 2, 3, 4]),
                (2, 2),
                ImageCodec::Png,
            ),
            dst: Rect::new(5.0, 6.0, 7.0, 8.0),
        }]);
        let svg = page_to_svg(&p, &SvgOptions::default()).expect("convert");
        assert!(svg.contains("<image href=\"data:image/png;base64,AQIDBA==\" x=\"5\" y=\"6\" width=\"7\" height=\"8\"/>"));
    }

    #[test]
    fn live_text_uses_the_run_transform_and_size() {
        let run = GlyphRun {
            font: FontId(1),
            size: Millimeter(3.5),
            glyphs: vec![1, 2],
            advances: vec![3.5, 3.5],
            text: Some("a<b".into()),
            cluster_map: None,
            codes_are_glyphs: false,
            transform: Affine::translate(12.0, 30.0),
            glyph_angle: 0.0,
            fill: Some(Paint::Solid(Color::BLACK)),
            stroke: None,
            stroke_style: Default::default(),
            letter_spacing: 0.0,
            bbox: Rect::new(0.0, 0.0, 7.0, 3.5),
        };
        let svg =
            page_to_svg(&page(vec![DrawOp::Text(run)]), &SvgOptions::default()).expect("convert");
        assert!(
            svg.contains(
                "<text x=\"12\" y=\"30\" font-size=\"3.5\" fill=\"#000000\">a&lt;b</text>"
            )
        );
    }

    #[test]
    fn a_rotated_run_emits_a_matrix_instead_of_x_and_y() {
        let run = GlyphRun {
            font: FontId(1),
            size: Millimeter(3.0),
            glyphs: vec![1],
            advances: vec![3.0],
            text: Some("A".into()),
            cluster_map: None,
            codes_are_glyphs: false,
            transform: Affine::translate(1.0, 2.0).then(Affine::rotate_deg(90.0)),
            glyph_angle: 0.0,
            fill: None,
            stroke: None,
            stroke_style: Default::default(),
            letter_spacing: 0.0,
            bbox: Rect::new(0.0, 0.0, 3.0, 3.0),
        };
        let svg =
            page_to_svg(&page(vec![DrawOp::Text(run)]), &SvgOptions::default()).expect("convert");
        // translate(1, 2) acts after the rotation, so the glyph origin lands at
        // (-2, 1) rather than (1, 2).
        assert!(svg.contains("transform=\"matrix(0 1 -1 0 -2 1)\""));
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
            bbox: Rect::EMPTY,
        };
        let mut sink = SvgSink::new(Rect::A4_PORTRAIT, &SvgOptions::default());
        replay(&page(vec![DrawOp::Text(run)]), &mut sink).expect("replay");
        let (svg, stats) = sink.finish();
        assert!(!svg.contains("<text"));
        assert_eq!(stats.dropped_runs, 1);
    }

    #[test]
    fn a_run_missing_a_glyph_is_counted_and_the_rest_still_draws() {
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
            bbox: Rect::new(0.0, 0.0, 6.0, 3.0),
        };
        let options = SvgOptions {
            text: TextExport::Outline,
            fonts: Some(&manager),
        };
        let mut sink = SvgSink::new(Rect::A4_PORTRAIT, &options);
        replay(&page(vec![DrawOp::Text(run)]), &mut sink).expect("replay");
        let (svg, stats) = sink.finish();
        assert_eq!(stats.missing_glyph_runs, 1);
        assert_eq!(stats.outlined_glyphs, 1, "the drawable glyph still landed");
        assert!(!stats.is_lossless(), "a short run is not a lossless page");
        assert!(svg.contains("<path"));
    }

    #[test]
    fn an_empty_run_emits_nothing_at_all() {
        let run = GlyphRun {
            font: FontId(1),
            size: Millimeter(3.0),
            glyphs: Vec::new(),
            advances: Vec::new(),
            text: Some(String::new()),
            cluster_map: None,
            codes_are_glyphs: false,
            transform: Affine::IDENTITY,
            glyph_angle: 0.0,
            fill: None,
            stroke: None,
            stroke_style: Default::default(),
            letter_spacing: 0.0,
            bbox: Rect::EMPTY,
        };
        let svg =
            page_to_svg(&page(vec![DrawOp::Text(run)]), &SvgOptions::default()).expect("convert");
        assert!(!svg.contains("<text"));
    }

    #[test]
    fn an_unbalanced_restore_is_an_error() {
        let err = page_to_svg(&page(vec![DrawOp::Restore]), &SvgOptions::default())
            .expect_err("an unbalanced restore must fail");
        assert!(matches!(
            err,
            sofd_error::OfdError::Format(sofd_error::FormatError::CircularReference { .. })
        ));
    }

    #[test]
    fn a_trailing_save_still_produces_balanced_markup() {
        let svg = page_to_svg(&page(vec![DrawOp::Save]), &SvgOptions::default()).expect("convert");
        assert_eq!(svg.matches("<g").count(), svg.matches("</g>").count());
    }

    #[test]
    fn arcs_carry_their_flags_into_the_path_data() {
        let mut path = Path::new();
        path.segments.push(PathSegment::MoveTo(0.0, 0.0));
        path.segments.push(PathSegment::Arc {
            rx: 5.0,
            ry: 5.0,
            x_axis_rotation: 30.0,
            large_arc: true,
            sweep: false,
            to: (10.0, 10.0),
        });
        let svg = page_to_svg(
            &page(vec![DrawOp::Path {
                path,
                fill: None,
                stroke: None,
                stroke_style: StrokeStyle::default(),
                rule: PaintRule::NonZero,
            }]),
            &SvgOptions::default(),
        )
        .expect("convert");
        assert!(svg.contains("A5 5 30 1 0 10 10"));
    }

    #[test]
    fn numbers_strip_trailing_zeros_and_negative_zero() {
        assert_eq!(num(2.0), "2");
        assert_eq!(num(0.5), "0.5");
        assert_eq!(num(-0.00001), "0");
        assert_eq!(num(1.0 / 3.0), "0.3333");
        assert_eq!(num(f64::NAN), "0");
    }

    #[test]
    fn text_content_is_escaped() {
        assert_eq!(escape_text("a&b<c>d"), "a&amp;b&lt;c&gt;d");
    }
}
