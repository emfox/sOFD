//! Vector rendering backend.
//! The same op stream the raster walks, walked into shapes instead of pixels:
//! one [`VectorPage`] per page, in page millimetres. A caller that draws on a
//! canvas of its own adds the page to it without decoding a bitmap first, and
//! receives an image's original bytes rather than a pixmap this crate decoded.

use std::sync::Arc;

use sofd_common::Rect;
use sofd_display::{DrawOp, GlyphRun, ImageRef, PageDisplay, Paint, PaintRule, StrokeStyle};
use sofd_font::FontManager;
use sofd_geometry::{Affine, Path};

use crate::raster::RenderOptions;

/// One page's drawing, as shapes in page millimetres.
///
/// Shapes arrive in drawing order, each already carrying the transform that was
/// in force when it was reached, so a caller draws them in the order given. A
/// [`Shape::Clip`] narrows what follows it, which is what a canvas stack does
/// for itself, and [`Shape::ClipReset`] says when to put the clip back.
#[derive(Debug, Clone, Default)]
pub struct VectorPage {
    /// The part of the page this covers, in millimetres.
    pub page_box: Rect,
    /// Shapes in drawing order.
    pub shapes: Vec<Shape>,
    /// Runs that lost at least one glyph, so the page shows fewer marks than it
    /// should. The same count [`crate::RasterOutput`] carries, so a caller can
    /// say what is missing whichever backend drew the page.
    pub missing_glyph_runs: usize,
    /// Glyph positions no face in the chain could draw.
    pub missing_glyphs: usize,
    /// Runs left out because no face could be loaded at all.
    pub skipped_font_runs: usize,
}

/// One thing a vector page draws.
#[derive(Debug, Clone)]
pub enum Shape {
    /// Fill or stroke a path.
    Path {
        /// Geometry, already in page millimetres.
        path: Path,
        /// Fill paint; `None` leaves the interior untouched.
        fill: Option<Paint>,
        /// Stroke paint; `None` leaves the outline untouched.
        stroke: Option<Paint>,
        /// Stroke geometry: width in millimetres, dashes, cap and join. A
        /// backend that took the paint without this would draw every outline at
        /// one width, which is a table rule and a seal at once.
        stroke_style: StrokeStyle,
        /// How the path decides what is inside itself.
        rule: PaintRule,
    },
    /// The outlines of a text run, with the paint the run carries.
    Text {
        /// Outlines, already in page millimetres.
        path: Path,
        /// Fill paint.
        fill: Option<Paint>,
        /// Stroke paint.
        stroke: Option<Paint>,
    },
    /// A bitmap to place, handed over undecoded.
    /// The bytes are the package's own, in the encoding [`ImageRef::codec`]
    /// names, so a browser can make an `ImageBitmap` of them and a toolkit can
    /// hand them to its decoder. Decoding here would pick this crate's decoder
    /// for every caller, and re-encode what a JPEG already was.
    Image {
        /// The resource, with its encoded bytes and intrinsic pixel size.
        image: ImageRef,
        /// Pixel space to page millimetres: the image's own `(0, 0)` and
        /// `(width, height)` corners map through this onto the page. A single
        /// matrix rather than a rectangle, because a placement can be rotated
        /// or sheared and a canvas takes both the same way.
        to_page: Affine,
    },
    /// Intersect the clip with a path. It governs the shapes after it.
    Clip {
        /// Clip geometry, already in page millimetres.
        path: Path,
        /// How the path decides what is inside itself.
        rule: PaintRule,
    },
    /// Put a clip back the way it was.
    ClipReset,
}

/// What one run gave the walk.
struct RunDraw {
    /// Outlines in page millimetres, absent when nothing could be drawn.
    path: Option<Path>,
    /// Glyph positions that resolved to no outline.
    missing: usize,
    /// Whether a face was found at all.
    had_face: bool,
}

