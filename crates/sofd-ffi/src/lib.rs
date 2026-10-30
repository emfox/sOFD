//! C ABI bindings: open an OFD from a host application and pull data out.
//! The data shapes come from `sofd-bindings`; this module only crosses the C
//! boundary, so the gathering stays testable without touching pointers.

#![warn(missing_docs)]
// The crate is the one place in the workspace where unsafe is allowed: every
// pointer that arrives from C is checked here and nowhere else. The ABI is
// pointer-based by nature, so the deref lint bends to the documented contract.
#![allow(clippy::not_unsafe_ptr_arg_deref)]

pub use sofd_bindings as data;

use std::ffi::{CStr, CString, c_char};
use std::os::raw::c_int;
use std::ptr;

use sofd::Document;
use sofd_error::OfdError;

// The handle stays opaque to C, which is also how the header generator is
// told the type has no layout: without repr(C) it emits the forward
// declaration alone rather than reaching for a field type C cannot name.

/// One open document, opaque to C: only a pointer crosses the boundary.
///
/// Threads: one handle belongs to one thread at a time, though not always the
/// one that opened it. Two threads must not call on one handle at once, since
/// a call may rewrite the arena the last pointer came from. Different handles
/// share nothing and run in parallel. Calls never re-enter, as none invokes one.
pub struct SofdDocument {
    inner: Document,
    arena: Arena,
}

/// What an open document declares, at a glance.
#[repr(C)]
pub struct SofdInfo {
    /// Version string, such as `1.0`.
    pub version: *const c_char,
    /// Number of pages.
    pub pages: usize,
    /// Page width in millimetres.
    pub width_mm: f64,
    /// Page height in millimetres.
    pub height_mm: f64,
    /// Fonts the container embeds.
    pub embedded_fonts: usize,
    /// Signatures the document points at.
    pub signatures: usize,
}

/// One search match.
#[repr(C)]
pub struct SofdHit {
    /// Zero-based page holding the match.
    pub page: usize,
    /// Text around the match, NUL-terminated.
    pub snippet: *const c_char,
}

/// The outcome of verifying one signature.
#[repr(C)]
pub struct SofdSignature {
    /// Archive path of the signature descriptor file, NUL-terminated.
    pub location: *const c_char,
    /// Whether the signature still matches the content it covers.
    pub valid: c_int,
    /// Whether nothing was proved, which `valid` being false does not say.
    pub unchecked: c_int,
    /// Whether the proof holds but its certificate chains to no root the
    /// caller named.
    pub untrusted: c_int,
}

/// Memory the C side borrows from, rewritten by each call that hands out data.
#[derive(Default)]
struct Arena {
    /// NUL-terminated strings, each in its own allocation so a growing list
    /// never moves a pointer already handed out.
    strings: Vec<CString>,
    /// The last rendered bytes, keyed by what produced them.
    output: Option<(OutputKey, Vec<u8>)>,
}

/// What the cached bytes are, so the probing call and the filling call agree.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum OutputKey {
    /// One page rasterized at a dpi, held as bits so it compares by value.
    Png { page: usize, dpi: u64 },
    /// The whole document as one PDF file.
    Pdf,
    /// One page as an SVG document.
    Svg { page: usize },
}

impl SofdDocument {
    /// Render once and cache the bytes, so the size-probing call and the
    /// buffer-filling call share a single computation.
    fn output_bytes(
        &mut self,
        key: OutputKey,
        produce: impl FnOnce(&Document) -> Result<Vec<u8>, OfdError>,
    ) -> Result<&[u8], OfdError> {
        let stale = self
            .arena
            .output
            .as_ref()
            .is_none_or(|(cached, _)| *cached != key);
        if stale {
            let bytes = produce(&self.inner)?;
            self.arena.output = Some((key, bytes));
        }
        Ok(&self.arena.output.as_ref().expect("stored above").1)
    }
}

/// Intern a string in the handle arena and hand out its pointer. Text
/// carrying a NUL byte cannot cross the boundary, so such an entry is
/// dropped rather than silently truncated.
fn intern(arena: &mut Vec<CString>, text: &str) -> Option<*const c_char> {
    let owned = CString::new(text).ok()?;
    arena.push(owned);
    arena.last().map(|entry| entry.as_ptr())
}

/// Report a failure the way C expects: a message with snprintf semantics,
/// always NUL-terminated when the buffer has room for one.
fn write_err(err_buf: *mut c_char, err_len: usize, message: &str) {
    if err_buf.is_null() || err_len == 0 {
        return;
    }
    let bytes = message.as_bytes();
    let copied = bytes.len().min(err_len - 1);
    // The caller promised `err_len` writable bytes at `err_buf`.
    unsafe {
        ptr::copy_nonoverlapping(bytes.as_ptr(), err_buf.cast::<u8>(), copied);
        *err_buf.add(copied) = 0;
    }
}

