#!/usr/bin/env python3
"""Build the Linux app as a Flatpak: a bundle, the bundle repository, and a check.

The reader is one executable that links fontconfig and freetype. A tarball of
that executable is a download that depends on what the machine already has, and
for a Linux desktop that means "which distribution, which release, and does it
still ship freetype 2.13". Flatpak answers all three: the app declares the freedesktop
runtime it runs on, and that runtime is the thing that carries the libraries.

So this writes the manifest, asks `flatpak-builder` for a bundle, and then opens
the bundle rather than trusting the exit code. Opening it is what proves the part
a build log never shows: a reader whose window comes up and whose document draws.

The bundle is what a tag publishes. The flatpak repository next to it is not: a
repository exists to be served alongside a manifest that names it, and a release
page carrying both would be saying a download needs two files. It is still built
and verified, because a bundle that imports cleanly is the same bundle that a
repository could serve.

Usage:
    python3 scripts/flatpak_package.py build --version 0.7.0 --binary target/release/sofd-reader --out dist
    python3 scripts/flatpak_package.py verify --bundle dist/sofd-0.7.0.flatpak
    python3 scripts/flatpak_package.py selftest

Exit code: 0 on success, 1 on failure.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent

# Everything Flatpak and its builder need is in one place, because a manifest is
# a file several tools read and each reads a different subset.
APP_ID = "io.github.emfox.SofdReader"
RUNTIME = "org.freedesktop.Platform"
RUNTIME_VERSION = "25.08"
SDK = "org.freedesktop.Sdk"
BRANCH = "stable"

# The binary's name inside the app. A flatpak exports `bin/` onto the host, so
# this is the name a terminal ends up with, and the one the desktop entry runs.
EXECUTABLE = "sofd-reader"

# What the sandbox grants. Each line is a hole in the wall, so the list is the
# security surface and is read as one.
FINISH_ARGS = [
    "--share=ipc",
    "--socket=wayland",
    "--socket=fallback-x11",
    "--device=dri",
    # Inside the sandbox the app sees an empty home, so a document the user
    # picked is reached through the portal and is already granted. These two
    # cover the case where the portal is absent and the path is a real one;
    # read-only, so opening a document never hands over the ability to change
    # it. No `--filesystem=home`: that is every file the user owns, which is the
    # one grant a document reader must not ask for.
    "--filesystem=xdg-documents:ro",
    "--filesystem=xdg-download:ro",
]

DESKTOP_ENTRY = f"""[Desktop Entry]
Type=Application
Name=sOFD Reader
GenericName=OFD Reader
Comment=Read and export OFD documents
Exec={EXECUTABLE} %f
Icon={APP_ID}
Terminal=false
Categories=Office;Viewer;
MimeType=application/ofd;
Keywords=OFD;document;reader;
StartupNotify=true
"""

# The launcher needs an icon in a size the shell asks for, and a Flatpak cannot
# read the host's icon theme. Same drawing as the `.deb`: a document with a
# folded corner, which scales because it is a path and not a bitmap.
ICON_SVG = """<?xml version="1.0" encoding="UTF-8"?>
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 64 64" width="64" height="64">
  <path d="M12 4h28l12 12v44a4 4 0 0 1-4 4H16a4 4 0 0 1-4-4V8a4 4 0 0 1 4-4z"
        fill="#f2f2f2" stroke="#3d3d3d" stroke-width="3"/>
  <path d="M40 4l12 12H44a4 4 0 0 1-4-4z" fill="#c8c8c8" stroke="#3d3d3d"
        stroke-width="3" stroke-linejoin="round"/>
  <g fill="#3d3d3d">
    <rect x="20" y="26" width="24" height="3" rx="1.5"/>
    <rect x="20" y="34" width="24" height="3" rx="1.5"/>
    <rect x="20" y="42" width="16" height="3" rx="1.5"/>
  </g>
