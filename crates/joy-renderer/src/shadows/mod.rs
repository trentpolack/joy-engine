// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Shadows: cascaded maps for the primary directional light and one depth atlas of cube faces for point lights.
//!
//! Cascades split the camera's view out to the shadow distance, between uniform and logarithmic spacing. Each cascade
//! bounds its slice of the view frustum with a sphere, so its footprint holds still as the camera turns; moves in whole
//! texels, so edges do not shimmer as the camera travels; and reaches toward the light only as far as the casters
//! above it, which keeps its depth range (and with it depth precision and the penumbra scale) tight.
//!
//! Point lights share one atlas. Each shadowed light renders six cube faces into square tiles sized by how large its
//! reach appears on screen, up to the quality preset's face size; when the frame's lights do not fit, the least
//! important lights shrink first, then lose their shadows. Every face renders in one pass, each into its own viewport.
//! Receivers (`shadow-receiver.wgsl`) filter both kinds with percentage-closer soft shadows.

mod atlas;
mod maps;

use joy_core::math::{
    Mat4,
    Vec3,
    Vec4,
};

use crate::camera::{
    CameraFrame,
    look_at,
    orthographic,
    perspective,
};
use crate::mesh::Bounds;

pub(crate) use atlas::{
    AtlasTile,
    fit_face_sizes,
    place_faces,
};
pub(crate) use maps::{
    CascadeRaw,
    PointShadowRaw,
    ShadowMaps,
    ShadowUniforms,
    caster_layout,
    receiver_layout,
};

/// Cascades at most; the receiver uniform always holds this many.
pub(crate) const MAX_CASCADES: usize = 4;
/// Faces rendered per shadowed point light.
pub(crate) const POINT_FACES: usize = 6;
/// Shadowed point lights the receiver uniform holds (`Shadows.points` in shadow-receiver.wgsl).
pub const MAX_SHADOWED_POINT_LIGHTS: usize = 32;
/// Smallest point-light face, in texels; lights that cannot fit at this size lose their shadows.
pub(crate) const MIN_POINT_FACE_SIZE: u32 = 32;
/// Default cascade split blend between uniform (0) and logarithmic (1) spacing.
pub(crate) const DEFAULT_CASCADE_SPLIT_LAMBDA: f32 = 0.5;
/// Fraction of each cascade blended into the next.
pub(crate) const CASCADE_BLEND_FRACTION: f32 = 0.1;
/// Fraction of the shadow distance over which shadows fade out.
pub(crate) const FAR_FADE_FRACTION: f32 = 0.1;
/// Light size used for point-light penumbrae, in world units.
pub(crate) const POINT_SOURCE_RADIUS: f32 = 0.2;
/// Minimum receiver offset in cascade texels: a fixed world-space offset is far smaller than one texel of an outer
/// cascade, so flat ground would self-shadow in rings at a distance.
pub(crate) const CASCADE_TEXEL_BIAS: f32 = 1.5;
/// Near plane of every cascade's orthographic projection.
const CASCADE_NEAR: f32 = 0.01;

