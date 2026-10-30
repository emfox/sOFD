//! Coverage for the reader: the state machine behind the window. A widget tree
//! proves little, so these tests drive the same `App` the window draws from and
//! check that opening, navigating and rendering a page behave.

use std::path::PathBuf;

use sofd_reader::viewer::{DroppedFile, Layout, MAX_DPI, MIN_DPI, PageCache};
use sofd_reader::{App, Canvas};

/// The real minimal document from the conformance fixtures.
fn minimal() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../compliance/fixtures/minimal.ofd")
}

fn open() -> App {
    let mut app = App::new();
    app.open_path(&minimal())
        .expect("the fixture document opens");
    app
}

/// A reader over a document with `pages` pages, so arrangements have room.
fn open_pages(pages: usize) -> App {
    let mut app = App::new();
    let bytes = sofd_testkit::multipage_archive(pages);
    app.open_bytes(&bytes, Some("many.ofd"))
        .expect("the fixture opens");
    app
}

/// Ask for a page and wait for the worker to deliver it.
/// Rendering is off the draw path, so a test that wants a raster has to pump
/// the queue the way a frame does; the bound fails loudly if it never answers.
fn wait_for_page(app: &mut App, page: usize) {
    app.page_image(page).expect("the request is accepted");
    // The worker runs on another thread, so the budget is wall clock: a shared
    // runner can stall it well past a laptop, and a real hang still panics.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while std::time::Instant::now() < deadline {
        app.poll();
        if app.cached(page).is_some() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    panic!("page {page} never arrived from the render worker");
}

#[test]
fn a_fresh_reader_has_nothing_open() {
    let app = App::new();
    assert_eq!(app.page_count(), 0);
    assert_eq!(app.current_page(), 0);
    assert!(app.path().is_none());
    assert!(app.error().is_none());
}

#[test]
fn opening_the_fixture_reports_its_pages() {
    let app = open();
    assert!(app.page_count() >= 1, "the fixture has at least one page");
    assert!(app.path().is_some());
    assert!(app.error().is_none(), "a clean open leaves no error");
}

#[test]
fn a_damaged_file_is_refused_with_the_engine_message() {
    let mut app = App::new();
    let err = app
        .open_bytes(b"this is not a zip archive at all", Some("broken.ofd"))
        .expect_err("garbage is not a document");
    assert!(!err.message.is_empty(), "the reader says something");
    assert_eq!(app.page_count(), 0, "nothing was opened");
}

#[test]
fn navigation_clamps_to_the_pages_that_exist() {
    let mut app = open();
    let last = app.page_count() - 1;

    app.go_to(usize::MAX);
    assert_eq!(
        app.current_page(),
        last,
        "past the end lands on the last page"
    );

    app.previous_page();
    assert_eq!(app.current_page(), 0, "before the start stays at the first");

    app.go_to(0);
    assert!(!app.has_previous());
    if last == 0 {
        assert!(!app.has_next());
    } else {
        assert!(app.has_next());
        app.next_page();
        assert_eq!(app.current_page(), 1);
    }
}

#[test]
fn a_closed_document_forgets_its_pages() {
    let mut app = open();
    let before = app.page_count();
    app.close();
    assert_eq!(app.page_count(), 0);
    assert!(app.path().is_none());
    assert!(before >= 1, "there was something to forget");
}

#[test]
fn a_page_rasterizes_and_is_reused() {
    let mut app = open();
    let page = app.current_page();

    wait_for_page(&mut app, page);
    let first = app
        .cached(page)
        .expect("the page is now cached")
        .pixels
        .size();
    assert!(
        first[0] > 0 && first[1] > 0,
        "the image has real dimensions"
    );

    // A second request must not rasterize again: the cache answers it.
    app.page_image(page).expect("the cached page is returned");
    assert_eq!(app.cached(page).map(|p| p.pixels.size()), Some(first));
}

