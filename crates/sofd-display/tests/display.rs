//! The display layer driven as a consumer.
//! A page is built with the public builder and measured through the public
//! accessors, so the builder's own contract is what these tests pin down.

use sofd_common::{Millimeter, Rect};
use sofd_display::ops::{Paint, PaintRule, StrokeStyle};
use sofd_display::text::GlyphRun;
use sofd_display::{
    DisplayList, DisplayListBuilder, DrawOp, ImageCodec, ImageRef, PageDisplay, Viewport,
};
use sofd_font::FontId;
use sofd_geometry::{Affine, Path};

fn rect(x: f64, y: f64, w: f64, h: f64) -> Path {
    Path::build(|b| {
        b.rect(x, y, w, h);
    })
}

fn filled(path: Path) -> DrawOp {
    DrawOp::Path {
        path,
        fill: Some(Paint::Solid(sofd_display::ops::Color::BLACK)),
        stroke: None,
        stroke_style: StrokeStyle::default(),
        rule: PaintRule::NonZero,
    }
}

fn empty_page(index: usize) -> PageDisplay {
    PageDisplay {
        index,
        page_box: Rect::A4_PORTRAIT,
        ops: Vec::new(),
        content_bbox: Rect::EMPTY,
    }
}

fn text_run(bbox: Rect) -> GlyphRun {
    GlyphRun {
        font: FontId(0),
        size: Millimeter(3.5),
        glyphs: vec![1, 2, 3],
        advances: vec![1.0, 1.0, 1.0],
        text: Some("abc".to_string()),
        cluster_map: Some(vec![0, 1, 2]),
        codes_are_glyphs: false,
        transform: Affine::IDENTITY,
        glyph_angle: 0.0,
        fill: Some(Paint::Solid(sofd_display::ops::Color::BLACK)),
        stroke: None,
        stroke_style: Default::default(),
        letter_spacing: 0.0,
        word_spacing: 0.0,
        bbox,
    }
}

#[test]
fn a_new_builder_emits_nothing_and_starts_at_the_identity() {
    let builder = DisplayListBuilder::new(Rect::A4_PORTRAIT);
    assert!(builder.is_empty());
    assert_eq!(builder.len(), 0);
    assert_eq!(builder.current_transform(), Affine::IDENTITY);
}

#[test]
fn a_pushed_path_widens_the_content_bounds() {
    let mut builder = DisplayListBuilder::new(Rect::A4_PORTRAIT);
    builder.push(filled(rect(10.0, 20.0, 30.0, 40.0)));
    let page = builder.finish(0);
    assert_eq!(page.ops.len(), 1);
    // `Rect` is origin plus size, so a 30x40 path at (10, 20) bounds exactly so.
    assert_eq!(page.content_bbox, Rect::new(10.0, 20.0, 30.0, 40.0));
    assert!(!page.is_blank());
}

#[test]
fn a_page_with_no_commands_is_blank() {
    let builder = DisplayListBuilder::new(Rect::A4_PORTRAIT);
    let page = builder.finish(0);
    assert!(page.is_blank());
    assert_eq!(page.index, 0);
}

#[test]
fn a_page_whose_content_has_no_area_is_blank_even_with_commands() {
    // A zero-area paint produces commands but no ink, so blank detection has to
    // look at the bounds rather than only at the command count.
    let mut builder = DisplayListBuilder::new(Rect::A4_PORTRAIT);
    builder.push(filled(rect(5.0, 5.0, 0.0, 0.0)));
    let page = builder.finish(0);
    assert_eq!(page.ops.len(), 1);
    assert!(page.is_blank());
}

#[test]
fn state_commands_do_not_mark_a_page_non_blank() {
    let mut builder = DisplayListBuilder::new(Rect::A4_PORTRAIT);
    builder.save().expect("save");
    builder.transform(Affine::translate(1.0, 2.0));
    builder.restore().expect("restore");
    let page = builder.finish(0);
    // Save, transform and restore are all on the list, yet nothing was painted.
    assert_eq!(page.ops.len(), 3);
    assert!(page.is_blank());
}

#[test]
fn saving_and_restoring_round_trips_the_transform() {
    let mut builder = DisplayListBuilder::new(Rect::A4_PORTRAIT);
    builder.transform(Affine::translate(5.0, 5.0));
    builder.save().expect("save");
    builder.transform(Affine::scale(2.0, 2.0));
    assert_ne!(builder.current_transform(), Affine::translate(5.0, 5.0));
    builder.restore().expect("restore");
    assert_eq!(builder.current_transform(), Affine::translate(5.0, 5.0));
}

#[test]
fn a_restore_without_a_save_is_an_error() {
    let mut builder = DisplayListBuilder::new(Rect::A4_PORTRAIT);
    let err = builder.restore().expect_err("unbalanced restore");
    assert!(err.to_string().contains("restore"), "{err}");
}

