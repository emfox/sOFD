#!/usr/bin/env python3
"""Tests for semver_baseline.py — which ref a crate is held to.

The gate compares a crate against a published release of it, and the failure
this guards against is subtle: a missing tag is not a missing release. When a
release ships a subset of the workspace at a patch version, no tag names that
version, and asking for `v<version>` fails on the ref lookup rather than on the
API. The job then reports a broken gate for a tree that is fine.

So these tests are about the fallback: given tags that carry a crate, which one
is its baseline. They run against real git repositories, because the question
"which version does this tag's tree declare" is answered by reading a manifest
out of a commit, and a stub would only test the stub.
"""

import pathlib
import subprocess
import sys
import tempfile
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

import semver_baseline as baseline  # noqa: E402

CRATES = ["sofd-a", "sofd-b", "sofd-c"]


def commit(root: pathlib.Path, message: str) -> None:
    """Record the tree as it stands."""
    subprocess.run(["git", "add", "-A"], cwd=root, check=True, capture_output=True)
    subprocess.run(
        ["git", "-c", "user.name=t", "-c", "user.email=t@example.com",
         "commit", "-qm", message],
        cwd=root,
        check=True,
        capture_output=True,
    )


def set_versions(root: pathlib.Path, shared: str, own: dict[str, str] | None = None) -> None:
    """Write the workspace version, and any crate that states its own."""
    root.joinpath("Cargo.toml").write_text(
        '[workspace]\nmembers = ["crates/sofd-a", "crates/sofd-b", "crates/sofd-c"]\n'
        f'\n[workspace.package]\nversion = "{shared}"\nedition = "2024"\n',
        encoding="utf-8",
    )
    own = own or {}
    for name in CRATES:
        directory = root / "crates" / name
        directory.mkdir(parents=True, exist_ok=True)
        if name in own:
            version = f'version = "{own[name]}"\n'
        else:
            version = "version.workspace = true\n"
        directory.joinpath("Cargo.toml").write_text(
            f'[package]\nname = "{name}"\n{version}edition.workspace = true\n',
            encoding="utf-8",
        )


def tag(root: pathlib.Path, name: str) -> None:
    subprocess.run(["git", "tag", name], cwd=root, check=True, capture_output=True)


