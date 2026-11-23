use super::*;
use sofd_common::{Millimeter, Point};
use sofd_display::DrawOp;
use sofd_display::MeshSpec;
use sofd_display::ops::{GradientStop, Paint};
use sofd_font::FontId;

/// A mesh vertex at a position, carrying a colour.
fn vertex(x: f64, y: f64, color: Color) -> sofd_display::MeshVertex {
    sofd_display::MeshVertex { x, y, color }
}

/// The vertex colours a mesh triangle carries, read off the XML.
fn triangle_fills(svg: &str) -> Vec<String> {
    svg.match_indices("<path d=\"M")
        .filter_map(|(i, _)| {
            let tail = &svg[i..];
            let end = tail.find("/>")?;
            let element = &tail[..end];
            if !element.contains("fill-opacity") {
                return None;
            }
            let start = element.find("fill=\"#")? + 7;
            Some(element[start - 1..start + 6].to_string())
        })
        .collect()
}

#[test]
fn a_mesh_is_exported_as_one_path_per_triangle() {
    // Two triangles over four vertices, each corner a different colour, so
    // one mean for the whole mesh and one per triangle differ visibly.
    let mut page = page(Vec::new());
    page.ops.push(DrawOp::Path {
        path: rect(0.0, 0.0, 40.0, 40.0),
        fill: Some(Paint::Mesh(Box::new(MeshSpec {
            points: vec![
                vertex(0.0, 0.0, Color::rgba(255, 0, 0, 255)),
                vertex(40.0, 0.0, Color::rgba(0, 255, 0, 255)),
                vertex(0.0, 40.0, Color::rgba(0, 0, 255, 255)),
                vertex(40.0, 40.0, Color::rgba(255, 255, 0, 255)),
            ],
            triangles: vec![[0, 1, 2], [1, 3, 2]],
        }))),
        stroke: None,
        stroke_style: StrokeStyle::default(),
        rule: PaintRule::NonZero,
    });
    let svg = page_to_svg(&page, &SvgOptions::default()).expect("convert");
    let fills = triangle_fills(&svg);
    assert_eq!(fills.len(), 2, "one path per triangle: {svg}");
    // Red, green and blue average to (85, 85, 85); the other triangle is
    // green, yellow and blue, which averages to (85, 170, 85).
    assert_eq!(fills[0], "#555555", "{svg}");
    assert_eq!(fills[1], "#55aa55", "{svg}");
    // The geometry keeps only its stroke, so it does not paint over them.
    assert!(svg.contains("fill=\"none\""), "{svg}");
}

#[test]
fn a_mesh_triangle_out_of_range_is_skipped_not_fatal() {
    let mut page = page(Vec::new());
    page.ops.push(DrawOp::Path {
        path: rect(0.0, 0.0, 40.0, 40.0),
        fill: Some(Paint::Mesh(Box::new(MeshSpec {
            points: vec![
                vertex(0.0, 0.0, Color::rgba(255, 0, 0, 255)),
                vertex(40.0, 0.0, Color::rgba(0, 255, 0, 255)),
                vertex(0.0, 40.0, Color::rgba(0, 0, 255, 255)),
            ],
            // The second triangle names a vertex that was never written.
            triangles: vec![[0, 1, 2], [0, 1, 9]],
        }))),
        stroke: None,
        stroke_style: StrokeStyle::default(),
        rule: PaintRule::NonZero,
    });
    let svg = page_to_svg(&page, &SvgOptions::default()).expect("convert");
    assert_eq!(triangle_fills(&svg).len(), 1, "{svg}");
}

#[test]
fn a_mesh_with_no_triangles_keeps_the_mean_colour() {
    let mut page = page(Vec::new());
    page.ops.push(DrawOp::Path {
        path: rect(0.0, 0.0, 40.0, 40.0),
        fill: Some(Paint::Mesh(Box::new(MeshSpec {
            points: vec![
                vertex(0.0, 0.0, Color::rgba(0, 0, 0, 255)),
                vertex(40.0, 40.0, Color::rgba(255, 255, 255, 255)),
            ],
            triangles: Vec::new(),
        }))),
        stroke: None,
        stroke_style: StrokeStyle::default(),
        rule: PaintRule::NonZero,
    });
    let svg = page_to_svg(&page, &SvgOptions::default()).expect("convert");
    assert!(triangle_fills(&svg).is_empty(), "{svg}");
    assert!(svg.contains("fill=\"#7f7f7f\""), "{svg}");
}

