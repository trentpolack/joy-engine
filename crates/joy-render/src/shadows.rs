// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Directional shadow configuration and light-camera fitting.
//!
//! The environment's primary light casts one orthographic shadow map centered on the camera's focus point. The volume
//! covers the shadow casters' bounds up to `shadow_distance`, and its projection is snapped to whole shadow texels so
//! shadows do not shimmer while the camera moves. Unlike the TypeScript renderer, these shadows also work on WebGL 2.

use glam::{Mat4, Vec3, Vec4};

use crate::camera::{look_at, orthographic};
use crate::mesh::Bounds;

/// Shadow sampling quality preset, matching the TypeScript names.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ShadowQuality {
    /// 1024-pixel map (the TypeScript default).
    #[default]
    Low,
    /// 2048-pixel map.
    Balanced,
    /// 4096-pixel map.
    Cinematic,
}

impl ShadowQuality {
    /// Directional map width and height in pixels.
    pub fn map_size(self) -> u32 {
        match self {
            Self::Low => 1024,
            Self::Balanced => 2048,
            Self::Cinematic => 4096,
        }
    }
}

/// Directional shadow settings; the renderer copies them at creation. Distances are world units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShadowOptions {
    /// Quality preset used when `directional_map_size` is `None`.
    pub quality: ShadowQuality,
    /// Explicit map size in pixels, clamped to the device limit.
    pub directional_map_size: Option<u32>,
    /// Receiver displacement along its world-space normal.
    pub normal_bias: f32,
    /// Constant caster raster bias in depth units.
    pub depth_bias: i32,
    /// Caster depth bias scaled by the depth slope.
    pub depth_bias_slope_scale: f32,
    /// Maximum radius of the shadow volume around the camera focus.
    pub shadow_distance: f32,
    /// Percentage-closer filter radius in shadow texels.
    pub filter_radius: f32,
}

impl Default for ShadowOptions {
    /// Defaults match the TypeScript `DEFAULT_SHADOW_OPTIONS`.
    fn default() -> Self {
        Self {
            quality: ShadowQuality::Low,
            directional_map_size: None,
            normal_bias: 0.01,
            depth_bias: 2,
            depth_bias_slope_scale: 2.0,
            shadow_distance: 100.0,
            filter_radius: 1.0,
        }
    }
}

impl ShadowOptions {
    /// Effective map size for a device's maximum texture dimension.
    pub fn map_size(&self, max_texture_dimension: u32) -> u32 {
        self.directional_map_size.unwrap_or(self.quality.map_size()).clamp(16, max_texture_dimension)
    }
}

/// Build a snapped light view-projection for a directional light.
///
/// `light_direction` points toward the light. `caster_bounds` are world-space bounds of every shadow caster, or `None`
/// when there are none (the volume then uses `max_distance`). The returned matrix maps into zero-to-one depth.
pub(crate) fn fit_directional_shadow(
    focus: Vec3,
    light_direction: Vec3,
    caster_bounds: Option<Bounds>,
    max_distance: f32,
    map_size: u32,
) -> Mat4 {
    let direction = light_direction.try_normalize().unwrap_or(Vec3::Y);

    // Cover the casters around the focus, but never beyond the configured distance.
    let reach = caster_bounds.map_or(max_distance, |bounds| {
        bounds.corners().iter().map(|corner| corner.distance(focus)).fold(0.0_f32, f32::max)
    });
    let radius = reach.min(max_distance).max(1.0);

    let up = if direction.y.abs() > 0.99 { Vec3::Z } else { Vec3::Y };
    let eye = focus + (direction * (radius * 2.0));
    let view = look_at(eye, focus, up);
    let mut projection = orthographic(-radius, radius, -radius, radius, 0.0, radius * 4.0);

    // Snap the world origin to a whole texel so the map content moves in texel steps.
    let origin = (projection * view) * Vec4::new(0.0, 0.0, 0.0, 1.0);
    let half_size = map_size as f32 * 0.5;
    let texel = Vec3::new(origin.x * half_size, origin.y * half_size, 0.0);
    let snapped = texel.round();
    projection.w_axis.x += (snapped.x - texel.x) / half_size;
    projection.w_axis.y += (snapped.y - texel.y) / half_size;

    projection * view
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fitted_volume_contains_casters() {
        let bounds = Bounds { min: Vec3::new(-20.0, -1.0, -20.0), max: Vec3::new(20.0, 3.0, 20.0) };
        let matrix = fit_directional_shadow(Vec3::ZERO, Vec3::new(0.3, 0.8, 0.2), Some(bounds), 100.0, 1024);
        for corner in bounds.corners() {
            let clip = matrix * corner.extend(1.0);
            let ndc = clip.truncate() / clip.w;
            assert!(ndc.x.abs() <= 1.001 && ndc.y.abs() <= 1.001, "corner {corner} outside: {ndc}");
            assert!((0.0..=1.0).contains(&ndc.z), "corner {corner} depth {}", ndc.z);
        }
    }

    #[test]
    fn distance_limits_the_volume() {
        let bounds = Bounds { min: Vec3::splat(-500.0), max: Vec3::splat(500.0) };
        let matrix = fit_directional_shadow(Vec3::ZERO, Vec3::Y, Some(bounds), 50.0, 1024);
        let edge = matrix * Vec4::new(49.0, 0.0, 0.0, 1.0);
        let beyond = matrix * Vec4::new(0.0, 0.0, 60.0, 1.0);
        assert!(edge.x.abs() <= 1.0);
        assert!(beyond.y.abs() > 1.0 || beyond.x.abs() > 1.0);
    }
}
