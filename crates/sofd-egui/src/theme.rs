//! The appearance tables from `docs/Ui-spec.md` section 5: the only place a
//! colour is written. The type scale is not here — it lives in
//! `sofd-session::typography`, so a second shell draws the same sizes — and
//! is re-exported below, so a widget still names one module.

use eframe::egui::{self, Color32, FontFamily, FontId, TextStyle, Theme};
pub use sofd_session::typography::{BASE_POINTS, SCALE, Style};

/// A colour role. Every painted colour in the window is one of these.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Token {
    /// The window behind the panels.
    Surface,
    /// A panel or a header.
    Panel,
    /// Body text.
    Text,
    /// Secondary text: a signer, a count.
    Dim,
    /// The line between zones.
    Border,
    /// Selection, focus and the current hit.
    Accent,
    /// A signature that holds.
    Valid,
    /// A signature that does not hold.
    Tampered,
    /// A signature nothing can confirm.
    Unverified,
    /// Behind the page itself.
    Canvas,
    /// A search hit drawn on the page, and the one the reader is on.
    Hit,
    /// An annotation's boundary drawn on the page.
    Mark,
}

/// A scheme is light or dark; it is the column of the table, not a comparison.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Scheme {
    Light,
    Dark,
}

/// The light column of section 5.1, indexed by [`Token`].
const LIGHT: [Color32; 12] = [
    Color32::from_rgb(0xfa, 0xfa, 0xfa),
    Color32::from_rgb(0xff, 0xff, 0xff),
    Color32::from_rgb(0x1a, 0x1a, 0x1a),
    Color32::from_rgb(0x5e, 0x5e, 0x5e),
    Color32::from_rgb(0xdc, 0xdc, 0xdc),
    Color32::from_rgb(0x1c, 0x71, 0xd8),
    Color32::from_rgb(0x2a, 0x7b, 0x3c),
    Color32::from_rgb(0xb4, 0x2a, 0x2a),
    Color32::from_rgb(0x8a, 0x61, 0x00),
    Color32::from_rgb(0xee, 0xee, 0xee),
    Color32::from_rgb(0x8a, 0x61, 0x00),
    Color32::from_rgb(0x15, 0x59, 0xb0),
];

/// The dark column of section 5.1, indexed by [`Token`].
const DARK: [Color32; 12] = [
    Color32::from_rgb(0x24, 0x24, 0x24),
    Color32::from_rgb(0x30, 0x30, 0x30),
    Color32::from_rgb(0xf2, 0xf2, 0xf2),
    Color32::from_rgb(0xb8, 0xb8, 0xb8),
    Color32::from_rgb(0x3f, 0x3f, 0x3f),
    Color32::from_rgb(0x78, 0xae, 0xed),
    Color32::from_rgb(0x5f, 0xbf, 0x78),
    Color32::from_rgb(0xf0, 0x73, 0x6a),
    Color32::from_rgb(0xe0, 0xb3, 0x41),
    Color32::from_rgb(0x1a, 0x1a, 0x1a),
    Color32::from_rgb(0x8a, 0x61, 0x00),
    Color32::from_rgb(0x15, 0x59, 0xb0),
];

/// The high-contrast substitution of section 5.1: pure black or white text and
/// a shade of each verdict, so contrast rises while the hue stays readable.
const HIGH_CONTRAST_LIGHT: [(Token, Color32); 6] = [
    (Token::Text, Color32::from_rgb(0x00, 0x00, 0x00)),
    (Token::Dim, Color32::from_rgb(0x00, 0x00, 0x00)),
    (Token::Border, Color32::from_rgb(0x00, 0x00, 0x00)),
    (Token::Accent, Color32::from_rgb(0x00, 0x3f, 0x8c)),
    (Token::Tampered, Color32::from_rgb(0x80, 0x00, 0x00)),
    (Token::Unverified, Color32::from_rgb(0x4d, 0x36, 0x00)),
];

/// The same substitution on the dark column.
const HIGH_CONTRAST_DARK: [(Token, Color32); 6] = [
    (Token::Text, Color32::from_rgb(0xff, 0xff, 0xff)),
    (Token::Dim, Color32::from_rgb(0xff, 0xff, 0xff)),
    (Token::Border, Color32::from_rgb(0xff, 0xff, 0xff)),
    (Token::Accent, Color32::from_rgb(0x9a, 0xc8, 0xff)),
    (Token::Tampered, Color32::from_rgb(0xff, 0xc2, 0xbd)),
    (Token::Unverified, Color32::from_rgb(0xff, 0xe0, 0x8a)),
];

