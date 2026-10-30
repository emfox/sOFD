//! Image placements used to reach the renderer as empty bytes, so every
//! document carrying a photograph, a seal or a QR code rendered with the image
//! missing and no error raised.

use sofd_common::Rect;
use sofd_display::{DrawOp, ImageCodec};
use sofd_geometry::Affine;
use sofd_package::{Package, build_archive};
use sofd_parser::{ParsedDocument, Parser};

/// The smallest archive a page can live in: entry point, document, resource
/// table, page content, and whatever the table points at.
fn archive(doc_dir: &str, media: &str, media_bytes: &[u8]) -> Vec<u8> {
    archive_named(doc_dir, media, "image.png", media_bytes)
}

/// The same, but storing the media under the name the table declares, so a
/// test can vary that name without the file going missing.
fn archive_named(doc_dir: &str, media: &str, file_name: &str, media_bytes: &[u8]) -> Vec<u8> {
    let ofd = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<OFD Version="1.0"><DocBody><DocRoot>{doc_dir}/Document.xml</DocRoot></DocBody></OFD>"#
    );
    let document = r#"<?xml version="1.0" encoding="UTF-8"?>
<Document><CommonData>
  <PageArea><PhysicalBox>0 0 210 297</PhysicalBox></PageArea>
  <DocumentRes>DocumentRes.xml</DocumentRes>
</CommonData><Pages><Page ID="1" BaseLoc="Pages/Page_0/Content.xml"/></Pages></Document>"#
        .to_string();
    let res = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<Res BaseLoc="Res"><MultiMedias>{media}</MultiMedias></Res>"#
    );
    let content = r#"<?xml version="1.0" encoding="UTF-8"?>
<Page><Content><Layer ID="1">
  <ImageObject ID="9" Boundary="10 10 50 50" ResourceID="7"/>
</Layer></Content></Page>"#
        .to_string();
    let path: &'static str = Box::leak(format!("Doc_0/Res/{file_name}").into_boxed_str());
    build_archive(&[
        ("OFD.xml", ofd.as_bytes()),
        ("Doc_0/Document.xml", document.as_bytes()),
        ("Doc_0/DocumentRes.xml", res.as_bytes()),
        ("Doc_0/Pages/Page_0/Content.xml", content.as_bytes()),
        (path, media_bytes),
    ])
    .expect("the archive builds")
}

/// A one-pixel red PNG: the smallest thing `image` will accept. Spelled out as
/// bytes so the test needs no encoder of its own to prove bytes travel intact.
const RED_PIXEL_PNG: &[u8] = &[
    137, 80, 78, 71, 13, 10, 26, 10, 0, 0, 0, 13, 73, 72, 68, 82, 0, 0, 0, 1, 0, 0, 0, 1, 8, 2, 0,
    0, 0, 144, 119, 83, 222, 0, 0, 0, 12, 73, 68, 65, 84, 120, 156, 99, 248, 207, 192, 0, 0, 3, 1,
    1, 0, 201, 254, 146, 239, 0, 0, 0, 0, 73, 69, 78, 68, 174, 66, 96, 130,
];

fn red_pixel() -> Vec<u8> {
    RED_PIXEL_PNG.to_vec()
}

/// The parser borrows its package, so each fixture leaks one; that is cheaper
/// than the lifetime plumbing every test would otherwise need.
fn open(bytes: &[u8]) -> (ParsedDocument, Parser<'static>) {
    let package = Package::from_bytes(bytes).expect("package");
    let mut parser = Parser::new(Box::leak(Box::new(package)));
    let doc = parser.open_document().expect("the document parses");
    (doc, parser)
}

fn placed_image(
    doc: &ParsedDocument,
    parser: &mut Parser<'_>,
) -> (Vec<u8>, ImageCodec, (u32, u32)) {
    let page = parser.page_display(doc, 0).expect("page display");
    let op = page
        .ops
        .iter()
        .find_map(|op| match op {
            DrawOp::Image { image, .. } => Some(image),
            _ => None,
        })
        .expect("the page places an image");
    (op.data.as_ref().clone(), op.codec, op.pixel_size)
}

