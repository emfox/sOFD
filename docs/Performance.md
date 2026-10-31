# Performance

What the engine costs, how to measure it, and what the current numbers are.

## How to run the benchmarks

```sh
cargo bench
```

Four targets, one per stage a reader pays for:

| Target | Crate | Measures |
| --- | --- | --- |
| `parse` | `sofd-parser` | Opening a document, and flattening one page or all of them |
| `memory` | `sofd-parser` | What a rasterized page holds, counted rather than timed |
| `convert` | `sofd-convert` | PNG, SVG and PDF output for one page |
| `search` | `sofd-search` | Building an index, and querying it |

The timed targets run at one page, eight and twenty, and report throughput in
objects, so both the per-page and the per-object cost are visible. A single
target runs alone with `cargo bench -p sofd-parser --bench parse`.

## The document being measured

The benchmarks do not use the fixtures. Samples are fetched rather than
committed, and a benchmark needs the same bytes on every machine and every
run. `sofd_testkit::bench_archive(pages)` builds one instead: a real
archive with a real embedded face, 120 objects per page, mixed on purpose —
filled paths, strokes, axial and radial shadings, and text runs — because a
baseline of one path and one word measures nothing a reader would wait for.

## The baseline

Measured on the development machine, 96 DPI, `cargo bench` defaults. Treat
these as the shape of the cost, not as a promise about other hardware.

| Benchmark | One page | Eight pages | Twenty pages |
| --- | --- | --- | --- |
| open a document | 24 µs | 32 µs | 45 µs |
| flatten a page | 477 µs | 481 µs | 465 µs |
| flatten a document | 477 µs | 3.72 ms | 9.27 ms |
| render to PNG | 5.46 ms | 5.17 ms | 5.19 ms |
| convert to SVG | 530 µs | 526 µs | 530 µs |
| convert to PDF | 2.21 ms | 2.13 ms | 2.13 ms |
| build a search index | 1.85 µs | 27.5 µs | — |
| run a query | 3.19 µs | 25.8 µs | — |

The per-page figures are flat across all three counts, which is what linearity
looks like: rasterizing a page costs the same whether it is the first or the
twenty-first. Rasterization dominates everything else, and it is the one number
a performance budget should be stated against.

`open a document`, `flatten a document` and `build a search index` grow with the
page count because all three walk every page, so their twenty-page figure is the
interesting one; the one-page figure is the fixed cost of reading the entry
point.

## The budget

A benchmark on its own is a number nobody acts on. A budget turns it into a
line: this is what a reader is allowed to wait for, and a change that crosses
it has to be argued for rather than merged by accident.

The figures below are the *one-page* case, because that is what a reader feels
when they open a document and scroll. They are set at roughly **four times the
measured baseline**, which is deliberately loose: ordinary refactoring,
dependency bumps and CI hardware all move a benchmark by tens of percent, and a
budget that tight would be red on a bad afternoon and ignored within a week.
What it catches is the change that makes a page cost ten times what it did —
a backend that stops caching, a font lookup turned back into a re-parse per
glyph, a shading resolved per pixel.

| Stage | Budget (one page) | Baseline | Headroom |
| --- | --- | --- | --- |
| Open a document | 150 µs | 24 µs | 6.3× |
| Flatten a page | 2 ms | 477 µs | 4.2× |
| Render a page to PNG | 20 ms | 5.46 ms | 3.7× |
| Convert a page to SVG | 2.5 ms | 530 µs | 4.7× |
| Convert a page to PDF | 10 ms | 2.21 ms | 4.5× |

Two budgets are stated per page rather than per document, because the fixed
cost of opening is already a separate line: a 500-page document is allowed
500 × its per-page budget plus one open, and anything that scales worse than
that is the regression.

> The budget is a ceiling for a *deliberate* regression check, not a target.
> A change that makes a stage slower and does not cross the ceiling is still
> worth a note in the commit message; the ceiling only prevents the slow
> change nobody noticed.

## The memory ceiling

A timing says how long a page takes and nothing about how much of the machine
it is holding. The documents this is for are scanned invoices of tens of pages,
and a single A4 at 300 dpi is about 26 MB once decoded — so a reader that keeps
a dozen of them is holding three hundred megabytes and will be killed on a
machine that looked fine a minute earlier. Time and memory therefore have
separate budgets, and the memory one is the reason the cache has a size at all.

| Ceiling | Budget | Measured | Headroom |
| --- | --- | --- | --- |
| One page, rasterized and held | 40 MB | 18.8 MB | 2.1× |
| A twenty-page packet, all pages held | 512 MB | 18.8 MB | 27× |

The single-page figure is what one raster holds once it is decoded: A4 at 96
DPI is 794 × 1123 pixels of RGBA, which is 3.6 MB, and the rest is the
flattened page, the glyph cache and the copy the encoder makes. The page is
measured at the reader's default zoom, not at its worst, because 40 MB is
already above what the worst case can reach within the texture limit: a page
that fits `MAX_TEXTURE_SIDE` is at most 2048 px on its long edge, so its raster
cannot exceed 2048 × 2048 × 4 bytes, or 16 MB, plus the same again for the copy
the texture upload makes.

The twenty-page figure is the one that decides the cache. Every page held at
once must stay under 512 MB, which at 18.8 MB a page is twenty-seven pages; the
reader holds twelve, so the ceiling has room for the whole window a continuous
layout shows and then some. A change that made a page cost 40 MB would bring
twenty pages to 800 MB and be caught here rather than on a reader's machine.

