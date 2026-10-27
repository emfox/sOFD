#!/usr/bin/env python3
"""Say which crates a release has to ship, and at what version.

A release used to be the whole workspace, which is what one shared version
makes it. With per-crate versions it becomes a subset: the crates whose code
changed, and the crates above them whose version requirement has to move
because of it. This script computes that subset from the tree instead of
leaving it to be remembered.

The rule, stated once:

* A crate whose own code changed ships at the next version its change needs —
  the semver gate decides which, and this script reads the number the manifest
  already states.
* A crate whose dependency moved ships too, because its requirement names the
  version it was built against. That is the propagation, and it stops at the
  first crate whose requirement still admits the new version.
* A crate nobody touched, and whose dependencies did not move, is not in the
  release. Its published version stays what it is.

What "changed" means is the release's decision, not this script's: it takes a
`--since` ref and reads the diff. A change to a crate's documentation is a
change to the crate's page on the registry, so it counts.

The ref to pass is the last release's tag, and this script will tell you what
that is: `--since @last` asks every tag which versions it names the registry
actually serves and starts from the newest one that offers one. A tag stays
`v<version>` — `v0.6.0`, `v0.6.1` — because the products are named from it.

A release usually ships a subset, so a crate can reach the registry at a
version no tag names: the tag stays where the last full release put it, and
the crates that changed ship at patch versions above it. That is expected,
and `semver_baseline.py` is what holds such a crate to the release its
callers are on.

Usage:
    python3 scripts/release_plan.py --since @last
    python3 scripts/release_plan.py --since v0.5.1
    python3 scripts/release_plan.py --since v0.5.1 --bump sofd-display=0.6.0
    python3 scripts/release_plan.py --since v0.5.1 --offline   # no registry

Exit code: 0 when a plan can be made, 1 when the tree contradicts itself.
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
import tempfile
import tomllib
from pathlib import Path

sys.path.insert(0, str(Path(__file__).resolve().parent))

import check_versions as versions  # noqa: E402  (same directory, same workspace)
import publish_order  # noqa: E402

ROOT = Path(__file__).resolve().parent.parent


def named_version(tag: str) -> str | None:
    """The version a tag names, or None if it names none.

    A release tag is `v<version>`, and the version is the products' version:
    the release workflow names every download by dropping everything before the
    first dot. So the tag is not a separate number to choose — it is the
    version, and there is nothing else for it to disagree with.

    A release that ships a subset of the workspace leaves the tag where the
    last full release put it, so a crate can be published at a version no tag
    names. `semver_baseline.py` is what handles that; the tag's own shape is
    unaffected, which is why it is still just the version.

    A prerelease (`v0.1.0-rc.1`) names no release: nothing is compared against
    it and no plan starts from it.
    """
    match = re.fullmatch(r"v(\d+\.\d+\.\d+)", tag)
    return None if match is None else match.group(1)


def release_tag(version: str) -> str:
    """The tag for a release of `version`.

    One place states the shape, so a release cannot be cut with a tag the
    planner then fails to read back as "the last release".
    """
    return f"v{version}"


def tags_by_age() -> list[str]:
    """Every tag, newest first, by the commit each one names."""
    completed = subprocess.run(
        ["git", "tag", "--sort=-creatordate"],
        cwd=ROOT,
        capture_output=True,
        text=True,
    )
    if completed.returncode != 0:
        raise SystemExit(f"FATAL: cannot list tags: {completed.stderr.strip()}")
    return [line.strip() for line in completed.stdout.splitlines() if line.strip()]


def last_release() -> str:
    """The newest tag that carries a version the registry serves.

    A tag is a claim; the registry is the fact. Asking it which tags name
    something a caller can install is what separates the last release from a
    tag an abandoned run left behind, and it is the same question the semver
    gate asks.
    """
    published = [
        versions.name_of(manifest)
        for manifest in versions.members()
        if versions.publishes(manifest)
    ]
    for tag in tags_by_age():
        version = named_version(tag)
        if version is None:
            continue
        # One crate served at that version is enough. A release publishes in
        # dependency order, so a run that died halfway leaves the lower crates
        # on the registry; the tag still names where that release began, and a
        # half-released version is one the plan below will name again.
        if any(versions.on_registry(name, version) for name in published):
            print(f"Reading changes since {tag}, the newest tag the registry serves.")
            return tag
    raise SystemExit("FATAL: no tag names a version the registry serves")


def changed_since(ref: str) -> set[str]:
    """The crates a diff touches, by directory."""
    completed = subprocess.run(
        ["git", "diff", "--name-only", ref, "--", "crates"],
        cwd=ROOT,
        capture_output=True,
        text=True,
    )
    if completed.returncode != 0:
        raise SystemExit(
            f"FATAL: cannot diff against {ref}: {completed.stderr.strip()}"
        )
    touched: set[str] = set()
    for line in completed.stdout.splitlines():
        parts = line.split("/")
        if len(parts) >= 2 and parts[0] == "crates":
            touched.add(parts[1])
    return touched


def bump_of(version: str, level: str) -> str:
    """The next version at a level: major, minor, or patch."""
    numbers = [int(n) for n in version.split(".")]
    while len(numbers) < 3:
        numbers.append(0)
    major, minor, patch = numbers[:3]
    if level == "major":
        return f"{major + 1}.0.0"
    if level == "minor":
        return f"{major}.{minor + 1}.0"
    return f"{major}.{minor}.{patch + 1}"


def already_set(manifest: Path, root: Path) -> bool:
    """Whether a manifest states its own version rather than inheriting it.

    The difference matters because a crate whose version is still inherited is
    one the migration has not reached: the number is the workspace's, and the
    planner cannot ask it what this crate is worth.
    """
    block = versions.package_table(manifest.read_text(encoding="utf-8"))
    return re.search(r"^version\s*=", block, re.M) is not None


def named_version_in(manifest: Path, root: Path) -> str | None:
    """The version this crate's own manifest states, or None if it inherits."""
    return versions.version_of(manifest, root) if already_set(manifest, root) else None