/// The spacing scale of section 5.4, in points. The guideline asks for
/// increments of six, growing with the distance between two things, so this is
/// one base and four multiples of it rather than four unrelated numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Space {
    /// Between a control and its label, and between two controls in a group.
    Small,
    /// Between the window edge and its contents, and between groups.
    Medium,
    /// Between a header and the content it heads.
    Large,
    /// Between a zone and sections that belong to different things.
    ExtraLarge,
}

impl Space {
    /// Every value, so a test can walk the scale.
    pub const ALL: [Self; 4] = [Self::Small, Self::Medium, Self::Large, Self::ExtraLarge];

    /// The step this spacing is in the guideline's six-point scale.
    pub(crate) const fn steps(self) -> f32 {
        match self {
            Self::Small => 1.0,
            Self::Medium => 2.0,
            Self::Large => 3.0,
            Self::ExtraLarge => 4.0,
        }
    }

    /// The gap in points at 100% text size. The reader's scale multiplies it,
    /// because a window's spacing that ignored the text size would crowd a
    /// reader who asked for larger text.
    pub fn points(self) -> f32 {
        BASE_SPACING * self.steps()
    }
}

/// The step the spacing scale is built on, in points.
pub const BASE_SPACING: f32 = 6.0;

/// The sizes section 5.5 states for a control, in points before the reader's
/// scale. They are what makes a row of buttons look like one set.
pub const CONTROL_HEIGHT: f32 = 34.0;
/// The corner radius the guideline gives every control.
pub const CORNER_RADIUS: u8 = 6;
/// The gap between two icons or two words, as the guideline's smallest step.
pub const ITEM_GAP: f32 = 6.0;

/// The type scale of section 5.2 as (name, ratio). It is strictly increasing,
/// which a test asserts: a scale with one ratio out of order stops being one.
/// The toolkit's style this one is drawn through, where it has a name of its
/// own. A free function because the style itself belongs to no toolkit.
fn text_style(style: Style) -> TextStyle {
    match style {
        Style::Body => TextStyle::Body,
        Style::Heading => TextStyle::Heading,
        Style::Caption => TextStyle::Small,
        named => TextStyle::Name(named.name().into()),
    }
}

/// Which column a token is read from.
pub(crate) fn scheme(theme: Theme) -> Scheme {
    match theme {
        Theme::Light => Scheme::Light,
        Theme::Dark => Scheme::Dark,
    }
}

/// The substitutions high contrast makes on one column.
fn substitutions(scheme: Scheme) -> &'static [(Token, Color32)] {
    match scheme {
        Scheme::Light => &HIGH_CONTRAST_LIGHT,
        Scheme::Dark => &HIGH_CONTRAST_DARK,
    }
}

/// A token's colour in one scheme, with or without high contrast.
pub(crate) fn colour_in(scheme: Scheme, high_contrast: bool, token: Token) -> Color32 {
    if high_contrast
        && let Some((_, colour)) = substitutions(scheme).iter().find(|(t, _)| *t == token)
    {
        return *colour;
    }
    let table = match scheme {
        Scheme::Light => &LIGHT,
        Scheme::Dark => &DARK,
    };
    table[token as usize]
}

/// A token's colour in the scheme the context is currently drawing in.
pub fn colour(ctx: &egui::Context, high_contrast: bool, token: Token) -> Color32 {
    colour_in(scheme(ctx.theme()), high_contrast, token)
}

