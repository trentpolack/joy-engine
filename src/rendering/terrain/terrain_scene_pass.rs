// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Scene terrain: the renderer's adapters for `RenderScene` terrain instances.
//!
//! Each live instance keeps one adapter with resident GPU resources; the asset stays shared. A frame prepares every
//! adapter before any pass opens (selection runs once per frame in the terrain's local space), then records the same
//! indexed, continuously morphed surface into the directional shadow map, the lit scene pass, and, when scene effects
//! run, the normal/roughness and motion attachments. Adapters for instances that leave the scene are released.

use std::collections::HashMap;
use std::collections::hash_map::Entry;

use bytemuck::{Pod, Zeroable};
use glam::{DMat4, DVec3, Mat4, Vec3};

use crate::rendering::GpuContext;
use crate::rendering::camera::CameraFrame;
use crate::rendering::materials::{CasterBias, DEPTH_FORMAT, SHADOW_FORMAT, ScenePipelines, scene_common};
use crate::rendering::mesh::Bounds;
use crate::rendering::scene::{RenderScene, TerrainInstance, TerrainInstanceId};
use crate::rendering::scene_effects::MotionUniforms;
use crate::rendering::terrain::terrain_asset::TerrainError;
use crate::rendering::terrain::terrain_renderer::{
    ModelUniforms, TerrainGeometry, grid_buffer_layout, terrain_bind_group_layout,
};
use crate::rendering::terrain::terrain_selection::TerrainView;
use crate::rendering::terrain::{
    TERRAIN_CASTER_SOURCE, TERRAIN_MOTION_SOURCE, TERRAIN_SCENE_SOURCE, TERRAIN_SURFACE_SOURCE, terrain_geometry_source,
};

/// Scene effect attachment formats (normals and motion), matching `scene_effects::attachments`.
const ATTACHMENT_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
/// Terrain surfaces are fully rough for screen-space reflections, as in TypeScript.
const TERRAIN_ROUGHNESS: f32 = 1.0;
/// The terrain group in the lit pipeline, after the scene pass's frame, shadow, and image-lighting groups.
const LIT_GROUP: u32 = 3;
/// The terrain group in the caster, surface, and motion pipelines, after their one pass group.
const GROUP: u32 = 1;

/// `TerrainMotionUniforms` in terrain-motion.wgsl. 336 bytes.
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable)]
struct TerrainMotionUniforms {
    current_view_projection: [[f32; 4]; 4],
    previous_view_projection: [[f32; 4]; 4],
    raster_view_projection: [[f32; 4]; 4],
    current_model: [[f32; 4]; 4],
    previous_model: [[f32; 4]; 4],
    jitter_delta: [f32; 4],
}

/// What the renderer needs from terrain preparation for culling statistics and shadow fitting.
#[derive(Clone, Copy, Debug, Default)]
pub(crate) struct PreparedTerrain {
    /// Terrain instances inside the camera frustum.
    pub visible: usize,
    /// Terrain instances outside the camera frustum.
    pub culled: usize,
    /// World bounds of shadow-casting terrain, for the directional shadow volume.
    pub caster_bounds: Option<Bounds>,
}

/// Pipelines shared by every terrain adapter; compiled when the first terrain appears.
struct TerrainPipelines {
    layout: wgpu::BindGroupLayout,
    motion_layout: wgpu::BindGroupLayout,
    lit: wgpu::RenderPipeline,
    caster: wgpu::RenderPipeline,
    surface: wgpu::RenderPipeline,
    motion: wgpu::RenderPipeline,
}

/// One scene terrain instance's resident resources and transform history.
struct TerrainScenePass {
    // Bind group before the buffer it references.
    motion_bind_group: wgpu::BindGroup,
    motion_uniforms: wgpu::Buffer,
    geometry: TerrainGeometry,
    current_model: Mat4,
    previous_model: Option<Mat4>,
    visible: bool,
    casts_shadow: bool,
}

/// The renderer's terrain adapters, keyed by scene instance.
pub(crate) struct SceneTerrain {
    passes: HashMap<TerrainInstanceId, TerrainScenePass>,
    /// Instances prepared this frame, in scene order.
    frame: Vec<TerrainInstanceId>,
    pipelines: Option<TerrainPipelines>,
    bias: CasterBias,
}

impl SceneTerrain {
    /// An empty collection whose caster pipeline will use `bias`.
    pub(crate) fn new(bias: CasterBias) -> Self {
        Self { passes: HashMap::new(), frame: Vec::new(), pipelines: None, bias }
    }

