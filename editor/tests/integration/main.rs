// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Rust integration tests for Joy Editor, built as one test binary (`cargo test -p joy-editor`). They exercise editor
//! behavior against temporary project folders and drive the real editor through `joy_engine::HeadlessShell` with
//! synthetic input. GPU tests skip with a note when no adapter is available; set `JOY_EDITOR_CAPTURE_DIR` to keep their
//! screenshots. The tooling's JavaScript tests live in `tests/tooling/`.

mod asset_authoring;
mod documents;
mod editor_policy;
mod level_editor;
mod level_ui;
mod script_language_tools;
mod session;
mod support;
mod tool_hosting;
mod workspace_files;
mod workspace_ui;
