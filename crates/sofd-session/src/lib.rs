//! The state one open document is read in: which page is shown, at what zoom,
//! what the last search found, and the queues that export and print it.
//!
//! Nothing here knows a window. A shell — this project's `egui` reader, a Tauri
//! window, whatever comes next — drives this and draws what it reports, so two
//! shells share the decisions and differ only in how they paint.

#![warn(missing_docs)]

pub mod action;
pub mod app;
pub mod export_worker;
pub mod message;
pub mod print_worker;
pub mod render_worker;
pub mod typography;
pub mod viewer;

pub use action::Action;
pub use app::{
    App, AppError, Canvas, ExportReport, Fit, Hit, PrintReport, SignatureReport, TextSize,
    ThemeChoice, Verdict,
};
pub use export_worker::{ExportFormat, ExportQueue, ExportRequest, ExportResult};
pub use message::ReaderMessage;
pub use print_worker::{PrintQueue, PrintRequest, PrintResult, SheetReport};
pub use render_worker::{RenderQueue, RenderRequest, RenderResult};
pub use typography::{BASE_POINTS, Style};
pub use viewer::{DroppedFile, PageCache, RenderedPage, Rgba, Short, looks_blank};
