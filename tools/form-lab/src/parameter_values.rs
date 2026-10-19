// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Script parameters as the inspector sees them: typed descriptors read from a build, default and modified state,
//! the overrides property history covers, mesh sources (presets and geometry-only GLB import), and material resource
//! edits. These models stay independent of the parameter controls' UI.

use joy_engine::form::{
    mesh_box_value,
    mesh_sphere_value,
    read_material_value,
    read_mesh,
};
use joy_engine::serde_json::{
    Value as Json,
    json,
};

/// Largest GLB a mesh parameter imports, in bytes.
pub const MAXIMUM_GLB_BYTES: usize = 16_000_000;

/// Longest imported mesh name, in characters.
const MAXIMUM_MESH_NAME_LENGTH: usize = 120;

/// A parameter's declared type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ParameterKind {
    /// One number.
    Scalar,
    /// An XYZ vector.
    Vector,
    /// A linear RGBA color.
    Color,
    /// A mesh resource.
    Mesh,
    /// A material resource.
    Material,
}

impl ParameterKind {
    /// The short type badge the inspector shows (`S`, `V`, `C`, `MESH`, `MAT`).
    pub fn badge(self) -> &'static str {
        match self {
            Self::Scalar => "S",
            Self::Vector => "V",
            Self::Color => "C",
            Self::Mesh => "MESH",
            Self::Material => "MAT",
        }
    }

    /// The serialized type name.
    pub fn name(self) -> &'static str {
        match self {
            Self::Scalar => "scalar",
            Self::Vector => "vector",
            Self::Color => "color",
            Self::Mesh => "mesh",
            Self::Material => "material",
        }
    }

    /// Component labels of a numeric parameter.
    pub fn components(self) -> &'static [&'static str] {
        match self {
            Self::Scalar => &["Value"],
            Self::Vector => &["X", "Y", "Z"],
            Self::Color => &["R", "G", "B", "A"],
            Self::Mesh | Self::Material => &[],
        }
    }
}

/// A bounded scalar, vector, or color parameter from the latest accepted build.
#[derive(Clone, Debug, PartialEq)]
pub struct NumericParameter {
    /// Script name.
    pub name: String,
    /// Scalar, vector, or color.
    pub kind: ParameterKind,
    /// Inclusive lower bound for every component.
    pub minimum: f64,
    /// Inclusive upper bound for every component.
    pub maximum: f64,
    /// Control step.
    pub step: f64,
    /// The script's default components.
    pub default_value: Vec<f64>,
    /// The resolved components of the build.
    pub value: Vec<f64>,
}

impl NumericParameter {
    /// Whether the resolved value differs from the script default.
    pub fn modified(&self) -> bool {
        self.value.iter().zip(&self.default_value).any(|(value, default)| value != default)
    }

    /// A JSON override for components: a scalar for scalars and an array otherwise, with integral values as integers.
    pub fn override_value(&self, components: &[f64]) -> Json {
        if self.kind == ParameterKind::Scalar {
            joy_engine::core::json_number(components.first().copied().unwrap_or_default())
        } else {
            Json::Array(components.iter().map(|value| joy_engine::core::json_number(*value)).collect())
        }
    }
}

/// A mesh or material parameter from the latest accepted build.
#[derive(Clone, Debug, PartialEq)]
pub struct ResourceParameter {
    /// Script name.
    pub name: String,
    /// Mesh or material.
    pub kind: ParameterKind,
    /// The script's default resource.
    pub default_value: Json,
    /// The resolved resource of the build.
    pub value: Json,
}

/// One inspector parameter.
#[derive(Clone, Debug, PartialEq)]
pub enum Parameter {
    /// Scalars, vectors, and colors.
    Numeric(NumericParameter),
    /// Meshes and materials.
    Resource(ResourceParameter),
}

impl Parameter {
    /// The script name.
    pub fn name(&self) -> &str {
        match self {
            Self::Numeric(parameter) => &parameter.name,
            Self::Resource(parameter) => &parameter.name,
        }
    }

    /// The declared type.
    pub fn kind(&self) -> ParameterKind {
        match self {
            Self::Numeric(parameter) => parameter.kind,
            Self::Resource(parameter) => parameter.kind,
        }
    }
}

/// Typed parameters from a build's serialized descriptors; malformed descriptors are skipped.
pub fn read_parameters(descriptors: &[Json]) -> Vec<Parameter> {
    descriptors.iter().filter_map(read_parameter).collect()
}

fn read_parameter(descriptor: &Json) -> Option<Parameter> {
    let name = descriptor.get("name")?.as_str()?.to_owned();
    let kind = match descriptor.get("type")?.as_str()? {
        "scalar" => ParameterKind::Scalar,
        "vector" => ParameterKind::Vector,
        "color" => ParameterKind::Color,
        "mesh" => ParameterKind::Mesh,
        "material" => ParameterKind::Material,
        _ => return None,
    };
    if matches!(kind, ParameterKind::Mesh | ParameterKind::Material) {
        return Some(Parameter::Resource(ResourceParameter {
            name,
            kind,
            default_value: descriptor.get("defaultValue").cloned().unwrap_or(Json::Null),
            value: descriptor.get("value").cloned().unwrap_or(Json::Null),
        }));
    }
    let components = |value: &Json| -> Option<Vec<f64>> {
        match value {
            Json::Number(number) => Some(vec![number.as_f64()?]),
            Json::Array(values) => values.iter().map(Json::as_f64).collect(),
            _ => None,
        }
    };
    let number = |key: &str| descriptor.get(key).and_then(Json::as_f64);
    Some(Parameter::Numeric(NumericParameter {
        name,
        kind,
        minimum: number("min")?,
        maximum: number("max")?,
        step: number("step").unwrap_or(0.0),
        default_value: components(descriptor.get("defaultValue")?)?,
        value: components(descriptor.get("value")?)?,
    }))
}

