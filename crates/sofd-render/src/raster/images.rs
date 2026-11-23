//! Decoding and painting placed images.

use super::*;

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
