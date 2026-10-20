// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The particle preview, drawn only through the engine renderer: an orbit camera framed
//! into the preview region, the editor grid, an optional matte light-receiver surface, editor backgrounds, particle
//! textures with a prepare/activate lifecycle, the lab's display processing, and per-frame statistics for the
//! performance monitor.
//!
//! The viewport borrows the application's effect each frame and never advances it; preview settings are presentation
//! only and never change the exported effect.

use joy_engine::core::LinearColor;
use joy_engine::math::{
    Vec2,
    Vec3,
};
use joy_engine::particles::{
    CompiledParticleEffect,
    ParticleBatch,
    ParticleCameraBasis,
    ParticleEffect,
    ParticleRenderMetrics,
    ParticleTextureSet,
    collect_particle_lights,
    create_particle_effect_batches,
    create_particle_render_metrics,
};
use joy_engine::rendering::post_process::{
    AgxLook,
    ToneMapper,
};
use joy_engine::rendering::{
    ColorVertex,
    EditorGrid,
    EditorGridView,
    FrameInputs,
    OrbitCamera,
    PostProcessSettings,
    RegionCamera,
    RenderScene,
    TransparentDraw,
};
use joy_engine::runtime::{
    RenderTarget,
    Stopwatch,
};

use crate::constants::{
    PREVIEW_LIGHT_LIMIT,
    PerformanceMode,
};

/// Height of the light-receiver surface; the grid floats two units above it.
pub const PREVIEW_RECEIVER_HEIGHT: f64 = -120.0;

/// Half the receiver's side, in world units.
const RECEIVER_HALF_SIZE: f32 = 500.0;

/// The receiver's matte linear color.
const RECEIVER_COLOR: [f32; 4] = [0.075, 0.085, 0.105, 1.0];

/// Ambient light on the receiver.
const PREVIEW_AMBIENT: f32 = 0.7;

/// Editor backgrounds (linear) in menu order.
pub const PREVIEW_BACKGROUNDS: [(&str, &str, LinearColor); 5] = [
    ("midnight", "Midnight", LinearColor::rgb(0.028, 0.05, 0.087)),
    ("slate", "Slate", LinearColor::rgb(0.09, 0.13, 0.19)),
    ("gray", "Gray", LinearColor::rgb(0.25, 0.25, 0.25)),
    ("cream", "Cream", LinearColor::rgb(0.75, 0.66, 0.47)),
    ("black", "Black", LinearColor::BLACK),
];

/// The lab's display processing.
pub fn post_process_settings() -> PostProcessSettings {
    PostProcessSettings {
        tone_mapper: ToneMapper::Agx,
        agx_look: AgxLook::Punchy,
        exposure: 1.05,
        bloom_strength: 0.8,
        bloom_radius: 0.025,
        bloom_threshold: 0.8,
        bloom_knee: 0.4,
        vignette_strength: 0.24,
        vignette_radius: 0.78,
        ..PostProcessSettings::DEFAULT
    }
}

/// Where the preview draws: the whole target's logical size and the preview region inside it (logical pixels, top-left
/// origin).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PreviewRegion {
    /// The whole render target's logical size.
    pub viewport_size: Vec2,
    /// The region's top-left corner.
    pub min: Vec2,
    /// The region's size.
    pub size: Vec2,
}

impl PreviewRegion {
    /// A region covering the whole target.
    pub fn full(size: Vec2) -> Self {
        Self {
            viewport_size: size,
            min: Vec2::ZERO,
            size,
        }
    }
}

/// One emitter's geometry statistics in detailed profiling.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EmitterGeometry {
    /// Emitter id.
    pub id: String,
    /// Triangles built in the last render.
    pub triangles: u64,
    /// Vertices built in the last render.
    pub vertices: u64,
    /// Geometry CPU time in the last render, in milliseconds.
    pub geometry_ms: f64,
    /// Particle lights the emitter contributed.
    pub lights: usize,
}

