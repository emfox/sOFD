//! What a shell needs to draw a document itself, rather than as a bitmap.
//!
//! A canvas takes path data, not geometry: `Path2D` in a browser, an `egui`
//! shape on the desktop. Handing that out is the difference between a shell
//! that reads the display list and one that reaches into the converter for it,
//! so the spelling lives here and the shells stay on one side of the facade.

/// A path as SVG `d` data, in millimetres.
///
/// The same string an SVG document carries, so a shell and an export cannot
/// disagree about where a line goes. Absolute segments only: a canvas has no
/// pen position to make a relative one mean anything.
#[cfg(feature = "convert")]
pub fn path_data(path: &sofd_geometry::Path) -> String {
    sofd_convert::svg::path_data(path)
}

/// The same, for a shell that only draws shapes.
#[cfg(feature = "render")]
pub fn shape_path_data(shape: &sofd_render::Shape) -> Option<String> {
    match shape {
        sofd_render::Shape::Path { path, .. }
        | sofd_render::Shape::Text { path, .. }
        | sofd_render::Shape::Clip { path, .. } => Some(path_data(path)),
        sofd_render::Shape::Image { .. } | sofd_render::Shape::ClipReset => None,
    }
}
