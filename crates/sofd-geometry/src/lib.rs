//! Geometry and transforms.
//! Backend-independent [`Path`] and [`Affine`], written out here rather than
//! re-exported: an arc becomes cubics exactly once, so the raster, the SVG and
//! the PDF backend cannot round the same curve three different ways.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod affine;
pub mod arc;
pub mod path;

pub use affine::Affine;
pub use arc::arc_cubics;
pub use path::{Cubic, Path, PathBuilder, PathSegment};
