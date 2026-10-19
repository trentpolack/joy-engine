// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Unit tests for `src/mesh.rs`.

use super::*;

fn triangle(alpha: f32) -> MeshAsset {
    MeshAsset::from_channels(MeshChannels {
        positions: &[0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        indices: &[0, 1, 2],
        alphas: &[alpha, alpha, alpha],
        ..Default::default()
    })
    .unwrap()
}

#[test]
fn assets_validate_and_partition_blending() {
    let opaque = triangle(1.0);
    assert_eq!(opaque.submeshes(), &[Submesh { indices: 0..3, blend: false, masked: false, textures: None }]);
    assert!(!opaque.has_vertex_alpha());

    let blended = triangle(0.5);
    assert_eq!(blended.submeshes(), &[Submesh { indices: 0..3, blend: true, masked: false, textures: None }]);
    assert!(blended.has_vertex_alpha());
    assert_ne!(opaque.id(), blended.id());
}

#[test]
fn shared_mesh_records_convert_with_their_colors() {
    let data = MeshAssetData {
        positions: vec![0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 3.0, 0.0],
        indices: vec![0, 1, 2],
        colors: Some([1.0, 0.5, 0.25].repeat(3)),
        ..Default::default()
    };
    let asset = MeshAsset::from_asset_data(&data).unwrap();
    assert_eq!(asset.data().vertices[1].color, [1.0, 0.5, 0.25, 1.0]);
    assert_eq!(asset.bounds().max, Vec3::new(2.0, 3.0, 0.0));
    assert!(!asset.has_vertex_alpha());
    let misaligned = MeshAssetData { colors: Some(vec![1.0; 4]), ..data };
    assert!(MeshAsset::from_asset_data(&misaligned).is_err());
}

fn pbr_material(alpha_mode: MaterialAlphaMode, base_color: Option<&str>) -> PbrMaterial {
    PbrMaterial {
        base_color: vec![0.5, 1.0, 1.0, 0.8],
        emissive: vec![0.1, 0.0, 0.0],
        metallic: 0.25,
        roughness: 0.75,
        alpha_mode,
        alpha_cutoff: 0.4,
        textures: joy_core::PbrMaterialTextures { base_color: base_color.map(str::to_owned), ..Default::default() },
    }
}

fn pbr_mesh(materials: Vec<PbrMaterial>, triangle_materials: Vec<u32>) -> PbrMesh {
    PbrMesh {
        positions: vec![0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 1.0],
        normals: vec![0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0],
        uvs: vec![0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        colors: vec![1.0; 12],
        alphas: vec![1.0, 1.0, 1.0, 0.5],
        triangles: vec![0, 2, 1, 1, 2, 3],
        triangle_materials,
        materials,
    }
}

#[test]
fn pbr_meshes_group_materials_and_bake_factors() {
    let texture = TextureAsset::from_rgba8(1, 1, vec![255; 4]).unwrap();
    let materials =
        vec![pbr_material(MaterialAlphaMode::Blend, None), pbr_material(MaterialAlphaMode::Mask, Some("cutout.png"))];
    let asset = MeshAsset::from_pbr_mesh(&pbr_mesh(materials, vec![0, 1]), |source| {
        (source == "cutout.png").then(|| texture.clone())
    })
    .unwrap();

    // Blended materials draw last; every triangle owns its three vertices.
    let submeshes = asset.submeshes();
    assert_eq!(submeshes.len(), 2);
    assert!(!submeshes[0].blend && submeshes[0].masked && submeshes[0].textures.is_some());
    assert!(submeshes[1].blend && !submeshes[1].masked && submeshes[1].textures.is_none());
    assert_eq!(asset.data().vertices.len(), 6);
    assert!(!asset.has_vertex_alpha(), "alpha belongs to the materials");

    // The masked triangle (1, 2, 3) comes first: factors multiply vertex color and alpha.
    let vertices = &asset.data().vertices;
    assert_eq!(vertices[0].position, [1.0, 0.0, 0.0]);
    assert_eq!(vertices[0].color, [0.5, 1.0, 1.0, 0.8]);
    assert_eq!(vertices[2].color[3], 0.5 * 0.8);
    assert_eq!(vertices[0].surface, [0.25, 0.75, 0.4, 1.0]);
    assert_eq!(vertices[0].emissive, [0.1, 0.0, 0.0, 0.0]);
    assert_eq!(vertices[2].uv, [1.0, 1.0]);
    // A zero authored normal takes the face normal; authored normals are normalized.
    assert_eq!(vertices[0].normal, [0.0, 1.0, 0.0]);
    assert_eq!(vertices[3].normal, [0.0, 1.0, 0.0]);
}

#[test]
fn pbr_meshes_reject_bad_documents() {
    let resolve = |_: &str| None;
    let missing = pbr_mesh(vec![pbr_material(MaterialAlphaMode::Opaque, Some("missing.png"))], vec![0, 0]);
    assert!(matches!(MeshAsset::from_pbr_mesh(&missing, resolve), Err(RenderError::InvalidMesh(_))));
    let unknown = pbr_mesh(vec![pbr_material(MaterialAlphaMode::Opaque, None)], vec![0, 3]);
    assert!(MeshAsset::from_pbr_mesh(&unknown, resolve).is_err());
    let mut misaligned = pbr_mesh(vec![pbr_material(MaterialAlphaMode::Opaque, None)], vec![0, 0]);
    misaligned.uvs.pop();
    assert!(MeshAsset::from_pbr_mesh(&misaligned, resolve).is_err());
    let mut rough = pbr_mesh(vec![pbr_material(MaterialAlphaMode::Opaque, None)], vec![0, 0]);
    rough.materials[0].roughness = 1.5;
    assert!(MeshAsset::from_pbr_mesh(&rough, resolve).is_err());
}

#[test]
fn malformed_geometry_is_rejected() {
    let result = MeshAsset::from_channels(MeshChannels {
        positions: &[0.0, 0.0, 0.0],
        indices: &[0, 1, 2],
        ..Default::default()
    });
    assert!(matches!(result, Err(RenderError::InvalidMesh(_))));
}