/// Walk a page into geometry.
///
/// `fonts` resolves glyph outlines, so a document whose faces are embedded is
/// drawn with its own letterforms rather than whatever the caller has
/// installed. A run this build cannot resolve is left out rather than replaced,
/// which keeps an absent glyph absent instead of drawing the wrong one.
pub fn page_to_vector(
    page: &PageDisplay,
    fonts: &FontManager,
    options: &RenderOptions,
) -> VectorPage {
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
    let mut out = VectorPage {
        page_box: view,
        ..VectorPage::default()
    };
    // The clip depth each pushed state was saved at, so a restore knows whether
    // it is putting a clip back or merely handing a transform back.
    let mut ctm = Affine::IDENTITY;
    let mut stack: Vec<(Affine, usize)> = Vec::new();
    let mut depth = 0usize;
    for op in &page.ops {
        match op {
            DrawOp::Save => stack.push((ctm, depth)),
            // A restore with nothing saved leaves the transform as it was; the
            // raster reports it, and a caller drawing shapes has no state to
            // hand back either.
            DrawOp::Restore => {
                if let Some((previous, saved)) = stack.pop() {
                    ctm = previous;
                    if depth > saved {
                        out.shapes.push(Shape::ClipReset);
                        depth = saved;
                    }
                }
            }
            DrawOp::Transform(t) => ctm = ctm.then(*t),
            DrawOp::ClipPath { path, rule } => {
                out.shapes.push(Shape::Clip {
                    path: path.transformed(&ctm),
                    rule: *rule,
                });
                depth += 1;
            }
            DrawOp::ClipText(run) => {
                // A clip this build cannot resolve clips nothing, which is what
                // the raster does with the same run rather than clipping the
                // page away.
                if let Some(path) = run_outline(fonts, run, ctm).path {
                    out.shapes.push(Shape::Clip {
                        path,
                        rule: PaintRule::NonZero,
                    });
                    depth += 1;
                }
            }
            DrawOp::Path {
                path,
                fill,
                stroke,
                stroke_style,
                rule,
            } => out.shapes.push(Shape::Path {
                path: path.transformed(&ctm),
                fill: fill.clone(),
                stroke: stroke.clone(),
                stroke_style: stroke_style.clone(),
                rule: *rule,
            }),
            DrawOp::Text(run) => {
                let drawn = run_outline(fonts, run, ctm);
                if !drawn.had_face {
                    out.skipped_font_runs += 1;
                    out.missing_glyphs += run.len();
                } else if drawn.missing > 0 {
                    out.missing_glyph_runs += 1;
                    out.missing_glyphs += drawn.missing;
                }
                if let Some(path) = drawn.path {
                    out.shapes.push(Shape::Text {
                        path,
                        fill: run.fill.clone(),
                        stroke: run.stroke.clone(),
                    });
                }
            }
            DrawOp::Image { image, dst, .. } => out.shapes.push(Shape::Image {
                image: image.clone(),
                to_page: image_matrix(image, *dst, ctm),
            }),
        }
    }
    out
}

/// Where an image's own pixels land on the page.
///
/// `dst` is in the object's content space — for an `ImageObject`, the unit
/// square the placement's CTM scales onto the boundary — so `dst` and `ctm`
/// together put the picture somewhere. The op's own `transform` is left out:
/// the placement already folded it in, and applying it again would double it.
fn image_matrix(image: &ImageRef, dst: Rect, ctm: Affine) -> Affine {
    let (pw, ph) = image.pixel_size;
    if pw == 0 || ph == 0 {
        // A resource that states no size falls back to the placement box, which
        // is what a caller has to work with anyway.
        return Affine::new(dst.w, 0.0, 0.0, dst.h, dst.x, dst.y).then(ctm);
    }
    Affine::new(
        dst.w / f64::from(pw),
        0.0,
        0.0,
        dst.h / f64::from(ph),
        dst.x,
        dst.y,
    )
    .then(ctm)
}

/// The outlines of a run under `ctm`, and what it cost to get them.
fn run_outline(fonts: &FontManager, run: &GlyphRun, ctm: Affine) -> RunDraw {
    let Some(face) = sofd_display::run_face(fonts, run) else {
        return RunDraw {
            path: None,
            missing: 0,
            had_face: false,
        };
    };
    let mut segments = Vec::new();
    let mut missing = 0usize;
    let mut offset = 0.0f64;
    for (i, &gid) in run.glyphs.iter().enumerate() {
        if i > 0 {
            offset += run.advances.get(i - 1).copied().unwrap_or(0.0);
        }
        offset += run.letter_spacing;
        let Some((outline, _source)) =
            fonts.outline_for_code_in(&face, gid, run.codes_are_glyphs, &[])
        else {
            if fonts.is_missing_code(&face, gid, run.codes_are_glyphs) {
                missing += 1;
            }
            continue;
        };
        let placed = outline.transformed(&run.glyph_matrix(offset).then(ctm));
        segments.extend(placed.segments);
    }
    if segments.is_empty() {
        return RunDraw {
            path: None,
            missing,
            had_face: true,
        };
    }
    // Assembled through the builder so the bounding box is computed where the
    // path itself decides what that means.
    let mut path = Path::new();
    path.segments = segments;
    RunDraw {
        path: Some(path),
        missing,
        had_face: true,
    }
}

/// A run's face, for a caller that wants to know which one a page used.
pub fn run_face(fonts: &FontManager, run: &GlyphRun) -> Option<Arc<sofd_font::FontFace>> {
    sofd_display::run_face(fonts, run)
}