/// A real face, so outlining runs for real rather than degrading.
fn font_bytes() -> Vec<u8> {
    std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../sofd-render/tests/data/Orbitron-Bold.ttf"
    ))
    .expect("font fixture")
}

fn rect(x: f64, y: f64, w: f64, h: f64) -> Path {
    Path::build(|b| {
        b.rect(x, y, w, h);
    })
}

fn page(ops: Vec<DrawOp>) -> sofd_display::PageDisplay {
    sofd_display::PageDisplay {
        index: 0,
        page_box: Rect::A4_PORTRAIT,
        ops,
        content_bbox: Rect::EMPTY,
    }
}

#[test]
fn the_root_element_carries_the_page_box_in_millimetres() {
    let svg = page_to_svg(&page(Vec::new()), &SvgOptions::default()).expect("convert");
    assert!(svg.starts_with("<?xml version=\"1.0\" encoding=\"UTF-8\"?>"));
    assert!(svg.contains("width=\"210mm\" height=\"297mm\""));
    assert!(svg.contains("viewBox=\"0 0 210 297\""));
    assert!(svg.ends_with("</svg>\n"));
}

#[test]
fn a_rect_becomes_one_path_with_its_paint() {
    let p = page(vec![DrawOp::Path {
        path: rect(10.0, 20.0, 30.0, 40.0),
        fill: Some(Paint::Solid(Color::rgba(255, 0, 0, 128))),
        stroke: None,
        stroke_style: StrokeStyle::default(),
        rule: PaintRule::NonZero,
    }]);
    let svg = page_to_svg(&p, &SvgOptions::default()).expect("convert");
    assert!(svg.contains("<path d=\"M10 20 L40 20 L40 60 L10 60 Z\" fill=\"#ff0000\" fill-opacity=\"0.502\" stroke=\"none\"/>"));
}

#[test]
fn a_path_without_fill_says_so_explicitly() {
    let p = page(vec![DrawOp::Path {
        path: rect(0.0, 0.0, 1.0, 1.0),
        fill: None,
        stroke: None,
        stroke_style: StrokeStyle::default(),
        rule: PaintRule::NonZero,
    }]);
    let svg = page_to_svg(&p, &SvgOptions::default()).expect("convert");
    assert!(svg.contains("fill=\"none\""));
    assert!(svg.contains("stroke=\"none\""));
}

/// Outlined text keeps both paints: the same path is filled and stroked,
/// which is what SVG already knows how to show.
#[test]
fn an_outlined_run_carries_its_fill_and_its_stroke() {
    let mut manager = FontManager::new();
    manager.registry_mut().register_embedded(
        "tests/data/Orbitron-Bold.ttf",
        "Orbitron",
        std::sync::Arc::new(font_bytes()),
    );
    let style = StrokeStyle {
        width: 0.5,
        dash: vec![2.0, 1.0],
        ..StrokeStyle::default()
    };
    let mut run = outlined_run();
    run.stroke = Some(Paint::Solid(Color::rgba(255, 0, 0, 255)));
    run.stroke_style = style;
    let options = SvgOptions {
        text: TextExport::Outline,
        fonts: Some(&manager),
    };
    let mut sink = SvgSink::new(Rect::A4_PORTRAIT, &options);
    replay(&page(vec![DrawOp::Text(run)]), &mut sink).expect("replay");
    let (svg, stats) = sink.finish();
    assert!(svg.contains("stroke=\"#ff0000\""), "got {svg}");
    assert!(svg.contains("stroke-width=\"0.5\""), "got {svg}");
    assert!(svg.contains("stroke-dasharray=\"2 1\""), "got {svg}");
    assert!(svg.contains("fill=\"#000000\""), "the fill is still there");
    assert_eq!(stats.stroke_only_glyph_runs, 0);
}

