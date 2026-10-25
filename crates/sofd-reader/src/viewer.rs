//! Page rasterization and the small cache the viewer keeps.

use std::collections::HashMap;

use sofd::{Document, RenderOptions};

/// The resolution bounds the reader will rasterize at.
pub const MIN_DPI: f32 = 24.0;
/// The upper bound the reader asks for; the page size may cap it lower.
pub const MAX_DPI: f32 = 300.0;
/// The resolution a freshly opened document is shown at, which is the one a
/// desktop screen is assumed to have and the one Acrobat and browsers use.
pub const DEFAULT_DPI: f32 = 96.0;
/// Millimetres in an inch, the constant every conversion below divides by.
pub const MM_PER_INCH: f64 = 25.4;
/// Points in a millimetre: a point is 1/72 inch, so A4 is 595 by 842 points.
pub const POINTS_PER_MM: f32 = 72.0 / MM_PER_INCH as f32;
/// The zoom bounds, as the share of a page's 72 dpi size it is drawn at. A
/// point is 1/72 inch, so this is the page's size in points times up to three.
pub const MIN_ZOOM: f32 = MIN_DPI / 72.0;
/// The upper zoom, matching `MAX_DPI`.
pub const MAX_ZOOM: f32 = MAX_DPI / 72.0;
/// The zoom a document is opened at: its size at `DEFAULT_DPI`, so A4 opens
/// 794 points wide, which is what a 96 dpi screen shows an inch as.
pub const DEFAULT_ZOOM: f32 = DEFAULT_DPI / 72.0;

/// A rasterized page, decoded and ready to paint.
pub struct RenderedPage {
    /// The decoded image, in `egui` colour order.
    pub image: egui::ColorImage,
    /// The page's physical size in millimetres.
    pub size_mm: (f32, f32),
}

/// How pages are laid out in the window.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Layout {
    /// One page at a time.
    #[default]
    Single,
    /// Two pages side by side, as a spread.
    Facing,
    /// Every page in one scrolling column.
    Continuous,
}

impl Layout {
    /// The label shown in the menu, in the reader's own words.
    pub fn label(self) -> &'static str {
        match self {
            Self::Single => "reader-layout-single",
            Self::Facing => "reader-layout-double",
            Self::Continuous => "reader-layout-continuous",
        }
    }

    /// How far the toolbar's next/previous buttons move. A spread advances a
    /// spread: stepping one page would leave the reader on the back of it.
    pub fn stride(self) -> usize {
        match self {
            Self::Single => 1,
            Self::Facing => 2,
            Self::Continuous => 1,
        }
    }
}

/// How many pages a continuous view keeps around the current one. Only a
/// wanted page is rasterized, so "continuous" means enough context to scroll
/// through and no more, never the whole document.
pub const CONTINUOUS_WINDOW: usize = 3;

/// The pages a layout shows for a given current page and page count. Pure, so
/// the arrangement can be asserted without a window; an empty document yields
/// nothing and every index is clamped to the pages that exist.
pub fn visible_pages(layout: Layout, current: usize, count: usize) -> Vec<usize> {
    if count == 0 {
        return Vec::new();
    }
    let current = current.min(count - 1);
    match layout {
        Layout::Single => vec![current],
        // A spread is a fixed pair, so page 0 is always on the right of an
        // empty left half and the pairing stays stable as the reader moves.
        Layout::Facing => {
            let start = current - (current % 2);
            (start..=start + 1).filter(|page| *page < count).collect()
        }
        Layout::Continuous => {
            let first = current.saturating_sub(CONTINUOUS_WINDOW);
            let last = (current + CONTINUOUS_WINDOW).min(count - 1);
            (first..=last).collect()
        }
    }
}

/// Keeps the pages a reader has already rasterized, because rendering is the
/// expensive part of reading and a reader revisits the same page constantly.
/// The key carries the dpi: one page at two zooms is two entries.
pub struct PageCache {
    entries: HashMap<(usize, u32), RenderedPage>,
    order: Vec<(usize, u32)>,
    limit: usize,
}

impl PageCache {
    /// A cache holding at most `limit` rendered pages.
    pub fn new(limit: usize) -> Self {
        Self {
            entries: HashMap::new(),
            order: Vec::new(),
            limit: limit.max(1),
        }
    }

    /// The cached page for this index and dpi, if it was rendered already.
    pub fn get(&self, page: usize, dpi: f32) -> Option<&RenderedPage> {
        self.entries.get(&(page, dpi.to_bits()))
    }

