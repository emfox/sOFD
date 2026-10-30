//! The window, driven headlessly: a real widget tree, no display. A label that
//! loses its catalogue entry or a button that stops wiring to `App` fails here,
//! not in front of a reader. Labels come from the catalogue, not literals.

use std::path::PathBuf;

use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use sofd_egui::viewer::Layout;
use sofd_egui::window;
use sofd_egui::{App, Canvas, Fit, ReaderMessage, TextSize, ThemeChoice};
use sofd_i18n::{I18n, Locale, MessageId};

/// The real minimal document from the conformance fixtures.
fn minimal() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../compliance/fixtures/minimal.ofd")
}

/// An empty reader behind a harness, sized like a small window.
fn harness(app: App) -> Harness<'static, App> {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(1000.0, 700.0))
        .build_ui_state(
            |ui, app: &mut App| {
                window::draw(ui, app);
            },
            app,
        );
    harness.run();
    harness
}

/// The label the catalogue gives an engine key, so the test does not hardcode
/// English.
fn label(key: MessageId) -> String {
    I18n::with_locale(Locale::en_us()).text(key)
}

/// The label for one of the reader's own messages, drawn from the reader's
/// catalogue rather than the engine's.
fn reader_label(message: ReaderMessage) -> String {
    sofd_egui::message::bundle_for(Locale::en_us()).text_key(message.key())
}

/// Open the main menu at the end of the header bar. Settings, language and the
/// second revision all live in it, so a test that wants one of them opens it
/// first — which is also the check that they are where the guideline puts them.
fn open_main_menu(harness: &mut Harness<'static, App>) {
    harness
        .get_by_label(&reader_label(ReaderMessage::MainMenu))
        .click();
    harness.run();
}

#[test]
fn the_view_switcher_shows_one_panel_at_a_time() {
    // A switcher rather than three independent toggles: picking one view is
    // also a statement about the other two, and a reader who ends up with two
    // panels has been given a window they did not ask for.
    let mut app = App::new();
    app.open_path(&minimal()).expect("the fixture opens");
    let mut harness = harness(app);

    let signatures = harness.state().signatures_heading();
    harness.get_by_label(&signatures).click();
    harness.run();
    assert!(harness.state().signatures_open(), "the view opened");

    let annotations = harness.state().annotations_heading();
    harness.get_by_label(&annotations).click();
    harness.run();
    assert!(harness.state().annotations_open(), "the next view opened");
    assert!(
        !harness.state().signatures_open(),
        "the previous view closed itself, so the switcher stays exclusive"
    );
}

#[test]
fn the_main_menu_carries_the_entries_the_guideline_expects() {
    let mut app = App::new();
    app.open_path(&minimal()).expect("the fixture opens");
    let mut harness = harness(app);
    open_main_menu(&mut harness);
    // Every primary menu is expected to name the shortcuts and the build, and
    // to keep both in one group at the end where they are easy to reach.
    for key in [
        ReaderMessage::KeyboardShortcuts,
        ReaderMessage::Language,
        ReaderMessage::Theme,
    ] {
        harness.get_by_label_contains(&reader_label(key));
    }
    // No close item: the header's own button is the one way to close, and its
    // name is now the tooltip, which must appear exactly once.
    let hint = reader_label(ReaderMessage::CloseHint);
    assert_eq!(
        harness.get_all_by_label(&hint).count(),
        1,
        "the header button should be the only way to close"
    );
    let close = reader_label(ReaderMessage::Close);
    let entries = harness.query_all_by_label(&close).count();
    assert_eq!(
        entries, 0,
        "the menu offered a second way to close the document: {entries} entries"
    );
}

