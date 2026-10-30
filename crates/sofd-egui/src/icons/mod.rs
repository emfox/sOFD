//! The symbolic icon set, filled for this toolkit. The glyphs themselves live
//! in `sofd-icons`, so the Tauri shell draws the same shapes rather than a
//! second copy; what is here is the fill, which `egui`'s own polygon shape
//! cannot do correctly for an outlined glyph.

use eframe::egui::{self, Color32, Pos2, Rect, Response, Sense, Shape, Vec2};

use crate::theme::{self, Token};

pub use sofd_icons::{Icon, SIZE};

/// A glyph's outlines as closed rings of points on the 16-unit square,
/// curves spelled out as the short straight steps a fill can count.
fn flattened(icon: Icon) -> Vec<(Vec<Pos2>, f32)> {
    icon.drawing()
        .parts
        .iter()
        .map(|(subpath, shade)| {
            let mut ring = vec![Pos2::new(subpath.start.0, subpath.start.1)];
            for segment in &subpath.segments {
                match *segment {
                    sofd_icons::path::Segment::Line(to) => ring.push(Pos2::new(to.0, to.1)),
                    sofd_icons::path::Segment::Cubic(c1, c2, to) => {
                        let from = ring[ring.len() - 1];
                        let at = |(x, y): (f32, f32)| Pos2::new(x, y);
                        ring.extend(curve(from, at(c1), at(c2), at(to)));
                    }
                }
            }
            if subpath.closed && ring.len() > 1 {
                ring.push(ring[0]);
            }
            (ring, *shade)
        })
        .collect()
}

/// Paint one icon into `rect`, in `colour`. The fill is worked out here rather
/// than handed to the painter, which fills a polygon as a fan over its points
/// and would turn every outlined glyph solid.
pub fn paint(painter: &egui::Painter, rect: Rect, icon: Icon, colour: Color32) {
    let span = rect.width().min(rect.height());
    let pp = painter.ctx().pixels_per_point();
    let mut shapes = Vec::new();
    for band in bands(icon) {
        let fill = match band.shade {
            1.0 => colour,
            shade => {
                let alpha = (f32::from(colour.a()) * shade).round() as u8;
                Color32::from_rgba_unmultiplied(colour.r(), colour.g(), colour.b(), alpha)
            }
        };
        // Both edges snap to the pixel grid, so a row's bottom edge is the
        // next row's top edge exactly: neighbours meet without a seam, and
        // without an overlap, which would darken the join.
        let y0 = ((rect.min.y + band.top * span / SIZE) * pp).round() / pp;
        let y1 = ((rect.min.y + (band.top + band.height) * span / SIZE) * pp).round() / pp;
        let points = [
            Pos2::new(rect.min.x + band.from * span / SIZE, y0),
            Pos2::new(rect.min.x + band.to * span / SIZE, y0),
            Pos2::new(rect.min.x + band.to * span / SIZE, y1),
            Pos2::new(rect.min.x + band.from * span / SIZE, y1),
        ];
        shapes.push(Shape::convex_polygon(
            points.to_vec(),
            fill,
            egui::Stroke::NONE,
        ));
    }
    painter.extend(shapes);
}

/// One run of a scanline the glyph covers, in the icon's own 16-unit space.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Band {
    /// Where the run starts, across the square.
    from: f32,
    /// Where it ends.
    to: f32,
    /// The top of the slice it fills, in units.
    top: f32,
    /// Its height, in units.
    height: f32,
    /// The strength the path that covers it is drawn at.
    shade: f32,
}

/// How many slices the square is cut into for the fill. Four times the grid, so
/// a slice is a quarter of a unit and a curve has steps small enough to read as
/// one at the size the window draws.
const SLICES: usize = (SIZE as usize) * 4;

/// Every run of every slice the glyph covers. The rule is nonzero winding, the
/// one an SVG path fills by default, and it is what makes the inner half of a
/// doubled outline a hole.
fn bands(icon: Icon) -> Vec<Band> {
    let paths = flattened(icon);
    let height = SIZE / SLICES as f32;
    let mut out = Vec::new();
    for slice in 0..SLICES {
        let top = slice as f32 * height;
        let y = top + height / 2.0;
        let mut crossings: Vec<(f32, bool, f32)> = Vec::new();
        for (points, shade) in &paths {
            for (a, b) in points.iter().zip(points.iter().cycle().skip(1)) {
                if (a.y > y) == (b.y > y) {
                    continue;
                }
                let t = (y - a.y) / (b.y - a.y);
                crossings.push((a.x + t * (b.x - a.x), b.y > a.y, *shade));
            }
        }
        crossings.sort_by(|a, b| a.0.total_cmp(&b.0));
        let (mut winding, mut from, mut shade) = (0, 0.0, 0.0);
        for (x, down, tone) in crossings {
            let before = winding;
            winding += if down { 1 } else { -1 };
            if before == 0 && winding != 0 {
                from = x;
                shade = tone;
            } else if before != 0 && winding == 0 {
                out.push(Band {
                    from,
                    to: x,
                    top,
                    height,
                    shade: shade.min(tone),
                });
            }
        }
    }
    out
}

/// Re-exported for the tests: the path data copied out of the theme, so a test
/// can check it against the file it claims to come from.
#[doc(hidden)]
pub fn path_data(icon: Icon) -> &'static [&'static str] {
    icon.paths()
}

