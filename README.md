# sOFD

**Super OFD, Simple OFD — or whatever you wish.**

A complete Rust engine for OFD documents: open, render, convert, print, sign,
edit, search and audit, behind one API. Pure Rust, zero unsafe, built to
GB/T 33190-2016.

[English](https://github.com/emfox/sOFD/blob/HEAD/README.md) | [简体中文](https://github.com/emfox/sOFD/blob/HEAD/README.zh-CN.md)

[![Rust](https://img.shields.io/badge/rust-1.92%2B-orange)](https://www.rust-lang.org/)
[![Edition](https://img.shields.io/badge/edition-2024-blue)](https://doc.rust-lang.org/edition-guide/rust-2024/)
[![License](https://img.shields.io/badge/license-Apache--2.0-green)](https://github.com/emfox/sOFD/blob/HEAD/LICENSE)
[![Standard](https://img.shields.io/badge/standard-GB%2FT%2033190--2016-red)]()

```rust
use sofd::{Document, RenderOptions};
use sofd_io::FileSource;

let source = FileSource::open("contract.ofd")?;
let doc = Document::open(&source)?;
let png = doc.to_png(0, &RenderOptions::default())?;
```

sOFD implements [GB/T 33190-2016, the Chinese national standard for
fixed-layout electronic documents](https://openstd.samr.gov.cn/bzgk/gb/newGbInfo?hcno=3AF6682D939116B6F5EED53D01A9DB5D).
Parsing and rendering are written from scratch; the one C library in the tree is
fontconfig, reached through `font-kit` by the reader on Linux.

The current release is `0.5.1`, the first one on crates.io: all twenty-one crates
are there, so `cargo add sofd` works as well as `cargo install sofd-cli`. It also
stops the archival rewrite from mistaking a `<` inside a comment for a tag. What
it offers is in the
[`CHANGELOG`](https://github.com/emfox/sOFD/blob/HEAD/CHANGELOG.md); concrete work still to do is in
the [issue tracker](https://github.com/emfox/sOFD/issues), and the direction it
serves is in [`docs/Roadmap.md`](https://github.com/emfox/sOFD/blob/HEAD/docs/Roadmap.md).

This document has two halves: the first is for anyone using sOFD, the second
for anyone working on it.

---

# Part 1: Using sOFD

## Features

| Capability | What it covers |
|---|---|
| Reading and rendering | GB/T 33190 rendering, lossless zoom, sharded loading for large documents |
| Conversion | OFD to and from PDF, SVG and PNG; batch and merge |
| Signatures | SM2 signatures, SM3 digests, SM4 encryption and GB/T 20520 timestamps. Verification checks the proof and the certificate's window at the signing time; a caller who wants the issuer question answered passes its own trust anchors and gets a separate verdict for a signer outside them. International algorithms are named by GB/T 38540-2020 but not implemented |
| Delivery | One engine, built as a CLI, WebAssembly, a C ABI library, an HTTP service and a desktop reader |
| Localization | Fluent message layer, en-US and zh-CN |

## Working examples

One per delivery form, each a single file that opens a document and takes it
somewhere, `cargo run` away:

| Form | Example | What it shows |
|---|---|---|
| CLI | [`command_line.rs`](https://github.com/emfox/sOFD/blob/HEAD/crates/sofd-cli/examples/command_line.rs) | open from a path, read what the document declares, render every page, verify signatures |
| C ABI | [`c_abi.rs`](https://github.com/emfox/sOFD/blob/HEAD/crates/sofd-ffi/examples/c_abi.rs) | the same calls a C host makes, against the generated [header](https://github.com/emfox/sOFD/blob/HEAD/crates/sofd-ffi/include/sofd.h) |
| Browser | [`browser.rs`](https://github.com/emfox/sOFD/blob/HEAD/crates/sofd-wasm/examples/browser.rs) | the JS boundary from the Rust side: raster, SVG, search |
| HTTP | [`http_service.rs`](https://github.com/emfox/sOFD/blob/HEAD/crates/sofd-server/examples/http_service.rs) | the router, the store and a graceful shutdown, mounted in your own process |
| Desktop | [`desktop_viewer.rs`](https://github.com/emfox/sOFD/blob/HEAD/crates/sofd-reader/examples/desktop_viewer.rs) | the reader's interface drawn into an `egui::Ui`, with no window and no display |

`cargo build --workspace --examples` builds four of them; the browser one needs
`--target wasm32-unknown-unknown`, and CI builds it there.

## Getting the artefacts

Pushing a `v*` tag builds every delivery form and publishes one archive per
product, named for the release it came from:

| Artefact | For | How it runs |
|---|---|---|
| `sofd-<version>-linux.tar.gz`, `-macos`, `-windows` | Desktop | unpack and run the `sofd-reader` inside; one download per system, so a user fetches only their own |
| `sofd-<version>-web.zip` | Browser | unpack; serve `site/` over HTTP, because a `file://` page cannot load the module |
| `sofd-<version>-android.apk` | Android 7.0+ (arm64) | sideload; signed with a throwaway debug key, not a store upload |
| `sofd-<version>-uos-amd64.deb` | UOS Desktop V20 | `dpkg -i`, or install it from the file manager |
| `sofd-<version>-ffi.tar.gz` | C, C++, or anything with a C ABI | link the library inside and include `sofd.h` beside it |

iOS is the one `egui` platform left out: an app for it can only be installed
after a macOS signing and packaging step, and no key ships with this project.

The builds are **unsigned** — there is no signing key — so each release carries
a `SHA256SUMS` manifest as the only thing a download can be held to:

```bash
python3 scripts/release_checksums.py verify .
sha256sum -c SHA256SUMS          # or without this project at all
```

All twenty-one crates are on crates.io at `0.5.1`. Only some of them are
interfaces: add `sofd` and you are done, and the crates it depends on come along
because cargo requires them, not because you were meant to name them. Which is
which is in
[Architecture.md § 3](https://github.com/emfox/sOFD/blob/HEAD/docs/Architecture.md#3-crate-inventory),
and `python3 scripts/publish_order.py` prints the dependency closure — from
`sofd` that is all twenty-one, while `sofd-crypto` alone needs three.

Each crate carries its own version, and at `0.5.1` they happen to agree. A
release that changes three crates bumps those three and leaves the rest where
they are, so the number to pin is the one `cargo add` writes for the crate you
actually named; a dependency's version is the interval its manifest states.
What that costs and why it is worth it is in
[Versioning.md](https://github.com/emfox/sOFD/blob/HEAD/docs/Versioning.md).

The command is one of them, published as `sofd-cli` because `sofd` is the
library:

```bash
cargo install sofd-cli        # installs the `sofd` command
```

In a project of your own:

```bash
cargo add sofd                # the facade: render, sign, convert, print, search
cargo add sofd-io             # FileSource, the file a Document is opened from
```

## What it reads

| Standard | Scope |
|---|---|
| [GB/T 33190-2016](https://openstd.samr.gov.cn/bzgk/gb/newGbInfo?hcno=3AF6682D939116B6F5EED53D01A9DB5D) | Fixed-layout document format for electronic file storage and exchange |
| [GB/T 33476.1-2016](https://openstd.samr.gov.cn/bzgk/gb/newGbInfo?hcno=419233A04D029066CF032CFACBC8713B) | Electronic official document format for Party and government bodies, part 1: document structure |
| [GB/T 33476.2-2016](https://openstd.samr.gov.cn/bzgk/gb/newGbInfo?hcno=5B4E730245FF2FAF909435EBE6A659D2) | Same, part 2: presentation |
| [GB/T 38540-2020](https://openstd.samr.gov.cn/bzgk/gb/newGbInfo?hcno=EBF1360C272E40E7A8B9B32ED0724AB1) | Cryptographic specification for secure electronic seals |
| [GM/T 0099-2020](https://std.samr.gov.cn/hb/search/stdHBDetailed?id=C60C676BC220358AE05397BE0A0AFABB) | Cryptography application for open fixed-layout documents: signature, encryption, integrity |
| [GB/T 39362-2020](https://openstd.samr.gov.cn/bzgk/gb/newGbInfo?hcno=C6B230010286DC79C84CF8EB72DE3F97) | Archiving specification for electronic official documents |

GB/T 33476 is published in three parts; parts 1 and 2 are the ones this project
follows. GM/T 0099-2020 is a cryptography industry standard issued by the State
Cryptography Administration, so it lives on the industry standards platform
rather than the national standards one.

Clause-level coverage lives in
[`compliance/gb-t-33190.toml`](https://github.com/emfox/sOFD/blob/HEAD/compliance/gb-t-33190.toml) and the method
behind it in [`compliance/CLAUSES.md`](https://github.com/emfox/sOFD/blob/HEAD/compliance/CLAUSES.md): 56 clauses are
registered, every one of them `verified`. The clause numbers are read out of the
standard's transcribed text by [`scripts/gbt33190.py`](https://github.com/emfox/sOFD/blob/HEAD/scripts/gbt33190.py),
which reads each numbered heading and checks the result against the contents
pages the standard prints, so a row cannot cite a clause the standard does not
have. What the engine can and cannot draw with is set out face by face in
[`docs/Fonts.md`](https://github.com/emfox/sOFD/blob/HEAD/docs/Fonts.md). How it compares to other readers in the field,
page by page against a second vendor's output, is in
[`docs/Interop.md`](https://github.com/emfox/sOFD/blob/HEAD/docs/Interop.md). What a signature can and cannot settle,
and why the standards leave the issuer to the caller, is in
[`docs/Trust.md`](https://github.com/emfox/sOFD/blob/HEAD/docs/Trust.md).

## Background

OFD and PDF are both fixed-layout formats, but OFD is an open ZIP + XML
container with national cryptography and electronic seals built into the
standard. Its layout is required to stay identical across software and hardware,
which is why it serves as the legal carrier for official documents, licences,
invoices and archives.

Rust fits here for two reasons. Document processing is CPU bound and has to run
in a browser, on a server and on the desktop, so zero-cost abstraction lets the
layering stay clean without a performance penalty. And since OFD files arrive
from arbitrary sources, `#![forbid(unsafe_code)]` matters more than usual: the
parser is the part handling untrusted input.

---

# Part 2: Working on sOFD

## Module map

Twenty-seven crates in five layers. An arrow points from a crate to what it
depends on; boxes below the facade are pulled in only when a feature names them.

```
  ┌──────────────────────────────────────────────────────────────────────┐
  │  delivery forms: applications, not dependencies                      │
  │                                                                      │
  │    reader        cli        wasm        ffi        server            │
  │    egui/eframe   binary     browser     C ABI      HTTP              │
  └──────────────────────────────────────────────────────────────────────┘
                                  │
                                  ▼
  ┌──────────────────────────────────────────────────────────────────────┐
  │  sofd                                                       │
  │  one handle, one feature per capability                              │
  └──────────────────────────────────────────────────────────────────────┘
                                  │
  ┌──────────────────────────────────────────────────────────────────────┐
  │  always linked: the open-to-display path                             │
  │                                                                      │
  │    parser ──► package ──► xml                                        │
  │      │             │       io                                        │
  │      ▼             │                                                 │
  │    model ◄─────────┘                                                 │
  │      │                                                               │
  │    display ──► font ──► geometry                                     │
  │                                                                      │
  │    every crate above resolves errors through error ──► common        │
  └──────────────────────────────────────────────────────────────────────┘

  ┌──────────────────────────────────────────────────────────────────────┐
  │  behind a feature of core: one capability each                       │
  │                                                                      │
  │          render    display, geometry, font                           │
  │          print     render, display                                   │
  │          convert   render, parser, display                           │
  │          sign      crypto, package, model                            │
  │          crypto    common, error                                     │
  │          edit      parser, package, model                            │
  │          search    display, geometry                                 │
  │          archive   package, model                                    │
  │          annot     nothing: the parser it reads is never optional    │
  └──────────────────────────────────────────────────────────────────────┘

  sofd-i18n      messages for the shells; the engine emits none
  sofd-testkit   fixtures for the test suites, a dev-dependency only
```

The keystone is `sofd-display`: every output backend reads the same
flattened list, so an export cannot drift from the preview. Section 3 of
[`docs/Architecture.md`](https://github.com/emfox/sOFD/blob/HEAD/docs/Architecture.md) carries the same inventory as a
table, with the responsibility and registry status of each crate.

## Where a document goes

`Content.xml` is a nested tree of templates. Having every output backend walk it
separately would duplicate the logic and, worse, let the backends drift apart.
That drift is the usual cause of an export not matching the preview.

So the tree is flattened once into a sequence of commands carrying absolute
transforms, and every downstream consumer reads that:

```
Content.xml (nested template tree)
       |
       v  parsed once
DisplayList (flattened, absolute CTM, resources resolved)
       |
       +--> render    raster
       +--> print
       +--> convert   PDF / SVG / PNG
       +--> search    text extraction
```

Office and OCR conversion are on the roadmap, not in the code: there is no
feature for either, so asking for one fails to build rather than compiling to
nothing.

## No user-facing text in the engine

Errors are structured enums with parameters, never prose:

```rust
// Any locale can render this
FormatError::PageOutOfRange { index: 42, total: 100 }

// Hardcoding prose here would make the engine unusable from C ABI, JS or Python
"page index out of range: 42 (document has 100 pages)"
```

The engine's own message catalogue (`sofd-i18n`) holds engine messages only.
A presentation layer that has product-facing strings of its own — the reader
does — keeps them beside itself and merges them in with `I18nBuilder`, so the
engine never names a widget it does not own.

## Untrusted input

| Threat | Defense |
|---|---|
| Zip Slip path traversal | `PackageError::IllegalPath` |
| Zip bomb | Entry count and decompressed size ceilings (512 MB) |
| Stack exhaustion via deep nesting | `DisplayListBuilder::max_depth`, default 64 |
| Hostile document hanging the renderer | `RenderError::BudgetExceeded`, degrading to partial output |
| Edits silently breaking signatures | `EditError::WouldInvalidateSignature`, requiring confirmation |

## Building

Rust 1.92+ with Edition 2024, which is what `rust-version` in the workspace
manifest states; `.github/workflows/rust.yml` names the version CI builds with.
`cargo check --workspace` is enough to confirm a toolchain works.

Building the crates needs no C toolchain. The reader is the exception on Linux:
`font-kit` links fontconfig, so a machine without the headers needs
`libfontconfig1-dev` and `pkg-config` installed.

```bash
git clone https://github.com/emfox/sOFD.git
cd sOFD

cargo check --workspace          # check the whole workspace
cargo build --release            # release build
cargo test --workspace           # run tests
```

## Checks

```bash
python3 scripts/check_all.py             # everything this machine can run
python3 scripts/check_all.py --quick     # the policy gates only
python3 scripts/check_i18n.py            # message key consistency across locales
python3 scripts/check_language.py        # English-only policy and comment density
python3 scripts/check_publish.py --list  # which crates reach a registry
python3 scripts/publish_order.py         # the order a registry accepts them in
python3 scripts/check_readme.py          # the counts both READMEs state
```

The i18n check compares the catalogues: same file set, same key set, consistent
placeholders, and the `all_keys()` list in Rust matching the keys in the FTL
files. The publish check holds the two sets — twenty-one crates that publish and
six that do not — to the whole workspace, so a new crate cannot slip in
without saying where it goes, and `scripts/publish_order.py` holds the published
set to being closed under its own dependencies. The README check reads every
count out of the English README and the same sentence out of the Chinese one, so
the two cannot state different figures: `README.md` is the source of truth and
`README.zh-CN.md` follows it.

## Contributing

Issues and pull requests are welcome. Before submitting, make sure these pass:

```bash
cargo test --workspace
cargo clippy --workspace -- -D warnings
python3 scripts/check_all.py
```

New code needs tests. Do not introduce `unsafe`; `sofd-ffi` is the only
documented exemption, because a C ABI requires it. Code, comments, commit
messages and issues are English only: [docs/Conventions.md](https://github.com/emfox/sOFD/blob/HEAD/docs/Conventions.md).

Performance baselines, and how to run them, are in
[`docs/Performance.md`](https://github.com/emfox/sOFD/blob/HEAD/docs/Performance.md).

## License

[Apache-2.0](https://github.com/emfox/sOFD/blob/HEAD/LICENSE)

The reader's icons are the Adwaita symbolic set, drawn by the GNOME Project and
used here under CC BY-SA 3.0 US; see [NOTICE.md](https://github.com/emfox/sOFD/blob/HEAD/NOTICE.md).

## Related

- [National standards full-text portal](https://openstd.samr.gov.cn/bzgk/gb/index), where the GB/T texts above can be previewed
- [Industry standards platform](https://std.samr.gov.cn/hb/), for GM/T standards
- [GmSSL](https://github.com/guanzhi/GmSSL), an open source national cryptography implementation
