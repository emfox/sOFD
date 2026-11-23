use super::*;
use sofd_common::{Millimeter, Point};
use sofd_display::MeshVertex;
use sofd_display::ops::Paint;
use sofd_font::FontId;

/// A 2x2 PNG of the given straight RGBA bytes. Written here rather than
/// by the image crate: a PDF case must compile without the raster stack.
fn png_fixture(rgba: &[u8]) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, 2, 2);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().expect("png header");
        writer.write_image_data(rgba).expect("png data");
    }
    out
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

fn page(ops: Vec<sofd_display::DrawOp>) -> sofd_display::PageDisplay {
    sofd_display::PageDisplay {
        index: 0,
        page_box: Rect::A4_PORTRAIT,
        ops,
        content_bbox: Rect::EMPTY,
    }
}

fn content_of(bytes: &[u8]) -> String {
    let doc = Document::load_from(bytes).expect("the pdf we just wrote must parse");
    let pages = doc.get_pages();
    let page_id = *pages.values().next().expect("one page");
    let content = doc.get_page_content(page_id);
    String::from_utf8(content).expect("utf8 content")
}

/// The object a page's pattern resource names.
fn pattern_object<'a>(doc: &'a Document, name: &str) -> &'a Object {
    let page_id = *doc.get_pages().values().next().expect("one page");
    let resources = doc
        .get_page_resources(page_id)
        .expect("resources")
        .0
        .expect("resource dict");
    let patterns = resources
        .get(b"Pattern")
        .expect("pattern dict")
        .as_dict()
        .expect("pattern dict");
    let id = patterns
        .get(name.as_bytes())
        .expect("the named pattern")
        .as_reference()
        .expect("a pattern is an indirect object");
    doc.get_object(id).expect("the pattern object")
}

/// The pattern dictionary a page resource names.
fn pattern_dict<'a>(doc: &'a Document, name: &str) -> &'a Dictionary {
    match pattern_object(doc, name) {
        Object::Dictionary(dict) => dict,
        _ => panic!("a shading pattern is a dictionary"),
    }
}

/// The pattern stream a page resource names, decoded by the caller.
fn pattern_stream<'a>(doc: &'a Document, name: &str) -> &'a Stream {
    match pattern_object(doc, name) {
        Object::Stream(stream) => stream,
        _ => panic!("a tiling pattern is a stream"),
    }
}

#[test]
fn a_rect_becomes_a_fill_operator_in_points() {
    // 10mm at 72/25.4 points per millimetre is 28.3465pt.
    let p = page(vec![sofd_display::DrawOp::Path {
        path: rect(10.0, 10.0, 10.0, 10.0),
        fill: Some(Paint::Solid(Color::rgba(255, 0, 0, 255))),
        stroke: None,
        stroke_style: StrokeStyle::default(),
        rule: PaintRule::NonZero,
    }]);
    let pdf = page_to_pdf(&p, &PdfOptions::default()).expect("convert");
    assert!(pdf.starts_with(b"%PDF-"));
    let content = content_of(&pdf);
    assert!(content.contains("1 0 0 rg"));
    // x runs 10..30mm; y is measured from the bottom, so 10..20mm from the
    // top of a 297mm page lands at 756.8504..813.5433 points.
    assert!(content.contains("28.3465 813.5433 m"));
    assert!(content.contains("56.6929 813.5433 l"));
    assert!(content.trim_end().ends_with("f"));
}

#[test]
fn the_y_axis_flips_into_pdf_space() {
    // A rect at the top of a 297mm page lands near the top in points.
    let p = page(vec![sofd_display::DrawOp::Path {
        path: rect(0.0, 0.0, 10.0, 10.0),
        fill: Some(Paint::Solid(Color::BLACK)),
        stroke: None,
        stroke_style: StrokeStyle::default(),
        rule: PaintRule::NonZero,
    }]);
    let content = content_of(&page_to_pdf(&p, &PdfOptions::default()).expect("convert"));
    assert!(
        content.contains("0 841.8898 m"),
        "the origin maps to the top"
    );
}

#[test]
fn the_even_odd_rule_selects_the_starred_fill() {
    let p = page(vec![sofd_display::DrawOp::Path {
        path: rect(0.0, 0.0, 10.0, 10.0),
        fill: Some(Paint::Solid(Color::BLACK)),
        stroke: None,
        stroke_style: StrokeStyle::default(),
        rule: PaintRule::EvenOdd,
    }]);
    let content = content_of(&page_to_pdf(&p, &PdfOptions::default()).expect("convert"));
    assert!(content.contains("f*"));
}

