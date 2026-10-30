//! The window's own appearance: the theme, the text size and high contrast.
//! They live in `App` rather than in `egui` so the state machine can be tested
//! without a window, and so no zone can style itself behind the others' backs.

use crate::message::ReaderMessage;
use crate::typography::{BASE_POINTS, Style};

use super::App;

/// Which theme the window draws in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeChoice {
    /// Whatever the desktop says, which is the default: a reader who told
    /// their desktop to go dark has told this window too.
    #[default]
    System,
    /// Light, whatever the desktop says.
    Light,
    /// Dark, whatever the desktop says.
    Dark,
}

impl ThemeChoice {
    /// Every choice, in the order the settings row offers them.
    pub const ALL: [Self; 3] = [Self::System, Self::Light, Self::Dark];

    /// The reader's own word for this choice.
    pub fn message(self) -> ReaderMessage {
        match self {
            Self::System => ReaderMessage::ThemeSystem,
            Self::Light => ReaderMessage::ThemeLight,
            Self::Dark => ReaderMessage::ThemeDark,
        }
    }
}

/// How large the interface text is drawn, as a ratio of the base size. Three
/// steps rather than a free number: a reader who needs larger text needs it
/// once, and a slider asks them to make the same decision every session.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum TextSize {
    /// The base size, 14 points.
    #[default]
    Normal,
    /// One step up, 115%.
    Large,
    /// Two steps up, 130%.
    Larger,
}

impl TextSize {
    /// Every step, smallest first.
    pub const ALL: [Self; 3] = [Self::Normal, Self::Large, Self::Larger];

    /// The ratio applied to every point of the type scale.
    pub fn scale(self) -> f32 {
        match self {
            Self::Normal => 1.0,
            Self::Large => 1.15,
            Self::Larger => 1.3,
        }
    }

    /// The reader's own word for this step.
    pub fn message(self) -> ReaderMessage {
        match self {
            Self::Normal => ReaderMessage::TextSizeNormal,
            Self::Large => ReaderMessage::TextSizeLarge,
            Self::Larger => ReaderMessage::TextSizeLarger,
        }
    }
}

impl App {
    /// The theme the window draws in.
    pub fn theme(&self) -> ThemeChoice {
        self.theme
    }

    /// Switch the theme. It is a setting of the window, so the document and
    /// its rendered pages are untouched.
    pub fn set_theme(&mut self, theme: ThemeChoice) {
        self.theme = theme;
    }

    /// How large interface text is drawn.
    pub fn text_size(&self) -> TextSize {
        self.text_size
    }

    /// Choose a text size.
    pub fn set_text_size(&mut self, size: TextSize) {
        self.text_size = size;
    }

    /// Whether high contrast is in force.
    pub fn high_contrast(&self) -> bool {
        self.high_contrast
    }

    /// Turn high contrast on or off. It tightens whichever theme is in force
    /// rather than replacing it, so the reader keeps their light-or-dark
    /// choice and gains the contrast they asked for.
    pub fn set_high_contrast(&mut self, on: bool) {
        self.high_contrast = on;
    }

    /// The base point size at the chosen text size, which the settings row
    /// shows so the choice is a number rather than a guess.
    pub fn base_points(&self) -> f32 {
        (BASE_POINTS * self.text_size.scale()).round()
    }

    /// The point size a named style is drawn at, for a test to compare.
    pub fn style_points(&self, style: Style) -> f32 {
        style.points(self.text_size.scale())
    }

    /// The label on the theme group inside the settings row.
    pub fn theme_label(&self) -> String {
        self.text(ReaderMessage::Theme)
    }

    /// The label on the text-size group inside the settings row.
    pub fn text_size_label(&self) -> String {
        self.text(ReaderMessage::TextSize)
    }

    /// The word for one theme choice.
    pub fn theme_name(&self, choice: ThemeChoice) -> String {
        self.text(choice.message())
    }

    /// The word for one text-size step, with the size it produces.
    pub fn text_size_name(&self, size: TextSize) -> String {
        self.text_with(
            size.message(),
            [(
                "points",
                (BASE_POINTS * size.scale()).round().to_string().into(),
            )],
        )
    }

    /// The word for the high-contrast switch.
    pub fn high_contrast_label(&self) -> String {
        self.text(ReaderMessage::HighContrast)
    }

    /// What the high-contrast switch says it does, for its tooltip.
    pub fn high_contrast_hint(&self) -> String {
        self.text(ReaderMessage::HighContrastHint)
    }
}
