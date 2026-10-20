// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Per-frame renderer inputs: lights, environment lighting, transient effect triangles, and pre-sorted transparent runs
//! (colored or textured) such as particle batches, plus the presentation clock the shell supplies.

use bytemuck::{
    Pod,
    Zeroable,
};
use joy_core::LinearColor;
use joy_core::math::Vec3;
use joy_render_types::{
    DirectionalLight,
    PointLight,
    Skylight,
};

use crate::environment::ImageLighting;
use crate::texture::TextureAsset;

/// Point lights the renderer draws per frame: the width of the light texture every lit pass reads (`pointLights` in
/// `scene-common.wgsl`). Each surface reads only the lights whose range reaches its light cluster, so the cost of many
/// lights follows the pixels they light. Frames may supply more; the renderer keeps the ones that contribute most to
/// the view (see [`FrameInputs::lights`]).
pub const MAX_POINT_LIGHTS: usize = 1024;

/// Default ambient irradiance when no environment is supplied; with an environment the default is zero, since the
/// environment's skylight provides ambient light.
pub const DEFAULT_AMBIENT_WITHOUT_ENVIRONMENT: f32 = 0.3;

/// Everything the renderer reads from an environment frame: lighting for surfaces and the packed sky inputs.
///
/// The environment module's `EnvironmentFrame` converts into this record, keeping the renderer free of simulation clocks.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EnvironmentLighting {
    /// Seconds of accumulated environment time (drives cloud advection).
    pub elapsed_seconds: f32,
    /// Normalized local solar time in `[0, 1)`.
    pub time_of_day: f32,
    /// The sun; the sky draws its disc and glow.
    pub sun: DirectionalLight,
    /// The moon; the sky draws its disc.
    pub moon: DirectionalLight,
    /// The light that illuminates and shadows surfaces (the sun by day, the moon by night), unless the frame supplies
    /// its own [`FrameInputs::directional_light`].
    pub primary: DirectionalLight,
    /// Ambient light from the sky and the ground.
    pub skylight: Skylight,
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
    /// Pack into the sixteen-vec4 `environment` block of the sky uniforms, in the slot order `atmosphere.wgsl` reads.
    pub fn pack(&self) -> [[f32; 4]; 16] {
        let light = |light: &DirectionalLight| -> ([f32; 4], [f32; 4]) {
            (
                light.direction.extend(light.intensity).to_array(),
                light.color.with_alpha(light.angular_radius).to_array(),
            )
        };
        let (sun_direction, sun_color) = light(&self.sun);
        let (moon_direction, moon_color) = light(&self.moon);
        let mut packed = [[0.0; 4]; 16];
        packed[0] = sun_direction;
        packed[1] = sun_color;
        packed[2] = moon_direction;
        packed[3] = moon_color;
        packed[4] = self.skylight.color.with_alpha(self.skylight.intensity).to_array();
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
/// The layout (36 bytes: XYZ, RGBA, UV) matches the vertex attributes of `textured.wgsl`.
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

/// One transparent run in [`FrameInputs::transparent_draws`].
///
/// Each triangle sorts back to front with the frame's other transparency (equal depths keep the supplied order), tests
/// depth against opaque geometry without writing it, and blends straight alpha source-over. Incomplete trailing
/// triangles are dropped.
#[derive(Clone, Copy, Debug)]
pub enum TransparentDraw<'a> {
    /// Unlit vertex-color triangles.
    Colored(&'a [ColorVertex]),
    /// Unlit triangles whose vertex color multiplies the texture (decoded from sRGB, sampled trilinearly from generated
    /// mipmaps with anisotropic filtering, clamped edges).
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
            Self::Textured {
                vertices,
                ..
            } => vertices.len() / 3,
        }
    }
}