#[test]
fn a_stroke_carries_width_dash_and_joins() {
    let style = StrokeStyle {
        width: 0.5,
        dash: vec![3.0, 1.0],
        dash_offset: 0.25,
        cap: sofd_display::ops::LineCap::Round,
        join: sofd_display::ops::LineJoin::Bevel,
        miter_limit: 2.0,
    };
    let p = page(vec![sofd_display::DrawOp::Path {
        path: rect(0.0, 0.0, 10.0, 10.0),
        fill: None,
        stroke: Some(Paint::Solid(Color::BLACK)),
        stroke_style: style,
        rule: PaintRule::NonZero,
    }]);
    let content = content_of(&page_to_pdf(&p, &PdfOptions::default()).expect("convert"));
    assert!(content.contains("1.4173 w"));
    assert!(content.contains("[8.5039 2.8346] 0.7087 d"));
    assert!(content.contains("1 J 2 j 2 M"));
    assert!(content.trim_end().ends_with("S"));
}

#[test]
fn a_transform_scales_geometry_and_stroke_together() {
    let p = page(vec![
        sofd_display::DrawOp::Save,
        sofd_display::DrawOp::Transform(Affine::scale(2.0, 2.0)),
        sofd_display::DrawOp::Path {
            path: rect(0.0, 0.0, 10.0, 10.0),
            fill: None,
            stroke: Some(Paint::Solid(Color::BLACK)),
            stroke_style: StrokeStyle::default(),
            rule: PaintRule::NonZero,
        },
        sofd_display::DrawOp::Restore,
    ]);
    let content = content_of(&page_to_pdf(&p, &PdfOptions::default()).expect("convert"));
    // The rect doubles to 56.69pt along the top edge and the 1mm stroke to
    // 5.6693pt.
    assert!(content.contains("56.6929 841.8898 l"));
    assert!(content.contains("5.6693 w"));
}

#[test]
fn a_clip_opens_a_q_group_the_restore_closes() {
    let p = page(vec![
        sofd_display::DrawOp::Save,
        sofd_display::DrawOp::ClipPath {
            path: rect(0.0, 0.0, 10.0, 10.0),
            rule: PaintRule::NonZero,
        },
        sofd_display::DrawOp::Restore,
    ]);
    let content = content_of(&page_to_pdf(&p, &PdfOptions::default()).expect("convert"));
    assert!(content.contains("q "), "the clip opens a group");
    assert!(content.contains("W n"), "the path becomes a clip");
    assert!(content.contains("Q"), "the restore closes the group");
}

#[test]
fn an_even_odd_clip_uses_the_starred_operator() {
    let p = page(vec![
        sofd_display::DrawOp::Save,
        sofd_display::DrawOp::ClipPath {
            path: rect(0.0, 0.0, 10.0, 10.0),
            rule: PaintRule::EvenOdd,
        },
        sofd_display::DrawOp::Restore,
    ]);
    let content = content_of(&page_to_pdf(&p, &PdfOptions::default()).expect("convert"));
    assert!(content.contains("W* n"));
}

#[test]
fn a_linear_gradient_becomes_a_shading_pattern() {
    let p = page(vec![sofd_display::DrawOp::Path {
        path: rect(0.0, 0.0, 10.0, 10.0),
        fill: Some(sofd_display::ops::Paint::LinearGradient(Box::new(
            LinearGradientSpec {
                start: Point::new(0.0, 0.0),
                end: Point::new(10.0, 0.0),
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
            },
        ))),
        stroke: None,
        stroke_style: Default::default(),
        rule: PaintRule::NonZero,
    }]);
    let bytes = page_to_pdf(&p, &PdfOptions::default()).expect("convert");
    let doc = Document::load_from(&bytes[..]).expect("parse");
    let page_id = *doc.get_pages().values().next().expect("page");
    let resources = doc.get_page_resources(page_id).expect("resources");
    let patterns = resources
        .0
        .expect("pattern dict")
        .get(b"Pattern")
        .expect("patterns");
    assert!(
        patterns.as_dict().is_ok(),
        "a shading pattern is registered"
    );
    let content = content_of(&bytes);
    // The path is its own clip for a pattern colour, so the fill needs no
    // `W n` group and a stroke that follows is not cut in half.
    assert!(content.contains("/Pattern cs /Sh0 scn f"), "{content}");
    assert!(!content.contains("W n"), "{content}");
}