/// The sentinel every size-returning call reports failure with, after
/// leaving the message in the caller's buffer.
fn fail(err: &OfdError, err_buf: *mut c_char, err_len: usize) -> usize {
    write_err(err_buf, err_len, &err.to_string());
    usize::MAX
}

/// Copy cached bytes into the caller's buffer when it fits. The size comes
/// back either way, so a too-small buffer is not an error: the caller
/// probes with a null buffer, then allocates and calls again.
fn copy_out(bytes: &[u8], buf: *mut u8, buf_len: usize) -> usize {
    if !buf.is_null() && buf_len >= bytes.len() {
        // The caller promised `buf_len` writable bytes at `buf`.
        unsafe { ptr::copy_nonoverlapping(bytes.as_ptr(), buf, bytes.len()) };
    }
    bytes.len()
}

/// The library version, such as `1.0`.
///
/// The string is static, so the pointer is valid for the process lifetime.
#[unsafe(no_mangle)]
pub extern "C" fn sofd_version() -> *const c_char {
    static VERSION: &str = concat!(env!("CARGO_PKG_VERSION"), "\0");
    VERSION.as_ptr().cast()
}

/// Open an `.ofd` file from its raw bytes. Returns a handle to release with
/// `sofd_close`, or null on failure with a message in `err_buf`. The
/// handle belongs to the calling thread; see `SofdDocument`.
#[unsafe(no_mangle)]
pub extern "C" fn sofd_open(
    bytes: *const u8,
    len: usize,
    err_buf: *mut c_char,
    err_len: usize,
) -> *mut SofdDocument {
    let slice = if bytes.is_null() {
        if len == 0 {
            &[][..]
        } else {
            write_err(err_buf, err_len, "a null byte pointer needs a zero length");
            return ptr::null_mut();
        }
    } else {
        // The caller promises `len` readable bytes at `bytes`.
        unsafe { std::slice::from_raw_parts(bytes, len) }
    };
    match Document::open_bytes(slice) {
        Ok(inner) => Box::into_raw(Box::new(SofdDocument {
            inner,
            arena: Arena::default(),
        })),
        Err(err) => {
            write_err(err_buf, err_len, &err.to_string());
            ptr::null_mut()
        }
    }
}

/// Release a handle. Null is a no-op, so teardown paths need no check. No other
/// call on the handle may be running on another thread; both go with it.
#[unsafe(no_mangle)]
pub extern "C" fn sofd_close(doc: *mut SofdDocument) {
    if !doc.is_null() {
        // The handle came from Box::into_raw in sofd_open.
        unsafe { drop(Box::from_raw(doc)) };
    }
}

/// How many pages the document declares. Null handles report zero.
#[unsafe(no_mangle)]
pub extern "C" fn sofd_page_count(doc: *const SofdDocument) -> usize {
    match unsafe { doc.as_ref() } {
        Some(doc) => doc.inner.page_count(),
        None => 0,
    }
}

/// Fill `out` with the declaration-level facts, returning 0 on success and
/// -1 on failure with a message in `err_buf`. `out->version` is borrowed
/// from the handle; see the lifetime rule in the header.
#[unsafe(no_mangle)]
pub extern "C" fn sofd_info(
    doc: *mut SofdDocument,
    out: *mut SofdInfo,
    err_buf: *mut c_char,
    err_len: usize,
) -> c_int {
    let Some(doc) = (unsafe { doc.as_mut() }) else {
        write_err(err_buf, err_len, "null document handle");
        return -1;
    };
    let Some(out) = (unsafe { out.as_mut() }) else {
        write_err(err_buf, err_len, "null info pointer");
        return -1;
    };
    let info = data::DocumentInfo::of(&mut doc.inner);
    doc.arena.strings.clear();
    let Some(version) = intern(&mut doc.arena.strings, &info.version) else {
        write_err(err_buf, err_len, "the version string carries a NUL byte");
        return -1;
    };
    *out = SofdInfo {
        version,
        pages: info.pages,
        width_mm: info.width_mm,
        height_mm: info.height_mm,
        embedded_fonts: info.embedded_fonts,
        signatures: info.signatures,
    };
    0
}

/// Full-text search over every page, case-insensitive. Returns the number
/// of hits, which may exceed `hits_len`: probe with a null array, then call
/// again with room for every entry. Snippets are borrowed from the handle.
#[unsafe(no_mangle)]
pub extern "C" fn sofd_search(
    doc: *mut SofdDocument,
    needle: *const c_char,
    hits: *mut SofdHit,
    hits_len: usize,
    err_buf: *mut c_char,
    err_len: usize,
) -> usize {
    let Some(doc) = (unsafe { doc.as_mut() }) else {
        write_err(err_buf, err_len, "null document handle");
        return usize::MAX;
    };
    let needle = if needle.is_null() {
        write_err(err_buf, err_len, "null search needle");
        return usize::MAX;
    } else {
        // The caller promises a NUL-terminated string at `needle`.
        match unsafe { CStr::from_ptr(needle) }.to_str() {
            Ok(text) => text,
            Err(_) => {
                write_err(err_buf, err_len, "the search needle is not valid UTF-8");
                return usize::MAX;
            }
        }
    };
    let data = data::search_hits(&doc.inner, needle, data::DEFAULT_SEARCH);
    let cap = if hits.is_null() {
        0
    } else {
        hits_len.min(data.len())
    };
    // The strings must be interned before the entries that point at them,
    // and only for the entries that actually get written.
    doc.arena.strings.clear();
    let mut written = 0;
    for hit in data.iter().take(cap) {
        let Some(snippet) = intern(&mut doc.arena.strings, &hit.snippet) else {
            continue;
        };
        // The caller promised room for `hits_len` entries.
        unsafe {
            hits.add(written).write(SofdHit {
                page: hit.page,
                snippet,
            })
        };
        written += 1;
    }
    data.len()
}

