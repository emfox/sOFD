#!/usr/bin/env python3
"""Enforce the English-only and comment-density policy. See docs/Conventions.md."""

from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path

REPO = Path(__file__).resolve().parent.parent

CJK_RANGES = (
    (0x3000, 0x303F),   # CJK punctuation
    (0x3040, 0x30FF),   # Hiragana / Katakana
    (0x3400, 0x4DBF),   # CJK ext A
    (0x4E00, 0x9FFF),   # CJK unified
    (0xF900, 0xFAFF),   # CJK compatibility
    (0xFF00, 0xFFEF),   # fullwidth forms
    (0x20000, 0x2FA1F), # CJK ext B and beyond
)

# Paths exempt from the code-language rule, with a one-line reason each.
ALLOWLIST_FILES = {
    "docs/Conventions.md": "the policy doc quotes the rule in examples",
    "scripts/check_language.py": "this script matches CJK by definition",
    "scripts/check_i18n.py": "reports CJK key mismatches",
    "scripts/sample_fetch.py": "pins samples by the name their producer gave",
}

# Section 1 of docs/Conventions.md: a Chinese document is allowed when its name
# declares the language. The marker is the exemption, so a new translation needs
# no list edit, and Chinese in an English document still fails.
ALLOWLIST_SUFFIX = ".zh-CN.md"

# Lines exempt from the language rule even inside otherwise English files.
# The bilingual README pair must cross-link using the other language's own name.
# The target is absolute because a published README resolves relative links
# against the repository, not against the crate page.
ALLOWLIST_LINE_RE = re.compile(
    r"\[\u7b80\u4f53\u4e2d\u6587\]\((?:\S*/)?README\.zh-CN\.md\)"
)

# Documentation that must stay English. Everything else under docs/ is checked too.
DOC_SUFFIXES = {".md", ".rst"}

# User-facing locales legitimately contain CJK, the engine's and the reader's.
LOCALE_DIR = "locales"

# Python is scanned too: the scripts are engineering artifacts, and the two that
# hold Chinese are the ones named in ALLOWLIST_FILES.
CODE_SUFFIXES = {".rs", ".toml", ".c", ".h", ".js", ".ts", ".py"}

COMMENT_RE = re.compile(r"^\s*(///|//!|//)")
# Section 2 of docs/Conventions.md: one line, two when they are needed. The
# wider allowance is for `///` and `//!` alone, because those are published as
# rustdoc, and the C header is generated from them: a contract a C caller reads
# has no other home. An inline `//` keeps the three-line ceiling.
DOC_COMMENT_RE = re.compile(r"^\s*(///|//!)")
MAX_COMMENT_BLOCK = 3
MAX_DOC_COMMENT_BLOCK = 6


def first_cjk(text: str) -> tuple[int, str] | None:
    for idx, ch in enumerate(text):
        cp = ord(ch)
        if any(lo <= cp <= hi for lo, hi in CJK_RANGES):
            return idx, ch
    return None


# Directories exempt wholesale, with a one-line reason each. Samples keep the
# name their producer gave them; renaming one severs its only link upstream.
ALLOWLIST_DIRS = {
    "tests/fixtures": "fetched test samples are named by their producer",
}


def is_allowlisted(rel: str) -> bool:
    if rel in ALLOWLIST_FILES or rel.endswith(ALLOWLIST_SUFFIX):
        return True
    parts = rel.split("/")
    if LOCALE_DIR in parts:
        return True
    return any(rel.startswith(d.rstrip("/") + "/") for d in ALLOWLIST_DIRS)


