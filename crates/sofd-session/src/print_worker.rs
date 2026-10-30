//! Printing the pages on screen, off the draw path. Rasterizing a sheet at
//! print resolution costs far more than drawing one, so it runs on its own
//! thread for the same reason rendering does.

use sofd_print::{PageSide, PrintJob, PrintSettings};

/// One unit of work: lay `pages` out with `settings`.
#[derive(Debug, Clone)]
pub struct PrintRequest {
    /// Monotonic stamp, so a result can be told from a later request's.
    pub generation: u64,
    /// The pages to include, in order; a selection, not a span.
    pub pages: Vec<usize>,
    /// Paper, margins, scale, copies and resolution.
    pub settings: PrintSettings,
}

/// One planned sheet, reduced to what the reader reports about it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SheetReport {
    /// Zero-based index into the document's pages.
    pub page_index: usize,
    /// Zero-based sheet within the job.
    pub sheet: usize,
    /// Which face of the sheet the page prints on.
    pub side: PageSide,
    /// Sheet width in pixels at the job's resolution.
    pub width: u32,
    /// Sheet height in pixels at the job's resolution.
    pub height: u32,
}

/// A finished print job, planned and rasterized.
#[derive(Debug, Clone)]
pub struct PrintResult {
    /// Which request produced this.
    pub generation: u64,
    /// How many pages went in.
    pub pages: usize,
    /// How many sheets the job consumes.
    pub sheets: usize,
    /// What each planned page came to.
    pub planned: Vec<SheetReport>,
    /// The sheets, or why none were produced.
    pub outcome: Result<(), String>,
}

/// The reader's half of the channel pair.
pub struct PrintQueue {
    to_worker: std::sync::mpsc::Sender<PrintRequest>,
    from_worker: std::sync::mpsc::Receiver<PrintResult>,
    newest: u64,
    answered: Option<u64>,
}

impl PrintQueue {
    /// Start a worker over a copy of `bytes`, where the document came from.
    ///
    /// The thread ends when the queue is dropped and the channel closes.
    pub fn spawn(bytes: Vec<u8>) -> Self {
        let (to_worker, work) = std::sync::mpsc::channel::<PrintRequest>();
        let (finish, from_worker) = std::sync::mpsc::channel::<PrintResult>();
        std::thread::Builder::new()
            .name("sofd-print".to_string())
            .spawn(move || run_worker(bytes, &work, &finish))
            .expect("the print worker thread starts");
        Self {
            to_worker,
            from_worker,
            newest: 0,
            answered: None,
        }
    }

    /// Whether a job is still outstanding.
    pub fn is_busy(&self) -> bool {
        self.answered != Some(self.newest)
    }

    /// Hand the worker a job. Returns the generation it was stamped with.
    pub fn submit(&mut self, request: PrintRequest) -> u64 {
        self.newest = request.generation;
        // A closed worker means the thread is gone; the caller then never sees
        // a result, which leaves the reader waiting rather than wrong.
        let _ = self.to_worker.send(request);
        self.newest
    }

    /// Everything the worker has finished since the last call.
    pub fn drain(&mut self) -> Vec<PrintResult> {
        let mut done = Vec::new();
        while let Ok(result) = self.from_worker.try_recv() {
            self.answered = Some(match self.answered {
                Some(seen) => seen.max(result.generation),
                None => result.generation,
            });
            done.push(result);
        }
        done
    }
}

/// The worker loop: plan and rasterize until the channel closes.
fn run_worker(
    bytes: Vec<u8>,
    work: &std::sync::mpsc::Receiver<PrintRequest>,
    finish: &std::sync::mpsc::Sender<PrintResult>,
) {
    let mut document = None;
    while let Ok(request) = work.recv() {
        let count = request.pages.len();
        if document.is_none() {
            match sofd::Document::open_bytes(&bytes) {
                Ok(doc) => document = Some(doc),
                Err(err) => {
                    let _ = finish.send(PrintResult {
                        generation: request.generation,
                        pages: count,
                        sheets: 0,
                        planned: Vec::new(),
                        outcome: Err(err.to_string()),
                    });
                    continue;
                }
            }
        }
        let doc = document.as_ref().expect("opened just above");
        let (planned, sheets, failure) = print(doc, &request);
        let _ = finish.send(PrintResult {
            generation: request.generation,
            pages: count,
            sheets,
            planned,
            outcome: failure.map_or(Ok(()), Err),
        });
    }
}

/// Plan the job over the selected pages and rasterize every sheet.
/// A job that plans but will not rasterize is not a print job, so the second
/// failure is reported rather than swallowed.
fn print(
    doc: &sofd::Document,
    request: &PrintRequest,
) -> (Vec<SheetReport>, usize, Option<String>) {
    let mut settings = request.settings.clone();
    settings.selection = sofd_print::PageSelection::List(
        request
            .pages
            .iter()
            .map(|index| index + 1)
            .collect::<Vec<_>>(),
    );
    let job = match doc.print_job(&settings) {
        Ok(job) => job,
        Err(err) => return (Vec::new(), 0, Some(err.to_string())),
    };
    match doc.print_render(&settings) {
        Ok(raster) => (describe(&job, &raster), job.sheet_count(), None),
        Err(err) => (Vec::new(), job.sheet_count(), Some(err.to_string())),
    }
}

/// Reduce a planned job and its rasters to what the reader reports.
fn describe(job: &PrintJob, raster: &[sofd_print::RasterPage]) -> Vec<SheetReport> {
    job.pages
        .iter()
        .zip(raster)
        .map(|(planned, page)| SheetReport {
            page_index: planned.page_index,
            sheet: planned.sheet,
            side: planned.side,
            width: page.width,
            height: page.height,
        })
        .collect()
}