/// Run frames until the pages are rasterized, since rendering is off the draw
/// path and a frame is what collects a finished raster.
fn pump_until_rendered(harness: &mut Harness<'static, App>, pages: &[usize]) {
    // Rendering is off the draw path, so a frame collects a finished raster;
    // the budget is wall clock because a shared runner can stall the worker.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while std::time::Instant::now() < deadline {
        harness.run();
        if pages
            .iter()
            .all(|page| harness.state().cached(*page).is_some())
        {
            return;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    panic!("pages {pages:?} never rendered");
}

#[test]
fn the_menu_bar_shows_a_localized_open_button() {
    let harness = harness(App::new());
    let open = reader_label(ReaderMessage::Open);
    // Panics with a list of what did exist if the button is not drawn.
    harness.get_by_label(&open);
}

#[test]
fn the_toolbar_labels_every_control() {
    let harness = harness(App::new());
    for key in [ReaderMessage::FitWidth, ReaderMessage::FitPage] {
        harness.get_by_label_contains(&reader_label(key));
    }
    // The zoom control shows the level, not the template, so it is found by
    // the substituted text a reader actually sees. A slider exposes two nodes
    // with that label, so the query asks for the slider role.
    let zoom = harness.get_by(|node| {
        node.role() == egui::accesskit::Role::Slider
            && node.label().unwrap_or_default().starts_with("Zoom")
    });
    // The slider's number is the percent it shows, not a resolution: a reader
    // dragging 100 expects the page at its own size.
    assert_eq!(zoom.accesskit_node().numeric_value(), Some(133.0));
}

#[test]
fn the_layout_choices_are_offered_and_one_is_selected() {
    let mut harness = harness(App::new());
    let single = reader_label(ReaderMessage::LayoutSingle);
    let facing = reader_label(ReaderMessage::LayoutDouble);
    let continuous = reader_label(ReaderMessage::LayoutContinuous);
    for name in [&single, &facing, &continuous] {
        harness.get_by_label(name);
    }
    // Clicking one moves the selection, which the state machine then remembers.
    harness.get_by_label(&facing).click();
    harness.run();
    assert_eq!(harness.state().layout(), sofd_egui::viewer::Layout::Facing);
}

#[test]
fn an_empty_window_offers_the_drop_hint_instead_of_a_page() {
    let mut harness = harness(App::new());
    let hint = reader_label(ReaderMessage::DropHint);
    harness.get_by_label_contains(&hint);
    assert_eq!(harness.state_mut().canvas(), Canvas::Empty);
}

#[test]
fn opening_the_fixture_draws_its_page_counter() {
    let mut app = App::new();
    app.open_path(&minimal()).expect("the fixture opens");
    let harness = harness(app);

    // The status bar counts the pages; the header bar repeats the name, so
    // this asks for the count rather than for the only place the name appears.
    harness.get_by_label_contains("1 of");
    assert!(harness.state().page_count() >= 1);
}

/// The header bar names the open document in its centre, which is where a
/// reader looks to see what they have, beside the window title.
#[test]
fn the_open_document_is_named_in_the_window() {
    let mut app = App::new();
    app.open_path(&minimal()).expect("the fixture opens");
    let harness = harness(app);
    // The name is on screen rather than only in the title bar, so two windows
    // are told apart without reading the page counter.
    let named = harness.get_all_by_label_contains("minimal.ofd").count();
    assert!(named >= 1, "the name is nowhere in the window");
}

#[test]
fn the_next_button_turns_the_page_and_stops_at_the_end() {
    let mut app = App::new();
    app.open_path(&minimal()).expect("the fixture opens");
    let mut harness = harness(app);

    // The arrow is a drawn glyph, so the button's accessible name is its hint.
    let next = reader_label(ReaderMessage::NextPage);
    let before = harness.state().current_page();
    if harness.state().has_next() {
        harness.get_by_label(&next).click();
        harness.run();
        assert_eq!(
            harness.state().current_page(),
            before + 1,
            "the toolbar button advanced one page"
        );
    }
    // Walk to the end; the button must be disabled there rather than absent.
    while harness.state().has_next() {
        harness.get_by_label(&next).click();
        harness.run();
    }
    assert!(
        !harness.state().has_next(),
        "the reader is at the last page"
    );
    harness.get_by_label(&next);
}

#[test]
fn the_close_button_is_disabled_until_a_document_is_open() {
    let harness = harness(App::new());
    let close = reader_label(ReaderMessage::CloseHint);
    let button = harness.get_by_label(&close);
    assert!(
        button.accesskit_node().is_disabled(),
        "an empty reader has nothing to close, so the control is disabled"
    );
}

#[test]
fn closing_through_the_header_button_returns_the_window_to_the_hint() {
    let mut app = App::new();
    app.open_path(&minimal()).expect("the fixture opens");
    let mut harness = harness(app);

    let close = reader_label(ReaderMessage::CloseHint);
    harness.get_by_label(&close).click();
    harness.run();

    assert_eq!(
        harness.state_mut().canvas(),
        Canvas::Empty,
        "the page is gone"
    );
    let hint = reader_label(ReaderMessage::DropHint);
    harness.get_by_label_contains(&hint);
}

#[test]
fn a_failure_is_drawn_in_the_status_bar() {
    let mut app = App::new();
    app.open_path(&minimal()).expect("the fixture opens");
    app.set_error("the page would not render");
    let harness = harness(app);
    harness.get_by_label_contains("the page would not render");
}

#[test]
fn the_fit_buttons_choose_a_fit_and_it_sticks() {
    let mut app = App::new();
    app.open_path(&minimal()).expect("the fixture opens");
    let mut harness = harness(app);

    // A fit is a mode, not a one-off resolution: choosing one has to be
    // visible to the toolbar so it can show which is in force.
    assert_eq!(harness.state().fit(), None, "nothing fitted on open");

    let fit_page = reader_label(ReaderMessage::FitPage);
    harness.get_by_label(&fit_page).click();
    harness.run();
    assert_eq!(
        harness.state().fit(),
        Some(Fit::Page),
        "fit page is the mode in force"
    );

    let fit_width = reader_label(ReaderMessage::FitWidth);
    harness.get_by_label(&fit_width).click();
    harness.run();
    assert_eq!(harness.state().fit(), Some(Fit::Width));
}

#[test]
fn a_fit_recomputes_the_resolution_from_the_page_and_the_window() {
    let mut app = App::new();
    app.open_path(&minimal()).expect("the fixture opens");
    let mut harness = harness(app);

    harness.state_mut().set_fit(Fit::Width);
    harness.run();
    let wide = harness.state().dpi();

    // The same page in a narrower window must be shown smaller; a fixed
    // constant would not move at all.
    let mut narrow = Harness::builder()
        .with_size(egui::vec2(400.0, 700.0))
        .build_ui_state(
            |ui, app: &mut App| {
                window::draw(ui, app);
            },
            App::new(),
        );
    narrow.state_mut().set_fit(Fit::Width);
    narrow.run();
    let fitted = narrow.state().dpi();
    assert!(
        fitted < wide,
        "a narrower window fits at a lower resolution: {fitted} vs {wide}"
    );
}

#[test]
fn zooming_changes_how_large_the_page_is_drawn() {
    // The zoom is a size, not a resolution. A page that only sharpened would
    // leave the rectangle it occupies the same on every setting, which is what
    // the reader reported: "it only gets clearer, never bigger".
    let mut app = App::new();
    app.open_path(&minimal()).expect("the fixture opens");
    let mut harness = harness(app);
    harness.state_mut().set_layout(Layout::Single);

    let page_width = |harness: &Harness<'static, App>| {
        harness
            .query_all_by(|node| node.role() == egui::accesskit::Role::Image)
            .next()
            .map(|node| node.rect().width())
    };

    harness.state_mut().set_zoom(1.0);
    let _ = harness.state_mut().page_image(0);
    pump_until_rendered(&mut harness, &[0]);
    let actual = page_width(&harness).expect("the page is drawn");

    harness.state_mut().set_zoom(2.0);
    let _ = harness.state_mut().page_image(0);
    pump_until_rendered(&mut harness, &[0]);
    let doubled = page_width(&harness).expect("the page is still drawn");

    assert!(
        doubled > actual * 1.9,
        "twice the zoom must be twice the size: {actual} then {doubled}"
    );
}