/// The relative luminance of a colour, as WCAG defines it. Only the tests
/// read it: a theme table is a small thing to get wrong quietly.
#[cfg(test)]
fn luminance(colour: Color32) -> f32 {
    let channel = |value: u8| {
        let value = f32::from(value) / 255.0;
        if value <= 0.039_28 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(colour.r()) + 0.7152 * channel(colour.g()) + 0.0722 * channel(colour.b())
}

/// The contrast ratio between two colours, from 1:1 to 21:1.
#[cfg(test)]
fn contrast(a: Color32, b: Color32) -> f32 {
    let (high, low) = (
        luminance(a).max(luminance(b)),
        luminance(a).min(luminance(b)),
    );
    (high + 0.05) / (low + 0.05)
}

/// The font a style is drawn in, at a text scale.
pub fn font(style: Style, scale: f32) -> FontId {
    FontId::new(style.points(scale), FontFamily::Proportional)
}

/// Push a scheme's tokens into an `egui` style, so every widget the zones build
/// draws from the table rather than from `egui`'s defaults.
fn dress(style: &mut egui::Style, scheme: Scheme, high_contrast: bool) {
    let token = |token: Token| colour_in(scheme, high_contrast, token);
    let visuals = &mut style.visuals;
    *visuals = match scheme {
        Scheme::Light => egui::Visuals::light(),
        Scheme::Dark => egui::Visuals::dark(),
    };
    visuals.panel_fill = token(Token::Panel);
    visuals.window_fill = token(Token::Panel);
    visuals.extreme_bg_color = token(Token::Surface);
    visuals.faint_bg_color = token(Token::Surface);
    visuals.override_text_color = None;
    visuals.window_stroke = egui::Stroke::new(1.0, token(Token::Border));
    visuals.selection.bg_fill = token(Token::Accent).gamma_multiply(0.4);
    let widgets = [
        &mut visuals.widgets.noninteractive,
        &mut visuals.widgets.inactive,
        &mut visuals.widgets.hovered,
        &mut visuals.widgets.active,
        &mut visuals.widgets.open,
    ];
    for widget in widgets {
        widget.bg_fill = token(Token::Panel);
        widget.weak_bg_fill = token(Token::Panel);
        widget.bg_stroke = egui::Stroke::new(1.0, token(Token::Border));
        widget.fg_stroke = egui::Stroke::new(1.0, token(Token::Text));
    }
    visuals.selection.stroke = egui::Stroke::new(1.0, token(Token::Text));
    visuals.hyperlink_color = token(Token::Accent);
}

/// Replace the `egui` style table with the window's own: one scheme's tokens
/// and the type scale at the reader's chosen size.
pub(crate) fn apply(ctx: &egui::Context, theme: Theme, text_scale: f32, high_contrast: bool) {
    let scheme = scheme(theme);
    // `set_theme` is deliberately not used: it would change the global theme
    // mid-pass and leave the active `Style` holding one theme's colours.
    ctx.style_mut_of(theme, |style| {
        dress(style, scheme, high_contrast);
        for style_name in Style::NAMED {
            style
                .text_styles
                .insert(text_style(style_name), font(style_name, text_scale));
        }
        style
            .text_styles
            .insert(TextStyle::Body, font(Style::Body, text_scale));
        style
            .text_styles
            .insert(TextStyle::Button, font(Style::Body, text_scale));
        style
            .text_styles
            .insert(TextStyle::Small, font(Style::Caption, text_scale));
        // A heading in `egui` is whatever the current theme calls one; naming
        // it here keeps the size from depending on the theme's defaults.
        style
            .text_styles
            .insert(TextStyle::Heading, font(Style::Heading, text_scale));
        let code = FontId::new(Style::Body.points(text_scale), FontFamily::Monospace);
        style.text_styles.insert(TextStyle::Monospace, code);
        dress_spacing(style, text_scale);
    });
}

/// The spacing scale and control sizes of sections 5.4 and 5.5, at the
/// reader's text size. `egui` spaces every row from `item_spacing`, so setting
/// it here is what puts the whole window on one six-point grid.
fn dress_spacing(style: &mut egui::Style, text_scale: f32) {
    let step = |space: Space| (space.points() * text_scale).round();
    let mut spacing = style.spacing.clone();
    spacing.item_spacing = egui::vec2(step(Space::Small), step(Space::Small));
    spacing.button_padding = egui::vec2(step(Space::Medium), step(Space::Small));
    spacing.interact_size.y = (CONTROL_HEIGHT * text_scale).round();
    spacing.menu_margin = egui::Margin::same(step(Space::Small) as i8);
    spacing.window_margin = egui::Margin::same(step(Space::Medium) as i8);
    style.spacing = spacing;
    for visuals in [
        &mut style.visuals.widgets.noninteractive,
        &mut style.visuals.widgets.inactive,
        &mut style.visuals.widgets.hovered,
        &mut style.visuals.widgets.active,
        &mut style.visuals.widgets.open,
    ] {
        visuals.corner_radius = egui::CornerRadius::same(CORNER_RADIUS);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every token, so a new one is covered by the contrast checks below.
    /// Every token a word can be drawn in. `Accent` and `Mark` are not among
    /// them: one is a selection fill and a link colour, the other an outline.
    const FOREGROUNDS: [Token; 5] = [
        Token::Text,
        Token::Dim,
        Token::Valid,
        Token::Tampered,
        Token::Unverified,
    ];

    const ALL_TOKENS: [Token; 12] = [
        Token::Surface,
        Token::Panel,
        Token::Text,
        Token::Dim,
        Token::Border,
        Token::Accent,
        Token::Valid,
        Token::Tampered,
        Token::Unverified,
        Token::Canvas,
        Token::Hit,
        Token::Mark,
    ];

    #[test]
    fn every_theme_defines_every_token() {
        for scheme in [Scheme::Light, Scheme::Dark] {
            for token in ALL_TOKENS {
                let colour = colour_in(scheme, false, token);
                assert_ne!(
                    colour,
                    Color32::TRANSPARENT,
                    "{token:?} is undefined in {scheme:?}"
                );
            }
        }
    }

    #[test]
    fn body_text_clears_the_readable_contrast() {
        // 4.5:1 is the level ordinary text is expected to reach. Every colour
        // that carries words is checked on every surface words land on, since
        // a token that passed on a panel is not thereby readable on the well.
        for scheme in [Scheme::Light, Scheme::Dark] {
            for token in FOREGROUNDS {
                for surface in [Token::Panel, Token::Surface, Token::Canvas] {
                    let ratio = contrast(
                        colour_in(scheme, false, token),
                        colour_in(scheme, false, surface),
                    );
                    assert!(
                        ratio >= 4.5,
                        "{token:?} on {surface:?} in {scheme:?} is {ratio:.1}:1"
                    );
                }
            }
        }
    }

    #[test]
    fn a_shape_on_a_page_clears_the_visible_contrast() {
        // A hit and an annotation boundary are drawn over the page, and the
        // page is white in every theme. 3:1 is what a shape must reach.
        for scheme in [Scheme::Light, Scheme::Dark] {
            for token in [Token::Hit, Token::Mark] {
                let ratio = contrast(colour_in(scheme, false, token), Color32::WHITE);
                assert!(
                    ratio >= 3.0,
                    "{token:?} on a page in {scheme:?} is {ratio:.1}:1"
                );
            }
        }
    }

    #[test]
    fn a_high_contrast_shape_clears_the_same_floor() {
        // A theme switch must not make a hit harder to find than it found it.
        for scheme in [Scheme::Light, Scheme::Dark] {
            for token in [Token::Hit, Token::Mark] {
                let ratio = contrast(colour_in(scheme, true, token), Color32::WHITE);
                assert!(
                    ratio >= 4.5,
                    "{token:?} on a page in high contrast {scheme:?} is {ratio:.1}:1"
                );
            }
        }
    }

    #[test]
    fn high_contrast_raises_every_pair_it_touches() {
        for scheme in [Scheme::Light, Scheme::Dark] {
            let panel = colour_in(scheme, true, Token::Panel);
            for (token, _) in substitutions(scheme) {
                if *token == Token::Border {
                    continue;
                }
                let plain = contrast(colour_in(scheme, false, *token), panel);
                let sharp = contrast(colour_in(scheme, true, *token), panel);
                assert!(
                    sharp > plain,
                    "{token:?} on {scheme:?} went {plain:.1} to {sharp:.1}"
                );
                assert!(sharp >= 7.0, "{token:?} on {scheme:?} is {sharp:.1}:1");
            }
        }
    }

    #[test]
    fn the_type_scale_never_goes_backwards() {
        let ratios: Vec<f32> = SCALE.iter().map(|(_, ratio)| *ratio).collect();
        for pair in ratios.windows(2) {
            assert!(pair[0] > pair[1], "the scale is not decreasing: {ratios:?}");
        }
    }

    #[test]
    fn the_spacing_scale_is_a_multiple_of_six() {
        for space in Space::ALL {
            let points = space.points();
            assert!(
                (points % BASE_SPACING).abs() < f32::EPSILON,
                "{space:?} is {points} pt, which is not a step of {BASE_SPACING}"
            );
        }
    }

    #[test]
    fn the_spacing_scale_only_grows() {
        let steps: Vec<f32> = Space::ALL.iter().map(|space| space.steps()).collect();
        for pair in steps.windows(2) {
            assert!(pair[0] < pair[1], "the scale is not increasing: {steps:?}");
        }
    }

    #[test]
    fn every_style_finds_its_ratio() {
        for style in Style::NAMED {
            assert!(
                SCALE.iter().any(|(name, _)| *name == style.name()),
                "{style:?} has no ratio"
            );
        }
        assert_eq!(Style::Body.points(1.0), BASE_POINTS);
        assert_eq!(Style::Body.points(1.3), (BASE_POINTS * 1.3).round());
    }

    #[test]
    fn a_named_style_is_drawn_through_its_own_entry() {
        let style = Style::Title1;
        assert!(matches!(text_style(style), TextStyle::Name(name) if &*name == "title-1"));
        assert_eq!(text_style(Style::Body), TextStyle::Body);
        assert_eq!(text_style(Style::Caption), TextStyle::Small);
    }
}
