#!/usr/bin/env python3
"""Tests for check_language.py.

The line exemption is the one rule that decides between data and prose, so the
cases here are the shapes the source actually takes: a constant holding a
statute's wording, an assertion stating a certificate subject, and the three
ways a Chinese string becomes interface text without a message key. The fixture
strings read as the producers wrote them rather than as escapes, because a rule
about readability should itself be readable.
"""

import pathlib
import sys
import tempfile
import unittest

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

import check_language as language  # noqa: E402


class ExternalLiteralTest(unittest.TestCase):
    def test_a_constant_holding_fixed_wording_is_data(self):
        self.assertTrue(language.is_external_string_literal(
            'pub const SEAL_TOP: &str = "全国统一发票监制章";'))
        self.assertTrue(language.is_external_string_literal(
            "const SEAL_LINE_BREAK: char = '／';"))

    def test_an_assertion_stating_a_subject_is_data(self):
        self.assertTrue(language.is_external_string_literal(
            '        assert_eq!(text.bottom, "重庆市税务局");'))
        self.assertTrue(language.is_external_string_literal(
            '    assert!(report.text.contains("页数：1"), "{}", report.text);'))

    def test_a_subject_passed_to_a_parser_is_data(self):
        self.assertTrue(language.is_external_string_literal(
            '    SealText::from_subject_cn("全国统一发票监制章").expect("a seal");'))
        self.assertTrue(language.is_external_string_literal(
            '            "C=CN,O=国家税务总局,CN=全国统一发票监制章",'))

    def test_a_call_result_bound_to_a_name_is_still_data(self):
        # `cargo fmt` breaks the call across lines whenever the subject is long,
        # so the shape a test actually takes is a `let` whose right side is a
        # call. The literal went in as an argument, which makes it data.
        self.assertTrue(language.is_external_string_literal(
            '        let parts = name_parts("C=CN,O=国家税务总局,CN=全国统一发票监制章")'))
        self.assertTrue(language.is_external_string_literal(
            '        let text = SealText::from_subject_cn("全国统一发票监制章")'))

    def test_a_literal_bound_to_a_bare_name_is_prose(self):
        self.assertFalse(language.is_external_string_literal(
            '    let cn = "全国统一发票监制章国家税务总局重庆市税务局";'))

    def test_a_name_bound_to_chinese_is_prose(self):
        self.assertFalse(language.is_external_string_literal(
            '    let label = "印章位置";'))
        self.assertFalse(language.is_external_string_literal(
            '    let font = format!("楷体{}", size);'))

    def test_a_literal_pushed_onto_the_interface_is_prose(self):
        self.assertFalse(language.is_external_string_literal(
            '    lines.push("页数：1");'))

    def test_a_comment_is_never_exempt(self):
        self.assertFalse(language.is_external_string_literal("    // 印章在签名里"))
        self.assertFalse(language.is_external_string_literal("    /// 印章在签名里"))

    def test_a_line_without_cjk_is_not_the_question(self):
        self.assertFalse(language.is_external_string_literal(
            '    assert_eq!(parts[0].kind, "C");'))


class CommentDensityTest(unittest.TestCase):
    def test_three_inline_lines_pass_and_four_do_not(self):
        three = "\n".join(["// one", "// two", "// three"])
        self.assertEqual(self.density(three), [])
        four = three + "\n// four"
        self.assertEqual(len(self.density(four)), 1)

    def test_doc_comments_take_the_wider_ceiling(self):
        five = "\n".join(["/// one", "/// two", "/// three", "/// four", "/// five"])
        self.assertEqual(self.density(five), [])

    def test_one_inline_line_drops_a_doc_block_to_the_narrow_ceiling(self):
        mixed = "\n".join(["/// one", "/// two", "/// three", "// note"])
        self.assertEqual(len(self.density(mixed)), 1)

    def density(self, block: str) -> list[str]:
        """Run the density check over a snippet, since it reads files.

        The checker reports paths relative to the repository, so the snippet is
        written inside one rather than in the system's temporary directory.
        """
        with tempfile.TemporaryDirectory(dir=language.REPO) as room:
            path = pathlib.Path(room) / "snippet.rs"
            path.write_text(block + "\nfn x() {}\n", encoding="utf-8")
            return language.check_comment_density([path])


if __name__ == "__main__":
    unittest.main()
