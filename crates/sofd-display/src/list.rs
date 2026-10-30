//! Display lists and their builder.

use serde::{Deserialize, Serialize};
use sofd_common::Rect;
use sofd_geometry::Affine;

use crate::ops::DrawOp;

/// The flattened display content of one page.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct PageDisplay {
    /// Zero-based page index.
    pub index: usize,
    /// Physical page size in millimetres.
    pub page_box: Rect,
    /// Flattened drawing commands.
    pub ops: Vec<DrawOp>,
    /// Union of all drawn geometry, used to detect blank pages and cull.
    pub content_bbox: Rect,
}

impl PageDisplay {
    /// Whether the page has nothing visible to draw.
    pub fn is_blank(&self) -> bool {
        !self.ops.iter().any(draws_ink)
    }
    /// Cheap upper bound on commands that may survive viewport culling.
    pub fn visible_ops_count(&self, viewport: Rect) -> usize {
        if !self.content_bbox.intersects(viewport) {
            return 0;
        }
        self.ops.len()
    }
}

/// Whether a command puts ink on the page. A table's rules are horizontal and
/// vertical lines, so they have no area at all, yet they are the whole of a
/// grid: deciding blankness by bounds alone calls such a page empty.
fn draws_ink(op: &DrawOp) -> bool {
    match op {
        DrawOp::Path {
            path,
            fill,
            stroke,
            stroke_style,
            ..
        } => {
            (fill.is_some() && path.tight_bounding_box().area() > 0.0)
                || (stroke.is_some() && stroke_style.width > 0.0)
        }
        DrawOp::Text(_) | DrawOp::Image { .. } => true,
        _ => false,
    }
}

/// Display list for a whole document, produced page by page.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct DisplayList {
    /// Total pages in the document.
    pub page_count: usize,
    /// Pages materialized so far; large documents may hold only a window.
    pub pages: Vec<PageDisplay>,
}

impl DisplayList {
    /// Look up a page by index.
    pub fn page(&self, index: usize) -> Option<&PageDisplay> {
        self.pages.iter().find(|p| p.index == index)
    }
}

/// Lazy handle to a page.
/// Large documents need not be fully flattened; a backend resolves a page only
/// when it needs it, which makes shard loading natural.
pub struct PageRef<'a> {
    /// Page index this handle resolves to.
    pub index: usize,
    /// Deferred producer of the page display list.
    pub resolve: Box<dyn FnOnce() -> sofd_error::Result<PageDisplay> + 'a>,
}

/// Builds a [`PageDisplay`] by feeding flattened commands.
/// Owns the graphics state stack, so emitted commands already carry absolute
/// coordinates and backends never re-apply the CTM.
#[derive(Debug)]
pub struct DisplayListBuilder {
    page_box: Rect,
    ops: Vec<DrawOp>,
    state_stack: Vec<Affine>,
    current: Affine,
    content_bbox: Rect,
    max_depth: usize,
}

impl DisplayListBuilder {
    /// Create a builder for a page of the given size.
    pub fn new(page_box: Rect) -> Self {
        Self {
            page_box,
            ops: Vec::with_capacity(64),
            state_stack: Vec::with_capacity(8),
            current: Affine::IDENTITY,
            content_bbox: Rect::EMPTY,
            max_depth: 64,
        }
    }

    /// Cap the graphics state stack, guarding against hostile nesting depth.
    pub fn with_max_depth(mut self, depth: usize) -> Self {
        self.max_depth = depth;
        self
    }

    /// The transform currently in effect.
    pub fn current_transform(&self) -> Affine {
        self.current
    }

    /// Push the graphics state.
    pub fn save(&mut self) -> sofd_error::Result<()> {
        if self.state_stack.len() >= self.max_depth {
            return Err(sofd_error::FormatError::MaxDepthExceeded(self.max_depth).into());
        }
        self.state_stack.push(self.current);
        self.ops.push(DrawOp::Save);
        Ok(())
    }

    /// Pop the graphics state.
    pub fn restore(&mut self) -> sofd_error::Result<()> {
        let prev = self.state_stack.pop().ok_or_else(|| {
            sofd_error::OfdError::Format(sofd_error::FormatError::CircularReference {
                chain: "restore without matching save".into(),
            })
        })?;
        self.current = prev;
        self.ops.push(DrawOp::Restore);
        Ok(())
    }

