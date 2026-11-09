// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! FORM, Joy Engine's procedural geometry language, without a renderer.
//!
//! This crate parses and evaluates `.form` scripts. Evaluation produces plain [`GeometryData`]
//! records: indexed, Y-up geometry in script units, plus materials, parameter descriptors, project
//! metadata, realized custom attributes, and named output snapshots. Renderers consume those
//! records; nothing here touches a GPU, window, worker, or timer.
//!
//! Scripts build geometry values explicitly and add them to the result with `output`; there is no
//! implicit drawing state.
//!
//! ```
//! use joy_form::CompiledScript;
//! use serde_json::json;
//!
//! let script = CompiledScript::parse("param width = 2 meta(min=1, max=8, step=1)\noutput(\"Slab\", box(width, 1, 1))")?;
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
//! # Cooking
//!
//! Release and browser builds can skip evaluation: a game's build script (`joy-cook`) evaluates its scripts ahead of
//! time into a bundle of [`encode_geometry`] records keyed by [`CookKey`] (a content hash of the sources, modules,
//! overrides, palette, and [`FORM_EVALUATOR_VERSION`]), and [`CookedForms::evaluate`] returns the cooked copy when the
//! inputs match, evaluating live otherwise. [`FormEvaluation::resolve`] layers a `.forminst` instance's overrides the way
//! every host does, so cooked keys and run-time keys agree.
//!
//! # Budgets
//!
//! Sources are limited to 200,000 characters and 40,000 tokens. Evaluation allows
//! 2,000,000 operations, 150,000 emitted vertices, and 180,000 emitted triangles. Limit
//! violations, malformed values, and unsupported functions return a [`Diagnostic`]; authored
//! content never panics.

mod ast;
mod builder;
mod cook_key;
mod cooked_forms;
mod cooked_geometry;
mod diagnostic;
mod evaluation;
mod evaluator;
mod geometry_data;
mod instance;
mod language_information;
mod materials;
mod math;
mod modules;
mod named_output;
mod parameter_descriptor;
mod parameters;
mod parser;
mod primitives;
mod procedural;
mod records;
mod resources;
mod scope;
mod tokenizer;
mod value;

use std::cell::RefCell;

use serde_json::Value as Json;

pub use cook_key::{
    CookKey,
    FORM_EVALUATOR_VERSION,
};
pub use cooked_forms::{
    COOKED_FORMS_FILE,
    CookedForms,
    write_cooked_bundle,
};
pub use cooked_geometry::{
    COOKED_GEOMETRY_VERSION,
    CookError,
    decode_geometry,
    encode_geometry,
};
pub use diagnostic::{
    Diagnostic,
    FormValueError,
    SourceToken,
};
pub use evaluation::{
    EvaluationError,
    FormEvaluation,
};
pub use geometry_data::{
    AttributeColumn,
    GeometryData,
};
pub use instance::{
    FORM_INSTANCE_EXTENSION,
    FORM_INSTANCE_VERSION,
    FormInstance,
    InstanceError,
    InstanceScript,
    MAXIMUM_NAME_LENGTH,
    MAXIMUM_SOURCE_LENGTH,
    MAXIMUM_VARIANT_NAME_LENGTH,
    MAXIMUM_VARIANTS,
    Overrides,
    ParameterVariant,
    read_overrides,
    read_variants,
};
pub use language_information::{
    FORM_ATTRIBUTE_DOMAINS,
    FORM_KEYWORDS,
    form_document_symbols,
    form_language_catalog,
};
pub use materials::{
    AlphaMode,
    FormMaterial,
    FormMaterialTextures,
};
pub use modules::{
    MODULE_EXTENSION,
    ModuleLoader,
};
pub use named_output::{
    NamedOutput,
    OutputValue,
};
pub use parameter_descriptor::{
    ParameterControl,
    parameter_control,
    parameter_group,
    parameter_info,
    parameter_label,
};
pub use primitives::{
    DEFAULT_SPHERE_SEGMENTS,
    mesh_box,
    mesh_sphere,
};

/// Largest FORM instance document, in bytes; capacity includes base64 texture expansion.
pub const DOCUMENT_FILE_BYTES: usize = 64_000_000;

/// Largest embedded material texture, in decoded bytes.
pub const DOCUMENT_TEXTURE_BYTES: usize = materials::MAX_TEXTURE_BYTES;

/// Largest combined embedded texture size of one material palette, in decoded bytes.
pub const DOCUMENT_TOTAL_TEXTURE_BYTES: usize = materials::MAX_TOTAL_TEXTURE_BYTES;

/// A parsed FORM script that can be evaluated any number of times.
///
/// Parsing validates syntax and source budgets without executing authored code. The compiled
/// script owns its syntax tree and retains no evaluation state, so evaluations are independent
/// and deterministic for identical inputs.
#[derive(Debug)]
pub struct CompiledScript {
    program: modules::Program,
    // The main source and every module's path and source, hashed while parsing: the source half of a `CookKey`.
    source_fingerprint: cook_key::SourceFingerprint,
}

impl CompiledScript {
    /// Parse FORM source text that uses no modules.
    ///
    /// Returns a [`Diagnostic`] with a one-based line and character column when the source is
    /// malformed or exceeds the source, token, or nesting budgets, or when it declares `use`. An
    /// empty source is valid.
    pub fn parse(source: &str) -> Result<Self, Diagnostic> {
        Ok(Self {
            program: modules::build(source, None, None)?,
            source_fingerprint: cook_key::SourceFingerprint::new(source, []),
        })
    }