/// Shadow quality preset.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ShadowQuality {
    /// Three cascades of 1024 pixels; point-light faces up to 256 pixels in an atlas of up to 2048 (the default).
    #[default]
    Low,
    /// Four cascades of 1536 pixels; point-light faces up to 512 pixels in an atlas of up to 4096.
    Balanced,
    /// Four cascades of 2048 pixels; point-light faces up to 1024 pixels in an atlas of up to 4096.
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

    /// Largest point-light face in pixels; lights that cover less of the screen get smaller faces.
    pub fn point_map_size(self) -> u32 {
        match self {
            Self::Low => 256,
            Self::Balanced => 512,
            Self::Cinematic => 1024,
        }
    }

    /// Largest width and height of the point-light atlas in pixels; it grows toward this as frames need it.
    pub fn point_atlas_size(self) -> u32 {
        match self {
            Self::Low => 2048,
            Self::Balanced | Self::Cinematic => 4096,
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

/// Shadow settings. Distances are world units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShadowOptions {
    /// Quality preset: cascade count, sample counts, and default map sizes.
    pub quality: ShadowQuality,
    /// Explicit cascade map size in pixels, clamped to the device limit; `None` uses the preset.
    pub directional_map_size: Option<u32>,
    /// Explicit largest point-light face size in pixels, clamped to the atlas; `None` uses the preset.
    pub point_map_size: Option<u32>,
    /// Point lights that may cast shadows in one frame, at most [`MAX_SHADOWED_POINT_LIGHTS`]. The lights that matter
    /// most to the view cast first.
    pub max_point_lights: usize,
    /// Minimum receiver displacement along its world-space normal; receivers move at least a few texels of the map
    /// they read.
    pub normal_bias: f32,
    /// Constant caster raster bias in depth units.
    pub depth_bias: i32,
    /// Caster depth bias scaled by the depth slope.
    pub depth_bias_slope_scale: f32,
    /// Maximum camera view depth that receives directional shadows.
    pub shadow_distance: f32,
    /// How cascade splits blend uniform (0) and logarithmic (1) spacing, in `[0, 1]`. Long shadow distances want more
    /// logarithmic splits, so the first cascade stays small (sharp, with a small texel-scaled receiver offset) near the
    /// camera instead of stretching over a fraction of the whole distance.
    pub cascade_split_lambda: f32,
}

impl Default for ShadowOptions {
    /// Low quality with the preset map sizes and up to 32 shadowed point lights; directional shadows reach 100 world
    /// units from the camera.
    fn default() -> Self {
        Self {
            quality: ShadowQuality::Low,
            directional_map_size: None,
            point_map_size: None,
            max_point_lights: MAX_SHADOWED_POINT_LIGHTS,
            normal_bias: 0.01,
            depth_bias: 2,
            depth_bias_slope_scale: 2.0,
            shadow_distance: 100.0,
            cascade_split_lambda: DEFAULT_CASCADE_SPLIT_LAMBDA,
        }
    }
}

impl ShadowOptions {
    /// Effective cascade map size for a device's maximum texture dimension.
    pub fn directional_map_size(&self, max_texture_dimension: u32) -> u32 {
        self.directional_map_size.unwrap_or(self.quality.directional_map_size()).clamp(16, max_texture_dimension)
    }

    /// Largest point-light atlas for a device's maximum texture dimension: a power of two. The atlas grows toward it
    /// only as far as the frame's shadowed lights need.
    pub fn point_atlas_size(&self, max_texture_dimension: u32) -> u32 {
        let largest = 1 << (31 - max_texture_dimension.max(MIN_POINT_FACE_SIZE).leading_zeros());
        self.quality.point_atlas_size().min(largest)
    }

    /// Effective largest point-light face for a device's maximum texture dimension: a power of two that fits the atlas
    /// with its siblings.
    pub fn point_map_size(&self, max_texture_dimension: u32) -> u32 {
        let requested = self.point_map_size.unwrap_or(self.quality.point_map_size()).max(MIN_POINT_FACE_SIZE);
        let largest = self.point_atlas_size(max_texture_dimension) / 4;
        requested.next_power_of_two().clamp(MIN_POINT_FACE_SIZE, largest.max(MIN_POINT_FACE_SIZE))
    }

    /// Shadowed point lights per frame, within the receiver's capacity.
    pub fn point_light_budget(&self) -> usize {
        self.max_point_lights.min(MAX_SHADOWED_POINT_LIGHTS)
    }
}

/// Far view depth of each cascade, blending uniform and logarithmic spacing; unused entries hold `far`.
pub(crate) fn cascade_splits(near: f32, far: f32, count: usize, lambda: f32) -> [f32; MAX_CASCADES] {
    let mut splits = [far; MAX_CASCADES];
    let count = count.clamp(1, MAX_CASCADES);
    let lambda = if lambda.is_finite() { lambda.clamp(0.0, 1.0) } else { DEFAULT_CASCADE_SPLIT_LAMBDA };
    for (index, split) in splits.iter_mut().enumerate().take(count) {
        let fraction = (index + 1) as f32 / count as f32;
        let logarithmic = near * (far / near).powf(fraction);
        let uniform = near + ((far - near) * fraction);
        *split = (uniform * (1.0 - lambda)) + (logarithmic * lambda);
    }
    splits
}

/// One directional cascade's light camera.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct CascadeFit {
    /// World to cascade clip space (orthographic, zero-to-one depth).
    pub matrix: Mat4,
    /// World size of one texel.
    pub texel: f32,
    /// World depth between the projection's near and far planes.
    pub depth_range: f32,
}

