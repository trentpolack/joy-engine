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
use joy_engine::serde_json::{
    self,
    Value,
};

/// Metadata keys of a baked document's mesh.
const PROJECT_KEY: &str = "project";
const VERTEX_ATTRIBUTES_KEY: &str = "vertexAttributes";
const TRIANGLE_ATTRIBUTES_KEY: &str = "triangleAttributes";

/// Copy a completed FORM evaluation into an immutable mesh whose palette draws as layered materials; scene instances
/// share it. Fails only for empty geometry.
pub fn asset_from_evaluation(geometry: &GeometryData) -> Result<MeshAsset, RenderError> {
    form_mesh_asset(geometry, ShadingModel::Layered)
}

/// The baked document of an evaluation: the mesh with its palette slots, each slot's layered material, and the project
/// and custom FORM attributes as mesh metadata.
///
/// Custom attributes keep their realized column form (`{"type", "size"?, "values"}` keyed by attribute name), so a
/// baked building still carries its per-triangle entrance records (`kind`, `index`, and `doorstep`). Absent attribute
/// maps are left out.
pub fn document_from_evaluation(geometry: &GeometryData) -> MeshDocument {
    let mut mesh = geometry.to_mesh();
    let mut metadata = JsonRecord::new();
    metadata.insert(PROJECT_KEY.to_owned(), geometry.project.clone().unwrap_or(Value::Null));

    let attribute_maps = [
        (VERTEX_ATTRIBUTES_KEY, &geometry.vertex_attributes),
        (TRIANGLE_ATTRIBUTES_KEY, &geometry.triangle_attributes),
    ];
    for (key, attributes) in attribute_maps {
        if attributes.is_empty() {
            continue;
        }

        // Attribute columns hold only numbers, text, and flags, so serialization cannot fail.
        let columns = serde_json::to_value(attributes).expect("FORM attribute columns serialize as JSON");
        metadata.insert(key.to_owned(), columns);
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
