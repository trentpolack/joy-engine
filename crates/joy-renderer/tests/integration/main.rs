// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Integration tests for `joy-renderer`: headless rendering, render scale, deferred and clustered lighting, materials,
//! shadows, bloom, display output, triangles, terrain, and particle drawing and simulation, with terrain and particle
//! golden recordings (re-record with `JOY_UPDATE_FIXTURES=1`). Run with `cargo test -p joy-renderer`. GPU modules skip
//! with a message when no adapter (hardware or software) is available; set `JOY_RENDER_CAPTURE_DIR` to keep their PNG
//! captures.

mod particles_effect;
mod particles_headless;
mod particles_parity;
mod particles_rendering;
mod rendering_bloom;
mod rendering_display;
mod rendering_headless;
mod rendering_lighting;
mod rendering_materials;
mod rendering_terrain;
mod rendering_triangles;
mod terrain;

/// A mesh from flat XYZ positions, a triangle list, and optional flat linear RGB colors (empty for white), as the
/// headless scenes build their boxes.
pub(crate) fn flat_mesh(positions: &[f32], indices: &[u32], colors: &[f32]) -> joy_render_types::Mesh {
    use joy_core::math::DVec3;
    let points =
        positions.as_chunks::<3>().0.iter().map(|[x, y, z]| DVec3::new(f64::from(*x), f64::from(*y), f64::from(*z)));
    let mut mesh = joy_render_types::Mesh::new(points.collect(), indices.to_vec());
    if !colors.is_empty() {
        mesh.colors = Some(colors.as_chunks::<3>().0.iter().map(|rgb| joy_core::LinearColor::from(*rgb)).collect());
    }
    mesh
}
