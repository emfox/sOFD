//! Flattening of the model tree into display commands.
//! Every object is emitted with its CTM already folded into the graphics
//! state, so downstream backends see absolute page-space commands only.

use std::collections::HashMap;
use std::sync::Arc;

use sofd_common::{Point, Rect};
use sofd_display::ops::{Color, GradientStop, LineCap, LineJoin};
use sofd_display::{
    DisplayListBuilder, DrawOp, GlyphRun, ImageCodec, ImageRef, LinearGradientSpec, MeshSpec,
    Paint, PaintRule, RadialGradientSpec, StrokeStyle,
};
use sofd_error::{FormatError, OfdError, Result};
use sofd_font::FontId;
use sofd_geometry::{Affine, Path};
use sofd_model::PageBlock;
use sofd_model::color::{ColorRef, PatternSpec, ReflectMethod, Shading};
use sofd_model::object::{
    CGTransform, CharDirection, DrawParam, ImageObject, LineCapStyle, LineJoinStyle, Object,
    ObjectBase, PathObject, ReadDirection, TextCode, TextObject,
};
use sofd_xml::parse_number_list_n;

use crate::color::{mesh_vertices, solid, stops_of};

/// Default stroke width in millimetres when neither the object nor its draw
/// parameter says otherwise; the standard names 0.35 mm as the default.
const DEFAULT_LINE_WIDTH: f64 = 0.35;

/// The vector graphics a document embeds, keyed by resource id. A
/// `CompositeObject` names one of these through its `ResourceID`.
pub type CompositeTable = HashMap<String, sofd_model::CompositeGraphicUnit>;

/// An empty composite table, for callers that have no document behind them.
pub fn no_composites() -> CompositeTable {
    CompositeTable::new()
}

/// The colour spaces a document declares, keyed by resource id. Clause 7.13:
/// a colour naming one is read in that space, not as sRGB.
pub type ColorTable = HashMap<String, sofd_model::ColorSpace>;

/// An empty colour table, for callers that have no document behind them.
pub fn no_colors() -> ColorTable {
    ColorTable::new()
}

/// Table of a document's colour spaces, keyed by resource id.
pub fn color_table(list: &sofd_model::ColorSpaceList) -> ColorTable {
    list.items
        .iter()
        .map(|c| (c.id.as_str().to_string(), c.clone()))
        .collect()
}

/// Flatten a whole page or template block onto the builder.
pub fn flatten_block(
    block: &PageBlock,
    params: &HashMap<String, DrawParam>,
    colors: &ColorTable,
    composites: &CompositeTable,
    builder: &mut DisplayListBuilder,
) -> Result<()> {
    for layer in &block.layers {
        // A layer may name a draw parameter for the objects inside it. One
        // that names its own still wins, so this only fills the silence.
        for object in &layer.objects {
            let inherited = layer.draw_param.as_deref();
            flatten_in_layer(object, inherited, params, colors, composites, builder)?;
        }
    }
    Ok(())
}

/// Flatten one object with the layer's draw parameter as a fallback.
///
/// Clause 8.5 puts `DrawParam` on every graphic unit, a layer included, so an
/// object that states none takes the one its layer states. One that states its
/// own keeps it, and a layer name that resolves to nothing leaves the object
/// as it stands rather than failing the page.
fn flatten_in_layer(
    object: &Object,
    inherited: Option<&str>,
    params: &HashMap<String, DrawParam>,
    colors: &ColorTable,
    composites: &CompositeTable,
    builder: &mut DisplayListBuilder,
) -> Result<()> {
    let Some(id) = inherited else {
        return flatten_object(object, params, colors, composites, builder);
    };
    if base_of(object).is_some_and(|b| b.draw_param.is_some()) {
        return flatten_object(object, params, colors, composites, builder);
    }
    let Some(param) = params.get(id) else {
        return flatten_object(object, params, colors, composites, builder);
    };
    let mut object = object.clone();
    if let Some(base) = base_of_mut(&mut object) {
        base.draw_param = Some(param.id.clone());
    }
    flatten_object(&object, params, colors, composites, builder)
}

fn flatten_object(
    object: &Object,
    params: &HashMap<String, DrawParam>,
    colors: &ColorTable,
    composites: &CompositeTable,
    builder: &mut DisplayListBuilder,
) -> Result<()> {
    // GB/T 33190 clause 8.5: Visible defaults to true and false hides the object.
    if visible_of(object) == Some(false) {
        return Ok(());
    }
    match object {
        Object::PathObject(p) => flatten_path(p, params, colors, builder),
        Object::TextObject(t) => flatten_text(t, params, colors, builder),
        Object::ImageObject(i) => flatten_image(i, builder),
        Object::CompositeObject(c) => flatten_composite(c, params, colors, composites, builder),
        // Table 3 gives a block the same choice a layer holds, so its objects
        // ride the block's own boundary and CTM before theirs apply.
        Object::PageBlockObject(b) => with_object_space(&b.base, params, builder, |scope| {
            for object in &b.objects {
                flatten_object(object, params, colors, composites, scope)?;
            }
            Ok(())
        }),
        Object::Unknown(_) => Ok(()),
    }
}

/// GB/T 33190 clause 8.5: every object exposes the same base attributes.
fn visible_of(object: &Object) -> Option<bool> {
    base_of(object).and_then(|base| base.visible)
}

/// The base attributes of whichever object this is, which every kind shares.
fn base_of(object: &Object) -> Option<&ObjectBase> {
    match object {
        Object::PathObject(o) => Some(&o.base),
        Object::TextObject(o) => Some(&o.base),
        Object::ImageObject(o) => Some(&o.base),
        Object::CompositeObject(o) => Some(&o.base),
        Object::PageBlockObject(o) => Some(&o.base),
        Object::Unknown(_) => None,
    }
}

/// The same base, for filling in what the layer supplies.
fn base_of_mut(object: &mut Object) -> Option<&mut ObjectBase> {
    match object {
        Object::PathObject(o) => Some(&mut o.base),
        Object::TextObject(o) => Some(&mut o.base),
        Object::ImageObject(o) => Some(&mut o.base),
        Object::CompositeObject(o) => Some(&mut o.base),
        Object::PageBlockObject(o) => Some(&mut o.base),
        Object::Unknown(_) => None,
    }
}

