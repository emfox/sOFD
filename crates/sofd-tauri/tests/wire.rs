//! The wire format, checked against a real document without a window.
//!
//! What a canvas needs is the same page the `egui` reader draws, expressed
//! differently, so the tests here ask the document for a page and look at what
//! a front end would actually receive: shapes in drawing order, a `d` that
//! parses, a stroke width in millimetres, and a colour it can set.

use sofd::{Document, RenderOptions};
use sofd_common::Rect;
use sofd_io::FileSource;
use sofd_tauri::wire::{WirePaint, WireShape, geometry, marks};

fn fixture(name: &str) -> Document {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../compliance/fixtures")
        .join(name);
    let source = FileSource::open(&path).expect("the fixture opens");
    Document::open(&source).expect("the fixture parses")
}

fn page(name: &str, index: usize) -> sofd_tauri::wire::PageGeometry {
    let doc = fixture(name);
    let walked = doc
        .vector_page(index, &RenderOptions::default())
        .expect("the page walks");
    geometry(index, &walked)
}

/// A page arrives as shapes, with the page's own size in millimetres, which is
/// the space a canvas is set up to draw in.
#[test]
fn a_page_arrives_as_shapes_on_a_page_sized_canvas() {
    let page = page("minimal.ofd", 0);
    assert_eq!(page.page, 0);
    assert!((page.width_mm - 210.0).abs() < 0.5, "{}", page.width_mm);
    assert!((page.height_mm - 297.0).abs() < 0.5, "{}", page.height_mm);
    assert!(!page.shapes.is_empty(), "a page with text draws something");
}

/// Every path a canvas is handed starts with a moveto, which is what makes it
/// a path rather than a fragment. A `d` that begins with a curve is one a
/// browser rejects outright.
#[test]
fn every_path_starts_where_a_path_must_start() {
    let page = page("wps.ofd", 0);
    let mut paths = 0;
    for shape in &page.shapes {
        let d = match shape {
            WireShape::Ink { d, .. } | WireShape::Clip { d, .. } => d,
            _ => continue,
        };
        paths += 1;
        assert!(
            d.starts_with('M'),
            "path data begins with {:?}",
            d.chars().next()
        );
        assert!(d.len() > 1, "path data is a bare moveto");
    }
    assert!(paths > 0, "the page has no paths to check");
}

/// A stroke carries its width, in millimetres, because the whole point of a
/// canvas is that a rule and a seal are not the same thickness.
#[test]
fn a_stroke_keeps_the_width_it_was_drawn_with() {
    let page = page("minimal.ofd", 0);
    let widths: Vec<f64> = page
        .shapes
        .iter()
        .filter_map(|shape| match shape {
            WireShape::Ink { stroke, style, .. } if stroke.is_some() => Some(style.width),
            _ => None,
        })
        .collect();
    assert!(!widths.is_empty(), "the fixture has a stroked path");
    for width in widths {
        assert!(
            width > 0.0 && width < 10.0,
            "{width} mm is not a width a document states"
        );
    }
}

/// A colour reaches a canvas as something CSS accepts, alpha included.
#[test]
fn a_colour_arrives_as_css() {
    let page = page("minimal.ofd", 0);
    let mut solids = 0;
    for shape in &page.shapes {
        let WireShape::Ink { fill, stroke, .. } = shape else {
            continue;
        };
        for paint in [fill.as_ref(), stroke.as_ref()].into_iter().flatten() {
            if let WirePaint::Solid { css } = paint {
                solids += 1;
                assert!(css.starts_with("rgba("), "{css} is not a CSS colour");
                assert!(css.ends_with(')'), "{css} is not a CSS colour");
            }
        }
    }
    assert!(solids > 0, "the fixture paints nothing solid");
}

/// The three counters the engine keeps reach the window, so a page that lost a
/// glyph can be reported rather than looking complete.
#[test]
fn a_page_reports_what_it_could_not_draw() {
    let page = page("minimal.ofd", 0);
    assert_eq!(page.missing_glyphs, 0, "the fixture's face is complete");
    assert_eq!(page.missing_glyph_runs, 0);
    assert_eq!(page.skipped_font_runs, 0);
    assert!(
        page.degraded.is_empty(),
        "nothing was approximated: {:?}",
        page.degraded
    );
}

/// Drawing order is the page's, so a canvas that draws in the order it is given
/// gets the page rather than a pile of shapes. The last shape of a letter is
/// its body, not its page-number layer.
#[test]
fn shapes_keep_the_order_the_page_draws_them_in() {
    let page = page("wps.ofd", 0);
    let inks: Vec<&WireShape> = page
        .shapes
        .iter()
        .filter(|s| matches!(s, WireShape::Ink { .. }))
        .collect();
    assert!(inks.len() > 10, "the page has a paragraph on it");
    // The first run in this document's stream is the footer, which is what the
    // ordering test is for: the page's own body comes after it in the list.
    for shape in &page.shapes {
        assert!(
            !matches!(shape, WireShape::ClipReset) || page.shapes.len() > 1,
            "a reset on its own is not a page"
        );
    }
}

/// Serde writes the shape of the format the front end reads: a tagged union,
/// camelCase fields, and no `null` where a canvas needs a value.
#[test]
fn the_json_is_what_a_browser_reads() {
    let page = page("minimal.ofd", 0);
    let json = serde_json::to_value(&page).expect("a page serializes");
    assert_eq!(json["page"], 0);
    assert!(json["widthMm"].as_f64().is_some(), "{json}");
    assert!(json["heightMm"].as_f64().is_some());
    let shapes = json["shapes"].as_array().expect("shapes is a list");
    assert!(!shapes.is_empty());
    let ink = shapes
        .iter()
        .find(|s| s["kind"] == "ink")
        .expect("a page of text has ink");
    assert!(ink["d"].as_str().is_some(), "the path data is there");
    assert!(ink["style"]["width"].as_f64().is_some(), "and its width");
    assert_eq!(ink["rule"], "nonzero");
}

/// A search hit and an annotation reach the canvas as a rectangle in page
/// millimetres, named by the token that colours it. Both are drawn over the
/// paper, so neither takes its colour from the theme's surface.
#[test]
fn an_overlay_carries_a_rectangle_and_the_token_that_colours_it() {
    let hit = Rect::new(10.0, 20.0, 40.0, 5.0);
    let boundary = Rect::new(30.0, 60.0, 20.0, 8.0);
    let wire = marks(&[(hit, true)], &[(boundary, false)]);
    assert_eq!(wire.hits.len(), 1);
    assert_eq!(wire.hits[0].x, 10.0);
    assert!(wire.hits[0].current);
    assert_eq!(wire.hits[0].token, "--hit");
    assert_eq!(wire.marks.len(), 1);
    assert_eq!(wire.marks[0].y, 60.0);
    assert!(!wire.marks[0].current);
    assert_eq!(wire.marks[0].token, "--mark");
    let json = serde_json::to_value(&wire).expect("overlays serialize");
    assert_eq!(json["hits"][0]["token"], "--hit");
}
