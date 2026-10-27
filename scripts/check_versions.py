#!/usr/bin/env python3
"""Check that every internal dependency requirement names a version that works.

`docs/Versioning.md` describes per-crate versions, and this is the check that
makes them safe. A crate's manifest states an interval for each sibling it
depends on — `sofd-display = "0.6"` — and cargo resolves that interval against
the registry once the crate is published. An interval naming a version that is
not there, or one that no longer resolves, is a defect a consumer sees and this
tree cannot: it installs fine locally, because locally the path is what cargo
uses.

So each requirement is asked two questions:

* Is the version this tree declares inside the interval? A requirement that
  excludes the sibling's own version describes a tree that does not exist.
* Does the interval resolve to something the registry serves? Asked only when
  the sibling is published at all, since before a first release there is
  nothing to resolve against. A crate the tree is about to publish is not a
  mistake — it is a release in progress — so that one is named and the run
  goes on. `release_plan.py` is where it becomes a blocker, because a registry
  refuses a crate whose dependency it does not have and there is no undo.

The two questions are the reason this exists: neither can be answered from the
other, and a mistake in either is invisible until a consumer's build fails.

Usage:
    python3 scripts/check_versions.py            # check the workspace
    python3 scripts/check_versions.py --list     # every requirement, resolved

Exit code: 0 when every requirement holds, 1 when one does not.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
import urllib.error
import urllib.request
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent

USER_AGENT = "sofd-checks (https://github.com/emfox/sOFD)"


def members(root: Path = ROOT) -> list[Path]:
    """Every member manifest, in the workspace's own order."""
    text = (root / "Cargo.toml").read_text(encoding="utf-8")
    block = re.search(r"^members\s*=\s*\[(.*?)\]", text, re.M | re.S)
    if block is None:
        return []
    found = sorted(set(re.findall(r'"(crates/[a-z0-9-]+)"', block.group(1))))
    return [root / member / "Cargo.toml" for member in found]


def package_table(text: str) -> str:
    """The `[package]` table of a manifest, and nothing after it."""
    match = re.search(r"^\[package\]\n(.*?)(?=^\[|\Z)", text, re.M | re.S)
    return match.group(1) if match else ""


def version_of(manifest: Path, root: Path = ROOT) -> str | None:
    """The version this manifest states, whether written or inherited."""
    text = (root / "Cargo.toml").read_text(encoding="utf-8")
    block = package_table(manifest.read_text(encoding="utf-8"))
    if re.search(r"^version\.workspace\s*=\s*true", block, re.M):
        workspace = re.search(
            r"^\[workspace\.package\]\n(.*?)(?=^\[|\Z)", text, re.M | re.S
        )
        if workspace is None:
            return None
        inherited = re.search(r'^version\s*=\s*"([^"]+)"', workspace.group(1), re.M)
        return inherited.group(1) if inherited else None
    own = re.search(r'^version\s*=\s*"([^"]+)"', block, re.M)
    return own.group(1) if own else None


def publishes(manifest: Path) -> bool:
    """Whether the crate goes to a registry."""
    block = package_table(manifest.read_text(encoding="utf-8"))
    stated = re.search(r"^publish\s*=\s*(.+)$", block, re.M)
    return not (stated and stated.group(1).strip() == "false")


def name_of(manifest: Path) -> str:
    block = package_table(manifest.read_text(encoding="utf-8"))
    found = re.search(r'^name\s*=\s*"([^"]+)"', block, re.M)
    return found.group(1) if found else manifest.parent.name


def publishes_named(name: str) -> bool:
    """Whether the crate with this name goes to a registry."""
    for manifest in members():
        if name_of(manifest) == name:
            return publishes(manifest)
    raise SystemExit(f"FATAL: no member crate is named {name}")


def tables(text: str) -> list[tuple[str, str]]:
    """Each dependency table in a manifest, as (heading, body).

    Three of them matter: `[dependencies]`, `[dev-dependencies]` and
    `[build-dependencies]`. A dev-dependency still has to name a version — the
    one sibling exempted is `sofd-testkit`, which cannot be named by version
    because it is never published, so it is skipped by name below rather than
    by table. A build-dependency would have to publish, and none does.
    """
    found = re.findall(
        r"^\[(dependencies|dev-dependencies|build-dependencies)\]\n(.*?)(?=^\[|\Z)",
        text,
        re.M | re.S,
    )
    return list(found)


