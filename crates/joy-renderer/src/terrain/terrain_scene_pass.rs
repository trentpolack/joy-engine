// Copyright (c) 2026 Trent Polack. All Rights Reserved.
// Licensed under the MIT License.

//! Scene terrain: the renderer's adapters for `RenderScene` terrain instances.
//!
//! Each live instance keeps one adapter with resident GPU resources; the asset stays shared. A frame prepares every
//! adapter before any pass opens (selection runs once per frame in the terrain's local space, for the camera), then
//! records the same indexed, continuously morphed surface into the shadow views whose volume it reaches, the depth
//! pre-pass, and the lit scene pass, which also writes the scene-effect attachments when effects need them. Adapters for
//! instances that leave the scene are released.

use std::collections::HashMap;
use std::collections::hash_map::Entry;

use joy_core::math::{
    DMat4,
    DVec3,
    Mat4,
};

use crate::GpuContext;
use crate::camera::CameraFrame;
use crate::culling::{
    bounds_outside_view,
    outside_frustum,
    world_bounds,
};
use crate::materials::{
    CasterBias,
    DEPTH_FORMAT,
    SHADOW_FORMAT,
    ScenePipelines,
    SceneTargets,
    scene_common,
};
use crate::mesh::Bounds;
use crate::scene::{
    RenderScene,
    TerrainInstance,
    TerrainInstanceId,
};
use crate::terrain::terrain_asset::TerrainError;
use crate::terrain::terrain_geometry::{
    ModelUniforms,
    TerrainGeometry,
    TerrainStats,
    grid_buffer_layout,
    terrain_bind_group_layout,
};
use crate::terrain::terrain_selection::TerrainView;
use crate::terrain::{
    TERRAIN_CASTER_SOURCE,
    TERRAIN_DEPTH_SOURCE,
    TERRAIN_SCENE_SOURCE,
    terrain_geometry_source,
};

/// Terrain surfaces are fully rough for screen-space reflections, as in TypeScript.
const TERRAIN_ROUGHNESS: f32 = 1.0;
/// The terrain group in the lit pipelines, after the scene pass's frame, shadow, and image-lighting groups.
const LIT_GROUP: u32 = 3;
/// The terrain group in the caster and depth pipelines, after their one pass group.
const GROUP: u32 = 1;

/// What the renderer needs from terrain preparation for culling statistics and shadow planning.
#[derive(Clone, Debug, Default)]
pub(crate) struct PreparedTerrain {
    /// Terrain instances inside the camera frustum.
    pub visible: usize,
    /// Terrain instances outside the camera frustum.
    pub culled: usize,
    /// World bounds of each shadow-casting terrain prepared this frame.
    pub caster_bounds: Vec<Bounds>,
}

/// Pipelines shared by every terrain adapter; compiled when the first terrain appears.
struct TerrainPipelines {
    layout: wgpu::BindGroupLayout,
    lit: wgpu::RenderPipeline,
    caster: wgpu::RenderPipeline,
    depth: wgpu::RenderPipeline,
    // The lit pipeline with scene-effect attachments; compiled when effects first need it.
    lit_targets: Option<wgpu::RenderPipeline>,
    lit_layout: wgpu::PipelineLayout,
    lit_source: String,
    scene_format: wgpu::TextureFormat,
}

/// One scene terrain instance's resident resources and transform history.
struct TerrainScenePass {
    geometry: TerrainGeometry,
    current_model: Mat4,
    previous_model: Option<Mat4>,
    world_bounds: Bounds,
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
        let live: Vec<(TerrainInstanceId, &TerrainInstance, Mat4)> = scene
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
            let world = world_bounds(bounds, model);
            if instance.casts_shadow {
                prepared.caster_bounds.push(world);
            }
            if !visible && !instance.casts_shadow {
                continue;
            }

