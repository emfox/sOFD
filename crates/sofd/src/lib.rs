//! sofd: the feature facade over the OFD stack. One handle opens a
//! document; rendering, conversion, printing, signing, editing, search and
//! auditing each sit behind their own cargo feature.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod compare;
mod document;
#[cfg(feature = "pdf")]
mod export;

pub use compare::{Comparison, PageChange, PageDiff};
#[cfg(feature = "annot")]
pub use document::Annotation;
pub use document::Document;

// Types that appear in `Document`'s own signatures. Re-exported so a caller
// can name them without taking a direct dependency on the crate that defines
// them, which is the point of a facade: one crate to depend on.
#[cfg(feature = "sign")]
pub use document::Seal;
// The face a device supplies for Chinese labels, so a caller can name it
// without taking a direct dependency on the crate that reads the fonts.
#[cfg(target_os = "android")]
pub use sofd_font::cjk_face;
// What a conversion reports about itself. `Document::to_pdf_with_stats_list`
// returns one, so a caller that reads the counts must be able to name the type
// without depending on the crate behind it.
#[cfg(feature = "convert")]
pub use sofd_convert::{ConversionStats, TextExport};
#[cfg(feature = "render")]
pub use sofd_render::RenderOptions;
// The geometry a shell draws with, so a canvas backend can name it without
// depending on the crate that produced it.
#[cfg(feature = "render")]
pub use sofd_render::{Shape, VectorPage};
#[cfg(feature = "search")]
pub use sofd_search::SearchOptions;
#[cfg(feature = "sign")]
pub use sofd_sign::{SealText, TimestampStatus, VerifyStatus};
