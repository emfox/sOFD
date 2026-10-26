//! The device's own font files.
//!
//! `font-kit` binds the desktop font stacks and has no backend for Android, so
//! the faces are read here instead. What a file holds is written in the file: a
//! collection keeps several faces under one path, each naming its own family,
//! and the one carrying Chinese is rarely the first.

use std::path::{Path, PathBuf};
use std::sync::Arc;

/// The most faces one file is read for. A collection holds a handful; a single
/// face file refuses every index but zero, which ends the walk.
const MAX_FACES: u32 = 32;

/// One face of one file. The bytes are shared by every face a collection holds.
///
/// The walk below and the tests read every field; only the lookup in
/// `manager.rs` exists on a device, so a desktop build sees less of this type.
#[cfg_attr(
    not(all(feature = "system-fonts", target_os = "android")),
    allow(dead_code)
)]
#[derive(Clone)]
pub struct Face {
    /// The family the face declares for itself.
    pub family: String,
    /// The file, read once.
    pub bytes: Arc<Vec<u8>>,
    /// Where the file is, which names the copy a caller may ask for.
    pub path: PathBuf,
    /// Which face inside the file.
    pub index: u32,
    /// Declared bold, which a caller asking for a bold run prefers.
    pub bold: bool,
    /// Declared slanted.
    pub italic: bool,
}

/// Every face in `paths`, skipping files that are absent or unreadable.
pub fn read<'a>(paths: impl IntoIterator<Item = &'a Path>) -> Vec<Face> {
    let mut faces = Vec::new();
    for path in paths {
        let Ok(data) = std::fs::read(path) else {
            continue;
        };
        let data = Arc::new(data);
        for index in 0..MAX_FACES {
            let Some((family, bold, italic)) = describe(&data, index) else {
                break;
            };
            faces.push(Face {
                family,
                bytes: Arc::clone(&data),
                path: path.to_path_buf(),
                index,
                bold,
                italic,
            });
        }
    }
    faces
}

/// The name and shape a face declares, or `None` when the file has no such face.
fn describe(data: &[u8], index: u32) -> Option<(String, bool, bool)> {
    let font = skrifa::FontRef::from_index(data, index).ok()?;
    let family = family_name(&font)?;
    let attributes = skrifa::attribute::Attributes::new(&font);
    let bold = attributes.weight.value() >= 600.0;
    let italic = !matches!(attributes.style, skrifa::attribute::Style::Normal);
    Some((family, bold, italic))
}

/// The name a face calls itself, the typographic one first: that is the name a
/// collection uses to tell its scripts apart, where the plain family is shared.
fn family_name(font: &skrifa::FontRef<'_>) -> Option<String> {
    use skrifa::raw::TableProvider;

    let name = font.name().ok()?;
    let storage = name.string_data();
    let mut plain = None;
    for record in name.name_record() {
        let id = record.name_id().to_u16();
        if id != 1 && id != 16 {
            continue;
        }
        let Ok(text) = record.string(storage) else {
            continue;
        };
        let text = text.to_string();
        let text = text.trim();
        if text.is_empty() {
            continue;
        }
        if id == 16 {
            return Some(text.to_string());
        }
        plain = Some(text.to_string());
    }
    plain
}

/// Whether the face draws every character given.
pub fn covers(face: &Face, wanted: &[u32]) -> bool {
    let Ok(font) = skrifa::FontRef::from_index(face.bytes.as_slice(), face.index) else {
        return false;
    };
    use skrifa::MetadataProvider;
    let charmap = font.charmap();
    wanted
        .iter()
        .all(|code| char::from_u32(*code).is_some_and(|ch| charmap.map(ch).is_some()))
}

/// One face of a file as a font file of its own.
///
/// The engine names a face by its bytes, and a collection holds several under
/// one path, so the face asked for is copied out: the tables are the same, only
/// where each one starts changes.
pub fn standalone(data: &[u8], index: u32) -> Option<Vec<u8>> {
    let base = if data.get(..4)? == b"ttcf" {
        let count = u32::from_be_bytes(data.get(8..12)?.try_into().ok()?) as usize;
        if index as usize >= count {
            return None;
        }
        let at = 12 + index as usize * 4;
        u32::from_be_bytes(data.get(at..at + 4)?.try_into().ok()?) as usize
    } else if index == 0 {
        0
    } else {
        return None;
    };
    let count = u16::from_be_bytes(data.get(base + 4..base + 6)?.try_into().ok()?) as usize;
    let records = base + 12;
    let mut tables = Vec::with_capacity(count);
    for table in 0..count {
        let at = records + table * 16;
        let record = data.get(at..at + 16)?;
        let offset = u32::from_be_bytes(record.get(8..12)?.try_into().ok()?) as usize;
        let length = u32::from_be_bytes(record.get(12..16)?.try_into().ok()?) as usize;
        let bytes = data.get(offset..offset.checked_add(length)?)?;
        tables.push((record, bytes));
    }
    // Every offset in the directory is measured from the start of the file, so
    // where each table lands is decided before any of them is written.
    let mut placed = Vec::with_capacity(count);
    let mut body = 12 + count * 16;
    for (_, bytes) in &tables {
        placed.push(body);
        body += bytes.len();
        body = (body + 3) & !3;
    }
    let mut out = Vec::with_capacity(body);
    out.extend_from_slice(data.get(base..base + 12)?);
    for ((record, _), at) in tables.iter().zip(&placed) {
        out.extend_from_slice(&record[..8]);
        out.extend_from_slice(&(*at as u32).to_be_bytes());
        out.extend_from_slice(&record[12..16]);
    }
    for ((_, bytes), at) in tables.iter().zip(&placed) {
        out.resize(*at, 0);
        out.extend_from_slice(bytes);
    }
    Some(out)
}

