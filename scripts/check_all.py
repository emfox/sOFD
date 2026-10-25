#!/usr/bin/env python3
"""Run every check this repository expects, from one command.

CI runs these as separate steps on a Linux runner that already has cargo, lxml
and the fetched samples. A working copy rarely does, so this script runs what the
machine can run, says why it skipped the rest, and never reports a skip as a
pass.

Usage:
    python3 scripts/check_all.py                # everything this machine can run
    python3 scripts/check_all.py --quick        # no cargo, no fetched samples
    python3 scripts/check_all.py --list         # what would run, and why
    python3 scripts/check_all.py --only policy --only i18n
    python3 scripts/check_all.py --skip rust
    python3 scripts/check_all.py --require-all  # a skip is a failure
    python3 scripts/check_all.py --verbose      # stream each check's output

Exit code: 0 when nothing failed, 1 when a check failed, and with --require-all
also when a check had to be skipped.
"""

from __future__ import annotations

import argparse
import importlib.util
import os
import re
import shutil
import subprocess
import sys
import time
from dataclasses import dataclass
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
PYTHON = sys.executable

# Cheap checks first, so a policy mistake is reported before a build is spent.
GROUPS = ("policy", "repo", "schema", "deps", "rust", "samples", "perf", "review")

# Groups that need a Rust toolchain, a network fetch or a benchmark run. --quick
# leaves them out; everything else can finish on a fresh clone.
SLOW_GROUPS = ("rust", "samples", "perf")

# Groups whose findings ask for a second look instead of blocking a merge. A
# check here reports what it noticed and the run still passes with it.
ADVISORY_GROUPS = ("review",)

FIXTURE_FILES = "compliance/fixtures"


@dataclass(frozen=True)
class Check:
    """One gate: the steps to run, and what the machine must have for them."""

    name: str
    group: str
    steps: tuple[tuple[str, ...], ...]
    needs: tuple[str, ...] = ()
    unset: tuple[str, ...] = ()
    env: tuple[tuple[str, str], ...] = ()
    # Run the steps under the manifest's minimum toolchain rather than whichever
    # one is on PATH, so what the manifest claims is what was checked.
    msrv: bool = False


def py(*args: str) -> tuple[str, ...]:
    """A sibling script, run by the interpreter running this one.

    Each argument stays its own word: a step is a sequence of words, so a
    joined script and argument reads as one executable that cannot be opened.
    """
    return (PYTHON, *args)


def fixture_targets() -> list[str]:
    """Every fixture archive and XML source, in the order CI passes them."""
    root = ROOT / FIXTURE_FILES
    found = [
        p for p in root.rglob("*")
        if p.is_file() and p.suffix.lower() in (".ofd", ".xml")
    ]
    return [str(p.relative_to(ROOT)) for p in sorted(found)]


