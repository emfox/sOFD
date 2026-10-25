#!/usr/bin/env python3
"""Print the crates a registry needs, in the order `cargo publish` accepts them.

A crate reaches a registry only with the crates it names. Cargo rewrites a
`path` dependency to a version requirement at packaging time, so a published
crate whose dependency is not in the registry cannot be installed by anyone —
the published set is a *closure*, not a list chosen per crate. Publishing
`sofd` alone is not possible: it names `sofd-parser`, which names
`sofd-model`, and so on down to `sofd-error`.

Order follows from the same rule: a crate can only go up once everything it
depends on is already there, so the topological order of the workspace is the
publish order. This script states that order instead of leaving a release to
rediscover it, and it fails when the closure cannot be built at all.

Which crates publish is `publish` in each manifest; `docs/Architecture.md`
section 3 says what each one promises a consumer.

Usage:
    python3 scripts/publish_order.py            # the closure, in order
    python3 scripts/publish_order.py --commands  # as cargo publish lines
    python3 scripts/publish_order.py --closure sofd  # just one entry point

Exit code: 0 when the closure is sound, 1 when a cycle or a missing crate
makes it unbuildable.
"""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

# Sections whose dependencies a published crate must also publish. A dev
# dependency never reaches a consumer, so it imposes no requirement; a build
# dependency does, because the consumer builds the crate from source.
DEP_SECTIONS = (
    "dependencies",
    "build-dependencies",
    "target.'cfg(unix)'.dependencies",
)


def manifests() -> dict[str, Path]:
    """Every workspace crate, by package name."""
    found = {}
    for path in sorted((ROOT / "crates").glob("*/Cargo.toml")):
        text = path.read_text()
        name = re.search(r'^name\s*=\s*"([^"]+)"', text, re.M)
        if name:
            found[name.group(1)] = path
    return found


def publishes(text: str) -> bool:
    """Whether the manifest lets this crate reach a registry."""
    m = re.search(r"^publish\s*=\s*(.+)$", text, re.M)
    return not (m and m.group(1).strip() == "false")


def internal_deps(text: str, known: dict[str, Path]) -> set[str]:
    """The workspace crates this manifest depends on.

    Dev-dependencies are left out: each one is declared path-only, so cargo
    drops it from the manifest it packs, and a registry never needs it.
    """
    body = re.split(r"^\[dev-dependencies\]", text, flags=re.M)[0]
    body = re.split(r"^\[package\.metadata", body, flags=re.M)[0]
    # Only the real dependency sections count; a `[[bench]]` table naming a
    # crate is not a dependency.
    body = re.split(r"^\[\[", body, flags=re.M)[0]
    # Matched against `known` rather than a `sofd-*` pattern: the facade is
    # called `sofd`, and a pattern that insists on a suffix loses the one edge
    # every surface crate has.
    named = {
        name
        for name in known
        if re.search(rf"^{re.escape(name)}\s*[.=\s]", body, re.M)
    }
    named |= set(re.findall(r'^\S+\s*=\s*\{[^}]*package\s*=\s*"([^"]+)"', body, re.M))
    return {n for n in named if n in known}


def closure(entries: list[str], deps: dict[str, set[str]], published: set[str]):
    """The published crates reachable from `entries`, plus what is missing."""
    seen: set[str] = set()
    order: list[str] = []
    missing: set[str] = set()

    def visit(name: str, path: tuple[str, ...]) -> None:
        if name in seen:
            return
        if name in path:
            raise SystemExit(f"FATAL: dependency cycle {' -> '.join(path + (name,))}")
        if name not in published:
            missing.add(name)
        for d in sorted(deps.get(name, ())):
            visit(d, path + (name,))
        seen.add(name)
        order.append(name)

    for e in entries:
        visit(e, ())
    return order, missing


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--commands", action="store_true", help="print cargo publish lines")
    ap.add_argument(
        "--closure",
        action="append",
        default=[],
        metavar="CRATE",
        help="the entries to close over; defaults to every published crate",
    )
    args = ap.parse_args()

    known = manifests()
    published: set[str] = set()
    deps: dict[str, set[str]] = {}
    for name, path in known.items():
        text = path.read_text()
        deps[name] = internal_deps(text, known)
        if publishes(text):
            published.add(name)

    entries = args.closure or sorted(published)
    unknown = [e for e in entries if e not in known]
    if unknown:
        print(f"FATAL: no such crate: {', '.join(unknown)}", file=sys.stderr)
        return 1

    order, missing = closure(entries, deps, published)

    if missing:
        print(
            "FATAL: these crates are in the closure but do not publish, so the\n"
            "closure cannot be built:",
            file=sys.stderr,
        )
        for m in sorted(missing):
            print(f"  {m}", file=sys.stderr)
        return 1

    if args.commands:
        for name in order:
            print(f"cargo publish -p {name}")
    else:
        print(f"{len(order)} crate(s), in the order a registry accepts them:")
        for i, name in enumerate(order, 1):
            print(f"  {i:>2}. {name}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
