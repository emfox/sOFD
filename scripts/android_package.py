#!/usr/bin/env python3
"""
sOFD Android package builder.

The reader is one activity that draws with `eframe`, so the package needs very
little: a manifest naming that activity, the native library it loads, and the
signature an installable archive must carry. This tool assembles exactly that
and calls the SDK's own tools for the steps that belong to them, rather than
carrying a Gradle project for three files.

The archive is signed with a debug key generated per run. It installs on a
device that allows sideloading, which is what a release page offers; it is not
a Play Store upload, and the key is not kept.

Usage:
    python3 scripts/android_package.py --sdk DIR --lib PATH --out DIR
    python3 scripts/android_package.py selftest

Exit code: 0 on success, 1 on failure.
"""

from __future__ import annotations

import argparse
import re
import shutil
import subprocess
import sys
import tempfile
import zipfile
from pathlib import Path

PACKAGE = "org.sofd.reader"
ACTIVITY = "android.app.NativeActivity"
MIN_SDK = 24
TARGET_SDK = 34
# The archive the release publishes, named so the workflow can checksum it
# beside the desktop executables rather than in a directory of its own.
ARCHIVE_NAME = "sofd-reader-android.apk"
# The name `System.loadLibrary` looks for, which is the crate's library name.
LIBRARY = "sofd_reader"
# A `NativeActivity` that is asked to handle a configuration change itself is
# not recreated for it. Recreation is what a reader cannot survive: the activity
# it holds is destroyed, and with it the EGL surface the frame is drawn on, so a
# rotation or a font-size change would end in a blank window. Every change the
# frame can absorb is therefore claimed here rather than left to the system.
CONFIG_CHANGES = "|".join(
    [
        "colorMode",
        "density",
        "fontScale",
        "fontWeightAdjustment",
        "keyboard",
        "keyboardHidden",
        "layoutDirection",
        "locale",
        "mcc",
        "mnc",
        "navigation",
        "orientation",
        "screenLayout",
        "screenSize",
        "smallestScreenSize",
        "touchscreen",
        "uiMode",
    ]
)
# The types a share can carry, and the reason there is more than one: a file
# manager that cannot name OFD sends the container's type instead, so a filter
# that named only `application/ofd` left the reader absent from the sheet that
# offers it. A share of the wrong type opens and is refused by the reader,
# which says so, and that is better than a share menu with no reader in it.
SEND_MIMES = ["application/ofd", "application/zip", "application/octet-stream"]
# The reader crate's manifest. The `versionCode` is the reader's version, not a
# workspace-wide one: there is no workspace-wide one any more, and the archive
# holds the reader and nothing else.
READER_TOML = Path(__file__).resolve().parent.parent / "crates" / "sofd-reader" / "Cargo.toml"


def version_code(version: str) -> int:
    """The integer Android orders releases by, from a semantic version.

    Android compares `versionCode` as a plain integer and refuses an archive
    that reuses one, so it has to move with the version rather than be written
    beside it: `major*10000 + minor*100 + patch`, which holds until a minor
    reaches 100. A prerelease sorts with the release it precedes, because the
    store has no way to say "before".
    """
    match = re.match(r"^(\d+)\.(\d+)\.(\d+)", version)
    if not match:
        raise SystemExit(f"error: {version} is not a version")
    major, minor, patch = (int(part) for part in match.groups())
    return major * 10_000 + minor * 100 + patch


def reader_version() -> str:
    """The version the reader crate states."""
    text = READER_TOML.read_text(encoding="utf-8")
    block = re.search(r"^\[package\]\n(.*?)(?=^\[|\Z)", text, re.M | re.S)
    match = re.search(r'^version\s*=\s*"([^"]+)"', block.group(1) if block else "", re.M)
    if not match:
        raise SystemExit(f"error: {READER_TOML} names no version")
    return match.group(1)


