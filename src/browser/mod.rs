// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Browser integration (platform layer, WebAssembly only): the game canvas, URL options, failure panels, egui input
//! translation for winit's web backend, gamepads, Web Audio output, `localStorage`, file inputs, and downloads.

pub(crate) mod audio;
pub(crate) mod dom;
pub(crate) mod egui_input;
pub(crate) mod file_input;
pub(crate) mod gamepad;
pub(crate) mod storage;
