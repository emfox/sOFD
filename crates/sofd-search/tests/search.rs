//! Tests for display-list text indexing and searching.

use sofd_common::Rect;
use sofd_display::text::GlyphRun;
use sofd_display::{DisplayList, DisplayListBuilder, DrawOp};
use sofd_font::FontId;
use sofd_geometry::Affine;
use sofd_search::{SearchIndex, SearchOptions, glyph_to_text};

fn run(text: &str, bbox: Rect, per_glyph: f64) -> GlyphRun {
    GlyphRun {
        font: FontId(0),
        size: sofd_common::Millimeter(3.5),
        glyphs: vec![1; text.chars().count()],
        advances: vec![per_glyph; text.chars().count()],
        text: Some(text.to_string()),
        cluster_map: Some((0..text.chars().count() as u32).collect()),
        codes_are_glyphs: false,
        transform: Affine::IDENTITY,
        glyph_angle: 0.0,
        fill: None,
        stroke: None,
        letter_spacing: 0.0,
        word_spacing: 0.0,
        bbox,
    }
}

fn list_with(texts: &[(&str, Rect)]) -> DisplayList {
    let mut builder = DisplayListBuilder::new(Rect::new(0.0, 0.0, 210.0, 297.0));
    for (text, bbox) in texts {
        builder.push(DrawOp::Text(run(text, *bbox, 3.5)));
    }
    DisplayList {
        page_count: 1,
        pages: vec![builder.finish(0)],
    }
}

fn box_at(y: f64, width: f64) -> Rect {
    Rect::new(10.0, y, width, 5.0)
}

#[test]
fn index_collects_run_text_per_page() {
    let list = list_with(&[
        ("hello world", box_at(10.0, 30.0)),
        ("second run", box_at(20.0, 30.0)),
    ]);
    let index = SearchIndex::build(&list);
    assert_eq!(index.pages().len(), 1);
    assert_eq!(index.pages()[0].runs.len(), 2);
    assert_eq!(index.page_text(0), "hello world\nsecond run");
    assert_eq!(index.page_text(9), "", "unknown page has no text");
}

#[test]
fn search_finds_case_insensitive_hits() {
    let list = list_with(&[("Hello World", box_at(10.0, 30.0))]);
    let index = SearchIndex::build(&list);
    let hits = index.search("hello", SearchOptions::default());
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].start, 0);
    assert_eq!(hits[0].end, 5);
    assert!(hits[0].snippet.contains("Hello"));
    let strict = index.search(
        "hello",
        SearchOptions {
            case_sensitive: true,
            ..Default::default()
        },
    );
    assert!(
        strict.is_empty(),
        "case sensitive search rejects the lowercase needle"
    );
}

#[test]
fn hit_rectangle_is_narrowed_to_the_match() {
    let list = list_with(&[("abcdef", box_at(10.0, 60.0))]);
    let index = SearchIndex::build(&list);
    let hits = index.search("cd", SearchOptions::default());
    assert_eq!(hits.len(), 1);
    let rect = hits[0].rect;
    assert!(
        (rect.x - 30.0).abs() < 0.5,
        "starts after two glyphs: {rect:?}"
    );
    assert!((rect.w - 20.0).abs() < 0.5, "covers two glyphs: {rect:?}");
    assert_eq!(rect.y, 10.0);
}

#[test]
fn whole_word_option_respects_boundaries() {
    let list = list_with(&[("catalog catalogue cat", box_at(10.0, 60.0))]);
    let index = SearchIndex::build(&list);
    let all = index.search("cat", SearchOptions::default());
    assert_eq!(all.len(), 3);
    let words = index.search(
        "cat",
        SearchOptions {
            whole_word: true,
            ..Default::default()
        },
    );
    assert_eq!(words.len(), 1);
    assert_eq!(words[0].start, 18);
}

#[test]
fn max_hits_stops_early() {
    let list = list_with(&[("aa aa aa aa", box_at(10.0, 60.0))]);
    let index = SearchIndex::build(&list);
    let limited = index.search(
        "aa",
        SearchOptions {
            max_hits: Some(2),
            ..Default::default()
        },
    );
    assert_eq!(limited.len(), 2);
}

