// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Indexed triangle meshes: the one geometry record every producer writes and every consumer reads.
//!
//! A [`Mesh`] is geometry and nothing else. Its triangles reference [`MaterialSlot`]s by index, and materials bind to
//! those slots later (a renderer asset's defaults, or a scene instance's overrides), so a mesh never names a material
//! or a shading model. FORM evaluations, glTF imports, destructible structures, baked documents, and procedural game
//! geometry all produce this record.
//!
//! Positions are `f64` ([`DVec3`]) so authored and generated geometry keeps its precision and deterministic producers
//! stay exact; renderers narrow them to `f32` when they upload. Normals, texture coordinates, and colors carry no such
//! requirement and use `f32` types. Optional attributes are absent or hold exactly one value per vertex.
//!
//! Records serialize with camelCase names; vectors are arrays (`[x, y, z]`) and colors are linear `[r, g, b, a]`.

use serde::{
    Deserialize,
    Serialize,
};

use joy_core::JsonRecord;
use joy_core::LinearColor;
use joy_core::math::{
    DVec3,
    Vec2,
    Vec3,
};

/// A named material slot. Triangles reference slots by index (see [`Mesh::triangle_slots`]).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MaterialSlot {
    /// Slot name for tools and for matching materials by name (FORM palette names, glTF material names); may be empty.
    #[serde(default)]
    pub name: String,
}

impl MaterialSlot {
    /// A slot named `name`.
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
        }
    }
}

/// An indexed triangle mesh in model space (Y-up, right-handed, world units).
///
/// The record is plain owned data; [`Mesh::validate`] checks it once at a boundary (an import, a document load, or a
/// renderer upload) and reports the first problem as a [`MeshError`].
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Mesh {
    /// Name for tools and diagnostics (a glTF mesh name, a FORM resource name); may be empty.
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub name: String,
    /// Vertex positions.
    pub positions: Vec<DVec3>,
    /// Unit vertex normals. A zero normal (or no normals at all) requests the triangle's flat face normal.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub normals: Option<Vec<Vec3>>,
    /// Texture coordinates with a top-left origin; they repeat outside `[0, 1]`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub uvs: Option<Vec<Vec2>>,
    /// Linear vertex colors with straight alpha. They multiply the material's base color; absent colors are white.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub colors: Option<Vec<LinearColor>>,
    /// Triangle list: three vertex indices per triangle, in authored winding order.
    pub indices: Vec<u32>,
    /// The material slot of each triangle; absent puts every triangle in slot 0.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub triangle_slots: Option<Vec<u32>>,
    /// Named material slots. An empty list means one unnamed slot.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub material_slots: Vec<MaterialSlot>,
    /// Free-form data producers attach and tools preserve without interpreting (FORM project data, chunk counts).
    #[serde(default, skip_serializing_if = "JsonRecord::is_empty")]
    pub metadata: JsonRecord,
}

impl Mesh {
    /// A mesh from positions and a triangle list, with no optional attributes and one unnamed slot.
    pub fn new(positions: Vec<DVec3>, indices: Vec<u32>) -> Self {
        Self {
            positions,
            indices,
            ..Self::default()
        }
    }

    /// Number of vertices.
    pub fn vertex_count(&self) -> usize {
        self.positions.len()
    }

    /// Number of complete triangles.
    pub fn triangle_count(&self) -> usize {
        self.indices.len() / 3
    }

    /// Number of material slots: the named slots, or one when none are named.
    pub fn slot_count(&self) -> usize {
        self.material_slots.len().max(1)
    }

    /// The material slot of triangle `triangle`; slot 0 when the mesh assigns none.
    pub fn triangle_slot(&self, triangle: usize) -> u32 {
        self.triangle_slots.as_ref().and_then(|slots| slots.get(triangle).copied()).unwrap_or(0)
    }

    /// The smallest box holding every position, as `(minimum, maximum)`; `None` for a mesh without vertices.
    pub fn bounds(&self) -> Option<(DVec3, DVec3)> {
        let first = *self.positions.first()?;
        Some(
            self.positions
                .iter()
                .fold((first, first), |(minimum, maximum), position| (minimum.min(*position), maximum.max(*position))),
        )
    }

