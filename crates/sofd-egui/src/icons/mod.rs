//! The symbolic icon set, drawn from the path data the desktop's icon theme
//! ships for the same names. Colours come from the window, so a glyph follows a
//! theme switch and nothing is rasterized until a frame asks for it.

mod path;

use std::sync::OnceLock;

use eframe::egui::{self, Color32, Pos2, Rect, Response, Sense, Shape, Vec2};

use crate::theme::{self, Token};
use path::Subpath;

/// The square every glyph is drawn on, before the reader's scale.
pub const SIZE: f32 = 16.0;

/// One of the window's icons. Each is the theme's own drawing of the same idea,
/// so a reader who knows the theme already knows these.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Icon {
    /// Open a file.
    Open,
    /// Close the open document.
    Close,
    /// Search the document.
    Search,
    /// The signature report.
    Signatures,
    /// The annotation list.
    Annotations,
    /// The main menu: three bars, as the theme draws it.
    Menu,
    /// The previous page or entry.
    Previous,
    /// The next page or entry.
    Next,
    /// One page at a time.
    SinglePage,
    /// Two pages facing.
    FacingPages,
    /// The document scrolling.
    Continuous,
    /// Fit the width.
    FitWidth,
    /// Fit the whole page.
    FitPage,
    /// Print.
    Print,
    /// Write a PDF.
    ExportPdf,
    /// Write PNG files.
    ExportPng,
    /// A signature that holds.
    Valid,
    /// Tampered.
    Tampered,
    /// A proof nothing can confirm.
    Unverified,
    /// A sound proof whose signer is not trusted.
    Untrusted,
}

impl Icon {
    /// Every icon, so a test can draw all of them.
    pub const ALL: [Self; 20] = [
        Self::Open,
        Self::Close,
        Self::Search,
        Self::Signatures,
        Self::Annotations,
        Self::Menu,
        Self::Previous,
        Self::Next,
        Self::SinglePage,
        Self::FacingPages,
        Self::Continuous,
        Self::FitWidth,
        Self::FitPage,
        Self::Print,
        Self::ExportPdf,
        Self::ExportPng,
        Self::Valid,
        Self::Tampered,
        Self::Unverified,
        Self::Untrusted,
    ];

