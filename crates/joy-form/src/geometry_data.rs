// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The owned geometry record every evaluation returns.

use serde::{
    Deserialize,
    Serialize,
};
use serde_json::Value as Json;

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

    /// The evaluated triangles as an engine [`Mesh`]: one material slot per palette entry (named after it), each
    /// triangle in its palette slot, vertex colors with their alphas, and normals and texture coordinates when every
    /// vertex has one. Isolated points, parameters, outputs, and per-vertex user data are FORM's own records and are
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
