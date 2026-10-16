// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Shadows: cascaded directional maps with percentage-closer soft shadows, and six-face depth maps for point lights.
//!
//! This implementation derives from the luma.gl 9.4.2 shadow module (`ShadowMapRenderer` and the `shadow` shader
//! module, with the engine's WebGPU-depth adapter for point lights): practical cascade splits fitted to the camera
//! frustum and snapped to whole texels, blocker search and filtering on a rotated disk, cascade blending, and a far
//! fade. Two portability differences keep every backend on one path: point lights store their six faces in a 2D
//! depth array instead of a cube array (the receiver selects the face itself), and shadows also run on WebGL 2.

mod maps;

use glam::{Mat4, Vec3, Vec4};

use crate::rendering::camera::{CameraFrame, look_at, orthographic, perspective};

pub(crate) use maps::{PointShadowRaw, ShadowMaps, ShadowUniforms, caster_layout, receiver_layout};

/// Cascades at most; the receiver uniform always holds this many matrices.
pub(crate) const MAX_CASCADES: usize = 4;
/// Faces rendered per shadowed point light.
pub(crate) const POINT_FACES: usize = 6;
/// Point-light maps are allocated in groups of this many lights, like the TypeScript light batches.
pub(crate) const SHADOW_LIGHTS_PER_BATCH: usize = 4;
/// Cascade split blend between uniform (0) and logarithmic (1) distribution.
const CASCADE_SPLIT_LAMBDA: f32 = 0.5;
/// Fraction of each cascade blended into the next.
pub(crate) const CASCADE_BLEND_FRACTION: f32 = 0.1;
/// Fraction of the shadow distance over which shadows fade out.
pub(crate) const FAR_FADE_FRACTION: f32 = 0.1;
/// Light size used for point-light penumbrae, in world units.
pub(crate) const POINT_SOURCE_RADIUS: f32 = 0.2;
/// Minimum receiver offset in cascade texels: a fixed world-space bias is far smaller than one texel of an outer
/// cascade, so flat ground self-shadows in rings at a distance.
pub(crate) const CASCADE_TEXEL_BIAS: f32 = 1.5;
/// Near plane of every cascade's orthographic projection.
const CASCADE_NEAR: f32 = 0.01;

/// Shadow sampling quality preset, matching the TypeScript names and the luma.gl quality table.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ShadowQuality {
    /// Three cascades, 1024-pixel directional and 256-pixel point maps (the TypeScript default).
    #[default]
    Low,
    /// Four cascades, 1536-pixel directional and 512-pixel point maps.
    Balanced,
    /// Four cascades, 2048-pixel directional and 1024-pixel point maps.
    Cinematic,
}

impl ShadowQuality {
    /// Directional cascades.
    pub fn cascade_count(self) -> usize {
        match self {
            Self::Low => 3,
            Self::Balanced | Self::Cinematic => 4,
        }
    }

    /// Directional cascade width and height in pixels.
    pub fn directional_map_size(self) -> u32 {
        match self {
            Self::Low => 1024,
            Self::Balanced => 1536,
            Self::Cinematic => 2048,
        }
    }

    /// Point-light face width and height in pixels.
    pub fn point_map_size(self) -> u32 {
        match self {
            Self::Low => 256,
            Self::Balanced => 512,
            Self::Cinematic => 1024,
        }
    }

    /// Taps searched for blockers before filtering.
    pub fn blocker_sample_count(self) -> u32 {
        match self {
            Self::Low => 8,
            Self::Balanced => 16,
            Self::Cinematic => 24,
        }
    }

    /// Comparison taps averaged for each shadow factor.
    pub fn filter_sample_count(self) -> u32 {
        match self {
            Self::Low => 12,
            Self::Balanced => 24,
            Self::Cinematic => 48,
        }
    }
}

/// Shadow settings; the renderer copies them at creation. Distances are world units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShadowOptions {
    /// Quality preset: cascade count, sample counts, and default map sizes.
    pub quality: ShadowQuality,
    /// Explicit cascade map size in pixels, clamped to the device limit; `None` uses the preset.
    pub directional_map_size: Option<u32>,
    /// Explicit point-light face size in pixels, clamped to the device limit; `None` uses the preset.
    pub point_map_size: Option<u32>,
    /// Receiver displacement along its world-space normal.
    pub normal_bias: f32,
    /// Constant caster raster bias in depth units.
    pub depth_bias: i32,
    /// Caster depth bias scaled by the depth slope.
    pub depth_bias_slope_scale: f32,
    /// Maximum camera view depth that receives directional shadows.
    pub shadow_distance: f32,
}

