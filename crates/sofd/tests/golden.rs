//! The golden-image suite: a document's exact pixels, committed as a PNG.
//! Every other suite asserts the ops a page emits, so a backend that paints
//! them wrong still passes. Set `BLESS=1` to rewrite the references.

use std::path::PathBuf;

use image::RgbaImage;
use sofd_render::RenderOptions;
use sofd_testkit::{DOCS, GoldenDoc, golden_archive};

/// One pixel channel this far off is still the same picture: anti-aliasing and
/// the odd rounding difference must not make a golden test flaky.
const CHANNEL_TOLERANCE: i32 = 4;
/// How much of the page may differ before it is a regression. A one-pixel
/// border shifting should not fail; a lost clip or a flat gradient should.
const MAX_DIFFERING_RATIO: f64 = 0.005;

/// Where the committed references live, beside the fixtures the other suites use.
fn reference_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/golden")
}

/// How a golden document is rendered: the default DPI, no background fill so
/// the comparison sees only what the page painted, and the default budget.
const RENDER_OPTIONS: RenderOptions = RenderOptions {
    dpi: 96.0,
    background: None,
    max_pixels: 64 * 1024 * 1024,
    window: None,
};

/// Render one golden document to PNG bytes, through the same path a caller uses.
fn render(doc: &GoldenDoc) -> Vec<u8> {
    let bytes = golden_archive(doc);
    let document = sofd::Document::open_bytes(&bytes).expect("open the golden archive");
    document
        .to_png(0, &RENDER_OPTIONS)
        .unwrap_or_else(|e| panic!("render {}: {e}", doc.name))
}

/// Decode a PNG to its raw RGBA pixels, failing loudly on a bad reference.
fn decode(png: &[u8], what: &str) -> RgbaImage {
    image::load_from_memory(png)
        .unwrap_or_else(|e| panic!("decode {what}: {e}"))
        .to_rgba8()
}

/// The share of pixels that differ by more than the channel tolerance.
fn difference_ratio(expected: &RgbaImage, actual: &RgbaImage) -> f64 {
    if expected.dimensions() != actual.dimensions() {
        return 1.0;
    }
    let total = (expected.width() * expected.height()) as f64;
    let mut differing = 0u64;
    for (e, a) in expected.pixels().zip(actual.pixels()) {
        let off =
            e.0.iter()
                .zip(a.0.iter())
                .map(|(x, y)| (*x as i32 - *y as i32).abs())
                .max()
                .unwrap_or(0);
        if off > CHANNEL_TOLERANCE {
            differing += 1;
        }
    }
    differing as f64 / total
}

/// Compare one document against its reference, writing it when `BLESS` is set.
fn check_one(doc: &GoldenDoc) {
    let actual_png = render(doc);
    let reference = reference_dir().join(format!("{}.png", doc.name));

    if std::env::var_os("BLESS").is_some() {
        std::fs::create_dir_all(reference_dir()).expect("create the reference directory");
        std::fs::write(&reference, &actual_png).expect("write the reference");
        return;
    }

    let expected_png = std::fs::read(&reference).unwrap_or_else(|e| {
        panic!(
            "read the reference {}: {e}\nRun with BLESS=1 to write it.",
            reference.display()
        )
    });
    let expected = decode(&expected_png, "the reference");
    let actual = decode(&actual_png, "the rendering");

    let ratio = difference_ratio(&expected, &actual);
    assert!(
        ratio <= MAX_DIFFERING_RATIO,
        "{} differs from its reference in {:.2}% of pixels (allowed {:.2}%): \
         the backend drifted, or the reference needs a deliberate `BLESS=1`",
        doc.name,
        ratio * 100.0,
        MAX_DIFFERING_RATIO * 100.0,
    );
}

#[test]
fn every_golden_document_matches_its_committed_pixels() {
    for doc in DOCS {
        check_one(doc);
    }
}

#[test]
fn the_golden_documents_render_to_distinct_pictures() {
    // A comparator that passed everything would pass a suite where all three
    // documents rendered the same. This asserts they do not, so the reference
    // set is known to be discriminating rather than merely green.
    let rendered: Vec<RgbaImage> = DOCS
        .iter()
        .map(|doc| decode(&render(doc), doc.name))
        .collect();
    for (i, left) in rendered.iter().enumerate() {
        for (j, right) in rendered.iter().enumerate().skip(i + 1) {
            let ratio = difference_ratio(left, right);
            assert!(
                ratio > MAX_DIFFERING_RATIO,
                "{} and {} render alike ({:.2}%), so the set cannot catch a regression",
                DOCS[i].name,
                DOCS[j].name,
                ratio * 100.0,
            );
        }
    }
}

#[test]
fn a_reference_png_is_readable_at_the_expected_size() {
    // A 100x60mm page at 96 DPI is 378x227 pixels and a 297x100mm one is
    // 1123x378, both rounded up by `size_px`. Asserting the pair keeps the
    // size path itself under test, now that the fixtures no longer share a box.
    for (doc, want) in [(&DOCS[0], (378, 227)), (&DOCS[8], (1123, 378))] {
        let actual = decode(&render(doc), doc.name);
        assert_eq!(
            (actual.width(), actual.height()),
            want,
            "{} is laid out at the wrong size",
            doc.name
        );
    }
}