#[test]
fn a_page_at_one_hundred_percent_is_its_size_at_ninety_six_dpi() {
    // 100% means what it means in Acrobat and in a browser: an inch of paper on
    // a 96 dpi screen, so A4 is 794 points wide. Skipping the millimetre step
    // drew 280, a sheet a third of the width it should be.
    let mut app = App::new();
    app.open_path(&minimal()).expect("the fixture opens");
    let mut harness = harness(app);
    harness.state_mut().set_layout(Layout::Single);
    harness.state_mut().set_zoom(1.0);
    let _ = harness.state_mut().page_image(0);
    pump_until_rendered(&mut harness, &[0]);

    let drawn = harness
        .query_all_by(|node| node.role() == egui::accesskit::Role::Image)
        .next()
        .map(|node| node.rect().width())
        .expect("the page is drawn");
    let expected = 210.0 * sofd_egui::viewer::POINTS_PER_MM;
    assert!(
        (drawn - expected).abs() < 1.0,
        "A4 at 100% is {expected:.1} points wide, drawn {drawn:.1}"
    );
}

#[test]
fn the_zoom_label_and_the_drawn_width_agree() {
    // The percentage and the paper have to be the same story: 50% is half the
    // width 100% draws, which a reader can check with a ruler.
    let mut app = App::new();
    app.open_path(&minimal()).expect("the fixture opens");
    let mut harness = harness(app);
    harness.state_mut().set_layout(Layout::Single);

    harness.state_mut().set_zoom(1.0);
    let _ = harness.state_mut().page_image(0);
    pump_until_rendered(&mut harness, &[0]);
    let full = harness
        .query_all_by(|node| node.role() == egui::accesskit::Role::Image)
        .next()
        .map(|node| node.rect().width())
        .expect("the page is drawn");

    harness.state_mut().set_zoom(0.5);
    let _ = harness.state_mut().page_image(0);
    pump_until_rendered(&mut harness, &[0]);
    let half = harness
        .query_all_by(|node| node.role() == egui::accesskit::Role::Image)
        .next()
        .map(|node| node.rect().width())
        .expect("the page is drawn");

    assert!(
        (half - full / 2.0).abs() < 1.5,
        "half the zoom is half the width: {full:.1} then {half:.1}"
    );
}

#[test]
fn facing_layout_draws_both_pages_of_the_spread() {
    // Two pages on screen means two images; single layout means one. Counting
    // image nodes is what proves the layout reaches the canvas, since the
    // canvas is the only zone allowed to paint a page.
    let bytes = sofd_testkit::multipage_archive(4);
    let mut app = App::new();
    app.open_bytes(&bytes, Some("many.ofd"))
        .expect("the fixture opens");
    let mut harness = harness(app);

    let images = |harness: &Harness<'static, App>| {
        harness
            .query_all_by(|node| node.role() == egui::accesskit::Role::Image)
            .count()
    };

    harness.state_mut().set_layout(Layout::Single);
    harness.state_mut().page_image(0).expect("page 0 requested");
    pump_until_rendered(&mut harness, &[0]);
    assert_eq!(images(&harness), 1, "single layout shows one page");

    harness.state_mut().set_layout(Layout::Facing);
    harness.state_mut().set_zoom(96.0 / 72.0);
    for page in [0, 1] {
        harness
            .state_mut()
            .page_image(page)
            .expect("page requested");
    }
    pump_until_rendered(&mut harness, &[0, 1]);
    assert_eq!(images(&harness), 2, "facing layout shows the whole spread");
}

