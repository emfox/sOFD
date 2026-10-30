//! PNG backend.
//! Rasterization belongs to the render crate; this only encodes its output,
//! so the two never disagree about what a page looks like.

use sofd_display::PageDisplay;
use sofd_error::{OfdError, RenderError, Result};
use sofd_font::FontManager;
use sofd_render::{RasterOutput, RenderOptions, render_page};

/// Rasterize one page and encode it as PNG bytes.
pub fn page_to_png(
    page: &PageDisplay,
    fonts: &FontManager,
    options: &RenderOptions,
) -> Result<Vec<u8>> {
    let raster = render_page(page, fonts, options)?;
    encode_png(&raster, page.index)
}

/// Encode a finished rasterization as PNG bytes.
pub fn encode_png(raster: &RasterOutput, page: usize) -> Result<Vec<u8>> {
    let image = image::RgbaImage::from_raw(raster.width, raster.height, raster.rgba_bytes())
        .ok_or_else(|| {
            OfdError::Render(RenderError::ImageEncode {
                page,
                reason: format!(
                    "{}x{} does not match the raster byte count",
                    raster.width, raster.height
                ),
            })
        })?;
    let mut out = std::io::Cursor::new(Vec::new());
    image
        .write_to(&mut out, image::ImageFormat::Png)
        .map_err(|e| {
            OfdError::Render(RenderError::ImageEncode {
                page,
                reason: e.to_string(),
            })
        })?;
    Ok(out.into_inner())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sofd_common::{Millimeter, Rect};
    use sofd_display::ops::{Color, Paint, PaintRule, StrokeStyle};
    use sofd_display::{DrawOp, GlyphRun};
    use sofd_font::FontId;
    use sofd_geometry::{Affine, Path};

    fn page(ops: Vec<DrawOp>) -> PageDisplay {
        PageDisplay {
            index: 3,
            page_box: Rect::new(0.0, 0.0, 50.0, 50.0),
            ops,
            content_bbox: Rect::EMPTY,
        }
    }

    fn filled_rect(x: f64, y: f64, w: f64, h: f64) -> DrawOp {
        let path = Path::build(|b| {
            b.rect(x, y, w, h);
        });
        DrawOp::Path {
            path,
            fill: Some(Paint::Solid(Color::BLACK)),
            stroke: None,
            stroke_style: StrokeStyle::default(),
            rule: PaintRule::NonZero,
        }
    }

    #[test]
    fn a_blank_page_still_encodes_a_valid_png() {
        let fonts = FontManager::new();
        let png = page_to_png(&page(Vec::new()), &fonts, &RenderOptions::default())
            .expect("encode a blank page");
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n", "PNG magic");
    }

    #[test]
    fn the_encoded_png_decodes_back_to_the_raster_size() {
        let fonts = FontManager::new();
        let p = page(vec![filled_rect(5.0, 5.0, 20.0, 20.0)]);
        let png = page_to_png(&p, &fonts, &RenderOptions::default()).expect("encode");
        let decoded = image::load_from_memory(&png).expect("decode the png we just wrote");
        // 50mm at 96 DPI is 188.98 pixels, rounded up by `size_px`.
        assert_eq!(decoded.width(), 189);
        assert_eq!(decoded.height(), 189);
    }

    #[test]
    fn a_drawn_rect_paints_dark_pixels() {
        let fonts = FontManager::new();
        let p = page(vec![filled_rect(10.0, 10.0, 30.0, 30.0)]);
        let png = page_to_png(&p, &fonts, &RenderOptions::default()).expect("encode");
        let decoded = image::load_from_memory(&png).expect("decode").to_rgba8();
        let mid = decoded.get_pixel(decoded.width() / 2, decoded.height() / 2);
        assert!(mid[0] < 128, "the middle of the rect is dark, got {mid:?}");
    }

    #[test]
    fn a_budget_exceeded_error_propagates() {
        let fonts = FontManager::new();
        let options = RenderOptions {
            max_pixels: 16,
            ..RenderOptions::default()
        };
        let err = page_to_png(
            &page(vec![filled_rect(0.0, 0.0, 10.0, 10.0)]),
            &fonts,
            &options,
        )
        .expect_err("a 188x188 page exceeds sixteen pixels");
        assert!(matches!(
            err,
            OfdError::Render(RenderError::BudgetExceeded(_))
        ));
    }

    #[test]
    fn an_empty_page_and_a_full_page_differ() {
        let fonts = FontManager::new();
        let options = RenderOptions::default();
        let blank = page_to_png(&page(Vec::new()), &fonts, &options).expect("blank");
        let full = page_to_png(
            &page(vec![filled_rect(0.0, 0.0, 50.0, 50.0)]),
            &fonts,
            &options,
        )
        .expect("full");
        assert_ne!(blank, full);
    }

    #[test]
    fn a_text_run_referencing_an_unknown_font_costs_only_its_own_run() {
        // A font id that was never registered used to abort the whole page.
        // The run is now skipped and counted instead, so the rest of the page
        // still renders and the caller can see what was lost.
        let fonts = FontManager::new();
        let run = GlyphRun {
            font: FontId(42),
            size: Millimeter(10.0),
            glyphs: vec![1],
            advances: vec![10.0],
            text: Some("A".into()),
            cluster_map: None,
            codes_are_glyphs: false,
            transform: Affine::translate(10.0, 10.0),
            glyph_angle: 0.0,
            fill: Some(Paint::Solid(Color::BLACK)),
            stroke: None,
            stroke_style: Default::default(),
            letter_spacing: 0.0,
            bbox: Rect::new(10.0, 10.0, 10.0, 10.0),
        };
        let png = page_to_png(
            &page(vec![DrawOp::Text(run)]),
            &fonts,
            &RenderOptions::default(),
        )
        .expect("the page still renders");
        assert!(!png.is_empty());
    }
}
