//! What a reader can ask the window to do, and the rules about when asking is
//! allowed. Keys are the toolkit's business; the intent they carry is not, so
//! a shortcut map in one shell and a menu item in another both land here.

/// What a key press asks the reader to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// Step forward by the layout's stride.
    NextPage,
    /// Step back by the layout's stride.
    PreviousPage,
    /// Jump to the first page.
    FirstPage,
    /// Jump to the last page.
    LastPage,
    /// Raise the resolution.
    ZoomIn,
    /// Lower the resolution.
    ZoomOut,
    /// Return to the default resolution.
    ZoomReset,
    /// Move to the next search hit.
    NextHit,
    /// Move to the previous search hit.
    PreviousHit,
    /// Put the caret in the search box.
    FocusSearch,
    /// Show or hide the search panel.
    ToggleSearch,
    /// Show or hide the signature panel.
    ToggleSignatures,
    /// Show or hide the annotation panel.
    ToggleAnnotations,
    /// Move to the next annotation, opening the panel.
    NextAnnotation,
    /// Move to the previous annotation, opening the panel.
    PreviousAnnotation,
    /// Leave whatever has focus, closing a panel or the search box.
    Escape,
    /// Ask the host to open a document.
    Open,
    /// Ask the host to write a PDF of the pages on screen.
    ExportPdf,
    /// Ask the host to print the pages on screen.
    Print,
}

/// How much one zoom step moves the resolution, in dots per inch.
pub const ZOOM_STEP: f32 = 12.0;

/// Whether the action depends on a document being open.
pub fn needs_document(action: Action) -> bool {
    !matches!(action, Action::Open | Action::Escape | Action::ToggleSearch)
}

/// What Escape should do, given what is open. The search box takes focus
/// before the panel opens, so Escape leaves the box first and closes the panel
/// on the next press. `None` means there is nothing to escape from.
pub fn escape_target(search_open: bool, search_focused: bool) -> Option<Action> {
    if search_focused {
        Some(Action::FocusSearch)
    } else if search_open {
        Some(Action::ToggleSearch)
    } else {
        None
    }
}

/// The keys a reader binds, named after what they do rather than where they
/// sit, so a shell translates its own key codes into these and the map below
/// stays the only place the bindings are written down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeyPress {
    /// The key that pages forward.
    Right,
    /// The key that pages backward.
    Left,
    /// The key that steps forward a screen.
    Down,
    /// The key that steps backward a screen.
    Up,
    /// A second key for paging, which a reader arrives with instead of arrows.
    PageDown,
    /// The matching second key for paging back.
    PageUp,
    /// Start of the document.
    Home,
    /// End of the document.
    End,
    /// The key a layout prints `=` on, which zooms in.
    Equal,
    /// The key a layout prints `+` on, which zooms in too.
    Plus,
    /// Zoom out.
    Minus,
    /// Reset the zoom.
    Zero,
    /// Walk the search hits.
    F3,
    /// The find shortcut's key, and a plain slash means nothing.
    Slash,
    /// Leave a panel or the search box.
    Escape,
    /// Open a document.
    O,
    /// Write a PDF.
    S,
    /// Print.
    P,
    /// Find on the page.
    F,
    /// Next annotation.
    J,
    /// Previous annotation, and with Ctrl the panel itself.
    K,
    /// A digit no binding uses, so a test can press one that means nothing.
    Seven,
    /// A letter no binding uses, for the same reason.
    Q,
    /// A second unused letter.
    A,
}

/// Which modifier keys were held with a press. Only the two that change a
/// binding are tracked; the rest are read as "no modifier".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Held {
    /// Ctrl, or the Command key where that is the platform's equivalent.
    pub ctrl: bool,
    /// Shift, which gives one key a second meaning.
    pub shift: bool,
}

/// Resolve one key press, or `None` if it means nothing to the reader. The
/// flag gates shortcuts that act on a page, so a key does nothing rather than
/// failing when there is no document to act on.
pub fn interpret(key: KeyPress, held: Held, has_document: bool) -> Option<Action> {
    let action = action_for(key, held)?;
    if !needs_document(action) || has_document {
        Some(action)
    } else {
        None
    }
}

/// The mapping itself, without the document check.
fn action_for(key: KeyPress, held: Held) -> Option<Action> {
    if held.ctrl {
        return match key {
            KeyPress::O => Some(Action::Open),
            KeyPress::F => Some(Action::ToggleSearch),
            KeyPress::S => Some(Action::ExportPdf),
            KeyPress::P => Some(Action::Print),
            // The browser's find shortcut. The key carries a slash, not the
            // question mark some layouts print on it: the binding follows the
            // hardware, because the layout is not ours to interpret.
            KeyPress::Slash => Some(Action::FocusSearch),
            KeyPress::Home => Some(Action::FirstPage),
            KeyPress::End => Some(Action::LastPage),
            KeyPress::K => Some(Action::ToggleAnnotations),
            KeyPress::J => Some(Action::NextAnnotation),
            _ => None,
        };
    }
    match key {
        KeyPress::Right | KeyPress::Down if held.shift => Some(Action::NextHit),
        KeyPress::Left | KeyPress::Up if held.shift => Some(Action::PreviousHit),
        KeyPress::Right | KeyPress::PageDown => Some(Action::NextPage),
        KeyPress::Left | KeyPress::PageUp => Some(Action::PreviousPage),
        KeyPress::Home => Some(Action::FirstPage),
        KeyPress::End => Some(Action::LastPage),
        // Animation-style stepping through the marks: one key forward, one
        // back, as the browsers that show annotations use.
        KeyPress::J => Some(Action::NextAnnotation),
        KeyPress::K => Some(Action::PreviousAnnotation),
        KeyPress::Equal | KeyPress::Plus => Some(Action::ZoomIn),
        KeyPress::Minus => Some(Action::ZoomOut),
        KeyPress::Zero => Some(Action::ZoomReset),
        KeyPress::F3 => Some(if held.shift {
            Action::PreviousHit
        } else {
            Action::NextHit
        }),
        KeyPress::Escape => Some(Action::Escape),
        _ => None,
    }
}