    /// Store a rendered page, evicting the oldest entry when full.
    pub fn insert(&mut self, page: usize, dpi: f32, rendered: RenderedPage) {
        let key = (page, dpi.to_bits());
        if self.entries.insert(key, rendered).is_none() {
            self.order.push(key);
        }
        while self.order.len() > self.limit {
            let oldest = self.order.remove(0);
            self.entries.remove(&oldest);
        }
    }

    /// Drop every entry; used when a different document is opened.
    pub fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
    }

    /// How many pages are currently held.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether the cache holds nothing.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// How close to white every pixel has to be before a page counts as blank.
const BLANK_THRESHOLD: u8 = 8;

/// A file the platform handed to the window, as a drop or a paste.
///
/// Desktops differ: some deliver a path, others only the bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DroppedFile {
    /// A path on disk the reader may read.
    Path(std::path::PathBuf),
    /// The file contents, with a name for display.
    Bytes {
        /// The file name, if the platform reported one.
        name: String,
        /// The whole file.
        bytes: Vec<u8>,
    },
}

impl DroppedFile {
    /// The file name, if there is one to show.
    pub fn name(&self) -> Option<String> {
        match self {
            Self::Path(path) => path.file_name().map(|n| n.to_string_lossy().into_owned()),
            Self::Bytes { name, .. } => Some(name.clone()),
        }
    }
}

/// Whether a rendered page carries no visible ink.
///
/// It reads the raster, because a page may hold nothing but a background.
pub fn looks_blank(image: &egui::ColorImage) -> bool {
    let pixels = image.pixels.as_slice();
    !pixels.is_empty()
        && pixels.iter().all(|p| {
            p.r() >= 255 - BLANK_THRESHOLD
                && p.g() >= 255 - BLANK_THRESHOLD
                && p.b() >= 255 - BLANK_THRESHOLD
        })
}

/// The largest texture `egui` will accept, matched to the smallest limit a
/// desktop driver imposes; a larger page is refused, so the resolution is
/// reduced in `render_page` until both sides fit.
pub const MAX_TEXTURE_SIDE: u32 = 2048;

/// The resolution a page can be rasterized at within the texture limit. A zoom
/// request is only a request: A4 at 300 dpi is 3,480 px wide, so the ceiling
/// here is arithmetic rather than preference.
pub fn dpi_that_fits(size_mm: (f32, f32)) -> f32 {
    let longest = size_mm.0.max(size_mm.1).max(1.0);
    let fit = f64::from(MAX_TEXTURE_SIDE) * MM_PER_INCH / f64::from(longest);
    (fit as f32).clamp(MIN_DPI, MAX_DPI)
}

/// The zoom that fits a page into an area, for a fit mode.
/// `available` is the area in points and `size_mm` the page's physical size; a
/// fit is clamped to the reader's range, so it is still only ever a request.
pub fn fit_zoom(fit: crate::app::Fit, size_mm: (f32, f32), available: (f32, f32)) -> f32 {
    use crate::app::Fit;
    // The page's size in points is what a zoom of one draws it at, so the
    // area divided by that is the zoom that fills it.
    let page_points = (
        size_mm.0.max(1.0) * POINTS_PER_MM,
        size_mm.1.max(1.0) * POINTS_PER_MM,
    );
    let area = (available.0.max(1.0), available.1.max(1.0));
    let by_side = match fit {
        Fit::Width => area.0 / page_points.0,
        Fit::Page => (area.0 / page_points.0).min(area.1 / page_points.1),
    };
    by_side
        .clamp(MIN_ZOOM, MAX_ZOOM)
        .min(fit_zoom_that_fits(size_mm))
}

/// The zoom a page can be drawn at and still be rasterized sharply, which is
/// what `dpi_that_fits` permits, expressed as the zoom it implies.
pub fn fit_zoom_that_fits(size_mm: (f32, f32)) -> f32 {
    dpi_that_fits(size_mm) / 72.0
}

/// Rasterize one page, within what the texture limit can hold. Past the limit
/// a whole page will not fit, so a reader zoomed in further asks for a part.
pub fn render_page(
    doc: &Document,
    page: usize,
    dpi: f32,
) -> Result<RenderedPage, sofd_error::OfdError> {
    render_page_at(doc, page, dpi, None)
}