/// The resource table is read and the bytes attached, which is what every
/// codec depends on: a placement used to carry a resource id and nothing else.
#[test]
fn a_declared_image_reaches_the_display_list_with_its_bytes() {
    let png = red_pixel();
    let bytes = archive(
        "Doc_0",
        r#"<MultiMedia ID="7" Type="Image"><MediaFile>image.png</MediaFile></MultiMedia>"#,
        &png,
    );
    let (doc, mut parser) = open(&bytes);
    let (data, codec, size) = placed_image(&doc, &mut parser);
    assert_eq!(data, png, "the encoded bytes travel with the placement");
    assert_eq!(codec, ImageCodec::Png);
    assert_eq!(
        size,
        (1, 1),
        "the size is read from the image, not left at 1x1"
    );
}

/// The encoding comes from the file name, because the resource table names a
/// format hint inconsistently or not at all.
#[test]
fn the_encoding_follows_the_media_file_name() {
    for (name, expected) in [
        ("image.png", ImageCodec::Png),
        ("image.PNG", ImageCodec::Png),
        ("image.jpeg", ImageCodec::Jpeg),
        ("image.gif", ImageCodec::Gif),
        ("image.GIF", ImageCodec::Gif),
        ("image.jb2", ImageCodec::Jbig2),
        ("image.JBIG2", ImageCodec::Jbig2),
        ("image.unknown", ImageCodec::Raw),
    ] {
        let png = red_pixel();
        let media = format!(
            r#"<MultiMedia ID="7" Type="Image"><MediaFile>{name}</MediaFile></MultiMedia>"#
        );
        let bytes = archive_named("Doc_0", &media, name, &png);
        let (doc, mut parser) = open(&bytes);
        let (_, codec, _) = placed_image(&doc, &mut parser);
        assert_eq!(codec, expected, "{name} must decode as {expected:?}");
    }
}

/// ZIP entry names are case-sensitive, and producers contradict themselves:
/// one stores `Doc_0/Document.xml` beside `DOC_0/Res/Image_4.JPEG`, so the
/// reference built from the first cannot reach the second without a fallback.
#[test]
fn a_media_file_is_found_when_the_directory_case_differs() {
    let png = red_pixel();
    let ofd = r#"<?xml version="1.0" encoding="UTF-8"?>
<OFD Version="1.0"><DocBody><DocRoot>Doc_0/Document.xml</DocRoot></DocBody></OFD>"#;
    let document = r#"<?xml version="1.0" encoding="UTF-8"?>
<Document><CommonData>
  <PageArea><PhysicalBox>0 0 210 297</PhysicalBox></PageArea>
  <DocumentRes>DocumentRes.xml</DocumentRes>
</CommonData><Pages><Page ID="1" BaseLoc="Pages/Page_0/Content.xml"/></Pages></Document>"#;
    let res = r#"<?xml version="1.0" encoding="UTF-8"?>
<Res BaseLoc="Res"><MultiMedias>
  <MultiMedia ID="7" Type="Image"><MediaFile>Image_7.PNG</MediaFile></MultiMedia>
</MultiMedias></Res>"#;
    let content = r#"<?xml version="1.0" encoding="UTF-8"?>
<Page><Content><Layer ID="1">
  <ImageObject ID="9" Boundary="10 10 50 50" ResourceID="7"/>
</Layer></Content></Page>"#;
    let bytes = build_archive(&[
        ("OFD.xml", ofd.as_bytes()),
        ("Doc_0/Document.xml", document.as_bytes()),
        ("Doc_0/DocumentRes.xml", res.as_bytes()),
        ("Doc_0/Pages/Page_0/Content.xml", content.as_bytes()),
        // Note the directory case: the document says Doc_0, the file says DOC_0.
        ("DOC_0/Res/Image_7.PNG", &png),
    ])
    .expect("archive");
    let (doc, mut parser) = open(&bytes);
    let (data, _, _) = placed_image(&doc, &mut parser);
    assert_eq!(data, png, "the bytes are found despite the case difference");
}

/// An exact spelling must win, or two spellings read as whichever came last.
#[test]
fn an_exact_entry_name_is_preferred_over_a_case_insensitive_match() {
    let mut package = Package::from_bytes(&archive(
        "Doc_0",
        r#"<MultiMedia ID="7" Type="Image"><MediaFile>image.png</MediaFile></MultiMedia>"#,
        &red_pixel(),
    ))
    .expect("package");
    assert!(package.contains("Doc_0/Res/image.png"));
    assert!(
        package.contains("DOC_0/RES/IMAGE.PNG"),
        "the fallback finds it too"
    );
    let exact = package.read("Doc_0/Res/image.png").expect("exact read");
    assert_eq!(exact, red_pixel());
}

