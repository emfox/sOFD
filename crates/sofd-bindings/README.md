# sofd-bindings

The data shapes the WASM, FFI and server surfaces all answer with, so a caller
sees one `SearchHit` or `SignatureStatus` rather than three that drifted apart.

`DocumentInfo`, `SearchHitData`, `SignatureData`, the `search_hits` and
`signature_statuses` helpers, and `DEFAULT_SEARCH` live here. They hold no
runtime of their own: each shell passes its own `Document` in and gets these
back, which is also what keeps the three shells from re-deriving the same
counts and the same string names.

Part of [sOFD](https://github.com/emfox/sOFD), a Rust engine for OFD documents.