    /// Prepare every terrain instance in `scene` before any pass opens; records compute into `encoder`.
    ///
    /// `camera` is the stable (unjittered) frame camera and `logical_height` the viewport height in CSS pixels, so LOD
    /// quality does not depend on display density. Releases adapters for instances no longer in the scene. Fails before
    /// recording a pass for invalid instance settings or transforms.
    pub(crate) fn prepare(
        &mut self,
        gpu: &GpuContext,
        scene_pipelines: &ScenePipelines,
        encoder: &mut wgpu::CommandEncoder,
        scene: &RenderScene,
        camera: &CameraFrame,
        logical_height: f32,
    ) -> Result<PreparedTerrain, TerrainError> {
        self.frame.clear();
        let mut prepared = PreparedTerrain::default();
        // World transforms and visibility include every ancestor scene node.
        let live: Vec<(TerrainInstanceId, &TerrainInstance, glam::Mat4)> = scene
            .terrains()
            .filter_map(|(id, instance)| {
                let (model, visible) = scene.compose(instance.parent, instance.transform.matrix(), instance.visible);
                visible.then_some((id, instance, model))
            })
            .collect();
        self.passes.retain(|id, pass| {
            live.iter().any(|(live_id, instance, _)| live_id == id && instance.asset.ptr_eq(pass.geometry.asset()))
        });
        if live.is_empty() {
            return Ok(prepared);
        }
        let format = gpu.capabilities().scene_format;
        let bias = self.bias;
        let pipelines =
            self.pipelines.get_or_insert_with(|| TerrainPipelines::new(gpu.device(), scene_pipelines, format, bias));

        let view_projection = camera.view_projection();
        let projection_scale = f64::from(logical_height) * f64::from(camera.projection.y_axis.y.abs()) / 2.0;
        for (id, instance, model) in live {
            instance.validate()?;
            if !model.is_finite() {
                return Err(TerrainError::InvalidTransform);
            }
            let bounds = instance.asset.bounds();
            let visible = !outside_frustum(bounds, model, view_projection);
            if visible {
                prepared.visible += 1;
            } else {
                prepared.culled += 1;
            }
            if instance.casts_shadow {
                let world = world_bounds(bounds, model);
                prepared.caster_bounds = Some(prepared.caster_bounds.map_or(world, |total| union(total, world)));
            }
            if !visible && !instance.casts_shadow {
                continue;
            }

            let pass = match self.passes.entry(id) {
                Entry::Occupied(entry) => entry.into_mut(),
                Entry::Vacant(entry) => {
                    entry.insert(TerrainScenePass::new(gpu, &pipelines.layout, &pipelines.motion_layout, instance)?)
                }
            };
            let view = TerrainView {
                eye: camera.eye.as_dvec3(),
                projection_scale,
                tolerance: f64::from(instance.tolerance),
                target: camera.focus.as_dvec3(),
                target_radius: f64::from(instance.target_radius),
            };
            pass.prepare(gpu.queue(), encoder, view_projection, model, &view, instance)?;
            pass.visible = visible;
            pass.casts_shadow = instance.casts_shadow;
            self.frame.push(id);
        }
        Ok(prepared)
    }

    /// Draw camera-visible terrain into the lit scene pass; returns draw calls and triangles.
    ///
    /// Expects the frame uniforms in group 0, the shadow receiver in group 1, and image lighting in group 2.
    pub(crate) fn draw_lit(&self, pass: &mut wgpu::RenderPass<'_>) -> (usize, usize) {
        let Some(pipelines) = &self.pipelines else {
            return (0, 0);
        };
        let (mut draws, mut triangles) = (0, 0);
        for terrain in self.frame_passes().filter(|terrain| terrain.visible) {
            draws += terrain.geometry.draw(pass, &pipelines.lit, LIT_GROUP);
            triangles += terrain.geometry.stats().triangles;
        }
        (draws, triangles)
    }

    /// Draw shadow-casting terrain into one shadow view (a directional cascade or point-light face); returns draws.
    ///
    /// Expects the renderer's caster uniforms in group 0 at this view's dynamic offset.
    pub(crate) fn draw_shadow(&self, pass: &mut wgpu::RenderPass<'_>) -> usize {
        let Some(pipelines) = &self.pipelines else {
            return 0;
        };
        let mut draws = 0;
        for terrain in self.frame_passes().filter(|terrain| terrain.casts_shadow) {
            draws += terrain.geometry.draw(pass, &pipelines.caster, GROUP);
        }
        draws
    }

