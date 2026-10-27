#!/usr/bin/env python3
"""Say which git ref cargo-semver-checks should compare a crate against.

The gate's question is "does this break an API somebody can already depend
on", and the answer is an *earlier published release* of that crate.

The ref that suggests itself is the tag naming the crate's own version, and it
is the wrong one. `v0.5.2` carries `sofd-archive 0.5.2`; comparing the tree
against it compares the crate to itself, and cargo-semver-checks reports
`no change; assume minor` — a pass that means nothing. The comparison that
carries information is against the release before it, `v0.5.1`: that is where
a caller is, and a break there is a break they would hit.

Per-crate versions make the naming harder on top of that. A release can ship a
subset — the crates whose code changed, and the crates above them whose
requirement moved because of it — and then the tag names the release, not any
one crate's version. Whether a given crate was in that release, and at what
version, is answered by reading its manifest out of that tag's tree. A tag is a
claim about the release; the tree says who was in it.

So the baseline is the newest tag *strictly below* the version under test that
carried this crate at the version it is named for. A crate no such tag carries
has no baseline: nothing can depend on it yet, so there is nothing to break,
and it is reported as `none` rather than as a failure.

`--groups` is what makes the gate affordable: it gathers the crates by the ref
they are held to, so a release that moves seventeen crates to one baseline
costs one build of that baseline rather than seventeen.

Usage:
    python3 scripts/semver_baseline.py sofd-archive 0.5.2   # -> v0.5.1
    python3 scripts/semver_baseline.py --list served.txt
    python3 scripts/semver_baseline.py --groups served.txt
    python3 scripts/semver_baseline.py --self-test

Exit code: 0 when every crate got an answer, 1 when git failed.
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def version_key(version: str) -> tuple[int, int, int] | None:
    """A version as comparable numbers, or None when it is not `x.y.z`."""
    match = re.fullmatch(r"(\d+)\.(\d+)\.(\d+)", version)
    if match is None:
        return None
    return int(match.group(1)), int(match.group(2)), int(match.group(3))


def release_tags(root: Path = ROOT) -> list[tuple[str, str]]:
    """Every release tag as (tag, version), newest version first.

    A prerelease names no release to compare against — nothing is held to it —
    so `v0.1.0-rc.1` is not in this list.
    """
    completed = subprocess.run(
        ["git", "tag", "--list", "v*"],
        cwd=root,
        capture_output=True,
        text=True,
    )
    if completed.returncode != 0:
        raise SystemExit(f"FATAL: cannot list tags: {completed.stderr.strip()}")
    found: list[tuple[str, str]] = []
    for tag in completed.stdout.split():
        match = re.fullmatch(r"v(\d+\.\d+\.\d+)", tag)
        if match is not None:
            found.append((tag, match.group(1)))
    found.sort(key=lambda pair: version_key(pair[1]) or (0, 0, 0), reverse=True)
    return found


def states(manifest: Path, version: str) -> bool:
    """Whether a manifest, as a tree has it, declares this version.

    Both spellings count: a crate at a full workspace release inherits the
    version with `version.workspace = true`, and a crate shipping on its own
    while the workspace stays behind states it outright. A tag carrying this
    crate is a tag whose tree declares the version being asked about, however
    it was written.
    """
    text = manifest.read_text(encoding="utf-8")
    block = re.search(r"^\[package\]\n(.*?)(?=^\[|\Z)", text, re.M | re.S)
    if block is None:
        return False
    table = block.group(1)
    if re.search(r"^version\.workspace\s*=\s*true", table, re.M):
        if manifest.parent.name == ".":
            return False
        workspace = manifest.parent.parent / "Cargo.toml"
        return stated_version(workspace) == version
    return stated_version(manifest) == version


def stated_version(manifest: Path) -> str | None:
    """The version a manifest writes down, ignoring anything inherited."""
    try:
        text = manifest.read_text(encoding="utf-8")
    except OSError:
        return None
    block = re.search(r"^\[package\]\n(.*?)(?=^\[|\Z)", text, re.M | re.S)
    if block is not None:
        match = re.search(r'^version\s*=\s*"([^"]+)"', block.group(1), re.M)
        if match is not None:
            return match.group(1)
    block = re.search(r"^\[workspace\.package\]\n(.*?)(?=^\[|\Z)", text, re.M | re.S)
    if block is not None:
        match = re.search(r'^version\s*=\s*"([^"]+)"', block.group(1), re.M)
        if match is not None:
            return match.group(1)
    return None


def carries(tag: str, crate: str, root: Path = ROOT) -> str | None:
    """The version a tag's tree declares for `crate`, or None if it lacks it."""
    manifest = root / "crates" / crate / "Cargo.toml"
    completed = subprocess.run(
        ["git", "show", f"{tag}:crates/{crate}/Cargo.toml"],
        cwd=root,
        capture_output=True,
        text=True,
    )
    if completed.returncode != 0:
        return None
    block = re.search(r"^\[package\]\n(.*?)(?=^\[|\Z)", completed.stdout, re.M | re.S)
    if block is None:
        return None
    table = block.group(1)
    if re.search(r"^version\.workspace\s*=\s*true", table, re.M):
        shown = subprocess.run(
            ["git", "show", f"{tag}:Cargo.toml"], cwd=root, capture_output=True, text=True
        )
        if shown.returncode != 0:
            return None
        space = re.search(
            r"^\[workspace\.package\]\n(.*?)(?=^\[|\Z)", shown.stdout, re.M | re.S
        )
        if space is None:
            return None
        match = re.search(r'^version\s*=\s*"([^"]+)"', space.group(1), re.M)
        return None if match is None else match.group(1)
    match = re.search(r'^version\s*=\s*"([^"]+)"', table, re.M)
    return None if match is None else match.group(1)


