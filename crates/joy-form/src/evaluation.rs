// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! One FORM evaluation's resolved inputs: what a `.form` script or `.forminst` instance asks the evaluator to run.
//!
//! Hosts (`FormAssetLibrary` for levels and tools) and the build-time cook resolve assets through
//! [`FormEvaluation::resolve`], so both layer overrides the same way and agree on every [`CookKey`]: a cooked copy is
//! found exactly when the runtime would have evaluated the same inputs.

use serde_json::{
    Map,
    Value as Json,
};

use crate::cook_key::{
    CookKey,
    evaluation_key,
};
use crate::geometry_data::GeometryData;
use crate::instance::{
    FORM_INSTANCE_EXTENSION,
    FormInstance,
};
use crate::modules::ModuleLoader;
use crate::{
    CompiledScript,
    Diagnostic,
};

/// Why an asset's evaluation inputs could not be resolved.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum EvaluationError {
    /// The instance document, its overrides, or the requested variant was rejected; the message names the problem.
    #[error("{0}")]
    Instance(String),
    /// The script or one of its modules failed to load or parse.
    #[error("{0}")]
    Form(Diagnostic),
}

/// A parsed script with its layered overrides and palette, ready to evaluate or look up cooked.
#[derive(Debug)]
pub struct FormEvaluation {
    script: CompiledScript,
    overrides: Json,
    materials: Option<Json>,
}

impl FormEvaluation {
    /// Inputs for `script` with `overrides` (a JSON object of parameter values) and an optional `materials` palette.
    pub fn new(script: CompiledScript, overrides: Json, materials: Option<Json>) -> Self {
        Self {
            script,
            overrides,
            materials,
        }
    }

    /// Resolve `text`, the contents of `path` (relative to the loader's root), the way levels and tools do.
    ///
    /// A `.forminst` instance supplies its referenced or packed script, its palette, and its overrides, then
    /// `variant`'s overrides (failing when it has no variant by that name), then `parameters`. A `.form` script takes
    /// `parameters` alone and refuses a variant. `loader` reads the instance's referenced script and every `use`
    /// module.
    pub fn resolve(
        path: &str,
        text: &str,
        loader: &dyn ModuleLoader,
        variant: Option<&str>,
        parameters: &Map<String, Json>,
    ) -> Result<Self, EvaluationError> {
        let (script, mut overrides, materials) = if path.ends_with(FORM_INSTANCE_EXTENSION) {
            let instance = FormInstance::read(text).map_err(|error| EvaluationError::Instance(error.to_string()))?;
            let script = instance.parse_script(path, loader).map_err(EvaluationError::Form)?;
            let overrides =
                instance.overrides_for(variant).map_err(|error| EvaluationError::Instance(error.to_string()))?;
            (script, overrides, instance.materials.map(Json::Array))
        } else {
            if let Some(name) = variant {
                return Err(EvaluationError::Instance(format!(
                    "variant \"{name}\" requires a FORM instance ({FORM_INSTANCE_EXTENSION})"
                )));
            }
            let script = CompiledScript::parse_with_modules(text, path, loader).map_err(EvaluationError::Form)?;
            (script, Map::new(), None)
        };
        overrides.extend(parameters.iter().map(|(name, value)| (name.clone(), value.clone())));
        Ok(Self::new(script, Json::Object(overrides), materials))
    }

    /// The parsed script and its modules.
    pub fn script(&self) -> &CompiledScript {
        &self.script
    }

    /// The layered parameter overrides the evaluation uses.
    pub fn overrides(&self) -> &Json {
        &self.overrides
    }

    /// The palette the evaluation uses, when the asset carries one.
    pub fn materials(&self) -> Option<&Json> {
        self.materials.as_ref()
    }

    /// The content key of these inputs under this evaluator build; see [`CookKey`].
    pub fn key(&self) -> CookKey {
        evaluation_key(self.script.source_fingerprint(), &self.overrides, self.materials.as_ref())
    }

    /// Evaluate live; see [`CompiledScript::evaluate`].
    pub fn evaluate(&self) -> Result<GeometryData, Diagnostic> {
        self.script.evaluate(&self.overrides, self.materials.as_ref())
    }
}