impl Default for ShadowOptions {
    /// Defaults match the TypeScript `DEFAULT_SHADOW_OPTIONS`.
    fn default() -> Self {
        Self {
            quality: ShadowQuality::Low,
            directional_map_size: None,
            point_map_size: None,
            normal_bias: 0.01,
            depth_bias: 2,
            depth_bias_slope_scale: 2.0,
            shadow_distance: 100.0,
        }
    }
}

impl ShadowOptions {
    /// Effective cascade map size for a device's maximum texture dimension.
    pub fn directional_map_size(&self, max_texture_dimension: u32) -> u32 {
        self.directional_map_size.unwrap_or(self.quality.directional_map_size()).clamp(16, max_texture_dimension)
    }

    /// Effective point-light face size for a device's maximum texture dimension.
    pub fn point_map_size(&self, max_texture_dimension: u32) -> u32 {
        self.point_map_size.unwrap_or(self.quality.point_map_size()).clamp(16, max_texture_dimension)
    }
}

/// Far view depth of each cascade, blending uniform and logarithmic distributions; unused entries hold `far`.
pub(crate) fn practical_cascade_splits(near: f32, far: f32, count: usize) -> [f32; MAX_CASCADES] {
    let mut splits = [far; MAX_CASCADES];
    let count = count.clamp(1, MAX_CASCADES);
    for (index, split) in splits.iter_mut().enumerate().take(count) {
        let fraction = (index + 1) as f32 / count as f32;
        let logarithmic = near * (far / near).powf(fraction);
        let uniform = near + ((far - near) * fraction);
        *split = (uniform * (1.0 - CASCADE_SPLIT_LAMBDA)) + (logarithmic * CASCADE_SPLIT_LAMBDA);
    }
    splits
}

/// Texel-snapped orthographic light cameras covering each cascade's slice of the camera frustum, each with the world
/// size of one of its texels.
///
/// `direction` points toward the light. Each cascade bounds its frustum slice with a sphere, so its footprint is stable
/// as the camera turns, and its center moves in whole texels so shadows do not shimmer. Casters up to
/// `caster_distance` beyond the sphere toward the light are included.
pub(crate) fn directional_cascades(
    camera: &CameraFrame,
    direction: Vec3,
    splits: &[f32; MAX_CASCADES],
    count: usize,
    caster_distance: f32,
    map_size: u32,
) -> Vec<(Mat4, f32)> {
    let inverse = camera.view_projection().inverse();
    let corner = |x: f32, y: f32, z: f32| {
        let point = inverse * Vec4::new(x, y, z, 1.0);
        point.truncate() / point.w
    };
    let ndc = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)];
    let near_corners = ndc.map(|(x, y)| corner(x, y, 0.0));
    let far_corners = ndc.map(|(x, y)| corner(x, y, 1.0));
    let direction = direction.try_normalize().unwrap_or(Vec3::Y);
    let forward = -direction;
    let reference_up = if forward.y.abs() > 0.99 { Vec3::Z } else { Vec3::Y };
    let right = forward.cross(reference_up).normalize();
    let up = right.cross(forward).normalize();
    let depth_range = camera.far - camera.near;

    let mut cascade_near = camera.near;
    let mut views = Vec::with_capacity(count);
    for split in splits.iter().take(count.clamp(1, MAX_CASCADES)) {
        let near_fraction = (cascade_near - camera.near) / depth_range;
        let far_fraction = (split - camera.near) / depth_range;
        let mut corners = Vec::with_capacity(8);
        for (near, far) in near_corners.iter().zip(&far_corners) {
            corners.push(near.lerp(*far, near_fraction));
            corners.push(near.lerp(*far, far_fraction));
        }
        let center = corners.iter().copied().sum::<Vec3>() / corners.len() as f32;
        let radius = corners.iter().map(|corner| corner.distance(center)).fold(0.0_f32, f32::max);
        let radius = (radius * 16.0).ceil() / 16.0;
        let texel = (radius * 2.0) / map_size as f32;
        let along_right = center.dot(right);
        let along_up = center.dot(up);
        let snapped = center
            + (right * (((along_right / texel).round() * texel) - along_right))
            + (up * (((along_up / texel).round() * texel) - along_up));
        let eye = snapped + (direction * (radius + caster_distance));
        let view = look_at(eye, snapped, up);
        let projection = orthographic(-radius, radius, -radius, radius, CASCADE_NEAR, (radius * 2.0) + caster_distance);
        views.push((projection * view, texel));
        cascade_near = *split;
    }
    views
}

/// Face forward and up directions, in layer order (+X, -X, +Y, -Y, +Z, -Z). The receiver in scene-common.wgsl uses the
/// same table to select and project faces.
pub(crate) const POINT_FACE_ORIENTATIONS: [(Vec3, Vec3); POINT_FACES] = [
    (Vec3::X, Vec3::NEG_Y),
    (Vec3::NEG_X, Vec3::NEG_Y),
    (Vec3::Y, Vec3::Z),
    (Vec3::NEG_Y, Vec3::NEG_Z),
    (Vec3::Z, Vec3::NEG_Y),
    (Vec3::NEG_Z, Vec3::NEG_Y),
];

