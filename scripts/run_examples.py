#!/usr/bin/env python3
"""Run the worked examples against a document this script writes.

The examples are the front door a third party copies, so an example that
compiles but does nothing is worse than one that fails loudly: the person who
copied it cannot tell whether the example is wrong or their document is. This
writes a document, runs each example on it, and checks what landed on disk.

Usage:
    python3 scripts/run_examples.py            # build, run, check
    python3 scripts/run_examples.py --reuse    # skip the build, use the binaries
    python3 scripts/run_examples.py --keep     # leave the work directory behind
    python3 scripts/run_examples.py --only c_abi

Exit code: 0 when every example produced what it claims to. A non-zero exit from
an example fails the run: a hang or a panic is not a pass.

The browser example is not run here. It needs a browser and a page, which this
script has neither of, and the CI step records that.
"""

from __future__ import annotations

import argparse
import json
import os
import shutil
import socket
import subprocess
import sys
import tempfile
import time
import urllib.error
import urllib.request
import zipfile
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
TARGET = ROOT / "target"

# The document every example reads. `write_book` lives in the testkit rather
# than beside the examples, so the file a third party is handed and the file
# the examples are proved against are written by the same code.
WRITER_CRATE = "sofd-testkit"
WRITER_EXAMPLE = "write_book"

# Seconds an example gets before the run is called a hang. Rendering a book and
# converting it to PDF are the slow ones; a minute is generous for both.
TIMEOUT = 180

# Seconds the service gets to answer `/healthz`. The example binds its listener
# before it serves, so a cold start is a fraction of a second; a longer wait here
# would only make a wrong port look like a slow one.
STARTUP = 20


class Failure(Exception):
    """An example did not produce what it claims to."""


def run(args: list[str], timeout: int = TIMEOUT, env: dict[str, str] | None = None) -> str:
    """Run a command, returning its combined output, or raising on a bad exit."""
    try:
        done = subprocess.run(
            args,
            cwd=ROOT,
            capture_output=True,
            text=True,
            timeout=timeout,
            check=False,
            env=env,
        )
    except subprocess.TimeoutExpired as expired:
        raise Failure(f"{' '.join(args)} did not finish in {timeout}s") from expired
    if done.returncode != 0:
        output = (done.stdout + done.stderr).strip()
        raise Failure(f"{' '.join(args)} exited {done.returncode}\n{output}")
    return done.stdout + done.stderr


def cargo(*args: str) -> str:
    """Run cargo, which `main` has already established is on PATH."""
    return run(["cargo", *args])


def binary(name: str) -> Path:
    """The example binary cargo built, whatever profile it was built in."""
    for profile in ("debug", "release"):
        found = TARGET / profile / "examples" / name
        if found.is_file():
            return found
    raise Failure(f"{name} was not built; run without --reuse")


def page_count(book: Path) -> int:
    """How many pages the document declares, read from the document itself.

    Pinning the number here would mean a change to the golden book leaves this
    script asserting yesterday's count, which fails for the wrong reason.
    """
    with zipfile.ZipFile(book) as archive:
        names = [n for n in archive.namelist() if n.startswith("Doc_0/Pages/Page_")]
    if not names:
        raise Failure("the document declares no pages")
    return len(names)


def write_document(work: Path) -> Path:
    """Write the document every example reads, and return its path."""
    book = work / "book.ofd"
    cargo(
        "run", "-q", "-p", WRITER_CRATE, "--example", WRITER_EXAMPLE, "--", str(book)
    )
    if not book.is_file() or book.stat().st_size == 0:
        raise Failure("write_book produced no document")
    return book


def expect_pages(directory: Path, count: int) -> None:
    """One PNG per page, each a real image rather than a truncated write."""
    pages = sorted(directory.glob("page-*.png"))
    if len(pages) != count:
        raise Failure(f"{len(pages)} PNG(s) on disk, expected {count}")
    for page in pages:
        data = page.read_bytes()
        # A PNG opens with its signature. A file that fails to is what a broken
        # render leaves behind, and a size floor rules out an empty picture.
        if not data.startswith(b"\x89PNG\r\n\x1a\n"):
            raise Failure(f"{page.name} is not a PNG")
        if len(data) < 1000:
            raise Failure(f"{page.name} is {len(data)} bytes, which is no picture")


def command_line(work: Path, book: Path, pages: int) -> None:
    """Open, report, render every page, and say the document has no signatures."""
    out = work / "out"
    output = run([str(binary("command_line")), str(book), str(out)])
    if "no signatures" not in output:
        raise Failure("command_line did not report the document as unsigned")
    if f"{pages} page(s)" not in output:
        raise Failure(f"command_line did not report {pages} pages: {output!r}")
    expect_pages(out, pages)


