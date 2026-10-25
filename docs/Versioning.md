# Per-crate versions

A proposal, not a decision. It states the problem, what independence would
actually buy, what it costs, and the order the work would go in. Nothing here
is scheduled.

## 1. The problem with one version

Every crate in the workspace states the same version, through
`[workspace.package] version` and twenty-one `sofd-* = { version = "0.5.1" }`
entries under `[workspace.dependencies]`. The constraint is cargo's, not a
style choice: a `path` dependency has to satisfy its own `version` field, so a
break at the bottom of the tree makes the requirement unsatisfiable for every
crate above it, and all of them have to move together.

The cost is that a release is all-or-nothing. The workspace has a very uneven
dependency graph:

| Crate | Crates it depends on | Crates that depend on it |
| --- | ---: | ---: |
| `sofd-error` | 0 | 22 |
| `sofd-common` | 0 | 14 |
| `sofd-font` | 2 | 10 |
| `sofd-geometry` | 1 | 8 |
| `sofd-display` | 4 | 7 |
| `sofd-io`, `sofd-package` | 2 | 6 |
| `sofd` | 17 | 5 |
| `sofd-cli` | 7 | 0 |
| `sofd-archive`, `sofd-search`, `sofd-edit`, `sofd-parser` | 1–9 | 1 |

`sofd-error` is a leaf that nothing here changes often, and it is the crate a
consumer is most likely to see in `cargo tree`. A typo fix in its documentation
spends a version of all twenty-one crates, and the changelog for a release
that touched one paragraph lists twenty-one entries. That is the annoyance the
uniform scheme creates, and it is real.

## 2. What independence buys, precisely

Less than "each crate does what it likes". Three things:

1. **A release spends only the versions it needs.** Change `sofd-search` and
   ship `sofd-search 0.5.2` plus whatever depends on it, and nothing else.
2. **The changelog says what changed.** One crate's entries, not twenty-one
   repetitions of "version bumped to match the workspace".
3. **A break stays local.** `sofd-display` can go `0.6.0` while `sofd-font`
   stays `0.5.3`, provided `sofd-display` is not in `sofd-font`'s closure. The
   publish order already computes that closure.

What it does not buy: a crate whose dependency broke still has to move, because
its `version` requirement named the old one. Independence reduces the blast
radius, it does not remove the requirement.

## 3. What it costs

### 3.1 Version requirements become a thing to get right

Today every internal dependency is `{ path = ..., version = "0.5.1" }`, written
once in the root. Independent versions mean each manifest states an interval —
`sofd-display = "0.6"` — and the correctness question changes from "is the
workspace consistent" to "does this interval name a version that exists, and
will it still resolve".

That is a new failure mode, and it is worse than the current one because it
appears only to a consumer installing from the registry. It needs a check.

### 3.2 The publish order gains a second constraint

`scripts/publish_order.py` currently answers "in what order does a registry
accept these". With independent versions it has to answer the same question
for a *subset*: this release ships `sofd-search 0.5.2`, so which crates have to
go with it, and at which versions.

### 3.3 The semver gate stops being one query

`docs/Conventions.md` §6 asks the crates.io API about `sofd-cli`, which
publishes last and therefore stands for the whole set. Independent versions
mean twenty-one queries, and the answer for each crate is about that crate's
own published version rather than the workspace's. This is closer to what
`cargo-semver-checks` does natively, so it is the cheapest of the three costs.

### 3.4 The registry has no undo

A version requirement that turns out wrong cannot be edited. `sofd 0.5.2` that
says `sofd-display = "0.6"` when the real answer was `"0.5"` has to be
superseded by `0.5.3`. Yanks help a consumer notice, not fix the manifest. The
first few releases after such a change deserve care that the current scheme
does not need.

## 4. What it would look like

### 4.1 The root keeps the policy, not the number

```toml
[workspace.package]
# No `version` here any more: each crate states its own.
edition = "2024"
rust-version = "1.92"
```

Each published manifest gains one line:

```toml
[package]
version = "0.6.0"          # was `version.workspace = true`
```

