#!/usr/bin/env python3
"""Check that a PDF export draws the page the engine itself draws.

The two renders come from the same display list but different backends: the
engine's rasterizer and a PDF viewer. A difference above the limits below means
the export lost or moved a paint, which no structural test on the PDF would
notice.

Usage: python3 scripts/pdf_fidelity.py [FIXTURE ...]
"""

from __future__ import annotations

import json
import re
import subprocess
import sys
import tempfile
from pathlib import Path

from PIL import Image, ImageChops

ROOT = Path(__file__).resolve().parent.parent
SOFD = ROOT / "target" / "debug" / "sofd"
FIXTURES = ROOT / "compliance" / "fixtures"

# Mean grey-level difference a page may show. The committed fixtures measure
# 0.04–2.79 today; the margins leave room for antialiasing changes without
# hiding a paint that was dropped.
LIMITS = {
    "features": 1.5,
    "minimal": 1.5,
    "suwell": 4.0,
    "vertical": 1.5,
    "wps": 4.5,
}
DEFAULT_LIMIT = 5.0

# What a search must find in each committed fixture. Kept in JSON so the text
# a document declares is data rather than a literal in this script.
EXPECTED_TEXT = json.loads((Path(__file__).resolve().parent / "pdf_text.json").read_text())


def run(command: list[str]) -> subprocess.CompletedProcess[str]:
    """Run a command and keep its output for a failure message."""
    return subprocess.run(command, capture_output=True, text=True, cwd=ROOT)


def page_count(doc: Path) -> int:
    """Pages the document declares, read from the CLI's own report."""
    done = run([str(SOFD), "--locale", "en-US", "info", str(doc)])
    if done.returncode != 0:
        raise SystemExit(f"{doc.name}: cannot read the document\n{done.stderr}")
    match = re.search(r"^Pages:\s*(\d+)", done.stdout, re.MULTILINE)
    if match is None:
        raise SystemExit(f"{doc.name}: no page count in:\n{done.stdout}")
    return int(match.group(1))


def compare(doc: Path, work: Path) -> list[str]:
    """One line per page: the measured difference against its limit."""
    limit = LIMITS.get(doc.stem, DEFAULT_LIMIT)
    pages = page_count(doc)
    pdf = work / f"{doc.stem}.pdf"
    done = run([str(SOFD), "convert", "-f", "pdf", "-o", str(work), str(doc)])
    # A conversion that warned still writes the file, and the page it wrote is
    # what this check is about.
    if not pdf.is_file():
        raise SystemExit(f"{doc.stem}: no PDF written\n{done.stderr}")
    lines = []
    for page in range(pages):
        raster = work / f"{doc.stem}-{page}.png"
        done = run([
            str(SOFD), "render", str(doc),
            "--page", str(page), "--dpi", "96", "-o", str(raster),
        ])
        if not raster.is_file():
            raise SystemExit(f"{doc.stem} page {page}: no PNG written\n{done.stderr}")
        prefix = work / f"{doc.stem}-{page}-pdf"
        done = run([
            "pdftoppm", "-r", "96", "-png",
            "-f", str(page + 1), "-l", str(page + 1), "-singlefile",
            str(pdf), str(prefix),
        ])
        rendered = prefix.with_suffix(".png")
        if not rendered.is_file():
            raise SystemExit(f"{doc.stem} page {page}: pdftoppm wrote nothing\n{done.stderr}")
        mine = Image.open(raster).convert("L")
        theirs = Image.open(rendered).convert("L")
        if mine.size != theirs.size:
            theirs = theirs.resize(mine.size)
        diff = ImageChops.difference(mine, theirs)
        histogram = diff.histogram()
        total = mine.size[0] * mine.size[1]
        mean = sum(level * count for level, count in enumerate(histogram)) / total
        verdict = "ok" if mean <= limit else "FAIL"
        lines.append(f"{doc.stem} page {page}: mean {mean:.2f} limit {limit:.1f} {verdict}")
    expected = EXPECTED_TEXT.get(doc.stem, [])
    if expected:
        extracted = run(["pdftotext", str(pdf), "-"]).stdout
        flat = "".join(extracted.split())
        missing = [marker for marker in expected if "".join(marker.split()) not in flat]
        if missing:
            lines.append(f"{doc.stem} text: missing {missing}")
        else:
            lines.append(f"{doc.stem} text: {len(expected)} marker(s) found")
    return lines


def main() -> int:
    if not SOFD.is_file():
        raise SystemExit(f"{SOFD} is missing (cargo build -p sofd-cli)")
    docs = [Path(arg) for arg in sys.argv[1:]] or sorted(FIXTURES.glob("*.ofd"))
    if not docs:
        raise SystemExit(f"no fixtures under {FIXTURES}")
    failed = False
    with tempfile.TemporaryDirectory(prefix="pdf-fidelity-") as tmp:
        work = Path(tmp)
        for doc in docs:
            if not doc.is_file():
                raise SystemExit(f"{doc} is not a file")
            for line in compare(doc.resolve(), work):
                print(line)
                if line.endswith("FAIL") or " text: missing " in line:
                    failed = True
    if failed:
        print("PDF export does not match the raster within the limits above")
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
