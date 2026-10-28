//! What a scroll step costs with the cache answering it, against the render
//! it stands in for. The two are measured apart because the point of the
//! cache is the ratio: a lookup that costs a fraction of one render is what
//! makes scrolling smooth, and a lookup that costs most of one would not be
//! worth the memory.

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use sofd_reader::viewer::{WINDOW_MARGIN, window_for};
use sofd_reader::{PageCache, RenderedPage, Short};

/// The resolutions a windowed read happens at. 96 dpi fits an A4 whole, so
/// these are the zooms that ask for a part; 2048 is the weakest card's limit
/// and the one that makes a window smallest against the view.
const ZOOMS: &[f32] = &[2.0, 4.0, 8.0];

/// The texture limit the floor machine reports.
const LIMIT: u32 = 2048;

/// A4, and the view a 1200x800 window shows of it at a zoom.
const PAGE_MM: (f32, f32) = (210.0, 297.0);
const SCREEN_PT: (f64, f64) = (1200.0, 800.0);

/// One raster of the part `window` names. The pixels are not the point: what
/// is measured is the lookup, and allocating the image keeps the cache's own
/// accounting honest.
fn shot(side: usize, window: sofd_common::Rect) -> RenderedPage {
    RenderedPage {
        image: egui::ColorImage::new([side, side], vec![egui::Color32::WHITE; side * side]),
        size_mm: PAGE_MM,
        window: Some(window),
        short: Short::default(),
    }
}

/// How much of the page a reader sees at `zoom`, in millimetres.
fn view_at(zoom: f32) -> sofd_common::Rect {
    let mm = 25.4 / 72.0;
    sofd_common::Rect::new(
        0.0,
        0.0,
        (SCREEN_PT.0 / f64::from(zoom) * mm).min(f64::from(PAGE_MM.0)),
        (SCREEN_PT.1 / f64::from(zoom) * mm).min(f64::from(PAGE_MM.1)),
    )
}

fn scroll(c: &mut Criterion) {
    let mut group = c.benchmark_group("reader_scroll");
    for &zoom in ZOOMS {
        let dpi = 96.0 * zoom;
        let view = view_at(zoom);
        let window = window_for(view, PAGE_MM, dpi, LIMIT);
        // A raster of the window, sized as the limit gives it.
        let side = (window.w * f64::from(dpi) / 25.4).ceil() as usize;

        // A scroll that stays inside the raster: the answer the cache exists
        // to give. The view moves by a tenth of the margin, which is what a
        // wheel notch does.
        let step = sofd_common::Rect::new(
            view.x,
            view.y + window.h * (WINDOW_MARGIN - 1.0) / 4.0,
            view.w,
            view.h,
        );
        let wanted = window_for(step, PAGE_MM, dpi, LIMIT);

        let mut cache = PageCache::new(12);
        cache.insert(0, dpi, shot(side, window));
        group.throughput(Throughput::Elements(1));
        group.bench_with_input(
            BenchmarkId::new("hit", format!("{zoom}x")),
            &wanted,
            |b, wanted| {
                b.iter(|| cache.covering(0, dpi, Some(*wanted)).is_some());
            },
        );
    }
    group.finish();
}

criterion_group!(benches, scroll);
criterion_main!(benches);
