//! Export and print: handing work to the host.

use super::*;

impl App {
    /// Ask to export the pages on screen. The host answers with a
    /// destination, because only the platform knows where a file should land.
    pub fn request_export(&mut self, format: ExportFormat) {
        if !self.is_open() {
            return;
        }
        self.want_export = Some(format);
    }

    /// Whether the window should ask the platform for a destination.
    pub fn wants_export(&self) -> Option<ExportFormat> {
        self.want_export
    }

    /// Consume the export request, yielding the format and the pages to write.
    pub fn take_export_request(&mut self) -> Option<(ExportFormat, Vec<usize>)> {
        let format = self.want_export.take()?;
        let pages = self.export_pages();
        Some((format, pages))
    }

    /// The pages an export covers: the reader's selection rather than the whole
    /// document, so a spread exports as the two pages shown and a search result
    /// exports the page it found.
    pub fn export_pages(&self) -> Vec<usize> {
        if self.page_count() == 0 {
            return Vec::new();
        }
        self.visible()
    }

    /// Export `pages` as `format` into `destination`, off the draw path. The
    /// destination is the file for PDF and the directory for PNG. Starting an
    /// export drops the previous report, so the bar never describes a stale file.
    pub fn export_to(&mut self, format: ExportFormat, pages: Vec<usize>, destination: PathBuf) {
        let Some(bytes) = self.bytes.clone() else {
            return;
        };
        if pages.is_empty() {
            return;
        }
        // Read the stem before the queue is borrowed, since it needs `self`.
        let stem = self.export_stem();
        let dpi = self.dpi;
        self.export_generation += 1;
        let generation = self.export_generation;
        self.export_report = None;
        let queue = self
            .exports
            .get_or_insert_with(|| ExportQueue::spawn(bytes));
        queue.submit(ExportRequest {
            generation,
            format,
            pages,
            dpi,
            destination,
            stem,
        });
    }

    /// Whether an export is still being written.
    pub fn is_exporting(&self) -> bool {
        self.exports.as_ref().is_some_and(ExportQueue::is_busy)
    }

    /// What the newest finished export wrote, if it has finished.
    pub fn export_report(&self) -> Option<&ExportReport> {
        self.export_report.as_ref()
    }

    /// The filename stem exported files are named from. A dropped file has no
    /// path, so it falls back to a fixed name rather than files called nothing.
    fn export_stem(&self) -> String {
        self.path
            .as_ref()
            .and_then(|path| path.file_stem())
            .map(|stem| stem.to_string_lossy().into_owned())
            .unwrap_or_else(|| "document".to_string())
    }

    /// Collect finished exports into the report and the error line. Files
    /// written before a failure are kept in the report, because they exist.
    pub fn collect_exports(&mut self) {
        let Some(queue) = self.exports.as_mut() else {
            return;
        };
        for result in queue.drain() {
            match result.outcome {
                Ok(()) => {
                    self.export_report = Some(ExportReport {
                        format: result.format,
                        pages: result.pages,
                        written: result.written,
                    });
                    self.info = None;
                }
                // Files written before the failure still exist, so the report
                // is kept and the failure is stated rather than replacing it.
                Err(message) => self.set_error(message),
            }
        }
    }

    /// What the status bar says about the last export. The two formats count
    /// differently because they are different things: one PDF holding two pages
    /// is one file but two pages, and reporting files would halve a spread.
    pub fn export_label(&self) -> Option<String> {
        let report = self.export_report.as_ref()?;
        let path = report.written.first().map(|p| p.display().to_string());
        Some(match report.format {
            ExportFormat::Pdf => self.text_with(
                ReaderMessage::ExportPdfDone,
                [
                    ("count", (report.pages as i64).into()),
                    ("path", path.unwrap_or_default().into()),
                ],
            ),
            ExportFormat::Png => self.text_with(
                ReaderMessage::ExportPngDone,
                [
                    ("count", (report.written.len() as i64).into()),
                    ("path", path.unwrap_or_default().into()),
                ],
            ),
        })
    }

    /// Ask to print the pages on screen. The host answers by choosing a
    /// printer and handing the settings back, because only the platform knows
    /// what the printer on this machine can do.
    pub fn request_print(&mut self) {
        if !self.is_open() {
            return;
        }
        self.want_print = true;
    }

    /// Whether the host should ask the platform for a printer.
    pub fn wants_print(&self) -> bool {
        self.want_print
    }

    /// Consume the print request, yielding the pages to lay out.
    pub fn take_print_request(&mut self) -> Option<Vec<usize>> {
        if !std::mem::take(&mut self.want_print) {
            return None;
        }
        Some(self.print_pages())
    }

    /// The pages a print job covers: the reader's selection, not the whole
    /// document, so a spread prints as the two pages shown and a search result
    /// prints the page it found.
    pub fn print_pages(&self) -> Vec<usize> {
        if self.page_count() == 0 {
            return Vec::new();
        }
        self.visible()
    }

