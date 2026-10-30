#!/usr/bin/env python3
"""
sOFD message catalogue consistency check.

Checks performed:
1. Every locale directory has the same set of .ftl files;
2. Every file has the same set of message keys, compared against en-US;
3. Variable placeholders ({$name}) match, so a locale cannot silently drop an
   argument and fail at runtime;
4. The Rust all_keys() list matches the keys actually present in the catalogues;
5. No natural language sentence is used as a key; keys are ASCII kebab-case.

Usage:
    python3 scripts/check_i18n.py          # check and print a report
    python3 scripts/check_i18n.py --strict # non-zero exit on warnings too

Exit code: 0 on success, 1 on failure.
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
# The catalogues live inside the crate that embeds them, because a published
# crate is built from the tarball cargo makes and that tarball holds the crate's
# own directory. `check_packaging.py` holds every published crate to this.
LOCALES_DIR = ROOT / "crates" / "sofd-i18n" / "locales"
I18N_SRC = ROOT / "crates" / "sofd-i18n" / "src"

# The reader owns its own catalogue, kept beside the crate that shows the
# strings rather than inside the engine. It is checked the same way, against
# the key list in its own message module.
READER_LOCALES_DIR = ROOT / "crates" / "sofd-session" / "locales"
READER_MESSAGE_RS = ROOT / "crates" / "sofd-session" / "src" / "message.rs"

# Baseline locale; every other locale is compared against it
BASE_LOCALE = "en-US"

# Key shape: starts with a lowercase ASCII letter, then lowercase, digits, dash
KEY_RE = re.compile(r"^([a-z][a-z0-9-]*)\s*=")
# Variable placeholders, either {$name} or { $name }
VAR_RE = re.compile(r"\{\s*\$([a-zA-Z_][a-zA-Z0-9_]*)\s*[}\s]")
# String literals inside the Rust all_keys() list
RUST_KEY_RE = re.compile(r'"([a-z][a-z0-9-]*)"')


class Report:
    def __init__(self) -> None:
        self.errors: list[str] = []
        self.warnings: list[str] = []

    def error(self, msg: str) -> None:
        self.errors.append(msg)

    def warn(self, msg: str) -> None:
        self.warnings.append(msg)

    @property
    def ok(self) -> bool:
        return not self.errors


def parse_ftl(path: Path) -> tuple[dict[str, set[str]], list[str]]:
    """Parse a .ftl file into {key: variables} plus a list of malformed keys.

    Fluent continuation lines and select expressions need care, so a key is
    recognized only at column zero and matching KEY_RE.
    """
    keys: dict[str, set[str]] = {}
    bad_keys: list[str] = []
    current: str | None = None

    for raw in path.read_text(encoding="utf-8").splitlines():
        line = raw.rstrip()
        if not line or line.lstrip().startswith("#"):
            continue

        m = KEY_RE.match(line)
        if m:
            key = m.group(1)
            current = key
            keys.setdefault(key, set())
            keys[key] |= set(VAR_RE.findall(line))
            continue

        # Continuation line or select branch; attribute it to the previous key.
        # A multiline select can close at column zero (`} to { $format }`), so
        # treat a leading `}` the same as indentation — otherwise the trailing
        # placeholders on that line are lost.
        if current and (
            line.startswith((" ", "\t", "}"))
            or line.lstrip().startswith(("*[", "["))
        ):
            stripped = line.lstrip()
            # A select variant key ([one] / *[other]) is syntax, not a placeholder,
            # yet the leading `[$name]`-shaped `[one]` reads as one. An embedded
            # `[0]` even reads as a `{ $0 }`-style variable. Drop the variant-key
            # prefix before scanning for real placeholders.
            if stripped.startswith("*["):
                stripped = stripped[2:]
            elif stripped.startswith("["):
                stripped = stripped[1:]
            if "]" in stripped:
                stripped = stripped.split("]", 1)[1]
            keys[current] |= set(VAR_RE.findall(stripped))
            continue

        # A line like `some prose = value` uses natural language as a key, which is invalid
        if "=" in line and not line.startswith((" ", "\t")):
            head = line.split("=", 1)[0].strip()
            if head and not head.startswith("#"):
                bad_keys.append(head)

    return keys, bad_keys


def check_catalogue(locales_dir: Path, rep: Report) -> int:
    """Compare every locale against the baseline. Returns the key count.

    One catalogue's worth of work: file set, key set, variables. The caller
    runs it once per catalogue, so the engine's and the reader's are held to
    the same rules.
    """
    if not locales_dir.is_dir():
        rep.error(f"locales directory not found: {locales_dir}")
        return 0

    locales = sorted(d.name for d in locales_dir.iterdir() if d.is_dir())
    if BASE_LOCALE not in locales:
        rep.error(f"baseline locale directory missing under {locales_dir}: {BASE_LOCALE}")
        return 0

    base_files = sorted(p.name for p in (locales_dir / BASE_LOCALE).glob("*.ftl"))
    for loc in locales:
        files = sorted(p.name for p in (locales_dir / loc).glob("*.ftl"))
        if files != base_files:
            rep.error(
                f"[file set] {locales_dir.name}/{loc} differs from {BASE_LOCALE}\n"
                f"          only in {loc}: {sorted(set(files) - set(base_files))}\n"
                f"          only in {BASE_LOCALE}: {sorted(set(base_files) - set(files))}"
            )

    base_parsed: dict[str, dict[str, set[str]]] = {}
    for fname in base_files:
        keys, bad = parse_ftl(locales_dir / BASE_LOCALE / fname)
        base_parsed[fname] = keys
        for b in bad:
            rep.error(
                f"[key format] {locales_dir.name}/{BASE_LOCALE}/{fname}: "
                f"invalid key {b!r}, must be ASCII kebab-case"
            )

    total_keys = 0
    for fname in base_files:
        base_keys = base_parsed[fname]
        total_keys += len(base_keys)

        for loc in locales:
            if loc == BASE_LOCALE:
                continue
            p = locales_dir / loc / fname
            if not p.exists():
                continue

            keys, bad = parse_ftl(p)
            for b in bad:
                rep.error(
                    f"[key format] {locales_dir.name}/{loc}/{fname}: "
                    f"invalid key {b!r}, must be ASCII kebab-case"
                )

            missing = sorted(set(base_keys) - set(keys))
            extra = sorted(set(keys) - set(base_keys))
            if missing:
                rep.error(f"[missing keys] {locales_dir.name}/{loc}/{fname}: {missing}")
            if extra:
                rep.warn(f"[extra keys] {locales_dir.name}/{loc}/{fname}: {extra}")

            for k in sorted(set(base_keys) & set(keys)):
                bv, lv = base_keys[k], keys[k]
                if bv != lv:
                    rep.error(
                        f"[variable mismatch] {locales_dir.name}/{loc}/{fname} :: {k}\n"
                        f"            {BASE_LOCALE}={sorted(bv)}  {loc}={sorted(lv)}"
                    )

    return total_keys


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--strict", action="store_true", help="treat warnings as failures")
    ap.add_argument("--quiet", action="store_true")
    args = ap.parse_args()

    rep = Report()

    # The engine catalogue: file set, keys, variables, and the two Rust lists
    # that must agree with it.
    engine_keys = check_catalogue(LOCALES_DIR, rep)

    bundle_rs = I18N_SRC / "bundle.rs"
    if bundle_rs.exists():
        src = bundle_rs.read_text(encoding="utf-8")
        m = re.search(r"pub fn all_keys\(\)[^{]*\{(.*?)\n\}", src, re.S)
        if m:
            rust_keys = set(RUST_KEY_RE.findall(m.group(1)))
            ftl_keys = {
                k
                for f in (LOCALES_DIR / BASE_LOCALE).glob("*.ftl")
                for k in parse_ftl(f)[0]
            }
            only_rust = sorted(rust_keys - ftl_keys)
            only_ftl = sorted(ftl_keys - rust_keys)
            if only_rust:
                rep.error(f"[code vs catalogue] in all_keys() but missing from FTL: {only_rust}")
            if only_ftl:
                rep.error(f"[code vs catalogue] in FTL but missing from all_keys(): {only_ftl}")
        else:
            rep.warn("could not parse all_keys() from bundle.rs, skipping code vs catalogue check")

    msg_rs = I18N_SRC / "messages.rs"
    if msg_rs.exists():
        src = msg_rs.read_text(encoding="utf-8")
        m = re.search(r"pub enum MessageId \{(.*?)\n\}", src, re.S)
        if m:
            variants = len(re.findall(r"^\s{4}[A-Z][A-Za-z0-9]*,", m.group(1), re.M))
            if variants != engine_keys:
                rep.error(
                    f"[variant count] MessageId has {variants} variants "
                    f"but the FTL catalogues have {engine_keys} keys"
                )

    # The reader's own catalogue, held to the same file/key/variable rules and
    # cross-checked against its `ReaderMessage::key()` map, which is the only
    # place the reader spells out a key.
    reader_keys = check_catalogue(READER_LOCALES_DIR, rep)
    if READER_MESSAGE_RS.exists():
        src = READER_MESSAGE_RS.read_text(encoding="utf-8")
        m = re.search(r"pub fn key\(self\)[^{]*\{(.*?)\n    \}", src, re.S)
        if m:
            rust_keys = set(RUST_KEY_RE.findall(m.group(1)))
            ftl_keys = {
                k
                for f in (READER_LOCALES_DIR / BASE_LOCALE).glob("*.ftl")
                for k in parse_ftl(f)[0]
            }
            only_rust = sorted(rust_keys - ftl_keys)
            only_ftl = sorted(ftl_keys - rust_keys)
            if only_rust:
                rep.error(f"[reader code vs catalogue] in key() but missing from FTL: {only_rust}")
            if only_ftl:
                rep.error(f"[reader code vs catalogue] in FTL but missing from key(): {only_ftl}")
        else:
            rep.warn("could not parse ReaderMessage::key() from message.rs")

    if not args.quiet:
        print(f"engine locales: {', '.join(sorted(d.name for d in LOCALES_DIR.iterdir() if d.is_dir()))}"
              f"   keys: {engine_keys}")
        print(f"reader locales: {', '.join(sorted(d.name for d in READER_LOCALES_DIR.iterdir() if d.is_dir()))}"
              f"   keys: {reader_keys}")
        print("-" * 60)

    for w in rep.warnings:
        print(f"WARN  {w}")
    for e in rep.errors:
        print(f"FAIL  {e}")

    if rep.ok and not (args.strict and rep.warnings):
        if not args.quiet:
            print("OK: message catalogues are consistent")
        return 0

    print(f"\nFAILED: {len(rep.errors)} error(s), {len(rep.warnings)} warning(s)")
    return 1


if __name__ == "__main__":
    sys.exit(main())