/// A stroke with no fill is still outlined text, not a dropped run.
#[test]
fn an_outlined_run_with_only_a_stroke_is_written() {
    let mut manager = FontManager::new();
    manager.registry_mut().register_embedded(
        "tests/data/Orbitron-Bold.ttf",
        "Orbitron",
        std::sync::Arc::new(font_bytes()),
    );
    let mut run = outlined_run();
    run.fill = None;
    run.stroke = Some(Paint::Solid(Color::BLACK));
    let options = SvgOptions {
        text: TextExport::Outline,
        fonts: Some(&manager),
    };
    let mut sink = SvgSink::new(Rect::A4_PORTRAIT, &options);
    replay(&page(vec![DrawOp::Text(run)]), &mut sink).expect("replay");
    let (svg, stats) = sink.finish();
    assert!(svg.contains("fill=\"none\""), "got {svg}");
    assert!(svg.contains("stroke=\"#000000\""), "got {svg}");
    assert!(!stats.is_lossless(), "a counted loss is still a loss");
}

/// A run built for the outline tests, filled in black.
fn outlined_run() -> GlyphRun {
    GlyphRun {
        font: FontId(0),
        size: Millimeter(5.0),
        glyphs: vec![65],
        advances: vec![5.0],
        text: Some("A".into()),
        cluster_map: None,
        codes_are_glyphs: false,
        transform: Affine::IDENTITY,
        glyph_angle: 0.0,
        fill: Some(Paint::Solid(Color::BLACK)),
        stroke: None,
        stroke_style: Default::default(),
        letter_spacing: 0.0,
        bbox: Rect::EMPTY,
    }
}

#[test]
fn the_even_odd_rule_reaches_the_fill_rule_attribute() {
    let p = page(vec![DrawOp::Path {
        path: rect(0.0, 0.0, 1.0, 1.0),
        fill: Some(Paint::Solid(Color::BLACK)),
        stroke: None,
        stroke_style: StrokeStyle::default(),
        rule: PaintRule::EvenOdd,
    }]);
    let svg = page_to_svg(&p, &SvgOptions::default()).expect("convert");
    assert!(svg.contains("fill-rule=\"evenodd\""));
}

#[test]
fn stroke_parameters_carry_over_in_millimetres() {
    let style = StrokeStyle {
        width: 0.5,
        dash: vec![3.0, 1.0],
        dash_offset: 0.25,
        cap: LineCap::Round,
        join: LineJoin::Bevel,
        miter_limit: 2.0,
    };
    let p = page(vec![DrawOp::Path {
        path: rect(0.0, 0.0, 1.0, 1.0),
        fill: None,
        stroke: Some(Paint::Solid(Color::BLACK)),
        stroke_style: style,
        rule: PaintRule::NonZero,
    }]);
    let svg = page_to_svg(&p, &SvgOptions::default()).expect("convert");
    assert!(svg.contains("stroke-width=\"0.5\""));
    assert!(svg.contains("stroke-dasharray=\"3 1\""));
    assert!(svg.contains("stroke-dashoffset=\"0.25\""));
    assert!(svg.contains("stroke-linecap=\"round\""));
    assert!(svg.contains("stroke-linejoin=\"bevel\""));
    assert!(svg.contains("stroke-miterlimit=\"2\""));
}

#[test]
fn transforms_nest_as_groups_that_restore_closes() {
    let p = page(vec![
        DrawOp::Save,
        DrawOp::Transform(Affine::translate(5.0, 6.0)),
        DrawOp::Restore,
    ]);
    let svg = page_to_svg(&p, &SvgOptions::default()).expect("convert");
    assert!(svg.contains("<g transform=\"matrix(1 0 0 1 5 6)\">"));
    // The group count balances: every opened group is closed again.
    assert_eq!(svg.matches("<g").count(), svg.matches("</g>").count());
}

#[test]
fn a_clip_emits_a_definition_and_a_referencing_group() {
    let p = page(vec![
        DrawOp::Save,
        DrawOp::ClipPath {
            path: rect(0.0, 0.0, 10.0, 10.0),
            rule: PaintRule::EvenOdd,
        },
        DrawOp::Restore,
    ]);
    let svg = page_to_svg(&p, &SvgOptions::default()).expect("convert");
    assert!(svg.contains("<clipPath id=\"clip1\" clipPathUnits=\"userSpaceOnUse\">"));
    assert!(svg.contains("clip-rule=\"evenodd\""));
    assert!(svg.contains("<g clip-path=\"url(#clip1)\">"));
}

