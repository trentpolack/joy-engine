// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Rust integration tests for Joy Engine, one module per subsystem, built as a single test binary so the engine links
//! once. Run with `cargo test`. GPU modules skip with a message when no adapter (hardware or software) is available;
//! set `JOY_RENDER_CAPTURE_DIR` to keep their PNG captures. TypeScript tests live beside this folder in `test/`.

mod component_schemas;
mod content;
mod core_parity;
mod destruction;
mod development_kit;
mod environment_system;
mod example_form_building;
mod example_form_destruction;
mod example_scene_effects;
mod form_evaluate_cli;
mod form_evaluator;
mod form_evaluator_parity;
mod form_parser;
mod form_procedural;
mod form_resources;
mod gameplay;
mod gameplay_components;
mod grid_navigation;
mod level_documents;
mod level_runtime;
mod parity_support;
mod particles_asset_scene;
mod particles_effect;
mod particles_headless;
mod particles_parity;
mod particles_rendering;
mod physics;
mod rendering_bloom;
mod rendering_form_assets;
mod rendering_headless;
mod rendering_materials;
mod rendering_scene_effects;
mod rendering_terrain;
mod rendering_triangles;
mod terrain;
