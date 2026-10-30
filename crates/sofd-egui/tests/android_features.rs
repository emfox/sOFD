//! What the Android build asks `eframe` for. A device has no Wayland or X
//! session, so a feature that names one compiles cleanly and then exits when
//! the library loads; only the manifest says which set was chosen, and nothing
//! else in the suite reads it.

use std::path::PathBuf;

/// The manifest text, so a feature list is read where it is written rather
/// than through a parse that could hide a second table for the same target.
fn manifest() -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    std::fs::read_to_string(&path).expect("the reader's manifest is readable")
}

/// The features one `[target.'cfg(...)'.dependencies.eframe]` table names.
fn eframe_features(text: &str, target: &str) -> Option<Vec<String>> {
    let header = format!("[target.'cfg({target})'.dependencies.eframe]");
    // The exact header, so the desktop table is not mistaken for the Android
    // one whose pattern appears inside it.
    let line = text.lines().position(|line| line.trim() == header)?;
    let body: Vec<&str> = text
        .lines()
        .skip(line + 1)
        .take_while(|line| !line.trim_start().starts_with('['))
        .collect();
    let body = body.join("\n");
    let after = body.split("features = [").nth(1)?;
    let list = after.split(']').next()?;
    Some(
        list.split(',')
            .map(|item| item.trim().trim_matches('"'))
            .filter(|item| !item.is_empty())
            .map(str::to_owned)
            .collect(),
    )
}

#[test]
fn the_android_build_names_no_desktop_session() {
    let text = manifest();
    let features = eframe_features(&text, r#"target_os = "android""#)
        .expect("Android names its own eframe features");
    for absent in ["wayland", "x11"] {
        assert!(
            !features.iter().any(|feature| feature == absent),
            "Android asked for `{absent}`, which no device provides: {features:?}"
        );
    }
    assert!(
        features
            .iter()
            .any(|feature| feature == "android-native-activity"),
        "Android has to name the activity backend that starts it: {features:?}"
    );
}

/// Both renderers, because a device cannot be asked which it will accept.
///
/// An emulator's software Vulkan builds a wgpu device and then refuses the
/// pipelines the frame needs, and a build that named wgpu alone would have
/// nothing to fall back to: the activity would open and then show nothing.
#[test]
fn the_android_build_carries_a_second_renderer() {
    let text = manifest();
    let features = eframe_features(&text, r#"target_os = "android""#)
        .expect("Android names its own eframe features");
    for present in ["glow", "wgpu"] {
        assert!(
            features.iter().any(|feature| feature == present),
            "Android cannot fall back without `{present}`: {features:?}"
        );
    }
}

#[test]
fn the_desktop_build_keeps_the_session_it_runs_in() {
    let text = manifest();
    let features = eframe_features(&text, r#"not(target_os = "android")"#)
        .expect("the desktop names its own eframe features");
    for present in ["wayland", "x11"] {
        assert!(
            features.iter().any(|feature| feature == present),
            "the desktop lost `{present}`, so the reader cannot open a window there: {features:?}"
        );
    }
}

/// What the empty window says on a device.
///
/// Android has no drop target and no file dialog, so the desktop hint tells a
/// reader to do two things that cannot be done there. The hint is chosen by
/// target rather than by a runtime check, so only the label that would be
/// drawn is asserted against, and the other is kept out of the way.
#[test]
fn the_system_without_a_dialog_is_told_what_it_can_do() {
    let hint = sofd_egui::App::new().drop_hint();
    assert!(!hint.is_empty());
    #[cfg(target_os = "android")]
    {
        assert!(
            !hint.contains("Drop") && !hint.contains("Open File"),
            "the hint names an action a device does not have: {hint}"
        );
    }
    #[cfg(not(target_os = "android"))]
    assert!(
        hint.contains("Drop"),
        "the desktop hint names the drop target: {hint}"
    );
}
