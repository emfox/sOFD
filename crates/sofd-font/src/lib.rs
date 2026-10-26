//! Font management and metrics: embedded fonts, system-resolved names, bare CID
//! indices and the reverse glyph-to-Unicode lookup. Text is not shaped here, since
//! a document states its glyphs through `CGTransform`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod cff;
pub mod config;
pub mod face;
mod guard;
// Read on a device, and under test so the scan can be checked without one.
#[cfg(any(target_os = "android", test))]
mod host;
pub mod manager;
pub mod metrics;
pub mod registry;
mod sfnt;
pub mod subset;

pub use config::{FamilyList, FontConfig, default_cjk_families};
pub use face::FontFace;
#[cfg(target_os = "android")]
pub use host::cjk_face;
pub use manager::FontManager;
pub use metrics::{FontMetrics, GlyphOutline};
pub use registry::{FontId, FontRegistry, FontSource, FontTraits, Unresolved};
pub use subset::subset;
