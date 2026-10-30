//! Raster rendering backend.
//! Consumes a [`PageDisplay`] and rasterizes its op stream with tiny-skia;
//! coordinates arrive in millimetres and are scaled by the target DPI.

use std::sync::Arc;

use sofd_common::Rect;
use sofd_display::ops::{Color, GradientStop, LineCap, LineJoin};
use sofd_display::{DrawOp, GlyphRun, ImageRef, PageDisplay, Paint, PaintRule, StrokeStyle};
use sofd_error::{OfdError, RenderError, Result};
use sofd_font::{FontManager, Unresolved};
use sofd_geometry::{Affine, Path, PathSegment};
use tiny_skia::{
    FillRule, IntSize, LineCap as TsLineCap, LineJoin as TsLineJoin, Mask, Paint as TsPaint,
    Path as TsPath, PathBuilder as TsBuilder, Pixmap, PixmapPaint, Point, PremultipliedColorU8,
    Shader, SpreadMode, Stroke, StrokeDash, Transform,
};

/// Rasterization parameters.
#[derive(Debug, Clone)]
pub struct RenderOptions {
    /// Output resolution; 96 DPI matches typical screen previews.
    pub dpi: f64,
    /// Colour painted before the page content; `None` keeps transparency.
    pub background: Option<Color>,
    /// Hard ceiling on total pixels, guarding against hostile page boxes.
    pub max_pixels: usize,
    /// The part of the page to rasterize, in millimetres. A zoomed-in view
    /// only needs the part on screen, which keeps a large page inside the
    /// texture limit instead of capping the zoom.
    pub window: Option<Rect>,
}

impl Default for RenderOptions {
    fn default() -> Self {
        Self {
            dpi: 96.0,
            background: Some(Color::WHITE),
            max_pixels: 64 * 1024 * 1024,
            window: None,
        }
    }
}

/// A finished rasterization of one page.
#[derive(Debug)]
pub struct RasterOutput {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Runs that drew with at least one glyph missing from their face, so the
    /// page shows fewer marks than it should.
    pub missing_glyph_runs: usize,
    /// Runs skipped because their face could not be loaded at all. The page
    /// still renders; the marks those runs would have made are absent.
    pub skipped_font_runs: usize,
    /// Images skipped because their bytes would not decode or their size was
    /// rejected. Like a missing font, one broken resource costs only its own
    /// placement, not the page around it.
    pub skipped_image_ops: usize,
    /// Glyph positions that resolved to no outline at all: the face was found
    /// but does not carry the character, and no substitute did either. These
    /// are the marks the page is silently short of.
    pub missing_glyphs: usize,
    /// Runs whose only paint was a stroke. Filling needs a fill and stroking
    /// needs an outline to stroke along, so such a run draws nothing here and
    /// is counted instead of vanishing quietly.
    pub stroke_only_runs: usize,
    /// Faces the document asked for and this build could not supply, one entry
    /// per face rather than per run. Empty is the only value that means the
    /// page is complete.
    pub missing_fonts: Vec<Unresolved>,
    pixmap: Pixmap,
}

impl RasterOutput {
    /// Whether the page is missing anything it was supposed to draw.
    pub fn is_complete(&self) -> bool {
        self.missing_glyph_runs == 0
            && self.skipped_font_runs == 0
            && self.skipped_image_ops == 0
            && self.missing_glyphs == 0
            && self.stroke_only_runs == 0
            && self.missing_fonts.is_empty()
    }
}

impl RasterOutput {
    /// Access the underlying pixmap for encoding or inspection.
    pub fn pixmap(&self) -> &Pixmap {
        &self.pixmap
    }

    /// Take the underlying pixmap.
    pub fn into_pixmap(self) -> Pixmap {
        self.pixmap
    }

    /// Straight (non-premultiplied) RGBA8 bytes, the exchange format most
    /// encoders and UI toolkits expect.
    pub fn rgba_bytes(&self) -> Vec<u8> {
        let (pixels, _) = self.pixmap.data().as_chunks::<4>();
        pixels.iter().flat_map(unpremultiply).collect()
    }
}

fn unpremultiply(px: &[u8; 4]) -> [u8; 4] {
    let a = px[3] as u32;
    if a == 0 || a == 255 {
        return [px[0], px[1], px[2], px[3]];
    }
    let un = |c: u8| ((c as u32 * 255 + a / 2) / a).min(255) as u8;
    [un(px[0]), un(px[1]), un(px[2]), px[3]]
}

