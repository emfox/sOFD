//! Command line grammar.
//! Parsing only: every flag lands in a plain struct, and `lib.rs` turns the
//! struct into work, so the grammar stays testable without a process.

use std::path::PathBuf;

use clap::{Parser, Subcommand, ValueEnum};

/// Output formats the convert command can produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum FormatArg {
    /// One PDF holding every page.
    Pdf,
    /// One PNG per page.
    Png,
    /// One SVG per page.
    Svg,
}

impl FormatArg {
    /// Lowercase name used in reports and file extensions.
    pub fn label(self) -> &'static str {
        match self {
            Self::Pdf => "pdf",
            Self::Png => "png",
            Self::Svg => "svg",
        }
    }
}

/// Paper sizes the print command accepts, mirroring the print engine.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub enum PaperArg {
    /// Follow each page's own box.
    Document,
    /// ISO A3.
    A3,
    /// ISO A4.
    A4,
    /// ISO A5.
    A5,
    /// ISO B5.
    B5,
    /// US Letter.
    Letter,
    /// US Legal.
    Legal,
}

impl PaperArg {
    /// Name shown in the print plan report.
    pub fn label(self) -> &'static str {
        match self {
            Self::Document => "document",
            Self::A3 => "A3",
            Self::A4 => "A4",
            Self::A5 => "A5",
            Self::B5 => "B5",
            Self::Letter => "Letter",
            Self::Legal => "Legal",
        }
    }
}

#[derive(Parser, Debug)]
#[command(
    name = "sofd",
    version,
    about = "Read, render and convert OFD documents"
)]
/// Top level command line: global options plus one subcommand.
pub struct Cli {
    /// Locale tag such as en-US or zh-CN; defaults to the system locale.
    #[arg(long, global = true)]
    pub locale: Option<String>,

    /// Log what the engine is doing to stderr.
    #[arg(short, long, global = true)]
    pub verbose: bool,

    #[command(subcommand)]
    /// The subcommand to run.
    pub command: Command,
}

#[derive(Subcommand, Debug)]
/// What to do with the document.
pub enum Command {
    /// Print what the document declares: pages, size, version, fonts.
    Info(InfoArgs),
    /// Rasterize one page to a PNG file.
    Render(RenderArgs),
    /// Convert the document to PDF, PNG or SVG files.
    Convert(ConvertArgs),
    /// Verify every signature the document declares.
    Verify(VerifyArgs),
    /// Full-text search over the document's pages.
    Search(SearchArgs),
    /// Audit the container for archival readiness.
    Audit(AuditArgs),
    /// Plan a print job and rasterize it at print resolution.
    Print(PrintArgs),
}

#[derive(Parser, Debug)]
/// Arguments of the info command.
pub struct InfoArgs {
    /// OFD file to inspect.
    pub path: PathBuf,
}

#[derive(Parser, Debug)]
/// Arguments of the render command.
pub struct RenderArgs {
    /// OFD file to render.
    pub path: PathBuf,

    /// Zero-based page index; defaults to the first page.
    #[arg(short, long, default_value_t = 0)]
    pub page: usize,

    /// Resolution in dots per inch.
    #[arg(long, default_value_t = 150.0)]
    pub dpi: f64,

    /// Output PNG path; defaults to `page-<index>.png`.
    #[arg(short, long)]
    pub output: Option<PathBuf>,
}

#[derive(Parser, Debug)]
/// Arguments of the convert command.
pub struct ConvertArgs {
    /// OFD file to convert.
    pub path: PathBuf,

    /// Output format.
    #[arg(short, long, value_enum, default_value_t = FormatArg::Pdf)]
    pub format: FormatArg,

    /// Zero-based page index for the single-page formats; every page when
    /// omitted. PDF always holds the whole document.
    #[arg(short, long)]
    pub page: Option<usize>,

