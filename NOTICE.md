# Third-party notices

sOFD is licensed under [Apache-2.0](LICENSE). It ships the artwork listed
here, which comes under its own terms.

## Adwaita icon theme

The desktop reader's icons, in `crates/sofd-egui/src/icons/mod.rs`, are the
Adwaita symbolic icons. Each one is stored as the `d` attribute of the matching
`<path>` element in the theme's SVG file, and each glyph records the theme's own
name for it (`document-open-symbolic`, `go-next-symbolic`, …).

| | |
| --- | --- |
| Source | [adwaita-icon-theme](https://gitlab.gnome.org/GNOME/adwaita-icon-theme), release 44.0 |
| Copyright | the GNOME Project and contributors |
| Licence | GNU LGPL v3, or Creative Commons Attribution-ShareAlike 3.0 United States |
| Attribution | "GNOME Project", with a link to https://www.gnome.org where possible |

This project takes the icons under **CC BY-SA 3.0 US**. The theme offers LGPL-3.0
as the other option, and a share-alike licence is the one that attaches to the
artwork rather than to the program that draws it: Apache-2.0 covers the source
of this repository, and the icon drawings remain under CC BY-SA.

The following files carry that licence in full:

- The icons are modified in one respect only: the path data is read into points
  by this project's own code instead of by a renderer, and the fill is computed
  at draw time so the glyph takes the window's current colour. The outline is
  not redrawn, and no control point is moved.
- `zoom-fit-page-symbolic` is not a name the theme publishes. `Icon::FitPage`
  keeps its name because it says what the control does, and carries the theme's
  `zoom-in-symbolic` path: a frame with a plus inside, beside the corner
  brackets `FitWidth` takes from `zoom-fit-best-symbolic`.

Copies of both licences:

- CC BY-SA 3.0 US: <https://creativecommons.org/licenses/by-sa/3.0/us/legalcode>
- LGPL-3.0: <https://www.gnu.org/licenses/lgpl-3.0.html>

## Fonts

The interface draws its own labels with the faces bundled by `epaint`, under the
SIL Open Font License 1.1. The reader asks the system for a CJK face where it
needs one, and links no CJK font of its own.