#[test]
fn the_book_holds_every_fixture_as_a_page() {
    // One file a third-party renderer can be handed once. If its pages drift
    // from the per-fixture documents the reference set is comparing something
    // no oracle ever saw.
    let bytes = sofd_testkit::golden_book();
    let document = sofd::Document::open_bytes(&bytes).expect("open the book");
    assert_eq!(
        document.page_count(),
        DOCS.len(),
        "one page per fixture, in DOCS order"
    );
    for (index, doc) in DOCS.iter().enumerate() {
        let from_book = document
            .to_png(index, &RENDER_OPTIONS)
            .unwrap_or_else(|e| panic!("render book page {index}: {e}"));
        let alone = render(doc);
        let ratio = difference_ratio(
            &decode(&alone, doc.name),
            &decode(&from_book, "the book page"),
        );
        assert!(
            ratio <= MAX_DIFFERING_RATIO,
            "book page {index} ({}) differs from its own document by {:.2}%",
            doc.name,
            ratio * 100.0,
        );
    }
}

#[test]
fn each_book_page_keeps_its_own_size() {
    // The fixtures no longer share a page box, so the book has to honour a
    // per-page extent rather than the common one it opens with.
    let bytes = sofd_testkit::golden_book();
    let document = sofd::Document::open_bytes(&bytes).expect("open the book");
    let wide = document
        .to_png(8, &RENDER_OPTIONS)
        .expect("render the landscape page");
    assert_eq!(
        decode(&wide, "the landscape page").dimensions(),
        (1123, 378),
        "page 8 carries its own 297x100mm box"
    );
}

#[test]
fn the_even_odd_page_keeps_its_hole() {
    // `Rule="Even-Odd"` on two nested rings has to cut the inner square out.
    // A backend that ignores the rule, or a fixture that misspells it and
    // falls back to `NonZero`, paints the square solid.
    let doc = DOCS
        .iter()
        .find(|doc| doc.name == "evenodd")
        .expect("the even-odd fixture");
    let page = decode(&render(doc), doc.name);
    let (w, h) = page.dimensions();
    let centre = page.get_pixel(w / 2, h / 2);
    let ring = page.get_pixel(w / 4, h / 2);
    assert_eq!(centre.0[3], 0, "the inner square is cut out, not filled");
    assert!(ring.0[3] > 0, "the ring around it is painted");
}

#[test]
fn every_golden_page_paints_something() {
    // A reference that is a blank page compares equal to the next blank page,
    // so a font fixture that draws nothing would pass forever while pinning
    // nothing. Every page must have ink somewhere.
    for doc in DOCS {
        let page = decode(&render(doc), doc.name);
        let inked = page.pixels().filter(|p| p.0[3] > 20).count();
        assert!(
            inked > 0,
            "{} renders as a blank page, so it pins no glyph",
            doc.name
        );
    }
}

#[test]
fn the_font_pages_draw_the_glyphs_their_face_provides() {
    // The five font fixtures exist because the drawing ones only ever draw one
    // Latin face. Each pairs a page with the matrix face `tests/matrix.rs` claims
    // covers its characters, so a lost glyph shows up as less ink than it had.
    for name in ["latin", "cjk", "ttc", "rtl", "manytomany"] {
        let doc = DOCS
            .iter()
            .find(|doc| doc.name == name)
            .unwrap_or_else(|| panic!("the {name} fixture"));
        assert!(
            doc.font.is_some(),
            "{name} must name the face it draws with"
        );
        let page = decode(&render(doc), doc.name);
        let inked = page.pixels().filter(|p| p.0[3] > 20).count();
        // The lightest of the five draws 880 pixels today; half that is the
        // floor, so a lost glyph or a face that falls back to .notdef trips it
        // while ordinary anti-aliasing noise does not.
        assert!(
            inked > 440,
            "{name} draws {inked} inked pixels, too few for its text"
        );
    }
}

#[test]
fn the_glyph_index_page_draws_the_glyphs_a_transform_names() {
    // Clause 11.4.5 on screen: two characters, three glyphs, and all three
    // have outlines in the subset, so the page shows three marks. A run that
    // drew the two glyphs it had characters for would be visibly different.
    let doc = DOCS
        .iter()
        .find(|doc| doc.name == "manytomany")
        .expect("the many-to-many fixture");
    let page = decode(&render(doc), doc.name);
    // The three glyphs cover 105 columns today. Two of them would cover about
    // seventy, so eighty separates three marks from two.
    let (w, h) = page.dimensions();
    let inked_columns = (0..w)
        .filter(|x| (0..h).any(|y| page.get_pixel(*x, y).0[3] > 20))
        .count();
    assert!(
        inked_columns > 80,
        "the run spans {inked_columns} columns, which is not three glyphs"
    );
}

#[test]
fn a_face_the_document_does_not_embed_still_draws() {
    // The right-to-left page names an id the resource table has no face for, so
    // the fallback chain has to supply one. That chain is what decides whether
    // a substituted document draws its text or its boxes.
    let doc = DOCS
        .iter()
        .find(|doc| doc.name == "rtl")
        .expect("the right-to-left fixture");
    let page = decode(&render(doc), doc.name);
    let inked = page.pixels().filter(|p| p.0[3] > 20).count();
    // 1001 pixels today: four letters of a 12mm Hebrew face. A page of .notdef
    // boxes has outlines too, so this is a floor rather than a proof, and the
    // pixel comparison above is what says the right face drew them.
    assert!(inked > 500, "the substitute draws {inked} inked pixels");
}
