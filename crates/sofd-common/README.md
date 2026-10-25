# sofd-common

Foundational types the rest of the OFD stack shares: `Millimeter`, `Point`,
`Rect`, version numbers, and the constants GB/T 33190-2016 states.

This crate is **transitive**. It is on crates.io because `sofd` depends on it,
not because it is an interface — depend on [`sofd`](https://crates.io/crates/sofd)
instead, and this comes along.

Part of [sOFD](https://github.com/emfox/sOFD), a Rust engine for OFD documents.
