//! The window's command surface, and the spellings it uses for things the
//! session names with a type.
//!
//! A string from a webview may name something that does not exist, so each
//! conversion answers an unknown name rather than panicking: a click that says
//! nothing changes nothing.

use std::sync::Mutex;

use serde::Deserialize;
use tauri::State;

use sofd_session::viewer::Layout;
use sofd_session::{
    Action, App, ExportFormat, Fit, Held, KeyPress, TextSize, ThemeChoice, interpret,
};

use crate::state::{self, Snapshot};
use crate::wire::{self, PageGeometry, WireMarks};
use crate::{icons, text};

/// The session, behind a lock the webview's threads share.
pub struct Host {
    /// The open document's state.
    pub app: Mutex<App>,
}

impl Host {
    /// A host with nothing open.
    pub fn new() -> Self {
        Self {
            app: Mutex::new(App::new()),
        }
    }
}

impl Default for Host {
    fn default() -> Self {
        Self::new()
    }
}

/// The action a name stands for, or `None` when it names nothing — or names
/// something that acts on a page while no page is open. A shortcut that does
/// nothing because nothing is open is not a failure worth a message.
pub fn action(name: &str, has_document: bool) -> Option<Action> {
    let action = match name {
        "nextPage" => Action::NextPage,
        "previousPage" => Action::PreviousPage,
        "firstPage" => Action::FirstPage,
        "lastPage" => Action::LastPage,
        "zoomIn" => Action::ZoomIn,
        "zoomOut" => Action::ZoomOut,
        "zoomReset" => Action::ZoomReset,
        "nextHit" => Action::NextHit,
        "previousHit" => Action::PreviousHit,
        "focusSearch" => Action::FocusSearch,
        "toggleSearch" => Action::ToggleSearch,
        "toggleSignatures" => Action::ToggleSignatures,
        "toggleAnnotations" => Action::ToggleAnnotations,
        "nextAnnotation" => Action::NextAnnotation,
        "previousAnnotation" => Action::PreviousAnnotation,
        "escape" => Action::Escape,
        "open" => Action::Open,
        "exportPdf" => Action::ExportPdf,
        "exportPng" => Action::ExportPng,
        "print" => Action::Print,
        _ => return None,
    };
    // The judgement is the session's, so a shell cannot hold a second opinion
    // about which actions need a document.
    if !has_document && sofd_session::action::needs_document(action) {
        return None;
    }
    Some(action)
}

/// The key a name stands for, spelled as `KeyboardEvent.key` spells it.
pub fn key(name: &str) -> Option<KeyPress> {
    Some(match name {
        "ArrowRight" => KeyPress::Right,
        "ArrowLeft" => KeyPress::Left,
        "ArrowDown" => KeyPress::Down,
        "ArrowUp" => KeyPress::Up,
        "PageDown" => KeyPress::PageDown,
        "PageUp" => KeyPress::PageUp,
        "Home" => KeyPress::Home,
        "End" => KeyPress::End,
        "=" | "+" => KeyPress::Equal,
        "-" => KeyPress::Minus,
        "0" => KeyPress::Zero,
        "F3" => KeyPress::F3,
        "/" => KeyPress::Slash,
        "Escape" => KeyPress::Escape,
        "o" | "O" => KeyPress::O,
        "s" | "S" => KeyPress::S,
        "e" | "E" => KeyPress::E,
        "p" | "P" => KeyPress::P,
        "f" | "F" => KeyPress::F,
        "j" | "J" => KeyPress::J,
        "k" | "K" => KeyPress::K,
        // A key with no name in the session's table is not an error: the
        // window sends every press, and most of them mean nothing.
        _ => return None,
    })
}

/// The theme a name stands for; an unknown name keeps the desktop's.
pub fn theme_of(name: &str) -> ThemeChoice {
    match name {
        "light" => ThemeChoice::Light,
        "dark" => ThemeChoice::Dark,
        _ => ThemeChoice::System,
    }
}

/// The text size a name stands for; an unknown name keeps the base size.
pub fn text_size_of(name: &str) -> TextSize {
    match name {
        "large" => TextSize::Large,
        "larger" => TextSize::Larger,
        _ => TextSize::Normal,
    }
}

/// The layout a name stands for; an unknown name keeps one page at a time.
pub fn layout_of(name: &str) -> Layout {
    match name {
        "facing" => Layout::Facing,
        "continuous" => Layout::Continuous,
        _ => Layout::Single,
    }
}

