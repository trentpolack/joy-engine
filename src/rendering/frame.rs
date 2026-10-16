// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Per-frame renderer inputs: lights, environment lighting, timing, transient effect triangles, and pre-sorted
//! transparent runs (colored or textured) such as particle batches.

use crate::core::{Rgb, Rgba};
use bytemuck::{Pod, Zeroable};
use glam::Vec3;

use crate::rendering::environment::ImageLighting;
use crate::rendering::texture::TextureAsset;

/// Maximum point lights per frame, matching the TypeScript engine.
pub const MAX_POINT_LIGHTS: usize = 32;

/// Default ambient irradiance when no environment is supplied; with an environment the default is zero, since the
/// environment's skylight provides ambient light.
pub const DEFAULT_AMBIENT_WITHOUT_ENVIRONMENT: f32 = 0.3;

/// A point light. Positions and range are world units; color is linear irradiance that reaches zero at `range`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointLight {
    /// World position.
    pub position: Vec3,
    /// Linear RGB irradiance at the light (color multiplied by intensity).
    pub color: Rgb,
    /// Distance at which the light's contribution reaches zero; must be positive.
    pub range: f32,
    /// Whether the light casts soft shadows from six depth faces. The TypeScript default is `true`.
    pub casts_shadow: bool,
}

/// One celestial light from the environment, in the renderer's terms.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CelestialLight {
    /// Unit direction toward the light, Y-up world space.
    pub direction: Vec3,
    /// Linear RGB color before intensity.
    pub color: Rgb,
    /// Radiance multiplier.
    pub intensity: f32,
    /// Disc angular radius in radians.
    pub angular_radius: f32,
    /// Whether the light casts the directional shadow when it is the primary light.
    pub casts_shadow: bool,
}

/// Everything the renderer reads from an environment frame: lighting for surfaces and the packed sky inputs.
///
/// The environment module's `EnvironmentFrame` converts into this record, keeping the renderer free of simulation clocks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EnvironmentLighting {
    /// Seconds of accumulated environment time (drives cloud advection).
    pub elapsed_seconds: f32,
    /// Normalized local solar time in `[0, 1)`.
    pub time_of_day: f32,
    /// The sun.
    pub sun: CelestialLight,
    /// The moon.
    pub moon: CelestialLight,
    /// The light that illuminates and shadows surfaces (the sun by day, the moon by night).
    pub primary: CelestialLight,
    /// Linear RGB skylight color.
    pub skylight_color: Rgb,
    /// Skylight intensity.
    pub skylight_intensity: f32,
    /// Wind velocity in world units per second.
    pub wind_velocity: Vec3,
    /// Rayleigh, Mie, ozone (horizon warmth), and density scattering multipliers.
    pub scattering: [f32; 4],
    /// Fog density, cloud coverage, cloud density, and cloud shadow strength.
    pub fog_and_clouds: [f32; 4],
    /// Cloud altitude, cloud thickness, light-shaft intensity, and light-shaft decay.
    pub cloud_shape_and_shafts: [f32; 4],
    /// Light-shaft exposure, ambient-occlusion intensity, and ambient-occlusion radius.
    pub shaft_exposure_and_occlusion: [f32; 3],
}

impl EnvironmentLighting {
    /// Pack into the sixteen-vec4 block shared with the TypeScript engine's environment uniforms.
    pub fn pack(&self) -> [[f32; 4]; 16] {
        let light = |light: &CelestialLight| -> ([f32; 4], [f32; 4]) {
            (
                light.direction.extend(light.intensity).to_array(),
                [light.color[0], light.color[1], light.color[2], light.angular_radius],
            )
        };
        let (sun_direction, sun_color) = light(&self.sun);
        let (moon_direction, moon_color) = light(&self.moon);
        let mut packed = [[0.0; 4]; 16];
        packed[0] = sun_direction;
        packed[1] = sun_color;
        packed[2] = moon_direction;
        packed[3] = moon_color;
        packed[4] = [self.skylight_color[0], self.skylight_color[1], self.skylight_color[2], self.skylight_intensity];
        packed[5] = self.wind_velocity.extend(self.elapsed_seconds).to_array();
        packed[6] = self.scattering;
        packed[7] = self.fog_and_clouds;
        packed[8] = self.cloud_shape_and_shafts;
        let [exposure, occlusion, radius] = self.shaft_exposure_and_occlusion;
        packed[9] = [exposure, occlusion, radius, self.time_of_day];
        packed
    }
}

/// A transient world-space vertex for procedural feedback and particles.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct ColorVertex {
    /// World position.
    pub position: [f32; 3],
    /// Straight-alpha linear RGBA.
    pub color: [f32; 4],
}

/// A transient world-space vertex with texture coordinates, for textured transparent draws such as particle sprites.
///
/// The layout (36 bytes: XYZ, RGBA, UV) matches the TypeScript textured particle stream.
#[repr(C)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Pod, Zeroable)]
pub struct TexturedVertex {
    /// World position.
    pub position: [f32; 3],
    /// Straight-alpha linear RGBA multiplied with the sampled texel.
    pub color: [f32; 4],
    /// Texture coordinates with a top-left origin.
    pub uv: [f32; 2],
}