/// A key that changes only when declarations change (names, types, bounds, defaults), so controls rebuild then and
/// otherwise keep their state across builds.
pub fn parameter_signature(parameters: &[Parameter]) -> String {
    let declarations: Vec<Json> = parameters
        .iter()
        .map(|parameter| match parameter {
            Parameter::Resource(resource) => json!({"name": resource.name, "type": resource.kind.name()}),
            Parameter::Numeric(numeric) => json!({
                "name": numeric.name, "type": numeric.kind.name(), "min": numeric.minimum, "max": numeric.maximum,
                "step": numeric.step, "defaultValue": numeric.default_value
            }),
        })
        .collect();
    Json::Array(declarations).to_string()
}

/// The inspector title of a parameter: underscores read as spaces, in capitals.
pub fn parameter_title(name: &str) -> String {
    name.replace('_', " ").to_uppercase()
}

/// Whether an override belongs to bounded property history: numbers, vectors, colors, and material factors.
/// Imported meshes stay outside it.
pub fn is_property_override(value: &Json) -> bool {
    value.is_number() || value.is_array() || value.get("kind").and_then(Json::as_str) == Some("material")
}

/// Built-in mesh sources a mesh parameter offers beside the current mesh.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeshPreset {
    /// `meshBox(1, 1, 1)`.
    Box,
    /// `meshSphere(1, 20)`.
    Sphere,
}

impl MeshPreset {
    /// The menu label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Box => "Box · unit size",
            Self::Sphere => "Sphere · unit radius",
        }
    }

    /// The mesh resource.
    pub fn value(self) -> Json {
        let value = match self {
            Self::Box => mesh_box_value(1.0, 1.0, 1.0),
            Self::Sphere => mesh_sphere_value(1.0, 20),
        };
        value.expect("unit presets are valid meshes")
    }
}

/// Import one triangle primitive's geometry from GLB bytes as a mesh resource named after the file (geometry only:
/// materials, transforms, and UVs are not read; UVs are zero).
pub fn import_glb_mesh(bytes: &[u8], file_name: &str) -> Result<Json, String> {
    if bytes.len() > MAXIMUM_GLB_BYTES {
        return Err("GLB input must be 16 MB or smaller.".into());
    }
    let decoded = joy_engine::dcc::parse_glb_mesh(bytes, None).map_err(|error| error.to_string())?;
    let name: String = file_name.chars().take(MAXIMUM_MESH_NAME_LENGTH).collect();
    let number = |value: f64| joy_engine::core::json_number(value);
    let mesh = json!({
        "kind": "mesh",
        "name": name,
        "positions": decoded.positions.iter().map(|value| number(*value)).collect::<Vec<_>>(),
        "normals": decoded.normals.iter().map(|value| number(*value)).collect::<Vec<_>>(),
        "uvs": vec![0; decoded.positions.len() / 3 * 2],
        "indices": decoded.indices,
    });
    read_mesh(&mesh).map_err(|error| error.0)
}

/// The mesh status line: counts and the import policy.
pub fn mesh_summary(mesh: &Json) -> String {
    let count = |key: &str| mesh.get(key).and_then(Json::as_array).map_or(0, Vec::len);
    format!(
        "{} vertices · {} triangles. GLB: one triangle primitive; geometry only.",
        count("positions") / 3,
        count("indices") / 3
    )
}

/// The display name of a mesh resource.
pub fn mesh_name(mesh: &Json) -> &str {
    mesh.get("name").and_then(Json::as_str).unwrap_or("Mesh")
}

/// A material resource with one base color channel changed; alpha below one switches to BLEND. Fails for a value
/// outside 0–1.
pub fn with_material_base_color(resource: &Json, channel: usize, value: f64) -> Result<Json, String> {
    let mut edited = read_material_value(resource).map_err(|error| error.0)?;
    let material = &mut edited["material"];
    material["baseColor"][channel] = joy_engine::core::json_number(value);
    let alpha = material["baseColor"][3].as_f64().unwrap_or(1.0);
    material["alphaMode"] = json!(if alpha < 1.0 { "BLEND" } else { "OPAQUE" });
    read_material_value(&edited).map_err(|error| error.0)
}

/// A material resource with all four base color channels replaced (for color pickers), alpha policy as above.
pub fn with_material_color(resource: &Json, color: [f64; 4]) -> Result<Json, String> {
    let mut edited = read_material_value(resource).map_err(|error| error.0)?;
    edited["material"]["baseColor"] =
        Json::Array(color.iter().map(|value| joy_engine::core::json_number(*value)).collect());
    edited["material"]["alphaMode"] = json!(if color[3] < 1.0 { "BLEND" } else { "OPAQUE" });
    read_material_value(&edited).map_err(|error| error.0)
}

/// A material resource with its metallic or roughness factor changed.
pub fn with_material_factor(resource: &Json, factor: &str, value: f64) -> Result<Json, String> {
    let mut edited = read_material_value(resource).map_err(|error| error.0)?;
    edited["material"][factor] = joy_engine::core::json_number(value);
    read_material_value(&edited).map_err(|error| error.0)
}

/// A material resource's base color, metallic, and roughness.
pub fn material_factors(resource: &Json) -> ([f64; 4], f64, f64) {
    let material = &resource["material"];
    let mut color = [1.0; 4];
    for (index, slot) in color.iter_mut().enumerate() {
        *slot = material["baseColor"][index].as_f64().unwrap_or(1.0);
    }
    (color, material["metallic"].as_f64().unwrap_or(0.0), material["roughness"].as_f64().unwrap_or(1.0))
}
