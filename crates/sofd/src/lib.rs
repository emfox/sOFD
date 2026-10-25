//! sofd: the feature facade over the OFD stack. One handle opens a
//! document; rendering, conversion, printing, signing, editing, search and
//! auditing each sit behind their own cargo feature.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

mod compare;
mod document;

pub use compare::{Comparison, PageChange, PageDiff};
#[cfg(feature = "annot")]
pub use document::Annotation;
pub use document::Document;

// Types that appear in `Document`'s own signatures. Re-exported so a caller
// can name them without taking a direct dependency on the crate that defines
// them, which is the point of a facade: one crate to depend on.
#[cfg(feature = "sign")]
pub use document::Seal;
#[cfg(feature = "render")]
pub use sofd_render::RenderOptions;
#[cfg(feature = "search")]
pub use sofd_search::SearchOptions;
#[cfg(feature = "sign")]
pub use sofd_sign::{SealText, TimestampStatus, VerifyStatus};
