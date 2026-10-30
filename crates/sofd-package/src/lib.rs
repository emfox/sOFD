//! OFD container layer.
//! An OFD file is a ZIP archive with `OFD.xml` as its entry point. This crate
//! opens that archive and hands out entries as bytes; it never interprets XML.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use std::collections::HashSet;
use std::io::Read;
use std::path::{Path, PathBuf};

use sofd_error::{OfdError, PackageError, Result};
use sofd_io::{ByteSource, DEFAULT_MAX_ENTRY_COUNT, DEFAULT_MAX_ENTRY_SIZE};

pub mod entry;

pub use entry::{EntryInfo, EntryKind};

/// Archive path of the document entry point, mandated by GB/T 33190-2016.
pub const ENTRY_POINT: &str = "OFD.xml";

/// Directory holding per-document subtrees.
pub const DOCS_PREFIX: &str = "Doc_";

/// Limits applied while opening an archive.
/// OFD files are routinely attacker-supplied, so every unbounded quantity gets a
/// ceiling rather than trusting the archive header.
#[derive(Debug, Clone, Copy)]
pub struct PackageLimits {
    /// Largest decompressed size accepted for one entry.
    pub max_entry_size: u64,
    /// Largest number of entries accepted.
    pub max_entry_count: usize,
    /// Largest total decompressed size accepted across all entries.
    pub max_total_size: u64,
}

impl Default for PackageLimits {
    fn default() -> Self {
        Self {
            max_entry_size: DEFAULT_MAX_ENTRY_SIZE,
            max_entry_count: DEFAULT_MAX_ENTRY_COUNT,
            max_total_size: 2 * 1024 * 1024 * 1024,
        }
    }
}

impl PackageLimits {
    /// Limits suitable for documents from an untrusted source.
    pub fn strict() -> Self {
        Self {
            max_entry_size: 64 * 1024 * 1024,
            max_entry_count: 4096,
            max_total_size: 256 * 1024 * 1024,
        }
    }

    /// Limits effectively off, for fuzzing and tests over known-good inputs.
    pub fn unbounded() -> Self {
        Self {
            max_entry_size: u64::MAX,
            max_entry_count: usize::MAX,
            max_total_size: u64::MAX,
        }
    }
}

/// An opened OFD container.
/// The reader stays alive for the container's lifetime because `zip` resolves
/// entries lazily against the central directory.
pub struct Package {
    archive: zip::ZipArchive<std::io::Cursor<Vec<u8>>>,
    limits: PackageLimits,
    /// Entry paths in archive order, captured once so callers need not re-walk.
    paths: Vec<String>,
    /// Whether any entry is ZIP-encrypted, captured at open because reading the
    /// central directory is what `&mut self` would otherwise be needed for.
    encrypted: bool,
}

impl std::fmt::Debug for Package {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Package")
            .field("entries", &self.paths.len())
            .field("limits", &self.limits)
            .finish()
    }
}

impl Package {
    /// Open a container from any byte source.
    pub fn open(source: &dyn ByteSource) -> Result<Self> {
        Self::open_with_limits(source, PackageLimits::default())
    }

    /// Open a container with explicit limits.
    pub fn open_with_limits(source: &dyn ByteSource, limits: PackageLimits) -> Result<Self> {
        // The archive is buffered rather than streamed: every OFD read touches the
        // central directory first, and owning the bytes keeps `Package` free of
        // the source's lifetime so callers can drop it immediately.
        let data = source.read_all()?;
        let cursor = std::io::Cursor::new(data);
        let archive = zip::ZipArchive::new(cursor).map_err(map_zip_error)?;
        Self::from_archive(archive, limits)
    }

