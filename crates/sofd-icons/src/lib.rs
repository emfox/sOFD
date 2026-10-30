//! The symbolic icon set, as the desktop's own drawing of it.
//!
//! Every glyph is the Adwaita symbolic icon of the same name, stored as that
//! file's `d` attributes and nothing else. A shell decides how to fill them, so
//! two shells draw the same shapes rather than each keeping a copy that drifts.
//! Source: `adwaita-icon-theme` 44.0, licence CC BY-SA 3.0 US, see NOTICE.md.

mod data;
pub mod path;

use std::sync::OnceLock;

pub use path::{ParseError, Segment, Subpath};

use data::*;

/// The square every glyph is drawn on, before a reader's text scale.
pub const SIZE: f32 = 16.0;

/// One of the window's icons. Each is the theme's own drawing of the same
/// idea, so a reader who knows the theme already knows these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Icon {
    /// Open a file.
    Open,
    /// Close the open document.
    Close,
    /// Search the document.
    Search,
    /// The signature report.
    Signatures,
    /// The annotation list.
    Annotations,
    /// The main menu: three bars, as the theme draws it.
    Menu,
    /// The previous page or entry.
    Previous,
    /// The next page or entry.
    Next,
    /// One page at a time.
    SinglePage,
    /// Two pages facing.
    FacingPages,
    /// Pages one after another.
    Continuous,
    /// Fit the page's width to the window.
    FitWidth,
    /// Fit the whole page to the window.
    FitPage,
    /// Print what is on screen.
    Print,
    /// Write a PDF.
    ExportPdf,
    /// Write one PNG per page.
    ExportPng,
    /// A signature that holds.
    Valid,
    /// A signature that does not.
    Tampered,
    /// A signature nothing can confirm.
    Unverified,
    /// A signature whose signer this reader cannot vouch for.
    Untrusted,
}

impl Icon {
    /// Every icon, so a test can walk the set.
    pub const ALL: [Self; 20] = [
        Self::Open,
        Self::Close,
        Self::Search,
        Self::Signatures,
        Self::Annotations,
        Self::Menu,
        Self::Previous,
        Self::Next,
        Self::SinglePage,
        Self::FacingPages,
        Self::Continuous,
        Self::FitWidth,
        Self::FitPage,
        Self::Print,
        Self::ExportPdf,
        Self::ExportPng,
        Self::Valid,
        Self::Tampered,
        Self::Unverified,
        Self::Untrusted,
    ];

    /// The name of the theme icon this glyph is. It is also the name of the
    /// file the path data was taken from, which is how a reviewer checks it.
    pub fn theme_name(self) -> &'static str {
        match self {
            Self::Open => "document-open-symbolic",
            Self::Close => "window-close-symbolic",
            Self::Search => "edit-find-symbolic",
            Self::Signatures => "application-certificate-symbolic",
            Self::Annotations => "view-list-symbolic",
            Self::Menu => "open-menu-symbolic",
            Self::Previous => "go-previous-symbolic",
            Self::Next => "go-next-symbolic",
            Self::SinglePage => "view-paged-symbolic",
            Self::FacingPages => "view-dual-symbolic",
            Self::Continuous => "view-continuous-symbolic",
            Self::FitWidth => "zoom-fit-best-symbolic",
            Self::FitPage => "zoom-in-symbolic",
            Self::Print => "document-print-symbolic",
            Self::ExportPdf => "document-save-symbolic",
            Self::ExportPng => "image-x-generic-symbolic",
            Self::Valid => "emblem-ok-symbolic",
            Self::Tampered => "dialog-error-symbolic",
            Self::Unverified => "dialog-question-symbolic",
            Self::Untrusted => "dialog-warning-symbolic",
        }
    }

    /// The themed paths this one is drawn from, in the order the theme's file
    /// lists them. Each is one `<path>` element's `d`, and nothing else.
    pub fn paths(self) -> &'static [&'static str] {
        match self {
            Self::Open => &[OPEN_0, OPEN_1],
            Self::Close => &[CLOSE_0],
            Self::Search => &[SEARCH_0],
            Self::Signatures => &[CERTIFICATE_0],
            Self::Annotations => &[LIST_0],
            Self::Menu => &[MENU_0, MENU_1, MENU_2],
            Self::Previous => &[PREVIOUS_0],
            Self::Next => &[NEXT_0],
            Self::SinglePage => &[PAGED_0, PAGED_1, PAGED_2],
            Self::FacingPages => &[DUAL_0, DUAL_1],
            Self::Continuous => &[CONTINUOUS_0, CONTINUOUS_1],
            Self::FitWidth => &[FIT_WIDTH_0],
            Self::FitPage => &[FIT_PAGE_0],
            Self::Print => &[PRINT_0],
            Self::ExportPdf => &[SAVE_0],
            Self::ExportPng => &[IMAGE_0, IMAGE_1],
            Self::Valid => &[OK_0],
            Self::Tampered => &[ERROR_0],
            Self::Unverified => &[QUESTION_0],
            Self::Untrusted => &[WARNING_0],
        }
    }

    /// The drawing as it is stored, read once and kept for the process.
    pub fn drawing(self) -> &'static Drawing {
        static CACHE: OnceLock<Vec<Drawing>> = OnceLock::new();
        let all = CACHE.get_or_init(|| Icon::ALL.iter().map(|icon| icon.read()).collect());
        &all[Icon::ALL
            .iter()
            .position(|icon| *icon == self)
            .expect("every icon is in ALL")]
    }

    /// Read this glyph's paths, and note the ones the theme draws faded: the
    /// parts that only hint at a second page behind the first.
    fn read(self) -> Drawing {
        let mut drawing = Drawing::default();
        for text in self.paths() {
            let shade = if text.contains(FADED) { 0.349 } else { 1.0 };
            let subpaths = path::parse(text)
                .unwrap_or_else(|e| panic!("{} does not read: {e:?}", self.theme_name()));
            drawing
                .parts
                .extend(subpaths.into_iter().map(|s| (s, shade)));
        }
        drawing
    }
}

/// One glyph's paths, each with the strength the theme draws it at.
#[derive(Debug, Default)]
pub struct Drawing {
    /// Every subpath, with the share of the ink it takes.
    pub parts: Vec<(Subpath, f32)>,
}