    /// Parse FORM source text and every module it names with `use`.
    ///
    /// `path` is the script's own location relative to the loader's root (for example
    /// `assets/form/city/block.form`); module paths resolve relative to the folder of the script that
    /// uses them and may not leave the root. `loader` is called once per distinct module while
    /// parsing; evaluation never loads files. A module failure is reported at the `use` declaration
    /// that reached it, with the module path, line, and column in the message.
    pub fn parse_with_modules(source: &str, path: &str, loader: &dyn ModuleLoader) -> Result<Self, Diagnostic> {
        // Record each module the parser loads, in load order, so the fingerprint covers exactly what was read.
        let loaded: RefCell<Vec<(String, String)>> = RefCell::new(Vec::new());
        let recording = |module: &str| -> Result<String, String> {
            let text = loader.load(module)?;
            loaded.borrow_mut().push((module.to_owned(), text.clone()));
            Ok(text)
        };
        let program = modules::build(source, Some(path), Some(&recording))?;
        let loaded = loaded.into_inner();
        let modules = loaded.iter().map(|(module, text)| (module.as_str(), text.as_str()));
        Ok(Self {
            program,
            source_fingerprint: cook_key::SourceFingerprint::new(source, modules),
        })
    }

    /// The normalized paths of every module this script uses, directly or through other modules, in load order.
    /// Hosts that hot reload a script also watch these files.
    pub fn module_paths(&self) -> Vec<&str> {
        self.program.modules.iter().skip(1).filter_map(|module| module.path.as_deref()).collect()
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
        evaluator::evaluate(&self.program, overrides, materials)
    }

    /// The content key of evaluating this script with `overrides` and `materials` under this evaluator build: what
    /// cooked FORM bundles are indexed by (see [`CookKey`]).
    pub fn cook_key(&self, overrides: &Json, materials: Option<&Json>) -> CookKey {
        cook_key::evaluation_key(self.source_fingerprint, overrides, materials)
    }

    pub(crate) fn source_fingerprint(&self) -> cook_key::SourceFingerprint {
        self.source_fingerprint
    }
}

/// Parse and evaluate FORM source in one step.
///
/// Equivalent to [`CompiledScript::parse`] followed by [`CompiledScript::evaluate`]; use a
/// [`CompiledScript`] directly when evaluating the same source with several overrides.
pub fn compile(source: &str, overrides: &Json, materials: Option<&Json>) -> Result<GeometryData, Diagnostic> {
    CompiledScript::parse(source)?.evaluate(overrides, materials)
}

/// The editable FORM default material record: white, fully rough, opaque, without textures.
pub fn default_material() -> Json {
    materials::default_material()
}

/// Validate a serialized material palette and return an owned copy with the Default material first.
///
/// `None` yields the Default material alone. Names must be unique ASCII identifiers; factors lie in 0–1; textures are
/// embedded PNG, JPEG, or WebP data URLs within [`DOCUMENT_TEXTURE_BYTES`] each and
/// [`DOCUMENT_TOTAL_TEXTURE_BYTES`] together; at most 128 materials.
pub fn read_materials(materials: Option<&Json>) -> Result<Vec<Json>, FormValueError> {
    materials::read_materials(materials).map_err(FormValueError)
}

/// Validate one parameter override and return an owned copy: a finite scalar, an XYZ vector, an RGBA color, true or
/// false, one line of text (a text parameter, or an option or enum member by name), a mesh resource, or a material
/// resource. Numbers use the document number spelling, so integral values stay JSON integers.
pub fn read_parameter_value(value: &Json) -> Result<Json, FormValueError> {
    match value {
        Json::Bool(_) => return Ok(value.clone()),
        Json::String(text) => return parameters::read_text(text).map(Json::String).map_err(FormValueError),
        _ => {}
    }
    let parsed = parameters::read_json(value).map_err(FormValueError)?;
    Ok(match parsed {
        value::Value::Number(number) => joy_core::json_number(number),
        value::Value::Array(components) => Json::Array(components.into_iter().map(joy_core::json_number).collect()),
        other => other.json(),
    })
}

/// Validate a serialized mesh resource: complete XYZ vertices within ±1,000,000, matching normals and
/// UVs, and complete triangles with valid indices.
pub fn read_mesh(value: &Json) -> Result<Json, FormValueError> {
    resources::read_mesh(value).map_err(FormValueError)
}

/// Validate a serialized material resource (`{"kind": "material", "material": …}`).
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
/// rename a project without reformatting its metadata. Fails with the parser's diagnostic for invalid source; `use`
/// declarations are not loaded.
pub fn project_name_tokens(source: &str) -> Result<Vec<SourceToken>, Diagnostic> {
    let statements = parser::parse(source)?;
    Ok(statements
        .iter()
        .filter_map(|statement| match &statement.kind {
            ast::StmtKind::Project {
                name,
                ..
            } => Some(SourceToken {
                text: name.text.clone(),
                line: name.line,
                column: name.column,
            }),
            _ => None,
        })
        .collect())
}

#[cfg(test)]
#[path = "lib.test.rs"]
mod tests;
