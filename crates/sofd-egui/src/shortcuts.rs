//! Keyboard shortcuts, resolved to intent before anything is drawn. This maps
//! a key press to an [`Action`] and nothing else, and it runs after the panels
//! are drawn, so a key a text field took never turns a page underneath it.

use eframe::egui::{Key, Modifiers};

pub use sofd_session::action::{Action, ZOOM_STEP, escape_target};

use sofd_session::action::needs_document;

/// Resolve one key press, or `None` if it means nothing to the reader. The
/// flag gates shortcuts that act on a page, so a key does nothing rather than
/// failing when there is no document to act on.
pub fn interpret(key: Key, modifiers: Modifiers, has_document: bool) -> Option<Action> {
    let action = action_for(key, modifiers)?;
    if !needs_document(action) || has_document {
        Some(action)
    } else {
        None
    }
}

/// The mapping itself, without the document check.
fn action_for(key: Key, modifiers: Modifiers) -> Option<Action> {
    let ctrl = modifiers.ctrl || modifiers.command;
    if ctrl {
        return match key {
            Key::O => Some(Action::Open),
            Key::F => Some(Action::ToggleSearch),
            Key::S => Some(Action::ExportPdf),
            Key::P => Some(Action::Print),
            // The browser's find shortcut. The key carries a slash, not the
            // question mark some layouts print on it: the binding follows the
            // hardware, because the layout is not ours to interpret.
            Key::Slash => Some(Action::FocusSearch),
            Key::Home => Some(Action::FirstPage),
            Key::End => Some(Action::LastPage),
            Key::K => Some(Action::ToggleAnnotations),
            Key::J => Some(Action::NextAnnotation),
            _ => None,
        };
    }
    match key {
        Key::ArrowRight | Key::PageDown => Some(Action::NextPage),
        Key::ArrowLeft | Key::PageUp => Some(Action::PreviousPage),
        Key::Home => Some(Action::FirstPage),
        Key::End => Some(Action::LastPage),
        Key::ArrowDown if modifiers.shift => Some(Action::NextHit),
        Key::ArrowUp if modifiers.shift => Some(Action::PreviousHit),
        // Animation-style stepping through the marks: one key forward, one
        // back, as the browsers that show annotations use.
        Key::J => Some(Action::NextAnnotation),
        Key::K => Some(Action::PreviousAnnotation),
        Key::Equals | Key::Plus => Some(Action::ZoomIn),
        Key::Minus => Some(Action::ZoomOut),
        Key::Num0 => Some(Action::ZoomReset),
        Key::F3 => Some(if modifiers.shift {
            Action::PreviousHit
        } else {
            Action::NextHit
        }),
        Key::Slash if modifiers.ctrl => Some(Action::FocusSearch),
        Key::Escape => Some(Action::Escape),
        _ => None,
    }
}

/// The search box's persistent id, shared by the widget and the focus request.
pub fn search_box_id() -> eframe::egui::Id {
    eframe::egui::Id::new("reader-search-box")
}