/// One pre-sorted transparent run in [`FrameInputs::transparent_draws`].
///
/// Runs draw in the order supplied, after the frame's other transparency, testing depth against opaque geometry without
/// writing it, and blending straight alpha source-over. Incomplete trailing triangles are dropped.
#[derive(Clone, Copy, Debug)]
pub enum TransparentDraw<'a> {
    /// Unlit vertex-color triangles.
    Colored(&'a [ColorVertex]),
    /// Unlit triangles whose vertex color multiplies the texture (sampled as sRGB, linear filtering, clamped edges).
    Textured {
        /// The image to sample; uploaded on first use and released after its last handle drops.
        texture: &'a TextureAsset,
        /// Three vertices per triangle.
        vertices: &'a [TexturedVertex],
    },
}

impl TransparentDraw<'_> {
    /// Number of complete triangles in the run.
    pub fn triangle_count(&self) -> usize {
        match self {
            Self::Colored(vertices) => vertices.len() / 3,
            Self::Textured { vertices, .. } => vertices.len() / 3,
        }
    }
}

/// Borrowed inputs for one frame. Time is in seconds; geometry is world space.
#[derive(Clone, Copy, Debug)]
pub struct FrameInputs<'a> {
    /// Environment lighting and sky; `None` hides the sky and leaves only ambient and point lights.
    pub environment: Option<&'a EnvironmentLighting>,
    /// Point lights; at most [`MAX_POINT_LIGHTS`] are used.
    pub lights: &'a [PointLight],
    /// Ambient irradiance; `None` uses 0.3 without an environment and 0 with one.
    pub ambient: Option<f32>,
    /// Linear background color behind the sky and geometry.
    pub clear_color: Rgba,
    /// Scene time in seconds (grain animation and diagnostics).
    pub time_seconds: f32,
    /// Opaque triangles (three vertices each), lit like `Shading::Lit` meshes.
    pub opaque_triangles: &'a [ColorVertex],
    /// Transparent triangles (three vertices each), unlit and sorted back to front by centroid.
    pub transparent_triangles: &'a [ColorVertex],
    /// Pre-sorted transparent runs drawn last, in the supplied order and without re-sorting (see [`TransparentDraw`]).
    /// Particle effects supply these through `crate::particles::create_particle_effect_batches`, which interleaves
    /// colored and textured particles back to front.
    pub transparent_draws: &'a [TransparentDraw<'a>],
    /// Image-based lighting for PBR surfaces; `None` leaves them lit by direct, ambient, and skylight only.
    pub image_lighting: Option<ImageLighting<'a>>,
}

impl Default for FrameInputs<'_> {
    fn default() -> Self {
        Self {
            environment: None,
            lights: &[],
            ambient: None,
            clear_color: [0.0, 0.0, 0.0, 1.0],
            time_seconds: 0.0,
            opaque_triangles: &[],
            transparent_triangles: &[],
            transparent_draws: &[],
            image_lighting: None,
        }
    }
}

impl FrameInputs<'_> {
    /// Ambient irradiance after applying the environment-dependent default.
    pub fn resolved_ambient(&self) -> f32 {
        self.ambient.unwrap_or(if self.environment.is_some() { 0.0 } else { DEFAULT_AMBIENT_WITHOUT_ENVIRONMENT })
    }
}

/// Sort transient triangles back to front for a camera looking along `-backward`.
///
/// Equal depths keep input order. Incomplete trailing vertices are dropped. Intersecting transparent surfaces remain an
/// approximation, not order-independent transparency.
pub fn sort_transparent_triangles(vertices: &[ColorVertex], backward: Vec3) -> Vec<ColorVertex> {
    let mut triangles: Vec<(f32, usize)> = vertices
        .chunks_exact(3)
        .enumerate()
        .map(|(index, triangle)| {
            let depth: f32 = triangle.iter().map(|vertex| Vec3::from_array(vertex.position).dot(backward)).sum();
            (depth, index)
        })
        .collect();
    triangles.sort_by(|left, right| left.0.total_cmp(&right.0));
    triangles.into_iter().flat_map(|(_, index)| vertices[index * 3..(index * 3) + 3].iter().copied()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn transparent_triangles_sort_back_to_front() {
        let triangle = |z: f32| {
            [
                ColorVertex { position: [0.0, 0.0, z], color: [1.0; 4] },
                ColorVertex { position: [1.0, 0.0, z], color: [1.0; 4] },
                ColorVertex { position: [0.0, 1.0, z], color: [1.0; 4] },
            ]
        };
        let vertices: Vec<ColorVertex> = [triangle(5.0), triangle(-5.0), triangle(0.0)].concat();
        // Camera on +Z looking toward -Z: the farthest (most negative Z) triangle draws first.
        let sorted = sort_transparent_triangles(&vertices, Vec3::Z);
        let order: Vec<f32> = sorted.chunks_exact(3).map(|triangle| triangle[0].position[2]).collect();
        assert_eq!(order, [-5.0, 0.0, 5.0]);
    }

    #[test]
    fn ambient_default_depends_on_environment() {
        assert_eq!(FrameInputs::default().resolved_ambient(), DEFAULT_AMBIENT_WITHOUT_ENVIRONMENT);
    }
}