    /// Draw camera-visible terrain into the normal/roughness attachment. Expects the frame uniforms in group 0.
    pub(crate) fn draw_surface(&self, pass: &mut wgpu::RenderPass<'_>) {
        let Some(pipelines) = &self.pipelines else {
            return;
        };
        for terrain in self.frame_passes().filter(|terrain| terrain.visible) {
            terrain.geometry.draw(pass, &pipelines.surface, GROUP);
        }
    }

    /// Draw camera-visible terrain into the motion attachments with this frame's camera matrices.
    pub(crate) fn draw_motion(&self, queue: &wgpu::Queue, pass: &mut wgpu::RenderPass<'_>, camera: &MotionUniforms) {
        let Some(pipelines) = &self.pipelines else {
            return;
        };
        for terrain in self.frame_passes().filter(|terrain| terrain.visible) {
            let uniforms = TerrainMotionUniforms {
                current_view_projection: camera.current_view_projection,
                previous_view_projection: camera.previous_view_projection,
                raster_view_projection: camera.raster_view_projection,
                current_model: terrain.current_model.to_cols_array_2d(),
                previous_model: terrain.previous_model.unwrap_or(terrain.current_model).to_cols_array_2d(),
                jitter_delta: camera.velocity_jitter_delta,
            };
            queue.write_buffer(&terrain.motion_uniforms, 0, bytemuck::bytes_of(&uniforms));
            pass.set_bind_group(0, &terrain.motion_bind_group, &[]);
            terrain.geometry.draw(pass, &pipelines.motion, GROUP);
        }
    }

    /// Adapters prepared this frame, in scene order.
    fn frame_passes(&self) -> impl Iterator<Item = &TerrainScenePass> {
        self.frame.iter().filter_map(|id| self.passes.get(id))
    }
}

impl TerrainScenePass {
    fn new(
        gpu: &GpuContext,
        layout: &wgpu::BindGroupLayout,
        motion_layout: &wgpu::BindGroupLayout,
        instance: &TerrainInstance,
    ) -> Result<Self, TerrainError> {
        let device = gpu.device();
        let geometry = TerrainGeometry::new(gpu, &instance.asset, layout)?;
        let motion_uniforms = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("terrain motion uniforms"),
            size: std::mem::size_of::<TerrainMotionUniforms>() as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let motion_bind_group = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("terrain motion"),
            layout: motion_layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: motion_uniforms.as_entire_binding() }],
        });
        Ok(Self {
            motion_bind_group,
            motion_uniforms,
            geometry,
            current_model: Mat4::IDENTITY,
            previous_model: None,
            visible: false,
            casts_shadow: false,
        })
    }

    /// Select LODs once for the camera and light views, in the terrain's local space, and upload the transform.
    fn prepare(
        &mut self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        view_projection: Mat4,
        model: Mat4,
        view: &TerrainView,
        instance: &TerrainInstance,
    ) -> Result<(), TerrainError> {
        let local = local_view(model, view, instance);
        self.geometry.prepare(queue, encoder, view_projection, &local, instance.diagnostic)?;
        self.previous_model = Some(if self.previous_model.is_some() { self.current_model } else { model });
        self.current_model = model;
        let flags = [if instance.receives_shadow { 1.0 } else { 0.0 }, TERRAIN_ROUGHNESS, 0.0, 0.0];
        self.geometry.write_model(queue, &ModelUniforms { model: model.to_cols_array_2d(), flags });
        Ok(())
    }
}