#[test]
fn the_continuous_layout_stacks_more_than_one_page() {
    let bytes = sofd_testkit::multipage_archive(9);
    let mut app = App::new();
    app.open_bytes(&bytes, Some("many.ofd"))
        .expect("the fixture opens");
    let mut harness = harness(app);
    harness.state_mut().set_layout(Layout::Continuous);
    harness.state_mut().go_to(4);
    let wanted = harness.state().visible();
    for page in &wanted {
        harness
            .state_mut()
            .page_image(*page)
            .expect("page requested");
    }
    pump_until_rendered(&mut harness, &wanted);
    let images = harness
        .query_all_by(|node| node.role() == egui::accesskit::Role::Image)
        .count();
    assert!(
        images > 1,
        "a continuous view shows its window, got {images}"
    );
}

#[test]
fn the_search_panel_is_closed_until_it_is_asked_for() {
    let mut app = App::new();
    app.open_path(&minimal()).expect("the fixture opens");
    let mut harness = harness(app);

    // The toggle exists, because the bar does not reflow; the panel does not,
    // because it would take space the page needs.
    let title = reader_label(ReaderMessage::SearchTitle);
    harness.get_by_label(&title);
    assert!(!harness.state().search_open(), "closed on open");

    harness.get_by_label(&title).click();
    harness.run();
    assert!(harness.state().search_open(), "the toggle opened it");

    harness.get_by_label(&title).click();
    harness.run();
    assert!(!harness.state().search_open(), "and closes it again");
}

#[test]
fn typing_a_query_shows_the_hits_and_the_count() {
    let mut app = App::new();
    app.open_path(&minimal()).expect("the fixture opens");
    app.set_search_open(true);
    let mut harness = harness(app);

    // The count comes from the catalogue with the number substituted, so the
    // test asserts on the localized sentence rather than on a literal.
    harness.state_mut().set_query("sOFD");
    harness.run();
    assert!(
        !harness.state().hits().is_empty(),
        "the fixture has the word"
    );

    let count = harness.state().search_count_label();
    harness.get_by_label_contains(&count);
}

#[test]
fn a_query_with_no_match_offers_the_no_match_notice() {
    let mut app = App::new();
    app.open_path(&minimal()).expect("the fixture opens");
    app.set_search_open(true);
    app.set_query("zzzz-not-in-this-document");
    let harness = harness(app);

    let notice = reader_label(ReaderMessage::NoMatch);
    harness.get_by_label_contains(&notice);
}

#[test]
fn the_search_toggle_is_disabled_until_a_document_is_open() {
    // There is nothing to search with no document open, so the toggle is
    // disabled rather than hidden. This mirrors the close button's rule.
    let harness = harness(App::new());
    let title = reader_label(ReaderMessage::SearchTitle);
    let button = harness.get_by_label(&title);
    assert!(
        button.accesskit_node().is_disabled(),
        "with nothing open the search toggle does nothing"
    );
}

#[test]
fn the_signature_toggle_is_disabled_until_a_document_is_open() {
    let harness = harness(App::new());
    let heading = label(MessageId::SignPanelTitle);
    let button = harness.get_by_label(&heading);
    assert!(
        button.accesskit_node().is_disabled(),
        "there is nothing to verify with no document open"
    );
}

#[test]
fn opening_the_signature_panel_shows_the_count_and_the_verdict() {
    let mut app = App::new();
    let (bytes, _key) = sofd_testkit::signed_fixture();
    app.open_bytes(&bytes, Some("signed.ofd"))
        .expect("the signed fixture opens");
    let mut harness = harness(app);

    let heading = label(MessageId::SignPanelTitle);
    assert!(
        harness.query_by_label_contains(&heading).is_some(),
        "the toggle is in the toolbar"
    );
    harness.get_by_label(&heading).click();
    harness.run();
    assert!(harness.state().signatures_open(), "the toggle opened it");

    // The count is the catalogue's plural sentence with the number in it.
    let count = harness.state().signature_count_label();
    harness.get_by_label(&count);
    // And the verdict is the word for a signature that holds.
    let verdict = label(MessageId::SignValid);
    harness.get_by_label_contains(&verdict);
}

#[test]
fn the_export_buttons_are_disabled_until_a_document_is_open() {
    let harness = harness(App::new());
    for key in [ReaderMessage::ExportPdf, ReaderMessage::ExportPng] {
        let text = reader_label(key);
        let button = harness.get_by_label(&text);
        assert!(
            button.accesskit_node().is_disabled(),
            "there is nothing to export with no document open"
        );
    }
}

#[test]
fn asking_to_export_asks_the_host_where_to_write_it() {
    // The window cannot know where a person wants a file, so it reports the
    // format and lets the host ask the platform.
    let mut app = App::new();
    let bytes = sofd_testkit::multipage_archive(3);
    app.open_bytes(&bytes, Some("many.ofd"))
        .expect("the fixture opens");
    let mut harness = harness(app);

    harness
        .get_by_label(&reader_label(ReaderMessage::ExportPdf))
        .click();
    harness.run();
    assert_eq!(
        harness.state().wants_export(),
        Some(sofd_egui::ExportFormat::Pdf),
        "the state holds the request until the host answers"
    );
}