fn resolve_param<'p>(
    base: &ObjectBase,
    params: &'p HashMap<String, DrawParam>,
) -> Result<Option<&'p DrawParam>> {
    match &base.draw_param {
        Some(id) => params.get(id).map(Some).ok_or_else(|| {
            OfdError::Format(FormatError::DanglingReference {
                kind: "DrawParam",
                id: id.clone(),
            })
        }),
        None => Ok(None),
    }
}

fn ctm_of(base: &ObjectBase) -> Result<Option<Affine>> {
    match &base.ctm {
        None => Ok(None),
        Some(raw) => {
            let v = parse_number_list_n::<6>("Object", "CTM", raw)?;
            Ok(Some(Affine::new(v[0], v[1], v[2], v[3], v[4], v[5])))
        }
    }
}

/// An object's clip, which clause 8.4 lets be a path or a stretch of text.
enum ClipSpec {
    /// A region bounded by path geometry.
    Path { path: Path, rule: PaintRule },
    /// A region bounded by glyph outlines, left for a backend to resolve.
    Text(Vec<GlyphRun>),
}

/// Place an object into the page. Content space starts at the boundary's
/// top-left corner and the CTM maps from there into page space, which every
/// sampled producer agrees on.
fn with_object_space(
    base: &ObjectBase,
    params: &HashMap<String, DrawParam>,
    builder: &mut DisplayListBuilder,
    body: impl FnOnce(&mut DisplayListBuilder) -> Result<()>,
) -> Result<()> {
    let ctm = ctm_of(base)?;
    let origin = Affine::translate(base.boundary.x, base.boundary.y);
    let clip = clip_of(base, params)?;
    // One scope carries both matrices: the CTM takes the content into
    // millimetres from the boundary corner, the boundary then moves that
    // corner onto the page. The clip shares that space and rides the scope.
    let single = match ctm {
        Some(t) => t.then(origin),
        None => origin,
    };
    if single.is_identity() && clip.is_none() {
        return body(builder);
    }
    // Keep the state stack balanced even when the body fails so later objects
    // start from a clean state.
    builder.save()?;
    if !single.is_identity() {
        builder.transform(single);
    }
    let result = match clip {
        None => body(builder),
        Some(ClipSpec::Path { path, rule }) => {
            builder.push(DrawOp::ClipPath { path, rule });
            body(builder)
        }
        Some(ClipSpec::Text(runs)) => {
            for run in runs {
                builder.push(DrawOp::ClipText(run));
            }
            body(builder)
        }
    };
    builder.restore()?;
    result
}

/// Resolve a colour into a paint. A colour carrying a shading becomes a
/// gradient paint that travels with the object, so no backend depends on
/// having seen some earlier op to know what the fill is.
fn paint_of(
    color: &ColorRef,
    _path: &Path,
    colors: &ColorTable,
    _builder: &mut DisplayListBuilder,
) -> Result<Paint> {
    match color.shading() {
        None => solid(color, colors),
        Some(Shading::Axial {
            start,
            end,
            segments,
        }) => Ok(Paint::LinearGradient(Box::new(LinearGradientSpec {
            start: Point::new(start.0, start.1),
            end: Point::new(end.0, end.1),
            stops: stops_of(segments)?,
        }))),
        Some(Shading::Radial {
            start,
            start_radius,
            end,
            end_radius,
            segments,
        }) => Ok(Paint::RadialGradient(Box::new(RadialGradientSpec {
            center: Point::new(start.0, start.1),
            radius: *start_radius,
            end: Point::new(end.0, end.1),
            end_radius: *end_radius,
            stops: stops_of(segments)?,
        }))),
        Some(Shading::Gouraud {
            points, triangles, ..
        }) => Ok(Paint::Mesh(Box::new(MeshSpec {
            points: mesh_vertices(points)?,
            triangles: triangles.iter().map(|t| [t.a, t.b, t.c]).collect(),
        }))),
        Some(Shading::Pattern(spec)) => Ok(Paint::Pattern(Box::new(pattern_spec(spec)?))),
    }
}

/// Flatten a pattern's cell content once, so a backend only has to tile the
/// finished ops. The cell sits in its own coordinates anchored at the origin,
/// which is what a repeating tile means.
fn pattern_spec(spec: &PatternSpec) -> Result<sofd_display::PatternSpec> {
    let cell_box = Rect::new(0.0, 0.0, spec.width.max(0.0), spec.height.max(0.0));
    let params = HashMap::new();
    let colors = no_colors();
    let composites = no_composites();
    let mut builder = DisplayListBuilder::new(cell_box);
    if let Some(raw) = &spec.ctm {
        let v = parse_number_list_n::<6>("Pattern", "CTM", raw)?;
        builder.transform(Affine::new(v[0], v[1], v[2], v[3], v[4], v[5]));
    }
    let block = PageBlock {
        layers: spec.layers.clone(),
    };
    flatten_block(&block, &params, &colors, &composites, &mut builder)?;
    let cell = builder.finish(0);
    Ok(sofd_display::PatternSpec {
        cell: Box::new(cell),
        width: spec.width,
        height: spec.height,
        step_x: spec.step_x(),
        step_y: spec.step_y(),
        reflect: match spec.reflect {
            ReflectMethod::Normal => sofd_display::Reflect::Normal,
            ReflectMethod::Row => sofd_display::Reflect::Row,
            ReflectMethod::Column => sofd_display::Reflect::Column,
            ReflectMethod::RowAndColumn => sofd_display::Reflect::RowAndColumn,
        },
    })
}

/// GB/T 33190 clause 8.4: the clip areas an object narrows its drawing to.
/// The region is the union of every area of every clip, so paths are read as
/// one path and text areas become clip runs for a backend to outline.
fn clip_of(base: &ObjectBase, _params: &HashMap<String, DrawParam>) -> Result<Option<ClipSpec>> {
    let Some(list) = base.clips.as_ref() else {
        return Ok(None);
    };
    let mut joined = Path::default();
    let mut rule = PaintRule::NonZero;
    let mut rule_stated = false;
    let mut text_runs: Vec<GlyphRun> = Vec::new();
    for clip in &list.items {
        for area in &clip.areas {
            // A text area contributes glyphs, a path area contributes segments;
            // both end up intersecting the same region.
            if let Some(text) = area.text.as_ref() {
                text_runs.extend(text_clip_runs(text));
                continue;
            }
            let Some(data) = area.path_data.as_deref().map(str::trim) else {
                continue;
            };
            if data.is_empty() {
                continue;
            }
            let mut path: Path = crate::pathdata::parse_path(data)?;
            // An area may carry its own CTM, applied to the clip path alone.
            if let Some(raw) = area.ctm.as_deref() {
                let v = parse_number_list_n::<6>("Area", "CTM", raw)?;
                path = path.transformed(&Affine::new(v[0], v[1], v[2], v[3], v[4], v[5]));
            }
            // One path carries one fill rule, so where areas disagree the first
            // one that states a rule decides which side of the outline is in.
            if !rule_stated && let Some(r) = area.rule.as_deref() {
                rule = paint_rule(r);
                rule_stated = true;
            }
            joined.segments.extend(path.segments);
        }
    }
    if !joined.is_empty() {
        return Ok(Some(ClipSpec::Path { path: joined, rule }));
    }
    if !text_runs.is_empty() {
        return Ok(Some(ClipSpec::Text(text_runs)));
    }
    Ok(None)
}

