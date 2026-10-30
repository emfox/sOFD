#!/usr/bin/env python3
"""
sOFD Android package builder.

The reader is one activity that draws with `eframe`, so the package needs very
little: a manifest naming that activity, the native library it loads, the dex of
the one fragment that receives the system picker's answer, and the signature an
installable archive must carry. This tool assembles exactly that and calls the
SDK's own tools for the steps that belong to them, rather than carrying a
Gradle project for four files.

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
LIBRARY = "sofd_egui"
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
# The types an open can carry. The same list as a share, for the same reason,
# and the omission of `octet-stream` here is what made a share appear in the
# sheet while an open did not: the file manager had already failed to name the
# file and sent what it sends for anything it does not know.
VIEW_MIMES = ["application/ofd", "application/octet-stream"]
# What the system is told an `.ofd` is. Android resolves an extension through
# this table when the sender did not state a type, and nothing had added `.ofd`
# to it: a file manager looked the suffix up, found nothing, and fell back to
# `octet-stream` -- which the `VIEW` filter did not accept. The file names the
# type, the manifest accepts it, and the two have to agree.
MIME_TYPES = {"application/ofd": ["ofd"]}
# The filter that catches a sender which matched neither a type nor a suffix.
# A file manager is free to send an intent that states nothing, and a filter
# cannot name "nothing" -- the only way to answer it is to match everything and
# refuse the document once the bytes are read, which the reader already does.
# Reported from a phone as "the open sheet does not list sOFD at all", which is
# this filter's absence rather than any one attribute being wrong. It states a
# type and no scheme: elements are alternatives, so a separate scheme element
# would take every file off a content provider, `notes.txt` included.
CATCH_ALL = True
# The reader crate's manifest. The `versionCode` is the reader's version, not a
# workspace-wide one: there is no workspace-wide one any more, and the archive
# holds the reader and nothing else.
READER_TOML = Path(__file__).resolve().parent.parent / "crates" / "sofd-egui" / "Cargo.toml"
# The one Java class in the project. A `NativeActivity` has no
# `onActivityResult` to override, so the answer the system picker sends back is
# received by a fragment attached to the activity that already exists. It is
# compiled here rather than kept as a dex, so the source is reviewable.
PICKER_SOURCE = READER_TOML.parent / "android" / "PickFragment.java"


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
    <application android:label="sOFD Reader" android:hasCode="true">
        <activity
            android:name="{activity}"
            android:theme="@android:style/Theme.DeviceDefault.NoActionBar"
            android:exported="true"
            android:configChanges="{config_changes}"
            android:screenOrientation="unspecified">
            <meta-data android:name="android.app.lib_name" android:value="{library}" />
            <intent-filter>
                <action android:name="android.intent.action.MAIN" />
                <category android:name="android.intent.category.LAUNCHER" />
            </intent-filter>
            <!-- The reader has no file dialog on Android: `rfd` has no backend
                 there, so a document arrives through one of these filters. A tap
                 in a file manager sends VIEW, a share from any app sends SEND,
                 and both hand over a content URI the reader copies the bytes of.
                 The suffix rather than a pattern: `.*\.ofd` stops at the first
                 dot, so `a.b.ofd` would never match. The type is listed beside
                 it because a provider that resolves the suffix sends the type
                 instead, and `octet-stream` because one that resolves neither
                 sends that. -->
            <intent-filter android:label="Open an OFD">
                <action android:name="android.intent.action.VIEW" />
                <category android:name="android.intent.category.DEFAULT" />
                <category android:name="android.intent.category.BROWSABLE" />
                <data android:scheme="content" android:host="*" android:pathSuffix=".ofd" />
                <data android:scheme="file" android:pathSuffix=".ofd" />
{view_mimes}            </intent-filter>
            <!-- A sender that states neither a type nor a readable suffix cannot
                 be named by a filter, so this one matches everything and the
                 reader refuses what it cannot open once the bytes arrive. -->
            <intent-filter android:label="Open an OFD">
                <action android:name="android.intent.action.VIEW" />
                <category android:name="android.intent.category.DEFAULT" />
                <data android:mimeType="*/*" />
            </intent-filter>
            <!-- A share arrives as one item with a type and no suffix: the
                 sender names what it thinks the document is, and a share sheet
                 asks the receiver whether it takes that type. So the types are
                 listed, and nothing about a file name is required. -->
            <intent-filter android:label="Open an OFD">
                <action android:name="android.intent.action.SEND" />
                <category android:name="android.intent.category.DEFAULT" />
{send_mimes}            </intent-filter>
            <!-- A share that states no type at all matches no named type. It is
                 the same trade as the catch-all open: an entry in somebody
                 else's share sheet is worth a refusal once the bytes arrive,
                 and without this the reader is simply absent from the sheet. -->
            <intent-filter android:label="Open an OFD">
                <action android:name="android.intent.action.SEND" />
                <category android:name="android.intent.category.DEFAULT" />
                <data android:mimeType="*/*" />
            </intent-filter>
        </activity>
    </application>
</manifest>
"""


