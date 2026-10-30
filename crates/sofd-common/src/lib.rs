//! Shared foundational types.
//! Layer L0: depends on no other crate, holds only primitives and constants.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod base64;
pub mod image;
pub mod time;
pub mod units;
pub mod version;

pub use image::ImageCodec;
pub use time::{parse_compact_utc, parse_utc};
pub use units::{Millimeter, Point, Rect, Unit};
pub use version::{OfdVersion, SpecVersion};

/// Conventional ZIP container entry point.
pub const OFD_ENTRY: &str = "OFD.xml";

/// Fixed name of a page content stream.
pub const CONTENT_XML: &str = "Content.xml";

/// Fixed name of the document structure file.
pub const DOCUMENT_XML: &str = "Document.xml";

/// Fixed name of the document resource file.
pub const DOCUMENT_RES_XML: &str = "DocumentRes.xml";

/// Fixed name of the public resource file.
pub const PUBLIC_RES_XML: &str = "PublicRes.xml";

/// Fixed name of the resource file a page carries beside its content.
pub const PAGE_RES_XML: &str = "PageRes.xml";

/// Fixed name of the resource file a page area outside `Document.xml` carries.
pub const FILE_RES_XML: &str = "FileRes.xml";

/// Fixed name of the signature list file.
pub const SIGNATURES_XML: &str = "Signatures.xml";

/// Fixed name of the annotation list file.
pub const ANNOTATIONS_XML: &str = "Annotations.xml";
