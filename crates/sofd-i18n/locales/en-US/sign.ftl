# sOFD English (US) · Signature module
# Note: English distinguishes singular/plural, unlike Chinese.

sign-valid = Valid
sign-invalid = Invalid
sign-unverified = Unverified
sign-untrusted = Signer certificate not trusted
sign-expired = Certificate expired
sign-integrity-ok = Document has not been altered
sign-tampered = Document content has been modified

# Signature count (plural-aware; add more categories for Slavic/Arabic locales)
sign-count = { $count ->
    [0] No signatures
    [one] { $count } signature
   *[other] { $count } signatures
}

sign-signer = Signer: { $name }
sign-signed-at = Signed at: { $time }
sign-algorithm = Algorithm: { $algo }
sign-issuer = Issuer: { $issuer }
sign-export-report = Export Verification Report
sign-panel-title = Signatures

# Annotations, clause 15. Read apart from the pages they mark up.
annot-panel-title = Annotations
annot-count = { $count ->
    [0] No annotations
    [one] { $count } annotation
   *[other] { $count } annotations
}
annot-none = This document has no annotations

# Comparison, two revisions of one file. Read over the flattened pages.
compare-panel-title = Comparison
compare-open-second = Open Revision to Compare
compare-identical = The two revisions draw the same pages
compare-changed = { $count ->
    [0] No pages changed
    [one] { $count } page changed
   *[other] { $count } pages changed
}
compare-page = Page { $page }: { $change }
