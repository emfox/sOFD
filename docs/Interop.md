# Interoperability: reading what other implementations produce

An OFD reader is only useful if it agrees with the other readers in the
field. This document records the differences found between sOFD and
other implementations on the same documents, what the standard says about
each, and which side each difference falls on. It is evidence, not a task
list: the finding outlives whatever issue it came from.

The rule for every entry: a difference is not a fault on the other side
until the standard's own text says so. Several of the bodies that produced
the compared reader helped draft GB/T 33190, and deference to that is not
the same as being right.

## 1. Method

The **golden book** ([`crates/sofd-testkit/src/golden.rs`](../crates/sofd-testkit/src/golden.rs))
is fourteen small pages: nine, one per renderer feature, each with a known
expected picture, and five font pages added later. The nine are what the oracle
saw. It was rendered twice at 96 dpi into 378x227 PNGs:

- by sOFD (`sofd render`), and
- by a **second vendor's desktop reader** ("the oracle" below).

The comparison was then checked a third way — against the standard's text
and against a second implementation's source — so no conclusion rests on a
single comparison. The second implementation read is **ofdrw 2.4.0**
(`github.com/ofdrw/ofdrw`), whose Java `AWTMaker` renders to a
`BufferedImage`; only its source is used here, as semantic evidence, never
as a second rendering baseline. Each point below cites file and line in
ofdrw as of commit `84e9eb2`.

Five of the nine pages agree to anti-aliasing. The four that differ are
`clip`, `gradient` (`AxialShd`), `radial` (`RadialShd`) and `wide` (a page
box wider than `CommonData.PageArea`). They are examined in turn.

The book grew five **font** pages afterwards, `latin`, `cjk`, `ttc`, `rtl` and
`manytomany`, and these were **not** taken to the oracle. They exist for the
other direction of drift: a font parser that rounds `units_per_em` differently
or resolves a `cmap` subtable in another order moves glyph geometry while every
op-level assertion still passes, and R22 replaced the parser outright.
Each page draws through one face of `crates/sofd-font/tests/data`, the same
subsets `tests/matrix.rs` claims coverage for, so a face that loses a glyph shows
up as a page with less ink. They are a regression detector, not an interop
finding, and no conclusion in the sections below rests on them.

Pixel counts below are read from the PNGs directly. Colour counts are of
distinct RGBA tuples; a real gradient carries hundreds, a flat fill one.

## 2. Clip: filled versus outlined

**Standard.** Clause 8.4 (*Clipping region*). A clipping region is "a set of
paths or text" that marks "a valid drawing area on the page; what falls
outside it is not affected by the drawing instructions" (the prose opening
8.4). The clip does not replace the object's paint: it only confines it.
Table 34 (`Clips`) makes the region the **intersection** of its `Clip`
children, and Table 35 (`Graphics object`) states the paint switches:

| Attribute | Type | Default |
| --- | --- | --- |
| `Stroke` | `xs:boolean` | `true` |
| `Fill` | `xs:boolean` | `false` |
| `FillColor` | `CT_Color` | transparent |
| `StrokeColor` | `CT_Color` | black |

The fixture sets `Fill="true" Stroke="false"` on a `PathObject` whose path
covers the whole 100x60 mm page, and clips it to the inner rectangle
`20 15 -> 80 45`. The required result is therefore a **solid green fill**
(`FillColor 30 160 90`) confined to that inner rectangle.

**Observed.**

| | Green pixels | Shape |
| --- | --- | --- |
| sOFD | 62 481 | solid fill of the clip rectangle |
| oracle | 569 | outline only |

The oracle's green pixels form a one-pixel frame whose bounding box is
`(76,56)-(302,170)`. The clip rectangle `20 15 -> 80 45` mm maps to
`(75.6,56.75)-(302.4,170.25)` at this scale — a pixel-for-pixel match. So
the oracle **read the clip geometry correctly and drew it as a visible
stroke**, with no fill, instead of using it to confine the object's fill.

**Second implementation.** ofdrw fills a `Fill="true"` path and strokes a
`Stroke="true"` one, exactly as Table 35 states
(`ofdrw-converter/.../AWTMaker.java:361-373`):

```java
if (pathObject.getStroke() == null || pathObject.getStroke()) {  // default true
    ...
    graphics.draw(path2D);
}
if (pathObject.getFill() != null && pathObject.getFill()) {      // default false
    ...
    graphics.fill(path2D);
}
```

It applies a clip with `graphics.setClip(shape)` and never draws it
(`AWTMaker.java:714-719`).

**Verdict.** A gap in the oracle. sOFD and ofdrw both follow Table 35;
the oracle fills nothing and strokes the clip outline instead.

## 3. Shadings: `AxialShd` and `RadialShd`

**Standard.** Clause 8.3.4 (*Gradient*): 8.3.4.2 axial (colour ramps along
an axis from a start to an end point) and 8.3.4.3 radial (two similar
ellipses with a ramped fill between their edges). Table 28 of clause 8.3.3
lists `AxialShd` and `RadialShd` as complex colours usable "for filling and
stroking graphics and text".

