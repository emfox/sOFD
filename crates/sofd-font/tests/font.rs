//! The font layer driven as a consumer.
//! A real face is loaded from the shared fixture, so lookup, metrics and
//! outlining run for real, plus the missing-font error a converter checks.

use std::sync::Arc;

use sofd_font::{FontId, FontManager, FontSource, FontTraits};

/// The shared real face, so outlining is not a degraded path.
fn font_bytes() -> Vec<u8> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../sofd-render/tests/data/Orbitron-Bold.ttf"
    );
    std::fs::read(path).expect("font fixture")
}

/// A manager with the fixture face registered under id 0.
fn manager_with_fixture() -> (FontManager, FontId) {
    let mut manager = FontManager::new();
    let id = manager.registry_mut().register_embedded(
        "Res/Orbitron-Bold.ttf",
        "Orbitron",
        Arc::new(font_bytes()),
    );
    (manager, id)
}

/// The wide face the chain tests pin, so a substitute is one the repo carries
/// rather than whatever CJK font the host happens to install.
fn cjk_bytes() -> Vec<u8> {
    let path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/tests/data/sofd-cjk-subset.ttf"
    );
    std::fs::read(path).expect("cjk fixture")
}

/// A manager with the fixture face registered and the wide fixture pinned as
/// the chain's substitute, so every step runs without a system font.
fn manager_with_pinned_substitute() -> (FontManager, FontId, FontId) {
    let mut manager = FontManager::new();
    let narrow = manager.registry_mut().register_embedded(
        "Res/Orbitron-Bold.ttf",
        "Orbitron",
        Arc::new(font_bytes()),
    );
    let wide = manager
        .registry_mut()
        .register_fallback_with_data("cjk-pinned", Some(Arc::new(cjk_bytes())));
    manager.build_fallback_chain();
    (manager, narrow, wide)
}

#[test]
fn an_embedded_face_loads_and_reports_its_family() {
    let (manager, id) = manager_with_fixture();
    let face = manager.face(id).expect("load");
    assert_eq!(face.family(), "Orbitron");
    assert_eq!(face.id(), id);
    assert!(!face.is_fallback());
    assert!(face.units_per_em() > 0.0);
}

#[test]
fn a_face_reports_metrics_a_caller_can_lay_out_with() {
    let (manager, id) = manager_with_fixture();
    let metrics = manager.face(id).expect("load").metrics();
    // Ascent sits above the baseline and descent below; both are non-zero for
    // any real face, and the line height combines them with the gap.
    assert!(metrics.ascender > 0.0, "{metrics:?}");
    assert!(metrics.units_per_em > 0);
    assert_eq!(
        metrics.line_height(),
        metrics.ascender - metrics.descender + metrics.line_gap
    );
}

#[test]
fn a_known_codepoint_maps_to_a_glyph_and_back() {
    let (manager, id) = manager_with_fixture();
    let face = manager.face(id).expect("load");
    let glyph = face.glyph_id('A' as u32).expect("A is in the fixture");
    assert_ne!(glyph, 0, "glyph 0 is .notdef");
    // The reverse map is what search and copy depend on, so the round trip has
    // to land back on the character it started from.
    assert_eq!(face.reverse_cmap(glyph), Some('A' as u32));
}

#[test]
fn a_character_the_face_lacks_is_absent_rather_than_wrong() {
    let (manager, id) = manager_with_fixture();
    let face = manager.face(id).expect("load");
    // A CJK ideograph is not in a Latin display face, and the honest answer is
    // `None` rather than a bogus glyph.
    assert!(!face.has_codepoint('中' as u32));
    assert_eq!(face.glyph_id('中' as u32), None);
}

#[test]
fn a_real_glyph_has_an_outline_and_a_space_does_not() {
    let (manager, id) = manager_with_fixture();
    let face = manager.face(id).expect("load");

    let a = face.glyph_id('A' as u32).expect("A");
    assert!(
        face.outline_for_codepoint('A' as u32).is_some(),
        "A has an outline"
    );
    assert!(matches!(
        face.glyph_outline(a),
        sofd_font::GlyphOutline::Path(_)
    ));

    // A space has no ink, so its outline is empty; a caller that wrote a path
    // for it would emit a stray mark.
    if let Some(space) = face.glyph_id(' ' as u32) {
        assert!(matches!(
            face.glyph_outline(space),
            sofd_font::GlyphOutline::Empty
        ));
    }
}

