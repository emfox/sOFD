# sofd-io

`ByteSource` and `ByteSink` over a file, a slice, memory or a directory, and the
one decoder every reader of an image resource goes through.

This crate is **transitive**. It is on crates.io because `sofd` depends on it,
not because it is an interface — depend on [`sofd`](https://crates.io/crates/sofd)
instead, and this comes along.

Part of [sOFD](https://github.com/emfox/sOFD), a Rust engine for OFD documents.