/// The reader asks for what is on screen once per frame, so a page already on
/// its way must not be asked for again. Restamping it every frame would leave
/// the worker forever rendering a raster the next frame had superseded.
#[test]
fn asking_for_the_same_page_every_frame_still_delivers_it() {
    let mut app = open();
    let page = app.current_page();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while std::time::Instant::now() < deadline {
        // What a frame does: ask for the visible page, then collect results.
        let _ = app.canvas();
        app.poll();
        if app.cached(page).is_some() {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    panic!("page {page} was asked for on every frame and never arrived");
}

#[test]
fn the_cache_keys_on_the_resolution_too() {
    let mut app = open();
    let page = app.current_page();
    wait_for_page(&mut app, page);
    let wide = app.cached(page).map(|p| p.pixels.size()).unwrap();

    app.set_zoom(200.0 / 72.0);
    wait_for_page(&mut app, page);
    let larger = app.cached(page).map(|p| p.pixels.size()).unwrap();

    assert_ne!(wide, larger, "a different zoom is a different picture");
}

#[test]
fn the_resolution_is_clamped_to_a_sane_range() {
    let mut app = App::new();
    app.set_zoom(0.001);
    assert_eq!(app.dpi(), MIN_DPI, "below the floor is clamped up");
    app.set_zoom(1000.0);
    assert_eq!(app.dpi(), MAX_DPI, "above the ceiling is clamped down");
}

#[test]
fn a_large_cache_evicts_the_oldest_page() {
    let mut cache = PageCache::new(2);
    for page in 0..3 {
        let rendered = sofd_reader::RenderedPage {
            pixels: sofd_reader::Rgba {
                width: 1,
                height: 1,
                bytes: vec![255, 255, 255, 255],
            },
            size_mm: (210.0, 297.0),
            window: None,
            short: sofd_reader::Short::default(),
        };
        cache.insert(page, 96.0, rendered);
    }
    assert_eq!(cache.len(), 2, "the cache never exceeds its limit");
    assert!(cache.get(0, 96.0).is_none(), "the oldest page was evicted");
    assert!(cache.get(2, 96.0).is_some(), "the newest page is kept");
}

#[test]
fn the_layout_choice_is_remembered() {
    let mut app = App::new();
    assert_eq!(app.layout(), Layout::Single, "one page is the default");
    app.set_layout(Layout::Facing);
    assert_eq!(app.layout(), Layout::Facing);
    app.set_layout(Layout::Continuous);
    assert_eq!(app.layout(), Layout::Continuous);
}

#[test]
fn the_page_label_counts_from_one() {
    let app = open();
    let label = app.page_label();
    assert!(label.contains('1'), "the first page reads as 1: {label}");
    assert!(
        label.contains(&app.page_count().to_string()),
        "the label names the total: {label}"
    );
}

#[test]
fn a_failure_is_shown_and_can_be_cleared() {
    let mut app = App::new();
    app.set_error("the page would not render");
    assert_eq!(
        app.error().map(|e| e.message.as_str()),
        Some("the page would not render")
    );
    app.clear_error();
    assert!(app.error().is_none());
}

#[test]
fn the_open_request_is_handed_out_once() {
    let mut app = App::new();
    assert!(!app.wants_open());
    app.request_open();
    assert!(app.wants_open());
    assert!(app.take_open_request(), "the window acts on it");
    assert!(!app.wants_open(), "and does not act on it twice");
}

#[test]
fn opening_another_document_replaces_the_first() {
    let mut app = open();
    let first = app.page_count();
    app.open_bytes(&std::fs::read(minimal()).unwrap(), Some("again.ofd"))
        .expect("the fixture opens a second time");
    assert_eq!(app.page_count(), first, "the same document, the same pages");
    assert_eq!(app.current_page(), 0, "reading starts at the first page");
}

#[test]
fn an_empty_reader_asks_for_a_document_rather_than_a_page() {
    let mut app = App::new();
    assert_eq!(app.canvas(), Canvas::Empty, "no document, nothing to paint");
    assert!(
        !app.drop_hint().is_empty(),
        "the hint is written in the reader's language"
    );
    assert!(
        !app.title().contains(".ofd"),
        "the title names no document yet"
    );
}

#[test]
fn an_open_reader_asks_the_worker_then_paints_its_page() {
    let mut app = open();
    let page = app.current_page();
    // The draw path never rasterizes: asked for a page it has not got, it
    // reports it as on its way, or hands back the page it already has if the
    // worker was quick enough to beat the question.
    let first = app.canvas();
    assert!(
        matches!(first, Canvas::Pending | Canvas::Page | Canvas::Blank),
        "the canvas either waits or paints, got {first:?}"
    );

    wait_for_page(&mut app, page);
    let canvas = app.canvas();
    assert!(
        matches!(canvas, Canvas::Page | Canvas::Blank),
        "the fixture rasterizes to something paintable, got {canvas:?}"
    );
    assert!(!app.is_rendering(), "and the worker owes nothing more");
}

#[test]
fn a_blank_page_is_still_drawn() {
    let mut app = open();
    let page = app.current_page();
    wait_for_page(&mut app, page);
    let blank = app.current_page_is_blank();
    assert_eq!(
        app.canvas(),
        if blank { Canvas::Blank } else { Canvas::Page },
        "a blank page is a page, so the canvas stays in the drawing branch"
    );
    assert!(
        !app.blank_notice().is_empty(),
        "and the notice is localized"
    );
}

#[test]
fn an_error_alone_does_not_displace_the_empty_state() {
    let mut app = App::new();
    app.set_error("the page would not render");
    assert_eq!(
        app.canvas(),
        Canvas::Empty,
        "the canvas checks for a document before it checks for a failure"
    );
    assert_eq!(
        app.error().map(|e| e.message.as_str()),
        Some("the page would not render"),
        "the status bar still shows it"
    );
}

#[test]
fn a_page_past_the_end_clamps_back_into_the_document() {
    let mut app = open();
    app.go_to(app.page_count() + 10);
    let page = app.current_page();
    wait_for_page(&mut app, page);
    assert_eq!(
        app.canvas(),
        Canvas::Page,
        "the clamped page still rasterizes rather than failing"
    );
}

#[test]
fn a_failed_open_leaves_the_previous_document_open() {
    let mut app = open();
    let pages = app.page_count();
    let err = app.open_bytes(b"not an ofd container", Some("broken.ofd"));
    assert!(err.is_err(), "the broken document is refused");
    assert_eq!(app.page_count(), pages, "and the good one is still open");
    assert!(app.path().is_some(), "the reader keeps its place");
}

#[test]
fn a_dropped_path_opens_like_a_chosen_one() {
    let mut app = App::new();
    app.open_dropped(DroppedFile::Path(minimal()))
        .expect("the fixture opens from a drop");
    assert!(app.page_count() >= 1);
    assert_eq!(app.file_name().as_deref(), Some("minimal.ofd"));
}

#[test]
fn a_dropped_payload_opens_and_keeps_its_name() {
    let bytes = std::fs::read(minimal()).unwrap();
    let mut app = App::new();
    app.open_dropped(DroppedFile::Bytes {
        name: "dropped.ofd".to_string(),
        bytes,
    })
    .expect("the fixture opens from dropped bytes");
    assert!(app.page_count() >= 1);
    assert_eq!(app.file_name().as_deref(), Some("dropped.ofd"));
}

#[test]
fn the_window_title_names_the_open_document() {
    let mut app = App::new();
    assert_eq!(app.title(), "sOFD Reader");
    app.open_path(&minimal()).unwrap();
    assert!(
        app.title().starts_with("minimal.ofd"),
        "got {}",
        app.title()
    );
}

#[test]
fn the_info_line_is_written_in_the_readers_language() {
    let mut app = App::new();
    assert!(app.info_label().is_empty(), "no document, no line");
    app.open_path(&minimal()).unwrap();
    assert!(
        app.info_label().contains("minimal.ofd"),
        "got {}",
        app.info_label()
    );
}

#[test]
fn a_page_never_rasterizes_past_the_texture_limit() {
    // A4 at the reader's highest zoom would be 3,480 px wide, which no driver
    // will hold, so the resolution has to fall back to what actually fits.
    let a4 = (210.0, 297.0);
    let capped = sofd_reader::viewer::dpi_that_fits(a4);
    assert!(
        capped < MAX_DPI,
        "A4 must be capped below the requested ceiling, got {capped}"
    );
    let side = f64::from(capped) / 25.4 * 297.0;
    assert!(
        side <= f64::from(sofd_reader::viewer::MAX_TEXTURE_SIDE),
        "the long side is {side:.0} px, past the limit"
    );
}

#[test]
fn a_rendered_page_is_small_enough_to_upload() {
    let mut app = open();
    app.set_zoom(MAX_DPI / 72.0);
    wait_for_page(&mut app, 0);
    let rendered = app.cached(0).expect("and lands in the cache");
    let (w, h) = (rendered.pixels.size()[0], rendered.pixels.size()[1]);
    let limit = sofd_reader::viewer::MAX_TEXTURE_SIDE as usize;
    assert!(
        w <= limit && h <= limit,
        "rasterized {w}x{h}, limit {limit}"
    );
}

#[test]
fn a_small_page_may_use_the_full_zoom_range() {
    // Nothing is capped for a page small enough to fit at the top zoom.
    let postage_stamp = (20.0, 20.0);
    assert_eq!(
        sofd_reader::viewer::dpi_that_fits(postage_stamp),
        MAX_DPI,
        "a small page is not penalised for the limit"
    );
}

#[test]
fn single_page_layout_shows_exactly_the_current_page() {
    let mut app = open_pages(5);
    app.set_layout(Layout::Single);
    for page in 0..5 {
        app.go_to(page);
        assert_eq!(app.visible(), vec![page], "single shows only page {page}");
    }
}

#[test]
fn facing_layout_pairs_pages_and_keeps_the_pairing_stable() {
    let mut app = open_pages(6);
    app.set_layout(Layout::Facing);

    // A spread is the fixed pair (even, even+1), so page 0 always sits with 1
    // and the pairing does not change as the reader moves through it.
    app.go_to(0);
    assert_eq!(app.visible(), vec![0, 1]);
    app.go_to(1);
    assert_eq!(app.visible(), vec![0, 1], "1 stays with 0, not with 2");
    app.go_to(2);
    assert_eq!(app.visible(), vec![2, 3]);
    app.go_to(3);
    assert_eq!(app.visible(), vec![2, 3], "3 stays with 2");
    app.go_to(5);
    assert_eq!(app.visible(), vec![4, 5]);
}

#[test]
fn a_trailing_odd_page_is_a_spread_of_one() {
    let mut app = open_pages(5);
    app.set_layout(Layout::Facing);
    app.go_to(4);
    assert_eq!(app.visible(), vec![4], "the last page has no partner");
}

#[test]
fn continuous_layout_shows_a_bounded_window_around_the_current_page() {
    let mut app = open_pages(20);
    app.set_layout(Layout::Continuous);

    app.go_to(10);
    let window = app.visible();
    assert!(
        window.contains(&10),
        "the current page is always in the window"
    );
    assert!(
        window.len() < 20,
        "continuous must not rasterize the whole document, got {} pages",
        window.len()
    );
    assert_eq!(window.first(), Some(&7), "three pages of lead-in");
    assert_eq!(window.last(), Some(&13), "three pages of look-ahead");

    app.go_to(0);
    assert_eq!(
        app.visible().first(),
        Some(&0),
        "the window clamps at the start"
    );
    app.go_to(19);
    assert_eq!(app.visible().last(), Some(&19), "and at the end");
}

#[test]
fn a_layout_with_no_pages_shows_nothing() {
    let mut app = App::new();
    for layout in [Layout::Single, Layout::Facing, Layout::Continuous] {
        app.set_layout(layout);
        assert!(app.visible().is_empty(), "{layout:?} on an empty reader");
    }
}

#[test]
fn facing_layout_steps_a_spread_at_a_time() {
    let mut app = open_pages(6);
    app.set_layout(Layout::Facing);

    app.go_to(0);
    app.next_page();
    assert_eq!(
        app.current_page(),
        2,
        "a spread advances to the next spread"
    );
    app.previous_page();
    assert_eq!(app.current_page(), 0, "and back again");

    app.go_to(4);
    assert!(!app.has_next(), "the last spread has nothing after it");
    assert!(app.has_previous());
}

#[test]
fn single_page_layout_still_steps_one_page() {
    let mut app = open_pages(3);
    app.set_layout(Layout::Single);
    app.go_to(0);
    app.next_page();
    assert_eq!(app.current_page(), 1);
    app.previous_page();
    assert_eq!(app.current_page(), 0);
    assert!(!app.has_previous(), "page 0 has nothing before it");
}

#[test]
fn changing_layout_keeps_the_page_the_reader_was_on() {
    let mut app = open_pages(6);
    app.go_to(3);
    app.set_layout(Layout::Facing);
    // Page 3 belongs to the spread [2, 3], so it is still on screen.
    assert!(app.visible().contains(&3), "the reader keeps its place");
    app.set_layout(Layout::Single);
    assert_eq!(app.current_page(), 3);
}

#[test]
fn a_query_finds_the_text_in_the_document() {
    let mut app = open();
    app.set_query("sOFD");
    assert!(!app.hits().is_empty(), "the fixture contains its own name");
    assert_eq!(
        app.hit_index(),
        Some(0),
        "the reader lands on the first hit rather than none"
    );
    assert!(!app.no_match());
}

#[test]
fn an_empty_query_finds_nothing_rather_than_everything() {
    let mut app = open();
    app.set_query("sOFD");
    assert!(!app.hits().is_empty());
    app.set_query("");
    assert!(app.hits().is_empty(), "an emptied box means done searching");
    assert_eq!(app.hit_index(), None);
    assert!(!app.no_match(), "no query is not the same as no match");
}

#[test]
fn a_query_with_no_match_says_so() {
    let mut app = open();
    app.set_query("zzzz-not-in-this-document");
    assert!(app.hits().is_empty());
    assert!(
        app.no_match(),
        "a query that found nothing is the case the notice is for"
    );
    assert!(!app.no_match_label().is_empty(), "and it is localized");
}

#[test]
fn searching_before_a_document_is_open_is_harmless() {
    let mut app = App::new();
    app.set_query("anything");
    assert!(app.hits().is_empty());
    assert!(!app.no_match(), "nothing to search is not a failed search");
}

#[test]
fn stepping_through_hits_wraps_around() {
    let mut app = open();
    app.set_query("sOFD");
    let count = app.hits().len();
    assert!(count >= 1);
    app.next_hit();
    assert_eq!(app.hit_index(), Some(1 % count), "next moves on");
    app.previous_hit();
    assert_eq!(app.hit_index(), Some(0), "previous comes back");
    app.previous_hit();
    assert_eq!(
        app.hit_index(),
        Some(count - 1),
        "and stepping back from the first wraps to the last"
    );
}

#[test]
fn jumping_to_a_hit_turns_to_its_page() {
    let mut app = open();
    app.set_query("sOFD");
    let page = app.hits()[0].page;
    app.go_to(0);
    app.show_hit(0);
    assert_eq!(
        app.current_page(),
        page,
        "a hit the reader cannot see is not a hit they found"
    );
    assert_eq!(app.hit_index(), Some(0));
}

#[test]
fn the_canvas_gets_rectangles_for_the_hits_on_a_page() {
    let mut app = open();
    app.set_query("sOFD");
    let page = app.hits()[0].page;
    let rects = app.hit_rects_on(page);
    assert!(!rects.is_empty(), "the page with the hit has rectangles");
    assert_eq!(
        rects.iter().filter(|(_, current)| *current).count(),
        1,
        "exactly one hit is the current one"
    );
    assert!(
        app.hit_rects_on(page + 999).is_empty(),
        "a page with no hits has none"
    );
}

#[test]
fn showing_a_hit_that_does_not_exist_changes_nothing() {
    let mut app = open();
    app.set_query("sOFD");
    let before = app.current_page();
    app.show_hit(9999);
    assert_eq!(app.current_page(), before, "an out-of-range hit is ignored");
}

#[test]
fn hiding_the_search_panel_keeps_the_results() {
    let mut app = open();
    app.set_query("sOFD");
    let count = app.hits().len();
    app.set_search_open(true);
    assert!(app.search_open());
    app.set_search_open(false);
    assert!(!app.search_open(), "the panel is hidden");
    assert_eq!(
        app.hits().len(),
        count,
        "reopening should not make the reader type it again"
    );
}

#[test]
fn a_hit_row_names_its_page_counting_from_one() {
    let mut app = open();
    app.set_query("sOFD");
    let row = app.hit_label(0, "sOFD");
    let page = app.hits()[0].page + 1;
    assert!(
        row.starts_with(&page.to_string()),
        "the row says which page it is on, got {row:?}"
    );
}

/// A reader over the genuinely signed fixture, whose proof verifies against
/// the bytes the engine reads. The fixture documents declare signatures they
/// cannot prove, so they test the other half.
fn open_signed() -> App {
    let mut app = App::new();
    let (bytes, _key) = sofd_testkit::signed_fixture();
    app.open_bytes(&bytes, Some("signed.ofd"))
        .expect("the signed fixture opens");
    app
}

#[test]
fn opening_the_signature_panel_verifies_and_remembers() {
    let mut app = open_signed();
    assert!(!app.signatures_open(), "the panel starts closed");
    assert!(
        app.signatures().is_empty(),
        "nothing is verified until the panel is asked for"
    );

    app.set_signatures_open(true);
    assert!(app.signatures_open());
    assert_eq!(
        app.signature_count(),
        1,
        "the fixture carries one signature"
    );
    assert_eq!(
        app.signatures()[0].verdict,
        sofd_reader::Verdict::Valid,
        "and the proof verifies against the document"
    );
    assert!(app.error().is_none(), "a good signature is not an error");
}

#[test]
fn a_verdict_is_worded_in_the_readers_language() {
    let mut app = open_signed();
    app.set_signatures_open(true);
    let report = app.signatures()[0].clone();
    let verdict = app.signature_status_label(&report);
    assert!(!verdict.is_empty(), "the verdict is localized");
    // A tampered signature must not read the same as a good one: the panel
    // exists to tell them apart.
    let broken = sofd_reader::SignatureReport {
        verdict: sofd_reader::Verdict::Tampered,
        ..report.clone()
    };
    assert_ne!(
        verdict,
        app.signature_status_label(&broken),
        "valid and invalid do not read alike"
    );
}

#[test]
fn verification_is_not_repeated_on_every_frame() {
    let mut app = open_signed();
    app.set_signatures_open(true);
    let first = app.signatures().to_vec();
    // Closing and reopening shows the same answer without re-reading the
    // package, which is what makes the panel cheap to leave in the toolbar.
    app.set_signatures_open(false);
    app.set_signatures_open(true);
    assert_eq!(app.signatures(), first.as_slice());
}

#[test]
fn closing_the_document_forgets_the_verdicts() {
    let mut app = open_signed();
    app.set_signatures_open(true);
    assert!(!app.signatures().is_empty());
    app.close();
    assert!(
        app.signatures().is_empty(),
        "a new document starts unverified"
    );
    assert!(!app.signatures_open(), "and the panel is closed with it");
}

#[test]
fn an_unsigned_document_reports_none_rather_than_failing() {
    // Unsigned is a fact about the document, not a fault in it, so the panel
    // must not put a red message on every ordinary file.
    let mut app = open();
    app.set_signatures_open(true);
    assert_eq!(app.signature_count(), 0, "the minimal fixture is unsigned");
    assert!(
        !app.signature_count_label().is_empty(),
        "and the count is still worded"
    );
    assert!(
        app.error().is_none(),
        "an unsigned document is not an error"
    );
}

#[test]
fn a_signature_that_cannot_be_proved_is_shown_rather_than_hidden() {
    // The features fixture declares a signature but carries no certificate, so
    // verification fails. A reader deciding whether to trust a document needs
    // to see that, which is the whole reason the panel exists.
    let mut app = App::new();
    let path =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../compliance/fixtures/features.ofd");
    app.open_path(&path).expect("the features fixture opens");
    app.set_signatures_open(true);
    assert_eq!(app.signature_count(), 1, "it declares a signature");
    assert_eq!(
        app.signatures()[0].verdict,
        sofd_reader::Verdict::Unverified,
        "an unprovable signature is reported, not silently dropped"
    );
    assert!(
        app.error().is_none(),
        "and it is a verdict, not a failure in the status bar"
    );
}

#[test]
fn verifying_with_nothing_open_is_harmless() {
    let mut app = App::new();
    app.set_signatures_open(true);
    assert_eq!(app.signature_count(), 0);
    assert!(app.error().is_none());
}

/// Export the pages on screen and wait for the files. Like rendering, export
/// is off the draw path, so the queue is pumped the way a frame pumps it, and
/// the bound fails loudly rather than hanging forever.
fn wait_for_export(app: &mut App) -> bool {
    // Export crosses a thread boundary like rendering, so the budget is wall
    // clock and a shared runner is given a full minute before the job is lost.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while std::time::Instant::now() < deadline {
        app.poll();
        if !app.is_exporting() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    false
}

/// A scratch directory that removes itself, so a test writes nothing permanent.
fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sofd-export-{name}"));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the scratch directory is creatable");
    dir
}

