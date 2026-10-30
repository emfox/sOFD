//! Native desktop reader for OFD documents. The engine is linked directly, so
//! a document is opened, rendered and searched on the machine that holds it;
//! nothing is uploaded anywhere.

#![warn(missing_docs)]

pub mod action;
pub mod app;
// The entry point a device calls. It is declared here and not in `main.rs`
// because Android loads the cdylib, and that library is built from this crate
// root: the same `mod` under the binary would compile and then export nothing.
#[cfg(target_os = "android")]
pub mod android;
pub mod export_worker;
pub mod fonts;
pub mod icons;
pub mod message;
pub mod print_worker;
pub mod render_worker;
pub mod shortcuts;
pub mod theme;
pub mod typography;
pub mod viewer;
pub mod window;

pub use action::Action;
pub use app::{
    App, AppError, Canvas, ExportReport, Fit, Hit, PrintReport, SignatureReport, TextSize,
    ThemeChoice, Verdict,
};
pub use export_worker::{ExportFormat, ExportQueue, ExportRequest, ExportResult};
pub use message::ReaderMessage;
pub use print_worker::{PrintQueue, PrintRequest, PrintResult, SheetReport};
pub use render_worker::{RenderQueue, RenderRequest, RenderResult};
pub use viewer::{DroppedFile, PageCache, RenderedPage, Rgba, Short, looks_blank};
