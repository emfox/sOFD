#!/usr/bin/env python3
"""Bundle release artefacts by the thing they belong to.

A release builds in five jobs and each job used to attach its files one by
one, so the page carried the parts rather than the products: a reader next to
an FFI header next to four `wasm` files, with nothing saying which ran where.
Some of those parts are not usable alone — the browser bindings are a `.js`
glue file and a `.wasm` module that must sit side by side, and the C ABI
library is a library and a header that belong together.

This tool writes one archive per product, named `sofd-<version>-<product>`
like a source tarball, so a download is self-explanatory and only one file
per product has to be explained in the README.

Each archive is written with the product directory as its root, so unpacking
one in place gives the layout CI verified, and the checksum of that directory
is the checksum of the archive's contents. Nothing here runs a compiler; the
workflow builds each product and this packs what the build left behind.

Usage:
    python3 scripts/release_package.py reader  --version 0.5.1 --out dist
    python3 scripts/release_package.py web     --version 0.5.1 --out dist --page DIR --pkg DIR
    python3 scripts/release_package.py android --version 0.5.1 --out dist --apk PATH
    python3 scripts/release_package.py uos     --version 0.5.1 --out dist --deb PATH
    python3 scripts/release_package.py ffi     --version 0.5.1 --out dist --lib DIR
    python3 scripts/release_package.py selftest

Exit code: 0 on success, 1 on failure.
"""

from __future__ import annotations

import argparse
import shutil
import sys
import tarfile
import tempfile
import zipfile
from pathlib import Path

# The reader's three desktop builds, named for the system each runs on.
DESKTOP = ("sofd-reader-linux", "sofd-reader-macos", "sofd-reader-windows.exe")

# The FFI library under each name a linker is given, plus its header.
FFI_LIBRARIES = ("libsofd_ffi.so", "libsofd_ffi.a")


def product(name: str, version: str) -> str:
    """The archive one product ships as."""
    return f"sofd-{version}-{name}"


def require(paths: list[Path], product_name: str) -> None:
    """Stop on a missing input rather than shipping a half-filled archive."""
    missing = [str(p) for p in paths if not p.exists()]
    if missing:
        raise SystemExit(f"error: {product_name} needs {', '.join(missing)}")


def write_zip(archive: Path, root: Path) -> None:
    """Zip every file below `root`, with `root` itself as the top directory."""
    archive.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as bundle:
        for path in sorted(root.rglob("*")):
            bundle.write(path, path.relative_to(root.parent))


def write_tar(archive: Path, root: Path) -> None:
    """The same layout as a `.tar.gz`, which keeps the executable bit."""
    archive.parent.mkdir(parents=True, exist_ok=True)
    with tarfile.open(archive, "w:gz") as bundle:
        bundle.add(root, arcname=root.name)


def reader(version: str, out: Path, dist: Path) -> Path:
    """The three desktop builds, which are one product and three systems."""
    sources = [dist / name for name in DESKTOP]
    if not any(p.exists() for p in sources):
        raise SystemExit(f"error: reader needs one of {', '.join(DESKTOP)} under {dist}")
    # A job builds one system, so the archive holds whichever arrived and the
    # last one to finish replaces it. The workflow packs once, after all three.
    name = product("reader", version)
    staging = out / name
    shutil.rmtree(staging, ignore_errors=True)
    staging.mkdir(parents=True)
    for source in sources:
        if source.exists():
            shutil.copy2(source, staging / source.name)
    archive = out / f"{name}.tar.gz"
    write_tar(archive, staging)
    shutil.rmtree(staging)
    return archive


def web(version: str, out: Path, page: Path, pkg: Path) -> Path:
    """The reader page and the bindings it loads, which only work together."""
    expect = {
        "page": page / "index.html",
        "bindings": pkg / "sofd_reader.js",
        "module": pkg / "sofd_reader_bg.wasm",
    }
    require(list(expect.values()), "web")
    name = product("web", version)
    staging = out / name
    shutil.rmtree(staging, ignore_errors=True)
    # The page is lifted one directory so the archive unpacks to a served
    # directory directly, with the bindings where `index.html` looks for them.
    site = staging / "site"
    site.mkdir(parents=True)
    shutil.copy2(page / "index.html", site / "index.html")
    shutil.copytree(pkg, site / "pkg", ignore=shutil.ignore_patterns("*.md", "package.json"))
    archive = out / f"{name}.zip"
    write_zip(archive, staging)
    shutil.rmtree(staging)
    return archive


def android(version: str, out: Path, apk: Path) -> Path:
    """One archive, so there is nothing to regroup; the version goes in the name."""
    require([apk], "android")
    archive = out / f"{product('android', version)}.apk"
    archive.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(apk, archive)
    return archive


def uos(version: str, out: Path, deb: Path) -> Path:
    """Same for the `.deb`: `dpkg` owns its internals, so only the name changes."""
    require([deb], "uos")
    archive = out / f"{product('uos', version)}-{deb.name.split('-')[-1]}"
    archive.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(deb, archive)
    return archive


