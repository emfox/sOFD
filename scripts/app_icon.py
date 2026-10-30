#!/usr/bin/env python3
"""Draw the reader's application icon from the icon set this tree already holds.

The window needs an icon, and the honest one is the artwork the app already
uses: the document glyph from the Adwaita symbolic set, filled in the accent
colour the reader paints with, on the panel colour. Reading that path data out of
`crates/sofd-icons/src/data.rs` keeps one source for the artwork, so the icon
cannot quietly show a shape the set no longer carries.

The fill is done here rather than by a converter because the honest fill for a
symbolic glyph is the nonzero winding rule, and an outlined glyph filled as a fan
comes out solid. The same rule the `egui` shell counts is the one applied below.

Usage:
    python3 scripts/app_icon.py            # write crates/sofd-tauri/icons/
    python3 scripts/app_icon.py --check    # fail if the icons are stale
"""

from __future__ import annotations

import argparse
import re
import struct
import sys
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DATA = ROOT / "crates" / "sofd-icons" / "src" / "data.rs"
SET = ROOT / "crates" / "sofd-icons" / "src" / "lib.rs"
OUT = ROOT / "crates" / "sofd-tauri" / "icons"

# The reader's own palette, from docs/Ui-spec.md 5.1, so the icon wears the
# window's colours rather than a third set invented for one picture.
SURFACE = (0xFA, 0xFA, 0xFA)
ACCENT = (0x1C, 0x71, 0xD8)

# Every size a desktop asks for: a task switcher at the small end, 512 for a
# shell that wants a big one.
SIZES = [16, 32, 48, 64, 128, 256, 512]

# The grid the theme draws a glyph on.
GRID = 16.0

# How far a cubic is subdivided. Four sub-steps per unit is what the `egui`
# shell already uses to fill a glyph, so the two agree at a glance.
CURVE_STEPS = 4


def glyph(name: str) -> list[str]:
    """The `d` attributes of one icon, in the order the theme lists them.

    The mapping from an icon to its paths is the `Self::Name => &[A, B]` arm in
    the set's own table, so the icon here is the icon the window draws.
    """
    set_text = SET.read_text()
    variant = variant_of(name)
    match = re.search(rf"Self::{variant} => &\[([^\]]+)\]", set_text)
    if not match:
        sys.exit(f"error: {name} has no paths in {SET.relative_to(ROOT)}")
    data = DATA.read_text()
    out = []
    for constant in (n.strip() for n in match.group(1).split(",")):
        if not constant:
            continue
        found = re.search(rf'pub const {constant}: &str = "(.*)";', data)
        if not found:
            sys.exit(f"error: {constant} is named but not defined")
        out.append(found.group(1))
    return out


def variant_of(name: str) -> str:
    """The set's variant name for a theme icon name."""
    set_text = SET.read_text()
    match = re.search(
        rf'Self::(\w+) => "{re.escape(name)}"', set_text
    )
    if not match:
        sys.exit(f"error: the set has no variant for {name}")
    return match.group(1)


def numbers(d: str) -> list[tuple[str, list[float]]]:
    """Split path data into commands and their operands."""
    tokens = re.findall(r"[A-Za-z]|-?\d*\.?\d+(?:e-?\d+)?", d)
    commands, at, index = [], None, 0
    while index < len(tokens):
        token = tokens[index]
        if token.isalpha():
            at = token
            index += 1
            operands = []
            while index < len(tokens) and not tokens[index].isalpha():
                operands.append(float(tokens[index]))
                index += 1
            commands.append((at, operands))
        else:
            index += 1
    return commands


def subpaths(d: str) -> list[list[tuple[float, float]]]:
    """One glyph's path as closed rings of points on the 16-unit grid."""
    rings: list[list[tuple[float, float]]] = []
    at = (0.0, 0.0)
    start = (0.0, 0.0)
    for command, operands in numbers(d):
        relative = command.islower()
        kind = command.upper()

        def place(x: float, y: float) -> tuple[float, float]:
            return (at[0] + x, at[1] + y) if relative else (x, y)

        if kind == "M":
            pairs = list(zip(operands[0::2], operands[1::2]))
            for n, (x, y) in enumerate(pairs):
                point = place(x, y)
                if n == 0:
                    rings.append([point])
                    start = point
                else:
                    rings[-1].append(point)
                at = point
        elif kind in ("L", "H", "V"):
            for x, y in operands_by(kind, operands, relative, at, place):
                rings[-1].append((x, y))
                at = (x, y)
        elif kind in ("C", "Q"):
            step = 6 if kind == "C" else 4
            for n in range(0, len(operands), step):
                chunk = operands[n : n + step]
                if kind == "C":
                    c1 = place(chunk[0], chunk[1])
                    c2 = place(chunk[2], chunk[3])
                    to = place(chunk[4], chunk[5])
                    rings[-1].extend(cubic(at, c1, c2, to))
                else:
                    c = place(chunk[0], chunk[1])
                    to = place(chunk[2], chunk[3])
                    c1 = (at[0] + 2 / 3 * (c[0] - at[0]), at[1] + 2 / 3 * (c[1] - at[1]))
                    c2 = (to[0] + 2 / 3 * (c[0] - to[0]), to[1] + 2 / 3 * (c[1] - to[1]))
                    rings[-1].extend(cubic(at, c1, c2, to))
                at = rings[-1][-1]
        elif kind == "Z":
            if rings and rings[-1][0] != rings[-1][-1]:
                rings[-1].append(rings[-1][0])
            at = start
    return [ring for ring in rings if len(ring) > 2]


