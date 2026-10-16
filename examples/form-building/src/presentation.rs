// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Building presentation (TypeScript `presentation.ts`). The TypeScript scene renderer ignores asset materials, so that
//! adapter splits triangles by material into tinted instances. The Rust renderer's PBR pass reads the metallic,
//! roughness, emission, and alpha factors baked into the asset, so one PBR instance draws every material group and the
//! canonical asset stays untouched.

use joy_engine::prelude::*;

/// Replace the scene's previous building with `asset` and return the new instance.
///
/// Prepare the asset before calling: a failed evaluation never reaches the scene, so the previous building stays
/// visible.
pub fn populate_building_scene(scene: &mut RenderScene, asset: MeshAsset) -> MeshInstanceId {
    scene.clear();
    scene.insert(MeshInstance::new(asset).with_shading(Shading::Pbr))
}
