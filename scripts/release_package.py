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

`reader` is the one product that is not one archive. The three desktop builds
are three downloads of about twenty megabytes each, and a reader for Linux
should not have to fetch a macOS binary to get one file: a reader for one
system holds that executable alone, which is also what its own checksum then
covers.

Its archive is the one of the system it runs on, because a download that needs
a tool installed first is not a portable build. Windows has had a ZIP reader
since it was Windows, Linux has had `tar.gz`, and macOS has a ZIP that keeps
the executable bit a bare binary loses in transit. `FORMAT` states which is
which; nothing here guesses from the file name.

Usage:
    python3 scripts/release_package.py reader  --version 0.5.1 --out dist --dist build \
        --system linux
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

# The reader's desktop builds, named for the system each runs on. The Windows
# one keeps its `.exe` here: the workflow copies the build out under this name,
# and a file without the extension is one Explorer cannot start.
DESKTOP = {
    "linux": "sofd-reader-linux",
    "macos": "sofd-reader-macos",
    "windows": "sofd-reader-windows.exe",
}
# What each one is called inside its archive, so an unpacked file runs as-is.
INSIDE = {
    "linux": "sofd-reader",
    "macos": "sofd-reader",
    "windows": "sofd-reader.exe",
}
# The archive each system opens without being asked to install anything else.
# Windows ships a ZIP reader and nothing for `.tar.gz`; macOS ships `zip` and
# `tar`, but a ZIP is what preserved the executable bit when this was checked,
# and it is what a macOS user expects to double-click. Linux gets `tar.gz`,
# which keeps the mode bits and is what `tar xf` in a build script already
# reads.
FORMAT = {
    "linux": "tar.gz",
    "macos": "zip",
    "windows": "zip",
}

# The FFI library under each name a linker is given, plus its header.
FFI_LIBRARIES = ("libsofd_ffi.so", "libsofd_ffi.a")


def product(name: str, version: str) -> str:
    """The archive one product ships as."""
    return f"sofd-{version}-{name}"


def _native(system: str) -> str:
    """The extension a system's own archive has, spelled out.

    Deliberately not `FORMAT[system]`: this is the answer the self-test is
    checking, and a test that reads it from the table under test agrees with
    whatever the table says, including a Windows reader shipped as `.tar.gz`.
    """
    return {
        "linux": ".tar.gz",
        "macos": ".zip",
        "windows": ".zip",
    }[system]


def require(paths: list[Path], product_name: str) -> None:
    """Stop on a missing input rather than shipping a half-filled archive."""
    missing = [str(p) for p in paths if not p.exists()]
    if missing:
        raise SystemExit(f"error: {product_name} needs {', '.join(missing)}")


def write_zip(archive: Path, root: Path) -> None:
    """Zip every file below `root`, with `root` itself as the top directory.

    The mode is set on the member rather than taken from the file: `ZipInfo`
    from `write()` carries an MS-DOS attribute word, and what a reader on Linux
    or macOS uses to decide "this is executable" is the Unix mode in the
    external-attributes field. Without it, an unpacked binary is a file the
    system refuses to run — which is exactly the failure a portable build
    cannot have.
    """
    archive.parent.mkdir(parents=True, exist_ok=True)
    with zipfile.ZipFile(archive, "w", zipfile.ZIP_DEFLATED) as bundle:
        for path in sorted(root.rglob("*")):
            info = zipfile.ZipInfo.from_file(path, path.relative_to(root.parent))
            info.compress_type = zipfile.ZIP_DEFLATED
            if path.is_dir():
                bundle.writestr(info, b"")
                continue
            # `from_file` records the mode in the high 16 bits, which is where
            # every unarchiver reads it from; `external_attr` needs it there.
            mode = path.stat().st_mode
            info.external_attr = (mode & 0xFFFF) << 16
            bundle.writestr(info, path.read_bytes())


def write_tar(archive: Path, root: Path) -> None:
    """The same layout as a `.tar.gz`, which keeps the executable bit."""
    archive.parent.mkdir(parents=True, exist_ok=True)
    with tarfile.open(archive, "w:gz") as tar:
        tar.add(root, arcname=root.name)


def pack(archive: Path, root: Path, fmt: str) -> None:
    """Write `root` as `fmt`, which is `tar.gz` or `zip`."""
    if fmt == "tar.gz":
        write_tar(archive, root)
    elif fmt == "zip":
        write_zip(archive, root)
    else:
        raise SystemExit(f"error: {fmt} is not an archive this writes")


def reader(version: str, out: Path, dist: Path, system: str) -> Path:
    """One desktop build, one archive in that system's own format."""
    source = dist / DESKTOP[system]
    require([source], f"the {system} reader")
    name = product(system, version)
    staging = out / name
    shutil.rmtree(staging, ignore_errors=True)
    staging.mkdir(parents=True)
    target = staging / INSIDE[system]
    shutil.copy2(source, target)
    # The build is unsigned and the file is what a user runs, so the bit that
    # makes it run has to survive the archive.
    target.chmod(0o755)
    archive = out / f"{name}.{FORMAT[system]}"
    pack(archive, staging, FORMAT[system])
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


