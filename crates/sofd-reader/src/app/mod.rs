//! The reader's state, and the widgets that draw it.

use std::path::{Path, PathBuf};

use sofd::{Annotation, Comparison, Document, SearchOptions, VerifyStatus};
use sofd_i18n::{I18n, MessageId};

use crate::message::ReaderMessage;

use crate::export_worker::{ExportFormat, ExportQueue, ExportRequest};
use crate::print_worker::{PrintQueue, PrintRequest};
use crate::render_worker::RenderQueue;
use crate::shortcuts::{self, ZOOM_STEP};
use crate::viewer::{self, DroppedFile, Layout, PageCache};

pub use appearance::{TextSize, ThemeChoice};

/// A failure the reader shows in its own words.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppError {
    /// The message shown to the reader, already localized where possible.
    pub message: String,
}

/// Everything the window draws is derived from this.
pub struct App {
    i18n: I18n,
    doc: Option<Document>,
    /// The bytes the document came from, so the render worker can open its own
    /// handle rather than sharing this one across threads.
    bytes: Option<Vec<u8>>,
    queue: Option<RenderQueue>,
    path: Option<PathBuf>,
    cache: PageCache,
    current: usize,
    /// How large the page is drawn, as a share of its size at 72 points to the
    /// inch. This is the reader's zoom; `dpi` is what it costs to draw it.
    zoom: f32,
    dpi: f32,
    fit: Option<Fit>,
    layout: Layout,
    /// The text being looked for, empty when nothing is.
    query: String,
    /// Every hit for `query`, in page order.
    hits: Vec<Hit>,
    /// Which hit the reader is on, if any.
    hit_index: Option<usize>,
    /// Whether the search panel is showing.
    search_open: bool,
    /// The verification outcome for each signature, filled on first ask.
    signatures: Option<Vec<SignatureReport>>,
    /// Whether the signature panel is showing.
    signatures_open: bool,
    /// Every annotation in the document, filled on first ask.
    annotations: Option<Vec<Annotation>>,
    /// Whether the annotation panel is showing.
    annotations_open: bool,
    /// Which annotation the reader is on, if any.
    annotation_index: Option<usize>,
    /// The worker that converts and writes exported pages.
    exports: Option<ExportQueue>,
    /// The stamp the newest export was asked under.
    export_generation: u64,
    /// What the newest finished export wrote, for the status bar to report.
    export_report: Option<ExportReport>,
    /// The format the reader asked to export, waiting for a destination.
    want_export: Option<ExportFormat>,
    /// The worker that lays the pages out and rasterizes them for printing.
    prints: Option<PrintQueue>,
    /// The stamp the newest print job was asked under.
    print_generation: u64,
    /// What the newest finished job came to, for the status bar to report.
    print_report: Option<PrintReport>,
    /// Set while the reader has asked to print but the host has not answered.
    want_print: bool,
    /// The settings the newest job was submitted with, so the report quotes
    /// them rather than guessing them back from the rasters.
    print_settings: Option<sofd_print::PrintSettings>,
    /// The verdict on the last comparison against a second revision.
    comparison: Option<Comparison>,
    /// Whether the comparison panel is showing.
    comparison_open: bool,
    /// The theme the window draws in.
    theme: ThemeChoice,
    /// How large interface text is drawn.
    text_size: TextSize,
    /// Whether high contrast tightens the theme in force.
    high_contrast: bool,
    error: Option<AppError>,
    info: Option<String>,
    want_open: bool,
    /// The part of the current page a reader can see, in millimetres, when it
    /// is drawn larger than the texture limit allows. `None` means nothing has
    /// been drawn yet, so the page is asked for whole.
    viewport_mm: Option<sofd_common::Rect>,
    /// The largest texture the backend says it can hold, in pixels. It starts
    /// at the crate's floor and is replaced by what the driver reports, so a
    /// desktop card is not held to a limit chosen for the weakest board.
    max_texture_side: u32,
    /// A document the platform handed over before the frame could take it. A
    /// host that learns of a file outside a frame — Android reads it out of the
    /// activity before the event loop is running — has nowhere else to put it,
    /// because a frame draws the state it is given rather than waiting for one.
    pending_open: Option<DroppedFile>,
    want_quit: bool,
}

/// What one finished export produced, in the reader's terms.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportReport {
    /// What was produced.
    pub format: ExportFormat,
    /// How many pages went in.
    pub pages: usize,
    /// Where the files landed.
    pub written: Vec<PathBuf>,
}

