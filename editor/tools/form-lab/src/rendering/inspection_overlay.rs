// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! CPU inspection data for the Output inspector: element locations, optional attribute colors, the selected element,
//! and screen-space picking. Its markers draw through the engine like point markers.

use joy_engine::glam::{
    Mat4,
    Vec2,
    Vec3,
    Vec4,
};
use joy_engine::rendering::ColorVertex;

use crate::rendering::batches::{
    MarkerCamera,
    append_marker,
};

/// Most attribute markers drawn at once; denser data is sampled at a regular stride.
pub const MAXIMUM_INSPECTION_MARKERS: usize = 12_000;

/// Picking radius around an element's screen position, in logical pixels.
pub const PICK_RADIUS_PIXELS: f32 = 12.0;

/// Highlight color of the selected element.
const SELECTION_COLOR: [f32; 4] = [1.0, 0.95, 0.82, 1.0];

/// Inspection locations, visualization colors, and the selection; owns no GPU resources.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct InspectionOverlay {
    /// World-space element locations (points, face centers, or corners).
    pub locations: Vec<[f64; 3]>,
    /// One linear RGB color per location when an attribute is visualized.
    pub colors: Option<Vec<[f32; 3]>>,
    /// The selected location's index.
    pub selected: Option<usize>,
}

impl InspectionOverlay {
    /// Replace the inspected elements and clear the selection.
    pub fn set(&mut self, locations: Vec<[f64; 3]>, colors: Option<Vec<[f32; 3]>>) {
        self.locations = locations;
        self.colors = colors;
        self.selected = None;
    }

    /// Markers for visualized attributes and the selection, nudged toward the camera so they sit on surfaces.
    ///
    /// `radius` is the camera's framed content radius and `viewport_height` the preview height in logical pixels.
    pub fn markers(&self, camera: &MarkerCamera, radius: f32, viewport_height: f32) -> Vec<ColorVertex> {
        let backward = camera.basis.backward;
        let size_radius =
            |size: f32| camera.distance * (std::f32::consts::PI / 8.0).tan() * size / viewport_height.max(100.0);
        let mut output = Vec::new();
        if let Some(colors) = &self.colors {
            let step = self.locations.len().div_ceil(MAXIMUM_INSPECTION_MARKERS).max(1);
            for index in (0..self.locations.len()).step_by(step) {
                let position = location(self.locations[index]) + (backward * radius * 0.002);
                let color = colors.get(index).copied().unwrap_or([1.0; 3]);
                append_marker(
                    &mut output,
                    position,
                    camera.basis,
                    size_radius(3.0),
                    [color[0], color[1], color[2], 1.0],
                );
            }
        }
        if let Some(selected) = self.selected.and_then(|index| self.locations.get(index)) {
            let position = location(*selected) + (backward * radius * 0.005);
            append_marker(&mut output, position, camera.basis, size_radius(7.0), SELECTION_COLOR);
        }
        output
    }

    /// The nearest element within [`PICK_RADIUS_PIXELS`] of `point` (logical pixels, top-left origin) on a viewport of
    /// `viewport_size`; coincident elements resolve to the nearer one, then the first.
    pub fn pick(&self, point: Vec2, view_projection: Mat4, viewport_size: Vec2) -> Option<usize> {
        let mut closest = PICK_RADIUS_PIXELS * PICK_RADIUS_PIXELS;
        let mut nearest_depth = f32::INFINITY;
        let mut selected = None;
        for (index, position) in self.locations.iter().enumerate() {
            let clip = view_projection * location(*position).extend(1.0);
            if clip.w <= 0.0 {
                continue;
            }
            let normalized = Vec4::new(clip.x / clip.w, clip.y / clip.w, clip.z / clip.w, clip.w);
            if !(0.0..=1.0).contains(&normalized.z) {
                continue;
            }
            let screen =
                Vec2::new((normalized.x + 1.0) * viewport_size.x / 2.0, (1.0 - normalized.y) * viewport_size.y / 2.0);
            let distance = screen.distance_squared(point);
            if distance < closest || ((distance - closest).abs() < 0.01 && clip.w < nearest_depth) {
                closest = distance;
                nearest_depth = clip.w;
                selected = Some(index);
            }
        }
        selected
    }
}

/// A location as a renderer vector.
fn location(position: [f64; 3]) -> Vec3 {
    Vec3::new(position[0] as f32, position[1] as f32, position[2] as f32)
}
