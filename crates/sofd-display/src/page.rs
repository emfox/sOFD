//! Pages, viewports and render parameters.

use serde::{Deserialize, Serialize};
use sofd_common::Rect;

/// The region of a page to render.
///
/// Tiled rendering and on-demand rendering of very large pages both rely on this.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Viewport {
    /// Visible region in page coordinates, in millimetres.
    pub area: Rect,
    /// Output resolution in dots per inch.
    pub dpi: f64,
    /// Additional zoom on top of `dpi`, for continuous zoom controls.
    pub zoom: f64,
}

impl Viewport {
    /// A viewport covering the whole page.
    pub fn whole_page(page_box: Rect, dpi: f64) -> Self {
        Self {
            area: page_box,
            dpi,
            zoom: 1.0,
        }
    }

    /// Resolution after zoom is applied.
    pub fn effective_dpi(&self) -> f64 {
        self.dpi * self.zoom
    }

    /// Output size in pixels.
    pub fn output_size(&self) -> (u32, u32) {
        self.area.size_px(self.effective_dpi())
    }

    /// Whether the viewport overlaps the page at all.
    pub fn intersects_page(&self, page_box: Rect) -> bool {
        self.area.intersects(page_box)
    }
}