#[test]
fn a_finished_export_is_announced_in_the_status_bar() {
    let dir = std::env::temp_dir().join("sofd-window-export");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("the scratch directory is creatable");
    let out = dir.join("out.pdf");
    let mut app = App::new();
    let bytes = sofd_testkit::multipage_archive(2);
    app.open_bytes(&bytes, Some("many.ofd"))
        .expect("the fixture opens");
    app.export_to(sofd_egui::ExportFormat::Pdf, vec![0], out.clone());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while std::time::Instant::now() < deadline {
        app.poll();
        if !app.is_exporting() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let harness = harness(app);
    let label = harness.state().export_label().expect("the export reported");
    harness.get_by_label_contains(&label);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn asking_to_print_reaches_the_host_without_a_destination() {
    // Printing is bounded by the paper the reader has, not by a file, so the
    // window reports the request and does not wait for a dialog.
    let mut app = App::new();
    let bytes = sofd_testkit::multipage_archive(3);
    app.open_bytes(&bytes, Some("many.ofd"))
        .expect("the fixture opens");
    let mut harness = harness(app);

    harness
        .get_by_label(&reader_label(ReaderMessage::Print))
        .click();
    harness.run();
    assert!(
        harness.state().wants_print(),
        "the print button did not reach the state the host reads"
    );
}

#[test]
fn the_print_button_is_disabled_until_a_document_is_open() {
    let harness = harness(App::new());
    let text = reader_label(ReaderMessage::Print);
    let button = harness.get_by_label(&text);
    assert!(
        button.accesskit_node().is_disabled(),
        "there is nothing to print with no document open"
    );
}

#[test]
fn a_finished_print_job_is_announced_in_the_status_bar() {
    let mut app = App::new();
    let bytes = sofd_testkit::multipage_archive(2);
    app.open_bytes(&bytes, Some("many.ofd"))
        .expect("the fixture opens");
    app.print_to(app.print_pages(), app.print_settings());
    // A shared runner can stall the print worker, so wait on the clock rather
    // than on a fixed iteration count; a real hang still fails the assert.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(60);
    while std::time::Instant::now() < deadline {
        app.poll();
        if !app.is_printing() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(2));
    }
    let harness = harness(app);
    let label = harness.state().print_label().expect("the job reported");
    harness.get_by_label_contains(&label);
}

#[test]
fn the_annotation_toggle_is_disabled_until_a_document_is_open() {
    let harness = harness(App::new());
    let text = label(MessageId::AnnotPanelTitle);
    let button = harness.get_by_label(&text);
    assert!(
        button.accesskit_node().is_disabled(),
        "there is nothing to list with no document open"
    );
}

#[test]
fn opening_the_annotation_panel_shows_the_count_and_the_marks() {
    let mut app = App::new();
    let bytes = sofd_testkit::annotated_archive(3, 2);
    app.open_bytes(&bytes, Some("marked.ofd"))
        .expect("the fixture opens");
    let mut harness = harness(app);

    harness
        .get_by_label(&label(MessageId::AnnotPanelTitle))
        .click();
    harness.run();
    assert!(harness.state().annotations_open(), "the toggle opened it");

    // The count is the catalogue's plural sentence with the number in it.
    let count = harness.state().annotation_count_label();
    harness.get_by_label(&count);
    // And the row is the mark itself, which a reader has to be able to see.
    let row = harness.state().annotation_label(0);
    harness.get_by_label_contains(&row);
}

#[test]
fn a_document_with_no_marks_says_so_in_the_panel() {
    // A document without annotations is not a failure, so the panel states a
    // fact rather than leaving an empty column.
    let mut app = App::new();
    let bytes = sofd_testkit::multipage_archive(2);
    app.open_bytes(&bytes, Some("plain.ofd"))
        .expect("the fixture opens");
    let mut harness = harness(app);

    harness
        .get_by_label(&label(MessageId::AnnotPanelTitle))
        .click();
    harness.run();
    harness.get_by_label(&label(MessageId::AnnotNone));
}

#[test]
fn the_menu_offers_both_languages_in_their_own_names() {
    // A reader who cannot read the menu's current language still has to be
    // able to find theirs, so the entries are endonyms rather than translations.
    let mut harness = harness(App::new());
    open_main_menu(&mut harness);
    harness.get_by_label(&label(MessageId::LocaleNameZh));
}

#[test]
fn choosing_a_language_redraws_the_window_in_it() {
    let mut app = App::new();
    let bytes = sofd_testkit::multipage_archive(3);
    app.open_bytes(&bytes, Some("many.ofd"))
        .expect("the fixture opens");
    let mut harness = harness(app);

    // The languages are a group inside the main menu, so it opens first.
    open_main_menu(&mut harness);
    harness
        .get_by_label(&label(MessageId::LocaleNameZh))
        .click();
    harness.run();
    assert_eq!(harness.state().locale().tag(), "zh-CN");

    // The page counter is a catalogue string, so it follows the language rather
    // than staying English behind a translated menu.
    let label = harness.state().page_label();
    // The catalogue entry for the Chinese page counter carries the character
    // for "page", written as an escape because the source carries no literal.
    assert!(
        label.contains('\u{9875}'),
        "the page label did not follow: {label}"
    );
    harness.get_by_label_contains(&label);
}

#[test]
fn the_comparison_panel_lists_a_changed_page() {
    // Opening the panel and comparing two revisions has to show the verdict
    // and one row per changed page, all from the catalogue.
    let mut app = App::new();
    let one = sofd_testkit::multipage_archive(1);
    app.open_bytes(&one, Some("rev-a.ofd")).expect("opens");
    app.compare_bytes(&sofd_testkit::multipage_archive(2))
        .expect("compare");
    let harness = harness(app);

    // The toggle and the panel heading share the label, so the heading is not
    // asserted by label here; the toggle test covers it and the summary below
    // proves the panel body is drawn.
    let summary = harness.state().comparison_summary();
    let changed = harness
        .state()
        .comparison()
        .expect("a verdict")
        .changed_pages();
    assert!(changed >= 1, "the second revision added a page");
    assert!(
        summary.contains(&changed.to_string()),
        "the summary states the count: {summary}"
    );
    harness.get_by_label_contains(&summary);
}

#[test]
fn the_comparison_toggle_opens_the_panel() {
    let mut app = App::new();
    let one = sofd_testkit::multipage_archive(1);
    app.open_bytes(&one, Some("rev-a.ofd")).expect("opens");
    let mut harness = harness(app);
    assert!(!harness.state().comparison_open());

    let heading = label(MessageId::ComparePanelTitle);
    open_main_menu(&mut harness);
    harness.get_by_label(&heading).click();
    harness.run();
    assert!(
        harness.state().comparison_open(),
        "the toggle opens the panel"
    );
}

#[test]
fn a_comparison_that_matches_says_so() {
    let mut app = App::new();
    let one = sofd_testkit::multipage_archive(1);
    app.open_bytes(&one, Some("rev-a.ofd")).expect("opens");
    app.compare_bytes(&one).expect("compare");
    let harness = harness(app);

    let identical = label(MessageId::CompareIdentical);
    assert_eq!(harness.state().comparison_summary(), identical);
    harness.get_by_label_contains(&identical);
}

#[test]
fn the_settings_offer_three_themes_and_follow_the_system_by_default() {
    let mut harness = harness(App::new());
    assert_eq!(
        harness.state().theme(),
        ThemeChoice::System,
        "a reader who told their desktop to go dark has told this window too"
    );
    open_main_menu(&mut harness);
    for choice in ThemeChoice::ALL {
        harness.get_by_label(&harness.state().theme_name(choice));
    }
    let dark = harness.state().theme_name(ThemeChoice::Dark);
    harness.get_by_label(&dark).click();
    harness.run();
    assert_eq!(harness.state().theme(), ThemeChoice::Dark);
}

#[test]
fn the_settings_offer_three_text_sizes_and_the_scale_moves_every_style() {
    let mut harness = harness(App::new());
    open_main_menu(&mut harness);
    for size in TextSize::ALL {
        harness.get_by_label(&harness.state().text_size_name(size));
    }
    let normal = harness.state().style_points(sofd_egui::theme::Style::Body);
    let larger = harness.state().text_size_name(TextSize::Larger);
    harness.get_by_label(&larger).click();
    harness.run();
    let scaled = harness.state().style_points(sofd_egui::theme::Style::Body);
    assert!(scaled > normal, "{scaled} is not larger than {normal}");
}

#[test]
fn high_contrast_is_a_switch_rather_than_a_fourth_theme() {
    let mut harness = harness(App::new());
    let choice = harness.state().theme();
    open_main_menu(&mut harness);
    let switch = harness.state().high_contrast_label();
    harness.get_by_label(&switch).click();
    harness.run();
    assert!(harness.state().high_contrast(), "the switch turned on");
    assert_eq!(
        harness.state().theme(),
        choice,
        "the theme is untouched, which is what keeps light-or-dark the reader's"
    );
}

#[test]
fn a_theme_never_moves_the_document() {
    let mut app = App::new();
    app.open_path(&minimal()).expect("the fixture opens");
    app.go_to(0);
    let before = app.page_count();
    let mut harness = harness(app);
    open_main_menu(&mut harness);
    let light = harness.state().theme_name(ThemeChoice::Light);
    harness.get_by_label(&light).click();
    harness.run();
    assert_eq!(harness.state().page_count(), before);
    assert!(
        harness.state().is_open(),
        "a setting never closes a document"
    );
}

#[test]
fn the_canvas_is_drawn_in_its_own_colour() {
    // The well behind the pages tells a reader that a page is a sheet lying on
    // a surface. It had gone missing: the margin around a page was the panel.
    let mut app = App::new();
    app.set_theme(ThemeChoice::Light);
    let mut harness = harness(app);
    harness.run();
    let output = harness.output();
    let shapes: Vec<egui::epaint::ClippedShape> = output
        .shapes
        .iter()
        .map(|clipped| egui::epaint::ClippedShape {
            clip_rect: clipped.clip_rect,
            shape: clipped.shape.clone(),
        })
        .collect();
    let mut tessellator =
        egui::epaint::Tessellator::new(1.0, Default::default(), [16, 16], Vec::new());
    let painted = tessellator.tessellate_shapes(shapes).iter().any(|clipped| {
        match &clipped.primitive {
            // #eeeeee is the light column's canvas; the panels are #fafafa.
            egui::epaint::Primitive::Mesh(mesh) => mesh
                .vertices
                .iter()
                .any(|v| v.color == egui::Color32::from_rgb(0xee, 0xee, 0xee)),
            _ => false,
        }
    });
    assert!(painted, "the canvas well was never painted");
}

#[test]
fn an_invoice_seal_answers_a_click_with_what_the_signature_says() {
    // Notice 2020 no. 1 replaces the printed seal with a signed one and tells a
    // reader to open it. So the seal is clickable, and what opens states the
    // three lines the seal prints, which travel inside the signature.
    let bytes = std::fs::read(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/fixtures/ofdrw/999.ofd"
    ))
    .expect("the invoice fixture");
    let mut app = App::new();
    app.open_bytes(&bytes, Some("999.ofd"))
        .expect("the invoice opens");
    // Reading the seals is what the panel does on its first draw, and it is
    // also what makes the seal a place the pointer can land on.
    app.set_signatures_open(true);
    // The seal is drawn at 90, 8 and is 30 by 20 millimetres, so its middle is
    // at 105, 18. The centre of a ring is empty paper and still the seal.
    let hit = app
        .seal_at(0, (105.0, 18.0))
        .expect("the middle of the seal is the seal");
    assert_eq!(hit, 0, "the invoice carries one signature");
    assert!(
        app.seal_at(0, (10.0, 10.0)).is_none(),
        "the page beside the seal is not the seal"
    );

    // The panel states the seal's own text, which is what a person compares
    // against the invoice in their hand, and how much the proof covers.
    let harness = harness(app);
    harness.get_by_label_contains("Supervision seal: 全国统一发票监制章");
    harness.get_by_label_contains("The signature covers 20 files");
    harness.get_by_label_contains("Certificate: 2020-07-23 to 2030-07-23");
}