def build_plan(
    since: str,
    overrides: dict[str, str],
    offline: bool,
    root: Path | None = None,
    changed: set[str] | None = None,
    served: "callable | None" = None,
    resolves: "callable | None" = None,
) -> tuple[list[tuple[str, str, str]], list[str]]:
    """The crates to ship as (name, version, reason), plus what is wrong.

    Walks the dependency graph upwards from the crates that changed. A crate
    joins when its requirement no longer admits a dependency's new version,
    which is exactly when a registry would resolve the wrong code.

    A crate whose manifest states its own version is taken to have shipped it:
    that number is the crate's, and a crate holds one version at a time. One
    whose version is still inherited has not been given a number for this
    release yet, and gets a patch bump.

    `root`, `changed`, `served` and `resolves` exist so the self-test can
    describe a workspace and a registry rather than point this at the real ones.
    """
    root = ROOT if root is None else root
    manifests = versions.members(root)
    names = {versions.name_of(m): m for m in manifests}
    current = {name: versions.version_of(m, root) for name, m in names.items()}
    shared = versions.workspace_versions()

    def decided(name: str) -> str | None:
        """The version this crate is being shipped at, if the tree says so."""
        stated = named_version_in(names[name], root)
        if stated is not None:
            return stated
        was = current.get(name) or shared.get(name)
        return None if was is None else bump_of(was, "patch")

    touched = changed_since(since) if changed is None else changed
    faults: list[str] = []

    unknown = sorted(set(overrides) - set(names))
    for name in unknown:
        faults.append(f"--bump names {name}, which is not a member")

    planned: dict[str, str] = {}
    reasons: dict[str, str] = {}
    for name in sorted(touched):
        if name not in names:
            continue
        if not versions.publishes(names[name]):
            continue
        version = overrides.get(name) or decided(name)
        if version is None:
            faults.append(f"{name} states no version")
            continue
        planned[name] = version
        if name in overrides:
            reasons[name] = "named by --bump"
        elif already_set(names[name], root):
            reasons[name] = f"states {version} in its manifest"
        else:
            reasons[name] = f"code changed since {since}"

    # Propagate: a dependent has to move when its requirement stops admitting
    # the new version. Repeat until nothing new joins, since a move can push
    # the version past another requirement further up.
    changed = True
    while changed:
        changed = False
        for name in sorted(names):
            if name in planned or not versions.publishes(names[name]):
                continue
            for key, crate, requirement in versions.requirements(names[name]):
                if crate not in planned:
                    continue
                if requirement is None:
                    satisfied = True  # inherited: still the workspace's number
                else:
                    satisfied = versions.admits(requirement, planned[crate])
                if satisfied:
                    continue
                version = overrides.get(name) or decided(name)
                if version is None:
                    faults.append(f"{name} states no version")
                    continue
                planned[name] = version
                reasons[name] = f"needs {crate} {planned[crate]}"
                changed = True
                break

    if not offline:
        ask = versions.on_registry if served is None else served
        for name, version in sorted(planned.items()):
            taken = ask(name, version)
            if taken:
                faults.append(
                    f"{name} {version} is already on the registry; pick a version "
                    "that is free with --bump"
                )
            elif taken is None:
                faults.append(f"{name} {version}: could not ask the registry")

        # A registry refuses a crate whose dependency version it does not serve
        # yet, so a plan has to move a sibling too whenever a shipping crate's
        # interval admits no version the registry has and the plan does not
        # carry one either. Publishing a list that fails halfway is what a
        # registry has no undo for, so this stops the plan rather than the
        # third `cargo publish`.
        shipping = set(planned)
        for name in sorted(shipping):
            for _, crate, requirement in versions.requirements(names[name]):
                if crate in shipping or crate not in names:
                    continue
                if requirement in (None, ""):
                    continue
                if ask(crate, current.get(crate) or "0.0.0") is None:
                    continue  # could not ask; the fault above already says so
                resolved = (
                    versions.resolves_on_registry(crate, requirement)
                    if resolves is None
                    else resolves(crate, requirement)
                )
                if resolved is not False:
                    continue
                wanted = current.get(crate) or shared.get(crate) or "?"
                faults.append(
                    f"{name} ships needing {crate} {requirement}, which the registry "
                    f"cannot resolve and the plan does not carry; add {crate} "
                    f"{wanted} with --bump {crate}={wanted}"
                )

    # The plan, as (name, version, reason). A release that names a version no
    # registry will accept is refused above; what is left is a list `main()`
    # prints and `publish_order.py` sorts into an order.
    plan = [(name, planned[name], reasons[name]) for name in sorted(planned)]
    return plan, faults