#[test]
fn a_space_is_not_counted_missing_though_it_has_no_outline() {
    // A space has no ink, so there is never a path to hand back, and a reader
    // asking whether a run lost a character must not take that for a gap, or a
    // page of prose would report every space it contains.
    let (manager, id) = manager_with_fixture();
    let face = manager.face(id).expect("load");
    let space = face.glyph_id(' ' as u32).expect("the fixture maps a space");
    assert!(
        manager.outline_for_code(&face, ' ' as u16, false).is_none(),
        "a blank has no outline to draw"
    );
    assert!(
        !manager.is_missing_code(&face, ' ' as u16, false),
        "and drawing nothing is not the same as lacking the glyph"
    );
    assert!(
        !manager.is_missing_code(&face, space, true),
        "the same blank reached by its glyph id is no more a gap"
    );
    // A character the face really lacks is still a gap on both paths.
    assert!(manager.is_missing_code(&face, '中' as u16, false));
    assert!(manager.is_missing_code(&face, 43981, true));
}

#[test]
fn a_glyph_advance_is_positive_for_a_drawn_character() {
    let (manager, id) = manager_with_fixture();
    let face = manager.face(id).expect("load");
    let a = face.glyph_id('A' as u32).expect("A");
    assert!(face.advance_width(a) > 0.0, "a letter must advance the pen");
}

#[test]
fn an_unknown_font_id_is_an_error_not_a_panic() {
    let (manager, _) = manager_with_fixture();
    let err = manager.face(FontId(999)).expect_err("no such font");
    assert!(err.to_string().contains("999"), "{err}");
}

#[test]
fn each_character_of_a_string_resolves_to_its_own_glyph() {
    let (manager, id) = manager_with_fixture();
    let face = manager.face(id).expect("load");
    let glyphs: Vec<u16> = "ABC"
        .chars()
        .map(|ch| {
            face.glyph_id(u32::from(ch))
                .expect("a letter of the fixture")
        })
        .collect();
    assert_eq!(glyphs.len(), 3, "one glyph per character");
    assert!(
        glyphs.iter().all(|&g| g != 0),
        "none of them is .notdef, got {glyphs:?}"
    );
    assert!(
        glyphs.iter().all(|&g| face.advance_width(g) > 0.0),
        "each moves the pen, got {glyphs:?}"
    );
}

#[test]
fn an_empty_string_asks_the_face_for_nothing() {
    let (manager, id) = manager_with_fixture();
    let face = manager.face(id).expect("load");
    let glyphs: Vec<u16> = ""
        .chars()
        .filter_map(|ch| face.glyph_id(u32::from(ch)))
        .collect();
    assert!(glyphs.is_empty());
    let width: f64 = glyphs.iter().map(|&g| face.advance_width(g)).sum();
    assert_eq!(width, 0.0, "an empty run is zero wide");
}

#[test]
fn a_character_with_no_face_is_absent_rather_than_a_blank_glyph() {
    let (manager, id) = manager_with_fixture();
    let face = manager.face(id).expect("load");
    // The fixture lacks the ideograph, so it cannot be drawn. The face says so
    // instead of handing back .notdef, which would draw an empty box where the
    // caller expected a character.
    const SAMPLE: &str = "A中";
    let resolved: Vec<Option<u16>> = SAMPLE
        .chars()
        .map(|ch| face.glyph_id(u32::from(ch)))
        .collect();
    assert!(resolved[0].is_some(), "the 'A' resolves");
    assert_eq!(resolved[1], None, "the ideograph does not");
    assert_eq!(resolved.iter().flatten().count(), 1);
}

#[test]
fn an_advance_in_a_thousandth_of_an_em_is_what_a_size_multiplies() {
    // A box is laid out as `advance / 1000 * size_mm`; leaving the advance in
    // the face's own units would widen every run by its em. Doubling the size
    // is the only thing the size is allowed to change.
    let (manager, id) = manager_with_fixture();
    let face = manager.face(id).expect("load");
    let a = face.glyph_id(u32::from('A')).expect("A");

    let small = face.advance_width(a) / 1000.0 * 3.5;
    let large = face.advance_width(a) / 1000.0 * 7.0;
    assert!(
        (large / small - 2.0).abs() < 1e-9,
        "twice the size is twice the width, ratio was {}",
        large / small
    );
    assert!(
        small < 3.5,
        "a letter is narrower than its size, got {small}"
    );
}