def ffi(version: str, out: Path, lib: Path) -> Path:
    """The library under both names a linker is given, and the header it needs."""
    expect = [lib / name for name in FFI_LIBRARIES] + [lib / "sofd.h"]
    require(expect, "ffi")
    name = product("ffi", version)
    staging = out / name
    shutil.rmtree(staging, ignore_errors=True)
    staging.mkdir(parents=True)
    for source in expect:
        shutil.copy2(source, staging / source.name)
    archive = out / f"{name}.tar.gz"
    write_tar(archive, staging)
    shutil.rmtree(staging)
    return archive


def check_layout(tmp: Path) -> list[tuple[str, bool]]:
    """Pack throwaway products and read the layout back out of them."""
    checks: list[tuple[str, bool]] = []
    out = tmp / "dist"
    dist = tmp / "build"
    page = tmp / "page"
    pkg = tmp / "pkg"
    lib = tmp / "lib"
    for directory in (out, dist, page, pkg, lib):
        directory.mkdir(parents=True)

    for name in DESKTOP:
        (dist / name).write_bytes(b"reader")
    (page / "index.html").write_text("<html></html>")
    (pkg / "sofd_reader.js").write_text("export {}")
    (pkg / "sofd_reader_bg.wasm").write_bytes(b"\0asm")
    (pkg / "README.md").write_text("generated")
    for name in FFI_LIBRARIES + ("sofd.h",):
        (lib / name).write_bytes(b"ffi")
    (tmp / "reader.apk").write_bytes(b"apk")
    (tmp / "reader-uos-amd64.deb").write_bytes(b"deb")

    bundle = reader("1.2.3", out, dist)
    with tarfile.open(bundle) as tar:
        names = tar.getnames()
    checks.append((
        "reader ships one archive for three systems",
        bundle.name == "sofd-1.2.3-reader.tar.gz"
        and all(f"sofd-1.2.3-reader/{name}" in names for name in DESKTOP),
    ))

    bundle = web("1.2.3", out, page, pkg)
    with zipfile.ZipFile(bundle) as zip_file:
        names = zip_file.namelist()
    checks.append((
        "web puts the page and the bindings in one tree",
        "sofd-1.2.3-web/site/index.html" in names
        and "sofd-1.2.3-web/site/pkg/sofd_reader_bg.wasm" in names,
    ))
    checks.append(("web leaves generated docs out", not any(n.endswith(".md") for n in names)))

    checks.append((
        "android carries the version in its name",
        android("1.2.3", out, tmp / "reader.apk").name == "sofd-1.2.3-android.apk",
    ))
    checks.append((
        "uos keeps the architecture it was built for",
        uos("1.2.3", out, tmp / "reader-uos-amd64.deb").name == "sofd-1.2.3-uos-amd64.deb",
    ))

    bundle = ffi("1.2.3", out, lib)
    with tarfile.open(bundle) as tar:
        names = tar.getnames()
    checks.append((
        "ffi ships both library names and the header",
        all(f"sofd-1.2.3-ffi/{name}" in names for name in FFI_LIBRARIES + ("sofd.h",)),
    ))

    try:
        reader("1.2.3", out, tmp / "empty")
    except SystemExit:
        checks.append(("an empty product fails rather than shipping", True))
    else:
        checks.append(("an empty product fails rather than shipping", False))

    return checks


def selftest() -> int:
    with tempfile.TemporaryDirectory() as scratch:
        checks = check_layout(Path(scratch))
    for label, ok in checks:
        print(f"{'PASS' if ok else 'FAIL'}  {label}")
    failed = sum(1 for _, ok in checks if not ok)
    print(f"{'FAILED' if failed else 'OK'}: {len(checks) - failed} of {len(checks)} check(s) passed")
    return 1 if failed else 0


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    parser.add_argument("what", choices=["reader", "web", "android", "uos", "ffi", "selftest"])
    parser.add_argument("--version", help="the release, without the leading v")
    parser.add_argument("--out", type=Path, default=Path("dist"), help="where the archives go")
    parser.add_argument("--dist", type=Path, default=Path("dist"), help="where the desktop builds were collected")
    parser.add_argument("--page", type=Path, help="the reader's web page directory")
    parser.add_argument("--pkg", type=Path, help="the wasm-bindgen output directory")
    parser.add_argument("--apk", type=Path, help="the built Android package")
    parser.add_argument("--deb", type=Path, help="the built Debian package")
    parser.add_argument("--lib", type=Path, help="the directory holding the FFI library and header")
    args = parser.parse_args(argv)

    if args.what == "selftest":
        return selftest()
    if not args.version:
        parser.error("--version is required")

    if args.what == "reader":
        archive = reader(args.version, args.out, args.dist)
        # The parts are replaced by the archive, so they do not ship beside it.
        for name in DESKTOP:
            (args.dist / name).unlink(missing_ok=True)
    elif args.what == "web":
        archive = web(args.version, args.out, args.page, args.pkg)
    elif args.what == "android":
        archive = android(args.version, args.out, args.apk)
    elif args.what == "uos":
        archive = uos(args.version, args.out, args.deb)
    else:
        archive = ffi(args.version, args.out, args.lib)
    print(archive)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