    /// Lay `pages` out under `settings` and rasterize them, off the draw path.
    /// Starting a job drops the previous report, so the status bar never
    /// describes a sheet that has already been replaced.
    pub fn print_to(&mut self, pages: Vec<usize>, settings: sofd_print::PrintSettings) {
        let Some(bytes) = self.bytes.clone() else {
            return;
        };
        if pages.is_empty() {
            return;
        }
        self.print_generation += 1;
        let generation = self.print_generation;
        self.print_report = None;
        self.print_settings = Some(settings.clone());
        let queue = self.prints.get_or_insert_with(|| PrintQueue::spawn(bytes));
        queue.submit(PrintRequest {
            generation,
            pages,
            settings,
        });
    }

    /// Whether a print job is still being laid out.
    pub fn is_printing(&self) -> bool {
        self.prints.as_ref().is_some_and(PrintQueue::is_busy)
    }

    /// What the newest finished job produced, if it has finished.
    pub fn print_report(&self) -> Option<&PrintReport> {
        self.print_report.as_ref()
    }

    /// Collect finished jobs into the report and the error line. A job that
    /// will not plan produces no sheets, so nothing is reported and the reason
    /// stands alone rather than beside an empty sheet count.
    pub fn collect_prints(&mut self) {
        let Some(queue) = self.prints.as_mut() else {
            return;
        };
        for result in queue.drain() {
            match result.outcome {
                Ok(()) => {
                    let settings = self
                        .print_settings
                        .clone()
                        .unwrap_or_else(|| self.print_settings());
                    self.print_report = Some(PrintReport {
                        sheets: result.planned,
                        sheet_count: result.sheets,
                        settings,
                    });
                    self.info = None;
                }
                Err(message) => self.set_error(message),
            }
        }
    }

    /// What the status bar says about the last job. It names the page count
    /// and the sheet count separately, so a job that puts two pages on one
    /// sheet does not look halved.
    pub fn print_label(&self) -> Option<String> {
        let report = self.print_report.as_ref()?;
        Some(
            self.text_with(
                ReaderMessage::PrintDone,
                [
                    ("pages", (report.sheets.len() as i64).into()),
                    ("sheets", (report.sheet_count as i64).into()),
                    (
                        "paper",
                        self.text(match report.settings.paper {
                            sofd_print::PaperSize::A3 => ReaderMessage::PaperA3,
                            sofd_print::PaperSize::A4 => ReaderMessage::PaperA4,
                            sofd_print::PaperSize::A5 => ReaderMessage::PaperA5,
                            sofd_print::PaperSize::B5 => ReaderMessage::PaperB5,
                            sofd_print::PaperSize::Letter => ReaderMessage::PaperLetter,
                            sofd_print::PaperSize::Legal => ReaderMessage::PaperLegal,
                            sofd_print::PaperSize::Document => ReaderMessage::PaperDocument,
                            // A custom size has no name in any language, so it
                            // is announced as the document's own paper rather
                            // than as a number the reader did not choose.
                            sofd_print::PaperSize::Custom { .. } => ReaderMessage::PaperDocument,
                        })
                        .into(),
                    ),
                ],
            ),
        )
    }

    /// The label on the toolbar control that starts a print job.
    pub fn print_label_text(&self) -> String {
        self.text(ReaderMessage::Print)
    }

    /// The settings a job runs under when nothing else says otherwise. The
    /// paper follows the document rather than a fixed size, so a page that is
    /// already A4 is not shrunk onto another sheet on the way to the printer.
    pub fn print_settings(&self) -> sofd_print::PrintSettings {
        sofd_print::PrintSettings {
            paper: sofd_print::PaperSize::Document,
            ..sofd_print::PrintSettings::default()
        }
    }

    /// Open a file the platform handed over, choosing bytes or path.
    /// Some desktops send bytes and others a path; the path is cheaper.
    pub fn open_dropped(&mut self, dropped: DroppedFile) -> Result<(), AppError> {
        match dropped {
            DroppedFile::Path(path) => self.open_path(&path),
            DroppedFile::Bytes { name, bytes } => self.open_bytes(&bytes, Some(&name)),
        }
    }

    /// What the canvas should draw, decided here so it can be tested.
    /// This is zone Z3: the window paints the answer and forms no opinion.
    /// It reports `Pending` rather than rendering on the draw path.
    pub fn canvas(&mut self) -> Canvas {
        if self.page_count() == 0 {
            return Canvas::Empty;
        }
        for page in self.visible() {
            let _ = self.page_image(page);
        }
        let page = self.current;
        match self.cache.get(page, self.dpi) {
            Some(rendered) if viewer::looks_blank(&rendered.pixels) => Canvas::Blank,
            Some(_) => Canvas::Page,
            None if self.error.is_some() => Canvas::Failed,
            None => Canvas::Pending,
        }
    }

    /// The window title for the current document.
    pub fn title(&self) -> String {
        match self.file_name() {
            Some(name) => format!("{name} — sOFD Reader"),
            None => "sOFD Reader".to_string(),
        }
    }
}
