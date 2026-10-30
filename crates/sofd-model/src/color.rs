//! Colour representation as written in OFD.
//! GB/T 33190 clause 8.3.2 writes `Value` as one decimal per channel,
//! space separated; hex spellings seen in the wild are accepted too.

use serde::{Deserialize, Serialize};
use sofd_error::{OfdError, XmlError};

use crate::Valid;
use crate::document::Layer;

/// Colour vocabulary of GB/T 33190-2016, used by `FillColor` and `StrokeColor`.
/// Clause 8.3.4 also lets a colour be a shading, in which case `value` is empty
/// and [`ColorRef::shading`] carries the segments and geometry.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ColorRef {
    /// `#RRGGBB` or `rgb(r,g,b)` as written.
    pub value: String,
    /// Alpha from 0 to 255, absent meaning opaque.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alpha: Option<u8>,
    /// Gradient instead of a flat colour, when the writer used a shading.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shading: Option<Shading>,
    /// Clause 7.13: the `ColorSpaces` entry this value's channels name.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub space: Option<crate::RefId>,
    /// Index into the named space's palette, for an indexed colour.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<i32>,
}

/// One colour stop of a shading.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Segment {
    /// Position along the gradient, from 0 to 1.
    #[serde(rename = "Position", default)]
    pub position: f64,
    /// Colour at this position.
    #[serde(rename = "Color", default)]
    pub color: ColorRef,
}

/// One vertex of a Gouraud mesh: where it sits and the colour it carries.
/// The colour is a plain `#RRGGBB` plus alpha rather than a [`ColorRef`],
/// which would nest a shading inside a shading and never size.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeshPoint {
    /// Position, in the object's content space.
    pub point: (f64, f64),
    /// Colour sampled at this vertex, `#RRGGBB`.
    pub color: String,
    /// Alpha at this vertex, 0 to 255.
    pub alpha: u8,
}

/// One triangle of a Gouraud mesh. The three vertices are indices into the
/// mesh's point list; a triangle whose vertices are all different colours
/// interpolates between them across its area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MeshTriangle {
    /// Index of the first vertex.
    pub a: usize,
    /// Index of the second vertex.
    pub b: usize,
    /// Index of the third vertex.
    pub c: usize,
}

/// How the cells of a pattern repeat, clause 8.3.3.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum ReflectMethod {
    /// Repeat the cell as drawn.
    #[default]
    Normal,
    /// Mirror each successive row.
    Row,
    /// Mirror each successive column.
    Column,
    /// Mirror both rows and columns.
    RowAndColumn,
}

/// What a pattern's cell is positioned against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum PatternSpace {
    /// The page origin.
    Page,
    /// The object being filled.
    #[default]
    Object,
}

/// A `Pattern` fill: a cell of vector content repeated over the filled area.
/// Clause 8.3.3 makes the cell a page block, not an image, so the content is
/// carried as the same layer list a page holds.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PatternSpec {
    /// The cell's content, one layer per `Layer` child.
    pub layers: Vec<Layer>,
    /// Cell width, in the units the cell's own coordinates use.
    pub width: f64,
    /// Cell height.
    pub height: f64,
    /// Horizontal step between cells; defaults to `width`.
    pub x_step: f64,
    /// Vertical step between cells; defaults to `height`.
    pub y_step: f64,
    /// How cells repeat.
    pub reflect: ReflectMethod,
    /// What the tiling is anchored to.
    pub relative_to: PatternSpace,
    /// An extra transform over the cell, as written.
    pub ctm: Option<String>,
}

impl PatternSpec {
    /// The horizontal step, falling back to the cell width.
    pub fn step_x(&self) -> f64 {
        if self.x_step > 0.0 {
            self.x_step
        } else {
            self.width
        }
    }

    /// The vertical step, falling back to the cell height.
    pub fn step_y(&self) -> f64 {
        if self.y_step > 0.0 {
            self.y_step
        } else {
            self.height
        }
    }
}