/// Show or hide one of the panels beside the page.
///
/// The three are exclusive views of one document, so opening one closes the
/// other two, and picking the one already showing closes it. A reader who ends
/// up with two panels open has been given a window they did not ask for.
pub fn toggle_panel(app: &mut App, name: &str) {
    // A panel is two answers — is it showing now, and how does it change — so
    // each name maps to a pair of plain setters rather than to a closure over
    // three different method types.
    type Setter = fn(&mut App, bool);
    let (showing, set, also): (bool, Setter, [Setter; 2]) = match name {
        "search" => (
            app.search_open(),
            |app: &mut App, on| app.set_search_open(on),
            [
                |app: &mut App, on| app.set_signatures_open(on),
                |app: &mut App, on| app.set_annotations_open(on),
            ],
        ),
        "signatures" => (
            app.signatures_open(),
            |app: &mut App, on| app.set_signatures_open(on),
            [
                |app: &mut App, on| app.set_search_open(on),
                |app: &mut App, on| app.set_annotations_open(on),
            ],
        ),
        "annotations" => (
            app.annotations_open(),
            |app: &mut App, on| app.set_annotations_open(on),
            [
                |app: &mut App, on| app.set_search_open(on),
                |app: &mut App, on| app.set_signatures_open(on),
            ],
        ),
        _ => return,
    };
    let opening = !showing;
    set(app, opening);
    if opening {
        for close in also {
            close(app, false);
        }
    }
}

/// Everything the window draws.
#[tauri::command]
pub fn snapshot(host: State<'_, Host>) -> Snapshot {
    let mut app = host.app.lock().expect("the session lock is never poisoned");
    state::snapshot(&mut app)
}

/// Every sentence the window can show, in the reader's language.
#[tauri::command]
pub fn messages(host: State<'_, Host>) -> Vec<text::Message> {
    let app = host.app.lock().expect("the session lock is never poisoned");
    text::messages(&app)
}

/// The glyphs, as the theme draws them.
#[tauri::command]
pub fn glyphs() -> icons::GlyphSet {
    icons::glyphs()
}

/// One page, as shapes a canvas draws.
#[tauri::command]
pub fn page(host: State<'_, Host>, index: usize) -> Result<PageGeometry, String> {
    let app = host.app.lock().expect("the session lock is never poisoned");
    let document = app
        .document()
        .ok_or_else(|| "no document is open".to_string())?;
    let walked = document
        .vector_page(index, &sofd::RenderOptions::default())
        .map_err(|err| err.to_string())?;
    Ok(wire::geometry(index, &walked))
}

/// The overlays of one page: search hits, and the boundaries of annotations
/// the document says are visible.
#[tauri::command]
pub fn overlays(host: State<'_, Host>, index: usize) -> WireMarks {
    let app = host.app.lock().expect("the session lock is never poisoned");
    let hits = app.hit_rects_on(index);
    let boundaries: Vec<_> = app
        .annotations_on(index)
        .into_iter()
        .filter(|(annotation, _)| annotation.visible)
        .filter_map(|(annotation, current)| Some((annotation.boundary?, current)))
        .collect();
    wire::marks(&hits, &boundaries)
}

/// What the reader did, named the way the front end names it.
#[tauri::command]
pub fn act(host: State<'_, Host>, name: String) {
    let mut app = host.app.lock().expect("the session lock is never poisoned");
    if let Some(action) = action(&name, app.is_open()) {
        app.apply(action);
    }
}

/// A key press, resolved through the session's own table so the two shells
/// cannot disagree about what a key does.
#[tauri::command]
pub fn press(host: State<'_, Host>, key_name: String, ctrl: bool, shift: bool) {
    let mut app = host.app.lock().expect("the session lock is never poisoned");
    let Some(press) = key(&key_name) else { return };
    let held = Held { ctrl, shift };
    if let Some(action) = interpret(press, held, app.is_open()) {
        app.apply(action);
    }
}

/// The area pages are laid out in, in CSS pixels. The session fits the page to
/// it, so a fit that follows the window is decided there rather than here.
#[tauri::command]
pub fn viewport(host: State<'_, Host>, width: f32, height: f32) {
    let mut app = host.app.lock().expect("the session lock is never poisoned");
    app.apply_fit((width, height));
}