/// Statistics from the last rendered frame.
#[derive(Clone, Debug, PartialEq)]
pub struct PreviewStatistics {
    /// Geometry, sorting, and batch assembly CPU time, in milliseconds.
    pub geometry_ms: f64,
    /// CPU time recording the frame's commands, in milliseconds (not GPU execution).
    pub submission_ms: f64,
    /// Sort and batch time in detailed profiling, in milliseconds.
    pub sort_ms: f64,
    /// Nonempty scene geometry batches (post-processing draws excluded).
    pub scene_batches: usize,
    /// Triangles submitted.
    pub triangles: usize,
    /// Vertices submitted.
    pub vertices: usize,
    /// Vertex bytes submitted (excluding uniforms and textures).
    pub upload_bytes: usize,
    /// Particle lights drawn.
    pub lights: usize,
    /// Particle lights over the preview's light cap.
    pub omitted_lights: usize,
    /// Display range, `HDR Scene → SDR Output`.
    pub range: String,
    /// Per-emitter geometry in detailed profiling.
    pub by_emitter: Vec<EmitterGeometry>,
}

impl Default for PreviewStatistics {
    fn default() -> Self {
        Self {
            geometry_ms: 0.0,
            submission_ms: 0.0,
            sort_ms: 0.0,
            scene_batches: 0,
            triangles: 0,
            vertices: 0,
            upload_bytes: 0,
            lights: 0,
            omitted_lights: 0,
            range: "Starting…".to_owned(),
            by_emitter: Vec::new(),
        }
    }
}

/// What a camera drag does.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CameraGesture {
    /// Rotate around the target.
    Orbit,
    /// Move the target in the view plane.
    Pan,
}

/// Framing values a camera reset restores.
#[derive(Clone, Copy, Debug, PartialEq)]
struct CameraHome {
    target: Vec3,
    distance: f32,
    radius: f32,
}

/// Owns the preview camera, grid, textures, and statistics; borrows the effect per frame.
pub struct ParticleViewport {
    camera: OrbitCamera,
    home: CameraHome,
    grid: EditorGrid,
    grid_spacing: Option<f64>,
    scene: RenderScene,
    textures: ParticleTextureSet,
    textures_ready: bool,
    metrics: Option<ParticleRenderMetrics>,
    performance_mode: PerformanceMode,
    stats: PreviewStatistics,
    /// Whether the editor grid draws.
    pub grid_visible: bool,
    /// Whether the matte light receiver draws.
    pub receiver_visible: bool,
    background: usize,
}

impl Default for ParticleViewport {
    fn default() -> Self {
        Self::new()
    }
}

impl ParticleViewport {
    /// A viewport with the lab's starting camera, the grid on, the receiver off, and the slate background.
    pub fn new() -> Self {
        let camera = OrbitCamera {
            distance: 650.0,
            radius: 250.0,
            ..OrbitCamera::default()
        };
        Self {
            home: CameraHome {
                target: camera.target,
                distance: camera.distance,
                radius: camera.radius,
            },
            camera,
            grid: EditorGrid::new(),
            grid_spacing: None,
            scene: RenderScene::new(),
            textures: ParticleTextureSet::new(),
            textures_ready: false,
            metrics: None,
            performance_mode: PerformanceMode::Overview,
            stats: PreviewStatistics::default(),
            grid_visible: true,
            receiver_visible: false,
            background: 1,
        }
    }

    /// The preview camera.
    pub fn camera(&self) -> &OrbitCamera {
        &self.camera
    }

    /// The last frame's statistics.
    pub fn stats(&self) -> &PreviewStatistics {
        &self.stats
    }

    /// The grid's minor spacing in the last frame, or `None` when the grid was off.
    pub fn grid_spacing(&self) -> Option<f64> {
        self.grid_spacing
    }

    /// The decoded particle textures.
    pub fn textures(&self) -> &ParticleTextureSet {
        &self.textures
    }

    /// Whether textured particles may draw (an effect was committed after its images were prepared).
    pub fn textures_ready(&self) -> bool {
        self.textures_ready
    }

    /// The selected background's index into [`PREVIEW_BACKGROUNDS`].
    pub fn background(&self) -> usize {
        self.background
    }

    /// Select a background by key (`midnight`, `slate`, …).
    pub fn set_background(&mut self, key: &str) -> Result<(), String> {
        let index =
            PREVIEW_BACKGROUNDS.iter().position(|(name, _, _)| *name == key).ok_or("Unknown preview background.")?;
        self.background = index;
        Ok(())
    }

    /// Decode the images `definition` needs before the application commits a new simulation. A failed decode leaves
    /// the current preview and its images intact.
    pub fn prepare_effect(&mut self, definition: &CompiledParticleEffect) -> Result<(), String> {
        self.textures.prepare(definition).map_err(|error| error.to_string())
    }

