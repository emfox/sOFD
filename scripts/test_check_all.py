#!/usr/bin/env python3
"""Tests for check_all.py.

Only `selected` is exercised. Running the gates themselves is what the script is
for, and a test that ran them would be the gate calling itself. What `selected`
decides is worth pinning down, because the flags interact: `--fast` leaves two
checks out, and naming one of them has to bring it back, or a person who typed
`--fast --only bench` would sit waiting for a bench that never started.
"""

import pathlib
import sys
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

import check_all as gate  # noqa: E402


def names(checks: tuple[gate.Check, ...]) -> list[str]:
    chosen = gate.selected(checks, [], [], quick=False)
    return [check.name for check in chosen]


class FastTest(unittest.TestCase):
    def test_fast_leaves_out_the_slowest_checks(self):
        chosen = gate.selected(gate.CHECKS, [], [], quick=False, fast=True)
        left_out = {check.name for check in gate.CHECKS} - {
            check.name for check in chosen
        }
        self.assertEqual(left_out, set(gate.FAST_SKIPS))

    def test_every_check_fast_leaves_out_is_one_it_names_a_reason_for(self):
        declared = {check.name for check in gate.CHECKS}
        self.assertEqual(set(gate.FAST_SKIPS), {"bench", "test-all-features"})
        self.assertTrue(set(gate.FAST_SKIPS) <= declared)

    def test_naming_one_of_them_brings_it_back(self):
        chosen = gate.selected(gate.CHECKS, ["bench"], [], quick=False, fast=True)
        self.assertEqual([check.name for check in chosen], ["bench"])

    def test_without_fast_every_check_is_offered(self):
        self.assertEqual(len(names(gate.CHECKS)), len(gate.CHECKS))


class SelectionTest(unittest.TestCase):
    def test_only_matches_a_name_or_a_group(self):
        by_group = gate.selected(gate.CHECKS, ["policy"], [], quick=False)
        self.assertTrue(by_group)
        self.assertTrue(all(check.group == "policy" for check in by_group))

    def test_quick_keeps_only_the_cheap_groups(self):
        chosen = gate.selected(gate.CHECKS, [], [], quick=True)
        self.assertTrue(all(check.group not in gate.SLOW_GROUPS for check in chosen))

    def test_skip_wins_over_the_declared_order(self):
        chosen = gate.selected(gate.CHECKS, [], ["rust"], quick=False)
        self.assertTrue(all(check.group != "rust" for check in chosen))


if __name__ == "__main__":
    unittest.main()