/// What one finished print job came to, in the reader's terms. The sheets are
/// kept rather than thrown away, so a caller can tell what a job produced
/// without repeating the rasterization to find out.
#[derive(Debug, Clone)]
pub struct PrintReport {
    /// What each planned page came to.
    pub sheets: Vec<crate::print_worker::SheetReport>,
    /// How many sheets the job consumes, which duplexing halves.
    pub sheet_count: usize,
    /// The settings the job ran under, kept for the status bar's wording.
    pub settings: sofd_print::PrintSettings,
}

/// What the reader needs to know about one signature. The verdict comes from
/// the engine; who signed and when come from the descriptor, read alongside
/// it, so the panel never reaches into the engine itself.
#[derive(Debug, Clone, PartialEq)]
pub struct SignatureReport {
    /// The signature's own identifier, as written in the document.
    pub id: Option<String>,
    /// How the signature stands, which is not a yes-or-no question.
    pub verdict: Verdict,
    /// The name of the signing provider, when the document records one.
    pub signer: Option<String>,
    /// When it was signed, as written.
    pub signed_at: Option<String>,
    /// The algorithm named in the descriptor.
    pub algorithm: Option<String>,
    /// What the certificate says about the signer: the issuer, the window it is
    /// valid in, and for an invoice the seal's own text. Kept on the report
    /// rather than fetched when a dialog opens, because reading it costs a
    /// second pass over the container and the panel needs it to draw.
    pub details: sofd_sign::details::SignatureDetails,
    /// Where the seals this signature carries are drawn, one entry per stamp.
    pub seals: Vec<SealBox>,
}

/// One seal's place on a page, in page millimetres.
///
/// This is what makes a seal clickable: the reader hit-tests the pointer
/// against these boxes rather than against the drawn pixels, so the whole seal
/// answers to a click even where its ink is thin.
#[derive(Debug, Clone, PartialEq)]
pub struct SealBox {
    /// The page the seal is on, as an index.
    pub page: usize,
    /// Where it sits, in page millimetres.
    pub boundary: sofd_common::Rect,
}

/// How one signature stands. Three states rather than two: a proof that fails
/// and a proof that cannot be checked call for different reactions, and
/// flattening them calls a document tampered with when it is only unconfirmable.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// The signature matches what it covers.
    Valid,
    /// The content no longer matches the signed digest.
    Tampered,
    /// The signature could not be checked at all, with the reason why.
    Unverified,
    /// The proof is sound, but its certificate chains to no root the caller
    /// named, so the signer is not one the reader was told to trust.
    Untrusted,
}

impl Verdict {
    /// The engine already keeps an unchecked signature apart from a tampered
    /// one, so this is a rename rather than a judgement.
    fn from_status(status: VerifyStatus) -> Self {
        match status {
            VerifyStatus::Valid => Self::Valid,
            VerifyStatus::Tampered => Self::Tampered,
            VerifyStatus::Unchecked => Self::Unverified,
            VerifyStatus::Untrusted => Self::Untrusted,
        }
    }
}

/// A resolution the reader keeps chasing as the window or page changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fit {
    /// Fill the available width with the page.
    Width,
    /// Show the whole page inside the available area.
    Page,
}

/// One search result, reduced to what the reader needs to show and jump to.
/// The engine's hit also carries run and op indices and byte offsets, which
/// matter to whoever edits the text rather than to a highlight.
#[derive(Debug, Clone, PartialEq)]
pub struct Hit {
    /// Page the match is on.
    pub page: usize,
    /// The text around the match, for the result list.
    pub snippet: String,
    /// Where the match is on the page, in page space.
    pub rect: sofd_common::Rect,
}

impl Default for App {
    fn default() -> Self {
        Self::new()
    }
}

mod appearance;
mod compare;
mod export;
mod mark;
mod search;
mod sign;