# Section 4: prose a reader sees comes from the catalogue, so a CJK literal in
# English source is data — a certificate's subject, a statute's wording — and a
# line whose literals are the only prose is exempt. Three shapes still fail: a
# comment; a literal bound to a bare name, which is a message no translator will
# ever see; and a literal handed to a widget. A literal passed to a function is
# data, whether the call binds its result or not, and that is the shape a test
# writing down a certificate subject takes. Escapes were tried first and compiled
# to the same bytes while making the source unreadable, which is the opposite of
# what the rule is for.
STRING_LITERAL_RE = re.compile(r'"(?:[^"\\]|\\.)*"')
CHAR_LITERAL_RE = re.compile(r"'(?:[^'\\]|\\.)*'")
IDENT_RE = re.compile(r"[A-Za-z_][A-Za-z0-9_]*")
PROSE_BYTES = set(" \t!\"#$%&'()*+,-./0123456789:;<=>?@[\\]^_{|}~")
# A test states the string it expects, so an assertion is exempt by nature.
CATALOG_MARKERS = ("i18n", "MessageId::", "assert", "expect", "panic", "debug_assert")
# A `const` or `static` names data the module carries, so it stays exempt.
CONST_BIND_RE = re.compile(r"^(?:const|static)\b")
BIND_RE = re.compile(r"^let\b[^=]*=\s*(.*)$")
CALL_RHS_RE = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*(?:\s*(?:\.|::)\s*[A-Za-z_][A-Za-z0-9_]*)*\s*\(")
# Functions that put their argument on the interface. Everywhere else a literal
# is a value being parsed, hashed or compared, which is data.
WIDGET_RE = re.compile(
    r"^(?:[A-Za-z_][A-Za-z0-9_]*\s*(?:\.|::)\s*)*(?:push|push_str|set_\w+|label\w*)\s*\("
)


def holds_cjk_literal(line: str) -> bool:
    """Whether the line carries a CJK string or character literal."""
    literals = STRING_LITERAL_RE.findall(line) + CHAR_LITERAL_RE.findall(line)
    return any(first_cjk(literal) for literal in literals)


def binds_a_bare_literal(line: str) -> bool:
    """Whether a `let` gives a literal a name with no function in between."""
    bound = BIND_RE.match(line)
    return bool(bound) and not CALL_RHS_RE.match(bound.group(1).strip())


def builds_prose(line: str) -> bool:
    """Whether the line makes the literal prose rather than passing data on."""
    if any(marker in line for marker in CATALOG_MARKERS):
        return False
    if CONST_BIND_RE.match(line):
        return False
    return binds_a_bare_literal(line) or bool(WIDGET_RE.match(line))


def is_external_string_literal(line: str) -> bool:
    """Whether the line's only prose is inside its literals."""
    stripped = line.strip()
    if not stripped or COMMENT_RE.match(line) or not holds_cjk_literal(stripped):
        return False
    if builds_prose(stripped):
        return False
    outside = STRING_LITERAL_RE.sub("", stripped)
    outside = IDENT_RE.sub("", CHAR_LITERAL_RE.sub("", outside))
    return all(ch in PROSE_BYTES for ch in outside)


def iter_targets() -> list[Path]:
    """Every tracked text file the language policy applies to."""
    out: list[Path] = []
    for p in REPO.rglob("*"):
        if not p.is_file():
            continue
        rel = p.relative_to(REPO).as_posix()
        if rel.startswith("target/") or "/target/" in rel:
            continue
        if rel.startswith(".git/"):
            continue
        if is_allowlisted(rel):
            continue
        if p.suffix in CODE_SUFFIXES or p.suffix in DOC_SUFFIXES:
            out.append(p)
    return sorted(out)


def check_cjk(files: list[Path]) -> list[str]:
    errors: list[str] = []
    for p in files:
        rel = p.relative_to(REPO).as_posix()
        try:
            lines = p.read_text(encoding="utf-8").splitlines()
        except (UnicodeDecodeError, OSError):
            continue
        hits = [
            (i, l)
            for i, l in enumerate(lines, 1)
            if first_cjk(l)
            and not ALLOWLIST_LINE_RE.search(l)
            and not is_external_string_literal(l)
        ]
        if not hits:
            continue
        preview = "; ".join(f"L{i}" for i, _ in hits[:6])
        more = f" (+{len(hits) - 6} more)" if len(hits) > 6 else ""
        errors.append(f"{rel}: CJK found at {preview}{more}")
    return errors


def check_comment_density(files: list[Path]) -> list[str]:
    errors: list[str] = []
    for p in files:
        if p.suffix != ".rs":
            continue
        rel = p.relative_to(REPO).as_posix()
        run: list[str] = []
        start = 0

        def close(last: int) -> None:
            if not run:
                return
            # One inline line among the doc ones drops the run to the narrow
            # ceiling, so a doc block cannot carry an implementation note past
            # the three lines a `//` comment is held to.
            doc_only = all(DOC_COMMENT_RE.match(line) for line in run)
            limit = MAX_DOC_COMMENT_BLOCK if doc_only else MAX_COMMENT_BLOCK
            if len(run) > limit:
                wider = "" if limit == MAX_COMMENT_BLOCK else f", doc comments allow {limit}"
                errors.append(
                    f"{rel}: comment block of {len(run)} lines at L{start}-L{last}{wider}"
                )

        for i, line in enumerate(p.read_text(encoding="utf-8").splitlines(), 1):
            if COMMENT_RE.match(line):
                if not run:
                    start = i
                run.append(line)
            else:
                close(i - 1)
                run = []
        close(start + len(run) - 1)
    return errors


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--cjk-only", action="store_true", help="skip comment density check")
    ap.add_argument("--density-only", action="store_true", help="skip CJK check")
    args = ap.parse_args()

    files = iter_targets()
    failures: list[str] = []

    print(f"Scanned {len(files)} files\n")

    if not args.density_only:
        cjk = check_cjk(files)
        print(f"[language] {len(cjk)} file(s) contain CJK")
        failures += cjk

    if not args.cjk_only:
        dense = check_comment_density(files)
        print(
            f"[comments] {len(dense)} comment block(s) exceed the density limits "
            f"({MAX_COMMENT_BLOCK} lines; {MAX_DOC_COMMENT_BLOCK} for doc comments)"
        )
        failures += dense

    if not failures:
        print("\nOK: policy satisfied")
        return 0

    print(f"\nFAILED: {len(failures)} violation(s)\n")
    for f in failures:
        print(f"  {f}")
    print("\nSee docs/Conventions.md for the rules.")
    return 1


if __name__ == "__main__":
    sys.exit(main())
