//! What a page costs in memory, not in time: the budget in `Performance.md`
//! states a peak per page and per document, and a timing cannot say whether the
//! cache that holds the pages fits. Bytes are counted by the allocator below.

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use sofd::Document;
use sofd_convert::png::page_to_png;
use sofd_render::RenderOptions;
use sofd_testkit::bench_archive;
use std::alloc::{GlobalAlloc, Layout, System};
use std::io::Write;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

/// The allocator, wrapped so a measurement can read how much is live.
struct Counting;

/// Bytes currently allocated minus bytes freed, across every thread.
static LIVE: AtomicUsize = AtomicUsize::new(0);
/// The fewest live bytes seen since the last reset, which is the baseline the
/// peak is measured against.
static FLOOR: AtomicUsize = AtomicUsize::new(usize::MAX);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let pointer = unsafe { System.alloc(layout) };
        if !pointer.is_null() {
            LIVE.fetch_add(layout.size(), Ordering::Relaxed);
        }
        pointer
    }

    unsafe fn dealloc(&self, pointer: *mut u8, layout: Layout) {
        unsafe { System.dealloc(pointer, layout) };
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
    }

    unsafe fn realloc(&self, pointer: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let moved = unsafe { System.realloc(pointer, layout, new_size) };
        if !moved.is_null() {
            if new_size >= layout.size() {
                LIVE.fetch_add(new_size - layout.size(), Ordering::Relaxed);
            } else {
                LIVE.fetch_sub(layout.size() - new_size, Ordering::Relaxed);
            }
        }
        moved
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

/// Forget every reading, so the next peak is measured from here.
fn reset() {
    FLOOR.store(LIVE.load(Ordering::Relaxed), Ordering::Relaxed);
}

/// The bytes live now, less the floor, which is what this measurement added
/// and is holding. A reading cannot be taken from inside criterion's loop,
/// because the allocation it would make is itself a measurement artefact.
fn held_since_reset() -> usize {
    LIVE.load(Ordering::Relaxed)
        .saturating_sub(FLOOR.load(Ordering::Relaxed))
}

/// The file a reading is kept in, beside criterion's own output.
const READINGS: &str = "memory-readings.log";

/// Page counts the peak is measured at: one page is the budget's unit, twenty
/// is the packet the per-document ceiling is stated for.
const PAGES: &[usize] = &[1, 20];

fn memory(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory");
    // Three samples: this measures a ceiling, and a ceiling does not need a
    // distribution around it the way a timing does.
    group.sample_size(10);
    for &pages in PAGES {
        let bytes = bench_archive(pages);
        let doc = Document::open_bytes(&bytes).expect("benchmark document");
        let options = RenderOptions::default();
        let fonts = doc.fonts();
        let page = doc.page(pages - 1).expect("last page");
        // Read before the loop: the raster is made once, outside criterion, so
        // the reading cannot be disturbed by the loop's own allocations. It is
        // printed rather than counted, and the gate reads the printout.
        reset();
        let png = page_to_png(page, fonts, &options).expect("png");
        report(pages, held_since_reset(), png.len());
        let held = png.len();
        group.bench_with_input(BenchmarkId::new("page_raster", pages), &held, |b, held| {
            b.iter(|| std::hint::black_box(*held))
        });
    }
    group.finish();
}

/// Publish one reading under a stable name, in the log and to stdout. The
/// decimal point and unit are deliberate: criterion reads every printed number
/// as a benchmark value, and a bare integer would become one more timing.
fn report(pages: usize, held: usize, png: usize) {
    let line = format!(
        "memory-held: page_raster/{pages} {:.1} MB ({png} byte png)",
        mb(held)
    );
    println!("{line}");
    let keep = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/criterion");
    if keep.is_dir() {
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(keep.join(READINGS))
            .expect("readings file");
        writeln!(file, "{line}").expect("append a reading");
    }
}

/// Bytes as mebibytes, the unit the budget is stated in.
fn mb(bytes: usize) -> f64 {
    bytes as f64 / (1024.0 * 1024.0)
}

criterion_group!(benches, memory);
criterion_main!(benches);