impl App {
    /// A reader with no document open.
    pub fn new() -> Self {
        Self {
            i18n: crate::message::bundle(),
            doc: None,
            bytes: None,
            queue: None,
            path: None,
            cache: PageCache::new(12),
            current: 0,
            zoom: viewer::DEFAULT_ZOOM,
            dpi: viewer::DEFAULT_DPI,
            fit: None,
            layout: Layout::Single,
            query: String::new(),
            hits: Vec::new(),
            hit_index: None,
            search_open: false,
            signatures: None,
            signatures_open: false,
            annotations: None,
            annotations_open: false,
            annotation_index: None,
            exports: None,
            export_generation: 0,
            export_report: None,
            want_export: None,
            prints: None,
            print_generation: 0,
            print_report: None,
            want_print: false,
            print_settings: None,
            comparison: None,
            comparison_open: false,
            theme: ThemeChoice::default(),
            text_size: TextSize::default(),
            high_contrast: false,
            error: None,
            info: None,
            want_open: false,
            viewport_mm: None,
            max_texture_side: viewer::MAX_TEXTURE_SIDE,
            pending_open: None,
            want_quit: false,
        }
    }

    /// The open document, if there is one.
    pub fn document(&self) -> Option<&Document> {
        self.doc.as_ref()
    }

    /// The file the open document came from.
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// The page the reader is showing.
    pub fn current_page(&self) -> usize {
        self.current
    }

    /// Whether a document is open.
    pub fn is_open(&self) -> bool {
        self.doc.is_some()
    }

    /// How many pages the open document has.
    pub fn page_count(&self) -> usize {
        self.doc.as_ref().map_or(0, Document::page_count)
    }

    /// How pages are laid out.
    pub fn layout(&self) -> Layout {
        self.layout
    }

    /// The last failure, if the reader has one to show.
    pub fn error(&self) -> Option<&AppError> {
        self.error.as_ref()
    }

    /// Ask for a file to open; the window turns this into a dialog.
    pub fn request_open(&mut self) {
        self.want_open = true;
    }

    /// Whether the window should open a file dialog this frame.
    pub fn wants_open(&self) -> bool {
        self.want_open
    }

    /// Consume the open request.
    pub fn take_open_request(&mut self) -> bool {
        std::mem::take(&mut self.want_open)
    }

    /// Whether the window should close.
    pub fn wants_quit(&self) -> bool {
        self.want_quit
    }

    /// Ask the window to close.
    pub fn quit(&mut self) {
        self.want_quit = true;
    }

    /// Open a document from disk, replacing whatever was open.
    pub fn open_path(&mut self, path: &Path) -> Result<(), AppError> {
        let bytes = std::fs::read(path).map_err(|err| AppError {
            message: format!("{}: {err}", path.display()),
        })?;
        self.install(&bytes, Some(path.to_path_buf()))?;
        self.info = Some(self.describe());
        Ok(())
    }

    /// Open a document already in memory, as a dropped file arrives.
    pub fn open_bytes(&mut self, bytes: &[u8], name: Option<&str>) -> Result<(), AppError> {
        self.install(bytes, name.map(PathBuf::from))?;
        self.info = Some(self.describe());
        Ok(())
    }

    /// Replace the open document, keeping the bytes for the render worker.
    /// A failed open changes nothing, so a good document is never lost.
    fn install(&mut self, bytes: &[u8], path: Option<PathBuf>) -> Result<(), AppError> {
        let doc = Document::open_bytes(bytes).map_err(|err| AppError {
            message: err.to_string(),
        })?;
        self.cache.clear();
        self.current = 0;
        self.fit = None;
        self.doc = Some(doc);
        self.bytes = Some(bytes.to_vec());
        self.queue = Some(RenderQueue::spawn(bytes.to_vec()));
        self.path = path;
        self.error = None;
        Ok(())
    }

    /// Close the open document and drop its pages.
    pub fn close(&mut self) {
        self.doc = None;
        self.bytes = None;
        self.queue = None;
        self.path = None;
        self.cache.clear();
        self.current = 0;
        self.fit = None;
        self.info = None;
        self.query.clear();
        self.hits.clear();
        self.hit_index = None;
        self.search_open = false;
        self.signatures = None;
        self.signatures_open = false;
        self.annotations = None;
        self.annotations_open = false;
        self.annotation_index = None;
        self.exports = None;
        self.export_report = None;
        self.want_export = None;
        self.prints = None;
        self.print_report = None;
        self.want_print = false;
        self.print_settings = None;
        self.comparison = None;
        self.comparison_open = false;
    }

    /// Take a document the platform handed over outside a frame.
    ///
    /// It is stored rather than opened, because the frame that opens it is the
    /// one that can report a failure: an open that fails before the window
    /// exists would set an error nothing had yet drawn.
    pub fn hand_over(&mut self, dropped: DroppedFile) {
        self.pending_open = Some(dropped);
    }