/// The face as a file of its own, copied once however many callers ask.
#[cfg_attr(
    not(all(feature = "system-fonts", target_os = "android")),
    allow(dead_code)
)]
pub fn as_file(face: &Face) -> Option<Arc<Vec<u8>>> {
    /// A copy, named by the face it was copied from.
    type Copy = ((PathBuf, u32), Arc<Vec<u8>>);

    static COPIES: std::sync::OnceLock<std::sync::Mutex<Vec<Copy>>> = std::sync::OnceLock::new();
    let copies = COPIES.get_or_init(|| std::sync::Mutex::new(Vec::new()));
    let key = (face.path.clone(), face.index);
    let mut held = copies.lock().ok()?;
    if let Some((_, bytes)) = held.iter().find(|(seen, _)| *seen == key) {
        return Some(Arc::clone(bytes));
    }
    let bytes = Arc::new(standalone(face.bytes.as_slice(), face.index)?);
    held.push((key, Arc::clone(&bytes)));
    Some(bytes)
}

/// Where Android keeps the faces a document is most likely to name. A file the
/// system does not have is skipped; what each carries is read from the file.
#[cfg(target_os = "android")]
const SYSTEM_FILES: &[&str] = &[
    "/system/fonts/NotoSansCJK-Regular.ttc",
    "/system/fonts/NotoSerifCJK-Regular.ttc",
    "/system/fonts/DroidSansFallback.ttf",
    "/system/fonts/NotoSansCJK-Bold.ttc",
    "/system/fonts/NotoSerifCJK-Bold.ttc",
    "/system/fonts/Roboto-Regular.ttf",
    "/system/fonts/DroidSansMono.ttf",
];

/// The faces this device lists, read once.
#[cfg(target_os = "android")]
pub fn faces() -> &'static [Face] {
    static FACES: std::sync::OnceLock<Vec<Face>> = std::sync::OnceLock::new();
    FACES.get_or_init(|| read(SYSTEM_FILES.iter().map(Path::new)))
}

/// A face that draws Chinese, for a caller that only needs one face: the
/// simplified-Chinese one when a collection names it, any Han glyph otherwise.
#[cfg(target_os = "android")]
pub fn cjk_face() -> Option<(Vec<u8>, u32)> {
    let faces = faces();
    // U+4E2D, the Han character for "middle": a face that draws it draws text.
    let han = 0x4E2D;
    let chosen = faces
        .iter()
        .find(|face| face.family.ends_with("SC") && covers(face, &[han]))
        .or_else(|| faces.iter().find(|face| covers(face, &[han])))?;
    Some((chosen.bytes.as_ref().clone(), chosen.index))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn data(name: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests")
            .join("data")
            .join(name)
    }

    #[test]
    fn every_face_of_a_collection_is_named() {
        let faces = read([data("sofd-collection.ttc").as_path()]);
        assert!(faces.len() > 1, "a collection holds several faces");
        assert!(faces.iter().all(|face| !face.family.is_empty()));
    }

    #[test]
    fn a_face_says_which_characters_it_carries() {
        let faces = read([data("sofd-cjk-subset.ttf").as_path()]);
        assert_eq!(faces.len(), 1);
        assert!(covers(&faces[0], &[0x4E2D]));
    }

    #[test]
    fn a_file_that_is_not_there_is_skipped() {
        assert!(read([Path::new("/nowhere/absent.ttf")]).is_empty());
    }

    #[test]
    fn a_face_copied_out_of_a_collection_opens_on_its_own() {
        let path = data("sofd-collection.ttc");
        let whole = std::fs::read(&path).expect("the fixture is readable");
        for face in read([path.as_path()]) {
            let single = standalone(&whole, face.index).expect("the face can be copied out");
            let alone = describe(&single, 0).expect("the copy opens");
            assert_eq!(alone.0, face.family);
        }
    }
}
