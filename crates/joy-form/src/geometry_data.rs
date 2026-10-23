// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The owned geometry record every evaluation returns.

use serde::{
    Deserialize,
    Serialize,
};
use serde_json::Value as Json;
use std::collections::BTreeMap;

use joy_core::LinearColor;
use joy_core::math::{
    DVec3,
    Vec2,
    Vec3,
};
use joy_render_types::{
    MaterialSlot,
    Mesh,
};

use crate::materials::FormMaterial;
use crate::named_output::NamedOutput;

/// Indexed, Y-up geometry produced by a FORM evaluation.
///
/// Distances use script units. Per-vertex arrays are flat and share one vertex order:
/// `positions` and `normals` hold three components per vertex, `uvs` two, `colors` three, and
/// `alphas` one. The JSON form uses camelCase document field names; empty attribute maps and an
/// absent `outputs` are omitted, and an absent `project` is `null`.
///
/// The record is plain owned data. The caller owns it outright; it borrows nothing from the
/// script, the overrides, or other evaluations.
#[derive(Serialize, Deserialize, Clone, Debug, Default, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct GeometryData {
    /// Vertex positions as XYZ triples in script units.
    pub positions: Vec<f64>,
    /// Unit vertex normals as XYZ triples. A zero vector marks an unauthored direction, which
    /// renderers shade with the triangle's flat face normal.
    pub normals: Vec<f64>,
    /// Texture coordinates as UV pairs.
    pub uvs: Vec<f64>,
    /// Vertex tint as RGB triples in 0–1.
    pub colors: Vec<f64>,
    /// Vertex opacity in 0–1, one value per vertex.
    pub alphas: Vec<f64>,
    /// Triangle vertex indices, three per triangle, in authored winding order.
    pub triangles: Vec<u32>,
    /// Vertex indices drawn as isolated points.
    pub points: Vec<u32>,
    /// Material palette index for each triangle.
    pub triangle_materials: Vec<u32>,
    /// Material palette index for each entry in `points`.
    pub point_materials: Vec<u32>,
    /// Serialized material records with Default first; see [`GeometryData::form_materials`].
    pub materials: Vec<Json>,
    /// Serialized parameter descriptors (name, type, bounds, default, and resolved value).
    pub parameters: Vec<Json>,
    /// Project metadata (`name`, optional `title` and `info`), when the script declares it.
    pub project: Option<Json>,
    /// Custom point and corner attributes realized per vertex, keyed by attribute name. Every column has one value
    /// per vertex; parts that lacked an attribute hold its default (zero, empty text, or false). Isolated points that
    /// carry attributes are how scripts place objects and record gameplay data.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub vertex_attributes: BTreeMap<String, AttributeColumn>,
    /// Custom face attributes realized per triangle, keyed by attribute name, one value per triangle.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub triangle_attributes: BTreeMap<String, AttributeColumn>,
    /// Named outputs and inspections in declaration order, present only when the script declares one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outputs: Option<Vec<NamedOutput>>,
}

/// One realized custom attribute: a value per vertex or per triangle.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum AttributeColumn {
    /// Numbers, XYZ vectors, or RGBA colors: `size` components per element, flattened.
    Number {
        /// Components per element: 1, 3, or 4.
        size: usize,
        /// Every element's components in element order.
        values: Vec<f64>,
    },
    /// Text per element; enum members are stored by name.
    Text {
        /// One text per element.
        values: Vec<String>,
    },
    /// True or false per element.
    Bool {
        /// One flag per element.
        values: Vec<bool>,
    },
}