/// The presentation clock: real frame time from the shell that drives the renderer, independent of any app's pausable
/// scene time.
///
/// Temporal effects (antialiasing history resets, exposure adaptation, film grain, and volumetric noise) read this clock
/// so they behave the same in every game, tool, and example. The windowed and headless shells set it before each render
/// phase ([`Renderer::set_presentation_time`](crate::Renderer::set_presentation_time)); scene animation stays on the
/// app's own clock.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PresentationTime {
    /// Seconds since the shell started presenting; never decreases.
    pub time_seconds: f64,
    /// Seconds since the previous presented frame: bounded by the shell, and zero for the first frame and after the
    /// shell resets its clock (pauses and hidden windows).
    pub elapsed_seconds: f32,
}

impl PresentationTime {
    /// The clock of frame `frame_index` (zero-based) presented every `frame_seconds`, as the headless shell runs it;
    /// for renderers driven without a shell, such as tests and offline captures.
    pub fn fixed_rate(frame_index: u32, frame_seconds: f32) -> Self {
        Self {
            time_seconds: f64::from(frame_index) * f64::from(frame_seconds),
            elapsed_seconds: if frame_index == 0 { 0.0 } else { frame_seconds },
        }
    }
}

/// Borrowed inputs for one frame. Geometry is world space; time comes from the renderer's presentation clock.
#[derive(Clone, Copy, Debug)]
pub struct FrameInputs<'a> {
    /// Environment lighting and sky; `None` hides the sky and leaves the directional light, ambient, and point lights.
    pub environment: Option<&'a EnvironmentLighting>,
    /// The directional light that illuminates and shadows surfaces, such as a studio key light; `None` uses the
    /// environment's primary light (or no directional light without an environment).
    pub directional_light: Option<DirectionalLight>,
    /// Point lights, in any number. Lights whose range cannot reach the camera's view are skipped, invalid lights
    /// (non-finite values, a range that is not positive, or negative color) are skipped, and of the rest the
    /// [`MAX_POINT_LIGHTS`] with the largest estimated contribution to the view are drawn. Skipped lights are counted
    /// in `RendererDiagnostics::point_lights_omitted`.
    pub lights: &'a [PointLight],
    /// Ambient irradiance; `None` uses 0.3 without an environment and 0 with one.
    pub ambient: Option<f32>,
    /// Linear background color behind the sky and geometry.
    pub clear_color: LinearColor,
    /// Opaque triangles (three vertices each), lit like `Shading::Lit` meshes.
    pub opaque_triangles: &'a [ColorVertex],
    /// Transparent triangles (three vertices each), unlit. Every transparent element of a frame (these triangles,
    /// blended scene meshes, and [`transparent_draws`](Self::transparent_draws)) is sorted back to front together, by
    /// view depth: a triangle by its centroid and a mesh by the center of its bounds.
    pub transparent_triangles: &'a [ColorVertex],
    /// Transparent runs (see [`TransparentDraw`]), such as the particle batches from
    /// `crate::particles::create_particle_effect_batches`. Their triangles sort back to front with the frame's other
    /// transparency, so a particle behind a glass mesh draws before it; triangles at equal depth keep the supplied
    /// order.
    pub transparent_draws: &'a [TransparentDraw<'a>],
    /// Image-based lighting for PBR surfaces; `None` leaves them lit by direct, ambient, and skylight only.
    pub image_lighting: Option<ImageLighting<'a>>,
}

impl Default for FrameInputs<'_> {
    fn default() -> Self {
        Self {
            environment: None,
            directional_light: None,
            lights: &[],
            ambient: None,
            clear_color: LinearColor::BLACK,
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

    /// The directional light that lights and shadows this frame: the frame's own, else the environment's primary light.
    pub fn key_light(&self) -> Option<DirectionalLight> {
        self.directional_light.or_else(|| self.environment.map(|environment| environment.primary))
    }

    /// The frame's skylight: the environment's, or none.
    pub fn skylight(&self) -> Skylight {
        self.environment.map_or_else(Skylight::default, |environment| environment.skylight)
    }
}

#[cfg(test)]
#[path = "frame.test.rs"]
mod tests;
