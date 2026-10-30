//! Entry classification.
//! OFD archives follow a fixed directory convention, so an entry's role can be
//! derived from its path instead of being recorded in a manifest.

use std::fmt;

/// What an archive entry holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum EntryKind {
    /// Top-level `OFD.xml`, the entry point.
    EntryPoint,
    /// A per-document `Document.xml`.
    Document,
    /// A page description under `Pages/`.
    Page,
    /// A template under `Tpls/`.
    Template,
    /// A resource container under `Res/`.
    Resource,
    /// A font program.
    Font,
    /// An image.
    Image,
    /// A signature description or value.
    Signature,
    /// Anything else, kept so unknown extensions survive a round trip.
    Other,
}

impl EntryKind {
    /// Classify by path. Every path has a kind: an entry the schema does not
    /// name is `Other`, which is still a kind, so the answer is not optional.
    pub fn from_path(path: &str) -> Self {
        let name = path.rsplit('/').next().unwrap_or(path);
        let lower = name.to_ascii_lowercase();

        if path == crate::ENTRY_POINT {
            return Self::EntryPoint;
        }
        if name == "Document.xml" {
            return Self::Document;
        }
        if name == "Signatures.xml" || name == "Signature.xml" {
            return Self::Signature;
        }

        // Extension wins over directory, since a document may store an image
        // anywhere and the bytes are what matter for decoding.
        if matches!(
            extension(&lower).as_deref(),
            Some("ttf" | "otf" | "ttc" | "woff" | "woff2")
        ) {
            return Self::Font;
        }
        if matches!(
            extension(&lower).as_deref(),
            Some("png" | "jpg" | "jpeg" | "jbig2" | "jb2" | "bmp" | "tif" | "tiff" | "gif")
        ) {
            return Self::Image;
        }

        if path.contains("/Pages/") || path.starts_with("Pages/") {
            return Self::Page;
        }
        if path.contains("/Tpls/") || path.starts_with("Tpls/") {
            return Self::Template;
        }
        if path.contains("/Res/") || path.starts_with("Res/") {
            return Self::Resource;
        }

        Self::Other
    }

    /// Whether the entry is binary rather than XML text.
    pub fn is_binary(self) -> bool {
        matches!(self, Self::Font | Self::Image)
    }

    /// Whether the entry holds XML.
    pub fn is_xml(self) -> bool {
        !self.is_binary() && !matches!(self, Self::Other)
    }
}

impl fmt::Display for EntryKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let name = match self {
            Self::EntryPoint => "entry-point",
            Self::Document => "document",
            Self::Page => "page",
            Self::Template => "template",
            Self::Resource => "resource",
            Self::Font => "font",
            Self::Image => "image",
            Self::Signature => "signature",
            Self::Other => "other",
        };
        f.write_str(name)
    }
}

/// Description of one entry, useful for listings and diagnostics.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryInfo {
    /// Archive path.
    pub path: String,
    /// Derived role.
    pub kind: EntryKind,
    /// Decompressed size in bytes.
    pub size: u64,
}

impl EntryInfo {
    /// Build a description.
    pub fn new(path: impl Into<String>, size: u64) -> Self {
        let path = path.into();
        let kind = EntryKind::from_path(&path);
        Self { path, kind, size }
    }
}

impl fmt::Display for EntryInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} [{}] {} bytes", self.path, self.kind, self.size)
    }
}

/// Lower-cased extension without the dot.
fn extension(name: &str) -> Option<String> {
    let (_, ext) = name.rsplit_once('.')?;
    Some(ext.to_ascii_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entry_point_is_recognized() {
        assert_eq!(EntryKind::from_path("OFD.xml"), EntryKind::EntryPoint);
    }

    #[test]
    fn document_and_page_paths_are_recognized() {
        assert_eq!(
            EntryKind::from_path("Doc_0/Document.xml"),
            EntryKind::Document
        );
        assert_eq!(
            EntryKind::from_path("Doc_0/Pages/Page_0/Content.xml"),
            EntryKind::Page
        );
    }

    #[test]
    fn templates_are_distinguished_from_pages() {
        assert_eq!(
            EntryKind::from_path("Doc_0/Tpls/Tpl_0/Content.xml"),
            EntryKind::Template
        );
    }

    #[test]
    fn fonts_and_images_are_detected_by_extension() {
        assert_eq!(
            EntryKind::from_path("Doc_0/Res/Font_0.ttf"),
            EntryKind::Font
        );
        assert_eq!(
            EntryKind::from_path("Doc_0/Res/Image_1.PNG"),
            EntryKind::Image
        );
    }

    #[test]
    fn an_image_outside_res_is_still_an_image() {
        // Extension wins over directory so a misplaced asset is still decodable.
        assert_eq!(
            EntryKind::from_path("Doc_0/Pages/img.png"),
            EntryKind::Image
        );
    }

    #[test]
    fn signatures_are_recognized() {
        assert_eq!(
            EntryKind::from_path("Doc_0/Signs/Signatures.xml"),
            EntryKind::Signature
        );
    }

    #[test]
    fn unknown_paths_fall_back_to_other() {
        assert_eq!(
            EntryKind::from_path("Doc_0/Misc/notes.bin"),
            EntryKind::Other
        );
    }

    #[test]
    fn kind_reports_binary_and_xml_correctly() {
        assert!(EntryKind::Font.is_binary());
        assert!(EntryKind::Image.is_binary());
        assert!(!EntryKind::Font.is_xml());
        assert!(EntryKind::Document.is_xml());
        assert!(!EntryKind::Other.is_xml());
    }

    #[test]
    fn entry_info_derives_its_kind() {
        let info = EntryInfo::new("Doc_0/Res/Font_0.ttf", 1234);
        assert_eq!(info.kind, EntryKind::Font);
        assert_eq!(info.size, 1234);
        assert!(info.to_string().contains("font"));
    }

    #[test]
    fn paths_without_an_extension_do_not_panic() {
        assert_eq!(EntryKind::from_path("Doc_0/Res/font"), EntryKind::Resource);
    }
}
