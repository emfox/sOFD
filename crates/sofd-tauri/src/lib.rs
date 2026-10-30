//! The Tauri shell of the OFD reader: a webview window over the same session
//! the `egui` reader drives, drawing a page as geometry on a canvas of its own.
//!
//! Everything testable lives in modules with no window in them — the wire
//! format, the snapshot, the text, the name conversions — so the tests run
//! without a display, and `main.rs` only opens the window and registers them.

#![warn(missing_docs)]

use tauri::Emitter;

pub mod commands;
pub mod icons;
pub mod state;
pub mod text;
pub mod wire;

/// Run the window.
///
/// A path on the command line is opened before the window appears, which is what
/// a file manager's "open with" hands over and what a check with no click can
/// use.
pub fn run() {
    let host = commands::Host::new();
    if let Some(path) = std::env::args().nth(1) {
        let mut app = host.app.lock().expect("the session lock is never poisoned");
        let _ = app.open_path(std::path::Path::new(&path));
    }
    tauri::Builder::default()
        .manage(host)
        .on_window_event(|window, event| {
            // A window with no document open is still a place to drop one, so
            // the drop is turned into the same `open` call a menu would make.
            if let tauri::WindowEvent::DragDrop(tauri::DragDropEvent::Drop { paths, .. }) = event
                && let Some(path) = paths.first()
            {
                let _ = window.emit("sofd://drop", path.to_string_lossy().to_string());
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::snapshot,
            commands::messages,
            commands::glyphs,
            commands::page,
            commands::act,
            commands::press,
            commands::viewport,
            commands::settings,
            commands::open,
            commands::go_to,
            commands::zoom,
            commands::layout,
            commands::panel,
            commands::fit,
            commands::export,
            commands::collect,
        ])
        .run(tauri::generate_context!())
        .expect("the window could not start");
}