/// Near plane of a point light's face projections, as the TypeScript `SceneShadows` sets it.
pub(crate) fn point_near_plane(range: f32) -> f32 {
    0.1_f32.min(range * 0.01)
}

/// The six 90-degree view-projections of a point light, in [`POINT_FACE_ORIENTATIONS`] order.
pub(crate) fn point_face_views(position: Vec3, range: f32) -> [Mat4; POINT_FACES] {
    let projection = perspective(std::f32::consts::FRAC_PI_2, 1.0, point_near_plane(range), range);
    POINT_FACE_ORIENTATIONS.map(|(forward, up)| projection * look_at(position, position + forward, up))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn camera() -> CameraFrame {
        let eye = Vec3::new(0.0, 8.0, 20.0);
        CameraFrame {
            view: look_at(eye, Vec3::ZERO, Vec3::Y),
            projection: perspective(1.0, 16.0 / 9.0, 0.1, 200.0),
            eye,
            focus: Vec3::ZERO,
            near: 0.1,
            far: 200.0,
        }
    }

    #[test]
    fn cascade_splits_blend_uniform_and_logarithmic_distributions() {
        let splits = practical_cascade_splits(0.1, 100.0, 3);
        // luma.gl values for near 0.1, far 100, three cascades, lambda 0.5.
        let expected = [17.2, 38.35, 100.0];
        for (split, expected) in splits.iter().zip(expected) {
            assert!((split - expected).abs() < 0.05, "{split} vs {expected}");
        }
        assert_eq!(splits[3], 100.0);
        assert!(splits.windows(2).all(|pair| pair[0] <= pair[1]));
    }

    #[test]
    fn cascades_contain_their_frustum_slices() {
        let camera = camera();
        let splits = practical_cascade_splits(camera.near, 100.0, 4);
        let direction = Vec3::new(0.3, 0.8, 0.2);
        let views = directional_cascades(&camera, direction, &splits, 4, 100.0, 1024);
        assert_eq!(views.len(), 4);
        // Points along the view ray at each cascade's middle depth project inside that cascade's map.
        let inverse_view = camera.view.inverse();
        let mut near = camera.near;
        for ((view, texel), far) in views.iter().zip(splits) {
            assert!(*texel > 0.0);
            let depth = (near + far) * 0.5;
            let world = inverse_view.transform_point3(Vec3::new(0.0, 0.0, -depth));
            let clip = *view * world.extend(1.0);
            let ndc = clip.truncate() / clip.w;
            assert!(ndc.x.abs() <= 1.0 && ndc.y.abs() <= 1.0, "depth {depth}: {ndc}");
            assert!((0.0..=1.0).contains(&ndc.z), "depth {depth}: {}", ndc.z);
            near = far;
        }
    }

    #[test]
    fn point_faces_match_the_receiver_projection() {
        let position = Vec3::new(1.0, 2.0, -3.0);
        let range = 12.0;
        let views = point_face_views(position, range);
        let near = point_near_plane(range);
        for target in [Vec3::new(5.0, 2.5, -2.0), Vec3::new(0.0, -4.0, -2.0), Vec3::new(1.5, 2.0, -9.0)] {
            let offset = target - position;
            // The receiver's face selection and projection (scene-common.wgsl `pointFace` and `pointFaceCoordinate`).
            let magnitude = offset.abs();
            let face = if magnitude.x >= magnitude.y && magnitude.x >= magnitude.z {
                if offset.x > 0.0 { 0 } else { 1 }
            } else if magnitude.y >= magnitude.z {
                if offset.y > 0.0 { 2 } else { 3 }
            } else if offset.z > 0.0 {
                4
            } else {
                5
            };
            let (forward, up) = POINT_FACE_ORIENTATIONS[face];
            let right = forward.cross(up);
            let major = forward.dot(offset);
            let uv = [(right.dot(offset) / major) * 0.5 + 0.5, 0.5 - ((up.dot(offset) / major) * 0.5)];
            let depth = (range / (range - near)) - ((range * near) / ((range - near) * major));

            let clip = views[face] * target.extend(1.0);
            let ndc = clip.truncate() / clip.w;
            assert!((ndc.x * 0.5 + 0.5 - uv[0]).abs() < 1e-5 && (0.5 - ndc.y * 0.5 - uv[1]).abs() < 1e-5);
            assert!((ndc.z - depth).abs() < 1e-5, "{} vs {depth}", ndc.z);
            assert!(uv.iter().all(|value| (0.0..=1.0).contains(value)));
        }
    }
}
