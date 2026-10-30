//! Colour resolution: a colour as the file states it, read into a paint.
//!
//! A colour may name a declared space, so its channels mean what that space
//! says they mean, and it may carry a shading instead of a flat value. Both are
//! questions about colour and not about the object tree, which is why they are
//! answered here rather than in the flattening pass.

use sofd_display::Paint;
use sofd_display::ops::{Color, GradientStop, MeshVertex};
use sofd_error::Result;
use sofd_model::color::{ColorRef, MeshPoint, Segment, from_cmyk};

use crate::flatten::ColorTable;

pub(crate) fn solid(color: &ColorRef, colors: &ColorTable) -> Result<Paint> {
    // A colour carrying a shading has no flat value: the gradient is the
    // colour. Text reaches here without going through `paint_of`, so such a
    // run read as invalid and took the document with it.
    let (r, g, b) = if color.shading.is_some() {
        (0, 0, 0)
    } else {
        resolve_rgb(color, colors)?
    };
    let alpha = color.clone().alpha();
    Ok(Paint::Solid(Color::rgba(r, g, b, alpha)))
}

/// GB/T 33190 clause 7.13: a colour that names a declared space has its
/// channels read in that space, `Index` picking a palette entry when the
/// space carries one. Anything that does not resolve falls back to sRGB.
pub(crate) fn resolve_rgb(color: &ColorRef, colors: &ColorTable) -> Result<(u8, u8, u8)> {
    let Some(space) = color.space.as_ref().and_then(|id| colors.get(id.as_str())) else {
        return color.to_rgb();
    };
    if let Some(entry) = palette_entry(space, color.index)? {
        return Ok(entry);
    }
    // An empty `Value` has no channels to read in any space, so the space
    // cannot change the answer.
    if color.value.trim().is_empty() {
        return color.to_rgb();
    }
    let channels = color.channels()?;
    Ok(convert_by_kind(space.kind, &channels))
}

/// The palette entry the index names, `Ok(None)` when the space has no
/// palette or the colour carries no index.
pub(crate) fn palette_entry(
    space: &sofd_model::ColorSpace,
    index: Option<i32>,
) -> Result<Option<(u8, u8, u8)>> {
    let Some(index) = index else {
        return Ok(None);
    };
    if space.palette.is_empty() {
        return Ok(None);
    }
    let at = usize::try_from(index)
        .ok()
        .filter(|i| *i < space.palette.len());
    let Some(entry) = at.map(|i| space.palette[i].as_str()) else {
        return Ok(None);
    };
    let samples: Vec<u8> = entry
        .split_whitespace()
        .filter_map(|t| t.parse::<f64>().ok())
        .map(|v| v.clamp(0.0, 255.0) as u8)
        .collect();
    if samples.is_empty() {
        return Ok(None);
    }
    Ok(Some(convert_by_kind(space.kind, &samples)))
}

/// Sample channels in a declared space down to sRGB.
pub(crate) fn convert_by_kind(kind: sofd_model::ColorSpaceKind, channels: &[u8]) -> (u8, u8, u8) {
    match kind {
        sofd_model::ColorSpaceKind::Gray => {
            let g = channels[0];
            (g, g, g)
        }
        sofd_model::ColorSpaceKind::Rgb => match channels {
            [r, g, b, ..] => (*r, *g, *b),
            [g] => (*g, *g, *g),
            _ => (0, 0, 0),
        },
        sofd_model::ColorSpaceKind::Cmyk => match channels {
            [c, m, y, k, ..] => (from_cmyk(*c, *k), from_cmyk(*m, *k), from_cmyk(*y, *k)),
            _ => (0, 0, 0),
        },
    }
}

/// One vertex per mesh point, its colour resolved to RGB here so the backend
/// interpolates numbers rather than parsing colour text per pixel.
pub(crate) fn mesh_vertices(points: &[MeshPoint]) -> Result<Vec<MeshVertex>> {
    points
        .iter()
        .map(|p| {
            let color = ColorRef::from_hex(p.color.clone()).with_alpha(p.alpha);
            let (r, g, b) = color.to_rgb()?;
            Ok(MeshVertex {
                x: p.point.0,
                y: p.point.1,
                color: Color::rgba(r, g, b, p.alpha),
            })
        })
        .collect()
}

/// The stops a gradient carries, each colour resolved to RGB and its alpha
/// read off the colour rather than the segment.
pub(crate) fn stops_of(segments: &[Segment]) -> Result<Vec<GradientStop>> {
    segments
        .iter()
        .map(|s| {
            let (r, g, b) = s.color.to_rgb()?;
            Ok(GradientStop {
                offset: s.position,
                color: Color::rgba(r, g, b, s.color.clone().alpha()),
            })
        })
        .collect()
}