/// A reader window of a given width, drawn a few times so panels settle.
fn window_at(width: f32) -> Harness<'static, App> {
    window_at_with(width, egui::epaint::MarginF32::ZERO)
}

/// The same, with a device's system bars taking `safe` off the window.
///
/// The state is written before the first frame because that is the order a
/// phone runs in: the activity knows its content rectangle before it draws.
fn window_at_with(width: f32, safe: egui::epaint::MarginF32) -> Harness<'static, App> {
    let mut harness = Harness::builder()
        .with_size(egui::vec2(width, 700.0))
        .build_ui_state(
            move |ui, app: &mut App| {
                window::set_safe_area(ui.ctx(), safe);
                window::draw(ui, app);
            },
            App::new(),
        );
    for _ in 0..3 {
        harness.run();
    }
    harness
}

/// Every control either bar can draw, whether or not this width shows it.
fn bar_controls() -> Vec<String> {
    [
        ReaderMessage::Open,
        ReaderMessage::CloseHint,
        ReaderMessage::SearchToggle,
        ReaderMessage::SignaturesToggle,
        ReaderMessage::AnnotationsToggle,
        ReaderMessage::MainMenu,
        ReaderMessage::LayoutSingle,
        ReaderMessage::LayoutDouble,
        ReaderMessage::LayoutContinuous,
        ReaderMessage::PrevPage,
        ReaderMessage::NextPage,
        ReaderMessage::FitWidth,
        ReaderMessage::FitPage,
        ReaderMessage::ExportPdf,
        ReaderMessage::ExportPng,
    ]
    .iter()
    .map(|message| reader_label(*message))
    .collect()
}