#[test]
fn a_text_clip_emits_the_glyph_outlines_as_a_clip_path() {
    // Clause 8.4: a text area clips to the glyph outlines, so the SVG
    // carries the outlines inside the clipPath rather than a path.
    let mut manager = FontManager::new();
    manager.registry_mut().register_embedded(
        "tests/data/Orbitron-Bold.ttf",
        "Orbitron",
        std::sync::Arc::new(font_bytes()),
    );
    let run = GlyphRun {
        font: FontId(0),
        size: Millimeter(5.0),
        glyphs: vec![65],
        advances: vec![5.0],
        text: Some("A".into()),
        cluster_map: None,
        codes_are_glyphs: false,
        transform: Affine::IDENTITY,
        glyph_angle: 0.0,
        fill: None,
        stroke: None,
        stroke_style: Default::default(),
        letter_spacing: 0.0,
        bbox: Rect::EMPTY,
    };
    let options = SvgOptions {
        text: TextExport::Outline,
        fonts: Some(&manager),
    };
    let mut sink = SvgSink::new(Rect::A4_PORTRAIT, &options);
    replay(
        &page(vec![DrawOp::Save, DrawOp::ClipText(run), DrawOp::Restore]),
        &mut sink,
    )
    .expect("replay");
    let (svg, _) = sink.finish();
    assert!(
        svg.contains("<clipPath id=\"clip1\" clipPathUnits=\"userSpaceOnUse\"><path d=\""),
        "the glyph outlines become the clip path, got {svg}"
    );
    assert!(svg.contains("<g clip-path=\"url(#clip1)\">"));
}

#[test]
fn a_pattern_becomes_a_tiling_definition() {
    // SVG's pattern is the clause's model: the element spans one step and
    // the cell repeats across it, so the cell's own path reaches the def.
    let cell = sofd_display::PageDisplay {
        index: 0,
        page_box: Rect::new(0.0, 0.0, 4.0, 4.0),
        ops: vec![DrawOp::Path {
            path: rect(0.0, 0.0, 2.0, 2.0),
            fill: Some(Paint::Solid(Color::rgba(255, 0, 0, 255))),
            stroke: None,
            stroke_style: Default::default(),
            rule: PaintRule::NonZero,
        }],
        content_bbox: Rect::EMPTY,
    };
    let p = page(vec![DrawOp::Path {
        path: rect(0.0, 0.0, 20.0, 20.0),
        fill: Some(Paint::Pattern(Box::new(sofd_display::PatternSpec {
            cell: Box::new(cell),
            width: 4.0,
            height: 4.0,
            step_x: 6.0,
            step_y: 6.0,
            reflect: sofd_display::Reflect::Normal,
        }))),
        stroke: None,
        stroke_style: Default::default(),
        rule: PaintRule::NonZero,
    }]);
    let svg = page_to_svg(&p, &SvgOptions::default()).expect("convert");
    assert!(
        svg.contains("<pattern id=\"pat1\" patternUnits=\"userSpaceOnUse\""),
        "a pattern def is written: {svg}"
    );
    // The step, not the tile size, is the element's span.
    assert!(
        svg.contains("width=\"6\" height=\"6\""),
        "the step sets the repeat: {svg}"
    );
    assert!(
        svg.contains("fill=\"#ff0000\""),
        "the cell's own fill is inside the def: {svg}"
    );
    assert!(
        svg.contains("fill=\"url(#pat1)\""),
        "the path references the pattern: {svg}"
    );
}

#[test]
fn a_reflecting_pattern_tiles_mirrored_copies() {
    // SVG repeats a tile unchanged, so a reflecting pattern is laid down as
    // a 2x2 block of mirrored copies that then tiles normally: the block is
    // two steps wide and tall, and the copies carry a negative scale.
    let cell = sofd_display::PageDisplay {
        index: 0,
        page_box: Rect::new(0.0, 0.0, 4.0, 4.0),
        ops: vec![DrawOp::Path {
            path: rect(0.0, 0.0, 2.0, 2.0),
            fill: Some(Paint::Solid(Color::rgba(0, 128, 0, 255))),
            stroke: None,
            stroke_style: Default::default(),
            rule: PaintRule::NonZero,
        }],
        content_bbox: Rect::EMPTY,
    };
    let p = page(vec![DrawOp::Path {
        path: rect(0.0, 0.0, 20.0, 20.0),
        fill: Some(Paint::Pattern(Box::new(sofd_display::PatternSpec {
            cell: Box::new(cell),
            width: 4.0,
            height: 4.0,
            step_x: 4.0,
            step_y: 4.0,
            reflect: sofd_display::Reflect::RowAndColumn,
        }))),
        stroke: None,
        stroke_style: Default::default(),
        rule: PaintRule::NonZero,
    }]);
    let svg = page_to_svg(&p, &SvgOptions::default()).expect("convert");
    assert!(
        svg.contains("<pattern id=\"pat1\""),
        "a reflecting pattern still tiles: {svg}"
    );
    // A 2x2 block spans two steps in each direction.
    assert!(
        svg.contains("width=\"8\" height=\"8\""),
        "the block spans two steps: {svg}"
    );
    assert!(
        svg.contains("scale(-1,-1)"),
        "the diagonal copy is mirrored: {svg}"
    );
    assert!(svg.contains("fill=\"#008000\""), "the cell is drawn: {svg}");
}