    /// Concatenate a transform onto the current state.
    pub fn transform(&mut self, t: Affine) {
        self.current = self.current.then(t);
        self.ops.push(DrawOp::Transform(t));
    }

    /// Append a command and fold its extent into the content bounds.
    pub fn push(&mut self, op: DrawOp) {
        if let Some(b) = op_bbox(&op) {
            // Geometry is recorded in object space, so the bounds must pass
            // through the transform in scope to land in page coordinates.
            self.content_bbox = self.content_bbox.union(transformed_rect(b, &self.current));
        }
        self.ops.push(op);
    }

    /// How many commands have been emitted so far. A caller that pushes a
    /// command and then needs to refer back to it reads this before the push.
    pub fn len(&self) -> usize {
        self.ops.len()
    }

    /// Whether no command has been emitted yet.
    pub fn is_empty(&self) -> bool {
        self.ops.is_empty()
    }

    /// Finish the page.
    pub fn finish(self, index: usize) -> PageDisplay {
        PageDisplay {
            index,
            page_box: self.page_box,
            ops: self.ops,
            content_bbox: self.content_bbox,
        }
    }
}

/// Conservative extent of a command, used only for bounds and blank detection.
/// State commands are absent on purpose, including [`DrawOp::ClipPath`]: a clip
/// bounds later drawing but paints nothing, so it must not mark a page non-blank.
fn op_bbox(op: &DrawOp) -> Option<Rect> {
    match op {
        DrawOp::Path { path, .. } => Some(path.bounding_box()),
        DrawOp::Text(run) => Some(run.bbox),
        DrawOp::Image { dst, .. } => Some(*dst),
        _ => None,
    }
}