    /// The name of the theme icon this glyph is. It is also the name of the
    /// file the path data was taken from, which is how a reviewer checks it.
    pub fn theme_name(self) -> &'static str {
        match self {
            Self::Open => "document-open-symbolic",
            Self::Close => "window-close-symbolic",
            Self::Search => "edit-find-symbolic",
            Self::Signatures => "application-certificate-symbolic",
            Self::Annotations => "view-list-symbolic",
            Self::Menu => "open-menu-symbolic",
            Self::Previous => "go-previous-symbolic",
            Self::Next => "go-next-symbolic",
            Self::SinglePage => "view-paged-symbolic",
            Self::FacingPages => "view-dual-symbolic",
            Self::Continuous => "view-continuous-symbolic",
            Self::FitWidth => "zoom-fit-best-symbolic",
            Self::FitPage => "zoom-in-symbolic",
            Self::Print => "document-print-symbolic",
            Self::ExportPdf => "document-save-symbolic",
            Self::ExportPng => "image-x-generic-symbolic",
            Self::Valid => "emblem-ok-symbolic",
            Self::Tampered => "dialog-error-symbolic",
            Self::Unverified => "dialog-question-symbolic",
            Self::Untrusted => "dialog-warning-symbolic",
        }
    }

    /// The themed paths this one is drawn from, in the order the theme's file
    /// lists them. Each is one `<path>` element's `d`, and nothing else.
    fn paths(self) -> &'static [&'static str] {
        match self {
            Self::Open => &[OPEN_0, OPEN_1],
            Self::Close => &[CLOSE_0],
            Self::Search => &[SEARCH_0],
            Self::Signatures => &[CERTIFICATE_0],
            Self::Annotations => &[LIST_0],
            Self::Menu => &[MENU_0, MENU_1, MENU_2],
            Self::Previous => &[PREVIOUS_0],
            Self::Next => &[NEXT_0],
            Self::SinglePage => &[PAGED_0, PAGED_1, PAGED_2],
            Self::FacingPages => &[DUAL_0, DUAL_1],
            Self::Continuous => &[CONTINUOUS_0, CONTINUOUS_1],
            Self::FitWidth => &[FIT_WIDTH_0],
            Self::FitPage => &[FIT_PAGE_0],
            Self::Print => &[PRINT_0],
            Self::ExportPdf => &[SAVE_0],
            Self::ExportPng => &[IMAGE_0, IMAGE_1],
            Self::Valid => &[OK_0],
            Self::Tampered => &[ERROR_0],
            Self::Unverified => &[QUESTION_0],
            Self::Untrusted => &[WARNING_0],
        }
    }

    /// The drawing as it is stored, read once and kept for the process.
    fn drawing(self) -> &'static Drawing {
        static CACHE: OnceLock<Vec<Drawing>> = OnceLock::new();
        let all = CACHE.get_or_init(|| Icon::ALL.iter().map(|icon| icon.read()).collect());
        &all[Icon::ALL
            .iter()
            .position(|icon| *icon == self)
            .expect("every icon is in ALL")]
    }

    /// Read this glyph's paths, and note the ones the theme draws faded: the
    /// parts that only hint at a second page behind the first.
    fn read(self) -> Drawing {
        let mut drawing = Drawing::default();
        for text in self.paths() {
            let shade = if text.contains(FADED) { 0.349 } else { 1.0 };
            let subpaths = path::parse(text)
                .unwrap_or_else(|e| panic!("{} does not read: {e:?}", self.theme_name()));
            drawing
                .parts
                .extend(subpaths.into_iter().map(|s| (s, shade)));
        }
        drawing
    }

    /// The glyph's outlines as closed rings of points on the 16-unit square,
    /// curves spelled out as the short straight steps a fill can count.
    fn flattened(self) -> Vec<(Vec<Pos2>, f32)> {
        self.drawing()
            .parts
            .iter()
            .map(|(subpath, shade)| {
                let mut ring = vec![Pos2::new(subpath.start.0, subpath.start.1)];
                for segment in &subpath.segments {
                    match *segment {
                        path::Segment::Line(to) => ring.push(Pos2::new(to.0, to.1)),
                        path::Segment::Cubic(c1, c2, to) => {
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
}

/// One glyph's paths, each with the strength the theme draws it at.
#[derive(Debug, Default)]
struct Drawing {
    parts: Vec<(Subpath, f32)>,
}

/// The `opacity` an SVG marks a path the theme draws faded with.
const FADED: &str = "fill-opacity=\"0.349\"";

/// `document-open-symbolic`: a folder with a sheet lifting out of it.
const OPEN_0: &str = "m 3 1 c -1.644531 0 -3 1.355469 -3 3 v 8 c 0 1.644531 1.355469 3 3 3 h 8.882812 c 0.832032 0 1.578126 -0.402344 2.054688 -0.9375 c 0.472656 -0.53125 0.738281 -1.167969 0.910156 -1.800781 l 0.972656 -2.609375 c 0.390626 -1.449219 -0.09375 -2.652344 -0.820312 -3.167969 c -0.484375 -0.34375 -0.714844 -0.292969 -1 -0.324219 v -1.160156 c 0 -0.855469 -0.558594 -1.589844 -1.09375 -1.828125 c -0.53125 -0.238281 -1.011719 -0.167969 -1.011719 -0.167969 l 0.105469 -0.003906 h -3.585938 l -1.707031 -1.707031 c -0.1875 -0.1875 -0.441406 -0.292969 -0.707031 -0.292969 z m 0 2 h 2.585938 l 1.707031 1.707031 c 0.1875 0.1875 0.441406 0.292969 0.707031 0.292969 h 4 c 0.035156 0 0.070312 -0.003906 0.105469 -0.007812 c 0 0 0.019531 0.019531 -0.011719 0.003906 c -0.035156 -0.011719 -0.09375 -0.25 -0.09375 0.003906 v 2 c 0 0.550781 0.449219 1 1 1 c 1 0 1.046875 0.703125 0.886719 1.128906 l -0.972657 2.609375 c -0.117187 0.4375 -0.296874 0.800781 -0.472656 0.996094 c -0.175781 0.199219 -0.285156 0.265625 -0.558594 0.265625 h -8.882812 c -0.570312 0 -1 -0.429688 -1 -1 v -8 c 0 -0.570312 0.46875 -0.792969 1 -1 z";
const OPEN_1: &str = "m 7 6 l 0.042969 0.003906 c -0.914063 -0.042968 -1.75 0.390625 -2.195313 0.96875 c -0.710937 1.222656 -1.15625 2.277344 -1.800781 3.71875 c -0.171875 0.523438 0.117187 1.089844 0.640625 1.261719 c 0.527344 0.171875 1.09375 -0.117187 1.261719 -0.640625 c 0.488281 -1.011719 0.921875 -1.816406 1.339843 -2.808594 c 0.210938 -0.503906 0.703126 -0.492187 0.898438 -0.503906 h 5.8125 c 0.550781 0 1 -0.449219 1 -1 c 0 -0.550781 -0.449219 -1 -1 -1 z";

/// `window-close-symbolic`: two bars crossing.
const CLOSE_0: &str = "m 4 4 h 1 h 0.03125 c 0.253906 0.011719 0.511719 0.128906 0.6875 0.3125 l 2.28125 2.28125 l 2.3125 -2.28125 c 0.265625 -0.230469 0.445312 -0.304688 0.6875 -0.3125 h 1 v 1 c 0 0.285156 -0.035156 0.550781 -0.25 0.75 l -2.28125 2.28125 l 2.25 2.25 c 0.1875 0.1875 0.28125 0.453125 0.28125 0.71875 v 1 h -1 c -0.265625 0 -0.53125 -0.09375 -0.71875 -0.28125 l -2.28125 -2.28125 l -2.28125 2.28125 c -0.1875 0.1875 -0.453125 0.28125 -0.71875 0.28125 h -1 v -1 c 0 -0.265625 0.09375 -0.53125 0.28125 -0.71875 l 2.28125 -2.25 l -2.28125 -2.28125 c -0.210938 -0.195312 -0.304688 -0.46875 -0.28125 -0.75 z";

/// `edit-find-symbolic`: a magnifier over the page.
const SEARCH_0: &str = "m 6.5 0 c -3.578125 0 -6.5 2.921875 -6.5 6.5 c 0 3.578125 2.921875 6.5 6.5 6.5 c 1.429688 0 2.753906 -0.46875 3.828125 -1.257812 l 2.945313 2.945312 c 0.957031 0.9375 2.363281 -0.5 1.40625 -1.4375 l -2.929688 -2.929688 c 0.785156 -1.074218 1.25 -2.394531 1.25 -3.820312 c 0 -3.578125 -2.921875 -6.5 -6.5 -6.5 z m 0 2 c 2.496094 0 4.5 2.003906 4.5 4.5 c 0 2.496094 -2.003906 4.5 -4.5 4.5 c -2.496094 0 -4.5 -2.003906 -4.5 -4.5 c 0 -2.496094 2.003906 -4.5 4.5 -4.5 z";

/// `application-certificate-symbolic`: a seal hanging from a certificate.
const CERTIFICATE_0: &str = "m 3 0 c -1.644531 0 -3 1.355469 -3 3 v 7 c 0 1.644531 1.355469 3 3 3 h 3 v -2 h -3 c -0.554688 0 -1 -0.445312 -1 -1 v -7 c 0 -0.570312 0.429688 -1 1 -1 h 10 c 0.570312 0 1 0.429688 1 1 v 7 c 0 0.554688 -0.445312 1 -1 1 h -1 v 2 h 1 c 1.644531 0 3 -1.355469 3 -3 v -7 c 0 -1.644531 -1.355469 -3 -3 -3 z m 6 4 c -1.65625 0 -3 1.34375 -3 3 c 0 0.851562 0.363281 1.664062 1 2.234375 v 6.765625 l 2 -1.5 l 2 1.5 v -6.765625 c 0.636719 -0.570313 1 -1.382813 1 -2.234375 c 0 -1.65625 -1.34375 -3 -3 -3 z";

/// `view-list-symbolic`: three markers down the side, three lines beside them.
const LIST_0: &str = "m 2 1 c -0.554688 0 -1 0.445312 -1 1 v 2 c 0 0.554688 0.445312 1 1 1 h 2 c 0.554688 0 1 -0.445312 1 -1 v -2 c 0 -0.554688 -0.445312 -1 -1 -1 z m 5 1 v 2 h 8 v -2 z m -5 4.015625 c -0.554688 0 -1 0.445313 -1 1 v 1.980469 c 0 0.550781 0.445312 1 1 1 h 2 c 0.554688 0 1 -0.449219 1 -1 v -1.980469 c 0 -0.554687 -0.445312 -1 -1 -1 z m 5 0.984375 v 2 h 8 v -2 z m -5 4 c -0.554688 0 -1 0.445312 -1 1 v 1.980469 c 0 0.550781 0.445312 1 1 1 h 2 c 0.554688 0 1 -0.449219 1 -1 v -1.980469 c 0 -0.554688 -0.445312 -1 -1 -1 z m 5 0.984375 v 2 h 8 v -2 z";

/// `open-menu-symbolic`: three bars.
const MENU_0: &str = "m 1 2 h 14 v 2 h -14 z";
const MENU_1: &str = "m 1 7 h 14 v 2 h -14 z";
const MENU_2: &str = "m 1 12 h 14 v 2 h -14 z";

/// `go-previous-symbolic`: a chevron pointing back.
const PREVIOUS_0: &str = "m 12 2 c 0 -0.265625 -0.105469 -0.519531 -0.292969 -0.707031 c -0.390625 -0.390625 -1.023437 -0.390625 -1.414062 0 l -6 6 c -0.1875 0.1875 -0.292969 0.441406 -0.292969 0.707031 c 0 0.265625 0.105469 0.519531 0.292969 0.707031 l 6 6 c 0.390625 0.390625 1.023437 0.390625 1.414062 0 c 0.1875 -0.1875 0.292969 -0.441406 0.292969 -0.707031 c 0 -0.265625 -0.105469 -0.519531 -0.292969 -0.707031 l -5.292969 -5.292969 l 5.292969 -5.292969 c 0.1875 -0.1875 0.292969 -0.441406 0.292969 -0.707031 z";

/// `go-next-symbolic`: the same chevron, pointing on.
const NEXT_0: &str = "m 4 2 c 0 -0.265625 0.105469 -0.519531 0.292969 -0.707031 c 0.390625 -0.390625 1.023437 -0.390625 1.414062 0 l 6 6 c 0.1875 0.1875 0.292969 0.441406 0.292969 0.707031 c 0 0.265625 -0.105469 0.519531 -0.292969 0.707031 l -6 6 c -0.390625 0.390625 -1.023437 0.390625 -1.414062 0 c -0.1875 -0.1875 -0.292969 -0.441406 -0.292969 -0.707031 c 0 -0.265625 0.105469 -0.519531 0.292969 -0.707031 l 5.292969 -5.292969 l -5.292969 -5.292969 c -0.1875 -0.1875 -0.292969 -0.441406 -0.292969 -0.707031 z";

/// `view-paged-symbolic`: one sheet, a second behind it, and two text lines.
const PAGED_0: &str = "m 9 5.5 v -1 c 0 -0.277344 -0.222656 -0.5 -0.5 -0.5 h -3 c -0.277344 0 -0.5 0.222656 -0.5 0.5 v 1 c 0 0.277344 0.222656 0.5 0.5 0.5 h 3 c 0.277344 0 0.5 -0.222656 0.5 -0.5 z";
const PAGED_1: &str = "m 4 0 c -1.644531 0 -3 1.355469 -3 3 v 7 c 0 1.644531 1.355469 3 3 3 h 6 c 1.644531 0 3 -1.355469 3 -3 v -7 c 0 -1.644531 -1.355469 -3 -3 -3 z m 0 2 h 6 c 0.570312 0 1 0.429688 1 1 v 7 c 0 0.570312 -0.429688 1 -1 1 h -6 c -0.570312 0 -1 -0.429688 -1 -1 v -7 c 0 -0.570312 0.429688 -1 1 -1 z m 11 2 c -0.550781 0 -1 0.449219 -1 1 v 8 c 0 0.550781 -0.449219 1 -1 1 h -7 c -0.550781 0 -1 0.449219 -1 1 c 0 0.550781 0.449219 1 1 1 h 7 c 1.65625 0 3 -1.34375 3 -3 v -8 c 0 -0.550781 -0.449219 -1 -1 -1 z";
const PAGED_2: &str = "m 9 8.5 v -1 c 0 -0.277344 -0.222656 -0.5 -0.5 -0.5 h -3 c -0.277344 0 -0.5 0.222656 -0.5 0.5 v 1 c 0 0.277344 0.222656 0.5 0.5 0.5 h 3 c 0.277344 0 0.5 -0.222656 0.5 -0.5 z";

/// `view-dual-symbolic`: two sheets side by side, lines on each.
const DUAL_0: &str = "m 3.5 5 c -0.277344 0 -0.5 0.222656 -0.5 0.5 v 1 c 0 0.277344 0.222656 0.5 0.5 0.5 h 2 c 0.277344 0 0.5 -0.222656 0.5 -0.5 v -1 c 0 -0.277344 -0.222656 -0.5 -0.5 -0.5 z m 7 0 c -0.277344 0 -0.5 0.222656 -0.5 0.5 v 1 c 0 0.277344 0.222656 0.5 0.5 0.5 h 2 c 0.277344 0 0.5 -0.222656 0.5 -0.5 v -1 c 0 -0.277344 -0.222656 -0.5 -0.5 -0.5 z m -7 3 c -0.277344 0 -0.5 0.222656 -0.5 0.5 v 1 c 0 0.277344 0.222656 0.5 0.5 0.5 h 2 c 0.277344 0 0.5 -0.222656 0.5 -0.5 v -1 c 0 -0.277344 -0.222656 -0.5 -0.5 -0.5 z m 7 0 c -0.277344 0 -0.5 0.222656 -0.5 0.5 v 1 c 0 0.277344 0.222656 0.5 0.5 0.5 h 2 c 0.277344 0 0.5 -0.222656 0.5 -0.5 v -1 c 0 -0.277344 -0.222656 -0.5 -0.5 -0.5 z";
const DUAL_1: &str = "m 3 1 c -1.644531 0 -3 1.355469 -3 3 v 8 c 0 1.644531 1.355469 3 3 3 h 10 c 1.644531 0 3 -1.355469 3 -3 v -8 c 0 -1.644531 -1.355469 -3 -3 -3 z m 0 2 h 4 v 10 h -4 c -0.570312 0 -1 -0.429688 -1 -1 v -8 c 0 -0.570312 0.429688 -1 1 -1 z m 6 0 h 4 c 0.570312 0 1 0.429688 1 1 v 8 c 0 0.570312 -0.429688 1 -1 1 h -4 z";

/// `view-continuous-symbolic`: a band scrolling, marked at both ends.
const CONTINUOUS_0: &str = "m 5.5 0 c -0.277344 0 -0.5 0.222656 -0.5 0.5 v 1 c 0 0.277344 0.222656 0.5 0.5 0.5 h 5 c 0.277344 0 0.5 -0.222656 0.5 -0.5 v -1 c 0 -0.277344 -0.222656 -0.5 -0.5 -0.5 z m 0 3 c -0.277344 0 -0.5 0.222656 -0.5 0.5 v 1 c 0 0.277344 0.222656 0.5 0.5 0.5 h 5 c 0.277344 0 0.5 -0.222656 0.5 -0.5 v -1 c 0 -0.277344 -0.222656 -0.5 -0.5 -0.5 z m 0 8 c -0.277344 0 -0.5 0.222656 -0.5 0.5 v 1 c 0 0.277344 0.222656 0.5 0.5 0.5 h 5 c 0.277344 0 0.5 -0.222656 0.5 -0.5 v -1 c 0 -0.277344 -0.222656 -0.5 -0.5 -0.5 z m 0 3 c -0.277344 0 -0.5 0.222656 -0.5 0.5 v 1 c 0 0.277344 0.222656 0.5 0.5 0.5 h 5 c 0.277344 0 0.5 -0.222656 0.5 -0.5 v -1 c 0 -0.277344 -0.222656 -0.5 -0.5 -0.5 z";
const CONTINUOUS_1: &str = "m 15 7 h -14 v 2 h 14 z";

/// `zoom-fit-best-symbolic`: four corner brackets reaching for the edges.
const FIT_WIDTH_0: &str = "m 4 1 c -1.660156 0 -3 1.339844 -3 3 v 8 c 0 1.660156 1.339844 3 3 3 h 8 c 1.660156 0 3 -1.339844 3 -3 v -8 c 0 -1.660156 -1.339844 -3 -3 -3 z m 0 2 h 3.011719 v 2 h -2 v 2 h -2 l -0.011719 -3 c 0 -0.128906 0.027344 -0.253906 0.074219 -0.375 c 0.003906 -0.003906 0.003906 -0.007812 0.007812 -0.011719 c 0.046875 -0.117187 0.117188 -0.21875 0.203125 -0.308593 c 0.007813 -0.007813 0.011719 -0.011719 0.019532 -0.019532 c 0.089843 -0.085937 0.191406 -0.15625 0.308593 -0.203125 c 0.003907 -0.003906 0.007813 -0.007812 0.015625 -0.007812 c 0.117188 -0.046875 0.242188 -0.074219 0.371094 -0.074219 z m 5 0 h 3.011719 c 0.125 0 0.253906 0.027344 0.371093 0.074219 c 0.003907 0.003906 0.011719 0.003906 0.015626 0.007812 c 0.113281 0.046875 0.21875 0.117188 0.304687 0.203125 c 0.007813 0.007813 0.015625 0.011719 0.019531 0.019532 c 0.089844 0.089843 0.160156 0.191406 0.207032 0.308593 c 0 0.003907 0.003906 0.007813 0.007812 0.015625 c 0.046875 0.117188 0.070312 0.242188 0.074219 0.371094 l -0.011719 3 h -2 v -2 h -2 z m -5.988281 6 h 2 v 2 h 2 v 2 h -3.011719 c -0.128906 0 -0.253906 -0.027344 -0.375 -0.074219 c -0.003906 -0.003906 -0.007812 -0.003906 -0.011719 -0.007812 c -0.117187 -0.046875 -0.21875 -0.117188 -0.308593 -0.203125 c -0.007813 -0.007813 -0.011719 -0.011719 -0.019532 -0.019532 c -0.085937 -0.089843 -0.15625 -0.191406 -0.203125 -0.308593 c -0.003906 -0.003907 -0.007812 -0.007813 -0.007812 -0.015625 c -0.046875 -0.117188 -0.074219 -0.242188 -0.074219 -0.371094 z m 7.988281 0 h 2 l 0.011719 3 c -0.003907 0.128906 -0.027344 0.253906 -0.074219 0.375 c -0.003906 0.003906 -0.003906 0.007812 -0.007812 0.011719 c -0.046876 0.117187 -0.117188 0.21875 -0.203126 0.308593 c -0.007812 0.007813 -0.015624 0.011719 -0.019531 0.019532 c -0.089843 0.085937 -0.195312 0.15625 -0.308593 0.203125 c -0.003907 0.003906 -0.011719 0.007812 -0.015626 0.007812 c -0.117187 0.046875 -0.246093 0.074219 -0.371093 0.074219 h -3.011719 v -2 h 2 z";

/// `zoom-in-symbolic`: a magnifier over a plus, on the same frame.
const FIT_PAGE_0: &str = "m 4 1 c -1.660156 0 -3 1.339844 -3 3 v 8 c 0 1.660156 1.339844 3 3 3 h 8 c 1.660156 0 3 -1.339844 3 -3 v -8 c 0 -1.660156 -1.339844 -3 -3 -3 z m 3 3 h 2 v 3 h 3 v 2 h -3 v 3 h -2 v -3 h -3 v -2 h 3 z";

/// `document-print-symbolic`: a sheet fed through a printer.
const PRINT_0: &str = "m 5 1 c -0.550781 0 -1 0.449219 -1 1 v 1 h 8 v -1 c 0 -0.550781 -0.449219 -1 -1 -1 z m -3 3 c -1.109375 0 -2 0.890625 -2 2 v 4 c 0 1.109375 0.890625 2 2 2 v 1 c 0 1.089844 0.910156 2 2 2 h 8 c 1.089844 0 2 -0.910156 2 -2 v -1 c 1.109375 0 2 -0.890625 2 -2 v -4 c 0 -1.109375 -0.890625 -2 -2 -2 z m 2 6 h 8 v 3 h -8 z";

/// `document-save-symbolic`: an arrow down onto a tray.
const SAVE_0: &str = "m 8 0 c -0.550781 0 -1 0.449219 -1 1 v 8.585938 l -1.292969 -1.292969 c -0.1875 -0.1875 -0.441406 -0.292969 -0.707031 -0.292969 c -0.265625 0 -0.519531 0.105469 -0.707031 0.292969 c -0.390625 0.390625 -0.390625 1.023437 0 1.414062 l 3 3 c 0.390625 0.390625 1.023437 0.390625 1.414062 0 l 3 -3 c 0.390625 -0.390625 0.390625 -1.023437 0 -1.414062 c -0.390625 -0.390625 -1.023437 -0.390625 -1.414062 0 l -1.292969 1.292969 v -8.585938 c 0 -0.550781 -0.449219 -1 -1 -1 z m -7 14 v 2 h 14 v -2 z";

/// `image-x-generic-symbolic`: a picture in a frame.
const IMAGE_0: &str = "m 6 5 c -0.550781 0 -1 0.449219 -1 1 c 0 0.550781 0.449219 1 1 1 c 0.550781 0 1 -0.449219 1 -1 c 0 -0.550781 -0.449219 -1 -1 -1 z m 3.5 3 l -2 2 l -1.5 -1 l -2 2 v 0.5 c 0 0.5 0.5 0.5 0.5 0.5 h 7 c 0 0 0.472656 -0.035156 0.5 -0.5 v -1 z";
const IMAGE_1: &str = "m 4 1 c -1.644531 0 -3 1.355469 -3 3 v 8 c 0 1.644531 1.355469 3 3 3 h 8 c 1.644531 0 3 -1.355469 3 -3 v -8 c 0 -1.644531 -1.355469 -3 -3 -3 z m 0 2 h 8 c 0.570312 0 1 0.429688 1 1 v 8 c 0 0.570312 -0.429688 1 -1 1 h -8 c -0.570312 0 -1 -0.429688 -1 -1 v -8 c 0 -0.570312 0.429688 -1 1 -1 z";

/// `emblem-ok-symbolic`: a tick, and no box around it.
const OK_0: &str = "m 13.753906 4.660156 c 0.175782 -0.199218 0.261719 -0.460937 0.246094 -0.726562 c -0.019531 -0.265625 -0.140625 -0.511719 -0.339844 -0.6875 c -0.199218 -0.175782 -0.460937 -0.261719 -0.726562 -0.246094 c -0.265625 0.019531 -0.511719 0.140625 -0.6875 0.339844 l -6.296875 7.195312 l -2.242188 -2.242187 c -0.390625 -0.390625 -1.023437 -0.390625 -1.414062 0 c -0.1875 0.1875 -0.292969 0.441406 -0.292969 0.707031 c 0 0.265625 0.105469 0.519531 0.292969 0.707031 l 3 3 c 0.195312 0.195313 0.464843 0.304688 0.738281 0.292969 c 0.277344 -0.007812 0.539062 -0.132812 0.722656 -0.339844 z";

/// `dialog-error-symbolic`: a filled disc with a bar cut out of it.
const ERROR_0: &str = "m 8 1 c -3.871094 0 -7 3.128906 -7 7 c 0 3.871094 3.128906 7 7 7 c 3.871094 0 7 -3.128906 7 -7 c 0 -3.871094 -3.128906 -7 -7 -7 z m -4 6 h 8 v 2 h -8 z";

/// `dialog-question-symbolic`: a diamond asking a question.
const QUESTION_0: &str = "m 8 0.03125 c -0.398438 0 -0.816406 0.160156 -1.125 0.46875 l -6.375 6.375 c -0.621094 0.621094 -0.621094 1.628906 0 2.25 l 6.375 6.375 c 0.621094 0.621094 1.628906 0.621094 2.25 0 l 6.375 -6.375 c 0.621094 -0.621094 0.621094 -1.628906 0 -2.25 l -6.375 -6.375 c -0.308594 -0.308594 -0.726562 -0.46875 -1.125 -0.46875 z m 0.152344 2.976562 c 0.726562 0.035157 1.433594 0.335938 1.96875 0.871094 c 0.855468 0.855469 1.113281 2.152344 0.652344 3.269532 c -0.332032 0.796874 -0.988282 1.390624 -1.773438 1.671874 v 1.179688 h -2 v -2 c 0 -0.550781 0.449219 -1 1 -1 c 0.40625 0 0.769531 -0.242188 0.925781 -0.617188 c 0.152344 -0.375 0.070313 -0.800781 -0.21875 -1.089843 c -0.289062 -0.289063 -0.714843 -0.371094 -1.089843 -0.21875 c -0.375 0.15625 -0.617188 0.519531 -0.617188 0.925781 h -2 c 0 -1.210938 0.734375 -2.308594 1.851562 -2.769531 c 0.417969 -0.175781 0.863282 -0.246094 1.300782 -0.222657 z m -0.152344 7.992188 c 0.550781 0 1 0.449219 1 1 c 0 0.550781 -0.449219 1 -1 1 c -0.550781 0 -1 -0.449219 -1 -1 c 0 -0.550781 0.449219 -1 1 -1 z";

/// `dialog-warning-symbolic`: a triangle with an exclaimed line and dot.
const WARNING_0: &str = "m 7.90625 0.09375 c -0.527344 -0.027344 -1.039062 0.28125 -1.4375 0.96875 l -6.25 11.59375 c -0.535156 0.964844 0.046875 2.34375 1.09375 2.34375 h 13.15625 c 0.980469 0 1.902344 -1.160156 1.21875 -2.34375 l -6.3125 -11.53125 c -0.398438 -0.644531 -0.941406 -1.003906 -1.46875 -1.03125 z m 1.09375 3.90625 v 5 c 0.007812 0.527344 -0.472656 1 -1 1 c -0.527344 0 -1.007812 -0.472656 -1 -1 v -5 z m -1 7 c 0.550781 0 1 0.449219 1 1 c 0 0.550781 -0.449219 1 -1 1 c -0.550781 0 -1 -0.449219 -1 -1 c 0 -0.550781 0.449219 -1 1 -1 z";

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
    let paths = icon.flattened();
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