/// Verify every signature, in document order, with the same probing pattern
/// as `sofd_search`. A document that declares no signatures is an error,
/// matching the CLI. Locations are borrowed from the handle.
#[unsafe(no_mangle)]
pub extern "C" fn sofd_verify(
    doc: *mut SofdDocument,
    sigs: *mut SofdSignature,
    sigs_len: usize,
    err_buf: *mut c_char,
    err_len: usize,
) -> usize {
    let Some(doc) = (unsafe { doc.as_mut() }) else {
        write_err(err_buf, err_len, "null document handle");
        return usize::MAX;
    };
    let data = match data::signature_statuses(&mut doc.inner) {
        Ok(data) => data,
        Err(err) => return fail(&err, err_buf, err_len),
    };
    let cap = if sigs.is_null() {
        0
    } else {
        sigs_len.min(data.len())
    };
    doc.arena.strings.clear();
    let mut written = 0;
    for sig in data.iter().take(cap) {
        let Some(location) = intern(&mut doc.arena.strings, &sig.location) else {
            continue;
        };
        // The caller promised room for `sigs_len` entries.
        unsafe {
            sigs.add(written).write(SofdSignature {
                location,
                valid: sig.valid as c_int,
                unchecked: sig.unchecked as c_int,
                untrusted: sig.untrusted as c_int,
            })
        };
        written += 1;
    }
    data.len()
}

/// Rasterize one page and hand back PNG bytes. Probe the size with a null
/// buffer, then call again with room for every byte; the render happens
/// once and stays cached on the handle.
#[unsafe(no_mangle)]
pub extern "C" fn sofd_render_png(
    doc: *mut SofdDocument,
    page: usize,
    dpi: f64,
    buf: *mut u8,
    buf_len: usize,
    err_buf: *mut c_char,
    err_len: usize,
) -> usize {
    let Some(doc) = (unsafe { doc.as_mut() }) else {
        write_err(err_buf, err_len, "null document handle");
        return usize::MAX;
    };
    let bytes = match doc.output_bytes(
        OutputKey::Png {
            page,
            dpi: dpi.to_bits(),
        },
        |inner| {
            let options = sofd::RenderOptions {
                dpi,
                ..Default::default()
            };
            inner.to_png(page, &options)
        },
    ) {
        Ok(bytes) => bytes,
        Err(err) => return fail(&err, err_buf, err_len),
    };
    copy_out(bytes, buf, buf_len)
}

/// Convert the whole document to one PDF file. Same buffer protocol as
/// `sofd_render_png`: probe with a null buffer, then call again with
/// room for every byte.
#[unsafe(no_mangle)]
pub extern "C" fn sofd_to_pdf(
    doc: *mut SofdDocument,
    buf: *mut u8,
    buf_len: usize,
    err_buf: *mut c_char,
    err_len: usize,
) -> usize {
    let Some(doc) = (unsafe { doc.as_mut() }) else {
        write_err(err_buf, err_len, "null document handle");
        return usize::MAX;
    };
    let bytes = match doc.output_bytes(OutputKey::Pdf, |inner| inner.to_pdf()) {
        Ok(bytes) => bytes,
        Err(err) => return fail(&err, err_buf, err_len),
    };
    copy_out(bytes, buf, buf_len)
}

/// Convert one page to an SVG document. Same buffer protocol as
/// `sofd_render_png`; the bytes are the SVG text, not NUL-terminated
/// inside the buffer.
#[unsafe(no_mangle)]
pub extern "C" fn sofd_to_svg(
    doc: *mut SofdDocument,
    page: usize,
    buf: *mut u8,
    buf_len: usize,
    err_buf: *mut c_char,
    err_len: usize,
) -> usize {
    let Some(doc) = (unsafe { doc.as_mut() }) else {
        write_err(err_buf, err_len, "null document handle");
        return usize::MAX;
    };
    let bytes = match doc.output_bytes(OutputKey::Svg { page }, |inner| {
        inner.to_svg(page).map(|svg| svg.into_bytes())
    }) {
        Ok(bytes) => bytes,
        Err(err) => return fail(&err, err_buf, err_len),
    };
    copy_out(bytes, buf, buf_len)
}