            let pass = match self.passes.entry(id) {
                Entry::Occupied(entry) => entry.into_mut(),
                Entry::Vacant(entry) => entry.insert(TerrainScenePass::new(gpu, &pipelines.layout, instance)?),
            };
            let view = TerrainView {
                eye: camera.eye.as_dvec3(),
                projection_scale,
                tolerance: f64::from(instance.tolerance),
                target: camera.focus.as_dvec3(),
                target_radius: f64::from(instance.target_radius),
            };
            pass.prepare(gpu.queue(), encoder, view_projection, model, &view, instance)?;
            pass.world_bounds = world;
            pass.visible = visible;
            pass.casts_shadow = instance.casts_shadow;
            self.frame.push(id);
        }
        Ok(prepared)
    }

    /// Compile the lit pipeline that writes `targets` before a pass that draws with it opens.
    pub(crate) fn ensure_lit(&mut self, device: &wgpu::Device, targets: SceneTargets) {
        if let Some(pipelines) = &mut self.pipelines
            && targets == SceneTargets::ColorAndAttachments
            && pipelines.lit_targets.is_none()
        {
            pipelines.lit_targets = Some(pipelines.lit_pipeline(device, targets));
        }
    }

    /// Draw camera-visible terrain into the lit scene pass; returns draw calls and triangles.
    ///
    /// Expects the frame uniforms in group 0, the shadow receiver in group 1, and image lighting in group 2, and
    /// [`ensure_lit`](Self::ensure_lit) for `targets`.
    pub(crate) fn draw_lit(&self, pass: &mut wgpu::RenderPass<'_>, targets: SceneTargets) -> (usize, usize) {
        let Some(pipelines) = &self.pipelines else {
            return (0, 0);
        };
        let pipeline = match targets {
            SceneTargets::Color => &pipelines.lit,
            SceneTargets::ColorAndAttachments => pipelines.lit_targets.as_ref().expect("ensure_lit compiled it"),
        };
        let (mut draws, mut triangles) = (0, 0);
        for terrain in self.frame_passes().filter(|terrain| terrain.visible) {
            draws += terrain.geometry.draw(pass, pipeline, LIT_GROUP);
            triangles += terrain.geometry.stats().triangles;
        }
        (draws, triangles)
    }

    /// Draw camera-visible terrain into the depth pre-pass; returns draw calls. Expects the frame uniforms in group 0.
    pub(crate) fn draw_depth(&self, pass: &mut wgpu::RenderPass<'_>) -> usize {
        let Some(pipelines) = &self.pipelines else {
            return 0;
        };
        self.frame_passes()
            .filter(|terrain| terrain.visible)
            .map(|terrain| terrain.geometry.draw(pass, &pipelines.depth, GROUP))
            .sum()
    }

    /// Draw the shadow-casting terrain whose bounds reach `view_projection` (a directional cascade or point-light face)
    /// into that view; returns draw calls.
    ///
    /// Expects the renderer's caster uniforms in group 0 at this view's dynamic offset.
    pub(crate) fn draw_shadow(&self, pass: &mut wgpu::RenderPass<'_>, view_projection: Mat4) -> usize {
        let Some(pipelines) = &self.pipelines else {
            return 0;
        };
        self.frame_passes()
            .filter(|terrain| terrain.casts_shadow && !bounds_outside_view(terrain.world_bounds, view_projection))
            .map(|terrain| terrain.geometry.draw(pass, &pipelines.caster, GROUP))
            .sum()
    }

    /// Statistics of a terrain instance prepared this frame.
    pub(crate) fn stats(&self, id: TerrainInstanceId) -> Option<&TerrainStats> {
        self.frame.contains(&id).then(|| self.passes.get(&id)).flatten().map(|terrain| terrain.geometry.stats())
    }

    /// Adapters prepared this frame, in scene order.
    fn frame_passes(&self) -> impl Iterator<Item = &TerrainScenePass> {
        self.frame.iter().filter_map(|id| self.passes.get(id))
    }
}

impl TerrainScenePass {
    fn new(gpu: &GpuContext, layout: &wgpu::BindGroupLayout, instance: &TerrainInstance) -> Result<Self, TerrainError> {
        Ok(Self {
            geometry: TerrainGeometry::new(gpu, &instance.asset, layout)?,
            current_model: Mat4::IDENTITY,
            previous_model: None,
            world_bounds: crate::culling::EMPTY_BOUNDS,
            visible: false,
            casts_shadow: false,
        })
    }