def requirements(manifest: Path) -> list[tuple[str, str, str]]:
    """Each internal dependency this manifest states, as (key, crate, requirement).

    A requirement of `""` means the entry states no version at all, which is a
    fault for every crate a registry will serve. `""` from a dev-dependency on
    `sofd-testkit` is expected: it is the one member that `publish = false`, so
    a version on it would be a requirement on a crate no registry serves.
    """
    text = manifest.read_text(encoding="utf-8")
    found: list[tuple[str, str, str]] = []
    for heading, body in tables(text):
        found.extend(_table_requirements(body))
    return found


def _table_requirements(block: str) -> list[tuple[str, str, str]]:
    found: list[tuple[str, str, str]] = []
    for line in block.splitlines():
        stripped = line.split("#", 1)[0].strip()
        if not stripped or "=" not in stripped:
            continue
        # Two spellings of the same thing: `sofd-error.workspace = true` and
        # `sofd-error = { workspace = true }`. Only the second carries its
        # settings inline, and only that one can restate a version.
        key, rest = stripped.split("=", 1)
        key = key.strip()
        # `sofd-error.workspace = true` states no number; `sofd-error = {..}`
        # carries its settings inline and can restate the version there.
        inherited = "." in key
        if inherited:
            key = key.split(".", 1)[0]
            rest = ""
        renamed = re.search(r'package\s*=\s*"([^"]+)"', rest)
        crate = renamed.group(1) if renamed else key
        if not crate.startswith("sofd"):
            continue
        if inherited:
            found.append((key, crate, None))
            continue
        stated = re.search(r'version\s*=\s*"([^"]+)"', rest)
        if stated is not None:
            found.append((key, crate, stated.group(1)))
            continue
        # `{ workspace = true, optional = true }` states no number either: the
        # requirement is the workspace entry's, and this says so.
        if re.search(r"workspace\s*=\s*true", rest):
            found.append((key, crate, None))
            continue
        found.append((key, crate, ""))
    return found


def parse(requirement: str) -> tuple[int, list[tuple[str, tuple[int, ...]]]] | None:
    """A version requirement as (count of comparators, [(operator, version)]).

    Only the forms this project uses or could reasonably reach for: a bare
    `0.6`, a caret, a tilde, and comma-joined comparisons. `*` is refused
    rather than expanded, because a wildcard requirement is a decision nobody
    made on purpose.
    """
    requirement = requirement.strip()
    if requirement in ("", "*"):
        return None
    parts: list[tuple[str, tuple[int, ...]]] = []
    for piece in requirement.split(","):
        piece = piece.strip()
        match = re.match(r"^(\^|~|>=|<=|>|<|=)?\s*v?(\d+(?:\.\d+)*)$", piece)
        if match is None:
            return None
        operator = match.group(1) or "^"
        numbers = tuple(int(n) for n in match.group(2).split("."))
        parts.append((operator, numbers))
    return len(parts), parts


def pad(version: tuple[int, ...], width: int = 3) -> tuple[int, ...]:
    return version + (0,) * (width - len(version))


def caret_upper(version: tuple[int, ...]) -> tuple[int, ...]:
    """The first version a caret requirement excludes.

    `^0.6.1` allows `0.6.x` and stops at `0.7.0`; below 1.0 the minor number is
    the incompatible one, and `^0.0.3` allows only `0.0.3`.
    """
    numbers = pad(version)
    major, minor, patch = numbers[0], numbers[1], numbers[2]
    if major > 0:
        return (major + 1, 0, 0)
    if minor > 0:
        return (0, minor + 1, 0)
    return (0, 0, patch + 1)


def tilde_upper(version: tuple[int, ...]) -> tuple[int, ...]:
    numbers = pad(version)
    return (numbers[0], numbers[1] + 1, 0)


