//! Resources resolved into a form backends can consume directly.

use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sofd_common::ImageCodec;
use sofd_common::Rect;
use sofd_font::FontId;

/// Font data ready for glyph rasterization by a backend.
#[derive(Clone)]
pub struct ResolvedFont {
    /// Globally unique font id.
    pub id: FontId,
    /// Font family name.
    pub family: String,
    /// Whether the face covers CJK.
    pub is_cjk: bool,
    /// Font binary, either embedded in the document or supplied by a system face.
    pub data: Arc<Vec<u8>>,
    /// Whether this is a substituted system font rather than embedded data.
    pub is_fallback: bool,
}

impl std::fmt::Debug for ResolvedFont {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ResolvedFont")
            .field("id", &self.id)
            .field("family", &self.family)
            .field("is_cjk", &self.is_cjk)
            .field("bytes", &self.data.len())
            .field("is_fallback", &self.is_fallback)
            .finish()
    }
}

/// Reference to an image resource, decoded lazily by the backend.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageRef {
    /// Resource id as it appears in the OFD `ID` attribute.
    pub id: String,
    /// Encoded bytes in the original format.
    #[serde(skip)]
    pub data: Arc<Vec<u8>>,
    /// Intrinsic pixel dimensions.
    pub pixel_size: (u32, u32),
    /// Nominal placement size on the page, in millimetres.
    pub nominal_size: Rect,
    /// Encoding of `data`.
    pub codec: ImageCodec,
}

impl ImageRef {
    /// Build a reference from an id and encoded bytes.
    pub fn new(
        id: impl Into<String>,
        data: Arc<Vec<u8>>,
        pixel_size: (u32, u32),
        codec: ImageCodec,
    ) -> Self {
        Self {
            id: id.into(),
            data,
            pixel_size,
            nominal_size: Rect::default(),
            codec,
        }
    }
}

/// Everything one page needs, already resolved.
#[derive(Debug, Default)]
pub struct ResolvedResources {
    /// Font table indexed by [`sofd_font::FontId`].
    pub fonts: Vec<ResolvedFont>,
    /// Image table keyed by resource id.
    pub images: Vec<ImageRef>,
}