**Observed.**

| Page | oracle | sOFD |
| --- | --- | --- |
| `gradient` (`AxialShd`) | 64 473 px of one colour, black | ramp, hundreds of colours |
| `radial` (`RadialShd`) | 2 colours total (black, white) | 727 colours |

The oracle renders both shadings as a **flat black fill**.

**Second implementation.** ofdrw's `AWTMaker` contains no shading code at
all — a search for `Shading`/`AxialShd`/`RadialShd`/`GouraudShd` in that
file returns nothing. Its colour path (`AWTMaker.java:804-845`) reads a
`CT_Color`'s `Value` array and returns `null` when the colour carries only a
shading (`845: if (array == null) return null;`), so such a path is simply
not filled. ofdrw therefore also does not draw these gradients, though it
degrades to *nothing* where the oracle degrades to *black*.

**Verdict.** A gap in the oracle (and, on this point, a smaller one in
ofdrw). Verified against Table 28 and clause 8.3.4; the ramp our render
produces is what the text asks for. Our `Shading` model lives in
[`crates/sofd-model/src/color.rs`](../crates/sofd-model/src/color.rs).

## 4. A page box wider than `CommonData.PageArea`

**Standard.** Clause 8.1.3 (*Page space*): "the size of the whole page
space is determined by `PhysicalBox` of the `PageArea` node (see 7.5,
document root)". A page may carry its own `Area`; the document default in
`CommonData` is the fallback.

**Observed.** The `wide` fixture declares `<Area><PhysicalBox>0 0 297 100</PhysicalBox></Area>`.

| | Output | Aspect |
| --- | --- | --- |
| sOFD | 1754x591 | 2.97 (297:100) |
| oracle | 378x227 | 1.67 |

The oracle renders the 297x100 mm page into a ~A4-ratio box, i.e. it takes
the document default `CommonData.PageArea` and ignores the page's own,
wider `PhysicalBox`.

**Second implementation.** ofdrw takes the page box from the page's own
area first (`ofdrw-reader/.../OFDReader.java:506-511`), falling back to the
template, then `CommonData.PageArea` (`539`), then A4; `ImageMaker.makePage`
sizes the bitmap from that box (`ImageMaker.java:59-63`). Same precedence as
sOFD (`crates/sofd-parser/src/lib.rs:105-107`: page area's
`physical_box`, else `common_data.page_area.physical_box`).

**Verdict.** A gap in the oracle. Both sOFD and ofdrw honour the page's
own box; the oracle clamps to the document default.

## 5. One correction to our own reading: `Clips` and `TransFlag`

The audit also caught a defect on **our** side, and then partly reversed the
first reading of it — recorded here because the reversal is the point of
using more than one reference.

The 2016 text (Table 33) gives a clip no attribute but its `Area` list, and
`Trans` appears nowhere in the standard's body. Our parser nonetheless read
a `TransFlag` on `Clips`, with a default of `true`.