def admits(requirement: str, version: str) -> bool:
    """Whether a requirement allows a version, by the rules cargo uses."""
    parsed = parse(requirement)
    if parsed is None:
        return False
    _, parts = parsed
    candidate = pad(tuple(int(n) for n in version.split(".")))
    for operator, bound in parts:
        low = pad(bound)
        if operator in ("=", "^"):
            if not (low <= candidate < caret_upper(bound)):
                return False
        elif operator == "~":
            if not (low <= candidate < tilde_upper(bound)):
                return False
        elif operator == ">=":
            if candidate < low:
                return False
        elif operator == ">":
            if candidate <= low:
                return False
        elif operator == "<=":
            if candidate > low:
                return False
        elif operator == "<":
            if candidate >= low:
                return False
    return True


def on_registry(crate: str, version: str) -> bool | None:
    """Whether the registry serves this version; `None` when it cannot be asked."""
    url = f"https://crates.io/api/v1/crates/{crate}/{version}"
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    try:
        with urllib.request.urlopen(request, timeout=20) as response:
            return response.status == 200
    except urllib.error.HTTPError as error:
        if error.code == 404:
            return False
        return None
    except (urllib.error.URLError, TimeoutError):
        return None


def resolves_on_registry(crate: str, requirement: str) -> bool | None:
    """Whether any published version of `crate` satisfies the requirement."""
    url = f"https://crates.io/api/v1/crates/{crate}"
    request = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    try:
        with urllib.request.urlopen(request, timeout=20) as response:
            body = json.loads(response.read().decode("utf-8"))
    except urllib.error.HTTPError as error:
        return False if error.code == 404 else None
    except (urllib.error.URLError, TimeoutError, json.JSONDecodeError):
        return None
    versions = body.get("versions") or []
    served = [
        entry["num"]
        for entry in versions
        if not entry.get("yanked") and re.match(r"^\d+(\.\d+)*$", entry.get("num", ""))
    ]
    if not served:
        return False
    return any(admits(requirement, version) for version in served)


def workspace_versions() -> dict[str, str]:
    """The version each internal crate states in `[workspace.dependencies]`.

    A manifest that writes `sofd-error.workspace = true` states no number of
    its own; the number is here. Reading it is what lets one crate move to its
    own version while its neighbours still point at the shared entry.
    """
    text = (ROOT / "Cargo.toml").read_text(encoding="utf-8")
    block = re.search(r"^\[workspace\.dependencies\]\n(.*?)(?=^\[|\Z)", text, re.M | re.S)
    if block is None:
        return {}
    found: dict[str, str] = {}
    for line in block.group(1).splitlines():
        stripped = line.split("#", 1)[0].strip()
        alias = re.match(r"^([a-z0-9-]+)\s*=\s*\{(.*)\}$", stripped)
        if alias is None:
            continue
        crate = re.search(r'package\s*=\s*"([^"]+)"', alias.group(2))
        version = re.search(r'version\s*=\s*"([^"]+)"', alias.group(2))
        if version is None:
            continue
        found[crate.group(1) if crate else alias.group(1)] = version.group(1)
    return found


