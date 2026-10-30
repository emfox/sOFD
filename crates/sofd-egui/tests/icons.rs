//! The icon set, drawn the way a frame draws it. The checks are that the stored
//! path data reads, that every glyph lands inside its grid, and that no two of
//! them are the same drawing.

use eframe::egui::epaint::{ClippedShape, Mesh, Primitive, Tessellator};
use eframe::egui::{self, Rect};
use sofd_egui::icons::{self, Icon};

/// The edge a glyph is drawn at in these checks.
const BOX: f32 = 64.0;

/// The square a glyph is drawn on, as a frame gives it.
fn box_rect() -> Rect {
    Rect::from_min_size(egui::pos2(16.0, 16.0), egui::vec2(BOX, BOX))
}

/// Every shape a glyph produced when painted, and the mesh they tessellated to.
fn drawn(icon: Icon) -> (Vec<egui::Shape>, Mesh) {
    let mut harness = egui_kittest::Harness::builder()
        .with_size(egui::vec2(96.0, 96.0))
        .build_ui(|ui| {
            icons::paint(ui.painter(), box_rect(), icon, egui::Color32::WHITE);
        });
    harness.run();
    let shapes: Vec<egui::Shape> = harness
        .output()
        .shapes
        .iter()
        .map(|s| s.shape.clone())
        .collect();
    let mut tessellator = Tessellator::new(1.0, Default::default(), [16, 16], Vec::new());
    let clipped: Vec<ClippedShape> = shapes
        .iter()
        .cloned()
        .map(|shape| ClippedShape {
            clip_rect: Rect::EVERYTHING,
            shape,
        })
        .collect();
    let mut mesh = Mesh::default();
    for primitive in tessellator.tessellate_shapes(clipped) {
        if let Primitive::Mesh(part) = primitive.primitive {
            let offset = mesh.vertices.len() as u32;
            mesh.vertices.extend(part.vertices);
            mesh.indices.extend(part.indices.iter().map(|i| i + offset));
        }
    }
    (shapes, mesh)
}

/// The bounding box of every point a glyph drew.
fn bounds(shapes: &[egui::Shape]) -> Option<Rect> {
    let mut rect: Option<Rect> = None;
    for shape in shapes {
        if let egui::Shape::Path(path) = shape {
            for point in &path.points {
                rect = Some(match rect {
                    None => Rect::from_min_max(*point, *point),
                    Some(current) => current.union(Rect::from_min_max(*point, *point)),
                });
            }
        }
    }
    rect
}

#[test]
fn every_icon_is_a_filled_drawing() {
    for icon in Icon::ALL {
        let (_, mesh) = drawn(icon);
        assert!(
            mesh.indices.len() >= 9,
            "{} tessellated to {} indices",
            icon.theme_name(),
            mesh.indices.len()
        );
    }
}

#[test]
fn every_icon_stays_in_its_grid() {
    // The glyphs are the theme's own, drawn on the 16-unit grid, so the whole
    // drawing sits inside it, with a point of slack for rounding.
    let allowed = box_rect().expand(1.0);
    for icon in Icon::ALL {
        let (shapes, _) = drawn(icon);
        let bounds =
            bounds(&shapes).unwrap_or_else(|| panic!("{} drew nothing", icon.theme_name()));
        assert!(
            allowed.contains_rect(bounds),
            "{} left its grid: {bounds:?}",
            icon.theme_name()
        );
    }
}

#[test]
fn every_icon_fills_a_share_of_its_grid() {
    // A glyph that collapsed to a dot would still pass the test above, so this
    // one asks for a drawing worth putting in a button: it spans a third of the
    // square either way, which is what a bar of three does.
    for icon in Icon::ALL {
        let (shapes, _) = drawn(icon);
        let bounds = bounds(&shapes).expect("drew something");
        assert!(
            bounds.width() > BOX * 0.3 && bounds.height() > BOX * 0.3,
            "{} is too small: {bounds:?}",
            icon.theme_name()
        );
    }
}

#[test]
fn no_two_icons_are_the_same_drawing() {
    for (index, icon) in Icon::ALL.iter().enumerate() {
        for other in &Icon::ALL[index + 1..] {
            let (mine, _) = drawn(*icon);
            let (theirs, _) = drawn(*other);
            assert_ne!(
                format!("{mine:?}"),
                format!("{theirs:?}"),
                "{} and {} are the same drawing",
                icon.theme_name(),
                other.theme_name()
            );
        }
    }
}

#[test]
fn the_theme_names_are_distinct() {
    let mut names: Vec<&str> = Icon::ALL.iter().map(|icon| icon.theme_name()).collect();
    names.sort_unstable();
    let count = names.len();
    names.dedup();
    assert_eq!(count, names.len(), "two icons claim the same theme name");
}