impl TerrainPipelines {
    fn new(device: &wgpu::Device, scene: &ScenePipelines, scene_format: wgpu::TextureFormat, bias: CasterBias) -> Self {
        let layout = terrain_bind_group_layout(device);
        let motion_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("terrain motion"),
            entries: &[crate::rendering::materials::uniform_entry(0)],
        });
        let pipeline_layout = |label: &str, groups: &[Option<&wgpu::BindGroupLayout>]| {
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some(label),
                bind_group_layouts: groups,
                immediate_size: 0,
            })
        };
        // The lit pass shares the scene pass's groups 0-2; the others add the terrain group after their own.
        let lit_layout = pipeline_layout(
            "terrain lit",
            &[Some(&scene.frame_layout), Some(&scene.shadow_layout), Some(&scene.image_lighting_layout), Some(&layout)],
        );
        let caster_layout = pipeline_layout("terrain caster", &[Some(&scene.caster_layout), Some(&layout)]);
        let surface_layout = pipeline_layout("terrain surfaces", &[Some(&scene.frame_layout), Some(&layout)]);
        let motion_pipeline_layout = pipeline_layout("terrain motion", &[Some(&motion_layout), Some(&layout)]);
        let (lit_geometry, geometry) = (terrain_geometry_source(LIT_GROUP), terrain_geometry_source(GROUP));

        let depth = |format, write, compare, bias: wgpu::DepthBiasState| wgpu::DepthStencilState {
            format,
            depth_write_enabled: Some(write),
            depth_compare: Some(compare),
            stencil: Default::default(),
            bias,
        };
        let common = scene_common(device);
        let lit = terrain_pipeline(
            device,
            "terrain lit",
            &format!("{common}\n{lit_geometry}\n{TERRAIN_SCENE_SOURCE}"),
            &lit_layout,
            &[scene_format],
            depth(DEPTH_FORMAT, true, wgpu::CompareFunction::LessEqual, Default::default()),
        );
        let caster = terrain_pipeline(
            device,
            "terrain caster",
            &format!("{geometry}\n{TERRAIN_CASTER_SOURCE}"),
            &caster_layout,
            &[],
            depth(
                SHADOW_FORMAT,
                true,
                wgpu::CompareFunction::LessEqual,
                wgpu::DepthBiasState { constant: bias.constant, slope_scale: bias.slope_scale, clamp: 0.0 },
            ),
        );
        let surface = terrain_pipeline(
            device,
            "terrain surfaces",
            &format!("{common}\n{geometry}\n{TERRAIN_SURFACE_SOURCE}"),
            &surface_layout,
            &[ATTACHMENT_FORMAT],
            depth(DEPTH_FORMAT, false, wgpu::CompareFunction::Equal, Default::default()),
        );
        let motion = terrain_pipeline(
            device,
            "terrain motion",
            &format!("{geometry}\n{TERRAIN_MOTION_SOURCE}"),
            &motion_pipeline_layout,
            &[ATTACHMENT_FORMAT, ATTACHMENT_FORMAT],
            depth(DEPTH_FORMAT, true, wgpu::CompareFunction::LessEqual, Default::default()),
        );
        Self { layout, motion_layout, lit, caster, surface, motion }
    }
}

/// Compile one terrain pipeline; an empty `targets` list builds a depth-only pipeline without a fragment stage.
fn terrain_pipeline(
    device: &wgpu::Device,
    label: &str,
    source: &str,
    layout: &wgpu::PipelineLayout,
    targets: &[wgpu::TextureFormat],
    depth: wgpu::DepthStencilState,
) -> wgpu::RenderPipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
    let targets: Vec<Option<wgpu::ColorTargetState>> = targets
        .iter()
        .map(|format| Some(wgpu::ColorTargetState { format: *format, blend: None, write_mask: wgpu::ColorWrites::ALL }))
        .collect();
    let buffers = [Some(grid_buffer_layout())];
    device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(layout),
        vertex: wgpu::VertexState {
            module: &module,
            entry_point: Some("vertexMain"),
            compilation_options: Default::default(),
            buffers: &buffers,
        },
        primitive: wgpu::PrimitiveState { cull_mode: None, ..Default::default() },
        depth_stencil: Some(depth),
        multisample: wgpu::MultisampleState::default(),
        fragment: (!targets.is_empty()).then(|| wgpu::FragmentState {
            module: &module,
            entry_point: Some("fragmentMain"),
            compilation_options: Default::default(),
            targets: &targets,
        }),
        multiview_mask: None,
        cache: None,
    })
}

/// Express the world-space view in the terrain's local space.
///
/// The stretch bound keeps projected local errors conservative under non-uniform scale and shear. A collapsed transform
/// pins the finest geometry everywhere instead of inverting.
fn local_view(model: Mat4, view: &TerrainView, instance: &TerrainInstance) -> TerrainView {
    let model = model.as_dmat4();
    match invert_matrix(model) {
        Some(inverse) => TerrainView {
            eye: inverse.transform_point3(view.eye),
            target: inverse.transform_point3(view.target),
            projection_scale: view.projection_scale * stretch_bound(model) * stretch_bound(inverse),
            target_radius: view.target_radius * stretch_bound(inverse),
            ..*view
        },
        None => {
            let extent = &instance.asset.manifest().extent;
            TerrainView {
                eye: DVec3::ZERO,
                target: DVec3::ZERO,
                target_radius: extent[0].max(extent[1]) * 2.0,
                ..*view
            }
        }
    }
}