    /// Open whatever was handed over, and report the failure the way a drop
    /// does. Called once per frame before anything else is drawn.
    pub fn take_handed_over(&mut self) -> bool {
        let Some(dropped) = self.pending_open.take() else {
            return false;
        };
        // A device has no console, so the outcome is the only record of whether
        // the hand-over reached the frame that opens it: the log is where an
        // empty window is told apart from a document that was refused.
        #[cfg(target_os = "android")]
        log::info!("opening the document the platform handed over");
        if let Err(err) = self.open_dropped(dropped) {
            #[cfg(target_os = "android")]
            log::error!("the handed-over document was refused: {}", err.message);
            #[cfg(not(target_os = "android"))]
            tracing::error!("the handed-over document was refused: {}", err.message);
            self.set_error(err.message);
        }
        true
    }

    /// The one-line description of the open document, for the status bar.
    pub fn describe(&self) -> String {
        let Some(doc) = self.doc.as_ref() else {
            return String::new();
        };
        let pages = self.text_with(
            ReaderMessage::PageOf,
            [
                ("current", "0".into()),
                ("total", doc.page_count().to_string().into()),
            ],
        );
        format!("{} · {pages}", doc.version())
    }

    /// The description captured when the document was opened, if one is open.
    pub fn info(&self) -> Option<&str> {
        self.info.as_deref()
    }

    /// The file name of the open document, for the window title.
    pub fn file_name(&self) -> Option<String> {
        self.path
            .as_ref()
            .and_then(|p| p.file_name())
            .map(|n| n.to_string_lossy().into_owned())
    }

    /// Show a page, clamped to the pages that exist.
    pub fn go_to(&mut self, page: usize) {
        if self.page_count() == 0 {
            self.current = 0;
            return;
        }
        self.current = page.min(self.page_count() - 1);
    }

    /// Step forward one screen, by the layout's stride.
    pub fn next_page(&mut self) {
        self.go_to(self.current + self.layout.stride());
    }

    /// Step back one screen, by the layout's stride.
    pub fn previous_page(&mut self) {
        self.current = self.current.saturating_sub(self.layout.stride());
    }

    /// The pages the canvas should show, given the layout. This is zone Z3's
    /// input: the window paints these and forms no opinion about which they are.
    pub fn visible(&self) -> Vec<usize> {
        viewer::visible_pages(self.layout, self.current, self.page_count())
    }

    /// Whether there is a screen after the current one.
    pub fn has_next(&self) -> bool {
        self.current + self.layout.stride() < self.page_count()
    }

    /// Whether there is a screen before the current one.
    pub fn has_previous(&self) -> bool {
        self.current >= self.layout.stride()
    }

    /// Set the zoom, clamped to the range a reader can use, and re-rasterize
    /// at the resolution that zoom needs. The page is drawn at this size, so
    /// the reader sees it grow; the raster only keeps it sharp.
    pub fn set_zoom(&mut self, zoom: f32) {
        let zoom = zoom.clamp(viewer::MIN_ZOOM, viewer::MAX_ZOOM);
        if zoom != self.zoom {
            self.zoom = zoom;
            self.resize();
        }
        self.fit = None;
    }

    /// The zoom the page is drawn at, as a share of its 72 dpi size.
    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    /// Keep the page fitted to the space it is drawn in.
    pub fn set_fit(&mut self, fit: Fit) {
        self.fit = Some(fit);
    }

    /// The fit the reader is holding, if any.
    pub fn fit(&self) -> Option<Fit> {
        self.fit
    }

    /// Recompute the zoom for the fit in force, given the space the canvas
    /// has. Called by the window each frame, so a resize re-fits.
    pub fn apply_fit(&mut self, available: (f32, f32)) {
        let Some(fit) = self.fit else {
            return;
        };
        let Some(size_mm) = self.page_size_mm(self.current) else {
            return;
        };
        let zoom = viewer::fit_zoom(fit, size_mm, available, self.max_texture_side);
        if zoom != self.zoom {
            self.zoom = zoom;
            self.resize();
        }
    }

    /// The resolution this zoom needs, had the reader the screen to show it.
    pub fn dpi(&self) -> f32 {
        (self.zoom * 72.0).clamp(viewer::MIN_DPI, viewer::MAX_DPI)
    }