    /// Open a container from a byte slice, the common case for tests and uploads.
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        let source = sofd_io::SliceSource::new(data, "package");
        Self::open(&source)
    }

    fn from_archive(
        mut archive: zip::ZipArchive<std::io::Cursor<Vec<u8>>>,
        limits: PackageLimits,
    ) -> Result<Self> {
        let count = archive.len();
        if count > limits.max_entry_count {
            return Err(PackageError::EntryCountLimit {
                actual: count,
                limit: limits.max_entry_count,
            }
            .into());
        }

        let mut paths = Vec::with_capacity(count);
        let mut seen = HashSet::with_capacity(count);
        let mut encrypted = false;

        for i in 0..count {
            // Raw access reads the central directory only: it neither
            // decompresses nor needs a password, so an encrypted entry is
            // recorded rather than refused at open time.
            let file = archive.by_index_raw(i).map_err(map_zip_error)?;
            encrypted |= file.encrypted();

            // Reject traversal rather than sanitising: a container that tries it
            // is malformed or hostile, and rewriting the path would hide that
            // from every layer above.
            let owned = match file.enclosed_name() {
                Some(p) => p,
                None => {
                    return Err(PackageError::IllegalPath {
                        path: file
                            .name()
                            .map(|name| name.to_string())
                            .unwrap_or_else(|_| "<unreadable>".to_string()),
                    }
                    .into());
                }
            };
            let path = normalize(&owned);

            // Duplicate names are legal in ZIP but ambiguous for OFD lookups, so
            // the first occurrence wins and the collision is reported.
            if seen.insert(path.clone()) {
                paths.push(path);
            } else {
                tracing::warn!(path, "duplicate archive entry, keeping the first");
            }
        }

        Ok(Self {
            archive,
            limits,
            paths,
            encrypted,
        })
    }

    /// Entry paths in archive order, directories excluded.
    pub fn entries(&self) -> &[String] {
        &self.paths
    }

    /// Number of entries.
    pub fn len(&self) -> usize {
        self.paths.len()
    }

    /// Whether the archive holds no entries.
    pub fn is_empty(&self) -> bool {
        self.paths.is_empty()
    }

    /// Limits in force for this container.
    pub fn limits(&self) -> PackageLimits {
        self.limits
    }

    /// Whether any entry carries ZIP-level encryption. Archival profiles forbid
    /// it, so the check is a property of the container rather than a read.
    pub fn is_encrypted(&self) -> bool {
        self.encrypted
    }

    /// Whether a path exists in the archive.
    /// Entries are named without a leading slash, but producers write paths
    /// from the root, so `/Doc_0/x.xml` and `Doc_0/x.xml` name one entry.
    pub fn contains(&self, path: &str) -> bool {
        let path = strip_root(path);
        self.paths.iter().any(|p| p == path) || self.match_ignoring_case(path).is_some()
    }

    /// Find the entry a path names, ignoring case when nothing matches exactly.
    /// Archives disagree with themselves: one stores `Doc_0/Document.xml` beside
    /// `DOC_0/Res/Image_4.JPEG`. Exact match wins, so two spellings still work.
    fn match_ignoring_case(&self, path: &str) -> Option<&str> {
        self.paths
            .iter()
            .find(|p| p.eq_ignore_ascii_case(path))
            .map(String::as_str)
    }

    /// Read one entry in full.
    pub fn read(&mut self, path: &str) -> Result<Vec<u8>> {
        let path = strip_root(path);
        let path = match self.paths.iter().any(|p| p == path) {
            true => path.to_string(),
            false => self
                .match_ignoring_case(path)
                .ok_or_else(|| {
                    sofd_error::OfdError::from(PackageError::MissingEntry {
                        entry: path.to_string(),
                    })
                })?
                .to_string(),
        };
        let mut file = self.archive.by_name(&path).map_err(|e| match e {
            zip::result::ZipError::FileNotFound => PackageError::MissingEntry {
                entry: path.to_string(),
            }
            .into(),
            other => map_zip_error(other),
        })?;

        // Read behind a hard ceiling rather than trusting the declared size: a
        // crafted header can understate it and inflate past the limit.
        let limit = self.limits.max_entry_size;
        let declared = file.size();
        if declared > limit {
            return Err(PackageError::DecompressionLimit {
                actual: declared,
                limit,
            }
            .into());
        }

        let mut buf = Vec::with_capacity(declared as usize);
        file.by_ref().take(limit + 1).read_to_end(&mut buf)?;
        if buf.len() as u64 > limit {
            return Err(PackageError::DecompressionLimit {
                actual: buf.len() as u64,
                limit,
            }
            .into());
        }
        Ok(buf)
    }

    /// Read one entry as text, rejecting invalid UTF-8.
    /// OFD XML is UTF-8 by specification, so a failure here means corruption.
    pub fn read_string(&mut self, path: &str) -> Result<String> {
        let bytes = self.read(path)?;
        String::from_utf8(bytes).map_err(|e| {
            PackageError::Corrupted {
                reason: format!("{path} is not valid UTF-8: {e}"),
            }
            .into()
        })
    }

    /// Read an entry, or `None` when it is absent.
    pub fn read_optional(&mut self, path: &str) -> Result<Option<Vec<u8>>> {
        if !self.contains(path) {
            return Ok(None);
        }
        self.read(path).map(Some)
    }

    /// Whether the archive has the mandated entry point.
    pub fn has_entry_point(&self) -> bool {
        self.contains(ENTRY_POINT)
    }

    /// Read the entry point, failing when it is absent.
    pub fn entry_point(&mut self) -> Result<String> {
        if !self.has_entry_point() {
            return Err(OfdError::Format(sofd_error::FormatError::NotOfd));
        }
        self.read_string(ENTRY_POINT)
    }

    /// Document directory names, sorted, such as `Doc_0`.
    pub fn document_dirs(&self) -> Vec<String> {
        let mut dirs: Vec<String> = self
            .paths
            .iter()
            .filter_map(|p| p.split_once('/'))
            .map(|(head, _)| head)
            .filter(|h| h.starts_with(DOCS_PREFIX))
            .map(str::to_string)
            .collect();
        dirs.sort_unstable();
        dirs.dedup();
        dirs
    }

    /// Entries under one document directory.
    pub fn entries_under(&self, prefix: &str) -> Vec<&str> {
        let needle = format!("{prefix}/");
        self.paths
            .iter()
            .filter(|p| p.starts_with(&needle))
            .map(String::as_str)
            .collect()
    }

    /// Classify an entry by its path.
    pub fn kind_of(&self, path: &str) -> EntryKind {
        EntryKind::from_path(path)
    }
}