/// Rasterize the part of one page a reader can see, at the resolution its zoom
/// asks for. `window` is the visible area in millimetres; a page small enough
/// to fit the texture limit is rasterized whole, whatever the window says.
pub fn render_page_at(
    doc: &Document,
    page: usize,
    dpi: f32,
    window: Option<sofd_common::Rect>,
) -> Result<RenderedPage, sofd_error::OfdError> {
    render_page_with_seals(doc, page, dpi, &[], window)
}

/// Rasterize one page and stamp any seals whose page it is onto the raster.
/// The seals arrive resolved, so a caller with none, such as the export path,
/// passes an empty list for the plain page and stamping is a scale and paste.
pub fn render_page_with_seals(
    doc: &Document,
    page: usize,
    dpi: f32,
    seals: &[sofd::Seal],
    window: Option<sofd_common::Rect>,
) -> Result<RenderedPage, sofd_error::OfdError> {
    let size_mm = doc
        .page(page)
        .map(|p| (p.page_box.w as f32, p.page_box.h as f32))
        .unwrap_or((210.0, 297.0));
    let wanted = dpi.clamp(MIN_DPI, MAX_DPI);
    // Past the limit a whole page cannot be held, but the part on screen can,
    // so the zoom is kept and the window narrowed rather than the zoom cut.
    let whole = wanted.min(dpi_that_fits(size_mm));
    let (effective, window) = if whole < wanted {
        (wanted, window.map(|w| narrow(w, wanted)))
    } else {
        (whole, None)
    };
    let options = RenderOptions {
        dpi: f64::from(effective),
        window,
        ..RenderOptions::default()
    };
    let png = doc.to_png(page, &options)?;
    let decoded =
        image::load_from_memory_with_format(&png, image::ImageFormat::Png).map_err(|reason| {
            sofd_error::OfdError::Render(sofd_error::RenderError::ImageEncode {
                page,
                reason: reason.to_string(),
            })
        })?;
    let mut rgba = decoded.to_rgba8();
    for seal in seals.iter().filter(|s| s.page == page) {
        stamp_seal(&mut rgba, seal, f64::from(effective), options.window);
    }
    let image = egui::ColorImage::from_rgba_unmultiplied(
        [rgba.width() as usize, rgba.height() as usize],
        rgba.as_raw(),
    );
    Ok(RenderedPage { image, size_mm })
}

/// Shrink a visible area until its raster fits the texture limit, keeping its
/// top left corner: a reader scrolled to a corner keeps that corner.
fn narrow(window: sofd_common::Rect, dpi: f32) -> sofd_common::Rect {
    let most = f64::from(MAX_TEXTURE_SIDE) / (f64::from(dpi) / MM_PER_INCH);
    sofd_common::Rect::new(window.x, window.y, window.w.min(most), window.h.min(most))
}

/// Composite one seal onto the page raster, scaled to its boundary. A seal is
/// drawn, not laid out: GB/T 38540 keeps it inside the signature, as a raster
/// image or as a whole OFD document, and the second case is a nested render
/// because its positions, rotations and typeface travel in the document. A seal
/// that will not decode is skipped, as a missing image resource is.
fn stamp_seal(
    raster: &mut image::RgbaImage,
    seal: &sofd::Seal,
    dpi: f64,
    window: Option<sofd_common::Rect>,
) {
    let Some(drawn) = draw_seal(seal, dpi) else {
        return;
    };
    let mm = dpi / MM_PER_INCH;
    // A window starts the raster at its own corner, so the seal moves back by
    // as much as the window did.
    let origin = window.map_or((0.0, 0.0), |w| (w.x, w.y));
    let box_ = seal.boundary;
    let (dx, dy) = (
        ((box_.x - origin.0) * mm).round() as i64,
        ((box_.y - origin.1) * mm).round() as i64,
    );
    let (dw, dh) = ((box_.w * mm).round() as i64, (box_.h * mm).round() as i64);
    if dw <= 0 || dh <= 0 {
        return;
    }
    let scaled = drawn
        .resize_exact(dw as u32, dh as u32, image::imageops::FilterType::Triangle)
        .to_rgba8();
    image::imageops::overlay(raster, &scaled, dx, dy);
}

