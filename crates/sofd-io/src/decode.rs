//! Decoding an image resource, so that every reader of one agrees.
//! OFD packs an image in whichever format the producer chose. The parser
//! measures the bytes, the rasteriser paints them and the exporters embed them,
//! so the reading is here: a format only some of them knew would still render
//! and then refuse to export.

use sofd_common::ImageCodec;

/// Decode an image resource into the pixels a consumer asked for.
///
/// `Pixels` says which layout is wanted rather than leaving the caller to
/// reshape the bytes: a raster backend wants RGBA, an exporter embedding a PDF
/// image XObject wants RGB, and both used to spell out the expansion
/// themselves.
pub fn decode(data: &[u8], codec: ImageCodec, pixels: Pixels) -> Result<Decoded, String> {
    // JBIG2 keeps no signature `image` could guess from, so it is read here and
    // handed on as bytes rather than through the crate's format table.
    if codec == ImageCodec::Jbig2 {
        return decode_jbig2(data, pixels);
    }
    let read = read(data, codec)?;
    let (width, height) = (read.width(), read.height());
    let bytes = match pixels {
        // An RGB image is written out as it is: premultiplying is the raster
        // backend's need, and an exporter embedding the pixels has nowhere to
        // put the alpha it would be multiplying by.
        Pixels::Rgb => read.to_rgb8().into_raw(),
        Pixels::Rgba => read
            .to_rgba8()
            .pixels()
            .flat_map(|p| premultiply(p.0))
            .collect(),
    };
    Ok(Decoded {
        bytes,
        width,
        height,
    })
}

/// The pixel size of an image, read from its own header rather than from OFD's
/// `<Size>`, which a placement may scale.
pub fn pixel_size(data: &[u8], codec: ImageCodec) -> Option<(u32, u32)> {
    // JBIG2 keeps its dimensions in a segment rather than a magic number, so it
    // is asked directly instead of being left to `image` to fail on.
    if codec == ImageCodec::Jbig2 {
        return jbig2::dimensions(data);
    }
    read(data, codec)
        .ok()
        .map(|image| (image.width(), image.height()))
}

/// Which layout a decode produces.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Pixels {
    /// Three channels, no alpha: what an image XObject embedded in a PDF holds.
    Rgb,
    /// Four channels, premultiplied: what a raster backend blits.
    Rgba,
}

/// One decoded image, row-major, with the size it actually came out as.
///
/// The size travels with the bytes because a stream may declare a different one
/// from what it holds, and a caller that assumed the declared size would read
/// past the buffer.
#[derive(Clone, Debug)]
pub struct Decoded {
    /// The pixels, in the layout the call asked for.
    pub bytes: Vec<u8>,
    /// Columns.
    pub width: u32,
    /// Rows.
    pub height: u32,
}

/// Decode a bi-level page, then repeat each sample into colour channels.
///
/// A decoded page has only two values, so the three colour channels are the
/// same byte and the fourth is opaque.
fn decode_jbig2(data: &[u8], pixels: Pixels) -> Result<Decoded, String> {
    let gray = jbig2::decode_to_gray(data)?;
    let (width, height) = jbig2::dimensions(data).ok_or("JBIG2 has no dimensions")?;
    if gray.len() != width as usize * height as usize {
        return Err(format!(
            "JBIG2 declares {width}x{height} but decoded {} bytes",
            gray.len()
        ));
    }
    let channels = match pixels {
        Pixels::Rgb => 3,
        Pixels::Rgba => 4,
    };
    let mut bytes = Vec::with_capacity(gray.len() * channels);
    for &v in &gray {
        bytes.extend_from_slice(&[v, v, v]);
        if channels == 4 {
            bytes.push(u8::MAX);
        }
    }
    Ok(Decoded {
        bytes,
        width,
        height,
    })
}

/// Read a resource with the `image` crate, naming the format it declares.
fn read(data: &[u8], codec: ImageCodec) -> Result<image::DynamicImage, String> {
    match codec {
        // Bare pixels carry no header, so `image` has nothing to read them by,
        // and naming one would paint noise. No sample uses a raw resource.
        ImageCodec::Raw => Err("bare pixels carry no header to read them by".to_string()),
        // Answered by `decode` and `pixel_size` before this is reached.
        ImageCodec::Jbig2 => Err("JBIG2 has no format in `image` to be named".to_string()),
        _ => image::load_from_memory_with_format(data, format_of(codec)).map_err(|e| e.to_string()),
    }
}

/// The `image` format an OFD codec corresponds to.
///
/// A JPEG carries a format marker of its own and `image` reads it under any
/// name, which is why OFD's producers spell the same codec `jpg`, `jpeg` and
/// `JPG` without any of them failing.
fn format_of(codec: ImageCodec) -> image::ImageFormat {
    match codec {
        ImageCodec::Jpeg => image::ImageFormat::Jpeg,
        ImageCodec::Png => image::ImageFormat::Png,
        ImageCodec::Tiff => image::ImageFormat::Tiff,
        ImageCodec::Bmp => image::ImageFormat::Bmp,
        ImageCodec::Gif => image::ImageFormat::Gif,
        // Answered by the callers above, for the reason each gives.
        ImageCodec::Jbig2 | ImageCodec::Raw => image::ImageFormat::Png,
    }
}

/// Scale a colour channel by its alpha, the form a raster canvas expects.
///
/// An opaque pixel is returned untouched, which is every pixel of a JPEG and
/// most of a scan.
pub fn premultiply(px: [u8; 4]) -> [u8; 4] {
    let a = px[3] as u32;
    if a == 255 {
        return px;
    }
    let m = |c: u8| ((c as u32 * a + 127) / 255) as u8;
    [m(px[0]), m(px[1]), m(px[2]), px[3]]
}

pub mod jbig2;