def manifest_text(version: str) -> str:
    """The manifest for one version, with every count filled in."""
    send_mimes = "".join(
        f'                <data android:mimeType="{mime}" />\n' for mime in SEND_MIMES
    )
    # Both schemes on every type: a provider hands out `content://` and a file
    # manager over shared storage hands out `file://`, and a type listed for
    # only one of them is a filter half-present.
    view_mimes = "".join(
        f'                <data android:scheme="{scheme}" android:host="*" '
        f'android:mimeType="{mime}" />\n'
        for mime in VIEW_MIMES
        for scheme in ("content", "file")
    )
    rendered = MANIFEST.format(
        package=PACKAGE,
        version_code=version_code(version),
        version_name=version,
        min_sdk=MIN_SDK,
        target_sdk=TARGET_SDK,
        activity=ACTIVITY,
        library=LIBRARY,
        config_changes=CONFIG_CHANGES,
        send_mimes=send_mimes,
        view_mimes=view_mimes,
    )
    # A catch-all is a filter that matches more than this app can open, so it is
    # written only when that trade is wanted; the flag is here so the decision
    # is in one place rather than in the template.
    if not CATCH_ALL:
        rendered = re.sub(r"\s*<!-- A sender that states.*?</intent-filter>", "", rendered, flags=re.S)
    return rendered


def mime_types_text() -> str:
    """The extension table stored in the archive's assets.

    Android reads it to answer "what is a `.ofd`" for an app that only has a
    file name. Without the entry a file manager cannot name the type, and what
    it sends instead is not what the manifest lists.
    """
    lines = []
    for mime, suffixes in MIME_TYPES.items():
        for suffix in suffixes:
            lines.append(f"{mime}\t{suffix}")
    return "\n".join(lines) + "\n"


# One `<intent-filter>` as the system reads it: an action, a set of categories
# and a set of data attributes, each of which is scheme/host/type.
def parse_filters(manifest: str) -> list[dict]:
    """Every intent-filter in a rendered manifest, with its parts separated.

    A filter is matched as a whole and a data attribute is matched as a whole:
    a `<data android:scheme="content" android:mimeType="application/ofd"/>`
    requires both, while two separate `<data/>` elements require either. Reading
    the rendering this way is the difference between a string search that finds
    what the file wrote and a match that finds what a phone would do.
    """
    filters = []
    for block in re.findall(r"<intent-filter\b.*?</intent-filter>", manifest, re.S):
        actions = re.findall(r'<action android:name="([^"]+)"', block)
        categories = re.findall(r'<category android:name="([^"]+)"', block)
        data = []
        for element in re.findall(r"<data\b[^>]*/?>", block):
            attribute = {}
            for name, value in re.findall(r'android:(\w+)="([^"]*)"', element):
                attribute[name] = value
            data.append(attribute)
        filters.append({"actions": actions, "categories": categories, "data": data})
    return filters


def scheme_of(uri: str) -> str:
    """The scheme of a URI, which is what a filter's `scheme` is compared to."""
    return uri.split(":", 1)[0] if ":" in uri else ""


def suffix_of(uri: str) -> str:
    """The path suffix of a URI, dot included, or "" when it has none."""
    path = uri.split("?", 1)[0].split("#", 1)[0]
    name = path.rsplit("/", 1)[-1]
    dot = name.rfind(".")
    return name[dot:] if dot > 0 else ""


def data_matches(attribute: dict, uri: str | None, mime: str | None) -> bool:
    """Whether one `<data>` element accepts this URI and type.

    The rule `IntentFilter` applies, and the one worth restating because it is
    the opposite of how it reads: attributes inside one element are ANDed, and
    a type with no scheme is a type on any scheme. A URI of `None` is a share,
    whose item is in the extra rather than the data field: nothing about a
    scheme or a name can then be asked, and only an element that states neither
    is a candidate.
    """
    scheme = attribute.get("scheme")
    if uri is None:
        if scheme is not None:
            return False
    else:
        if scheme is not None and scheme != scheme_of(uri):
            return False
        if "pathSuffix" in attribute and not suffix_of(uri).endswith(attribute["pathSuffix"]):
            return False
        if "pathPattern" in attribute and not re.search(attribute["pathPattern"], uri):
            return False
    wanted = attribute.get("mimeType")
    if wanted is None:
        return True
    if mime is None:
        return False
    if wanted == "*/*":
        return True
    if wanted.endswith("/*"):
        return mime.split("/", 1)[0] == wanted.split("/", 1)[0]
    return mime == wanted