### 4.2 Internal dependencies state a real interval

`[workspace.dependencies]` stops being the only place an internal version is
written; each crate states the interval it actually needs.

```toml
# crates/sofd/Cargo.toml
sofd-display = "0.6"       # not "0.5.1"
sofd-error   = "0.5"       # unchanged across the display break
```

One rule keeps this from drifting: **an internal dependency states the lowest
version that compiles against this tree**, and `scripts/check_versions.py`
proves it by resolving each crate's manifest against the registry.

### 4.3 The release names what it ships

A release becomes a list — the crates whose code changed, plus the closure
that has to move because their requirements did:

```bash
python3 scripts/release_plan.py --since v0.5.1
# sofd-display 0.6.0    API changed
# sofd-font    0.5.2    requires sofd-display 0.6
# sofd         0.5.2    requires sofd-display 0.6
# ...
```

## 5. The order the work went in

Each step was a merge that left the tree working, so the change could stop after
any of them. All six are done.

| # | Step | Why there |
| --- | --- | --- |
| 1 | `scripts/check_versions.py` — every internal requirement resolves against crates.io | The safety net goes in before the thing it catches. It passed trivially while the workspace was uniform, which was the point: proven before it was needed |
| 2 | `scripts/release_plan.py` — which crates a change set has to ship | The release stops being "everything" before the manifests stop being uniform |
| 3 | Move `version` from `[workspace.package]` into each of the twenty-one published manifests | The mechanical change. Nothing observed it yet |
| 4 | Replace the `[workspace.dependencies]` version entries with per-crate intervals | The step that could go wrong. Step 1 is what says when it did |
| 5 | Semver gate asks per crate instead of asking about `sofd-cli` | Cheap, and closed the gap the other four opened |
| 6 | `docs/Conventions.md` §6 and `README.md` state the new rule | Documentation last, once the rule had stopped moving |

### 5.1 The tag, because a version can no longer name a release

A tag used to be `v<version>`, and that worked because there was one version.
With a version per crate it does not: `v0.5.2` would name `sofd-next`, which is
exactly why `release_plan.py` emits `cargo publish -p …` lines instead of a
single command. So a tag names the **release**:

```
v0.6.1     release 0, version 0.6.1
```

The extra number counts releases of that version line, and the last three are
what the artefacts are named for — `sofd-0.6.1-linux.tar.gz` whatever versions
the crates inside state. Two consequences worth stating:

* The root manifest carries the prefix as
  `[workspace.metadata.cargo-metadata.package] release = "z"`, so the tag shape
  is in the tree rather than in a workflow pattern. `release_plan.py --since
  @last` asks the registry which tag is the newest it serves, which is what makes
  "changes since the last release" answerable without remembering a tag.
* Every tag before this one stays as it is. `v0.5.1` reads as release `0`,
  version `0.5.1`, so the rule has one shape from `v0.1.0` onward.

## 6. When it was done

After `0.5.1`, the first release on a registry, and before `0.5.2`.

The plan originally said to wait for a second release, on the grounds that the
benefit was unmeasured and the uniform scheme has one number to get right
instead of twenty-one. It was done earlier for a reason that outranked both:
`0.5.1` is the release everything published so far hangs off, and every later
release would either have to carry every crate's version forward or undo the
uniform scheme with a registry full of published versions to reconcile. Doing it
while the tree was uniform meant steps 3 and 4 were mechanical, and step 1 could
prove the result against twenty-one crates already on the registry.

## 7. The alternative, if 0.5.2 shows the cost is too high

Keep one version, and solve the annoyance where it actually shows up: the
changelog. A release entry can say which crates moved and which were carried
along for the version alone, so a reader is not asked to read twenty-one
entries to find the two that changed.

Going back is cheap for the same reason going forward was: one `version` key in
`[workspace.package]`, `version.workspace = true` in each manifest, and the
intervals in the dependency tables collapse back to one number. The two scripts
are the part worth keeping either way — `check_versions.py` would still catch a
requirement that does not admit its dependency, and `release_plan.py` would
still say which crates a change set touches.