def operands_by(kind, operands, relative, at, place):
    """`L`, `H` and `V` share one loop; each names a different operand shape."""
    pairs = []
    if kind == "L":
        for x, y in zip(operands[0::2], operands[1::2]):
            pairs.append(place(x, y))
    elif kind == "H":
        for x in operands:
            pairs.append(place(x, 0.0))
    else:
        for y in operands:
            pairs.append(place(0.0, y))
    return pairs


def cubic(origin, c1, c2, to):
    """The points a cubic passes through after its start."""
    points = []
    for step in range(1, CURVE_STEPS * 4 + 1):
        t = step / (CURVE_STEPS * 4)
        u = 1.0 - t
        points.append(
            (
                origin[0] * u**3 + c1[0] * 3 * u * u * t + c2[0] * 3 * u * t * t + to[0] * t**3,
                origin[1] * u**3 + c1[1] * 3 * u * u * t + c2[1] * 3 * u * t * t + to[1] * t**3,
            )
        )
    return points


def spans(rings) -> list[tuple[float, float, bool]]:
    """Every edge as `(y_low, y_high, direction)`, sorted by its low end.

    Sorting once per icon is what makes the fill linear: a scanline then walks
    the edges that straddle it in order, rather than testing every edge of every
    ring at every sample.
    """
    edges = []
    for ring in rings:
        for (x0, y0), (x1, y1) in zip(ring, ring[1:]):
            if y0 != y1:
                edges.append((min(y0, y1), max(y0, y1), 1 if y1 > y0 else -1, x0, y0, x1, y1))
    edges.sort()
    return edges


def crossings(edges, y: float) -> list[tuple[float, int]]:
    """Where a scanline meets the outline, with each edge's direction."""
    out = []
    for low, high, direction, x0, y0, x1, y1 in edges:
        if low > y:
            # Sorted by the low end, so nothing later straddles this line.
            break
        if high <= y:
            continue
        out.append((x0 + (y - y0) / (y1 - y0) * (x1 - x0), direction))
    return out


def covered(crossed: list[tuple[float, int]], x: float) -> bool:
    """Nonzero winding at one point: the rule an SVG path fills by."""
    winding = 0
    for at, direction in crossed:
        if at > x:
            winding += direction
    return winding != 0


def pixels(size: int) -> list[tuple[int, int, int]]:
    """The glyph, centred and insetted, on the reader's panel colour."""
    rings = [ring for d in glyph("document-open-symbolic") for ring in subpaths(d)]
    edges = spans(rings)
    inset = size / 6.0
    scale = (size - 2 * inset) / GRID
    # Three samples per axis per pixel: enough for a curve's edge to read as
    # one, and the cost is nine tests against a scanline already computed.
    samples = 3
    out = []
    for row in range(size):
        rows = []
        for sy in range(samples):
            y = (row + (sy + 0.5) / samples - inset) / scale
            rows.append(crossings(edges, y))
        for column in range(size):
            ink = 0
            for crossed in rows:
                for sx in range(samples):
                    x = (column + (sx + 0.5) / samples - inset) / scale
                    if covered(crossed, x):
                        ink += 1
            share = ink / (samples * samples)
            out.append(tuple(round(b * (1 - share) + a * share) for a, b in zip(ACCENT, SURFACE)))
    return out


def png(size: int) -> bytes:
    """A square RGBA PNG, written plainly: no dependency, same bytes every run."""
    rows = pixels(size)
    raw = bytearray()
    for row in range(size):
        raw.append(0)
        for column in range(size):
            raw.extend((*rows[row * size + column], 255))

    def chunk(tag: bytes, data: bytes) -> bytes:
        body = tag + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    header = struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + chunk(b"IHDR", header)
        + chunk(b"IDAT", zlib.compress(bytes(raw), 9))
        + chunk(b"IEND", b"")
    )


def svg(size: int) -> str:
    """The same picture as a vector, for a shell that prefers one."""
    inset = size / 6.0
    scale = (size - 2 * inset) / GRID
    paths = "\n    ".join(
        f'<path d="{d}" fill="#{ACCENT[0]:02x}{ACCENT[1]:02x}{ACCENT[2]:02x}"/>'
        for d in glyph("document-open-symbolic")
    )
    return (
        f'<svg xmlns="http://www.w3.org/2000/svg" width="{size}" height="{size}" '
        f'viewBox="0 0 {size} {size}">\n'
        f'  <rect width="{size}" height="{size}" fill="#'
        f'{SURFACE[0]:02x}{SURFACE[1]:02x}{SURFACE[2]:02x}"/>\n'
        f'  <g transform="translate({inset:.3f} {inset:.3f}) scale({scale:.4f})">\n'
        f"    {paths}\n"
        f"  </g>\n"
        f"</svg>\n"
    )


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "--check", action="store_true", help="compare instead of writing"
    )
    args = parser.parse_args()

    OUT.mkdir(parents=True, exist_ok=True)
    wanted = {OUT / "icon.svg": svg(512).encode()}
    for size in SIZES:
        wanted[OUT / f"{size}x{size}.png"] = png(size)
    # The Tauri context names one icon specifically, so the 512 copy is it.
    wanted[OUT / "icon.png"] = wanted[OUT / "512x512.png"]

    stale = [
        path.name
        for path, data in wanted.items()
        if (path.read_bytes() if path.exists() else b"") != data
    ]
    if args.check:
        if stale:
            print(f"error: stale icons: {', '.join(stale)}", file=sys.stderr)
            return 1
        print(f"OK: {len(wanted)} icon file(s) are current")
        return 0
    for path, data in wanted.items():
        path.write_bytes(data)
    print(f"wrote {len(wanted)} icon file(s) to {OUT.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
