//! Render backends.
//! Every backend consumes a [`sofd_display::PageDisplay`] and translates
//! the [`sofd_display::DrawOp`] stream into its own drawing model.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod raster;

pub use raster::{RasterOutput, RenderOptions, render_page};

// Subsetting is not this crate's to do: `sofd` cuts each embedded face
// to the glyphs the display list names, and neither exporter carries a font;
// they outline every glyph into a path.

// The cache is the caller's, not this crate's: a raster is returned and the
// reader decides what to keep. `sofd-reader` keeps one per page, resolution
// and window, and `docs/Performance.md` carries what a scroll costs.