    /// Bring the raster in line with the zoom. A page past the texture limit is
    /// not cut short here: the raster takes the part on screen at full
    /// resolution, so the reader's zoom is what decides the size.
    fn resize(&mut self) {
        self.dpi = self.dpi();
        if let Some(queue) = self.queue.as_mut() {
            queue.rezoom();
        }
    }

    /// The physical size of one page in millimetres, if it has one. The canvas
    /// needs it to turn a hit rectangle in page space into one on screen.
    pub fn page_size_mm(&self, page: usize) -> Option<(f32, f32)> {
        self.doc
            .as_ref()
            .and_then(|doc| doc.page(page))
            .map(|page| (page.page_box.w as f32, page.page_box.h as f32))
    }

    /// Choose how pages are laid out.
    pub fn set_layout(&mut self, layout: Layout) {
        self.layout = layout;
    }

    /// Record a failure to show in the status bar.
    pub fn set_error(&mut self, message: impl Into<String>) {
        self.error = Some(AppError {
            message: message.into(),
        });
    }

    /// Clear the last failure.
    pub fn clear_error(&mut self) {
        self.error = None;
    }

    /// The sentence the canvas shows when a page would not rasterize.
    /// The status bar carries the engine's own message, so the canvas says
    /// only that something failed rather than repeating it verbatim.
    pub fn error_label(&self) -> String {
        self.text(ReaderMessage::ErrorLabel)
    }

    /// The reader's language bundle, for widgets that need a string.
    pub fn i18n(&self) -> &I18n {
        &self.i18n
    }

    /// One of the reader's own messages, in the current language.
    pub fn text(&self, message: crate::message::ReaderMessage) -> String {
        self.i18n.text_key(message.key())
    }

    /// One of the reader's own messages, with arguments.
    pub fn text_with<'a, I>(&self, message: crate::message::ReaderMessage, args: I) -> String
    where
        I: IntoIterator<Item = (&'a str, sofd_i18n::FluentValue<'a>)>,
    {
        self.i18n.text_key_with(message.key(), args)
    }

    /// The locale every label is currently drawn in.
    pub fn locale(&self) -> sofd_i18n::Locale {
        self.i18n.locale().clone()
    }

    /// The locales this build can switch to, from the bundled catalogues.
    pub fn locales(&self) -> Vec<sofd_i18n::Locale> {
        self.i18n.available_locales().to_vec()
    }

    /// Switch every label to `locale`. The page cache survives, because the
    /// pages are pixels and a label is a word: a translated interface does not
    /// make a rendered page wrong.
    pub fn set_locale(&mut self, locale: sofd_i18n::Locale) {
        if self.i18n.locale() == &locale {
            return;
        }
        self.i18n.set_locale(locale);
        // The description is assembled from labels, so it is rebuilt — but only
        // while a document is open, because with none there is nothing to say.
        if self.doc.is_some() {
            self.info = Some(self.describe());
        }
    }

    /// The name of `locale` written in that locale, so a reader who cannot
    /// read the current language can still find theirs. It comes from the
    /// catalogue because it is text, and all the reader's text lives there.
    pub fn locale_name(&self, locale: &sofd_i18n::Locale) -> String {
        let key = match locale.tag() {
            "zh-CN" => MessageId::LocaleNameZh,
            _ => MessageId::LocaleNameEn,
        };
        self.i18n.text(key)
    }

    /// The rendered page if it is cached at the current resolution, and one
    /// that covers what the reader can see. Asked by area rather than by key,
    /// so a scroll the margin already paid for still finds its raster.
    pub fn cached(&self, page: usize) -> Option<&viewer::RenderedPage> {
        let window = if page == self.current {
            self.window_for_current()
        } else {
            None
        };
        self.cache.covering(page, self.dpi, window)
    }

    /// The part of a page to rasterize, in millimetres; `None` is the whole
    /// page, because it fits the limit or because no viewport has been drawn
    /// yet. Only the current page has one: a spread or a column is zoomed out
    /// to fit, so its pages are asked for whole.
    fn window_mm(&self, page: usize) -> Option<sofd_common::Rect> {
        if page != self.current {
            return None;
        }
        self.window_for_current()
    }

    /// The window this reader is looking at, or `None` when the page fits
    /// whole. Split from `window_mm` because the draw path asks it on a shared
    /// borrow, and a page other than the current one is never windowed.
    fn window_for_current(&self) -> Option<sofd_common::Rect> {
        let size_mm = self.page_size_mm(self.current)?;
        if self.dpi <= viewer::dpi_within(size_mm, self.max_texture_side) {
            return None;
        }
        let visible = self.viewport_mm?;
        Some(viewer::window_for(
            visible,
            size_mm,
            self.dpi,
            self.max_texture_side,
        ))
    }