/// Rasterize one page of a display list.
pub fn render_page(
    page: &PageDisplay,
    fonts: &FontManager,
    options: &RenderOptions,
) -> Result<RasterOutput> {
    // A window is clipped to the page, so a view past the edge rasterizes to
    // nothing rather than to a negative size.
    let view = match options.window {
        Some(w) => {
            let x1 =
                w.x.clamp(page.page_box.x, page.page_box.x + page.page_box.w);
            let y1 =
                w.y.clamp(page.page_box.y, page.page_box.y + page.page_box.h);
            let x2 = (w.x + w.w).clamp(x1, page.page_box.x + page.page_box.w);
            let y2 = (w.y + w.h).clamp(y1, page.page_box.y + page.page_box.h);
            Rect::new(x1, y1, x2 - x1, y2 - y1)
        }
        None => page.page_box,
    };
    let (width, height) = view.size_px(options.dpi);
    if (width as usize).saturating_mul(height as usize) > options.max_pixels {
        return Err(OfdError::Render(RenderError::BudgetExceeded(format!(
            "page raster {width}x{height} exceeds {} pixels",
            options.max_pixels
        ))));
    }
    let mut pixmap =
        Pixmap::new(width, height).ok_or(OfdError::Render(RenderError::InvalidSize {
            w: width,
            h: height,
        }))?;
    if let Some(bg) = options.background {
        pixmap.fill(to_color(&bg));
    }

    let mut state = State {
        ctm: Affine::IDENTITY,
        clip: None,
    };
    let mut stack: Vec<State> = Vec::new();
    let mut missing_glyph_runs = 0usize;
    let mut skipped_font_runs = 0usize;
    let mut skipped_image_ops = 0usize;
    let mut missing_glyphs = 0usize;
    let mut stroke_only_runs = 0usize;

    for op in &page.ops {
        match op {
            DrawOp::Save => stack.push(state.clone()),
            DrawOp::Restore => {
                state = stack.pop().ok_or_else(|| {
                    OfdError::Render(RenderError::BudgetExceeded(
                        "restore without a matching save".to_string(),
                    ))
                })?;
            }
            DrawOp::Transform(t) => state.ctm = state.ctm.then(*t),
            DrawOp::ClipPath { path, rule } => {
                let total = total_transform(&state, &view, options.dpi);
                let ts_path = to_ts_path(path, total)?;
                let mut mask = clip_mask(&state, width, height)?;
                mask.intersect_path(&ts_path, fill_rule(*rule), true, Transform::identity());
                state.clip = Some(Arc::new(mask));
            }
            DrawOp::ClipText(run) => {
                // A text clip is the glyph outlines themselves, resolved the
                // way drawn text is. Without its face there is nothing to
                // intersect, so the clip is left as it was.
                if let Some(face) = sofd_display::run_face(fonts, run) {
                    let total = total_transform(&state, &view, options.dpi);
                    let mut mask = clip_mask(&state, width, height)?;
                    for ts_path in glyph_outlines(fonts, run, face, total)? {
                        mask.intersect_path(
                            &ts_path,
                            FillRule::Winding,
                            true,
                            Transform::identity(),
                        );
                    }
                    state.clip = Some(Arc::new(mask));
                }
            }
            DrawOp::Path {
                path,
                fill,
                stroke,
                stroke_style,
                rule,
            } => {
                let total = total_transform(&state, &view, options.dpi);
                let ts_path = to_ts_path(path, total)?;
                if let Some(paint) = fill {
                    match paint {
                        // A mesh cannot be a tiny-skia shader, so it is painted
                        // per pixel and masked by the outline being filled.
                        Paint::Mesh(spec) => {
                            mesh::paint_mesh(&mut pixmap, spec, &ts_path, state.clip_ref(), total)
                        }
                        // A pattern is vector content, so it is rasterized once
                        // and tiled; the same mask confines the repeat.
                        Paint::Pattern(spec) => pattern::paint_pattern(
                            &mut pixmap,
                            spec,
                            &ts_path,
                            state.clip_ref(),
                            fonts,
                            total,
                        ),
                        _ => {
                            let p = paint_for(paint, total).unwrap_or_else(|| solid_paint(paint));
                            pixmap.fill_path(
                                &ts_path,
                                &p,
                                fill_rule(*rule),
                                Transform::identity(),
                                state.clip_ref(),
                            );
                        }
                    }
                }
                if let Some(paint) = stroke {
                    let p = paint_for(paint, total).unwrap_or_else(|| solid_paint(paint));
                    let mut ts_stroke = to_ts_stroke(stroke_style, &state, options.dpi);
                    if is_hairline(&p, &ts_stroke) {
                        ts_stroke.width = hairline_width(&ts_stroke);
                    }
                    pixmap.stroke_path(
                        &ts_path,
                        &p,
                        &ts_stroke,
                        Transform::identity(),
                        state.clip_ref(),
                    );
                }
            }
            DrawOp::Text(run) => {
                match draw_text(&mut pixmap, run, &state, view, fonts, options)? {
                    TextOutcome::Drawn => {}
                    TextOutcome::ShortGlyphs { glyphs } => {
                        missing_glyph_runs += 1;
                        missing_glyphs += glyphs;
                    }
                    TextOutcome::NoFace => {
                        // No face at all, so every mark is absent; counted as
                        // glyphs too, so one counter answers what is missing.
                        skipped_font_runs += 1;
                        missing_glyphs += run.len();
                    }
                    TextOutcome::StrokeOnly => stroke_only_runs += 1,
                }
            }
            DrawOp::Image { image, dst } => {
                match draw_image(&mut pixmap, image, *dst, &state, view, options)? {
                    ImageOutcome::Drawn => {}
                    ImageOutcome::Failed => skipped_image_ops += 1,
                }
            }
        }
    }

    Ok(RasterOutput {
        width,
        height,
        missing_glyph_runs,
        skipped_font_runs,
        skipped_image_ops,
        missing_glyphs,
        stroke_only_runs,
        missing_fonts: fonts.unresolved(),
        pixmap,
    })
}