#[test]
fn a_pattern_becomes_a_tiling_pattern_stream() {
    // The cell is content of its own: a PDF tiling pattern repeats it, so
    // the fill keeps the stripes instead of flattening them to one colour.
    let cell = sofd_display::PageDisplay {
        index: 0,
        page_box: Rect::new(0.0, 0.0, 4.0, 4.0),
        ops: vec![sofd_display::DrawOp::Path {
            path: rect(0.0, 0.0, 4.0, 4.0),
            fill: Some(sofd_display::ops::Paint::Solid(Color::rgba(255, 0, 0, 255))),
            stroke: None,
            stroke_style: Default::default(),
            rule: PaintRule::NonZero,
        }],
        content_bbox: Rect::EMPTY,
    };
    let p = page(vec![sofd_display::DrawOp::Path {
        path: rect(0.0, 0.0, 20.0, 20.0),
        fill: Some(sofd_display::ops::Paint::Pattern(Box::new(
            sofd_display::PatternSpec {
                cell: Box::new(cell),
                width: 4.0,
                height: 4.0,
                step_x: 4.0,
                step_y: 4.0,
                reflect: sofd_display::Reflect::Normal,
            },
        ))),
        stroke: None,
        stroke_style: Default::default(),
        rule: PaintRule::NonZero,
    }]);
    let (bytes, stats) = page_to_pdf_with_stats(&p, &PdfOptions::default()).expect("convert");
    let content = content_of(&bytes);
    assert!(content.contains("/Pattern cs /P0 scn f"), "{content}");
    let doc = Document::load_from(&bytes[..]).expect("parse");
    let stream = pattern_stream(&doc, "P0");
    assert_eq!(
        stream.dict.get(b"PatternType").unwrap().as_i64().unwrap(),
        1,
        "a tiling pattern, not a shading"
    );
    assert_eq!(
        stream.dict.get(b"XStep").unwrap().as_float().unwrap(),
        (4.0 * PT_PER_MM) as f32,
    );
    let body = String::from_utf8(stream.decompressed_content().expect("decode")).expect("utf8");
    assert!(
        body.contains("1 0 0 rg"),
        "the cell keeps its own paint: {body}"
    );
    assert!(
        body.contains("0 11.3386 m"),
        "the cell is in its own points: {body}"
    );
    assert_eq!(stats.paints_approximated, 0, "nothing was simplified");
    assert_eq!(stats.gradients, 0, "a pattern is not a gradient");
}

#[test]
fn a_pattern_cell_bleeds_past_its_box_so_repeats_join() {
    // A cell clipped exactly at its edge leaves a hairline at every repeat:
    // each tile's antialiased edge blends with the page, not the neighbour.
    // The box reaches past the cell, and the path fill still clips it.
    let cell = sofd_display::PageDisplay {
        index: 0,
        page_box: Rect::new(0.0, 0.0, 4.0, 4.0),
        ops: vec![sofd_display::DrawOp::Path {
            path: rect(0.0, 0.0, 4.0, 4.0),
            fill: Some(sofd_display::ops::Paint::Solid(Color::BLACK)),
            stroke: None,
            stroke_style: Default::default(),
            rule: PaintRule::NonZero,
        }],
        content_bbox: Rect::EMPTY,
    };
    let p = page(vec![sofd_display::DrawOp::Path {
        path: rect(0.0, 0.0, 20.0, 20.0),
        fill: Some(sofd_display::ops::Paint::Pattern(Box::new(
            sofd_display::PatternSpec {
                cell: Box::new(cell),
                width: 4.0,
                height: 4.0,
                step_x: 4.0,
                step_y: 4.0,
                reflect: sofd_display::Reflect::Normal,
            },
        ))),
        stroke: None,
        stroke_style: Default::default(),
        rule: PaintRule::NonZero,
    }]);
    let bytes = page_to_pdf(&p, &PdfOptions::default()).expect("convert");
    let doc = Document::load_from(&bytes[..]).expect("parse");
    let bbox: Vec<f32> = pattern_stream(&doc, "P0")
        .dict
        .get(b"BBox")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_f32().ok())
        .collect();
    assert!(
        bbox[0] < 0.0 && bbox[1] < 0.0,
        "the box starts early: {bbox:?}"
    );
    assert!(
        bbox[2] > (4.0 * PT_PER_MM) as f32 && bbox[3] > (4.0 * PT_PER_MM) as f32,
        "the box reaches past the cell: {bbox:?}"
    );
}

#[test]
fn a_stroke_is_painted_with_the_same_pattern_colour() {
    // A gradient on a stroke used to be written black, because only a fill
    // had an operator that could name a pattern.
    let p = page(vec![sofd_display::DrawOp::Path {
        path: rect(1.0, 1.0, 10.0, 10.0),
        fill: None,
        stroke: Some(sofd_display::ops::Paint::LinearGradient(Box::new(
            LinearGradientSpec {
                start: Point::new(0.0, 0.0),
                end: Point::new(10.0, 0.0),
                stops: vec![GradientStop {
                    offset: 0.0,
                    color: Color::WHITE,
                }],
            },
        ))),
        stroke_style: Default::default(),
        rule: PaintRule::NonZero,
    }]);
    let content = content_of(&page_to_pdf(&p, &PdfOptions::default()).expect("convert"));
    assert!(content.contains("/Pattern CS /Sh0 SCN"), "{content}");
    assert!(content.trim_end().ends_with('S'), "{content}");
    assert!(
        !content.contains("0 0 0 RG"),
        "no black stand-in: {content}"
    );
}

