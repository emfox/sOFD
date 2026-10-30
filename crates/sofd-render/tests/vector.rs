//! The vector backend driven as a consumer.
//! A page is walked into geometry, drawn onto a canvas the way a shell would
//! draw it, and compared with what the raster backend paints from the same
//! ops. The two must agree, or a caller that picks one gets a different page
//! than a caller that picks the other.

use std::sync::Arc;

use sofd_common::{Millimeter, Rect};
use sofd_display::ops::{Color, Paint, PaintRule, StrokeStyle};
use sofd_display::text::GlyphRun;
use sofd_display::{DisplayListBuilder, DrawOp, ImageCodec, ImageRef, PageDisplay};
use sofd_font::{FontId, FontManager};
use sofd_geometry::{Affine, Path};
use sofd_render::{RenderOptions, Shape, VectorPage, page_to_vector, render_page};

fn rect(x: f64, y: f64, w: f64, h: f64) -> Path {
    Path::build(|b| {
        b.rect(x, y, w, h);
    })
}

fn page_with(ops: Vec<DrawOp>) -> PageDisplay {
    let mut builder = DisplayListBuilder::new(Rect::A4_PORTRAIT);
    for op in ops {
        builder.push(op);
    }
    builder.finish(0)
}

fn filled(path: Path, color: Color) -> DrawOp {
    DrawOp::Path {
        path,
        fill: Some(Paint::Solid(color)),
        stroke: None,
        stroke_style: StrokeStyle::default(),
        rule: PaintRule::NonZero,
    }
}

/// The fixture face, registered under id 0.
fn manager_with_fixture() -> (FontManager, FontId) {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/Orbitron-Bold.ttf");
    let data = std::fs::read(path).expect("font fixture");
    let mut manager = FontManager::new();
    let id = manager.registry_mut().register_embedded(
        "Res/Orbitron-Bold.ttf",
        "Orbitron",
        Arc::new(data),
    );
    (manager, id)
}

fn run_of(text: &str, fonts: &FontManager, id: FontId, at: (f64, f64)) -> GlyphRun {
    let face = fonts.face(id).expect("the fixture loads");
    let glyphs: Vec<u16> = text
        .chars()
        .filter_map(|ch| face.glyph_id(u32::from(ch)))
        .collect();
    let advances: Vec<f64> = glyphs
        .iter()
        .map(|&g| face.advance_width(g) / 1000.0 * 20.0)
        .collect();
    GlyphRun {
        font: id,
        size: Millimeter(20.0),
        glyphs,
        advances,
        text: Some(text.to_string()),
        cluster_map: None,
        codes_are_glyphs: false,
        transform: Affine::translate(at.0, at.1),
        glyph_angle: 0.0,
        fill: Some(Paint::Solid(Color::BLACK)),
        stroke: None,
        letter_spacing: 0.0,
        word_spacing: 0.0,
        bbox: Rect::new(at.0, at.1, 0.0, 0.0),
    }
}

/// Draw a vector page the way a shell would: a clip applies until a reset, and
/// a shape is filled with its solid paint. Any other paint is left off, since
/// this is a check on the geometry rather than on the shading.
fn draw_onto_canvas(page: &VectorPage, dpi: f64) -> (u32, u32, Vec<u8>) {
    let scale = dpi / 25.4;
    let width = (page.page_box.w * scale).ceil().max(1.0) as u32;
    let height = (page.page_box.h * scale).ceil().max(1.0) as u32;
    let mut canvas = vec![255u8; (width * height * 4) as usize];
    let mut clip: Option<(Path, PaintRule)> = None;
    for shape in &page.shapes {
        match shape {
            Shape::Clip { path, rule } => clip = Some((path.clone(), *rule)),
            Shape::ClipReset => clip = None,
            Shape::Path {
                path, fill, rule, ..
            } => {
                let Some(Paint::Solid(color)) = fill else {
                    continue;
                };
                paint(
                    path,
                    *color,
                    *rule,
                    &clip,
                    &mut canvas,
                    width,
                    height,
                    scale,
                );
            }
            // This canvas fills solid colours only, so a bitmap has nothing to
            // say to it; what an image carries is checked on its own below.
            Shape::Image { .. } => {}
            Shape::Text { path, fill, .. } => {
                let Some(Paint::Solid(color)) = fill else {
                    continue;
                };
                paint(
                    path,
                    *color,
                    PaintRule::NonZero,
                    &clip,
                    &mut canvas,
                    width,
                    height,
                    scale,
                );
            }
        }
    }
    (width, height, canvas)
}