#[test]
fn an_export_covers_the_pages_on_screen() {
    let mut app = open_pages(6);
    app.set_layout(sofd_reader::viewer::Layout::Facing);
    app.go_to(2);
    assert_eq!(app.export_pages(), vec![2, 3], "a spread is both pages");
}

#[test]
fn asking_to_export_before_a_document_is_open_does_nothing() {
    // The menu disables the button, but the state must also refuse: a widget
    // that is reachable should not be able to start a job with no document.
    let mut app = App::new();
    app.request_export(sofd_reader::ExportFormat::Pdf);
    assert!(app.wants_export().is_none());
    assert!(app.take_export_request().is_none());
}

#[test]
fn the_export_request_is_handed_out_once() {
    let mut app = open();
    app.request_export(sofd_reader::ExportFormat::Pdf);
    let (format, pages) = app.take_export_request().expect("a request was made");
    assert_eq!(format, sofd_reader::ExportFormat::Pdf);
    assert_eq!(pages, vec![0]);
    assert!(
        app.take_export_request().is_none(),
        "a second read would ask the platform twice for the same file"
    );
}

#[test]
fn a_pdf_export_writes_the_file_the_pages_were_asked_for() {
    let dir = scratch("pdf");
    let out = dir.join("out.pdf");
    let mut app = open_pages(5);
    app.set_layout(sofd_reader::viewer::Layout::Facing);
    app.go_to(2);
    app.request_export(sofd_reader::ExportFormat::Pdf);
    let (_, pages) = app.take_export_request().expect("a request was made");
    app.export_to(sofd_reader::ExportFormat::Pdf, pages, out.clone());
    assert!(
        wait_for_export(&mut app),
        "the export worker never answered"
    );
    assert!(app.error().is_none(), "export failed: {:?}", app.error());
    let bytes = std::fs::read(&out).expect("the PDF was written");
    assert!(bytes.starts_with(b"%PDF-"), "not a PDF");
    let report = app
        .export_report()
        .expect("the reader reports what it wrote");
    assert_eq!(report.written, vec![out]);
    assert_eq!(report.pages, 2, "the spread went in as two pages");
    assert!(
        app.export_label().is_some_and(|label| label.contains("2")),
        "the status bar names the page count: {:?}",
        app.export_label()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_png_export_writes_one_file_per_page() {
    let dir = scratch("png");
    let mut app = open_pages(3);
    app.set_layout(sofd_reader::viewer::Layout::Facing);
    app.go_to(0);
    app.request_export(sofd_reader::ExportFormat::Png);
    let (_, pages) = app.take_export_request().expect("a request was made");
    app.export_to(sofd_reader::ExportFormat::Png, pages, dir.clone());
    assert!(
        wait_for_export(&mut app),
        "the export worker never answered"
    );
    assert!(app.error().is_none(), "export failed: {:?}", app.error());
    let written: Vec<String> = std::fs::read_dir(&dir)
        .expect("the directory is readable")
        .map(|entry| {
            entry
                .expect("an entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert_eq!(
        written.len(),
        2,
        "a two-page spread writes two files, got {written:?}"
    );
    assert!(
        written.iter().all(|name| name.starts_with("many-")),
        "files are named from the document: {written:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_export_of_no_pages_never_starts() {
    // Nothing to export is not a zero-page PDF; it is no export at all.
    let dir = scratch("empty");
    let mut app = open();
    app.export_to(
        sofd_reader::ExportFormat::Pdf,
        Vec::new(),
        dir.join("out.pdf"),
    );
    assert!(!app.is_exporting(), "an empty export was submitted anyway");
    assert!(app.export_report().is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn an_export_that_cannot_be_written_says_so_and_writes_nothing() {
    // A destination inside a file that is not a directory cannot be created, so
    // the worker must report it rather than fail silently.
    let dir = scratch("failure");
    let blocker = dir.join("blocker");
    std::fs::write(&blocker, b"not a directory").expect("the blocker is writable");
    let mut app = open_pages(2);
    app.request_export(sofd_reader::ExportFormat::Pdf);
    let (_, pages) = app.take_export_request().expect("a request was made");
    app.export_to(
        sofd_reader::ExportFormat::Pdf,
        pages,
        blocker.join("out.pdf"),
    );
    assert!(
        wait_for_export(&mut app),
        "the export worker never answered"
    );
    assert!(
        app.error().is_some(),
        "writing into a file as if it were a directory must be reported"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn closing_the_document_forgets_the_export_report() {
    let dir = scratch("close");
    let out = dir.join("out.pdf");
    let mut app = open_pages(2);
    app.request_export(sofd_reader::ExportFormat::Pdf);
    let (_, pages) = app.take_export_request().expect("a request was made");
    app.export_to(sofd_reader::ExportFormat::Pdf, pages, out);
    assert!(
        wait_for_export(&mut app),
        "the export worker never answered"
    );
    assert!(app.export_report().is_some());
    app.close();
    assert!(
        app.export_report().is_none(),
        "a report must not outlive the document it describes"
    );
    assert!(!app.is_exporting(), "and the worker goes with it");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_dropped_document_still_names_its_export_files() {
    // A dropped file has no path, so the stem has to fall back to something
    // rather than producing files called nothing at all.
    let dir = scratch("stem");
    let mut app = App::new();
    let bytes = sofd_testkit::multipage_archive(2);
    app.open_bytes(&bytes, None).expect("the fixture opens");
    app.request_export(sofd_reader::ExportFormat::Png);
    let (_, pages) = app.take_export_request().expect("a request was made");
    app.export_to(sofd_reader::ExportFormat::Png, pages, dir.clone());
    assert!(
        wait_for_export(&mut app),
        "the export worker never answered"
    );
    let written: Vec<String> = std::fs::read_dir(&dir)
        .expect("the directory is readable")
        .map(|entry| {
            entry
                .expect("an entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    assert!(
        written.iter().all(|name| name.starts_with("document-")),
        "a pathless document still names its files: {written:?}"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The keyboard, pressed without a window: what a reader can reach with a key.
mod keyboard {
    use super::{App, open_pages};
    use eframe::egui::{Key, Modifiers};
    use sofd_reader::shortcuts::{self, Action};

    /// No modifier at all.
    fn plain() -> Modifiers {
        Modifiers::default()
    }

    /// Ctrl held, as a shortcut uses it.
    fn ctrl() -> Modifiers {
        Modifiers {
            ctrl: true,
            ..Modifiers::default()
        }
    }

    /// Shift held, which is what distinguishes two meanings of one key.
    fn shift() -> Modifiers {
        Modifiers {
            shift: true,
            ..Modifiers::default()
        }
    }

    /// Resolve a key the way the window does, with a document open.
    fn press(key: Key, modifiers: Modifiers) -> Option<Action> {
        shortcuts::interpret(key, modifiers, true)
    }

    #[test]
    fn the_arrow_keys_and_page_keys_both_page() {
        // Two keys for one action, because a reader arrives with either habit.
        assert_eq!(press(Key::ArrowRight, plain()), Some(Action::NextPage));
        assert_eq!(press(Key::PageDown, plain()), Some(Action::NextPage));
        assert_eq!(press(Key::ArrowLeft, plain()), Some(Action::PreviousPage));
        assert_eq!(press(Key::PageUp, plain()), Some(Action::PreviousPage));
    }

    #[test]
    fn home_and_end_jump_to_the_ends_of_the_document() {
        assert_eq!(press(Key::Home, plain()), Some(Action::FirstPage));
        assert_eq!(press(Key::End, plain()), Some(Action::LastPage));
        assert_eq!(press(Key::Home, ctrl()), Some(Action::FirstPage));
        assert_eq!(press(Key::End, ctrl()), Some(Action::LastPage));
    }

    #[test]
    fn the_zoom_keys_step_the_resolution_and_reset_it() {
        assert_eq!(press(Key::Equals, plain()), Some(Action::ZoomIn));
        assert_eq!(press(Key::Minus, plain()), Some(Action::ZoomOut));
        assert_eq!(press(Key::Num0, plain()), Some(Action::ZoomReset));
    }

    #[test]
    fn f3_walks_the_hits_in_both_directions() {
        // One key, two meanings, distinguished by shift: the browser habit.
        assert_eq!(press(Key::F3, plain()), Some(Action::NextHit));
        assert_eq!(press(Key::F3, shift()), Some(Action::PreviousHit));
    }

    #[test]
    fn the_command_keys_reach_the_expensive_actions() {
        assert_eq!(press(Key::O, ctrl()), Some(Action::Open));
        assert_eq!(press(Key::S, ctrl()), Some(Action::ExportPdf));
        assert_eq!(press(Key::P, ctrl()), Some(Action::Print));
        assert_eq!(press(Key::F, ctrl()), Some(Action::ToggleSearch));
        assert_eq!(press(Key::Slash, ctrl()), Some(Action::FocusSearch));
    }

    #[test]
    fn the_annotation_keys_walk_the_marks() {
        // One key forward, one back, as a reader paging through notes expects.
        assert_eq!(press(Key::J, plain()), Some(Action::NextAnnotation));
        assert_eq!(press(Key::K, plain()), Some(Action::PreviousAnnotation));
        assert_eq!(press(Key::K, ctrl()), Some(Action::ToggleAnnotations));
    }

    #[test]
    fn a_key_with_no_meaning_is_none_rather_than_a_guess() {
        assert_eq!(press(Key::Num7, plain()), None);
        assert_eq!(press(Key::Q, plain()), None);
        assert_eq!(press(Key::A, plain()), None);
    }

    #[test]
    fn page_keys_do_nothing_with_no_document_open() {
        // Not an error the reader should hear about: there is no page to turn.
        assert_eq!(shortcuts::interpret(Key::ArrowRight, plain(), false), None);
        assert_eq!(shortcuts::interpret(Key::End, plain(), false), None);
    }

    #[test]
    fn opening_and_searching_still_work_with_nothing_open() {
        // The two things a reader can do before opening anything.
        assert_eq!(
            shortcuts::interpret(Key::O, ctrl(), false),
            Some(Action::Open)
        );
        assert_eq!(
            shortcuts::interpret(Key::F, ctrl(), false),
            Some(Action::ToggleSearch)
        );
    }

    #[test]
    fn escape_leaves_the_search_box_before_it_closes_the_panel() {
        // A caret the reader cannot get out of is the one real trap here.
        assert_eq!(
            shortcuts::escape_target(true, true),
            Some(Action::FocusSearch)
        );
        assert_eq!(
            shortcuts::escape_target(true, false),
            Some(Action::ToggleSearch)
        );
        assert_eq!(shortcuts::escape_target(false, false), None);
    }

    #[test]
    fn a_shortcut_and_the_button_it_mirrors_reach_the_same_state() {
        // The point of resolving intent: a key and a click cannot drift apart.
        let mut by_key = App::new();
        let mut by_click = App::new();
        for app in [&mut by_key, &mut by_click] {
            *app = open_pages(8);
        }
        by_key.apply(Action::NextPage);
        by_click.next_page();
        assert_eq!(by_key.current_page(), by_click.current_page());

        by_key.apply(Action::LastPage);
        by_click.go_to(7);
        assert_eq!(by_key.current_page(), by_click.current_page());

        by_key.apply(Action::FirstPage);
        by_click.go_to(0);
        assert_eq!(by_key.current_page(), by_click.current_page());
    }

    #[test]
    fn paging_by_key_steps_by_the_layouts_stride() {
        // Facing mode advances a spread, so the key must not land mid-spread.
        let mut app = open_pages(8);
        app.set_layout(sofd_reader::viewer::Layout::Facing);
        app.go_to(2);
        app.apply(Action::NextPage);
        assert_eq!(app.current_page(), 4, "a key advances a whole spread");
    }

    #[test]
    fn the_zoom_keys_stop_at_the_permitted_ends() {
        let mut app = open_pages(2);
        for _ in 0..200 {
            app.apply(Action::ZoomIn);
        }
        assert_eq!(
            app.dpi(),
            sofd_reader::viewer::MAX_DPI,
            "zooming in forever must stop"
        );
        for _ in 0..200 {
            app.apply(Action::ZoomOut);
        }
        assert_eq!(
            app.dpi(),
            sofd_reader::viewer::MIN_DPI,
            "zooming out forever must stop"
        );
        app.apply(Action::ZoomReset);
        assert_eq!(app.dpi(), sofd_reader::viewer::DEFAULT_DPI);
    }

    #[test]
    fn the_toggle_keys_open_and_close_the_panels() {
        let mut app = open_pages(2);
        assert!(!app.search_open());
        app.apply(Action::ToggleSearch);
        assert!(app.search_open());
        app.apply(Action::ToggleSearch);
        assert!(!app.search_open());

        app.apply(Action::ToggleSignatures);
        assert!(app.signatures_open());
        app.apply(Action::ToggleSignatures);
        assert!(!app.signatures_open());
    }
}

/// A reader over a document whose first `annotated` pages carry marks.
fn open_annotated(pages: usize, annotated: usize) -> App {
    let mut app = App::new();
    let bytes = sofd_testkit::annotated_archive(pages, annotated);
    app.open_bytes(&bytes, Some("marked.ofd"))
        .expect("the fixture opens");
    app
}

#[test]
fn the_annotation_panel_lists_the_marks_in_page_order() {
    let mut app = open_annotated(3, 2);
    app.set_annotations_open(true);
    assert!(app.annotations_open());
    assert_eq!(app.annotation_count(), 2);
    assert_eq!(
        app.annotation_label(0),
        "1  Highlight  note 0",
        "a row names its page, its kind and its note"
    );
    assert_eq!(app.annotations()[1].page, 1);
}

#[test]
fn a_reader_with_nothing_open_lists_no_annotations() {
    // The panel is disabled with no document, but the state must answer too.
    let mut app = App::new();
    app.set_annotations_open(true);
    assert_eq!(app.annotation_count(), 0);
    assert!(app.error().is_none());
}

#[test]
fn a_document_with_no_marks_says_so_rather_than_failing() {
    let mut app = open();
    app.set_annotations_open(true);
    assert_eq!(app.annotation_count(), 0);
    assert!(
        !app.no_annotations_label().is_empty(),
        "the notice comes from the catalogue"
    );
    assert!(app.error().is_none(), "no marks is not a failure");
}

#[test]
fn choosing_an_annotation_turns_to_the_page_it_marks() {
    // A note the reader cannot see is not a note they found, so picking one
    // moves the document as well as the selection.
    let mut app = open_annotated(4, 3);
    app.set_annotations_open(true);
    assert_eq!(app.current_page(), 0);
    app.show_annotation(2);
    assert_eq!(app.current_page(), 2);
    assert_eq!(app.annotation_index(), Some(2));
    assert_eq!(app.current_annotation().map(|a| a.page), Some(2));
}

#[test]
fn stepping_walks_the_marks_in_both_directions_and_wraps() {
    let mut app = open_annotated(4, 3);
    app.set_annotations_open(true);
    app.next_annotation();
    assert_eq!(app.annotation_index(), Some(0));
    app.next_annotation();
    app.next_annotation();
    assert_eq!(app.annotation_index(), Some(2), "the last mark");
    app.next_annotation();
    assert_eq!(app.annotation_index(), Some(0), "it wraps to the first");
    app.previous_annotation();
    assert_eq!(app.annotation_index(), Some(2), "and back to the last");
}

#[test]
fn stepping_through_marks_opens_the_panel() {
    // Stepping blind would move the document for no visible reason, so the
    // key that walks the marks opens the list as well.
    let mut app = open_annotated(3, 2);
    assert!(!app.annotations_open());
    app.apply(sofd_reader::shortcuts::Action::NextAnnotation);
    assert!(app.annotations_open());
    assert_eq!(app.annotation_index(), Some(0));
}

#[test]
fn only_the_marks_on_one_page_are_offered_to_the_canvas() {
    let mut app = open_annotated(3, 2);
    app.set_annotations_open(true);
    app.show_annotation(1);
    let on_first = app.annotations_on(0);
    assert_eq!(on_first.len(), 1);
    assert!(
        !on_first[0].1,
        "the mark on page one is not the one being visited"
    );
    let on_second = app.annotations_on(1);
    assert_eq!(on_second.len(), 1);
    assert!(on_second[0].1, "this is the one being visited");
    assert!(app.annotations_on(2).is_empty());
}

#[test]
fn closing_the_document_forgets_the_annotations() {
    let mut app = open_annotated(2, 2);
    app.set_annotations_open(true);
    assert_eq!(app.annotation_count(), 2);
    app.close();
    assert!(!app.annotations_open(), "the panel goes with the document");
    assert_eq!(app.annotation_count(), 0);
    assert_eq!(app.annotation_index(), None);
}

/// Lay out and print the pages on screen, and wait for the sheets. Like
/// export, printing is off the draw path, so the queue is pumped the way a
/// frame pumps it and the bound fails loudly rather than hanging forever.
fn wait_for_print(app: &mut App) -> bool {
    // The budget is wall clock, not iterations: a shared runner can stall the
    // worker well past what a laptop sees, so wait a full minute before
    // calling a job lost. The polling itself stays cheap.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while std::time::Instant::now() < deadline {
        app.poll();
        if !app.is_printing() {
            return true;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    false
}

#[test]
fn a_print_job_covers_the_pages_on_screen() {
    // Printing what the reader is looking at is the whole point of the menu
    // entry: a spread prints as two pages, not as the whole document.
    let mut app = open_pages(6);
    app.set_layout(sofd_reader::viewer::Layout::Facing);
    app.go_to(2);
    assert_eq!(app.print_pages(), vec![2, 3], "a spread is both pages");
}

#[test]
fn asking_to_print_before_a_document_is_open_does_nothing() {
    // The button is disabled with nothing open, but the state must refuse too:
    // a reachable widget must not be able to start a job with no document.
    let mut app = App::new();
    app.request_print();
    assert!(!app.wants_print());
    assert!(app.take_print_request().is_none());
}

#[test]
fn the_print_request_is_handed_out_once() {
    let mut app = open();
    app.request_print();
    assert_eq!(
        app.take_print_request(),
        Some(vec![0]),
        "the request names the pages to lay out"
    );
    assert!(
        app.take_print_request().is_none(),
        "a second read would start the job twice"
    );
}

#[test]
fn printing_the_pages_on_screen_produces_rasters_for_each_of_them() {
    let mut app = open_pages(4);
    app.set_layout(sofd_reader::viewer::Layout::Facing);
    app.go_to(0);
    let pages = app.print_pages();
    let settings = app.print_settings();
    app.print_to(pages, settings);
    assert!(wait_for_print(&mut app), "the print worker never answered");
    assert!(app.error().is_none(), "printing failed: {:?}", app.error());
    let report = app.print_report().expect("the reader reports the job");
    assert_eq!(report.sheets.len(), 2, "the spread laid out as two pages");
    assert_eq!(report.sheet_count, 2, "single-sided, so one sheet each");
    assert!(
        report
            .sheets
            .iter()
            .all(|sheet| sheet.width > 0 && sheet.height > 0),
        "every sheet rasterized: {:?}",
        report.sheets
    );
    // The plan carries the page each sheet came from, so a caller can tell
    // which page landed where rather than only how many there were.
    let planned: Vec<usize> = report.sheets.iter().map(|s| s.page_index).collect();
    assert_eq!(planned, vec![0, 1]);
}

#[test]
fn a_print_job_of_no_pages_never_starts() {
    // Nothing to print is not a zero-page job; it is no job at all.
    let mut app = open();
    app.print_to(Vec::new(), app.print_settings());
    assert!(!app.is_printing(), "an empty job was submitted anyway");
    assert!(app.print_report().is_none());
}

#[test]
fn closing_the_document_forgets_the_print_report() {
    let mut app = open_pages(2);
    app.print_to(app.print_pages(), app.print_settings());
    assert!(wait_for_print(&mut app), "the print worker never answered");
    assert!(app.print_report().is_some());
    app.close();
    assert!(
        app.print_report().is_none(),
        "a report must not outlive the document it describes"
    );
    assert!(!app.is_printing(), "and the worker goes with it");
}

#[test]
fn the_status_bar_names_the_pages_the_sheets_and_the_paper() {
    let mut app = open_pages(2);
    app.set_layout(sofd_reader::viewer::Layout::Facing);
    app.go_to(0);
    app.print_to(app.print_pages(), app.print_settings());
    assert!(wait_for_print(&mut app), "the print worker never answered");
    let label = app.print_label().expect("the job reported");
    // Two pages on two sheets, on the paper the document itself declares. The
    // counts are separate because they differ: a duplexed job would say one.
    assert!(
        label.contains("2 page") && label.contains("2 sheet"),
        "the label names both counts: {label}"
    );
    assert!(
        !label.contains('$'),
        "the label is a rendered sentence, not a template: {label}"
    );
}

#[test]
fn the_paper_names_come_from_the_catalogue() {
    // A paper size is a word like any other, so it is translated rather than
    // hardcoded in the reader's source. The reader owns its catalogue, so the
    // bundle it builds is what holds these keys.
    use sofd_i18n::{FluentValue, Locale};
    use sofd_reader::ReaderMessage;
    let english = sofd_reader::message::bundle_for(Locale::en_us());
    assert_eq!(english.text_key(ReaderMessage::PaperA4.key()), "A4");
    assert!(
        !english
            .text_key_with(
                ReaderMessage::PrintDone.key(),
                [
                    ("pages", FluentValue::from(1)),
                    ("sheets", FluentValue::from(1)),
                    ("paper", FluentValue::from("A4")),
                ]
            )
            .is_empty()
    );
}

/// Switching language, which has to redraw every label from the catalogue.
mod language {
    use super::{App, open_pages};
    use sofd_i18n::Locale;

    #[test]
    fn switching_locale_changes_every_label() {
        // The reader's own strings and the engine's share one catalogue, so a
        // locale switch that only moved the reader's labels would be a lie.
        let mut app = App::new();
        let english = app.page_label();
        app.set_locale(Locale::zh_cn());
        let chinese = app.page_label();
        assert_ne!(english, chinese, "the page label did not follow");
        // The page counter in Chinese carries the character for "page".
        assert!(
            chinese.contains('页'),
            "the label did not follow the catalogue: {chinese}"
        );
    }

    #[test]
    fn both_bundled_languages_are_offered() {
        let app = App::new();
        let tags: Vec<String> = app
            .locales()
            .iter()
            .map(|locale| locale.tag().to_string())
            .collect();
        assert!(
            tags.iter().any(|tag| tag == "en-US"),
            "English is missing: {tags:?}"
        );
        assert!(
            tags.iter().any(|tag| tag == "zh-CN"),
            "Chinese is missing: {tags:?}"
        );
    }

    #[test]
    fn a_locale_is_named_in_its_own_language() {
        // Each entry is an endonym, written in the language it names rather
        // than translated into the current one. Both catalogues carry the same
        // two names, which is what makes them endonyms rather than labels.
        let english = App::new();
        let names: Vec<String> = english
            .locales()
            .iter()
            .map(|locale| english.locale_name(locale))
            .collect();
        assert!(names.contains(&"English".to_string()), "{names:?}");
        assert!(
            names.contains(&"简体中文".to_string()),
            "Chinese must be offered in Chinese whatever the menu language is: {names:?}"
        );

        // Switching the interface language must not translate the names, or a
        // reader would lose the one entry they were looking for.
        let mut switched = App::new();
        switched.set_locale(Locale::zh_cn());
        for locale in switched.locales() {
            assert_eq!(
                switched.locale_name(&locale),
                english.locale_name(&locale),
                "the name of {} changed with the interface language",
                locale.tag()
            );
        }
    }

    #[test]
    fn switching_to_the_locale_already_in_force_changes_nothing() {
        let mut app = App::new();
        let before = app.page_label();
        let current = app.locale();
        app.set_locale(current);
        assert_eq!(app.page_label(), before);
    }

    #[test]
    fn switching_language_keeps_the_rendered_pages() {
        // Pages are pixels and a label is a word: a translated interface does
        // not make a cached raster wrong, so throwing it away would be waste.
        let mut app = open_pages(3);
        app.page_image(0).expect("the request is accepted");
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
        while std::time::Instant::now() < deadline {
            app.poll();
            if app.cached(0).is_some() {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(2));
        }
        assert!(app.cached(0).is_some(), "the page should have landed");
        app.set_locale(sofd_i18n::Locale::zh_cn());
        assert!(
            app.cached(0).is_some(),
            "changing language must not throw away rendered pages"
        );
    }
}

mod comparison {
    use super::App;

    /// A reader over a one-page document, so a two-page one reads as a change.
    fn one_page() -> App {
        let mut app = App::new();
        let bytes = sofd_testkit::multipage_archive(1);
        app.open_bytes(&bytes, Some("rev-a.ofd"))
            .expect("the first revision opens");
        app
    }

    #[test]
    fn a_fresh_reader_has_no_comparison() {
        let app = App::new();
        assert!(app.comparison().is_none());
        assert!(!app.comparison_open());
    }

    #[test]
    fn an_added_page_reads_as_a_change() {
        let mut app = one_page();
        app.compare_bytes(&sofd_testkit::multipage_archive(2))
            .expect("the second revision opens");
        let comparison = app.comparison().expect("a verdict");
        assert!(!comparison.is_identical());
        assert_eq!(comparison.changed_pages(), 1);
        assert!(app.comparison_open(), "comparing opens the panel");
    }

    #[test]
    fn the_same_revision_reads_as_identical() {
        let mut app = one_page();
        app.compare_bytes(&sofd_testkit::multipage_archive(1))
            .expect("the same revision opens");
        let comparison = app.comparison().expect("a verdict");
        assert!(comparison.is_identical());
        assert_eq!(app.comparison_changes(), Vec::new());
    }

    #[test]
    fn comparing_with_nothing_open_is_an_error() {
        let mut app = App::new();
        let err = app
            .compare_bytes(&sofd_testkit::multipage_archive(1))
            .expect_err("there is nothing to compare");
        assert!(err.message.contains("no document"), "got {}", err.message);
    }

    #[test]
    fn opening_a_new_document_drops_the_old_comparison() {
        let mut app = one_page();
        app.compare_bytes(&sofd_testkit::multipage_archive(2))
            .expect("compare");
        assert!(app.comparison().is_some());
        app.close();
        assert!(
            app.comparison().is_none(),
            "a comparison of a closed document is stale"
        );
    }
}

/// A document the platform hands over outside a frame: Android reads the bytes
/// out of the activity's intent before the event loop starts, and has nowhere
/// to put them until the first frame runs.
mod handed_over {
    use super::*;

    #[test]
    fn a_document_handed_over_lands_on_the_next_frame() {
        let mut app = App::new();
        let bytes = sofd_testkit::multipage_archive(2);
        app.hand_over(DroppedFile::Bytes {
            name: "shared.ofd".to_owned(),
            bytes,
        });
        assert!(
            !app.is_open(),
            "the hand-over is stored, not opened behind the frame"
        );

        assert!(
            app.take_handed_over(),
            "the frame takes what was handed over"
        );
        assert!(app.is_open(), "the document is open after the first frame");
        assert_eq!(app.page_count(), 2);
        assert!(
            app.title().contains("shared.ofd"),
            "the name travels with the bytes: {}",
            app.title()
        );
        assert_eq!(
            app.error(),
            None,
            "a document that opened is not also an error"
        );
    }

    #[test]
    fn taking_twice_opens_once() {
        let mut app = App::new();
        app.hand_over(DroppedFile::Bytes {
            name: "shared.ofd".to_owned(),
            bytes: sofd_testkit::multipage_archive(1),
        });
        assert!(app.take_handed_over());
        assert!(
            !app.take_handed_over(),
            "the second frame has nothing left to open"
        );
        assert_eq!(app.page_count(), 1);
    }

    #[test]
    fn nothing_handed_over_is_not_a_failure() {
        let mut app = App::new();
        assert!(!app.take_handed_over());
        assert_eq!(app.error(), None);
    }

    /// A share of something that is not a document is reported the way a drop
    /// of one is, rather than opening an empty window with no explanation.
    #[test]
    fn a_share_that_is_not_a_document_is_an_error() {
        let mut app = App::new();
        app.hand_over(DroppedFile::Bytes {
            name: "notes.txt".to_owned(),
            bytes: b"not an archive".to_vec(),
        });
        app.take_handed_over();
        assert!(!app.is_open());
        assert!(app.error().is_some(), "the failure reaches the status bar");
    }
}