/// One custom attribute element, borrowed while realization copies it into a column.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum AttributeValue<'a> {
    /// One, three, or four components.
    Numbers(&'a [f64]),
    Text(&'a str),
    Bool(bool),
}

impl AttributeColumn {
    /// The number of elements the column holds.
    pub fn len(&self) -> usize {
        match self {
            Self::Number {
                size,
                values,
            } => values.len() / (*size).max(1),
            Self::Text {
                values,
            } => values.len(),
            Self::Bool {
                values,
            } => values.len(),
        }
    }

    /// Whether the column holds no elements.
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Element `index` as JSON: a number, an array of components, text, or a boolean. `None` past the end.
    pub fn json_at(&self, index: usize) -> Option<Json> {
        match self {
            Self::Number {
                size,
                values,
            } => {
                let components = values.get((index * size)..((index + 1) * size))?;
                Some(if *size == 1 { Json::from(components[0]) } else { Json::from(components.to_vec()) })
            }
            Self::Text {
                values,
            } => values.get(index).map(|text| Json::String(text.clone())),
            Self::Bool {
                values,
            } => values.get(index).map(|flag| Json::Bool(*flag)),
        }
    }

    /// A column shaped for `value` holding `length` default elements.
    pub(crate) fn for_value(value: AttributeValue<'_>, length: usize) -> Self {
        let mut column = match value {
            AttributeValue::Numbers(components) => Self::Number {
                size: components.len(),
                values: Vec::new(),
            },
            AttributeValue::Text(_) => Self::Text {
                values: Vec::new(),
            },
            AttributeValue::Bool(_) => Self::Bool {
                values: Vec::new(),
            },
        };
        for _ in 0..length {
            column.push_default();
        }
        column
    }

    /// Element `index` borrowed from the column.
    pub(crate) fn value_at(&self, index: usize) -> AttributeValue<'_> {
        match self {
            Self::Number {
                size,
                values,
            } => AttributeValue::Numbers(&values[(index * size)..((index + 1) * size)]),
            Self::Text {
                values,
            } => AttributeValue::Text(&values[index]),
            Self::Bool {
                values,
            } => AttributeValue::Bool(values[index]),
        }
    }

    /// A column of the same type holding `length` default elements.
    pub(crate) fn defaults_like(&self, length: usize) -> Self {
        let mut column = match self {
            Self::Number {
                size,
                ..
            } => Self::Number {
                size: *size,
                values: Vec::new(),
            },
            Self::Text {
                ..
            } => Self::Text {
                values: Vec::new(),
            },
            Self::Bool {
                ..
            } => Self::Bool {
                values: Vec::new(),
            },
        };
        for _ in 0..length {
            column.push_default();
        }
        column
    }

    /// Append `value`, or return a description of the column's type when `value` does not match it.
    pub(crate) fn push(&mut self, value: AttributeValue<'_>) -> Result<(), String> {
        match (self, value) {
            (
                Self::Number {
                    size,
                    values,
                },
                AttributeValue::Numbers(components),
            ) if components.len() == *size => values.extend_from_slice(components),
            (
                Self::Text {
                    values,
                },
                AttributeValue::Text(text),
            ) => values.push(text.to_owned()),
            (
                Self::Bool {
                    values,
                },
                AttributeValue::Bool(flag),
            ) => values.push(flag),
            (column, _) => return Err(column.description()),
        }
        Ok(())
    }

    /// Append the default element: zero components, empty text, or false.
    pub(crate) fn push_default(&mut self) {
        match self {
            Self::Number {
                size,
                values,
            } => values.extend(std::iter::repeat_n(0.0, *size)),
            Self::Text {
                values,
            } => values.push(String::new()),
            Self::Bool {
                values,
            } => values.push(false),
        }
    }

    fn description(&self) -> String {
        match self {
            Self::Number {
                size: 1,
                ..
            } => "numbers".into(),
            Self::Number {
                size,
                ..
            } => format!("{size}-component values"),
            Self::Text {
                ..
            } => "text".into(),
            Self::Bool {
                ..
            } => "true/false".into(),
        }
    }
}

impl GeometryData {
    /// Every custom attribute of vertex `index` as a JSON record, in attribute-name order. Empty when the vertex has
    /// no custom attributes or does not exist.
    pub fn vertex_record(&self, index: usize) -> serde_json::Map<String, Json> {
        self.vertex_attributes
            .iter()
            .filter_map(|(name, column)| column.json_at(index).map(|value| (name.clone(), value)))
            .collect()
    }

    /// Every custom attribute of triangle `index` as a JSON record, in attribute-name order.
    pub fn triangle_record(&self, index: usize) -> serde_json::Map<String, Json> {
        self.triangle_attributes
            .iter()
            .filter_map(|(name, column)| column.json_at(index).map(|value| (name.clone(), value)))
            .collect()
    }

    /// Return typed copies of the material palette for renderers.
    ///
    /// The result has one entry per `materials` record, in palette order, so
    /// `triangle_materials` and `point_materials` index it directly. Evaluated palettes are
    /// already validated; a missing or malformed field (for example, after manual editing)
    /// falls back to the `FormMaterial::default()` value for that field.
    pub fn form_materials(&self) -> Vec<FormMaterial> {
        self.materials.iter().map(FormMaterial::from_json).collect()
    }

    /// The evaluated triangles as an engine [`Mesh`]: one material slot per palette entry (named after it), each
    /// triangle in its palette slot, vertex colors with their alphas, and normals and texture coordinates when every
    /// vertex has one. Isolated points, parameters, outputs, and custom attributes are FORM's own records and are
    /// not part of the mesh. Positions keep their `f64` precision; other attributes narrow to `f32`.
    pub fn to_mesh(&self) -> Mesh {
        let vertex_count = self.positions.len() / 3;
        let triples = |values: &[f64]| -> Vec<[f64; 3]> { values.as_chunks::<3>().0.to_vec() };
        let normals = (self.normals.len() == vertex_count * 3).then(|| {
            triples(&self.normals).into_iter().map(|[x, y, z]| Vec3::new(x as f32, y as f32, z as f32)).collect()
        });
        let uvs = (self.uvs.len() == vertex_count * 2)
            .then(|| self.uvs.as_chunks::<2>().0.iter().map(|[u, v]| Vec2::new(*u as f32, *v as f32)).collect());
        let colors = (self.colors.len() == vertex_count * 3).then(|| {
            triples(&self.colors)
                .into_iter()
                .enumerate()
                .map(|(index, [red, green, blue])| {
                    let alpha = self.alphas.get(index).copied().unwrap_or(1.0);
                    LinearColor::new(red as f32, green as f32, blue as f32, alpha as f32)
                })
                .collect()
        });
        let material_slots: Vec<MaterialSlot> =
            self.form_materials().into_iter().map(|material| MaterialSlot::named(material.name)).collect();
        let triangle_count = self.triangles.len() / 3;
        let triangle_slots = (self.triangle_materials.len() == triangle_count).then(|| self.triangle_materials.clone());
        Mesh {
            name: String::new(),
            positions: triples(&self.positions).into_iter().map(DVec3::from_array).collect(),
            normals,
            uvs,
            colors,
            indices: self.triangles.clone(),
            triangle_slots,
            material_slots,
            metadata: Default::default(),
        }
    }
}
