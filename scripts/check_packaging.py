#!/usr/bin/env python3
"""Fail when a published crate reads a file `cargo package` would not ship.

A crate that reaches a registry is built from the tarball `cargo package`
makes, not from the working tree, and the two do not hold the same files: cargo
takes the crate's own directory, so a path that climbs above it is left out
unless an `include` names it. `include_str!` and `include_bytes!` are resolved
by the compiler at build time, so a file left out is not a missing optional: the
build fails, and it fails after the tag is pushed and the tarball is uploaded.

The check is about the tarball rather than about cargo's rules, which are
subtle enough that reading the manifest has already been got wrong once. It asks
cargo for the file list, then asks, for each macro, whether the file it names is
in that list.
"""

from __future__ import annotations

import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

MACRO = re.compile(r'include_(?:str|bytes)!\s*\(\s*"([^"]+)"\s*\)')

# Written into every tarball by cargo itself. Their absence means the command
# failed rather than that the crate ships nothing, and the two must not be
# confused: a failed listing would otherwise pass every crate.
REQUIRED = (".cargo_vcs_info.json", "Cargo.toml")


def published() -> list[str]:
    """The crates `publish_order.py` says reach a registry."""
    done = subprocess.run(
        [sys.executable, str(ROOT / "scripts" / "publish_order.py")],
        capture_output=True, text=True, check=True,
    )
    return re.findall(r"^\s+\d+\.\s+([a-z0-9-]+)$", done.stdout, re.M)


def shipped(crate: str) -> set[str] | None:
    """Every path in the crate's tarball, relative to the crate root.

    `--allow-dirty` is what makes this a check about packaging rather than about
    git: a working tree with an uncommitted change is the normal state while
    somebody is fixing the very thing this looks for.
    """
    done = subprocess.run(
        ["cargo", "package", "--list", "-p", crate, "--allow-dirty"],
        cwd=ROOT, capture_output=True, text=True,
    )
    if done.returncode != 0:
        return None
    paths = {line.strip() for line in done.stdout.splitlines() if line.strip()}
    return paths if all(one in paths for one in REQUIRED) else None


def includes(directory: Path) -> list[tuple[Path, str]]:
    """Each include macro below the crate, with the path it names."""
    found: list[tuple[Path, str]] = []
    for source in sorted(directory.rglob("*.rs")):
        if "target" in source.parts:
            continue
        text = source.read_text(encoding="utf-8", errors="ignore")
        found.extend((source, name) for name in MACRO.findall(text))
    return found


def missing(source: Path, name: str, crate: Path, paths: set[str]) -> bool:
    """Whether the file the macro names is absent from the tarball."""
    target = (source.parent / name).resolve()
    directory = crate.resolve()
    if target != directory and directory not in target.parents:
        # Above the crate directory: only an `include` entry carries it, and
        # nothing here reads the manifest to guess which entries exist.
        return True
    return str(target.relative_to(directory)) not in paths


def main() -> int:
    problems: list[str] = []
    checked = 0
    for crate in published():
        paths = shipped(crate)
        if paths is None:
            print(f"SKIP {crate}: cargo package --list produced no tarball")
            continue
        checked += 1
        directory = ROOT / "crates" / crate
        for source, name in includes(directory):
            if missing(source, name, directory, paths):
                where = source.relative_to(ROOT)
                problems.append(f"{where} includes {name}, which the tarball lacks")

    print(f"[package] {checked} published crate(s) checked for files their tarball lacks")
    if problems:
        print(f"\nFAILED: {len(problems)} file(s) a published crate reads and would not ship\n")
        for problem in problems:
            print(f"  {problem}")
        return 1
    print("\nOK: every file a published crate includes is in the tarball cargo makes")
    return 0


if __name__ == "__main__":
    sys.exit(main())