#[allow(clippy::too_many_arguments)]
fn paint(
    path: &Path,
    color: Color,
    rule: PaintRule,
    clip: &Option<(Path, PaintRule)>,
    canvas: &mut [u8],
    width: u32,
    height: u32,
    scale: f64,
) {
    let bbox = path.bounding_box();
    let x0 = (bbox.x * scale).floor().max(0.0) as u32;
    let y0 = (bbox.y * scale).floor().max(0.0) as u32;
    let x1 = ((bbox.x + bbox.w) * scale).ceil().min(width as f64) as u32;
    let y1 = ((bbox.y + bbox.h) * scale).ceil().min(height as f64) as u32;
    for y in y0..y1 {
        for x in x0..x1 {
            let px = (x as f64 + 0.5) / scale;
            let py = (y as f64 + 0.5) / scale;
            if !covers(path, rule, px, py) {
                continue;
            }
            if let Some((clip_path, clip_rule)) = clip
                && !covers(clip_path, *clip_rule, px, py)
            {
                continue;
            }
            let base = ((y * width + x) * 4) as usize;
            canvas[base] = color.r;
            canvas[base + 1] = color.g;
            canvas[base + 2] = color.b;
            canvas[base + 3] = 255;
        }
    }
}

/// Whether a point is inside a path, by counting the segments a ray to the
/// right crosses. Only the segments each test page uses are handled.
fn covers(path: &Path, rule: PaintRule, x: f64, y: f64) -> bool {
    let crossings = crossings_at(path, x, y);
    let mut winding = 0i32;
    for forward in crossings {
        winding += if forward { 1 } else { -1 };
    }
    match rule {
        PaintRule::NonZero => winding != 0,
        PaintRule::EvenOdd => winding.abs() % 2 == 1,
    }
}

fn crossings_at(path: &Path, x: f64, y: f64) -> Vec<bool> {
    use sofd_geometry::PathSegment;
    let mut out = Vec::new();
    let mut start = (0.0, 0.0);
    let mut current = (0.0, 0.0);
    let mut points: Vec<(f64, f64)> = Vec::new();
    for segment in &path.segments {
        match segment {
            PathSegment::MoveTo(px, py) => {
                flush(&points, x, y, &mut out);
                points.clear();
                start = (*px, *py);
                current = (*px, *py);
                points.push(start);
            }
            PathSegment::LineTo(px, py) => {
                points.push((*px, *py));
                current = (*px, *py);
            }
            PathSegment::Close => {
                points.push(start);
                flush(&points, x, y, &mut out);
                points.clear();
                current = start;
            }
            PathSegment::CurveTo { c1, c2, to } => {
                // Sampled densely enough for a check at this resolution.
                let from = current;
                for step in 1..=24 {
                    let t = step as f64 / 24.0;
                    let u = 1.0 - t;
                    let px = u * u * u * from.0
                        + 3.0 * u * u * t * c1.0
                        + 3.0 * u * t * t * c2.0
                        + t * t * t * to.0;
                    let py = u * u * u * from.1
                        + 3.0 * u * u * t * c1.1
                        + 3.0 * u * t * t * c2.1
                        + t * t * t * to.1;
                    points.push((px, py));
                }
                current = *to;
            }
            PathSegment::QuadTo { c, to } => {
                let from = current;
                for step in 1..=24 {
                    let t = step as f64 / 24.0;
                    let u = 1.0 - t;
                    let px = u * u * from.0 + 2.0 * u * t * c.0 + t * t * to.0;
                    let py = u * u * from.1 + 2.0 * u * t * c.1 + t * t * to.1;
                    points.push((px, py));
                }
                current = *to;
            }
            PathSegment::Arc { .. } => {}
        }
    }
    flush(&points, x, y, &mut out);
    out
}

