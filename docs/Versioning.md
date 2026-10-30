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
# sofd-display 0.6.0     states 0.6.0 in its manifest
# sofd-font    0.5.2     states 0.5.2 in its manifest
# sofd         0.6.0     needs sofd-crypto 0.6.0
# ...
```

The version beside a crate is the one its manifest states, not one the planner
invents. A crate's number is decided by the commit that changed it, which is the
commit that knew whether the change was a feature or a fix; a planner working
from a diff only knows that something moved, and a patch bump guessed from a
diff is wrong for a break. The planner's job is the part the diff can answer:
which crates have to ship, and in what order.

Two refusals follow from that, both of them about a release that fails after it
has already spent versions:

* A crate the registry already serves cannot ship again. The message names the
  crate and says to give it a free version.
* A crate whose dependency interval admits no version the registry has, and
  that the plan does not carry an entry for, stops the plan. Publishing it would
  have `cargo publish` stop at that crate — with everything below it already
  published, which a registry has no undo for.

`scripts/check_versions.py` states the same fault from the other side. There it
is a note rather than a failure, because a crate waiting on a dependency the
tree is about to publish is not wrong, it is early; `release_plan.py` is where
"early" becomes "cannot go out yet".

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

### 5.1 The tag stays a version

A tag used to be `v<version>`, and it still is: `v0.6.0`, then `v0.6.1`. What it
names is the **release** — the products, not any one of the twenty-one crates,
which no longer agree on a version — but it spells that as a plain version,
because two things read it that way and neither can be changed for free:

* The release workflow names every artefact by dropping everything before the
  first dot of the tag, so `v0.6.0` gives `sofd-0.6.0-linux.tar.gz` and
  `sofd-reader-0.6.0-android.apk`. That is the number a user compares against
  `sOFD --version`, and the number the phone's package installer shows.
* People read tags. `v0.6.0` is what a user types when they want the release
  that shipped 0.6.0.

A third reader used to be the semver gate, on the theory that it compares a
crate against `v<its version>`. It does not, and holding it to that tag would
have been useless: a crate compared with the tag that carries its own version
is compared with itself, and `cargo-semver-checks` reports `no change` however
the API looks. The gate holds a crate to the newest tag *below* it that carried
it, which `scripts/semver_baseline.py` works out by reading each crate's
manifest out of the tagged tree. So the tag being a release number rather than
a crate version costs the gate nothing.

So there is **one number to choose per release**, not two. It is the version the
products are known by, and it is stated where a version is always stated — in
the manifests. The rule for choosing it:

| The release… | Version | Tag |
| --- | --- | --- |
| changes what a user sees or can do — a feature, a break, a new crate | the highest major.minor the crates inside reached | `v0.6.0` |
| only fixes behaviour, and every crate inside stays on its patch numbers | the next patch of the line the products are on | `v0.5.2` |

`release_plan.py --since v0.5.1 --tag 0.6.0` prints the tag to push, so the
spelling lives in one function (`release_tag`) rather than in a shell command
somebody retypes each release. `--since @last` asks the registry which tag is the
newest it serves, which is what makes "changes since the last release"
answerable without remembering a tag.

The two rows are chosen by the change, and the crates' own versions follow the
same reading — how much each one changed. `v0.7.0` is the worked example: it
split `sofd-egui` in two and added `sofd-icons`, which is a feature a user sees,
so the release is the highest line the crates inside reached, and inside it
`sofd`, `sofd-session` and `sofd-server` are `0.7.0`, `sofd-render` and
`sofd-convert` are `0.6.0` for a `Shape` field and a public path, and the crates
that only had to name a new interval are `0.5.3`.

A crate that is `publish = false` still has a version, and it still matters:
`sofd-egui`'s is what `sOFD --version` prints. So the bump is written into every
manifest rather than only the ones `release_plan.py` lists, and `Cargo.lock` is
regenerated in the same commit so the interval and the resolved version cannot
disagree in a tree that was merely committed and not built.

`docs/Versioning.md` used to propose a letter in the tag (`v0.6.0z`) to separate
the release from any crate's version. It was withdrawn: the letter had to be
read by the workflow, the gate and every user, and it bought nothing the rule
above does not — the crates' own versions are in their manifests, and a release
that ships them does not need a second number to say so.

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