/// A path with any leading slashes removed, the way producers spell a
/// container path from the root.
fn strip_root(path: &str) -> &str {
    path.trim_start_matches('/')
}

/// Strip a leading `./` and collapse backslashes so lookups are predictable.
/// `enclosed_name` already guarantees the path stays inside the root, so this
/// only removes cosmetic differences between writers.
fn normalize(path: &Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    s.strip_prefix("./").unwrap_or(&s).to_string()
}

/// Map a `zip` failure onto the container taxonomy.
fn map_zip_error(e: zip::result::ZipError) -> OfdError {
    // An entry the archive encrypted and no password can open is not damage:
    // the bytes are exactly what the producer wrote. Reporting it as
    // corruption would invite a caller to retry what will always fail.
    if matches!(
        &e,
        zip::result::ZipError::UnsupportedArchive(zip::result::ZipError::PASSWORD_REQUIRED)
    ) {
        return OfdError::PasswordRequired;
    }
    let reason = match &e {
        zip::result::ZipError::Io(io) => io.to_string(),
        other => other.to_string(),
    };
    PackageError::Corrupted { reason }.into()
}

/// Build a ZIP container from named entries: what the archive writer emits,
/// and what every test that needs a document starts from.
/// Every name is checked the way the reader checks it, so this never writes an
/// archive its own `Package::open` would reject.
pub fn build_archive(entries: &[(&str, &[u8])]) -> Result<Vec<u8>> {
    use std::io::Write;

    for (name, _) in entries {
        if !is_safe_entry_name(name) {
            return Err(PackageError::IllegalPath {
                path: (*name).to_string(),
            }
            .into());
        }
    }

    let mut cursor = std::io::Cursor::new(Vec::new());
    {
        let mut writer = zip::ZipWriter::new(&mut cursor);
        // The default timestamp is the clock, and two writes of one document
        // then differ in bytes and in digest. The epoch the format starts at
        // keeps the writer a pure function of its entries.
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated)
            .last_modified_time(zip::DateTime::default());
        for (name, data) in entries {
            writer
                .start_file(*name, options)
                .map_err(|e| PackageError::Corrupted {
                    reason: format!("failed to start entry {name}: {e}"),
                })?;
            writer.write_all(data)?;
        }
        writer.finish().map_err(|e| PackageError::Corrupted {
            reason: format!("failed to finish archive: {e}"),
        })?;
    }
    Ok(cursor.into_inner())
}