#[test]
fn a_row_reflection_mirrors_across_rows_not_columns() {
    // `Row` flips left and right as the row advances, so its block is one
    // step wide and two tall; getting the axes the wrong way round tiles
    // two plain copies side by side and never mirrors at all.
    let cell = sofd_display::PageDisplay {
        index: 0,
        page_box: Rect::new(0.0, 0.0, 4.0, 4.0),
        ops: vec![DrawOp::Path {
            path: rect(0.0, 0.0, 2.0, 1.0),
            fill: Some(Paint::Solid(Color::rgba(0, 128, 0, 255))),
            stroke: None,
            stroke_style: Default::default(),
            rule: PaintRule::NonZero,
        }],
        content_bbox: Rect::EMPTY,
    };
    let p = page(vec![DrawOp::Path {
        path: rect(0.0, 0.0, 20.0, 20.0),
        fill: Some(Paint::Pattern(Box::new(sofd_display::PatternSpec {
            cell: Box::new(cell),
            width: 4.0,
            height: 4.0,
            step_x: 4.0,
            step_y: 4.0,
            reflect: sofd_display::Reflect::Row,
        }))),
        stroke: None,
        stroke_style: Default::default(),
        rule: PaintRule::NonZero,
    }]);
    let svg = page_to_svg(&p, &SvgOptions::default()).expect("convert");
    assert!(
        svg.contains("width=\"4\" height=\"8\""),
        "the block is one step wide and two tall: {svg}"
    );
    assert!(
        svg.contains("translate(4,4) scale(-1,1)"),
        "the second row sits below and is flipped left to right: {svg}"
    );
    assert!(
        !svg.contains("scale(1,-1)"),
        "the row is not flipped up and down: {svg}"
    );
}

#[test]
fn a_linear_gradient_becomes_a_def_and_a_fill() {
    let spec = LinearGradientSpec {
        start: Point::new(0.0, 0.0),
        end: Point::new(100.0, 0.0),
        stops: vec![
            GradientStop {
                offset: 0.0,
                color: Color::WHITE,
            },
            GradientStop {
                offset: 1.0,
                color: Color::BLACK,
            },
        ],
    };
    let p = page(vec![DrawOp::Path {
        path: rect(0.0, 0.0, 100.0, 50.0),
        fill: Some(Paint::LinearGradient(Box::new(spec))),
        stroke: None,
        stroke_style: Default::default(),
        rule: PaintRule::NonZero,
    }]);
    let svg = page_to_svg(&p, &SvgOptions::default()).expect("convert");
    assert!(svg.contains("<linearGradient id=\"grad1\" gradientUnits=\"userSpaceOnUse\" x1=\"0\" y1=\"0\" x2=\"100\" y2=\"0\">"));
    assert!(svg.contains("<stop offset=\"0\" stop-color=\"#ffffff\" stop-opacity=\"1\"/>"));
    assert!(svg.contains("<stop offset=\"1\" stop-color=\"#000000\" stop-opacity=\"1\"/>"));
    assert!(svg.contains("fill=\"url(#grad1)\""));
}