CHECKS: tuple[Check, ...] = (
    # Section 4 of docs/Conventions.md: the policy gates.
    Check("language", "policy", (py("scripts/check_language.py"),)),
    Check("i18n", "policy", (py("scripts/check_i18n.py", "--strict"),)),
    # Whether a crate reaches a registry is a decision, and one made once per
    # crate. This holds the two sets to the whole workspace and a published
    # crate to what a registry asks for, so a new crate cannot slip in unnamed.
    Check("publish", "policy", (py("scripts/check_publish.py"),)),
    # Publishing is a closure, not a list: a published crate names another, and
    # cargo resolves that name from the registry once the crate leaves this
    # tree. This fails when the published set is not closed under its own
    # dependencies, which is the mistake that only shows up at release time.
    Check("publish-order", "policy", (py("scripts/publish_order.py"),)),
    # A crate reaches a registry as the tarball cargo makes, which holds the
    # crate's own directory and not the repository above it. An `include_str!`
    # that climbs out of it therefore compiles here and fails at release time,
    # after the tag is pushed. This builds the tarball list and reads it back.
    Check("packaging", "policy", (py("scripts/check_packaging.py"),),
          needs=("cargo",)),
    # The two READMEs drifted twice, and every count in them is a copy of
    # something the workspace knows. The English one is the source of truth;
    # this fails when the Chinese one states a different figure.
    Check("readme", "policy", (py("scripts/check_readme.py"),)),
    Check("android", "package", (py("scripts/android_package.py", "selftest"),)),
    # A release attaches one archive per product rather than the parts a build
    # leaves behind, so what a download contains is a promise this makes.
    Check("release-package", "package", (py("scripts/release_package.py", "selftest"),)),
    Check("uos", "package", (py("scripts/uos_package.py", "selftest"),)),
    Check(
        "commits",
        "policy",
        (py("scripts/check_commits.py", "--grandfathered",
            "scripts/commit-baseline.txt"),),
        needs=("git",),
    ),
    # Section 3.1 asks for one logical change per commit; this names the
    # commits that read as more than one. It reports, it does not block.
    Check(
        "commit-scope",
        "review",
        (py("scripts/check_commit_scope.py"),),
        needs=("git",),
    ),
    # Artefacts the repository carries: a manifest tool and a build product.
    # The checkers are code too. A checker that silently narrows what it
    # reports is worse than one that fails, so it gets the same treatment.
    Check(
        "self-tests",
        "repo",
        (py("-m", "unittest", "discover", "-s", "scripts", "-p", "test_*.py"),),
    ),
    Check("checksums", "repo", (py("scripts/release_checksums.py", "selftest"),)),
    Check("fixture-pack", "repo", (py("scripts/pack_fixtures.py", "--check"),)),
    # Annex A, over the fixtures that ship with the tree.
    Check(
        "annex-a",
        "schema",
        (py("scripts/validate_ofd.py", "--allow-foreign", *fixture_targets()),),
        needs=("lxml", "fixture-sources"),
    ),
    # The Rust gate, in the order CI runs it.
    Check("fmt", "rust", (("cargo", "fmt", "--all", "--check"),), needs=("cargo",)),
    Check(
        "clippy",
        "rust",
        (("cargo", "clippy", "--workspace", "--all-targets", "--all-features",
          "--", "-D", "warnings"),),
        needs=("cargo",),
        # Only this workspace is held to -D warnings; a dependency's lint is not
        # this build's to fail. CI passes an empty value for the same reason.
        env=(("RUSTFLAGS", ""),),
    ),
    # The manifest names the minimum; this checks the workspace under exactly
    # that toolchain, so `rust-version` is a fact rather than a claim.
    Check(
        "msrv",
        "rust",
        (("cargo", "check", "--workspace", "--all-targets"),),
        needs=("cargo", "msrv"),
        msrv=True,
    ),
    Check(
        "build",
        "rust",
        (("cargo", "build", "--workspace", "--all-targets", "--all-features"),),
        needs=("cargo",),
    ),
    Check("test", "rust", (("cargo", "test", "--workspace"),), needs=("cargo",)),
    Check(
        "test-all-features",
        "rust",
        (("cargo", "test", "--workspace", "--all-features"),),
        needs=("cargo",),
    ),
    Check(
        "golden",
        "rust",
        (("cargo", "test", "-p", "sofd-convert", "--test", "golden"),),
        needs=("cargo",),
    ),
    Check(
        "reader-window",
        "rust",
        (("cargo", "test", "-p", "sofd-reader", "--test", "window"),),
        needs=("cargo",),
        unset=("DISPLAY", "WAYLAND_DISPLAY"),
    ),
    Check(
        "compliance",
        "rust",
        (py("scripts/check_compliance.py", "--check-only"),),
        needs=("cargo",),
    ),
    Check(
        "compliance-matrix",
        "rust",
        (
            py("scripts/check_compliance.py"),
            ("git", "diff", "--exit-code", "--", "docs/Compliance.md"),
        ),
        needs=("cargo", "git"),
    ),
    # The dependency policy. It reads the lock file, so it answers a question
    # about a crate nobody in this repository wrote, and fails on one with a
    # published advisory rather than on one that is merely old.
    Check(
        "advisories",
        "deps",
        (("cargo", "deny", "--all-features", "check"),),
        needs=("cargo", "cargo-deny"),
    ),
    # The fetched samples. Each one reports a missing file, so it is only
    # meaningful once the download has run.
    # `--check` fetches only what the record cannot account for, so a working
    # copy that already has the samples runs it with no network at all.
    Check(
        "ofdrw-check",
        "samples",
        (py("scripts/sample_fetch.py", "ofdrw", "--check"),),
        needs=("ofdrw-samples",),
    ),
    Check(
        "ofdrw-parse",
        "samples",
        (py("scripts/sample_fetch.py", "ofdrw", "--parse"),),
        needs=("cargo", "ofdrw-samples"),
    ),
    Check(
        "gb-check",
        "samples",
        (py("scripts/sample_fetch.py", "gbt33190-samples", "--check"),),
        needs=("gb-samples",),
    ),
    Check(
        "gb-parse",
        "samples",
        (py("scripts/sample_fetch.py", "gbt33190-samples", "--parse"),),
        needs=("cargo", "gb-samples"),
    ),
    # The budget in docs/Performance.md. Two steps rather than one: the gate
    # reads `target/criterion`, so a cached reading would let it pass on code
    # that has since changed, and only an actual run rules that out. The run
    # also writes the log the memory readings are read from.
    Check(
        "bench",
        "perf",
        (py("scripts/run_benchmarks.py"),),
        needs=("cargo",),
    ),
    Check(
        "performance",
        "perf",
        (py("scripts/check_performance.py"),),
        needs=("bench",),
    ),
)

