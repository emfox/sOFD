//! The type scale the interface is drawn at: named roles and the size each one
//! takes. Nothing here knows a toolkit, so a second shell draws the same scale
//! and the state that picks a text size can be tested with no window.

/// The size interface text is drawn at before the reader's own choice scales
/// it.
pub const BASE_POINTS: f32 = 14.0;

/// Each role's size as a ratio of the body size, in the order section 5.2
/// lists them. The name is what a toolkit registers the style under.
pub const SCALE: [(&str, f32); 8] = [
    ("large-title", 2.000),
    ("title-1", 1.600),
    ("title-2", 1.400),
    ("title-3", 1.250),
    ("heading", 1.200),
    ("title-4", 1.100),
    ("body", 1.000),
    ("caption", 0.850),
];

/// The style name a caller asks for. `Body`, `Heading` and `Caption` double as
/// the toolkit's styles of the same role, so the two cannot drift apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Style {
    /// The document name.
    LargeTitle,
    /// A panel title.
    Title1,
    /// A section inside a panel.
    Title2,
    /// A group heading.
    Title3,
    /// A row heading.
    Title4,
    /// A heading that is not titled.
    Heading,
    /// Everything read in sentences.
    Body,
    /// A signer, a time, a count.
    Caption,
}

impl Style {
    /// The named styles, in the order section 5.2 lists them.
    pub const NAMED: [Self; 5] = [
        Self::LargeTitle,
        Self::Title1,
        Self::Title2,
        Self::Title3,
        Self::Title4,
    ];

    /// The name this style is registered under in a toolkit's style table.
    pub fn name(self) -> &'static str {
        match self {
            Self::LargeTitle => "large-title",
            Self::Title1 => "title-1",
            Self::Title2 => "title-2",
            Self::Title3 => "title-3",
            Self::Title4 => "title-4",
            Self::Heading => "heading",
            Self::Body => "body",
            Self::Caption => "caption",
        }
    }

    /// The ratio against the body size.
    pub fn ratio(self) -> f32 {
        SCALE
            .iter()
            .find(|(name, _)| *name == self.name())
            .map_or(1.0, |(_, ratio)| *ratio)
    }

    /// The size in points at a given text scale, rounded so the rasterizer is
    /// not asked for a hundredth of a point.
    pub fn points(self, scale: f32) -> f32 {
        (BASE_POINTS * self.ratio() * scale).round().max(1.0)
    }
}
