#![no_main]

//! The parse path a downloaded file actually takes: open the container, read
//! the entry point and the document, then flatten every page the document
//! claims. Each step may return an error for any input, but none may panic.
//! `package_open` covers the container alone; this is the whole chain behind it.

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(mut package) = sofd_package::Package::from_bytes(data) else {
        return;
    };
    let mut parser = sofd_parser::Parser::new(&mut package);
    let Ok(document) = parser.open_document() else {
        return;
    };
    // A document can declare more pages than a fuzz input could ever fill, so
    // the walk stops at what the tree actually lists.
    let pages = document.document.pages.len();
    for index in 0..pages {
        let _ = parser.page_display(&document, index);
    }
});
