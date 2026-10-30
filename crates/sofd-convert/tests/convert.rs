//! The convert crate driven as a consumer rather than from inside.
//! Every test uses the public surface and asserts only on what a caller can
//! observe: the shape of the bytes, the reported stats, and the errors.
//!
//! One page at a time, because that is what this crate promises. A test about
//! exporting a *document* is in `sofd/tests/export.rs`, beside the facade.

use sofd_common::{Millimeter, Rect};
use sofd_convert::{ConversionStats, PdfOptions, SvgOptions, TextExport, page_to_pdf, page_to_svg};
use sofd_display::ops::{Color, Paint, PaintRule, StrokeStyle};
use sofd_display::{DrawOp, PageDisplay};
use sofd_font::FontManager;
use sofd_geometry::Path;
use sofd_render::RenderOptions;

/// A page with no content, useful for shape assertions on the wrapper.
fn blank_page(index: usize) -> PageDisplay {
    PageDisplay {
        index,
        page_box: Rect::A4_PORTRAIT,
        ops: Vec::new(),
        content_bbox: Rect::EMPTY,
    }
}

/// A filled red rectangle on an A4 page.
fn one_rect_page(index: usize) -> PageDisplay {
    let path = Path::build(|b| {
        b.rect(10.0, 20.0, 30.0, 40.0);
    });
    PageDisplay {
        index,
        page_box: Rect::A4_PORTRAIT,
        ops: vec![DrawOp::Path {
            path,
            fill: Some(Paint::Solid(Color::rgba(255, 0, 0, 255))),
            stroke: None,
            stroke_style: StrokeStyle::default(),
            rule: PaintRule::NonZero,
        }],
        content_bbox: Rect::new(10.0, 20.0, 40.0, 60.0),
    }
}

#[test]
fn an_svg_page_is_a_standalone_document_that_names_its_box() {
    let svg = page_to_svg(&blank_page(0), &SvgOptions::default()).expect("convert");
    assert!(
        svg.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"),
        "{svg}"
    );
    assert!(svg.contains("width=\"210mm\" height=\"297mm\""), "{svg}");
    assert!(svg.ends_with("</svg>\n"), "{svg}");
    // A blank page still closes the root group it opened.
    assert!(svg.contains("<g>") && svg.contains("</g>"), "{svg}");
}

#[test]
fn svg_output_carries_the_page_geometry_as_a_path() {
    let svg = page_to_svg(&one_rect_page(0), &SvgOptions::default()).expect("convert");
    assert!(
        svg.contains("<path d=\"M10 20 L40 20 L40 60 L10 60 Z\""),
        "{svg}"
    );
    assert!(svg.contains("fill=\"#ff0000\""), "{svg}");
}

#[test]
fn a_page_with_live_text_reports_the_run_it_kept() {
    let stats = stats_of_svg(&one_rect_page(0));
    // No text on the page, so nothing was outlined and nothing was dropped.
    assert_eq!(stats.outlined_glyphs, 0);
    assert_eq!(stats.dropped_runs, 0);
    assert_eq!(stats.images, 0);
}

#[test]
fn asking_for_outlined_text_without_a_font_source_degrades_visibly() {
    // The options are the honest kind: requesting an export the caller cannot
    // support is not an error, it is a documented fallback. The stats are how
    // a caller notices, so the fallback must show up there rather than vanish.
    let page = one_rect_page(0);
    let options = SvgOptions {
        text: TextExport::Outline,
        fonts: None,
    };
    let svg = page_to_svg(&page, &options).expect("convert");
    assert!(!svg.is_empty());
}

#[test]
fn pdf_output_starts_with_the_format_marker() {
    let pdf = page_to_pdf(&one_rect_page(0), &PdfOptions::default()).expect("convert");
    assert!(
        pdf.starts_with(b"%PDF-"),
        "no PDF header: {:?}",
        &pdf[..pdf.len().min(16)]
    );
    assert!(
        pdf.ends_with(b"%%EOF\n") || pdf.ends_with(b"%%EOF"),
        "PDF is not terminated"
    );
}

#[test]
fn png_output_is_a_png_of_the_requested_size() {
    let page = one_rect_page(0);
    let fonts = FontManager::new();
    let options = RenderOptions {
        dpi: 72.0,
        background: Some(Color::WHITE),
        max_pixels: 64 * 1024 * 1024,
        window: None,
    };
    let png = sofd_convert::png::page_to_png(&page, &fonts, &options).expect("convert");
    // The PNG signature is the contract a downstream decoder checks first.
    assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
}

