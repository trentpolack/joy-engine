// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! The preview: accepted FORM geometry drawn by the engine renderer.
//!
//! Geometry becomes a mesh through `form_mesh_asset` (the path games and the editor use), surfaces shade in the
//! PBR pass with `MaterialTextures`, the studio environment lights them through `Renderer::prepare_environment_map`,
//! the direct light casts directional (and alpha-masked) shadows, `EditorGrid` draws the reference grid, and point and
//! inspection markers are unlit transparent runs. FORM content therefore looks the same here, in Joy Editor, and in
//! games.
//!
//! The preview occupies a rectangle of the window between the tool's panels. The engine's `RegionCamera` maps the orbit
//! camera onto that rectangle with its own aspect, field of view, and centering while the renderer draws the whole
//! window.

use joy_engine::core::Transform;
use joy_engine::form::GeometryData;
use joy_engine::glam::{Vec2, Vec3};
use joy_engine::rendering::{
    Camera, CelestialLight, ColorVertex, EditorGrid, EditorGridView, EnvironmentData, EnvironmentLighting,
    EnvironmentMap, EnvironmentPixels, FormTextureCache, FrameInputs, ImageLighting, MeshInstance, MeshInstanceId,
    OrbitCamera, RegionCamera, RenderPassKind, RenderScene, Renderer, Shading, ShadowOptions, TransparentDraw,
    form_mesh_asset_with_textures, sort_transparent_triangles,
};
use joy_engine::runtime::RenderTarget;

use crate::constants::ViewMode;
use crate::rendering::batches::{MarkerCamera, point_batch};
use crate::rendering::inspection_overlay::InspectionOverlay;
use crate::rendering::preview_lighting::{
    EDITOR_AMBIENT, EDITOR_BACKGROUND, PreviewLightingSettings, STUDIO_ENVIRONMENT, create_studio_pixels,
    studio_environment_options,
};

/// Angular radius of the studio light, matching the sun disc the engine's soft shadows assume.
const LIGHT_ANGULAR_RADIUS: f32 = 0.00465;

/// Orbit sensitivity in radians per logical pixel.
const ORBIT_RADIANS_PER_PIXEL: f32 = 0.007;

/// Pitch limit in radians.
const PITCH_LIMIT: f32 = 1.5;

/// A rectangle of the window in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PaneRect {
    /// Top-left corner.
    pub min: Vec2,
    /// Width and height.
    pub size: Vec2,
}

impl PaneRect {
    /// Whether a logical point lies inside.
    pub fn contains(&self, point: Vec2) -> bool {
        point.cmpge(self.min).all() && point.cmplt(self.min + self.size).all()
    }
}

/// Owns the preview scene, camera, grid, studio environment, and accepted geometry.
pub struct Viewport {
    // Scene content: the accepted mesh and its decoded maps.
    scene: RenderScene,
    instance: Option<MeshInstanceId>,
    textures: FormTextureCache,
    environment: Option<EnvironmentMap>,

    // The accepted geometry paired with the drawn mesh, for markers and framing.
    data: Option<GeometryData>,
    display_revision: u64,

    // Camera and presentation.
    /// The orbit camera.
    pub camera: OrbitCamera,
    grid: EditorGrid,
    grid_height: f64,
    grid_spacing: Option<f64>,
    /// Shaded surfaces or every vertex as a point.
    pub mode: ViewMode,
    /// Whether the editor-only line grid draws.
    pub show_grid: bool,
    /// Point marker size, one to six.
    pub point_size: f32,
    /// Inspection markers and selection.
    pub inspection: InspectionOverlay,
    lighting: PreviewLightingSettings,
    pane: Option<PaneRect>,
    viewport: Vec2,
}

impl Default for Viewport {
    fn default() -> Self {
        Self::new()
    }
}

impl Viewport {
    /// An empty preview with the default camera and inspection lighting.
    pub fn new() -> Self {
        Self {
            scene: RenderScene::new(),
            instance: None,
            textures: FormTextureCache::new(),
            environment: None,
            data: None,
            display_revision: 0,
            camera: OrbitCamera::default(),
            grid: EditorGrid::new(),
            grid_height: 0.0,
            grid_spacing: None,
            mode: ViewMode::Shaded,
            show_grid: true,
            point_size: 2.0,
            inspection: InspectionOverlay::default(),
            lighting: PreviewLightingSettings::default(),
            pane: None,
            viewport: Vec2::ZERO,
        }
    }

