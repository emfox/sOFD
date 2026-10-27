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
    python3 scripts/publish_order.py --check     # assert the order holds

`--rename` takes a version a dependency interval stands for and prints the one
its `--commands` will name. An interval answers "what resolves", and at
packaging time `sofd = "0.5"` becomes `sofd = "0.5.1"` — the newest the
registry holds. A release that moves `sofd` to `0.6.0` therefore has to say so,
or the commands below would ask for a `sofd` the registry does not have yet.

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
    # A `[[bench]]` table names the crate its benchmark lives in, which is not a
    # dependency; drop only that table. `[[bin]]` and `[[test]]` sit among the
    # real dependency tables, so cutting at the first `[[` would lose them.
    body = re.sub(r"^\[\[bench\]\].*?(?=^\[|\Z)", "", body, flags=re.M | re.S)
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
    """The published crates reachable from `entries`, plus what is missing.

    Kahn's algorithm rather than a depth-first walk. A walk emits a crate the
    first time it is reached, so a crate can land before a dependency of a
    later entry — `sofd-cli` came out ahead of `sofd-i18n`, which it needs.
    Repeatedly taking every crate whose dependencies are already in the order
    cannot do that.
    """
    reachable: set[str] = set()
    missing: set[str] = set()

    def walk(name: str, path: tuple[str, ...]) -> None:
        if name in reachable:
            return
        if name in path:
            raise SystemExit(f"FATAL: dependency cycle {' -> '.join(path + (name,))}")
        if name not in published:
            missing.add(name)
        reachable.add(name)
        for d in sorted(deps.get(name, ())):
            walk(d, path + (name,))

    for e in entries:
        walk(e, ())

    order: list[str] = []
    done: set[str] = set()
    while len(order) < len(reachable):
        ready = sorted(
            name
            for name in reachable - done
            if not (deps.get(name, set()) & reachable) - done
        )
        if not ready:
            raise SystemExit(
                f"FATAL: dependency cycle among {', '.join(sorted(reachable - done))}"
            )
        order.extend(ready)
        done.update(ready)
    return order, missing


def verify(order: list[str], deps: dict[str, set[str]], known: dict[str, Path]) -> int:
    """Check the printed order against the dependency edges it claims to respect.

    An edge that was never parsed cannot be checked, so the second question is
    whether the parse saw the dependencies the manifest actually states: every
    workspace crate named in a `[dependencies]` table has to appear in `deps`.
    """
    position = {name: i for i, name in enumerate(order)}
    faults = [
        (name, dep)
        for name in order
        for dep in sorted(deps.get(name, ()))
        if dep in position and position[dep] > position[name]
    ]
    for name, dep in faults:
        print(f"{name} is placed before {dep}, which it depends on", file=sys.stderr)

    dropped = []
    for name, path in sorted(known.items()):
        stated = stated_deps(path.read_text(), known)
        if stated - deps.get(name, set()):
            dropped.append((name, sorted(stated - deps.get(name, set()))))
    for name, lost in dropped:
        print(f"{name} states {', '.join(lost)} but the parse missed it", file=sys.stderr)

    if faults or dropped:
        print(
            f"FAILED: {len(faults)} edge(s) out of order, "
            f"{len(dropped)} crate(s) with unparsed dependencies",
            file=sys.stderr,
        )
        return 1
    print(f"OK: {len(order)} crate(s), every dependency ahead of the crate that needs it")
    return 0


def stated_deps(text: str, known: dict[str, Path]) -> set[str]:
    """The workspace crates a manifest names under `[dependencies]`, however far down.

    Deliberately a separate, simpler reading than `internal_deps`: it is what
    the real parse is checked against, so sharing the code would share the bug.
    """
    found: set[str] = set()
    inside = False
    for line in text.splitlines():
        stripped = line.strip()
        if stripped.startswith("["):
            inside = stripped == "[dependencies]"
            continue
        if not inside or "=" not in stripped or stripped.startswith("#"):
            continue
        key, rest = stripped.split("=", 1)
        key = key.strip()
        # The key is the crate's own name, or one it renames with `package`.
        renamed = re.search(r'package\s*=\s*"([^"]+)"', rest)
        if renamed and renamed.group(1) in known:
            found.add(renamed.group(1))
        elif key in known:
            found.add(key)
    return found


def constraints(name: str, known: dict[str, Path], shipped: dict[str, str]) -> str:
    """What the line for `name` has to say beyond `cargo publish -p`.

    Cargo has no flag that names a dependency's version, so a release that
    ships one says it in the manifest and the line adds nothing — but a reader
    of the printed order needs to know that `sofd-cli` will be built against
    `sofd 0.6.0` rather than the `0.5.1` the registry already holds. Saying it
    here is what keeps `--commands` honest about that, since the packaged
    manifest is where the number actually comes from.
    """
    if name not in known:
        return ""
    needs = sorted(
        crate for crate in internal_deps(known[name].read_text(), known) if crate in shipped
    )
    if not needs:
        return ""
    named = ", ".join(f"{crate} {shipped[crate]}" for crate in needs)
    return f"  # built against {named}"


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--commands", action="store_true", help="print cargo publish lines")
    ap.add_argument(
        "--check",
        action="store_true",
        help="assert the order respects every dependency edge",
    )
    ap.add_argument(
        "--closure",
        action="append",
        default=[],
        metavar="CRATE",
        help="the entries to close over; defaults to every published crate",
    )
    ap.add_argument(
        "--rename",
        action="append",
        default=[],
        metavar="CRATE=VERSION",
        help="the version this release ships a dependency at, so --commands names "
        "that one instead of the newest the registry holds; repeatable",
    )
    args = ap.parse_args()

    shipped: dict[str, str] = {}
    for entry in args.rename:
        if "=" not in entry:
            print(f"FATAL: --rename expects CRATE=VERSION, not {entry!r}", file=sys.stderr)
            return 1
        key, version = entry.split("=", 1)
        shipped[key.strip()] = version.strip()
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

    if args.check:
        return verify(order, deps, known)

    if args.commands:
        for name in order:
            # A release names the versions it ships, so the line for a crate
            # that depends on one of them says which. Left alone, cargo would
            # pick the newest version the registry holds, which is the one the
            # release is replacing.
            print(f"cargo publish -p {name}{constraints(name, known, shipped)}")
    else:
        print(f"{len(order)} crate(s), in the order a registry accepts them:")
        for i, name in enumerate(order, 1):
            print(f"  {i:>2}. {name}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