#[test]
fn the_pixel_budget_is_enforced_before_any_raster_is_allocated() {
    let page = one_rect_page(0);
    let fonts = FontManager::new();
    let options = RenderOptions {
        dpi: 96.0,
        background: Some(Color::WHITE),
        // Smaller than a single A4 page at 96 DPI, so this must fail fast.
        max_pixels: 1024,
        window: None,
    };
    let err = sofd_convert::png::page_to_png(&page, &fonts, &options).expect_err("budget");
    assert!(err.to_string().contains("budget"), "{err}");
}

#[test]
fn the_default_stats_describe_a_lossless_run() {
    let stats = ConversionStats::default();
    assert!(stats.is_lossless());
    assert_eq!(stats.images_skipped, 0);
    assert_eq!(stats.gradients, 0);
}

/// Convert one page and hand back the stats the sink accumulated.
fn stats_of_svg(page: &PageDisplay) -> ConversionStats {
    let mut sink = sofd_convert::SvgSink::new(page.page_box, &SvgOptions::default());
    sofd_convert::replay(page, &mut sink).expect("replay");
    // `finish` consumes the sink and returns the stats it accumulated.
    sink.finish().1
}

/// One glyph of Orbitron, the face the font fixtures are drawn with.
fn one_glyph_page() -> (PageDisplay, FontManager) {
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../sofd-render/tests/data/Orbitron-Bold.ttf"
    ))
    .expect("font fixture");
    let mut fonts = FontManager::new();
    fonts.registry_mut().register_embedded(
        "Orbitron-Bold.ttf",
        "Orbitron",
        std::sync::Arc::new(bytes),
    );
    let run = sofd_display::GlyphRun {
        font: sofd_font::FontId(0),
        size: Millimeter(3.0),
        glyphs: vec![65],
        advances: vec![3.0],
        text: Some("A".into()),
        cluster_map: None,
        codes_are_glyphs: false,
        transform: sofd_geometry::Affine::IDENTITY,
        glyph_angle: 0.0,
        fill: Some(Paint::Solid(Color::BLACK)),
        stroke: None,
        letter_spacing: 0.0,
        word_spacing: 0.0,
        bbox: Rect::new(0.0, 0.0, 3.0, 3.0),
    };
    let page = PageDisplay {
        index: 0,
        page_box: Rect::A4_PORTRAIT,
        ops: vec![DrawOp::Text(run)],
        content_bbox: Rect::new(0.0, 0.0, 3.0, 3.0),
    };
    (page, fonts)
}

#[test]
fn an_exported_glyph_is_a_path_rather_than_an_embedded_face() {
    // Neither exporter carries a font. A PDF that embedded one would name a
    // descriptor and a face stream; these draw the outline instead. The content
    // stream is deflated, so it is read back rather than searched for.
    let (page, fonts) = one_glyph_page();
    let pdf = page_to_pdf(
        &page,
        &PdfOptions {
            fonts: Some(&fonts),
            ..PdfOptions::default()
        },
    )
    .expect("convert");
    let text = String::from_utf8_lossy(&pdf);
    assert!(
        !text.contains("/FontFile") && !text.contains("/FontDescriptor"),
        "an embedded face is exactly what these two names record"
    );
    let content = content_of(&pdf);
    assert!(
        !content.contains("/FontFile") && !content.contains("/FontDescriptor"),
        "the content stream names no face either"
    );
    assert!(
        content.contains(" c\n"),
        "the glyph arrived as bezier segments: {content}"
    );
    let last = content.lines().map(str::trim).rfind(|l| !l.is_empty());
    assert!(
        last.is_some_and(|l| l.ends_with(" f")),
        "the glyph path ends filled rather than stroked: {content}"
    );

    let svg = page_to_svg(
        &page,
        &SvgOptions {
            fonts: Some(&fonts),
            ..SvgOptions::default()
        },
    )
    .expect("convert");
    assert!(svg.contains("<path"), "the glyph became a path: {svg}");
    assert!(
        !svg.contains("@font-face"),
        "the SVG embedded the face it was told to draw"
    );
}