/// Turn a `Text` clip area into the runs a backend draws the outlines from.
/// The shape matches a drawn text object, so the glyph mapping is applied the
/// same way, and a run with no drawable codes is dropped.
fn text_clip_runs(clip: &sofd_model::TextClip) -> Vec<GlyphRun> {
    let size = sofd_common::Millimeter::new(clip.size.unwrap_or(1.0));
    let font = FontId(clip.font.parse::<u32>().unwrap_or(0));
    let mut runs = Vec::new();
    for code in &clip.codes {
        if code.text.is_empty() {
            continue;
        }
        for segment in segments_of(code, &clip.cg_transforms, size.value()) {
            runs.push(GlyphRun {
                font,
                size,
                glyphs: segment.glyphs,
                advances: segment.advances,
                text: Some(segment.text),
                cluster_map: segment.cluster_map,
                codes_are_glyphs: segment.glyph_index,
                transform: Affine::translate(code.x, code.y),
                glyph_angle: 0.0,
                fill: None,
                stroke: None,
                stroke_style: StrokeStyle::default(),
                letter_spacing: 0.0,
                word_spacing: 0.0,
                bbox: Rect::new(0.0, 0.0, 0.0, 0.0),
            });
        }
    }
    runs
}

/// The fill rule an area names; clause 9.1 allows either spelling.
fn paint_rule(raw: &str) -> PaintRule {
    if raw.eq_ignore_ascii_case("even-odd") || raw.eq_ignore_ascii_case("evenodd") {
        PaintRule::EvenOdd
    } else {
        PaintRule::NonZero
    }
}

fn default_color() -> ColorRef {
    ColorRef::from_hex("#000000")
}

/// GB/T 33190 clause 9.1: a graphic unit may state the stroke parameters
/// itself, and those win over the ones its `DrawParam` carries.
fn stroke_style_of(base: &ObjectBase, param: Option<&DrawParam>) -> StrokeStyle {
    let dash = param
        .and_then(|p| p.dash_pattern.as_ref())
        .map(|d| d.pattern.clone())
        .unwrap_or_default();
    let dash_offset = param
        .and_then(|p| p.dash_pattern.as_ref())
        .map(|d| d.phase)
        .unwrap_or(0.0);
    StrokeStyle {
        width: base
            .line_width
            .or_else(|| param.and_then(|p| p.line_width))
            .unwrap_or(DEFAULT_LINE_WIDTH),
        dash,
        dash_offset,
        cap: match base.cap.or_else(|| param.and_then(|p| p.line_cap)) {
            Some(LineCapStyle::Round) => LineCap::Round,
            Some(LineCapStyle::Square) => LineCap::Square,
            _ => LineCap::Butt,
        },
        join: match base.join.or_else(|| param.and_then(|p| p.line_join)) {
            Some(LineJoinStyle::Round) => LineJoin::Round,
            Some(LineJoinStyle::Bevel) => LineJoin::Bevel,
            _ => LineJoin::Miter,
        },
        miter_limit: base.miter_limit.unwrap_or(4.0),
    }
}

fn fill_rule(rule: Option<sofd_model::object::FillRuleStyle>) -> PaintRule {
    match rule {
        Some(sofd_model::object::FillRuleStyle::EvenOdd) => PaintRule::EvenOdd,
        _ => PaintRule::NonZero,
    }
}

/// The paint an object ends up with, resolved the one way clause 12.2 describes:
/// what the object writes, else what the draw parameter it names writes, else
/// the default for its kind. Only the default differs between kinds, and annex A
/// sets the two in opposite directions, so the order is stated once here and the
/// pair passed in.
fn resolve_paint(
    base: &ObjectBase,
    param: Option<&DrawParam>,
    fallback: (bool, bool),
) -> (bool, bool) {
    let on = |own: Option<bool>, from_param: Option<bool>, otherwise: bool| {
        own.or(from_param).unwrap_or(otherwise)
    };
    (
        on(base.fill, param.and_then(|d| d.fill), fallback.0),
        on(base.stroke, param.and_then(|d| d.stroke), fallback.1),
    )
}

/// The colour a paint uses, resolved the same way as its switch: the object's
/// own, else the draw parameter's, else black.
fn resolve_paint_color<'a>(
    own: Option<&'a ColorRef>,
    param: Option<&'a DrawParam>,
    which: fn(&'a DrawParam) -> Option<&'a ColorRef>,
    black: &'a ColorRef,
) -> &'a ColorRef {
    own.or_else(|| param.and_then(which)).unwrap_or(black)
}

/// Scale a paint's alpha by the object's own `Alpha`, which multiplies rather
/// than replaces what the colour already states.
///
/// A backplate written `Alpha="0"` is the reason: read as opaque, it covers
/// everything the page drew before it, a seal under a heading included.
fn faded(paint: Paint, object_alpha: Option<u8>) -> Paint {
    let Some(factor) = object_alpha else {
        return paint;
    };
    if factor == u8::MAX {
        return paint;
    }
    let scale = factor as u32;
    let dim = |c: u8| ((c as u32 * scale + 127) / 255) as u8;
    match paint {
        Paint::Solid(c) => Paint::Solid(Color::rgba(c.r, c.g, c.b, dim(c.a))),
        // A gradient's stops carry the alpha a backend paints with.
        Paint::LinearGradient(mut g) => {
            g.stops = g
                .stops
                .into_iter()
                .map(|stop| GradientStop {
                    color: Color::rgba(stop.color.r, stop.color.g, stop.color.b, dim(stop.color.a)),
                    ..stop
                })
                .collect();
            Paint::LinearGradient(g)
        }
        Paint::RadialGradient(mut g) => {
            g.stops = g
                .stops
                .into_iter()
                .map(|stop| GradientStop {
                    color: Color::rgba(stop.color.r, stop.color.g, stop.color.b, dim(stop.color.a)),
                    ..stop
                })
                .collect();
            Paint::RadialGradient(g)
        }
        other => other,
    }
}