/// A seal's appearance as an image, whichever way the signature states it.
fn draw_seal(seal: &sofd::Seal, dpi: f64) -> Option<image::DynamicImage> {
    if let Some(sealed) = &seal.sealed {
        // Drawn at the page's resolution, so it stays sharp at every zoom. The
        // picture states its size in millimetres, which is what makes it a seal
        // rather than a picture of one.
        if sealed.kind.eq_ignore_ascii_case("ofd") {
            let inner = sofd::Document::open_bytes(&sealed.data).ok()?;
            // Only one page is asked for: a seal document holds exactly one,
            // and rendering whatever else it might hold would paste the wrong
            // thing over the invoice.
            let options = RenderOptions {
                dpi: dpi_at(dpi),
                ..RenderOptions::default()
            };
            let png = inner.to_png(0, &options).ok()?;
            return image::load_from_memory_with_format(&png, image::ImageFormat::Png).ok();
        }
        return image::load_from_memory(&sealed.data).ok();
    }
    if seal.image.is_empty() {
        return None;
    }
    image::load_from_memory(&seal.image).ok()
}

/// The DPI a seal document is drawn at: the page's own, so the nested render
/// arrives at the seal's stated millimetre size and the paste below scales it to
/// the annotation's box by construction rather than by a factor computed here.
fn dpi_at(wanted: f64) -> f64 {
    wanted.clamp(f64::from(MIN_DPI), f64::from(MAX_DPI))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The invoice in the fixtures carries its seal the way GB/T 38540 says to:
    /// inside the signed value, as a nested OFD document, with no `Seal`
    /// element anywhere for the descriptor to point at. Rendering it therefore
    /// has to go through the nested document, and this is the test that says so
    /// — a reader that only looked for `Seal` drew nothing on this file for a
    /// long time, and the page looked complete without it.
    #[test]
    fn an_invoice_seal_is_read_from_inside_the_signature() {
        let bytes = fixture("tests/fixtures/ofdrw/999.ofd");
        let Some(bytes) = bytes else {
            return; // The fixture is not shipped with every build.
        };
        let mut doc = sofd::Document::open_bytes(&bytes).expect("the fixture opens");
        let seals = doc.seals().expect("the seals read");
        assert_eq!(seals.len(), 1, "the invoice carries one seal");
        let seal = &seals[0];
        let sealed = seal
            .sealed
            .as_ref()
            .expect("the seal travels inside the signature");
        assert_eq!(sealed.kind, "ofd", "the seal is a nested document");
        assert_eq!(
            (sealed.width, sealed.height),
            (30, 20),
            "the size notice 2018 no. 41 states, in millimetres"
        );
        assert!(
            sealed.data.starts_with(b"PK"),
            "a nested document, not an image"
        );
        // The annotation puts it where the issuer put it: 30 by 20 mm at 90, 8.
        assert_eq!(seal.boundary.w, 30.0);
        assert_eq!(seal.boundary.h, 20.0);
    }

    /// The seal draws in red and covers the box it was placed in. The page has
    /// a white background, so red pixels inside that box are the seal and
    /// nothing else is.
    #[test]
    fn the_seal_reaches_the_page() {
        let Some(bytes) = fixture("tests/fixtures/ofdrw/999.ofd") else {
            return;
        };
        let mut doc = sofd::Document::open_bytes(&bytes).expect("the fixture opens");
        let seals = doc.seals().expect("the seals read");
        let page = seals[0].page;
        let drawn =
            render_page_with_seals(&doc, page, 96.0, &seals, None).expect("the page renders");
        let mm = 96.0 / MM_PER_INCH;
        let (x0, y0) = (
            (seals[0].boundary.x * mm) as usize,
            (seals[0].boundary.y * mm) as usize,
        );
        let (x1, y1) = (
            ((seals[0].boundary.x + seals[0].boundary.w) * mm) as usize,
            ((seals[0].boundary.y + seals[0].boundary.h) * mm) as usize,
        );
        let mut red = 0;
        let mut total = 0;
        for y in y0..y1.min(drawn.image.height()) {
            for x in x0..x1.min(drawn.image.width()) {
                let pixel = drawn.image.pixels[y * drawn.image.width() + x];
                total += 1;
                if pixel.r() > 150 && pixel.g() < 120 && pixel.b() < 120 {
                    red += 1;
                }
            }
        }
        assert!(total > 0, "the seal box is on the page");
        // A ring plus three lines of text is a fifth of the box, not a trace and
        // not a filled rectangle: both extremes would mean the wrong thing was
        // pasted.
        let share = red as f64 / total as f64;
        assert!(
            share > 0.05 && share < 0.6,
            "red covers {:.1}% of the seal box",
            share * 100.0
        );
    }

    /// Read a fixture from the workspace root, which is where `cargo test` runs
    /// a package's tests from.
    fn fixture(path: &str) -> Option<Vec<u8>> {
        let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()?
            .parent()?;
        std::fs::read(root.join(path)).ok()
    }
}
