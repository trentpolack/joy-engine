// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! FORM evaluations as renderable assets and portable records: the layered mesh the scene draws, and the baked
//! `.mesh.json` document that lets baked mode draw without evaluating FORM.

use joy_engine::core::JsonRecord;
use joy_engine::form::GeometryData;
use joy_engine::prelude::*;
use joy_engine::render_types::MeshDocument;
use joy_engine::rendering::{
    RenderError,
    form_mesh_asset,
};
use joy_engine::serde_json::Value;

/// Metadata keys of a baked document's mesh.
const PROJECT_KEY: &str = "project";
const USER_DATA_KEY: &str = "userData";

/// Copy a completed FORM evaluation into an immutable mesh whose palette draws as layered materials; scene instances
/// share it. Fails only for empty geometry.
pub fn asset_from_evaluation(geometry: &GeometryData) -> Result<MeshAsset, RenderError> {
    form_mesh_asset(geometry, ShadingModel::Layered)
}

/// The baked document of an evaluation: the mesh with its palette slots, each slot's layered material, and the project
/// and per-vertex user data as mesh metadata.
pub fn document_from_evaluation(geometry: &GeometryData) -> MeshDocument {
    let mut mesh = geometry.to_mesh();
    let mut metadata = JsonRecord::new();
    metadata.insert(PROJECT_KEY.to_owned(), geometry.project.clone().unwrap_or(Value::Null));
    if let Some(user_data) = &geometry.user_data {
        metadata.insert(USER_DATA_KEY.to_owned(), Value::Array(user_data.clone()));
    }
    mesh.metadata = metadata;
    let materials =
        geometry.form_materials().iter().filter_map(|material| material.to_material(ShadingModel::Layered)).collect();
    MeshDocument::new(mesh, materials)
}

/// The asset a baked document draws, decoding any embedded material maps.
pub fn asset_from_document(document: &MeshDocument) -> Result<MeshAsset, RenderError> {
    MeshAsset::from_document(document, |source| TextureAsset::decode_data_url(source).ok())
}
