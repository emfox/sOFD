//! Conversion and file writing off the draw path. A PDF of a long document is
//! written in one go, so export runs on its own thread for the same reason
//! rasterizing does. Unlike rendering, an export is never superseded.

use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};

/// What an export produces.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    /// One PDF holding every selected page.
    Pdf,
    /// One PNG file per selected page.
    Png,
}

/// One unit of work: write `pages` as `format` under `destination`.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportRequest {
    /// Monotonic stamp, so a result can be told from a later request's.
    pub generation: u64,
    /// What to produce.
    pub format: ExportFormat,
    /// The pages to include, in order; a selection, not a span.
    pub pages: Vec<usize>,
    /// Resolution for raster output, ignored by PDF.
    pub dpi: f32,
    /// Where the output goes: the file itself for PDF, a directory for PNG.
    pub destination: PathBuf,
    /// Filename stem for raster output, which writes one file per page.
    pub stem: String,
}

/// A finished export.
pub struct ExportResult {
    /// Which request produced this.
    pub generation: u64,
    /// What was being produced.
    pub format: ExportFormat,
    /// How many pages went in.
    pub pages: usize,
    /// The files written, in page order.
    pub written: Vec<PathBuf>,
    /// The files written, or why none were.
    pub outcome: Result<(), String>,
}

/// The reader's half of the channel pair.
pub struct ExportQueue {
    to_worker: Sender<ExportRequest>,
    from_worker: Receiver<ExportResult>,
    newest: u64,
    answered: Option<u64>,
}

impl ExportQueue {
    /// Start a worker over a copy of `bytes`, where the document came from.
    ///
    /// The thread ends when the queue is dropped and the channel closes.
    pub fn spawn(bytes: Vec<u8>) -> Self {
        let (to_worker, work) = std::sync::mpsc::channel::<ExportRequest>();
        let (finish, from_worker) = std::sync::mpsc::channel::<ExportResult>();
        std::thread::Builder::new()
            .name("sofd-export".to_string())
            .spawn(move || run_worker(bytes, &work, &finish))
            .expect("the export worker thread starts");
        Self {
            to_worker,
            from_worker,
            newest: 0,
            answered: None,
        }
    }

    /// Whether an export is still outstanding.
    pub fn is_busy(&self) -> bool {
        self.answered != Some(self.newest)
    }

    /// Hand the worker a job. Returns the generation it was stamped with.
    pub fn submit(&mut self, request: ExportRequest) -> u64 {
        self.newest = request.generation;
        // A closed worker means the thread is gone; the caller then never sees a
        // result, which leaves the reader waiting rather than wrong.
        let _ = self.to_worker.send(request);
        self.newest
    }

    /// Everything the worker has finished since the last call.
    pub fn drain(&mut self) -> Vec<ExportResult> {
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

/// The worker loop: convert and write until the channel closes.
fn run_worker(bytes: Vec<u8>, work: &Receiver<ExportRequest>, finish: &Sender<ExportResult>) {
    let mut document = None;
    while let Ok(request) = work.recv() {
        let count = request.pages.len();
        if document.is_none() {
            match sofd::Document::open_bytes(&bytes) {
                Ok(doc) => document = Some(doc),
                Err(err) => {
                    let _ = finish.send(ExportResult {
                        generation: request.generation,
                        format: request.format,
                        pages: count,
                        written: Vec::new(),
                        outcome: Err(err.to_string()),
                    });
                    continue;
                }
            }
        }
        let doc = document.as_ref().expect("opened just above");
        let (written, failure) = write(doc, &request);
        let _ = finish.send(ExportResult {
            generation: request.generation,
            format: request.format,
            pages: count,
            written,
            outcome: failure.map_or(Ok(()), Err),
        });
    }
}

/// Produce the requested files. The files written before a failure are returned
/// alongside the error, because they exist on disk and a caller that reports
/// "nothing was written" would be lying about half the output.
fn write(doc: &sofd::Document, request: &ExportRequest) -> (Vec<PathBuf>, Option<String>) {
    match request.format {
        ExportFormat::Pdf => match doc.to_pdf_pages(&request.pages) {
            Ok(bytes) => match std::fs::write(&request.destination, bytes) {
                Ok(()) => (vec![request.destination.clone()], None),
                Err(err) => (
                    Vec::new(),
                    Some(format!("{}: {err}", request.destination.display())),
                ),
            },
            Err(err) => (Vec::new(), Some(err.to_string())),
        },
        ExportFormat::Png => {
            let options = sofd::RenderOptions {
                dpi: f64::from(request.dpi),
                ..Default::default()
            };
            // One page failing does not abandon the pages that converted, so
            // the error is reported and the files written so far still stand.
            let mut written = Vec::with_capacity(request.pages.len());
            for page in &request.pages {
                let encoded = doc.to_png(*page, &options).map_err(|err| err.to_string());
                let bytes = match encoded {
                    Ok(bytes) => bytes,
                    Err(err) => return (written, Some(err)),
                };
                let path = request
                    .destination
                    .join(format!("{}-{page}.png", request.stem));
                if let Err(err) = std::fs::write(&path, bytes) {
                    return (written, Some(format!("{}: {err}", path.display())));
                }
                written.push(path);
            }
            (written, None)
        }
    }
}