def self_test() -> int:
    """The interval rules, on the cases that are easy to get wrong.

    cargo reads a bare `0.5.1` as `^0.5.1`, so it admits `0.5.2`; below 1.0 the
    minor number is the incompatible one, so `^0.5.1` stops at `0.6.0` and
    `^0.0.3` stops at `0.0.4`. Getting any of these backwards would make the
    check pass a requirement cargo rejects, or reject one it accepts.
    """
    cases = [
        ("^0.5.1", "0.5.1", True),
        ("^0.5.1", "0.5.9", True),
        ("^0.5.1", "0.6.0", False),
        ("^0.5.1", "0.5.0", False),
        ("0.6", "0.6.3", True),
        ("0.6", "0.7.0", False),
        ("0.5.1", "0.5.2", True),
        ("0.5.1", "0.4.9", False),
        (">=0.5, <0.7", "0.6.1", True),
        (">=0.5, <0.7", "0.7.0", False),
        ("~0.5.1", "0.5.8", True),
        ("~0.5.1", "0.6.0", False),
        ("^0.0.3", "0.0.3", True),
        ("^0.0.3", "0.0.4", False),
        ("^1.2", "1.9.0", True),
        ("^1.2", "2.0.0", False),
    ]
    failed = 0
    for requirement, version, wanted in cases:
        got = admits(requirement, version)
        if got != wanted:
            failed += 1
            print(
                f"FAIL  {requirement!r} against {version} gave {got}, wanted {wanted}",
                file=sys.stderr,
            )
    print(f"{'FAILED' if failed else 'OK'}: {len(cases) - failed} of {len(cases)} interval(s)")
    return 1 if failed else 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--list", action="store_true", help="print every requirement and its verdict"
    )
    parser.add_argument(
        "--offline",
        action="store_true",
        help="skip the registry question, checking only the tree against itself",
    )
    parser.add_argument(
        "--selftest", action="store_true", help="check the interval rules themselves"
    )
    args = parser.parse_args()

    if args.selftest:
        return self_test()

    manifests = members()
    if not manifests:
        print("FATAL: no member crates found", file=sys.stderr)
        return 1

    names = {name_of(m): m for m in manifests}
    versions = {name: version_of(m) for name, m in names.items()}
    shared = workspace_versions()
    faults: list[str] = []
    rows: list[tuple[str, str, str, str]] = []
    # A crate not on the registry yet is mid-release rather than wrong, which
    # is why it is a warning here and a blocker in `release_plan.py`: this
    # check answers "is the tree right", the plan answers "may it go out now".
    unreleased: list[str] = []

    for name in sorted(names):
        for key, crate, requirement in requirements(names[name]):
            if crate not in names:
                faults.append(f"{name}: depends on {crate}, which is not a member")
                continue
            sibling = versions.get(crate) or shared.get(crate)
            if requirement is None:
                if sibling is None:
                    faults.append(
                        f"{name}: {key} inherits a version that neither the workspace "
                        f"nor {crate} states"
                    )
                    continue
                requirement = sibling
            if requirement == "":
                # `sofd-testkit` is the one member no registry will serve, so a
                # version on it is the thing that cannot work. Read from that
                # crate's own manifest rather than assumed from its name: a new
                # crate that opts out is exempt without this check being edited.
                if not any(
                    publishes(m) is False and name_of(m) == crate for m in manifests
                ):
                    faults.append(
                        f"{name}: {key} states no version, so a registry cannot resolve it"
                    )
                continue
            if parse(requirement) is None:
                faults.append(
                    f"{name}: {key} = \"{requirement}\" is not a requirement this check reads"
                )
                continue

            if sibling is None:
                faults.append(f"{name}: {key} names {crate}, which states no version")
                continue

            own = admits(requirement, sibling)
            if not own:
                faults.append(
                    f"{name}: {key} = \"{requirement}\" excludes {crate} {sibling}, "
                    "the version this tree builds"
                )
                rows.append((name, key, requirement, "excludes the tree"))
                continue

            if args.offline or not publishes(names[crate]):
                rows.append((name, key, requirement, f"admits {sibling}"))
                continue

            served = resolves_on_registry(crate, requirement)
            if served:
                rows.append((name, key, requirement, f"admits {sibling}"))
                continue
            if served is None:
                rows.append((name, key, requirement, "registry unasked"))
                continue

            # Nothing served satisfies the interval. That is either a mistake
            # or a release that has not finished: the sibling's own version may
            # be the one intended, waiting on the registry.
            if on_registry(crate, sibling) is False:
                rows.append((name, key, requirement, f"admits {sibling}, unreleased"))
                entry = f"{name} needs {crate} {sibling}, which the registry does not serve yet"
                if entry not in unreleased:
                    unreleased.append(entry)
                continue
            faults.append(
                f"{name}: {key} = \"{requirement}\" resolves against no published "
                f"version of {crate}"
            )
            rows.append((name, key, requirement, "unresolvable"))

    if args.list:
        for name, key, requirement, verdict in rows:
            print(f"{name:16} {key:18} {requirement:10} {verdict}")

    for fault in faults:
        print(fault, file=sys.stderr)
    if faults:
        print(f"FAILED: {len(faults)} requirement(s) do not hold", file=sys.stderr)
        return 1
    for entry in unreleased:
        print(f"NOTE: {entry}; publish it first", file=sys.stderr)
    print(
        f"OK: {len(rows)} internal requirement(s) name a version that resolves"
        + (f" once {len(unreleased)} pending release(s) go out" if unreleased else "")
    )
    return 0


if __name__ == "__main__":
    sys.exit(main())