SAMPLE_DIRS = {
    "ofdrw-samples": ("tests/fixtures/ofdrw", "scripts/sample_fetch.py ofdrw"),
    "gb-samples": ("tests/fixtures/gbt33190-samples", "scripts/sample_fetch.py"),
}


def msrv_toolchain() -> str:
    """The `+toolchain` matching the manifest's `rust-version`, or empty.

    The manifest is the only place the minimum is stated, so it is read here
    rather than repeated in this script. Empty means no answer: no `rustup`, no
    `rust-version`, or no installed channel with that version.
    """
    matched = re.search(
        r'^rust-version\s*=\s*"([^"]+)"',
        (ROOT / "Cargo.toml").read_text(encoding="utf-8"),
        re.MULTILINE,
    )
    if matched is None:
        return ""
    listing = subprocess.run(
        ["rustup", "toolchain", "list"], capture_output=True, text=True,
    )
    if listing.returncode != 0:
        return ""
    for line in listing.stdout.splitlines():
        name = line.split()[0] if line.split() else ""
        # An installed channel carries the full version (`1.92.0`) while the
        # manifest names the minor (`1.92`), so only the leading two parts are
        # compared; a pinned nightly is never the minimum and is left out.
        channel = name.split("-")[0]
        if channel.count(".") == 2 and channel.rsplit(".", 1)[0] == matched.group(1):
            return f"+{name}"
    return ""


def precondition(name: str) -> tuple[bool, str]:
    """Whether this machine can run the check, and why not when it cannot."""
    # A cargo subcommand is a binary in its own right, so `cargo-deny` is asked
    # for by name rather than assumed to come with cargo.
    if name in ("cargo", "cargo-deny", "git"):
        if shutil.which(name) is None:
            hint = " (cargo install cargo-deny --locked)" if name == "cargo-deny" else ""
            return False, f"{name} is not on PATH{hint}"
        return True, ""
    if name == "lxml":
        if importlib.util.find_spec("lxml") is None:
            return False, "lxml is missing (python3 -m pip install lxml)"
        return True, ""
    if name == "bench":
        # A reading is only as good as the run that produced it, so this is a
        # dependency rather than a suggestion: a skipped `bench` skips the gate.
        if not (ROOT / "target" / "criterion").is_dir():
            return False, "no benchmark run (perf/bench produces one)"
        return True, ""
    if name == "fixture-sources":
        if not (ROOT / FIXTURE_FILES).is_dir():
            return False, f"{FIXTURE_FILES} is missing"
        return True, ""
    if name == "msrv":
        if msrv_toolchain():
            return True, ""
        return False, "no rustup channel matches rust-version (rustup toolchain install it)"
    directory, script = SAMPLE_DIRS[name]
    if not (ROOT / directory).is_dir():
        return False, f"{directory} is absent (run python3 {script})"
    return True, ""


def blocked(check: Check) -> str:
    """The first unmet precondition, or an empty string when nothing blocks."""
    for need in check.needs:
        ok, reason = precondition(need)
        if not ok:
            return reason
    return ""


def environment(check: Check) -> dict[str, str]:
    """The process environment, with this check's overrides applied."""
    env = dict(os.environ)
    for key in check.unset:
        env.pop(key, None)
    env.update(dict(check.env))
    return env


