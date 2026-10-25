# Documentation and Language Policy

sOFD is an international open-source project. To keep the codebase consistent
and reviewable by contributors worldwide, all engineering artifacts use English.

## 1. Language rules

| Scope | Language | Notes |
| --- | --- | --- |
| Rust/Python/C source: identifiers, comments, docs | English only | No CJK anywhere in `.rs`, `.py`, `.toml`, `.c`, `.h`, `.js`, `.ts` |
| Commit messages, branch names, PR titles and descriptions | English only | Conventional Commits format |
| Code review comments, issue titles and bodies | English only | |
| File names, directory names, config keys | English only | Except fetched test samples, which keep the name their producer gave them ([4.1](#41-exemptions-from-the-language-policy)) |
| Document names | Sentence case: `Architecture.md`, not `ARCHITECTURE.md` | `README.md` and `CHANGELOG.md` keep their names, which are the ones every repository uses ([1.1](#11-document-names)) |
| `README` | Bilingual | `README.md` (English, default) + `README.zh-CN.md`, cross-linked |
| `CHANGELOG` | English only | Release notes describe the product, not the people reading it; the README is the only bilingual document |
| Other project documents (`docs/`) | English preferred | Chinese allowed only when a document is explicitly user-facing, and must be named `*.zh-CN.md` |
| UI strings shown to end users | Any locale | Must live in the `locales/<tag>/*.ftl` of the crate that embeds them, never hardcoded |

### 1.1 Document names

A document is named in sentence case: `Conventions.md`, `Ui-spec.md`, not
`CONVENTIONS.md`. All-caps was once a way to shout a file to the front of a
listing, and it survives on habit rather than on a rule any style guide states.
`README.md` and `CHANGELOG.md` keep their names, which are the same in every
repository and are the one convention a reader arrives with; everything under
`docs/` is renamed, since those names are this project's own. A compound is
hyphenated with only its first word capitalised, and a translation keeps the
`.zh-CN.md` suffix section 1 asks for.

### User-facing text is never hardcoded

Engine and business layers must not contain literal user-facing text. See
[docs/Architecture.md](../docs/Architecture.md) for the engine-layer-zero-text
rule and the error-code plus parameters pattern.

## 2. Comment density

Comments explain *why*, not *what*. If the code already says what it does, the
comment is noise.

| Position | Limit |
| --- | --- |
| Inline comment inside a function body | 1 line; 2 lines only when genuinely needed |
| Item doc comment (`///`, `//!`) on a struct, enum, field, variant, or function | 6 lines maximum; see below |
| Module-level header (`//!` at top of `lib.rs` or a large module) | 3 lines maximum |
| Trailing comment on one line | 1 line |

Doc comments get the wider limit because they are published: rustdoc shows
them, and cbindgen writes them into `crates/sofd-ffi/include/sofd.h`, so
a contract a C caller has to honour has no other home than the `///` above the
type it describes. The wider limit does not extend to a `//` line, so an
implementation note in the middle of a doc block still fails.

Hard rules:

- No comment block longer than 3 lines, except a block of doc comments only,
  which stops at 6.
- Do not restate the function signature in prose.
- Do not keep commented-out code; delete it, git remembers.
- Do not write changelog or history notes in comments; that belongs in commits.
- Prefer a clear name over a comment explaining a poor one.

Long-form design rationale belongs in `docs/`, not in source comments.

## 3. Commit messages

```
<type>(<scope>): <subject in English, imperative, no trailing period>
```

One line is the norm, and the subject is capped at **75 bytes** so it stays
one line in a terminal and in `git log`. A genuinely significant change may
add a short body of at most two lines explaining why. Anything longer belongs
in `docs/`.

```text
<type>(<scope>): <subject>

<why, at most two lines, only for significant changes>
```

Allowed types: `feat`, `fix`, `docs`, `chore`, `refactor`, `perf`, `test`, `build`, `ci`.

`<scope>` is normally the crate name without the `sofd-` prefix, for example
`parser`, `display`, `font`, `i18n`, `cli`.

The subject alone should be enough to understand the change. Do not list every
file touched, do not restate the diff, and do not write a changelog entry.

### 3.1 One logical change per commit

A commit carries **one** logical change: a new behaviour, a new test, or a fix.
Not two, and not a fix tangled with the test that proves it is a fix.

Commit as the change is finished. Do not accumulate work in the tree and split
it afterwards: a split made from memory separates hunks that belong together
and joins hunks that do not, and neither failure is visible in the diff. The
history is written while the reason for each hunk is still known, or it is
written badly.

What counts as one change:

| Together, one commit | Together, still wrong |
| --- | --- |
| A behaviour and the test that fails without it | Two unrelated fixes |
| A register entry and the generated matrix, moved by the same test | A refactor and a behaviour change |
| A source change and its own doc correction | A source change and an unrelated doc typo |

A test that arrives before the behaviour it covers, because the behaviour is
not written yet, commits alone and says so. A behaviour whose test lives in a
later commit is a behaviour that shipped unproven.

Push when about eight commits have accumulated, or earlier when something needs
to be shared, reviewed or backed up. The count is a habit rather than a rule
this repository can check; what it protects is the ability to push a run of
commits that were each verified on their own.

Check the scope of a commit before writing it, not after:

```bash
git add <the files of this one change>
git diff --cached --stat      # one reason, or stop and split
git commit
```

`git diff --cached --stat` is the cheapest test there is. If the staged files do
not share a single reason that the subject can state, the change is not one
change yet.

## 4. Enforcement

Every check lives in `scripts/`, and `scripts/check_all.py` runs them all:

```bash
python3 scripts/check_all.py            # everything this machine can run
python3 scripts/check_all.py --quick    # no cargo, no fetched samples
python3 scripts/check_all.py --list     # what would run, and why
```

It runs what the machine can run and names the reason it skipped the rest, so a
missing toolchain is visible rather than silently green. `--require-all` turns a
skip into a failure; a CI runner has every tool, so nothing is skipped there.

CI runs the same checks as separate steps, so a failure lands on the run page
next to the check that produced it. Grouped by what they need:

| Group | Checks | Needs |
| --- | --- | --- |
| `policy` | `check_language.py`, `check_i18n.py --strict`, `check_commits.py`, `check_publish.py`, `publish_order.py`, `check_packaging.py` | `git`, and `cargo` for `check_packaging.py` |
| `repo` | the checkers' own unit tests, `release_checksums.py selftest`, `pack_fixtures.py --check` | nothing |
| `schema` | `validate_ofd.py --allow-foreign` over `compliance/fixtures` | `lxml` |
| `rust` | `cargo fmt`, `clippy`, the `msrv` check, `build` and `test`, the golden images, the headless reader window, `check_compliance.py`, and the matrix freshness diff | `cargo`, `git`, and the Rust channel named by `rust-version` for `msrv` |
| `samples` | `sample_fetch.py`, `--check` then `--parse`, once per set | the fetched samples, `cargo` |
| `perf` | `run_benchmarks.py` then `check_performance.py` against the budget in [Performance.md](Performance.md) | `cargo` |
| `review` | `check_commit_scope.py`, which names commits that read as more than one change | `git` |
| `deps` | `cargo deny --all-features check`: advisories, licences, bans and sources | `cargo-deny` |

`msrv` is the one check that runs under a toolchain other than the one on
`PATH`: it reads `rust-version` from the workspace manifest and runs
`cargo +<that version> check`, so the minimum the project states is a fact
rather than a claim. It skips, with the reason, on a machine that has not
installed that channel.

`perf` is two checks because `check_performance.py` reads `target/criterion`,
and a reading left by an older run would pass a gate on code that has since
changed: the first check is the `cargo bench` run that produces a fresh one,
and the gate refuses a log older than the newest source file. It is also the
slowest group, which is why `--quick` leaves it out.

`samples` is the one group that can need the network. `sample_fetch.py` fetches
a third-party sample only when the pinned hash cannot account for what is on
disk, so a working copy that already holds the sets runs offline; a machine
without them and without a route out fails here rather than passing quietly.

`deps` reads `deny.toml` rather than a command line, so a decision about a
dependency is a reviewable diff instead of a flag somebody remembered. A
vulnerability fails the build; an unmaintained crate fails it too unless the
ignore list names the advisory with the reason and the roadmap item that
retires it. Duplicate versions are warnings: two copies of a crate are a smell,
but a version pin already records where they came from.

Three gate shapes recur, and each exists because the failure it guards against
is silent:

- **A dependency is upgraded or dropped out of policy, not out of tidiness.**
  `zip` 9, `lopdf` 0.45 and `zbus_xml` 5.2 are the versions that carry no
  advisory, and the writer changes each of them asks for ship in the same
  commit. A crate no line of code reaches is removed rather than upgraded, so
  the advisory leaves with the compile time.
- **A parser that reads bytes a document supplies is fuzzed**, not only a parser
  that reads what a caller passes. `ttf_face`, `certificate` and `pdf_write` sit
  beside `package_open` and `xml_cursor` for that reason, and `fuzz run` is the
  check that calls them.
- **An edit is checked by reopening what it wrote.** A test that asserts on the
  writer's own bookkeeping passes while the reader loses an entry, so
  `sofd-edit` asserts on the document it gets back.

Everything in `review` reports rather than blocks. A script can measure where
the lines landed; it cannot know why they landed there, and a move, a rename or
an i18n change that reaches its callers all look like a squash from the outside.
The check exists to put the candidates in front of a reader, so the run says
`note` and still passes.

Commit messages are checked at three points, so a violation is caught before
it becomes history rather than during a review:

| Where | What it checks |
| --- | --- |
| `scripts/hooks/commit-msg` | the message being written, before the commit exists |
| `.github/workflows/rust.yml` | the whole reachable history, on a push or a pull request that touches more than documentation |
| `python3 scripts/check_commits.py REF` | the same history, on demand and locally |

The hook is one line of shell and can be skipped with `--no-verify`; the CI step
is the one that cannot. That is why both exist.

Install the local hook once per clone:

```bash
sh scripts/install-hooks.sh
```

The rules it enforces are the ones above: an allowed type, a subject of at
most 75 bytes with no trailing period, a body of at most two non-blank lines,
and no CJK anywhere in the message.

`scripts/commit-baseline.txt` exists to exempt commits that already break
these rules and cannot be rewritten without breaking existing clones. **It is
currently empty**, and the whole history passes without it: the three
over-long messages were rewritten in place once the checker landed.

Adding a line to it is a deliberate decision to accept a violation, and it
should carry a reason. Prefer rewriting the commit — a published message is
the only thing in this repository that cannot be fixed later, and
`git push --force-with-lease` is a small price for a history that needs no
exemption list at all.

### 4.1 Exemptions from the language policy

`check_language.py` scans Rust and Python sources, `Cargo.toml` files, and
Markdown or reST documentation. It fails on CJK characters in any of them and on
comment blocks longer than three lines.

Seven exemptions exist, each deliberately narrow:

| Exemption | Where | Why |
| --- | --- | --- |
| Whole file | any `*.zh-CN.md` | Section 1 asks a Chinese document to declare the language in its name, so the name is the exemption and a new translation needs no list edit |
| Whole file | anything under a `locales/` directory | Locale catalogues are the translation data, wherever the crate that owns them keeps them |
| Whole file and name | fetched test samples under `tests/fixtures/` | A sample is named by the producer that wrote it, and renaming one severs the only link back to where it came from |
| Whole file | `docs/Conventions.md`, `scripts/check_language.py`, `scripts/check_i18n.py` | They exist to hold, quote or match Chinese |
| Whole file | `scripts/sample_fetch.py` | Its pin lists spell out the producer's own file names, exactly like the samples they fetch |
| Whole file | `crates/sofd-i18n/src/format.rs` | Holds the locale day-period literals, which are Chinese and Japanese |
| Single line | the `README.md` language switcher | The bilingual pair must cross-link by the other language's own name |

The `format.rs` exemption covers the whole file rather than its string literals
alone: the CJK in it is three literals and no prose, and a Rust lexer inside the
checker would cost more than that risk is worth.

The sample exemption covers names as well as contents, and it is why the
samples are fetched rather than committed: none of them is source, none is
built, and one nobody can trace upstream is one nobody can explain.

New exemptions should be added to the allowlists in the script with a one-line
reason, so that every exception stays self-documenting.

## 5. Closing an issue

An issue closes when its exit criterion is met, and the closing commit writes
what it delivered into the `Unreleased` section of `CHANGELOG.md`, in the
release's own words rather than the issue's: the issue says what was missing
and why, and the changelog says what a reader can now do. An issue closed for
another reason — superseded, withdrawn, decided against — says so in the
changelog only when a user could notice.

`docs/Roadmap.md` is not updated by a closing issue. It carries directions and
needs that have no exit criterion yet, so an issue finishing says nothing about
whether the direction it served has been reached.

An entry is one sentence, and two only when the change is one a user could get
hurt by. What a reader needs is what they can now do and what it replaced; the
mechanism, the test that proves it and the clause that motivated it belong in
the commit that made the change, which is where anyone debugging it will look.
A changelog long enough that nobody reads it has stopped being a changelog,
which is the failure this rule exists to prevent.

Entries are grouped by capability rather than by the conventional-commit type
that produced them, and a `Changed` or `Removed` heading appears only when the
release actually has one.

## 6. Public API changes

The rule has one clause, and everything below follows from it: **a break fails
the build when it would land on a version crates.io serves, and is free when
that version has not been published.**

`cargo-semver-checks` compares the public API of every crate that states
`publish` (twenty-one of them; the shells and the testkit opt out) and fails
when a change needs a larger version bump than the manifests declare. The
workspace root carries the lint configuration.

The tool has no configuration file of its own: a policy reaches it only through
`[package.metadata.cargo-semver-checks.lints]`, and only for a crate that opts
in with `workspace = true` there. So the policy is written once in the root
manifest and each published crate repeats one line to accept it. A crate that
omits the line is checked against the tool's defaults instead, silently, which
is why a new published crate has to add it.

The default answer to "may we break this between minor releases" is no. Three
decisions depart from the crate's defaults:

| Decision | Effect | Why |
| --- | --- | --- |
| `enum_variant_added` requires `minor` | Adding a variant to an exhaustive enum is allowed in a minor release | Every such enum names an outcome a caller must tell apart. `VerifyStatus` gained `Unchecked` because reporting an unchecked signature as tampered is worse than reporting nothing, and the alternative — a new enum returned from a new function — leaves the old function answering the question wrongly. A break still needs a version, which the check below enforces |
| `constructible_struct_adds_field` requires `minor` | Adding a public field to a struct a caller can build with a literal is allowed in a minor release | The model is data. `TextObject` gained `ReadDirection` and `CharDirection` because the format has them and ignoring them drew turned text upright, so the field is the feature rather than an accident of it. A caller writing that literal has to add the field, which the compiler reports at the line to change |
| The check runs twice, and only against published versions | Against the tree of the released version, and against `HEAD^1` | Against the release both trees carry that release's version, so a break answers "a patch would do"; against the parent it reads the bump the commit intends and answers "not a patch". Both compare a version that is on the registry with the one this tree declares |

The baseline is the crates.io API, not the tag list. A tag is a tree this
project built; a version in a manifest is one a caller can depend on, and only
the second is something a break can hurt. So the gate asks whether the version
this tree declares — read from `[workspace.package]`, and checked against
`sofd-cli`, which publishes last — is on the registry. If it is not, the job
does nothing and says so.

That is what makes the two comparisons safe rather than merely conventional.
Comparing neighbours is what ties a break to the number the next release will
spend: a version on a registry cannot be replaced, so a break merged without a
bump turns the next release into a fix. Comparing against the release is what
catches a break that a bump was meant to cover but that a caller on the old
version would still not expect. Neither comparison has anything to say about a
version nobody can install yet, which is why an unpublished version may be
reshaped freely and only has to settle before it ships.

`cargo semver-checks` skips `publish = false` without being told, which is why
the eight unpublished crates need no opt-in line at all.