fn flush(points: &[(f64, f64)], x: f64, y: f64, out: &mut Vec<bool>) {
    for pair in points.windows(2) {
        let (x0, y0) = pair[0];
        let (x1, y1) = pair[1];
        if (y0 <= y) != (y1 <= y) {
            let t = (y - y0) / (y1 - y0);
            let hit = x0 + t * (x1 - x0);
            if hit > x {
                out.push(y1 > y0);
            }
        }
    }
}

#[test]
fn a_filled_rectangle_is_the_same_shape_either_way() {
    let fonts = FontManager::new();
    let page = page_with(vec![filled(
        rect(20.0, 30.0, 40.0, 25.0),
        Color::rgba(0, 0, 255, 255),
    )]);
    let options = RenderOptions::default();
    let vector = page_to_vector(&page, &fonts, &options);
    assert_eq!(vector.shapes.len(), 1, "one shape, one entry");

    let raster = render_page(&page, &fonts, &options).expect("render");
    let bytes = raster.rgba_bytes();
    // The middle of the rectangle is painted, and the point past its right
    // edge is not, in both outputs.
    let scale = 96.0 / 25.4;
    let inside = |w: u32| (((40.0 * scale) as u32 * w + (30.0 * scale) as u32) * 4) as usize;
    let outside = |w: u32| (((40.0 * scale) as u32 * w + (80.0 * scale) as u32) * 4) as usize;
    assert_eq!(
        &bytes[inside(raster.width)..inside(raster.width) + 3],
        &[0, 0, 255]
    );
    assert_eq!(
        &bytes[outside(raster.width)..outside(raster.width) + 3],
        &[255, 255, 255]
    );

    let Shape::Path { path, rule, .. } = &vector.shapes[0] else {
        panic!("expected a path, got {:?}", vector.shapes[0]);
    };
    assert!(
        covers(path, *rule, 40.0, 40.0),
        "the vector path covers the middle"
    );
    assert!(!covers(path, *rule, 80.0, 40.0), "and stops at its edge");
    let bbox = path.bounding_box();
    assert!(
        (bbox.x - 20.0).abs() < 1e-9 && (bbox.w - 40.0).abs() < 1e-9,
        "{bbox:?}"
    );
}

#[test]
fn a_transform_reaches_the_geometry_the_caller_draws() {
    let fonts = FontManager::new();
    let page = page_with(vec![
        DrawOp::Transform(Affine::translate(50.0, 10.0)),
        filled(rect(0.0, 0.0, 10.0, 10.0), Color::BLACK),
    ]);
    let vector = page_to_vector(&page, &fonts, &RenderOptions::default());
    let Shape::Path { path, .. } = &vector.shapes[0] else {
        panic!("expected a path");
    };
    let bbox = path.bounding_box();
    assert!(
        (bbox.x - 50.0).abs() < 1e-9 && (bbox.y - 10.0).abs() < 1e-9,
        "the translate is in the geometry, not in a matrix the caller has to know: {bbox:?}"
    );
}

