//! The window, and nothing else: the shell builds it and hands every call to
//! `sofd_tauri`, which is where the logic lives and where the tests reach it.

#![windows_subsystem = "windows"]

fn main() {
    sofd_tauri::run();
}
