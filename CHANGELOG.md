# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html). How a
release is written up is in
[Conventions.md](docs/Conventions.md#5-closing-an-issue).

## [Unreleased]

### Architecture

- **`sofd-convert` converts one page, the facade assembles documents.** The
  document-level exports (`to_pdf` over several pages, page-tree and catalog
  assembly, summed counts) moved from `sofd-convert` up to `sofd` — the crate
  that holds the `Document` — removing the dev-dependency that made the two
  crates publish in a cycle. Page-level entry points (`page_to_pdf`,
  `page_to_svg`, `page_to_png`) are unchanged; document-level tests moved to
  `sofd/tests/export.rs` and the golden tests to `sofd/tests/golden.rs`.
  **Breaking**: `sofd-convert` 0.7.0 drops `document_to_pdf*` and
  `pages_to_svg`; `Document::to_pdf*` on the facade is unchanged. `sofd` 0.7.1
  adds `Document::to_svg_pages`.

## [0.7.0] - 2026-10-30

### Engine

- **The reader's state and its `egui` shell are separate crates**: `sofd-session` holds the open document, the page cache and every queue, and `sofd-egui` draws them, so a second shell — Tauri, Qt, a canvas of its own — drives the same decisions instead of reimplementing them. The binary is still `sofd-reader`.
- **The glyph set is a crate of its own**, `sofd-icons`, with no toolkit and no dependency, so a second shell reads the theme's path data rather than a copy of it.
- **A key press maps to intent without the toolkit**: `sofd_session::action::interpret` takes the reader's own key names and modifiers, and `sofd-egui` translates `egui`'s codes into them, so which key does what is written down once and tested with no window attached.
- **A page can be drawn from geometry instead of a bitmap**: `Document::vector_page` hands a shell the outlines in page millimetres, so a zoom is a transform rather than another render. `canvas_shell` is the worked example, and a document from another producer is now committed and checked against annex A.
- **A third producer's document is read**: `compliance/fixtures/wps.ofd` is four pages of a Chinese official letter WPS Office wrote, so the committed fixtures now hold two producers other than this project rather than one — and the annex A gate validates all of them.
- **A space is no longer counted as a missing glyph**, so a document whose fonts are complete stops warning that it lost every space it contains.

### Server

- **A running server says whether it can answer**: `/healthz` reports the build and the store, so a container runtime can tell a serving instance from one that is still starting.
- **The server ships as a container image** on the GitHub container registry, `ghcr.io/emfox/sofd-server`, tagged with the version and `latest`; the release job proves it serves and draws before it pushes.

### Reader

- **The Linux reader ships as a Flatpak** alongside the tarball, so it runs on the runtime rather than on whichever freetype the distribution happens to keep, and it opens a document from inside a sandbox with no access to the home directory.
- **A zoomed page keeps its shape and shows the part on screen**, instead of the whole sheet drawn from its top-left corner.
- **A page zooms to 1,667%**, where it stopped at 417%: what limits a raster is the card's texture, not a fixed ceiling.
- **A page is rasterized at what the card can hold**, so a desktop shows an A4 at nearly four times the resolution chosen for the weakest board.
- **A page that drew less than it holds says so in the status bar**, with the count the rasterizer already kept.
- **A page reaches the screen in half the time**, because it is no longer encoded to PNG and decoded again on the way.
- **The part of a page a zoomed view gets is the part it asked for**, where any window away from the top-left corner came back blank.
- **Scrolling a zoomed page stops redrawing it at every step**, because the reader keeps each part it has already drawn and hands it back while it still covers the view.
- **The open dialog on a phone lists the documents the system can hand it**, because the filter it offers now matches the way Android matches a MIME type.
- **A second reader window exists, on Tauri**: `sofd-tauri` draws the same document as geometry on a webview canvas, over the same session, the same message catalogue, the same shortcut table and the same glyphs, laid out in the same zones with the same tokens. It is cross-platform by its toolkit — WebKitGTK, WKWebView, WebView2, Android, iOS — and it is not one of the download artefacts yet.
- **The width of a stroke now reaches a canvas**, which it did not: the walk kept the paint and dropped the style, so every rule, table line and seal on a page drew at one width.
- **The application icon is drawn by the set it belongs to**, from `document-open-symbolic`'s own path data filled by the nonzero rule, written by a script with no image library behind it and checked for staleness.

## [0.6.0] - 2026-10-27

### Signatures

- **A sealed invoice draws its seal and `sofd verify` reads it back**, since the seal travels inside the signature and the certificate already states its three lines.

### Reader

- **An OFD opens from a phone's file manager**, not only as a share.
- **The open button opens a document on a phone** through the system picker.
- **A document that ships no font draws on a phone**, with the device's own faces standing in.

### Examples

- **Every worked example is run against a generated document**, so one that compiles and does nothing fails the build.

### Tooling

- **A release tag is the products' version**, `v0.6.0`, and names the release rather than any one crate.

## [0.5.1] - 2026-10-25

### Added

- **`cargo install sofd-cli` puts a `sofd` command on the PATH.** The one crate
  a user needed to install was the one crate no registry had.

### Changed

- **`sofd-convert::base64` moved to `sofd_common::base64`.** Four crates each
  carried the same table, so the encoder now lives once.

### Removed

- **The `mmap` feature of `sofd-io`.** It enabled a dependency nothing called.

### Fixed

- **Archiving keeps a document whole when a `<` appears inside a comment, a
  CDATA section or an escaped attribute value.** Those were read as tags, so a
  stripped element could leave its text behind in the page.

## [0.5.0] - 2026-10-25

### Changed

- **The crates are named `sofd`, and the facade is that bare name.** Every
  package, module path and C symbol moved from `steleofd_*` to `sofd_*`, so
  `cargo add sofd` and `use sofd::Document` is the whole of what a caller now
  needs; the eighteen internals keep their suffixes, and `sOFD` is how the
  project is written in prose. Nothing a document draws changes: 116 pages over
  the 35 samples, plus 22 PDFs and 90 SVGs, are byte-identical before and after.

- **The paint a graphic unit carries is stated once, on `ObjectBase`.** Fill
  colour, stroke colour and the two switches were repeated on `PathObject`,
  `TextObject` and `DrawParam` and resolved again in each flattening path; they
  now live on the base every object already had, and one `resolve_paint` states
  the object-then-draw-parameter-then-default order clause 12.2 describes. Only
  the default differs between kinds, and annex A sets the two in opposite
  directions, so it stays a parameter rather than a second copy of the rule.
  `PathGraphics` is gone; `PathObject` keeps `rule`, which decides geometry
  rather than paint. `CompositeObject` loses the paint fields it never had in
  `CT_Composite` and never read.

  This breaks the public API: `PathObject.graphics.*` and the inline
  `TextObject.{fill,stroke,fill_color,stroke_color}` are now `base.*`. Nothing
  changes in what a document draws — 25 samples render byte-identically before
  and after.

- **An image resource is decoded in one place, `sofd_io::decode`.** The
  parser, the raster backend and the PDF exporter each read the same bytes and
  each accepted a slightly different set of formats, so a resource could paint
  and then refuse to export. `parse::pixel_size` and the rasteriser's
  `decode_resource` now ask there, and `Pixels` picks RGB for an image XObject
  and premultiplied RGBA for a canvas. JBIG2 is read through the decoder
  `hayro-jbig2` registers with `image`, so the hand-written collector and its
  separate format branch are gone; `sofd-io` now depends on
  `sofd-display` for `ImageCodec`.

- **A comparison's verdicts are told apart from the judging that produces
  them.** `Comparison`, `PageDiff` and `PageChange` are what a caller reads and
  stay in `compare`; `compare()` and the page-by-page rule moved to
  `compare::judge`, so the file that states the data no longer carries the rule
  and 118 lines of its tests. No API change.

### Examples

- **One worked example per delivery form**, each a single file that opens a
  document and takes it somewhere: `command_line.rs` under `sofd-cli`,
  `c_abi.rs` under `sofd-ffi`, `browser.rs` under `sofd-wasm`,
  `http_service.rs` under `sofd-server` and `desktop_viewer.rs` under
  `sofd-reader`. CI builds all five, the browser one on `wasm32`.

### Reader

- **An OFD opened from a share arrives even when the sender named the container**:
  the Android filter now takes `application/zip` and `application/octet-stream`
  beside `application/ofd`, which is what a file manager that cannot name OFD
  sends. A share of the wrong type opens and is refused with a reason.
- **The window frame draws its title through Crossfont** rather than `ab_glyph`,
  which leaves no unmaintained crate in the dependency tree.
- **Two writes of one document are byte-identical.** The archive writer stamped
  each entry with the clock, so a document saved twice differed and no digest
  could be compared.

## [0.4.0] - 2026-10-23

This release removes four feature names, which is why it is `0.4.0` and not a
patch. What it adds is a shaded region that exports as shaded, and a page whose
clips cost one mask rather than one per save.

### Features

- **Four features that compiled to nothing are gone**: `office` and `ocr` in
  `sofd-convert` and `sofd`, `xsd-validate` in
  `sofd-archive`, and `backend-raster` in `sofd-render`. Enabling one
  now fails the build rather than succeeding and changing nothing, so a caller
  who asks for a capability is told it is absent. `scripts/check_publish.py`
  holds every remaining feature to being read by code.

### Convert

- **SVG export writes a Gouraud mesh as one path per triangle** rather than one
  mean colour for the whole region. SVG 1.1 has no mesh gradient, so the shaded
  area is now a faceted copy of itself instead of a flat patch; `docs/Interop.md`
  states the limit and what replaces it.

### Render

- **A clip is shared rather than copied** by the saved state, and a page whose
  clips are one at a time now holds one mask instead of one per save. What a
  clip costs is stated in `docs/Performance.md`, which is where the budget for
  it lives.

### C ABI

- **The threading and re-entrancy contract is stated** in the header beside the handle.

### Parser

- **A text code's offsets are read as positions or as steps** according to how many of them the list supplies, which nothing pinned before.

## [0.3.0] - 2026-10-21

This release removes public API, which is why it is `0.3.0` and not a patch. What
it adds is a reader that follows a published guideline and can say what it sees
without relying on colour, an export path that no longer loses pages, and a
budget for memory beside the one for time.

### Reader interface

- **The window follows the GNOME Human Interface Guidelines**, with the two clauses it does not take recorded in `Ui-spec.md`.
- **A theme (follow-the-system, light, dark), a text size and a high contrast switch**, with every colour and size a token and each text-on-panel pair held to 4.5:1.
- **A verdict is written out beside a glyph**, so colour alone never carries it.
- **The header bar holds only what changes the document**: one view switcher, one main menu, and every page action on the toolbar below it.
- **Twenty icon glyphs on the symbolic grid** replace the words, each keeping its tooltip, which is also its accessible name.

### Performance

- **Time and memory have separate budgets**, 40 MB for a page and 512 MB for a twenty-page packet, both enforced.
- **A `memory` benchmark counts what a drawn page holds**, because criterion measures time and not bytes.
- **The benchmarks run at twenty pages**, where a cost per page that had stopped being linear would show.

### Rendering

- **A run read downwards keeps its glyphs upright**, and a line set downwards states no `DeltaX`.
- **`GlyphRun::glyph_matrix` is the one way a glyph is drawn**, so a fix cannot reach one backend and miss the other two.

### Export

- **A PDF export keeps every page**, where the CLI had been overwriting each page with the last.
- **Exported glyphs carry the document's own font**, instead of falling back to live text.
- **Every page keeps its own physical box**, so a receipt and its A4 continuations are both drawn whole.

### Removed

- **`sofd-font` no longer shapes text.** `rustybuzz` went with `shape_text`, because clause 11.4's `CGTransform` already says which glyph to draw. This is what makes the version `0.3.0`.
- **`ttf-parser` is gone**, replaced by `skrifa` after RustSec recorded its author declaring it unmaintained. The five font golden pages render byte for byte unchanged.

### Tooling

- **Gates for advisories, fuzzing and edit round-trips**, which a 690-package tree had none of.
- **A semver job whose bump is decided when a break merges**, not when it ships.
- **Crates state whether they publish and what a registry asks for**, enforced by
  `check_publish.py`.
- **The Windows reader is cross-compiled from Ubuntu** with `mingw-w64`.
- **The performance gate runs the benchmarks** rather than reading whatever the last run left in `target/criterion`.
- **An export test proves a glyph is a path, not an embedded face**, replacing the render crate's stale TODO.
- **`corpus` is now `fixtures` and `samples`**, through to the namespace inside the archives.

### Known limits

- No crate is published to a registry yet, so this release is a source tag and a set of artefacts rather than a `cargo add sofd`.
- Blending is deliberately unsupported: the standard defines no blend mode, and `Alpha` is the whole of its transparency model.
- Office formats and OCR are planned, not built.
- iOS is not in the release matrix, and the release artefacts are unsigned.

## [0.2.0] - 2026-10-19

The first release since `0.1.1` that changes what the engine draws and what a
signature is allowed to say.

### Signatures

- **A signer can be checked against an anchor the caller names.** `verify_signature_under` adds a `TrustPolicy`, because GB/T 38540-2020 7.3 e) 1) requires a certification path and names no root.
- **Two outcomes were added, so a verdict says no more than it checked.** `Unchecked` and `Untrusted` reach the C, JSON, wasm, server and reader surfaces, where the first had been reported as `Tampered`.
- **`verify_signature` keeps its meaning**: whether the digest and the certificate hold, and nothing about who issued them.