    /// Commit `definition` (prepared first) as the displayed one, releasing images it no longer uses.
    pub fn set_effect(&mut self, definition: Option<&CompiledParticleEffect>) {
        self.metrics = definition
            .filter(|_| self.performance_mode == PerformanceMode::Detailed)
            .map(create_particle_render_metrics);
        self.textures.activate(definition);
        self.textures_ready = true;
    }

    /// Release every image.
    pub fn release_textures(&mut self) {
        self.textures.clear();
        self.textures_ready = false;
    }

    /// Switch summary or detailed timing without changing the effect.
    pub fn set_performance_mode(&mut self, mode: PerformanceMode, definition: Option<&CompiledParticleEffect>) {
        if self.performance_mode == mode {
            return;
        }
        self.performance_mode = mode;
        self.metrics = definition.filter(|_| mode == PerformanceMode::Detailed).map(create_particle_render_metrics);
        self.stats.by_emitter.clear();
        self.stats.sort_ms = 0.0;
    }

    /// Apply a drag of `delta` logical pixels in a preview `region_height` pixels tall.
    pub fn drag(&mut self, gesture: CameraGesture, delta: Vec2, region_height: f32) {
        match gesture {
            CameraGesture::Pan => {
                let basis = self.camera.basis();
                let scale = self.camera.distance / region_height.max(100.0);
                self.camera.target += (-(delta.x * scale) * basis.right) + ((delta.y * scale) * basis.up);
            }
            CameraGesture::Orbit => {
                self.camera.yaw -= delta.x * 0.007;
                self.camera.pitch = (self.camera.pitch + (delta.y * 0.007)).clamp(-1.5, 1.5);
            }
        }
    }

    /// Zoom by wheel page pixels (positive zooms out).
    pub fn zoom(&mut self, delta: f32) {
        let factor = (delta.clamp(-200.0, 200.0) * 0.001).exp();
        self.camera.distance = (self.camera.distance * factor).clamp(5.0, 100_000.0);
    }

    /// Orbit by keyboard: `yaw` and `pitch` in steps of 0.1 radians.
    pub fn nudge(&mut self, yaw_steps: f32, pitch_steps: f32) {
        self.camera.yaw += yaw_steps * 0.1;
        self.camera.pitch = (self.camera.pitch + (pitch_steps * 0.1)).clamp(-1.5, 1.5);
    }

    /// Restore the remembered framing with the default orbit angles.
    pub fn reset_camera(&mut self) {
        self.camera = OrbitCamera {
            target: self.home.target,
            distance: self.home.distance,
            radius: self.home.radius,
            ..OrbitCamera::default()
        };
    }

    /// Fit the current particle volume for a preview of `region_size`, optionally making it the reset framing. With no
    /// particles the camera resets.
    pub fn frame_effect(&mut self, effect: Option<&ParticleEffect>, region_size: Vec2, remember_as_home: bool) {
        let mut positions = Vec::new();
        for particle in effect.map(ParticleEffect::particles).unwrap_or_default() {
            let size = particle.size;
            positions.push(Vec3::new(
                (particle.x - size) as f32,
                (particle.y - size) as f32,
                (particle.z - size) as f32,
            ));
            positions.push(Vec3::new(
                (particle.x + size) as f32,
                (particle.y + size) as f32,
                (particle.z + size) as f32,
            ));
        }
        if positions.is_empty() {
            self.reset_camera();
            return;
        }
        self.camera.frame_positions(&positions);
        self.camera.distance *= (region_size.y / region_size.x.max(1.0)).max(1.0);
        if remember_as_home {
            self.home = CameraHome {
                target: self.camera.target,
                distance: self.camera.distance,
                radius: self.camera.radius,
            };
        }
    }

    /// The matte receiver surface's two triangles.
    pub fn receiver_geometry() -> Vec<ColorVertex> {
        let height = PREVIEW_RECEIVER_HEIGHT as f32;
        let corner = |x: f32, z: f32| ColorVertex {
            position: [x, height, z],
            color: RECEIVER_COLOR,
        };
        let size = RECEIVER_HALF_SIZE;
        vec![
            corner(-size, -size),
            corner(size, -size),
            corner(size, size),
            corner(-size, -size),
            corner(size, size),
            corner(-size, size),
        ]
    }

