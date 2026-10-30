//! Gradients plus the content-stream helpers the sink writes with.

use super::*;

pub(crate) fn channel_array(c: &Color) -> Object {
    Object::Array(
        channel_triplet(c)
            .into_iter()
            .map(|channel| Object::Real(channel as f32))
            .collect(),
    )
}

/// An image XObject and, when the resource carries transparency, its mask.
pub(crate) struct Painted {
    /// The colour image.
    pub(crate) image: Stream,
    /// The alpha channel as a `DeviceGray` image, referenced from
    /// `/SMask`. Present only when some pixel is not fully opaque.
    pub(crate) mask: Option<Stream>,
}

/// Build an image XObject from encoded bytes, keeping its transparency.
///
/// A seal is drawn *over* the heading it signs, so dropping the alpha paints a
/// white card across the text. PDF has no fourth image channel, so the alpha
/// travels as an `/SMask`, the soft mask multiplying the colour by it. The
/// decode is `sofd_io`'s, the same one the rasteriser asks for.
pub(crate) fn image_xobject(image: &PendingImage) -> Result<Painted> {
    let decoded = sofd_io::decode(&image.data, image.codec, sofd_io::Pixels::Rgba)
        .map_err(|reason| convert_error("image", reason))?;
    let (rgb, alpha) = split_alpha(&decoded.bytes);
    let mut stream = Stream::new(image_dict(image, b"DeviceRGB"), rgb);
    stream
        .compress()
        .map_err(|e| convert_error("image stream", e.to_string()))?;
    let mask = match alpha {
        None => None,
        Some(gray) => {
            let mut stream = Stream::new(image_dict(image, b"DeviceGray"), gray);
            stream
                .compress()
                .map_err(|e| convert_error("image mask", e.to_string()))?;
            Some(stream)
        }
    };
    Ok(Painted {
        image: stream,
        mask,
    })
}

/// The dictionary both halves of an image share: same box, one channel each.
fn image_dict(image: &PendingImage, colorspace: &[u8]) -> Dictionary {
    let mut dict = Dictionary::new();
    dict.set("Type", Object::Name(b"XObject".to_vec()));
    dict.set("Subtype", Object::Name(b"Image".to_vec()));
    dict.set("Width", image.pixels.0 as i64);
    dict.set("Height", image.pixels.1 as i64);
    dict.set("ColorSpace", Object::Name(colorspace.to_vec()));
    dict.set("BitsPerComponent", Object::Integer(8));
    dict
}

/// Split premultiplied RGBA into colour and an alpha plane.
///
/// The colour is un-premultiplied, because PDF composites the mask over the
/// page itself and would otherwise multiply the alpha in twice. A fully opaque
/// image yields no plane at all, so the common case writes no mask.
fn split_alpha(rgba: &[u8]) -> (Vec<u8>, Option<Vec<u8>>) {
    let opaque = rgba.chunks_exact(4).all(|px| px[3] == u8::MAX);
    if opaque {
        return (
            rgba.chunks_exact(4)
                .flat_map(|px| &px[..3])
                .copied()
                .collect(),
            None,
        );
    }
    let mut rgb = Vec::with_capacity(rgba.len() / 4 * 3);
    let mut alpha = Vec::with_capacity(rgba.len() / 4);
    for px in rgba.chunks_exact(4) {
        let a = px[3] as u32;
        let un = |c: u8| {
            if a == 0 || a == 255 {
                c
            } else {
                ((c as u32 * 255 + a / 2) / a).min(255) as u8
            }
        };
        rgb.extend_from_slice(&[un(px[0]), un(px[1]), un(px[2])]);
        alpha.push(px[3]);
    }
    (rgb, Some(alpha))
}

/// `r g b` operands for a colour, each channel `0..=1`.
pub(crate) fn rgb(c: &Color) -> String {
    format!(
        "{} {} {}",
        num(c.r as f64 / 255.0),
        num(c.g as f64 / 255.0),
        num(c.b as f64 / 255.0)
    )
}

/// `r g b` for the first fill in a pattern's cell, used where no tiling pattern
/// can be written. An empty cell is left black.
pub(crate) fn pattern_rgb(spec: &sofd_display::PatternSpec) -> String {
    for op in &spec.cell.ops {
        if let sofd_display::DrawOp::Path {
            fill: Some(sofd_display::Paint::Solid(c)),
            ..
        } = op
        {
            return rgb(c);
        }
    }
    "0 0 0".to_string()
}

/// Channel triplet of a colour, for gradient function dictionaries.
pub(crate) fn channel_triplet(c: &Color) -> [f64; 3] {
    [c.r as f64 / 255.0, c.g as f64 / 255.0, c.b as f64 / 255.0]
}

/// Stroke parameters, scaled by the transform in effect.
pub(crate) fn stroke_prefix(style: &StrokeStyle, ctm: &Affine) -> String {
    let s = PT_PER_MM * ctm.scale_factor();
    let mut out = format!("{} w", num(style.width.max(0.0) * s));
    if !style.dash.is_empty() {
        let pattern: Vec<String> = style.dash.iter().map(|d| num(*d * s)).collect();
        let _ = write!(
            out,
            " [{}] {} d",
            pattern.join(" "),
            num(style.dash_offset * s)
        );
    }
    let cap = match style.cap {
        LineCap::Butt => 0,
        LineCap::Round => 1,
        LineCap::Square => 2,
    };
    let join = match style.join {
        LineJoin::Miter => 0,
        LineJoin::Round => 1,
        LineJoin::Bevel => 2,
    };
    let _ = write!(out, " {cap} J {join} j {} M", num(style.miter_limit));
    out
}

/// One point operand followed by an operator.
pub(crate) fn point_op(point: (f64, f64), operator: &str) -> String {
    format!("{} {} {operator}", num(point.0), num(point.1))
}

/// A cubic Bezier operator with all three points mapped.
pub(crate) fn curve_op(total: Affine, c1: (f64, f64), c2: (f64, f64), to: (f64, f64)) -> String {
    let (c1x, c1y) = total.apply(c1.0, c1.1);
    let (c2x, c2y) = total.apply(c2.0, c2.1);
    let (x, y) = total.apply(to.0, to.1);
    format!(
        "{} {} {} {} {} {} c",
        num(c1x),
        num(c1y),
        num(c2x),
        num(c2y),
        num(x),
        num(y)
    )
}

/// Escape a string for a PDF literal, keeping raw bytes intact.
pub(crate) fn pdf_string(text: &str) -> String {
    let mut out = Vec::with_capacity(text.len() + 2);
    for &byte in text.as_bytes() {
        match byte {
            b'(' | b')' | b'\\' => {
                out.push(b'\\');
                out.push(byte);
            }
            b'\r' => out.extend_from_slice(b"\\r"),
            b'\n' => out.extend_from_slice(b"\\n"),
            _ => out.push(byte),
        }
    }
    // Escaping only inserts ASCII, so the result stays valid UTF-8.
    String::from_utf8(out).unwrap_or_default()
}

/// A conversion failure tied to one part of the document.
pub(crate) fn convert_error(object: &str, reason: String) -> OfdError {
    OfdError::Convert(ConvertError::MappingFailed {
        page: 0,
        object: object.to_string(),
        reason,
    })
}

/// Compact number: at most four decimals, no trailing zeros, no negative zero.
pub(crate) fn num(v: f64) -> String {
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