#[test]
fn empty_needle_and_blank_pages_yield_nothing() {
    let list = list_with(&[("", box_at(10.0, 0.0))]);
    let index = SearchIndex::build(&list);
    assert!(index.search("", SearchOptions::default()).is_empty());
    assert!(index.search("x", SearchOptions::default()).is_empty());
    let empty = DisplayList {
        page_count: 0,
        pages: Vec::new(),
    };
    let index = SearchIndex::build(&empty);
    assert!(index.search("x", SearchOptions::default()).is_empty());
}

#[test]
fn glyph_to_text_maps_clusters_back_to_characters() {
    let glyph_run = run("abc", box_at(10.0, 30.0), 3.5);
    assert_eq!(glyph_to_text(&glyph_run, 1).as_deref(), Some("b"));
    assert_eq!(glyph_to_text(&glyph_run, 9), None);
    let mut bare = run("abc", box_at(10.0, 30.0), 3.5);
    bare.cluster_map = None;
    assert_eq!(glyph_to_text(&bare, 1), None);
}

/// The face the run was set in, so a glyph id can be turned back into a
/// character the way a subset font needs.
fn manager_with_fixture() -> (sofd_font::FontManager, FontId) {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../sofd-render/tests/data/Orbitron-Bold.ttf"
    );
    let bytes = std::fs::read(path).expect("font fixture");
    let mut manager = sofd_font::FontManager::new();
    let id = manager.registry_mut().register_embedded(
        "Res/Orbitron-Bold.ttf",
        "Orbitron",
        std::sync::Arc::new(bytes),
    );
    (manager, id)
}

/// A run whose text was never mapped: glyph ids only, as a subset font ships.
fn glyph_run_of(font: FontId, glyphs: &[u16]) -> GlyphRun {
    GlyphRun {
        font,
        size: sofd_common::Millimeter(3.5),
        glyphs: glyphs.to_vec(),
        advances: vec![3.5; glyphs.len()],
        text: None,
        cluster_map: None,
        codes_are_glyphs: true,
        transform: Affine::IDENTITY,
        glyph_angle: 0.0,
        fill: None,
        stroke: None,
        letter_spacing: 0.0,
        word_spacing: 0.0,
        bbox: box_at(10.0, 30.0),
    }
}

#[test]
fn a_run_without_text_is_indexed_from_its_font() {
    let (fonts, id) = manager_with_fixture();
    let face = fonts.face(id).expect("fixture face");
    let glyphs: Vec<u16> = "OFD"
        .chars()
        .filter_map(|c| face.glyph_id(c as u32))
        .collect();
    assert_eq!(glyphs.len(), 3, "the fixture maps O, F and D");
    let mut builder = DisplayListBuilder::new(Rect::new(0.0, 0.0, 210.0, 297.0));
    builder.push(DrawOp::Text(glyph_run_of(id, &glyphs)));
    let list = DisplayList {
        page_count: 1,
        pages: vec![builder.finish(0)],
    };

    // Without a face the run carries no text, which is the old behaviour.
    assert_eq!(SearchIndex::build(&list).page_text(0), "");
    let index = SearchIndex::build_with_fonts(&list, &fonts);
    assert_eq!(index.page_text(0), "OFD");
    assert_eq!(index.search("ofd", SearchOptions::default()).len(), 1);
}

#[test]
fn a_face_answers_when_the_cluster_map_is_missing() {
    let (fonts, id) = manager_with_fixture();
    let face = fonts.face(id).expect("fixture face");
    let glyph = face.glyph_id('A' as u32).expect("A in the fixture");
    let run = glyph_run_of(id, &[glyph]);
    assert_eq!(
        sofd_search::glyph_to_text_with_face(&run, 0, &face).as_deref(),
        Some("A")
    );
    assert_eq!(sofd_search::glyph_to_text_with_face(&run, 5, &face), None);
}