#[test]
fn a_radial_gradient_becomes_a_def_and_a_fill() {
    let spec = RadialGradientSpec {
        center: Point::new(10.0, 10.0),
        radius: 0.0,
        end: Point::new(10.0, 10.0),
        end_radius: 25.0,
        stops: vec![GradientStop {
            offset: 0.5,
            color: Color::WHITE,
        }],
    };
    let p = page(vec![DrawOp::Path {
        path: rect(0.0, 0.0, 20.0, 20.0),
        fill: Some(Paint::RadialGradient(Box::new(spec))),
        stroke: None,
        stroke_style: Default::default(),
        rule: PaintRule::NonZero,
    }]);
    let svg = page_to_svg(&p, &SvgOptions::default()).expect("convert");
    // The clause's start circle is a point here, so the cone reduces to
    // the plain radial SVG paints: the circle at the far end, the point
    // at the near one.
    assert!(svg.contains("<radialGradient id=\"grad1\" gradientUnits=\"userSpaceOnUse\" cx=\"10\" cy=\"10\" r=\"25\" fx=\"10\" fy=\"10\">"));
    assert!(svg.contains("fill=\"url(#grad1)\""));
}

#[test]
fn an_image_embeds_as_a_data_uri_rectangle() {
    let p = page(vec![DrawOp::Image {
        image: ImageRef::new(
            "i",
            std::sync::Arc::new(vec![1, 2, 3, 4]),
            (2, 2),
            ImageCodec::Png,
        ),
        dst: Rect::new(5.0, 6.0, 7.0, 8.0),
    }]);
    let svg = page_to_svg(&p, &SvgOptions::default()).expect("convert");
    assert!(svg.contains(
        "<image href=\"data:image/png;base64,AQIDBA==\" x=\"5\" y=\"6\" width=\"7\" height=\"8\"/>"
    ));
}

#[test]
fn live_text_uses_the_run_transform_and_size() {
    let run = GlyphRun {
        font: FontId(1),
        size: Millimeter(3.5),
        glyphs: vec![1, 2],
        advances: vec![3.5, 3.5],
        text: Some("a<b".into()),
        cluster_map: None,
        codes_are_glyphs: false,
        transform: Affine::translate(12.0, 30.0),
        glyph_angle: 0.0,
        fill: Some(Paint::Solid(Color::BLACK)),
        stroke: None,
        stroke_style: Default::default(),
        letter_spacing: 0.0,
        bbox: Rect::new(0.0, 0.0, 7.0, 3.5),
    };
    let svg = page_to_svg(&page(vec![DrawOp::Text(run)]), &SvgOptions::default()).expect("convert");
    assert!(
        svg.contains("<text x=\"12\" y=\"30\" font-size=\"3.5\" fill=\"#000000\">a&lt;b</text>")
    );
}

#[test]
fn a_rotated_run_emits_a_matrix_instead_of_x_and_y() {
    let run = GlyphRun {
        font: FontId(1),
        size: Millimeter(3.0),
        glyphs: vec![1],
        advances: vec![3.0],
        text: Some("A".into()),
        cluster_map: None,
        codes_are_glyphs: false,
        transform: Affine::translate(1.0, 2.0).then(Affine::rotate_deg(90.0)),
        glyph_angle: 0.0,
        fill: None,
        stroke: None,
        stroke_style: Default::default(),
        letter_spacing: 0.0,
        bbox: Rect::new(0.0, 0.0, 3.0, 3.0),
    };
    let svg = page_to_svg(&page(vec![DrawOp::Text(run)]), &SvgOptions::default()).expect("convert");
    // translate(1, 2) acts after the rotation, so the glyph origin lands at
    // (-2, 1) rather than (1, 2).
    assert!(svg.contains("transform=\"matrix(0 1 -1 0 -2 1)\""));
}

#[test]
fn a_run_without_text_and_without_fonts_is_counted_as_dropped() {
    let run = GlyphRun {
        font: FontId(9),
        size: Millimeter(3.0),
        glyphs: vec![1],
        advances: vec![3.0],
        text: None,
        cluster_map: None,
        codes_are_glyphs: false,
        transform: Affine::IDENTITY,
        glyph_angle: 0.0,
        fill: None,
        stroke: None,
        stroke_style: Default::default(),
        letter_spacing: 0.0,
        bbox: Rect::EMPTY,
    };
    let mut sink = SvgSink::new(Rect::A4_PORTRAIT, &SvgOptions::default());
    replay(&page(vec![DrawOp::Text(run)]), &mut sink).expect("replay");
    let (svg, stats) = sink.finish();
    assert!(!svg.contains("<text"));
    assert_eq!(stats.dropped_runs, 1);
}

