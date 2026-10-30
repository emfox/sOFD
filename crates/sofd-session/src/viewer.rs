//! Page rasterization and the small cache the viewer keeps.

use std::collections::HashMap;

use sofd::{Document, RenderOptions};

/// The resolution bounds the reader will rasterize at.
pub const MIN_DPI: f32 = 24.0;
/// The upper bound the reader asks for; the page size may cap it lower. An OFD
/// is a page description, not a scan: its text is outlines and its rules are
/// vectors, so there is detail at any magnification, and 300 dpi stopped a
/// reader at 417% for no reason the document gave. The texture is the real
/// limit, and it is handled by rasterizing the part on screen.
pub const MAX_DPI: f32 = 1200.0;
/// The resolution a freshly opened document is shown at, which is the one a
/// desktop screen is assumed to have and the one Acrobat and browsers use.
pub const DEFAULT_DPI: f32 = 96.0;
/// Millimetres in an inch, the constant every conversion below divides by.
pub const MM_PER_INCH: f64 = 25.4;
/// Points in a millimetre: a point is 1/72 inch, so A4 is 595 by 842 points.
pub const POINTS_PER_MM: f32 = 72.0 / MM_PER_INCH as f32;
/// The zoom bounds, as the share of a page's 72 dpi size it is drawn at. A
/// point is 1/72 inch, so a zoom of one draws the page at its size in points.
pub const MIN_ZOOM: f32 = MIN_DPI / 72.0;
/// The upper zoom, matching `MAX_DPI`.
pub const MAX_ZOOM: f32 = MAX_DPI / 72.0;
/// The zoom a document is opened at: its size at `DEFAULT_DPI`, so A4 opens
/// 794 points wide, which is what a 96 dpi screen shows an inch as.
pub const DEFAULT_ZOOM: f32 = DEFAULT_DPI / 72.0;

/// What a page was short of when it was rasterized.
///
/// A page draws what it can and keeps going, so a document with a font it
/// cannot find still shows the marks that font was not needed for. The counts
/// turn "this page looks wrong" into "this page is missing three glyphs".
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Short {
    /// Glyphs the page meant to draw and could not, because the face it found
    /// did not carry the character and no substitute did either.
    pub glyphs: usize,
    /// Runs whose face could not be loaded at all, so their text is absent.
    pub font_runs: usize,
    /// Images whose bytes would not decode, so their place is blank.
    pub images: usize,
}

impl Short {
    /// Whether the page drew everything it meant to.
    pub fn is_empty(&self) -> bool {
        self.glyphs == 0 && self.font_runs == 0 && self.images == 0
    }
}

/// A rasterized page, decoded and ready to paint.
///
/// Straight rather than premultiplied RGBA8, row-major from the top left, which
/// is what a texture upload and an encoder both take. It is this crate's own
/// type so the state that holds it does not need a particular toolkit to exist.
#[derive(Clone)]
pub struct Rgba {
    /// Width in pixels.
    pub width: u32,
    /// Height in pixels.
    pub height: u32,
    /// Four bytes per pixel, straight alpha, row-major.
    pub bytes: Vec<u8>,
}

impl Rgba {
    /// The size in a toolkit's own order, which is what a texture takes.
    pub fn size(&self) -> [usize; 2] {
        [self.width as usize, self.height as usize]
    }

    /// One pixel, or white outside the image.
    pub fn pixel(&self, x: u32, y: u32) -> [u8; 4] {
        if x >= self.width || y >= self.height {
            return [255, 255, 255, 255];
        }
        let base = ((y * self.width + x) * 4) as usize;
        [
            self.bytes[base],
            self.bytes[base + 1],
            self.bytes[base + 2],
            self.bytes[base + 3],
        ]
    }
}