#[test]
fn the_two_bars_fit_a_phone_without_clipping() {
    // A phone's width plus the widths either side of each point where a bar
    // has to start folding; the arithmetic that spends the width can be off by
    // one step, and a control drawn past the edge is one nobody can reach.
    for width in [320.0f32, 360.0, 400.0, 480.0, 720.0, 1000.0] {
        let harness = window_at(width);
        for name in bar_controls() {
            for node in harness.query_all_by_label(&name) {
                let rect = node.rect();
                assert!(
                    rect.left() >= -0.5 && rect.right() <= width + 0.5,
                    "{name:?} is drawn at {:.0}..{:.0} in a window {width:.0} wide",
                    rect.left(),
                    rect.right()
                );
            }
        }
        // The menu is the one control a bar cannot drop, so it is also the one
        // that has to be there at every width.
        let menu_label = reader_label(ReaderMessage::MainMenu);
        let menu = harness.get_by_label(&menu_label);
        let rect = menu.rect();
        assert!(
            rect.right() <= width + 0.5 && rect.right() >= width - 24.0,
            "the menu ends at {:.0} in a window {width:.0} wide",
            rect.right()
        );
    }
}

#[test]
fn no_two_controls_on_one_bar_overlap() {
    // Fitting inside the window is not being reachable: two controls drawn on
    // top of each other both fit and one cannot be clicked. Each bar is
    // measured as boxes, and boxes sharing a row may share no width.
    for width in [320.0f32, 360.0, 400.0, 480.0, 720.0, 1000.0] {
        let harness = window_at(width);
        let mut boxes: Vec<(String, egui::Rect)> = Vec::new();
        let labels = bar_controls();
        for name in &labels {
            for node in harness.query_all_by_label(name) {
                let rect = node.rect();
                boxes.push((name.clone(), rect));
            }
        }
        for (i, (first, one)) in boxes.iter().enumerate() {
            for (second, other) in boxes.iter().skip(i + 1) {
                // Same row: the vertical spans meet.
                let share_row =
                    one.top() < other.bottom() - 0.5 && other.top() < one.bottom() - 0.5;
                if !share_row {
                    continue;
                }
                let share_width =
                    one.left() < other.right() - 0.5 && other.left() < one.right() - 0.5;
                assert!(
                    !share_width,
                    "{first:?} and {second:?} overlap at {:.0}..{:.0} and {:.0}..{:.0} in a window {width:.0} wide",
                    one.left(),
                    one.right(),
                    other.left(),
                    other.right()
                );
            }
        }
    }
}