#[test]
fn a_page_of_text_carries_glyph_outlines_rather_than_codes() {
    let (fonts, id) = manager_with_fixture();
    let run = run_of("ABC", &fonts, id, (30.0, 60.0));
    let page = page_with(vec![DrawOp::Text(run)]);
    let vector = page_to_vector(&page, &fonts, &RenderOptions::default());
    let Shape::Text { path, fill, .. } = &vector.shapes[0] else {
        panic!("expected text, got {:?}", vector.shapes[0]);
    };
    assert!(fill.is_some(), "the run's paint travels with its outlines");
    assert!(path.len() > 3, "three letters are more than three segments");
    let bbox = path.bounding_box();
    assert!(
        bbox.x >= 29.0 && bbox.x < 31.0,
        "the outlines start where the run does: {bbox:?}"
    );
    assert!(
        bbox.w > 20.0 && bbox.w < 60.0,
        "and span the three letters at 20 mm: {bbox:?}"
    );
    // The canvased page agrees with the raster about where the ink is.
    let (width, height, canvas) = draw_onto_canvas(&vector, 96.0);
    let raster = render_page(&page, &fonts, &RenderOptions::default()).expect("render");
    assert_eq!((width, height), (raster.width, raster.height));
    let ink_columns = |bytes: &[u8], w: u32, h: u32| -> (u32, u32) {
        let (mut first, mut last) = (w, 0u32);
        for x in 0..w {
            for y in 0..h {
                let base = ((y * w + x) * 4) as usize;
                if bytes[base] < 128 {
                    first = first.min(x);
                    last = last.max(x);
                }
            }
        }
        (first, last)
    };
    let ours = ink_columns(&canvas, width, height);
    let theirs = ink_columns(&raster.rgba_bytes(), raster.width, raster.height);
    assert!(
        ours.0.abs_diff(theirs.0) <= 2 && ours.1.abs_diff(theirs.1) <= 2,
        "the two backends put the text in the same columns: {ours:?} against {theirs:?}"
    );
}

#[test]
fn a_clip_is_reset_where_the_scope_that_set_it_closes() {
    let fonts = FontManager::new();
    let page = page_with(vec![
        DrawOp::Save,
        DrawOp::ClipPath {
            path: rect(0.0, 0.0, 50.0, 50.0),
            rule: PaintRule::NonZero,
        },
        filled(rect(0.0, 0.0, 100.0, 100.0), Color::BLACK),
        DrawOp::Restore,
        filled(rect(0.0, 0.0, 100.0, 100.0), Color::rgba(255, 0, 0, 255)),
    ]);
    let vector = page_to_vector(&page, &fonts, &RenderOptions::default());
    let clips = vector
        .shapes
        .iter()
        .filter(|s| matches!(s, Shape::Clip { .. }))
        .count();
    let resets = vector
        .shapes
        .iter()
        .filter(|s| matches!(s, Shape::ClipReset))
        .count();
    assert_eq!(clips, 1, "the page states one clip");
    assert_eq!(resets, 1, "and the restore that closes its scope says so");
    // The reset comes before the second fill, not after it.
    let reset_at = vector
        .shapes
        .iter()
        .position(|s| matches!(s, Shape::ClipReset))
        .expect("a reset");
    assert_eq!(reset_at, 2, "the clip, the fill it governs, then the reset");
}

#[test]
fn a_windowed_page_covers_only_the_window() {
    let fonts = FontManager::new();
    let page = page_with(vec![filled(rect(0.0, 0.0, 210.0, 297.0), Color::BLACK)]);
    let options = RenderOptions {
        window: Some(Rect::new(10.0, 20.0, 30.0, 40.0)),
        ..RenderOptions::default()
    };
    let vector = page_to_vector(&page, &fonts, &options);
    assert!(
        (vector.page_box.x - 10.0).abs() < 1e-9,
        "{:?}",
        vector.page_box
    );
    assert!(
        (vector.page_box.h - 40.0).abs() < 1e-9,
        "{:?}",
        vector.page_box
    );
}