/// Gauss-Jordan inverse with partial pivoting, matching the TypeScript `invertMatrix` singularity threshold.
fn invert_matrix(matrix: DMat4) -> Option<DMat4> {
    let columns = matrix.to_cols_array();
    let mut rows = [[0.0_f64; 8]; 4];
    for (row, values) in rows.iter_mut().enumerate() {
        for column in 0..4 {
            values[column] = columns[(column * 4) + row];
            values[column + 4] = if row == column { 1.0 } else { 0.0 };
        }
    }
    for column in 0..4 {
        let pivot = (column..4).max_by(|a, b| rows[*a][column].abs().total_cmp(&rows[*b][column].abs()))?;
        let divisor = rows[pivot][column];
        if !divisor.is_finite() || divisor.abs() < 1e-15 {
            return None;
        }
        rows.swap(column, pivot);
        for value in rows[column].iter_mut() {
            *value /= divisor;
        }
        let pivot_row = rows[column];
        for (row, values) in rows.iter_mut().enumerate() {
            if row == column {
                continue;
            }
            let factor = values[column];
            for (value, pivot_value) in values.iter_mut().zip(pivot_row) {
                *value -= factor * pivot_value;
            }
        }
    }
    let mut inverse = [0.0; 16];
    for (row, values) in rows.iter().enumerate() {
        for column in 0..4 {
            inverse[(column * 4) + row] = values[column + 4];
        }
    }
    Some(DMat4::from_cols_array(&inverse))
}

/// `sqrt(||A||_1 * ||A||_inf)` of the linear part, an upper bound on the affine stretch.
fn stretch_bound(matrix: DMat4) -> f64 {
    let columns = matrix.to_cols_array();
    let mut rows = 0.0_f64;
    let mut sums = 0.0_f64;
    for axis in 0..3 {
        rows = rows.max(columns[axis].abs() + columns[axis + 4].abs() + columns[axis + 8].abs());
        sums = sums.max(columns[axis * 4].abs() + columns[(axis * 4) + 1].abs() + columns[(axis * 4) + 2].abs());
    }
    (rows * sums).sqrt()
}

/// Conservative clip-space test of transformed local bounds against the six frustum planes (zero-to-one depth).
fn outside_frustum(bounds: Bounds, model: Mat4, view_projection: Mat4) -> bool {
    let matrix = view_projection * model;
    let mut outside = [true; 6];
    for corner in bounds.corners() {
        let clip = matrix * corner.extend(1.0);
        let distances = [clip.x + clip.w, clip.w - clip.x, clip.y + clip.w, clip.w - clip.y, clip.z, clip.w - clip.z];
        for (plane, distance) in distances.iter().enumerate() {
            outside[plane] = outside[plane] && *distance < 0.0;
        }
    }
    outside.iter().any(|plane| *plane)
}

/// Transform local bounds into conservative world bounds.
fn world_bounds(bounds: Bounds, model: Mat4) -> Bounds {
    bounds.corners().iter().fold(
        Bounds { min: Vec3::splat(f32::INFINITY), max: Vec3::splat(f32::NEG_INFINITY) },
        |result, corner| {
            let point = model.transform_point3(*corner);
            Bounds { min: result.min.min(point), max: result.max.max(point) }
        },
    )
}

/// Smallest bounds containing both inputs.
fn union(left: Bounds, right: Bounds) -> Bounds {
    Bounds { min: left.min.min(right.min), max: left.max.max(right.max) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn local_views_keep_screen_error_invariant_under_uniform_scale() {
        // The TypeScript adapter test: scale 2, translation (8, 4, -6), eye (10, 8, -4).
        let model =
            Mat4::from_cols_array(&[2.0, 0.0, 0.0, 0.0, 0.0, 2.0, 0.0, 0.0, 0.0, 0.0, 2.0, 0.0, 8.0, 4.0, -6.0, 1.0]);
        let inverse = invert_matrix(model.as_dmat4()).expect("invertible");
        let eye = inverse.transform_point3(DVec3::new(10.0, 8.0, -4.0));
        assert_eq!(eye, DVec3::new(1.0, 2.0, 1.0));
        let scale = 100.0 * stretch_bound(model.as_dmat4()) * stretch_bound(inverse);
        assert!((scale - 100.0).abs() < 1e-12);
        assert!(invert_matrix(DMat4::ZERO).is_none());
    }
}