def image(version: str, out: Path, image_tar: Path) -> Path:
    """The server's container image, as the archive `docker load` reads.

    Not zipped again: `docker save` already writes a tar of layers, and a
    `.tar.gz` of it would have to be unpacked by hand before `docker load`
    would take it. The name says what it is and how to load it.
    """
    require([image_tar], "image")
    archive = out / f"{product('server-image', version)}.tar"
    archive.parent.mkdir(parents=True, exist_ok=True)
    shutil.copy2(image_tar, archive)
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

    # Named the way the workflow's `Collect the executable` step names them,
    # spelled out rather than read from DESKTOP: a self-test that takes its
    # input from the table it is testing passes whatever the table says.
    for name in ("sofd-reader-linux", "sofd-reader-macos", "sofd-reader-windows.exe"):
        (dist / name).write_bytes(b"reader")
    (page / "index.html").write_text("<html></html>")
    (pkg / "sofd_reader.js").write_text("export {}")
    (pkg / "sofd_reader_bg.wasm").write_bytes(b"\0asm")
    (pkg / "README.md").write_text("generated")
    for name in FFI_LIBRARIES + ("sofd.h",):
        (lib / name).write_bytes(b"ffi")
    (tmp / "reader.apk").write_bytes(b"apk")
    (tmp / "reader-uos-amd64.deb").write_bytes(b"deb")

    for system, inside in INSIDE.items():
        bundle = reader("1.2.3", out, dist, system)
        entry = f"sofd-1.2.3-{system}/{inside}"
        # Each system's own format is the answer being checked, so the format is
        # read back out of the archive rather than compared with `FORMAT`: a
        # table that changed its mind would still pass a comparison with itself.
        if zipfile.is_zipfile(bundle):
            with zipfile.ZipFile(bundle) as archive:
                members = {name: archive.getinfo(name) for name in archive.namelist()}
            mode = members[entry].external_attr >> 16
            # A ZIP records the directory only if something wrote it; the file
            # under it is what an unarchiver creates the directory from, so the
            # layout is one member and the top directory is implied.
            layout = set(members) == {entry}
        else:
            with tarfile.open(bundle) as archive:
                members = {member.name: member for member in archive.getmembers()}
            mode = members[entry].mode
            # `tar` writes the directory itself, and the executable beside it.
            layout = set(members) == {f"sofd-1.2.3-{system}", entry}
        checks.append((
            f"the {system} reader ships alone, as {inside}, in a format {system} opens",
            layout
            and bundle.name.endswith(_native(system))
            and bool(mode & 0o111),
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

    image_tar = tmp / "image.tar"
    image_tar.write_bytes(b"docker save")
    bundle = image("1.2.3", out, image_tar)
    # `docker load` reads a tar, not a compressed one, so the suffix is part of
    # the answer: a `.tar.gz` would have to be unpacked by hand first.
    checks.append((
        "the server image is named for the release and loads as written",
        bundle.name == "sofd-1.2.3-server-image.tar" and bundle.read_bytes() == b"docker save",
    ))

    try:
        reader("1.2.3", out, tmp / "empty", "linux")
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
    parser.add_argument("what", choices=["reader", "web", "android", "uos", "ffi", "image", "selftest"])
    parser.add_argument("--version", help="the release, without the leading v")
    parser.add_argument("--out", type=Path, default=Path("dist"), help="where the archives go")
    parser.add_argument("--dist", type=Path, default=Path("dist"), help="where the desktop builds were collected")
    parser.add_argument("--page", type=Path, help="the reader's web page directory")
    parser.add_argument("--pkg", type=Path, help="the wasm-bindgen output directory")
    parser.add_argument("--apk", type=Path, help="the built Android package")
    parser.add_argument("--deb", type=Path, help="the built Debian package")
    parser.add_argument("--lib", type=Path, help="the directory holding the FFI library and header")
    parser.add_argument("--system", help="for reader: linux, macos or windows")
    parser.add_argument("--image", type=Path, help="the container image, as `docker save` wrote it")
    args = parser.parse_args(argv)

    if args.what == "selftest":
        return selftest()
    if not args.version:
        parser.error("--version is required")

    if args.what == "reader":
        if args.system not in DESKTOP:
            parser.error(f"--system must be one of {', '.join(DESKTOP)}")
        archive = reader(args.version, args.out, args.dist, args.system)
        # The part is replaced by the archive, so it does not ship beside it.
        (args.dist / DESKTOP[args.system]).unlink(missing_ok=True)
    elif args.what == "web":
        archive = web(args.version, args.out, args.page, args.pkg)
    elif args.what == "android":
        archive = android(args.version, args.out, args.apk)
    elif args.what == "uos":
        archive = uos(args.version, args.out, args.deb)
    elif args.what == "image":
        archive = image(args.version, args.out, args.image)
    else:
        archive = ffi(args.version, args.out, args.lib)
    print(archive)
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
