//! Vector rendering backend.
//! The same op stream the raster walks, walked into geometry instead of pixels:
//! one [`VectorPage`] per page, in page millimetres. A caller that draws on a
//! canvas of its own adds the page to it without decoding a bitmap first.

use std::sync::Arc;

use sofd_common::Rect;
use sofd_display::{DrawOp, GlyphRun, PageDisplay, Paint, PaintRule};
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
        shapes: Vec::new(),
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
                if let Some(path) = run_outline(fonts, run, ctm) {
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
                rule,
                ..
            } => out.shapes.push(Shape::Path {
                path: path.transformed(&ctm),
                fill: fill.clone(),
                stroke: stroke.clone(),
                rule: *rule,
            }),
            DrawOp::Text(run) => {
                if let Some(path) = run_outline(fonts, run, ctm) {
                    out.shapes.push(Shape::Text {
                        path,
                        fill: run.fill.clone(),
                        stroke: run.stroke.clone(),
                    });
                }
            }
            // An image is not geometry. A caller that needs the photographs
            // wants the raster, which decodes them; this page is the marks.
            DrawOp::Image { .. } => {}
        }
    }
    out
}

/// The outlines of a run under `ctm`, or `None` when no face resolves them.
fn run_outline(fonts: &FontManager, run: &GlyphRun, ctm: Affine) -> Option<Path> {
    let face = sofd_display::run_face(fonts, run)?;
    let mut segments = Vec::new();
    let mut offset = 0.0f64;
    for (i, &gid) in run.glyphs.iter().enumerate() {
        if i > 0 {
            offset += run.advances.get(i - 1).copied().unwrap_or(0.0);
        }
        offset += run.letter_spacing;
        let Some((outline, _source)) =
            fonts.outline_for_code_in(&face, gid, run.codes_are_glyphs, &[])
        else {
            continue;
        };
        let placed = outline.transformed(&run.glyph_matrix(offset).then(ctm));
        segments.extend(placed.segments);
    }
    if segments.is_empty() {
        return None;
    }
    // Assembled through the builder so the bounding box is computed where the
    // path itself decides what that means.
    let mut path = Path::new();
    path.segments = segments;
    Some(path)
}

/// A run's face, for a caller that wants to know which one a page used.
pub fn run_face(fonts: &FontManager, run: &GlyphRun) -> Option<Arc<sofd_font::FontFace>> {
    sofd_display::run_face(fonts, run)
}