/// The shadings clause 8.3 defines: a linear axis, a radial cone, and the two
/// colour-interpolated meshes of 8.3.4.4 and 8.3.4.5.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum Shading {
    /// `AxialShd`: a linear gradient from one point to another.
    Axial {
        /// Start of the gradient axis.
        start: (f64, f64),
        /// End of the gradient axis.
        end: (f64, f64),
        /// Colour stops, at least two, in the order written.
        segments: Vec<Segment>,
    },
    /// `RadialShd`: a cone from one circle to another.
    Radial {
        /// Centre of the start circle.
        start: (f64, f64),
        /// Radius of the start circle.
        start_radius: f64,
        /// Centre of the end circle.
        end: (f64, f64),
        /// Radius of the end circle.
        end_radius: f64,
        /// Colour stops, at least two, in the order written.
        segments: Vec<Segment>,
    },
    /// `GouraudShd` (8.3.4.4) and `LaGouraudShd` (8.3.4.5): a mesh of coloured
    /// vertices whose triangles interpolate the colour between them. The two
    /// differ only in how the triangles are written, not in what they mean.
    Gouraud {
        /// Every vertex, in the order the shading wrote them.
        points: Vec<MeshPoint>,
        /// Triangles over those vertices.
        triangles: Vec<MeshTriangle>,
        /// Whether the writer used the lattice form of 8.3.4.5.
        lattice: bool,
        /// Colour for an area no triangle covers, if the writer named one.
        /// Boxed so a shading can nest inside a colour without sizing to it.
        background: Option<Box<ColorRef>>,
    },
    /// `Pattern` (8.3.3): a cell of vector content repeated over the area.
    Pattern(Box<PatternSpec>),
}
impl Shading {
    /// The colour stops, whichever shading this is. A mesh carries its colours
    /// on its vertices and a pattern in its cell content, so neither has any.
    pub fn segments(&self) -> &[Segment] {
        match self {
            Shading::Axial { segments, .. } | Shading::Radial { segments, .. } => segments,
            Shading::Gouraud { .. } | Shading::Pattern(_) => &[],
        }
    }
}

impl Default for ColorRef {
    /// Opaque black, the colour an absent value means.
    fn default() -> Self {
        Self::BLACK
    }
}

impl ColorRef {
    /// Opaque black, the OFD default when no colour is written.
    pub const BLACK: Self = Self {
        value: String::new(),
        alpha: None,
        shading: None,
        space: None,
        index: None,
    };

    /// Build from an `#RRGGBB` string.
    pub fn from_hex(hex: impl Into<String>) -> Self {
        Self {
            value: hex.into(),
            alpha: None,
            shading: None,
            space: None,
            index: None,
        }
    }

    /// Build from a shading, which carries its own colours.
    pub fn from_shading(shading: Shading) -> Self {
        Self {
            value: String::new(),
            alpha: None,
            shading: Some(shading),
            space: None,
            index: None,
        }
    }

    /// Attach the clause 7.13 space and palette index the value names.
    pub fn with_space(mut self, space: impl Into<String>, index: Option<i32>) -> Self {
        self.space = Some(space.into());
        self.index = index;
        self
    }

    /// Attach an alpha value.
    pub fn with_alpha(mut self, alpha: u8) -> Self {
        self.alpha = Some(alpha);
        self
    }

    /// Whether the colour text is absent, meaning the OFD default applies.
    /// Alpha does not count: a `<Color>` without a `Value` picks up a default
    /// alpha, and counting it made an omitted value parse as an invalid one.
    pub fn is_default(&self) -> bool {
        self.value.trim().is_empty() && self.shading.is_none()
    }

    /// The shading this colour carries, if it is a gradient rather than a flat
    /// colour.
    pub fn shading(&self) -> Option<&Shading> {
        self.shading.as_ref()
    }

    /// The channel values as written, without guessing a colour space.
    /// GB/T 33190 clause 8.3.2: one decimal per channel, space separated.
    pub fn channels(&self) -> sofd_error::Result<Vec<u8>> {
        let s = self.value.trim();
        if s.is_empty() {
            return Ok(vec![0, 0, 0]);
        }
        // The marker decides the base: `#ee #20 #25` is hex, the form the
        // standard writes, while a bare `156 82 35` is decimal.
        let marked = s.split_whitespace().all(|part| part.starts_with('#'));
        s.split_whitespace()
            .map(|part| {
                let digits = part.strip_prefix('#').unwrap_or(part);
                let radix = if marked { 16 } else { 10 };
                u8::from_str_radix(digits, radix).map_err(|_| invalid(s))
            })
            .collect()
    }