#[test]
fn a_run_missing_a_glyph_is_counted_and_the_rest_still_draws() {
    // Orbitron draws 'A'; a code point far outside its coverage resolves
    // to no outline, so the run reaches the output short one mark.
    let mut manager = FontManager::new();
    manager.registry_mut().register_embedded(
        "tests/data/Orbitron-Bold.ttf",
        "Orbitron",
        std::sync::Arc::new(font_bytes()),
    );
    let run = GlyphRun {
        font: FontId(0),
        size: Millimeter(3.0),
        glyphs: vec![65, 60000],
        advances: vec![3.0, 3.0],
        text: Some("Ax".into()),
        cluster_map: None,
        codes_are_glyphs: false,
        transform: Affine::IDENTITY,
        glyph_angle: 0.0,
        fill: Some(Paint::Solid(Color::BLACK)),
        stroke: None,
        stroke_style: Default::default(),
        letter_spacing: 0.0,
        bbox: Rect::new(0.0, 0.0, 6.0, 3.0),
    };
    let options = SvgOptions {
        text: TextExport::Outline,
        fonts: Some(&manager),
    };
    let mut sink = SvgSink::new(Rect::A4_PORTRAIT, &options);
    replay(&page(vec![DrawOp::Text(run)]), &mut sink).expect("replay");
    let (svg, stats) = sink.finish();
    assert_eq!(stats.missing_glyph_runs, 1);
    assert_eq!(stats.outlined_glyphs, 1, "the drawable glyph still landed");
    assert!(!stats.is_lossless(), "a short run is not a lossless page");
    assert!(svg.contains("<path"));
}

#[test]
fn an_empty_run_emits_nothing_at_all() {
    let run = GlyphRun {
        font: FontId(1),
        size: Millimeter(3.0),
        glyphs: Vec::new(),
        advances: Vec::new(),
        text: Some(String::new()),
        cluster_map: None,
        codes_are_glyphs: false,
        transform: Affine::IDENTITY,
        glyph_angle: 0.0,
        fill: None,
        stroke: None,
        stroke_style: Default::default(),
        letter_spacing: 0.0,
        bbox: Rect::EMPTY,
    };
    let svg = page_to_svg(&page(vec![DrawOp::Text(run)]), &SvgOptions::default()).expect("convert");
    assert!(!svg.contains("<text"));
}

#[test]
fn an_unbalanced_restore_is_an_error() {
    let err = page_to_svg(&page(vec![DrawOp::Restore]), &SvgOptions::default())
        .expect_err("an unbalanced restore must fail");
    assert!(matches!(
        err,
        sofd_error::OfdError::Format(sofd_error::FormatError::CircularReference { .. })
    ));
}

#[test]
fn a_trailing_save_still_produces_balanced_markup() {
    let svg = page_to_svg(&page(vec![DrawOp::Save]), &SvgOptions::default()).expect("convert");
    assert_eq!(svg.matches("<g").count(), svg.matches("</g>").count());
}

#[test]
fn arcs_carry_their_flags_into_the_path_data() {
    let mut path = Path::new();
    path.segments.push(PathSegment::MoveTo(0.0, 0.0));
    path.segments.push(PathSegment::Arc {
        rx: 5.0,
        ry: 5.0,
        x_axis_rotation: 30.0,
        large_arc: true,
        sweep: false,
        to: (10.0, 10.0),
    });
    let svg = page_to_svg(
        &page(vec![DrawOp::Path {
            path,
            fill: None,
            stroke: None,
            stroke_style: StrokeStyle::default(),
            rule: PaintRule::NonZero,
        }]),
        &SvgOptions::default(),
    )
    .expect("convert");
    assert!(svg.contains("A5 5 30 1 0 10 10"));
}

#[test]
fn numbers_strip_trailing_zeros_and_negative_zero() {
    assert_eq!(num(2.0), "2");
    assert_eq!(num(0.5), "0.5");
    assert_eq!(num(-0.00001), "0");
    assert_eq!(num(1.0 / 3.0), "0.3333");
    assert_eq!(num(f64::NAN), "0");
}

#[test]
fn text_content_is_escaped() {
    assert_eq!(escape_text("a&b<c>d"), "a&amp;b&lt;c&gt;d");
}
