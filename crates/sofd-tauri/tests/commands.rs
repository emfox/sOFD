//! The names a webview sends, and what they must mean.
//!
//! A string from the front end is the one thing here that is not type-checked,
//! so every conversion gets a defined answer for a name that does not exist.
//! The answer is the state the window was in: a click that says nothing changes
//! nothing, rather than panicking behind a button nobody can press safely.

use sofd_session::action::needs_document;
use sofd_session::viewer::Layout;
use sofd_session::{Action, App, TextSize, ThemeChoice};
use sofd_tauri::commands;

/// Every action the window can ask for resolves, with a document open.
#[test]
fn every_action_name_resolves_with_a_document() {
    let names = [
        "nextPage",
        "previousPage",
        "firstPage",
        "lastPage",
        "zoomIn",
        "zoomOut",
        "zoomReset",
        "nextHit",
        "previousHit",
        "focusSearch",
        "toggleSearch",
        "toggleSignatures",
        "toggleAnnotations",
        "nextAnnotation",
        "previousAnnotation",
        "escape",
        "open",
        "exportPdf",
        "exportPng",
        "print",
    ];
    for name in names {
        assert!(
            commands::action(name, true).is_some(),
            "{name} is a button the window draws and nothing handles it"
        );
    }
    assert_eq!(commands::action("nextPage", true), Some(Action::NextPage));
    assert_eq!(commands::action("open", true), Some(Action::Open));
    assert_eq!(commands::action("exportPng", true), Some(Action::ExportPng));
}

/// With no document, the actions that act on a page do nothing, and the ones
/// about the window still work: opening a file and quitting are not about a
/// page. This is the session's own rule, not a second one written here.
#[test]
fn an_action_on_a_page_needs_a_document() {
    for name in [
        "nextPage",
        "previousPage",
        "zoomIn",
        "exportPdf",
        "exportPng",
        "print",
    ] {
        assert_eq!(
            commands::action(name, false),
            None,
            "{name} fired with nothing open"
        );
    }
    for name in ["open", "toggleSearch"] {
        assert!(
            commands::action(name, false).is_some(),
            "{name} is about the window and should not wait for a document"
        );
    }
    // And the rule is the session's: an action added there without a name here
    // is caught by the test above, not by this one disagreeing with it.
    assert!(needs_document(Action::NextPage));
    assert!(!needs_document(Action::Open));
}

/// A name the window does not use resolves to nothing rather than to a guess.
#[test]
fn an_unknown_action_changes_nothing() {
    for name in ["", "quit", "nextpage", "NextPage", "selfDestruct"] {
        assert_eq!(commands::action(name, true), None, "{name:?} was accepted");
    }
}

/// Keys are spelled the way `KeyboardEvent.key` spells them, and a key the
/// session has no binding for resolves to nothing.
#[test]
fn keys_are_spelled_as_a_browser_spells_them() {
    use sofd_session::KeyPress;
    assert_eq!(commands::key("ArrowRight"), Some(KeyPress::Right));
    assert_eq!(commands::key("PageDown"), Some(KeyPress::PageDown));
    assert_eq!(commands::key("Escape"), Some(KeyPress::Escape));
    assert_eq!(commands::key("o"), Some(KeyPress::O));
    assert_eq!(commands::key("O"), Some(KeyPress::O));
    assert_eq!(commands::key("="), Some(KeyPress::Equal));
    for name in ["", "F13", "ArrowHorizontal", "Meta"] {
        assert_eq!(commands::key(name), None, "{name:?} was accepted");
    }
}

/// The three settings offer three values each, and an unknown name keeps what
/// the reader already had rather than silently resetting it.
#[test]
fn a_setting_name_resolves_and_an_unknown_one_keeps_the_default() {
    assert_eq!(commands::theme_of("light"), ThemeChoice::Light);
    assert_eq!(commands::theme_of("dark"), ThemeChoice::Dark);
    assert_eq!(commands::theme_of("system"), ThemeChoice::System);
    assert_eq!(commands::theme_of("solarized"), ThemeChoice::System);

    assert_eq!(commands::text_size_of("large"), TextSize::Large);
    assert_eq!(commands::text_size_of("larger"), TextSize::Larger);
    assert_eq!(commands::text_size_of("normal"), TextSize::Normal);
    assert_eq!(commands::text_size_of("huge"), TextSize::Normal);

    assert_eq!(commands::layout_of("facing"), Layout::Facing);
    assert_eq!(commands::layout_of("continuous"), Layout::Continuous);
    assert_eq!(commands::layout_of("single"), Layout::Single);
    assert_eq!(commands::layout_of("mosaic"), Layout::Single);
}

/// The three panels are exclusive views of one document, so opening one closes
/// the other two and picking the one already showing closes it. A reader with
/// two panels open has been given a window they did not ask for.
#[test]
fn the_panels_are_exclusive_and_toggle() {
    let mut app = App::new();
    commands::toggle_panel(&mut app, "search");
    assert!(app.search_open());
    assert!(!app.signatures_open());

    commands::toggle_panel(&mut app, "signatures");
    assert!(app.signatures_open(), "the second panel opened");
    assert!(!app.search_open(), "and the first closed");

    commands::toggle_panel(&mut app, "annotations");
    assert!(app.annotations_open());
    assert!(!app.signatures_open(), "one at a time");

    // Picking the view already showing gives the reader their window back.
    commands::toggle_panel(&mut app, "annotations");
    assert!(!app.annotations_open());
    assert!(!app.search_open());
    assert!(!app.signatures_open());
}

/// A panel name that does not exist leaves every panel as it was.
#[test]
fn an_unknown_panel_changes_nothing() {
    let mut app = App::new();
    commands::toggle_panel(&mut app, "outline");
    assert!(!app.search_open());
    assert!(!app.signatures_open());
    assert!(!app.annotations_open());
}

/// The formats are the two the toolbar offers, and nothing else is written.
#[test]
fn the_export_formats_are_the_two_the_toolbar_offers() {
    assert!(commands::format("pdf").is_some());
    assert!(commands::format("png").is_some());
    assert!(commands::format("svg").is_none());
    assert!(commands::format("").is_none());
}