def c_abi(work: Path, book: Path, pages: int) -> None:
    """The C ABI path writes a PDF beside the document it read."""
    output = run([str(binary("c_abi")), str(book)])
    if "no signatures" not in output:
        raise Failure(f"c_abi did not report the document as unsigned: {output!r}")
    pdf = book.with_suffix(".pdf")
    if not pdf.is_file():
        raise Failure("c_abi wrote no PDF")
    data = pdf.read_bytes()
    if not data.startswith(b"%PDF-"):
        raise Failure("c_abi wrote a file that is not a PDF")
    if b"%%EOF" not in data:
        raise Failure("the PDF is not complete: no end-of-file marker")
    if f"/Count {pages}".encode() not in data:
        raise Failure(f"the PDF does not state {pages} pages")


def desktop_viewer(work: Path, book: Path, pages: int) -> None:
    """Draw one frame and report the page count the interface shows."""
    output = run([str(binary("desktop_viewer")), str(book)])
    if f"{pages} page(s)" not in output:
        raise Failure(f"desktop_viewer did not report {pages} pages: {output!r}")
    if "the frame asked for" not in output:
        raise Failure("desktop_viewer drew no frame")


def free_port() -> int:
    """A port nothing is listening on, so two runs cannot collide."""
    with socket.socket() as probe:
        probe.bind(("127.0.0.1", 0))
        return int(probe.getsockname()[1])


def wait_for(url: str, process: subprocess.Popen, seconds: int = STARTUP) -> bytes:
    """Poll a URL until it answers, or the process dies, or time runs out."""
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if process.poll() is not None:
            raise Failure(f"the service exited {process.returncode} before answering")
        try:
            with urllib.request.urlopen(url, timeout=2) as answer:
                return answer.read()
        except (urllib.error.URLError, TimeoutError, ConnectionError):
            time.sleep(0.2)
    raise Failure(f"{url} did not answer in {seconds}s — is the port it binds ours?")


def http_service(work: Path, book: Path, pages: int) -> None:
    """Answer `/healthz`, take a document, and hand back a PDF of it."""
    port = free_port()
    process = subprocess.Popen(
        [str(binary("http_service"))],
        cwd=ROOT,
        env={**os.environ, "PORT": str(port)},
        stdout=subprocess.PIPE,
        stderr=subprocess.STDOUT,
        text=True,
    )
    base = f"http://127.0.0.1:{port}"
    try:
        health = json.loads(wait_for(f"{base}/healthz", process))
        if health.get("status") != "ok":
            raise Failure(f"/healthz answered {health!r}")

        upload = urllib.request.Request(
            f"{base}/api/v1/documents",
            data=book.read_bytes(),
            headers={"Content-Type": "application/ofd"},
            method="POST",
        )
        with urllib.request.urlopen(upload, timeout=30) as answer:
            opened = json.loads(answer.read())
        if opened.get("info", {}).get("pages") != pages:
            raise Failure(f"the service read {opened!r}, expected {pages} pages")

        with urllib.request.urlopen(
            f"{base}/api/v1/documents/{opened['id']}/pdf", timeout=TIMEOUT
        ) as answer:
            pdf = answer.read()
        if not pdf.startswith(b"%PDF-"):
            raise Failure("the service returned something that is not a PDF")
        (work / "service.pdf").write_bytes(pdf)
    finally:
        process.terminate()
        try:
            process.wait(timeout=10)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=10)


# The examples that run, in order. Each takes the work directory, the document
# and its page count, and raises `Failure` when the output is not what it says.
EXAMPLES = (
    ("command_line", command_line),
    ("c_abi", c_abi),
    ("desktop_viewer", desktop_viewer),
    ("http_service", http_service),
)


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--reuse", action="store_true", help="skip the build")
    parser.add_argument("--keep", action="store_true", help="keep the work directory")
    parser.add_argument(
        "--only", action="append", default=[], help="run named examples only"
    )
    args = parser.parse_args()

    if shutil.which("cargo") is None:
        print("cargo is not on PATH", file=sys.stderr)
        return 1

    work = Path(tempfile.mkdtemp(prefix="sofd-examples-"))
    try:
        if args.reuse:
            # A stale binary would make the run prove what the code used to do,
            # so the skip is announced rather than taken quietly.
            print("--reuse: running the binaries in target/, not a fresh build")
        else:
            cargo("build", "--workspace", "--examples")
        book = write_document(work)
        pages = page_count(book)

        chosen = [e for e in EXAMPLES if not args.only or e[0] in args.only]
        if not chosen:
            print(f"no example matches --only {' '.join(args.only)}", file=sys.stderr)
            return 1
        failed = 0
        for name, example in chosen:
            try:
                example(work, book, pages)
                print(f"  {name:<16} ok")
            except Failure as fault:
                failed += 1
                print(f"  {name:<16} FAILED: {fault}")

        if failed:
            print(f"\n{failed} example(s) did not do what they claim")
            return 1
        print(f"\n{len(chosen)} example(s) ran against a {pages}-page document")
        return 0
    finally:
        if args.keep:
            print(f"work directory: {work}")
        else:
            shutil.rmtree(work, ignore_errors=True)


if __name__ == "__main__":
    sys.exit(main())
