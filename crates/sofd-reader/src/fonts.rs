//! A CJK fallback for the interface font. egui ships latin and cyrillic
//! only, so a Chinese label would draw as tofu boxes without one. The face
//! comes from the host; a host without one still starts, in latin.

use egui::FontFamily;

/// Family names tried in order until one resolves on this host. The order is
/// preference, not correctness: each carries the CJK glyphs the interface
/// needs, and the first that is installed wins.
#[cfg(target_os = "windows")]
const CANDIDATES: &[&str] = &["Microsoft YaHei", "SimHei", "SimSun", "Microsoft JhengHei"];

/// Family names tried in order until one resolves on this host.
#[cfg(target_os = "macos")]
const CANDIDATES: &[&str] = &["PingFang SC", "Hiragino Sans GB", "STHeiti", "Heiti SC"];

/// Family names tried in order until one resolves on this host.
#[cfg(not(any(
    target_os = "windows",
    target_os = "macos",
    target_os = "android",
    target_arch = "wasm32"
)))]
const CANDIDATES: &[&str] = &[
    "Noto Sans CJK SC",
    "Noto Sans SC",
    "Source Han Sans SC",
    "WenQuanYi Zen Hei",
    "Droid Sans Fallback",
];

/// Read a CJK-capable system font's bytes and the face inside it, or `None`
/// when the host has none.
#[cfg(not(any(target_arch = "wasm32", target_os = "android")))]
fn load_cjk_font() -> Option<(Vec<u8>, u32)> {
    use font_kit::family_name::FamilyName;
    use font_kit::handle::Handle;
    use font_kit::properties::Properties;
    use font_kit::source::SystemSource;

    let source = SystemSource::new();
    for family in CANDIDATES {
        let name = FamilyName::Title((*family).to_string());
        let Ok(handle) = source.select_best_match(&[name], &Properties::new()) else {
            continue;
        };
        let found = match handle {
            Handle::Path {
                path, font_index, ..
            } => std::fs::read(path).ok().map(|bytes| (bytes, font_index)),
            Handle::Memory {
                bytes, font_index, ..
            } => Some(((*bytes).clone(), font_index)),
        };
        if let Some(found) = found {
            tracing::debug!(family, "using a system font for CJK labels");
            return Some(found);
        }
    }
    tracing::debug!("no CJK system font found; labels stay latin-only");
    None
}

/// Android reads the device's own faces through the font crate, so the label
/// face is one a document font resolves against too.
#[cfg(target_os = "android")]
fn load_cjk_font() -> Option<(Vec<u8>, u32)> {
    sofd::cjk_face()
}

/// The browser resolves its own fonts, so there is nothing to read here.
#[cfg(target_arch = "wasm32")]
fn load_cjk_font() -> Option<(Vec<u8>, u32)> {
    None
}

/// Append a host CJK face to the default families as a fallback. The default
/// faces stay first, so latin text is unchanged; returns whether one landed.
pub fn install_cjk_fallback(ctx: &egui::Context) -> bool {
    let Some((bytes, face)) = load_cjk_font() else {
        return false;
    };

    let mut definitions = egui::FontDefinitions::default();
    let name = "cjk-fallback".to_owned();
    let mut data = egui::FontData::from_owned(bytes);
    // A `.ttc` holds several faces under one path, and egui draws face zero
    // unless told otherwise; the CJK one is rarely index 0.
    data.index = face;
    definitions
        .font_data
        .insert(name.clone(), std::sync::Arc::new(data));
    for family in [FontFamily::Proportional, FontFamily::Monospace] {
        definitions
            .families
            .entry(family)
            .or_default()
            .push(name.clone());
    }
    ctx.set_fonts(definitions);
    true
}