#[test]
fn a_filled_and_stroked_path_is_not_clipped_to_its_fill() {
    // The fill used to open a `W n` group, which cut the outer half of the
    // stroke away; the path itself is the clip a pattern fill needs.
    let p = page(vec![sofd_display::DrawOp::Path {
        path: rect(0.0, 0.0, 10.0, 10.0),
        fill: Some(sofd_display::ops::Paint::LinearGradient(Box::new(
            LinearGradientSpec {
                start: Point::new(0.0, 0.0),
                end: Point::new(10.0, 0.0),
                stops: vec![GradientStop {
                    offset: 0.0,
                    color: Color::WHITE,
                }],
            },
        ))),
        stroke: Some(sofd_display::ops::Paint::Solid(Color::BLACK)),
        stroke_style: Default::default(),
        rule: PaintRule::NonZero,
    }]);
    let content = content_of(&page_to_pdf(&p, &PdfOptions::default()).expect("convert"));
    assert!(!content.contains("W n"), "no clip group: {content}");
    assert!(content.contains("0 0 0 RG"), "the stroke keeps its colour");
}

#[test]
fn a_reflecting_pattern_repeats_a_mirrored_block() {
    // PDF tiles a cell unchanged, so the mirror has to be part of the
    // repeated block: the step doubles and the second row is flipped.
    let cell = sofd_display::PageDisplay {
        index: 0,
        page_box: Rect::new(0.0, 0.0, 4.0, 4.0),
        ops: vec![sofd_display::DrawOp::Path {
            path: rect(0.0, 0.0, 4.0, 4.0),
            fill: Some(sofd_display::ops::Paint::Solid(Color::BLACK)),
            stroke: None,
            stroke_style: Default::default(),
            rule: PaintRule::NonZero,
        }],
        content_bbox: Rect::EMPTY,
    };
    let p = page(vec![sofd_display::DrawOp::Path {
        path: rect(0.0, 0.0, 20.0, 20.0),
        fill: Some(sofd_display::ops::Paint::Pattern(Box::new(
            sofd_display::PatternSpec {
                cell: Box::new(cell),
                width: 4.0,
                height: 4.0,
                step_x: 4.0,
                step_y: 4.0,
                reflect: sofd_display::Reflect::Row,
            },
        ))),
        stroke: None,
        stroke_style: Default::default(),
        rule: PaintRule::NonZero,
    }]);
    let bytes = page_to_pdf(&p, &PdfOptions::default()).expect("convert");
    let doc = Document::load_from(&bytes[..]).expect("parse");
    let stream = pattern_stream(&doc, "P0");
    assert_eq!(
        stream.dict.get(b"YStep").unwrap().as_float().unwrap(),
        (8.0 * PT_PER_MM) as f32,
        "two rows per block"
    );
    let body = String::from_utf8(stream.decompressed_content().expect("decode")).expect("utf8");
    assert!(
        body.contains("q -1 0 0 1"),
        "the second row is mirrored: {body}"
    );
}

#[test]
fn a_radial_gradient_uses_shading_type_three() {
    let p = page(vec![sofd_display::DrawOp::Path {
        path: rect(0.0, 0.0, 10.0, 10.0),
        fill: Some(sofd_display::ops::Paint::RadialGradient(Box::new(
            RadialGradientSpec {
                center: Point::new(5.0, 5.0),
                radius: 1.0,
                end: Point::new(8.0, 5.0),
                end_radius: 5.0,
                stops: vec![GradientStop {
                    offset: 0.0,
                    color: Color::WHITE,
                }],
            },
        ))),
        stroke: None,
        stroke_style: Default::default(),
        rule: PaintRule::NonZero,
    }]);
    let bytes = page_to_pdf(&p, &PdfOptions::default()).expect("convert");
    let doc = Document::load_from(&bytes[..]).expect("parse");
    let pattern = pattern_dict(&doc, "Sh0");
    assert_eq!(
        pattern.get(b"PatternType").unwrap().as_i64().unwrap(),
        2,
        "a shading pattern"
    );
    let shading = pattern
        .get(b"Shading")
        .expect("the shading")
        .as_dict()
        .expect("an inline shading dictionary");
    assert_eq!(
        shading.get(b"ShadingType").unwrap().as_i64().unwrap(),
        3,
        "a radial shading is type 3"
    );
    // Both circles travel: PDF's type 3 is the cone the clause describes,
    // so the two centres and both radii reach the shading as written.
    let coords: Vec<f32> = shading
        .get(b"Coords")
        .unwrap()
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|v| v.as_f32().ok())
        .collect();
    assert_eq!(coords.len(), 6, "two circles, each a centre and a radius");
    let pt = (72.0 / 25.4) as f32;
    assert!((coords[2] - pt).abs() < 1e-3, "start radius: {:?}", coords);
    assert!(
        (coords[5] - 5.0 * pt).abs() < 1e-3,
        "end radius: {:?}",
        coords
    );
    assert!(
        coords[0] < coords[3],
        "the centres stay apart: {:?}",
        coords
    );
}