def run(check: Check, verbose: bool) -> tuple[bool, str, float]:
    """Run every step in order; return (passed, captured output, seconds)."""
    started = time.perf_counter()
    captured: list[str] = []
    passed = True
    for step in check.steps:
        command = list(step)
        if check.msrv and command[0] == "cargo":
            command.insert(1, msrv_toolchain())
        if verbose:
            print(f"    $ {' '.join(command)}", flush=True)
        done = subprocess.run(
            command,
            cwd=ROOT,
            env=environment(check),
            text=True,
            encoding="utf-8",
            errors="replace",
            capture_output=not verbose,
        )
        if not verbose:
            captured.append(done.stdout or "")
            captured.append(done.stderr or "")
        if done.returncode != 0:
            passed = False
            break
    return passed, "".join(captured), time.perf_counter() - started


def selected(checks: tuple[Check, ...], only: list[str], skip: list[str],
             quick: bool) -> list[Check]:
    """The checks the flags leave in, keeping the declared order."""
    chosen: list[Check] = []
    for check in checks:
        if quick and check.group in SLOW_GROUPS:
            continue
        if only and check.name not in only and check.group not in only:
            continue
        if check.name in skip or check.group in skip:
            continue
        chosen.append(check)
    return chosen


def describe(check: Check) -> str:
    """The command a check runs, for --list."""
    steps = [
        " ".join("python3" if arg == PYTHON else arg for arg in step)
        for step in check.steps
    ]
    return " && ".join(steps)


def main() -> int:
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument("--quick", action="store_true",
                        help="skip the groups that need cargo, a fetch or a bench")
    parser.add_argument("--list", action="store_true",
                        help="print the plan instead of running it")
    parser.add_argument("--only", action="append", default=[], metavar="NAME",
                        help="a check or group to run; repeatable")
    parser.add_argument("--skip", action="append", default=[], metavar="NAME",
                        help="a check or group to leave out; repeatable")
    parser.add_argument("--require-all", action="store_true",
                        help="treat a skipped check as a failure")
    parser.add_argument("--verbose", action="store_true",
                        help="stream each check's output as it runs")
    args = parser.parse_args()

    for stream in (sys.stdout, sys.stderr):
        if hasattr(stream, "reconfigure"):
            stream.reconfigure(errors="replace")

    chosen = selected(CHECKS, args.only, args.skip, args.quick)
    if not chosen:
        print("nothing selected; --list shows what is available")
        return 2

    plan: list[tuple[Check, str]] = [(c, blocked(c)) for c in chosen]

    if args.list:
        for group in GROUPS:
            entries = [(c, why) for c, why in plan if c.group == group]
            if not entries:
                continue
            print(f"{group}:")
            for check, why in entries:
                state = f"SKIP  {why}" if why else "run"
                print(f"  {check.name:<20} {state}")
                if not why:
                    print(f"  {'':<20} {describe(check)}")
        return 0

    failed: list[str] = []
    noticed: list[str] = []
    skipped: list[tuple[str, str]] = []
    passed: list[str] = []
    started = time.perf_counter()
    group = ""

    for check, why in plan:
        if check.group != group:
            group = check.group
            print(f"\n== {group} ==")
        label = f"{check.group}/{check.name}"
        if why:
            skipped.append((label, why))
            print(f"  {check.name:<20} skip    {why}")
            continue
        ok, output, elapsed = run(check, args.verbose)
        if ok:
            passed.append(label)
            print(f"  {check.name:<20} ok      {elapsed:5.1f}s")
            continue
        if check.group in ADVISORY_GROUPS:
            noticed.append(label)
            print(f"  {check.name:<20} note    {elapsed:5.1f}s")
        else:
            failed.append(label)
            print(f"  {check.name:<20} FAIL    {elapsed:5.1f}s")
        if not args.verbose and output.strip():
            for line in output.strip().splitlines()[-40:]:
                print(f"      {line}")

    total = time.perf_counter() - started
    print("\n== summary ==")
    print(f"  {len(passed)} passed, {len(failed)} failed, "
          f"{len(noticed)} noted, {len(skipped)} skipped in {total:.1f}s")

    if noticed:
        print("\n  noted, and worth a look rather than a block:")
        for label in noticed:
            print(f"    {label}")
    if skipped:
        print("\n  skipped, with the reason:")
        for label, why in skipped:
            print(f"    {label:<28} {why}")
    if failed:
        print("\n  failed:")
        for label in failed:
            print(f"    {label}")
        print("\nNothing here is advisory: a failed check blocks a merge.")
        return 1
    if args.require_all and skipped:
        print("\n--require-all: a skipped check counts as a failure.")
        return 1
    print("\nOK: every check that could run passed")
    return 0


if __name__ == "__main__":
    sys.exit(main())
