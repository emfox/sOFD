//! Backend-agnostic replay of a page's op stream.
//! The display list keeps object-local geometry plus explicit transform ops,
//! so every backend replays one state machine instead of trusting coordinates.

use sofd_common::Rect;
use sofd_display::ops::{Paint, PaintRule, StrokeStyle};
use sofd_display::{DrawOp, GlyphRun, ImageRef, PageDisplay};
use sofd_error::{FormatError, OfdError, Result};
use sofd_geometry::{Affine, Path};

/// The paint for one fill or stroke, resolved by the backend.
/// A gradient is stated on the object itself, so a sink can resolve it from
/// the shape alone instead of having to remember an earlier op.
#[derive(Clone, Copy)]
pub enum PaintShape<'a> {
    /// A flat colour.
    Solid(&'a sofd_display::ops::Color),
    /// A linear shading to be resolved against the path it fills.
    Linear(&'a sofd_display::LinearGradientSpec),
    /// A radial shading: the cone between its two circles.
    Radial(&'a sofd_display::RadialGradientSpec),
    /// A Gouraud mesh: coloured vertices and the triangles over them.
    Mesh(&'a sofd_display::MeshSpec),
    /// A pattern: a cell of vector content repeated over the area.
    Pattern(&'a sofd_display::PatternSpec),
}

/// Split a paint into the shape a sink resolves it from.
pub fn paint_shape(paint: &Paint) -> PaintShape<'_> {
    match paint {
        Paint::Solid(c) => PaintShape::Solid(c),
        Paint::LinearGradient(spec) => PaintShape::Linear(spec),
        Paint::RadialGradient(spec) => PaintShape::Radial(spec),
        Paint::Mesh(spec) => PaintShape::Mesh(spec),
        Paint::Pattern(spec) => PaintShape::Pattern(spec),
    }
}

/// A glyph run carries its paint directly, but a sink resolves the same shape,
/// so both routes go through one function.
pub fn text_paint_shape(paint: &Paint) -> PaintShape<'_> {
    paint_shape(paint)
}

/// A path with its paint, as the op stream describes it.
pub struct PathShape<'a> {
    /// Geometry in current-state coordinates.
    pub path: &'a Path,
    /// Fill paint; `None` leaves the interior untouched.
    pub fill: Option<PaintShape<'a>>,
    /// Stroke paint; `None` leaves the outline untouched.
    pub stroke: Option<PaintShape<'a>>,
    /// Stroke geometry parameters.
    pub stroke_style: &'a StrokeStyle,
    /// Rule deciding which regions the path covers.
    pub rule: PaintRule,
}

/// Backend hooks for one page of a display list.
/// Coordinates arrive in millimetres in the coordinate system of the current
/// transform; a sink tracks that transform from the calls it receives.
pub trait PageSink {
    /// Push the graphics state.
    fn save(&mut self);

    /// Pop the graphics state.
    fn restore(&mut self);

    /// Concatenate a transform onto the current state.
    fn transform(&mut self, matrix: Affine);

    /// Intersect the clip with a path.
    fn clip(&mut self, path: &Path, rule: PaintRule);

    /// Intersect the clip with the outlines of a glyph run, clause 8.4. The
    /// default draws nothing extra: a backend without fonts leaves the clip
    /// as it found it rather than clearing the page.
    fn clip_text(&mut self, _run: &GlyphRun) {}

    /// Fill and/or stroke a path.
    fn path(&mut self, shape: PathShape<'_>);

    /// Draw a glyph run.
    fn text(&mut self, run: &GlyphRun);

    /// Draw an image.
    fn image(&mut self, image: &ImageRef, dst: Rect);
}

/// Replay one page into `sink`, rejecting an unbalanced restore.
pub fn replay(page: &PageDisplay, sink: &mut impl PageSink) -> Result<()> {
    let mut depth = 0usize;
    for op in &page.ops {
        match op {
            DrawOp::Save => {
                depth += 1;
                sink.save();
            }
            DrawOp::Restore => {
                if depth == 0 {
                    return Err(OfdError::Format(FormatError::CircularReference {
                        chain: "restore without a matching save".into(),
                    }));
                }
                depth -= 1;
                sink.restore();
            }
            DrawOp::Transform(matrix) => sink.transform(*matrix),
            DrawOp::ClipPath { path, rule } => sink.clip(path, *rule),
            DrawOp::ClipText(run) => sink.clip_text(run),
            DrawOp::Path {
                path,
                fill,
                stroke,
                stroke_style,
                rule,
            } => sink.path(PathShape {
                path,
                fill: fill.as_ref().map(paint_shape),
                stroke: stroke.as_ref().map(paint_shape),
                stroke_style,
                rule: *rule,
            }),
            DrawOp::Text(run) => sink.text(run),
            DrawOp::Image { image, dst } => sink.image(image, *dst),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use sofd_common::{Millimeter, Point};
    use sofd_display::ImageCodec;
    use sofd_display::ops::{Color, Paint};
    use sofd_display::{LinearGradientSpec, RadialGradientSpec};
    use sofd_font::FontId;
    use sofd_geometry::PathBuilder;

    /// Records the hook sequence so the replay contract can be asserted.
    #[derive(Default)]
    struct Recorder {
        log: Vec<String>,
        depth: usize,
    }

    impl PageSink for Recorder {
        fn save(&mut self) {
            self.depth += 1;
            self.log.push("save".into());
        }

        fn restore(&mut self) {
            self.depth -= 1;
            self.log.push("restore".into());
        }

        fn transform(&mut self, _: Affine) {
            self.log.push("transform".into());
        }

        fn clip(&mut self, _: &Path, _: PaintRule) {
            self.log.push("clip".into());
        }

        fn path(&mut self, _: PathShape<'_>) {
            self.log.push("path".into());
        }

        fn text(&mut self, _: &GlyphRun) {
            self.log.push("text".into());
        }

        fn image(&mut self, _: &ImageRef, _: Rect) {
            self.log.push("image".into());
        }
    }

    fn run(font: FontId) -> GlyphRun {
        GlyphRun {
            font,
            size: Millimeter(3.0),
            glyphs: vec![1],
            advances: vec![3.0],
            text: Some("A".into()),
            cluster_map: None,
            codes_are_glyphs: false,
            transform: Affine::translate(1.0, 2.0),
            glyph_angle: 0.0,
            fill: None,
            stroke: None,
            stroke_style: Default::default(),
            letter_spacing: 0.0,
            word_spacing: 0.0,
            bbox: Rect::new(0.0, 0.0, 3.0, 3.0),
        }
    }

    fn page(ops: Vec<DrawOp>) -> PageDisplay {
        PageDisplay {
            index: 0,
            page_box: Rect::A4_PORTRAIT,
            ops,
            content_bbox: Rect::EMPTY,
        }
    }

    #[test]
    fn every_op_reaches_the_sink_in_order() {
        let rect = Path::build(|b| {
            b.rect(0.0, 0.0, 10.0, 10.0);
        });
        let p = page(vec![
            DrawOp::Save,
            DrawOp::Transform(Affine::scale(2.0, 2.0)),
            DrawOp::ClipPath {
                path: rect.clone(),
                rule: PaintRule::EvenOdd,
            },
            DrawOp::Path {
                path: rect.clone(),
                fill: None,
                stroke: None,
                stroke_style: Default::default(),
                rule: PaintRule::NonZero,
            },
            DrawOp::Text(run(FontId(1))),
            DrawOp::Image {
                image: ImageRef::new(
                    "i",
                    std::sync::Arc::new(Vec::new()),
                    (1, 1),
                    ImageCodec::Png,
                ),
                dst: Rect::new(0.0, 0.0, 1.0, 1.0),
            },
            DrawOp::Path {
                path: rect.clone(),
                fill: Some(Paint::LinearGradient(Box::new(LinearGradientSpec {
                    start: Point::new(0.0, 0.0),
                    end: Point::new(1.0, 0.0),
                    stops: Vec::new(),
                }))),
                stroke: Some(Paint::RadialGradient(Box::new(RadialGradientSpec {
                    center: Point::new(0.0, 0.0),
                    radius: 5.0,
                    end: Point::new(0.0, 0.0),
                    end_radius: 9.0,
                    stops: Vec::new(),
                }))),
                stroke_style: Default::default(),
                rule: PaintRule::NonZero,
            },
            DrawOp::Restore,
        ]);

        let mut sink = Recorder::default();
        replay(&p, &mut sink).expect("a balanced stream replays");
        assert_eq!(
            sink.log,
            vec![
                "save",
                "transform",
                "clip",
                "path",
                "text",
                "image",
                "path",
                "restore"
            ]
        );
        assert_eq!(sink.depth, 0);
    }

    #[test]
    fn a_restore_without_a_save_is_refused() {
        let mut sink = Recorder::default();
        let err = replay(&page(vec![DrawOp::Restore]), &mut sink)
            .expect_err("an unbalanced restore must fail");
        assert!(matches!(
            err,
            OfdError::Format(FormatError::CircularReference { .. })
        ));
        assert!(sink.log.is_empty(), "the refused op is not forwarded");
    }

    #[test]
    fn a_trailing_save_is_left_to_the_sink() {
        // Replay only guards balance it can see; closing an unclosed save is
        // the sink's job, since only it knows what an open group means.
        let mut sink = Recorder::default();
        replay(&page(vec![DrawOp::Save, DrawOp::Save]), &mut sink).expect("saves always replay");
        assert_eq!(sink.depth, 2);
    }

    #[test]
    fn shapes_borrow_the_stream_rather_than_clone() {
        let rect = Path::build(|b| {
            b.rect(0.0, 0.0, 1.0, 1.0);
        });
        let p = page(vec![DrawOp::Path {
            path: rect.clone(),
            fill: None,
            stroke: None,
            stroke_style: Default::default(),
            rule: PaintRule::NonZero,
        }]);

        let mut sink = Recorder::default();
        replay(&p, &mut sink).expect("replay");
        assert_eq!(sink.log, vec!["path"]);
    }

    #[test]
    fn a_nested_restore_keeps_the_sink_balanced() {
        let mut builder = PathBuilder::new();
        builder.move_to(0.0, 0.0);
        let line = builder.finish();
        let p = page(vec![
            DrawOp::Save,
            DrawOp::Save,
            DrawOp::Path {
                path: line.clone(),
                fill: Some(Paint::Solid(Color::BLACK)),
                stroke: None,
                stroke_style: Default::default(),
                rule: PaintRule::NonZero,
            },
            DrawOp::Restore,
            DrawOp::Restore,
        ]);

        let mut sink = Recorder::default();
        replay(&p, &mut sink).expect("nested balance replays");
        assert_eq!(sink.depth, 0);
        assert_eq!(sink.log.iter().filter(|l| *l == "save").count(), 2);
        assert_eq!(sink.log.iter().filter(|l| *l == "restore").count(), 2);
    }
}