#[test]
fn a_gouraud_mesh_becomes_a_type_four_shading() {
    // PDF's ShadingType 4 is a free-form triangle mesh with a colour at
    // every vertex, which is the clause's model, so a mesh reaches the
    // file with its vertices rather than collapsing to one mean colour.
    let p = page(vec![sofd_display::DrawOp::Path {
        path: rect(0.0, 0.0, 10.0, 10.0),
        fill: Some(sofd_display::ops::Paint::Mesh(Box::new(MeshSpec {
            points: vec![
                MeshVertex {
                    x: 0.0,
                    y: 0.0,
                    color: Color::rgba(255, 0, 0, 255),
                },
                MeshVertex {
                    x: 10.0,
                    y: 0.0,
                    color: Color::rgba(0, 0, 255, 255),
                },
                MeshVertex {
                    x: 0.0,
                    y: 10.0,
                    color: Color::rgba(0, 255, 0, 255),
                },
            ],
            triangles: vec![[0, 1, 2]],
        }))),
        stroke: None,
        stroke_style: Default::default(),
        rule: PaintRule::NonZero,
    }]);
    let bytes = page_to_pdf(&p, &PdfOptions::default()).expect("convert");
    let doc = Document::load_from(&bytes[..]).expect("parse");
    let pattern = pattern_dict(&doc, "Sh0");
    let id = pattern
        .get(b"Shading")
        .expect("the shading")
        .as_reference()
        .expect("a free-form shading is a stream");
    let stream = match doc.get_object(id).expect("the shading object") {
        Object::Stream(stream) => stream,
        _ => panic!("a type 4 shading is a stream"),
    };
    let kind = stream
        .dict
        .get(b"ShadingType")
        .and_then(|t| t.as_i64())
        .ok();
    assert_eq!(kind, Some(4), "a mesh is a free-form triangle shading");
    // The three vertex colours travel: red, blue and green, in order.
    let data = stream.content.clone();
    // The stream is the raw bytes the format asks for: red, blue and
    // green, one per vertex, each a triple.
    let has = |tri: [u8; 3]| data.windows(3).any(|w| w == tri);
    assert!(has([0xff, 0x00, 0x00]), "the first vertex keeps its red");
    assert!(has([0x00, 0x00, 0xff]), "the second vertex keeps its blue");
    assert!(has([0x00, 0xff, 0x00]), "the third vertex keeps its green");
}

#[test]
fn a_multi_stop_gradient_stitches_its_intervals() {
    let p = page(vec![sofd_display::DrawOp::Path {
        path: rect(0.0, 0.0, 10.0, 10.0),
        fill: Some(sofd_display::ops::Paint::LinearGradient(Box::new(
            LinearGradientSpec {
                start: Point::new(0.0, 0.0),
                end: Point::new(10.0, 0.0),
                stops: vec![
                    GradientStop {
                        offset: 0.0,
                        color: Color::WHITE,
                    },
                    GradientStop {
                        offset: 0.5,
                        color: Color::rgba(255, 0, 0, 255),
                    },
                    GradientStop {
                        offset: 1.0,
                        color: Color::BLACK,
                    },
                ],
            },
        ))),
        stroke: None,
        stroke_style: Default::default(),
        rule: PaintRule::NonZero,
    }]);
    let bytes = page_to_pdf(&p, &PdfOptions::default()).expect("convert");
    let doc = Document::load_from(&bytes[..]).expect("parse");
    let stitched = doc.objects.values().any(|o| {
        o.as_dict()
            .ok()
            .and_then(|d| d.get(b"FunctionType").ok())
            .and_then(|t| t.as_i64().ok())
            == Some(3)
    });
    assert!(stitched, "three stops stitch two intervals");
}

