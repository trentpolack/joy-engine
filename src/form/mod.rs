// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! FORM, Joy Engine's procedural geometry language, without a renderer.
//!
//! This module parses and evaluates `.form` scripts. Frozen reference fixtures preserve
//! compatibility with existing authored content. Evaluation produces plain
//! [`GeometryData`] records: indexed, Y-up geometry in script units, plus materials, parameter
//! descriptors, project metadata, per-vertex user data, and named output snapshots. Renderers
//! consume those records; nothing here touches a GPU, window, worker, or timer.
//!
//! ```
//! use joy_engine::form::CompiledScript;
//! use serde_json::json;
//!
//! let script = CompiledScript::parse("param width = 2 meta(min=1, max=8, step=1)\nbox(0, 0, 0, width, 1, 1)")?;
//! let geometry = script.evaluate(&json!({"width": 4}), None)?;
//! assert_eq!(geometry.positions.len() / 3, 8);
//! assert_eq!(geometry.form_materials()[0].name, "Default");
//! # Ok::<(), joy_engine::form::Diagnostic>(())
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
mod language_information;
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

pub use diagnostic::{
    Diagnostic,
    FormValueError,
    SourceToken,
};
pub use geometry_data::GeometryData;
pub use language_information::{
    FORM_ATTRIBUTE_DOMAINS,
    FORM_COMMANDS,
    FORM_KEYWORDS,
    form_language_catalog,
};
pub use materials::{
    AlphaMode,
    FormMaterial,
    FormMaterialTextures,
};
pub use primitives::{
    DEFAULT_SPHERE_SEGMENTS,
    mesh_box,
    mesh_sphere,
};

/// Largest FORM LAB document, in bytes (and UTF-16 code units); capacity includes base64 texture expansion. Matches
/// the TypeScript `DOCUMENT_LIMITS.fileBytes`.
pub const DOCUMENT_FILE_BYTES: usize = 64_000_000;

/// Largest embedded material texture, in decoded bytes (`DOCUMENT_LIMITS.textureBytes`).
pub const DOCUMENT_TEXTURE_BYTES: usize = materials::MAX_TEXTURE_BYTES;

/// Largest combined embedded texture size of one material palette, in decoded bytes
/// (`DOCUMENT_LIMITS.totalTextureBytes`).
pub const DOCUMENT_TOTAL_TEXTURE_BYTES: usize = materials::MAX_TOTAL_TEXTURE_BYTES;

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

/// The editable FORM default material record (`defaultMaterial()`): white, fully rough, opaque, without textures.
pub fn default_material() -> Json {
    materials::default_material()
}

/// Validate a serialized material palette and return an owned copy with the Default material first (`readMaterials`).
///
/// `None` yields the Default material alone. Names must be unique ASCII identifiers; factors lie in 0–1; textures are
/// embedded PNG, JPEG, or WebP data URLs within [`DOCUMENT_TEXTURE_BYTES`] each and
/// [`DOCUMENT_TOTAL_TEXTURE_BYTES`] together; at most 128 materials.
pub fn read_materials(materials: Option<&Json>) -> Result<Vec<Json>, FormValueError> {
    materials::read_materials(materials).map_err(FormValueError)
}

/// Validate one parameter override and return an owned copy (`readParameterValue`): a finite scalar, an XYZ vector,
/// an RGBA color, a mesh resource, or a material resource. Numbers are written as JavaScript writes them, so integral
/// values stay JSON integers.
pub fn read_parameter_value(value: &Json) -> Result<Json, FormValueError> {
    let parsed = parameters::read_json(value).map_err(FormValueError)?;
    Ok(match parsed {
        value::Value::Number(number) => crate::core::json_number(number),
        value::Value::Array(components) => Json::Array(components.into_iter().map(crate::core::json_number).collect()),
        other => other.json(),
    })
}

/// Validate a serialized mesh resource (`readMesh`): complete XYZ vertices within ±1,000,000, matching normals and
/// UVs, and complete triangles with valid indices.
pub fn read_mesh(value: &Json) -> Result<Json, FormValueError> {
    resources::read_mesh(value).map_err(FormValueError)
}

/// Validate a serialized material resource (`{"kind": "material", "material": …}`, `readMaterialValue`).
pub fn read_material_value(value: &Json) -> Result<Json, FormValueError> {
    resources::read_material_value(value).map_err(FormValueError)
}

/// A centered box mesh resource in script units (`meshBox`), the value a mesh parameter accepts.
pub fn mesh_box_value(width: f64, height: f64, depth: f64) -> Result<Json, FormValueError> {
    resources::mesh_box(width, height, depth).map_err(FormValueError)
}

/// A centered sphere mesh resource in script units (`meshSphere`), the value a mesh parameter accepts.
pub fn mesh_sphere_value(radius: f64, segments: u32) -> Result<Json, FormValueError> {
    resources::mesh_sphere(radius, f64::from(segments)).map_err(FormValueError)
}

/// The quoted name token of each top-level `project meta(name="…")` declaration, in source order, so editors can
/// rename a project without reformatting its metadata. Fails with the parser's diagnostic for invalid source.
pub fn project_name_tokens(source: &str) -> Result<Vec<SourceToken>, Diagnostic> {
    let statements = parser::parse(source)?;
    Ok(statements
        .iter()
        .filter_map(|statement| match &statement.kind {
            ast::StmtKind::Project { name, .. } => {
                Some(SourceToken { text: name.text.clone(), line: name.line, column: name.column })
            }
            _ => None,
        })
        .collect())
}

#[cfg(test)]
mod api_tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn project_name_tokens_locate_the_quoted_name() {
        let tokens = project_name_tokens("# x\nproject meta(info=\"A\", name=\"Old\")").unwrap();
        assert_eq!(tokens, [SourceToken { text: "\"Old\"".into(), line: 2, column: 29 }]);
        assert!(project_name_tokens("point(").is_err());
        assert!(project_name_tokens("point(0,0,0)").unwrap().is_empty());
    }

    #[test]
    fn parameter_values_validate_and_keep_integers() {
        assert_eq!(read_parameter_value(&json!(2.0)).unwrap().to_string(), "2");
        assert_eq!(read_parameter_value(&json!([1, 0.5, 0, 0.25])).unwrap(), json!([1, 0.5, 0, 0.25]));
        for invalid in [json!("2"), json!([1, 2]), json!([1, 2, null]), json!({"kind": "mesh"})] {
            assert!(read_parameter_value(&invalid).is_err(), "{invalid}");
        }
        let mesh = mesh_box_value(1.0, 1.0, 1.0).unwrap();
        assert_eq!(read_parameter_value(&mesh).unwrap(), mesh);
        assert_eq!(read_materials(None).unwrap(), vec![default_material()]);
    }
}
