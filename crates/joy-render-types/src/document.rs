// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Mesh documents: a mesh with the materials its slots start with, as a renderable asset is saved and exchanged
//! (baked FORM output, tool exports, and hand-authored props).
//!
//! The JSON form is `{"version": 1, "mesh": {…}, "materials": [{…}, …]}`: the [`Mesh`] and [`Material`] records as they
//! serialize on their own, so a document reads like the types it holds.

use serde::{
    Deserialize,
    Serialize,
};

use crate::material::{
    Material,
    MaterialError,
};
use crate::mesh::{
    Mesh,
    MeshError,
};

/// The mesh document format this engine writes and reads.
pub const MESH_DOCUMENT_VERSION: u32 = 1;

/// A mesh and one material per slot, in slot order.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MeshDocument {
    /// Format version; [`MESH_DOCUMENT_VERSION`].
    pub version: u32,
    /// The geometry.
    pub mesh: Mesh,
    /// Each slot's material, in slot order. Slots past the end draw with the default material.
    #[serde(default)]
    pub materials: Vec<Material>,
}

impl MeshDocument {
    /// A current-version document.
    pub fn new(mesh: Mesh, materials: Vec<Material>) -> Self {
        Self {
            version: MESH_DOCUMENT_VERSION,
            mesh,
            materials,
        }
    }

    /// Check the version, the mesh, and every material.
    pub fn validate(&self) -> Result<(), MeshDocumentError> {
        if self.version != MESH_DOCUMENT_VERSION {
            return Err(MeshDocumentError::UnsupportedVersion(self.version));
        }
        self.mesh.validate()?;
        for (slot, material) in self.materials.iter().enumerate() {
            material.validate().map_err(|error| MeshDocumentError::Material {
                slot,
                error,
            })?;
        }
        Ok(())
    }
}

/// Why a mesh document cannot be used.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum MeshDocumentError {
    /// The document was written by a newer or unknown format.
    #[error("Mesh document version {0} is not supported (expected {MESH_DOCUMENT_VERSION}).")]
    UnsupportedVersion(u32),
    /// The mesh is malformed.
    #[error(transparent)]
    Mesh(#[from] MeshError),
    /// A slot's material is malformed.
    #[error("Mesh document material {slot}: {error}")]
    Material {
        /// The slot whose material failed.
        slot: usize,
        /// Why it failed.
        error: MaterialError,
    },
}