/// A rasterized page, decoded and ready to paint.
pub struct RenderedPage {
    /// The decoded image.
    pub pixels: Rgba,
    /// The page's physical size in millimetres.
    pub size_mm: (f32, f32),
    /// The part of the page this image covers, in millimetres, or `None` for
    /// the whole page. A zoom past the texture limit rasterizes only what is
    /// on screen, and the canvas has to know which part it got: drawing it as
    /// if it were the page stretched the content and moved every mark.
    pub window: Option<sofd_common::Rect>,
    /// What the page was short of, for the reader to report.
    pub short: Short,
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
/// The key carries the dpi and the window: one page at two zooms is two
/// entries. A count of pages is not a budget, because one raster's size is the
/// zoom's to decide: the same twelve pages can be 200 MB or three gigabytes.
pub struct PageCache {
    /// The window is part of the key, in millimetres rounded to the micrometre:
    /// two requests a thousandth of a millimetre apart are the same request,
    /// and the pixel they would differ by is below what a raster can show.
    entries: HashMap<Key, RenderedPage>,
    order: Vec<Key>,
    limit: usize,
    /// Pixels held across every entry, so eviction answers to area.
    pixels: usize,
    pixel_budget: usize,
}

/// What one cached raster answers to. `None` is a whole page.
type Key = (usize, u32, Option<(i64, i64, i64, i64)>);

/// The cache key a window gives, in micrometres of page space.
fn key_of(page: usize, dpi: f32, window: Option<sofd_common::Rect>) -> Key {
    let mm = |v: f64| (v * 1000.0).round() as i64;
    (
        page,
        dpi.to_bits(),
        window.map(|w| (mm(w.x), mm(w.y), mm(w.w), mm(w.h))),
    )
}

/// How much raster the cache may hold, in pixels. 64 M is 256 MB of `Color32`:
/// a dozen pages at the floor texture size, or one at the largest.
pub const CACHE_PIXEL_BUDGET: usize = 64 * 1024 * 1024;

/// The pixels one raster costs, which is what the cache budgets in.
fn area(rendered: &RenderedPage) -> usize {
    rendered.pixels.width as usize * rendered.pixels.height as usize
}

impl PageCache {
    /// A cache holding at most `limit` rendered pages.
    pub fn new(limit: usize) -> Self {
        Self::with_budget(limit, CACHE_PIXEL_BUDGET)
    }

    /// The same, with the pixel budget stated. A test says what it needs.
    pub fn with_budget(limit: usize, pixel_budget: usize) -> Self {
        Self {
            entries: HashMap::new(),
            order: Vec::new(),
            limit: limit.max(1),
            pixels: 0,
            pixel_budget: pixel_budget.max(1),
        }
    }

    /// The cached page for this index and dpi, if it was rendered already.
    /// A page asked for whole is looked up as one; a window is a different
    /// entry, so this returns the whole page rather than any part of it.
    pub fn get(&self, page: usize, dpi: f32) -> Option<&RenderedPage> {
        self.entries.get(&key_of(page, dpi, None))
    }

    /// A cached raster of this page at this dpi that covers `wanted`, if the
    /// cache holds one. Scrolling asks for a new window every step while the
    /// raster in hand is still the answer, so the question is asked by area:
    /// the exact window first, then any other held for this page and zoom.
    pub fn covering(
        &self,
        page: usize,
        dpi: f32,
        wanted: Option<sofd_common::Rect>,
    ) -> Option<&RenderedPage> {
        if let Some(exact) = self.entries.get(&key_of(page, dpi, wanted)) {
            return Some(exact);
        }
        let bits = dpi.to_bits();
        let mut best: Option<&RenderedPage> = None;
        for (key, held) in &self.entries {
            if key.0 != page || key.1 != bits {
                continue;
            }
            let fits = match wanted {
                None => held.window.is_none(),
                Some(area) => covers(held.window, area),
            };
            if !fits {
                continue;
            }
            // The smallest that fits is kept: it is the least memory to hold
            // for the same answer, and the next scroll is likelier to miss it.
            if best.is_none_or(|kept| area(held) < area(kept)) {
                best = Some(held);
            }
        }
        best
    }

    /// Store a rendered page, evicting the oldest entry until the count and
    /// the pixel budget both hold. The newest entry is never the one evicted:
    /// a page too large for the budget is still the one being looked at.
    pub fn insert(&mut self, page: usize, dpi: f32, rendered: RenderedPage) {
        let key = key_of(page, dpi, rendered.window);
        let pixels = area(&rendered);
        if let Some(previous) = self.entries.insert(key, rendered) {
            // An entry the cache already held: its pixels were counted, and
            // only the difference is new.
            self.pixels = self.pixels.saturating_sub(area(&previous));
        } else {
            self.order.push(key);
        }
        self.pixels += pixels;
        while self.order.len() > self.limit
            || (self.pixels > self.pixel_budget && self.order.len() > 1)
        {
            let oldest = self.order.remove(0);
            if let Some(evicted) = self.entries.remove(&oldest) {
                self.pixels = self.pixels.saturating_sub(area(&evicted));
            }
        }
    }