    /// Tell the reader which part of the current page is on screen, in
    /// millimetres of page space. The canvas calls this each frame; the raster
    /// for it is asked for on the next one, so scrolling costs a frame.
    pub fn set_viewport(&mut self, visible: sofd_common::Rect) {
        self.viewport_mm = Some(visible);
    }

    /// Take the texture limit the backend reports. A raised limit is a sharper
    /// page at the same zoom, so the raster in hand is no longer what this
    /// zoom asks for and is rendered again.
    pub fn set_texture_limit(&mut self, side: u32) {
        let side = side.max(1);
        if side == self.max_texture_side {
            return;
        }
        self.max_texture_side = side;
        // The page still on screen was rasterized under the old limit. If the
        // limit grew, the part it covers may now fit at a higher resolution.
        self.resize();
    }

    /// The texture limit in force, in pixels.
    pub fn texture_limit(&self) -> u32 {
        self.max_texture_side
    }

    /// Rasterize a page, or return the cached one.
    /// Rendering happens off the draw path, so this asks the worker and does
    /// not wait; the page arrives through `poll` on a later frame.
    pub fn page_image(&mut self, page: usize) -> Result<(), AppError> {
        if self.doc.is_none() {
            return Ok(());
        }
        if self.page_size_mm(page).is_none() {
            return Ok(());
        }
        let window = self.window_mm(page);
        // Any raster that covers the view answers, whatever window it was
        // asked for: that is what keeps a scroll inside the margin free. A
        // whole page at a resolution too low to hold is not one of them.
        if self.cache.covering(page, self.dpi, window).is_some() {
            return Ok(());
        }
        if let Some(queue) = self.queue.as_mut() {
            queue.request(page, self.dpi, window, self.max_texture_side);
        }
        Ok(())
    }

    /// Take whatever the worker has finished and put it in the cache.
    /// Called once per frame before anything is drawn, so a page that landed
    /// during the last frame is on screen in this one.
    pub fn poll(&mut self) {
        self.collect_exports();
        self.collect_prints();
        let Some(queue) = self.queue.as_mut() else {
            return;
        };
        for result in queue.drain() {
            // A raster of a resolution the reader has already left is stale:
            // it goes into the cache under its own resolution, where a reader
            // who comes back to that zoom will find it, but it is not drawn.
            match result.outcome {
                Ok(rendered) => self.cache.insert(result.page, result.dpi, rendered),
                Err(message) => self.set_error(message),
            }
        }
    }

    /// Whether the worker still owes the reader a page at the current zoom.
    pub fn is_rendering(&self) -> bool {
        self.queue.as_ref().is_some_and(RenderQueue::is_busy)
    }

    /// The page label shown under the window.
    pub fn page_label(&self) -> String {
        self.text_with(
            ReaderMessage::PageOf,
            [
                ("current", (self.current + 1).to_string().into()),
                ("total", self.page_count().to_string().into()),
            ],
        )
    }

    /// Act on a keyboard shortcut. Paging goes through the same methods the
    /// toolbar buttons call, so a key and a click cannot drift apart, and it
    /// is all testable without a keyboard.
    pub fn apply(&mut self, action: shortcuts::Action) {
        use shortcuts::Action;
        match action {
            Action::NextPage => self.next_page(),
            Action::PreviousPage => self.previous_page(),
            Action::FirstPage => self.go_to(0),
            Action::LastPage => self.go_to(self.page_count().saturating_sub(1)),
            Action::ZoomIn => self.step_zoom(ZOOM_STEP),
            Action::ZoomOut => self.step_zoom(-ZOOM_STEP),
            Action::ZoomReset => self.set_zoom(viewer::DEFAULT_ZOOM),
            Action::NextHit => self.next_hit(),
            Action::PreviousHit => self.previous_hit(),
            Action::ToggleSearch => self.set_search_open(!self.search_open),
            Action::ToggleSignatures => self.set_signatures_open(!self.signatures_open),
            Action::ToggleAnnotations => self.set_annotations_open(!self.annotations_open),
            Action::NextAnnotation => {
                // Opening the panel first, because stepping through marks the
                // reader cannot see would move the document for no visible
                // reason.
                self.set_annotations_open(true);
                self.next_annotation();
            }
            Action::PreviousAnnotation => {
                self.set_annotations_open(true);
                self.previous_annotation();
            }
            // Focus and Escape are the window's business: they are about where
            // the caret is, which the state does not track.
            Action::FocusSearch | Action::Escape => {}
            Action::Open => self.request_open(),
            Action::ExportPdf => self.request_export(crate::export_worker::ExportFormat::Pdf),
            Action::Print => self.request_print(),
        }
    }