/// The settings the reader chose, as the window names them.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    /// `system`, `light` or `dark`.
    theme: Option<String>,
    /// `normal`, `large` or `larger`.
    text_size: Option<String>,
    /// Whether contrast is raised.
    high_contrast: Option<bool>,
    /// A locale tag, as `sofd-i18n` spells it.
    locale: Option<String>,
}

/// Change how the window looks. Settings belong to the reader, not to the
/// document, so nothing here touches what is open.
#[tauri::command]
pub fn settings(host: State<'_, Host>, settings: Settings) {
    let mut app = host.app.lock().expect("the session lock is never poisoned");
    if let Some(name) = settings.theme.as_deref() {
        app.set_theme(theme_of(name));
    }
    if let Some(name) = settings.text_size.as_deref() {
        app.set_text_size(text_size_of(name));
    }
    if let Some(on) = settings.high_contrast {
        app.set_high_contrast(on);
    }
    if let Some(tag) = settings.locale.as_deref() {
        let wanted = app.locales().into_iter().find(|l| l.to_string() == tag);
        if let Some(locale) = wanted {
            app.set_locale(locale);
        }
    }
}

/// Open a document by path, which is what dragging one in produces.
#[tauri::command]
pub fn open(host: State<'_, Host>, path: String) -> Result<(), String> {
    let mut app = host.app.lock().expect("the session lock is never poisoned");
    app.open_path(std::path::Path::new(&path))
        .map_err(|err| err.message)
}

/// Go to a page the reader picked.
#[tauri::command]
pub fn go_to(host: State<'_, Host>, page: usize) {
    let mut app = host.app.lock().expect("the session lock is never poisoned");
    if page < app.page_count() {
        app.go_to(page);
    }
}

/// Drain what the export and print workers finished. The window has no frame
/// loop of its own, so it asks after a click and this answers: a report the
/// reader is told about, and nothing when the work is still running.
#[tauri::command]
pub fn collect(host: State<'_, Host>) -> Vec<String> {
    let mut app = host.app.lock().expect("the session lock is never poisoned");
    app.collect_exports();
    app.collect_prints();
    app.export_label()
        .into_iter()
        .chain(app.print_label())
        .collect()
}

/// Set the zoom as the reader reads it: a percentage of the page's 72 dpi size.
#[tauri::command]
pub fn zoom(host: State<'_, Host>, percent: f32) {
    let mut app = host.app.lock().expect("the session lock is never poisoned");
    app.set_zoom(percent / 100.0);
}

/// Choose how pages are arranged.
#[tauri::command]
pub fn layout(host: State<'_, Host>, name: String) {
    let mut app = host.app.lock().expect("the session lock is never poisoned");
    app.set_layout(layout_of(&name));
}

/// Show or hide one of the panels beside the page.
#[tauri::command]
pub fn panel(host: State<'_, Host>, name: String) {
    let mut app = host.app.lock().expect("the session lock is never poisoned");
    toggle_panel(&mut app, &name);
}

/// The export action a name stands for, or `None` when it names nothing.
pub fn format(name: &str) -> Option<ExportFormat> {
    match name {
        "pdf" => Some(ExportFormat::Pdf),
        "png" => Some(ExportFormat::Png),
        _ => None,
    }
}

/// Write the pages on screen out.
///
/// The window cannot know where a file should land, so it names one: a PDF is
/// one file and gets its path, a PNG is one file per page and gets a directory.
/// The session does the writing off the draw path, which is why this returns as
/// soon as the work is queued rather than when the file appears.
#[tauri::command]
pub fn export(host: State<'_, Host>, format: String, destination: String) {
    let mut app = host.app.lock().expect("the session lock is never poisoned");
    let Some(format) = crate::commands::format(&format) else {
        return;
    };
    let pages = app.export_pages();
    if pages.is_empty() {
        return;
    }
    app.export_to(format, pages, std::path::PathBuf::from(destination));
}

/// Keep the fit the reader picked, which is what a double-click on the page
/// does: fit the width, or the whole page once it already fits.
#[tauri::command]
pub fn fit(host: State<'_, Host>, name: String) {
    let mut app = host.app.lock().expect("the session lock is never poisoned");
    app.set_fit(match name.as_str() {
        "page" => Fit::Page,
        _ => Fit::Width,
    });
}
