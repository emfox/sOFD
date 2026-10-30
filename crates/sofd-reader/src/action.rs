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