/// Re-exported for the tests: the shape of a glyph, as a painter sees it.
#[doc(hidden)]
pub fn outline(icon: Icon) -> Vec<Shape> {
    let mut shapes = Vec::new();
    for band in bands(icon) {
        let over = 0.02;
        let points = [
            Pos2::new(band.from, band.top - over),
            Pos2::new(band.to, band.top - over),
            Pos2::new(band.to, band.top + band.height + over),
            Pos2::new(band.from, band.top + band.height + over),
        ];
        shapes.push(Shape::convex_polygon(
            points.to_vec(),
            Color32::WHITE,
            egui::Stroke::NONE,
        ));
    }
    shapes
}

/// How many straight steps one cubic is drawn with. The glyphs reach the window
/// at twenty points, so a step is about a third of a point: the curve reads as
/// the theme drew it, and the fill has points it can count.
const CURVE_STEPS: usize = 24;

/// The points a cubic passes through after its start.
fn curve(from: Pos2, c1: Pos2, c2: Pos2, to: Pos2) -> Vec<Pos2> {
    (1..=CURVE_STEPS)
        .map(|step| {
            let t = step as f32 / CURVE_STEPS as f32;
            let u = 1.0 - t;
            let point = from.to_vec2() * (u * u * u)
                + c1.to_vec2() * (3.0 * u * u * t)
                + c2.to_vec2() * (3.0 * u * t * t)
                + to.to_vec2() * (t * t * t);
            Pos2::new(point.x, point.y)
        })
        .collect()
}

/// The edge an icon occupies at the reader's scale, for a layout to reserve.
pub fn box_size(scale: f32) -> Vec2 {
    Vec2::splat((BOX * scale).round().max(1.0))
}

/// Draw an icon as a widget of its own, in the window's text colour.
pub fn show(ui: &mut egui::Ui, icon: Icon) -> Response {
    let colour = theme::colour(ui.ctx(), false, Token::Text);
    show_toned(ui, icon, colour)
}

/// The same in a named colour, for a glyph that stands for a verdict.
pub fn show_toned(ui: &mut egui::Ui, icon: Icon, colour: Color32) -> Response {
    let side = box_size((ui.spacing().interact_size.y / CONTROL_RATIO).max(0.6)).x;
    let (rect, response) = ui.allocate_exact_size(Vec2::splat(side), Sense::hover());
    if ui.is_rect_visible(rect) {
        paint(ui.painter(), rect.shrink(side * 0.1), icon, colour);
    }
    response
}

/// The edge a glyph is rasterized at for the texture layer, in points: four
/// times the grid, so the picture has pixels to spend when it is scaled up.
const WANTED: f32 = SIZE * 4.0;

/// The glyph as a texture, for a button, which lays its contents out before it
/// knows where they land.
pub fn texture(ctx: &egui::Context, icon: Icon, colour: Color32) -> egui::TextureHandle {
    let key = egui::Id::new(("icon-texture", icon as u8, colour.to_array()));
    if let Some(handle) = ctx.data(|data| data.get_temp::<egui::TextureHandle>(key)) {
        return handle;
    }
    let image = ctx.load_texture(
        format!("icon-{}", icon.theme_name()),
        egui::ImageData::Color(std::sync::Arc::new(picture(icon, colour))),
        egui::TextureOptions::LINEAR,
    );
    ctx.data_mut(|data| data.insert_temp(key, image.clone()));
    image
}

/// One glyph as a picture in its own grid, for `texture` to hand over. The same
/// runs the painter draws, at four times the resolution.
fn picture(icon: Icon, colour: Color32) -> egui::ColorImage {
    let scale = WANTED / SIZE;
    let side = WANTED.round() as usize;
    let mut canvas = Canvas {
        pixels: vec![Color32::TRANSPARENT; side * side],
        side,
    };
    for band in bands(icon) {
        let alpha = (f32::from(colour.a()) * band.shade).round() as u8;
        let fill = Color32::from_rgba_unmultiplied(colour.r(), colour.g(), colour.b(), alpha);
        let y0 = (band.top * scale).round() as usize;
        let y1 = ((band.top + band.height) * scale).round() as usize;
        let x0 = (band.from * scale).round();
        let x1 = (band.to * scale).round();
        canvas.fill(x0, x1, y0, y1, fill);
    }
    egui::ColorImage::new([side, side], canvas.pixels)
}

/// A pixel buffer the runs are written into.
struct Canvas {
    pixels: Vec<Color32>,
    side: usize,
}

impl Canvas {
    /// Lay one run of a scanline down, clipped to the picture.
    fn fill(&mut self, x0: f32, x1: f32, y0: usize, y1: usize, colour: Color32) {
        let lo = x0.max(0.0) as usize;
        let hi = (x1.max(0.0) as usize).min(self.side);
        for y in y0.min(self.side)..y1.min(self.side) {
            for x in lo..hi {
                self.pixels[y * self.side + x] = colour;
            }
        }
    }
}

/// The edge an icon is drawn at, in points, before the reader's scale.
pub const BOX: f32 = 20.0;

/// The ratio between a control's height and the box an icon sits in, so a glyph
/// reads as part of a button rather than filling it edge to edge.
const CONTROL_RATIO: f32 = 2.2;
