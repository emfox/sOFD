#!/usr/bin/env python3
"""
Extract the release notes for one tag from the changelog.

GitHub's own generated notes describe the commits since the previous release,
which is the wrong shape here twice over. A conventional-commit summary says
what the work was, not what the release offers, and the first release has no
previous release to compare against, so the generated body came out empty on
`v0.1.0`.

This reads the section `## [<version>] - <date>` from `CHANGELOG.md` and prints
it, so the published notes and the changelog are the same text from the same
commit. The link definitions at the foot of the file are dropped and the
changelog's relative links are made absolute, because a release page has no
repository beside it for `docs/Roadmap.md` to resolve against.

Usage:
    python3 scripts/release_notes.py v0.1.1 > notes.md

Exit code: 0 when the section exists and is not empty, 1 otherwise.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
CHANGELOG = REPO / "CHANGELOG.md"
BLOB = "https://github.com/emfox/sOFD/blob/main/"
REPO_URL = "https://github.com/emfox/sOFD"


def version_from_tag(tag: str) -> str:
    """`v0.1.1` names the changelog section `0.1.1`.

    A tag whose numbers no longer agree — `v0.6.1` for the second release, since
    the crates under it carry their own versions — names the same section: the
    release is what the changelog records, and the first three numbers are how
    it is spelled. A one-time name is what makes the old tags stay readable.
    """
    named = tag[1:] if tag.startswith("v") else tag
    # Four numbers is `v<release>.<major>.<minor>.<patch>`; three is a version
    # the tag carries itself, which is how every tag up to `v0.5.1` is spelled.
    parts = named.split(".")
    if len(parts) == 4 and all(part.isdigit() for part in parts):
        return ".".join(parts[1:])
    return named

def section(text: str, version: str) -> str:
    """The body of one version's section, heading excluded."""
    heading = re.compile(rf"^## \[{re.escape(version)}\][^\n]*$", re.MULTILINE)
    found = heading.search(text)
    if found is None:
        raise KeyError(version)
    body = text[found.end():]
    following = re.search(r"^## ", body, re.MULTILINE)
    if following is not None:
        body = body[: following.start()]
    return body.strip("\n")


def absolutise(body: str) -> str:
    """Drop link definitions and point relative links at the repository."""
    kept = [line for line in body.splitlines() if not re.match(r"^\[[^\]]+\]: ", line)]
    joined = "\n".join(kept).strip("\n")

    def replace(match: re.Match[str]) -> str:
        target = match.group(1)
        if re.match(r"^[a-z]+:", target) or target.startswith("#"):
            return match.group(0)
        return f"]({BLOB}{target.lstrip('./')})"

    return re.sub(r"\]\(([^)]+)\)", replace, joined)


def notes(tag: str) -> str:
    body = section(CHANGELOG.read_text(encoding="utf-8"), version_from_tag(tag))
    if not body.strip():
        raise ValueError(f"{tag} has no changelog section")
    header = f"Published from [CHANGELOG.md]({REPO_URL}/blob/main/CHANGELOG.md).\n"
    return header + "\n" + absolutise(body) + "\n"


def selftest() -> int:
    sample = """# Changelog

## [Unreleased]

## [9.9.9] - 2026-01-01

A release. See [`docs/Roadmap.md`](docs/Roadmap.md).

[Unreleased]: https://example.com/compare
[9.9.9]: https://example.com/tag
"""
    body = absolutise(section(sample, "9.9.9"))
    failures = []
    if "A release." not in body:
        failures.append("the section body was not found")
    if f"{BLOB}docs/Roadmap.md" not in body:
        failures.append("a relative link was left relative")
    if "example.com/compare" in body:
        failures.append("a link definition survived")
    # A release tag carries one more number than a version does, and both have
    # to name the changelog section the same way. Reading too eagerly here would
    # publish the notes for the wrong release.
    named = {"v0.5.1": "0.5.1", "v1.2.3": "1.2.3", "v0.6.1": "0.6.1"}
    for tag, version in named.items():
        got = version_from_tag(tag)
        if got != version:
            failures.append(f"{tag} named the section {got}, not {version}")
    for failure in failures:
        print(f"FAIL: {failure}", file=sys.stderr)
    if failures:
        return 1
    print("OK: notes are extracted, links are absolute")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description="Print one version's release notes.")
    ap.add_argument("tag", nargs="?", help="the tag, for example v0.1.1")
    ap.add_argument("--selftest", action="store_true", help="check the extractor alone")
    args = ap.parse_args()

    if args.selftest:
        return selftest()
    if not args.tag:
        ap.error("a tag is required")
    try:
        print(notes(args.tag))
    except (KeyError, ValueError) as error:
        print(f"no release notes for {args.tag}: {error}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
