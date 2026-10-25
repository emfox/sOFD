//! Message id constants.
//! Kebab-case English ids mapping one to one onto `.ftl` keys; never a sentence,
//! since rewording would then touch every catalogue.

use std::fmt;

/// A localizable message identifier
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MessageId {
    // Application level
    /// Application name
    AppName,
    /// Ready
    AppReady,
    /// Loading, with a percentage
    AppLoading,
    /// Operation cancelled
    AppCancelled,
    /// Generic confirm
    CommonConfirm,
    /// Generic cancel
    CommonCancel,
    /// Generic save
    CommonSave,
    /// Generic close
    CommonClose,
    /// Generic retry
    CommonRetry,
    /// Generic delete
    CommonDelete,

    /// Name of this language, written in itself
    LocaleNameEn,
    /// Name of the Chinese locale, written in itself
    LocaleNameZh,

    // Conversion module
    /// Conversion started
    ConvertStart,
    /// Conversion progress, completed of total
    ConvertProgress,
    /// Conversion finished
    ConvertDone,
    /// Conversion failed, with page and reason
    ConvertFailed,
    /// Output format
    ConvertOutputFormat,
    /// Page range
    ConvertPageRange,
    /// Output resolution in DPI
    ConvertDpi,
    /// Keep bookmarks and links
    ConvertKeepBookmarks,
    /// Keep seals as images
    ConvertKeepSignatures,

    // Signature module
    /// Signature is valid
    SignValid,
    /// Signature is invalid
    SignInvalid,
    /// Signature could not be verified
    SignUnverified,
    /// Signer certificate does not chain to a trusted root
    SignUntrusted,
    /// Certificate has expired
    SignExpired,
    /// Document integrity intact
    SignIntegrityOk,
    /// Document has been tampered with
    SignTampered,
    /// Signature count, plural
    SignCount,
    /// Signer
    SignSigner,
    /// Signed at
    SignSignedAt,
    /// Algorithm
    SignAlgorithm,
    /// Issuer
    SignIssuer,
    /// Export verification report
    SignExportReport,
    /// Heading on the signature panel in the reader
    SignPanelTitle,
    /// The certificate's validity window, from and to
    SignValidUntil,
    /// How many files a signature covers, and how many nothing covers
    SignCovers,
    /// The three lines an invoice seal prints
    SignSealText,
    /// The details of one seal, opened by clicking it
    SignSealDetails,

    // Annotation module
    /// Heading on the annotation panel in the reader
    AnnotPanelTitle,
    /// Annotation count, plural
    AnnotCount,
    /// The document carries no annotations
    AnnotNone,

    // Comparison module
    /// Heading on the comparison panel in the reader
    ComparePanelTitle,
    /// Menu item that opens a second revision to compare against
    CompareOpenSecond,
    /// Whether every page matches
    CompareIdentical,
    /// Page count of a change summary, plural
    CompareChanged,
    /// One page row: its index and what changed
    ComparePage,

    // Edit module
    /// Confirm page deletion
    EditConfirmDeletePages,
    /// Warn that signatures will be invalidated, with a count
    EditWarnInvalidateSignature,
    /// Undo
    EditUndo,
    /// Redo
    EditRedo,

    // Search module
    /// Search result count, plural
    SearchResultCount,
    /// Search input placeholder
    SearchPlaceholder,

    // Command line output
    /// Page count line in document info
    CliPages,
    /// Page size line in document info
    CliPageSize,
    /// Version line in document info
    CliVersion,
    /// Embedded font count line in document info
    CliEmbeddedFonts,
    /// Font line in document info, naming a face the reader had to supply
    CliFontSubstituted,
    /// Font line in document info, naming a face nothing could supply
    CliFontMissing,
    /// Warning: glyphs had no outline in any face
    CliMissingGlyphs,
    /// Warning: images could not be decoded and were left off the page
    CliMissingImages,
    /// Signature count line in document info
    CliSignatures,
    /// The document declares no pages
    CliNoPages,
    /// Confirmation that a file was written
    CliWrote,
    /// One page rendered at a resolution
    CliRenderedPage,
    /// A batch of pages converted to a format
    CliConverted,
    /// One search hit with its snippet
    CliSearchHit,
    /// The audit found nothing
    CliAuditClean,
    /// One audit finding
    CliAuditFinding,
    /// Print job plan summary
    CliPrintPlan,
    /// One signature with its verification status
    CliSignEntry,
    /// The document declares no signatures
    CliSignNone,
    /// The certificate's validity window, from and to
    CliCertValidity,
    /// The validator that wrote the signature
    CliSignProvider,
    /// How many files the signature covers and how many nothing covers
    CliSignCoverage,
    /// Where the seals are drawn, one box per stamp
    CliSignStamps,
    /// The three lines an invoice seal prints
    CliSignSeal,
    /// Prefix for a reported error
    CliError,

    // Error messages
    /// File is not a valid OFD document
    ErrNotOfd,
    /// Document requires a password
    ErrPasswordRequired,
    /// Page index out of range, with index and total
    ErrPageOutOfRange,
    /// Container is corrupted
    ErrPackageCorrupted,
    /// Font missing, named
    ErrFontMissing,
    /// Referenced resource does not exist
    ErrDanglingReference,
    /// Resource limit exceeded
    ErrResourceLimit,
    /// I/O failure
    ErrIo,
    /// Unsupported feature
    ErrUnsupported,
}