#[test]
fn the_state_stack_respects_its_depth_ceiling() {
    let mut builder = DisplayListBuilder::new(Rect::A4_PORTRAIT).with_max_depth(4);
    for _ in 0..4 {
        builder.save().expect("within the ceiling");
    }
    // The next save would exceed the ceiling and must be refused rather than
    // growing the stack without bound.
    let err = builder.save().expect_err("past the ceiling");
    assert!(err.to_string().contains('4'), "{err}");
}

#[test]
fn a_text_run_contributes_its_bounding_box() {
    let mut builder = DisplayListBuilder::new(Rect::A4_PORTRAIT);
    builder.push(DrawOp::Text(text_run(Rect::new(0.0, 0.0, 20.0, 5.0))));
    let page = builder.finish(0);
    assert_eq!(page.content_bbox.w, 20.0);
    assert!(!page.is_blank());
}

#[test]
fn an_image_contributes_its_destination_rectangle() {
    let mut builder = DisplayListBuilder::new(Rect::A4_PORTRAIT);
    builder.push(DrawOp::Image {
        image: ImageRef::new(
            "img0",
            std::sync::Arc::new(Vec::new()),
            (10, 10),
            ImageCodec::Png,
        ),
        dst: Rect::new(1.0, 2.0, 30.0, 40.0),
    });
    let page = builder.finish(0);
    assert_eq!(page.content_bbox, Rect::new(1.0, 2.0, 30.0, 40.0));
    assert_eq!(page.content_bbox.area(), 1200.0);
}

#[test]
fn a_clip_alone_leaves_the_page_blank() {
    let mut builder = DisplayListBuilder::new(Rect::A4_PORTRAIT);
    builder.push(DrawOp::ClipPath {
        path: rect(0.0, 0.0, 10.0, 10.0),
        rule: PaintRule::NonZero,
    });
    let page = builder.finish(0);
    // A clip narrows later drawing but paints nothing, so the page is still
    // blank; treating it otherwise would report ink that is not there.
    assert!(page.is_blank());
}

#[test]
fn a_viewport_that_misses_the_content_culls_every_command() {
    let mut builder = DisplayListBuilder::new(Rect::A4_PORTRAIT);
    builder.push(filled(rect(0.0, 0.0, 10.0, 10.0)));
    let page = builder.finish(0);
    let far_away = Rect::new(500.0, 500.0, 100.0, 100.0);
    assert_eq!(page.visible_ops_count(far_away), 0);
}

#[test]
fn a_viewport_that_overlaps_the_content_keeps_the_commands() {
    let mut builder = DisplayListBuilder::new(Rect::A4_PORTRAIT);
    builder.push(filled(rect(0.0, 0.0, 10.0, 10.0)));
    builder.push(filled(rect(20.0, 20.0, 10.0, 10.0)));
    let page = builder.finish(0);
    let overlapping = Rect::new(5.0, 5.0, 20.0, 20.0);
    assert_eq!(page.visible_ops_count(overlapping), 2);
}

#[test]
fn a_display_list_looks_pages_up_by_index_not_position() {
    let list = DisplayList {
        page_count: 3,
        // Deliberately not in index order, to prove lookup is by `index`.
        pages: vec![empty_page(2), empty_page(0)],
    };
    assert_eq!(list.page(0).expect("page 0").index, 0);
    assert_eq!(list.page(2).expect("page 2").index, 2);
    assert!(list.page(1).is_none(), "a page not materialized is absent");
}

#[test]
fn a_whole_page_viewport_covers_the_page_box_at_the_requested_dpi() {
    let viewport = Viewport::whole_page(Rect::A4_PORTRAIT, 96.0);
    assert_eq!(viewport.area, Rect::A4_PORTRAIT);
    assert_eq!(viewport.effective_dpi(), 96.0);
    let (w, h) = viewport.output_size();
    // 210mm at 96 DPI is about 794px and 297mm about 1123px.
    assert!((w as i64 - 794).abs() <= 2, "width was {w}");
    assert!((h as i64 - 1123).abs() <= 2, "height was {h}");
}

#[test]
fn zoom_multiplies_the_effective_dpi_and_the_output_size() {
    let base = Viewport::whole_page(Rect::A4_PORTRAIT, 96.0);
    let zoomed = Viewport { zoom: 2.0, ..base };
    assert_eq!(zoomed.effective_dpi(), 192.0);
    let (w0, _) = base.output_size();
    let (w1, _) = zoomed.output_size();
    assert!(
        (w1 as i64 - w0 as i64 * 2).abs() <= 2,
        "widths were {w0} and {w1}"
    );
}

#[test]
fn a_viewport_entirely_off_the_page_does_not_intersect_it() {
    let away = Viewport {
        area: Rect::new(1000.0, 1000.0, 10.0, 10.0),
        dpi: 96.0,
        zoom: 1.0,
    };
    assert!(!away.intersects_page(Rect::A4_PORTRAIT));
    let on = Viewport::whole_page(Rect::A4_PORTRAIT, 96.0);
    assert!(on.intersects_page(Rect::A4_PORTRAIT));
}
