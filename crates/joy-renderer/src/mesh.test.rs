// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/mesh.rs`.

use super::*;
use joy_core::math::DVec3;
use joy_render_types::{
    Material,
    MaterialSlot,
};

/// A quad of two triangles whose second triangle uses slot 1, with vertex colors and an unnormalized normal.
fn two_slot_quad(alpha: f32) -> Mesh {
    Mesh {
        positions: vec![DVec3::ZERO, DVec3::new(2.0, 0.0, 0.0), DVec3::new(2.0, 3.0, 0.0), DVec3::new(0.0, 3.0, 0.0)],
        normals: Some(vec![Vec3::new(0.0, 0.0, 4.0); 4]),
        colors: Some(vec![LinearColor::new(1.0, 0.5, 0.25, alpha); 4]),
        indices: vec![0, 2, 3, 0, 1, 2],
        triangle_slots: Some(vec![1, 0]),
        material_slots: vec![MaterialSlot::named("body"), MaterialSlot::named("trim")],
        ..Mesh::default()
    }
}

#[test]
fn assets_group_triangles_by_slot_and_keep_attributes() {
    let asset = MeshAsset::new(&two_slot_quad(1.0)).unwrap();

    // Slot 0's triangle comes first whatever the authored order.
    assert_eq!(
        asset.submeshes(),
        &[
            Submesh {
                indices: 0..3,
                slot: 0
            },
            Submesh {
                indices: 3..6,
                slot: 1
            }
        ]
    );
    assert_eq!(asset.indices(), &[0, 1, 2, 0, 2, 3]);
    assert_eq!(asset.vertices()[1].color, [1.0, 0.5, 0.25, 1.0]);
    assert_eq!(asset.vertices()[1].normal, [0.0, 0.0, 1.0]);
    assert_eq!(asset.bounds().max, Vec3::new(2.0, 3.0, 0.0));
    assert!(!asset.has_vertex_alpha());

    // Every slot has a default material until the asset supplies its own.
    assert_eq!(asset.slot_count(), 2);
    let trim = MaterialAsset::new(Material::unlit(LinearColor::WHITE)).unwrap();
    let painted = MeshAsset::with_materials(&two_slot_quad(0.5), vec![MaterialAsset::default(), trim.clone()]).unwrap();
    assert!(painted.materials()[1].ptr_eq(&trim));
    assert!(painted.has_vertex_alpha());
    assert_ne!(asset.id(), painted.id());
}

#[test]
fn invalid_meshes_are_rejected_before_upload() {
    let misaligned = Mesh {
        colors: Some(vec![LinearColor::WHITE; 3]),
        ..two_slot_quad(1.0)
    };
    assert!(matches!(MeshAsset::new(&misaligned), Err(RenderError::InvalidMesh(_))));

    // Positions beyond f32 range cannot be drawn even though they are finite doubles.
    let mut huge = two_slot_quad(1.0);
    huge.positions[0].x = 1e300;
    assert!(matches!(MeshAsset::new(&huge), Err(RenderError::InvalidMesh(_))));
}