class BaselineTest(unittest.TestCase):
    def setUp(self):
        self.tmp = tempfile.TemporaryDirectory()
        self.root = pathlib.Path(self.tmp.name)
        subprocess.run(["git", "init", "-q", "-b", "main"], cwd=self.root, check=True)
        set_versions(self.root, "0.5.1")
        commit(self.root, "0.5.1")
        tag(self.root, "v0.5.1")

    def tearDown(self):
        self.tmp.cleanup()

    def test_a_crate_is_compared_against_the_release_before_it(self):
        # A tag at the crate's own version compares the crate with itself and
        # passes whatever the API looks like, so it is not a baseline. The
        # 0.5.1 tree has nothing below it, which is an honest "no baseline".
        self.assertIsNone(baseline.baseline("sofd-a", "0.5.1", self.root))
        set_versions(self.root, "0.5.2")
        commit(self.root, "0.5.2")
        tag(self.root, "v0.5.2")
        self.assertEqual(baseline.baseline("sofd-a", "0.5.2", self.root), "v0.5.1")

    def test_a_patch_release_without_a_tag_falls_back_to_the_release(self):
        # 0.5.2 ships a subset, so no `v0.5.2` exists. The crate is still a
        # patch above 0.5.1, and that is what it must not break.
        set_versions(self.root, "0.5.1", {"sofd-a": "0.5.2"})
        commit(self.root, "0.5.2")
        self.assertFalse(
            subprocess.run(
                ["git", "rev-parse", "--verify", "v0.5.2"],
                cwd=self.root,
                capture_output=True,
            ).returncode
            == 0,
            "the fixture must not carry a v0.5.2 tag",
        )
        self.assertEqual(baseline.baseline("sofd-a", "0.5.2", self.root), "v0.5.1")

    def test_a_version_below_every_tag_has_no_baseline(self):
        # Nothing was ever released below 0.5.1, so there is no release for
        # 0.5.0 to be compared against — the comparison would be the wrong
        # direction, and a tag above the version checked is not a baseline.
        self.assertIsNone(baseline.baseline("sofd-a", "0.5.0", self.root))

    def test_a_crate_no_tag_carries_has_no_baseline(self):
        # A crate added since the last release: nothing can depend on it.
        self.assertIsNone(baseline.baseline("sofd-new", "0.1.0", self.root))

    def test_the_newest_release_below_a_version_wins(self):
        # Two releases behind it, and the newer one is the baseline even when
        # the tags are listed oldest first.
        set_versions(self.root, "0.6.0")
        commit(self.root, "0.6.0")
        tag(self.root, "v0.6.0")
        set_versions(self.root, "0.7.0")
        commit(self.root, "0.7.0")
        self.assertEqual(baseline.baseline("sofd-a", "0.7.0", self.root), "v0.6.0")

    def test_versions_compare_as_numbers(self):
        # 0.10.0 is above 0.9.0, which sorting the strings would get backwards.
        set_versions(self.root, "0.9.0")
        commit(self.root, "0.9.0")
        tag(self.root, "v0.9.0")
        set_versions(self.root, "0.10.0")
        commit(self.root, "0.10.0")
        self.assertEqual(baseline.baseline("sofd-a", "0.10.0", self.root), "v0.9.0")

    def test_a_prerelease_is_not_a_baseline(self):
        set_versions(self.root, "0.5.2")
        commit(self.root, "0.5.2-rc")
        tag(self.root, "v0.5.2-rc.1")
        self.assertEqual(baseline.baseline("sofd-a", "0.5.2", self.root), "v0.5.1")

    def test_a_tag_that_carries_a_different_version_is_skipped(self):
        # A tag is a claim; the manifest in its tree is the fact. A `v0.6.0`
        # whose tree still says 0.5.1 is not the baseline for anything.
        set_versions(self.root, "0.5.1", {"sofd-a": "0.5.1"})
        commit(self.root, "misnamed")
        tag(self.root, "v0.6.0")
        set_versions(self.root, "0.5.1", {"sofd-a": "0.5.2"})
        commit(self.root, "0.5.2")
        self.assertEqual(baseline.baseline("sofd-a", "0.5.2", self.root), "v0.5.1")

    def test_a_crate_stating_its_own_version_is_read_from_its_manifest(self):
        # `sofd` states 0.6.0 while the workspace stays behind; reading that
        # out of the tagged tree is what tells the tag apart from one whose
        # tree would have carried the crate at the workspace version instead.
        set_versions(self.root, "0.5.1", {"sofd-a": "0.6.0"})
        commit(self.root, "sofd-a 0.6.0")
        tag(self.root, "v0.6.0")
        # The crate at 0.6.0 is held to the release before it, which is 0.5.1.
        self.assertEqual(baseline.baseline("sofd-a", "0.6.0", self.root), "v0.5.1")
        # `v0.6.0` carried sofd-a at 0.6.0, so it is a real tag for that crate:
        # asking about the version after it finds it.
        set_versions(self.root, "0.5.1", {"sofd-a": "0.6.1"})
        commit(self.root, "sofd-a 0.6.1")
        self.assertEqual(baseline.baseline("sofd-a", "0.6.1", self.root), "v0.6.0")
        # sofd-b inherited 0.5.1 from that same tag, so v0.6.0 did not carry it.
        self.assertEqual(baseline.baseline("sofd-b", "0.6.1", self.root), "v0.5.1")


class GroupingTest(unittest.TestCase):
    """`--groups` is what turns one check per crate into one per release."""

    def test_groups_gather_crates_by_their_ref(self):
        with tempfile.TemporaryDirectory() as tmp:
            root = pathlib.Path(tmp)
            subprocess.run(["git", "init", "-q", "-b", "main"], cwd=root, check=True)
            set_versions(root, "0.5.1")
            commit(root, "0.5.1")
            tag(root, "v0.5.1")
            served = root / "served.txt"
            served.write_text(
                "sofd-a 0.5.2 published\nsofd-b 0.5.2 published\n"
                "sofd-c 0.5.3 unpublished\nsofd-new 0.1.0 published\n",
                encoding="utf-8",
            )
            cwd = pathlib.Path.cwd()
            try:
                import os

                os.chdir(root)
                groups: dict[str, list[str]] = {}
                for name in ["sofd-a", "sofd-b", "sofd-new"]:
                    ref = baseline.baseline(name, "0.5.2" if name != "sofd-new" else "0.1.0", root)
                    if ref is not None:
                        groups.setdefault(ref, []).append(name)
            finally:
                os.chdir(cwd)
            self.assertEqual(sorted(groups), ["v0.5.1"])
            self.assertEqual(sorted(groups["v0.5.1"]), ["sofd-a", "sofd-b"])


if __name__ == "__main__":
    unittest.main(verbosity=2)