/// An SVG path as drawing commands, so one spelling can be compared with
/// another: a shorthand counts as the long form it stands for.
fn commands(d: &str) -> Vec<(char, Vec<f64>)> {
    let mut out: Vec<(char, Vec<f64>)> = Vec::new();
    let mut letter = ' ';
    let mut number = String::new();
    let flush = |letter: char, number: &mut String, out: &mut Vec<(char, Vec<f64>)>| {
        if let Ok(value) = number.parse::<f64>() {
            if let Some(last) = out.last_mut().filter(|(l, _)| *l == letter) {
                last.1.push(value);
            } else {
                out.push((letter, vec![value]));
            }
        }
        number.clear();
    };
    for c in d.chars() {
        if c.is_ascii_alphabetic() {
            // A letter ends the number before it, but the first number may run
            // straight on from it: `m3.99 1c-1.66 0` leaves no room to spare.
            flush(letter, &mut number, &mut out);
            letter = c;
        } else if c == ',' || c.is_whitespace() {
            flush(letter, &mut number, &mut out);
        } else if c == '-' && !number.is_empty() && !number.ends_with('e') {
            // A minus that follows a number starts the next one: no separator
            // is needed, and the theme leans on that to keep its files small.
            flush(letter, &mut number, &mut out);
            number.push(c);
        } else {
            number.push(c);
        }
    }
    flush(letter, &mut number, &mut out);
    out
}

/// The points a path walks through, whatever letters it spells them with, as
/// the fill would see them: every curve is cut into the points it passes close
/// to, so a `c` and the `s` that stands for it walk the same way.
fn points(d: &str) -> Vec<f64> {
    let mut out = Vec::new();
    let mut at = (0.0_f64, 0.0_f64);
    let mut start = at;
    for (letter, values) in commands(d) {
        let step = match letter.to_ascii_uppercase() {
            'M' | 'L' | 'T' => 2,
            'H' | 'V' => 1,
            'C' => 6,
            'S' | 'Q' => 4,
            'Z' => 0,
            _ => 0,
        };
        if step == 0 {
            if letter == 'z' || letter == 'Z' {
                at = start;
            }
            continue;
        }
        for (index, chunk) in values.chunks(step).enumerate() {
            if chunk.len() < step {
                break;
            }
            // A second pair after `m` is a line, the same as `l`.
            let run = match (index, letter) {
                (0, _) => letter,
                (_, 'M') => 'L',
                (_, 'm') => 'l',
                _ => letter,
            };
            let absolute = run.is_ascii_uppercase();
            let place = |x: f64, y: f64| {
                if absolute {
                    (x, y)
                } else {
                    (at.0 + x, at.1 + y)
                }
            };
            let taken = match run.to_ascii_uppercase() {
                // A curve and its shorthands only agree on where they land, and
                // that is the point the path is on afterwards.
                'C' => place(chunk[4], chunk[5]),
                'S' | 'Q' => place(chunk[2], chunk[3]),
                'T' => place(chunk[0], chunk[1]),
                'H' => (if absolute { chunk[0] } else { at.0 + chunk[0] }, at.1),
                'V' => (at.0, if absolute { chunk[0] } else { at.1 + chunk[0] }),
                'M' => {
                    start = place(chunk[0], chunk[1]);
                    start
                }
                _ => place(chunk[0], chunk[1]),
            };
            at = taken;
            out.push(at.0);
            out.push(at.1);
        }
    }
    out
}

/// Whether `whole` walks the same points as `part`, as far as `part` goes.
fn starts_with(whole: &[f64], part: &[f64]) -> bool {
    whole.len() >= part.len() && part.iter().zip(whole).all(|(a, b)| (a - b).abs() < 0.02)
}

#[test]
fn every_glyph_matches_the_icon_it_is_named_for() {
    // The path data is copied from the installed theme, so where the theme is
    // here the copy is checked against it: a name it does not ship reads as
    // absent, and so does a drawing borrowed from the file next door.
    let dir = std::path::Path::new("/usr/share/icons/Adwaita/symbolic");
    if !dir.is_dir() {
        eprintln!("no theme installed; the copy cannot be checked here");
        return;
    }
    for icon in Icon::ALL {
        let name = icon.theme_name();
        // The theme keeps a name in one of several contexts, and the file with
        // that name is the drawing whichever context it sits in.
        let found = std::fs::read_dir(dir)
            .expect("the theme reads")
            .filter_map(|part| part.ok())
            .map(|part| part.path().join(format!("{name}.svg")))
            .find(|path| path.is_file());
        let path = found.unwrap_or_else(|| panic!("{name} is not a file the theme ships"));
        let source = std::fs::read_to_string(&path).expect("the icon file reads");
        let in_file: Vec<Vec<f64>> = source
            .split("d=\"")
            .skip(1)
            .map(|part| points(part.split('"').next().unwrap_or("")))
            .filter(|path| !path.is_empty())
            .collect();
        // A shorthand is spelled out in full here, so compare the points a
        // path walks through rather than the letters it uses. The theme also
        // closes by repeating the start, which the reader leaves off.
        for (index, d) in icons::path_data(icon).iter().enumerate() {
            let mine = points(d);
            let mine = &mine[..mine.len().saturating_sub(2)];
            assert!(
                in_file.iter().any(|path| starts_with(path, mine)),
                "{name}: path {index} is not in {}",
                path.display()
            );
        }
    }
}

#[test]
fn a_glyph_becomes_a_texture() {
    let ctx = egui::Context::default();
    for icon in Icon::ALL {
        let handle = icons::texture(&ctx, icon, egui::Color32::WHITE);
        let size = handle.size();
        assert!(
            size[0] >= 32 && size[1] >= 32,
            "{} made a {size:?} texture",
            icon.theme_name()
        );
    }
}
