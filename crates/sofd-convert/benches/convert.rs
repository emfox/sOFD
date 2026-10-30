//! What it costs to turn a flattened page into PNG, SVG and PDF.
//! A reader spends most of its time here on an opened document, so the three
//! backends are measured apart: they share the display list and nothing else.

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use sofd_convert::png::page_to_png;
use sofd_convert::{PdfOptions, SvgOptions, TextExport, page_to_pdf, page_to_svg};
use sofd_display::PageDisplay;
use sofd_font::FontManager;
use sofd_io::SliceSource;
use sofd_package::Package;
use sofd_parser::Parser;
use sofd_render::RenderOptions;
use sofd_testkit::{OBJECTS_PER_PAGE, bench_archive};

/// Page counts to measure at. One page is a preview, eight is a document
/// somebody would export, and twenty is the packet the budgets are stated for.
const PAGES: &[usize] = &[1, 8, 20];

/// The first page of a document, flattened, with the faces it draws from.
///
/// This crate converts one page and never opens a document, so the benchmark
/// parses one directly rather than going through the facade. The measurement is
/// the conversion; reaching the page is setup, and reaching it through the
/// facade would make this crate depend on the layer above it — the very edge
/// this crate no longer has.
fn fixture(pages: usize) -> (PageDisplay, FontManager) {
    let bytes = bench_archive(pages);
    let source = SliceSource::new(&bytes, "benchmark document");
    let mut package = Package::open(&source).expect("open the benchmark archive");
    let mut parser = Parser::new(&mut package);
    let parsed = parser
        .open_document()
        .expect("parse the benchmark document");
    let page = parser
        .page_display(&parsed, 0)
        .expect("flatten the first page");
    let mut fonts = FontManager::new();
    // The page declares Orbitron. Without it every run would be measured as a
    // drop rather than as the shaping work it is.
    let table = sofd_parser::fonts::load_fonts(&mut package, &parsed.doc_dir).expect("load fonts");
    for resource in table.values().filter(|r| r.embedded) {
        fonts.registry_mut().register_embedded(
            resource.path.clone(),
            resource.family.clone(),
            resource.data.clone(),
        );
    }
    fonts.build_fallback_chain();
    (page, fonts)
}

fn converting(c: &mut Criterion) {
    let mut group = c.benchmark_group("convert");
    for &pages in PAGES {
        let (page, fonts) = fixture(pages);
        // The throughput unit is objects, so the three backends are
        // comparable on the same document.
        group.throughput(Throughput::Elements((pages * OBJECTS_PER_PAGE) as u64));

        group.bench_with_input(BenchmarkId::new("png", pages), &(), |b, ()| {
            let options = RenderOptions::default();
            b.iter(|| page_to_png(&page, &fonts, &options).expect("png"));
        });

        group.bench_with_input(BenchmarkId::new("svg", pages), &(), |b, ()| {
            let options = SvgOptions {
                text: TextExport::LiveText,
                fonts: None,
            };
            b.iter(|| page_to_svg(&page, &options).expect("svg"));
        });

        group.bench_with_input(BenchmarkId::new("pdf", pages), &(), |b, ()| {
            let options = PdfOptions {
                text: TextExport::LiveText,
                fonts: None,
            };
            b.iter(|| page_to_pdf(&page, &options).expect("pdf"));
        });
    }
    group.finish();
}

criterion_group!(benches, converting);
criterion_main!(benches);
