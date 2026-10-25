//! Input and output abstraction.
//! OFD data may come from a file, memory, a stream or a map; [`ByteSource`] and
//! [`ByteSink`] abstract that away so WASM and server builds share parsing code.
//! [`decode::decode`] reads an image resource, so the parser, the rasteriser and the
//! exporters all accept the same set of formats instead of each their own.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod decode;
pub mod sink;
pub mod source;

pub use decode::{Decoded, Pixels, decode, pixel_size, premultiply};
pub use sink::{ByteSink, DirSink, VecSink};
pub use source::{ByteSource, FileSource, MemorySource, SliceSource};

/// Read and seek combined.
pub trait ReadSeek: std::io::Read + std::io::Seek {}
impl<T: std::io::Read + std::io::Seek> ReadSeek for T {}

/// Above this size a memory map is preferred over reading everything.
pub const DEFAULT_MMAP_THRESHOLD: u64 = 32 * 1024 * 1024;

/// Default decompressed size ceiling per entry, guarding against zip bombs.
pub const DEFAULT_MAX_ENTRY_SIZE: u64 = 512 * 1024 * 1024;

/// Default ceiling on the number of archive entries.
pub const DEFAULT_MAX_ENTRY_COUNT: usize = 8192;

/// Default recursion depth ceiling, since OFD templates may nest.
pub const DEFAULT_MAX_DEPTH: usize = 64;
