//! The desktop reader, driven without a person: open a file, draw a frame, read
//! what the interface shows. `window::draw` is the whole interface, so it takes
//! an `egui::Ui` and nothing else: no window, no display server, which is how a
//! caller embeds it in their own `eframe` application or drives it in a test.
//!
//! `cargo run -p sofd-reader --example desktop_viewer -- <file.ofd>`

use eframe::egui;

use sofd_egui::{App, window};

fn main() {
    let path = std::env::args().nth(1);
    let mut app = App::new();
    if let Some(path) = path {
        // The same entry point a double-click or a share goes through, so what
        // this opens is what a launch by file association opens.
        if let Err(err) = app.open_path(std::path::Path::new(&path)) {
            eprintln!("{err:?}");
            return;
        }
    }

    // One pass into a context that owns no surface, which is the frame the
    // binary repeats with a window around it.
    let ctx = egui::Context::default();
    let mut drawn = None;
    let _ = ctx.run_ui(Default::default(), |ui| {
        drawn = Some(window::draw(ui, &mut app));
    });

    println!(
        "{:?}: {} page(s), showing page {}",
        app.file_name(),
        app.page_count(),
        app.current_page()
    );
    if let Some(requests) = drawn {
        // What the interface asked the window to do, which is how a file dialog
        // or a quit reaches the platform layer.
        println!("the frame asked for {requests:?}");
    }
}
