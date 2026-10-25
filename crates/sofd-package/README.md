# sofd-package

Opens the ZIP container an OFD file is, validates its structure, hands out
entries as bytes, and replaces the file atomically when it is rewritten.

This crate is **transitive**. It is on crates.io because `sofd` depends on it,
not because it is an interface — depend on [`sofd`](https://crates.io/crates/sofd)
instead, and this comes along.

Part of [sOFD](https://github.com/emfox/sOFD), a Rust engine for OFD documents.
