//! Command implementations behind the `sofd` binary.
//! Every entry point takes parsed arguments plus a localization context and
//! returns a rendered report, so the logic runs under test without a process.

#![warn(missing_docs)]

pub mod cli;

use std::path::{Path, PathBuf};

use anyhow::Result;
use clap::Parser;
use sofd::{
    AuditReport, ConversionOutcome, Document, PaperSize, PrintSettings, RenderOptions,
    SearchOptions, Severity, SignatureDetails, VerifyStatus,
};
use sofd_error::{FontError, FormatError, OfdError};
use sofd_font::FontSource;
use sofd_i18n::format::LocalizedFormat;
use sofd_i18n::{FluentValue, I18n, Locale, MessageId};
use sofd_io::FileSource;

use crate::cli::{Cli, Command, FormatArg, PaperArg};

/// What a command has to say, and whether the process should exit nonzero.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// Text ready to print, one line per finding.
    pub text: String,
    /// False when the command ran but its outcome is a failure.
    pub ok: bool,
    /// Problems that did not stop the command: a substituted face, characters
    /// no face carried. They go to stderr and make the exit code nonzero, so a
    /// script cannot mistake a degraded page for a good one.
    pub warnings: Vec<String>,
}

impl Report {
    /// A successful report.
    pub fn ok(text: String) -> Self {
        Self {
            text,
            ok: true,
            warnings: Vec::new(),
        }
    }

    /// A report that should turn the exit code nonzero.
    pub fn failed(text: String) -> Self {
        Self {
            text,
            ok: false,
            warnings: Vec::new(),
        }
    }

    /// Attach warnings, and step the exit code down when there are any.
    pub fn warn(mut self, warnings: Vec<String>) -> Self {
        if !warnings.is_empty() {
            self.ok = false;
        }
        self.warnings = warnings;
        self
    }
}

/// Run one parsed command line.
pub fn run(cli: &Cli, i18n: &I18n) -> Result<Report> {
    match &cli.command {
        Command::Info(args) => info(&args.path, i18n),
        Command::Render(args) => render(
            &args.path,
            args.page,
            args.dpi,
            args.output.as_deref(),
            i18n,
        ),
        Command::Convert(args) => convert(
            &args.path,
            args.format,
            args.page,
            args.dpi,
            args.output.as_deref(),
            i18n,
        ),
        Command::Verify(args) => verify(&args.path, i18n),
        Command::Search(args) => search(
            &args.path,
            &args.needle,
            args.case_sensitive,
            args.whole_word,
            i18n,
        ),
        Command::Audit(args) => audit(&args.path, i18n),
        Command::Print(args) => print(&args.path, args.paper, args.dpi, args.copies, i18n),
    }
}

/// Turn a failure into a printable report.
/// Structured engine errors get a localized reason; anything else prints as-is.
pub fn error_report(err: &anyhow::Error, i18n: &I18n) -> Report {
    let message = match err.downcast_ref::<OfdError>() {
        Some(ofd) => localized_error(ofd, i18n),
        None => err.to_string(),
    };
    Report::failed(i18n.text_with(MessageId::CliError, [("message", message.into())]))
}

/// Open a document from a path.
fn open(path: &Path) -> Result<Document> {
    let source = FileSource::open(path)?;
    Ok(Document::open(&source)?)
}

/// Parse a command line, for tests and embedders.
/// The program name is supplied, so callers list arguments only.
pub fn parse(args: &[&str]) -> std::result::Result<Cli, clap::Error> {
    Cli::try_parse_from(std::iter::once("sofd").chain(args.iter().copied()))
}

/// Localization context for a parsed command line.
/// An explicit `--locale` wins; otherwise the system locale is negotiated.
pub fn i18n_for(cli: &Cli) -> I18n {
    match &cli.locale {
        Some(tag) => I18n::with_locale(Locale::new(tag)),
        None => I18n::with_system_locale(),
    }
}