fn flatten_path(
    p: &PathObject,
    params: &HashMap<String, DrawParam>,
    colors: &ColorTable,
    builder: &mut DisplayListBuilder,
) -> Result<()> {
    let param = resolve_param(&p.base, params)?;
    let data = p.abbreviated_data.trim();
    if data.is_empty() {
        return Err(OfdError::Format(FormatError::MissingPathData));
    }
    let path: Path = crate::pathdata::parse_path(data)?;
    // Annex A defaults `Stroke` true and `Fill` false. A producer that writes
    // only `FillColor` is drawing a line, and reading the pair the other way
    // round turns every such line into a zero-area fill that shows nothing.
    let (fill_on, stroke_on) = resolve_paint(&p.base, param, (false, true));
    let style = stroke_style_of(&p.base, param);
    let black = default_color();
    let fill_color = resolve_paint_color(
        p.base.fill_color.as_ref(),
        param,
        |d| d.fill_color.as_ref(),
        &black,
    );
    let stroke_color = resolve_paint_color(
        p.base.stroke_color.as_ref(),
        param,
        |d| d.stroke_color.as_ref(),
        &black,
    );
    with_object_space(&p.base, params, builder, |b| {
        let fill = if fill_on {
            Some(faded(paint_of(fill_color, &path, colors, b)?, p.base.alpha))
        } else {
            None
        };
        let stroke = if stroke_on {
            Some(faded(
                paint_of(stroke_color, &path, colors, b)?,
                p.base.alpha,
            ))
        } else {
            None
        };
        b.push(DrawOp::Path {
            path,
            fill,
            stroke,
            stroke_style: style,
            rule: fill_rule(p.rule),
        });
        Ok(())
    })
}

fn flatten_text(
    t: &TextObject,
    params: &HashMap<String, DrawParam>,
    colors: &ColorTable,
    builder: &mut DisplayListBuilder,
) -> Result<()> {
    let param = resolve_param(&t.base, params)?;
    // Text is filled by default and stroked only when asked, which is the
    // opposite of a path's defaults.
    let (fill_on, stroke_on) = resolve_paint(&t.base, param, (true, false));
    let black = default_color();
    let fill = if fill_on {
        Some(faded(
            solid(
                resolve_paint_color(
                    t.base.fill_color.as_ref(),
                    param,
                    |d| d.fill_color.as_ref(),
                    &black,
                ),
                colors,
            )?,
            t.base.alpha,
        ))
    } else {
        None
    };
    let stroke = if stroke_on {
        Some(faded(
            solid(
                resolve_paint_color(
                    t.base.stroke_color.as_ref(),
                    param,
                    |d| d.stroke_color.as_ref(),
                    &black,
                ),
                colors,
            )?,
            t.base.alpha,
        ))
    } else {
        None
    };
    let stroke_style = stroke_style_of(&t.base, param);
    let size = t.size.value();
    // HScale widens or narrows the glyphs without changing their advance.
    let h_scale = t.h_scale.unwrap_or(1.0);
    let letter_spacing = t.char_space.unwrap_or(0.0);
    let font = FontId(t.font.parse::<u32>().unwrap_or(0));
    let read = t.read_direction.unwrap_or_else(|| read_of(&t.codes));
    let char_dir = t.char_direction.unwrap_or_default();
    let glyph_angle = glyph_turn(read, char_dir);
    with_object_space(&t.base, params, builder, |b| {
        for code in &t.codes {
            if code.text.is_empty() {
                continue;
            }
            // Every stretch knows where its first glyph sits within the code,
            // whatever cut it, so the code's origin is the only thing missing.
            let (code_x, code_y) = code.origin();
            for segment in segments_of(code, &t.cg_transforms, size) {
                // Table 47: the two directions are read in the line's own
                // frame, so the glyphs reach the page through one transform.
                let (dx, dy) = read.place(segment.offset, segment.across);
                let transform = run_placement(Point::new(code_x + dx, code_y + dy), read, h_scale);
                b.push(DrawOp::Text(GlyphRun {
                    font,
                    size: t.size,
                    glyphs: segment.glyphs,
                    advances: segment.advances,
                    text: Some(segment.text),
                    cluster_map: segment.cluster_map,
                    codes_are_glyphs: segment.glyph_index,
                    transform,
                    glyph_angle,
                    fill: fill.clone(),
                    stroke: stroke.clone(),
                    stroke_style: stroke_style.clone(),
                    letter_spacing,
                    word_spacing: 0.0,
                    // Text codes address the object space, so the box lives
                    // at its origin too.
                    bbox: Rect::new(0.0, 0.0, t.base.boundary.w, t.base.boundary.h),
                }));
            }
        }
        Ok(())
    })
}

/// The direction a run is read when the object does not say. Figure 63 sets a
/// downward line with DeltaY alone, so a code whose only offsets run down the
/// page is read that way; without them the default is rightwards.
fn read_of(codes: &[TextCode]) -> ReadDirection {
    let downwards = codes
        .iter()
        .any(|code| code.delta_x.is_empty() && !code.delta_y.is_empty());
    if downwards {
        ReadDirection(90)
    } else {
        ReadDirection::ZERO
    }
}

/// The transform a run of text is drawn through. Figure 62 puts the pen in the
/// line's frame, where `+x` is the read direction, so the read angle carries it
/// onto the page and the advances run along it.
fn run_placement(origin: Point, read: ReadDirection, h_scale: f64) -> Affine {
    let widened = Affine::new(h_scale, 0.0, 0.0, 1.0, 0.0, 0.0);
    Affine::rotate_deg(f64::from(read.degrees()))
        .then(widened)
        .then(Affine::translate(origin.x, origin.y))
}

/// How far a glyph turns about its own origin, in the line's frame. Table 47
/// measures `CharDirection` from the page and the glyph is drawn in a frame
/// already turned by the read angle, so the two are subtracted.
fn glyph_turn(read: ReadDirection, char_dir: CharDirection) -> f64 {
    let turn = i64::from(char_dir.degrees()) - i64::from(read.degrees());
    turn.rem_euclid(360) as f64
}