The second implementation shows the attribute is real but is **not** in the
2016 edition: ofdrw carries `Clips.setTransFlag`/`getTransFlag` with the note
"controlled by `TransFlag` since version 1.2 ... default `false`"
(`ofdrw-core/.../clips/Clips.java:82-99`), i.e. it is an **OFD 1.2
extension**. An independent third implementation (the Go `zc310/ofd`,
CHANGELOG) states the same and adds the decisive detail: the attribute "is
missing from the XSD ... real OFD (including the official reader samples)
and the parsing side have always treated it as `false`", and "the default
(**spec `true**) makes the reader fold the object CTM onto the clip path".

So the honest position has two halves:

- We were **wrong to read it at all** from a 2016 document, and wrong about
  the default — the spec default is `true`, we assumed `true`, but every
  real sample omits it and means `false`.
- We were also **wrong to call it fictional**: it is a documented 1.2
  extension, not an invention.

The fix is the same either way: the attribute is gone
(`crates/sofd-model/src/object.rs`, `crates/sofd-parser/src/content.rs`),
and the clip stays in object space unconditionally, which is what clause 8.4
says and what every sample does. The regression test is
`gb_t_33190_8_4_a_clip_stays_in_object_space_beside_the_ctm`
(`crates/sofd-parser/tests/conformance.rs`).

## 6. A Gouraud mesh in SVG, where SVG 1.1 has no mesh

**Standard.** Clause 8.3.4.4 (*GouraudShd*): a lattice of points, each with a
colour, and the triangles over them, coloured by straight-line interpolation
across each face. Table 28 makes it a complex colour like the two shadings
above, so it fills and strokes graphics and text.

**What the other formats do.** PDF has a construct for exactly this:
`ShadingType 4`, a free-form triangle mesh with a colour per vertex, which is
why the PDF export reproduces the mesh rather than approximating it. SVG 1.1
has none: its only paints are a solid colour, a linear gradient, a radial
gradient and a pattern, and none of them varies by vertex. SVG 2's `<meshgradient>`
would, and no renderer here reads SVG 2 gradients.

**Verdict.** The limit is in the format, not in this project, so the export no
longer spends a whole region on one colour: each triangle becomes one `<path>`
filled with the mean of its three vertices. The colour then changes at every
edge rather than across every face, which is a coarser copy of the page but a
copy of it — a mesh of 400 triangles carries 400 flat patches of the shaded
region instead of one flat patch of the whole. The geometry keeps its stroke
and no fill, the triangles carry the fill, and a mesh with no triangles at all
keeps the mean vertex colour, because there is nothing finer to draw.

Two consequences a caller should know. A stroked mesh outline is drawn over the
triangles rather than under them, so a stroke wide enough to overlap the fill
covers it. And the file grows by roughly one `<path>` per triangle, which for a
page-sized mesh is a few thousand short elements rather than a single fill.

## 7. Blending: no mode to support, so nothing is left unsupported
The maintainer asked whether the engine should honour a blend mode, so the
first move was to look for one. There is none to find.

GB/T 33190-2016 carries **no blend-mode element or attribute**. A search of
the vendored Annex A schemas for any name containing `blend` returns nothing,
and the only transparency control the standard defines is the per-object
`Alpha` on a fill or stroke colour (`Page.xsd`, `CT_Color`) — which the engine
already honours, in every backend.

| Name | Where the standard defines it | Read today? |
| --- | --- | --- |
| `Alpha` on `FillColor`/`StrokeColor` | `Page.xsd`, `CT_Color` | Yes |
| A blend mode (`Multiply`, `Screen`, …) | nowhere | Nothing to read |

So the fix is not to implement a mode but to **stop implying one is missing**:
the item is recorded as deliberately not supported rather than deferred, and
`Alpha` is documented as the whole of the standard's transparency model. A
blend mode that a producer writes anyway travels in a non-standard extension,
which the reader ignores the same way it ignores every unrecognised attribute —
it does not fail, and it does not guess at an effect the standard never named.

## 8. What this changed in our own code

The comparison paid for itself in the other direction. The four oracle gaps
above are external; these were ours, found while establishing them:

| Defect | Where | Fix |
| --- | --- | --- |
| `FillRule` read where the attribute is `Rule` | parser | renamed to match Table 35 |
| `Fill`/`Stroke` defaults taken from the wrong model | parser | Table 35 defaults restored |
| Path coordinates read as absolute, not offsets from the boundary corner | parser | made relative, clause 8.1.4 |
| `Clips` read with a nonexistent `TransFlag` | parser, model | attribute removed (section 5) |

## 9. Reproducing

The golden fixtures and the writer are in
[`crates/sofd-testkit`](../crates/sofd-testkit). Regression compares
sOFD's own output against the committed reference PNGs only; no second
implementation is invoked in CI. The oracle renders and the ofdrw source
citations above are evidence for this document, not test inputs.

### A document from a second producer

Evidence of a producer's shape does not have to stay out of CI, so one
document another implementation wrote is committed and tested:
[`compliance/fixtures/suwell.ofd`](../compliance/fixtures/suwell). Its source
XML sits beside the archive, as it does for the fixtures we write, because
that is the shape a person reads.

It is `testPathFillOpacity.ofd` from the ofdrw sample set, written by
**Suwell OFD convertor** 1.0.22.0526.1854 (`native_producer`
`swdomsdkqt_20220526`) — a commercial producer, not ofdrw itself and not this
project. The ofdrw set is fetched rather than committed because its
documents are not ours to redistribute; that is true of this one too, so the
copy here is the sample's own bytes under the four corrections below, taken
under the same footing as citing that set's source.

Four fields had to take the shape annex A declares before the schema would
accept the document. Each is a producer habit, and the correction is what
annex A says instead:

| Field | What the producer wrote | What annex A declares |
| --- | --- | --- |
| `OFD/@Version` | `1.1` | fixed to `1.0` |
| `DocInfo/CreationDate`, `ModDate` | `2023-7-12` | `xs:date`, so `2023-07-12` |
| `CommonData` order | `PageArea` before `MaxUnitID` | `MaxUnitID` first |
| `Permissions` | `CopyText`, `ContentRegist` | neither is in the 2016 text; removed |

Nothing else about the document is altered: the page content, both embedded
fonts and the background image are the sample's own bytes. The four
corrections move no ink — the page renders pixel for pixel the same as the
sample it came from, which is what `testPathFillOpacity.ofd` and this fixture
are compared on when either is regenerated.

The 1.1 version is worth naming rather than fixing quietly. The sample states
it while its `Permissions` carries `CopyText`, an element the 2016 edition
does not define, so 1.1 is what a producer wrote rather than what this engine
must accept on the strength of a number alone. `sofd info` on the committed
fixture reports `Version: 1.0`, because that is what it now declares.

`crates/sofd-parser/tests/third_party.rs` flattens its page and asserts what
the page has: 36 text runs spelling characters, an image, and ink. The annex A
gate in `scripts/check_all.py` validates it beside our own fixtures, so it is
held to the schema and to the renderer at once.
