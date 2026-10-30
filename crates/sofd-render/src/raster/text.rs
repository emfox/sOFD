//! Painting text runs and images.

use super::*;

/// What a text run contributed to the page.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TextOutcome {
    /// Every glyph the run named was drawn.
    Drawn,
    /// The run drew, but some of its glyphs had no outline in any face. The
    /// count is carried rather than a flag: one missing ideograph in a heading
    /// and a run of them are the same boolean and not the same problem.
    ShortGlyphs {
        /// Glyph positions that came out empty.
        glyphs: usize,
    },
    /// The run's face could not be loaded, so the run was skipped.
    NoFace,
}

pub(crate) fn draw_text(
    pixmap: &mut Pixmap,
    run: &GlyphRun,
    state: &State,
    view: Rect,
    fonts: &FontManager,
    options: &RenderOptions,
) -> Result<TextOutcome> {
    if run.glyphs.is_empty() {
        return Ok(TextOutcome::Drawn);
    }
    // A face that will not load costs its run, not the page, and is counted in
    // `skipped_font_runs`. The chain gets one more chance, told what the run
    // is about to draw so the substitute can cover those characters.
    let wanted = run_wanted(run);
    let Some(face) = sofd_display::run_face_for(fonts, run, &wanted) else {
        return Ok(TextOutcome::NoFace);
    };
    draw_text_inner(pixmap, run, state, view, fonts, face, options)
}

/// The code points a run will draw, or nothing when its codes are glyph
/// indices. `text` is no help there: it serves search and copy, not drawing.
pub(crate) fn run_wanted(run: &GlyphRun) -> Vec<u32> {
    if run.codes_are_glyphs {
        return Vec::new();
    }
    run.glyphs.iter().map(|&c| u32::from(c)).collect()
}

pub(crate) fn draw_text_inner(
    pixmap: &mut Pixmap,
    run: &GlyphRun,
    state: &State,
    view: Rect,
    fonts: &FontManager,
    face: Arc<sofd_font::FontFace>,
    options: &RenderOptions,
) -> Result<TextOutcome> {
    let mut offset = 0.0f64;
    let mut missing = 0usize;
    let wanted = run_wanted(run);
    for (i, &gid) in run.glyphs.iter().enumerate() {
        if i > 0 {
            offset += run.advances.get(i - 1).copied().unwrap_or(0.0);
        }
        offset += run.letter_spacing;
        let Some((outline, _source)) =
            fonts.outline_for_code_in(&face, gid, run.codes_are_glyphs, &wanted)
        else {
            if fonts.is_missing_code(&face, gid, run.codes_are_glyphs) {
                missing += 1;
                tracing::debug!(
                    code = gid,
                    glyphs = run.codes_are_glyphs,
                    "a run code had no outline in its face or any fallback"
                );
            }
            continue;
        };
        // Outlines are normalized to 1/1000 em with y pointing up, and the
        // run's own matrix scales, turns and places them from there.
        let glyph = run.glyph_matrix(offset);
        let total = total_transform(state, &view, options.dpi);
        let ts_path = to_ts_path(&outline, total.pre_concat(to_ts(glyph)))?;
        if let Some(paint) = &run.fill {
            let p = solid_paint(paint);
            pixmap.fill_path(&ts_path, &p, FillRule::Winding, Transform::identity(), None);
        }
    }
    Ok(if missing > 0 {
        TextOutcome::ShortGlyphs { glyphs: missing }
    } else {
        TextOutcome::Drawn
    })
}

/// What became of one placed image.
pub(crate) enum ImageOutcome {
    /// The image was decoded and painted.
    Drawn,
    /// The image was skipped: its bytes would not decode or its size was
    /// rejected. The page keeps going, so a single bad resource cannot take
    /// the rest of the page with it.
    Failed,
}

pub(crate) fn draw_image(
    pixmap: &mut Pixmap,
    image: &ImageRef,
    dst: Rect,
    state: &State,
    view: Rect,
    options: &RenderOptions,
) -> Result<ImageOutcome> {
    if image.data.is_empty() {
        // A missing resource must not abort the page; leave the area empty.
        return Ok(ImageOutcome::Failed);
    }
    let (pixels, iw, ih) = match decode_resource(image) {
        Ok(decoded) => decoded,
        Err(e) => {
            tracing::warn!(id = %image.id, error = %e, "skipping an image that would not decode");
            return Ok(ImageOutcome::Failed);
        }
    };
    let Some(size) = IntSize::from_wh(iw, ih) else {
        tracing::warn!(id = %image.id, w = iw, h = ih, "skipping an image with a rejected size");
        return Ok(ImageOutcome::Failed);
    };
    let Some(img) = Pixmap::from_vec(pixels, size) else {
        tracing::warn!(id = %image.id, w = iw, h = ih, "skipping an image whose pixels were rejected");
        return Ok(ImageOutcome::Failed);
    };

    // The pixels fill the destination rect and nothing else: the placement
    // CTM is already in the graphics state. `post_concat` applies the rect
    // before the millimetre-to-pixel step, which is the order offsets need.
    let pixel_rect = Affine::new(dst.w / iw as f64, 0.0, 0.0, dst.h / ih as f64, dst.x, dst.y);
    let total = to_ts(pixel_rect).post_concat(total_transform(state, &view, options.dpi));
    pixmap.draw_pixmap(
        0,
        0,
        img.as_ref(),
        &PixmapPaint::default(),
        total,
        state.clip_ref(),
    );
    Ok(ImageOutcome::Drawn)
}

/// Decode a placed resource into premultiplied RGBA.
///
/// `sofd_io` decodes straight alpha; tiny-skia's `Pixmap` takes premultiplied
/// pixels, so the premultiplication happens here, at that boundary. Which
/// encodings this accepts is `sofd_io`'s to say, so a format known here and
/// not there would render and then refuse to leave the page.
pub(crate) fn decode_resource(image: &ImageRef) -> Result<(Vec<u8>, u32, u32)> {
    let decoded =
        sofd_io::decode(&image.data, image.codec, sofd_io::Pixels::Rgba).map_err(|reason| {
            OfdError::Render(RenderError::ImageDecode {
                page: 0,
                reason: format!("resource {}: {reason}", image.id),
            })
        })?;
    if decoded.width == 0 || decoded.height == 0 {
        return Err(OfdError::Render(RenderError::ImageDecode {
            page: 0,
            reason: format!(
                "resource {}: size {}x{} rejected",
                image.id, decoded.width, decoded.height
            ),
        }));
    }
    let bytes = decoded
        .bytes
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|&px| sofd_io::premultiply(px))
        .collect();
    Ok((bytes, decoded.width, decoded.height))
}