mod gradient;
mod mesh;
mod pattern;
mod shape;
mod text;

use gradient::{focal_shader, linear_shader};
use shape::{
    State, clip_mask, fill_rule, glyph_outlines, hairline_width, is_hairline, paint_for,
    solid_paint, to_color, to_ts, to_ts_path, to_ts_stroke, total_transform,
};
use text::{ImageOutcome, TextOutcome, draw_image, draw_text};

/// Rasterize a pattern cell at `scale` pixels per millimetre, so a tile and the
/// page it repeats on agree. The cell carries its own bounds, which become the
/// page box of the little display list it is rendered as.
pub(crate) fn render_cell(
    spec: &sofd_display::PatternSpec,
    scale: f64,
    fonts: &FontManager,
) -> Option<Pixmap> {
    let mut cell = spec.cell.as_ref().clone();
    cell.page_box = Rect::new(0.0, 0.0, spec.width.max(1e-6), spec.height.max(1e-6));
    let options = RenderOptions {
        dpi: scale * 25.4,
        // A tile is transparent where its cell draws nothing, or the repeats
        // would paint over each other with an opaque background.
        background: None,
        ..RenderOptions::default()
    };
    render_page(&cell, fonts, &options)
        .ok()
        .map(|out| out.pixmap)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sofd_display::ops::GradientStop as DisplayStop;
    use sofd_display::{LinearGradientSpec, RadialGradientSpec};

    use sofd_display::{
        DisplayListBuilder, DrawOp, ImageCodec, ImageRef, Paint, PaintRule, StrokeStyle,
    };
    use sofd_font::{FontId, FontManager};
    use sofd_geometry::Path;

    const FONT_BYTES: &[u8] = include_bytes!("../../tests/data/Orbitron-Bold.ttf");

    fn manager_with_font() -> FontManager {
        let mut manager = FontManager::new();
        manager.registry_mut().register_embedded(
            "tests/data/Orbitron-Bold.ttf",
            "Orbitron",
            Arc::new(FONT_BYTES.to_vec()),
        );
        manager
    }

    fn small_box() -> Rect {
        Rect::new(0.0, 0.0, 20.0, 20.0)
    }

    fn rect_path(x: f64, y: f64, w: f64, h: f64) -> Path {
        Path::build(|b| {
            b.move_to(x, y)
                .line_to(x + w, y)
                .line_to(x + w, y + h)
                .line_to(x, y + h)
                .close();
        })
    }

    fn fill_rect_op(x: f64, y: f64, w: f64, h: f64, color: Color) -> DrawOp {
        DrawOp::Path {
            path: rect_path(x, y, w, h),
            fill: Some(Paint::Solid(color)),
            stroke: None,
            stroke_style: StrokeStyle::default(),
            rule: PaintRule::NonZero,
        }
    }

    fn page_with(ops: Vec<DrawOp>) -> PageDisplay {
        let mut b = DisplayListBuilder::new(small_box());
        for op in ops {
            b.push(op);
        }
        b.finish(0)
    }

    fn render(page: &PageDisplay, fonts: &FontManager) -> RasterOutput {
        render_page(page, fonts, &RenderOptions::default()).expect("render")
    }

    fn pixel_at(out: &RasterOutput, x: u32, y: u32) -> [u8; 4] {
        let px = out.pixmap().pixel(x, y).expect("in-bounds pixel");
        [px.red(), px.green(), px.blue(), px.alpha()]
    }

    #[test]
    fn a_blank_page_comes_out_white() {
        let manager = FontManager::new();
        let page = page_with(Vec::new());
        let out = render(&page, &manager);
        assert_eq!(out.width, 76);
        assert_eq!(out.height, 76);
        assert_eq!(pixel_at(&out, 5, 5), [255, 255, 255, 255]);
    }

    #[test]
    fn a_filled_rectangle_lands_where_asked() {
        let manager = FontManager::new();
        // 20 mm box at 96 DPI is about 76 px, so 10 mm is roughly centred.
        let page = page_with(vec![fill_rect_op(
            10.0,
            10.0,
            5.0,
            5.0,
            Color::rgba(255, 0, 0, 255),
        )]);
        let out = render(&page, &manager);
        assert_eq!(pixel_at(&out, 50, 50)[0], 255, "inside the rect is red");
        assert_eq!(
            pixel_at(&out, 5, 5),
            [255, 255, 255, 255],
            "outside stays white"
        );
    }

    #[test]
    fn a_transform_moves_geometry() {
        let manager = FontManager::new();
        let mut b = DisplayListBuilder::new(small_box());
        b.save().expect("save");
        b.transform(Affine::translate(10.0, 10.0));
        b.push(fill_rect_op(
            0.0,
            0.0,
            5.0,
            5.0,
            Color::rgba(0, 128, 0, 255),
        ));
        b.restore().expect("restore");
        let out = render(&b.finish(0), &manager);
        assert_eq!(
            pixel_at(&out, 50, 50)[1],
            128,
            "the rect moved with its CTM"
        );
        assert_eq!(
            pixel_at(&out, 10, 10),
            [255, 255, 255, 255],
            "origin stays untouched"
        );
    }

    #[test]
    fn save_and_restore_isolate_the_clip() {
        let manager = FontManager::new();
        let mut b = DisplayListBuilder::new(small_box());
        b.save().expect("save");
        b.push(DrawOp::ClipPath {
            path: rect_path(0.0, 0.0, 5.0, 5.0),
            rule: PaintRule::NonZero,
        });
        b.push(fill_rect_op(
            0.0,
            0.0,
            20.0,
            20.0,
            Color::rgba(0, 0, 255, 255),
        ));
        b.restore().expect("restore");
        b.push(fill_rect_op(
            10.0,
            10.0,
            5.0,
            5.0,
            Color::rgba(255, 0, 255, 255),
        ));
        // 5 mm and 10 mm on a 96 DPI page land near 19 px and 38 px.
        let out = render(&b.finish(0), &manager);
        assert_eq!(
            pixel_at(&out, 40, 40),
            [255, 0, 255, 255],
            "the restored state has no clip, so the fill lands"
        );
        assert_eq!(
            pixel_at(&out, 2, 2),
            [0, 0, 255, 255],
            "the clipped fill stayed inside its region"
        );
        assert_eq!(
            pixel_at(&out, 25, 25),
            [255, 255, 255, 255],
            "outside the clip region nothing was painted"
        );
        assert_eq!(
            pixel_at(&out, 50, 50)[0],
            255,
            "restored state allows new drawing"
        );
    }

    #[test]
    fn a_stroke_paints_the_border_not_the_middle() {
        let manager = FontManager::new();
        let page = page_with(vec![DrawOp::Path {
            path: rect_path(5.0, 5.0, 10.0, 10.0),
            fill: None,
            stroke: Some(Paint::Solid(Color::rgba(0, 0, 0, 255))),
            stroke_style: StrokeStyle::default(),
            rule: PaintRule::NonZero,
        }]);
        let out = render(&page, &manager);
        let dark_in_row = (0..out.width)
            .filter(|x| pixel_at(&out, *x, 38)[0] < 128)
            .count();
        assert!(
            dark_in_row >= 2,
            "border pixels are inked, found {dark_in_row}"
        );
        assert_eq!(
            pixel_at(&out, 40, 40),
            [255, 255, 255, 255],
            "middle is untouched"
        );
    }

    #[test]
    fn a_linear_gradient_runs_light_to_dark() {
        let manager = FontManager::new();
        let page = page_with(vec![DrawOp::Path {
            path: rect_path(0.0, 0.0, 20.0, 20.0),
            fill: Some(Paint::LinearGradient(Box::new(LinearGradientSpec {
                start: sofd_common::Point::new(0.0, 10.0),
                end: sofd_common::Point::new(20.0, 10.0),
                stops: vec![
                    DisplayStop {
                        offset: 0.0,
                        color: Color::rgba(0, 0, 0, 255),
                    },
                    DisplayStop {
                        offset: 1.0,
                        color: Color::rgba(255, 255, 255, 255),
                    },
                ],
            }))),
            stroke: None,
            stroke_style: Default::default(),
            rule: PaintRule::NonZero,
        }]);
        let out = render(&page, &manager);
        let left = pixel_at(&out, 5, 38)[0] as i32;
        let right = pixel_at(&out, 70, 38)[0] as i32;
        assert!(left < 80, "left edge is dark, got {left}");
        assert!(right > 175, "right edge is light, got {right}");
    }

    #[test]
    fn a_radial_gradient_runs_from_its_centre_out_to_its_circle() {
        // The shape our fixtures write: no StartRadius, so the first stop sits
        // at the centre and the last on the circle the clause names.
        let manager = FontManager::new();
        let page = page_with(vec![DrawOp::Path {
            path: rect_path(0.0, 0.0, 20.0, 20.0),
            fill: Some(Paint::RadialGradient(Box::new(RadialGradientSpec {
                center: sofd_common::Point::new(10.0, 10.0),
                radius: 0.0,
                end: sofd_common::Point::new(10.0, 10.0),
                end_radius: 8.0,
                stops: vec![
                    DisplayStop {
                        offset: 0.0,
                        color: Color::rgba(0, 0, 0, 255),
                    },
                    DisplayStop {
                        offset: 1.0,
                        color: Color::rgba(255, 255, 255, 255),
                    },
                ],
            }))),
            stroke: None,
            stroke_style: Default::default(),
            rule: PaintRule::NonZero,
        }]);
        let out = render(&page, &manager);
        let centre = pixel_at(&out, 38, 38);
        assert!(
            centre[0] < 40 && centre[1] < 40 && centre[2] < 40,
            "the centre holds the first stop, got {centre:?}"
        );
        // 8 mm out at 96 dpi is about 30 px, near the far circle.
        let near_edge = pixel_at(&out, 38, 8)[0];
        assert!(
            near_edge > 200,
            "the rim holds the last stop, got {near_edge}"
        );
        let middle = pixel_at(&out, 38, 23)[0] as i32;
        assert!(
            (30..225).contains(&middle),
            "halfway out is halfway between, got {middle}"
        );
    }

    #[test]
    fn a_shading_with_no_extent_paints_one_colour_and_keeps_the_page() {
        // A document whose radial shading names two points and no radius at
        // all once aborted the whole page here; the fill degrades to the
        // first stop instead and the rest of the drawing survives.
        let manager = FontManager::new();
        let page = page_with(vec![
            DrawOp::Path {
                path: rect_path(0.0, 0.0, 8.0, 8.0),
                fill: Some(Paint::RadialGradient(Box::new(RadialGradientSpec {
                    center: sofd_common::Point::new(5.0, 5.0),
                    radius: 0.0,
                    end: sofd_common::Point::new(6.0, 5.0),
                    end_radius: 0.0,
                    stops: vec![DisplayStop {
                        offset: 0.0,
                        color: Color::rgba(255, 0, 0, 255),
                    }],
                }))),
                stroke: None,
                stroke_style: Default::default(),
                rule: PaintRule::NonZero,
            },
            fill_rect_op(12.0, 12.0, 4.0, 4.0, Color::rgba(0, 0, 255, 255)),
        ]);
        let out = render(&page, &manager);
        assert_eq!(
            pixel_at(&out, 15, 15),
            [255, 0, 0, 255],
            "the extentless shading paints its first stop"
        );
        assert_eq!(
            pixel_at(&out, 50, 50),
            [0, 0, 255, 255],
            "the rect after it still paints"
        );
    }

    #[test]
    fn an_image_is_placed_on_the_page() {
        let manager = FontManager::new();
        // Encode a 4x4 solid blue PNG through the image crate itself.
        let mut img = image::RgbaImage::from_pixel(4, 4, image::Rgba([0, 0, 255, 255]));
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(img.clone())
            .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .expect("encode");
        let _ = &mut img;
        let page = page_with(vec![DrawOp::Image {
            image: ImageRef::new("img1", Arc::new(png), (4, 4), ImageCodec::Png),
            dst: Rect::new(10.0, 10.0, 8.0, 8.0),
        }]);
        let out = render(&page, &manager);
        assert_eq!(pixel_at(&out, 50, 50)[2], 255, "image area is blue");
        assert_eq!(
            pixel_at(&out, 5, 5),
            [255, 255, 255, 255],
            "outside stays white"
        );
    }

    #[test]
    fn an_image_fills_the_rect_its_placement_scope_sets() {
        // An image is flattened in object space, so the state already carries
        // the scale onto the page while the rect's origin is still millimetres.
        // Composing the page transform on the wrong side skewed that origin.
        let manager = FontManager::new();
        let mut png = Vec::new();
        image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(
            4,
            4,
            image::Rgba([0, 0, 255, 255]),
        ))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .expect("encode");
        let page = page_with(vec![
            DrawOp::Save,
            DrawOp::Transform(Affine::new(8.0, 0.0, 0.0, 8.0, 0.0, 0.0)),
            DrawOp::Image {
                image: ImageRef::new("img1", Arc::new(png), (4, 4), ImageCodec::Png),
                dst: Rect::new(1.0, 1.0, 1.0, 1.0),
            },
            DrawOp::Restore,
        ]);
        let out = render(&page, &manager);
        // The rect spans 1 to 2 mm, so (5, 5) at 96 dpi is inside it and
        // (2, 2) is the millimetre before it.
        assert_eq!(pixel_at(&out, 5, 5)[2], 255, "inside the placement is blue");
        assert_eq!(
            pixel_at(&out, 2, 2),
            [255, 255, 255, 255],
            "outside the placement stays white"
        );
    }

    #[test]
    fn text_renders_real_glyph_outlines() {
        let manager = manager_with_font();
        let run = sofd_display::GlyphRun {
            font: FontId(0),
            size: sofd_common::Millimeter::new(6.0),
            glyphs: vec![65u16], // Orbitron covers basic Latin
            advances: vec![5.0],
            text: Some("A".to_string()),
            cluster_map: None,
            codes_are_glyphs: false,
            transform: Affine::translate(2.0, 12.0),
            glyph_angle: 0.0,
            fill: Some(Paint::Solid(Color::rgba(0, 0, 0, 255))),
            stroke: None,
            stroke_style: Default::default(),
            letter_spacing: 0.0,
            word_spacing: 0.0,
            bbox: Rect::new(2.0, 6.0, 5.0, 6.0),
        };
        let page = page_with(vec![DrawOp::Text(run)]);
        let out = render(&page, &manager);
        // Ink must sit where the run was placed, at glyph height: a squashed
        // or misplaced outline fails the extent check even if it inks pixels.
        let mut min_y = u32::MAX;
        let mut max_y = 0u32;
        let mut dark = 0usize;
        for y in 0..out.height {
            for x in 0..out.width {
                if pixel_at(&out, x, y)[0] < 128 {
                    dark += 1;
                    min_y = min_y.min(y);
                    max_y = max_y.max(y);
                }
            }
        }
        assert!(dark > 5, "expected inked glyph pixels, found {dark}");
        let ink_height = max_y - min_y;
        // 6mm at 96dpi is a 22px em; 'A' reaches most of it.
        assert!(
            ink_height >= 12,
            "glyph ink spans {min_y}..{max_y}, expected a full-height glyph"
        );
    }

    #[test]
    fn a_turned_glyph_is_drawn_turned() {
        let manager = manager_with_font();
        let run = sofd_display::GlyphRun {
            font: FontId(0),
            size: sofd_common::Millimeter::new(6.0),
            glyphs: vec![65u16],
            advances: vec![5.0],
            text: Some("A".to_string()),
            cluster_map: None,
            codes_are_glyphs: false,
            transform: Affine::translate(2.0, 12.0),
            glyph_angle: 90.0,
            fill: Some(Paint::Solid(Color::rgba(0, 0, 0, 255))),
            stroke: None,
            stroke_style: Default::default(),
            letter_spacing: 0.0,
            word_spacing: 0.0,
            bbox: Rect::new(2.0, 6.0, 5.0, 6.0),
        };
        let page = page_with(vec![DrawOp::Text(run)]);
        let out = render(&page, &manager);
        // A quarter turn lays the glyph's height along x, so the ink becomes
        // wider than it is tall where an upright one is the other way round.
        let (mut min_x, mut max_x, mut min_y, mut max_y) = (u32::MAX, 0u32, u32::MAX, 0u32);
        for y in 0..out.height {
            for x in 0..out.width {
                if pixel_at(&out, x, y)[0] < 128 {
                    min_x = min_x.min(x);
                    max_x = max_x.max(x);
                    min_y = min_y.min(y);
                    max_y = max_y.max(y);
                }
            }
        }
        let (wide, tall) = (max_x - min_x, max_y - min_y);
        assert!(
            wide > tall,
            "turned glyph spans {wide}x{tall}, expected it wider than tall"
        );
    }

    #[test]
    fn a_run_missing_a_glyph_is_counted_without_failing_the_page() {
        let manager = manager_with_font();
        // 65 is 'A', which Orbitron draws; 60000 is far past any glyph the
        // face holds, so that code point resolves to no outline at all.
        let run = sofd_display::GlyphRun {
            font: FontId(0),
            size: sofd_common::Millimeter::new(6.0),
            glyphs: vec![65u16, 60000u16],
            advances: vec![5.0, 5.0],
            text: Some("Ax".to_string()),
            cluster_map: None,
            codes_are_glyphs: false,
            transform: Affine::translate(2.0, 12.0),
            glyph_angle: 0.0,
            fill: Some(Paint::Solid(Color::rgba(0, 0, 0, 255))),
            stroke: None,
            stroke_style: Default::default(),
            letter_spacing: 0.0,
            word_spacing: 0.0,
            bbox: Rect::new(2.0, 6.0, 10.0, 6.0),
        };
        let page = page_with(vec![DrawOp::Text(run)]);
        let out = render(&page, &manager);
        assert_eq!(
            out.missing_glyph_runs, 1,
            "the run drew, but one of its glyphs had no outline"
        );
    }

    #[test]
    fn a_run_whose_glyphs_all_resolve_reports_nothing_missing() {
        let manager = manager_with_font();
        let run = sofd_display::GlyphRun {
            font: FontId(0),
            size: sofd_common::Millimeter::new(6.0),
            glyphs: vec![65u16, 66u16],
            advances: vec![5.0, 5.0],
            text: Some("AB".to_string()),
            cluster_map: None,
            codes_are_glyphs: false,
            transform: Affine::translate(2.0, 12.0),
            glyph_angle: 0.0,
            fill: Some(Paint::Solid(Color::rgba(0, 0, 0, 255))),
            stroke: None,
            stroke_style: Default::default(),
            letter_spacing: 0.0,
            word_spacing: 0.0,
            bbox: Rect::new(2.0, 6.0, 10.0, 6.0),
        };
        let page = page_with(vec![DrawOp::Text(run)]);
        let out = render(&page, &manager);
        assert_eq!(out.missing_glyph_runs, 0);
    }

    #[test]
    fn a_run_with_an_unloadable_face_is_skipped_and_counted() {
        // An unknown font id means the face cannot be loaded. That costs the
        // run, not the page: rendering succeeds and the count records the loss.
        let manager = FontManager::new();
        let run = sofd_display::GlyphRun {
            font: FontId(7),
            size: sofd_common::Millimeter::new(3.0),
            glyphs: vec![65u16],
            advances: vec![3.0],
            text: Some("A".to_string()),
            cluster_map: None,
            codes_are_glyphs: false,
            transform: Affine::IDENTITY,
            glyph_angle: 0.0,
            fill: Some(Paint::Solid(Color::BLACK)),
            stroke: None,
            stroke_style: Default::default(),
            letter_spacing: 0.0,
            word_spacing: 0.0,
            bbox: Rect::new(0.0, 0.0, 3.0, 3.0),
        };
        let page = page_with(vec![DrawOp::Text(run)]);
        let out = render_page(&page, &manager, &RenderOptions::default()).expect("render");
        assert_eq!(out.skipped_font_runs, 1);
        assert_eq!(out.missing_glyph_runs, 0);
    }

    /// A run that is filled and stroked keeps both paints: the stroke is
    /// painted over the fill, so the outline widens the mark rather than
    /// replacing it.
    #[test]
    fn a_filled_run_keeps_its_stroke_and_a_stroke_only_run_is_counted() {
        let manager = manager_with_font();
        let mut filled = run_of_codes(&[65u16]);
        filled.fill = Some(Paint::Solid(Color::BLACK));
        let page = page_with(vec![DrawOp::Text(filled)]);
        let plain = render(&page, &manager);

        let mut outlined = run_of_codes(&[65u16]);
        outlined.fill = Some(Paint::Solid(Color::BLACK));
        outlined.stroke = Some(Paint::Solid(Color::rgba(255, 0, 0, 255)));
        outlined.stroke_style = StrokeStyle {
            width: 0.4,
            ..StrokeStyle::default()
        };
        let page = page_with(vec![DrawOp::Text(outlined)]);
        let stroked = render(&page, &manager);

        assert_eq!(stroked.stroke_only_runs, 0);
        assert_eq!(stroked.missing_glyph_runs, 0);
        let ink = |out: &RasterOutput| {
            out.pixmap()
                .data()
                .as_chunks::<4>()
                .0
                .iter()
                .filter(|p| p[3] > 0 && (p[0] < 250 || p[1] < 250 || p[2] < 250))
                .count()
        };
        assert!(
            ink(&stroked) > ink(&plain),
            "the stroke should add mark around the fill"
        );
    }

    /// Nothing can be painted from a stroke alone here, so the run is counted
    /// rather than dropped in silence.
    #[test]
    fn a_stroke_only_run_is_counted_rather_than_drawn() {
        let manager = manager_with_font();
        let mut run = run_of_codes(&[65u16]);
        run.fill = None;
        run.stroke = Some(Paint::Solid(Color::BLACK));
        let page = page_with(vec![DrawOp::Text(run)]);
        let out = render(&page, &manager);
        assert_eq!(out.stroke_only_runs, 1);
        assert!(!out.is_complete());
    }

    /// A run of the glyphs a test wants, placed plainly.
    fn run_of_codes(codes: &[u16]) -> sofd_display::GlyphRun {
        sofd_display::GlyphRun {
            font: FontId(0),
            size: sofd_common::Millimeter::new(8.0),
            glyphs: codes.to_vec(),
            advances: vec![5.0; codes.len()],
            text: None,
            cluster_map: None,
            codes_are_glyphs: false,
            transform: Affine::translate(2.0, 14.0),
            glyph_angle: 0.0,
            fill: Some(Paint::Solid(Color::BLACK)),
            stroke: None,
            stroke_style: Default::default(),
            letter_spacing: 0.0,
            word_spacing: 0.0,
            bbox: Rect::new(2.0, 6.0, 10.0, 8.0),
        }
    }

    #[test]
    fn an_oversized_page_refuses_to_rasterize() {
        let manager = FontManager::new();
        let options = RenderOptions::default();
        let huge = DisplayListBuilder::new(Rect::new(0.0, 0.0, 100000.0, 100000.0)).finish(0);
        let err = render_page(&huge, &manager, &options).expect_err("must fail");
        assert!(err.to_string().contains("budget"), "{err}");
    }

    /// The mask a clip builds starts opaque: `Mask::new` hands back a fully
    /// transparent buffer and `intersect_path` multiplies into it, so an
    /// unseeded mask would clip every pixel away.
    #[test]
    fn a_clip_keeps_the_pixels_inside_its_region() {
        let manager = FontManager::new();
        let mut b = DisplayListBuilder::new(small_box());
        b.save().expect("save");
        b.push(DrawOp::ClipPath {
            path: rect_path(10.0, 10.0, 20.0, 20.0),
            rule: PaintRule::NonZero,
        });
        b.push(fill_rect_op(
            0.0,
            0.0,
            40.0,
            40.0,
            Color::rgba(255, 0, 0, 255),
        ));
        b.restore().expect("restore");
        let out = render(&b.finish(0), &manager);
        let inside = pixel_at(&out, 38, 38);
        let outside = pixel_at(&out, 5, 5);
        assert_ne!(inside, [255, 255, 255, 255], "inside the clip is painted");
        assert_eq!(
            outside,
            [255, 255, 255, 255],
            "outside the clip stays blank"
        );
    }

    #[test]
    fn a_transparent_background_keeps_alpha_zero() {
        let manager = FontManager::new();
        let options = RenderOptions {
            background: None,
            ..RenderOptions::default()
        };
        let page = page_with(Vec::new());
        let out = render_page(&page, &manager, &options).expect("render");
        assert_eq!(
            pixel_at(&out, 5, 5)[3],
            0,
            "no background means transparent"
        );
    }

    #[test]
    fn output_pixels_are_straight_alpha() {
        let manager = FontManager::new();
        let options = RenderOptions {
            background: Some(Color::rgba(255, 0, 0, 128)),
            ..RenderOptions::default()
        };
        let page = page_with(Vec::new());
        let out = render_page(&page, &manager, &options).expect("render");
        let bytes = out.rgba_bytes();
        // Premultiplied red at half alpha would read 128; straight keeps 255.
        assert_eq!(bytes[0], 255, "red channel un-premultiplied");
        assert_eq!(bytes[3], 128, "alpha preserved");
    }

    #[test]
    fn an_arc_path_rasterizes_without_error() {
        let manager = FontManager::new();
        let arc = Path::build(|b| {
            b.move_to(5.0, 10.0);
        });
        let arc = {
            // Reuse the segment form a parser would emit for `A`.
            let mut p = arc;
            p.segments.push(PathSegment::Arc {
                rx: 5.0,
                ry: 5.0,
                x_axis_rotation: 0.0,
                large_arc: false,
                sweep: true,
                to: (15.0, 10.0),
            });
            p
        };
        let page = page_with(vec![
            fill_rect_op(0.0, 0.0, 0.0, 0.0, Color::BLACK),
            DrawOp::Path {
                path: arc,
                fill: Some(Paint::Solid(Color::rgba(0, 0, 0, 255))),
                stroke: None,
                stroke_style: StrokeStyle::default(),
                rule: PaintRule::NonZero,
            },
        ]);
        let out = render(&page, &manager);
        let dark = (0..out.width)
            .step_by(2)
            .flat_map(|x| (0..out.height).step_by(2).map(move |y| (x, y)))
            .filter(|(x, y)| pixel_at(&out, *x, *y)[0] < 128)
            .count();
        assert!(dark > 3, "the semicircle contributes ink, found {dark}");
    }
}