fn info(path: &Path, i18n: &I18n) -> Result<Report> {
    let mut doc = open(path)?;
    let fmt = LocalizedFormat::new(i18n.locale().clone());
    let page_box = doc.page_box();
    let embedded = doc
        .fonts()
        .registry()
        .iter()
        .filter(|entry| matches!(entry.source, FontSource::Embedded { .. }))
        .count();
    let mut lines = vec![
        i18n.text_with(MessageId::CliVersion, [("version", doc.version().into())]),
        i18n.text_with(
            MessageId::CliPages,
            [("count", fmt.integer(doc.page_count() as i64).into())],
        ),
        i18n.text_with(
            MessageId::CliPageSize,
            [
                ("w", fmt.decimal(page_box.w, 1).into()),
                ("h", fmt.decimal(page_box.h, 1).into()),
            ],
        ),
        i18n.text_with(
            MessageId::CliEmbeddedFonts,
            [("count", fmt.integer(embedded as i64).into())],
        ),
    ];
    if doc.page_count() == 0 {
        lines.push(i18n.text(MessageId::CliNoPages));
    }
    // Info is asked precisely because something looked wrong, so every face is
    // resolved here rather than waiting for a page to need it: a document that
    // names a face it never shipped says so even before anything is drawn.
    resolve_fonts(&doc);
    let mut warnings = Vec::new();
    lines.extend(font_lines(&doc, i18n, &fmt, &mut warnings));
    let signatures = doc.signatures().map(|list| list.len()).unwrap_or(0);
    lines.push(i18n.text_with(
        MessageId::CliSignatures,
        [("count", fmt.integer(signatures as i64).into())],
    ));
    Ok(Report::ok(lines.join("\n")).warn(warnings))
}

/// Load every face the registry holds, so `unresolved` is complete.
fn resolve_fonts(doc: &Document) {
    for entry in doc.fonts().registry().iter() {
        let _ = doc.fonts().face(entry.id);
    }
}

/// Two lines at most: the faces a substitute covered, and the ones nothing
/// could. A declaration the host searched for and found is not news.
fn font_lines(
    doc: &Document,
    i18n: &I18n,
    fmt: &LocalizedFormat,
    warnings: &mut Vec<String>,
) -> Vec<String> {
    let mut substituted = Vec::new();
    let mut missing = Vec::new();
    for face in doc.missing_fonts() {
        match face.source {
            FontSource::System { .. } => substituted.push(face.family),
            _ => missing.push(face.family),
        }
    }
    let mut lines = Vec::new();
    if !substituted.is_empty() {
        lines.push(i18n.text_with(
            MessageId::CliFontSubstituted,
            [
                ("count", fmt.integer(substituted.len() as i64).into()),
                ("names", substituted.join(", ").into()),
            ],
        ));
    }
    if !missing.is_empty() {
        let line = i18n.text_with(
            MessageId::CliFontMissing,
            [
                ("count", fmt.integer(missing.len() as i64).into()),
                ("names", missing.join(", ").into()),
            ],
        );
        warnings.push(line.clone());
        lines.push(line);
    }
    lines
}

fn render(
    path: &Path,
    page: usize,
    dpi: f64,
    output: Option<&Path>,
    i18n: &I18n,
) -> Result<Report> {
    let doc = open(path)?;
    let out = output
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from(format!("page-{page}.png")));
    let options = RenderOptions {
        dpi,
        ..Default::default()
    };
    let (png, raster) = doc.to_png_with_stats(page, &options)?;
    std::fs::write(&out, png)?;
    let fmt = LocalizedFormat::new(i18n.locale().clone());
    let text = [
        i18n.text_with(
            MessageId::CliRenderedPage,
            [
                ("page", fmt.integer(page as i64).into()),
                ("dpi", fmt.decimal(dpi, 0).into()),
            ],
        ),
        i18n.text_with(
            MessageId::CliWrote,
            [("path", out.display().to_string().into())],
        ),
    ]
    .join("\n");
    // The page was written either way. What it is short of is said on stderr
    // and in the exit code, because a PNG of a blank page looks exactly like
    // a PNG of a page that had nothing on it.
    let mut warnings = font_warnings(&doc, i18n, &fmt);
    warnings.extend(glyph_warning(raster.missing_glyphs, i18n, &fmt));
    warnings.extend(image_warning(raster.skipped_image_ops, i18n, &fmt));
    Ok(Report::ok(text).warn(warnings))
}

/// Faces the document wanted and did not get, one line per face.
fn font_warnings(doc: &Document, i18n: &I18n, fmt: &LocalizedFormat) -> Vec<String> {
    let mut substituted = Vec::new();
    let mut missing = Vec::new();
    for face in doc.missing_fonts() {
        match face.source {
            FontSource::System { .. } => substituted.push(face.family),
            _ => missing.push(face.family),
        }
    }
    let mut lines = Vec::new();
    if !substituted.is_empty() {
        lines.push(i18n.text_with(
            MessageId::CliFontSubstituted,
            [
                ("count", fmt.integer(substituted.len() as i64).into()),
                ("names", substituted.join(", ").into()),
            ],
        ));
    }
    if !missing.is_empty() {
        lines.push(i18n.text_with(
            MessageId::CliFontMissing,
            [
                ("count", fmt.integer(missing.len() as i64).into()),
                ("names", missing.join(", ").into()),
            ],
        ));
    }
    lines
}

