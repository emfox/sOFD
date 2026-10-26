#!/usr/bin/env python3
"""Tests for run_examples.py.

The script's job is to decide whether an example did what it claims, so what is
worth testing is the judgement: a short PDF, a wrong page count, a directory of
files that are not pictures. Each case here builds the shape on disk and asks
whether the check notices, which is the part that would rot quietly.
"""

import pathlib
import sys
import tempfile
import unittest
import zipfile

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

import run_examples as examples  # noqa: E402

# A one-pixel PNG, so a test can write a file that really is an image.
PNG = bytes.fromhex(
    "89504e470d0a1a0a0000000d494844520000000100000001080600000"
    "01f15c4890000000a49444154789c6300010000050001"
)


class PageCountTest(unittest.TestCase):
    def test_pages_are_counted_from_the_document(self):
        with tempfile.TemporaryDirectory() as room:
            book = pathlib.Path(room) / "book.ofd"
            with zipfile.ZipFile(book, "w") as archive:
                for page in range(3):
                    archive.writestr(f"Doc_0/Pages/Page_{page}/Content.xml", "<x/>")
                archive.writestr("OFD.xml", "<x/>")
            self.assertEqual(examples.page_count(book), 3)

    def test_a_document_with_no_pages_is_a_failure(self):
        with tempfile.TemporaryDirectory() as room:
            book = pathlib.Path(room) / "book.ofd"
            with zipfile.ZipFile(book, "w") as archive:
                archive.writestr("OFD.xml", "<x/>")
            with self.assertRaises(examples.Failure):
                examples.page_count(book)


class PngTest(unittest.TestCase):
    def test_a_picture_per_page_passes(self):
        with tempfile.TemporaryDirectory() as room:
            directory = pathlib.Path(room)
            for page in range(2):
                (directory / f"page-{page}.png").write_bytes(PNG + b"\0" * 2000)
            examples.expect_pages(directory, 2)

    def test_a_missing_page_is_a_failure(self):
        with tempfile.TemporaryDirectory() as room:
            directory = pathlib.Path(room)
            (directory / "page-0.png").write_bytes(PNG + b"\0" * 2000)
            with self.assertRaises(examples.Failure):
                examples.expect_pages(directory, 2)

    def test_a_file_that_is_not_a_picture_is_a_failure(self):
        with tempfile.TemporaryDirectory() as room:
            directory = pathlib.Path(room)
            (directory / "page-0.png").write_bytes(b"not a picture" * 200)
            with self.assertRaises(examples.Failure):
                examples.expect_pages(directory, 1)

    def test_a_truncated_picture_is_a_failure(self):
        with tempfile.TemporaryDirectory() as room:
            directory = pathlib.Path(room)
            (directory / "page-0.png").write_bytes(PNG)
            with self.assertRaises(examples.Failure):
                examples.expect_pages(directory, 1)


class PdfTest(unittest.TestCase):
    def test_a_pdf_of_the_right_length_is_written(self):
        with tempfile.TemporaryDirectory() as room:
            work = pathlib.Path(room)
            book = work / "book.ofd"
            book.write_bytes(b"")
            (work / "book.pdf").write_bytes(
                b"%PDF-1.4\n1 0 obj\n<< /Count 14 >>\nendobj\n%%EOF\n"
            )
            original = examples.run
            examples.run = lambda *a, **k: "no signatures\n"
            try:
                examples.c_abi(work, book, 14)
            finally:
                examples.run = original

    def test_a_pdf_without_an_end_marker_is_a_failure(self):
        with tempfile.TemporaryDirectory() as room:
            work = pathlib.Path(room)
            book = work / "book.ofd"
            book.write_bytes(b"")
            (work / "book.pdf").write_bytes(b"%PDF-1.4\n<< /Count 14 >>\n")

            original = examples.run
            examples.run = lambda *a, **k: "no signatures\n"
            try:
                with self.assertRaises(examples.Failure):
                    examples.c_abi(work, book, 14)
            finally:
                examples.run = original


if __name__ == "__main__":
    unittest.main()
