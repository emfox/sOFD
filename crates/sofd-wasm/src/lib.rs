//! Browser bindings: open an OFD in a page and pull data out of it.
//! The data shapes come from `sofd-bindings`, so this module only crosses the
//! JS boundary and the shapes stay testable on the host.

#![warn(missing_docs)]

pub use sofd_bindings as shapes;

#[cfg(target_arch = "wasm32")]
use std::sync::Once;

use serde::Serialize;
use wasm_bindgen::prelude::*;

use sofd::{Document, RenderOptions};

use crate::shapes::DocumentInfo;

#[cfg(target_arch = "wasm32")]
static INIT: Once = Once::new();

/// One-time browser setup: panics to the console, logs to `console.log`.
fn init_once() {
    #[cfg(target_arch = "wasm32")]
    INIT.call_once(|| {
        console_error_panic_hook::set_once();
        let _ = tracing_wasm::set_as_global_default();
    });
}

/// An open OFD document.
/// One handle per document; pages are rendered on demand.
#[wasm_bindgen]
pub struct OfdDocument {
    inner: Document,
}

#[wasm_bindgen]
impl OfdDocument {
    /// Open from the raw bytes of an `.ofd` file.
    pub fn open(bytes: &[u8]) -> Result<OfdDocument, JsError> {
        init_once();
        let inner = Document::open_bytes(bytes).map_err(js_error)?;
        Ok(Self { inner })
    }

    /// How many pages the document declares.
    #[wasm_bindgen(getter)]
    pub fn page_count(&self) -> usize {
        self.inner.page_count()
    }

    /// Declaration-level facts, as a plain object for the page to display.
    pub fn info(&mut self) -> Result<JsValue, JsError> {
        to_js(&DocumentInfo::of(&mut self.inner))
    }

    /// Rasterize one page and hand back PNG bytes.
    pub fn render_png(&self, page: usize, dpi: f64) -> Result<Vec<u8>, JsError> {
        let options = RenderOptions {
            dpi,
            ..Default::default()
        };
        self.inner.to_png(page, &options).map_err(js_error)
    }

    /// One page as an SVG document string, ready to drop into the DOM.
    pub fn to_svg(&self, page: usize) -> Result<String, JsError> {
        self.inner.to_svg(page).map_err(js_error)
    }

    /// Full-text search over every page.
    pub fn search(&self, needle: &str) -> Result<JsValue, JsError> {
        let hits = shapes::search_hits(&self.inner, needle, shapes::DEFAULT_SEARCH);
        to_js(&hits)
    }

    /// Verify every signature; each entry says where it is and whether it holds.
    pub fn verify(&mut self) -> Result<JsValue, JsError> {
        let statuses = shapes::signature_statuses(&mut self.inner).map_err(js_error)?;
        to_js(&statuses)
    }
}

/// Render a structured error as a JS `Error` carrying the engine's message.
fn js_error(err: sofd_error::OfdError) -> JsError {
    JsError::new(&err.to_string())
}

/// Serialize a data shape into a plain JS value.
fn to_js<T: Serialize>(value: &T) -> Result<JsValue, JsError> {
    serde_wasm_bindgen::to_value(value).map_err(|err| JsError::new(&err.to_string()))
}
