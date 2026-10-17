// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The level and reusable-object editor: [`main`] owns the document and its edits, [`property_editor`] describes the
//! schema-driven inspector, [`viewport`] presents and picks through the engine renderer, and [`ui`] draws all of it
//! with egui.

pub mod constants;
pub mod main;
pub mod property_editor;
pub mod ui;
pub mod viewport;

pub use main::{
    AssetAccess,
    ComponentChoice,
    FieldKey,
    LevelEditor,
    TransformField,
};