/// The bounding box of a rectangle pushed through an affine map. Rotation and
/// shear grow the box; mirroring keeps it the same size.
fn transformed_rect(r: Rect, t: &Affine) -> Rect {
    let (mut min_x, mut min_y) = t.apply(r.x, r.y);
    let (mut max_x, mut max_y) = (min_x, min_y);
    for (x, y) in [(r.x + r.w, r.y), (r.x, r.y + r.h), (r.x + r.w, r.y + r.h)] {
        let (px, py) = t.apply(x, y);
        min_x = min_x.min(px);
        min_y = min_y.min(py);
        max_x = max_x.max(px);
        max_y = max_y.max(py);
    }
    Rect::new(min_x, min_y, max_x - min_x, max_y - min_y)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LinearGradientSpec;
    use crate::ops::{Color, Paint, PaintRule, StrokeStyle};
    use crate::resource::ImageRef;
    use crate::text::GlyphRun;
    use sofd_common::ImageCodec;
    use sofd_common::Point;
    use sofd_error::FormatError;
    use sofd_font::FontId;
    use sofd_geometry::Path;

    fn rect_path(x: f64, y: f64, w: f64, h: f64) -> Path {
        Path::build(|b| {
            b.move_to(x, y)
                .line_to(x + w, y)
                .line_to(x + w, y + h)
                .line_to(x, y + h)
                .close();
        })
    }

    fn fill_rect(builder: &mut DisplayListBuilder, x: f64, y: f64, w: f64, h: f64) {
        builder.push(DrawOp::Path {
            path: rect_path(x, y, w, h),
            fill: Some(Paint::Solid(Color::BLACK)),
            stroke: None,
            stroke_style: StrokeStyle::default(),
            rule: PaintRule::NonZero,
        });
    }

    /// A straight line drawn as a stroke, which is what a table rule is.
    fn stroke_line(builder: &mut DisplayListBuilder, x1: f64, y1: f64, x2: f64, y2: f64) {
        let path = Path::build(|b| {
            b.move_to(x1, y1).line_to(x2, y2);
        });
        builder.push(DrawOp::Path {
            path,
            fill: None,
            stroke: Some(Paint::Solid(Color::BLACK)),
            stroke_style: StrokeStyle {
                width: 0.25,
                ..StrokeStyle::default()
            },
            rule: PaintRule::NonZero,
        });
    }

    fn glyph_run(bbox: Rect) -> GlyphRun {
        GlyphRun {
            font: FontId(1),
            size: sofd_common::Millimeter(3.0),
            glyphs: vec![42],
            advances: vec![1.5],
            text: Some("A".to_string()),
            cluster_map: None,
            codes_are_glyphs: false,
            transform: Affine::IDENTITY,
            glyph_angle: 0.0,
            fill: Some(Paint::Solid(Color::BLACK)),
            stroke: None,
            stroke_style: StrokeStyle::default(),
            letter_spacing: 0.0,
            word_spacing: 0.0,
            bbox,
        }
    }

    fn image_ref() -> ImageRef {
        ImageRef::new(
            "img1",
            std::sync::Arc::new(Vec::new()),
            (4, 4),
            ImageCodec::Png,
        )
    }

    #[test]
    fn a_fresh_builder_starts_identity_with_no_ops() {
        let b = DisplayListBuilder::new(Rect::A4_PORTRAIT);
        assert_eq!(b.current_transform(), Affine::IDENTITY);
        let page = b.finish(0);
        assert!(page.ops.is_empty());
        assert_eq!(page.index, 0);
        assert_eq!(page.page_box, Rect::A4_PORTRAIT);
    }

    #[test]
    fn max_depth_defaults_to_sixty_four() {
        let mut b = DisplayListBuilder::new(Rect::A4_PORTRAIT);
        for _ in 0..64 {
            b.save().expect("64 levels should fit");
        }
        let err = b.save().expect_err("the 65th save must be refused");
        assert!(matches!(
            err,
            sofd_error::OfdError::Format(FormatError::MaxDepthExceeded(64))
        ));
    }

    #[test]
    fn with_max_depth_lowers_the_ceiling() {
        let mut b = DisplayListBuilder::new(Rect::A4_PORTRAIT).with_max_depth(2);
        b.save().unwrap();
        b.save().unwrap();
        let err = b.save().expect_err("third save exceeds a depth of two");
        assert!(matches!(
            err,
            sofd_error::OfdError::Format(FormatError::MaxDepthExceeded(2))
        ));
    }

    #[test]
    fn a_refused_save_emits_no_command() {
        // The guard runs before any mutation, so a rejected save cannot leave a
        // dangling Save in the stream that a backend would pair with a Restore.
        let mut b = DisplayListBuilder::new(Rect::A4_PORTRAIT).with_max_depth(1);
        b.save().unwrap();
        assert!(b.save().is_err());
        let page = b.finish(0);
        assert_eq!(
            page.ops
                .iter()
                .filter(|o| matches!(o, DrawOp::Save))
                .count(),
            1
        );
    }

    #[test]
    fn save_and_restore_round_trip_the_transform() {
        let mut b = DisplayListBuilder::new(Rect::A4_PORTRAIT);
        b.transform(Affine::translate(10.0, 20.0));
        b.save().unwrap();
        b.transform(Affine::scale(2.0, 2.0));
        assert_ne!(b.current_transform(), Affine::translate(10.0, 20.0));

        b.restore().unwrap();
        assert_eq!(
            b.current_transform(),
            Affine::translate(10.0, 20.0),
            "restore must reinstate the saved transform"
        );

        let page = b.finish(0);
        assert!(matches!(page.ops[0], DrawOp::Transform(_)));
        assert!(matches!(page.ops[1], DrawOp::Save));
        assert!(matches!(page.ops[3], DrawOp::Restore));
    }

    #[test]
    fn restore_without_a_matching_save_is_an_error() {
        let mut b = DisplayListBuilder::new(Rect::A4_PORTRAIT);
        let err = b.restore().expect_err("unbalanced restore must fail");
        assert!(matches!(
            err,
            sofd_error::OfdError::Format(FormatError::CircularReference { .. })
        ));
        assert!(b.finish(0).ops.is_empty());
    }

    #[test]
    fn nested_saves_unwind_in_reverse_order() {
        let mut b = DisplayListBuilder::new(Rect::A4_PORTRAIT);
        b.transform(Affine::translate(1.0, 0.0));
        b.save().unwrap();
        b.transform(Affine::translate(2.0, 0.0));
        b.save().unwrap();
        b.transform(Affine::translate(4.0, 0.0));

        let innermost = b.current_transform();
        b.restore().unwrap();
        let middle = b.current_transform();
        b.restore().unwrap();
        let outermost = b.current_transform();

        assert_ne!(innermost, middle);
        assert_ne!(middle, outermost);
        assert_eq!(outermost, Affine::translate(1.0, 0.0));
    }

    #[test]
    fn transforms_accumulate_left_to_right() {
        // The builder folds each transform into the running CTM, so the emitted
        // ops describe absolute placement and backends never reapply a matrix.
        // `then` applies self first: translate(5) after scale(2) sends 1 -> 7.
        let mut b = DisplayListBuilder::new(Rect::A4_PORTRAIT);
        b.transform(Affine::translate(5.0, 0.0));
        b.transform(Affine::scale(2.0, 2.0));
        let ctm = b.current_transform();

        assert_point(ctm.apply(1.0, 0.0), 12.0, 0.0);
        assert_point(ctm.apply(2.0, 0.0), 14.0, 0.0);
    }

    fn assert_point(actual: (f64, f64), x: f64, y: f64) {
        assert!(
            (actual.0 - x).abs() < 1e-9 && (actual.1 - y).abs() < 1e-9,
            "expected ({x}, {y}), got {actual:?}"
        );
    }

    #[test]
    fn content_bbox_is_the_union_of_drawn_geometry() {
        let mut b = DisplayListBuilder::new(Rect::A4_PORTRAIT);
        fill_rect(&mut b, 10.0, 10.0, 20.0, 20.0);
        fill_rect(&mut b, 50.0, 40.0, 10.0, 10.0);
        let bbox = b.finish(0).content_bbox;

        assert!((bbox.x - 10.0).abs() < 1e-9);
        assert!((bbox.y - 10.0).abs() < 1e-9);
        assert!(((bbox.x + bbox.w) - 60.0).abs() < 1e-9);
        assert!(((bbox.y + bbox.h) - 50.0).abs() < 1e-9);
    }

    #[test]
    fn state_commands_do_not_contribute_to_content_bbox() {
        // Save, Restore, Transform and Clip draw nothing, so a page made only of
        // them must still read as blank.
        let mut b = DisplayListBuilder::new(Rect::A4_PORTRAIT);
        b.save().unwrap();
        b.transform(Affine::translate(100.0, 100.0));
        b.push(DrawOp::ClipPath {
            path: rect_path(0.0, 0.0, 10.0, 10.0),
            rule: PaintRule::NonZero,
        });
        b.restore().unwrap();
        let page = b.finish(0);
        assert!(page.is_blank());
    }

    #[test]
    fn text_and_images_extend_the_content_bbox() {
        let mut b = DisplayListBuilder::new(Rect::A4_PORTRAIT);
        b.push(DrawOp::Text(glyph_run(Rect::new(10.0, 20.0, 30.0, 4.0))));
        b.push(DrawOp::Image {
            image: image_ref(),
            dst: Rect::new(80.0, 90.0, 40.0, 30.0),
        });
        let bbox = b.finish(0).content_bbox;

        assert!((bbox.x - 10.0).abs() < 1e-9);
        assert!(((bbox.y + bbox.h) - 120.0).abs() < 1e-9);
        assert!(((bbox.x + bbox.w) - 120.0).abs() < 1e-9);
    }

    #[test]
    fn a_gradient_fill_bounds_by_its_path_not_its_axis() {
        // A gradient's start and end may lie far outside the shape it fills, so
        // only the path is a safe bound.
        let mut b = DisplayListBuilder::new(Rect::A4_PORTRAIT);
        b.push(DrawOp::Path {
            path: rect_path(10.0, 10.0, 20.0, 20.0),
            fill: Some(Paint::LinearGradient(Box::new(LinearGradientSpec {
                start: Point::new(-500.0, -500.0),
                end: Point::new(500.0, 500.0),
                stops: Vec::new(),
            }))),
            stroke: None,
            stroke_style: Default::default(),
            rule: PaintRule::NonZero,
        });
        let bbox = b.finish(0).content_bbox;
        assert!((bbox.x - 10.0).abs() < 1e-9);
        assert!(((bbox.x + bbox.w) - 30.0).abs() < 1e-9);
    }

    #[test]
    fn an_empty_page_is_blank() {
        let page = DisplayListBuilder::new(Rect::A4_PORTRAIT).finish(0);
        assert!(page.is_blank());
        assert_eq!(page.visible_ops_count(Rect::A4_PORTRAIT), 0);
    }

    #[test]
    fn a_page_with_zero_area_content_is_blank() {
        // A degenerate path is the classic accidental blank page.
        let mut b = DisplayListBuilder::new(Rect::A4_PORTRAIT);
        fill_rect(&mut b, 10.0, 10.0, 0.0, 0.0);
        let page = b.finish(0);
        assert!(!page.ops.is_empty(), "the op was still recorded");
        assert!(page.is_blank(), "but it draws nothing");
    }

    #[test]
    fn a_page_with_geometry_is_not_blank() {
        let mut b = DisplayListBuilder::new(Rect::A4_PORTRAIT);
        fill_rect(&mut b, 10.0, 10.0, 20.0, 20.0);
        let page = b.finish(0);
        assert!(!page.is_blank());
        assert_eq!(page.ops.len(), 1);
        assert_eq!(page.visible_ops_count(Rect::A4_PORTRAIT), 1);
    }

    #[test]
    fn a_page_of_table_rules_is_not_blank() {
        // A grid is made of horizontal and vertical rules, and each has no area
        // of its own: the union of their bounds does, so reading blankness off
        // the bounds answered "blank" for every table page.
        let mut b = DisplayListBuilder::new(Rect::A4_PORTRAIT);
        stroke_line(&mut b, 10.0, 40.0, 190.0, 40.0);
        stroke_line(&mut b, 10.0, 40.0, 10.0, 250.0);
        let page = b.finish(0);
        assert!(!page.is_blank(), "a frame and a rule are ink");
    }

    #[test]
    fn a_viewport_missing_the_content_culls_everything() {
        let mut b = DisplayListBuilder::new(Rect::A4_PORTRAIT);
        fill_rect(&mut b, 10.0, 10.0, 20.0, 20.0);
        let page = b.finish(0);

        assert_eq!(page.visible_ops_count(Rect::new(0.0, 0.0, 5.0, 5.0)), 0);
        assert_eq!(page.visible_ops_count(Rect::new(10.0, 10.0, 20.0, 20.0)), 1);
    }

    #[test]
    fn finish_records_the_requested_page_index() {
        let mut b = DisplayListBuilder::new(Rect::A4_LANDSCAPE);
        fill_rect(&mut b, 0.0, 0.0, 10.0, 10.0);
        let page = b.finish(7);
        assert_eq!(page.index, 7);
        assert_eq!(page.page_box, Rect::A4_LANDSCAPE);
    }

    #[test]
    fn display_list_finds_pages_by_index() {
        // Pages need not be contiguous, since a large document may hold a window.
        let mut first = DisplayListBuilder::new(Rect::A4_PORTRAIT);
        fill_rect(&mut first, 0.0, 0.0, 10.0, 10.0);
        let mut second = DisplayListBuilder::new(Rect::A4_PORTRAIT);
        fill_rect(&mut second, 0.0, 0.0, 20.0, 20.0);

        let list = DisplayList {
            page_count: 100,
            pages: vec![first.finish(0), second.finish(5)],
        };

        assert_eq!(list.page(0).map(|p| p.index), Some(0));
        assert_eq!(list.page(5).map(|p| p.index), Some(5));
        assert!(list.page(1).is_none());
        assert!(list.page(99).is_none());
        assert_eq!(list.page_count, 100);
    }

    #[test]
    fn display_list_defaults_to_empty() {
        let list = DisplayList::default();
        assert_eq!(list.page_count, 0);
        assert!(list.page(0).is_none());
    }

    #[test]
    fn a_page_display_survives_a_serde_round_trip() {
        // Pages cross a process boundary when rendering is farmed out, so the
        // wire format has to preserve both the ops and the cached bounds.
        let mut b = DisplayListBuilder::new(Rect::A4_PORTRAIT);
        fill_rect(&mut b, 10.0, 10.0, 20.0, 20.0);
        let page = b.finish(3);

        let json = serde_json::to_string(&page).expect("serialize");
        let back: PageDisplay = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(back.index, 3);
        assert_eq!(back.ops.len(), page.ops.len());
        assert!((back.content_bbox.area() - page.content_bbox.area()).abs() < 1e-9);
    }
}