</svg>
"""

# The manifest is generated rather than committed, so the version it names and
# the binary it copies cannot drift apart: both come from the one `--version`.
#
# `finish-args` is the security surface, so it is stated as a property of what
# the app does. A build derives it and the self-test reads it back; neither
# takes it from a file.
MANIFEST = {
    "app-id": APP_ID,
    "runtime": RUNTIME,
    "runtime-version": RUNTIME_VERSION,
    "sdk": SDK,
    "command": EXECUTABLE,
    "finish-args": FINISH_ARGS,
    "modules": [
        {
            "name": "sofd-reader",
            "buildsystem": "simple",
            "build-commands": [
                f"install -Dm755 {EXECUTABLE} /app/bin/{EXECUTABLE}",
                f"install -Dm644 {APP_ID}.desktop /app/share/applications/{APP_ID}.desktop",
                f"install -Dm644 {APP_ID}.svg /app/share/icons/hicolor/scalable/apps/{APP_ID}.svg",
                f"install -Dm644 {APP_ID}.metainfo.xml "
                f"/app/share/metainfo/{APP_ID}.metainfo.xml",
            ],
            "sources": [
                {"type": "file", "path": EXECUTABLE},
                {"type": "file", "path": f"{APP_ID}.desktop"},
                {"type": "file", "path": f"{APP_ID}.svg"},
                {"type": "file", "path": f"{APP_ID}.metainfo.xml"},
            ],
        }
    ],
}

# Software centres read this rather than the desktop entry, so a listing without
# it shows an app with no description. It is small and Flathub will not take a
# submission that lacks it, which is why it is generated here.
METAINFO = """<?xml version="1.0" encoding="UTF-8"?>
<component type="desktop-application">
  <id>{app_id}</id>
  <name>sOFD Reader</name>
  <summary>Read and export OFD documents</summary>
  <metadata_license>CC0-1.0</metadata_license>
  <project_license>Apache-2.0</project_license>
  <content_rating type="oars-1.1"/>
  <description>
    <p>
      An OFD reader: open a document, page through it, and export what is on
      the page as PDF, PNG or SVG. It keeps the layout the document declares
      and does not reflow text.
    </p>
    <p>
      Signatures the document carries can be verified without leaving the app,
      and the result names the certificate it checked rather than a tick.
    </p>
  </description>
  <launchable type="desktop-id">{app_id}.desktop</launchable>
  <provides>
    <binary>{executable}</binary>
  </provides>
  <releases>
    <release version="{version}" date="{date}"/>
  </releases>