impl MessageId {
    /// The Fluent key for this message
    pub fn key(self) -> &'static str {
        match self {
            // Application level
            Self::AppName => "app-name",
            Self::AppReady => "app-ready",
            Self::AppLoading => "app-loading",
            Self::AppCancelled => "app-cancelled",
            Self::CommonConfirm => "common-confirm",
            Self::CommonCancel => "common-cancel",
            Self::CommonSave => "common-save",
            Self::CommonClose => "common-close",
            Self::CommonRetry => "common-retry",
            Self::CommonDelete => "common-delete",

            Self::LocaleNameEn => "locale-name-en",
            Self::LocaleNameZh => "locale-name-zh",

            // Conversion
            Self::ConvertStart => "convert-start",
            Self::ConvertProgress => "convert-progress",
            Self::ConvertDone => "convert-done",
            Self::ConvertFailed => "convert-failed",
            Self::ConvertOutputFormat => "convert-output-format",
            Self::ConvertPageRange => "convert-page-range",
            Self::ConvertDpi => "convert-dpi",
            Self::ConvertKeepBookmarks => "convert-keep-bookmarks",
            Self::ConvertKeepSignatures => "convert-keep-signatures",

            // Signature
            Self::SignValid => "sign-valid",
            Self::SignInvalid => "sign-invalid",
            Self::SignUnverified => "sign-unverified",
            Self::SignUntrusted => "sign-untrusted",
            Self::SignExpired => "sign-expired",
            Self::SignIntegrityOk => "sign-integrity-ok",
            Self::SignTampered => "sign-tampered",
            Self::SignCount => "sign-count",
            Self::SignSigner => "sign-signer",
            Self::SignSignedAt => "sign-signed-at",
            Self::SignAlgorithm => "sign-algorithm",
            Self::SignIssuer => "sign-issuer",
            Self::SignExportReport => "sign-export-report",
            Self::SignPanelTitle => "sign-panel-title",
            Self::SignValidUntil => "sign-valid-until",
            Self::SignCovers => "sign-covers",
            Self::SignSealText => "sign-seal-text",
            Self::SignSealDetails => "sign-seal-details",

            // Annotation
            Self::AnnotPanelTitle => "annot-panel-title",
            Self::AnnotCount => "annot-count",
            Self::AnnotNone => "annot-none",

            // Comparison
            Self::ComparePanelTitle => "compare-panel-title",
            Self::CompareOpenSecond => "compare-open-second",
            Self::CompareIdentical => "compare-identical",
            Self::CompareChanged => "compare-changed",
            Self::ComparePage => "compare-page",

            // Edit
            Self::EditConfirmDeletePages => "edit-confirm-delete-pages",
            Self::EditWarnInvalidateSignature => "edit-warn-invalidate-signature",
            Self::EditUndo => "edit-undo",
            Self::EditRedo => "edit-redo",

            // Search
            Self::SearchResultCount => "search-result-count",
            Self::SearchPlaceholder => "search-placeholder",

            // Command line output
            Self::CliPages => "cli-pages",
            Self::CliPageSize => "cli-page-size",
            Self::CliVersion => "cli-version",
            Self::CliEmbeddedFonts => "cli-embedded-fonts",
            Self::CliFontSubstituted => "cli-font-substituted",
            Self::CliFontMissing => "cli-font-missing",
            Self::CliMissingGlyphs => "cli-missing-glyphs",
            Self::CliMissingImages => "cli-missing-images",
            Self::CliSignatures => "cli-signatures",
            Self::CliNoPages => "cli-no-pages",
            Self::CliWrote => "cli-wrote",
            Self::CliRenderedPage => "cli-rendered-page",
            Self::CliConverted => "cli-converted",
            Self::CliSearchHit => "cli-search-hit",
            Self::CliAuditClean => "cli-audit-clean",
            Self::CliAuditFinding => "cli-audit-finding",
            Self::CliPrintPlan => "cli-print-plan",
            Self::CliSignEntry => "cli-sign-entry",
            Self::CliSignNone => "cli-sign-none",
            Self::CliCertValidity => "cli-cert-validity",
            Self::CliSignProvider => "cli-sign-provider",
            Self::CliSignCoverage => "cli-sign-coverage",
            Self::CliSignStamps => "cli-sign-stamps",
            Self::CliSignSeal => "cli-sign-seal",
            Self::CliError => "cli-error",

            // Errors
            Self::ErrNotOfd => "err-not-ofd",
            Self::ErrPasswordRequired => "err-password-required",
            Self::ErrPageOutOfRange => "err-page-out-of-range",
            Self::ErrPackageCorrupted => "err-package-corrupted",
            Self::ErrFontMissing => "err-font-missing",
            Self::ErrDanglingReference => "err-dangling-reference",
            Self::ErrResourceLimit => "err-resource-limit",
            Self::ErrIo => "err-io",
            Self::ErrUnsupported => "err-unsupported",
        }
    }

    /// The message id for a Fluent key, or `None` when the key is unknown.
    /// The inverse of [`MessageId::key`]; used to prove the two never drift.
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|id| id.key() == key)
    }

    /// Every message id, in the same order as `bundle::all_keys`.
    pub const ALL: &'static [Self] = &[
        // Application level
        Self::AppName,
        Self::AppReady,
        Self::AppLoading,
        Self::AppCancelled,
        Self::CommonConfirm,
        Self::CommonCancel,
        Self::CommonSave,
        Self::CommonClose,
        Self::CommonRetry,
        Self::CommonDelete,
        Self::LocaleNameEn,
        Self::LocaleNameZh,
        // Conversion
        Self::ConvertStart,
        Self::ConvertProgress,
        Self::ConvertDone,
        Self::ConvertFailed,
        Self::ConvertOutputFormat,
        Self::ConvertPageRange,
        Self::ConvertDpi,
        Self::ConvertKeepBookmarks,
        Self::ConvertKeepSignatures,
        // Signature
        Self::SignValid,
        Self::SignInvalid,
        Self::SignUnverified,
        Self::SignUntrusted,
        Self::SignExpired,
        Self::SignIntegrityOk,
        Self::SignTampered,
        Self::SignCount,
        Self::SignSigner,
        Self::SignSignedAt,
        Self::SignAlgorithm,
        Self::SignIssuer,
        Self::SignExportReport,
        Self::SignPanelTitle,
        Self::SignValidUntil,
        Self::SignCovers,
        Self::SignSealText,
        Self::SignSealDetails,
        // Annotation
        Self::AnnotPanelTitle,
        Self::AnnotCount,
        Self::AnnotNone,
        // Comparison
        Self::ComparePanelTitle,
        Self::CompareOpenSecond,
        Self::CompareIdentical,
        Self::CompareChanged,
        Self::ComparePage,
        Self::EditConfirmDeletePages,
        Self::EditWarnInvalidateSignature,
        Self::EditUndo,
        Self::EditRedo,
        // Search
        Self::SearchResultCount,
        Self::SearchPlaceholder,
        // Command line output
        Self::CliPages,
        Self::CliPageSize,
        Self::CliVersion,
        Self::CliEmbeddedFonts,
        Self::CliFontSubstituted,
        Self::CliFontMissing,
        Self::CliMissingGlyphs,
        Self::CliMissingImages,
        Self::CliSignatures,
        Self::CliNoPages,
        Self::CliWrote,
        Self::CliRenderedPage,
        Self::CliConverted,
        Self::CliSearchHit,
        Self::CliAuditClean,
        Self::CliAuditFinding,
        Self::CliPrintPlan,
        Self::CliSignEntry,
        Self::CliSignNone,
        Self::CliCertValidity,
        Self::CliSignProvider,
        Self::CliSignCoverage,
        Self::CliSignStamps,
        Self::CliSignSeal,
        Self::CliError,
        // Errors
        Self::ErrNotOfd,
        Self::ErrPasswordRequired,
        Self::ErrPageOutOfRange,
        Self::ErrPackageCorrupted,
        Self::ErrFontMissing,
        Self::ErrDanglingReference,
        Self::ErrResourceLimit,
        Self::ErrIo,
        Self::ErrUnsupported,
    ];

    /// Whether the message takes variables, read by the static checker
    pub fn is_parametric(self) -> bool {
        matches!(
            self,
            Self::AppLoading
                | Self::ConvertProgress
                | Self::ConvertFailed
                | Self::SignCount
                | Self::AnnotCount
                | Self::CompareChanged
                | Self::SignSigner
                | Self::SignSignedAt
                | Self::SignAlgorithm
                | Self::SignIssuer
                | Self::SignValidUntil
                | Self::SignCovers
                | Self::SignSealText
                | Self::EditWarnInvalidateSignature
                | Self::SearchResultCount
                | Self::ErrPageOutOfRange
                | Self::ErrFontMissing
                | Self::ErrDanglingReference
                | Self::ErrResourceLimit
                | Self::CliPages
                | Self::CliPageSize
                | Self::CliVersion
                | Self::CliEmbeddedFonts
                | Self::CliFontSubstituted
                | Self::CliFontMissing
                | Self::CliMissingGlyphs
                | Self::CliMissingImages
                | Self::CliSignatures
                | Self::CliWrote
                | Self::CliRenderedPage
                | Self::CliConverted
                | Self::CliSearchHit
                | Self::CliAuditFinding
                | Self::CliPrintPlan
                | Self::CliSignEntry
                | Self::CliCertValidity
                | Self::CliSignProvider
                | Self::CliSignCoverage
                | Self::CliSignStamps
                | Self::CliSignSeal
                | Self::CliError
        )
    }
}

impl fmt::Display for MessageId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.key())
    }
}
