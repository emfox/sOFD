//! The reader executable: a frame around [`window::draw`], which holds the
//! layout so it can be driven headlessly. This file owns the platform parts
//! only, so a page keeps the frame and supplies its own entry in `web.rs`.

// A GUI app must not open a console window on Windows; the attribute is a
// no-op on the other systems, so it stays unconditional.
#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(not(target_os = "android"))]
use eframe::egui::{self, Ui};
#[cfg(not(target_os = "android"))]
use sofd_egui::App;
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
use sofd_egui::ExportFormat;
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
use sofd_egui::ReaderMessage;
#[cfg(not(target_os = "android"))]
use sofd_egui::window;
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
use sofd_i18n::MessageId;

#[cfg(target_arch = "wasm32")]
mod web;

/// The window state, boxed so `eframe` can own it.
///
/// Android has its own copy inside the library, because the device loads the
/// cdylib and the binary is never started there.
#[cfg(not(target_os = "android"))]
pub(crate) struct Reader {
    app: App,
    /// The title last pushed to the window, so it is pushed only on a change.
    title: String,
}

#[cfg(not(target_os = "android"))]
impl Reader {
    pub(crate) fn with_app(app: App) -> Self {
        Self {
            app,
            title: String::new(),
        }
    }
}

#[cfg(not(target_os = "android"))]
impl eframe::App for Reader {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let requests = window::draw(ui, &mut self.app);
        #[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
        self.serve_platform_requests(&requests);
        self.sync_title(ui);
        if requests.quit {
            ui.ctx().send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
}

#[cfg(not(target_os = "android"))]
impl Reader {
    /// Push the document's name into the window title bar, when it changes.
    fn sync_title(&mut self, ui: &mut Ui) {
        let title = self.app.title();
        if title == self.title {
            return;
        }
        self.title = title.clone();
        ui.ctx()
            .send_viewport_cmd(egui::ViewportCommand::Title(title));
    }
}

/// What the platform does on its own: choose a file, choose where one goes,
/// reach a printer. The web build has none of this, so the requests stay
/// pending there rather than being answered with a dialog that cannot open.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
impl Reader {
    fn serve_platform_requests(&mut self, requests: &window::WindowRequests) {
        if requests.open_dialog {
            open_with_dialog(&mut self.app);
        }
        if let Some(format) = requests.export {
            export_with_dialog(&mut self.app, format);
        }
        if requests.print {
            print_current_view(&mut self.app);
        }
        if requests.compare {
            compare_with_dialog(&mut self.app);
        }
    }
}

/// The platform asks for a second revision and compares it against the open
/// one. Cancelling is not a failure; a comparison that cannot run reports why.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
fn compare_with_dialog(app: &mut App) {
    let title = app.i18n().text(MessageId::CompareOpenSecond);
    let picked = rfd::FileDialog::new()
        .add_filter("OFD document", &["ofd"])
        .set_title(title)
        .pick_file();
    let Some(path) = picked else {
        return;
    };
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(err) => {
            app.set_error(format!("{}: {err}", path.display()));
            return;
        }
    };
    if let Err(err) = app.compare_bytes(&bytes) {
        app.set_error(err.message);
    }
}

/// Ask the platform for a file, then open it.
///
/// `egui` 0.35 dropped its built-in dialog, so this goes through the platform.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
fn open_with_dialog(app: &mut App) {
    let picked = rfd::FileDialog::new()
        .add_filter("OFD document", &["ofd"])
        .set_title(app.text(ReaderMessage::Open))
        .pick_file();
    let Some(path) = picked else {
        return;
    };
    if let Err(err) = app.open_path(&path) {
        app.set_error(err.message);
    }
}

/// Ask the platform where the export should go. A PDF is one file so the
/// platform names it; PNG writes one file per page, so it names a folder.
/// Cancelling is not a failure, so nothing is reported.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
fn export_with_dialog(app: &mut App, requested: ExportFormat) {
    // Consuming the request also learns the page selection, which the window
    // does not own. Cancelling the dialog leaves the request cleared, so the
    // platform is not asked again on the next frame.
    let Some((format, pages)) = app.take_export_request() else {
        return;
    };
    debug_assert_eq!(format, requested, "the window reported a different format");
    let destination = match format {
        ExportFormat::Pdf => {
            let title = app.text(ReaderMessage::ExportPdfTitle);
            let mut dialog = rfd::FileDialog::new().set_title(&title);
            if let Some(stem) = app.path().and_then(|p| p.file_stem()) {
                dialog = dialog.set_file_name(format!("{}.pdf", stem.to_string_lossy()));
            }
            dialog.add_filter("PDF document", &["pdf"]).save_file()
        }
        ExportFormat::Png => {
            let title = app.text(ReaderMessage::ExportPngTitle);
            rfd::FileDialog::new().set_title(&title).pick_folder()
        }
    };
    let Some(destination) = destination else {
        return;
    };
    app.export_to(format, pages, destination);
}

/// Print what the reader is looking at. Neither `egui` nor `eframe` prints, so
/// the pages on screen are laid out and rasterized here; reaching a printer is
/// the desktop's own path, and a job that will not rasterize is reported.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
fn print_current_view(app: &mut App) {
    let Some(pages) = app.take_print_request() else {
        return;
    };
    if pages.is_empty() {
        return;
    }
    app.print_to(pages, app.print_settings());
}

