//! The one place the toolkit's key codes meet the reader's own keys, and the
//! search box's id. Everything past the translation belongs to the session, so
//! a second shell repeats only the `match` in [`press`].

use eframe::egui::{Key, Modifiers};

use sofd_session::action::Held;

pub use sofd_session::action::{Action, ZOOM_STEP, escape_target, interpret};

/// Read a key as the reader's vocabulary, or `None` for one no binding names.
pub fn press(key: Key) -> Option<sofd_session::action::KeyPress> {
    use sofd_session::action::KeyPress;
    let key = match key {
        Key::ArrowRight => KeyPress::Right,
        Key::ArrowLeft => KeyPress::Left,
        Key::ArrowDown => KeyPress::Down,
        Key::ArrowUp => KeyPress::Up,
        Key::PageDown => KeyPress::PageDown,
        Key::PageUp => KeyPress::PageUp,
        Key::Home => KeyPress::Home,
        Key::End => KeyPress::End,
        Key::Equals => KeyPress::Equal,
        Key::Plus => KeyPress::Plus,
        Key::Minus => KeyPress::Minus,
        Key::Num0 => KeyPress::Zero,
        Key::F3 => KeyPress::F3,
        Key::Slash => KeyPress::Slash,
        Key::Escape => KeyPress::Escape,
        Key::O => KeyPress::O,
        Key::S => KeyPress::S,
        Key::P => KeyPress::P,
        Key::F => KeyPress::F,
        Key::J => KeyPress::J,
        Key::K => KeyPress::K,
        _ => return None,
    };
    Some(key)
}

/// The modifiers that came with a press, as the reader's own type.
pub fn held(modifiers: Modifiers) -> Held {
    Held {
        ctrl: modifiers.ctrl || modifiers.command,
        shift: modifiers.shift,
    }
}

/// The search box's persistent id, shared by the widget and the focus request.
pub fn search_box_id() -> eframe::egui::Id {
    eframe::egui::Id::new("reader-search-box")
}