def offers(filters: list[dict], action: str, uri: str | None, mime: str | None) -> bool:
    """Whether the system would list this app for a given intent.

    The three tests `PackageManager` applies, in its order: the action has to
    match, every category the intent carries has to be in the filter (the
    caller's `DEFAULT` included), and one `<data>` element has to accept the
    URI and type together. A filter whose data all fail is as absent as no
    filter at all, which is exactly what a phone reported.
    """
    for one in filters:
        if action not in one["actions"]:
            continue
        if any(name not in one["categories"] for name in ("android.intent.category.DEFAULT",)):
            continue
        if not any(data_matches(element, uri, mime) for element in one["data"]):
            continue
        return True
    return False


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


def compile_dex(sdk: Path, work: Path, platform: Path) -> Path:
    """Compile the picker fragment into the dex the archive carries.

    The source is compiled at level 8 so the class file is the same whichever
    JDK runs the build, and `d8` turns it into the form a device loads.
    """
    jar_path = platform / "android.jar"
    classes = work / "classes"
    classes.mkdir()
    run([
        java_tool("javac"), "-nowarn", "-source", "8", "-target", "8",
        "-bootclasspath", str(jar_path), "-d", str(classes), str(PICKER_SOURCE),
    ])
    jar = work / "picker.jar"
    run([java_tool("jar"), "cf", str(jar), "-C", str(classes), "."])
    dex = work / "dex"
    dex.mkdir()
    run([
        build_tool(sdk, "d8"), "--min-api", str(MIN_SDK),
        "--lib", str(jar_path), "--output", str(dex), str(jar),
    ])
    return dex / "classes.dex"


