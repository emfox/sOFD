//! The browser entry point: the binary built for wasm32, around the same frame
//! `main` drives. `wasm-bindgen` reads the binary, which is why this sits beside
//! `main` rather than in the library.

// Through `eframe` rather than directly: it pins the version the glue is
// generated with, so the two can never disagree.
use eframe::wasm_bindgen::{self, JsValue, prelude::*};
use sofd_egui::App;

use crate::Reader;

/// The handle a page holds on to: it starts the reader and owns its lifetime.
#[wasm_bindgen]
pub struct ReaderHandle {
    runner: eframe::WebRunner,
}

#[wasm_bindgen]
impl ReaderHandle {
    /// A runner with no canvas behind it yet.
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        eframe::WebLogger::init(log::LevelFilter::Debug).ok();
        Self {
            runner: eframe::WebRunner::new(),
        }
    }

    /// Start drawing on `canvas`, once the page has one. The promise rejects
    /// when the renderer cannot be created, so a page can say so.
    #[wasm_bindgen]
    pub async fn start(&self, canvas: web_sys::HtmlCanvasElement) -> Result<(), JsValue> {
        self.runner
            .start(
                canvas,
                eframe::WebOptions::default(),
                Box::new(|cc| {
                    sofd_egui::fonts::install_cjk_fallback(&cc.egui_ctx);
                    Ok(Box::new(Reader::with_app(App::new())))
                }),
            )
            .await
    }

    /// Give the GPU resources back when the page navigates away.
    #[wasm_bindgen]
    pub fn destroy(&self) {
        self.runner.destroy();
    }
}

impl Default for ReaderHandle {
    fn default() -> Self {
        Self::new()
    }
}
