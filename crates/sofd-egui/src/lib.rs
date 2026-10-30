//! The `egui` shell for reading OFD documents: this project's desktop reader,
//! and the same interface compiled to a browser page or an Android activity.
//! The state it draws lives in `sofd-session`, so what another shell has to
//! write again is the painting and nothing else.

#![warn(missing_docs)]

// The entry point a device calls. It is declared here and not in `main.rs`
// because Android loads the cdylib, and that library is built from this crate
// root: the same `mod` under the binary would compile and then export nothing.
#[cfg(target_os = "android")]
pub mod android;
pub mod fonts;
pub mod icons;
pub mod shortcuts;
pub mod theme;
pub mod window;

// The state this shell drives, re-exported so a caller names one crate.
pub use sofd_session::*;