def baseline(crate: str, version: str, root: Path = ROOT) -> str | None:
    """The ref to compare `crate` at `version` against, or None for nothing.

    The comparison has to be against a *different*, earlier version of the
    crate, because the question is "did this tree break something its callers
    depend on". Comparing a crate with the tag that carries the same version
    answers nothing: cargo-semver-checks sees an identical tree and reports
    `no change; assume minor`, which passes every time. So the newest tag
    strictly below `version` that carries the crate is the baseline, and the
    tag named after the version itself is deliberately not used.

    "Newest below" is what makes a subset release work: 0.5.2 shipped with
    `v0.5.2` as its tag, and the release its callers were on before it is
    `v0.5.1`, which is the one a break would surprise them on.

    A crate no tag carries has no baseline: nothing can depend on it yet.
    """
    wanted = version_key(version)
    if wanted is None:
        return None
    tags = release_tags(root)
    # Sorted here rather than trusted from the caller: the baseline is the
    # newest qualifying tag, and "newest" has to be true of the list itself.
    tags = sorted(
        tags,
        key=lambda pair: version_key(pair[1]) or (0, 0, 0),
        reverse=True,
    )
    for tag, tag_version in tags:
        key = version_key(tag_version)
        # Strictly below: a tag at or above the version under test is not a
        # baseline. Equal is the self-comparison this docstring rules out.
        if key is None or key >= wanted:
            continue
        carried = carries(tag, crate, root)
        if carried is None:
            continue
        carriage = version_key(carried)
        # The tag has to have carried this crate at the version it is named
        # for. A tag is a claim about the release; the manifest in its tree
        # says whether this crate was part of it, and at what version.
        if carriage is None or carriage != key:
            continue
        return tag
    return None


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("crate", nargs="?", help="the crate to ask about")
    parser.add_argument("version", nargs="?", help="the version being checked")
    parser.add_argument(
        "--list",
        metavar="FILE",
        help="a served.txt: lines of `crate version published|unpublished`",
    )
    parser.add_argument(
        "--groups",
        metavar="FILE",
        help="a served.txt, printed as refs holding every crate to compare",
    )
    parser.add_argument("--self-test", action="store_true")
    args = parser.parse_args()

    if args.self_test:
        return self_test()

    if args.list or args.groups:
        source = Path(args.list or args.groups)
        by_ref: dict[str, list[str]] = {}
        for line in source.read_text(encoding="utf-8").splitlines():
            parts = line.split()
            if len(parts) != 3 or parts[2] != "published":
                continue
            crate, version = parts[0], parts[1]
            ref = baseline(crate, version)
            if args.groups:
                if ref is not None:
                    by_ref.setdefault(ref, []).append(f"{crate}@{version}")
            else:
                print(f"{crate} {version} {ref or 'none'}")
        if args.groups:
            if not by_ref:
                print("No published crate to compare.", file=sys.stderr)
                return 0
            for ref in sorted(by_ref, reverse=True):
                print(f"{ref} {' '.join(sorted(by_ref[ref]))}")
        return 0

    if not args.crate or not args.version:
        parser.error("give a crate and a version, or --list, --groups, or --self-test")
    ref = baseline(args.crate, args.version)
    print(ref or "none")
    return 0


def self_test() -> int:
    """The rule, against the shapes a release actually takes."""
    global carries, release_tags

    real_carries, real_tags = carries, release_tags
    try:
        # A crate whose own tag exists is still compared against the release
        # before it: comparing it with its own tag is comparing it with itself.
        release_tags = lambda root=None: [("v0.6.0", "0.6.0"), ("v0.5.1", "0.5.1")]
        carries = lambda tag, crate, root=None: {"v0.6.0": "0.6.0", "v0.5.1": "0.5.1"}.get(tag)
        assert baseline("sofd-error", "0.6.0") == "v0.5.1", "own tag is not a baseline"

        # A subset release: the tag names the release and carries the crate at
        # the version under test, so the release before it is the baseline.
        release_tags = lambda root=None: [
            ("v0.5.2", "0.5.2"),
            ("v0.5.1", "0.5.1"),
        ]
        carries = lambda tag, crate, root=None: {"v0.5.2": "0.5.2", "v0.5.1": "0.5.1"}.get(tag)
        assert baseline("sofd-archive", "0.5.2") == "v0.5.1", "previous release wins"

        # A crate no tag carries: nothing can depend on it yet.
        carries = lambda tag, crate, root=None: None
        assert baseline("sofd-new", "0.1.0") is None, "unreleased has no baseline"

        # A crate whose tag carries a different version than its name says is
        # not a baseline: the name is a claim, the tree is the fact.
        release_tags = lambda root=None: [("v0.7.0", "0.7.0"), ("v0.5.1", "0.5.1")]
        carries = lambda tag, crate, root=None: {"v0.7.0": "0.6.0", "v0.5.1": "0.5.1"}.get(tag)
        assert baseline("sofd-thing", "0.6.5") == "v0.5.1", "mismatched tag is skipped"

        # Versions compare as numbers, not strings: 0.10.0 is above 0.9.0.
        release_tags = lambda root=None: [("v0.9.0", "0.9.0"), ("v0.10.0", "0.10.0")]
        carries = lambda tag, crate, root=None: {"v0.9.0": "0.9.0", "v0.10.0": "0.10.0"}.get(tag)
        assert baseline("sofd-x", "0.11.0") == "v0.10.0", "numeric order, not lexical"
    finally:
        carries, release_tags = real_carries, real_tags
    print("OK: 5 baseline cases")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
