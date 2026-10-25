# sofd-error

One error type per OFD subsystem, each converting losslessly into a single
taxonomy, so a caller can match on what went wrong without knowing which layer
raised it.

This crate is **transitive**. It is on crates.io because `sofd` depends on it,
not because it is an interface — depend on [`sofd`](https://crates.io/crates/sofd)
instead, and this comes along.

Part of [sOFD](https://github.com/emfox/sOFD), a Rust engine for OFD documents.