    /// Select LODs once for the camera, in the terrain's local space, and upload the transform and its history.
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
        let previous = self.previous_model.unwrap_or(model);
        self.geometry.write_model(
            queue,
            &ModelUniforms { model: model.to_cols_array_2d(), previous_model: previous.to_cols_array_2d(), flags },
        );
        Ok(())
    }
}

impl TerrainPipelines {
    fn new(device: &wgpu::Device, scene: &ScenePipelines, scene_format: wgpu::TextureFormat, bias: CasterBias) -> Self {
        let layout = terrain_bind_group_layout(device);
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
        let depth_layout = pipeline_layout("terrain depth", &[Some(&scene.frame_layout), Some(&layout)]);
        let (lit_geometry, geometry) = (terrain_geometry_source(LIT_GROUP), terrain_geometry_source(GROUP));
        let common = scene_common(device);

        let caster = terrain_pipeline(
            device,
            "terrain caster",
            &format!("{geometry}\n{TERRAIN_CASTER_SOURCE}"),
            &caster_layout,
            None,
            &[],
            depth_state(
                SHADOW_FORMAT,
                wgpu::DepthBiasState { constant: bias.constant, slope_scale: bias.slope_scale, clamp: 0.0 },
            ),
        );
        let depth = terrain_pipeline(
            device,
            "terrain depth",
            &format!("{common}\n{geometry}\n{TERRAIN_DEPTH_SOURCE}"),
            &depth_layout,
            None,
            &[],
            depth_state(DEPTH_FORMAT, Default::default()),
        );
        let lit_source = format!("{common}\n{lit_geometry}\n{TERRAIN_SCENE_SOURCE}");
        let lit = lit_pipeline(device, &lit_source, &lit_layout, scene_format, SceneTargets::Color);
        Self { layout, lit, caster, depth, lit_targets: None, lit_layout, lit_source, scene_format }
    }

    /// The lit terrain pipeline writing `targets`.
    fn lit_pipeline(&self, device: &wgpu::Device, targets: SceneTargets) -> wgpu::RenderPipeline {
        lit_pipeline(device, &self.lit_source, &self.lit_layout, self.scene_format, targets)
    }
}

/// Compile the lit terrain pipeline for `targets`.
fn lit_pipeline(
    device: &wgpu::Device,
    source: &str,
    layout: &wgpu::PipelineLayout,
    scene_format: wgpu::TextureFormat,
    targets: SceneTargets,
) -> wgpu::RenderPipeline {
    terrain_pipeline(
        device,
        "terrain lit",
        source,
        layout,
        Some(targets.fragment_entry()),
        &targets.states(scene_format, true),
        depth_state(DEPTH_FORMAT, Default::default()),
    )
}

/// Less-or-equal depth with writes, like the scene's opaque pipelines.
fn depth_state(format: wgpu::TextureFormat, bias: wgpu::DepthBiasState) -> wgpu::DepthStencilState {
    wgpu::DepthStencilState {
        format,
        depth_write_enabled: Some(true),
        depth_compare: Some(wgpu::CompareFunction::LessEqual),
        stencil: Default::default(),
        bias,
    }
}

/// Compile one terrain pipeline; without a fragment entry it is depth-only.
fn terrain_pipeline(
    device: &wgpu::Device,
    label: &str,
    source: &str,
    layout: &wgpu::PipelineLayout,
    fragment_entry: Option<&str>,
    targets: &[Option<wgpu::ColorTargetState>],
    depth: wgpu::DepthStencilState,
) -> wgpu::RenderPipeline {
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    });
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
        fragment: fragment_entry.map(|entry| wgpu::FragmentState {
            module: &module,
            entry_point: Some(entry),
            compilation_options: Default::default(),
            targets,
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

#[cfg(test)]
#[path = "terrain_scene_pass.test.rs"]
mod tests;