    /// Parse the colour into RGB, converting a four-channel CMYK value.
    pub fn to_rgb(&self) -> sofd_error::Result<(u8, u8, u8)> {
        if self.is_default() {
            return Ok((0, 0, 0));
        }

        let s = self.value.trim();
        if let Some(rest) = s.strip_prefix('#') {
            // `#` introduces either a hex triple (`#RGB`, `#RRGGBB`) or the
            // standard's own `#R #G #B`, each channel carrying its marker.
            if rest.contains(char::is_whitespace) {
                return parse_marked_channels(rest);
            }
            return parse_hex(rest);
        }
        if let Some(inner) = s.strip_prefix("rgb(").and_then(|v| v.strip_suffix(')')) {
            return parse_triple(inner);
        }
        if s.chars().any(char::is_whitespace) {
            return self.channels().and_then(to_rgb_channels);
        }
        // Written without a prefix at all.
        parse_hex(s)
    }

    /// Alpha, defaulting to opaque.
    pub fn alpha(self) -> u8 {
        self.alpha.unwrap_or(255)
    }
}

impl Valid for ColorRef {
    fn validate(&self) -> sofd_error::Result<()> {
        self.to_rgb().map(|_| ())
    }
}

/// Parse a 3- or 6-digit hex string into channels.
fn parse_hex(s: &str) -> sofd_error::Result<(u8, u8, u8)> {
    match s.len() {
        6 => {
            let v = u32::from_str_radix(s, 16).map_err(|_| invalid(s))?;
            Ok((
                ((v >> 16) & 0xFF) as u8,
                ((v >> 8) & 0xFF) as u8,
                (v & 0xFF) as u8,
            ))
        }
        // The three-digit form expands each digit, so `abc` is `aabbcc`.
        3 => {
            let v = u32::from_str_radix(s, 16).map_err(|_| invalid(s))?;
            let expand = |n: u32| ((n & 0xF) * 0x11) as u8;
            Ok((expand(v >> 8), expand(v >> 4), expand(v)))
        }
        _ => Err(invalid(s)),
    }
}

/// Three channels are RGB; four are CMYK and are converted.
fn to_rgb_channels(channels: Vec<u8>) -> sofd_error::Result<(u8, u8, u8)> {
    match channels.as_slice() {
        [r, g, b] => Ok((*r, *g, *b)),
        [c, m, y, k] => Ok((from_cmyk(*c, *k), from_cmyk(*m, *k), from_cmyk(*y, *k))),
        _ => Err(invalid(&format!("{channels:?}"))),
    }
}

/// One CMYK channel to RGB: `255 - (channel * (255 - K) / 255 + K)`.
///
/// The format's only CMYK reduction, so a colour means the same thing whether
/// it arrives as a `ColorRef` or through a declared space.
pub fn from_cmyk(channel: u8, key: u8) -> u8 {
    let ink = u16::from(channel) * u16::from(255 - key) / 255 + u16::from(key);
    255 - ink.min(255) as u8
}

/// Parse the standard's `#`-marked channel list, e.g. `#ee #20 #25`: each
/// token drops its `#` and is one hex channel. A longer token is rejected
/// rather than truncated, since guessing paints the page the wrong colour.
fn parse_marked_channels(inner: &str) -> sofd_error::Result<(u8, u8, u8)> {
    let mut channels: Vec<u8> = Vec::with_capacity(3);
    for token in inner.split_whitespace() {
        let digits = token.strip_prefix('#').unwrap_or(token);
        if digits.is_empty() || digits.len() > 2 {
            return Err(invalid(inner));
        }
        channels.push(u8::from_str_radix(digits, 16).map_err(|_| invalid(inner))?);
    }
    to_rgb_channels(channels)
}

/// Parse a comma-separated triple into channels.
fn parse_triple(inner: &str) -> sofd_error::Result<(u8, u8, u8)> {
    let nums: Result<Vec<u8>, _> = inner.split(',').map(|p| p.trim().parse::<u8>()).collect();
    match nums {
        Ok(v) if v.len() == 3 => Ok((v[0], v[1], v[2])),
        _ => Err(invalid(inner)),
    }
}

fn invalid(value: &str) -> OfdError {
    OfdError::Xml(XmlError::InvalidAttribute {
        element: "Color".to_string(),
        attr: "Value".to_string(),
        value: value.to_string(),
    })
}