def write_archive(work: Path, library: Path, dex: Path, compiled: Path, abi_dir: str) -> Path:
    """Add the library and the dex to the archive the packager built.

    The manifest inside must stay the binary form the packager wrote: the
    signer reads the minimum SDK out of it, and source text has no chunks.
    """
    unsigned = work / "unsigned.apk"
    shutil.copyfile(compiled, unsigned)
    with zipfile.ZipFile(unsigned, "a", zipfile.ZIP_DEFLATED) as archive:
        # Stored rather than deflated: the runtime maps the dex out of the
        # archive instead of unpacking it, and a deflated entry cannot be
        # mapped. `zipalign` then puts it on a four-byte boundary.
        archive.write(dex, "classes.dex", compress_type=zipfile.ZIP_STORED)
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
        # The packager copies this directory into the archive, and it is where
        # Android looks for an extension table of the app's own. It was empty
        # until a phone said the reader never appeared in an open sheet: the
        # system could not name an `.ofd`, so the sender passed a type the
        # manifest did not list.
        (assets / "mime.types").write_text(mime_types_text(), encoding="utf-8")
        compiled = work / "compiled.apk"
        platform = sorted((sdk / "platforms").glob("android-*"))[-1]
        run([
            base, "package", "-f", "-M", str(manifest),
            "-I", str(platform / "android.jar"), "-A", str(assets), "-F", str(compiled),
        ])

        unsigned = write_archive(
            work, library, compile_dex(sdk, work, platform), compiled, library_abi(library)
        )
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
    assert 'android:hasCode="true"' in rendered
    # A `NativeActivity` with no theme gets the platform's default one, which is
    # a dark full-screen window: the strip above the frame is that decoration.
    # The theme names one with no decoration and no title bar, and the frame
    # fills the rest.
    assert 'android:theme="@android:style/Theme.DeviceDefault.NoActionBar"' in rendered, (
        "the activity must not take the default decorated theme"
    )
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
    # An open has to arrive the same way, and this is what a phone reported
    # missing: the two lists were different, so a file manager that had already
    # failed to name the file found a filter that would not take what it sent.
    # Asserted on the list rather than on the rendering, because a catch-all
    # filter further down would satisfy a string search and hide the omission.
    assert "application/octet-stream" in VIEW_MIMES, "an open must take what a nameless sender sends"
    view = rendered.split("android.intent.action.VIEW", 1)[1].split("</intent-filter>", 1)[0]
    for mime in VIEW_MIMES:
        assert f'android:mimeType="{mime}"' in view, mime
    # A sender that states nothing matches no named filter, so only a catch-all
    # reaches it. It is a filter that takes more than the reader can open, which
    # is why the reader refuses the document after reading it.
    assert 'android:mimeType="*/*"' in rendered, "no filter answers an untyped sender"
    # A share has to answer an untyped sender too. A phone reported the reader
    # present in the open sheet and absent from the share sheet, and a share
    # states one item rather than a URI: a sender that could not name the type
    # sent nothing the typed filter took, and there was no second filter.
    shares = rendered.split("android.intent.action.SEND")[1:]
    assert any('android:mimeType="*/*"' in share.split("</intent-filter>", 1)[0] for share in shares), (
        "no share filter answers an untyped sender"
    )
    # Every filter carries the same label on purpose: it is the name the two
    # sheets show, and a reader who sees one label in both sheets has found the
    # same reader twice rather than two different apps. The launcher's own
    # filter is the exception: a launcher shows the app's label.
    assert rendered.count('android:label="Open an OFD"') == rendered.count("<intent-filter") - 1, (
        "every open and share filter is offered under one name"
    )

    # The extension table is what lets a file manager name an `.ofd` at all.
    # Its absence is not visible in the manifest, which is why it went unnoticed.
    table = mime_types_text()
    assert "application/ofd\tofd" in table, table

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
        dex = work / "classes.dex"
        dex.write_bytes(b"dex\n035\0")
        compiled = work / "compiled.apk"
        with zipfile.ZipFile(compiled, "w", zipfile.ZIP_DEFLATED) as archive:
            archive.write(write_manifest(work, version), "AndroidManifest.xml")
        unsigned = write_archive(work, library, dex, compiled, library_abi(library))
        with zipfile.ZipFile(unsigned) as archive:
            names = set(archive.namelist())
    expected = {
        "AndroidManifest.xml",
        "classes.dex",
        f"lib/arm64-v8a/lib{LIBRARY}.so",
    }
    assert names == expected, names

    # What the system does with those filters, rather than what they say. A
    # string search finds a `<data>` element whether or not the element it sits
    # in can ever match, and the phone that reported this issue saw two sheets
    # disagree while every attribute was present.
    filters = parse_filters(rendered)
    view, send = "android.intent.action.VIEW", "android.intent.action.SEND"
    # The routes a file manager takes to a `.ofd`, by how well it knows the type.
    # A provider that resolves the suffix sends `application/ofd`; one that
    # cannot sends `octet-stream`; one that states nothing at all sends neither.
    routes = [
        ("content://com.android.providers.downloads.documents/document/42.ofd", "application/ofd"),
        ("content://com.android.externalstorage.documents/document/primary%3ADoc.ofd", "application/ofd"),
        ("content://com.android.externalstorage.documents/document/primary%3ADoc.ofd", "application/octet-stream"),
        ("content://com.android.externalstorage.documents/document/primary%3ADoc.ofd", None),
        ("file:///sdcard/Download/invoice.ofd", None),
        ("file:///sdcard/Download/invoice.ofd", "application/octet-stream"),
        # A name with more than one dot: the suffix rule has to look at the last
        # one, which a `.*\.ofd` pattern would get right by accident and a
        # first-dot rule would get wrong.
        ("content://com.android.externalstorage.documents/document/primary%3A2024.01.ofd", None),
    ]
    for uri, mime in routes:
        assert offers(filters, view, uri, mime), f"a file manager sending {uri} ({mime}) finds no reader"
    # A share names its item in the extra rather than in the data field, and the
    # sheet asks about the type alone: no URI is in the query, so the scheme of
    # a filter is not consulted. The unknown type is the interesting one -- a
    # provider that cannot name a `.ofd` shares it as `*/*`, and a filter that
    # lists only named types is not offered for it.
    for mime in ("application/ofd", "application/octet-stream", "application/zip", "*/*"):
        assert offers(filters, send, None, mime), f"a share of {mime} finds no reader"

    # A filter that accepts everything makes the assertions above worthless on
    # their own, so what the reader must *not* take is checked as well -- against
    # the named filters, because the catch-all is known to take more than the
    # reader can open and that trade was made deliberately. What matters is that
    # nothing but the catch-all reaches a document the reader cannot open, and
    # that a sender with no type at all reaches nothing: `.txt` beside the route
    # above that must work, on the same provider, is the pair that proves the
    # suffix rule is reading the name rather than accepting the scheme.
    named = [one for one in filters if not any(d.get("mimeType") == "*/*" for d in one["data"])]
    for uri, mime in [
        ("content://com.android.externalstorage.documents/document/primary%3Anotes.txt", "text/plain"),
        ("content://com.android.externalstorage.documents/document/primary%3Anotes.txt", None),
        ("file:///sdcard/Download/photo.jpg", "image/jpeg"),
        ("file:///sdcard/Download/photo.jpg", None),
    ]:
        assert not offers(named, view, uri, mime), f"the reader claims {uri} ({mime})"
    # The catch-all answers a sender that states a type but no usable name, and
    # nothing else: an intent with neither is matched by no filter at all, so the
    # reader is absent from the sheet rather than present and wrong.
    assert not offers(filters, view, "content://com.android.externalstorage.documents/x", None)
    # A share has no URI, so the only thing a filter can be judged on is its
    # type. A `.txt` shared from the same app is the catch-all's known cost.
    for mime in ("text/plain", "image/jpeg"):
        assert not offers(named, send, None, mime), f"a share of {mime} reaches a named filter"

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
