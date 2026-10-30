//! The keyboard, pressed without a window. A shortcut is resolved to intent
//! here, so what a key reaches is provable before any shell draws a frame.

use sofd_session::viewer::Layout;
use sofd_session::{Action, App, Held, KeyPress, escape_target, interpret};

/// A reader over a document with `pages` pages, so paging has room to move.
fn open_pages(pages: usize) -> App {
    let mut app = App::new();
    let bytes = sofd_testkit::multipage_archive(pages);
    app.open_bytes(&bytes, Some("many.ofd"))
        .expect("the fixture opens");
    app
}

/// No modifier at all.
fn plain() -> Held {
    Held::default()
}

/// Ctrl held, as a shortcut uses it.
fn ctrl() -> Held {
    Held {
        ctrl: true,
        ..Held::default()
    }
}

/// Shift held, which is what distinguishes two meanings of one key.
fn shift() -> Held {
    Held {
        shift: true,
        ..Held::default()
    }
}

/// Resolve a key the way the window does, with a document open.
fn press(key: KeyPress, held: Held) -> Option<Action> {
    interpret(key, held, true)
}

#[test]
fn the_arrow_keys_and_page_keys_both_page() {
    // Two keys for one action, because a reader arrives with either habit.
    assert_eq!(press(KeyPress::Right, plain()), Some(Action::NextPage));
    assert_eq!(press(KeyPress::PageDown, plain()), Some(Action::NextPage));
    assert_eq!(press(KeyPress::Left, plain()), Some(Action::PreviousPage));
    assert_eq!(press(KeyPress::PageUp, plain()), Some(Action::PreviousPage));
}

#[test]
fn home_and_end_jump_to_the_ends_of_the_document() {
    assert_eq!(press(KeyPress::Home, plain()), Some(Action::FirstPage));
    assert_eq!(press(KeyPress::End, plain()), Some(Action::LastPage));
    assert_eq!(press(KeyPress::Home, ctrl()), Some(Action::FirstPage));
    assert_eq!(press(KeyPress::End, ctrl()), Some(Action::LastPage));
}

#[test]
fn the_zoom_keys_step_the_resolution_and_reset_it() {
    assert_eq!(press(KeyPress::Equal, plain()), Some(Action::ZoomIn));
    assert_eq!(press(KeyPress::Minus, plain()), Some(Action::ZoomOut));
    assert_eq!(press(KeyPress::Zero, plain()), Some(Action::ZoomReset));
}

#[test]
fn f3_walks_the_hits_in_both_directions() {
    // One key, two meanings, distinguished by shift: the browser habit.
    assert_eq!(press(KeyPress::F3, plain()), Some(Action::NextHit));
    assert_eq!(press(KeyPress::F3, shift()), Some(Action::PreviousHit));
}

#[test]
fn the_command_keys_reach_the_expensive_actions() {
    assert_eq!(press(KeyPress::O, ctrl()), Some(Action::Open));
    assert_eq!(press(KeyPress::S, ctrl()), Some(Action::ExportPdf));
    // The second export sits beside the first, so the pair reads as a pair.
    assert_eq!(press(KeyPress::E, ctrl()), Some(Action::ExportPng));
    assert_eq!(press(KeyPress::P, ctrl()), Some(Action::Print));
    assert_eq!(press(KeyPress::F, ctrl()), Some(Action::ToggleSearch));
    assert_eq!(press(KeyPress::Slash, ctrl()), Some(Action::FocusSearch));
}

#[test]
fn the_annotation_keys_walk_the_marks() {
    // One key forward, one back, as a reader paging through notes expects.
    assert_eq!(press(KeyPress::J, plain()), Some(Action::NextAnnotation));
    assert_eq!(
        press(KeyPress::K, plain()),
        Some(Action::PreviousAnnotation)
    );
    assert_eq!(press(KeyPress::K, ctrl()), Some(Action::ToggleAnnotations));
}

#[test]
fn a_key_with_no_meaning_is_none_rather_than_a_guess() {
    assert_eq!(press(KeyPress::Seven, plain()), None);
    assert_eq!(press(KeyPress::Q, plain()), None);
    assert_eq!(press(KeyPress::A, plain()), None);
}

#[test]
fn page_keys_do_nothing_with_no_document_open() {
    // Not an error the reader should hear about: there is no page to turn.
    assert_eq!(interpret(KeyPress::Right, plain(), false), None);
    assert_eq!(interpret(KeyPress::End, plain(), false), None);
}

#[test]
fn opening_and_searching_still_work_with_nothing_open() {
    // The two things a reader can do before opening anything.
    assert_eq!(interpret(KeyPress::O, ctrl(), false), Some(Action::Open));
    assert_eq!(
        interpret(KeyPress::F, ctrl(), false),
        Some(Action::ToggleSearch)
    );
}

#[test]
fn escape_leaves_the_search_box_before_it_closes_the_panel() {
    // A caret the reader cannot get out of is the one real trap here.
    assert_eq!(escape_target(true, true), Some(Action::FocusSearch));
    assert_eq!(escape_target(true, false), Some(Action::ToggleSearch));
    assert_eq!(escape_target(false, false), None);
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
    app.set_layout(Layout::Facing);
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
        sofd_session::viewer::MAX_DPI,
        "zooming in forever must stop"
    );
    for _ in 0..200 {
        app.apply(Action::ZoomOut);
    }
    assert_eq!(
        app.dpi(),
        sofd_session::viewer::MIN_DPI,
        "zooming out forever must stop"
    );
    app.apply(Action::ZoomReset);
    assert_eq!(app.dpi(), sofd_session::viewer::DEFAULT_DPI);
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