/// A line when characters were left out because no face carried them.
/// The count is passed as a number, not as formatted text: Fluent picks the
/// plural branch from the value's type, so a `String` always falls to `other`.
fn glyph_warning(missing: usize, i18n: &I18n, _fmt: &LocalizedFormat) -> Vec<String> {
    if missing == 0 {
        return Vec::new();
    }
    vec![i18n.text_with(
        MessageId::CliMissingGlyphs,
        [("count", (missing as i64).into())],
    )]
}

/// An image the backend could not decode leaves a hole no other warning covers:
/// the page still renders, and only the count says part of it is absent.
fn image_warning(skipped: usize, i18n: &I18n, _fmt: &LocalizedFormat) -> Vec<String> {
    if skipped == 0 {
        return Vec::new();
    }
    vec![i18n.text_with(
        MessageId::CliMissingImages,
        [("count", (skipped as i64).into())],
    )]
}

fn convert(
    path: &Path,
    format: FormatArg,
    page: Option<usize>,
    dpi: f64,
    output: Option<&Path>,
    i18n: &I18n,
) -> Result<Report> {
    let doc = open(path)?;
    let dir = output
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let stem = path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "document".to_string());
    let pages = doc.page_count();
    let every: Vec<usize> = (0..pages).collect();
    let indices: Vec<usize> = match page {
        Some(index) => vec![index],
        None => every.clone(),
    };

    // A conversion that loses a face or a glyph still writes a file, so the
    // counts collected on the way out are the only sign it happened.
    // The facade merges the per-page counts, so this loop only writes files.
    let mut lost = ConversionOutcome::default();
    let (count, wrote) = match format {
        FormatArg::Pdf => {
            let out = dir.join(format!("{stem}.pdf"));
            // One document, not one file per page: a page at a time leaves
            // only the last of them under the document's name.
            let (pdf, outcome) = doc.pdf_outcome(&indices)?;
            lost = outcome;
            std::fs::write(&out, pdf)?;
            (indices.len(), Some(out))
        }
        FormatArg::Png => {
            let options = RenderOptions {
                dpi,
                ..Default::default()
            };
            for index in &indices {
                let out = dir.join(format!("{stem}-{index}.png"));
                let (png, outcome) = doc.png_outcome(*index, &options)?;
                lost.merge(&outcome);
                std::fs::write(&out, png)?;
            }
            (indices.len(), None)
        }
        FormatArg::Svg => {
            for index in &indices {
                let out = dir.join(format!("{stem}-{index}.svg"));
                let (svg, outcome) = doc.svg_outcome(*index)?;
                lost.merge(&outcome);
                std::fs::write(&out, svg)?;
            }
            (indices.len(), None)
        }
    };

    let mut lines = vec![i18n.text_with(
        MessageId::CliConverted,
        [
            ("count", (count as i64).into()),
            ("format", format.label().into()),
        ],
    )];
    if let Some(out) = wrote {
        lines.push(i18n.text_with(
            MessageId::CliWrote,
            [("path", out.display().to_string().into())],
        ));
    }
    let work = LocalizedFormat::new(i18n.locale().clone());
    let mut warnings = font_warnings(&doc, i18n, &work);
    warnings.extend(glyph_warning(lost.missing_glyphs, i18n, &work));
    warnings.extend(image_warning(lost.missing_images, i18n, &work));
    Ok(Report::ok(lines.join("\n")).warn(warnings))
}

fn verify(path: &Path, i18n: &I18n) -> Result<Report> {
    let mut doc = open(path)?;
    let refs = match doc.signatures() {
        Err(OfdError::Format(FormatError::MissingSignatures)) => {
            return Ok(Report::failed(i18n.text(MessageId::CliSignNone)));
        }
        result => result?,
    };
    let statuses = doc.verify_signatures()?;
    // Who signed, when, and how much of the file the proof covers: a verdict on
    // its own says a signature holds and nothing about what it is over, which
    // is the part a reader carrying an invoice needs.
    let details = doc.signature_details().unwrap_or_default();
    let mut lines: Vec<String> = Vec::new();
    for (index, (sig, status)) in refs.iter().zip(&statuses).enumerate() {
        lines.push(i18n.text_with(
            MessageId::CliSignEntry,
            [
                ("loc", sig.base_loc.clone().into()),
                ("status", verify_label(status, i18n).into()),
            ],
        ));
        let Some(detail) = details.get(index) else {
            continue;
        };
        for line in signature_detail_lines(detail, i18n) {
            lines.push(line);
        }
    }
    let ok = statuses.iter().all(|s| matches!(s, VerifyStatus::Valid));
    Ok(Report {
        text: lines.join("\n"),
        ok,
        warnings: Vec::new(),
    })
}

