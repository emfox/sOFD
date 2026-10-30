//! Draw commands and their paint attributes.
//! The shared vocabulary of every backend: adding one means translating these
//! commands, not understanding the OFD source format.

use serde::{Deserialize, Serialize};
use sofd_common::{Point, Rect};
use sofd_geometry::{Affine, Path};

use crate::list::PageDisplay;
use crate::resource::ImageRef;
use crate::text::GlyphRun;

/// A single drawing command.
/// Coordinates are already flattened to absolute page space in millimetres;
/// no command performs nested lookup or resource resolution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DrawOp {
    /// Push the current graphics state.
    Save,

    /// Pop the graphics state.
    Restore,

    /// Concatenate a transform onto the current state.
    Transform(Affine),

    /// Intersect the clip with a path.
    ClipPath {
        /// Clip geometry.
        path: Path,
        /// Fill rule used to interpret the path.
        rule: PaintRule,
    },

    /// Intersect the clip with the outlines of a text run. The region is the
    /// glyph shapes themselves, so a backend resolves them the way it resolves
    /// drawn text; a run whose font cannot be found clips nothing.
    ClipText(GlyphRun),

    /// Fill and/or stroke a path.
    Path {
        /// Path geometry.
        path: Path,
        /// Fill paint; `None` leaves the interior untouched.
        fill: Option<Paint>,
        /// Stroke paint; `None` leaves the outline untouched.
        stroke: Option<Paint>,
        /// Stroke geometry parameters.
        stroke_style: StrokeStyle,
        /// Fill rule.
        rule: PaintRule,
    },

    /// Draw shaped text.
    Text(GlyphRun),

    /// Draw an image.
    ///
    /// `dst` is the image's unit square in the object's content space, in
    /// absolute page millimetres: a backend maps the bitmap onto `dst` and
    /// the placement CTM on the state carries it to the boundary. No
    /// per-image transform exists; a placement folds any into the state.
    Image {
        /// Resolved image source.
        image: ImageRef,
        /// Destination rectangle in absolute page space.
        dst: Rect,
    },
}

/// Straight sRGB colour with alpha, each channel in `0..=255`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Color {
    /// Red channel.
    pub r: u8,
    /// Green channel.
    pub g: u8,
    /// Blue channel.
    pub b: u8,
    /// Alpha channel; 255 is opaque.
    pub a: u8,
}

impl Color {
    /// Opaque black.
    pub const BLACK: Self = Self {
        r: 0,
        g: 0,
        b: 0,
        a: 255,
    };
    /// Opaque white.
    pub const WHITE: Self = Self {
        r: 255,
        g: 255,
        b: 255,
        a: 255,
    };
    /// Fully transparent.
    pub const TRANSPARENT: Self = Self {
        r: 0,
        g: 0,
        b: 0,
        a: 0,
    };

    /// Build a colour from raw channels.
    pub const fn rgba(r: u8, g: u8, b: u8, a: u8) -> Self {
        Self { r, g, b, a }
    }

    /// Parse `#RRGGBB` or `RRGGBB`; alpha defaults to opaque.
    pub fn from_hex(s: &str) -> Option<Self> {
        let s = s.trim_start_matches('#');
        if s.len() != 6 {
            return None;
        }
        let v = u32::from_str_radix(s, 16).ok()?;
        Some(Self {
            r: ((v >> 16) & 0xFF) as u8,
            g: ((v >> 8) & 0xFF) as u8,
            b: (v & 0xFF) as u8,
            a: 255,
        })
    }
}

/// How a region is filled or stroked.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Paint {
    /// Uniform colour.
    Solid(Color),
    /// A linear shading: a gradient axis and its stops.
    LinearGradient(Box<LinearGradientSpec>),
    /// A radial shading: the cone between two circles.
    RadialGradient(Box<RadialGradientSpec>),
    /// A Gouraud mesh: triangles whose vertices carry colours.
    Mesh(Box<MeshSpec>),
    /// A pattern: a cell of vector content repeated over the filled area.
    Pattern(Box<PatternSpec>),
}

/// A linear shading an object carries with it, so a backend never has to have
/// seen an earlier op to paint it; clause 8.3.4's standalone shading and a
/// fill that folds one in both arrive as this.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinearGradientSpec {
    /// Gradient start point, in the object's content space.
    pub start: Point,
    /// Gradient end point, in the object's content space.
    pub end: Point,
    /// Colour stops, at least two.
    pub stops: Vec<GradientStop>,
}