    /// Move the zoom by `delta` percent, staying inside the permitted range.
    fn step_zoom(&mut self, delta: f32) {
        self.set_zoom(self.zoom + delta / 100.0);
    }

    /// The zoom label shown in the toolbar.
    pub fn zoom_label(&self) -> String {
        self.text_with(
            ReaderMessage::Zoom,
            [(
                "percent",
                ((self.zoom * 100.0).round() as i32).to_string().into(),
            )],
        )
    }

    /// The document line shown in the toolbar, localized.
    pub fn info_label(&self) -> String {
        match self.file_name() {
            Some(name) => self.text_with(ReaderMessage::Info, [("name", name.into())]),
            None => String::new(),
        }
    }

    /// Whether the current page holds no visible ink.
    /// It asks for the page first, but the answer is only known once the
    /// raster has landed; an uncached page is not blank, merely unknown.
    pub fn current_page_is_blank(&mut self) -> bool {
        if self.page_count() == 0 {
            return true;
        }
        let page = self.current;
        let _ = self.page_image(page);
        self.cache
            .get(page, self.dpi)
            .is_some_and(|rendered| viewer::looks_blank(&rendered.pixels))
    }

    /// Whether the page at `page` is cached and holds no visible ink.
    pub fn page_is_blank(&self, page: usize) -> bool {
        self.cache
            .get(page, self.dpi)
            .is_some_and(|rendered| viewer::looks_blank(&rendered.pixels))
    }

    /// The notice shown when the current page has nothing on it.
    pub fn blank_notice(&self) -> String {
        self.text(ReaderMessage::BlankPage)
    }

    /// What the current page failed to draw, as a sentence, or nothing when it
    /// drew everything. The counts come out of the raster, which is the only
    /// place that knows. One cause is reported rather than all: a font that
    /// would not load takes its glyphs with it.
    pub fn short_notice(&self) -> Option<String> {
        let short = self.cache.get(self.current, self.dpi)?.short;
        let (message, count) = if short.glyphs > 0 {
            (ReaderMessage::ShortGlyphs, short.glyphs)
        } else if short.font_runs > 0 {
            (ReaderMessage::ShortFonts, short.font_runs)
        } else if short.images > 0 {
            (ReaderMessage::ShortImages, short.images)
        } else {
            return None;
        };
        Some(self.text_with(message, [("count", count.to_string().into())]))
    }

    /// The hint shown on the empty window, where a file can be dropped.
    ///
    /// Android has nothing to drop onto, so the hint there names the one action
    /// that is drawn — the open button — rather than telling the reader to
    /// bring a file in from another app.
    pub fn drop_hint(&self) -> String {
        #[cfg(target_os = "android")]
        {
            self.text(ReaderMessage::OpenHint)
        }
        #[cfg(not(target_os = "android"))]
        {
            self.text(ReaderMessage::DropHint)
        }
    }
}

/// The four states of the canvas, in the order the window checks them.
///
/// A blank page is content, so it is painted like a page and only annotated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Canvas {
    /// No document is open, so the drop hint is shown instead.
    Empty,
    /// The page would not rasterize. The wording lives in Z4.
    Failed,
    /// The page is rasterizing and has not landed in the cache yet.
    Pending,
    /// The page rasterized to nothing but white.
    Blank,
    /// The page is ready to paint.
    Page,
}

#[cfg(test)]
mod tests {
    use super::*;
    use sofd_common::Rect;

    fn open() -> App {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../compliance/fixtures/minimal.ofd");
        let mut app = App::new();
        app.open_path(&path).expect("the fixture opens");
        app
    }