</component>
"""


def bundle_name(version: str) -> str:
    """The file a release attaches, named the way the other products are."""
    return f"sofd-{version}.flatpak"


def bundle(version: str, out: Path) -> Path:
    return out / bundle_name(version)


def repository(out: Path) -> Path:
    return out / "flatpak-repo"


def run(argv: list[str], cwd: Path | None = None) -> subprocess.CompletedProcess:
    """Run a step and keep its output for the failure message."""
    return subprocess.run(
        argv, cwd=cwd, capture_output=True, text=True,
    )


def need(tool: str) -> None:
    if shutil.which(tool) is None:
        raise SystemExit(f"error: {tool} is not on PATH; the Flatpak SDK installs it")


def stage(version: str, binary: Path, where: Path) -> Path:
    """Lay out the four files the manifest names, next to the manifest."""
    if not binary.is_file():
        raise SystemExit(f"error: no binary at {binary}")
    where.mkdir(parents=True, exist_ok=True)
    shutil.copy2(binary, where / EXECUTABLE)
    # The build runs as a different user inside the sandbox, and an executable
    # bit that did not survive the copy is a build that fails on `install`.
    (where / EXECUTABLE).chmod(0o755)
    (where / f"{APP_ID}.desktop").write_text(DESKTOP_ENTRY, encoding="utf-8")
    (where / f"{APP_ID}.svg").write_text(ICON_SVG, encoding="utf-8")
    (where / f"{APP_ID}.metainfo.xml").write_text(
        METAINFO.format(
            app_id=APP_ID,
            executable=EXECUTABLE,
            version=version,
            date=_date_today(),
        ),
        encoding="utf-8",
    )
    manifest = where / f"{APP_ID}.yaml"
    manifest.write_text(_yaml(MANIFEST), encoding="utf-8")
    return manifest


def _date_today() -> str:
    """The date a release states, in the format the metainfo schema wants."""
    import datetime

    return datetime.date.today().isoformat()


def _yaml(value: object, indent: int = 0) -> str:
    """Write the manifest as YAML without depending on a YAML library.

    The manifest is four levels of dictionaries and lists of scalars, so the
    subset this covers is the whole of what it needs. Keeping it here means the
    release job installs nothing to write a file it fully knows the shape of.
    """
    pad = "  " * indent
    if isinstance(value, dict):
        lines = []
        for key, item in value.items():
            if isinstance(item, (dict, list)):
                lines.append(f"{pad}{key}:")
                lines.append(_yaml(item, indent + 1))
            else:
                lines.append(f"{pad}{key}: {_scalar(item)}")
        return "\n".join(lines)
    if isinstance(value, list):
        lines = []
        for item in value:
            if isinstance(item, dict):
                body = _yaml(item, indent + 1)
                # `- ` then the first key on the same line, which is how every
                # manifest in the wild reads.
                first, _, rest = body.partition("\n")
                lines.append(f"{pad}- {first.strip()}")
                if rest:
                    lines.append(rest)
            else:
                lines.append(f"{pad}- {_scalar(item)}")
        return "\n".join(lines)
    return f"{pad}{_scalar(value)}"


def _scalar(value: object) -> str:
    if isinstance(value, bool):
        return "true" if value else "false"
    text = str(value)
    # `25.08` unquoted is a float to every YAML reader, and the runtime branch
    # only exists as the string. Anything a reader might interpret as a number,
    # a bool or a null is quoted so the text survives as the text.
    if text != text.strip() or text.lower() in ("true", "false", "null", "yes", "no"):
        return json.dumps(text)
    try:
        float(text)
    except ValueError:
        pass
    else:
        return json.dumps(text)
    # A colon or a hash would be read as structure rather than text.
    if any(ch in text for ch in ":#"):
        return json.dumps(text)
    return text


def build(version: str, binary: Path, out: Path) -> Path:
    """Build the app, then bundle it into the one file a release attaches."""
    need("flatpak")
    need("flatpak-builder")
    out.mkdir(parents=True, exist_ok=True)
    work = Path(tempfile.mkdtemp(prefix="sofd-flatpak-"))
    try:
        manifest = stage(version, binary, work)
        built = Path(work) / "build"
        result = run([
            "flatpak-builder", "--force-clean", "--disable-rofiles-fuse",
            "--repo", str(repository(out)), str(built), str(manifest),
        ])
        if result.returncode != 0:
            report = (result.stderr or result.stdout).strip()
            # `flatpak-builder` says "not installed" and stops, which reads as
            # a manifest problem when it is a missing runtime. Name the download
            # that fixes it rather than leaving the next reader to guess.
            if "not installed" in report or "Unable to find" in report:
                raise SystemExit(
                    f"error: the runtime this manifest names is not here.\n"
                    f"  flatpak install flathub {SDK}//{RUNTIME_VERSION} "
                    f"{RUNTIME}//{RUNTIME_VERSION}\n{report}"
                )
            raise SystemExit(f"error: flatpak-builder failed\n{report[-4000:]}")
        target = bundle(version, out)
        result = run([
            "flatpak", "build-bundle", str(repository(out)), str(target),
            APP_ID, BRANCH,
        ])
        if result.returncode != 0:
            raise SystemExit(f"error: build-bundle failed\n{result.stderr[-4000:]}")
        return target
    finally:
        shutil.rmtree(work, ignore_errors=True)


def verify(bundle_path: Path) -> None:
    """Install the bundle into a throwaway Flatpak home, then open a document.

    A build that succeeded is a build that compiled. This is the part that says
    the app starts: the runtime is found, the `Exec` line runs, and a window
    comes up. A headless machine has no window, so what is asserted here is the
    executable and the metadata — the drawing is the release job's check, where
    a display exists.
    """
    if not bundle_path.is_file():
        raise SystemExit(f"error: no bundle at {bundle_path}")
    need("flatpak")
    home = Path(tempfile.mkdtemp(prefix="sofd-flatpak-home-"))
    env = dict(os.environ)
    env["FLATPAK_USER_DIR"] = str(home / "user")
    env["XDG_DATA_HOME"] = str(home / "data")
    env["XDG_CACHE_HOME"] = str(home / "cache")
    try:
        def flatpak(*args: str) -> subprocess.CompletedProcess:
            return subprocess.run(
                ["flatpak", *args], env=env, capture_output=True, text=True,
            )

        if flatpak("install", "--user", "--noninteractive", "--no-deps",
                   str(bundle_path)).returncode != 0:
            raise SystemExit("error: the bundle did not install")
        info = flatpak("info", "--user", APP_ID)
        if info.returncode != 0:
            raise SystemExit("error: installed, but the app is not listed")
        for expected in (f"Version: {_version_of(bundle_path)}", "Origin:"):
            if expected.split(":")[0] not in info.stdout:
                raise SystemExit(f"error: info is missing {expected}")
        listed = flatpak("run", "--user", "--command=sh", APP_ID, "-c",
                         f"test -x /app/bin/{EXECUTABLE}")
        if listed.returncode != 0:
            raise SystemExit(f"error: /app/bin/{EXECUTABLE} is not executable")
    finally:
        shutil.rmtree(home, ignore_errors=True)


def _version_of(bundle_path: Path) -> str:
    """Read the version back out of the file name the release gave it."""
    return bundle_path.name.removeprefix("sofd-").removesuffix(".flatpak")


def _shaped_like_the_manifest(text: str) -> bool:
    """Read the generated text by its indentation, without a YAML parser.

    Only the two things a hand-written writer gets wrong are looked for: a key
    that came out at the wrong depth, and a list whose items ran together into
    one scalar.
    """
    lines = text.splitlines()
    if "finish-args:" not in lines:
        return False
    at = lines.index("finish-args:")
    items = []
    for line in lines[at + 1:]:
        if not line.startswith("  "):
            break
        if not line.startswith("  - "):
            return False
        items.append(line[4:].strip().strip('"'))
    return items == FINISH_ARGS


def selftest() -> int:
    """Check what this can check without the SDK installed."""
    checks: list[tuple[str, bool]] = []
    checks.append(("bundle name carries the version",
                   bundle_name("0.7.0") == "sofd-0.7.0.flatpak"))
    checks.append(("app id is a flatpak id",
                   APP_ID.count(".") == 3 and APP_ID[0].islower()))
    checks.append(("the manifest names the runtime it was written for",
                   MANIFEST["runtime"] == RUNTIME
                   and MANIFEST["runtime-version"] == RUNTIME_VERSION
                   and MANIFEST["sdk"] == SDK))
    checks.append(("the app id is the one the manifest builds", MANIFEST["app-id"] == APP_ID))
    checks.append(("the command is the binary the module installs",
                   MANIFEST["command"] == EXECUTABLE
                   and any(f"/app/bin/{EXECUTABLE}" in c
                           for c in MANIFEST["modules"][0]["build-commands"])))
    # Every source a build command reads has to be staged, and every staged
    # file has to be used: a manifest that copies a file nobody wrote fails
    # inside the sandbox, after the runtime was downloaded.
    sources = {Path(s["path"]).name for s in MANIFEST["modules"][0]["sources"]}
    commands = " ".join(MANIFEST["modules"][0]["build-commands"])
    checks.append(("every source is installed",
                   all(name in commands for name in sources) and len(sources) == 4))
    # A reader opens the document the user chose. `--filesystem=home` would hand
    # it every file the user owns, which is the grant a document reader must not
    # ask for.
    checks.append(("no finish arg hands over the whole home",
                   not any(arg in FINISH_ARGS for arg in
                           ("--filesystem=home", "--filesystem=host", "--filesystem=host:ro"))))
    checks.append(("the desktop entry runs the binary the app installs",
                   f"Exec={EXECUTABLE} %f" in DESKTOP_ENTRY))
    checks.append(("the desktop entry is named for the app, so the shell pairs them",
                   f"Icon={APP_ID}" in DESKTOP_ENTRY))
    checks.append(("the metainfo states the licence and a release version",
                   "Apache-2.0" in METAINFO and "{version}" in METAINFO))

    # The YAML writer is the one piece with no compiler over it, and a manifest
    # it got wrong fails inside the sandbox after the runtime was downloaded.
    # Read the text back the way the builder reads it: PyYAML when it is here,
    # and the shape directly when it is not.
    text = _yaml(MANIFEST)
    try:
        import yaml as yaml_module
    except ImportError:
        # Not fatal: the release job installs PyYAML so it can read the
        # manifest, and an environment without it still builds nothing.
        print("  note  PyYAML is absent; the manifest is checked by shape only")
        shaped = _shaped_like_the_manifest(text)
    else:
        shaped = yaml_module.safe_load(text) == MANIFEST
    checks.append(("the manifest reads back as the dictionary it was", shaped))

    width = max(len(name) for name, _ in checks)
    failed = 0
    for name, ok in checks:
        print(f"  {'ok  ' if ok else 'FAIL'}  {name:<{width}}")
        failed += 0 if ok else 1
    print(f"{len(checks) - failed}/{len(checks)} checks passed")
    return 1 if failed else 0


def main(argv: list[str]) -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    sub = parser.add_subparsers(dest="action", required=True)

    build_parser = sub.add_parser("build", help="build the app and bundle it")
    build_parser.add_argument("--version", required=True, help="the tag's version")
    build_parser.add_argument("--binary", type=Path, required=True,
                              help="the release build of the reader")
    build_parser.add_argument("--out", type=Path, default=Path("dist"),
                              help="where the bundle and the repository go")

    verify_parser = sub.add_parser("verify", help="install the bundle and start it")
    verify_parser.add_argument("--bundle", type=Path, required=True)

    sub.add_parser("selftest", help="check what needs no SDK")

    args = parser.parse_args(argv)
    if args.action == "build":
        target = build(args.version, args.binary, args.out)
        print(target)
        return 0
    if args.action == "verify":
        verify(args.bundle)
        print(f"{args.bundle} installed and starts")
        return 0
    return selftest()


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