/// A radial shading an object carries with it: two circles describe the cone.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RadialGradientSpec {
    /// Centre of the starting circle.
    pub center: Point,
    /// Radius of the starting circle.
    pub radius: f64,
    /// Centre of the ending circle.
    pub end: Point,
    /// Radius of the ending circle.
    pub end_radius: f64,
    /// Colour stops, at least two.
    pub stops: Vec<GradientStop>,
}

/// One colour stop of a gradient.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct GradientStop {
    /// Position along the gradient, `0.0..=1.0`.
    pub offset: f64,
    /// Colour at this position.
    pub color: Color,
}

/// A Gouraud mesh an object carries with it, from clause 8.3.4.4 (`GouraudShd`)
/// or the lattice form of 8.3.4.5. The two are the same once read, since the
/// lattice only changes how the triangles were written.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MeshSpec {
    /// Every vertex, in the order the shading wrote them.
    pub points: Vec<MeshVertex>,
    /// Triangles over those vertices.
    pub triangles: Vec<[usize; 3]>,
}

/// One vertex of a [`MeshSpec`]: a position and the colour it carries.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct MeshVertex {
    /// Position, in the object's content space.
    pub x: f64,
    /// Position, in the object's content space.
    pub y: f64,
    /// Colour sampled here, straight-line interpolated across a triangle.
    pub color: Color,
}

/// A pattern an object carries with it, from clause 8.3.3. The cell is vector
/// content, so it is flattened once here and the backend only tiles it.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PatternSpec {
    /// The cell's content, already flattened and in cell-local coordinates.
    pub cell: Box<PageDisplay>,
    /// Cell width.
    pub width: f64,
    /// Cell height.
    pub height: f64,
    /// Horizontal step between cells.
    pub step_x: f64,
    /// Vertical step between cells.
    pub step_y: f64,
    /// How the cell repeats, as the schema's `ReflectMethod`.
    pub reflect: Reflect,
}

/// How a pattern's cells repeat; the schema's `ReflectMethod`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum Reflect {
    /// Repeat as drawn.
    #[default]
    Normal,
    /// Mirror each successive row.
    Row,
    /// Mirror each successive column.
    Column,
    /// Mirror both.
    RowAndColumn,
}

impl Reflect {
    /// Whether a cell at the given column and row is mirrored.
    /// The field named is the one the mirroring runs along: a `Row` reflects
    /// left and right as the row advances.
    pub fn mirrors(&self, col: i64, row: i64) -> (bool, bool) {
        let mirror_x =
            matches!(self, Reflect::Row | Reflect::RowAndColumn) && row.rem_euclid(2) == 1;
        let mirror_y =
            matches!(self, Reflect::Column | Reflect::RowAndColumn) && col.rem_euclid(2) == 1;
        (mirror_x, mirror_y)
    }
}

/// Rule deciding which regions a path covers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PaintRule {
    /// Non-zero winding.
    #[default]
    NonZero,
    /// Even-odd.
    EvenOdd,
}

/// Stroke geometry parameters.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StrokeStyle {
    /// Line width in millimetres.
    pub width: f64,
    /// Dash pattern; empty means solid.
    pub dash: Vec<f64>,
    /// Offset into the dash pattern, in millimetres.
    pub dash_offset: f64,
    /// End cap style.
    pub cap: LineCap,
    /// Segment join style.
    pub join: LineJoin,
    /// Miter length limit before a join is bevelled.
    pub miter_limit: f64,
}

impl Default for StrokeStyle {
    fn default() -> Self {
        Self {
            width: 1.0,
            dash: Vec::new(),
            dash_offset: 0.0,
            cap: LineCap::Butt,
            join: LineJoin::Miter,
            miter_limit: 4.0,
        }
    }
}

/// How an open subpath is terminated.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LineCap {
    /// Stop exactly at the endpoint.
    #[default]
    Butt,
    /// Extend by a half-circle.
    Round,
    /// Extend by half the line width.
    Square,
}

/// How two segments meet.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum LineJoin {
    /// Extend to a sharp corner.
    #[default]
    Miter,
    /// Round the corner.
    Round,
    /// Cut the corner flat.
    Bevel,
}
