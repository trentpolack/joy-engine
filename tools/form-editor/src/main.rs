// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! FORM EDITOR's native entry point; browsers start through the library's `web_main!` instead.

fn main() {
    #[cfg(not(target_arch = "wasm32"))]
    if let Err(error) = form_editor::run() {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
