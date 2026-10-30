//! What the window is told each time it asks: the document, where it is, what
//! is open beside it, and how it should look.
//!
//! A snapshot rather than a set of events: the session holds the truth, and a
//! webview that accumulated its own would be a second copy to keep in step.

use serde::Serialize;
use sofd_session::viewer::Layout;
use sofd_session::{App, Canvas, Fit, TextSize, ThemeChoice};

/// Everything the window needs to draw one frame.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    /// Whether a document is open.
    pub open: bool,
    /// The document's name, absent when nothing is open.
    pub title: Option<String>,
    /// The document's own description line, absent when nothing is open.
    pub info: Option<String>,
    /// Current page, counting from zero.
    pub page: usize,
    /// How many pages there are.
    pub page_count: usize,
    /// The pages on screen, which is one for single and two for facing.
    pub visible: Vec<usize>,
    /// `single`, `facing` or `continuous`.
    pub layout: &'static str,
    /// `width` or `page`, absent when the reader set a zoom by hand.
    pub fit: Option<&'static str>,
    /// The zoom, as the resolution the page is drawn at. 72 dpi is 100%, which
    /// is the figure a browser and Acrobat give an A4.
    pub dpi: f32,
    /// The zoom as a percentage, which is what the slider and its readout show.
    pub zoom_percent: f32,
    /// The zoom as the reader reads it, a percentage.
    pub zoom_label: String,
    /// `Page 2 of 7`, from the catalogue.
    pub page_label: String,
    /// What the canvas is showing, in the order the rule checks it.
    pub canvas: &'static str,
    /// The last error, absent when there is none.
    pub error: Option<String>,
    /// A note about the page that is on screen, absent when it drew whole.
    pub notice: Option<String>,
    /// The drop hint, shown where a document would go.
    pub drop_hint: String,
    /// Whether search results are shown.
    pub search_open: bool,
    /// Whether the signature report is shown.
    pub signatures_open: bool,
    /// Whether the annotation list is shown.
    pub annotations_open: bool,
    /// Whether a page is still being rasterized.
    pub rendering: bool,
    /// How the window should look.
    pub appearance: Appearance,
    /// Whether a page before this one exists.
    pub has_previous: bool,
    /// Whether a page after this one exists.
    pub has_next: bool,
}

/// The settings the reader chose, which belong to the window rather than to the
/// document and survive closing one.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Appearance {
    /// `system`, `light` or `dark`.
    pub theme: &'static str,
    /// The text size as a multiplier, applied to every style.
    pub text_scale: f32,
    /// Whether contrast is raised.
    pub high_contrast: bool,
}

/// Take a snapshot of the session.
pub fn snapshot(app: &mut App) -> Snapshot {
    let open = app.is_open();
    Snapshot {
        open,
        title: app.file_name(),
        info: app.info().map(str::to_owned),
        page: app.current_page(),
        page_count: app.page_count(),
        visible: app.visible(),
        layout: match app.layout() {
            Layout::Single => "single",
            Layout::Facing => "facing",
            Layout::Continuous => "continuous",
        },
        fit: app.fit().map(|fit| match fit {
            Fit::Width => "width",
            Fit::Page => "page",
        }),
        dpi: app.dpi(),
        zoom_percent: (app.zoom() * 100.0).round(),
        zoom_label: app.zoom_label(),
        page_label: app.page_label(),
        canvas: match app.canvas() {
            Canvas::Empty => "empty",
            Canvas::Failed => "failed",
            Canvas::Pending => "pending",
            Canvas::Blank => "blank",
            Canvas::Page => "page",
        },
        error: app.error().map(|err| err.message.clone()),
        notice: app
            .short_notice()
            .or_else(|| app.current_page_is_blank().then(|| app.blank_notice())),
        drop_hint: app.drop_hint(),
        search_open: app.search_open(),
        signatures_open: app.signatures_open(),
        annotations_open: app.annotations_open(),
        rendering: app.is_rendering(),
        appearance: Appearance {
            theme: match app.theme() {
                ThemeChoice::System => "system",
                ThemeChoice::Light => "light",
                ThemeChoice::Dark => "dark",
            },
            text_scale: app.text_size().scale(),
            high_contrast: app.high_contrast(),
        },
        has_previous: app.has_previous(),
        has_next: app.has_next(),
    }
}

/// The settings the reader can change, as the window names them.
pub fn text_scale(size: TextSize) -> f32 {
    size.scale()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn open() -> App {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../compliance/fixtures/minimal.ofd");
        let mut app = App::new();
        app.open_path(&path).expect("the fixture opens");
        app
    }

    /// An empty window says so, and says nothing else it cannot know. A title
    /// or a page count on a window with no document is the bug this catches.
    #[test]
    fn a_window_with_no_document_reports_only_its_hint() {
        let mut app = App::new();
        let snapshot = snapshot(&mut app);
        assert!(!snapshot.open);
        assert_eq!(snapshot.canvas, "empty");
        assert!(snapshot.title.is_none(), "no document, no name");
        assert!(snapshot.info.is_none());
        assert_eq!(snapshot.page_count, 0);
        assert!(!snapshot.drop_hint.trim().is_empty(), "the hint has words");
        assert!(snapshot.error.is_none(), "an empty window is not an error");
    }

    /// Every canvas state the session can report has a name the window knows,
    /// so a state added later cannot arrive as an unknown string.
    #[test]
    fn an_open_document_reports_a_state_the_window_can_draw() {
        let mut app = open();
        let snapshot = snapshot(&mut app);
        assert!(snapshot.open);
        assert_eq!(snapshot.page_count, 1);
        assert_eq!(snapshot.visible, vec![0]);
        assert_eq!(snapshot.layout, "single");
        assert!(
            ["empty", "failed", "pending", "blank", "page"].contains(&snapshot.canvas),
            "unknown canvas state {}",
            snapshot.canvas
        );
        assert!(!snapshot.zoom_label.is_empty());
        assert!(!snapshot.page_label.is_empty());
        assert_eq!(snapshot.appearance.theme, "system", "follows the desktop");
        assert!(!snapshot.appearance.high_contrast);
    }

    /// The appearance is the session's, not the window's: changing it in the
    /// session is how the window learns, and nothing else carries it.
    #[test]
    fn the_appearance_follows_the_session() {
        let mut app = open();
        app.set_theme(ThemeChoice::Dark);
        app.set_text_size(TextSize::Larger);
        app.set_high_contrast(true);
        let snapshot = snapshot(&mut app);
        assert_eq!(snapshot.appearance.theme, "dark");
        assert!(snapshot.appearance.text_scale > 1.0);
        assert!(snapshot.appearance.high_contrast);
        assert_eq!(snapshot.appearance.text_scale, text_scale(TextSize::Larger));
    }
}
