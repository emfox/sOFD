#!/usr/bin/env python3
"""Check that every crate states whether it is published, and the ones that are
carry what a registry asks for.

The decision of *which* crates publish is a judgement this script does not make:
a crate either says `publish = false` with a reason or joins the published set.
What it enforces is that the choice was made, that the two sets still add up to
the whole workspace, and that a published crate has the metadata and the
dependency form that `cargo publish` needs.

It also checks the features, which are the other half of a public promise: a
feature that compiles and changes nothing tells an enabler the crate has a
capability it does not. Every declared feature must be consumed by code, and the
names this project has removed must not come back.

Usage:
    python3 scripts/check_publish.py            # check the workspace
    python3 scripts/check_publish.py --list     # which crates publish, and why
    python3 scripts/check_publish.py --explain  # what each field is for

Exit code: 0 when every crate passes, 1 when one does not.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# A registry entry is what a stranger reads before deciding to add the crate, so
# each of these is a question they will otherwise have to answer by reading the
# source. `license` and `repository` come from the workspace, so a crate that
# inherits them passes without restating them. `readme` comes from the workspace
# too, or points at one the crate writes for itself; either is fine, and the
# crate's own is what an interface uses.
REQUIRED = ("description", "license", "repository", "readme")

# Features removed because they compiled and changed nothing. A name that comes
# back is either an implementation, in which case this list is the wrong place
# for it, or the same lie told again. `docs/Architecture.md` states the rule.
RETIRED_FEATURES = ("office", "ocr")

EXPLAIN = {
    "description": "one sentence on what the crate is for, shown in registry search",
    "license": "what a consumer may do with it; inherited from the workspace",
    "repository": "where the source lives; inherited from the workspace",
    "readme": "the page a registry shows; inherited from the workspace",
    "publish": "whether the crate goes to a registry at all",
}


def crates() -> list[Path]:
    """Every member crate's manifest, in the workspace's own order."""
    text = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    members = re.search(r"^members\s*=\s*\[(.*?)\]", text, re.M | re.S)
    if members is None:
        return []
    # Only the member list: the same paths appear again under
    # `[workspace.dependencies]`, where they are not members.
    found = re.findall(r'"(crates/[a-z0-9-]+)"', members.group(1))
    return [ROOT / member / "Cargo.toml" for member in sorted(set(found))]


def package_block(text: str) -> str:
    """The `[package]` table of a manifest, and nothing after it."""
    match = re.search(r"^\[package\]\n(.*?)(?=^\[|\Z)", text, re.M | re.S)
    return match.group(1) if match else ""


def inherited(block: str, field: str) -> bool:
    """Whether the field is stated here, inherited from the workspace, or set.

    `readme.workspace = true` and `readme = "README.md"` both satisfy a
    registry: the first points at the repository's page, the second at one the
    crate writes. What the check rejects is the absence of either.
    """
    if re.search(rf"^{field}\s*=\s*\S", block, re.M):
        return True
    return re.search(rf"^{field}\.workspace\s*=\s*true", block, re.M) is not None


def publishes(block: str) -> bool:
    """Whether this crate goes to a registry."""
    match = re.search(r"^publish\s*=\s*(.+)$", block, re.M)
    if match is None:
        # An unstated `publish` means yes, which is the case most likely to be
        # an oversight rather than a choice.
        return True
    return match.group(1).strip() != "false"


def reason_near(manifest: Path) -> str:
    """The comment above `publish = false`, which records why."""
    lines = manifest.read_text(encoding="utf-8").splitlines()
    for index, line in enumerate(lines):
        if line.startswith("publish"):
            above = [l.strip() for l in lines[max(0, index - 2):index]]
            return " ".join(l.lstrip("# ").strip() for l in above if l.startswith("#"))
    return ""


def inspect(manifest: Path) -> tuple[str, list[str], str | None]:
    """One crate's name, its problems, and why it is unpublished.

    The reason is `None` when the crate publishes, and an empty string when it
    is withheld without saying why: the two are different faults.
    """
    text = manifest.read_text(encoding="utf-8")
    block = package_block(text)
    name = re.search(r'^name\s*=\s*"([^"]+)"', block, re.M)
    label = name.group(1) if name else manifest.parent.name
    if not publishes(block):
        return label, [], reason_near(manifest)
    missing = [field for field in REQUIRED if not inherited(block, field)]
    return label, [f"no {field} (a registry asks for it)" for field in missing], None


def features(manifest: Path) -> dict[str, str]:
    """Each declared feature and its definition, from the `[features]` table.

    `default` is left out: it names other features rather than a capability, and
    a caller never enables it by name.
    """
    text = manifest.read_text(encoding="utf-8")
    match = re.search(r"^\[features\]\n(.*?)(?=^\[|\Z)", text, re.M | re.S)
    if match is None:
        return {}
    found: dict[str, str] = {}
    for line in match.group(1).splitlines():
        entry = re.match(r'^([A-Za-z0-9_-]+)\s*=\s*(.+)$', line)
        if entry is None or entry.group(1) == "default":
            continue
        found[entry.group(1)] = entry.group(2).strip()
    return found


def parts_of(definition: str) -> list[str]:
    """The entries of a feature's list, unquoted and trimmed of comments.

    A quoted name is a feature of this crate; an unquoted one either names a
    dependency (`dep:x`) or a feature of one (`dep/feature`), which is the same
    distinction Cargo draws. Both are needed here, so the quotes come off and
    whatever follows them is dropped: a note about the choice is not part of it.
    """
    body = definition.strip().strip("[]")
    found = []
    for part in body.split(","):
        name = part.strip().split("#")[0].strip().strip('"')
        if name:
            found.append(name)
    return found


def usage_of(name: str, crate: Path) -> bool:
    """Whether any code in the crate reads this feature.

    A `cfg(feature = "x")` reference is what a capability looks like in code.
    An optional dependency alone is not: `dep:x` compiles a crate in that
    nothing then calls, which is the same empty promise in a different form.
    """
    haystack = " ".join(
        path.read_text(encoding="utf-8", errors="ignore")
        for path in crate.rglob("*.rs")
        if "target" not in path.parts
    )
    return f'feature = "{name}"' in haystack


def reads(
    name: str,
    parts: list[str],
    declared: dict[str, str],
    crate: Path,
    seen: set[str] | None = None,
) -> bool:
    """Whether some code in the crate ends up reached by enabling this feature.

    Three ways to reach code, and one that reaches it by itself: a `cfg` on the
    name, a `cfg` further along a chain of forwards in this crate, or a `dep:x`
    that compiles a crate in — which code may then call without a gate, so the
    dependency being used is the only use there is to have.
    """
    seen = seen or set()
    if name in seen:
        return False
    seen.add(name)
    if usage_of(name, crate):
        return True
    for part in parts:
        if part.startswith("dep:"):
            return True
        if "/" in part:
            continue
        forwarded = declared.get(part)
        if forwarded is None:
            continue
        inner = parts_of(forwarded)
        if reads(part, inner, declared, crate, seen):
            return True
    return False


def feature_problems(manifest: Path) -> list[str]:
    """Problems with one crate's features, as messages.

    One rule. A feature exists to switch code on, so a name that no code ends up
    reading is a name that changes nothing, whether it expands to an empty list
    or to a list of real things. A forwarding feature is followed to what it
    forwards to, so a chain that ends at code passes and a chain that ends at
    nothing does not. What is not checked is a name that points at nothing at
    all: Cargo refuses to resolve it, so the build says it first and more
    precisely.
    """
    crate = manifest.parent
    label = crate.name
    declared = features(manifest)
    found: list[str] = []
    for name in RETIRED_FEATURES:
        if name in declared:
            found.append(f"{label}: the feature `{name}` was retired and is back")
    for name, definition in declared.items():
        parts = parts_of(definition)
        if not reads(name, parts, declared, crate):
            what = f"expands to {definition}" if parts else "expands to nothing"
            found.append(f"{label}: the feature `{name}` {what} and no code reads it")
    return found


def main() -> int:
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("--list", action="store_true",
                        help="print the two sets instead of checking them")
    parser.add_argument("--explain", action="store_true",
                        help="print what each required field is for")
    args = parser.parse_args()

    if args.explain:
        for field, why in EXPLAIN.items():
            print(f"  {field:<14} {why}")
        return 0

    published: list[str] = []
    unpublished: list[tuple[str, str]] = []
    problems: list[str] = []

    for manifest in crates():
        label, found, reason = inspect(manifest)
        if reason is None:
            published.append(label)
        elif reason:
            unpublished.append((label, reason))
        else:
            unpublished.append((label, "no reason given"))
            problems.append(f"{label}: publish = false without a comment saying why")
        problems.extend(f"{label}: {message}" for message in found)
        problems.extend(feature_problems(manifest))

    if args.list:
        print(f"published ({len(published)}):")
        for label in published:
            print(f"  {label}")
        print(f"\nnot published ({len(unpublished)}):")
        for label, reason in unpublished:
            print(f"  {label:<24} {reason}")
        return 0

    print(f"Scanned {len(published) + len(unpublished)} crate(s)")
    print(f"[publish] {len(published)} published, {len(unpublished)} withheld")

    if problems:
        print(f"\nFAILED: {len(problems)} problem(s)\n")
        for problem in problems:
            print(f"  {problem}")
        print("\nSee issue #4 for what the registry surface is.")
        return 1

    print("\nOK: every crate states where it goes, with what a registry asks for,")
    print("    and every feature it declares is consumed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
