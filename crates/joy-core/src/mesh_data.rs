// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Plain mesh records shared by geometry producers (FORM, glTF import, destruction, procedural games) and the renderer:
//! indexed triangle meshes, flat-shaded triangle lists, and the mesh asset input with optional vertex attributes.
//!
//! Coordinates are `f64` to keep authored and generated precision; converting to `f32` for GPU upload is the
//! renderer's job. Field names serialize in camelCase for authored and exported JSON.

use serde::{
    Deserialize,
    Serialize,
};

use crate::JsonRecord;
use crate::material::PbrMaterial;

/// A named indexed triangle mesh in model space: flat XYZ positions and normals with one vertex order.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct IndexedMesh3D {
    /// Mesh name (glTF meshes without a name use `mesh`).
    pub name: String,
    /// Vertex positions as XYZ triples.
    pub positions: Vec<f64>,
    /// Vertex normals as XYZ triples, one per vertex.
    pub normals: Vec<f64>,
    /// Triangle vertex indices, three per triangle.
    pub indices: Vec<u32>,
}

/// One flat-shaded triangle: three XYZ positions (nine coordinates) followed by a face brightness multiplier.
pub type ShadedTriangle3D = [f64; 10];

/// A named list of flat-shaded triangles.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct ShadedMesh3D {
    /// Mesh name.
    pub name: String,
    /// Triangles: nine coordinates then a shade factor each.
    pub triangles: Vec<ShadedTriangle3D>,
}

/// Local Y-up indexed mesh input for mesh assets. Optional attributes are absent or vertex-aligned.
///
/// Fields serialize with camelCase document names; absent optional attributes are omitted.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MeshAssetData {
    /// Vertex positions as XYZ triples.
    pub positions: Vec<f64>,
    /// Triangle vertex indices, three per triangle.
    pub indices: Vec<u32>,
    /// Vertex normals as XYZ triples; zero normals request a flat face normal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normals: Option<Vec<f64>>,
    /// Texture coordinates as UV pairs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uvs: Option<Vec<f64>>,
    /// Linear vertex colors as RGB triples.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colors: Option<Vec<f64>>,
    /// Vertex opacity, one value per vertex.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub alphas: Option<Vec<f64>>,
    /// Material palette index for each triangle.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub triangle_materials: Option<Vec<u32>>,
    /// Material records, interpreted by the consuming renderer.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub materials: Option<Vec<serde_json::Value>>,
    /// Free-form metadata preserved for tools.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metadata: Option<JsonRecord>,
}

/// Indexed, world-space Y-up geometry with a material palette. Zero normals request a flat face normal.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PbrMesh {
    /// Vertex positions as XYZ triples.
    pub positions: Vec<f64>,
    /// Vertex normals as XYZ triples.
    pub normals: Vec<f64>,
    /// Texture coordinates as UV pairs.
    pub uvs: Vec<f64>,
    /// Linear vertex colors as RGB triples.
    pub colors: Vec<f64>,
    /// Vertex opacity, one value per vertex.
    pub alphas: Vec<f64>,
    /// Triangle vertex indices, three per triangle.
    pub triangles: Vec<u32>,
    /// Material palette index for each triangle.
    pub triangle_materials: Vec<u32>,
    /// Material palette.
    pub materials: Vec<PbrMaterial>,
}
