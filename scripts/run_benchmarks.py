#!/usr/bin/env python3
"""Run the budgeted benchmarks and keep what they printed.

`cargo bench` writes its timings to `target/criterion/` and tees its output to
the terminal, where nothing reads it back. `check_performance.py` needs both:
the estimates give the timings, and a log gives the memory readings, which are
printouts rather than durations.

Two things this script does that a bare `cargo bench` does not. It writes the
combined output to `target/criterion/benchmarks.log`, so the gate can read a
memory reading that was printed a moment ago. And it clears the readings log
first, because that file is appended to on every run: without the reset a
benchmark that stopped printing one would still be gated on an old number.

The three benches are the ones the budget names. Running every benchmark in the
workspace would spend a runner's afternoon on numbers nothing gates on.
"""

from __future__ import annotations

import subprocess
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent
CRITERION = REPO / "target" / "criterion"
LOG = CRITERION / "benchmarks.log"
READINGS = CRITERION / "memory-readings.log"

# Crate and bench target, in the order the gate expects to find them.
BENCHES = (
    ("sofd-parser", "parse"),
    ("sofd", "memory"),
    ("sofd-convert", "convert"),
)


def main() -> int:
    CRITERION.mkdir(parents=True, exist_ok=True)
    for stale in (LOG, READINGS):
        stale.unlink(missing_ok=True)

    printed: list[str] = []
    for package, bench in BENCHES:
        command = ["cargo", "bench", "-p", package, "--bench", bench]
        print(f"== {package} --bench {bench} ==", flush=True)
        done = subprocess.run(
            command,
            cwd=REPO,
            text=True,
            encoding="utf-8",
            errors="replace",
            stdout=subprocess.PIPE,
            stderr=subprocess.STDOUT,
        )
        printed.append(done.stdout or "")
        # The bench prints its own progress; showing it live is what makes a
        # fourteen-minute gate look alive rather than hung.
        sys.stdout.write(done.stdout or "")
        sys.stdout.flush()
        if done.returncode != 0:
            LOG.write_text("".join(printed), encoding="utf-8", errors="replace")
            print(f"\nFAILED: {package} --bench {bench} exited {done.returncode}")
            return done.returncode or 1

    LOG.write_text("".join(printed), encoding="utf-8", errors="replace")
    print(f"\nwrote {LOG.relative_to(REPO)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