    /// Apply the renderer settings the preview needs (no sky; the studio environment prepared once). Call when the tool
    /// starts and whenever it becomes active in a shared renderer.
    pub fn prepare(&mut self, renderer: &mut Renderer) -> Result<(), String> {
        renderer.set_atmosphere(false);
        if self.environment.is_none() {
            let pixels = create_studio_pixels();
            let image = EnvironmentPixels {
                width: STUDIO_ENVIRONMENT.width,
                height: STUDIO_ENVIRONMENT.height,
                data: EnvironmentData::Float(&pixels),
            };
            let map = renderer
                .prepare_environment_map(&image, &studio_environment_options())
                .map_err(|error| format!("Studio lighting is unavailable: {error}"))?;
            self.environment = Some(map);
        }
        Ok(())
    }

    /// Whether the studio environment is ready.
    pub fn has_environment(&self) -> bool {
        self.environment.is_some()
    }

    /// The geometry currently displayed.
    pub fn data(&self) -> Option<&GeometryData> {
        self.data.as_ref()
    }

    /// How many geometries have been accepted; increases with every successful [`set_data`](Self::set_data).
    pub fn display_revision(&self) -> u64 {
        self.display_revision
    }

    /// The grid's minor spacing in world units from the latest frame, or `None` while the grid is hidden.
    pub fn grid_spacing(&self) -> Option<f64> {
        self.grid_spacing
    }

    /// The preview lighting.
    pub fn lighting(&self) -> PreviewLightingSettings {
        self.lighting
    }

    /// Change preview-only lighting after validating it; documents and exports never see it.
    pub fn set_lighting(&mut self, settings: PreviewLightingSettings) -> Result<(), String> {
        settings.validate()?;
        self.lighting = settings;
        Ok(())
    }

    /// The scene, for tests and diagnostics.
    pub fn scene(&self) -> &RenderScene {
        &self.scene
    }

    /// Make `data` the displayed geometry once its mesh (and every material map) is ready.
    ///
    /// On failure the previous mesh, markers, and framing stay paired and displayed, and the error explains why. Point-only
    /// geometry has no mesh and is always accepted. `frame` fits the camera to the new geometry.
    pub fn set_data(&mut self, data: &GeometryData, frame: bool) -> Result<(), String> {
        let asset = if data.triangles.is_empty() {
            None
        } else {
            let asset = form_mesh_asset_with_textures(data, Shading::Pbr, &mut self.textures)
                .map_err(|error| format!("Material preview unavailable: {error}"))?;
            Some(asset)
        };
        if let Some(previous) = self.instance.take() {
            self.scene.remove(previous);
        }
        if let Some(asset) = asset {
            let mut instance = MeshInstance::new(asset).with_shading(Shading::Pbr).with_transform(Transform::IDENTITY);
            // Blended materials draw in the PBR blend queue; the rest of the mesh stays opaque and casts shadows.
            instance.pass = RenderPassKind::Opaque;
            instance.visible = self.mode == ViewMode::Shaded;
            self.instance = Some(self.scene.insert(instance));
        }
        let sources: std::collections::HashSet<String> = data
            .form_materials()
            .into_iter()
            .flat_map(|material| {
                let maps = material.textures;
                [maps.base_color, maps.normal, maps.emissive, maps.metallic, maps.roughness, maps.ao]
            })
            .flatten()
            .collect();
        self.textures.retain(|source| sources.contains(source));
        self.data = Some(data.clone());
        self.display_revision += 1;
        if frame {
            self.camera.frame_positions(&positions(data));
        }
        let floor = data.positions.iter().skip(1).step_by(3).fold(0.0_f64, |floor, y| floor.min(*y));
        self.grid_height = floor - f64::from(self.camera.radius) * 0.015;
        Ok(())
    }