Both readings come from the `memory` benchmark, which wraps the allocator and
prints what a page holds once it has been drawn. It lives in the facade rather
than in the parser, because a page's bytes are only settled after the page has
been flattened and rasterized, and that is the whole chain:

```sh
cargo bench -p sofd --bench memory | tee memory.log
python3 scripts/check_performance.py --logs memory.log
```

It is a printout rather than a criterion estimate because criterion measures
time: a byte count has no duration, and criterion would report the work as
nanoseconds and mean nothing by it. The reading is taken once, outside the
measurement loop, so the loop's own allocations cannot disturb it.

## The clip stack

A clip is a mask the size of the page: one byte per pixel, and the pixel count
is the raster's own. A4 at the reader's default zoom is 794 × 1123, so 0.83 MB
per clip; at 300 dpi the same page is 2481 × 3508, so 8.3 MB. Saving the
graphics state holds that mask, which is why the state keeps a shared one
rather than a copy: `q` costs a pointer, and `Q` gives it back.

| Page | Mask | 120 clips, all in force | 120 clips, one at a time |
| --- | --- | --- | --- |
| A4 at 96 dpi | 0.83 MB | 103 MB | 1.7 MB |
| A4 at 150 dpi | 2.1 MB | 260 MB | 4.2 MB |
| A4 at 300 dpi | 8.3 MB | 1038 MB | 17 MB |

Measured with a counting allocator over a page of 120 objects, against 3.4 MB
for the same page with no clip at all. The right-hand column is the shape a
producer emits: one clip, its objects, then the restore, so the peak is one mask
above the rest of the page. The middle column is 120 clips left open at once,
which is a depth no document reaches: the display list refuses a save past 64
levels, so the page would fail to open rather than hold those masks. It is
listed because it is the ceiling, and because it says what the failure is worth
knowing: a mask per level, from which the nesting depth alone predicts the peak.

The cost of one clip change, on a page with one clip and one object, is 261 µs
at 96 dpi and 6.1 ms at 300 dpi, against 104 µs and 6.8 ms for the same page
without the clip. Both figures are all three masks: the clip's own, the copy it
starts from, and the temporary `intersect_path` fills before it multiplies by
the current alpha. They are the same size, so a clip costs three pages of mask
in time and one in peak, whatever the raster's own size.

The budget in the table above is stated for the same one-page case and holds
with this: the measured pages here carry one clip at a time, the mask is counted
in the 40 MB per page, and the PNG figure those pages are budgeted against
already includes drawing them. What is not budgeted is a page whose clip count
grows with its object count, because no measured document has one.

## How the budget is enforced

`scripts/check_performance.py` reads the criterion output a `cargo bench` run
leaves in `target/criterion/`, plus the memory printout when one is given, and
checks each budgeted line against its ceiling. It runs locally after a
`cargo bench`, and in CI as the `performance` job:

```sh
cargo bench -p sofd-parser --bench parse
cargo bench -p sofd --bench memory | tee memory.log
cargo bench -p sofd-convert --bench convert
python3 scripts/check_performance.py --logs memory.log
```

Only the ceiling is enforced, never the change against a stored baseline. A
gate on the change would fail on ordinary noise and be disabled within a week;
a gate on the ceiling fails on the change worth stopping. A missing reading is
a failure rather than a pass, because a budget nobody measures is a budget
nobody has. Moving a ceiling is a deliberate edit to this document, which is
the point: the argument happens in the diff rather than in a comment nobody
reads.

## What a scroll step costs

A zoom past the texture limit rasterizes the part of the page on screen, and a
scroll asks for a new part. The reader's page cache answers that by area: a
raster already covering the view is handed back rather than drawn again, which
is what `WINDOW_MARGIN` pays for.

| Zoom, 2048 limit | 20 wheel steps | Rasters | DPI |
| --- | --- | --- | --- |
| 2x | 1 | 50 ms | 192 |
| 3x | 1 | 53 ms | 288 |
| 4x | 3 | 84 ms | 384 |
| 6x | 14 | 147 ms | 576 |
| 8x | 16 | 151 ms | 768 |

One raster of an A4 at these resolutions costs 50 to 120 ms, so the table reads
as the number of times the reader pays that. Up to 4x the margin absorbs almost
every step. Further in it stops helping, and the reason is arithmetic: the
window is capped by the texture limit, the view is not, so at 8x a 2,048-pixel
raster covers 68 mm while the view is already 53 mm of a page that scrolls. The
margin has nowhere left to grow.

`reader_scroll` measures the lookup on its own: **44 ns**, against the 50 to
120 ms render it stands in for. The cache is not the cost; deciding to use it
is free, and what is left is a render that no cache can avoid because the
pixels asked for genuinely are not in hand.

## What is not measured yet

The budget covers opening, flattening, the three one-page conversions and the
memory a page holds. Search is benchmarked but not budgeted, because a reader
never waits on it directly.

Two costs are still unmeasured: the memory of a *flattened* document without any
raster, which matters for export and for the CLI; and the reader's own peak,
which includes the decoded image, its GPU upload and the `egui` texture, not
only the raster counted here. Both are candidates once a reader on a small
machine says which one hurts.
