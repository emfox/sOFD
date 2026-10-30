//! The glyphs, handed to the webview as the theme's own path data.
//!
//! A canvas fills a glyph correctly with the nonzero rule, so the front end
//! builds one `Path2D` per icon and fills it with `currentColor`. What it must
//! not do is draw its own: this is the same `sofd-icons` the other shell reads.

use serde::Serialize;
use sofd_icons::Icon;

/// One glyph: what it does, the theme's name for it, and its paths.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Glyph {
    /// The variant's name, which the front end uses as its key: `Open`,
    /// `Next`, `FitWidth`.
    pub id: &'static str,
    /// The theme's name, so a reviewer can compare it with the file it came
    /// from.
    pub theme_name: &'static str,
    /// One `d` per path element, in the order the theme lists them.
    pub paths: &'static [&'static str],
}

/// The whole set, and the square it is drawn on.
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlyphSet {
    /// The grid every glyph is drawn on, in its own units.
    pub size: f32,
    /// Every glyph.
    pub glyphs: Vec<Glyph>,
}

/// The set, as the front end takes it.
pub fn glyphs() -> GlyphSet {
    // The names come from `sofd-icons` so this shell and the egui one cannot
    // drift: the front end keys its canvas by them.
    let (size, icons) = Icon::wire_set();
    GlyphSet {
        size,
        glyphs: icons
            .into_iter()
            .map(|glyph| Glyph {
                id: glyph.id,
                theme_name: glyph.theme_name,
                paths: glyph.paths,
            })
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every glyph reaches the window, and two of them never share a name: the
    /// name is the key the front end looks one up by.
    #[test]
    fn every_glyph_reaches_the_window_under_its_own_name() {
        let set = glyphs();
        assert_eq!(set.glyphs.len(), Icon::ALL.len());
        assert_eq!(set.size, 16.0, "the theme draws on a 16-unit square");
        let mut seen = std::collections::HashSet::new();
        for glyph in &set.glyphs {
            assert!(seen.insert(glyph.id), "{} is claimed twice", glyph.id);
            assert!(!glyph.paths.is_empty(), "{} carries no path", glyph.id);
            assert!(
                glyph.theme_name.ends_with("-symbolic"),
                "{} is not a theme name",
                glyph.theme_name
            );
        }
    }
}