/// Where a `TextCode` puts each of its glyphs, from the offsets in table 46.
/// A list shorter than the steps between the glyphs is read as positions, and
/// `docs/Architecture.md` says why a producer needs both readings.
struct GlyphAt<'a> {
    code: &'a TextCode,
    /// List too short for the text, so the entries place glyphs.
    placed: bool,
    /// Offsets of each character, already read the way this list means them.
    advances: Vec<f64>,
    /// Where each character sits across the run, accumulated from the steps.
    cross: Vec<f64>,
    /// Text size, which is what an unlisted character advances by.
    size: f64,
}

impl<'a> GlyphAt<'a> {
    fn new(code: &'a TextCode, text: &str, size: f64) -> Self {
        let chars: Vec<char> = text.chars().collect();
        // One offset follows every character but the last, so a list that
        // stops one short of that is placing the characters it covers; a
        // shorter list has been trimmed, and what it says are the steps kept.
        let listed = reach(&code.delta_x, &code.delta_y);
        let steps = chars.len().saturating_sub(1);
        let placed = listed < steps;
        // Every offset is read once here, so a stretch only ever looks up the
        // character it starts from.
        let advances: Vec<f64> = (0..chars.len())
            .map(|i| advance_of(code, &chars, i, size, placed))
            .collect();
        // A line placed on both axes names one step per pair of characters, so
        // DeltaY only says how far the next row sits: a row returning on both
        // axes lands its first character back at the left edge, a row down.
        let cross = crossings(&code.delta_y, chars.len(), placed);
        debug_assert_eq!(cross.len(), chars.len());
        Self {
            code,
            placed,
            advances,
            cross,
            size,
        }
    }

    /// How far character `i` sits across the run, from DeltaY.
    fn across(&self, i: usize) -> f64 {
        let at = i.min(self.cross.len().saturating_sub(1));
        self.cross[at]
    }

    /// How far the line moves on from character `i`: where the next glyph
    /// sits, which is the offset the list gave for it, drawn or not.
    fn reach_at(&self, i: usize) -> f64 {
        match self.advances.get(i) {
            Some(at) => *at,
            // Past the offsets, a character moves on by its own size, the only
            // advance the code still states.
            None => self.size,
        }
    }

    /// Where glyph `i` sits within the code, in the x of the text code.
    fn offset(&self, i: usize) -> f64 {
        if self.placed {
            // A placing list names the spot of each character it reached, and
            // the characters past it follow the last one at their own size.
            let last = self.code.delta_x.len().saturating_sub(1);
            let held = self.code.delta_x.get(i.min(last)).copied().unwrap_or(0.0);
            return held + i.saturating_sub(last) as f64 * self.size;
        }
        self.advances[..i.min(self.advances.len())].iter().sum()
    }
}

/// How far the line moves on from character `i`, read the way `placed` says.
/// A stepping list gives the offset itself; a placing one gives a spot, so the
/// advance is the distance to the next and an unroomed glyph moves it by none.
fn advance_of(code: &TextCode, chars: &[char], i: usize, size: f64, placed: bool) -> f64 {
    if placed {
        return placed_at(code, chars, i, size);
    }
    // Figure 63: a downward line is set with DeltaY alone, so the step before
    // the second glyph is the one that moves it. A line that states a DeltaX
    // keeps it, since DeltaY then only places the next row.
    if code.delta_x.is_empty() && !code.delta_y.is_empty() {
        return code.delta_y.get(i).copied().unwrap_or(size);
    }
    code.delta_x.get(i).copied().unwrap_or(size)
}

/// Where each character sits across the line, from the steps that reach it. A
/// placing list already names the spot of each character it covers, so its
/// steps are read as the spots themselves.
fn crossings(delta_y: &[f64], len: usize, placed: bool) -> Vec<f64> {
    if placed {
        return (0..len)
            .map(|i| delta_y.get(i).copied().unwrap_or(0.0))
            .collect();
    }
    let mut running = 0.0;
    let mut out = Vec::with_capacity(len);
    for i in 0..len {
        out.push(running);
        running += delta_y.get(i).copied().unwrap_or(0.0);
    }
    out
}

/// Distance from where character `i` sits to where `i + 1` sits, when the list
/// gives a place for each. Past the last place it gives, and after the last
/// character, a size is the only advance the code still states.
fn placed_at(code: &TextCode, chars: &[char], i: usize, size: f64) -> f64 {
    match (code.delta_x.get(i), code.delta_x.get(i + 1)) {
        // Two places the list gave: the distance between them is the advance.
        (Some(here), Some(there)) if i + 1 < chars.len() => (there - here).max(0.0),
        _ => size,
    }
}

/// Offsets a list supplies before it runs out, counting both axes.
fn reach(delta_x: &[f64], delta_y: &[f64]) -> usize {
    delta_x.len().max(delta_y.len())
}

/// One stretch of a `TextCode` as it reaches the display list.
struct RunSegment {
    /// Codes to draw: glyph indices when `glyph_index` is set, Unicode code
    /// points otherwise.
    glyphs: Vec<u16>,
    /// Whether `glyphs` holds indices into the font's own glyph space.
    glyph_index: bool,
    /// Character index each glyph belongs to; absent when the two agree.
    cluster_map: Option<Vec<u32>>,
    /// Horizontal origin of this stretch within the text code.
    offset: f64,
    /// How far that origin sits across the line, from `DeltaY`.
    across: f64,
    /// Per-glyph advances, in the text code's own order.
    advances: Vec<f64>,
    /// The characters this stretch covers.
    text: String,
}

/// A stretch of a code before it knows where it starts: the glyphs it draws
/// and the characters they stand for.
struct RunShape {
    glyphs: Vec<u16>,
    glyph_index: bool,
    cluster_map: Option<Vec<u32>>,
    start: usize,
    end: usize,
}