# Written out rather than left to the packager, which defaults both to empty.
# Both are filled from the workspace version when the manifest is written, so a
# release cannot ship a package that claims to be the one before it.
MANIFEST = """<?xml version="1.0" encoding="utf-8"?>
<manifest xmlns:android="http://schemas.android.com/apk/res/android"
    package="{package}"
    android:versionCode="{version_code}"
    android:versionName="{version_name}">
    <uses-sdk android:minSdkVersion="{min_sdk}" android:targetSdkVersion="{target_sdk}" />
    <application android:label="sOFD Reader" android:hasCode="false">
        <activity
            android:name="{activity}"
            android:exported="true"
            android:configChanges="{config_changes}"
            android:screenOrientation="unspecified">
            <meta-data android:name="android.app.lib_name" android:value="{library}" />
            <intent-filter>
                <action android:name="android.intent.action.MAIN" />
                <category android:name="android.intent.category.LAUNCHER" />
            </intent-filter>
            <!-- The reader has no file dialog on Android: `rfd` has no backend
                 there, so a document arrives through one of these two. A tap in
                 a file manager sends VIEW, a share from any app sends SEND, and
                 both hand over a content URI the reader copies the bytes of.
                 The suffix rather than a pattern: `.*\.ofd` stops at the first
                 dot, so `a.b.ofd` would never match, and a provider whose MIME
                 type it cannot name is the common case for a sideloaded one. -->
            <intent-filter android:label="Open an OFD">
                <action android:name="android.intent.action.VIEW" />
                <category android:name="android.intent.category.DEFAULT" />
                <category android:name="android.intent.category.BROWSABLE" />
                <data android:scheme="content" android:host="*" android:mimeType="application/ofd" />
                <data android:scheme="content" android:host="*" android:pathSuffix=".ofd" />
                <data android:scheme="file" android:pathSuffix=".ofd" />
                <data android:scheme="file" android:mimeType="application/ofd" />
                <data android:mimeType="application/ofd" />
            </intent-filter>
            <intent-filter android:label="Open an OFD">
                <action android:name="android.intent.action.SEND" />
                <category android:name="android.intent.category.DEFAULT" />
{send_mimes}            </intent-filter>
        </activity>
    </application>
</manifest>
"""


def manifest_text(version: str) -> str:
    """The manifest for one version, with every count filled in."""
    send_mimes = "".join(
        f'                <data android:mimeType="{mime}" />\n' for mime in SEND_MIMES
    )
    return MANIFEST.format(
        package=PACKAGE,
        version_code=version_code(version),
        version_name=version,
        min_sdk=MIN_SDK,
        target_sdk=TARGET_SDK,
        activity=ACTIVITY,
        library=LIBRARY,
        config_changes=CONFIG_CHANGES,
        send_mimes=send_mimes,
    )


# The directory the loader reads the library out of. Its name is the ABI, and a
# library filed under the wrong one is installed happily and then fails at
# `dlopen` with a bare architecture mismatch, so the ABI is read from the
# library rather than assumed: `--lib` may be any target the workflow builds.
ABI_DIRS = {
    0xB7: "arm64-v8a",  # EM_AARCH64
    0x3E: "x86_64",  # EM_X86_64
}


def el_fake(machine: int, *, sixty_four_bit: bool = True) -> Path:
    """A file holding just enough of an ELF header to be told apart."""
    header = bytearray(20)
    header[:4] = b"\x7fELF"
    header[4] = 2 if sixty_four_bit else 1
    header[18:20] = machine.to_bytes(2, "little")
    path = Path(tempfile.mkdtemp()) / "libfake.so"
    path.write_bytes(bytes(header))
    return path


def library_abi(path: Path) -> str:
    """The ABI directory a library belongs in, from its ELF header."""
    header = path.open("rb").read(20)
    if len(header) < 20 or header[:4] != b"\x7fELF":
        raise SystemExit(f"error: {path} is not an ELF library")
    if header[4] != 2:
        raise SystemExit(f"error: {path} is not a 64-bit library")
    machine = int.from_bytes(header[18:20], "little")
    if machine not in ABI_DIRS:
        raise SystemExit(f"error: {path} is built for machine {machine:#x}, which no ABI dir names")
    return f"lib/{ABI_DIRS[machine]}"


def build_tool(sdk: Path, name: str) -> str:
    """The SDK's copy of one build tool, newest build-tools first."""
    found = sorted((sdk / "build-tools").glob(f"*/{name}"))
    if not found:
        raise SystemExit(f"error: {name} is not in {sdk / 'build-tools'}")
    return str(found[-1])