    /// Check that the mesh can be drawn: at least one triangle, complete triangles, indices inside the vertex array,
    /// finite positions and attributes, optional attributes with one value per vertex, and slots that exist.
    pub fn validate(&self) -> Result<(), MeshError> {
        let vertex_count = self.positions.len();
        if vertex_count == 0 || self.indices.is_empty() {
            return Err(MeshError::Empty);
        }
        if !self.indices.len().is_multiple_of(3) {
            return Err(MeshError::IncompleteTriangle);
        }
        if let Some(index) = self.indices.iter().find(|index| **index as usize >= vertex_count) {
            return Err(MeshError::IndexOutOfRange {
                index: *index,
                vertex_count,
            });
        }

        // Optional attributes: one value per vertex, all finite.
        check_attribute(MeshAttribute::Normals, self.normals.as_ref().map(Vec::len), vertex_count)?;
        check_attribute(MeshAttribute::Uvs, self.uvs.as_ref().map(Vec::len), vertex_count)?;
        check_attribute(MeshAttribute::Colors, self.colors.as_ref().map(Vec::len), vertex_count)?;
        if !self.positions.iter().all(|position| position.is_finite()) {
            return Err(MeshError::NonFinite(MeshAttribute::Positions));
        }
        if self.normals.iter().flatten().any(|normal| !normal.is_finite()) {
            return Err(MeshError::NonFinite(MeshAttribute::Normals));
        }
        if self.uvs.iter().flatten().any(|uv| !uv.is_finite()) {
            return Err(MeshError::NonFinite(MeshAttribute::Uvs));
        }
        if self.colors.iter().flatten().any(|color| !color.to_array().iter().all(|channel| channel.is_finite())) {
            return Err(MeshError::NonFinite(MeshAttribute::Colors));
        }

        // Slots: one per triangle, each naming a slot that exists.
        if let Some(slots) = &self.triangle_slots {
            if slots.len() != self.triangle_count() {
                return Err(MeshError::Misaligned {
                    attribute: MeshAttribute::TriangleSlots,
                    expected: self.triangle_count(),
                    actual: slots.len(),
                });
            }
            let slot_count = self.slot_count();
            if let Some(slot) = slots.iter().find(|slot| **slot as usize >= slot_count) {
                return Err(MeshError::SlotOutOfRange {
                    slot: *slot,
                    slot_count,
                });
            }
        }
        Ok(())
    }
}

/// One of a mesh's per-vertex or per-triangle arrays, for error reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeshAttribute {
    /// [`Mesh::positions`].
    Positions,
    /// [`Mesh::normals`].
    Normals,
    /// [`Mesh::uvs`].
    Uvs,
    /// [`Mesh::colors`].
    Colors,
    /// [`Mesh::triangle_slots`].
    TriangleSlots,
}

impl std::fmt::Display for MeshAttribute {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::Positions => "positions",
            Self::Normals => "normals",
            Self::Uvs => "texture coordinates",
            Self::Colors => "colors",
            Self::TriangleSlots => "triangle slots",
        })
    }
}

/// Why a mesh cannot be drawn.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum MeshError {
    /// The mesh has no vertices or no triangles.
    #[error("A mesh needs at least one vertex and one triangle.")]
    Empty,
    /// The index count is not a multiple of three.
    #[error("Mesh indices must form complete triangles.")]
    IncompleteTriangle,
    /// A triangle references a vertex past the end of the vertex arrays.
    #[error("Mesh index {index} is outside its {vertex_count} vertices.")]
    IndexOutOfRange {
        /// The offending index.
        index: u32,
        /// The number of vertices.
        vertex_count: usize,
    },
    /// An optional array does not hold one value per vertex (or per triangle, for slots).
    #[error("Mesh {attribute} hold {actual} values; expected {expected}.")]
    Misaligned {
        /// The misaligned array.
        attribute: MeshAttribute,
        /// The required length.
        expected: usize,
        /// The array's length.
        actual: usize,
    },
    /// An array holds an infinite or NaN value.
    #[error("Mesh {0} must be finite.")]
    NonFinite(MeshAttribute),
    /// A triangle references a material slot the mesh does not have.
    #[error("Mesh triangle slot {slot} is outside its {slot_count} material slots.")]
    SlotOutOfRange {
        /// The offending slot.
        slot: u32,
        /// The number of slots.
        slot_count: usize,
    },
}

/// Check that an optional per-vertex attribute, when present, has one value per vertex.
fn check_attribute(attribute: MeshAttribute, length: Option<usize>, vertex_count: usize) -> Result<(), MeshError> {
    match length {
        Some(actual) if actual != vertex_count => Err(MeshError::Misaligned {
            attribute,
            expected: vertex_count,
            actual,
        }),
        _ => Ok(()),
    }
}

#[cfg(test)]
#[path = "mesh.test.rs"]
mod tests;