    /// Resolution in dots per inch, for PNG output.
    #[arg(long, default_value_t = 150.0)]
    pub dpi: f64,

    /// Output directory; defaults to the current one.
    #[arg(short, long)]
    pub output: Option<PathBuf>,
}

#[derive(Parser, Debug)]
/// Arguments of the verify command.
pub struct VerifyArgs {
    /// OFD file to verify.
    pub path: PathBuf,
}

#[derive(Parser, Debug)]
/// Arguments of the search command.
pub struct SearchArgs {
    /// OFD file to search.
    pub path: PathBuf,

    /// Text to look for.
    pub needle: String,

    /// Match case exactly.
    #[arg(short, long)]
    pub case_sensitive: bool,

    /// Require the match to be delimited by non-word characters.
    #[arg(short, long)]
    pub whole_word: bool,
}

#[derive(Parser, Debug)]
/// Arguments of the audit command.
pub struct AuditArgs {
    /// OFD file to audit.
    pub path: PathBuf,
}

#[derive(Parser, Debug)]
/// Arguments of the print command.
pub struct PrintArgs {
    /// OFD file to print.
    pub path: PathBuf,

    /// Paper size.
    #[arg(long, value_enum, default_value_t = PaperArg::A4)]
    pub paper: PaperArg,

    /// Resolution in dots per inch.
    #[arg(long, default_value_t = 300.0)]
    pub dpi: f64,

    /// How many copies of the whole document.
    #[arg(short, long, default_value_t = 1)]
    pub copies: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn grammar_is_well_formed() {
        // Catches flag typos and missing values at test time, not at runtime.
        Cli::command().debug_assert();
    }

    #[test]
    fn parses_global_locale_before_the_subcommand() {
        let cli = Cli::parse_from(["sofd", "--locale", "zh-CN", "info", "doc.ofd"]);
        assert_eq!(cli.locale.as_deref(), Some("zh-CN"));
        assert!(matches!(cli.command, Command::Info(_)));
    }

    #[test]
    fn parses_locale_after_the_subcommand_too() {
        let cli = Cli::parse_from(["sofd", "info", "doc.ofd", "--locale", "en-US"]);
        assert_eq!(cli.locale.as_deref(), Some("en-US"));
    }

    #[test]
    fn render_defaults_to_first_page_and_150_dpi() {
        let cli = Cli::parse_from(["sofd", "render", "doc.ofd"]);
        let Command::Render(args) = cli.command else {
            panic!("expected render");
        };
        assert_eq!(args.page, 0);
        assert_eq!(args.dpi, 150.0);
        assert!(args.output.is_none());
    }

    #[test]
    fn convert_defaults_to_pdf_and_every_page() {
        let cli = Cli::parse_from(["sofd", "convert", "doc.ofd"]);
        let Command::Convert(args) = cli.command else {
            panic!("expected convert");
        };
        assert_eq!(args.format, FormatArg::Pdf);
        assert_eq!(args.page, None);
    }

    #[test]
    fn convert_takes_a_page() {
        let cli = Cli::parse_from(["sofd", "convert", "doc.ofd", "-p", "2"]);
        let Command::Convert(args) = cli.command else {
            panic!("expected convert");
        };
        assert_eq!(args.page, Some(2));
    }

    #[test]
    fn print_parses_paper_and_copies() {
        let cli = Cli::parse_from(["sofd", "print", "doc.ofd", "--paper", "letter", "-c", "3"]);
        let Command::Print(args) = cli.command else {
            panic!("expected print");
        };
        assert_eq!(args.paper, PaperArg::Letter);
        assert_eq!(args.copies, 3);
    }

    #[test]
    fn search_takes_a_needle() {
        let cli = Cli::parse_from(["sofd", "search", "doc.ofd", "OFD"]);
        let Command::Search(args) = cli.command else {
            panic!("expected search");
        };
        assert_eq!(args.needle, "OFD");
        assert!(!args.case_sensitive);
    }
}