/// Whether a name is a relative path with no traversal and no root, which is
/// exactly what the reader requires of an entry before it will accept it.
fn is_safe_entry_name(name: &str) -> bool {
    if name.is_empty() || name.starts_with('/') || name.starts_with('\\') {
        return false;
    }
    // A Windows drive prefix is not a container path either.
    let bytes = name.as_bytes();
    if bytes.len() >= 2 && bytes[1] == b':' {
        return false;
    }
    // An empty segment is not traversal, but the reader collapses it when it
    // normalises, so a writer that kept one would lose its own entry.
    name.split(['/', '\\'])
        .all(|part| !part.is_empty() && part != ".." && part != ".")
}

/// Whether a path is under the `Doc_*` prefix.
pub fn is_document_path(path: &str) -> bool {
    path.starts_with(DOCS_PREFIX)
}

/// Join an archive directory and a file name.
pub fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{}/{}", dir.trim_end_matches('/'), name)
    }
}

/// Parent directory of an archive path, or an empty string at the root.
pub fn parent_of(path: &str) -> String {
    match path.rsplit_once('/') {
        Some((head, _)) => head.to_string(),
        None => String::new(),
    }
}

/// The last path segment.
pub fn file_name_of(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Resolve a possibly relative reference against a base directory.
/// XML references inside a document are relative to the file containing them, so
/// parsing must resolve them the way the writer intended.
pub fn resolve(base: &str, reference: &str) -> String {
    if reference.starts_with('/') {
        return reference.trim_start_matches('/').to_string();
    }
    if reference.contains(':') {
        // Absolute ORIs and URLs are kept verbatim; the caller decides.
        return reference.to_string();
    }

    let mut parts: Vec<&str> = if base.is_empty() {
        Vec::new()
    } else {
        base.split('/').filter(|s| !s.is_empty()).collect()
    };

    for seg in reference.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }

    parts.join("/")
}

/// Whether a path looks like a directory entry.
pub fn looks_like_dir(path: &str) -> bool {
    path.ends_with('/')
}