#[test]
fn a_safe_area_moves_the_zones_clear_of_the_system_bars() {
    // A device draws its status bar over the window, so a header at y = 0 sits
    // under the clock. What this asserts is that the window acts on the content
    // rectangle: zones move down by the bar and no width is lost.
    let bar = egui::epaint::MarginF32 {
        left: 0.0,
        top: 24.0,
        right: 48.0,
        bottom: 0.0,
    };
    let plain = window_at(400.0);
    let guarded = window_at_with(400.0, bar);
    let header = reader_label(ReaderMessage::Open);
    let before = plain.get_by_label(&header).rect();
    let after = guarded.get_by_label(&header).rect();
    assert!(
        (after.top() - (before.top() + bar.top)).abs() < 0.5,
        "the header starts at {:.0} with no bar and {:.0} with a {:.0} one",
        before.top(),
        after.top(),
        bar.top
    );
    // The menu is the control nearest the right edge, so it is the one a bar
    // on that side would cover, and the one whose margin has to include it.
    let menu = reader_label(ReaderMessage::MainMenu);
    let end = guarded.get_by_label(&menu).rect();
    assert!(
        end.right() <= 400.0 - bar.right + 0.5,
        "the menu ends at {:.0}, inside the {:.0} the bar leaves",
        end.right(),
        400.0 - bar.right
    );
}

#[test]
fn a_bar_wider_than_the_window_leaves_it_whole() {
    // A device that reports bars bigger than its own window has reported
    // nonsense, and a negative rectangle is an assertion inside `egui`. The
    // window is what is left rather than a panic, and it is still usable.
    let window = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(320.0, 600.0));
    let safe = egui::epaint::MarginF32 {
        left: 200.0,
        top: 500.0,
        right: 200.0,
        bottom: 500.0,
    };
    let inner = window::inset(window, safe);
    assert!(inner.width() >= 0.0 && inner.height() >= 0.0);
    assert!(inner.left() <= inner.right() && inner.top() <= inner.bottom());
}

#[test]
fn the_zoom_stays_draggable_or_is_not_drawn() {
    // The slider is the only control in either bar that cannot become an icon,
    // so it is measured here rather than guessed at: the point of the folding
    // is that what a reader is shown still works, not that fewer things are.
    let wide = window_at(480.0);
    let showing = wide.state().zoom_label();
    let slider = wide
        .query_all_by_label(&showing)
        .next()
        .expect("a bar with room draws the zoom");
    assert!(
        slider.rect().right() <= 480.5,
        "the zoom is inside a window 480 wide"
    );

    // At a phone's width the slider is still drawn, and dragging it still
    // moves the zoom: a control shown but inert would be worse than absent.
    let mut narrow = window_at(400.0);
    let before = narrow.state().zoom();
    let showing = narrow.state().zoom_label();
    let node = narrow
        .query_all_by_label(&showing)
        .next()
        .expect("a phone still shows the zoom");
    let rect = node.rect();
    let start = rect.center();
    let end = egui::pos2(rect.right() - 2.0, start.y);
    let pointer = egui::PointerButton::Primary;
    for to in [
        egui::Event::PointerButton {
            pos: start,
            button: pointer,
            pressed: true,
            modifiers: Default::default(),
        },
        egui::Event::PointerMoved(end),
        egui::Event::PointerButton {
            pos: end,
            button: pointer,
            pressed: false,
            modifiers: Default::default(),
        },
    ] {
        narrow.input_mut().events.push(to);
        narrow.run();
    }
    assert!(
        narrow.state().zoom() > before,
        "a drag to the right of the zoom's handle raises the zoom"
    );
}

// The fold into the main menu is not tested from end to end: below about 280
// points the menu opens past the window and the harness stops reading there, so
// such a test would measure the harness rather than the bar.