### Rendering

- **`ReadDirection` and `CharDirection` now place text**, so a vertical heading or a right-to-left line is no longer drawn along `+x`.
- **An offset list shorter than the steps places glyphs rather than moving them.**
- **A code that returns to the left edge is drawn as rows.**
- **Escaped markup in text survives**, where a `&lt;` had been dropped as a parser event.
- **A named monospaced family is read as fixed width**, whether or not `FixedWidth` is set.
- **An advance is measured in a thousandth of an em**, whatever the face's units-per-em.

### Robustness

- **A font whose table directory does not fit is a parse error, not a crash.**
- **A password-protected entry asks for a password** instead of reading as corruption.
- **A name an archive reader would collapse is refused at write time.**

### Tooling

- **Gates for advisories, fuzzing and edit round-trips**, and the dependency upgrades that clear the known advisories (`zip` 9, `lopdf` 0.45, `zbus_xml` 5.2).
- **A semver job whose bump is decided when a break merges**, not when it ships.
- **Nineteen crates publish and eight do not**, each stating a `description`, enforced by `check_publish.py`.
- **The Windows reader is cross-compiled from Ubuntu** with `mingw-w64`.
- **The performance gate runs the benchmarks** rather than reading whatever the last run left in `target/criterion`.
- **An export test proves a glyph is a path, not an embedded face**, replacing the render crate's stale TODO.
- **`corpus` is now `fixtures` and `samples`**, through to the namespace inside the archives.

