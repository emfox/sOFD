//! The reader's own message ids. They live here, not in `sofd-i18n`,
//! because they name things only a reader shows — a toolbar toggle, a paper
//! size, an export dialog title. The engine catalogue stays engine messages.

use sofd_i18n::{I18n, I18nBuilder, Locale};

/// The reader's English catalogue, compiled in. It is merged into the engine
/// bundle at build time, so a reader key and an engine key resolve the same way.
const EN_US: &str = include_str!("../locales/en-US/reader.ftl");

/// The reader's Chinese catalogue, compiled in.
const ZH_CN: &str = include_str!("../locales/zh-CN/reader.ftl");

/// An [`I18n`] with both engine catalogues and the reader's own merged in.
/// The reader supplies its catalogue here rather than shipping it inside the
/// engine crate, which is what keeps `sofd-i18n` free of reader keys.
pub fn bundle() -> I18n {
    build(I18n::builder())
}

/// The same, pinned to one locale.
pub fn bundle_for(locale: Locale) -> I18n {
    build(I18n::builder().locale(locale))
}

fn build(builder: I18nBuilder) -> I18n {
    builder
        .add_resource(Locale::en_us(), EN_US)
        .add_resource(Locale::zh_cn(), ZH_CN)
        .build()
}

/// What the reader says. Each variant maps to one Fluent key in this crate's
/// own `locales/` catalogue, which the shell loads as a resource.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReaderMessage {
    /// Open a document
    Open,
    /// Close the open document
    Close,
    /// Shown where a file can be dropped
    DropHint,
    /// Shown where a drop target is not the way in
    OpenHint,
    /// The document description line
    Info,
    /// Page N of M
    PageOf,
    /// Zoom level
    Zoom,
    /// Fit to width
    FitWidth,
    /// Fit to page
    FitPage,
    /// Single page layout
    LayoutSingle,
    /// Facing pages layout
    LayoutDouble,
    /// Continuous scroll layout
    LayoutContinuous,
    /// Page has no visible content
    BlankPage,
    /// The page is missing glyphs the face did not carry
    ShortGlyphs,
    /// The page left out text runs whose font could not be loaded
    ShortFonts,
    /// The page left out images that would not decode
    ShortImages,
    /// The canvas says a page failed; the status bar says why
    ErrorLabel,
    /// Label on the toolbar toggle that opens the search panel
    SearchTitle,
    /// Heading inside the search panel, distinct from the toggle
    SearchPanelTitle,
    /// No match found
    NoMatch,
    /// Menu heading for the export actions
    Export,
    /// Export the pages on screen as one PDF
    ExportPdf,
    /// Export the pages on screen as one PNG file each
    ExportPng,
    /// A PDF export finished, with its page count and path
    ExportPdfDone,
    /// A PNG export finished, with its file count and path
    ExportPngDone,
    /// Title of the platform dialog choosing where the PDF goes
    ExportPdfTitle,
    /// Title of the platform dialog choosing where the PNG files go
    ExportPngTitle,
    /// Print the pages on screen
    Print,
    /// A print job finished, with its page, sheet and paper count
    PrintDone,
    /// A3 paper
    PaperA3,
    /// A4 paper
    PaperA4,
    /// A5 paper
    PaperA5,
    /// B5 paper
    PaperB5,
    /// US Letter paper
    PaperLetter,
    /// US Legal paper
    PaperLegal,
    /// The paper the document itself declares
    PaperDocument,
    /// Label on the button that opens the appearance settings
    Settings,
    /// Label on the far end of the header bar, holding the occasional actions
    MainMenu,
    /// What the main menu button opens
    MainMenuHint,
    /// Menu entry naming the keyboard shortcuts
    KeyboardShortcuts,
    /// Menu entry naming the build
    About,
    /// Heading over the locales in the main menu
    Language,
    /// The view switcher's own name, spoken rather than drawn
    Views,
    /// Label on a workspace panel, and on its toggle in the view switcher
    PanelToggle,
    /// Toggle for the search results, named by what it shows
    SearchToggle,
    /// Toggle for the signature report
    SignaturesToggle,
    /// Toggle for the annotation list
    AnnotationsToggle,
    /// Heading over the theme choices
    Theme,
    /// Theme that follows the desktop
    ThemeSystem,
    /// Light theme
    ThemeLight,
    /// Dark theme
    ThemeDark,
    /// Heading over the text-size choices
    TextSize,
    /// The base text size
    TextSizeNormal,
    /// One step larger
    TextSizeLarge,
    /// Two steps larger
    TextSizeLarger,
    /// The high-contrast switch
    HighContrast,
    /// What the high-contrast switch does
    HighContrastHint,
    /// Previous page, as a tooltip
    PrevPage,
    /// Next page, as a tooltip
    NextPage,
    /// Previous search hit, as a tooltip
    PrevHit,
    /// Next search hit, as a tooltip
    NextHit,
    /// Previous annotation, as a tooltip
    PrevAnnotation,
    /// Next annotation, as a tooltip
    NextAnnotation,
    /// Closing the document, as a tooltip
    CloseHint,
    /// A signature that holds
    VerdictValid,
    /// A signature that does not hold
    VerdictTampered,
    /// A signature nothing can confirm
    VerdictUnverified,
    /// A sound proof whose signer is not trusted
    VerdictUntrusted,
}