def self_test() -> int:
    """What the plan says about a workspace small enough to hold in the head.

    The cases are the ones the real tree only reaches at release time: a change
    that has to move its dependents, a change that must not, and a version the
    registry has already spent. `served` stands in for crates.io, so the test
    says what the answer should be instead of learning it from the network.
    """
    # The dependency sits under a `[dependencies]` heading, which is where the
    # parser looks: `requirements()` reads that table and nothing else.
    manifest = """[package]
name = "{name}"
version = "{version}"
edition = "2024"

[dependencies]
{deps}
"""
    workspace = """[workspace]
members = ["crates/sofd-a", "crates/sofd-b", "crates/sofd-c"]

[workspace.package]
edition = "2024"
"""

    def build(tree: dict[str, tuple[str, str]], changed: set[str], overrides, served):
        with tempfile.TemporaryDirectory() as temporary:
            root = Path(temporary)
            (root / "Cargo.toml").write_text(workspace, encoding="utf-8")
            for name, (version, deps) in tree.items():
                directory = root / "crates" / name
                directory.mkdir(parents=True)
                (directory / "Cargo.toml").write_text(
                    manifest.format(name=name, version=version, deps=deps),
                    encoding="utf-8",
                )
            return build_plan(
                "v1",
                overrides,
                offline=served is None,
                root=root,
                changed=changed,
                served=served,
            )

    # `b` needs `^0.5` of `a`, and `c` needs `^0.1` of `b`. Moving `a` to
    # `0.6.0` breaks `b`'s requirement, so `b` ships at `0.5.2` — which `c`'s
    # `^0.1` of `b` then stops admitting, so `c` ships too. Moving `a` to
    # `0.5.2` instead stays inside `b`'s interval and nothing above it ships.
    # That second step is the part worth testing: propagation is transitive.
    chain = {
        "sofd-a": ("0.5.1", ""),
        "sofd-b": ("0.5.1", 'sofd-a = { path = "../sofd-a", version = "0.5" }'),
        "sofd-c": ("0.5.1", 'sofd-b = { path = "../sofd-b", version = "0.1" }'),
    }

    cases: list[tuple[str, dict, set, dict, object, list[str]]] = [
        (
            "a patch inside the interval stops at a",
            chain,
            {"sofd-a"},
            {},
            None,
            ["sofd-a"],
        ),
        (
            "a minor break moves the chain above it",
            chain,
            {"sofd-a"},
            {"sofd-a": "0.6.0"},
            None,
            ["sofd-a", "sofd-b", "sofd-c"],
        ),
        (
            "a version the registry already serves is a fault",
            chain,
            {"sofd-a"},
            {},
            lambda crate, version: True,
            None,
        ),
        (
            "an untouched crate with untouched deps stays out",
            chain,
            {"sofd-c"},
            {},
            None,
            ["sofd-c"],
        ),
    ]

    failed = 0
    for description, tree, changed, overrides, served, wanted in cases:
        plan, faults = build(tree, changed, overrides, served)
        got = [name for name, _, _ in plan]
        # `None` means the case expects a fault rather than a plan: a version
        # already spent is refused, and `main()` stops on it.
        if wanted is None:
            if not faults:
                failed += 1
                print(f"FAIL  {description}: planned {got} with no fault")
        elif got != wanted or faults:
            failed += 1
            print(f"FAIL  {description}: got {got} {faults}, wanted {wanted}")

    # The reason is what a release record quotes, so it has to name the crate
    # that forced the move rather than saying "changed" about a crate that did
    # not.
    plan, _ = build(chain, {"sofd-a"}, {"sofd-a": "0.6.0"}, None)
    reasons = {name: reason for name, _, reason in plan}
    if reasons.get("sofd-b") != "needs sofd-a 0.6.0":
        failed += 1
        print(f"FAIL  sofd-b's reason: {reasons.get('sofd-b')!r}")

    print(f"{'FAILED' if failed else 'OK'}: {len(cases) + 1 - failed} of {len(cases) + 1} case(s)")
    return 1 if failed else 0


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--selftest",
        action="store_true",
        help="check the planner against a workspace it builds and a registry it "
        "pretends to ask",
    )
    parser.add_argument(
        "--since",
        metavar="REF",
        help="the commit or tag the release starts from; @last for the newest "
        "tag the registry serves",
    )
    parser.add_argument(
        "--bump",
        action="append",
        default=[],
        metavar="CRATE=VERSION",
        help="state a version rather than deriving one; repeatable",
    )
    parser.add_argument(
        "--offline",
        action="store_true",
        help="do not ask the registry whether a version is free",
    )
    parser.add_argument(
        "--tag",
        metavar="VERSION",
        help="the version this release ships as, so the tag to push can be "
        "printed; it names the products, not any crate in the plan",
    )
    args = parser.parse_args()

    if args.selftest:
        return self_test()
    if args.since is None:
        parser.error("--since is required unless --selftest is given")
    if args.tag is not None and re.fullmatch(r"\d+\.\d+\.\d+", args.tag) is None:
        parser.error(f"--tag expects a version like 0.6.0, not {args.tag!r}")

    overrides: dict[str, str] = {}
    for entry in args.bump:
        if "=" not in entry:
            print(f"FATAL: --bump expects CRATE=VERSION, not {entry!r}", file=sys.stderr)
            return 1
        name, version = entry.split("=", 1)
        if re.fullmatch(r"\d+\.\d+\.\d+", version) is None:
            print(f"FATAL: {name} {version} is not a version", file=sys.stderr)
            return 1
        overrides[name.strip()] = version.strip()

    since = args.since
    if since == "@last" and not args.offline:
        since = last_release()
    elif since == "@last":
        print(
            "FATAL: --since @last asks the registry, and --offline says not to",
            file=sys.stderr,
        )
        return 1

    plan, faults = build_plan(since, overrides, args.offline)
    for fault in faults:
        print(f"FATAL: {fault}", file=sys.stderr)
    if faults:
        return 1

    if not plan:
        print(f"Nothing to release since {args.since}.")
        return 0

    width = max(len(name) for name, _, _ in plan)
    print(f"{len(plan)} crate(s) to release:")
    for name, version, reason in plan:
        print(f"  {name:<{width}}  {version:<8}  {reason}")
    print()
    # The plan is a subset, so it needs the same topological sort the whole
    # workspace gets: `sofd-cli` may ship while the crate it needs does not
    # ship again, and the order still has to put that crate first.
    manifests = versions.members()
    known = {versions.name_of(m): m for m in manifests}
    deps = {
        name: publish_order.internal_deps(m.read_text(), known)
        for name, m in known.items()
    }
    # Only the edges between crates that are both shipping: a dependency that
    # stays put is already on the registry, so it does not join the plan.
    shipping = {name for name, _, _ in plan}
    inside = {name: deps.get(name, set()) & shipping for name in shipping}
    order, _ = publish_order.closure(sorted(shipping), inside, shipping)
    print("Publish in this order:")
    for name in order:
        print(f"  cargo publish -p {name}")

    # The tag, once the registry has the plan. A release is named for the
    # products it ships, not for any crate inside it, so the number is the one
    # a user would say if asked which version of sOFD they have.
    if args.tag:
        print()
        print(f"Tag this commit {release_tag(args.tag)}.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
