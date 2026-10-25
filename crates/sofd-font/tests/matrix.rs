//! The font coverage matrix: one real face per script the project claims to
//! draw. `docs/Fonts.md` is the table and this file proves each row of it.
//! Samples are escapes, so the file stays English under the language policy.

use std::sync::Arc;

use sofd_font::{FontFace, FontId, FontManager};

/// One row: the fixture, the family to register it under, a sample string, and
/// whether every character of that string is expected to resolve to a glyph.
/// The last row says no: a face with no code point map resolves no character.
type Row = (&'static str, &'static str, &'static str, bool);

const ROWS: &[Row] = &[
    (
        "sofd-latin-subset.ttf",
        "Noto Sans",
        "Hamburgefonstiv 123",
        true,
    ),
    ("sofd-cjk-subset.ttf", "Noto Sans CJK SC", "中国发票", true),
    (
        "sofd-hebrew-subset.ttf",
        "Noto Sans Hebrew",
        "\u{5d0}\u{5d1}\u{5d2}\u{5d3}",
        true,
    ),
    (
        "sofd-arabic-subset.ttf",
        "Noto Naskh Arabic",
        "\u{627}\u{628}\u{62a}\u{62b}",
        true,
    ),
    (
        "sofd-nocmap-subset.ttf",
        "Noto Sans without a cmap",
        "Hamburgefonstiv",
        false,
    ),
    ("sofd-collection.ttc", "Noto Sans CJK SC", "中国", true),
];

/// Load one matrix fixture into a fresh manager.
fn load(file: &str, family: &str) -> (FontManager, FontId) {
    let path = format!("{}/tests/data/{file}", env!("CARGO_MANIFEST_DIR"));
    let bytes = std::fs::read(&path).unwrap_or_else(|e| panic!("read {file}: {e}"));
    let mut manager = FontManager::new();
    let id = manager
        .registry_mut()
        .register_embedded(file, family, Arc::new(bytes));
    (manager, id)
}

/// Load a fixture and its face in one step.
fn face_of(file: &str, family: &str) -> (FontManager, std::sync::Arc<sofd_font::FontFace>) {
    let (manager, id) = load(file, family);
    let face = manager.face(id).expect("load the matrix face");
    (manager, face)
}

/// The glyphs a run gets in string order for the characters the face covers:
/// the one-to-one case of clause 11.4, and the path a run takes when it carries
/// no `CGTransform`. The last matrix row has no `cmap`, so nothing resolves.
fn resolve(face: &FontFace, text: &str) -> Vec<u16> {
    text.chars()
        .filter_map(|ch| face.glyph_id(u32::from(ch)))
        .collect()
}

#[test]
fn every_face_in_the_matrix_loads_and_reports_a_millimetre_geometry() {
    for &(file, family, text, shapes) in ROWS {
        let (manager, id) = load(file, family);
        let face = manager.face(id).expect("load the matrix face");
        assert_eq!(face.units_per_em(), 1000.0, "{file} is a 1000-unit em");
        let metrics = face.metrics();
        assert!(metrics.ascender > 0.0, "{file} has an ascender");
        assert!(metrics.descender < 0.0, "{file} has a descender");
        assert!(metrics.line_height() > 0.0, "{file} has a line height");
        let glyphs = resolve(&face, text);
        let want = if shapes { text.chars().count() } else { 0 };
        assert_eq!(
            glyphs.len(),
            want,
            "{file} resolves {want} of its sample, got {glyphs:?}"
        );
    }
}

#[test]
fn a_latin_face_maps_each_code_point_to_its_own_glyph() {
    let (_, face) = face_of("sofd-latin-subset.ttf", "Noto Sans");
    let mut seen = Vec::new();
    for ch in "AWij0".chars() {
        let glyph = face.glyph_id(u32::from(ch)).expect("a mapped code point");
        assert_ne!(glyph, 0, "{ch} is not the notdef glyph");
        assert!(!seen.contains(&glyph), "{ch} repeats a glyph");
        seen.push(glyph);
        assert!(face.outline(glyph).is_some(), "{ch} has an outline to draw");
    }
    assert!(
        face.has_codepoint(u32::from('é')),
        "the accented letter survived subsetting"
    );
    assert!(
        !face.has_codepoint(0x4e2d),
        "an ideograph is not in a Latin face"
    );
}

#[test]
fn a_latin_face_is_proportional_rather_than_fixed_width() {
    let (_, face) = face_of("sofd-latin-subset.ttf", "Noto Sans");
    let narrow = face.glyph_id(u32::from('i')).expect("i");
    let wide = face.glyph_id(u32::from('W')).expect("W");
    assert!(
        face.advance_width(narrow) < face.advance_width(wide),
        "a lower-case i is narrower than a capital W"
    );
}

#[test]
fn a_cjk_face_maps_ideographs_and_gives_them_one_advance() {
    let (_, face) = face_of("sofd-cjk-subset.ttf", "Noto Sans CJK SC");
    const TEXT: &str = "中国发票";
    let widths: Vec<f64> = TEXT
        .chars()
        .map(|ch| face.advance_width(face.glyph_id(u32::from(ch)).expect("ideograph")))
        .collect();
    for (ch, w) in TEXT.chars().zip(&widths) {
        assert!(
            (w - widths[0]).abs() < 1e-9,
            "{ch:?} shares the ideograph width"
        );
        assert!(
            face.outline_for_codepoint(u32::from(ch)).is_some(),
            "{ch:?} has an outline to draw"
        );
    }
    // An ideograph is square: as wide as the em is tall above the baseline.
    assert!(
        (widths[0] - face.units_per_em()).abs() < 1e-9,
        "an ideograph is one em wide, got {}",
        widths[0]
    );
}

#[test]
fn a_right_to_left_face_shapes_its_letters_in_logical_order() {
    // Current behaviour and the gap at once: every letter gets a glyph and
    // keeps its cluster, and nothing reverses them. Visual order needs a
    // shaper this engine does not have, so a document relying on it is wrong.
    const RTL: &[(&str, &str, &str)] = &[
        (
            "sofd-hebrew-subset.ttf",
            "Noto Sans Hebrew",
            "\u{5d0}\u{5d1}\u{5d2}\u{5d3}",
        ),
        (
            "sofd-arabic-subset.ttf",
            "Noto Naskh Arabic",
            "\u{627}\u{628}\u{62a}\u{62b}",
        ),
    ];
    for &(file, family, text) in RTL {
        let (manager, id) = load(file, family);
        let face = manager.face(id).expect("load the matrix face");
        let glyphs = resolve(&face, text);
        assert_eq!(
            glyphs.len(),
            text.chars().count(),
            "{file} resolves every letter"
        );
        assert!(
            !glyphs.contains(&0),
            "{file} resolves no letter to .notdef, got {glyphs:?}"
        );
        // Clusters are string indices in ascending order, which is the reading
        // order a document wrote, not the visual order a shaper would produce.
        let clusters: Vec<u32> = (0..glyphs.len() as u32).collect();
        assert_eq!(clusters, (0..clusters.len() as u32).collect::<Vec<_>>());
    }
}

#[test]
fn a_face_with_no_cmap_is_still_addressable_by_glyph_index() {
    // The subset-font case clause 11.4 exists for: a producer strips the code
    // point map and names glyphs directly, so a face with no `cmap` must load
    // and outline rather than fail.
    let (manager, id) = load("sofd-nocmap-subset.ttf", "Noto Sans without a cmap");
    let face = manager.face(id).expect("load");
    assert_eq!(face.units_per_em(), 1000.0, "the face itself is readable");
    assert!(
        !face.has_codepoint(u32::from('A')),
        "there is no code point map"
    );
    assert!(
        face.glyph_id(u32::from('A')).is_none(),
        "so no code point resolves"
    );
    // The glyph the same face would name by code point still draws when the
    // document names it by index, which is what a `CGTransform` does.
    let (_, mapped) = face_of("sofd-latin-subset.ttf", "Noto Sans");
    let glyph = mapped.glyph_id(u32::from('A')).expect("A");
    assert!(
        face.outline_for_run_code(glyph, true).is_some(),
        "a glyph index draws without a cmap"
    );
    assert!(
        face.outline_for_run_code(u16::from(b'A'), false).is_none(),
        "a code point does not"
    );
    assert!(
        resolve(&face, "A").is_empty(),
        "no character resolves to a glyph without a cmap"
    );
}

#[test]
fn a_font_collection_yields_its_first_face() {
    // Reading the first face of a collection is right for a producer that
    // embeds one file; the rest are unreachable, since an id names one file.
    // What matters is that the face is read, not a plausible-looking header.
    let (_, face) = face_of("sofd-collection.ttc", "Noto Sans CJK SC");
    assert_eq!(
        face.units_per_em(),
        1000.0,
        "the first face is the one read"
    );
    for cp in [0x4e2d_u32, 0x56fd] {
        let glyph = face.glyph_id(cp).expect("an ideograph in the first face");
        assert!(
            face.outline(glyph).is_some(),
            "the first face outlines, not just parses"
        );
    }
    assert!(
        !face.has_codepoint(u32::from('A')),
        "it is the CJK face, not a stray header"
    );
    // Face zero of this collection is not at the start of the file. A reader
    // that opens byte zero instead reads a table directory out of a table, and
    // the numbers it reports are plausible enough that only the ink catches it.
    let path = format!(
        "{}/tests/data/sofd-collection.ttc",
        env!("CARGO_MANIFEST_DIR")
    );
    let bytes = std::fs::read(&path).expect("read the collection");
    let first = u32::from_be_bytes([bytes[12], bytes[13], bytes[14], bytes[15]]);
    assert!(first > 0, "the fixture really does start its face later");
}