fn search(
    path: &Path,
    needle: &str,
    case_sensitive: bool,
    whole_word: bool,
    i18n: &I18n,
) -> Result<Report> {
    let doc = open(path)?;
    let options = SearchOptions {
        case_sensitive,
        whole_word,
        max_hits: None,
    };
    let fmt = LocalizedFormat::new(i18n.locale().clone());
    let lines: Vec<String> = doc
        .search(needle, options)
        .iter()
        .map(|hit| {
            i18n.text_with(
                MessageId::CliSearchHit,
                [
                    ("page", fmt.integer(hit.page as i64).into()),
                    ("snippet", hit.snippet.clone().into()),
                ],
            )
        })
        .collect();
    Ok(Report::ok(lines.join("\n")))
}

fn audit(path: &Path, i18n: &I18n) -> Result<Report> {
    let mut doc = open(path)?;
    let report: AuditReport = doc.audit()?;
    if report.findings.is_empty() {
        return Ok(Report::ok(i18n.text(MessageId::CliAuditClean)));
    }
    let lines: Vec<String> = report
        .findings
        .iter()
        .map(|finding| {
            i18n.text_with(
                MessageId::CliAuditFinding,
                [
                    ("code", finding.code.into()),
                    ("severity", finding.severity.label().into()),
                    ("location", finding.location.clone().into()),
                    ("detail", finding.detail.clone().into()),
                ],
            )
        })
        .collect();
    let ok = !report
        .findings
        .iter()
        .any(|f| matches!(f.severity, Severity::Error));
    Ok(Report {
        text: lines.join("\n"),
        ok,
        warnings: Vec::new(),
    })
}

fn print(path: &Path, paper: PaperArg, dpi: f64, copies: usize, i18n: &I18n) -> Result<Report> {
    let doc = open(path)?;
    let settings = PrintSettings {
        paper: paper_size(paper),
        dpi,
        copies: copies.max(1),
        ..Default::default()
    };
    let job = doc.print_job(&settings)?;
    // Rasterize too: a plan that cannot produce bits is not a print job.
    let raster = doc.print_render(&settings)?;
    let sheets = job
        .pages
        .iter()
        .map(|planned| planned.sheet + 1)
        .max()
        .unwrap_or(0);
    let fmt = LocalizedFormat::new(i18n.locale().clone());
    let text = i18n.text_with(
        MessageId::CliPrintPlan,
        [
            ("pages", (job.pages.len() as i64).into()),
            ("sheets", (sheets as i64).into()),
            ("paper", paper.label().into()),
            ("dpi", fmt.decimal(dpi, 0).into()),
        ],
    );
    debug_assert_eq!(raster.len(), job.pages.len());
    Ok(Report::ok(text))
}

fn paper_size(paper: PaperArg) -> PaperSize {
    match paper {
        PaperArg::Document => PaperSize::Document,
        PaperArg::A3 => PaperSize::A3,
        PaperArg::A4 => PaperSize::A4,
        PaperArg::A5 => PaperSize::A5,
        PaperArg::B5 => PaperSize::B5,
        PaperArg::Letter => PaperSize::Letter,
        PaperArg::Legal => PaperSize::Legal,
    }
}