/// Orthographic light cameras covering each cascade's slice of the camera frustum.
///
/// `direction` points toward the light. Each cascade's volume reaches toward the light far enough to hold every caster
/// in `casters` above its footprint, and at most `caster_distance` beyond its bounding sphere.
pub(crate) fn fit_cascades(
    camera: &CameraFrame,
    direction: Vec3,
    splits: &[f32; MAX_CASCADES],
    count: usize,
    map_size: u32,
    caster_distance: f32,
    casters: &[Bounds],
) -> Vec<CascadeFit> {
    // The camera frustum's corners at its near and far planes.
    let inverse = camera.view_projection().inverse();
    let corner = |x: f32, y: f32, z: f32| {
        let point = inverse * Vec4::new(x, y, z, 1.0);
        point.truncate() / point.w
    };
    let screen_corners = [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)];
    let near_corners = screen_corners.map(|(x, y)| corner(x, y, 0.0));
    let far_corners = screen_corners.map(|(x, y)| corner(x, y, 1.0));

    // The light's basis: `toward` the light, and right and up across its maps.
    let toward = direction.try_normalize().unwrap_or(Vec3::Y);
    let reference_up = if toward.y.abs() > 0.99 { Vec3::Z } else { Vec3::Y };
    let right = (-toward).cross(reference_up).normalize();
    let up = right.cross(-toward).normalize();
    let depth_span = camera.far - camera.near;

    // Casters as boxes (center and half size); a box's reach along a unit axis is its half size dotted with the axis's
    // magnitudes.
    let caster_boxes: Vec<(Vec3, Vec3)> =
        casters.iter().map(|bounds| (bounds.center(), (bounds.max - bounds.min) * 0.5)).collect();
    let extent = |half: Vec3, axis: Vec3| half.dot(axis.abs());

    let mut slice_near = camera.near;
    let mut fits = Vec::with_capacity(count);
    for split in splits.iter().take(count.clamp(1, MAX_CASCADES)) {
        // Bound this slice of the frustum with a sphere, its radius rounded so small camera motions keep it fixed.
        let near_fraction = (slice_near - camera.near) / depth_span;
        let far_fraction = (split - camera.near) / depth_span;
        let mut corners = Vec::with_capacity(8);
        for (near, far) in near_corners.iter().zip(&far_corners) {
            corners.push(near.lerp(*far, near_fraction));
            corners.push(near.lerp(*far, far_fraction));
        }
        let center = corners.iter().copied().sum::<Vec3>() / corners.len() as f32;
        let radius = corners.iter().map(|corner| corner.distance(center)).fold(0.0_f32, f32::max);
        let radius = (radius * 16.0).ceil() / 16.0;

        // Move the center in whole texels across the map.
        let texel = (radius * 2.0) / map_size as f32;
        let snap = |axis: Vec3| {
            let along = center.dot(axis);
            axis * (((along / texel).round() * texel) - along)
        };
        let snapped = center + snap(right) + snap(up);

        // Reach toward the light only as far as the highest caster above the footprint.
        let mut reach = radius;
        for (caster_center, half) in &caster_boxes {
            let offset = *caster_center - snapped;
            let beside = offset.dot(right).abs() > radius + extent(*half, right);
            let above = offset.dot(up).abs() > radius + extent(*half, up);
            if beside || above {
                continue;
            }
            reach = reach.max(offset.dot(toward) + extent(*half, toward));
        }
        let reach = reach.min(radius + caster_distance) + texel;

        let eye = snapped + (toward * reach);
        let view = look_at(eye, snapped, up);
        let far = reach + radius;
        let projection = orthographic(-radius, radius, -radius, radius, CASCADE_NEAR, far);
        fits.push(CascadeFit {
            matrix: projection * view,
            texel,
            depth_range: far - CASCADE_NEAR,
        });
        slice_near = *split;
    }
    fits
}

/// Face forward and up directions, in tile order (+X, -X, +Y, -Y, +Z, -Z). The receiver in shadow-receiver.wgsl uses
/// the same table to select and project faces.
pub(crate) const POINT_FACE_ORIENTATIONS: [(Vec3, Vec3); POINT_FACES] = [
    (Vec3::X, Vec3::NEG_Y),
    (Vec3::NEG_X, Vec3::NEG_Y),
    (Vec3::Y, Vec3::Z),
    (Vec3::NEG_Y, Vec3::NEG_Z),
    (Vec3::Z, Vec3::NEG_Y),
    (Vec3::NEG_Z, Vec3::NEG_Y),
];

/// Near plane of a point light's face projections: one hundredth of its range, at most 0.1 world units.
pub(crate) fn point_near_plane(range: f32) -> f32 {
    0.1_f32.min(range * 0.01)
}

/// The six 90-degree view-projections of a point light, in [`POINT_FACE_ORIENTATIONS`] order.
pub(crate) fn point_face_views(position: Vec3, range: f32) -> [Mat4; POINT_FACES] {
    let projection = perspective(std::f32::consts::FRAC_PI_2, 1.0, point_near_plane(range), range);
    POINT_FACE_ORIENTATIONS.map(|(forward, up)| projection * look_at(position, position + forward, up))
}

/// The face size a point light's shadow deserves: about the height its reach covers on a `render_height`-pixel view,
/// as a power of two between [`MIN_POINT_FACE_SIZE`] and `largest`. A light whose reach holds the eye gets the
/// largest.
pub(crate) fn point_face_size(
    position: Vec3,
    range: f32,
    camera: &CameraFrame,
    render_height: u32,
    largest: u32,
) -> u32 {
    let vertical_scale = camera.projection.y_axis.y.abs();
    let perspective = camera.projection.z_axis.w != 0.0;
    let apparent_range = if perspective { range / camera.eye.distance(position).max(range) } else { range };
    let pixels = (0.5 * render_height as f32 * vertical_scale * apparent_range).ceil().max(1.0) as u32;
    pixels.next_power_of_two().clamp(MIN_POINT_FACE_SIZE, largest.max(MIN_POINT_FACE_SIZE))
}

#[cfg(test)]
#[path = "mod.test.rs"]
mod tests;
