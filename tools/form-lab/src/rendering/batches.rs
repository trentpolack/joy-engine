// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Camera-facing point markers for authored points and the Points view.
//!
//! Markers are two triangles each, sized to a constant pixel footprint at the camera's focus distance and colored by
//! their vertex tint, material base color, and emission, with alpha following the material's glTF policy. The engine
//! draws them as unlit transparent runs after the scene, so FORM points get no renderer of their own.

use joy_engine::form::{
    AlphaMode,
    FormMaterial,
    GeometryData,
};
use joy_engine::math::Vec3;
use joy_engine::rendering::{
    CameraBasis,
    ColorVertex,
};

/// Most markers drawn at once; denser data is sampled at a regular stride. Exports keep every point.
pub const MAXIMUM_PREVIEW_MARKERS: usize = 40_000;

/// The camera values markers face and scale with.
#[derive(Clone, Copy, Debug)]
pub struct MarkerCamera {
    /// The camera's basis (right and up span each marker).
    pub basis: CameraBasis,
    /// Eye distance from the orbit target, in world units.
    pub distance: f32,
}

/// Markers for `data`: its authored points, or every vertex when `all_vertices` is set.
///
/// `size` is the marker size control (one to six) and `viewport_height` the preview's height in logical pixels. Each
/// marker contributes six vertices.
pub fn point_batch(
    data: &GeometryData,
    all_vertices: bool,
    camera: &MarkerCamera,
    size: f32,
    viewport_height: f32,
) -> Vec<ColorVertex> {
    let materials = data.form_materials();
    let fallback = FormMaterial::default();
    let vertex_count = data.positions.len() / 3;

    // In the Points view a vertex takes the material of the last triangle corner or point that uses it.
    let mut assignments = vec![0_u32; if all_vertices { vertex_count } else { 0 }];
    if all_vertices {
        for (corner, index) in data.triangles.iter().enumerate() {
            if let (Some(slot), Some(material)) =
                (assignments.get_mut(*index as usize), data.triangle_materials.get(corner / 3))
            {
                *slot = *material;
            }
        }
        for (point, index) in data.points.iter().enumerate() {
            if let (Some(slot), Some(material)) =
                (assignments.get_mut(*index as usize), data.point_materials.get(point))
            {
                *slot = *material;
            }
        }
    }
    let count = if all_vertices { vertex_count } else { data.points.len() };
    let stride = count.div_ceil(MAXIMUM_PREVIEW_MARKERS).max(1);
    let radius = camera.distance * (std::f32::consts::PI / 8.0).tan() * size / viewport_height.max(100.0);
    let mut output = Vec::with_capacity((count / stride + 1) * 6);
    for sample in (0..count).step_by(stride) {
        let (index, material) = if all_vertices {
            (sample, assignments[sample])
        } else {
            (data.points[sample] as usize, data.point_materials.get(sample).copied().unwrap_or(0))
        };
        let material = materials.get(material as usize).unwrap_or(&fallback);
        let Some(position) = data.positions.get(index * 3..(index * 3) + 3) else {
            continue;
        };
        let position = Vec3::new(position[0] as f32, position[1] as f32, position[2] as f32);
        let tint = data.colors.get(index * 3..(index * 3) + 3).unwrap_or(&[1.0, 1.0, 1.0]);
        let color = [0, 1, 2]
            .map(|channel| ((tint[channel] * material.base_color[channel]) + material.emissive[channel]) as f32);
        let alpha = material_alpha(material, data.alphas.get(index).copied().unwrap_or(1.0));
        append_marker(&mut output, position, camera.basis, radius, [color[0], color[1], color[2], alpha]);
    }
    output
}

/// Append one camera-facing square of half-size `radius` centered at `position`.
pub fn append_marker(output: &mut Vec<ColorVertex>, position: Vec3, basis: CameraBasis, radius: f32, color: [f32; 4]) {
    let corners = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)]
        .map(|(x, y): (f32, f32)| position + ((basis.right * x) + (basis.up * y)) * radius);
    for corner in [0, 1, 2, 0, 2, 3] {
        output.push(ColorVertex {
            position: corners[corner].to_array(),
            color,
        });
    }
}

/// Alpha under the material's glTF policy, including vertex alpha.
pub fn material_alpha(material: &FormMaterial, vertex_alpha: f64) -> f32 {
    let alpha = material.base_color[3] * vertex_alpha;
    match material.alpha_mode {
        AlphaMode::Opaque => 1.0,
        AlphaMode::Mask if alpha >= material.alpha_cutoff => 1.0,
        AlphaMode::Mask => 0.0,
        AlphaMode::Blend => alpha as f32,
    }
}