    /// Draw `effect` at its current simulation state into `region` of `target` without advancing it.
    pub fn render(
        &mut self,
        target: &mut RenderTarget<'_>,
        effect: Option<&ParticleEffect>,
        region: PreviewRegion,
    ) -> Result<(), String> {
        let monitoring = self.performance_mode != PerformanceMode::Disabled;
        let geometry_watch = Stopwatch::start();
        let basis = self.camera.basis();
        let illumination = effect.map(|effect| collect_particle_lights(effect, PREVIEW_LIGHT_LIMIT));
        let lights = illumination.as_ref().map(|selection| selection.point_lights()).unwrap_or_default();
        let opaque = if self.receiver_visible { Self::receiver_geometry() } else { Vec::new() };
        let grid: Vec<ColorVertex> = if self.grid_visible && region.size.y > 0.0 {
            let view = EditorGridView {
                target: self.camera.target.to_array().map(f64::from),
                backward: basis.backward.to_array().map(f64::from),
                distance: f64::from(self.camera.distance),
                viewport_height_pixels: f64::from(region.size.y),
                height: PREVIEW_RECEIVER_HEIGHT + 2.0,
            };
            let frame = self.grid.frame(&view).map_err(|error| error.to_string())?;
            self.grid_spacing = Some(frame.spacing);
            frame.triangles.clone()
        } else {
            self.grid_spacing = None;
            Vec::new()
        };
        let batches = match effect {
            Some(effect) if self.textures_ready => {
                create_particle_effect_batches(effect, &ParticleCameraBasis::from(basis), self.metrics.as_mut(), &grid)
                    .map_err(|error| error.to_string())?
            }
            _ if grid.is_empty() => Vec::new(),
            _ => vec![ParticleBatch::Colored(grid)],
        };
        let draws: Vec<TransparentDraw<'_>> =
            self.textures.transparent_draws(&batches).map_err(|error| error.to_string())?;
        let geometry_ms = if monitoring { geometry_watch.elapsed_milliseconds() } else { 0.0 };

        let camera = RegionCamera::new(&self.camera, region.viewport_size, region.min, region.size);
        let inputs = FrameInputs {
            lights: &lights,
            ambient: Some(PREVIEW_AMBIENT),
            clear_color: PREVIEW_BACKGROUNDS[self.background].2,
            opaque_triangles: &opaque,
            transparent_draws: &draws,
            ..FrameInputs::default()
        };
        let submission_watch = Stopwatch::start();
        target.draw(&self.scene, &camera, &inputs).map_err(|error| error.to_string())?;
        if !monitoring {
            return Ok(());
        }
        let submission_ms = submission_watch.elapsed_milliseconds();
        let vertices = opaque.len() + draws.iter().map(|draw| draw.triangle_count() * 3).sum::<usize>();
        let upload_bytes = (opaque.len() * std::mem::size_of::<ColorVertex>())
            + batches
                .iter()
                .map(|batch| match batch {
                    ParticleBatch::Colored(vertices) => vertices.len() * std::mem::size_of::<ColorVertex>(),
                    ParticleBatch::Textured {
                        vertices,
                        ..
                    } => vertices.len() * 36,
                })
                .sum::<usize>();
        let hdr = target.renderer().diagnostics().hdr;
        let by_emitter = match (&self.metrics, &illumination) {
            (Some(metrics), Some(illumination)) => metrics
                .emitters
                .iter()
                .enumerate()
                .map(|(index, row)| EmitterGeometry {
                    id: row.id.clone(),
                    triangles: row.triangles,
                    vertices: row.vertices,
                    geometry_ms: row.geometry_ms,
                    lights: illumination.lights.iter().filter(|light| light.emitter_index == index).count(),
                })
                .collect(),
            _ => Vec::new(),
        };
        self.stats = PreviewStatistics {
            geometry_ms,
            submission_ms,
            sort_ms: self.metrics.as_ref().map_or(0.0, |metrics| metrics.sort_ms),
            scene_batches: usize::from(!opaque.is_empty())
                + batches.iter().filter(|batch| batch.vertex_count() > 0).count(),
            triangles: vertices / 3,
            vertices,
            upload_bytes,
            lights: lights.len(),
            omitted_lights: illumination.as_ref().map_or(0, |selection| selection.omitted),
            range: format!("{} Scene → SDR Output", if hdr { "HDR" } else { "SDR" }),
            by_emitter,
        };
        Ok(())
    }
}
