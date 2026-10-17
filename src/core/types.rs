// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Shared numeric contracts: plain records every layer can name.
//!
//! Scalars use `f64` to retain authored numerical precision; converting to `f32` for GPU upload is the
//! consumer's job. Units, coordinate spaces, and gameplay meaning belong to each consuming API, which documents them.
//! Records preserve the established serialized field names for compatible JSON interchange.
//!
//! Renderer-facing records that reference scene nodes, prepared environments, point lights, or shadow cameras belong
//! to the rendering layer and are intentionally not part of these foundation contracts.

use serde::{
    Deserialize,
    Serialize,
};

use crate::core::JsonRecord;

// ---------------------------------------------------------------------------------------------------------------------
// Vectors and transforms
// ---------------------------------------------------------------------------------------------------------------------

/// A 2D position in the caller's coordinate space.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct Vector2 {
    /// Horizontal component.
    pub x: f64,
    /// Vertical component.
    pub y: f64,
}

impl Vector2 {
    /// Create a vector from its components.
    pub const fn new(x: f64, y: f64) -> Self {
        Self { x, y }
    }
}

/// A 2D velocity in the caller's units per second (or per tick, as the consuming API documents).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Velocity2 {
    /// Horizontal velocity.
    pub velocity_x: f64,
    /// Vertical velocity.
    pub velocity_y: f64,
}

/// A 2D body with position and velocity (the TypeScript `Vector2 & Velocity2`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MovingBody {
    /// Horizontal position.
    pub x: f64,
    /// Vertical position.
    pub y: f64,
    /// Horizontal velocity.
    pub velocity_x: f64,
    /// Vertical velocity.
    pub velocity_y: f64,
}

impl MovingBody {
    /// The body's position as a [`Vector2`].
    pub const fn position(&self) -> Vector2 {
        Vector2 { x: self.x, y: self.y }
    }

    /// The body's velocity as a [`Velocity2`].
    pub const fn velocity(&self) -> Velocity2 {
        Velocity2 { velocity_x: self.velocity_x, velocity_y: self.velocity_y }
    }
}

/// Ordered X/Y/Z components in the coordinate space documented by the consuming API.
pub type Vector3 = [f64; 3];

/// Column-major affine transform: three scaled basis columns (X, Y, then Z) followed by the XYZ translation.
///
/// Maps local XYZ positions into the coordinate space documented by the consuming API and supports shear.
pub type AffineTransform3D = [f64; 12];

/// Straight-alpha RGBA color with JavaScript-number channels, usually in `[0, 1]`.
///
/// This is the foundation-layer color record shared with the TypeScript engine; GPU-facing code uses the `f32`
/// [`Rgba`](crate::core::Rgba).
pub type Color = [f64; 4];

// ---------------------------------------------------------------------------------------------------------------------
// Meshes
// ---------------------------------------------------------------------------------------------------------------------

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
/// Fields serialize with the TypeScript names; absent optional attributes are omitted.
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

/// Conservative axis-aligned bounds in the coordinate space of the consuming API.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct GeometryBounds {
    /// Smallest X/Y/Z corner.
    pub min: Vector3,
    /// Largest X/Y/Z corner.
    pub max: Vector3,
}

// ---------------------------------------------------------------------------------------------------------------------
// Materials
// ---------------------------------------------------------------------------------------------------------------------

/// glTF-compatible material alpha modes, serialized with the TypeScript `MATERIAL_ALPHA_MODE` spellings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum MaterialAlphaMode {
    /// Alpha is ignored.
    #[default]
    Opaque,
    /// Fragments below the alpha cutoff are discarded.
    Mask,
    /// Alpha blends the surface over what is behind it.
    Blend,
}

/// Borrowed texture sources (URLs) for one PBR material; the renderer owns decoded GPU textures.
///
/// Color maps use sRGB; normal and scalar maps use linear data (scalar maps read the red channel).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PbrMaterialTextures {
    /// Base color map.
    pub base_color: Option<String>,
    /// Tangent-space normal map.
    pub normal: Option<String>,
    /// Emissive map.
    pub emissive: Option<String>,
    /// Metallic map.
    pub metallic: Option<String>,
    /// Roughness map.
    pub roughness: Option<String>,
    /// Ambient occlusion map.
    pub ao: Option<String>,
}

/// Metallic-roughness material. Factors are linear; colors have RGB or RGBA order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PbrMaterial {
    /// Linear RGB or RGBA base color factor.
    pub base_color: Vec<f64>,
    /// Linear RGB emissive factor.
    pub emissive: Vec<f64>,
    /// Metalness factor in `[0, 1]`.
    pub metallic: f64,
    /// Perceptual roughness factor in `[0, 1]`.
    pub roughness: f64,
    /// Alpha handling.
    pub alpha_mode: MaterialAlphaMode,
    /// Alpha threshold for [`MaterialAlphaMode::Mask`].
    pub alpha_cutoff: f64,
    /// Optional texture sources.
    pub textures: PbrMaterialTextures,
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

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn records_use_typescript_field_names() {
        let body = MovingBody { x: 1.0, y: 2.0, velocity_x: 3.0, velocity_y: 4.0 };
        assert_eq!(
            serde_json::to_value(body).unwrap(),
            json!({"x": 1.0, "y": 2.0, "velocityX": 3.0, "velocityY": 4.0})
        );
        assert_eq!(body.position(), Vector2::new(1.0, 2.0));

        let mesh = MeshAssetData { positions: vec![0.0], indices: vec![0], ..Default::default() };
        let value = serde_json::to_value(&mesh).unwrap();
        assert_eq!(value.as_object().unwrap().keys().collect::<Vec<_>>(), ["positions", "indices"]);
        assert_eq!(serde_json::to_value(MaterialAlphaMode::Blend).unwrap(), json!("BLEND"));
    }
}