/// A 100x100 QR code as an embedded JBIG2 stream: the encoding a scanned
/// invoice carries, which `image` cannot read. Spelled out as bytes so the
/// test needs no encoder of its own and no fixture outside the crate.
const QR_JBIG2: &[u8] = &[
    151, 74, 66, 50, 13, 10, 26, 10, 1, 0, 0, 0, 1, 0, 0, 0, 0, 48, 0, 1, 0, 0, 0, 19, 0, 0, 0,
    100, 0, 0, 0, 100, 0, 0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 1, 38, 0, 1, 0, 0, 1, 57, 0, 0, 0,
    100, 0, 0, 0, 100, 0, 0, 0, 0, 0, 0, 0, 0, 0, 8, 3, 255, 253, 255, 2, 254, 254, 254, 4, 40,
    191, 174, 189, 134, 171, 6, 215, 182, 1, 249, 238, 232, 235, 4, 204, 6, 130, 37, 10, 182, 77,
    3, 245, 98, 87, 2, 254, 86, 139, 187, 66, 236, 122, 204, 117, 189, 217, 54, 21, 169, 173, 42,
    178, 128, 197, 216, 20, 219, 121, 130, 14, 98, 9, 144, 181, 233, 16, 85, 223, 68, 18, 216, 41,
    109, 137, 37, 76, 36, 148, 110, 93, 155, 243, 164, 108, 208, 105, 107, 2, 195, 242, 185, 115,
    54, 185, 146, 48, 189, 120, 95, 84, 6, 177, 121, 95, 145, 203, 129, 254, 155, 101, 188, 30,
    204, 212, 119, 62, 84, 118, 154, 193, 73, 255, 90, 180, 122, 97, 52, 191, 105, 101, 115, 219,
    172, 186, 204, 65, 120, 24, 48, 200, 1, 199, 107, 203, 156, 118, 216, 69, 144, 2, 158, 236,
    196, 91, 247, 56, 228, 135, 246, 202, 29, 115, 168, 151, 197, 177, 152, 165, 30, 150, 114, 8,
    42, 84, 19, 94, 150, 56, 246, 131, 15, 102, 57, 26, 24, 168, 211, 147, 184, 94, 245, 84, 92,
    189, 192, 95, 125, 216, 184, 11, 177, 216, 204, 12, 154, 241, 124, 234, 55, 44, 90, 253, 120,
    167, 221, 158, 187, 168, 217, 25, 72, 146, 90, 194, 149, 37, 2, 43, 230, 196, 2, 16, 105, 110,
    63, 168, 76, 175, 112, 34, 165, 76, 79, 112, 84, 74, 136, 158, 166, 235, 30, 115, 184, 164, 2,
    252, 54, 166, 59, 102, 4, 127, 33, 159, 11, 31, 248, 71, 62, 86, 98, 223, 249, 173, 215, 167,
    187, 39, 119, 44, 133, 34, 171, 132, 153, 131, 206, 128, 64, 99, 226, 63, 255, 172, 0, 0, 0, 2,
    49, 0, 1, 0, 0, 0, 0, 0, 0, 0, 3, 51, 0, 1, 0, 0, 0, 0,
];

#[test]
fn a_jbig2_image_becomes_an_xobject_of_its_own_pixels() {
    // `image` reads no JBIG2, so the bytes used to fail the whole document
    // here while the raster backend painted them; both now decode the same
    // way, and the XObject carries the dimensions the stream declares.
    let p = page(vec![sofd_display::DrawOp::Image {
        image: ImageRef::new(
            "qr",
            std::sync::Arc::new(QR_JBIG2.to_vec()),
            (100, 100),
            sofd_display::ImageCodec::Jbig2,
        ),
        dst: Rect::new(20.0, 20.0, 50.0, 50.0),
    }]);
    let bytes = page_to_pdf(&p, &PdfOptions::default()).expect("convert");
    let doc = Document::load_from(&bytes[..]).expect("parse");
    let page_id = *doc.get_pages().values().next().expect("page");
    let images = doc.get_page_images(page_id).expect("images");
    assert_eq!(images.len(), 1, "the bi-level image becomes one xobject");
    assert_eq!(images[0].width, 100);
    assert_eq!(images[0].height, 100);
}

#[test]
fn an_image_becomes_an_xobject_drawn_with_a_matrix() {
    let png = png_fixture(&[10, 20, 30, 255].repeat(4));
    let p = page(vec![sofd_display::DrawOp::Image {
        image: ImageRef::new(
            "i",
            std::sync::Arc::new(png),
            (2, 2),
            sofd_display::ImageCodec::Png,
        ),
        dst: Rect::new(5.0, 5.0, 10.0, 10.0),
    }]);
    let bytes = page_to_pdf(&p, &PdfOptions::default()).expect("convert");
    let doc = Document::load_from(&bytes[..]).expect("parse");
    let page_id = *doc.get_pages().values().next().expect("page");
    let images = doc.get_page_images(page_id).expect("images");
    assert_eq!(images.len(), 1, "one image xobject");
    let content = content_of(&bytes);
    assert!(content.contains("/Im0 Do"));
    // The placement is 10 mm square whatever the image's pixel count is:
    // `Do` paints across the unit square. The height is negative before the
    // page flip because the image's space starts at its top-left corner.
    assert!(
        content.contains("28.3465 0 0 28.3465 14.1732 799.3701 cm"),
        "{content}"
    );
}

#[test]
fn live_text_positions_each_character_with_its_own_matrix() {
    let run = GlyphRun {
        font: FontId(1),
        size: Millimeter(10.0),
        glyphs: vec![1, 2],
        advances: vec![4.0, 4.0],
        text: Some("ab".into()),
        cluster_map: None,
        codes_are_glyphs: false,
        transform: Affine::translate(20.0, 30.0),
        glyph_angle: 0.0,
        fill: Some(Paint::Solid(Color::BLACK)),
        stroke: None,
        stroke_style: Default::default(),
        letter_spacing: 0.0,
        bbox: Rect::new(0.0, 0.0, 8.0, 10.0),
    };
    let content = content_of(
        &page_to_pdf(
            &page(vec![sofd_display::DrawOp::Text(run)]),
            &PdfOptions::default(),
        )
        .expect("convert"),
    );
    assert!(content.contains("BT /F1 28.3465 Tf"));
    assert!(content.contains("1 0 0 1 56.6929 756.8504 Tm (a) Tj"));
    assert!(content.contains("1 0 0 1 68.0315 756.8504 Tm (b) Tj"));
    assert!(content.contains("ET"));
}