#[test]
fn an_image_travels_with_its_bytes_and_where_they_go() {
    // A canvas backend cannot draw a picture the engine decoded for it without
    // decoding it again, so the bytes come through as the package carries them
    // and the placement as one matrix.
    let fonts = FontManager::new();
    let png = {
        let mut px = tiny_skia::Pixmap::new(4, 2).expect("pixmap");
        px.fill(tiny_skia::Color::from_rgba8(255, 0, 0, 255));
        px.encode_png().expect("png")
    };
    let dst = Rect::new(20.0, 30.0, 40.0, 20.0);
    let image = ImageRef::new("7", Arc::new(png.clone()), (4, 2), ImageCodec::Png);
    let page = page_with(vec![DrawOp::Image {
        image,
        dst,
        transform: Affine::IDENTITY,
    }]);
    let vector = page_to_vector(&page, &fonts, &RenderOptions::default());
    let Shape::Image { image, to_page } = &vector.shapes[0] else {
        panic!("expected an image, got {:?}", vector.shapes[0]);
    };
    assert_eq!(
        image.data.as_slice(),
        png.as_slice(),
        "the bytes are the package's own"
    );
    assert_eq!(image.pixel_size, (4, 2));

    // The four pixel corners land on the four page corners of the box.
    let corner = |x: f64, y: f64| to_page.apply(x, y);
    let near = |a: (f64, f64), b: (f64, f64)| (a.0 - b.0).abs() < 1e-9 && (a.1 - b.1).abs() < 1e-9;
    assert!(
        near(corner(0.0, 0.0), (20.0, 30.0)),
        "{:?}",
        corner(0.0, 0.0)
    );
    assert!(
        near(corner(4.0, 2.0), (60.0, 50.0)),
        "{:?}",
        corner(4.0, 2.0)
    );
}

#[test]
fn a_rotated_page_carries_its_images_round_with_it() {
    // The matrix rather than a rectangle, because a placement can be turned and
    // a canvas takes a turned image the same way it takes an upright one.
    let fonts = FontManager::new();
    let png = {
        let mut px = tiny_skia::Pixmap::new(2, 2).expect("pixmap");
        px.fill(tiny_skia::Color::from_rgba8(0, 255, 0, 255));
        px.encode_png().expect("png")
    };
    let image = ImageRef::new("8", Arc::new(png), (2, 2), ImageCodec::Png);
    let page = page_with(vec![
        DrawOp::Transform(Affine::rotate_deg(90.0)),
        DrawOp::Image {
            image,
            dst: Rect::new(10.0, 0.0, 10.0, 10.0),
            transform: Affine::IDENTITY,
        },
    ]);
    let vector = page_to_vector(&page, &fonts, &RenderOptions::default());
    let Shape::Image { to_page, .. } = &vector.shapes[0] else {
        panic!("expected an image");
    };
    // A quarter turn sends the picture's own x axis down the page, and leaves
    // the two axes at right angles to each other.
    let origin = to_page.apply(0.0, 0.0);
    let along = |x: f64, y: f64| {
        let p = to_page.apply(x, y);
        (p.0 - origin.0, p.1 - origin.1)
    };
    let across = along(2.0, 0.0);
    let down = along(0.0, 2.0);
    assert!(
        across.1.abs() > across.0.abs(),
        "the x axis turned down the page: {across:?}"
    );
    let dot = across.0 * down.0 + across.1 * down.1;
    assert!(dot.abs() < 1e-9, "the axes stayed square: {dot}");
}

#[test]
fn a_page_reports_the_glyphs_it_could_not_draw() {
    // The raster counts what it lost; a caller drawing shapes has to be told
    // the same way, or a page missing a character looks complete.
    let (fonts, id) = manager_with_fixture();
    let run = run_of("AB", &fonts, id, (30.0, 60.0));
    let page = page_with(vec![DrawOp::Text(run)]);
    let clean = page_to_vector(&page, &fonts, &RenderOptions::default());
    assert_eq!(clean.missing_glyphs, 0, "the letters resolve");
    assert_eq!(clean.missing_glyph_runs, 0);

    // The fixture has no ideograph, and no substitute is chained here.
    let mut run = run_of("AB", &fonts, id, (30.0, 60.0));
    run.glyphs.push(0x4E2D);
    run.advances.push(20.0);
    run.text = Some("AB\u{4e2d}".to_string());
    let page = page_with(vec![DrawOp::Text(run)]);
    let partial = page_to_vector(&page, &fonts, &RenderOptions::default());
    assert_eq!(partial.missing_glyphs, 1, "one character had no outline");
    assert_eq!(partial.missing_glyph_runs, 1, "and one run was short");
    assert_eq!(partial.shapes.len(), 1, "the two letters are still drawn");
}