/// The native entry point: a window, a file dialog and a run loop. Android is
/// started by the system and the browser by a page, so this is absent there.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
fn main() -> eframe::Result {
    start(std::env::args().any(|arg| arg.starts_with("--renderer")))
}

/// The window settings one run starts with.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
fn window_options() -> eframe::NativeOptions {
    eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("sOFD Reader")
            .with_inner_size([1100.0, 820.0])
            .with_min_inner_size([640.0, 480.0]),
        renderer: renderer_from_args(),
        ..Default::default()
    }
}

/// Open the window and hand the process to the run loop.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
fn run(options: eframe::NativeOptions) -> eframe::Result {
    eframe::run_native(
        "sOFD Reader",
        options,
        Box::new(|cc| {
            // egui ships latin and cyrillic only, so the Chinese interface
            // needs a CJK face before the first frame draws.
            sofd_egui::fonts::install_cjk_fallback(&cc.egui_ctx);
            Ok(Box::new(Reader::with_app(App::new())))
        }),
    )
}

/// `--renderer` asks for, wgpu when it says nothing.
/// A driver that cannot make a requested device may die rather than return an
/// error, so the other renderer is tried in a child process when one is named.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
fn renderer_from_args() -> eframe::Renderer {
    renderer_from(std::env::args().skip(1))
}

/// The renderer named by the arguments, or the default when none is.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
fn renderer_from<I: Iterator<Item = String>>(args: I) -> eframe::Renderer {
    let mut args = args;
    while let Some(arg) = args.next() {
        let value = match arg.split_once('=') {
            Some(("--renderer", value)) => Some(value.to_owned()),
            Some(_) => None,
            None if arg == "--renderer" => args.next(),
            None => continue,
        };
        match value.as_deref() {
            Some("glow") => return eframe::Renderer::Glow,
            Some("wgpu") => return eframe::Renderer::Wgpu,
            // An unknown name is not worth refusing to start over: the window
            // is the point, and the default renderer is the one worth trying.
            _ => {}
        }
    }
    eframe::Renderer::default()
}

/// Start the window, trying the other renderer if the window fails to come up.
/// A driver can end the process by signal rather than return an error, which
/// cannot be caught, so a named first attempt runs in a child process.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
fn start(named: bool) -> eframe::Result {
    let retry = std::env::var_os(FELL_BACK).is_some();
    let mut options = window_options();
    if retry {
        options.renderer = other_renderer(options.renderer);
    }
    if !named || retry {
        return run(options);
    }
    // A named renderer is attempted in a child so a driver that kills the
    // process can be noticed at all; the child itself falls back when re-run.
    let program = std::env::current_exe().unwrap_or_else(|_| "sofd-reader".into());
    let status = std::process::Command::new(program)
        .args(std::env::args().skip(1))
        .env(FELL_BACK, "1")
        .status();
    match status {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => {
            eprintln!(
                "sofd-reader: neither renderer could open a window; the last exit was {status}"
            );
            std::process::exit(status.code().unwrap_or(1));
        }
        Err(err) => {
            eprintln!("sofd-reader: could not restart itself to try the other renderer: {err}");
            Ok(())
        }
    }
}

/// The renderer to use when the requested one could not open a window.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
fn other_renderer(renderer: eframe::Renderer) -> eframe::Renderer {
    match renderer {
        eframe::Renderer::Glow => eframe::Renderer::Wgpu,
        eframe::Renderer::Wgpu => eframe::Renderer::Glow,
    }
}

/// Whether the environment says this run is the retry after a failed one.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
const FELL_BACK: &str = "SOFD_RENDERER_FELL_BACK";

/// Neither of the other two builds has a `main` to start from.
#[cfg(any(target_arch = "wasm32", target_os = "android"))]
fn main() {}

#[cfg(all(test, not(any(target_arch = "wasm32", target_os = "android"))))]
mod tests {
    use super::{other_renderer, renderer_from};

    fn parsed(args: &[&str]) -> eframe::Renderer {
        renderer_from(args.iter().map(|arg| (*arg).to_owned()))
    }

    #[test]
    fn a_named_renderer_is_the_one_used() {
        assert!(matches!(
            parsed(&["--renderer", "glow"]),
            eframe::Renderer::Glow
        ));
        assert!(matches!(
            parsed(&["--renderer=wgpu"]),
            eframe::Renderer::Wgpu
        ));
    }

    #[test]
    fn no_argument_keeps_the_default() {
        assert!(matches!(parsed(&[]), eframe::Renderer::Wgpu));
        assert!(matches!(parsed(&["page.ofd"]), eframe::Renderer::Wgpu));
    }

    #[test]
    fn an_unknown_name_keeps_the_default() {
        assert!(matches!(
            parsed(&["--renderer", "software"]),
            eframe::Renderer::Wgpu
        ));
        assert!(matches!(parsed(&["--renderer"]), eframe::Renderer::Wgpu));
    }

    #[test]
    fn the_fallback_is_the_renderer_that_was_not_asked_for() {
        assert!(matches!(
            other_renderer(eframe::Renderer::Glow),
            eframe::Renderer::Wgpu
        ));
        assert!(matches!(
            other_renderer(eframe::Renderer::Wgpu),
            eframe::Renderer::Glow
        ));
    }
}
