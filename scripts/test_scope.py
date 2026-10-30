#!/usr/bin/env python3
"""Pick which crates a change touches, so tests can be scoped instead of full.

A full `cargo test --workspace` spends most of its time rebuilding and
re-running crates the change cannot reach. This script maps changed files to
their crate, walks the reverse-dependency graph, and prints or runs the tests
for just that closure.

Tiers:
  crate     the crates that own the changed files only;
  affected  the crates plus everything downstream that depends on them
            (the default, and what a PR should run);
  full      the whole workspace, for releases and changes to shared roots.

Usage:
  test_scope.py --tier affected            # diff against HEAD (working tree)
  test_scope.py --tier affected --base main
  test_scope.py --tier affected --run      # actually run cargo test
"""

import argparse
import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent


def changed_files(base: str | None) -> list[str]:
    if base:
        out = subprocess.run(
            ["git", "diff", "--name-only", f"{base}...HEAD"],
            cwd=ROOT, capture_output=True, text=True, check=True,
        )
    else:
        out = subprocess.run(
            ["git", "diff", "--name-only", "HEAD"],
            cwd=ROOT, capture_output=True, text=True, check=True,
        )
    files = out.stdout.split()
    # Untracked files are real changes too, and `git diff` misses them.
    untracked = subprocess.run(
        ["git", "ls-files", "--others", "--exclude-standard"],
        cwd=ROOT, capture_output=True, text=True, check=True,
    )
    return files + untracked.stdout.split()


def crate_of(path: str) -> str | None:
    parts = Path(path).parts
    if len(parts) >= 2 and parts[0] == "crates":
        return parts[1]
    return None


def load_reverse_deps() -> dict[str, set[str]]:
    out = subprocess.run(
        ["cargo", "metadata", "--format-version", "1", "--no-deps", "--offline"],
        cwd=ROOT, capture_output=True, text=True, check=True,
    )
    meta = json.loads(out.stdout)
    members = set(meta["workspace_members"])
    rdeps: dict[str, set[str]] = {}
    for pkg in meta["packages"]:
        if pkg["id"] not in members:
            continue
        for dep in pkg["dependencies"]:
            if dep.get("source") is not None:
                continue
            rdeps.setdefault(dep["name"], set()).add(pkg["name"])
    return rdeps


def closure(seeds: set[str], rdeps: dict[str, set[str]]) -> set[str]:
    seen: set[str] = set()
    stack = list(seeds)
    while stack:
        crate = stack.pop()
        if crate in seen:
            continue
        seen.add(crate)
        stack.extend(rdeps.get(crate, ()))
    return seen


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--tier", choices=["crate", "affected", "full"], default="affected")
    ap.add_argument("--base", help="git ref to diff against (default: working tree)")
    ap.add_argument("--run", action="store_true", help="run cargo test, not just print")
    args = ap.parse_args()

    files = changed_files(args.base)
    seeds = {c for f in files if (c := crate_of(f))}
    if args.tier == "full" or not seeds:
        crates = None
    else:
        rdeps = load_reverse_deps()
        crates = closure(seeds, rdeps)
        if args.tier == "crate":
            crates = seeds

    names = " ".join(sorted(crates)) if crates else "--workspace"
    reason = "workspace (shared root or no crate mapping)" if crates is None else "affected closure"
    print(f"# seeds: {', '.join(sorted(seeds)) or '(none)'}")
    print(f"# tier: {args.tier} -> {reason}")
    print(f"cargo test -q -p {names}" if crates else "cargo test -q --workspace")

    if args.run:
        cmd = ["cargo", "test", "-q"] + (
            sum([["-p", c] for c in sorted(crates)], []) if crates else ["--workspace"]
        )
        return subprocess.run(cmd, cwd=ROOT).returncode
    return 0


if __name__ == "__main__":
    sys.exit(main())
