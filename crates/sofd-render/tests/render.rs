//! The render layer driven as a consumer.
//! A page is rasterized through `render_page` and inspected pixel by pixel, so
//! the tests assert what reaches the buffer, plus the budget and size guards.

use std::sync::Arc;

use sofd_common::{Millimeter, Rect};
use sofd_display::ops::{Color, Paint, PaintRule, StrokeStyle};
use sofd_display::text::GlyphRun;
use sofd_display::{DisplayListBuilder, DrawOp, ImageCodec, ImageRef, PageDisplay};
use sofd_font::{FontId, FontManager};
use sofd_geometry::{Affine, Path};
use sofd_render::{RenderOptions, render_page};

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

/// The RGBA pixel at (x, y) in a rasterization.
fn pixel(output: &sofd_render::RasterOutput, x: u32, y: u32) -> [u8; 4] {
    let bytes = output.rgba_bytes();
    let base = ((y * output.width + x) * 4) as usize;
    [
        bytes[base],
        bytes[base + 1],
        bytes[base + 2],
        bytes[base + 3],
    ]
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

#[test]
fn a_blank_page_rasterizes_to_the_requested_size() {
    let fonts = FontManager::new();
    let output =
        render_page(&page_with(Vec::new()), &fonts, &RenderOptions::default()).expect("render");
    // A4 at 96 DPI is about 794x1123 pixels.
    assert!(
        (output.width as i64 - 794).abs() <= 2,
        "width {}",
        output.width
    );
    assert!(
        (output.height as i64 - 1123).abs() <= 2,
        "height {}",
        output.height
    );
    // The default background is white, so a blank page is uniformly white.
    assert_eq!(pixel(&output, 10, 10), [255, 255, 255, 255]);
}

#[test]
fn the_background_colour_is_applied_before_the_content() {
    let fonts = FontManager::new();
    let options = RenderOptions {
        background: Some(Color::rgba(10, 20, 30, 255)),
        ..Default::default()
    };
    let output = render_page(&page_with(Vec::new()), &fonts, &options).expect("render");
    assert_eq!(pixel(&output, 5, 5), [10, 20, 30, 255]);
}

#[test]
fn a_transparent_background_leaves_the_page_clear() {
    let fonts = FontManager::new();
    let options = RenderOptions {
        background: None,
        ..Default::default()
    };
    let output = render_page(&page_with(Vec::new()), &fonts, &options).expect("render");
    assert_eq!(pixel(&output, 5, 5), [0, 0, 0, 0], "alpha must be zero");
}

#[test]
fn a_filled_rectangle_paints_the_pixels_it_covers() {
    let fonts = FontManager::new();
    let page = page_with(vec![filled(
        rect(10.0, 10.0, 50.0, 50.0),
        Color::rgba(255, 0, 0, 255),
    )]);
    let output = render_page(&page, &fonts, &RenderOptions::default()).expect("render");

    // 10mm at 96 DPI is about 38px, so (100, 100) is well inside the rectangle
    // and (600, 600) is well outside it.
    let inside = pixel(&output, 100, 100);
    assert_eq!(inside[0], 255, "red channel inside: {inside:?}");
    assert!(
        inside[1] < 60 && inside[2] < 60,
        "should be red, got {inside:?}"
    );

    // A point outside the rectangle stays white.
    assert_eq!(pixel(&output, 600, 600), [255, 255, 255, 255]);
}

#[test]
fn the_pixel_budget_is_enforced_before_the_raster_is_allocated() {
    let fonts = FontManager::new();
    let options = RenderOptions {
        // Far too small for an A4 page at 96 DPI.
        max_pixels: 1024,
        ..Default::default()
    };
    let err = render_page(&page_with(Vec::new()), &fonts, &options).expect_err("budget");
    assert!(err.to_string().contains("budget"), "{err}");
}

#[test]
fn a_higher_dpi_produces_a_larger_raster() {
    let fonts = FontManager::new();
    let page = page_with(Vec::new());
    let low = render_page(&page, &fonts, &RenderOptions::default()).expect("96");
    let high = render_page(
        &page,
        &fonts,
        &RenderOptions {
            dpi: 192.0,
            ..Default::default()
        },
    )
    .expect("192");
    assert!(high.width > low.width);
    assert!(high.height > low.height);
}

#[test]
fn an_unbalanced_restore_is_reported_rather_than_panicking() {
    let fonts = FontManager::new();
    // A hand-built page can carry a restore with no save; the renderer must
    // report it instead of indexing into an empty stack.
    let page = page_with(vec![DrawOp::Restore]);
    let err = render_page(&page, &fonts, &RenderOptions::default()).expect_err("unbalanced");
    assert!(err.to_string().contains("restore"), "{err}");
}

#[test]
fn a_transform_moves_the_painted_pixels() {
    let fonts = FontManager::new();
    // A small square at the origin, translated right by 100mm.
    let page = page_with(vec![
        DrawOp::Transform(Affine::translate(100.0, 100.0)),
        filled(rect(0.0, 0.0, 20.0, 20.0), Color::rgba(0, 0, 255, 255)),
    ]);
    let output = render_page(&page, &fonts, &RenderOptions::default()).expect("render");
    // The translated square covers around (100mm, 100mm), not the origin.
    let moved = pixel(&output, 400, 400);
    assert!(
        moved[2] > 200,
        "expected blue where the square moved, got {moved:?}"
    );
    assert_eq!(
        pixel(&output, 10, 10),
        [255, 255, 255, 255],
        "origin stays clear"
    );
}

#[test]
fn a_window_away_from_the_corner_shows_what_belongs_there() {
    let fonts = FontManager::new();
    // Blue covers 100..120mm across and 80..100mm down, which is the whole of
    // the window asked for below and none of the window before it.
    let page = page_with(vec![filled(
        rect(100.0, 80.0, 20.0, 20.0),
        Color::rgba(0, 0, 255, 255),
    )]);
    let at = |x: f64, y: f64| {
        render_page(
            &page,
            &fonts,
            &RenderOptions {
                window: Some(Rect::new(x, y, 40.0, 40.0)),
                ..Default::default()
            },
        )
        .expect("render")
    };

    // 96 DPI puts 100mm at 378px, so the square sits 27..103px into a window
    // that starts at 90mm. Halfway in is inside it.
    let windowed = at(90.0, 70.0);
    let middle = pixel(&windowed, 64, 64);
    assert!(
        middle[2] > 200 && middle[0] < 60,
        "the window starting before the square should hold it, got {middle:?}"
    );

    // The window before it holds nothing, and is a window's size rather than the
    // page's: a raster ignoring the origin paints the square here instead. 40mm
    // at 96 DPI is 151.18px, which rounds up to 152.
    let before = at(20.0, 20.0);
    assert_eq!((before.width, before.height), (152, 152));
    assert_eq!(pixel(&before, 64, 64), [255, 255, 255, 255]);
}

#[test]
fn a_clip_confines_the_painted_pixels() {
    let fonts = FontManager::new();
    // A large red rectangle clipped to a small square at the origin.
    let page = page_with(vec![
        DrawOp::ClipPath {
            path: rect(0.0, 0.0, 20.0, 20.0),
            rule: PaintRule::NonZero,
        },
        filled(rect(0.0, 0.0, 200.0, 200.0), Color::rgba(255, 0, 0, 255)),
    ]);
    let output = render_page(&page, &fonts, &RenderOptions::default()).expect("render");
    // Inside the clip is painted; well outside it is not, even though the path
    // covered it.
    assert_eq!(pixel(&output, 30, 30)[0], 255);
    assert_eq!(pixel(&output, 300, 300), [255, 255, 255, 255]);
}

#[test]
fn a_text_clip_confines_the_paint_to_the_glyph_outlines() {
    let (fonts, id) = manager_with_fixture();
    // A large red rectangle clipped to one big glyph at (60, 80): the red
    // survives only where the glyph's outline covers it, so the corner far
    // from the glyph stays white even though the rectangle reached it.
    let run = GlyphRun {
        font: id,
        size: Millimeter(40.0),
        glyphs: vec!['H' as u16],
        advances: vec![0.0],
        text: Some("H".to_string()),
        cluster_map: None,
        codes_are_glyphs: false,
        transform: Affine::translate(60.0, 80.0),
        glyph_angle: 0.0,
        fill: Some(Paint::Solid(Color::BLACK)),
        stroke: None,
        stroke_style: Default::default(),
        letter_spacing: 0.0,
        bbox: Rect::new(0.0, 0.0, 0.0, 0.0),
    };
    let page = page_with(vec![
        DrawOp::ClipText(run),
        filled(rect(0.0, 0.0, 200.0, 200.0), Color::rgba(255, 0, 0, 255)),
    ]);
    let output = render_page(&page, &fonts, &RenderOptions::default()).expect("render");
    let red = output
        .rgba_bytes()
        .as_chunks::<4>()
        .0
        .iter()
        .filter(|p| p[0] > 180 && p[1] < 80 && p[2] < 80)
        .count();
    // The whole rectangle is 200x200 mm; the clip must cut it down to glyphs.
    let whole = output.rgba_bytes().len() / 4;
    assert!(
        red > 0 && red < whole / 4,
        "the glyph clip must keep some paint and drop most of it, got {red} of {whole}"
    );
    assert_eq!(
        pixel(&output, 5, 5),
        [255, 255, 255, 255],
        "a corner clear of the glyph stays unpainted"
    );
}

#[test]
fn a_clip_of_two_subpaths_paints_both_of_them() {
    let fonts = FontManager::new();
    // Clause 8.4: the areas of a clip cover their union, so a clip path made of
    // two separate squares leaves the gap between them unpainted and both
    // squares painted. Reading the areas as one each would lose half.
    let clip = Path::build(|b| {
        b.rect(0.0, 0.0, 20.0, 20.0);
        b.rect(60.0, 0.0, 20.0, 20.0);
    });
    let page = page_with(vec![
        DrawOp::ClipPath {
            path: clip,
            rule: PaintRule::NonZero,
        },
        filled(rect(0.0, 0.0, 200.0, 200.0), Color::rgba(255, 0, 0, 255)),
    ]);
    let output = render_page(&page, &fonts, &RenderOptions::default()).expect("render");
    assert_eq!(pixel(&output, 30, 30)[0], 255, "the first square");
    assert_eq!(pixel(&output, 190, 30)[0], 255, "the second square");
    assert_eq!(
        pixel(&output, 110, 30),
        [255, 255, 255, 255],
        "the gap between them is outside the union"
    );
}

#[test]
fn a_clip_lasts_until_the_scope_that_set_it_closes() {
    let fonts = FontManager::new();
    // A clip inside a saved scope, over a red square that the clip covers
    // whole. The blue after the restore is a different square, so a blue pixel
    // there can only have arrived because the clip was gone by then.
    let page = page_with(vec![
        DrawOp::Save,
        DrawOp::ClipPath {
            path: rect(0.0, 0.0, 40.0, 40.0),
            rule: PaintRule::NonZero,
        },
        filled(rect(0.0, 0.0, 40.0, 40.0), Color::rgba(255, 0, 0, 255)),
        DrawOp::Restore,
        filled(rect(60.0, 60.0, 60.0, 60.0), Color::rgba(0, 0, 255, 255)),
    ]);
    let output = render_page(&page, &fonts, &RenderOptions::default()).expect("render");
    assert_eq!(pixel(&output, 30, 30), [255, 0, 0, 255], "inside the clip");
    assert_eq!(
        pixel(&output, 300, 300),
        [0, 0, 255, 255],
        "past the restore the clip is gone, so blue reaches here"
    );
    assert_eq!(
        pixel(&output, 30, 300),
        [255, 255, 255, 255],
        "and the clip still held while it was in force"
    );
}

#[test]
fn nested_clips_intersect_rather_than_replace() {
    let fonts = FontManager::new();
    // The first clip is two disjoint squares, so dropping it leaves paint where
    // the second clip alone would still allow some: the result distinguishes an
    // intersection from a replacement, which one square and one clip would not.
    let outer = Path::build(|b| {
        b.rect(0.0, 0.0, 20.0, 20.0);
        b.rect(40.0, 40.0, 20.0, 20.0);
    });
    let page = page_with(vec![
        DrawOp::ClipPath {
            path: outer,
            rule: PaintRule::NonZero,
        },
        DrawOp::ClipPath {
            path: rect(0.0, 0.0, 30.0, 30.0),
            rule: PaintRule::NonZero,
        },
        filled(rect(0.0, 0.0, 200.0, 200.0), Color::rgba(255, 0, 0, 255)),
    ]);
    let output = render_page(&page, &fonts, &RenderOptions::default()).expect("render");
    let k = RenderOptions::default().dpi / 25.4;
    let at = |mm: f64| (mm * k) as u32;
    assert_eq!(
        pixel(&output, at(10.0), at(10.0)),
        [255, 0, 0, 255],
        "the overlap of the two clips is painted"
    );
    assert_eq!(
        pixel(&output, at(18.0), at(6.0)),
        [255, 0, 0, 255],
        "including the part only the second clip's outline would have allowed"
    );
    assert_eq!(
        pixel(&output, at(25.0), at(25.0)),
        [255, 255, 255, 255],
        "inside the second clip only, which the first one excludes"
    );
    assert_eq!(
        pixel(&output, at(50.0), at(50.0)),
        [255, 255, 255, 255],
        "the first clip's second square is outside the second clip"
    );
}

#[test]
fn a_run_with_no_glyphs_paints_nothing() {
    let (fonts, id) = manager_with_fixture();
    // An empty run is a legitimate input — a blank line in a text block — and
    // must leave the page untouched rather than fail.
    let run = GlyphRun {
        font: id,
        size: Millimeter(20.0),
        glyphs: Vec::new(),
        advances: Vec::new(),
        text: Some(String::new()),
        cluster_map: None,
        codes_are_glyphs: false,
        transform: Affine::IDENTITY,
        glyph_angle: 0.0,
        fill: Some(Paint::Solid(Color::BLACK)),
        stroke: None,
        stroke_style: Default::default(),
        letter_spacing: 0.0,
        bbox: Rect::new(0.0, 0.0, 0.0, 0.0),
    };
    let page = page_with(vec![DrawOp::Text(run)]);
    let output = render_page(&page, &fonts, &RenderOptions::default()).expect("render");
    assert_eq!(pixel(&output, 50, 50), [255, 255, 255, 255]);
}

#[test]
fn a_degenerate_page_box_still_yields_one_pixel() {
    let fonts = FontManager::new();
    // A zero-size box is degenerate but not fatal: the size helper floors at a
    // single pixel rather than producing a zero-dimension raster that no
    // encoder could write.
    let mut builder = DisplayListBuilder::new(Rect::new(0.0, 0.0, 0.0, 0.0));
    builder.push(filled(rect(0.0, 0.0, 1.0, 1.0), Color::BLACK));
    let page = builder.finish(0);
    let output = render_page(&page, &fonts, &RenderOptions::default()).expect("render");
    assert_eq!((output.width, output.height), (1, 1));
    assert_eq!(output.rgba_bytes().len(), 4, "one RGBA pixel");
}

/// The placement transform used to be composed the wrong way round, so a
/// picture landed thousands of pixels off the page and rendered as pure white
/// while the command still reported success.
#[test]
fn an_image_lands_inside_the_rect_its_boundary_names() {
    let fonts = FontManager::new();
    // A 4x4 red PNG, drawn into a 40x40 mm box 20 mm from each edge.
    let png = {
        let mut px = tiny_skia::Pixmap::new(4, 4).expect("pixmap");
        px.fill(tiny_skia::Color::from_rgba8(255, 0, 0, 255));
        px.encode_png().expect("png")
    };
    let dst = Rect::new(20.0, 20.0, 40.0, 40.0);
    let image = ImageRef::new("1", Arc::new(png), (4, 4), ImageCodec::Png);
    let page = page_with(vec![DrawOp::Image { image, dst }]);
    let options = RenderOptions::default();
    let output = render_page(&page, &fonts, &options).expect("render");
    let k = options.dpi / 25.4;
    let cx = ((dst.x + dst.w / 2.0) * k) as u32;
    let cy = ((dst.y + dst.h / 2.0) * k) as u32;
    assert_eq!(
        pixel(&output, cx, cy),
        [255, 0, 0, 255],
        "red at the box centre"
    );
    assert_eq!(
        pixel(&output, 2, 2),
        [255, 255, 255, 255],
        "the top-left corner stays outside the box"
    );
}

/// JBIG2 is the encoding a scanned page carries, and `image` reads it through
/// the decoder `hayro-jbig2` registers, so it is named rather than guessed: an
/// embedded stream has no signature `image` could look for.
#[test]
fn a_jbig2_image_is_decoded_and_painted() {
    let fonts = FontManager::new();
    let data = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/qr.jb2"))
        .expect("the sample is present");
    let (w, h) = sofd_io::decode::pixel_size(&data, ImageCodec::Jbig2).expect("dimensions");
    assert_eq!((w, h), (100, 100), "the sample is a 100x100 QR code");
    let dst = Rect::new(20.0, 20.0, 50.0, 50.0);
    let image = ImageRef::new("1", Arc::new(data), (w, h), ImageCodec::Jbig2);
    let page = page_with(vec![DrawOp::Image { image, dst }]);
    let output = render_page(&page, &fonts, &RenderOptions::default()).expect("render");
    let bytes = output.rgba_bytes();
    let (pixels, _) = bytes.as_chunks::<4>();
    let dark = pixels.iter().filter(|p| p[0] < 128).count();
    assert!(
        dark > 1000,
        "the QR code paints {dark} dark pixels, not none"
    );
}

/// The decoded page is not a black rectangle: a QR code is mostly white with
/// dark modules, so a fully dark result would mean the bit order is inverted.
#[test]
fn a_jbig2_page_is_black_on_white_rather_than_inverted() {
    let _fonts = FontManager::new();
    let data = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/qr.jb2"))
        .expect("the sample is present");
    let decoded = sofd_io::decode(&data, ImageCodec::Jbig2, sofd_io::Pixels::Rgb).expect("decode");
    assert_eq!(decoded.bytes.len(), 100 * 100 * 3, "three bytes per pixel");
    let black = decoded.bytes.iter().filter(|&&v| v == 0).count();
    let ratio = black as f64 / decoded.bytes.len() as f64;
    assert!(
        (0.2..0.6).contains(&ratio),
        "black covers {ratio:.2} of the page, which is neither blank nor solid"
    );
}

/// A resource the archive never supplied leaves the area empty rather than
/// failing the page, which is what a missing file has always meant.
#[test]
fn a_placement_with_no_bytes_leaves_the_area_untouched() {
    let fonts = FontManager::new();
    let image = ImageRef::new("1", Arc::new(Vec::new()), (1, 1), ImageCodec::Png);
    let page = page_with(vec![DrawOp::Image {
        image,
        dst: Rect::new(10.0, 10.0, 50.0, 50.0),
    }]);
    let output = render_page(&page, &fonts, &RenderOptions::default()).expect("render");
    assert_eq!(pixel(&output, 100, 100), [255, 255, 255, 255]);
}

/// GIF is an encoding GB/T 33190's own test documents embed, so the decoder
/// has to read it rather than fail the page that places it.
#[test]
fn a_gif_image_is_decoded_and_painted() {
    let fonts = FontManager::new();
    let data = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/data/block.gif"))
        .expect("the sample is present");
    let dst = Rect::new(20.0, 20.0, 40.0, 30.0);
    let image = ImageRef::new("1", Arc::new(data), (40, 30), ImageCodec::Gif);
    let page = page_with(vec![DrawOp::Image { image, dst }]);
    let output = render_page(&page, &fonts, &RenderOptions::default()).expect("render");
    let bytes = output.rgba_bytes();
    let (pixels, _) = bytes.as_chunks::<4>();
    let dark = pixels.iter().filter(|p| p[0] < 128).count();
    let light = pixels.iter().filter(|p| p[0] > 200).count();
    assert!(
        dark > 100 && light > 100,
        "the sample's black block and white ground both paint, got {dark} dark and {light} light"
    );
}

/// A resource whose bytes will not decode must cost only its own placement:
/// the rest of the page still renders, and the miss is counted so a caller can
/// tell a blank area from an absent one.
#[test]
fn an_image_that_will_not_decode_is_skipped_not_fatal() {
    let fonts = FontManager::new();
    let broken = ImageRef::new("7", Arc::new(vec![0u8; 32]), (1, 1), ImageCodec::Png);
    let page = page_with(vec![
        filled(rect(0.0, 0.0, 50.0, 50.0), Color::rgba(0, 0, 0, 255)),
        DrawOp::Image {
            image: broken,
            dst: Rect::new(10.0, 10.0, 20.0, 20.0),
        },
    ]);
    let output = render_page(&page, &fonts, &RenderOptions::default()).expect("render");
    assert_eq!(output.skipped_image_ops, 1, "the bad image is counted");
    // The op that came before it still painted.
    assert_eq!(pixel(&output, 5, 5), [0, 0, 0, 255]);
}

/// A mesh fill interpolates its vertex colours across the path it fills, which
/// is what clauses 8.3.4.4 and 8.3.4.5 ask for. The left vertex is red, the right
/// is blue, so the midpoint is a blend rather than either endpoint.
#[test]
fn a_mesh_fill_interpolates_between_its_vertex_colours() {
    let fonts = FontManager::new();
    let spec = sofd_display::MeshSpec {
        points: vec![
            sofd_display::MeshVertex {
                x: 0.0,
                y: 0.0,
                color: Color::rgba(255, 0, 0, 255),
            },
            sofd_display::MeshVertex {
                x: 20.0,
                y: 0.0,
                color: Color::rgba(0, 0, 255, 255),
            },
            sofd_display::MeshVertex {
                x: 20.0,
                y: 20.0,
                color: Color::rgba(0, 0, 255, 255),
            },
            sofd_display::MeshVertex {
                x: 0.0,
                y: 20.0,
                color: Color::rgba(255, 0, 0, 255),
            },
        ],
        triangles: vec![[0, 1, 3], [1, 2, 3]],
    };
    let page = page_with(vec![DrawOp::Path {
        path: rect(2.0, 2.0, 16.0, 16.0),
        fill: Some(Paint::Mesh(Box::new(spec))),
        stroke: None,
        stroke_style: StrokeStyle::default(),
        rule: PaintRule::NonZero,
    }]);
    let output = render_page(&page, &fonts, &RenderOptions::default()).expect("render");
    // At 96 DPI the 20 mm page is about 76 px, so the rect spans ~8..68.
    let left = pixel(&output, 12, 38);
    let right = pixel(&output, 63, 38);
    let middle = pixel(&output, 38, 38);
    assert!(
        left[0] > left[2],
        "the left edge is nearer the red vertex: {left:?}"
    );
    assert!(
        right[2] > right[0],
        "the right edge is nearer the blue vertex: {right:?}"
    );
    assert!(
        middle[0] > 0 && middle[2] > 0,
        "the middle blends both, not either end: {middle:?}"
    );
}

/// A mesh is clipped by the path it fills, so a triangle reaching past that
/// path does not paint outside it.
#[test]
fn a_mesh_fill_stays_inside_its_path() {
    let fonts = FontManager::new();
    let spec = sofd_display::MeshSpec {
        points: vec![
            sofd_display::MeshVertex {
                x: 0.0,
                y: 0.0,
                color: Color::rgba(0, 255, 0, 255),
            },
            sofd_display::MeshVertex {
                x: 20.0,
                y: 0.0,
                color: Color::rgba(0, 255, 0, 255),
            },
            sofd_display::MeshVertex {
                x: 0.0,
                y: 20.0,
                color: Color::rgba(0, 255, 0, 255),
            },
        ],
        triangles: vec![[0, 1, 2]],
    };
    let page = page_with(vec![DrawOp::Path {
        path: rect(2.0, 2.0, 6.0, 6.0),
        fill: Some(Paint::Mesh(Box::new(spec))),
        stroke: None,
        stroke_style: StrokeStyle::default(),
        rule: PaintRule::NonZero,
    }]);
    let output = render_page(&page, &fonts, &RenderOptions::default()).expect("render");
    // The 6 mm path spans roughly px 8..30, so a point well beyond it is
    // covered by the triangle but not by the path.
    assert_eq!(
        pixel(&output, 45, 45),
        [255, 255, 255, 255],
        "inside the triangle but outside the path stays clear"
    );
    assert_eq!(pixel(&output, 15, 15)[1], 255, "inside the path is painted");
}

/// A pattern fill tiles its cell over the area, which is what clause 8.3.3
/// asks for. The cell paints a small square of its own, so a tiled fill shows
/// that square more than once rather than a single flat block.
#[test]
fn a_pattern_fill_tiles_its_cell_across_the_area() {
    let fonts = FontManager::new();
    let cell = {
        let mut builder = DisplayListBuilder::new(Rect::new(0.0, 0.0, 4.0, 4.0));
        builder.push(filled(
            rect(0.0, 0.0, 2.0, 4.0),
            Color::rgba(255, 0, 0, 255),
        ));
        builder.finish(0)
    };
    let spec = sofd_display::PatternSpec {
        cell: Box::new(cell),
        width: 4.0,
        height: 4.0,
        step_x: 4.0,
        step_y: 4.0,
        reflect: sofd_display::Reflect::Normal,
    };
    let page = page_with(vec![DrawOp::Path {
        path: rect(0.0, 0.0, 20.0, 20.0),
        fill: Some(Paint::Pattern(Box::new(spec))),
        stroke: None,
        stroke_style: StrokeStyle::default(),
        rule: PaintRule::NonZero,
    }]);
    let output = render_page(&page, &fonts, &RenderOptions::default()).expect("render");
    // The cell's left half is red and its right half is clear, so a row across
    // the tiles alternates instead of staying one colour. A cell is ~15 px at
    // 96 DPI, so the first repeat puts its clear half around x=20.
    let red = pixel(&output, 5, 38);
    let clear = pixel(&output, 12, 38);
    assert_eq!(red, [255, 0, 0, 255], "the cell's painted half shows");
    assert_eq!(
        clear,
        [255, 255, 255, 255],
        "the cell's clear half lets the page through"
    );
}

/// A pattern is clipped by the path it fills, so the tiling does not spill
/// outside the region it is meant to paint.
#[test]
fn a_pattern_fill_stays_inside_its_path() {
    let fonts = FontManager::new();
    let cell = {
        let mut builder = DisplayListBuilder::new(Rect::new(0.0, 0.0, 4.0, 4.0));
        builder.push(filled(
            rect(0.0, 0.0, 4.0, 4.0),
            Color::rgba(0, 128, 0, 255),
        ));
        builder.finish(0)
    };
    let spec = sofd_display::PatternSpec {
        cell: Box::new(cell),
        width: 4.0,
        height: 4.0,
        step_x: 4.0,
        step_y: 4.0,
        reflect: sofd_display::Reflect::Normal,
    };
    let page = page_with(vec![DrawOp::Path {
        path: rect(2.0, 2.0, 8.0, 8.0),
        fill: Some(Paint::Pattern(Box::new(spec))),
        stroke: None,
        stroke_style: StrokeStyle::default(),
        rule: PaintRule::NonZero,
    }]);
    let output = render_page(&page, &fonts, &RenderOptions::default()).expect("render");
    assert_eq!(
        pixel(&output, 60, 60),
        [255, 255, 255, 255],
        "beyond the path the page stays clear"
    );
    assert_eq!(pixel(&output, 15, 15)[1], 128, "inside the path is painted");
}

/// A circle, drawn as four quarter arcs, on a page that fills the
/// raster exactly: 20 mm across at 25.4 DPI, so a point's millimetres and its
/// pixels coincide and a rim of radius 8 must reach 8px in every direction.
fn circle_page(stroke_width: f64) -> PageDisplay {
    let mut rim = Path::default();
    rim.segments
        .push(sofd_geometry::PathSegment::MoveTo(18.0, 10.0));
    for to in [(10.0, 2.0), (2.0, 10.0), (10.0, 18.0), (18.0, 10.0)] {
        rim.segments.push(sofd_geometry::PathSegment::Arc {
            rx: 8.0,
            ry: 8.0,
            x_axis_rotation: 0.0,
            large_arc: false,
            sweep: false,
            to,
        });
    }
    PageDisplay {
        index: 0,
        page_box: Rect::new(0.0, 0.0, 20.0, 20.0),
        ops: vec![DrawOp::Path {
            path: rim,
            fill: None,
            stroke: Some(Paint::Solid(Color::rgba(0, 0, 0, 255))),
            stroke_style: StrokeStyle {
                width: stroke_width,
                ..StrokeStyle::default()
            },
            rule: PaintRule::NonZero,
        }],
        content_bbox: Rect::new(2.0, 2.0, 16.0, 16.0),
    }
}

/// How far the ink reaches along a ray from the centre, in pixels.
fn rim_reach(output: &sofd_render::RasterOutput, cx: f64, cy: f64, degrees: f64) -> u32 {
    let a = degrees.to_radians();
    let mut reach = 0;
    for t in 0..40u32 {
        let t = t as f64;
        let (x, y) = (cx + t * a.cos(), cy + t * a.sin());
        if x < 0.0 || y < 0.0 || x >= output.width as f64 || y >= output.height as f64 {
            break;
        }
        if pixel(output, x as u32, y as u32)[3] > 40 {
            reach = t as u32;
        }
    }
    reach
}

#[test]
fn a_one_pixel_stroke_follows_the_curve_it_outlines() {
    // tiny-skia paints a stroke under a pixel as a hairline, and that painter
    // drops cubic segments: the circle becomes a polygon up to a tenth of its
    // radius off the rim, which turned a real invoice's round mark into a square.
    let fonts = FontManager::new();
    let options = RenderOptions {
        background: None,
        dpi: 25.4,
        ..Default::default()
    };
    for width in [0.01, 0.02, 0.04] {
        let output = render_page(&circle_page(width), &fonts, &options).expect("render");
        let (cx, cy) = (output.width as f64 / 2.0, output.height as f64 / 2.0);
        let reaches: Vec<u32> = (0..24)
            .map(|i| rim_reach(&output, cx, cy, i as f64 * 15.0))
            .collect();

        let (min, max) = (
            *reaches.iter().min().unwrap(),
            *reaches.iter().max().unwrap(),
        );
        assert!(
            min > 0,
            "width {width}: the rim did not render at all, reaches {reaches:?}"
        );
        assert!(
            max - min <= 2,
            "width {width}: the rim is not a circle, reaches {reaches:?}"
        );
    }
}

/// Mark every pixel that has ink on it.
fn ink_map(output: &sofd_render::RasterOutput) -> Vec<bool> {
    let mut map = vec![false; (output.width * output.height) as usize];
    for y in 0..output.height {
        for x in 0..output.width {
            map[(y * output.width + x) as usize] = pixel(output, x, y)[3] > 40;
        }
    }
    map
}

/// The placeholder a real invoice puts before the amount in words, as the file
/// writes it: `M 10.07 5.54`, four `B` commands of three points each, then the
/// strokes of the cross, all moved five millimetres right and down onto a
/// twenty-millimetre page. Read one point out, as taking `B` for `C` does, the
/// last arc ends at `(10.07,8.04)` instead of `(10.07,5.54)` and the ring is
/// left open on its right, which is the break this test exists to catch.
fn placeholder_page(stroke_width: f64) -> PageDisplay {
    use sofd_geometry::PathSegment;
    // The file's own numbers, in the order they follow the `M`.
    const DATA: [(f64, f64); 12] = [
        (10.07, 3.04),
        (8.04, 1.0),
        (5.53, 1.0),
        (3.03, 1.0),
        (1.0, 3.04),
        (1.0, 5.54),
        (1.0, 8.04),
        (3.03, 10.08),
        (5.53, 10.08),
        (8.04, 10.08),
        (10.07, 8.04),
        (10.07, 5.54),
    ];
    const START: (f64, f64) = (10.07, 5.54);
    let shifted = |p: (f64, f64)| (p.0 + 5.0, p.1 + 5.0);
    let mut mark = Path::default();
    let from = shifted(START);
    mark.segments.push(PathSegment::MoveTo(from.0, from.1));
    for chunk in DATA.chunks(3) {
        mark.segments.push(PathSegment::CurveTo {
            c1: shifted(chunk[0]),
            c2: shifted(chunk[1]),
            to: shifted(chunk[2]),
        });
    }
    mark.segments.push(PathSegment::MoveTo(7.3, 7.3));
    mark.segments.push(PathSegment::LineTo(13.7, 13.7));
    mark.segments.push(PathSegment::MoveTo(7.3, 13.7));
    mark.segments.push(PathSegment::LineTo(13.7, 7.3));
    PageDisplay {
        index: 0,
        page_box: Rect::new(0.0, 0.0, 20.0, 20.0),
        ops: vec![DrawOp::Path {
            path: mark,
            fill: None,
            stroke: Some(Paint::Solid(Color::rgba(0, 0, 0, 255))),
            stroke_style: StrokeStyle {
                width: stroke_width,
                ..StrokeStyle::default()
            },
            rule: PaintRule::NonZero,
        }],
        content_bbox: Rect::new(6.0, 6.0, 10.0, 10.0),
    }
}

#[test]
fn a_placeholder_ring_written_as_four_cubics_comes_out_whole() {
    // Each arc has to end where the next begins, and the last where the ring
    // began. Read one point out and the ring is left open on its right: no
    // direction may fall short of the others.
    let fonts = FontManager::new();
    // Two pixels to the millimetre, so the ring's four-and-a-half-millimetre
    // radius shows as a circle about nine pixels out, where a gap of half a
    // millimetre on an axis cannot pass as part of it.
    let options = RenderOptions {
        background: None,
        dpi: 50.8,
        ..Default::default()
    };
    // The axis and diagonal directions, which is where a gap in the ring shows.
    let directions = [0.0_f64, 45.0, 90.0, 135.0, 180.0, 225.0, 270.0, 315.0];
    for width in [0.5, 1.0] {
        let output = render_page(&placeholder_page(width), &fonts, &options).expect("render");
        let reaches: Vec<f64> = directions
            .iter()
            .map(|d| reach_from_middle(&ink_map(&output), output.width, output.height, *d))
            .collect();
        let (min, max) = (
            reaches.iter().cloned().fold(f64::MAX, f64::min),
            reaches.iter().cloned().fold(f64::MIN, f64::max),
        );
        // A ring reaches the same distance in every direction; one with a gap in
        // it falls short where the gap is.
        assert!(
            max - min <= 1.5,
            "width {width}: the ring reaches {min:.1}px at its narrowest and {max:.1}px at \
             its widest, so it is not whole, reaches {reaches:?}"
        );
    }
}

/// The centre of everything inked, and how far the ink reaches in one direction
/// from it, in pixels.
fn reach_from_middle(map: &[bool], width: u32, height: u32, degrees: f64) -> f64 {
    let (mut x0, mut y0, mut x1, mut y1) = (f64::MAX, f64::MAX, f64::MIN, f64::MIN);
    for y in 0..height {
        for x in 0..width {
            if map[(y * width + x) as usize] {
                x0 = x0.min(x as f64);
                y0 = y0.min(y as f64);
                x1 = x1.max(x as f64 + 1.0);
                y1 = y1.max(y as f64 + 1.0);
            }
        }
    }
    let centre = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let (sin, cos) = degrees.to_radians().sin_cos();
    let mut reach = 0.0;
    // Walk out a tenth of a pixel at a time; a pixel is as fine as the edge can
    // be told, so the reach carries the grid's own error and no more.
    for step in 1..=300 {
        let t = step as f64 * 0.1;
        let px = (centre.0 + t * cos).floor();
        let py = (centre.1 + t * sin).floor();
        if px < 0.0 || py < 0.0 || px >= width as f64 || py >= height as f64 {
            break;
        }
        if map[(py as u32 * width + px as u32) as usize] {
            reach = t;
        }
    }
    reach
}