impl ReaderMessage {
    /// The Fluent key for this message.
    pub fn key(self) -> &'static str {
        match self {
            Self::Open => "reader-open",
            Self::Close => "reader-close",
            Self::DropHint => "reader-drop-hint",
            Self::OpenHint => "reader-open-hint",
            Self::Info => "reader-info",
            Self::PageOf => "reader-page-of",
            Self::Zoom => "reader-zoom",
            Self::FitWidth => "reader-fit-width",
            Self::FitPage => "reader-fit-page",
            Self::LayoutSingle => "reader-layout-single",
            Self::LayoutDouble => "reader-layout-double",
            Self::LayoutContinuous => "reader-layout-continuous",
            Self::BlankPage => "reader-blank-page",
            Self::ShortGlyphs => "reader-short-glyphs",
            Self::ShortFonts => "reader-short-fonts",
            Self::ShortImages => "reader-short-images",
            Self::ErrorLabel => "reader-error-label",
            Self::SearchTitle => "reader-search-title",
            Self::SearchPanelTitle => "reader-search-panel-title",
            Self::NoMatch => "reader-no-match",
            Self::Export => "reader-export",
            Self::ExportPdf => "reader-export-pdf",
            Self::ExportPng => "reader-export-png",
            Self::ExportPdfDone => "reader-export-pdf-done",
            Self::ExportPngDone => "reader-export-png-done",
            Self::ExportPdfTitle => "reader-export-pdf-title",
            Self::ExportPngTitle => "reader-export-png-title",
            Self::Print => "reader-print",
            Self::PrintDone => "reader-print-done",
            Self::PaperA3 => "reader-paper-a3",
            Self::PaperA4 => "reader-paper-a4",
            Self::PaperA5 => "reader-paper-a5",
            Self::PaperB5 => "reader-paper-b5",
            Self::PaperLetter => "reader-paper-letter",
            Self::PaperLegal => "reader-paper-legal",
            Self::PaperDocument => "reader-paper-document",
            Self::Settings => "reader-settings",
            Self::MainMenu => "reader-main-menu",
            Self::MainMenuHint => "reader-main-menu-hint",
            Self::KeyboardShortcuts => "reader-keyboard-shortcuts",
            Self::About => "reader-about",
            Self::Views => "reader-views",
            Self::PanelToggle => "reader-panel-toggle",
            Self::SearchToggle => "reader-search-title",
            Self::SignaturesToggle => "reader-panel-toggle",
            Self::AnnotationsToggle => "reader-panel-toggle",
            Self::Language => "reader-language",
            Self::Theme => "reader-theme",
            Self::ThemeSystem => "reader-theme-system",
            Self::ThemeLight => "reader-theme-light",
            Self::ThemeDark => "reader-theme-dark",
            Self::TextSize => "reader-text-size",
            Self::TextSizeNormal => "reader-text-size-normal",
            Self::TextSizeLarge => "reader-text-size-large",
            Self::TextSizeLarger => "reader-text-size-larger",
            Self::HighContrast => "reader-high-contrast",
            Self::HighContrastHint => "reader-high-contrast-hint",
            Self::PrevPage => "reader-prev-page",
            Self::NextPage => "reader-next-page",
            Self::PrevHit => "reader-prev-hit",
            Self::NextHit => "reader-next-hit",
            Self::PrevAnnotation => "reader-prev-annotation",
            Self::NextAnnotation => "reader-next-annotation",
            Self::CloseHint => "reader-close-hint",
            Self::VerdictValid => "reader-verdict-valid",
            Self::VerdictTampered => "reader-verdict-tampered",
            Self::VerdictUnverified => "reader-verdict-unverified",
            Self::VerdictUntrusted => "reader-verdict-untrusted",
        }
    }

    /// Every message, in the same order as the catalogue lists them.
    pub const ALL: &'static [Self] = &[
        Self::Open,
        Self::Close,
        Self::DropHint,
        Self::OpenHint,
        Self::Info,
        Self::PageOf,
        Self::Zoom,
        Self::FitWidth,
        Self::FitPage,
        Self::LayoutSingle,
        Self::LayoutDouble,
        Self::LayoutContinuous,
        Self::BlankPage,
        Self::ShortGlyphs,
        Self::ShortFonts,
        Self::ShortImages,
        Self::ErrorLabel,
        Self::SearchTitle,
        Self::SearchPanelTitle,
        Self::NoMatch,
        Self::Export,
        Self::ExportPdf,
        Self::ExportPng,
        Self::ExportPdfDone,
        Self::ExportPngDone,
        Self::ExportPdfTitle,
        Self::ExportPngTitle,
        Self::Print,
        Self::PrintDone,
        Self::PaperA3,
        Self::PaperA4,
        Self::PaperA5,
        Self::PaperB5,
        Self::PaperLetter,
        Self::PaperLegal,
        Self::PaperDocument,
        Self::Settings,
        Self::MainMenu,
        Self::MainMenuHint,
        Self::KeyboardShortcuts,
        Self::About,
        Self::Views,
        Self::PanelToggle,
        Self::SearchToggle,
        Self::SignaturesToggle,
        Self::AnnotationsToggle,
        Self::Language,
        Self::Theme,
        Self::ThemeSystem,
        Self::ThemeLight,
        Self::ThemeDark,
        Self::TextSize,
        Self::TextSizeNormal,
        Self::TextSizeLarge,
        Self::TextSizeLarger,
        Self::HighContrast,
        Self::HighContrastHint,
        Self::PrevPage,
        Self::NextPage,
        Self::PrevHit,
        Self::NextHit,
        Self::PrevAnnotation,
        Self::NextAnnotation,
        Self::CloseHint,
        Self::VerdictValid,
        Self::VerdictTampered,
        Self::VerdictUnverified,
        Self::VerdictUntrusted,
    ];
}