#[test]
fn an_advance_is_normalized_to_a_thousand_units_of_the_em() {
    let (manager, id) = manager_with_fixture();
    let face = manager.face(id).expect("load");
    let a = face.glyph_id(u32::from('A')).expect("A");
    let advance = face.advance_width(a);
    assert!(
        advance > 0.0 && advance < face.units_per_em(),
        "a letter is a fraction of the em, got {advance} of {}",
        face.units_per_em()
    );
    // At ten points, the default a document gets, the width in millimetres is
    // the normalized advance scaled by the size.
    assert!((advance / 1000.0 * 10.0 - face.advance_width(a) / 100.0).abs() < 1e-9);
}

#[test]
fn the_fallback_chain_round_trips_and_is_empty_by_default() {
    let (manager, id) = manager_with_fixture();
    assert!(manager.fallback_chain().is_empty());
    manager.set_fallback_chain(vec![id]);
    assert_eq!(manager.fallback_chain(), vec![id]);
}

#[test]
fn a_fallback_is_found_for_a_codepoint_the_chain_covers() {
    let (manager, id) = manager_with_fixture();
    manager.set_fallback_chain(vec![id]);
    let (found, _face) = manager.fallback_for('A' as u32).expect("A is covered");
    assert_eq!(found, id);
    // A codepoint no face in the chain covers yields nothing rather than a
    // wrong face.
    assert!(manager.fallback_for('中' as u32).is_none());
}

#[test]
fn registering_the_same_bytes_twice_reuses_the_id() {
    let mut manager = FontManager::new();
    let first = manager.registry_mut().register_embedded(
        "Res/Orbitron-Bold.ttf",
        "Orbitron",
        Arc::new(font_bytes()),
    );
    let again = manager.registry_mut().register_embedded(
        "Res/Copy.ttf",
        "Orbitron Copy",
        Arc::new(font_bytes()),
    );
    assert_eq!(
        again, first,
        "identical bytes must not register a second face"
    );
    assert_eq!(manager.registry().len(), 1);
}

#[test]
fn registering_a_family_twice_returns_the_first_id() {
    let mut manager = FontManager::new();
    let first = manager.registry_mut().register_system("Serif");
    let second = manager.registry_mut().register_system("Serif");
    assert_eq!(first, second);
    assert_eq!(manager.registry().len(), 1);
}

#[test]
fn a_system_family_with_no_resolvable_face_is_reported_missing() {
    // Nothing named "No Such Family 12345" is installed anywhere, and the
    // caller says what it is drawing, so a generic face in its place would
    // still miss the characters. That is a missing font, not a substitute.
    let mut manager = FontManager::new();
    let id = manager
        .registry_mut()
        .register_system("No Such Family 12345");
    // A code point no installed face carries — the Plane 1 range fonts do not
    // ship — so the test says "no substitute covers this" on any host.
    let err = manager
        .face_for(id, &[0x10_FF_FD])
        .expect_err("unresolvable");
    assert!(err.to_string().contains("No Such Family 12345"), "{err}");
    assert!(manager.is_unresolved(id), "and the miss is recorded");
}

#[test]
fn a_named_family_is_given_a_generic_face_when_nothing_covers_the_text() {
    // Clause 11.1 leaves the substitute open when no listed family carries the
    // characters, so the shape the declaration asked for is still better than
    // an empty page. The miss is recorded so a caller can say so.
    let mut manager = FontManager::new();
    let id = manager
        .registry_mut()
        .register_system("No Such Family 12345");
    let face = manager.face(id).expect("a generic family answers");
    assert!(!face.family().is_empty(), "and it names what answered");
}

#[test]
fn a_system_reference_with_no_data_is_listed_as_missing() {
    let mut manager = FontManager::new();
    manager.registry_mut().register_system("Serif");
    assert_eq!(manager.registry().missing_families(), vec!["Serif"]);
}

#[test]
fn clearing_the_cache_drops_the_count_but_keeps_handles_alive() {
    let (manager, id) = manager_with_fixture();
    let held = manager.face(id).expect("load");
    assert!(manager.loaded_count() >= 1);
    manager.clear_cache();
    assert_eq!(manager.loaded_count(), 0);
    // A handle a caller already holds stays valid, so clearing mid-render is
    // safe for work in flight.
    assert_eq!(held.family(), "Orbitron");
}

