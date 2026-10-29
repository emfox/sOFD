//! The engine behind a canvas a caller brings: a page as geometry rather than
//! as a bitmap, so a zoom is a transform instead of another render. A Tauri
//! window, a Qt painter or an SVG writer each walk these shapes and hand them
//! to a canvas of their own; this one only reports what it walked.
//!
//! `cargo run -p sofd-cli --example canvas_shell -- <file.ofd>`

use sofd::{Document, RenderOptions, Shape};
use sofd_io::FileSource;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let file = std::env::args()
        .nth(1)
        .ok_or("usage: canvas_shell <file.ofd>")?;
    let source = FileSource::open(&file)?;
    let doc = Document::open(&source)?;
    let pages = doc.page_count();
    println!("{file}: {pages} page(s)");

    // The options name a resolution because that is what a raster needs. A
    // vector page is in millimetres, so only `window` means anything here.
    let options = RenderOptions::default();
    let mut total = Counts::default();
    for index in 0..pages {
        let page = doc.vector_page(index, &options)?;
        let mut counts = Counts::default();
        for shape in &page.shapes {
            match shape {
                Shape::Path {
                    path, fill, stroke, ..
                } => {
                    counts.paths += 1;
                    if fill.is_some() {
                        counts.fills += 1;
                    }
                    if stroke.is_some() {
                        counts.strokes += 1;
                    }
                    counts.segments += path.len();
                }
                Shape::Text { path, fill, .. } => {
                    counts.texts += 1;
                    if fill.is_some() {
                        counts.fills += 1;
                    }
                    counts.segments += path.len();
                }
                Shape::Clip { .. } => counts.clips += 1,
                Shape::ClipReset => counts.resets += 1,
            }
        }
        println!(
            "  page {index}: {:.1} x {:.1} mm, {} shape(s), {} segment(s)",
            page.page_box.w,
            page.page_box.h,
            counts.paths + counts.texts,
            counts.segments
        );
        total.add(&counts);
    }
    println!(
        "{} path(s), {} text run(s), {} fill(s), {} stroke(s), {} clip(s) in {} scope(s)",
        total.paths, total.texts, total.fills, total.strokes, total.clips, total.resets
    );
    Ok(())
}

/// What one page, or the whole document, walks.
#[derive(Default)]
struct Counts {
    paths: usize,
    texts: usize,
    fills: usize,
    strokes: usize,
    segments: usize,
    clips: usize,
    resets: usize,
}

impl Counts {
    fn add(&mut self, other: &Counts) {
        self.paths += other.paths;
        self.texts += other.texts;
        self.fills += other.fills;
        self.strokes += other.strokes;
        self.segments += other.segments;
        self.clips += other.clips;
        self.resets += other.resets;
    }
}