/// Cut a code's characters into the stretches its transforms name, leaving
/// the characters no transform covers as stretches of their own. A step along
/// that walks the pen back starts a row, and a row is a stretch of its own.
fn run_shapes(code: &TextCode, chars: &[char], transforms: &[CGTransform]) -> Vec<RunShape> {
    let len = chars.len();
    let mut spans: Vec<(usize, usize, &CGTransform)> = Vec::new();
    for t in transforms {
        // A transform that covers nothing has nothing to replace. One that
        // covers characters but names no glyph still takes them out of the
        // text, so it keeps its place and draws nothing.
        let start = t.code_position.min(len);
        let end = (start + t.code_count.max(1)).min(len);
        if start < end {
            spans.push((start, end, t));
        }
    }
    spans.sort_by_key(|(start, _, _)| *start);

    // A stretch opens at a transform, at a row, or where the last one ended.
    let mut cuts: Vec<usize> = vec![0];
    for (start, _, _) in &spans {
        cuts.push(*start);
    }
    // A step that walks the pen back ends a row, so the next character opens
    // the one below. The last character has no step after it, and a code whose
    // offsets are places rather than steps says nothing about rows.
    for i in 1..len.saturating_sub(1) {
        if code.delta_x.get(i).is_some_and(|step| *step < 0.0) {
            cuts.push(i + 1);
        }
    }
    cuts.retain(|at| *at < len);
    cuts.sort_unstable();
    cuts.dedup();

    let mut out: Vec<RunShape> = Vec::new();
    let mut at = 0usize;
    for (start, end, t) in spans {
        // Overlapping transforms: the first to reach a character wins.
        if start < at {
            continue;
        }
        for row in rows_within(&cuts, at, start) {
            out.push(plain_shape(chars, at, row));
            at = row;
        }
        if start > at {
            out.push(plain_shape(chars, at, start));
        }
        // A glyph maps to the first character it replaces, so a one-to-many
        // transform keeps every glyph anchored to the same character.
        let cluster_map: Vec<u32> = (0..t.glyphs.len())
            .map(|i| (start + i.min(end - start - 1)) as u32)
            .collect();
        out.push(RunShape {
            glyphs: t.glyphs.clone(),
            glyph_index: true,
            cluster_map: Some(cluster_map),
            start,
            end,
        });
        at = end;
    }
    for row in rows_within(&cuts, at, len) {
        out.push(plain_shape(chars, at, row));
        at = row;
    }
    if at < len {
        out.push(plain_shape(chars, at, len));
    }
    out
}

/// The row breaks that fall strictly inside `from..to`, so a break on either
/// end of a stretch does not cut it twice.
fn rows_within(cuts: &[usize], from: usize, to: usize) -> Vec<usize> {
    cuts.iter()
        .copied()
        .filter(|cut| *cut > from && *cut < to)
        .collect()
}

/// Characters drawn as they are written, from `start` up to `end`.
fn plain_shape(chars: &[char], start: usize, end: usize) -> RunShape {
    RunShape {
        glyphs: chars[start..end]
            .iter()
            .map(|c| u32::from(*c) as u16)
            .collect(),
        glyph_index: false,
        cluster_map: None,
        start,
        end,
    }
}

/// Split a `TextCode` into the stretches a `CGTransform` carves out of it.
/// Untransformed characters keep their code points; a transformed stretch
/// carries glyph indices, so the two kinds never share a run.
fn segments_of(code: &TextCode, transforms: &[CGTransform], size: f64) -> Vec<RunSegment> {
    let chars: Vec<char> = code.text.chars().collect();
    let at = GlyphAt::new(code, &code.text, size);
    let shapes = run_shapes(code, &chars, transforms);
    let mut out: Vec<RunSegment> = Vec::with_capacity(shapes.len());
    for shape in shapes {
        // A stretch that names no glyph took its characters out of the text
        // without drawing anything, so it has no run.
        if shape.glyphs.is_empty() {
            continue;
        }
        let start = at.offset(shape.start);
        let across = at.across(shape.start);
        // A run draws the glyphs it lists, so its advances follow its glyphs:
        // the renderer reaches each glyph by the advance before it, which
        // leaves the first of a row to the offset the row already carries.
        let advances: Vec<f64> = (0..shape.glyphs.len())
            .map(|k| at.reach_at(shape.start + k))
            .collect();
        out.push(RunSegment {
            glyphs: shape.glyphs,
            glyph_index: shape.glyph_index,
            cluster_map: shape.cluster_map,
            offset: start,
            across,
            advances,
            text: chars[shape.start..shape.end].iter().collect(),
        });
    }
    out
}

fn flatten_image(i: &ImageObject, builder: &mut DisplayListBuilder) -> Result<()> {
    if i.resource_id.is_empty() {
        return Ok(());
    }
    // Bytes are attached later by the resource resolver; the display list
    // only records which resource each placement refers to.
    let image = ImageRef::new(
        i.resource_id.clone(),
        Arc::new(Vec::new()),
        (1, 1),
        ImageCodec::Png,
    );
    with_object_space(&i.base, &HashMap::new(), builder, |b| {
        // Image content fills the unit square of the object space; the
        // CTM inside the scope above scales it onto the boundary. Feeding
        // the boundary here as well would scale the placement twice.
        b.push(DrawOp::Image {
            image,
            dst: Rect::new(0.0, 0.0, 1.0, 1.0),
        });
        Ok(())
    })
}