/// Audio and video share the multimedia table, and their bytes are not
/// something a backend can paint.
#[test]
fn a_video_resource_is_not_treated_as_a_placed_image() {
    let bytes = archive(
        "Doc_0",
        r#"<MultiMedia ID="7" Type="Video"><MediaFile>image.png</MediaFile></MultiMedia>"#,
        &red_pixel(),
    );
    let (doc, mut parser) = open(&bytes);
    assert!(doc.images.is_empty(), "only images enter the table");
    let page = parser.page_display(&doc, 0).expect("page display");
    let placed = page
        .ops
        .iter()
        .any(|op| matches!(op, DrawOp::Image { image, .. } if !image.data.is_empty()));
    assert!(!placed, "a video never becomes a painted image");
}

/// A placement naming nothing keeps its empty bytes rather than failing the
/// page: one bad id must not lose the rest.
#[test]
fn a_placement_of_an_undeclared_resource_leaves_the_page_alone() {
    let bytes = archive("Doc_0", "", &red_pixel());
    let (doc, mut parser) = open(&bytes);
    let page = parser.page_display(&doc, 0).expect("page display");
    let placed: Vec<_> = page
        .ops
        .iter()
        .filter_map(|op| match op {
            DrawOp::Image { image, .. } => Some(image.data.len()),
            _ => None,
        })
        .collect();
    assert_eq!(placed, vec![0], "the placement survives with no bytes");
}

/// Clause 10.1: a placement maps the object's unit square onto the boundary,
/// and what the resource calls its own size does not move it.
#[test]
fn gb_t_33190_10_an_image_placement_scales_the_unit_square() {
    let png = red_pixel();
    let media = r#"<MultiMedia ID="7" Type="Image"><MediaFile>image.png</MediaFile></MultiMedia>"#;
    let bytes = archive("Doc_0", media, &png);
    let (doc, mut parser) = open(&bytes);
    let page = parser.page_display(&doc, 0).expect("page display");

    let placement = page
        .ops
        .iter()
        .position(|op| matches!(op, DrawOp::Image { .. }))
        .expect("the page places an image");
    let DrawOp::Image { dst, image } = &page.ops[placement] else {
        unreachable!()
    };
    // The fixture boundaries the object at "10 10 50 50": an origin of
    // (10, 10) and an extent of 50 by 50, matching the rectangle the boundary
    // is a sibling of elsewhere in the flattener.
    assert_eq!(
        *dst,
        Rect::new(0.0, 0.0, 1.0, 1.0),
        "the placement addresses a unit square"
    );
    // The boundary reaches the list as a transform bracketing the placement.
    let boundary = page.ops[..placement]
        .iter()
        .rev()
        .find_map(|op| match op {
            DrawOp::Transform(t) => Some(*t),
            _ => None,
        })
        .expect("the boundary reached the list ahead of the placement");
    assert_eq!(
        boundary.apply(0.0, 0.0),
        (10.0, 10.0),
        "the unit square's origin lands on the boundary's"
    );
    assert_eq!(
        boundary.apply(1.0, 1.0),
        (11.0, 11.0),
        "and its far corner lands one boundary away from that origin"
    );
    assert_eq!(image.pixel_size, (1, 1), "the bytes are read, not guessed");
}

/// Clause 10.2: the media file names the encoding, and every format a
/// document may carry has to reach the painter under its own name rather
/// than being assumed to be PNG.
#[test]
fn gb_t_33190_10_each_media_format_reaches_the_painter() {
    let png = red_pixel();
    for (name, expected) in [
        ("image.png", ImageCodec::Png),
        ("image.jpg", ImageCodec::Jpeg),
        ("image.jb2", ImageCodec::Jbig2),
    ] {
        let media = format!(
            r#"<MultiMedia ID="7" Type="Image"><MediaFile>{name}</MediaFile></MultiMedia>"#
        );
        let bytes = archive_named("Doc_0", &media, name, &png);
        let (doc, mut parser) = open(&bytes);
        let (data, codec, _) = placed_image(&doc, &mut parser);
        assert_eq!(codec, expected, "{name} names its own codec");
        assert_eq!(
            data, png,
            "and the bytes reach the painter whatever the format claims"
        );
    }
}