/// The lines under a verdict: who signed, when, and what the proof covers.
///
/// Quiet about what it does not know, rather than printing `unknown` five
/// times: a signature with no provider line has no provider, and saying so adds
/// a row nobody reads.
fn signature_detail_lines(detail: &SignatureDetails, i18n: &I18n) -> Vec<String> {
    // One row per fact the descriptor or the certificate actually states. A
    // signature with no provider gets no provider row, rather than a row
    // reading "unknown" that nobody can act on.
    let mut rows: Vec<(MessageId, Vec<(&str, String)>)> = Vec::new();
    if let Some(name) = &detail.common_name {
        rows.push((MessageId::SignSigner, vec![("name", name.clone())]));
    }
    if let Some(time) = &detail.signed_at {
        rows.push((MessageId::SignSignedAt, vec![("time", time.clone())]));
    }
    if let Some(issuer) = &detail.issuer {
        rows.push((MessageId::SignIssuer, vec![("issuer", issuer.clone())]));
    }
    if let (Some(from), Some(to)) = (&detail.not_before, &detail.not_after) {
        rows.push((
            MessageId::CliCertValidity,
            vec![("from", from.clone()), ("to", to.clone())],
        ));
    }
    if let Some(method) = &detail.check_method {
        rows.push((MessageId::SignAlgorithm, vec![("algo", method.clone())]));
    }
    if let Some(provider) = &detail.provider {
        rows.push((MessageId::CliSignProvider, vec![("name", provider.clone())]));
    }
    rows.push((
        MessageId::CliSignCoverage,
        vec![
            ("count", detail.covered.to_string()),
            ("loose", detail.uncovered.len().to_string()),
        ],
    ));
    // A stamp box matters for an invoice and nothing else, so it is reported
    // only when there is one.
    if !detail.stamps.is_empty() {
        rows.push((
            MessageId::CliSignStamps,
            vec![(
                "list",
                detail
                    .stamps
                    .iter()
                    .map(ToString::to_string)
                    .collect::<Vec<_>>()
                    .join("; "),
            )],
        ));
    }
    if let Some(seal) = &detail.seal {
        rows.push((MessageId::CliSignSeal, vec![("text", seal.row())]));
    }
    rows.into_iter()
        .map(|(id, fields)| {
            let args: Vec<(&str, FluentValue)> = fields
                .into_iter()
                .map(|(key, value)| (key, value.into()))
                .collect();
            i18n.text_with(id, args)
        })
        .collect()
}

fn verify_label(status: &VerifyStatus, i18n: &I18n) -> String {
    match status {
        VerifyStatus::Valid => i18n.text(MessageId::SignValid),
        VerifyStatus::Tampered => i18n.text(MessageId::SignInvalid),
        VerifyStatus::Unchecked => i18n.text(MessageId::SignUnverified),
        VerifyStatus::Untrusted => i18n.text(MessageId::SignUntrusted),
    }
}

/// Localized reason for a structured engine error.
fn localized_error(err: &OfdError, i18n: &I18n) -> String {
    let fmt = LocalizedFormat::new(i18n.locale().clone());
    match err {
        OfdError::Io(e) => i18n.text_with(MessageId::ErrIo, [("detail", e.to_string().into())]),
        OfdError::Package(e) => i18n.text_with(
            MessageId::ErrPackageCorrupted,
            [("reason", e.to_string().into())],
        ),
        OfdError::Xml(e) => i18n.text_with(
            MessageId::ErrPackageCorrupted,
            [("reason", e.to_string().into())],
        ),
        OfdError::Format(format) => match format {
            FormatError::NotOfd => i18n.text(MessageId::ErrNotOfd),
            FormatError::PageOutOfRange { index, total } => i18n.text_with(
                MessageId::ErrPageOutOfRange,
                [
                    ("index", fmt.integer(*index as i64).into()),
                    ("total", fmt.integer(*total as i64).into()),
                ],
            ),
            FormatError::DanglingReference { kind, id } => i18n.text_with(
                MessageId::ErrDanglingReference,
                [("kind", (*kind).into()), ("id", id.clone().into())],
            ),
            FormatError::MaxDepthExceeded(depth) => i18n.text_with(
                MessageId::ErrResourceLimit,
                [("detail", fmt.integer(*depth as i64).into())],
            ),
            _ => i18n.text_with(
                MessageId::ErrUnsupported,
                [("feature", err.to_string().into())],
            ),
        },
        OfdError::Font(FontError::Missing { family }) => i18n.text_with(
            MessageId::ErrFontMissing,
            [("family", family.clone().into())],
        ),
        OfdError::Font(_) => i18n.text_with(
            MessageId::ErrUnsupported,
            [("feature", err.to_string().into())],
        ),
        OfdError::ResourceLimit(detail) => i18n.text_with(
            MessageId::ErrResourceLimit,
            [("detail", detail.clone().into())],
        ),
        OfdError::PasswordRequired => i18n.text(MessageId::ErrPasswordRequired),
        _ => i18n.text_with(
            MessageId::ErrUnsupported,
            [("feature", err.to_string().into())],
        ),
    }
}