    /// Switch between shaded surfaces and point markers.
    pub fn set_mode(&mut self, mode: ViewMode) {
        self.mode = mode;
        if let Some(instance) = self.instance.and_then(|id| self.scene.get_mut(id)) {
            instance.visible = mode == ViewMode::Shaded;
        }
    }

    /// Fit the accepted geometry; tall panes back the camera off so it still fits horizontally.
    pub fn frame(&mut self) {
        if let Some(data) = &self.data {
            self.camera.frame_positions(&positions(data));
            if let Some(pane) = self.pane
                && pane.size.x < pane.size.y
            {
                self.camera.distance *= pane.size.y / pane.size.x.max(1.0);
            }
        }
    }

    /// Restore the default orientation, then fit accepted geometry.
    pub fn reset_camera(&mut self) {
        self.camera = OrbitCamera::default();
        let positions = self.data.as_ref().map(positions).unwrap_or_default();
        self.camera.frame_positions(&positions);
        self.frame();
    }

    /// Orbit by a pointer movement in logical pixels.
    pub fn orbit(&mut self, delta: Vec2) {
        self.camera.yaw -= delta.x * ORBIT_RADIANS_PER_PIXEL;
        self.camera.pitch = (self.camera.pitch + (delta.y * ORBIT_RADIANS_PER_PIXEL)).clamp(-PITCH_LIMIT, PITCH_LIMIT);
    }

    /// Pan by a pointer movement in logical pixels; the target follows the cursor at the focus distance.
    pub fn pan(&mut self, delta: Vec2) {
        let basis = self.camera.basis();
        let height = self.pane.map_or(self.viewport.y, |pane| pane.size.y);
        let scale = self.camera.distance / height.max(100.0);
        self.camera.target += (basis.up * delta.y * scale) - (basis.right * delta.x * scale);
    }

    /// Zoom by a wheel movement in logical pixels (positive moves away).
    pub fn zoom(&mut self, delta: f32) {
        let radius = self.camera.radius;
        self.camera.distance =
            (self.camera.distance * (delta.clamp(-200.0, 200.0) * 0.001).exp()).clamp(radius * 0.05, radius * 100.0);
    }

    /// Keyboard shortcuts while the preview has focus: `f` frames, `0` resets, and arrows orbit.
    pub fn shortcut(&mut self, key: &str) {
        match key {
            "f" => self.frame(),
            "0" => self.reset_camera(),
            "arrowleft" => self.camera.yaw -= 0.1,
            "arrowright" => self.camera.yaw += 0.1,
            "arrowup" => self.camera.pitch = (self.camera.pitch + 0.1).min(PITCH_LIMIT),
            "arrowdown" => self.camera.pitch = (self.camera.pitch - 0.1).max(-PITCH_LIMIT),
            _ => {}
        }
    }

    /// Set the preview rectangle within a viewport of `viewport` logical pixels; `None` when the preview is hidden.
    pub fn set_pane(&mut self, pane: Option<PaneRect>, viewport: Vec2) {
        self.pane = pane.filter(|pane| pane.size.x >= 1.0 && pane.size.y >= 1.0);
        self.viewport = viewport;
    }

    /// The preview rectangle.
    pub fn pane(&self) -> Option<PaneRect> {
        self.pane
    }