def java_tool(name: str) -> str:
    """`keytool` ships with the JDK rather than the SDK, so it is found by PATH."""
    found = shutil.which(name)
    if found is None:
        raise SystemExit(f"error: {name} is not on PATH")
    return found


def run(command: list[str]) -> None:
    """Run one tool, and fail loudly: a half-built archive is not published."""
    printable = " ".join(command)
    print(f"+ {printable}")
    code = subprocess.run(command, check=False).returncode
    if code != 0:
        raise SystemExit(f"error: {printable} failed with {code}")


def write_manifest(work: Path, version: str) -> Path:
    """Write the manifest out, so the packager reads it from a file."""
    manifest = work / "AndroidManifest.xml"
    manifest.write_text(manifest_text(version), encoding="utf-8")
    return manifest


def write_archive(work: Path, library: Path, compiled: Path, abi_dir: str) -> Path:
    """Add the library to the archive the packager built, and return it.

    The manifest inside must stay the binary form the packager wrote: the
    signer reads the minimum SDK out of it, and source text has no chunks.
    """
    unsigned = work / "unsigned.apk"
    shutil.copyfile(compiled, unsigned)
    with zipfile.ZipFile(unsigned, "a", zipfile.ZIP_DEFLATED) as archive:
        archive.write(library, f"{abi_dir}/{library.name}")
    return unsigned


def keystore(work: Path) -> Path:
    """A throwaway key: the release page is not a store, and keys are not kept."""
    path = work / "debug.keystore"
    run([
        java_tool("keytool"), "-genkeypair",
        "-keystore", str(path),
        "-alias", "sofd",
        "-storepass", "sofdpass",
        "-keypass", "sofdpass",
        "-keyalg", "RSA",
        "-keysize", "2048",
        "-validity", "10000",
        "-dname", "CN=sOFD Reader",
    ])
    return path


def package(sdk: Path, library: Path, out: Path) -> Path:
    """Compile, align and sign the archive into `out`, beside everything else.

    `out` holds the rest of the release too, so the scaffolding is staged beside
    a temporary directory instead of beside the archive, and only the archive is
    written here: emptying this directory would take the notes and any part of
    the manifest a re-run had already gathered with it.
    """
    work = Path(tempfile.mkdtemp(prefix="sofd-android-"))
    try:
        version = reader_version()
        manifest = write_manifest(work, version)

        # The resource table has to be the SDK's own binary form: the signer reads
        # the minimum SDK out of a compiled manifest, so the archive cannot be a
        # plain zip of the source text.
        base = build_tool(sdk, "aapt")
        assets = work / "assets"
        assets.mkdir()
        compiled = work / "compiled.apk"
        platform = sorted((sdk / "platforms").glob("android-*"))[-1]
        run([
            base, "package", "-f", "-M", str(manifest),
            "-I", str(platform / "android.jar"), "-A", str(assets), "-F", str(compiled),
        ])

        unsigned = write_archive(work, library, compiled, library_abi(library))
        # Alignment must happen before signing: a signer rewrites the archive, and
        # an unaligned entry cannot be mapped by the loader afterwards.
        aligned = work / "aligned.apk"
        run([build_tool(sdk, "zipalign"), "-f", "4", str(unsigned), str(aligned)])

        key = keystore(work)
        signed = work / ARCHIVE_NAME
        run([
            build_tool(sdk, "apksigner"), "sign",
            "--ks", str(key),
            "--ks-pass", "pass:sofdpass",
            "--ks-key-alias", "sofd",
            "--key-pass", "pass:sofdpass",
            "--out", str(signed),
            str(aligned),
        ])
        # A signature that does not verify is an archive no device will install,
        # so the build fails here rather than at the downloader.
        run([build_tool(sdk, "apksigner"), "verify", str(signed)])
        # Only the archive is published; the manifest, the key and the unsigned
        # copies are scaffolding, and a leftover would be checksummed beside it.
        out.mkdir(parents=True, exist_ok=True)
        published = out / ARCHIVE_NAME
        shutil.copyfile(signed, published)
    finally:
        shutil.rmtree(work, ignore_errors=True)
    return published