### Known limits

- No crate is published to a registry yet, so this release is a source tag and a set of artefacts rather than a `cargo add sofd`.
- Blending is deliberately unsupported: the standard defines no blend mode, and `Alpha` is the whole of its transparency model.
- Office formats and OCR are planned, not built.
- iOS is not in the release matrix, and the release artefacts are unsigned.

## [0.1.1] - 2026-10-18

A repair release for `0.1.0`: no behaviour changed, and both defects were in what the release said about itself.

- **The release page carried no notes**, because the first release had nothing to diff against. The notes now come from this file through `scripts/release_notes.py`.
- **`repository` was a placeholder** naming `example.com`; it now names this repository.

## [0.1.0] - 2026-10-17

The first release, cut from `0.1.0-rc.1`: the two tags differ in the documentation and in no code at all.

### What this is

sOFD implements [GB/T 33190-2016](https://openstd.samr.gov.cn/bzgk/gb/newGbInfo?hcno=3AF6682D939116B6F5EED53D01A9DB5D), the Chinese national standard for fixed-layout electronic documents, in safe Rust: the tree carries `#![forbid(unsafe_code)]`, and the only C library under it is fontconfig. Twenty-six crates sit in layers that only ever point downward, and [`docs/Architecture.md`](docs/Architecture.md) draws them out.

### Reading

- Open a document, flatten its pages to a display list, and render at any zoom without loss.
- All three chapter 8.3 fill models, every colour space the standard defines, and `RadialShd` carried intact to each backend.
- Large documents load in shards, so a long file is not resident all at once.
- Font fallback resolves through a `FontConfig` list, and a glyph no face covers is reported rather than stretched.
- Font subsetting rebuilds an embedded face down to the glyphs the document uses (28 648 to 2 804 bytes on the sample).

### Writing and conversion

- `sofd-convert` replays a display list into SVG, PDF and PNG, so export and re-parse stay one code path.
- `sofd-edit` gives command-pattern editing with undo and permission checks, `sofd-search` indexes display-list text, and `sofd-print` rasterizes at print resolution.

### Signatures

- SM3, SM4, SM2 and X.509 primitives in `sofd-crypto`, with the international algorithms behind the same interface.
- `sofd-sign` verifies SM3+SM2 check values, reading the check method and signed-value path from the schema rather than assuming them.
- Seal and incremental update, so a signed document can carry a countersignature (`18.2.4`).
- GB/T 20520 timestamp tokens read as a CMS `SignedData` and given four verdicts through `Document::verify_timestamps`.

### Archiving

- `sofd-archive` audits a container for archival readiness, and `archive()` produces the GB/T 42133-2022 profile (`6.2.2`) with `DocType="OFD-A"`.

### Delivery

| Form | Crate | What it gives you |
|---|---|---|
| CLI | `sofd-cli` | The engine as the `sofd` command |
| WebAssembly | `sofd-wasm` | The engine in a browser page, over the same frame code |
| C ABI | `sofd-ffi` | The engine behind a committed header, `sofd.h` |
| HTTP | `sofd-server` | Documents over HTTP: a persistent store, per-tenant API keys, rate limiting, graceful drain and OpenAPI docs held to the router |
| Desktop and mobile | `sofd-reader` | A window over the engine: drag to open, print, annotations, a two-revision comparison, full keyboard navigation |

The reader is built for every platform `egui` runs on, and every artefact ships beside a `SHA256SUMS` manifest.

### Safety

- OFD files arrive from arbitrary sources, so the container and parser are hardened first: Zip Slip and zip-bomb defences, a nesting ceiling in the XML reader, and a pixel budget guarding hostile page boxes.
- `proptest` suites, a crash-free invariant over every parser entry point, and `cargo-fuzz` targets for the ZIP and XML readers, all in CI.
- A golden-image suite, the annex A schemas rebuilt from the standard, and the clause register gated.
- `cargo-semver-checks` against the last tag, a coverage floor, and the minimum Rust version built in CI.

### Interoperability

A nine-page golden book is compared against another vendor's reader and a second implementation's source, which showed four gaps in the oracle and three in our own reading; the evidence is in [`docs/Interop.md`](docs/Interop.md). The third-party set of 28 documents opens cleanly.

### Known limits

- No crate is published to a registry yet.
- Blending is deliberately unsupported: the standard defines no blend mode, and `Alpha` is the whole of its transparency model.
- Office formats and OCR are planned, not built.
- iOS is not in the release matrix, and the release artefacts are unsigned.

[Unreleased]: https://github.com/emfox/sOFD/compare/v0.7.0...HEAD
[0.7.0]: https://github.com/emfox/sOFD/releases/tag/v0.7.0
[0.6.0]: https://github.com/emfox/sOFD/releases/tag/v0.6.0
[0.5.1]: https://github.com/emfox/sOFD/releases/tag/v0.5.1
[0.5.0]: https://github.com/emfox/sOFD/releases/tag/v0.5.0
[0.4.0]: https://github.com/emfox/sOFD/releases/tag/v0.4.0
[0.3.0]: https://github.com/emfox/sOFD/releases/tag/v0.3.0
[0.2.0]: https://github.com/emfox/sOFD/releases/tag/v0.2.0
[0.1.1]: https://github.com/emfox/sOFD/releases/tag/v0.1.1
[0.1.0]: https://github.com/emfox/sOFD/releases/tag/v0.1.0
[0.1.0-rc.1]: https://github.com/emfox/sOFD/releases/tag/v0.1.0-rc.1