    /// The camera as drawn into the pane.
    pub fn pane_camera(&self) -> Option<RegionCamera<'_, OrbitCamera>> {
        self.pane.map(|pane| RegionCamera::new(&self.camera, self.viewport, pane.min, pane.size))
    }

    /// The inspected element nearest a logical window point, if any lies within the picking radius.
    pub fn pick(&self, point: Vec2) -> Option<usize> {
        let camera = self.pane_camera()?;
        let frame = camera.camera_frame(1.0);
        self.inspection.pick(point, frame.view_projection(), self.viewport)
    }

    /// Draw the frame: the preview through the engine renderer, or just the background while it is hidden.
    pub fn render(&mut self, target: &mut RenderTarget<'_>, time_seconds: f32) -> Result<(), String> {
        let renderer = target.renderer();
        let viewport = renderer.logical_size();
        self.viewport = viewport;
        let Some(pane) = self.pane else {
            let inputs = FrameInputs { clear_color: EDITOR_BACKGROUND, ..FrameInputs::default() };
            return target.draw(&RenderScene::new(), &self.camera, &inputs).map_err(|error| error.to_string());
        };

        // Shadows fit the framed content every frame, as the TypeScript preview's shadow camera did.
        let radius = self.camera.radius;
        let shadows = ShadowOptions {
            quality: self.lighting.quality,
            normal_bias: radius * 0.001,
            shadow_distance: self.camera.far_plane().min(self.camera.distance + (radius * 4.0)),
            ..renderer.shadow_options().unwrap_or_default()
        };
        renderer.set_shadow_options(Some(shadows));

        let basis = self.camera.basis();
        let marker_camera = MarkerCamera { basis, distance: self.camera.distance };
        let height = pane.size.y;
        let mut markers = match &self.data {
            Some(data) => point_batch(data, self.mode == ViewMode::Points, &marker_camera, self.point_size, height),
            None => Vec::new(),
        };
        markers.extend(self.inspection.markers(&marker_camera, radius, height));
        let markers = sort_transparent_triangles(&markers, basis.backward);

        let grid: &[ColorVertex] = if self.show_grid {
            let view = EditorGridView {
                target: self.camera.target.as_dvec3().to_array(),
                backward: basis.backward.as_dvec3().to_array(),
                distance: f64::from(self.camera.distance),
                viewport_height_pixels: f64::from(height),
                height: self.grid_height,
            };
            match self.grid.frame(&view) {
                Ok(frame) => {
                    self.grid_spacing = Some(frame.spacing);
                    &frame.triangles
                }
                Err(_) => &[],
            }
        } else {
            self.grid_spacing = None;
            &[]
        };

        let lighting = self.lighting;
        let light = CelestialLight {
            direction: Vec3::from_array(lighting.direction()),
            color: [lighting.color_r as f32, lighting.color_g as f32, lighting.color_b as f32],
            intensity: lighting.intensity as f32,
            angular_radius: LIGHT_ANGULAR_RADIUS,
            casts_shadow: lighting.casts_shadow,
        };
        let environment = studio_lighting(light);
        let draws = [TransparentDraw::Colored(&markers)];
        let shaded = self.mode == ViewMode::Shaded && self.data.is_some();
        let inputs = FrameInputs {
            environment: Some(&environment),
            ambient: Some(EDITOR_AMBIENT),
            clear_color: EDITOR_BACKGROUND,
            time_seconds,
            transparent_triangles: grid,
            transparent_draws: if markers.is_empty() { &[] } else { &draws },
            image_lighting: self.environment.as_ref().filter(|_| shaded).map(|map| ImageLighting {
                map,
                intensity: lighting.environment_intensity as f32,
                rotation: lighting.environment_rotation as f32,
            }),
            ..FrameInputs::default()
        };
        let camera = RegionCamera::new(&self.camera, viewport, pane.min, pane.size);
        target.draw(&self.scene, &camera, &inputs).map_err(|error| format!("Preview unavailable: {error}"))
    }
}

/// Packed world positions of geometry.
fn positions(data: &GeometryData) -> Vec<Vec3> {
    data.positions
        .as_chunks::<3>()
        .0
        .iter()
        .map(|point| Vec3::new(point[0] as f32, point[1] as f32, point[2] as f32))
        .collect()
}

/// The environment record for the studio: its light as the primary (and sun), no moon, skylight, sky, or fog.
fn studio_lighting(light: CelestialLight) -> EnvironmentLighting {
    EnvironmentLighting {
        elapsed_seconds: 0.0,
        time_of_day: 0.5,
        sun: light,
        moon: CelestialLight { intensity: 0.0, casts_shadow: false, ..light },
        primary: light,
        skylight_color: [0.0; 3],
        skylight_intensity: 0.0,
        wind_velocity: Vec3::ZERO,
        scattering: [1.0, 1.0, 1.0, 1.0],
        fog_and_clouds: [0.0; 4],
        cloud_shape_and_shafts: [0.0; 4],
        shaft_exposure_and_occlusion: [0.0; 3],
    }
}
