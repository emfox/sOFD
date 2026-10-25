# What is left

sOFD builds, tests and releases on every platform `egui` runs on, and the
register holds no `partial` clause.

This file is for **direction**: the large outcomes the project is steering
towards, and the changes still too vague to be planned. A concrete piece of
work does not belong here. It belongs in an issue, where its problem, its exit
criterion and the commits that met it stay together. What a release offers is
in [`CHANGELOG.md`](../CHANGELOG.md).

## Where the work happens

- **[Issues](https://github.com/emfox/sOFD/issues)** carry everything that
  can be stated as a problem with an exit criterion: a defect, a missing
  feature, a specific debt. If you can write "done when", it is an issue.
- **This file** carries what has no such statement yet: a direction the project
  is heading in, or a need nobody has worked out the shape of. An entry here
  graduates into one or more issues once the shape appears.

An entry that turns out to be concrete is not kept in both places. It moves
out, and a pointer is left behind only while the issues it became are open.

## Direction

These are the outcomes the project is steering towards. None has a date, and
none is a promise of a version.

| Direction | What reaching it would mean |
| --- | --- |
| **Every render is right on a real document** | A reader that draws a licence or an invoice incorrectly is worse than one that is slow. Correctness of output outranks everything below it, and a crash is the same failure in its worst form. |
| **A signature verification that says no more than it checked** | A signature reported valid is the reason the format is used at all. A verification that overstates its coverage defeats the point of having one. |
| **The engine is usable without this project's applications** | The crates are what a third party builds with, so they need a registry, worked examples and an extension surface. Distribution follows from being usable, not the other way round. |
| **Installs and upgrades are a step an organisation takes, not a project** | Every form that gets deployed — package, bundle, installer, container — installs without a manual step and upgrades without losing state. |
| **The evidence covers more than one producer** | Every construct is checked against documents this project did not write. A construct only our writer emits is one we have only ever read through somebody else's file. |

## Open, and still unshaped

The needs below are agreed to be worth answering, and nobody has yet said what
the answer looks like. Most of them hold an issue for the part that is already
concrete; the issue is where the work goes, and the row stays until the whole
question has a shape.

| Need | Why it is not shaped yet |
| --- | --- |
| **Other shells** — a Tauri shell, a Qt or GTK binding | Both are alternatives to the shipped `egui` reader rather than gaps in it. Which one is worth building depends on a user who wants it. [#13](https://github.com/emfox/sOFD/issues/13) |
| **A public extension surface** for colour profiles, font providers and render backends | The three traits can be guessed, but an extension point without a worked example is a guess at a requirement. It waits until somebody forks a crate to plug something in, because that fork says what the trait has to be. [#12](https://github.com/emfox/sOFD/issues/12) |
| **A cache for what a window redraws** | The stated budgets are met without one, so the shape of the answer depends on a measurement nobody has taken. [#6](https://github.com/emfox/sOFD/issues/6) |
| **Which constructs `office` and `ocr` would name, if implemented** | A DOCX or OOXML conversion in one direction and OCR in the other each needs a model before it needs code, and nobody has asked for either. [#10](https://github.com/emfox/sOFD/issues/10) |
| **Whether version numbers should stay uniform across the twenty-one crates** | The workspace states one version for all of them, so a leaf's patch spends every crate's version. Whether that costs enough to be worth changing is not yet measurable, because nothing has shipped twice on the current scheme. [`Versioning.md`](Versioning.md) states the case, the costs and the order the work would go in |
| **Tracking revisions of GB/T 33190** | Nothing is pending today. This is the standing task that keeps it that way rather than work with a shape. |

## Standing constraints

- **`8.1.6` `PrintQuality` stays a `deviation`.** Annex A carries no such
  element, so there is nothing in a document to assert against. The reason
  travels in the register and the row is not open.
- **Blending is deliberately unsupported.** GB/T 33190-2016 defines no blend
  mode; `Alpha` is the whole of its transparency model. Recorded in
  `docs/Interop.md` §6.
- **Far-term work is not scheduled.** The directions above describe an
  outcome, not a date.
- **A closed issue is not a closed direction.** The direction stays until the
  outcome it names is reached, whatever the issue count underneath it does.

## How to propose a change

Open an issue, state the problem, name the exit criterion. A task without a
verifiable exit criterion belongs in neither place: it is a wish, and this file
already lists the wishes it has.
