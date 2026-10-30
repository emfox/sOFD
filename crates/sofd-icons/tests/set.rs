//! What the set must be true of, checked without a toolkit: every glyph has
//! data, every piece of that data reads, and no two glyphs claim one name.

use std::collections::HashSet;

use sofd_icons::{Drawing, Icon, SIZE, path};

/// Every icon resolves to at least one path, and every path parses.
#[test]
fn every_icon_carries_path_data_that_reads() {
    for icon in Icon::ALL {
        let paths = icon.paths();
        assert!(
            !paths.is_empty(),
            "{} has no path data at all",
            icon.theme_name()
        );
        for text in paths {
            assert!(
                !text.trim().is_empty(),
                "{} has an empty path",
                icon.theme_name()
            );
            path::parse(text)
                .unwrap_or_else(|e| panic!("{} does not read: {e:?}", icon.theme_name()));
        }
    }
}

/// Two icons wearing one name is the bug that makes a reviewer check the wrong
/// file, so the theme name is the key a glyph is looked up by.
#[test]
fn the_theme_names_are_distinct() {
    let mut seen = HashSet::new();
    for icon in Icon::ALL {
        assert!(
            seen.insert(icon.theme_name()),
            "{} is claimed twice",
            icon.theme_name()
        );
    }
}

/// A glyph that reads to nothing is a button a reader cannot find.
#[test]
fn every_icon_draws_something() {
    for icon in Icon::ALL {
        let drawing: &Drawing = icon.drawing();
        assert!(
            !drawing.parts.is_empty(),
            "{} read to an empty drawing",
            icon.theme_name()
        );
        let points: usize = drawing
            .parts
            .iter()
            .map(|(subpath, _)| subpath.segments.len())
            .sum();
        assert!(
            points > 0,
            "{} has subpaths but no segments",
            icon.theme_name()
        );
    }
}

/// The drawing stays on the square the theme draws it on. A path that leaves
/// it would be clipped by a canvas and silently lose part of the glyph.
#[test]
fn every_point_stays_on_the_grid() {
    for icon in Icon::ALL {
        for (subpath, _) in &icon.drawing().parts {
            let mut points = vec![subpath.start];
            for segment in &subpath.segments {
                match *segment {
                    path::Segment::Line(to) => points.push(to),
                    path::Segment::Cubic(c1, c2, to) => points.extend([c1, c2, to]),
                }
            }
            for (x, y) in points {
                assert!(
                    (-0.5..=SIZE + 0.5).contains(&x) && (-0.5..=SIZE + 0.5).contains(&y),
                    "{} reaches ({x}, {y}), outside the {SIZE}-unit square",
                    icon.theme_name()
                );
            }
        }
    }
}

/// A path the theme draws faded says so in its own `d`, and reading it is what
/// tells a canvas to lay the hint down lighter than the glyph. No glyph carries
/// one today, so this asserts the rule rather than its presence: whatever share
/// comes back is a share of full ink. A path marked at 0 — invisible, and
/// indistinguishable from a piece that was lost — is what it catches.
#[test]
fn a_subpath_is_drawn_at_some_share_of_full_ink() {
    for icon in Icon::ALL {
        for (_, shade) in &icon.drawing().parts {
            assert!(
                *shade > 0.0 && *shade <= 1.0,
                "{} came back at {shade}, which is not a share of ink",
                icon.theme_name()
            );
        }
    }
    for icon in Icon::ALL {
        for text in icon.paths() {
            if text.contains("fill-opacity") {
                assert!(
                    icon.drawing().parts.iter().any(|(_, s)| *s < 1.0),
                    "{} marks a faded path the reader did not read as faded",
                    icon.theme_name()
                );
            }
        }
    }
}
