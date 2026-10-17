// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! FORM evaluations as renderable assets and portable records (TypeScript `asset.ts`): the PBR mesh the scene draws,
//! the baked `.mesh.json` record, and the reverse conversion that lets baked mode draw without evaluating FORM.

use joy_engine::core::{
    JsonRecord,
    MeshAssetData,
};
use joy_engine::form::GeometryData;
use joy_engine::prelude::*;
use joy_engine::rendering::{
    RenderError,
    form_mesh_asset,
};
use joy_engine::serde_json::Value;

/// Metadata keys of a baked record, matching the TypeScript `{project, userData}`.
const PROJECT_KEY: &str = "project";
const USER_DATA_KEY: &str = "userData";

/// Copy a completed FORM evaluation into an immutable mesh with its materials baked for PBR shading; scene instances
/// share it. Fails only for empty geometry.
pub fn asset_from_evaluation(geometry: &GeometryData) -> Result<MeshAsset, RenderError> {
    form_mesh_asset(geometry, Shading::Pbr)
}

/// The engine-local baked record of an evaluation (TypeScript `asset.geometry`): every channel, the material palette
/// and assignments, and the project and per-vertex user data as metadata.
pub fn mesh_data_from_evaluation(geometry: &GeometryData) -> MeshAssetData {
    let mut metadata = JsonRecord::new();
    metadata.insert(PROJECT_KEY.to_owned(), geometry.project.clone().unwrap_or(Value::Null));
    if let Some(user_data) = &geometry.user_data {
        metadata.insert(USER_DATA_KEY.to_owned(), Value::Array(user_data.clone()));
    }
    MeshAssetData {
        positions: geometry.positions.clone(),
        indices: geometry.triangles.clone(),
        normals: Some(geometry.normals.clone()),
        uvs: Some(geometry.uvs.clone()),
        colors: Some(geometry.colors.clone()),
        alphas: Some(geometry.alphas.clone()),
        triangle_materials: Some(geometry.triangle_materials.clone()),
        materials: Some(geometry.materials.clone()),
        metadata: Some(metadata),
    }
}

/// Geometry for a baked record, so it draws through the same FORM material conversion without parsing or evaluating
/// source. Missing channels stay empty (white, opaque, flat-shaded).
pub fn geometry_from_mesh_data(data: &MeshAssetData) -> GeometryData {
    let metadata = data.metadata.as_ref();
    let user_data = metadata.and_then(|metadata| metadata.get(USER_DATA_KEY)).and_then(Value::as_array).cloned();
    GeometryData {
        positions: data.positions.clone(),
        normals: data.normals.clone().unwrap_or_default(),
        uvs: data.uvs.clone().unwrap_or_default(),
        colors: data.colors.clone().unwrap_or_default(),
        alphas: data.alphas.clone().unwrap_or_default(),
        triangles: data.indices.clone(),
        triangle_materials: data.triangle_materials.clone().unwrap_or_default(),
        materials: data.materials.clone().unwrap_or_default(),
        project: metadata.and_then(|metadata| metadata.get(PROJECT_KEY)).filter(|value| !value.is_null()).cloned(),
        user_data,
        ..GeometryData::default()
    }
}
