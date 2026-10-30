#![no_main]

//! The PDF writer takes geometry from the document, so this target fuzzes the
//! display list: a page's numbers must yield PDF bytes or an error. The target
//! fuzzes one page, because a page is what this crate converts. It keeps
//! no seed inputs, because any eight bytes reach the writer already.

use libfuzzer_sys::fuzz_target;
use sofd_common::Rect;
use sofd_convert::PdfOptions;
use sofd_display::ops::{Color, Paint, PaintRule, StrokeStyle};
use sofd_display::{DrawOp, PageDisplay};
use sofd_geometry::Path;

fn coordinate(raw: &u8) -> f64 {
    // A spread wide enough to reach denormals, huge values and negative ones.
    (*raw as f64 - 128.0) * 1.7e4
}

fuzz_target!(|data: &[u8]| {
    if data.len() < 8 {
        return;
    }
    let path = Path::build(|b| {
        b.move_to(coordinate(&data[0]), coordinate(&data[1]));
        b.line_to(coordinate(&data[2]), coordinate(&data[3]));
        b.close();
    });
    let page = PageDisplay {
        index: 0,
        page_box: Rect::new(0.0, 0.0, 210.0, 297.0),
        ops: vec![DrawOp::Path {
            path,
            fill: Some(Paint::Solid(Color::rgba(data[4], data[5], data[6], data[7]))),
            stroke: None,
            stroke_style: StrokeStyle::default(),
            rule: PaintRule::NonZero,
        }],
        content_bbox: Rect::EMPTY,
    };
    let _ = sofd_convert::page_to_pdf(&page, &PdfOptions::default());
});