    /// A page that fits is asked for whole; a page that does not is asked for
    /// the part on screen. Asking for the whole page anyway is what made the
    /// reader rasterize the same corner at every scroll position.
    #[test]
    fn the_raster_window_follows_the_zoom_and_the_viewport() {
        let mut app = open();
        app.set_viewport(Rect::new(0.0, 0.0, 40.0, 40.0));
        app.set_zoom(1.0);
        assert!(
            app.window_mm(0).is_none(),
            "a page that fits the texture limit is asked for whole"
        );

        app.set_zoom(viewer::MAX_ZOOM);
        let visible = Rect::new(90.0, 200.0, 40.0, 40.0);
        app.set_viewport(visible);
        let window = app
            .window_mm(0)
            .expect("a zoom past the limit is asked for in parts");
        assert!(
            viewer::covers(Some(window), visible),
            "the raster {window:?} does not cover what is on screen {visible:?}"
        );
        assert!(
            window.x > 0.0 || window.y > 0.0,
            "the window is still the page's corner: {window:?}"
        );
    }

    /// A driver that offers more texture than the floor changes what the
    /// reader asks for: the same zoom becomes one whole-page raster instead of
    /// a windowed one, which is both sharper and cheaper to scroll.
    #[test]
    fn a_bigger_texture_limit_asks_for_a_whole_page_sooner() {
        let mut app = open();
        app.set_zoom(3.0);
        app.set_viewport(Rect::new(40.0, 40.0, 60.0, 60.0));
        app.set_texture_limit(viewer::MAX_TEXTURE_SIDE);
        assert!(
            app.window_mm(0).is_some(),
            "at {} dpi a page is past the {} px floor and has to be windowed",
            app.dpi,
            viewer::MAX_TEXTURE_SIDE
        );
        app.set_texture_limit(8192);
        assert!(
            app.window_mm(0).is_none(),
            "the same zoom fits whole once the driver reports 8192 px"
        );
    }

    /// The whole point of the higher ceiling: a reader can resolve the detail
    /// a vector page still holds at 800%. The zoom a reader asks for is clamped
    /// to the range, and 800% has to be inside it.
    #[test]
    fn the_reader_can_zoom_past_eight_hundred_percent() {
        let mut app = open();
        app.set_zoom(1000.0);
        assert!(
            app.zoom() >= 8.0,
            "the zoom stops at {:.0}%, short of the 800% a page of outlines deserves",
            app.zoom() * 100.0
        );
        assert_eq!(
            app.dpi(),
            viewer::MAX_DPI,
            "the ceiling is what the highest zoom costs to draw"
        );
    }

    /// A page that drew less than it holds says so, and says which way: the
    /// first number is the one that explains the rest, because a font that
    /// would not load takes its glyphs with it.
    #[test]
    fn a_page_that_is_short_says_which_way() {
        let mut app = open();
        let dpi = app.dpi;
        app.cache.insert(
            0,
            dpi,
            crate::viewer::RenderedPage {
                pixels: viewer::Rgba {
                    width: 1,
                    height: 1,
                    bytes: vec![255, 255, 255, 255],
                },
                size_mm: (210.0, 297.0),
                window: None,
                short: crate::viewer::Short {
                    glyphs: 7,
                    font_runs: 2,
                    images: 1,
                },
            },
        );
        let notice = app.short_notice().expect("a short page reports");
        assert!(
            notice.contains('7'),
            "the glyph count is what explains the rest: {notice}"
        );
        assert!(!notice.contains('2'), "one cause is reported: {notice}");
    }

    #[test]
    fn a_complete_page_reports_nothing() {
        let mut app = open();
        assert!(app.short_notice().is_none(), "nothing has been drawn yet");
        let dpi = app.dpi;
        app.cache.insert(
            0,
            dpi,
            crate::viewer::RenderedPage {
                pixels: viewer::Rgba {
                    width: 1,
                    height: 1,
                    bytes: vec![255, 255, 255, 255],
                },
                size_mm: (210.0, 297.0),
                window: None,
                short: crate::viewer::Short::default(),
            },
        );
        assert!(app.short_notice().is_none(), "a complete page says nothing");
    }

    /// Only the page being read has a viewport, because only it can be zoomed
    /// past the limit. Its neighbours in a spread are drawn to fit.
    #[test]
    fn a_neighbouring_page_is_asked_for_whole() {
        let mut app = open();
        app.set_viewport(Rect::new(90.0, 200.0, 40.0, 40.0));
        app.set_zoom(viewer::MAX_ZOOM);
        assert!(app.window_mm(0).is_some());
        assert!(
            app.window_mm(1).is_none(),
            "a page the reader is not on has no viewport to narrow to"
        );
    }
}