#[test]
fn parentheses_in_text_are_escaped() {
    assert_eq!(pdf_string("a(b)c\\d"), "a\\(b\\)c\\\\d");
    assert_eq!(pdf_string("line\nbreak"), "line\\nbreak");
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
    let mut sink = PdfSink::new(Rect::A4_PORTRAIT, &PdfOptions::default());
    replay(&page(vec![sofd_display::DrawOp::Text(run)]), &mut sink).expect("replay");
    assert_eq!(sink.stats().dropped_runs, 1);
    assert!(!sink.ops.contains("BT"), "nothing was emitted");
}

#[test]
fn a_run_missing_a_glyph_is_counted_and_not_dropped() {
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
    let options = PdfOptions {
        text: TextExport::Outline,
        fonts: Some(&manager),
        ..PdfOptions::default()
    };
    let mut sink = PdfSink::new(Rect::A4_PORTRAIT, &options);
    replay(&page(vec![sofd_display::DrawOp::Text(run)]), &mut sink).expect("replay");
    let stats = sink.stats();
    assert_eq!(stats.missing_glyph_runs, 1);
    assert_eq!(stats.outlined_glyphs, 1, "the drawable glyph still landed");
    assert_eq!(stats.dropped_runs, 0, "the run did reach the output");
    assert!(!stats.is_lossless(), "a short run is not a lossless page");
}

/// Outlined text in PDF is a path like any other, so a stroked run writes
/// its colour and its width and is stroked rather than filled.
#[test]
fn an_outlined_run_writes_its_stroke_state() {
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
        fill: Some(Paint::Solid(Color::BLACK)),
        stroke: Some(Paint::Solid(Color::rgba(255, 0, 0, 255))),
        stroke_style: StrokeStyle {
            width: 1.0,
            ..StrokeStyle::default()
        },
        letter_spacing: 0.0,
        bbox: Rect::new(0.0, 0.0, 5.0, 5.0),
    };
    let options = PdfOptions {
        text: TextExport::Outline,
        fonts: Some(&manager),
        ..PdfOptions::default()
    };
    let p = page(vec![sofd_display::DrawOp::Text(run)]);
    let (bytes, stats) = page_to_pdf_with_stats(&p, &options).expect("convert");
    let content = content_of(&bytes);
    assert!(content.contains("1 0 0 rg"), "stroke colour, got {content}");
    // One millimetre is 72/25.4 points, and the width goes with it.
    assert!(content.contains("2.8346 w"), "stroke width, got {content}");
    assert!(
        content.contains(" S"),
        "the glyph is stroked, got {content}"
    );
    assert_eq!(stats.stroke_only_glyph_runs, 0);
}

/// A stroke-only run keeps its outline: nothing fills it, but every viewer
/// strokes it, so the text is still on the page.
#[test]
fn a_stroke_only_run_is_still_drawn() {
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
        text: None,
        cluster_map: None,
        codes_are_glyphs: false,
        transform: Affine::IDENTITY,
        glyph_angle: 0.0,
        fill: None,
        stroke: Some(Paint::Solid(Color::BLACK)),
        stroke_style: StrokeStyle::default(),
        letter_spacing: 0.0,
        bbox: Rect::new(0.0, 0.0, 5.0, 5.0),
    };
    let options = PdfOptions {
        text: TextExport::Outline,
        fonts: Some(&manager),
        ..PdfOptions::default()
    };
    let mut sink = PdfSink::new(Rect::A4_PORTRAIT, &options);
    replay(&page(vec![sofd_display::DrawOp::Text(run)]), &mut sink).expect("replay");
    let stats = sink.stats();
    assert_eq!(stats.outlined_glyphs, 1);
    assert_eq!(stats.dropped_runs, 0);
    assert_eq!(stats.stroke_only_glyph_runs, 1);
    assert!(!stats.is_lossless());
}