impl std::fmt::Display for ColorRef {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self.alpha {
            Some(a) => write!(f, "{} alpha={}", self.value, a),
            None => f.write_str(&self.value),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn six_digit_hex_parses_to_channels() {
        let c = ColorRef::from_hex("#FF8000");
        assert_eq!(c.to_rgb().expect("parse"), (255, 128, 0));
    }

    #[test]
    fn six_digit_hex_without_the_hash_parses_the_same() {
        let c = ColorRef::from_hex("FF8000");
        assert_eq!(c.to_rgb().expect("parse"), (255, 128, 0));
    }

    #[test]
    fn three_digit_hex_expands_each_channel() {
        // #F80 must mean the same colour as #FF8800.
        let short = ColorRef::from_hex("#F80");
        let long = ColorRef::from_hex("#FF8800");
        assert_eq!(
            short.to_rgb().expect("parse"),
            long.to_rgb().expect("parse")
        );
    }

    #[test]
    fn whitespaces_in_hex_are_tolerated() {
        let c = ColorRef::from_hex("  #FF8000  ");
        assert_eq!(c.to_rgb().expect("parse"), (255, 128, 0));
    }

    #[test]
    fn rgb_function_syntax_parses() {
        let c = ColorRef::from_hex("rgb(10, 20, 30)");
        assert_eq!(c.to_rgb().expect("parse"), (10, 20, 30));
    }

    #[test]
    fn garbage_reports_the_value() {
        let c = ColorRef::from_hex("notacolour");
        let err = c.to_rgb().expect_err("must fail");
        match err {
            OfdError::Xml(XmlError::InvalidAttribute { element, value, .. }) => {
                assert_eq!(element, "Color");
                assert_eq!(value, "notacolour");
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn a_missing_colour_falls_back_to_black() {
        let c = ColorRef::BLACK;
        assert!(c.is_default());
        assert_eq!(c.to_rgb().expect("parse"), (0, 0, 0));
    }

    #[test]
    fn alpha_defaults_to_opaque() {
        let c = ColorRef::from_hex("#FFFFFF");
        assert_eq!(c.clone().alpha(), 255);
        assert_eq!(c.with_alpha(128).alpha(), 128);
    }

    #[test]
    fn display_shows_alpha_only_when_present() {
        assert_eq!(ColorRef::from_hex("#FFF").to_string(), "#FFF");
        assert_eq!(
            ColorRef::from_hex("#FFF").with_alpha(64).to_string(),
            "#FFF alpha=64"
        );
    }

    #[test]
    fn validation_accepts_well_formed_colours() {
        assert!(ColorRef::from_hex("#AABBCC").validate().is_ok());
        assert!(ColorRef::from_hex("zzz").validate().is_err());
    }

    // GB/T 33190 clause 8.3.2: `Value` holds one decimal per channel.
    #[test]
    fn gb_t_33190_8_3_2_value_is_space_separated_channels() {
        let c = ColorRef::from_hex("255 0 0");
        assert_eq!(c.to_rgb().expect("parse"), (255, 0, 0));
        assert_eq!(c.channels().expect("channels"), vec![255, 0, 0]);
    }

    #[test]
    fn gb_t_33190_8_3_2_four_channels_are_cmyk() {
        // Cyan with no key must land on pure cyan once converted to RGB.
        let c = ColorRef::from_hex("255 0 0 0");
        assert_eq!(c.channels().expect("channels"), vec![255, 0, 0, 0]);
        assert_eq!(c.to_rgb().expect("parse"), (0, 255, 255));
    }

    #[test]
    fn gb_t_33190_8_3_2_absent_value_and_index_are_all_zero() {
        // Both optional, so a colour without either is channel-wise zero.
        let c = ColorRef::BLACK;
        assert_eq!(c.channels().expect("channels"), vec![0, 0, 0]);
        assert_eq!(c.to_rgb().expect("parse"), (0, 0, 0));
    }

    #[test]
    fn gb_t_33190_8_3_2_alpha_defaults_to_255() {
        // Clause 8.3.2: Alpha ranges over 0~255 and defaults to opaque.
        assert_eq!(ColorRef::from_hex("255 0 0").alpha(), 255);
    }

    #[test]
    fn colours_degrade_gracefully_when_serde_round_trips() {
        let c = ColorRef::from_hex("#123456").with_alpha(10);
        let json = serde_json::to_string(&c).expect("serialize");
        let back: ColorRef = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, c);
    }
}
