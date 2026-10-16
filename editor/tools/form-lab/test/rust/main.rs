// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! FORM LAB's Rust tests, built as one binary: one module per TypeScript test file in `test/` (documents, GLB
//! export, the library, the language and geometry exports, materials, material integration, runtime parameters, and
//! the viewport's material commit), plus the compile lifecycle and headless UI tests that drive the real tool with
//! synthetic input. GPU tests skip with a note when no adapter exists; set `JOY_RENDER_CAPTURE_DIR` to keep captures.

mod compiler;
mod document;
mod export_glb;
mod form_library;
mod hosting;
mod language;
mod material_integration;
mod materials;
mod runtime_parameters;
mod session;
mod support;
mod viewport_material_commit;
mod workbench;
