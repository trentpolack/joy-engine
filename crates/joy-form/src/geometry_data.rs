// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The owned geometry record every evaluation returns.

use serde::{
    Deserialize,
    Serialize,
};
use serde_json::Value as Json;

use crate::materials::FormMaterial;

/// Indexed, Y-up geometry produced by a FORM evaluation.
///
/// Distances use script units. Per-vertex arrays are flat and share one vertex order:
/// `positions` and `normals` hold three components per vertex, `uvs` two, `colors` three, and
/// `alphas` one. The JSON form uses camelCase document field names; absent
/// `userData` and `outputs` are omitted and an absent `project` is `null`.
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
    /// Per-vertex authored user data, present only when any vertex carries a record.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub user_data: Option<Vec<Json>>,
    /// Named output and inspection snapshots, present only when the script declares one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub outputs: Option<Vec<Json>>,
}

impl GeometryData {
    /// Return typed copies of the material palette for renderers.
    ///
    /// The result has one entry per `materials` record, in palette order, so
    /// `triangle_materials` and `point_materials` index it directly. Evaluated palettes are
    /// already validated; a missing or malformed field (for example, after manual editing)
    /// falls back to the `FormMaterial::default()` value for that field.
    pub fn form_materials(&self) -> Vec<FormMaterial> {
        self.materials.iter().map(FormMaterial::from_json).collect()
    }
}