#[test]
fn a_translucent_fill_registers_one_extgstate() {
    let p = page(vec![
        sofd_display::DrawOp::Path {
            path: rect(0.0, 0.0, 10.0, 10.0),
            fill: Some(Paint::Solid(Color::rgba(0, 0, 0, 128))),
            stroke: None,
            stroke_style: StrokeStyle::default(),
            rule: PaintRule::NonZero,
        },
        sofd_display::DrawOp::Path {
            path: rect(20.0, 20.0, 10.0, 10.0),
            fill: Some(Paint::Solid(Color::rgba(0, 0, 0, 128))),
            stroke: None,
            stroke_style: StrokeStyle::default(),
            rule: PaintRule::NonZero,
        },
    ]);
    let bytes = page_to_pdf(&p, &PdfOptions::default()).expect("convert");
    let doc = Document::load_from(&bytes[..]).expect("parse");
    let page_id = *doc.get_pages().values().next().expect("page");
    // The graphics states live inline in the page resources, so count them
    // there rather than among the top level objects.
    let resources = doc.get_page_resources(page_id).expect("resources");
    let states = resources
        .0
        .expect("resource dict")
        .get(b"ExtGState")
        .expect("extgstate dict")
        .as_dict()
        .expect("state dict");
    assert_eq!(states.len(), 1, "one alpha, one state");
    let content = content_of(&bytes);
    assert!(content.contains("/GS0 gs"));
}

#[test]
fn an_unbalanced_restore_is_an_error() {
    let err = page_to_pdf(
        &page(vec![sofd_display::DrawOp::Restore]),
        &PdfOptions::default(),
    )
    .expect_err("an unbalanced restore must fail");
    assert!(matches!(
        err,
        OfdError::Format(sofd_error::FormatError::CircularReference { .. })
    ));
}

#[test]
fn a_trailing_save_still_writes_a_balanced_document() {
    let bytes = page_to_pdf(
        &page(vec![sofd_display::DrawOp::Save]),
        &PdfOptions::default(),
    )
    .expect("convert");
    let doc = Document::load_from(&bytes[..]).expect("parse");
    assert_eq!(doc.get_pages().len(), 1);
}

#[test]
fn an_arc_is_flattened_into_cubics() {
    let mut path = Path::new();
    path.segments.push(PathSegment::MoveTo(0.0, 0.0));
    path.segments.push(PathSegment::Arc {
        rx: 5.0,
        ry: 5.0,
        x_axis_rotation: 0.0,
        large_arc: false,
        sweep: true,
        to: (10.0, 0.0),
    });
    let p = page(vec![sofd_display::DrawOp::Path {
        path,
        fill: Some(Paint::Solid(Color::BLACK)),
        stroke: None,
        stroke_style: StrokeStyle::default(),
        rule: PaintRule::NonZero,
    }]);
    let content = content_of(&page_to_pdf(&p, &PdfOptions::default()).expect("convert"));
    assert!(content.contains(" c"), "arcs flatten to cubic operators");
    assert!(!content.contains('A'), "no arc operator survives");
}

#[test]
fn numbers_strip_trailing_zeros_and_negative_zero() {
    assert_eq!(num(2.0), "2");
    assert_eq!(num(0.5), "0.5");
    assert_eq!(num(-0.00001), "0");
    assert_eq!(num(f64::NAN), "0");
}

#[test]
fn several_pages_become_one_file_in_the_order_given() {
    let first = page(vec![sofd_display::DrawOp::Path {
        path: rect(0.0, 0.0, 10.0, 10.0),
        fill: Some(Paint::Solid(Color::rgba(255, 0, 0, 255))),
        stroke: None,
        stroke_style: StrokeStyle::default(),
        rule: PaintRule::NonZero,
    }]);
    let second = page(vec![sofd_display::DrawOp::Path {
        path: rect(0.0, 0.0, 10.0, 10.0),
        fill: Some(Paint::Solid(Color::rgba(0, 0, 255, 255))),
        stroke: None,
        stroke_style: StrokeStyle::default(),
        rule: PaintRule::NonZero,
    }]);
    let (bytes, _) = pages_to_pdf(&[&first, &second], &PdfOptions::default()).expect("convert");
    let doc = Document::load_from(&bytes[..]).expect("parse");
    assert_eq!(doc.get_pages().len(), 2, "the /Pages tree counts both");
    let catalog = doc.catalog().expect("catalog");
    assert!(catalog.get(b"Pages").is_ok(), "the catalog roots the tree");
}

#[test]
fn a_repeated_page_is_referenced_twice_rather_than_written_twice() {
    let p = page(vec![sofd_display::DrawOp::Path {
        path: rect(0.0, 0.0, 10.0, 10.0),
        fill: Some(Paint::Solid(Color::BLACK)),
        stroke: None,
        stroke_style: StrokeStyle::default(),
        rule: PaintRule::NonZero,
    }]);
    let (bytes, _) = pages_to_pdf(&[&p, &p], &PdfOptions::default()).expect("convert");
    let doc = Document::load_from(&bytes[..]).expect("parse");
    // `get_pages` walks Kids, so a repeated id appears twice even though
    // the object it names was written once.
    assert_eq!(doc.get_pages().len(), 2, "listed twice");
    let count = doc
        .catalog()
        .and_then(|c| c.get(b"Pages"))
        .and_then(|p| doc.get_dictionary(p.as_reference().expect("a reference")))
        .and_then(|p| p.get(b"Count"))
        .and_then(Object::as_i64)
        .expect("count");
    assert_eq!(count, 2, "the tree still reads as two pages");
}