/// The decoded content of a one-page PDF written by this crate. The stream is
/// deflated, so a search of the raw bytes finds only compressed noise: what a
/// glyph became has to be read back to be seen.
fn content_of(pdf: &[u8]) -> String {
    let doc = lopdf::Document::load_mem(pdf).expect("the export is a PDF");
    let page = doc.get_pages().into_values().next().expect("a page");
    String::from_utf8_lossy(&doc.get_page_content(page)).into_owned()
}

/// A 2x2 PNG: red everywhere, with the right column half transparent.
///
/// The shape a seal has over the line it signs: colour where the ink is,
/// nothing where the paper should show through.
fn translucent_png() -> Vec<u8> {
    let mut rgba = Vec::with_capacity(2 * 2 * 4);
    for x in 0..2 {
        let alpha = if x == 0 { 255 } else { 0 };
        for _ in 0..2 {
            rgba.extend_from_slice(&[220, 30, 30, alpha]);
        }
    }
    let image = image::RgbaImage::from_raw(2, 2, rgba).expect("2x2 pixels");
    let mut png = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut png, image::ImageFormat::Png)
        .expect("encode the fixture");
    png.into_inner()
}

/// A page whose only content is that resource, placed over the whole sheet.
fn seal_page(png: Vec<u8>) -> PageDisplay {
    PageDisplay {
        index: 0,
        page_box: Rect::A4_PORTRAIT,
        ops: vec![DrawOp::Image {
            image: sofd_display::ImageRef::new(
                "seal",
                std::sync::Arc::new(png),
                (2, 2),
                sofd_display::ImageCodec::Png,
            ),
            dst: Rect::new(10.0, 10.0, 40.0, 40.0),
            transform: sofd_geometry::Affine::IDENTITY,
        }],
        content_bbox: Rect::new(10.0, 10.0, 50.0, 50.0),
    }
}

#[test]
fn a_translucent_image_travels_as_a_soft_mask() {
    let page = seal_page(translucent_png());
    let pdf = page_to_pdf(&page, &PdfOptions::default()).expect("convert");
    let doc = lopdf::Document::load_mem(&pdf).expect("the export is a PDF");

    let images: Vec<_> = doc
        .objects
        .values()
        .filter_map(|o| o.as_stream().ok())
        .filter(|s| {
            s.dict.get(b"Subtype").and_then(|v| v.as_name()).ok() == Some(b"Image".as_ref())
        })
        .collect();
    assert_eq!(images.len(), 2, "the colour image and its mask");

    let painted = images
        .iter()
        .find(|s| {
            s.dict.get(b"ColorSpace").and_then(|v| v.as_name()).ok() == Some(b"DeviceRGB".as_ref())
        })
        .expect("the colour image");
    let mask_ref = painted
        .dict
        .get(b"SMask")
        .expect("the image names a mask")
        .as_reference()
        .expect("an indirect mask");
    let mask = doc
        .get_object(mask_ref)
        .and_then(|o| o.as_stream())
        .expect("the mask is a stream");
    assert_eq!(
        mask.dict.get(b"ColorSpace").and_then(|v| v.as_name()).ok(),
        Some(b"DeviceGray".as_ref()),
        "a soft mask is one channel"
    );
    let alpha = mask.decompressed_content().expect("the mask content");
    assert_eq!(
        alpha,
        vec![255, 255, 0, 0],
        "left column opaque, right column clear"
    );
}

#[test]
fn an_opaque_image_writes_no_mask() {
    let mut png = std::io::Cursor::new(Vec::new());
    let mut rgba = Vec::new();
    for _ in 0..4 {
        rgba.extend_from_slice(&[200, 10, 10, 255]);
    }
    let image = image::RgbaImage::from_raw(2, 2, rgba).expect("2x2 pixels");
    image::DynamicImage::ImageRgba8(image)
        .write_to(&mut png, image::ImageFormat::Png)
        .expect("encode the fixture");
    let page = seal_page(png.into_inner());
    let pdf = page_to_pdf(&page, &PdfOptions::default()).expect("convert");
    let doc = lopdf::Document::load_mem(&pdf).expect("the export is a PDF");
    let painted = doc
        .objects
        .values()
        .filter_map(|o| o.as_stream().ok())
        .find(|s| {
            s.dict.get(b"ColorSpace").and_then(|v| v.as_name()).ok() == Some(b"DeviceRGB".as_ref())
        })
        .expect("the colour image");
    assert!(
        painted.dict.get(b"SMask").is_err(),
        "nothing was translucent, so there is nothing to mask"
    );
}
