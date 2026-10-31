//! The shell's names for the page wire format.
//!
//! The format itself lives in `sofd::shell::wire`: it is what any canvas shell
//! needs, and a second shell (#13) would otherwise have copied it. What is
//! left here is the spelling this shell already answered to, so the command
//! layer and its tests keep naming one module.

pub use sofd::shell::wire::{
    Degraded, PageGeometry, WireMarks, WirePaint, WireRect, WireShape, WireStop, WireStroke,
    count_ops, css, geometry, marks,
};
