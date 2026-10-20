// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Interaction rules shared by the editor's views: the viewport camera response, inspector drafts, and the egui
//! widgets that present them ([`widgets`]). Gesture reading, number rules, and panel layout come from the authoring
//! kit in `joy_engine::development`.

pub mod constants;
pub mod numeric_controls;
pub mod viewport_input;
pub mod widgets;