def selftest() -> int:
    """The parts that need no SDK: the manifest and the archive layout."""
    version = reader_version()
    rendered = manifest_text(version)
    assert f'package="{PACKAGE}"' in rendered
    assert f'android:value="{LIBRARY}"' in rendered
    assert f'android:name="{ACTIVITY}"' in rendered
    assert 'android:hasCode="false"' in rendered
    assert MIN_SDK < TARGET_SDK

    # The ABI directory comes from the library, so a package cannot be built
    # for one architecture and filed under another: that installs cleanly and
    # then fails at `dlopen` on the device.
    assert library_abi(el_fake(0xB7)) == "lib/arm64-v8a"
    assert library_abi(el_fake(0x3E)) == "lib/x86_64"

    # The version is read rather than written twice, so a release cannot ship a
    # package whose `versionCode` still names the release before it.
    assert f'android:versionName="{version}"' in rendered, version
    assert f'android:versionCode="{version_code(version)}"' in rendered
    assert version_code("0.4.0") == 400
    assert version_code("1.2.3") == 10203
    assert version_code("0.4.0-rc.1") == 400, "a prerelease sorts with its release"
    assert version_code("0.4.1") > version_code("0.4.0"), "a patch is a higher code"
    assert version_code("0.5.0") > version_code("0.4.9"), "a minor outranks every patch"

    # A document can only reach the reader through one of these two actions:
    # there is no file dialog to fall back on, so their absence is the reader
    # starting on a canvas nothing can fill.
    assert "android.intent.action.VIEW" in rendered
    assert "android.intent.action.SEND" in rendered
    assert 'android:mimeType="application/ofd"' in rendered
    assert 'android:pathSuffix=".ofd"' in rendered
    # A share the reader is offered has to arrive even when the sender names
    # the container rather than the format, which most file managers do.
    send = rendered.split("android.intent.action.SEND", 1)[1].split("</intent-filter>", 1)[0]
    for mime in SEND_MIMES:
        assert f'android:mimeType="{mime}"' in send, mime

    # A configuration change the activity does not claim recreates it, and the
    # recreation destroys the surface the frame is drawn on: the reader then
    # shows a blank window after a rotation or a theme switch, which is the
    # failure this attribute exists to prevent.
    claimed = CONFIG_CHANGES.split("|")
    for required in ("orientation", "screenSize", "smallestScreenSize", "density", "uiMode"):
        assert required in claimed, required
    assert f'android:configChanges="{CONFIG_CHANGES}"' in rendered
    assert "keyboardHidden" in claimed, "a hardware keyboard must not rebuild the app"

    # The manifest is XML, and the packager rejects one that will not parse:
    # a comment may not hold a double hyphen, and an attribute may not be bare.
    assert "--" not in rendered.split("<application")[0].split("<manifest")[1]
    assert "&" not in rendered.replace("&amp;", ""), "an unescaped ampersand"

    # The archive is laid out the same way here, so a change to the layout is
    # caught without an SDK or a toolchain.
    with tempfile.TemporaryDirectory() as tmp:
        work = Path(tmp)
        library = work / f"lib{LIBRARY}.so"
        library.write_bytes(el_fake(0xB7).read_bytes())
        compiled = work / "compiled.apk"
        with zipfile.ZipFile(compiled, "w", zipfile.ZIP_DEFLATED) as archive:
            archive.write(write_manifest(work, version), "AndroidManifest.xml")
        unsigned = write_archive(work, library, compiled, library_abi(library))
        with zipfile.ZipFile(unsigned) as archive:
            names = set(archive.namelist())
    expected = {
        "AndroidManifest.xml",
        f"lib/arm64-v8a/lib{LIBRARY}.so",
    }
    assert names == expected, names
    print("selftest passed")
    return 0


def main(argv: list[str]) -> int:
    if argv and argv[0] == "selftest":
        return selftest()
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--sdk", type=Path, required=True, help="Android SDK root")
    parser.add_argument("--lib", type=Path, required=True, help="the built .so")
    parser.add_argument("--out", type=Path, required=True, help="where the archive goes")
    args = parser.parse_args(argv)

    if not args.lib.is_file():
        raise SystemExit(f"error: {args.lib} is not a file")
    signed = package(args.sdk, args.lib, args.out)
    print(f"built {signed}")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