/// GB/T 33190 clause 13: a composite object places a vector resource. The
/// graphic is drawn when the resource carries one; otherwise the substitution
/// image stands in for it, which is what the substitution exists for.
fn flatten_composite(
    c: &sofd_model::object::CompositeObject,
    params: &HashMap<String, DrawParam>,
    colors: &ColorTable,
    composites: &CompositeTable,
    builder: &mut DisplayListBuilder,
) -> Result<()> {
    if c.resource_id.is_empty() {
        return Ok(());
    }
    let unit = composites.get(c.resource_id.as_str());
    with_object_space(&c.base, params, builder, |b| {
        if let Some(block) = unit.and_then(|u| u.content.as_ref()) {
            // The graphic's own objects, drawn inside the placement box.
            flatten_block(block, params, colors, composites, b)
        } else if let Some(sub) = unit.and_then(|u| u.substitution.as_ref()) {
            // No vector content: paint the stand-in, whose bytes the image
            // resolver fills in later, exactly as an image object's are.
            b.push(DrawOp::Image {
                image: ImageRef::new(sub.clone(), Arc::new(Vec::new()), (1, 1), ImageCodec::Png),
                dst: Rect::new(0.0, 0.0, 1.0, 1.0),
            });
            Ok(())
        } else {
            // Nothing to draw and nothing to stand in: the placement is empty.
            Ok(())
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use sofd_common::Rect;
    use sofd_display::ops::Color;
    use sofd_model::document::{Layer, PageBlock};
    use sofd_model::object::{ImageObject, PathObject, TextCode, TextObject};

    fn builder() -> DisplayListBuilder {
        DisplayListBuilder::new(Rect::A4_PORTRAIT)
    }

    fn run(objects: Vec<Object>, params: &HashMap<String, DrawParam>) -> Result<Vec<DrawOp>> {
        run_in_layer(None, objects, params)
    }

    /// Flatten one layer whose own draw parameter the test chooses.
    fn run_in_layer(
        layer_param: Option<&str>,
        objects: Vec<Object>,
        params: &HashMap<String, DrawParam>,
    ) -> Result<Vec<DrawOp>> {
        let block = PageBlock {
            layers: vec![Layer {
                id: None,
                draw_param: layer_param.map(str::to_string),
                objects,
            }],
        };
        let mut b = builder();
        flatten_block(&block, params, &no_colors(), &no_composites(), &mut b)?;
        Ok(b.finish(0).ops)
    }

    fn path_object(data: &str) -> PathObject {
        PathObject {
            base: ObjectBase {
                id: Some("1".to_string()),
                boundary: Rect::new(0.0, 0.0, 10.0, 10.0),
                ..Default::default()
            },
            abbreviated_data: data.to_string(),
            ..Default::default()
        }
    }

    /// A path object that asks for a fill, so the colour defaults are visible
    /// without the object also asking for a stroke.
    fn filled_path_object(data: &str) -> PathObject {
        let p = path_object(data);
        PathObject {
            base: ObjectBase {
                fill: Some(true),
                ..p.base
            },
            ..p
        }
    }

    #[test]
    fn an_absent_fill_means_not_filled_and_an_absent_stroke_means_stroked() {
        let ops = run(
            vec![Object::PathObject(path_object("M 0 0 L 10 10"))],
            &HashMap::new(),
        )
        .expect("flatten");
        let DrawOp::Path { fill, stroke, .. } = &ops[0] else {
            panic!("expected a path op, got {ops:?}");
        };
        assert!(fill.is_none());
        assert!(stroke.is_some());
    }

    #[test]
    fn object_colours_default_to_black() {
        let ops = run(
            vec![Object::PathObject(filled_path_object("M 0 0 L 10 10"))],
            &HashMap::new(),
        )
        .expect("flatten");
        let DrawOp::Path {
            fill: Some(Paint::Solid(c)),
            ..
        } = &ops[0]
        else {
            panic!("expected a filled path, got {ops:?}");
        };
        assert_eq!(*c, Color::BLACK);
    }

    #[test]
    fn a_draw_param_supplies_colour_and_width() {
        let mut params = HashMap::new();
        params.insert(
            "7".to_string(),
            DrawParam {
                id: "7".to_string(),
                line_width: Some(0.5),
                fill_color: Some(ColorRef::from_hex("#FF0000")),
                ..Default::default()
            },
        );
        let mut p = filled_path_object("M 0 0 L 10 10");
        p.base.draw_param = Some("7".to_string());
        let ops = run(vec![Object::PathObject(p)], &params).expect("flatten");
        let DrawOp::Path {
            fill, stroke_style, ..
        } = &ops[0]
        else {
            panic!("expected a path op, got {ops:?}");
        };
        assert!(matches!(
            fill,
            Some(Paint::Solid(Color {
                r: 255,
                g: 0,
                b: 0,
                a: 255
            }))
        ));
        assert!((stroke_style.width - 0.5).abs() < 1e-9);
    }

    #[test]
    fn a_dangling_draw_param_reference_is_reported() {
        let mut p = path_object("M 0 0 L 10 10");
        p.base.draw_param = Some("missing".to_string());
        assert!(run(vec![Object::PathObject(p)], &HashMap::new()).is_err());
    }

    /// One red, half-a-millimetre parameter under the id the tests ask for.
    fn red_param(id: &str) -> HashMap<String, DrawParam> {
        let mut params = HashMap::new();
        params.insert(
            id.to_string(),
            DrawParam {
                id: id.to_string(),
                line_width: Some(0.5),
                fill_color: Some(ColorRef::from_hex("#FF0000")),
                ..Default::default()
            },
        );
        params
    }

    /// The paint and width a path op carries, which is where a parameter shows.
    fn paint_of(op: &DrawOp) -> (Option<Paint>, f64) {
        let DrawOp::Path {
            fill, stroke_style, ..
        } = op
        else {
            panic!("expected a path op, got {op:?}");
        };
        (fill.clone(), stroke_style.width)
    }

    #[test]
    fn gb_t_33190_8_5_a_layer_gives_its_objects_a_draw_parameter() {
        let ops = run_in_layer(
            Some("7"),
            vec![Object::PathObject(filled_path_object("M 0 0 L 10 10"))],
            &red_param("7"),
        )
        .expect("flatten");
        let (fill, width) = paint_of(&ops[0]);
        assert!(
            matches!(
                fill,
                Some(Paint::Solid(Color {
                    r: 255,
                    g: 0,
                    b: 0,
                    a: 255
                }))
            ),
            "the layer's colour reached the object: {fill:?}"
        );
        assert!((width - 0.5).abs() < 1e-9, "and so did its width: {width}");
    }

    #[test]
    fn an_object_that_states_its_own_draw_param_keeps_it() {
        // The layer says blue and wide, the object says red and thin: clause
        // 8.5 leaves the nearer statement in charge.
        let mut own = HashMap::new();
        own.insert(
            "9".to_string(),
            DrawParam {
                id: "9".to_string(),
                line_width: Some(0.25),
                fill_color: Some(ColorRef::from_hex("#0000FF")),
                ..Default::default()
            },
        );
        let mut layer = red_param("7");
        layer.extend(own);

        let mut p = filled_path_object("M 0 0 L 10 10");
        p.base.draw_param = Some("9".to_string());
        let ops = run_in_layer(Some("7"), vec![Object::PathObject(p)], &layer).expect("flatten");
        let (fill, width) = paint_of(&ops[0]);
        assert!(
            matches!(
                fill,
                Some(Paint::Solid(Color {
                    r: 0,
                    g: 0,
                    b: 255,
                    a: 255
                }))
            ),
            "the object's own colour wins over the layer's: {fill:?}"
        );
        assert!((width - 0.25).abs() < 1e-9, "and its own width: {width}");
    }

    #[test]
    fn a_layer_draw_param_that_names_nothing_leaves_the_object_as_it_stands() {
        // The sample this reader is measured against writes a layer style on
        // every template and ships no such parameter, so failing here would
        // refuse a document that draws perfectly well without it.
        let ops = run_in_layer(
            Some("missing"),
            vec![Object::PathObject(path_object("M 0 0 L 10 10"))],
            &HashMap::new(),
        )
        .expect("a layer style that names nothing is no reason to refuse a page");
        assert_eq!(ops.len(), 1, "the object is still drawn: {ops:?}");
        let (fill, _) = paint_of(&ops[0]);
        assert!(fill.is_none(), "and carries no fill it was never given");
    }

    #[test]
    fn an_object_draw_param_that_names_nothing_is_still_reported() {
        // An object saying which style it wants, when that style is absent, is
        // a statement about the object rather than a layer habit.
        let mut p = path_object("M 0 0 L 10 10");
        p.base.draw_param = Some("missing".to_string());
        assert!(run_in_layer(Some("7"), vec![Object::PathObject(p)], &red_param("7")).is_err());
    }

    #[test]
    fn a_ctm_wraps_the_object_in_save_transform_restore() {
        let mut p = path_object("M 0 0 L 10 10");
        p.base.boundary = Rect::new(20.0, 30.0, 10.0, 10.0);
        p.base.ctm = Some("2 0 0 2 5 5".to_string());
        let ops = run(vec![Object::PathObject(p)], &HashMap::new()).expect("flatten");
        // Origin and CTM fold into one matrix: the origin lands at the
        // boundary corner and the CTM applies inside it.
        assert!(matches!(ops[0], DrawOp::Save));
        assert!(matches!(ops[1], DrawOp::Transform(t) if t.apply(0.0, 0.0) == (25.0, 35.0)));
        assert!(matches!(ops[2], DrawOp::Path { .. }));
        assert!(matches!(ops[3], DrawOp::Restore));
    }

    #[test]
    fn path_data_is_required() {
        let p = path_object("   ");
        assert!(run(vec![Object::PathObject(p)], &HashMap::new()).is_err());
    }

    #[test]
    fn unknown_objects_are_skipped_silently() {
        let ops = run(vec![Object::unknown()], &HashMap::new()).expect("flatten");
        assert!(ops.is_empty());
    }

    #[test]
    fn a_short_list_places_its_glyphs_and_a_long_one_steps_them() {
        // "AB" has one step between its two characters, so two offsets are not
        // shorter than the steps: the list describes distance, not position.
        let stepping = only_offsets(&[4.0, 9.0], "AB");
        let steps = GlyphAt::new(&stepping, "AB", 3.0);
        assert!(!steps.placed);
        assert_eq!(steps.offset(1), 4.0, "the first entry, not the second");
        assert_eq!(steps.reach_at(1), 9.0, "the second entry is the one after");

        // "A B" has two steps and two offsets, which is not shorter: these are
        // distances, and a placing read would put the second glyph at 4 mm.
        let placing = only_offsets(&[4.0, 4.0], "A B");
        let spaced = GlyphAt::new(&placing, "A B", 3.0);
        assert!(!spaced.placed);
        assert_eq!(spaced.offset(2), 8.0, "the two entries summed");

        // One offset for two steps is shorter, so it is a spot, and the glyphs
        // it does not name walk on from it by their size.
        let placing = only_offsets(&[4.0], "A B");
        let placed = GlyphAt::new(&placing, "A B", 3.0);
        assert!(placed.placed);
        assert_eq!(placed.offset(1), 7.0, "one glyph on from the only spot");
        assert_eq!(placed.offset(2), 10.0, "and the next one size past it");
    }

    /// A text code whose horizontal offsets are the only thing the test states.
    fn only_offsets(delta_x: &[f64], text: &str) -> TextCode {
        let mut code = TextCode::default();
        code.delta_x = delta_x.to_vec();
        code.text = text.to_string();
        code
    }

    #[test]
    fn each_text_code_becomes_its_own_glyph_run() {
        let t = TextObject {
            base: ObjectBase {
                boundary: Rect::new(1.0, 2.0, 30.0, 4.0),
                ..Default::default()
            },
            size: sofd_common::Millimeter::new(3.0),
            codes: vec![
                {
                    let mut code = TextCode::default();
                    code.text = "AB".to_string();
                    code
                },
                {
                    let mut code = TextCode::default();
                    code.x = 10.0;
                    code
                },
            ],
            ..Default::default()
        };
        let ops = run(vec![Object::TextObject(t)], &HashMap::new()).expect("flatten");
        let drawn: Vec<_> = ops
            .iter()
            .filter(|op| !matches!(op, DrawOp::Save | DrawOp::Restore | DrawOp::Transform(_)))
            .collect();
        assert_eq!(drawn.len(), 1, "the empty TextCode produces no run");
        let DrawOp::Text(run_out) = drawn[0] else {
            panic!("expected a text op, got {drawn:?}");
        };
        assert_eq!(run_out.glyphs, vec![65u16, 66u16]);
        assert_eq!(run_out.advances, vec![3.0, 3.0]);
        assert_eq!(run_out.plain_text(), "AB");
    }

    #[test]
    fn an_image_object_records_its_resource() {
        let img = ImageObject {
            base: ObjectBase {
                boundary: Rect::new(0.0, 0.0, 40.0, 30.0),
                ..Default::default()
            },
            resource_id: "img_9".to_string(),
            ..Default::default()
        };
        let ops = run(vec![Object::ImageObject(img)], &HashMap::new()).expect("flatten");
        let DrawOp::Image { image, dst, .. } = &ops[0] else {
            panic!("expected an image op, got {ops:?}");
        };
        assert_eq!(image.id, "img_9");
        // Images fill the unit square of the object space; the transform op
        // above carries them onto the boundary.
        assert_eq!(*dst, Rect::new(0.0, 0.0, 1.0, 1.0));
    }

    #[test]
    fn an_image_without_a_resource_is_dropped() {
        let img = ImageObject::default();
        let ops = run(vec![Object::ImageObject(img)], &HashMap::new()).expect("flatten");
        assert!(ops.is_empty());
    }

    #[test]
    fn layers_draw_bottom_first_in_order() {
        let block = PageBlock {
            layers: vec![
                Layer {
                    id: None,
                    draw_param: None,
                    objects: vec![Object::PathObject(path_object("M 0 0 L 1 1"))],
                },
                Layer {
                    id: None,
                    draw_param: None,
                    objects: vec![Object::PathObject(path_object("M 2 2 L 3 3"))],
                },
            ],
        };
        let mut b = builder();
        flatten_block(
            &block,
            &HashMap::new(),
            &no_colors(),
            &no_composites(),
            &mut b,
        )
        .expect("flatten");
        assert_eq!(b.finish(0).ops.len(), 2);
    }
}