    /// The pixels the cache is holding, which is what the budget counts.
    pub fn pixels(&self) -> usize {
        self.pixels
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
pub fn looks_blank(image: &Rgba) -> bool {
    // `as_chunks` rather than `chunks_exact`: a pixel is four bytes by
    // construction, so the remainder is empty and saying so is the point.
    let (pixels, _) = image.bytes.as_chunks::<4>();
    !pixels.is_empty()
        && pixels.iter().all(|p| {
            p[0] >= 255 - BLANK_THRESHOLD
                && p[1] >= 255 - BLANK_THRESHOLD
                && p[2] >= 255 - BLANK_THRESHOLD
        })
}

/// The largest texture this crate assumes, matched to the smallest limit a
/// desktop driver imposes. This is the floor, not the answer: the backend
/// reports what it can really hold in the toolkit's input each frame, and `App`
/// passes that down, so a driver offering 8,192 is not held to a number
/// chosen for the weakest board. Every function here takes the limit as an
/// argument for that reason.
pub const MAX_TEXTURE_SIDE: u32 = 2048;

/// The resolution a page can be rasterized within a texture limit. A zoom is
/// only a request: A4 at 300 dpi is 3,480 px wide, so this is arithmetic.
///
/// The result is held a hair under what the limit allows, because pixel sizes
/// round up: a resolution landing exactly on the limit comes back one pixel too
/// wide, and the toolkit refuses the texture.
pub fn dpi_within(size_mm: (f32, f32), limit: u32) -> f32 {
    let longest = size_mm.0.max(size_mm.1).max(1.0);
    let fit = f64::from(limit.max(1)) * MM_PER_INCH / f64::from(longest);
    let safe = fit * (1.0 - LIMIT_MARGIN);
    (safe as f32).clamp(MIN_DPI, MAX_DPI)
}

/// The same at the floor this crate assumes when nobody has said otherwise.
pub fn dpi_that_fits(size_mm: (f32, f32)) -> f32 {
    dpi_within(size_mm, MAX_TEXTURE_SIDE)
}

/// How far under the texture limit a raster is held. A part in ten thousand
/// costs a fifth of a pixel at 2,048 and keeps rounding from crossing the line.
const LIMIT_MARGIN: f64 = 1e-4;

/// The zoom that fits a page into an area, for a fit mode.
/// `available` is the area in points and `size_mm` the page's physical size; a
/// fit is clamped to the reader's range, so it is still only ever a request.
pub fn fit_zoom(
    fit: crate::app::Fit,
    size_mm: (f32, f32),
    available: (f32, f32),
    limit: u32,
) -> f32 {
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
        .min(fit_zoom_that_fits(size_mm, limit))
}

/// The zoom a page can be drawn at and still be rasterized sharply, which is
/// what `dpi_within` permits, expressed as the zoom it implies.
pub fn fit_zoom_that_fits(size_mm: (f32, f32), limit: u32) -> f32 {
    dpi_within(size_mm, limit) / 72.0
}

/// Rasterize one page, within what the texture limit can hold. Past the limit
/// a whole page will not fit, so a reader zoomed in further asks for a part.
pub fn render_page(
    doc: &Document,
    page: usize,
    dpi: f32,
) -> Result<RenderedPage, sofd_error::OfdError> {
    render_page_at(doc, page, dpi, None, MAX_TEXTURE_SIDE)
}

/// Rasterize the part of one page a reader can see, at the resolution its zoom
/// asks for. `window` is the visible area in millimetres; a page small enough
/// to fit the texture limit is rasterized whole, whatever the window says.
pub fn render_page_at(
    doc: &Document,
    page: usize,
    dpi: f32,
    window: Option<sofd_common::Rect>,
    limit: u32,
) -> Result<RenderedPage, sofd_error::OfdError> {
    render_page_with_seals(doc, page, dpi, &[], window, limit)
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
    limit: u32,
) -> Result<RenderedPage, sofd_error::OfdError> {
    let size_mm = doc
        .page(page)
        .map(|p| (p.page_box.w as f32, p.page_box.h as f32))
        .unwrap_or((210.0, 297.0));
    let wanted = dpi.clamp(MIN_DPI, MAX_DPI);
    // Past the limit a whole page cannot be held but the part on screen can, so
    // the zoom is kept and the window narrowed. A caller with no window asked
    // for the whole page, so there the resolution is what gives way.
    let whole = wanted.min(dpi_within(size_mm, limit));
    let (effective, window) = match window {
        Some(area) if whole < wanted => (wanted, Some(narrow(area, wanted, limit))),
        _ => (whole, None),
    };
    let options = RenderOptions {
        dpi: f64::from(effective),
        window,
        ..RenderOptions::default()
    };
    // Straight from the rasterizer, not through a PNG: the encode and decode
    // cost more than drawing the page, and they threw away the counts of what
    // it was short of, which travel in the raster rather than in the file.
    let raster = doc.render_page(page, &options)?;
    let short = Short {
        glyphs: raster.missing_glyphs,
        font_runs: raster.skipped_font_runs,
        images: raster.skipped_image_ops,
    };
    let (width, height) = (raster.width, raster.height);
    let mut rgba =
        image::RgbaImage::from_raw(width, height, raster.rgba_bytes()).ok_or_else(|| {
            sofd_error::OfdError::Render(sofd_error::RenderError::ImageEncode {
                page,
                reason: format!(
                    "the raster is {width} by {height} px, which is not its pixel count"
                ),
            })
        })?;
    for seal in seals.iter().filter(|s| s.page == page) {
        stamp_seal(&mut rgba, seal, f64::from(effective), options.window);
    }
    Ok(RenderedPage {
        pixels: Rgba {
            width,
            height,
            bytes: rgba.into_raw(),
        },
        size_mm,
        window: window.map(|area| clip_to_page(area, size_mm)),
        short,
    })
}

/// Keep a window inside the page it belongs to. The rasterizer clips as well,
/// but the canvas needs the size the image actually covers, and reading it
/// back from the rasterizer would mean a second copy of the same arithmetic.
fn clip_to_page(area: sofd_common::Rect, size_mm: (f32, f32)) -> sofd_common::Rect {
    let page = sofd_common::Rect::new(0.0, 0.0, f64::from(size_mm.0), f64::from(size_mm.1));
    let x0 = area.x.max(page.x);
    let y0 = area.y.max(page.y);
    let x1 = (area.x + area.w).min(page.x + page.w);
    let y1 = (area.y + area.h).min(page.y + page.h);
    sofd_common::Rect::new(x0, y0, (x1 - x0).max(0.0), (y1 - y0).max(0.0))
}

/// Shrink a visible area until its raster fits the texture limit, keeping its
/// top left corner: a reader scrolled to a corner keeps that corner.
fn narrow(window: sofd_common::Rect, dpi: f32, limit: u32) -> sofd_common::Rect {
    let most = f64::from(limit.max(1)) * (1.0 - LIMIT_MARGIN) / (f64::from(dpi) / MM_PER_INCH);
    sofd_common::Rect::new(window.x, window.y, window.w.min(most), window.h.min(most))
}

/// How much more than the visible area a raster covers. Scrolling inside the
/// margin costs no new raster, so a page stays sharp while the reader moves
/// around it instead of going blank at every step.
pub const WINDOW_MARGIN: f64 = 2.0;

/// The part of a page to rasterize for a reader looking at `visible`, in
/// millimetres. A page that fits at this resolution is asked for whole, which
/// is what keeps every ordinary zoom a single raster. Covering the view comes
/// before the margin, because the texture limit can leave a raster smaller
/// than the view it serves; a view off the page covers only what the page
/// holds.
pub fn window_for(
    visible: sofd_common::Rect,
    size_mm: (f32, f32),
    dpi: f32,
    limit: u32,
) -> sofd_common::Rect {
    let page_w = f64::from(size_mm.0);
    let page_h = f64::from(size_mm.1);
    let on_page = clip_to_page(visible, size_mm);
    // A view that holds none of the page has no part to ask for. The page is
    // the answer that draws nothing extra rather than a sliver at its edge.
    if on_page.w <= 0.0 || on_page.h <= 0.0 {
        return sofd_common::Rect::new(0.0, 0.0, page_w.max(1.0), page_h.max(1.0));
    }
    let most = f64::from(limit.max(1)) * (1.0 - LIMIT_MARGIN) / (f64::from(dpi) / MM_PER_INCH);
    let w = (on_page.w * WINDOW_MARGIN).clamp(1.0, page_w.max(1.0));
    let h = (on_page.h * WINDOW_MARGIN).clamp(1.0, page_h.max(1.0));
    // Never smaller than the part on screen: the margin is what the limit may
    // eat, not the view itself.
    let mut area = sofd_common::Rect::new(0.0, 0.0, w.min(most.max(1.0)), h.min(most.max(1.0)));
    // Centred on the view, pulled inside the page, then pulled back over the
    // view: a window narrower than the view cannot cover it from anywhere else.
    area.x = ((on_page.x + on_page.w / 2.0 - area.w / 2.0).clamp(0.0, (page_w - area.w).max(0.0)))
        .min(on_page.x)
        .max(on_page.x + on_page.w - area.w);
    area.y = ((on_page.y + on_page.h / 2.0 - area.h / 2.0).clamp(0.0, (page_h - area.h).max(0.0)))
        .min(on_page.y)
        .max(on_page.y + on_page.h - area.h);
    area
}

/// Whether `outer` covers `inner`, in the same millimetres: the question a
/// reader asks before spending a raster on a scroll it has already prepared
/// for.
pub fn covers(outer: Option<sofd_common::Rect>, inner: sofd_common::Rect) -> bool {
    let Some(outer) = outer else {
        return true; // The whole page is held, so everything on it is.
    };
    outer.x <= inner.x
        && outer.y <= inner.y
        && outer.x + outer.w >= inner.x + inner.w
        && outer.y + outer.h >= inner.y + inner.h
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
            // The same shortcut as a page: a seal is drawn, not filed, so the
            // raster is what is wanted and encoding it to PNG to read it back
            // would be work with nothing on the other side of it.
            let raster = inner.render_page(0, &options).ok()?;
            let (width, height) = (raster.width, raster.height);
            let drawn = image::RgbaImage::from_raw(width, height, raster.rgba_bytes())?;
            return Some(image::DynamicImage::ImageRgba8(drawn));
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
        let drawn = render_page_with_seals(&doc, page, 96.0, &seals, None, MAX_TEXTURE_SIDE)
            .expect("the page renders");
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
        for y in y0..y1.min(drawn.pixels.height as usize) {
            for x in x0..x1.min(drawn.pixels.width as usize) {
                let [r, g, b, _] = drawn.pixels.pixel(x as u32, y as u32);
                total += 1;
                if r > 150 && g < 120 && b < 120 {
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

    /// A page asks for more than the texture limit can hold, so it is
    /// rasterized in parts. The part it covers has to come back with the
    /// raster: the canvas paints the image into the rectangle that part
    /// occupies, and without it every zoomed page was stretched over the
    /// whole sheet, which moved the content and changed the page's shape.
    #[test]
    fn a_windowed_raster_reports_the_part_it_covers() {
        let Some(bytes) = fixture("tests/fixtures/ofdrw/999.ofd") else {
            return;
        };
        let doc = sofd::Document::open_bytes(&bytes).expect("the fixture opens");
        let size = doc
            .page(0)
            .map(|page| (page.page_box.w as f32, page.page_box.h as f32))
            .expect("the page has a box");
        let asked = sofd_common::Rect::new(20.0, 10.0, 60.0, 60.0);
        // Past what the fixture page fits whole (about 248 dpi), but small
        // enough that the part asked for is not itself narrowed by the limit.
        let rendered = render_page_with_seals(&doc, 0, 300.0, &[], Some(asked), MAX_TEXTURE_SIDE)
            .expect("the part renders");
        let part = rendered
            .window
            .expect("a page past the limit is rasterized in parts");
        assert!(
            (part.x - asked.x).abs() < 0.01
                && (part.y - asked.y).abs() < 0.01
                && (part.w - asked.w).abs() < 0.01
                && (part.h - asked.h).abs() < 0.01,
            "the raster covers {part:?}, which is not the window asked for {asked:?}"
        );
        // The image is the part, not the page: its shape follows the window,
        // which is the whole point of carrying it.
        let from_image = rendered.pixels.height as f32 / rendered.pixels.width as f32;
        let from_part = (part.h / part.w) as f32;
        assert!(
            (from_image - from_part).abs() < 0.01,
            "image is {from_image:.3} tall per wide but the window is {from_part:.3}"
        );
        let whole = size.1 / size.0;
        assert!(
            (from_image - whole).abs() > 0.05,
            "the fixture page is {whole:.3}, too close to the window to tell them apart"
        );
    }

    /// The ordinary case has to stay a single raster: a page that fits costs
    /// one windowless render, and a reader who never zooms past the limit
    /// never notices any of this.
    #[test]
    fn a_page_that_fits_is_rasterized_whole() {
        let Some(bytes) = fixture("tests/fixtures/ofdrw/999.ofd") else {
            return;
        };
        let doc = sofd::Document::open_bytes(&bytes).expect("the fixture opens");
        let size = doc
            .page(0)
            .map(|page| (page.page_box.w as f32, page.page_box.h as f32))
            .expect("the page has a box");
        let page = sofd_common::Rect::new(0.0, 0.0, f64::from(size.0), f64::from(size.1));
        let rendered = render_page_with_seals(&doc, 0, 96.0, &[], Some(page), MAX_TEXTURE_SIDE)
            .expect("the page renders");
        assert!(
            rendered.window.is_none(),
            "a page that fits is asked for whole, whatever window was offered"
        );
    }

    /// A caller with no window asked for the whole page. Past the texture
    /// limit there is nothing to narrow, so the resolution gives way instead
    /// of the raster being handed to the toolkit over its own limit.
    #[test]
    fn a_whole_page_asked_for_past_the_limit_comes_back_within_it() {
        let Some(bytes) = fixture("tests/fixtures/ofdrw/999.ofd") else {
            return;
        };
        let doc = sofd::Document::open_bytes(&bytes).expect("the fixture opens");
        let rendered = render_page(&doc, 0, MAX_DPI).expect("the page renders whole");
        assert!(rendered.window.is_none(), "no window was asked for");
        let side = MAX_TEXTURE_SIDE as usize;
        assert!(
            rendered.pixels.size()[0] <= side && rendered.pixels.size()[1] <= side,
            "a {}x{} raster exceeds the {side} px texture limit",
            rendered.pixels.width,
            rendered.pixels.height
        );
    }

    /// The window is placed on what the reader is looking at, never at the
    /// page's corner: scrolling is what makes a windowed raster useful.
    #[test]
    fn the_window_follows_the_visible_area() {
        let size = (210.0_f32, 297.0_f32);
        let dpi = 300.0_f32;
        let top = window_for(
            sofd_common::Rect::new(0.0, 0.0, 80.0, 80.0),
            size,
            dpi,
            MAX_TEXTURE_SIDE,
        );
        assert_eq!((top.x, top.y), (0.0, 0.0), "the corner stays the corner");

        let far = window_for(
            sofd_common::Rect::new(130.0, 217.0, 80.0, 80.0),
            size,
            dpi,
            MAX_TEXTURE_SIDE,
        );
        assert!(
            covers(Some(far), sofd_common::Rect::new(130.0, 217.0, 80.0, 80.0)),
            "the visible area at the far corner is not inside {far:?}"
        );
        assert!(
            far.x > 0.0 && far.y > 0.0,
            "a reader scrolled to the far corner is still given the corner: {far:?}"
        );
    }

    /// The window is what gets sharp ink. If it stops covering the view, the
    /// rest of the screen is either blank or a raster of another zoom, and no
    /// scrolling makes it whole again.
    #[test]
    fn a_window_always_covers_the_view_it_serves() {
        let size = (210.0_f32, 297.0_f32);
        let limit = 2048;
        // The view shrinks as the resolution rises, because both come from the
        // same zoom: a reader at 1,536 dpi is looking at a third of a page, not
        // at a page 10,882 pixels across.
        for dpi in [96.0_f32, 300.0, 768.0, 1536.0] {
            let zoom = f64::from(dpi) / 96.0;
            let screen = (1200.0 / zoom * 25.4 / 72.0, 800.0 / zoom * 25.4 / 72.0);
            let (vw, vh) = (screen.0.min(210.0), screen.1.min(297.0));
            for (x, y) in [(0.0, 0.0), (40.0, 30.0), (-30.0, -20.0), (190.0, 280.0)] {
                let view = sofd_common::Rect::new(x, y, vw, vh);
                // Only the part of the view the page holds can be covered.
                let on_page = clip_to_page(view, size);
                if on_page.w <= 0.0 || on_page.h <= 0.0 {
                    continue;
                }
                let area = window_for(view, size, dpi, limit);
                assert!(
                    covers(Some(area), on_page),
                    "at {dpi} dpi the window {area:?} leaves {on_page:?} of the view unrendered"
                );
                assert!(
                    area.x >= -0.01
                        && area.y >= -0.01
                        && area.x + area.w <= 210.01
                        && area.y + area.h <= 297.01,
                    "window {area:?} leaves the page"
                );
            }
        }
    }

    #[test]
    fn the_window_stays_on_the_page() {
        let size = (210.0_f32, 297.0_f32);
        for (x, y) in [(0.0, 0.0), (200.0, 290.0), (-40.0, -40.0), (400.0, 500.0)] {
            let area = window_for(
                sofd_common::Rect::new(x, y, 80.0, 80.0),
                size,
                300.0,
                MAX_TEXTURE_SIDE,
            );
            assert!(
                area.x >= 0.0
                    && area.y >= 0.0
                    && area.x + area.w <= 210.01
                    && area.y + area.h <= 297.01,
                "window {area:?} leaves the page"
            );
        }
    }

    /// The raster is handed straight to the window now, so its size is what
    /// the page asks for at that resolution rather than whatever survived an
    /// encode and a decode.
    #[test]
    fn a_raster_is_the_size_the_page_asks_for() {
        let Some(bytes) = fixture("tests/fixtures/ofdrw/999.ofd") else {
            return;
        };
        let doc = sofd::Document::open_bytes(&bytes).expect("the fixture opens");
        let dpi = 96.0_f64;
        let rendered = render_page(&doc, 0, dpi as f32).expect("the page renders");
        let size = rendered.size_mm;
        let expect_w = (f64::from(size.0) * dpi / MM_PER_INCH).ceil() as usize;
        let expect_h = (f64::from(size.1) * dpi / MM_PER_INCH).ceil() as usize;
        assert_eq!(
            (
                rendered.pixels.width as usize,
                rendered.pixels.height as usize
            ),
            (expect_w, expect_h),
            "the raster is not {expect_w} by {expect_h} px at {dpi} dpi"
        );
        // What the page was short of is not asserted: this fixture sets
        // Chinese text, so a machine with no face for it reports hundreds of
        // missing glyphs and one with a fallback reports none.
    }

    /// The limit the driver reports is what the page is held to, so a desktop
    /// card is not shown at the resolution chosen for the weakest board this
    /// ships on. The floor stays the floor.
    #[test]
    fn a_bigger_texture_limit_buys_a_sharper_page() {
        let size = (210.0_f32, 297.0_f32);
        let floor = dpi_within(size, MAX_TEXTURE_SIDE);
        let desktop = dpi_within(size, 8192);
        assert!(
            desktop > floor * 3.5,
            "8192 px should buy far more than 2048: {floor} then {desktop} dpi"
        );
    }

    /// Rounding up is what once turned a resolution that fits exactly into a
    /// raster one pixel too wide, which the toolkit then refused outright.
    #[test]
    fn no_limit_produces_a_raster_past_it() {
        let size = (210.0_f32, 297.0_f32);
        for limit in [512_u32, 1024, 2048, 4096, 8192, 16384] {
            let dpi = dpi_within(size, limit);
            for mm in [size.0, size.1] {
                let px = (f64::from(mm) * f64::from(dpi) / MM_PER_INCH).ceil() as u32;
                assert!(
                    px <= limit,
                    "{px} px at {dpi} dpi exceeds the {limit} px limit"
                );
            }
        }
    }

    fn page_of(side: usize) -> RenderedPage {
        RenderedPage {
            pixels: Rgba {
                width: side as u32,
                height: side as u32,
                bytes: vec![255; side * side * 4],
            },
            size_mm: (210.0, 297.0),
            window: None,
            short: Short::default(),
        }
    }

    /// The same, covering the part of the page `window` names.
    fn part_of(side: usize, window: sofd_common::Rect) -> RenderedPage {
        RenderedPage {
            window: Some(window),
            ..page_of(side)
        }
    }

    /// A scroll asks for a new window each step while the raster in hand still
    /// holds the view. The cache has to answer that by area, because the key
    /// moved even though the ink did not.
    #[test]
    fn the_cache_answers_a_scroll_that_stays_inside_its_raster() {
        let mut cache = PageCache::new(12);
        let held = sofd_common::Rect::new(40.0, 30.0, 120.0, 100.0);
        cache.insert(0, 300.0, part_of(3, held));

        // A view still inside the raster: the same entry answers.
        let inside = sofd_common::Rect::new(50.0, 40.0, 60.0, 40.0);
        assert!(
            cache.covering(0, 300.0, Some(inside)).is_some(),
            "a view inside the cached raster is not looked up"
        );
        // A view the raster no longer holds: nothing answers, so it renders.
        let outside = sofd_common::Rect::new(140.0, 100.0, 60.0, 40.0);
        assert!(
            cache.covering(0, 300.0, Some(outside)).is_none(),
            "a view past the cached raster must not borrow it"
        );
        // The whole page is not a window, so it is not answered by one.
        assert!(cache.covering(0, 300.0, None).is_none());
    }

    /// Two windows of one page at one zoom are two answers, not one that
    /// overwrites the other: scrolling back has to find what it left.
    #[test]
    fn windows_of_one_page_and_zoom_stay_apart() {
        let mut cache = PageCache::new(12);
        let near = sofd_common::Rect::new(0.0, 0.0, 80.0, 80.0);
        let far = sofd_common::Rect::new(120.0, 200.0, 80.0, 80.0);
        cache.insert(0, 300.0, part_of(2, near));
        cache.insert(0, 300.0, part_of(2, far));

        assert_eq!(cache.len(), 2, "the second window replaced the first");
        assert!(
            cache.get(0, 300.0).is_none(),
            "get asks for a whole page, which no window is"
        );
        assert!(
            cache
                .covering(
                    0,
                    300.0,
                    Some(sofd_common::Rect::new(10.0, 10.0, 20.0, 20.0))
                )
                .is_some(),
            "scrolling back to the first window found nothing"
        );
        assert!(
            cache
                .covering(
                    0,
                    300.0,
                    Some(sofd_common::Rect::new(130.0, 210.0, 20.0, 20.0))
                )
                .is_some(),
            "the second window is not held either"
        );
    }

    /// When more than one raster covers the view, the smallest is the one to
    /// keep: it is the same answer for less memory, and it is the one the next
    /// scroll is likeliest to have outgrown.
    #[test]
    fn the_smallest_raster_that_covers_the_view_is_the_one_found() {
        let mut cache = PageCache::new(12);
        let wide = sofd_common::Rect::new(0.0, 0.0, 200.0, 200.0);
        let tight = sofd_common::Rect::new(20.0, 20.0, 60.0, 60.0);
        cache.insert(0, 300.0, part_of(9, wide));
        cache.insert(0, 300.0, part_of(2, tight));

        let view = sofd_common::Rect::new(30.0, 30.0, 40.0, 40.0);
        let found = cache.covering(0, 300.0, Some(view)).expect("one covers it");
        assert_eq!(
            (found.pixels.width, found.pixels.height),
            (2, 2),
            "the larger raster answered a view the smaller one already held"
        );
    }

    /// A count of pages is not a memory budget: the zoom decides each raster's
    /// size, and twelve pages of the largest raster is three gigabytes.
    #[test]
    fn the_cache_evicts_by_area_and_not_only_by_count() {
        // Room for one two-by-two page, but a count of twelve.
        let mut cache = PageCache::with_budget(12, 4);
        cache.insert(0, 96.0, page_of(2));
        assert_eq!(cache.pixels(), 4, "one page of four pixels");
        cache.insert(1, 96.0, page_of(2));
        assert_eq!(cache.len(), 1, "two pages no longer fit the budget");
        assert!(cache.get(0, 96.0).is_none(), "the older page gave way");
        assert!(cache.get(1, 96.0).is_some(), "the newest page stays");
        assert_eq!(
            cache.pixels(),
            4,
            "the count of pixels followed the eviction"
        );
    }

    /// Showing nothing is worse than holding one page too large for the
    /// budget, so the newest entry is never the one evicted.
    #[test]
    fn a_page_larger_than_the_budget_is_still_kept() {
        let mut cache = PageCache::with_budget(12, 1);
        cache.insert(0, 96.0, page_of(4));
        assert!(cache.get(0, 96.0).is_some(), "the page being read is kept");
        assert_eq!(cache.pixels(), 16);
    }

    /// Replacing an entry the cache already held adds only the difference; a
    /// re-render at the same key must not look like two pages of memory.
    #[test]
    fn replacing_a_page_does_not_double_its_pixels() {
        let mut cache = PageCache::with_budget(12, 1024);
        cache.insert(0, 96.0, page_of(2));
        cache.insert(0, 96.0, page_of(3));
        assert_eq!(cache.pixels(), 9, "only the page that is there counts");
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn a_whole_page_covers_every_viewport() {
        let visible = sofd_common::Rect::new(10.0, 10.0, 20.0, 20.0);
        assert!(covers(None, visible));
        assert!(covers(
            Some(sofd_common::Rect::new(0.0, 0.0, 100.0, 100.0)),
            visible
        ));
        assert!(!covers(
            Some(sofd_common::Rect::new(0.0, 0.0, 15.0, 15.0)),
            visible
        ));
    }
}