/// Convert an archive path to a `PathBuf` for interop with I/O code.
pub fn to_path_buf(path: &str) -> PathBuf {
    PathBuf::from(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_walks_up_and_down() {
        assert_eq!(
            resolve("Doc_0/Pages", "Page_0.xml"),
            "Doc_0/Pages/Page_0.xml"
        );
        assert_eq!(
            resolve("Doc_0/Pages", "../Res/Font_0.ttf"),
            "Doc_0/Res/Font_0.ttf"
        );
        assert_eq!(
            resolve("Doc_0", "./Res/Image_1.png"),
            "Doc_0/Res/Image_1.png"
        );
    }

    #[test]
    fn resolve_keeps_absolute_references_intact() {
        assert_eq!(resolve("Doc_0", "/Res/Font_0.ttf"), "Res/Font_0.ttf");
        assert_eq!(
            resolve("Doc_0", "http://example.com/a.ttf"),
            "http://example.com/a.ttf"
        );
    }

    #[test]
    fn resolve_from_the_root_has_no_parent_to_pop() {
        // Popping past the root must not panic; it simply stays at the root.
        assert_eq!(resolve("", "../../evil.ttf"), "evil.ttf");
    }

    #[test]
    fn join_and_parent_round_trip() {
        assert_eq!(join("Doc_0", "OFD.xml"), "Doc_0/OFD.xml");
        assert_eq!(join("", "OFD.xml"), "OFD.xml");
        assert_eq!(parent_of("Doc_0/Pages/Page_0.xml"), "Doc_0/Pages");
        assert_eq!(parent_of("OFD.xml"), "");
        assert_eq!(file_name_of("Doc_0/Pages/Page_0.xml"), "Page_0.xml");
    }

    #[test]
    fn entry_point_name_matches_the_standard() {
        assert_eq!(ENTRY_POINT, "OFD.xml");
        assert!(is_document_path("Doc_0/Document.xml"));
        assert!(!is_document_path("OFD.xml"));
    }

    // --- container behaviour against real archive bytes -------------------

    const MINIMAL_OFD: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<OFD xmlns="http://www.ofdspec.org/2016"><DocBody><DocRoot>Doc_0/Document.xml</DocRoot></DocBody></OFD>"#;

    fn fixture() -> Vec<u8> {
        build_archive(&[
            ("OFD.xml", MINIMAL_OFD.as_bytes()),
            ("Doc_0/Document.xml", b"<Document/>"),
            ("Doc_0/Pages/Page_0/Content.xml", b"<Page/>"),
            ("Doc_0/Res/Font_0.ttf", b"\x00\x01\x00\x00font"),
        ])
        .expect("fixture should build")
    }

    #[test]
    fn a_well_formed_archive_lists_its_entries() {
        let pkg = Package::from_bytes(&fixture()).expect("open");
        assert_eq!(pkg.len(), 4);
        assert!(!pkg.is_empty());
        assert!(pkg.contains("OFD.xml"));
        assert!(pkg.contains("Doc_0/Res/Font_0.ttf"));
        assert!(!pkg.contains("Doc_0/Missing.xml"));
    }

    #[test]
    fn the_entry_point_reads_back_verbatim() {
        let mut pkg = Package::from_bytes(&fixture()).expect("open");
        assert!(pkg.has_entry_point());
        assert_eq!(pkg.entry_point().expect("entry point"), MINIMAL_OFD);
    }

    #[test]
    fn entries_read_back_byte_for_byte() {
        let mut pkg = Package::from_bytes(&fixture()).expect("open");
        assert_eq!(
            pkg.read("Doc_0/Document.xml").expect("read"),
            b"<Document/>"
        );
        // Binary content must survive the deflate round trip untouched.
        assert_eq!(
            pkg.read("Doc_0/Res/Font_0.ttf").expect("read"),
            b"\x00\x01\x00\x00font"
        );
    }

    #[test]
    fn reading_an_absent_entry_reports_it_by_name() {
        let mut pkg = Package::from_bytes(&fixture()).expect("open");
        let err = pkg.read("Doc_0/Nope.xml").expect_err("must fail");
        match err {
            OfdError::Package(PackageError::MissingEntry { entry }) => {
                assert_eq!(entry, "Doc_0/Nope.xml");
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn an_archive_without_an_entry_point_is_not_an_ofd() {
        let bytes = build_archive(&[("readme.txt", b"hello")]).expect("build");
        let mut pkg = Package::from_bytes(&bytes).expect("open");
        assert!(!pkg.has_entry_point());
        let err = pkg.entry_point().expect_err("must fail");
        assert!(matches!(
            err,
            OfdError::Format(sofd_error::FormatError::NotOfd)
        ));
    }

    #[test]
    fn optional_reads_distinguish_absent_from_empty() {
        let bytes = build_archive(&[
            ("OFD.xml", MINIMAL_OFD.as_bytes()),
            ("Doc_0/empty.xml", b""),
        ])
        .expect("build");
        let mut pkg = Package::from_bytes(&bytes).expect("open");

        assert_eq!(
            pkg.read_optional("Doc_0/empty.xml").expect("read"),
            Some(vec![])
        );
        assert_eq!(pkg.read_optional("Doc_0/absent.xml").expect("read"), None);
    }

    #[test]
    fn document_directories_are_discovered_and_sorted() {
        let bytes = build_archive(&[
            ("OFD.xml", MINIMAL_OFD.as_bytes()),
            ("Doc_0/Document.xml", b"<a/>"),
            ("Doc_1/Document.xml", b"<b/>"),
            ("Doc_1/Pages/Page_0/Content.xml", b"<c/>"),
        ])
        .expect("build");
        let pkg = Package::from_bytes(&bytes).expect("open");

        assert_eq!(pkg.document_dirs(), vec!["Doc_0", "Doc_1"]);
        assert_eq!(pkg.entries_under("Doc_1").len(), 2);
        assert!(pkg.entries_under("Doc_9").is_empty());
    }

    #[test]
    fn entry_kinds_are_derived_from_paths() {
        let pkg = Package::from_bytes(&fixture()).expect("open");
        assert_eq!(pkg.kind_of("OFD.xml"), EntryKind::EntryPoint);
        assert_eq!(pkg.kind_of("Doc_0/Document.xml"), EntryKind::Document);
        assert_eq!(pkg.kind_of("Doc_0/Res/Font_0.ttf"), EntryKind::Font);
    }

    #[test]
    fn too_many_entries_is_refused_before_reading_any() {
        let entries: Vec<(String, &[u8])> = (0..8)
            .map(|i| (format!("Doc_0/f{i}.xml"), b"x" as &[u8]))
            .collect();
        let refs: Vec<(&str, &[u8])> = entries.iter().map(|(n, d)| (n.as_str(), *d)).collect();
        let bytes = build_archive(&refs).expect("build");

        let limits = PackageLimits {
            max_entry_count: 3,
            ..PackageLimits::default()
        };
        let err = Package::open_with_limits(&sofd_io::SliceSource::new(&bytes, "t"), limits)
            .expect_err("must fail");
        match err {
            OfdError::Package(PackageError::EntryCountLimit { actual, limit }) => {
                assert_eq!(actual, 8);
                assert_eq!(limit, 3);
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn an_oversized_entry_is_refused_on_read() {
        let big = vec![b'a'; 4096];
        let bytes = build_archive(&[("OFD.xml", &big)]).expect("build");

        let limits = PackageLimits {
            max_entry_size: 64,
            ..PackageLimits::default()
        };
        let mut pkg = Package::open_with_limits(&sofd_io::SliceSource::new(&bytes, "t"), limits)
            .expect("open");
        let err = pkg.read("OFD.xml").expect_err("must fail");
        assert!(matches!(
            err,
            OfdError::Package(PackageError::DecompressionLimit { .. })
        ));
    }

    #[test]
    fn garbage_bytes_are_reported_as_corruption() {
        let err = Package::from_bytes(b"this is not a zip archive at all").expect_err("must fail");
        assert!(matches!(
            err,
            OfdError::Package(PackageError::Corrupted { .. })
        ));
        assert!(err.is_corruption());
    }

    #[test]
    fn the_writer_refuses_a_name_the_reader_would_reject() {
        // Found by the round-trip property test: the writer used to build an
        // archive with a leading slash that its own reader then refused, so a
        // document could be "written" and never read back.
        for bad in [
            "/a.xml",
            "\\a.xml",
            "a/../../b.xml",
            "C:/a.xml",
            "a/./b.xml",
        ] {
            let err = build_archive(&[(bad, b"x".as_slice())])
                .expect_err("a path the reader rejects must not be written");
            assert!(
                matches!(err, OfdError::Package(PackageError::IllegalPath { .. })),
                "{bad:?} gave {err:?}"
            );
        }
        // A normal nested name is still accepted.
        assert!(build_archive(&[("Doc_0/Document.xml", b"<Document/>".as_slice())]).is_ok());
    }

    #[test]
    fn truncated_archives_are_reported_as_corruption() {
        let mut bytes = fixture();
        bytes.truncate(bytes.len() / 2);
        let err = Package::from_bytes(&bytes).expect_err("must fail");
        assert!(err.is_corruption(), "unexpected error: {err}");
    }

    #[test]
    fn non_utf8_xml_is_rejected_with_the_entry_named() {
        let bytes = build_archive(&[("OFD.xml", &[0xff, 0xfe, 0xfd])]).expect("build");
        let mut pkg = Package::from_bytes(&bytes).expect("open");
        let err = pkg.read_string("OFD.xml").expect_err("must fail");
        match err {
            OfdError::Package(PackageError::Corrupted { reason }) => {
                assert!(reason.contains("OFD.xml"), "reason was {reason}");
            }
            other => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn an_empty_archive_is_simply_empty() {
        let bytes = build_archive(&[]).expect("build");
        let pkg = Package::from_bytes(&bytes).expect("open");
        assert!(pkg.is_empty());
        assert_eq!(pkg.len(), 0);
        assert!(!pkg.has_entry_point());
    }

    #[test]
    fn debug_output_summarizes_without_dumping_bytes() {
        let pkg = Package::from_bytes(&fixture()).expect("open");
        let text = format!("{pkg:?}");
        assert!(text.contains("Package"));
        assert!(text.contains("entries: 4"));
    }

    #[test]
    fn limits_round_trip_through_the_container() {
        let limits = PackageLimits::strict();
        let bytes = fixture();
        let pkg = Package::open_with_limits(&sofd_io::SliceSource::new(&bytes, "t"), limits)
            .expect("open");
        assert_eq!(pkg.limits().max_entry_count, limits.max_entry_count);
        assert_eq!(pkg.limits().max_entry_size, limits.max_entry_size);
    }
}
