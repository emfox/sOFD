#!/usr/bin/env python3
"""Tests for release_plan.py's tag shape.

The plan itself is what `--selftest` checks. What is tested here is the tag,
because the planner starts every run by asking a tag what version it names: a
tag it cannot read is a tag it silently skips, so the next release would be
planned from the release before it — every crate shipping again at versions the
registry has already spent.
"""

import pathlib
import sys
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

import release_plan as plan  # noqa: E402


class TagShapeTest(unittest.TestCase):
    def test_a_tag_is_the_version(self):
        self.assertEqual(plan.release_tag("0.6.0"), "v0.6.0")

    def test_a_tag_this_project_writes_reads_back_as_its_version(self):
        self.assertEqual(plan.named_version(plan.release_tag("0.6.1")), "0.6.1")

    def test_the_version_is_still_at_the_tail_of_the_name(self):
        # The release workflow names every artefact by dropping everything
        # before the first dot, so the number a user reads has to be the last
        # thing on the tag. `v0.5.1` has always read as `5.1` there; keeping
        # the tag a bare version is what keeps that from changing.
        tag = plan.release_tag("0.6.0")
        version = tag[tag.index("v") + 1 :]
        self.assertEqual(version[version.index(".") + 1 :], "6.0")

    def test_a_tag_that_names_no_version_is_skipped(self):
        # `v0.1.0-rc.1` is a prerelease this project cut, and nothing is
        # compared against it; the backup tags are not releases at all.
        self.assertIsNone(plan.named_version("v0.1.0-rc.1"))
        self.assertIsNone(plan.named_version("v0.6.0z"))
        self.assertIsNone(plan.named_version("backup-pre-rewrite-20261026"))

    def test_the_semver_gate_can_still_find_a_crate_version(self):
        # `.github/workflows/rust.yml` compares a crate against `v<version>`,
        # so a tag that is not the version leaves that comparison pointing at
        # a revision that does not exist.
        self.assertEqual(f"v{'0.6.0'}", plan.release_tag("0.6.0"))


if __name__ == "__main__":
    unittest.main()
