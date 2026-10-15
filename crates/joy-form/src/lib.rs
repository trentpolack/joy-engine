// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! FORM, Joy Engine's procedural geometry language, without a renderer.
//!
//! This crate parses and evaluates the same `.form` scripts as the TypeScript runtime in
//! `src/form/*.ts`, which remains the behavioral reference. Evaluation produces plain
//! [`GeometryData`] records: indexed, Y-up geometry in script units, plus materials, parameter
//! descriptors, project metadata, per-vertex user data, and named output snapshots. Renderers
//! consume those records; nothing here touches a GPU, window, worker, or timer.
//!
//! ```
//! use joy_form::CompiledScript;
//! use serde_json::json;
//!
//! let script = CompiledScript::parse("param width = 2 meta(min=1, max=8, step=1)\nbox(0, 0, 0, width, 1, 1)")?;
//! let geometry = script.evaluate(&json!({"width": 4}), None)?;
//! assert_eq!(geometry.positions.len() / 3, 8);
//! assert_eq!(geometry.form_materials()[0].name, "Default");
//! # Ok::<(), joy_form::Diagnostic>(())
//! ```
//!
//! # Ownership
//!
//! [`CompiledScript`] holds only immutable syntax. Every [`CompiledScript::evaluate`] call creates
//! its own geometry, random stream, material registry, procedural store, and work budgets, and
//! returns owned data that never borrows the caller's overrides, palette, or earlier results.
//!
//! # Budgets
//!
//! Sources are limited to 200,000 UTF-16 code units and 40,000 tokens. Evaluation allows
//! 2,000,000 operations, 150,000 emitted vertices, and 180,000 emitted triangles. Limit
//! violations, malformed values, and unsupported functions return a [`Diagnostic`]; authored
//! content never panics.

mod ast;
mod builder;
mod diagnostic;
mod evaluator;
mod geometry_data;
mod materials;
mod math;
mod parameters;
mod parser;
mod primitives;
mod procedural;
mod resources;
mod user_data;
mod value;

use serde_json::Value as Json;

pub use diagnostic::Diagnostic;
pub use geometry_data::GeometryData;
pub use materials::{AlphaMode, FormMaterial, FormMaterialTextures};
pub use primitives::{DEFAULT_SPHERE_SEGMENTS, mesh_box, mesh_sphere};

/// A parsed FORM script that can be evaluated any number of times.
///
/// Parsing validates syntax and source budgets without executing authored code. The compiled
/// script owns its syntax tree and retains no evaluation state, so evaluations are independent
/// and deterministic for identical inputs.
#[derive(Debug)]
pub struct CompiledScript {
    statements: Vec<ast::Statement>,
}

impl CompiledScript {
    /// Parse FORM source text.
    ///
    /// Returns a [`Diagnostic`] with a one-based line and UTF-16 column when the source is
    /// malformed or exceeds the source, token, or nesting budgets. An empty source is valid.
    pub fn parse(source: &str) -> Result<Self, Diagnostic> {
        Ok(Self { statements: parser::parse(source)? })
    }

    /// Evaluate the script and return owned geometry.
    ///
    /// `overrides` must be a JSON object keyed by parameter name. Unknown names are ignored,
    /// numeric values clamp to the declared bounds, and mesh or material values use the
    /// serialized FORM resource records. `materials` is an optional palette of serialized
    /// material records (see [`FormMaterial`]); `None` yields the single Default material.
    /// Neither input is retained or mutated.
    ///
    /// Returns a [`Diagnostic`] located at the failing statement or expression when evaluation
    /// fails or exceeds a budget; no partial geometry is returned.
    pub fn evaluate(&self, overrides: &Json, materials: Option<&Json>) -> Result<GeometryData, Diagnostic> {
        evaluator::evaluate(&self.statements, overrides, materials)
    }
}

/// Parse and evaluate FORM source in one step.
///
/// Equivalent to [`CompiledScript::parse`] followed by [`CompiledScript::evaluate`]; use a
/// [`CompiledScript`] directly when evaluating the same source with several overrides.
pub fn compile(source: &str, overrides: &Json, materials: Option<&Json>) -> Result<GeometryData, Diagnostic> {
    CompiledScript::parse(source)?.evaluate(overrides, materials)
}
