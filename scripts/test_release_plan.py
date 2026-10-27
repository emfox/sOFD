#!/usr/bin/env python3
"""Tests for release_plan.py's tag shape and its publish list.

The plan itself is what `--selftest` checks. What is tested here is the two
things `--selftest` cannot see: the tag, because the planner starts every run by
asking a tag what version it names, and the publish list, because a plan is a
list a registry has to accept from end to end.

A tag the planner cannot read is a tag it silently skips, so the next release
would be planned from the release before it — every crate shipping again at
versions the registry has already spent.

A plan that leaves a dependency out does not fail at planning time. `cargo
publish` cannot be undone and it stops at the first crate whose dependency the
registry does not serve yet, so the failure lands after the crates below it are
already spent.
"""

import pathlib
import sys
import tempfile
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

import release_plan as plan  # noqa: E402

# A workspace of four crates, each one level above the last: `d` under `c`,
# `c` under `b`, `b` under `a`. Small enough to say what the answer is.
DEPS = {
    "sofd-d": "",
    "sofd-c": 'sofd-d = { path = "../sofd-d", version = "0.5" }',
    "sofd-b": 'sofd-c = { path = "../sofd-c", version = "0.6" }',
    "sofd-a": 'sofd-b = { path = "../sofd-b", version = "0.5" }',
}
VERSIONS = {"sofd-a": "0.5.1", "sofd-b": "0.5.1", "sofd-c": "0.5.1", "sofd-d": "0.5.1"}


def workspace(root: pathlib.Path, versions: dict[str, str]) -> pathlib.Path:
    """Write the four crates under `root` and return the workspace root."""
    root.joinpath("Cargo.toml").write_text(
        '[workspace]\nmembers = ["crates/sofd-a", "crates/sofd-b", '
        '"crates/sofd-c", "crates/sofd-d"]\n\n[workspace.package]\n'
        'edition = "2024"\n',
        encoding="utf-8",
    )
    for name, version in versions.items():
        directory = root / "crates" / name
        directory.mkdir(parents=True)
        directory.joinpath("Cargo.toml").write_text(
            f'[package]\nname = "{name}"\nversion = "{version}"\n'
            f'edition = "2024"\n\n[dependencies]\n{DEPS[name]}\n',
            encoding="utf-8",
        )
    return root


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


class PublishListTest(unittest.TestCase):
    """Whether a plan is a list a registry accepts, not just a list."""

    def plan(self, versions, changed, served, resolves=None):
        with tempfile.TemporaryDirectory() as temporary:
            root = workspace(pathlib.Path(temporary), versions)
            return plan.build_plan(
                "v0.5.1",
                {},
                offline=False,
                root=root,
                changed=changed,
                served=served,
                resolves=resolves,
            )

    def test_a_dependency_outside_the_plan_is_refused(self):
        # `sofd-b` needs `sofd-c 0.6`, which neither the registry nor the plan
        # has. Publishing `sofd-b` would stop there, with `sofd-c` unpublishable
        # and `sofd-b` half-spent, so the plan refuses to be printed at all.
        versions = dict(VERSIONS, **{"sofd-b": "0.6.0"})
        _, faults = self.plan(versions, {"sofd-b"}, served=lambda crate, version: False)
        self.assertTrue(
            any("sofd-b ships needing sofd-c" in fault for fault in faults), faults
        )

    def test_a_dependency_the_registry_serves_needs_no_entry(self):
        # `sofd-a` needs `sofd-b 0.5`, which `sofd-b 0.5.1` on the registry
        # satisfies: shipping `sofd-a` alone is fine, and the plan has to say
        # so rather than dragging `sofd-b` along at a version nobody meant to
        # spend.
        found, faults = self.plan(
            VERSIONS,
            {"sofd-a"},
            served=lambda crate, version: crate == "sofd-b" and version == "0.5.1",
            resolves=lambda crate, requirement: crate == "sofd-b",
        )
        self.assertEqual([name for name, _, _ in found], ["sofd-a"], faults)
        self.assertEqual(faults, [])

    def test_a_version_the_manifest_states_is_the_version_it_ships(self):
        # The tree is the record. A crate whose manifest says `0.6.0` was given
        # that number by the commit that broke its API; bumping it again here
        # would publish `0.6.1` while the tag names `0.6.0`.
        versions = dict(VERSIONS, **{"sofd-b": "0.6.0"})
        found, _ = self.plan(versions, {"sofd-b"}, served=lambda crate, version: False)
        self.assertIn(("sofd-b", "0.6.0", "states 0.6.0 in its manifest"), found)

    def test_a_crate_still_inheriting_its_version_is_bumped(self):
        # The migration to per-crate versions is not finished everywhere. A
        # crate whose number is still the workspace's has not been given one
        # for this release, so the planner proposes the next patch.
        with tempfile.TemporaryDirectory() as temporary:
            root = pathlib.Path(temporary)
            workspace(root, VERSIONS)
            inherited = root / "crates" / "sofd-d" / "Cargo.toml"
            inherited.write_text(
                inherited.read_text(encoding="utf-8").replace(
                    'version = "0.5.1"', "version.workspace = true"
                ),
                encoding="utf-8",
            )
            root.joinpath("Cargo.toml").write_text(
                root.joinpath("Cargo.toml")
                .read_text(encoding="utf-8")
                .replace("edition = \"2024\"\n", "edition = \"2024\"\nversion = \"0.5.1\"\n"),
                encoding="utf-8",
            )
            found, faults = plan.build_plan(
                "v0.5.1",
                {},
                offline=True,
                root=root,
                changed={"sofd-d"},
            )
        self.assertEqual(faults, [])
        self.assertEqual([version for _, version, _ in found], ["0.5.2"])


if __name__ == "__main__":
    unittest.main()
