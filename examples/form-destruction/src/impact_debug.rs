// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Bounded world-space impact markers: a white hit cross, a yellow incoming-ray segment, and three cyan rings outlining
//! the spherical damage radius. Markers draw in the transparent pass (unlit, depth tested), fade before expiry, and are
//! released when disabled or cleared. Scenes are borrowed per call.

use joy_engine::prelude::*;
use joy_engine::rendering::{
    MeshChannels,
    RenderPassKind,
};

use super::config::destruction_config::IMPACT_DEBUG;

/// Box-prism faces for one marker line segment (two four-corner rings).
const SEGMENT_INDICES: [u32; 36] =
    [0, 2, 1, 0, 3, 2, 4, 5, 6, 4, 6, 7, 0, 1, 5, 0, 5, 4, 1, 2, 6, 1, 6, 5, 2, 3, 7, 2, 7, 6, 3, 0, 4, 3, 4, 7];

/// One live marker.
struct Marker {
    mesh: MeshInstanceId,
    age_seconds: f32,
}

/// Owns bounded hit, incoming-ray, and damage-radius markers.
pub struct ImpactDebugDraw {
    markers: Vec<Marker>,
    enabled: bool,
}

impl Default for ImpactDebugDraw {
    fn default() -> Self {
        Self::new()
    }
}

impl ImpactDebugDraw {
    /// Markers visible according to the configured default.
    pub fn new() -> Self {
        Self { markers: Vec::new(), enabled: IMPACT_DEBUG.enabled }
    }

    /// Mark a hit at `point` (meters) for a unit incoming `direction` and damage `radius`; the oldest marker is replaced
    /// at the limit. Disabled markers allocate nothing.
    pub fn add(&mut self, scene: &mut RenderScene, point: DVec3, direction: Vec3, radius: f32) -> AppResult {
        if !self.enabled {
            return Ok(());
        }
        if self.markers.len() >= IMPACT_DEBUG.maximum_markers {
            self.remove_marker(scene, 0);
        }
        let offset = IMPACT_DEBUG.surface_offset;
        let position = Vec3::new(
            (point[0] - (f64::from(direction.x) * offset)) as f32,
            (point[1] - (f64::from(direction.y) * offset)) as f32,
            (point[2] - (f64::from(direction.z) * offset)) as f32,
        );
        let mut mesh = MeshInstance::new(marker_asset(direction, radius)?).with_transform(Transform::new(
            position,
            Vec3::ZERO,
            Vec3::ONE,
        ));
        // The transparent pass is unlit but stays depth tested against the structure.
        mesh.pass = RenderPassKind::Transparent;
        mesh.casts_shadow = false;
        let mesh = scene.insert(mesh);
        self.markers.push(Marker { mesh, age_seconds: 0.0 });
        Ok(())
    }

    /// Age markers by `elapsed_seconds`, fading them over their last second and releasing expired ones.
    pub fn update(&mut self, scene: &mut RenderScene, elapsed_seconds: f32) {
        for index in (0..self.markers.len()).rev() {
            let marker = &mut self.markers[index];
            marker.age_seconds += elapsed_seconds;
            let remaining = IMPACT_DEBUG.lifetime_seconds - marker.age_seconds;
            if remaining <= 0.0 {
                self.remove_marker(scene, index);
                continue;
            }
            if let Some(mesh) = scene.get_mut(marker.mesh) {
                mesh.opacity = (remaining / IMPACT_DEBUG.fade_seconds).min(1.0);
            }
        }
    }

    /// Show or hide markers; hiding releases existing markers immediately.
    pub fn set_enabled(&mut self, scene: &mut RenderScene, enabled: bool) {
        self.enabled = enabled;
        if !enabled {
            self.clear(scene);
        }
    }

    /// Whether markers draw.
    pub fn enabled(&self) -> bool {
        self.enabled
    }

    /// Live markers.
    #[allow(dead_code, reason = "part of the capture and test API that the integration tests drive")]
    pub fn count(&self) -> usize {
        self.markers.len()
    }

    /// Release every marker, keeping the visibility preference.
    pub fn clear(&mut self, scene: &mut RenderScene) {
        while !self.markers.is_empty() {
            self.remove_marker(scene, self.markers.len() - 1);
        }
    }

    fn remove_marker(&mut self, scene: &mut RenderScene, index: usize) {
        let marker = self.markers.remove(index);
        scene.remove(marker.mesh);
    }
}

/// Marker geometry around the local origin: the incoming ray, three great circles of the damage sphere, and the cross.
fn marker_asset(direction: Vec3, radius: f32) -> AppResult<MeshAsset> {
    let mut geometry = MarkerGeometry::default();
    let direction = direction.normalize_or(Vec3::NEG_Z);
    geometry.segment(-direction * IMPACT_DEBUG.ray_length, Vec3::ZERO, IMPACT_DEBUG.ray_color);

    // Three great circles show the spherical damage volume from any orbit angle.
    for normal_axis in 0..3 {
        let first_axis = (normal_axis + 1) % 3;
        let second_axis = (normal_axis + 2) % 3;
        let point = |segment: u32| {
            let angle = segment as f32 * std::f32::consts::TAU / IMPACT_DEBUG.ring_segments as f32;
            let mut point = Vec3::ZERO;
            point[first_axis] = angle.cos() * radius;
            point[second_axis] = angle.sin() * radius;
            point
        };
        for segment in 0..IMPACT_DEBUG.ring_segments {
            geometry.segment(point(segment), point(segment + 1), IMPACT_DEBUG.radius_color);
        }
    }
    for axis in 0..3 {
        let mut half = Vec3::ZERO;
        half[axis] = IMPACT_DEBUG.point_size / 2.0;
        geometry.segment(-half, half, IMPACT_DEBUG.point_color);
    }
    Ok(MeshAsset::from_channels(MeshChannels {
        positions: &geometry.positions,
        indices: &geometry.indices,
        colors: &geometry.colors,
        ..Default::default()
    })?)
}

/// Accumulated marker channels.
#[derive(Default)]
struct MarkerGeometry {
    positions: Vec<f32>,
    indices: Vec<u32>,
    colors: Vec<f32>,
}

impl MarkerGeometry {
    /// Append a thin closed prism from `first` to `second`, so the line keeps a visible width from any angle.
    fn segment(&mut self, first: Vec3, second: Vec3, color: [f32; 3]) {
        let forward = (second - first).normalize_or(Vec3::X);
        let reference = if forward.y.abs() < 0.9 { Vec3::Y } else { Vec3::X };
        let right = forward.cross(reference).normalize();
        let up = right.cross(forward);
        let half_width = IMPACT_DEBUG.line_width / 2.0;
        let start = (self.positions.len() / 3) as u32;
        for point in [first, second] {
            for (right_sign, up_sign) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                let corner = point + (((right * right_sign) + (up * up_sign)) * half_width);
                self.positions.extend_from_slice(&corner.to_array());
                self.colors.extend_from_slice(&color);
            }
        }
        self.indices.extend(SEGMENT_INDICES.iter().map(|index| start + index));
    }
}
