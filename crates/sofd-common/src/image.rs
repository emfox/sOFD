//! Image encodings a document may embed.
//!
//! The encoding is a fact about the bytes, not about how they are shown, so
//! the enum lives below the display list: the parser names it, the decoder
//! reads it, and every backend takes the decoded pixels.

use serde::{Deserialize, Serialize};

/// Supported image encodings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ImageCodec {
    /// JPEG.
    Jpeg,
    /// PNG.
    Png,
    /// TIFF.
    Tiff,
    /// BMP.
    Bmp,
    /// GIF, which the standard's own test documents embed.
    Gif,
    /// JBIG2, common for bi-level scanned pages where it saves a lot of space.
    Jbig2,
    /// Uncompressed bitmap.
    Raw,
}

impl ImageCodec {
    /// Guess the encoding from a file name, the way the package does when it
    /// classifies an entry. Unknown names read as `Raw`, which no decoder
    /// claims, so a wrong guess fails loudly instead of painting noise.
    pub fn from_file_name(name: &str) -> Self {
        let lower = name.to_ascii_lowercase();
        let ext = lower.rsplit('.').next().unwrap_or_default();
        match ext {
            "jpg" | "jpeg" => Self::Jpeg,
            "png" => Self::Png,
            "tif" | "tiff" => Self::Tiff,
            "bmp" => Self::Bmp,
            "gif" => Self::Gif,
            "jbig2" | "jb2" => Self::Jbig2,
            _ => Self::Raw,
        }
    }

    /// Whether a decoder exists for this encoding.
    pub fn is_decodable(self) -> bool {
        !matches!(self, Self::Raw)
    }
}

#[cfg(test)]
mod tests {
    use super::ImageCodec;

    #[test]
    fn a_file_name_maps_to_its_codec() {
        assert_eq!(ImageCodec::from_file_name("photo.jpeg"), ImageCodec::Jpeg);
        assert_eq!(ImageCodec::from_file_name("seal.JB2"), ImageCodec::Jbig2);
        assert_eq!(ImageCodec::from_file_name("data"), ImageCodec::Raw);
    }

    #[test]
    fn raw_has_no_decoder() {
        assert!(ImageCodec::Png.is_decodable());
        assert!(!ImageCodec::Raw.is_decodable());
    }
}