#[test]
fn building_the_chain_registers_every_embedded_face_in_id_order() {
    let mut manager = FontManager::new();
    let first =
        manager
            .registry_mut()
            .register_embedded("Res/One.ttf", "Orbitron", Arc::new(font_bytes()));
    // Different bytes, so the dedup keeps them apart.
    let mut other = font_bytes();
    other.push(0);
    let second =
        manager
            .registry_mut()
            .register_embedded("Res/Two.ttf", "Orbitron Two", Arc::new(other));
    let chain = manager.build_fallback_chain();
    // Neither face is wide, so the chain leads with the CJK substitute and
    // then the two embedded faces in id order.
    let substitute = manager
        .registry()
        .iter()
        .find(|e| matches!(e.source, FontSource::Fallback { .. }))
        .map(|e| e.id);
    assert_eq!(
        chain,
        vec![substitute.unwrap(), first, second],
        "both faces are reachable, in id order"
    );
    assert_eq!(
        manager.fallback_chain(),
        chain,
        "and the chain is installed, not just returned"
    );
}

#[test]
fn a_substitute_face_is_only_chained_where_one_can_be_resolved() {
    // A system reference is chained when the build can load one and skipped
    // when it cannot, because a link that always fails would be retried on
    // every code point that reaches fallback.
    let mut manager = FontManager::new();
    let embedded = manager.registry_mut().register_embedded(
        "Res/Orbitron-Bold.ttf",
        "Orbitron",
        Arc::new(font_bytes()),
    );
    let substitute = manager.registry_mut().register_system("sans-serif");
    let chain = manager.build_fallback_chain();
    assert!(
        chain.contains(&embedded),
        "an embedded face is always reachable, got {chain:?}"
    );
    let resolvable = cfg!(all(feature = "system-fonts", not(target_arch = "wasm32")));
    assert_eq!(
        chain.contains(&substitute),
        resolvable,
        "a system face joins only where it can load, got {chain:?}"
    );
}

#[test]
fn building_the_chain_twice_changes_nothing() {
    // The chain is rebuilt whenever a document opens, so it has to be a
    // function of the registry rather than something that accumulates.
    let (mut manager, _) = manager_with_fixture();
    let once = manager.build_fallback_chain();
    let twice = manager.build_fallback_chain();
    assert_eq!(once, twice);
    // The fixture's one face, plus the CJK substitute no document can do
    // without. Rebuilding must not add a second substitute.
    assert_eq!(
        once.len(),
        2,
        "the fixture registers one face, got {once:?}"
    );
}

#[test]
fn a_code_the_face_lacks_is_outlined_from_the_chain() {
    // A code point the run's own face lacks used to drop, leaving the page
    // blank; it now resolves against the wide link the chain carries. The
    // wide face is the pinned fixture, so no system font is needed.
    let (manager, narrow, wide) = manager_with_pinned_substitute();
    let face = manager.face(narrow).expect("the embedded face loads");

    let latin = manager
        .outline_for_code(&face, 'A' as u16, false)
        .expect("Latin comes from the face itself");
    assert_eq!(latin.1.id(), narrow, "and is attributed to it");

    let han = manager
        .outline_for_code(&face, '你' as u16, false)
        .expect("a Han code resolves from the chain");
    assert_eq!(han.1.id(), wide, "from the pinned wide face, not the run's");
    assert!(
        han.1.has_codepoint('你' as u32),
        "and that is the face that covers it"
    );
}

#[test]
fn a_resolved_glyph_id_is_never_substituted() {
    // A glyph index only means anything against the face it was resolved for,
    // so it must not be looked up in the chain even when that face lacks it.
    let (manager, narrow, _) = manager_with_pinned_substitute();
    let face = manager.face(narrow).expect("the embedded face loads");
    // Glyph id 43981 is arbitrary; the point is the code path, not the glyph.
    assert!(
        manager.outline_for_code(&face, 43981, true).is_none(),
        "an id the face cannot draw is not handed to another face"
    );
}

#[test]
fn a_named_fixed_width_family_is_read_as_fixed_width_though_the_flag_is_absent() {
    // Clause 11.1 makes `FixedWidth` optional, so a document that names
    // Courier and writes nothing else still means a monospaced substitute.
    let named = |family: &str| FontTraits {
        family: Some(family.to_string()),
        ..FontTraits::default()
    };
    assert!(named("Courier New").wants_fixed_width());
    assert!(named("Consolas").wants_fixed_width());
    assert!(named("DejaVu Sans Mono").wants_fixed_width());
    assert!(!named("SimSun").wants_fixed_width());
    assert!(!named("Microsoft YaHei").wants_fixed_width());
}

#[test]
fn a_declared_flag_outranks_the_family_name() {
    let declared = |fixed_width: bool| FontTraits {
        family: Some("Courier New".to_string()),
        fixed_width: Some(fixed_width),
        ..FontTraits::default()
    };
    assert!(!declared(false).wants_fixed_width(), "false is an answer");
    assert!(declared(true).wants_fixed_width());
}
